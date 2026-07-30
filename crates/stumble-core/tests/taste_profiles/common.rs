use chrono::{TimeZone, Utc};
use stumble_core::*;

pub(crate) struct TestDataDir(pub(crate) std::path::PathBuf);

impl TestDataDir {
    pub(crate) fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-taste-profile-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn feedback_harness(tools: &AgentTools) -> (AuthContext, HarnessToken) {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "Taste Profile user".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Feedback, HarnessCapability::FeedRead],
                pod_ids: None,
            },
        )
        .unwrap();
    let context = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    (context, issued.token)
}

pub(crate) fn harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
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

pub(crate) fn unattended_harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Unattended,
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

pub(crate) fn accepted_item(
    tools: &AgentTools,
    slug: &str,
    ordinal: usize,
    source: &str,
    tags: Vec<String>,
) -> (Pod, ContentItemId) {
    let curator = harness(
        tools,
        &format!("curator-{ordinal}"),
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::SubscriptionManagement,
        ],
    );
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: slug.into(),
                slug: slug.into(),
                description: "Taste learning Pod".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    tools.join_pod(&curator, &pod.slug).unwrap();
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let submitter = harness(
        tools,
        &format!("submitter-{ordinal}"),
        vec![HarnessCapability::CandidateSubmission],
    );
    let submitted = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Strong match".into(),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://{source}/{ordinal}"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Item {ordinal}")),
                        author: None,
                        published_at: None,
                    },
                    permitted_excerpt: None,
                    summary: Some(format!("A report about {}", tags.join(" "))),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags,
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
                        discovery_method: "interactive_search".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: format!("taste-harness-{ordinal}"),
                    client_idempotency_key: format!("taste-client-{ordinal}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    let placement = tools
        .review_candidate_placement(
            &curator,
            submitted.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
    (pod, placement.content_item_id.unwrap())
}
