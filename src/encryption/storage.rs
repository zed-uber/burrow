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

// Individual store implementations that share the SQLite pool

#[derive(Clone)]
pub struct SqliteSessionStore {
    pool: Arc<SqlitePool>,
}

#[derive(Clone)]
pub struct SqlitePreKeyStore {
    pool: Arc<SqlitePool>,
}

#[derive(Clone)]
pub struct SqliteSignedPreKeyStore {
    pool: Arc<SqlitePool>,
}

#[derive(Clone)]
pub struct SqliteIdentityKeyStore {
    pool: Arc<SqlitePool>,
    identity_key_pair: Arc<IdentityKeyPair>,
    registration_id: u32,
}

#[derive(Clone)]
pub struct SqliteSenderKeyStore {
    pool: Arc<SqlitePool>,
}

#[derive(Clone)]
pub struct InMemoryKyberPreKeyStore {
    kyber_pre_keys: Arc<TokioMutex<HashMap<u32, KyberPreKeyRecord>>>,
}

/// SQLite-backed Signal Protocol storage
///
/// Each store is independently lockable to allow concurrent access
/// This is necessary because libsignal functions require multiple store types simultaneously
#[derive(Clone)]
pub struct SignalStore {
    pub session_store: Arc<TokioMutex<SqliteSessionStore>>,
    pub pre_key_store: Arc<TokioMutex<SqlitePreKeyStore>>,
    pub signed_pre_key_store: Arc<TokioMutex<SqliteSignedPreKeyStore>>,
    pub identity_store: Arc<TokioMutex<SqliteIdentityKeyStore>>,
    pub sender_key_store: Arc<TokioMutex<SqliteSenderKeyStore>>,
    pub kyber_pre_key_store: Arc<TokioMutex<InMemoryKyberPreKeyStore>>,
}

impl SignalStore {
    pub fn new(pool: SqlitePool, identity_key_pair: IdentityKeyPair, registration_id: u32) -> Self {
        let pool = Arc::new(pool);

        Self {
            session_store: Arc::new(TokioMutex::new(SqliteSessionStore {
                pool: pool.clone(),
            })),
            pre_key_store: Arc::new(TokioMutex::new(SqlitePreKeyStore {
                pool: pool.clone(),
            })),
            signed_pre_key_store: Arc::new(TokioMutex::new(SqliteSignedPreKeyStore {
                pool: pool.clone(),
            })),
            identity_store: Arc::new(TokioMutex::new(SqliteIdentityKeyStore {
                pool: pool.clone(),
                identity_key_pair: Arc::new(identity_key_pair.clone()),
                registration_id,
            })),
            sender_key_store: Arc::new(TokioMutex::new(SqliteSenderKeyStore {
                pool: pool.clone(),
            })),
            kyber_pre_key_store: Arc::new(TokioMutex::new(InMemoryKyberPreKeyStore {
                kyber_pre_keys: Arc::new(TokioMutex::new(HashMap::new())),
            })),
        }
    }

    pub async fn identity_key_pair(&self) -> IdentityKeyPair {
        let store = self.identity_store.lock().await;
        (*store.identity_key_pair).clone()
    }

    pub async fn registration_id(&self) -> u32 {
        let store = self.identity_store.lock().await;
        store.registration_id
    }
}

// Implement IdentityKeyStore for SqliteIdentityKeyStore
#[async_trait::async_trait(?Send)]
impl IdentityKeyStore for SqliteIdentityKeyStore {
    async fn get_identity_key_pair(&self) -> Result<IdentityKeyPair, libsignal_protocol::SignalProtocolError> {
        Ok((*self.identity_key_pair).clone())
    }

    async fn get_local_registration_id(&self) -> Result<u32, libsignal_protocol::SignalProtocolError> {
        Ok(self.registration_id)
    }

    async fn save_identity(
        &mut self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
    ) -> Result<IdentityChange, libsignal_protocol::SignalProtocolError> {
        let address_str = address.name();
        let identity_bytes = identity.serialize();

        // Check if we already have an identity for this address
        let existing = self.get_identity(address).await?;

        let result = sqlx::query(
            "INSERT INTO identity_keys (address, identity_key, trust_level) VALUES (?, ?, 0)
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

        if existing.is_some() && existing.as_ref() != Some(identity) {
            Ok(IdentityChange::ReplacedExisting)
        } else {
            Ok(IdentityChange::NewOrUnchanged)
        }
    }

    async fn is_trusted_identity(
        &self,
        address: &ProtocolAddress,
        identity: &IdentityKey,
        _direction: Direction,
    ) -> Result<bool, libsignal_protocol::SignalProtocolError> {
        let address_str = address.name();
        let identity_bytes = identity.serialize();

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
                Ok(&stored_bytes[..] == &identity_bytes[..])
            }
            None => Ok(true), // Trust on first use
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
                let identity = IdentityKey::decode(&identity_bytes)?;
                Ok(Some(identity))
            }
            None => Ok(None),
        }
    }
}

// Implement PreKeyStore for SqlitePreKeyStore
#[async_trait::async_trait(?Send)]
impl PreKeyStore for SqlitePreKeyStore {
    async fn get_pre_key(&self, prekey_id: PreKeyId) -> Result<PreKeyRecord, libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = prekey_id.into();

        let row = sqlx::query(
            "SELECT record FROM pre_keys WHERE pre_key_id = ?"
        )
        .bind(id_value as i64)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidPreKeyId)?;

        let record_bytes: Vec<u8> = row.get("record");
        PreKeyRecord::deserialize(&record_bytes)
    }

    async fn save_pre_key(
        &mut self,
        prekey_id: PreKeyId,
        record: &PreKeyRecord,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = prekey_id.into();
        let record_bytes = record.serialize()?;

        sqlx::query(
            "INSERT OR REPLACE INTO pre_keys (pre_key_id, record) VALUES (?, ?)"
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

    async fn remove_pre_key(&mut self, prekey_id: PreKeyId) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = prekey_id.into();

        sqlx::query("DELETE FROM pre_keys WHERE pre_key_id = ?")
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

// Implement SignedPreKeyStore for SqliteSignedPreKeyStore
#[async_trait::async_trait(?Send)]
impl SignedPreKeyStore for SqliteSignedPreKeyStore {
    async fn get_signed_pre_key(
        &self,
        signed_prekey_id: SignedPreKeyId,
    ) -> Result<SignedPreKeyRecord, libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = signed_prekey_id.into();

        let row = sqlx::query(
            "SELECT record FROM signed_pre_keys WHERE signed_pre_key_id = ?"
        )
        .bind(id_value as i64)
        .fetch_one(&*self.pool)
        .await
        .map_err(|e| libsignal_protocol::SignalProtocolError::InvalidSignedPreKeyId)?;

        let record_bytes: Vec<u8> = row.get("record");
        SignedPreKeyRecord::deserialize(&record_bytes)
    }

    async fn save_signed_pre_key(
        &mut self,
        signed_prekey_id: SignedPreKeyId,
        record: &SignedPreKeyRecord,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let id_value: u32 = signed_prekey_id.into();
        let record_bytes = record.serialize()?;
        let timestamp = record.timestamp()?.epoch_millis() as i64;

        sqlx::query(
            "INSERT OR REPLACE INTO signed_pre_keys (signed_pre_key_id, record, timestamp) VALUES (?, ?, ?)"
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

// Implement SessionStore for SqliteSessionStore
#[async_trait::async_trait(?Send)]
impl SessionStore for SqliteSessionStore {
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
                let record = SessionRecord::deserialize(&record_bytes)?;
                Ok(Some(record))
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
        let record_bytes = record.serialize()?;

        sqlx::query(
            "INSERT OR REPLACE INTO sessions (address, device_id, record) VALUES (?, ?, ?)"
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

// Implement SenderKeyStore for SqliteSenderKeyStore
#[async_trait::async_trait(?Send)]
impl SenderKeyStore for SqliteSenderKeyStore {
    async fn store_sender_key(
        &mut self,
        sender: &ProtocolAddress,
        distribution_id: uuid::Uuid,
        record: &SenderKeyRecord,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        let sender_str = sender.name();
        let distribution_bytes = distribution_id.as_bytes();
        let record_bytes = record.serialize()?;

        sqlx::query(
            "INSERT OR REPLACE INTO sender_keys (address, distribution_id, record) VALUES (?, ?, ?)"
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

// Implement KyberPreKeyStore for InMemoryKyberPreKeyStore
#[async_trait::async_trait(?Send)]
impl KyberPreKeyStore for InMemoryKyberPreKeyStore {
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
        _kyber_prekey_id: KyberPreKeyId,
        _ec_prekey_id: SignedPreKeyId,
        _base_key: &PublicKey,
    ) -> Result<(), libsignal_protocol::SignalProtocolError> {
        // For in-memory store, we just keep the key
        // In a production implementation, you'd mark it as used in the database
        Ok(())
    }
}

// Note: We don't implement ProtocolStore on SignalStore itself
// Instead, we use the individual fields which each implement their respective traits
// This allows independent borrowing when calling libsignal functions
