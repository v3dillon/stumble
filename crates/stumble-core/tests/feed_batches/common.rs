use chrono::{TimeZone, Utc};
use stumble_core::*;

pub(crate) struct TestDataDir(pub(crate) std::path::PathBuf);

impl TestDataDir {
    pub(crate) fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-feed-batches-{}", uuid::Uuid::now_v7()));
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
    capabilities: Vec<HarnessCapability>,
    pod_ids: Option<Vec<PodId>>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind: AgentHarnessKind::Interactive,
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

pub(crate) fn accepted_item(
    tools: &AgentTools,
    slug: &str,
    ordinal: usize,
) -> (Pod, ContentItemId) {
    let curator = harness(
        tools,
        &format!("curator-{ordinal}"),
        vec![
            HarnessCapability::PodCuration,
            HarnessCapability::SubscriptionManagement,
        ],
        None,
    );
    let pod = tools
        .create_pod(
            &curator,
            CreatePodRequest {
                name: slug.into(),
                slug: slug.into(),
                description: "Feed acceptance Pod".into(),
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
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Strong subject match".into(),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://source{ordinal}.example/report"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Report {ordinal}")),
                        author: Some("Researcher".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted excerpt".into()),
                    summary: Some(format!("A useful report about topic-{ordinal}")),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec![format!("topic-{ordinal}")],
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
                        discovery_method: "interactive_search".into(),
                        referrer_url: Some("https://search.example".into()),
                    },
                    harness_idempotency_key: format!("feed-harness-{ordinal}"),
                    client_idempotency_key: format!("feed-client-{ordinal}"),
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

pub(crate) fn accepted_item_in_pod(tools: &AgentTools, pod: &Pod, ordinal: usize) -> ContentItemId {
    let curator = harness(
        tools,
        &format!("pod-curator-{ordinal}"),
        vec![HarnessCapability::PodCuration],
        Some(vec![pod.id]),
    );
    let submitter = harness(
        tools,
        &format!("pod-submitter-{ordinal}"),
        vec![HarnessCapability::CandidateSubmission],
        Some(vec![pod.id]),
    );
    let submitted = tools
        .submit_candidate(
            &submitter,
            CandidateSubmissionRequest {
                target: CandidateSubmissionRequestTarget::PodPlacements {
                    placements: vec![ProposedCandidatePlacement {
                        pod_id: pod.id,
                        reason: "Strong subject match".into(),
                        confidence: CandidateConfidence::new(0.9).unwrap(),
                    }],
                    task_context: None,
                },
                evidence: CandidateSubmissionEvidence {
                    source_url: format!("https://pod-source{ordinal}.example/report"),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Pod report {ordinal}")),
                        author: Some("Researcher".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("Permitted excerpt".into()),
                    summary: Some(format!("A useful report about pod-topic-{ordinal}")),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags: vec![format!("pod-topic-{ordinal}")],
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 10, 0, 0).unwrap(),
                        discovery_method: "interactive_search".into(),
                        referrer_url: Some("https://search.example".into()),
                    },
                    harness_idempotency_key: format!("pod-harness-{ordinal}"),
                    client_idempotency_key: format!("pod-client-{ordinal}"),
                },
            },
        )
        .unwrap();
    tools
        .curate_candidate(&curator, submitted.candidate.id, Utc::now())
        .unwrap();
    tools
        .review_candidate_placement(
            &curator,
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

pub(crate) fn make_unsubscribed_public(tools: &AgentTools, pod: &Pod) {
    let shared_store = tools.store();
    let mut store = shared_store.write().unwrap();
    store.pods.get_mut(&pod.id).unwrap().visibility = Visibility::Public;
    store
        .subscriptions
        .retain(|_, subscription| subscription.local_pod_id != pod.id);
}
