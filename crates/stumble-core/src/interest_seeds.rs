use crate::agent_tools::AgentToolsError;
use crate::domain::*;
use crate::store::{InMemoryStore, StoreError};
use std::collections::{HashMap, HashSet};
use url::Url;

pub(crate) fn record_interest_seed(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    candidate: &Candidate,
    submission: &CandidateSubmission,
) -> Result<(), AgentToolsError> {
    let user_id = ctx.user_id.ok_or_else(|| {
        StoreError::Validation("Interest Seed requires an authenticated User".into())
    })?;
    submission.target.interest_seed_metadata().ok_or_else(|| {
        StoreError::Validation("Interest Seed requires a User submission target".into())
    })?;
    let signals = candidate_submission_taste_signals(candidate, submission);
    let key = (user_id, candidate.id);
    let provenance = submission.evidence.provenance.clone();
    let mut evidence = signals
        .into_iter()
        .map(|signal| InterestSeedSignalEvidence {
            signal,
            provenance: provenance.clone(),
        })
        .collect::<Vec<_>>();
    evidence.sort_by(|left, right| {
        taste_signal_key(&left.signal).cmp(&taste_signal_key(&right.signal))
    });
    if let Some(seed) = store.interest_seeds.get_mut(&key) {
        for item in evidence {
            if !seed.evidence.contains(&item) {
                seed.evidence.push(item);
            }
        }
        seed.evidence.sort_by(|left, right| {
            taste_signal_key(&left.signal).cmp(&taste_signal_key(&right.signal))
        });
        return Ok(());
    }
    store.interest_seeds.insert(
        key,
        InterestSeed {
            user_id,
            tenant_id: ctx.tenant_id,
            candidate_id: candidate.id,
            evidence,
            created_at: submission.created_at,
            retracted_at: None,
        },
    );
    Ok(())
}

pub(crate) fn candidate_submission_taste_signals(
    candidate: &Candidate,
    submission: &CandidateSubmission,
) -> HashSet<LearnedTasteSignal> {
    let mut signals = HashSet::new();
    if let Some(domain) = Url::parse(&candidate.canonical_url)
        .ok()
        .and_then(|url| url.domain().map(normalized_value))
        .filter(|value| !value.is_empty())
    {
        signals.insert(LearnedTasteSignal::Source(domain));
    }
    signals.extend(
        submission
            .evidence
            .tags
            .iter()
            .filter_map(|value| normalized_nonempty(value).map(LearnedTasteSignal::Topic)),
    );
    if let Some(value) = submission
        .evidence
        .source_metadata
        .author
        .as_deref()
        .and_then(normalized_nonempty)
    {
        signals.insert(LearnedTasteSignal::AuthorOrAccount(value));
    }
    if let Some(metadata) = submission.target.interest_seed_metadata() {
        if let Some(value) = metadata.publisher.as_deref().and_then(normalized_nonempty) {
            signals.insert(LearnedTasteSignal::Publisher(value));
        }
        if let Some(value) = metadata.community.as_deref().and_then(normalized_nonempty) {
            signals.insert(LearnedTasteSignal::Community(value));
        }
    }
    if let Some(referrer) = submission.evidence.provenance.referrer_url.as_deref() {
        if let Some(value) = Url::parse(referrer)
            .ok()
            .and_then(|url| url.domain().map(normalized_value))
            .filter(|value| !value.is_empty())
        {
            signals.insert(LearnedTasteSignal::ReferrerContext(value));
        }
    }
    signals
}

fn normalized_nonempty(value: &str) -> Option<String> {
    let value = normalized_value(value);
    (!value.is_empty()).then_some(value)
}

fn normalized_value(value: &str) -> String {
    value.trim().to_lowercase()
}

pub(crate) fn interest_seed_evidence(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
) -> InterestSeedEvidenceSummary {
    let (active, retracted) = store
        .interest_seeds
        .values()
        .filter(|seed| seed.user_id == user_id && seed.tenant_id == tenant_id)
        .fold((0_u32, 0_u32), |(active, retracted), seed| {
            if seed.retracted_at.is_none() {
                (active.saturating_add(1), retracted)
            } else {
                (active, retracted.saturating_add(1))
            }
        });
    InterestSeedEvidenceSummary {
        active_seed_count: active,
        retracted_seed_count: retracted,
    }
}

pub(crate) struct TasteProfileProjections {
    pub learned: Vec<LearnedTasteWeight>,
    pub source_affinities: Vec<SourceAffinity>,
}

pub(crate) fn taste_profile_projections(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    preferences: Option<&UserPreferences>,
) -> TasteProfileProjections {
    let mut projections = taste_signal_aggregates(store, user_id, tenant_id)
        .into_iter()
        .map(
            |(signal, aggregate)| match source_affinity_signal(&signal) {
                Some(source_signal) => TasteProfileProjection::Source(
                    aggregate.source_affinity(source_signal, preferences),
                ),
                None => TasteProfileProjection::Learned(aggregate.learned_weight(signal)),
            },
        )
        .collect::<Vec<_>>();
    projections.sort_by(|left, right| left.key().cmp(&right.key()));
    let mut source_affinities = Vec::new();
    let mut learned = Vec::new();
    for projection in projections {
        match projection {
            TasteProfileProjection::Source(affinity) => source_affinities.push(affinity),
            TasteProfileProjection::Learned(weight) => learned.push(weight),
        }
    }
    TasteProfileProjections {
        learned,
        source_affinities,
    }
}

#[derive(Default)]
struct TasteSignalAggregate {
    supporting_seeds: u32,
    supporting_feedback: u32,
    opposing_feedback: u32,
    evidence_counts: HashMap<LearnedTasteEvidenceKind, u32>,
}

impl TasteSignalAggregate {
    fn source_affinity(
        &self,
        signal: SourceAffinitySignal,
        preferences: Option<&UserPreferences>,
    ) -> SourceAffinity {
        let explicitly_blocked =
            preferences.is_some_and(|preferences| source_affinity_is_blocked(preferences, &signal));
        SourceAffinity {
            signal,
            weight: self.inferred_weight(explicitly_blocked),
            supporting_seeds: self.supporting_seeds,
            supporting_feedback: self.supporting_feedback,
            opposing_feedback: self.opposing_feedback,
            explicitly_blocked,
        }
    }

    fn learned_weight(self, signal: LearnedTasteSignal) -> LearnedTasteWeight {
        let weight = self.inferred_weight(false);
        let supporting_signals = self
            .supporting_seeds
            .saturating_add(self.supporting_feedback);
        let opposing_signals = self.opposing_feedback;
        let mut evidence_summary = self
            .evidence_counts
            .into_iter()
            .map(|(kind, count)| LearnedTasteEvidenceSummary { kind, count })
            .collect::<Vec<_>>();
        evidence_summary.sort_by_key(|summary| summary.kind);
        LearnedTasteWeight {
            signal,
            weight,
            supporting_signals,
            opposing_signals,
            evidence_summary,
        }
    }

    fn inferred_weight(&self, explicitly_blocked: bool) -> f32 {
        let supporting = self
            .supporting_seeds
            .saturating_add(self.supporting_feedback);
        if explicitly_blocked || supporting.saturating_add(self.opposing_feedback) < 2 {
            return 0.0;
        }
        let net = i64::from(supporting) - i64::from(self.opposing_feedback);
        f32::from(i16::try_from(net.clamp(-6, 6)).unwrap_or_default()) * 0.5
    }
}

enum TasteProfileProjection {
    Source(SourceAffinity),
    Learned(LearnedTasteWeight),
}

impl TasteProfileProjection {
    fn key(&self) -> (&str, &str) {
        match self {
            Self::Source(affinity) => source_affinity_key(&affinity.signal),
            Self::Learned(weight) => taste_signal_key(&weight.signal),
        }
    }
}

fn source_affinity_signal(signal: &LearnedTasteSignal) -> Option<SourceAffinitySignal> {
    match signal {
        LearnedTasteSignal::Topic(_) => None,
        LearnedTasteSignal::Source(value) => Some(SourceAffinitySignal::Source(value.clone())),
        LearnedTasteSignal::Publisher(value) => {
            Some(SourceAffinitySignal::Publisher(value.clone()))
        }
        LearnedTasteSignal::AuthorOrAccount(value) => {
            Some(SourceAffinitySignal::AuthorOrAccount(value.clone()))
        }
        LearnedTasteSignal::Community(value) => {
            Some(SourceAffinitySignal::Community(value.clone()))
        }
        LearnedTasteSignal::ReferrerContext(value) => {
            Some(SourceAffinitySignal::ReferrerContext(value.clone()))
        }
    }
}

pub(crate) fn source_affinity_key(signal: &SourceAffinitySignal) -> (&str, &str) {
    match signal {
        SourceAffinitySignal::Source(value) => ("source", value),
        SourceAffinitySignal::Publisher(value) => ("publisher", value),
        SourceAffinitySignal::AuthorOrAccount(value) => ("author_or_account", value),
        SourceAffinitySignal::Community(value) => ("community", value),
        SourceAffinitySignal::ReferrerContext(value) => ("referrer_context", value),
    }
}

pub(crate) fn source_affinity_signals_match(
    left: &SourceAffinitySignal,
    right: &SourceAffinitySignal,
) -> bool {
    match (left, right) {
        (SourceAffinitySignal::Source(left), SourceAffinitySignal::Source(right))
        | (SourceAffinitySignal::Publisher(left), SourceAffinitySignal::Publisher(right))
        | (
            SourceAffinitySignal::AuthorOrAccount(left),
            SourceAffinitySignal::AuthorOrAccount(right),
        )
        | (SourceAffinitySignal::Community(left), SourceAffinitySignal::Community(right))
        | (
            SourceAffinitySignal::ReferrerContext(left),
            SourceAffinitySignal::ReferrerContext(right),
        ) => left.eq_ignore_ascii_case(right),
        _ => false,
    }
}

pub(crate) fn source_affinity_is_blocked(
    preferences: &UserPreferences,
    signal: &SourceAffinitySignal,
) -> bool {
    preferences
        .blocked_source_affinities
        .iter()
        .any(|blocked| source_affinity_signals_match(blocked, signal))
        || matches!(signal, SourceAffinitySignal::Source(source)
            if preferences.blocked_sources.iter().any(|blocked| blocked.eq_ignore_ascii_case(source)))
}

fn taste_signal_aggregates(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
) -> HashMap<LearnedTasteSignal, TasteSignalAggregate> {
    let mut aggregates = HashMap::new();
    for seed in store.interest_seeds.values().filter(|seed| {
        seed.user_id == user_id && seed.tenant_id == tenant_id && seed.retracted_at.is_none()
    }) {
        let signals: HashSet<_> = seed
            .evidence
            .iter()
            .map(|evidence| evidence.signal.clone())
            .collect();
        for signal in signals {
            let aggregate: &mut TasteSignalAggregate = aggregates.entry(signal).or_default();
            aggregate.supporting_seeds = aggregate.supporting_seeds.saturating_add(1);
            let count = aggregate
                .evidence_counts
                .entry(LearnedTasteEvidenceKind::UserSubmission)
                .or_default();
            *count = count.saturating_add(1);
        }
    }
    for evidence in store
        .taste_learning_evidence
        .iter()
        .filter(|evidence| evidence.user_id == user_id && evidence.tenant_id == tenant_id)
    {
        let aggregate = aggregates.entry(evidence.signal.clone()).or_default();
        match evidence.direction {
            TasteEvidenceDirection::Supporting => {
                aggregate.supporting_feedback = aggregate.supporting_feedback.saturating_add(1);
            }
            TasteEvidenceDirection::Opposing => {
                aggregate.opposing_feedback = aggregate.opposing_feedback.saturating_add(1);
            }
        }
        let count = aggregate.evidence_counts.entry(evidence.kind).or_default();
        *count = count.saturating_add(1);
    }
    aggregates
}

pub(crate) fn reset_interest_seed_evidence(
    store: &mut InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    signal: Option<&LearnedTasteSignal>,
) {
    for seed in store
        .interest_seeds
        .values_mut()
        .filter(|seed| seed.user_id == user_id && seed.tenant_id == tenant_id)
    {
        if let Some(signal) = signal {
            seed.evidence
                .retain(|evidence| !taste_signals_match(&evidence.signal, signal));
            if seed.evidence.is_empty() {
                seed.retracted_at.get_or_insert_with(chrono::Utc::now);
            }
        } else {
            seed.retracted_at.get_or_insert_with(chrono::Utc::now);
        }
    }
}

pub(crate) fn taste_signal_key(signal: &LearnedTasteSignal) -> (&str, &str) {
    match signal {
        LearnedTasteSignal::Topic(value) => ("topic", value),
        LearnedTasteSignal::Source(value) => ("source", value),
        LearnedTasteSignal::Publisher(value) => ("publisher", value),
        LearnedTasteSignal::AuthorOrAccount(value) => ("author_or_account", value),
        LearnedTasteSignal::Community(value) => ("community", value),
        LearnedTasteSignal::ReferrerContext(value) => ("referrer_context", value),
    }
}

pub(crate) fn taste_signals_match(left: &LearnedTasteSignal, right: &LearnedTasteSignal) -> bool {
    left == right
}
