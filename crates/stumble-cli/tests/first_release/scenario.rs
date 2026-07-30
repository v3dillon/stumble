use crate::common::*;
use chrono::{Duration, Utc};
use serde_json::json;
use stumble_api::router_with_base_url;
use stumble_core::*;

pub(crate) struct TwoNodeScenario {
    pub(crate) home_dir: TestDataDir,
    pub(crate) _origin_dir: TestDataDir,
    pub(crate) home: AgentTools,
    pub(crate) origin: AgentTools,
    pub(crate) primary_pod: Pod,
    pub(crate) secondary_pod: Pod,
    pub(crate) worker: AuthContext,
    pub(crate) worker_token: String,
    pub(crate) user: AuthContext,
    pub(crate) user_token: String,
}

pub(crate) struct DiscoveryEvidence {
    pub(crate) task: DiscoveryTask,
    pub(crate) local_item_id: ContentItemId,
}

pub(crate) struct FederationEvidence {
    pub(crate) _server: OriginServer,
    pub(crate) public_origin_pod: Pod,
    pub(crate) origin_owner: AuthContext,
    pub(crate) origin_content_item_id: ContentItemId,
    pub(crate) subscription: Subscription,
    pub(crate) local_placement: PodPlacement,
    pub(crate) synchronized_content_item_id: ContentItemId,
}

pub(crate) struct FeedMixEvidence {
    pub(crate) exploration_content_item_id: ContentItemId,
    pub(crate) competing_content_item_ids: Vec<ContentItemId>,
}

pub(crate) struct CompositionEvidence {
    pub(crate) batch: FeedBatch,
    pub(crate) score_before_feedback: f32,
}

pub(crate) fn arrange_two_node_scenario() -> TwoNodeScenario {
    // Arrange: two independent real SQLite nodes and capability-scoped harnesses.
    let home_dir = TestDataDir::initialize_with_stumble("home");
    let origin_dir = TestDataDir::initialize_with_stumble("origin");
    let home = AgentTools::open_initialized_home_node(&home_dir.0).unwrap();
    let origin = AgentTools::open_initialized_home_node(&origin_dir.0).unwrap();
    let (bootstrap, _) = harness(
        &home,
        "Interactive Pod bootstrap",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::PackageManagement,
        ],
        None,
    );
    let primary_pod = home
        .create_private_pod_with_package(
            &bootstrap,
            CreatePrivatePodWithPackageRequest {
                name: "Resilient systems".into(),
                slug: "resilient-systems".into(),
                description: "Production resilience reports".into(),
                package: package(),
            },
        )
        .unwrap()
        .pod;
    let secondary_pod = home
        .create_pod(
            &bootstrap,
            CreatePodRequest {
                name: "Recovery practice".into(),
                slug: "recovery-practice".into(),
                description: "Practical recovery guidance".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let (worker, worker_token) = harness(
        &home,
        "Scoped unattended discovery worker private needle",
        AgentHarnessKind::Unattended,
        vec![
            HarnessCapability::DiscoveryTasks,
            HarnessCapability::CandidateSubmission,
        ],
        Some(vec![primary_pod.id, secondary_pod.id]),
    );
    let (user, user_token) = harness(
        &home,
        "Interactive Feed operator private needle",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::CandidateSubmission,
            HarnessCapability::PodCuration,
            HarnessCapability::SubscriptionManagement,
        ],
        None,
    );
    for pod in [&primary_pod, &secondary_pod] {
        home.join_pod(&user, &pod.slug).unwrap();
        let pod_id = pod.id;
        home.set_pod_curation_policy(&user, pod_id, CurationPolicy::Manual, Utc::now())
            .unwrap();
    }
    TwoNodeScenario {
        home_dir,
        _origin_dir: origin_dir,
        home,
        origin,
        primary_pod,
        secondary_pod,
        worker,
        worker_token,
        user,
        user_token,
    }
}

pub(crate) fn discover_and_curate_local_content(
    scenario: &TwoNodeScenario,
    now: chrono::DateTime<Utc>,
) -> DiscoveryEvidence {
    // Act: wake, claim, submit, complete, curate, and route one task-driven Candidate.
    let task = materialize_and_wake_discovery(
        &scenario.home,
        &scenario.home_dir,
        &scenario.worker,
        &scenario.worker_token,
        scenario.primary_pod.id,
        now,
    );
    let claimed = scenario
        .home
        .claim_discovery_task(
            &scenario.worker,
            task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let submitted = submit_candidate(
        &scenario.home,
        &scenario.worker,
        &claimed,
        &[scenario.primary_pod.id],
        "https://example.com/control-plane-recovery?utm_source=harness",
    );
    scenario
        .home
        .complete_discovery_task(&scenario.worker, task.id, now + Duration::seconds(1))
        .unwrap();
    let curated = scenario
        .home
        .curate_candidate(&scenario.user, submitted.candidate.id, now)
        .unwrap();

    // Assert: curation starts at the task Pod and retains one canonical identity when routed.
    assert_eq!(curated.placements.len(), 1);
    let first_placement = scenario
        .home
        .review_candidate_placement(
            &scenario.user,
            submitted.candidate.id,
            scenario.primary_pod.id,
            PlacementReviewDecision::Accept,
            None,
            now,
        )
        .unwrap();
    scenario
        .home
        .route_candidate_placement(
            &scenario.user,
            submitted.candidate.id,
            RouteCandidatePlacementRequest::new(
                scenario.secondary_pod.id,
                "Independent recovery-practice evidence",
                CandidateConfidence::new(0.9).unwrap(),
            )
            .unwrap(),
            now,
        )
        .unwrap();
    let second_placement = scenario
        .home
        .review_candidate_placement(
            &scenario.user,
            submitted.candidate.id,
            scenario.secondary_pod.id,
            PlacementReviewDecision::Accept,
            None,
            now,
        )
        .unwrap();
    assert_eq!(
        first_placement.content_item_id,
        second_placement.content_item_id
    );
    DiscoveryEvidence {
        task,
        local_item_id: first_placement.content_item_id.unwrap(),
    }
}

pub(crate) async fn withdraw_and_synchronize_origin_placement(
    scenario: &TwoNodeScenario,
    federation: &FederationEvidence,
    now: chrono::DateTime<Utc>,
) {
    // Arrange and Act: independently approve the public withdrawal.
    let (withdrawer, _) = harness(
        &scenario.origin,
        "Origin withdrawal proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![federation.public_origin_pod.id]),
    );
    let (approver, _) = harness(
        &scenario.origin,
        "Origin withdrawal approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        Some(vec![federation.public_origin_pod.id]),
    );
    let outcome = scenario
        .origin
        .request_remove_submission_from_pod(
            &withdrawer,
            &federation.public_origin_pod.slug,
            federation.origin_content_item_id.into(),
            now + Duration::days(32),
        )
        .unwrap();
    let RemoveSubmissionOutcome::PendingApproval(proposal) = outcome else {
        panic!("public withdrawal must require independent approval");
    };
    scenario
        .origin
        .approve_pending_proposal(&approver, proposal.id, now + Duration::days(32))
        .unwrap();
    let incremental = scenario
        .origin
        .federation_pod_snapshot(
            &federation.origin_owner,
            &federation.public_origin_pod.slug,
            federation.subscription.last_event_hash.as_deref(),
        )
        .unwrap();

    // Act and Assert: reject tampering, then fetch and project the valid cursor segment.
    let mut tampered = incremental;
    tampered.events.first_mut().unwrap().payload_json["placement_tombstone"]["withdrawn_at"] =
        json!("2020-01-01T00:00:00Z");
    assert!(matches!(
        scenario.home.synchronize_subscription(
            &scenario.user,
            federation.subscription.id,
            tampered
        ),
        Err(AgentToolsError::Store(StoreError::InvalidSignature))
    ));
    let synchronized = stumble_sync::synchronize_subscription_from_origin(
        &scenario.home,
        &scenario.user,
        federation.subscription.id,
    )
    .await
    .unwrap();
    assert_eq!(synchronized.imported_events, 1);
    let retained = scenario
        .home
        .pod_placement(
            &scenario.user,
            federation.local_placement.candidate_id,
            scenario.primary_pod.id,
        )
        .unwrap();
    assert_eq!(retained.status, PodPlacementStatus::Accepted);
    assert_eq!(
        retained.origin_placements,
        federation.local_placement.origin_placements
    );
    assert_eq!(retained.origin_withdrawals.len(), 1);
}

pub(crate) async fn establish_federation(
    scenario: &TwoNodeScenario,
    now: chrono::DateTime<Utc>,
) -> FederationEvidence {
    let public_origin_pod = create_public_pod(&scenario.origin, "origin-operations");
    let origin_content_item_id =
        accept_origin_content_item_placement(&scenario.origin, &public_origin_pod);
    let origin_owner = scenario.origin.default_auth_context().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let origin_router = router_with_base_url(scenario.origin.clone(), &base_url);
    let server = OriginServer {
        base_url: base_url.clone(),
        task: tokio::spawn(async move { axum::serve(listener, origin_router).await }),
    };

    // Retain a signed Explore sample without making its Pod Feed-eligible.
    let exploration_pod = create_public_pod(&scenario.origin, "exploration-operations");
    accept_origin_content_item_placement(&scenario.origin, &exploration_pod);
    let announcement = scenario
        .origin
        .pod_announcement(
            &origin_owner,
            &exploration_pod.slug,
            &format!(
                "{}/federation/pods/{}",
                server.base_url, exploration_pod.slug
            ),
        )
        .unwrap();
    let samples = scenario
        .origin
        .pod_explore_samples(&origin_owner, &announcement, 1)
        .unwrap();
    scenario.home.index_pod_announcement(announcement).unwrap();
    scenario
        .home
        .accept_pod_explore_samples(&scenario.user, samples)
        .unwrap();
    let explore = scenario
        .home
        .explore_public_pods(
            &scenario.user,
            ExploreRequest::new("exploration", 10, 1).unwrap(),
        )
        .unwrap();
    assert_eq!(explore.results.len(), 1);
    assert!(!explore.results[0].is_subscribed);
    assert_eq!(explore.results[0].sample_content_references.len(), 1);

    let synchronized = stumble_sync::subscribe_pod_from_url(
        &scenario.home,
        &scenario.user,
        &format!("{}/federation/pods/origin-operations", server.base_url),
    )
    .await
    .unwrap();
    scenario
        .home
        .set_priority_subscription(&scenario.user, synchronized.subscription.local_pod_id, true)
        .unwrap();
    let synchronized_content_item_id = scenario
        .home
        .accepted_placements_for_pod(&scenario.user, synchronized.subscription.local_pod_id)
        .unwrap()[0]
        .content_item_id;
    let local_placement = scenario
        .home
        .add_content_item_to_pod(
            &scenario.user,
            AddContentItemToPodRequest::new(
                synchronized_content_item_id,
                scenario.primary_pod.id,
                Some("Keep this recovery report locally".into()),
            )
            .unwrap(),
            now,
        )
        .unwrap();
    assert_eq!(local_placement.origin_placements.len(), 1);

    FederationEvidence {
        _server: server,
        public_origin_pod,
        origin_owner,
        origin_content_item_id,
        subscription: synchronized.subscription,
        local_placement,
        synchronized_content_item_id,
    }
}

pub(crate) fn arrange_feed_mix_evidence(
    scenario: &TwoNodeScenario,
    now: chrono::DateTime<Utc>,
) -> FeedMixEvidence {
    // A different local User publishes one Feed-eligible Exploration Pod.
    let (exploration_curator, publication_approver) = grant_alternate_curator(&scenario.home, now);
    let exploration_pod = scenario
        .home
        .create_pod(
            &exploration_curator,
            CreatePodRequest {
                name: "Independent exploration".into(),
                slug: "independent-exploration".into(),
                description: "Public exploration outside the Feed User's memberships".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    scenario
        .home
        .set_pod_curation_policy(
            &exploration_curator,
            exploration_pod.id,
            CurationPolicy::Manual,
            now,
        )
        .unwrap();
    let exploration_content_item_id = accept_local_candidate(
        &scenario.home,
        &exploration_curator,
        &exploration_pod,
        "exploration-release",
        now,
    );
    let publication = match scenario
        .home
        .request_set_pod_visibility(
            &exploration_curator,
            exploration_pod.id,
            Visibility::Public,
            now,
        )
        .unwrap()
    {
        PodVisibilityOutcome::PendingApproval(proposal) => proposal,
        PodVisibilityOutcome::Updated(_) => panic!("publication must require approval"),
    };
    scenario
        .home
        .approve_pending_proposal(&publication_approver, publication.id, now)
        .unwrap();

    // Two non-priority candidates compete for the remaining subscribed slot.
    let competing_content_item_ids = ["competing-alpha", "competing-beta"]
        .into_iter()
        .map(|label| {
            accept_local_candidate(
                &scenario.home,
                &scenario.user,
                &scenario.secondary_pod,
                label,
                now,
            )
        })
        .collect();
    FeedMixEvidence {
        exploration_content_item_id,
        competing_content_item_ids,
    }
}

pub(crate) fn prove_complete_feed_composition(
    scenario: &TwoNodeScenario,
    discovery: &DiscoveryEvidence,
    federation: &FederationEvidence,
    feed_mix: &FeedMixEvidence,
    now: chrono::DateTime<Utc>,
) -> CompositionEvidence {
    let complete_mix = FeedMix::new(50, 25, 25, 3, 2).unwrap();
    // Four slots make subscribed, Exploration, and Old Gem quotas independently observable.
    let mixed = scenario
        .home
        .get_feed_batch(
            &scenario.user,
            FeedBatchRequest::new(4)
                .unwrap()
                .with_feed_mix(complete_mix),
            now,
        )
        .unwrap();
    assert_eq!(mixed.items.len(), 4);
    let exploration = mixed
        .items
        .iter()
        .find(|item| item.kind == FeedItemKind::Exploration)
        .unwrap();
    assert!(exploration.is_exploration);
    assert_eq!(
        exploration.content_reference.content_item_id,
        feed_mix.exploration_content_item_id
    );
    let local = mixed
        .items
        .iter()
        .find(|item| item.content_reference.content_item_id == discovery.local_item_id)
        .unwrap();
    assert_eq!(local.kind, FeedItemKind::OldGem);
    let score_before_feedback = local.ranking_evidence.attention_value;
    let priority_remote = mixed
        .items
        .iter()
        .find(|item| {
            item.content_reference.content_item_id == federation.synchronized_content_item_id
        })
        .unwrap();
    assert_eq!(priority_remote.kind, FeedItemKind::Subscribed);
    assert!(priority_remote
        .placements
        .iter()
        .any(|placement| placement.pod_id == federation.subscription.local_pod_id));
    assert!(priority_remote
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason == "Priority Subscription guaranteed bounded representation"));
    assert_eq!(
        mixed
            .items
            .iter()
            .filter(|item| feed_mix
                .competing_content_item_ids
                .contains(&item.content_reference.content_item_id))
            .count(),
        1
    );
    assert_eq!(
        scenario
            .home
            .get_feed_batch(
                &scenario.user,
                FeedBatchRequest::new(4)
                    .unwrap()
                    .with_feed_mix(complete_mix),
                now,
            )
            .unwrap(),
        mixed
    );
    CompositionEvidence {
        batch: mixed,
        score_before_feedback,
    }
}

pub(crate) fn apply_feedback_and_prove_reranking(
    scenario: &TwoNodeScenario,
    discovery: &DiscoveryEvidence,
    federation: &FederationEvidence,
    composition: &CompositionEvidence,
    now: chrono::DateTime<Utc>,
) -> FeedBatch {
    scenario
        .home
        .record_feed_feedback(
            &scenario.user,
            federation.synchronized_content_item_id,
            FeedbackKind::Interesting,
            None,
            Some("private feedback needle".into()),
            now,
        )
        .unwrap();
    scenario
        .home
        .record_feed_feedback(
            &scenario.user,
            discovery.local_item_id,
            FeedbackKind::Saved,
            None,
            None,
            now,
        )
        .unwrap();
    assert!(scenario
        .home
        .taste_profile(&scenario.user)
        .unwrap()
        .learned
        .iter()
        .any(|weight| {
            weight.signal == LearnedTasteSignal::Topic("resilience".into()) && weight.weight > 0.0
        }));
    scenario
        .home
        .complete_feed_batch(&scenario.user, composition.batch.id, now)
        .unwrap();
    let ranked = scenario
        .home
        .get_feed_batch(
            &scenario.user,
            FeedBatchRequest::new(4)
                .unwrap()
                .with_feed_mix(FeedMix::new(50, 25, 25, 3, 2).unwrap())
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + Duration::seconds(1),
        )
        .unwrap();
    let ranked_local = ranked
        .items
        .iter()
        .find(|item| item.content_reference.content_item_id == discovery.local_item_id)
        .unwrap();
    assert!(ranked_local.ranking_evidence.attention_value > composition.score_before_feedback);
    assert!(ranked_local
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason.contains("Learned topic 'resilience' affinity increased value")));
    ranked
}

pub(crate) fn prove_unavailable_category_backfill(
    scenario: &TwoNodeScenario,
    ranked: &FeedBatch,
    now: chrono::DateTime<Utc>,
) {
    // Once all categories were just delivered, recurrence zero makes only Old Gems eligible;
    // unavailable subscribed and Exploration quotas backfill to the requested finite size.
    scenario
        .home
        .complete_feed_batch(&scenario.user, ranked.id, now + Duration::seconds(1))
        .unwrap();
    let backfilled = scenario
        .home
        .get_feed_batch(
            &scenario.user,
            FeedBatchRequest::new(2)
                .unwrap()
                .with_feed_mix(FeedMix::new(50, 50, 0, 3, 2).unwrap())
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + Duration::seconds(2),
        )
        .unwrap();
    assert_eq!(backfilled.items.len(), 2);
    assert!(backfilled
        .items
        .iter()
        .all(|item| item.kind == FeedItemKind::OldGem));
}

pub(crate) fn deliver_local_item_for_old_gem(
    scenario: &TwoNodeScenario,
    discovery: &DiscoveryEvidence,
    now: chrono::DateTime<Utc>,
) {
    let delivered_at = now - Duration::days(31);
    let first = scenario
        .home
        .get_feed_batch(
            &scenario.user,
            FeedBatchRequest::new(100).unwrap(),
            delivered_at,
        )
        .unwrap();
    assert!(first
        .items
        .iter()
        .any(|item| item.content_reference.content_item_id == discovery.local_item_id));
    scenario
        .home
        .complete_feed_batch(&scenario.user, first.id, delivered_at)
        .unwrap();
}

pub(crate) fn prove_restart(scenario: &TwoNodeScenario, federation: &FederationEvidence) {
    let candidate_id = federation.local_placement.candidate_id;
    let primary_pod_id = scenario.primary_pod.id;
    let restarted = AgentTools::open_home_node(&scenario.home_dir.0, seed_store).unwrap();
    let restarted_user = restarted
        .authenticate_token(&scenario.user_token)
        .unwrap()
        .unwrap();
    let retained = restarted
        .pod_placement(&restarted_user, candidate_id, primary_pod_id)
        .unwrap();
    assert_eq!(retained.status, PodPlacementStatus::Accepted);
    assert_eq!(retained.origin_withdrawals.len(), 1);
}
