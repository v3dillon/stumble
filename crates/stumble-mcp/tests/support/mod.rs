#![allow(dead_code)]

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    response::Response,
    Router,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use stumble_api::router_with_base_url;
use stumble_core::{
    seed_store, AgentHarnessId, AgentHarnessKind, AgentTools, CreatePodOutcome, DiscoveryTask,
    HarnessCapability, PendingProposal, Pod, PodContentItem, PodId, PodPlacement,
    RegisterAgentHarnessRequest, RegisterAgentHarnessResponse, SubmittedCandidate,
    SynchronizationResult,
};
use stumble_mcp::streamable_http_router;
use tower::ServiceExt;

pub struct PersistentNode {
    pub tools: AgentTools,
    _data_dir: TestDataDir,
}

impl PersistentNode {
    pub fn open(label: &str) -> Self {
        let data_dir = TestDataDir::new(label);
        let tools = AgentTools::open_home_node(data_dir.path(), seed_store)
            .expect("open persistent test node");
        Self {
            tools,
            _data_dir: data_dir,
        }
    }

    pub fn harness(
        &self,
        label: &str,
        capabilities: Vec<HarnessCapability>,
        pod_ids: Option<Vec<PodId>>,
    ) -> ScopedHarness {
        ScopedHarness::register(&self.tools, label, capabilities, pod_ids)
    }

    pub fn mcp(&self, harness: &ScopedHarness) -> McpClient {
        McpClient::new(streamable_http_router(self.tools.clone()), harness.token())
    }
}

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-mcp-{label}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).expect("create persistent test node directory");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct ScopedHarness(RegisterAgentHarnessResponse);

impl ScopedHarness {
    pub fn register(
        tools: &AgentTools,
        label: &str,
        capabilities: Vec<HarnessCapability>,
        pod_ids: Option<Vec<PodId>>,
    ) -> Self {
        let response = tools
            .register_agent_harness(
                &tools.default_auth_context().expect("node owner context"),
                RegisterAgentHarnessRequest {
                    label: label.into(),
                    kind: AgentHarnessKind::Interactive,
                    capabilities,
                    pod_ids,
                },
            )
            .expect("register capability-scoped Agent Harness");
        Self(response)
    }

    pub fn token(&self) -> &str {
        self.0.token.expose()
    }

    pub fn id(&self) -> AgentHarnessId {
        self.0.harness.id
    }
}

pub struct EphemeralHttpServer {
    pub base_url: String,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl EphemeralHttpServer {
    pub async fn start(app: Router) -> Self {
        Self::start_with(|_| app).await
    }

    pub async fn start_origin(tools: AgentTools) -> Self {
        Self::start_with(|base_url| router_with_base_url(tools, base_url)).await
    }

    async fn start_with(build_app: impl FnOnce(&str) -> Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral HTTP listener");
        let base_url = format!("http://{}", listener.local_addr().expect("HTTP address"));
        let app = build_app(&base_url);
        let task = tokio::spawn(async move { axum::serve(listener, app).await });
        Self { base_url, task }
    }
}

impl Drop for EphemeralHttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
pub struct McpClient {
    app: Router,
    token: String,
}

impl McpClient {
    pub fn new(app: Router, token: &str) -> Self {
        Self {
            app,
            token: token.to_owned(),
        }
    }

    async fn request(&self, id: u64, method: &str, params: Value) -> Value {
        let response = self
            .app
            .clone()
            .oneshot(mcp_request(
                &self.token,
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params
                }),
            ))
            .await
            .expect("MCP response");
        assert_eq!(response.status(), StatusCode::OK);
        response_json(response).await
    }

    pub async fn call_tool(&self, id: u64, name: &str, arguments: Value) -> McpToolResult {
        McpToolResult(
            self.request(
                id,
                "tools/call",
                json!({"name": name, "arguments": arguments}),
            )
            .await,
        )
    }

    pub async fn list_tool_names(&self, id: u64) -> Vec<String> {
        self.list_tools(id).await.names()
    }

    pub async fn list_tools(&self, id: u64) -> McpToolCatalog {
        McpToolCatalog(
            self.request(id, "tools/list", json!({})).await["result"]["tools"]
                .as_array()
                .expect("capability-filtered MCP tool descriptors")
                .clone(),
        )
    }
}

pub struct McpToolCatalog(Vec<Value>);

impl McpToolCatalog {
    pub fn names(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|tool| tool["name"].as_str().expect("MCP tool name").to_owned())
            .collect()
    }

    pub fn descriptor(&self, name: &str) -> &Value {
        self.0
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("MCP tool descriptor for {name}"))
    }
}

pub struct McpToolResult(Value);

impl McpToolResult {
    pub fn from_json(value: Value) -> Self {
        Self(value)
    }

    pub fn create_pod_outcome(&self) -> CreatePodOutcome {
        self.decode("Pod creation result")
    }

    pub fn pending_proposal(&self) -> PendingProposal {
        self.decode("Pending Proposal result")
    }

    pub fn submitted_candidate(&self) -> SubmittedCandidate {
        self.decode("Candidate submission result")
    }

    pub fn discovery_task(&self) -> DiscoveryTask {
        self.decode("Discovery Task result")
    }

    pub fn discovery_tasks(&self) -> Vec<DiscoveryTask> {
        self.decode("Discovery Task list result")
    }

    pub fn pod_placement(&self) -> PodPlacement {
        self.decode("Pod Placement result")
    }

    pub fn synchronization_result(&self) -> SynchronizationResult {
        self.decode("Subscription synchronization result")
    }

    pub fn pods(&self) -> Vec<Pod> {
        self.decode("Pod list result")
    }

    pub fn pod_content(&self) -> Vec<PodContentItem> {
        let items = self
            .structured_value()
            .as_array()
            .unwrap_or_else(|| panic!("Pod content result must be an array"));
        assert!(items
            .iter()
            .all(|item| { item.get("candidate").is_none() && item.get("submissions").is_none() }));
        self.decode("Pod content result")
    }

    pub fn structured_content(&self) -> &Value {
        &self.0["result"]["structuredContent"]
    }

    pub fn is_error(&self) -> bool {
        self.0["result"]["isError"]
            .as_bool()
            .expect("MCP tool error marker")
    }

    pub fn error_text(&self) -> &str {
        self.content_text()
    }

    pub fn content_text(&self) -> &str {
        self.0["result"]["content"][0]["text"]
            .as_str()
            .expect("MCP tool error text")
    }

    fn decode<T: DeserializeOwned>(&self, label: &str) -> T {
        serde_json::from_value(self.structured_value().clone())
            .unwrap_or_else(|error| panic!("decode {label}: {error}"))
    }

    fn structured_value(&self) -> &Value {
        &self.0["result"]["structuredContent"]["value"]
    }
}

pub fn mcp_request(token: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-06-18")
        .body(Body::from(body.to_string()))
        .expect("valid MCP request")
}

pub async fn response_json(response: Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read MCP response body");
    serde_json::from_slice(&bytes).expect("MCP JSON response")
}
