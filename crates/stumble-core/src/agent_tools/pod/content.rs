use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Immediately creates an Accepted Placement for an existing Content Item.
    ///
    /// # Errors
    ///
    /// Returns an error when the item or Pod is missing, authorization is denied,
    /// the Pod is remote, the note is empty, or persistence fails.
    pub fn add_content_item_to_pod(
        &self,
        ctx: &AuthContext,
        request: AddContentItemToPodRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPlacement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, request.pod_id)?;
        let item = store
            .submissions
            .get(&Uuid::from(request.content_item_id))
            .filter(|item| item.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Content Item".into()))?;
        let origin_placements = store
            .accepted_placement_projections
            .values()
            .filter(|placement| placement.content_item_id == request.content_item_id)
            .cloned()
            .collect::<Vec<_>>();
        if let Some((key, existing)) = store.pod_placements.iter().find(|(_, placement)| {
            placement.pod_id == request.pod_id
                && placement.content_item_id == Some(request.content_item_id)
        }) {
            if existing.status == PodPlacementStatus::Accepted {
                return Ok(existing.clone());
            }
            let key = *key;
            let candidate = store
                .candidates
                .get(&existing.candidate_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
            let actor = curation_actor(ctx);
            let placement = store
                .pod_placements
                .get_mut(&key)
                .ok_or_else(|| StoreError::NotFound("Pod Placement".into()))?;
            placement.status = PodPlacementStatus::Accepted;
            placement.curation_path = CurationPath::AddToPod;
            placement.actor = actor;
            let retained_origin_ids = placement
                .origin_placements
                .iter()
                .map(origin_placement_identity)
                .collect::<HashSet<_>>();
            placement
                .origin_placements
                .extend(origin_placements.into_iter().filter(|origin_placement| {
                    !retained_origin_ids.contains(&origin_placement_identity(origin_placement))
                }));
            placement.reason = request
                .curation_note
                .clone()
                .unwrap_or(CurationRationale::new("Explicit Add to Pod")?);
            placement.updated_at = now;
            placement.audit_history.push(PlacementAuditEntry {
                status: PodPlacementStatus::Accepted,
                curation_path: CurationPath::AddToPod,
                actor,
                note: request.curation_note,
                occurred_at: now,
            });
            let placement = placement.clone();
            accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
            record_add_to_pod_learning(&mut store, ctx, &item, now);
            record_harness_write_at(
                &mut store,
                ctx,
                HarnessWriteOperation::AddContentItemToPod,
                Some(request.pod_id),
                now,
            );
            self.persist_locked(&mut store)?;
            return Ok(placement);
        }
        let candidate_id = CandidateId::from(stable_candidate_uuid(
            "add-to-pod",
            &[&request.content_item_id.to_string()],
        ));
        let candidate = store
            .candidates
            .entry(candidate_id)
            .or_insert_with(|| Candidate {
                id: candidate_id,
                tenant_id: item.tenant_id,
                source_url: item.url.clone(),
                canonical_url: item.canonical_url.clone(),
                review_state: CandidateReviewState::Accepted,
                created_at: now,
            })
            .clone();
        let actor = curation_actor(ctx);
        let placement = PodPlacement {
            candidate_id,
            pod_id: request.pod_id,
            content_item_id: Some(item.id.into()),
            reason: request
                .curation_note
                .clone()
                .unwrap_or(CurationRationale::new("Explicit Add to Pod")?),
            confidence: CandidateConfidence::new(1.0)
                .map_err(|error| StoreError::Validation(error.to_string()))?,
            source_submission_ids: Vec::new(),
            origin_placements,
            origin_withdrawals: Vec::new(),
            status: PodPlacementStatus::Accepted,
            curation_path: CurationPath::AddToPod,
            actor,
            audit_history: vec![PlacementAuditEntry {
                status: PodPlacementStatus::Accepted,
                curation_path: CurationPath::AddToPod,
                actor,
                note: request.curation_note,
                occurred_at: now,
            }],
            created_at: now,
            updated_at: now,
        };
        accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
        record_add_to_pod_learning(&mut store, ctx, &item, now);
        store
            .pod_placements
            .insert((candidate_id, request.pod_id), placement.clone());
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::AddContentItemToPod,
            Some(request.pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(placement)
    }

    /// Reverses one accepted local private-Pod placement without deleting its Content Item.
    ///
    /// # Errors
    ///
    /// Returns an error when the placement is not accepted, authorization is denied,
    /// the Pod is public or remote, the reason is empty, or persistence fails.
    pub fn reverse_pod_placement(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        pod_id: PodId,
        reason: CurationRationale,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPlacement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        let pod = store
            .pods
            .get(&pod_id)
            .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
        if pod.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public placement reversal requires a Pending Proposal".into(),
            )
            .into());
        }
        let actor = curation_actor(ctx);
        let placement = store
            .pod_placements
            .get_mut(&(candidate_id, pod_id))
            .ok_or_else(|| StoreError::NotFound("Pod Placement".into()))?;
        if placement.status != PodPlacementStatus::Accepted {
            return Err(StoreError::Validation("Pod Placement is not accepted".into()).into());
        }
        let content_item_id = placement.content_item_id.ok_or_else(|| {
            StoreError::Validation("Accepted Placement has no Content Item".into())
        })?;
        placement.status = PodPlacementStatus::Reversed;
        placement.curation_path = CurationPath::ManualReview;
        placement.actor = actor;
        placement.updated_at = now;
        placement.audit_history.push(PlacementAuditEntry {
            status: PodPlacementStatus::Reversed,
            curation_path: CurationPath::ManualReview,
            actor,
            note: Some(reason),
            occurred_at: now,
        });
        let placement = placement.clone();
        store.submission_pods.retain(|association| {
            !(association.pod_id == pod_id
                && association.submission_id == Uuid::from(content_item_id))
        });
        store
            .accepted_placement_projections
            .remove(&(content_item_id, pod_id));
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::ReviewCandidatePlacement,
            Some(pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(placement)
    }

    /// Removes one accepted Content Item placement using the Pod's visibility policy.
    ///
    /// Private and invite-only placements reverse immediately without deleting the
    /// Content Item. Public placements become Pending Proposals and emit their
    /// Placement Tombstone only when independently approved.
    pub fn request_remove_content_item_from_pod(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        content_item_id: ContentItemId,
        reason: CurationRationale,
        now: chrono::DateTime<Utc>,
    ) -> Result<RemoveContentItemOutcome, AgentToolsError> {
        let (pod, candidate_id) = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_local_pod_curation(&store, ctx, pod_id)?;
            let pod = store
                .pods
                .get(&pod_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound(format!("Pod {pod_id}")))?;
            let candidate_id = store
                .pod_placements
                .values()
                .find(|placement| {
                    placement.pod_id == pod_id
                        && placement.content_item_id == Some(content_item_id)
                        && placement.status == PodPlacementStatus::Accepted
                })
                .map(|placement| placement.candidate_id)
                .ok_or_else(|| StoreError::NotFound("Accepted Placement".into()))?;
            (pod, candidate_id)
        };
        if pod.visibility == Visibility::Public {
            let proposal = self.create_pending_proposal_from_request(
                ctx,
                CreatePendingProposalRequest {
                    requested_change: SensitiveChange::RemovePublicSubmissionFromPod {
                        pod_id,
                        submission_id: content_item_id.into(),
                    },
                    expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                },
                now,
            )?;
            return Ok(RemoveContentItemOutcome::PendingApproval {
                proposal: Box::new(proposal),
            });
        }
        self.reverse_pod_placement(ctx, candidate_id, pod_id, reason, now)
            .map(|placement| RemoveContentItemOutcome::Removed {
                placement: Box::new(placement),
            })
    }

    /// Remove a submission's association with a pod. If no pod references the
    /// submission afterward, the submission and its assets (plus any per-user
    /// saves/notes/history keyed on it) are purged too. Emits a signed
    /// `link_removed` event for the pod. Returns `true` if the submission itself
    /// was purged. Errors with `NotFound` if the link is not present in the pod.
    pub fn remove_submission_from_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        let store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(pod.id))?;
        if pod.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public-content removal requires a Pending Proposal".to_string(),
            )
            .into());
        }

        drop(store);
        self.remove_submission_from_pod_immediately(ctx, pod_slug, submission_id)
    }

    #[cfg(test)]
    pub(crate) fn remove_submission_from_pod_for_test(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        self.remove_submission_from_pod_immediately(ctx, pod_slug, submission_id)
    }

    pub(crate) fn remove_submission_from_pod_immediately(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
    ) -> Result<bool, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::PodCuration, Some(pod.id))?;

        let before = store.submission_pods.len();
        store
            .submission_pods
            .retain(|link| !(link.pod_id == pod.id && link.submission_id == submission_id));
        if store.submission_pods.len() == before {
            return Err(StoreError::NotFound(format!(
                "submission {submission_id} in pod {pod_slug}"
            ))
            .into());
        }

        // Purge the submission entirely once no pod references it anymore.
        let purged = !store
            .submission_pods
            .iter()
            .any(|link| link.submission_id == submission_id);
        if purged {
            store.submissions.remove(&submission_id);
            store
                .submission_assets
                .retain(|_, asset| asset.submission_id != submission_id);
            store.saves.retain(|(_, sid)| *sid != submission_id);
            store
                .reading_history
                .retain(|(_, sid)| *sid != submission_id);
            store
                .private_notes
                .retain(|(_, sid), _| *sid != submission_id);
        }

        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "link_removed",
            &pod.slug,
            json!({"submission_id": submission_id, "submission_purged": purged}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::RemoveSubmissionFromPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(purged)
    }

    /// Routes Pod Placement removal through the sensitive-change policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod or placement is missing, authorization is
    /// denied, proposal validation fails, or persistence fails.
    pub fn request_remove_submission_from_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        submission_id: SubmissionId,
        now: chrono::DateTime<Utc>,
    ) -> Result<RemoveSubmissionOutcome, AgentToolsError> {
        let pod = self.pod_by_slug(pod_slug, ctx.tenant_id)?;
        if pod.visibility == Visibility::Public {
            return self
                .create_pending_proposal_from_request(
                    ctx,
                    CreatePendingProposalRequest {
                        requested_change: SensitiveChange::RemovePublicSubmissionFromPod {
                            pod_id: pod.id,
                            submission_id,
                        },
                        expires_in_seconds: DEFAULT_PENDING_PROPOSAL_SECONDS,
                    },
                    now,
                )
                .map(|proposal| RemoveSubmissionOutcome::PendingApproval(Box::new(proposal)));
        }
        self.remove_submission_from_pod(ctx, pod_slug, submission_id)
            .map(|submission_purged| RemoveSubmissionOutcome::Removed { submission_purged })
    }

    pub fn add_submission_asset(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
        request: RepresentativeImageRequest,
    ) -> Result<SubmissionAsset, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        for pod_id in store
            .submission_pods
            .iter()
            .filter(|placement| placement.submission_id == submission_id)
            .map(|placement| placement.pod_id)
        {
            authorize_harness_pod_scope(&store, ctx, pod_id)?;
        }
        let pod_ids = store
            .submission_pods
            .iter()
            .filter(|placement| placement.submission_id == submission_id)
            .map(|placement| placement.pod_id)
            .collect::<Vec<_>>();
        for pod_id in &pod_ids {
            authorize_harness(
                &store,
                ctx,
                HarnessCapability::CandidateSubmission,
                Some(*pod_id),
            )?;
        }
        if request.url.is_none() && request.local_path.is_none() {
            return Err(StoreError::Validation(
                "representative image requires url or local_path".to_string(),
            )
            .into());
        }
        if let Some(existing) = store.submission_assets.values().find(|asset| {
            asset.submission_id == submission_id
                && asset.asset_type == SubmissionAssetType::RepresentativeImage
                && asset.source == request.source
                && asset.url == request.url
                && asset.local_path == request.local_path
        }) {
            return Ok(existing.clone());
        }
        let asset = SubmissionAsset {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            submission_id,
            asset_type: SubmissionAssetType::RepresentativeImage,
            source: request.source,
            url: request.url,
            local_path: request.local_path,
            mime_type: request.mime_type,
            alt_text: request.alt_text,
            created_at: Utc::now(),
        };
        store.submission_assets.insert(asset.id, asset.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::AddSubmissionAsset,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(asset)
    }

    pub fn assets_for_submission(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
    ) -> Result<Vec<SubmissionAsset>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        authorize_harness_submission_scope(&store, ctx, submission_id)?;
        Ok(store
            .submission_assets
            .values()
            .filter(|asset| asset.submission_id == submission_id)
            .cloned()
            .collect())
    }

    pub fn add_source_to_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        source_type: CrawlerSourceType,
        url: String,
    ) -> Result<CrawlerSource, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(&store, ctx, HarnessCapability::DiscoveryTasks, Some(pod.id))?;
        let source = CrawlerSource {
            id: Uuid::now_v7(),
            tenant_id: ctx.tenant_id,
            pod_id: pod.id,
            source_type,
            url,
            enabled: true,
            crawl_interval_minutes: 1440,
            last_crawled_at: None,
            origin_event_id: None,
        };
        store.crawler_sources.insert(source.id, source.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::AddSourceToPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(source)
    }

}
