//! Optional signed Pod Event Relay: verified Origin snapshots in, verbatim out.
//!
//! The Relay stores the signed public snapshot and an optional Origin-signed
//! Explore sample artifact for an Origin Node. It never re-signs, never becomes
//! the Origin, and never holds Home Node private state (ADR-0031, ADR-0055).

use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Admits an Origin-signed public Pod snapshot into the Relay cache.
    ///
    /// The snapshot is verified with the Origin public key it carries and is
    /// stored unchanged. An upsert replaces the prior snapshot only when the
    /// new event chain extends it or replays it exactly (idempotent).
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::RelayDisabled`] when Relay is off, and
    /// validation errors for identity mismatches, invalid signatures,
    /// discontinuous chains, or non-extending replacements.
    pub fn admit_relay_snapshot(
        &self,
        origin_node_id: NodeIdentityId,
        pod_slug: &str,
        snapshot: FederationPodSnapshot,
        now: chrono::DateTime<Utc>,
    ) -> Result<RelayPublication, AgentToolsError> {
        if !self.relay_enabled() {
            return Err(AgentToolsError::RelayDisabled);
        }
        // Bounded open admission, checked before any verification or storage.
        if crate::bootstrap::estimated_payload_bytes(&snapshot) > MAX_RELAY_SNAPSHOT_PAYLOAD_BYTES {
            return Err(AgentToolsError::RelayPayloadTooLarge);
        }
        if snapshot.node.node_id != origin_node_id {
            return Err(StoreError::Validation(
                "snapshot Origin Node does not match the Relay URL origin_node_id".into(),
            )
            .into());
        }
        if snapshot.manifest.pod.slug != pod_slug {
            return Err(StoreError::Validation(
                "snapshot Pod slug does not match the Relay URL slug".into(),
            )
            .into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        // Same Origin signature and chain checks direct subscription uses.
        validate_federation_snapshot(&store, None, None, &snapshot)?;
        let key = (origin_node_id, pod_slug.to_string());
        let mut existing_samples = None;
        if let Some(existing) = store.relay_publications.get(&key) {
            if existing.snapshot.node.public_key != snapshot.node.public_key {
                return Err(StoreError::Validation(
                    "snapshot Origin public key does not match the stored Relay publication".into(),
                )
                .into());
            }
            let stored_hashes: Vec<&str> = existing
                .snapshot
                .events
                .iter()
                .map(|event| event.content_hash.as_str())
                .collect();
            let new_hashes: Vec<&str> = snapshot
                .events
                .iter()
                .map(|event| event.content_hash.as_str())
                .collect();
            if new_hashes.len() < stored_hashes.len()
                || new_hashes[..stored_hashes.len()] != stored_hashes[..]
            {
                return Err(StoreError::Validation(
                    "snapshot does not extend or replay the stored signed event chain".into(),
                )
                .into());
            }
            existing_samples = existing.explore_samples.clone();
        }
        let publication = RelayPublication {
            origin_node_id,
            pod_slug: pod_slug.to_string(),
            snapshot,
            explore_samples: existing_samples,
            received_at: now,
        };
        store.relay_publications.insert(key, publication.clone());
        self.persist_locked(&mut store)?;
        Ok(publication)
    }

    /// Builds an Origin manifest view from this process's own Relay cache when
    /// the announcement URL is Relay-shaped and the snapshot is stored locally.
    ///
    /// Returns `None` when Relay is off, the URL is Origin-shaped, or the cache
    /// has no matching publication; the caller then uses its injected probe.
    pub(crate) fn local_relay_probe_view(
        &self,
        store: &InMemoryStore,
        announcement: &PodAnnouncement,
    ) -> Option<crate::bootstrap::OriginPublicManifestView> {
        if !self.relay_enabled() {
            return None;
        }
        let url = url::Url::parse(&announcement.public_pod_url).ok()?;
        let (origin_node_id, pod_slug) =
            crate::pod_announcement::relay_public_pod_url_parts(url.path())?;
        let publication = store.relay_publications.get(&(origin_node_id, pod_slug))?;
        let snapshot = &publication.snapshot;
        Some(crate::bootstrap::OriginPublicManifestView {
            protocol_version: snapshot.node.supported_protocol_version.clone(),
            pod_slug: snapshot.manifest.pod.slug.clone(),
            pod_name: snapshot.manifest.pod.name.clone(),
            subject: snapshot.manifest.pod.description.clone(),
            package_version: snapshot.manifest.skill_pack_version,
            latest_event_hash: snapshot.manifest.latest_known_event_hash.clone(),
            visibility_public: snapshot.manifest.pod.visibility == Visibility::Public,
            origin_node_id: snapshot.manifest.pod.origin_node_id,
        })
    }

    /// Returns the stored Relay publication for one Origin Pod.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::RelayDisabled`] when Relay is off and
    /// `NotFound` when no snapshot is stored for the key.
    pub fn relay_publication(
        &self,
        origin_node_id: NodeIdentityId,
        pod_slug: &str,
    ) -> Result<RelayPublication, AgentToolsError> {
        if !self.relay_enabled() {
            return Err(AgentToolsError::RelayDisabled);
        }
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        store
            .relay_publications
            .get(&(origin_node_id, pod_slug.to_string()))
            .cloned()
            .ok_or_else(|| {
                StoreError::NotFound(format!("relay publication {origin_node_id}/{pod_slug}"))
                    .into()
            })
    }

    /// Admits an Origin-signed Explore sample artifact into a stored Relay publication.
    ///
    /// Samples require a stored snapshot for the same Origin and slug. The Relay
    /// verifies the Origin signature and identity, then stores the artifact
    /// unchanged. The latest Origin push replaces any prior samples.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::RelayDisabled`] when Relay is off,
    /// [`AgentToolsError::RelayPayloadTooLarge`] when the payload exceeds the
    /// bound, `NotFound` when no snapshot is stored, and validation or signature
    /// errors for identity mismatches.
    pub fn admit_relay_explore_samples(
        &self,
        origin_node_id: NodeIdentityId,
        pod_slug: &str,
        samples: PodExploreSamples,
        _now: chrono::DateTime<Utc>,
    ) -> Result<PodExploreSamples, AgentToolsError> {
        if !self.relay_enabled() {
            return Err(AgentToolsError::RelayDisabled);
        }
        if crate::bootstrap::estimated_payload_bytes(&samples)
            > MAX_RELAY_EXPLORE_SAMPLES_PAYLOAD_BYTES
        {
            return Err(AgentToolsError::RelayPayloadTooLarge);
        }
        if samples.samples.len() > crate::MAX_ORIGIN_EXPLORE_SAMPLES {
            return Err(StoreError::Validation(
                "Pod Explore sample artifact must not exceed 10 references".into(),
            )
            .into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let key = (origin_node_id, pod_slug.to_string());
        let Some(publication) = store.relay_publications.get_mut(&key) else {
            return Err(StoreError::NotFound(format!(
                "relay publication {origin_node_id}/{pod_slug}"
            ))
            .into());
        };
        if !samples.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        if samples.origin_node_id != origin_node_id {
            return Err(StoreError::Validation(
                "samples Origin Node does not match the Relay URL origin_node_id".into(),
            )
            .into());
        }
        if samples.pod_slug != pod_slug {
            return Err(StoreError::Validation(
                "samples Pod slug does not match the Relay URL slug".into(),
            )
            .into());
        }
        if samples.origin_node_id != publication.snapshot.node.node_id
            || samples.signer.node_id != publication.snapshot.node.node_id
            || samples.signer.public_key != publication.snapshot.node.public_key
        {
            return Err(StoreError::Validation(
                "samples Origin identity does not match the stored snapshot Origin".into(),
            )
            .into());
        }
        publication.explore_samples = Some(samples.clone());
        self.persist_locked(&mut store)?;
        Ok(samples)
    }

    /// Returns the stored Origin-signed Explore samples for one Origin Pod.
    ///
    /// The Relay never produces samples and never calls `pod_explore_samples`.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::RelayDisabled`] when Relay is off and
    /// `NotFound` when no samples are stored for the key.
    pub fn relay_explore_samples(
        &self,
        origin_node_id: NodeIdentityId,
        pod_slug: &str,
    ) -> Result<PodExploreSamples, AgentToolsError> {
        let publication = self.relay_publication(origin_node_id, pod_slug)?;
        publication.explore_samples.ok_or_else(|| {
            StoreError::NotFound(format!("relay explore samples {origin_node_id}/{pod_slug}"))
                .into()
        })
    }
}
