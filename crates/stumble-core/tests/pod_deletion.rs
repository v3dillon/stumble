mod support;

use chrono::Utc;
use stumble_core::*;
use support::*;

fn private_pod(tools: &AgentTools, owner: &AuthContext, slug: &str) -> Pod {
    tools
        .create_pod(
            owner,
            CreatePodRequest {
                name: slug.into(),
                slug: slug.into(),
                description: "Owner-governed collection".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap()
}

#[test]
fn owner_deletes_a_private_pod_and_purges_unshared_content() {
    let tools = AgentTools::new(empty_home_node_store());
    let owner = tools.local_owner_auth_context().unwrap();
    let now = Utc::now();
    let rust = private_pod(&tools, &owner, "rust");
    let keep = private_pod(&tools, &owner, "keep");
    let only_rust = tools
        .add_reference(
            &owner,
            AddReferenceRequest {
                url: "https://example.com/only-rust".into(),
                pod: Some("rust".into()),
                title: Some("Only Rust".into()),
                summary: Some("Lives in one Pod".into()),
                excerpt: None,
                tags: Vec::new(),
                note: None,
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    let shared = tools
        .add_reference(
            &owner,
            AddReferenceRequest {
                url: "https://example.com/shared".into(),
                pod: Some("rust".into()),
                title: Some("Shared".into()),
                summary: Some("Lives in two Pods".into()),
                excerpt: None,
                tags: Vec::new(),
                note: None,
                images: Vec::new(),
            },
            now,
        )
        .unwrap();
    tools
        .add_content_item_to_pod(
            &owner,
            AddContentItemToPodRequest::new(shared.content_item.id(), keep.id, None).unwrap(),
            now,
        )
        .unwrap();

    let outcome = tools.request_delete_pod(&owner, rust.id, now).unwrap();
    let DeletePodOutcome::Deleted(deleted) = outcome else {
        panic!("expected immediate delete");
    };
    assert_eq!(deleted.slug, "rust");
    assert!(!deleted.withdrawn);
    assert!(tools.pod_by_slug("rust", owner.tenant_id).is_err());
    assert!(tools.pod_by_slug("keep", owner.tenant_id).is_ok());

    let kept = tools.list_content_items_for_pod(&owner, keep.id).unwrap();
    assert_eq!(kept[0].id(), shared.content_item.id());
    assert!(tools.list_content_items_for_pod(&owner, rust.id).is_err());
    let store = tools.store();
    let store = store.read().unwrap();
    assert!(!store
        .submissions
        .contains_key(&only_rust.content_item.id().into()));
    assert!(store
        .submissions
        .contains_key(&shared.content_item.id().into()));
}

#[test]
fn inbox_and_remote_replicas_cannot_be_deleted() {
    let tools = AgentTools::new(empty_home_node_store());
    let owner = tools.local_owner_auth_context().unwrap();
    let now = Utc::now();
    let inbox_slug = format!("inbox-{}", owner.user_id.unwrap());
    let inbox = private_pod(&tools, &owner, &inbox_slug);
    assert!(matches!(
        tools.request_delete_pod(&owner, inbox.id, now),
        Err(AgentToolsError::Store(StoreError::Validation(message)))
            if message.contains("Inbox")
    ));
    assert!(tools.pod_by_slug(&inbox_slug, owner.tenant_id).is_ok());

    let origin = AgentTools::new(empty_home_node_store());
    let replica = create_public_pod(&origin, "remote-systems");
    submit_and_accept_placement(&origin, &replica);
    let snapshot = origin
        .federation_pod_snapshot(&origin.default_auth_context().unwrap(), &replica.slug, None)
        .unwrap();
    let subscriber = register_authenticated_harness(
        &tools,
        "subscriber",
        vec![HarnessCapability::SubscriptionManagement],
    );
    tools
        .subscribe_public_pod(
            &subscriber,
            SubscribePublicPodRequest::new(
                "https://origin.example/federation/pods/remote-systems",
                snapshot,
            ),
            now,
        )
        .unwrap();
    let local = tools
        .pod_by_slug("remote-systems", owner.tenant_id)
        .unwrap();
    assert!(matches!(
        tools.request_delete_pod(&owner, local.id, now),
        Err(AgentToolsError::Forbidden { .. })
            | Err(AgentToolsError::Store(StoreError::Validation(_)))
    ));
    assert!(tools.pod_by_slug("remote-systems", owner.tenant_id).is_ok());
}

#[test]
fn public_delete_from_a_harness_waits_for_approval_then_withdraws() {
    let tools = AgentTools::new(empty_home_node_store());
    let pod = create_public_pod(&tools, "systems");
    let proposer = register_authenticated_harness(
        &tools,
        "delete proposer",
        vec![HarnessCapability::PodCuration],
    );
    let approver = register_authenticated_harness(
        &tools,
        "delete approver",
        vec![HarnessCapability::Approval],
    );
    let now = Utc::now();
    let outcome = tools.request_delete_pod(&proposer, pod.id, now).unwrap();
    let DeletePodOutcome::PendingApproval(proposal) = outcome else {
        panic!("expected a Pending Proposal for a public Pod");
    };
    assert!(tools.pod_by_slug("systems", None).is_ok());
    tools
        .approve_pending_proposal(&approver, proposal.id, now)
        .unwrap();
    assert!(tools.pod_by_slug("systems", None).is_err());
    let store = tools.store();
    let store = store.read().unwrap();
    let node = store.node_for_tenant(None).unwrap();
    assert!(store
        .known_pod_withdrawals
        .contains_key(&(node.id, "systems".into())));
}
