use crate::encryption::storage::SignalStore;
use crate::types::PeerId;
use anyhow::Result;
use libsignal_protocol::{
    message_decrypt, message_encrypt, process_prekey_bundle, CiphertextMessage, DeviceId,
    PreKeyBundle, ProtocolAddress,
};
use rand::rngs::OsRng;
use rand::TryRngCore as _;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;

/// Session manager for Signal Protocol encryption
///
/// Wraps libsignal operations with proper store management
pub struct SessionManager {
    store: Arc<Mutex<SignalStore>>,
}

impl SessionManager {
    pub fn new(store: SignalStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    /// Convert PeerId to ProtocolAddress (device ID is always 1 for P2P)
    pub fn peer_to_address(peer_id: &PeerId) -> ProtocolAddress {
        ProtocolAddress::new(peer_id.0.to_string(), DeviceId::new(1).expect("Device ID 1 is valid"))
    }

    /// Get reference to the store
    pub fn store(&self) -> Arc<Mutex<SignalStore>> {
        self.store.clone()
    }

    /// Process a prekey bundle to establish a session
    pub async fn process_prekey_bundle(
        &self,
        remote_address: &ProtocolAddress,
        bundle: &PreKeyBundle,
    ) -> Result<()> {
        let mut rng = OsRng.unwrap_err();
        let store = self.store.lock().await;

        // Lock each store independently to allow borrowing
        let mut session_store = store.session_store.lock().await;
        let mut identity_store = store.identity_store.lock().await;

        process_prekey_bundle(
            remote_address,
            &mut *session_store,
            &mut *identity_store,
            bundle,
            SystemTime::now(),
            &mut rng,
        )
        .await?;

        Ok(())
    }

    /// Encrypt a message for a remote peer
    pub async fn encrypt_message(
        &self,
        remote_address: &ProtocolAddress,
        plaintext: &[u8],
    ) -> Result<CiphertextMessage> {
        let mut rng = OsRng.unwrap_err();
        let store = self.store.lock().await;

        // Lock each store independently to allow borrowing
        let mut session_store = store.session_store.lock().await;
        let mut identity_store = store.identity_store.lock().await;

        let ciphertext = message_encrypt(
            plaintext,
            remote_address,
            &mut *session_store,
            &mut *identity_store,
            SystemTime::now(),
            &mut rng,
        )
        .await?;

        Ok(ciphertext)
    }

    /// Decrypt a message from a remote peer
    pub async fn decrypt_message(
        &self,
        remote_address: &ProtocolAddress,
        ciphertext: &CiphertextMessage,
    ) -> Result<Vec<u8>> {
        let mut rng = OsRng.unwrap_err();
        let store = self.store.lock().await;

        // Lock each store independently to allow borrowing
        let mut session_store = store.session_store.lock().await;
        let mut identity_store = store.identity_store.lock().await;
        let mut pre_key_store = store.pre_key_store.lock().await;
        let signed_pre_key_store = store.signed_pre_key_store.lock().await;
        let mut kyber_pre_key_store = store.kyber_pre_key_store.lock().await;

        let plaintext = message_decrypt(
            ciphertext,
            remote_address,
            &mut *session_store,
            &mut *identity_store,
            &mut *pre_key_store,
            &*signed_pre_key_store,
            &mut *kyber_pre_key_store,
            &mut rng,
        )
        .await?;

        Ok(plaintext)
    }
}

/// Group session manager for Sender Keys encryption
pub struct GroupSessionManager {
    store: Arc<Mutex<SignalStore>>,
}

impl GroupSessionManager {
    pub fn new(store: SignalStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    // TODO: Phase 5 - Implement sender key encryption for groups
}
