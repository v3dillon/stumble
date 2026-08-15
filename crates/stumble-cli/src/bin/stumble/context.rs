use super::{agent_tools_error, internal_error, json_input_error, CliResult};
use crate::parser::ContextWorkflow;
use stumble_cli::{read_json_input, ExitStatusCategory};
use stumble_core::{AgentTools, AuthContext, SetUserContextRequest};

pub(super) fn execute(
    command: ContextWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        ContextWorkflow::Show => serde_json::to_value(
            tools
                .user_context_packet(actor)
                .map_err(agent_tools_error)?,
        )
        .map_err(internal_error),
        ContextWorkflow::Set(args) => {
            let value = read_json_input(&args.input)
                .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
            let request =
                serde_json::from_value::<SetUserContextRequest>(value).map_err(json_input_error)?;
            tools
                .set_user_context(actor, request, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            serde_json::to_value(
                tools
                    .user_context_packet(actor)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
    }
}
