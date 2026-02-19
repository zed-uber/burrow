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

use super::Timestamp;
use crate::types::PeerId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Hybrid Logical Clock for causality tracking with physical time awareness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridLogicalClock {
    peer_id: PeerId,
    latest: Timestamp,
}

impl HybridLogicalClock {
    pub fn new(peer_id: PeerId) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            peer_id,
            latest: Timestamp::new(now, 0, peer_id),
        }
    }

    /// Generate a new timestamp for a local event
    pub fn tick(&mut self) -> Timestamp {
        let physical_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // If physical time has advanced, use it with logical = 0
        // Otherwise, keep physical time and increment logical
        if physical_now > self.latest.physical {
            self.latest = Timestamp::new(physical_now, 0, self.peer_id);
        } else {
            self.latest.logical += 1;
        }

        self.latest
    }

    /// Update clock when receiving a message with remote timestamp
    pub fn update(&mut self, remote: Timestamp) -> Timestamp {
        let physical_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Take the maximum of physical times
        let new_physical = physical_now.max(self.latest.physical).max(remote.physical);

        // If physical time advanced, reset logical to 0
        // Otherwise, increment the max logical time
        let new_logical = if new_physical > self.latest.physical.max(remote.physical) {
            0
        } else if self.latest.physical == remote.physical {
            self.latest.logical.max(remote.logical) + 1
        } else if self.latest.physical > remote.physical {
            self.latest.logical + 1
        } else {
            remote.logical + 1
        };

        self.latest = Timestamp::new(new_physical, new_logical, self.peer_id);
        self.latest
    }

    pub fn latest(&self) -> Timestamp {
        self.latest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PeerId;

    #[test]
    fn test_hlc_tick() {
        let peer_id = PeerId::new();
        let mut hlc = HybridLogicalClock::new(peer_id);

        let t1 = hlc.tick();
        let t2 = hlc.tick();

        assert!(t2 > t1, "Later tick should be greater");
    }

    #[test]
    fn test_hlc_update() {
        let peer1 = PeerId::new();
        let peer2 = PeerId::new();

        let mut hlc1 = HybridLogicalClock::new(peer1);
        let mut hlc2 = HybridLogicalClock::new(peer2);

        let t1 = hlc1.tick();
        let t2 = hlc2.update(t1);

        assert!(t2 > t1, "Update should produce greater timestamp");
    }
}
