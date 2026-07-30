use axum::{body::Body, http::Request};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use std::process::Command;
use stumble_api::{router_with_options, RouterOptions};
use stumble_core::*;
use stumble_mcp::{streamable_http_router, McpToolCall, McpToolRouter};
use tower::ServiceExt;

pub(crate) struct TestDataDir(pub(crate) std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-first-release-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn initialize_with_stumble(label: &str) -> Self {
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

pub(crate) struct OriginServer {
    pub(crate) base_url: String,
    pub(crate) task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl Drop for OriginServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) fn package() -> PodPackageContents {
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

pub(crate) fn harness(
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

pub(crate) fn create_public_pod(tools: &AgentTools, slug: &str) -> Pod {
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

pub(crate) fn submit_candidate(
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

pub(crate) fn accept_origin_content_item_placement(tools: &AgentTools, pod: &Pod) -> ContentItemId {
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

pub(crate) fn grant_alternate_curator(
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

pub(crate) fn accept_local_candidate(
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

pub(crate) fn canonical_feed(mut value: Value) -> Value {
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

pub(crate) fn materialize_and_wake_discovery(
    home: &AgentTools,
    home_dir: &TestDataDir,
    worker: &AuthContext,
    worker_token: &str,
    pod_id: PodId,
    now: chrono::DateTime<Utc>,
) -> DiscoveryTask {
    // Arrange and Act: materialize due work and invoke the unified runner.
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
    let config_path = home_dir.0.join("runner.yaml");
    std::fs::write(
        &config_path,
        format!(
            "version: 1\ndata_dir: {}\ncredentials:\n  worker:\n    command:\n      program: /usr/bin/printf\n      args: [{}]\nagents:\n  test:\n    program: /usr/bin/true\n    args: [\"{{prompt}}\"]\nworkers:\n  pod:\n    credential: worker\n    agent: test\n    prompt: test discovery\n    event_path: {}\n",
            home_dir.0.display(),
            worker_token,
            scheduler_event.display()
        ),
    )
    .unwrap();
    let scheduler = Command::new(env!("CARGO_BIN_EXE_stumble-runner"))
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "discovery",
            "pod",
        ])
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

pub(crate) async fn assert_adapter_parity(
    home_dir: &TestDataDir,
    user_token: &str,
    expected: &Value,
) {
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
}

pub(crate) async fn assert_home_public_exports_are_private(
    home: &AgentTools,
    private_values: &[&str],
) {
    // Act: inspect every unauthenticated Home Node federation root after private activity.
    let public_home = || {
        router_with_options(
            home.clone(),
            "https://home.example",
            RouterOptions {
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
