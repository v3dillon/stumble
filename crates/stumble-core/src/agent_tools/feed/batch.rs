use super::super::prelude::*;
use super::super::*;

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
}
