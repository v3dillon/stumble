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
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Directly concerns the Pod subject".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                },
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
                    media_references: Vec::new(),
                    tags,
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                        discovery_method: "browser_search".into(),
                        referrer_url: None,
                    },
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

fn pod_owner(tools: &AgentTools, pod_id: PodId) -> AuthContext {
    let store = tools.store();
    let store = store.read().unwrap();
    let owner_user = store
        .pod_roles
        .iter()
        .find(|assignment| assignment.pod_id == pod_id && assignment.role == PodRole::Owner)
        .map(|assignment| assignment.user_id)
        .expect("public Pod has an Owner");
    let mut ctx = tools.default_auth_context().unwrap();
    ctx.user_id = Some(owner_user);
    // Owner operations authorize via User pod role rather than harness capability alone.
    ctx
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

#[test]
fn newly_produced_announcement_carries_signed_thirty_day_lease() {
    let origin_dir = TestDataDir::new("lease-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "lease-systems",
        "Systems research with a renewable lease",
    );
    let issued_at = Utc.with_ymd_and_hms(2026, 3, 1, 12, 0, 0).unwrap();
    let announcement = origin
        .pod_announcement_at(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/lease-systems",
            issued_at,
        )
        .unwrap();

    assert_eq!(announcement.announced_at, issued_at);
    assert_eq!(
        announcement.expires_at,
        issued_at + announcement_lease_duration()
    );
    assert_eq!(
        announcement.expires_at - announcement.announced_at,
        chrono::Duration::days(ANNOUNCEMENT_LEASE_DURATION_DAYS)
    );
    assert!(announcement.verify().unwrap());
    assert_eq!(announcement.origin_node_id, announcement.signer.node_id);

    let mut forged = announcement.clone();
    forged.expires_at = issued_at + chrono::Duration::days(60);
    assert!(!forged.verify().unwrap());
}

#[test]
fn origin_can_renew_lease_and_consumers_prefer_current_valid_lease() {
    let origin_dir = TestDataDir::new("renew-origin");
    let home_dir = TestDataDir::new("renew-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "renewable-systems", "Renewable systems curation");
    let t0 = Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
    let t1 = t0 + chrono::Duration::days(10);
    let url = "https://origin.example/federation/pods/renewable-systems";
    let first = origin
        .pod_announcement_at(&origin.default_auth_context().unwrap(), &pod.slug, url, t0)
        .unwrap();
    let renewal = origin
        .pod_announcement_at(&origin.default_auth_context().unwrap(), &pod.slug, url, t1)
        .unwrap();

    assert!(renewal.announced_at > first.announced_at);
    assert_eq!(renewal.expires_at, t1 + announcement_lease_duration());
    assert!(renewal.verify().unwrap());

    home.index_pod_announcement_at(first.clone(), t1).unwrap();
    let retained = home.index_pod_announcement_at(renewal.clone(), t1).unwrap();
    assert_eq!(retained.announcement.id, renewal.id);
    assert!(matches!(
        home.index_pod_announcement_at(first, t1),
        Err(AgentToolsError::Store(StoreError::AnnouncementStale))
    ));
    let store = home.store();
    let known = store.read().unwrap();
    let current = known
        .known_pod_announcements
        .get(&(renewal.origin_node_id, renewal.pod_slug.clone()))
        .unwrap();
    assert_eq!(current.announcement.id, renewal.id);
}

#[test]
fn public_metadata_package_or_event_changes_refresh_announcement() {
    let origin_dir = TestDataDir::new("refresh-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "refresh-systems", "Initial systems subject");
    let url = "https://origin.example/federation/pods/refresh-systems";
    let t0 = Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap();
    let first = origin
        .pod_announcement_at(&origin.default_auth_context().unwrap(), &pod.slug, url, t0)
        .unwrap();

    // Change public metadata and package version; accept_item appends a federated
    // event and must auto-refresh the retained Origin announcement without a
    // second manual pod_announcement call.
    {
        let store = origin.store();
        let mut store = store.write().unwrap();
        store.pods.get_mut(&pod.id).unwrap().description =
            "Updated systems subject after package revision".into();
        store.pods.get_mut(&pod.id).unwrap().name = "Refresh Systems v2".into();
        let pack = store.pod_skill_packs.get_mut(&pod.id).unwrap();
        pack.version = 2;
    }
    accept_item(
        &origin,
        &pod,
        "refresh-item",
        "https://allowed.example/refresh",
        vec!["systems".into()],
    );

    let store = origin.store();
    let store = store.read().unwrap();
    let node_id = store.default_node().unwrap().id;
    let refreshed = &store
        .known_pod_announcements
        .get(&(node_id, pod.slug.clone()))
        .expect("origin retains refreshed announcement after public state change")
        .announcement;

    assert_ne!(refreshed.id, first.id);
    assert!(refreshed.announced_at > first.announced_at);
    assert_eq!(
        refreshed.expires_at,
        refreshed.announced_at + announcement_lease_duration()
    );
    assert_eq!(
        refreshed.subject,
        "Updated systems subject after package revision"
    );
    assert_eq!(refreshed.pod_name, "Refresh Systems v2");
    assert_eq!(refreshed.package_version, PackageVersion::new(2).unwrap());
    assert!(refreshed.latest_event_hash.is_some());
    assert_ne!(refreshed.latest_event_hash, first.latest_event_hash);
    assert_eq!(refreshed.public_pod_url, url);
    assert!(refreshed.verify().unwrap());
}

#[test]
fn making_public_pod_private_produces_origin_signed_withdrawal() {
    let origin_dir = TestDataDir::new("private-withdraw-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "soon-private", "Will leave public discovery");
    let url = "https://origin.example/federation/pods/soon-private";
    let now = Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap();
    let announcement = origin
        .pod_announcement_at(&origin.default_auth_context().unwrap(), &pod.slug, url, now)
        .unwrap();
    origin
        .index_pod_announcement_at(announcement.clone(), now)
        .unwrap();

    let owner = pod_owner(&origin, pod.id);
    let outcome = origin
        .request_set_pod_visibility(&owner, pod.id, Visibility::Private, now)
        .unwrap();
    assert!(matches!(outcome, PodVisibilityOutcome::Updated(_)));

    let store = origin.store();
    let store = store.read().unwrap();
    let known = store
        .known_pod_withdrawals
        .get(&(announcement.origin_node_id, pod.slug.clone()))
        .expect("withdrawal retained after making private");
    assert!(known.withdrawal.verify().unwrap());
    assert_eq!(known.withdrawal.pod_slug, pod.slug);
    assert_eq!(known.withdrawal.origin_node_id, announcement.origin_node_id);
    assert!(!store
        .known_pod_announcements
        .contains_key(&(announcement.origin_node_id, pod.slug.clone())));
}

#[test]
fn explicit_withdraw_produces_origin_signed_withdrawal_bound_to_pod() {
    let origin_dir = TestDataDir::new("explicit-withdraw-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "explicit-withdraw",
        "Explicitly withdrawn systems Pod",
    );
    let url = "https://origin.example/federation/pods/explicit-withdraw";
    let now = Utc.with_ymd_and_hms(2026, 5, 2, 0, 0, 0).unwrap();
    let announcement = origin
        .pod_announcement_at(&origin.default_auth_context().unwrap(), &pod.slug, url, now)
        .unwrap();
    origin
        .index_pod_announcement_at(announcement.clone(), now)
        .unwrap();

    let withdrawal = origin
        .withdraw_public_pod(
            &pod_owner(&origin, pod.id),
            &pod.slug,
            Some(url),
            true,
            now + chrono::Duration::minutes(1),
        )
        .unwrap();

    assert!(withdrawal.verify().unwrap());
    assert_eq!(withdrawal.pod_slug, pod.slug);
    assert_eq!(withdrawal.origin_node_id, announcement.origin_node_id);
    assert_eq!(withdrawal.covers_announcement_id, Some(announcement.id));
    assert_eq!(withdrawal.public_pod_url.as_deref(), Some(url));
    assert_eq!(
        origin.pod_by_slug(&pod.slug, None).unwrap().visibility,
        Visibility::Private
    );
}

#[test]
fn invalid_stale_forged_renewals_and_withdrawals_are_rejected_without_state_change() {
    let origin_dir = TestDataDir::new("reject-origin");
    let home_dir = TestDataDir::new("reject-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "reject-systems", "Rejection cases");
    let url = "https://origin.example/federation/pods/reject-systems";
    let t0 = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
    let t1 = t0 + chrono::Duration::days(5);
    let good = origin
        .pod_announcement_at(&origin.default_auth_context().unwrap(), &pod.slug, url, t0)
        .unwrap();
    home.index_pod_announcement_at(good.clone(), t0).unwrap();
    let before = home.store().read().unwrap().known_pod_announcements.clone();

    let mut forged = good.clone();
    forged.subject = "attacker subject".into();
    assert!(matches!(
        home.index_pod_announcement_at(forged, t0),
        Err(AgentToolsError::Store(StoreError::InvalidSignature))
    ));

    // Build older/expired Origin signatures without retaining them on the Origin
    // (issuance always retains, which would reject a strictly older lease).
    let (stale, expired) = {
        let store = origin.store();
        let store = store.read().unwrap();
        let node = store.default_node().unwrap();
        let pod = store.pod_by_slug(&pod.slug, None).unwrap();
        let stale =
            build_signed_pod_announcement(&store, &node, &pod, url, t0 - chrono::Duration::days(1))
                .unwrap();
        let expired = build_signed_pod_announcement(&store, &node, &pod, url, t0).unwrap();
        (stale, expired)
    };
    assert!(matches!(
        home.index_pod_announcement_at(stale, t1),
        Err(AgentToolsError::Store(StoreError::AnnouncementStale))
    ));

    let expired_now = t0 + announcement_lease_duration() + chrono::Duration::seconds(1);
    let expired_home_dir = TestDataDir::new("reject-expired-home");
    let expired_home = AgentTools::open_home_node(&expired_home_dir.0, seed_store).unwrap();
    assert!(matches!(
        expired_home.index_pod_announcement_at(expired, expired_now),
        Err(AgentToolsError::Store(StoreError::AnnouncementExpired))
    ));

    let mut forged_subject = good.clone();
    forged_subject.signature = "not-a-real-signature".into();
    assert!(matches!(
        home.index_pod_announcement_at(forged_subject, t0),
        Err(AgentToolsError::Store(StoreError::InvalidSignature))
            | Err(AgentToolsError::Signing(_))
    ));

    let withdrawal = origin
        .withdraw_public_pod(&pod_owner(&origin, pod.id), &pod.slug, Some(url), false, t1)
        .unwrap();
    let mut forged_withdrawal = withdrawal.clone();
    forged_withdrawal.pod_slug = "other-pod".into();
    assert!(matches!(
        home.index_pod_withdrawal_at(forged_withdrawal, t1),
        Err(AgentToolsError::Store(StoreError::InvalidSignature))
    ));

    let after = home.store().read().unwrap().known_pod_announcements.clone();
    assert_eq!(before, after);
}

#[test]
fn expiry_and_withdrawal_remove_discovery_while_preserving_subscriptions() {
    let origin_dir = TestDataDir::new("lifecycle-origin");
    let home_dir = TestDataDir::new("lifecycle-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "lifecycle-systems", "Lifecycle systems research");
    accept_item(
        &origin,
        &pod,
        "lifecycle-item",
        "https://allowed.example/lifecycle",
        vec!["systems".into()],
    );
    let url = "https://origin.example/federation/pods/lifecycle-systems";
    let t0 = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
    let announcement = origin
        .pod_announcement_at(&origin.default_auth_context().unwrap(), &pod.slug, url, t0)
        .unwrap();

    // Home indexes and forms a subscription (private local state).
    home.index_pod_announcement_at(announcement.clone(), t0)
        .unwrap();
    let reader = harness(
        &home,
        "lifecycle feed reader",
        vec![HarnessCapability::FeedRead],
    );
    let admin = harness(
        &home,
        "lifecycle admin",
        vec![HarnessCapability::Administration],
    );
    let origin_info = origin
        .node_info(&origin.default_auth_context().unwrap())
        .unwrap();
    let peer = trust_peer(&home, &origin_info, "https://origin.example");
    // Simulate subscription and synchronized content without full peer sync.
    {
        let store = home.store();
        let mut store = store.write().unwrap();
        let user_id = reader.user_id.unwrap();
        let local_pod_id = uuid::Uuid::now_v7();
        let local_pod = Pod {
            id: local_pod_id,
            tenant_id: None,
            name: pod.name.clone(),
            slug: pod.slug.clone(),
            description: pod.description.clone(),
            visibility: Visibility::Public,
            created_by: None,
            created_at: t0,
            origin_node_id: Some(announcement.origin_node_id),
        };
        store.pods.insert(local_pod_id, local_pod.clone());
        let node = store.default_node().unwrap().clone();
        let mut subscription = Subscription::new_local(
            SubscriptionId::from(uuid::Uuid::now_v7()),
            user_id,
            &local_pod,
            &node,
            t0,
        );
        // Pin remote origin identity while keeping local projection state.
        subscription.origin_node_id = announcement.origin_node_id;
        subscription.origin_public_key = announcement.signer.public_key.clone();
        subscription.public_pod_url = url.into();
        subscription.last_event_hash = announcement.latest_event_hash.clone();
        store.subscriptions.insert(subscription.id, subscription);
        let content_id = uuid::Uuid::now_v7();
        store.submissions.insert(
            content_id,
            Submission {
                id: content_id,
                tenant_id: None,
                url: "https://allowed.example/lifecycle".into(),
                canonical_url: "https://allowed.example/lifecycle".into(),
                title: "Lifecycle item".into(),
                description: None,
                domain: "allowed.example".into(),
                submitted_by: None,
                discovered_by_crawler: false,
                submitter_note: None,
                summary: Some("kept after withdrawal".into()),
                media_references: Vec::new(),
                tags: vec!["systems".into()],
                embedding: None,
                created_at: t0,
                origin_event_id: None,
            },
        );
    }

    assert_eq!(
        home.explore_public_pods(&reader, ExploreRequest::new("systems", 10, 0).unwrap())
            .unwrap()
            .results
            .len(),
        1
    );
    assert_eq!(
        home.search_pod_announcements("systems", 10)
            .unwrap()
            .results
            .len(),
        1
    );
    assert_eq!(
        home.relay_pod_announcements(&admin, peer.id).unwrap().len(),
        1
    );

    // Expiry-driven exclusion (advanced clock) without mutating subscriptions.
    let expired_now = t0 + announcement_lease_duration() + Duration::seconds(1);
    {
        let store = home.store();
        let store = store.read().unwrap();
        assert!(
            !announcement_is_discovery_eligible(&store, &announcement, expired_now),
            "lease end is exclusive; advanced clock must exclude discovery"
        );
        assert!(store
            .subscriptions
            .values()
            .any(|sub| sub.pod_slug == pod.slug));
        assert!(!store.submissions.is_empty());
    }

    // Withdrawal removes discovery eligibility while the Pod is still in the
    // active window relative to real Utc::now() used by search/relay/explore.
    // Re-index a freshly timed announcement first so surface APIs still see it.
    let live_now = Utc::now();
    let live_announcement = origin
        .pod_announcement_at(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            url,
            live_now,
        )
        .unwrap();
    home.index_pod_announcement_at(live_announcement.clone(), live_now)
        .unwrap();
    assert_eq!(
        home.search_pod_announcements("systems", 10)
            .unwrap()
            .results
            .len(),
        1
    );

    let withdrawal = origin
        .withdraw_public_pod(
            &pod_owner(&origin, pod.id),
            &pod.slug,
            Some(url),
            true,
            live_now + chrono::Duration::hours(1),
        )
        .unwrap();
    home.index_pod_withdrawal_at(withdrawal, live_now + chrono::Duration::hours(1))
        .unwrap();

    assert!(home
        .explore_public_pods(&reader, ExploreRequest::new("systems", 10, 0).unwrap())
        .unwrap()
        .results
        .is_empty());
    assert!(home
        .search_pod_announcements("systems", 10)
        .unwrap()
        .results
        .is_empty());
    assert!(home
        .relay_pod_announcements(&admin, peer.id)
        .unwrap()
        .is_empty());

    // Subscription and synchronized content remain.
    let store = home.store();
    let store = store.read().unwrap();
    assert!(store
        .subscriptions
        .values()
        .any(|sub| sub.pod_slug == pod.slug));
    assert!(!store.submissions.is_empty());
    assert!(store.pods.values().any(|p| p.slug == pod.slug));
}

#[test]
fn announcement_lease_and_withdrawal_state_survive_sqlite_restart() {
    let data_dir = TestDataDir::new("lease-persist");
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&tools, "persist-lease", "Persistent lease and withdrawal");
    let url = "https://home.example/federation/pods/persist-lease";
    let t0 = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();
    let announcement = tools
        .pod_announcement_at(&tools.default_auth_context().unwrap(), &pod.slug, url, t0)
        .unwrap();
    tools
        .index_pod_announcement_at(announcement.clone(), t0)
        .unwrap();
    let withdrawal = tools
        .withdraw_public_pod(
            &pod_owner(&tools, pod.id),
            &pod.slug,
            Some(url),
            false,
            t0 + chrono::Duration::minutes(5),
        )
        .unwrap();
    drop(tools);

    let restarted = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let store = restarted.store();
    let store = store.read().unwrap();
    assert!(
        !store
            .known_pod_announcements
            .contains_key(&(announcement.origin_node_id, pod.slug.clone())),
        "withdrawal should clear discovery announcement"
    );
    let known = store
        .known_pod_withdrawals
        .get(&(announcement.origin_node_id, pod.slug.clone()))
        .expect("withdrawal survives restart");
    assert_eq!(known.withdrawal.id, withdrawal.id);
    assert!(known.withdrawal.verify().unwrap());
}

#[test]
fn home_node_fetches_bounded_origin_samples_only_when_signature_and_binding_verify() {
    let origin_dir = TestDataDir::new("sample-fetch-origin");
    let home_dir = TestDataDir::new("sample-fetch-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "fetch-systems",
        "Distributed systems sample fetch subject",
    );
    accept_item(
        &origin,
        &pod,
        "fetch-allowed",
        "https://allowed.example/fetch-research",
        vec!["systems".into()],
    );
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/fetch-systems",
        )
        .unwrap();
    let good_samples = origin
        .pod_explore_samples(&origin.default_auth_context().unwrap(), &announcement, 5)
        .unwrap();
    home.index_pod_announcement(announcement.clone()).unwrap();
    let reader = harness(
        &home,
        "sample fetch reader",
        vec![HarnessCapability::FeedRead],
    );

    let mut client = ScriptedOriginExploreSampleClient::new();
    client.push(good_samples.clone());
    let accepted = home
        .fetch_origin_explore_samples(
            &reader,
            announcement.origin_node_id,
            &announcement.pod_slug,
            5,
            &client,
        )
        .unwrap();
    assert_eq!(accepted.announcement_id, announcement.id);
    assert_eq!(accepted.samples.len(), good_samples.samples.len());
    let captured = client.captured.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].announcement_id, announcement.id);
    assert_eq!(captured[0].limit, 5);
    // Outbound request carries only public announcement identity + limit.
    let payload = serde_json::json!({
        "public_pod_url": captured[0].public_pod_url,
        "announcement_id": captured[0].announcement_id,
        "limit": captured[0].limit,
    });
    assert!(sample_request_is_public_only(&payload));
    drop(captured);

    // Stale binding is rejected.
    let mut stale = good_samples;
    stale.announcement_id = uuid::Uuid::now_v7();
    let mut bad_client = ScriptedOriginExploreSampleClient::new();
    bad_client.push(stale);
    assert!(home
        .fetch_origin_explore_samples(
            &reader,
            announcement.origin_node_id,
            &announcement.pod_slug,
            5,
            &bad_client,
        )
        .is_err());
}

#[test]
fn deterministic_similarity_exposes_subject_source_sample_and_endorsement_reasons() {
    let origin_dir = TestDataDir::new("sim-origin");
    let home_dir = TestDataDir::new("sim-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "sim-systems",
        "Distributed systems reliability research",
    );
    let endorser = create_public_pod(&origin, "sim-curators", "Systems curators");
    accept_item(
        &origin,
        &pod,
        "sim-sample",
        "https://acm.org/systems-paper",
        vec!["systems".into(), "reliability".into()],
    );
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/sim-systems",
        )
        .unwrap();
    let endorser_announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &endorser.slug,
            "https://origin.example/federation/pods/sim-curators",
        )
        .unwrap();
    let samples = origin
        .pod_explore_samples(&origin.default_auth_context().unwrap(), &announcement, 5)
        .unwrap();
    home.index_pod_announcement(announcement.clone()).unwrap();
    home.index_pod_announcement(endorser_announcement.clone())
        .unwrap();
    let reader = harness(&home, "sim reader", vec![HarnessCapability::FeedRead]);
    home.accept_pod_explore_samples(&reader, samples).unwrap();
    let curator = harness(
        &origin,
        "sim endorsement curator",
        vec![HarnessCapability::PodCuration],
    );
    let endorsement = origin
        .endorse_public_pod(
            &curator,
            &endorser_announcement,
            &announcement,
            "Careful systems curation".into(),
        )
        .unwrap();
    home.index_pod_endorsement(endorsement).unwrap();

    let explored = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems reliability", 10, 5).unwrap(),
        )
        .unwrap();
    let result = explored
        .results
        .iter()
        .find(|result| result.announcement.pod_slug == "sim-systems")
        .expect("similar pod returned");
    assert!(result.relevance > 0.0);
    assert!(!result.trial_exposure, "endorsed pods are not trial");
    assert!(result
        .reasons
        .iter()
        .any(|reason| reason.contains("subject evidence")));
    assert!(result
        .reasons
        .iter()
        .any(|reason| reason.contains("sample evidence") || reason.contains("source evidence")));
    assert!(result
        .reasons
        .iter()
        .any(|reason| reason.contains("endorsement evidence")));
    assert_eq!(result.endorsements.len(), 1);
}

#[test]
fn unendorsed_pod_receives_limited_labeled_trial_exposure_after_verification() {
    let origin_dir = TestDataDir::new("trial-origin");
    let home_dir = TestDataDir::new("trial-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "trial-systems",
        "Distributed systems research without endorsements",
    );
    accept_item(
        &origin,
        &pod,
        "trial-sample",
        "https://research.example/distributed-systems",
        vec!["systems".into(), "distributed".into()],
    );
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/trial-systems",
        )
        .unwrap();
    let samples = origin
        .pod_explore_samples(&origin.default_auth_context().unwrap(), &announcement, 5)
        .unwrap();
    home.index_pod_announcement(announcement.clone()).unwrap();
    let reader = harness(&home, "trial reader", vec![HarnessCapability::FeedRead]);
    home.accept_pod_explore_samples(&reader, samples).unwrap();

    let explored = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 5).unwrap(),
        )
        .unwrap();
    let result = explored
        .results
        .iter()
        .find(|result| result.announcement.pod_slug == "trial-systems")
        .expect("unendorsed similar pod");
    assert!(result.endorsements.is_empty());
    assert!(result.trial_exposure);
    assert!(result
        .reasons
        .iter()
        .any(|reason| reason.contains("trial exposure")));
    assert!(!result.sample_content_references.is_empty());
}

#[test]
fn per_origin_caps_bound_explore_results_from_one_origin() {
    let origin_dir = TestDataDir::new("cap-origin");
    let home_dir = TestDataDir::new("cap-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    for slug in ["cap-a", "cap-b", "cap-c", "cap-d"] {
        let pod = create_public_pod(
            &origin,
            slug,
            &format!("Distributed systems research {slug}"),
        );
        let announcement = origin
            .pod_announcement(
                &origin.default_auth_context().unwrap(),
                &pod.slug,
                &format!("https://origin.example/federation/pods/{slug}"),
            )
            .unwrap();
        home.index_pod_announcement(announcement).unwrap();
    }
    let reader = harness(&home, "cap reader", vec![HarnessCapability::FeedRead]);
    let explored = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 0).unwrap(),
        )
        .unwrap();
    assert!(
        explored.results.len() <= MAX_RESULTS_PER_ORIGIN,
        "expected at most {MAX_RESULTS_PER_ORIGIN} results from one Origin, got {}",
        explored.results.len()
    );
    let origins: std::collections::HashSet<_> = explored
        .results
        .iter()
        .map(|result| result.announcement.origin_node_id)
        .collect();
    assert_eq!(origins.len(), 1);
}

#[test]
fn local_blocks_exclude_before_similarity_ranking() {
    let origin_dir = TestDataDir::new("block-sim-origin");
    let home_dir = TestDataDir::new("block-sim-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let allowed = create_public_pod(
        &origin,
        "block-allowed",
        "Distributed systems research allowed",
    );
    let blocked = create_public_pod(
        &origin,
        "block-denied",
        "Distributed systems research blocked",
    );
    for pod in [&allowed, &blocked] {
        let announcement = origin
            .pod_announcement(
                &origin.default_auth_context().unwrap(),
                &pod.slug,
                &format!("https://origin.example/federation/pods/{}", pod.slug),
            )
            .unwrap();
        home.index_pod_announcement(announcement).unwrap();
    }
    let blocked_origin = {
        let store = home.store();
        let store = store.read().unwrap();
        store
            .known_pod_announcements
            .values()
            .find(|known| known.announcement.pod_slug == "block-denied")
            .unwrap()
            .announcement
            .origin_node_id
    };
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockPod {
            origin_node_id: blocked_origin,
            pod_slug: "block-denied".into(),
        },
    );
    let reader = harness(&home, "block sim reader", vec![HarnessCapability::FeedRead]);
    let explored = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 0).unwrap(),
        )
        .unwrap();
    assert!(explored
        .results
        .iter()
        .all(|result| result.announcement.pod_slug != "block-denied"));
    assert!(explored
        .results
        .iter()
        .any(|result| result.announcement.pod_slug == "block-allowed"));
}

#[test]
fn explore_similarity_is_local_without_remote_interest_queries() {
    // Scripted client records every outbound sample fetch. Explore ranking itself
    // never calls the client; private interests stay local.
    let origin_dir = TestDataDir::new("local-only-origin");
    let home_dir = TestDataDir::new("local-only-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "local-only-systems",
        "Distributed systems research local only",
    );
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/local-only-systems",
        )
        .unwrap();
    home.index_pod_announcement(announcement).unwrap();
    let reader = harness(
        &home,
        "local only reader",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
    );
    // Set private interest evidence that must never leave the Home Node.
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["distributed systems".into()]);
    home.update_taste_profile(&reader, taste).unwrap();

    let client = ScriptedOriginExploreSampleClient::new();
    let explored = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems", 10, 0).unwrap(),
        )
        .unwrap();
    assert!(!explored.results.is_empty());
    // No Origin sample fetch was authorized during pure local ranking.
    assert!(
        client.captured.lock().unwrap().is_empty(),
        "explore ranking must not issue background interest-derived remote queries"
    );
    assert!(feedback_affects_future_exposure(FeedbackKind::Interesting));
    assert!(!feedback_affects_future_exposure(FeedbackKind::Dismissed));
}
