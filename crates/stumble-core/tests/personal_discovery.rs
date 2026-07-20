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
