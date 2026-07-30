use super::super::prelude::*;
use super::super::*;

pub(crate) struct FeedItemSelection {
    pub(crate) recurrence_penalty_applied: bool,
    pub(crate) attention_value: f32,
    pub(crate) reasons: Vec<String>,
    pub(crate) kind: FeedItemKind,
}

pub(crate) fn authorize_taste_profile(
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

pub(crate) fn authorize_interactive_user_action(
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

pub(crate) fn authorize_feed_item_scope(
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

pub(crate) fn taste_profile_from_store(
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

pub(crate) fn feed_content_reference(item: &Submission) -> FeedContentReference {
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

pub(crate) fn feed_batch_item(
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

pub(crate) fn project_feed_batch_for_context(
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

pub(crate) fn feed_attention_value(
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

pub(crate) fn taste_evidence_for_feedback(
    kind: FeedbackKind,
) -> Option<(LearnedTasteEvidenceKind, TasteEvidenceDirection)> {
    // Explicit feed feedback adjusts durable taste. Blocks update preferences
    // directly rather than learned weights. Personal Discovery batch dismiss is
    // a separate path that never calls this helper.
    match kind {
        FeedbackKind::Saved => Some((
            LearnedTasteEvidenceKind::Save,
            TasteEvidenceDirection::Supporting,
        )),
        FeedbackKind::Interesting => Some((
            LearnedTasteEvidenceKind::MoreLikeThis,
            TasteEvidenceDirection::Supporting,
        )),
        FeedbackKind::NotForMe | FeedbackKind::Dismissed => Some((
            LearnedTasteEvidenceKind::LessLikeThis,
            TasteEvidenceDirection::Opposing,
        )),
        FeedbackKind::BlockSource | FeedbackKind::BlockTopic => None,
    }
}

pub(crate) fn record_taste_learning_evidence(
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

pub(crate) fn record_add_to_pod_learning(
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

pub(crate) fn feed_allowed_actions(
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

pub(crate) fn feed_feedback_state(
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
