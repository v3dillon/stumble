use super::super::prelude::*;
use super::super::*;

impl AgentTools {
    pub fn get_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()).into())
    }

    /// Reads one immutable historical Pod Package version.
    ///
    /// # Errors
    ///
    /// Returns an error when the Pod or version does not exist, the Harness is
    /// outside its Pod scope, or the store lock is poisoned.
    pub fn get_pod_package_version(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        version: PackageVersion,
    ) -> Result<PodPackage, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        store
            .pod_package_version(pod.id, version)
            .cloned()
            .ok_or_else(|| {
                StoreError::NotFound(format!("Pod Package version {}", version.value())).into()
            })
    }

    /// Requests a complete, version-aware revision from a portable Pod Package.
    ///
    /// Non-public origin packages are revised immediately. Public package
    /// revisions become Pending Proposals and do not alter authoritative state
    /// before approval.
    pub fn request_revise_pod_package(
        &self,
        ctx: &AuthContext,
        pod_id: PodId,
        base_version: PackageVersion,
        files: BTreeMap<String, String>,
        now: chrono::DateTime<Utc>,
    ) -> Result<PodPackageRevisionOutcome, AgentToolsError> {
        validate_portable_package_files(&files)?;
        let contents = pod_package_contents_from_files(&files)?;
        let validation = validate_pod_package_contents(&contents);
        if !validation.valid {
            return Err(StoreError::Validation(validation.errors.join(", ")).into());
        }

        let patch = complete_package_patch(&contents);
        let is_public = {
            let store = self
                .store
                .read()
                .map_err(|_| AgentToolsError::LockPoisoned)?;
            let pod = store
                .pods
                .get(&pod_id)
                .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
            store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
            authorize_harness(
                &store,
                ctx,
                HarnessCapability::PackageManagement,
                Some(pod.id),
            )?;
            ensure_direct_package_revision_allowed_for_origin(&store, ctx, pod)?;
            let existing = store
                .pod_skill_packs
                .get(&pod.id)
                .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
            ensure_package_base_version(existing, base_version)?;
            verify_portable_package_history_for_base(&store, &files, existing)?;
            pod.visibility == Visibility::Public
        };

        if is_public {
            let proposal = self.create_pending_proposal(
                ctx,
                SensitiveChange::RevisePublicPodPackage {
                    pod_id,
                    base_version,
                    patch,
                },
                now,
                now + Duration::hours(24),
            )?;
            return Ok(PodPackageRevisionOutcome::PendingApproval(Box::new(
                proposal,
            )));
        }

        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store
            .pods
            .get(&pod_id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(format!("pod {pod_id}")))?;
        store.assert_tenant(pod.tenant_id, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::PackageManagement,
            Some(pod.id),
        )?;
        ensure_direct_package_revision_allowed(&store, ctx, &pod)?;
        let existing = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        ensure_package_base_version(&existing, base_version)?;
        verify_portable_package_history_for_base(&store, &files, &existing)?;

        let mut package = patch_skill_pack(&existing, patch);
        let created_at = now;
        package.created_at = created_at;
        package.updated_at = created_at;
        package.proposer_harness_id = ctx.harness_id;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_skill_pack_updated",
            &pod.slug,
            json!({"package": package}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.insert_pod_package_version(package.clone())?;
        store.pod_skill_packs.insert(pod.id, package.clone());
        store.event_log.push(event);
        refresh_public_pod_announcement_if_needed(&mut store, pod.id, now)?;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::PatchSkillPack,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(PodPackageRevisionOutcome::Revised(Box::new(package)))
    }

    pub fn patch_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        patch: SkillPackPatch,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::PackageManagement,
            Some(pod.id),
        )?;
        ensure_direct_package_revision_allowed(&store, ctx, &pod)?;
        let existing = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let mut pack = patch_skill_pack(&existing, patch);
        let validation = validate_skill_pack(&pack);
        if !validation.valid {
            return Err(StoreError::Validation(validation.errors.join(", ")).into());
        }
        let now = Utc::now();
        pack.created_at = now;
        pack.updated_at = now;
        pack.proposer_harness_id = ctx.harness_id;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_skill_pack_updated",
            &pod.slug,
            json!({"package": pack}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.insert_pod_package_version(pack.clone())?;
        store.pod_skill_packs.insert(pod.id, pack.clone());
        store.event_log.push(event);
        refresh_public_pod_announcement_if_needed(&mut store, pod.id, now)?;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::PatchSkillPack,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(pack)
    }

    pub fn export_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<ExportedSkillPack, AgentToolsError> {
        let store = self
            .store
            .read()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness_pod_scope(&store, ctx, pod.id)?;
        let pack = store
            .pod_skill_packs
            .get(&pod.id)
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        let events_jsonl = store
            .portable_package_events_for_pod(&pod.slug)
            .into_iter()
            .map(|event| serde_json::to_string(&event))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default()
            .join("\n");
        Ok(export_skill_pack(pack, events_jsonl))
    }

    pub fn import_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
        files: BTreeMap<String, String>,
    ) -> Result<PodSkillPack, AgentToolsError> {
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        let pod = store.pod_by_slug(pod_slug, ctx.tenant_id)?;
        authorize_harness(
            &store,
            ctx,
            HarnessCapability::PackageManagement,
            Some(pod.id),
        )?;
        ensure_direct_package_revision_allowed(&store, ctx, &pod)?;
        let existing = store
            .pod_skill_packs
            .get(&pod.id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound("skill pack".to_string()))?;
        validate_portable_package_files(&files)?;
        verify_portable_package_history(&store, &files)?;
        let mut pack = import_skill_pack(&existing, &files);
        let report = validate_skill_pack(&pack);
        if !report.valid {
            return Err(StoreError::Validation(report.errors.join(", ")).into());
        }
        let now = Utc::now();
        pack.created_at = now;
        pack.updated_at = now;
        pack.proposer_harness_id = ctx.harness_id;
        store.insert_pod_package_version(pack.clone())?;
        store.pod_skill_packs.insert(pod.id, pack.clone());
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_package_imported",
            &pod.slug,
            json!({"package": pack}),
            store.latest_event_hash(&pod.slug),
        )?;
        store.event_log.push(event);
        refresh_public_pod_announcement_if_needed(&mut store, pod.id, now)?;
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ImportSkillPack,
            Some(pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(pack)
    }

    pub fn fork_skill_pack(
        &self,
        ctx: &AuthContext,
        source_pod_slug: &str,
        target: CreatePodRequest,
    ) -> Result<PodSkillPack, AgentToolsError> {
        if target.visibility == Visibility::Public {
            return Err(StoreError::Validation(
                "public Package Revisions require Pending Proposal approval".to_string(),
            )
            .into());
        }
        let source_pack = self.get_skill_pack(ctx, source_pod_slug)?;
        let target_pod = self.create_pod(ctx, target)?;
        let mut forked = fork_skill_pack(&source_pack, &target_pod);
        forked.proposer_harness_id = ctx.harness_id;
        let mut store = self
            .store
            .write()
            .map_err(|_| AgentToolsError::LockPoisoned)?;
        forked.version = 2;
        let node = store.node_for_tenant(ctx.tenant_id)?;
        let event = sign_public_event(
            &node,
            "pod_package_forked",
            &target_pod.slug,
            json!({"package": forked}),
            store.latest_event_hash(&target_pod.slug),
        )?;
        store.insert_pod_package_version(forked.clone())?;
        store.pod_skill_packs.insert(target_pod.id, forked.clone());
        store.event_log.push(event);
        record_harness_write(
            &mut store,
            ctx,
            HarnessWriteOperation::ForkSkillPack,
            Some(target_pod.id),
        );
        self.persist_locked(&mut store)?;
        Ok(forked)
    }

    pub fn validate_pod_skill_pack(
        &self,
        ctx: &AuthContext,
        pod_slug: &str,
    ) -> Result<ValidationReport, AgentToolsError> {
        let pack = self.get_skill_pack(ctx, pod_slug)?;
        Ok(validate_skill_pack(&pack))
    }
}
