use libp2p::{Multiaddr, PeerId};
use std::collections::HashMap;
use std::time::SystemTime;

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub addresses: Vec<Multiaddr>,
    pub connected_at: SystemTime,
    pub last_seen: SystemTime,
}

/// Peer manager tracking connected peers
#[derive(Debug, Default)]
pub struct PeerManager {
    peers: HashMap<PeerId, PeerInfo>,
}

impl PeerManager {
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
        }
    }

    /// Add a new peer
    pub fn add_peer(&mut self, peer_id: PeerId, address: Option<Multiaddr>) {
        let now = SystemTime::now();
        let addresses = address.into_iter().collect();

        self.peers.insert(
            peer_id,
            PeerInfo {
                peer_id,
                addresses,
                connected_at: now,
                last_seen: now,
            },
        );
    }

    /// Remove a peer
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
    }

    /// Update last seen time for a peer
    pub fn update_last_seen(&mut self, peer_id: &PeerId) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.last_seen = SystemTime::now();
        }
    }

    /// Get peer info
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Get all connected peers
    pub fn get_all_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Count of connected peers
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }
}
