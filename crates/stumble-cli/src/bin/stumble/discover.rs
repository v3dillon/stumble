use super::{
    agent_tools_error, candidate_inspection_result, discovery_lease, discovery_task_detail,
    discovery_task_mutation_result, internal_error, page, parse_id, pod_placement_results,
    resolve_pod, CliResult,
};
use crate::parser::{
    CandidateStatus, CandidateWorkflow, DiscoverWorkflow, PersonalDiscoveryWorkflow,
    PersonalScheduleWorkflow, ReviewDecision, TaskStateFilter, TaskWorkflow,
};
use serde_json::json;
use stumble_cli::{read_json_input, ErrorBody, ExitStatusCategory};
use stumble_core::{
    AgentTools, AuthContext, CandidateConfidence, CandidateId, CandidateReviewState,
    CandidateSubmissionRequest, CompleteDiscoveryResultBatchRequest,
    CreatePersonalDiscoveryScheduleRequest, CurationRationale, DiscoveryPlanId,
    DiscoveryResultBatchId, DiscoveryTask, DiscoveryTaskId, DiscoveryTaskState,
    PersonalDiscoveryScheduleId, PlacementReviewDecision, RequestPersonalDiscovery,
    ReviewDiscoveryResultItemRequest, RouteCandidatePlacementRequest, StoreError,
    UpdatePersonalDiscoveryScheduleRequest,
};

pub(super) fn execute(
    command: DiscoverWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        DiscoverWorkflow::Personal { command } => execute_personal(command, tools, actor),
        DiscoverWorkflow::Task { command } => execute_task(command, tools, actor),
        DiscoverWorkflow::Candidate { command } => execute_candidate(command, tools, actor),
    }
}

fn execute_personal(
    command: PersonalDiscoveryWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        PersonalDiscoveryWorkflow::Readiness => serde_json::to_value(
            tools
                .personal_discovery_readiness(actor)
                .map_err(agent_tools_error)?,
        )
        .map_err(internal_error),
        PersonalDiscoveryWorkflow::Request(args) => {
            let request: RequestPersonalDiscovery = serde_json::from_value(
                read_json_input(&args.input)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?,
            )
            .map_err(|error| {
                (
                    ErrorBody::new("invalid_input", error.to_string()),
                    ExitStatusCategory::ValidationOrConflict,
                )
            })?;
            serde_json::to_value(
                tools
                    .request_personal_discovery(actor, request, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalDiscoveryWorkflow::Plan(args) => {
            let id = parse_id::<DiscoveryPlanId>(&args.id)?;
            serde_json::to_value(tools.discovery_plan(actor, id).map_err(agent_tools_error)?)
                .map_err(internal_error)
        }
        PersonalDiscoveryWorkflow::CompleteBatch(args) => {
            let request: CompleteDiscoveryResultBatchRequest = serde_json::from_value(
                read_json_input(&args.input)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?,
            )
            .map_err(|error| {
                (
                    ErrorBody::new("invalid_input", error.to_string()),
                    ExitStatusCategory::ValidationOrConflict,
                )
            })?;
            serde_json::to_value(
                tools
                    .complete_discovery_result_batch(actor, request, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalDiscoveryWorkflow::Batches => serde_json::to_value(
            tools
                .list_discovery_result_batches(actor)
                .map_err(agent_tools_error)?,
        )
        .map_err(internal_error),
        PersonalDiscoveryWorkflow::Batch(args) => {
            let id = parse_id::<DiscoveryResultBatchId>(&args.id)?;
            serde_json::to_value(
                tools
                    .discovery_result_batch(actor, id)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalDiscoveryWorkflow::DismissBatch(args) => {
            let id = parse_id::<DiscoveryResultBatchId>(&args.id)?;
            serde_json::to_value(
                tools
                    .dismiss_discovery_result_batch(actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalDiscoveryWorkflow::ReviewBatch(args) => {
            let id = parse_id::<DiscoveryResultBatchId>(&args.id)?;
            serde_json::to_value(
                tools
                    .mark_discovery_result_batch_reviewed(actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalDiscoveryWorkflow::ReviewItem(args) => {
            let request: ReviewDiscoveryResultItemRequest = serde_json::from_value(
                read_json_input(&args.input)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?,
            )
            .map_err(|error| {
                (
                    ErrorBody::new("invalid_input", error.to_string()),
                    ExitStatusCategory::ValidationOrConflict,
                )
            })?;
            serde_json::to_value(
                tools
                    .review_discovery_result_item(actor, request, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalDiscoveryWorkflow::Schedule { command } => {
            execute_personal_schedule(command, tools, actor)
        }
        PersonalDiscoveryWorkflow::NotifyBatch(args) => {
            let id = parse_id::<DiscoveryResultBatchId>(&args.id)?;
            serde_json::to_value(
                tools
                    .attempt_discovery_results_ready_notification(actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
    }
}

fn execute_personal_schedule(
    command: PersonalScheduleWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        PersonalScheduleWorkflow::Create(args) => {
            let request: CreatePersonalDiscoveryScheduleRequest = serde_json::from_value(
                read_json_input(&args.input)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?,
            )
            .map_err(|error| {
                (
                    ErrorBody::new("invalid_input", error.to_string()),
                    ExitStatusCategory::ValidationOrConflict,
                )
            })?;
            serde_json::to_value(
                tools
                    .create_personal_discovery_schedule(actor, request, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalScheduleWorkflow::List => serde_json::to_value(
            tools
                .list_personal_discovery_schedules(actor, chrono::Utc::now())
                .map_err(agent_tools_error)?,
        )
        .map_err(internal_error),
        PersonalScheduleWorkflow::Show(args) => {
            let id = parse_id::<PersonalDiscoveryScheduleId>(&args.id)?;
            serde_json::to_value(
                tools
                    .personal_discovery_schedule(actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalScheduleWorkflow::Update(args) => {
            let id = parse_id::<PersonalDiscoveryScheduleId>(&args.id)?;
            let request: UpdatePersonalDiscoveryScheduleRequest = serde_json::from_value(
                read_json_input(&args.input)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?,
            )
            .map_err(|error| {
                (
                    ErrorBody::new("invalid_input", error.to_string()),
                    ExitStatusCategory::ValidationOrConflict,
                )
            })?;
            serde_json::to_value(
                tools
                    .update_personal_discovery_schedule(actor, id, request, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalScheduleWorkflow::Disable(args) => {
            let id = parse_id::<PersonalDiscoveryScheduleId>(&args.id)?;
            serde_json::to_value(
                tools
                    .disable_personal_discovery_schedule(actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        PersonalScheduleWorkflow::Remove(args) => {
            let id = parse_id::<PersonalDiscoveryScheduleId>(&args.id)?;
            serde_json::to_value(
                tools
                    .remove_personal_discovery_schedule(actor, id)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
    }
}

fn execute_task(command: TaskWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        TaskWorkflow::List(args) => {
            let now = chrono::Utc::now();
            let pod = args
                .pod
                .as_deref()
                .map(|reference| resolve_pod(tools, actor, reference))
                .transpose()?;
            let mut items = tools
                .list_discovery_tasks(actor, now)
                .map_err(agent_tools_error)?;
            items.retain(|task| {
                pod.as_ref().is_none_or(|pod| {
                    task.target
                        .pod()
                        .is_some_and(|(pod_id, _)| pod_id == pod.id)
                }) && task_matches_state(task, args.state, now)
            });
            items.sort_by(|left, right| {
                left.due_at
                    .cmp(&right.due_at)
                    .then_with(|| left.id.cmp(&right.id))
            });
            serde_json::to_value(page(items, &args.page)?).map_err(internal_error)
        }
        TaskWorkflow::Show(args) => {
            let id = parse_id::<DiscoveryTaskId>(&args.id)?;
            let now = chrono::Utc::now();
            let task = tools
                .discovery_task_status(actor, id, now)
                .map_err(agent_tools_error)?;
            discovery_task_detail(actor, task, now)
        }
        TaskWorkflow::Claim(args) => {
            let id = parse_id::<DiscoveryTaskId>(&args.id)?;
            let lease = discovery_lease(args.lease_seconds)?;
            let now = chrono::Utc::now();
            let task = tools
                .claim_discovery_task(actor, id, now, lease)
                .map_err(agent_tools_error)?;
            discovery_task_mutation_result(actor, task, now)
        }
        TaskWorkflow::Renew(args) => {
            let id = parse_id::<DiscoveryTaskId>(&args.id)?;
            let lease = discovery_lease(args.lease_seconds)?;
            let now = chrono::Utc::now();
            let task = tools
                .renew_discovery_task_lease(actor, id, now, lease)
                .map_err(agent_tools_error)?;
            discovery_task_mutation_result(actor, task, now)
        }
        TaskWorkflow::Complete(args) => {
            let id = parse_id::<DiscoveryTaskId>(&args.id)?;
            let now = chrono::Utc::now();
            let task = tools
                .complete_discovery_task(actor, id, now)
                .map_err(agent_tools_error)?;
            discovery_task_mutation_result(actor, task, now)
        }
        TaskWorkflow::Fail(args) => {
            let id = parse_id::<DiscoveryTaskId>(&args.id)?;
            let now = chrono::Utc::now();
            let task = tools
                .fail_discovery_task(actor, id, now, args.reason)
                .map_err(agent_tools_error)?;
            discovery_task_mutation_result(actor, task, now)
        }
    }
}

fn execute_candidate(
    command: CandidateWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        CandidateWorkflow::List(args) => {
            let mut items = tools.list_candidates(actor).map_err(agent_tools_error)?;
            items.retain(|candidate| match args.status {
                None => true,
                Some(CandidateStatus::Pending) => {
                    candidate.review_state == CandidateReviewState::Pending
                }
                Some(CandidateStatus::Accepted) => {
                    candidate.review_state == CandidateReviewState::Accepted
                }
            });
            items.sort_by_key(|candidate| (candidate.created_at, candidate.id));
            serde_json::to_value(page(items, &args.page)?).map_err(internal_error)
        }
        CandidateWorkflow::Submit(args) => {
            let mut input = read_json_input(&args.input)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            let input_object = input.as_object_mut().ok_or_else(|| {
                (
                    ErrorBody::new("invalid_input", "Candidate input must be a JSON object"),
                    ExitStatusCategory::ValidationOrConflict,
                )
            })?;
            input_object.insert(
                "harness_idempotency_key".into(),
                json!(args.idempotency_key),
            );
            input_object.insert("client_idempotency_key".into(), json!(args.idempotency_key));
            let request: CandidateSubmissionRequest =
                serde_json::from_value(input).map_err(|error| {
                    (
                        ErrorBody::new("invalid_input", error.to_string()),
                        ExitStatusCategory::ValidationOrConflict,
                    )
                })?;
            serde_json::to_value(
                tools
                    .submit_candidate(actor, request)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        CandidateWorkflow::Show(args) => {
            let candidate_id = parse_id::<CandidateId>(&args.candidate_id)?;
            let inspection = tools
                .inspect_candidate(actor, candidate_id)
                .map_err(agent_tools_error)?;
            candidate_inspection_result(tools, actor, inspection)
        }
        CandidateWorkflow::Evaluate(args) => {
            let candidate_id = parse_id::<CandidateId>(&args.candidate_id)?;
            let result = tools
                .curate_candidate(actor, candidate_id, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            let placements = pod_placement_results(tools, actor, result.placements)?;
            Ok(json!({
                "candidate": result.candidate,
                "content_item": result.content_item,
                "placements": placements,
            }))
        }
        CandidateWorkflow::Route(args) => {
            let candidate_id = parse_id::<CandidateId>(&args.candidate_id)?;
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let confidence = CandidateConfidence::new(args.confidence).map_err(|error| {
                agent_tools_error(StoreError::Validation(error.to_string()).into())
            })?;
            let request = RouteCandidatePlacementRequest::new(pod.id, args.reason, confidence)
                .map_err(|error| agent_tools_error(error.into()))?;
            let placement = tools
                .route_candidate_placement(actor, candidate_id, request, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "placement": placement }))
        }
        CandidateWorkflow::Review(args) => {
            let candidate_id = parse_id::<CandidateId>(&args.candidate_id)?;
            let pod = resolve_pod(tools, actor, &args.pod)?;
            let decision = match args.decision {
                ReviewDecision::Accept => PlacementReviewDecision::Accept,
                ReviewDecision::Reject => PlacementReviewDecision::Reject,
            };
            let note = args
                .note
                .map(CurationRationale::new)
                .transpose()
                .map_err(|error| agent_tools_error(error.into()))?;
            let placement = tools
                .review_candidate_placement(
                    actor,
                    candidate_id,
                    pod.id,
                    decision,
                    note,
                    chrono::Utc::now(),
                )
                .map_err(agent_tools_error)?;
            Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "placement": placement }))
        }
    }
}

pub(super) fn task_matches_state(
    task: &DiscoveryTask,
    state: Option<TaskStateFilter>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    match state {
        None => true,
        Some(TaskStateFilter::Ready) => {
            task.state == DiscoveryTaskState::Pending && task.due_at <= now
        }
        Some(TaskStateFilter::Pending) => task.state == DiscoveryTaskState::Pending,
        Some(TaskStateFilter::Leased) => matches!(task.state, DiscoveryTaskState::Leased(_)),
        Some(TaskStateFilter::Completed) => task.state == DiscoveryTaskState::Completed,
        Some(TaskStateFilter::TerminalFailure) => task.state == DiscoveryTaskState::TerminalFailure,
    }
}
