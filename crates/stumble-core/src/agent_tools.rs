use crate::bootstrap::{
    add_bootstrap_endpoint, admit_bootstrap_announcement, admit_bootstrap_withdrawal,
    apply_bootstrap_stream_pages, bootstrap_endpoint_statuses, ensure_default_bootstrap_endpoint,
    fetch_bootstrap_stream_pages, list_bootstrap_endpoints, plan_bootstrap_sync,
    project_bootstrap_withdrawal, read_announcement_stream, remove_bootstrap_endpoint,
    set_bootstrap_endpoint_enabled, AnnouncementStreamClient, OriginProbe, UnreachableOriginProbe,
};
use crate::discovery_peer::{
    admit_discovery_peer_advertisement, apply_discovery_peer_stream_pages,
    disable_discovery_peer_service, discovery_status, enable_discovery_peer_service,
    ensure_discovery_peer_gossip_config, fetch_discovery_peer_stream_pages,
    list_active_outbound_peers, outbound_discovery_peer_statuses, peer_gossip_is_enabled,
    peer_service_is_enabled, plan_discovery_peer_sync, project_peer_serving_announcement,
    read_peer_announcement_stream, renew_discovery_peer_advertisement,
    sample_discovery_peer_advertisements, sample_known_discovery_peer_advertisements,
    set_automatic_peer_gossip_enabled, DiscoveryPeerProbe, DiscoveryPeerStreamClient,
    PeerAdvertisementSampleClient, UnreachableDiscoveryPeerProbe,
};
use crate::domain::*;
use crate::feed_mix::{
    compare_feed_candidates, compose_feed_candidates, content_matches_any_topic,
    normalized_intent_topics, DeliveryRecord, RankedFeedCandidate,
};
use crate::index::{
    import_from_configured_indexes, retain_index_search_results, search_index_catalog,
    IndexSearchClient,
};
use crate::interest_seeds::{
    candidate_submission_taste_signals, interest_seed_evidence, record_interest_seed,
    reset_interest_seed_evidence, source_affinity_is_blocked, taste_profile_projections,
    TasteProfileProjections,
};
use crate::personal_discovery::{
    build_discovery_result_batch, build_plan, clear_discovery_result_learning,
    discovery_result_allowed_actions, ensure_private_inbox, ensure_results_ready_event,
    evaluate_authentication_notices, materialize_due_personal_schedules,
    normalize_browser_grant_eligibility, normalize_intent, normalize_reports,
    notification_state_for_schedule, prepare_request, readiness, record_discovery_result_learning,
    resolve_completion_reports, retry, schedule_status, set_discovery_result_learning_link,
    task_is_scheduled, upsert_task_source_availability, validate_name, validate_result_count,
    BatchAvailabilityInput, DiscoveryResultLearningInput, TaskAvailabilityIdentity,
};
use crate::pod_announcement::{
    announcement_is_discovery_eligible, issue_and_retain_origin_pod_announcement,
    issue_origin_pod_withdrawal, refresh_public_pod_announcement_if_needed,
    retain_verified_pod_announcement, retain_verified_pod_withdrawal, validate_public_pod_url,
    DeliveryProvenance,
};
use crate::pod_similarity::{
    announcement_scoring_eligible, append_trial_exposure_label, collect_policy_endorsements,
    feedback_affects_future_exposure, fetch_verified_origin_explore_samples, rank_similar_pods,
    score_exploration_item, verify_explore_samples_for_announcement, ExplorationCapTracker,
    ExplorationCaps, LocalSimilarityContext, OriginExploreSampleClient, OwnedCandidateEvidence,
    PodSimilarityScore, SampleFetchError, MAX_RESULTS_PER_ORIGIN, MAX_TRIAL_ITEMS_PER_ORIGIN,
};
use crate::ranking::{rank_discovery, RankingInput};
use crate::signing::{
    hash_api_token, new_plaintext_api_token, sign_pod_endorsement, sign_pod_explore_samples,
    sign_public_event, verify_event, SigningError,
};
use crate::skill_pack::{
    default_skill_pack, export_skill_pack, fork_skill_pack, import_skill_pack, patch_skill_pack,
    pod_package_contents_from_files, source_rule_cadences, validate_pod_package_contents,
    validate_portable_package_files, validate_skill_pack, SourceRuleCadence,
};
use crate::store::{
    load_or_initialize_sqlite_store, load_sqlite_store, persist_sqlite_store_changes,
    save_store_snapshot, sqlite_home_node_is_initialized, FederatedContentItemKey, InMemoryStore,
    StoreError, StorePersistenceError,
};
use chrono::{Duration, Utc};
use rand_core::{OsRng, RngCore};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use url::Url;
use uuid::Uuid;

/// Cryptographic server entropy used for production peer-advertisement sampling.
fn server_sample_seed() -> u64 {
    OsRng.next_u64()
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AgentToolsError {
    /// The Explore request's bounds are invalid.
    #[error(transparent)]
    ExploreRequest(#[from] ExploreRequestError),
    /// Origin Explore sample fetch or verification failed.
    #[error(transparent)]
    SampleFetch(#[from] SampleFetchError),
    #[error(transparent)]
    CurationRationale(#[from] CurationRationaleError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Signing(#[from] SigningError),
    #[error(transparent)]
    Persistence(#[from] StorePersistenceError),
    #[error("lock poisoned")]
    LockPoisoned,
    #[error("bad url: {0}")]
    BadUrl(String),
    #[error("harness authorization denied: {reason}")]
    Forbidden { reason: String },
    #[error("Discovery Task lease is held by another harness")]
    TaskLeaseConflict,
    #[error("Discovery Task is terminal")]
    TaskTerminal,
    #[error("Discovery Task has no active lease owned by this harness")]
    TaskLeaseRequired,
    #[error("candidate submission requires an authenticated Agent Harness")]
    CandidateHarnessRequired,
    #[error("unattended candidate submission requires a Discovery Task")]
    CandidateTaskRequired,
    #[error("candidate submission requires the submitting harness to own the active task lease")]
    CandidateTaskLeaseRequired,
    #[error("candidate submission Pod Package version does not match its Discovery Task")]
    CandidatePackageVersionMismatch,
    #[error("candidate submission idempotency key was reused with different input")]
    CandidateIdempotencyConflict,
    #[error("Personal Discovery needs an explicit interest, corroborated User evidence, or temporary intent")]
    PersonalDiscoveryNotReady,
    #[error("Personal Discovery idempotency key was reused with different input")]
    PersonalDiscoveryIdempotencyConflict,
    #[error("Home Node is not initialized")]
    NodeNotInitialized,
    #[error("Home Node is already initialized")]
    NodeAlreadyInitialized,
    /// A remote node advertises a protocol this node cannot safely interpret.
    #[error("incompatible protocol version {received}; this node supports {supported}")]
    IncompatibleProtocol {
        /// Protocol version advertised by the remote node.
        received: String,
        /// Protocol version supported by this node.
        supported: &'static str,
    },
    /// Open Bootstrap admission or stream request was rejected.
    #[error("bootstrap rejected: {reason}")]
    BootstrapRejected {
        /// Stable machine-readable rejection reason.
        reason: BootstrapAdmissionRejectionReason,
        /// Human-readable detail for operators and Origins.
        message: String,
    },
    /// Public Index search failed with a bounded typed outcome.
    #[error(transparent)]
    IndexSearch(#[from] IndexSearchFailure),
    /// Discovery Peer enablement, admission, or serving was rejected.
    #[error("discovery peer rejected: {reason}")]
    DiscoveryPeerRejected {
        /// Stable machine-readable rejection reason.
        reason: DiscoveryPeerAdmissionRejectionReason,
        /// Human-readable detail for operators.
        message: String,
    },
}

const MAX_DISCOVERY_TASK_ATTEMPTS: usize = 3;
const DEFAULT_PENDING_PROPOSAL_SECONDS: u64 = 3_600;

#[derive(Clone)]
pub struct AgentTools {
    store: Arc<RwLock<InMemoryStore>>,
    persistence: Option<Persistence>,
    /// Independent Bootstrap capability configuration (not a Hub role).
    bootstrap: BootstrapCapability,
    /// Independent Index capability (may share a process with Bootstrap).
    index: IndexCapability,
    /// Injectable reachability probe for Discovery Peer enablement and admission.
    discovery_peer_probe: Arc<dyn DiscoveryPeerProbe>,
}

/// Runtime configuration for open Bootstrap admission and Announcement Streams.
#[derive(Clone)]
struct BootstrapCapability {
    enabled: bool,
    origin_probe: Arc<dyn OriginProbe>,
}

impl Default for BootstrapCapability {
    fn default() -> Self {
        Self {
            enabled: false,
            origin_probe: Arc::new(UnreachableOriginProbe),
        }
    }
}

/// Runtime configuration for public Index announcement search.
#[derive(Clone)]
struct IndexCapability {
    enabled: bool,
}

impl Default for IndexCapability {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Clone)]
enum Persistence {
    Json(Arc<PathBuf>),
    Sqlite {
        path: Arc<PathBuf>,
        baseline: Arc<Mutex<InMemoryStore>>,
    },
}

impl AgentTools {
    /// Returns the current stable Feed Batch or creates and delivers a new one.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, no User is authenticated,
    /// the request is invalid, or persistence fails.
    pub fn get_feed_batch(
        &self,
        ctx: &AuthContext,
        request: FeedBatchRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<FeedBatch, AgentToolsError> {
        if !(1..=100).contains(&request.size) {
            return Err(
                StoreError::Validation("Feed Batch size must be between 1 and 100".into()).into(),
            );
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Feed Batch requires an authenticated User".into())
        })?;
        let scoped_pod_ids =
            harness_for_context(&store, ctx)?.and_then(|harness| harness.grant.pod_ids.clone());
        if let Some(batch) = store.feed_batches.values().find(|batch| {
            batch.user_id == user_id
                && batch.tenant_id == ctx.tenant_id
                && batch.harness_id == ctx.harness_id
                && batch.completed_at.is_none()
        }) {
            return project_feed_batch_for_context(&store, ctx, batch);
        }

        let preferences = store.user_preferences.get(&(user_id, ctx.tenant_id));
        let recurrence_penalty_days = request.recurrence_penalty_days.unwrap_or_else(|| {
            preferences.map_or_else(RecurrencePenaltyDays::default, |preferences| {
                preferences.recurrence_penalty_days
            })
        });
        let recurrence_cutoff = now - Duration::days(i64::from(recurrence_penalty_days.get()));
        let mut last_delivered = HashMap::<SubmissionId, DeliveryRecord>::new();
        for batch in store
            .feed_batches
            .values()
            .filter(|batch| batch.user_id == user_id && batch.tenant_id == ctx.tenant_id)
        {
            for item in &batch.items {
                let submission_id = SubmissionId::from(item.content_reference.content_item_id);
                let record = DeliveryRecord {
                    delivered_at: batch.created_at,
                    pod_ids: item
                        .placements
                        .iter()
                        .map(|placement| placement.pod_id)
                        .collect(),
                };
                last_delivered
                    .entry(submission_id)
                    .and_modify(|existing| {
                        if record.delivered_at > existing.delivered_at {
                            *existing = record.clone();
                        }
                    })
                    .or_insert(record);
            }
        }
        let rejected: HashSet<SubmissionId> = store
            .feedback_events
            .iter()
            .filter(|event| event.user_id == user_id && event.tenant_id == ctx.tenant_id)
            .filter(|event| {
                matches!(
                    event.event_type,
                    FeedbackKind::Dismissed | FeedbackKind::NotForMe
                )
            })
            .map(|event| event.submission_id)
            .collect();
        let focus_topics = normalized_intent_topics(&request.batch_intent.focus_topics);
        let avoid_topics = normalized_intent_topics(&request.batch_intent.avoid_topics);
        let mut eligible = store
            .submissions
            .values()
            .filter(|item| item.tenant_id == ctx.tenant_id)
            .filter(|item| {
                store
                    .accepted_placement_projections
                    .keys()
                    .any(|(content_item_id, pod_id)| {
                        *content_item_id == item.id.into()
                            && scoped_pod_ids
                                .as_ref()
                                .is_none_or(|pod_ids| pod_ids.contains(pod_id))
                    })
            })
            .filter(|item| !rejected.contains(&item.id))
            .filter(|item| !content_matches_any_topic(item, &avoid_topics))
            .filter(|item| {
                preferences.is_none_or(|preferences| {
                    !source_affinity_is_blocked(
                        preferences,
                        &SourceAffinitySignal::Source(item.domain.clone()),
                    ) && !preferences.blocked_topics.iter().any(|topic| {
                        item.tags.iter().any(|tag| tag.eq_ignore_ascii_case(topic))
                            || item.title.to_lowercase().contains(&topic.to_lowercase())
                    })
                })
            })
            .filter_map(|item| {
                let mut placement_pod_ids = store
                    .accepted_placement_projections
                    .values()
                    .filter(|placement| {
                        placement.content_item_id == item.id.into()
                            && scoped_pod_ids
                                .as_ref()
                                .is_none_or(|pod_ids| pod_ids.contains(&placement.pod_id))
                    })
                    .map(|placement| placement.pod_id)
                    .collect::<Vec<_>>();
                placement_pod_ids.sort_unstable();
                placement_pod_ids.dedup();
                let subscribed_pod_ids = placement_pod_ids
                    .iter()
                    .copied()
                    .filter(|pod_id| {
                        store.subscriptions.values().any(|subscription| {
                            subscription.user_id == user_id && subscription.local_pod_id == *pod_id
                        })
                    })
                    .collect::<Vec<_>>();
                let priority_pod_ids = subscribed_pod_ids
                    .iter()
                    .copied()
                    .filter(|pod_id| {
                        store.subscriptions.values().any(|subscription| {
                            subscription.user_id == user_id
                                && subscription.local_pod_id == *pod_id
                                && subscription.is_priority
                        })
                    })
                    .collect::<Vec<_>>();
                let is_exploration = subscribed_pod_ids.is_empty()
                    && placement_pod_ids.iter().any(|pod_id| {
                        store
                            .pods
                            .get(pod_id)
                            .is_some_and(|pod| pod.visibility == Visibility::Public)
                    });
                if subscribed_pod_ids.is_empty() && !is_exploration {
                    return None;
                }
                let delivery = last_delivered.get(&item.id);
                let has_new_placement = delivery.is_some_and(|delivery| {
                    store
                        .accepted_placement_projections
                        .values()
                        .any(|placement| {
                            placement.content_item_id == item.id.into()
                                && !delivery.pod_ids.contains(&placement.pod_id)
                        })
                });
                let feedback_state = feed_feedback_state(&store, user_id, item.id);
                let has_strong_feedback = feedback_state.saved && feedback_state.more_like_this;
                let has_matching_intent = content_matches_any_topic(item, &focus_topics);
                let recurrence_penalty_applied = recurrence_penalty_days.get() > 0
                    && delivery.is_some_and(|delivery| delivery.delivered_at >= recurrence_cutoff);
                let kind = match delivery {
                    Some(_)
                        if has_matching_intent
                            || !recurrence_penalty_applied
                            || has_new_placement
                            || has_strong_feedback =>
                    {
                        FeedItemKind::OldGem
                    }
                    Some(_) => return None,
                    None if is_exploration => FeedItemKind::Exploration,
                    None => FeedItemKind::Subscribed,
                };
                let (mut score, mut reasons) =
                    feed_attention_value(&store, user_id, item, scoped_pod_ids.as_deref(), now);
                if recurrence_penalty_applied {
                    score -= 2.5;
                    reasons.push("Recent delivery applied a recurrence penalty".into());
                } else {
                    reasons.push("Item is outside the recurrence penalty window".into());
                }
                if has_matching_intent {
                    score += 1.0;
                    reasons.push(format!(
                        "Batch Intent focus matched: {}",
                        request.batch_intent.focus_topics.join(", ")
                    ));
                }
                if !request.batch_intent.avoid_topics.is_empty() {
                    reasons.push(format!(
                        "Batch Intent avoided: {}",
                        request.batch_intent.avoid_topics.join(", ")
                    ));
                }
                // Local Pod Similarity for Exploration Items (deterministic, no model).
                let mut trial_exposure = false;
                if kind == FeedItemKind::Exploration {
                    if let Some(similarity) = exploration_similarity_for_item(
                        &store,
                        user_id,
                        ctx.tenant_id,
                        item,
                        &placement_pod_ids,
                        preferences,
                    ) {
                        score += similarity.score;
                        reasons.extend(
                            similarity
                                .reasons
                                .iter()
                                .map(crate::pod_similarity::SimilarityReason::display),
                        );
                        trial_exposure = similarity.trial_exposure;
                        // Label trial once at the DTO boundary; trial is not an evidence kind.
                        append_trial_exposure_label(&mut reasons, similarity.trial_exposure);
                    }
                }
                let cap_pod_ids = if subscribed_pod_ids.is_empty() {
                    placement_pod_ids
                } else {
                    subscribed_pod_ids
                };
                Some(RankedFeedCandidate {
                    item,
                    recurrence_penalty_applied,
                    score,
                    reasons,
                    kind,
                    pod_ids: cap_pod_ids,
                    priority_pod_ids,
                    trial_exposure,
                })
            })
            .filter(|candidate| candidate.score > 0.0)
            .collect::<Vec<_>>();
        eligible.sort_by(compare_feed_candidates);

        let allowed_actions = feed_allowed_actions(&store, ctx)?;

        let selected = compose_feed_candidates(eligible, request.size, request.feed_mix);
        // Apply per-Origin exploration / trial caps on top of Feed Mix pod/source caps.
        let selected = apply_exploration_origin_caps(&store, user_id, selected);
        let items = selected
            .into_iter()
            .map(|candidate| {
                feed_batch_item(
                    &store,
                    user_id,
                    candidate.item,
                    &allowed_actions,
                    scoped_pod_ids.as_deref(),
                    FeedItemSelection {
                        recurrence_penalty_applied: candidate.recurrence_penalty_applied,
                        attention_value: candidate.score,
                        reasons: candidate.reasons,
                        kind: candidate.kind,
                    },
                )
            })
            .collect::<Vec<_>>();
        let state = if items.is_empty() {
            FeedBatchState::CaughtUp
        } else {
            FeedBatchState::Ready
        };
        let batch = FeedBatch {
            id: Uuid::now_v7(),
            user_id,
            harness_id: ctx.harness_id,
            tenant_id: ctx.tenant_id,
            requested_size: request.size,
            recurrence_penalty_days: recurrence_penalty_days.get(),
            feed_mix: request.feed_mix,
            batch_intent: request.batch_intent,
            state,
            items,
            created_at: now,
            completed_at: None,
        };
        store.feed_batches.insert(batch.id, batch.clone());
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::CreateFeedBatch,
            None,
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(batch)
    }

    /// Marks the current finite Feed Batch consumed so the User may deliberately request another.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the batch is missing or belongs
    /// to another User, or persistence fails.
    pub fn complete_feed_batch(
        &self,
        ctx: &AuthContext,
        batch_id: Uuid,
        now: chrono::DateTime<Utc>,
    ) -> Result<FeedBatch, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Feed Batch requires an authenticated User".into())
        })?;
        let batch = store
            .feed_batches
            .get_mut(&batch_id)
            .filter(|batch| {
                batch.user_id == user_id
                    && batch.tenant_id == ctx.tenant_id
                    && batch.harness_id == ctx.harness_id
            })
            .ok_or_else(|| StoreError::NotFound("Feed Batch".into()))?;
        let newly_completed = batch.completed_at.is_none();
        batch.completed_at.get_or_insert(now);
        batch.state = FeedBatchState::CaughtUp;
        let batch = batch.clone();
        if newly_completed {
            record_harness_write_at(
                &mut store,
                ctx,
                HarnessWriteOperation::CompleteFeedBatch,
                None,
                now,
            );
        }
        self.persist_locked(&mut store)?;
        Ok(batch)
    }

    /// Records one explicit private Feedback Signal for a delivered Content Item.
    ///
    /// # Errors
    ///
    /// Returns an error when feedback is denied, the item is missing or outside
    /// the Harness Grant's Pod scope, no User is authenticated, or persistence fails.
    pub fn record_feed_feedback(
        &self,
        ctx: &AuthContext,
        content_item_id: ContentItemId,
        kind: FeedbackKind,
        topic: Option<String>,
        reason: Option<String>,
        now: chrono::DateTime<Utc>,
    ) -> Result<FeedFeedbackState, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Feedback, None)?;
        authorize_interactive_user_action(
            &store,
            ctx,
            "Feedback Signal recording requires an interactive User action",
        )?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Feedback Signal requires an authenticated User".into())
        })?;
        let submission_id = SubmissionId::from(content_item_id);
        let item = store
            .submissions
            .get(&submission_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
        store.assert_tenant(item.tenant_id, ctx.tenant_id)?;
        authorize_feed_item_scope(&store, ctx, content_item_id)?;
        let was_delivered = store.feed_batches.values().any(|batch| {
            batch.user_id == user_id
                && batch.tenant_id == ctx.tenant_id
                && batch.items.iter().any(|batch_item| {
                    batch_item.content_reference.content_item_id == content_item_id
                })
        });
        if !was_delivered {
            return Err(
                StoreError::Validation("Feedback Signal requires a Delivered Item".into()).into(),
            );
        }
        let blocked_topic = if kind == FeedbackKind::BlockTopic {
            let requested = topic
                .filter(|topic| !topic.trim().is_empty())
                .ok_or_else(|| {
                    StoreError::Validation("topic block requires a non-empty target topic".into())
                })?;
            Some(
                item.tags
                    .iter()
                    .find(|tag| tag.eq_ignore_ascii_case(requested.trim()))
                    .cloned()
                    .ok_or_else(|| {
                        StoreError::Validation(
                            "topic block target must be one of the Delivered Item's topics".into(),
                        )
                    })?,
            )
        } else {
            None
        };
        // Durable preference changes only for explicit feedback kinds (not dismiss/ignore).
        let affects_future = feedback_affects_future_exposure(kind);
        match kind {
            FeedbackKind::Saved if affects_future => {
                store.saves.insert((user_id, submission_id));
            }
            FeedbackKind::BlockSource | FeedbackKind::BlockTopic if affects_future => {
                let source = item.domain.clone();
                let preferences = store
                    .user_preferences
                    .entry((user_id, ctx.tenant_id))
                    .or_insert(UserPreferences {
                        user_id,
                        tenant_id: ctx.tenant_id,
                        interests: vec![],
                        blocked_topics: vec![],
                        blocked_sources: vec![],
                        blocked_source_affinities: vec![],
                        preferred_brief_length: 7,
                        preferred_discovery_mode: DiscoveryMode::DeepMatch,
                        recurrence_penalty_days: RecurrencePenaltyDays::default(),
                    });
                if kind == FeedbackKind::BlockSource
                    && !preferences.blocked_sources.contains(&source)
                {
                    preferences.blocked_sources.push(source);
                }
                if let Some(topic) = blocked_topic {
                    if !preferences.blocked_topics.contains(&topic) {
                        preferences.blocked_topics.push(topic);
                    }
                }
            }
            FeedbackKind::Interesting
            | FeedbackKind::NotForMe
            | FeedbackKind::Dismissed
            | FeedbackKind::Saved
            | FeedbackKind::BlockSource
            | FeedbackKind::BlockTopic => {}
        }
        let is_new_feedback = !store.feedback_events.iter().any(|event| {
            event.user_id == user_id
                && event.tenant_id == ctx.tenant_id
                && event.submission_id == submission_id
                && event.event_type == kind
        });
        if is_new_feedback {
            store.feedback_events.push(FeedbackEvent {
                user_id,
                tenant_id: ctx.tenant_id,
                submission_id,
                event_type: kind,
                reason,
                created_at: now,
                local_only: true,
            });
            if affects_future {
                if let Some((evidence_kind, direction)) = taste_evidence_for_feedback(kind) {
                    record_taste_learning_evidence(
                        &mut store,
                        user_id,
                        ctx.tenant_id,
                        &item,
                        evidence_kind,
                        direction,
                        now,
                    );
                }
            }
        }
        let state = feed_feedback_state(&store, user_id, submission_id);
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::RecordFeedFeedback,
            None,
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(state)
    }
    pub fn new(store: InMemoryStore) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            persistence: None,
            bootstrap: BootstrapCapability::default(),
            index: IndexCapability::default(),
            discovery_peer_probe: Arc::new(UnreachableDiscoveryPeerProbe),
        }
    }

    pub fn new_persistent(store: InMemoryStore, path: impl Into<PathBuf>) -> Self {
        Self {
            store: Arc::new(RwLock::new(store)),
            persistence: Some(Persistence::Json(Arc::new(path.into()))),
            bootstrap: BootstrapCapability::default(),
            index: IndexCapability::default(),
            discovery_peer_probe: Arc::new(UnreachableDiscoveryPeerProbe),
        }
    }

    pub fn new_sqlite_persistent(store: InMemoryStore, path: impl Into<PathBuf>) -> Self {
        Self {
            store: Arc::new(RwLock::new(store.clone())),
            persistence: Some(Persistence::Sqlite {
                path: Arc::new(path.into()),
                baseline: Arc::new(Mutex::new(store)),
            }),
            bootstrap: BootstrapCapability::default(),
            index: IndexCapability::default(),
            discovery_peer_probe: Arc::new(UnreachableDiscoveryPeerProbe),
        }
    }

    /// Enables or reconfigures the independent Bootstrap capability.
    ///
    /// Bootstrap does not imply Hub authority or private-state proxying. The
    /// injected [`OriginProbe`] verifies Origin reachability and public manifests.
    #[must_use]
    pub fn with_bootstrap_capability(
        mut self,
        enabled: bool,
        origin_probe: Arc<dyn OriginProbe>,
    ) -> Self {
        self.bootstrap = BootstrapCapability {
            enabled,
            origin_probe,
        };
        self
    }

    /// Enables or disables the independent Index capability.
    ///
    /// Index may share a process with Bootstrap. Enabling Index does not grant
    /// ranking or trust authority over Home Nodes.
    #[must_use]
    pub fn with_index_capability(mut self, enabled: bool) -> Self {
        self.index = IndexCapability { enabled };
        self
    }

    /// Returns whether this process currently enables open Bootstrap admission.
    #[must_use]
    pub fn bootstrap_enabled(&self) -> bool {
        self.bootstrap.enabled
    }

    /// Returns whether this process currently enables public Index search.
    #[must_use]
    pub fn index_enabled(&self) -> bool {
        self.index.enabled
    }

    /// Injects the reachability probe used for Discovery Peer enablement and
    /// Bootstrap peer-advertisement admission.
    #[must_use]
    pub fn with_discovery_peer_probe(mut self, probe: Arc<dyn DiscoveryPeerProbe>) -> Self {
        self.discovery_peer_probe = probe;
        self
    }

    /// Returns whether this Home Node currently enables inbound Discovery Peer serving.
    ///
    /// Default is false: ordinary Home Nodes remain outbound-only for discovery.
    #[must_use]
    pub fn discovery_peer_service_enabled(&self) -> bool {
        self.store
            .read()
            .map(|store| peer_service_is_enabled(&store))
            .unwrap_or(false)
    }

    pub fn open_home_node(
        data_dir: impl AsRef<Path>,
        seed: impl FnOnce() -> InMemoryStore,
    ) -> Result<Self, AgentToolsError> {
        let database_path = data_dir.as_ref().join("stumble.sqlite3");
        let legacy_path = data_dir.as_ref().join("store.json");
        let store = load_or_initialize_sqlite_store(&database_path, &legacy_path, seed)?;
        Ok(Self::new_sqlite_persistent(store, database_path))
    }

    /// Returns whether the selected path contains initialized Home Node state.
    pub fn home_node_is_initialized(data_dir: impl AsRef<Path>) -> Result<bool, AgentToolsError> {
        Ok(sqlite_home_node_is_initialized(
            &data_dir.as_ref().join("stumble.sqlite3"),
        )?)
    }

    /// Initializes a new Home Node and refuses to reopen an existing one.
    pub fn initialize_home_node(
        data_dir: impl AsRef<Path>,
        seed: impl FnOnce() -> InMemoryStore,
    ) -> Result<Self, AgentToolsError> {
        let data_dir = data_dir.as_ref();
        if Self::home_node_is_initialized(data_dir)? {
            return Err(AgentToolsError::NodeAlreadyInitialized);
        }
        Self::open_home_node(data_dir, seed)
    }

    /// Opens existing Home Node state without initializing an empty path.
    pub fn open_initialized_home_node(data_dir: impl AsRef<Path>) -> Result<Self, AgentToolsError> {
        let data_dir = data_dir.as_ref();
        let database_path = data_dir.join("stumble.sqlite3");
        if !sqlite_home_node_is_initialized(&database_path)? {
            return Err(AgentToolsError::NodeNotInitialized);
        }
        let store = load_sqlite_store(&database_path)?;
        Ok(Self::new_sqlite_persistent(store, database_path))
    }

    pub fn store(&self) -> Arc<RwLock<InMemoryStore>> {
        self.store.clone()
    }

    pub fn persistence_path(&self) -> Option<&Path> {
        match &self.persistence {
            Some(Persistence::Json(path)) => Some(path.as_path()),
            Some(Persistence::Sqlite { path, .. }) => Some(path.as_path()),
            None => None,
        }
    }

    pub fn default_auth_context(&self) -> Result<AuthContext, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let node = store.default_node()?;
        Ok(AuthContext {
            user_id: None,
            tenant_id: node.tenant_id,
            node_id: node.id,
            harness_id: None,
        })
    }

    /// Returns the automatically authenticated local User context for the Home
    /// Node Owner Credential.
    pub fn local_owner_auth_context(&self) -> Result<AuthContext, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let node = store.default_node()?;
        let user_id = store
            .users
            .values()
            .min_by_key(|user| (user.created_at, user.id))
            .map(|user| user.id);
        Ok(AuthContext {
            user_id,
            tenant_id: node.tenant_id,
            node_id: node.id,
            harness_id: None,
        })
    }

    fn persist_locked(&self, store: &mut InMemoryStore) -> Result<(), AgentToolsError> {
        match &self.persistence {
            Some(Persistence::Json(path)) => save_store_snapshot(store, path)?,
            Some(Persistence::Sqlite { path, baseline }) => {
                let mut baseline = baseline.lock().map_err(|_| AgentToolsError::LockPoisoned)?;
                if let Err(error) = persist_sqlite_store_changes(path, &baseline, store) {
                    let authoritative =
                        load_sqlite_store(path).unwrap_or_else(|_| baseline.clone());
                    *baseline = authoritative.clone();
                    *store = authoritative;
                    return Err(error.into());
                }
                *baseline = store.clone();
            }
            None => {}
        }
        Ok(())
    }

    pub fn list_pods(&self, tenant_id: Option<TenantId>) -> Result<Vec<Pod>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        Ok(store
            .pods
            .values()
            .filter(|pod| pod.tenant_id == tenant_id || pod.tenant_id.is_none())
            .cloned()
            .collect())
    }

    /// Lists only Pods visible within the caller's optional Harness Grant scope.
    ///
    /// # Errors
    ///
    /// Returns an error if the Home Node store lock is poisoned.
    pub fn list_pods_for_harness(&self, ctx: &AuthContext) -> Result<Vec<Pod>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let scoped_pod_ids =
            harness_for_context(&store, ctx)?.and_then(|harness| harness.grant.pod_ids.as_ref());
        Ok(store
            .pods
            .values()
            .filter(|pod| pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
            .filter(|pod| scoped_pod_ids.is_none_or(|pod_ids| pod_ids.contains(&pod.id)))
            .cloned()
            .collect())
    }

    /// Returns Pod workflow actions allowed by relationship, Harness Grant, and Pod scope.
    pub fn pod_allowed_actions(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<PodAllowedAction>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let harness = harness_for_context(&store, ctx)?;
        let capability = |capability| {
            harness.is_none_or(|harness| {
                harness.grant.capabilities.contains(&capability)
                    && harness
                        .grant
                        .pod_ids
                        .as_ref()
                        .is_none_or(|pod_ids| pod_ids.contains(&pod_id))
            })
        };
        let subscribed = ctx.user_id.is_some_and(|user_id| {
            store.subscriptions.values().any(|subscription| {
                subscription.user_id == user_id && subscription.local_pod_id == pod_id
            })
        });
        let role = ctx.user_id.and_then(|user_id| {
            store
                .pod_roles
                .iter()
                .find(|assignment| assignment.user_id == user_id && assignment.pod_id == pod_id)
                .map(|assignment| assignment.role.clone())
        });
        let mut actions = Vec::new();
        if capability(HarnessCapability::SubscriptionManagement) {
            if subscribed {
                actions.extend([
                    PodAllowedAction::Unsubscribe,
                    PodAllowedAction::SubscriptionSet,
                ]);
            } else {
                actions.push(PodAllowedAction::Subscribe);
            }
        }
        if capability(HarnessCapability::PodCuration) && role.is_some() {
            actions.push(PodAllowedAction::RoleList);
            if role == Some(PodRole::Owner) {
                actions.extend([
                    PodAllowedAction::VisibilitySet,
                    PodAllowedAction::RoleGrant,
                    PodAllowedAction::RoleRevoke,
                ]);
            }
        }
        Ok(actions)
    }

    /// Pods that are safe to expose on the unauthenticated federation surface.
    /// Only `Public` pods are returned; private and invite-only pods are withheld.
    pub fn list_public_pods(&self, ctx: &AuthContext) -> Result<Vec<Pod>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        harness_for_context(&store, ctx)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        Ok(store
            .pods
            .values()
            .filter(|pod| {
                (pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
                    && pod.visibility == Visibility::Public
                    && pod
                        .origin_node_id
                        .is_none_or(|origin_node_id| origin_node_id == node.id)
            })
            .cloned()
            .collect())
    }

    /// Look up a pod by slug. Thin accessor over the store.
    pub fn pod_by_slug(
        &self,
        slug: &str,
        tenant_id: Option<TenantId>,
    ) -> Result<Pod, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        Ok(store.pod_by_slug(slug, tenant_id)?)
    }

    /// Pod manifest for the federation surface. A non-public pod is reported as
    /// `NotFound` — byte-identical to a missing pod — so private pods cannot be
    /// probed for existence through this endpoint.
    pub fn federation_pod_manifest(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodManifest, AgentToolsError> {
        let node_id = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            store.node_for_tenant(ctx.tenant_id)?.id
        };
        let manifest = self.pod_manifest(ctx, pod_slug)?;
        if manifest.pod.visibility != Visibility::Public {
            return Err(StoreError::NotFound(format!("pod {pod_slug}")).into());
        }
        if manifest
            .pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node_id)
        {
            return Err(StoreError::NotFound(format!("pod {pod_slug}")).into());
        }
        Ok(manifest)
    }

    /// Pod event log for the federation surface. A non-public pod is reported as
    /// `NotFound` so private pods never expose their events.
    pub fn federation_pod_events(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<Vec<EventLog>, AgentToolsError> {
        let node_id = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            store.node_for_tenant(ctx.tenant_id)?.id
        };
        let pod = self.pod_by_slug(pod_slug, ctx.tenant_id)?;
        if pod.visibility != Visibility::Public {
            return Err(StoreError::NotFound(format!("pod {pod_slug}")).into());
        }
        if pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node_id)
        {
            return Err(StoreError::NotFound(format!("pod {pod_slug}")).into());
        }
        self.export_pod_events(ctx, pod_slug)
    }

    /// Exports one public Pod's signed artifacts after an optional event cursor.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is not locally authoritative and public,
    /// the cursor is unknown, or the Home Node store lock is poisoned.
    pub fn federation_pod_snapshot(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        after_event_hash: Option<&str>,
    ) -> Result<FederationPodSnapshot, AgentToolsError> {
        let node = self.node_info(ctx)?;
        let manifest = self.federation_pod_manifest(ctx, pod_slug)?;
        let all_events = self.federation_pod_events(ctx, pod_slug)?;
        let events = match after_event_hash {
            Some(cursor) => {
                let index = all_events
                    .iter()
                    .position(|event| event.content_hash == cursor)
                    .ok_or_else(|| {
                        StoreError::Validation("synchronization cursor is unknown".to_string())
                    })?;
                all_events.into_iter().skip(index + 1).collect()
            }
            None => all_events,
        };
        Ok(FederationPodSnapshot {
            node,
            manifest,
            events,
        })
    }

    /// Subscribes to a directly addressed public Pod and projects verified artifacts.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied, the public URL is invalid,
    /// signed artifacts fail verification, or persistence fails.
    pub fn subscribe_public_pod(
        &self,
        ctx: &AuthContext,
        request: SubscribePublicPodRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<SynchronizationResult, AgentToolsError> {
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Subscription requires an authenticated User".to_string())
        })?;
        let public_pod_url =
            validate_public_pod_url(&request.public_pod_url, &request.snapshot.manifest.pod.slug)?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::SubscriptionManagement, None)?;
        if store.subscriptions.values().any(|subscription| {
            subscription.user_id == user_id
                && subscription.tenant_id == ctx.tenant_id
                && subscription.public_pod_url == public_pod_url
        }) {
            return Err(StoreError::Duplicate(format!("Subscription to {public_pod_url}")).into());
        }
        validate_federation_snapshot(&store, ctx.tenant_id, None, &request.snapshot)?;
        let mut projected = store.clone();
        let imported_events =
            project_snapshot_events(&mut projected, ctx, &request.snapshot.events)?;
        let local_pod = projected
            .pods
            .values()
            .find(|pod| {
                pod.tenant_id == ctx.tenant_id
                    && pod.slug == request.snapshot.manifest.pod.slug
                    && pod.origin_node_id == Some(request.snapshot.node.node_id)
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound("synchronized public Pod".to_string()))?;
        let subscription = Subscription {
            id: Uuid::now_v7().into(),
            user_id,
            tenant_id: ctx.tenant_id,
            public_pod_url,
            origin_node_id: request.snapshot.node.node_id,
            origin_public_key: request.snapshot.node.public_key,
            pod_slug: request.snapshot.manifest.pod.slug,
            local_pod_id: local_pod.id,
            is_priority: false,
            last_event_hash: request.snapshot.manifest.latest_known_event_hash,
            created_at: now,
            synchronized_at: now,
            last_sync_failure: None,
        };
        projected
            .subscriptions
            .insert(subscription.id, subscription.clone());
        record_harness_write_at(
            &mut projected,
            ctx,
            HarnessWriteOperation::SubscribePublicPod,
            Some(local_pod.id),
            now,
        );
        self.persist_locked(&mut projected)?;
        *store = projected;
        Ok(SynchronizationResult {
            subscription,
            imported_events,
        })
    }

    /// Applies the next contiguous signed event segment for a Subscription.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied, Origin identity changes,
    /// the event chain is discontinuous or invalid, or persistence fails.
    pub fn synchronize_subscription(
        &self,
        ctx: &AuthContext,
        subscription_id: SubscriptionId,
        mut snapshot: FederationPodSnapshot,
    ) -> Result<SynchronizationResult, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let existing = store
            .subscriptions
            .get(&subscription_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Subscription {subscription_id}")))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(existing.local_pod_id),
        )?;
        if ctx.user_id != Some(existing.user_id)
            || existing.tenant_id != ctx.tenant_id
            || existing.origin_node_id != snapshot.node.node_id
            || existing.origin_public_key != snapshot.node.public_key
            || existing.pod_slug != snapshot.manifest.pod.slug
        {
            return Err(StoreError::Validation(
                "synchronization artifacts do not match the Subscription".to_string(),
            )
            .into());
        }
        discard_replayed_events(&store, existing.last_event_hash.as_deref(), &mut snapshot)?;
        validate_federation_snapshot(
            &store,
            ctx.tenant_id,
            existing.last_event_hash.as_deref(),
            &snapshot,
        )?;
        let mut projected = store.clone();
        let imported_events = project_snapshot_events(&mut projected, ctx, &snapshot.events)?;
        let synchronized_at = Utc::now();
        let subscription = projected
            .subscriptions
            .get_mut(&subscription_id)
            .ok_or_else(|| StoreError::NotFound(format!("Subscription {subscription_id}")))?;
        subscription.last_event_hash = snapshot.manifest.latest_known_event_hash;
        subscription.synchronized_at = synchronized_at;
        subscription.last_sync_failure = None;
        let subscription = subscription.clone();
        record_harness_write_at(
            &mut projected,
            ctx,
            HarnessWriteOperation::SynchronizeSubscription,
            Some(subscription.local_pod_id),
            synchronized_at,
        );
        self.persist_locked(&mut projected)?;
        *store = projected;
        Ok(SynchronizationResult {
            subscription,
            imported_events,
        })
    }

    /// Reads one local Subscription within the authenticated User boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when the Subscription is missing, belongs to another
    /// User or tenant, or the store lock is poisoned.
    pub fn subscription(
        &self,
        ctx: &AuthContext,
        subscription_id: SubscriptionId,
    ) -> Result<Subscription, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let subscription = store
            .subscriptions
            .get(&subscription_id)
            .filter(|subscription| {
                Some(subscription.user_id) == ctx.user_id && subscription.tenant_id == ctx.tenant_id
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Subscription {subscription_id}")))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(subscription.local_pod_id),
        )?;
        Ok(subscription)
    }

    /// Resolves the authenticated User's Subscription for one local Pod projection.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is not subscribed by this User or authorization is denied.
    pub fn subscription_for_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Subscription, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let subscription = store
            .subscriptions
            .values()
            .find(|subscription| {
                subscription.local_pod_id == pod_id
                    && Some(subscription.user_id) == ctx.user_id
                    && subscription.tenant_id == ctx.tenant_id
            })
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Subscription for Pod {pod_id}")))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(pod_id),
        )?;
        Ok(subscription)
    }

    /// Records an operator-visible failure without changing synchronized Pod state.
    ///
    /// # Errors
    ///
    /// Returns an error when the Subscription is inaccessible or persistence fails.
    pub fn record_subscription_sync_failure(
        &self,
        ctx: &AuthContext,
        subscription_id: SubscriptionId,
        code: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
        now: chrono::DateTime<Utc>,
    ) -> Result<Subscription, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let existing = store
            .subscriptions
            .get(&subscription_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Subscription {subscription_id}")))?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(existing.local_pod_id),
        )?;
        if ctx.user_id != Some(existing.user_id) || ctx.tenant_id != existing.tenant_id {
            return Err(StoreError::NotFound(format!("Subscription {subscription_id}")).into());
        }
        let subscription = store
            .subscriptions
            .get_mut(&subscription_id)
            .expect("checked above");
        subscription.last_sync_failure = Some(SynchronizationFailure {
            code: code.into(),
            message: message.into(),
            retryable,
            occurred_at: now,
        });
        let subscription = subscription.clone();
        self.persist_locked(&mut store)?;
        Ok(subscription)
    }

    pub fn route_link_to_pods(
        &self,
        ctx: &AuthContext,
        request: RouteLinkRequest,
        confidence_threshold: f32,
    ) -> Result<RouteLinkResponse, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let text = route_text(&request);
        let harness = harness_for_context(&store, ctx)?;
        let mut candidates = store
            .pods
            .values()
            .filter(|pod| pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
            .filter(|pod| {
                harness
                    .and_then(|harness| harness.grant.pod_ids.as_ref())
                    .is_none_or(|pod_ids| pod_ids.contains(&pod.id))
            })
            .map(|pod| {
                score_pod_route(
                    pod,
                    store.pod_skill_packs.get(&pod.id),
                    &text,
                    &request.tags,
                )
            })
            .collect::<Vec<_>>();
        let existing_slugs = candidates
            .iter()
            .map(|candidate| candidate.pod_slug.clone())
            .collect::<HashSet<_>>();
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        let selected = candidates.first().cloned().and_then(|top| {
            let second_score = candidates
                .get(1)
                .map(|candidate| candidate.score)
                .unwrap_or(0.0);
            if top.score >= confidence_threshold && top.score - second_score >= 0.75 {
                Some(top)
            } else {
                None
            }
        });
        let needs_confirmation = selected.is_none();
        let suggested_new_pod = if needs_confirmation {
            Some(suggest_new_pod_for_link(
                &request,
                &candidates,
                &existing_slugs,
            ))
        } else {
            None
        };
        Ok(RouteLinkResponse {
            needs_confirmation,
            selected,
            candidates,
            confidence_threshold,
            suggested_new_pod,
        })
    }

    pub fn create_tenant(&self, request: CreateTenantRequest) -> Result<Tenant, AgentToolsError> {
        self.create_tenant_inner(None, request)
    }

    /// Creates a tenant on behalf of an authorized administrative harness.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied, the slug is duplicated,
    /// the store lock is poisoned, signing fails, or persistence fails.
    pub fn create_tenant_as(
        &self,
        ctx: &AuthContext,
        request: CreateTenantRequest,
    ) -> Result<Tenant, AgentToolsError> {
        self.create_tenant_inner(Some(ctx), request)
    }

    fn create_tenant_inner(
        &self,
        ctx: Option<&AuthContext>,
        request: CreateTenantRequest,
    ) -> Result<Tenant, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if let Some(ctx) = ctx {
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        }
        if store
            .tenants
            .values()
            .any(|tenant| tenant.slug == request.slug)
        {
            return Err(StoreError::Duplicate(format!("tenant {}", request.slug)).into());
        }
        let tenant = Tenant {
            id: Uuid::now_v7(),
            name: request.name,
            slug: request.slug,
            created_at: Utc::now(),
        };
        store.tenants.insert(tenant.id, tenant.clone());
        let node = crate::signing::create_node_identity(
            format!("{} managed node", tenant.name),
            Some(tenant.id),
        );
        store.node_identities.insert(node.id, node);
        if let Some(ctx) = ctx {
            record_harness_write(&mut store, ctx, HarnessWriteOperation::CreateTenant, None);
        }
        self.persist_locked(&mut store)?;
        Ok(tenant)
    }

    pub fn create_dev_token(
        &self,
        request: DevTokenRequest,
    ) -> Result<DevTokenResponse, AgentToolsError> {
        self.create_dev_token_inner(None, request)
    }

    /// Creates a legacy development token under administration authority.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied, the tenant is missing,
    /// the store lock is poisoned, or persistence fails.
    pub fn create_dev_token_as(
        &self,
        ctx: &AuthContext,
        request: DevTokenRequest,
    ) -> Result<DevTokenResponse, AgentToolsError> {
        self.create_dev_token_inner(Some(ctx), request)
    }

    fn create_dev_token_inner(
        &self,
        ctx: Option<&AuthContext>,
        request: DevTokenRequest,
    ) -> Result<DevTokenResponse, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if let Some(ctx) = ctx {
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        }
        let tenant_id = request
            .tenant_slug
            .as_deref()
            .map(|slug| store.tenant_by_slug(slug).map(|t| t.id))
            .transpose()?;
        let user_id = request.user_id.unwrap_or_else(Uuid::now_v7);
        store.users.entry(user_id).or_insert(User {
            id: user_id,
            display_name: "Remote Agent User".to_string(),
            created_at: Utc::now(),
        });
        if let Some(tenant_id) = tenant_id {
            let exists = store
                .tenant_users
                .iter()
                .any(|tu| tu.tenant_id == tenant_id && tu.user_id == user_id);
            if !exists {
                store.tenant_users.push(TenantUser {
                    tenant_id,
                    user_id,
                    role: TenantRole::Member,
                    created_at: Utc::now(),
                });
            }
        }
        let token = new_plaintext_api_token();
        let token_hash = hash_api_token(&token);
        let harness = AgentHarness {
            id: AgentHarnessId::from(Uuid::now_v7()),
            user_id,
            tenant_id,
            label: request.label.clone(),
            kind: AgentHarnessKind::Interactive,
            grant: HarnessGrant {
                capabilities: vec![],
                pod_ids: None,
            },
            created_at: Utc::now(),
            revoked_at: None,
        };
        let api_token = ApiToken {
            id: Uuid::now_v7(),
            user_id,
            tenant_id,
            token_hash: token_hash.clone(),
            label: request.label,
            created_at: Utc::now(),
            last_used_at: None,
            revoked_at: None,
            harness_id: Some(harness.id),
        };
        store.agent_harnesses.insert(harness.id, harness);
        store.api_tokens.insert(api_token.id, api_token);
        if let Some(ctx) = ctx {
            record_harness_write(&mut store, ctx, HarnessWriteOperation::CreateDevToken, None);
        }
        self.persist_locked(&mut store)?;
        Ok(DevTokenResponse {
            token,
            token_hash,
            user_id,
            tenant_id,
        })
    }

    pub fn authenticate_token(&self, token: &str) -> Result<Option<AuthContext>, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let hash = hash_api_token(token);
        let Some((token_id, user_id, tenant_id, harness_id)) = store
            .api_tokens
            .iter()
            .find(|(_, token)| token.token_hash == hash && token.revoked_at.is_none())
            .filter(|(_, token)| {
                token.harness_id.is_none_or(|harness_id| {
                    store
                        .agent_harnesses
                        .get(&harness_id)
                        .is_some_and(|harness| harness.revoked_at.is_none())
                })
            })
            .map(|(id, token)| (*id, token.user_id, token.tenant_id, token.harness_id))
        else {
            return Ok(None);
        };
        let node = store.node_for_tenant(tenant_id)?;
        let api_token = store
            .api_tokens
            .get_mut(&token_id)
            .ok_or_else(|| StoreError::NotFound("api token".to_string()))?;
        api_token.last_used_at = Some(Utc::now());
        self.persist_locked(&mut store)?;
        Ok(Some(AuthContext {
            user_id: Some(user_id),
            tenant_id,
            node_id: node.id,
            harness_id,
        }))
    }

    /// Registers a harness and returns its bearer token exactly once.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks administration authority, a Pod
    /// scope is invalid, no User exists, or persistence fails.
    pub fn register_agent_harness(
        &self,
        ctx: &AuthContext,
        request: RegisterAgentHarnessRequest,
    ) -> Result<RegisterAgentHarnessResponse, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if ctx.harness_id.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "only the Home Node Owner may register an Agent Harness directly"
                    .to_string(),
            });
        }
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        if request.label.trim().is_empty() {
            return Err(StoreError::Validation(
                "Agent Harness label must not be empty".to_string(),
            )
            .into());
        }
        let user_id = ctx
            .user_id
            .or_else(|| store.users.keys().next().copied())
            .ok_or_else(|| {
                StoreError::Validation("an Agent Harness must belong to a User".to_string())
            })?;
        for pod_id in request.pod_ids.iter().flatten() {
            let pod = store
                .pods
                .get(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        }
        let capabilities = normalize_capabilities(request.capabilities);
        if request.kind == AgentHarnessKind::Unattended
            && capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    HarnessCapability::Administration | HarnessCapability::Approval
                )
            })
        {
            return Err(AgentToolsError::Forbidden {
                reason: "unattended harnesses cannot receive administration or approval"
                    .to_string(),
            });
        }
        if ctx.harness_id.is_none()
            && capabilities.len() > 1
            && capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    HarnessCapability::Administration | HarnessCapability::Approval
                )
            })
        {
            return Err(StoreError::Validation(
                "bootstrap administration and approval grants must be isolated".to_string(),
            )
            .into());
        }
        if let Some(caller) = harness_for_context(&store, ctx)? {
            if request.kind == AgentHarnessKind::Interactive
                || capabilities.contains(&HarnessCapability::Administration)
            {
                return Err(AgentToolsError::Forbidden {
                    reason: "a harness cannot delegate interactive or administration authority"
                        .to_string(),
                });
            }
            if capabilities
                .iter()
                .any(|capability| !caller.grant.capabilities.contains(capability))
            {
                return Err(AgentToolsError::Forbidden {
                    reason: "a harness cannot delegate capabilities it does not hold".to_string(),
                });
            }
            ensure_child_pod_scope(&caller.grant.pod_ids, &request.pod_ids)?;
        }
        let harness = AgentHarness {
            id: AgentHarnessId::from(Uuid::now_v7()),
            user_id,
            tenant_id: ctx.tenant_id,
            label: request.label,
            kind: request.kind,
            grant: HarnessGrant {
                capabilities,
                pod_ids: request.pod_ids.map(normalize_pod_ids),
            },
            created_at: Utc::now(),
            revoked_at: None,
        };
        let token = new_plaintext_api_token();
        let api_token = ApiToken {
            id: Uuid::now_v7(),
            user_id,
            tenant_id: ctx.tenant_id,
            token_hash: hash_api_token(&token),
            label: format!("Harness: {}", harness.label),
            created_at: Utc::now(),
            last_used_at: None,
            revoked_at: None,
            harness_id: Some(harness.id),
        };
        store.agent_harnesses.insert(harness.id, harness.clone());
        store.api_tokens.insert(api_token.id, api_token);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RegisterAgentHarness,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(RegisterAgentHarnessResponse {
            harness,
            token: HarnessToken::new(token),
        })
    }

    /// Revokes a harness and all of its bearer tokens immediately.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks administration authority, the
    /// harness does not exist, or persistence fails.
    pub fn revoke_agent_harness(
        &self,
        ctx: &AuthContext,
        harness_id: AgentHarnessId,
    ) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if ctx.harness_id.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "only the Home Node Owner may revoke an Agent Harness directly".to_string(),
            });
        }
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let now = Utc::now();
        let harness = store
            .agent_harnesses
            .get_mut(&harness_id)
            .ok_or_else(|| StoreError::NotFound(format!("agent harness {harness_id}")))?;
        harness.revoked_at = Some(now);
        for token in store
            .api_tokens
            .values_mut()
            .filter(|token| token.harness_id == Some(harness_id))
        {
            token.revoked_at = Some(now);
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RevokeAgentHarness,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(())
    }

    /// Lists Agent Harness metadata without exposing bearer credentials or hashes.
    pub fn list_agent_harnesses(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<AgentHarnessView>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let mut harnesses = store
            .agent_harnesses
            .values()
            .filter(|harness| harness.tenant_id == ctx.tenant_id)
            .map(|harness| agent_harness_view(&store, harness))
            .collect::<Result<Vec<_>, _>>()?;
        harnesses.sort_by_key(|view| view.harness.created_at);
        Ok(harnesses)
    }

    /// Returns one Agent Harness's safe metadata view.
    pub fn agent_harness(
        &self,
        ctx: &AuthContext,
        harness_id: AgentHarnessId,
    ) -> Result<AgentHarnessView, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let harness = store
            .agent_harnesses
            .get(&harness_id)
            .ok_or_else(|| StoreError::NotFound(format!("Agent Harness {harness_id}")))?;
        store.assert_tenant(harness.tenant_id, ctx.tenant_id)?;
        agent_harness_view(&store, harness)
    }

    /// Requests an authority expansion through the Pending Proposal policy.
    pub fn request_harness_grant_expansion(
        &self,
        ctx: &AuthContext,
        harness_id: AgentHarnessId,
        capabilities: Vec<HarnessCapability>,
        pod_ids: Option<Vec<PodId>>,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal(
            ctx,
            SensitiveChange::ExpandHarnessGrant {
                harness_id,
                capabilities,
                pod_ids,
            },
            now,
            now + Duration::hours(24),
        )
    }

    /// Returns local-only harness write attribution records.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks administration authority or the
    /// Home Node store lock is poisoned.
    pub fn list_harness_write_audit(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<HarnessWriteAudit>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(store.harness_write_audit.clone())
    }

    /// Verifies a non-Pod-specific capability for the current harness context.
    /// Local non-harness contexts remain unrestricted for compatibility.
    ///
    /// # Errors
    ///
    /// Returns an error when the harness is revoked, mismatched, or lacks the
    /// requested capability, or when the store lock is poisoned.
    pub fn require_harness_capability(
        &self,
        ctx: &AuthContext,
        capability: HarnessCapability,
    ) -> Result<(), AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, capability, None)
    }

    /// Verifies interactive Feedback authority for adapter capability projection.
    ///
    /// # Errors
    ///
    /// Returns an error when the harness is unattended, revoked, lacks Feedback
    /// authority, or the lock is poisoned.
    pub fn require_interactive_feedback(&self, ctx: &AuthContext) -> Result<(), AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Feedback, None)?;
        authorize_interactive_user_action(
            &store,
            ctx,
            "Feedback requires an interactive User action",
        )
    }

    /// Verifies unscoped interactive authority for private Taste Profile adapters.
    ///
    /// # Errors
    ///
    /// Returns an error when the harness is unattended, revoked, Pod-scoped,
    /// lacks Feedback authority, or the lock is poisoned.
    pub fn require_unscoped_interactive_feedback(
        &self,
        ctx: &AuthContext,
    ) -> Result<(), AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)
    }

    /// Verifies the complete interactive, unscoped Personal Discovery management policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the Harness kind, scope, capability, or identity is invalid.
    pub fn require_personal_discovery_management(
        &self,
        ctx: &AuthContext,
    ) -> Result<(), AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)
    }

    /// Verifies the complete unattended, unscoped Personal Discovery execution policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the Harness kind, scope, capability, or identity is invalid.
    pub fn require_personal_discovery_execution(
        &self,
        ctx: &AuthContext,
    ) -> Result<(), AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_execution(&store, ctx)
    }

    /// Verifies whether this context can participate in authorized plan reads.
    ///
    /// # Errors
    ///
    /// Returns an error unless the complete management or execution policy applies.
    pub fn require_personal_discovery_plan_access(
        &self,
        ctx: &AuthContext,
    ) -> Result<(), AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)
            .or_else(|_| authorize_personal_discovery_execution(&store, ctx))
    }

    /// Creates an expiring proposal without applying its sensitive change.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller is not an authenticated harness, lacks
    /// authority for the affected resource, supplies an invalid expiry, or
    /// persistence fails.
    pub fn create_pending_proposal(
        &self,
        ctx: &AuthContext,
        requested_change: SensitiveChange,
        now: chrono::DateTime<Utc>,
        expires_at: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let proposer = ctx.harness_id.ok_or_else(|| AgentToolsError::Forbidden {
            reason: "Pending Proposals require an authenticated Agent Harness".to_string(),
        })?;
        let proposer_harness =
            harness_for_context(&store, ctx)?.ok_or_else(|| AgentToolsError::Forbidden {
                reason: "Pending Proposals require an authenticated Agent Harness".to_string(),
            })?;
        let proposer_user_id = proposer_harness.user_id;
        let proposer_tenant_id = proposer_harness.tenant_id;
        if expires_at <= now || expires_at > now + Duration::days(7) {
            return Err(StoreError::Validation(
                "Pending Proposal expiry must be within seven days".to_string(),
            )
            .into());
        }
        let (affected_resources, expected_consequences, structured_diff) = match &requested_change {
            SensitiveChange::CreatePublicPod { request } => {
                authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PodCuration)?;
                if request.visibility != Visibility::Public {
                    return Err(StoreError::Validation(
                        "CreatePublicPod requires public visibility".to_string(),
                    )
                    .into());
                }
                if store
                    .pods
                    .values()
                    .any(|pod| pod.slug == request.slug && pod.tenant_id == ctx.tenant_id)
                {
                    return Err(StoreError::Duplicate(format!("pod {}", request.slug)).into());
                }
                let resource = ProposalResource::PodSlug(request.slug.clone());
                (
                    vec![resource.clone()],
                    vec!["A new Pod and its signed Package become immediately available through federation and Explore surfaces.".to_string()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: serde_json::Value::Null,
                        after: json!(request),
                    }],
                )
            }
            SensitiveChange::CreatePublicPodLifecycle { request } => {
                authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PodCuration)?;
                if !matches!(request.package, PodCreationPackage::Default) {
                    authorize_harness_for_new_pod(
                        &store,
                        ctx,
                        HarnessCapability::PackageManagement,
                    )?;
                }
                if request.pod.visibility != Visibility::Public {
                    return Err(StoreError::Validation(
                        "public Pod lifecycle creation requires public visibility".into(),
                    )
                    .into());
                }
                if store
                    .pods
                    .values()
                    .any(|pod| pod.slug == request.pod.slug && pod.tenant_id == ctx.tenant_id)
                {
                    return Err(StoreError::Duplicate(format!("pod {}", request.pod.slug)).into());
                }
                validate_creation_package_locked(&store, ctx, &request.package)?;
                let resource = ProposalResource::PodSlug(request.pod.slug.clone());
                (
                    vec![resource.clone()],
                    vec!["A new Pod and its selected signed Package become available atomically through federation and Explore surfaces.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: serde_json::Value::Null,
                        after: json!(request),
                    }],
                )
            }
            SensitiveChange::PublishPod { pod_id } => {
                authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(*pod_id))?;
                let pod = store
                    .pods
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                if pod.visibility == Visibility::Public {
                    return Err(StoreError::Validation("Pod is already public".to_string()).into());
                }
                let resource = ProposalResource::Pod(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The Pod and its signed public events become available through federation and Explore surfaces.".to_string()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!({"visibility": pod.visibility}),
                        after: json!({"visibility": Visibility::Public}),
                    }],
                )
            }
            SensitiveChange::ExpandPodVisibility { pod_id, visibility } => {
                authorize_pod_role_owner(&store, ctx, *pod_id)?;
                let pod = store
                    .pods
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                if visibility_exposure(visibility) <= visibility_exposure(&pod.visibility) {
                    return Err(StoreError::Validation(
                        "Pending Proposals only apply to visibility expansion".into(),
                    )
                    .into());
                }
                let resource = ProposalResource::Pod(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The Pod becomes visible to a broader audience.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!({"visibility": pod.visibility}),
                        after: json!({"visibility": visibility}),
                    }],
                )
            }
            SensitiveChange::ExpandHarnessGrant {
                harness_id,
                capabilities,
                pod_ids,
            } => {
                authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
                let target = store
                    .agent_harnesses
                    .get(harness_id)
                    .ok_or_else(|| StoreError::NotFound(format!("Agent Harness {harness_id}")))?;
                store.assert_tenant(target.tenant_id, ctx.tenant_id)?;
                for pod_id in pod_ids.iter().flatten() {
                    let pod = store
                        .pods
                        .get(pod_id)
                        .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                    store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                }
                let normalized_capabilities = normalize_capabilities(capabilities.clone());
                if target
                    .grant
                    .capabilities
                    .iter()
                    .any(|capability| !normalized_capabilities.contains(capability))
                    || !grant_scope_expands(&target.grant.pod_ids, pod_ids)
                {
                    return Err(StoreError::Validation(
                        "sensitive grant change must only expand authority".to_string(),
                    )
                    .into());
                }
                if target.kind == AgentHarnessKind::Unattended
                    && normalized_capabilities.iter().any(|capability| {
                        matches!(
                            capability,
                            HarnessCapability::Administration | HarnessCapability::Approval
                        )
                    })
                {
                    return Err(AgentToolsError::Forbidden {
                        reason: "unattended harnesses cannot receive administration or approval"
                            .to_string(),
                    });
                }
                let resource = ProposalResource::AgentHarness(*harness_id);
                (
                    vec![resource.clone()],
                    vec![
                        "The Harness Grant gains additional authority for future requests."
                            .to_string(),
                    ],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!(target.grant),
                        after: json!(HarnessGrant {
                            capabilities: normalized_capabilities,
                            pod_ids: pod_ids.clone().map(normalize_pod_ids),
                        }),
                    }],
                )
            }
            SensitiveChange::AddTrustedPeer {
                node_id,
                display_name,
                base_url,
                public_key,
            } => {
                authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
                if display_name.trim().is_empty()
                    || public_key.trim().is_empty()
                    || Url::parse(base_url).is_err()
                {
                    return Err(StoreError::Validation(
                        "trusted peer name, URL, and public key must be valid".to_string(),
                    )
                    .into());
                }
                if store.trusted_peers.values().any(|peer| {
                    peer.tenant_id == ctx.tenant_id
                        && (peer.base_url == *base_url
                            || (!node_id.is_nil() && peer.node_id == *node_id))
                }) {
                    return Err(StoreError::Duplicate(format!("trusted peer {base_url}")).into());
                }
                let resource = ProposalResource::TrustedPeerUrl(base_url.clone());
                (
                    vec![resource.clone()],
                    vec!["The Home Node will trust signed public data from this peer.".to_string()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: serde_json::Value::Null,
                        after: json!({
                            "node_id": node_id,
                            "display_name": display_name,
                            "base_url": base_url,
                            "public_key": public_key,
                            "trust_level": TrustLevel::ReadOnly,
                            "enabled": true,
                        }),
                    }],
                )
            }
            SensitiveChange::RemoveTrustedPeer { peer_id } => {
                authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
                let peer = store
                    .trusted_peers
                    .get(peer_id)
                    .ok_or_else(|| StoreError::NotFound(format!("trusted peer {peer_id}")))?;
                store.assert_tenant(peer.tenant_id, ctx.tenant_id)?;
                if !peer.enabled {
                    return Err(
                        StoreError::Validation("trusted peer is already disabled".into()).into(),
                    );
                }
                let resource = ProposalResource::TrustedPeerUrl(peer.base_url.clone());
                let mut disabled = peer.clone();
                disabled.enabled = false;
                (
                    vec![resource.clone()],
                    vec!["The peer can no longer exchange signed public discovery data with this Home Node.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!(peer),
                        after: json!(disabled),
                    }],
                )
            }
            SensitiveChange::ChangeTrustPolicy { change } => {
                authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
                let current = store
                    .trust_policies
                    .get(&(proposer_user_id, proposer_tenant_id))
                    .cloned()
                    .unwrap_or_else(|| TrustPolicy::new(proposer_user_id, proposer_tenant_id));
                let mut prospective = current.clone();
                apply_trust_policy_change(&mut prospective, change)?;
                let resource = ProposalResource::TrustPolicy(proposer_user_id);
                (
                    vec![resource.clone()],
                    vec!["The Home Node's local public Pod discovery rules change.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!(current),
                        after: json!(prospective),
                    }],
                )
            }
            SensitiveChange::RevisePublicPodPackage {
                pod_id,
                base_version,
                patch,
            } => {
                authorize_harness(
                    &store,
                    ctx,
                    HarnessCapability::PackageManagement,
                    Some(*pod_id),
                )?;
                let pod = store
                    .pods
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                if pod.visibility != Visibility::Public {
                    return Err(StoreError::Validation(
                        "this proposal type requires a public Pod".to_string(),
                    )
                    .into());
                }
                ensure_direct_package_revision_allowed_for_origin(&store, ctx, pod)?;
                let existing = store
                    .pod_skill_packs
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
                if PackageVersion::new(existing.version)
                    .map_err(|error| StoreError::Validation(error.to_string()))?
                    != *base_version
                {
                    return Err(StoreError::Validation(
                        "public Package Revision base version is stale".to_string(),
                    )
                    .into());
                }
                let prospective = patch_skill_pack(existing, patch.clone());
                let validation = validate_skill_pack(&prospective);
                if !validation.valid {
                    return Err(StoreError::Validation(validation.errors.join(", ")).into());
                }
                let pod_resource = ProposalResource::Pod(*pod_id);
                let package_resource = ProposalResource::PodPackage(*pod_id);
                (
                    vec![pod_resource, package_resource.clone()],
                    vec![
                        "The signed public Pod Package changes for current and future subscribers."
                            .to_string(),
                    ],
                    vec![ProposalResourceDiff {
                        resource: package_resource,
                        before: json!(existing),
                        after: json!(prospective),
                    }],
                )
            }
            SensitiveChange::RemovePublicSubmissionFromPod {
                pod_id,
                submission_id,
            } => {
                authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(*pod_id))?;
                let pod = store
                    .pods
                    .get(pod_id)
                    .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
                store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
                if pod.visibility != Visibility::Public {
                    return Err(StoreError::Validation(
                        "this proposal type requires a public Pod".to_string(),
                    )
                    .into());
                }
                if !store.submission_pods.iter().any(|placement| {
                    placement.pod_id == *pod_id && placement.submission_id == *submission_id
                }) {
                    return Err(StoreError::NotFound(format!(
                        "submission {submission_id} in pod {}",
                        pod.slug
                    ))
                    .into());
                }
                let resource = ProposalResource::SubmissionPlacement {
                    pod_id: *pod_id,
                    submission_id: *submission_id,
                };
                (
                    vec![resource.clone()],
                    vec!["The public Pod Placement is withdrawn from future federation and discovery.".to_string()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!({"accepted": true}),
                        after: json!({"accepted": false}),
                    }],
                )
            }
            SensitiveChange::EnableAutonomousCuration {
                pod_id,
                confidence_threshold,
            } => {
                authorize_local_pod_curation(&store, ctx, *pod_id)?;
                let current = store
                    .pod_curation_policies
                    .get(pod_id)
                    .copied()
                    .unwrap_or_default();
                if matches!(current, CurationPolicy::Autonomous { .. }) {
                    return Err(StoreError::Validation(
                        "Pod already uses Autonomous Curation".into(),
                    )
                    .into());
                }
                let resource = ProposalResource::PodCurationPolicy(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The Pod may accept qualifying Candidate Placements without manual or trusted-source review.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before: json!({"curation_policy": current}),
                        after: json!({
                            "curation_policy": CurationPolicy::Autonomous {
                                confidence_threshold: *confidence_threshold,
                            }
                        }),
                    }],
                )
            }
            SensitiveChange::GrantPodRole {
                pod_id,
                user_id,
                role,
            } => {
                authorize_pod_role_owner(&store, ctx, *pod_id)?;
                if !store.users.contains_key(user_id) {
                    return Err(StoreError::NotFound(format!("User {user_id}")).into());
                }
                if store.pod_roles.iter().any(|assignment| {
                    assignment.pod_id == *pod_id
                        && assignment.user_id == *user_id
                        && assignment.role == *role
                }) {
                    return Err(
                        StoreError::Duplicate(format!("Pod Role for User {user_id}")).into(),
                    );
                }
                if *role != PodRole::Owner
                    && store.pod_roles.iter().any(|assignment| {
                        assignment.pod_id == *pod_id
                            && assignment.user_id == *user_id
                            && assignment.role == PodRole::Owner
                    })
                    && store
                        .pod_roles
                        .iter()
                        .filter(|assignment| {
                            assignment.pod_id == *pod_id && assignment.role == PodRole::Owner
                        })
                        .count()
                        == 1
                {
                    return Err(
                        StoreError::Validation("cannot replace the last Pod Owner".into()).into(),
                    );
                }
                let before = pod_roles_value(&store, *pod_id);
                let mut prospective = store.clone();
                prospective.pod_roles.retain(|assignment| {
                    assignment.pod_id != *pod_id || assignment.user_id != *user_id
                });
                prospective.pod_roles.push(PodRoleAssignment {
                    user_id: *user_id,
                    pod_id: *pod_id,
                    role: role.clone(),
                    created_at: now,
                });
                let resource = ProposalResource::PodRoles(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The User gains explicit authority over this Pod.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before,
                        after: pod_roles_value(&prospective, *pod_id),
                    }],
                )
            }
            SensitiveChange::RevokePodRole {
                pod_id,
                user_id,
                role,
            } => {
                authorize_pod_role_owner(&store, ctx, *pod_id)?;
                let assignment = store
                    .pod_roles
                    .iter()
                    .find(|assignment| {
                        assignment.pod_id == *pod_id
                            && assignment.user_id == *user_id
                            && assignment.role == *role
                    })
                    .cloned()
                    .ok_or_else(|| StoreError::NotFound(format!("Pod Role for User {user_id}")))?;
                if assignment.role == PodRole::Owner
                    && store
                        .pod_roles
                        .iter()
                        .filter(|candidate| {
                            candidate.pod_id == *pod_id && candidate.role == PodRole::Owner
                        })
                        .count()
                        == 1
                {
                    return Err(
                        StoreError::Validation("cannot revoke the last Pod Owner".into()).into(),
                    );
                }
                let before = pod_roles_value(&store, *pod_id);
                let mut prospective = store.clone();
                prospective
                    .pod_roles
                    .retain(|candidate| candidate != &assignment);
                let resource = ProposalResource::PodRoles(*pod_id);
                (
                    vec![resource.clone()],
                    vec!["The User loses explicit authority over this Pod.".into()],
                    vec![ProposalResourceDiff {
                        resource,
                        before,
                        after: pod_roles_value(&prospective, *pod_id),
                    }],
                )
            }
        };
        let proposal = PendingProposal {
            id: PendingProposalId::from(Uuid::now_v7()),
            requested_change,
            affected_resources,
            expected_consequences,
            structured_diff,
            proposer,
            user_id: proposer_user_id,
            tenant_id: proposer_tenant_id,
            created_at: now,
            expires_at,
            status: ProposalStatus::Pending,
            decided_by: None,
            decided_at: None,
            rejection_reason: None,
        };
        store
            .pending_proposals
            .insert(proposal.id, proposal.clone());
        self.persist_locked(&mut store)?;
        Ok(proposal)
    }

    /// Creates a proposal from the transport-neutral relative-expiry request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::create_pending_proposal`] and rejects
    /// durations that cannot be represented safely.
    pub fn create_pending_proposal_from_request(
        &self,
        ctx: &AuthContext,
        request: CreatePendingProposalRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        let seconds = i64::try_from(request.expires_in_seconds).map_err(|_| {
            StoreError::Validation("Pending Proposal expiry is too large".to_string())
        })?;
        self.create_pending_proposal(
            ctx,
            request.requested_change,
            now,
            now + Duration::seconds(seconds),
        )
    }

    /// Returns one proposal and records expiry when it is first observed late.
    ///
    /// # Errors
    ///
    /// Returns an error when the proposal is missing, the caller is neither a
    /// local owner nor an authorized participant, or persistence fails.
    pub fn pending_proposal(
        &self,
        ctx: &AuthContext,
        proposal_id: PendingProposalId,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_proposal_reader(&store, ctx, proposal_id)?;
        expire_proposal(&mut store, proposal_id, now)?;
        let proposal = store
            .pending_proposals
            .get(&proposal_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        self.persist_locked(&mut store)?;
        Ok(proposal)
    }

    /// Lists Pending Proposals visible to the acting User or Harness Grant.
    pub fn list_pending_proposals(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<PendingProposal>, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        // Validate a supplied Harness identity even when there are no visible proposals.
        let _ = harness_for_context(&store, ctx)?;
        let proposal_ids = store.pending_proposals.keys().copied().collect::<Vec<_>>();
        for proposal_id in proposal_ids {
            expire_proposal(&mut store, proposal_id, now)?;
        }
        let mut proposals = store
            .pending_proposals
            .values()
            .filter(|proposal| authorize_proposal_reader(&store, ctx, proposal.id).is_ok())
            .cloned()
            .collect::<Vec<_>>();
        proposals.sort_by_key(|proposal| proposal.created_at);
        self.persist_locked(&mut store)?;
        Ok(proposals)
    }

    /// Returns the proposal decisions currently allowed for this actor.
    pub fn pending_proposal_allowed_actions(
        &self,
        ctx: &AuthContext,
        proposal_id: PendingProposalId,
    ) -> Result<Vec<ProposalAllowedAction>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let proposal = store
            .pending_proposals
            .get(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        authorize_proposal_reader(&store, ctx, proposal_id)?;
        if proposal.status == ProposalStatus::Pending
            && authorize_independent_approver(&store, ctx, proposal_id).is_ok()
        {
            Ok(vec![
                ProposalAllowedAction::Approve,
                ProposalAllowedAction::Reject,
            ])
        } else {
            Ok(Vec::new())
        }
    }

    /// Independently approves and atomically applies a live proposal.
    ///
    /// # Errors
    ///
    /// Returns an error when approval authority or independence is missing,
    /// the proposal is expired or terminal, the change is no longer valid, or
    /// persistence fails.
    pub fn approve_pending_proposal(
        &self,
        ctx: &AuthContext,
        proposal_id: PendingProposalId,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let approver = authorize_independent_approver(&store, ctx, proposal_id)?;
        expire_proposal(&mut store, proposal_id, now)?;
        let proposal_status = store
            .pending_proposals
            .get(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?
            .status;
        if proposal_status != ProposalStatus::Pending {
            self.persist_locked(&mut store)?;
            return Err(StoreError::Validation("Pending Proposal is terminal".to_string()).into());
        }
        let proposal_snapshot = store
            .pending_proposals
            .get(&proposal_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        validate_structured_diff(&store, &proposal_snapshot)?;
        let requested_change = proposal_snapshot.requested_change;
        let proposer = proposal_snapshot.proposer;
        let before_approval = store.clone();
        if let Err(error) = apply_sensitive_change(&mut store, ctx, proposer, &requested_change) {
            *store = before_approval;
            return Err(error);
        }
        let proposal = store
            .pending_proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        proposal.status = ProposalStatus::Accepted;
        proposal.decided_by = Some(approver);
        proposal.decided_at = Some(now);
        let proposal = proposal.clone();
        if let Err(error) = self.persist_locked(&mut store) {
            if matches!(self.persistence, Some(Persistence::Json(_))) {
                *store = before_approval;
            }
            return Err(error);
        }
        Ok(proposal)
    }

    /// Independently rejects a live proposal without applying its change.
    ///
    /// # Errors
    ///
    /// Returns an error when approval authority or independence is missing,
    /// the reason is empty, the proposal is expired or terminal, or
    /// persistence fails.
    pub fn reject_pending_proposal(
        &self,
        ctx: &AuthContext,
        proposal_id: PendingProposalId,
        now: chrono::DateTime<Utc>,
        reason: String,
    ) -> Result<PendingProposal, AgentToolsError> {
        if reason.trim().is_empty() {
            return Err(
                StoreError::Validation("rejection reason must not be empty".to_string()).into(),
            );
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let approver = authorize_independent_approver(&store, ctx, proposal_id)?;
        expire_proposal(&mut store, proposal_id, now)?;
        let proposal_status = store
            .pending_proposals
            .get(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?
            .status;
        if proposal_status != ProposalStatus::Pending {
            self.persist_locked(&mut store)?;
            return Err(StoreError::Validation("Pending Proposal is terminal".to_string()).into());
        }
        let proposal = store
            .pending_proposals
            .get_mut(&proposal_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
        proposal.status = ProposalStatus::Rejected;
        proposal.decided_by = Some(approver);
        proposal.decided_at = Some(now);
        proposal.rejection_reason = Some(reason);
        let proposal = proposal.clone();
        self.persist_locked(&mut store)?;
        Ok(proposal)
    }

    /// Reports whether the authenticated User has enough evidence for Personal Discovery.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization fails, the User identity is missing, or the store
    /// cannot be read.
    pub fn personal_discovery_readiness(
        &self,
        ctx: &AuthContext,
    ) -> Result<PersonalDiscoveryReadiness, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        Ok(readiness(&store, user_id, ctx.tenant_id))
    }

    /// Creates an immutable private plan and first-class User-scoped task atomically.
    pub fn request_personal_discovery(
        &self,
        ctx: &AuthContext,
        request: RequestPersonalDiscovery,
        now: chrono::DateTime<Utc>,
    ) -> Result<RequestedPersonalDiscovery, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let prepared = prepare_request(&request)?;
        let result_count = prepared.result_count;
        let requested_intent = prepared.persisted_intent();
        if let Some(existing) = retry(
            &store,
            user_id,
            ctx.tenant_id,
            &request.idempotency_key,
            ctx.harness_id,
        ) {
            if existing.plan.intent != requested_intent
                || existing.plan.result_count != result_count
            {
                return Err(AgentToolsError::PersonalDiscoveryIdempotencyConflict);
            }
            return Ok(existing);
        }
        if request.intent.is_none() && !readiness(&store, user_id, ctx.tenant_id).ready {
            return Err(AgentToolsError::PersonalDiscoveryNotReady);
        }
        let plan = build_plan(&store, user_id, ctx.tenant_id, prepared, now)?;
        let task = DiscoveryTask {
            id: Uuid::now_v7().into(),
            target: DiscoveryTaskTarget::Personal {
                discovery_plan_id: plan.id,
            },
            origin: DiscoveryTaskOrigin::PersonalRequest {
                idempotency_key: request.idempotency_key,
                requested_by: ctx.harness_id,
            },
            due_at: now,
            state: DiscoveryTaskState::Pending,
            attempts: Vec::new(),
            created_at: now,
        };
        let mut staged = store.clone();
        staged.discovery_plans.insert(plan.id, plan.clone());
        staged.discovery_tasks.insert(task.id, task.clone());
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::RequestPersonalDiscovery,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(RequestedPersonalDiscovery { plan, task })
    }

    /// Creates a named private Personal Discovery schedule.
    pub fn create_personal_discovery_schedule(
        &self,
        ctx: &AuthContext,
        request: CreatePersonalDiscoveryScheduleRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PersonalDiscoveryScheduleStatus, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let name = validate_name(&request.name).map_err(StoreError::Validation)?;
        let intent = normalize_intent(request.intent).map_err(StoreError::Validation)?;
        let result_count = validate_result_count(request.result_count.unwrap_or(10))
            .map_err(StoreError::Validation)?;
        if store.personal_discovery_schedules.values().any(|schedule| {
            schedule.user_id == user_id
                && schedule.tenant_id == ctx.tenant_id
                && schedule.name.eq_ignore_ascii_case(&name)
        }) {
            return Err(
                StoreError::Duplicate(format!("Personal Discovery schedule named {name}")).into(),
            );
        }
        let schedule = PersonalDiscoverySchedule {
            id: Uuid::now_v7().into(),
            user_id,
            tenant_id: ctx.tenant_id,
            name,
            cadence: request.cadence,
            intent,
            result_count,
            delivery_mode: request.delivery_mode,
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        let mut staged = store.clone();
        staged
            .personal_discovery_schedules
            .insert(schedule.id, schedule.clone());
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::CreatePersonalDiscoverySchedule,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(schedule_status(&store, schedule, now))
    }

    /// Lists private Personal Discovery schedules with inspectable backpressure.
    pub fn list_personal_discovery_schedules(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<PersonalDiscoveryScheduleStatus>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_schedule_read(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let mut schedules: Vec<_> = store
            .personal_discovery_schedules
            .values()
            .filter(|schedule| schedule.user_id == user_id && schedule.tenant_id == ctx.tenant_id)
            .cloned()
            .map(|schedule| schedule_status(&store, schedule, now))
            .collect();
        schedules.sort_by(|left, right| {
            left.schedule
                .name
                .cmp(&right.schedule.name)
                .then_with(|| left.schedule.id.cmp(&right.schedule.id))
        });
        Ok(schedules)
    }

    /// Inspects one private Personal Discovery schedule and its backpressure state.
    pub fn personal_discovery_schedule(
        &self,
        ctx: &AuthContext,
        schedule_id: PersonalDiscoveryScheduleId,
        now: chrono::DateTime<Utc>,
    ) -> Result<PersonalDiscoveryScheduleStatus, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_schedule_read(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let schedule = store
            .personal_discovery_schedules
            .get(&schedule_id)
            .ok_or_else(|| StoreError::NotFound("Personal Discovery schedule".into()))?
            .clone();
        if schedule.user_id != user_id || schedule.tenant_id != ctx.tenant_id {
            return Err(AgentToolsError::Forbidden {
                reason: "Personal Discovery schedule belongs to another User".into(),
            });
        }
        Ok(schedule_status(&store, schedule, now))
    }

    /// Updates configuration of a private Personal Discovery schedule.
    pub fn update_personal_discovery_schedule(
        &self,
        ctx: &AuthContext,
        schedule_id: PersonalDiscoveryScheduleId,
        request: UpdatePersonalDiscoveryScheduleRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PersonalDiscoveryScheduleStatus, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let existing = store
            .personal_discovery_schedules
            .get(&schedule_id)
            .ok_or_else(|| StoreError::NotFound("Personal Discovery schedule".into()))?
            .clone();
        if existing.user_id != user_id || existing.tenant_id != ctx.tenant_id {
            return Err(AgentToolsError::Forbidden {
                reason: "Personal Discovery schedule belongs to another User".into(),
            });
        }
        let name = match request.name {
            Some(name) => validate_name(&name).map_err(StoreError::Validation)?,
            None => existing.name.clone(),
        };
        if store.personal_discovery_schedules.values().any(|schedule| {
            schedule.id != schedule_id
                && schedule.user_id == user_id
                && schedule.tenant_id == ctx.tenant_id
                && schedule.name.eq_ignore_ascii_case(&name)
        }) {
            return Err(
                StoreError::Duplicate(format!("Personal Discovery schedule named {name}")).into(),
            );
        }
        let intent = match request.intent {
            Some(intent) => normalize_intent(intent).map_err(StoreError::Validation)?,
            None => existing.intent.clone(),
        };
        let result_count = match request.result_count {
            Some(count) => validate_result_count(count).map_err(StoreError::Validation)?,
            None => existing.result_count,
        };
        let schedule = PersonalDiscoverySchedule {
            id: existing.id,
            user_id: existing.user_id,
            tenant_id: existing.tenant_id,
            name,
            cadence: request.cadence.unwrap_or(existing.cadence),
            intent,
            result_count,
            delivery_mode: request.delivery_mode.unwrap_or(existing.delivery_mode),
            enabled: request.enabled.unwrap_or(existing.enabled),
            created_at: existing.created_at,
            updated_at: now,
        };
        let mut staged = store.clone();
        staged
            .personal_discovery_schedules
            .insert(schedule.id, schedule.clone());
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::UpdatePersonalDiscoverySchedule,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(schedule_status(&store, schedule, now))
    }

    /// Disables a schedule without removing its configuration history.
    pub fn disable_personal_discovery_schedule(
        &self,
        ctx: &AuthContext,
        schedule_id: PersonalDiscoveryScheduleId,
        now: chrono::DateTime<Utc>,
    ) -> Result<PersonalDiscoveryScheduleStatus, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let mut schedule = store
            .personal_discovery_schedules
            .get(&schedule_id)
            .ok_or_else(|| StoreError::NotFound("Personal Discovery schedule".into()))?
            .clone();
        if schedule.user_id != user_id || schedule.tenant_id != ctx.tenant_id {
            return Err(AgentToolsError::Forbidden {
                reason: "Personal Discovery schedule belongs to another User".into(),
            });
        }
        schedule.enabled = false;
        schedule.updated_at = now;
        let mut staged = store.clone();
        staged
            .personal_discovery_schedules
            .insert(schedule.id, schedule.clone());
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::DisablePersonalDiscoverySchedule,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(schedule_status(&store, schedule, now))
    }

    /// Removes a private Personal Discovery schedule configuration.
    ///
    /// Historical tasks, plans, batches, and results-ready events remain inspectable.
    pub fn remove_personal_discovery_schedule(
        &self,
        ctx: &AuthContext,
        schedule_id: PersonalDiscoveryScheduleId,
    ) -> Result<PersonalDiscoverySchedule, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let schedule = store
            .personal_discovery_schedules
            .get(&schedule_id)
            .ok_or_else(|| StoreError::NotFound("Personal Discovery schedule".into()))?
            .clone();
        if schedule.user_id != user_id || schedule.tenant_id != ctx.tenant_id {
            return Err(AgentToolsError::Forbidden {
                reason: "Personal Discovery schedule belongs to another User".into(),
            });
        }
        let mut staged = store.clone();
        staged.personal_discovery_schedules.remove(&schedule_id);
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::RemovePersonalDiscoverySchedule,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(schedule)
    }

    /// Attempts the single notify-when-supported delivery for a completed scheduled batch.
    ///
    /// Delivery never marks the batch reviewed. Queue-only batches retain silently.
    pub fn attempt_discovery_results_ready_notification(
        &self,
        ctx: &AuthContext,
        batch_id: DiscoveryResultBatchId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryResultsReadyNotificationOutcome, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_schedule_read(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let batch = store
            .discovery_result_batches
            .get(&batch_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Result Batch".into()))?
            .clone();
        if batch.user_id != user_id || batch.tenant_id != ctx.tenant_id {
            return Err(AgentToolsError::Forbidden {
                reason: "Discovery Result Batch belongs to another User".into(),
            });
        }
        let event = store
            .discovery_results_ready_events
            .get(&batch_id)
            .ok_or_else(|| StoreError::NotFound("Discovery-results-ready Event".into()))?
            .clone();
        match event.delivery_mode {
            PersonalDiscoveryDeliveryMode::QueueOnly => {
                return Ok(DiscoveryResultsReadyNotificationOutcome::QueueOnly { event, batch });
            }
            PersonalDiscoveryDeliveryMode::NotifyWhenSupported => {}
        }
        if event.notification_attempted_at.is_some()
            || batch.notification_state == DiscoveryResultNotificationState::Delivered
        {
            return Ok(DiscoveryResultsReadyNotificationOutcome::AlreadyAttempted { event, batch });
        }
        let mut staged = store.clone();
        let event = {
            let event = staged
                .discovery_results_ready_events
                .get_mut(&batch_id)
                .expect("BUG: event exists after lookup");
            event.notification_attempted_at = Some(now);
            event.clone()
        };
        let batch = {
            let batch = staged
                .discovery_result_batches
                .get_mut(&batch_id)
                .expect("BUG: batch exists after lookup");
            batch.notification_state = DiscoveryResultNotificationState::Delivered;
            batch.clone()
        };
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::AttemptDiscoveryResultsReadyNotification,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(DiscoveryResultsReadyNotificationOutcome::ShouldNotify { event, batch })
    }

    /// Reports planned source availability facts for a leased Personal Discovery Task.
    ///
    /// Stores availability facts only — never credentials, cookies, tokens, or browser state.
    /// On-demand runs may emit at most one authentication-needed notice per unavailable
    /// source state; scheduled runs never wait for authentication.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller does not hold the task lease, reports contain
    /// authentication material, authorization is denied, or persistence fails.
    pub fn report_discovery_source_availability(
        &self,
        ctx: &AuthContext,
        request: ReportDiscoverySourceAvailabilityRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<ReportedDiscoverySourceAvailability, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let (pod_id, harness_id) =
            authorized_discovery_task_mutation(&store, ctx, request.task_id)?;
        if pod_id.is_some() {
            return Err(StoreError::Validation(
                "report_discovery_source_availability requires a Personal Discovery Task".into(),
            )
            .into());
        }
        let task = store
            .discovery_tasks
            .get(&request.task_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?
            .clone();
        let plan_id = task
            .target
            .discovery_plan_id()
            .ok_or_else(|| StoreError::Validation("Personal Discovery Task missing plan".into()))?;
        let plan = store
            .discovery_plans
            .get(&plan_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Discovery Plan".into()))?;
        let DiscoveryTaskState::Leased(lease) = &task.state else {
            return Err(AgentToolsError::TaskLeaseRequired);
        };
        if lease.harness_id != harness_id || lease.expires_at <= now {
            return Err(AgentToolsError::TaskLeaseRequired);
        }

        let reports = normalize_reports(request.reports).map_err(StoreError::Validation)?;
        let eligible = normalize_browser_grant_eligibility(request.browser_grant_eligible_sources)
            .map_err(StoreError::Validation)?;
        let scheduled = task_is_scheduled(&task);

        let mut staged = store.clone();
        let availability = upsert_task_source_availability(
            &mut staged,
            TaskAvailabilityIdentity {
                task_id: request.task_id,
                user_id: plan.user_id,
                tenant_id: plan.tenant_id,
                reported_by: harness_id,
            },
            reports.clone(),
            eligible,
            now,
        );
        let authentication_notices = evaluate_authentication_notices(
            &mut staged,
            plan.user_id,
            plan.tenant_id,
            request.task_id,
            scheduled,
            &availability.reports,
            now,
        );
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::ReportDiscoverySourceAvailability,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(ReportedDiscoverySourceAvailability {
            availability,
            authentication_notices,
        })
    }

    /// Lists private authentication-needed notices for the authenticated User.
    pub fn list_authentication_needed_notices(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<AuthenticationNeededNotice>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let mut notices: Vec<_> = store
            .authentication_needed_notices
            .iter()
            .filter(|notice| notice.user_id == user_id && notice.tenant_id == ctx.tenant_id)
            .cloned()
            .collect();
        notices.sort_by_key(|notice| (notice.first_emitted_at, notice.id));
        Ok(notices)
    }

    /// Inspects lease-scoped source availability for one Personal Discovery Task.
    pub fn discovery_task_source_availability(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
    ) -> Result<DiscoveryTaskSourceAvailability, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_schedule_read(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        store
            .discovery_task_source_availability
            .get(&task_id)
            .filter(|entry| entry.user_id == user_id && entry.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Discovery Task source availability".into()).into())
    }

    /// Atomically completes a leased Personal Discovery Task into one ordered result batch.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller does not hold the task lease, submissions are
    /// invalid for the task, authorization is denied, or persistence fails.
    pub fn complete_discovery_result_batch(
        &self,
        ctx: &AuthContext,
        request: CompleteDiscoveryResultBatchRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryResultBatch, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let (pod_id, harness_id) =
            authorized_discovery_task_mutation(&store, ctx, request.task_id)?;
        if pod_id.is_some() {
            return Err(StoreError::Validation(
                "complete_discovery_result_batch requires a Personal Discovery Task".into(),
            )
            .into());
        }
        let task = store
            .discovery_tasks
            .get(&request.task_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?
            .clone();
        let plan_id = task
            .target
            .discovery_plan_id()
            .ok_or_else(|| StoreError::Validation("Personal Discovery Task missing plan".into()))?;
        let plan = store
            .discovery_plans
            .get(&plan_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Discovery Plan".into()))?;

        if let Some(existing) = store
            .discovery_result_batches
            .values()
            .find(|batch| batch.task_id == request.task_id)
            .cloned()
        {
            return Ok(existing);
        }

        let DiscoveryTaskState::Leased(lease) = &task.state else {
            return Err(AgentToolsError::TaskLeaseRequired);
        };
        if lease.harness_id != harness_id || lease.expires_at <= now {
            return Err(AgentToolsError::TaskLeaseRequired);
        }
        let lease = lease.clone();

        let (resolved_reports, eligible) = resolve_completion_reports(
            &store,
            request.task_id,
            request.source_availability,
            request.browser_grant_eligible_sources,
        )
        .map_err(StoreError::Validation)?;

        let mut seen_ids = HashSet::new();
        let mut ordered = Vec::with_capacity(request.submission_ids.len());
        for submission_id in &request.submission_ids {
            if !seen_ids.insert(*submission_id) {
                continue;
            }
            let submission = store
                .candidate_submissions
                .get(submission_id)
                .ok_or_else(|| StoreError::NotFound("Candidate Submission".into()))?;
            match &submission.target {
                CandidateSubmissionTarget::PersonalDiscovery {
                    task_id,
                    discovery_plan_id,
                    user_id,
                    ..
                } if *task_id == request.task_id
                    && *discovery_plan_id == plan.id
                    && *user_id == plan.user_id
                    && submission.submitted_by == harness_id =>
                {
                    ordered.push(submission.clone());
                }
                _ => {
                    return Err(StoreError::Validation(
                        "submission is not a task-bound Personal Discovery result for this lease"
                            .into(),
                    )
                    .into());
                }
            }
        }
        let ordered_refs: Vec<&CandidateSubmission> = ordered.iter().collect();
        let scheduled = task_is_scheduled(&task);
        let mut batch = build_discovery_result_batch(
            &store,
            &plan,
            request.task_id,
            &ordered_refs,
            &store.candidates,
            BatchAvailabilityInput {
                reported: &resolved_reports,
                scheduled,
            },
            now,
        );

        let scheduled_delivery = match task.origin {
            DiscoveryTaskOrigin::PersonalScheduled { schedule_id } => {
                let delivery_mode = store
                    .personal_discovery_schedules
                    .get(&schedule_id)
                    .map(|schedule| schedule.delivery_mode)
                    .unwrap_or(PersonalDiscoveryDeliveryMode::QueueOnly);
                Some((schedule_id, delivery_mode))
            }
            _ => None,
        };
        if let Some((_, delivery_mode)) = scheduled_delivery {
            batch.notification_state = notification_state_for_schedule(delivery_mode);
        }

        let mut staged = store.clone();
        upsert_task_source_availability(
            &mut staged,
            TaskAvailabilityIdentity {
                task_id: request.task_id,
                user_id: plan.user_id,
                tenant_id: plan.tenant_id,
                reported_by: harness_id,
            },
            resolved_reports.clone(),
            eligible,
            now,
        );
        let _notices = evaluate_authentication_notices(
            &mut staged,
            plan.user_id,
            plan.tenant_id,
            request.task_id,
            scheduled,
            &resolved_reports,
            now,
        );
        staged
            .discovery_result_batches
            .insert(batch.id, batch.clone());
        if let Some((schedule_id, delivery_mode)) = scheduled_delivery {
            ensure_results_ready_event(&mut staged, schedule_id, delivery_mode, &batch, now);
        }
        let task = staged
            .discovery_tasks
            .get_mut(&request.task_id)
            .expect("BUG: task exists after lookup");
        task.attempts.push(DiscoveryTaskAttempt {
            harness_id,
            started_at: lease.claimed_at,
            finished_at: now,
            outcome: DiscoveryTaskAttemptOutcome::Completed,
        });
        task.state = DiscoveryTaskState::Completed;
        record_harness_write(
            &mut staged,
            ctx,
            HarnessWriteOperation::CompleteDiscoveryResultBatch,
            None,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(batch)
    }

    /// Lists private Discovery Result Batches for the authenticated User.
    pub fn list_discovery_result_batches(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<DiscoveryResultBatch>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let mut batches: Vec<_> = store
            .discovery_result_batches
            .values()
            .filter(|batch| batch.user_id == user_id && batch.tenant_id == ctx.tenant_id)
            .cloned()
            .collect();
        batches.sort_by_key(|batch| (batch.created_at, batch.id));
        Ok(batches)
    }

    /// Inspects one private Discovery Result Batch owned by the authenticated User.
    pub fn discovery_result_batch(
        &self,
        ctx: &AuthContext,
        batch_id: DiscoveryResultBatchId,
    ) -> Result<DiscoveryResultBatch, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        store
            .discovery_result_batches
            .get(&batch_id)
            .filter(|batch| batch.user_id == user_id && batch.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Discovery Result Batch".into()).into())
    }

    /// Dismisses an entire ready batch without creating item-level learning evidence.
    pub fn dismiss_discovery_result_batch(
        &self,
        ctx: &AuthContext,
        batch_id: DiscoveryResultBatchId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryResultBatch, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let batch = store
            .discovery_result_batches
            .get_mut(&batch_id)
            .filter(|batch| batch.user_id == user_id && batch.tenant_id == ctx.tenant_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Result Batch".into()))?;
        match batch.state {
            DiscoveryResultBatchState::Dismissed => {
                return Ok(batch.clone());
            }
            DiscoveryResultBatchState::Reviewed => {
                return Err(StoreError::Validation(
                    "reviewed Discovery Result Batch cannot be dismissed".into(),
                )
                .into());
            }
            DiscoveryResultBatchState::Ready => {
                batch.state = DiscoveryResultBatchState::Dismissed;
                batch.dismissed_at = Some(now);
            }
        }
        let result = batch.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::DismissDiscoveryResultBatch,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(result)
    }

    /// Marks a ready batch reviewed without recording item-level learning evidence.
    pub fn mark_discovery_result_batch_reviewed(
        &self,
        ctx: &AuthContext,
        batch_id: DiscoveryResultBatchId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryResultBatch, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;
        let batch = store
            .discovery_result_batches
            .get_mut(&batch_id)
            .filter(|batch| batch.user_id == user_id && batch.tenant_id == ctx.tenant_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Result Batch".into()))?;
        match batch.state {
            DiscoveryResultBatchState::Reviewed => {
                return Ok(batch.clone());
            }
            DiscoveryResultBatchState::Dismissed => {
                return Err(StoreError::Validation(
                    "dismissed Discovery Result Batch cannot be marked reviewed".into(),
                )
                .into());
            }
            DiscoveryResultBatchState::Ready => {
                batch.state = DiscoveryResultBatchState::Reviewed;
                batch.reviewed_at = Some(now);
            }
        }
        let result = batch.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::MarkDiscoveryResultBatchReviewed,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(result)
    }

    /// Reviews one private Discovery Result Batch item with a deliberate User action.
    ///
    /// Save places into the private Inbox; Add to Pod uses existing curation boundaries;
    /// More like this / Not for me write replaceable private learning evidence; Ignore
    /// records item review state without learning. Whole-batch dismiss and notification
    /// paths remain separate and create no item evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when management authority is missing, the batch/item is missing
    /// or dismissed, Add to Pod authorization fails, or persistence fails.
    pub fn review_discovery_result_item(
        &self,
        ctx: &AuthContext,
        request: ReviewDiscoveryResultItemRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryResultItemReviewOutcome, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_personal_discovery_management(&store, ctx)?;
        authorize_interactive_user_action(
            &store,
            ctx,
            "Discovery Result review requires an interactive User action",
        )?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Personal Discovery requires an authenticated User".into())
        })?;

        let batch = store
            .discovery_result_batches
            .get(&request.batch_id)
            .filter(|batch| batch.user_id == user_id && batch.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Discovery Result Batch".into()))?;
        if batch.state == DiscoveryResultBatchState::Dismissed {
            return Err(StoreError::Validation(
                "dismissed Discovery Result Batch cannot receive item review".into(),
            )
            .into());
        }
        let item_index = batch
            .items
            .iter()
            .position(|item| item.candidate_id == request.candidate_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Result Item".into()))?;
        let current_item = batch.items[item_index].clone();
        let requested_action = request.action.action();

        // Idempotent repeat of the same action: return current state without inflating evidence.
        if let DiscoveryResultItemReview::Reviewed {
            action,
            placement_pod_id,
            content_item_id,
            ..
        } = &current_item.review
        {
            if *action == requested_action {
                let placement = match (placement_pod_id, content_item_id) {
                    (Some(pod_id), _) => store
                        .pod_placements
                        .values()
                        .find(|placement| {
                            placement.pod_id == *pod_id
                                && placement.candidate_id == current_item.candidate_id
                                && placement.status == PodPlacementStatus::Accepted
                        })
                        .cloned(),
                    _ => None,
                };
                let allowed_actions = discovery_result_allowed_actions(&store, ctx);
                let taste_profile = taste_profile_from_store(&store, ctx, user_id)?;
                return Ok(DiscoveryResultItemReviewOutcome {
                    batch,
                    item: current_item,
                    placement,
                    action_replaced: false,
                    allowed_actions,
                    taste_profile,
                });
            }
        }

        let candidate = store
            .candidates
            .get(&current_item.candidate_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        let submission = store
            .candidate_submissions
            .get(&current_item.submission_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate Submission".into()))?;

        let mut staged = store.clone();
        let previous_action = match current_item.review {
            DiscoveryResultItemReview::Reviewed { action, .. } => Some(action),
            DiscoveryResultItemReview::Unreviewed => None,
        };
        let action_replaced = previous_action.is_some_and(|action| action != requested_action);
        if action_replaced {
            clear_discovery_result_learning(&mut staged, request.batch_id, request.candidate_id);
        }

        let mut placement = None;
        let mut placement_pod_id = None;
        let mut content_item_id = None;
        let mut evidence_ids = Vec::new();

        match &request.action {
            DiscoveryResultItemActionRequest::Save => {
                let inbox = ensure_private_inbox(&mut staged, ctx, user_id, now)
                    .map_err(StoreError::Validation)?;
                let accepted = accept_discovery_result_into_pod(
                    &mut staged,
                    ctx,
                    &candidate,
                    &submission,
                    inbox.id,
                    CurationRationale::new("Saved from Personal Discovery")?,
                    now,
                )?;
                placement_pod_id = Some(inbox.id);
                content_item_id = accepted.content_item_id;
                placement = Some(accepted);
                // Save is durable placement; learning comes only from explicit reinforce/reject.
            }
            DiscoveryResultItemActionRequest::AddToPod {
                pod_id,
                curation_note,
            } => {
                authorize_local_pod_curation(&staged, ctx, *pod_id)?;
                let note = match curation_note {
                    Some(note) => note.clone(),
                    None => CurationRationale::new("Added from Personal Discovery")?,
                };
                let accepted = accept_discovery_result_into_pod(
                    &mut staged,
                    ctx,
                    &candidate,
                    &submission,
                    *pod_id,
                    note,
                    now,
                )?;
                if let Some(item) = accepted
                    .content_item_id
                    .and_then(|id| staged.submissions.get(&Uuid::from(id)).cloned())
                {
                    record_add_to_pod_learning(&mut staged, ctx, &item, now);
                }
                placement_pod_id = Some(*pod_id);
                content_item_id = accepted.content_item_id;
                placement = Some(accepted);
            }
            DiscoveryResultItemActionRequest::MoreLikeThis => {
                evidence_ids = record_discovery_result_learning(
                    &mut staged,
                    DiscoveryResultLearningInput {
                        user_id,
                        tenant_id: ctx.tenant_id,
                        candidate: &candidate,
                        submission: &submission,
                        kind: LearnedTasteEvidenceKind::MoreLikeThis,
                        direction: TasteEvidenceDirection::Supporting,
                        now,
                    },
                );
            }
            DiscoveryResultItemActionRequest::NotForMe => {
                evidence_ids = record_discovery_result_learning(
                    &mut staged,
                    DiscoveryResultLearningInput {
                        user_id,
                        tenant_id: ctx.tenant_id,
                        candidate: &candidate,
                        submission: &submission,
                        kind: LearnedTasteEvidenceKind::LessLikeThis,
                        direction: TasteEvidenceDirection::Opposing,
                        now,
                    },
                );
            }
            DiscoveryResultItemActionRequest::Ignore => {
                // Item review only — no learning evidence.
            }
        }

        set_discovery_result_learning_link(
            &mut staged,
            request.batch_id,
            request.candidate_id,
            evidence_ids,
        );

        // Durable placements from Save / Add to Pod remain inspectable after a later
        // learning-only action replaces the review action.
        let (final_placement_pod_id, final_content_item_id) = match &current_item.review {
            DiscoveryResultItemReview::Reviewed {
                placement_pod_id: existing_pod,
                content_item_id: existing_item,
                ..
            } => (
                placement_pod_id.or(*existing_pod),
                content_item_id.or(*existing_item),
            ),
            DiscoveryResultItemReview::Unreviewed => (placement_pod_id, content_item_id),
        };

        let batch = staged
            .discovery_result_batches
            .get_mut(&request.batch_id)
            .expect("BUG: batch exists after lookup");
        let item = batch
            .items
            .get_mut(item_index)
            .expect("BUG: item index valid");
        item.review = DiscoveryResultItemReview::Reviewed {
            action: requested_action,
            reviewed_at: now,
            replaced_action: previous_action.filter(|action| *action != requested_action),
            placement_pod_id: final_placement_pod_id,
            content_item_id: final_content_item_id,
        };

        let item = item.clone();
        let batch = batch.clone();
        let allowed_actions = discovery_result_allowed_actions(&staged, ctx);
        let taste_profile = taste_profile_from_store(&staged, ctx, user_id)?;
        record_harness_write_at(
            &mut staged,
            ctx,
            HarnessWriteOperation::ReviewDiscoveryResultItem,
            placement_pod_id,
            now,
        );
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(DiscoveryResultItemReviewOutcome {
            batch,
            item,
            placement,
            action_replaced,
            allowed_actions,
            taste_profile,
        })
    }

    /// Reads a plan for its interactive owner or the worker holding its task lease.
    pub fn discovery_plan(
        &self,
        ctx: &AuthContext,
        plan_id: DiscoveryPlanId,
    ) -> Result<DiscoveryPlan, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let plan = store
            .discovery_plans
            .get(&plan_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Plan".into()))?;
        if authorize_personal_discovery_management(&store, ctx).is_ok()
            && ctx.user_id == Some(plan.user_id)
            && ctx.tenant_id == plan.tenant_id
        {
            return Ok(plan.clone());
        }
        authorize_personal_discovery_execution(&store, ctx)?;
        let harness_id = ctx.harness_id.ok_or(AgentToolsError::TaskLeaseRequired)?;
        let assigned = store.discovery_tasks.values().any(|task| {
            task.target.discovery_plan_id() == Some(plan_id)
                && matches!(&task.state, DiscoveryTaskState::Leased(lease)
                    if lease.harness_id == harness_id && lease.expires_at > Utc::now())
        });
        if !assigned {
            return Err(AgentToolsError::TaskLeaseRequired);
        }
        Ok(plan.clone())
    }

    pub fn materialize_due_discovery_tasks(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<DiscoveryTask>, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, None)?;
        let scoped = harness_for_context(&store, ctx)?
            .and_then(|harness| harness.grant.pod_ids.as_ref())
            .cloned();
        let packages = store
            .pod_skill_packs
            .values()
            .filter(|package| {
                scoped
                    .as_ref()
                    .is_none_or(|pods| pods.contains(&package.pod_id))
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut created = Vec::new();
        for package in packages {
            let version = PackageVersion::new(package.version)
                .map_err(|error| StoreError::Validation(error.to_string()))?;
            for (source_rule_index, cadence) in source_rule_cadences(&package.sources_yaml)
                .map_err(|error| StoreError::Validation(error.to_string()))?
                .into_iter()
                .enumerate()
            {
                if cadence == SourceRuleCadence::OnDemand {
                    continue;
                }
                let due_at = cadence.period_start(now);
                let target = DiscoveryTaskTarget::Pod {
                    pod_id: package.pod_id,
                    package_version: version,
                };
                let exists = store.discovery_tasks.values().any(|task| {
                    matches!(task.origin, DiscoveryTaskOrigin::Scheduled { source_rule_index: index } if index == source_rule_index)
                        && task.target == target
                        && task.due_at == due_at
                });
                if exists {
                    continue;
                }
                let task = DiscoveryTask {
                    id: Uuid::now_v7().into(),
                    target,
                    origin: DiscoveryTaskOrigin::Scheduled { source_rule_index },
                    due_at,
                    state: DiscoveryTaskState::Pending,
                    attempts: Vec::new(),
                    created_at: now,
                };
                store.discovery_tasks.insert(task.id, task.clone());
                record_harness_write(
                    &mut store,
                    ctx,
                    HarnessWriteOperation::CreateDiscoveryTask,
                    Some(package.pod_id),
                );
                created.push(task);
            }
        }
        self.persist_locked(&mut store)?;
        Ok(created)
    }

    /// Creates immediate conversational discovery work through the task contract.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod or Package is missing, authorization is denied,
    /// or locking or persistence fails.
    pub fn create_immediate_discovery_task(
        &self,
        ctx: &AuthContext,
        request: CreateImmediateDiscoveryTaskRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::DiscoveryTasks,
            Some(request.pod_id),
        )?;
        let requested_by = ctx.harness_id.ok_or_else(|| AgentToolsError::Forbidden {
            reason: "immediate tasks require an Agent Harness".into(),
        })?;
        if request.instructions.trim().is_empty() || request.idempotency_key.trim().is_empty() {
            return Err(StoreError::Validation(
                "immediate task instructions and idempotency key must not be empty".into(),
            )
            .into());
        }
        if let Some(existing) = store.discovery_tasks.values().find(|task| {
            matches!(&task.origin,
            DiscoveryTaskOrigin::Immediate { idempotency_key, requested_by: creator, .. }
                if creator == &requested_by && idempotency_key == &request.idempotency_key)
        }) {
            return Ok(existing.clone());
        }
        let package = store
            .pod_skill_packs
            .get(&request.pod_id)
            .ok_or_else(|| StoreError::NotFound("Pod Package".into()))?;
        let task = DiscoveryTask {
            id: Uuid::now_v7().into(),
            target: DiscoveryTaskTarget::Pod {
                pod_id: request.pod_id,
                package_version: PackageVersion::new(package.version)
                    .map_err(|error| StoreError::Validation(error.to_string()))?,
            },
            origin: DiscoveryTaskOrigin::Immediate {
                instructions: request.instructions,
                idempotency_key: request.idempotency_key,
                requested_by,
            },
            due_at: now,
            state: DiscoveryTaskState::Pending,
            attempts: Vec::new(),
            created_at: now,
        };
        store.discovery_tasks.insert(task.id, task.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::CreateDiscoveryTask,
            Some(request.pod_id),
        );
        self.persist_locked(&mut store)?;
        Ok(task)
    }

    /// Lists visible tasks, presenting expired leases as pending work.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied or the store lock is poisoned.
    pub fn list_discovery_tasks(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<DiscoveryTask>, AgentToolsError> {
        let (can_materialize_pods, can_materialize_personal) = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            (
                authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, None).is_ok(),
                authorize_personal_discovery_execution(&store, ctx).is_ok()
                    || authorize_personal_discovery_management(&store, ctx).is_ok(),
            )
        };
        if can_materialize_pods {
            self.materialize_due_discovery_tasks(ctx, now)?;
        }
        if can_materialize_personal {
            if let Some(user_id) = ctx.user_id {
                let mut store = self
                    .store
                    .write()
                    .map_err(|_| AgentToolsError::LockPoisoned)?;
                let created =
                    materialize_due_personal_schedules(&mut store, user_id, ctx.tenant_id, now)?;
                for _ in &created {
                    record_harness_write(
                        &mut store,
                        ctx,
                        HarnessWriteOperation::CreateDiscoveryTask,
                        None,
                    );
                }
                if !created.is_empty() {
                    self.persist_locked(&mut store)?;
                }
            }
        }
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let personal_execution = authorize_personal_discovery_execution(&store, ctx);
        let pod_execution = authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, None);
        if let (Err(_), Err(error)) = (personal_execution, pod_execution) {
            return Err(error);
        }
        Ok(store
            .discovery_tasks
            .values()
            .filter(|task| authorize_discovery_task(&store, ctx, task).is_ok())
            .cloned()
            .map(|task| task_with_expired_lease_recorded(task, now))
            .collect())
    }

    /// Lists only tasks that can be claimed now, including safely expired leases.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied or the store lock is poisoned.
    pub fn list_ready_discovery_tasks(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<DiscoveryTask>, AgentToolsError> {
        Ok(self
            .list_discovery_tasks(ctx, now)?
            .into_iter()
            .filter(|task| task.state == DiscoveryTaskState::Pending && task.due_at <= now)
            .collect())
    }

    /// Returns one visible task and its retry history.
    ///
    /// # Errors
    ///
    /// Returns an error when the task is missing, authorization is denied, or the
    /// store lock is poisoned.
    pub fn discovery_task_status(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let task = store
            .discovery_tasks
            .get(&task_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
        authorize_discovery_task(&store, ctx, task)?;
        Ok(task_with_expired_lease_recorded(task.clone(), now))
    }

    /// Claims pending or safely expired work for one positive lease duration.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid durations, missing or terminal tasks, active
    /// competing leases, denied authorization, or persistence failures.
    pub fn claim_discovery_task(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
        lease_duration: DiscoveryLeaseSeconds,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let (pod_id, harness_id) = authorized_discovery_task_mutation(&store, ctx, task_id)?;
        let expires_at = now
            .checked_add_signed(lease_duration.as_duration())
            .ok_or_else(|| {
                StoreError::Validation(
                    "lease expiration is outside the supported time range".into(),
                )
            })?;
        let task = store
            .discovery_tasks
            .get_mut(&task_id)
            .expect("BUG: task exists after lookup");
        record_expired_lease(task, now);
        if matches!(
            task.state,
            DiscoveryTaskState::Completed | DiscoveryTaskState::TerminalFailure
        ) {
            return Err(AgentToolsError::TaskTerminal);
        }
        if matches!(&task.state, DiscoveryTaskState::Leased(lease) if lease.expires_at > now) {
            return Err(AgentToolsError::TaskLeaseConflict);
        }
        task.state = DiscoveryTaskState::Leased(DiscoveryTaskLease {
            harness_id,
            claimed_at: now,
            expires_at,
        });
        let result = task.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ClaimDiscoveryTask,
            pod_id,
        );
        self.persist_locked(&mut store)?;
        Ok(result)
    }

    /// Extends an active lease owned by the calling harness.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid durations, missing tasks, absent or foreign
    /// leases, denied authorization, or persistence failures.
    pub fn renew_discovery_task_lease(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
        lease_duration: DiscoveryLeaseSeconds,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let (pod_id, harness_id) = authorized_discovery_task_mutation(&store, ctx, task_id)?;
        let expires_at = now
            .checked_add_signed(lease_duration.as_duration())
            .ok_or_else(|| {
                StoreError::Validation(
                    "lease expiration is outside the supported time range".into(),
                )
            })?;
        let task = store
            .discovery_tasks
            .get_mut(&task_id)
            .expect("BUG: task exists after lookup");
        let DiscoveryTaskState::Leased(lease) = &mut task.state else {
            return Err(AgentToolsError::TaskLeaseRequired);
        };
        if lease.harness_id != harness_id || lease.expires_at <= now {
            return Err(AgentToolsError::TaskLeaseRequired);
        }
        lease.expires_at = expires_at;
        let result = task.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RenewDiscoveryTaskLease,
            pod_id,
        );
        self.persist_locked(&mut store)?;
        Ok(result)
    }

    /// Completes an actively leased task and records its successful attempt.
    ///
    /// # Errors
    ///
    /// Returns an error when the task or caller-owned lease is missing,
    /// authorization is denied, or persistence fails.
    pub fn complete_discovery_task(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        self.finish_discovery_task(ctx, task_id, now, None)
    }

    /// Fails an actively leased task, making it retryable or terminal by history.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty reason, missing task or caller-owned lease,
    /// denied authorization, or persistence failure.
    pub fn fail_discovery_task(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
        reason: String,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        if reason.trim().is_empty() {
            return Err(StoreError::Validation("failure reason must not be empty".into()).into());
        }
        self.finish_discovery_task(ctx, task_id, now, Some(reason))
    }

    fn finish_discovery_task(
        &self,
        ctx: &AuthContext,
        task_id: DiscoveryTaskId,
        now: chrono::DateTime<Utc>,
        failure: Option<String>,
    ) -> Result<DiscoveryTask, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let (pod_id, harness_id) = authorized_discovery_task_mutation(&store, ctx, task_id)?;
        let task = store
            .discovery_tasks
            .get_mut(&task_id)
            .expect("BUG: task exists after lookup");
        let DiscoveryTaskState::Leased(lease) = &task.state else {
            return Err(AgentToolsError::TaskLeaseRequired);
        };
        if lease.harness_id != harness_id || lease.expires_at <= now {
            return Err(AgentToolsError::TaskLeaseRequired);
        }
        // Personal Discovery success must produce exactly one Discovery Result Batch.
        // Workers complete via complete_discovery_result_batch; bare complete is invalid.
        // Failures remain available so leased personal work can still be released for retry.
        if failure.is_none() && matches!(task.target, DiscoveryTaskTarget::Personal { .. }) {
            return Err(StoreError::Validation(
                "Personal Discovery tasks complete only through complete_discovery_result_batch"
                    .into(),
            )
            .into());
        }
        let lease = lease.clone();
        let outcome = if let Some(reason) = failure {
            DiscoveryTaskAttemptOutcome::Failed { reason }
        } else {
            DiscoveryTaskAttemptOutcome::Completed
        };
        task.attempts.push(DiscoveryTaskAttempt {
            harness_id,
            started_at: lease.claimed_at,
            finished_at: now,
            outcome,
        });
        task.state = if matches!(
            task.attempts.last().map(|attempt| &attempt.outcome),
            Some(DiscoveryTaskAttemptOutcome::Completed)
        ) {
            DiscoveryTaskState::Completed
        } else if task.attempts.len() >= MAX_DISCOVERY_TASK_ATTEMPTS {
            DiscoveryTaskState::TerminalFailure
        } else {
            DiscoveryTaskState::Pending
        };
        let result = task.clone();
        let operation = if result.state == DiscoveryTaskState::Completed {
            HarnessWriteOperation::CompleteDiscoveryTask
        } else {
            HarnessWriteOperation::FailDiscoveryTask
        };
        record_harness_write(&mut store, ctx, operation, pod_id);
        self.persist_locked(&mut store)?;
        Ok(result)
    }

    /// Routes Pod creation through the sensitive-change policy.
    ///
    /// # Errors
    ///
    /// Returns an error when private creation or public proposal authorization,
    /// validation, signing, or persistence fails.
    pub fn request_create_pod(
        &self,
        ctx: &AuthContext,
        request: CreatePodRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<CreatePodOutcome, AgentToolsError> {
        if request.visibility == Visibility::Public {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::CreatePublicPod { request },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| CreatePodOutcome::PendingApproval(Box::new(proposal)));
        }
        self.create_pod(ctx, request).map(CreatePodOutcome::Created)
    }

    /// Atomically creates a Pod with its selected initial package, routing
    /// public exposure through a Pending Proposal.
    pub fn request_create_pod_lifecycle(
        &self,
        ctx: &AuthContext,
        request: CreatePodLifecycleRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<CreatePodOutcome, AgentToolsError> {
        if request.pod.visibility == Visibility::Public {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::CreatePublicPodLifecycle { request },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| CreatePodOutcome::PendingApproval(Box::new(proposal)));
        }
        self.create_pod_lifecycle_immediately(ctx, request, PodCreationMode::Canonical)
            .map(|created| CreatePodOutcome::Created(created.pod))
    }

    fn create_pod_lifecycle_immediately(
        &self,
        ctx: &AuthContext,
        request: CreatePodLifecycleRequest,
        mode: PodCreationMode,
    ) -> Result<CreatedPodPackage, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PodCuration)?;
        if !matches!(request.package, PodCreationPackage::Default) {
            authorize_harness_for_new_pod(&store, ctx, HarnessCapability::PackageManagement)?;
        }
        let mut staged = store.clone();
        let created = stage_pod_lifecycle(&mut staged, ctx, request, ctx.harness_id, mode)?;
        self.persist_locked(&mut staged)?;
        *store = staged;
        Ok(created)
    }

    /// Changes Pod visibility directly for restrictions and proposes expansions.
    pub fn request_set_pod_visibility(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        visibility: Visibility,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodVisibilityOutcome, AgentToolsError> {
        let current = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_pod_role_owner(&store, ctx, pod_id)?;
            let pod = store
                .pods
                .get(&pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            pod.visibility.clone()
        };
        if current == visibility {
            return Err(StoreError::Validation("Pod already has that visibility".into()).into());
        }
        if visibility_exposure(&visibility) > visibility_exposure(&current) {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::ExpandPodVisibility {
                            pod_id,
                            visibility,
                        },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| PodVisibilityOutcome::PendingApproval(Box::new(proposal)));
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_pod_role_owner(&store, ctx, pod_id)?;
        let was_public = current == Visibility::Public;
        let pod = store
            .pods
            .get_mut(&pod_id)
            .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
        pod.visibility = visibility;
        let result = pod.clone();
        if let Some(rules) = store.pod_rules.get_mut(&pod_id) {
            rules.federate_sources = result.visibility == Visibility::Public;
        }
        if was_public && result.visibility != Visibility::Public {
            let node = store.node_for_tenant(ctx.tenant_id)?;
            issue_origin_pod_withdrawal(&mut store, &node, &result.slug, None, now)?;
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::WithdrawPublicPod,
                Some(pod_id),
            );
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::CreatePod,
            Some(pod_id),
        );
        self.persist_locked(&mut store)?;
        Ok(PodVisibilityOutcome::Updated(result))
    }

    pub fn create_pod(
        &self,
        ctx: &AuthContext,
        request: CreatePodRequest,
    ) -> Result<Pod, AgentToolsError> {
        if request.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public exposure requires a Pending Proposal".to_string(),
            )
            .into());
        }
        self.create_pod_lifecycle_immediately(
            ctx,
            CreatePodLifecycleRequest {
                pod: request,
                package: PodCreationPackage::Default,
            },
            PodCreationMode::SimpleCreate,
        )
        .map(|created| created.pod)
    }

    #[cfg(test)]
    pub(crate) fn create_pod_for_test(
        &self,
        ctx: &AuthContext,
        request: CreatePodRequest,
    ) -> Result<Pod, AgentToolsError> {
        self.create_pod_lifecycle_immediately(
            ctx,
            CreatePodLifecycleRequest {
                pod: request,
                package: PodCreationPackage::Default,
            },
            PodCreationMode::SimpleCreate,
        )
        .map(|created| created.pod)
    }

    /// Atomically creates a private Pod and its complete initial Pod Package.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization or validation fails, the slug is in
    /// use, signing fails, or persistence cannot commit the complete operation.
    pub fn create_private_pod_with_package(
        &self,
        ctx: &AuthContext,
        request: CreatePrivatePodWithPackageRequest,
    ) -> Result<CreatedPodPackage, AgentToolsError> {
        self.create_pod_lifecycle_immediately(
            ctx,
            CreatePodLifecycleRequest {
                pod: CreatePodRequest {
                    name: request.name,
                    slug: request.slug,
                    description: request.description,
                    visibility: Visibility::Private,
                },
                package: PodCreationPackage::Initial {
                    package: request.package,
                },
            },
            PodCreationMode::PrivatePackage,
        )
    }

    /// Creates Feed eligibility for a local Pod without granting Pod authority.
    pub fn subscribe_local_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Subscription, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store
            .pods
            .get(&pod_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(pod.id),
        )?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Subscription requires an authenticated User".into())
        })?;
        if let Some(subscription) = store.subscriptions.values().find(|subscription| {
            subscription.user_id == user_id && subscription.local_pod_id == pod.id
        }) {
            return Ok(subscription.clone());
        }
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let now = Utc::now();
        let subscription =
            Subscription::new_local(Uuid::now_v7().into(), user_id, &pod, &node, now);
        store
            .subscriptions
            .insert(subscription.id, subscription.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::JoinPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(subscription)
    }

    pub fn join_pod(&self, ctx: &AuthContext, pod_slug: &str) -> Result<(), AgentToolsError> {
        let pod = self.pod_by_slug(pod_slug, ctx.tenant_id)?;
        self.subscribe_local_pod(ctx, pod.id).map(|_| ())
    }

    /// Removes Feed eligibility while leaving all Pod Roles unchanged.
    pub fn unsubscribe_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Subscription, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(pod_id),
        )?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("unsubscribe requires an authenticated User".into())
        })?;
        let subscription_id = store
            .subscriptions
            .values()
            .find(|subscription| {
                subscription.user_id == user_id && subscription.local_pod_id == pod_id
            })
            .map(|subscription| subscription.id)
            .ok_or_else(|| StoreError::NotFound("Subscription".into()))?;
        let subscription = store
            .subscriptions
            .remove(&subscription_id)
            .expect("Subscription was resolved above");
        self.persist_locked(&mut store)?;
        Ok(subscription)
    }

    /// Lists canonical Pod Roles for an authorized Owner or Curator.
    pub fn list_pod_roles(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<PodRoleAssignment>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(pod_id))?;
        let mut roles = store
            .pod_roles
            .iter()
            .filter(|assignment| assignment.pod_id == pod_id)
            .cloned()
            .collect::<Vec<_>>();
        roles.sort_by_key(|assignment| (assignment.created_at, assignment.user_id));
        Ok(roles)
    }

    /// Requests an Owner-authorized Pod Role grant through independent approval.
    pub fn request_grant_pod_role(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        user_id: UserId,
        role: PodRole,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal(
            ctx,
            SensitiveChange::GrantPodRole {
                pod_id,
                user_id,
                role,
            },
            now,
            now + Duration::hours(24),
        )
    }

    /// Requests an Owner-authorized Pod Role revocation through independent approval.
    pub fn request_revoke_pod_role(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        user_id: UserId,
        role: PodRole,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal(
            ctx,
            SensitiveChange::RevokePodRole {
                pod_id,
                user_id,
                role,
            },
            now,
            now + Duration::hours(24),
        )
    }

    /// Configures bounded Priority Subscription representation in future Feed Batches.
    ///
    /// # Errors
    ///
    /// Returns an error when Subscription management is denied, the User is not
    /// subscribed to the Pod, or persistence fails.
    pub fn set_priority_subscription(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        is_priority: bool,
    ) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::SubscriptionManagement,
            Some(pod_id),
        )?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Priority Subscription requires an authenticated User".into())
        })?;
        let subscription = store
            .subscriptions
            .values_mut()
            .find(|subscription| {
                subscription.user_id == user_id && subscription.local_pod_id == pod_id
            })
            .ok_or_else(|| StoreError::NotFound("Subscription".into()))?;
        subscription.is_priority = is_priority;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::SetPrioritySubscription,
            Some(pod_id),
        );
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn submit_link_to_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        request: SubmitLinkRequest,
    ) -> Result<Submission, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::CandidateSubmission,
            Some(pod.id),
        )?;
        let canonical_url = canonicalize_url(&request.url)?;
        let domain = Url::parse(&canonical_url)
            .map_err(|e| AgentToolsError::BadUrl(e.to_string()))?
            .domain()
            .unwrap_or("unknown")
            .to_string();
        let existing = store
            .submissions
            .values()
            .find(|s| s.canonical_url == canonical_url && s.tenant_id == ctx.tenant_id)
            .cloned();
        let submission = if let Some(existing) = existing {
            existing
        } else {
            let description = request.description.clone();
            let submission = Submission {
                id: Uuid::now_v7(),
                tenant_id: ctx.tenant_id,
                url: request.url.clone(),
                canonical_url: canonical_url.clone(),
                title: request.title.unwrap_or_else(|| canonical_url.clone()),
                description,
                domain,
                submitted_by: ctx.user_id,
                discovered_by_crawler: request.discovered_by_crawler,
                submitter_note: request.note,
                summary: request.description,
                media_references: Vec::new(),
                tags: request.tags,
                embedding: None,
                created_at: Utc::now(),
                origin_event_id: None,
            };
            store.submissions.insert(submission.id, submission.clone());
            submission
        };
        if !store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod.id && link.submission_id == submission.id)
        {
            store.submission_pods.push(SubmissionPod {
                submission_id: submission.id,
                pod_id: pod.id,
                created_at: Utc::now(),
            });
        }
        if pod.visibility != Visibility::Public {
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let event = sign_public_event(
                &node,
                "link_submitted",
                &pod.slug,
                json!({"submission": submission.clone()}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.event_log.push(event);
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::SubmitLinkToPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(submission)
    }

    /// Authenticates, validates, canonicalizes, and privately records external discovery evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization, task ownership, input validation,
    /// idempotency, canonicalization, persistence, or locking fails.
    pub fn submit_candidate(
        &self,
        ctx: &AuthContext,
        request: CandidateSubmissionRequest,
    ) -> Result<SubmittedCandidate, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let harness_id = ctx
            .harness_id
            .ok_or(AgentToolsError::CandidateHarnessRequired)?;
        let harness =
            harness_for_context(&store, ctx)?.ok_or(AgentToolsError::CandidateHarnessRequired)?;
        validate_candidate_submission(&store, ctx, &request)?;

        if let Some(existing) = idempotent_candidate_submission(&store, harness_id, &request)? {
            let candidate = store
                .candidates
                .get(&existing.candidate_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
            return Ok(SubmittedCandidate {
                candidate,
                submission: existing,
                allowed_actions: vec![CandidateAllowedAction::InspectCandidate],
            });
        }
        validate_candidate_task_context(&store, ctx, harness, &request)?;

        let mut projected = store.clone();

        let canonical_url = canonicalize_url(&request.evidence.source_url)?;
        let candidate = projected
            .candidates
            .values()
            .find(|candidate| {
                candidate.tenant_id == ctx.tenant_id && candidate.canonical_url == canonical_url
            })
            .cloned()
            .unwrap_or_else(|| Candidate {
                id: stable_candidate_uuid(
                    "candidate",
                    &[
                        &ctx.tenant_id
                            .map_or_else(|| "local".into(), |id| id.to_string()),
                        &canonical_url,
                    ],
                )
                .into(),
                tenant_id: ctx.tenant_id,
                source_url: canonical_url.clone(),
                canonical_url,
                review_state: CandidateReviewState::Pending,
                created_at: Utc::now(),
            });
        projected.candidates.insert(candidate.id, candidate.clone());

        let target = match &request.target {
            CandidateSubmissionRequestTarget::User {
                learn,
                interest_seed_metadata,
            } => CandidateSubmissionTarget::User {
                user_id: ctx
                    .user_id
                    .expect("User submission operation was validated"),
                learn: *learn,
                interest_seed_metadata: interest_seed_metadata.clone(),
            },
            CandidateSubmissionRequestTarget::PodPlacements {
                placements,
                task_context,
            } => CandidateSubmissionTarget::PodPlacements {
                placements: placements.clone(),
                task_context: *task_context,
            },
            CandidateSubmissionRequestTarget::PersonalDiscovery {
                task_id,
                allocation_role,
                source_facts,
            } => {
                let task = projected
                    .discovery_tasks
                    .get(task_id)
                    .expect("Personal Discovery task was validated");
                let plan_id = task
                    .target
                    .discovery_plan_id()
                    .expect("Personal Discovery task carries a plan");
                let plan = projected
                    .discovery_plans
                    .get(&plan_id)
                    .expect("Personal Discovery plan was validated");
                CandidateSubmissionTarget::PersonalDiscovery {
                    user_id: plan.user_id,
                    task_id: *task_id,
                    discovery_plan_id: plan_id,
                    allocation_role: *allocation_role,
                    source_facts: source_facts.clone(),
                }
            }
        };
        let submission = CandidateSubmission {
            id: stable_candidate_uuid(
                "candidate-submission",
                &[
                    &harness_id.to_string(),
                    &request.evidence.harness_idempotency_key,
                    &request.evidence.client_idempotency_key,
                ],
            )
            .into(),
            candidate_id: candidate.id,
            tenant_id: ctx.tenant_id,
            submitted_by: harness_id,
            target,
            evidence: request.evidence,
            created_at: Utc::now(),
        };
        projected
            .candidate_submissions
            .insert(submission.id, submission.clone());
        if submission.target.learning_enabled() {
            record_interest_seed(&mut projected, &candidate, &submission);
        }
        enrich_accepted_content_item(&mut projected, ctx, &candidate)?;
        record_harness_write(
            &mut projected,
            ctx,
            HarnessWriteOperation::SubmitCandidate,
            None,
        );
        self.persist_locked(&mut projected)?;
        *store = projected;
        Ok(SubmittedCandidate {
            candidate,
            submission,
            allowed_actions: vec![CandidateAllowedAction::InspectCandidate],
        })
    }

    /// Lists private Candidates visible within the active Harness Grant.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization state cannot be read.
    pub fn list_candidates(&self, ctx: &AuthContext) -> Result<Vec<Candidate>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let harness = harness_for_context(&store, ctx)?;
        let mut candidates = Vec::new();
        for candidate in store
            .candidates
            .values()
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
        {
            let visible = store
                .candidate_submissions
                .values()
                .filter(|submission| submission.candidate_id == candidate.id)
                .any(|submission| {
                    candidate_submission_is_visible(&store, ctx, harness, submission)
                });
            if visible {
                candidates.push(candidate.clone());
            }
        }
        Ok(candidates)
    }

    /// Inspects a private Candidate and all independently retained evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when the Candidate is missing, outside the caller's
    /// tenant or Pod scope, or the Home Node lock is poisoned.
    pub fn inspect_candidate(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
    ) -> Result<CandidateInspection, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        let harness = harness_for_context(&store, ctx)?;
        let mut submissions: Vec<_> = store
            .candidate_submissions
            .values()
            .filter(|submission| {
                submission.candidate_id == candidate_id
                    && candidate_submission_is_visible(&store, ctx, harness, submission)
            })
            .cloned()
            .collect();
        if submissions.is_empty() {
            return Err(AgentToolsError::Forbidden {
                reason: "Harness Grant cannot inspect this Candidate evidence".into(),
            });
        }
        submissions.sort_by_key(|submission| (submission.created_at, submission.id));
        let mut placements: Vec<_> = store
            .pod_placements
            .values()
            .filter(|placement| {
                placement.candidate_id == candidate_id
                    && candidate_placement_is_visible(&store, ctx, placement.pod_id)
            })
            .cloned()
            .collect();
        placements.sort_by_key(|placement| placement.pod_id);
        let proposal_pod_ids: HashSet<_> = submissions
            .iter()
            .flat_map(|submission| submission.target.placements())
            .map(|placement| placement.pod_id)
            .collect();
        let can_curate_all = harness.is_some()
            && proposal_pod_ids
                .iter()
                .all(|pod_id| authorize_local_pod_curation(&store, ctx, *pod_id).is_ok());
        let mut allowed_actions = Vec::new();
        let can_submit_evidence = !submissions.is_empty()
            && submissions
                .iter()
                .all(|submission| match submission.target {
                    CandidateSubmissionTarget::User { user_id, .. } => {
                        harness.is_some_and(|harness| {
                            harness.kind == AgentHarnessKind::Interactive
                                && harness.grant.pod_ids.is_none()
                                && ctx.user_id == Some(user_id)
                        })
                    }
                    CandidateSubmissionTarget::PodPlacements { ref placements, .. } => {
                        placements.iter().all(|placement| {
                            authorize_harness(
                                &store,
                                ctx,
                                HarnessCapability::CandidateSubmission,
                                Some(placement.pod_id),
                            )
                            .is_ok()
                        })
                    }
                    CandidateSubmissionTarget::PersonalDiscovery { .. } => false,
                });
        if harness.is_some() && can_submit_evidence {
            allowed_actions.push(CandidateAllowedAction::SubmitCandidateEvidence);
        }
        if can_curate_all && !proposal_pod_ids.is_empty() {
            allowed_actions.push(CandidateAllowedAction::EvaluateCandidate);
        }
        if harness.is_some()
            && store
                .pods
                .values()
                .any(|pod| authorize_local_pod_curation(&store, ctx, pod.id).is_ok())
        {
            allowed_actions.push(CandidateAllowedAction::RouteCandidatePlacement);
        }
        if harness.is_some()
            && placements.iter().any(|placement| {
                placement.status == PodPlacementStatus::Pending
                    && authorize_local_pod_curation(&store, ctx, placement.pod_id).is_ok()
            })
        {
            allowed_actions.push(CandidateAllowedAction::ReviewCandidatePlacement);
        }
        Ok(CandidateInspection {
            candidate,
            submissions,
            placements,
            allowed_actions,
        })
    }

    /// Changes a Pod's curation policy; Autonomous Curation must use a Pending Proposal.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization is denied, the Pod is remote or missing,
    /// Autonomous Curation is requested directly, or persistence fails.
    pub fn set_pod_curation_policy(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        policy: CurationPolicy,
        now: chrono::DateTime<Utc>,
    ) -> Result<CurationPolicy, AgentToolsError> {
        if matches!(policy, CurationPolicy::Autonomous { .. }) {
            return Err(StoreError::Validation(
                "Autonomous Curation requires a Pending Proposal".into(),
            )
            .into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        store.pod_curation_policies.insert(pod_id, policy);
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::SetPodCurationPolicy,
            Some(pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(policy)
    }

    /// Returns the Pod-owned Curation Policy, including its configured threshold.
    pub fn pod_curation_policy(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<CurationPolicy, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        Ok(store
            .pod_curation_policies
            .get(&pod_id)
            .copied()
            .unwrap_or_default())
    }

    /// Evaluates every proposed Pod Placement for a private Candidate independently.
    ///
    /// # Errors
    ///
    /// Returns an error when the Candidate is missing, a placement is outside the
    /// caller's local curation scope, evidence is inconsistent, or persistence fails.
    pub fn curate_candidate(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        now: chrono::DateTime<Utc>,
    ) -> Result<CandidateCurationResult, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        let submissions = candidate_submissions_for(&store, candidate_id);
        let proposals = merged_candidate_proposals(&submissions)?;
        if proposals.is_empty() {
            return Err(
                StoreError::Validation("Candidate has no Pod Placement evidence".into()).into(),
            );
        }
        for proposal in &proposals {
            authorize_local_pod_curation(&store, ctx, proposal.pod_id)?;
        }

        for proposal in proposals {
            if store
                .pod_placements
                .contains_key(&(candidate_id, proposal.pod_id))
            {
                continue;
            }
            let policy = store
                .pod_curation_policies
                .get(&proposal.pod_id)
                .copied()
                .unwrap_or_default();
            let trusted_confidence =
                trusted_placement_confidence(&store, &submissions, proposal.pod_id);
            let automatic_path = match policy {
                CurationPolicy::Manual => None,
                CurationPolicy::Assisted {
                    confidence_threshold,
                } if trusted_confidence.is_some_and(|confidence| {
                    confidence.value() >= confidence_threshold.value()
                }) =>
                {
                    Some(CurationPath::AssistedAutomatic)
                }
                CurationPolicy::Autonomous {
                    confidence_threshold,
                } if proposal.confidence.value() >= confidence_threshold.value() => {
                    Some(CurationPath::AutonomousAutomatic)
                }
                CurationPolicy::Assisted { .. } | CurationPolicy::Autonomous { .. } => None,
            };
            let actor = curation_actor(ctx);
            let status = automatic_path
                .map(|_| PodPlacementStatus::Accepted)
                .unwrap_or(PodPlacementStatus::Pending);
            let curation_path = automatic_path.unwrap_or(CurationPath::CandidateProposal);
            let content_item_id = if status == PodPlacementStatus::Accepted {
                Some(
                    ensure_content_item(
                        &mut store,
                        &candidate,
                        &submissions,
                        &proposal.source_submission_ids,
                        now,
                    )?
                    .id(),
                )
            } else {
                None
            };
            let placement = PodPlacement {
                candidate_id,
                pod_id: proposal.pod_id,
                content_item_id,
                reason: proposal.reason,
                confidence: proposal.confidence,
                source_submission_ids: proposal.source_submission_ids,
                origin_placements: Vec::new(),
                origin_withdrawals: Vec::new(),
                status,
                curation_path,
                actor,
                audit_history: vec![PlacementAuditEntry {
                    status,
                    curation_path,
                    actor,
                    note: None,
                    occurred_at: now,
                }],
                created_at: now,
                updated_at: now,
            };
            if status == PodPlacementStatus::Accepted {
                accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
            }
            store
                .pod_placements
                .insert((candidate_id, proposal.pod_id), placement);
        }
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::CurateCandidate,
            None,
            now,
        );
        self.persist_locked(&mut store)?;
        candidate_curation_result(&store, candidate_id)
    }

    /// Applies an authorized manual decision to one pending Pod Placement.
    ///
    /// # Errors
    ///
    /// Returns an error for missing or non-pending placements, unauthorized or
    /// remote Pods, empty notes, inconsistent evidence, or persistence failure.
    pub fn review_candidate_placement(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        pod_id: PodId,
        decision: PlacementReviewDecision,
        note: Option<CurationRationale>,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPlacement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        let submissions = candidate_submissions_for(&store, candidate_id);
        let current = store
            .pod_placements
            .get(&(candidate_id, pod_id))
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Pod Placement".into()))?;
        if current.status != PodPlacementStatus::Pending {
            return Err(
                StoreError::Validation("Pod Placement is not pending review".into()).into(),
            );
        }
        let status = match decision {
            PlacementReviewDecision::Accept => PodPlacementStatus::Accepted,
            PlacementReviewDecision::Reject => PodPlacementStatus::Rejected,
        };
        let actor = curation_actor(ctx);
        let content_item_id = if status == PodPlacementStatus::Accepted {
            Some(
                ensure_content_item(
                    &mut store,
                    &candidate,
                    &submissions,
                    &current.source_submission_ids,
                    now,
                )?
                .id(),
            )
        } else {
            None
        };
        let placement = store
            .pod_placements
            .get_mut(&(candidate_id, pod_id))
            .ok_or_else(|| StoreError::NotFound("Pod Placement".into()))?;
        placement.status = status;
        placement.content_item_id = content_item_id;
        placement.curation_path = CurationPath::ManualReview;
        placement.actor = actor;
        placement.updated_at = now;
        placement.audit_history.push(PlacementAuditEntry {
            status,
            curation_path: CurationPath::ManualReview,
            actor,
            note,
            occurred_at: now,
        });
        let placement = placement.clone();
        if status == PodPlacementStatus::Accepted {
            accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
        }
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::ReviewCandidatePlacement,
            Some(pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(placement)
    }

    /// Records an evidence-backed Routing Agent proposal for an authorized local Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when the Candidate or Pod is missing, the Pod is remote or
    /// outside the Harness Grant, evidence is empty, or persistence fails.
    pub fn route_candidate_placement(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        request: RouteCandidatePlacementRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPlacement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, request.pod_id)?;
        let harness = harness_for_context(&store, ctx)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        if let Some(existing) = store.pod_placements.get(&(candidate_id, request.pod_id)) {
            return Ok(existing.clone());
        }
        let actor = curation_actor(ctx);
        let policy = store
            .pod_curation_policies
            .get(&request.pod_id)
            .copied()
            .unwrap_or_default();
        let accepted = matches!(
            policy,
            CurationPolicy::Autonomous {
                confidence_threshold
            } if request.confidence.value() >= confidence_threshold.value()
        );
        let status = if accepted {
            PodPlacementStatus::Accepted
        } else {
            PodPlacementStatus::Pending
        };
        let curation_path = if accepted {
            CurationPath::AutonomousAutomatic
        } else {
            CurationPath::RoutingAgent
        };
        let submissions = candidate_submissions_for(&store, candidate_id)
            .into_iter()
            .filter(|submission| {
                matches!(
                    submission.target,
                    CandidateSubmissionTarget::PodPlacements { .. }
                ) && candidate_submission_is_visible(&store, ctx, harness, submission)
            })
            .collect::<Vec<_>>();
        let source_submission_ids = submissions
            .iter()
            .map(|submission| submission.id)
            .collect::<Vec<_>>();
        let content_item_id = if accepted {
            Some(
                ensure_content_item(
                    &mut store,
                    &candidate,
                    &submissions,
                    &source_submission_ids,
                    now,
                )?
                .id(),
            )
        } else {
            None
        };
        let placement = PodPlacement {
            candidate_id,
            pod_id: request.pod_id,
            content_item_id,
            reason: request.reason,
            confidence: request.confidence,
            source_submission_ids,
            origin_placements: Vec::new(),
            origin_withdrawals: Vec::new(),
            status,
            curation_path,
            actor,
            audit_history: vec![PlacementAuditEntry {
                status,
                curation_path,
                actor,
                note: None,
                occurred_at: now,
            }],
            created_at: now,
            updated_at: now,
        };
        if accepted {
            accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
        }
        store
            .pod_placements
            .insert((candidate_id, request.pod_id), placement.clone());
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::RouteCandidatePlacement,
            Some(request.pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(placement)
    }

    /// Immediately creates an Accepted Placement for an existing Content Item.
    ///
    /// # Errors
    ///
    /// Returns an error when the item or Pod is missing, authorization is denied,
    /// the Pod is remote, the note is empty, or persistence fails.
    pub fn add_content_item_to_pod(
        &self,
        ctx: &AuthContext,
        request: AddContentItemToPodRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPlacement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, request.pod_id)?;
        let item = store
            .submissions
            .get(&Uuid::from(request.content_item_id))
            .filter(|item| item.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
        let origin_placements = store
            .accepted_placement_projections
            .values()
            .filter(|placement| placement.content_item_id == request.content_item_id)
            .cloned()
            .collect::<Vec<_>>();
        if let Some((key, existing)) = store.pod_placements.iter().find(|(_, placement)| {
            placement.pod_id == request.pod_id
                && placement.content_item_id == Some(request.content_item_id)
        }) {
            if existing.status == PodPlacementStatus::Accepted {
                return Ok(existing.clone());
            }
            let key = *key;
            let candidate = store
                .candidates
                .get(&existing.candidate_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
            let actor = curation_actor(ctx);
            let placement = store
                .pod_placements
                .get_mut(&key)
                .ok_or_else(|| StoreError::NotFound("Pod Placement".into()))?;
            placement.status = PodPlacementStatus::Accepted;
            placement.curation_path = CurationPath::AddToPod;
            placement.actor = actor;
            let retained_origin_ids = placement
                .origin_placements
                .iter()
                .map(origin_placement_identity)
                .collect::<HashSet<_>>();
            placement
                .origin_placements
                .extend(origin_placements.into_iter().filter(|origin_placement| {
                    !retained_origin_ids.contains(&origin_placement_identity(origin_placement))
                }));
            placement.reason = request
                .curation_note
                .clone()
                .unwrap_or(CurationRationale::new("Explicit Add to Pod")?);
            placement.updated_at = now;
            placement.audit_history.push(PlacementAuditEntry {
                status: PodPlacementStatus::Accepted,
                curation_path: CurationPath::AddToPod,
                actor,
                note: request.curation_note,
                occurred_at: now,
            });
            let placement = placement.clone();
            accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
            record_add_to_pod_learning(&mut store, ctx, &item, now);
            record_harness_write_at(
                &mut store,
                ctx,
                HarnessWriteOperation::AddContentItemToPod,
                Some(request.pod_id),
                now,
            );
            self.persist_locked(&mut store)?;
            return Ok(placement);
        }
        let candidate_id = CandidateId::from(stable_candidate_uuid(
            "add-to-pod",
            &[&request.content_item_id.to_string()],
        ));
        let candidate = store
            .candidates
            .entry(candidate_id)
            .or_insert_with(|| Candidate {
                id: candidate_id,
                tenant_id: item.tenant_id,
                source_url: item.url.clone(),
                canonical_url: item.canonical_url.clone(),
                review_state: CandidateReviewState::Accepted,
                created_at: now,
            })
            .clone();
        let actor = curation_actor(ctx);
        let placement = PodPlacement {
            candidate_id,
            pod_id: request.pod_id,
            content_item_id: Some(item.id.into()),
            reason: request
                .curation_note
                .clone()
                .unwrap_or(CurationRationale::new("Explicit Add to Pod")?),
            confidence: CandidateConfidence::new(1.0)
                .map_err(|error| StoreError::Validation(error.to_string()))?,
            source_submission_ids: Vec::new(),
            origin_placements,
            origin_withdrawals: Vec::new(),
            status: PodPlacementStatus::Accepted,
            curation_path: CurationPath::AddToPod,
            actor,
            audit_history: vec![PlacementAuditEntry {
                status: PodPlacementStatus::Accepted,
                curation_path: CurationPath::AddToPod,
                actor,
                note: request.curation_note,
                occurred_at: now,
            }],
            created_at: now,
            updated_at: now,
        };
        accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
        record_add_to_pod_learning(&mut store, ctx, &item, now);
        store
            .pod_placements
            .insert((candidate_id, request.pod_id), placement.clone());
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::AddContentItemToPod,
            Some(request.pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(placement)
    }

    /// Reverses one accepted local private-Pod placement without deleting its Content Item.
    ///
    /// # Errors
    ///
    /// Returns an error when the placement is not accepted, authorization is denied,
    /// the Pod is public or remote, the reason is empty, or persistence fails.
    pub fn reverse_pod_placement(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        pod_id: PodId,
        reason: CurationRationale,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPlacement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        let pod = store
            .pods
            .get(&pod_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
        if pod.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public placement reversal requires a Pending Proposal".into(),
            )
            .into());
        }
        let actor = curation_actor(ctx);
        let placement = store
            .pod_placements
            .get_mut(&(candidate_id, pod_id))
            .ok_or_else(|| StoreError::NotFound("Pod Placement".into()))?;
        if placement.status != PodPlacementStatus::Accepted {
            return Err(StoreError::Validation("Pod Placement is not accepted".into()).into());
        }
        let content_item_id = placement.content_item_id.ok_or_else(|| {
            StoreError::Validation("Accepted Placement has no Content Item".into())
        })?;
        placement.status = PodPlacementStatus::Reversed;
        placement.curation_path = CurationPath::ManualReview;
        placement.actor = actor;
        placement.updated_at = now;
        placement.audit_history.push(PlacementAuditEntry {
            status: PodPlacementStatus::Reversed,
            curation_path: CurationPath::ManualReview,
            actor,
            note: Some(reason),
            occurred_at: now,
        });
        let placement = placement.clone();
        store.submission_pods.retain(|association| {
            !(association.pod_id == pod_id
                && association.submission_id == Uuid::from(content_item_id))
        });
        store
            .accepted_placement_projections
            .remove(&(content_item_id, pod_id));
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::ReviewCandidatePlacement,
            Some(pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(placement)
    }

    /// Removes one accepted Content Item placement using the Pod's visibility policy.
    ///
    /// Private and invite-only placements reverse immediately without deleting the
    /// Content Item. Public placements become Pending Proposals and emit their
    /// Placement Tombstone only when independently approved.
    pub fn request_remove_content_item_from_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        content_item_id: ContentItemId,
        reason: CurationRationale,
        now: chrono::DateTime<Utc>,
    ) -> Result<RemoveContentItemOutcome, AgentToolsError> {
        let (pod, candidate_id) = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_local_pod_curation(&store, ctx, pod_id)?;
            let pod = store
                .pods
                .get(&pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
            let candidate_id = store
                .pod_placements
                .values()
                .find(|placement| {
                    placement.pod_id == pod_id
                        && placement.content_item_id == Some(content_item_id)
                        && placement.status == PodPlacementStatus::Accepted
                })
                .map(|placement| placement.candidate_id)
                .ok_or_else(|| StoreError::NotFound("Accepted Placement".into()))?;
            (pod, candidate_id)
        };
        if pod.visibility == Visibility::Public {
            let proposal = self.create_pending_proposal_from_request(
                ctx,
                CreatePendingProposalRequest {
                    requested_change: SensitiveChange::RemovePublicSubmissionFromPod {
                        pod_id,
                        submission_id: content_item_id.into(),
                    },
                    expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                },
                now,
            )?;
            return Ok(RemoveContentItemOutcome::PendingApproval {
                proposal: Box::new(proposal),
            });
        }
        self.reverse_pod_placement(ctx, candidate_id, pod_id, reason, now)
            .map(|placement| RemoveContentItemOutcome::Removed {
                placement: Box::new(placement),
            })
    }

    /// Lists canonical Content Items with an Accepted Placement in one Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is missing, outside local curation scope,
    /// or the store lock is poisoned.
    pub fn list_content_items_for_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<ContentItem>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        let mut items = store
            .pod_placements
            .values()
            .filter(|placement| {
                placement.pod_id == pod_id && placement.status == PodPlacementStatus::Accepted
            })
            .filter_map(|placement| placement.content_item_id)
            .filter_map(|content_item_id| {
                store
                    .submissions
                    .get(&Uuid::from(content_item_id))
                    .map(ContentItem::from)
            })
            .collect::<Vec<_>>();
        items.sort_by_key(ContentItem::id);
        Ok(items)
    }

    /// Lists a Pod's complete accepted stream independently of Feed selection.
    ///
    /// This includes local and synchronized Accepted Placements visible through
    /// the caller's Feed-read grant and never applies ranking or delivery state.
    pub fn pod_content_stream(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<PodContentItem>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store
            .pods
            .get(&pod_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, Some(pod_id))?;
        let mut items = store
            .accepted_placement_projections
            .values()
            .filter(|placement| placement.pod_id == pod_id)
            .map(|accepted_placement| {
                let submission = store
                    .submissions
                    .get(&Uuid::from(accepted_placement.content_item_id))
                    .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
                Ok(PodContentItem {
                    content_item: ContentItem::from(submission),
                    accepted_placement: accepted_placement.clone(),
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        items.sort_by_key(|item| {
            (
                item.accepted_placement.accepted_at,
                item.accepted_placement.content_item_id,
            )
        });
        Ok(items)
    }

    /// Lists synchronization-safe Accepted Placement evidence for one visible Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the Pod is missing or outside
    /// the Harness Grant, the tenant boundary differs, or the lock is poisoned.
    pub fn accepted_placements_for_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
    ) -> Result<Vec<AcceptedPlacementProjection>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store
            .pods
            .get(&pod_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, Some(pod_id))?;
        let mut placements = store
            .accepted_placement_projections
            .values()
            .filter(|placement| placement.pod_id == pod_id)
            .cloned()
            .collect::<Vec<_>>();
        placements.sort_by_key(|placement| (placement.accepted_at, placement.content_item_id));
        Ok(placements)
    }

    /// Reads one locally governed Pod Placement with retained origin provenance.
    ///
    /// # Errors
    ///
    /// Returns an error when local Pod curation is denied, the placement is
    /// missing, or the Home Node store lock is poisoned.
    pub fn pod_placement(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        pod_id: PodId,
    ) -> Result<PodPlacement, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        store
            .pod_placements
            .get(&(candidate_id, pod_id))
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Pod Placement".into()).into())
    }

    /// Lists private Saves with any signed origin-withdrawal provenance.
    ///
    /// # Errors
    ///
    /// Returns an error when feedback access is denied, no User is authenticated,
    /// or the Home Node store lock is poisoned.
    pub fn saved_content_references(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<SavedContentReference>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Saved Content References require an authenticated User".into())
        })?;
        let mut saved = store
            .saves
            .iter()
            .filter(|(saved_user_id, _)| *saved_user_id == user_id)
            .filter_map(|(_, submission_id)| store.submissions.get(submission_id))
            .map(|item| {
                let content_item_id = ContentItemId::from(item.id);
                SavedContentReference {
                    content_reference: feed_content_reference(item),
                    origin_withdrawals: store
                        .placement_tombstones
                        .iter()
                        .filter(|tombstone| {
                            tombstone.origin_placement.content_item_id == content_item_id
                        })
                        .cloned()
                        .collect(),
                }
            })
            .collect::<Vec<_>>();
        saved.sort_by_key(|saved| saved.content_reference.content_item_id);
        Ok(saved)
    }

    /// Remove a submission's association with a pod. If no pod references the
    /// submission afterward, the submission and its assets (plus any per-user
    /// saves/notes/history keyed on it) are purged too. Emits a signed
    /// `link_removed` event for the pod. Returns `true` if the submission itself
    /// was purged. Errors with `NotFound` if the link is not present in the pod.
    pub fn remove_submission_from_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        let store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(pod.id))?;
        if pod.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public-content removal requires a Pending Proposal".to_string(),
            )
            .into());
        }

        drop(store);
        self.remove_submission_from_pod_immediately(ctx, pod_slug, submission_id)
    }

    #[cfg(test)]
    pub(crate) fn remove_submission_from_pod_for_test(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        self.remove_submission_from_pod_immediately(ctx, pod_slug, submission_id)
    }

    fn remove_submission_from_pod_immediately(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(pod.id))?;

        let before = store.submission_pods.len();
        store
            .submission_pods
            .retain(|link| !(link.pod_id == pod.id && link.submission_id == submission_id));
        if store.submission_pods.len() == before {
            return Err(StoreError::NotFound(format!(
                "submission {submission_id} in pod {pod_slug}"
            ))
            .into());
        }

        // Purge the submission entirely once no pod references it anymore.
        let purged = !store
            .submission_pods
            .iter()
            .any(|link| link.submission_id == submission_id);
        if purged {
            store.submissions.remove(&submission_id);
            store
                .submission_assets
                .retain(|_, asset| asset.submission_id != submission_id);
            store.saves.retain(|(_, sid)| *sid != submission_id);
            store
                .reading_history
                .retain(|(_, sid)| *sid != submission_id);
            store
                .private_notes
                .retain(|(_, sid), _| *sid != submission_id);
        }

        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "link_removed",
            &pod.slug,
            json!({"submission_id": submission_id, "submission_purged": purged}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RemoveSubmissionFromPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(purged)
    }

    /// Routes Pod Placement removal through the sensitive-change policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod or placement is missing, authorization is
    /// denied, proposal validation fails, or persistence fails.
    pub fn request_remove_submission_from_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
        now: chrono::DateTime<Utc>,
    ) -> Result<RemoveSubmissionOutcome, AgentToolsError> {
        let pod = self.pod_by_slug(pod_slug, ctx.tenant_id)?;
        if pod.visibility == Visibility::Public {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::RemovePublicSubmissionFromPod {
                            pod_id: pod.id,
                            submission_id,
                        },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| RemoveSubmissionOutcome::PendingApproval(Box::new(proposal)));
        }
        self.remove_submission_from_pod(ctx, pod_slug, submission_id)
            .map(|submission_purged| RemoveSubmissionOutcome::Removed { submission_purged })
    }

    pub fn add_submission_asset(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
        request: RepresentativeImageRequest,
    ) -> Result<SubmissionAsset, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        for pod_id in store
            .submission_pods
            .iter()
            .filter(|placement| placement.submission_id == submission_id)
            .map(|placement| placement.pod_id)
        {
            authorize_harness_pod_scope(&store, ctx, pod_id)?;
        }
        let pod_ids = store
            .submission_pods
            .iter()
            .filter(|placement| placement.submission_id == submission_id)
            .map(|placement| placement.pod_id)
            .collect::<Vec<_>>();
        for pod_id in &pod_ids {
            authorize_harness(
                &store,
                ctx,
                HarnessCapability::CandidateSubmission,
                Some(*pod_id),
            )?;
        }
        if request.url.is_none() && request.local_path.is_none() {
            return Err(StoreError::Validation(
                "representative image requires url or local_path".to_string(),
            )
            .into());
        }
        if let Some(existing) = store.submission_assets.values().find(|asset| {
            asset.submission_id == submission_id
                && asset.asset_type == SubmissionAssetType::RepresentativeImage
                && asset.source == request.source
                && asset.url == request.url
                && asset.local_path == request.local_path
        }) {
            return Ok(existing.clone());
        }
        let asset = SubmissionAsset {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            submission_id,
            asset_type: SubmissionAssetType::RepresentativeImage,
            source: request.source,
            url: request.url,
            local_path: request.local_path,
            mime_type: request.mime_type,
            alt_text: request.alt_text,
            created_at: Utc::now(),
        };
        store.submission_assets.insert(asset.id, asset.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::AddSubmissionAsset,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(asset)
    }

    pub fn assets_for_submission(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
    ) -> Result<Vec<SubmissionAsset>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        authorize_harness_submission_scope(&store, ctx, submission_id)?;
        Ok(store
            .submission_assets
            .values()
            .filter(|asset| asset.submission_id == submission_id)
            .cloned()
            .collect())
    }

    pub fn discover_in_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        request: DiscoverRequest,
    ) -> Result<Vec<DiscoveryItem>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, Some(pod.id))?;
        let pack = store
            .pod_skill_packs
            .get(&pod.id)
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let submissions = store.submissions_for_pod(pod.id);
        let user_id = effective_user_id(ctx, request.user_id);
        let preferences = user_id.and_then(|id| store.user_preferences.get(&(id, ctx.tenant_id)));
        let feedback = store
            .feedback_events
            .iter()
            .filter(|f| user_id.is_some_and(|id| f.user_id == id) && f.tenant_id == ctx.tenant_id)
            .collect();
        Ok(rank_discovery(RankingInput {
            pod: &pod,
            rules: store.pod_rules.get(&pod.id),
            skill_pack: pack,
            submissions,
            preferences,
            feedback,
            query: &request.query,
            avoid: &request.avoid,
            mode: request.mode,
            limit: request.limit,
        }))
    }

    /// Lists private briefs visible to the caller's User and Harness Grant.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, a brief falls outside the
    /// harness Pod scope, or the store lock is poisoned.
    pub fn list_briefs_for_harness(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<Brief>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let is_harness = ctx.harness_id.is_some();
        let mut briefs = Vec::new();
        'briefs: for brief in store
            .briefs
            .values()
            .filter(|brief| brief.tenant_id == ctx.tenant_id)
            .filter(|brief| !is_harness || brief.user_id == ctx.user_id)
        {
            for item in &brief.items {
                match authorize_harness_submission_scope(&store, ctx, item.submission_id) {
                    Ok(()) => {}
                    Err(AgentToolsError::Forbidden { .. }) => continue 'briefs,
                    Err(error) => return Err(error),
                }
            }
            briefs.push(brief.clone());
        }
        Ok(briefs)
    }

    pub fn generate_brief(
        &self,
        ctx: &AuthContext,
        request: GenerateBriefRequest,
    ) -> Result<Brief, AgentToolsError> {
        let user_id = effective_user_id(ctx, request.user_id);
        let query = request
            .query
            .clone()
            .unwrap_or_else(|| "daily brief".to_string());
        let mut all_items = Vec::new();
        for slug in &request.pod_slugs {
            let mut items = self.discover_in_pod(
                ctx,
                slug,
                DiscoverRequest {
                    query: query.clone(),
                    avoid: vec![],
                    limit: 4,
                    mode: DiscoveryMode::DeepMatch,
                    user_id,
                },
            )?;
            all_items.append(&mut items);
        }
        all_items = self.filter_brief_candidates(ctx, user_id, all_items)?;
        all_items.truncate(4);
        let roles = [
            "one thing to read",
            "one thing to explore",
            "one older gem",
            "one adjacent surprise",
        ];
        let brief_items = all_items
            .iter()
            .enumerate()
            .map(|(idx, item)| BriefItem {
                submission_id: item.submission_id,
                role: roles.get(idx).unwrap_or(&"recommended").to_string(),
                title: item.title.clone(),
                url: item.url.clone(),
                summary: item.short_summary.clone(),
                why_it_matters: item.why_belongs_in_pod.clone(),
                why_user_may_care: item.why_matches_request.clone(),
            })
            .collect();
        let brief = Brief {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            user_id,
            title: "Stumble Brief".to_string(),
            query: request.query,
            created_at: Utc::now(),
            private: true,
            items: brief_items,
            reflection: Some(
                "What would be useful to try, not just interesting to read?".to_string(),
            ),
        };
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        store.briefs.insert(brief.id, brief.clone());
        record_harness_write(&mut store, ctx, HarnessWriteOperation::GenerateBrief, None);
        self.persist_locked(&mut store)?;
        Ok(brief)
    }

    fn filter_brief_candidates(
        &self,
        ctx: &AuthContext,
        user_id: Option<UserId>,
        items: Vec<DiscoveryItem>,
    ) -> Result<Vec<DiscoveryItem>, AgentToolsError> {
        let Some(user_id) = user_id else {
            return Ok(items);
        };
        let stale_before = Utc::now() - Duration::days(30);
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let recently_briefed_own_links = store
            .briefs
            .values()
            .filter(|brief| {
                brief.tenant_id == ctx.tenant_id
                    && brief.user_id == Some(user_id)
                    && brief.created_at >= stale_before
            })
            .flat_map(|brief| brief.items.iter().map(|item| item.submission_id))
            .collect::<HashSet<_>>();

        Ok(items
            .into_iter()
            .filter(|item| {
                let Some(submission) = store.submissions.get(&item.submission_id) else {
                    return true;
                };
                if submission.submitted_by != Some(user_id) {
                    return true;
                }
                submission.created_at < stale_before
                    && !recently_briefed_own_links.contains(&item.submission_id)
            })
            .collect())
    }

    pub fn get_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()).into())
    }

    /// Reads one immutable historical Pod Package version.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod or version does not exist, the Harness is
    /// outside its Pod scope, or the store lock is poisoned.
    pub fn get_pod_package_version(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        version: PackageVersion,
    ) -> Result<PodPackage, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        store
            .pod_package_version(pod.id, version)
            .cloned()
            .ok_or_else(|| {
                StoreError::NotFound(format!("Pod Package version {}", version.value())).into()
            })
    }

    /// Requests a complete, version-aware revision from a portable Pod Package.
    ///
    /// Non-public origin packages are revised immediately. Public package
    /// revisions become Pending Proposals and do not alter authoritative state
    /// before approval.
    pub fn request_revise_pod_package(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        base_version: PackageVersion,
        files: BTreeMap<String, String>,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPackageRevisionOutcome, AgentToolsError> {
        validate_portable_package_files(&files)?;
        let contents = pod_package_contents_from_files(&files)?;
        let validation = validate_pod_package_contents(&contents);
        if !validation.valid {
            return Err(StoreError::Validation(validation.errors.join(", ")).into());
        }

        let patch = complete_package_patch(&contents);
        let is_public = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            let pod = store
                .pods
                .get(&pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            authorize_harness(
                &store,
                ctx,
                HarnessCapability::PackageManagement,
                Some(pod.id),
            )?;
            ensure_direct_package_revision_allowed_for_origin(&store, ctx, pod)?;
            let existing = store
                .pod_skill_packs
                .get(&pod.id)
                .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
            ensure_package_base_version(existing, base_version)?;
            verify_portable_package_history_for_base(&store, &files, existing)?;
            pod.visibility == Visibility::Public
        };

        if is_public {
            let proposal = self.create_pending_proposal(
                ctx,
                SensitiveChange::RevisePublicPodPackage {
                    pod_id,
                    base_version,
                    patch,
                },
                now,
                now + Duration::hours(24),
            )?;
            return Ok(PodPackageRevisionOutcome::PendingApproval(Box::new(
                proposal,
            )));
        }

        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store
            .pods
            .get(&pod_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::PackageManagement,
            Some(pod.id),
        )?;
        ensure_direct_package_revision_allowed(&store, ctx, &pod)?;
        let existing = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        ensure_package_base_version(&existing, base_version)?;
        verify_portable_package_history_for_base(&store, &files, &existing)?;

        let mut package = patch_skill_pack(&existing, patch);
        let created_at = now;
        package.created_at = created_at;
        package.updated_at = created_at;
        package.proposer_harness_id = ctx.harness_id;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_skill_pack_updated",
            &pod.slug,
            json!({"package": package}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.insert_pod_package_version(package.clone())?;
        store.pod_skill_packs.insert(pod.id, package.clone());
        store.event_log.push(event);
        refresh_public_pod_announcement_if_needed(&mut store, pod.id, now)?;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::PatchSkillPack,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(PodPackageRevisionOutcome::Revised(Box::new(package)))
    }

    pub fn pod_agent_context(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodAgentContext, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        let pack = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let validation = validate_skill_pack(&pack);
        if !validation.valid {
            return Err(StoreError::Validation(validation.errors.join(", ")).into());
        }
        Ok(PodAgentContext {
            pod_slug: pod.slug,
            pod_name: pod.name,
            skill_pack_version: pack.version,
            skill_md: pack.skill_md,
            pod_yaml: pack.pod_yaml,
            filters_yaml: pack.filters_yaml,
            validation,
        })
    }

    pub fn patch_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        patch: SkillPackPatch,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::PackageManagement,
            Some(pod.id),
        )?;
        ensure_direct_package_revision_allowed(&store, ctx, &pod)?;
        let existing = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let mut pack = patch_skill_pack(&existing, patch);
        let validation = validate_skill_pack(&pack);
        if !validation.valid {
            return Err(StoreError::Validation(validation.errors.join(", ")).into());
        }
        let now = Utc::now();
        pack.created_at = now;
        pack.updated_at = now;
        pack.proposer_harness_id = ctx.harness_id;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_skill_pack_updated",
            &pod.slug,
            json!({"package": pack}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.insert_pod_package_version(pack.clone())?;
        store.pod_skill_packs.insert(pod.id, pack.clone());
        store.event_log.push(event);
        refresh_public_pod_announcement_if_needed(&mut store, pod.id, now)?;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::PatchSkillPack,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(pack)
    }

    pub fn export_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<ExportedSkillPack, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        let pack = store
            .pod_skill_packs
            .get(&pod.id)
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let events_jsonl = store
            .portable_package_events_for_pod(&pod.slug)
            .into_iter()
            .map(|event| serde_json::to_string(&event))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default()
            .join("\n");
        Ok(export_skill_pack(pack, events_jsonl))
    }

    pub fn import_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        files: BTreeMap<String, String>,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::PackageManagement,
            Some(pod.id),
        )?;
        ensure_direct_package_revision_allowed(&store, ctx, &pod)?;
        let existing = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        validate_portable_package_files(&files)?;
        verify_portable_package_history(&store, &files)?;
        let mut pack = import_skill_pack(&existing, &files);
        let report = validate_skill_pack(&pack);
        if !report.valid {
            return Err(StoreError::Validation(report.errors.join(", ")).into());
        }
        let now = Utc::now();
        pack.created_at = now;
        pack.updated_at = now;
        pack.proposer_harness_id = ctx.harness_id;
        store.insert_pod_package_version(pack.clone())?;
        store.pod_skill_packs.insert(pod.id, pack.clone());
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_package_imported",
            &pod.slug,
            json!({"package": pack}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        refresh_public_pod_announcement_if_needed(&mut store, pod.id, now)?;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ImportSkillPack,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(pack)
    }

    pub fn fork_skill_pack(
        &self,
        ctx: &AuthContext,
        source_pod_slug: &str,
        target: CreatePodRequest,
    ) -> Result<PodSkillPack, AgentToolsError> {
        if target.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public Package Revisions require Pending Proposal approval".to_string(),
            )
            .into());
        }
        let source_pack = self.get_skill_pack(ctx, source_pod_slug)?;
        let target_pod = self.create_pod(ctx, target)?;
        let mut forked = fork_skill_pack(&source_pack, &target_pod);
        forked.proposer_harness_id = ctx.harness_id;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        forked.version = 2;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_package_forked",
            &target_pod.slug,
            json!({"package": forked}),
            store.latest_event_hash(&target_pod.slug),
        )?;
        store.insert_pod_package_version(forked.clone())?;
        store.pod_skill_packs.insert(target_pod.id, forked.clone());
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ForkSkillPack,
            Some(target_pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(forked)
    }

    pub fn validate_pod_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<ValidationReport, AgentToolsError> {
        let pack = self.get_skill_pack(ctx, pod_slug)?;
        Ok(validate_skill_pack(&pack))
    }

    pub fn add_source_to_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        source_type: CrawlerSourceType,
        url: String,
    ) -> Result<CrawlerSource, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, Some(pod.id))?;
        let source = CrawlerSource {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            pod_id: pod.id,
            source_type,
            url,
            enabled: true,
            crawl_interval_minutes: 1440,
            last_crawled_at: None,
            origin_event_id: None,
        };
        store.crawler_sources.insert(source.id, source.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::AddSourceToPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(source)
    }

    pub fn create_crawl_candidate(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        source_id: Uuid,
        request: SubmitLinkRequest,
    ) -> Result<CrawlCandidate, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, Some(pod.id))?;
        let canonical_url = canonicalize_url(&request.url)?;
        let domain = Url::parse(&canonical_url)
            .map_err(|e| AgentToolsError::BadUrl(e.to_string()))?
            .domain()
            .unwrap_or("unknown")
            .to_string();
        let candidate = CrawlCandidate {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            pod_id: pod.id,
            crawler_source_id: source_id,
            url: request.url,
            canonical_url,
            title: request
                .title
                .unwrap_or_else(|| "Untitled crawler candidate".to_string()),
            description: request.description,
            domain,
            summary: None,
            tags: request.tags,
            status: CrawlCandidateStatus::Pending,
            rejection_reason: None,
            created_at: Utc::now(),
        };
        store
            .crawl_candidates
            .insert(candidate.id, candidate.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::CreateCrawlCandidate,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(candidate)
    }

    pub fn promote_crawl_candidate(
        &self,
        ctx: &AuthContext,
        candidate_id: Uuid,
    ) -> Result<Submission, AgentToolsError> {
        let (pod_slug, request) = {
            let mut store = self
                .store
                .write()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            let candidate = store
                .crawl_candidates
                .get(&candidate_id)
                .ok_or_else(|| StoreError::NotFound("crawl candidate".to_string()))?;
            authorize_harness(
                &store,
                ctx,
                HarnessCapability::CandidateSubmission,
                Some(candidate.pod_id),
            )?;
            let candidate = store
                .crawl_candidates
                .get_mut(&candidate_id)
                .ok_or_else(|| StoreError::NotFound("crawl candidate".to_string()))?;
            candidate.status = CrawlCandidateStatus::Promoted;
            let candidate = candidate.clone();
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::PromoteCrawlCandidate,
                Some(candidate.pod_id),
            );
            self.persist_locked(&mut store)?;
            let pod = store
                .pods
                .get(&candidate.pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("pod".to_string()))?;
            (
                pod.slug,
                SubmitLinkRequest {
                    url: candidate.url.clone(),
                    title: Some(candidate.title.clone()),
                    description: candidate.description.clone(),
                    note: Some("Promoted from approved crawler source.".to_string()),
                    tags: candidate.tags.clone(),
                    discovered_by_crawler: true,
                },
            )
        };
        self.submit_link_to_pod(ctx, &pod_slug, request)
    }

    pub fn save_link(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
    ) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        authorize_harness_submission_scope(&store, ctx, submission_id)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        store.saves.insert((user_id, submission_id));
        store.feedback_events.push(FeedbackEvent {
            user_id,
            tenant_id: ctx.tenant_id,
            submission_id,
            event_type: FeedbackKind::Saved,
            reason: None,
            created_at: Utc::now(),
            local_only: true,
        });
        record_harness_write(&mut store, ctx, HarnessWriteOperation::SaveLink, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn block_source(&self, ctx: &AuthContext, source: String) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            });
        if !prefs.blocked_sources.contains(&source) {
            prefs.blocked_sources.push(source);
        }
        record_harness_write(&mut store, ctx, HarnessWriteOperation::BlockSource, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn block_topic(&self, ctx: &AuthContext, topic: String) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            });
        if !prefs.blocked_topics.contains(&topic) {
            prefs.blocked_topics.push(topic);
        }
        record_harness_write(&mut store, ctx, HarnessWriteOperation::BlockTopic, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn update_preferences(
        &self,
        ctx: &AuthContext,
        request: UpdatePreferencesRequest,
    ) -> Result<UserPreferences, AgentToolsError> {
        let Some(user_id) = ctx.user_id else {
            return Err(StoreError::Validation(
                "preferences require an authenticated user".to_string(),
            )
            .into());
        };
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            });
        if let Some(interests) = request.interests {
            prefs.interests = normalize_unique(interests);
        }
        if let Some(blocked_topics) = request.blocked_topics {
            prefs.blocked_topics = normalize_unique(blocked_topics);
        }
        if let Some(blocked_sources) = request.blocked_sources {
            prefs.blocked_sources = normalize_unique(blocked_sources);
        }
        if let Some(length) = request.preferred_brief_length {
            prefs.preferred_brief_length = length.clamp(1, 10);
        }
        if let Some(mode) = request.preferred_discovery_mode {
            prefs.preferred_discovery_mode = mode;
        }
        let prefs = prefs.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::UpdatePreferences,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(prefs)
    }

    /// Returns the User's private explicit and learned Taste Profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the interactive Harness Grant lacks feedback access,
    /// no User is authenticated, or local state cannot be read.
    pub fn taste_profile(&self, ctx: &AuthContext) -> Result<TasteProfile, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Taste Profile requires an authenticated User".into())
        })?;
        let preferences = store.user_preferences.get(&(user_id, ctx.tenant_id));
        let interest_seed_evidence = interest_seed_evidence(&store, user_id, ctx.tenant_id);
        let projections = taste_profile_projections(&store, user_id, ctx.tenant_id, preferences);
        let mut allowed_actions = vec![
            TasteProfileAllowedAction::Set,
            TasteProfileAllowedAction::Reset,
        ];
        if interest_seed_evidence.active_seed_count > 0 {
            allowed_actions.push(TasteProfileAllowedAction::Retract);
        }
        Ok(TasteProfile {
            user_id,
            tenant_id: ctx.tenant_id,
            explicit: ExplicitTastePreferences {
                interests: preferences
                    .map(|preferences| preferences.interests.clone())
                    .unwrap_or_default(),
                blocked_topics: preferences
                    .map(|preferences| preferences.blocked_topics.clone())
                    .unwrap_or_default(),
                blocked_sources: preferences
                    .map(|preferences| preferences.blocked_sources.clone())
                    .unwrap_or_default(),
                blocked_source_affinities: preferences
                    .map(|preferences| preferences.blocked_source_affinities.clone())
                    .unwrap_or_default(),
                recurrence_penalty_days: preferences
                    .map_or_else(RecurrencePenaltyDays::default, |preferences| {
                        preferences.recurrence_penalty_days
                    })
                    .get(),
            },
            learned: projections.learned,
            interest_seed_evidence,
            source_affinities: projections.source_affinities,
            allowed_actions,
        })
    }

    /// Retracts one canonical submission's learning contribution without deleting content.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks private retraction authority, the
    /// Interest Seed does not exist in the caller's User and tenant scope, or
    /// persistence fails.
    pub fn retract_interest_seed(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
    ) -> Result<TasteProfile, AgentToolsError> {
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Interest Seed retraction requires an authenticated User".into())
        })?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let mut projected = store.clone();
        let seed = projected
            .interest_seeds
            .get_mut(&(user_id, candidate_id))
            .filter(|seed| seed.tenant_id == ctx.tenant_id)
            .ok_or_else(|| StoreError::NotFound("Interest Seed".into()))?;
        if seed.retracted_at.is_none() {
            seed.retracted_at = Some(Utc::now());
            record_harness_write(
                &mut projected,
                ctx,
                HarnessWriteOperation::RetractInterestSeed,
                None,
            );
            self.persist_locked(&mut projected)?;
            *store = projected;
        }
        drop(store);
        self.taste_profile(ctx)
    }

    /// Replaces any supplied explicit Taste Profile settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the interactive Harness Grant lacks feedback access,
    /// no User is authenticated, or persistence fails.
    pub fn update_taste_profile(
        &self,
        ctx: &AuthContext,
        request: UpdateTasteProfileRequest,
    ) -> Result<TasteProfile, AgentToolsError> {
        let Some(user_id) = ctx.user_id else {
            return Err(StoreError::Validation(
                "Taste Profile requires an authenticated User".into(),
            )
            .into());
        };
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let preferences = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: Vec::new(),
                blocked_topics: Vec::new(),
                blocked_sources: Vec::new(),
                blocked_source_affinities: Vec::new(),
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            });
        if let Some(interests) = request.interests {
            preferences.interests = normalize_unique_case_insensitive(interests);
        }
        if let Some(blocked_topics) = request.blocked_topics {
            preferences.blocked_topics = normalize_unique_case_insensitive(blocked_topics);
        }
        if let Some(blocked_sources) = request.blocked_sources {
            preferences.blocked_sources = normalize_unique_case_insensitive(blocked_sources);
        }
        if let Some(blocked_source_affinities) = request.blocked_source_affinities {
            preferences.blocked_source_affinities =
                normalize_source_affinity_signals(blocked_source_affinities);
        }
        if let Some(recurrence_penalty_days) = request.recurrence_penalty_days {
            preferences.recurrence_penalty_days = recurrence_penalty_days;
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::UpdatePreferences,
            None,
        );
        self.persist_locked(&mut store)?;
        drop(store);
        self.taste_profile(ctx)
    }

    /// Resets one learned preference, or the entire learned layer.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks interactive private-profile authority,
    /// no User is authenticated, or persistence fails.
    pub fn reset_learned_taste(
        &self,
        ctx: &AuthContext,
        request: ResetLearnedTasteRequest,
    ) -> Result<TasteProfile, AgentToolsError> {
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Taste Profile requires an authenticated User".into())
        })?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let mut projected = store.clone();
        projected.taste_learning_evidence.retain(|evidence| {
            if evidence.user_id != user_id || evidence.tenant_id != ctx.tenant_id {
                return true;
            }
            request
                .signal
                .as_ref()
                .is_some_and(|signal| signal != &evidence.signal)
        });
        reset_interest_seed_evidence(
            &mut projected,
            user_id,
            ctx.tenant_id,
            request.signal.as_ref(),
        );
        record_harness_write(
            &mut projected,
            ctx,
            HarnessWriteOperation::ResetLearnedTaste,
            None,
        );
        self.persist_locked(&mut projected)?;
        *store = projected;
        drop(store);
        self.taste_profile(ctx)
    }

    pub fn node_info(&self, ctx: &AuthContext) -> Result<NodeInfo, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        harness_for_context(&store, ctx)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        Ok(NodeInfo {
            node_id: node.id,
            display_name: node.display_name,
            public_key: node.public_key,
            supported_protocol_version: CURRENT_PROTOCOL_VERSION.to_string(),
        })
    }

    /// Requests a Trust Policy addition without applying it immediately.
    ///
    /// # Errors
    ///
    /// Returns an error when proposal authorization, validation, or persistence fails.
    pub fn request_add_trusted_peer(
        &self,
        ctx: &AuthContext,
        display_name: String,
        base_url: String,
        public_key: String,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal_from_request(
            ctx,
            CreatePendingProposalRequest {
                requested_change: SensitiveChange::AddTrustedPeer {
                    node_id: Uuid::nil(),
                    display_name,
                    base_url,
                    public_key,
                },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
    }

    /// Requests approval to trust one canonical remote Node identity.
    ///
    /// # Errors
    ///
    /// Returns an error when proposal authorization, identity validation, or persistence fails.
    pub fn request_add_trusted_node(
        &self,
        ctx: &AuthContext,
        node: NodeInfo,
        base_url: String,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        if node.node_id.is_nil() {
            return Err(StoreError::Validation("canonical Node ID must not be nil".into()).into());
        }
        if node.supported_protocol_version != CURRENT_PROTOCOL_VERSION {
            return Err(StoreError::Validation(format!(
                "unsupported Node protocol {}",
                node.supported_protocol_version
            ))
            .into());
        }
        self.create_pending_proposal_from_request(
            ctx,
            CreatePendingProposalRequest {
                requested_change: SensitiveChange::AddTrustedPeer {
                    node_id: node.node_id,
                    display_name: node.display_name,
                    base_url,
                    public_key: node.public_key,
                },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
    }

    /// Requests an independently approved local Trust Policy change.
    ///
    /// # Errors
    ///
    /// Returns an error when proposal authorization, validation, or persistence fails.
    pub fn request_trust_policy_change(
        &self,
        ctx: &AuthContext,
        change: TrustPolicyChange,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal_from_request(
            ctx,
            CreatePendingProposalRequest {
                requested_change: SensitiveChange::ChangeTrustPolicy { change },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
    }

    /// Requests independent approval to disable one trusted peer.
    ///
    /// # Errors
    ///
    /// Returns an error when proposal authorization, validation, or persistence fails.
    pub fn request_remove_trusted_peer(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        now: chrono::DateTime<Utc>,
    ) -> Result<PendingProposal, AgentToolsError> {
        self.create_pending_proposal_from_request(
            ctx,
            CreatePendingProposalRequest {
                requested_change: SensitiveChange::RemoveTrustedPeer { peer_id },
                expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
            },
            now,
        )
    }

    /// Returns the authenticated User's local public discovery Trust Policy.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, no User is authenticated,
    /// or local state is unavailable.
    pub fn trust_policy(&self, ctx: &AuthContext) -> Result<TrustPolicy, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Trust Policy requires an authenticated User".into())
        })?;
        Ok(store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .cloned()
            .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id)))
    }

    pub fn well_known_node(
        &self,
        ctx: &AuthContext,
        base_url: &str,
    ) -> Result<WellKnownNode, AgentToolsError> {
        let node = self.node_info(ctx)?;
        let base = base_url.trim_end_matches('/');
        let mut endpoints = BTreeMap::new();
        endpoints.insert("node".to_string(), format!("{base}/federation/node"));
        endpoints.insert("pods".to_string(), format!("{base}/federation/pods"));
        endpoints.insert(
            "pod_manifest_template".to_string(),
            format!("{base}/federation/pods/{{slug}}/manifest"),
        );
        endpoints.insert(
            "pod_events_template".to_string(),
            format!("{base}/federation/pods/{{slug}}/events"),
        );
        endpoints.insert(
            "hub_search_pods".to_string(),
            format!("{base}/hub/search-pods"),
        );
        if self.bootstrap.enabled {
            endpoints.insert(
                "bootstrap_announcements".to_string(),
                format!("{base}/bootstrap/announcements"),
            );
            endpoints.insert(
                "bootstrap_announcement_stream".to_string(),
                format!("{base}/bootstrap/announcements/stream"),
            );
            endpoints.insert(
                "bootstrap_withdrawals".to_string(),
                format!("{base}/bootstrap/withdrawals"),
            );
        }
        if self.index.enabled {
            endpoints.insert(
                "index_search_announcements".to_string(),
                format!("{base}/discovery/announcements"),
            );
        }
        // Discovery Peer inbound endpoints are advertised only while the User has
        // explicitly opted into announcement serving (ADR-0044 / ADR-0049).
        if self.discovery_peer_service_enabled() {
            endpoints.insert(
                "discovery_peer_announcement_stream".to_string(),
                format!("{base}/discovery/peer/announcements/stream"),
            );
            endpoints.insert(
                "discovery_peer_advertisement_sample".to_string(),
                format!("{base}/discovery/peer/advertisements"),
            );
        }
        if self.bootstrap.enabled {
            endpoints.insert(
                "bootstrap_peer_advertisements".to_string(),
                format!("{base}/bootstrap/peer-advertisements"),
            );
            endpoints.insert(
                "bootstrap_peer_advertisement_sample".to_string(),
                format!("{base}/bootstrap/peer-advertisements"),
            );
        }
        Ok(WellKnownNode {
            protocol: CURRENT_PROTOCOL_VERSION.to_string(),
            node,
            endpoints,
        })
    }

    /// Enables inbound Discovery Peer announcement serving after verification.
    ///
    /// Requires a declared public endpoint and successful identity, protocol,
    /// HTTPS-outside-loopback, and reachability checks. Produces a signed
    /// expiring Discovery Peer Advertisement.
    ///
    /// # Errors
    ///
    /// Returns authorization, verification, or persistence errors.
    pub fn enable_discovery_peer_service(
        &self,
        ctx: &AuthContext,
        public_endpoint: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdvertisement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let advertisement = enable_discovery_peer_service(
            &mut store,
            &node,
            public_endpoint,
            self.discovery_peer_probe.as_ref(),
            now,
        )
        .map_err(|reason| AgentToolsError::DiscoveryPeerRejected {
            message: format!("discovery peer enable rejected: {reason}"),
            reason,
        })?;
        self.persist_locked(&mut store)?;
        Ok(advertisement)
    }

    /// Disables inbound Discovery Peer serving without affecting outbound discovery.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors.
    pub fn disable_discovery_peer_service(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerServiceState, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        disable_discovery_peer_service(&mut store, now);
        let state = store.discovery_peer_service.clone().unwrap_or_default();
        self.persist_locked(&mut store)?;
        Ok(state)
    }

    /// Reports Discovery Peer opt-in state for operators.
    ///
    /// # Errors
    ///
    /// Returns authorization or lock errors.
    pub fn discovery_peer_service_status(
        &self,
        ctx: &AuthContext,
    ) -> Result<DiscoveryPeerServiceState, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(store.discovery_peer_service.clone().unwrap_or_default())
    }

    /// Renews the current Discovery Peer Advertisement while service is enabled.
    ///
    /// # Errors
    ///
    /// Returns authorization, verification, or persistence errors.
    pub fn renew_discovery_peer_advertisement(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdvertisement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let advertisement = renew_discovery_peer_advertisement(
            &mut store,
            &node,
            self.discovery_peer_probe.as_ref(),
            now,
        )
        .map_err(|reason| AgentToolsError::DiscoveryPeerRejected {
            message: format!("discovery peer renew rejected: {reason}"),
            reason,
        })?;
        self.persist_locked(&mut store)?;
        Ok(advertisement)
    }

    /// Open Bootstrap admission for a signed Discovery Peer Advertisement.
    ///
    /// Requires no User account or Trusted Peer relationship.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::DiscoveryPeerRejected`] on verification failure.
    pub fn admit_discovery_peer_advertisement(
        &self,
        advertisement: DiscoveryPeerAdvertisement,
    ) -> Result<DiscoveryPeerAdmissionAcceptance, AgentToolsError> {
        self.admit_discovery_peer_advertisement_at(advertisement, Utc::now())
    }

    /// Open Bootstrap peer-advertisement admission at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::admit_discovery_peer_advertisement`].
    pub fn admit_discovery_peer_advertisement_at(
        &self,
        advertisement: DiscoveryPeerAdvertisement,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdmissionAcceptance, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let result = admit_discovery_peer_advertisement(
            &mut store,
            advertisement,
            self.discovery_peer_probe.as_ref(),
            self.bootstrap.enabled,
            now,
        );
        self.persist_locked(&mut store)?;
        result.map_err(|reason| AgentToolsError::DiscoveryPeerRejected {
            message: format!("discovery peer admission rejected: {reason}"),
            reason,
        })
    }

    /// Serves a bounded Announcement Stream page from an enabled Discovery Peer.
    ///
    /// Preserves Origin announcement bytes and signatures unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::DiscoveryPeerRejected`] when service is disabled
    /// or the cursor is invalid.
    pub fn peer_announcement_stream(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<AnnouncementStreamPage, AgentToolsError> {
        self.peer_announcement_stream_at(cursor, limit, Utc::now())
    }

    /// Peer Announcement Stream at an explicit clock time.
    ///
    /// Read-only: acquires a shared store lock and does not persist.
    ///
    /// # Errors
    ///
    /// Same as [`Self::peer_announcement_stream`].
    pub fn peer_announcement_stream_at(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
        now: chrono::DateTime<Utc>,
    ) -> Result<AnnouncementStreamPage, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        read_peer_announcement_stream(&store, cursor, limit, now).map_err(|reason| {
            AgentToolsError::DiscoveryPeerRejected {
                message: format!("discovery peer stream rejected: {reason}"),
                reason,
            }
        })
    }

    /// Serves a small randomized sample of current peer advertisements (unranked).
    ///
    /// Uses server entropy for shuffle selection. Deterministic sampling for
    /// tests is available via [`Self::peer_advertisement_sample_at`].
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::DiscoveryPeerRejected`] when service is disabled.
    pub fn peer_advertisement_sample(
        &self,
        limit: Option<usize>,
    ) -> Result<DiscoveryPeerAdvertisementSample, AgentToolsError> {
        self.peer_advertisement_sample_at(limit, server_sample_seed(), Utc::now())
    }

    /// Peer advertisement sample at an explicit clock time and optional test seed.
    ///
    /// `seed` is for Core/tests only; production HTTP must not accept a client
    /// seed query parameter.
    ///
    /// # Errors
    ///
    /// Same as [`Self::peer_advertisement_sample`].
    pub fn peer_advertisement_sample_at(
        &self,
        limit: Option<usize>,
        seed: u64,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdvertisementSample, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        sample_discovery_peer_advertisements(&store, limit, now, seed).map_err(|reason| {
            AgentToolsError::DiscoveryPeerRejected {
                message: format!("discovery peer sample rejected: {reason}"),
                reason,
            }
        })
    }

    /// Bootstrap-open sample of currently admitted peer advertisements (unranked).
    ///
    /// Available when Bootstrap capability is enabled. Does not require Discovery
    /// Peer serving opt-in. Uses server entropy for shuffle.
    ///
    /// # Errors
    ///
    /// Returns rejection when Bootstrap is disabled.
    pub fn bootstrap_peer_advertisement_sample(
        &self,
        limit: Option<usize>,
    ) -> Result<DiscoveryPeerAdvertisementSample, AgentToolsError> {
        self.bootstrap_peer_advertisement_sample_at(limit, server_sample_seed(), Utc::now())
    }

    /// Bootstrap peer-advertisement sample at an explicit clock and seed.
    ///
    /// # Errors
    ///
    /// Returns rejection when Bootstrap is disabled.
    pub fn bootstrap_peer_advertisement_sample_at(
        &self,
        limit: Option<usize>,
        seed: u64,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerAdvertisementSample, AgentToolsError> {
        if !self.bootstrap.enabled {
            return Err(AgentToolsError::DiscoveryPeerRejected {
                message: "bootstrap peer sample rejected: bootstrap_disabled".into(),
                reason: DiscoveryPeerAdmissionRejectionReason::BootstrapDisabled,
            });
        }
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        Ok(sample_known_discovery_peer_advertisements(
            &store, limit, now, seed,
        ))
    }

    /// Enables or disables automatic Discovery Peer gossip without deleting audit state.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors.
    pub fn set_automatic_peer_gossip_enabled(
        &self,
        ctx: &AuthContext,
        enabled: bool,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerGossipConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let config = set_automatic_peer_gossip_enabled(&mut store, enabled, now);
        self.persist_locked(&mut store)?;
        Ok(config)
    }

    /// Reports automatic peer gossip configuration.
    ///
    /// # Errors
    ///
    /// Returns authorization or lock errors.
    pub fn peer_gossip_config(
        &self,
        ctx: &AuthContext,
    ) -> Result<DiscoveryPeerGossipConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(ensure_discovery_peer_gossip_config(&mut store).clone())
    }

    /// Lists the rotating outbound Discovery Peer set with sync/health state.
    ///
    /// # Errors
    ///
    /// Returns authorization or lock errors.
    pub fn outbound_discovery_peers(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<OutboundDiscoveryPeerStatus>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(outbound_discovery_peer_statuses(&store))
    }

    /// Reports discovery readiness (including degraded Bootstrap-outage state).
    ///
    /// Direct Pod URL operation remains available when discovery is degraded.
    ///
    /// # Errors
    ///
    /// Returns authorization or lock errors.
    pub fn discovery_status(&self, ctx: &AuthContext) -> Result<DiscoveryStatus, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(discovery_status(&store))
    }

    /// Learns peer advertisements from Bootstrap and existing peers, then selects
    /// a bounded outbound set. Selection is randomized under `selection_seed`
    /// (tests inject a fixed seed; production may use server entropy).
    ///
    /// Network sample fetches run **outside** the store write lock; verification,
    /// retention, and selection run under a short write lock. Does not create
    /// Trusted Peer relationships.
    ///
    /// Reachability probing is **not** applied on this path by default: outbound
    /// learning verifies the signed advertisement locally (identity, capability,
    /// protocol, endpoint policy, lease, signature). Live reachability remains
    /// required when enabling a node as a Discovery Peer. Operators that want
    /// live endpoint match during learn can call the core retain helper with an
    /// injected [`DiscoveryPeerProbe`].
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors. Per-source sample failures
    /// fall through without aborting the pass.
    pub fn learn_and_select_discovery_peers(
        &self,
        ctx: &AuthContext,
        sample_client: &dyn PeerAdvertisementSampleClient,
        now: chrono::DateTime<Utc>,
        selection_seed: u64,
    ) -> Result<Vec<OutboundDiscoveryPeer>, AgentToolsError> {
        let (mut sources, local_node_id, gossip_enabled) = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
            let mut sources: Vec<String> = store
                .bootstrap_endpoints
                .values()
                .filter(|endpoint| endpoint.enabled)
                .map(|endpoint| endpoint.base_url.clone())
                .collect();
            sources.extend(
                list_active_outbound_peers(&store)
                    .into_iter()
                    .map(|peer| peer.public_endpoint),
            );
            let local_node_id = store.default_node().ok().map(|node| node.id);
            let gossip_enabled = peer_gossip_is_enabled(&store);
            (sources, local_node_id, gossip_enabled)
        };

        if !gossip_enabled {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            return Ok(list_active_outbound_peers(&store));
        }

        sources.sort();
        sources.dedup();

        // Sample fetches without holding any store lock (no HTTP under write).
        let fetched =
            crate::discovery_peer::fetch_peer_advertisement_samples(sample_client, &sources);

        // Short write: verify/retain/select only (no network I/O).
        // Probe is None: signed-ad local verify is sufficient for outbound learning.
        // Default UnreachableDiscoveryPeerProbe would soft-skip every ad.
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let selected = crate::discovery_peer::retain_learned_samples_and_select(
            &mut store,
            &fetched,
            None,
            local_node_id,
            now,
            selection_seed,
        );
        self.persist_locked(&mut store)?;
        Ok(selected)
    }

    /// Synchronizes Announcement Streams from each viable outbound Discovery Peer.
    ///
    /// Network I/O runs outside the store write lock. Invalid data, flooding,
    /// incompatible versions, expired advertisements, or repeated transport
    /// failures cause bounded backoff and automatic local eviction.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors. Per-peer typed failures are
    /// reported inside the [`DiscoveryPeerSyncReport`].
    pub fn sync_outbound_discovery_peers(
        &self,
        ctx: &AuthContext,
        client: &dyn DiscoveryPeerStreamClient,
        now: chrono::DateTime<Utc>,
    ) -> Result<DiscoveryPeerSyncReport, AgentToolsError> {
        let plans = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
            plan_discovery_peer_sync(&store, now)
        };

        let mut outcomes = Vec::with_capacity(plans.len());
        let mut retained_announcements = 0usize;
        let mut retained_withdrawals = 0usize;
        let mut evicted = Vec::new();

        for plan in plans {
            let fetched = fetch_discovery_peer_stream_pages(
                client,
                &plan.peer.public_endpoint,
                plan.cursor.clone(),
            );
            let outcome = {
                let mut store = self
                    .store
                    .write()
                    .map_err(|_| AgentToolsError::LockPoisoned)?;
                // Re-check lease under the write lock.
                if let Some(known) = store
                    .known_discovery_peer_advertisements
                    .get(&plan.peer.node_id)
                {
                    if !known.advertisement.lease_is_active(now) {
                        store.outbound_discovery_peers.remove(&plan.peer.node_id);
                        if let Some(state) =
                            store.discovery_peer_sync_states.get_mut(&plan.peer.node_id)
                        {
                            state.health = DiscoveryPeerHealth::Evicted;
                            state.last_attempt_at = Some(now);
                            state.last_error = Some(DiscoveryPeerSyncFailure::new(
                                DiscoveryPeerSyncFailureKind::ExpiredAdvertisement,
                                "peer advertisement expired before sync",
                            ));
                        }
                        self.persist_locked(&mut store)?;
                        evicted.push(plan.peer.node_id);
                        outcomes.push(DiscoveryPeerSyncOutcome {
                            node_id: plan.peer.node_id,
                            public_endpoint: plan.peer.public_endpoint.clone(),
                            ok: false,
                            pages_fetched: 0,
                            retained_announcements: 0,
                            retained_withdrawals: 0,
                            cursor: plan.cursor.clone(),
                            health: DiscoveryPeerHealth::Evicted,
                            error: Some(DiscoveryPeerSyncFailure::new(
                                DiscoveryPeerSyncFailureKind::ExpiredAdvertisement,
                                "peer advertisement expired before sync",
                            )),
                        });
                        continue;
                    }
                }
                let outcome =
                    apply_discovery_peer_stream_pages(&mut store, &plan.peer, fetched, now);
                if outcome.health == DiscoveryPeerHealth::Evicted {
                    store.outbound_discovery_peers.remove(&outcome.node_id);
                    evicted.push(outcome.node_id);
                }
                self.persist_locked(&mut store)?;
                outcome
            };
            retained_announcements =
                retained_announcements.saturating_add(outcome.retained_announcements);
            retained_withdrawals =
                retained_withdrawals.saturating_add(outcome.retained_withdrawals);
            outcomes.push(outcome);
        }

        Ok(DiscoveryPeerSyncReport {
            outcomes,
            retained_announcements,
            retained_withdrawals,
            evicted,
        })
    }

    /// Whether automatic peer gossip is currently enabled.
    #[must_use]
    pub fn peer_gossip_enabled(&self) -> bool {
        self.store
            .read()
            .map(|store| peer_gossip_is_enabled(&store))
            .unwrap_or(true)
    }

    /// Projects a verified announcement into the peer serving stream while enabled.
    ///
    /// Requires Administration. Verifies the announcement signature and active
    /// lease before appending to the peer-local stream.
    ///
    /// # Errors
    ///
    /// Returns authorization errors, rejection when service is disabled or the
    /// announcement fails verify/lease checks, or persistence errors.
    pub fn project_peer_serving_announcement(
        &self,
        ctx: &AuthContext,
        announcement: PodAnnouncement,
        now: chrono::DateTime<Utc>,
    ) -> Result<u64, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let sequence =
            project_peer_serving_announcement(&mut store, announcement, now).map_err(|reason| {
                AgentToolsError::DiscoveryPeerRejected {
                    message: format!("discovery peer project rejected: {reason}"),
                    reason,
                }
            })?;
        self.persist_locked(&mut store)?;
        Ok(sequence)
    }

    /// Open Bootstrap admission for a public Pod Announcement.
    ///
    /// Requires no User account or Trusted Peer relationship. Verifies origin
    /// identity, signature, lease, protocol, canonical URL, reachability, live
    /// public manifest, and resource bounds before retaining the announcement
    /// and appending a topic-neutral stream entry.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::BootstrapRejected`] with a stable reason code
    /// when verification or policy fails, or persistence errors when the store
    /// cannot commit.
    pub fn admit_bootstrap_announcement(
        &self,
        announcement: PodAnnouncement,
    ) -> Result<BootstrapAdmissionAcceptance, AgentToolsError> {
        self.admit_bootstrap_announcement_at(announcement, Utc::now())
    }

    /// Open Bootstrap admission at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::admit_bootstrap_announcement`].
    pub fn admit_bootstrap_announcement_at(
        &self,
        announcement: PodAnnouncement,
        now: chrono::DateTime<Utc>,
    ) -> Result<BootstrapAdmissionAcceptance, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let result = admit_bootstrap_announcement(
            &mut store,
            announcement,
            self.bootstrap.origin_probe.as_ref(),
            self.bootstrap.enabled,
            now,
        );
        // Persist acceptance and rejection audit rows transactionally.
        self.persist_locked(&mut store)?;
        result.map_err(|reason| AgentToolsError::BootstrapRejected {
            message: format!("bootstrap admission rejected: {reason}"),
            reason,
        })
    }

    /// Open Bootstrap admission for an Origin-signed Pod Withdrawal.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::BootstrapRejected`] on verification or policy
    /// failure, or a persistence error when the store cannot commit.
    pub fn admit_bootstrap_withdrawal(
        &self,
        withdrawal: PodWithdrawal,
    ) -> Result<BootstrapWithdrawalAcceptance, AgentToolsError> {
        self.admit_bootstrap_withdrawal_at(withdrawal, Utc::now())
    }

    /// Open Bootstrap withdrawal admission at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::admit_bootstrap_withdrawal`].
    pub fn admit_bootstrap_withdrawal_at(
        &self,
        withdrawal: PodWithdrawal,
        now: chrono::DateTime<Utc>,
    ) -> Result<BootstrapWithdrawalAcceptance, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let result =
            admit_bootstrap_withdrawal(&mut store, withdrawal, self.bootstrap.enabled, now);
        self.persist_locked(&mut store)?;
        result.map_err(|reason| AgentToolsError::BootstrapRejected {
            message: format!("bootstrap withdrawal rejected: {reason}"),
            reason,
        })
    }

    /// Reads a topic-neutral cursor-paginated Announcement Stream page.
    ///
    /// Emits pending lease-expiry transitions at `now` before serving. The
    /// stream never includes Taste Profiles, Subscriptions, feedback, or
    /// personalized ranking data.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::BootstrapRejected`] when Bootstrap is disabled
    /// or the cursor is unknown/invalid, or a persistence error when expiry
    /// transitions cannot be committed.
    pub fn announcement_stream(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<AnnouncementStreamPage, AgentToolsError> {
        self.announcement_stream_at(cursor, limit, Utc::now())
    }

    /// Reads the Announcement Stream at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::announcement_stream`].
    pub fn announcement_stream_at(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
        now: chrono::DateTime<Utc>,
    ) -> Result<AnnouncementStreamPage, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let page = read_announcement_stream(&mut store, cursor, limit, self.bootstrap.enabled, now)
            .map_err(|reason| AgentToolsError::BootstrapRejected {
                message: format!("announcement stream rejected: {reason}"),
                reason,
            })?;
        self.persist_locked(&mut store)?;
        Ok(page)
    }

    /// Lists configured Bootstrap endpoints in order (User-controlled, removable).
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied or the store lock is poisoned.
    pub fn list_bootstrap_endpoints(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<BootstrapEndpointConfig>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(list_bootstrap_endpoints(&store))
    }

    /// Reports configured Bootstrap endpoints with cursor and last-attempt state.
    ///
    /// Surfaces never include Taste Profile, Subscriptions, feedback, or other
    /// private discovery evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied or the store lock is poisoned.
    pub fn bootstrap_status(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<BootstrapEndpointStatus>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(bootstrap_endpoint_statuses(&store))
    }

    /// Adds a Bootstrap endpoint to the ordered User-controlled list.
    ///
    /// # Errors
    ///
    /// Returns validation, duplicate, authorization, or persistence errors.
    pub fn add_bootstrap_endpoint(
        &self,
        ctx: &AuthContext,
        label: &str,
        base_url: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<BootstrapEndpointConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let endpoint = add_bootstrap_endpoint(&mut store, label, base_url, now)?;
        self.persist_locked(&mut store)?;
        Ok(endpoint)
    }

    /// Enables or disables a configured Bootstrap endpoint.
    ///
    /// # Errors
    ///
    /// Returns not-found, authorization, or persistence errors.
    pub fn set_bootstrap_endpoint_enabled(
        &self,
        ctx: &AuthContext,
        endpoint_id: BootstrapEndpointId,
        enabled: bool,
    ) -> Result<BootstrapEndpointConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let endpoint = set_bootstrap_endpoint_enabled(&mut store, endpoint_id, enabled)?;
        self.persist_locked(&mut store)?;
        Ok(endpoint)
    }

    /// Removes a Bootstrap endpoint from configuration.
    ///
    /// Announcements known only through this endpoint leave current eligibility
    /// while remaining in the local audit store.
    ///
    /// # Errors
    ///
    /// Returns not-found, authorization, or persistence errors.
    pub fn remove_bootstrap_endpoint(
        &self,
        ctx: &AuthContext,
        endpoint_id: BootstrapEndpointId,
    ) -> Result<BootstrapEndpointConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let endpoint = remove_bootstrap_endpoint(&mut store, endpoint_id)?;
        self.persist_locked(&mut store)?;
        Ok(endpoint)
    }

    /// Ensures the sponsored default Bootstrap endpoint is present for new nodes.
    ///
    /// Idempotent: does nothing when any Bootstrap endpoint is already configured.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors.
    pub fn ensure_default_bootstrap_endpoint(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<BootstrapEndpointConfig>, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        ensure_default_bootstrap_endpoint(&mut store, now);
        self.persist_locked(&mut store)?;
        Ok(list_bootstrap_endpoints(&store))
    }

    /// Synchronizes Announcement Streams from each enabled Bootstrap in order.
    ///
    /// On transport or protocol failure the pass falls through to the next
    /// configured endpoint without discarding previously verified announcements.
    /// Outbound requests carry only cursor pagination fields.
    ///
    /// Network I/O runs **outside** the store write lock: endpoints and cursors
    /// are snapshotted under a read lock, pages are fetched without holding the
    /// store, and each endpoint is applied + persisted under a short write lock
    /// so partial progress survives.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors. Per-endpoint typed failures
    /// are reported inside the [`BootstrapSyncReport`], not as hard errors.
    pub fn sync_bootstrap_endpoints(
        &self,
        ctx: &AuthContext,
        client: &dyn AnnouncementStreamClient,
        now: chrono::DateTime<Utc>,
    ) -> Result<BootstrapSyncReport, AgentToolsError> {
        let plans = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
            plan_bootstrap_sync(&store)
        };

        let mut outcomes = Vec::with_capacity(plans.len());
        let mut retained_announcements = 0usize;
        let mut retained_withdrawals = 0usize;

        for plan in plans {
            // Fetch without holding the store lock (no network I/O under write).
            let fetched =
                fetch_bootstrap_stream_pages(client, &plan.endpoint.base_url, plan.cursor);
            let outcome = {
                let mut store = self
                    .store
                    .write()
                    .map_err(|_| AgentToolsError::LockPoisoned)?;
                let outcome =
                    apply_bootstrap_stream_pages(&mut store, &plan.endpoint, fetched, now);
                self.persist_locked(&mut store)?;
                outcome
            };
            retained_announcements =
                retained_announcements.saturating_add(outcome.retained_announcements);
            retained_withdrawals =
                retained_withdrawals.saturating_add(outcome.retained_withdrawals);
            outcomes.push(outcome);
        }

        Ok(BootstrapSyncReport {
            outcomes,
            retained_announcements,
            retained_withdrawals,
        })
    }

    pub fn pod_manifest(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodManifest, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        let pack = store
            .pod_skill_packs
            .get(&pod.id)
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let public_source_summary = store
            .crawler_sources
            .values()
            .filter(|source| source.pod_id == pod.id && source.enabled)
            .map(|source| source.url.clone())
            .collect();
        Ok(PodManifest {
            pod: pod.clone(),
            latest_known_event_hash: store.latest_federated_event_hash(&pod.slug),
            skill_pack_version: pack.version,
            public_source_summary,
        })
    }

    /// Produces a compact signed advertisement for a public Origin Pod.
    ///
    /// The signed payload includes a renewable 30-day Announcement Lease
    /// (`expires_at = announced_at + 30 days`) reflecting current public metadata.
    /// The Origin retains the announcement so later public-state changes can
    /// refresh the lease against the same direct address.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is not public or authoritative at this
    /// node, the direct address is invalid, signing fails, or state is unavailable.
    pub fn pod_announcement(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        public_pod_url: &str,
    ) -> Result<PodAnnouncement, AgentToolsError> {
        self.pod_announcement_at(ctx, pod_slug, public_pod_url, Utc::now())
    }

    /// Produces a Pod Announcement at an explicit issuance time (testable clocks).
    ///
    /// Also used to renew an active lease: each call issues a fresh signature
    /// with a new `announced_at` / `expires_at` reflecting current public state.
    ///
    /// # Errors
    ///
    /// Same as [`Self::pod_announcement`].
    pub fn pod_announcement_at(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        public_pod_url: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodAnnouncement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        if pod.visibility != Visibility::Public {
            return Err(StoreError::NotFound(format!("public Pod {pod_slug}")).into());
        }
        let node = store.node_for_tenant(ctx.tenant_id)?;
        if pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node.id)
        {
            return Err(StoreError::Validation(
                "only an Origin Node can announce its public Pod".into(),
            )
            .into());
        }
        let announcement =
            issue_and_retain_origin_pod_announcement(&mut store, &node, &pod, public_pod_url, now)?;
        self.persist_locked(&mut store)?;
        Ok(announcement)
    }

    /// Produces an Origin-signed Pod Withdrawal for a public Pod.
    ///
    /// When `make_private` is true, the local Pod is also restricted to private
    /// visibility. Existing Subscriptions and synchronized content are preserved.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod is not public or authoritative, ownership is
    /// denied, signing fails, or state is unavailable.
    pub fn withdraw_public_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        public_pod_url: Option<&str>,
        make_private: bool,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodWithdrawal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?.clone();
        authorize_pod_role_owner(&store, ctx, pod.id)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        if pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node.id)
        {
            return Err(StoreError::Validation(
                "only an Origin Node can withdraw its public Pod".into(),
            )
            .into());
        }
        if pod.visibility != Visibility::Public && !make_private {
            return Err(StoreError::NotFound(format!("public Pod {pod_slug}")).into());
        }
        let withdrawal = issue_origin_pod_withdrawal(
            &mut store,
            &node,
            &pod.slug,
            public_pod_url.map(str::to_owned),
            now,
        )?;
        if make_private && pod.visibility != Visibility::Private {
            let pod_id = pod.id;
            let mut_pod = store
                .pods
                .get_mut(&pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            mut_pod.visibility = Visibility::Private;
            if let Some(rules) = store.pod_rules.get_mut(&pod_id) {
                rules.federate_sources = false;
            }
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::WithdrawPublicPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(withdrawal)
    }

    /// Produces bounded Origin-signed Content Reference samples for Explore.
    ///
    /// The sample artifact is separate from Pod synchronization and does not
    /// create or require a Subscription.
    ///
    /// # Errors
    ///
    /// Returns an error when the announcement is invalid or stale, the Pod is
    /// not locally authoritative and public, the limit exceeds ten, signing
    /// fails, or state is unavailable.
    pub fn pod_explore_samples(
        &self,
        ctx: &AuthContext,
        announcement: &PodAnnouncement,
        limit: usize,
    ) -> Result<PodExploreSamples, AgentToolsError> {
        if limit > 10 {
            return Err(StoreError::Validation(
                "Pod Explore sample limit must not exceed 10".into(),
            )
            .into());
        }
        if !announcement.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        if announcement.origin_node_id != node.id
            || announcement.signer.public_key != node.public_key
        {
            return Err(StoreError::Validation(
                "Explore samples must be produced by the Pod's Origin Node".into(),
            )
            .into());
        }
        let pod = store.pod_by_slug(&announcement.pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        if pod.visibility != Visibility::Public
            || pod.origin_node_id.is_some_and(|origin| origin != node.id)
        {
            return Err(
                StoreError::NotFound(format!("public Pod {}", announcement.pod_slug)).into(),
            );
        }
        let package = store
            .pod_skill_packs
            .get(&pod.id)
            .ok_or_else(|| StoreError::NotFound("Pod Package".into()))?;
        let package_version = PackageVersion::new(package.version)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        if announcement.pod_name != pod.name
            || announcement.subject != pod.description
            || announcement.package_version != package_version
            || announcement.latest_event_hash != store.latest_federated_event_hash(&pod.slug)
        {
            return Err(StoreError::Validation(
                "Explore samples require the current Pod Announcement".into(),
            )
            .into());
        }
        let empty_policy = TrustPolicy::new(Uuid::nil(), ctx.tenant_id);
        let samples = explore_content_samples(
            &store,
            ctx.tenant_id,
            node.id,
            announcement,
            &empty_policy,
            limit,
        );
        sign_pod_explore_samples(
            &node,
            PodExploreSamples {
                id: Uuid::now_v7(),
                announcement_id: announcement.id,
                origin_node_id: node.id,
                signer: announcement.signer.clone(),
                pod_slug: announcement.pod_slug.clone(),
                samples,
                sampled_at: Utc::now(),
                signature: String::new(),
            },
        )
        .map_err(Into::into)
    }

    /// Lists peers explicitly enabled by this Home Node's local Trust Policy.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied or state is unavailable.
    pub fn trusted_peers(&self, ctx: &AuthContext) -> Result<Vec<TrustedPeer>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let mut peers = store
            .trusted_peers
            .values()
            .filter(|peer| peer.tenant_id == ctx.tenant_id && peer.enabled)
            .cloned()
            .collect::<Vec<_>>();
        peers.sort_by(|left, right| left.base_url.cmp(&right.base_url));
        Ok(peers)
    }

    /// Resolves one enabled peer within the caller's tenant trust boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied, the peer is absent,
    /// disabled, belongs to another tenant, or state is unavailable.
    pub fn trusted_peer(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
    ) -> Result<TrustedPeer, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        store
            .trusted_peers
            .get(&peer_id)
            .filter(|peer| peer.tenant_id == ctx.tenant_id && peer.enabled)
            .cloned()
            .ok_or_else(|| StoreError::UntrustedPeer.into())
    }

    /// Serves retained Origin-signed announcements to an explicitly trusted peer.
    ///
    /// The relay returns the Origin's bytes and signature unchanged, so it never
    /// becomes authoritative for the advertised Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied, the requesting peer is
    /// not trusted, retained signature verification fails, or state is unavailable.
    pub fn relay_pod_announcements(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
    ) -> Result<Vec<PodAnnouncement>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let peer = store
            .trusted_peers
            .get(&peer_id)
            .ok_or(StoreError::UntrustedPeer)?;
        if peer.tenant_id != ctx.tenant_id || !peer.enabled {
            return Err(StoreError::UntrustedPeer.into());
        }
        let now = Utc::now();
        let mut announcements = Vec::with_capacity(store.known_pod_announcements.len());
        for known in store.known_pod_announcements.values() {
            if !announcement_is_discovery_eligible(&store, &known.announcement, now) {
                continue;
            }
            if !known.announcement.verify()? {
                return Err(StoreError::InvalidSignature.into());
            }
            announcements.push(known.announcement.clone());
        }
        announcements.sort_by(|left, right| {
            left.origin_node_id
                .cmp(&right.origin_node_id)
                .then_with(|| left.pod_slug.cmp(&right.pod_slug))
        });
        Ok(announcements)
    }

    /// Verifies and retains an Origin-signed announcement delivered by a trusted peer.
    ///
    /// The immediate peer remains delivery provenance only and cannot replace the
    /// announcement's signer or alter its authoritative fields.
    ///
    /// # Errors
    ///
    /// Returns an error for an untrusted peer, invalid signature, stale package
    /// version, malformed direct address, denied administration, or persistence failure.
    pub fn receive_pod_announcement(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        announcement: PodAnnouncement,
    ) -> Result<KnownPodAnnouncement, AgentToolsError> {
        self.receive_pod_announcement_at(ctx, peer_id, announcement, Utc::now())
    }

    /// Verifies and retains a peer-delivered announcement at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::receive_pod_announcement`].
    pub fn receive_pod_announcement_at(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        announcement: PodAnnouncement,
        now: chrono::DateTime<Utc>,
    ) -> Result<KnownPodAnnouncement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let peer = store
            .trusted_peers
            .get(&peer_id)
            .ok_or(StoreError::UntrustedPeer)?;
        if peer.tenant_id != ctx.tenant_id || !peer.enabled {
            return Err(StoreError::UntrustedPeer.into());
        }
        let known = retain_verified_pod_announcement(
            &mut store,
            announcement,
            DeliveryProvenance::peer(peer_id),
            now,
        )?;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ReceivePodAnnouncement,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(known)
    }

    /// Aggregates a verified announcement on an optional, non-authoritative Index Node.
    ///
    /// # Errors
    ///
    /// Returns an error when the signature or direct address is invalid, the
    /// announcement is stale or expired, the Pod is withdrawn, or persistence fails.
    pub fn index_pod_announcement(
        &self,
        announcement: PodAnnouncement,
    ) -> Result<KnownPodAnnouncement, AgentToolsError> {
        self.index_pod_announcement_at(announcement, Utc::now())
    }

    /// Indexes a verified announcement at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::index_pod_announcement`].
    pub fn index_pod_announcement_at(
        &self,
        announcement: PodAnnouncement,
        now: chrono::DateTime<Utc>,
    ) -> Result<KnownPodAnnouncement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let known = retain_verified_pod_announcement(
            &mut store,
            announcement,
            DeliveryProvenance::LOCAL,
            now,
        )?;
        self.persist_locked(&mut store)?;
        Ok(known)
    }

    /// Verifies and retains an Origin-signed Pod Withdrawal from a trusted peer.
    ///
    /// # Errors
    ///
    /// Returns an error for an untrusted peer, invalid signature, stale withdrawal,
    /// denied administration, or persistence failure.
    pub fn receive_pod_withdrawal(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        withdrawal: PodWithdrawal,
    ) -> Result<KnownPodWithdrawal, AgentToolsError> {
        self.receive_pod_withdrawal_at(ctx, peer_id, withdrawal, Utc::now())
    }

    /// Retains a peer-delivered Pod Withdrawal at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::receive_pod_withdrawal`].
    pub fn receive_pod_withdrawal_at(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        withdrawal: PodWithdrawal,
        now: chrono::DateTime<Utc>,
    ) -> Result<KnownPodWithdrawal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let peer = store
            .trusted_peers
            .get(&peer_id)
            .ok_or(StoreError::UntrustedPeer)?;
        if peer.tenant_id != ctx.tenant_id || !peer.enabled {
            return Err(StoreError::UntrustedPeer.into());
        }
        let known = retain_verified_pod_withdrawal(&mut store, withdrawal, Some(peer_id), now)?;
        // Keep Bootstrap stream closed under co-located Index/peer withdraw retain.
        project_bootstrap_withdrawal(&mut store, &known.withdrawal, now);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ReceivePodWithdrawal,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(known)
    }

    /// Aggregates a verified Pod Withdrawal on an optional Index Node.
    ///
    /// # Errors
    ///
    /// Returns an error when the signature is invalid, the withdrawal is stale,
    /// or persistence fails.
    pub fn index_pod_withdrawal(
        &self,
        withdrawal: PodWithdrawal,
    ) -> Result<KnownPodWithdrawal, AgentToolsError> {
        self.index_pod_withdrawal_at(withdrawal, Utc::now())
    }

    /// Indexes a verified Pod Withdrawal at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::index_pod_withdrawal`].
    pub fn index_pod_withdrawal_at(
        &self,
        withdrawal: PodWithdrawal,
        now: chrono::DateTime<Utc>,
    ) -> Result<KnownPodWithdrawal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let known = retain_verified_pod_withdrawal(&mut store, withdrawal, None, now)?;
        // Keep Bootstrap stream closed under co-located Index/peer withdraw retain.
        project_bootstrap_withdrawal(&mut store, &known.withdrawal, now);
        self.persist_locked(&mut store)?;
        Ok(known)
    }

    /// Searches verified announcements held by this Index-capable node.
    ///
    /// Relevance reflects only the caller's explicit query and never represents
    /// global Pod quality, trust, popularity, or personalized authority.
    /// Requires no User account. Does not retain product analytics.
    ///
    /// # Errors
    ///
    /// Returns a typed [`IndexSearchFailure`] when Index is disabled, the query
    /// is oversized/malformed, rate-limited, or local state is unavailable.
    pub fn search_pod_announcements(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<PodAnnouncementSearchResponse, AgentToolsError> {
        self.search_pod_announcements_at(&IndexSearchRequest::new(query, Some(limit)), Utc::now())
    }

    /// Index catalog search at an explicit clock time (tests / deterministic clocks).
    ///
    /// # Errors
    ///
    /// Same as [`Self::search_pod_announcements`].
    pub fn search_pod_announcements_at(
        &self,
        request: &IndexSearchRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodAnnouncementSearchResponse, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let response = search_index_catalog(&mut store, request, self.index.enabled, now)?;
        // Persist rate-limit timestamps only (no query text / User id).
        self.persist_locked(&mut store)?;
        Ok(response)
    }

    /// Accepts verified results fetched from one configured optional Index Node.
    ///
    /// The Index Node's relevance is discarded; Explore recomputes ordering
    /// under the User's local Trust Policy. Provenance records the Index URL.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the Index Node is not in
    /// the User's Trust Policy, any announcement is invalid, or persistence fails.
    pub fn accept_index_search_results(
        &self,
        ctx: &AuthContext,
        index_base_url: &str,
        response: PodAnnouncementSearchResponse,
    ) -> Result<usize, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Index Node results require an authenticated User".into())
        })?;
        let index_base_url = normalized_url(validate_hub_base_url(index_base_url, "base_url")?);
        let policy = store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .ok_or_else(|| StoreError::Validation("Index Node is not configured".into()))?;
        if !policy.retains_index_url(&index_base_url) {
            return Err(StoreError::Validation("Index Node is not configured".into()).into());
        }
        let retained =
            retain_index_search_results(&mut store, &index_base_url, response, Utc::now())?;
        self.persist_locked(&mut store)?;
        Ok(retained)
    }

    /// Explicit Explore path: query configured Index Nodes with the User-authored
    /// query string only, verify/import results, then rank locally.
    ///
    /// Never called from Taste Profile, Source Affinity, Subscription, feedback,
    /// or Discovery Plan inference. Empty queries skip remote Index contact.
    /// Remote ordering is discarded; local Trust Policy and similarity recompute
    /// the Explore result order.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, no User is authenticated,
    /// the request is out of range, or local state is unavailable. Per-Index
    /// transport failures are recorded on the import report without failing the
    /// whole Explore when at least local ranking remains possible.
    pub fn explore_public_pods_with_indexes(
        &self,
        ctx: &AuthContext,
        request: ExploreRequest,
        client: &dyn IndexSearchClient,
    ) -> Result<ExploreResponse, AgentToolsError> {
        if !(1..=50).contains(&request.limit) || request.sample_size > 10 {
            return Err(ExploreRequestError.into());
        }
        // Import remote hits under a short write section, then rank from store.
        {
            let mut store = self
                .store
                .write()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
            let user_id = ctx.user_id.ok_or_else(|| {
                StoreError::Validation("Explore requires an authenticated User".into())
            })?;
            let policy = store
                .trust_policies
                .get(&(user_id, ctx.tenant_id))
                .cloned()
                .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
            let now = Utc::now();
            import_from_configured_indexes(
                &mut store,
                &policy,
                &request.query,
                request.limit,
                client,
                now,
            )?;
            self.persist_locked(&mut store)?;
        }
        self.explore_public_pods(ctx, request)
    }

    /// Queries configured Index Nodes for an explicit Explore query without ranking.
    ///
    /// # Errors
    ///
    /// Same authorization and Trust Policy rules as
    /// [`Self::accept_index_search_results`].
    pub fn import_explicit_index_search(
        &self,
        ctx: &AuthContext,
        query: &str,
        limit: usize,
        client: &dyn IndexSearchClient,
    ) -> Result<IndexExploreImportReport, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Index Node search requires an authenticated User".into())
        })?;
        let policy = store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .cloned()
            .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
        let report =
            import_from_configured_indexes(&mut store, &policy, query, limit, client, Utc::now())?;
        self.persist_locked(&mut store)?;
        Ok(report)
    }

    /// Retains Origin-signed remote samples for a known current announcement.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the announcement is unknown
    /// or stale, the Origin signature is invalid, the artifact exceeds ten
    /// samples, or persistence fails.
    pub fn accept_pod_explore_samples(
        &self,
        ctx: &AuthContext,
        samples: PodExploreSamples,
    ) -> Result<PodExploreSamples, AgentToolsError> {
        if samples.samples.len() > 10 {
            return Err(StoreError::Validation(
                "Pod Explore sample artifact must not exceed 10 references".into(),
            )
            .into());
        }
        if !samples.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let known = store
            .known_pod_announcements
            .get(&(samples.origin_node_id, samples.pod_slug.clone()))
            .ok_or_else(|| StoreError::NotFound("current Pod Announcement".into()))?;
        verify_explore_samples_for_announcement(&samples, &known.announcement)?;
        store
            .pod_explore_sample_sets
            .insert(samples.announcement_id, samples.clone());
        self.persist_locked(&mut store)?;
        Ok(samples)
    }

    /// Retrieves bounded Explore samples from the canonical Origin and retains
    /// them only when signature and current announcement binding verify.
    ///
    /// The injectable client performs the outbound Origin fetch. Requests carry
    /// only announcement identity and a sample limit—never private interests.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the announcement is unknown
    /// or ineligible, the client fails, verification fails, or persistence fails.
    pub fn fetch_origin_explore_samples(
        &self,
        ctx: &AuthContext,
        origin_node_id: NodeIdentityId,
        pod_slug: &str,
        limit: usize,
        client: &dyn OriginExploreSampleClient,
    ) -> Result<PodExploreSamples, AgentToolsError> {
        let announcement = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
            let known = store
                .known_pod_announcements
                .get(&(origin_node_id, pod_slug.to_string()))
                .ok_or_else(|| StoreError::NotFound("current Pod Announcement".into()))?;
            let now = Utc::now();
            if !announcement_is_discovery_eligible(&store, &known.announcement, now) {
                return Err(StoreError::Validation(
                    "Pod Announcement is not discovery-eligible".into(),
                )
                .into());
            }
            if !known.announcement.verify()? {
                return Err(StoreError::InvalidSignature.into());
            }
            known.announcement.clone()
        };
        // Fetch outside the store lock; never attach private matching context.
        let samples = fetch_verified_origin_explore_samples(client, &announcement, limit)?;
        self.accept_pod_explore_samples(ctx, samples)
    }

    /// Signs an optional recommendation from one locally curated public Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when curation is denied, either Pod identity is invalid,
    /// the reason is empty, signing fails, or persistence fails.
    pub fn endorse_public_pod(
        &self,
        ctx: &AuthContext,
        endorsing: &PodAnnouncement,
        endorsed: &PodAnnouncement,
        reason: String,
    ) -> Result<PodEndorsement, AgentToolsError> {
        if reason.trim().is_empty() {
            return Err(
                StoreError::Validation("Pod Endorsement reason must not be empty".into()).into(),
            );
        }
        if !endorsing.verify()? || !endorsed.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let endorsing_pod = store
            .pod_by_slug(&endorsing.pod_slug, ctx.tenant_id)?
            .clone();
        authorize_local_pod_curation(&store, ctx, endorsing_pod.id)?;
        if endorsing_pod.visibility != Visibility::Public {
            return Err(
                StoreError::Validation("only a public Pod can endorse another Pod".into()).into(),
            );
        }
        let node = store.node_for_tenant(ctx.tenant_id)?;
        if endorsing_pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node.id)
        {
            return Err(StoreError::Validation(
                "only an Origin Node can sign its Pod Endorsement".into(),
            )
            .into());
        }
        if endorsing.origin_node_id != node.id || endorsing.pod_slug != endorsing_pod.slug {
            return Err(StoreError::Validation(
                "endorsing announcement does not identify the local public Pod".into(),
            )
            .into());
        }
        let endorsement = sign_pod_endorsement(
            &node,
            PodEndorsement {
                id: Uuid::now_v7(),
                endorsing_node_id: node.id,
                signer: NodeInfo {
                    node_id: node.id,
                    display_name: node.display_name.clone(),
                    public_key: node.public_key.clone(),
                    supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
                },
                endorsing_pod_slug: endorsing_pod.slug,
                endorsing_announcement_id: endorsing.id,
                endorsed_node_id: endorsed.origin_node_id,
                endorsed_pod_slug: endorsed.pod_slug.clone(),
                endorsed_announcement_id: endorsed.id,
                reason: reason.trim().to_string(),
                endorsed_at: Utc::now(),
                signature: String::new(),
            },
        )?;
        store
            .pod_endorsements
            .insert(endorsement.id, endorsement.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::EndorsePublicPod,
            Some(endorsing_pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(endorsement)
    }

    /// Aggregates a valid Pod Endorsement without treating it as authority.
    ///
    /// # Errors
    ///
    /// Returns an error when the endorsement signature is invalid or persistence fails.
    pub fn index_pod_endorsement(
        &self,
        endorsement: PodEndorsement,
    ) -> Result<PodEndorsement, AgentToolsError> {
        if !endorsement.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let endorsing_is_known = store
            .known_pod_announcements
            .get(&(
                endorsement.endorsing_node_id,
                endorsement.endorsing_pod_slug.clone(),
            ))
            .is_some_and(|known| {
                known.announcement.id == endorsement.endorsing_announcement_id
                    && known.announcement.signer.public_key == endorsement.signer.public_key
            });
        let endorsed_is_known = store
            .known_pod_announcements
            .get(&(
                endorsement.endorsed_node_id,
                endorsement.endorsed_pod_slug.clone(),
            ))
            .is_some_and(|known| known.announcement.id == endorsement.endorsed_announcement_id);
        if !endorsing_is_known || !endorsed_is_known {
            return Err(StoreError::Validation(
                "Pod Endorsement must bind two known current Pod Announcements".into(),
            )
            .into());
        }
        if store
            .pod_endorsements
            .get(&endorsement.id)
            .is_some_and(|existing| existing != &endorsement)
        {
            return Err(
                StoreError::Duplicate(format!("Pod Endorsement {}", endorsement.id)).into(),
            );
        }
        store
            .pod_endorsements
            .insert(endorsement.id, endorsement.clone());
        self.persist_locked(&mut store)?;
        Ok(endorsement)
    }

    /// Intentionally discovers public Pods under the User's local Trust Policy.
    ///
    /// Explore does not create Subscriptions. Deterministic Pod Similarity uses
    /// verified public subject/context text, source neighborhoods, Explore
    /// samples, and optional Endorsements. Scoring is local from synchronized
    /// metadata and private evidence; it never issues interest-derived remote
    /// queries. Endorsements strengthen evidence but are never mandatory and
    /// never become transferable trust or global reputation.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, no User is authenticated,
    /// the request is out of range, or local state is unavailable.
    pub fn explore_public_pods(
        &self,
        ctx: &AuthContext,
        request: ExploreRequest,
    ) -> Result<ExploreResponse, AgentToolsError> {
        if !(1..=50).contains(&request.limit) || request.sample_size > 10 {
            return Err(ExploreRequestError.into());
        }
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Explore requires an authenticated User".into())
        })?;
        let policy = store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .cloned()
            .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
        let local_node_id = store.node_for_tenant(ctx.tenant_id)?.id;
        let query = request.query.trim().to_lowercase();
        let preferences = store.user_preferences.get(&(user_id, ctx.tenant_id));
        let projections = taste_profile_projections(&store, user_id, ctx.tenant_id, preferences);
        // Private evidence stays in-process for ranking only; never sent remotely.
        let local =
            local_similarity_context_from_store(Some(query.as_str()), preferences, &projections);
        let now = Utc::now();

        // Materialize owned candidate evidence so ranking can borrow it safely.
        let mut owned: Vec<OwnedCandidateEvidence> = Vec::new();
        for known in store.known_pod_announcements.values() {
            let announcement = &known.announcement;
            if !announcement_scoring_eligible(&store, known, &policy, now) {
                continue;
            }
            let endorsements = collect_policy_endorsements(&store, announcement, &policy);
            let (samples, samples_verified) = retained_or_local_explore_samples(
                &store,
                announcement,
                &policy,
                ctx.tenant_id,
                local_node_id,
                request.sample_size,
            );
            let context_text = store
                .pods
                .values()
                .find(|pod| {
                    pod.slug == announcement.pod_slug
                        && pod.origin_node_id.unwrap_or(local_node_id)
                            == announcement.origin_node_id
                })
                .and_then(|pod| store.pod_skill_packs.get(&pod.id))
                .map(|package| package.context_md.clone());
            owned.push(OwnedCandidateEvidence {
                announcement: announcement.clone(),
                context_text,
                samples,
                endorsements,
                samples_verified,
            });
        }

        let candidates = owned
            .iter()
            .map(OwnedCandidateEvidence::as_evidence)
            .collect();
        let caps = ExplorationCaps::explore_defaults();
        let ranked = rank_similar_pods(&local, candidates, &policy, caps, request.limit);
        let results = ranked
            .into_iter()
            .map(|ranked| {
                let is_subscribed = store.subscriptions.values().any(|subscription| {
                    subscription.user_id == user_id
                        && subscription.tenant_id == ctx.tenant_id
                        && subscription.origin_node_id == ranked.announcement.origin_node_id
                        && subscription.pod_slug == ranked.announcement.pod_slug
                });
                let mut samples = ranked.samples;
                samples.truncate(request.sample_size);
                let mut reasons = ranked
                    .similarity
                    .reasons
                    .iter()
                    .map(crate::pod_similarity::SimilarityReason::display)
                    .collect::<Vec<_>>();
                // Trial is a typed flag; label once at the Explore DTO boundary.
                append_trial_exposure_label(&mut reasons, ranked.similarity.trial_exposure);
                ExplorePodResult {
                    announcement: ranked.announcement,
                    relevance: ranked.similarity.score,
                    reasons,
                    endorsements: ranked.endorsements,
                    sample_content_references: samples,
                    is_subscribed,
                    trial_exposure: ranked.similarity.trial_exposure,
                }
            })
            .collect();
        Ok(ExploreResponse { query, results })
    }

    pub fn register_hub_node(
        &self,
        request: HubRegisterNodeRequest,
    ) -> Result<HubRegisteredNode, AgentToolsError> {
        validate_protocol_version(&request.protocol_version)?;
        let base_url = validate_hub_base_url(&request.base_url, "base_url")?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let now = Utc::now();
        let registered = HubRegisteredNode {
            node_id: request.node_id,
            display_name: request.display_name,
            base_url: normalized_url(base_url),
            public_key: request.public_key,
            protocol_version: request.protocol_version,
            registered_at: store
                .hub_nodes
                .get(&request.node_id)
                .map(|node| node.registered_at)
                .unwrap_or(now),
            last_seen_at: now,
        };
        store
            .hub_nodes
            .insert(registered.node_id, registered.clone());
        self.persist_locked(&mut store)?;
        Ok(registered)
    }

    pub fn list_hub_nodes(&self) -> Result<Vec<HubRegisteredNode>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        Ok(store.hub_nodes.values().cloned().collect())
    }

    pub fn register_hub_pod(
        &self,
        request: HubRegisterPodRequest,
    ) -> Result<HubRegisteredPod, AgentToolsError> {
        let node_base_url = validate_hub_base_url(&request.node_base_url, "node_base_url")?;
        let manifest_url =
            validate_hub_endpoint_url(&request.manifest_url, "manifest_url", &node_base_url)?;
        let events_url =
            validate_hub_endpoint_url(&request.events_url, "events_url", &node_base_url)?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if !store.hub_nodes.contains_key(&request.node_id) {
            return Err(StoreError::Validation(format!(
                "hub pod node_id {} is not registered",
                request.node_id
            ))
            .into());
        }
        let now = Utc::now();
        let key = (request.node_id, request.pod_slug.clone());
        let registered_at = store
            .hub_pods
            .get(&key)
            .map(|pod| pod.registered_at)
            .unwrap_or(now);
        let pod = HubRegisteredPod {
            id: store
                .hub_pods
                .get(&key)
                .map(|pod| pod.id)
                .unwrap_or_else(Uuid::now_v7),
            node_id: request.node_id,
            node_base_url: normalized_url(node_base_url),
            pod_slug: request.pod_slug,
            pod_name: request.pod_name,
            description: request.description,
            tags: normalize_unique(request.tags),
            skill_pack_version: request.skill_pack_version,
            latest_event_hash: request.latest_event_hash,
            manifest_url: normalized_url(manifest_url),
            events_url: normalized_url(events_url),
            registered_at,
            updated_at: now,
        };
        store.hub_pods.insert(key, pod.clone());
        self.persist_locked(&mut store)?;
        Ok(pod)
    }

    pub fn index_local_public_pods_in_hub(
        &self,
        ctx: &AuthContext,
        base_url: &str,
    ) -> Result<Vec<HubRegisteredPod>, AgentToolsError> {
        let node_base_url = validate_hub_base_url(base_url, "base_url")?;
        let normalized_base_url = normalized_url(node_base_url.clone());
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let now = Utc::now();
        let registered_node = HubRegisteredNode {
            node_id: node.id,
            display_name: node.display_name.clone(),
            base_url: normalized_base_url.clone(),
            public_key: node.public_key.clone(),
            protocol_version: CURRENT_PROTOCOL_VERSION.to_string(),
            registered_at: store
                .hub_nodes
                .get(&node.id)
                .map(|node| node.registered_at)
                .unwrap_or(now),
            last_seen_at: now,
        };
        store.hub_nodes.insert(node.id, registered_node);

        let public_pods = store
            .pods
            .values()
            .filter(|pod| {
                (pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
                    && pod.visibility == Visibility::Public
                    && pod
                        .origin_node_id
                        .is_none_or(|origin_node_id| origin_node_id == node.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut indexed = Vec::new();
        for pod in public_pods {
            let Some(pack) = store.pod_skill_packs.get(&pod.id) else {
                continue;
            };
            let manifest_url = validate_hub_endpoint_url(
                &format!(
                    "{}/federation/pods/{}/manifest",
                    normalized_base_url, pod.slug
                ),
                "manifest_url",
                &node_base_url,
            )?;
            let events_url = validate_hub_endpoint_url(
                &format!(
                    "{}/federation/pods/{}/events",
                    normalized_base_url, pod.slug
                ),
                "events_url",
                &node_base_url,
            )?;
            let key = (node.id, pod.slug.clone());
            let registered_at = store
                .hub_pods
                .get(&key)
                .map(|pod| pod.registered_at)
                .unwrap_or(now);
            let registered_pod = HubRegisteredPod {
                id: store
                    .hub_pods
                    .get(&key)
                    .map(|pod| pod.id)
                    .unwrap_or_else(Uuid::now_v7),
                node_id: node.id,
                node_base_url: normalized_base_url.clone(),
                pod_slug: pod.slug.clone(),
                pod_name: pod.name.clone(),
                description: pod.description.clone(),
                tags: route_tokens(&format!("{} {} {}", pod.slug, pod.name, pod.description)),
                skill_pack_version: pack.version,
                latest_event_hash: store.latest_event_hash(&pod.slug),
                manifest_url: normalized_url(manifest_url),
                events_url: normalized_url(events_url),
                registered_at,
                updated_at: now,
            };
            store.hub_pods.insert(key, registered_pod.clone());
            indexed.push(registered_pod);
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::IndexPublicPods,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(indexed)
    }

    pub fn search_hub_pods(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<HubSearchPodsResponse, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let query = query.trim().to_lowercase();
        let query_tokens = route_tokens(&query);
        let mut results = store
            .hub_pods
            .values()
            .filter_map(|pod| score_hub_pod(pod, &query_tokens))
            .collect::<Vec<_>>();
        results.sort_by(|a, b| b.score.total_cmp(&a.score));
        results.truncate(limit.clamp(1, 50));
        Ok(HubSearchPodsResponse { query, results })
    }

    pub fn pod_discovery_feed(
        &self,
        ctx: &AuthContext,
        base_url: &str,
        query: &str,
        limit: usize,
    ) -> Result<PodDiscoveryFeedResponse, AgentToolsError> {
        self.index_local_public_pods_in_hub(ctx, base_url)?;
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let query = query.trim().to_lowercase();
        let query_tokens = route_tokens(&query);
        let mut local_public_pods = Vec::new();
        let mut global_public_pods = Vec::new();
        for pod in store.hub_pods.values() {
            let scope = if pod.node_id == node.id {
                PodDiscoveryScope::Local
            } else {
                PodDiscoveryScope::Global
            };
            let Some(item) = discovery_feed_item(pod, scope, &query_tokens) else {
                continue;
            };
            match item.scope {
                PodDiscoveryScope::Local => local_public_pods.push(item),
                PodDiscoveryScope::Global => global_public_pods.push(item),
            }
        }
        sort_discovery_feed_items(&mut local_public_pods);
        sort_discovery_feed_items(&mut global_public_pods);
        let limit = limit.clamp(1, 50);
        local_public_pods.truncate(limit);
        global_public_pods.truncate(limit);
        Ok(PodDiscoveryFeedResponse {
            query,
            local_public_pods,
            global_public_pods,
            private_interests_exported: false,
        })
    }

    pub fn discover_public_pods_for_home(
        &self,
        ctx: &AuthContext,
        topics: Vec<String>,
        limit: usize,
    ) -> Result<HomePublicPodDiscoveryResponse, AgentToolsError> {
        let mut effective_topics = normalize_unique(topics);
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        if effective_topics.is_empty() {
            if let Some(user_id) = ctx.user_id {
                if let Some(prefs) = store.user_preferences.get(&(user_id, ctx.tenant_id)) {
                    effective_topics = prefs.interests.clone();
                }
            }
        }
        let query = effective_topics.join(" ");
        let route_request = RouteLinkRequest {
            url: String::new(),
            title: Some(query.clone()),
            summary: Some(query.clone()),
            tags: effective_topics.clone(),
        };
        // This is a public discovery surface. Routing scores every pod the caller
        // can see (including their own private pods), so restrict the results to
        // public slugs before returning them.
        let public_slugs: std::collections::HashSet<String> = store
            .pods
            .values()
            .filter(|pod| {
                (pod.tenant_id == ctx.tenant_id || pod.tenant_id.is_none())
                    && pod.visibility == Visibility::Public
            })
            .map(|pod| pod.slug.clone())
            .collect();
        drop(store);
        let local_public_pods = self
            .route_link_to_pods(ctx, route_request, 0.0)?
            .candidates
            .into_iter()
            .filter(|candidate| candidate.score > 0.0 && public_slugs.contains(&candidate.pod_slug))
            .take(limit.clamp(1, 25))
            .collect();
        let hub_results = self.search_hub_pods(&query, limit)?.results;
        Ok(HomePublicPodDiscoveryResponse {
            topics: effective_topics,
            local_public_pods,
            hub_results,
            private_interests_exported: false,
        })
    }

    pub fn export_pod_events(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<Vec<EventLog>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        Ok(store.public_events_for_pod(&pod.slug))
    }

    pub fn import_pod_events(
        &self,
        ctx: &AuthContext,
        peer_id: PeerId,
        events: Vec<EventLog>,
    ) -> Result<usize, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let peer = store
            .trusted_peers
            .get(&peer_id)
            .cloned()
            .ok_or(StoreError::UntrustedPeer)?;
        if !peer.enabled {
            return Err(StoreError::UntrustedPeer.into());
        }
        let mut imported = 0;
        for mut event in events {
            if store.event_log.iter().any(|existing| {
                existing.event_id == event.event_id || existing.content_hash == event.content_hash
            }) {
                continue;
            }
            if !verify_event(&event, &peer.public_key)? {
                return Err(StoreError::InvalidSignature.into());
            }
            event.imported_from_peer_id = Some(peer_id);
            event.verified = true;
            event.tenant_id = ctx.tenant_id;
            project_imported_public_event(&mut store, ctx, &event)?;
            store.event_log.push(event);
            imported += 1;
        }
        if imported > 0 {
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::ImportPodEvents,
                None,
            );
            self.persist_locked(&mut store)?;
        }
        Ok(imported)
    }

    pub fn import_public_events_from_hub_node(
        &self,
        ctx: &AuthContext,
        node_id: NodeIdentityId,
        events: Vec<EventLog>,
    ) -> Result<usize, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let node = store
            .hub_nodes
            .get(&node_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("hub node {node_id}")))?;
        let mut imported = 0;
        for mut event in events {
            if event.author_node_id != node_id || crate::store::is_private_event(&event.event_type)
            {
                continue;
            }
            if store.event_log.iter().any(|existing| {
                existing.event_id == event.event_id || existing.content_hash == event.content_hash
            }) {
                continue;
            }
            if !verify_event(&event, &node.public_key)? {
                return Err(StoreError::InvalidSignature.into());
            }
            event.imported_from_peer_id = None;
            event.verified = true;
            event.tenant_id = ctx.tenant_id;
            project_imported_public_event(&mut store, ctx, &event)?;
            store.event_log.push(event);
            imported += 1;
        }
        if imported > 0 {
            record_harness_write(
                &mut store,
                ctx,
                HarnessWriteOperation::ImportPodEvents,
                None,
            );
            self.persist_locked(&mut store)?;
        }
        Ok(imported)
    }
}

fn task_with_expired_lease_recorded(
    mut task: DiscoveryTask,
    now: chrono::DateTime<Utc>,
) -> DiscoveryTask {
    record_expired_lease(&mut task, now);
    task
}

fn record_expired_lease(task: &mut DiscoveryTask, now: chrono::DateTime<Utc>) {
    let DiscoveryTaskState::Leased(lease) = &task.state else {
        return;
    };
    if lease.expires_at > now {
        return;
    }
    let lease = lease.clone();
    task.attempts.push(DiscoveryTaskAttempt {
        harness_id: lease.harness_id,
        started_at: lease.claimed_at,
        finished_at: lease.expires_at,
        outcome: DiscoveryTaskAttemptOutcome::LeaseExpired,
    });
    task.state = if task.attempts.len() >= MAX_DISCOVERY_TASK_ATTEMPTS {
        DiscoveryTaskState::TerminalFailure
    } else {
        DiscoveryTaskState::Pending
    };
}

pub fn canonicalize_url(value: &str) -> Result<String, AgentToolsError> {
    canonicalize_url_spelling(value).map_err(|error| AgentToolsError::BadUrl(error.to_string()))
}

fn canonicalize_candidate_evidence_url(value: &str) -> Result<String, AgentToolsError> {
    let parsed = Url::parse(value).map_err(|error| AgentToolsError::BadUrl(error.to_string()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AgentToolsError::BadUrl(
            "Candidate evidence URLs must not contain credentials".into(),
        ));
    }
    canonicalize_url(value)
}

fn discard_replayed_events(
    store: &InMemoryStore,
    cursor: Option<&str>,
    snapshot: &mut FederationPodSnapshot,
) -> Result<(), AgentToolsError> {
    let mut previous_hash = snapshot
        .events
        .first()
        .and_then(|event| event.previous_event_hash.clone());
    for event in &snapshot.events {
        if event.author_node_id != snapshot.node.node_id
            || event.pod_slug != snapshot.manifest.pod.slug
            || event.previous_event_hash != previous_hash
            || !is_subscription_projection_event(&event.event_type)
            || !verify_event(event, &snapshot.node.public_key)?
        {
            return Err(StoreError::InvalidSignature.into());
        }
        previous_hash = Some(event.content_hash.clone());
    }
    let Some(cursor) = cursor else {
        return Ok(());
    };
    if snapshot
        .events
        .first()
        .is_none_or(|event| event.previous_event_hash.as_deref() == Some(cursor))
    {
        return Ok(());
    }
    if let Some(cursor_index) = snapshot
        .events
        .iter()
        .position(|event| event.content_hash == cursor)
    {
        snapshot.events.drain(..=cursor_index);
        return Ok(());
    }
    let is_complete_retry = snapshot
        .events
        .last()
        .is_some_and(|event| event.content_hash == cursor)
        && snapshot.events.iter().all(|event| {
            store.event_log.iter().any(|existing| {
                existing.event_id == event.event_id && existing.content_hash == event.content_hash
            })
        });
    if is_complete_retry {
        snapshot.events.clear();
        return Ok(());
    }
    Err(StoreError::Validation("signed Pod Event chain is discontinuous".to_string()).into())
}

/// Validates and canonicalizes a direct public Pod address before outbound I/O.
///
/// # Errors
///
/// Returns an error unless the address uses HTTPS (or loopback HTTP) and has
/// the canonical `/federation/pods/<slug>` shape.
pub fn canonical_public_pod_url(value: &str) -> Result<String, AgentToolsError> {
    crate::pod_announcement::canonical_public_pod_url(value).map_err(Into::into)
}

fn validate_federation_snapshot(
    store: &InMemoryStore,
    tenant_id: Option<TenantId>,
    expected_previous_hash: Option<&str>,
    snapshot: &FederationPodSnapshot,
) -> Result<(), AgentToolsError> {
    let pod = &snapshot.manifest.pod;
    validate_protocol_version(&snapshot.node.supported_protocol_version)?;
    if pod.visibility != Visibility::Public || pod.origin_node_id != Some(snapshot.node.node_id) {
        return Err(StoreError::Validation(
            "federation snapshot does not describe an authoritative public Pod".to_string(),
        )
        .into());
    }
    validate_remote_pod_identity(store, tenant_id, snapshot)?;
    let mut previous_hash = expected_previous_hash.map(str::to_string).or_else(|| {
        snapshot
            .events
            .first()
            .filter(|event| event.event_type == "pod_published")
            .and_then(|event| event.previous_event_hash.clone())
    });
    for event in &snapshot.events {
        if event.pod_slug != pod.slug
            || event.author_node_id != snapshot.node.node_id
            || !is_subscription_projection_event(&event.event_type)
        {
            return Err(StoreError::Validation(
                "event is outside the subscribed public Pod stream".to_string(),
            )
            .into());
        }
        if event.previous_event_hash != previous_hash {
            return Err(StoreError::Validation(
                "signed Pod Event chain is discontinuous".to_string(),
            )
            .into());
        }
        if !verify_event(event, &snapshot.node.public_key)? {
            return Err(StoreError::InvalidSignature.into());
        }
        validate_imported_event_payload(event)?;
        previous_hash = Some(event.content_hash.clone());
    }
    if previous_hash != snapshot.manifest.latest_known_event_hash {
        return Err(StoreError::Validation(
            "federation snapshot does not reach the manifest event pointer".to_string(),
        )
        .into());
    }

    let signed_packages = snapshot
        .events
        .iter()
        .filter_map(|event| event.payload_json.get("package"))
        .map(|value| serde_json::from_value::<PodPackage>(value.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StoreError::Validation("signed Pod Package is malformed".to_string()))?;
    validate_signed_package_versions(store, tenant_id, snapshot, &signed_packages)?;
    Ok(())
}

fn validate_remote_pod_identity(
    store: &InMemoryStore,
    tenant_id: Option<TenantId>,
    snapshot: &FederationPodSnapshot,
) -> Result<(), AgentToolsError> {
    let remote = &snapshot.manifest.pod;
    let origin_node_id = snapshot.node.node_id;
    if store.pods.values().any(|local| {
        local.tenant_id == tenant_id
            && local.slug == remote.slug
            && local.origin_node_id != Some(origin_node_id)
    }) {
        return Err(StoreError::Duplicate(format!(
            "local Pod slug {} conflicts with the subscribed Origin",
            remote.slug
        ))
        .into());
    }
    if store.pods.get(&remote.id).is_some_and(|local| {
        local.tenant_id != tenant_id
            || local.slug != remote.slug
            || local.origin_node_id != Some(origin_node_id)
    }) {
        return Err(StoreError::Duplicate(format!(
            "Origin Pod identity {} conflicts with local state",
            remote.id
        ))
        .into());
    }
    Ok(())
}

fn validate_signed_package_versions(
    store: &InMemoryStore,
    tenant_id: Option<TenantId>,
    snapshot: &FederationPodSnapshot,
    signed_packages: &[PodPackage],
) -> Result<(), AgentToolsError> {
    let remote_pod = &snapshot.manifest.pod;
    let local_pod = store.pods.values().find(|local| {
        local.tenant_id == tenant_id
            && local.slug == remote_pod.slug
            && local.origin_node_id == Some(snapshot.node.node_id)
    });
    let local_package = local_pod.and_then(|pod| store.pod_skill_packs.get(&pod.id));
    let mut verified_version = local_package.map(|package| package.version);
    let mut immutable_versions = BTreeMap::new();
    if let Some(package) = local_package {
        immutable_versions.insert(
            package.version,
            normalized_package_value(package, package.pod_id)?,
        );
    }
    let projected_pod_id = local_pod.map_or(remote_pod.id, |pod| pod.id);
    for package in signed_packages {
        PackageVersion::new(package.version)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        if package.pod_id != remote_pod.id || !validate_skill_pack(package).valid {
            return Err(StoreError::Validation(
                "signed Pod Package is invalid or belongs to another Pod".to_string(),
            )
            .into());
        }
        if verified_version.is_some_and(|version| package.version < version) {
            return Err(StoreError::Validation(
                "signed Pod Package version cannot move backwards".to_string(),
            )
            .into());
        }
        let value = normalized_package_value(package, projected_pod_id)?;
        if immutable_versions
            .get(&package.version)
            .is_some_and(|existing| existing != &value)
        {
            return Err(StoreError::Validation(
                "signed Pod Package version cannot be reused with different contents".to_string(),
            )
            .into());
        }
        immutable_versions.insert(package.version, value);
        verified_version = Some(package.version);
    }
    PackageVersion::new(snapshot.manifest.skill_pack_version)
        .map_err(|error| StoreError::Validation(error.to_string()))?;
    if verified_version != Some(snapshot.manifest.skill_pack_version) {
        return Err(StoreError::Validation(
            "manifest Pod Package version lacks a matching signed event".to_string(),
        )
        .into());
    }
    Ok(())
}

fn normalized_package_value(
    package: &PodPackage,
    projected_pod_id: PodId,
) -> Result<serde_json::Value, AgentToolsError> {
    let mut package = package.clone();
    package.pod_id = projected_pod_id;
    package.owner_id = None;
    package.proposer_harness_id = None;
    serde_json::to_value(package).map_err(|error| {
        StoreError::Validation(format!("signed Pod Package cannot be compared: {error}")).into()
    })
}

fn validate_imported_event_payload(event: &EventLog) -> Result<(), AgentToolsError> {
    let event_type = FederatedPodEventType::from_wire(&event.event_type)
        .ok_or_else(|| StoreError::Validation("event is not synchronization-safe".to_string()))?;
    match event_type {
        FederatedPodEventType::PodCreated => {
            imported_event_payload::<Pod>(event, "pod")?;
            imported_event_payload::<PodPackage>(event, "package")?;
        }
        FederatedPodEventType::PodPublished => {
            imported_event_payload::<Pod>(event, "pod")?;
            imported_event_payload::<PodPackage>(event, "package")?;
        }
        FederatedPodEventType::PodSkillPackUpdated
        | FederatedPodEventType::PodPackageImported
        | FederatedPodEventType::PodPackageForked => {
            imported_event_payload::<PodPackage>(event, "package")?;
        }
        FederatedPodEventType::ContentItemPlaced => {
            imported_event_payload::<ContentItem>(event, "content_item")?;
            imported_event_payload::<AcceptedPlacementProjection>(event, "accepted_placement")?;
        }
        FederatedPodEventType::ContentItemMetadataUpdated => {
            let payload = imported_event_body::<ContentItemMetadataUpdatedPayload>(event)?;
            resolve_media_for_store(&payload.metadata_update.media_references)?;
        }
        FederatedPodEventType::PlacementTombstoned => {
            imported_event_payload::<PlacementTombstone>(event, "placement_tombstone")?;
        }
        FederatedPodEventType::LegacyLinkRemoved => {
            imported_event_payload::<SubmissionId>(event, "submission_id")?;
        }
        FederatedPodEventType::LegacyLinkSubmitted => {
            return Err(
                StoreError::Validation("event is not synchronization-safe".to_string()).into(),
            )
        }
    }
    Ok(())
}

fn project_snapshot_events(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    events: &[EventLog],
) -> Result<usize, AgentToolsError> {
    let mut imported = 0;
    for event in events {
        if store.event_log.iter().any(|existing| {
            existing.event_id == event.event_id || existing.content_hash == event.content_hash
        }) {
            continue;
        }
        let mut imported_event = event.clone();
        imported_event.tenant_id = ctx.tenant_id;
        imported_event.imported_from_peer_id = None;
        imported_event.verified = true;
        if is_subscription_projection_event(&imported_event.event_type) {
            project_imported_public_event(store, ctx, &imported_event)?;
        }
        store.event_log.push(imported_event);
        imported += 1;
    }
    Ok(imported)
}

fn is_subscription_projection_event(event_type: &str) -> bool {
    matches!(
        FederatedPodEventType::from_wire(event_type),
        Some(
            FederatedPodEventType::PodCreated
                | FederatedPodEventType::PodPublished
                | FederatedPodEventType::PodSkillPackUpdated
                | FederatedPodEventType::PodPackageImported
                | FederatedPodEventType::PodPackageForked
                | FederatedPodEventType::ContentItemPlaced
                | FederatedPodEventType::ContentItemMetadataUpdated
                | FederatedPodEventType::PlacementTombstoned
        )
    )
}

fn project_imported_public_event(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    event: &EventLog,
) -> Result<(), AgentToolsError> {
    let Some(event_type) = FederatedPodEventType::from_wire(&event.event_type) else {
        return Ok(());
    };
    match event_type {
        FederatedPodEventType::PodCreated => {
            let pod = imported_event_payload::<Pod>(event, "pod")?;
            let local_pod_id = project_imported_pod(store, ctx, event.author_node_id, pod)?;
            let mut package = imported_event_payload::<PodPackage>(event, "package")?;
            project_imported_package(store, local_pod_id, &mut package)?;
        }
        FederatedPodEventType::PodPublished => {
            let pod = imported_event_payload::<Pod>(event, "pod")?;
            let local_pod_id = project_imported_pod(store, ctx, event.author_node_id, pod)?;
            let mut package = imported_event_payload::<PodPackage>(event, "package")?;
            project_imported_package(store, local_pod_id, &mut package)?;
        }
        FederatedPodEventType::PodSkillPackUpdated
        | FederatedPodEventType::PodPackageImported
        | FederatedPodEventType::PodPackageForked => {
            let mut package = imported_event_payload::<PodPackage>(event, "package")?;
            let local_pod_id = synchronized_origin_pod_id(store, ctx, event)?;
            project_imported_package(store, local_pod_id, &mut package)?;
        }
        FederatedPodEventType::LegacyLinkSubmitted => {
            let submission = imported_event_payload::<Submission>(event, "submission")?;
            project_imported_submission(store, ctx, event, submission)?;
        }
        FederatedPodEventType::ContentItemPlaced => {
            let content_item = imported_event_payload::<ContentItem>(event, "content_item")?;
            let content_item_id =
                project_imported_submission(store, ctx, event, content_item.into_legacy_record())?;
            let mut projection =
                imported_event_payload::<AcceptedPlacementProjection>(event, "accepted_placement")?;
            let local_pod_id = synchronized_origin_pod_id(store, ctx, event)?;
            projection.content_item_id = content_item_id;
            projection.pod_id = local_pod_id;
            projection.origin_node_id = event.author_node_id;
            store
                .accepted_placement_projections
                .insert((content_item_id, local_pod_id), projection);
        }
        FederatedPodEventType::ContentItemMetadataUpdated => {
            let payload = imported_event_body::<ContentItemMetadataUpdatedPayload>(event)?;
            let update = payload.metadata_update;
            let media_references = resolve_media_for_store(&update.media_references)?;
            let key = FederatedContentItemKey::new(
                ctx.tenant_id,
                event.author_node_id,
                update.content_item_id,
            );
            let local_content_item_id = store
                .federated_content_item_ids
                .get(&key)
                .copied()
                .ok_or_else(|| StoreError::NotFound("synchronized Content Item".into()))?;
            let local_pod_id = synchronized_origin_pod_id(store, ctx, event)?;
            if !store
                .accepted_placement_projections
                .contains_key(&(local_content_item_id, local_pod_id))
            {
                return Err(StoreError::Validation(
                    "metadata update requires a synchronized Accepted Placement".into(),
                )
                .into());
            }
            let item = store
                .submissions
                .get_mut(&Uuid::from(local_content_item_id))
                .ok_or_else(|| StoreError::NotFound("synchronized Content Item".into()))?;
            item.media_references =
                resolve_media_for_store(item.media_references.iter().chain(&media_references))?;
        }
        FederatedPodEventType::PlacementTombstoned => {
            let mut tombstone =
                imported_event_payload::<PlacementTombstone>(event, "placement_tombstone")?;
            if tombstone.origin_placement.origin_node_id != event.author_node_id
                || tombstone.content_reference.content_item_id
                    != tombstone.origin_placement.content_item_id
            {
                return Err(StoreError::Validation(
                    "signed Placement Tombstone does not match its Origin Placement".into(),
                )
                .into());
            }
            let origin_content_item_id = tombstone.origin_placement.content_item_id;
            let key = FederatedContentItemKey::new(
                ctx.tenant_id,
                event.author_node_id,
                origin_content_item_id,
            );
            let Some(local_content_item_id) = store.federated_content_item_ids.get(&key).copied()
            else {
                return Ok(());
            };
            let local_submission_id = Uuid::from(local_content_item_id);
            if let Some(pod_id) = store
                .pods
                .values()
                .find(|pod| {
                    pod.slug == event.pod_slug
                        && pod.tenant_id == ctx.tenant_id
                        && pod.origin_node_id == Some(event.author_node_id)
                })
                .map(|pod| pod.id)
            {
                let existing = store
                    .accepted_placement_projections
                    .get(&(local_content_item_id, pod_id))
                    .ok_or_else(|| {
                        StoreError::Validation(
                            "Placement Tombstone has no matching accepted Origin Placement".into(),
                        )
                    })?;
                let mut expected = tombstone.origin_placement.clone();
                expected.content_item_id = local_content_item_id;
                expected.pod_id = pod_id;
                if existing != &expected {
                    return Err(StoreError::Validation(
                        "Placement Tombstone does not match accepted Origin Placement evidence"
                            .into(),
                    )
                    .into());
                }
                store.submission_pods.retain(|link| {
                    !(link.pod_id == pod_id && link.submission_id == local_submission_id)
                });
                store
                    .accepted_placement_projections
                    .remove(&(local_content_item_id, pod_id));
                tombstone.origin_placement = expected;
                tombstone.content_reference.content_item_id = local_content_item_id;
                let tombstoned_origin_id = origin_placement_identity(&tombstone.origin_placement);
                for placement in store.pod_placements.values_mut().filter(|placement| {
                    placement.content_item_id == Some(local_content_item_id)
                        && placement
                            .origin_placements
                            .iter()
                            .map(origin_placement_identity)
                            .collect::<HashSet<_>>()
                            .contains(&tombstoned_origin_id)
                }) {
                    placement.origin_withdrawals.push(tombstone.clone());
                }
                store.placement_tombstones.push(tombstone);
            }
        }
        FederatedPodEventType::LegacyLinkRemoved => {}
    }
    Ok(())
}

fn synchronized_origin_pod_id(
    store: &InMemoryStore,
    ctx: &AuthContext,
    event: &EventLog,
) -> Result<PodId, AgentToolsError> {
    store
        .pods
        .values()
        .find(|pod| {
            pod.slug == event.pod_slug
                && pod.tenant_id == ctx.tenant_id
                && pod.origin_node_id == Some(event.author_node_id)
        })
        .map(|pod| pod.id)
        .ok_or_else(|| StoreError::NotFound("synchronized public Pod".into()).into())
}

fn imported_event_payload<T: serde::de::DeserializeOwned>(
    event: &EventLog,
    field: &str,
) -> Result<T, AgentToolsError> {
    let value = event.payload_json.get(field).cloned().ok_or_else(|| {
        StoreError::Validation(format!(
            "signed {} event is missing {field}",
            event.event_type
        ))
    })?;
    serde_json::from_value(value).map_err(|error| {
        StoreError::Validation(format!(
            "signed {} event has invalid {field}: {error}",
            event.event_type
        ))
        .into()
    })
}

fn imported_event_body<T: serde::de::DeserializeOwned>(
    event: &EventLog,
) -> Result<T, AgentToolsError> {
    serde_json::from_value(event.payload_json.clone()).map_err(|error| {
        StoreError::Validation(format!(
            "signed {} payload is malformed: {error}",
            event.event_type
        ))
        .into()
    })
}

fn project_imported_package(
    store: &mut InMemoryStore,
    local_pod_id: PodId,
    package: &mut PodPackage,
) -> Result<(), AgentToolsError> {
    package.pod_id = local_pod_id;
    package.owner_id = None;
    package.proposer_harness_id = None;
    let version = PackageVersion::new(package.version)
        .map_err(|error| StoreError::Validation(error.to_string()))?;
    if !validate_skill_pack(package).valid {
        return Err(StoreError::Validation("signed Pod Package is invalid".to_string()).into());
    }
    let package_value = normalized_package_value(package, local_pod_id)?;
    if let Some(current) = store.pod_skill_packs.get(&local_pod_id) {
        if package.version < current.version {
            return Err(StoreError::Validation(
                "signed Pod Package version cannot move backwards".to_string(),
            )
            .into());
        }
        if package.version == current.version
            && normalized_package_value(current, local_pod_id)? != package_value
        {
            return Err(StoreError::Validation(
                "signed Pod Package version cannot be reused with different contents".to_string(),
            )
            .into());
        }
    }
    if let Some(existing) = store.pod_package_versions.get(&(local_pod_id, version)) {
        if normalized_package_value(existing, local_pod_id)? != package_value {
            return Err(StoreError::Validation(
                "signed Pod Package history is immutable".to_string(),
            )
            .into());
        }
    }
    store
        .pod_package_versions
        .entry((local_pod_id, version))
        .or_insert_with(|| package.clone());
    store.pod_skill_packs.insert(local_pod_id, package.clone());
    Ok(())
}

fn project_imported_pod(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    origin_node_id: NodeIdentityId,
    mut pod: Pod,
) -> Result<PodId, AgentToolsError> {
    pod.tenant_id = ctx.tenant_id;
    pod.visibility = Visibility::Public;
    pod.created_by = None;
    pod.origin_node_id = Some(origin_node_id);

    if let Some(existing) = store
        .pods
        .values()
        .find(|existing| {
            existing.slug == pod.slug
                && existing.tenant_id == ctx.tenant_id
                && existing.origin_node_id == Some(origin_node_id)
        })
        .cloned()
    {
        ensure_projected_pod_support(store, &existing);
        return Ok(existing.id);
    }

    if store
        .pods
        .values()
        .any(|existing| existing.slug == pod.slug && existing.tenant_id == ctx.tenant_id)
    {
        return Err(StoreError::Duplicate(format!("Pod slug {}", pod.slug)).into());
    }
    let pod_id = Uuid::now_v7();
    pod.id = pod_id;
    store.pods.insert(pod_id, pod.clone());
    ensure_projected_pod_support(store, &pod);
    Ok(pod_id)
}

fn ensure_projected_pod_support(store: &mut InMemoryStore, pod: &Pod) {
    store.pod_rules.entry(pod.id).or_insert(PodRules {
        pod_id: pod.id,
        blocked_topics: vec![],
        blocked_domains: vec![],
        auto_promote_crawler_candidates: false,
        federate_sources: true,
    });
}

fn project_imported_submission(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    event: &EventLog,
    mut submission: Submission,
) -> Result<ContentItemId, AgentToolsError> {
    let origin_content_item_id = ContentItemId::from(submission.id);
    let pod_id = store
        .pods
        .values()
        .find(|pod| {
            pod.slug == event.pod_slug
                && pod.tenant_id == ctx.tenant_id
                && pod.origin_node_id == Some(event.author_node_id)
        })
        .map(|pod| pod.id)
        .map(Ok)
        .unwrap_or_else(|| {
            let pod = Pod {
                id: Uuid::now_v7(),
                tenant_id: ctx.tenant_id,
                name: event.pod_slug.clone(),
                slug: event.pod_slug.clone(),
                description: "Imported public pod from a federated node.".to_string(),
                visibility: Visibility::Public,
                created_by: None,
                created_at: event.created_at,
                origin_node_id: Some(event.author_node_id),
            };
            project_imported_pod(store, ctx, event.author_node_id, pod)
        })?;

    submission.tenant_id = ctx.tenant_id;
    submission.submitted_by = None;
    submission.origin_event_id = Some(event.event_id);
    let submission_id = store
        .submissions
        .values()
        .find(|existing| {
            existing.tenant_id == ctx.tenant_id
                && existing.canonical_url == submission.canonical_url
        })
        .map(|existing| existing.id)
        .unwrap_or_else(|| {
            let id = Uuid::now_v7();
            submission.id = id;
            store.submissions.insert(id, submission);
            id
        });

    if !store
        .submission_pods
        .iter()
        .any(|link| link.pod_id == pod_id && link.submission_id == submission_id)
    {
        store.submission_pods.push(SubmissionPod {
            submission_id,
            pod_id,
            created_at: event.created_at,
        });
    }
    let local_content_item_id = ContentItemId::from(submission_id);
    store.federated_content_item_ids.insert(
        FederatedContentItemKey::new(ctx.tenant_id, event.author_node_id, origin_content_item_id),
        local_content_item_id,
    );
    Ok(local_content_item_id)
}

fn validate_protocol_version(value: &str) -> Result<(), AgentToolsError> {
    if value == CURRENT_PROTOCOL_VERSION {
        return Ok(());
    }
    Err(AgentToolsError::IncompatibleProtocol {
        received: value.to_string(),
        supported: CURRENT_PROTOCOL_VERSION,
    })
}

fn validate_hub_base_url(value: &str, field: &str) -> Result<Url, AgentToolsError> {
    let mut url = parse_hub_url(value, field)?;
    url.set_query(None);
    url.set_fragment(None);
    validate_hub_scheme_and_host(&url, field)?;
    Ok(url)
}

fn apply_trust_policy_change(
    policy: &mut TrustPolicy,
    change: &TrustPolicyChange,
) -> Result<(), AgentToolsError> {
    match change {
        TrustPolicyChange::AddIndexNode { label, base_url } => {
            let label = label.trim();
            if label.is_empty() {
                return Err(
                    StoreError::Validation("Index Node label must not be empty".into()).into(),
                );
            }
            let base_url = normalized_url(validate_hub_base_url(base_url, "base_url")?);
            if !policy
                .index_nodes
                .iter()
                .any(|node| node.base_url == base_url)
            {
                policy.index_nodes.push(IndexNode {
                    label: label.to_string(),
                    base_url,
                });
                policy
                    .index_nodes
                    .sort_by(|left, right| left.base_url.cmp(&right.base_url));
            }
        }
        TrustPolicyChange::RemoveIndexNode { base_url } => {
            let base_url = normalized_url(validate_hub_base_url(base_url, "base_url")?);
            let original_len = policy.index_nodes.len();
            policy
                .index_nodes
                .retain(|index| index.base_url != base_url);
            if policy.index_nodes.len() == original_len {
                return Err(StoreError::NotFound(format!("Index Node {base_url}")).into());
            }
        }
        TrustPolicyChange::BlockPod {
            origin_node_id,
            pod_slug,
        } => {
            let pod_slug = pod_slug.trim().to_lowercase();
            if pod_slug.is_empty() {
                return Err(
                    StoreError::Validation("blocked Pod slug must not be empty".into()).into(),
                );
            }
            policy
                .blocked_pods
                .insert(BlockedPod::new(*origin_node_id, pod_slug));
        }
        TrustPolicyChange::BlockNode { node_id } => {
            policy.blocked_nodes.insert(*node_id);
        }
        TrustPolicyChange::BlockSource { source } => {
            insert_normalized_policy_term(&mut policy.blocked_sources, source, "blocked source")?;
        }
        TrustPolicyChange::BlockTopic { topic } => {
            insert_normalized_policy_term(&mut policy.blocked_topics, topic, "blocked topic")?;
        }
    }
    Ok(())
}

fn insert_normalized_policy_term(
    values: &mut std::collections::BTreeSet<String>,
    value: &str,
    field: &str,
) -> Result<(), AgentToolsError> {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return Err(StoreError::Validation(format!("{field} must not be empty")).into());
    }
    values.insert(value);
    Ok(())
}

fn validate_hub_endpoint_url(
    value: &str,
    field: &str,
    node_base_url: &Url,
) -> Result<Url, AgentToolsError> {
    let mut url = parse_hub_url(value, field)?;
    url.set_query(None);
    url.set_fragment(None);
    validate_hub_scheme_and_host(&url, field)?;
    if url.scheme() != node_base_url.scheme()
        || url.host_str() != node_base_url.host_str()
        || url.port_or_known_default() != node_base_url.port_or_known_default()
    {
        return Err(StoreError::Validation(format!(
            "{field} must use the same scheme, host, and port as node_base_url"
        ))
        .into());
    }
    if !url
        .path()
        .starts_with(node_base_url.path().trim_end_matches('/'))
    {
        return Err(StoreError::Validation(format!("{field} must be under node_base_url")).into());
    }
    Ok(url)
}

fn parse_hub_url(value: &str, field: &str) -> Result<Url, AgentToolsError> {
    let url = Url::parse(value)
        .map_err(|error| StoreError::Validation(format!("{field} is not a valid URL: {error}")))?;
    if url.username() != "" || url.password().is_some() {
        return Err(StoreError::Validation(format!("{field} must not include credentials")).into());
    }
    Ok(url)
}

fn validate_hub_scheme_and_host(url: &Url, field: &str) -> Result<(), AgentToolsError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(StoreError::Validation(format!("{field} must use http or https")).into());
    }
    if url.host_str().is_none() {
        return Err(StoreError::Validation(format!("{field} must include a host")).into());
    }
    if !hub_url_is_loopback(url) && url.scheme() != "https" {
        return Err(StoreError::Validation(format!(
            "{field} must use https unless it is loopback-only"
        ))
        .into());
    }
    Ok(())
}

fn hub_url_is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn normalized_url(url: Url) -> String {
    url.to_string().trim_end_matches('/').to_string()
}

fn normalize_unique(values: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for value in values {
        let value = value.trim().to_lowercase();
        if !value.is_empty() && !output.contains(&value) {
            output.push(value);
        }
    }
    output
}

fn normalize_unique_case_insensitive(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.to_lowercase()))
        .collect()
}

fn normalize_source_affinity_signals(
    values: Vec<SourceAffinitySignal>,
) -> Vec<SourceAffinitySignal> {
    let mut output = Vec::new();
    for signal in values {
        let Some(normalized) = signal.normalized() else {
            continue;
        };
        if output
            .iter()
            .any(|existing: &SourceAffinitySignal| existing.eq_ignore_ascii_case(&normalized))
        {
            continue;
        }
        output.push(normalized);
    }
    output
}

fn route_text(request: &RouteLinkRequest) -> String {
    format!(
        "{} {} {} {}",
        request.url,
        request.title.clone().unwrap_or_default(),
        request.summary.clone().unwrap_or_default(),
        request.tags.join(" ")
    )
    .to_lowercase()
}

fn suggest_new_pod_for_link(
    request: &RouteLinkRequest,
    candidates: &[PodRouteCandidate],
    existing_slugs: &HashSet<String>,
) -> CreatePodRequest {
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    let tags = normalize_unique(request.tags.clone());
    let name = tags
        .first()
        .map(|tag| title_case_words(tag))
        .or_else(|| title.map(compact_title_for_pod))
        .or_else(|| domain_label(&request.url).map(|domain| title_case_words(&domain)))
        .unwrap_or_else(|| "New Links".to_string());
    let slug = unique_slug(slugify(&name), existing_slugs);
    let basis = if !tags.is_empty() {
        format!("tagged {}", tags.join(", "))
    } else if let Some(domain) = domain_label(&request.url) {
        format!("from {domain}")
    } else {
        "from submitted links".to_string()
    };
    let description = if let Some(top) = candidates.first() {
        format!(
            "User-approved links {basis}. Suggested because no existing pod cleared the routing threshold; closest match was {} with score {:.1}.",
            top.pod_name, top.score
        )
    } else {
        format!(
            "User-approved links {basis}. Suggested because there are no existing pods to route this link into."
        )
    };
    CreatePodRequest {
        name,
        slug,
        description,
        visibility: Visibility::Private,
    }
}

fn unique_slug(base: String, existing_slugs: &HashSet<String>) -> String {
    if !existing_slugs.contains(&base) {
        return base;
    }
    for idx in 2..=100 {
        let candidate = format!("{base}-{idx}");
        if !existing_slugs.contains(&candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", Uuid::now_v7())
}

fn compact_title_for_pod(title: &str) -> String {
    let words = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(4)
        .collect::<Vec<_>>();
    if words.is_empty() {
        "New Links".to_string()
    } else {
        title_case_words(&words.join(" "))
    }
}

fn domain_label(url: &str) -> Option<String> {
    let domain = Url::parse(url).ok()?.domain()?.to_string();
    let mut parts = domain
        .trim_start_matches("www.")
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() > 1 {
        parts.pop();
    }
    parts.last().map(|part| part.replace('-', " "))
}

fn title_case_words(value: &str) -> String {
    let words = value
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(4)
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().collect::<String>();
                    out.push_str(&chars.as_str().to_lowercase());
                    out
                }
                None => String::new(),
            }
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if words.is_empty() {
        "New Links".to_string()
    } else {
        words.join(" ")
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "new-links".to_string()
    } else {
        slug
    }
}

fn score_pod_route(
    pod: &Pod,
    pack: Option<&PodSkillPack>,
    text: &str,
    tags: &[String],
) -> PodRouteCandidate {
    let mut score = 0.0_f32;
    let mut reasons = Vec::new();
    let tag_text = tags.join(" ").to_lowercase();
    let pod_text = format!("{} {} {}", pod.name, pod.slug, pod.description).to_lowercase();
    for token in route_tokens(&pod_text) {
        if text.contains(&token) || tag_text.contains(&token) {
            score += 1.5;
            if reasons.len() < 4 {
                reasons.push(format!("matched pod term '{token}'"));
            }
        }
    }
    if let Some(pack) = pack {
        let skill_text =
            format!("{} {} {}", pack.skill_md, pack.pod_yaml, pack.filters_yaml).to_lowercase();
        for token in route_tokens(&skill_text) {
            if text.contains(&token) || tag_text.contains(&token) {
                score += 0.4;
                if reasons.len() < 6 {
                    reasons.push(format!("matched skill-pack term '{token}'"));
                }
            }
        }
    }
    let domain_bonus = if text.contains("x.com") || text.contains("twitter.com") {
        if pod.slug.contains("alien") || pod.slug.contains("internet") {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    };
    if domain_bonus > 0.0 {
        score += domain_bonus;
        reasons.push("social link fits this pod's discovery surface".to_string());
    }
    PodRouteCandidate {
        pod_slug: pod.slug.clone(),
        pod_name: pod.name.clone(),
        score,
        reasons,
    }
}

fn score_hub_pod(pod: &HubRegisteredPod, query_tokens: &[String]) -> Option<HubSearchPodResult> {
    let haystack = format!(
        "{} {} {} {}",
        pod.pod_slug,
        pod.pod_name,
        pod.description,
        pod.tags.join(" ")
    )
    .to_lowercase();
    let mut score = 0.0_f32;
    let mut reasons = Vec::new();
    for token in query_tokens {
        if haystack.contains(token) {
            score += if pod.tags.iter().any(|tag| tag.eq_ignore_ascii_case(token)) {
                2.0
            } else {
                1.0
            };
            if reasons.len() < 6 {
                reasons.push(format!("matched public pod term '{token}'"));
            }
        }
    }
    if score <= 0.0 {
        None
    } else {
        Some(HubSearchPodResult {
            pod: pod.clone(),
            score,
            reasons,
        })
    }
}

fn discovery_feed_item(
    pod: &HubRegisteredPod,
    scope: PodDiscoveryScope,
    query_tokens: &[String],
) -> Option<PodDiscoveryFeedItem> {
    if query_tokens.is_empty() {
        return Some(PodDiscoveryFeedItem {
            pod: pod.clone(),
            scope,
            score: 0.0,
            reasons: Vec::new(),
        });
    }
    let scored = score_hub_pod(pod, query_tokens)?;
    Some(PodDiscoveryFeedItem {
        pod: scored.pod,
        scope,
        score: scored.score,
        reasons: scored.reasons,
    })
}

fn sort_discovery_feed_items(items: &mut [PodDiscoveryFeedItem]) {
    items.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.pod.updated_at.cmp(&a.pod.updated_at))
    });
}

fn authorize_harness(
    store: &InMemoryStore,
    ctx: &AuthContext,
    capability: HarnessCapability,
    pod_id: Option<PodId>,
) -> Result<(), AgentToolsError> {
    let Some(harness) = harness_for_context(store, ctx)? else {
        return Ok(());
    };
    if !harness.grant.capabilities.contains(&capability) {
        return Err(AgentToolsError::Forbidden {
            reason: format!("harness grant lacks {capability}"),
        });
    }
    if let (Some(allowed), Some(pod_id)) = (&harness.grant.pod_ids, pod_id) {
        if !allowed.contains(&pod_id) {
            return Err(AgentToolsError::Forbidden {
                reason: format!("harness grant does not include Pod {pod_id}"),
            });
        }
    }
    if capability == HarnessCapability::PodCuration {
        if let (Some(user_id), Some(pod_id)) = (ctx.user_id, pod_id) {
            if !store.pod_roles.iter().any(|assignment| {
                assignment.user_id == user_id
                    && assignment.pod_id == pod_id
                    && matches!(assignment.role, PodRole::Owner | PodRole::Curator)
            }) {
                return Err(AgentToolsError::Forbidden {
                    reason: format!("User has no Pod Role for Pod {pod_id}"),
                });
            }
        }
    }
    Ok(())
}

fn candidate_placement_is_visible(store: &InMemoryStore, ctx: &AuthContext, pod_id: PodId) -> bool {
    authorize_harness(
        store,
        ctx,
        HarnessCapability::CandidateSubmission,
        Some(pod_id),
    )
    .is_ok()
        || authorize_local_pod_curation(store, ctx, pod_id).is_ok()
}

fn candidate_submission_is_visible(
    store: &InMemoryStore,
    ctx: &AuthContext,
    harness: Option<&AgentHarness>,
    submission: &CandidateSubmission,
) -> bool {
    match submission.target {
        CandidateSubmissionTarget::User { user_id, .. } => {
            ctx.user_id == Some(user_id)
                && harness.is_some_and(|harness| {
                    harness.kind == AgentHarnessKind::Interactive
                        && harness.grant.pod_ids.is_none()
                        && harness
                            .grant
                            .capabilities
                            .contains(&HarnessCapability::CandidateSubmission)
                })
        }
        CandidateSubmissionTarget::PodPlacements { ref placements, .. } => {
            !placements.is_empty()
                && placements
                    .iter()
                    .all(|placement| candidate_placement_is_visible(store, ctx, placement.pod_id))
        }
        CandidateSubmissionTarget::PersonalDiscovery {
            user_id, task_id, ..
        } => {
            if authorize_personal_discovery_management(store, ctx).is_ok()
                && ctx.user_id == Some(user_id)
            {
                return true;
            }
            harness.is_some_and(|harness| {
                authorize_personal_discovery_execution(store, ctx).is_ok()
                    && (submission.submitted_by == harness.id
                        || store.discovery_tasks.get(&task_id).is_some_and(|task| {
                            matches!(
                                &task.state,
                                DiscoveryTaskState::Leased(lease)
                                    if lease.harness_id == harness.id
                                        && lease.expires_at > Utc::now()
                            )
                        }))
            })
        }
    }
}

fn agent_harness_view(
    store: &InMemoryStore,
    harness: &AgentHarness,
) -> Result<AgentHarnessView, AgentToolsError> {
    let token_hash = store
        .api_tokens
        .values()
        .find(|token| token.harness_id == Some(harness.id))
        .map(|token| token.token_hash.as_str())
        .ok_or_else(|| {
            StoreError::NotFound(format!("credential for Agent Harness {}", harness.id))
        })?;
    let prefix = &token_hash[..token_hash.len().min(12)];
    Ok(AgentHarnessView {
        harness: harness.clone(),
        credential_fingerprint: format!("sha256:{prefix}"),
        status: if harness.revoked_at.is_some() {
            AgentHarnessStatus::Revoked
        } else {
            AgentHarnessStatus::Active
        },
    })
}

fn authorize_taste_profile(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Result<(), AgentToolsError> {
    authorize_harness(store, ctx, HarnessCapability::Feedback, None)?;
    if let Some(harness) = harness_for_context(store, ctx)? {
        if harness.grant.pod_ids.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "Taste Profile access requires an unscoped feedback grant".into(),
            });
        }
        if harness.kind != AgentHarnessKind::Interactive {
            return Err(AgentToolsError::Forbidden {
                reason: "Taste Profile access requires an interactive User action".into(),
            });
        }
    }
    Ok(())
}

fn authorize_interactive_user_action(
    store: &InMemoryStore,
    ctx: &AuthContext,
    reason: &str,
) -> Result<(), AgentToolsError> {
    if harness_for_context(store, ctx)?
        .is_some_and(|harness| harness.kind != AgentHarnessKind::Interactive)
    {
        return Err(AgentToolsError::Forbidden {
            reason: reason.into(),
        });
    }
    Ok(())
}

fn authorize_feed_item_scope(
    store: &InMemoryStore,
    ctx: &AuthContext,
    content_item_id: ContentItemId,
) -> Result<(), AgentToolsError> {
    let Some(harness) = harness_for_context(store, ctx)? else {
        return Ok(());
    };
    let Some(pod_ids) = &harness.grant.pod_ids else {
        return Ok(());
    };
    if store
        .accepted_placement_projections
        .keys()
        .any(|(item_id, pod_id)| *item_id == content_item_id && pod_ids.contains(pod_id))
    {
        return Ok(());
    }
    Err(AgentToolsError::Forbidden {
        reason: "Harness Grant cannot access this Content Item through an allowed Pod".into(),
    })
}

fn validate_candidate_submission(
    store: &InMemoryStore,
    ctx: &AuthContext,
    request: &CandidateSubmissionRequest,
) -> Result<(), AgentToolsError> {
    let evidence = &request.evidence;
    if evidence.harness_idempotency_key.trim().is_empty()
        || evidence.client_idempotency_key.trim().is_empty()
    {
        return Err(StoreError::Validation(
            "Candidate Submission idempotency keys must not be empty".into(),
        )
        .into());
    }
    if evidence.provenance.discovery_method.trim().is_empty() {
        return Err(StoreError::Validation(
            "Candidate Submission discovery method must not be empty".into(),
        )
        .into());
    }
    let harness =
        harness_for_context(store, ctx)?.ok_or(AgentToolsError::CandidateHarnessRequired)?;
    match &request.target {
        CandidateSubmissionRequestTarget::PersonalDiscovery { .. } => {
            authorize_personal_discovery_execution(store, ctx)?;
        }
        CandidateSubmissionRequestTarget::User { .. }
        | CandidateSubmissionRequestTarget::PodPlacements { .. } => {
            authorize_harness(store, ctx, HarnessCapability::CandidateSubmission, None)?;
        }
    }
    if matches!(
        request.target,
        CandidateSubmissionRequestTarget::User { .. }
    ) && (harness.kind != AgentHarnessKind::Interactive
        || harness.grant.pod_ids.is_some()
        || ctx.user_id.is_none())
    {
        return Err(AgentToolsError::Forbidden {
            reason: "User-targeted Candidate Submission requires an unscoped interactive grant"
                .into(),
        });
    }
    let canonical_source_url = canonicalize_candidate_evidence_url(&evidence.source_url)?;
    if let Some(referrer_url) = &evidence.provenance.referrer_url {
        canonicalize_candidate_evidence_url(referrer_url)?;
    }
    resolve_media_for_store(
        store
            .submissions
            .values()
            .filter(|item| {
                item.tenant_id == ctx.tenant_id && item.canonical_url == canonical_source_url
            })
            .flat_map(|item| &item.media_references)
            .chain(
                store
                    .candidate_submissions
                    .values()
                    .filter(|submission| {
                        store
                            .candidates
                            .get(&submission.candidate_id)
                            .is_some_and(|candidate| {
                                candidate.tenant_id == ctx.tenant_id
                                    && candidate.canonical_url == canonical_source_url
                            })
                            && candidate_submission_is_visible(
                                store,
                                ctx,
                                Some(harness),
                                submission,
                            )
                            && matches!(
                                (&request.target, &submission.target),
                                (
                                    CandidateSubmissionRequestTarget::User { .. },
                                    CandidateSubmissionTarget::User { .. },
                                ) | (
                                    CandidateSubmissionRequestTarget::PodPlacements { .. },
                                    CandidateSubmissionTarget::PodPlacements { .. },
                                ) | (
                                    CandidateSubmissionRequestTarget::PersonalDiscovery { .. },
                                    CandidateSubmissionTarget::PersonalDiscovery { .. },
                                )
                            )
                    })
                    .flat_map(|submission| &submission.evidence.media_references),
            )
            .chain(&evidence.media_references),
    )?;

    let placements = request.target.placements();
    if matches!(
        request.target,
        CandidateSubmissionRequestTarget::PodPlacements { .. }
    ) && placements.is_empty()
    {
        return Err(StoreError::Validation(
            "Pod-targeted Candidate Submission requires at least one placement".into(),
        )
        .into());
    }
    let mut pod_ids = HashSet::with_capacity(placements.len());
    let local_node_id = store.node_for_tenant(ctx.tenant_id)?.id;
    for placement in placements {
        if !pod_ids.insert(placement.pod_id) {
            return Err(StoreError::Validation(
                "Candidate Submission cannot propose the same Pod twice".into(),
            )
            .into());
        }
        if placement.reason.trim().is_empty() {
            return Err(StoreError::Validation(
                "Candidate Placement reason must not be empty".into(),
            )
            .into());
        }
        let pod = store
            .pods
            .get(&placement.pod_id)
            .ok_or_else(|| StoreError::NotFound("Pod".into()))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        if pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != local_node_id)
        {
            return Err(AgentToolsError::Forbidden {
                reason: format!(
                    "Candidate Submission cannot propose remote Pod {} as a local placement",
                    placement.pod_id
                ),
            });
        }
        authorize_harness(
            store,
            ctx,
            HarnessCapability::CandidateSubmission,
            Some(placement.pod_id),
        )?;
    }

    Ok(())
}

fn resolve_media_for_store<'a>(
    references: impl IntoIterator<Item = &'a MediaReference>,
) -> Result<Vec<MediaReference>, AgentToolsError> {
    resolve_media_evidence(references)
        .map_err(|error| StoreError::Validation(error.to_string()).into())
}

fn validate_candidate_task_context(
    store: &InMemoryStore,
    ctx: &AuthContext,
    harness: &AgentHarness,
    request: &CandidateSubmissionRequest,
) -> Result<(), AgentToolsError> {
    if let CandidateSubmissionRequestTarget::PersonalDiscovery { task_id, .. } = &request.target {
        let task = store
            .discovery_tasks
            .get(task_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
        if task.target.discovery_plan_id().is_none() {
            return Err(StoreError::Validation(
                "Personal Discovery result requires a Personal Discovery Task".into(),
            )
            .into());
        }
        authorize_discovery_task(store, ctx, task)?;
        if !matches!(
            &task.state,
            DiscoveryTaskState::Leased(lease)
                if lease.harness_id == harness.id && lease.expires_at > Utc::now()
        ) {
            return Err(AgentToolsError::CandidateTaskLeaseRequired);
        }
        let plan_id = task
            .target
            .discovery_plan_id()
            .expect("Personal target checked above");
        store
            .discovery_plans
            .get(&plan_id)
            .ok_or_else(|| StoreError::NotFound("Discovery Plan".into()))?;
        return Ok(());
    }
    match request.target.task_context() {
        Some(task_context) => {
            let task = store
                .discovery_tasks
                .get(&task_context.task_id)
                .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
            let Some((pod_id, package_version)) = task.target.pod() else {
                return Err(StoreError::Validation(
                    "Pod Candidate Submission cannot use a Personal Discovery Task".into(),
                )
                .into());
            };
            authorize_harness(store, ctx, HarnessCapability::DiscoveryTasks, Some(pod_id))?;
            if package_version != task_context.package_version {
                return Err(AgentToolsError::CandidatePackageVersionMismatch);
            }
            if !request
                .target
                .placements()
                .iter()
                .any(|placement| placement.pod_id == pod_id)
            {
                return Err(StoreError::Validation(
                    "task-driven Candidate Submission must propose its task Pod".into(),
                )
                .into());
            }
            if !matches!(
                &task.state,
                DiscoveryTaskState::Leased(lease)
                    if lease.harness_id == harness.id && lease.expires_at > Utc::now()
            ) {
                return Err(AgentToolsError::CandidateTaskLeaseRequired);
            }
        }
        None if harness.kind == AgentHarnessKind::Unattended => {
            return Err(AgentToolsError::CandidateTaskRequired)
        }
        None => {}
    }
    Ok(())
}

fn idempotent_candidate_submission(
    store: &InMemoryStore,
    harness_id: AgentHarnessId,
    request: &CandidateSubmissionRequest,
) -> Result<Option<CandidateSubmission>, AgentToolsError> {
    let matching_key = store.candidate_submissions.values().find(|submission| {
        submission.submitted_by == harness_id
            && (submission.evidence.harness_idempotency_key
                == request.evidence.harness_idempotency_key
                || submission.evidence.client_idempotency_key
                    == request.evidence.client_idempotency_key)
    });
    let Some(existing) = matching_key else {
        return Ok(None);
    };
    if candidate_submission_matches_request(existing, request) {
        Ok(Some(existing.clone()))
    } else {
        Err(AgentToolsError::CandidateIdempotencyConflict)
    }
}

fn candidate_submission_matches_request(
    submission: &CandidateSubmission,
    request: &CandidateSubmissionRequest,
) -> bool {
    let target_matches = match (&submission.target, &request.target) {
        (
            CandidateSubmissionTarget::User {
                learn: stored_learn,
                interest_seed_metadata: stored_metadata,
                ..
            },
            CandidateSubmissionRequestTarget::User {
                learn: requested_learn,
                interest_seed_metadata: requested_metadata,
            },
        ) => stored_learn == requested_learn && stored_metadata == requested_metadata,
        (
            CandidateSubmissionTarget::PersonalDiscovery {
                task_id: stored_task,
                allocation_role: stored_role,
                source_facts: stored_facts,
                ..
            },
            CandidateSubmissionRequestTarget::PersonalDiscovery {
                task_id: requested_task,
                allocation_role: requested_role,
                source_facts: requested_facts,
            },
        ) => {
            stored_task == requested_task
                && stored_role == requested_role
                && stored_facts == requested_facts
        }
        (
            CandidateSubmissionTarget::PodPlacements {
                placements: stored,
                task_context: stored_task,
            },
            CandidateSubmissionRequestTarget::PodPlacements {
                placements: requested,
                task_context: requested_task,
            },
        ) => stored == requested && stored_task == requested_task,
        _ => false,
    };
    target_matches && submission.evidence == request.evidence
}

fn stable_candidate_uuid(namespace: &str, parts: &[&str]) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(namespace.len().to_be_bytes());
    hasher.update(namespace.as_bytes());
    for part in parts {
        hasher.update(part.len().to_be_bytes());
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn authorized_discovery_task_mutation(
    store: &InMemoryStore,
    ctx: &AuthContext,
    task_id: DiscoveryTaskId,
) -> Result<(Option<PodId>, AgentHarnessId), AgentToolsError> {
    let task = store
        .discovery_tasks
        .get(&task_id)
        .ok_or_else(|| StoreError::NotFound("Discovery Task".into()))?;
    let pod_id = authorize_discovery_task(store, ctx, task)?;
    let harness_id = ctx.harness_id.ok_or_else(|| AgentToolsError::Forbidden {
        reason: "task mutation requires an Agent Harness".into(),
    })?;
    Ok((pod_id, harness_id))
}

fn authorize_discovery_task(
    store: &InMemoryStore,
    ctx: &AuthContext,
    task: &DiscoveryTask,
) -> Result<Option<PodId>, AgentToolsError> {
    match task.target {
        DiscoveryTaskTarget::Pod { pod_id, .. } => {
            authorize_harness(store, ctx, HarnessCapability::DiscoveryTasks, Some(pod_id))?;
            Ok(Some(pod_id))
        }
        DiscoveryTaskTarget::Personal { .. } => {
            authorize_personal_discovery_execution(store, ctx)?;
            let plan_id = task
                .target
                .discovery_plan_id()
                .expect("Personal target carries a Discovery Plan ID");
            let plan = store
                .discovery_plans
                .get(&plan_id)
                .ok_or_else(|| StoreError::NotFound("Discovery Plan".into()))?;
            if ctx.user_id != Some(plan.user_id) || ctx.tenant_id != plan.tenant_id {
                return Err(AgentToolsError::Forbidden {
                    reason: "Personal Discovery task belongs to another User".into(),
                });
            }
            Ok(None)
        }
    }
}

fn authorize_personal_discovery_management(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Result<(), AgentToolsError> {
    authorize_harness(
        store,
        ctx,
        HarnessCapability::PersonalDiscoveryManagement,
        None,
    )?;
    if let Some(harness) = harness_for_context(store, ctx)? {
        if harness.kind != AgentHarnessKind::Interactive || harness.grant.pod_ids.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "Personal Discovery management requires an unscoped interactive grant"
                    .into(),
            });
        }
    }
    Ok(())
}

fn authorize_personal_discovery_execution(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Result<(), AgentToolsError> {
    authorize_harness(
        store,
        ctx,
        HarnessCapability::PersonalDiscoveryExecution,
        None,
    )?;
    if let Some(harness) = harness_for_context(store, ctx)? {
        if harness.kind != AgentHarnessKind::Unattended || harness.grant.pod_ids.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "Personal Discovery execution requires an unscoped unattended grant".into(),
            });
        }
    }
    Ok(())
}

/// Management or execution may inspect schedules and backpressure; only management may mutate.
fn authorize_personal_discovery_schedule_read(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Result<(), AgentToolsError> {
    if authorize_personal_discovery_management(store, ctx).is_ok() {
        return Ok(());
    }
    authorize_personal_discovery_execution(store, ctx)
}

fn authorize_harness_for_new_pod(
    store: &InMemoryStore,
    ctx: &AuthContext,
    capability: HarnessCapability,
) -> Result<(), AgentToolsError> {
    authorize_harness(store, ctx, capability, None)?;
    if let Some(harness) = ctx
        .harness_id
        .and_then(|harness_id| store.agent_harnesses.get(&harness_id))
    {
        if harness.grant.pod_ids.is_some() {
            return Err(AgentToolsError::Forbidden {
                reason: "a Pod-scoped harness grant cannot create a new Pod".to_string(),
            });
        }
    }
    Ok(())
}

fn authorize_harness_pod_scope(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod_id: PodId,
) -> Result<(), AgentToolsError> {
    let Some(harness) = harness_for_context(store, ctx)? else {
        return Ok(());
    };
    if harness
        .grant
        .pod_ids
        .as_ref()
        .is_some_and(|pod_ids| !pod_ids.contains(&pod_id))
    {
        return Err(AgentToolsError::Forbidden {
            reason: format!("harness grant does not include Pod {pod_id}"),
        });
    }
    Ok(())
}

fn authorize_harness_submission_scope(
    store: &InMemoryStore,
    ctx: &AuthContext,
    submission_id: SubmissionId,
) -> Result<(), AgentToolsError> {
    harness_for_context(store, ctx)?;
    for pod_id in store
        .submission_pods
        .iter()
        .filter(|placement| placement.submission_id == submission_id)
        .map(|placement| placement.pod_id)
    {
        authorize_harness_pod_scope(store, ctx, pod_id)?;
    }
    Ok(())
}

fn harness_for_context<'a>(
    store: &'a InMemoryStore,
    ctx: &AuthContext,
) -> Result<Option<&'a AgentHarness>, AgentToolsError> {
    let Some(harness_id) = ctx.harness_id else {
        return Ok(None);
    };
    let harness = store
        .agent_harnesses
        .get(&harness_id)
        .filter(|harness| harness.revoked_at.is_none())
        .ok_or_else(|| AgentToolsError::Forbidden {
            reason: "harness grant is revoked or missing".to_string(),
        })?;
    if Some(harness.user_id) != ctx.user_id || harness.tenant_id != ctx.tenant_id {
        return Err(AgentToolsError::Forbidden {
            reason: "harness grant does not match the authorization context".to_string(),
        });
    }
    Ok(Some(harness))
}

fn record_harness_write(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    operation: HarnessWriteOperation,
    pod_id: Option<PodId>,
) {
    record_harness_write_at(store, ctx, operation, pod_id, Utc::now());
}

fn record_harness_write_at(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    operation: HarnessWriteOperation,
    pod_id: Option<PodId>,
    occurred_at: chrono::DateTime<Utc>,
) {
    if let Some(harness_id) = ctx.harness_id {
        store.harness_write_audit.push(HarnessWriteAudit {
            id: Uuid::now_v7(),
            harness_id,
            operation,
            pod_id,
            occurred_at,
        });
    }
}

fn curation_actor(ctx: &AuthContext) -> CurationActor {
    ctx.harness_id
        .map(CurationActor::Harness)
        .or_else(|| ctx.user_id.map(CurationActor::User))
        .unwrap_or(CurationActor::NodeAgent)
}

fn authorize_local_pod_curation(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod_id: PodId,
) -> Result<(), AgentToolsError> {
    authorize_harness(store, ctx, HarnessCapability::PodCuration, Some(pod_id))?;
    let pod = store
        .pods
        .get(&pod_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
    store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
    let local_node_id = store.node_for_tenant(ctx.tenant_id)?.id;
    if pod
        .origin_node_id
        .is_some_and(|origin_node_id| origin_node_id != local_node_id)
    {
        return Err(AgentToolsError::Forbidden {
            reason: format!("remote Pod {pod_id} cannot receive local curation"),
        });
    }
    Ok(())
}

fn candidate_submissions_for(
    store: &InMemoryStore,
    candidate_id: CandidateId,
) -> Vec<CandidateSubmission> {
    let mut submissions = store
        .candidate_submissions
        .values()
        .filter(|submission| submission.candidate_id == candidate_id)
        .cloned()
        .collect::<Vec<_>>();
    submissions.sort_by_key(|submission| (submission.created_at, submission.id));
    submissions
}

struct MergedCandidateProposal {
    pod_id: PodId,
    reason: CurationRationale,
    confidence: CandidateConfidence,
    source_submission_ids: Vec<CandidateSubmissionId>,
}

fn merged_candidate_proposals(
    submissions: &[CandidateSubmission],
) -> Result<Vec<MergedCandidateProposal>, AgentToolsError> {
    let mut proposals: BTreeMap<PodId, MergedCandidateProposal> = BTreeMap::new();
    for submission in submissions {
        for placement in submission.target.placements() {
            let rationale = CurationRationale::new(placement.reason.clone())?;
            let entry =
                proposals
                    .entry(placement.pod_id)
                    .or_insert_with(|| MergedCandidateProposal {
                        pod_id: placement.pod_id,
                        reason: rationale.clone(),
                        confidence: placement.confidence,
                        source_submission_ids: Vec::new(),
                    });
            if placement.confidence.value() > entry.confidence.value() {
                entry.reason = rationale;
                entry.confidence = placement.confidence;
            }
            entry.source_submission_ids.push(submission.id);
        }
    }
    Ok(proposals.into_values().collect())
}

fn trusted_placement_confidence(
    store: &InMemoryStore,
    submissions: &[CandidateSubmission],
    pod_id: PodId,
) -> Option<CandidateConfidence> {
    submissions
        .iter()
        .filter(|submission| {
            submission
                .target
                .task_context()
                .and_then(|context| store.discovery_tasks.get(&context.task_id))
                .is_some_and(|task| {
                    task.target
                        .pod()
                        .is_some_and(|(task_pod_id, _)| task_pod_id == pod_id)
                })
        })
        .flat_map(|submission| submission.target.placements())
        .filter(|placement| placement.pod_id == pod_id)
        .map(|placement| placement.confidence)
        .max_by(|left, right| left.value().total_cmp(&right.value()))
}

fn taste_profile_from_store(
    store: &InMemoryStore,
    ctx: &AuthContext,
    user_id: UserId,
) -> Result<TasteProfile, AgentToolsError> {
    let preferences = store.user_preferences.get(&(user_id, ctx.tenant_id));
    let interest_seed_evidence = interest_seed_evidence(store, user_id, ctx.tenant_id);
    let projections = taste_profile_projections(store, user_id, ctx.tenant_id, preferences);
    let mut allowed_actions = vec![
        TasteProfileAllowedAction::Set,
        TasteProfileAllowedAction::Reset,
    ];
    if interest_seed_evidence.active_seed_count > 0 {
        allowed_actions.push(TasteProfileAllowedAction::Retract);
    }
    Ok(TasteProfile {
        user_id,
        tenant_id: ctx.tenant_id,
        explicit: ExplicitTastePreferences {
            interests: preferences
                .map(|preferences| preferences.interests.clone())
                .unwrap_or_default(),
            blocked_topics: preferences
                .map(|preferences| preferences.blocked_topics.clone())
                .unwrap_or_default(),
            blocked_sources: preferences
                .map(|preferences| preferences.blocked_sources.clone())
                .unwrap_or_default(),
            blocked_source_affinities: preferences
                .map(|preferences| preferences.blocked_source_affinities.clone())
                .unwrap_or_default(),
            recurrence_penalty_days: preferences
                .map_or_else(RecurrencePenaltyDays::default, |preferences| {
                    preferences.recurrence_penalty_days
                })
                .get(),
        },
        learned: projections.learned,
        interest_seed_evidence,
        source_affinities: projections.source_affinities,
        allowed_actions,
    })
}

fn accept_discovery_result_into_pod(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    candidate: &Candidate,
    submission: &CandidateSubmission,
    pod_id: PodId,
    reason: CurationRationale,
    now: chrono::DateTime<Utc>,
) -> Result<PodPlacement, AgentToolsError> {
    if let Some(existing) = store.pod_placements.get(&(candidate.id, pod_id)).cloned() {
        if existing.status == PodPlacementStatus::Accepted {
            return Ok(existing);
        }
    }
    let content_item = ensure_content_item(
        store,
        candidate,
        std::slice::from_ref(submission),
        &[submission.id],
        now,
    )?;
    let actor = curation_actor(ctx);
    let placement = PodPlacement {
        candidate_id: candidate.id,
        pod_id,
        content_item_id: Some(content_item.id()),
        reason,
        confidence: CandidateConfidence::new(1.0)
            .map_err(|error| StoreError::Validation(error.to_string()))?,
        source_submission_ids: vec![submission.id],
        origin_placements: Vec::new(),
        origin_withdrawals: Vec::new(),
        status: PodPlacementStatus::Accepted,
        curation_path: CurationPath::AddToPod,
        actor,
        audit_history: vec![PlacementAuditEntry {
            status: PodPlacementStatus::Accepted,
            curation_path: CurationPath::AddToPod,
            actor,
            note: None,
            occurred_at: now,
        }],
        created_at: now,
        updated_at: now,
    };
    accept_candidate_placement(store, ctx, candidate, &placement)?;
    store
        .pod_placements
        .insert((candidate.id, pod_id), placement.clone());
    Ok(placement)
}

fn ensure_content_item(
    store: &mut InMemoryStore,
    candidate: &Candidate,
    submissions: &[CandidateSubmission],
    authorized_submission_ids: &[CandidateSubmissionId],
    now: chrono::DateTime<Utc>,
) -> Result<ContentItem, AgentToolsError> {
    if let Some(existing) = store
        .submissions
        .values()
        .find(|item| {
            item.tenant_id == candidate.tenant_id && item.canonical_url == candidate.canonical_url
        })
        .cloned()
    {
        return Ok(ContentItem::from(&existing));
    }
    let evidence = submissions
        .iter()
        .find(|submission| authorized_submission_ids.contains(&submission.id))
        .ok_or_else(|| {
            StoreError::Validation(
                "Candidate placement has no explicitly authorized submission evidence".into(),
            )
        })?;
    let domain = Url::parse(&candidate.canonical_url)
        .map_err(|error| AgentToolsError::BadUrl(error.to_string()))?
        .domain()
        .unwrap_or("unknown")
        .to_string();
    let submitted_by = store
        .agent_harnesses
        .get(&evidence.submitted_by)
        .map(|harness| harness.user_id);
    let media_references = resolve_media_for_store(
        submissions
            .iter()
            .filter(|submission| authorized_submission_ids.contains(&submission.id))
            .flat_map(|submission| &submission.evidence.media_references),
    )?;
    let item = Submission {
        id: stable_candidate_uuid("content-item", &[&candidate.id.to_string()]),
        tenant_id: candidate.tenant_id,
        url: evidence.evidence.source_url.clone(),
        canonical_url: candidate.canonical_url.clone(),
        title: evidence
            .evidence
            .source_metadata
            .title
            .clone()
            .unwrap_or_else(|| candidate.canonical_url.clone()),
        description: evidence.evidence.permitted_excerpt.clone(),
        domain,
        submitted_by,
        discovered_by_crawler: false,
        submitter_note: None,
        summary: evidence.evidence.summary.clone(),
        media_references,
        tags: evidence.evidence.tags.clone(),
        embedding: None,
        created_at: now,
        origin_event_id: None,
    };
    store.submissions.insert(item.id, item.clone());
    Ok(ContentItem::from(&item))
}

fn enrich_accepted_content_item(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    candidate: &Candidate,
) -> Result<(), AgentToolsError> {
    let Some(item_id) = store
        .submissions
        .values()
        .find(|item| {
            item.tenant_id == candidate.tenant_id && item.canonical_url == candidate.canonical_url
        })
        .map(|item| item.id)
    else {
        return Ok(());
    };
    let existing_media = store
        .submissions
        .get(&item_id)
        .ok_or_else(|| StoreError::NotFound("Content Item".into()))?
        .media_references
        .clone();
    let content_item_id = ContentItemId::from(item_id);
    let accepted_pod_ids = store
        .accepted_placement_projections
        .values()
        .filter(|placement| placement.content_item_id == content_item_id)
        .map(|placement| placement.pod_id)
        .collect::<HashSet<_>>();
    let resolved = resolve_media_for_store(
        existing_media.iter().chain(
            store
                .candidate_submissions
                .values()
                .filter(|submission| {
                    submission.candidate_id == candidate.id
                        && submission
                            .target
                            .placements()
                            .iter()
                            .any(|placement| accepted_pod_ids.contains(&placement.pod_id))
                })
                .flat_map(|submission| &submission.evidence.media_references),
        ),
    )?;
    let item = store
        .submissions
        .get_mut(&item_id)
        .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
    if item.media_references == resolved {
        return Ok(());
    }
    item.media_references = resolved;

    let node = store.node_for_tenant(ctx.tenant_id)?;
    let mut pods = store
        .accepted_placement_projections
        .values()
        .filter(|placement| {
            placement.content_item_id == content_item_id && placement.origin_node_id == node.id
        })
        .filter_map(|placement| store.pods.get(&placement.pod_id).cloned())
        .collect::<Vec<_>>();
    pods.sort_by(|left, right| left.slug.cmp(&right.slug).then(left.id.cmp(&right.id)));
    let media_references = store
        .submissions
        .get(&item_id)
        .expect("accepted Content Item remains present")
        .media_references
        .clone();
    let now = Utc::now();
    for pod in pods {
        let payload = ContentItemMetadataUpdatedPayload {
            metadata_update: ContentItemMetadataUpdate {
                content_item_id,
                media_references: media_references.clone(),
            },
        };
        let event = sign_public_event(
            &node,
            FederatedPodEventType::ContentItemMetadataUpdated.as_wire(),
            &pod.slug,
            serde_json::to_value(payload).map_err(|error| {
                StoreError::Validation(format!("metadata update cannot be signed: {error}"))
            })?,
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        refresh_public_pod_announcement_if_needed(store, pod.id, now)?;
    }
    Ok(())
}

fn accept_placement(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    placement: &PodPlacement,
) -> Result<(), AgentToolsError> {
    let content_item_id = placement.content_item_id.ok_or_else(|| {
        StoreError::Validation("Accepted Placement requires a Content Item".into())
    })?;
    if !store.submission_pods.iter().any(|existing| {
        existing.pod_id == placement.pod_id && existing.submission_id == Uuid::from(content_item_id)
    }) {
        store.submission_pods.push(SubmissionPod {
            submission_id: content_item_id.into(),
            pod_id: placement.pod_id,
            created_at: placement.updated_at,
        });
    }
    let pod = store
        .pods
        .get(&placement.pod_id)
        .cloned()
        .ok_or_else(|| StoreError::NotFound(format!("Pod {}", placement.pod_id)))?;
    let item = store
        .submissions
        .get(&Uuid::from(content_item_id))
        .cloned()
        .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
    let node = store.node_for_tenant(ctx.tenant_id)?;
    let projection = AcceptedPlacementProjection {
        content_item_id,
        pod_id: placement.pod_id,
        reason: placement.reason.clone(),
        curation_path: placement.curation_path,
        origin_node_id: node.id,
        accepted_at: placement.updated_at,
    };
    store
        .accepted_placement_projections
        .insert((content_item_id, placement.pod_id), projection.clone());
    let event = sign_public_event(
        &node,
        "content_item_placed",
        &pod.slug,
        json!({
            "content_item": ContentItem::from(&item),
            "accepted_placement": projection,
        }),
        store.latest_event_hash(&pod.slug),
    )?;
    store.event_log.push(event);
    refresh_public_pod_announcement_if_needed(store, placement.pod_id, placement.updated_at)?;
    Ok(())
}

fn accept_candidate_placement(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    candidate: &Candidate,
    placement: &PodPlacement,
) -> Result<(), AgentToolsError> {
    accept_placement(store, ctx, placement)?;
    if let Some(candidate) = store.candidates.get_mut(&candidate.id) {
        candidate.review_state = CandidateReviewState::Accepted;
    }
    Ok(())
}

fn candidate_curation_result(
    store: &InMemoryStore,
    candidate_id: CandidateId,
) -> Result<CandidateCurationResult, AgentToolsError> {
    let candidate = store
        .candidates
        .get(&candidate_id)
        .cloned()
        .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
    let mut placements = store
        .pod_placements
        .values()
        .filter(|placement| placement.candidate_id == candidate_id)
        .cloned()
        .collect::<Vec<_>>();
    placements.sort_by_key(|placement| placement.pod_id);
    let content_item = placements
        .iter()
        .find_map(|placement| placement.content_item_id)
        .and_then(|content_item_id| {
            store
                .submissions
                .get(&Uuid::from(content_item_id))
                .map(ContentItem::from)
        });
    Ok(CandidateCurationResult {
        candidate,
        content_item,
        placements,
    })
}

fn verify_portable_package_history(
    store: &InMemoryStore,
    files: &BTreeMap<String, String>,
) -> Result<(), AgentToolsError> {
    let events = verified_portable_package_events(store, files)?;
    let requested = pod_package_contents_from_files(files)?;
    let has_signed_contents = events.iter().any(|event| {
        event
            .payload_json
            .get("package")
            .and_then(|value| serde_json::from_value::<PodSkillPack>(value.clone()).ok())
            .is_some_and(|package| package_contents_match(&package, &requested))
    });
    if !has_signed_contents {
        return Err(StoreError::Validation(
            "events.jsonl does not contain the signed package contents".to_string(),
        )
        .into());
    }
    Ok(())
}

fn verify_portable_package_history_for_base(
    store: &InMemoryStore,
    files: &BTreeMap<String, String>,
    base: &PodPackage,
) -> Result<(), AgentToolsError> {
    let events = verified_portable_package_events(store, files)?;
    let has_signed_base = events.iter().any(|event| {
        event
            .payload_json
            .get("package")
            .and_then(|value| serde_json::from_value::<PodPackage>(value.clone()).ok())
            .is_some_and(|package| package == *base)
    });
    if !has_signed_base {
        return Err(StoreError::Validation(
            "events.jsonl does not contain the signed base Package version".to_string(),
        )
        .into());
    }
    Ok(())
}

fn verified_portable_package_events(
    store: &InMemoryStore,
    files: &BTreeMap<String, String>,
) -> Result<Vec<EventLog>, AgentToolsError> {
    let events_text = files.get("events.jsonl").ok_or_else(|| {
        StoreError::Validation("portable Pod Package is missing events.jsonl".into())
    })?;
    let events = events_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<EventLog>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| StoreError::Validation(format!("events.jsonl is invalid: {error}")))?;
    if events.is_empty() {
        return Err(StoreError::Validation(
            "events.jsonl must contain signed package history".to_string(),
        )
        .into());
    }
    for event in &events {
        let public_key = store
            .node_identities
            .get(&event.author_node_id)
            .filter(|node| node.tenant_id == event.tenant_id)
            .map(|node| node.public_key.as_str())
            .or_else(|| {
                store
                    .hub_nodes
                    .get(&event.author_node_id)
                    .map(|node| node.public_key.as_str())
            })
            .or_else(|| {
                store
                    .trusted_peers
                    .get(&event.author_node_id)
                    .filter(|peer| peer.enabled)
                    .map(|peer| peer.public_key.as_str())
            })
            .ok_or(StoreError::UntrustedPeer)?;
        if !verify_event(event, public_key).map_err(|_| StoreError::InvalidSignature)? {
            return Err(StoreError::InvalidSignature.into());
        }
    }
    Ok(events)
}

fn ensure_package_base_version(
    existing: &PodPackage,
    base_version: PackageVersion,
) -> Result<(), AgentToolsError> {
    if PackageVersion::new(existing.version)
        .map_err(|error| StoreError::Validation(error.to_string()))?
        != base_version
    {
        return Err(StoreError::Validation("Package Revision base version is stale".into()).into());
    }
    Ok(())
}

fn complete_package_patch(contents: &PodPackageContents) -> SkillPackPatch {
    SkillPackPatch {
        context_md: Some(contents.context_md.clone()),
        pod_yaml: None,
        skill_md: Some(contents.skill_md.clone()),
        sources_yaml: Some(contents.sources_yaml.clone()),
        filters_yaml: Some(contents.filters_yaml.clone()),
        examples_good_md: Some(contents.examples_good_md.clone()),
        examples_bad_md: Some(contents.examples_bad_md.clone()),
    }
}

fn ensure_direct_package_revision_allowed(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod: &Pod,
) -> Result<(), AgentToolsError> {
    ensure_direct_package_revision_allowed_for_origin(store, ctx, pod)?;
    if pod.visibility == Visibility::Public {
        return Err(StoreError::Validation(
            "public Package Revisions require Pending Proposal approval".to_string(),
        )
        .into());
    }
    Ok(())
}

fn ensure_direct_package_revision_allowed_for_origin(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod: &Pod,
) -> Result<(), AgentToolsError> {
    let local_node = store.node_for_tenant(ctx.tenant_id)?;
    if pod
        .origin_node_id
        .is_some_and(|origin_node_id| origin_node_id != local_node.id)
    {
        return Err(StoreError::Validation(
            "remote Pod Packages may change only through verified synchronization".to_string(),
        )
        .into());
    }
    Ok(())
}

fn package_contents_match(package: &PodSkillPack, contents: &PodPackageContents) -> bool {
    package.context_md == contents.context_md
        && package.skill_md == contents.skill_md
        && package.sources_yaml == contents.sources_yaml
        && package.filters_yaml == contents.filters_yaml
        && package.examples_good_md == contents.examples_good_md
        && package.examples_bad_md == contents.examples_bad_md
}

fn normalize_capabilities(mut capabilities: Vec<HarnessCapability>) -> Vec<HarnessCapability> {
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

struct FeedItemSelection {
    recurrence_penalty_applied: bool,
    attention_value: f32,
    reasons: Vec<String>,
    kind: FeedItemKind,
}

fn feed_content_reference(item: &Submission) -> FeedContentReference {
    FeedContentReference {
        content_item_id: ContentItemId::from(item.id),
        source_url: item.url.clone(),
        canonical_url: item.canonical_url.clone(),
        title: item.title.clone(),
        permitted_description: item.description.clone(),
        summary: item.summary.clone(),
        media_references: item.media_references.clone(),
        source: item.domain.clone(),
        tags: item.tags.clone(),
    }
}

/// Builds shared private LocalSimilarityContext for Explore and Feed ranking.
fn local_similarity_context_from_store(
    query: Option<&str>,
    preferences: Option<&UserPreferences>,
    projections: &TasteProfileProjections,
) -> LocalSimilarityContext {
    let interests = preferences
        .map(|prefs| {
            prefs
                .interests
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let source_signals = projections
        .source_affinities
        .iter()
        .filter(|affinity| affinity.weight > 0.0 && !affinity.explicitly_blocked)
        .filter_map(|affinity| match &affinity.signal {
            SourceAffinitySignal::Source(source) | SourceAffinitySignal::Community(source) => {
                Some(source.as_str())
            }
            _ => None,
        });
    LocalSimilarityContext::from_private_evidence(query, interests, source_signals)
}

/// Retained Origin-signed samples when verified; otherwise local content without
/// claiming verification (trial requires real retained Origin-signed samples).
fn retained_or_local_explore_samples(
    store: &InMemoryStore,
    announcement: &PodAnnouncement,
    policy: &TrustPolicy,
    tenant_id: Option<TenantId>,
    local_node_id: NodeIdentityId,
    sample_size: usize,
) -> (Vec<FeedContentReference>, bool) {
    if let Some(sample_set) = store.pod_explore_sample_sets.get(&announcement.id) {
        let verified = verify_explore_samples_for_announcement(sample_set, announcement).is_ok();
        let samples = sample_set
            .samples
            .iter()
            .filter(|sample| !policy.blocks_content_reference(sample))
            .take(sample_size)
            .cloned()
            .collect::<Vec<_>>();
        return (samples, verified);
    }
    let local_samples = explore_content_samples(
        store,
        tenant_id,
        local_node_id,
        announcement,
        policy,
        sample_size,
    );
    // Local content may inform scoring but never claims Origin-signed verification.
    (local_samples, false)
}

/// Whether retained Origin-signed explore samples verify for this announcement.
fn retained_samples_verified(store: &InMemoryStore, announcement: &PodAnnouncement) -> bool {
    store
        .pod_explore_sample_sets
        .get(&announcement.id)
        .is_some_and(|sample_set| {
            verify_explore_samples_for_announcement(sample_set, announcement).is_ok()
        })
}

/// Local similarity for an unsubscribed public Pod Exploration Item.
///
/// Uses the same eligibility and endorsement policy gates as Explore. Requires a
/// real current verified announcement — never fabricates synthetic announcements.
/// `samples_verified` only from retained Origin-signed samples.
fn exploration_similarity_for_item(
    store: &InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    item: &Submission,
    placement_pod_ids: &[PodId],
    preferences: Option<&UserPreferences>,
) -> Option<PodSimilarityScore> {
    let projections = taste_profile_projections(store, user_id, tenant_id, preferences);
    let local = local_similarity_context_from_store(None, preferences, &projections);
    if local.is_empty() {
        return None;
    }
    let policy = store
        .trust_policies
        .get(&(user_id, tenant_id))
        .cloned()
        .unwrap_or_else(|| TrustPolicy::new(user_id, tenant_id));
    let now = Utc::now();
    let sample_ref = feed_content_reference(item);
    let mut best: Option<PodSimilarityScore> = None;
    for pod_id in placement_pod_ids {
        let Some(pod) = store.pods.get(pod_id) else {
            continue;
        };
        let Some(origin) = pod.origin_node_id.or_else(|| {
            store
                .node_identities
                .values()
                .find(|node| node.tenant_id == tenant_id)
                .map(|node| node.id)
        }) else {
            continue;
        };
        // Skip when no verified current announcement — never fabricate Uuid::nil shells.
        let Some(known) = store
            .known_pod_announcements
            .get(&(origin, pod.slug.clone()))
        else {
            continue;
        };
        if !announcement_scoring_eligible(store, known, &policy, now) {
            continue;
        }
        let announcement = &known.announcement;
        let endorsements = collect_policy_endorsements(store, announcement, &policy);
        let context_text = store
            .pod_skill_packs
            .get(pod_id)
            .map(|package| package.context_md.as_str());
        let samples_verified = retained_samples_verified(store, announcement);
        let Some(similarity) = score_exploration_item(
            &local,
            announcement,
            context_text,
            &sample_ref,
            &endorsements,
            samples_verified,
        ) else {
            continue;
        };
        best = Some(match best {
            Some(existing) if existing.score >= similarity.score => existing,
            _ => similarity,
        });
    }
    best
}

/// Enforces per-Origin exploration and trial caps after Feed Mix selection.
fn apply_exploration_origin_caps<'a>(
    store: &InMemoryStore,
    _user_id: UserId,
    selected: Vec<RankedFeedCandidate<'a>>,
) -> Vec<RankedFeedCandidate<'a>> {
    let caps = ExplorationCaps {
        per_origin: MAX_RESULTS_PER_ORIGIN,
        per_pod: usize::MAX,
        per_source: usize::MAX,
        per_origin_trial: MAX_TRIAL_ITEMS_PER_ORIGIN,
    };
    let local_node_id = store.node_identities.values().next().map(|node| node.id);
    let mut tracker = ExplorationCapTracker::new();
    let mut kept = Vec::with_capacity(selected.len());
    for candidate in selected {
        if candidate.kind != FeedItemKind::Exploration {
            kept.push(candidate);
            continue;
        }
        let origin = candidate
            .pod_ids
            .iter()
            .find_map(|pod_id| {
                store
                    .pods
                    .get(pod_id)
                    .map(|pod| pod.origin_node_id.or(local_node_id))
            })
            .flatten()
            .or(local_node_id);
        let Some(origin) = origin else {
            kept.push(candidate);
            continue;
        };
        // Typed flag only — never infer trial from reason-string contains.
        let trial = candidate.trial_exposure;
        let pod_slug = candidate
            .pod_ids
            .first()
            .and_then(|pod_id| store.pods.get(pod_id).map(|pod| pod.slug.clone()))
            .unwrap_or_default();
        if !tracker.can_admit_origin(origin, caps) {
            continue;
        }
        if trial && !tracker.can_admit_trial(origin, caps) {
            continue;
        }
        tracker.record(origin, &pod_slug, Some(&candidate.item.domain), trial);
        kept.push(candidate);
    }
    kept
}

fn explore_content_samples(
    store: &InMemoryStore,
    tenant_id: Option<TenantId>,
    local_node_id: NodeIdentityId,
    announcement: &PodAnnouncement,
    policy: &TrustPolicy,
    sample_size: usize,
) -> Vec<FeedContentReference> {
    let Some(pod) = store.pods.values().find(|pod| {
        pod.tenant_id == tenant_id
            && pod.visibility == Visibility::Public
            && pod.slug == announcement.pod_slug
            && pod.origin_node_id.unwrap_or(local_node_id) == announcement.origin_node_id
    }) else {
        return Vec::new();
    };
    let mut samples = store
        .submissions
        .values()
        .filter(|item| item.tenant_id == tenant_id)
        .filter(|item| {
            store
                .accepted_placement_projections
                .contains_key(&(ContentItemId::from(item.id), pod.id))
        })
        .filter(|item| {
            !policy.blocks_source_and_topics(
                &item.domain,
                &item.tags,
                &item.title,
                item.summary.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    samples.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.canonical_url.cmp(&right.canonical_url))
    });
    samples
        .into_iter()
        .take(sample_size)
        .map(feed_content_reference)
        .collect()
}

fn origin_placement_identity(
    placement: &AcceptedPlacementProjection,
) -> (ContentItemId, PodId, NodeIdentityId, chrono::DateTime<Utc>) {
    (
        placement.content_item_id,
        placement.pod_id,
        placement.origin_node_id,
        placement.accepted_at,
    )
}

fn feed_batch_item(
    store: &InMemoryStore,
    user_id: UserId,
    item: &Submission,
    allowed_actions: &[FeedAllowedAction],
    scoped_pod_ids: Option<&[PodId]>,
    selection: FeedItemSelection,
) -> FeedBatchItem {
    let FeedItemSelection {
        recurrence_penalty_applied,
        attention_value,
        mut reasons,
        kind,
    } = selection;
    let content_item_id = ContentItemId::from(item.id);
    let placements = store
        .accepted_placement_projections
        .values()
        .filter(|placement| {
            placement.content_item_id == content_item_id
                && scoped_pod_ids.is_none_or(|pod_ids| pod_ids.contains(&placement.pod_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let provenance = store
        .pod_placements
        .values()
        .filter(|placement| {
            placement.content_item_id == Some(content_item_id)
                && scoped_pod_ids.is_none_or(|pod_ids| pod_ids.contains(&placement.pod_id))
        })
        .flat_map(|placement| placement.source_submission_ids.iter())
        .filter_map(|submission_id| store.candidate_submissions.get(submission_id))
        .map(|submission| submission.evidence.provenance.clone())
        .collect::<Vec<_>>();
    let is_exploration = kind == FeedItemKind::Exploration;
    let inferred_exploration = !placements.is_empty()
        && placements.iter().all(|placement| {
            store
                .pods
                .get(&placement.pod_id)
                .is_some_and(|pod| pod.visibility == Visibility::Public)
                && !store.subscriptions.values().any(|subscription| {
                    subscription.user_id == user_id && subscription.local_pod_id == placement.pod_id
                })
        });
    const EXPLORATION_REASON: &str = "Clearly labeled exploration from an unsubscribed public Pod";
    if (is_exploration || inferred_exploration)
        && !reasons.iter().any(|reason| reason == EXPLORATION_REASON)
    {
        reasons.push(EXPLORATION_REASON.into());
    }
    FeedBatchItem {
        content_reference: feed_content_reference(item),
        placements,
        provenance,
        ranking_evidence: FeedRankingEvidence {
            attention_value,
            reasons,
            recurrence_penalty_applied,
        },
        is_exploration: is_exploration || inferred_exploration,
        kind,
        feedback_state: feed_feedback_state(store, user_id, item.id),
        allowed_actions: allowed_actions.to_vec(),
    }
}

fn project_feed_batch_for_context(
    store: &InMemoryStore,
    ctx: &AuthContext,
    batch: &FeedBatch,
) -> Result<FeedBatch, AgentToolsError> {
    let scoped_pod_ids =
        harness_for_context(store, ctx)?.and_then(|harness| harness.grant.pod_ids.as_deref());
    let allowed_actions = feed_allowed_actions(store, ctx)?;
    let mut projected = batch.clone();
    projected.items = batch
        .items
        .iter()
        .filter_map(|existing| {
            let submission_id = SubmissionId::from(existing.content_reference.content_item_id);
            let item = store.submissions.get(&submission_id)?;
            let has_visible_placement =
                store
                    .accepted_placement_projections
                    .keys()
                    .any(|(content_item_id, pod_id)| {
                        *content_item_id == existing.content_reference.content_item_id
                            && scoped_pod_ids.is_none_or(|pod_ids| pod_ids.contains(pod_id))
                    });
            has_visible_placement.then(|| {
                feed_batch_item(
                    store,
                    batch.user_id,
                    item,
                    &allowed_actions,
                    scoped_pod_ids,
                    FeedItemSelection {
                        recurrence_penalty_applied: existing
                            .ranking_evidence
                            .recurrence_penalty_applied,
                        attention_value: existing.ranking_evidence.attention_value,
                        reasons: existing.ranking_evidence.reasons.clone(),
                        kind: existing.kind,
                    },
                )
            })
        })
        .collect();
    Ok(projected)
}

fn feed_attention_value(
    store: &InMemoryStore,
    user_id: UserId,
    item: &Submission,
    scoped_pod_ids: Option<&[PodId]>,
    now: chrono::DateTime<Utc>,
) -> (f32, Vec<String>) {
    let state = feed_feedback_state(store, user_id, item.id);
    let placement_count = store
        .accepted_placement_projections
        .values()
        .filter(|placement| {
            placement.content_item_id == item.id.into()
                && scoped_pod_ids.is_none_or(|pod_ids| pod_ids.contains(&placement.pod_id))
        })
        .count();
    let preferences = store.user_preferences.get(&(user_id, item.tenant_id));
    let matched_explicit_interests = scoped_pod_ids
        .is_none()
        .then_some(preferences)
        .flatten()
        .map(|preferences| {
            preferences
                .interests
                .iter()
                .filter(|interest| {
                    item.tags
                        .iter()
                        .any(|tag| tag.eq_ignore_ascii_case(interest))
                        || item.title.to_lowercase().contains(&interest.to_lowercase())
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let relevance_matches = matched_explicit_interests.len();
    let relevance = u16::try_from(relevance_matches).map_or(f32::from(u16::MAX), f32::from);
    let age_days = (now - item.created_at).num_days().max(0);
    let timeliness = if age_days <= 30 { 0.5 } else { 0.0 };
    let feedback =
        if state.saved { 2.0 } else { 0.0 } + if state.more_like_this { 1.0 } else { 0.0 };
    let quality = u16::try_from(placement_count).map_or(f32::from(u16::MAX), f32::from) * 0.25;
    let projections = if scoped_pod_ids.is_none() {
        Some(taste_profile_projections(
            store,
            user_id,
            item.tenant_id,
            preferences,
        ))
    } else {
        None
    };
    let mut learned_value = 0.0;
    let mut learned_reasons = Vec::new();
    for weight in projections
        .iter()
        .flat_map(|projections| &projections.learned)
        .filter(|weight| weight.weight != 0.0)
    {
        let matches = match &weight.signal {
            LearnedTasteSignal::Topic(topic) => {
                item.tags.iter().any(|tag| tag.eq_ignore_ascii_case(topic))
            }
            _ => false,
        };
        if !matches {
            continue;
        }
        let explicit_interest_matches = match &weight.signal {
            LearnedTasteSignal::Topic(topic) => preferences.is_some_and(|preferences| {
                preferences
                    .interests
                    .iter()
                    .any(|interest| interest.eq_ignore_ascii_case(topic))
            }),
            _ => false,
        };
        let applied_weight = if explicit_interest_matches {
            weight.weight.max(0.0)
        } else {
            weight.weight
        };
        learned_value += applied_weight;
        let (signal_kind, signal_value) = weight.signal.key();
        if explicit_interest_matches && weight.weight < 0.0 {
            learned_reasons.push(format!(
                "Explicit interest '{signal_value}' overrode learned {signal_kind} '{signal_value}' aversion from {} opposing signals",
                weight.opposing_signals
            ));
        } else if applied_weight != 0.0 {
            let (direction, evidence_count) = if applied_weight > 0.0 {
                ("affinity increased value", weight.supporting_signals)
            } else {
                ("aversion reduced value", weight.opposing_signals)
            };
            learned_reasons.push(format!(
                "Learned {signal_kind} '{signal_value}' {direction} from {evidence_count} relevant signals ({} supporting, {} opposing)",
                weight.supporting_signals, weight.opposing_signals
            ));
        }
    }
    for affinity in projections
        .iter()
        .flat_map(|projections| &projections.source_affinities)
        .filter(|affinity| affinity.weight != 0.0)
    {
        let matches = match &affinity.signal {
            SourceAffinitySignal::Source(source) => item.domain.eq_ignore_ascii_case(source),
            SourceAffinitySignal::Publisher(_)
            | SourceAffinitySignal::AuthorOrAccount(_)
            | SourceAffinitySignal::Community(_)
            | SourceAffinitySignal::ReferrerContext(_) => false,
        };
        if !matches {
            continue;
        }
        learned_value += affinity.weight;
        let (signal_kind, signal_value) = affinity.signal.key();
        let supporting = affinity
            .supporting_seeds
            .saturating_add(affinity.supporting_feedback);
        let (direction, evidence_count) = if affinity.weight > 0.0 {
            ("affinity increased value", supporting)
        } else {
            ("aversion reduced value", affinity.opposing_feedback)
        };
        learned_reasons.push(format!(
            "Learned {signal_kind} '{signal_value}' {direction} from {evidence_count} relevant signals ({supporting} supporting, {} opposing)",
            affinity.opposing_feedback
        ));
    }
    let score = 1.0 + relevance + quality + timeliness + feedback + learned_value;
    let mut reasons = vec![format!(
        "{placement_count} Accepted Placement(s) support quality and Pod context"
    )];
    if relevance > 0.0 {
        reasons.push(format!(
            "Explicit interests matched the Content Reference: {}",
            matched_explicit_interests.join(", ")
        ));
    }
    if timeliness > 0.0 {
        reasons.push("Recent publication increased timeliness".into());
    }
    if feedback > 0.0 {
        reasons.push("Explicit Save or More like this feedback increased value".into());
    }
    reasons.extend(learned_reasons);
    if placement_count > 1 {
        reasons.push("Independent Pod Placements increased diversity evidence".into());
    }
    (score, reasons)
}

fn taste_evidence_for_feedback(
    kind: FeedbackKind,
) -> Option<(LearnedTasteEvidenceKind, TasteEvidenceDirection)> {
    // Dismiss/ignore never create durable preference (see feedback_affects_future_exposure).
    match kind {
        FeedbackKind::Saved => Some((
            LearnedTasteEvidenceKind::Save,
            TasteEvidenceDirection::Supporting,
        )),
        FeedbackKind::Interesting => Some((
            LearnedTasteEvidenceKind::MoreLikeThis,
            TasteEvidenceDirection::Supporting,
        )),
        FeedbackKind::NotForMe => Some((
            LearnedTasteEvidenceKind::LessLikeThis,
            TasteEvidenceDirection::Opposing,
        )),
        FeedbackKind::Dismissed | FeedbackKind::BlockSource | FeedbackKind::BlockTopic => None,
    }
}

fn record_taste_learning_evidence(
    store: &mut InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    item: &Submission,
    kind: LearnedTasteEvidenceKind,
    direction: TasteEvidenceDirection,
    now: chrono::DateTime<Utc>,
) {
    let content_item_id = ContentItemId::from(item.id);
    let accepted_submission_ids = store
        .pod_placements
        .values()
        .filter(|placement| {
            placement.status == PodPlacementStatus::Accepted
                && placement.content_item_id == Some(content_item_id)
        })
        .flat_map(|placement| placement.source_submission_ids.iter().copied())
        .collect::<HashSet<_>>();
    let mut signals = HashSet::new();
    signals.insert(LearnedTasteSignal::Source(item.domain.to_lowercase()));
    signals.extend(
        item.tags
            .iter()
            .map(|tag| LearnedTasteSignal::Topic(tag.to_lowercase())),
    );
    for candidate in store.candidates.values().filter(|candidate| {
        candidate.tenant_id == tenant_id && candidate.canonical_url == item.canonical_url
    }) {
        for submission in store
            .candidate_submissions
            .values()
            .filter(|submission| submission.candidate_id == candidate.id)
            .filter(|submission| match submission.target {
                CandidateSubmissionTarget::User {
                    user_id: target_user,
                    ..
                } => target_user == user_id,
                CandidateSubmissionTarget::PodPlacements { .. } => {
                    accepted_submission_ids.contains(&submission.id)
                }
                // Agent-discovered Personal Discovery results never train taste alone.
                CandidateSubmissionTarget::PersonalDiscovery { .. } => false,
            })
        {
            signals.extend(candidate_submission_taste_signals(candidate, submission));
        }
    }
    store
        .taste_learning_evidence
        .extend(signals.into_iter().map(|signal| TasteLearningEvidence {
            id: Uuid::now_v7(),
            user_id,
            tenant_id,
            signal,
            kind,
            direction,
            created_at: now,
        }));
}

fn record_add_to_pod_learning(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    item: &Submission,
    now: chrono::DateTime<Utc>,
) {
    if authorize_interactive_user_action(
        store,
        ctx,
        "Add-to-Pod learning requires an interactive User action",
    )
    .is_err()
    {
        return;
    }
    if let Some(user_id) = ctx.user_id {
        record_taste_learning_evidence(
            store,
            user_id,
            ctx.tenant_id,
            item,
            LearnedTasteEvidenceKind::AddToPod,
            TasteEvidenceDirection::Supporting,
            now,
        );
    }
}

fn feed_allowed_actions(
    store: &InMemoryStore,
    ctx: &AuthContext,
) -> Result<Vec<FeedAllowedAction>, AgentToolsError> {
    let Some(harness) = harness_for_context(store, ctx)? else {
        return Ok(vec![
            FeedAllowedAction::Save,
            FeedAllowedAction::MoreLikeThis,
            FeedAllowedAction::LessLikeThis,
            FeedAllowedAction::Dismiss,
            FeedAllowedAction::BlockSource,
            FeedAllowedAction::BlockTopic,
            FeedAllowedAction::AddToPod,
        ]);
    };
    let mut actions = Vec::new();
    if harness.kind == AgentHarnessKind::Interactive
        && harness
            .grant
            .capabilities
            .contains(&HarnessCapability::Feedback)
    {
        actions.extend([
            FeedAllowedAction::Save,
            FeedAllowedAction::MoreLikeThis,
            FeedAllowedAction::LessLikeThis,
            FeedAllowedAction::Dismiss,
            FeedAllowedAction::BlockSource,
            FeedAllowedAction::BlockTopic,
        ]);
    }
    if harness
        .grant
        .capabilities
        .contains(&HarnessCapability::PodCuration)
        && harness
            .grant
            .pod_ids
            .as_ref()
            .is_none_or(|pod_ids| !pod_ids.is_empty())
    {
        actions.push(FeedAllowedAction::AddToPod);
    }
    Ok(actions)
}

fn feed_feedback_state(
    store: &InMemoryStore,
    user_id: UserId,
    submission_id: SubmissionId,
) -> FeedFeedbackState {
    let item = store.submissions.get(&submission_id);
    let preferences = item.and_then(|item| {
        store
            .user_preferences
            .get(&(user_id, item.tenant_id))
            .map(|preferences| (item, preferences))
    });
    let has_feedback = |kind| {
        store.feedback_events.iter().any(|event| {
            event.user_id == user_id
                && event.submission_id == submission_id
                && event.event_type == kind
        })
    };
    FeedFeedbackState {
        saved: has_feedback(FeedbackKind::Saved),
        more_like_this: has_feedback(FeedbackKind::Interesting),
        less_like_this: has_feedback(FeedbackKind::NotForMe),
        dismissed: has_feedback(FeedbackKind::Dismissed),
        source_blocked: preferences.is_some_and(|(item, preferences)| {
            source_affinity_is_blocked(
                preferences,
                &SourceAffinitySignal::Source(item.domain.clone()),
            )
        }),
        topic_blocked: preferences.is_some_and(|(item, preferences)| {
            preferences
                .blocked_topics
                .iter()
                .any(|topic| item.tags.iter().any(|tag| tag.eq_ignore_ascii_case(topic)))
        }),
    }
}

fn authorize_proposal_reader(
    store: &InMemoryStore,
    ctx: &AuthContext,
    proposal_id: PendingProposalId,
) -> Result<(), AgentToolsError> {
    let proposal = store
        .pending_proposals
        .get(&proposal_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
    let Some(harness) = harness_for_context(store, ctx)? else {
        if ctx.tenant_id == proposal.tenant_id
            && (ctx.user_id.is_none() || ctx.user_id == Some(proposal.user_id))
        {
            return Ok(());
        }
        return Err(AgentToolsError::Forbidden {
            reason: "Pending Proposal belongs to another User or tenant".to_string(),
        });
    };
    if harness.tenant_id == proposal.tenant_id
        && harness.user_id == proposal.user_id
        && (harness.id == proposal.proposer
            || (harness
                .grant
                .capabilities
                .contains(&HarnessCapability::Approval)
                && approval_scope_allows(harness, proposal)))
    {
        return Ok(());
    }
    Err(AgentToolsError::Forbidden {
        reason: "harness cannot inspect this Pending Proposal".to_string(),
    })
}

fn authorize_independent_approver(
    store: &InMemoryStore,
    ctx: &AuthContext,
    proposal_id: PendingProposalId,
) -> Result<ProposalDecisionActor, AgentToolsError> {
    let proposal = store
        .pending_proposals
        .get(&proposal_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
    let Some(harness) = harness_for_context(store, ctx)? else {
        if ctx.tenant_id == proposal.tenant_id && ctx.user_id == Some(proposal.user_id) {
            return Ok(ProposalDecisionActor::Owner {
                owner_user_id: proposal.user_id,
            });
        }
        return Err(AgentToolsError::Forbidden {
            reason: "approval must belong to the proposal User and tenant".to_string(),
        });
    };
    authorize_harness(store, ctx, HarnessCapability::Approval, None)?;
    if harness.kind != AgentHarnessKind::Interactive {
        return Err(AgentToolsError::Forbidden {
            reason: "approval requires an interactive Agent Harness".to_string(),
        });
    }
    if harness.tenant_id != proposal.tenant_id || harness.user_id != proposal.user_id {
        return Err(AgentToolsError::Forbidden {
            reason: "approval must belong to the proposal User and tenant".to_string(),
        });
    }
    if !approval_scope_allows(harness, proposal) {
        return Err(AgentToolsError::Forbidden {
            reason: "approval Harness Grant does not cover the affected resources".to_string(),
        });
    }
    if proposal.proposer == harness.id {
        return Err(AgentToolsError::Forbidden {
            reason: "a harness cannot approve its own Pending Proposal".to_string(),
        });
    }
    Ok(ProposalDecisionActor::Harness(harness.id))
}

fn approval_scope_allows(harness: &AgentHarness, proposal: &PendingProposal) -> bool {
    proposal
        .affected_resources
        .iter()
        .all(|resource| match resource {
            ProposalResource::Pod(pod_id)
            | ProposalResource::PodPackage(pod_id)
            | ProposalResource::PodCurationPolicy(pod_id)
            | ProposalResource::PodRoles(pod_id)
            | ProposalResource::SubmissionPlacement { pod_id, .. } => harness
                .grant
                .pod_ids
                .as_ref()
                .is_none_or(|pod_ids| pod_ids.contains(pod_id)),
            ProposalResource::PodSlug(_)
            | ProposalResource::AgentHarness(_)
            | ProposalResource::TrustedPeerUrl(_)
            | ProposalResource::TrustPolicy(_) => harness.grant.pod_ids.is_none(),
        })
}

fn visibility_exposure(visibility: &Visibility) -> u8 {
    match visibility {
        Visibility::Private => 0,
        Visibility::InviteOnly => 1,
        Visibility::Public => 2,
    }
}

fn validate_creation_package_locked(
    store: &InMemoryStore,
    ctx: &AuthContext,
    package: &PodCreationPackage,
) -> Result<(), AgentToolsError> {
    match package {
        PodCreationPackage::Default => Ok(()),
        PodCreationPackage::Initial { package } => {
            let report = validate_pod_package_contents(package);
            if report.valid {
                Ok(())
            } else {
                Err(StoreError::Validation(report.errors.join(", ")).into())
            }
        }
        PodCreationPackage::Derived { source_package } => {
            let source = store
                .pod_package_versions
                .values()
                .find(|candidate| candidate.id == source_package.id)
                .ok_or_else(|| StoreError::NotFound("source Pod Package".into()))?;
            let source_pod = store
                .pods
                .get(&source.pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {}", source.pod_id)))?;
            store.assert_tenant(source_pod.tenant_id, ctx.tenant_id)?;
            authorize_harness(
                store,
                ctx,
                HarnessCapability::PackageManagement,
                Some(source_pod.id),
            )?;
            if source != source_package {
                return Err(StoreError::Validation(
                    "derived source Pod Package does not match stored provenance".into(),
                )
                .into());
            }
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
enum PodCreationMode {
    Canonical,
    SimpleCreate,
    PrivatePackage,
    LegacyPublic,
}

impl PodCreationMode {
    const fn event_type(self) -> &'static str {
        match self {
            Self::PrivatePackage => "private_pod_package_created",
            Self::Canonical | Self::SimpleCreate | Self::LegacyPublic => "pod_created",
        }
    }

    const fn records_audit(self) -> bool {
        !matches!(self, Self::LegacyPublic)
    }
}

fn create_pod_lifecycle_locked(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    request: CreatePodLifecycleRequest,
    proposer: Option<AgentHarnessId>,
    mode: PodCreationMode,
) -> Result<CreatedPodPackage, AgentToolsError> {
    let mut staged = store.clone();
    let created = stage_pod_lifecycle(&mut staged, ctx, request, proposer, mode)?;
    *store = staged;
    Ok(created)
}

fn stage_pod_lifecycle(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    request: CreatePodLifecycleRequest,
    proposer: Option<AgentHarnessId>,
    mode: PodCreationMode,
) -> Result<CreatedPodPackage, AgentToolsError> {
    validate_creation_package_locked(store, ctx, &request.package)?;
    if store
        .pods
        .values()
        .any(|pod| pod.slug == request.pod.slug && pod.tenant_id == ctx.tenant_id)
    {
        return Err(StoreError::Duplicate(format!("pod {}", request.pod.slug)).into());
    }
    let node = store.node_for_tenant(ctx.tenant_id)?;
    let proposer_user_id = || {
        proposer.and_then(|id| {
            store
                .agent_harnesses
                .get(&id)
                .map(|harness| harness.user_id)
        })
    };
    let owner_id = match mode {
        PodCreationMode::PrivatePackage => Some(ctx.user_id.ok_or_else(|| {
            StoreError::Validation("private Pod Package requires an owner".to_string())
        })?),
        PodCreationMode::Canonical => Some(
            proposer_user_id()
                .or(ctx.user_id)
                .ok_or_else(|| StoreError::Validation("Pod creation requires an owner".into()))?,
        ),
        PodCreationMode::SimpleCreate => Some(
            proposer_user_id()
                .or(ctx.user_id)
                .or_else(|| store.users.keys().next().copied())
                .ok_or_else(|| StoreError::Validation("Pod creation requires an owner".into()))?,
        ),
        PodCreationMode::LegacyPublic => proposer_user_id(),
    };
    let now = Utc::now();
    let pod = Pod {
        id: Uuid::now_v7(),
        tenant_id: ctx.tenant_id,
        name: request.pod.name,
        slug: request.pod.slug,
        description: request.pod.description,
        visibility: request.pod.visibility,
        created_by: owner_id,
        created_at: now,
        origin_node_id: Some(node.id),
    };
    let mut package = match request.package {
        PodCreationPackage::Default => default_skill_pack(&pod),
        PodCreationPackage::Initial { package } => PodSkillPack {
            id: Uuid::now_v7(),
            pod_id: pod.id,
            version: 1,
            context_md: package.context_md,
            pod_yaml: format!(
                "name: {}\nslug: {}\ndescription: {}\nvisibility: {}\n",
                pod.name,
                pod.slug,
                pod.description,
                match pod.visibility {
                    Visibility::Public => "public",
                    Visibility::InviteOnly => "invite_only",
                    Visibility::Private => "private",
                }
            ),
            skill_md: package.skill_md,
            sources_yaml: package.sources_yaml,
            filters_yaml: package.filters_yaml,
            examples_good_md: package.examples_good_md,
            examples_bad_md: package.examples_bad_md,
            owner_id,
            proposer_harness_id: proposer,
            created_at: now,
            updated_at: now,
        },
        PodCreationPackage::Derived { source_package } => fork_skill_pack(&source_package, &pod),
    };
    package.version = 1;
    package.proposer_harness_id = proposer;
    store.pods.insert(pod.id, pod.clone());
    store.pod_rules.insert(
        pod.id,
        PodRules {
            pod_id: pod.id,
            blocked_topics: Vec::new(),
            blocked_domains: Vec::new(),
            auto_promote_crawler_candidates: false,
            federate_sources: pod.visibility == Visibility::Public,
        },
    );
    if let Some(owner_id) = owner_id {
        store.pod_roles.push(PodRoleAssignment {
            user_id: owner_id,
            pod_id: pod.id,
            role: PodRole::Owner,
            created_at: now,
        });
    }
    store.insert_pod_package_version(package.clone())?;
    store.pod_skill_packs.insert(pod.id, package.clone());
    let event = sign_public_event(
        &node,
        mode.event_type(),
        &pod.slug,
        json!({"pod": pod, "package": package}),
        store.latest_event_hash(&pod.slug),
    )?;
    store.event_log.push(event);
    if mode.records_audit() {
        record_harness_write(store, ctx, HarnessWriteOperation::CreatePod, Some(pod.id));
    }
    Ok(CreatedPodPackage { pod, package })
}

fn expire_proposal(
    store: &mut InMemoryStore,
    proposal_id: PendingProposalId,
    now: chrono::DateTime<Utc>,
) -> Result<(), AgentToolsError> {
    let proposal = store
        .pending_proposals
        .get_mut(&proposal_id)
        .ok_or_else(|| StoreError::NotFound(format!("Pending Proposal {proposal_id}")))?;
    if proposal.status == ProposalStatus::Pending && now >= proposal.expires_at {
        proposal.status = ProposalStatus::Expired;
        proposal.decided_at = Some(now);
    }
    Ok(())
}

fn validate_structured_diff(
    store: &InMemoryStore,
    proposal: &PendingProposal,
) -> Result<(), AgentToolsError> {
    for difference in &proposal.structured_diff {
        let current = match &difference.resource {
            ProposalResource::Pod(pod_id) => store.pods.get(pod_id).map_or(
                serde_json::Value::Null,
                |pod| json!({"visibility": pod.visibility}),
            ),
            ProposalResource::PodSlug(slug) => store
                .pods
                .values()
                .find(|pod| pod.tenant_id == proposal.tenant_id && pod.slug == *slug)
                .map_or(serde_json::Value::Null, |pod| json!(pod)),
            ProposalResource::AgentHarness(harness_id) => store
                .agent_harnesses
                .get(harness_id)
                .map_or(serde_json::Value::Null, |harness| json!(harness.grant)),
            ProposalResource::TrustedPeerUrl(base_url) => store
                .trusted_peers
                .values()
                .find(|peer| peer.tenant_id == proposal.tenant_id && peer.base_url == *base_url)
                .map_or(serde_json::Value::Null, |peer| json!(peer)),
            ProposalResource::TrustPolicy(user_id) => json!(store
                .trust_policies
                .get(&(*user_id, proposal.tenant_id))
                .cloned()
                .unwrap_or_else(|| TrustPolicy::new(*user_id, proposal.tenant_id))),
            ProposalResource::PodPackage(pod_id) => store
                .pod_skill_packs
                .get(pod_id)
                .map_or(serde_json::Value::Null, |package| json!(package)),
            ProposalResource::PodCurationPolicy(pod_id) => json!({
                "curation_policy": store
                    .pod_curation_policies
                    .get(pod_id)
                    .copied()
                    .unwrap_or_default()
            }),
            ProposalResource::PodRoles(pod_id) => pod_roles_value(store, *pod_id),
            ProposalResource::SubmissionPlacement {
                pod_id,
                submission_id,
            } => json!({
                "accepted": store.submission_pods.iter().any(|placement| {
                    placement.pod_id == *pod_id && placement.submission_id == *submission_id
                })
            }),
        };
        if current != difference.before {
            return Err(StoreError::Validation(
                "proposal structured diff is stale; create a new Pending Proposal".to_string(),
            )
            .into());
        }
    }
    Ok(())
}

fn pod_roles_value(store: &InMemoryStore, pod_id: PodId) -> serde_json::Value {
    let mut roles = store
        .pod_roles
        .iter()
        .filter(|assignment| assignment.pod_id == pod_id)
        .cloned()
        .collect::<Vec<_>>();
    roles.sort_by_key(|assignment| {
        (
            assignment.user_id,
            match assignment.role {
                PodRole::Owner => 0,
                PodRole::Curator => 1,
            },
        )
    });
    json!(roles)
}

fn authorize_pod_role_owner(
    store: &InMemoryStore,
    ctx: &AuthContext,
    pod_id: PodId,
) -> Result<(), AgentToolsError> {
    authorize_harness(store, ctx, HarnessCapability::PodCuration, Some(pod_id))?;
    if ctx.user_id.is_some_and(|user_id| {
        store.pod_roles.iter().any(|assignment| {
            assignment.user_id == user_id
                && assignment.pod_id == pod_id
                && assignment.role == PodRole::Owner
        })
    }) {
        Ok(())
    } else {
        Err(AgentToolsError::Forbidden {
            reason: format!("User is not an Owner of Pod {pod_id}"),
        })
    }
}

fn apply_sensitive_change(
    store: &mut InMemoryStore,
    ctx: &AuthContext,
    proposer: AgentHarnessId,
    requested_change: &SensitiveChange,
) -> Result<(), AgentToolsError> {
    match requested_change {
        SensitiveChange::CreatePublicPod { request } => {
            create_pod_lifecycle_locked(
                store,
                ctx,
                CreatePodLifecycleRequest {
                    pod: request.clone(),
                    package: PodCreationPackage::Default,
                },
                Some(proposer),
                PodCreationMode::LegacyPublic,
            )?;
        }
        SensitiveChange::CreatePublicPodLifecycle { request } => {
            create_pod_lifecycle_locked(
                store,
                ctx,
                request.clone(),
                Some(proposer),
                PodCreationMode::Canonical,
            )?;
        }
        SensitiveChange::PublishPod { pod_id } => {
            let tenant_id = store
                .pods
                .get(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?
                .tenant_id;
            store.assert_tenant(tenant_id, ctx.tenant_id)?;
            let pod = store
                .pods
                .get_mut(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            if pod.visibility == Visibility::Public {
                return Err(StoreError::Validation("Pod is already public".to_string()).into());
            }
            pod.visibility = Visibility::Public;
            let pod = pod.clone();
            if let Some(rules) = store.pod_rules.get_mut(pod_id) {
                rules.federate_sources = true;
            }
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let package = store
                .pod_skill_packs
                .get(pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("Pod Package".to_string()))?;
            let event = sign_public_event(
                &node,
                "pod_published",
                &pod.slug,
                json!({"pod": pod, "package": package}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.event_log.push(event);
        }
        SensitiveChange::ExpandPodVisibility { pod_id, visibility } => {
            let tenant_id = store
                .pods
                .get(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?
                .tenant_id;
            store.assert_tenant(tenant_id, ctx.tenant_id)?;
            let pod = store
                .pods
                .get_mut(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            if visibility_exposure(visibility) <= visibility_exposure(&pod.visibility) {
                return Err(StoreError::Validation(
                    "approved visibility must expand exposure".into(),
                )
                .into());
            }
            pod.visibility = visibility.clone();
            let pod = pod.clone();
            if let Some(rules) = store.pod_rules.get_mut(pod_id) {
                rules.federate_sources = *visibility == Visibility::Public;
            }
            if *visibility == Visibility::Public {
                let node = store.node_for_tenant(ctx.tenant_id)?;
                let package = store
                    .pod_skill_packs
                    .get(pod_id)
                    .cloned()
                    .ok_or_else(|| StoreError::NotFound("Pod Package".to_string()))?;
                let event = sign_public_event(
                    &node,
                    "pod_published",
                    &pod.slug,
                    json!({"pod": pod, "package": package}),
                    store.latest_event_hash(&pod.slug),
                )?;
                store.event_log.push(event);
            }
        }
        SensitiveChange::ExpandHarnessGrant {
            harness_id,
            capabilities,
            pod_ids,
        } => {
            let target = store
                .agent_harnesses
                .get_mut(harness_id)
                .ok_or_else(|| StoreError::NotFound(format!("Agent Harness {harness_id}")))?;
            if target.tenant_id != ctx.tenant_id {
                return Err(StoreError::TenantBoundary.into());
            }
            let requested_capabilities = normalize_capabilities(capabilities.clone());
            if target
                .grant
                .capabilities
                .iter()
                .any(|capability| !requested_capabilities.contains(capability))
                || !grant_scope_expands(&target.grant.pod_ids, pod_ids)
            {
                return Err(StoreError::Validation(
                    "Harness Grant changed after proposal creation".to_string(),
                )
                .into());
            }
            target.grant.capabilities = requested_capabilities;
            target.grant.pod_ids = pod_ids.clone().map(normalize_pod_ids);
        }
        SensitiveChange::AddTrustedPeer {
            node_id,
            display_name,
            base_url,
            public_key,
        } => {
            if store.trusted_peers.values().any(|peer| {
                peer.tenant_id == ctx.tenant_id
                    && (peer.base_url == *base_url
                        || (!node_id.is_nil() && peer.node_id == *node_id))
            }) {
                return Err(StoreError::Duplicate(format!("trusted peer {base_url}")).into());
            }
            let peer = TrustedPeer {
                id: Uuid::now_v7(),
                node_id: *node_id,
                tenant_id: ctx.tenant_id,
                display_name: display_name.clone(),
                base_url: base_url.clone(),
                public_key: public_key.clone(),
                trust_level: TrustLevel::ReadOnly,
                enabled: true,
                created_at: Utc::now(),
            };
            store.trusted_peers.insert(peer.id, peer);
        }
        SensitiveChange::RemoveTrustedPeer { peer_id } => {
            let peer = store
                .trusted_peers
                .get_mut(peer_id)
                .ok_or_else(|| StoreError::NotFound(format!("trusted peer {peer_id}")))?;
            if peer.tenant_id != ctx.tenant_id {
                return Err(StoreError::TenantBoundary.into());
            }
            if !peer.enabled {
                return Err(
                    StoreError::Validation("trusted peer is already disabled".into()).into(),
                );
            }
            peer.enabled = false;
        }
        SensitiveChange::ChangeTrustPolicy { change } => {
            let user_id = store
                .agent_harnesses
                .get(&proposer)
                .map(|harness| harness.user_id)
                .ok_or_else(|| StoreError::NotFound(format!("Agent Harness {proposer}")))?;
            let key = (user_id, ctx.tenant_id);
            let mut policy = store
                .trust_policies
                .get(&key)
                .cloned()
                .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
            apply_trust_policy_change(&mut policy, change)?;
            store.trust_policies.insert(key, policy);
        }
        SensitiveChange::RevisePublicPodPackage {
            pod_id,
            base_version,
            patch,
        } => {
            let pod = store
                .pods
                .get(pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            let existing = store
                .pod_skill_packs
                .get(pod_id)
                .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
            if PackageVersion::new(existing.version)
                .map_err(|error| StoreError::Validation(error.to_string()))?
                != *base_version
            {
                return Err(StoreError::Validation(
                    "public Package Revision base version is stale".to_string(),
                )
                .into());
            }
            let mut package = patch_skill_pack(existing, patch.clone());
            let validation = validate_skill_pack(&package);
            if !validation.valid {
                return Err(StoreError::Validation(validation.errors.join(", ")).into());
            }
            let now = Utc::now();
            package.created_at = now;
            package.updated_at = now;
            package.proposer_harness_id = Some(proposer);
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let event = sign_public_event(
                &node,
                "pod_skill_pack_updated",
                &pod.slug,
                json!({"package": package}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.insert_pod_package_version(package.clone())?;
            store.pod_skill_packs.insert(*pod_id, package);
            store.event_log.push(event);
            refresh_public_pod_announcement_if_needed(store, *pod_id, now)?;
        }
        SensitiveChange::RemovePublicSubmissionFromPod {
            pod_id,
            submission_id,
        } => {
            let pod = store
                .pods
                .get(pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            let content_item_id = ContentItemId::from(*submission_id);
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let origin_placement = store
                .accepted_placement_projections
                .get(&(content_item_id, *pod_id))
                .cloned();
            let before = store.submission_pods.len();
            store.submission_pods.retain(|placement| {
                !(placement.pod_id == *pod_id && placement.submission_id == *submission_id)
            });
            if store.submission_pods.len() == before {
                return Err(StoreError::Validation(
                    "public Pod Placement changed after proposal creation".to_string(),
                )
                .into());
            }
            let withdrawn_at = Utc::now();
            if let Some(placement) = store.pod_placements.values_mut().find(|placement| {
                placement.pod_id == *pod_id
                    && placement.content_item_id == Some((*submission_id).into())
                    && placement.status == PodPlacementStatus::Accepted
            }) {
                let actor = curation_actor(ctx);
                placement.status = PodPlacementStatus::Reversed;
                placement.curation_path = CurationPath::ManualReview;
                placement.actor = actor;
                placement.updated_at = withdrawn_at;
                placement.audit_history.push(PlacementAuditEntry {
                    status: PodPlacementStatus::Reversed,
                    curation_path: CurationPath::ManualReview,
                    actor,
                    note: Some(CurationRationale::new(
                        "approved public placement reversal",
                    )?),
                    occurred_at: withdrawn_at,
                });
            }
            let event = if let Some(origin_placement) = origin_placement {
                let content_reference = store
                    .submissions
                    .get(submission_id)
                    .map(feed_content_reference)
                    .ok_or_else(|| StoreError::NotFound("Content Reference".into()))?;
                let tombstone = PlacementTombstone {
                    content_reference,
                    origin_placement,
                    withdrawn_at,
                };
                let tombstoned_origin_id = origin_placement_identity(&tombstone.origin_placement);
                for placement in store.pod_placements.values_mut().filter(|placement| {
                    placement.content_item_id == Some(content_item_id)
                        && placement
                            .origin_placements
                            .iter()
                            .map(origin_placement_identity)
                            .collect::<HashSet<_>>()
                            .contains(&tombstoned_origin_id)
                }) {
                    placement.origin_withdrawals.push(tombstone.clone());
                }
                store
                    .accepted_placement_projections
                    .remove(&(content_item_id, *pod_id));
                store.placement_tombstones.push(tombstone.clone());
                sign_public_event(
                    &node,
                    FederatedPodEventType::PlacementTombstoned.as_wire(),
                    &pod.slug,
                    json!({"placement_tombstone": tombstone}),
                    store.latest_event_hash(&pod.slug),
                )?
            } else {
                sign_public_event(
                    &node,
                    FederatedPodEventType::LegacyLinkRemoved.as_wire(),
                    &pod.slug,
                    json!({"submission_id": submission_id, "submission_purged": false}),
                    store.latest_event_hash(&pod.slug),
                )?
            };
            store.event_log.push(event);
            refresh_public_pod_announcement_if_needed(store, *pod_id, withdrawn_at)?;
        }
        SensitiveChange::EnableAutonomousCuration {
            pod_id,
            confidence_threshold,
        } => {
            let pod = store
                .pods
                .get(pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            store.pod_curation_policies.insert(
                *pod_id,
                CurationPolicy::Autonomous {
                    confidence_threshold: *confidence_threshold,
                },
            );
        }
        SensitiveChange::GrantPodRole {
            pod_id,
            user_id,
            role,
        } => {
            store.pod_roles.retain(|assignment| {
                assignment.pod_id != *pod_id || assignment.user_id != *user_id
            });
            store.pod_roles.push(PodRoleAssignment {
                user_id: *user_id,
                pod_id: *pod_id,
                role: role.clone(),
                created_at: Utc::now(),
            });
        }
        SensitiveChange::RevokePodRole {
            pod_id,
            user_id,
            role,
        } => {
            let before = store.pod_roles.len();
            store.pod_roles.retain(|assignment| {
                assignment.pod_id != *pod_id
                    || assignment.user_id != *user_id
                    || assignment.role != *role
            });
            if store.pod_roles.len() == before {
                return Err(StoreError::NotFound(format!("Pod Role for User {user_id}")).into());
            }
        }
    }
    Ok(())
}

fn grant_scope_expands(current: &Option<Vec<PodId>>, requested: &Option<Vec<PodId>>) -> bool {
    match (current, requested) {
        (None, None) | (Some(_), None) => true,
        (Some(current), Some(requested)) => current.iter().all(|pod_id| requested.contains(pod_id)),
        (None, Some(_)) => false,
    }
}

fn ensure_child_pod_scope(
    parent: &Option<Vec<PodId>>,
    child: &Option<Vec<PodId>>,
) -> Result<(), AgentToolsError> {
    match (parent, child) {
        (Some(_), None) => Err(AgentToolsError::Forbidden {
            reason: "a harness cannot delegate a broader Pod scope".to_string(),
        }),
        (Some(parent), Some(child)) if child.iter().any(|pod_id| !parent.contains(pod_id)) => {
            Err(AgentToolsError::Forbidden {
                reason: "a harness cannot delegate a broader Pod scope".to_string(),
            })
        }
        _ => Ok(()),
    }
}

fn effective_user_id(ctx: &AuthContext, requested: Option<UserId>) -> Option<UserId> {
    if ctx.harness_id.is_some() {
        ctx.user_id
    } else {
        requested.or(ctx.user_id)
    }
}

fn normalize_pod_ids(mut pod_ids: Vec<PodId>) -> Vec<PodId> {
    pod_ids.sort();
    pod_ids.dedup();
    pod_ids
}

fn route_tokens(text: &str) -> Vec<String> {
    discovery_tokens(text)
}

#[cfg(test)]
mod federation_projection_tests {
    use super::*;

    fn context(tenant_id: TenantId) -> AuthContext {
        AuthContext {
            user_id: None,
            tenant_id: Some(tenant_id),
            node_id: Uuid::now_v7(),
            harness_id: None,
        }
    }

    fn public_pod(tenant_id: TenantId, slug: &str) -> Pod {
        Pod {
            id: Uuid::now_v7(),
            tenant_id: Some(tenant_id),
            name: slug.to_string(),
            slug: slug.to_string(),
            description: String::new(),
            visibility: Visibility::Public,
            created_by: None,
            created_at: Utc::now(),
            origin_node_id: None,
        }
    }

    fn submission(id: SubmissionId, tenant_id: TenantId, canonical_url: &str) -> Submission {
        Submission {
            id,
            tenant_id: Some(tenant_id),
            url: canonical_url.to_string(),
            canonical_url: canonical_url.to_string(),
            title: "Federated item".to_string(),
            description: None,
            domain: "example.com".to_string(),
            submitted_by: None,
            discovered_by_crawler: false,
            submitter_note: None,
            summary: None,
            media_references: Vec::new(),
            tags: Vec::new(),
            embedding: None,
            created_at: Utc::now(),
            origin_event_id: None,
        }
    }

    fn placement_event(
        origin_node_id: NodeIdentityId,
        pod: &Pod,
        origin_submission: &Submission,
    ) -> EventLog {
        EventLog {
            event_id: Uuid::now_v7(),
            tenant_id: None,
            event_type: "content_item_placed".to_string(),
            pod_slug: pod.slug.clone(),
            author_node_id: origin_node_id,
            author_display_name: None,
            payload_json: json!({
                "content_item": ContentItem::from(origin_submission),
                "accepted_placement": AcceptedPlacementProjection {
                    content_item_id: ContentItemId::from(origin_submission.id),
                    pod_id: pod.id,
                    reason: CurationRationale::new("Federated acceptance").unwrap(),
                    curation_path: CurationPath::ManualReview,
                    origin_node_id,
                    accepted_at: Utc::now(),
                },
            }),
            created_at: Utc::now(),
            previous_event_hash: None,
            content_hash: String::new(),
            signature: String::new(),
            imported_from_peer_id: None,
            verified: true,
        }
    }

    fn removal_event(
        origin_node_id: NodeIdentityId,
        pod: &Pod,
        origin_submission: &Submission,
        placed: Option<&EventLog>,
    ) -> EventLog {
        let origin_placement = placed
            .and_then(|event| {
                serde_json::from_value(event.payload_json["accepted_placement"].clone()).ok()
            })
            .unwrap_or(AcceptedPlacementProjection {
                content_item_id: origin_submission.id.into(),
                pod_id: pod.id,
                reason: CurationRationale::new("Federated acceptance").unwrap(),
                curation_path: CurationPath::ManualReview,
                origin_node_id,
                accepted_at: Utc::now(),
            });
        let tombstone = PlacementTombstone {
            content_reference: feed_content_reference(origin_submission),
            origin_placement,
            withdrawn_at: Utc::now(),
        };
        EventLog {
            event_id: Uuid::now_v7(),
            tenant_id: None,
            event_type: FederatedPodEventType::PlacementTombstoned
                .as_wire()
                .to_string(),
            pod_slug: pod.slug.clone(),
            author_node_id: origin_node_id,
            author_display_name: None,
            payload_json: json!({ "placement_tombstone": tombstone }),
            created_at: Utc::now(),
            previous_event_hash: None,
            content_hash: String::new(),
            signature: String::new(),
            imported_from_peer_id: None,
            verified: true,
        }
    }

    #[test]
    fn federated_tombstones_resolve_ids_within_the_importing_tenant() {
        let tenant_a = Uuid::now_v7();
        let tenant_b = Uuid::now_v7();
        let ctx_a = context(tenant_a);
        let ctx_b = context(tenant_b);
        let origin_node_id = Uuid::now_v7();
        let mut pod_a = public_pod(tenant_a, "shared-pod");
        pod_a.origin_node_id = Some(origin_node_id);
        let mut pod_b = public_pod(tenant_b, "shared-pod");
        pod_b.origin_node_id = Some(origin_node_id);
        let origin_submission_id = Uuid::now_v7();
        let origin_submission =
            submission(origin_submission_id, tenant_a, "https://example.com/item");
        let local_a = submission(Uuid::now_v7(), tenant_a, &origin_submission.canonical_url);
        let local_b = submission(Uuid::now_v7(), tenant_b, &origin_submission.canonical_url);
        let mut store = InMemoryStore::default();
        store.pods.insert(pod_a.id, pod_a.clone());
        store.pods.insert(pod_b.id, pod_b.clone());
        store.submissions.insert(local_a.id, local_a.clone());
        store.submissions.insert(local_b.id, local_b.clone());

        let placed = placement_event(origin_node_id, &pod_a, &origin_submission);
        project_imported_public_event(&mut store, &ctx_a, &placed).unwrap();
        project_imported_public_event(&mut store, &ctx_b, &placed).unwrap();
        project_imported_public_event(
            &mut store,
            &ctx_a,
            &removal_event(origin_node_id, &pod_a, &origin_submission, Some(&placed)),
        )
        .unwrap();

        assert!(!store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod_a.id && link.submission_id == local_a.id));
        assert!(store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod_b.id && link.submission_id == local_b.id));
    }

    #[test]
    fn unmapped_federated_tombstone_never_treats_an_origin_id_as_local() {
        let tenant_id = Uuid::now_v7();
        let ctx = context(tenant_id);
        let pod = public_pod(tenant_id, "unmapped-pod");
        let origin_node_id = Uuid::now_v7();
        let coincident_id = Uuid::now_v7();
        let local = submission(coincident_id, tenant_id, "https://local.example/item");
        let mut store = InMemoryStore::default();
        store.pods.insert(pod.id, pod.clone());
        store.submissions.insert(local.id, local.clone());
        store.submission_pods.push(SubmissionPod {
            submission_id: local.id,
            pod_id: pod.id,
            created_at: Utc::now(),
        });

        project_imported_public_event(
            &mut store,
            &ctx,
            &removal_event(origin_node_id, &pod, &local, None),
        )
        .unwrap();

        assert!(store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod.id && link.submission_id == local.id));
    }

    #[test]
    fn federated_content_id_collision_cannot_alias_a_same_tenant_item() {
        let tenant_id = Uuid::now_v7();
        let ctx = context(tenant_id);
        let origin_node_id = Uuid::now_v7();
        let mut pod = public_pod(tenant_id, "remote-collision-pod");
        pod.origin_node_id = Some(origin_node_id);
        let origin_id = Uuid::now_v7();
        let local = submission(origin_id, tenant_id, "https://local.example/item");
        let remote = submission(origin_id, tenant_id, "https://remote.example/item");
        let mut store = InMemoryStore::default();
        store.pods.insert(pod.id, pod.clone());
        store.submissions.insert(local.id, local.clone());

        project_imported_public_event(
            &mut store,
            &ctx,
            &placement_event(origin_node_id, &pod, &remote),
        )
        .unwrap();

        assert_eq!(
            store.submissions.get(&local.id).unwrap().canonical_url,
            local.canonical_url
        );
        let mapped = store
            .federated_content_item_ids
            .get(&FederatedContentItemKey::new(
                Some(tenant_id),
                origin_node_id,
                ContentItemId::from(origin_id),
            ))
            .copied()
            .unwrap();
        assert_ne!(Uuid::from(mapped), origin_id);
        assert_eq!(
            store
                .submissions
                .get(&Uuid::from(mapped))
                .unwrap()
                .canonical_url,
            remote.canonical_url
        );
    }

    #[test]
    fn federated_content_id_collision_cannot_overwrite_another_tenant() {
        let tenant_a = Uuid::now_v7();
        let tenant_b = Uuid::now_v7();
        let ctx_b = context(tenant_b);
        let origin_node_id = Uuid::now_v7();
        let mut pod_b = public_pod(tenant_b, "tenant-b-remote-pod");
        pod_b.origin_node_id = Some(origin_node_id);
        let origin_id = Uuid::now_v7();
        let tenant_a_item = submission(origin_id, tenant_a, "https://tenant-a.example/item");
        let remote = submission(origin_id, tenant_b, "https://remote.example/tenant-b-item");
        let mut store = InMemoryStore::default();
        store.pods.insert(pod_b.id, pod_b.clone());
        store
            .submissions
            .insert(tenant_a_item.id, tenant_a_item.clone());

        project_imported_public_event(
            &mut store,
            &ctx_b,
            &placement_event(origin_node_id, &pod_b, &remote),
        )
        .unwrap();

        assert_eq!(
            store.submissions.get(&tenant_a_item.id).unwrap().tenant_id,
            Some(tenant_a)
        );
        let tenant_b_item = store
            .submissions
            .values()
            .find(|item| {
                item.tenant_id == Some(tenant_b) && item.canonical_url == remote.canonical_url
            })
            .unwrap();
        assert_ne!(tenant_b_item.id, origin_id);
    }

    #[test]
    fn federated_content_deduplicates_canonical_urls_only_within_the_tenant() {
        let tenant_id = Uuid::now_v7();
        let ctx = context(tenant_id);
        let origin_node_id = Uuid::now_v7();
        let mut pod = public_pod(tenant_id, "canonical-dedupe-pod");
        pod.origin_node_id = Some(origin_node_id);
        let local = submission(Uuid::now_v7(), tenant_id, "https://canonical.example/item");
        let remote = submission(Uuid::now_v7(), tenant_id, "https://canonical.example/item");
        let mut store = InMemoryStore::default();
        store.pods.insert(pod.id, pod.clone());
        store.submissions.insert(local.id, local.clone());

        project_imported_public_event(
            &mut store,
            &ctx,
            &placement_event(origin_node_id, &pod, &remote),
        )
        .unwrap();

        assert_eq!(store.submissions.len(), 1);
        assert_eq!(
            store
                .federated_content_item_ids
                .get(&FederatedContentItemKey::new(
                    Some(tenant_id),
                    origin_node_id,
                    ContentItemId::from(remote.id),
                ))
                .copied(),
            Some(ContentItemId::from(local.id))
        );
    }
}
