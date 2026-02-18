mod network;
mod protocol;
mod storage;
mod tui;
mod types;

use anyhow::Result;
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

    // Generate or load peer ID (for Phase 1, just generate a new one)
    // In later phases, this will be loaded from config
    let peer_id = PeerId::new();
    tracing::info!("Peer ID: {}", peer_id.0);

    // Create network channels
    let (event_tx, event_rx, command_tx, command_rx) = network::create_network_channels();

    // Create and configure network
    let mut network = Network::new(event_tx, command_rx).await?;

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
    let mut app = tui::App::new(storage, peer_id, event_rx, command_tx).await?;
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
