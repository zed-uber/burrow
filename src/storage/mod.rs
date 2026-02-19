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
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to database")?;

        let storage = Self { pool };

        // Initialize schema
        storage.initialize_schema().await?;

        Ok(storage)
    }

    /// Initialize the database schema
    async fn initialize_schema(&self) -> Result<()> {
        let schema = include_str!("schema.sql");
        sqlx::query(schema)
            .execute(&self.pool)
            .await
            .context("Failed to initialize schema")?;

        // Run migrations for existing databases
        self.migrate_schema().await?;

        Ok(())
    }

    /// Migrate existing database schema to latest version
    async fn migrate_schema(&self) -> Result<()> {
        // Check if channels table has the new columns
        let table_info: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM pragma_table_info('channels') WHERE name IN ('channel_type', 'members')"
        )
        .fetch_all(&self.pool)
        .await?;

        // If we don't have 2 results, we need to migrate
        if table_info.len() < 2 {
            tracing::info!("Migrating database schema to add channel_type and members columns");

            // Add channel_type column if missing
            if !table_info.iter().any(|(name,)| name == "channel_type") {
                sqlx::query("ALTER TABLE channels ADD COLUMN channel_type TEXT NOT NULL DEFAULT 'Group'")
                    .execute(&self.pool)
                    .await
                    .context("Failed to add channel_type column")?;
            }

            // Add members column if missing
            if !table_info.iter().any(|(name,)| name == "members") {
                // Default to empty members list (empty bincode vec)
                let empty_members: Vec<PeerId> = Vec::new();
                let empty_members_bytes = bincode::serialize(&empty_members)?;

                sqlx::query("ALTER TABLE channels ADD COLUMN members BLOB NOT NULL DEFAULT X''")
                    .execute(&self.pool)
                    .await
                    .context("Failed to add members column")?;

                // Update all existing channels with empty members list
                sqlx::query("UPDATE channels SET members = ?")
                    .bind(&empty_members_bytes)
                    .execute(&self.pool)
                    .await
                    .context("Failed to set default members")?;
            }

            tracing::info!("Database migration completed");
        }

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

    /// Store a channel
    pub async fn store_channel(&self, channel: &Channel) -> Result<()> {
        let id_bytes = channel.id.0.as_bytes();
        let channel_type_str = match channel.channel_type {
            ChannelType::PeerToPeer => "PeerToPeer",
            ChannelType::Group => "Group",
        };
        let members_bytes = bincode::serialize(&channel.members)?;
        let created_at = channel
            .created_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        sqlx::query(
            r#"
            INSERT INTO channels (id, name, channel_type, members, created_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                channel_type = excluded.channel_type,
                members = excluded.members
            "#,
        )
        .bind(&id_bytes[..])
        .bind(&channel.name)
        .bind(channel_type_str)
        .bind(members_bytes)
        .bind(created_at)
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
            SELECT id, name, channel_type, members, created_at
            FROM channels
            WHERE id = ?
            "#,
        )
        .bind(&id_bytes[..])
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let id_bytes: Vec<u8> = row.get("id");
                let name: String = row.get("name");
                let channel_type_str: String = row.get("channel_type");
                let members_bytes: Vec<u8> = row.get("members");
                let created_at: i64 = row.get("created_at");

                let id = ChannelId(uuid::Uuid::from_slice(&id_bytes)?);
                let channel_type = match channel_type_str.as_str() {
                    "PeerToPeer" => ChannelType::PeerToPeer,
                    "Group" => ChannelType::Group,
                    _ => ChannelType::Group, // Default to group for unknown types
                };
                let members: Vec<PeerId> = bincode::deserialize(&members_bytes)?;
                let created_at = UNIX_EPOCH + std::time::Duration::from_secs(created_at as u64);

                Ok(Some(Channel {
                    id,
                    name,
                    channel_type,
                    members,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get all channels
    pub async fn get_all_channels(&self) -> Result<Vec<Channel>> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, channel_type, members, created_at
            FROM channels
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut channels = Vec::new();
        for row in rows {
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
            let members: Vec<PeerId> = bincode::deserialize(&members_bytes)?;
            let created_at = UNIX_EPOCH + std::time::Duration::from_secs(created_at as u64);

            channels.push(Channel {
                id,
                name,
                channel_type,
                members,
                created_at,
            });
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
        assert_eq!(retrieved.name, channel.name);

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
