use chrono::Utc;
use stumble_core::*;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "stumble-subscription-role-migration-{}",
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

fn context_for_user(user_id: UserId, node_id: NodeIdentityId) -> AuthContext {
    AuthContext {
        user_id: Some(user_id),
        tenant_id: None,
        node_id,
        harness_id: None,
    }
}

fn harness_for_user(
    tools: &AgentTools,
    user_id: UserId,
    label: &str,
    capabilities: Vec<HarnessCapability>,
) -> AuthContext {
    let node_id = tools.default_auth_context().unwrap().node_id;
    let issued = tools
        .register_agent_harness(
            &context_for_user(user_id, node_id),
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

#[test]
fn curator_authority_and_subscription_feed_eligibility_are_independent() {
    let tools = AgentTools::new(seed_store());
    let mut owner = tools.default_auth_context().unwrap();
    owner.user_id = Some(*tools.store().read().unwrap().users.keys().next().unwrap());
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Independent relationships".into(),
                slug: "independent-relationships".into(),
                description: "Subscription grants no Pod authority".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let curator_id = uuid::Uuid::now_v7();
    let subscriber_id = uuid::Uuid::now_v7();
    {
        let shared = tools.store();
        let mut store = shared.write().unwrap();
        for (id, name) in [(curator_id, "Curator"), (subscriber_id, "Subscriber")] {
            store.users.insert(
                id,
                User {
                    id,
                    display_name: name.into(),
                    created_at: Utc::now(),
                },
            );
        }
        store.pod_roles.push(PodRoleAssignment {
            user_id: curator_id,
            pod_id: pod.id,
            role: PodRole::Curator,
            created_at: Utc::now(),
        });
    }

    let subscriber = harness_for_user(
        &tools,
        subscriber_id,
        "subscriber",
        vec![
            HarnessCapability::SubscriptionManagement,
            HarnessCapability::PodCuration,
        ],
    );
    tools.join_pod(&subscriber, &pod.slug).unwrap();

    let curator = harness_for_user(
        &tools,
        curator_id,
        "curator",
        vec![HarnessCapability::PodCuration],
    );
    tools
        .set_pod_curation_policy(&curator, pod.id, CurationPolicy::Manual, Utc::now())
        .unwrap();
    let denied = tools
        .set_pod_curation_policy(
            &subscriber,
            pod.id,
            CurationPolicy::Assisted {
                confidence_threshold: CandidateConfidence::new(0.9).unwrap(),
            },
            Utc::now(),
        )
        .unwrap_err();

    assert!(matches!(denied, AgentToolsError::Forbidden { .. }));
    let shared = tools.store();
    let store = shared.read().unwrap();
    assert!(store.subscriptions.values().any(|subscription| {
        subscription.user_id == subscriber_id && subscription.local_pod_id == pod.id
    }));
    assert!(!store.subscriptions.values().any(|subscription| {
        subscription.user_id == curator_id && subscription.local_pod_id == pod.id
    }));
    assert!(!store
        .pod_roles
        .iter()
        .any(|assignment| assignment.user_id == subscriber_id && assignment.pod_id == pod.id));
    assert!(store.pod_roles.iter().any(|assignment| {
        assignment.user_id == curator_id
            && assignment.pod_id == pod.id
            && assignment.role == PodRole::Curator
    }));
}

#[test]
fn legacy_memberships_migrate_losslessly_and_idempotently_across_restart() {
    let data_dir = TestDataDir::new();
    let legacy_path = data_dir.0.join("store.json");
    let tools = AgentTools::new(seed_store());
    let mut owner = tools.default_auth_context().unwrap();
    owner.user_id = Some(*tools.store().read().unwrap().users.keys().next().unwrap());
    let make_pod = |slug: &str| {
        tools
            .create_pod(
                &owner,
                CreatePodRequest {
                    name: slug.into(),
                    slug: slug.into(),
                    description: "Legacy Pod membership".into(),
                    visibility: Visibility::Private,
                },
            )
            .unwrap()
    };
    let owner_pod = make_pod("legacy-owner");
    let curator_pod = make_pod("legacy-curator");
    let member_pod = make_pod("legacy-member");
    let curator_id = uuid::Uuid::now_v7();
    let member_id = uuid::Uuid::now_v7();
    let passive_tenant_member_id = uuid::Uuid::now_v7();
    let now = Utc::now();
    {
        let shared = tools.store();
        let mut store = shared.write().unwrap();
        for (id, name) in [
            (curator_id, "Legacy Curator"),
            (member_id, "Legacy Member"),
            (passive_tenant_member_id, "Passive Tenant Member"),
        ] {
            store.users.insert(
                id,
                User {
                    id,
                    display_name: name.into(),
                    created_at: now,
                },
            );
        }
        let tenant_id = *store.tenants.keys().next().unwrap();
        store.tenant_users.push(TenantUser {
            tenant_id,
            user_id: passive_tenant_member_id,
            role: TenantRole::Member,
            created_at: now,
        });
        let node = store
            .node_identities
            .values()
            .find(|node| node.tenant_id.is_none())
            .unwrap()
            .clone();
        let subscription = Subscription::new_local(
            uuid::Uuid::now_v7().into(),
            member_id,
            &member_pod,
            &node,
            now,
        );
        store.subscriptions.insert(subscription.id, subscription);
    }
    save_store_snapshot(&tools.store().read().unwrap(), &legacy_path).unwrap();
    let mut snapshot: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&legacy_path).unwrap()).unwrap();
    let object = snapshot.as_object_mut().unwrap();
    object.remove("pod_roles");
    for subscription in object
        .get_mut("subscriptions")
        .unwrap()
        .as_array_mut()
        .unwrap()
    {
        subscription.as_object_mut().unwrap().remove("is_priority");
    }
    object.insert(
        "pod_memberships".into(),
        serde_json::json!([
            {
                "user_id": owner.user_id.unwrap(),
                "pod_id": owner_pod.id,
                "role": "owner",
                "is_priority": true,
                "created_at": now,
            },
            {
                "user_id": curator_id,
                "pod_id": curator_pod.id,
                "role": "moderator",
                "is_priority": false,
                "created_at": now,
            },
            {
                "user_id": member_id,
                "pod_id": member_pod.id,
                "role": "member",
                "is_priority": true,
                "created_at": now,
            }
        ]),
    );
    std::fs::write(&legacy_path, serde_json::to_vec_pretty(&snapshot).unwrap()).unwrap();

    let first_load = load_store_snapshot(&legacy_path).unwrap();
    let second_load = load_store_snapshot(&legacy_path).unwrap();
    let first_ids = first_load
        .subscriptions
        .keys()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let second_ids = second_load
        .subscriptions
        .keys()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(first_ids, second_ids);

    let migrated = AgentTools::open_home_node(&data_dir.0, || {
        load_store_snapshot(&legacy_path).expect("load legacy membership snapshot")
    })
    .unwrap();
    assert_migrated_relationships(
        &migrated,
        owner.user_id.unwrap(),
        owner_pod.id,
        curator_id,
        curator_pod.id,
        member_id,
        member_pod.id,
        passive_tenant_member_id,
    );
    drop(migrated);

    let restarted = AgentTools::open_home_node(&data_dir.0, InMemoryStore::default).unwrap();
    assert_migrated_relationships(
        &restarted,
        owner.user_id.unwrap(),
        owner_pod.id,
        curator_id,
        curator_pod.id,
        member_id,
        member_pod.id,
        passive_tenant_member_id,
    );
}

#[allow(clippy::too_many_arguments)]
fn assert_migrated_relationships(
    tools: &AgentTools,
    owner_id: UserId,
    owner_pod_id: PodId,
    curator_id: UserId,
    curator_pod_id: PodId,
    member_id: UserId,
    member_pod_id: PodId,
    passive_tenant_member_id: UserId,
) {
    let shared = tools.store();
    let store = shared.read().unwrap();
    assert!(store.pod_roles.iter().any(|assignment| {
        assignment.user_id == owner_id
            && assignment.pod_id == owner_pod_id
            && assignment.role == PodRole::Owner
    }));
    assert!(store.pod_roles.iter().any(|assignment| {
        assignment.user_id == curator_id
            && assignment.pod_id == curator_pod_id
            && assignment.role == PodRole::Curator
    }));
    assert!(!store
        .pod_roles
        .iter()
        .any(|assignment| assignment.user_id == member_id));
    for (user_id, pod_id, is_priority) in [
        (owner_id, owner_pod_id, true),
        (curator_id, curator_pod_id, false),
        (member_id, member_pod_id, true),
    ] {
        let subscription = store
            .subscriptions
            .values()
            .find(|subscription| {
                subscription.user_id == user_id && subscription.local_pod_id == pod_id
            })
            .unwrap();
        assert_eq!(subscription.is_priority, is_priority);
    }
    assert!(!store
        .subscriptions
        .values()
        .any(|subscription| subscription.user_id == passive_tenant_member_id));
}

#[test]
fn priority_is_stored_on_the_subscription_without_changing_pod_roles() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.default_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Priority relationship".into(),
                slug: "priority-relationship".into(),
                description: "Priority belongs to Subscription".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let subscriber_id = uuid::Uuid::now_v7();
    {
        let shared = tools.store();
        let mut store = shared.write().unwrap();
        store.users.insert(
            subscriber_id,
            User {
                id: subscriber_id,
                display_name: "Priority Subscriber".into(),
                created_at: Utc::now(),
            },
        );
    }
    let subscriber = harness_for_user(
        &tools,
        subscriber_id,
        "priority subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );
    tools.join_pod(&subscriber, &pod.slug).unwrap();
    let roles_before = tools.store().read().unwrap().pod_roles.clone();

    tools
        .set_priority_subscription(&subscriber, pod.id, true)
        .unwrap();

    let shared = tools.store();
    let store = shared.read().unwrap();
    let subscription = store
        .subscriptions
        .values()
        .find(|subscription| {
            subscription.user_id == subscriber_id && subscription.local_pod_id == pod.id
        })
        .unwrap();
    assert!(subscription.is_priority);
    assert_eq!(store.pod_roles, roles_before);
}

#[test]
fn role_grants_require_an_owner_proposal_and_independent_approval() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.local_owner_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Governed roles".into(),
                slug: "governed-roles".into(),
                description: "Two-step role delegation".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let target_id = uuid::Uuid::now_v7();
    {
        let shared = tools.store();
        shared.write().unwrap().users.insert(
            target_id,
            User {
                id: target_id,
                display_name: "Future Curator".into(),
                created_at: Utc::now(),
            },
        );
    }
    let owner_id = owner.user_id.unwrap();
    let proposer = harness_for_user(
        &tools,
        owner_id,
        "role proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = harness_for_user(
        &tools,
        owner_id,
        "independent approver",
        vec![HarnessCapability::Approval],
    );

    let proposal = tools
        .request_grant_pod_role(&proposer, pod.id, target_id, PodRole::Curator, Utc::now())
        .unwrap();
    assert!(tools
        .list_pod_roles(&owner, pod.id)
        .unwrap()
        .iter()
        .all(|assignment| assignment.user_id != target_id));
    let self_approval = tools
        .approve_pending_proposal(&proposer, proposal.id, Utc::now())
        .unwrap_err();
    assert!(matches!(self_approval, AgentToolsError::Forbidden { .. }));

    tools
        .approve_pending_proposal(&approver, proposal.id, Utc::now())
        .unwrap();
    assert!(tools
        .list_pod_roles(&owner, pod.id)
        .unwrap()
        .iter()
        .any(|assignment| assignment.user_id == target_id && assignment.role == PodRole::Curator));
}

#[test]
fn subscribers_curators_owners_and_scoped_harnesses_have_separate_authority() {
    let tools = AgentTools::new(seed_store());
    let owner = tools.local_owner_auth_context().unwrap();
    let pod = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Scoped authority".into(),
                slug: "scoped-authority".into(),
                description: "Relationship authorization".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let other = tools
        .create_pod(
            &owner,
            CreatePodRequest {
                name: "Other scope".into(),
                slug: "other-scope".into(),
                description: "Outside the grant".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    let subscriber_id = uuid::Uuid::now_v7();
    let curator_id = uuid::Uuid::now_v7();
    {
        let shared = tools.store();
        let mut store = shared.write().unwrap();
        for (id, display_name) in [(subscriber_id, "Subscriber"), (curator_id, "Curator")] {
            store.users.insert(
                id,
                User {
                    id,
                    display_name: display_name.into(),
                    created_at: Utc::now(),
                },
            );
        }
        store.pod_roles.push(PodRoleAssignment {
            user_id: curator_id,
            pod_id: pod.id,
            role: PodRole::Curator,
            created_at: Utc::now(),
        });
    }
    let subscriber = harness_for_user(
        &tools,
        subscriber_id,
        "subscriber harness",
        vec![HarnessCapability::SubscriptionManagement],
    );
    tools.subscribe_local_pod(&subscriber, pod.id).unwrap();
    assert_eq!(
        tools.pod_allowed_actions(&subscriber, pod.id).unwrap(),
        vec![
            PodAllowedAction::Unsubscribe,
            PodAllowedAction::SubscriptionSet
        ]
    );
    assert!(matches!(
        tools.list_pod_roles(&subscriber, pod.id),
        Err(AgentToolsError::Forbidden { .. })
    ));

    let curator = harness_for_user(
        &tools,
        curator_id,
        "curator harness",
        vec![HarnessCapability::PodCuration],
    );
    assert!(tools.list_pod_roles(&curator, pod.id).is_ok());
    assert_eq!(
        tools.pod_allowed_actions(&curator, pod.id).unwrap(),
        vec![PodAllowedAction::RoleList]
    );
    assert!(matches!(
        tools.request_grant_pod_role(
            &curator,
            pod.id,
            subscriber_id,
            PodRole::Curator,
            Utc::now()
        ),
        Err(AgentToolsError::Forbidden { .. })
    ));

    let scoped = tools
        .register_agent_harness(
            &owner,
            RegisterAgentHarnessRequest {
                label: "scoped subscriber".into(),
                kind: AgentHarnessKind::Interactive,
                capabilities: vec![HarnessCapability::SubscriptionManagement],
                pod_ids: Some(vec![pod.id]),
            },
        )
        .unwrap();
    let scoped = tools
        .authenticate_token(scoped.token.expose())
        .unwrap()
        .unwrap();
    assert!(tools.subscribe_local_pod(&scoped, pod.id).is_ok());
    assert!(matches!(
        tools.subscribe_local_pod(&scoped, other.id),
        Err(AgentToolsError::Forbidden { .. })
    ));
}
