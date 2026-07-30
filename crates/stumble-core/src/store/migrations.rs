//! Forward migrations for persisted record values.
//!
//! Both loaders (SQLite and the legacy JSON snapshot) funnel every record
//! through [`migrate_record_value`], so a value migration is written exactly
//! once. Legacy pod memberships migrate structurally during
//! `TryFrom<PersistedStore>` and are rewritten to disk by the load path.

use super::registry::StoreRecords;
use super::StorePersistenceError;
use crate::domain::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LegacyPodMembership {
    pub(super) user_id: UserId,
    pub(super) pod_id: PodId,
    pub(super) role: LegacyPodRole,
    #[serde(default)]
    pub(super) is_priority: bool,
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LegacyPodRole {
    Owner,
    Moderator,
    Admin,
    Member,
}

/// Applies in-place value migrations for one persisted record. Returns whether
/// the stored row is legacy-shaped and must be rewritten canonically after the
/// typed store has loaded. (`discovery_tasks` rows migrate through typed serde
/// defaults, so they are flagged without being edited here.)
pub(super) fn migrate_record_value(
    collection: &str,
    value: &mut serde_json::Value,
) -> Result<bool, StorePersistenceError> {
    match collection {
        "discovery_tasks" => Ok(value.get("target").is_none()),
        "candidates" => migrate_candidate_value(value),
        "candidate_submissions" => migrate_candidate_submission_value(value),
        "user_preferences" => migrate_user_preferences_value(value),
        _ => Ok(false),
    }
}

fn invalid_record(message: &'static str) -> StorePersistenceError {
    StorePersistenceError::Json(serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    )))
}

fn migrate_candidate_value(value: &mut serde_json::Value) -> Result<bool, StorePersistenceError> {
    let record = value
        .as_object_mut()
        .ok_or_else(|| invalid_record("Candidate row must be an object"))?;
    let Some(canonical_url) = record
        .get("canonical_url")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
    else {
        return Ok(false);
    };
    if record.get("source_url").and_then(serde_json::Value::as_str) == Some(canonical_url.as_str())
    {
        return Ok(false);
    }
    record.insert(
        "source_url".into(),
        serde_json::Value::String(canonical_url),
    );
    Ok(true)
}

fn migrate_candidate_submission_value(
    value: &mut serde_json::Value,
) -> Result<bool, StorePersistenceError> {
    if value.get("target").is_some() {
        return Ok(false);
    }
    let record = value
        .as_object_mut()
        .ok_or_else(|| invalid_record("Candidate Submission row must be an object"))?;
    let placements = record
        .remove("proposed_placements")
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let task_context = record
        .remove("task_context")
        .unwrap_or(serde_json::Value::Null);
    record.insert(
        "target".into(),
        serde_json::json!({
            "kind": "pod_placements",
            "placements": placements,
            "task_context": task_context,
        }),
    );
    Ok(true)
}

fn migrate_user_preferences_value(
    value: &mut serde_json::Value,
) -> Result<bool, StorePersistenceError> {
    let record = value
        .as_object_mut()
        .ok_or_else(|| invalid_record("User Preferences row must be an object"))?;
    if record.contains_key("blocked_source_affinities") {
        return Ok(false);
    }
    record.insert(
        "blocked_source_affinities".into(),
        serde_json::Value::Array(Vec::new()),
    );
    Ok(true)
}

pub(super) fn migrate_legacy_pod_memberships(
    legacy_memberships: &[LegacyPodMembership],
    pods: &[Pod],
    node_identities: &[NodeIdentity],
    subscriptions: &mut Vec<Subscription>,
    pod_roles: &mut Vec<PodRoleAssignment>,
) {
    for membership in legacy_memberships {
        let Some(pod) = pods.iter().find(|pod| pod.id == membership.pod_id) else {
            continue;
        };
        if let Some(subscription) = subscriptions.iter_mut().find(|subscription| {
            subscription.user_id == membership.user_id
                && subscription.local_pod_id == membership.pod_id
        }) {
            subscription.is_priority |= membership.is_priority;
        } else {
            let origin = pod
                .origin_node_id
                .and_then(|node_id| node_identities.iter().find(|node| node.id == node_id))
                .or_else(|| {
                    node_identities
                        .iter()
                        .find(|node| node.tenant_id == pod.tenant_id)
                });
            if let Some(origin) = origin {
                let mut subscription = Subscription::new_local(
                    legacy_subscription_id(membership.user_id, membership.pod_id),
                    membership.user_id,
                    pod,
                    origin,
                    membership.created_at,
                );
                subscription.is_priority = membership.is_priority;
                subscriptions.push(subscription);
            }
        }

        let role = match membership.role {
            LegacyPodRole::Owner => Some(PodRole::Owner),
            LegacyPodRole::Moderator | LegacyPodRole::Admin => Some(PodRole::Curator),
            LegacyPodRole::Member => None,
        };
        if let Some(role) = role {
            if let Some(assignment) = pod_roles.iter_mut().find(|assignment| {
                assignment.user_id == membership.user_id && assignment.pod_id == membership.pod_id
            }) {
                assignment.role = role;
            } else {
                pod_roles.push(PodRoleAssignment {
                    user_id: membership.user_id,
                    pod_id: membership.pod_id,
                    role,
                    created_at: membership.created_at,
                });
            }
        }
    }
}

fn legacy_subscription_id(user_id: UserId, pod_id: PodId) -> SubscriptionId {
    let mut hasher = Sha256::new();
    hasher.update(b"stumble legacy Subscription\0");
    hasher.update(user_id.as_bytes());
    hasher.update(pod_id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).into()
}

/// Rewrites legacy-shaped rows with their canonical serialization, keyed by
/// the row's existing record key.
pub(super) fn persist_migrated_records(
    transaction: &rusqlite::Transaction<'_>,
    records: &StoreRecords,
    legacy_rows: &[(String, String)],
) -> Result<(), StorePersistenceError> {
    for (collection, record_key) in legacy_rows {
        let value_json = records
            .get(&(collection.clone(), record_key.clone()))
            .expect("loaded migrated value has a canonical store record");
        let updated = transaction.execute(
            "UPDATE stumble_store_records SET value_json = ?1
             WHERE collection = ?2 AND record_key = ?3",
            rusqlite::params![value_json, collection, record_key],
        )?;
        debug_assert_eq!(updated, 1, "loaded migrated row still exists");
    }
    Ok(())
}

/// Replaces legacy `pod_memberships` rows with the subscriptions and pod
/// roles they migrated into.
pub(super) fn persist_migrated_pod_relationships(
    transaction: &rusqlite::Transaction<'_>,
    records: &StoreRecords,
) -> Result<(), StorePersistenceError> {
    transaction.execute(
        "DELETE FROM stumble_store_records WHERE collection = 'pod_memberships'",
        [],
    )?;
    for ((collection, record_key), value_json) in records
        .iter()
        .filter(|((collection, _), _)| collection == "subscriptions" || collection == "pod_roles")
    {
        transaction.execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json) VALUES (?1, ?2, ?3)
             ON CONFLICT (collection, record_key) DO UPDATE SET value_json = excluded.value_json",
            rusqlite::params![collection, record_key, value_json],
        )?;
    }
    Ok(())
}

/// Atomically rewrites one collection's rows with their canonical keys and
/// values, used when a collection's key scheme changes (for example the
/// content-keyed log collections that moved to positional keys).
pub(super) fn rewrite_collection(
    transaction: &rusqlite::Transaction<'_>,
    collection: &str,
    records: &StoreRecords,
) -> Result<(), StorePersistenceError> {
    transaction.execute(
        "DELETE FROM stumble_store_records WHERE collection = ?1",
        rusqlite::params![collection],
    )?;
    for ((_, record_key), value_json) in records
        .iter()
        .filter(|((record_collection, _), _)| record_collection == collection)
    {
        transaction.execute(
            "INSERT INTO stumble_store_records (collection, record_key, value_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![collection, record_key, value_json],
        )?;
    }
    Ok(())
}
