use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    pub fn save_link(
        &self,
        ctx: &AuthContext,
        submission_id: SubmissionId,
    ) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let submission = store
            .submissions
            .get(&submission_id)
            .ok_or_else(|| StoreError::NotFound("submission".to_string()))?;
        store.assert_tenant(submission.tenant_id, ctx.tenant_id)?;
        authorize_harness_submission_scope(&store, ctx, submission_id)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        store.saves.insert((user_id, submission_id));
        store.feedback_events.push(FeedbackEvent {
            user_id,
            tenant_id: ctx.tenant_id,
            submission_id,
            event_type: FeedbackKind::Saved,
            reason: None,
            created_at: Utc::now(),
            local_only: true,
        });
        record_harness_write(&mut store, ctx, HarnessWriteOperation::SaveLink, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn block_source(&self, ctx: &AuthContext, source: String) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            });
        if !prefs.blocked_sources.contains(&source) {
            prefs.blocked_sources.push(source);
        }
        record_harness_write(&mut store, ctx, HarnessWriteOperation::BlockSource, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn block_topic(&self, ctx: &AuthContext, topic: String) -> Result<(), AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let Some(user_id) = ctx.user_id else {
            return Ok(());
        };
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            });
        if !prefs.blocked_topics.contains(&topic) {
            prefs.blocked_topics.push(topic);
        }
        record_harness_write(&mut store, ctx, HarnessWriteOperation::BlockTopic, None);
        self.persist_locked(&mut store)?;
        Ok(())
    }

    pub fn update_preferences(
        &self,
        ctx: &AuthContext,
        request: UpdatePreferencesRequest,
    ) -> Result<UserPreferences, AgentToolsError> {
        let Some(user_id) = ctx.user_id else {
            return Err(StoreError::Validation(
                "preferences require an authenticated user".to_string(),
            )
            .into());
        };
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let prefs = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: vec![],
                blocked_topics: vec![],
                blocked_sources: vec![],
                blocked_source_affinities: vec![],
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            });
        if let Some(interests) = request.interests {
            prefs.interests = normalize_unique(interests);
        }
        if let Some(blocked_topics) = request.blocked_topics {
            prefs.blocked_topics = normalize_unique(blocked_topics);
        }
        if let Some(blocked_sources) = request.blocked_sources {
            prefs.blocked_sources = normalize_unique(blocked_sources);
        }
        if let Some(length) = request.preferred_brief_length {
            prefs.preferred_brief_length = length.clamp(1, 10);
        }
        if let Some(mode) = request.preferred_discovery_mode {
            prefs.preferred_discovery_mode = mode;
        }
        let prefs = prefs.clone();
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::UpdatePreferences,
            None,
        );
        self.persist_locked(&mut store)?;
        Ok(prefs)
    }

    /// Returns the User's private explicit and learned Taste Profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the interactive Harness Grant lacks feedback access,
    /// no User is authenticated, or local state cannot be read.
    pub fn taste_profile(&self, ctx: &AuthContext) -> Result<TasteProfile, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Taste Profile requires an authenticated User".into())
        })?;
        let preferences = store.user_preferences.get(&(user_id, ctx.tenant_id));
        let interest_seed_evidence = interest_seed_evidence(&store, user_id, ctx.tenant_id);
        let projections = taste_profile_projections(&store, user_id, ctx.tenant_id, preferences);
        let mut allowed_actions = vec![
            TasteProfileAllowedAction::Set,
            TasteProfileAllowedAction::Reset,
        ];
        if interest_seed_evidence.active_seed_count > 0 {
            allowed_actions.push(TasteProfileAllowedAction::Retract);
        }
        Ok(TasteProfile {
            user_id,
            tenant_id: ctx.tenant_id,
            explicit: ExplicitTastePreferences {
                interests: preferences
                    .map(|preferences| preferences.interests.clone())
                    .unwrap_or_default(),
                blocked_topics: preferences
                    .map(|preferences| preferences.blocked_topics.clone())
                    .unwrap_or_default(),
                blocked_sources: preferences
                    .map(|preferences| preferences.blocked_sources.clone())
                    .unwrap_or_default(),
                blocked_source_affinities: preferences
                    .map(|preferences| preferences.blocked_source_affinities.clone())
                    .unwrap_or_default(),
                recurrence_penalty_days: preferences
                    .map_or_else(RecurrencePenaltyDays::default, |preferences| {
                        preferences.recurrence_penalty_days
                    })
                    .get(),
            },
            learned: projections.learned,
            interest_seed_evidence,
            source_affinities: projections.source_affinities,
            allowed_actions,
        })
    }

    /// Retracts one canonical submission's learning contribution without deleting content.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks private retraction authority, the
    /// Interest Seed does not exist in the caller's User and tenant scope, or
    /// persistence fails.
    pub fn retract_interest_seed(
        &self,
        ctx: &AuthContext,
        candidate_id: CandidateId,
    ) -> Result<TasteProfile, AgentToolsError> {
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Interest Seed retraction requires an authenticated User".into())
        })?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let mut projected = store.clone();
        let seed = projected
            .interest_seeds
            .get_mut(&(user_id, candidate_id))
            .filter(|seed| seed.tenant_id == ctx.tenant_id)
            .ok_or_else(|| StoreError::NotFound("Interest Seed".into()))?;
        if seed.retracted_at.is_none() {
            seed.retracted_at = Some(Utc::now());
            record_harness_write(
                &mut projected,
                ctx,
                HarnessWriteOperation::RetractInterestSeed,
                None,
            );
            self.persist_locked(&mut projected)?;
            *store = projected;
        }
        drop(store);
        self.taste_profile(ctx)
    }

    /// Replaces any supplied explicit Taste Profile settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the interactive Harness Grant lacks feedback access,
    /// no User is authenticated, or persistence fails.
    pub fn update_taste_profile(
        &self,
        ctx: &AuthContext,
        request: UpdateTasteProfileRequest,
    ) -> Result<TasteProfile, AgentToolsError> {
        let Some(user_id) = ctx.user_id else {
            return Err(StoreError::Validation(
                "Taste Profile requires an authenticated User".into(),
            )
            .into());
        };
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let preferences = store
            .user_preferences
            .entry((user_id, ctx.tenant_id))
            .or_insert(UserPreferences {
                user_id,
                tenant_id: ctx.tenant_id,
                interests: Vec::new(),
                blocked_topics: Vec::new(),
                blocked_sources: Vec::new(),
                blocked_source_affinities: Vec::new(),
                preferred_brief_length: 7,
                preferred_discovery_mode: DiscoveryMode::DeepMatch,
                recurrence_penalty_days: RecurrencePenaltyDays::default(),
            });
        if let Some(interests) = request.interests {
            preferences.interests = normalize_unique_case_insensitive(interests);
        }
        if let Some(blocked_topics) = request.blocked_topics {
            preferences.blocked_topics = normalize_unique_case_insensitive(blocked_topics);
        }
        if let Some(blocked_sources) = request.blocked_sources {
            preferences.blocked_sources = normalize_unique_case_insensitive(blocked_sources);
        }
        if let Some(blocked_source_affinities) = request.blocked_source_affinities {
            preferences.blocked_source_affinities =
                normalize_source_affinity_signals(blocked_source_affinities);
        }
        if let Some(recurrence_penalty_days) = request.recurrence_penalty_days {
            preferences.recurrence_penalty_days = recurrence_penalty_days;
        }
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::UpdatePreferences,
            None,
        );
        self.persist_locked(&mut store)?;
        drop(store);
        self.taste_profile(ctx)
    }

    /// Resets one learned preference, or the entire learned layer.
    ///
    /// # Errors
    ///
    /// Returns an error when the caller lacks interactive private-profile authority,
    /// no User is authenticated, or persistence fails.
    pub fn reset_learned_taste(
        &self,
        ctx: &AuthContext,
        request: ResetLearnedTasteRequest,
    ) -> Result<TasteProfile, AgentToolsError> {
        let user_id = ctx.user_id.ok_or_else(|| {
            StoreError::Validation("Taste Profile requires an authenticated User".into())
        })?;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_taste_profile(&store, ctx)?;
        let mut projected = store.clone();
        projected.taste_learning_evidence.retain(|evidence| {
            if evidence.user_id != user_id || evidence.tenant_id != ctx.tenant_id {
                return true;
            }
            request
                .signal
                .as_ref()
                .is_some_and(|signal| signal != &evidence.signal)
        });
        reset_interest_seed_evidence(
            &mut projected,
            user_id,
            ctx.tenant_id,
            request.signal.as_ref(),
        );
        record_harness_write(
            &mut projected,
            ctx,
            HarnessWriteOperation::ResetLearnedTaste,
            None,
        );
        self.persist_locked(&mut projected)?;
        *store = projected;
        drop(store);
        self.taste_profile(ctx)
    }

}
