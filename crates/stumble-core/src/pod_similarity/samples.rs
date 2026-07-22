//! Origin-signed Explore sample fetch and verification.

use super::caps::MAX_ORIGIN_EXPLORE_SAMPLES;
use crate::domain::PodAnnouncement;
use crate::domain::PodExploreSamples;
use crate::signing::SigningError;
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

/// Typed failure while retrieving Origin Explore samples.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SampleFetchError {
    /// Transport or protocol failure talking to the Origin.
    #[error("origin sample transport failed: {0}")]
    Transport(String),
    /// Returned samples failed signature or announcement binding verification.
    #[error("origin explore samples failed verification: {0}")]
    Verification(String),
    /// Request exceeded the bounded sample limit.
    #[error("explore sample limit must not exceed {MAX_ORIGIN_EXPLORE_SAMPLES}")]
    LimitExceeded,
}

/// Outbound port for fetching bounded Origin-signed Explore samples.
///
/// Production implementations request samples from the canonical Origin address
/// in the announcement. Tests inject scripted artifacts. Requests must carry
/// only announcement identity and a sample limit—never private interests.
pub trait OriginExploreSampleClient: Send + Sync {
    /// Fetches bounded samples for the given verified announcement.
    ///
    /// # Errors
    ///
    /// Returns transport or verification failures from the Origin path.
    fn fetch_explore_samples(
        &self,
        announcement: &PodAnnouncement,
        limit: usize,
    ) -> Result<PodExploreSamples, SampleFetchError>;
}

/// Captured outbound sample request for privacy assertions in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedSampleRequest {
    /// Canonical public Pod URL used for the fetch.
    pub public_pod_url: String,
    /// Announcement id requested.
    pub announcement_id: uuid::Uuid,
    /// Requested sample limit.
    pub limit: usize,
}

/// In-memory scripted Origin sample client for tests.
#[derive(Debug, Default)]
pub struct ScriptedOriginExploreSampleClient {
    /// Samples keyed by announcement id.
    pub samples: HashMap<uuid::Uuid, PodExploreSamples>,
    /// Forced failures keyed by announcement id.
    pub failures: HashMap<uuid::Uuid, SampleFetchError>,
    /// Captured outbound requests (never include private evidence).
    pub captured: Mutex<Vec<CapturedSampleRequest>>,
}

impl ScriptedOriginExploreSampleClient {
    /// Creates an empty scripted client.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a successful sample artifact for one announcement.
    pub fn push(&mut self, samples: PodExploreSamples) {
        self.samples.insert(samples.announcement_id, samples);
    }

    /// Registers a forced failure for one announcement.
    pub fn fail(&mut self, announcement_id: uuid::Uuid, error: SampleFetchError) {
        self.failures.insert(announcement_id, error);
    }
}

impl OriginExploreSampleClient for ScriptedOriginExploreSampleClient {
    fn fetch_explore_samples(
        &self,
        announcement: &PodAnnouncement,
        limit: usize,
    ) -> Result<PodExploreSamples, SampleFetchError> {
        if let Ok(mut captured) = self.captured.lock() {
            captured.push(CapturedSampleRequest {
                public_pod_url: announcement.public_pod_url.clone(),
                announcement_id: announcement.id,
                limit,
            });
        }
        if let Some(error) = self.failures.get(&announcement.id) {
            return Err(error.clone());
        }
        self.samples.get(&announcement.id).cloned().ok_or_else(|| {
            SampleFetchError::Transport(format!(
                "no scripted samples for announcement {}",
                announcement.id
            ))
        })
    }
}

/// Verifies Origin-signed samples bind the exact current announcement.
///
/// # Errors
///
/// Returns [`SampleFetchError::Verification`] when signature, binding, size, or
/// identity checks fail.
pub fn verify_explore_samples_for_announcement(
    samples: &PodExploreSamples,
    announcement: &PodAnnouncement,
) -> Result<(), SampleFetchError> {
    if samples.samples.len() > MAX_ORIGIN_EXPLORE_SAMPLES {
        return Err(SampleFetchError::LimitExceeded);
    }
    if samples.announcement_id != announcement.id {
        return Err(SampleFetchError::Verification(
            "samples do not bind the current announcement id".into(),
        ));
    }
    if samples.origin_node_id != announcement.origin_node_id
        || samples.pod_slug != announcement.pod_slug
        || samples.signer.public_key != announcement.signer.public_key
        || samples.signer.node_id != announcement.origin_node_id
    {
        return Err(SampleFetchError::Verification(
            "samples do not bind the current Origin identity".into(),
        ));
    }
    match samples.verify() {
        Ok(true) => Ok(()),
        Ok(false) => Err(SampleFetchError::Verification(
            "invalid explore sample signature".into(),
        )),
        Err(SigningError::InvalidSignature)
        | Err(SigningError::InvalidPublicKey)
        | Err(SigningError::InvalidPrivateKey) => Err(SampleFetchError::Verification(
            "invalid explore sample signature".into(),
        )),
        Err(error) => Err(SampleFetchError::Verification(error.to_string())),
    }
}

/// Fetches samples from the canonical Origin and accepts only verified bindings.
///
/// # Errors
///
/// Returns limit, transport, or verification failures. Never attaches private
/// matching context to the remote request.
pub fn fetch_verified_origin_explore_samples(
    client: &dyn OriginExploreSampleClient,
    announcement: &PodAnnouncement,
    limit: usize,
) -> Result<PodExploreSamples, SampleFetchError> {
    if limit > MAX_ORIGIN_EXPLORE_SAMPLES {
        return Err(SampleFetchError::LimitExceeded);
    }
    // Outbound request carries only announcement identity + limit (via trait).
    let samples = client.fetch_explore_samples(announcement, limit)?;
    verify_explore_samples_for_announcement(&samples, announcement)?;
    Ok(samples)
}

/// Public-only fields permitted on outbound Origin sample requests.
const SAMPLE_REQUEST_ALLOWED_KEYS: &[&str] = &["public_pod_url", "announcement_id", "limit"];

/// Asserts a sample request payload contains only public announcement fields.
///
/// Used by tests and operators to prove background discovery never ships
/// Taste Profile or interest-derived fields to Origins. Allowlist-aligned with
/// [`crate::bootstrap::request_is_public_only`].
#[must_use]
pub fn sample_request_is_public_only(payload: &serde_json::Value) -> bool {
    let Some(object) = payload.as_object() else {
        return false;
    };
    let allowed: BTreeSet<&str> = SAMPLE_REQUEST_ALLOWED_KEYS.iter().copied().collect();
    object.keys().all(|key| allowed.contains(key.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, FeedContentReference, NodeInfo, PackageVersion,
        CURRENT_PROTOCOL_VERSION,
    };
    use crate::signing::{create_node_identity, sign_pod_announcement, sign_pod_explore_samples};
    use chrono::Utc;
    use uuid::Uuid;

    fn announcement(subject: &str, slug: &str) -> (crate::domain::NodeIdentity, PodAnnouncement) {
        let node = create_node_identity("origin", None);
        let now = Utc::now();
        let announcement = sign_pod_announcement(
            &node,
            PodAnnouncement {
                id: Uuid::now_v7(),
                origin_node_id: node.id,
                signer: NodeInfo {
                    node_id: node.id,
                    display_name: node.display_name.clone(),
                    public_key: node.public_key.clone(),
                    supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
                },
                pod_slug: slug.into(),
                pod_name: slug.replace('-', " "),
                subject: subject.into(),
                public_pod_url: format!("https://origin.example/federation/pods/{slug}"),
                package_version: PackageVersion::new(1).unwrap(),
                latest_event_hash: None,
                announced_at: now,
                expires_at: now + announcement_lease_duration(),
                signature: String::new(),
            },
        )
        .unwrap();
        (node, announcement)
    }

    fn sample(title: &str, source: &str, tags: &[&str]) -> FeedContentReference {
        FeedContentReference {
            content_item_id: Uuid::now_v7().into(),
            source_url: format!("https://{source}/item"),
            canonical_url: format!("https://{source}/item"),
            title: title.into(),
            permitted_description: None,
            summary: Some(title.into()),
            media_references: vec![],
            source: source.into(),
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        }
    }

    #[test]
    fn verified_samples_require_signature_and_binding() {
        let (node, announcement) = announcement("systems", "systems");
        let samples = sign_pod_explore_samples(
            &node,
            PodExploreSamples {
                id: Uuid::now_v7(),
                announcement_id: announcement.id,
                origin_node_id: node.id,
                signer: announcement.signer.clone(),
                pod_slug: announcement.pod_slug.clone(),
                samples: vec![sample("ok", "ok.example", &["systems"])],
                sampled_at: Utc::now(),
                signature: String::new(),
            },
        )
        .unwrap();
        assert!(verify_explore_samples_for_announcement(&samples, &announcement).is_ok());

        let mut stale = samples.clone();
        stale.announcement_id = Uuid::now_v7();
        assert!(verify_explore_samples_for_announcement(&stale, &announcement).is_err());

        let mut bad_sig = samples;
        bad_sig.signature = "not-a-signature".into();
        assert!(verify_explore_samples_for_announcement(&bad_sig, &announcement).is_err());
    }

    #[test]
    fn sample_fetch_is_public_only_and_rejects_interest_payloads() {
        let mut client = ScriptedOriginExploreSampleClient::new();
        let (origin, announcement) = announcement("systems research", "systems");
        let samples = sign_pod_explore_samples(
            &origin,
            PodExploreSamples {
                id: Uuid::now_v7(),
                announcement_id: announcement.id,
                origin_node_id: origin.id,
                signer: announcement.signer.clone(),
                pod_slug: announcement.pod_slug.clone(),
                samples: vec![sample("sys", "a.example", &["systems"])],
                sampled_at: Utc::now(),
                signature: String::new(),
            },
        )
        .unwrap();
        client.push(samples);
        let fetched = fetch_verified_origin_explore_samples(&client, &announcement, 5).unwrap();
        assert_eq!(fetched.samples.len(), 1);
        let captured = client.captured.lock().unwrap();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].announcement_id, announcement.id);
        assert_eq!(captured[0].limit, 5);
        let payload = serde_json::json!({
            "public_pod_url": captured[0].public_pod_url,
            "announcement_id": captured[0].announcement_id,
            "limit": captured[0].limit,
        });
        assert!(sample_request_is_public_only(&payload));
        assert!(!sample_request_is_public_only(&serde_json::json!({
            "announcement_id": announcement.id,
            "interests": ["distributed systems"],
        })));
        assert!(!sample_request_is_public_only(&serde_json::json!({
            "public_pod_url": captured[0].public_pod_url,
            "announcement_id": captured[0].announcement_id,
            "limit": captured[0].limit,
            "query": "private",
        })));
    }
}
