use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub user_id: Option<UserId>,
    pub tenant_id: Option<TenantId>,
    pub node_id: NodeIdentityId,
    /// Harness whose scoped grant applies to this request.
    #[serde(default)]
    pub harness_id: Option<AgentHarnessId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePodRequest {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub visibility: Visibility,
}

/// Complete portable contents of one Pod Package version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "snake_case",
    try_from = "RawPodPackageContents"
)]
pub struct PodPackageContents {
    /// Subject language, scope, and boundaries.
    pub context_md: String,
    /// Scoped, untrusted harness instructions.
    pub skill_md: String,
    /// Declarative Source Rules.
    pub sources_yaml: String,
    /// Pod-owned filters.
    pub filters_yaml: String,
    /// Positive calibration examples.
    pub examples_good_md: String,
    /// Negative calibration examples.
    pub examples_bad_md: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub(crate) struct RawPodPackageContents {
    pub context_md: String,
    pub skill_md: String,
    pub sources_yaml: String,
    pub filters_yaml: String,
    pub examples_good_md: String,
    pub examples_bad_md: String,
}

/// Atomic request to create a private Pod and its first package version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CreatePrivatePodWithPackageRequest {
    /// Display name for the private Pod.
    pub name: String,
    /// Stable local URL slug.
    pub slug: String,
    /// Human-readable Pod summary.
    pub description: String,
    /// Complete validated initial package contents.
    pub package: PodPackageContents,
}

/// Result of atomically creating a private Pod and package.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CreatedPodPackage {
    /// Newly created private Pod.
    pub pod: Pod,
    /// Immutable initial package version.
    pub package: PodPackage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitLinkRequest {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub discovered_by_crawler: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionWithAssets {
    pub submission: Submission,
    pub assets: Vec<SubmissionAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkIntakeRequest {
    pub url: String,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub representative_image: Option<RepresentativeImageRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRouteIntakeRequest {
    pub url: String,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub representative_image: Option<RepresentativeImageRequest>,
    pub min_confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepresentativeImageRequest {
    pub source: SubmissionAssetSource,
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub mime_type: Option<String>,
    pub alt_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadableSnapshotRequest {
    pub source: ReadableSnapshotSource,
    pub local_path: String,
    pub mime_type: Option<String>,
}

/// Origin of a Readable Snapshot's text. A snapshot is an archive of what
/// the page said, so an AI-generated source does not exist here (ADR-0052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadableSnapshotSource {
    PageText,
    UserProvided,
}

impl From<ReadableSnapshotSource> for SubmissionAssetSource {
    fn from(source: ReadableSnapshotSource) -> Self {
        match source {
            ReadableSnapshotSource::PageText => Self::PageText,
            ReadableSnapshotSource::UserProvided => Self::UserProvided,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkIntakeResponse {
    pub submission: Submission,
    pub assets: Vec<SubmissionAsset>,
    pub fetched_title: Option<String>,
    pub fetched_summary: Option<String>,
    pub representative_image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodRouteCandidate {
    pub pod_slug: String,
    pub pod_name: String,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteLinkRequest {
    pub url: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteLinkResponse {
    pub candidates: Vec<PodRouteCandidate>,
    pub selected: Option<PodRouteCandidate>,
    pub needs_confirmation: bool,
    pub confidence_threshold: f32,
    pub suggested_new_pod: Option<CreatePodRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRouteIntakeResponse {
    pub routing: RouteLinkResponse,
    pub intake: Option<LinkIntakeResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverRequest {
    pub query: String,
    pub avoid: Vec<String>,
    pub limit: usize,
    pub mode: DiscoveryMode,
    pub user_id: Option<UserId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateBriefRequest {
    pub pod_slugs: Vec<String>,
    pub query: Option<String>,
    pub user_id: Option<UserId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPackPatch {
    #[serde(default)]
    pub context_md: Option<String>,
    pub pod_yaml: Option<String>,
    pub skill_md: Option<String>,
    pub sources_yaml: Option<String>,
    pub filters_yaml: Option<String>,
    pub examples_good_md: Option<String>,
    pub examples_bad_md: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevTokenRequest {
    pub user_id: Option<UserId>,
    pub tenant_slug: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevTokenResponse {
    pub token: String,
    pub token_hash: String,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
}

/// Strict registration payload for a new Agent Harness and Harness Grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterAgentHarnessRequest {
    /// Human-readable operator label.
    pub label: String,
    /// Interactive or unattended operating mode.
    pub kind: AgentHarnessKind,
    /// Independently allowed operations.
    pub capabilities: Vec<HarnessCapability>,
    /// Optional allowlist of local Pods.
    pub pod_ids: Option<Vec<PodId>>,
}

/// One-time bearer token whose diagnostic formatting is always redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HarnessToken(String);

impl HarnessToken {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    /// Borrows the token for authentication without copying it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for HarnessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HarnessToken([redacted])")
    }
}

/// Result of registration. The plaintext token is never returned again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterAgentHarnessResponse {
    /// Persisted harness and normalized grant.
    pub harness: AgentHarness,
    /// Returned only at registration. Only its hash is retained by the Home Node.
    pub token: HarnessToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub interests: Option<Vec<String>>,
    pub blocked_topics: Option<Vec<String>>,
    pub blocked_sources: Option<Vec<String>>,
    pub preferred_brief_length: Option<usize>,
    pub preferred_discovery_mode: Option<DiscoveryMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodAgentContext {
    pub pod_slug: String,
    pub pod_name: String,
    pub skill_pack_version: i32,
    pub skill_md: String,
    pub pod_yaml: String,
    pub filters_yaml: String,
    pub validation: ValidationReport,
}
