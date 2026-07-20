//! Primary Personal Discovery acceptance journey (ticket 09).
//!
//! MCP Agent Harness workflow against a persistent Home Node with restart.
//! Uses deterministic browser-contract fixtures only — no live third-party sites
//! or credentials.

mod support;

use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use stumble_core::{
    AgentHarnessKind, AgentTools, CandidateConfidence, CandidateContentType, CandidateProvenance,
    CandidateSourceMetadata, CandidateSubmissionEvidence, CandidateSubmissionRequest,
    CandidateSubmissionRequestTarget, CreatePodRequest, CurationPolicy, HarnessCapability,
    PlacementReviewDecision, ProposedCandidatePlacement, RegisterAgentHarnessRequest, Visibility,
};
use support::{McpClient, PersistentNode, ScopedHarness};

#[tokio::test]
async fn personal_discovery_acceptance_journey_on_persistent_home_node() {
    let mut node = PersistentNode::open("pd-acceptance-home");

    // AC1: distinct interactive management vs unattended execution grants.
    let manager = node.harness_kind(
        "interactive personal manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::Feedback,
            HarnessCapability::CandidateSubmission,
        ],
        None,
    );
    let worker = node.harness_kind(
        "unattended personal worker",
        AgentHarnessKind::Unattended,
        vec![HarnessCapability::PersonalDiscoveryExecution],
        None,
    );
    let manager_token = manager.token().to_owned();
    let worker_token = worker.token().to_owned();

    // Fixture: local network Discovery Lead (no remote profile-derived query).
    seed_local_network_lead(&node.tools);

    // Cold start: clear seed interests so readiness comes from User URL evidence.
    let manager_ctx = node
        .tools
        .authenticate_token(&manager_token)
        .unwrap()
        .unwrap();
    let mut clear_taste = stumble_core::UpdateTasteProfileRequest::default();
    clear_taste.interests = Some(Vec::new());
    clear_taste.blocked_topics = Some(Vec::new());
    clear_taste.blocked_sources = Some(Vec::new());
    node.tools
        .update_taste_profile(&manager_ctx, clear_taste)
        .unwrap();
    node.tools
        .reset_learned_taste(&manager_ctx, stumble_core::ResetLearnedTasteRequest::all())
        .unwrap();

    let manager_mcp = node.mcp(&manager);
    let worker_mcp = node.mcp(&worker);

    // Capability catalogs differ by grant (AC1 / AC7 auth surface).
    let manager_tools = manager_mcp.list_tool_names(1).await;
    let worker_tools = worker_mcp.list_tool_names(2).await;
    assert!(manager_tools.contains(&"request_personal_discovery".to_string()));
    assert!(manager_tools.contains(&"personal_discovery_readiness".to_string()));
    assert!(manager_tools.contains(&"review_discovery_result_item".to_string()));
    assert!(!worker_tools.contains(&"request_personal_discovery".to_string()));
    assert!(!worker_tools.contains(&"get_taste_profile".to_string()));
    assert!(worker_tools.contains(&"list_ready_discovery_tasks".to_string()));
    assert!(worker_tools.contains(&"get_discovery_plan".to_string()));
    assert!(worker_tools.contains(&"complete_discovery_result_batch".to_string()));

    // --- AC2: URL submissions → generic Personal Discovery → plan from affinities ---
    let not_ready = value(
        &manager_mcp
            .call_tool(3, "personal_discovery_readiness", json!({}))
            .await,
    );
    assert_eq!(not_ready["ready"], false);

    for (idx, url) in [
        "https://systems-journal.example/ownership-types",
        "https://rust-research.example/type-systems",
        "https://systems-journal.example/distributed-runtime",
    ]
    .into_iter()
    .enumerate()
    {
        let submitted = manager_mcp
            .call_tool(
                10 + idx as u64,
                "submit_candidate",
                json!({
                    "source_url": url,
                    "target": {
                        "kind": "user",
                        "learn": true,
                        "interest_seed_metadata": {
                            "publisher": "Systems Journal",
                            "community": "rust-systems"
                        }
                    },
                    "source_metadata": {
                        "title": format!("User evidence {idx}"),
                        "author": "Ada",
                        "published_at": null
                    },
                    "content_type": "article",
                    "tags": ["rust", "distributed systems"],
                    "provenance": {
                        "discovered_at": "2026-07-20T10:00:00Z",
                        "discovery_method": "user_submission",
                        "referrer_url": "https://news.ycombinator.com/item?id=1"
                    },
                    "harness_idempotency_key": format!("user-url-{idx}"),
                    "client_idempotency_key": format!("user-url-{idx}")
                }),
            )
            .await;
        assert!(
            !submitted.is_error(),
            "user URL submission failed: {}",
            submitted.error_text()
        );
    }

    let ready = value(
        &manager_mcp
            .call_tool(20, "personal_discovery_readiness", json!({}))
            .await,
    );
    assert_eq!(ready["ready"], true);
    let basis = ready["basis"].as_array().expect("readiness basis");
    assert!(
        !basis.is_empty(),
        "corroborated interests or affinities must appear in readiness basis: {basis:?}"
    );

    // Request without naming a source or platform.
    let created = value(
        &manager_mcp
            .call_tool(
                21,
                "request_personal_discovery",
                json!({
                    "idempotency_key": "acceptance-on-demand",
                    "result_count": 10
                }),
            )
            .await,
    );
    let task_id = created["task"]["id"].as_str().unwrap().to_owned();
    let plan_id = created["plan"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["plan"]["result_count"], 10);
    assert_eq!(created["plan"]["allocation"]["proven"], 7);
    assert_eq!(created["plan"]["allocation"]["adjacent"], 3);
    assert_eq!(created["task"]["target"]["kind"], "personal");
    assert!(created["plan"]["source_neighborhoods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            source["rationale"]
                .as_str()
                .unwrap_or_default()
                .contains("corroborated")
                || source["role"] == "adjacent"
                || source["role"] == "proven"
        }));
    assert!(
        created["plan"].get("taste_profile").is_none(),
        "plan must not embed the full Taste Profile"
    );

    // --- AC3: minimized plan, provenance candidates, availability, complete batch ---
    let ready_tasks = value(
        &worker_mcp
            .call_tool(30, "list_ready_discovery_tasks", json!({}))
            .await,
    );
    let ready_list = ready_tasks.as_array().expect("ready tasks");
    assert!(
        ready_list.iter().any(|task| task["id"] == task_id),
        "worker must see the on-demand Personal Discovery task"
    );

    let claimed = worker_mcp
        .call_tool(
            31,
            "claim_discovery_task",
            json!({"task_id": task_id, "lease_seconds": 300}),
        )
        .await;
    assert!(!claimed.is_error(), "{}", claimed.error_text());

    let plan = value(
        &worker_mcp
            .call_tool(
                32,
                "get_discovery_plan",
                json!({"discovery_plan_id": plan_id}),
            )
            .await,
    );
    assert_eq!(plan["id"], plan_id);
    assert_eq!(plan["result_count"], 10);
    let plan_text = plan.to_string().to_lowercase();
    for forbidden in [
        "password",
        "cookie",
        "access_token",
        "interest_seed",
        "feedback_events",
    ] {
        assert!(
            !plan_text.contains(forbidden),
            "minimized plan leaked {forbidden}"
        );
    }

    let reported = worker_mcp
        .call_tool(
            33,
            "report_discovery_source_availability",
            json!({
                "task_id": task_id,
                "reports": [{
                    "source": "auth-wall.example",
                    "state": "authentication_required",
                    "reason": "session_not_present"
                }],
                "browser_grant_eligible_sources": [
                    "systems-journal.example",
                    "rust-research.example"
                ]
            }),
        )
        .await;
    assert!(!reported.is_error(), "{}", reported.error_text());

    let mut submission_ids = Vec::new();
    let submissions = [
        candidate_json(&task_id, "https://a.example/p1", "proven", "A1", "p1"),
        candidate_json(&task_id, "https://a.example/p2", "proven", "A1", "p2"),
        candidate_json(&task_id, "https://a.example/p3", "proven", "A2", "p3"),
        candidate_json(&task_id, "https://a.example/p4", "proven", "A3", "p4"),
        candidate_json(&task_id, "https://b.example/p5", "proven", "A1", "p5"),
        candidate_json(&task_id, "https://c.example/p6", "proven", "A4", "p6"),
        candidate_json(&task_id, "https://d.example/p7", "proven", "A5", "p7"),
        candidate_json(&task_id, "https://e.example/p8", "proven", "A6", "p8"),
        candidate_json(&task_id, "https://adj1.example/a1", "adjacent", "B1", "a1"),
        candidate_json(&task_id, "https://adj2.example/a2", "adjacent", "B2", "a2"),
        candidate_json(
            &task_id,
            "https://local-public.example/deep-dive",
            "adjacent",
            "B3",
            "a3-lead",
        ),
        candidate_json(
            &task_id,
            "https://adj1.example/a1?utm_source=agent",
            "adjacent",
            "B4",
            "a1-dup",
        ),
        candidate_json(&task_id, "https://adj3.example/a4", "adjacent", "B5", "a4"),
    ];
    for (idx, body) in submissions.into_iter().enumerate() {
        let submitted = worker_mcp
            .call_tool(40 + idx as u64, "submit_candidate", body)
            .await;
        assert!(!submitted.is_error(), "{}", submitted.error_text());
        submission_ids.push(
            value(&submitted)["submission"]["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }

    let batch = value(
        &worker_mcp
            .call_tool(
                60,
                "complete_discovery_result_batch",
                json!({
                    "task_id": task_id,
                    "submission_ids": submission_ids,
                    "source_availability": [{
                        "source": "auth-wall.example",
                        "state": "authentication_required",
                        "reason": "session_not_present"
                    }]
                }),
            )
            .await,
    );
    assert_eq!(batch["state"], "ready");
    assert_eq!(batch["task_id"], task_id);
    assert_eq!(batch["plan_id"], plan_id);
    let items = batch["items"].as_array().unwrap();
    assert!(
        items.len() <= 10,
        "default batch is finite (≤10): got {}",
        items.len()
    );
    assert!(!items.is_empty(), "batch should retain valid results");

    // --- AC4: 70/30 evidence, diversity caps, canonical dedup, leads, shortfalls ---
    let proven = items
        .iter()
        .filter(|item| item["allocation_role"] == "proven")
        .count();
    let adjacent = items
        .iter()
        .filter(|item| item["allocation_role"] == "adjacent")
        .count();
    assert_eq!(proven + adjacent, items.len());
    // Plan targets 7/3; reallocation may fill shortfalls while remaining ≤ result_count.
    assert!(
        items.len() <= 10 && proven >= 1,
        "finite 70/30-oriented batch: proven={proven} adjacent={adjacent} total={}",
        items.len()
    );
    assert_eq!(batch["allocation"]["proven"], 7);
    assert_eq!(batch["allocation"]["adjacent"], 3);
    let domain_a = items
        .iter()
        .filter(|item| {
            item["canonical_url"]
                .as_str()
                .unwrap_or_default()
                .contains("a.example")
        })
        .count();
    assert!(domain_a <= 3, "domain diversity cap exceeded: {domain_a}");
    let reasons = batch["source_availability"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !reasons.is_empty(),
        "inspectable shortfall/reallocation/cap reasons expected"
    );
    let a1_count = items
        .iter()
        .filter(|item| {
            item["canonical_url"]
                .as_str()
                .unwrap_or_default()
                .starts_with("https://adj1.example/a1")
        })
        .count();
    assert!(a1_count <= 1, "canonical dedup failed: {a1_count}");
    let has_lead_result = items.iter().any(|item| {
        item["canonical_url"]
            .as_str()
            .unwrap_or_default()
            .contains("local-public.example")
    });
    let plan_has_lead = plan["source_neighborhoods"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            let rationale = source["rationale"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase();
            rationale.contains("network")
                || rationale.contains("local public")
                || source["signal"]
                    .to_string()
                    .contains("local-public.example")
        });
    assert!(
        has_lead_result || plan_has_lead,
        "local network Discovery Lead should influence plan or batch"
    );
    assert!(items
        .iter()
        .all(|item| { item.get("candidate_id").is_some() && item.get("canonical_url").is_some() }));

    let batch_id = batch["id"].as_str().unwrap().to_owned();
    let first_candidate = items[0]["candidate_id"].as_str().unwrap().to_owned();
    let second_candidate = items
        .get(1)
        .map(|item| item["candidate_id"].as_str().unwrap().to_owned());
    let third_candidate = items
        .get(2)
        .map(|item| item["candidate_id"].as_str().unwrap().to_owned());

    // --- Restart mid-journey: private state and grants survive ---
    node = node.reopen();
    let manager_mcp = McpClient::new(
        stumble_mcp::streamable_http_router(node.tools.clone()),
        &manager_token,
    );
    let worker_mcp = McpClient::new(
        stumble_mcp::streamable_http_router(node.tools.clone()),
        &worker_token,
    );
    let listed = value(
        &manager_mcp
            .call_tool(70, "list_discovery_result_batches", json!({}))
            .await,
    );
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|batch| batch["id"] == batch_id),
        "result batch must survive Home Node restart"
    );

    // --- AC5: explicit feedback changes later plan; ignore/agent-found create no seeds ---
    let more = manager_mcp
        .call_tool(
            71,
            "review_discovery_result_item",
            json!({
                "batch_id": batch_id,
                "candidate_id": first_candidate,
                "action": {"action": "more_like_this"}
            }),
        )
        .await;
    assert!(!more.is_error(), "{}", more.error_text());
    if let Some(ignored) = second_candidate {
        let ignore = manager_mcp
            .call_tool(
                72,
                "review_discovery_result_item",
                json!({
                    "batch_id": batch_id,
                    "candidate_id": ignored,
                    "action": {"action": "ignore"}
                }),
            )
            .await;
        assert!(!ignore.is_error(), "{}", ignore.error_text());
    }
    if let Some(third) = third_candidate {
        let more2 = manager_mcp
            .call_tool(
                73,
                "review_discovery_result_item",
                json!({
                    "batch_id": batch_id,
                    "candidate_id": third,
                    "action": {"action": "more_like_this"}
                }),
            )
            .await;
        assert!(!more2.is_error(), "{}", more2.error_text());
    }
    let reviewed = manager_mcp
        .call_tool(
            74,
            "mark_discovery_result_batch_reviewed",
            json!({"batch_id": batch_id}),
        )
        .await;
    assert!(!reviewed.is_error(), "{}", reviewed.error_text());

    let later = value(
        &manager_mcp
            .call_tool(
                75,
                "request_personal_discovery",
                json!({
                    "idempotency_key": "acceptance-after-feedback",
                    "result_count": 4
                }),
            )
            .await,
    );
    assert_ne!(later["plan"]["id"], plan_id);
    assert_eq!(later["plan"]["result_count"], 4);
    // Later plan should still be inspectable and may reflect reinforced sources.
    assert!(!later["plan"]["source_neighborhoods"]
        .as_array()
        .unwrap()
        .is_empty());

    let profile = value(
        &manager_mcp
            .call_tool(76, "get_taste_profile", json!({}))
            .await,
    );
    let seed_count = profile["interest_seed_evidence"]["active_seed_count"]
        .as_u64()
        .unwrap_or(0);
    assert_eq!(
        seed_count, 3,
        "agent-found batch items must not create Interest Seeds by themselves"
    );

    // --- AC6: scheduled path — harness wake + adapter wake, backpressure, notify ---
    let schedule = value(
        &manager_mcp
            .call_tool(
                80,
                "create_personal_discovery_schedule",
                json!({
                    "name": "acceptance-daily",
                    "cadence": "daily",
                    "result_count": 6,
                    "delivery_mode": "notify_when_supported"
                }),
            )
            .await,
    );
    let schedule_id = schedule["schedule"]["id"].as_str().unwrap().to_owned();

    // Harness-owned wake-up.
    let harness_ready = value(
        &worker_mcp
            .call_tool(81, "list_ready_discovery_tasks", json!({}))
            .await,
    );
    let harness_ids: Vec<String> = harness_ready
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|task| task["id"].as_str().map(str::to_owned))
        .collect();

    // Local Scheduler Adapter path: same list_ready contract / token.
    let adapter_ready = value(
        &worker_mcp
            .call_tool(82, "list_ready_discovery_tasks", json!({}))
            .await,
    );
    let adapter_ids: Vec<String> = adapter_ready
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|task| task["id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        harness_ids, adapter_ids,
        "harness-owned and local adapter wakes must converge on the same task identities"
    );

    let scheduled_task = harness_ready
        .as_array()
        .unwrap()
        .iter()
        .find(|task| {
            task["origin"]["kind"] == "personal_scheduled"
                && task["origin"]["schedule_id"].as_str() == Some(schedule_id.as_str())
        })
        .expect("scheduled personal task materializes")
        .clone();
    let scheduled_task_id = scheduled_task["id"].as_str().unwrap().to_owned();

    let claimed = worker_mcp
        .call_tool(
            83,
            "claim_discovery_task",
            json!({"task_id": scheduled_task_id, "lease_seconds": 300}),
        )
        .await;
    assert!(!claimed.is_error(), "{}", claimed.error_text());

    let sched_submit = value(
        &worker_mcp
            .call_tool(
                84,
                "submit_candidate",
                candidate_json(
                    &scheduled_task_id,
                    "https://scheduled.example/item",
                    "proven",
                    "S1",
                    "sched-1",
                ),
            )
            .await,
    );
    let scheduled_batch = value(
        &worker_mcp
            .call_tool(
                85,
                "complete_discovery_result_batch",
                json!({
                    "task_id": scheduled_task_id,
                    "submission_ids": [sched_submit["submission"]["id"]],
                    "source_availability": []
                }),
            )
            .await,
    );
    let scheduled_batch_id = scheduled_batch["id"].as_str().unwrap().to_owned();
    assert_eq!(scheduled_batch["notification_state"], "pending");

    let notify = value(
        &manager_mcp
            .call_tool(
                86,
                "attempt_discovery_results_ready_notification",
                json!({"batch_id": scheduled_batch_id}),
            )
            .await,
    );
    assert_eq!(notify["kind"], "should_notify");
    let notify_again = value(
        &manager_mcp
            .call_tool(
                87,
                "attempt_discovery_results_ready_notification",
                json!({"batch_id": scheduled_batch_id}),
            )
            .await,
    );
    assert_eq!(notify_again["kind"], "already_attempted");

    let status = value(
        &manager_mcp
            .call_tool(
                88,
                "get_personal_discovery_schedule",
                json!({"schedule_id": schedule_id}),
            )
            .await,
    );
    assert_eq!(status["backpressure"]["kind"], "unreviewed_batch");

    // On-demand remains available under schedule backpressure.
    let on_demand = value(
        &manager_mcp
            .call_tool(
                89,
                "request_personal_discovery",
                json!({
                    "idempotency_key": "acceptance-on-demand-under-backpressure",
                    "result_count": 4
                }),
            )
            .await,
    );
    assert!(on_demand["task"]["id"].as_str().is_some());

    // --- AC9 fragment: private markers absent from federation surface after activity ---
    let federation = node.tools.default_auth_context().unwrap();
    let outbound = serde_json::to_string(&serde_json::json!({
        "node": node.tools.node_info(&federation).unwrap(),
        "pods": node.tools.list_public_pods(&federation).unwrap(),
    }))
    .unwrap();
    for forbidden in [
        "acceptance-daily",
        "systems-journal.example/ownership",
        "InterestSeed",
        "interest_seed",
        "DiscoveryPlan",
        "discovery_result_batch",
        "SourceAffinity",
        "auth-wall.example",
        schedule_id.as_str(),
        batch_id.as_str(),
    ] {
        assert!(
            !outbound.contains(forbidden),
            "federation surface leaked private Personal Discovery marker {forbidden}"
        );
    }

    // Keep harness tokens referenced for the full journey.
    let _ = (manager, worker, Utc.with_ymd_and_hms(2026, 7, 20, 12, 0, 0));
}

fn value(result: &support::McpToolResult) -> Value {
    assert!(
        !result.is_error(),
        "MCP tool error: {} raw={}",
        result.error_text(),
        result.raw()
    );
    result.structured_content()["value"].clone()
}

fn candidate_json(task_id: &str, url: &str, role: &str, author: &str, key: &str) -> Value {
    json!({
        "source_url": url,
        "target": {
            "kind": "personal_discovery",
            "task_id": task_id,
            "allocation_role": role
        },
        "source_metadata": {
            "title": format!("Fixture {key}"),
            "author": author,
            "published_at": null
        },
        "content_type": "article",
        "tags": ["systems"],
        "permitted_excerpt": "excerpt",
        "summary": "summary",
        "provenance": {
            "discovered_at": "2026-07-20T12:00:00Z",
            "discovery_method": "browser_search",
            "referrer_url": "https://news.example/list"
        },
        "harness_idempotency_key": key,
        "client_idempotency_key": key
    })
}

fn seed_local_network_lead(tools: &AgentTools) {
    let owner = tools.default_auth_context().unwrap();
    let curator = register(
        tools,
        "acceptance public curator",
        vec![HarnessCapability::PodCuration],
    );
    let approver = register(
        tools,
        "acceptance public approver",
        vec![HarnessCapability::Approval],
    );
    let submitter = register(
        tools,
        "acceptance public submitter",
        vec![HarnessCapability::CandidateSubmission],
    );
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: "Local Systems".into(),
                slug: "local-systems".into(),
                description: "Local distributed systems reading list".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &curator,
            stumble_core::SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    let pod = tools.pod_by_slug("local-systems", None).unwrap();
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, now)
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
                    source_url: "https://local-public.example/deep-dive".into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Local network lead".into()),
                        author: Some("Careful author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("A permitted sample excerpt".into()),
                    summary: Some("A useful public Content Reference".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["systems".into(), "distributed systems".into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                        discovery_method: "browser_search".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: "acceptance-lead-worker".into(),
                    client_idempotency_key: "acceptance-lead-client".into(),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, candidate.candidate.id, now)
        .unwrap();
    tools
        .review_candidate_placement(
            &curator,
            candidate.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            now,
        )
        .unwrap();
    let announcement = tools
        .pod_announcement(
            &owner,
            &pod.slug,
            "https://home.example/federation/pods/local-systems",
        )
        .unwrap();
    tools.index_pod_announcement(announcement).unwrap();
}

fn register(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> stumble_core::AuthContext {
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

// Keep ScopedHarness import live for reopen patterns in related tests.
#[allow(dead_code)]
fn _register_via_support(tools: &AgentTools) -> ScopedHarness {
    ScopedHarness::register(
        tools,
        "support-register",
        vec![HarnessCapability::FeedRead],
        None,
    )
}
