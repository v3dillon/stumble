use chrono::{TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-feed-batches-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
    pod_ids: Option<Vec<PodId>>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Interactive,
                capabilities,
                pod_ids,
            },
        )
        .unwrap();
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

fn accepted_item(tools: &AgentTools, slug: &str, ordinal: usize) -> (Pod, ContentItemId) {
    let curator = harness(
        tools,
        &format!("curator-{ordinal}"),
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::SubscriptionManagement,
        ],
        None,
    );
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: slug.into(),
                slug: slug.into(),
                description: "Feed acceptance Pod".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    tools.join_pod(&curator, &pod.slug).unwrap();
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitter = harness(
        tools,
        &format!("submitter-{ordinal}"),
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Strong subject match".into(),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://source{ordinal}.example/report"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Report {ordinal}")),
                        author: Some("Researcher".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted excerpt".into()),
                    summary: Some(format!("A useful report about topic-{ordinal}")),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec![format!("topic-{ordinal}")],
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
                        discovery_method: "interactive_search".into(),
                        referrer_url: Some("https://search.example".into()),
                    },
                    harness_idempotency_key: format!("feed-harness-{ordinal}"),
                    client_idempotency_key: format!("feed-client-{ordinal}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    let placement = tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
    (pod, placement.content_item_id.unwrap())
}

fn accepted_item_in_pod(tools: &AgentTools, pod: &Pod, ordinal: usize) -> ContentItemId {
    let curator = harness(
        tools,
        &format!("pod-curator-{ordinal}"),
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    let submitter = harness(
        tools,
        &format!("pod-submitter-{ordinal}"),
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Strong subject match".into(),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://pod-source{ordinal}.example/report"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Pod report {ordinal}")),
                        author: Some("Researcher".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted excerpt".into()),
                    summary: Some(format!("A useful report about pod-topic-{ordinal}")),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec![format!("pod-topic-{ordinal}")],
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
                        discovery_method: "interactive_search".into(),
                        referrer_url: Some("https://search.example".into()),
                    },
                    harness_idempotency_key: format!("pod-harness-{ordinal}"),
                    client_idempotency_key: format!("pod-client-{ordinal}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap()
        .content_item_id
        .unwrap()
}

fn make_unsubscribed_public(tools: &AgentTools, pod: &Pod) {
    let shared_store = tools.store();
    let mut store = shared_store.write().unwrap();
    store.pods.get_mut(&pod.id).unwrap().visibility = Visibility::Public;
    store
        .subscriptions
        .retain(|_, subscription| subscription.local_pod_id != pod.id);
}

#[test]
fn feed_batch_is_finite_stable_and_reaches_caught_up() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    accepted_item(&tools, "feed-one", 1);
    accepted_item(&tools, "feed-two", 2);
    let feed = harness(
        &tools,
        "feed reader",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::PodCuration,
        ],
        None,
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();

    let first = tools
        .get_feed_batch(&feed, FeedBatchRequest::new(1).unwrap(), now)
        .unwrap();
    let repeated = tools
        .get_feed_batch(&feed, FeedBatchRequest::new(1).unwrap(), now)
        .unwrap();

    assert_eq!(first, repeated);
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.state, FeedBatchState::Ready);
    assert!(!first.items[0].placements.is_empty());
    assert!(!first.items[0].provenance.is_empty());
    assert!(!first.items[0].ranking_evidence.reasons.is_empty());
    assert!(!first.items[0].is_exploration);
    assert_eq!(first.items[0].allowed_actions.len(), 7);

    let completed = tools.complete_feed_batch(&feed, first.id, now).unwrap();
    assert_eq!(completed.state, FeedBatchState::CaughtUp);
    let second = tools
        .get_feed_batch(&feed, FeedBatchRequest::new(1).unwrap(), now)
        .unwrap();
    assert_ne!(second.id, first.id);
    assert_eq!(second.items.len(), 1);
    tools.complete_feed_batch(&feed, second.id, now).unwrap();

    let caught_up = tools
        .get_feed_batch(&feed, FeedBatchRequest::new(1).unwrap(), now)
        .unwrap();
    assert_eq!(caught_up.state, FeedBatchState::CaughtUp);
    assert!(caught_up.items.is_empty());
}

#[test]
fn source_and_topic_blocks_exclude_matching_items_from_later_batches() {
    let tools = AgentTools::new(seed_store());
    let (_, source_blocked_id) = accepted_item(&tools, "block-source", 21);
    let (_, topic_blocked_id) = accepted_item(&tools, "block-topic", 22);
    let user = harness(
        &tools,
        "blocking user",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
        None,
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let initial = tools
        .get_feed_batch(&user, FeedBatchRequest::new(2).unwrap(), now)
        .unwrap();
    tools
        .record_feed_feedback(
            &user,
            source_blocked_id,
            FeedbackKind::BlockSource,
            None,
            None,
            now,
        )
        .unwrap();
    tools
        .record_feed_feedback(
            &user,
            topic_blocked_id,
            FeedbackKind::BlockTopic,
            Some("topic-22".into()),
            None,
            now,
        )
        .unwrap();
    tools.complete_feed_batch(&user, initial.id, now).unwrap();

    let later = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(2)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
    assert_eq!(later.state, FeedBatchState::CaughtUp);
    assert!(later.items.is_empty());
}

#[test]
fn typed_source_blocks_control_feedback_state_and_feed_eligibility() {
    let tools = AgentTools::new(seed_store());
    let (_, blocked_id) = accepted_item(&tools, "typed-block-source", 23);
    let (_, allowed_id) = accepted_item(&tools, "typed-block-allowed", 24);
    let user = harness(
        &tools,
        "typed blocking user",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
        None,
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let initial = tools
        .get_feed_batch(&user, FeedBatchRequest::new(2).unwrap(), now)
        .unwrap();
    let mut update = UpdateTasteProfileRequest::default();
    update.blocked_source_affinities = Some(vec![SourceAffinitySignal::Source(
        "SOURCE23.EXAMPLE".into(),
    )]);
    tools.update_taste_profile(&user, update).unwrap();
    let receipt = tools
        .record_feed_feedback(&user, blocked_id, FeedbackKind::Saved, None, None, now)
        .unwrap();
    assert!(receipt.source_blocked);
    tools.complete_feed_batch(&user, initial.id, now).unwrap();

    let later = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(2)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
    assert_eq!(later.items.len(), 1);
    assert_eq!(later.items[0].content_reference.content_item_id, allowed_id);
}

#[test]
fn pod_scoped_feed_filters_items_and_cross_pod_evidence() {
    let tools = AgentTools::new(seed_store());
    let (allowed, content_item_id) = accepted_item(&tools, "scope-allowed", 31);
    let owner = tools.default_auth_context().unwrap();
    let denied = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Denied".into(),
                slug: "scope-denied".into(),
                description: "Outside the Feed grant".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    tools
        .add_content_item_to_pod(
            &owner,
            AddContentItemToPodRequest::new(content_item_id, denied.id, None).unwrap(),
            Utc::now(),
        )
        .unwrap();
    let broad = harness(
        &tools,
        "broad feed",
        vec![HarnessCapability::FeedRead, HarnessCapability::PodCuration],
        None,
    );
    let now = Utc::now();
    let broad_batch = tools
        .get_feed_batch(&broad, FeedBatchRequest::new(5).unwrap(), now)
        .unwrap();
    assert_eq!(broad_batch.items.len(), 1);
    assert_eq!(broad_batch.items[0].placements.len(), 2);
    tools
        .revoke_agent_harness(&owner, broad.harness_id.unwrap())
        .unwrap();
    assert!(tools
        .get_feed_batch(&broad, FeedBatchRequest::new(5).unwrap(), now)
        .is_err());
    let scoped = harness(
        &tools,
        "scoped feed",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
        Some(vec![allowed.id]),
    );

    let batch = tools
        .get_feed_batch(
            &scoped,
            FeedBatchRequest::new(5)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
    assert_eq!(batch.items.len(), 1);
    assert_eq!(batch.items[0].placements.len(), 1);
    assert_eq!(batch.items[0].placements[0].pod_id, allowed.id);
    tools
        .record_feed_feedback(
            &scoped,
            content_item_id,
            FeedbackKind::Saved,
            None,
            None,
            Utc::now(),
        )
        .unwrap();
    let limited = harness(
        &tools,
        "limited feed",
        vec![HarnessCapability::FeedRead],
        Some(vec![allowed.id]),
    );
    let limited_batch = tools
        .get_feed_batch(
            &limited,
            FeedBatchRequest::new(5)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + chrono::Duration::seconds(2),
        )
        .unwrap();
    assert!(limited_batch.items[0].allowed_actions.is_empty());
    let administrator = harness(
        &tools,
        "grant proposer",
        vec![HarnessCapability::Administration],
        None,
    );
    let approver = harness(
        &tools,
        "grant approver",
        vec![HarnessCapability::Approval],
        None,
    );
    let proposal = tools
        .create_pending_proposal(
            &administrator,
            SensitiveChange::ExpandHarnessGrant {
                harness_id: limited.harness_id.unwrap(),
                capabilities: vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
                pod_ids: Some(vec![allowed.id]),
            },
            now,
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    let refreshed = tools
        .get_feed_batch(
            &limited,
            FeedBatchRequest::new(5).unwrap(),
            now + chrono::Duration::seconds(3),
        )
        .unwrap();
    assert_eq!(refreshed.id, limited_batch.id);
    assert_eq!(refreshed.items[0].allowed_actions.len(), 6);
}

#[test]
fn feedback_and_recurrence_change_subsequent_feed_behavior() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (_, first_id) = accepted_item(&tools, "feedback-one", 11);
    let (_, second_id) = accepted_item(&tools, "feedback-two", 12);
    let (_, third_id) = accepted_item(&tools, "feedback-three", 13);
    let user = harness(
        &tools,
        "feed feedback user",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::PodCuration,
        ],
        None,
    );
    let saved_pod = tools
        .create_pod(
            &user,
            CreatePodRequest {
                name: "Saved references".into(),
                slug: "saved-references".into(),
                description: "Explicit Add to Pod target".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let batch = tools
        .get_feed_batch(&user, FeedBatchRequest::new(3).unwrap(), now)
        .unwrap();
    assert_eq!(batch.items.len(), 3);
    tools
        .record_feed_feedback(&user, first_id, FeedbackKind::Saved, None, None, now)
        .unwrap();
    tools
        .record_feed_feedback(&user, first_id, FeedbackKind::Interesting, None, None, now)
        .unwrap();
    tools
        .add_content_item_to_pod(
            &user,
            AddContentItemToPodRequest::new(
                first_id,
                saved_pod.id,
                Some("Saved from the Feed".into()),
            )
            .unwrap(),
            now,
        )
        .unwrap();
    tools
        .record_feed_feedback(
            &user,
            second_id,
            FeedbackKind::NotForMe,
            None,
            Some("Not relevant".into()),
            now,
        )
        .unwrap();
    tools
        .record_feed_feedback(&user, third_id, FeedbackKind::Dismissed, None, None, now)
        .unwrap();
    tools.complete_feed_batch(&user, batch.id, now).unwrap();

    let penalized = tools
        .get_feed_batch(&user, FeedBatchRequest::new(3).unwrap(), now)
        .unwrap();
    assert_eq!(penalized.state, FeedBatchState::Ready);
    assert_eq!(penalized.items.len(), 1);
    assert!(
        penalized.items[0]
            .ranking_evidence
            .recurrence_penalty_applied
    );
    tools.complete_feed_batch(&user, penalized.id, now).unwrap();

    let resurfaced = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(3)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
    assert_eq!(resurfaced.items.len(), 1);
    assert_eq!(resurfaced.items[0].kind, FeedItemKind::OldGem);
    assert_eq!(
        resurfaced.items[0].content_reference.content_item_id,
        first_id
    );
    assert!(resurfaced.items[0].feedback_state.saved);
    assert!(resurfaced.items[0].feedback_state.more_like_this);
    assert!(resurfaced.items[0].placements.len() >= 2);
}

#[test]
fn feed_batch_records_the_default_feed_mix() {
    let tools = AgentTools::new(seed_store());
    accepted_item(&tools, "mix-default", 41);
    let user = harness(
        &tools,
        "mix reader",
        vec![HarnessCapability::FeedRead],
        None,
    );

    let batch = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(10).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
        )
        .unwrap();

    assert_eq!(batch.feed_mix, FeedMix::default());
    assert_eq!(batch.batch_intent, BatchIntent::default());
    assert_eq!(batch.items[0].kind, FeedItemKind::Subscribed);
}

#[test]
fn feed_mix_rejects_invalid_construction_and_transport_values() {
    assert!(FeedMix::new(101, 0, 0, 1, 1).is_err());
    assert!(FeedMix::new(80, 10, 10, 0, 1).is_err());
    assert!(FeedMix::new(80, 20, 10, 1, 1).is_err());
    assert!(
        serde_json::from_value::<FeedBatchRequest>(serde_json::json!({
            "size": 10,
            "feed_mix": {
                "high_value_percent": 80,
                "exploration_percent": 10,
                "old_gem_percent": 10,
                "per_pod_cap": 0,
                "per_source_cap": 2
            }
        }))
        .is_err()
    );
}

#[test]
fn default_feed_mix_blends_subscribed_exploration_and_old_gems() {
    let tools = AgentTools::new(seed_store());
    let old_gem = accepted_item(&tools, "old-gem", 50).1;
    let user = harness(
        &tools,
        "blend reader",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let first_delivery = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();
    let old_batch = tools
        .get_feed_batch(&user, FeedBatchRequest::new(1).unwrap(), first_delivery)
        .unwrap();
    assert_eq!(
        old_batch.items[0].content_reference.content_item_id,
        old_gem
    );
    tools
        .complete_feed_batch(&user, old_batch.id, first_delivery)
        .unwrap();

    for ordinal in 51..=58 {
        accepted_item(&tools, &format!("subscribed-{ordinal}"), ordinal);
    }
    for ordinal in 59..=61 {
        let (pod, _) = accepted_item(&tools, &format!("explore-{ordinal}"), ordinal);
        make_unsubscribed_public(&tools, &pod);
    }

    let batch = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(10).unwrap(),
            first_delivery + chrono::Duration::days(31),
        )
        .unwrap();
    let subscribed = batch
        .items
        .iter()
        .filter(|item| item.kind == FeedItemKind::Subscribed)
        .count();
    let exploration = batch
        .items
        .iter()
        .filter(|item| item.kind == FeedItemKind::Exploration)
        .count();
    let old_gems = batch
        .items
        .iter()
        .filter(|item| item.kind == FeedItemKind::OldGem)
        .count();

    assert!((7..=8).contains(&subscribed));
    assert_eq!(exploration, 1);
    assert_eq!(old_gems, 1);
}

#[test]
fn priority_subscription_is_represented_without_filling_the_batch() {
    let tools = AgentTools::new(seed_store());
    let (priority_pod, priority_item) = accepted_item(&tools, "priority", 70);
    let (_, high_value_one) = accepted_item(&tools, "high-value-one", 71);
    let (_, high_value_two) = accepted_item(&tools, "high-value-two", 72);
    let user = harness(
        &tools,
        "priority reader",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::SubscriptionManagement,
        ],
        None,
    );
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["topic-71".into(), "topic-72".into()]);
    tools.update_taste_profile(&user, taste).unwrap();
    tools
        .set_priority_subscription(&user, priority_pod.id, true)
        .unwrap();

    let batch = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(2).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
        )
        .unwrap();
    let ids = batch
        .items
        .iter()
        .map(|item| item.content_reference.content_item_id)
        .collect::<Vec<_>>();

    assert!(ids.contains(&priority_item));
    assert!(ids.contains(&high_value_one) || ids.contains(&high_value_two));
}

#[test]
fn every_priority_subscription_is_represented_when_it_fits_the_subscribed_target() {
    let tools = AgentTools::new(seed_store());
    let priorities = (73..=75)
        .map(|ordinal| accepted_item(&tools, &format!("priority-{ordinal}"), ordinal))
        .collect::<Vec<_>>();
    let high_value = accepted_item(&tools, "priority-backfill", 76).1;
    let user = harness(
        &tools,
        "multiple priority reader",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::SubscriptionManagement,
        ],
        None,
    );
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["topic-76".into()]);
    tools.update_taste_profile(&user, taste).unwrap();
    for (pod, _) in &priorities {
        tools
            .set_priority_subscription(&user, pod.id, true)
            .unwrap();
    }

    let batch = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(4).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
        )
        .unwrap();
    let ids = batch
        .items
        .iter()
        .map(|item| item.content_reference.content_item_id)
        .collect::<Vec<_>>();

    assert!(priorities.iter().all(|(_, id)| ids.contains(id)));
    assert!(ids.contains(&high_value));
}

#[test]
fn shared_item_represents_both_priority_pods_without_skipping_a_third() {
    let tools = AgentTools::new(seed_store());
    let (priority_a, shared_item) = accepted_item(&tools, "priority-overlap-a", 77);
    let user = harness(
        &tools,
        "overlapping priority reader",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::PodCuration,
            HarnessCapability::SubscriptionManagement,
        ],
        None,
    );
    let priority_b = tools
        .create_pod(
            &user,
            CreatePodRequest {
                name: "Priority overlap B".into(),
                slug: "priority-overlap-b".into(),
                description: "Shares one canonical item with A".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    tools.join_pod(&user, &priority_b.slug).unwrap();
    tools
        .add_content_item_to_pod(
            &user,
            AddContentItemToPodRequest::new(shared_item, priority_b.id, None).unwrap(),
            Utc::now(),
        )
        .unwrap();
    let (priority_c, priority_c_item) = accepted_item(&tools, "priority-overlap-c", 78);
    let high_value = accepted_item(&tools, "priority-overlap-high-value", 79).1;
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["topic-79".into()]);
    tools.update_taste_profile(&user, taste).unwrap();
    for pod in [&priority_a, &priority_b, &priority_c] {
        tools
            .set_priority_subscription(&user, pod.id, true)
            .unwrap();
    }
    let mix = FeedMix::default().with_targets(100, 0, 0).unwrap();

    let batch = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(2).unwrap().with_feed_mix(mix),
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
        )
        .unwrap();
    let ids = batch
        .items
        .iter()
        .map(|item| item.content_reference.content_item_id)
        .collect::<Vec<_>>();

    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&shared_item));
    assert!(ids.contains(&priority_c_item));
    assert!(!ids.contains(&high_value));
}

#[test]
fn partial_feed_mix_overrides_resolve_against_one_set_of_defaults() {
    let overrides = FeedMixOverrides::new(
        Some(FeedPercentage::new(70).unwrap()),
        Some(FeedPercentage::new(20).unwrap()),
        None,
        Some(FeedCap::new(5).unwrap()),
        None,
    );

    let resolved = overrides.resolve(FeedMix::default()).unwrap();

    assert_eq!(resolved.high_value_percent().value(), 70);
    assert_eq!(resolved.exploration_percent().value(), 20);
    assert_eq!(resolved.old_gem_percent().value(), 10);
    assert_eq!(resolved.per_pod_cap().value(), 5);
    assert_eq!(resolved.per_source_cap().value(), 2);
}

#[test]
fn pod_caps_backfill_from_other_subscriptions() {
    let tools = AgentTools::new(seed_store());
    let (dominant_pod, _) = accepted_item(&tools, "dominant", 80);
    for ordinal in 81..=84 {
        accepted_item_in_pod(&tools, &dominant_pod, ordinal);
    }
    for ordinal in 85..=89 {
        accepted_item(&tools, &format!("backfill-{ordinal}"), ordinal);
    }
    let user = harness(
        &tools,
        "cap reader",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let mix = FeedMix::default()
        .with_targets(100, 0, 0)
        .unwrap()
        .with_caps(2, 10)
        .unwrap();

    let batch = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(6).unwrap().with_feed_mix(mix),
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
        )
        .unwrap();
    let dominant_count = batch
        .items
        .iter()
        .filter(|item| {
            item.placements
                .iter()
                .any(|placement| placement.pod_id == dominant_pod.id)
        })
        .count();

    assert_eq!(batch.items.len(), 6);
    assert!(dominant_count <= 2);
}

#[test]
fn source_caps_backfill_from_other_sources() {
    let tools = AgentTools::new(seed_store());
    let shared_source_ids = (90..=92)
        .map(|ordinal| accepted_item(&tools, &format!("shared-{ordinal}"), ordinal).1)
        .collect::<Vec<_>>();
    accepted_item(&tools, "source-backfill-one", 93);
    accepted_item(&tools, "source-backfill-two", 94);
    {
        let shared_store = tools.store();
        let mut store = shared_store.write().unwrap();
        for content_item_id in &shared_source_ids {
            store
                .submissions
                .get_mut(&SubmissionId::from(*content_item_id))
                .unwrap()
                .domain = "shared.example".into();
        }
    }
    let user = harness(
        &tools,
        "source cap reader",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let mix = FeedMix::default()
        .with_targets(100, 0, 0)
        .unwrap()
        .with_caps(10, 2)
        .unwrap();

    let batch = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(4).unwrap().with_feed_mix(mix),
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
        )
        .unwrap();
    let shared_count = batch
        .items
        .iter()
        .filter(|item| item.content_reference.source == "shared.example")
        .count();

    assert_eq!(batch.items.len(), 4);
    assert!(shared_count <= 2);
}

#[test]
fn batch_intent_is_temporary_and_visible_in_explanations() {
    let tools = AgentTools::new(seed_store());
    let focused_id = accepted_item(&tools, "intent-focus", 101).1;
    let avoided_id = accepted_item(&tools, "intent-avoid", 102).1;
    let user = harness(
        &tools,
        "intent reader",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
        None,
    );
    let before = tools.taste_profile(&user).unwrap();
    let intent = BatchIntent::new(vec!["topic-101".into()], vec!["topic-102".into()]);
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();

    let focused = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(2)
                .unwrap()
                .with_batch_intent(intent.clone()),
            now,
        )
        .unwrap();
    assert_eq!(focused.batch_intent, intent);
    assert_eq!(focused.items.len(), 1);
    assert_eq!(
        focused.items[0].content_reference.content_item_id,
        focused_id
    );
    assert!(focused.items[0]
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason.contains("Batch Intent focus")));
    tools.complete_feed_batch(&user, focused.id, now).unwrap();

    let later = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(2)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
    assert_eq!(later.batch_intent, BatchIntent::default());
    assert!(later
        .items
        .iter()
        .any(|item| item.content_reference.content_item_id == avoided_id));
    assert_eq!(tools.taste_profile(&user).unwrap(), before);
}

#[test]
fn matching_batch_intent_can_resurface_a_recent_delivery_as_an_old_gem() {
    let tools = AgentTools::new(seed_store());
    let content_item_id = accepted_item(&tools, "intent-resurface", 103).1;
    let user = harness(
        &tools,
        "intent resurfacing reader",
        vec![HarnessCapability::FeedRead],
        None,
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let first = tools
        .get_feed_batch(&user, FeedBatchRequest::new(1).unwrap(), now)
        .unwrap();
    tools.complete_feed_batch(&user, first.id, now).unwrap();

    let resurfaced = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(1)
                .unwrap()
                .with_batch_intent(BatchIntent::new(vec!["topic-103".into()], Vec::new())),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();

    assert_eq!(resurfaced.items.len(), 1);
    assert_eq!(
        resurfaced.items[0].content_reference.content_item_id,
        content_item_id
    );
    assert_eq!(resurfaced.items[0].kind, FeedItemKind::OldGem);
    assert!(resurfaced.items[0]
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason.contains("Batch Intent focus")));
}

#[test]
fn exploration_is_labeled_without_creating_a_subscription() {
    let tools = AgentTools::new(seed_store());
    let (pod, exploration_id) = accepted_item(&tools, "labeled-exploration", 110);
    make_unsubscribed_public(&tools, &pod);
    let user = harness(
        &tools,
        "exploration reader",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::SubscriptionManagement,
        ],
        None,
    );

    let batch = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(10).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
        )
        .unwrap();
    let repeated = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(10).unwrap(),
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 1).unwrap(),
        )
        .unwrap();

    assert_eq!(batch.items.len(), 1);
    assert_eq!(repeated, batch);
    assert_eq!(
        batch.items[0].content_reference.content_item_id,
        exploration_id
    );
    assert!(batch.items[0].is_exploration);
    assert_eq!(batch.items[0].kind, FeedItemKind::Exploration);
    assert!(tools
        .set_priority_subscription(&user, pod.id, true)
        .is_err());
}
