use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionAssetType {
    RepresentativeImage,
    /// Reader-mode text copy of the source page, strictly local (ADR-0052).
    ReadableSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionAssetSource {
    PageImage,
    PageText,
    AiGenerated,
    UserProvided,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Submission {
    pub id: SubmissionId,
    pub tenant_id: Option<TenantId>,
    pub url: String,
    pub canonical_url: String,
    pub title: String,
    #[serde(default)]
    pub source_metadata: CandidateSourceMetadata,
    pub description: Option<String>,
    pub domain: String,
    pub submitted_by: Option<UserId>,
    pub discovered_by_crawler: bool,
    pub submitter_note: Option<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub provenance: Vec<CandidateProvenance>,
    #[serde(default)]
    pub media_references: Vec<MediaReference>,
    pub tags: Vec<String>,
    pub embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    pub origin_event_id: Option<Uuid>,
}

/// Review lifecycle of a private Candidate before curation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateReviewState {
    /// No authoritative Pod Placement has been created.
    Pending,
    /// At least one proposed Pod Placement became authoritative.
    Accepted,
}

/// Pod-owned autonomy mode for turning Candidate evidence into authoritative placements.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CurationPolicy {
    /// Every proposed placement waits for authorized review.
    Manual,
    /// Trusted task evidence may be accepted at or above the configured threshold.
    Assisted {
        /// Inclusive confidence floor for automatic acceptance.
        confidence_threshold: CandidateConfidence,
    },
    /// Any proposal at or above the configured threshold may be accepted automatically.
    Autonomous {
        /// Inclusive confidence floor for automatic acceptance.
        confidence_threshold: CandidateConfidence,
    },
}

impl Default for CurationPolicy {
    fn default() -> Self {
        Self::Assisted {
            confidence_threshold: CandidateConfidence(0.8),
        }
    }
}

/// Authoritative lifecycle of one Candidate-to-Pod association.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PodPlacementStatus {
    /// Waiting for an authorized decision.
    Pending,
    /// Authoritative and eligible for synchronization and Feeds.
    Accepted,
    /// Declined and suppressed from identical future local routing.
    Rejected,
    /// Formerly accepted but withdrawn and suppressed from identical routing.
    Reversed,
}

/// Path by which a placement reached its current authoritative state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CurationPath {
    /// Initial gated Candidate evidence.
    CandidateProposal,
    /// Explicit authorized review.
    ManualReview,
    /// Trusted high-confidence acceptance under Assisted Curation.
    AssistedAutomatic,
    /// Threshold acceptance under approved Autonomous Curation.
    AutonomousAutomatic,
    /// Additional local Pod proposed by the Routing Agent.
    RoutingAgent,
    /// Explicit authorized User curation that bypassed Candidate review.
    AddToPod,
}

/// Authenticated actor responsible for a curation transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CurationActor {
    /// Authenticated Agent Harness.
    Harness(AgentHarnessId),
    /// Directly authenticated User.
    User(UserId),
    /// Deterministic local automation without a harness identity.
    NodeAgent,
}

/// Immutable audit entry for a Pod Placement transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlacementAuditEntry {
    /// State produced by this transition.
    pub status: PodPlacementStatus,
    /// Curation path responsible for this transition.
    pub curation_path: CurationPath,
    /// Attributable actor responsible for this transition.
    pub actor: CurationActor,
    /// Optional review or reversal rationale.
    pub note: Option<CurationRationale>,
    /// Time at which the transition committed.
    pub occurred_at: DateTime<Utc>,
}

/// Evidence-backed association between one canonical Content Item and one Pod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PodPlacement {
    /// Private Candidate from which this association originated.
    pub candidate_id: CandidateId,
    /// Independently governed Pod receiving the association.
    pub pod_id: PodId,
    /// Canonical Content Item once the placement has been accepted.
    pub content_item_id: Option<ContentItemId>,
    /// Strongest retained explanation for this Pod association.
    pub reason: CurationRationale,
    /// Strongest retained confidence evidence, never authority by itself.
    pub confidence: CandidateConfidence,
    /// Immutable Candidate Submissions supporting this association.
    pub source_submission_ids: Vec<CandidateSubmissionId>,
    /// Origin placements visible when an explicit Add to Pod action preserved this item.
    #[serde(default)]
    pub origin_placements: Vec<AcceptedPlacementProjection>,
    /// Later signed withdrawals affecting preserved origin placements.
    #[serde(default)]
    pub origin_withdrawals: Vec<PlacementTombstone>,
    /// Current authoritative lifecycle state.
    pub status: PodPlacementStatus,
    /// Path that produced the current state.
    pub curation_path: CurationPath,
    /// Actor responsible for the current state.
    pub actor: CurationActor,
    /// Append-only state transition history.
    pub audit_history: Vec<PlacementAuditEntry>,
    /// Time at which the route was first proposed.
    pub created_at: DateTime<Utc>,
    /// Time at which the latest transition committed.
    pub updated_at: DateTime<Utc>,
}

/// Canonical unit of accepted content, independent of its Pod Placements.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ContentItem {
    legacy_record: Submission,
}

impl ContentItem {
    /// Returns the stable canonical identity shared by all Pod Placements.
    #[must_use]
    pub fn id(&self) -> ContentItemId {
        self.legacy_record.id.into()
    }

    /// Returns the original source reference retained for provenance.
    #[must_use]
    pub fn source_url(&self) -> &str {
        &self.legacy_record.url
    }

    /// Returns the normalized source identity used for deduplication.
    #[must_use]
    pub fn canonical_url(&self) -> &str {
        &self.legacy_record.canonical_url
    }

    /// Returns the source title or canonical-URL fallback.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.legacy_record.title
    }

    /// Returns the generated understanding retained independently of the source.
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.legacy_record.summary.as_deref()
    }

    /// Returns the excerpt that source policy permits Stumble to retain.
    #[must_use]
    pub fn permitted_description(&self) -> Option<&str> {
        self.legacy_record.description.as_deref()
    }

    /// Returns descriptive tags retained with this Content Reference.
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.legacy_record.tags
    }

    /// Returns source title, author, and publication time retained at acceptance.
    #[must_use]
    pub const fn source_metadata(&self) -> &CandidateSourceMetadata {
        &self.legacy_record.source_metadata
    }

    /// Returns the discovery evidence retained with this Content Reference.
    #[must_use]
    pub fn provenance(&self) -> &[CandidateProvenance] {
        &self.legacy_record.provenance
    }

    /// Returns permitted attached-media URLs without implying byte archival.
    #[must_use]
    pub fn media_references(&self) -> &[MediaReference] {
        &self.legacy_record.media_references
    }

    pub(crate) fn into_legacy_record(self) -> Submission {
        self.legacy_record
    }
}

#[derive(Serialize, Deserialize)]
struct ContentItemWire {
    id: ContentItemId,
    source_url: String,
    canonical_url: String,
    title: String,
    #[serde(default)]
    source_metadata: CandidateSourceMetadata,
    permitted_description: Option<String>,
    domain: String,
    summary: Option<String>,
    #[serde(default)]
    provenance: Vec<CandidateProvenance>,
    #[serde(default)]
    media_references: Vec<MediaReference>,
    tags: Vec<String>,
    created_at: DateTime<Utc>,
    origin_event_id: Option<Uuid>,
}

impl Serialize for ContentItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ContentItemWire {
            id: self.id(),
            source_url: self.legacy_record.url.clone(),
            canonical_url: self.legacy_record.canonical_url.clone(),
            title: self.legacy_record.title.clone(),
            source_metadata: self.legacy_record.source_metadata.clone(),
            permitted_description: self.legacy_record.description.clone(),
            domain: self.legacy_record.domain.clone(),
            summary: self.legacy_record.summary.clone(),
            provenance: self.legacy_record.provenance.clone(),
            media_references: self.legacy_record.media_references.clone(),
            tags: self.legacy_record.tags.clone(),
            created_at: self.legacy_record.created_at,
            origin_event_id: self.legacy_record.origin_event_id,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContentItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ContentItemWire::deserialize(deserializer)?;
        Ok(Self {
            legacy_record: Submission {
                id: wire.id.into(),
                tenant_id: None,
                url: wire.source_url,
                canonical_url: wire.canonical_url,
                title: wire.title,
                source_metadata: wire.source_metadata,
                description: wire.permitted_description,
                domain: wire.domain,
                submitted_by: None,
                discovered_by_crawler: false,
                submitter_note: None,
                summary: wire.summary,
                provenance: wire.provenance,
                media_references: wire.media_references,
                tags: wire.tags,
                embedding: None,
                created_at: wire.created_at,
                origin_event_id: wire.origin_event_id,
            },
        })
    }
}

impl From<&Submission> for ContentItem {
    fn from(value: &Submission) -> Self {
        Self {
            legacy_record: value.clone(),
        }
    }
}

/// Public, synchronization-safe evidence for one Accepted Placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AcceptedPlacementProjection {
    /// Canonical Content Item placed in the Pod.
    pub content_item_id: ContentItemId,
    /// Local or origin Pod identity, remapped by Pod slug on import.
    pub pod_id: PodId,
    /// Public evidence explaining why the item belongs in the Pod.
    pub reason: CurationRationale,
    /// Curation path that produced the Accepted Placement.
    pub curation_path: CurationPath,
    /// Origin Node responsible for the authoritative acceptance.
    pub origin_node_id: NodeIdentityId,
    /// Time at which the placement became accepted.
    pub accepted_at: DateTime<Utc>,
}

/// Cumulative signed metadata retained for one accepted Content Reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContentItemMetadataUpdate {
    pub(crate) content_item_id: ContentItemId,
    #[serde(default)]
    pub(crate) source_metadata: CandidateSourceMetadata,
    #[serde(default)]
    pub(crate) permitted_excerpt: Option<String>,
    #[serde(default)]
    pub(crate) summary: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) provenance: Vec<CandidateProvenance>,
    #[serde(default)]
    pub(crate) media_references: Vec<MediaReference>,
}

/// Typed signed-event body for a Content Reference metadata update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContentItemMetadataUpdatedPayload {
    pub(crate) metadata_update: ContentItemMetadataUpdate,
}

/// One entry in a Pod's complete accepted stream, independent of Feed selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PodContentItem {
    /// Canonical item shared across every independent Pod Placement.
    pub content_item: ContentItem,
    /// Synchronization-safe evidence for this Pod's Accepted Placement.
    pub accepted_placement: AcceptedPlacementProjection,
}

/// Signed withdrawal of one Origin Pod's previously accepted placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlacementTombstone {
    /// Reference-first content snapshot retained for required withdrawal audit.
    pub content_reference: FeedContentReference,
    /// Immutable placement evidence that existed before withdrawal.
    pub origin_placement: AcceptedPlacementProjection,
    /// Time at which approval committed the withdrawal.
    pub withdrawn_at: DateTime<Utc>,
}

/// One private Save together with any signed origin withdrawals recorded for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SavedContentReference {
    /// Locally retained reference-first content representation.
    pub content_reference: FeedContentReference,
    /// Signed origin withdrawals retained without cancelling the Save.
    pub origin_withdrawals: Vec<PlacementTombstone>,
}

/// Result of evaluating all proposed placements for a Candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CandidateCurationResult {
    /// Candidate evaluated by this operation.
    pub candidate: Candidate,
    /// Canonical Content Item when any placement was accepted.
    pub content_item: Option<ContentItem>,
    /// Independently evaluated Pod Placements.
    pub placements: Vec<PodPlacement>,
}

/// Authorized review decision for one pending placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PlacementReviewDecision {
    /// Create an Accepted Placement.
    Accept,
    /// Retain a rejected route for audit and suppression.
    Reject,
}

/// Routing Agent request to propose another authorized local Pod Placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct RouteCandidatePlacementRequest {
    /// Authorized local Pod proposed by the Routing Agent.
    pub pod_id: PodId,
    /// Evidence explaining the additional subject match.
    pub reason: CurationRationale,
    /// Bounded routing confidence retained as evidence.
    pub confidence: CandidateConfidence,
}

impl RouteCandidatePlacementRequest {
    /// Creates a validated Routing Agent proposal.
    ///
    /// # Errors
    ///
    /// Returns an error when `reason` is empty or only whitespace.
    pub fn new(
        pod_id: PodId,
        reason: impl Into<String>,
        confidence: CandidateConfidence,
    ) -> Result<Self, CurationRationaleError> {
        Ok(Self {
            pod_id,
            reason: CurationRationale::new(reason)?,
            confidence,
        })
    }
}

/// Explicit User curation request that bypasses Candidate review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AddContentItemToPodRequest {
    /// Existing canonical Content Item to place.
    pub content_item_id: ContentItemId,
    /// Authorized local Pod receiving the item.
    pub pod_id: PodId,
    /// Optional User-authored curation rationale.
    pub curation_note: Option<CurationRationale>,
}

impl AddContentItemToPodRequest {
    /// Creates an explicit Add to Pod request with an optional validated note.
    ///
    /// # Errors
    ///
    /// Returns an error when the supplied note is empty or only whitespace.
    pub fn new(
        content_item_id: ContentItemId,
        pod_id: PodId,
        curation_note: Option<String>,
    ) -> Result<Self, CurationRationaleError> {
        Ok(Self {
            content_item_id,
            pod_id,
            curation_note: curation_note.map(CurationRationale::new).transpose()?,
        })
    }
}

/// Slug of the default private Pod that receives `stumble add` references.
pub const DEFAULT_SAVED_POD_SLUG: &str = "saved";

/// One-shot request that turns a shared URL into Feed-eligible content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct AddReferenceRequest {
    /// External source URL being shared.
    pub url: String,
    /// Target Pod slug; the default private `saved` Pod is used when omitted.
    #[serde(default)]
    pub pod: Option<String>,
    /// Source title when known; the canonical URL is used otherwise.
    #[serde(default)]
    pub title: Option<String>,
    /// Understanding of the source generated by the sharer or their harness.
    #[serde(default)]
    pub summary: Option<String>,
    /// Excerpt that source policy permits Stumble to retain.
    #[serde(default)]
    pub excerpt: Option<String>,
    /// Descriptive subject tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional curation rationale recorded on the Accepted Placement.
    #[serde(default)]
    pub note: Option<String>,
    /// Harness-selected illustrative image URLs from the source page,
    /// reference-first (bytes are not archived).
    #[serde(default)]
    pub images: Vec<String>,
}

/// Outcome of a one-shot `stumble add`, including any setup it performed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AddedReference {
    /// Canonical Content Item now placed in the Pod.
    pub content_item: ContentItem,
    /// Pod that received the Accepted Placement.
    pub pod_id: PodId,
    /// Slug of the Pod that received the Accepted Placement.
    pub pod_slug: String,
    /// Whether the default `saved` Pod was created by this call.
    pub pod_created: bool,
    /// Whether the caller's User is subscribed so the item is Feed-eligible.
    pub subscribed: bool,
    /// The Accepted Placement created or confirmed by this call.
    pub placement: PodPlacement,
}

/// Validated non-empty evidence or rationale retained in a curation audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CurationRationale(String);

impl CurationRationale {
    /// Parses a non-empty rationale, trimming surrounding whitespace.
    ///
    /// # Errors
    ///
    /// Returns an error when the rationale is empty or only whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, CurationRationaleError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(CurationRationaleError);
        }
        Ok(Self(trimmed.to_string()))
    }

    /// Returns the validated rationale text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for CurationRationale {
    type Error = CurationRationaleError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl std::str::FromStr for CurationRationale {
    type Err = CurationRationaleError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl std::fmt::Display for CurationRationale {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CurationRationale {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Error returned for an empty curation rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("curation rationale must not be empty")]
pub struct CurationRationaleError;

/// Coarse external media type supplied by an Agent Harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CandidateContentType {
    Article,
    Video,
    Audio,
    Image,
    Podcast,
    Repository,
    Dataset,
    Other,
}

/// Permitted attached-media category supplied by an Agent Harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum MediaReferenceType {
    /// An image available at the referenced source URL.
    Image,
    /// A video available at the referenced source URL.
    Video,
}

/// Reference-first attached media retained without downloading its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaReference {
    /// Typed media category for presentation and policy decisions.
    media_type: MediaReferenceType,
    /// Canonical permitted HTTP(S) location; Stumble does not archive the target bytes.
    url: String,
}

/// Error returned when a URL cannot cross Stumble's canonical URL boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid or unsupported URL: {0}")]
pub struct CanonicalUrlError(String);

impl MediaReference {
    /// Validates and canonicalizes an attached-media reference at its domain boundary.
    pub fn new(
        media_type: MediaReferenceType,
        url: impl AsRef<str>,
    ) -> Result<Self, CanonicalUrlError> {
        Ok(Self {
            media_type,
            url: canonicalize_web_url(url.as_ref())?,
        })
    }

    /// Returns the presentation category supplied for this canonical media identity.
    #[must_use]
    pub const fn media_type(&self) -> MediaReferenceType {
        self.media_type
    }

    /// Returns the canonical permitted HTTP(S) location.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct MediaReferenceWire {
    media_type: MediaReferenceType,
    url: String,
}

impl Serialize for MediaReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        MediaReferenceWire {
            media_type: self.media_type,
            url: self.url.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for MediaReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MediaReferenceWire::deserialize(deserializer)?;
        Self::new(wire.media_type, wire.url).map_err(serde::de::Error::custom)
    }
}

/// Applies Stumble's canonical URL spelling policy to a permitted web URL.
pub(crate) fn canonicalize_web_url(value: &str) -> Result<String, CanonicalUrlError> {
    let canonical = canonicalize_url_spelling(value)?;
    let url = url::Url::parse(&canonical).map_err(|error| CanonicalUrlError(error.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(CanonicalUrlError(value.to_string()));
    }
    Ok(canonical)
}

/// Applies Stumble's shared canonical spelling policy without restricting URL schemes.
pub(crate) fn canonicalize_url_spelling(value: &str) -> Result<String, CanonicalUrlError> {
    let mut url = url::Url::parse(value).map_err(|error| CanonicalUrlError(error.to_string()))?;
    url.set_fragment(None);
    if (url.scheme() == "https" && url.port() == Some(443))
        || (url.scheme() == "http" && url.port() == Some(80))
    {
        let _ = url.set_port(None);
    }
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| {
            !matches!(
                key.as_ref(),
                "utm_source" | "utm_medium" | "utm_campaign" | "utm_term" | "utm_content"
            )
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    pairs.sort();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    Ok(url.to_string())
}

/// Error returned when one canonical media identity has incompatible type evidence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("canonical media URL {url} has conflicting media types")]
pub(crate) struct MediaEvidenceConflictError {
    url: String,
}

/// Resolves media evidence into a canonical, deduplicated, URL-sorted union.
pub(crate) fn resolve_media_evidence<'a>(
    references: impl IntoIterator<Item = &'a MediaReference>,
) -> Result<Vec<MediaReference>, MediaEvidenceConflictError> {
    let mut resolved = BTreeMap::new();
    for reference in references {
        if resolved
            .insert(reference.url(), reference.clone())
            .is_some_and(|existing: MediaReference| existing.media_type() != reference.media_type())
        {
            return Err(MediaEvidenceConflictError {
                url: reference.url().into(),
            });
        }
    }
    Ok(resolved.into_values().collect())
}

/// Harness confidence retained as bounded evidence, never authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandidateConfidence(f32);

// Construction and deserialization reject NaN, making equality reflexive.
impl Eq for CandidateConfidence {}

impl Serialize for CandidateConfidence {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Canonicalize through the shortest decimal that round-trips to this f32.
        // This keeps nested proposal JSON stable across SQLite reloads while
        // retaining a numeric wire value and the exact domain value.
        let canonical = self
            .0
            .to_string()
            .parse::<f64>()
            .expect("a finite f32 always has a valid decimal representation");
        serializer.serialize_f64(canonical)
    }
}

impl CandidateConfidence {
    /// Creates finite confidence evidence in the inclusive range `0.0..=1.0`.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite or out-of-range values.
    pub fn new(value: f32) -> Result<Self, CandidateConfidenceError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(CandidateConfidenceError(value))
        }
    }

    /// Returns the wire-compatible confidence value.
    #[must_use]
    pub const fn value(self) -> f32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for CandidateConfidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(f32::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Error returned for invalid Candidate confidence evidence.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("candidate confidence must be finite and between 0 and 1, got {0}")]
pub struct CandidateConfidenceError(f32);
