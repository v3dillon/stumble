use crate::agent_tools::AgentTools;
use crate::domain::*;
use crate::signing::{
    create_node_identity, hash_api_token, new_plaintext_api_token, sign_public_event,
};
use crate::skill_pack::{default_skill_pack, pod_request_from_template};
use crate::store::InMemoryStore;
use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

pub fn seed_store() -> InMemoryStore {
    let mut store = InMemoryStore::default();
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
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
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
            tenant_id: Some(hosted_tenant.id),
            display_name: "Hosted Relay Example".to_string(),
            base_url: "https://relay.example".to_string(),
            public_key: peer_two.public_key,
            trust_level: TrustLevel::ReadOnly,
            enabled: true,
            created_at: Utc::now(),
        },
    );

    let pods = [
        (
            "Beautiful Interfaces",
            "beautiful-interfaces",
            "Thoughtful, strange, useful interface design.",
        ),
        (
            "Tools for Thought",
            "tools-for-thought",
            "Durable systems for thinking, writing, memory, and synthesis.",
        ),
        (
            "Agentic Software",
            "agentic-software",
            "Practical agent workflows, interfaces, protocols, and engineering patterns.",
        ),
        (
            "Weird Internet",
            "weird-internet",
            "Odd, humane, surprising corners of the web with artifacts.",
        ),
    ];
    for (name, slug, description) in pods {
        insert_seed_pod(&mut store, &local_node, name, slug, description);
    }

    let user_ids: Vec<_> = store.users.keys().copied().collect();
    let pod_ids: Vec<_> = store
        .pods
        .values()
        .map(|pod| (pod.id, pod.slug.clone()))
        .collect();
    let submissions = vec![
        (
            "Magic Ink",
            "https://worrydream.com/MagicInk/",
            "beautiful-interfaces",
            "visual artifact implementation detail interface",
        ),
        (
            "Dynamicland",
            "https://dynamicland.org/",
            "beautiful-interfaces",
            "spatial interface working demo independent research",
        ),
        (
            "The Humane Representation of Thought",
            "https://worrydream.com/HumaneRepresentationOfThought/",
            "tools-for-thought",
            "tools thinking visual explanation",
        ),
        (
            "Ink and Switch Muse",
            "https://www.inkandswitch.com/muse/",
            "tools-for-thought",
            "tools for thought research artifact",
        ),
        (
            "Local-first Software",
            "https://www.inkandswitch.com/local-first/",
            "agentic-software",
            "software architecture collaboration",
        ),
        (
            "Model Context Protocol",
            "https://modelcontextprotocol.io/",
            "agentic-software",
            "agent protocol integration",
        ),
        (
            "Websim Experiments",
            "https://websim.ai/",
            "weird-internet",
            "weird internet generative artifact",
        ),
        (
            "Naive UI Lab",
            "https://example.com/naive-ui-lab",
            "beautiful-interfaces",
            "unusual interaction pattern",
        ),
        (
            "Agent Inbox Patterns",
            "https://example.com/agent-inbox",
            "agentic-software",
            "agent user interface practical",
        ),
        (
            "Personal Wiki Rituals",
            "https://example.com/personal-wiki",
            "tools-for-thought",
            "notes memory synthesis",
        ),
        (
            "Old Hypertext Garden",
            "https://example.com/old-hypertext-garden",
            "weird-internet",
            "old gem hypertext",
        ),
        (
            "Practical Color Pickers",
            "https://example.com/color-picker",
            "beautiful-interfaces",
            "working demo implementation detail",
        ),
        (
            "Spatial Notes",
            "https://example.com/spatial-notes",
            "tools-for-thought",
            "spatial interface notes",
        ),
        (
            "Agent Runbooks",
            "https://example.com/agent-runbooks",
            "agentic-software",
            "agent workflow runbook",
        ),
        (
            "Tiny Web Toys",
            "https://example.com/tiny-web-toys",
            "weird-internet",
            "playful artifact",
        ),
        (
            "Interface Archaeology",
            "https://example.com/interface-archaeology",
            "beautiful-interfaces",
            "old gem interface",
        ),
        (
            "Composable Memory",
            "https://example.com/composable-memory",
            "tools-for-thought",
            "memory tools architecture",
        ),
        (
            "Protocol Workbench",
            "https://example.com/protocol-workbench",
            "agentic-software",
            "protocol demo",
        ),
        (
            "Strange Search Engines",
            "https://example.com/strange-search",
            "weird-internet",
            "search weird practical",
        ),
        (
            "No Artifact Launch",
            "https://example.com/no-artifact-launch",
            "beautiful-interfaces",
            "product launch without artifact generic AI hype",
        ),
    ];
    for (idx, (title, url, pod_slug, tag_text)) in submissions.into_iter().enumerate() {
        let pod_id = pod_ids.iter().find(|(_, slug)| slug == pod_slug).unwrap().0;
        let id = Uuid::now_v7();
        let parsed = url::Url::parse(url).expect("seed url");
        let submission = Submission {
            id,
            tenant_id: None,
            url: url.to_string(),
            canonical_url: url.to_string(),
            title: title.to_string(),
            description: Some(format!("Seeded item for {pod_slug}.")),
            domain: parsed.domain().unwrap_or("example.com").to_string(),
            submitted_by: if idx % 3 == 0 {
                user_ids.first().copied()
            } else {
                None
            },
            discovered_by_crawler: idx % 3 != 0,
            submitter_note: if idx % 3 == 0 {
                Some("Human note: worth reviewing for practical inspiration.".to_string())
            } else {
                None
            },
            summary: Some(format!("{title} is a seeded discovery item.")),
            tags: tag_text
                .split_whitespace()
                .map(ToString::to_string)
                .collect(),
            embedding: None,
            created_at: Utc::now() - Duration::days((idx * 31) as i64),
            origin_event_id: None,
        };
        store.submissions.insert(id, submission.clone());
        store.submission_pods.push(SubmissionPod {
            submission_id: id,
            pod_id,
            created_at: Utc::now(),
        });
        if let Ok(event) = sign_public_event(
            &local_node,
            "link_submitted",
            pod_slug,
            json!({"submission_id": id, "title": title, "url": url}),
            store.latest_event_hash(pod_slug),
        ) {
            store.event_log.push(event);
        }
    }

    for (_, slug) in &pod_ids {
        if let Some(pod) = store.pods.values().find(|pod| &pod.slug == slug) {
            let source_id = Uuid::now_v7();
            store.crawler_sources.insert(
                source_id,
                CrawlerSource {
                    id: source_id,
                    tenant_id: None,
                    pod_id: pod.id,
                    source_type: CrawlerSourceType::Rss,
                    url: format!("https://example.com/{slug}/feed.xml"),
                    enabled: true,
                    crawl_interval_minutes: 1440,
                    last_crawled_at: None,
                    origin_event_id: None,
                },
            );
        }
    }
    if let Some(user_id) = user_ids.first().copied() {
        let bad = store
            .submissions
            .values()
            .find(|submission| submission.title == "No Artifact Launch")
            .map(|submission| submission.id);
        if let Some(submission_id) = bad {
            store.feedback_events.push(FeedbackEvent {
                user_id,
                tenant_id: None,
                submission_id,
                event_type: FeedbackKind::Dismissed,
                reason: Some("Generic AI hype".to_string()),
                created_at: Utc::now(),
                local_only: true,
            });
        }
    }

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
            },
        );
    }

    store
}

pub fn seed_agent_tools() -> AgentTools {
    AgentTools::new(seed_store())
}

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
    store
        .pod_skill_packs
        .insert(pod.id, default_skill_pack(&pod));
    if let Ok(event) = sign_public_event(
        node,
        "pod_created",
        &pod.slug,
        json!({"slug": pod.slug, "name": pod.name}),
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

    fn ctx(store: &InMemoryStore) -> AuthContext {
        AuthContext {
            user_id: store.users.keys().next().copied(),
            tenant_id: None,
            node_id: store.default_node().unwrap().id,
        }
    }

    #[test]
    fn private_pods_are_hidden_from_the_federation_surface() {
        let store = seed_store();
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
        let public = tools.list_public_pods(ctx.tenant_id).unwrap();
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
        let store = seed_store();
        let tools = AgentTools::new(store.clone());
        let ctx = ctx(&store);

        // A private pod whose name and description strongly match the query topics.
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

        let discovery = tools
            .discover_public_pods_for_home(
                &ctx,
                vec!["interface".to_string(), "design".to_string()],
                25,
            )
            .unwrap();
        let slugs: Vec<&str> = discovery
            .local_public_pods
            .iter()
            .map(|candidate| candidate.pod_slug.as_str())
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
    fn removing_a_submission_unlinks_then_purges_when_orphaned() {
        let store = seed_store();
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
            .remove_submission_from_pod(&ctx, "beautiful-interfaces", submission.id)
            .unwrap());
        // Removing the same link again is a NotFound (it is already gone).
        assert!(tools
            .remove_submission_from_pod(&ctx, "beautiful-interfaces", submission.id)
            .is_err());

        // Remove from the last pod: now orphaned, so the submission is purged.
        assert!(tools
            .remove_submission_from_pod(&ctx, "tools-for-thought", submission.id)
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
        let peer_node = create_node_identity("peer", None);
        let peer_id = Uuid::now_v7();
        store.trusted_peers.insert(
            peer_id,
            TrustedPeer {
                id: peer_id,
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
        let event = sign_public_event(
            &peer_node,
            "link_submitted",
            "beautiful-interfaces",
            json!({"url":"https://x.test"}),
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
                    visibility: Visibility::Public,
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
                    visibility: Visibility::Public,
                },
            )
            .unwrap();
        assert_ne!(pack.id, forked.id);
    }

    #[test]
    fn skill_pack_import_export_validation_and_private_data_not_exported() {
        let store = seed_store();
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
        let imported = tools
            .import_skill_pack(&context, "beautiful-interfaces", export.files)
            .unwrap();
        assert!(imported.version > 1);
    }

    #[test]
    fn link_submission_and_canonical_dedupe() {
        let store = seed_store();
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
        let store = seed_store();
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
        let store = seed_store();
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
    fn crawler_candidate_promotion_creates_public_event() {
        let store = seed_store();
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
        assert!(events
            .iter()
            .any(|event| event.event_type == "link_submitted"));
    }

    #[test]
    fn representative_image_asset_storage_dedupes() {
        let store = seed_store();
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
    fn hub_register_search_and_home_discovery_do_not_export_private_interests() {
        let store = seed_store();
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let node = create_node_identity("public alien node", None);
        tools
            .register_hub_node(HubRegisterNodeRequest {
                node_id: node.id,
                display_name: "Public Alien Node".to_string(),
                base_url: "https://alien-node.example".to_string(),
                public_key: node.public_key.clone(),
                protocol_version: "stumble/0.1".to_string(),
            })
            .unwrap();
        tools
            .register_hub_pod(HubRegisterPodRequest {
                node_id: node.id,
                node_base_url: "https://alien-node.example".to_string(),
                pod_slug: "public-uap-research".to_string(),
                pod_name: "Public UAP Research".to_string(),
                description: "Public links about aliens, UAP, and signals.".to_string(),
                tags: vec![
                    "aliens".to_string(),
                    "uap".to_string(),
                    "signals".to_string(),
                ],
                skill_pack_version: 1,
                latest_event_hash: None,
                manifest_url:
                    "https://alien-node.example/federation/pods/public-uap-research/manifest"
                        .to_string(),
                events_url: "https://alien-node.example/federation/pods/public-uap-research/events"
                    .to_string(),
            })
            .unwrap();
        let search = tools.search_hub_pods("aliens signals", 10).unwrap();
        assert_eq!(search.results[0].pod.pod_slug, "public-uap-research");
        let discovery = tools
            .discover_public_pods_for_home(
                &context,
                vec!["aliens".to_string(), "signals".to_string()],
                10,
            )
            .unwrap();
        assert!(!discovery.private_interests_exported);
        assert!(discovery
            .hub_results
            .iter()
            .any(|result| result.pod.pod_slug == "public-uap-research"));
    }

    #[test]
    fn local_public_pods_are_automatically_indexed_for_hub_discovery() {
        let store = seed_store();
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let public = tools
            .create_pod(
                &context,
                CreatePodRequest {
                    name: "Aliens".to_string(),
                    slug: "aliens".to_string(),
                    description: "Public research about aliens, UAP, and signals.".to_string(),
                    visibility: Visibility::Public,
                },
            )
            .unwrap();
        let private = tools
            .create_pod(
                &context,
                CreatePodRequest {
                    name: "Private Aliens".to_string(),
                    slug: "private-aliens".to_string(),
                    description: "Private notes about aliens.".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();

        let before = tools.search_hub_pods("aliens", 10).unwrap();
        assert!(!before
            .results
            .iter()
            .any(|result| result.pod.pod_slug == public.slug));

        let indexed = tools
            .index_local_public_pods_in_hub(&context, "http://127.0.0.1:8787")
            .unwrap();
        assert!(indexed.iter().any(|pod| pod.pod_slug == public.slug));
        assert!(!indexed.iter().any(|pod| pod.pod_slug == private.slug));

        let search = tools.search_hub_pods("aliens", 10).unwrap();
        assert!(search
            .results
            .iter()
            .any(|result| result.pod.pod_slug == public.slug));
        assert!(!search
            .results
            .iter()
            .any(|result| result.pod.pod_slug == private.slug));
    }

    #[test]
    fn discovery_feed_splits_local_public_pods_from_global_hub_pods() {
        let store = seed_store();
        let tools = AgentTools::new(store.clone());
        let context = ctx(&store);
        let local_public = tools
            .create_pod(
                &context,
                CreatePodRequest {
                    name: "Aliens".to_string(),
                    slug: "aliens".to_string(),
                    description: "Public research about aliens and UAP.".to_string(),
                    visibility: Visibility::Public,
                },
            )
            .unwrap();
        let local_private = tools
            .create_pod(
                &context,
                CreatePodRequest {
                    name: "Private Aliens".to_string(),
                    slug: "private-aliens".to_string(),
                    description: "Private notes about aliens.".to_string(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        let remote_node = create_node_identity("remote public node", None);
        tools
            .register_hub_node(HubRegisterNodeRequest {
                node_id: remote_node.id,
                display_name: "Remote Public Node".to_string(),
                base_url: "https://remote-node.example".to_string(),
                public_key: remote_node.public_key,
                protocol_version: "stumble/0.1".to_string(),
            })
            .unwrap();
        tools
            .register_hub_pod(HubRegisterPodRequest {
                node_id: remote_node.id,
                node_base_url: "https://remote-node.example".to_string(),
                pod_slug: "remote-aliens".to_string(),
                pod_name: "Remote Aliens".to_string(),
                description: "Remote public pod about aliens and signals.".to_string(),
                tags: vec!["aliens".to_string(), "signals".to_string()],
                skill_pack_version: 1,
                latest_event_hash: None,
                manifest_url: "https://remote-node.example/federation/pods/remote-aliens/manifest"
                    .to_string(),
                events_url: "https://remote-node.example/federation/pods/remote-aliens/events"
                    .to_string(),
            })
            .unwrap();

        let feed = tools
            .pod_discovery_feed(&context, "http://127.0.0.1:8787", "aliens", 10)
            .unwrap();
        assert!(!feed.private_interests_exported);
        assert!(feed
            .local_public_pods
            .iter()
            .any(|item| item.pod.pod_slug == local_public.slug));
        assert!(feed
            .global_public_pods
            .iter()
            .any(|item| item.pod.pod_slug == "remote-aliens"));
        assert!(!feed
            .local_public_pods
            .iter()
            .chain(feed.global_public_pods.iter())
            .any(|item| item.pod.pod_slug == local_private.slug));
    }

    #[test]
    fn hub_registration_rejects_public_http_urls() {
        let tools = AgentTools::new(seed_store());
        let node = create_node_identity("public node", None);
        let error = tools
            .register_hub_node(HubRegisterNodeRequest {
                node_id: node.id,
                display_name: "Public Node".to_string(),
                base_url: "http://public-node.example".to_string(),
                public_key: node.public_key,
                protocol_version: "stumble/0.1".to_string(),
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("base_url must use https unless it is loopback-only"));
    }

    #[test]
    fn hub_registration_allows_loopback_http_for_local_hubs() {
        let tools = AgentTools::new(seed_store());
        let node = create_node_identity("local node", None);
        tools
            .register_hub_node(HubRegisterNodeRequest {
                node_id: node.id,
                display_name: "Local Node".to_string(),
                base_url: "http://127.0.0.1:8787".to_string(),
                public_key: node.public_key,
                protocol_version: "stumble/0.1".to_string(),
            })
            .unwrap();
        let pod = tools
            .register_hub_pod(HubRegisterPodRequest {
                node_id: node.id,
                node_base_url: "http://127.0.0.1:8787".to_string(),
                pod_slug: "local-public".to_string(),
                pod_name: "Local Public".to_string(),
                description: "Local public metadata.".to_string(),
                tags: vec!["local".to_string()],
                skill_pack_version: 1,
                latest_event_hash: None,
                manifest_url: "http://127.0.0.1:8787/federation/pods/local-public/manifest"
                    .to_string(),
                events_url: "http://127.0.0.1:8787/federation/pods/local-public/events".to_string(),
            })
            .unwrap();
        assert_eq!(pod.node_base_url, "http://127.0.0.1:8787");
    }

    #[test]
    fn hub_pod_registration_requires_endpoint_origin_to_match_node_base() {
        let tools = AgentTools::new(seed_store());
        let node = create_node_identity("public node", None);
        tools
            .register_hub_node(HubRegisterNodeRequest {
                node_id: node.id,
                display_name: "Public Node".to_string(),
                base_url: "https://public-node.example".to_string(),
                public_key: node.public_key,
                protocol_version: "stumble/0.1".to_string(),
            })
            .unwrap();
        let error = tools
            .register_hub_pod(HubRegisterPodRequest {
                node_id: node.id,
                node_base_url: "https://public-node.example".to_string(),
                pod_slug: "public-pod".to_string(),
                pod_name: "Public Pod".to_string(),
                description: "Public metadata.".to_string(),
                tags: vec!["public".to_string()],
                skill_pack_version: 1,
                latest_event_hash: None,
                manifest_url: "https://other.example/federation/pods/public-pod/manifest"
                    .to_string(),
                events_url: "https://public-node.example/federation/pods/public-pod/events"
                    .to_string(),
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("manifest_url must use the same scheme, host, and port"));
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
        };
        assert!(tools.get_skill_pack(&local_ctx, &pod.slug).is_err());
    }
}
