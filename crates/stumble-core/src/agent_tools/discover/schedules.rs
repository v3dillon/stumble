use super::super::prelude::*;
use super::super::*;

impl AgentTools {
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
}
