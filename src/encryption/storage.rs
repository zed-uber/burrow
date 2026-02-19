use anyhow::{Context, Result};
use libsignal_protocol::{
    Direction, GenericSignedPreKey, IdentityChange, IdentityKey, IdentityKeyPair,
    IdentityKeyStore, KyberPreKeyId, KyberPreKeyRecord, KyberPreKeyStore, PreKeyId, PreKeyRecord,
    PreKeyStore, ProtocolAddress, ProtocolStore, PublicKey, SenderKeyRecord, SenderKeyStore,
    SessionRecord, SessionStore, SignedPreKeyId, SignedPreKeyRecord, SignedPreKeyStore,
};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

/// SQLite-backed Signal Protocol storage
///
/// Implements all required Signal Protocol storage traits:
/// - IdentityKeyStore: Identity keys and trust
/// - PreKeyStore: Pre-keys for X3DH
/// - SignedPreKeyStore: Signed pre-keys
/// - SessionStore: Double Ratchet sessions
/// - SenderKeyStore: Sender keys for groups
/// - KyberPreKeyStore: Kyber pre-keys (in-memory for now)
#[derive(Clone)]
pub struct SignalStore {
    pool: Arc<SqlitePool>,
    identity_key_pair: Arc<IdentityKeyPair>,
    registration_id: u32,
    // In-memory store for kyber pre-keys (TODO: persist to database)
    kyber_pre_keys: Arc<TokioMutex<HashMap<u32, KyberPreKeyRecord>>>,
}

impl SignalStore {
    pub fn new(pool: SqlitePool, identity_key_pair: IdentityKeyPair, registration_id: u32) -> Self {
        Self {
            pool: Arc::new(pool),
            identity_key_pair: Arc::new(identity_key_pair),
            registration_id,
            kyber_pre_keys: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    pub fn identity_key_pair(&self) -> &IdentityKeyPair {
        &self.identity_key_pair
    }

    pub fn registration_id(&self) -> u32 {
        self.registration_id
    }
}

// Implement IdentityKeyStore
#[async_trait::async_trait(?Send)]
impl IdentityKeyStore for SignalStore {
    async fn get_identity_key_pair(&self) -> Result<IdentityKeyPair, libsignal_protocol::SignalProtocolError> {
        Ok((*self.identity_key_pair).clone())
    }

    async fn get_local_registration_id(&self) -> Result<u32, libsignal_protocol::SignalProtocolError> {
        Ok(self.registration_id)
    }

    async fn save_identity(
        &mut self,
        address: &ProtocolAddress,
        identity_key: &IdentityKey,
    ) -> Result<IdentityChange, libsignal_protocol::SignalProtocolError> {
        let address_str = address.name();
        let identity_bytes = identity_key.serialize();

        // Check if identity already exists
        let existing = self.get_identity(address).await?;

        sqlx::query(
            "INSERT INTO identity_keys (address, identity_key, trust_level)
             VALUES (?, ?, 1)
             ON CONFLICT(address) DO UPDATE SET identity_key = excluded.identity_key"
        )
        .bind(address_str)
        .bind(&identity_bytes[..])
        .execute(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "save_identity",
            format!("Database error: {}", e)
        ))?;

        match existing {
            Some(existing_key) if existing_key.public_key() != identity_key.public_key() => {
                // Identity changed - replaced existing
                Ok(IdentityChange::ReplacedExisting)
            }
            _ => {
                // New identity or unchanged
                Ok(IdentityChange::NewOrUnchanged)
            }
        }
    }

    async fn is_trusted_identity(
        &self,
        address: &ProtocolAddress,
        identity_key: &IdentityKey,
        _direction: Direction,
    ) -> Result<bool, libsignal_protocol::SignalProtocolError> {
        let address_str = address.name();

        let row = sqlx::query(
            "SELECT identity_key FROM identity_keys WHERE address = ?"
        )
        .bind(address_str)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "is_trusted_identity",
            format!("Database error: {}", e)
        ))?;

        match row {
            Some(row) => {
                let stored_bytes: Vec<u8> = row.get("identity_key");
                let stored_key = IdentityKey::decode(&stored_bytes)
                    .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
                        "is_trusted_identity",
                        format!("Failed to decode identity key: {}", e)
                    ))?;

                Ok(stored_key.public_key() == identity_key.public_key())
            }
            None => Ok(true), // First contact, trust on first use (TOFU)
        }
    }

    async fn get_identity(
        &self,
        address: &ProtocolAddress,
    ) -> Result<Option<IdentityKey>, libsignal_protocol::SignalProtocolError> {
        let address_str = address.name();

        let row = sqlx::query(
            "SELECT identity_key FROM identity_keys WHERE address = ?"
        )
        .bind(address_str)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "get_identity",
            format!("Database error: {}", e)
        ))?;

        match row {
            Some(row) => {
                let identity_bytes: Vec<u8> = row.get("identity_key");
                let identity_key = IdentityKey::decode(&identity_bytes)
                    .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
                        "get_identity",
                        format!("Failed to decode identity key: {}", e)
                    ))?;
                Ok(Some(identity_key))
            }
            None => Ok(None),
        }
    }
}

// Implement PreKeyStore
#[async_trait::async_trait(?Send)]
impl PreKeyStore for SignalStore {
    async fn get_pre_key(
        &self,
        pre_key_id: PreKeyId,
    ) -> Result<PreKeyRecord, libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = pre_key_id.into();
        let row = sqlx::query(
            "SELECT record FROM pre_keys WHERE pre_key_id = ?"
        )
        .bind(id_value as i64)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "get_pre_key",
            format!("Database error: {}", e)
        ))?;

        match row {
            Some(row) => {
                let record_bytes: Vec<u8> = row.get("record");
                PreKeyRecord::deserialize(&record_bytes)
            }
            None => Err(libsignal_protocol::SignalProtocolError::InvalidPreKeyId),
        }
    }

    async fn save_pre_key(
        &mut self,
        pre_key_id: PreKeyId,
        record: &PreKeyRecord,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = pre_key_id.into();
        let record_bytes = record.serialize()
            .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
                "save_pre_key",
                format!("Failed to serialize: {}", e)
            ))?;

        sqlx::query(
            "INSERT INTO pre_keys (pre_key_id, record) VALUES (?, ?)
             ON CONFLICT(pre_key_id) DO UPDATE SET record = excluded.record"
        )
        .bind(id_value as i64)
        .bind(&record_bytes[..])
        .execute(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "save_pre_key",
            format!("Database error: {}", e)
        ))?;

        Ok(())
    }

    async fn remove_pre_key(
        &mut self,
        pre_key_id: PreKeyId,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = pre_key_id.into();
        sqlx::query(
            "DELETE FROM pre_keys WHERE pre_key_id = ?"
        )
        .bind(id_value as i64)
        .execute(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "remove_pre_key",
            format!("Database error: {}", e)
        ))?;

        Ok(())
    }
}

// Implement SignedPreKeyStore
#[async_trait::async_trait(?Send)]
impl SignedPreKeyStore for SignalStore {
    async fn get_signed_pre_key(
        &self,
        signed_pre_key_id: SignedPreKeyId,
    ) -> Result<SignedPreKeyRecord, libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = signed_pre_key_id.into();
        let row = sqlx::query(
            "SELECT record FROM signed_pre_keys WHERE signed_pre_key_id = ?"
        )
        .bind(id_value as i64)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "get_signed_pre_key",
            format!("Database error: {}", e)
        ))?;

        match row {
            Some(row) => {
                let record_bytes: Vec<u8> = row.get("record");
                SignedPreKeyRecord::deserialize(&record_bytes)
            }
            None => Err(libsignal_protocol::SignalProtocolError::InvalidSignedPreKeyId),
        }
    }

    async fn save_signed_pre_key(
        &mut self,
        signed_pre_key_id: SignedPreKeyId,
        record: &SignedPreKeyRecord,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = signed_pre_key_id.into();
        let record_bytes = record.serialize()
            .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
                "save_signed_pre_key",
                format!("Failed to serialize: {}", e)
            ))?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        sqlx::query(
            "INSERT INTO signed_pre_keys (signed_pre_key_id, record, timestamp) VALUES (?, ?, ?)
             ON CONFLICT(signed_pre_key_id) DO UPDATE SET record = excluded.record, timestamp = excluded.timestamp"
        )
        .bind(id_value as i64)
        .bind(&record_bytes[..])
        .bind(timestamp)
        .execute(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "save_signed_pre_key",
            format!("Database error: {}", e)
        ))?;

        Ok(())
    }
}

// Implement SessionStore
#[async_trait::async_trait(?Send)]
impl SessionStore for SignalStore {
    async fn load_session(
        &self,
        address: &ProtocolAddress,
    ) -> Result<Option<SessionRecord>, libsignal_protocol::SignalProtocolError> {
        let address_str = address.name();
        let device_id: u32 = address.device_id().into();

        let row = sqlx::query(
            "SELECT record FROM sessions WHERE address = ? AND device_id = ?"
        )
        .bind(address_str)
        .bind(device_id as i64)
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "load_session",
            format!("Database error: {}", e)
        ))?;

        match row {
            Some(row) => {
                let record_bytes: Vec<u8> = row.get("record");
                let session = SessionRecord::deserialize(&record_bytes)?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    async fn store_session(
        &mut self,
        address: &ProtocolAddress,
        record: &SessionRecord,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let address_str = address.name();
        let device_id: u32 = address.device_id().into();
        let record_bytes = record.serialize()
            .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
                "store_session",
                format!("Failed to serialize: {}", e)
            ))?;

        sqlx::query(
            "INSERT INTO sessions (address, device_id, record) VALUES (?, ?, ?)
             ON CONFLICT(address, device_id) DO UPDATE SET record = excluded.record"
        )
        .bind(address_str)
        .bind(device_id as i64)
        .bind(&record_bytes[..])
        .execute(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "store_session",
            format!("Database error: {}", e)
        ))?;

        Ok(())
    }
}

// Implement SenderKeyStore
#[async_trait::async_trait(?Send)]
impl SenderKeyStore for SignalStore {
    async fn store_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: uuid::Uuid,
        record: &SenderKeyRecord,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let sender_str = sender.name();
        let distribution_bytes = distribution_id.as_bytes();
        let record_bytes = record.serialize()
            .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
                "store_sender_key",
                format!("Failed to serialize: {}", e)
            ))?;

        sqlx::query(
            "INSERT INTO sender_keys (address, distribution_id, record) VALUES (?, ?, ?)
             ON CONFLICT(address, distribution_id) DO UPDATE SET record = excluded.record"
        )
        .bind(sender_str)
        .bind(&distribution_bytes[..])
        .bind(&record_bytes[..])
        .execute(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "store_sender_key",
            format!("Database error: {}", e)
        ))?;

        Ok(())
    }

    async fn load_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: uuid::Uuid,
    ) -> Result<Option<SenderKeyRecord>, libsignal_protocol::SignalProtocolError> {
        let sender_str = sender.name();
        let distribution_bytes = distribution_id.as_bytes();

        let row = sqlx::query(
            "SELECT record FROM sender_keys WHERE address = ? AND distribution_id = ?"
        )
        .bind(sender_str)
        .bind(&distribution_bytes[..])
        .fetch_optional(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidState(
            "load_sender_key",
            format!("Database error: {}", e)
        ))?;

        match row {
            Some(row) => {
                let record_bytes: Vec<u8> = row.get("record");
                let record = SenderKeyRecord::deserialize(&record_bytes)?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }
}

// Implement KyberPreKeyStore (in-memory for now)
#[async_trait::async_trait(?Send)]
impl KyberPreKeyStore for SignalStore {
    async fn get_kyber_pre_key(
        &self,
        kyber_prekey_id: KyberPreKeyId,
    ) -> Result<KyberPreKeyRecord, libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = kyber_prekey_id.into();
        let store = self.kyber_pre_keys.lock().await;

        store
            .get(&id_value)
            .cloned()
            .ok_or(libsignal_protocol::SignalProtocolError::InvalidKyberPreKeyId)
    }

    async fn save_kyber_pre_key(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
        record: &KyberPreKeyRecord,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = kyber_prekey_id.into();
        let mut store = self.kyber_pre_keys.lock().await;
        store.insert(id_value, record.clone());
        Ok(())
    }

    async fn mark_kyber_pre_key_used(
        &mut self,
        kyber_prekey_id: KyberPreKeyId,
        _ec_prekey_id: SignedPreKeyId,
        _base_key: &PublicKey,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        // For in-memory store, we just keep the key
        // In a production implementation, you'd mark it as used in the database
        Ok(())
    }
}

// Implement ProtocolStore (marker trait)
impl ProtocolStore for SignalStore {}
