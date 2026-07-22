//! Origin reachability probe used by open Bootstrap admission.

use crate::domain::{NodeIdentityId, PodAnnouncement, CURRENT_PROTOCOL_VERSION};

/// Public manifest facts a Bootstrap Node requires from a reachable Origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginPublicManifestView {
    /// Protocol version advertised by the Origin Node.
    pub protocol_version: String,
    /// Public Pod slug served at the canonical URL.
    pub pod_slug: String,
    /// Human-readable Pod name from the live manifest.
    pub pod_name: String,
    /// Subject/description from the live manifest.
    pub subject: String,
    /// Current Package version advertised by the Origin.
    pub package_version: i32,
    /// Latest federated event pointer, when present.
    pub latest_event_hash: Option<String>,
    /// Whether the Pod is currently public at the Origin.
    pub visibility_public: bool,
    /// Origin Node identity when the Origin publishes one.
    pub origin_node_id: Option<NodeIdentityId>,
}

/// Failure while probing an Origin's public endpoint or manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginProbeError {
    /// Transport or DNS failure; Origin is not currently reachable.
    Unreachable,
    /// Origin responded but did not yield a usable public manifest.
    ManifestUnavailable,
}

/// Port for verifying Origin reachability and fetching the current public manifest.
///
/// Production nodes inject an HTTP client; tests inject deterministic fakes.
pub trait OriginProbe: Send + Sync {
    /// Probes the Origin behind `public_pod_url` for the announced Pod.
    ///
    /// # Errors
    ///
    /// Returns [`OriginProbeError`] when the Origin cannot be reached or does not
    /// expose a usable public manifest.
    fn probe_public_manifest(
        &self,
        public_pod_url: &str,
        pod_slug: &str,
    ) -> Result<OriginPublicManifestView, OriginProbeError>;
}

/// Probe that always reports the Origin unreachable.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnreachableOriginProbe;

impl OriginProbe for UnreachableOriginProbe {
    fn probe_public_manifest(
        &self,
        _public_pod_url: &str,
        _pod_slug: &str,
    ) -> Result<OriginPublicManifestView, OriginProbeError> {
        Err(OriginProbeError::Unreachable)
    }
}

/// Configurable Origin probe that returns a fixed view or error.
#[derive(Debug, Clone)]
pub struct FixedOriginProbe {
    /// Successful view when `error` is `None`.
    pub view: Option<OriginPublicManifestView>,
    /// Forced probe failure.
    pub error: Option<OriginProbeError>,
}

impl OriginProbe for FixedOriginProbe {
    fn probe_public_manifest(
        &self,
        _public_pod_url: &str,
        _pod_slug: &str,
    ) -> Result<OriginPublicManifestView, OriginProbeError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.view
            .clone()
            .ok_or(OriginProbeError::ManifestUnavailable)
    }
}

/// Builds a probe view that exactly matches a signed announcement.
#[must_use]
pub fn probe_view_matching(announcement: &PodAnnouncement) -> OriginPublicManifestView {
    OriginPublicManifestView {
        protocol_version: announcement.signer.supported_protocol_version.clone(),
        pod_slug: announcement.pod_slug.clone(),
        pod_name: announcement.pod_name.clone(),
        subject: announcement.subject.clone(),
        package_version: announcement.package_version.value(),
        latest_event_hash: announcement.latest_event_hash.clone(),
        visibility_public: true,
        origin_node_id: Some(announcement.origin_node_id),
    }
}

/// Origin probe that always mirrors the last announcement given to it via
/// [`Self::set_announcement`]. Useful when a test produces announcements at runtime.
#[derive(Debug, Default)]
pub struct ScriptedMatchingOriginProbe {
    announcement: std::sync::Mutex<Option<PodAnnouncement>>,
}

impl ScriptedMatchingOriginProbe {
    /// Records the announcement whose public facts the next probe should mirror.
    pub fn set_announcement(&self, announcement: &PodAnnouncement) {
        *self.announcement.lock().expect("probe lock") = Some(announcement.clone());
    }
}

impl OriginProbe for ScriptedMatchingOriginProbe {
    fn probe_public_manifest(
        &self,
        _public_pod_url: &str,
        pod_slug: &str,
    ) -> Result<OriginPublicManifestView, OriginProbeError> {
        let guard = self.announcement.lock().expect("probe lock");
        let Some(announcement) = guard.as_ref() else {
            return Err(OriginProbeError::ManifestUnavailable);
        };
        if announcement.pod_slug != pod_slug {
            return Err(OriginProbeError::ManifestUnavailable);
        }
        Ok(probe_view_matching(announcement))
    }
}

/// Validates that a live public manifest matches the signed announcement.
///
/// # Errors
///
/// Returns [`crate::domain::BootstrapAdmissionRejectionReason::ManifestMismatch`]
/// when public facts disagree, or
/// [`crate::domain::BootstrapAdmissionRejectionReason::IncompatibleProtocol`]
/// when the Origin advertises an unsupported protocol version.
pub fn manifest_matches(
    announcement: &PodAnnouncement,
    view: &OriginPublicManifestView,
) -> Result<(), crate::domain::BootstrapAdmissionRejectionReason> {
    use crate::domain::BootstrapAdmissionRejectionReason;

    if !view.visibility_public {
        return Err(BootstrapAdmissionRejectionReason::ManifestMismatch);
    }
    if view.pod_slug != announcement.pod_slug {
        return Err(BootstrapAdmissionRejectionReason::ManifestMismatch);
    }
    if view.pod_name != announcement.pod_name {
        return Err(BootstrapAdmissionRejectionReason::ManifestMismatch);
    }
    if view.subject != announcement.subject {
        return Err(BootstrapAdmissionRejectionReason::ManifestMismatch);
    }
    if view.package_version != announcement.package_version.value() {
        return Err(BootstrapAdmissionRejectionReason::ManifestMismatch);
    }
    if view.latest_event_hash != announcement.latest_event_hash {
        return Err(BootstrapAdmissionRejectionReason::ManifestMismatch);
    }
    if let Some(origin_node_id) = view.origin_node_id {
        if origin_node_id != announcement.origin_node_id {
            return Err(BootstrapAdmissionRejectionReason::ManifestMismatch);
        }
    }
    // Protocol compatibility is a single check against the Bootstrap-supported version.
    if view.protocol_version != CURRENT_PROTOCOL_VERSION {
        return Err(BootstrapAdmissionRejectionReason::IncompatibleProtocol);
    }
    Ok(())
}
