use crate::types::PeerId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod hlc;
pub mod lww_register;
pub mod or_set;

pub use hlc::HybridLogicalClock;
pub use lww_register::LWWRegister;
pub use or_set::ORSet;

/// Timestamp combining physical and logical time
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp {
    /// Physical time (milliseconds since epoch)
    pub physical: u64,
    /// Logical counter for causality
    pub logical: u64,
    /// Peer ID for tie-breaking
    pub peer_id: PeerId,
}

impl Timestamp {
    pub fn new(physical: u64, logical: u64, peer_id: PeerId) -> Self {
        Self {
            physical,
            logical,
            peer_id,
        }
    }

    pub fn now(peer_id: PeerId) -> Self {
        let physical = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        Self::new(physical, 0, peer_id)
    }
}
