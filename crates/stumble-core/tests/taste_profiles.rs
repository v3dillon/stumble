use chrono::{TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-taste-profile-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn feedback_harness(tools: &AgentTools) -> (AuthContext, HarnessToken) {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "Taste Profile user".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Feedback, HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .unwrap();
    let context = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    (context, issued.token)
}

fn harness(tools: &AgentTools, label: &str, capabilities: Vec<HarnessCapability>) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
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

fn unattended_harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Unattended,
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

fn accepted_item(
    tools: &AgentTools,
    slug: &str,
    ordinal: usize,
    source: &str,
    tags: Vec<String>,
) -> (Pod, ContentItemId) {
    let curator = harness(
        tools,
        &format!("curator-{ordinal}"),
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::SubscriptionManagement,
        ],
    );
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: slug.into(),
                slug: slug.into(),
                description: "Taste learning Pod".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    tools.join_pod(&curator, &pod.slug).unwrap();
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitter = harness(
        tools,
        &format!("submitter-{ordinal}"),
        vec![HarnessCapability::CandidateSubmission],
    );
    let submitted = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Strong match".into(),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://{source}/{ordinal}"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Item {ordinal}")),
                        author: None,
                        published_at: None,
                    },
                    permitted_excerpt: None,
                    summary: Some(format!("A report about {}", tags.join(" "))),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags,
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
                        discovery_method: "interactive_search".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: format!("taste-harness-{ordinal}"),
                    client_idempotency_key: format!("taste-client-{ordinal}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    let placement = tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
    (pod, placement.content_item_id.unwrap())
}

#[test]
fn user_can_inspect_and_edit_explicit_taste_preferences() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (user, token) = feedback_harness(&tools);

    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["Rust".into(), "rust".into()]);
    update.blocked_topics = Some(vec!["clickbait".into()]);
    update.blocked_sources = Some(vec!["noise.example".into()]);
    update.recurrence_penalty_days = Some(RecurrencePenaltyDays::new(14).unwrap());
    tools.update_taste_profile(&user, update).unwrap();
    let profile = tools.taste_profile(&user).unwrap();

    assert_eq!(profile.explicit.interests, vec!["Rust"]);
    assert_eq!(profile.explicit.blocked_topics, vec!["clickbait"]);
    assert_eq!(profile.explicit.blocked_sources, vec!["noise.example"]);
    assert_eq!(profile.explicit.recurrence_penalty_days, 14);
    assert!(profile.learned.is_empty());
    assert_eq!(
        tools
            .get_feed_batch(
                &user,
                FeedBatchRequest::new(1).unwrap(),
                Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
            )
            .unwrap()
            .recurrence_penalty_days,
        14
    );
    let exact_override = tools
        .complete_feed_batch(
            &user,
            tools
                .get_feed_batch(
                    &user,
                    FeedBatchRequest::new(1).unwrap(),
                    Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                )
                .unwrap()
                .id,
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 1).unwrap(),
        )
        .unwrap();
    assert!(exact_override.completed_at.is_some());
    let mut override_request = FeedBatchRequest::new(1).unwrap();
    override_request.recurrence_penalty_days = Some(RecurrencePenaltyDays::new(30).unwrap());
    assert_eq!(
        tools
            .get_feed_batch(
                &user,
                override_request,
                Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 2).unwrap(),
            )
            .unwrap()
            .recurrence_penalty_days,
        30
    );

    drop(tools);
    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    assert_eq!(
        reopened
            .taste_profile(
                &reopened
                    .authenticate_token(token.expose())
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
        profile
    );
}

#[test]
fn legacy_preferences_can_be_edited_after_sqlite_restart() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (user, token) = feedback_harness(&tools);
    let user_id = user.user_id.unwrap();
    drop(tools);

    let database_path = data_dir.0.join("stumble.sqlite3");
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    let record_key =
        serde_json::to_string(&[serde_json::json!(user_id), serde_json::Value::Null]).unwrap();
    let value_json: String = connection
        .query_row(
            "SELECT value_json FROM stumble_store_records
             WHERE collection = 'user_preferences' AND record_key = ?1",
            [&record_key],
            |row| row.get(0),
        )
        .unwrap();
    let mut legacy: serde_json::Value = serde_json::from_str(&value_json).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("blocked_source_affinities");
    connection
        .execute(
            "UPDATE stumble_store_records SET value_json = ?1
             WHERE collection = 'user_preferences' AND record_key = ?2",
            rusqlite::params![serde_json::to_string(&legacy).unwrap(), record_key],
        )
        .unwrap();
    drop(connection);

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let user = reopened
        .authenticate_token(token.expose())
        .unwrap()
        .unwrap();
    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["consciousness".into()]);
    let profile = reopened.update_taste_profile(&user, update).unwrap();

    assert_eq!(profile.explicit.interests, vec!["consciousness"]);
    let connection = rusqlite::Connection::open(database_path).unwrap();
    let migrated: String = connection
        .query_row(
            "SELECT value_json FROM stumble_store_records
             WHERE collection = 'user_preferences' AND record_key = ?1",
            [&record_key],
            |row| row.get(0),
        )
        .unwrap();
    assert!(migrated.contains("\"blocked_source_affinities\""));
}

#[test]
fn legacy_preferences_are_persisted_canonically_after_snapshot_load() {
    let data_dir = TestDataDir::new();
    let snapshot_path = data_dir.0.join("store.json");
    let tools = AgentTools::new(seed_store());
    let (user, _) = feedback_harness(&tools);
    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["consciousness".into()]);
    tools.update_taste_profile(&user, update).unwrap();
    save_store_snapshot(&tools.store().read().unwrap(), &snapshot_path).unwrap();

    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&snapshot_path).unwrap()).unwrap();
    snapshot["user_preferences"][0]
        .as_object_mut()
        .unwrap()
        .remove("blocked_source_affinities");
    std::fs::write(
        &snapshot_path,
        serde_json::to_vec_pretty(&snapshot).unwrap(),
    )
    .unwrap();

    load_store_snapshot(&snapshot_path).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_slice(&std::fs::read(snapshot_path).unwrap()).unwrap();
    assert!(migrated["user_preferences"][0]["blocked_source_affinities"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn corroborated_feedback_learns_explainable_weights_and_weak_signal_does_not_rank() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (_, first_id) = accepted_item(
        &tools,
        "learn-one",
        101,
        "research.example",
        vec!["rust".into()],
    );
    let (_, second_id) = accepted_item(
        &tools,
        "learn-two",
        102,
        "other.example",
        vec!["rust".into()],
    );
    let user = harness(
        &tools,
        "learning user",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let batch = tools
        .get_feed_batch(&user, FeedBatchRequest::new(100).unwrap(), now)
        .unwrap();

    tools
        .record_feed_feedback(&user, first_id, FeedbackKind::Interesting, None, None, now)
        .unwrap();
    let weak = tools.taste_profile(&user).unwrap();
    let weak_rust = weak
        .learned
        .iter()
        .find(|weight| weight.signal == LearnedTasteSignal::Topic("rust".into()))
        .unwrap();
    assert_eq!(weak_rust.supporting_signals, 1);
    assert_eq!(weak_rust.weight, 0.0);

    tools
        .record_feed_feedback(&user, second_id, FeedbackKind::Saved, None, None, now)
        .unwrap();
    let learned = tools.taste_profile(&user).unwrap();
    let rust = learned
        .learned
        .iter()
        .find(|weight| weight.signal == LearnedTasteSignal::Topic("rust".into()))
        .unwrap();
    assert!(rust.weight > 0.0);
    assert_eq!(rust.supporting_signals, 2);
    assert_eq!(rust.evidence_summary.len(), 2);
    assert!(!serde_json::to_string(rust)
        .unwrap()
        .contains(&first_id.to_string()));

    drop(tools);
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    assert_eq!(tools.taste_profile(&user).unwrap(), learned);

    tools.complete_feed_batch(&user, batch.id, now).unwrap();
    let next = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(100)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
    let explained = next
        .items
        .iter()
        .find(|item| item.content_reference.content_item_id == first_id)
        .unwrap();
    assert!(explained
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| {
            reason
                == "Learned topic 'rust' affinity increased value from 2 relevant signals (2 supporting, 0 opposing)"
        }));
}

#[test]
fn unattended_harness_cannot_record_learning_feedback() {
    let tools = AgentTools::new(seed_store());
    let (_, item_id) = accepted_item(
        &tools,
        "unattended-learning",
        150,
        "worker.example",
        vec!["automation".into()],
    );
    let worker = unattended_harness(
        &tools,
        "unattended learner",
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::PodCuration,
        ],
    );
    let now = Utc::now();
    let batch = tools
        .get_feed_batch(&worker, FeedBatchRequest::new(100).unwrap(), now)
        .unwrap();

    assert!(batch.items.iter().all(|item| {
        item.allowed_actions == vec![stumble_core::domain::FeedAllowedAction::AddToPod]
    }));

    assert!(matches!(
        tools.record_feed_feedback(
            &worker,
            item_id,
            FeedbackKind::Interesting,
            None,
            None,
            now,
        ),
        Err(AgentToolsError::Forbidden { reason }) if reason.contains("interactive")
    ));
    assert!(tools
        .save_link(&worker, SubmissionId::from(item_id))
        .is_err());
    assert!(tools
        .block_source(&worker, "worker.example".into())
        .is_err());
    assert!(tools.block_topic(&worker, "automation".into()).is_err());
    assert!(tools
        .update_preferences(
            &worker,
            UpdatePreferencesRequest {
                interests: Some(vec!["automation".into()]),
                blocked_topics: None,
                blocked_sources: None,
                preferred_brief_length: None,
                preferred_discovery_mode: None,
            },
        )
        .is_err());

    let curator = harness(
        &tools,
        "interactive target curator",
        vec![HarnessCapability::PodCuration],
    );
    let target = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: "Automation review".into(),
                slug: "automation-review".into(),
                description: "Review worker discoveries".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    tools
        .add_content_item_to_pod(
            &worker,
            AddContentItemToPodRequest::new(item_id, target.id, None).unwrap(),
            now,
        )
        .unwrap();
    let inspector = harness(
        &tools,
        "interactive profile inspector",
        vec![HarnessCapability::Feedback],
    );
    assert!(tools
        .taste_profile(&inspector)
        .unwrap()
        .learned
        .iter()
        .all(|weight| weight.signal != LearnedTasteSignal::Topic("automation".into())));
}

#[test]
fn add_to_pod_updates_local_learning_and_user_can_reset_some_or_all_weights() {
    let tools = AgentTools::new(seed_store());
    let (_, item_id) = accepted_item(
        &tools,
        "add-learning-source",
        201,
        "curated.example",
        vec!["systems".into()],
    );
    let user = harness(
        &tools,
        "curating user",
        vec![HarnessCapability::Feedback, HarnessCapability::PodCuration],
    );
    let target = tools
        .create_pod(
            &user,
            CreatePodRequest {
                name: "Personal systems".into(),
                slug: "personal-systems".into(),
                description: "User curation target".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();

    tools
        .add_content_item_to_pod(
            &user,
            AddContentItemToPodRequest::new(item_id, target.id, None).unwrap(),
            Utc::now(),
        )
        .unwrap();
    tools
        .add_content_item_to_pod(
            &user,
            AddContentItemToPodRequest::new(item_id, target.id, None).unwrap(),
            Utc::now(),
        )
        .unwrap();
    let learned = tools.taste_profile(&user).unwrap();
    assert!(learned.learned.iter().any(|weight| {
        weight.signal == LearnedTasteSignal::Topic("systems".into())
            && weight.weight == 0.0
            && weight.supporting_signals == 1
            && weight.evidence_summary.len() == 1
            && weight.evidence_summary[0].kind == LearnedTasteEvidenceKind::AddToPod
            && weight.evidence_summary[0].count == 1
    }));

    let source_only = tools
        .reset_learned_taste(
            &user,
            ResetLearnedTasteRequest::for_signal(LearnedTasteSignal::Topic("systems".into())),
        )
        .unwrap();
    assert!(!source_only
        .learned
        .iter()
        .any(|weight| matches!(weight.signal, LearnedTasteSignal::Topic(_))));
    assert!(source_only
        .source_affinities
        .iter()
        .any(|affinity| matches!(affinity.signal, SourceAffinitySignal::Source(_))));

    let empty = tools
        .reset_learned_taste(&user, ResetLearnedTasteRequest::all())
        .unwrap();
    assert!(empty.learned.is_empty());
}

#[test]
fn explicit_blocks_override_positive_learning_and_private_profile_never_exports() {
    let tools = AgentTools::new(seed_store());
    let (_, first_id) = accepted_item(
        &tools,
        "private-one",
        301,
        "private-learning.example",
        vec!["secret-topic".into()],
    );
    let (_, second_id) = accepted_item(
        &tools,
        "private-two",
        302,
        "another.example",
        vec!["secret-topic".into()],
    );
    let (_, unseen_id) = accepted_item(
        &tools,
        "private-unseen",
        303,
        "third.example",
        vec!["secret-topic".into()],
    );
    let user = harness(
        &tools,
        "privacy user",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let batch = tools
        .get_feed_batch(&user, FeedBatchRequest::new(100).unwrap(), now)
        .unwrap();
    tools
        .record_feed_feedback(&user, first_id, FeedbackKind::Interesting, None, None, now)
        .unwrap();
    tools
        .record_feed_feedback(&user, second_id, FeedbackKind::Saved, None, None, now)
        .unwrap();
    assert!(tools
        .taste_profile(&user)
        .unwrap()
        .learned
        .iter()
        .any(
            |weight| weight.signal == LearnedTasteSignal::Topic("secret-topic".into())
                && weight.weight > 0.0
        ));
    tools.complete_feed_batch(&user, batch.id, now).unwrap();

    let mut update = UpdateTasteProfileRequest::default();
    update.blocked_topics = Some(vec!["secret-topic".into()]);
    tools.update_taste_profile(&user, update).unwrap();
    let next = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(100)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now,
        )
        .unwrap();
    assert!(!next
        .items
        .iter()
        .any(|item| item.content_reference.content_item_id == unseen_id));

    let federation = tools.default_auth_context().unwrap();
    let public_pods = tools.list_public_pods(&federation).unwrap();
    let manifests = public_pods
        .iter()
        .map(|pod| {
            tools
                .federation_pod_manifest(&federation, &pod.slug)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let events = public_pods
        .iter()
        .map(|pod| tools.federation_pod_events(&federation, &pod.slug).unwrap())
        .collect::<Vec<_>>();
    let packages = public_pods
        .iter()
        .map(|pod| tools.export_skill_pack(&federation, &pod.slug).unwrap())
        .collect::<Vec<_>>();
    let public_artifacts = serde_json::to_string(&serde_json::json!({
        "node": tools.node_info(&federation).unwrap(),
        "pods": public_pods,
        "manifests": manifests,
        "events": events,
        "packages": packages,
    }))
    .unwrap();
    assert!(!public_artifacts.contains("secret-topic"));
    assert!(!public_artifacts.contains("private-learning.example"));
    assert!(!public_artifacts.contains("learned"));
}

#[test]
fn taste_profile_requires_unscoped_feedback_authority_and_honors_revocation() {
    let tools = AgentTools::new(seed_store());
    let (pod, _) = accepted_item(
        &tools,
        "taste-authority",
        401,
        "authority.example",
        vec!["authority".into()],
    );
    let scoped = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "scoped profile".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Feedback],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let scoped = tools
        .authenticate_token(scoped.token.expose())
        .unwrap()
        .unwrap();

    assert!(tools.taste_profile(&scoped).is_err());
    assert!(tools
        .update_taste_profile(&scoped, UpdateTasteProfileRequest::default())
        .is_err());
    assert!(tools
        .reset_learned_taste(&scoped, ResetLearnedTasteRequest::all())
        .is_err());

    let unattended = unattended_harness(
        &tools,
        "unattended profile",
        vec![HarnessCapability::Feedback],
    );
    assert!(matches!(
        tools.taste_profile(&unattended),
        Err(AgentToolsError::Forbidden { reason }) if reason.contains("interactive")
    ));
    assert!(tools
        .update_taste_profile(&unattended, UpdateTasteProfileRequest::default())
        .is_err());
    assert!(tools
        .reset_learned_taste(&unattended, ResetLearnedTasteRequest::all())
        .is_err());

    let (unscoped, _) = feedback_harness(&tools);
    assert!(tools.taste_profile(&unscoped).is_ok());
    tools
        .revoke_agent_harness(
            &tools.default_auth_context().unwrap(),
            unscoped.harness_id.unwrap(),
        )
        .unwrap();
    assert!(tools.taste_profile(&unscoped).is_err());
    assert!(tools
        .update_taste_profile(&unscoped, UpdateTasteProfileRequest::default())
        .is_err());
    assert!(tools
        .reset_learned_taste(&unscoped, ResetLearnedTasteRequest::all())
        .is_err());
}

#[test]
fn pod_scoped_feed_explanations_do_not_reveal_global_learned_evidence() {
    let tools = AgentTools::new(seed_store());
    let (allowed_pod, allowed_id) = accepted_item(
        &tools,
        "scoped-visible",
        451,
        "visible.example",
        vec!["shared-private-signal".into()],
    );
    let (_, learned_one) = accepted_item(
        &tools,
        "scoped-hidden-one",
        452,
        "hidden-one.example",
        vec!["shared-private-signal".into()],
    );
    let (_, learned_two) = accepted_item(
        &tools,
        "scoped-hidden-two",
        453,
        "hidden-two.example",
        vec!["shared-private-signal".into()],
    );
    let broad = harness(
        &tools,
        "broad learner",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
    );
    let now = Utc::now();
    let broad_batch = tools
        .get_feed_batch(&broad, FeedBatchRequest::new(100).unwrap(), now)
        .unwrap();
    tools
        .record_feed_feedback(
            &broad,
            learned_one,
            FeedbackKind::Interesting,
            None,
            None,
            now,
        )
        .unwrap();
    tools
        .record_feed_feedback(&broad, learned_two, FeedbackKind::Saved, None, None, now)
        .unwrap();
    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["shared-private-signal".into()]);
    tools.update_taste_profile(&broad, update).unwrap();
    tools
        .complete_feed_batch(&broad, broad_batch.id, now)
        .unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "scoped feed reader".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: Some(vec![allowed_pod.id]),
            },
        )
        .unwrap();
    let scoped = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();

    let batch = tools
        .get_feed_batch(
            &scoped,
            FeedBatchRequest::new(10)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + chrono::Duration::seconds(1),
        )
        .unwrap();
    let visible = batch
        .items
        .iter()
        .find(|item| item.content_reference.content_item_id == allowed_id)
        .unwrap();
    assert!(!visible
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason.contains("Learned")));
    assert!(!visible
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason.contains("Explicit interests")));
}

#[test]
fn explicit_interest_suppresses_conflicting_learned_aversion_explanation() {
    let tools = AgentTools::new(seed_store());
    let (_, first_id) = accepted_item(
        &tools,
        "conflict-one",
        501,
        "one.example",
        vec!["databases".into()],
    );
    let (_, second_id) = accepted_item(
        &tools,
        "conflict-two",
        502,
        "two.example",
        vec!["databases".into()],
    );
    let (_, unseen_id) = accepted_item(
        &tools,
        "conflict-unseen",
        503,
        "three.example",
        vec!["databases".into()],
    );
    let user = harness(
        &tools,
        "conflict user",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
    let batch = tools
        .get_feed_batch(&user, FeedBatchRequest::new(100).unwrap(), now)
        .unwrap();
    tools
        .record_feed_feedback(&user, first_id, FeedbackKind::NotForMe, None, None, now)
        .unwrap();
    tools
        .record_feed_feedback(&user, second_id, FeedbackKind::Dismissed, None, None, now)
        .unwrap();
    let weight = tools
        .taste_profile(&user)
        .unwrap()
        .learned
        .into_iter()
        .find(|weight| weight.signal == LearnedTasteSignal::Topic("databases".into()))
        .unwrap();
    assert_eq!(weight.opposing_signals, 2);
    assert!(weight.weight < 0.0);
    tools.complete_feed_batch(&user, batch.id, now).unwrap();
    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["databases".into()]);
    tools.update_taste_profile(&user, update).unwrap();

    let next = tools
        .get_feed_batch(
            &user,
            FeedBatchRequest::new(100)
                .unwrap()
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now,
        )
        .unwrap();
    let unseen = next
        .items
        .iter()
        .find(|item| item.content_reference.content_item_id == unseen_id)
        .unwrap();
    assert!(unseen
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason == "Explicit interests matched the Content Reference: databases"));
    assert!(unseen
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| {
            reason
                == "Explicit interest 'databases' overrode learned topic 'databases' aversion from 2 opposing signals"
        }));

    let reset = tools
        .reset_learned_taste(
            &user,
            ResetLearnedTasteRequest::for_signal(LearnedTasteSignal::Source("one.example".into())),
        )
        .unwrap();
    assert!(!reset
        .source_affinities
        .iter()
        .any(|affinity| { affinity.signal == SourceAffinitySignal::Source("one.example".into()) }));
}

#[test]
fn feedback_ignores_unaccepted_candidate_evidence_for_the_same_url() {
    let tools = AgentTools::new(seed_store());
    let (_, content_item_id) = accepted_item(
        &tools,
        "accepted-feedback-source",
        551,
        "accepted-feedback.example",
        vec!["trusted-topic".into()],
    );
    let curator = harness(
        &tools,
        "unaccepted evidence curator",
        vec![HarnessCapability::PodCuration],
    );
    let unrelated_pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: "Unaccepted evidence".into(),
                slug: "unaccepted-feedback-evidence".into(),
                description: "Evidence that must not train feedback".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let submitter = harness(
        &tools,
        "unaccepted evidence submitter",
        vec![HarnessCapability::CandidateSubmission],
    );
    tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: unrelated_pod.id,
                        reason: "Unaccepted proposal".into(),
                        confidence: CandidateConfidence::new(0.7).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: "https://accepted-feedback.example/551".into(),
                    source_metadata: CandidateSourceMetadata {
                        title: None,
                        author: Some("Injected Author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: None,
                    summary: None,
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["injected-topic".into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc::now(),
                        discovery_method: "unaccepted_agent_evidence".into(),
                        referrer_url: Some("https://injected-referrer.example/post".into()),
                    },
                    harness_idempotency_key: "unaccepted-feedback-worker".into(),
                    client_idempotency_key: "unaccepted-feedback-client".into(),
                },
            },
        )
        .unwrap();
    let user = harness(
        &tools,
        "feedback provenance user",
        vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
    );
    let now = Utc::now();
    tools
        .get_feed_batch(&user, FeedBatchRequest::new(100).unwrap(), now)
        .unwrap();
    tools
        .record_feed_feedback(
            &user,
            content_item_id,
            FeedbackKind::Interesting,
            None,
            None,
            now,
        )
        .unwrap();

    let profile = tools.taste_profile(&user).unwrap();
    assert!(!profile
        .learned
        .iter()
        .any(|weight| weight.signal == LearnedTasteSignal::Topic("injected-topic".into())));
    let affinities = profile.source_affinities;
    for injected in [
        SourceAffinitySignal::AuthorOrAccount("injected author".into()),
        SourceAffinitySignal::ReferrerContext("injected-referrer.example".into()),
    ] {
        assert!(!affinities
            .iter()
            .any(|affinity| affinity.signal == injected));
    }
}

#[test]
fn two_node_public_event_import_never_carries_origin_taste_profile() {
    let origin_dir = TestDataDir::new();
    let home_dir = TestDataDir::new();
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let now = Utc::now();
    let (origin_user, _) = feedback_harness(&origin);
    let mut update = UpdateTasteProfileRequest::default();
    update.interests = Some(vec!["origin-private-needle".into()]);
    origin.update_taste_profile(&origin_user, update).unwrap();
    let (_, learned_one) = accepted_item(
        &origin,
        "origin-learning-one",
        601,
        "origin-learning.example",
        vec!["origin-learned-topic".into()],
    );
    let (_, learned_two) = accepted_item(
        &origin,
        "origin-learning-two",
        602,
        "origin-other.example",
        vec!["origin-learned-topic".into()],
    );
    origin
        .get_feed_batch(&origin_user, FeedBatchRequest::new(100).unwrap(), now)
        .unwrap();
    origin
        .record_feed_feedback(
            &origin_user,
            learned_one,
            FeedbackKind::Interesting,
            None,
            None,
            now,
        )
        .unwrap();
    origin
        .record_feed_feedback(
            &origin_user,
            learned_two,
            FeedbackKind::Saved,
            None,
            None,
            now,
        )
        .unwrap();
    let proposer = harness(
        &origin,
        "public proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness(
        &origin,
        "public approver",
        vec![HarnessCapability::Approval],
    );
    let proposal = origin
        .create_pending_proposal(
            &proposer,
            SensitiveChange::CreatePublicPod {
                request: CreatePodRequest {
                    name: "Portable public Pod".into(),
                    slug: "portable-public-pod".into(),
                    description: "Public import privacy".into(),
                    visibility: Visibility::Public,
                },
            },
            now,
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    origin
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    let events = origin
        .export_pod_events(&proposer, "portable-public-pod")
        .unwrap();
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("origin-private-needle"));
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("origin-learned-topic"));
    let origin_info = origin.node_info(&proposer).unwrap();

    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let (home_user, _) = feedback_harness(&home);
    let home_profile_before = home.taste_profile(&home_user).unwrap();
    let administrator = harness(
        &home,
        "home administrator",
        vec![HarnessCapability::Administration],
    );
    let trust_approver = harness(
        &home,
        "home trust approver",
        vec![HarnessCapability::Approval],
    );
    let trust = home
        .request_add_trusted_peer(
            &administrator,
            "origin".into(),
            "https://origin.example".into(),
            origin_info.public_key,
            now,
        )
        .unwrap();
    home.approve_pending_proposal(&trust_approver, trust.id, now)
        .unwrap();
    let peer_id = home
        .store()
        .read()
        .unwrap()
        .trusted_peers
        .values()
        .find(|peer| peer.base_url == "https://origin.example")
        .unwrap()
        .id;
    home.import_pod_events(&administrator, peer_id, events)
        .unwrap();
    assert_eq!(home.taste_profile(&home_user).unwrap(), home_profile_before);
    let imported = home
        .export_pod_events(&administrator, "portable-public-pod")
        .unwrap();
    assert!(!serde_json::to_string(&imported)
        .unwrap()
        .contains("origin-private-needle"));
    assert!(!serde_json::to_string(&imported)
        .unwrap()
        .contains("origin-learned-topic"));
}
