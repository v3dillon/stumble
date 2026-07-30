use super::*;

/// Current lifecycle state with state-specific lease data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "lease", rename_all = "snake_case")]
pub enum DiscoveryTaskState {
    /// Available to an authorized harness.
    Pending,
    /// Exclusively owned until the embedded lease expires.
    Leased(DiscoveryTaskLease),
    /// Successfully completed and immutable.
    Completed,
    /// Exhausted the permitted attempts.
    TerminalFailure,
}

/// Provenance and instructions that created a Discovery Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscoveryTaskOrigin {
    /// Due work derived from one versioned Source Rule.
    Scheduled {
        /// Zero-based position in the Pod Package Source Rules.
        source_rule_index: usize,
    },
    /// Immediate work requested during a conversation.
    Immediate {
        /// Discovery intent that a later claiming harness must follow.
        instructions: String,
        /// Retry-safe key unique to the requesting harness.
        idempotency_key: String,
        /// Harness that supplied the intent.
        requested_by: AgentHarnessId,
    },
    /// On-demand User-scoped work governed only by its pinned Discovery Plan.
    PersonalRequest {
        /// Retry-safe key unique to the requesting interactive Harness.
        idempotency_key: String,
        /// Interactive Harness that requested the plan.
        requested_by: Option<AgentHarnessId>,
    },
    /// Due work derived from one named private Personal Discovery schedule.
    ///
    /// Identity for a schedule period is `(schedule_id, due_at)`; materialization is
    /// idempotent across retries, restarts, concurrent wakeups, and scheduler changes.
    PersonalScheduled {
        /// Schedule that produced this period's task.
        schedule_id: PersonalDiscoveryScheduleId,
    },
}

/// Evidence basis that makes generic Personal Discovery ready.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum DiscoveryPlanBasis {
    ExplicitTopic(String),
    CorroboratedTopic(String),
    CorroboratedSource(SourceAffinitySignal),
}

/// Private finite shortlist returned from one Personal Discovery Task.
///
/// Never federated. Retains task and plan identity for explainability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryResultBatch {
    pub id: DiscoveryResultBatchId,
    pub user_id: UserId,
    pub tenant_id: Option<TenantId>,
    pub task_id: DiscoveryTaskId,
    pub plan_id: DiscoveryPlanId,
    /// Ready / reviewed / dismissed lifecycle, independent of task state.
    pub state: DiscoveryResultBatchState,
    /// One-shot results-ready notice state, independent of review.
    pub notification_state: DiscoveryResultNotificationState,
    /// Plan-requested finite size.
    pub requested_size: u16,
    /// Plan allocation quotas at completion time.
    pub allocation: DiscoveryPlanAllocation,
    /// How many selected items filled each allocation role after policy.
    pub allocation_filled: DiscoveryPlanAllocation,
    /// Ordered finite Candidate references with provenance.
    pub items: Vec<DiscoveryResultItem>,
    /// Inspectable underfill, reallocation, cap, block, and availability reasons.
    pub source_availability: Vec<DiscoveryResultAvailabilityReason>,
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub dismissed_at: Option<DateTime<Utc>>,
}

/// Exclusive, expiring ownership of a Discovery Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryTaskLease {
    /// Harness with exclusive execution authority.
    pub harness_id: AgentHarnessId,
    /// Time at which this attempt began.
    pub claimed_at: DateTime<Utc>,
    /// Time after which another harness may safely claim the task.
    pub expires_at: DateTime<Utc>,
}

/// Inspectable outcome of one claimed task attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryTaskAttemptOutcome {
    /// Harness completed the task successfully.
    Completed,
    /// Harness explicitly failed the attempt with an inspectable reason.
    Failed {
        /// Harness-supplied explanation.
        reason: String,
    },
    /// Harness abandoned the task until its lease expired.
    LeaseExpired,
}

/// Immutable history entry for a completed or failed lease attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryTaskAttempt {
    /// Harness responsible for this attempt.
    pub harness_id: AgentHarnessId,
    /// Lease claim time.
    pub started_at: DateTime<Utc>,
    /// Completion, failure, or expiry time.
    pub finished_at: DateTime<Utc>,
    /// Inspectable terminal result of this attempt.
    pub outcome: DiscoveryTaskAttemptOutcome,
}

/// Immutable contract governing one Discovery Task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscoveryTaskTarget {
    /// Pod discovery pinned to the Package version the worker must follow.
    Pod {
        /// Pod whose Package governs this work.
        pod_id: PodId,
        /// Immutable Package version used by the worker.
        package_version: PackageVersion,
    },
    /// Personal Discovery pinned to a private immutable Discovery Plan.
    Personal {
        /// Plan minimized for and pinned to this task.
        discovery_plan_id: DiscoveryPlanId,
    },
}

impl DiscoveryTaskTarget {
    /// Returns the Pod contract when this is Pod discovery.
    #[must_use]
    pub const fn pod(&self) -> Option<(PodId, PackageVersion)> {
        match self {
            Self::Pod {
                pod_id,
                package_version,
            } => Some((*pod_id, *package_version)),
            Self::Personal { .. } => None,
        }
    }

    /// Returns the pinned plan identity when this is Personal Discovery.
    #[must_use]
    pub const fn discovery_plan_id(&self) -> Option<DiscoveryPlanId> {
        match self {
            Self::Personal { discovery_plan_id } => Some(*discovery_plan_id),
            Self::Pod { .. } => None,
        }
    }
}

/// Leaseable discovery work derived from a Source Rule or immediate request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryTask {
    /// Stable task identity.
    pub id: DiscoveryTaskId,
    /// Immutable Pod or Personal Discovery contract.
    pub target: DiscoveryTaskTarget,
    /// Scheduled or conversational provenance.
    pub origin: DiscoveryTaskOrigin,
    /// Earliest claim time.
    pub due_at: DateTime<Utc>,
    /// Current lifecycle state.
    pub state: DiscoveryTaskState,
    /// Completed, failed, and expired attempt history.
    pub attempts: Vec<DiscoveryTaskAttempt>,
    /// Time at which Stumble created the task.
    pub created_at: DateTime<Utc>,
}

impl Serialize for DiscoveryTask {
    fn serialize<Serializer>(
        &self,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error>
    where
        Serializer: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let field_count = if self.target.pod().is_some() { 9 } else { 7 };
        let mut task = serializer.serialize_struct("DiscoveryTask", field_count)?;
        task.serialize_field("id", &self.id)?;
        task.serialize_field("target", &self.target)?;
        if let Some((pod_id, package_version)) = self.target.pod() {
            task.serialize_field("pod_id", &pod_id)?;
            task.serialize_field("package_version", &package_version)?;
        }
        task.serialize_field("origin", &self.origin)?;
        task.serialize_field("due_at", &self.due_at)?;
        task.serialize_field("state", &self.state)?;
        task.serialize_field("attempts", &self.attempts)?;
        task.serialize_field("created_at", &self.created_at)?;
        task.end()
    }
}

#[derive(Deserialize)]
struct DiscoveryTaskWire {
    id: DiscoveryTaskId,
    #[serde(default)]
    target: Option<DiscoveryTaskTarget>,
    #[serde(default)]
    pod_id: Option<PodId>,
    #[serde(default)]
    package_version: Option<PackageVersion>,
    origin: DiscoveryTaskOrigin,
    due_at: DateTime<Utc>,
    state: DiscoveryTaskState,
    attempts: Vec<DiscoveryTaskAttempt>,
    created_at: DateTime<Utc>,
}

impl<'de> Deserialize<'de> for DiscoveryTask {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let wire = DiscoveryTaskWire::deserialize(deserializer)?;
        let target = match (wire.target, wire.pod_id, wire.package_version) {
            (Some(target), None, None) => target,
            (
                Some(
                    target @ DiscoveryTaskTarget::Pod {
                        pod_id: target_pod_id,
                        package_version: target_package_version,
                    },
                ),
                Some(pod_id),
                Some(package_version),
            ) if target_pod_id == pod_id && target_package_version == package_version => target,
            (Some(_), Some(_), Some(_)) => {
                return Err(Deserializer::Error::custom(
                    "typed and legacy Discovery Task targets must agree",
                ))
            }
            (Some(_), _, _) => {
                return Err(Deserializer::Error::custom(
                    "legacy Discovery Task target fields must be complete",
                ))
            }
            (None, Some(pod_id), Some(package_version)) => DiscoveryTaskTarget::Pod {
                pod_id,
                package_version,
            },
            (None, None, _) => return Err(Deserializer::Error::missing_field("target")),
            (None, Some(_), None) => {
                return Err(Deserializer::Error::missing_field("package_version"))
            }
        };
        Ok(Self {
            id: wire.id,
            target,
            origin: wire.origin,
            due_at: wire.due_at,
            state: wire.state,
            attempts: wire.attempts,
            created_at: wire.created_at,
        })
    }
}

/// Request for immediate conversational discovery with retry-safe identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateImmediateDiscoveryTaskRequest {
    /// Pod to discover for.
    pub pod_id: PodId,
    /// Conversation-derived discovery intent.
    pub instructions: String,
    /// Retry-safe caller key.
    pub idempotency_key: String,
}
