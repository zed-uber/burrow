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
