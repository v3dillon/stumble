mod support;

use serde_json::json;
use stumble_core::{
    CreatePodOutcome, DiscoveryTask, DiscoveryTaskState, HarnessCapability, MediaReference,
    MediaReferenceType, PodContentItem, PodId, PodPlacementStatus, ProposalStatus, Visibility,
};
use stumble_mcp::{McpToolCall, McpToolRouter};
use support::{EphemeralHttpServer, PersistentNode, ScopedHarness};

#[tokio::test]
async fn agent_harness_discoveries_federate_between_two_independent_nodes() {
    let scenario = TwoNodeScenario::new();

    let pods = scenario.create_private_inbox_and_public_pod().await;
    let origin_harnesses = scenario.scope_origin_harnesses(&pods);
    scenario.verify_origin_scopes(&origin_harnesses).await;
    let discoveries = scenario.discover_six_posts(&pods, &origin_harnesses).await;
    scenario.verify_discovery_cannot_curate(&pods, &origin_harnesses, &discoveries);
    scenario
        .curate_six_posts(&pods, &origin_harnesses, &discoveries)
        .await;
    let private_sentinel = scenario
        .verify_origin_and_add_private_sentinel(&pods, &origin_harnesses)
        .await;
    let origin_http = EphemeralHttpServer::start_origin(scenario.origin.tools.clone()).await;
    let home_harnesses = scenario.scope_home_harnesses();
    scenario.subscribe_home(&home_harnesses, &origin_http).await;
    scenario.verify_public_federation(&home_harnesses).await;
    scenario
        .verify_private_state_stayed_on_origin(
            &origin_harnesses,
            &home_harnesses,
            &discoveries,
            &private_sentinel,
        )
        .await;
}

struct TwoNodeScenario {
    origin: PersistentNode,
    home: PersistentNode,
    origin_creator: ScopedHarness,
    origin_approver: ScopedHarness,
}

struct OriginPods {
    inbox: PodId,
    public: PodId,
}

struct OriginHarnesses {
    discovery: ScopedHarness,
    curator: ScopedHarness,
    reader: ScopedHarness,
}

struct Discoveries {
    task_id: String,
    candidate_ids: Vec<String>,
}

struct HomeHarnesses {
    subscriber: ScopedHarness,
    reader: ScopedHarness,
    private_state_reader: ScopedHarness,
}

impl TwoNodeScenario {
    fn new() -> Self {
        let origin = PersistentNode::open("two-node-origin");
        let home = PersistentNode::open("two-node-home");
        let origin_creator = origin.harness(
            "Origin Pod creator",
            vec![HarnessCapability::PodCuration],
            None,
        );
        let origin_approver = origin.harness(
            "Origin public exposure approver",
            vec![HarnessCapability::Approval],
            None,
        );
        Self {
            origin,
            home,
            origin_creator,
            origin_approver,
        }
    }

    async fn create_private_inbox_and_public_pod(&self) -> OriginPods {
        let creator = self.origin.mcp(&self.origin_creator);
        let inbox = creator
            .call_tool(
                1,
                "create_pod",
                json!({
                    "name": "Federation Acceptance Inbox",
                    "slug": "federation-acceptance-inbox",
                    "description": "Private intake that must never federate",
                    "visibility": "private"
                }),
            )
            .await;
        let inbox = match inbox.create_pod_outcome() {
            CreatePodOutcome::Created(pod) => pod.id,
            CreatePodOutcome::PendingApproval(_) => {
                panic!("private Pod must be created immediately")
            }
        };

        let proposed = creator
            .call_tool(
                2,
                "create_pod",
                json!({
                    "name": "Federated Post Acceptance",
                    "slug": "federated-post-acceptance",
                    "description": "Isolated public Pod for two-node acceptance",
                    "visibility": "public"
                }),
            )
            .await;
        let proposal_id = match proposed.create_pod_outcome() {
            CreatePodOutcome::PendingApproval(proposal) => proposal.id,
            CreatePodOutcome::Created(_) => panic!("public Pod creation requires approval"),
        };
        let approved = self
            .origin
            .mcp(&self.origin_approver)
            .call_tool(
                3,
                "approve_pending_proposal",
                json!({"proposal_id": proposal_id}),
            )
            .await;
        assert_eq!(approved.pending_proposal().status, ProposalStatus::Accepted);

        let origin_pods = creator.call_tool(4, "list_pods", json!({})).await;
        let origin_pods = origin_pods.pods();
        assert!(origin_pods.iter().any(|pod| {
            pod.slug == "federation-acceptance-inbox" && pod.visibility == Visibility::Private
        }));
        let public_pod_id = origin_pods
            .iter()
            .find(|pod| pod.slug == "federated-post-acceptance")
            .map(|pod| pod.id)
            .expect("approved public Pod");
        OriginPods {
            inbox,
            public: public_pod_id,
        }
    }

    fn scope_origin_harnesses(&self, pods: &OriginPods) -> OriginHarnesses {
        let discovery = self.origin.harness(
            "Origin discovery worker",
            vec![
                HarnessCapability::CandidateSubmission,
                HarnessCapability::DiscoveryTasks,
            ],
            Some(vec![pods.inbox]),
        );
        let curator = self.origin.harness(
            "Origin public Pod curator",
            vec![HarnessCapability::PodCuration],
            Some(vec![pods.inbox, pods.public]),
        );
        let reader = self.origin.harness(
            "Origin accepted content reader",
            vec![HarnessCapability::FeedRead],
            Some(vec![pods.public]),
        );
        OriginHarnesses {
            discovery,
            curator,
            reader,
        }
    }

    async fn verify_origin_scopes(&self, harnesses: &OriginHarnesses) {
        let discovery_tools = self
            .origin
            .mcp(&harnesses.discovery)
            .list_tool_names(5)
            .await;
        assert!(has_tool(&discovery_tools, "submit_candidate"));
        assert!(has_tool(
            &discovery_tools,
            "create_immediate_discovery_task"
        ));
        assert!(!has_tool(&discovery_tools, "route_candidate"));
        assert!(!has_tool(&discovery_tools, "approve_pending_proposal"));
        assert!(!has_tool(&discovery_tools, "subscribe_public_pod"));

        let curator_tools = self.origin.mcp(&harnesses.curator).list_tool_names(6).await;
        assert!(has_tool(&curator_tools, "route_candidate"));
        assert!(has_tool(&curator_tools, "review_candidate_placement"));
        assert!(!has_tool(&curator_tools, "submit_candidate"));
        assert!(!has_tool(&curator_tools, "approve_pending_proposal"));
    }

    async fn discover_six_posts(
        &self,
        pods: &OriginPods,
        harnesses: &OriginHarnesses,
    ) -> Discoveries {
        let discovery = self.origin.mcp(&harnesses.discovery);
        let task = discovery
            .call_tool(
                7,
                "create_immediate_discovery_task",
                json!({
                    "pod_id": pods.inbox,
                    "instructions": "Find exactly six relevant public posts.",
                    "idempotency_key": "two-node-six-posts"
                }),
            )
            .await;
        let task = task.discovery_task();
        let task_id = task.id.to_string();
        let package_version = task.target.pod().unwrap().1;
        discovery
            .call_tool(8, "claim_discovery_task", json!({"task_id": task_id}))
            .await;

        let mut candidate_ids = Vec::new();
        for index in 1..=6 {
            let media_references = if index == 1 {
                json!([{
                    "media_type": "image",
                    "url": "https://media.example/post-1/image.jpg"
                }])
            } else {
                json!([])
            };
            let submitted = discovery
                .call_tool(
                    10 + index,
                    "submit_candidate",
                    json!({
                        "source_url": format!("https://social.example/author/status/{index}"),
                        "source_metadata": {
                            "title": format!("Federated post {index}"),
                            "author": "@author"
                        },
                        "summary": format!("Acceptance post {index}"),
                        "content_type": "other",
                        "media_references": media_references,
                        "tags": ["federation", "acceptance"],
                        "provenance": {
                            "discovered_at": "2026-07-18T16:00:00Z",
                            "discovery_method": "agent_harness_browser"
                        },
                        "target": {
                            "kind": "pod_placements",
                            "placements": [{
                                "pod_id": pods.inbox,
                                "reason": "Keep discovery intake private before explicit routing.",
                                "confidence": 0.95
                            }],
                            "task_context": {
                                "task_id": task_id,
                                "package_version": package_version
                            }
                        },
                        "harness_idempotency_key": format!("two-node-harness-{index}"),
                        "client_idempotency_key": format!("two-node-client-{index}")
                    }),
                )
                .await;
            assert!(!submitted.is_error());
            candidate_ids.push(submitted.submitted_candidate().candidate.id.to_string());
        }
        discovery
            .call_tool(20, "complete_discovery_task", json!({"task_id": task_id}))
            .await;
        Discoveries {
            task_id,
            candidate_ids,
        }
    }

    fn verify_discovery_cannot_curate(
        &self,
        pods: &OriginPods,
        harnesses: &OriginHarnesses,
        discoveries: &Discoveries,
    ) {
        let direct_discovery_router =
            McpToolRouter::authenticated(self.origin.tools.clone(), harnesses.discovery.token())
                .expect("authenticate direct discovery router");
        let denied_route = direct_discovery_router
            .call(McpToolCall {
                tool: "route_candidate".into(),
                arguments: json!({
                    "candidate_id": discoveries.candidate_ids[0],
                    "pod_id": pods.public,
                    "reason": "This discovery-only Harness must not curate.",
                    "confidence": 1.0
                }),
            })
            .expect_err("core denies direct curation without Pod Curation");
        assert!(denied_route
            .to_string()
            .contains("harness grant lacks pod_curation"));
        let completed_task = direct_discovery_router
            .call(McpToolCall {
                tool: "discovery_task_status".into(),
                arguments: json!({"task_id": discoveries.task_id}),
            })
            .expect("Origin retains its completed Discovery Task");
        let completed_task: DiscoveryTask =
            serde_json::from_value(completed_task).expect("completed Discovery Task result");
        assert_eq!(completed_task.state, DiscoveryTaskState::Completed);
    }

    async fn curate_six_posts(
        &self,
        pods: &OriginPods,
        harnesses: &OriginHarnesses,
        discoveries: &Discoveries,
    ) {
        let curator = self.origin.mcp(&harnesses.curator);
        for (index, candidate_id) in discoveries.candidate_ids.iter().enumerate() {
            let routed = curator
                .call_tool(
                    30 + index as u64,
                    "route_candidate",
                    json!({
                        "candidate_id": candidate_id,
                        "pod_id": pods.public,
                        "reason": "The post matches the isolated public acceptance Pod.",
                        "confidence": 0.98
                    }),
                )
                .await;
            assert_eq!(routed.pod_placement().status, PodPlacementStatus::Pending);
            let accepted = curator
                .call_tool(
                    40 + index as u64,
                    "review_candidate_placement",
                    json!({
                        "candidate_id": candidate_id,
                        "pod_id": pods.public,
                        "decision": "accept",
                        "note": "Accepted through the scoped Origin curation adapter."
                    }),
                )
                .await;
            assert_eq!(
                accepted.pod_placement().status,
                PodPlacementStatus::Accepted
            );
        }
    }

    async fn verify_origin_and_add_private_sentinel(
        &self,
        pods: &OriginPods,
        harnesses: &OriginHarnesses,
    ) -> String {
        let content = self
            .origin
            .mcp(&harnesses.reader)
            .call_tool(50, "list_pod_content", json!({"pod_id": pods.public}))
            .await;
        assert_six_content_item_references_with_seed_image(&content.pod_content());

        let private_task = self
            .origin
            .mcp(&harnesses.discovery)
            .call_tool(
                51,
                "create_immediate_discovery_task",
                json!({
                    "pod_id": pods.inbox,
                    "instructions": "Private task that must remain only on the Origin.",
                    "idempotency_key": "two-node-private-sentinel"
                }),
            )
            .await;
        private_task.discovery_task().id.to_string()
    }

    fn scope_home_harnesses(&self) -> HomeHarnesses {
        let subscriber = self.home.harness(
            "Home subscription manager",
            vec![HarnessCapability::SubscriptionManagement],
            None,
        );
        let reader = self.home.harness(
            "Home accepted content reader",
            vec![HarnessCapability::FeedRead],
            None,
        );
        let private_state_reader = self.home.harness(
            "Home private state verifier",
            vec![
                HarnessCapability::CandidateSubmission,
                HarnessCapability::DiscoveryTasks,
            ],
            None,
        );
        HomeHarnesses {
            subscriber,
            reader,
            private_state_reader,
        }
    }

    async fn subscribe_home(&self, harnesses: &HomeHarnesses, origin_http: &EphemeralHttpServer) {
        let subscriber = self.home.mcp(&harnesses.subscriber);
        let tools = subscriber.list_tool_names(60).await;
        assert!(has_tool(&tools, "subscribe_public_pod"));
        assert!(has_tool(&tools, "synchronize_subscription"));
        assert!(!has_tool(&tools, "submit_candidate"));
        assert!(!has_tool(&tools, "list_pod_content"));
        assert!(!has_tool(&tools, "list_ready_discovery_tasks"));

        let subscribed = subscriber
            .call_tool(
                61,
                "subscribe_public_pod",
                json!({
                    "public_pod_url": format!(
                        "{}/federation/pods/federated-post-acceptance",
                        origin_http.base_url
                    )
                }),
            )
            .await;
        assert!(!subscribed.is_error());
        let subscribed = subscribed.synchronization_result();
        assert!(subscribed.imported_events >= 6);
        assert!(subscribed
            .subscription
            .last_event_hash
            .is_some_and(|cursor| !cursor.is_empty()));
    }

    async fn verify_public_federation(&self, harnesses: &HomeHarnesses) {
        let reader = self.home.mcp(&harnesses.reader);
        let home_pods = reader.call_tool(62, "list_pods", json!({})).await;
        let home_pods = home_pods.pods();
        assert!(!home_pods
            .iter()
            .any(|pod| pod.slug == "federation-acceptance-inbox"));
        let synchronized_pod_id = home_pods
            .iter()
            .find(|pod| pod.slug == "federated-post-acceptance")
            .map(|pod| pod.id)
            .expect("synchronized public Pod identity");
        let content = reader
            .call_tool(
                63,
                "list_pod_content",
                json!({"pod_id": synchronized_pod_id}),
            )
            .await;
        assert_six_content_item_references_with_seed_image(&content.pod_content());
    }

    async fn verify_private_state_stayed_on_origin(
        &self,
        origin_harnesses: &OriginHarnesses,
        home_harnesses: &HomeHarnesses,
        discoveries: &Discoveries,
        private_sentinel_task_id: &str,
    ) {
        let private_reader = self.home.mcp(&home_harnesses.private_state_reader);
        for (index, candidate_id) in discoveries.candidate_ids.iter().enumerate() {
            let missing_candidate = private_reader
                .call_tool(
                    70 + index as u64,
                    "inspect_candidate",
                    json!({"candidate_id": candidate_id}),
                )
                .await;
            assert!(missing_candidate.is_error());
            assert!(missing_candidate.error_text().contains("Candidate"));
            assert!(missing_candidate.error_text().contains("not found"));
        }

        let home_private_router = McpToolRouter::authenticated(
            self.home.tools.clone(),
            home_harnesses.private_state_reader.token(),
        )
        .expect("authenticate direct Home private-state router");
        let missing_completed_task = home_private_router
            .call(McpToolCall {
                tool: "discovery_task_status".into(),
                arguments: json!({"task_id": discoveries.task_id}),
            })
            .expect_err("Origin Discovery Task must not exist on Home Node");
        assert!(missing_completed_task
            .to_string()
            .contains("Discovery Task"));
        assert!(missing_completed_task.to_string().contains("not found"));

        let home_ready_tasks = private_reader
            .call_tool(80, "list_ready_discovery_tasks", json!({}))
            .await;
        assert!(home_ready_tasks.discovery_tasks().is_empty());
        let origin_ready_tasks = self
            .origin
            .mcp(&origin_harnesses.discovery)
            .call_tool(81, "list_ready_discovery_tasks", json!({}))
            .await;
        assert!(origin_ready_tasks
            .discovery_tasks()
            .iter()
            .any(|task| task.id.to_string() == private_sentinel_task_id));
    }
}

fn has_tool(tools: &[String], expected: &str) -> bool {
    tools.iter().any(|tool| tool == expected)
}

fn assert_six_content_item_references_with_seed_image(content: &[PodContentItem]) {
    assert_eq!(content.len(), 6);
    let mut urls = content
        .iter()
        .map(|placement| placement.content_item.canonical_url())
        .collect::<Vec<_>>();
    urls.sort_unstable();
    assert_eq!(
        urls,
        (1..=6)
            .map(|index| format!("https://social.example/author/status/{index}"))
            .collect::<Vec<_>>()
    );
    let seed = content
        .iter()
        .find(|placement| {
            placement.content_item.canonical_url() == "https://social.example/author/status/1"
        })
        .expect("seed post Content Reference");
    assert_eq!(
        seed.content_item.media_references(),
        &[MediaReference::new(
            MediaReferenceType::Image,
            "https://media.example/post-1/image.jpg",
        )
        .expect("valid expected media reference")]
    );
}
