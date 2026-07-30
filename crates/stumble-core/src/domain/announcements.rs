use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: NodeIdentityId,
    pub display_name: String,
    pub public_key: String,
    pub supported_protocol_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodManifest {
    pub pod: Pod,
    pub latest_known_event_hash: Option<String>,
    pub skill_pack_version: i32,
    pub public_source_summary: Vec<String>,
}

/// Renewable Announcement Lease duration in whole days.
pub const ANNOUNCEMENT_LEASE_DURATION_DAYS: i64 = 30;

/// Returns the renewable validity period carried by every signed Pod Announcement.
#[must_use]
pub fn announcement_lease_duration() -> chrono::Duration {
    chrono::Duration::days(ANNOUNCEMENT_LEASE_DURATION_DAYS)
}

/// Compact signed advertisement for one public Pod on the Stumble Substrate.
///
/// Announcements identify where authoritative artifacts can be fetched without
/// carrying the Pod Package, Pod Events, or Content Items themselves. Each
/// announcement carries a renewable 30-day Announcement Lease in `expires_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PodAnnouncement {
    /// Stable identity of this signed advertisement.
    pub id: Uuid,
    /// Authoritative Origin Node.
    pub origin_node_id: NodeIdentityId,
    /// Origin identity and verification key.
    pub signer: NodeInfo,
    /// Public Pod identity at the Origin Node.
    pub pod_slug: String,
    /// Human-readable Pod name.
    pub pod_name: String,
    /// Compact subject description used for discovery.
    pub subject: String,
    /// Canonical direct address, independent of any Index Node.
    pub public_pod_url: String,
    /// Current signed Pod Package version.
    pub package_version: PackageVersion,
    /// Latest authoritative Pod Event pointer.
    pub latest_event_hash: Option<String>,
    /// Time at which the Origin Node signed this advertisement.
    pub announced_at: DateTime<Utc>,
    /// Exclusive end of the renewable Announcement Lease (`announced_at` + 30 days).
    /// The lease is active while `expires_at > now`.
    pub expires_at: DateTime<Utc>,
    /// Ed25519 signature over every preceding field.
    pub signature: String,
}

impl PodAnnouncement {
    /// Returns whether this announcement's Announcement Lease is still active at `now`.
    ///
    /// The lease end is exclusive: active only while `expires_at > now`.
    #[must_use]
    pub fn lease_is_active(&self, now: DateTime<Utc>) -> bool {
        self.expires_at > now
    }
}

/// Origin-signed statement that a formerly public Pod leaves new discovery.
///
/// A withdrawal ends announcement relaying and Explore eligibility for the Pod
/// without deleting Subscriptions or previously synchronized content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PodWithdrawal {
    /// Stable identity of this signed withdrawal.
    pub id: Uuid,
    /// Authoritative Origin Node.
    pub origin_node_id: NodeIdentityId,
    /// Origin identity and verification key.
    pub signer: NodeInfo,
    /// Public Pod identity withdrawn from discovery.
    pub pod_slug: String,
    /// Optional canonical direct address covered by the withdrawal.
    pub public_pod_url: Option<String>,
    /// Optional exact announcement identity this withdrawal supersedes.
    pub covers_announcement_id: Option<Uuid>,
    /// Time at which the Origin Node signed the withdrawal.
    pub withdrawn_at: DateTime<Utc>,
    /// Ed25519 signature over every preceding field.
    pub signature: String,
}

/// Locally retained verified Pod Withdrawal and delivery provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct KnownPodWithdrawal {
    /// Origin-authored signed withdrawal, unchanged by relays.
    pub withdrawal: PodWithdrawal,
    /// Trusted peer that delivered it, absent when indexed directly.
    pub received_from_peer_id: Option<PeerId>,
    /// Time at which this node verified and retained it.
    pub received_at: DateTime<Utc>,
}

/// Locally retained verified announcement and its immediate delivery provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct KnownPodAnnouncement {
    /// Origin-authored signed advertisement, unchanged by relays.
    pub announcement: PodAnnouncement,
    /// Trusted peer that delivered it, absent when indexed directly.
    pub received_from_peer_id: Option<PeerId>,
    /// Configured Index Node base URLs that returned this announcement (multi-source).
    ///
    /// Multiple Indexes accumulate across retains of the same signed announcement
    /// identity. Removing an Index excludes announcements whose *only* remaining
    /// delivery source was that Index from current eligibility while preserving
    /// this audit row. Accepts legacy singular `received_from_index_url` on load.
    #[serde(
        default,
        alias = "received_from_index_url",
        deserialize_with = "deserialize_index_provenance_urls"
    )]
    pub received_from_index_urls: BTreeSet<String>,
    /// Bootstrap base URLs that delivered this announcement (multi-source).
    ///
    /// Removing a configured Bootstrap excludes announcements whose *only*
    /// remaining delivery source was that endpoint from current eligibility
    /// while preserving this audit row.
    #[serde(default)]
    pub received_from_bootstrap_urls: BTreeSet<String>,
    /// Discovery Peer public endpoints that delivered this announcement (multi-source).
    ///
    /// Evicting or losing a Discovery Peer excludes announcements whose *only*
    /// remaining delivery source was that peer endpoint from current eligibility
    /// while preserving this audit row. Independent Bootstrap/Index/peer sources
    /// keep the announcement eligible.
    #[serde(default)]
    pub received_from_discovery_peer_endpoints: BTreeSet<String>,
    /// Time at which this node verified and retained it.
    pub received_at: DateTime<Utc>,
}

/// Deserializes multi-Index provenance, migrating legacy singular URL strings.
fn deserialize_index_provenance_urls<'de, D>(deserializer: D) -> Result<BTreeSet<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        Many(Vec<String>),
        One(String),
    }

    match Option::<OneOrMany>::deserialize(deserializer)? {
        None => Ok(BTreeSet::new()),
        Some(OneOrMany::One(url)) => {
            let mut set = BTreeSet::new();
            if !url.is_empty() {
                set.insert(url);
            }
            Ok(set)
        }
        Some(OneOrMany::Many(urls)) => Ok(urls.into_iter().filter(|url| !url.is_empty()).collect()),
    }
}
