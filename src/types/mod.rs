use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use uuid::Uuid;

/// Peer identifier (Ed25519 public key will be used later)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct PeerId(pub Uuid);

impl PeerId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for PeerId {
    fn default() -> Self {
        Self::new()
    }
}

/// Channel identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub Uuid);

impl ChannelId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ChannelId {
    fn default() -> Self {
        Self::new()
    }
}

/// Message identifier (time-ordered UUID v7)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct MessageId(pub Uuid);

impl MessageId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

/// Vector clock for causal ordering
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VectorClock {
    pub clocks: HashMap<PeerId, u64>,
}

impl VectorClock {
    pub fn new() -> Self {
        Self {
            clocks: HashMap::new(),
        }
    }

    /// Increment the clock for a given peer
    pub fn increment(&mut self, peer_id: PeerId) {
        let counter = self.clocks.entry(peer_id).or_insert(0);
        *counter += 1;
    }

    /// Get the clock value for a peer
    pub fn get(&self, peer_id: &PeerId) -> u64 {
        self.clocks.get(peer_id).copied().unwrap_or(0)
    }

    /// Merge two vector clocks (take max of each peer's clock)
    pub fn merge(&mut self, other: &VectorClock) {
        for (peer_id, &clock) in &other.clocks {
            let entry = self.clocks.entry(*peer_id).or_insert(0);
            *entry = (*entry).max(clock);
        }
    }

    /// Check if this clock happened before another (strict partial order)
    pub fn happened_before(&self, other: &VectorClock) -> bool {
        let mut strictly_less = false;

        // Check all peers in self
        for (peer_id, &self_clock) in &self.clocks {
            let other_clock = other.get(peer_id);
            if self_clock > other_clock {
                return false; // Not happened-before
            }
            if self_clock < other_clock {
                strictly_less = true;
            }
        }

        // Check peers only in other
        for (peer_id, &other_clock) in &other.clocks {
            if !self.clocks.contains_key(peer_id) && other_clock > 0 {
                strictly_less = true;
            }
        }

        strictly_less
    }

    /// Check if two clocks are concurrent (neither happened before the other)
    pub fn concurrent(&self, other: &VectorClock) -> bool {
        !self.happened_before(other) && !other.happened_before(self)
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Message content (plaintext for Phase 1, will be encrypted later)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContent {
    pub text: String,
}

/// A message with causal ordering metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub author: PeerId,
    pub content: MessageContent,
    pub vector_clock: VectorClock,
    pub lamport_timestamp: u64,
    pub parent_hashes: Vec<MessageId>, // For DAG structure (Phase 4)
    pub created_at: SystemTime,
    // Signature will be added in Phase 5
}

impl Message {
    pub fn new(
        channel_id: ChannelId,
        author: PeerId,
        content: MessageContent,
        vector_clock: VectorClock,
        lamport_timestamp: u64,
    ) -> Self {
        Self {
            id: MessageId::new(),
            channel_id,
            author,
            content,
            vector_clock,
            lamport_timestamp,
            parent_hashes: Vec::new(),
            created_at: SystemTime::now(),
        }
    }
}

/// Channel metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub created_at: SystemTime,
    // CRDT members will be added in Phase 3
    // Encryption keys will be added in Phase 5
}

impl Channel {
    pub fn new(name: String) -> Self {
        Self {
            id: ChannelId::new(),
            name,
            created_at: SystemTime::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_happened_before() {
        let mut vc1 = VectorClock::new();
        let mut vc2 = VectorClock::new();

        let peer1 = PeerId::new();
        let peer2 = PeerId::new();

        vc1.increment(peer1);
        vc2.clocks = vc1.clocks.clone();
        vc2.increment(peer2);

        assert!(vc1.happened_before(&vc2));
        assert!(!vc2.happened_before(&vc1));
    }

    #[test]
    fn test_vector_clock_concurrent() {
        let mut vc1 = VectorClock::new();
        let mut vc2 = VectorClock::new();

        let peer1 = PeerId::new();
        let peer2 = PeerId::new();

        vc1.increment(peer1);
        vc2.increment(peer2);

        assert!(vc1.concurrent(&vc2));
        assert!(vc2.concurrent(&vc1));
    }

    #[test]
    fn test_vector_clock_merge() {
        let mut vc1 = VectorClock::new();
        let mut vc2 = VectorClock::new();

        let peer1 = PeerId::new();
        let peer2 = PeerId::new();

        vc1.increment(peer1);
        vc1.increment(peer1);
        vc2.increment(peer2);
        vc2.increment(peer2);
        vc2.increment(peer2);

        vc1.merge(&vc2);

        assert_eq!(vc1.get(&peer1), 2);
        assert_eq!(vc1.get(&peer2), 3);
    }
}
