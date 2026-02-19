pub mod gossip;

use crate::types::{ChannelId, Message, MessageId};
use std::collections::{HashMap, HashSet, VecDeque};

/// Message DAG (Directed Acyclic Graph) for causal ordering
///
/// The DAG tracks the causal relationships between messages using parent hashes.
/// Each message can have multiple parents (for concurrent messages) and the DAG
/// maintains the "heads" - messages with no children that represent the current
/// frontier of conversation.
pub struct MessageDAG {
    /// Messages indexed by ID
    messages: HashMap<MessageId, Message>,

    /// Child relationships: message_id -> set of children
    children: HashMap<MessageId, HashSet<MessageId>>,

    /// Current heads (messages with no children) per channel
    heads: HashMap<ChannelId, HashSet<MessageId>>,
}

impl MessageDAG {
    pub fn new() -> Self {
        Self {
            messages: HashMap::new(),
            children: HashMap::new(),
            heads: HashMap::new(),
        }
    }

    /// Add a message to the DAG
    pub fn add_message(&mut self, message: Message) -> Result<(), DagError> {
        let message_id = message.id;
        let channel_id = message.channel_id;

        // Verify all parents exist (unless this is a root message)
        for parent_id in &message.parent_hashes {
            if !self.messages.contains_key(parent_id) {
                return Err(DagError::MissingParent {
                    message_id,
                    missing_parent: *parent_id,
                });
            }
        }

        // Remove parents from heads (they now have a child)
        if let Some(channel_heads) = self.heads.get_mut(&channel_id) {
            for parent_id in &message.parent_hashes {
                channel_heads.remove(parent_id);
            }
        }

        // Add child relationships
        for parent_id in &message.parent_hashes {
            self.children
                .entry(*parent_id)
                .or_insert_with(HashSet::new)
                .insert(message_id);
        }

        // Add message as a new head
        self.heads
            .entry(channel_id)
            .or_insert_with(HashSet::new)
            .insert(message_id);

        // Store the message
        self.messages.insert(message_id, message);

        Ok(())
    }

    /// Get current heads for a channel (messages to use as parents for new messages)
    pub fn get_heads(&self, channel_id: &ChannelId) -> Vec<MessageId> {
        self.heads
            .get(channel_id)
            .map(|heads| heads.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Get a message by ID
    pub fn get_message(&self, message_id: &MessageId) -> Option<&Message> {
        self.messages.get(message_id)
    }

    /// Get all messages in the DAG
    pub fn all_messages(&self) -> impl Iterator<Item = &Message> {
        self.messages.values()
    }

    /// Get messages for a specific channel in topological order
    pub fn get_ordered_messages(&self, channel_id: &ChannelId) -> Vec<Message> {
        let channel_messages: Vec<_> = self
            .messages
            .values()
            .filter(|m| m.channel_id == *channel_id)
            .collect();

        self.topological_sort(channel_messages)
    }

    /// Perform topological sort on messages using Kahn's algorithm
    /// Messages with the same causal depth are ordered by Lamport timestamp,
    /// then by message ID (UUID v7, which is time-ordered)
    fn topological_sort(&self, messages: Vec<&Message>) -> Vec<Message> {
        if messages.is_empty() {
            return Vec::new();
        }

        // Build local parent/child maps for just these messages
        let message_ids: HashSet<MessageId> = messages.iter().map(|m| m.id).collect();

        let mut in_degree: HashMap<MessageId, usize> = HashMap::new();
        let mut local_children: HashMap<MessageId, Vec<MessageId>> = HashMap::new();

        // Calculate in-degrees and build adjacency list
        for message in &messages {
            in_degree.entry(message.id).or_insert(0);

            for parent_id in &message.parent_hashes {
                // Only count parents that are in our message set
                if message_ids.contains(parent_id) {
                    *in_degree.entry(message.id).or_insert(0) += 1;
                    local_children
                        .entry(*parent_id)
                        .or_insert_with(Vec::new)
                        .push(message.id);
                }
            }
        }

        // Find all nodes with in-degree 0 (roots)
        let mut queue: VecDeque<MessageId> = in_degree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(id, _)| *id)
            .collect();

        // Sort queue by Lamport timestamp and message ID for deterministic ordering
        let mut queue_vec: Vec<_> = queue.drain(..).collect();
        queue_vec.sort_by(|a, b| {
            let msg_a = self.messages.get(a).unwrap();
            let msg_b = self.messages.get(b).unwrap();

            msg_a.lamport_timestamp
                .cmp(&msg_b.lamport_timestamp)
                .then_with(|| msg_a.id.cmp(&msg_b.id))
        });
        queue.extend(queue_vec);

        let mut sorted = Vec::new();

        while let Some(message_id) = queue.pop_front() {
            let message = self.messages.get(&message_id).unwrap().clone();
            sorted.push(message);

            // Process children
            if let Some(children) = local_children.get(&message_id) {
                let mut processable = Vec::new();

                for child_id in children {
                    let degree = in_degree.get_mut(child_id).unwrap();
                    *degree -= 1;

                    if *degree == 0 {
                        processable.push(*child_id);
                    }
                }

                // Sort processable by Lamport timestamp and message ID
                processable.sort_by(|a, b| {
                    let msg_a = self.messages.get(a).unwrap();
                    let msg_b = self.messages.get(b).unwrap();

                    msg_a.lamport_timestamp
                        .cmp(&msg_b.lamport_timestamp)
                        .then_with(|| msg_a.id.cmp(&msg_b.id))
                });

                queue.extend(processable);
            }
        }

        sorted
    }

    /// Find missing messages that are referenced but not present
    pub fn find_missing_messages(&self) -> HashSet<MessageId> {
        let mut missing = HashSet::new();

        for message in self.messages.values() {
            for parent_id in &message.parent_hashes {
                if !self.messages.contains_key(parent_id) {
                    missing.insert(*parent_id);
                }
            }
        }

        missing
    }

    /// Get all message IDs we currently have
    pub fn all_message_ids(&self) -> HashSet<MessageId> {
        self.messages.keys().copied().collect()
    }

    /// Check if we have a specific message
    pub fn has_message(&self, message_id: &MessageId) -> bool {
        self.messages.contains_key(message_id)
    }

    /// Load messages from storage into the DAG
    pub fn load_messages(&mut self, messages: Vec<Message>) -> Result<(), DagError> {
        // Sort messages by created_at to ensure parents come before children
        let mut sorted_messages = messages;
        sorted_messages.sort_by_key(|m| m.created_at);

        // First pass: add all messages without parent validation
        // This handles the case where messages may be out of order
        for message in sorted_messages {
            let message_id = message.id;

            // Add child relationships for existing parents
            for parent_id in &message.parent_hashes {
                if self.messages.contains_key(parent_id) {
                    self.children
                        .entry(*parent_id)
                        .or_insert_with(HashSet::new)
                        .insert(message_id);
                }
            }

            // Store the message
            self.messages.insert(message_id, message);
        }

        // Second pass: rebuild heads
        self.heads.clear();
        for message in self.messages.values() {
            // A message is a head if it has no children
            if !self.children.contains_key(&message.id)
                || self.children.get(&message.id).unwrap().is_empty()
            {
                self.heads
                    .entry(message.channel_id)
                    .or_insert_with(HashSet::new)
                    .insert(message.id);
            }
        }

        Ok(())
    }
}

impl Default for MessageDAG {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DagError {
    #[error("Message {message_id:?} references missing parent {missing_parent:?}")]
    MissingParent {
        message_id: MessageId,
        missing_parent: MessageId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MessageContent, PeerId, VectorClock};

    fn create_test_message(
        channel_id: ChannelId,
        author: PeerId,
        lamport: u64,
        parents: Vec<MessageId>,
    ) -> Message {
        let mut vc = VectorClock::new();
        vc.increment(author);

        let mut msg = Message::new(
            channel_id,
            author,
            MessageContent {
                text: format!("Message {}", lamport),
            },
            vc,
            lamport,
        );
        msg.parent_hashes = parents;
        msg
    }

    #[test]
    fn test_dag_basic_chain() {
        let mut dag = MessageDAG::new();
        let channel = ChannelId::new();
        let author = PeerId::new();

        // Create a chain: m1 -> m2 -> m3
        let m1 = create_test_message(channel, author, 1, vec![]);
        let m1_id = m1.id;

        let m2 = create_test_message(channel, author, 2, vec![m1_id]);
        let m2_id = m2.id;

        let m3 = create_test_message(channel, author, 3, vec![m2_id]);
        let m3_id = m3.id;

        dag.add_message(m1).unwrap();
        dag.add_message(m2).unwrap();
        dag.add_message(m3).unwrap();

        let heads = dag.get_heads(&channel);
        assert_eq!(heads.len(), 1);
        assert_eq!(heads[0], m3_id);
    }

    #[test]
    fn test_dag_concurrent_messages() {
        let mut dag = MessageDAG::new();
        let channel = ChannelId::new();
        let author = PeerId::new();

        // Create concurrent messages: m1 <- m2, m1 <- m3
        let m1 = create_test_message(channel, author, 1, vec![]);
        let m1_id = m1.id;

        let m2 = create_test_message(channel, author, 2, vec![m1_id]);
        let m3 = create_test_message(channel, author, 3, vec![m1_id]);

        dag.add_message(m1).unwrap();
        dag.add_message(m2).unwrap();
        dag.add_message(m3).unwrap();

        let heads = dag.get_heads(&channel);
        assert_eq!(heads.len(), 2);
    }

    #[test]
    fn test_dag_merge() {
        let mut dag = MessageDAG::new();
        let channel = ChannelId::new();
        let author = PeerId::new();

        // Create: m1 <- m2, m1 <- m3, m2+m3 <- m4 (merge)
        let m1 = create_test_message(channel, author, 1, vec![]);
        let m1_id = m1.id;

        let m2 = create_test_message(channel, author, 2, vec![m1_id]);
        let m2_id = m2.id;

        let m3 = create_test_message(channel, author, 3, vec![m1_id]);
        let m3_id = m3.id;

        let m4 = create_test_message(channel, author, 4, vec![m2_id, m3_id]);
        let m4_id = m4.id;

        dag.add_message(m1).unwrap();
        dag.add_message(m2).unwrap();
        dag.add_message(m3).unwrap();
        dag.add_message(m4).unwrap();

        let heads = dag.get_heads(&channel);
        assert_eq!(heads.len(), 1);
        assert_eq!(heads[0], m4_id);
    }

    #[test]
    fn test_topological_sort() {
        let mut dag = MessageDAG::new();
        let channel = ChannelId::new();
        let author = PeerId::new();

        let m1 = create_test_message(channel, author, 1, vec![]);
        let m1_id = m1.id;

        let m2 = create_test_message(channel, author, 2, vec![m1_id]);
        let m2_id = m2.id;

        let m3 = create_test_message(channel, author, 3, vec![m2_id]);

        dag.add_message(m1).unwrap();
        dag.add_message(m2).unwrap();
        dag.add_message(m3).unwrap();

        let ordered = dag.get_ordered_messages(&channel);
        assert_eq!(ordered.len(), 3);
        assert_eq!(ordered[0].lamport_timestamp, 1);
        assert_eq!(ordered[1].lamport_timestamp, 2);
        assert_eq!(ordered[2].lamport_timestamp, 3);
    }
}
