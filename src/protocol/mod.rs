use crate::types::{Channel, ChannelId, Message, MessageId, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Network protocol messages exchanged between peers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// A chat message to be broadcast to channel members
    ChatMessage(Message),

    /// Request to sync messages for a channel
    SyncRequest {
        channel_id: ChannelId,
        since_timestamp: u64,
    },

    /// Response with messages for sync
    SyncResponse {
        channel_id: ChannelId,
        messages: Vec<Message>,
    },

    /// Peer announcement (when connecting)
    PeerAnnounce {
        peer_id: PeerId,
        listen_addresses: Vec<String>,
    },

    /// Channel announcement - broadcast when creating a new channel
    ChannelAnnounce {
        channel: Channel,
    },

    /// Request full CRDT state for a channel
    ChannelStateRequest {
        channel_id: ChannelId,
    },

    /// Response with full CRDT state for a channel
    ChannelStateResponse {
        channel: Channel,
    },

    /// Incremental CRDT update for a channel (name change, member add/remove)
    ChannelUpdate {
        channel: Channel,
    },

    // Phase 4: DAG Synchronization Messages

    /// Request specific messages by ID (to fill DAG gaps)
    MessageRequest {
        channel_id: ChannelId,
        message_ids: Vec<MessageId>,
    },

    /// Response with requested messages
    MessageResponse {
        channel_id: ChannelId,
        messages: Vec<Message>,
    },

    /// Anti-entropy: announce which messages we have for a channel
    /// Peers can use this to detect missing messages
    MessageInventory {
        channel_id: ChannelId,
        message_ids: HashSet<MessageId>,
    },

    /// Request message inventory from peers for anti-entropy
    InventoryRequest {
        channel_id: ChannelId,
    },
}

impl NetworkMessage {
    /// Serialize to bytes for network transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bytes received from network
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}
