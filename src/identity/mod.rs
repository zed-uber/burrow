use anyhow::{Context, Result};
use libp2p::identity::Keypair;
use std::path::Path;

/// Manages persistent cryptographic identity for the peer
pub struct Identity {
    keypair: Keypair,
}

impl Identity {
    /// Load identity from disk, or generate a new one if it doesn't exist
    pub fn load_or_generate(path: &Path) -> Result<Self> {
        let keypair = if path.exists() {
            tracing::info!("Loading existing identity from {:?}", path);
            Self::load_keypair(path)?
        } else {
            tracing::info!("Generating new identity at {:?}", path);
            let keypair = Keypair::generate_ed25519();
            Self::save_keypair(&keypair, path)?;
            keypair
        };

        Ok(Self { keypair })
    }

    /// Get the libp2p keypair
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    /// Get the peer ID derived from the keypair
    pub fn peer_id(&self) -> libp2p::PeerId {
        self.keypair.public().to_peer_id()
    }

    /// Load keypair from file
    fn load_keypair(path: &Path) -> Result<Keypair> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read identity file: {:?}", path))?;

        Keypair::from_protobuf_encoding(&bytes)
            .with_context(|| format!("Failed to decode identity from {:?}", path))
    }

    /// Save keypair to file
    fn save_keypair(keypair: &Keypair, path: &Path) -> Result<()> {
        let bytes = keypair.to_protobuf_encoding()
            .context("Failed to encode keypair")?;

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {:?}", parent))?;
        }

        std::fs::write(path, bytes)
            .with_context(|| format!("Failed to write identity to {:?}", path))?;

        // Set restrictive permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(0o600); // Owner read/write only
            std::fs::set_permissions(path, perms)?;
        }

        Ok(())
    }
}
