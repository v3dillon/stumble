use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

pub type TenantId = Uuid;
pub type UserId = Uuid;
pub type PodId = Uuid;
pub type SubmissionId = Uuid;
pub type PeerId = Uuid;
pub type NodeIdentityId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    Local,
    Hosted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    InviteOnly,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TenantRole {
    Owner,
    Admin,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodRole {
    Owner,
    Moderator,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    DeepMatch,
    Adjacent,
    OldGem,
    HumanPick,
    RabbitHole,
    Stumble,
}

impl Default for DiscoveryMode {
    fn default() -> Self {
        Self::DeepMatch
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlerSourceType {
    Rss,
    Atom,
    Sitemap,
    Webpage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlCandidateStatus {
    Pending,
    Promoted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    Interesting,
    NotForMe,
    Dismissed,
    Saved,
    BlockSource,
    BlockTopic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionAssetType {
    RepresentativeImage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionAssetSource {
    PageImage,
    AiGenerated,
    UserProvided,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantUser {
    pub tenant_id: TenantId,
    pub user_id: UserId,
    pub role: TenantRole,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: Uuid,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub token_hash: String,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

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
    pub tenant_id: Option<TenantId>,
    pub display_name: String,
    pub base_url: String,
    pub public_key: String,
    pub trust_level: TrustLevel,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodMembership {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodSkillPack {
    pub id: Uuid,
    pub pod_id: PodId,
    pub version: i32,
    pub pod_yaml: String,
    pub skill_md: String,
    pub sources_yaml: String,
    pub filters_yaml: String,
    pub examples_good_md: String,
    pub examples_bad_md: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submission {
    pub id: SubmissionId,
    pub tenant_id: Option<TenantId>,
    pub url: String,
    pub canonical_url: String,
    pub title: String,
    pub description: Option<String>,
    pub domain: String,
    pub submitted_by: Option<UserId>,
    pub discovered_by_crawler: bool,
    pub submitter_note: Option<String>,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    pub origin_event_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionPod {
    pub submission_id: SubmissionId,
    pub pod_id: PodId,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionAsset {
    pub id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub submission_id: SubmissionId,
    pub asset_type: SubmissionAssetType,
    pub source: SubmissionAssetSource,
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub mime_type: Option<String>,
    pub alt_text: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerSource {
    pub id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub pod_id: PodId,
    pub source_type: CrawlerSourceType,
    pub url: String,
    pub enabled: bool,
    pub crawl_interval_minutes: i32,
    pub last_crawled_at: Option<DateTime<Utc>>,
    pub origin_event_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlCandidate {
    pub id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub pod_id: PodId,
    pub crawler_source_id: Uuid,
    pub url: String,
    pub canonical_url: String,
    pub title: String,
    pub description: Option<String>,
    pub domain: String,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub status: CrawlCandidateStatus,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub interests: Vec<String>,
    pub blocked_topics: Vec<String>,
    pub blocked_sources: Vec<String>,
    pub preferred_brief_length: usize,
    pub preferred_discovery_mode: DiscoveryMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackEvent {
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub submission_id: SubmissionId,
    pub event_type: FeedbackKind,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub local_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brief {
    pub id: Uuid,
    pub tenant_id: Option<TenantId>,
    pub user_id: Option<UserId>,
    pub title: String,
    pub query: Option<String>,
    pub created_at: DateTime<Utc>,
    pub private: bool,
    pub items: Vec<BriefItem>,
    pub reflection: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefItem {
    pub submission_id: SubmissionId,
    pub role: String,
    pub title: String,
    pub url: String,
    pub summary: String,
    pub why_it_matters: String,
    pub why_user_may_care: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecommendationExplanation {
    pub matched_interests: Vec<String>,
    pub matched_pod_signals: Vec<String>,
    pub blocked_or_downranked_signals_avoided: Vec<String>,
    pub source_reason: String,
    pub novelty_reason: String,
    pub human_or_crawler_origin: String,
    pub final_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryItem {
    pub title: String,
    pub url: String,
    pub short_summary: String,
    pub why_matches_request: String,
    pub why_belongs_in_pod: String,
    pub source: String,
    pub origin: String,
    pub recommendation_explanation: RecommendationExplanation,
    pub submission_id: SubmissionId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellKnownNode {
    pub protocol: String,
    pub node: NodeInfo,
    pub endpoints: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubRegisteredNode {
    pub node_id: NodeIdentityId,
    pub display_name: String,
    pub base_url: String,
    pub public_key: String,
    pub protocol_version: String,
    pub registered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubRegisteredPod {
    pub id: Uuid,
    pub node_id: NodeIdentityId,
    pub node_base_url: String,
    pub pod_slug: String,
    pub pod_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub skill_pack_version: i32,
    pub latest_event_hash: Option<String>,
    pub manifest_url: String,
    pub events_url: String,
    pub registered_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubRegisterNodeRequest {
    pub node_id: NodeIdentityId,
    pub display_name: String,
    pub base_url: String,
    pub public_key: String,
    pub protocol_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubRegisterPodRequest {
    pub node_id: NodeIdentityId,
    pub node_base_url: String,
    pub pod_slug: String,
    pub pod_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub skill_pack_version: i32,
    pub latest_event_hash: Option<String>,
    pub manifest_url: String,
    pub events_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubSearchPodResult {
    pub pod: HubRegisteredPod,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubSearchPodsResponse {
    pub query: String,
    pub results: Vec<HubSearchPodResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PodDiscoveryScope {
    Local,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodDiscoveryFeedItem {
    pub pod: HubRegisteredPod,
    pub scope: PodDiscoveryScope,
    pub score: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodDiscoveryFeedResponse {
    pub query: String,
    pub local_public_pods: Vec<PodDiscoveryFeedItem>,
    pub global_public_pods: Vec<PodDiscoveryFeedItem>,
    pub private_interests_exported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomePublicPodDiscoveryResponse {
    pub topics: Vec<String>,
    pub local_public_pods: Vec<PodRouteCandidate>,
    pub hub_results: Vec<HubSearchPodResult>,
    pub private_interests_exported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedSkillPack {
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub user_id: Option<UserId>,
    pub tenant_id: Option<TenantId>,
    pub node_id: NodeIdentityId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePodRequest {
    pub name: String,
    pub slug: String,
    pub description: String,
    pub visibility: Visibility,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPackPatch {
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
