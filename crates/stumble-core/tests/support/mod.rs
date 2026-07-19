use chrono::{Duration, TimeZone, Utc};
use stumble_core::*;

pub fn media_reference(media_type: MediaReferenceType, url: &str) -> MediaReference {
    MediaReference::new(media_type, url).unwrap()
}

pub struct TestDataDir(pub std::path::PathBuf);

impl TestDataDir {
    pub fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-subscriptions-{label}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        // Test cleanup is best effort; a failed assertion should remain the primary failure.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn register_authenticated_harness(
    tools: &AgentTools,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
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
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

pub fn create_public_pod(tools: &AgentTools, slug: &str) -> Pod {
    let proposer = register_authenticated_harness(
        tools,
        "public Pod proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = register_authenticated_harness(
        tools,
        "public Pod approver",
        vec![HarnessCapability::Approval],
    );
    let private_pod = tools
        .create_pod(
            &proposer,
            CreatePodRequest {
                name: "Remote systems".into(),
                slug: slug.into(),
                description: "Accepted references from the Origin Node".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let now = Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod {
                pod_id: private_pod.id,
            },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

pub fn submit_and_accept_placement(tools: &AgentTools, pod: &Pod) {
    let submitter = register_authenticated_harness(
        tools,
        "origin submitter",
        vec![HarnessCapability::CandidateSubmission],
    );
    let curator = register_authenticated_harness(
        tools,
        "origin curator",
        vec![HarnessCapability::PodCuration],
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let candidate = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                evidence: CandidateSubmissionEvidence {
                    source_url: "https://reference.example/remote-report?utm_source=origin".into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some("Remote report".into()),
                        author: Some("Reference author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("A permitted excerpt".into()),
                    summary: Some("An accepted remote Content Reference".into()),
                    content_type: CandidateContentType::Article,
                    media_references: vec![media_reference(
                        MediaReferenceType::Image,
                        "https://media.reference.example/remote-report/diagram.jpg",
                    )],
                    tags: vec!["systems".into()],
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                        discovery_method: "browser_search".into(),
                        referrer_url: Some("https://search.example/results".into()),
                    },
                    proposed_placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Directly concerns distributed systems".into(),
                        confidence: CandidateConfidence::new(0.95).unwrap(),
                    }],
                    task_context: None,
                    harness_idempotency_key: "origin-worker-1".into(),
                    client_idempotency_key: "origin-client-1".into(),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, candidate.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &curator,
            candidate.candidate.id,
            pod.id,
            PlacementReviewDecision::Accept,
            None,
            Utc::now(),
        )
        .unwrap();
}
