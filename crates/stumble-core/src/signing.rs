use crate::domain::{EventLog, NodeIdentity};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Utc;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum SigningError {
    #[error("invalid private key")]
    InvalidPrivateKey,
    #[error("invalid public key")]
    InvalidPublicKey,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("json serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn create_node_identity(
    display_name: impl Into<String>,
    tenant_id: Option<Uuid>,
) -> NodeIdentity {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    NodeIdentity {
        id: Uuid::now_v7(),
        tenant_id,
        display_name: display_name.into(),
        public_key: STANDARD.encode(verifying_key.as_bytes()),
        private_key_encrypted_or_local: STANDARD.encode(signing_key.to_bytes()),
        created_at: Utc::now(),
    }
}

pub fn canonical_event_bytes(
    event_id: Uuid,
    event_type: &str,
    pod_slug: &str,
    author_node_id: Uuid,
    payload_json: &Value,
    created_at: chrono::DateTime<Utc>,
    previous_event_hash: Option<&str>,
) -> Result<Vec<u8>, SigningError> {
    let canonical = serde_json::json!({
        "event_id": event_id,
        "event_type": event_type,
        "pod_slug": pod_slug,
        "author_node_id": author_node_id,
        "payload_json": payload_json,
        "created_at": created_at,
        "previous_event_hash": previous_event_hash,
    });
    Ok(serde_json::to_vec(&canonical)?)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn sign_public_event(
    node: &NodeIdentity,
    event_type: impl Into<String>,
    pod_slug: impl Into<String>,
    payload_json: Value,
    previous_event_hash: Option<String>,
) -> Result<EventLog, SigningError> {
    let event_id = Uuid::now_v7();
    let event_type = event_type.into();
    let pod_slug = pod_slug.into();
    let created_at = Utc::now();
    let bytes = canonical_event_bytes(
        event_id,
        &event_type,
        &pod_slug,
        node.id,
        &payload_json,
        created_at,
        previous_event_hash.as_deref(),
    )?;
    let key_bytes: [u8; 32] = STANDARD
        .decode(&node.private_key_encrypted_or_local)
        .map_err(|_| SigningError::InvalidPrivateKey)?
        .try_into()
        .map_err(|_| SigningError::InvalidPrivateKey)?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let signature = signing_key.sign(&bytes);
    Ok(EventLog {
        event_id,
        tenant_id: node.tenant_id,
        event_type,
        pod_slug,
        author_node_id: node.id,
        author_display_name: Some(node.display_name.clone()),
        payload_json,
        created_at,
        previous_event_hash,
        content_hash: sha256_hex(&bytes),
        signature: STANDARD.encode(signature.to_bytes()),
        imported_from_peer_id: None,
        verified: true,
    })
}

pub fn verify_event(event: &EventLog, public_key: &str) -> Result<bool, SigningError> {
    let bytes = canonical_event_bytes(
        event.event_id,
        &event.event_type,
        &event.pod_slug,
        event.author_node_id,
        &event.payload_json,
        event.created_at,
        event.previous_event_hash.as_deref(),
    )?;
    if sha256_hex(&bytes) != event.content_hash {
        return Ok(false);
    }
    let public_bytes: [u8; 32] = STANDARD
        .decode(public_key)
        .map_err(|_| SigningError::InvalidPublicKey)?
        .try_into()
        .map_err(|_| SigningError::InvalidPublicKey)?;
    let signature_bytes: [u8; 64] = STANDARD
        .decode(&event.signature)
        .map_err(|_| SigningError::InvalidSignature)?
        .try_into()
        .map_err(|_| SigningError::InvalidSignature)?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_bytes).map_err(|_| SigningError::InvalidPublicKey)?;
    let signature = Signature::from_bytes(&signature_bytes);
    Ok(verifying_key.verify(&bytes, &signature).is_ok())
}

pub fn hash_api_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"stumble-api-token-v1:");
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn new_plaintext_api_token() -> String {
    format!("st_{}", Uuid::new_v4().simple())
}
