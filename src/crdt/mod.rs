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
