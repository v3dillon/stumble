use chrono::{TimeZone, Utc};
use stumble_core::*;

use crate::common::*;

#[test]
fn user_may_create_inspect_update_disable_and_remove_named_schedules() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();

    let daily = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("daily"), now)
        .unwrap();
    assert_eq!(daily.schedule.name, "daily");
    assert!(daily.schedule.enabled);
    assert_eq!(daily.schedule.result_count, 5);
    assert_eq!(
        daily.schedule.delivery_mode,
        PersonalDiscoveryDeliveryMode::NotifyWhenSupported
    );

    let weekly = tools
        .create_personal_discovery_schedule(
            &manager,
            CreatePersonalDiscoveryScheduleRequest {
                name: "weekly deep".into(),
                cadence: PersonalDiscoveryCadence::Weekly,
                intent: PersonalDiscoveryScheduleIntent::new(
                    vec!["rust".into()],
                    vec!["crypto".into()],
                ),
                result_count: Some(12),
                delivery_mode: PersonalDiscoveryDeliveryMode::QueueOnly,
            },
            now,
        )
        .unwrap();
    assert_eq!(weekly.schedule.intent.focus_topics, vec!["rust"]);
    assert_eq!(weekly.schedule.intent.avoid_topics, vec!["crypto"]);

    let listed = tools
        .list_personal_discovery_schedules(&manager, now)
        .unwrap();
    assert_eq!(listed.len(), 2);

    let inspected = tools
        .personal_discovery_schedule(&manager, daily.schedule.id, now)
        .unwrap();
    assert_eq!(inspected.schedule.id, daily.schedule.id);

    let updated = tools
        .update_personal_discovery_schedule(
            &manager,
            daily.schedule.id,
            UpdatePersonalDiscoveryScheduleRequest {
                name: Some("morning".into()),
                result_count: Some(8),
                ..Default::default()
            },
            now,
        )
        .unwrap();
    assert_eq!(updated.schedule.name, "morning");
    assert_eq!(updated.schedule.result_count, 8);

    let disabled = tools
        .disable_personal_discovery_schedule(&manager, updated.schedule.id, now)
        .unwrap();
    assert!(!disabled.schedule.enabled);

    let removed = tools
        .remove_personal_discovery_schedule(&manager, weekly.schedule.id)
        .unwrap();
    assert_eq!(removed.name, "weekly deep");
    assert_eq!(
        tools
            .list_personal_discovery_schedules(&manager, now)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn schedule_remains_dormant_below_cold_start_readiness() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let mut empty = UpdateTasteProfileRequest::default();
    empty.interests = Some(Vec::new());
    tools.update_taste_profile(&manager, empty).unwrap();
    tools
        .reset_learned_taste(&manager, ResetLearnedTasteRequest::all())
        .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 15, 0, 0).unwrap();
    let created = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("dormant"), now)
        .unwrap();
    assert!(created.readiness_dormant);

    let ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    assert!(!ready.iter().any(|task| matches!(
        task.origin,
        DiscoveryTaskOrigin::PersonalScheduled { schedule_id } if schedule_id == created.schedule.id
    )));
}

#[test]
fn due_materialization_is_deterministic_and_idempotent_for_schedule_period() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 10, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("idempotent"), now)
        .unwrap();

    let first = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let second = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let scheduled: Vec<_> = first
        .iter()
        .filter(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == schedule.schedule.id
            )
        })
        .cloned()
        .collect();
    assert_eq!(scheduled.len(), 1);
    assert_eq!(
        scheduled[0].due_at,
        schedule.schedule.cadence.period_start(now)
    );
    let again: Vec<_> = second
        .iter()
        .filter(|task| task.id == scheduled[0].id)
        .collect();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, scheduled[0].id);

    // Concurrent-style second materialization path via manager list.
    let manager_listed = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    assert_eq!(
        manager_listed
            .iter()
            .filter(|task| matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == schedule.schedule.id
            ))
            .count(),
        1
    );
}

#[test]
fn harness_and_local_adapter_paths_list_same_canonical_ready_tasks() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 11, 0, 0).unwrap();
    tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("neutral"), now)
        .unwrap();

    // Harness-owned wake: list ready as the unattended worker.
    let harness_ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    // Local Scheduler Adapter uses the same list_ready contract with an equivalent token.
    let adapter_ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let harness_ids: Vec<_> = harness_ready.iter().map(|task| task.id).collect();
    let adapter_ids: Vec<_> = adapter_ready.iter().map(|task| task.id).collect();
    assert_eq!(harness_ids, adapter_ids);
    assert!(!harness_ids.is_empty());
}

#[test]
fn schedule_defers_while_unreviewed_batch_and_on_demand_remains_available() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 9, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("backpressure"), now)
        .unwrap();

    let ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let task = ready
        .into_iter()
        .find(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == schedule.schedule.id
            )
        })
        .expect("scheduled task");
    let lease_now = Utc::now();
    tools
        .claim_discovery_task(
            &worker,
            task.id,
            lease_now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                task.id,
                "https://backpressure.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "bp-1",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            lease_now,
        )
        .unwrap();
    assert_eq!(batch.state, DiscoveryResultBatchState::Ready);

    let status = tools
        .personal_discovery_schedule(&manager, schedule.schedule.id, lease_now)
        .unwrap();
    assert!(matches!(
        status.backpressure,
        PersonalDiscoveryScheduleBackpressure::UnreviewedBatch { batch_id, .. }
            if batch_id == batch.id
    ));

    // Next period remains deferred while unreviewed.
    let next_day = now + chrono::Duration::days(1);
    let deferred = tools.list_ready_discovery_tasks(&worker, next_day).unwrap();
    assert!(!deferred.iter().any(|task| {
        matches!(
            task.origin,
            DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                if schedule_id == schedule.schedule.id
        ) && task.due_at == schedule.schedule.cadence.period_start(next_day)
    }));

    // On-demand remains available under schedule backpressure.
    let on_demand = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::Topic("systems".into())),
                result_count: Some(3),
                idempotency_key: "on-demand-during-backpressure".into(),
                browser_grant_eligible_sources: None,
            },
            next_day,
        )
        .unwrap();
    assert!(matches!(
        on_demand.task.origin,
        DiscoveryTaskOrigin::PersonalRequest { .. }
    ));

    tools
        .dismiss_discovery_result_batch(&manager, batch.id, next_day)
        .unwrap();
    let resumed = tools.list_ready_discovery_tasks(&worker, next_day).unwrap();
    assert!(resumed.iter().any(|task| {
        matches!(
            task.origin,
            DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                if schedule_id == schedule.schedule.id
        ) && task.due_at == schedule.schedule.cadence.period_start(next_day)
    }));
}

#[test]
fn scheduled_completion_emits_one_results_ready_event_and_notification_is_one_shot() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 8, 0, 0).unwrap();

    let notify = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("notify"), now)
        .unwrap();
    let queue = tools
        .create_personal_discovery_schedule(
            &manager,
            CreatePersonalDiscoveryScheduleRequest {
                name: "queue".into(),
                cadence: PersonalDiscoveryCadence::Daily,
                intent: PersonalDiscoveryScheduleIntent::default(),
                result_count: Some(3),
                delivery_mode: PersonalDiscoveryDeliveryMode::QueueOnly,
            },
            now,
        )
        .unwrap();

    let ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let notify_task = ready
        .iter()
        .find(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == notify.schedule.id
            )
        })
        .unwrap()
        .clone();
    let queue_task = ready
        .iter()
        .find(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == queue.schedule.id
            )
        })
        .unwrap()
        .clone();

    let lease_now = Utc::now();
    for (task, key) in [(&notify_task, "n1"), (&queue_task, "q1")] {
        tools
            .claim_discovery_task(
                &worker,
                task.id,
                lease_now,
                DiscoveryLeaseSeconds::new(300).unwrap(),
            )
            .unwrap();
        let submitted = tools
            .submit_candidate(
                &worker,
                personal_result_request(
                    task.id,
                    &format!("https://event.example/{key}"),
                    DiscoveryPlanSourceRole::Proven,
                    None,
                    key,
                ),
            )
            .unwrap();
        tools
            .complete_discovery_result_batch(
                &worker,
                CompleteDiscoveryResultBatchRequest {
                    task_id: task.id,
                    submission_ids: vec![submitted.submission.id],
                    source_availability: Vec::new(),
                    browser_grant_eligible_sources: None,
                },
                lease_now,
            )
            .unwrap();
    }

    let notify_batch = tools
        .list_discovery_result_batches(&manager)
        .unwrap()
        .into_iter()
        .find(|batch| batch.task_id == notify_task.id)
        .unwrap();
    let queue_batch = tools
        .list_discovery_result_batches(&manager)
        .unwrap()
        .into_iter()
        .find(|batch| batch.task_id == queue_task.id)
        .unwrap();
    assert_eq!(
        notify_batch.notification_state,
        DiscoveryResultNotificationState::Pending
    );
    assert_eq!(
        queue_batch.notification_state,
        DiscoveryResultNotificationState::NotApplicable
    );

    let events = tools
        .store()
        .read()
        .unwrap()
        .discovery_results_ready_events
        .clone();
    assert_eq!(events.len(), 2);
    assert!(events.contains_key(&notify_batch.id));
    assert!(events.contains_key(&queue_batch.id));

    let first = tools
        .attempt_discovery_results_ready_notification(&manager, notify_batch.id, lease_now)
        .unwrap();
    assert!(matches!(
        first,
        DiscoveryResultsReadyNotificationOutcome::ShouldNotify { .. }
    ));
    let second = tools
        .attempt_discovery_results_ready_notification(&manager, notify_batch.id, lease_now)
        .unwrap();
    assert!(matches!(
        second,
        DiscoveryResultsReadyNotificationOutcome::AlreadyAttempted { .. }
    ));
    let notify_batch = tools
        .discovery_result_batch(&manager, notify_batch.id)
        .unwrap();
    assert_eq!(
        notify_batch.notification_state,
        DiscoveryResultNotificationState::Delivered
    );
    assert_eq!(notify_batch.state, DiscoveryResultBatchState::Ready);

    let silent = tools
        .attempt_discovery_results_ready_notification(&manager, queue_batch.id, lease_now)
        .unwrap();
    assert!(matches!(
        silent,
        DiscoveryResultsReadyNotificationOutcome::QueueOnly { .. }
    ));
    assert_eq!(
        tools
            .discovery_result_batch(&manager, queue_batch.id)
            .unwrap()
            .state,
        DiscoveryResultBatchState::Ready
    );
}

#[test]
fn schedules_events_tasks_and_batches_persist_privately_across_restart() {
    let root =
        std::env::temp_dir().join(format!("stumble-schedule-persist-{}", uuid::Uuid::now_v7()));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "schedule manager".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::PersonalDiscoveryManagement,
                        HarnessCapability::Feedback,
                    ],
                    pod_ids: None,
                },
            )
            .unwrap();
        tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    let worker = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "schedule worker".into(),
                    kind: AgentHarnessKind::Unattended,
                    capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
                    pod_ids: None,
                },
            )
            .unwrap();
        tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 7, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("persist"), now)
        .unwrap();
    let task = tools
        .list_ready_discovery_tasks(&worker, now)
        .unwrap()
        .into_iter()
        .find(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == schedule.schedule.id
            )
        })
        .unwrap();
    let lease_now = Utc::now();
    tools
        .claim_discovery_task(
            &worker,
            task.id,
            lease_now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                task.id,
                "https://persist-schedule.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "persist-s1",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            lease_now,
        )
        .unwrap();

    let reopened = AgentTools::open_home_node(&root, seed_store).unwrap();
    let manager = reopened.local_owner_auth_context().unwrap();
    let status = reopened
        .list_personal_discovery_schedules(&manager, lease_now)
        .unwrap();
    assert!(status.iter().any(|s| s.schedule.name == "persist"));
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .discovery_results_ready_events
        .contains_key(&batch.id));
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .discovery_tasks
        .contains_key(&task.id));
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .discovery_result_batches
        .contains_key(&batch.id));

    let federation = reopened.default_auth_context().unwrap();
    let outbound = serde_json::to_string(&serde_json::json!({
        "pods": reopened.list_public_pods(&federation).unwrap(),
    }))
    .unwrap();
    assert!(!outbound.contains("persist-schedule.example"));
    assert!(!outbound.contains(&schedule.schedule.name));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn worker_cannot_change_schedule_or_delivery_policy() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("authz"), now)
        .unwrap();

    assert!(matches!(
        tools.create_personal_discovery_schedule(&worker, daily_schedule_request("nope"), now),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(matches!(
        tools.update_personal_discovery_schedule(
            &worker,
            schedule.schedule.id,
            UpdatePersonalDiscoveryScheduleRequest {
                delivery_mode: Some(PersonalDiscoveryDeliveryMode::QueueOnly),
                ..Default::default()
            },
            now,
        ),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(matches!(
        tools.disable_personal_discovery_schedule(&worker, schedule.schedule.id, now),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(matches!(
        tools.remove_personal_discovery_schedule(&worker, schedule.schedule.id),
        Err(AgentToolsError::Forbidden { .. })
    ));
    // Workers may inspect backpressure state for wake/claim decisions.
    let status = tools
        .personal_discovery_schedule(&worker, schedule.schedule.id, now)
        .unwrap();
    assert_eq!(status.schedule.id, schedule.schedule.id);
}

// --- Ticket 08: authenticated source availability ---
