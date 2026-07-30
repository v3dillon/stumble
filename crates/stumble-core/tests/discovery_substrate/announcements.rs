use crate::common::*;
use chrono::Utc;
use stumble_core::*;

#[test]
fn trusted_peer_lookup_enforces_authorization_tenant_and_enabled_state() {
    let tools = AgentTools::new(seed_store());
    let admin = harness(
        &tools,
        "peer reader",
        vec![HarnessCapability::Administration],
    );
    let unauthorized = harness(&tools, "feed reader", vec![HarnessCapability::FeedRead]);
    let peer = tools
        .trusted_peers(&admin)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

    assert_eq!(tools.trusted_peer(&admin, peer.id).unwrap().id, peer.id);
    assert!(matches!(
        tools.trusted_peer(&unauthorized, peer.id),
        Err(AgentToolsError::Forbidden { .. })
    ));

    {
        let store = tools.store();
        let mut store = store.write().unwrap();
        store.trusted_peers.get_mut(&peer.id).unwrap().enabled = false;
    }
    assert!(matches!(
        tools.trusted_peer(&admin, peer.id),
        Err(AgentToolsError::Store(StoreError::UntrustedPeer))
    ));

    {
        let store = tools.store();
        let mut store = store.write().unwrap();
        let peer = store.trusted_peers.get_mut(&peer.id).unwrap();
        peer.enabled = true;
        peer.tenant_id = Some(uuid::Uuid::now_v7());
    }
    assert!(matches!(
        tools.trusted_peer(&admin, peer.id),
        Err(AgentToolsError::Store(StoreError::UntrustedPeer))
    ));
}

#[test]
fn public_origin_produces_a_compact_verifiable_pod_announcement() {
    let origin_dir = TestDataDir::new("announcement-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "distributed-systems",
        "Carefully curated distributed-systems references",
    );

    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/distributed-systems",
        )
        .unwrap();

    assert_eq!(announcement.pod_slug, "distributed-systems");
    assert_eq!(announcement.origin_node_id, announcement.signer.node_id);
    assert_eq!(
        announcement.package_version,
        PackageVersion::new(1).unwrap()
    );
    assert!(announcement.verify().unwrap());
    let wire = serde_json::to_value(&announcement).unwrap();
    assert!(wire.get("signature").is_some());
    assert!(wire.get("content_items").is_none());
    assert!(wire.get("events").is_none());
    assert!(wire.get("package").is_none());

    let index_dir = TestDataDir::new("tampered-announcement-index");
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let mut tampered = announcement;
    tampered.subject = "An attacker changed the signed subject".into();
    assert!(index.index_pod_announcement(tampered).is_err());
}

#[test]
fn trusted_peers_relay_origin_signed_announcements_without_gaining_authority() {
    let origin_dir = TestDataDir::new("relay-origin");
    let relay_dir = TestDataDir::new("relay");
    let home_dir = TestDataDir::new("relay-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let relay = AgentTools::open_home_node(&relay_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "relay-safe", "Origin-signed systems research");
    let origin_info = origin
        .node_info(&origin.default_auth_context().unwrap())
        .unwrap();
    let relay_info = relay
        .node_info(&relay.default_auth_context().unwrap())
        .unwrap();
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/relay-safe",
        )
        .unwrap();
    let relay_peer = trust_peer(&relay, &origin_info, "https://origin.example");
    let relay_admin = harness(
        &relay,
        "relay receiver",
        vec![HarnessCapability::Administration],
    );
    relay
        .receive_pod_announcement(&relay_admin, relay_peer.id, announcement.clone())
        .unwrap();
    // Peer delivery reuses Origin-signed bytes from the relay's retained store;
    // the relay never re-signs or becomes the Origin.
    let relayed_announcement = {
        let store = relay.store();
        let store = store.read().unwrap();
        store
            .known_pod_announcements
            .values()
            .find(|known| known.announcement.pod_slug == announcement.pod_slug)
            .map(|known| known.announcement.clone())
            .expect("relay retained Origin announcement")
    };

    let home_peer = trust_peer(&home, &relay_info, "https://relay.example");
    let home_admin = harness(
        &home,
        "home receiver",
        vec![HarnessCapability::Administration],
    );
    let relayed = home
        .receive_pod_announcement(&home_admin, home_peer.id, relayed_announcement)
        .unwrap();

    assert_eq!(relayed.announcement.origin_node_id, origin_info.node_id);
    assert_ne!(relayed.announcement.origin_node_id, relay_info.node_id);
    assert_eq!(relayed.received_from_peer_id, Some(home_peer.id));
    assert!(relayed.announcement.verify().unwrap());

    let trust_approver = harness(
        &home,
        "peer revocation approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let proposal = home
        .request_remove_trusted_peer(&home_admin, home_peer.id, now)
        .unwrap();
    home.approve_pending_proposal(&trust_approver, proposal.id, now)
        .unwrap();
    assert!(home
        .trusted_peers(&home_admin)
        .unwrap()
        .iter()
        .all(|peer| peer.id != home_peer.id));
    assert!(home
        .receive_pod_announcement(&home_admin, home_peer.id, relayed.announcement)
        .is_err());
}

#[test]
fn replaceable_index_nodes_aggregate_and_search_signed_announcements() {
    let origin_dir = TestDataDir::new("index-origin");
    let first_index_dir = TestDataDir::new("first-index");
    let replacement_index_dir = TestDataDir::new("replacement-index");
    let home_dir = TestDataDir::new("index-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let first_index = AgentTools::open_home_node(&first_index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let replacement_index = AgentTools::open_home_node(&replacement_index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "rust-systems",
        "Rust ownership and distributed systems",
    );
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/rust-systems",
        )
        .unwrap();

    first_index
        .index_pod_announcement(announcement.clone())
        .unwrap();
    replacement_index
        .index_pod_announcement(announcement)
        .unwrap();
    let first = first_index.search_pod_announcements("rust", 10).unwrap();
    let replacement = replacement_index
        .search_pod_announcements("rust", 10)
        .unwrap();

    assert_eq!(first.results.len(), 1);
    assert_eq!(replacement.results.len(), 1);
    assert_eq!(
        first.results[0].announcement.origin_node_id,
        replacement.results[0].announcement.origin_node_id
    );
    assert!(first.results[0].announcement.verify().unwrap());
    let wire = serde_json::to_value(&first).unwrap();
    assert!(wire.get("global_quality_score").is_none());
    assert!(wire.get("authority").is_none());

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "first index".into(),
            base_url: "https://first-index.example".into(),
        },
    );
    let reader = harness(
        &home,
        "Index Explore reader",
        vec![HarnessCapability::FeedRead],
    );
    home.accept_index_search_results(&reader, "https://first-index.example", first)
        .unwrap();
    assert_eq!(
        home.explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
            .unwrap()
            .results
            .len(),
        1
    );

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::RemoveIndexNode {
            base_url: "https://first-index.example".into(),
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "replacement index".into(),
            base_url: "https://replacement-index.example".into(),
        },
    );
    assert!(home
        .explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
        .unwrap()
        .results
        .is_empty());
    home.accept_index_search_results(&reader, "https://replacement-index.example", replacement)
        .unwrap();
    assert_eq!(
        home.explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
            .unwrap()
            .results
            .len(),
        1
    );
}

#[test]
fn user_configures_index_nodes_and_local_discovery_blocks_through_trust_policy() {
    let home_dir = TestDataDir::new("trust-policy-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let blocked_node_id = uuid::Uuid::now_v7();
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "replaceable index".into(),
            base_url: "https://index.example".into(),
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockPod {
            origin_node_id: blocked_node_id,
            pod_slug: "blocked-pod".into(),
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockNode {
            node_id: blocked_node_id,
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockSource {
            source: "blocked.example".into(),
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockTopic {
            topic: "rage bait".into(),
        },
    );
    let reader = harness(&home, "Explore reader", vec![HarnessCapability::FeedRead]);

    let policy = home.trust_policy(&reader).unwrap();

    assert_eq!(policy.index_nodes.len(), 1);
    assert_eq!(policy.index_nodes[0].base_url, "https://index.example");
    assert!(policy.blocked_nodes.contains(&blocked_node_id));
    assert!(policy
        .blocked_pods
        .contains(&BlockedPod::new(blocked_node_id, "blocked-pod")));
    assert!(policy.blocked_sources.contains("blocked.example"));
    assert!(policy.blocked_topics.contains("rage bait"));
}

#[test]
fn signed_pod_endorsements_remain_optional_origin_evidence() {
    let origin_dir = TestDataDir::new("endorsement-origin");
    let index_dir = TestDataDir::new("endorsement-index");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let endorsing = create_public_pod(
        &origin,
        "systems-curators",
        "Curators of thoughtful systems work",
    );
    let target = create_public_pod(
        &origin,
        "rust-research",
        "Research about reliable Rust systems",
    );
    let target_announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &target.slug,
            "https://origin.example/federation/pods/rust-research",
        )
        .unwrap();
    let endorsing_announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &endorsing.slug,
            "https://origin.example/federation/pods/systems-curators",
        )
        .unwrap();
    index
        .index_pod_announcement(endorsing_announcement.clone())
        .unwrap();
    index
        .index_pod_announcement(target_announcement.clone())
        .unwrap();
    let curator = harness(
        &origin,
        "Pod endorsement curator",
        vec![HarnessCapability::PodCuration],
    );

    let endorsement = origin
        .endorse_public_pod(
            &curator,
            &endorsing_announcement,
            &target_announcement,
            "Consistently careful primary-source curation".into(),
        )
        .unwrap();
    let indexed = index.index_pod_endorsement(endorsement).unwrap();

    assert!(indexed.verify().unwrap());
    assert_eq!(indexed.endorsing_pod_slug, "systems-curators");
    assert_eq!(indexed.endorsed_pod_slug, "rust-research");
    let wire = serde_json::to_value(indexed).unwrap();
    assert!(wire.get("global_reputation").is_none());
    assert!(wire.get("quality_score").is_none());

    let refreshed_target = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &target.slug,
            "https://origin.example/federation/pods/rust-research",
        )
        .unwrap();
    index.index_pod_announcement(refreshed_target).unwrap();
    let reader = harness(
        &index,
        "stale endorsement reader",
        vec![HarnessCapability::FeedRead],
    );
    assert!(index
        .explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
        .unwrap()
        .results[0]
        .endorsements
        .is_empty());
}
