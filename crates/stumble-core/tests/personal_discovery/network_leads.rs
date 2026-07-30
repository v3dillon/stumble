use chrono::{TimeZone, Utc};
use stumble_core::*;

use crate::common::*;

#[test]
fn local_public_content_references_produce_adjacent_network_leads() {
    let home_dir = TestDataDir::new("local-public-content-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    let local_pod = create_public_pod(
        &home,
        "local-systems",
        "Local distributed systems reading list",
    );
    accept_public_item(
        &home,
        &local_pod,
        "local-public-ref",
        "https://local-public.example/deep-dive",
        vec!["systems".into()],
    );
    home.index_pod_announcement(
        home.pod_announcement(
            &home.default_auth_context().unwrap(),
            &local_pod.slug,
            "https://home.example/federation/pods/local-systems",
        )
        .unwrap(),
    )
    .unwrap();

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "local-public-content".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap()
        .plan;

    assert!(plan.source_neighborhoods.iter().any(|source| {
        source.signal == SourceAffinitySignal::Source("local-public.example".into())
            && source.role == DiscoveryPlanSourceRole::Adjacent
            && source.rationale.contains("local public Content Reference")
    }));
    assert!(plan.source_neighborhoods.iter().any(|source| {
        matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "local-systems"
        ) && source.role == DiscoveryPlanSourceRole::Adjacent
            && source.rationale.contains("Pod Announcement")
    }));
}

#[test]
fn verified_network_metadata_produces_adjacent_discovery_leads_with_provenance() {
    let home_dir = TestDataDir::new("network-leads-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    let subscription_count_before = home.store().read().unwrap().subscriptions.len();
    let (_announcement, samples) = import_verified_network_metadata(&home);

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "network-leads".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap()
        .plan;

    let adjacent = plan
        .source_neighborhoods
        .iter()
        .filter(|source| source.role == DiscoveryPlanSourceRole::Adjacent)
        .collect::<Vec<_>>();
    assert!(!adjacent.is_empty());
    assert!(adjacent.iter().all(|source| {
        source.rationale.contains("verified public")
            || source.rationale.contains("local public Content Reference")
    }));
    assert!(adjacent.iter().any(|source| {
        source.signal == SourceAffinitySignal::Source("allowed.example".into())
            && source.rationale.contains("Explore sample")
    }));
    assert!(adjacent.iter().any(|source| {
        matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "rust-systems"
        ) && (source.rationale.contains("Pod Announcement")
            || source.rationale.contains("Pod Endorsement"))
    }));
    assert!(plan
        .source_neighborhoods
        .iter()
        .filter(|source| source.role == DiscoveryPlanSourceRole::Adjacent)
        .all(|source| !source.temporary));
    assert_eq!(plan.allocation.adjacent, 3);
    assert_eq!(
        home.store().read().unwrap().subscriptions.len(),
        subscription_count_before
    );
    // Explore samples retained locally still include blocked sources; the plan must not.
    assert!(samples
        .samples
        .iter()
        .any(|sample| sample.source == "blocked.example"));
    assert!(plan
        .source_neighborhoods
        .iter()
        .all(|source| { source.signal != SourceAffinitySignal::Source("blocked.example".into()) }));
}

#[test]
fn invalid_stale_blocked_untrusted_or_withdrawn_metadata_cannot_influence_plans() {
    let home_dir = TestDataDir::new("filtered-network-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    let (announcement, samples) = import_verified_network_metadata(&home);

    // Block the remote source domain through Trust Policy after import.
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockSource {
            source: "allowed.example".into(),
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockPod {
            origin_node_id: announcement.origin_node_id,
            pod_slug: announcement.pod_slug.clone(),
        },
    );

    let blocked_plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "blocked-network".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    assert!(blocked_plan.source_neighborhoods.iter().all(|source| {
        source.role != DiscoveryPlanSourceRole::Adjacent
            || (!matches!(
                &source.signal,
                SourceAffinitySignal::Source(domain) if domain == "allowed.example"
            ) && !matches!(
                &source.signal,
                SourceAffinitySignal::Community(slug) if slug == "rust-systems"
            ))
    }));

    // Remove Index: announcements retained only via that Index must drop out of new plans.
    let fresh_dir = TestDataDir::new("index-removed-home");
    let index_dir = TestDataDir::new("index-removed-index");
    let fresh = AgentTools::open_home_node(&fresh_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let fresh_manager = personal_manager(&fresh);
    set_interest(&fresh, &fresh_manager, "distributed systems");
    index.index_pod_announcement(announcement.clone()).unwrap();
    let search = index.search_pod_announcements("systems", 10).unwrap();
    approve_trust_policy_change(
        &fresh,
        TrustPolicyChange::AddIndexNode {
            label: "ephemeral index".into(),
            base_url: "https://ephemeral-index.example".into(),
        },
    );
    let reader = admin_harness(
        &fresh,
        "ephemeral reader",
        vec![HarnessCapability::FeedRead],
    );
    fresh
        .accept_index_search_results(&reader, "https://ephemeral-index.example", search)
        .unwrap();
    // Stale/mismatched samples (wrong announcement binding) must not produce leads.
    let mut stale_samples = samples;
    stale_samples.announcement_id = uuid::Uuid::now_v7();
    assert!(fresh
        .accept_pod_explore_samples(&reader, stale_samples)
        .is_err());
    approve_trust_policy_change(
        &fresh,
        TrustPolicyChange::RemoveIndexNode {
            base_url: "https://ephemeral-index.example".into(),
        },
    );
    let after_index_removal = fresh
        .request_personal_discovery(
            &fresh_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "index-removed".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    assert!(after_index_removal
        .source_neighborhoods
        .iter()
        .all(|source| source.role != DiscoveryPlanSourceRole::Adjacent
            || !matches!(
                &source.signal,
                SourceAffinitySignal::Community(slug) if slug == "rust-systems"
            )));

    // Tampered (invalid signature) announcement cannot be retained.
    let mut tampered = announcement;
    tampered.subject = "attacker changed subject to systems".into();
    assert!(fresh.index_pod_announcement(tampered).is_err());

    // Withdrawn local public Accepted Placements cannot produce Content Reference leads.
    let local_withdraw_dir = TestDataDir::new("withdrawn-local-public-home");
    let local_home = AgentTools::open_home_node(&local_withdraw_dir.0, seed_store).unwrap();
    let local_manager = personal_manager(&local_home);
    set_interest(&local_home, &local_manager, "distributed systems");
    let local_pod = create_public_pod(
        &local_home,
        "withdrawn-local-systems",
        "Local distributed systems reading list",
    );
    accept_public_item(
        &local_home,
        &local_pod,
        "withdrawable-public-ref",
        "https://withdrawn-local.example/deep-dive",
        vec!["systems".into()],
    );
    local_home
        .index_pod_announcement(
            local_home
                .pod_announcement(
                    &local_home.default_auth_context().unwrap(),
                    &local_pod.slug,
                    "https://home.example/federation/pods/withdrawn-local-systems",
                )
                .unwrap(),
        )
        .unwrap();
    let before_withdrawal = local_home
        .request_personal_discovery(
            &local_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "before-local-withdrawal".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    assert!(
        before_withdrawal.source_neighborhoods.iter().any(|source| {
            matches!(
                &source.signal,
                SourceAffinitySignal::Source(domain) if domain == "withdrawn-local.example"
            ) && source.role == DiscoveryPlanSourceRole::Adjacent
        }),
        "local public Content Reference must produce a lead before withdrawal"
    );
    let content_item_id = {
        let store = local_home.store();
        let store = store.read().unwrap();
        store
            .accepted_placement_projections
            .keys()
            .find(|(_, pod_id)| *pod_id == local_pod.id)
            .map(|(content_item_id, _)| *content_item_id)
            .expect("local accepted placement exists")
    };
    let proposer = admin_harness(
        &local_home,
        "local withdrawal proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = admin_harness(
        &local_home,
        "local withdrawal approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let outcome = local_home
        .request_remove_submission_from_pod(&proposer, &local_pod.slug, content_item_id.into(), now)
        .unwrap();
    let RemoveSubmissionOutcome::PendingApproval(proposal) = outcome else {
        panic!("public placement withdrawal must require approval");
    };
    local_home
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    let after_local_withdrawal = local_home
        .request_personal_discovery(
            &local_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "withdrawn-local-public".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap()
        .plan;
    assert!(
        after_local_withdrawal
            .source_neighborhoods
            .iter()
            .all(|source| {
                !matches!(
                    &source.signal,
                    SourceAffinitySignal::Source(domain) if domain == "withdrawn-local.example"
                )
            }),
        "withdrawn local public Content Reference must not produce a network lead"
    );
}

#[test]
fn local_relevance_discards_remote_index_scores_and_autonomous_planning_is_local_only() {
    let home_dir = TestDataDir::new("local-relevance-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    let (announcement, _) = import_verified_network_metadata(&home);

    // Import a second announcement that would only match a private profile term if
    // we wrongly issued a remote query; it stays unmatched because subjects differ.
    let unrelated_dir = TestDataDir::new("unrelated-origin");
    let index_dir = TestDataDir::new("unrelated-index");
    let unrelated = AgentTools::open_home_node(&unrelated_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let cooking = create_public_pod(
        &unrelated,
        "cooking-notes",
        "Weeknight pasta and baking techniques",
    );
    let cooking_announcement = unrelated
        .pod_announcement(
            &unrelated.default_auth_context().unwrap(),
            &cooking.slug,
            "https://unrelated.example/federation/pods/cooking-notes",
        )
        .unwrap();
    index
        .index_pod_announcement(cooking_announcement.clone())
        .unwrap();
    // Re-index the systems pod with a low remote score path; accept both via Index.
    index.index_pod_announcement(announcement.clone()).unwrap();
    let search = index.search_pod_announcements("", 10).unwrap();
    let reader = admin_harness(
        &home,
        "local relevance reader",
        vec![HarnessCapability::FeedRead],
    );
    home.accept_index_search_results(&reader, "https://network-index.example", search)
        .unwrap();

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "local-relevance".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap()
        .plan;

    assert!(plan.source_neighborhoods.iter().any(|source| {
        matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "rust-systems"
        ) && source.role == DiscoveryPlanSourceRole::Adjacent
    }));
    assert!(plan.source_neighborhoods.iter().all(|source| {
        !matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "cooking-notes"
        )
    }));
    // Plan rationales stay aggregate and never leak private matching inputs.
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("matching_topics"));
    assert!(!serialized.contains("InterestSeed"));
    assert!(!serialized.contains("DiscoveryLead"));
    assert!(!serialized.contains("local_relevance"));
    assert!(!serialized.contains("TasteProfile"));
    for source in &plan.source_neighborhoods {
        if source.role == DiscoveryPlanSourceRole::Adjacent {
            assert!(
                source.rationale.starts_with("adjacent exploration from"),
                "unexpected rationale {}",
                source.rationale
            );
            assert!(!source.rationale.contains("distributed systems"));
        }
    }
}

#[test]
fn explicit_explore_query_remains_distinct_from_autonomous_personal_discovery() {
    let home_dir = TestDataDir::new("explore-distinct-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    import_verified_network_metadata(&home);

    let explored = home
        .explore_public_pods(
            &manager,
            ExploreRequest::new("rust ownership", 10, 5).unwrap(),
        )
        .unwrap();
    assert_eq!(explored.query, "rust ownership");
    assert!(!explored.results.is_empty());

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "explore-distinct".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    // Autonomous plan does not embed the explicit Explore query.
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("rust ownership"));
    assert_ne!(
        serde_json::to_value(&plan).unwrap()["intent"],
        serde_json::json!({"kind":"topic","value":"rust ownership"})
    );
}

#[test]
fn network_lead_selection_does_not_create_subscription_import_state_or_browser_authority() {
    let home_dir = TestDataDir::new("no-side-effects-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    import_verified_network_metadata(&home);
    let store_guard = home.store();
    let before = {
        let store = store_guard.read().unwrap();
        (
            store.subscriptions.len(),
            store.accepted_placement_projections.len(),
            store.agent_harnesses.len(),
            store.pods.len(),
        )
    };
    drop(store_guard);

    let created = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "no-side-effects".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert!(created
        .plan
        .source_neighborhoods
        .iter()
        .any(|source| source.role == DiscoveryPlanSourceRole::Adjacent));

    let store_guard = home.store();
    let after = {
        let store = store_guard.read().unwrap();
        (
            store.subscriptions.len(),
            store.accepted_placement_projections.len(),
            store.agent_harnesses.len(),
            store.pods.len(),
        )
    };
    assert_eq!(before.0, after.0);
    assert_eq!(before.1, after.1);
    assert_eq!(before.2, after.2);
    assert_eq!(before.3, after.3);
    assert!(created.task.target.pod().is_none());
}

#[test]
fn network_leads_fill_only_adjacent_allocation_unless_user_evidence_corroborates() {
    let home_dir = TestDataDir::new("adjacent-only-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    import_verified_network_metadata(&home);

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: Some(10),
                idempotency_key: "adjacent-only".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    assert_eq!(plan.allocation.proven, 7);
    assert_eq!(plan.allocation.adjacent, 3);
    for source in &plan.source_neighborhoods {
        let is_network = source.rationale.contains("adjacent exploration from");
        if is_network {
            assert_eq!(source.role, DiscoveryPlanSourceRole::Adjacent);
        }
    }
    assert!(plan
        .source_neighborhoods
        .iter()
        .filter(|source| source.role == DiscoveryPlanSourceRole::Proven)
        .all(|source| !source.rationale.contains("adjacent exploration from")));
}

#[test]
fn restart_trust_and_equivalent_metadata_replacement_are_deterministic() {
    let adjacent_signals = |plan: &DiscoveryPlan| {
        plan.source_neighborhoods
            .iter()
            .filter(|source| source.role == DiscoveryPlanSourceRole::Adjacent)
            .map(|source| source.signal.clone())
            .collect::<Vec<_>>()
    };
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 15, 0, 0).unwrap();

    // Restart + idempotent retry preserve the full request and adjacent set.
    let home_dir = TestDataDir::new("deterministic-network-home");
    let home = AgentTools::initialize_home_node(&home_dir.0, seed_store).unwrap();
    let owner = home.local_owner_auth_context().unwrap();
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["distributed systems".into()]);
    home.update_taste_profile(&owner, taste).unwrap();
    let (announcement, _samples) = import_verified_network_metadata(&home);
    let first = home
        .request_personal_discovery(
            &owner,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "deterministic-network".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    let first_adjacent = adjacent_signals(&first.plan);
    assert!(
        !first_adjacent.is_empty(),
        "seeded network metadata must produce adjacent leads before restart"
    );
    drop(home);

    let restarted = AgentTools::open_initialized_home_node(&home_dir.0).unwrap();
    let owner = restarted.local_owner_auth_context().unwrap();
    let retried = restarted
        .request_personal_discovery(
            &owner,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "deterministic-network".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    assert_eq!(retried, first);
    assert_eq!(adjacent_signals(&retried.plan), first_adjacent);

    // Replacement of equivalent verified metadata: two independent Home Nodes that import
    // the same verified announcement/sample/endorsement set recompute the same adjacent signals.
    let equivalent_a_dir = TestDataDir::new("equivalent-meta-a");
    let equivalent_b_dir = TestDataDir::new("equivalent-meta-b");
    let equivalent_a = AgentTools::open_home_node(&equivalent_a_dir.0, seed_store).unwrap();
    let equivalent_b = AgentTools::open_home_node(&equivalent_b_dir.0, seed_store).unwrap();
    let manager_a = personal_manager(&equivalent_a);
    let manager_b = personal_manager(&equivalent_b);
    set_interest(&equivalent_a, &manager_a, "distributed systems");
    set_interest(&equivalent_b, &manager_b, "distributed systems");
    import_verified_network_metadata(&equivalent_a);
    import_verified_network_metadata(&equivalent_b);
    let plan_a = equivalent_a
        .request_personal_discovery(
            &manager_a,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "equivalent-a".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap()
        .plan;
    let plan_b = equivalent_b
        .request_personal_discovery(
            &manager_b,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "equivalent-b".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap()
        .plan;
    assert_eq!(
        adjacent_signals(&plan_a),
        adjacent_signals(&plan_b),
        "equivalent verified metadata must recompute the same adjacent signals"
    );
    assert!(
        !adjacent_signals(&plan_a).is_empty(),
        "equivalent import must still produce network adjacent leads"
    );

    // After a Trust Policy mutation, two independent recomputes agree.
    approve_trust_policy_change(
        &equivalent_a,
        TrustPolicyChange::BlockSource {
            source: "allowed.example".into(),
        },
    );
    let after_trust_a = equivalent_a
        .request_personal_discovery(
            &manager_a,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "deterministic-trust-a".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap()
        .plan;
    let after_trust_b = equivalent_a
        .request_personal_discovery(
            &manager_a,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "deterministic-trust-b".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap()
        .plan;
    assert_eq!(
        adjacent_signals(&after_trust_a),
        adjacent_signals(&after_trust_b),
        "trust-policy recomputes must be deterministic"
    );
    assert!(
        after_trust_a.source_neighborhoods.iter().all(|source| {
            source.role != DiscoveryPlanSourceRole::Adjacent
                || !matches!(
                    &source.signal,
                    SourceAffinitySignal::Source(domain) if domain == "allowed.example"
                )
        }),
        "blocked source must stay excluded after trust change"
    );

    // Index removal: Index-only retained communities drop, and two recomputes agree.
    let index_dir = TestDataDir::new("deterministic-index-only");
    let index_only_home_dir = TestDataDir::new("deterministic-index-removal-home");
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let index_home = AgentTools::open_home_node(&index_only_home_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let index_manager = personal_manager(&index_home);
    set_interest(&index_home, &index_manager, "distributed systems");
    index.index_pod_announcement(announcement.clone()).unwrap();
    let search = index.search_pod_announcements("systems", 10).unwrap();
    approve_trust_policy_change(
        &index_home,
        TrustPolicyChange::AddIndexNode {
            label: "replaceable index".into(),
            base_url: "https://replaceable-index.example".into(),
        },
    );
    let index_reader = admin_harness(
        &index_home,
        "index removal reader",
        vec![HarnessCapability::FeedRead],
    );
    index_home
        .accept_index_search_results(&index_reader, "https://replaceable-index.example", search)
        .unwrap();
    let with_index = index_home
        .request_personal_discovery(
            &index_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "with-index".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap()
        .plan;
    assert!(with_index.source_neighborhoods.iter().any(|source| {
        matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "rust-systems"
        ) && source.role == DiscoveryPlanSourceRole::Adjacent
    }));
    approve_trust_policy_change(
        &index_home,
        TrustPolicyChange::RemoveIndexNode {
            base_url: "https://replaceable-index.example".into(),
        },
    );
    let without_index_a = index_home
        .request_personal_discovery(
            &index_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "without-index-a".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap()
        .plan;
    let without_index_b = index_home
        .request_personal_discovery(
            &index_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "without-index-b".into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap()
        .plan;
    assert_eq!(
        adjacent_signals(&without_index_a),
        adjacent_signals(&without_index_b)
    );
    assert!(without_index_a.source_neighborhoods.iter().all(|source| {
        source.role != DiscoveryPlanSourceRole::Adjacent
            || !matches!(
                &source.signal,
                SourceAffinitySignal::Community(slug) if slug == "rust-systems"
            )
    }));
}

#[test]
fn private_discovery_lead_types_remain_absent_from_federation_and_outbound_artifacts() {
    let home_dir = TestDataDir::new("privacy-network-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "secret-private-topic");
    // Also plant a network lead that would match systems, not the secret topic.
    import_verified_network_metadata(&home);
    home.request_personal_discovery(
        &manager,
        RequestPersonalDiscovery {
            intent: Some(PersonalDiscoveryIntent::Topic(
                "secret-private-topic".into(),
            )),
            result_count: None,
            idempotency_key: "privacy-network".into(),
            browser_grant_eligible_sources: None,
        },
        Utc::now(),
    )
    .unwrap();

    let federation = home.default_auth_context().unwrap();
    let public_pods = home.list_public_pods(&federation).unwrap();
    let manifests = public_pods
        .iter()
        .map(|pod| {
            home.federation_pod_manifest(&federation, &pod.slug)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let events = public_pods
        .iter()
        .map(|pod| home.federation_pod_events(&federation, &pod.slug).unwrap())
        .collect::<Vec<_>>();
    let packages = public_pods
        .iter()
        .map(|pod| home.export_skill_pack(&federation, &pod.slug).unwrap())
        .collect::<Vec<_>>();
    let announcements = home
        .store()
        .read()
        .unwrap()
        .known_pod_announcements
        .values()
        .map(|known| known.announcement.clone())
        .collect::<Vec<_>>();
    let samples = home
        .store()
        .read()
        .unwrap()
        .pod_explore_sample_sets
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let endorsements = home
        .store()
        .read()
        .unwrap()
        .pod_endorsements
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let index_search = home.search_pod_announcements("systems", 10).unwrap();
    let explored = home
        .explore_public_pods(&manager, ExploreRequest::new("systems", 10, 5).unwrap())
        .unwrap();

    let outbound = serde_json::to_string(&serde_json::json!({
        "node": home.node_info(&federation).unwrap(),
        "pods": public_pods,
        "manifests": manifests,
        "events": events,
        "packages": packages,
        "announcements": announcements,
        "samples": samples,
        "endorsements": endorsements,
        "index_search": index_search,
        "explore": explored,
    }))
    .unwrap();
    for forbidden in [
        "secret-private-topic",
        "DiscoveryLead",
        "discovery_lead",
        "InterestSeed",
        "interest_seed",
        "TasteProfile",
        "matching_topics",
        "SourceAffinity",
        "local_relevance",
        "DiscoveryPlan",
        "discovery_plan",
        "PersonalDiscovery",
    ] {
        assert!(
            !outbound.contains(forbidden),
            "outbound artifact leaked private marker {forbidden}"
        );
    }
}
