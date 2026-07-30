use chrono::Utc;
use stumble_core::*;

use crate::common::*;

#[test]
fn only_lease_holder_may_submit_personal_results_or_complete_batch() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let other = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "lease-only");

    let request = personal_result_request(
        created.task.id,
        "https://a.example/1",
        DiscoveryPlanSourceRole::Proven,
        None,
        "r1",
    );
    assert!(matches!(
        tools.submit_candidate(&other, request.clone()),
        Err(AgentToolsError::CandidateTaskLeaseRequired)
    ));
    let submitted = tools.submit_candidate(&worker, request).unwrap();
    assert_eq!(
        submitted.submission.target.acquisition_origin(),
        CandidateAcquisitionOrigin::AgentDiscovery
    );

    let complete = CompleteDiscoveryResultBatchRequest {
        task_id: created.task.id,
        submission_ids: vec![submitted.submission.id],
        source_availability: Vec::new(),
        browser_grant_eligible_sources: None,
    };
    assert!(matches!(
        tools.complete_discovery_result_batch(&other, complete.clone(), Utc::now()),
        Err(AgentToolsError::TaskLeaseRequired | AgentToolsError::Forbidden { .. })
    ));
    let batch = tools
        .complete_discovery_result_batch(&worker, complete, Utc::now())
        .unwrap();
    assert_eq!(batch.task_id, created.task.id);
    assert_eq!(batch.plan_id, created.plan.id);
    assert_eq!(batch.state, DiscoveryResultBatchState::Ready);
}

#[test]
fn personal_discovery_tasks_cannot_complete_without_a_result_batch() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "no-bare-complete");
    let now = Utc::now();

    let err = tools
        .complete_discovery_task(&worker, created.task.id, now)
        .expect_err("bare complete must not finish Personal Discovery");
    assert!(
        matches!(err, AgentToolsError::Store(StoreError::Validation(ref message)) if message.contains("complete_discovery_result_batch")),
        "unexpected error: {err:?}"
    );
    let task = tools
        .list_discovery_tasks(&worker, now)
        .unwrap()
        .into_iter()
        .find(|task| task.id == created.task.id)
        .expect("task still listed");
    assert!(
        matches!(task.state, DiscoveryTaskState::Leased(_)),
        "task must remain leased after rejected bare complete"
    );

    // Failures remain available so the worker can release the lease without a batch.
    let failed = tools
        .fail_discovery_task(
            &worker,
            created.task.id,
            now,
            "source neighborhoods unavailable".into(),
        )
        .unwrap();
    assert!(matches!(
        failed.state,
        DiscoveryTaskState::Pending | DiscoveryTaskState::TerminalFailure
    ));
}

#[test]
fn personal_results_retain_provenance_and_never_create_interest_seeds() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "provenance");

    let mut request = personal_result_request(
        created.task.id,
        "https://blog.example/post?utm=1",
        DiscoveryPlanSourceRole::Proven,
        Some("Ada"),
        "prov-1",
    );
    if let CandidateSubmissionRequestTarget::PersonalDiscovery { source_facts, .. } =
        &mut request.target
    {
        *source_facts =
            CandidateInterestSeedMetadata::new(Some("Example Press".into()), Some("rust".into()));
    }
    request.evidence.media_references = vec![MediaReference::new(
        MediaReferenceType::Image,
        "https://cdn.example.com/post.png",
    )
    .unwrap()];
    let submitted = tools.submit_candidate(&worker, request).unwrap();
    assert!(
        submitted
            .candidate
            .canonical_url
            .starts_with("https://blog.example/post"),
        "canonical identity retained: {}",
        submitted.candidate.canonical_url
    );
    assert_eq!(
        submitted.candidate.source_url,
        submitted.candidate.canonical_url
    );
    assert_eq!(
        submitted.submission.evidence.provenance.discovery_method,
        "browser_search"
    );
    assert_eq!(
        submitted
            .submission
            .evidence
            .provenance
            .referrer_url
            .as_deref(),
        Some("https://news.example/list")
    );
    match &submitted.submission.target {
        CandidateSubmissionTarget::PersonalDiscovery {
            task_id,
            discovery_plan_id,
            allocation_role,
            source_facts,
            ..
        } => {
            assert_eq!(*task_id, created.task.id);
            assert_eq!(*discovery_plan_id, created.plan.id);
            assert_eq!(*allocation_role, DiscoveryPlanSourceRole::Proven);
            assert_eq!(source_facts.publisher.as_deref(), Some("Example Press"));
            assert_eq!(source_facts.community.as_deref(), Some("rust"));
        }
        other => panic!("expected personal target, got {other:?}"),
    }
    assert!(!submitted.submission.target.learning_enabled());
    assert_eq!(
        tools
            .taste_profile(&manager)
            .unwrap()
            .interest_seed_evidence
            .active_seed_count,
        0
    );

    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(batch.items[0].candidate_id, submitted.candidate.id);
    assert_eq!(batch.items[0].submission_id, submitted.submission.id);
    assert_eq!(
        batch.items[0].canonical_url,
        submitted.candidate.canonical_url
    );
}

#[test]
fn batch_completion_enforces_size_allocation_caps_blocks_dedup_and_recent_suppression() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let mut taste = UpdateTasteProfileRequest::default();
    taste.blocked_sources = Some(vec!["blocked.example".into()]);
    tools.update_taste_profile(&manager, taste).unwrap();
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(6), "caps");

    let mut submission_ids = Vec::new();
    // 4 proven + 2 adjacent requested for size 6 (70/30 => 5/1 actually for 6? 6*7/10=4.2 -> (42+9)/10=5 proven, 1 adjacent)
    assert_eq!(created.plan.allocation.proven, 5);
    assert_eq!(created.plan.allocation.adjacent, 1);

    let specs = [
        (
            "https://d1.example/1",
            DiscoveryPlanSourceRole::Proven,
            Some("A1"),
            "s1",
        ),
        (
            "https://d1.example/2",
            DiscoveryPlanSourceRole::Proven,
            Some("A1"),
            "s2",
        ),
        (
            "https://d1.example/3",
            DiscoveryPlanSourceRole::Proven,
            Some("A2"),
            "s3",
        ),
        (
            "https://d1.example/4",
            DiscoveryPlanSourceRole::Proven,
            Some("A3"),
            "s4",
        ), // domain cap (>3)
        (
            "https://d2.example/1",
            DiscoveryPlanSourceRole::Proven,
            Some("A1"),
            "s5",
        ), // author cap (>2)
        (
            "https://blocked.example/x",
            DiscoveryPlanSourceRole::Proven,
            None,
            "s6",
        ),
        (
            "https://d3.example/1",
            DiscoveryPlanSourceRole::Proven,
            Some("A4"),
            "s7",
        ),
        (
            "https://d3.example/1#dup",
            DiscoveryPlanSourceRole::Adjacent,
            Some("A5"),
            "s8",
        ), // canonical dup
        (
            "https://d4.example/adj",
            DiscoveryPlanSourceRole::Adjacent,
            Some("A6"),
            "s9",
        ),
    ];
    for (url, role, author, key) in specs {
        let submitted = tools
            .submit_candidate(
                &worker,
                personal_result_request(created.task.id, url, role, author, key),
            )
            .unwrap();
        submission_ids.push(submitted.submission.id);
    }

    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: submission_ids.clone(),
                source_availability: vec![ReportedSourceAvailability {
                    source: "auth.example".into(),
                    state: SourceAvailabilityState::AuthenticationRequired,
                    reason: "authentication_required".into(),
                }],
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();

    assert!(batch.items.len() <= 6);
    assert!(batch
        .items
        .iter()
        .all(|item| !item.canonical_url.contains("blocked.example")));
    let d1 = batch
        .items
        .iter()
        .filter(|item| item.canonical_url.contains("d1.example"))
        .count();
    assert!(d1 <= 3, "domain cap exceeded: {d1}");
    assert!(batch
        .source_availability
        .iter()
        .any(|reason| matches!(reason, DiscoveryResultAvailabilityReason::DomainCap { .. })));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::AuthorOrAccountCap { .. }
    )));
    assert!(batch
        .source_availability
        .iter()
        .any(|reason| matches!(reason, DiscoveryResultAvailabilityReason::Blocked { .. })));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::CanonicalDuplicate { .. }
    )));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::AuthenticationAssistanceRequested { source, .. }
            if source == "auth.example"
    )));

    // Recent-result suppression for a second run.
    let second = claim_personal_run(&tools, &manager, &worker, Some(4), "recent-suppression");
    let first_url = batch.items[0].canonical_url.clone();
    let recent = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                second.task.id,
                &first_url,
                DiscoveryPlanSourceRole::Proven,
                Some("Z"),
                "recent-1",
            ),
        )
        .unwrap();
    let fresh = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                second.task.id,
                "https://fresh.example/new",
                DiscoveryPlanSourceRole::Proven,
                Some("Y"),
                "recent-2",
            ),
        )
        .unwrap();
    let suppressed = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: second.task.id,
                submission_ids: vec![recent.submission.id, fresh.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert!(suppressed
        .items
        .iter()
        .all(|item| item.canonical_url != first_url));
    assert!(suppressed.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::RecentlyReviewed { .. }
    )));
}

#[test]
fn underfilled_batch_records_reasons_without_inventing_results() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(10), "underfill");
    assert_eq!(created.plan.result_count, 10);
    assert_eq!(created.plan.allocation.proven, 7);
    assert_eq!(created.plan.allocation.adjacent, 3);

    let only = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://only.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "only",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![only.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(batch.items.len(), 1);
    assert_eq!(batch.requested_size, 10);
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::Underfilled {
            requested: 10,
            filled: 1
        }
    )));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::InsufficientProven { .. }
    )));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::InsufficientAdjacent { .. }
    )));
}

#[test]
fn completion_is_atomic_retry_safe_and_duplicate_submissions_do_not_inflate() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "atomic");

    let first_request = personal_result_request(
        created.task.id,
        "https://a.example/1",
        DiscoveryPlanSourceRole::Proven,
        None,
        "a1",
    );
    let first = tools
        .submit_candidate(&worker, first_request.clone())
        .unwrap();
    let retry_submit = tools.submit_candidate(&worker, first_request).unwrap();
    assert_eq!(first.submission.id, retry_submit.submission.id);

    let second = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://b.example/2",
                DiscoveryPlanSourceRole::Adjacent,
                None,
                "b1",
            ),
        )
        .unwrap();
    let request = CompleteDiscoveryResultBatchRequest {
        task_id: created.task.id,
        submission_ids: vec![
            first.submission.id,
            first.submission.id,
            second.submission.id,
        ],
        source_availability: Vec::new(),
        browser_grant_eligible_sources: None,
    };
    let batch = tools
        .complete_discovery_result_batch(&worker, request.clone(), Utc::now())
        .unwrap();
    assert_eq!(batch.items.len(), 2);
    let again = tools
        .complete_discovery_result_batch(&worker, request, Utc::now())
        .unwrap();
    assert_eq!(again.id, batch.id);
    assert_eq!(again.items, batch.items);
    let task = tools
        .discovery_task_status(&worker, created.task.id, Utc::now())
        .unwrap();
    assert_eq!(task.state, DiscoveryTaskState::Completed);
    let batches: Vec<_> = tools
        .store()
        .read()
        .unwrap()
        .discovery_result_batches
        .values()
        .filter(|batch| batch.task_id == created.task.id)
        .cloned()
        .collect();
    assert_eq!(batches.len(), 1);
}

#[test]
fn batch_states_and_notification_are_distinct_and_dismissal_creates_no_learning() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "states");
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://state.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "state-1",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(batch.state, DiscoveryResultBatchState::Ready);
    assert_eq!(
        batch.notification_state,
        DiscoveryResultNotificationState::NotApplicable
    );
    let task = tools
        .discovery_task_status(&worker, created.task.id, Utc::now())
        .unwrap();
    assert_eq!(task.state, DiscoveryTaskState::Completed);
    assert_ne!(
        serde_json::to_value(batch.state).unwrap(),
        serde_json::to_value(task.state).unwrap()
    );

    // Workers cannot dismiss.
    assert!(matches!(
        tools.dismiss_discovery_result_batch(&worker, batch.id, Utc::now()),
        Err(AgentToolsError::Forbidden { .. })
    ));

    let profile_before = tools.taste_profile(&manager).unwrap();
    let feedback_before = tools.store().read().unwrap().feedback_events.len();
    let dismissed = tools
        .dismiss_discovery_result_batch(&manager, batch.id, Utc::now())
        .unwrap();
    assert_eq!(dismissed.state, DiscoveryResultBatchState::Dismissed);
    assert!(dismissed.dismissed_at.is_some());
    let profile_after = tools.taste_profile(&manager).unwrap();
    assert_eq!(
        profile_after.interest_seed_evidence.active_seed_count,
        profile_before.interest_seed_evidence.active_seed_count
    );
    assert_eq!(profile_after.learned.len(), profile_before.learned.len());
    assert_eq!(
        tools.store().read().unwrap().feedback_events.len(),
        feedback_before
    );

    let other = claim_personal_run(&tools, &manager, &worker, Some(4), "reviewed-state");
    let item = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                other.task.id,
                "https://review.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "rev-1",
            ),
        )
        .unwrap();
    let ready = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: other.task.id,
                submission_ids: vec![item.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    let reviewed = tools
        .mark_discovery_result_batch_reviewed(&manager, ready.id, Utc::now())
        .unwrap();
    assert_eq!(reviewed.state, DiscoveryResultBatchState::Reviewed);
    assert!(reviewed.reviewed_at.is_some());
    assert_eq!(
        reviewed.notification_state,
        DiscoveryResultNotificationState::NotApplicable
    );
}

#[test]
fn batches_and_candidate_provenance_persist_privately_across_restart() {
    let root = std::env::temp_dir().join(format!("stumble-batch-persist-{}", uuid::Uuid::now_v7()));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "persist manager".into(),
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
                    label: "persist worker".into(),
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
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "persist");
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://persist.example/item",
                DiscoveryPlanSourceRole::Proven,
                Some("Writer"),
                "persist-1",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(
        tools.store().read().unwrap().discovery_result_batches.len(),
        1
    );
    drop(tools);

    let reopened = AgentTools::open_initialized_home_node(&root).unwrap();
    assert_eq!(
        reopened
            .store()
            .read()
            .unwrap()
            .discovery_result_batches
            .len(),
        1,
        "Discovery Result Batches must survive restart"
    );
    let owner = reopened.local_owner_auth_context().unwrap();
    let manager = {
        let issued = reopened
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "persist manager reopen".into(),
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
    let listed = reopened.list_discovery_result_batches(&manager).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, batch.id);
    assert_eq!(
        listed[0].items[0].canonical_url,
        batch.items[0].canonical_url
    );
    let inspected = reopened.discovery_result_batch(&manager, batch.id).unwrap();
    assert_eq!(inspected.plan_id, created.plan.id);
    let submission = reopened
        .store()
        .read()
        .unwrap()
        .candidate_submissions
        .get(&submitted.submission.id)
        .cloned()
        .unwrap();
    assert_eq!(
        submission.evidence.provenance.discovery_method,
        "browser_search"
    );

    // Private / non-federated: batch markers absent from public federation export.
    let federation = reopened.default_auth_context().unwrap();
    let outbound = serde_json::to_string(&serde_json::json!({
        "node": reopened.node_info(&federation).unwrap(),
        "pods": reopened.list_public_pods(&federation).unwrap(),
    }))
    .unwrap();
    for forbidden in [
        "DiscoveryResultBatch",
        "discovery_result",
        "persist.example",
        &batch.id.to_string(),
    ] {
        assert!(
            !outbound.contains(forbidden),
            "federated surface leaked {forbidden}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}
