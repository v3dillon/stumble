use crate::common::*;
use chrono::Utc;
use stumble_core::*;

#[test]
fn agent_evidence_enriches_ordering_with_inspectable_reason_under_core_authority() {
    let origin_dir = TestDataDir::new("agent-ev-origin");
    let home_dir = TestDataDir::new("agent-ev-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let anchor = create_public_pod(
        &origin,
        "agent-anchor-sys",
        "Distributed systems research notes",
    );
    let related = create_public_pod(
        &origin,
        "agent-related-sys",
        "Distributed systems reliability lab",
    );
    let unrelated = create_public_pod(
        &origin,
        "agent-food-blog",
        "Cooking recipes and baking tips",
    );
    let mut announcements = Vec::new();
    for pod in [&anchor, &related, &unrelated] {
        let announcement = origin
            .pod_announcement(
                &origin.default_auth_context().unwrap(),
                &pod.slug,
                &format!("https://origin.example/federation/pods/{}", pod.slug),
            )
            .unwrap();
        home.index_pod_announcement(announcement.clone()).unwrap();
        announcements.push(announcement);
    }
    let anchor_ann = announcements
        .iter()
        .find(|a| a.pod_slug == "agent-anchor-sys")
        .unwrap()
        .clone();
    let related_ann = announcements
        .iter()
        .find(|a| a.pod_slug == "agent-related-sys")
        .unwrap()
        .clone();
    let food_ann = announcements
        .iter()
        .find(|a| a.pod_slug == "agent-food-blog")
        .unwrap()
        .clone();

    let reader = harness(&home, "agent-ev reader", vec![HarnessCapability::FeedRead]);
    let baseline = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 0).unwrap(),
        )
        .unwrap();
    let baseline_related = baseline
        .results
        .iter()
        .find(|r| r.announcement.pod_slug == "agent-related-sys")
        .expect("related pod in baseline")
        .relevance;

    let agent = harness(
        &home,
        "similarity agent",
        vec![HarnessCapability::PodSimilarityEvidence],
    );
    let submitted = home
        .submit_pod_similarity_agent_evidence(
            &agent,
            SubmitPodSimilarityAgentEvidenceRequest {
                left_announcement_id: anchor_ann.id,
                right_announcement_id: related_ann.id,
                confidence: CandidateConfidence::new(0.9).unwrap(),
                explanation: "Shared careful systems research subject and methods".into(),
                public_inputs: vec![
                    PodSimilarityAgentEvidenceAnnouncementRef {
                        announcement_id: anchor_ann.id,
                        origin_node_id: anchor_ann.origin_node_id,
                        pod_slug: anchor_ann.pod_slug.clone(),
                    },
                    PodSimilarityAgentEvidenceAnnouncementRef {
                        announcement_id: related_ann.id,
                        origin_node_id: related_ann.origin_node_id,
                        pod_slug: related_ann.pod_slug.clone(),
                    },
                ],
                model_provenance: "local-test-model".into(),
                harness_idempotency_key: "agent-ev-1".into(),
                freshness_hours: Some(24),
            },
        )
        .unwrap();
    assert_eq!(submitted.submitted_by, agent.harness_id.unwrap());

    // Idempotent replay returns the same record.
    let again = home
        .submit_pod_similarity_agent_evidence(
            &agent,
            SubmitPodSimilarityAgentEvidenceRequest {
                left_announcement_id: anchor_ann.id,
                right_announcement_id: related_ann.id,
                confidence: CandidateConfidence::new(0.9).unwrap(),
                explanation: "Shared careful systems research subject and methods".into(),
                public_inputs: vec![
                    PodSimilarityAgentEvidenceAnnouncementRef {
                        announcement_id: anchor_ann.id,
                        origin_node_id: anchor_ann.origin_node_id,
                        pod_slug: anchor_ann.pod_slug.clone(),
                    },
                    PodSimilarityAgentEvidenceAnnouncementRef {
                        announcement_id: related_ann.id,
                        origin_node_id: related_ann.origin_node_id,
                        pod_slug: related_ann.pod_slug.clone(),
                    },
                ],
                model_provenance: "local-test-model".into(),
                harness_idempotency_key: "agent-ev-1".into(),
                freshness_hours: Some(24),
            },
        )
        .unwrap();
    assert_eq!(again.id, submitted.id);

    let enriched = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 0).unwrap(),
        )
        .unwrap();
    let related_result = enriched
        .results
        .iter()
        .find(|r| r.announcement.pod_slug == "agent-related-sys")
        .expect("related pod after agent evidence");
    assert!(
        related_result.relevance > baseline_related,
        "agent evidence should raise local ordering score"
    );
    assert!(related_result
        .reasons
        .iter()
        .any(|reason| reason.contains("agent evidence")
            && reason.contains("not transferable trust")
            && reason.contains("not an Endorsement")));
    // Agent evidence never appears as an Endorsement on the Explore DTO.
    assert!(related_result.endorsements.is_empty());
    // Agent evidence alone cannot surface an unrelated zero-base pod.
    assert!(enriched
        .results
        .iter()
        .all(|r| r.announcement.pod_slug != "agent-food-blog"));
    // Attempt to force food via agent evidence still cannot create eligibility.
    let force = home.submit_pod_similarity_agent_evidence(
        &agent,
        SubmitPodSimilarityAgentEvidenceRequest {
            left_announcement_id: anchor_ann.id,
            right_announcement_id: food_ann.id,
            confidence: CandidateConfidence::new(1.0).unwrap(),
            explanation: "Attempted forced relationship".into(),
            public_inputs: vec![
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: anchor_ann.id,
                    origin_node_id: anchor_ann.origin_node_id,
                    pod_slug: anchor_ann.pod_slug.clone(),
                },
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: food_ann.id,
                    origin_node_id: food_ann.origin_node_id,
                    pod_slug: food_ann.pod_slug.clone(),
                },
            ],
            model_provenance: "local-test-model".into(),
            harness_idempotency_key: "agent-ev-food".into(),
            freshness_hours: Some(24),
        },
    );
    assert!(force.is_ok());
    let after_force = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 0).unwrap(),
        )
        .unwrap();
    assert!(after_force
        .results
        .iter()
        .all(|r| r.announcement.pod_slug != "agent-food-blog"));
}

#[test]
fn agent_evidence_rejects_stale_blocked_and_missing_capability() {
    let origin_dir = TestDataDir::new("agent-rej-origin");
    let home_dir = TestDataDir::new("agent-rej-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let left_pod = create_public_pod(&origin, "rej-left", "Distributed systems left");
    let right_pod = create_public_pod(&origin, "rej-right", "Distributed systems right");
    let left = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &left_pod.slug,
            "https://origin.example/federation/pods/rej-left",
        )
        .unwrap();
    let right = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &right_pod.slug,
            "https://origin.example/federation/pods/rej-right",
        )
        .unwrap();
    home.index_pod_announcement(left.clone()).unwrap();
    home.index_pod_announcement(right.clone()).unwrap();

    let reader_only = harness(
        &home,
        "no capability agent",
        vec![HarnessCapability::FeedRead],
    );
    let denied = home.submit_pod_similarity_agent_evidence(
        &reader_only,
        SubmitPodSimilarityAgentEvidenceRequest {
            left_announcement_id: left.id,
            right_announcement_id: right.id,
            confidence: CandidateConfidence::new(0.5).unwrap(),
            explanation: "should be denied".into(),
            public_inputs: vec![
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: left.id,
                    origin_node_id: left.origin_node_id,
                    pod_slug: left.pod_slug.clone(),
                },
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: right.id,
                    origin_node_id: right.origin_node_id,
                    pod_slug: right.pod_slug.clone(),
                },
            ],
            model_provenance: "m".into(),
            harness_idempotency_key: "denied-1".into(),
            freshness_hours: None,
        },
    );
    assert!(matches!(denied, Err(AgentToolsError::Forbidden { .. })));

    let agent = harness(
        &home,
        "rej agent",
        vec![HarnessCapability::PodSimilarityEvidence],
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockPod {
            origin_node_id: right.origin_node_id,
            pod_slug: right.pod_slug.clone(),
        },
    );
    let blocked = home.submit_pod_similarity_agent_evidence(
        &agent,
        SubmitPodSimilarityAgentEvidenceRequest {
            left_announcement_id: left.id,
            right_announcement_id: right.id,
            confidence: CandidateConfidence::new(0.5).unwrap(),
            explanation: "blocked right side".into(),
            public_inputs: vec![
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: left.id,
                    origin_node_id: left.origin_node_id,
                    pod_slug: left.pod_slug.clone(),
                },
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: right.id,
                    origin_node_id: right.origin_node_id,
                    pod_slug: right.pod_slug.clone(),
                },
            ],
            model_provenance: "m".into(),
            harness_idempotency_key: "blocked-1".into(),
            freshness_hours: None,
        },
    );
    assert!(
        matches!(blocked, Err(AgentToolsError::Store(StoreError::Validation(ref msg))) if msg.contains("blocked")),
        "expected blocked validation, got {blocked:?}"
    );

    // Unknown announcement id is unverifiable.
    let unknown = home.submit_pod_similarity_agent_evidence(
        &agent,
        SubmitPodSimilarityAgentEvidenceRequest {
            left_announcement_id: left.id,
            right_announcement_id: uuid::Uuid::now_v7(),
            confidence: CandidateConfidence::new(0.5).unwrap(),
            explanation: "unknown right".into(),
            public_inputs: vec![
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: left.id,
                    origin_node_id: left.origin_node_id,
                    pod_slug: left.pod_slug.clone(),
                },
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: uuid::Uuid::now_v7(),
                    origin_node_id: right.origin_node_id,
                    pod_slug: "missing".into(),
                },
            ],
            model_provenance: "m".into(),
            harness_idempotency_key: "unknown-1".into(),
            freshness_hours: None,
        },
    );
    assert!(matches!(
        unknown,
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
}

#[test]
fn revoking_harness_excludes_agent_evidence_from_ranking_and_blocks_new_submissions() {
    let origin_dir = TestDataDir::new("agent-rev-origin");
    let home_dir = TestDataDir::new("agent-rev-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let a = create_public_pod(&origin, "rev-a", "Distributed systems research a");
    let b = create_public_pod(&origin, "rev-b", "Distributed systems research b");
    let left = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &a.slug,
            "https://origin.example/federation/pods/rev-a",
        )
        .unwrap();
    let right = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &b.slug,
            "https://origin.example/federation/pods/rev-b",
        )
        .unwrap();
    home.index_pod_announcement(left.clone()).unwrap();
    home.index_pod_announcement(right.clone()).unwrap();
    let agent = harness(
        &home,
        "revocable agent",
        vec![HarnessCapability::PodSimilarityEvidence],
    );
    let reader = harness(&home, "rev reader", vec![HarnessCapability::FeedRead]);
    home.submit_pod_similarity_agent_evidence(
        &agent,
        SubmitPodSimilarityAgentEvidenceRequest {
            left_announcement_id: left.id,
            right_announcement_id: right.id,
            confidence: CandidateConfidence::new(0.95).unwrap(),
            explanation: "Will be revoked".into(),
            public_inputs: vec![
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: left.id,
                    origin_node_id: left.origin_node_id,
                    pod_slug: left.pod_slug.clone(),
                },
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: right.id,
                    origin_node_id: right.origin_node_id,
                    pod_slug: right.pod_slug.clone(),
                },
            ],
            model_provenance: "m".into(),
            harness_idempotency_key: "rev-1".into(),
            freshness_hours: Some(24),
        },
    )
    .unwrap();
    let with_evidence = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 0).unwrap(),
        )
        .unwrap();
    let boosted = with_evidence
        .results
        .iter()
        .find(|r| r.announcement.pod_slug == "rev-b")
        .unwrap()
        .relevance;
    assert!(with_evidence.results.iter().any(|r| r
        .reasons
        .iter()
        .any(|reason| reason.contains("agent evidence"))));

    let owner = home.default_auth_context().unwrap();
    home.revoke_agent_harness(&owner, agent.harness_id.unwrap())
        .unwrap();

    let after_revoke = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 0).unwrap(),
        )
        .unwrap();
    assert!(after_revoke.results.iter().all(|r| r
        .reasons
        .iter()
        .all(|reason| !reason.contains("agent evidence"))));
    let after_score = after_revoke
        .results
        .iter()
        .find(|r| r.announcement.pod_slug == "rev-b")
        .unwrap()
        .relevance;
    assert!(
        after_score < boosted,
        "revoked grant evidence must leave ranking"
    );

    let new_submit = home.submit_pod_similarity_agent_evidence(
        &agent,
        SubmitPodSimilarityAgentEvidenceRequest {
            left_announcement_id: left.id,
            right_announcement_id: right.id,
            confidence: CandidateConfidence::new(0.95).unwrap(),
            explanation: "After revoke".into(),
            public_inputs: vec![
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: left.id,
                    origin_node_id: left.origin_node_id,
                    pod_slug: left.pod_slug.clone(),
                },
                PodSimilarityAgentEvidenceAnnouncementRef {
                    announcement_id: right.id,
                    origin_node_id: right.origin_node_id,
                    pod_slug: right.pod_slug.clone(),
                },
            ],
            model_provenance: "m".into(),
            harness_idempotency_key: "rev-2".into(),
            freshness_hours: Some(24),
        },
    );
    assert!(matches!(new_submit, Err(AgentToolsError::Forbidden { .. })));
}

#[test]
fn agent_evidence_survives_sqlite_restart_and_baseline_without_evidence_matches() {
    let origin_dir = TestDataDir::new("agent-persist-origin");
    let home_dir = TestDataDir::new("agent-persist-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let left_pod = create_public_pod(&origin, "persist-left", "Distributed systems left");
    let right_pod = create_public_pod(&origin, "persist-right", "Distributed systems right");
    let left = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &left_pod.slug,
            "https://origin.example/federation/pods/persist-left",
        )
        .unwrap();
    let right = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &right_pod.slug,
            "https://origin.example/federation/pods/persist-right",
        )
        .unwrap();
    home.index_pod_announcement(left.clone()).unwrap();
    home.index_pod_announcement(right.clone()).unwrap();
    let agent = harness(
        &home,
        "persist agent",
        vec![HarnessCapability::PodSimilarityEvidence],
    );
    // Explore as the same User the agent evidence will be scoped to.
    let mut reader = home.default_auth_context().unwrap();
    reader.user_id = agent.user_id;
    let baseline = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems", 10, 0).unwrap(),
        )
        .unwrap();
    let baseline_slugs: Vec<_> = baseline
        .results
        .iter()
        .map(|r| r.announcement.pod_slug.clone())
        .collect();
    let baseline_scores: Vec<_> = baseline.results.iter().map(|r| r.relevance).collect();

    let evidence = home
        .submit_pod_similarity_agent_evidence(
            &agent,
            SubmitPodSimilarityAgentEvidenceRequest {
                left_announcement_id: left.id,
                right_announcement_id: right.id,
                confidence: CandidateConfidence::new(0.75).unwrap(),
                explanation: "Persisted semantic link".into(),
                public_inputs: vec![
                    PodSimilarityAgentEvidenceAnnouncementRef {
                        announcement_id: left.id,
                        origin_node_id: left.origin_node_id,
                        pod_slug: left.pod_slug.clone(),
                    },
                    PodSimilarityAgentEvidenceAnnouncementRef {
                        announcement_id: right.id,
                        origin_node_id: right.origin_node_id,
                        pod_slug: right.pod_slug.clone(),
                    },
                ],
                model_provenance: "persist-model".into(),
                harness_idempotency_key: "persist-1".into(),
                freshness_hours: Some(48),
            },
        )
        .unwrap();

    // Restart Home Node from the same SQLite database.
    let harness_id = agent.harness_id.unwrap();
    let evidence_user = evidence.user_id;
    drop(home);
    let home = AgentTools::open_initialized_home_node(&home_dir.0).unwrap();
    {
        let store = home.store();
        let store = store.read().unwrap();
        let restored = store
            .pod_similarity_agent_evidence
            .get(&evidence.id)
            .expect("agent evidence survives SQLite restart");
        assert_eq!(restored.explanation, "Persisted semantic link");
        assert_eq!(restored.model_provenance, "persist-model");
        assert_eq!(restored.harness_idempotency_key, "persist-1");
        assert!(store.harness_write_audit.iter().any(|entry| {
            entry.operation == HarnessWriteOperation::SubmitPodSimilarityAgentEvidence
        }));
        let harness = store
            .agent_harnesses
            .get(&harness_id)
            .expect("submitting harness survives restart");
        assert!(harness.revoked_at.is_none());
        assert!(harness
            .grant
            .capabilities
            .contains(&HarnessCapability::PodSimilarityEvidence));
        assert!(agent_evidence_is_active(
            &store,
            restored,
            &TrustPolicy::new(restored.user_id, restored.tenant_id),
            Utc::now(),
        ));
    }
    let mut reader = home.default_auth_context().unwrap();
    reader.user_id = Some(evidence_user);
    let after = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems", 10, 0).unwrap(),
        )
        .unwrap();
    assert!(
        after.results.iter().any(|r| r
            .reasons
            .iter()
            .any(|reason| reason.contains("agent evidence"))),
        "expected agent evidence reasons after restart; results={:?}",
        after
            .results
            .iter()
            .map(|r| (&r.announcement.pod_slug, &r.reasons, r.relevance))
            .collect::<Vec<_>>()
    );

    // Fresh Home Node without agent evidence keeps the deterministic baseline set.
    let clean_dir = TestDataDir::new("agent-baseline-home");
    let clean = AgentTools::open_home_node(&clean_dir.0, seed_store).unwrap();
    clean.index_pod_announcement(left).unwrap();
    clean.index_pod_announcement(right).unwrap();
    // Clean node has no agent evidence; deterministic baseline must match.
    let clean_reader = harness(&clean, "clean reader", vec![HarnessCapability::FeedRead]);
    let clean_explore = clean
        .explore_public_pods(
            &clean_reader,
            ExploreRequest::new("distributed systems", 10, 0).unwrap(),
        )
        .unwrap();
    let clean_slugs: Vec<_> = clean_explore
        .results
        .iter()
        .map(|r| r.announcement.pod_slug.clone())
        .collect();
    let clean_scores: Vec<_> = clean_explore.results.iter().map(|r| r.relevance).collect();
    assert_eq!(clean_slugs, baseline_slugs);
    assert_eq!(clean_scores, baseline_scores);
    assert!(clean_explore.results.iter().all(|r| r
        .reasons
        .iter()
        .all(|reason| !reason.contains("agent evidence"))));
}

#[test]
fn agent_evidence_respects_blocks_and_caps_after_layering() {
    let origin_dir = TestDataDir::new("agent-caps-origin");
    let home_dir = TestDataDir::new("agent-caps-home");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let mut announcements = Vec::new();
    for slug in ["cap-agent-a", "cap-agent-b", "cap-agent-c", "cap-agent-d"] {
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
        home.index_pod_announcement(announcement.clone()).unwrap();
        announcements.push(announcement);
    }
    let agent = harness(
        &home,
        "cap agent",
        vec![HarnessCapability::PodSimilarityEvidence],
    );
    let anchor = &announcements[0];
    for target in &announcements[1..] {
        home.submit_pod_similarity_agent_evidence(
            &agent,
            SubmitPodSimilarityAgentEvidenceRequest {
                left_announcement_id: anchor.id,
                right_announcement_id: target.id,
                confidence: CandidateConfidence::new(1.0).unwrap(),
                explanation: format!("Boost {}", target.pod_slug),
                public_inputs: vec![
                    PodSimilarityAgentEvidenceAnnouncementRef {
                        announcement_id: anchor.id,
                        origin_node_id: anchor.origin_node_id,
                        pod_slug: anchor.pod_slug.clone(),
                    },
                    PodSimilarityAgentEvidenceAnnouncementRef {
                        announcement_id: target.id,
                        origin_node_id: target.origin_node_id,
                        pod_slug: target.pod_slug.clone(),
                    },
                ],
                model_provenance: "cap-model".into(),
                harness_idempotency_key: format!("cap-{}", target.pod_slug),
                freshness_hours: Some(24),
            },
        )
        .unwrap();
    }
    // Block one heavily boosted pod; blocks apply before ranking.
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockPod {
            origin_node_id: announcements[1].origin_node_id,
            pod_slug: announcements[1].pod_slug.clone(),
        },
    );
    let reader = harness(&home, "cap agent reader", vec![HarnessCapability::FeedRead]);
    let explored = home
        .explore_public_pods(
            &reader,
            ExploreRequest::new("distributed systems research", 10, 0).unwrap(),
        )
        .unwrap();
    assert!(explored
        .results
        .iter()
        .all(|r| r.announcement.pod_slug != announcements[1].pod_slug));
    assert!(
        explored.results.len() <= MAX_RESULTS_PER_ORIGIN,
        "caps apply after agent evidence; got {}",
        explored.results.len()
    );
}
