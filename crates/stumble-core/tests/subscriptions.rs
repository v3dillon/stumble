mod support;

use chrono::Utc;
use stumble_core::*;
use support::*;

#[test]
fn incompatible_protocol_is_rejected_before_events_are_projected() {
    let origin = AgentTools::new(seed_store());
    let home = AgentTools::new(seed_store());
    let pod = create_public_pod(&origin, "future-event-shapes");
    submit_and_accept_placement(&origin, &pod);
    let mut snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &pod.slug, None)
        .unwrap();
    snapshot.node.supported_protocol_version = "stumble/0.1".into();
    let subscriber = register_authenticated_harness(
        &home,
        "protocol negotiator",
        vec![HarnessCapability::SubscriptionManagement],
    );

    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/future-event-shapes",
            snapshot,
        ),
        Utc::now(),
    );

    assert!(matches!(
        result,
        Err(AgentToolsError::IncompatibleProtocol { received, supported })
            if received == "stumble/0.1" && supported == CURRENT_PROTOCOL_VERSION
    ));
    assert!(home.pod_by_slug(&pod.slug, subscriber.tenant_id).is_err());
}

#[test]
fn invalid_signed_artifacts_are_rejected_before_any_remote_projection() {
    // Arrange
    let origin_dir = TestDataDir::new("invalid-origin");
    let home_dir = TestDataDir::new("invalid-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "tampered-pod");
    let mut snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &pod.slug, None)
        .unwrap();
    snapshot.events[0].payload_json["pod"]["description"] =
        serde_json::json!("an attacker changed signed metadata");
    let subscriber = register_authenticated_harness(
        &home,
        "careful subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    // Act
    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/tampered-pod",
            snapshot,
        ),
        Utc::now(),
    );

    // Assert
    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::InvalidSignature))
    ));
    assert!(home.pod_by_slug("tampered-pod", None).is_err());
}

#[test]
fn malformed_but_validly_signed_event_is_rejected_before_projection() {
    // Arrange
    let origin_dir = TestDataDir::new("malformed-origin");
    let home_dir = TestDataDir::new("malformed-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "malformed-pod");
    let mut snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &pod.slug, None)
        .unwrap();
    let node = origin.store().read().unwrap().default_node().unwrap();
    let malformed = sign_public_event(
        &node,
        "content_item_placed",
        &pod.slug,
        serde_json::json!({
            "content_item": "not a Content Item",
            "accepted_placement": "not an Accepted Placement"
        }),
        snapshot.manifest.latest_known_event_hash.clone(),
    )
    .unwrap();
    snapshot.manifest.latest_known_event_hash = Some(malformed.content_hash.clone());
    snapshot.events.push(malformed);
    let subscriber = register_authenticated_harness(
        &home,
        "strict subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    // Act
    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/malformed-pod",
            snapshot,
        ),
        Utc::now(),
    );

    // Assert
    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
    assert!(home.pod_by_slug("malformed-pod", None).is_err());
}

#[test]
fn unaccepted_legacy_submission_is_absent_from_synchronization_artifacts() {
    // Arrange
    let origin_dir = TestDataDir::new("unaccepted-origin");
    let home_dir = TestDataDir::new("unaccepted-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "accepted-only");
    let curator = register_authenticated_harness(
        &origin,
        "legacy submitter",
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::CandidateSubmission,
        ],
    );
    origin
        .submit_link_to_pod(
            &curator,
            &pod.slug,
            SubmitLinkRequest {
                url: "https://private-candidate.example/unaccepted".into(),
                title: Some("Unaccepted legacy submission".into()),
                description: Some("must not synchronize".into()),
                note: Some("private submitter note".into()),
                tags: vec!["private-candidate".into()],
                discovered_by_crawler: false,
            },
        )
        .unwrap();

    // Act
    let snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &pod.slug, None)
        .unwrap();
    let artifacts = serde_json::to_string(&snapshot).unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "accepted-only subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );
    home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/accepted-only",
            snapshot,
        ),
        Utc::now(),
    )
    .unwrap();

    // Assert
    assert!(!artifacts.contains("private-candidate.example"));
    assert!(!artifacts.contains("private submitter note"));
    assert!(home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn later_accepted_placement_syncs_from_cursor_without_exporting_home_feedback() {
    // Arrange: subscribe before the Origin Node accepts any Content Item.
    let origin_dir = TestDataDir::new("incremental-origin");
    let home_dir = TestDataDir::new("incremental-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "incremental-pod");
    let origin_owner = origin.default_auth_context().unwrap();
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let user = register_authenticated_harness(
        &home,
        "private Home Node user",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
        ],
    );
    let subscribed = home
        .subscribe_public_pod(
            &user,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/incremental-pod",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(subscribed.imported_events, 1);
    submit_and_accept_placement(&origin, &pod);

    // Act: resume from the stored cursor and apply only the new accepted placement.
    let incremental = origin
        .federation_pod_snapshot(
            &origin_owner,
            &pod.slug,
            subscribed.subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    assert_eq!(incremental.events.len(), 1);
    let synchronized = home
        .synchronize_subscription(&user, subscribed.subscription.id, incremental)
        .unwrap();

    // Assert: the permitted reference is delivered, while feedback stays Home-Node local.
    assert_eq!(synchronized.imported_events, 1);
    let feed = home
        .get_feed_batch(&user, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap();
    let item = &feed.items[0];
    assert_eq!(item.content_reference.title, "Remote report");
    assert_eq!(
        item.content_reference.permitted_description.as_deref(),
        Some("A permitted excerpt")
    );
    home.record_feed_feedback(
        &user,
        item.content_reference.content_item_id,
        FeedbackKind::Interesting,
        None,
        Some("locally useful".into()),
        Utc::now(),
    )
    .unwrap();
    let mut private_taste = UpdateTasteProfileRequest::default();
    private_taste.interests = Some(vec!["secret-home-interest".into()]);
    home.update_taste_profile(&user, private_taste).unwrap();
    assert!(home
        .taste_profile(&user)
        .unwrap()
        .explicit
        .interests
        .contains(&"secret-home-interest".to_string()));
    let public_artifacts = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let public_json = serde_json::to_string(&public_artifacts).unwrap();
    assert!(!public_json.contains("locally useful"));
    assert!(!public_json.contains("feedback"));
    let home_public_json = serde_json::to_string(&(
        home.node_info(&home.default_auth_context().unwrap())
            .unwrap(),
        home.list_public_pods(&home.default_auth_context().unwrap())
            .unwrap(),
    ))
    .unwrap();
    assert!(!home_public_json.contains("secret-home-interest"));
    assert!(!home_public_json.contains("locally useful"));
    assert!(!home_public_json.contains("feedback"));
    assert!(home
        .federation_pod_snapshot(&home.default_auth_context().unwrap(), &pod.slug, None)
        .is_err());
}

#[test]
fn a_remote_slug_collision_cannot_mutate_a_local_private_pod() {
    let origin_dir = TestDataDir::new("collision-origin");
    let home_dir = TestDataDir::new("collision-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let remote_pod = create_public_pod(&origin, "collision-pod");
    let local_owner = register_authenticated_harness(
        &home,
        "local Pod owner",
        vec![HarnessCapability::PodCuration],
    );
    let local_pod = home
        .create_pod(
            &local_owner,
            CreatePodRequest {
                name: "Private local Pod".into(),
                slug: "collision-pod".into(),
                description: "must remain local".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let local_package = home
        .get_pod_package_version(
            &local_owner,
            &local_pod.slug,
            PackageVersion::new(1).unwrap(),
        )
        .unwrap();
    let snapshot = origin
        .federation_pod_snapshot(
            &origin.default_auth_context().unwrap(),
            &remote_pod.slug,
            None,
        )
        .unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "collision subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/collision-pod",
            snapshot,
        ),
        Utc::now(),
    );

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Duplicate(_)))
    ));
    let unchanged = home.pod_by_slug("collision-pod", None).unwrap();
    assert_eq!(unchanged.id, local_pod.id);
    assert_eq!(unchanged.visibility, Visibility::Private);
    assert_eq!(unchanged.origin_node_id, local_pod.origin_node_id);
    assert_eq!(
        home.get_pod_package_version(
            &local_owner,
            &local_pod.slug,
            PackageVersion::new(1).unwrap(),
        )
        .unwrap()
        .context_md,
        local_package.context_md
    );
}

#[test]
fn a_remote_pod_id_collision_cannot_overwrite_an_unrelated_local_pod() {
    let origin_dir = TestDataDir::new("id-collision-origin");
    let home_dir = TestDataDir::new("id-collision-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let remote_pod = create_public_pod(&origin, "remote-id-collision");
    let local_owner = register_authenticated_harness(
        &home,
        "ID collision owner",
        vec![HarnessCapability::PodCuration],
    );
    let mut local_pod = home
        .create_pod(
            &local_owner,
            CreatePodRequest {
                name: "Unrelated private Pod".into(),
                slug: "unrelated-private-pod".into(),
                description: "must not be overwritten".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    {
        let store = home.store();
        let mut store = store.write().unwrap();
        store.pods.remove(&local_pod.id);
        local_pod.id = remote_pod.id;
        store.pods.insert(local_pod.id, local_pod.clone());
    }
    let snapshot = origin
        .federation_pod_snapshot(
            &origin.default_auth_context().unwrap(),
            &remote_pod.slug,
            None,
        )
        .unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "ID collision subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );

    let result = home.subscribe_public_pod(
        &subscriber,
        SubscribePublicPodRequest::new(
            "https://origin.example/federation/pods/remote-id-collision",
            snapshot,
        ),
        Utc::now(),
    );

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Duplicate(_)))
    ));
    assert_eq!(
        home.pod_by_slug("unrelated-private-pod", None).unwrap().id,
        remote_pod.id
    );
    assert!(home.pod_by_slug("remote-id-collision", None).is_err());
}

#[test]
fn anonymous_context_cannot_synchronize_an_owned_subscription() {
    let origin_dir = TestDataDir::new("anonymous-origin");
    let home_dir = TestDataDir::new("anonymous-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "owned-subscription");
    let origin_owner = origin.default_auth_context().unwrap();
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "subscription owner",
        vec![HarnessCapability::SubscriptionManagement],
    );
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/owned-subscription",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    let empty = origin
        .federation_pod_snapshot(
            &origin_owner,
            &pod.slug,
            subscribed.subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    let mut anonymous = subscriber.clone();
    anonymous.user_id = None;
    anonymous.harness_id = None;

    let result = home.synchronize_subscription(&anonymous, subscribed.subscription.id, empty);

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
}

#[test]
fn invalid_package_version_rejects_the_whole_increment_before_projection() {
    let origin_dir = TestDataDir::new("package-origin");
    let home_dir = TestDataDir::new("package-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "package-version-pod");
    let origin_owner = origin.default_auth_context().unwrap();
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "package subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::FeedRead,
        ],
    );
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/package-version-pod",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    submit_and_accept_placement(&origin, &pod);
    let mut incremental = origin
        .federation_pod_snapshot(
            &origin_owner,
            &pod.slug,
            subscribed.subscription.last_event_hash.as_deref(),
        )
        .unwrap();
    let mut invalid_package = origin
        .get_pod_package_version(&origin_owner, &pod.slug, PackageVersion::new(1).unwrap())
        .unwrap();
    invalid_package.version = 0;
    let node = origin.store().read().unwrap().default_node().unwrap();
    let invalid_event = sign_public_event(
        &node,
        "pod_skill_pack_updated",
        &pod.slug,
        serde_json::json!({"package": invalid_package}),
        incremental.manifest.latest_known_event_hash.clone(),
    )
    .unwrap();
    incremental.manifest.skill_pack_version = 0;
    incremental.manifest.latest_known_event_hash = Some(invalid_event.content_hash.clone());
    incremental.events.push(invalid_event);

    let result =
        home.synchronize_subscription(&subscriber, subscribed.subscription.id, incremental);

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
    assert!(home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        home.subscription(&subscriber, subscribed.subscription.id)
            .unwrap()
            .last_event_hash,
        subscribed.subscription.last_event_hash
    );
}

#[test]
fn a_changed_package_cannot_reuse_an_immutable_version() {
    let origin_dir = TestDataDir::new("reused-package-origin");
    let home_dir = TestDataDir::new("reused-package-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let pod = create_public_pod(&origin, "immutable-package-pod");
    let origin_owner = origin.default_auth_context().unwrap();
    let initial = origin
        .federation_pod_snapshot(&origin_owner, &pod.slug, None)
        .unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "immutable package subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );
    let subscribed = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/immutable-package-pod",
                initial,
            ),
            Utc::now(),
        )
        .unwrap();
    let mut changed = origin
        .get_pod_package_version(&origin_owner, &pod.slug, PackageVersion::new(1).unwrap())
        .unwrap();
    changed
        .context_md
        .push_str("\nChanged while reusing version one.\n");
    let node = origin.store().read().unwrap().default_node().unwrap();
    let event = sign_public_event(
        &node,
        "pod_skill_pack_updated",
        &pod.slug,
        serde_json::json!({"package": changed}),
        subscribed.subscription.last_event_hash.clone(),
    )
    .unwrap();
    let snapshot = FederationPodSnapshot::new(
        origin.node_info(&origin_owner).unwrap(),
        PodManifest {
            pod,
            latest_known_event_hash: Some(event.content_hash.clone()),
            skill_pack_version: 1,
            public_source_summary: vec![],
        },
        vec![event],
    );

    let result = home.synchronize_subscription(&subscriber, subscribed.subscription.id, snapshot);

    assert!(matches!(
        result,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
}

#[test]
fn publication_boundary_hides_private_history_but_reemits_accepted_content() {
    // Arrange: accept content while the Pod is still private, then publish.
    let origin = AgentTools::new(seed_store());
    let owner = origin.local_owner_auth_context().unwrap();
    let pod = origin
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Private then public".into(),
                slug: "private-then-public".into(),
                description: "publication boundary fixture".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    submit_and_accept_placement(&origin, &pod);
    let outcome = origin
        .request_set_pod_visibility(&owner, pod.id, Visibility::Public, Utc::now())
        .unwrap();
    assert!(matches!(outcome, PodVisibilityOutcome::Updated(_)));

    // Assert: federation serves only the publication onward, with the accepted
    // content re-emitted after `pod_published`.
    let store = origin.store();
    let store = store.read().unwrap();
    let served = store.public_events_for_pod(&pod.slug);
    assert_eq!(served[0].event_type, "pod_published");
    assert!(served
        .iter()
        .all(|event| event.event_type != "pod_created"));
    assert!(served
        .iter()
        .any(|event| event.event_type == "content_item_placed"));
    drop(store);

    // The private-era creation event stays in the local log.
    let store = origin.store();
    let store = store.read().unwrap();
    assert!(store
        .event_log
        .iter()
        .any(|event| event.pod_slug == pod.slug && event.event_type == "pod_created"));
    drop(store);

    // A subscriber imports the served suffix and receives the content.
    let home = AgentTools::new(seed_store());
    let snapshot = origin
        .federation_pod_snapshot(&owner, &pod.slug, None)
        .unwrap();
    let subscriber = register_authenticated_harness(
        &home,
        "post-publication subscriber",
        vec![HarnessCapability::SubscriptionManagement, HarnessCapability::FeedRead],
    );
    let synchronized = home
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/private-then-public",
                snapshot,
            ),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(synchronized.imported_events, 2);
    let feed = home
        .get_feed_batch(&subscriber, FeedBatchRequest::new(1).unwrap(), Utc::now())
        .unwrap();
    assert!(!feed.items.is_empty());
}
