use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Produces bounded Origin-signed Content Reference samples for Explore.
    ///
    /// The sample artifact is separate from Pod synchronization and does not
    /// create or require a Subscription.
    ///
    /// # Errors
    ///
    /// Returns an error when the announcement is invalid or stale, the Pod is
    /// not locally authoritative and public, the limit exceeds ten, signing
    /// fails, or state is unavailable.
    pub fn pod_explore_samples(
        &self,
        ctx: &AuthContext,
        announcement: &PodAnnouncement,
        limit: usize,
    ) -> Result<PodExploreSamples, AgentToolsError> {
        if limit > 10 {
            return Err(StoreError::Validation(
                "Pod Explore sample limit must not exceed 10".into(),
            )
            .into());
        }
        if !announcement.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        if announcement.origin_node_id != node.id
            || announcement.signer.public_key != node.public_key
        {
            return Err(StoreError::Validation(
                "Explore samples must be produced by the Pod's Origin Node".into(),
            )
            .into());
        }
        let pod = store.pod_by_slug(&announcement.pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        if pod.visibility != Visibility::Public
            || pod.origin_node_id.is_some_and(|origin| origin != node.id)
        {
            return Err(
                StoreError::NotFound(format!("public Pod {}", announcement.pod_slug)).into(),
            );
        }
        let package = store
            .pod_skill_packs
            .get(&pod.id)
            .ok_or_else(|| StoreError::NotFound("Pod Package".into()))?;
        let package_version = PackageVersion::new(package.version)
            .map_err(|error| StoreError::Validation(error.to_string()))?;
        if announcement.pod_name != pod.name
            || announcement.subject != pod.description
            || announcement.package_version != package_version
            || announcement.latest_event_hash != store.latest_federated_event_hash(&pod.slug)
        {
            return Err(StoreError::Validation(
                "Explore samples require the current Pod Announcement".into(),
            )
            .into());
        }
        let empty_policy = TrustPolicy::new(Uuid::nil(), ctx.tenant_id);
        let samples = explore_content_samples(
            &store,
            ctx.tenant_id,
            node.id,
            announcement,
            &empty_policy,
            limit,
        );
        sign_pod_explore_samples(
            &node,
            PodExploreSamples {
                id: Uuid::now_v7(),
                announcement_id: announcement.id,
                origin_node_id: node.id,
                signer: announcement.signer.clone(),
                pod_slug: announcement.pod_slug.clone(),
                samples,
                sampled_at: Utc::now(),
                signature: String::new(),
            },
        )
        .map_err(Into::into)
    }

    /// Aggregates a verified announcement on an optional, non-authoritative Index Node.
    ///
    /// # Errors
    ///
    /// Returns an error when the signature or direct address is invalid, the
    /// announcement is stale or expired, the Pod is withdrawn, or persistence fails.
    pub fn index_pod_announcement(
        &self,
        announcement: PodAnnouncement,
    ) -> Result<KnownPodAnnouncement, AgentToolsError> {
        self.index_pod_announcement_at(announcement, Utc::now())
    }

    /// Indexes a verified announcement at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::index_pod_announcement`].
    pub fn index_pod_announcement_at(
        &self,
        announcement: PodAnnouncement,
        now: chrono::DateTime<Utc>,
    ) -> Result<KnownPodAnnouncement, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let known = retain_verified_pod_announcement(
            &mut store,
            announcement,
            DeliveryProvenance::LOCAL,
            now,
        )?;
        self.persist_locked(&mut store)?;
        Ok(known)
    }

    /// Aggregates a verified Pod Withdrawal on an optional Index Node.
    ///
    /// # Errors
    ///
    /// Returns an error when the signature is invalid, the withdrawal is stale,
    /// or persistence fails.
    pub fn index_pod_withdrawal(
        &self,
        withdrawal: PodWithdrawal,
    ) -> Result<KnownPodWithdrawal, AgentToolsError> {
        self.index_pod_withdrawal_at(withdrawal, Utc::now())
    }

    /// Indexes a verified Pod Withdrawal at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::index_pod_withdrawal`].
    pub fn index_pod_withdrawal_at(
        &self,
        withdrawal: PodWithdrawal,
        now: chrono::DateTime<Utc>,
    ) -> Result<KnownPodWithdrawal, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let known = retain_verified_pod_withdrawal(&mut store, withdrawal, None, now)?;
        // Keep Bootstrap stream closed under co-located Index/peer withdraw retain.
        project_bootstrap_withdrawal(&mut store, &known.withdrawal, now);
        self.persist_locked(&mut store)?;
        Ok(known)
    }

    /// Searches verified announcements held by this Index-capable node.
    ///
    /// Relevance reflects only the caller's explicit query and never represents
    /// global Pod quality, trust, popularity, or personalized authority.
    /// Requires no User account. Does not retain product analytics.
    ///
    /// # Errors
    ///
    /// Returns a typed [`IndexSearchFailure`] when Index is disabled, the query
    /// is oversized/malformed, rate-limited, or local state is unavailable.
    pub fn search_pod_announcements(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<PodAnnouncementSearchResponse, AgentToolsError> {
        self.search_pod_announcements_at(&IndexSearchRequest::new(query, Some(limit)), Utc::now())
    }

    /// Index catalog search at an explicit clock time (tests / deterministic clocks).
    ///
    /// # Errors
    ///
    /// Same as [`Self::search_pod_announcements`].
    pub fn search_pod_announcements_at(
        &self,
        request: &IndexSearchRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodAnnouncementSearchResponse, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let response = search_index_catalog(&mut store, request, self.index.enabled, now)?;
        // Persist rate-limit timestamps only (no query text / User id).
        self.persist_locked(&mut store)?;
        Ok(response)
    }

    /// Accepts verified results fetched from one configured optional Index Node.
    ///
    /// The Index Node's relevance is discarded; Explore recomputes ordering
    /// under the User's local Trust Policy. Provenance records the Index URL.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the Index Node is not in
    /// the User's Trust Policy, any announcement is invalid, or persistence fails.
    pub fn accept_index_search_results(
        &self,
        ctx: &AuthContext,
        index_base_url: &str,
        response: PodAnnouncementSearchResponse,
    ) -> Result<usize, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Index Node results require an authenticated User".into())
        })?;
        let index_base_url = normalized_url(validate_public_base_url(index_base_url, "base_url")?);
        let policy = store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .ok_or_else(|| StoreError::Validation("Index Node is not configured".into()))?;
        if !policy.retains_index_url(&index_base_url) {
            return Err(StoreError::Validation("Index Node is not configured".into()).into());
        }
        let retained =
            retain_index_search_results(&mut store, &index_base_url, response, Utc::now())?;
        self.persist_locked(&mut store)?;
        Ok(retained)
    }

    /// Explicit Explore path: query configured Index Nodes with the User-authored
    /// query string only, verify/import results, then rank locally.
    ///
    /// Never called from Taste Profile, Source Affinity, Subscription, feedback,
    /// or Discovery Plan inference. Empty queries skip remote Index contact.
    /// Remote ordering is discarded; local Trust Policy and similarity recompute
    /// the Explore result order.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, no User is authenticated,
    /// the request is out of range, or local state is unavailable. Per-Index
    /// transport failures are recorded on the import report without failing the
    /// whole Explore when at least local ranking remains possible.
    pub fn explore_public_pods_with_indexes(
        &self,
        ctx: &AuthContext,
        request: ExploreRequest,
        client: &dyn IndexSearchClient,
    ) -> Result<ExploreResponse, AgentToolsError> {
        if !(1..=50).contains(&request.limit) || request.sample_size > 10 {
            return Err(ExploreRequestError.into());
        }
        // Import remote hits under a short write section, then rank from store.
        {
            let mut store = self
                .store
                .write()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
            let user_id = ctx.user_id.ok_or_else(|| {
                StoreError::Validation("Explore requires an authenticated User".into())
            })?;
            let policy = store
                .trust_policies
                .get(&(user_id, ctx.tenant_id))
                .cloned()
                .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
            let now = Utc::now();
            import_from_configured_indexes(
                &mut store,
                &policy,
                &request.query,
                request.limit,
                client,
                now,
            )?;
            self.persist_locked(&mut store)?;
        }
        self.explore_public_pods(ctx, request)
    }

    /// Queries configured Index Nodes for an explicit Explore query without ranking.
    ///
    /// # Errors
    ///
    /// Same authorization and Trust Policy rules as
    /// [`Self::accept_index_search_results`].
    pub fn import_explicit_index_search(
        &self,
        ctx: &AuthContext,
        query: &str,
        limit: usize,
        client: &dyn IndexSearchClient,
    ) -> Result<IndexExploreImportReport, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Index Node search requires an authenticated User".into())
        })?;
        let policy = store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .cloned()
            .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
        let report =
            import_from_configured_indexes(&mut store, &policy, query, limit, client, Utc::now())?;
        self.persist_locked(&mut store)?;
        Ok(report)
    }

    /// Retains Origin-signed remote samples for a known current announcement.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the announcement is unknown
    /// or stale, the Origin signature is invalid, the artifact exceeds ten
    /// samples, or persistence fails.
    pub fn accept_pod_explore_samples(
        &self,
        ctx: &AuthContext,
        samples: PodExploreSamples,
    ) -> Result<PodExploreSamples, AgentToolsError> {
        if samples.samples.len() > 10 {
            return Err(StoreError::Validation(
                "Pod Explore sample artifact must not exceed 10 references".into(),
            )
            .into());
        }
        if !samples.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let known = store
            .known_pod_announcements
            .get(&(samples.origin_node_id, samples.pod_slug.clone()))
            .ok_or_else(|| StoreError::NotFound("current Pod Announcement".into()))?;
        verify_explore_samples_for_announcement(&samples, &known.announcement)?;
        store
            .pod_explore_sample_sets
            .insert(samples.announcement_id, samples.clone());
        self.persist_locked(&mut store)?;
        Ok(samples)
    }

    /// Retrieves bounded Explore samples from the canonical Origin and retains
    /// them only when signature and current announcement binding verify.
    ///
    /// The injectable client performs the outbound Origin fetch. Requests carry
    /// only announcement identity and a sample limit—never private interests.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, the announcement is unknown
    /// or ineligible, the client fails, verification fails, or persistence fails.
    pub fn fetch_origin_explore_samples(
        &self,
        ctx: &AuthContext,
        origin_node_id: NodeIdentityId,
        pod_slug: &str,
        limit: usize,
        client: &dyn OriginExploreSampleClient,
    ) -> Result<PodExploreSamples, AgentToolsError> {
        let announcement = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
            let known = store
                .known_pod_announcements
                .get(&(origin_node_id, pod_slug.to_string()))
                .ok_or_else(|| StoreError::NotFound("current Pod Announcement".into()))?;
            let now = Utc::now();
            if !announcement_is_discovery_eligible(&store, &known.announcement, now) {
                return Err(StoreError::Validation(
                    "Pod Announcement is not discovery-eligible".into(),
                )
                .into());
            }
            if !known.announcement.verify()? {
                return Err(StoreError::InvalidSignature.into());
            }
            known.announcement.clone()
        };
        // Fetch outside the store lock; never attach private matching context.
        let samples = fetch_verified_origin_explore_samples(client, &announcement, limit)?;
        self.accept_pod_explore_samples(ctx, samples)
    }

    /// Signs an optional recommendation from one locally curated public Pod.
    ///
    /// # Errors
    ///
    /// Returns an error when curation is denied, either Pod identity is invalid,
    /// the reason is empty, signing fails, or persistence fails.
    pub fn endorse_public_pod(
        &self,
        ctx: &AuthContext,
        endorsing: &PodAnnouncement,
        endorsed: &PodAnnouncement,
        reason: String,
    ) -> Result<PodEndorsement, AgentToolsError> {
        if reason.trim().is_empty() {
            return Err(
                StoreError::Validation("Pod Endorsement reason must not be empty".into()).into(),
            );
        }
        if !endorsing.verify()? || !endorsed.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let endorsing_pod = store
            .pod_by_slug(&endorsing.pod_slug, ctx.tenant_id)?
            .clone();
        authorize_local_pod_curation(&store, ctx, endorsing_pod.id)?;
        if endorsing_pod.visibility != Visibility::Public {
            return Err(
                StoreError::Validation("only a public Pod can endorse another Pod".into()).into(),
            );
        }
        let node = store.node_for_tenant(ctx.tenant_id)?;
        if endorsing_pod
            .origin_node_id
            .is_some_and(|origin_node_id| origin_node_id != node.id)
        {
            return Err(StoreError::Validation(
                "only an Origin Node can sign its Pod Endorsement".into(),
            )
            .into());
        }
        if endorsing.origin_node_id != node.id || endorsing.pod_slug != endorsing_pod.slug {
            return Err(StoreError::Validation(
                "endorsing announcement does not identify the local public Pod".into(),
            )
            .into());
        }
        let endorsement = sign_pod_endorsement(
            &node,
            PodEndorsement {
                id: Uuid::now_v7(),
                endorsing_node_id: node.id,
                signer: NodeInfo {
                    node_id: node.id,
                    display_name: node.display_name.clone(),
                    public_key: node.public_key.clone(),
                    supported_protocol_version: CURRENT_PROTOCOL_VERSION.into(),
                },
                endorsing_pod_slug: endorsing_pod.slug,
                endorsing_announcement_id: endorsing.id,
                endorsed_node_id: endorsed.origin_node_id,
                endorsed_pod_slug: endorsed.pod_slug.clone(),
                endorsed_announcement_id: endorsed.id,
                reason: reason.trim().to_string(),
                endorsed_at: Utc::now(),
                signature: String::new(),
            },
        )?;
        store
            .pod_endorsements
            .insert(endorsement.id, endorsement.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::EndorsePublicPod,
            Some(endorsing_pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(endorsement)
    }

    /// Aggregates a valid Pod Endorsement without treating it as authority.
    ///
    /// # Errors
    ///
    /// Returns an error when the endorsement signature is invalid or persistence fails.
    pub fn index_pod_endorsement(
        &self,
        endorsement: PodEndorsement,
    ) -> Result<PodEndorsement, AgentToolsError> {
        if !endorsement.verify()? {
            return Err(StoreError::InvalidSignature.into());
        }
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let endorsing_is_known = store
            .known_pod_announcements
            .get(&(
                endorsement.endorsing_node_id,
                endorsement.endorsing_pod_slug.clone(),
            ))
            .is_some_and(|known| {
                known.announcement.id == endorsement.endorsing_announcement_id
                    && known.announcement.signer.public_key == endorsement.signer.public_key
            });
        let endorsed_is_known = store
            .known_pod_announcements
            .get(&(
                endorsement.endorsed_node_id,
                endorsement.endorsed_pod_slug.clone(),
            ))
            .is_some_and(|known| known.announcement.id == endorsement.endorsed_announcement_id);
        if !endorsing_is_known || !endorsed_is_known {
            return Err(StoreError::Validation(
                "Pod Endorsement must bind two known current Pod Announcements".into(),
            )
            .into());
        }
        if store
            .pod_endorsements
            .get(&endorsement.id)
            .is_some_and(|existing| existing != &endorsement)
        {
            return Err(
                StoreError::Duplicate(format!("Pod Endorsement {}", endorsement.id)).into(),
            );
        }
        store
            .pod_endorsements
            .insert(endorsement.id, endorsement.clone());
        self.persist_locked(&mut store)?;
        Ok(endorsement)
    }

    /// Submits bounded, confidence-scored local agent semantic evidence between
    /// two exact current Pod Announcements.
    ///
    /// Evidence adjusts inspectable local Pod Similarity ordering under Core
    /// policy only. It never creates trust, Subscription, Accepted Placement,
    /// or Feed eligibility, and never leaves the Home Node as an Endorsement,
    /// global score, announcement field, or remote interest query.
    ///
    /// # Errors
    ///
    /// Returns an error when the harness lacks [`HarnessCapability::PodSimilarityEvidence`],
    /// announcements are stale/withdrawn/expired/blocked/mismatched/unverifiable,
    /// bounds fail, or persistence fails.
    pub fn submit_pod_similarity_agent_evidence(
        &self,
        ctx: &AuthContext,
        request: SubmitPodSimilarityAgentEvidenceRequest,
    ) -> Result<PodSimilarityAgentEvidence, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::PodSimilarityEvidence, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation(
                "agent Pod Similarity evidence requires an authenticated User".into(),
            )
        })?;
        let harness = harness_for_context(&store, ctx)?.ok_or(AgentToolsError::Forbidden {
            reason: "agent Pod Similarity evidence requires an authenticated Agent Harness".into(),
        })?;
        let harness_id = harness.id;
        let policy = store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .cloned()
            .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
        let now = Utc::now();

        if let Some(existing) = find_idempotent_agent_evidence(
            &store,
            user_id,
            ctx.tenant_id,
            harness_id,
            request.harness_idempotency_key.trim(),
        ) {
            return Ok(existing.clone());
        }

        let (pair, left, right, public_inputs, freshness) = validate_agent_evidence_submission(
            &store,
            &request,
            user_id,
            ctx.tenant_id,
            Some(harness),
            &policy,
            now,
        )
        .map_err(agent_evidence_error_to_tools)?;

        // Bound active evidence by pair + model/harness provenance + freshness:
        // replace any prior active record for the same bound key.
        if let Some(prior) = find_bounded_agent_evidence_for_pair(
            &store,
            user_id,
            ctx.tenant_id,
            harness_id,
            request.model_provenance.trim(),
            pair,
            now,
        ) {
            let prior_id = prior.id;
            store.pod_similarity_agent_evidence.remove(&prior_id);
        }

        let evidence = build_agent_evidence_record(
            &request,
            user_id,
            ctx.tenant_id,
            harness_id,
            left,
            right,
            public_inputs,
            now,
            freshness,
        );
        store
            .pod_similarity_agent_evidence
            .insert(evidence.id, evidence.clone());
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::SubmitPodSimilarityAgentEvidence,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(evidence)
    }

    /// Intentionally discovers public Pods under the User's local Trust Policy.
    ///
    /// Explore does not create Subscriptions. Deterministic Pod Similarity uses
    /// verified public subject/context text, source neighborhoods, Explore
    /// samples, and optional Endorsements. Authorized local agent evidence may
    /// adjust ordering with inspectable reasons after deterministic scoring;
    /// Core still applies blocks and caps. Scoring is local from synchronized
    /// metadata and private evidence; it never issues interest-derived remote
    /// queries. Endorsements and agent evidence strengthen ranking only and are
    /// never transferable trust or global reputation.
    ///
    /// # Errors
    ///
    /// Returns an error when Feed reads are denied, no User is authenticated,
    /// the request is out of range, or local state is unavailable.
    pub fn explore_public_pods(
        &self,
        ctx: &AuthContext,
        request: ExploreRequest,
    ) -> Result<ExploreResponse, AgentToolsError> {
        if !(1..=50).contains(&request.limit) || request.sample_size > 10 {
            return Err(ExploreRequestError.into());
        }
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::FeedRead, None)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Explore requires an authenticated User".into())
        })?;
        let policy = store
            .trust_policies
            .get(&(user_id, ctx.tenant_id))
            .cloned()
            .unwrap_or_else(|| TrustPolicy::new(user_id, ctx.tenant_id));
        let local_node_id = store.node_for_tenant(ctx.tenant_id)?.id;
        let query = request.query.trim().to_lowercase();
        let preferences = store.user_preferences.get(&(user_id, ctx.tenant_id));
        let projections = taste_profile_projections(&store, user_id, ctx.tenant_id, preferences);
        // Private evidence stays in-process for ranking only; never sent remotely.
        let local =
            local_similarity_context_from_store(Some(query.as_str()), preferences, &projections);
        let now = Utc::now();

        // Materialize owned candidate evidence so ranking can borrow it safely.
        let mut owned: Vec<OwnedCandidateEvidence> = Vec::new();
        let mut agent_evidence_by_announcement: HashMap<Uuid, Vec<&PodSimilarityAgentEvidence>> =
            HashMap::new();
        for known in store.known_pod_announcements.values() {
            let announcement = &known.announcement;
            if !announcement_scoring_eligible(&store, known, &policy, now) {
                continue;
            }
            let endorsements = collect_policy_endorsements(&store, announcement, &policy);
            let (samples, samples_verified) = retained_or_local_explore_samples(
                &store,
                announcement,
                &policy,
                ctx.tenant_id,
                local_node_id,
                request.sample_size,
            );
            let context_text = store
                .pods
                .values()
                .find(|pod| {
                    pod.slug == announcement.pod_slug
                        && pod.origin_node_id.unwrap_or(local_node_id)
                            == announcement.origin_node_id
                })
                .and_then(|pod| store.pod_skill_packs.get(&pod.id))
                .map(|package| package.context_md.clone());
            let agent_evidence = collect_active_agent_evidence_for_candidate(
                &store,
                user_id,
                ctx.tenant_id,
                announcement,
                &policy,
                now,
            );
            if !agent_evidence.is_empty() {
                agent_evidence_by_announcement.insert(announcement.id, agent_evidence);
            }
            owned.push(OwnedCandidateEvidence {
                announcement: announcement.clone(),
                context_text,
                samples,
                endorsements,
                samples_verified,
            });
        }

        let candidates = owned
            .iter()
            .map(OwnedCandidateEvidence::as_evidence)
            .collect();
        let caps = ExplorationCaps::explore_defaults();
        let ranked = rank_similar_pods_with_agent_evidence(
            &local,
            candidates,
            &policy,
            caps,
            request.limit,
            &agent_evidence_by_announcement,
        );
        let results = ranked
            .into_iter()
            .map(|ranked| {
                let is_subscribed = store.subscriptions.values().any(|subscription| {
                    subscription.user_id == user_id
                        && subscription.tenant_id == ctx.tenant_id
                        && subscription.origin_node_id == ranked.announcement.origin_node_id
                        && subscription.pod_slug == ranked.announcement.pod_slug
                });
                let mut samples = ranked.samples;
                samples.truncate(request.sample_size);
                let mut reasons = ranked
                    .similarity
                    .reasons
                    .iter()
                    .map(crate::pod_similarity::SimilarityReason::display)
                    .collect::<Vec<_>>();
                // Trial is a typed flag; label once at the Explore DTO boundary.
                append_trial_exposure_label(&mut reasons, ranked.similarity.trial_exposure);
                ExplorePodResult {
                    announcement: ranked.announcement,
                    relevance: ranked.similarity.score,
                    reasons,
                    // Agent evidence never appears as Endorsements or announcement fields.
                    endorsements: ranked.endorsements,
                    sample_content_references: samples,
                    is_subscribed,
                    trial_exposure: ranked.similarity.trial_exposure,
                }
            })
            .collect();
        Ok(ExploreResponse { query, results })
    }

}
