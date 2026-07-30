use crate::common::*;
use stumble_core::*;

#[test]
fn remote_unsubscribed_explore_uses_origin_signed_policy_filtered_samples() {
    let origin_dir = TestDataDir::new("remote-sample-origin");
    let index_dir = TestDataDir::new("remote-sample-index");
    let home_dir = TestDataDir::new("remote-sample-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "remote-explore-systems",
        "Remote distributed systems research",
    );
    accept_item(
        &origin,
        &pod,
        "remote-allowed",
        "https://allowed.example/remote-research",
        vec!["systems".into()],
    );
    accept_item(
        &origin,
        &pod,
        "remote-blocked-source",
        "https://blocked.example/remote-noise",
        vec!["systems".into()],
    );
    accept_item(
        &origin,
        &pod,
        "remote-blocked-topic",
        "https://allowed.example/remote-rage",
        vec!["rage bait".into()],
    );
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://remote-origin.example/federation/pods/remote-explore-systems",
        )
        .unwrap();
    let samples = origin
        .pod_explore_samples(&origin.default_auth_context().unwrap(), &announcement, 10)
        .unwrap();
    index.index_pod_announcement(announcement.clone()).unwrap();
    let index_results = index.search_pod_announcements("systems", 10).unwrap();
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "sample index".into(),
            base_url: "https://sample-index.example".into(),
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
    let reader = harness(
        &home,
        "remote sample Explore reader",
        vec![HarnessCapability::FeedRead],
    );
    home.accept_index_search_results(&reader, "https://sample-index.example", index_results)
        .unwrap();
    home.accept_pod_explore_samples(&reader, samples).unwrap();

    let explored = home
        .explore_public_pods(&reader, ExploreRequest::new("systems", 10, 10).unwrap())
        .unwrap();

    assert_eq!(explored.results.len(), 1);
    assert!(!explored.results[0].is_subscribed);
    assert_eq!(explored.results[0].sample_content_references.len(), 1);
    assert_eq!(
        explored.results[0].sample_content_references[0].canonical_url,
        "https://allowed.example/remote-research"
    );
}

#[test]
fn explore_returns_unsubscribed_public_pods_and_policy_filtered_content_samples() {
    let home_dir = TestDataDir::new("explore-home");
    let blocked_node_dir = TestDataDir::new("explore-blocked-node");
    let blocked_pod_dir = TestDataDir::new("explore-blocked-pod");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let blocked_node = AgentTools::open_home_node(&blocked_node_dir.0, seed_store).unwrap();
    let blocked_pod_origin = AgentTools::open_home_node(&blocked_pod_dir.0, seed_store).unwrap();
    let visible = create_public_pod(
        &home,
        "visible-systems",
        "Careful distributed systems research",
    );
    let endorser = create_public_pod(&home, "trusted-curators", "Systems curators");
    accept_item(
        &home,
        &visible,
        "allowed",
        "https://allowed.example/research",
        vec!["systems".into()],
    );
    accept_item(
        &home,
        &visible,
        "blocked-source",
        "https://blocked.example/noise",
        vec!["systems".into()],
    );
    accept_item(
        &home,
        &visible,
        "blocked-topic",
        "https://allowed.example/rage",
        vec!["rage bait".into()],
    );
    let visible_announcement = home
        .pod_announcement(
            &home.default_auth_context().unwrap(),
            &visible.slug,
            "https://home.example/federation/pods/visible-systems",
        )
        .unwrap();
    let endorser_announcement = home
        .pod_announcement(
            &home.default_auth_context().unwrap(),
            &endorser.slug,
            "https://home.example/federation/pods/trusted-curators",
        )
        .unwrap();
    home.index_pod_announcement(visible_announcement.clone())
        .unwrap();
    home.index_pod_announcement(endorser_announcement.clone())
        .unwrap();
    let curator = harness(
        &home,
        "Explore endorsement curator",
        vec![HarnessCapability::PodCuration],
    );
    home.endorse_public_pod(
        &curator,
        &endorser_announcement,
        &visible_announcement,
        "Careful source selection".into(),
    )
    .unwrap();

    let node_blocked_pod = create_public_pod(
        &blocked_node,
        "node-blocked-systems",
        "Systems from a locally blocked node",
    );
    let node_blocked_announcement = blocked_node
        .pod_announcement(
            &blocked_node.default_auth_context().unwrap(),
            &node_blocked_pod.slug,
            "https://blocked-node.example/federation/pods/node-blocked-systems",
        )
        .unwrap();
    home.index_pod_announcement(node_blocked_announcement.clone())
        .unwrap();
    let pod_blocked = create_public_pod(
        &blocked_pod_origin,
        "pod-blocked-systems",
        "Systems from a locally blocked Pod",
    );
    let pod_blocked_announcement = blocked_pod_origin
        .pod_announcement(
            &blocked_pod_origin.default_auth_context().unwrap(),
            &pod_blocked.slug,
            "https://blocked-pod.example/federation/pods/pod-blocked-systems",
        )
        .unwrap();
    home.index_pod_announcement(pod_blocked_announcement.clone())
        .unwrap();
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockNode {
            node_id: node_blocked_announcement.origin_node_id,
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockPod {
            origin_node_id: pod_blocked_announcement.origin_node_id,
            pod_slug: pod_blocked_announcement.pod_slug,
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
    let reader = harness(
        &home,
        "intentional Explore",
        vec![HarnessCapability::FeedRead],
    );

    let explored = home
        .explore_public_pods(&reader, ExploreRequest::new("systems", 10, 3).unwrap())
        .unwrap();

    assert!(explored.results.iter().all(|result| {
        !matches!(
            result.announcement.pod_slug.as_str(),
            "node-blocked-systems" | "pod-blocked-systems"
        )
    }));
    let result = explored
        .results
        .iter()
        .find(|result| result.announcement.pod_slug == "visible-systems")
        .unwrap();
    assert!(!result.is_subscribed);
    assert_eq!(result.sample_content_references.len(), 1);
    assert_eq!(
        result.sample_content_references[0].canonical_url,
        "https://allowed.example/research"
    );
    assert_eq!(result.endorsements.len(), 1);
    assert!(result.reasons.iter().any(|reason| {
        reason.contains("endorsement evidence") && reason.contains("local ranking evidence")
    }));
    assert!(result
        .reasons
        .iter()
        .any(|reason| reason.contains("subject evidence")));
}

#[test]
fn discovery_substrate_state_survives_sqlite_restart() {
    let data_dir = TestDataDir::new("restart");
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let endorser = create_public_pod(&tools, "restart-curators", "Persistent curators");
    let target = create_public_pod(&tools, "restart-systems", "Persistent systems research");
    accept_item(
        &tools,
        &target,
        "restart-sample",
        "https://restart.example/reference",
        vec!["systems".into()],
    );
    let announcement = tools
        .pod_announcement(
            &tools.default_auth_context().unwrap(),
            &target.slug,
            "https://restart.example/federation/pods/restart-systems",
        )
        .unwrap();
    let endorser_announcement = tools
        .pod_announcement(
            &tools.default_auth_context().unwrap(),
            &endorser.slug,
            "https://restart.example/federation/pods/restart-curators",
        )
        .unwrap();
    tools.index_pod_announcement(announcement.clone()).unwrap();
    tools
        .index_pod_announcement(endorser_announcement.clone())
        .unwrap();
    let samples = tools
        .pod_explore_samples(&tools.default_auth_context().unwrap(), &announcement, 1)
        .unwrap();
    tools
        .accept_pod_explore_samples(&tools.default_auth_context().unwrap(), samples)
        .unwrap();
    let curator = harness(
        &tools,
        "restart endorsement curator",
        vec![HarnessCapability::PodCuration],
    );
    tools
        .endorse_public_pod(
            &curator,
            &endorser_announcement,
            &announcement,
            "Persistent recommendation".into(),
        )
        .unwrap();
    approve_trust_policy_change(
        &tools,
        TrustPolicyChange::AddIndexNode {
            label: "persistent index".into(),
            base_url: "https://index.example".into(),
        },
    );
    let user_id = curator.user_id.unwrap();
    drop(tools);

    let restarted = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let mut ctx = restarted.default_auth_context().unwrap();
    ctx.user_id = Some(user_id);
    let explored = restarted
        .explore_public_pods(&ctx, ExploreRequest::new("systems", 10, 1).unwrap())
        .unwrap();

    assert_eq!(explored.results.len(), 1);
    assert_eq!(explored.results[0].endorsements.len(), 1);
    assert_eq!(explored.results[0].sample_content_references.len(), 1);
    assert_eq!(restarted.trust_policy(&ctx).unwrap().index_nodes.len(), 1);
}
