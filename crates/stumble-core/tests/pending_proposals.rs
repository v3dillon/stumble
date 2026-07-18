use chrono::{Duration, Utc};
use stumble_core::*;
use uuid::Uuid;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("stumble-pending-proposals-{}", Uuid::now_v7())))
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn harness_context(
    tools: &AgentTools,
    owner: &AuthContext,
    label: &str,
    kind: AgentHarnessKind,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
    let issued = tools
        .register_agent_harness(
            owner,
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

fn create_public_pod(tools: &AgentTools, owner: &AuthContext, name: &str, slug: &str) -> Pod {
    let proposer = harness_context(
        tools,
        owner,
        "public Pod proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness_context(
        tools,
        owner,
        "public Pod approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
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
            now + Duration::hours(1),
        )
        .unwrap();
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    tools.pod_by_slug(slug, owner.tenant_id).unwrap()
}

#[test]
fn public_exposure_requires_independent_interactive_approval_and_remains_auditable() {
    // Arrange
    let data_dir = TestDataDir::new();
    let tools = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let owner = tools.default_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Approval Test".into(),
                slug: "approval-test".into(),
                description: "private until approved".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let proposer = harness_context(
        &tools,
        &owner,
        "curator",
        AgentHarnessKind::Unattended,
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness_context(
        &tools,
        &owner,
        "owner chat",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
    );
    let other_pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Other approval scope".into(),
                slug: "other-approval-scope".into(),
                description: String::new(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let scoped_approval = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "wrong Pod approver".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::Approval],
                pod_ids: Some(vec![other_pod.id]),
            },
        )
        .unwrap();
    let scoped_approval = tools
        .authenticate_token(scoped_approval.token.expose())
        .unwrap()
        .unwrap();
    let now = Utc::now();

    // Act
    let proposal = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod { pod_id: pod.id },
            now,
            now + Duration::hours(1),
        )
        .unwrap();

    // Assert
    assert_eq!(proposal.status, ProposalStatus::Pending);
    assert_eq!(
        proposal.affected_resources,
        vec![ProposalResource::Pod(pod.id)]
    );
    assert!(!proposal.expected_consequences.is_empty());
    assert_eq!(proposal.structured_diff.len(), 1);
    assert_eq!(proposal.structured_diff[0].before["visibility"], "private");
    assert_eq!(proposal.structured_diff[0].after["visibility"], "public");
    assert_eq!(proposal.proposer, proposer.harness_id.unwrap());
    assert_eq!(
        tools.pod_by_slug("approval-test", None).unwrap().visibility,
        Visibility::Private
    );
    assert!(matches!(
        tools.approve_pending_proposal(&proposer, proposal.id, now),
        Err(AgentToolsError::Forbidden { .. })
    ));
    assert!(matches!(
        tools.approve_pending_proposal(&scoped_approval, proposal.id, now),
        Err(AgentToolsError::Forbidden { .. })
    ));
    let wrong_user = AuthContext {
        user_id: Some(UserId::from(Uuid::now_v7())),
        tenant_id: owner.tenant_id,
        node_id: owner.node_id,
        harness_id: None,
    };
    assert!(matches!(
        tools.pending_proposal(&wrong_user, proposal.id, now),
        Err(AgentToolsError::Forbidden { .. })
    ));

    let accepted = tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    assert_eq!(accepted.status, ProposalStatus::Accepted);
    assert_eq!(
        tools.pod_by_slug("approval-test", None).unwrap().visibility,
        Visibility::Public
    );

    drop(tools);
    let reopened = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    assert_eq!(
        reopened
            .pending_proposal(&reopened.default_auth_context().unwrap(), proposal.id, now)
            .unwrap()
            .status,
        ProposalStatus::Accepted
    );
}

#[test]
fn rejected_and_expired_proposals_are_terminal_without_applying_changes() {
    // Arrange
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let first = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Rejected".into(),
                slug: "rejected".into(),
                description: String::new(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let second = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Expired".into(),
                slug: "expired".into(),
                description: String::new(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let proposer = harness_context(
        &tools,
        &owner,
        "worker",
        AgentHarnessKind::Unattended,
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness_context(
        &tools,
        &owner,
        "interactive owner",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
    );
    let unattended_approver = tools.register_agent_harness(
        &owner,
        RegisterAgentHarnessRequest {
            label: "unattended approver".into(),
            kind: AgentHarnessKind::Unattended,
            capabilities: vec![HarnessCapability::Approval],
            pod_ids: None,
        },
    );
    assert!(matches!(
        unattended_approver,
        Err(AgentToolsError::Forbidden { .. })
    ));
    let now = Utc::now();
    let rejected = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod { pod_id: first.id },
            now,
            now + Duration::hours(1),
        )
        .unwrap();
    let expired = tools
        .create_pending_proposal(
            &proposer,
            SensitiveChange::PublishPod { pod_id: second.id },
            now,
            now + Duration::minutes(1),
        )
        .unwrap();

    // Act
    let rejected = tools
        .reject_pending_proposal(&approver, rejected.id, now, "not ready".into())
        .unwrap();
    let expired = tools
        .pending_proposal(&approver, expired.id, now + Duration::minutes(2))
        .unwrap();

    // Assert
    assert_eq!(rejected.status, ProposalStatus::Rejected);
    assert_eq!(rejected.rejection_reason.as_deref(), Some("not ready"));
    assert_eq!(expired.status, ProposalStatus::Expired);
    assert!(matches!(
        tools.approve_pending_proposal(&approver, expired.id, now + Duration::minutes(2)),
        Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
    assert_eq!(
        tools.pod_by_slug("rejected", None).unwrap().visibility,
        Visibility::Private
    );
    assert_eq!(
        tools.pod_by_slug("expired", None).unwrap().visibility,
        Visibility::Private
    );
}

#[test]
fn authority_trust_and_public_package_changes_apply_only_after_approval() {
    // Arrange
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let pod = create_public_pod(&tools, &owner, "Public Package", "public-package");
    let worker = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "worker".into(),
                kind: AgentHarnessKind::Unattended,
                capabilities: vec![
                    HarnessCapability::Administration,
                    HarnessCapability::PackageManagement,
                ],
                pod_ids: None,
            },
        )
        .unwrap_err();
    assert!(matches!(worker, AgentToolsError::Forbidden { .. }));
    let admin_proposer = harness_context(
        &tools,
        &owner,
        "interactive admin",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Administration],
    );
    let package_proposer = harness_context(
        &tools,
        &owner,
        "package manager",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PackageManagement],
    );
    let approver = harness_context(
        &tools,
        &owner,
        "approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
    );
    let target = harness_context(
        &tools,
        &owner,
        "target",
        AgentHarnessKind::Unattended,
        vec![HarnessCapability::FeedRead],
    )
    .harness_id
    .unwrap();
    let original_peer_count = tools.store().read().unwrap().trusted_peers.len();
    let now = Utc::now();
    let changes = [
        (
            &admin_proposer,
            SensitiveChange::ExpandHarnessGrant {
                harness_id: target,
                capabilities: vec![HarnessCapability::FeedRead, HarnessCapability::Feedback],
                pod_ids: None,
            },
        ),
        (
            &admin_proposer,
            SensitiveChange::AddTrustedPeer {
                node_id: Uuid::now_v7(),
                display_name: "Origin".into(),
                base_url: "https://origin.example".into(),
                public_key: "public-key".into(),
            },
        ),
        (
            &package_proposer,
            SensitiveChange::RevisePublicPodPackage {
                pod_id: pod.id,
                base_version: PackageVersion::new(1).unwrap(),
                patch: SkillPackPatch {
                    context_md: None,
                    pod_yaml: None,
                    skill_md: Some("Approved public instructions".into()),
                    sources_yaml: None,
                    filters_yaml: None,
                    examples_good_md: None,
                    examples_bad_md: None,
                },
            },
        ),
    ];

    // Act
    for (proposer, change) in changes {
        let proposal = tools
            .create_pending_proposal(proposer, change, now, now + Duration::hours(1))
            .unwrap();
        tools
            .approve_pending_proposal(&approver, proposal.id, now)
            .unwrap();
    }

    // Assert
    let store = tools.store();
    let store = store.read().unwrap();
    assert!(store
        .agent_harnesses
        .get(&target)
        .unwrap()
        .grant
        .capabilities
        .contains(&HarnessCapability::Feedback));
    assert_eq!(store.trusted_peers.len(), original_peer_count + 1);
    assert_eq!(
        store.pod_skill_packs.get(&pod.id).unwrap().skill_md,
        "Approved public instructions"
    );
}

#[test]
fn public_content_removal_requires_approval_and_foreign_tenant_cannot_decide() {
    // Arrange
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let pod = create_public_pod(&tools, &owner, "Public Removal", "public-removal");
    let submission = tools
        .submit_link_to_pod(
            &owner,
            &pod.slug,
            SubmitLinkRequest {
                url: "https://example.com/public-removal".into(),
                title: Some("Public removal".into()),
                description: None,
                note: None,
                tags: vec![],
                discovered_by_crawler: false,
            },
        )
        .unwrap();
    let saver = harness_context(
        &tools,
        &owner,
        "saving harness",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Feedback],
    );
    tools.save_link(&saver, submission.id).unwrap();
    let proposer = harness_context(
        &tools,
        &owner,
        "removal proposer",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness_context(
        &tools,
        &owner,
        "removal approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
    );
    let foreign_tenant = tools
        .create_tenant(CreateTenantRequest {
            name: "Foreign".into(),
            slug: "foreign".into(),
        })
        .unwrap();
    let foreign_node = tools
        .store()
        .read()
        .unwrap()
        .node_for_tenant(Some(foreign_tenant.id))
        .unwrap();
    let foreign_owner = AuthContext {
        user_id: owner.user_id,
        tenant_id: Some(foreign_tenant.id),
        node_id: foreign_node.id,
        harness_id: None,
    };
    let foreign_approver = harness_context(
        &tools,
        &foreign_owner,
        "foreign approver",
        AgentHarnessKind::Interactive,
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();

    // Act
    let outcome = tools
        .request_remove_submission_from_pod(&proposer, &pod.slug, submission.id, now)
        .unwrap();
    let RemoveSubmissionOutcome::PendingApproval(proposal) = outcome else {
        panic!("public removal must create a Pending Proposal");
    };

    // Assert
    assert!(matches!(
        tools.approve_pending_proposal(&foreign_approver, proposal.id, now),
        Err(AgentToolsError::Forbidden { .. })
    ));
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    assert!(!tools
        .store()
        .read()
        .unwrap()
        .submission_pods
        .iter()
        .any(|placement| placement.pod_id == pod.id && placement.submission_id == submission.id));
    let store = tools.store();
    let store = store.read().unwrap();
    assert!(store.submissions.contains_key(&submission.id));
    assert!(store
        .saves
        .contains(&(saver.user_id.unwrap(), submission.id)));
    drop(store);
    assert!(tools
        .federation_pod_events(&owner, &pod.slug)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "link_removed"));
}

#[test]
fn legacy_add_peer_proposals_default_the_missing_canonical_node_id() {
    let change: SensitiveChange = serde_json::from_value(serde_json::json!({
        "kind": "add_trusted_peer",
        "display_name": "Legacy peer",
        "base_url": "https://peer.example",
        "public_key": "legacy-key"
    }))
    .unwrap();

    assert!(matches!(
        change,
        SensitiveChange::AddTrustedPeer { node_id, .. } if node_id.is_nil()
    ));
}
