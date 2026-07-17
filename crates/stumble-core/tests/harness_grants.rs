use stumble_core::*;
use uuid::Uuid;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(prefix: &str) -> Self {
        Self(std::env::temp_dir().join(format!("{prefix}-{}", Uuid::now_v7())))
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

fn create_public_pod(tools: &AgentTools, owner: &AuthContext, name: &str, slug: &str) -> Pod {
    let proposer = harness_context(tools, owner, HarnessCapability::PodCuration, None);
    let approver = harness_context(tools, owner, HarnessCapability::Approval, None);
    let now = chrono::Utc::now();
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::CreatePublicPod {
                request: CreatePodRequest {
                    name: name.into(),
                    slug: slug.into(),
                    description: String::new(),
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
    tools.pod_by_slug(slug, owner.tenant_id).unwrap()
}

#[test]
fn scoped_harness_grants_are_revocable_and_auditable() {
    let data_dir = TestDataDir::new("stumble-harness-grants");
    let tools = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    let owner = tools.default_auth_context().unwrap();
    let allowed = create_public_pod(&tools, &owner, "Allowed", "allowed");
    tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Denied".into(),
                slug: "denied".into(),
                description: String::new(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();

    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "Nightly discovery".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::CandidateSubmission],
                pod_ids: Some(vec![allowed.id]),
            },
        )
        .unwrap();
    assert!(issued.token.expose().starts_with("st_"));
    assert!(!format!("{issued:?}").contains(issued.token.expose()));

    let harness_ctx = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    let visible = tools.list_pods_for_harness(&harness_ctx).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, allowed.id);
    assert!(matches!(
        tools.list_briefs_for_harness(&harness_ctx),
        Err(AgentToolsError::Forbidden { .. })
    ));
    let submission = tools
        .submit_link_to_pod(
            &harness_ctx,
            "allowed",
            SubmitLinkRequest {
                url: "https://example.com/allowed".into(),
                title: Some("Allowed".into()),
                description: None,
                note: None,
                tags: vec![],
                discovered_by_crawler: false,
            },
        )
        .unwrap();
    let error = tools
        .submit_link_to_pod(
            &harness_ctx,
            "denied",
            SubmitLinkRequest {
                url: "https://example.com/denied".into(),
                title: None,
                description: None,
                note: None,
                tags: vec![],
                discovered_by_crawler: false,
            },
        )
        .unwrap_err();
    assert!(matches!(error, AgentToolsError::Forbidden { .. }));
    let error = tools.save_link(&harness_ctx, submission.id).unwrap_err();
    assert!(matches!(error, AgentToolsError::Forbidden { .. }));

    let audits = tools.list_harness_write_audit(&owner).unwrap();
    assert!(audits.iter().any(|entry| {
        entry.harness_id == issued.harness.id
            && entry.operation == HarnessWriteOperation::SubmitLinkToPod
            && entry.pod_id == Some(allowed.id)
    }));

    tools
        .revoke_agent_harness(&owner, issued.harness.id)
        .unwrap();
    assert!(matches!(
        tools.list_pods_for_harness(&harness_ctx),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .is_none());

    drop(tools);
    let reopened = AgentTools::open_home_node(data_dir.path(), seed_store).unwrap();
    assert!(reopened
        .authenticate_token(issued.token.expose())
        .unwrap()
        .is_none());
    assert!(reopened
        .list_harness_write_audit(&reopened.default_auth_context().unwrap())
        .unwrap()
        .iter()
        .any(|entry| entry.harness_id == issued.harness.id));
}

fn harness_context(
    tools: &AgentTools,
    owner: &AuthContext,
    capability: HarnessCapability,
    pod_ids: Option<Vec<PodId>>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            owner,
            RegisterAgentHarnessRequest {
                label: format!("{capability:?}"),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![capability],
                pod_ids,
            },
        )
        .unwrap();
    tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap()
}

#[test]
fn grant_capabilities_are_independent_and_local_only() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let pod = create_public_pod(&tools, &owner, "Capabilities", "capabilities");

    let feed = harness_context(
        &tools,
        &owner,
        HarnessCapability::FeedRead,
        Some(vec![pod.id]),
    );
    tools
        .discover_in_pod(
            &feed,
            "capabilities",
            DiscoverRequest {
                query: "anything".into(),
                avoid: vec![],
                limit: 1,
                mode: DiscoveryMode::DeepMatch,
                user_id: feed.user_id,
            },
        )
        .unwrap();

    let feedback = harness_context(&tools, &owner, HarnessCapability::Feedback, None);
    tools.block_topic(&feedback, "noise".into()).unwrap();

    let tasks = harness_context(
        &tools,
        &owner,
        HarnessCapability::DiscoveryTasks,
        Some(vec![pod.id]),
    );
    tools
        .add_source_to_pod(
            &tasks,
            "capabilities",
            CrawlerSourceType::Rss,
            "https://example.com/feed".into(),
        )
        .unwrap();

    let curation = harness_context(&tools, &owner, HarnessCapability::PodCuration, None);
    let curated = tools
        .create_pod(
            &curation,
            CreatePodRequest {
                name: "Curated".into(),
                slug: "curated".into(),
                description: String::new(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();

    let packages = harness_context(
        &tools,
        &owner,
        HarnessCapability::PackageManagement,
        Some(vec![curated.id]),
    );
    tools
        .patch_skill_pack(
            &packages,
            "curated",
            SkillPackPatch {
                context_md: None,
                pod_yaml: None,
                skill_md: Some("# Capability package\n\nUse only relevant sources.".into()),
                sources_yaml: None,
                filters_yaml: None,
                examples_good_md: None,
                examples_bad_md: None,
            },
        )
        .unwrap();

    let subscriptions = harness_context(
        &tools,
        &owner,
        HarnessCapability::SubscriptionManagement,
        Some(vec![pod.id]),
    );
    tools.join_pod(&subscriptions, "capabilities").unwrap();

    let admin = harness_context(&tools, &owner, HarnessCapability::Administration, None);
    let operations = tools
        .list_harness_write_audit(&admin)
        .unwrap()
        .into_iter()
        .map(|entry| entry.operation)
        .collect::<Vec<_>>();
    for expected in [
        HarnessWriteOperation::BlockTopic,
        HarnessWriteOperation::AddSourceToPod,
        HarnessWriteOperation::CreatePod,
        HarnessWriteOperation::PatchSkillPack,
    ] {
        assert!(operations.contains(&expected));
    }

    let submitter = harness_context(
        &tools,
        &owner,
        HarnessCapability::CandidateSubmission,
        Some(vec![pod.id]),
    );
    assert!(matches!(
        tools.block_source(&submitter, "example.com".into()),
        Err(AgentToolsError::Forbidden { .. })
    ));

    let public = serde_json::to_string(&(
        tools.list_public_pods(&owner).unwrap(),
        tools.federation_pod_events(&owner, "capabilities").unwrap(),
    ))
    .unwrap();
    assert!(!public.contains("Harness Grant"));
    assert!(!public.contains("CandidateSubmission"));
    assert!(!public.contains("submitter"));
}

#[test]
fn submission_reads_and_feedback_preserve_pod_scope_and_revocation() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let allowed = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Scoped".into(),
                slug: "scoped".into(),
                description: String::new(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let denied = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Outside".into(),
                slug: "outside".into(),
                description: String::new(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let submission = tools
        .submit_link_to_pod(
            &owner,
            "outside",
            SubmitLinkRequest {
                url: "https://example.com/outside".into(),
                title: None,
                description: None,
                note: None,
                tags: vec![],
                discovered_by_crawler: false,
            },
        )
        .unwrap();
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "scoped feedback".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::Feedback],
                pod_ids: Some(vec![allowed.id]),
            },
        )
        .unwrap();
    let ctx = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    assert!(matches!(
        tools.assets_for_submission(&ctx, submission.id),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(matches!(
        tools.save_link(&ctx, submission.id),
        Err(AgentToolsError::Forbidden { .. })
    ));
    tools
        .revoke_agent_harness(&owner, issued.harness.id)
        .unwrap();
    assert!(matches!(
        tools.node_info(&ctx),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert_ne!(allowed.id, denied.id);
}

#[test]
fn harnesses_cannot_escalate_or_delegate_administration() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let admin = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "interactive admin".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Administration],
                pod_ids: None,
            },
        )
        .unwrap();
    let admin_ctx = tools
        .authenticate_token(admin.token.expose())
        .unwrap()
        .unwrap();
    let error = tools
        .register_agent_harness(
            &admin_ctx,
            RegisterAgentHarnessRequest {
                label: "delegated admin".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![HarnessCapability::Administration],
                pod_ids: None,
            },
        )
        .unwrap_err();
    assert!(matches!(error, AgentToolsError::Forbidden { .. }));
}

#[test]
fn legacy_dev_tokens_are_identified_but_receive_no_implicit_capabilities() {
    let tools = AgentTools::new(seed_store());
    let issued = tools
        .create_dev_token(DevTokenRequest {
            user_id: None,
            tenant_slug: None,
            label: "legacy".into(),
        })
        .unwrap();
    let ctx = tools.authenticate_token(&issued.token).unwrap().unwrap();
    assert!(ctx.harness_id.is_some());
    assert!(matches!(
        tools.block_source(&ctx, "example.com".into()),
        Err(AgentToolsError::Forbidden { .. })
    ));
}

#[test]
fn brief_reads_filter_by_harness_user_and_pod_scope() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let mut submissions = Vec::new();
    let mut pods = Vec::new();
    for (name, slug) in [
        ("Brief allowed", "brief-allowed"),
        ("Brief denied", "brief-denied"),
    ] {
        let pod = tools
            .create_pod(
                &owner,
                CreatePodRequest {
                    name: name.into(),
                    slug: slug.into(),
                    description: String::new(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap();
        submissions.push(
            tools
                .submit_link_to_pod(
                    &owner,
                    slug,
                    SubmitLinkRequest {
                        url: format!("https://example.com/{slug}"),
                        title: Some(name.into()),
                        description: None,
                        note: None,
                        tags: vec![],
                        discovered_by_crawler: false,
                    },
                )
                .unwrap(),
        );
        pods.push(pod);
    }
    let issued = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "brief reader".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::FeedRead],
                pod_ids: Some(vec![pods[0].id]),
            },
        )
        .unwrap();
    let ctx = tools
        .authenticate_token(issued.token.expose())
        .unwrap()
        .unwrap();
    let generated = tools
        .generate_brief(
            &ctx,
            GenerateBriefRequest {
                pod_slugs: vec!["brief-allowed".into()],
                query: Some("private ranking".into()),
                user_id: Some(Uuid::now_v7()),
            },
        )
        .unwrap();
    assert_eq!(generated.user_id, ctx.user_id);
    let brief = |id, user_id, submission: &Submission| Brief {
        id,
        tenant_id: ctx.tenant_id,
        user_id,
        title: "Private".into(),
        query: None,
        created_at: chrono::Utc::now(),
        private: true,
        items: vec![BriefItem {
            submission_id: submission.id,
            role: "read".into(),
            title: submission.title.clone(),
            url: submission.url.clone(),
            summary: String::new(),
            why_it_matters: String::new(),
            why_user_may_care: String::new(),
        }],
        reflection: None,
    };
    let visible_id = Uuid::now_v7();
    let store = tools.store();
    let mut store = store.write().unwrap();
    store
        .briefs
        .insert(visible_id, brief(visible_id, ctx.user_id, &submissions[0]));
    let outside_pod_id = Uuid::now_v7();
    store.briefs.insert(
        outside_pod_id,
        brief(outside_pod_id, ctx.user_id, &submissions[1]),
    );
    let other_user_id = Uuid::now_v7();
    store.briefs.insert(
        other_user_id,
        brief(other_user_id, Some(Uuid::now_v7()), &submissions[0]),
    );
    drop(store);

    let visible = tools.list_briefs_for_harness(&ctx).unwrap();
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|brief| brief.id == visible_id));
    assert!(visible.iter().any(|brief| brief.id == generated.id));
    assert!(!visible.iter().any(|brief| brief.id == outside_pod_id));
    assert!(!visible.iter().any(|brief| brief.id == other_user_id));
}
