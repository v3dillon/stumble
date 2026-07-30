use chrono::{TimeZone, Utc};
use stumble_core::*;

use super::common::{accepted_item, accepted_item_in_pod, harness, make_unsubscribed_public};

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
