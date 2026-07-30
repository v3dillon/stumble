//! Domain queries over the in-memory store: node and slug resolution, tenant
//! boundary checks, and event-chain reads. No persistence concerns here.

use super::{InMemoryStore, StoreError};
use crate::domain::*;
use std::collections::HashSet;

impl InMemoryStore {
    /// Stores a Pod Package version once and refuses replacement.
    pub(crate) fn insert_pod_package_version(
        &mut self,
        package: PodPackage,
    ) -> Result<(), StoreError> {
        let version = PackageVersion::new(package.version)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        let key = (package.pod_id, version);
        if self.pod_package_versions.contains_key(&key) {
            return Err(StoreError::Duplicate(format!(
                "Pod Package version {} for Pod {}",
                version.value(),
                package.pod_id
            )));
        }
        self.pod_package_versions.insert(key, package);
        Ok(())
    }

    pub(crate) fn pod_package_version(
        &self,
        pod_id: PodId,
        version: PackageVersion,
    ) -> Option<&PodPackage> {
        self.pod_package_versions.get(&(pod_id, version))
    }

    pub fn default_node(&self) -> Result<NodeIdentity, StoreError> {
        self.node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .or_else(|| self.node_identities.values().next())
            .cloned()
            .ok_or_else(|| StoreError::NotFound("node identity".to_string()))
    }

    pub fn node_for_tenant(&self, tenant_id: Option<TenantId>) -> Result<NodeIdentity, StoreError> {
        self.node_identities
            .values()
            .find(|node| node.tenant_id == tenant_id)
            .or_else(|| {
                self.node_identities
                    .values()
                    .find(|node| node.tenant_id.is_none())
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound("node identity".to_string()))
    }

    pub fn pod_by_slug(&self, slug: &str, tenant_id: Option<TenantId>) -> Result<Pod, StoreError> {
        self.pods
            .values()
            .find(|pod| pod.slug == slug && pod.tenant_id == tenant_id)
            .or_else(|| {
                self.pods
                    .values()
                    .find(|pod| pod.slug == slug && pod.tenant_id.is_none())
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("pod {slug}")))
    }

    pub fn tenant_by_slug(&self, slug: &str) -> Result<Tenant, StoreError> {
        self.tenants
            .values()
            .find(|tenant| tenant.slug == slug)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("tenant {slug}")))
    }

    pub fn assert_tenant(
        &self,
        actual: Option<TenantId>,
        expected: Option<TenantId>,
    ) -> Result<(), StoreError> {
        if actual == expected || actual.is_none() {
            Ok(())
        } else {
            Err(StoreError::TenantBoundary)
        }
    }

    pub fn submissions_for_pod(&self, pod_id: PodId) -> Vec<&Submission> {
        let ids: HashSet<_> = self
            .submission_pods
            .iter()
            .filter(|link| link.pod_id == pod_id)
            .map(|link| link.submission_id)
            .collect();
        self.submissions
            .values()
            .filter(|submission| ids.contains(&submission.id))
            .collect()
    }

    /// Federated events for a public Pod, starting at its most recent
    /// publication. History from before the Pod became public stays local:
    /// `pod_published` carries the full current Pod and package, and publish
    /// re-emits placements for the accepted content that should federate.
    pub fn public_events_for_pod(&self, pod_slug: &str) -> Vec<EventLog> {
        let events: Vec<EventLog> = self
            .event_log
            .iter()
            .filter(|event| event.pod_slug == pod_slug && event.event_type.is_federated())
            .cloned()
            .collect();
        let publication_start = events
            .iter()
            .rposition(|event| event.event_type == PodEventType::PodPublished);
        match publication_start {
            Some(start) => events[start..].to_vec(),
            None => events,
        }
    }

    pub fn portable_package_events_for_pod(&self, pod_slug: &str) -> Vec<EventLog> {
        self.event_log
            .iter()
            .filter(|event| event.pod_slug == pod_slug && event.event_type.is_portable_package())
            .cloned()
            .collect()
    }

    pub fn latest_federated_event_hash(&self, pod_slug: &str) -> Option<String> {
        self.event_log
            .iter()
            .rev()
            .find(|event| event.pod_slug == pod_slug && event.event_type.is_federated())
            .map(|event| event.content_hash.clone())
    }

    pub fn latest_event_hash(&self, pod_slug: &str) -> Option<String> {
        self.event_log
            .iter()
            .rev()
            .find(|event| event.pod_slug == pod_slug)
            .map(|event| event.content_hash.clone())
    }
}
