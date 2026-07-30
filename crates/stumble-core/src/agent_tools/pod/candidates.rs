use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    pub fn submit_link_to_pod(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        request: SubmitLinkRequest,
    ) -> Result<Submission, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::CandidateSubmission,
            Some(pod.id),
        )?;
        let canonical_url = canonicalize_url(&request.url)?;
        let domain = Url::parse(&canonical_url)
            .map_err(|e| AgentToolsError::BadUrl(e.to_string()))?
            .domain()
            .unwrap_or("unknown")
            .to_string();
        let existing = store
            .submissions
            .values()
            .find(|s| s.canonical_url == canonical_url && s.tenant_id == ctx.tenant_id)
            .cloned();
        let submission = if let Some(existing) = existing {
            existing
        } else {
            let description = request.description.clone();
            let title = request.title.unwrap_or_else(|| canonical_url.clone());
            let submission = Submission {
                id: Uuid::now_v7(),
                tenant_id: ctx.tenant_id,
                url: request.url.clone(),
                canonical_url: canonical_url.clone(),
                title: title.clone(),
                source_metadata: CandidateSourceMetadata {
                    title: Some(title),
                    ..CandidateSourceMetadata::default()
                },
                description,
                domain,
                submitted_by: ctx.user_id,
                discovered_by_crawler: request.discovered_by_crawler,
                submitter_note: request.note,
                summary: request.description,
                provenance: Vec::new(),
                media_references: Vec::new(),
                tags: request.tags,
                embedding: None,
                created_at: Utc::now(),
                origin_event_id: None,
            };
            store.submissions.insert(submission.id, submission.clone());
            submission
        };
        if !store
            .submission_pods
            .iter()
            .any(|link| link.pod_id == pod.id && link.submission_id == submission.id)
        {
            store.submission_pods.push(SubmissionPod {
                submission_id: submission.id,
                pod_id: pod.id,
                created_at: Utc::now(),
            });
        }
        if pod.visibility != Visibility::Public {
            let node = store.node_for_tenant(ctx.tenant_id)?;
            let event = sign_public_event(
                &node,
                "link_submitted",
                &pod.slug,
                json!({"submission": submission.clone()}),
                store.latest_event_hash(&pod.slug),
            )?;
            store.event_log.push(event);
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::SubmitLinkToPod,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(submission)
    }

    /// Authenticates, validates, canonicalizes, and privately records external discovery evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization, task ownership, input validation,
    /// idempotency, canonicalization, persistence, or locking fails.
    pub fn submit_candidate(
        &self,
        ctx: &AuthContext,
        request: CandidateSubmissionRequest,
    ) -> Result<SubmittedCandidate, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let harness_id = ctx
            .harness_id
            .ok_or(AgentToolsError::CandidateHarnessRequired)?;
        let harness =
            harness_for_context(&store, ctx)?.ok_or(AgentToolsError::CandidateHarnessRequired)?;
        validate_candidate_submission(&store, ctx, &request)?;

        if let Some(existing) = idempotent_candidate_submission(&store, harness_id, &request)? {
            let candidate = store
                .candidates
                .get(&existing.candidate_id)
                .cloned()
                .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
            return Ok(SubmittedCandidate {
                candidate,
                submission: existing,
                allowed_actions: vec![CandidateAllowedAction::InspectCandidate],
            });
        }
        validate_candidate_task_context(&store, ctx, harness, &request)?;

        let mut projected = store.clone();

        let canonical_url = canonicalize_url(&request.evidence.source_url)?;
        let candidate = projected
            .candidates
            .values()
            .find(|candidate| {
                candidate.tenant_id == ctx.tenant_id && candidate.canonical_url == canonical_url
            })
            .cloned()
            .unwrap_or_else(|| Candidate {
                id: stable_candidate_uuid(
                    "candidate",
                    &[
                        &ctx.tenant_id
                            .map_or_else(|| "local".into(), |id| id.to_string()),
                        &canonical_url,
                    ],
                )
                .into(),
                tenant_id: ctx.tenant_id,
                source_url: canonical_url.clone(),
                canonical_url,
                review_state: CandidateReviewState::Pending,
                created_at: Utc::now(),
            });
        projected.candidates.insert(candidate.id, candidate.clone());

        let target = match &request.target {
            CandidateSubmissionRequestTarget::User {
                learn,
                interest_seed_metadata,
            } => CandidateSubmissionTarget::User {
                user_id: ctx
                    .user_id
                    .expect("User submission operation was validated"),
                learn: *learn,
                interest_seed_metadata: interest_seed_metadata.clone(),
            },
            CandidateSubmissionRequestTarget::PodPlacements {
                placements,
                task_context,
            } => CandidateSubmissionTarget::PodPlacements {
                placements: placements.clone(),
                task_context: *task_context,
            },
            CandidateSubmissionRequestTarget::PersonalDiscovery {
                task_id,
                allocation_role,
                source_facts,
            } => {
                let task = projected
                    .discovery_tasks
                    .get(task_id)
                    .expect("Personal Discovery task was validated");
                let plan_id = task
                    .target
                    .discovery_plan_id()
                    .expect("Personal Discovery task carries a plan");
                let plan = projected
                    .discovery_plans
                    .get(&plan_id)
                    .expect("Personal Discovery plan was validated");
                CandidateSubmissionTarget::PersonalDiscovery {
                    user_id: plan.user_id,
                    task_id: *task_id,
                    discovery_plan_id: plan_id,
                    allocation_role: *allocation_role,
                    source_facts: source_facts.clone(),
                }
            }
        };
        let submission = CandidateSubmission {
            id: stable_candidate_uuid(
                "candidate-submission",
                &[
                    &harness_id.to_string(),
                    &request.evidence.harness_idempotency_key,
                    &request.evidence.client_idempotency_key,
                ],
            )
            .into(),
            candidate_id: candidate.id,
            tenant_id: ctx.tenant_id,
            submitted_by: harness_id,
            target,
            evidence: request.evidence,
            created_at: Utc::now(),
        };
        projected
            .candidate_submissions
            .insert(submission.id, submission.clone());
        if submission.target.learning_enabled() {
            record_interest_seed(&mut projected, &candidate, &submission);
        }
        enrich_accepted_content_item(&mut projected, ctx, &candidate)?;
        record_harness_write(
            &mut projected,
            ctx,
            HarnessWriteOperation::SubmitCandidate,
            None,
        );
        self.persist_locked(&mut projected)?;
        *store = projected;
        Ok(SubmittedCandidate {
            candidate,
            submission,
            allowed_actions: vec![CandidateAllowedAction::InspectCandidate],
        })
    }

    /// Lists private Candidates visible within the active Harness Grant.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization state cannot be read.
    pub fn list_candidates(&self, ctx: &AuthContext) -> Result<Vec<Candidate>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let harness = harness_for_context(&store, ctx)?;
        Ok(store
            .candidates
            .values()
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
            .filter(|candidate| {
                store.candidate_submissions.values().any(|submission| {
                    submission.candidate_id == candidate.id
                        && candidate_submission_is_visible(&store, ctx, harness, submission)
                })
            })
            .cloned()
            .collect())
    }

    /// Lists visible Candidates with their merged summary-rich source reference.
    ///
    /// # Errors
    ///
    /// Returns an error when authorization state cannot be read.
    pub fn list_candidate_references(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<CandidateListItem>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let harness = harness_for_context(&store, ctx)?;
        let mut candidates = Vec::new();
        for candidate in store
            .candidates
            .values()
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
        {
            let visible_submissions = store
                .candidate_submissions
                .values()
                .filter(|submission| submission.candidate_id == candidate.id)
                .filter(|submission| {
                    candidate_submission_is_visible(&store, ctx, harness, submission)
                });
            if let Some(reference) = CandidateReference::from_submissions(visible_submissions) {
                candidates.push(CandidateListItem {
                    candidate: candidate.clone(),
                    reference,
                });
            }
        }
        Ok(candidates)
    }

    /// Inspects a private Candidate and all independently retained evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when the Candidate is missing, outside the caller's
    /// tenant or Pod scope, or the Home Node lock is poisoned.
    pub fn inspect_candidate(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
    ) -> Result<CandidateInspection, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        let harness = harness_for_context(&store, ctx)?;
        let mut submissions: Vec<_> = store
            .candidate_submissions
            .values()
            .filter(|submission| {
                submission.candidate_id == candidate_id
                    && candidate_submission_is_visible(&store, ctx, harness, submission)
            })
            .cloned()
            .collect();
        if submissions.is_empty() {
            return Err(AgentToolsError::Forbidden {
                reason: "Harness Grant cannot inspect this Candidate evidence".into(),
            });
        }
        submissions.sort_by_key(|submission| (submission.created_at, submission.id));
        let reference = CandidateReference::from_submissions(&submissions)
            .expect("visible Candidate inspection has at least one submission");
        let mut placements: Vec<_> = store
            .pod_placements
            .values()
            .filter(|placement| {
                placement.candidate_id == candidate_id
                    && candidate_placement_is_visible(&store, ctx, placement.pod_id)
            })
            .cloned()
            .collect();
        placements.sort_by_key(|placement| placement.pod_id);
        let proposal_pod_ids: HashSet<_> = submissions
            .iter()
            .flat_map(|submission| submission.target.placements())
            .map(|placement| placement.pod_id)
            .collect();
        let can_curate_all = harness.is_some()
            && proposal_pod_ids
                .iter()
                .all(|pod_id| authorize_local_pod_curation(&store, ctx, *pod_id).is_ok());
        let mut allowed_actions = Vec::new();
        let can_submit_evidence = !submissions.is_empty()
            && submissions
                .iter()
                .all(|submission| match submission.target {
                    CandidateSubmissionTarget::User { user_id, .. } => {
                        harness.is_some_and(|harness| {
                            harness.kind == AgentHarnessKind::Interactive
                                && harness.grant.pod_ids.is_none()
                                && ctx.user_id == Some(user_id)
                        })
                    }
                    CandidateSubmissionTarget::PodPlacements { ref placements, .. } => {
                        placements.iter().all(|placement| {
                            authorize_harness(
                                &store,
                                ctx,
                                HarnessCapability::CandidateSubmission,
                                Some(placement.pod_id),
                            )
                            .is_ok()
                        })
                    }
                    CandidateSubmissionTarget::PersonalDiscovery { .. } => false,
                });
        if harness.is_some() && can_submit_evidence {
            allowed_actions.push(CandidateAllowedAction::SubmitCandidateEvidence);
        }
        if can_curate_all && !proposal_pod_ids.is_empty() {
            allowed_actions.push(CandidateAllowedAction::EvaluateCandidate);
        }
        if harness.is_some()
            && store
                .pods
                .values()
                .any(|pod| authorize_local_pod_curation(&store, ctx, pod.id).is_ok())
        {
            allowed_actions.push(CandidateAllowedAction::RouteCandidatePlacement);
        }
        if harness.is_some()
            && placements.iter().any(|placement| {
                placement.status == PodPlacementStatus::Pending
                    && authorize_local_pod_curation(&store, ctx, placement.pod_id).is_ok()
            })
        {
            allowed_actions.push(CandidateAllowedAction::ReviewCandidatePlacement);
        }
        Ok(CandidateInspection {
            candidate,
            reference,
            submissions,
            placements,
            allowed_actions,
        })
    }

    /// Evaluates every proposed Pod Placement for a private Candidate independently.
    ///
    /// # Errors
    ///
    /// Returns an error when the Candidate is missing, a placement is outside the
    /// caller's local curation scope, evidence is inconsistent, or persistence fails.
    pub fn curate_candidate(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        now: chrono::DateTime<Utc>,
    ) -> Result<CandidateCurationResult, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        let submissions = candidate_submissions_for(&store, candidate_id);
        let proposals = merged_candidate_proposals(&submissions)?;
        if proposals.is_empty() {
            return Err(
                StoreError::Validation("Candidate has no Pod Placement evidence".into()).into(),
            );
        }
        for proposal in &proposals {
            authorize_local_pod_curation(&store, ctx, proposal.pod_id)?;
        }

        for proposal in proposals {
            if store
                .pod_placements
                .contains_key(&(candidate_id, proposal.pod_id))
            {
                continue;
            }
            let policy = store
                .pod_curation_policies
                .get(&proposal.pod_id)
                .copied()
                .unwrap_or_default();
            let trusted_confidence =
                trusted_placement_confidence(&store, &submissions, proposal.pod_id);
            let automatic_path = match policy {
                CurationPolicy::Manual => None,
                CurationPolicy::Assisted {
                    confidence_threshold,
                } if trusted_confidence.is_some_and(|confidence| {
                    confidence.value() >= confidence_threshold.value()
                }) =>
                {
                    Some(CurationPath::AssistedAutomatic)
                }
                CurationPolicy::Autonomous {
                    confidence_threshold,
                } if proposal.confidence.value() >= confidence_threshold.value() => {
                    Some(CurationPath::AutonomousAutomatic)
                }
                CurationPolicy::Assisted { .. } | CurationPolicy::Autonomous { .. } => None,
            };
            let actor = curation_actor(ctx);
            let status = automatic_path
                .map(|_| PodPlacementStatus::Accepted)
                .unwrap_or(PodPlacementStatus::Pending);
            let curation_path = automatic_path.unwrap_or(CurationPath::CandidateProposal);
            let content_item_id = if status == PodPlacementStatus::Accepted {
                Some(
                    ensure_content_item(
                        &mut store,
                        &candidate,
                        &submissions,
                        &proposal.source_submission_ids,
                        now,
                    )?
                    .id(),
                )
            } else {
                None
            };
            let placement = PodPlacement {
                candidate_id,
                pod_id: proposal.pod_id,
                content_item_id,
                reason: proposal.reason,
                confidence: proposal.confidence,
                source_submission_ids: proposal.source_submission_ids,
                origin_placements: Vec::new(),
                origin_withdrawals: Vec::new(),
                status,
                curation_path,
                actor,
                audit_history: vec![PlacementAuditEntry {
                    status,
                    curation_path,
                    actor,
                    note: None,
                    occurred_at: now,
                }],
                created_at: now,
                updated_at: now,
            };
            if status == PodPlacementStatus::Accepted {
                accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
                enrich_accepted_content_item(&mut store, ctx, &candidate)?;
            }
            store
                .pod_placements
                .insert((candidate_id, proposal.pod_id), placement);
        }
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::CurateCandidate,
            None,
            now,
        );
        self.persist_locked(&mut store)?;
        candidate_curation_result(&store, candidate_id)
    }

    /// Applies an authorized manual decision to one pending Pod Placement.
    ///
    /// # Errors
    ///
    /// Returns an error for missing or non-pending placements, unauthorized or
    /// remote Pods, empty notes, inconsistent evidence, or persistence failure.
    pub fn review_candidate_placement(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        pod_id: PodId,
        decision: PlacementReviewDecision,
        note: Option<CurationRationale>,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPlacement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, pod_id)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        let submissions = candidate_submissions_for(&store, candidate_id);
        let current = store
            .pod_placements
            .get(&(candidate_id, pod_id))
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Pod Placement".into()))?;
        if current.status != PodPlacementStatus::Pending {
            return Err(
                StoreError::Validation("Pod Placement is not pending review".into()).into(),
            );
        }
        let status = match decision {
            PlacementReviewDecision::Accept => PodPlacementStatus::Accepted,
            PlacementReviewDecision::Reject => PodPlacementStatus::Rejected,
        };
        let actor = curation_actor(ctx);
        let content_item_id = if status == PodPlacementStatus::Accepted {
            Some(
                ensure_content_item(
                    &mut store,
                    &candidate,
                    &submissions,
                    &current.source_submission_ids,
                    now,
                )?
                .id(),
            )
        } else {
            None
        };
        let placement = store
            .pod_placements
            .get_mut(&(candidate_id, pod_id))
            .ok_or_else(|| StoreError::NotFound("Pod Placement".into()))?;
        placement.status = status;
        placement.content_item_id = content_item_id;
        placement.curation_path = CurationPath::ManualReview;
        placement.actor = actor;
        placement.updated_at = now;
        placement.audit_history.push(PlacementAuditEntry {
            status,
            curation_path: CurationPath::ManualReview,
            actor,
            note,
            occurred_at: now,
        });
        let placement = placement.clone();
        if status == PodPlacementStatus::Accepted {
            accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
            enrich_accepted_content_item(&mut store, ctx, &candidate)?;
        }
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

    /// Records an evidence-backed Routing Agent proposal for an authorized local Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when the Candidate or Pod is missing, the Pod is remote or
    /// outside the Harness Grant, evidence is empty, or persistence fails.
    pub fn route_candidate_placement(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
        request: RouteCandidatePlacementRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPlacement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_local_pod_curation(&store, ctx, request.pod_id)?;
        let harness = harness_for_context(&store, ctx)?;
        let candidate = store
            .candidates
            .get(&candidate_id)
            .filter(|candidate| candidate.tenant_id == ctx.tenant_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("Candidate".into()))?;
        if let Some(existing) = store.pod_placements.get(&(candidate_id, request.pod_id)) {
            return Ok(existing.clone());
        }
        let actor = curation_actor(ctx);
        let policy = store
            .pod_curation_policies
            .get(&request.pod_id)
            .copied()
            .unwrap_or_default();
        let accepted = matches!(
            policy,
            CurationPolicy::Autonomous {
                confidence_threshold
            } if request.confidence.value() >= confidence_threshold.value()
        );
        let status = if accepted {
            PodPlacementStatus::Accepted
        } else {
            PodPlacementStatus::Pending
        };
        let curation_path = if accepted {
            CurationPath::AutonomousAutomatic
        } else {
            CurationPath::RoutingAgent
        };
        let submissions = candidate_submissions_for(&store, candidate_id)
            .into_iter()
            .filter(|submission| {
                matches!(
                    submission.target,
                    CandidateSubmissionTarget::PodPlacements { .. }
                ) && candidate_submission_is_visible(&store, ctx, harness, submission)
            })
            .collect::<Vec<_>>();
        let source_submission_ids = submissions
            .iter()
            .map(|submission| submission.id)
            .collect::<Vec<_>>();
        let content_item_id = if accepted {
            Some(
                ensure_content_item(
                    &mut store,
                    &candidate,
                    &submissions,
                    &source_submission_ids,
                    now,
                )?
                .id(),
            )
        } else {
            None
        };
        let placement = PodPlacement {
            candidate_id,
            pod_id: request.pod_id,
            content_item_id,
            reason: request.reason,
            confidence: request.confidence,
            source_submission_ids,
            origin_placements: Vec::new(),
            origin_withdrawals: Vec::new(),
            status,
            curation_path,
            actor,
            audit_history: vec![PlacementAuditEntry {
                status,
                curation_path,
                actor,
                note: None,
                occurred_at: now,
            }],
            created_at: now,
            updated_at: now,
        };
        if accepted {
            accept_candidate_placement(&mut store, ctx, &candidate, &placement)?;
            enrich_accepted_content_item(&mut store, ctx, &candidate)?;
        }
        store
            .pod_placements
            .insert((candidate_id, request.pod_id), placement.clone());
        record_harness_write_at(
            &mut store,
            ctx,
            HarnessWriteOperation::RouteCandidatePlacement,
            Some(request.pod_id),
            now,
        );
        self.persist_locked(&mut store)?;
        Ok(placement)
    }
}
