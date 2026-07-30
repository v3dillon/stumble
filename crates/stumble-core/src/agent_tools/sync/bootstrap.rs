use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    /// Open Bootstrap admission for a public Pod Announcement.
    ///
    /// Requires no User account or Trusted Peer relationship. Verifies origin
    /// identity, signature, lease, protocol, canonical URL, reachability, live
    /// public manifest, and resource bounds before retaining the announcement
    /// and appending a topic-neutral stream entry.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::BootstrapRejected`] with a stable reason code
    /// when verification or policy fails, or persistence errors when the store
    /// cannot commit.
    pub fn admit_bootstrap_announcement(
        &self,
        announcement: PodAnnouncement,
    ) -> Result<BootstrapAdmissionAcceptance, AgentToolsError> {
        self.admit_bootstrap_announcement_at(announcement, Utc::now())
    }

    /// Open Bootstrap admission at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::admit_bootstrap_announcement`].
    pub fn admit_bootstrap_announcement_at(
        &self,
        announcement: PodAnnouncement,
        now: chrono::DateTime<Utc>,
    ) -> Result<BootstrapAdmissionAcceptance, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let result = admit_bootstrap_announcement(
            &mut store,
            announcement,
            self.bootstrap.origin_probe.as_ref(),
            self.bootstrap.enabled,
            now,
        );
        // Persist acceptance and rejection audit rows transactionally.
        self.persist_locked(&mut store)?;
        result.map_err(|reason| AgentToolsError::BootstrapRejected {
            message: format!("bootstrap admission rejected: {reason}"),
            reason,
        })
    }

    /// Open Bootstrap admission for an Origin-signed Pod Withdrawal.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::BootstrapRejected`] on verification or policy
    /// failure, or a persistence error when the store cannot commit.
    pub fn admit_bootstrap_withdrawal(
        &self,
        withdrawal: PodWithdrawal,
    ) -> Result<BootstrapWithdrawalAcceptance, AgentToolsError> {
        self.admit_bootstrap_withdrawal_at(withdrawal, Utc::now())
    }

    /// Open Bootstrap withdrawal admission at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::admit_bootstrap_withdrawal`].
    pub fn admit_bootstrap_withdrawal_at(
        &self,
        withdrawal: PodWithdrawal,
        now: chrono::DateTime<Utc>,
    ) -> Result<BootstrapWithdrawalAcceptance, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let result =
            admit_bootstrap_withdrawal(&mut store, withdrawal, self.bootstrap.enabled, now);
        self.persist_locked(&mut store)?;
        result.map_err(|reason| AgentToolsError::BootstrapRejected {
            message: format!("bootstrap withdrawal rejected: {reason}"),
            reason,
        })
    }

    /// Reads a topic-neutral cursor-paginated Announcement Stream page.
    ///
    /// Emits pending lease-expiry transitions at `now` before serving. The
    /// stream never includes Taste Profiles, Subscriptions, feedback, or
    /// personalized ranking data.
    ///
    /// # Errors
    ///
    /// Returns [`AgentToolsError::BootstrapRejected`] when Bootstrap is disabled
    /// or the cursor is unknown/invalid, or a persistence error when expiry
    /// transitions cannot be committed.
    pub fn announcement_stream(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
    ) -> Result<AnnouncementStreamPage, AgentToolsError> {
        self.announcement_stream_at(cursor, limit, Utc::now())
    }

    /// Reads the Announcement Stream at an explicit clock time.
    ///
    /// # Errors
    ///
    /// Same as [`Self::announcement_stream`].
    pub fn announcement_stream_at(
        &self,
        cursor: Option<&str>,
        limit: Option<usize>,
        now: chrono::DateTime<Utc>,
    ) -> Result<AnnouncementStreamPage, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let page = read_announcement_stream(&mut store, cursor, limit, self.bootstrap.enabled, now)
            .map_err(|reason| AgentToolsError::BootstrapRejected {
                message: format!("announcement stream rejected: {reason}"),
                reason,
            })?;
        self.persist_locked(&mut store)?;
        Ok(page)
    }

    /// Lists configured Bootstrap endpoints in order (User-controlled, removable).
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied or the store lock is poisoned.
    pub fn list_bootstrap_endpoints(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<BootstrapEndpointConfig>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(list_bootstrap_endpoints(&store))
    }

    /// Reports configured Bootstrap endpoints with cursor and last-attempt state.
    ///
    /// Surfaces never include Taste Profile, Subscriptions, feedback, or other
    /// private discovery evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when administration is denied or the store lock is poisoned.
    pub fn bootstrap_status(
        &self,
        ctx: &AuthContext,
    ) -> Result<Vec<BootstrapEndpointStatus>, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        Ok(bootstrap_endpoint_statuses(&store))
    }

    /// Adds a Bootstrap endpoint to the ordered User-controlled list.
    ///
    /// # Errors
    ///
    /// Returns validation, duplicate, authorization, or persistence errors.
    pub fn add_bootstrap_endpoint(
        &self,
        ctx: &AuthContext,
        label: &str,
        base_url: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<BootstrapEndpointConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let endpoint = add_bootstrap_endpoint(&mut store, label, base_url, now)?;
        self.persist_locked(&mut store)?;
        Ok(endpoint)
    }

    /// Enables or disables a configured Bootstrap endpoint.
    ///
    /// # Errors
    ///
    /// Returns not-found, authorization, or persistence errors.
    pub fn set_bootstrap_endpoint_enabled(
        &self,
        ctx: &AuthContext,
        endpoint_id: BootstrapEndpointId,
        enabled: bool,
    ) -> Result<BootstrapEndpointConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let endpoint = set_bootstrap_endpoint_enabled(&mut store, endpoint_id, enabled)?;
        self.persist_locked(&mut store)?;
        Ok(endpoint)
    }

    /// Removes a Bootstrap endpoint from configuration.
    ///
    /// Announcements known only through this endpoint leave current eligibility
    /// while remaining in the local audit store.
    ///
    /// # Errors
    ///
    /// Returns not-found, authorization, or persistence errors.
    pub fn remove_bootstrap_endpoint(
        &self,
        ctx: &AuthContext,
        endpoint_id: BootstrapEndpointId,
    ) -> Result<BootstrapEndpointConfig, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        let endpoint = remove_bootstrap_endpoint(&mut store, endpoint_id)?;
        self.persist_locked(&mut store)?;
        Ok(endpoint)
    }

    /// Ensures the sponsored default Bootstrap endpoint is present for new nodes.
    ///
    /// Idempotent: does nothing when any Bootstrap endpoint is already configured.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors.
    pub fn ensure_default_bootstrap_endpoint(
        &self,
        ctx: &AuthContext,
        now: chrono::DateTime<Utc>,
    ) -> Result<Vec<BootstrapEndpointConfig>, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
        ensure_default_bootstrap_endpoint(&mut store, now);
        self.persist_locked(&mut store)?;
        Ok(list_bootstrap_endpoints(&store))
    }

    /// Synchronizes Announcement Streams from each enabled Bootstrap in order.
    ///
    /// On transport or protocol failure the pass falls through to the next
    /// configured endpoint without discarding previously verified announcements.
    /// Outbound requests carry only cursor pagination fields.
    ///
    /// Network I/O runs **outside** the store write lock: endpoints and cursors
    /// are snapshotted under a read lock, pages are fetched without holding the
    /// store, and each endpoint is applied + persisted under a short write lock
    /// so partial progress survives.
    ///
    /// # Errors
    ///
    /// Returns authorization or persistence errors. Per-endpoint typed failures
    /// are reported inside the [`BootstrapSyncReport`], not as hard errors.
    pub fn sync_bootstrap_endpoints(
        &self,
        ctx: &AuthContext,
        client: &dyn AnnouncementStreamClient,
        now: chrono::DateTime<Utc>,
    ) -> Result<BootstrapSyncReport, AgentToolsError> {
        let plans = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            authorize_harness(&store, ctx, HarnessCapability::Administration, None)?;
            plan_bootstrap_sync(&store)
        };

        let mut outcomes = Vec::with_capacity(plans.len());
        let mut retained_announcements = 0usize;
        let mut retained_withdrawals = 0usize;

        for plan in plans {
            // Fetch without holding the store lock (no network I/O under write).
            let fetched =
                fetch_bootstrap_stream_pages(client, &plan.endpoint.base_url, plan.cursor);
            let outcome = {
                let mut store = self
                    .store
                    .write()
                    .map_err(|_| AgentToolsError::LockPoisoned)?;
                let outcome =
                    apply_bootstrap_stream_pages(&mut store, &plan.endpoint, fetched, now);
                self.persist_locked(&mut store)?;
                outcome
            };
            retained_announcements =
                retained_announcements.saturating_add(outcome.retained_announcements);
            retained_withdrawals =
                retained_withdrawals.saturating_add(outcome.retained_withdrawals);
            outcomes.push(outcome);
        }

        Ok(BootstrapSyncReport {
            outcomes,
            retained_announcements,
            retained_withdrawals,
        })
    }
}
