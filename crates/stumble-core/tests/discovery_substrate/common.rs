use chrono::{Duration, TimeZone, Utc};
use stumble_core::*;

pub(crate) struct TestDataDir(pub(crate) std::path::PathBuf);

impl TestDataDir {
    pub(crate) fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-discovery-substrate-{label}-{}",
            uuid::Uuid::now_v7()
        ));
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

pub(crate) fn create_public_pod(tools: &AgentTools, slug: &str, description: &str) -> Pod {
    let proposer = harness(
        tools,
        "public Pod proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness(
        tools,
        "public Pod approver",
        vec![HarnessCapability::Approval],
    );
    let pod = tools
        .create_pod(
            &proposer,
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
            &proposer,
            SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

pub(crate) fn accept_item(
    tools: &AgentTools,
    pod: &Pod,
    suffix: &str,
    source_url: &str,
    tags: Vec<String>,
) {
    let submitter = harness(
        tools,
        &format!("{suffix} submitter"),
        vec![HarnessCapability::CandidateSubmission],
    );
    let curator = harness(
        tools,
        &format!("{suffix} curator"),
        vec![HarnessCapability::PodCuration],
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
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
                    source_url: source_url.into(),
                    source_metadata: CandidateSourceMetadata {
                        title: Some(format!("Reference {suffix}")),
                        author: Some("Careful author".into()),
                        published_at: None,
                    },
                    permitted_excerpt: Some("A permitted sample excerpt".into()),
                    summary: Some("A useful public Content Reference".into()),
                    content_type: CandidateContentType::Article,
                    media_references: Vec::new(),
                    tags,
                    provenance: CandidateProvenance {
                        discovered_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
                        discovery_method: "browser_search".into(),
                        referrer_url: None,
                    },
                    harness_idempotency_key: format!("worker-{suffix}"),
                    client_idempotency_key: format!("client-{suffix}"),
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

pub(crate) fn trust_peer(tools: &AgentTools, peer: &NodeInfo, base_url: &str) -> TrustedPeer {
    let proposer = harness(
        tools,
        "Trust Policy proposer",
        vec![HarnessCapability::Administration],
    );
    let approver = harness(
        tools,
        "Trust Policy approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let proposal = tools
        .request_add_trusted_peer(
            &proposer,
            peer.display_name.clone(),
            base_url.into(),
            peer.public_key.clone(),
            now,
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools
        .trusted_peers(&proposer)
        .unwrap()
        .into_iter()
        .find(|trusted| trusted.public_key == peer.public_key)
        .unwrap()
}

pub(crate) fn pod_owner(tools: &AgentTools, pod_id: PodId) -> AuthContext {
    let store = tools.store();
    let store = store.read().unwrap();
    let owner_user = store
        .pod_roles
        .iter()
        .find(|assignment| assignment.pod_id == pod_id && assignment.role == PodRole::Owner)
        .map(|assignment| assignment.user_id)
        .expect("public Pod has an Owner");
    let mut ctx = tools.default_auth_context().unwrap();
    ctx.user_id = Some(owner_user);
    // Owner operations authorize via User pod role rather than harness capability alone.
    ctx
}

pub(crate) fn approve_trust_policy_change(tools: &AgentTools, change: TrustPolicyChange) {
    let proposer = harness(
        tools,
        "local Trust Policy editor",
        vec![HarnessCapability::Administration],
    );
    let approver = harness(
        tools,
        "local Trust Policy approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let proposal = tools
        .request_trust_policy_change(&proposer, change, now)
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
}
