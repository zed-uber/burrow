use super::Timestamp;
use serde::{Deserialize, Serialize};

/// Last-Write-Wins Register CRDT
/// Stores a value with a timestamp, automatically resolving conflicts by keeping the latest write
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LWWRegister<T> {
    value: T,
    timestamp: Timestamp,
}

impl<T: Clone> LWWRegister<T> {
    pub fn new(value: T, timestamp: Timestamp) -> Self {
        Self { value, timestamp }
    }

    /// Get the current value
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Get the timestamp
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Set a new value with a timestamp
    pub fn set(&mut self, value: T, timestamp: Timestamp) {
        // Only update if the new timestamp is greater
        if timestamp > self.timestamp {
            self.value = value;
            self.timestamp = timestamp;
        }
    }

    /// Merge with another LWWRegister, keeping the value with the latest timestamp
    pub fn merge(&mut self, other: &LWWRegister<T>) {
        if other.timestamp > self.timestamp {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PeerId;

    #[test]
    fn test_lww_register_merge() {
        let peer1 = PeerId::new();
        let peer2 = PeerId::new();

        let t1 = Timestamp::new(1000, 0, peer1);
        let t2 = Timestamp::new(2000, 0, peer2);

        let mut reg1 = LWWRegister::new("value1".to_string(), t1);
        let reg2 = LWWRegister::new("value2".to_string(), t2);

        reg1.merge(&reg2);

        assert_eq!(reg1.value(), "value2", "Should keep value with later timestamp");
    }

    #[test]
    fn test_lww_register_set() {
        let peer = PeerId::new();
        let t1 = Timestamp::new(1000, 0, peer);
        let t2 = Timestamp::new(500, 0, peer); // Earlier timestamp

        let mut reg = LWWRegister::new("value1".to_string(), t1);
        reg.set("value2".to_string(), t2);

        assert_eq!(reg.value(), "value1", "Should not update with earlier timestamp");
    }
}
