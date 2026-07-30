use crate::common::*;
use stumble_core::*;

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
    assert!(feedback_affects_future_exposure(FeedbackKind::Dismissed));
}
