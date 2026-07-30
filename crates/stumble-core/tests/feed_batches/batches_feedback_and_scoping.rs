use chrono::{TimeZone, Utc};
use stumble_core::*;

use super::common::{accepted_item, harness, TestDataDir};

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
