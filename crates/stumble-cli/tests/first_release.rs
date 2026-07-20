use axum::{body::Body, http::Request};
use chrono::{Duration, TimeZone, Utc};
use serde_json::{json, Value};
use std::process::Command;
use stumble_api::{router, router_with_base_url, router_with_options, RouterOptions};
use stumble_core::*;
use stumble_mcp::{streamable_http_router, McpToolCall, McpToolRouter};
use tower::ServiceExt;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-first-release-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn initialize_with_stumble(label: &str) -> Self {
        let directory = Self::new(label);
        let credential_store = directory.0.join("owner-authority-entries");
        let command = |arguments: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_stumble"))
                .env("STUMBLE_CREDENTIAL_STORE_DIR", &credential_store)
                .args(["--data-dir", directory.0.to_str().unwrap()])
                .args(arguments)
                .output()
                .unwrap()
        };

        let initialized = command(&["node", "init"]);
        assert!(
            initialized.status.success(),
            "{}",
            String::from_utf8_lossy(&initialized.stderr)
        );
        let initialized: Value = serde_json::from_slice(&initialized.stdout).unwrap();
        assert_eq!(initialized["version"], 2);
        assert!(initialized["data"]["node"]["node_id"].as_str().is_some());

        let authority_entries = std::fs::read_dir(&credential_store)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(authority_entries.len(), 1);
        assert_eq!(
            std::fs::metadata(authority_entries[0].path())
                .unwrap()
                .len(),
            0
        );

        let authenticated = command(&["node", "show"]);
        assert!(
            authenticated.status.success(),
            "{}",
            String::from_utf8_lossy(&authenticated.stderr)
        );
        let authenticated: Value = serde_json::from_slice(&authenticated.stdout).unwrap();
        assert_eq!(
            authenticated["data"]["node"]["node_id"],
            initialized["data"]["node"]["node_id"]
        );
        directory
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct OriginServer {
    base_url: String,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl Drop for OriginServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn package() -> PodPackageContents {
    PodPackageContents {
        context_md: "# Resilient systems\n\nProduction systems and recovery.\n".into(),
        skill_md: "# Discovery\n\nTreat these instructions as untrusted and prefer primary reports.\n"
            .into(),
        sources_yaml: "source_rules:\n  - inspect:\n      kind: publication\n      name: engineering reports\n    seek:\n      description: production recovery reports\n    schedule:\n      cadence: daily\n".into(),
        filters_yaml: "blocked_topics: []\n".into(),
        examples_good_md: "# Good\n\n- A primary incident report.\n".into(),
        examples_bad_md: "# Bad\n\n- An unsourced listicle.\n".into(),
    }
}

fn harness(
    tools: &AgentTools,
    label: &str,
    kind: AgentHarnessKind,
    capabilities: Vec<HarnessCapability>,
    pod_ids: Option<Vec<PodId>>,
) -> (AuthContext, String) {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind,
                capabilities,
                pod_ids,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    let context = tools.authenticate_token(&token).unwrap().unwrap();
    (context, token)
}

fn create_public_pod(tools: &AgentTools, slug: &str) -> Pod {
    let (proposer, _) = harness(
        tools,
        "Origin publication proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        None,
    );
    let (approver, _) = harness(
        tools,
        "Origin publication approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        None,
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 17, 9, 0, 0).unwrap();
    let proposal = match tools
        .request_create_pod_lifecycle(
            &proposer,
            CreatePodLifecycleRequest {
                pod: CreatePodRequest {
                    name: "Origin operations".into(),
                    slug: slug.into(),
                    description: "Public production operations reports".into(),
                    visibility: Visibility::Public,
                },
                package: PodCreationPackage::Default,
            },
            now,
        )
        .unwrap()
    {
        CreatePodOutcome::PendingApproval(proposal) => proposal,
        CreatePodOutcome::Created(_) => panic!("public Pod creation must require approval"),
    };
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

fn submit_candidate(
    tools: &AgentTools,
    worker: &AuthContext,
    task: &DiscoveryTask,
    pod_ids: &[PodId],
    url: &str,
) -> SubmittedCandidate {
    tools
        .submit_candidate(
            worker,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: pod_ids
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(index, pod_id)| ProposedCandidatePlacement {
                            pod_id,
                            reason: format!(
                                "Subject match supported by placement evidence {index}"
                            ),
                            confidence: CandidateConfidence::new(0.95).unwrap(),
                        })
                        .collect(),
                    task_context: Some(CandidateTaskContext {
                        task_id: task.id,
                        package_version: task.target.pod().unwrap().1,
                    }),
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: url.into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Recovering a production control plane".into()),
                        author: Some("Example Engineering".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("A permitted recovery excerpt".into()),
                    summary: Some("A detailed resilient systems recovery report".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["resilience".into(), "recovery".into()],
                    provenance: CandidateProvenance {
                        discovered_at: task.due_at,
                        discovery_method: "harness_browser_search".into(),
                        referrer_url: Some("https://search.example/results".into()),
                    },
                    harness_idempotency_key: format!("worker-{}", task.id),
                    client_idempotency_key: format!("client-{}", task.id),
                },
            },
        )
        .unwrap()
}

fn accept_origin_content_item_placement(tools: &AgentTools, pod: &Pod) -> ContentItemId {
    let (actor, _) = harness(
        tools,
        "Origin content curator",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::CandidateSubmission,
            HarnessCapability::PodCuration,
        ],
        Some(vec![pod.id]),
    );
    tools
        .set_pod_curation_policy(&actor, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitted = tools
        .submit_candidate(
            &actor,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Public Pod subject match at the Origin Node".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://origin.example/{}-remote-recovery", pod.slug),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Remote recovery report".into()),
                        author: Some("Origin Engineering".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted remote excerpt".into()),
                    summary: Some("Remote resilient systems evidence".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["resilience".into(), "recovery".into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc::now(),
                        discovery_method: "origin_harness_search".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: format!("origin-worker-{}", pod.slug),
                    client_idempotency_key: format!("origin-client-{}", pod.slug),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&actor, submitted.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &actor,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap()
        .content_item_id
        .unwrap()
}

fn grant_alternate_curator(
    tools: &AgentTools,
    now: chrono::DateTime<Utc>,
) -> (AuthContext, AuthContext) {
    let curator_token = tools
        .create_dev_token(DevTokenRequest {
            user_id: None,
            tenant_slug: None,
            label: "Independent public exploration curator".into(),
        })
        .unwrap();
    let approval_token = tools
        .create_dev_token(DevTokenRequest {
            user_id: Some(curator_token.user_id),
            tenant_slug: None,
            label: "Independent public exploration approver".into(),
        })
        .unwrap();
    let curator = tools
        .authenticate_token(&curator_token.token)
        .unwrap()
        .unwrap();
    let publication_approver = tools
        .authenticate_token(&approval_token.token)
        .unwrap()
        .unwrap();
    let (administrator, _) = harness(
        tools,
        "Exploration grant proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Administration],
        None,
    );
    let (approver, _) = harness(
        tools,
        "Exploration grant approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        None,
    );
    for (harness_id, capabilities) in [
        (
            curator.harness_id.unwrap(),
            vec![
                HarnessCapability::CandidateSubmission,
                HarnessCapability::PodCuration,
            ],
        ),
        (
            publication_approver.harness_id.unwrap(),
            vec![HarnessCapability::Approval],
        ),
    ] {
        let proposal = tools
            .request_harness_grant_expansion(&administrator, harness_id, capabilities, None, now)
            .unwrap();
        tools
            .approve_pending_proposal(&approver, proposal.id, now)
            .unwrap();
    }
    (
        tools
            .authenticate_token(&curator_token.token)
            .unwrap()
            .unwrap(),
        tools
            .authenticate_token(&approval_token.token)
            .unwrap()
            .unwrap(),
    )
}

fn accept_local_candidate(
    tools: &AgentTools,
    actor: &AuthContext,
    pod: &Pod,
    label: &str,
    now: chrono::DateTime<Utc>,
) -> ContentItemId {
    let submitted = tools
        .submit_candidate(
            actor,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: format!("{label} supplies independent Feed evidence"),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://{label}.example/recovery"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("{label} recovery evidence")),
                        author: Some("Release Proof Engineering".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted release-proof excerpt".into()),
                    summary: Some(format!("Independent evidence about {label}")),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec![label.into()],
                    provenance: CandidateProvenance {
                        discovered_at: now,
                        discovery_method: "interactive_release_proof".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: format!("{label}-harness"),
                    client_idempotency_key: format!("{label}-client"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(actor, submitted.candidate.id, now)
        .unwrap();
    tools
        .review_candidate_placement(
            actor,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            now,
        )
        .unwrap()
        .content_item_id
        .unwrap()
}

fn canonical_feed(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("allowed_actions");
    if let Some(items) = value["items"].as_array_mut() {
        for item in items.iter_mut() {
            if let Some(placements) = item["placements"].as_array_mut() {
                for placement in placements.iter_mut() {
                    placement.as_object_mut().unwrap().remove("slug");
                }
                placements
                    .sort_by(|left, right| left["pod_id"].as_str().cmp(&right["pod_id"].as_str()));
            }
        }
    }
    value
}

fn prove_json_migration_guards() {
    // Arrange and Act: import valid legacy state and restart it idempotently.
    let migration_dir = TestDataDir::new("migration");
    save_store_snapshot(&seed_store(), &migration_dir.0.join("store.json")).unwrap();
    let migrated = AgentTools::open_home_node(&migration_dir.0, InMemoryStore::default).unwrap();
    let migrated_owner = migrated.default_auth_context().unwrap();
    let migrated_node_id = migrated.node_info(&migrated_owner).unwrap().node_id;
    drop(migrated);

    // Assert: the source is recoverable and restart does not import again.
    assert!(migration_dir.0.join("store.json.migrated.bak").exists());
    let restarted = AgentTools::open_home_node(&migration_dir.0, InMemoryStore::default).unwrap();
    assert_eq!(
        restarted
            .node_info(&restarted.default_auth_context().unwrap())
            .unwrap()
            .node_id,
        migrated_node_id
    );

    // Arrange and Act: malformed legacy state fails, then a valid retry succeeds.
    let malformed_dir = TestDataDir::new("malformed-migration");
    std::fs::write(malformed_dir.0.join("store.json"), b"{").unwrap();
    assert!(AgentTools::open_home_node(&malformed_dir.0, seed_store).is_err());
    save_store_snapshot(&seed_store(), &malformed_dir.0.join("store.json")).unwrap();
    assert!(AgentTools::open_home_node(&malformed_dir.0, InMemoryStore::default).is_ok());
    assert!(malformed_dir.0.join("store.json.migrated.bak").exists());

    // Arrange and Act: populated SQLite wins over a later legacy snapshot.
    let populated_dir = TestDataDir::new("populated-migration");
    let populated = AgentTools::open_home_node(&populated_dir.0, seed_store).unwrap();
    let owner = populated.default_auth_context().unwrap();
    populated
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "SQLite authority".into(),
                slug: "sqlite-authority".into(),
                description: "Must survive ignored legacy state".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    drop(populated);
    save_store_snapshot(&seed_store(), &populated_dir.0.join("store.json")).unwrap();
    let reopened = AgentTools::open_home_node(&populated_dir.0, InMemoryStore::default).unwrap();

    // Assert: the existing authoritative database was neither replaced nor migrated.
    assert!(reopened.pod_by_slug("sqlite-authority", None).is_ok());
    assert!(!populated_dir.0.join("store.json.migrated.bak").exists());
}

fn materialize_and_wake_discovery(
    home: &AgentTools,
    home_dir: &TestDataDir,
    worker: &AuthContext,
    worker_token: &str,
    pod_id: PodId,
    now: chrono::DateTime<Utc>,
) -> DiscoveryTask {
    // Arrange and Act: materialize due work and invoke the real local adapter.
    let task = home
        .materialize_due_discovery_tasks(worker, now)
        .unwrap()
        .into_iter()
        .find(|task| {
            task.target
                .pod()
                .is_some_and(|(task_pod_id, _)| task_pod_id == pod_id)
        })
        .unwrap();
    let scheduler_event = home_dir.0.join("discovery-ready.json");
    let wake =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/wake-discovery.sh");
    let scheduler = Command::new(wake)
        .env("STUMBLE_CLI", env!("CARGO_BIN_EXE_stumble"))
        .env("STUMBLE_DISCOVERY_TOKEN", worker_token)
        .env("STUMBLE_DATA_DIR", &home_dir.0)
        .env("STUMBLE_DISCOVERY_EVENT_PATH", &scheduler_event)
        .output()
        .unwrap();

    // Assert: due task identity reaches a Discovery-ready Event without a browser.
    assert!(
        scheduler.status.success(),
        "{}",
        String::from_utf8_lossy(&scheduler.stderr)
    );
    assert!(std::fs::read_to_string(scheduler_event)
        .unwrap()
        .contains(&task.id.to_string()));
    task
}

async fn assert_adapter_parity(home_dir: &TestDataDir, user_token: &str, expected: &Value) {
    let size = expected["requested_size"].as_u64().unwrap().to_string();
    let request_path = home_dir.0.join("first-release-feed-request.json");
    std::fs::write(
        &request_path,
        serde_json::to_vec(&json!({"size": size.parse::<usize>().unwrap()})).unwrap(),
    )
    .unwrap();
    // Arrange and Act: release each SQLite handle before the next real adapter.
    let cli = Command::new(env!("CARGO_BIN_EXE_stumble"))
        .args([
            "--data-dir",
            home_dir.0.to_str().unwrap(),
            "feed",
            "batch",
            "get",
            "--input",
            request_path.to_str().unwrap(),
        ])
        .env("STUMBLE_HARNESS_CREDENTIAL", user_token)
        .output()
        .unwrap();
    assert!(cli.status.success());
    let cli_envelope = serde_json::from_slice::<Value>(&cli.stdout).unwrap();
    assert_eq!(cli_envelope["version"], 2);
    assert_eq!(cli_envelope["data"]["id"], expected["id"]);
    assert_eq!(cli_envelope["data"]["allowed_actions"], json!(["complete"]));
    assert_eq!(canonical_feed(cli_envelope["data"].clone()), *expected);

    let mcp_tools = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let mcp = McpToolRouter::authenticated(mcp_tools.clone(), user_token).unwrap();
    assert_eq!(
        canonical_feed(
            mcp.call(McpToolCall {
                tool: "get_feed_batch".into(),
                arguments: json!({"size": size.parse::<usize>().unwrap()}),
            })
            .unwrap()
        ),
        *expected
    );
    drop(mcp);
    drop(mcp_tools);

    let streamable_mcp_tools = AgentTools::open_initialized_home_node(&home_dir.0).unwrap();
    let streamable_mcp = streamable_http_router(streamable_mcp_tools);
    let response = streamable_mcp
        .oneshot(
            Request::post("/mcp")
                .header("authorization", format!("Bearer {user_token}"))
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2025-06-18")
                .body(Body::from(
                    json!({
                        "jsonrpc": "2.0",
                        "id": "first-release-feed",
                        "method": "tools/call",
                        "params": {
                            "name": "get_feed_batch",
                            "arguments": {"size": size.parse::<usize>().unwrap()}
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let response: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        canonical_feed(response["result"]["structuredContent"]["value"].clone()),
        *expected
    );
    let http_tools = AgentTools::open_home_node(&home_dir.0, seed_store).unwrap();
    let response = router(http_tools)
        .oneshot(
            Request::get(format!("/feed?size={size}"))
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        canonical_feed(
            serde_json::from_slice::<Value>(
                &axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
            )
            .unwrap()
        ),
        *expected
    );
}

async fn assert_home_public_exports_are_private(home: &AgentTools, private_values: &[&str]) {
    // Act: inspect every unauthenticated Home Node federation root after private activity.
    let public_home = || {
        router_with_options(
            home.clone(),
            "https://home.example",
            RouterOptions {
                dev_tokens_allowed: false,
                owner_access_allowed: false,
            },
        )
    };
    let mut public_bodies = String::new();
    for path in [
        "/.well-known/stumble-node",
        "/federation/node",
        "/federation/pods",
    ] {
        let response = public_home()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        public_bodies.push_str(&String::from_utf8_lossy(&body));
    }

    // Assert: exported protocol artifacts contain neither values nor private projections.
    for private_value in private_values {
        assert!(!public_bodies.contains(private_value));
    }
    for private_field in [
        "taste_profile",
        "feedback_events",
        "saved_content_references",
        "feed_batches",
        "discovery_tasks",
        "harness_grants",
        "api_tokens",
        "subscriptions",
    ] {
        assert!(!public_bodies.contains(private_field));
    }
}

struct TwoNodeScenario {
    home_dir: TestDataDir,
    _origin_dir: TestDataDir,
    home: AgentTools,
    origin: AgentTools,
    primary_pod: Pod,
    secondary_pod: Pod,
    worker: AuthContext,
    worker_token: String,
    user: AuthContext,
    user_token: String,
}

struct DiscoveryEvidence {
    task: DiscoveryTask,
    local_item_id: ContentItemId,
}

struct FederationEvidence {
    _server: OriginServer,
    public_origin_pod: Pod,
    origin_owner: AuthContext,
    origin_content_item_id: ContentItemId,
    subscription: Subscription,
    local_placement: PodPlacement,
    synchronized_content_item_id: ContentItemId,
}

struct FeedMixEvidence {
    exploration_content_item_id: ContentItemId,
    competing_content_item_ids: Vec<ContentItemId>,
}

struct CompositionEvidence {
    batch: FeedBatch,
    score_before_feedback: f32,
}

fn arrange_two_node_scenario() -> TwoNodeScenario {
    // Arrange: two independent real SQLite nodes and capability-scoped harnesses.
    let home_dir = TestDataDir::initialize_with_stumble("home");
    let origin_dir = TestDataDir::initialize_with_stumble("origin");
    let home = AgentTools::open_initialized_home_node(&home_dir.0).unwrap();
    let origin = AgentTools::open_initialized_home_node(&origin_dir.0).unwrap();
    let (bootstrap, _) = harness(
        &home,
        "Interactive Pod bootstrap",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::PackageManagement,
        ],
        None,
    );
    let primary_pod = home
        .create_private_pod_with_package(
            &bootstrap,
            CreatePrivatePodWithPackageRequest {
                name: "Resilient systems".into(),
                slug: "resilient-systems".into(),
                description: "Production resilience reports".into(),
                package: package(),
            },
        )
        .unwrap()
        .pod;
    let secondary_pod = home
        .create_pod(
            &bootstrap,
            CreatePodRequest {
                name: "Recovery practice".into(),
                slug: "recovery-practice".into(),
                description: "Practical recovery guidance".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let (worker, worker_token) = harness(
        &home,
        "Scoped unattended discovery worker private needle",
        AgentHarnessKind::Unattended,
        vec![
            HarnessCapability::DiscoveryTasks,
            HarnessCapability::CandidateSubmission,
        ],
        Some(vec![primary_pod.id, secondary_pod.id]),
    );
    let (user, user_token) = harness(
        &home,
        "Interactive Feed operator private needle",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::FeedRead,
            HarnessCapability::Feedback,
            HarnessCapability::CandidateSubmission,
            HarnessCapability::PodCuration,
            HarnessCapability::SubscriptionManagement,
        ],
        None,
    );
    for pod in [&primary_pod, &secondary_pod] {
        home.join_pod(&user, &pod.slug).unwrap();
        let pod_id = pod.id;
        home.set_pod_curation_policy(&user, pod_id, CurationPolicy::Manual, Utc::now())
            .unwrap();
    }
    TwoNodeScenario {
        home_dir,
        _origin_dir: origin_dir,
        home,
        origin,
        primary_pod,
        secondary_pod,
        worker,
        worker_token,
        user,
        user_token,
    }
}

fn discover_and_curate_local_content(
    scenario: &TwoNodeScenario,
    now: chrono::DateTime<Utc>,
) -> DiscoveryEvidence {
    // Act: wake, claim, submit, complete, curate, and route one task-driven Candidate.
    let task = materialize_and_wake_discovery(
        &scenario.home,
        &scenario.home_dir,
        &scenario.worker,
        &scenario.worker_token,
        scenario.primary_pod.id,
        now,
    );
    let claimed = scenario
        .home
        .claim_discovery_task(
            &scenario.worker,
            task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    let submitted = submit_candidate(
        &scenario.home,
        &scenario.worker,
        &claimed,
        &[scenario.primary_pod.id],
        "https://example.com/control-plane-recovery?utm_source=harness",
    );
    scenario
        .home
        .complete_discovery_task(&scenario.worker, task.id, now + Duration::seconds(1))
        .unwrap();
    let curated = scenario
        .home
        .curate_candidate(&scenario.user, submitted.candidate.id, now)
        .unwrap();

    // Assert: curation starts at the task Pod and retains one canonical identity when routed.
    assert_eq!(curated.placements.len(), 1);
    let first_placement = scenario
        .home
        .review_candidate_placement(
            &scenario.user,
            submitted.candidate.id,
            scenario.primary_pod.id,
            PlacementReviewDecision::Accept,
            None,
            now,
        )
        .unwrap();
    scenario
        .home
        .route_candidate_placement(
            &scenario.user,
            submitted.candidate.id,
            RouteCandidatePlacementRequest::new(
                scenario.secondary_pod.id,
                "Independent recovery-practice evidence",
                CandidateConfidence::new(0.9).unwrap(),
            )
            .unwrap(),
            now,
        )
        .unwrap();
    let second_placement = scenario
        .home
        .review_candidate_placement(
            &scenario.user,
            submitted.candidate.id,
            scenario.secondary_pod.id,
            PlacementReviewDecision::Accept,
            None,
            now,
        )
        .unwrap();
    assert_eq!(
        first_placement.content_item_id,
        second_placement.content_item_id
    );
    DiscoveryEvidence {
        task,
        local_item_id: first_placement.content_item_id.unwrap(),
    }
}

async fn withdraw_and_synchronize_origin_placement(
    scenario: &TwoNodeScenario,
    federation: &FederationEvidence,
    now: chrono::DateTime<Utc>,
) {
    // Arrange and Act: independently approve the public withdrawal.
    let (withdrawer, _) = harness(
        &scenario.origin,
        "Origin withdrawal proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        Some(vec![federation.public_origin_pod.id]),
    );
    let (approver, _) = harness(
        &scenario.origin,
        "Origin withdrawal approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        Some(vec![federation.public_origin_pod.id]),
    );
    let outcome = scenario
        .origin
        .request_remove_submission_from_pod(
            &withdrawer,
            &federation.public_origin_pod.slug,
            federation.origin_content_item_id.into(),
            now + Duration::days(32),
        )
        .unwrap();
    let RemoveSubmissionOutcome::PendingApproval(proposal) = outcome else {
        panic!("public withdrawal must require independent approval");
    };
    scenario
        .origin
        .approve_pending_proposal(&approver, proposal.id, now + Duration::days(32))
        .unwrap();
    let incremental = scenario
        .origin
        .federation_pod_snapshot(
            &federation.origin_owner,
            &federation.public_origin_pod.slug,
            federation.subscription.last_event_hash.as_deref(),
        )
        .unwrap();

    // Act and Assert: reject tampering, then fetch and project the valid cursor segment.
    let mut tampered = incremental;
    tampered.events.first_mut().unwrap().payload_json["placement_tombstone"]["withdrawn_at"] =
        json!("2020-01-01T00:00:00Z");
    assert!(matches!(
        scenario.home.synchronize_subscription(
            &scenario.user,
            federation.subscription.id,
            tampered
        ),
        Err(AgentToolsError::Store(StoreError::InvalidSignature))
    ));
    let synchronized = stumble_sync::synchronize_subscription_from_origin(
        &scenario.home,
        &scenario.user,
        federation.subscription.id,
    )
    .await
    .unwrap();
    assert_eq!(synchronized.imported_events, 1);
    let retained = scenario
        .home
        .pod_placement(
            &scenario.user,
            federation.local_placement.candidate_id,
            scenario.primary_pod.id,
        )
        .unwrap();
    assert_eq!(retained.status, PodPlacementStatus::Accepted);
    assert_eq!(
        retained.origin_placements,
        federation.local_placement.origin_placements
    );
    assert_eq!(retained.origin_withdrawals.len(), 1);
}

async fn establish_federation(
    scenario: &TwoNodeScenario,
    now: chrono::DateTime<Utc>,
) -> FederationEvidence {
    let public_origin_pod = create_public_pod(&scenario.origin, "origin-operations");
    let origin_content_item_id =
        accept_origin_content_item_placement(&scenario.origin, &public_origin_pod);
    let origin_owner = scenario.origin.default_auth_context().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let origin_router = router_with_base_url(scenario.origin.clone(), &base_url);
    let server = OriginServer {
        base_url: base_url.clone(),
        task: tokio::spawn(async move { axum::serve(listener, origin_router).await }),
    };

    // Retain a signed Explore sample without making its Pod Feed-eligible.
    let exploration_pod = create_public_pod(&scenario.origin, "exploration-operations");
    accept_origin_content_item_placement(&scenario.origin, &exploration_pod);
    let announcement = scenario
        .origin
        .pod_announcement(
            &origin_owner,
            &exploration_pod.slug,
            &format!(
                "{}/federation/pods/{}",
                server.base_url, exploration_pod.slug
            ),
        )
        .unwrap();
    let samples = scenario
        .origin
        .pod_explore_samples(&origin_owner, &announcement, 1)
        .unwrap();
    scenario.home.index_pod_announcement(announcement).unwrap();
    scenario
        .home
        .accept_pod_explore_samples(&scenario.user, samples)
        .unwrap();
    let explore = scenario
        .home
        .explore_public_pods(
            &scenario.user,
            ExploreRequest::new("exploration", 10, 1).unwrap(),
        )
        .unwrap();
    assert_eq!(explore.results.len(), 1);
    assert!(!explore.results[0].is_subscribed);
    assert_eq!(explore.results[0].sample_content_references.len(), 1);

    let synchronized = stumble_sync::subscribe_pod_from_url(
        &scenario.home,
        &scenario.user,
        &format!("{}/federation/pods/origin-operations", server.base_url),
    )
    .await
    .unwrap();
    scenario
        .home
        .set_priority_subscription(&scenario.user, synchronized.subscription.local_pod_id, true)
        .unwrap();
    let synchronized_content_item_id = scenario
        .home
        .accepted_placements_for_pod(&scenario.user, synchronized.subscription.local_pod_id)
        .unwrap()[0]
        .content_item_id;
    let local_placement = scenario
        .home
        .add_content_item_to_pod(
            &scenario.user,
            AddContentItemToPodRequest::new(
                synchronized_content_item_id,
                scenario.primary_pod.id,
                Some("Keep this recovery report locally".into()),
            )
            .unwrap(),
            now,
        )
        .unwrap();
    assert_eq!(local_placement.origin_placements.len(), 1);

    FederationEvidence {
        _server: server,
        public_origin_pod,
        origin_owner,
        origin_content_item_id,
        subscription: synchronized.subscription,
        local_placement,
        synchronized_content_item_id,
    }
}

fn arrange_feed_mix_evidence(
    scenario: &TwoNodeScenario,
    now: chrono::DateTime<Utc>,
) -> FeedMixEvidence {
    // A different local User publishes one Feed-eligible Exploration Pod.
    let (exploration_curator, publication_approver) = grant_alternate_curator(&scenario.home, now);
    let exploration_pod = scenario
        .home
        .create_pod(
            &exploration_curator,
            CreatePodRequest {
                name: "Independent exploration".into(),
                slug: "independent-exploration".into(),
                description: "Public exploration outside the Feed User's memberships".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    scenario
        .home
        .set_pod_curation_policy(
            &exploration_curator,
            exploration_pod.id,
            CurationPolicy::Manual,
            now,
        )
        .unwrap();
    let exploration_content_item_id = accept_local_candidate(
        &scenario.home,
        &exploration_curator,
        &exploration_pod,
        "exploration-release",
        now,
    );
    let publication = match scenario
        .home
        .request_set_pod_visibility(
            &exploration_curator,
            exploration_pod.id,
            Visibility::Public,
            now,
        )
        .unwrap()
    {
        PodVisibilityOutcome::PendingApproval(proposal) => proposal,
        PodVisibilityOutcome::Updated(_) => panic!("publication must require approval"),
    };
    scenario
        .home
        .approve_pending_proposal(&publication_approver, publication.id, now)
        .unwrap();

    // Two non-priority candidates compete for the remaining subscribed slot.
    let competing_content_item_ids = ["competing-alpha", "competing-beta"]
        .into_iter()
        .map(|label| {
            accept_local_candidate(
                &scenario.home,
                &scenario.user,
                &scenario.secondary_pod,
                label,
                now,
            )
        })
        .collect();
    FeedMixEvidence {
        exploration_content_item_id,
        competing_content_item_ids,
    }
}

fn prove_complete_feed_composition(
    scenario: &TwoNodeScenario,
    discovery: &DiscoveryEvidence,
    federation: &FederationEvidence,
    feed_mix: &FeedMixEvidence,
    now: chrono::DateTime<Utc>,
) -> CompositionEvidence {
    let complete_mix = FeedMix::new(50, 25, 25, 3, 2).unwrap();
    // Four slots make subscribed, Exploration, and Old Gem quotas independently observable.
    let mixed = scenario
        .home
        .get_feed_batch(
            &scenario.user,
            FeedBatchRequest::new(4)
                .unwrap()
                .with_feed_mix(complete_mix),
            now,
        )
        .unwrap();
    assert_eq!(mixed.items.len(), 4);
    let exploration = mixed
        .items
        .iter()
        .find(|item| item.kind == FeedItemKind::Exploration)
        .unwrap();
    assert!(exploration.is_exploration);
    assert_eq!(
        exploration.content_reference.content_item_id,
        feed_mix.exploration_content_item_id
    );
    let local = mixed
        .items
        .iter()
        .find(|item| item.content_reference.content_item_id == discovery.local_item_id)
        .unwrap();
    assert_eq!(local.kind, FeedItemKind::OldGem);
    let score_before_feedback = local.ranking_evidence.attention_value;
    let priority_remote = mixed
        .items
        .iter()
        .find(|item| {
            item.content_reference.content_item_id == federation.synchronized_content_item_id
        })
        .unwrap();
    assert_eq!(priority_remote.kind, FeedItemKind::Subscribed);
    assert!(priority_remote
        .placements
        .iter()
        .any(|placement| placement.pod_id == federation.subscription.local_pod_id));
    assert!(priority_remote
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason == "Priority Subscription guaranteed bounded representation"));
    assert_eq!(
        mixed
            .items
            .iter()
            .filter(|item| feed_mix
                .competing_content_item_ids
                .contains(&item.content_reference.content_item_id))
            .count(),
        1
    );
    assert_eq!(
        scenario
            .home
            .get_feed_batch(
                &scenario.user,
                FeedBatchRequest::new(4)
                    .unwrap()
                    .with_feed_mix(complete_mix),
                now,
            )
            .unwrap(),
        mixed
    );
    CompositionEvidence {
        batch: mixed,
        score_before_feedback,
    }
}

fn apply_feedback_and_prove_reranking(
    scenario: &TwoNodeScenario,
    discovery: &DiscoveryEvidence,
    federation: &FederationEvidence,
    composition: &CompositionEvidence,
    now: chrono::DateTime<Utc>,
) -> FeedBatch {
    scenario
        .home
        .record_feed_feedback(
            &scenario.user,
            federation.synchronized_content_item_id,
            FeedbackKind::Interesting,
            None,
            Some("private feedback needle".into()),
            now,
        )
        .unwrap();
    scenario
        .home
        .record_feed_feedback(
            &scenario.user,
            discovery.local_item_id,
            FeedbackKind::Saved,
            None,
            None,
            now,
        )
        .unwrap();
    assert!(scenario
        .home
        .taste_profile(&scenario.user)
        .unwrap()
        .learned
        .iter()
        .any(|weight| {
            weight.signal == LearnedTasteSignal::Topic("resilience".into()) && weight.weight > 0.0
        }));
    scenario
        .home
        .complete_feed_batch(&scenario.user, composition.batch.id, now)
        .unwrap();
    let ranked = scenario
        .home
        .get_feed_batch(
            &scenario.user,
            FeedBatchRequest::new(4)
                .unwrap()
                .with_feed_mix(FeedMix::new(50, 25, 25, 3, 2).unwrap())
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + Duration::seconds(1),
        )
        .unwrap();
    let ranked_local = ranked
        .items
        .iter()
        .find(|item| item.content_reference.content_item_id == discovery.local_item_id)
        .unwrap();
    assert!(ranked_local.ranking_evidence.attention_value > composition.score_before_feedback);
    assert!(ranked_local
        .ranking_evidence
        .reasons
        .iter()
        .any(|reason| reason.contains("Learned topic 'resilience' affinity increased value")));
    ranked
}

fn prove_unavailable_category_backfill(
    scenario: &TwoNodeScenario,
    ranked: &FeedBatch,
    now: chrono::DateTime<Utc>,
) {
    // Once all categories were just delivered, recurrence zero makes only Old Gems eligible;
    // unavailable subscribed and Exploration quotas backfill to the requested finite size.
    scenario
        .home
        .complete_feed_batch(&scenario.user, ranked.id, now + Duration::seconds(1))
        .unwrap();
    let backfilled = scenario
        .home
        .get_feed_batch(
            &scenario.user,
            FeedBatchRequest::new(2)
                .unwrap()
                .with_feed_mix(FeedMix::new(50, 50, 0, 3, 2).unwrap())
                .with_recurrence_penalty_days(RecurrencePenaltyDays::new(0).unwrap()),
            now + Duration::seconds(2),
        )
        .unwrap();
    assert_eq!(backfilled.items.len(), 2);
    assert!(backfilled
        .items
        .iter()
        .all(|item| item.kind == FeedItemKind::OldGem));
}

fn deliver_local_item_for_old_gem(
    scenario: &TwoNodeScenario,
    discovery: &DiscoveryEvidence,
    now: chrono::DateTime<Utc>,
) {
    let delivered_at = now - Duration::days(31);
    let first = scenario
        .home
        .get_feed_batch(
            &scenario.user,
            FeedBatchRequest::new(100).unwrap(),
            delivered_at,
        )
        .unwrap();
    assert!(first
        .items
        .iter()
        .any(|item| item.content_reference.content_item_id == discovery.local_item_id));
    scenario
        .home
        .complete_feed_batch(&scenario.user, first.id, delivered_at)
        .unwrap();
}

fn prove_restart(scenario: &TwoNodeScenario, federation: &FederationEvidence) {
    let candidate_id = federation.local_placement.candidate_id;
    let primary_pod_id = scenario.primary_pod.id;
    let restarted = AgentTools::open_home_node(&scenario.home_dir.0, seed_store).unwrap();
    let restarted_user = restarted
        .authenticate_token(&scenario.user_token)
        .unwrap()
        .unwrap();
    let retained = restarted
        .pod_placement(&restarted_user, candidate_id, primary_pod_id)
        .unwrap();
    assert_eq!(retained.status, PodPlacementStatus::Accepted);
    assert_eq!(retained.origin_withdrawals.len(), 1);
}

#[tokio::test]
async fn scoped_harness_proves_the_complete_headless_two_node_first_release() {
    prove_json_migration_guards();
    let scenario = arrange_two_node_scenario();
    let now = Utc::now();
    let discovery = discover_and_curate_local_content(&scenario, now);
    deliver_local_item_for_old_gem(&scenario, &discovery, now);
    let federation = establish_federation(&scenario, now).await;
    let feed_mix = arrange_feed_mix_evidence(&scenario, now);
    let composition =
        prove_complete_feed_composition(&scenario, &discovery, &federation, &feed_mix, now);
    let ranked =
        apply_feedback_and_prove_reranking(&scenario, &discovery, &federation, &composition, now);
    prove_unavailable_category_backfill(&scenario, &ranked, now);
    withdraw_and_synchronize_origin_placement(&scenario, &federation, now).await;
    assert_home_public_exports_are_private(
        &scenario.home,
        &[
            "private feedback needle",
            "Scoped unattended discovery worker private needle",
            "Interactive Feed operator private needle",
            scenario.worker_token.as_str(),
            &discovery.task.id.to_string(),
        ],
    )
    .await;
    // Only placement arrays are canonicalized; ranked Feed item order remains contractual.
    let adapter_expected = canonical_feed(
        serde_json::to_value(
            scenario
                .home
                .get_feed_batch(&scenario.user, FeedBatchRequest::new(2).unwrap(), now)
                .unwrap(),
        )
        .unwrap(),
    );
    assert_adapter_parity(&scenario.home_dir, &scenario.user_token, &adapter_expected).await;
    prove_restart(&scenario, &federation);
}
