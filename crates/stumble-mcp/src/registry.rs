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
    pub(crate) capability: Option<HarnessCapability>,
    pub(crate) input_schema: Value,
    pub(crate) handler: ToolHandlerKind,
    pub(crate) discovery_order: Option<usize>,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
    pub(crate) read_only: bool,
    pub(crate) destructive: bool,
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
    use ToolHandlerKind::{Async, Blocking};

    static DEFINITIONS: OnceLock<Vec<ToolDefinition>> = OnceLock::new();
    DEFINITIONS.get_or_init(|| vec![
        d(Tool::ListPods, "list_pods", None, empty_schema(), Blocking, published(0, "List Pods", "List the Pods visible to this Agent Harness.", true, false)),
        d(Tool::GetFeedBatch, "get_feed_batch", Some(Capability::FeedRead), feed_batch_schema(), Blocking, published(18, "Get Personal Feed", "Return a stable finite Feed Batch with provenance, explanations, and allowed actions.", false, false)),
        d(Tool::CompleteFeedBatch, "complete_feed_batch", Some(Capability::FeedRead), uuid_schema("batch_id"), Blocking, published(19, "Complete Feed Batch", "Mark a finite Feed Batch complete after presentation.", false, false)),
        d(Tool::RecordFeedFeedback, "record_feed_feedback", Some(Capability::Feedback), feedback_schema(), Blocking, published(20, "Record Feed Feedback", "Record an explicit private Feedback Signal for a delivered Content Item.", false, false)),
        d(Tool::SetPrioritySubscription, "set_priority_subscription", Some(Capability::SubscriptionManagement), object_schema(json!({"pod_id": uuid(), "is_priority": {"type": "boolean"}}), &["pod_id", "is_priority"]), Blocking, hidden("Set Priority Subscription", "Change whether a Subscription is prioritized in Feed composition.", false, false)),
        d(Tool::GetTasteProfile, "get_taste_profile", Some(Capability::FeedRead), empty_schema(), Blocking, hidden("Get Taste Profile", "Read the private inspectable Taste Profile.", true, false)),
        d(Tool::UpdateTasteProfile, "update_taste_profile", Some(Capability::Feedback), update_taste_schema(), Blocking, hidden("Update Taste Profile", "Update explicit private Taste Profile settings.", false, false)),
        d(Tool::ResetLearnedTaste, "reset_learned_taste", Some(Capability::Feedback), reset_taste_schema(), Blocking, hidden("Reset Learned Taste", "Reset learned private Taste Profile weights.", false, true)),
        d(Tool::RetractInterestSeed, "retract_interest_seed", Some(Capability::Feedback), uuid_schema("candidate_id"), Blocking, published(21, "Retract Interest Seed", "Retract one submitted reference's private learning contribution without deleting the reference.", false, true)),
        d(Tool::RegisterAgentHarness, "register_agent_harness", Some(Capability::Administration), register_harness_schema(), Blocking, hidden("Register Agent Harness", "Register a scoped Agent Harness grant.", false, false)),
        d(Tool::RevokeAgentHarness, "revoke_agent_harness", Some(Capability::Administration), uuid_schema("harness_id"), Blocking, hidden("Revoke Agent Harness", "Revoke an Agent Harness grant.", false, true)),
        d(Tool::CreatePendingProposal, "create_pending_proposal", Some(Capability::PodCuration), pending_proposal_schema(), Blocking, hidden("Create Pending Proposal", "Propose a sensitive change for independent approval.", false, false)),
        d(Tool::GetPendingProposal, "get_pending_proposal", Some(Capability::Approval), uuid_schema("proposal_id"), Blocking, published(3, "Inspect Pending Proposal", "Inspect a sensitive change before making an independent approval decision.", true, false)),
        d(Tool::ApprovePendingProposal, "approve_pending_proposal", Some(Capability::Approval), uuid_schema("proposal_id"), Blocking, published(4, "Approve Pending Proposal", "Approve a sensitive change as a separately authorized interactive Harness.", false, false)),
        d(Tool::RejectPendingProposal, "reject_pending_proposal", Some(Capability::Approval), object_schema(json!({"proposal_id": uuid(), "reason": {"type": "string"}}), &["proposal_id", "reason"]), Blocking, published(5, "Reject Pending Proposal", "Reject a sensitive change as a separately authorized interactive Agent Harness.", false, false)),
        d(Tool::CreatePod, "create_pod", Some(Capability::PodCuration), create_pod_schema(), Blocking, published(2, "Create Pod", "Create an isolated Pod. Public exposure returns a Pending Proposal and does not take effect until independently approved.", false, false)),
        d(Tool::RouteCandidate, "route_candidate", Some(Capability::PodCuration), route_candidate_schema(), Blocking, published(6, "Route Candidate", "Propose an evidence-backed Pod Placement for a private Candidate in an authorized local Pod.", false, false)),
        d(Tool::ReviewCandidatePlacement, "review_candidate_placement", Some(Capability::PodCuration), review_candidate_schema(), Blocking, published(7, "Review Candidate Placement", "Accept or reject one pending Pod Placement under existing curation authority.", false, false)),
        d(Tool::ListPodContent, "list_pod_content", Some(Capability::FeedRead), uuid_schema("pod_id"), Blocking, published(8, "List Pod Content", "List the complete accepted Content Item stream for one Pod without private Candidate data.", true, false)),
        d(Tool::CreatePrivatePodWithPackage, "create_private_pod_with_package", Some(Capability::PodCuration), private_pod_package_schema(), Blocking, hidden("Create Private Pod with Package", "Create a private Pod with its initial Pod Package.", false, false)),
        d(Tool::JoinPod, "join_pod", Some(Capability::SubscriptionManagement), string_schema("pod_slug"), Blocking, hidden("Join Pod", "Subscribe to a local Pod by slug.", false, false)),
        d(Tool::SubmitCandidate, "submit_candidate", Some(Capability::CandidateSubmission), candidate_schema(), Blocking, published(11, "Save Discovered Link", "Submit one externally discovered link with source metadata, provenance, and proposed Pod Placements. This creates a private Candidate, not an Accepted Placement.", false, false)),
        d(Tool::InspectCandidate, "inspect_candidate", Some(Capability::CandidateSubmission), uuid_schema("candidate_id"), Blocking, published(12, "Inspect Candidate", "Inspect a private Candidate and its independent provenance records.", true, false)),
        d(Tool::MaterializeDiscoveryTasks, "materialize_discovery_tasks", Some(Capability::DiscoveryTasks), empty_schema(), Blocking, hidden("Materialize Discovery Tasks", "Materialize due Discovery Tasks from Source Rules.", false, false)),
        d(Tool::ListDiscoveryTasks, "list_discovery_tasks", Some(Capability::DiscoveryTasks), empty_schema(), Blocking, hidden("List Discovery Tasks", "List Discovery Tasks visible to this Harness.", true, false)),
        d(Tool::ListReadyDiscoveryTasks, "list_ready_discovery_tasks", Some(Capability::DiscoveryTasks), empty_schema(), Blocking, published(13, "List Ready Discovery Tasks", "List due discovery work that this Agent Harness is authorized to claim.", true, false)),
        d(Tool::CreateImmediateDiscoveryTask, "create_immediate_discovery_task", Some(Capability::DiscoveryTasks), immediate_task_schema(), Blocking, published(14, "Request Discovery", "Create retry-safe discovery work for a Pod from the user's current instructions.", false, false)),
        d(Tool::DiscoveryTaskStatus, "discovery_task_status", Some(Capability::DiscoveryTasks), uuid_schema("task_id"), Blocking, hidden("Discovery Task Status", "Inspect one Discovery Task.", true, false)),
        d(Tool::ClaimDiscoveryTask, "claim_discovery_task", Some(Capability::DiscoveryTasks), task_lease_schema(), Blocking, published(15, "Claim Discovery Task", "Claim a ready Discovery Task with an exclusive, expiring lease.", false, false)),
        d(Tool::RenewDiscoveryTask, "renew_discovery_task", Some(Capability::DiscoveryTasks), task_lease_schema(), Blocking, hidden("Renew Discovery Task", "Renew an owned Discovery Task lease.", false, false)),
        d(Tool::CompleteDiscoveryTask, "complete_discovery_task", Some(Capability::DiscoveryTasks), uuid_schema("task_id"), Blocking, published(16, "Complete Discovery Task", "Mark a claimed Discovery Task complete after its Candidates have been submitted.", false, false)),
        d(Tool::FailDiscoveryTask, "fail_discovery_task", Some(Capability::DiscoveryTasks), object_schema(json!({"task_id": uuid(), "reason": {"type": "string"}}), &["task_id", "reason"]), Blocking, published(17, "Fail Discovery Task", "Record a failed Discovery Task attempt with an inspectable reason.", false, true)),
        d(Tool::GetPodPackage, "get_pod_package", None, string_schema("pod_slug"), Blocking, published(1, "Read Pod Package", "Read the versioned context, curation instructions, and Source Rules for one Pod.", true, false)),
        d(Tool::ExportPodPackage, "export_pod_package", Some(Capability::PackageManagement), string_schema("pod_slug"), Blocking, hidden("Export Pod Package", "Export a portable Pod Package.", true, false)),
        d(Tool::ImportPodPackage, "import_pod_package", Some(Capability::PackageManagement), object_schema(json!({"pod_slug": {"type": "string"}, "files": {"type": "object"}}), &["pod_slug", "files"]), Blocking, hidden("Import Pod Package", "Import Pod Package files.", false, false)),
        d(Tool::ForkPodPackage, "fork_pod_package", Some(Capability::PackageManagement), fork_package_schema(), Blocking, hidden("Fork Pod Package", "Fork a Pod Package into a new Pod.", false, false)),
        d(Tool::ValidatePodPackage, "validate_pod_package", Some(Capability::PackageManagement), string_schema("pod_slug"), Blocking, hidden("Validate Pod Package", "Validate a Pod Package.", true, false)),
        d(Tool::GetNodeInfo, "get_node_info", None, empty_schema(), Blocking, hidden("Get Node Info", "Read public node identity information.", true, false)),
        d(Tool::ListTrustedPeers, "list_trusted_peers", Some(Capability::Administration), empty_schema(), Blocking, hidden("List Trusted Peers", "List locally trusted peers.", true, false)),
        d(Tool::AddTrustedPeer, "add_trusted_peer", Some(Capability::Administration), object_schema(json!({"display_name": {"type": "string"}, "base_url": {"type": "string", "format": "uri"}, "public_key": {"type": "string"}}), &["display_name", "base_url", "public_key"]), Blocking, hidden("Add Trusted Peer", "Propose adding a trusted peer.", false, false)),
        d(Tool::SubscribePublicPod, "subscribe_public_pod", Some(Capability::SubscriptionManagement), object_schema(json!({"public_pod_url": {"type": "string", "format": "uri"}}), &["public_pod_url"]), Async, published(9, "Subscribe to Public Pod", "Subscribe to a canonical public Pod URL and import its verified signed history from the Origin Node.", false, false)),
        d(Tool::SynchronizeSubscription, "synchronize_subscription", Some(Capability::SubscriptionManagement), uuid_schema("subscription_id"), Async, published(10, "Synchronize Subscription", "Fetch and apply signed Pod Events from the Origin Node after the Subscription's verified cursor.", false, false)),
        d(Tool::SyncPodWithPeer, "sync_pod_with_peer", Some(Capability::Administration), object_schema(json!({"peer_id": uuid(), "pod_slug": {"type": "string"}}), &["peer_id", "pod_slug"]), Async, hidden("Synchronize Pod with Peer", "Synchronize signed Pod Events from a trusted peer.", false, false)),
        d(Tool::ExportPodEvents, "export_pod_events", None, string_schema("pod_slug"), Blocking, hidden("Export Pod Events", "Export signed Pod Events.", true, false)),
        d(Tool::ImportPodEvents, "import_pod_events", Some(Capability::Administration), object_schema(json!({"peer_id": uuid(), "events": {"type": "array"}}), &["peer_id", "events"]), Blocking, hidden("Import Pod Events", "Import verified signed Pod Events.", false, false)),
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
    capability: Option<HarnessCapability>,
    input_schema: Value,
    handler: ToolHandlerKind,
    discovery: DiscoveryMetadata,
) -> ToolDefinition {
    ToolDefinition {
        tool,
        name,
        capability,
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
                {"type": "object", "properties": {"kind": {"const": "pod_placements"}, "placements": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"pod_id": uuid(), "reason": {"type": "string"}, "confidence": {"type": "number", "minimum": 0, "maximum": 1}}, "required": ["pod_id", "reason", "confidence"], "additionalProperties": false}}, "task_context": {"type": ["object", "null"], "properties": {"task_id": uuid(), "package_version": {"type": "integer", "minimum": 1}}, "required": ["task_id", "package_version"], "additionalProperties": false}}, "required": ["kind", "placements"], "additionalProperties": false}
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
