//! Shared helpers and fixtures for the sponsored deployment acceptance tests.

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use chrono::{Duration, Utc};
use serde_json::Value;
use stumble_core::*;
use tower::ServiceExt;

// ─── Temp dirs & ephemeral servers ───────────────────────────────────────────

pub(crate) struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    pub(crate) fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-sponsored-accept-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).expect("create temp data dir");
        Self(path)
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct EphemeralHttpServer {
    pub(crate) base_url: String,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl EphemeralHttpServer {
    pub(crate) async fn start_with(build: impl FnOnce(&str) -> Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        let base_url = format!("http://{}", listener.local_addr().expect("addr"));
        let app = build(&base_url);
        let task = tokio::spawn(async move { axum::serve(listener, app).await });
        Self { base_url, task }
    }
}

impl Drop for EphemeralHttpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// ─── Auth & Pod helpers ──────────────────────────────────────────────────────

pub(crate) struct AuthLike {
    pub(crate) ctx: AuthContext,
}

pub(crate) fn register_harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> AuthLike {
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
    AuthLike {
        ctx: tools
            .authenticate_token(issued.token.expose())
            .unwrap()
            .unwrap(),
    }
}

pub(crate) fn local_node(tools: &AgentTools) -> NodeIdentity {
    tools.store().read().unwrap().default_node().unwrap()
}

pub(crate) fn create_public_pod(tools: &AgentTools, slug: &str, description: &str) -> Pod {
    let proposer = register_harness(
        tools,
        &format!("{slug} curator"),
        vec![HarnessCapability::PodCuration],
    );
    let approver = register_harness(
        tools,
        &format!("{slug} approver"),
        vec![HarnessCapability::Approval],
    );
    let pod = tools
        .create_pod(
            &proposer.ctx,
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
            &proposer.ctx,
            SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver.ctx, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

pub(crate) fn accept_public_item(tools: &AgentTools, pod: &Pod, source_url: &str, title: &str) {
    let submitter = register_harness(
        tools,
        &format!("{} submitter", pod.slug),
        vec![HarnessCapability::CandidateSubmission],
    );
    let curator = register_harness(
        tools,
        &format!("{} item curator", pod.slug),
        vec![HarnessCapability::PodCuration],
    );
    let now = Utc::now();
    tools
        .set_pod_curation_policy(&curator.ctx, pod.id, CurationPolicy::Manual, now)
        .unwrap();
    let submitted = tools
        .submit_candidate(
            &submitter.ctx,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Matches the public Pod subject".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: source_url.into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(title.into()),
                        author: None,
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted excerpt for trial samples".into()),
                    summary: Some(title.into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec!["systems".into(), "distributed".into()],
                    provenance: CandidateProvenance {
                        discovered_at: now,
                        discovery_method: "origin_curation".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: format!("{}-worker", pod.slug),
                    client_idempotency_key: format!("{}-client", pod.slug),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator.ctx, submitted.candidate.id, now)
        .unwrap();
    tools
        .review_candidate_placement(
            &curator.ctx,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            now,
        )
        .unwrap();
}

pub(crate) fn approve_trust_policy_change(tools: &AgentTools, change: TrustPolicyChange) {
    let proposer = register_harness(
        tools,
        "trust proposer",
        vec![HarnessCapability::Administration],
    );
    let approver = register_harness(tools, "trust approver", vec![HarnessCapability::Approval]);
    let now = Utc::now();
    let proposal = tools
        .request_trust_policy_change(&proposer.ctx, change, now)
        .unwrap();
    tools
        .approve_pending_proposal(&approver.ctx, proposal.id, now)
        .unwrap();
}

pub(crate) fn clear_default_bootstrap(tools: &AgentTools, admin: &AuthContext) {
    let endpoints = tools.list_bootstrap_endpoints(admin).unwrap();
    for endpoint in endpoints {
        tools.remove_bootstrap_endpoint(admin, endpoint.id).unwrap();
    }
}

// ─── HTTP helpers ────────────────────────────────────────────────────────────

pub(crate) async fn http_json(
    app: &Router,
    method: &str,
    path: &str,
    authorization: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    // body is owned so callers can pass `Some(json!(...))` without reborrow issues.
    let mut builder = match method {
        "GET" => Request::get(path),
        "POST" => Request::post(path),
        "PATCH" => Request::patch(path),
        "DELETE" => Request::delete(path),
        other => panic!("unsupported method {other}"),
    };
    if let Some(auth) = authorization {
        builder = builder.header("authorization", auth);
    }
    let request = if let Some(body) = body {
        builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

pub(crate) async fn client_json(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    authorization: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = client.request(method, url);
    if let Some(auth) = authorization {
        req = req.header("authorization", auth);
    }
    if let Some(body) = body {
        req = req.json(&body);
    }
    let response = req.send().await.unwrap();
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap();
    let value = response.json::<Value>().await.unwrap_or(Value::Null);
    (status, value)
}
