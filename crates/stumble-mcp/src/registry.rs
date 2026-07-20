use serde_json::{json, Value};
use std::sync::OnceLock;
use stumble_core::HarnessCapability;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolHandlerKind {
    Blocking,
    Async,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolDefinition {
    pub(crate) tool: McpTool,
    pub(crate) name: &'static str,
    pub(crate) availability: ToolAvailability,
    pub(crate) input_schema: Value,
    pub(crate) handler: ToolHandlerKind,
    pub(crate) discovery_order: Option<usize>,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) read_only: bool,
    pub(crate) destructive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolAvailability {
    Public,
    CapabilityOnly(HarnessCapability),
    InteractiveFeedback,
    UnscopedInteractiveFeedback,
    DiscoveryExecution,
    /// CandidateSubmission grant, or Personal Discovery execution for task-bound results.
    CandidateSubmissionOrPersonalExecution,
    PersonalPlanAccess,
    PersonalDiscoveryManagement,
}

impl ToolAvailability {
    pub(crate) const fn capability(self) -> Option<HarnessCapability> {
        match self {
            Self::Public => None,
            Self::CapabilityOnly(capability) => Some(capability),
            Self::InteractiveFeedback | Self::UnscopedInteractiveFeedback => {
                Some(HarnessCapability::Feedback)
            }
            Self::DiscoveryExecution
            | Self::CandidateSubmissionOrPersonalExecution
            | Self::PersonalPlanAccess
            | Self::PersonalDiscoveryManagement => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum McpTool {
    ListPods,
    GetFeedBatch,
    CompleteFeedBatch,
    RecordFeedFeedback,
    SetPrioritySubscription,
    GetTasteProfile,
    UpdateTasteProfile,
    ResetLearnedTaste,
    RetractInterestSeed,
    RegisterAgentHarness,
    RevokeAgentHarness,
    CreatePendingProposal,
    GetPendingProposal,
    ApprovePendingProposal,
    RejectPendingProposal,
    CreatePod,
    RouteCandidate,
    ReviewCandidatePlacement,
    ListPodContent,
    CreatePrivatePodWithPackage,
    JoinPod,
    SubmitCandidate,
    InspectCandidate,
    MaterializeDiscoveryTasks,
    ListDiscoveryTasks,
    ListReadyDiscoveryTasks,
    CreateImmediateDiscoveryTask,
    DiscoveryTaskStatus,
    ClaimDiscoveryTask,
    RenewDiscoveryTask,
    CompleteDiscoveryTask,
    FailDiscoveryTask,
    PersonalDiscoveryReadiness,
    RequestPersonalDiscovery,
    GetDiscoveryPlan,
    CompleteDiscoveryResultBatch,
    ReportDiscoverySourceAvailability,
    GetDiscoveryTaskSourceAvailability,
    ListAuthenticationNeededNotices,
    ListDiscoveryResultBatches,
    GetDiscoveryResultBatch,
    DismissDiscoveryResultBatch,
    MarkDiscoveryResultBatchReviewed,
    ReviewDiscoveryResultItem,
    CreatePersonalDiscoverySchedule,
    ListPersonalDiscoverySchedules,
    GetPersonalDiscoverySchedule,
    UpdatePersonalDiscoverySchedule,
    DisablePersonalDiscoverySchedule,
    RemovePersonalDiscoverySchedule,
    AttemptDiscoveryResultsReadyNotification,
    GetPodPackage,
    ExportPodPackage,
    ImportPodPackage,
    ForkPodPackage,
    ValidatePodPackage,
    GetNodeInfo,
    ListTrustedPeers,
    AddTrustedPeer,
    SubscribePublicPod,
    SynchronizeSubscription,
    SyncPodWithPeer,
    ExportPodEvents,
    ImportPodEvents,
}

#[cfg(test)]
impl McpTool {
    pub(crate) const VARIANT_COUNT: usize = Self::ImportPodEvents as usize + 1;
}

pub(crate) fn definitions() -> &'static [ToolDefinition] {
    use HarnessCapability as Capability;
    use McpTool as Tool;
    use ToolAvailability::{
        CandidateSubmissionOrPersonalExecution, CapabilityOnly, DiscoveryExecution,
        InteractiveFeedback, PersonalDiscoveryManagement, PersonalPlanAccess, Public,
        UnscopedInteractiveFeedback,
    };
    use ToolHandlerKind::{Async, Blocking};

    static DEFINITIONS: OnceLock<Vec<ToolDefinition>> = OnceLock::new();
    DEFINITIONS.get_or_init(|| vec![
        d(Tool::ListPods, "list_pods", Public, empty_schema(), Blocking, published(0, "List Pods", "List the Pods visible to this Agent Harness.", true, false)),
        d(Tool::GetFeedBatch, "get_feed_batch", CapabilityOnly(Capability::FeedRead), feed_batch_schema(), Blocking, published(18, "Get Personal Feed", "Return a stable finite Feed Batch with provenance, explanations, and allowed actions.", false, false)),
        d(Tool::CompleteFeedBatch, "complete_feed_batch", CapabilityOnly(Capability::FeedRead), uuid_schema("batch_id"), Blocking, published(19, "Complete Feed Batch", "Mark a finite Feed Batch complete after presentation.", false, false)),
        d(Tool::RecordFeedFeedback, "record_feed_feedback", InteractiveFeedback, feedback_schema(), Blocking, published(20, "Record Feed Feedback", "Record an explicit private Feedback Signal for a delivered Content Item.", false, false)),
        d(Tool::SetPrioritySubscription, "set_priority_subscription", CapabilityOnly(Capability::SubscriptionManagement), object_schema(json!({"pod_id": uuid(), "is_priority": {"type": "boolean"}}), &["pod_id", "is_priority"]), Blocking, hidden("Set Priority Subscription", "Change whether a Subscription is prioritized in Feed composition.", false, false)),
        d(Tool::GetTasteProfile, "get_taste_profile", UnscopedInteractiveFeedback, empty_schema(), Blocking, published(21, "Get Taste Profile", "Read the private inspectable Taste Profile.", true, false)),
        d(Tool::UpdateTasteProfile, "update_taste_profile", UnscopedInteractiveFeedback, update_taste_schema(), Blocking, hidden("Update Taste Profile", "Update explicit private Taste Profile settings.", false, false)),
        d(Tool::ResetLearnedTaste, "reset_learned_taste", UnscopedInteractiveFeedback, reset_taste_schema(), Blocking, hidden("Reset Learned Taste", "Reset learned private Taste Profile weights.", false, true)),
        d(Tool::RetractInterestSeed, "retract_interest_seed", UnscopedInteractiveFeedback, uuid_schema("candidate_id"), Blocking, published(22, "Retract Interest Seed", "Retract one submitted reference's private learning contribution without deleting the reference.", false, true)),
        d(Tool::RegisterAgentHarness, "register_agent_harness", CapabilityOnly(Capability::Administration), register_harness_schema(), Blocking, hidden("Register Agent Harness", "Register a scoped Agent Harness grant.", false, false)),
        d(Tool::RevokeAgentHarness, "revoke_agent_harness", CapabilityOnly(Capability::Administration), uuid_schema("harness_id"), Blocking, hidden("Revoke Agent Harness", "Revoke an Agent Harness grant.", false, true)),
        d(Tool::CreatePendingProposal, "create_pending_proposal", CapabilityOnly(Capability::PodCuration), pending_proposal_schema(), Blocking, hidden("Create Pending Proposal", "Propose a sensitive change for independent approval.", false, false)),
        d(Tool::GetPendingProposal, "get_pending_proposal", CapabilityOnly(Capability::Approval), uuid_schema("proposal_id"), Blocking, published(3, "Inspect Pending Proposal", "Inspect a sensitive change before making an independent approval decision.", true, false)),
        d(Tool::ApprovePendingProposal, "approve_pending_proposal", CapabilityOnly(Capability::Approval), uuid_schema("proposal_id"), Blocking, published(4, "Approve Pending Proposal", "Approve a sensitive change as a separately authorized interactive Harness.", false, false)),
        d(Tool::RejectPendingProposal, "reject_pending_proposal", CapabilityOnly(Capability::Approval), object_schema(json!({"proposal_id": uuid(), "reason": {"type": "string"}}), &["proposal_id", "reason"]), Blocking, published(5, "Reject Pending Proposal", "Reject a sensitive change as a separately authorized interactive Agent Harness.", false, false)),
        d(Tool::CreatePod, "create_pod", CapabilityOnly(Capability::PodCuration), create_pod_schema(), Blocking, published(2, "Create Pod", "Create an isolated Pod. Public exposure returns a Pending Proposal and does not take effect until independently approved.", false, false)),
        d(Tool::RouteCandidate, "route_candidate", CapabilityOnly(Capability::PodCuration), route_candidate_schema(), Blocking, published(6, "Route Candidate", "Propose an evidence-backed Pod Placement for a private Candidate in an authorized local Pod.", false, false)),
        d(Tool::ReviewCandidatePlacement, "review_candidate_placement", CapabilityOnly(Capability::PodCuration), review_candidate_schema(), Blocking, published(7, "Review Candidate Placement", "Accept or reject one pending Pod Placement under existing curation authority.", false, false)),
        d(Tool::ListPodContent, "list_pod_content", CapabilityOnly(Capability::FeedRead), uuid_schema("pod_id"), Blocking, published(8, "List Pod Content", "List the complete accepted Content Item stream for one Pod without private Candidate data.", true, false)),
        d(Tool::CreatePrivatePodWithPackage, "create_private_pod_with_package", CapabilityOnly(Capability::PodCuration), private_pod_package_schema(), Blocking, hidden("Create Private Pod with Package", "Create a private Pod with its initial Pod Package.", false, false)),
        d(Tool::JoinPod, "join_pod", CapabilityOnly(Capability::SubscriptionManagement), string_schema("pod_slug"), Blocking, hidden("Join Pod", "Subscribe to a local Pod by slug.", false, false)),
        d(Tool::SubmitCandidate, "submit_candidate", CandidateSubmissionOrPersonalExecution, candidate_schema(), Blocking, published(11, "Save Discovered Link", "Submit one externally discovered link with source metadata and provenance to an explicit User, Pod Placements, or Personal Discovery Task target. This creates a private Candidate, not an Accepted Placement.", false, false)),
        d(Tool::InspectCandidate, "inspect_candidate", CandidateSubmissionOrPersonalExecution, uuid_schema("candidate_id"), Blocking, published(12, "Inspect Candidate", "Inspect a private Candidate and its independent provenance records.", true, false)),
        d(Tool::MaterializeDiscoveryTasks, "materialize_discovery_tasks", CapabilityOnly(Capability::DiscoveryTasks), empty_schema(), Blocking, hidden("Materialize Discovery Tasks", "Materialize due Discovery Tasks from Source Rules.", false, false)),
        d(Tool::ListDiscoveryTasks, "list_discovery_tasks", DiscoveryExecution, empty_schema(), Blocking, hidden("List Discovery Tasks", "List Discovery Tasks visible to this Harness.", true, false)),
        d(Tool::ListReadyDiscoveryTasks, "list_ready_discovery_tasks", DiscoveryExecution, empty_schema(), Blocking, published(13, "List Ready Discovery Tasks", "List due discovery work that this Agent Harness is authorized to claim.", true, false)),
        d(Tool::CreateImmediateDiscoveryTask, "create_immediate_discovery_task", CapabilityOnly(Capability::DiscoveryTasks), immediate_task_schema(), Blocking, published(14, "Request Discovery", "Create retry-safe discovery work for a Pod from the user's current instructions.", false, false)),
        d(Tool::DiscoveryTaskStatus, "discovery_task_status", DiscoveryExecution, uuid_schema("task_id"), Blocking, hidden("Discovery Task Status", "Inspect one Discovery Task.", true, false)),
        d(Tool::ClaimDiscoveryTask, "claim_discovery_task", DiscoveryExecution, task_lease_schema(), Blocking, published(15, "Claim Discovery Task", "Claim a ready Discovery Task with an exclusive, expiring lease.", false, false)),
        d(Tool::RenewDiscoveryTask, "renew_discovery_task", DiscoveryExecution, task_lease_schema(), Blocking, hidden("Renew Discovery Task", "Renew an owned Discovery Task lease.", false, false)),
        d(Tool::CompleteDiscoveryTask, "complete_discovery_task", DiscoveryExecution, uuid_schema("task_id"), Blocking, published(16, "Complete Discovery Task", "Mark a claimed Discovery Task complete after its Candidates have been submitted.", false, false)),
        d(Tool::FailDiscoveryTask, "fail_discovery_task", DiscoveryExecution, object_schema(json!({"task_id": uuid(), "reason": {"type": "string"}}), &["task_id", "reason"]), Blocking, published(17, "Fail Discovery Task", "Record a failed Discovery Task attempt with an inspectable reason.", false, true)),
        d(Tool::PersonalDiscoveryReadiness, "personal_discovery_readiness", PersonalDiscoveryManagement, empty_schema(), Blocking, published(23, "Check Personal Discovery Readiness", "Inspect whether private aggregate User evidence can support a generic Personal Discovery run.", true, false)),
        d(Tool::RequestPersonalDiscovery, "request_personal_discovery", PersonalDiscoveryManagement, personal_discovery_schema(), Blocking, published(24, "Request Personal Discovery", "Create a retry-safe minimized Discovery Plan and User-scoped task without selecting a Pod or source.", false, false)),
        d(Tool::GetDiscoveryPlan, "get_discovery_plan", PersonalPlanAccess, uuid_schema("discovery_plan_id"), Blocking, published(25, "Read Discovery Plan", "Read an authorized minimized task-specific Discovery Plan.", true, false)),
        d(Tool::CompleteDiscoveryResultBatch, "complete_discovery_result_batch", DiscoveryExecution, complete_batch_schema(), Blocking, published(26, "Complete Discovery Result Batch", "Atomically complete a claimed Personal Discovery Task into one private ordered result batch.", false, false)),
        d(Tool::ReportDiscoverySourceAvailability, "report_discovery_source_availability", DiscoveryExecution, report_source_availability_schema(), Blocking, published(36, "Report Source Availability", "Report planned source availability and Browser Grant eligibility facts without credentials, cookies, tokens, or browser state.", false, false)),
        d(Tool::GetDiscoveryTaskSourceAvailability, "get_discovery_task_source_availability", PersonalPlanAccess, uuid_schema("task_id"), Blocking, published(37, "Inspect Task Source Availability", "Inspect lease-scoped private source availability facts for one Personal Discovery Task.", true, false)),
        d(Tool::ListAuthenticationNeededNotices, "list_authentication_needed_notices", PersonalDiscoveryManagement, empty_schema(), Blocking, published(38, "List Authentication-needed Notices", "List one-shot private authentication-needed notices for unavailable sources.", true, false)),
        d(Tool::ListDiscoveryResultBatches, "list_discovery_result_batches", PersonalDiscoveryManagement, empty_schema(), Blocking, published(27, "List Discovery Result Batches", "List private Discovery Result Batches for the authenticated User.", true, false)),
        d(Tool::GetDiscoveryResultBatch, "get_discovery_result_batch", PersonalDiscoveryManagement, uuid_schema("batch_id"), Blocking, published(28, "Inspect Discovery Result Batch", "Inspect one private Discovery Result Batch and its Candidate provenance.", true, false)),
        d(Tool::DismissDiscoveryResultBatch, "dismiss_discovery_result_batch", PersonalDiscoveryManagement, uuid_schema("batch_id"), Blocking, published(29, "Dismiss Discovery Result Batch", "Dismiss an entire ready batch without creating item-level learning evidence.", false, true)),
        d(Tool::MarkDiscoveryResultBatchReviewed, "mark_discovery_result_batch_reviewed", PersonalDiscoveryManagement, uuid_schema("batch_id"), Blocking, hidden("Mark Discovery Result Batch Reviewed", "Mark a ready batch reviewed without item-level learning evidence.", false, false)),
        d(Tool::ReviewDiscoveryResultItem, "review_discovery_result_item", PersonalDiscoveryManagement, review_result_item_schema(), Blocking, published(30, "Review Discovery Result Item", "Deliberately save, place, reinforce, reject, or ignore one private Discovery Result Batch item.", false, false)),
        d(Tool::CreatePersonalDiscoverySchedule, "create_personal_discovery_schedule", PersonalDiscoveryManagement, personal_schedule_schema(), Blocking, published(31, "Create Personal Discovery Schedule", "Create a named private Personal Discovery schedule with cadence, optional focus and avoidance, batch size, and delivery mode.", false, false)),
        d(Tool::ListPersonalDiscoverySchedules, "list_personal_discovery_schedules", PersonalPlanAccess, empty_schema(), Blocking, published(32, "List Personal Discovery Schedules", "List private Personal Discovery schedules with inspectable backpressure state.", true, false)),
        d(Tool::GetPersonalDiscoverySchedule, "get_personal_discovery_schedule", PersonalPlanAccess, uuid_schema("schedule_id"), Blocking, published(33, "Inspect Personal Discovery Schedule", "Inspect one private Personal Discovery schedule and its backpressure state.", true, false)),
        d(Tool::UpdatePersonalDiscoverySchedule, "update_personal_discovery_schedule", PersonalDiscoveryManagement, update_personal_schedule_schema(), Blocking, published(34, "Update Personal Discovery Schedule", "Update cadence, temporary intent, batch size, delivery mode, or enabled state for a private schedule.", false, false)),
        d(Tool::DisablePersonalDiscoverySchedule, "disable_personal_discovery_schedule", PersonalDiscoveryManagement, uuid_schema("schedule_id"), Blocking, hidden("Disable Personal Discovery Schedule", "Disable a private schedule without deleting its configuration.", false, true)),
        d(Tool::RemovePersonalDiscoverySchedule, "remove_personal_discovery_schedule", PersonalDiscoveryManagement, uuid_schema("schedule_id"), Blocking, hidden("Remove Personal Discovery Schedule", "Remove a private schedule configuration.", false, true)),
        d(Tool::AttemptDiscoveryResultsReadyNotification, "attempt_discovery_results_ready_notification", PersonalPlanAccess, uuid_schema("batch_id"), Blocking, published(35, "Attempt Results-ready Notification", "Perform the single notify-when-supported attempt for a completed scheduled batch without marking it reviewed.", false, false)),
        d(Tool::GetPodPackage, "get_pod_package", Public, string_schema("pod_slug"), Blocking, published(1, "Read Pod Package", "Read the versioned context, curation instructions, and Source Rules for one Pod.", true, false)),
        d(Tool::ExportPodPackage, "export_pod_package", CapabilityOnly(Capability::PackageManagement), string_schema("pod_slug"), Blocking, hidden("Export Pod Package", "Export a portable Pod Package.", true, false)),
        d(Tool::ImportPodPackage, "import_pod_package", CapabilityOnly(Capability::PackageManagement), object_schema(json!({"pod_slug": {"type": "string"}, "files": {"type": "object"}}), &["pod_slug", "files"]), Blocking, hidden("Import Pod Package", "Import Pod Package files.", false, false)),
        d(Tool::ForkPodPackage, "fork_pod_package", CapabilityOnly(Capability::PackageManagement), fork_package_schema(), Blocking, hidden("Fork Pod Package", "Fork a Pod Package into a new Pod.", false, false)),
        d(Tool::ValidatePodPackage, "validate_pod_package", CapabilityOnly(Capability::PackageManagement), string_schema("pod_slug"), Blocking, hidden("Validate Pod Package", "Validate a Pod Package.", true, false)),
        d(Tool::GetNodeInfo, "get_node_info", Public, empty_schema(), Blocking, hidden("Get Node Info", "Read public node identity information.", true, false)),
        d(Tool::ListTrustedPeers, "list_trusted_peers", CapabilityOnly(Capability::Administration), empty_schema(), Blocking, hidden("List Trusted Peers", "List locally trusted peers.", true, false)),
        d(Tool::AddTrustedPeer, "add_trusted_peer", CapabilityOnly(Capability::Administration), object_schema(json!({"display_name": {"type": "string"}, "base_url": {"type": "string", "format": "uri"}, "public_key": {"type": "string"}}), &["display_name", "base_url", "public_key"]), Blocking, hidden("Add Trusted Peer", "Propose adding a trusted peer.", false, false)),
        d(Tool::SubscribePublicPod, "subscribe_public_pod", CapabilityOnly(Capability::SubscriptionManagement), object_schema(json!({"public_pod_url": {"type": "string", "format": "uri"}}), &["public_pod_url"]), Async, published(9, "Subscribe to Public Pod", "Subscribe to a canonical public Pod URL and import its verified signed history from the Origin Node.", false, false)),
        d(Tool::SynchronizeSubscription, "synchronize_subscription", CapabilityOnly(Capability::SubscriptionManagement), uuid_schema("subscription_id"), Async, published(10, "Synchronize Subscription", "Fetch and apply signed Pod Events from the Origin Node after the Subscription's verified cursor.", false, false)),
        d(Tool::SyncPodWithPeer, "sync_pod_with_peer", CapabilityOnly(Capability::Administration), object_schema(json!({"peer_id": uuid(), "pod_slug": {"type": "string"}}), &["peer_id", "pod_slug"]), Async, hidden("Synchronize Pod with Peer", "Synchronize signed Pod Events from a trusted peer.", false, false)),
        d(Tool::ExportPodEvents, "export_pod_events", Public, string_schema("pod_slug"), Blocking, hidden("Export Pod Events", "Export signed Pod Events.", true, false)),
        d(Tool::ImportPodEvents, "import_pod_events", CapabilityOnly(Capability::Administration), object_schema(json!({"peer_id": uuid(), "events": {"type": "array"}}), &["peer_id", "events"]), Blocking, hidden("Import Pod Events", "Import verified signed Pod Events.", false, false)),
    ])
}

pub(crate) fn definition(name: &str) -> Option<&'static ToolDefinition> {
    definitions()
        .iter()
        .find(|definition| definition.name == name)
}

pub(crate) fn names() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        definitions()
            .iter()
            .map(|definition| definition.name)
            .collect()
    })
}

fn d(
    tool: McpTool,
    name: &'static str,
    availability: ToolAvailability,
    input_schema: Value,
    handler: ToolHandlerKind,
    discovery: DiscoveryMetadata,
) -> ToolDefinition {
    ToolDefinition {
        tool,
        name,
        availability,
        input_schema,
        handler,
        discovery_order: discovery.order,
        title: discovery.title,
        description: discovery.description,
        read_only: discovery.read_only,
        destructive: discovery.destructive,
    }
}

struct DiscoveryMetadata {
    order: Option<usize>,
    title: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
}

fn published(
    order: usize,
    title: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
) -> DiscoveryMetadata {
    DiscoveryMetadata {
        order: Some(order),
        title,
        description,
        read_only,
        destructive,
    }
}

fn hidden(
    title: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
) -> DiscoveryMetadata {
    DiscoveryMetadata {
        order: None,
        title,
        description,
        read_only,
        destructive,
    }
}

fn empty_schema() -> Value {
    object_schema(json!({}), &[])
}
fn uuid() -> Value {
    json!({"type": "string", "format": "uuid"})
}
fn uuid_schema(name: &str) -> Value {
    object_schema(json!({name: uuid()}), &[name])
}
fn string_schema(name: &str) -> Value {
    object_schema(json!({name: {"type": "string"}}), &[name])
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

fn create_pod_schema() -> Value {
    object_schema(
        json!({"name": {"type": "string"}, "slug": {"type": "string"}, "description": {"type": "string"}, "visibility": {"type": "string", "enum": ["private", "invite_only", "public"]}}),
        &["name", "slug", "description", "visibility"],
    )
}
fn route_candidate_schema() -> Value {
    object_schema(
        json!({"candidate_id": uuid(), "pod_id": uuid(), "reason": {"type": "string"}, "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}}),
        &["candidate_id", "pod_id", "reason", "confidence"],
    )
}
fn review_candidate_schema() -> Value {
    object_schema(
        json!({"candidate_id": uuid(), "pod_id": uuid(), "decision": {"type": "string", "enum": ["accept", "reject"]}, "note": {"type": "string"}}),
        &["candidate_id", "pod_id", "decision"],
    )
}
fn task_lease_schema() -> Value {
    object_schema(
        json!({"task_id": uuid(), "lease_seconds": {"type": "integer", "minimum": 1, "maximum": 604800}}),
        &["task_id"],
    )
}
fn immediate_task_schema() -> Value {
    object_schema(
        json!({"pod_id": uuid(), "instructions": {"type": "string"}, "idempotency_key": {"type": "string"}}),
        &["pod_id", "instructions", "idempotency_key"],
    )
}

fn personal_discovery_schema() -> Value {
    object_schema(
        json!({
            "intent": {
                "oneOf": [
                    {"type": "null"},
                    {"type": "object", "properties": {"kind": {"const": "topic"}, "value": {"type": "string"}}, "required": ["kind", "value"], "additionalProperties": false},
                    {"type": "object", "properties": {"kind": {"const": "similar_to_url"}, "value": {"type": "string", "format": "uri"}}, "required": ["kind", "value"], "additionalProperties": false}
                ]
            },
            "result_count": {"type": ["integer", "null"], "minimum": 1, "maximum": 100},
            "idempotency_key": {"type": "string"},
            "browser_grant_eligible_sources": {
                "type": ["array", "null"],
                "items": {"type": "string"},
                "description": "Optional Browser Grant eligibility set that restricts planned sources; never broadened by Taste Profile, Pod Package, or Discovery Leads."
            }
        }),
        &["idempotency_key"],
    )
}

fn personal_schedule_schema() -> Value {
    object_schema(
        json!({
            "name": {"type": "string"},
            "cadence": {"type": "string", "enum": ["hourly", "daily", "weekly", "monthly"]},
            "intent": {
                "type": "object",
                "properties": {
                    "focus_topics": {"type": "array", "items": {"type": "string"}},
                    "avoid_topics": {"type": "array", "items": {"type": "string"}}
                },
                "additionalProperties": false
            },
            "result_count": {"type": ["integer", "null"], "minimum": 1, "maximum": 100},
            "delivery_mode": {"type": "string", "enum": ["notify_when_supported", "queue_only"]}
        }),
        &["name", "cadence", "delivery_mode"],
    )
}

fn update_personal_schedule_schema() -> Value {
    object_schema(
        json!({
            "schedule_id": uuid(),
            "name": {"type": ["string", "null"]},
            "cadence": {"type": ["string", "null"], "enum": ["hourly", "daily", "weekly", "monthly"]},
            "intent": {
                "oneOf": [
                    {"type": "null"},
                    {
                        "type": "object",
                        "properties": {
                            "focus_topics": {"type": "array", "items": {"type": "string"}},
                            "avoid_topics": {"type": "array", "items": {"type": "string"}}
                        },
                        "additionalProperties": false
                    }
                ]
            },
            "result_count": {"type": ["integer", "null"], "minimum": 1, "maximum": 100},
            "delivery_mode": {"type": ["string", "null"], "enum": ["notify_when_supported", "queue_only"]},
            "enabled": {"type": ["boolean", "null"]}
        }),
        &["schedule_id"],
    )
}

fn review_result_item_schema() -> Value {
    object_schema(
        json!({
            "batch_id": uuid(),
            "candidate_id": uuid(),
            "action": {
                "oneOf": [
                    {"type": "object", "properties": {"action": {"const": "save"}}, "required": ["action"], "additionalProperties": false},
                    {"type": "object", "properties": {"action": {"const": "add_to_pod"}, "pod_id": uuid(), "curation_note": {"type": ["string", "null"]}}, "required": ["action", "pod_id"], "additionalProperties": false},
                    {"type": "object", "properties": {"action": {"const": "more_like_this"}}, "required": ["action"], "additionalProperties": false},
                    {"type": "object", "properties": {"action": {"const": "not_for_me"}}, "required": ["action"], "additionalProperties": false},
                    {"type": "object", "properties": {"action": {"const": "ignore"}}, "required": ["action"], "additionalProperties": false}
                ]
            }
        }),
        &["batch_id", "candidate_id", "action"],
    )
}

fn source_availability_item_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "source": {"type": "string"},
            "state": {
                "type": "string",
                "enum": [
                    "available",
                    "authentication_required",
                    "session_expired",
                    "inaccessible",
                    "browser_grant_ineligible"
                ]
            },
            "reason": {"type": "string"}
        },
        "required": ["source", "state"],
        "additionalProperties": false
    })
}

fn report_source_availability_schema() -> Value {
    object_schema(
        json!({
            "task_id": uuid(),
            "reports": {
                "type": "array",
                "items": source_availability_item_schema()
            },
            "browser_grant_eligible_sources": {
                "type": ["array", "null"],
                "items": {"type": "string"}
            }
        }),
        &["task_id", "reports"],
    )
}

fn complete_batch_schema() -> Value {
    object_schema(
        json!({
            "task_id": uuid(),
            "submission_ids": {"type": "array", "items": uuid()},
            "source_availability": {
                "type": "array",
                "items": source_availability_item_schema()
            },
            "browser_grant_eligible_sources": {
                "type": ["array", "null"],
                "items": {"type": "string"}
            }
        }),
        &["task_id", "submission_ids"],
    )
}

fn update_taste_schema() -> Value {
    object_schema(
        json!({
            "interests": {"type": "array", "items": {"type": "string"}},
            "blocked_topics": {"type": "array", "items": {"type": "string"}},
            "blocked_sources": {"type": "array", "items": {"type": "string"}},
            "blocked_source_affinities": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": {"enum": ["source", "publisher", "author_or_account", "community", "referrer_context"]},
                        "value": {"type": "string"}
                    },
                    "required": ["kind", "value"],
                    "additionalProperties": false
                }
            },
            "recurrence_penalty_days": {"type": "integer", "minimum": 0, "maximum": 36500}
        }),
        &[],
    )
}

fn reset_taste_schema() -> Value {
    object_schema(json!({"signal": {"type": ["object", "null"]}}), &[])
}

fn register_harness_schema() -> Value {
    object_schema(
        json!({
            "label": {"type": "string"},
            "kind": {"type": "string", "enum": ["interactive", "unattended"]},
            "capabilities": {"type": "array", "items": {"type": "string"}},
            "pod_ids": {"type": ["array", "null"], "items": uuid()}
        }),
        &["label", "kind", "capabilities"],
    )
}

fn pending_proposal_schema() -> Value {
    object_schema(
        json!({
            "requested_change": {"type": "object"},
            "expires_in_seconds": {"type": "integer", "minimum": 1, "maximum": 604800}
        }),
        &["requested_change", "expires_in_seconds"],
    )
}

fn private_pod_package_schema() -> Value {
    object_schema(
        json!({
            "name": {"type": "string"},
            "slug": {"type": "string"},
            "description": {"type": "string"},
            "package": {
                "type": "object",
                "properties": {
                    "context_md": {"type": "string"}, "skill_md": {"type": "string"},
                    "sources_yaml": {"type": "string"}, "filters_yaml": {"type": "string"},
                    "examples_good_md": {"type": "string"}, "examples_bad_md": {"type": "string"}
                },
                "required": ["context_md", "skill_md", "sources_yaml", "filters_yaml", "examples_good_md", "examples_bad_md"],
                "additionalProperties": false
            }
        }),
        &["name", "slug", "description", "package"],
    )
}

fn fork_package_schema() -> Value {
    object_schema(
        json!({"source_pod_slug": {"type": "string"}, "target": create_pod_schema()}),
        &["source_pod_slug", "target"],
    )
}
fn feed_batch_schema() -> Value {
    object_schema(
        json!({
            "size": {"type": "integer", "minimum": 1, "maximum": 100},
            "recurrence_penalty_days": {"type": "integer", "minimum": 0, "maximum": 36500},
            "feed_mix": {"type": "object", "properties": {
                "high_value_percent": {"type": "integer", "minimum": 0, "maximum": 100},
                "exploration_percent": {"type": "integer", "minimum": 0, "maximum": 100},
                "old_gem_percent": {"type": "integer", "minimum": 0, "maximum": 100},
                "per_pod_cap": {"type": "integer", "minimum": 1},
                "per_source_cap": {"type": "integer", "minimum": 1}
            }, "additionalProperties": false},
            "batch_intent": {"type": "object", "properties": {
                "focus_topics": {"type": "array", "items": {"type": "string"}},
                "avoid_topics": {"type": "array", "items": {"type": "string"}}
            }, "additionalProperties": false}
        }),
        &[],
    )
}
fn feedback_schema() -> Value {
    object_schema(
        json!({"content_item_id": uuid(), "kind": {"type": "string", "enum": ["interesting", "not_for_me", "dismissed", "saved", "block_source", "block_topic"]}, "topic": {"type": "string"}, "reason": {"type": "string"}}),
        &["content_item_id", "kind"],
    )
}

fn candidate_schema() -> Value {
    object_schema(
        json!({
            "source_url": {"type": "string", "format": "uri"},
            "target": {"oneOf": [
                {"type": "object", "properties": {"kind": {"const": "user"}, "learn": {"type": "boolean", "default": true}, "interest_seed_metadata": {"type": "object", "properties": {"publisher": {"type": ["string", "null"]}, "community": {"type": ["string", "null"]}}, "additionalProperties": false}}, "required": ["kind"], "additionalProperties": false},
                {"type": "object", "properties": {"kind": {"const": "pod_placements"}, "placements": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"pod_id": uuid(), "reason": {"type": "string"}, "confidence": {"type": "number", "minimum": 0, "maximum": 1}}, "required": ["pod_id", "reason", "confidence"], "additionalProperties": false}}, "task_context": {"type": ["object", "null"], "properties": {"task_id": uuid(), "package_version": {"type": "integer", "minimum": 1}}, "required": ["task_id", "package_version"], "additionalProperties": false}}, "required": ["kind", "placements"], "additionalProperties": false},
                {"type": "object", "properties": {"kind": {"const": "personal_discovery"}, "task_id": uuid(), "allocation_role": {"type": "string", "enum": ["proven", "adjacent"]}, "source_facts": {"type": "object", "properties": {"publisher": {"type": ["string", "null"]}, "community": {"type": ["string", "null"]}}, "additionalProperties": false}}, "required": ["kind", "task_id", "allocation_role"], "additionalProperties": false}
            ]},
            "source_metadata": {"type": "object", "properties": {"title": {"type": ["string", "null"]}, "author": {"type": ["string", "null"]}, "published_at": {"type": ["string", "null"], "format": "date-time"}}, "additionalProperties": false},
            "permitted_excerpt": {"type": ["string", "null"]}, "summary": {"type": ["string", "null"]},
            "content_type": {"type": "string", "enum": ["article", "video", "audio", "image", "podcast", "repository", "dataset", "other"]},
            "media_references": {"type": "array", "items": {"type": "object", "properties": {"media_type": {"type": "string", "enum": ["image", "video"]}, "url": {"type": "string", "format": "uri"}}, "required": ["media_type", "url"], "additionalProperties": false}},
            "tags": {"type": "array", "items": {"type": "string"}},
            "provenance": {"type": "object", "properties": {"discovered_at": {"type": "string", "format": "date-time"}, "discovery_method": {"type": "string"}, "referrer_url": {"type": ["string", "null"]}}, "required": ["discovered_at", "discovery_method"], "additionalProperties": false},
            "harness_idempotency_key": {"type": "string"}, "client_idempotency_key": {"type": "string"}
        }),
        &[
            "source_url",
            "target",
            "source_metadata",
            "content_type",
            "tags",
            "provenance",
            "harness_idempotency_key",
            "client_idempotency_key",
        ],
    )
}
