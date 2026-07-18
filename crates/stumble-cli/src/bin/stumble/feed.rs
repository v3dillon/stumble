use super::{
    agent_tools_error, feed_batch_result, json_input_error, parse_id, taste_profile_result,
    CliResult,
};
use crate::parser::{BatchWorkflow, FeedWorkflow, FeedbackWorkflow, TasteWorkflow};
use serde_json::json;
use stumble_cli::{read_json_input, ExitStatusCategory};
use stumble_core::{
    AgentTools, AuthContext, ContentItemId, FeedBatchRequest, ResetLearnedTasteRequest,
    UpdateTasteProfileRequest,
};

pub(super) fn execute(command: FeedWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        FeedWorkflow::Batch { command } => match command {
            BatchWorkflow::Get(args) => {
                let request = if let Some(input) = args.input {
                    let value = read_json_input(&input)
                        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
                    serde_json::from_value::<FeedBatchRequest>(value).map_err(json_input_error)?
                } else {
                    FeedBatchRequest::new(7).expect("the default Feed Batch size is valid")
                };
                let batch = tools
                    .get_feed_batch(actor, request, chrono::Utc::now())
                    .map_err(agent_tools_error)?;
                feed_batch_result(tools, actor, batch)
            }
            BatchWorkflow::Complete(args) => {
                let id = parse_id::<uuid::Uuid>(&args.id)?;
                let batch = tools
                    .complete_feed_batch(actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?;
                feed_batch_result(tools, actor, batch)
            }
        },
        FeedWorkflow::Feedback { command } => match command {
            FeedbackWorkflow::Record(args) => {
                let content_item_id = parse_id::<ContentItemId>(&args.content_item_id)?;
                let feedback_state = tools
                    .record_feed_feedback(
                        actor,
                        content_item_id,
                        args.kind,
                        args.topic,
                        args.reason,
                        chrono::Utc::now(),
                    )
                    .map_err(agent_tools_error)?;
                Ok(json!({
                    "content_item_id": content_item_id,
                    "feedback_state": feedback_state,
                    "allowed_actions": [
                        "save",
                        "more_like_this",
                        "less_like_this",
                        "dismiss",
                        "block_source",
                        "block_topic",
                    ],
                }))
            }
        },
        FeedWorkflow::Taste { command } => match command {
            TasteWorkflow::Show => {
                let profile = tools.taste_profile(actor).map_err(agent_tools_error)?;
                taste_profile_result(profile)
            }
            TasteWorkflow::Set(args) => {
                let value = read_json_input(&args.input)
                    .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
                let request = serde_json::from_value::<UpdateTasteProfileRequest>(value)
                    .map_err(json_input_error)?;
                let profile = tools
                    .update_taste_profile(actor, request)
                    .map_err(agent_tools_error)?;
                taste_profile_result(profile)
            }
            TasteWorkflow::Reset(args) => {
                let request = if let Some(input) = args.input {
                    let value = read_json_input(&input)
                        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
                    serde_json::from_value::<ResetLearnedTasteRequest>(value)
                        .map_err(json_input_error)?
                } else {
                    ResetLearnedTasteRequest::all()
                };
                let profile = tools
                    .reset_learned_taste(actor, request)
                    .map_err(agent_tools_error)?;
                taste_profile_result(profile)
            }
        },
    }
}
