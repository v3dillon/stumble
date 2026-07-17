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
    assert_eq!(first[0].pod_id, pod.id);
    assert_eq!(first[0].package_version, PackageVersion::new(1).unwrap());
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
                pod_id: task.pod_id,
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
                pod_id: task.pod_id,
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
