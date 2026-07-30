use chrono::{TimeZone, Utc};
use stumble_core::*;

use crate::common::*;

#[test]
fn worker_reports_source_availability_without_credentials_and_rejects_auth_material() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "avail-report");
    let now = Utc::now();

    let reported = tools
        .report_discovery_source_availability(
            &worker,
            ReportDiscoverySourceAvailabilityRequest {
                task_id: created.task.id,
                reports: vec![
                    ReportedSourceAvailability {
                        source: "open.example".into(),
                        state: SourceAvailabilityState::Available,
                        reason: "public feed reachable".into(),
                    },
                    ReportedSourceAvailability {
                        source: "auth.example".into(),
                        state: SourceAvailabilityState::SessionExpired,
                        reason: "session expired".into(),
                    },
                ],
                browser_grant_eligible_sources: Some(vec![
                    "open.example".into(),
                    "auth.example".into(),
                ]),
            },
            now,
        )
        .unwrap();
    assert_eq!(reported.availability.reports.len(), 2);
    assert!(reported
        .availability
        .reports
        .iter()
        .any(|r| r.source == "auth.example" && r.authentication_required()));
    // Snapshot contains facts only — no credential fields exist on the contract.
    let serialized = serde_json::to_string(&reported.availability).unwrap();
    for forbidden in [
        "password",
        "cookie",
        "token",
        "authorization",
        "raw_browser",
        "cdp_session",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "availability snapshot leaked {forbidden}"
        );
    }

    let denied = tools.report_discovery_source_availability(
        &worker,
        ReportDiscoverySourceAvailabilityRequest {
            task_id: created.task.id,
            reports: vec![ReportedSourceAvailability {
                source: "auth.example".into(),
                state: SourceAvailabilityState::AuthenticationRequired,
                reason: "cookie: session=secret".into(),
            }],
            browser_grant_eligible_sources: None,
        },
        now,
    );
    assert!(matches!(
        denied,
        Err(AgentToolsError::Store(StoreError::Validation(message)))
            if message.contains("authentication material")
    ));

    // Deny unknown credential-bearing fields at the wire boundary.
    let smuggle: Result<ReportDiscoverySourceAvailabilityRequest, _> =
        serde_json::from_value(serde_json::json!({
            "task_id": created.task.id,
            "reports": [{
                "source": "x.com",
                "state": "authentication_required",
                "reason": "login",
                "password": "hunter2",
                "cookies": ["a=b"]
            }]
        }));
    assert!(smuggle.is_err());
}

#[test]
fn browser_grant_eligibility_restricts_planning_and_execution_not_broadened_by_taste_or_leads() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let now = Utc::now();
    // Temporary similar-to intent would otherwise inject taste-only.example as proven.
    // Browser Grant eligibility must restrict it; Taste Profile / leads cannot broaden.
    let created = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::SimilarToUrl(
                    "https://taste-only.example/article".into(),
                )),
                result_count: Some(6),
                idempotency_key: "grant-restrict".into(),
                browser_grant_eligible_sources: Some(vec!["open.example".into()]),
            },
            now,
        )
        .unwrap();
    let selected: Vec<_> = created
        .plan
        .source_neighborhoods
        .iter()
        .map(|n| match &n.signal {
            SourceAffinitySignal::Source(value)
            | SourceAffinitySignal::Publisher(value)
            | SourceAffinitySignal::AuthorOrAccount(value)
            | SourceAffinitySignal::Community(value)
            | SourceAffinitySignal::ReferrerContext(value) => value.clone(),
            _ => String::new(),
        })
        .collect();
    assert!(
        !selected.iter().any(|s| s == "taste-only.example"),
        "grant must exclude taste-only source, got {selected:?}"
    );
    assert!(
        selected.iter().all(|s| s == "open.example"),
        "only grant-eligible sources may remain, got {selected:?}"
    );

    tools
        .claim_discovery_task(
            &worker,
            created.task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    // Execution: worker cannot mark a non-eligible source Available even if a lead suggested it.
    let reported = tools
        .report_discovery_source_availability(
            &worker,
            ReportDiscoverySourceAvailabilityRequest {
                task_id: created.task.id,
                reports: vec![ReportedSourceAvailability {
                    source: "taste-only.example".into(),
                    state: SourceAvailabilityState::Available,
                    reason: "lead said so".into(),
                }],
                browser_grant_eligible_sources: Some(vec!["open.example".into()]),
            },
            now,
        )
        .unwrap();
    assert!(reported.availability.reports.iter().any(|r| {
        r.source == "taste-only.example"
            && r.state == SourceAvailabilityState::BrowserGrantIneligible
    }));
}

#[test]
fn on_demand_requests_auth_assistance_while_continuing_accessible_sources() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "on-demand-auth");
    let now = Utc::now();

    let reported = tools
        .report_discovery_source_availability(
            &worker,
            ReportDiscoverySourceAvailabilityRequest {
                task_id: created.task.id,
                reports: vec![
                    ReportedSourceAvailability {
                        source: "open.example".into(),
                        state: SourceAvailabilityState::Available,
                        reason: String::new(),
                    },
                    ReportedSourceAvailability {
                        source: "private.example".into(),
                        state: SourceAvailabilityState::AuthenticationRequired,
                        reason: "login required".into(),
                    },
                ],
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    assert!(reported.authentication_notices.iter().any(|outcome| {
        matches!(
            outcome,
            AuthenticationNeededNoticeOutcome::ShouldNotify { notice }
                if notice.source == "private.example" && notice.delivery_pending
        )
    }));
    assert_eq!(
        tools
            .list_authentication_needed_notices(&manager)
            .unwrap()
            .len(),
        1
    );

    // Accessible planned work continues and completes.
    let open = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://open.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "open-1",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![open.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    assert_eq!(batch.items.len(), 1);
    assert!(batch.items[0].canonical_url.contains("open.example"));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::AuthenticationAssistanceRequested { source, .. }
            if source == "private.example"
    )));
}

#[test]
fn scheduled_run_skips_authenticated_sources_reallocates_and_never_waits() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 11, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("auth-skip"), now)
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

    let reported = tools
        .report_discovery_source_availability(
            &worker,
            ReportDiscoverySourceAvailabilityRequest {
                task_id: task.id,
                reports: vec![
                    ReportedSourceAvailability {
                        source: "auth.example".into(),
                        state: SourceAvailabilityState::AuthenticationRequired,
                        reason: "session missing".into(),
                    },
                    ReportedSourceAvailability {
                        source: "open.example".into(),
                        state: SourceAvailabilityState::Available,
                        reason: String::new(),
                    },
                ],
                browser_grant_eligible_sources: None,
            },
            lease_now,
        )
        .unwrap();
    assert!(reported.authentication_notices.iter().all(|outcome| {
        !matches!(
            outcome,
            AuthenticationNeededNoticeOutcome::ShouldNotify { .. }
        )
    }));
    assert!(reported.authentication_notices.iter().any(|outcome| {
        matches!(
            outcome,
            AuthenticationNeededNoticeOutcome::ScheduledSkip { source }
                if source == "auth.example"
        )
    }));
    assert!(tools
        .list_authentication_needed_notices(&manager)
        .unwrap()
        .is_empty());

    // Partial batch from accessible sources; reallocation within policy.
    let mut submission_ids = Vec::new();
    for (i, url) in [
        "https://open.example/1",
        "https://open.example/2",
        "https://adjacent.example/1",
    ]
    .into_iter()
    .enumerate()
    {
        let role = if i < 2 {
            DiscoveryPlanSourceRole::Proven
        } else {
            DiscoveryPlanSourceRole::Adjacent
        };
        let submitted = tools
            .submit_candidate(
                &worker,
                personal_result_request(task.id, url, role, None, &format!("sched-{i}")),
            )
            .unwrap();
        submission_ids.push(submitted.submission.id);
    }
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: task.id,
                submission_ids,
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            lease_now,
        )
        .unwrap();
    assert_eq!(batch.items.len(), 3);
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::AuthenticationSkippedScheduled { source, .. }
            if source == "auth.example"
    )));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::Underfilled { .. }
            | DiscoveryResultAvailabilityReason::InsufficientProven { .. }
            | DiscoveryResultAvailabilityReason::Reallocated { .. }
    )));
    // Task completed without waiting for authentication.
    let status = tools
        .discovery_task_status(&worker, task.id, lease_now)
        .unwrap();
    assert_eq!(status.state, DiscoveryTaskState::Completed);
}

#[test]
fn authentication_needed_notice_is_one_shot_until_availability_changes() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let first = claim_personal_run(&tools, &manager, &worker, Some(4), "notice-1");
    let now = Utc::now();
    let report = ReportDiscoverySourceAvailabilityRequest {
        task_id: first.task.id,
        reports: vec![ReportedSourceAvailability {
            source: "x.example".into(),
            state: SourceAvailabilityState::SessionExpired,
            reason: "expired".into(),
        }],
        browser_grant_eligible_sources: None,
    };
    let first_outcome = tools
        .report_discovery_source_availability(&worker, report.clone(), now)
        .unwrap();
    assert!(matches!(
        &first_outcome.authentication_notices[0],
        AuthenticationNeededNoticeOutcome::ShouldNotify { .. }
    ));

    // Same unavailable state on a later on-demand run is suppressed.
    let second = {
        let created = tools
            .request_personal_discovery(
                &manager,
                RequestPersonalDiscovery {
                    intent: Some(PersonalDiscoveryIntent::Topic("systems".into())),
                    result_count: Some(4),
                    idempotency_key: "notice-2".into(),
                    browser_grant_eligible_sources: None,
                },
                now,
            )
            .unwrap();
        tools
            .claim_discovery_task(
                &worker,
                created.task.id,
                now,
                DiscoveryLeaseSeconds::new(300).unwrap(),
            )
            .unwrap();
        created
    };
    let suppressed = tools
        .report_discovery_source_availability(
            &worker,
            ReportDiscoverySourceAvailabilityRequest {
                task_id: second.task.id,
                reports: vec![ReportedSourceAvailability {
                    source: "x.example".into(),
                    state: SourceAvailabilityState::SessionExpired,
                    reason: "expired".into(),
                }],
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    assert!(matches!(
        &suppressed.authentication_notices[0],
        AuthenticationNeededNoticeOutcome::Suppressed { .. }
    ));
    assert_eq!(
        tools
            .list_authentication_needed_notices(&manager)
            .unwrap()
            .len(),
        1
    );

    // Restored session clears suppression; later expiry is eligible again.
    tools
        .report_discovery_source_availability(
            &worker,
            ReportDiscoverySourceAvailabilityRequest {
                task_id: second.task.id,
                reports: vec![ReportedSourceAvailability {
                    source: "x.example".into(),
                    state: SourceAvailabilityState::Available,
                    reason: "session restored".into(),
                }],
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    assert!(tools
        .list_authentication_needed_notices(&manager)
        .unwrap()
        .is_empty());

    let third = {
        let created = tools
            .request_personal_discovery(
                &manager,
                RequestPersonalDiscovery {
                    intent: Some(PersonalDiscoveryIntent::Topic("systems".into())),
                    result_count: Some(4),
                    idempotency_key: "notice-3".into(),
                    browser_grant_eligible_sources: None,
                },
                now,
            )
            .unwrap();
        tools
            .claim_discovery_task(
                &worker,
                created.task.id,
                now,
                DiscoveryLeaseSeconds::new(300).unwrap(),
            )
            .unwrap();
        created
    };
    let again = tools
        .report_discovery_source_availability(
            &worker,
            ReportDiscoverySourceAvailabilityRequest {
                task_id: third.task.id,
                reports: vec![ReportedSourceAvailability {
                    source: "x.example".into(),
                    state: SourceAvailabilityState::SessionExpired,
                    reason: "expired again".into(),
                }],
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    assert!(matches!(
        &again.authentication_notices[0],
        AuthenticationNeededNoticeOutcome::ShouldNotify { .. }
    ));
}

#[test]
fn unavailable_source_cannot_discard_valid_results_from_other_sources() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(6), "partial-batch");
    let now = Utc::now();

    let a = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://a.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "a1",
            ),
        )
        .unwrap();
    let b = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://b.example/1",
                DiscoveryPlanSourceRole::Adjacent,
                None,
                "b1",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![a.submission.id, b.submission.id],
                source_availability: vec![
                    ReportedSourceAvailability {
                        source: "down.example".into(),
                        state: SourceAvailabilityState::Inaccessible,
                        reason: "timeout".into(),
                    },
                    ReportedSourceAvailability {
                        source: "auth.example".into(),
                        state: SourceAvailabilityState::AuthenticationRequired,
                        reason: "login".into(),
                    },
                ],
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    assert_eq!(batch.items.len(), 2);
    assert!(batch
        .items
        .iter()
        .any(|item| item.canonical_url.contains("a.example")));
    assert!(batch
        .items
        .iter()
        .any(|item| item.canonical_url.contains("b.example")));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::SourceUnavailable { source, .. }
            if source == "down.example"
    )));
}

#[test]
fn source_availability_is_retry_safe_lease_scoped_persisted_and_private() {
    let root = std::env::temp_dir().join(format!("pd-avail-{}", uuid::Uuid::now_v7()));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "avail manager".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![HarnessCapability::PersonalDiscoveryManagement],
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
                    label: "avail worker".into(),
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
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "avail-persist");
    let now = Utc::now();
    let request = ReportDiscoverySourceAvailabilityRequest {
        task_id: created.task.id,
        reports: vec![ReportedSourceAvailability {
            source: "private-auth.example".into(),
            state: SourceAvailabilityState::AuthenticationRequired,
            reason: "needs login".into(),
        }],
        browser_grant_eligible_sources: Some(vec!["private-auth.example".into()]),
    };
    let first = tools
        .report_discovery_source_availability(&worker, request.clone(), now)
        .unwrap();
    let retry = tools
        .report_discovery_source_availability(&worker, request, now)
        .unwrap();
    assert_eq!(
        first.availability.reports.len(),
        retry.availability.reports.len()
    );
    // Same unavailable fingerprint remains one-shot (suppressed on retry).
    assert!(matches!(
        &retry.authentication_notices[0],
        AuthenticationNeededNoticeOutcome::Suppressed { .. }
    ));

    // Other worker without lease cannot report.
    let other = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "other worker".into(),
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
    assert!(matches!(
        tools.report_discovery_source_availability(
            &other,
            ReportDiscoverySourceAvailabilityRequest {
                task_id: created.task.id,
                reports: vec![ReportedSourceAvailability {
                    source: "private-auth.example".into(),
                    state: SourceAvailabilityState::Available,
                    reason: String::new(),
                }],
                browser_grant_eligible_sources: None,
            },
            now,
        ),
        Err(AgentToolsError::TaskLeaseRequired | AgentToolsError::Forbidden { .. })
    ));

    let inspected = tools
        .discovery_task_source_availability(&manager, created.task.id)
        .unwrap();
    assert_eq!(inspected.task_id, created.task.id);
    drop(tools);

    let reopened = AgentTools::open_initialized_home_node(&root).unwrap();
    let owner = reopened.local_owner_auth_context().unwrap();
    let manager = {
        let issued = reopened
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "avail manager reopen".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![HarnessCapability::PersonalDiscoveryManagement],
                    pod_ids: None,
                },
            )
            .unwrap();
        reopened
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    let persisted = reopened
        .discovery_task_source_availability(&manager, created.task.id)
        .unwrap();
    assert!(persisted
        .reports
        .iter()
        .any(|r| r.source == "private-auth.example"));
    assert_eq!(
        reopened
            .list_authentication_needed_notices(&manager)
            .unwrap()
            .len(),
        1
    );
    let federation = reopened.default_auth_context().unwrap();
    let outbound = serde_json::to_string(&serde_json::json!({
        "pods": reopened.list_public_pods(&federation).unwrap(),
        "node": reopened.node_info(&federation).unwrap(),
    }))
    .unwrap();
    assert!(!outbound.contains("private-auth.example"));
    assert!(!outbound.contains("authentication_required"));
    let _ = std::fs::remove_dir_all(root);
}
