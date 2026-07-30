use chrono::{TimeZone, Utc};
use stumble_core::*;

pub(crate) fn harness(
    tools: &AgentTools,
    label: &str,
    kind: AgentHarnessKind,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
    let owner = tools.default_auth_context().unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: label.into(),
                kind,
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

pub(crate) struct TestDataDir(pub(crate) std::path::PathBuf);

impl TestDataDir {
    pub(crate) fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-personal-discovery-{label}-{}",
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

pub(crate) fn admin_harness(
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
    let proposer = admin_harness(
        tools,
        &format!("{slug} proposer"),
        vec![HarnessCapability::PodCuration],
    );
    let approver = admin_harness(
        tools,
        &format!("{slug} approver"),
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
            now + chrono::Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, None).unwrap()
}

pub(crate) fn accept_public_item(
    tools: &AgentTools,
    pod: &Pod,
    suffix: &str,
    source_url: &str,
    tags: Vec<String>,
) {
    let submitter = admin_harness(
        tools,
        &format!("{suffix} submitter"),
        vec![HarnessCapability::CandidateSubmission],
    );
    let curator = admin_harness(
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

pub(crate) fn approve_trust_policy_change(tools: &AgentTools, change: TrustPolicyChange) {
    let proposer = admin_harness(
        tools,
        "trust proposer",
        vec![HarnessCapability::Administration],
    );
    let approver = admin_harness(tools, "trust approver", vec![HarnessCapability::Approval]);
    let now = Utc::now();
    let proposal = tools
        .request_trust_policy_change(&proposer, change, now)
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
}

pub(crate) fn personal_manager(tools: &AgentTools) -> AuthContext {
    harness(
        tools,
        "network lead manager",
        AgentHarnessKind::Interactive,
        vec![
            HarnessCapability::PersonalDiscoveryManagement,
            HarnessCapability::Feedback,
            HarnessCapability::FeedRead,
        ],
    )
}

pub(crate) fn set_interest(tools: &AgentTools, manager: &AuthContext, topic: &str) {
    let mut taste = UpdateTasteProfileRequest::default();
    taste.interests = Some(vec![topic.into()]);
    tools.update_taste_profile(manager, taste).unwrap();
}

pub(crate) fn import_verified_network_metadata(
    home: &AgentTools,
) -> (PodAnnouncement, PodExploreSamples) {
    let origin_dir = TestDataDir::new("network-lead-origin");
    let index_dir = TestDataDir::new("network-lead-index");
    let origin = AgentTools::open_home_node(&origin_dir.0, seed_store).unwrap();
    let index = AgentTools::open_home_node(&index_dir.0, seed_store)
        .unwrap()
        .with_index_capability(true);
    let pod = create_public_pod(
        &origin,
        "rust-systems",
        "Rust ownership and distributed systems",
    );
    accept_public_item(
        &origin,
        &pod,
        "network-allowed",
        "https://allowed.example/systems-research",
        vec!["systems".into(), "rust".into()],
    );
    accept_public_item(
        &origin,
        &pod,
        "network-blocked-source",
        "https://blocked.example/noise",
        vec!["systems".into()],
    );
    let announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &pod.slug,
            "https://origin.example/federation/pods/rust-systems",
        )
        .unwrap();
    let samples = origin
        .pod_explore_samples(&origin.default_auth_context().unwrap(), &announcement, 10)
        .unwrap();
    let endorser = create_public_pod(
        &origin,
        "systems-curators",
        "Systems curators recommending careful research",
    );
    let endorser_announcement = origin
        .pod_announcement(
            &origin.default_auth_context().unwrap(),
            &endorser.slug,
            "https://origin.example/federation/pods/systems-curators",
        )
        .unwrap();
    let curator = admin_harness(
        &origin,
        "network endorsement curator",
        vec![HarnessCapability::PodCuration],
    );
    let endorsement = origin
        .endorse_public_pod(
            &curator,
            &endorser_announcement,
            &announcement,
            "Careful systems research neighborhood".into(),
        )
        .unwrap();

    index.index_pod_announcement(announcement.clone()).unwrap();
    index
        .index_pod_announcement(endorser_announcement.clone())
        .unwrap();
    index.index_pod_endorsement(endorsement.clone()).unwrap();
    let search = index.search_pod_announcements("systems", 10).unwrap();

    approve_trust_policy_change(
        home,
        TrustPolicyChange::AddIndexNode {
            label: "network index".into(),
            base_url: "https://network-index.example".into(),
        },
    );
    approve_trust_policy_change(
        home,
        TrustPolicyChange::BlockSource {
            source: "blocked.example".into(),
        },
    );
    let reader = admin_harness(
        home,
        "network import reader",
        vec![HarnessCapability::FeedRead],
    );
    home.accept_index_search_results(&reader, "https://network-index.example", search)
        .unwrap();
    // Peer/direct retention of the endorser so endorsement binding remains current.
    home.index_pod_announcement(endorser_announcement).unwrap();
    home.accept_pod_explore_samples(&reader, samples.clone())
        .unwrap();
    home.index_pod_endorsement(endorsement).unwrap();
    (announcement, samples)
}

pub(crate) fn personal_worker(tools: &AgentTools) -> AuthContext {
    harness(
        tools,
        "personal worker",
        AgentHarnessKind::Unattended,
        vec![HarnessCapability::PersonalDiscoveryExecution],
    )
}

pub(crate) fn claim_personal_run(
    tools: &AgentTools,
    manager: &AuthContext,
    worker: &AuthContext,
    result_count: Option<u16>,
    key: &str,
) -> RequestedPersonalDiscovery {
    let now = Utc::now();
    let created = tools
        .request_personal_discovery(
            manager,
            RequestPersonalDiscovery {
                intent: Some(PersonalDiscoveryIntent::Topic("systems".into())),
                result_count,
                idempotency_key: key.into(),
                browser_grant_eligible_sources: None,
            },
            now,
        )
        .unwrap();
    tools
        .claim_discovery_task(
            worker,
            created.task.id,
            now,
            DiscoveryLeaseSeconds::new(300).unwrap(),
        )
        .unwrap();
    created
}

pub(crate) fn personal_result_request(
    task_id: DiscoveryTaskId,
    url: &str,
    role: DiscoveryPlanSourceRole,
    author: Option<&str>,
    key: &str,
) -> CandidateSubmissionRequest {
    CandidateSubmissionRequest {
        target: CandidateSubmissionRequestTarget::PersonalDiscovery {
            task_id,
            allocation_role: role,
            source_facts: CandidateInterestSeedMetadata::default(),
        },
        evidence: CandidateSubmissionEvidence {
            source_url: url.into(),
            source_metadata: CandidateSourceMetadata {
                title: Some("Result".into()),
                author: author.map(str::to_owned),
                published_at: None,
            },
            permitted_excerpt: Some("excerpt".into()),
            summary: Some("summary".into()),
            content_type: CandidateContentType::Article,
            media_references: Vec::new(),
            tags: vec!["systems".into()],
            provenance: CandidateProvenance {
                discovered_at: Utc::now(),
                discovery_method: "browser_search".into(),
                referrer_url: Some("https://news.example/list".into()),
            },
            harness_idempotency_key: key.into(),
            client_idempotency_key: key.into(),
        },
    }
}

pub(crate) fn complete_one_result_batch(
    tools: &AgentTools,
    manager: &AuthContext,
    worker: &AuthContext,
    key: &str,
    url: &str,
) -> (
    RequestedPersonalDiscovery,
    DiscoveryResultBatch,
    SubmittedCandidate,
) {
    let created = claim_personal_run(tools, manager, worker, Some(4), key);
    let mut request = personal_result_request(
        created.task.id,
        url,
        DiscoveryPlanSourceRole::Proven,
        Some("Ada"),
        &format!("{key}-sub"),
    );
    if let CandidateSubmissionRequestTarget::PersonalDiscovery { source_facts, .. } =
        &mut request.target
    {
        *source_facts =
            CandidateInterestSeedMetadata::new(Some("Example Press".into()), Some("rust".into()));
    }
    let submitted = tools.submit_candidate(worker, request).unwrap();
    let batch = tools
        .complete_discovery_result_batch(
            worker,
            CompleteDiscoveryResultBatchRequest {
                task_id: created.task.id,
                submission_ids: vec![submitted.submission.id],
                source_availability: Vec::new(),
                browser_grant_eligible_sources: None,
            },
            Utc::now(),
        )
        .unwrap();
    (created, batch, submitted)
}

pub(crate) fn feedback_signal_total(profile: &TasteProfile) -> u32 {
    let learned: u32 = profile
        .learned
        .iter()
        .map(|weight| {
            weight
                .supporting_signals
                .saturating_add(weight.opposing_signals)
        })
        .sum();
    let affinities: u32 = profile
        .source_affinities
        .iter()
        .map(|affinity| {
            affinity
                .supporting_feedback
                .saturating_add(affinity.opposing_feedback)
        })
        .sum();
    learned.saturating_add(affinities)
}

pub(crate) fn daily_schedule_request(name: &str) -> CreatePersonalDiscoveryScheduleRequest {
    CreatePersonalDiscoveryScheduleRequest {
        name: name.into(),
        cadence: PersonalDiscoveryCadence::Daily,
        intent: PersonalDiscoveryScheduleIntent::default(),
        result_count: Some(5),
        delivery_mode: PersonalDiscoveryDeliveryMode::NotifyWhenSupported,
    }
}
