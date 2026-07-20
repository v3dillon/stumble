use chrono::{TimeZone, Utc};
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-interest-seeds-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness(tools: &AgentTools, kind: AgentHarnessKind) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "seed submitter".into(),
                kind,
                capabilities: vec![
                    HarnessCapability::CandidateSubmission,
                    HarnessCapability::Feedback,
                ],
                pod_ids: None,
            },
        )
        .unwrap();
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

fn harness_with(
    tools: &AgentTools,
    kind: AgentHarnessKind,
    capabilities: Vec<HarnessCapability>,
    pod_ids: Option<Vec<PodId>>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "scoped seed harness".into(),
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

fn request(url: &str, learn: bool, key: &str) -> CandidateSubmissionRequest {
    CandidateSubmissionRequest {
        target: CandidateSubmissionRequestTarget::User {
            learn,
            interest_seed_metadata: CandidateInterestSeedMetadata {
                publisher: Some("Systems Weekly".into()),
                community: Some("rust-lang".into()),
            },
        },
        evidence: CandidateSubmissionEvidence {
            source_url: url.into(),
            source_metadata: CandidateSourceMetadata {
                title: Some("Typed evidence".into()),
                author: Some("Ada".into()),
                published_at: None,
            },
            permitted_excerpt: None,
            summary: None,
            content_type: CandidateContentType::Article,
            media_references: Vec::new(),
            tags: vec!["Rust".into()],
            provenance: CandidateProvenance {
                discovered_at: Utc.with_ymd_and_hms(2026, 7, 19, 10, 0, 0).unwrap(),
                discovery_method: "user_submission".into(),
                referrer_url: Some("https://news.ycombinator.com/item?id=1".into()),
            },
            harness_idempotency_key: format!("harness-{key}"),
            client_idempotency_key: format!("client-{key}"),
        },
    }
}

#[test]
fn interactive_submission_creates_one_retractable_private_seed() {
    let tools = AgentTools::new(seed_store());
    let user = harness(&tools, AgentHarnessKind::Interactive);
    let first = tools
        .submit_candidate(
            &user,
            request("https://EXAMPLE.com/story?utm_source=x#top", true, "one"),
        )
        .unwrap();
    tools
        .submit_candidate(&user, request("https://example.com/story", true, "two"))
        .unwrap();

    let profile = tools.taste_profile(&user).unwrap();
    assert_eq!(profile.interest_seed_evidence.active_seed_count, 1);
    assert!(profile
        .allowed_actions
        .contains(&TasteProfileAllowedAction::Retract));
    assert!(profile
        .source_affinities
        .iter()
        .all(|affinity| affinity.weight == 0.0));
    assert!(profile.source_affinities.iter().any(|affinity| {
        affinity.signal == SourceAffinitySignal::Source("example.com".into())
            && affinity.supporting_seeds == 1
    }));
    assert!(profile.source_affinities.iter().any(|affinity| {
        affinity.signal == SourceAffinitySignal::Publisher("systems weekly".into())
    }));
    assert!(profile.source_affinities.iter().any(|affinity| {
        affinity.signal == SourceAffinitySignal::AuthorOrAccount("ada".into())
    }));
    assert!(profile.source_affinities.iter().any(|affinity| {
        affinity.signal == SourceAffinitySignal::Community("rust-lang".into())
    }));
    assert!(profile.source_affinities.iter().any(|affinity| {
        affinity.signal == SourceAffinitySignal::ReferrerContext("news.ycombinator.com".into())
    }));
    assert!(profile
        .learned
        .iter()
        .all(|weight| matches!(weight.signal, LearnedTasteSignal::Topic(_))));
    assert!(!serde_json::to_string(&profile)
        .unwrap()
        .contains("https://example.com/story"));

    tools
        .retract_interest_seed(&user, first.candidate.id)
        .unwrap();
    assert_eq!(
        tools
            .store()
            .read()
            .unwrap()
            .harness_write_audit
            .last()
            .unwrap()
            .operation,
        HarnessWriteOperation::RetractInterestSeed
    );
    let retracted = tools.taste_profile(&user).unwrap();
    assert_eq!(retracted.interest_seed_evidence.active_seed_count, 0);
    assert!(tools.inspect_candidate(&user, first.candidate.id).is_ok());
}

#[test]
fn ip_source_retains_non_domain_interest_seed_evidence() {
    let tools = AgentTools::new(seed_store());
    let user = harness(&tools, AgentHarnessKind::Interactive);

    tools
        .submit_candidate(&user, request("https://127.0.0.1/story", true, "ip"))
        .unwrap();

    let profile = tools.taste_profile(&user).unwrap();
    assert!(profile
        .learned
        .iter()
        .any(|weight| weight.signal == LearnedTasteSignal::Topic("rust".into())));
    let affinities = profile.source_affinities;
    for signal in [
        SourceAffinitySignal::Publisher("systems weekly".into()),
        SourceAffinitySignal::AuthorOrAccount("ada".into()),
        SourceAffinitySignal::Community("rust-lang".into()),
        SourceAffinitySignal::ReferrerContext("news.ycombinator.com".into()),
    ] {
        assert!(affinities.iter().any(|affinity| affinity.signal == signal));
    }
}

#[test]
fn explicit_preferences_override_inferred_affinity() {
    let tools = AgentTools::new(seed_store());
    let user = harness(&tools, AgentHarnessKind::Interactive);
    tools
        .submit_candidate(&user, request("https://example.com/rust", true, "one"))
        .unwrap();
    let mut prefer = UpdateTasteProfileRequest::default();
    prefer.interests = Some(vec!["rust".into()]);
    let preferred = tools.update_taste_profile(&user, prefer).unwrap();
    assert_eq!(preferred.explicit.interests, vec!["rust"]);
    assert!(preferred
        .learned
        .iter()
        .any(|weight| weight.signal == LearnedTasteSignal::Topic("rust".into())));

    let mut block = UpdateTasteProfileRequest::default();
    block.blocked_topics = Some(vec!["rust".into()]);
    block.blocked_sources = Some(vec![
        "example.com".into(),
        "systems weekly".into(),
        "ada".into(),
        "rust-lang".into(),
        "news.ycombinator.com".into(),
    ]);
    let blocked = tools.update_taste_profile(&user, block).unwrap();
    assert_eq!(blocked.explicit.blocked_topics, vec!["rust"]);
    assert!(blocked.source_affinities.iter().any(|affinity| {
        affinity.signal == SourceAffinitySignal::Source("example.com".into())
            && affinity.explicitly_blocked
    }));
    for signal in [
        SourceAffinitySignal::Publisher("systems weekly".into()),
        SourceAffinitySignal::AuthorOrAccount("ada".into()),
        SourceAffinitySignal::Community("rust-lang".into()),
        SourceAffinitySignal::ReferrerContext("news.ycombinator.com".into()),
    ] {
        assert!(blocked
            .source_affinities
            .iter()
            .any(|affinity| affinity.signal == signal && !affinity.explicitly_blocked));
    }

    let mut qualify = UpdateTasteProfileRequest::default();
    qualify.blocked_source_affinities = Some(vec![
        SourceAffinitySignal::Publisher("Systems Weekly".into()),
        SourceAffinitySignal::AuthorOrAccount("Ada".into()),
        SourceAffinitySignal::Community("Rust-Lang".into()),
        SourceAffinitySignal::ReferrerContext("News.YCombinator.com".into()),
    ]);
    let qualified = tools.update_taste_profile(&user, qualify).unwrap();
    assert_eq!(
        qualified.explicit.blocked_source_affinities,
        vec![
            SourceAffinitySignal::Publisher("Systems Weekly".into()),
            SourceAffinitySignal::AuthorOrAccount("Ada".into()),
            SourceAffinitySignal::Community("Rust-Lang".into()),
            SourceAffinitySignal::ReferrerContext("News.YCombinator.com".into()),
        ]
    );
    for signal in [
        SourceAffinitySignal::Publisher("systems weekly".into()),
        SourceAffinitySignal::AuthorOrAccount("ada".into()),
        SourceAffinitySignal::Community("rust-lang".into()),
        SourceAffinitySignal::ReferrerContext("news.ycombinator.com".into()),
    ] {
        assert!(qualified
            .source_affinities
            .iter()
            .any(|affinity| affinity.signal == signal && affinity.explicitly_blocked));
    }
}

#[test]
fn seeds_and_retractions_survive_restart() {
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let issued = tools
        .register_agent_harness(
            &tools.default_auth_context().unwrap(),
            RegisterAgentHarnessRequest {
                label: "persistent seed user".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![
                    HarnessCapability::CandidateSubmission,
                    HarnessCapability::Feedback,
                ],
                pod_ids: None,
            },
        )
        .unwrap();
    let token = issued.token.expose().to_string();
    let user = tools.authenticate_token(&token).unwrap().unwrap();
    let submitted = tools
        .submit_candidate(
            &user,
            request("https://persist.example/rust", true, "persist"),
        )
        .unwrap();
    tools
        .retract_interest_seed(&user, submitted.candidate.id)
        .unwrap();
    drop(tools);

    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let user = reopened.authenticate_token(&token).unwrap().unwrap();
    let profile = reopened.taste_profile(&user).unwrap();
    assert_eq!(profile.interest_seed_evidence.active_seed_count, 0);
    assert_eq!(profile.interest_seed_evidence.retracted_seed_count, 1);
    assert!(reopened
        .inspect_candidate(&user, submitted.candidate.id)
        .is_ok());
}

#[test]
fn independent_user_submissions_corroborate_but_opt_out_and_workers_do_not() {
    let tools = AgentTools::new(seed_store());
    let user = harness(&tools, AgentHarnessKind::Interactive);
    tools
        .submit_candidate(&user, request("https://one.example/rust", true, "one"))
        .unwrap();
    tools
        .submit_candidate(&user, request("https://two.example/rust", false, "opt-out"))
        .unwrap();
    assert_eq!(
        tools
            .taste_profile(&user)
            .unwrap()
            .learned
            .iter()
            .find(|weight| weight.signal == LearnedTasteSignal::Topic("rust".into()))
            .unwrap()
            .weight,
        0.0
    );

    tools
        .submit_candidate(&user, request("https://three.example/rust", true, "two"))
        .unwrap();
    assert!(
        tools
            .taste_profile(&user)
            .unwrap()
            .learned
            .iter()
            .find(|weight| weight.signal == LearnedTasteSignal::Topic("rust".into()))
            .unwrap()
            .weight
            > 0.0
    );

    let worker = harness(&tools, AgentHarnessKind::Unattended);
    let result = tools.submit_candidate(
        &worker,
        request("https://worker.example/rust", true, "worker"),
    );
    assert!(matches!(result, Err(AgentToolsError::Forbidden { .. })));
    let candidate_id = tools.list_candidates(&user).unwrap()[0].id;
    assert!(matches!(
        tools.retract_interest_seed(&worker, candidate_id),
        Err(AgentToolsError::Forbidden { .. })
    ));
}

#[test]
fn user_target_requires_unscoped_interactive_authority_and_private_reads() {
    let tools = AgentTools::new(seed_store());
    let scoped = harness_with(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::CandidateSubmission],
        Some(Vec::new()),
    );
    assert!(matches!(
        tools.submit_candidate(
            &scoped,
            request("https://private.example/scoped", true, "scoped")
        ),
        Err(AgentToolsError::Forbidden { .. })
    ));

    let user = harness(&tools, AgentHarnessKind::Interactive);
    let submitted = tools
        .submit_candidate(&user, request("https://private.example/user", true, "user"))
        .unwrap();
    assert!(matches!(
        submitted.submission.target,
        CandidateSubmissionTarget::User { user_id, .. } if user_id == user.user_id.unwrap()
    ));
    let profile_only = harness_with(
        &tools,
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Feedback],
        None,
    );
    assert!(!tools
        .list_candidates(&profile_only)
        .unwrap()
        .iter()
        .any(|candidate| candidate.id == submitted.candidate.id));
    assert!(matches!(
        tools.inspect_candidate(&profile_only, submitted.candidate.id),
        Err(AgentToolsError::Forbidden { .. })
    ));
}

#[test]
fn learned_reset_clears_seed_affinities() {
    let tools = AgentTools::new(seed_store());
    let user = harness(&tools, AgentHarnessKind::Interactive);
    tools
        .submit_candidate(&user, request("https://one.example/rust", true, "one"))
        .unwrap();
    tools
        .submit_candidate(&user, request("https://two.example/rust", true, "two"))
        .unwrap();
    assert!(!tools
        .taste_profile(&user)
        .unwrap()
        .source_affinities
        .is_empty());

    let targeted = tools
        .reset_learned_taste(
            &user,
            ResetLearnedTasteRequest::for_signal(LearnedTasteSignal::Source("one.example".into())),
        )
        .unwrap();
    assert!(!targeted
        .source_affinities
        .iter()
        .any(|affinity| { affinity.signal == SourceAffinitySignal::Source("one.example".into()) }));
    assert!(targeted
        .learned
        .iter()
        .any(|weight| { weight.signal == LearnedTasteSignal::Topic("rust".into()) }));

    let reset = tools
        .reset_learned_taste(&user, ResetLearnedTasteRequest::all())
        .unwrap();
    assert!(reset.source_affinities.is_empty());
    assert_eq!(reset.interest_seed_evidence.active_seed_count, 0);
    assert_eq!(reset.interest_seed_evidence.retracted_seed_count, 2);
    assert_eq!(
        reset.allowed_actions,
        vec![
            TasteProfileAllowedAction::Set,
            TasteProfileAllowedAction::Reset
        ]
    );
}
