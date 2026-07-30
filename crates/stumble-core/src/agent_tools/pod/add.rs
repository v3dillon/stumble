use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Turns a shared URL into an Accepted Placement in one authorized step.
    ///
    /// Ensures the target Pod exists (creating the default private `saved`
    /// Pod on first use), ensures the canonical Content Item exists, places it
    /// through the explicit Add to Pod path, and subscribes the caller's User
    /// so the item is Feed-eligible.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL is invalid, an explicitly named Pod is
    /// missing or remote, authorization is denied, or persistence fails.
    pub fn add_reference(
        &self,
        ctx: &AuthContext,
        request: AddReferenceRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<AddedReference, AgentToolsError> {
        let slug = request
            .pod
            .clone()
            .unwrap_or_else(|| DEFAULT_SAVED_POD_SLUG.to_string());
        let existing_pod = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            store.pod_by_slug(&slug, ctx.tenant_id).ok()
        };
        let (pod, pod_created) = match existing_pod {
            Some(pod) => (pod, false),
            None if request.pod.is_none() => {
                let pod = self.create_pod(
                    ctx,
                    CreatePodRequest {
                        name: "Saved".to_string(),
                        slug: DEFAULT_SAVED_POD_SLUG.to_string(),
                        description: "Content shared directly with stumble add".to_string(),
                        visibility: Visibility::Private,
                    },
                )?;
                (pod, true)
            }
            None => return Err(StoreError::NotFound(format!("pod {slug}")).into()),
        };
        let item = self.ensure_reference_content_item(ctx, &request, now)?;
        let placement_request =
            AddContentItemToPodRequest::new(item.id(), pod.id, request.note.clone())
                .map_err(|error| StoreError::Validation(error.to_string()))?;
        let placement = self.add_content_item_to_pod(ctx, placement_request, now)?;
        let subscribed = match self.subscribe_local_pod(ctx, pod.id) {
            Ok(_) => true,
            Err(AgentToolsError::Forbidden { .. }) => false,
            Err(error) => return Err(error),
        };
        Ok(AddedReference {
            content_item: item,
            pod_id: pod.id,
            pod_slug: pod.slug,
            pod_created,
            subscribed,
            placement,
        })
    }

    fn ensure_reference_content_item(
        &self,
        ctx: &AuthContext,
        request: &AddReferenceRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<ContentItem, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let canonical_url = canonicalize_url(&request.url)?;
        if let Some(existing) = store
            .submissions
            .values()
            .find(|item| item.tenant_id == ctx.tenant_id && item.canonical_url == canonical_url)
        {
            return Ok(ContentItem::from(existing));
        }
        let domain = Url::parse(&canonical_url)
            .map_err(|error| AgentToolsError::BadUrl(error.to_string()))?
            .domain()
            .unwrap_or("unknown")
            .to_string();
        let title = request
            .title
            .clone()
            .unwrap_or_else(|| canonical_url.clone());
        let item = Submission {
            id: stable_candidate_uuid(
                "content-item",
                &[
                    &ctx.tenant_id
                        .map_or_else(|| "local".into(), |id| id.to_string()),
                    &canonical_url,
                ],
            ),
            tenant_id: ctx.tenant_id,
            url: request.url.clone(),
            canonical_url,
            title: title.clone(),
            source_metadata: CandidateSourceMetadata {
                title: Some(title),
                ..CandidateSourceMetadata::default()
            },
            description: request.excerpt.clone(),
            domain,
            submitted_by: ctx.user_id,
            discovered_by_crawler: false,
            submitter_note: None,
            summary: request.summary.clone(),
            media_references: request
                .images
                .iter()
                .map(|url| MediaReference::new(MediaReferenceType::Image, url))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| StoreError::Validation(error.to_string()))?,
            provenance: vec![CandidateProvenance {
                discovered_at: now,
                discovery_method: "user_share".to_string(),
                referrer_url: None,
            }],
            tags: request.tags.clone(),
            embedding: None,
            created_at: now,
            origin_event_id: None,
        };
        store.submissions.insert(item.id, item.clone());
        self.persist_locked(&mut store)?;
        Ok(ContentItem::from(&item))
    }
}
