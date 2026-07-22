//! Constrained Feed Mix composition independent of persistence and delivery.

use crate::domain::{FeedItemKind, FeedMix, PodId, Submission};
use chrono::{DateTime, Utc};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub(crate) struct DeliveryRecord {
    pub(crate) delivered_at: DateTime<Utc>,
    pub(crate) pod_ids: HashSet<PodId>,
}

pub(crate) struct RankedFeedCandidate<'a> {
    pub(crate) item: &'a Submission,
    pub(crate) recurrence_penalty_applied: bool,
    pub(crate) score: f32,
    pub(crate) reasons: Vec<String>,
    pub(crate) kind: FeedItemKind,
    pub(crate) pod_ids: Vec<PodId>,
    pub(crate) priority_pod_ids: Vec<PodId>,
    /// Typed trial-exposure flag for Exploration Items (never inferred from strings).
    pub(crate) trial_exposure: bool,
}

pub(crate) fn compare_feed_candidates(
    left: &RankedFeedCandidate<'_>,
    right: &RankedFeedCandidate<'_>,
) -> Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| right.item.created_at.cmp(&left.item.created_at))
        .then_with(|| left.item.canonical_url.cmp(&right.item.canonical_url))
}

pub(crate) fn normalized_intent_topics(topics: &[String]) -> Vec<String> {
    topics
        .iter()
        .map(|topic| topic.trim().to_lowercase())
        .filter(|topic| !topic.is_empty())
        .collect()
}

pub(crate) fn content_matches_any_topic(item: &Submission, topics: &[String]) -> bool {
    topics.iter().any(|topic| {
        item.tags.iter().any(|tag| tag.eq_ignore_ascii_case(topic))
            || item.title.to_lowercase().contains(topic)
            || item
                .summary
                .as_ref()
                .is_some_and(|summary| summary.to_lowercase().contains(topic))
    })
}

pub(crate) fn compose_feed_candidates<'a>(
    candidates: Vec<RankedFeedCandidate<'a>>,
    size: usize,
    feed_mix: FeedMix,
) -> Vec<RankedFeedCandidate<'a>> {
    let mut subscribed = Vec::new();
    let mut exploration = Vec::new();
    let mut old_gems = Vec::new();
    for candidate in candidates {
        match candidate.kind {
            FeedItemKind::Subscribed => subscribed.push(candidate),
            FeedItemKind::Exploration => exploration.push(candidate),
            FeedItemKind::OldGem => old_gems.push(candidate),
        }
    }
    let subscribed_target =
        size.saturating_mul(usize::from(feed_mix.high_value_percent().value())) / 100;
    let exploration_target =
        size.saturating_mul(usize::from(feed_mix.exploration_percent().value())) / 100;
    let old_gem_target = size.saturating_mul(usize::from(feed_mix.old_gem_percent().value())) / 100;
    let mut state = SelectionState::new(size);
    select_priority_candidates(
        &mut subscribed,
        subscribed_target.max(1).min(size),
        feed_mix,
        &mut state,
    );
    let subscribed_remaining = subscribed_target.saturating_sub(state.selected.len());
    select_candidates(&mut subscribed, subscribed_remaining, feed_mix, &mut state);
    select_candidates(&mut exploration, exploration_target, feed_mix, &mut state);
    select_candidates(&mut old_gems, old_gem_target, feed_mix, &mut state);
    let mut backfill = subscribed;
    backfill.extend(exploration);
    backfill.extend(old_gems);
    backfill.sort_by(compare_feed_candidates);
    let remaining = size.saturating_sub(state.selected.len());
    select_candidates(&mut backfill, remaining, feed_mix, &mut state);
    state.selected
}

struct SelectionState<'a> {
    selected: Vec<RankedFeedCandidate<'a>>,
    pod_counts: HashMap<PodId, usize>,
    source_counts: HashMap<String, usize>,
}

impl<'a> SelectionState<'a> {
    fn new(size: usize) -> Self {
        Self {
            selected: Vec::with_capacity(size),
            pod_counts: HashMap::new(),
            source_counts: HashMap::new(),
        }
    }

    fn can_select(&self, candidate: &RankedFeedCandidate<'_>, feed_mix: FeedMix) -> bool {
        self.source_counts
            .get(&candidate.item.domain.to_lowercase())
            .copied()
            .unwrap_or_default()
            < feed_mix.per_source_cap().value()
            && !candidate.pod_ids.is_empty()
            && candidate.pod_ids.iter().all(|pod_id| {
                self.pod_counts.get(pod_id).copied().unwrap_or_default()
                    < feed_mix.per_pod_cap().value()
            })
    }

    fn push(&mut self, candidate: RankedFeedCandidate<'a>) {
        for pod_id in &candidate.pod_ids {
            let count = self.pod_counts.entry(*pod_id).or_default();
            *count = count.saturating_add(1);
        }
        let count = self
            .source_counts
            .entry(candidate.item.domain.to_lowercase())
            .or_default();
        *count = count.saturating_add(1);
        self.selected.push(candidate);
    }
}

fn select_priority_candidates<'a>(
    candidates: &mut Vec<RankedFeedCandidate<'a>>,
    limit: usize,
    feed_mix: FeedMix,
    state: &mut SelectionState<'a>,
) {
    let mut priority_pods = candidates
        .iter()
        .flat_map(|candidate| candidate.priority_pod_ids.iter().copied())
        .collect::<Vec<_>>();
    priority_pods.sort_unstable();
    priority_pods.dedup();
    let mut represented_pods = HashSet::new();
    let mut selected_items = 0;
    for priority_pod_id in priority_pods {
        if selected_items >= limit {
            break;
        }
        if represented_pods.contains(&priority_pod_id) {
            continue;
        }
        let Some(index) = candidates.iter().position(|candidate| {
            candidate.priority_pod_ids.contains(&priority_pod_id)
                && state.can_select(candidate, feed_mix)
        }) else {
            continue;
        };
        let mut candidate = candidates.remove(index);
        candidate
            .reasons
            .push("Priority Subscription guaranteed bounded representation".into());
        represented_pods.extend(candidate.priority_pod_ids.iter().copied());
        state.push(candidate);
        selected_items = selected_items.saturating_add(1);
    }
}

fn select_candidates<'a>(
    candidates: &mut Vec<RankedFeedCandidate<'a>>,
    limit: usize,
    feed_mix: FeedMix,
    state: &mut SelectionState<'a>,
) {
    let mut index = 0;
    let initial_len = state.selected.len();
    while index < candidates.len() && state.selected.len().saturating_sub(initial_len) < limit {
        let candidate = &candidates[index];
        if !state.can_select(candidate, feed_mix) {
            index = index.saturating_add(1);
            continue;
        }
        let candidate = candidates.remove(index);
        state.push(candidate);
    }
}
