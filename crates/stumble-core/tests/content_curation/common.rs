use chrono::{TimeZone, Utc};
use stumble_core::*;

pub(crate) fn media(media_type: MediaReferenceType, url: &str) -> MediaReference {
    MediaReference::new(media_type, url).unwrap()
}

pub(crate) struct TestDataDir(pub(crate) std::path::PathBuf);

impl TestDataDir {
    pub(crate) fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-content-curation-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn harness(
    tools: &AgentTools,
    label: &str,
    kind: AgentHarnessKind,
    capabilities: Vec<HarnessCapability>,
    pod_ids: Option<Vec<PodId>>,
) -> AuthContext {
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
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

pub(crate) fn private_pod(tools: &AgentTools, slug: &str) -> Pod {
    let curator = harness(
        tools,
        "pod owner",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        None,
    );
    tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: slug.into(),
                slug: slug.into(),
                description: "Curation acceptance Pod".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap()
}

pub(crate) fn public_pod(tools: &AgentTools, slug: &str) -> Pod {
    let proposer = harness(
        tools,
        "public Pod proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
        None,
    );
    let approver = harness(
        tools,
        "public Pod approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
        None,
    );
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::CreatePublicPod {
                request: CreatePodRequest {
                    name: slug.into(),
                    slug: slug.into(),
                    description: "Public curation acceptance Pod".into(),
                    visibility: Visibility::Public,
                },
            },
            now,
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

pub(crate) fn candidate_request(pod_id: PodId, confidence: f32) -> CandidateSubmissionRequest {
    CandidateSubmissionRequest {
        target: CandidateSubmissionRequestTarget::PodPlacements {
            placements: vec![ProposedCandidatePlacement {
                pod_id,
                reason: "Strong topical match".into(),
                confidence: CandidateConfidence::new(confidence).unwrap(),
            }],
            task_context: None,
        },
        evidence: CandidateSubmissionEvidence {
            source_url: "https://example.com/curation?utm_source=test".into(),
            source_metadata: CandidateSourceMetadata {
                title: Some("Curation report".into()),
                author: Some("Example Engineering".into()),
                published_at: None,
            },
            permitted_excerpt: Some("Permitted evidence".into()),
            summary: Some("A report worth curating".into()),
            content_type: CandidateContentType::Article,
            media_references: vec![media(
                MediaReferenceType::Image,
                "https://media.example.com/curation/preview.jpg",
            )],
            tags: vec!["curation".into()],
            provenance: CandidateProvenance {
                discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
                discovery_method: "interactive_search".into(),
                referrer_url: None,
            },
            harness_idempotency_key: format!("worker-{pod_id}"),
            client_idempotency_key: format!("client-{pod_id}"),
        },
    }
}

pub(crate) fn rationale(value: &str) -> CurationRationale {
    CurationRationale::new(value).unwrap()
}
