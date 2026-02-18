-- Messages table
CREATE TABLE IF NOT EXISTS messages (
    id BLOB PRIMARY KEY NOT NULL,              -- MessageId (UUID)
    channel_id BLOB NOT NULL,                   -- ChannelId
    author BLOB NOT NULL,                       -- PeerId
    content TEXT NOT NULL,                      -- JSON serialized MessageContent
    vector_clock BLOB NOT NULL,                 -- Bincode serialized VectorClock
    lamport_timestamp INTEGER NOT NULL,
    parent_hashes BLOB NOT NULL,                -- Bincode serialized Vec<MessageId>
    created_at INTEGER NOT NULL,                -- Unix timestamp in seconds

    -- Indexes for efficient querying
    FOREIGN KEY (channel_id) REFERENCES channels(id)
);

CREATE INDEX IF NOT EXISTS idx_messages_channel_time
    ON messages(channel_id, created_at);

CREATE INDEX IF NOT EXISTS idx_messages_lamport
    ON messages(channel_id, lamport_timestamp);

-- Channels table
CREATE TABLE IF NOT EXISTS channels (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL                 -- Unix timestamp in seconds
    -- CRDT state will be added in Phase 3
    -- Encryption keys will be added in Phase 5
);

-- Peer information (for later use)
CREATE TABLE IF NOT EXISTS peers (
    peer_id BLOB PRIMARY KEY NOT NULL,
    last_seen INTEGER NOT NULL,                 -- Unix timestamp in seconds
    -- Public key and trust level will be added in Phase 5
    metadata TEXT                               -- JSON metadata
);
