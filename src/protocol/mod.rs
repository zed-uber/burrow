use crate::types::{Channel, ChannelId, Message, PeerId};
use serde::{Deserialize, Serialize};

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
