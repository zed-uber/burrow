use anyhow::Result;
use libsignal_protocol::{
    GenericSignedPreKey, IdentityKeyPair, KeyPair, PreKeyId, PreKeyRecord, SignedPreKeyId,
    SignedPreKeyRecord, Timestamp,
};
use rand::rngs::OsRng;
use rand::{Rng, TryRngCore as _};
use std::time::SystemTime;

/// Generate a registration ID (random 14-bit number)
pub fn generate_registration_id() -> u32 {
    let mut rng = OsRng.unwrap_err();
    // Signal uses 14-bit registration IDs
    rng.random_range(1..16384)
}

/// Generate a new identity key pair
pub fn generate_identity_keypair() -> Result<IdentityKeyPair> {
    let mut rng = OsRng.unwrap_err();
    Ok(IdentityKeyPair::generate(&mut rng))
}

/// Generate multiple prekeys
pub fn generate_prekeys(start_id: u32, count: u32) -> Result<Vec<PreKeyRecord>> {
    let mut rng = OsRng.unwrap_err();
    let mut prekeys = Vec::new();

    for i in 0..count {
        let id = PreKeyId::from(start_id + i);
        let keypair = KeyPair::generate(&mut rng);
        prekeys.push(PreKeyRecord::new(id, &keypair));
    }

    Ok(prekeys)
}

/// Generate a signed prekey
pub fn generate_signed_prekey(
    id: u32,
    identity_keypair: &IdentityKeyPair,
) -> Result<SignedPreKeyRecord> {
    let mut rng = OsRng.unwrap_err();
    let signed_prekey_id = SignedPreKeyId::from(id);
    let keypair = KeyPair::generate(&mut rng);

    // Sign the public key
    let signature = identity_keypair
        .private_key()
        .calculate_signature(&keypair.public_key.serialize(), &mut rng)?;

    // Get current timestamp
    let timestamp_millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    Ok(SignedPreKeyRecord::new(
        signed_prekey_id,
        Timestamp::from_epoch_millis(timestamp_millis),
        &keypair,
        &signature,
    ))
}
