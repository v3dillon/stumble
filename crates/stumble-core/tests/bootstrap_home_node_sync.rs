//! Focused acceptance for Home Node outbound Bootstrap configuration and sync.
//!
//! Drives Core entry points with temporary SQLite, a scripted Announcement Stream
//! transport, and deterministic clocks. Verifies removable defaults, multi-endpoint
//! fallthrough, sole-source exclusion, privacy of outbound requests, and restart.

use chrono::{TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-home-bootstrap-{label}-{}",
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

fn origin_tools() -> AgentTools {
    AgentTools::new(seed_store())
}

fn sample_announcement(
    origin: &AgentTools,
    slug: &str,
    announced_at: chrono::DateTime<Utc>,
) -> PodAnnouncement {
    let ctx = origin.default_auth_context().unwrap();
    let curator = harness(
        origin,
        &format!("curator-{slug}"),
        vec![HarnessCapability::PodCuration],
    );
    let private = origin
        .create_pod(
            &curator,
            CreatePodRequest {
                name: slug.replace('-', " "),
                slug: slug.into(),
                description: format!("{slug} subject"),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let proposal = origin
        .create_pending_proposal(
            &curator,
            SensitiveChange::PublishPod { pod_id: private.id },
            announced_at,
            announced_at + chrono::Duration::hours(1),
        )
        .unwrap();
    let approver = harness(
        origin,
        &format!("approver-{slug}"),
        vec![HarnessCapability::Approval],
    );
    origin
        .approve_pending_proposal(&approver, proposal.id, announced_at)
        .unwrap();
    let url = format!("https://origin.example/federation/pods/{slug}");
    origin
        .pod_announcement_at(&ctx, slug, &url, announced_at)
        .unwrap()
}

fn stream_page(
    announcement: &PodAnnouncement,
    now: chrono::DateTime<Utc>,
) -> AnnouncementStreamPage {
    AnnouncementStreamPage::new(
        vec![AnnouncementStreamEntry::new(
            1,
            now,
            AnnouncementStreamEventKind::Admitted,
            announcement.origin_node_id,
            announcement.pod_slug.clone(),
            AnnouncementStreamPayload::Announcement(announcement.clone()),
        )],
        None,
        50,
    )
}

fn empty_page() -> AnnouncementStreamPage {
    AnnouncementStreamPage::new(vec![], None, 50)
}

fn clear_default_bootstrap(tools: &AgentTools, admin: &AuthContext) {
    let default_id = tools.list_bootstrap_endpoints(admin).unwrap()[0].id;
    tools.remove_bootstrap_endpoint(admin, default_id).unwrap();
}

#[test]
fn new_home_node_receives_sponsored_default_as_removable_entry() {
    let dir = TestDataDir::new("default");
    let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
    let admin = harness(
        &tools,
        "bootstrap admin",
        vec![HarnessCapability::Administration],
    );
    let endpoints = tools.list_bootstrap_endpoints(&admin).unwrap();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].base_url, DEFAULT_SPONSORED_BOOTSTRAP_URL);
    assert!(endpoints[0].enabled);
    assert!(endpoints[0].is_sponsored_default);

    let removed = tools
        .remove_bootstrap_endpoint(&admin, endpoints[0].id)
        .unwrap();
    assert_eq!(removed.base_url, DEFAULT_SPONSORED_BOOTSTRAP_URL);
    assert!(tools.list_bootstrap_endpoints(&admin).unwrap().is_empty());
}

#[test]
fn ordered_list_supports_add_disable_remove_and_inspect() {
    let dir = TestDataDir::new("crud");
    let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
    let admin = harness(
        &tools,
        "bootstrap admin",
        vec![HarnessCapability::Administration],
    );
    clear_default_bootstrap(&tools, &admin);

    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let a = tools
        .add_bootstrap_endpoint(&admin, "alpha", "https://alpha.bootstrap.example", now)
        .unwrap();
    let b = tools
        .add_bootstrap_endpoint(&admin, "beta", "https://beta.bootstrap.example", now)
        .unwrap();
    let listed = tools.list_bootstrap_endpoints(&admin).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, a.id);
    assert_eq!(listed[1].id, b.id);

    tools
        .set_bootstrap_endpoint_enabled(&admin, a.id, false)
        .unwrap();
    assert!(!tools.list_bootstrap_endpoints(&admin).unwrap()[0].enabled);

    tools.remove_bootstrap_endpoint(&admin, b.id).unwrap();
    let remaining = tools.list_bootstrap_endpoints(&admin).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, a.id);

    let status = tools.bootstrap_status(&admin).unwrap();
    assert_eq!(status.len(), 1);
    assert!(status[0].sync.cursor.is_none());
    assert!(status[0].sync.last_error.is_none());
}

#[test]
fn sync_fetches_verifies_and_persists_cursor_per_bootstrap() {
    let dir = TestDataDir::new("sync-cursor");
    let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
    let admin = harness(
        &tools,
        "bootstrap admin",
        vec![HarnessCapability::Administration],
    );
    clear_default_bootstrap(&tools, &admin);

    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let endpoint = tools
        .add_bootstrap_endpoint(&admin, "primary", "https://boot.example", now)
        .unwrap();
    let origin = origin_tools();
    let announcement = sample_announcement(&origin, "systems", now);
    let mut page = stream_page(&announcement, now);
    page.next_cursor = Some("1".into());

    let mut client = ScriptedAnnouncementStreamClient::new();
    client.push_page(&endpoint.base_url, None, page);
    client.push_page(&endpoint.base_url, Some("1"), empty_page());

    let report = tools
        .sync_bootstrap_endpoints(&admin, &client, now)
        .unwrap();
    assert!(report.outcomes[0].ok);
    assert_eq!(report.retained_announcements, 1);
    assert_eq!(report.outcomes[0].cursor.as_deref(), Some("1"));

    let status = tools.bootstrap_status(&admin).unwrap();
    assert_eq!(status[0].sync.cursor.as_deref(), Some("1"));
    assert_eq!(status[0].sync.last_success_at, Some(now));

    let store = tools.store();
    let guard = store.read().unwrap();
    let known = guard
        .known_pod_announcements
        .get(&(announcement.origin_node_id, "systems".into()))
        .unwrap();
    assert!(known
        .received_from_bootstrap_urls
        .contains(&endpoint.base_url));
}

#[test]
fn refresh_falls_through_without_discarding_verified_announcements() {
    let dir = TestDataDir::new("fallthrough");
    let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
    let admin = harness(
        &tools,
        "bootstrap admin",
        vec![HarnessCapability::Administration],
    );
    clear_default_bootstrap(&tools, &admin);

    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let primary = tools
        .add_bootstrap_endpoint(&admin, "primary", "https://primary.bootstrap.example", now)
        .unwrap();
    let backup = tools
        .add_bootstrap_endpoint(&admin, "backup", "https://backup.bootstrap.example", now)
        .unwrap();

    let origin = origin_tools();
    let existing = sample_announcement(&origin, "already-known", now);
    {
        let store = tools.store();
        let mut guard = store.write().unwrap();
        retain_verified_pod_announcement(
            &mut guard,
            existing.clone(),
            DeliveryProvenance::bootstrap(primary.base_url.clone()),
            now,
        )
        .unwrap();
    }

    let fresh = sample_announcement(&origin, "from-backup", now);
    let mut client = ScriptedAnnouncementStreamClient::new();
    client.fail(
        &primary.base_url,
        BootstrapSyncFailure::new(BootstrapSyncFailureKind::Transport, "connection refused"),
    );
    client.push_page(&backup.base_url, None, stream_page(&fresh, now));

    let report = tools
        .sync_bootstrap_endpoints(&admin, &client, now)
        .unwrap();
    assert!(!report.outcomes[0].ok);
    assert_eq!(
        report.outcomes[0].error.as_ref().unwrap().kind,
        BootstrapSyncFailureKind::Transport
    );
    assert!(report.outcomes[1].ok);
    assert_eq!(report.retained_announcements, 1);

    let store = tools.store();
    let guard = store.read().unwrap();
    assert!(guard
        .known_pod_announcements
        .contains_key(&(existing.origin_node_id, existing.pod_slug.clone())));
    assert!(guard
        .known_pod_announcements
        .contains_key(&(fresh.origin_node_id, fresh.pod_slug.clone())));
}

#[test]
fn remove_excludes_sole_source_preserves_audit_and_independent_copies() {
    let dir = TestDataDir::new("remove-eligibility");
    let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
    let admin = harness(
        &tools,
        "bootstrap admin",
        vec![HarnessCapability::Administration],
    );
    let reader = harness(&tools, "explore reader", vec![HarnessCapability::FeedRead]);
    clear_default_bootstrap(&tools, &admin);

    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let a = tools
        .add_bootstrap_endpoint(&admin, "a", "https://a.bootstrap.example", now)
        .unwrap();
    let b = tools
        .add_bootstrap_endpoint(&admin, "b", "https://b.bootstrap.example", now)
        .unwrap();

    let origin = origin_tools();
    let sole = sample_announcement(&origin, "sole-rust-pod", now);
    let shared = sample_announcement(&origin, "shared-rust-pod", now);
    {
        let store = tools.store();
        let mut guard = store.write().unwrap();
        retain_verified_pod_announcement(
            &mut guard,
            sole.clone(),
            DeliveryProvenance::bootstrap(a.base_url.clone()),
            now,
        )
        .unwrap();
        retain_verified_pod_announcement(
            &mut guard,
            shared.clone(),
            DeliveryProvenance::bootstrap(a.base_url.clone()),
            now,
        )
        .unwrap();
        retain_verified_pod_announcement(
            &mut guard,
            shared.clone(),
            DeliveryProvenance::bootstrap(b.base_url.clone()),
            now,
        )
        .unwrap();
    }

    let before = tools
        .explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
        .unwrap();
    assert_eq!(before.results.len(), 2);

    tools.remove_bootstrap_endpoint(&admin, a.id).unwrap();

    let after = tools
        .explore_public_pods(&reader, ExploreRequest::new("rust", 10, 0).unwrap())
        .unwrap();
    assert_eq!(after.results.len(), 1);
    assert_eq!(after.results[0].announcement.pod_slug, "shared-rust-pod");

    let store = tools.store();
    let guard = store.read().unwrap();
    assert!(guard
        .known_pod_announcements
        .contains_key(&(sole.origin_node_id, sole.pod_slug.clone())));
}

#[test]
fn outbound_sync_sends_no_private_evidence() {
    let dir = TestDataDir::new("privacy");
    let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
    let admin = harness(
        &tools,
        "bootstrap admin",
        vec![HarnessCapability::Administration],
    );
    clear_default_bootstrap(&tools, &admin);

    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let endpoint = tools
        .add_bootstrap_endpoint(&admin, "primary", "https://private-check.example", now)
        .unwrap();
    let mut client = ScriptedAnnouncementStreamClient::new();
    client.push_page(&endpoint.base_url, None, empty_page());

    tools
        .sync_bootstrap_endpoints(&admin, &client, now)
        .unwrap();

    let captured = client.captured.lock().unwrap();
    assert_eq!(captured.len(), 1);
    let (_, request) = &captured[0];
    assert!(request_is_public_only(request));
    let wire = serde_json::to_value(request).unwrap();
    let object = wire.as_object().unwrap();
    for forbidden in [
        "taste_profile",
        "subscriptions",
        "feedback",
        "source_affinity",
        "interests",
        "query",
        "user_id",
        "affinity",
    ] {
        assert!(
            !object.contains_key(forbidden),
            "outbound request must not carry {forbidden}"
        );
    }
}

#[test]
fn config_and_sync_progress_survive_sqlite_restart() {
    let dir = TestDataDir::new("restart");
    let now = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let endpoint_id;
    {
        let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
        let admin = harness(
            &tools,
            "bootstrap admin",
            vec![HarnessCapability::Administration],
        );
        clear_default_bootstrap(&tools, &admin);
        let endpoint = tools
            .add_bootstrap_endpoint(&admin, "durable", "https://durable.bootstrap.example", now)
            .unwrap();
        endpoint_id = endpoint.id;
        let origin = origin_tools();
        let announcement = sample_announcement(&origin, "durable-pod", now);
        let mut client = ScriptedAnnouncementStreamClient::new();
        let mut page = stream_page(&announcement, now);
        page.next_cursor = Some("9".into());
        client.push_page(&endpoint.base_url, None, page);
        client.push_page(&endpoint.base_url, Some("9"), empty_page());
        tools
            .sync_bootstrap_endpoints(&admin, &client, now)
            .unwrap();
    }

    let restarted = AgentTools::open_initialized_home_node(&dir.0).unwrap();
    let admin = harness(
        &restarted,
        "bootstrap admin after restart",
        vec![HarnessCapability::Administration],
    );
    let status = restarted.bootstrap_status(&admin).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].endpoint.id, endpoint_id);
    assert_eq!(
        status[0].endpoint.base_url,
        "https://durable.bootstrap.example"
    );
    assert_eq!(status[0].sync.cursor.as_deref(), Some("9"));
    assert_eq!(status[0].sync.last_success_at, Some(now));
}

#[test]
fn direct_pod_url_subscription_works_with_all_bootstraps_disabled() {
    let dir = TestDataDir::new("direct-url");
    let tools = AgentTools::initialize_home_node(&dir.0, seed_store).unwrap();
    let admin = harness(
        &tools,
        "bootstrap admin",
        vec![HarnessCapability::Administration],
    );
    for endpoint in tools.list_bootstrap_endpoints(&admin).unwrap() {
        tools
            .set_bootstrap_endpoint_enabled(&admin, endpoint.id, false)
            .unwrap();
    }
    assert!(tools
        .list_bootstrap_endpoints(&admin)
        .unwrap()
        .iter()
        .all(|endpoint| !endpoint.enabled));

    let direct_url = "http://127.0.0.1/federation/pods/reachable-origin";
    assert_eq!(
        validate_public_pod_url(direct_url, "reachable-origin").unwrap(),
        "http://127.0.0.1/federation/pods/reachable-origin"
    );

    let curator = harness(&tools, "curator", vec![HarnessCapability::PodCuration]);
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: "Local Only".into(),
                slug: "local-only".into(),
                description: "works without bootstrap".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let subscriber = harness(
        &tools,
        "subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );
    tools
        .subscribe_local_pod(&subscriber, pod.id)
        .expect("local Subscription must work with every Bootstrap disabled");
}
