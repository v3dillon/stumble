use chrono::Utc;
use stumble_core::*;

use crate::common::*;

#[test]
fn save_creates_inbox_placement_with_original_provenance() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let (_created, batch, submitted) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "save-inbox",
        "https://save.example/article?utm=1",
    );
    let candidate_id = batch.items[0].candidate_id;
    let now = Utc::now();

    let outcome = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::Save,
            },
            now,
        )
        .unwrap();

    assert_eq!(outcome.batch.state, DiscoveryResultBatchState::Ready);
    assert!(matches!(
        outcome.item.review,
        DiscoveryResultItemReview::Reviewed {
            action: DiscoveryResultItemAction::Save,
            ..
        }
    ));
    let placement = outcome.placement.expect("Save creates a placement");
    assert_eq!(placement.status, PodPlacementStatus::Accepted);
    assert_eq!(placement.curation_path, CurationPath::AddToPod);
    assert_eq!(
        placement.source_submission_ids,
        vec![submitted.submission.id]
    );
    let pod = tools
        .store()
        .read()
        .unwrap()
        .pods
        .get(&placement.pod_id)
        .cloned()
        .unwrap();
    assert_eq!(pod.visibility, Visibility::Private);
    assert_eq!(pod.name, "Inbox");
    assert_eq!(pod.created_by, manager.user_id);
    let content = tools
        .store()
        .read()
        .unwrap()
        .submissions
        .get(&uuid::Uuid::from(placement.content_item_id.unwrap()))
        .cloned()
        .unwrap();
    assert_eq!(content.canonical_url, submitted.candidate.canonical_url);
    // Save does not create learning evidence by itself.
    assert!(outcome
        .taste_profile
        .source_affinities
        .iter()
        .all(|affinity| { affinity.supporting_feedback == 0 && affinity.opposing_feedback == 0 }));
}

#[test]
fn add_to_pod_respects_role_grant_and_public_policy_boundaries() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let curator = harness(
        &tools,
        "curator manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::PodCuration,
            HarnessCapability::Feedback,
        ],
    );
    let worker = personal_worker(&tools);
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: "Curation Target".into(),
                slug: "curation-target".into(),
                description: "authorized private pod".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let (_created, batch, submitted) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "add-pod",
        "https://place.example/item",
    );
    let candidate_id = batch.items[0].candidate_id;

    // Personal Discovery management alone cannot bypass Pod Role / grant boundaries.
    let denied = tools.review_discovery_result_item(
        &manager,
        ReviewDiscoveryResultItemRequest {
            batch_id: batch.id,
            candidate_id,
            action: DiscoveryResultItemActionRequest::AddToPod {
                pod_id: pod.id,
                curation_note: None,
            },
        },
        Utc::now(),
    );
    assert!(
        matches!(denied, Err(AgentToolsError::Forbidden { .. })),
        "unexpected: {denied:?}"
    );

    // Curator with PodCuration + Owner role may place.
    let outcome = tools
        .review_discovery_result_item(
            &curator,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::AddToPod {
                    pod_id: pod.id,
                    curation_note: Some(CurationRationale::new("fits the pod").unwrap()),
                },
            },
            Utc::now(),
        )
        .unwrap();
    let placement = outcome.placement.expect("placement");
    assert_eq!(placement.pod_id, pod.id);
    assert_eq!(placement.status, PodPlacementStatus::Accepted);
    assert_eq!(
        placement.source_submission_ids,
        vec![submitted.submission.id]
    );
    assert!(outcome
        .allowed_actions
        .contains(&DiscoveryResultAllowedAction::AddToPod));
}

#[test]
fn more_like_this_and_not_for_me_create_replaceable_learning_evidence() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let (_created, batch, _submitted) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "learn-item",
        "https://learn.example/post",
    );
    let candidate_id = batch.items[0].candidate_id;
    let now = Utc::now();

    let reinforced = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::MoreLikeThis,
            },
            now,
        )
        .unwrap();
    assert!(!reinforced.action_replaced);
    let evidence_after_first = feedback_signal_total(&reinforced.taste_profile);
    assert!(evidence_after_first > 0);
    assert!(reinforced.taste_profile.source_affinities.iter().any(|a| {
        a.signal == SourceAffinitySignal::Source("learn.example".into())
            && a.supporting_feedback > 0
    }));

    // Repeat is idempotent — evidence count must not inflate.
    let repeated = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::MoreLikeThis,
            },
            now,
        )
        .unwrap();
    assert!(!repeated.action_replaced);
    assert_eq!(
        feedback_signal_total(&repeated.taste_profile),
        evidence_after_first
    );

    // Changing action replaces evidence rather than stacking.
    let rejected = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::NotForMe,
            },
            now,
        )
        .unwrap();
    assert!(rejected.action_replaced);
    assert!(matches!(
        rejected.item.review,
        DiscoveryResultItemReview::Reviewed {
            action: DiscoveryResultItemAction::NotForMe,
            replaced_action: Some(DiscoveryResultItemAction::MoreLikeThis),
            ..
        }
    ));
    let affinities = &rejected.taste_profile.source_affinities;
    let source = affinities
        .iter()
        .find(|a| a.signal == SourceAffinitySignal::Source("learn.example".into()))
        .expect("source affinity");
    assert_eq!(source.supporting_feedback, 0);
    assert!(source.opposing_feedback > 0);
}

#[test]
fn ignore_dismiss_and_batch_review_create_no_learning_evidence() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let (_created, batch, _) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "ignore-item",
        "https://ignore.example/1",
    );
    let candidate_id = batch.items[0].candidate_id;
    let evidence_before = feedback_signal_total(&tools.taste_profile(&manager).unwrap());

    let ignored = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::Ignore,
            },
            Utc::now(),
        )
        .unwrap();
    assert!(matches!(
        ignored.item.review,
        DiscoveryResultItemReview::Reviewed {
            action: DiscoveryResultItemAction::Ignore,
            ..
        }
    ));
    assert_eq!(
        feedback_signal_total(&ignored.taste_profile),
        evidence_before
    );
    // Item review does not complete the batch.
    assert_eq!(ignored.batch.state, DiscoveryResultBatchState::Ready);

    let other = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "dismiss-no-learn",
        "https://dismiss.example/1",
    );
    let dismissed = tools
        .dismiss_discovery_result_batch(&manager, other.1.id, Utc::now())
        .unwrap();
    assert_eq!(dismissed.state, DiscoveryResultBatchState::Dismissed);
    assert_eq!(
        feedback_signal_total(&tools.taste_profile(&manager).unwrap()),
        evidence_before
    );

    let third = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "mark-reviewed-no-learn",
        "https://mark.example/1",
    );
    let reviewed = tools
        .mark_discovery_result_batch_reviewed(&manager, third.1.id, Utc::now())
        .unwrap();
    assert_eq!(reviewed.state, DiscoveryResultBatchState::Reviewed);
    assert_eq!(
        feedback_signal_total(&tools.taste_profile(&manager).unwrap()),
        evidence_before
    );
    // Batch reviewed remains distinct from item Save / placement.
    assert!(reviewed
        .items
        .iter()
        .all(|item| { matches!(item.review, DiscoveryResultItemReview::Unreviewed) }));
}

#[test]
fn feedback_changes_next_plan_while_blocks_override_and_rejection_suppresses_rediscovery() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);

    // Two independent More like this actions corroborate source affinity weight.
    for (idx, url) in [
        "https://corroborate.example/a",
        "https://corroborate.example/b",
    ]
    .into_iter()
    .enumerate()
    {
        let (_c, batch, _) = complete_one_result_batch(
            &tools,
            &manager,
            &worker,
            &format!("corroborate-{idx}"),
            url,
        );
        tools
            .review_discovery_result_item(
                &manager,
                ReviewDiscoveryResultItemRequest {
                    batch_id: batch.id,
                    candidate_id: batch.items[0].candidate_id,
                    action: DiscoveryResultItemActionRequest::MoreLikeThis,
                },
                Utc::now(),
            )
            .unwrap();
    }

    let planned = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: Some(4),
                idempotency_key: "after-feedback".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert!(
        planned.plan.source_neighborhoods.iter().any(|source| {
            source.signal == SourceAffinitySignal::Source("corroborate.example".into())
                && source.rationale.contains("corroborated")
        }),
        "next plan should reflect reinforced source: {:?}",
        planned
            .plan
            .source_neighborhoods
            .iter()
            .map(|s| (&s.signal, &s.rationale))
            .collect::<Vec<_>>()
    );

    // Explicit block overrides learned evidence.
    let mut taste = UpdateTasteProfileRequest::default();
    taste.blocked_sources = Some(vec!["corroborate.example".into()]);
    tools.update_taste_profile(&manager, taste).unwrap();
    let blocked_plan = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: Some(4),
                idempotency_key: "after-block".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert!(blocked_plan.plan.source_neighborhoods.iter().all(|source| {
        source.signal != SourceAffinitySignal::Source("corroborate.example".into())
    }));

    // Rejected result cannot be rediscovered via equivalent URL spelling.
    let reject_run = claim_personal_run(&tools, &manager, &worker, Some(4), "reject-run");
    let rejected_submit = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                reject_run.task.id,
                "https://reject.example/story",
                DiscoveryPlanSourceRole::Proven,
                None,
                "reject-sub",
            ),
        )
        .unwrap();
    let reject_batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: reject_run.task.id,
                submission_ids: vec![rejected_submit.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: reject_batch.id,
                candidate_id: reject_batch.items[0].candidate_id,
                action: DiscoveryResultItemActionRequest::NotForMe,
            },
            Utc::now(),
        )
        .unwrap();

    let next = claim_personal_run(&tools, &manager, &worker, Some(4), "reject-rediscover");
    let equivalent = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                next.task.id,
                "https://reject.example/story?utm_source=agent",
                DiscoveryPlanSourceRole::Proven,
                None,
                "reject-equiv",
            ),
        )
        .unwrap();
    let next_batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: next.task.id,
                submission_ids: vec![equivalent.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert!(
        next_batch.items.is_empty()
            || next_batch.items.iter().all(|item| {
                !item
                    .canonical_url
                    .starts_with("https://reject.example/story")
            }),
        "rejected URL must not reappear: {:?}",
        next_batch.items
    );
    assert!(next_batch.source_availability.iter().any(|reason| {
        matches!(
            reason,
            DiscoveryResultAvailabilityReason::RecentlyReviewed { canonical_url }
                if canonical_url.starts_with("https://reject.example/story")
        )
    }));
}

#[test]
fn item_review_placement_learning_and_batch_state_commit_atomically_and_survive_restart() {
    let root =
        std::env::temp_dir().join(format!("stumble-review-persist-{}", uuid::Uuid::now_v7()));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "review manager".into(),
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
                    label: "review worker".into(),
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
    let (_created, batch, submitted) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "atomic-review",
        "https://atomic.example/item",
    );
    let outcome = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id: batch.items[0].candidate_id,
                action: DiscoveryResultItemActionRequest::Save,
            },
            Utc::now(),
        )
        .unwrap();
    let placement_id = outcome.placement.as_ref().map(|p| p.pod_id).unwrap();
    tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id: batch.items[0].candidate_id,
                action: DiscoveryResultItemActionRequest::MoreLikeThis,
            },
            Utc::now(),
        )
        .unwrap();
    let evidence_len = feedback_signal_total(&tools.taste_profile(&manager).unwrap());
    assert!(evidence_len > 0);
    drop(tools);

    let reopened = AgentTools::open_initialized_home_node(&root).unwrap();
    let owner = reopened.local_owner_auth_context().unwrap();
    let manager = {
        let issued = reopened
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "review manager reopen".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::PersonalDiscoveryManagement,
                        HarnessCapability::Feedback,
                    ],
                    pod_ids: None,
                },
            )
            .unwrap();
        reopened
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    let inspected = reopened.discovery_result_batch(&manager, batch.id).unwrap();
    assert!(matches!(
        inspected.items[0].review,
        DiscoveryResultItemReview::Reviewed {
            action: DiscoveryResultItemAction::MoreLikeThis,
            placement_pod_id: Some(pod_id),
            ..
        } if pod_id == placement_id
    ));
    assert_eq!(
        feedback_signal_total(&reopened.taste_profile(&manager).unwrap()),
        evidence_len
    );
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .pod_placements
        .values()
        .any(|placement| {
            placement.pod_id == placement_id
                && placement
                    .source_submission_ids
                    .contains(&submitted.submission.id)
                && placement.status == PodPlacementStatus::Accepted
        }));
    // Private: review markers stay off public federation surfaces.
    let federation = reopened.default_auth_context().unwrap();
    let outbound = serde_json::to_string(&serde_json::json!({
        "pods": reopened.list_public_pods(&federation).unwrap(),
    }))
    .unwrap();
    assert!(!outbound.contains("atomic.example"));
    let _ = std::fs::remove_dir_all(root);
}
