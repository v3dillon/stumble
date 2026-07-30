use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub id: NodeIdentityId,
    pub tenant_id: Option<TenantId>,
    pub display_name: String,
    pub public_key: String,
    pub private_key_encrypted_or_local: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub id: PeerId,
    /// Canonical identity advertised and signed by the remote Node.
    #[serde(default)]
    pub node_id: NodeIdentityId,
    pub tenant_id: Option<TenantId>,
    pub display_name: String,
    pub base_url: String,
    pub public_key: String,
    pub trust_level: TrustLevel,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pod {
    pub id: PodId,
    pub tenant_id: Option<TenantId>,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub visibility: Visibility,
    pub created_by: Option<UserId>,
    pub created_at: DateTime<Utc>,
    pub origin_node_id: Option<NodeIdentityId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodRoleAssignment {
    pub user_id: UserId,
    pub pod_id: PodId,
    pub role: PodRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodRules {
    pub pod_id: PodId,
    pub blocked_topics: Vec<String>,
    pub blocked_domains: Vec<String>,
    pub auto_promote_crawler_candidates: bool,
    pub federate_sources: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PodSkillPack {
    /// Stable identity shared by the versions of this package.
    pub id: Uuid,
    /// Pod governed by this package.
    pub pod_id: PodId,
    /// Legacy wire-compatible numeric version. New storage APIs use [`PackageVersion`].
    pub version: i32,
    /// Subject language, scope, and boundaries. This is deliberately separate
    /// from the operational instructions in `skill_md`.
    #[serde(default)]
    pub context_md: String,
    /// Legacy Pod metadata retained for compatibility.
    pub pod_yaml: String,
    /// Scoped, untrusted discovery and curation instructions.
    pub skill_md: String,
    /// Declarative Source Rule suggestions.
    pub sources_yaml: String,
    /// Pod-owned filtering suggestions.
    pub filters_yaml: String,
    /// Positive calibration examples.
    pub examples_good_md: String,
    /// Negative calibration examples.
    pub examples_bad_md: String,
    /// User who owns the authoritative package version.
    #[serde(default)]
    pub owner_id: Option<UserId>,
    /// Harness that proposed this package version, if any.
    #[serde(default)]
    pub proposer_harness_id: Option<AgentHarnessId>,
    /// Timestamp at which this immutable version was created.
    pub created_at: DateTime<Utc>,
    /// Legacy alias of `created_at` retained for wire compatibility.
    pub updated_at: DateTime<Utc>,
}

/// Canonical name for the signed, versioned bundle historically exposed as a
/// `PodSkillPack`. The legacy name remains for wire and source compatibility.
pub type PodPackage = PodSkillPack;

/// Result of requesting one version-aware Pod Package Revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum PodPackageRevisionOutcome {
    /// A non-public origin package was revised immediately.
    Revised(Box<PodPackage>),
    /// A public origin package is unchanged until this proposal is approved.
    PendingApproval(Box<PendingProposal>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FederatedPodEventType {
    PodCreated,
    PodPublished,
    PodSkillPackUpdated,
    PodPackageImported,
    PodPackageForked,
    ContentItemPlaced,
    ContentItemMetadataUpdated,
    PlacementTombstoned,
    LegacyLinkRemoved,
    LegacyLinkSubmitted,
}

impl FederatedPodEventType {
    pub(crate) fn from_wire(value: &str) -> Option<Self> {
        match value {
            "pod_created" => Some(Self::PodCreated),
            "pod_published" => Some(Self::PodPublished),
            "pod_skill_pack_updated" => Some(Self::PodSkillPackUpdated),
            "pod_package_imported" => Some(Self::PodPackageImported),
            "pod_package_forked" => Some(Self::PodPackageForked),
            "content_item_placed" => Some(Self::ContentItemPlaced),
            "content_item_metadata_updated" => Some(Self::ContentItemMetadataUpdated),
            "placement_tombstoned" => Some(Self::PlacementTombstoned),
            "link_removed" => Some(Self::LegacyLinkRemoved),
            "link_submitted" => Some(Self::LegacyLinkSubmitted),
            _ => None,
        }
    }

    pub(crate) const fn as_wire(self) -> &'static str {
        match self {
            Self::PodCreated => "pod_created",
            Self::PodPublished => "pod_published",
            Self::PodSkillPackUpdated => "pod_skill_pack_updated",
            Self::PodPackageImported => "pod_package_imported",
            Self::PodPackageForked => "pod_package_forked",
            Self::ContentItemPlaced => "content_item_placed",
            Self::ContentItemMetadataUpdated => "content_item_metadata_updated",
            Self::PlacementTombstoned => "placement_tombstoned",
            Self::LegacyLinkRemoved => "link_removed",
            Self::LegacyLinkSubmitted => "link_submitted",
        }
    }

    pub(crate) const fn is_federated(self) -> bool {
        match self {
            Self::PodCreated
            | Self::PodPublished
            | Self::PodSkillPackUpdated
            | Self::PodPackageImported
            | Self::PodPackageForked
            | Self::ContentItemPlaced
            | Self::ContentItemMetadataUpdated
            | Self::PlacementTombstoned
            | Self::LegacyLinkRemoved => true,
            Self::LegacyLinkSubmitted => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLog {
    pub event_id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub event_type: String,
    pub pod_slug: String,
    pub author_node_id: NodeIdentityId,
    pub author_display_name: Option<String>,
    pub payload_json: Value,
    pub created_at: DateTime<Utc>,
    pub previous_event_hash: Option<String>,
    pub content_hash: String,
    pub signature: String,
    pub imported_from_peer_id: Option<PeerId>,
    pub verified: bool,
}
