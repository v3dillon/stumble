//! Exploration / trial caps that bound open-admission flooding.

use crate::domain::{FeedMix, NodeIdentityId};
use std::collections::HashMap;

/// Maximum Explore samples fetched from one Origin in a single request.
pub const MAX_ORIGIN_EXPLORE_SAMPLES: usize = 10;

/// Maximum Explore / trial results attributed to one Origin Node per response.
pub const MAX_RESULTS_PER_ORIGIN: usize = 3;

/// Maximum trial-exposure Exploration Items attributed to one Origin per batch.
pub const MAX_TRIAL_ITEMS_PER_ORIGIN: usize = 1;

/// Minimum non-endorsement similarity for labeled unendorsed trial exposure.
pub const TRIAL_SIMILARITY_THRESHOLD: f32 = 0.55;

/// Caps preventing open-admission flooding of Explore and Feed exploration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplorationCaps {
    /// Maximum results attributed to one Origin Node.
    pub per_origin: usize,
    /// Maximum items attributed to one Pod (Feed Mix).
    pub per_pod: usize,
    /// Maximum items from one source domain (Feed Mix).
    pub per_source: usize,
    /// Maximum trial items attributed to one Origin.
    pub per_origin_trial: usize,
}

impl ExplorationCaps {
    /// Builds caps from Feed Mix diversity limits plus Origin trial bounds.
    #[must_use]
    pub fn from_feed_mix(feed_mix: FeedMix) -> Self {
        Self {
            per_origin: MAX_RESULTS_PER_ORIGIN,
            per_pod: feed_mix.per_pod_cap().value(),
            per_source: feed_mix.per_source_cap().value(),
            per_origin_trial: MAX_TRIAL_ITEMS_PER_ORIGIN,
        }
    }

    /// Default caps for Explore responses (one result per Pod identity).
    #[must_use]
    pub const fn explore_defaults() -> Self {
        Self {
            per_origin: MAX_RESULTS_PER_ORIGIN,
            per_pod: 1,
            per_source: MAX_ORIGIN_EXPLORE_SAMPLES,
            per_origin_trial: MAX_RESULTS_PER_ORIGIN,
        }
    }
}

/// Running counters for Origin / Pod / source diversity during selection.
#[derive(Debug, Default, Clone)]
pub struct ExplorationCapTracker {
    origin_counts: HashMap<NodeIdentityId, usize>,
    origin_trial_counts: HashMap<NodeIdentityId, usize>,
    pod_counts: HashMap<(NodeIdentityId, String), usize>,
    source_counts: HashMap<String, usize>,
}

impl ExplorationCapTracker {
    /// Creates empty counters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether another result for this Origin is within the per-Origin cap.
    #[must_use]
    pub fn can_admit_origin(&self, origin: NodeIdentityId, caps: ExplorationCaps) -> bool {
        self.origin_counts.get(&origin).copied().unwrap_or_default() < caps.per_origin
    }

    /// Whether another trial item for this Origin is within the trial cap.
    #[must_use]
    pub fn can_admit_trial(&self, origin: NodeIdentityId, caps: ExplorationCaps) -> bool {
        self.origin_trial_counts
            .get(&origin)
            .copied()
            .unwrap_or_default()
            < caps.per_origin_trial
    }

    /// Whether another item for this Pod is within the per-Pod cap.
    #[must_use]
    pub fn can_admit_pod(
        &self,
        origin: NodeIdentityId,
        pod_slug: &str,
        caps: ExplorationCaps,
    ) -> bool {
        self.pod_counts
            .get(&(origin, pod_slug.to_lowercase()))
            .copied()
            .unwrap_or_default()
            < caps.per_pod
    }

    /// Whether another item from this source domain is within the per-source cap.
    #[must_use]
    pub fn can_admit_source(&self, source: &str, caps: ExplorationCaps) -> bool {
        self.source_counts
            .get(&source.to_lowercase())
            .copied()
            .unwrap_or_default()
            < caps.per_source
    }

    /// Records one admitted result.
    pub fn record(
        &mut self,
        origin: NodeIdentityId,
        pod_slug: &str,
        source: Option<&str>,
        trial: bool,
    ) {
        *self.origin_counts.entry(origin).or_default() += 1;
        *self
            .pod_counts
            .entry((origin, pod_slug.to_lowercase()))
            .or_default() += 1;
        if trial {
            *self.origin_trial_counts.entry(origin).or_default() += 1;
        }
        if let Some(source) = source {
            *self.source_counts.entry(source.to_lowercase()).or_default() += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn origin_and_trial_caps_track_independently() {
        let origin = Uuid::now_v7().into();
        let caps = ExplorationCaps {
            per_origin: 2,
            per_pod: 1,
            per_source: 5,
            per_origin_trial: 1,
        };
        let mut tracker = ExplorationCapTracker::new();
        assert!(tracker.can_admit_origin(origin, caps));
        assert!(tracker.can_admit_trial(origin, caps));
        tracker.record(origin, "pod-a", Some("example.com"), true);
        assert!(tracker.can_admit_origin(origin, caps));
        assert!(!tracker.can_admit_trial(origin, caps));
        tracker.record(origin, "pod-b", None, false);
        assert!(!tracker.can_admit_origin(origin, caps));
    }
}
