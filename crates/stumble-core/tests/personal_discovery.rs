use chrono::{TimeZone, Utc};
use stumble_core::*;

fn harness(
    tools: &AgentTools,
    label: &str,
    kind: AgentHarnessKind,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
    let owner = tools.default_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind,
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

#[test]
fn generic_personal_discovery_requires_corroborated_or_explicit_taste() {
    let tools = AgentTools::new(seed_store());
    let manager = harness(
        &tools,
        "interactive manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::Feedback,
        ],
    );
    let mut empty = UpdateTasteProfileRequest::default();
    empty.interests = Some(Vec::new());
    tools.update_taste_profile(&manager, empty).unwrap();
    tools
        .reset_learned_taste(&manager, ResetLearnedTasteRequest::all())
        .unwrap();

    let readiness = tools.personal_discovery_readiness(&manager).unwrap();
    assert!(!readiness.ready);
    assert_eq!(readiness.basis, Vec::<DiscoveryPlanBasis>::new());

    let error = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "cold-start".into(),
            },
            Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap(),
        )
        .unwrap_err();
    assert!(matches!(error, AgentToolsError::PersonalDiscoveryNotReady));

    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["distributed systems".into()]);
    tools.update_taste_profile(&manager, taste).unwrap();
    assert!(tools.personal_discovery_readiness(&manager).unwrap().ready);
}

#[test]
fn request_creates_minimized_plan_and_first_class_personal_task() {
    let tools = AgentTools::new(seed_store());
    let manager = harness(
        &tools,
        "interactive manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::Feedback,
        ],
    );
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["Rust".into()]);
    taste.blocked_topics = Some(vec!["cryptocurrency".into()]);
    taste.blocked_sources = Some(vec!["blocked.example".into()]);
    tools.update_taste_profile(&manager, taste).unwrap();
    let now = Utc::now();

    let created = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "request-1".into(),
            },
            now,
        )
        .unwrap();

    assert_eq!(created.plan.result_count, 10);
    assert_eq!(created.plan.allocation.proven, 7);
    assert_eq!(created.plan.allocation.adjacent, 3);
    assert_eq!(created.plan.constraints.max_per_domain, 3);
    assert_eq!(created.plan.constraints.max_per_author_or_account, 2);
    assert_eq!(created.plan.constraints.max_per_publisher, 2);
    assert_eq!(created.plan.constraints.max_per_community, 2);
    let serialized_constraints = serde_json::to_value(&created.plan.constraints).unwrap();
    assert_eq!(serialized_constraints["max_per_domain"], 3);
    assert_eq!(serialized_constraints["max_per_author_or_account"], 2);
    assert_eq!(serialized_constraints["max_per_publisher"], 2);
    assert_eq!(serialized_constraints["max_per_community"], 2);
    assert!(serialized_constraints
        .get("max_per_source_neighborhood")
        .is_none());
    assert!(created.plan.constraints.canonical_deduplication);
    assert_eq!(
        created.plan.constraints.blocked_topics,
        vec!["cryptocurrency"]
    );
    assert_eq!(
        created.plan.constraints.blocked_sources,
        vec!["blocked.example"]
    );
    assert_eq!(created.task.target.pod(), None);
    assert_eq!(
        created.task.target.discovery_plan_id(),
        Some(created.plan.id)
    );
    assert!(matches!(
        created.task.origin,
        DiscoveryTaskOrigin::PersonalRequest { .. }
    ));
    assert!(tools.store().read().unwrap().pods.is_empty());

    let retried = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "request-1".into(),
            },
            now,
        )
        .unwrap();
    assert_eq!(retried, created);
}

#[test]
fn temporary_topic_supports_cold_start_and_worker_reads_only_its_claimed_plan() {
    let tools = AgentTools::new(seed_store());
    let manager = harness(
        &tools,
        "interactive manager",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PersonalDiscoveryManagement],
    );
    let worker = harness(
        &tools,
        "personal worker",
        AgentHarnessKind::Unattended,
        vec![HarnessCapability::PersonalDiscoveryExecution],
    );
    let pod_worker = harness(
        &tools,
        "pod worker",
        AgentHarnessKind::Unattended,
        vec![HarnessCapability::DiscoveryTasks],
    );
    let now = Utc::now();
    let created = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::Topic("type systems".into())),
                result_count: Some(4),
                idempotency_key: "topic-run".into(),
            },
            now,
        )
        .unwrap();

    assert_eq!(created.plan.result_count, 4);
    assert_eq!(created.plan.allocation.proven, 3);
    assert_eq!(created.plan.allocation.adjacent, 1);
    assert!(created
        .plan
        .topics
        .iter()
        .any(|topic| topic.value == "type systems" && topic.temporary));
    assert!(tools
        .list_ready_discovery_tasks(&worker, now)
        .unwrap()
        .iter()
        .any(|task| task.id == created.task.id));
    assert!(!tools
        .list_discovery_tasks(&pod_worker, now)
        .unwrap()
        .iter()
        .any(|task| task.id == created.task.id));
    assert!(matches!(
        tools.discovery_plan(&worker, created.plan.id),
        Err(AgentToolsError::TaskLeaseRequired)
    ));

    tools
        .claim_discovery_task(
            &worker,
            created.task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    assert_eq!(
        tools.discovery_plan(&worker, created.plan.id).unwrap(),
        created.plan
    );
    assert!(matches!(
        tools.taste_profile(&worker),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert_eq!(
        tools.discovery_plan(&manager, created.plan.id).unwrap(),
        created.plan
    );
}

#[test]
fn personal_plan_and_retry_identity_survive_restart() {
    let root = std::env::temp_dir().join(format!(
        "stumble-personal-plan-persistence-{}",
        uuid::Uuid::now_v7()
    ));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();
    let request = RequestPersonalDiscovery {
        intent: Some(PersonalDiscoveryIntent::Topic("ownership types".into())),
        result_count: Some(6),
        idempotency_key: "restart-safe".into(),
    };
    let created = tools
        .request_personal_discovery(&owner, request.clone(), now)
        .unwrap();
    drop(tools);

    let restarted = AgentTools::open_initialized_home_node(&root).unwrap();
    let owner = restarted.local_owner_auth_context().unwrap();
    assert_eq!(
        restarted.discovery_plan(&owner, created.plan.id).unwrap(),
        created.plan
    );
    assert_eq!(
        restarted
            .request_personal_discovery(&owner, request, now)
            .unwrap(),
        created
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_blocks_reject_conflicting_temporary_intent() {
    let tools = AgentTools::new(seed_store());
    let manager = harness(
        &tools,
        "blocking manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::Feedback,
        ],
    );
    let mut taste = UpdateTasteProfileRequest::default();
    taste.blocked_topics = Some(vec!["gambling".into()]);
    taste.blocked_sources = Some(vec!["blocked.example".into()]);
    tools.update_taste_profile(&manager, taste).unwrap();

    for (key, intent) in [
        (
            "blocked-topic",
            PersonalDiscoveryIntent::Topic("Gambling".into()),
        ),
        (
            "blocked-source",
            PersonalDiscoveryIntent::SimilarToUrl("https://blocked.example/article".into()),
        ),
    ] {
        assert!(matches!(
            tools.request_personal_discovery(
                &manager,
                RequestPersonalDiscovery {
                    intent: Some(intent),
                    result_count: None,
                    idempotency_key: key.into(),
                },
                Utc::now(),
            ),
            Err(AgentToolsError::Store(StoreError::Validation(_)))
        ));
    }
}

#[test]
fn credential_bearing_temporary_urls_never_enter_a_discovery_plan() {
    let tools = AgentTools::new(seed_store());
    let manager = harness(
        &tools,
        "credential safety manager",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PersonalDiscoveryManagement],
    );

    let error = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::SimilarToUrl(
                    "https://user:password@example.com/article".into(),
                )),
                result_count: None,
                idempotency_key: "credential-bearing-url".into(),
            },
            Utc::now(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        AgentToolsError::Store(StoreError::Validation(message))
            if message.contains("credentials")
    ));
    let store = tools.store();
    let store = store.read().unwrap();
    assert!(store.discovery_plans.is_empty());
    assert!(store.discovery_tasks.is_empty());
}

#[test]
fn temporary_url_query_and_fragment_secrets_are_not_persisted() {
    let tools = AgentTools::new(seed_store());
    let manager = harness(
        &tools,
        "url minimization manager",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PersonalDiscoveryManagement],
    );

    let created = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::SimilarToUrl(
                    "https://example.com/article?access_token=query-secret#token=fragment-secret"
                        .into(),
                )),
                result_count: None,
                idempotency_key: "url-secret-minimization".into(),
            },
            Utc::now(),
        )
        .unwrap();

    assert_eq!(
        created.plan.intent,
        Some(PersonalDiscoveryIntent::SimilarToUrl(
            "https://example.com/article".into()
        ))
    );
    let serialized = serde_json::to_string(&created.plan).unwrap();
    assert!(!serialized.contains("query-secret"));
    assert!(!serialized.contains("fragment-secret"));
}

#[test]
fn minimized_topic_selection_keeps_explicit_preferences_ahead_of_learned_signals() {
    let tools = AgentTools::new(seed_store());
    let manager = harness(
        &tools,
        "priority manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::Feedback,
        ],
    );
    let explicit = ["zeta", "epsilon", "delta", "gamma", "beta", "alpha"];
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(explicit.iter().map(|topic| (*topic).into()).collect());
    tools.update_taste_profile(&manager, taste).unwrap();

    let plan = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "bounded-priority".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;

    assert_eq!(plan.topics.len(), 5);
    assert!(plan
        .topics
        .iter()
        .all(|topic| topic.rationale == "explicit User interest"));
}

#[test]
fn personal_workers_cannot_observe_or_claim_another_users_task() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = harness(
        &tools,
        "first user manager",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PersonalDiscoveryManagement],
    );
    let created = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::Topic("privacy".into())),
                result_count: None,
                idempotency_key: "first-user".into(),
            },
            Utc::now(),
        )
        .unwrap();

    let other_user_id = uuid::Uuid::now_v7();
    tools.store().write().unwrap().users.insert(
        other_user_id,
        User {
            id: other_user_id,
            display_name: "Other User".into(),
            created_at: Utc::now(),
        },
    );
    let mut other_owner = owner;
    other_owner.user_id = Some(other_user_id);
    let issued = tools
        .register_agent_harness(
            &other_owner,
            RegisterAgentHarnessRequest {
                label: "other user worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
                pod_ids: None,
            },
        )
        .unwrap();
    let worker = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    let now = Utc::now();

    assert!(tools
        .list_discovery_tasks(&worker, now)
        .unwrap()
        .iter()
        .all(|task| task.id != created.task.id));
    assert!(matches!(
        tools.claim_discovery_task(
            &worker,
            created.task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        ),
        Err(AgentToolsError::Forbidden { .. })
    ));
}

#[test]
fn personal_request_idempotency_is_scoped_to_the_requesting_harness() {
    let tools = AgentTools::new(seed_store());
    let first = harness(
        &tools,
        "first manager",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PersonalDiscoveryManagement],
    );
    let second = harness(
        &tools,
        "second manager",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PersonalDiscoveryManagement],
    );
    let request = RequestPersonalDiscovery {
        intent: Some(PersonalDiscoveryIntent::Topic("idempotency".into())),
        result_count: None,
        idempotency_key: "ordinary-client-key".into(),
    };
    let first_created = tools
        .request_personal_discovery(&first, request.clone(), Utc::now())
        .unwrap();
    let second_created = tools
        .request_personal_discovery(&second, request.clone(), Utc::now())
        .unwrap();

    assert_ne!(first_created.task.id, second_created.task.id);
    assert_ne!(first_created.plan.id, second_created.plan.id);
    assert_eq!(
        tools
            .request_personal_discovery(&first, request, Utc::now())
            .unwrap(),
        first_created
    );
}

#[test]
fn invalid_personal_execution_grants_are_denied_at_the_core_list_boundary() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let pod_id = uuid::Uuid::now_v7();
    tools.store().write().unwrap().pods.insert(
        pod_id,
        Pod {
            id: pod_id,
            tenant_id: owner.tenant_id,
            name: "Scoped".into(),
            slug: "scoped".into(),
            description: "Authorization fixture".into(),
            visibility: Visibility::Private,
            created_by: owner.user_id,
            created_at: Utc::now(),
            origin_node_id: None,
        },
    );
    let invalid = [
        RegisterAgentHarnessRequest {
            label: "interactive executor".into(),
            kind: AgentHarnessKind::Interactive,
            capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
            pod_ids: None,
        },
        RegisterAgentHarnessRequest {
            label: "pod scoped executor".into(),
            kind: AgentHarnessKind::Unattended,
            capabilities: vec![HarnessCapability::PersonalDiscoveryExecution],
            pod_ids: Some(vec![pod_id]),
        },
    ];

    for request in invalid {
        let issued = tools.register_agent_harness(&owner, request).unwrap();
        let context = tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap();
        assert!(matches!(
            tools.list_discovery_tasks(&context, Utc::now()),
            Err(AgentToolsError::Forbidden { .. })
        ));
    }
}

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-personal-discovery-{label}-{}",
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

fn admin_harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
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
    let proposer = admin_harness(
        tools,
        &format!("{slug} proposer"),
        vec![HarnessCapability::PodCuration],
    );
    let approver = admin_harness(
        tools,
        &format!("{slug} approver"),
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

fn accept_public_item(
    tools: &AgentTools,
    pod: &Pod,
    suffix: &str,
    source_url: &str,
    tags: Vec<String>,
) {
    let submitter = admin_harness(
        tools,
        &format!("{suffix} submitter"),
        vec![HarnessCapability::CandidateSubmission],
    );
    let curator = admin_harness(
        tools,
        &format!("{suffix} curator"),
        vec![HarnessCapability::PodCuration],
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let candidate = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Directly concerns the Pod subject".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: source_url.into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Reference {suffix}")),
                        author: Some("Careful author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("A permitted sample excerpt".into()),
                    summary: Some("A useful public Content Reference".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags,
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                        discovery_method: "browser_search".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: format!("worker-{suffix}"),
                    client_idempotency_key: format!("client-{suffix}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, candidate.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &curator,
            candidate.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
}

fn approve_trust_policy_change(tools: &AgentTools, change: TrustPolicyChange) {
    let proposer = admin_harness(
        tools,
        "trust proposer",
        vec![HarnessCapability::Administration],
    );
    let approver = admin_harness(tools, "trust approver", vec![HarnessCapability::Approval]);
    let now = Utc::now();
    let proposal = tools
        .request_trust_policy_change(&proposer, change, now)
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
}

fn personal_manager(tools: &AgentTools) -> AuthContext {
    harness(
        tools,
        "network lead manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::Feedback,
            HarnessCapability::FeedRead,
        ],
    )
}

fn set_interest(tools: &AgentTools, manager: &AuthContext, topic: &str) {
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec![topic.into()]);
    tools.update_taste_profile(manager, taste).unwrap();
}

fn import_verified_network_metadata(home: &AgentTools) -> (PodAnnouncement, PodExploreSamples) {
    let origin_dir = TestDataDir::new("network-lead-origin");
    let index_dir = TestDataDir::new("network-lead-index");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store).unwrap();
    let pod = create_public_pod(
        &origin,
        "rust-systems",
        "Rust ownership and distributed systems",
    );
    accept_public_item(
        &origin,
        &pod,
        "network-allowed",
        "https://allowed.example/systems-research",
        vec!["systems".into(), "rust".into()],
    );
    accept_public_item(
        &origin,
        &pod,
        "network-blocked-source",
        "https://blocked.example/noise",
        vec!["systems".into()],
    );
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/rust-systems",
        )
        .unwrap();
    let samples = origin
        .pod_explore_samples(&origin.default_auth_context().unwrap(), &announcement, 10)
        .unwrap();
    let endorser = create_public_pod(
        &origin,
        "systems-curators",
        "Systems curators recommending careful research",
    );
    let endorser_announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &endorser.slug,
            "https://origin.example/federation/pods/systems-curators",
        )
        .unwrap();
    let curator = admin_harness(
        &origin,
        "network endorsement curator",
        vec![HarnessCapability::PodCuration],
    );
    let endorsement = origin
        .endorse_public_pod(
            &curator,
            &endorser_announcement,
            &announcement,
            "Careful systems research neighborhood".into(),
        )
        .unwrap();

    index.index_pod_announcement(announcement.clone()).unwrap();
    index
        .index_pod_announcement(endorser_announcement.clone())
        .unwrap();
    index.index_pod_endorsement(endorsement.clone()).unwrap();
    let search = index.search_pod_announcements("systems", 10).unwrap();

    approve_trust_policy_change(
        home,
        TrustPolicyChange::AddIndexNode {
            label: "network index".into(),
            base_url: "https://network-index.example".into(),
        },
    );
    approve_trust_policy_change(
        home,
        TrustPolicyChange::BlockSource {
            source: "blocked.example".into(),
        },
    );
    let reader = admin_harness(
        home,
        "network import reader",
        vec![HarnessCapability::FeedRead],
    );
    home.accept_index_search_results(&reader, "https://network-index.example", search)
        .unwrap();
    // Peer/direct retention of the endorser so endorsement binding remains current.
    home.index_pod_announcement(endorser_announcement).unwrap();
    home.accept_pod_explore_samples(&reader, samples.clone())
        .unwrap();
    home.index_pod_endorsement(endorsement).unwrap();
    (announcement, samples)
}

#[test]
fn local_public_content_references_produce_adjacent_network_leads() {
    let home_dir = TestDataDir::new("local-public-content-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    let local_pod = create_public_pod(
        &home,
        "local-systems",
        "Local distributed systems reading list",
    );
    accept_public_item(
        &home,
        &local_pod,
        "local-public-ref",
        "https://local-public.example/deep-dive",
        vec!["systems".into()],
    );
    home.index_pod_announcement(
        home.pod_announcement(
            &home.default_auth_context().unwrap(),
            &local_pod.slug,
            "https://home.example/federation/pods/local-systems",
        )
        .unwrap(),
    )
    .unwrap();

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "local-public-content".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;

    assert!(plan.source_neighborhoods.iter().any(|source| {
        source.signal == SourceAffinitySignal::Source("local-public.example".into())
            && source.role == DiscoveryPlanSourceRole::Adjacent
            && source.rationale.contains("local public Content Reference")
    }));
    assert!(plan.source_neighborhoods.iter().any(|source| {
        matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "local-systems"
        ) && source.role == DiscoveryPlanSourceRole::Adjacent
            && source.rationale.contains("Pod Announcement")
    }));
}

#[test]
fn verified_network_metadata_produces_adjacent_discovery_leads_with_provenance() {
    let home_dir = TestDataDir::new("network-leads-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    let subscription_count_before = home.store().read().unwrap().subscriptions.len();
    let (_announcement, samples) = import_verified_network_metadata(&home);

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "network-leads".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;

    let adjacent = plan
        .source_neighborhoods
        .iter()
        .filter(|source| source.role == DiscoveryPlanSourceRole::Adjacent)
        .collect::<Vec<_>>();
    assert!(!adjacent.is_empty());
    assert!(adjacent.iter().all(|source| {
        source.rationale.contains("verified public")
            || source.rationale.contains("local public Content Reference")
    }));
    assert!(adjacent.iter().any(|source| {
        source.signal == SourceAffinitySignal::Source("allowed.example".into())
            && source.rationale.contains("Explore sample")
    }));
    assert!(adjacent.iter().any(|source| {
        matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "rust-systems"
        ) && (source.rationale.contains("Pod Announcement")
            || source.rationale.contains("Pod Endorsement"))
    }));
    assert!(plan
        .source_neighborhoods
        .iter()
        .filter(|source| source.role == DiscoveryPlanSourceRole::Adjacent)
        .all(|source| !source.temporary));
    assert_eq!(plan.allocation.adjacent, 3);
    assert_eq!(
        home.store().read().unwrap().subscriptions.len(),
        subscription_count_before
    );
    // Explore samples retained locally still include blocked sources; the plan must not.
    assert!(samples
        .samples
        .iter()
        .any(|sample| sample.source == "blocked.example"));
    assert!(plan
        .source_neighborhoods
        .iter()
        .all(|source| { source.signal != SourceAffinitySignal::Source("blocked.example".into()) }));
}

#[test]
fn invalid_stale_blocked_untrusted_or_withdrawn_metadata_cannot_influence_plans() {
    let home_dir = TestDataDir::new("filtered-network-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    let (announcement, samples) = import_verified_network_metadata(&home);

    // Block the remote source domain through Trust Policy after import.
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockSource {
            source: "allowed.example".into(),
        },
    );
    approve_trust_policy_change(
        &home,
        TrustPolicyChange::BlockPod {
            origin_node_id: announcement.origin_node_id,
            pod_slug: announcement.pod_slug.clone(),
        },
    );

    let blocked_plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "blocked-network".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    assert!(blocked_plan.source_neighborhoods.iter().all(|source| {
        source.role != DiscoveryPlanSourceRole::Adjacent
            || (!matches!(
                &source.signal,
                SourceAffinitySignal::Source(domain) if domain == "allowed.example"
            ) && !matches!(
                &source.signal,
                SourceAffinitySignal::Community(slug) if slug == "rust-systems"
            ))
    }));

    // Remove Index: announcements retained only via that Index must drop out of new plans.
    let fresh_dir = TestDataDir::new("index-removed-home");
    let index_dir = TestDataDir::new("index-removed-index");
    let fresh = AgentTools::open_home_node(&fresh_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store).unwrap();
    let fresh_manager = personal_manager(&fresh);
    set_interest(&fresh, &fresh_manager, "distributed systems");
    index.index_pod_announcement(announcement.clone()).unwrap();
    let search = index.search_pod_announcements("systems", 10).unwrap();
    approve_trust_policy_change(
        &fresh,
        TrustPolicyChange::AddIndexNode {
            label: "ephemeral index".into(),
            base_url: "https://ephemeral-index.example".into(),
        },
    );
    let reader = admin_harness(
        &fresh,
        "ephemeral reader",
        vec![HarnessCapability::FeedRead],
    );
    fresh
        .accept_index_search_results(&reader, "https://ephemeral-index.example", search)
        .unwrap();
    // Stale/mismatched samples (wrong announcement binding) must not produce leads.
    let mut stale_samples = samples;
    stale_samples.announcement_id = uuid::Uuid::now_v7();
    assert!(fresh
        .accept_pod_explore_samples(&reader, stale_samples)
        .is_err());
    approve_trust_policy_change(
        &fresh,
        TrustPolicyChange::RemoveIndexNode {
            base_url: "https://ephemeral-index.example".into(),
        },
    );
    let after_index_removal = fresh
        .request_personal_discovery(
            &fresh_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "index-removed".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    assert!(after_index_removal
        .source_neighborhoods
        .iter()
        .all(|source| source.role != DiscoveryPlanSourceRole::Adjacent
            || !matches!(
                &source.signal,
                SourceAffinitySignal::Community(slug) if slug == "rust-systems"
            )));

    // Tampered (invalid signature) announcement cannot be retained.
    let mut tampered = announcement;
    tampered.subject = "attacker changed subject to systems".into();
    assert!(fresh.index_pod_announcement(tampered).is_err());

    // Withdrawn local public Accepted Placements cannot produce Content Reference leads.
    let local_withdraw_dir = TestDataDir::new("withdrawn-local-public-home");
    let local_home = AgentTools::open_home_node(&local_withdraw_dir.0, seed_store).unwrap();
    let local_manager = personal_manager(&local_home);
    set_interest(&local_home, &local_manager, "distributed systems");
    let local_pod = create_public_pod(
        &local_home,
        "withdrawn-local-systems",
        "Local distributed systems reading list",
    );
    accept_public_item(
        &local_home,
        &local_pod,
        "withdrawable-public-ref",
        "https://withdrawn-local.example/deep-dive",
        vec!["systems".into()],
    );
    local_home
        .index_pod_announcement(
            local_home
                .pod_announcement(
                    &local_home.default_auth_context().unwrap(),
                    &local_pod.slug,
                    "https://home.example/federation/pods/withdrawn-local-systems",
                )
                .unwrap(),
        )
        .unwrap();
    let before_withdrawal = local_home
        .request_personal_discovery(
            &local_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "before-local-withdrawal".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    assert!(
        before_withdrawal.source_neighborhoods.iter().any(|source| {
            matches!(
                &source.signal,
                SourceAffinitySignal::Source(domain) if domain == "withdrawn-local.example"
            ) && source.role == DiscoveryPlanSourceRole::Adjacent
        }),
        "local public Content Reference must produce a lead before withdrawal"
    );
    let content_item_id = {
        let store = local_home.store();
        let store = store.read().unwrap();
        store
            .accepted_placement_projections
            .keys()
            .find(|(_, pod_id)| *pod_id == local_pod.id)
            .map(|(content_item_id, _)| *content_item_id)
            .expect("local accepted placement exists")
    };
    let proposer = admin_harness(
        &local_home,
        "local withdrawal proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = admin_harness(
        &local_home,
        "local withdrawal approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let outcome = local_home
        .request_remove_submission_from_pod(&proposer, &local_pod.slug, content_item_id.into(), now)
        .unwrap();
    let RemoveSubmissionOutcome::PendingApproval(proposal) = outcome else {
        panic!("public placement withdrawal must require approval");
    };
    local_home
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    let after_local_withdrawal = local_home
        .request_personal_discovery(
            &local_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "withdrawn-local-public".into(),
            },
            now,
        )
        .unwrap()
        .plan;
    assert!(
        after_local_withdrawal
            .source_neighborhoods
            .iter()
            .all(|source| {
                !matches!(
                    &source.signal,
                    SourceAffinitySignal::Source(domain) if domain == "withdrawn-local.example"
                )
            }),
        "withdrawn local public Content Reference must not produce a network lead"
    );
}

#[test]
fn local_relevance_discards_remote_index_scores_and_autonomous_planning_is_local_only() {
    let home_dir = TestDataDir::new("local-relevance-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    let (announcement, _) = import_verified_network_metadata(&home);

    // Import a second announcement that would only match a private profile term if
    // we wrongly issued a remote query; it stays unmatched because subjects differ.
    let unrelated_dir = TestDataDir::new("unrelated-origin");
    let index_dir = TestDataDir::new("unrelated-index");
    let unrelated = AgentTools::open_home_node(&unrelated_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store).unwrap();
    let cooking = create_public_pod(
        &unrelated,
        "cooking-notes",
        "Weeknight pasta and baking techniques",
    );
    let cooking_announcement = unrelated
        .pod_announcement(
            &unrelated.default_auth_context().unwrap(),
            &cooking.slug,
            "https://unrelated.example/federation/pods/cooking-notes",
        )
        .unwrap();
    index
        .index_pod_announcement(cooking_announcement.clone())
        .unwrap();
    // Re-index the systems pod with a low remote score path; accept both via Index.
    index.index_pod_announcement(announcement.clone()).unwrap();
    let search = index.search_pod_announcements("", 10).unwrap();
    let reader = admin_harness(
        &home,
        "local relevance reader",
        vec![HarnessCapability::FeedRead],
    );
    home.accept_index_search_results(&reader, "https://network-index.example", search)
        .unwrap();

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "local-relevance".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;

    assert!(plan.source_neighborhoods.iter().any(|source| {
        matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "rust-systems"
        ) && source.role == DiscoveryPlanSourceRole::Adjacent
    }));
    assert!(plan.source_neighborhoods.iter().all(|source| {
        !matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "cooking-notes"
        )
    }));
    // Plan rationales stay aggregate and never leak private matching inputs.
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("matching_topics"));
    assert!(!serialized.contains("InterestSeed"));
    assert!(!serialized.contains("DiscoveryLead"));
    assert!(!serialized.contains("local_relevance"));
    assert!(!serialized.contains("TasteProfile"));
    for source in &plan.source_neighborhoods {
        if source.role == DiscoveryPlanSourceRole::Adjacent {
            assert!(
                source.rationale.starts_with("adjacent exploration from"),
                "unexpected rationale {}",
                source.rationale
            );
            assert!(!source.rationale.contains("distributed systems"));
        }
    }
}

#[test]
fn explicit_explore_query_remains_distinct_from_autonomous_personal_discovery() {
    let home_dir = TestDataDir::new("explore-distinct-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    import_verified_network_metadata(&home);

    let explored = home
        .explore_public_pods(
            &manager,
            ExploreRequest::new("rust ownership", 10, 5).unwrap(),
        )
        .unwrap();
    assert_eq!(explored.query, "rust ownership");
    assert!(!explored.results.is_empty());

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "explore-distinct".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    // Autonomous plan does not embed the explicit Explore query.
    let serialized = serde_json::to_string(&plan).unwrap();
    assert!(!serialized.contains("rust ownership"));
    assert_ne!(
        serde_json::to_value(&plan).unwrap()["intent"],
        serde_json::json!({"kind":"topic","value":"rust ownership"})
    );
}

#[test]
fn network_lead_selection_does_not_create_subscription_import_state_or_browser_authority() {
    let home_dir = TestDataDir::new("no-side-effects-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    import_verified_network_metadata(&home);
    let store_guard = home.store();
    let before = {
        let store = store_guard.read().unwrap();
        (
            store.subscriptions.len(),
            store.accepted_placement_projections.len(),
            store.agent_harnesses.len(),
            store.pods.len(),
        )
    };
    drop(store_guard);

    let created = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "no-side-effects".into(),
            },
            Utc::now(),
        )
        .unwrap();
    assert!(created
        .plan
        .source_neighborhoods
        .iter()
        .any(|source| source.role == DiscoveryPlanSourceRole::Adjacent));

    let store_guard = home.store();
    let after = {
        let store = store_guard.read().unwrap();
        (
            store.subscriptions.len(),
            store.accepted_placement_projections.len(),
            store.agent_harnesses.len(),
            store.pods.len(),
        )
    };
    assert_eq!(before.0, after.0);
    assert_eq!(before.1, after.1);
    assert_eq!(before.2, after.2);
    assert_eq!(before.3, after.3);
    assert!(created.task.target.pod().is_none());
}

#[test]
fn network_leads_fill_only_adjacent_allocation_unless_user_evidence_corroborates() {
    let home_dir = TestDataDir::new("adjacent-only-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "distributed systems");
    import_verified_network_metadata(&home);

    let plan = home
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: Some(10),
                idempotency_key: "adjacent-only".into(),
            },
            Utc::now(),
        )
        .unwrap()
        .plan;
    assert_eq!(plan.allocation.proven, 7);
    assert_eq!(plan.allocation.adjacent, 3);
    for source in &plan.source_neighborhoods {
        let is_network = source.rationale.contains("adjacent exploration from");
        if is_network {
            assert_eq!(source.role, DiscoveryPlanSourceRole::Adjacent);
        }
    }
    assert!(plan
        .source_neighborhoods
        .iter()
        .filter(|source| source.role == DiscoveryPlanSourceRole::Proven)
        .all(|source| !source.rationale.contains("adjacent exploration from")));
}

#[test]
fn restart_trust_and_equivalent_metadata_replacement_are_deterministic() {
    let adjacent_signals = |plan: &DiscoveryPlan| {
        plan.source_neighborhoods
            .iter()
            .filter(|source| source.role == DiscoveryPlanSourceRole::Adjacent)
            .map(|source| source.signal.clone())
            .collect::<Vec<_>>()
    };
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 15, 0, 0).unwrap();

    // Restart + idempotent retry preserve the full request and adjacent set.
    let home_dir = TestDataDir::new("deterministic-network-home");
    let home = AgentTools::initialize_home_node(&home_dir.0, seed_store).unwrap();
    let owner = home.local_owner_auth_context().unwrap();
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec!["distributed systems".into()]);
    home.update_taste_profile(&owner, taste).unwrap();
    let (announcement, _samples) = import_verified_network_metadata(&home);
    let first = home
        .request_personal_discovery(
            &owner,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "deterministic-network".into(),
            },
            now,
        )
        .unwrap();
    let first_adjacent = adjacent_signals(&first.plan);
    assert!(
        !first_adjacent.is_empty(),
        "seeded network metadata must produce adjacent leads before restart"
    );
    drop(home);

    let restarted = AgentTools::open_initialized_home_node(&home_dir.0).unwrap();
    let owner = restarted.local_owner_auth_context().unwrap();
    let retried = restarted
        .request_personal_discovery(
            &owner,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "deterministic-network".into(),
            },
            now,
        )
        .unwrap();
    assert_eq!(retried, first);
    assert_eq!(adjacent_signals(&retried.plan), first_adjacent);

    // Replacement of equivalent verified metadata: two independent Home Nodes that import
    // the same verified announcement/sample/endorsement set recompute the same adjacent signals.
    let equivalent_a_dir = TestDataDir::new("equivalent-meta-a");
    let equivalent_b_dir = TestDataDir::new("equivalent-meta-b");
    let equivalent_a = AgentTools::open_home_node(&equivalent_a_dir.0, seed_store).unwrap();
    let equivalent_b = AgentTools::open_home_node(&equivalent_b_dir.0, seed_store).unwrap();
    let manager_a = personal_manager(&equivalent_a);
    let manager_b = personal_manager(&equivalent_b);
    set_interest(&equivalent_a, &manager_a, "distributed systems");
    set_interest(&equivalent_b, &manager_b, "distributed systems");
    import_verified_network_metadata(&equivalent_a);
    import_verified_network_metadata(&equivalent_b);
    let plan_a = equivalent_a
        .request_personal_discovery(
            &manager_a,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "equivalent-a".into(),
            },
            now,
        )
        .unwrap()
        .plan;
    let plan_b = equivalent_b
        .request_personal_discovery(
            &manager_b,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "equivalent-b".into(),
            },
            now,
        )
        .unwrap()
        .plan;
    assert_eq!(
        adjacent_signals(&plan_a),
        adjacent_signals(&plan_b),
        "equivalent verified metadata must recompute the same adjacent signals"
    );
    assert!(
        !adjacent_signals(&plan_a).is_empty(),
        "equivalent import must still produce network adjacent leads"
    );

    // After a Trust Policy mutation, two independent recomputes agree.
    approve_trust_policy_change(
        &equivalent_a,
        TrustPolicyChange::BlockSource {
            source: "allowed.example".into(),
        },
    );
    let after_trust_a = equivalent_a
        .request_personal_discovery(
            &manager_a,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "deterministic-trust-a".into(),
            },
            now,
        )
        .unwrap()
        .plan;
    let after_trust_b = equivalent_a
        .request_personal_discovery(
            &manager_a,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "deterministic-trust-b".into(),
            },
            now,
        )
        .unwrap()
        .plan;
    assert_eq!(
        adjacent_signals(&after_trust_a),
        adjacent_signals(&after_trust_b),
        "trust-policy recomputes must be deterministic"
    );
    assert!(
        after_trust_a.source_neighborhoods.iter().all(|source| {
            source.role != DiscoveryPlanSourceRole::Adjacent
                || !matches!(
                    &source.signal,
                    SourceAffinitySignal::Source(domain) if domain == "allowed.example"
                )
        }),
        "blocked source must stay excluded after trust change"
    );

    // Index removal: Index-only retained communities drop, and two recomputes agree.
    let index_dir = TestDataDir::new("deterministic-index-only");
    let index_only_home_dir = TestDataDir::new("deterministic-index-removal-home");
    let index = AgentTools::open_home_node(&index_dir.0, seed_store).unwrap();
    let index_home = AgentTools::open_home_node(&index_only_home_dir.0, seed_store).unwrap();
    let index_manager = personal_manager(&index_home);
    set_interest(&index_home, &index_manager, "distributed systems");
    index.index_pod_announcement(announcement.clone()).unwrap();
    let search = index.search_pod_announcements("systems", 10).unwrap();
    approve_trust_policy_change(
        &index_home,
        TrustPolicyChange::AddIndexNode {
            label: "replaceable index".into(),
            base_url: "https://replaceable-index.example".into(),
        },
    );
    let index_reader = admin_harness(
        &index_home,
        "index removal reader",
        vec![HarnessCapability::FeedRead],
    );
    index_home
        .accept_index_search_results(&index_reader, "https://replaceable-index.example", search)
        .unwrap();
    let with_index = index_home
        .request_personal_discovery(
            &index_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "with-index".into(),
            },
            now,
        )
        .unwrap()
        .plan;
    assert!(with_index.source_neighborhoods.iter().any(|source| {
        matches!(
            &source.signal,
            SourceAffinitySignal::Community(slug) if slug == "rust-systems"
        ) && source.role == DiscoveryPlanSourceRole::Adjacent
    }));
    approve_trust_policy_change(
        &index_home,
        TrustPolicyChange::RemoveIndexNode {
            base_url: "https://replaceable-index.example".into(),
        },
    );
    let without_index_a = index_home
        .request_personal_discovery(
            &index_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "without-index-a".into(),
            },
            now,
        )
        .unwrap()
        .plan;
    let without_index_b = index_home
        .request_personal_discovery(
            &index_manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: None,
                idempotency_key: "without-index-b".into(),
            },
            now,
        )
        .unwrap()
        .plan;
    assert_eq!(
        adjacent_signals(&without_index_a),
        adjacent_signals(&without_index_b)
    );
    assert!(without_index_a.source_neighborhoods.iter().all(|source| {
        source.role != DiscoveryPlanSourceRole::Adjacent
            || !matches!(
                &source.signal,
                SourceAffinitySignal::Community(slug) if slug == "rust-systems"
            )
    }));
}

#[test]
fn private_discovery_lead_types_remain_absent_from_federation_and_outbound_artifacts() {
    let home_dir = TestDataDir::new("privacy-network-home");
    let home = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let manager = personal_manager(&home);
    set_interest(&home, &manager, "secret-private-topic");
    // Also plant a network lead that would match systems, not the secret topic.
    import_verified_network_metadata(&home);
    home.request_personal_discovery(
        &manager,
        RequestPersonalDiscovery {
            intent: Some(PersonalDiscoveryIntent::Topic(
                "secret-private-topic".into(),
            )),
            result_count: None,
            idempotency_key: "privacy-network".into(),
        },
        Utc::now(),
    )
    .unwrap();

    let federation = home.default_auth_context().unwrap();
    let public_pods = home.list_public_pods(&federation).unwrap();
    let manifests = public_pods
        .iter()
        .map(|pod| {
            home.federation_pod_manifest(&federation, &pod.slug)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let events = public_pods
        .iter()
        .map(|pod| home.federation_pod_events(&federation, &pod.slug).unwrap())
        .collect::<Vec<_>>();
    let packages = public_pods
        .iter()
        .map(|pod| home.export_skill_pack(&federation, &pod.slug).unwrap())
        .collect::<Vec<_>>();
    let announcements = home
        .store()
        .read()
        .unwrap()
        .known_pod_announcements
        .values()
        .map(|known| known.announcement.clone())
        .collect::<Vec<_>>();
    let samples = home
        .store()
        .read()
        .unwrap()
        .pod_explore_sample_sets
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let endorsements = home
        .store()
        .read()
        .unwrap()
        .pod_endorsements
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let index_search = home.search_pod_announcements("systems", 10).unwrap();
    let explored = home
        .explore_public_pods(&manager, ExploreRequest::new("systems", 10, 5).unwrap())
        .unwrap();
    let relayed = {
        // Relay surface requires a trusted peer; create a disposable peer node.
        let peer_dir = TestDataDir::new("privacy-peer");
        let peer = AgentTools::open_home_node(&peer_dir.0, seed_store).unwrap();
        let peer_info = peer
            .node_info(&peer.default_auth_context().unwrap())
            .unwrap();
        let proposer = admin_harness(
            &home,
            "privacy peer proposer",
            vec![HarnessCapability::Administration],
        );
        let approver = admin_harness(
            &home,
            "privacy peer approver",
            vec![HarnessCapability::Approval],
        );
        let now = Utc::now();
        let proposal = home
            .request_add_trusted_peer(
                &proposer,
                peer_info.display_name.clone(),
                "https://privacy-peer.example".into(),
                peer_info.public_key.clone(),
                now,
            )
            .unwrap();
        home.approve_pending_proposal(&approver, proposal.id, now)
            .unwrap();
        let trusted = home
            .trusted_peers(&proposer)
            .unwrap()
            .into_iter()
            .find(|trusted| trusted.public_key == peer_info.public_key)
            .unwrap();
        home.relay_pod_announcements(&proposer, trusted.id).unwrap()
    };

    let outbound = serde_json::to_string(&serde_json::json!({
        "node": home.node_info(&federation).unwrap(),
        "pods": public_pods,
        "manifests": manifests,
        "events": events,
        "packages": packages,
        "announcements": announcements,
        "samples": samples,
        "endorsements": endorsements,
        "index_search": index_search,
        "explore": explored,
        "relayed": relayed,
    }))
    .unwrap();
    for forbidden in [
        "secret-private-topic",
        "DiscoveryLead",
        "discovery_lead",
        "InterestSeed",
        "interest_seed",
        "TasteProfile",
        "matching_topics",
        "SourceAffinity",
        "local_relevance",
        "DiscoveryPlan",
        "discovery_plan",
        "PersonalDiscovery",
    ] {
        assert!(
            !outbound.contains(forbidden),
            "outbound artifact leaked private marker {forbidden}"
        );
    }
}

fn personal_worker(tools: &AgentTools) -> AuthContext {
    harness(
        tools,
        "personal worker",
        AgentHarnessKind::Unattended,
        vec![HarnessCapability::PersonalDiscoveryExecution],
    )
}

fn claim_personal_run(
    tools: &AgentTools,
    manager: &AuthContext,
    worker: &AuthContext,
    result_count: Option<u16>,
    key: &str,
) -> RequestedPersonalDiscovery {
    let now = Utc::now();
    let created = tools
        .request_personal_discovery(
            manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::Topic("systems".into())),
                result_count,
                idempotency_key: key.into(),
            },
            now,
        )
        .unwrap();
    tools
        .claim_discovery_task(
            worker,
            created.task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    created
}

fn personal_result_request(
    task_id: DiscoveryTaskId,
    url: &str,
    role: DiscoveryPlanSourceRole,
    author: Option<&str>,
    key: &str,
) -> CandidateSubmissionRequest {
    CandidateSubmissionRequest {
        target: CandidateSubmissionRequestTarget::PersonalDiscovery {
            task_id,
            allocation_role: role,
            source_facts: CandidateInterestSeedMetadata::default(),
        },
        evidence: CandidateSubmissionEvidence {
            source_url: url.into(),
            source_metadata: CandidateSourceMetadata {
                title: Some("Result".into()),
                author: author.map(str::to_owned),
                published_at: None,
            },
            permitted_excerpt: Some("excerpt".into()),
            summary: Some("summary".into()),
            content_type: CandidateContentType::Article,
            media_references: Vec::new(),
            tags: vec!["systems".into()],
            provenance: CandidateProvenance {
                discovered_at: Utc::now(),
                discovery_method: "browser_search".into(),
                referrer_url: Some("https://news.example/list".into()),
            },
            harness_idempotency_key: key.into(),
            client_idempotency_key: key.into(),
        },
    }
}

#[test]
fn only_lease_holder_may_submit_personal_results_or_complete_batch() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let other = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "lease-only");

    let request = personal_result_request(
        created.task.id,
        "https://a.example/1",
        DiscoveryPlanSourceRole::Proven,
        None,
        "r1",
    );
    assert!(matches!(
        tools.submit_candidate(&other, request.clone()),
        Err(AgentToolsError::CandidateTaskLeaseRequired)
    ));
    let submitted = tools.submit_candidate(&worker, request).unwrap();
    assert_eq!(
        submitted.submission.target.acquisition_origin(),
        CandidateAcquisitionOrigin::AgentDiscovery
    );

    let complete = CompleteDiscoveryResultBatchRequest {
        task_id: created.task.id,
        submission_ids: vec![submitted.submission.id],
        source_availability: Vec::new(),
    };
    assert!(matches!(
        tools.complete_discovery_result_batch(&other, complete.clone(), Utc::now()),
        Err(AgentToolsError::TaskLeaseRequired | AgentToolsError::Forbidden { .. })
    ));
    let batch = tools
        .complete_discovery_result_batch(&worker, complete, Utc::now())
        .unwrap();
    assert_eq!(batch.task_id, created.task.id);
    assert_eq!(batch.plan_id, created.plan.id);
    assert_eq!(batch.state, DiscoveryResultBatchState::Ready);
}

#[test]
fn personal_discovery_tasks_cannot_complete_without_a_result_batch() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "no-bare-complete");
    let now = Utc::now();

    let err = tools
        .complete_discovery_task(&worker, created.task.id, now)
        .expect_err("bare complete must not finish Personal Discovery");
    assert!(
        matches!(err, AgentToolsError::Store(StoreError::Validation(ref message)) if message.contains("complete_discovery_result_batch")),
        "unexpected error: {err:?}"
    );
    let task = tools
        .list_discovery_tasks(&worker, now)
        .unwrap()
        .into_iter()
        .find(|task| task.id == created.task.id)
        .expect("task still listed");
    assert!(
        matches!(task.state, DiscoveryTaskState::Leased(_)),
        "task must remain leased after rejected bare complete"
    );

    // Failures remain available so the worker can release the lease without a batch.
    let failed = tools
        .fail_discovery_task(
            &worker,
            created.task.id,
            now,
            "source neighborhoods unavailable".into(),
        )
        .unwrap();
    assert!(matches!(
        failed.state,
        DiscoveryTaskState::Pending | DiscoveryTaskState::TerminalFailure
    ));
}

#[test]
fn personal_results_retain_provenance_and_never_create_interest_seeds() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "provenance");

    let mut request = personal_result_request(
        created.task.id,
        "https://blog.example/post?utm=1",
        DiscoveryPlanSourceRole::Proven,
        Some("Ada"),
        "prov-1",
    );
    if let CandidateSubmissionRequestTarget::PersonalDiscovery { source_facts, .. } =
        &mut request.target
    {
        *source_facts =
            CandidateInterestSeedMetadata::new(Some("Example Press".into()), Some("rust".into()));
    }
    request.evidence.media_references = vec![MediaReference::new(
        MediaReferenceType::Image,
        "https://cdn.example.com/post.png",
    )
    .unwrap()];
    let submitted = tools.submit_candidate(&worker, request).unwrap();
    assert!(
        submitted
            .candidate
            .canonical_url
            .starts_with("https://blog.example/post"),
        "canonical identity retained: {}",
        submitted.candidate.canonical_url
    );
    assert_eq!(
        submitted.candidate.source_url,
        submitted.candidate.canonical_url
    );
    assert_eq!(
        submitted.submission.evidence.provenance.discovery_method,
        "browser_search"
    );
    assert_eq!(
        submitted
            .submission
            .evidence
            .provenance
            .referrer_url
            .as_deref(),
        Some("https://news.example/list")
    );
    match &submitted.submission.target {
        CandidateSubmissionTarget::PersonalDiscovery {
            task_id,
            discovery_plan_id,
            allocation_role,
            source_facts,
            ..
        } => {
            assert_eq!(*task_id, created.task.id);
            assert_eq!(*discovery_plan_id, created.plan.id);
            assert_eq!(*allocation_role, DiscoveryPlanSourceRole::Proven);
            assert_eq!(source_facts.publisher.as_deref(), Some("Example Press"));
            assert_eq!(source_facts.community.as_deref(), Some("rust"));
        }
        other => panic!("expected personal target, got {other:?}"),
    }
    assert!(!submitted.submission.target.learning_enabled());
    assert_eq!(
        tools
            .taste_profile(&manager)
            .unwrap()
            .interest_seed_evidence
            .active_seed_count,
        0
    );

    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(batch.items[0].candidate_id, submitted.candidate.id);
    assert_eq!(batch.items[0].submission_id, submitted.submission.id);
    assert_eq!(
        batch.items[0].canonical_url,
        submitted.candidate.canonical_url
    );
}

#[test]
fn batch_completion_enforces_size_allocation_caps_blocks_dedup_and_recent_suppression() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let mut taste = UpdateTasteProfileRequest::default();
    taste.blocked_sources = Some(vec!["blocked.example".into()]);
    tools.update_taste_profile(&manager, taste).unwrap();
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(6), "caps");

    let mut submission_ids = Vec::new();
    // 4 proven + 2 adjacent requested for size 6 (70/30 => 5/1 actually for 6? 6*7/10=4.2 -> (42+9)/10=5 proven, 1 adjacent)
    assert_eq!(created.plan.allocation.proven, 5);
    assert_eq!(created.plan.allocation.adjacent, 1);

    let specs = [
        (
            "https://d1.example/1",
            DiscoveryPlanSourceRole::Proven,
            Some("A1"),
            "s1",
        ),
        (
            "https://d1.example/2",
            DiscoveryPlanSourceRole::Proven,
            Some("A1"),
            "s2",
        ),
        (
            "https://d1.example/3",
            DiscoveryPlanSourceRole::Proven,
            Some("A2"),
            "s3",
        ),
        (
            "https://d1.example/4",
            DiscoveryPlanSourceRole::Proven,
            Some("A3"),
            "s4",
        ), // domain cap (>3)
        (
            "https://d2.example/1",
            DiscoveryPlanSourceRole::Proven,
            Some("A1"),
            "s5",
        ), // author cap (>2)
        (
            "https://blocked.example/x",
            DiscoveryPlanSourceRole::Proven,
            None,
            "s6",
        ),
        (
            "https://d3.example/1",
            DiscoveryPlanSourceRole::Proven,
            Some("A4"),
            "s7",
        ),
        (
            "https://d3.example/1#dup",
            DiscoveryPlanSourceRole::Adjacent,
            Some("A5"),
            "s8",
        ), // canonical dup
        (
            "https://d4.example/adj",
            DiscoveryPlanSourceRole::Adjacent,
            Some("A6"),
            "s9",
        ),
    ];
    for (url, role, author, key) in specs {
        let submitted = tools
            .submit_candidate(
                &worker,
                personal_result_request(created.task.id, url, role, author, key),
            )
            .unwrap();
        submission_ids.push(submitted.submission.id);
    }

    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: submission_ids.clone(),
                source_availability: vec![ReportedSourceAvailability {
                    source: "auth.example".into(),
                    reason: "authentication_required".into(),
                }],
            },
            Utc::now(),
        )
        .unwrap();

    assert!(batch.items.len() <= 6);
    assert!(batch
        .items
        .iter()
        .all(|item| !item.canonical_url.contains("blocked.example")));
    let d1 = batch
        .items
        .iter()
        .filter(|item| item.canonical_url.contains("d1.example"))
        .count();
    assert!(d1 <= 3, "domain cap exceeded: {d1}");
    assert!(batch
        .source_availability
        .iter()
        .any(|reason| matches!(reason, DiscoveryResultAvailabilityReason::DomainCap { .. })));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::AuthorOrAccountCap { .. }
    )));
    assert!(batch
        .source_availability
        .iter()
        .any(|reason| matches!(reason, DiscoveryResultAvailabilityReason::Blocked { .. })));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::CanonicalDuplicate { .. }
    )));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::SourceUnavailable { source, .. }
            if source == "auth.example"
    )));

    // Recent-result suppression for a second run.
    let second = claim_personal_run(&tools, &manager, &worker, Some(4), "recent-suppression");
    let first_url = batch.items[0].canonical_url.clone();
    let recent = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                second.task.id,
                &first_url,
                DiscoveryPlanSourceRole::Proven,
                Some("Z"),
                "recent-1",
            ),
        )
        .unwrap();
    let fresh = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                second.task.id,
                "https://fresh.example/new",
                DiscoveryPlanSourceRole::Proven,
                Some("Y"),
                "recent-2",
            ),
        )
        .unwrap();
    let suppressed = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: second.task.id,
                submission_ids: vec![recent.submission.id, fresh.submission.id],
                source_availability: Vec::new(),
            },
            Utc::now(),
        )
        .unwrap();
    assert!(suppressed
        .items
        .iter()
        .all(|item| item.canonical_url != first_url));
    assert!(suppressed.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::RecentlyReviewed { .. }
    )));
}

#[test]
fn underfilled_batch_records_reasons_without_inventing_results() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(10), "underfill");
    assert_eq!(created.plan.result_count, 10);
    assert_eq!(created.plan.allocation.proven, 7);
    assert_eq!(created.plan.allocation.adjacent, 3);

    let only = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://only.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "only",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![only.submission.id],
                source_availability: Vec::new(),
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(batch.items.len(), 1);
    assert_eq!(batch.requested_size, 10);
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::Underfilled {
            requested: 10,
            filled: 1
        }
    )));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::InsufficientProven { .. }
    )));
    assert!(batch.source_availability.iter().any(|reason| matches!(
        reason,
        DiscoveryResultAvailabilityReason::InsufficientAdjacent { .. }
    )));
}

#[test]
fn completion_is_atomic_retry_safe_and_duplicate_submissions_do_not_inflate() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "atomic");

    let first_request = personal_result_request(
        created.task.id,
        "https://a.example/1",
        DiscoveryPlanSourceRole::Proven,
        None,
        "a1",
    );
    let first = tools
        .submit_candidate(&worker, first_request.clone())
        .unwrap();
    let retry_submit = tools.submit_candidate(&worker, first_request).unwrap();
    assert_eq!(first.submission.id, retry_submit.submission.id);

    let second = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://b.example/2",
                DiscoveryPlanSourceRole::Adjacent,
                None,
                "b1",
            ),
        )
        .unwrap();
    let request = CompleteDiscoveryResultBatchRequest {
        task_id: created.task.id,
        submission_ids: vec![
            first.submission.id,
            first.submission.id,
            second.submission.id,
        ],
        source_availability: Vec::new(),
    };
    let batch = tools
        .complete_discovery_result_batch(&worker, request.clone(), Utc::now())
        .unwrap();
    assert_eq!(batch.items.len(), 2);
    let again = tools
        .complete_discovery_result_batch(&worker, request, Utc::now())
        .unwrap();
    assert_eq!(again.id, batch.id);
    assert_eq!(again.items, batch.items);
    let task = tools
        .discovery_task_status(&worker, created.task.id, Utc::now())
        .unwrap();
    assert_eq!(task.state, DiscoveryTaskState::Completed);
    let batches: Vec<_> = tools
        .store()
        .read()
        .unwrap()
        .discovery_result_batches
        .values()
        .filter(|batch| batch.task_id == created.task.id)
        .cloned()
        .collect();
    assert_eq!(batches.len(), 1);
}

#[test]
fn batch_states_and_notification_are_distinct_and_dismissal_creates_no_learning() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "states");
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://state.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "state-1",
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
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(batch.state, DiscoveryResultBatchState::Ready);
    assert_eq!(
        batch.notification_state,
        DiscoveryResultNotificationState::NotApplicable
    );
    let task = tools
        .discovery_task_status(&worker, created.task.id, Utc::now())
        .unwrap();
    assert_eq!(task.state, DiscoveryTaskState::Completed);
    assert_ne!(
        serde_json::to_value(batch.state).unwrap(),
        serde_json::to_value(task.state).unwrap()
    );

    // Workers cannot dismiss.
    assert!(matches!(
        tools.dismiss_discovery_result_batch(&worker, batch.id, Utc::now()),
        Err(AgentToolsError::Forbidden { .. })
    ));

    let profile_before = tools.taste_profile(&manager).unwrap();
    let feedback_before = tools.store().read().unwrap().feedback_events.len();
    let dismissed = tools
        .dismiss_discovery_result_batch(&manager, batch.id, Utc::now())
        .unwrap();
    assert_eq!(dismissed.state, DiscoveryResultBatchState::Dismissed);
    assert!(dismissed.dismissed_at.is_some());
    let profile_after = tools.taste_profile(&manager).unwrap();
    assert_eq!(
        profile_after.interest_seed_evidence.active_seed_count,
        profile_before.interest_seed_evidence.active_seed_count
    );
    assert_eq!(profile_after.learned.len(), profile_before.learned.len());
    assert_eq!(
        tools.store().read().unwrap().feedback_events.len(),
        feedback_before
    );

    let other = claim_personal_run(&tools, &manager, &worker, Some(4), "reviewed-state");
    let item = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                other.task.id,
                "https://review.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "rev-1",
            ),
        )
        .unwrap();
    let ready = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: other.task.id,
                submission_ids: vec![item.submission.id],
                source_availability: Vec::new(),
            },
            Utc::now(),
        )
        .unwrap();
    let reviewed = tools
        .mark_discovery_result_batch_reviewed(&manager, ready.id, Utc::now())
        .unwrap();
    assert_eq!(reviewed.state, DiscoveryResultBatchState::Reviewed);
    assert!(reviewed.reviewed_at.is_some());
    assert_eq!(
        reviewed.notification_state,
        DiscoveryResultNotificationState::NotApplicable
    );
}

#[test]
fn batches_and_candidate_provenance_persist_privately_across_restart() {
    let root = std::env::temp_dir().join(format!("stumble-batch-persist-{}", uuid::Uuid::now_v7()));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "persist manager".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![HarnessCapability::PersonalDiscoveryManagement],
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
                    label: "persist worker".into(),
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
    let created = claim_personal_run(&tools, &manager, &worker, Some(4), "persist");
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                created.task.id,
                "https://persist.example/item",
                DiscoveryPlanSourceRole::Proven,
                Some("Writer"),
                "persist-1",
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
            },
            Utc::now(),
        )
        .unwrap();
    assert_eq!(
        tools.store().read().unwrap().discovery_result_batches.len(),
        1
    );
    drop(tools);

    let reopened = AgentTools::open_initialized_home_node(&root).unwrap();
    assert_eq!(
        reopened
            .store()
            .read()
            .unwrap()
            .discovery_result_batches
            .len(),
        1,
        "Discovery Result Batches must survive restart"
    );
    let owner = reopened.local_owner_auth_context().unwrap();
    let manager = {
        let issued = reopened
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "persist manager reopen".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![HarnessCapability::PersonalDiscoveryManagement],
                    pod_ids: None,
                },
            )
            .unwrap();
        reopened
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    let listed = reopened.list_discovery_result_batches(&manager).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, batch.id);
    assert_eq!(
        listed[0].items[0].canonical_url,
        batch.items[0].canonical_url
    );
    let inspected = reopened.discovery_result_batch(&manager, batch.id).unwrap();
    assert_eq!(inspected.plan_id, created.plan.id);
    let submission = reopened
        .store()
        .read()
        .unwrap()
        .candidate_submissions
        .get(&submitted.submission.id)
        .cloned()
        .unwrap();
    assert_eq!(
        submission.evidence.provenance.discovery_method,
        "browser_search"
    );

    // Private / non-federated: batch markers absent from public federation export.
    let federation = reopened.default_auth_context().unwrap();
    let outbound = serde_json::to_string(&serde_json::json!({
        "node": reopened.node_info(&federation).unwrap(),
        "pods": reopened.list_public_pods(&federation).unwrap(),
    }))
    .unwrap();
    for forbidden in [
        "DiscoveryResultBatch",
        "discovery_result",
        "persist.example",
        &batch.id.to_string(),
    ] {
        assert!(
            !outbound.contains(forbidden),
            "federated surface leaked {forbidden}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

fn complete_one_result_batch(
    tools: &AgentTools,
    manager: &AuthContext,
    worker: &AuthContext,
    key: &str,
    url: &str,
) -> (
    RequestedPersonalDiscovery,
    DiscoveryResultBatch,
    SubmittedCandidate,
) {
    let created = claim_personal_run(tools, manager, worker, Some(4), key);
    let mut request = personal_result_request(
        created.task.id,
        url,
        DiscoveryPlanSourceRole::Proven,
        Some("Ada"),
        &format!("{key}-sub"),
    );
    if let CandidateSubmissionRequestTarget::PersonalDiscovery { source_facts, .. } =
        &mut request.target
    {
        *source_facts =
            CandidateInterestSeedMetadata::new(Some("Example Press".into()), Some("rust".into()));
    }
    let submitted = tools.submit_candidate(worker, request).unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
            },
            Utc::now(),
        )
        .unwrap();
    (created, batch, submitted)
}

#[test]
fn save_creates_inbox_placement_with_original_provenance() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let (_created, batch, submitted) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "save-inbox",
        "https://save.example/article?utm=1",
    );
    let candidate_id = batch.items[0].candidate_id;
    let now = Utc::now();

    let outcome = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::Save,
            },
            now,
        )
        .unwrap();

    assert_eq!(outcome.batch.state, DiscoveryResultBatchState::Ready);
    assert!(matches!(
        outcome.item.review,
        DiscoveryResultItemReview::Reviewed {
            action: DiscoveryResultItemAction::Save,
            ..
        }
    ));
    let placement = outcome.placement.expect("Save creates a placement");
    assert_eq!(placement.status, PodPlacementStatus::Accepted);
    assert_eq!(placement.curation_path, CurationPath::AddToPod);
    assert_eq!(
        placement.source_submission_ids,
        vec![submitted.submission.id]
    );
    let pod = tools
        .store()
        .read()
        .unwrap()
        .pods
        .get(&placement.pod_id)
        .cloned()
        .unwrap();
    assert_eq!(pod.visibility, Visibility::Private);
    assert_eq!(pod.name, "Inbox");
    assert_eq!(pod.created_by, manager.user_id);
    let content = tools
        .store()
        .read()
        .unwrap()
        .submissions
        .get(&uuid::Uuid::from(placement.content_item_id.unwrap()))
        .cloned()
        .unwrap();
    assert_eq!(content.canonical_url, submitted.candidate.canonical_url);
    // Save does not create learning evidence by itself.
    assert!(outcome
        .taste_profile
        .source_affinities
        .iter()
        .all(|affinity| { affinity.supporting_feedback == 0 && affinity.opposing_feedback == 0 }));
}

fn feedback_signal_total(profile: &TasteProfile) -> u32 {
    let learned: u32 = profile
        .learned
        .iter()
        .map(|weight| {
            weight
                .supporting_signals
                .saturating_add(weight.opposing_signals)
        })
        .sum();
    let affinities: u32 = profile
        .source_affinities
        .iter()
        .map(|affinity| {
            affinity
                .supporting_feedback
                .saturating_add(affinity.opposing_feedback)
        })
        .sum();
    learned.saturating_add(affinities)
}

#[test]
fn add_to_pod_respects_role_grant_and_public_policy_boundaries() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let curator = harness(
        &tools,
        "curator manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::PodCuration,
            HarnessCapability::Feedback,
        ],
    );
    let worker = personal_worker(&tools);
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: "Curation Target".into(),
                slug: "curation-target".into(),
                description: "authorized private pod".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let (_created, batch, submitted) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "add-pod",
        "https://place.example/item",
    );
    let candidate_id = batch.items[0].candidate_id;

    // Personal Discovery management alone cannot bypass Pod Role / grant boundaries.
    let denied = tools.review_discovery_result_item(
        &manager,
        ReviewDiscoveryResultItemRequest {
            batch_id: batch.id,
            candidate_id,
            action: DiscoveryResultItemActionRequest::AddToPod {
                pod_id: pod.id,
                curation_note: None,
            },
        },
        Utc::now(),
    );
    assert!(
        matches!(denied, Err(AgentToolsError::Forbidden { .. })),
        "unexpected: {denied:?}"
    );

    // Curator with PodCuration + Owner role may place.
    let outcome = tools
        .review_discovery_result_item(
            &curator,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::AddToPod {
                    pod_id: pod.id,
                    curation_note: Some(CurationRationale::new("fits the pod").unwrap()),
                },
            },
            Utc::now(),
        )
        .unwrap();
    let placement = outcome.placement.expect("placement");
    assert_eq!(placement.pod_id, pod.id);
    assert_eq!(placement.status, PodPlacementStatus::Accepted);
    assert_eq!(
        placement.source_submission_ids,
        vec![submitted.submission.id]
    );
    assert!(outcome
        .allowed_actions
        .contains(&DiscoveryResultAllowedAction::AddToPod));
}

#[test]
fn more_like_this_and_not_for_me_create_replaceable_learning_evidence() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let (_created, batch, _submitted) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "learn-item",
        "https://learn.example/post",
    );
    let candidate_id = batch.items[0].candidate_id;
    let now = Utc::now();

    let reinforced = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::MoreLikeThis,
            },
            now,
        )
        .unwrap();
    assert!(!reinforced.action_replaced);
    let evidence_after_first = feedback_signal_total(&reinforced.taste_profile);
    assert!(evidence_after_first > 0);
    assert!(reinforced.taste_profile.source_affinities.iter().any(|a| {
        a.signal == SourceAffinitySignal::Source("learn.example".into())
            && a.supporting_feedback > 0
    }));

    // Repeat is idempotent — evidence count must not inflate.
    let repeated = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::MoreLikeThis,
            },
            now,
        )
        .unwrap();
    assert!(!repeated.action_replaced);
    assert_eq!(
        feedback_signal_total(&repeated.taste_profile),
        evidence_after_first
    );

    // Changing action replaces evidence rather than stacking.
    let rejected = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::NotForMe,
            },
            now,
        )
        .unwrap();
    assert!(rejected.action_replaced);
    assert!(matches!(
        rejected.item.review,
        DiscoveryResultItemReview::Reviewed {
            action: DiscoveryResultItemAction::NotForMe,
            replaced_action: Some(DiscoveryResultItemAction::MoreLikeThis),
            ..
        }
    ));
    let affinities = &rejected.taste_profile.source_affinities;
    let source = affinities
        .iter()
        .find(|a| a.signal == SourceAffinitySignal::Source("learn.example".into()))
        .expect("source affinity");
    assert_eq!(source.supporting_feedback, 0);
    assert!(source.opposing_feedback > 0);
}

#[test]
fn ignore_dismiss_and_batch_review_create_no_learning_evidence() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let (_created, batch, _) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "ignore-item",
        "https://ignore.example/1",
    );
    let candidate_id = batch.items[0].candidate_id;
    let evidence_before = feedback_signal_total(&tools.taste_profile(&manager).unwrap());

    let ignored = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id,
                action: DiscoveryResultItemActionRequest::Ignore,
            },
            Utc::now(),
        )
        .unwrap();
    assert!(matches!(
        ignored.item.review,
        DiscoveryResultItemReview::Reviewed {
            action: DiscoveryResultItemAction::Ignore,
            ..
        }
    ));
    assert_eq!(
        feedback_signal_total(&ignored.taste_profile),
        evidence_before
    );
    // Item review does not complete the batch.
    assert_eq!(ignored.batch.state, DiscoveryResultBatchState::Ready);

    let other = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "dismiss-no-learn",
        "https://dismiss.example/1",
    );
    let dismissed = tools
        .dismiss_discovery_result_batch(&manager, other.1.id, Utc::now())
        .unwrap();
    assert_eq!(dismissed.state, DiscoveryResultBatchState::Dismissed);
    assert_eq!(
        feedback_signal_total(&tools.taste_profile(&manager).unwrap()),
        evidence_before
    );

    let third = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "mark-reviewed-no-learn",
        "https://mark.example/1",
    );
    let reviewed = tools
        .mark_discovery_result_batch_reviewed(&manager, third.1.id, Utc::now())
        .unwrap();
    assert_eq!(reviewed.state, DiscoveryResultBatchState::Reviewed);
    assert_eq!(
        feedback_signal_total(&tools.taste_profile(&manager).unwrap()),
        evidence_before
    );
    // Batch reviewed remains distinct from item Save / placement.
    assert!(reviewed
        .items
        .iter()
        .all(|item| { matches!(item.review, DiscoveryResultItemReview::Unreviewed) }));
}

#[test]
fn feedback_changes_next_plan_while_blocks_override_and_rejection_suppresses_rediscovery() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);

    // Two independent More like this actions corroborate source affinity weight.
    for (idx, url) in [
        "https://corroborate.example/a",
        "https://corroborate.example/b",
    ]
    .into_iter()
    .enumerate()
    {
        let (_c, batch, _) = complete_one_result_batch(
            &tools,
            &manager,
            &worker,
            &format!("corroborate-{idx}"),
            url,
        );
        tools
            .review_discovery_result_item(
                &manager,
                ReviewDiscoveryResultItemRequest {
                    batch_id: batch.id,
                    candidate_id: batch.items[0].candidate_id,
                    action: DiscoveryResultItemActionRequest::MoreLikeThis,
                },
                Utc::now(),
            )
            .unwrap();
    }

    let planned = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: Some(4),
                idempotency_key: "after-feedback".into(),
            },
            Utc::now(),
        )
        .unwrap();
    assert!(
        planned.plan.source_neighborhoods.iter().any(|source| {
            source.signal == SourceAffinitySignal::Source("corroborate.example".into())
                && source.rationale.contains("corroborated")
        }),
        "next plan should reflect reinforced source: {:?}",
        planned
            .plan
            .source_neighborhoods
            .iter()
            .map(|s| (&s.signal, &s.rationale))
            .collect::<Vec<_>>()
    );

    // Explicit block overrides learned evidence.
    let mut taste = UpdateTasteProfileRequest::default();
    taste.blocked_sources = Some(vec!["corroborate.example".into()]);
    tools.update_taste_profile(&manager, taste).unwrap();
    let blocked_plan = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: None,
                result_count: Some(4),
                idempotency_key: "after-block".into(),
            },
            Utc::now(),
        )
        .unwrap();
    assert!(blocked_plan.plan.source_neighborhoods.iter().all(|source| {
        source.signal != SourceAffinitySignal::Source("corroborate.example".into())
    }));

    // Rejected result cannot be rediscovered via equivalent URL spelling.
    let reject_run = claim_personal_run(&tools, &manager, &worker, Some(4), "reject-run");
    let rejected_submit = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                reject_run.task.id,
                "https://reject.example/story",
                DiscoveryPlanSourceRole::Proven,
                None,
                "reject-sub",
            ),
        )
        .unwrap();
    let reject_batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: reject_run.task.id,
                submission_ids: vec![rejected_submit.submission.id],
                source_availability: Vec::new(),
            },
            Utc::now(),
        )
        .unwrap();
    tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: reject_batch.id,
                candidate_id: reject_batch.items[0].candidate_id,
                action: DiscoveryResultItemActionRequest::NotForMe,
            },
            Utc::now(),
        )
        .unwrap();

    let next = claim_personal_run(&tools, &manager, &worker, Some(4), "reject-rediscover");
    let equivalent = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                next.task.id,
                "https://reject.example/story?utm_source=agent",
                DiscoveryPlanSourceRole::Proven,
                None,
                "reject-equiv",
            ),
        )
        .unwrap();
    let next_batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: next.task.id,
                submission_ids: vec![equivalent.submission.id],
                source_availability: Vec::new(),
            },
            Utc::now(),
        )
        .unwrap();
    assert!(
        next_batch.items.is_empty()
            || next_batch.items.iter().all(|item| {
                !item
                    .canonical_url
                    .starts_with("https://reject.example/story")
            }),
        "rejected URL must not reappear: {:?}",
        next_batch.items
    );
    assert!(next_batch.source_availability.iter().any(|reason| {
        matches!(
            reason,
            DiscoveryResultAvailabilityReason::RecentlyReviewed { canonical_url }
                if canonical_url.starts_with("https://reject.example/story")
        )
    }));
}

#[test]
fn item_review_placement_learning_and_batch_state_commit_atomically_and_survive_restart() {
    let root =
        std::env::temp_dir().join(format!("stumble-review-persist-{}", uuid::Uuid::now_v7()));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "review manager".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::PersonalDiscoveryManagement,
                        HarnessCapability::Feedback,
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
                    label: "review worker".into(),
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
    let (_created, batch, submitted) = complete_one_result_batch(
        &tools,
        &manager,
        &worker,
        "atomic-review",
        "https://atomic.example/item",
    );
    let outcome = tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id: batch.items[0].candidate_id,
                action: DiscoveryResultItemActionRequest::Save,
            },
            Utc::now(),
        )
        .unwrap();
    let placement_id = outcome.placement.as_ref().map(|p| p.pod_id).unwrap();
    tools
        .review_discovery_result_item(
            &manager,
            ReviewDiscoveryResultItemRequest {
                batch_id: batch.id,
                candidate_id: batch.items[0].candidate_id,
                action: DiscoveryResultItemActionRequest::MoreLikeThis,
            },
            Utc::now(),
        )
        .unwrap();
    let evidence_len = feedback_signal_total(&tools.taste_profile(&manager).unwrap());
    assert!(evidence_len > 0);
    drop(tools);

    let reopened = AgentTools::open_initialized_home_node(&root).unwrap();
    let owner = reopened.local_owner_auth_context().unwrap();
    let manager = {
        let issued = reopened
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "review manager reopen".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::PersonalDiscoveryManagement,
                        HarnessCapability::Feedback,
                    ],
                    pod_ids: None,
                },
            )
            .unwrap();
        reopened
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap()
    };
    let inspected = reopened.discovery_result_batch(&manager, batch.id).unwrap();
    assert!(matches!(
        inspected.items[0].review,
        DiscoveryResultItemReview::Reviewed {
            action: DiscoveryResultItemAction::MoreLikeThis,
            placement_pod_id: Some(pod_id),
            ..
        } if pod_id == placement_id
    ));
    assert_eq!(
        feedback_signal_total(&reopened.taste_profile(&manager).unwrap()),
        evidence_len
    );
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .pod_placements
        .values()
        .any(|placement| {
            placement.pod_id == placement_id
                && placement
                    .source_submission_ids
                    .contains(&submitted.submission.id)
                && placement.status == PodPlacementStatus::Accepted
        }));
    // Private: review markers stay off public federation surfaces.
    let federation = reopened.default_auth_context().unwrap();
    let outbound = serde_json::to_string(&serde_json::json!({
        "pods": reopened.list_public_pods(&federation).unwrap(),
    }))
    .unwrap();
    assert!(!outbound.contains("atomic.example"));
    let _ = std::fs::remove_dir_all(root);
}

fn daily_schedule_request(name: &str) -> CreatePersonalDiscoveryScheduleRequest {
    CreatePersonalDiscoveryScheduleRequest {
        name: name.into(),
        cadence: PersonalDiscoveryCadence::Daily,
        intent: PersonalDiscoveryScheduleIntent::default(),
        result_count: Some(5),
        delivery_mode: PersonalDiscoveryDeliveryMode::NotifyWhenSupported,
    }
}

#[test]
fn user_may_create_inspect_update_disable_and_remove_named_schedules() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0).unwrap();

    let daily = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("daily"), now)
        .unwrap();
    assert_eq!(daily.schedule.name, "daily");
    assert!(daily.schedule.enabled);
    assert_eq!(daily.schedule.result_count, 5);
    assert_eq!(
        daily.schedule.delivery_mode,
        PersonalDiscoveryDeliveryMode::NotifyWhenSupported
    );

    let weekly = tools
        .create_personal_discovery_schedule(
            &manager,
            CreatePersonalDiscoveryScheduleRequest {
                name: "weekly deep".into(),
                cadence: PersonalDiscoveryCadence::Weekly,
                intent: PersonalDiscoveryScheduleIntent::new(
                    vec!["rust".into()],
                    vec!["crypto".into()],
                ),
                result_count: Some(12),
                delivery_mode: PersonalDiscoveryDeliveryMode::QueueOnly,
            },
            now,
        )
        .unwrap();
    assert_eq!(weekly.schedule.intent.focus_topics, vec!["rust"]);
    assert_eq!(weekly.schedule.intent.avoid_topics, vec!["crypto"]);

    let listed = tools
        .list_personal_discovery_schedules(&manager, now)
        .unwrap();
    assert_eq!(listed.len(), 2);

    let inspected = tools
        .personal_discovery_schedule(&manager, daily.schedule.id, now)
        .unwrap();
    assert_eq!(inspected.schedule.id, daily.schedule.id);

    let updated = tools
        .update_personal_discovery_schedule(
            &manager,
            daily.schedule.id,
            UpdatePersonalDiscoveryScheduleRequest {
                name: Some("morning".into()),
                result_count: Some(8),
                ..Default::default()
            },
            now,
        )
        .unwrap();
    assert_eq!(updated.schedule.name, "morning");
    assert_eq!(updated.schedule.result_count, 8);

    let disabled = tools
        .disable_personal_discovery_schedule(&manager, updated.schedule.id, now)
        .unwrap();
    assert!(!disabled.schedule.enabled);

    let removed = tools
        .remove_personal_discovery_schedule(&manager, weekly.schedule.id)
        .unwrap();
    assert_eq!(removed.name, "weekly deep");
    assert_eq!(
        tools
            .list_personal_discovery_schedules(&manager, now)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn schedule_remains_dormant_below_cold_start_readiness() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    let mut empty = UpdateTasteProfileRequest::default();
    empty.interests = Some(Vec::new());
    tools.update_taste_profile(&manager, empty).unwrap();
    tools
        .reset_learned_taste(&manager, ResetLearnedTasteRequest::all())
        .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 15, 0, 0).unwrap();
    let created = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("dormant"), now)
        .unwrap();
    assert!(created.readiness_dormant);

    let ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    assert!(!ready.iter().any(|task| matches!(
        task.origin,
        DiscoveryTaskOrigin::PersonalScheduled { schedule_id } if schedule_id == created.schedule.id
    )));
}

#[test]
fn due_materialization_is_deterministic_and_idempotent_for_schedule_period() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 10, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("idempotent"), now)
        .unwrap();

    let first = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let second = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let scheduled: Vec<_> = first
        .iter()
        .filter(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == schedule.schedule.id
            )
        })
        .cloned()
        .collect();
    assert_eq!(scheduled.len(), 1);
    assert_eq!(
        scheduled[0].due_at,
        schedule.schedule.cadence.period_start(now)
    );
    let again: Vec<_> = second
        .iter()
        .filter(|task| task.id == scheduled[0].id)
        .collect();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, scheduled[0].id);

    // Concurrent-style second materialization path via manager list.
    let manager_listed = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    assert_eq!(
        manager_listed
            .iter()
            .filter(|task| matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == schedule.schedule.id
            ))
            .count(),
        1
    );
}

#[test]
fn harness_and_local_adapter_paths_list_same_canonical_ready_tasks() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 11, 0, 0).unwrap();
    tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("neutral"), now)
        .unwrap();

    // Harness-owned wake: list ready as the unattended worker.
    let harness_ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    // Local Scheduler Adapter uses the same list_ready contract with an equivalent token.
    let adapter_ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let harness_ids: Vec<_> = harness_ready.iter().map(|task| task.id).collect();
    let adapter_ids: Vec<_> = adapter_ready.iter().map(|task| task.id).collect();
    assert_eq!(harness_ids, adapter_ids);
    assert!(!harness_ids.is_empty());
}

#[test]
fn schedule_defers_while_unreviewed_batch_and_on_demand_remains_available() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 9, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("backpressure"), now)
        .unwrap();

    let ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let task = ready
        .into_iter()
        .find(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == schedule.schedule.id
            )
        })
        .expect("scheduled task");
    let lease_now = Utc::now();
    tools
        .claim_discovery_task(
            &worker,
            task.id,
            lease_now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                task.id,
                "https://backpressure.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "bp-1",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
            },
            lease_now,
        )
        .unwrap();
    assert_eq!(batch.state, DiscoveryResultBatchState::Ready);

    let status = tools
        .personal_discovery_schedule(&manager, schedule.schedule.id, lease_now)
        .unwrap();
    assert!(matches!(
        status.backpressure,
        PersonalDiscoveryScheduleBackpressure::UnreviewedBatch { batch_id, .. }
            if batch_id == batch.id
    ));

    // Next period remains deferred while unreviewed.
    let next_day = now + chrono::Duration::days(1);
    let deferred = tools.list_ready_discovery_tasks(&worker, next_day).unwrap();
    assert!(!deferred.iter().any(|task| {
        matches!(
            task.origin,
            DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                if schedule_id == schedule.schedule.id
        ) && task.due_at == schedule.schedule.cadence.period_start(next_day)
    }));

    // On-demand remains available under schedule backpressure.
    let on_demand = tools
        .request_personal_discovery(
            &manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::Topic("systems".into())),
                result_count: Some(3),
                idempotency_key: "on-demand-during-backpressure".into(),
            },
            next_day,
        )
        .unwrap();
    assert!(matches!(
        on_demand.task.origin,
        DiscoveryTaskOrigin::PersonalRequest { .. }
    ));

    tools
        .dismiss_discovery_result_batch(&manager, batch.id, next_day)
        .unwrap();
    let resumed = tools.list_ready_discovery_tasks(&worker, next_day).unwrap();
    assert!(resumed.iter().any(|task| {
        matches!(
            task.origin,
            DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                if schedule_id == schedule.schedule.id
        ) && task.due_at == schedule.schedule.cadence.period_start(next_day)
    }));
}

#[test]
fn scheduled_completion_emits_one_results_ready_event_and_notification_is_one_shot() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 8, 0, 0).unwrap();

    let notify = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("notify"), now)
        .unwrap();
    let queue = tools
        .create_personal_discovery_schedule(
            &manager,
            CreatePersonalDiscoveryScheduleRequest {
                name: "queue".into(),
                cadence: PersonalDiscoveryCadence::Daily,
                intent: PersonalDiscoveryScheduleIntent::default(),
                result_count: Some(3),
                delivery_mode: PersonalDiscoveryDeliveryMode::QueueOnly,
            },
            now,
        )
        .unwrap();

    let ready = tools.list_ready_discovery_tasks(&worker, now).unwrap();
    let notify_task = ready
        .iter()
        .find(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == notify.schedule.id
            )
        })
        .unwrap()
        .clone();
    let queue_task = ready
        .iter()
        .find(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == queue.schedule.id
            )
        })
        .unwrap()
        .clone();

    let lease_now = Utc::now();
    for (task, key) in [(&notify_task, "n1"), (&queue_task, "q1")] {
        tools
            .claim_discovery_task(
                &worker,
                task.id,
                lease_now,
                DiscoveryLeaseSeconds::new(300).unwrap(),
            )
            .unwrap();
        let submitted = tools
            .submit_candidate(
                &worker,
                personal_result_request(
                    task.id,
                    &format!("https://event.example/{key}"),
                    DiscoveryPlanSourceRole::Proven,
                    None,
                    key,
                ),
            )
            .unwrap();
        tools
            .complete_discovery_result_batch(
                &worker,
                CompleteDiscoveryResultBatchRequest {
                    task_id: task.id,
                    submission_ids: vec![submitted.submission.id],
                    source_availability: Vec::new(),
                },
                lease_now,
            )
            .unwrap();
    }

    let notify_batch = tools
        .list_discovery_result_batches(&manager)
        .unwrap()
        .into_iter()
        .find(|batch| batch.task_id == notify_task.id)
        .unwrap();
    let queue_batch = tools
        .list_discovery_result_batches(&manager)
        .unwrap()
        .into_iter()
        .find(|batch| batch.task_id == queue_task.id)
        .unwrap();
    assert_eq!(
        notify_batch.notification_state,
        DiscoveryResultNotificationState::Pending
    );
    assert_eq!(
        queue_batch.notification_state,
        DiscoveryResultNotificationState::NotApplicable
    );

    let events = tools
        .store()
        .read()
        .unwrap()
        .discovery_results_ready_events
        .clone();
    assert_eq!(events.len(), 2);
    assert!(events.contains_key(&notify_batch.id));
    assert!(events.contains_key(&queue_batch.id));

    let first = tools
        .attempt_discovery_results_ready_notification(&manager, notify_batch.id, lease_now)
        .unwrap();
    assert!(matches!(
        first,
        DiscoveryResultsReadyNotificationOutcome::ShouldNotify { .. }
    ));
    let second = tools
        .attempt_discovery_results_ready_notification(&manager, notify_batch.id, lease_now)
        .unwrap();
    assert!(matches!(
        second,
        DiscoveryResultsReadyNotificationOutcome::AlreadyAttempted { .. }
    ));
    let notify_batch = tools
        .discovery_result_batch(&manager, notify_batch.id)
        .unwrap();
    assert_eq!(
        notify_batch.notification_state,
        DiscoveryResultNotificationState::Delivered
    );
    assert_eq!(notify_batch.state, DiscoveryResultBatchState::Ready);

    let silent = tools
        .attempt_discovery_results_ready_notification(&manager, queue_batch.id, lease_now)
        .unwrap();
    assert!(matches!(
        silent,
        DiscoveryResultsReadyNotificationOutcome::QueueOnly { .. }
    ));
    assert_eq!(
        tools
            .discovery_result_batch(&manager, queue_batch.id)
            .unwrap()
            .state,
        DiscoveryResultBatchState::Ready
    );
}

#[test]
fn schedules_events_tasks_and_batches_persist_privately_across_restart() {
    let root =
        std::env::temp_dir().join(format!("stumble-schedule-persist-{}", uuid::Uuid::now_v7()));
    let tools = AgentTools::initialize_home_node(&root, seed_store).unwrap();
    let owner = tools.local_owner_auth_context().unwrap();
    let manager = {
        let issued = tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: "schedule manager".into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities: vec![
                        HarnessCapability::PersonalDiscoveryManagement,
                        HarnessCapability::Feedback,
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
                    label: "schedule worker".into(),
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
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 7, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("persist"), now)
        .unwrap();
    let task = tools
        .list_ready_discovery_tasks(&worker, now)
        .unwrap()
        .into_iter()
        .find(|task| {
            matches!(
                task.origin,
                DiscoveryTaskOrigin::PersonalScheduled { schedule_id }
                    if schedule_id == schedule.schedule.id
            )
        })
        .unwrap();
    let lease_now = Utc::now();
    tools
        .claim_discovery_task(
            &worker,
            task.id,
            lease_now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let submitted = tools
        .submit_candidate(
            &worker,
            personal_result_request(
                task.id,
                "https://persist-schedule.example/1",
                DiscoveryPlanSourceRole::Proven,
                None,
                "persist-s1",
            ),
        )
        .unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            &worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
            },
            lease_now,
        )
        .unwrap();

    let reopened = AgentTools::open_home_node(&root, seed_store).unwrap();
    let manager = reopened.local_owner_auth_context().unwrap();
    let status = reopened
        .list_personal_discovery_schedules(&manager, lease_now)
        .unwrap();
    assert!(status.iter().any(|s| s.schedule.name == "persist"));
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .discovery_results_ready_events
        .contains_key(&batch.id));
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .discovery_tasks
        .contains_key(&task.id));
    assert!(reopened
        .store()
        .read()
        .unwrap()
        .discovery_result_batches
        .contains_key(&batch.id));

    let federation = reopened.default_auth_context().unwrap();
    let outbound = serde_json::to_string(&serde_json::json!({
        "pods": reopened.list_public_pods(&federation).unwrap(),
    }))
    .unwrap();
    assert!(!outbound.contains("persist-schedule.example"));
    assert!(!outbound.contains(&schedule.schedule.name));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn worker_cannot_change_schedule_or_delivery_policy() {
    let tools = AgentTools::new(seed_store());
    let manager = personal_manager(&tools);
    let worker = personal_worker(&tools);
    set_interest(&tools, &manager, "systems");
    let now = Utc.with_ymd_and_hms(2026, 7, 20, 6, 0, 0).unwrap();
    let schedule = tools
        .create_personal_discovery_schedule(&manager, daily_schedule_request("authz"), now)
        .unwrap();

    assert!(matches!(
        tools.create_personal_discovery_schedule(&worker, daily_schedule_request("nope"), now),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(matches!(
        tools.update_personal_discovery_schedule(
            &worker,
            schedule.schedule.id,
            UpdatePersonalDiscoveryScheduleRequest {
                delivery_mode: Some(PersonalDiscoveryDeliveryMode::QueueOnly),
                ..Default::default()
            },
            now,
        ),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(matches!(
        tools.disable_personal_discovery_schedule(&worker, schedule.schedule.id, now),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(matches!(
        tools.remove_personal_discovery_schedule(&worker, schedule.schedule.id),
        Err(AgentToolsError::Forbidden { .. })
    ));
    // Workers may inspect backpressure state for wake/claim decisions.
    let status = tools
        .personal_discovery_schedule(&worker, schedule.schedule.id, now)
        .unwrap();
    assert_eq!(status.schedule.id, schedule.schedule.id);
}
