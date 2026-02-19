use crate::dag::MessageDAG;
use crate::network::NetworkCommand;
use crate::storage::Storage;
use crate::types::{ChannelId, MessageId};
use anyhow::Result;
use std::collections::HashSet;
use tokio::sync::mpsc;
use tracing::{debug, info};

/// Gossip protocol manager for anti-entropy and message synchronization
pub struct GossipManager {
    network_tx: mpsc::UnboundedSender<NetworkCommand>,
}

impl GossipManager {
    pub fn new(network_tx: mpsc::UnboundedSender<NetworkCommand>) -> Self {
        Self { network_tx }
    }

    /// Request inventory from peers for a channel
    pub fn request_inventory(&self, channel_id: ChannelId) -> Result<()> {
        debug!("Requesting inventory for channel {:?}", channel_id);
        self.network_tx
            .send(NetworkCommand::RequestInventory { channel_id })?;
        Ok(())
    }

    /// Send our inventory for a channel
    pub async fn send_inventory(
        &self,
        channel_id: ChannelId,
        storage: &Storage,
    ) -> Result<()> {
        let message_ids = storage.get_channel_message_ids(channel_id).await?;
        let message_id_set: HashSet<MessageId> = message_ids.into_iter().collect();

        debug!(
            "Sending inventory for channel {:?} with {} messages",
            channel_id,
            message_id_set.len()
        );

        self.network_tx.send(NetworkCommand::BroadcastInventory {
            channel_id,
            message_ids: message_id_set,
        })?;

        Ok(())
    }

    /// Handle received inventory: compare with our DAG and request missing messages
    pub fn handle_inventory(
        &self,
        channel_id: ChannelId,
        their_message_ids: HashSet<MessageId>,
        dag: &MessageDAG,
    ) -> Result<()> {
        let our_message_ids = dag.all_message_ids();

        // Find messages they have that we don't
        let missing: Vec<MessageId> = their_message_ids
            .difference(&our_message_ids)
            .copied()
            .collect();

        if !missing.is_empty() {
            info!(
                "Found {} missing messages for channel {:?}, requesting them",
                missing.len(),
                channel_id
            );

            self.network_tx.send(NetworkCommand::RequestMessages {
                channel_id,
                message_ids: missing,
            })?;
        } else {
            debug!(
                "No missing messages for channel {:?}",
                channel_id
            );
        }

        Ok(())
    }

    /// Handle message request: respond with requested messages
    pub async fn handle_message_request(
        &self,
        channel_id: ChannelId,
        requested_ids: Vec<MessageId>,
        storage: &Storage,
    ) -> Result<()> {
        debug!(
            "Handling message request for {} messages in channel {:?}",
            requested_ids.len(),
            channel_id
        );

        let messages = storage.get_messages_by_ids(&requested_ids).await?;

        if !messages.is_empty() {
            info!(
                "Responding with {} messages for channel {:?}",
                messages.len(),
                channel_id
            );

            self.network_tx.send(NetworkCommand::RespondWithMessages {
                channel_id,
                messages,
            })?;
        }

        Ok(())
    }

    /// Detect missing messages in DAG and request them
    pub fn detect_and_request_missing(
        &self,
        channel_id: ChannelId,
        dag: &MessageDAG,
    ) -> Result<()> {
        let missing_ids: Vec<MessageId> = dag.find_missing_messages().into_iter().collect();

        if !missing_ids.is_empty() {
            info!(
                "Detected {} missing parent messages for channel {:?}, requesting them",
                missing_ids.len(),
                channel_id
            );

            self.network_tx.send(NetworkCommand::RequestMessages {
                channel_id,
                message_ids: missing_ids,
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_gossip_manager_creation() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let _manager = GossipManager::new(tx);
        // Just test that it can be created
    }
}
