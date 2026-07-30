use super::super::prelude::*;
use super::super::*;

impl AgentTools {
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

}
