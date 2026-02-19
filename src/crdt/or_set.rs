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

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use uuid::Uuid;

/// Observed-Remove Set CRDT
/// Adds and removes are conflict-free. An element is in the set if it has been
/// added but not all of its add tags have been removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ORSet<T: Eq + Hash + Clone> {
    /// Maps elements to their unique add tags
    elements: HashMap<T, HashSet<Uuid>>,
}

impl<T: Eq + Hash + Clone> ORSet<T> {
    pub fn new() -> Self {
        Self {
            elements: HashMap::new(),
        }
    }

    /// Add an element to the set with a unique tag
    pub fn add(&mut self, element: T) -> Uuid {
        let tag = Uuid::now_v7();
        self.elements
            .entry(element)
            .or_insert_with(HashSet::new)
            .insert(tag);
        tag
    }

    /// Remove an element by removing all its tags
    pub fn remove(&mut self, element: &T) {
        self.elements.remove(element);
    }

    /// Remove an element with a specific tag (for precise removal in merges)
    pub fn remove_tag(&mut self, element: &T, tag: Uuid) {
        if let Some(tags) = self.elements.get_mut(element) {
            tags.remove(&tag);
            if tags.is_empty() {
                self.elements.remove(element);
            }
        }
    }

    /// Check if an element is in the set
    pub fn contains(&self, element: &T) -> bool {
        self.elements
            .get(element)
            .map(|tags| !tags.is_empty())
            .unwrap_or(false)
    }

    /// Get all elements in the set
    pub fn elements(&self) -> Vec<T> {
        self.elements
            .iter()
            .filter(|(_, tags)| !tags.is_empty())
            .map(|(elem, _)| elem.clone())
            .collect()
    }

    /// Get tags for an element
    pub fn tags(&self, element: &T) -> Option<&HashSet<Uuid>> {
        self.elements.get(element)
    }

    /// Merge with another OR-Set
    pub fn merge(&mut self, other: &ORSet<T>) {
        for (element, other_tags) in &other.elements {
            let tags = self.elements.entry(element.clone()).or_insert_with(HashSet::new);
            tags.extend(other_tags);
        }
    }

    /// Get the number of elements
    pub fn len(&self) -> usize {
        self.elements
            .iter()
            .filter(|(_, tags)| !tags.is_empty())
            .count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T: Eq + Hash + Clone> Default for ORSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_or_set_add_remove() {
        let mut set = ORSet::new();

        set.add("alice");
        assert!(set.contains(&"alice"));

        set.remove(&"alice");
        assert!(!set.contains(&"alice"));
    }

    #[test]
    fn test_or_set_merge() {
        let mut set1 = ORSet::new();
        let mut set2 = ORSet::new();

        set1.add("alice");
        set2.add("bob");

        set1.merge(&set2);

        assert!(set1.contains(&"alice"));
        assert!(set1.contains(&"bob"));
        assert_eq!(set1.len(), 2);
    }

    #[test]
    fn test_or_set_concurrent_add_remove() {
        let mut set1 = ORSet::new();
        let mut set2 = ORSet::new();

        // Both add "alice" with different tags
        set1.add("alice");
        set2.add("alice");

        // set1 removes alice
        set1.remove(&"alice");

        // Merge - alice should still be in the set because set2's add wasn't observed by set1's remove
        set1.merge(&set2);

        assert!(set1.contains(&"alice"), "Concurrent add should win over remove");
    }
}
