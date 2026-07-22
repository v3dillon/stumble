//! Replaceable private Index search acceptance (ticket 05).
//!
//! Covers Index capability search, explicit Explore-only remote queries,
//! local re-rank, multi-Index replacement, typed failures, privacy, and
//! provenance persistence.

use chrono::{TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-index-search-{label}-{}",
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
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

fn approve_trust_policy_change(tools: &AgentTools, change: TrustPolicyChange) {
    let proposer = harness(
        tools,
        "trust policy proposer",
        vec![HarnessCapability::Administration],
    );
    let approver = harness(
        tools,
        "trust policy approver",
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
fn index_capable_node_searches_catalog_without_user_or_analytics() {
    let origin_dir = TestDataDir::new("origin");
    let index_dir = TestDataDir::new("index");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true)
        .with_bootstrap_capability(true, std::sync::Arc::new(UnreachableOriginProbe));
    assert!(index.index_enabled());
    assert!(index.bootstrap_enabled());

    let pod = create_public_pod(&origin, "rust-systems", "Rust ownership systems");
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/rust-systems",
        )
        .unwrap();
    index.index_pod_announcement(announcement).unwrap();

    let response = index.search_pod_announcements("rust", 10).unwrap();
    assert_eq!(response.results.len(), 1);
    assert!(response.results[0].announcement.verify().unwrap());
    let wire = serde_json::to_value(&response).unwrap();
    assert!(wire.get("user_id").is_none());
    assert!(wire.get("global_quality_score").is_none());
    assert!(wire.get("authority").is_none());
    assert!(wire.get("popularity").is_none());
    assert!(wire.get("trust").is_none());

    // Rate bookkeeping timestamps only — no query text retained as analytics.
    let binding = index.store();
    let store = binding.read().unwrap();
    let runtime = store.index_runtime.as_ref().expect("index runtime");
    assert!(!runtime.recent_search_attempts.is_empty());
    let serialized = serde_json::to_string(runtime).unwrap();
    assert!(!serialized.contains("rust"));
}

#[test]
fn explicit_explore_queries_indexes_local_rerank_and_replacement() {
    let origin_dir = TestDataDir::new("explore-origin");
    let home_dir = TestDataDir::new("explore-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
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

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "first".into(),
            base_url: "https://first-index.example".into(),
        },
    );
    let reader = harness(&home, "explore reader", vec![HarnessCapability::FeedRead]);

    let client = ScriptedIndexSearchClient::new();
    client.push_response(
        "https://first-index.example",
        PodAnnouncementSearchResponse::new(
            "rust",
            vec![PodAnnouncementSearchResult::new(
                announcement.clone(),
                0.001, // remote low score must not control local order
                vec!["remote only".into()],
            )],
        ),
    );

    let explored = home
        .explore_public_pods_with_indexes(
            &reader,
            ExploreRequest::new("rust", 10, 0).unwrap(),
            &client,
        )
        .unwrap();
    assert_eq!(explored.results.len(), 1);
    assert!(explored.results[0].relevance > 0.001);
    assert!(!explored.results[0].reasons.is_empty());

    let known = {
        let binding = home.store();
        let store = binding.read().unwrap();
        store
            .known_pod_announcements
            .get(&(announcement.origin_node_id, announcement.pod_slug.clone()))
            .cloned()
            .unwrap()
    };
    assert!(known
        .received_from_index_urls
        .contains("https://first-index.example"));

    let captured = client.captured.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].1.query, "rust");
    assert!(index_request_is_public_only(&captured[0].1));
    drop(captured);

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::RemoveIndexNode {
            base_url: "https://first-index.example".into(),
        },
    );
    assert!(home
        .explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
        .unwrap()
        .results
        .is_empty());

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "replacement".into(),
            base_url: "https://replacement-index.example".into(),
        },
    );
    client.push_response(
        "https://replacement-index.example",
        PodAnnouncementSearchResponse::new(
            "rust",
            vec![PodAnnouncementSearchResult::new(announcement, 0.5, vec![])],
        ),
    );
    let restored = home
        .explore_public_pods_with_indexes(
            &reader,
            ExploreRequest::new("rust", 10, 0).unwrap(),
            &client,
        )
        .unwrap();
    assert_eq!(restored.results.len(), 1);
}

#[test]
fn empty_query_explore_never_contacts_index() {
    let home_dir = TestDataDir::new("empty-query-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "index".into(),
            base_url: "https://index.example".into(),
        },
    );
    let reader = harness(&home, "reader", vec![HarnessCapability::FeedRead]);
    let client = ScriptedIndexSearchClient::new();
    home.explore_public_pods_with_indexes(
        &reader,
        ExploreRequest::new("", 10, 0).unwrap(),
        &client,
    )
    .unwrap();
    assert!(
        client.captured.lock().unwrap().is_empty(),
        "empty Explore must not send remote Index queries"
    );
}

#[test]
fn personal_discovery_never_receives_index_client() {
    let home_dir = TestDataDir::new("pd-no-index");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = harness(
        &home,
        "pd manager",
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::Feedback,
        ],
    );
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["distributed systems".into()]);
    home.update_taste_profile(&manager, taste).unwrap();
    // Planning succeeds without any Index client — no remote query path.
    let ready = home.personal_discovery_readiness(&manager).unwrap();
    assert!(ready.ready);
}

#[test]
fn typed_failures_for_disabled_oversized_malformed() {
    let index_dir = TestDataDir::new("typed-failures");
    let disabled = AgentTools::open_home_node(&index_dir.0, seed_store).unwrap();
    let err = disabled.search_pod_announcements("rust", 10).unwrap_err();
    match err {
        AgentToolsError::IndexSearch(failure) => {
            assert_eq!(failure.kind, IndexSearchFailureKind::IndexDisabled);
            assert_eq!(failure.kind.as_code(), "index_disabled");
        }
        other => panic!("unexpected {other:?}"),
    }

    let enabled = AgentTools::open_home_node(&TestDataDir::new("typed-enabled").0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let oversized = enabled
        .search_pod_announcements_at(
            &IndexSearchRequest::new("x".repeat(MAX_INDEX_QUERY_BYTES + 1), Some(10)),
            Utc::now(),
        )
        .unwrap_err();
    match oversized {
        AgentToolsError::IndexSearch(failure) => {
            assert_eq!(failure.kind, IndexSearchFailureKind::QueryTooLarge);
        }
        other => panic!("unexpected {other:?}"),
    }

    let malformed = enabled
        .search_pod_announcements_at(&IndexSearchRequest::new("rust", Some(0)), Utc::now())
        .unwrap_err();
    match malformed {
        AgentToolsError::IndexSearch(failure) => {
            assert_eq!(failure.kind, IndexSearchFailureKind::Malformed);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn index_search_provenance_survives_sqlite_restart() {
    let home_dir = TestDataDir::new("provenance-restart");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let origin_dir = TestDataDir::new("provenance-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "persist-systems", "Persisted systems pod");
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/persist-systems",
        )
        .unwrap();

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "persist".into(),
            base_url: "https://persist-index.example".into(),
        },
    );
    let reader = harness(&home, "reader", vec![HarnessCapability::FeedRead]);
    home.accept_index_search_results(
        &reader,
        "https://persist-index.example",
        PodAnnouncementSearchResponse::new(
            "persist",
            vec![PodAnnouncementSearchResult::new(
                announcement.clone(),
                1.0,
                vec![],
            )],
        ),
    )
    .unwrap();

    let index_dir = TestDataDir::new("index-runtime-persist");
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    index.index_pod_announcement(announcement.clone()).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap();
    index
        .search_pod_announcements_at(&IndexSearchRequest::new("persist", Some(5)), now)
        .unwrap();
    drop(index);

    let reopened_index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    {
        let binding = reopened_index.store();
        let store = binding.read().unwrap();
        assert!(store
            .index_runtime
            .as_ref()
            .is_some_and(|runtime| !runtime.recent_search_attempts.is_empty()));
    }

    drop(home);
    let reopened_home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let binding = reopened_home.store();
    let store = binding.read().unwrap();
    let known = store
        .known_pod_announcements
        .get(&(announcement.origin_node_id, announcement.pod_slug.clone()))
        .unwrap();
    assert!(known
        .received_from_index_urls
        .contains("https://persist-index.example"));
}

#[test]
fn multi_index_fallthrough_and_independent_copies() {
    let home_dir = TestDataDir::new("multi-index-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let origin_dir = TestDataDir::new("multi-index-origin");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let sole_pod = create_public_pod(&origin, "sole-pod", "Only on index B");
    let shared_pod = create_public_pod(&origin, "shared-pod", "On both indexes");
    let sole = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &sole_pod.slug,
            "https://origin.example/federation/pods/sole-pod",
        )
        .unwrap();
    let shared = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &shared_pod.slug,
            "https://origin.example/federation/pods/shared-pod",
        )
        .unwrap();

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "a".into(),
            base_url: "https://index-a.example".into(),
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::AddIndexNode {
            label: "b".into(),
            base_url: "https://index-b.example".into(),
        },
    );
    let reader = harness(&home, "reader", vec![HarnessCapability::FeedRead]);
    let client = ScriptedIndexSearchClient::new();
    // A returns shared only; B returns sole + shared. Shared accumulates both URLs.
    client.push_response(
        "https://index-a.example",
        PodAnnouncementSearchResponse::new(
            "pod",
            vec![PodAnnouncementSearchResult::new(
                shared.clone(),
                1.0,
                vec![],
            )],
        ),
    );
    client.push_response(
        "https://index-b.example",
        PodAnnouncementSearchResponse::new(
            "pod",
            vec![
                PodAnnouncementSearchResult::new(sole.clone(), 1.0, vec![]),
                PodAnnouncementSearchResult::new(shared.clone(), 1.0, vec![]),
            ],
        ),
    );

    let report = home
        .import_explicit_index_search(&reader, "pod", 10, &client)
        .unwrap();
    assert!(report.outcomes[0].ok);
    assert!(report.outcomes[1].ok);
    assert_eq!(report.retained_announcements, 3);

    {
        let binding = home.store();
        let store = binding.read().unwrap();
        let shared_known = store
            .known_pod_announcements
            .get(&(shared.origin_node_id, shared.pod_slug.clone()))
            .unwrap();
        assert!(shared_known
            .received_from_index_urls
            .contains("https://index-a.example"));
        assert!(shared_known
            .received_from_index_urls
            .contains("https://index-b.example"));
        let sole_known = store
            .known_pod_announcements
            .get(&(sole.origin_node_id, sole.pod_slug.clone()))
            .unwrap();
        assert!(sole_known
            .received_from_index_urls
            .contains("https://index-b.example"));
        assert!(!sole_known
            .received_from_index_urls
            .contains("https://index-a.example"));
    }

    approve_trust_policy_change(
        &home,
        TrustPolicyChange::RemoveIndexNode {
            base_url: "https://index-b.example".into(),
        },
    );

    let binding = home.store();
    let store = binding.read().unwrap();
    let policy = store.trust_policies.values().next().cloned().unwrap();
    let sole_known = store
        .known_pod_announcements
        .get(&(sole.origin_node_id, sole.pod_slug.clone()))
        .unwrap();
    assert!(!announcement_delivery_is_active(
        &store,
        sole_known,
        Some(&policy)
    ));
    // Audit rows remain for sole and shared.
    assert!(store
        .known_pod_announcements
        .contains_key(&(sole.origin_node_id, sole.pod_slug.clone())));
    assert!(store
        .known_pod_announcements
        .contains_key(&(shared.origin_node_id, shared.pod_slug.clone())));
    let shared_known = store
        .known_pod_announcements
        .get(&(shared.origin_node_id, shared.pod_slug.clone()))
        .unwrap();
    // Shared remains eligible via Index A provenance after removing B.
    assert!(announcement_delivery_is_active(
        &store,
        shared_known,
        Some(&policy)
    ));
}
