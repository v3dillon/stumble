//! Pure deterministic Pod Similarity scoring.

use super::caps::TRIAL_SIMILARITY_THRESHOLD;
use crate::domain::{discovery_tokens, FeedContentReference, PodAnnouncement, PodEndorsement};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

/// Maximum endorsement contribution (optional evidence, not a gate).
const MAX_ENDORSEMENT_BOOST: f32 = 0.5;
const ENDORSEMENT_UNIT: f32 = 0.1;

/// Stable DTO-boundary label for limited trial exposure (not a similarity evidence kind).
pub const TRIAL_EXPOSURE_REASON: &str =
    "limited labeled trial exposure for strong unendorsed similarity after verification";

/// Inspectable evidence class for a similarity reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SimilarityEvidenceKind {
    /// Match against public subject / Pod Context text.
    Subject,
    /// Match against source neighborhoods (sample domains / communities).
    Source,
    /// Match against bounded Explore sample titles, tags, or summaries.
    Sample,
    /// Valid signed Pod Endorsement used as local ranking evidence only.
    Endorsement,
}

/// One inspectable reason supporting a local Pod Similarity score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SimilarityReason {
    /// Evidence class for User inspection.
    pub kind: SimilarityEvidenceKind,
    /// Human-readable detail without private matching inputs.
    pub detail: String,
}

impl SimilarityReason {
    /// Formats a stable reason string for Explore / Feed surfaces.
    #[must_use]
    pub fn display(&self) -> String {
        match self.kind {
            SimilarityEvidenceKind::Subject => format!("subject evidence: {}", self.detail),
            SimilarityEvidenceKind::Source => format!("source evidence: {}", self.detail),
            SimilarityEvidenceKind::Sample => format!("sample evidence: {}", self.detail),
            SimilarityEvidenceKind::Endorsement => {
                format!("endorsement evidence: {}", self.detail)
            }
        }
    }
}

/// Deterministic local similarity outcome for one candidate public Pod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PodSimilarityScore {
    /// Local non-universal score used only for ordering on this Home Node.
    pub score: f32,
    /// Score before optional endorsement boost (used for trial eligibility).
    pub base_score: f32,
    /// Inspectable evidence classes that contributed.
    pub reasons: Vec<SimilarityReason>,
    /// Number of valid endorsements considered (never a global reputation).
    pub endorsement_count: usize,
    /// Whether an unendorsed Pod may receive limited labeled trial exposure.
    ///
    /// Sole trial signal; DTO surfaces label trial exposure separately via
    /// [`append_trial_exposure_label`] — trial is never a
    /// [`SimilarityEvidenceKind`].
    pub trial_exposure: bool,
}

/// Private matching inputs never sent to remote infrastructure.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalSimilarityContext {
    /// Tokens from an explicit Explore query and/or private interests.
    pub topic_tokens: BTreeSet<String>,
    /// Source domains / communities known from private local evidence.
    pub source_signals: HashSet<String>,
}

impl LocalSimilarityContext {
    /// Builds matching tokens from an explicit query string.
    #[must_use]
    pub fn from_query(query: &str) -> Self {
        let mut topic_tokens = BTreeSet::new();
        for token in discovery_tokens(&query.to_lowercase()) {
            topic_tokens.insert(token);
        }
        Self {
            topic_tokens,
            source_signals: HashSet::new(),
        }
    }

    /// Builds private ranking context from optional query, interests, and sources.
    ///
    /// Shared by Explore and Feed so both surfaces use the same construction.
    #[must_use]
    pub fn from_private_evidence(
        query: Option<&str>,
        interests: impl IntoIterator<Item = impl AsRef<str>>,
        source_signals: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        let mut local = query.map(Self::from_query).unwrap_or_default();
        local.extend_topics(interests);
        local.extend_sources(source_signals);
        local
    }

    /// Extends context with private interest tokens held locally.
    pub fn extend_topics(&mut self, interests: impl IntoIterator<Item = impl AsRef<str>>) {
        for interest in interests {
            for token in discovery_tokens(&interest.as_ref().to_lowercase()) {
                self.topic_tokens.insert(token);
            }
        }
    }

    /// Extends context with private source neighborhood signals held locally.
    pub fn extend_sources(&mut self, sources: impl IntoIterator<Item = impl AsRef<str>>) {
        for source in sources {
            let source = source.as_ref().trim().to_lowercase();
            if !source.is_empty() {
                self.source_signals.insert(source);
            }
        }
    }

    /// Whether any private or explicit matching signal is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.topic_tokens.is_empty() && self.source_signals.is_empty()
    }
}

/// Verified public evidence for one candidate Pod already on the Home Node.
#[derive(Debug, Clone)]
pub struct CandidatePodEvidence<'a> {
    /// Current verified announcement.
    pub announcement: &'a PodAnnouncement,
    /// Optional local Pod Context text (CONTEXT.md) when the package is local.
    pub context_text: Option<&'a str>,
    /// Policy-filtered Explore samples bound to the current announcement.
    pub samples: &'a [FeedContentReference],
    /// Valid endorsements binding the current announcement (optional).
    pub endorsements: &'a [PodEndorsement],
    /// Whether samples passed Origin signature + current announcement binding.
    ///
    /// Must reflect real retained Origin-signed samples only — never synthetic.
    pub samples_verified: bool,
}

/// Appends the trial-exposure label at a DTO boundary when trial is active.
///
/// Trial is carried solely by [`PodSimilarityScore::trial_exposure`]; this label
/// is presentation only and is never a [`SimilarityEvidenceKind`].
pub fn append_trial_exposure_label(reasons: &mut Vec<String>, trial_exposure: bool) {
    if trial_exposure {
        reasons.push(TRIAL_EXPOSURE_REASON.into());
    }
}

/// Scores a single Feed Exploration Item against a verified current announcement.
///
/// Returns `None` when private context is empty, no positive score results, or
/// the caller has no verified current announcement (callers must not fabricate
/// synthetic announcements). Trial eligibility requires real
/// `samples_verified` evidence.
#[must_use]
pub fn score_exploration_item(
    local: &LocalSimilarityContext,
    announcement: &PodAnnouncement,
    context_text: Option<&str>,
    item_sample: &FeedContentReference,
    endorsements: &[PodEndorsement],
    samples_verified: bool,
) -> Option<PodSimilarityScore> {
    if local.is_empty() {
        return None;
    }
    let samples = std::slice::from_ref(item_sample);
    let similarity = score_pod_similarity(
        local,
        &CandidatePodEvidence {
            announcement,
            context_text,
            samples,
            endorsements,
            samples_verified,
        },
    );
    if similarity.score <= 0.0 {
        None
    } else {
        Some(similarity)
    }
}

/// Computes deterministic Pod Similarity from local public evidence + private context.
///
/// Endorsements only boost an already-scored candidate (`base_score > 0`); a
/// zero-endorsement Pod with strong subject/source/sample match remains eligible
/// for trial exposure when samples verified. No model service is required.
/// Trial exposure is returned as a typed flag only — not as a similarity reason.
#[must_use]
pub fn score_pod_similarity(
    local: &LocalSimilarityContext,
    candidate: &CandidatePodEvidence<'_>,
) -> PodSimilarityScore {
    let mut reasons = Vec::new();
    let mut base_score = 0.0_f32;

    let subject_text = format!(
        "{} {} {}",
        candidate.announcement.pod_slug,
        candidate.announcement.pod_name,
        candidate.announcement.subject
    );
    let mut subject_tokens = token_set(&subject_text);
    if let Some(context) = candidate.context_text {
        subject_tokens.extend(token_set(context));
    }

    if local.topic_tokens.is_empty() && local.source_signals.is_empty() {
        // No matching signals: availability-only baseline for empty Explore queries.
        base_score = 1.0;
        reasons.push(SimilarityReason {
            kind: SimilarityEvidenceKind::Subject,
            detail: "public Pod is available through the configured Stumble Substrate".into(),
        });
    } else {
        let subject_overlap = coverage_overlap(&local.topic_tokens, &subject_tokens);
        if subject_overlap > 0.0 {
            base_score += subject_overlap;
            let matched = matched_tokens(&local.topic_tokens, &subject_tokens);
            reasons.push(SimilarityReason {
                kind: SimilarityEvidenceKind::Subject,
                detail: format!(
                    "matched public subject/context tokens: {}",
                    matched.join(", ")
                ),
            });
        }

        let mut source_hit = false;
        let mut sample_tokens = BTreeSet::new();
        for sample in candidate.samples {
            let source = sample.source.trim().to_lowercase();
            if !source.is_empty()
                && local
                    .source_signals
                    .iter()
                    .any(|signal| signal.eq_ignore_ascii_case(&source))
            {
                source_hit = true;
            }
            sample_tokens.extend(token_set(&sample.title));
            if let Some(summary) = &sample.summary {
                sample_tokens.extend(token_set(summary));
            }
            for tag in &sample.tags {
                sample_tokens.extend(token_set(tag));
            }
            // Community-style source neighborhood: sample source domain itself.
            if !source.is_empty() {
                sample_tokens.insert(source);
            }
        }
        if source_hit {
            base_score += 0.8;
            reasons.push(SimilarityReason {
                kind: SimilarityEvidenceKind::Source,
                detail: "sample source neighborhood overlaps private source evidence".into(),
            });
        }

        let sample_overlap = coverage_overlap(&local.topic_tokens, &sample_tokens);
        if sample_overlap > 0.0 {
            base_score += sample_overlap * 0.6;
            let matched = matched_tokens(&local.topic_tokens, &sample_tokens);
            reasons.push(SimilarityReason {
                kind: SimilarityEvidenceKind::Sample,
                detail: format!("matched Explore sample tokens: {}", matched.join(", ")),
            });
        }
    }

    let endorsement_count = candidate.endorsements.len();
    // ADR 0040 / discovery.md: endorsements strengthen existing similarity only.
    let endorsement_boost = if base_score > 0.0 && endorsement_count > 0 {
        let boost = (endorsement_count.min(5) as f32) * ENDORSEMENT_UNIT;
        let boost = boost.min(MAX_ENDORSEMENT_BOOST);
        reasons.push(SimilarityReason {
            kind: SimilarityEvidenceKind::Endorsement,
            detail: format!(
                "{endorsement_count} optional Pod Endorsement(s) used as local ranking evidence (not transferable trust)"
            ),
        });
        boost
    } else {
        0.0
    };

    let score = base_score + endorsement_boost;
    let trial_exposure = endorsement_count == 0
        && candidate.samples_verified
        && !candidate.samples.is_empty()
        && base_score >= TRIAL_SIMILARITY_THRESHOLD;

    PodSimilarityScore {
        score,
        base_score,
        reasons,
        endorsement_count,
        trial_exposure,
    }
}

fn token_set(text: &str) -> BTreeSet<String> {
    discovery_tokens(&text.to_lowercase()).into_iter().collect()
}

/// Coverage of `left` against `right`: |intersection| / min(|left|, |right|).
///
/// Prefer recall against the smaller local query set so a focused interest still
/// scores strongly against a broader public subject.
fn coverage_overlap(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f32 {
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let intersection = left.intersection(right).count();
    if intersection == 0 {
        return 0.0;
    }
    let denom = left.len().min(right.len()).max(1);
    let matched = u16::try_from(intersection).unwrap_or(u16::MAX);
    let total = u16::try_from(denom).unwrap_or(u16::MAX);
    f32::from(matched) / f32::from(total)
}

fn matched_tokens(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    let mut matched = left.intersection(right).cloned().collect::<Vec<_>>();
    matched.sort();
    matched.truncate(8);
    matched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        announcement_lease_duration, NodeInfo, PackageVersion, CURRENT_PROTOCOL_VERSION,
    };
    use crate::signing::{create_node_identity, sign_pod_announcement};
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
    fn subject_match_is_deterministic_and_inspectable() {
        let (_node, announcement) =
            announcement("Careful distributed systems research", "systems-lab");
        let local = LocalSimilarityContext::from_query("distributed systems");
        let score = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &announcement,
                context_text: Some("CONTEXT: reliability and distributed systems"),
                samples: &[],
                endorsements: &[],
                samples_verified: false,
            },
        );
        assert!(score.score > 0.0);
        assert!(score
            .reasons
            .iter()
            .any(|r| r.kind == SimilarityEvidenceKind::Subject));
        assert!(!score.trial_exposure);
        let again = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &announcement,
                context_text: Some("CONTEXT: reliability and distributed systems"),
                samples: &[],
                endorsements: &[],
                samples_verified: false,
            },
        );
        assert_eq!(score.score, again.score);
        assert_eq!(score.reasons, again.reasons);
    }

    #[test]
    fn sample_and_source_evidence_raise_score_with_reasons() {
        let (_node, announcement) = announcement("Rust ownership patterns", "rust-lab");
        let samples = vec![sample(
            "Ownership in concurrent systems",
            "rust-lang.org",
            &["rust", "ownership"],
        )];
        let mut local = LocalSimilarityContext::from_query("ownership");
        local.extend_sources(["rust-lang.org"]);
        let score = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &announcement,
                context_text: None,
                samples: &samples,
                endorsements: &[],
                samples_verified: true,
            },
        );
        assert!(score.score > 0.5);
        assert!(score
            .reasons
            .iter()
            .any(|r| r.kind == SimilarityEvidenceKind::Sample));
        assert!(score
            .reasons
            .iter()
            .any(|r| r.kind == SimilarityEvidenceKind::Source));
    }

    #[test]
    fn endorsements_strengthen_but_are_not_required_for_trial() {
        let (node, announcement) =
            announcement("Thoughtful distributed systems research notes", "systems");
        let samples = vec![sample(
            "Distributed systems research survey",
            "acm.org",
            &["systems", "research"],
        )];
        let local = LocalSimilarityContext::from_query("distributed systems research");
        let unendorsed = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &announcement,
                context_text: None,
                samples: &samples,
                endorsements: &[],
                samples_verified: true,
            },
        );
        assert!(unendorsed.trial_exposure);
        assert_eq!(unendorsed.endorsement_count, 0);
        // Trial is a typed flag only — never a SimilarityEvidenceKind reason.
        assert!(!unendorsed
            .reasons
            .iter()
            .any(|r| r.detail.contains("trial exposure")));

        let endorsement = PodEndorsement {
            id: Uuid::now_v7(),
            endorsing_node_id: node.id,
            signer: NodeInfo {
                node_id: node.id,
                display_name: node.display_name.clone(),
                public_key: node.public_key.clone(),
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            },
            endorsing_pod_slug: "curators".into(),
            endorsing_announcement_id: Uuid::now_v7(),
            endorsed_node_id: announcement.origin_node_id,
            endorsed_pod_slug: announcement.pod_slug.clone(),
            endorsed_announcement_id: announcement.id,
            reason: "Careful curation".into(),
            endorsed_at: Utc::now(),
            signature: "sig".into(),
        };
        let endorsed = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &announcement,
                context_text: None,
                samples: &samples,
                endorsements: &[endorsement],
                samples_verified: true,
            },
        );
        assert!(endorsed.score > unendorsed.score);
        assert!(!endorsed.trial_exposure);
        assert!(endorsed
            .reasons
            .iter()
            .any(|r| r.kind == SimilarityEvidenceKind::Endorsement
                && r.detail.contains("not transferable trust")));
    }

    #[test]
    fn endorsement_alone_does_not_surface_unrelated_pod() {
        let (node, announcement) = announcement("Cooking recipes and baking tips", "food-blog");
        let local = LocalSimilarityContext::from_query("distributed systems research");
        let endorsement = PodEndorsement {
            id: Uuid::now_v7(),
            endorsing_node_id: node.id,
            signer: NodeInfo {
                node_id: node.id,
                display_name: node.display_name.clone(),
                public_key: node.public_key.clone(),
                supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
            },
            endorsing_pod_slug: "curators".into(),
            endorsing_announcement_id: Uuid::now_v7(),
            endorsed_node_id: announcement.origin_node_id,
            endorsed_pod_slug: announcement.pod_slug.clone(),
            endorsed_announcement_id: announcement.id,
            reason: "Friendly shout-out".into(),
            endorsed_at: Utc::now(),
            signature: "sig".into(),
        };
        let score = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &announcement,
                context_text: None,
                samples: &[],
                endorsements: &[endorsement],
                samples_verified: false,
            },
        );
        assert_eq!(score.base_score, 0.0);
        assert_eq!(score.score, 0.0);
        assert!(!score
            .reasons
            .iter()
            .any(|r| r.kind == SimilarityEvidenceKind::Endorsement));
    }

    #[test]
    fn trial_label_appended_only_at_dto_boundary() {
        let mut reasons = vec!["subject evidence: matched".into()];
        append_trial_exposure_label(&mut reasons, false);
        assert_eq!(reasons.len(), 1);
        append_trial_exposure_label(&mut reasons, true);
        assert_eq!(reasons.len(), 2);
        assert!(reasons[1].contains("trial exposure"));
    }

    #[test]
    fn works_without_agent_harness_or_model_service() {
        // Pure function path: no AgentTools, no network, no model.
        let (_node, announcement) = announcement("machine learning systems", "ml-sys");
        let local = LocalSimilarityContext::from_query("machine learning");
        let score = score_pod_similarity(
            &local,
            &CandidatePodEvidence {
                announcement: &announcement,
                context_text: None,
                samples: &[],
                endorsements: &[],
                samples_verified: false,
            },
        );
        assert!(score.score > 0.0);
        assert!(score.reasons.iter().all(|r| !r.detail.is_empty()));
    }

    #[test]
    fn coverage_overlap_prefers_query_recall() {
        let left: BTreeSet<String> = ["systems", "research"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let right: BTreeSet<String> = ["systems", "research", "distributed", "careful"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert!((coverage_overlap(&left, &right) - 1.0).abs() < f32::EPSILON);
        assert_eq!(coverage_overlap(&left, &BTreeSet::new()), 0.0);
    }
}
