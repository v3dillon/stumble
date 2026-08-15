//! Named private Personal Discovery schedules, materialization, and backpressure.

use super::{build_plan, prepare_schedule_run, readiness, stamp_planned_watches};
use crate::domain::*;
use crate::store::InMemoryStore;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Validates a create/update schedule result count (1..=100).
pub(crate) fn validate_result_count(result_count: u16) -> Result<u16, String> {
    if (1..=100).contains(&result_count) {
        Ok(result_count)
    } else {
        Err("Personal Discovery schedule result count must be between 1 and 100".into())
    }
}

/// Validates and normalizes a schedule name.
pub(crate) fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("schedule name must not be empty".into());
    }
    if name.len() > 120 {
        return Err("schedule name must be at most 120 characters".into());
    }
    Ok(name)
}

/// Normalizes temporary focus/avoidance topics for a schedule.
pub(crate) fn normalize_intent(
    intent: PersonalDiscoveryScheduleIntent,
) -> Result<PersonalDiscoveryScheduleIntent, String> {
    let focus_topics = normalize_topics(intent.focus_topics, "focus")?;
    let avoid_topics = normalize_topics(intent.avoid_topics, "avoid")?;
    for focus in &focus_topics {
        if avoid_topics
            .iter()
            .any(|avoid| avoid.eq_ignore_ascii_case(focus))
        {
            return Err(format!(
                "focus topic {focus:?} cannot also appear in avoid topics"
            ));
        }
    }
    Ok(PersonalDiscoveryScheduleIntent {
        focus_topics,
        avoid_topics,
    })
}

fn normalize_topics(topics: Vec<String>, label: &str) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for topic in topics {
        let value = topic.trim().to_lowercase();
        if value.is_empty() {
            return Err(format!("{label} topic must not be empty"));
        }
        if !normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&value))
        {
            normalized.push(value);
        }
    }
    Ok(normalized)
}

/// Returns inspectable backpressure for one schedule.
pub(crate) fn schedule_backpressure(
    store: &InMemoryStore,
    schedule_id: PersonalDiscoveryScheduleId,
) -> PersonalDiscoveryScheduleBackpressure {
    let mut schedule_tasks: Vec<&DiscoveryTask> = store
        .discovery_tasks
        .values()
        .filter(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled {
                    schedule_id: id
                } if id == schedule_id
            )
        })
        .collect();
    schedule_tasks.sort_by_key(|task| (task.created_at, task.id));

    for task in &schedule_tasks {
        match &task.state {
            DiscoveryTaskState::Pending | DiscoveryTaskState::Leased(_) => {
                return PersonalDiscoveryScheduleBackpressure::InFlightTask { task_id: task.id };
            }
            DiscoveryTaskState::Completed | DiscoveryTaskState::TerminalFailure => {}
        }
        if let Some(batch) = store.discovery_result_batches.values().find(|batch| {
            batch.task_id == task.id && batch.state == DiscoveryResultBatchState::Ready
        }) {
            return PersonalDiscoveryScheduleBackpressure::UnreviewedBatch {
                batch_id: batch.id,
                task_id: task.id,
            };
        }
    }
    PersonalDiscoveryScheduleBackpressure::None
}

/// Builds the inspectable status view for one schedule at `now`.
pub(crate) fn schedule_status(
    store: &InMemoryStore,
    schedule: PersonalDiscoverySchedule,
    now: DateTime<Utc>,
) -> PersonalDiscoveryScheduleStatus {
    let readiness_dormant = !readiness(store, schedule.user_id, schedule.tenant_id).ready;
    let current_period_start = schedule.cadence.period_start(now);
    let current_period_task_id = store.discovery_tasks.values().find_map(|task| {
        matches!(
            &task.origin,
            DiscoveryTaskOrigin::PersonalScheduled {
                schedule_id
            } if *schedule_id == schedule.id
        )
        .then_some(())
        .filter(|_| task.due_at == current_period_start)
        .map(|_| task.id)
    });
    PersonalDiscoveryScheduleStatus {
        backpressure: schedule_backpressure(store, schedule.id),
        readiness_dormant,
        current_period_start,
        current_period_task_id,
        schedule,
    }
}

/// Idempotently materializes due Personal Discovery schedule tasks for one User.
///
/// Skips disabled schedules, cold-start dormancy, backpressure, and periods that
/// already have a task. Builds each plan from the User's current private profile.
pub(crate) fn materialize_due_personal_schedules(
    store: &mut InMemoryStore,
    user_id: UserId,
    tenant_id: Option<TenantId>,
    now: DateTime<Utc>,
) -> Result<Vec<DiscoveryTask>, crate::agent_tools::AgentToolsError> {
    let ready = readiness(store, user_id, tenant_id).ready;
    let mut schedules: Vec<PersonalDiscoverySchedule> = store
        .personal_discovery_schedules
        .values()
        .filter(|schedule| {
            schedule.user_id == user_id && schedule.tenant_id == tenant_id && schedule.enabled
        })
        .cloned()
        .collect();
    schedules.sort_by_key(|schedule| (schedule.name.clone(), schedule.id));

    let mut created = Vec::new();
    for schedule in schedules {
        if !ready {
            continue;
        }
        if !matches!(
            schedule_backpressure(store, schedule.id),
            PersonalDiscoveryScheduleBackpressure::None
        ) {
            continue;
        }
        let due_at = schedule.cadence.period_start(now);
        let exists = store.discovery_tasks.values().any(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled {
                    schedule_id
                } if schedule_id == schedule.id
            ) && task.due_at == due_at
        });
        if exists {
            continue;
        }
        let prepared = prepare_schedule_run(&schedule)?;
        let plan = build_plan(store, user_id, tenant_id, prepared, now)?;
        let task = DiscoveryTask {
            id: Uuid::now_v7().into(),
            target: DiscoveryTaskTarget::Personal {
                discovery_plan_id: plan.id,
            },
            origin: DiscoveryTaskOrigin::PersonalScheduled {
                schedule_id: schedule.id,
            },
            due_at,
            state: DiscoveryTaskState::Pending,
            attempts: Vec::new(),
            created_at: now,
        };
        stamp_planned_watches(store, &plan, now);
        store.discovery_plans.insert(plan.id, plan);
        store.discovery_tasks.insert(task.id, task.clone());
        created.push(task);
    }
    Ok(created)
}

/// Whether a completed batch should start in Pending notification state.
pub(crate) fn notification_state_for_schedule(
    delivery_mode: PersonalDiscoveryDeliveryMode,
) -> DiscoveryResultNotificationState {
    match delivery_mode {
        PersonalDiscoveryDeliveryMode::NotifyWhenSupported => {
            DiscoveryResultNotificationState::Pending
        }
        PersonalDiscoveryDeliveryMode::QueueOnly => DiscoveryResultNotificationState::NotApplicable,
    }
}

/// Records the private one-shot Discovery-results-ready Event for a completed scheduled batch.
pub(crate) fn ensure_results_ready_event(
    store: &mut InMemoryStore,
    schedule_id: PersonalDiscoveryScheduleId,
    delivery_mode: PersonalDiscoveryDeliveryMode,
    batch: &DiscoveryResultBatch,
    now: DateTime<Utc>,
) -> DiscoveryResultsReadyEvent {
    if let Some(existing) = store.discovery_results_ready_events.get(&batch.id) {
        return existing.clone();
    }
    let event = DiscoveryResultsReadyEvent {
        id: Uuid::now_v7(),
        user_id: batch.user_id,
        tenant_id: batch.tenant_id,
        schedule_id,
        batch_id: batch.id,
        task_id: batch.task_id,
        delivery_mode,
        created_at: now,
        notification_attempted_at: None,
    };
    store
        .discovery_results_ready_events
        .insert(batch.id, event.clone());
    event
}
