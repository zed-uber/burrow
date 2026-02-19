use crate::crdt::{HybridLogicalClock, LWWRegister, ORSet};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use uuid::Uuid;

/// Peer identifier derived from libp2p PeerId (public key hash)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct PeerId(pub Uuid);

impl PeerId {
    /// Create a new random peer ID (for testing only)
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Create a peer ID from a libp2p PeerId by hashing it deterministically
    pub fn from_libp2p(peer_id: &libp2p::PeerId) -> Self {
        // Convert libp2p PeerId to bytes and hash to get a deterministic UUID
        let peer_bytes = peer_id.to_bytes();

        // Use the first 16 bytes of the peer_id as UUID bytes
        // If less than 16 bytes, pad with hash of remaining bytes
        let mut uuid_bytes = [0u8; 16];
        if peer_bytes.len() >= 16 {
            uuid_bytes.copy_from_slice(&peer_bytes[..16]);
        } else {
            uuid_bytes[..peer_bytes.len()].copy_from_slice(&peer_bytes);
        }

        Self(Uuid::from_bytes(uuid_bytes))
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

/// Channel type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    /// Direct message with one other peer
    PeerToPeer,
    /// Group channel with multiple peers
    Group,
}

/// Channel metadata with CRDT state for conflict-free replication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: LWWRegister<String>,       // Last-Write-Wins for channel name
    pub channel_type: ChannelType,
    pub members: ORSet<PeerId>,          // Observed-Remove Set for membership
    pub created_at: SystemTime,
    pub hlc: HybridLogicalClock,         // For generating timestamps
    // Encryption keys will be added in Phase 5
}

impl Channel {
    /// Create a new group channel with the creator as the first member
    pub fn new(name: String, creator: PeerId) -> Self {
        let mut hlc = HybridLogicalClock::new(creator);
        let timestamp = hlc.tick();

        let mut members = ORSet::new();
        members.add(creator);

        Self {
            id: ChannelId::new(),
            name: LWWRegister::new(name, timestamp),
            channel_type: ChannelType::Group,
            members,
            created_at: SystemTime::now(),
            hlc,
        }
    }

    /// Create a new peer-to-peer channel between two peers
    pub fn new_peer_to_peer(peer1: PeerId, peer2: PeerId) -> Self {
        // Name is the other peer's ID (will show nicely formatted in UI)
        let name = format!("@{}", peer2.0.simple());
        let mut hlc = HybridLogicalClock::new(peer1);
        let timestamp = hlc.tick();

        let mut members = ORSet::new();
        members.add(peer1);
        members.add(peer2);

        Self {
            id: ChannelId::new(),
            name: LWWRegister::new(name, timestamp),
            channel_type: ChannelType::PeerToPeer,
            members,
            created_at: SystemTime::now(),
            hlc,
        }
    }

    /// Create a placeholder channel (for received messages from unknown channels)
    pub fn placeholder(id: ChannelId, name: String, creator: PeerId) -> Self {
        let mut hlc = HybridLogicalClock::new(creator);
        let timestamp = hlc.tick();

        Self {
            id,
            name: LWWRegister::new(name, timestamp),
            channel_type: ChannelType::Group,
            members: ORSet::new(),  // Unknown members initially
            created_at: SystemTime::now(),
            hlc,
        }
    }

    /// Get the current channel name
    pub fn get_name(&self) -> &String {
        self.name.value()
    }

    /// Update the channel name
    pub fn set_name(&mut self, new_name: String) {
        let timestamp = self.hlc.tick();
        self.name.set(new_name, timestamp);
    }

    /// Add a member to the channel
    pub fn add_member(&mut self, peer_id: PeerId) -> Uuid {
        self.members.add(peer_id)
    }

    /// Remove a member from the channel
    pub fn remove_member(&mut self, peer_id: &PeerId) {
        self.members.remove(peer_id);
    }

    /// Get all members as a Vec
    pub fn get_members(&self) -> Vec<PeerId> {
        self.members.elements()
    }

    /// Merge another channel's state (for CRDT synchronization)
    pub fn merge(&mut self, other: &Channel) {
        self.name.merge(&other.name);
        self.members.merge(&other.members);
        // Update HLC with the remote timestamp
        let remote_ts = other.hlc.latest();
        self.hlc.update(remote_ts);
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
