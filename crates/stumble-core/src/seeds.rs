use crate::agent_tools::AgentTools;
use crate::domain::*;
use crate::signing::{create_node_identity, hash_api_token, new_plaintext_api_token};
#[cfg(test)]
use crate::skill_pack::{
    default_skill_pack, pod_package_contents_from_files, pod_request_from_template,
};
use crate::store::InMemoryStore;
#[cfg(test)]
use chrono::Duration;
use chrono::Utc;
use uuid::Uuid;

pub fn seed_store() -> InMemoryStore {
    let mut store = InMemoryStore::default();
    // Sponsored Bootstrap is ordinary removable Home Node config, not protocol authority.
    crate::bootstrap::ensure_default_bootstrap_endpoint(&mut store, Utc::now());
    // Automatic Discovery Peer gossip is enabled by default (outbound only).
    crate::discovery_peer::ensure_discovery_peer_gossip_config(&mut store);
    let local_node = create_node_identity("local stumble node", None);
    store
        .node_identities
        .insert(local_node.id, local_node.clone());

    let hosted_tenant = Tenant {
        id: Uuid::now_v7(),
        name: "Default Hosted Tenant".to_string(),
        slug: "default-hosted".to_string(),
        created_at: Utc::now(),
    };
    store
        .tenants
        .insert(hosted_tenant.id, hosted_tenant.clone());
    let hosted_node = create_node_identity("default hosted managed node", Some(hosted_tenant.id));
    store
        .node_identities
        .insert(hosted_node.id, hosted_node.clone());

    for idx in 1..=3 {
        let user = User {
            id: Uuid::now_v7(),
            display_name: format!("Seed User {idx}"),
            created_at: Utc::now(),
        };
        store.tenant_users.push(TenantUser {
            tenant_id: hosted_tenant.id,
            user_id: user.id,
            role: if idx == 1 {
                TenantRole::Owner
            } else {
                TenantRole::Member
            },
            created_at: Utc::now(),
        });
        store.user_preferences.insert(
            (user.id, None),
            UserPreferences {
                user_id: user.id,
                tenant_id: None,
                interests: vec![
                    "interfaces".to_string(),
                    "tools".to_string(),
                    "agents".to_string(),
                ],
                blocked_topics: vec!["politics".to_string()],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            },
        );
        store.users.insert(user.id, user);
    }

    let peer_one = create_node_identity("trusted design lab", None);
    let peer_two = create_node_identity("hosted relay example", None);
    let peer_one_id = Uuid::now_v7();
    store.trusted_peers.insert(
        peer_one_id,
        TrustedPeer {
            id: peer_one_id,
            node_id: peer_one.id,
            tenant_id: None,
            display_name: "Trusted Design Lab".to_string(),
            base_url: "https://design-lab.example".to_string(),
            public_key: peer_one.public_key,
            trust_level: TrustLevel::ReadWrite,
            enabled: true,
            created_at: Utc::now(),
        },
    );
    let peer_two_id = Uuid::now_v7();
    store.trusted_peers.insert(
        peer_two_id,
        TrustedPeer {
            id: peer_two_id,
            node_id: peer_two.id,
            tenant_id: Some(hosted_tenant.id),
            display_name: "Hosted Relay Example".to_string(),
            base_url: "https://relay.example".to_string(),
            public_key: peer_two.public_key,
            trust_level: TrustLevel::ReadOnly,
            enabled: true,
            created_at: Utc::now(),
        },
    );

    let user_ids: Vec<_> = store.users.keys().copied().collect();

    let token = new_plaintext_api_token();
    let token_hash = hash_api_token(&token);
    if let Some(user_id) = user_ids.first().copied() {
        let api_token_id = Uuid::now_v7();
        store.api_tokens.insert(
            api_token_id,
            ApiToken {
                id: api_token_id,
                user_id,
                tenant_id: None,
                token_hash,
                label: "seed-dev-token".to_string(),
                created_at: Utc::now(),
                last_used_at: None,
                revoked_at: None,
                harness_id: None,
            },
        );
    }

    store
}

pub fn seed_agent_tools() -> AgentTools {
    AgentTools::new(seed_store())
}

#[cfg(test)]
fn insert_seed_pod(
    store: &mut InMemoryStore,
    node: &NodeIdentity,
    name: &str,
    slug: &str,
    description: &str,
) {
    let request = pod_request_from_template(name, slug);
    let pod = Pod {
        id: Uuid::now_v7(),
        tenant_id: None,
        name: request.name,
        slug: request.slug,
        description: description.to_string(),
        visibility: Visibility::Public,
        created_by: None,
        created_at: Utc::now(),
        origin_node_id: Some(node.id),
    };
    store.pod_rules.insert(
        pod.id,
        PodRules {
            pod_id: pod.id,
            blocked_topics: vec!["politics".to_string()],
            blocked_domains: vec![],
            auto_promote_crawler_candidates: false,
            federate_sources: true,
        },
    );
    let package = default_skill_pack(&pod);
    let _ = store.insert_pod_package_version(package.clone());
    store.pod_skill_packs.insert(pod.id, package.clone());
    if let Ok(event) = crate::signing::sign_public_event(
        node,
        "pod_created",
        &pod.slug,
        serde_json::json!({"pod": pod.clone(), "package": package}),
        store.latest_event_hash(&pod.slug),
    ) {
        store.event_log.push(event);
    }
    store.pods.insert(pod.id, pod);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_tools::canonicalize_url;
    use crate::signing::{create_node_identity, sign_public_event, verify_event};
    use serde_json::json;

    fn ctx(store: &InMemoryStore) -> AuthContext {
        AuthContext {
            user_id: store.users.keys().next().copied(),
            tenant_id: None,
            node_id: store.default_node().unwrap().id,
            harness_id: None,
        }
    }

    fn add_public_seed_pod(store: &mut InMemoryStore, name: &str, slug: &str, description: &str) {
        let node = store.default_node().unwrap();
        insert_seed_pod(store, &node, name, slug, description);
    }

    fn add_beautiful_interfaces(store: &mut InMemoryStore) {
        add_public_seed_pod(
            store,
            "Beautiful Interfaces",
            "beautiful-interfaces",
            "Thoughtful, strange, useful interface design.",
        );
    }

    fn add_tools_for_thought(store: &mut InMemoryStore) {
        add_public_seed_pod(
            store,
            "Tools for Thought",
            "tools-for-thought",
            "Durable systems for thinking, writing, memory, and synthesis.",
        );
    }

    fn add_test_submission(
        store: &mut InMemoryStore,
        pod_slug: &str,
        title: &str,
        url: &str,
        tag_text: &str,
    ) -> SubmissionId {
        let pod_id = store.pod_by_slug(pod_slug, None).unwrap().id;
        let id = Uuid::now_v7();
        let parsed = url::Url::parse(url).expect("test url");
        let submission = Submission {
            id,
            tenant_id: None,
            url: url.to_string(),
            canonical_url: canonicalize_url(url).unwrap(),
            title: title.to_string(),
            source_metadata: CandidateSourceMetadata::default(),
            description: Some(format!("Test item for {pod_slug}.")),
            domain: parsed.domain().unwrap_or("example.com").to_string(),
            submitted_by: None,
            discovered_by_crawler: true,
            submitter_note: None,
            summary: Some(format!("{title} is a test discovery item.")),
            provenance: Vec::new(),
            media_references: Vec::new(),
            tags: tag_text
                .split_whitespace()
                .map(ToString::to_string)
                .collect(),
            embedding: None,
            created_at: Utc::now() - Duration::days(31),
            origin_event_id: None,
        };
        store.submissions.insert(id, submission);
        store.submission_pods.push(SubmissionPod {
            submission_id: id,
            pod_id,
            created_at: Utc::now(),
        });
        id
    }

    #[test]
    fn private_pods_are_hidden_from_the_federation_surface() {
        let mut store = seed_store();
        add_public_seed_pod(
            &mut store,
            "Weird Internet",
            "weird-internet",
            "Odd, humane, surprising corners of the web with artifacts.",
        );
        let tools = AgentTools::new(store.clone());
        let ctx = ctx(&store);

        // Seed pods ship as public.
        let public_slug = "weird-internet";

        let private = tools
            .create_pod(
                &ctx,
                CreatePodRequest {
                    name: "Secret".to_string(),
                    slug: "secret".to_string(),
                    description: "private pod".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();

        // The owner-facing listing still includes the private pod...
        let all = tools.list_pods(ctx.tenant_id).unwrap();
        assert!(all.iter().any(|pod| pod.slug == private.slug));

        // ...but the federation listing must expose public pods only.
        let public = tools.list_public_pods(&ctx).unwrap();
        assert!(
            public
                .iter()
                .all(|pod| pod.visibility == Visibility::Public),
            "federation listing leaked a non-public pod"
        );
        assert!(!public.iter().any(|pod| pod.slug == private.slug));
        assert!(public.iter().any(|pod| pod.slug == public_slug));

        // A private pod is reported exactly like a missing one over federation:
        // identical error, so there is no existence oracle.
        assert_eq!(
            tools
                .federation_pod_manifest(&ctx, &private.slug)
                .unwrap_err()
                .to_string(),
            "not found: pod secret",
        );
        assert_eq!(
            tools
                .federation_pod_manifest(&ctx, "ghost")
                .unwrap_err()
                .to_string(),
            "not found: pod ghost",
        );
        assert_eq!(
            tools
                .federation_pod_events(&ctx, &private.slug)
                .unwrap_err()
                .to_string(),
            "not found: pod secret",
        );

        // Public pods remain fully reachable over federation.
        assert!(tools.federation_pod_manifest(&ctx, public_slug).is_ok());
        assert!(tools.federation_pod_events(&ctx, public_slug).is_ok());
    }

    #[test]
    fn private_pods_are_excluded_from_home_public_discovery() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        let tools = AgentTools::new(store.clone());
        let ctx = ctx(&store);

        // A private pod whose name and description strongly match the Explore query.
        tools
            .create_pod(
                &ctx,
                CreatePodRequest {
                    name: "Secret Interfaces".to_string(),
                    slug: "secret-interfaces".to_string(),
                    description: "private interface design notes".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        // Public seed pods become Explore-eligible only through verified announcements.
        let announcement = tools
            .pod_announcement(
                &ctx,
                "beautiful-interfaces",
                "https://example.local/federation/pods/beautiful-interfaces",
            )
            .unwrap();
        tools.index_pod_announcement(announcement).unwrap();

        let discovery = tools
            .explore_public_pods(
                &ctx,
                ExploreRequest::new("interface design", 25, 0).unwrap(),
            )
            .unwrap();
        let slugs: Vec<&str> = discovery
            .results
            .iter()
            .map(|result| result.announcement.pod_slug.as_str())
            .collect();

        // The private pod must never surface on the public discovery surface...
        assert!(
            !slugs.contains(&"secret-interfaces"),
            "private pod leaked into home public discovery: {slugs:?}"
        );
        // ...while the matching public seed pod still does.
        assert!(slugs.contains(&"beautiful-interfaces"));
    }

    #[test]
    fn route_link_suggests_new_private_pod_when_no_pods_match() {
        let store = seed_store();
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);

        let routed = tools
            .route_link_to_pods(
                &context,
                RouteLinkRequest {
                    url: "https://example.com/robotics-lab".to_string(),
                    title: Some("Robotics Lab Notes".to_string()),
                    summary: Some("Hands-on robot perception experiments.".to_string()),
                    tags: vec!["robotics".to_string(), "perception".to_string()],
                },
                2.5,
            )
            .unwrap();

        assert!(routed.needs_confirmation);
        assert!(routed.selected.is_none());
        assert!(routed.candidates.is_empty());
        let suggested = routed.suggested_new_pod.unwrap();
        assert_eq!(suggested.name, "Robotics");
        assert_eq!(suggested.slug, "robotics");
        assert_eq!(suggested.visibility, Visibility::Private);
    }

    #[test]
    fn route_link_selects_clear_existing_pod_without_new_pod_suggestion() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);

        let routed = tools
            .route_link_to_pods(
                &context,
                RouteLinkRequest {
                    url: "https://example.com/interface-demo".to_string(),
                    title: Some("Interface Demo".to_string()),
                    summary: Some("Practical interface design artifact.".to_string()),
                    tags: vec!["interfaces".to_string(), "design".to_string()],
                },
                2.5,
            )
            .unwrap();

        assert!(!routed.needs_confirmation);
        assert_eq!(
            routed.selected.as_ref().map(|pod| pod.pod_slug.as_str()),
            Some("beautiful-interfaces")
        );
        assert!(routed.suggested_new_pod.is_none());
    }

    #[test]
    fn removing_a_submission_unlinks_then_purges_when_orphaned() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        add_tools_for_thought(&mut store);
        let tools = AgentTools::new(store.clone());
        let ctx = ctx(&store);

        let req = || SubmitLinkRequest {
            url: "https://example.com/move-me".to_string(),
            title: Some("Move Me".to_string()),
            description: None,
            note: None,
            tags: vec![],
            discovered_by_crawler: false,
        };

        // Same URL submitted to two pods => one submission, two pod links.
        let submission = tools
            .submit_link_to_pod(&ctx, "beautiful-interfaces", req())
            .unwrap();
        tools
            .submit_link_to_pod(&ctx, "tools-for-thought", req())
            .unwrap();
        {
            let guard = tools.store();
            let s = guard.read().unwrap();
            assert!(s.submissions.contains_key(&submission.id));
            assert_eq!(
                s.submission_pods
                    .iter()
                    .filter(|link| link.submission_id == submission.id)
                    .count(),
                2
            );
        }

        // Remove from one pod: still linked elsewhere, so not purged.
        assert!(!tools
            .remove_submission_from_pod_for_test(&ctx, "beautiful-interfaces", submission.id)
            .unwrap());
        // Removing the same link again is a NotFound (it is already gone).
        assert!(tools
            .remove_submission_from_pod_for_test(&ctx, "beautiful-interfaces", submission.id)
            .is_err());

        // Remove from the last pod: now orphaned, so the submission is purged.
        assert!(tools
            .remove_submission_from_pod_for_test(&ctx, "tools-for-thought", submission.id)
            .unwrap());
        {
            let guard = tools.store();
            let s = guard.read().unwrap();
            assert!(!s.submissions.contains_key(&submission.id));
            assert!(s
                .submission_pods
                .iter()
                .all(|link| link.submission_id != submission.id));
        }
    }

    #[test]
    fn node_identity_creation_and_event_signing_verify() {
        let node = create_node_identity("test node", None);
        let event =
            sign_public_event(&node, "pod_created", "test", json!({"ok": true}), None).unwrap();
        assert!(verify_event(&event, &node.public_key).unwrap());
    }

    #[test]
    fn tenant_creation_and_token_auth_hashes_plaintext() {
        let tools = AgentTools::new(seed_store());
        let tenant = tools
            .create_tenant(CreateTenantRequest {
                name: "Acme".to_string(),
                slug: "acme".to_string(),
            })
            .unwrap();
        let token = tools
            .create_dev_token(DevTokenRequest {
                user_id: None,
                tenant_slug: Some("acme".to_string()),
                label: "agent".to_string(),
            })
            .unwrap();
        assert_eq!(token.tenant_id, Some(tenant.id));
        assert_ne!(token.token, token.token_hash);
        assert!(tools.authenticate_token(&token.token).unwrap().is_some());
    }

    #[test]
    fn duplicate_event_rejection_and_trusted_peer_rules() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        let peer_node = create_node_identity("peer", None);
        let peer_id = Uuid::now_v7();
        store.trusted_peers.insert(
            peer_id,
            TrustedPeer {
                id: peer_id,
                node_id: peer_node.id,
                tenant_id: None,
                display_name: "peer".to_string(),
                base_url: "https://peer.example".to_string(),
                public_key: peer_node.public_key.clone(),
                trust_level: TrustLevel::ReadOnly,
                enabled: true,
                created_at: Utc::now(),
            },
        );
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let submission = Submission {
            id: Uuid::now_v7(),
            tenant_id: None,
            url: "https://x.test".to_string(),
            canonical_url: "https://x.test/".to_string(),
            title: "Trusted peer link".to_string(),
            source_metadata: CandidateSourceMetadata::default(),
            description: None,
            domain: "x.test".to_string(),
            submitted_by: None,
            discovered_by_crawler: false,
            submitter_note: None,
            summary: None,
            provenance: Vec::new(),
            media_references: Vec::new(),
            tags: vec![],
            embedding: None,
            created_at: Utc::now(),
            origin_event_id: None,
        };
        let event = sign_public_event(
            &peer_node,
            "link_submitted",
            "trusted-peer-pod",
            json!({"submission": submission}),
            None,
        )
        .unwrap();
        assert_eq!(
            tools
                .import_pod_events(&context, peer_id, vec![event.clone()])
                .unwrap(),
            1
        );
        assert_eq!(
            tools
                .import_pod_events(&context, peer_id, vec![event])
                .unwrap(),
            0
        );
        assert!(tools
            .import_pod_events(&context, Uuid::now_v7(), vec![])
            .is_err());
    }

    #[test]
    fn pod_creation_default_skill_pack_and_fork() {
        let store = seed_store();
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let pod = tools
            .create_pod(
                &context,
                CreatePodRequest {
                    name: "Research Toys".to_string(),
                    slug: "research-toys".to_string(),
                    description: "Small research artifacts.".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        let pack = tools.get_skill_pack(&context, &pod.slug).unwrap();
        assert!(pack.skill_md.contains("Research Toys"));
        let forked = tools
            .fork_skill_pack(
                &context,
                &pod.slug,
                CreatePodRequest {
                    name: "Research Toys Fork".to_string(),
                    slug: "research-toys-fork".to_string(),
                    description: "Fork.".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        assert_ne!(pack.id, forked.id);
        let exported = tools
            .export_skill_pack(&context, "research-toys-fork")
            .unwrap();
        let imported = tools
            .import_skill_pack(&context, "research-toys-fork", exported.files)
            .unwrap();
        assert_eq!(imported.version, 3);
    }

    #[test]
    fn skill_pack_import_export_validation_and_private_data_not_exported() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let export = tools
            .export_skill_pack(&context, "beautiful-interfaces")
            .unwrap();
        assert!(export.files.contains_key("events.jsonl"));
        assert!(!export.files["events.jsonl"].contains("private_note_added"));
        let report = tools
            .validate_pod_skill_pack(&context, "beautiful-interfaces")
            .unwrap();
        assert!(report.valid);
        let public_error = tools
            .import_skill_pack(&context, "beautiful-interfaces", export.files.clone())
            .unwrap_err();
        assert!(public_error
            .to_string()
            .contains("Pending Proposal approval"));
        let package = pod_package_contents_from_files(&export.files).unwrap();
        tools
            .create_private_pod_with_package(
                &context,
                CreatePrivatePodWithPackageRequest {
                    name: "Portable private package".to_string(),
                    slug: "portable-private-package".to_string(),
                    description: "Import/export acceptance".to_string(),
                    package,
                },
            )
            .unwrap();
        let private_export = tools
            .export_skill_pack(&context, "portable-private-package")
            .unwrap();
        let imported = tools
            .import_skill_pack(&context, "portable-private-package", private_export.files)
            .unwrap();
        assert!(imported.version > 1);
    }

    #[test]
    fn link_submission_and_canonical_dedupe() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let first = tools
            .submit_link_to_pod(
                &context,
                "beautiful-interfaces",
                SubmitLinkRequest {
                    url: "https://example.com/demo?utm_source=x&b=2&a=1#frag".to_string(),
                    title: Some("Demo".to_string()),
                    description: None,
                    note: None,
                    tags: vec![],
                    discovered_by_crawler: false,
                },
            )
            .unwrap();
        let second = tools
            .submit_link_to_pod(
                &context,
                "beautiful-interfaces",
                SubmitLinkRequest {
                    url: "https://example.com/demo?a=1&b=2".to_string(),
                    title: Some("Demo duplicate".to_string()),
                    description: None,
                    note: None,
                    tags: vec![],
                    discovered_by_crawler: false,
                },
            )
            .unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(
            canonicalize_url("https://example.com/demo?utm_campaign=y").unwrap(),
            "https://example.com/demo"
        );
    }

    #[test]
    fn blocked_source_topic_and_negative_signal_filtering() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        add_test_submission(
            &mut store,
            "beautiful-interfaces",
            "No Artifact Launch",
            "https://example.com/no-artifact-launch",
            "product launch without artifact generic AI hype",
        );
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        tools
            .block_source(&context, "example.com".to_string())
            .unwrap();
        let items = tools
            .discover_in_pod(
                &context,
                "beautiful-interfaces",
                DiscoverRequest {
                    query: "generic AI hype".to_string(),
                    avoid: vec!["generic AI hype".to_string()],
                    limit: 10,
                    mode: DiscoveryMode::DeepMatch,
                    user_id: context.user_id,
                },
            )
            .unwrap();
        assert!(items.iter().all(|item| item.source != "example.com"));
        assert!(items.iter().all(|item| !item.title.contains("No Artifact")));
    }

    #[test]
    fn discovery_ranking_stumble_and_brief_generation() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        add_test_submission(
            &mut store,
            "beautiful-interfaces",
            "Magic Ink",
            "https://worrydream.com/MagicInk/",
            "visual artifact implementation detail interface",
        );
        add_test_submission(
            &mut store,
            "beautiful-interfaces",
            "Dynamicland",
            "https://dynamicland.org/",
            "spatial interface working demo independent research",
        );
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let items = tools
            .discover_in_pod(
                &context,
                "beautiful-interfaces",
                DiscoverRequest {
                    query: "weird practical UI inspiration".to_string(),
                    avoid: vec!["politics".to_string()],
                    limit: 7,
                    mode: DiscoveryMode::Stumble,
                    user_id: context.user_id,
                },
            )
            .unwrap();
        assert!(!items.is_empty());
        assert!(items
            .iter()
            .all(|item| item.recommendation_explanation.final_score > 0.0));
        let brief = tools
            .generate_brief(
                &context,
                GenerateBriefRequest {
                    pod_slugs: vec!["beautiful-interfaces".to_string()],
                    query: Some("interfaces".to_string()),
                    user_id: context.user_id,
                },
            )
            .unwrap();
        assert!(brief.private);
        assert!(!brief.items.is_empty());
    }

    #[test]
    fn crawler_candidate_promotion_does_not_federate_an_unaccepted_submission() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let source = tools
            .add_source_to_pod(
                &context,
                "beautiful-interfaces",
                CrawlerSourceType::Rss,
                "https://example.com/feed.xml".to_string(),
            )
            .unwrap();
        let candidate = tools
            .create_crawl_candidate(
                &context,
                "beautiful-interfaces",
                source.id,
                SubmitLinkRequest {
                    url: "https://example.org/candidate".to_string(),
                    title: Some("Candidate".to_string()),
                    description: None,
                    note: None,
                    tags: vec!["working".to_string(), "demo".to_string()],
                    discovered_by_crawler: true,
                },
            )
            .unwrap();
        let submission = tools
            .promote_crawl_candidate(&context, candidate.id)
            .unwrap();
        assert!(submission.discovered_by_crawler);
        let events = tools
            .export_pod_events(&context, "beautiful-interfaces")
            .unwrap();
        assert!(!events
            .iter()
            .any(|event| event.event_type == "link_submitted"));
    }

    #[test]
    fn representative_image_asset_storage_dedupes() {
        let mut store = seed_store();
        add_beautiful_interfaces(&mut store);
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let submission = tools
            .submit_link_to_pod(
                &context,
                "beautiful-interfaces",
                SubmitLinkRequest {
                    url: "https://example.com/design-reference".to_string(),
                    title: Some("Design Reference".to_string()),
                    description: None,
                    note: None,
                    tags: vec!["design".to_string()],
                    discovered_by_crawler: false,
                },
            )
            .unwrap();
        let request = RepresentativeImageRequest {
            source: SubmissionAssetSource::PageImage,
            url: Some("https://example.com/image.png".to_string()),
            local_path: None,
            mime_type: Some("image/png".to_string()),
            alt_text: Some("Representative image".to_string()),
        };
        let first = tools
            .add_submission_asset(&context, submission.id, request.clone())
            .unwrap();
        let second = tools
            .add_submission_asset(&context, submission.id, request)
            .unwrap();
        assert_eq!(first.id, second.id);
        let assets = tools
            .assets_for_submission(&context, submission.id)
            .unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(
            assets[0].asset_type,
            SubmissionAssetType::RepresentativeImage
        );
    }

    #[test]
    fn imported_peer_events_materialize_remote_pod_links_for_briefs() {
        let mut store = seed_store();
        let remote_node = create_node_identity("remote alien node", None);
        let peer_id = Uuid::now_v7();
        store.trusted_peers.insert(
            peer_id,
            TrustedPeer {
                id: peer_id,
                node_id: remote_node.id,
                tenant_id: None,
                display_name: "Remote Alien Node".to_string(),
                base_url: "https://remote-alien-node.example".to_string(),
                public_key: remote_node.public_key.clone(),
                trust_level: TrustLevel::ReadOnly,
                enabled: true,
                created_at: Utc::now(),
            },
        );
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);

        let remote_pod = Pod {
            id: Uuid::now_v7(),
            tenant_id: None,
            name: "Remote Aliens".to_string(),
            slug: "remote-aliens".to_string(),
            description: "Remote public pod about aliens, UAP, and signals.".to_string(),
            visibility: Visibility::Public,
            created_by: None,
            created_at: Utc::now(),
            origin_node_id: Some(remote_node.id),
        };
        let created_event = sign_public_event(
            &remote_node,
            "pod_created",
            &remote_pod.slug,
            json!({
                "pod": remote_pod.clone(),
                "package": default_skill_pack(&remote_pod),
            }),
            None,
        )
        .unwrap();
        let remote_submission = Submission {
            id: Uuid::now_v7(),
            tenant_id: None,
            url: "https://x.com/example/status/42".to_string(),
            canonical_url: canonicalize_url("https://x.com/example/status/42?s=20").unwrap(),
            title: "Remote Alien Signal Thread".to_string(),
            source_metadata: CandidateSourceMetadata::default(),
            description: Some("Alien and UAP signal discussion.".to_string()),
            domain: "x.com".to_string(),
            submitted_by: None,
            discovered_by_crawler: false,
            submitter_note: Some("Remote public alien pod link.".to_string()),
            summary: Some("Alien and UAP signal discussion.".to_string()),
            provenance: Vec::new(),
            media_references: Vec::new(),
            tags: vec![
                "aliens".to_string(),
                "uap".to_string(),
                "signals".to_string(),
            ],
            embedding: None,
            created_at: Utc::now(),
            origin_event_id: None,
        };
        let submitted_event = sign_public_event(
            &remote_node,
            "link_submitted",
            &remote_pod.slug,
            json!({"submission": remote_submission.clone()}),
            Some(created_event.content_hash.clone()),
        )
        .unwrap();

        let imported = tools
            .import_pod_events(&context, peer_id, vec![created_event, submitted_event])
            .unwrap();
        assert_eq!(imported, 2);

        tools.join_pod(&context, "remote-aliens").unwrap();
        let discoveries = tools
            .discover_in_pod(
                &context,
                "remote-aliens",
                DiscoverRequest {
                    query: "aliens uap signals".to_string(),
                    avoid: vec![],
                    limit: 7,
                    mode: DiscoveryMode::DeepMatch,
                    user_id: context.user_id,
                },
            )
            .unwrap();
        assert!(discoveries
            .iter()
            .any(|item| item.url == "https://x.com/example/status/42"));

        let brief = tools
            .generate_brief(
                &context,
                GenerateBriefRequest {
                    pod_slugs: vec!["remote-aliens".to_string()],
                    query: Some("aliens uap signals".to_string()),
                    user_id: context.user_id,
                },
            )
            .unwrap();
        assert!(brief
            .items
            .iter()
            .any(|item| item.url == "https://x.com/example/status/42"));
    }

    #[test]
    fn brief_suppresses_fresh_own_links_until_they_are_stale() {
        let store = seed_store();
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        tools
            .create_pod_for_test(
                &context,
                CreatePodRequest {
                    name: "Agent Aliens".to_string(),
                    slug: "agent-aliens".to_string(),
                    description: "Alien links submitted by this agent.".to_string(),
                    visibility: Visibility::Public,
                },
            )
            .unwrap();
        let own_submission = tools
            .submit_link_to_pod(
                &context,
                "agent-aliens",
                SubmitLinkRequest {
                    url: "https://x.com/agent/status/1".to_string(),
                    title: Some("Agent Alien Link".to_string()),
                    description: Some("Aliens and UAP signal trail.".to_string()),
                    note: Some("Submitted by this agent.".to_string()),
                    tags: vec!["aliens".to_string(), "uap".to_string()],
                    discovered_by_crawler: false,
                },
            )
            .unwrap();

        let brief = tools
            .generate_brief(
                &context,
                GenerateBriefRequest {
                    pod_slugs: vec!["agent-aliens".to_string()],
                    query: Some("aliens uap".to_string()),
                    user_id: context.user_id,
                },
            )
            .unwrap();
        assert!(!brief
            .items
            .iter()
            .any(|item| item.submission_id == own_submission.id));

        {
            let store = tools.store();
            let mut store = store.write().unwrap();
            store
                .submissions
                .get_mut(&own_submission.id)
                .unwrap()
                .created_at = Utc::now() - Duration::days(31);
        }

        let stale_brief = tools
            .generate_brief(
                &context,
                GenerateBriefRequest {
                    pod_slugs: vec!["agent-aliens".to_string()],
                    query: Some("aliens uap".to_string()),
                    user_id: context.user_id,
                },
            )
            .unwrap();
        assert!(stale_brief
            .items
            .iter()
            .any(|item| item.submission_id == own_submission.id));

        let repeated_brief = tools
            .generate_brief(
                &context,
                GenerateBriefRequest {
                    pod_slugs: vec!["agent-aliens".to_string()],
                    query: Some("aliens uap".to_string()),
                    user_id: context.user_id,
                },
            )
            .unwrap();
        assert!(!repeated_brief
            .items
            .iter()
            .any(|item| item.submission_id == own_submission.id));
    }

    #[test]
    fn hosted_tenant_boundary_enforced() {
        let tools = AgentTools::new(seed_store());
        let tenant = tools
            .create_tenant(CreateTenantRequest {
                name: "Tenant A".to_string(),
                slug: "tenant-a".to_string(),
            })
            .unwrap();
        let token = tools
            .create_dev_token(DevTokenRequest {
                user_id: None,
                tenant_slug: Some("tenant-a".to_string()),
                label: "agent".to_string(),
            })
            .unwrap();
        let ctx = AuthContext {
            user_id: Some(token.user_id),
            tenant_id: Some(tenant.id),
            node_id: Uuid::now_v7(),
            harness_id: None,
        };
        let pod = tools
            .create_pod(
                &ctx,
                CreatePodRequest {
                    name: "Private Tenant Pod".to_string(),
                    slug: "private-tenant-pod".to_string(),
                    description: "Tenant scoped.".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        let local_ctx = AuthContext {
            user_id: None,
            tenant_id: None,
            node_id: Uuid::now_v7(),
            harness_id: None,
        };
        assert!(tools.get_skill_pack(&local_ctx, &pod.slug).is_err());
    }
}
