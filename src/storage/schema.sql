-- Channels table (must be first due to foreign key references)
CREATE TABLE IF NOT EXISTS channels (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,                         -- Cached name for display (extracted from CRDT)
    channel_type TEXT NOT NULL,                 -- "PeerToPeer" or "Group"
    members BLOB NOT NULL,                      -- Cached members for display (extracted from CRDT)
    created_at INTEGER NOT NULL,                -- Unix timestamp in seconds
    crdt_state BLOB                             -- Bincode serialized full Channel CRDT state
    -- Encryption keys will be added in Phase 5
);

-- Messages table
CREATE TABLE IF NOT EXISTS messages (
    id BLOB PRIMARY KEY NOT NULL,              -- MessageId (UUID)
    channel_id BLOB NOT NULL,                   -- ChannelId
    author BLOB NOT NULL,                       -- PeerId
    content TEXT NOT NULL,                      -- JSON serialized MessageContent
    vector_clock BLOB NOT NULL,                 -- Bincode serialized VectorClock
    lamport_timestamp INTEGER NOT NULL,
    parent_hashes BLOB NOT NULL,                -- Bincode serialized Vec<MessageId>
    created_at INTEGER NOT NULL                 -- Unix timestamp in seconds
);

CREATE INDEX IF NOT EXISTS idx_messages_channel_time
    ON messages(channel_id, created_at);

CREATE INDEX IF NOT EXISTS idx_messages_lamport
    ON messages(channel_id, lamport_timestamp);

-- Peer information (for later use)
CREATE TABLE IF NOT EXISTS peers (
    peer_id BLOB PRIMARY KEY NOT NULL,
    last_seen INTEGER NOT NULL,                 -- Unix timestamp in seconds
    metadata TEXT                               -- JSON metadata
);

-- Phase 5: Signal Protocol Storage Tables

-- Identity keys (our identity + known peer identities)
CREATE TABLE IF NOT EXISTS identity_keys (
    address TEXT PRIMARY KEY NOT NULL,          -- Peer address (PeerId string)
    identity_key BLOB NOT NULL,                 -- Serialized IdentityKey
    trust_level INTEGER NOT NULL DEFAULT 0      -- Trust level (0=untrusted, 1=trusted)
);

-- Pre-keys for X3DH
CREATE TABLE IF NOT EXISTS pre_keys (
    pre_key_id INTEGER PRIMARY KEY NOT NULL,
    record BLOB NOT NULL                        -- Serialized PreKeyRecord
);

-- Signed pre-keys
CREATE TABLE IF NOT EXISTS signed_pre_keys (
    signed_pre_key_id INTEGER PRIMARY KEY NOT NULL,
    record BLOB NOT NULL,                       -- Serialized SignedPreKeyRecord
    timestamp INTEGER NOT NULL                  -- Unix timestamp in seconds
);

-- Double Ratchet sessions
CREATE TABLE IF NOT EXISTS sessions (
    address TEXT NOT NULL,                      -- Peer address (PeerId string)
    device_id INTEGER NOT NULL,                 -- Device ID (we use 1 for P2P)
    record BLOB NOT NULL,                       -- Serialized SessionRecord
    PRIMARY KEY (address, device_id)
);

-- Sender keys for group encryption
CREATE TABLE IF NOT EXISTS sender_keys (
    address TEXT NOT NULL,                      -- Sender address
    distribution_id BLOB NOT NULL,              -- UUID for the distribution
    record BLOB NOT NULL,                       -- Serialized SenderKeyRecord
    PRIMARY KEY (address, distribution_id)
);
