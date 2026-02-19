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

mod crdt;
mod dag;
mod identity;
mod network;
mod protocol;
mod storage;
mod tui;
mod types;

use anyhow::Result;
use identity::Identity;
use network::Network;
use storage::Storage;
use tracing_subscriber::EnvFilter;
use types::PeerId;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize storage directory
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("burrow");

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&data_dir)?;

    // Initialize logging to file (not stdout, to avoid interfering with TUI)
    let log_file = std::fs::File::create(data_dir.join("burrow.log"))?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("burrow=info".parse()?))
        .with_writer(std::sync::Mutex::new(log_file))
        .with_ansi(false) // Disable ANSI colors in log file
        .init();

    tracing::info!("Starting Burrow...");

    // Initialize storage
    let db_path = data_dir.join("burrow.db");

    tracing::info!("Database path: {:?}", db_path);

    let storage = Storage::new(&db_path).await?;

    // Load or generate persistent identity
    let identity_path = data_dir.join("identity.key");
    let identity = Identity::load_or_generate(&identity_path)?;
    let libp2p_peer_id = identity.peer_id();
    let peer_id = PeerId::from_libp2p(&libp2p_peer_id);

    tracing::info!("Peer ID: {}", libp2p_peer_id);
    tracing::info!("App Peer UUID: {}", peer_id.0);

    // Create network channels
    let (event_tx, event_rx, command_tx, command_rx) = network::create_network_channels();

    // Create and configure network with persistent keypair
    let mut network = Network::new(identity.keypair().clone(), event_tx, command_rx).await?;

    // Start listening on a port (default: 9000)
    let listen_port = std::env::var("BURROW_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9000);

    network.listen(listen_port)?;
    tracing::info!("Network listening on port {}", listen_port);

    // Spawn network task
    let network_handle = tokio::spawn(async move {
        if let Err(e) = network.run().await {
            tracing::error!("Network error: {}", e);
        }
    });

    // Run TUI with network channels
    let mut app = tui::App::new(storage, peer_id, libp2p_peer_id, event_rx, command_tx).await?;
    let tui_result = app.run().await;

    // Cleanup
    tracing::info!("Burrow shutting down...");
    network_handle.abort();

    tui_result
}

// Helper to get user directories (will add this as a dependency)
mod dirs {
    use std::path::PathBuf;

    pub fn data_local_dir() -> Option<PathBuf> {
        if cfg!(target_os = "linux") {
            std::env::var("XDG_DATA_HOME")
                .ok()
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|home| PathBuf::from(home).join(".local").join("share"))
                })
        } else if cfg!(target_os = "macos") {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join("Library").join("Application Support"))
        } else if cfg!(target_os = "windows") {
            std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
        } else {
            None
        }
    }
}
