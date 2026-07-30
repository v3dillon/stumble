use super::prelude::*;
use super::*;

#[cfg(test)]
mod federation_projection_tests {
    use super::*;

    fn context(tenant_id: TenantId) -> AuthContext {
        AuthContext {
            user_id: None,
            tenant_id: Some(tenant_id),
            node_id: Uuid::now_v7(),
            harness_id: None,
        }
    }

    fn public_pod(tenant_id: TenantId, slug: &str) -> Pod {
        Pod {
            id: Uuid::now_v7(),
            tenant_id: Some(tenant_id),
            name: slug.to_string(),
            slug: slug.to_string(),
            description: String::new(),
            visibility: Visibility::Public,
            created_by: None,
            created_at: Utc::now(),
            origin_node_id: None,
        }
    }

    fn submission(id: SubmissionId, tenant_id: TenantId, canonical_url: &str) -> Submission {
        Submission {
            id,
            tenant_id: Some(tenant_id),
            url: canonical_url.to_string(),
            canonical_url: canonical_url.to_string(),
            title: "Federated item".to_string(),
            source_metadata: CandidateSourceMetadata::default(),
            description: None,
            domain: "example.com".to_string(),
            submitted_by: None,
            discovered_by_crawler: false,
            submitter_note: None,
            summary: None,
            provenance: Vec::new(),
            media_references: Vec::new(),
            tags: Vec::new(),
            embedding: None,
            created_at: Utc::now(),
            origin_event_id: None,
        }
    }

    fn placement_event(
        origin_node_id: NodeIdentityId,
        pod: &Pod,
        origin_submission: &Submission,
    ) -> EventLog {
        EventLog {
            event_id: Uuid::now_v7(),
            tenant_id: None,
            event_type: PodEventType::ContentItemPlaced,
            pod_slug: pod.slug.clone(),
            author_node_id: origin_node_id,
            author_display_name: None,
            payload_json: json!({
                "content_item": ContentItem::from(origin_submission),
                "accepted_placement": AcceptedPlacementProjection {
                    content_item_id: ContentItemId::from(origin_submission.id),
                    pod_id: pod.id,
                    reason: CurationRationale::new("Federated acceptance").unwrap(),
                    curation_path: CurationPath::ManualReview,
                    origin_node_id,
                    accepted_at: Utc::now(),
                },
            }),
            created_at: Utc::now(),
            previous_event_hash: None,
            content_hash: String::new(),
            signature: String::new(),
            imported_from_peer_id: None,
            verified: true,
        }
    }

    fn removal_event(
        origin_node_id: NodeIdentityId,
        pod: &Pod,
        origin_submission: &Submission,
        placed: Option<&EventLog>,
    ) -> EventLog {
        let origin_placement = placed
            .and_then(|event| {
                serde_json::from_value(event.payload_json["accepted_placement"].clone()).ok()
            })
            .unwrap_or(AcceptedPlacementProjection {
                content_item_id: origin_submission.id.into(),
                pod_id: pod.id,
                reason: CurationRationale::new("Federated acceptance").unwrap(),
                curation_path: CurationPath::ManualReview,
                origin_node_id,
                accepted_at: Utc::now(),
            });
        let tombstone = PlacementTombstone {
            content_reference: feed_content_reference(origin_submission),
            origin_placement,
            withdrawn_at: Utc::now(),
        };
        EventLog {
            event_id: Uuid::now_v7(),
            tenant_id: None,
            event_type: PodEventType::PlacementTombstoned,
            pod_slug: pod.slug.clone(),
            author_node_id: origin_node_id,
            author_display_name: None,
            payload_json: json!({ "placement_tombstone": tombstone }),
            created_at: Utc::now(),
            previous_event_hash: None,
            content_hash: String::new(),
            signature: String::new(),
            imported_from_peer_id: None,
            verified: true,
        }
    }

    #[test]
    fn federated_tombstones_resolve_ids_within_the_importing_tenant() {
        let tenant_a = Uuid::now_v7();
        let tenant_b = Uuid::now_v7();
        let ctx_a = context(tenant_a);
        let ctx_b = context(tenant_b);
        let origin_node_id = Uuid::now_v7();
        let mut pod_a = public_pod(tenant_a, "shared-pod");
        pod_a.origin_node_id = Some(origin_node_id);
        let mut pod_b = public_pod(tenant_b, "shared-pod");
        pod_b.origin_node_id = Some(origin_node_id);
        let origin_submission_id = Uuid::now_v7();
        let origin_submission =
            submission(origin_submission_id, tenant_a, "https://example.com/item");
        let local_a = submission(Uuid::now_v7(), tenant_a, &origin_submission.canonical_url);
        let local_b = submission(Uuid::now_v7(), tenant_b, &origin_submission.canonical_url);
        let mut store = InMemoryStore::default();
        store.pods.insert(pod_a.id, pod_a.clone());
        store.pods.insert(pod_b.id, pod_b.clone());
        store.submissions.insert(local_a.id, local_a.clone());
        store.submissions.insert(local_b.id, local_b.clone());

        let placed = placement_event(origin_node_id, &pod_a, &origin_submission);
        project_imported_public_event(&mut store, &ctx_a, &placed).unwrap();
        project_imported_public_event(&mut store, &ctx_b, &placed).unwrap();
        project_imported_public_event(
            &mut store,
            &ctx_a,
            &removal_event(origin_node_id, &pod_a, &origin_submission, Some(&placed)),
        )
        .unwrap();

        assert!(!store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod_a.id && link.submission_id == local_a.id));
        assert!(store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod_b.id && link.submission_id == local_b.id));
    }

    #[test]
    fn unmapped_federated_tombstone_never_treats_an_origin_id_as_local() {
        let tenant_id = Uuid::now_v7();
        let ctx = context(tenant_id);
        let pod = public_pod(tenant_id, "unmapped-pod");
        let origin_node_id = Uuid::now_v7();
        let coincident_id = Uuid::now_v7();
        let local = submission(coincident_id, tenant_id, "https://local.example/item");
        let mut store = InMemoryStore::default();
        store.pods.insert(pod.id, pod.clone());
        store.submissions.insert(local.id, local.clone());
        store.submission_pods.push(SubmissionPod {
            submission_id: local.id,
            pod_id: pod.id,
            created_at: Utc::now(),
        });

        project_imported_public_event(
            &mut store,
            &ctx,
            &removal_event(origin_node_id, &pod, &local, None),
        )
        .unwrap();

        assert!(store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod.id && link.submission_id == local.id));
    }

    #[test]
    fn federated_content_id_collision_cannot_alias_a_same_tenant_item() {
        let tenant_id = Uuid::now_v7();
        let ctx = context(tenant_id);
        let origin_node_id = Uuid::now_v7();
        let mut pod = public_pod(tenant_id, "remote-collision-pod");
        pod.origin_node_id = Some(origin_node_id);
        let origin_id = Uuid::now_v7();
        let local = submission(origin_id, tenant_id, "https://local.example/item");
        let remote = submission(origin_id, tenant_id, "https://remote.example/item");
        let mut store = InMemoryStore::default();
        store.pods.insert(pod.id, pod.clone());
        store.submissions.insert(local.id, local.clone());

        project_imported_public_event(
            &mut store,
            &ctx,
            &placement_event(origin_node_id, &pod, &remote),
        )
        .unwrap();

        assert_eq!(
            store.submissions.get(&local.id).unwrap().canonical_url,
            local.canonical_url
        );
        let mapped = store
            .federated_content_item_ids
            .get(&FederatedContentItemKey::new(
                Some(tenant_id),
                origin_node_id,
                ContentItemId::from(origin_id),
            ))
            .copied()
            .unwrap();
        assert_ne!(Uuid::from(mapped), origin_id);
        assert_eq!(
            store
                .submissions
                .get(&Uuid::from(mapped))
                .unwrap()
                .canonical_url,
            remote.canonical_url
        );
    }

    #[test]
    fn federated_content_id_collision_cannot_overwrite_another_tenant() {
        let tenant_a = Uuid::now_v7();
        let tenant_b = Uuid::now_v7();
        let ctx_b = context(tenant_b);
        let origin_node_id = Uuid::now_v7();
        let mut pod_b = public_pod(tenant_b, "tenant-b-remote-pod");
        pod_b.origin_node_id = Some(origin_node_id);
        let origin_id = Uuid::now_v7();
        let tenant_a_item = submission(origin_id, tenant_a, "https://tenant-a.example/item");
        let remote = submission(origin_id, tenant_b, "https://remote.example/tenant-b-item");
        let mut store = InMemoryStore::default();
        store.pods.insert(pod_b.id, pod_b.clone());
        store
            .submissions
            .insert(tenant_a_item.id, tenant_a_item.clone());

        project_imported_public_event(
            &mut store,
            &ctx_b,
            &placement_event(origin_node_id, &pod_b, &remote),
        )
        .unwrap();

        assert_eq!(
            store.submissions.get(&tenant_a_item.id).unwrap().tenant_id,
            Some(tenant_a)
        );
        let tenant_b_item = store
            .submissions
            .values()
            .find(|item| {
                item.tenant_id == Some(tenant_b) && item.canonical_url == remote.canonical_url
            })
            .unwrap();
        assert_ne!(tenant_b_item.id, origin_id);
    }

    #[test]
    fn federated_content_deduplicates_canonical_urls_only_within_the_tenant() {
        let tenant_id = Uuid::now_v7();
        let ctx = context(tenant_id);
        let origin_node_id = Uuid::now_v7();
        let mut pod = public_pod(tenant_id, "canonical-dedupe-pod");
        pod.origin_node_id = Some(origin_node_id);
        let local = submission(Uuid::now_v7(), tenant_id, "https://canonical.example/item");
        let remote = submission(Uuid::now_v7(), tenant_id, "https://canonical.example/item");
        let mut store = InMemoryStore::default();
        store.pods.insert(pod.id, pod.clone());
        store.submissions.insert(local.id, local.clone());

        project_imported_public_event(
            &mut store,
            &ctx,
            &placement_event(origin_node_id, &pod, &remote),
        )
        .unwrap();

        assert_eq!(store.submissions.len(), 1);
        assert_eq!(
            store
                .federated_content_item_ids
                .get(&FederatedContentItemKey::new(
                    Some(tenant_id),
                    origin_node_id,
                    ContentItemId::from(remote.id),
                ))
                .copied(),
            Some(ContentItemId::from(local.id))
        );
    }
}
