use chrono::{Duration, TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-discovery-substrate-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(tools: &AgentTools, label: &str, capabilities: Vec<HarnessCapability>) -> AuthContext {
    let owner = tools.default_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Interactive,
                capabilities,
                pod_ids: None,
            },
        )
        .unwrap();
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

fn create_public_pod(tools: &AgentTools, slug: &str, description: &str) -> Pod {
    let proposer = harness(
        tools,
        "public Pod proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness(
        tools,
        "public Pod approver",
        vec![HarnessCapability::Approval],
    );
    let pod = tools
        .create_pod(
            &proposer,
            CreatePodRequest {
                name: slug.replace('-', " "),
                slug: slug.into(),
                description: description.into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

fn accept_item(tools: &AgentTools, pod: &Pod, suffix: &str, source_url: &str, tags: Vec<String>) {
    let submitter = harness(
        tools,
        &format!("{suffix} submitter"),
        vec![HarnessCapability::CandidateSubmission],
    );
    let curator = harness(
        tools,
        &format!("{suffix} curator"),
        vec![HarnessCapability::PodCuration],
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let candidate = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                evidence: CandidateSubmissionEvidence {
                    source_url: source_url.into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Reference {suffix}")),
                        author: Some("Careful author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("A permitted sample excerpt".into()),
                    summary: Some("A useful public Content Reference".into()),
                    content_type: CandidateContentType::Article,
                    tags,
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                        discovery_method: "browser_search".into(),
                        referrer_url: None,
                    },
                    proposed_placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Directly concerns the Pod subject".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                    harness_idempotency_key: format!("worker-{suffix}"),
                    client_idempotency_key: format!("client-{suffix}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, candidate.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &curator,
            candidate.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
}

fn trust_peer(tools: &AgentTools, peer: &NodeInfo, base_url: &str) -> TrustedPeer {
    let proposer = harness(
        tools,
        "Trust Policy proposer",
        vec![HarnessCapability::Administration],
    );
    let approver = harness(
        tools,
        "Trust Policy approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let proposal = tools
        .request_add_trusted_peer(
            &proposer,
            peer.display_name.clone(),
            base_url.into(),
            peer.public_key.clone(),
            now,
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools
        .trusted_peers(&proposer)
        .unwrap()
        .into_iter()
        .find(|trusted| trusted.public_key == peer.public_key)
        .unwrap()
}

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

fn approve_trust_policy_change(tools: &AgentTools, change: TrustPolicyChange) {
    let proposer = harness(
        tools,
        "local Trust Policy editor",
        vec![HarnessCapability::Administration],
    );
    let approver = harness(
        tools,
        "local Trust Policy approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let proposal = tools
        .request_trust_policy_change(&proposer, change, now)
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
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
    let index = AgentTools::open_home_node(&index_dir.0, seed_store).unwrap();
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
    let home_info = home
        .node_info(&home.default_auth_context().unwrap())
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
    let relay_home_peer = trust_peer(&relay, &home_info, "https://home.example");
    let relayed_announcement = relay
        .relay_pod_announcements(&relay_admin, relay_home_peer.id)
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.pod_slug == announcement.pod_slug)
        .unwrap();

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
    let first_index = AgentTools::open_home_node(&first_index_dir.0, seed_store).unwrap();
    let replacement_index =
        AgentTools::open_home_node(&replacement_index_dir.0, seed_store).unwrap();
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
    let index = AgentTools::open_home_node(&index_dir.0, seed_store).unwrap();
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

#[test]
fn remote_unsubscribed_explore_uses_origin_signed_policy_filtered_samples() {
    let origin_dir = TestDataDir::new("remote-sample-origin");
    let index_dir = TestDataDir::new("remote-sample-index");
    let home_dir = TestDataDir::new("remote-sample-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store).unwrap();
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
    assert!(result
        .reasons
        .iter()
        .any(|reason| reason.contains("local ranking evidence")));
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
