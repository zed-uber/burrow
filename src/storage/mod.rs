// Copyright (C) 2026 Burrow Contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published
// by the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::types::{Channel, ChannelId, ChannelType, Message, MessageId, PeerId, VectorClock};
use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Storage layer for persisting messages and channels
pub struct Storage {
    pool: SqlitePool,
}

impl Storage {
    /// Create a new storage instance with the given database path
    pub async fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let path_str = db_path.as_ref().to_str().unwrap_or("");
        let is_memory = path_str == ":memory:";

        // For in-memory databases, use shared cache URI to ensure all connections see the same database
        let connect_str = if is_memory {
            "sqlite::memory:?cache=shared"
        } else {
            path_str
        };

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(connect_str)
            .await
            .context("Failed to connect to database")?;

        let storage = Self { pool };

        // Initialize schema
        storage.initialize_schema().await?;

        Ok(storage)
    }

    /// Initialize the database schema
    async fn initialize_schema(&self) -> Result<()> {
        // Use a single connection for all schema operations to ensure they see each other's changes
        let mut conn = self.pool.acquire().await?;

        // Create channels table first (referenced by messages)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS channels (
                id BLOB PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                channel_type TEXT NOT NULL,
                members BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                crdt_state BLOB
            )
            "#
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create channels table")?;

        // Create messages table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id BLOB PRIMARY KEY NOT NULL,
                channel_id BLOB NOT NULL,
                author BLOB NOT NULL,
                content TEXT NOT NULL,
                vector_clock BLOB NOT NULL,
                lamport_timestamp INTEGER NOT NULL,
                parent_hashes BLOB NOT NULL,
                created_at INTEGER NOT NULL
            )
            "#
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create messages table")?;

        // Create indexes
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_channel_time ON messages(channel_id, created_at)"
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create messages channel time index")?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_lamport ON messages(channel_id, lamport_timestamp)"
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create messages lamport index")?;

        // Create peers table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS peers (
                peer_id BLOB PRIMARY KEY NOT NULL,
                last_seen INTEGER NOT NULL,
                metadata TEXT
            )
            "#
        )
        .execute(&mut *conn)
        .await
        .context("Failed to create peers table")?;

        // Release connection before running migrations
        drop(conn);

        // Run migrations for existing databases
        self.migrate_schema().await?;

        Ok(())
    }

    /// Migrate existing database schema to latest version
    async fn migrate_schema(&self) -> Result<()> {
        // Phase 4: Schema migrations disabled - schema.sql now contains all required columns
        // For production databases from earlier phases, manual migration will be needed
        Ok(())
    }

    /// Store a message
    pub async fn store_message(&self, message: &Message) -> Result<()> {
        let id_bytes = message.id.0.as_bytes();
        let channel_id_bytes = message.channel_id.0.as_bytes();
        let author_bytes = message.author.0.as_bytes();
        let content_json = serde_json::to_string(&message.content)?;
        let vector_clock_bytes = bincode::serialize(&message.vector_clock)?;
        let parent_hashes_bytes = bincode::serialize(&message.parent_hashes)?;
        let created_at = message
            .created_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        sqlx::query(
            r#"
            INSERT INTO messages (id, channel_id, author, content, vector_clock, lamport_timestamp, parent_hashes, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id_bytes[..])
        .bind(&channel_id_bytes[..])
        .bind(&author_bytes[..])
        .bind(content_json)
        .bind(vector_clock_bytes)
        .bind(message.lamport_timestamp as i64)
        .bind(parent_hashes_bytes)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .context("Failed to store message")?;

        Ok(())
    }

    /// Get a message by ID
    pub async fn get_message(&self, message_id: MessageId) -> Result<Option<Message>> {
        let id_bytes = message_id.0.as_bytes();

        let row = sqlx::query(
            r#"
            SELECT id, channel_id, author, content, vector_clock, lamport_timestamp, parent_hashes, created_at
            FROM messages
            WHERE id = ?
            "#,
        )
        .bind(&id_bytes[..])
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let message = self.row_to_message(row)?;
                Ok(Some(message))
            }
            None => Ok(None),
        }
    }

    /// Get all messages for a channel, ordered by creation time
    pub async fn get_channel_messages(&self, channel_id: ChannelId) -> Result<Vec<Message>> {
        let channel_id_bytes = channel_id.0.as_bytes();

        let rows = sqlx::query(
            r#"
            SELECT id, channel_id, author, content, vector_clock, lamport_timestamp, parent_hashes, created_at
            FROM messages
            WHERE channel_id = ?
            ORDER BY created_at ASC, lamport_timestamp ASC
            "#,
        )
        .bind(&channel_id_bytes[..])
        .fetch_all(&self.pool)
        .await?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(self.row_to_message(row)?);
        }

        Ok(messages)
    }

    /// Helper to convert a database row to a Message
    fn row_to_message(&self, row: sqlx::sqlite::SqliteRow) -> Result<Message> {
        let id_bytes: Vec<u8> = row.get("id");
        let channel_id_bytes: Vec<u8> = row.get("channel_id");
        let author_bytes: Vec<u8> = row.get("author");
        let content_json: String = row.get("content");
        let vector_clock_bytes: Vec<u8> = row.get("vector_clock");
        let lamport_timestamp: i64 = row.get("lamport_timestamp");
        let parent_hashes_bytes: Vec<u8> = row.get("parent_hashes");
        let created_at: i64 = row.get("created_at");

        let id = MessageId(uuid::Uuid::from_slice(&id_bytes)?);
        let channel_id = ChannelId(uuid::Uuid::from_slice(&channel_id_bytes)?);
        let author = PeerId(uuid::Uuid::from_slice(&author_bytes)?);
        let content = serde_json::from_str(&content_json)?;
        let vector_clock: VectorClock = bincode::deserialize(&vector_clock_bytes)?;
        let parent_hashes: Vec<MessageId> = bincode::deserialize(&parent_hashes_bytes)?;
        let created_at = UNIX_EPOCH + std::time::Duration::from_secs(created_at as u64);

        Ok(Message {
            id,
            channel_id,
            author,
            content,
            vector_clock,
            lamport_timestamp: lamport_timestamp as u64,
            parent_hashes,
            created_at,
        })
    }

    /// Store a channel with CRDT state
    pub async fn store_channel(&self, channel: &Channel) -> Result<()> {
        let id_bytes = channel.id.0.as_bytes();
        let channel_type_str = match channel.channel_type {
            ChannelType::PeerToPeer => "PeerToPeer",
            ChannelType::Group => "Group",
        };

        // Cache the name and members for quick display
        let name = channel.get_name().clone();
        let members = channel.get_members();
        let members_bytes = bincode::serialize(&members)?;

        // Serialize the full CRDT state
        let crdt_state = bincode::serialize(channel)?;

        let created_at = channel
            .created_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        sqlx::query(
            r#"
            INSERT INTO channels (id, name, channel_type, members, created_at, crdt_state)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                channel_type = excluded.channel_type,
                members = excluded.members,
                crdt_state = excluded.crdt_state
            "#,
        )
        .bind(&id_bytes[..])
        .bind(name)
        .bind(channel_type_str)
        .bind(members_bytes)
        .bind(created_at)
        .bind(crdt_state)
        .execute(&self.pool)
        .await
        .context("Failed to store channel")?;

        Ok(())
    }

    /// Get a channel by ID
    pub async fn get_channel(&self, channel_id: ChannelId) -> Result<Option<Channel>> {
        let id_bytes = channel_id.0.as_bytes();

        let row = sqlx::query(
            r#"
            SELECT id, name, channel_type, members, created_at, crdt_state
            FROM channels
            WHERE id = ?
            "#,
        )
        .bind(&id_bytes[..])
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                // Try to deserialize from crdt_state first (Phase 3+)
                let crdt_state_bytes: Option<Vec<u8>> = row.try_get("crdt_state").ok().flatten();

                if let Some(state_bytes) = crdt_state_bytes {
                    if let Ok(channel) = bincode::deserialize::<Channel>(&state_bytes) {
                        return Ok(Some(channel));
                    }
                }

                // Fall back to old format (Phase 2) - reconstruct Channel with CRDTs
                let id_bytes: Vec<u8> = row.get("id");
                let name: String = row.get("name");
                let channel_type_str: String = row.get("channel_type");
                let members_bytes: Vec<u8> = row.get("members");
                let created_at: i64 = row.get("created_at");

                let id = ChannelId(uuid::Uuid::from_slice(&id_bytes)?);
                let channel_type = match channel_type_str.as_str() {
                    "PeerToPeer" => ChannelType::PeerToPeer,
                    "Group" => ChannelType::Group,
                    _ => ChannelType::Group,
                };
                let old_members: Vec<PeerId> = bincode::deserialize(&members_bytes)?;
                let created_at = UNIX_EPOCH + std::time::Duration::from_secs(created_at as u64);

                // Create a new channel with CRDT state from old data
                // Use first member as creator, or generate a placeholder peer
                let creator = old_members.first().copied().unwrap_or_else(PeerId::new);
                let mut channel = Channel::placeholder(id, name, creator);
                channel.channel_type = channel_type;
                channel.created_at = created_at;

                // Add all members to the ORSet
                for member in old_members {
                    channel.add_member(member);
                }

                Ok(Some(channel))
            }
            None => Ok(None),
        }
    }

    /// Get all channels
    pub async fn get_all_channels(&self) -> Result<Vec<Channel>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, channel_type, members, created_at, crdt_state
            FROM channels
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
            // Try to deserialize from crdt_state first (Phase 3+)
            let crdt_state_bytes: Option<Vec<u8>> = row.try_get("crdt_state").ok().flatten();

            if let Some(state_bytes) = crdt_state_bytes {
                if let Ok(channel) = bincode::deserialize::<Channel>(&state_bytes) {
                    channels.push(channel);
                    continue;
                }
            }

            // Fall back to old format (Phase 2) - reconstruct Channel with CRDTs
            let id_bytes: Vec<u8> = row.get("id");
            let name: String = row.get("name");
            let channel_type_str: String = row.get("channel_type");
            let members_bytes: Vec<u8> = row.get("members");
            let created_at: i64 = row.get("created_at");

            let id = ChannelId(uuid::Uuid::from_slice(&id_bytes)?);
            let channel_type = match channel_type_str.as_str() {
                "PeerToPeer" => ChannelType::PeerToPeer,
                "Group" => ChannelType::Group,
                _ => ChannelType::Group,
            };
            let old_members: Vec<PeerId> = bincode::deserialize(&members_bytes)?;
            let created_at = UNIX_EPOCH + std::time::Duration::from_secs(created_at as u64);

            // Create a new channel with CRDT state from old data
            let creator = old_members.first().copied().unwrap_or_else(PeerId::new);
            let mut channel = Channel::placeholder(id, name, creator);
            channel.channel_type = channel_type;
            channel.created_at = created_at;

            // Add all members to the ORSet
            for member in old_members {
                channel.add_member(member);
            }

            channels.push(channel);
        }

        Ok(channels)
    }

    /// Delete a channel and all its messages
    pub async fn delete_channel(&self, channel_id: ChannelId) -> Result<()> {
        let id_bytes = channel_id.0.as_bytes();

        // Delete messages first
        sqlx::query("DELETE FROM messages WHERE channel_id = ?")
            .bind(&id_bytes[..])
            .execute(&self.pool)
            .await?;

        // Delete channel
        sqlx::query("DELETE FROM channels WHERE id = ?")
            .bind(&id_bytes[..])
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    // Phase 4: DAG-specific query methods

    /// Get messages by a list of IDs (for DAG synchronization)
    pub async fn get_messages_by_ids(&self, message_ids: &[MessageId]) -> Result<Vec<Message>> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut messages = Vec::new();
        for message_id in message_ids {
            if let Some(message) = self.get_message(*message_id).await? {
                messages.push(message);
            }
        }

        Ok(messages)
    }

    /// Check if a message exists
    pub async fn has_message(&self, message_id: MessageId) -> Result<bool> {
        let id_bytes = message_id.0.as_bytes();

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE id = ?"
        )
        .bind(&id_bytes[..])
        .fetch_one(&self.pool)
        .await?;

        Ok(count > 0)
    }

    /// Get all message IDs for a channel (for inventory)
    pub async fn get_channel_message_ids(&self, channel_id: ChannelId) -> Result<Vec<MessageId>> {
        let channel_id_bytes = channel_id.0.as_bytes();

        let rows = sqlx::query(
            "SELECT id FROM messages WHERE channel_id = ?"
        )
        .bind(&channel_id_bytes[..])
        .fetch_all(&self.pool)
        .await?;

        let mut ids = Vec::new();
        for row in rows {
            let id_bytes: Vec<u8> = row.get("id");
            let id = MessageId(uuid::Uuid::from_slice(&id_bytes)?);
            ids.push(id);
        }

        Ok(ids)
    }

    /// Store multiple messages efficiently (for bulk DAG sync)
    pub async fn store_messages(&self, messages: &[Message]) -> Result<()> {
        for message in messages {
            // Use INSERT OR IGNORE to skip duplicates
            let id_bytes = message.id.0.as_bytes();
            let channel_id_bytes = message.channel_id.0.as_bytes();
            let author_bytes = message.author.0.as_bytes();
            let content_json = serde_json::to_string(&message.content)?;
            let vector_clock_bytes = bincode::serialize(&message.vector_clock)?;
            let parent_hashes_bytes = bincode::serialize(&message.parent_hashes)?;
            let created_at = message
                .created_at
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            sqlx::query(
                r#"
                INSERT OR IGNORE INTO messages (id, channel_id, author, content, vector_clock, lamport_timestamp, parent_hashes, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id_bytes[..])
            .bind(&channel_id_bytes[..])
            .bind(&author_bytes[..])
            .bind(content_json)
            .bind(vector_clock_bytes)
            .bind(message.lamport_timestamp as i64)
            .bind(parent_hashes_bytes)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .context("Failed to store message")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MessageContent, VectorClock};

    #[tokio::test]
    async fn test_channel_crud() {
        let storage = Storage::new(":memory:").await.unwrap();

        let peer_id = PeerId::new();
        let channel = Channel::new("test-channel".to_string(), peer_id);
        storage.store_channel(&channel).await.unwrap();

        let retrieved = storage.get_channel(channel.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, channel.id);
        assert_eq!(retrieved.get_name(), channel.get_name());

        let all_channels = storage.get_all_channels().await.unwrap();
        assert_eq!(all_channels.len(), 1);
    }

    #[tokio::test]
    async fn test_message_crud() {
        let storage = Storage::new(":memory:").await.unwrap();

        let peer_id = PeerId::new();
        let channel = Channel::new("test-channel".to_string(), peer_id);
        storage.store_channel(&channel).await.unwrap();
        let mut vector_clock = VectorClock::new();
        vector_clock.increment(peer_id);

        let message = Message::new(
            channel.id,
            peer_id,
            MessageContent {
                text: "Hello, world!".to_string(),
            },
            vector_clock,
            1,
        );

        storage.store_message(&message).await.unwrap();

        let retrieved = storage.get_message(message.id).await.unwrap().unwrap();
        assert_eq!(retrieved.id, message.id);
        assert_eq!(retrieved.content.text, "Hello, world!");

        let channel_messages = storage.get_channel_messages(channel.id).await.unwrap();
        assert_eq!(channel_messages.len(), 1);
    }
}
