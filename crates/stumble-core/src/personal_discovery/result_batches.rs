//! Personal Discovery Result Batch selection and policy enforcement.

use crate::domain::*;
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use url::Url;
use uuid::Uuid;

/// Build an ordered private batch from task-bound submissions under plan policy.
pub(crate) fn build_discovery_result_batch(
    store: &InMemoryStore,
    plan: &DiscoveryPlan,
    task_id: DiscoveryTaskId,
    ordered_submissions: &[&CandidateSubmission],
    candidates: &HashMap<CandidateId, Candidate>,
    reported: &[ReportedSourceAvailability],
    now: DateTime<Utc>,
) -> DiscoveryResultBatch {
    let mut reasons: Vec<DiscoveryResultAvailabilityReason> = reported
        .iter()
        .map(
            |report| DiscoveryResultAvailabilityReason::SourceUnavailable {
                source: report.source.clone(),
                reason: report.reason.clone(),
            },
        )
        .collect();

    let recent_canonical = recently_reviewed_canonical_urls(store, plan.user_id, plan.tenant_id);
    let mut state = SelectionState::default();
    let mut rejected_submissions = HashSet::new();

    // First pass: honor declared allocation roles without silent policy weakening.
    for submission in ordered_submissions {
        if state.selected.len() as u16 >= plan.result_count {
            break;
        }
        let CandidateSubmissionTarget::PersonalDiscovery {
            allocation_role,
            source_facts,
            ..
        } = &submission.target
        else {
            continue;
        };
        let Some(candidate) = candidates.get(&submission.candidate_id) else {
            continue;
        };
        if let Some(reason) =
            state.reject_for_policy(plan, candidate, submission, source_facts, &recent_canonical)
        {
            if rejected_submissions.insert(submission.id) {
                record_rejection(&reason, &mut reasons, &mut state);
            }
            continue;
        }
        let role_full = match allocation_role {
            DiscoveryPlanSourceRole::Proven => state.proven_filled >= plan.allocation.proven,
            DiscoveryPlanSourceRole::Adjacent => state.adjacent_filled >= plan.allocation.adjacent,
        };
        if role_full {
            continue;
        }
        state.push_selected(candidate, submission, *allocation_role, source_facts);
    }

    // Second pass: reallocate remaining slots across roles when one side underfills.
    if (state.selected.len() as u16) < plan.result_count {
        let mut reallocated_to_proven = 0_u16;
        let mut reallocated_to_adjacent = 0_u16;
        for submission in ordered_submissions {
            if state.selected.len() as u16 >= plan.result_count {
                break;
            }
            if state
                .selected
                .iter()
                .any(|item| item.submission_id == submission.id)
            {
                continue;
            }
            let CandidateSubmissionTarget::PersonalDiscovery {
                allocation_role,
                source_facts,
                ..
            } = &submission.target
            else {
                continue;
            };
            let Some(candidate) = candidates.get(&submission.candidate_id) else {
                continue;
            };
            if let Some(reason) = state.reject_for_policy(
                plan,
                candidate,
                submission,
                source_facts,
                &recent_canonical,
            ) {
                if rejected_submissions.insert(submission.id) {
                    record_rejection(&reason, &mut reasons, &mut state);
                }
                continue;
            }
            state.push_selected(candidate, submission, *allocation_role, source_facts);
            match allocation_role {
                DiscoveryPlanSourceRole::Proven => reallocated_to_proven += 1,
                DiscoveryPlanSourceRole::Adjacent => reallocated_to_adjacent += 1,
            }
        }
        if reallocated_to_proven > 0 {
            reasons.push(DiscoveryResultAvailabilityReason::Reallocated {
                from: DiscoveryPlanSourceRole::Adjacent,
                to: DiscoveryPlanSourceRole::Proven,
                count: reallocated_to_proven,
            });
        }
        if reallocated_to_adjacent > 0 {
            reasons.push(DiscoveryResultAvailabilityReason::Reallocated {
                from: DiscoveryPlanSourceRole::Proven,
                to: DiscoveryPlanSourceRole::Adjacent,
                count: reallocated_to_adjacent,
            });
        }
    }

    if state.proven_filled < plan.allocation.proven {
        reasons.push(DiscoveryResultAvailabilityReason::InsufficientProven {
            requested: plan.allocation.proven,
            filled: state.proven_filled.min(plan.allocation.proven),
        });
    }
    if state.adjacent_filled < plan.allocation.adjacent {
        reasons.push(DiscoveryResultAvailabilityReason::InsufficientAdjacent {
            requested: plan.allocation.adjacent,
            filled: state.adjacent_filled.min(plan.allocation.adjacent),
        });
    }
    if (state.selected.len() as u16) < plan.result_count {
        reasons.push(DiscoveryResultAvailabilityReason::Underfilled {
            requested: plan.result_count,
            filled: state.selected.len() as u16,
        });
    }
    flush_cap_reasons(
        &state.domain_rejected,
        |domain, rejected_count| DiscoveryResultAvailabilityReason::DomainCap {
            domain,
            rejected_count,
        },
        &mut reasons,
    );
    flush_cap_reasons(
        &state.author_rejected,
        |identity, rejected_count| DiscoveryResultAvailabilityReason::AuthorOrAccountCap {
            identity,
            rejected_count,
        },
        &mut reasons,
    );
    flush_cap_reasons(
        &state.publisher_rejected,
        |identity, rejected_count| DiscoveryResultAvailabilityReason::PublisherCap {
            identity,
            rejected_count,
        },
        &mut reasons,
    );
    flush_cap_reasons(
        &state.community_rejected,
        |identity, rejected_count| DiscoveryResultAvailabilityReason::CommunityCap {
            identity,
            rejected_count,
        },
        &mut reasons,
    );

    // Stable batch identity from task so retries cannot create a second batch.
    let id = stable_batch_id(task_id);
    DiscoveryResultBatch {
        id,
        user_id: plan.user_id,
        tenant_id: plan.tenant_id,
        task_id,
        plan_id: plan.id,
        state: DiscoveryResultBatchState::Ready,
        notification_state: DiscoveryResultNotificationState::NotApplicable,
        requested_size: plan.result_count,
        allocation: plan.allocation,
        allocation_filled: DiscoveryPlanAllocation {
            proven: state.proven_filled,
            adjacent: state.adjacent_filled,
        },
        items: state.selected,
        source_availability: reasons,
        created_at: now,
        reviewed_at: None,
        dismissed_at: None,
    }
}

#[derive(Default)]
struct SelectionState {
    selected: Vec<DiscoveryResultItem>,
    proven_filled: u16,
    adjacent_filled: u16,
    seen_canonical: HashSet<String>,
    domain_counts: HashMap<String, u16>,
    author_counts: HashMap<String, u16>,
    publisher_counts: HashMap<String, u16>,
    community_counts: HashMap<String, u16>,
    domain_rejected: HashMap<String, u16>,
    author_rejected: HashMap<String, u16>,
    publisher_rejected: HashMap<String, u16>,
    community_rejected: HashMap<String, u16>,
}

impl SelectionState {
    fn reject_for_policy(
        &self,
        plan: &DiscoveryPlan,
        candidate: &Candidate,
        submission: &CandidateSubmission,
        source_facts: &CandidateInterestSeedMetadata,
        recent_canonical: &HashSet<String>,
    ) -> Option<PolicyReject> {
        if let Some(detail) = blocked_detail(plan, candidate, submission, source_facts) {
            return Some(PolicyReject::Blocked(detail));
        }
        if plan.constraints.canonical_deduplication
            && self.seen_canonical.contains(&candidate.canonical_url)
        {
            return Some(PolicyReject::CanonicalDuplicate(
                candidate.canonical_url.clone(),
            ));
        }
        if plan.constraints.suppress_recently_reviewed
            && recent_canonical.contains(&candidate.canonical_url)
        {
            return Some(PolicyReject::RecentlyReviewed(
                candidate.canonical_url.clone(),
            ));
        }
        if let Some(domain) = domain_of(&candidate.canonical_url) {
            let count = self.domain_counts.get(&domain).copied().unwrap_or(0);
            if count >= plan.constraints.max_per_domain {
                return Some(PolicyReject::DomainCap(domain));
            }
        }
        if let Some(author) =
            normalized_optional(submission.evidence.source_metadata.author.as_deref())
        {
            let count = self.author_counts.get(&author).copied().unwrap_or(0);
            if count >= plan.constraints.max_per_author_or_account {
                return Some(PolicyReject::AuthorCap(author));
            }
        }
        if let Some(publisher) = normalized_optional(source_facts.publisher.as_deref()) {
            let count = self.publisher_counts.get(&publisher).copied().unwrap_or(0);
            if count >= plan.constraints.max_per_publisher {
                return Some(PolicyReject::PublisherCap(publisher));
            }
        }
        if let Some(community) = normalized_optional(source_facts.community.as_deref()) {
            let count = self.community_counts.get(&community).copied().unwrap_or(0);
            if count >= plan.constraints.max_per_community {
                return Some(PolicyReject::CommunityCap(community));
            }
        }
        None
    }

    fn push_selected(
        &mut self,
        candidate: &Candidate,
        submission: &CandidateSubmission,
        allocation_role: DiscoveryPlanSourceRole,
        source_facts: &CandidateInterestSeedMetadata,
    ) {
        let position = self.selected.len() as u16;
        self.seen_canonical.insert(candidate.canonical_url.clone());
        if let Some(domain) = domain_of(&candidate.canonical_url) {
            *self.domain_counts.entry(domain).or_insert(0) += 1;
        }
        if let Some(author) =
            normalized_optional(submission.evidence.source_metadata.author.as_deref())
        {
            *self.author_counts.entry(author).or_insert(0) += 1;
        }
        if let Some(publisher) = normalized_optional(source_facts.publisher.as_deref()) {
            *self.publisher_counts.entry(publisher).or_insert(0) += 1;
        }
        if let Some(community) = normalized_optional(source_facts.community.as_deref()) {
            *self.community_counts.entry(community).or_insert(0) += 1;
        }
        match allocation_role {
            DiscoveryPlanSourceRole::Proven => self.proven_filled += 1,
            DiscoveryPlanSourceRole::Adjacent => self.adjacent_filled += 1,
        }
        self.selected.push(DiscoveryResultItem {
            position,
            candidate_id: candidate.id,
            submission_id: submission.id,
            canonical_url: candidate.canonical_url.clone(),
            allocation_role,
            review: DiscoveryResultItemReview::Unreviewed,
        });
    }
}

fn stable_batch_id(task_id: DiscoveryTaskId) -> DiscoveryResultBatchId {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"stumble discovery result batch\0");
    hasher.update(task_id.to_string().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).into()
}

fn recently_reviewed_canonical_urls(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
) -> HashSet<String> {
    let mut urls = HashSet::new();
    for batch in store.discovery_result_batches.values().filter(|batch| {
        batch.user_id == user_id
            && batch.tenant_id == tenant_id
            && matches!(
                batch.state,
                DiscoveryResultBatchState::Ready
                    | DiscoveryResultBatchState::Reviewed
                    | DiscoveryResultBatchState::Dismissed
            )
    }) {
        for item in &batch.items {
            urls.insert(item.canonical_url.clone());
            // Explicit Not for me rejections also suppress equivalent spellings
            // even if batch membership were later reinterpreted.
            if matches!(
                item.review,
                DiscoveryResultItemReview::Reviewed {
                    action: DiscoveryResultItemAction::NotForMe,
                    ..
                }
            ) {
                urls.insert(item.canonical_url.clone());
            }
        }
    }
    urls
}

enum PolicyReject {
    Blocked(String),
    CanonicalDuplicate(String),
    RecentlyReviewed(String),
    DomainCap(String),
    AuthorCap(String),
    PublisherCap(String),
    CommunityCap(String),
}

fn blocked_detail(
    plan: &DiscoveryPlan,
    candidate: &Candidate,
    submission: &CandidateSubmission,
    source_facts: &CandidateInterestSeedMetadata,
) -> Option<String> {
    let domain = domain_of(&candidate.canonical_url);
    if let Some(domain) = domain.as_ref() {
        if plan
            .constraints
            .blocked_sources
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(domain))
        {
            return Some(format!("blocked source {domain}"));
        }
        let signal = SourceAffinitySignal::Source(domain.clone());
        if plan
            .constraints
            .blocked_source_affinities
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(&signal))
        {
            return Some(format!("blocked source affinity {domain}"));
        }
    }
    for tag in &submission.evidence.tags {
        if plan
            .constraints
            .blocked_topics
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(tag.trim()))
        {
            return Some(format!("blocked topic {}", tag.trim()));
        }
    }
    if let Some(author) = normalized_optional(submission.evidence.source_metadata.author.as_deref())
    {
        let signal = SourceAffinitySignal::AuthorOrAccount(author.clone());
        if plan
            .constraints
            .blocked_source_affinities
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(&signal))
        {
            return Some(format!("blocked author_or_account {author}"));
        }
    }
    if let Some(publisher) = normalized_optional(source_facts.publisher.as_deref()) {
        let signal = SourceAffinitySignal::Publisher(publisher.clone());
        if plan
            .constraints
            .blocked_source_affinities
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(&signal))
        {
            return Some(format!("blocked publisher {publisher}"));
        }
    }
    if let Some(community) = normalized_optional(source_facts.community.as_deref()) {
        let signal = SourceAffinitySignal::Community(community.clone());
        if plan
            .constraints
            .blocked_source_affinities
            .iter()
            .any(|blocked| blocked.eq_ignore_ascii_case(&signal))
        {
            return Some(format!("blocked community {community}"));
        }
    }
    None
}

fn record_rejection(
    reason: &PolicyReject,
    reasons: &mut Vec<DiscoveryResultAvailabilityReason>,
    state: &mut SelectionState,
) {
    match reason {
        PolicyReject::Blocked(detail) => {
            reasons.push(DiscoveryResultAvailabilityReason::Blocked {
                detail: detail.clone(),
            });
        }
        PolicyReject::CanonicalDuplicate(canonical_url) => {
            reasons.push(DiscoveryResultAvailabilityReason::CanonicalDuplicate {
                canonical_url: canonical_url.clone(),
            });
        }
        PolicyReject::RecentlyReviewed(canonical_url) => {
            reasons.push(DiscoveryResultAvailabilityReason::RecentlyReviewed {
                canonical_url: canonical_url.clone(),
            });
        }
        PolicyReject::DomainCap(domain) => {
            *state.domain_rejected.entry(domain.clone()).or_insert(0) += 1;
        }
        PolicyReject::AuthorCap(identity) => {
            *state.author_rejected.entry(identity.clone()).or_insert(0) += 1;
        }
        PolicyReject::PublisherCap(identity) => {
            *state
                .publisher_rejected
                .entry(identity.clone())
                .or_insert(0) += 1;
        }
        PolicyReject::CommunityCap(identity) => {
            *state
                .community_rejected
                .entry(identity.clone())
                .or_insert(0) += 1;
        }
    }
}

fn flush_cap_reasons(
    rejected: &HashMap<String, u16>,
    build: impl Fn(String, u16) -> DiscoveryResultAvailabilityReason,
    reasons: &mut Vec<DiscoveryResultAvailabilityReason>,
) {
    let mut entries: Vec<_> = rejected.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    for (key, count) in entries {
        reasons.push(build(key.clone(), *count));
    }
}

fn domain_of(url: &str) -> Option<String> {
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.domain().map(|domain| domain.to_ascii_lowercase()))
        .filter(|domain| !domain.is_empty())
}

fn normalized_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}
