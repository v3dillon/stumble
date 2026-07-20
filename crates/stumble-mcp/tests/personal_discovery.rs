use serde_json::json;
use stumble_core::{
    seed_store, AgentHarnessKind, AgentTools, HarnessCapability, RegisterAgentHarnessRequest,
};
use stumble_mcp::{McpToolCall, McpToolRouter};

mod support;
use support::McpClient;

fn router(
    tools: &AgentTools,
    label: &str,
    kind: AgentHarnessKind,
    capability: HarnessCapability,
) -> McpToolRouter {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind,
                capabilities: vec![capability],
                pod_ids: None,
            },
        )
        .unwrap();
    McpToolRouter::authenticated(tools.clone(), issued.token.expose()).unwrap()
}

#[test]
fn mcp_manager_and_worker_share_only_the_pinned_personal_plan() {
    let tools = AgentTools::new(seed_store());
    let manager = router(
        &tools,
        "manager",
        AgentHarnessKind::Interactive,
        HarnessCapability::PersonalDiscoveryManagement,
    );
    let worker = router(
        &tools,
        "worker",
        AgentHarnessKind::Unattended,
        HarnessCapability::PersonalDiscoveryExecution,
    );

    let readiness = manager
        .call(McpToolCall {
            tool: "personal_discovery_readiness".into(),
            arguments: json!({}),
        })
        .unwrap();
    assert_eq!(readiness["ready"], true);
    let created = manager
        .call(McpToolCall {
            tool: "request_personal_discovery".into(),
            arguments: json!({"idempotency_key": "mcp-personal"}),
        })
        .unwrap();
    let task_id = created["task"]["id"].as_str().unwrap();
    let plan_id = created["plan"]["id"].as_str().unwrap();

    let ready = worker
        .call(McpToolCall {
            tool: "list_ready_discovery_tasks".into(),
            arguments: json!({}),
        })
        .unwrap();
    assert_eq!(ready.as_array().unwrap().len(), 1);
    worker
        .call(McpToolCall {
            tool: "claim_discovery_task".into(),
            arguments: json!({"task_id": task_id, "lease_seconds": 300}),
        })
        .unwrap();
    let plan = worker
        .call(McpToolCall {
            tool: "get_discovery_plan".into(),
            arguments: json!({"discovery_plan_id": plan_id}),
        })
        .unwrap();
    assert_eq!(plan["id"], plan_id);
    assert!(worker
        .call(McpToolCall {
            tool: "get_taste_profile".into(),
            arguments: json!({}),
        })
        .is_err());
}

#[tokio::test]
async fn mcp_catalog_projects_the_canonical_personal_management_policy() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let pod_id = uuid::Uuid::now_v7();
    tools.store().write().unwrap().pods.insert(
        pod_id,
        stumble_core::Pod {
            id: pod_id,
            tenant_id: owner.tenant_id,
            name: "Scoped".into(),
            slug: "scoped".into(),
            description: "Authorization fixture".into(),
            visibility: stumble_core::Visibility::Private,
            created_by: owner.user_id,
            created_at: chrono::Utc::now(),
            origin_node_id: None,
        },
    );
    let register = |label: &str,
                    kind: AgentHarnessKind,
                    capability: HarnessCapability,
                    pod_ids: Option<Vec<stumble_core::PodId>>| {
        tools
            .register_agent_harness(
                &owner,
                RegisterAgentHarnessRequest {
                    label: label.into(),
                    kind,
                    capabilities: vec![capability],
                    pod_ids,
                },
            )
            .unwrap()
    };
    let interactive_manager = register(
        "interactive manager",
        AgentHarnessKind::Interactive,
        HarnessCapability::PersonalDiscoveryManagement,
        None,
    );
    let worker = register(
        "worker",
        AgentHarnessKind::Unattended,
        HarnessCapability::PersonalDiscoveryExecution,
        None,
    );
    let unattended_manager = register(
        "unattended manager",
        AgentHarnessKind::Unattended,
        HarnessCapability::PersonalDiscoveryManagement,
        None,
    );
    let pod_scoped_manager = register(
        "pod scoped manager",
        AgentHarnessKind::Interactive,
        HarnessCapability::PersonalDiscoveryManagement,
        Some(vec![pod_id]),
    );
    let pod_scoped_worker = register(
        "pod scoped worker",
        AgentHarnessKind::Unattended,
        HarnessCapability::PersonalDiscoveryExecution,
        Some(vec![pod_id]),
    );

    for (issued, management, plan_access, execution) in [
        (interactive_manager, true, true, false),
        (worker, false, true, true),
        (unattended_manager, false, false, false),
        (pod_scoped_manager, false, false, false),
        (pod_scoped_worker, false, false, false),
    ] {
        let client = McpClient::new(
            stumble_mcp::streamable_http_router(tools.clone()),
            issued.token.expose(),
        );
        let names = client.list_tool_names(1).await;
        assert_eq!(
            names.contains(&"personal_discovery_readiness".to_string()),
            management
        );
        assert_eq!(
            names.contains(&"request_personal_discovery".to_string()),
            management
        );
        assert_eq!(
            names.contains(&"get_discovery_plan".to_string()),
            plan_access
        );
        assert_eq!(
            names.contains(&"list_ready_discovery_tasks".to_string()),
            execution
        );
    }
}
