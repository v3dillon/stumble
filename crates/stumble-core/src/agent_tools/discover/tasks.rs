use super::super::prelude::*;
use super::super::*;

impl AgentTools {
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

    pub(crate) fn finish_discovery_task(
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

}
