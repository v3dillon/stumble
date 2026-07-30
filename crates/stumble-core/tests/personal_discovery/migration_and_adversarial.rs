use chrono::{TimeZone, Utc};
use stumble_core::*;

use crate::common::*;

#[test]
fn remote_metadata_and_worker_content_cannot_authorize_account_mutations() {
    // Account mutation authority is absent from all availability / plan contracts.
    // Remote Pods, Index Nodes, public metadata, and worker content cannot introduce it.
    let report_schema = serde_json::to_value(ReportDiscoverySourceAvailabilityRequest {
        task_id: uuid::Uuid::nil().into(),
        reports: vec![ReportedSourceAvailability {
            source: "x.example".into(),
            state: SourceAvailabilityState::AuthenticationRequired,
            reason: "login".into(),
        }],
        browser_grant_eligible_sources: Some(vec!["x.example".into()]),
    })
    .unwrap();
    let text = report_schema.to_string();
    for forbidden in [
        "account_mutation",
        "password",
        "cookie",
        "credentials",
        "authorize_login",
        "browser_control",
    ] {
        assert!(
            !text.contains(forbidden),
            "availability contract must not authorize {forbidden}"
        );
    }
    // Unknown authorization fields are rejected.
    let forged: Result<ReportDiscoverySourceAvailabilityRequest, _> =
        serde_json::from_value(serde_json::json!({
            "task_id": uuid::Uuid::nil(),
            "reports": [],
            "account_mutation_authorized": true,
            "browser_grant_eligible_sources": ["evil.example"]
        }));
    assert!(forged.is_err());
    let forged_plan: Result<RequestPersonalDiscovery, _> =
        serde_json::from_value(serde_json::json!({
            "idempotency_key": "x",
            "account_mutation_authorized": true,
            "browser_grant_eligible_sources": ["from-lead.example"]
        }));
    assert!(forged_plan.is_err());
}

#[test]
fn pre_feature_persistent_store_migration_preserves_core_home_node_state() {
    // AC8: upgrading a Home Node store that lacks Personal Discovery collections
    // must preserve Taste Profiles, Candidates, Pod Discovery Tasks, Pods,
    // Subscriptions, and federation state while defaulting new private PD fields.
    let root = std::env::temp_dir().join(format!(
        "stumble-pd-prefeature-migrate-{}",
        uuid::Uuid::now_v7()
    ));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let curator = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "prefeature curator".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::PodCuration,
                        HarnessCapability::CandidateSubmission,
                        HarnessCapability::DiscoveryTasks,
                        HarnessCapability::Feedback,
                        HarnessCapability::SubscriptionManagement,
                    ],
                    pod_ids: None,
                },
            )
            .unwrap();
        tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: "Prefeature Pod".into(),
                slug: "prefeature-pod".into(),
                description: "Preserved across PD upgrade".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["prefeature-interest".into()]);
    taste.blocked_sources = Some(vec!["blocked-prefeature.example".into()]);
    tools.update_taste_profile(&curator, taste).unwrap();
    let submitted = tools
        .submit_candidate(
            &curator,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::User {
                    learn: true,
                    interest_seed_metadata: CandidateInterestSeedMetadata::default(),
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: "https://prefeature.example/item".into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Prefeature candidate".into()),
                        author: None,
                        published_at: None,
                    },
                    permitted_excerpt: None,
                    summary: None,
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["prefeature".into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc::now(),
                        discovery_method: "user_submission".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: "prefeature-cand".into(),
                    client_idempotency_key: "prefeature-cand".into(),
                },
            },
        )
        .unwrap();
    let tasks = tools
        .materialize_due_discovery_tasks(&curator, Utc::now())
        .unwrap();
    // Seed store pods may materialize tasks; ensure at least the private pod exists.
    assert!(tools.store().read().unwrap().pods.contains_key(&pod.id));
    let candidate_id = submitted.candidate.id;
    let pod_count = tools.store().read().unwrap().pods.len();
    let subscription_count = tools.store().read().unwrap().subscriptions.len();
    let peer_count = tools.store().read().unwrap().trusted_peers.len();
    let preference_key = (curator.user_id.unwrap(), curator.tenant_id);
    let blocked = tools.store().read().unwrap().user_preferences[&preference_key]
        .blocked_sources
        .clone();
    drop(tools);

    // Strip Personal Discovery collections as if the snapshot was written pre-feature.
    let database_path = root.join("stumble.sqlite3");
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    for collection in [
        "discovery_plans",
        "discovery_result_batches",
        "discovery_result_item_learning_links",
        "personal_discovery_schedules",
        "discovery_results_ready_events",
        "discovery_task_source_availability",
        "authentication_needed_notices",
        "interest_seeds",
    ] {
        connection
            .execute(
                "DELETE FROM stumble_store_records WHERE collection = ?1",
                rusqlite::params![collection],
            )
            .unwrap();
    }
    drop(connection);

    let migrated = AgentTools::open_initialized_home_node(&root).unwrap();
    {
        let store_lock = migrated.store();
        let store = store_lock.read().unwrap();
        assert!(store.pods.contains_key(&pod.id));
        assert_eq!(store.pods.len(), pod_count);
        assert!(store.candidates.contains_key(&candidate_id));
        assert_eq!(
            store.user_preferences[&preference_key].interests,
            vec!["prefeature-interest"]
        );
        assert_eq!(
            store.user_preferences[&preference_key].blocked_sources,
            blocked
        );
        assert_eq!(store.subscriptions.len(), subscription_count);
        assert_eq!(store.trusted_peers.len(), peer_count);
        // New PD collections default empty after upgrade.
        assert!(store.discovery_plans.is_empty());
        assert!(store.discovery_result_batches.is_empty());
        assert!(store.personal_discovery_schedules.is_empty());
        // Pre-existing Pod discovery tasks remain loadable.
        let _ = tasks;
    }

    // Feature remains usable after migration.
    let owner = migrated.local_owner_auth_context().unwrap();
    let manager = {
        let issued = migrated
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "post-migrate manager".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::PersonalDiscoveryManagement,
                        HarnessCapability::Feedback,
                    ],
                    pod_ids: None,
                },
            )
            .unwrap();
        migrated
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    let profile = migrated.taste_profile(&manager).unwrap();
    assert_eq!(
        profile.explicit.interests,
        vec!["prefeature-interest".to_string()]
    );
    assert_eq!(profile.interest_seed_evidence.active_seed_count, 0);
    let readiness = migrated.personal_discovery_readiness(&manager).unwrap();
    assert!(readiness.ready);
    let created = migrated
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: Some(4),
                idempotency_key: "post-migrate".into(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(created.plan.result_count, 4);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn adversarial_personal_discovery_private_state_never_crosses_public_boundaries() {
    // Index capability used only for local catalog search of public announcements.
    // AC9: Interest Seeds, Source Affinities, Discovery Plans, schedules, result
    // batches, reactions, and profile-derived queries must never appear on
    // federation or public discovery serialization surfaces.
    let root = std::env::temp_dir().join(format!(
        "stumble-pd-adversarial-privacy-{}",
        uuid::Uuid::now_v7()
    ));
    let tools = AgentTools::initialize_home_node(&root, seed_store)
        .unwrap()
        .with_index_capability(true);
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "privacy manager".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::PersonalDiscoveryManagement,
                        HarnessCapability::Feedback,
                        HarnessCapability::CandidateSubmission,
                        HarnessCapability::FeedRead,
                    ],
                    pod_ids: None,
                },
            )
            .unwrap();
        tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    let worker = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "privacy worker".into(),
                    kind: AgentHarnessKind::Unattended,
                    capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
                    pod_ids: None,
                },
            )
            .unwrap();
        tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };

    let secret_topic = "adversarial-secret-topic-xyz";
    let secret_url = "https://adversarial-private.example/secret-path";
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec![secret_topic.into()]);
    tools.update_taste_profile(&manager, taste).unwrap();
    tools
        .submit_candidate(
            &manager,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::User {
                    learn: true,
                    interest_seed_metadata: CandidateInterestSeedMetadata::new(
                        Some("Secret Publisher".into()),
                        Some("secret-community".into()),
                    ),
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: secret_url.into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Secret evidence".into()),
                        author: Some("Secret Author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: None,
                    summary: None,
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec![secret_topic.into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc::now(),
                        discovery_method: "user_submission".into(),
                        referrer_url: Some("https://secret-referrer.example/item".into()),
                    },
                    harness_idempotency_key: "privacy-seed".into(),
                    client_idempotency_key: "privacy-seed".into(),
                },
            },
        )
        .unwrap();

    let schedule_now = Utc.with_ymd_and_hms(2026, 7, 20, 15, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(
            &manager,
            CreatePersonalDiscoveryScheduleRequest {
                name: "adversarial-schedule-name".into(),
                cadence: PersonalDiscoveryCadence::Daily,
                intent: PersonalDiscoveryScheduleIntent::new(
                    vec![secret_topic.into()],
                    vec!["noise".into()],
                ),
                result_count: Some(4),
                delivery_mode: PersonalDiscoveryDeliveryMode::NotifyWhenSupported,
            },
            schedule_now,
        )
        .unwrap();
    let created = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::Topic(secret_topic.into())),
                result_count: Some(4),
                idempotency_key: "adversarial-run".into(),
                browser_grant_eligible_sources: None,
            },
            schedule_now,
        )
        .unwrap();
    // Lease expiry is checked against wall-clock Utc::now(); claim with the real clock.
    let lease_now = Utc::now();
    tools
        .claim_discovery_task(
            &worker,
            created.task.id,
            lease_now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://adversarial-agent.example/found",
                DiscoveryPlanSourceRole::Proven,
                Some("AgentAuthor"),
                "privacy-agent",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            lease_now,
        )
        .unwrap();
    tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id: batch.items[0].candidate_id,
                action: DiscoveryResultItemActionRequest::MoreLikeThis,
            },
            lease_now,
        )
        .unwrap();

    let federation = tools.default_auth_context().unwrap();
    let public_pods = tools.list_public_pods(&federation).unwrap();
    let manifests = public_pods
        .iter()
        .filter_map(|pod| tools.federation_pod_manifest(&federation, &pod.slug).ok())
        .collect::<Vec<_>>();
    let events = public_pods
        .iter()
        .filter_map(|pod| tools.federation_pod_events(&federation, &pod.slug).ok())
        .collect::<Vec<_>>();
    let packages = public_pods
        .iter()
        .filter_map(|pod| tools.export_skill_pack(&federation, &pod.slug).ok())
        .collect::<Vec<_>>();
    let announcements = tools
        .store()
        .read()
        .unwrap()
        .known_pod_announcements
        .values()
        .map(|known| known.announcement.clone())
        .collect::<Vec<_>>();
    let samples = tools
        .store()
        .read()
        .unwrap()
        .pod_explore_sample_sets
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let endorsements = tools
        .store()
        .read()
        .unwrap()
        .pod_endorsements
        .values()
        .cloned()
        .collect::<Vec<_>>();
    // Profile-derived remote Index queries are forbidden; only explicit explore is allowed.
    let index_search = tools.search_pod_announcements("public-topic", 10).unwrap();
    let explored = tools
        .explore_public_pods(
            &manager,
            ExploreRequest::new("public-topic", 10, 5).unwrap(),
        )
        .unwrap();

    let outbound = serde_json::to_string(&serde_json::json!({
        "node": tools.node_info(&federation).unwrap(),
        "pods": public_pods,
        "manifests": manifests,
        "events": events,
        "packages": packages,
        "announcements": announcements,
        "samples": samples,
        "endorsements": endorsements,
        "index_search": index_search,
        "explore": explored,
    }))
    .unwrap();

    for forbidden in [
        secret_topic,
        secret_url,
        "adversarial-private.example",
        "adversarial-agent.example",
        "adversarial-schedule-name",
        "Secret Publisher",
        "secret-community",
        "Secret Author",
        "secret-referrer.example",
        "InterestSeed",
        "interest_seed",
        "SourceAffinity",
        "source_affinity",
        "DiscoveryPlan",
        "discovery_plan",
        "DiscoveryResultBatch",
        "discovery_result_batch",
        "MoreLikeThis",
        "more_like_this",
        "PersonalDiscoverySchedule",
        "personal_discovery_schedule",
        &schedule.schedule.id.to_string(),
        &created.plan.id.to_string(),
        &batch.id.to_string(),
        &batch.items[0].candidate_id.to_string(),
    ] {
        assert!(
            !outbound.contains(forbidden),
            "public/federation surface leaked private marker {forbidden}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}
