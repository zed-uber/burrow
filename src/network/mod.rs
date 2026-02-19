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

use crate::protocol::NetworkMessage;
use crate::types::{Channel, ChannelId, Message, MessageId};
use anyhow::{Context, Result};
use libp2p::{
    core::upgrade,
    dns, gossipsub, identify, mdns, noise,
    futures::StreamExt,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm, Transport,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

pub mod peer;

/// Network events sent to the application
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// A new peer has connected
    PeerConnected(PeerId),

    /// A peer has disconnected
    PeerDisconnected(PeerId),

    /// Received a chat message from a peer
    MessageReceived(Message),

    /// Local listening address established
    ListeningOn(Multiaddr),

    /// Connection attempt to a peer failed
    ConnectionFailed {
        address: String,
        error: String,
    },

    /// Successfully initiated connection to a peer
    ConnectionDialing {
        address: String,
    },

    /// A peer announced a new channel
    ChannelAnnounced(Channel),

    /// Received full channel state in response to a request
    ChannelStateReceived(Channel),

    /// A peer sent an incremental channel update
    ChannelUpdated(Channel),

    /// Received a request for channel state (we should respond)
    ChannelStateRequested {
        channel_id: ChannelId,
        requesting_peer: PeerId,
    },

    // Phase 4: DAG Synchronization Events

    /// Received a request for specific messages
    MessageRequested {
        channel_id: ChannelId,
        message_ids: Vec<MessageId>,
        requesting_peer: PeerId,
    },

    /// Received messages in response to a request
    MessagesReceived {
        channel_id: ChannelId,
        messages: Vec<Message>,
    },

    /// Received message inventory from a peer
    InventoryReceived {
        channel_id: ChannelId,
        message_ids: std::collections::HashSet<MessageId>,
        from_peer: PeerId,
    },

    /// Received inventory request from a peer
    InventoryRequested {
        channel_id: ChannelId,
        requesting_peer: PeerId,
    },
}

/// Commands sent to the network layer
#[derive(Debug, Clone)]
pub enum NetworkCommand {
    /// Broadcast a message to all peers
    BroadcastMessage(Message),

    /// Connect to a specific peer address
    ConnectToPeer(Multiaddr),

    /// Get list of connected peers
    ListPeers,

    /// Broadcast a new channel announcement
    AnnounceChannel(Channel),

    /// Request full channel state from peers
    RequestChannelState(ChannelId),

    /// Broadcast a channel update (name change, member change, etc)
    BroadcastChannelUpdate(Channel),

    // Phase 4: DAG Synchronization Commands

    /// Request specific messages by ID
    RequestMessages {
        channel_id: ChannelId,
        message_ids: Vec<MessageId>,
    },

    /// Send messages in response to a request
    RespondWithMessages {
        channel_id: ChannelId,
        messages: Vec<Message>,
    },

    /// Broadcast message inventory for anti-entropy
    BroadcastInventory {
        channel_id: ChannelId,
        message_ids: std::collections::HashSet<MessageId>,
    },

    /// Request message inventory from peers
    RequestInventory {
        channel_id: ChannelId,
    },
}

/// Network behavior combining multiple protocols
#[derive(NetworkBehaviour)]
pub struct BurrowBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub identify: identify::Behaviour,
}

/// Network manager handling P2P communication
pub struct Network {
    swarm: Swarm<BurrowBehaviour>,
    event_tx: mpsc::UnboundedSender<NetworkEvent>,
    command_rx: mpsc::UnboundedReceiver<NetworkCommand>,
    gossip_topic: gossipsub::IdentTopic,
}

impl Network {
    /// Create a new network instance
    pub async fn new(
        keypair: libp2p::identity::Keypair,
        event_tx: mpsc::UnboundedSender<NetworkEvent>,
        command_rx: mpsc::UnboundedReceiver<NetworkCommand>,
    ) -> Result<Self> {
        // Use provided keypair for persistent identity
        let local_key = keypair;
        let local_peer_id = PeerId::from(local_key.public());
        info!("Local peer ID: {}", local_peer_id);

        // Set up TCP transport with noise encryption and yamux multiplexing
        let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true));

        let transport = dns::tokio::Transport::system(tcp_transport)?
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key)?)
            .multiplex(yamux::Config::default())
            .boxed();

        // Configure gossipsub for message broadcasting
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(1))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .message_id_fn(message_id_fn)
            .build()
            .expect("Valid gossipsub config");

        let mut gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create gossipsub: {}", e))?;

        // Create the gossipsub topic for chat messages
        let gossip_topic = gossipsub::IdentTopic::new("burrow-chat");
        gossipsub.subscribe(&gossip_topic)?;

        // Set up mDNS for local peer discovery
        let mdns = mdns::tokio::Behaviour::new(
            mdns::Config::default(),
            local_peer_id,
        )?;

        // Set up identify protocol
        let identify = identify::Behaviour::new(identify::Config::new(
            "/burrow/0.1.0".to_string(),
            local_key.public(),
        ));

        // Combine behaviors
        let behaviour = BurrowBehaviour {
            gossipsub,
            mdns,
            identify,
        };

        // Create the swarm
        let swarm = Swarm::new(
            transport,
            behaviour,
            local_peer_id,
            libp2p::swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(Duration::from_secs(60)),
        );

        Ok(Self {
            swarm,
            event_tx,
            command_rx,
            gossip_topic,
        })
    }

    /// Start listening on a TCP port
    pub fn listen(&mut self, port: u16) -> Result<()> {
        let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port)
            .parse()
            .context("Invalid listen address")?;

        self.swarm
            .listen_on(listen_addr)
            .context("Failed to start listening")?;

        Ok(())
    }

    /// Run the network event loop
    pub async fn run(mut self) -> Result<()> {
        info!("Starting network event loop");

        loop {
            tokio::select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => {
                    if let Err(e) = self.handle_swarm_event(event).await {
                        error!("Error handling swarm event: {}", e);
                    }
                }

                // Handle commands from application
                Some(command) = self.command_rx.recv() => {
                    if let Err(e) = self.handle_command(command).await {
                        error!("Error handling command: {}", e);
                    }
                }
            }
        }
    }

    /// Handle swarm events
    async fn handle_swarm_event(&mut self, event: SwarmEvent<<BurrowBehaviour as NetworkBehaviour>::ToSwarm>) -> Result<()> {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
                self.event_tx.send(NetworkEvent::ListeningOn(address))?;
            }

            SwarmEvent::Behaviour(BurrowBehaviourEvent::Gossipsub(
                gossipsub::Event::Message {
                    propagation_source: peer_id,
                    message,
                    ..
                },
            )) => {
                debug!("Received message from {}", peer_id);
                if let Ok(network_msg) = NetworkMessage::from_bytes(&message.data) {
                    match network_msg {
                        NetworkMessage::ChatMessage(msg) => {
                            debug!("Chat message: {:?}", msg);
                            self.event_tx.send(NetworkEvent::MessageReceived(msg))?;
                        }
                        NetworkMessage::ChannelAnnounce { channel } => {
                            debug!("Channel announcement from {}: {}", peer_id, channel.get_name());
                            self.event_tx.send(NetworkEvent::ChannelAnnounced(channel))?;
                        }
                        NetworkMessage::ChannelStateRequest { channel_id } => {
                            debug!("Channel state request from {} for {:?}", peer_id, channel_id);
                            self.event_tx.send(NetworkEvent::ChannelStateRequested {
                                channel_id,
                                requesting_peer: peer_id,
                            })?;
                        }
                        NetworkMessage::ChannelStateResponse { channel } => {
                            debug!("Channel state response from {}: {}", peer_id, channel.get_name());
                            self.event_tx.send(NetworkEvent::ChannelStateReceived(channel))?;
                        }
                        NetworkMessage::ChannelUpdate { channel } => {
                            debug!("Channel update from {}: {}", peer_id, channel.get_name());
                            self.event_tx.send(NetworkEvent::ChannelUpdated(channel))?;
                        }
                        NetworkMessage::MessageRequest { channel_id, message_ids } => {
                            debug!("Message request from {} for {} messages", peer_id, message_ids.len());
                            self.event_tx.send(NetworkEvent::MessageRequested {
                                channel_id,
                                message_ids,
                                requesting_peer: peer_id,
                            })?;
                        }
                        NetworkMessage::MessageResponse { channel_id, messages } => {
                            debug!("Message response from {} with {} messages", peer_id, messages.len());
                            self.event_tx.send(NetworkEvent::MessagesReceived {
                                channel_id,
                                messages,
                            })?;
                        }
                        NetworkMessage::MessageInventory { channel_id, message_ids } => {
                            debug!("Message inventory from {} with {} messages", peer_id, message_ids.len());
                            self.event_tx.send(NetworkEvent::InventoryReceived {
                                channel_id,
                                message_ids,
                                from_peer: peer_id,
                            })?;
                        }
                        NetworkMessage::InventoryRequest { channel_id } => {
                            debug!("Inventory request from {} for channel {:?}", peer_id, channel_id);
                            self.event_tx.send(NetworkEvent::InventoryRequested {
                                channel_id,
                                requesting_peer: peer_id,
                            })?;
                        }
                        _ => {
                            debug!("Received other network message type");
                        }
                    }
                }
            }

            SwarmEvent::Behaviour(BurrowBehaviourEvent::Mdns(mdns::Event::Discovered(
                peers,
            ))) => {
                for (peer_id, addr) in peers {
                    info!("Discovered peer via mDNS: {} at {}", peer_id, addr);
                    // Auto-dial discovered peers silently (no notification for auto-discovery)
                    if let Err(e) = self.swarm.dial(addr.clone()) {
                        debug!("Failed to auto-dial discovered peer {}: {}", peer_id, e);
                    }
                }
            }

            SwarmEvent::Behaviour(BurrowBehaviourEvent::Mdns(mdns::Event::Expired(
                peers,
            ))) => {
                for (peer_id, _) in peers {
                    debug!("mDNS peer expired: {}", peer_id);
                }
            }

            SwarmEvent::Behaviour(BurrowBehaviourEvent::Identify(
                identify::Event::Received { peer_id, info, .. },
            )) => {
                debug!(
                    "Identified peer {}: protocol={} agent={}",
                    peer_id, info.protocol_version, info.agent_version
                );
            }

            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                info!("Connection established with {} via {}", peer_id, endpoint.get_remote_address());
                self.event_tx.send(NetworkEvent::PeerConnected(peer_id))?;
            }

            SwarmEvent::ConnectionClosed {
                peer_id, cause, ..
            } => {
                info!("Connection closed with {}: {:?}", peer_id, cause);
                self.event_tx.send(NetworkEvent::PeerDisconnected(peer_id))?;
            }

            SwarmEvent::IncomingConnection { .. } => {
                debug!("Incoming connection");
            }

            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                // Log but don't send notification - this is often from auto-discovery
                // Manual dial failures are caught immediately in handle_command
                debug!("Outgoing connection error to {:?}: {}", peer_id, error);
            }

            SwarmEvent::IncomingConnectionError { error, .. } => {
                warn!("Incoming connection error: {}", error);
            }

            _ => {}
        }

        Ok(())
    }

    /// Handle commands from the application
    async fn handle_command(&mut self, command: NetworkCommand) -> Result<()> {
        match command {
            NetworkCommand::BroadcastMessage(message) => {
                debug!("Broadcasting message: {:?}", message.id);
                let network_msg = NetworkMessage::ChatMessage(message);
                let bytes = network_msg.to_bytes()?;

                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.gossip_topic.clone(), bytes)?;
            }

            NetworkCommand::ConnectToPeer(addr) => {
                info!("Attempting to connect to peer at {}", addr);
                match self.swarm.dial(addr.clone()) {
                    Ok(_) => {
                        info!("Dialing {}", addr);
                        self.event_tx.send(NetworkEvent::ConnectionDialing {
                            address: addr.to_string(),
                        })?;
                    }
                    Err(e) => {
                        warn!("Failed to dial {}: {}", addr, e);
                        self.event_tx.send(NetworkEvent::ConnectionFailed {
                            address: addr.to_string(),
                            error: e.to_string(),
                        })?;
                    }
                }
            }

            NetworkCommand::ListPeers => {
                let peers: Vec<_> = self.swarm.connected_peers().collect();
                info!("Connected peers: {:?}", peers);
            }

            NetworkCommand::AnnounceChannel(channel) => {
                debug!("Broadcasting channel announcement: {}", channel.get_name());
                let network_msg = NetworkMessage::ChannelAnnounce { channel };
                let bytes = network_msg.to_bytes()?;

                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.gossip_topic.clone(), bytes)?;
            }

            NetworkCommand::RequestChannelState(channel_id) => {
                debug!("Requesting channel state for {:?}", channel_id);
                let network_msg = NetworkMessage::ChannelStateRequest { channel_id };
                let bytes = network_msg.to_bytes()?;

                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.gossip_topic.clone(), bytes)?;
            }

            NetworkCommand::BroadcastChannelUpdate(channel) => {
                debug!("Broadcasting channel update: {}", channel.get_name());
                let network_msg = NetworkMessage::ChannelUpdate { channel };
                let bytes = network_msg.to_bytes()?;

                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.gossip_topic.clone(), bytes)?;
            }

            NetworkCommand::RequestMessages { channel_id, message_ids } => {
                debug!("Requesting {} messages for channel {:?}", message_ids.len(), channel_id);
                let network_msg = NetworkMessage::MessageRequest { channel_id, message_ids };
                let bytes = network_msg.to_bytes()?;

                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.gossip_topic.clone(), bytes)?;
            }

            NetworkCommand::RespondWithMessages { channel_id, messages } => {
                debug!("Sending {} messages for channel {:?}", messages.len(), channel_id);
                let network_msg = NetworkMessage::MessageResponse { channel_id, messages };
                let bytes = network_msg.to_bytes()?;

                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.gossip_topic.clone(), bytes)?;
            }

            NetworkCommand::BroadcastInventory { channel_id, message_ids } => {
                debug!("Broadcasting inventory with {} messages for channel {:?}", message_ids.len(), channel_id);
                let network_msg = NetworkMessage::MessageInventory { channel_id, message_ids };
                let bytes = network_msg.to_bytes()?;

                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.gossip_topic.clone(), bytes)?;
            }

            NetworkCommand::RequestInventory { channel_id } => {
                debug!("Requesting inventory for channel {:?}", channel_id);
                let network_msg = NetworkMessage::InventoryRequest { channel_id };
                let bytes = network_msg.to_bytes()?;

                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(self.gossip_topic.clone(), bytes)?;
            }
        }

        Ok(())
    }
}

/// Create network channels for communication
pub fn create_network_channels() -> (
    mpsc::UnboundedSender<NetworkEvent>,
    mpsc::UnboundedReceiver<NetworkEvent>,
    mpsc::UnboundedSender<NetworkCommand>,
    mpsc::UnboundedReceiver<NetworkCommand>,
) {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    (event_tx, event_rx, command_tx, command_rx)
}
