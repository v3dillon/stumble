use crate::common::*;
use chrono::{Duration, TimeZone, Utc};
use stumble_core::*;

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
    // Index capability enabled so search_pod_announcements exercises Index eligibility.
    let home = AgentTools::open_home_node(&home_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
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
                source_metadata: CandidateSourceMetadata::default(),
                description: None,
                domain: "allowed.example".into(),
                submitted_by: None,
                discovered_by_crawler: false,
                submitter_note: None,
                summary: Some("kept after withdrawal".into()),
                provenance: Vec::new(),
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
    {
        let store = home.store();
        let store = store.read().unwrap();
        assert!(announcement_is_discovery_eligible(
            &store,
            &announcement,
            t0
        ));
    }

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
    {
        let store = home.store();
        let store = store.read().unwrap();
        assert!(!announcement_is_discovery_eligible(
            &store,
            &live_announcement,
            live_now + chrono::Duration::hours(1)
        ));
    }

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
