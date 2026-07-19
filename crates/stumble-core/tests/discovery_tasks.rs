use chrono::{Duration, TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-discovery-tasks-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn package(cadence: &str) -> PodPackageContents {
    PodPackageContents {
        context_md: "# Systems\n\nReliable systems engineering.\n".into(),
        skill_md: "# Discovery\n\nPrefer primary engineering reports.\n".into(),
        sources_yaml: format!(
            "source_rules:\n  - inspect:\n      kind: publication\n      name: official engineering blogs\n    seek:\n      description: incident analyses\n    schedule:\n      cadence: {cadence}\n"
        ),
        filters_yaml: "blocked_topics: []\n".into(),
        examples_good_md: "# Good\n\n- Detailed incident report.\n".into(),
        examples_bad_md: "# Bad\n\n- Unsourced listicle.\n".into(),
    }
}

fn lease(seconds: u64) -> DiscoveryLeaseSeconds {
    DiscoveryLeaseSeconds::new(seconds).unwrap()
}

fn task_harness(tools: &AgentTools) -> (AuthContext, Pod) {
    let owner = tools.default_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "discovery worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![
                    HarnessCapability::PodCuration,
                    HarnessCapability::PackageManagement,
                    HarnessCapability::DiscoveryTasks,
                ],
                pod_ids: None,
            },
        )
        .unwrap();
    let harness = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    let created = tools
        .create_private_pod_with_package(
            &harness,
            CreatePrivatePodWithPackageRequest {
                name: "Systems".into(),
                slug: "systems".into(),
                description: "Systems work".into(),
                package: package("daily"),
            },
        )
        .unwrap();
    (harness, created.pod)
}

#[test]
fn due_source_rules_create_idempotent_versioned_tasks() {
    let tools = AgentTools::new(seed_store());
    let (harness, pod) = task_harness(&tools);
    let due_at = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();

    let first = tools
        .materialize_due_discovery_tasks(&harness, due_at)
        .unwrap();
    let second = tools
        .materialize_due_discovery_tasks(&harness, due_at)
        .unwrap();

    assert_eq!(first.len(), 1);
    assert!(second.is_empty());
    assert_eq!(
        first[0].target,
        DiscoveryTaskTarget::Pod {
            pod_id: pod.id,
            package_version: PackageVersion::new(1).unwrap(),
        }
    );
    assert!(matches!(
        first[0].origin,
        DiscoveryTaskOrigin::Scheduled {
            source_rule_index: 0
        }
    ));
    assert_eq!(first[0].due_at, due_at);
    assert_eq!(first[0].state, DiscoveryTaskState::Pending);
}

#[test]
fn personal_target_carries_a_plan_without_a_pod_contract() {
    let plan_id = DiscoveryPlanId::from(uuid::Uuid::now_v7());
    let target = DiscoveryTaskTarget::Personal {
        discovery_plan_id: plan_id,
    };

    assert_eq!(target.discovery_plan_id(), Some(plan_id));
    assert_eq!(target.pod(), None);
}

#[test]
fn typed_and_legacy_task_targets_must_agree_exactly() {
    let tools = AgentTools::new(seed_store());
    let (worker, _) = task_harness(&tools);
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
    let task = tools.materialize_due_discovery_tasks(&worker, now).unwrap()[0].clone();
    let mut contradictory = serde_json::to_value(&task).unwrap();
    contradictory["pod_id"] = serde_json::json!(uuid::Uuid::now_v7());
    assert!(serde_json::from_value::<DiscoveryTask>(contradictory).is_err());

    let mut partial = serde_json::to_value(&task).unwrap();
    partial.as_object_mut().unwrap().remove("package_version");
    assert!(serde_json::from_value::<DiscoveryTask>(partial).is_err());

    let personal = DiscoveryTaskTarget::Personal {
        discovery_plan_id: uuid::Uuid::now_v7().into(),
    };
    let mut personal_with_pod_aliases = serde_json::to_value(&task).unwrap();
    personal_with_pod_aliases["target"] = serde_json::to_value(personal).unwrap();
    assert!(serde_json::from_value::<DiscoveryTask>(personal_with_pod_aliases).is_err());
}

#[test]
fn pod_discovery_workers_cannot_list_future_personal_tasks() {
    let tools = AgentTools::new(seed_store());
    let (unscoped_worker, pod) = task_harness(&tools);
    let owner = tools.default_auth_context().unwrap();
    let scoped_worker = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "Pod-scoped worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::DiscoveryTasks],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let scoped_worker = tools
        .authenticate_token(scoped_worker.token.expose())
        .unwrap()
        .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
    let personal_task = DiscoveryTask {
        id: uuid::Uuid::now_v7().into(),
        target: DiscoveryTaskTarget::Personal {
            discovery_plan_id: uuid::Uuid::now_v7().into(),
        },
        origin: DiscoveryTaskOrigin::Immediate {
            instructions: "future private plan".into(),
            idempotency_key: "future-personal".into(),
            requested_by: unscoped_worker.harness_id.unwrap(),
        },
        due_at: now,
        state: DiscoveryTaskState::Pending,
        attempts: Vec::new(),
        created_at: now,
    };
    tools
        .store()
        .write()
        .unwrap()
        .discovery_tasks
        .insert(personal_task.id, personal_task.clone());

    for worker in [&unscoped_worker, &scoped_worker] {
        let listed = tools.list_discovery_tasks(worker, now).unwrap();
        assert!(!listed.iter().any(|task| task.id == personal_task.id));
        let ready = tools.list_ready_discovery_tasks(worker, now).unwrap();
        assert!(!ready.iter().any(|task| task.id == personal_task.id));
    }
}

#[test]
fn leases_are_exclusive_renewable_and_expire_safely() {
    let tools = AgentTools::new(seed_store());
    let (worker, _) = task_harness(&tools);
    let owner = tools.default_auth_context().unwrap();
    let competitor = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "competitor".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::DiscoveryTasks],
                pod_ids: None,
            },
        )
        .unwrap();
    let competitor = tools
        .authenticate_token(competitor.token.expose())
        .unwrap()
        .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
    let task = tools.materialize_due_discovery_tasks(&worker, now).unwrap()[0].clone();

    assert!(DiscoveryLeaseSeconds::new(u64::from(DiscoveryLeaseSeconds::MAX) + 1).is_err());

    let claimed = tools
        .claim_discovery_task(&worker, task.id, now, lease(300))
        .unwrap();
    assert!(matches!(claimed.state, DiscoveryTaskState::Leased(_)));
    assert!(matches!(
        tools.claim_discovery_task(&competitor, task.id, now, lease(300)),
        Err(AgentToolsError::TaskLeaseConflict)
    ));
    let renewed = tools
        .renew_discovery_task_lease(&worker, task.id, now + Duration::minutes(2), lease(600))
        .unwrap();
    assert_eq!(
        match renewed.state {
            DiscoveryTaskState::Leased(lease) => lease.expires_at,
            _ => panic!("expected lease"),
        },
        now + Duration::minutes(12)
    );
    assert!(matches!(
        tools
            .renew_discovery_task_lease(&worker, task.id, now + Duration::minutes(13), lease(600),),
        Err(AgentToolsError::TaskLeaseRequired)
    ));
    let reclaimed = tools
        .claim_discovery_task(
            &competitor,
            task.id,
            now + Duration::minutes(13),
            lease(300),
        )
        .unwrap();
    assert_eq!(
        match reclaimed.state {
            DiscoveryTaskState::Leased(ref lease) => lease.harness_id,
            _ => panic!("expected lease"),
        },
        competitor.harness_id.unwrap()
    );
    assert!(matches!(
        reclaimed.attempts[0].outcome,
        DiscoveryTaskAttemptOutcome::LeaseExpired
    ));
}

#[test]
fn failure_retry_and_completion_history_are_inspectable() {
    let tools = AgentTools::new(seed_store());
    let (worker, _) = task_harness(&tools);
    let start = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
    let task = tools
        .materialize_due_discovery_tasks(&worker, start)
        .unwrap()[0]
        .clone();

    for retry in 0..3 {
        let now = start + Duration::minutes(i64::from(retry));
        tools
            .claim_discovery_task(&worker, task.id, now, lease(300))
            .unwrap();
        tools
            .fail_discovery_task(&worker, task.id, now, format!("failure {retry}"))
            .unwrap();
    }
    let terminal = tools
        .discovery_task_status(&worker, task.id, start)
        .unwrap();
    assert_eq!(terminal.state, DiscoveryTaskState::TerminalFailure);
    assert_eq!(terminal.attempts.len(), 3);
    assert!(matches!(
        tools.claim_discovery_task(&worker, task.id, start, lease(300)),
        Err(AgentToolsError::TaskTerminal)
    ));

    let immediate = tools
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: task.target.pod().unwrap().0,
                instructions: "find a fresh incident analysis".into(),
                idempotency_key: "conversation-1".into(),
            },
            start + Duration::hours(1),
        )
        .unwrap();
    let duplicate = tools
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: task.target.pod().unwrap().0,
                instructions: "find a fresh incident analysis".into(),
                idempotency_key: "conversation-1".into(),
            },
            start + Duration::hours(1),
        )
        .unwrap();
    assert_eq!(duplicate.id, immediate.id);
    assert!(matches!(
        immediate.origin,
        DiscoveryTaskOrigin::Immediate { ref instructions, .. }
            if instructions == "find a fresh incident analysis"
    ));
    tools
        .claim_discovery_task(
            &worker,
            immediate.id,
            start + Duration::hours(1),
            lease(300),
        )
        .unwrap();
    let completed = tools
        .complete_discovery_task(&worker, immediate.id, start + Duration::hours(1))
        .unwrap();
    assert_eq!(completed.state, DiscoveryTaskState::Completed);
    assert!(matches!(
        completed.attempts[0].outcome,
        DiscoveryTaskAttemptOutcome::Completed
    ));
}

#[test]
fn repeated_abandoned_leases_are_recorded_and_become_terminal() {
    let tools = AgentTools::new(seed_store());
    let (worker, _) = task_harness(&tools);
    let start = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
    let task = tools
        .materialize_due_discovery_tasks(&worker, start)
        .unwrap()[0]
        .clone();

    for attempt in 0..3 {
        let now = start + Duration::minutes(i64::from(attempt) * 2);
        tools
            .claim_discovery_task(&worker, task.id, now, lease(60))
            .unwrap();
    }
    let terminal = tools
        .discovery_task_status(&worker, task.id, start + Duration::minutes(5))
        .unwrap();
    assert_eq!(terminal.state, DiscoveryTaskState::TerminalFailure);
    assert_eq!(terminal.attempts.len(), 3);
    assert!(terminal
        .attempts
        .iter()
        .all(|attempt| matches!(attempt.outcome, DiscoveryTaskAttemptOutcome::LeaseExpired)));
}

#[test]
fn tasks_and_retry_history_survive_home_node_restart() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (worker, _) = task_harness(&tools);
    let at = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
    let task = tools.materialize_due_discovery_tasks(&worker, at).unwrap()[0].clone();
    tools
        .claim_discovery_task(&worker, task.id, at, lease(300))
        .unwrap();
    tools
        .fail_discovery_task(&worker, task.id, at, "temporary outage".into())
        .unwrap();
    drop(tools);

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let restored = reopened
        .discovery_task_status(&worker, task.id, at)
        .unwrap();
    assert_eq!(restored.state, DiscoveryTaskState::Pending);
    assert_eq!(restored.attempts.len(), 1);
    assert!(matches!(
        restored.attempts[0].outcome,
        DiscoveryTaskAttemptOutcome::Failed { ref reason } if reason == "temporary outage"
    ));
}

#[test]
fn initialized_sqlite_migrates_pre_feature_tasks_before_lifecycle_mutation() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (worker, _) = task_harness(&tools);
    let at = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
    let scheduled = tools.materialize_due_discovery_tasks(&worker, at).unwrap()[0].clone();
    let immediate = tools
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: scheduled.target.pod().unwrap().0,
                instructions: "find a migration report".into(),
                idempotency_key: "sqlite-legacy-task".into(),
            },
            at,
        )
        .unwrap();
    drop(tools);

    let database_path = data_dir.0.join("stumble.sqlite3");
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    let legacy_rows = {
        let mut statement = connection
            .prepare(
                "SELECT record_key, value_json FROM stumble_store_records \
                 WHERE collection = 'discovery_tasks'",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    for (record_key, value_json) in legacy_rows {
        let mut task: serde_json::Value = serde_json::from_str(&value_json).unwrap();
        task.as_object_mut().unwrap().remove("target").unwrap();
        connection
            .execute(
                "UPDATE stumble_store_records SET value_json = ?1 \
                 WHERE collection = 'discovery_tasks' AND record_key = ?2",
                rusqlite::params![serde_json::to_string(&task).unwrap(), record_key],
            )
            .unwrap();
    }
    drop(connection);

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    reopened
        .claim_discovery_task(&worker, scheduled.id, at, lease(300))
        .unwrap();
    reopened
        .renew_discovery_task_lease(&worker, scheduled.id, at, lease(600))
        .unwrap();
    reopened
        .fail_discovery_task(&worker, scheduled.id, at, "retry after migration".into())
        .unwrap();
    reopened
        .claim_discovery_task(&worker, immediate.id, at, lease(300))
        .unwrap();
    reopened
        .complete_discovery_task(&worker, immediate.id, at)
        .unwrap();
    drop(reopened);

    let restarted = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    assert_eq!(
        restarted
            .discovery_task_status(&worker, scheduled.id, at)
            .unwrap()
            .attempts
            .len(),
        1
    );
    assert_eq!(
        restarted
            .discovery_task_status(&worker, immediate.id, at)
            .unwrap()
            .state,
        DiscoveryTaskState::Completed
    );
}

#[test]
fn pre_feature_tasks_migrate_identity_provenance_history_and_idempotency() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::new(seed_store());
    let (worker, _) = task_harness(&tools);
    let at = Utc.with_ymd_and_hms(2026, 7, 17, 0, 0, 0).unwrap();
    let scheduled = tools.materialize_due_discovery_tasks(&worker, at).unwrap()[0].clone();
    tools
        .claim_discovery_task(&worker, scheduled.id, at, lease(300))
        .unwrap();
    tools
        .fail_discovery_task(&worker, scheduled.id, at, "temporary outage".into())
        .unwrap();
    let immediate = tools
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: scheduled.target.pod().unwrap().0,
                instructions: "find a durable incident analysis".into(),
                idempotency_key: "legacy-conversation".into(),
            },
            at + Duration::hours(1),
        )
        .unwrap();
    let snapshot_path = data_dir.0.join("store.json");
    save_store_snapshot(&tools.store().read().unwrap(), &snapshot_path).unwrap();
    drop(tools);

    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&snapshot_path).unwrap()).unwrap();
    for task in snapshot["discovery_tasks"].as_array_mut().unwrap() {
        let target = task.as_object_mut().unwrap().remove("target").unwrap();
        let target = target.as_object().unwrap();
        assert_eq!(target["kind"], "pod");
        task["pod_id"] = target["pod_id"].clone();
        task["package_version"] = target["package_version"].clone();
    }
    std::fs::write(
        &snapshot_path,
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let restored = reopened
        .discovery_task_status(&worker, scheduled.id, at)
        .unwrap();
    assert_eq!(restored.id, scheduled.id);
    assert_eq!(restored.target, scheduled.target);
    assert_eq!(restored.origin, scheduled.origin);
    assert_eq!(restored.state, DiscoveryTaskState::Pending);
    assert_eq!(restored.attempts.len(), 1);
    assert!(matches!(
        restored.attempts[0].outcome,
        DiscoveryTaskAttemptOutcome::Failed { ref reason } if reason == "temporary outage"
    ));
    let retried = reopened
        .create_immediate_discovery_task(
            &worker,
            CreateImmediateDiscoveryTaskRequest {
                pod_id: scheduled.target.pod().unwrap().0,
                instructions: "find a durable incident analysis".into(),
                idempotency_key: "legacy-conversation".into(),
            },
            at + Duration::hours(1),
        )
        .unwrap();
    assert_eq!(retried.id, immediate.id);
}
