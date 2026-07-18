use super::{
    agent_tools_error, initialize_node, internal_error, open_home_node, page, parse_id, CliResult,
};
use crate::parser::{HarnessWorkflow, NodeWorkflow, ProposalWorkflow};
use serde_json::json;
use std::path::Path;
use stumble_cli::{ErrorBody, ExitStatusCategory, OwnerCredentialStore, ResourceDetail};
use stumble_core::{
    AgentHarnessId, AgentTools, AuthContext, PendingProposalId, RegisterAgentHarnessRequest,
};

pub(super) fn execute(
    command: NodeWorkflow,
    selected_data_dir: &Path,
    credentials: &dyn OwnerCredentialStore,
) -> CliResult {
    match command {
        NodeWorkflow::Init => initialize_node(selected_data_dir, credentials),
        NodeWorkflow::Show => {
            let (data_dir, tools, actor) = open_home_node(selected_data_dir, credentials)?;
            let node = tools.node_info(&actor).map_err(agent_tools_error)?;
            Ok(json!({ "data_dir": data_dir, "node": node, "allowed_actions": [] }))
        }
        NodeWorkflow::Harness { command } => {
            let (_, tools, actor) = open_home_node(selected_data_dir, credentials)?;
            execute_harness(command, &tools, &actor)
        }
        NodeWorkflow::Proposal { command } => {
            let (_, tools, actor) = open_home_node(selected_data_dir, credentials)?;
            execute_proposal(command, &tools, &actor)
        }
    }
}

fn execute_harness(command: HarnessWorkflow, tools: &AgentTools, actor: &AuthContext) -> CliResult {
    match command {
        HarnessWorkflow::List(args) => {
            let items = tools
                .list_agent_harnesses(actor)
                .map_err(agent_tools_error)?;
            serde_json::to_value(page(items, &args)?).map_err(internal_error)
        }
        HarnessWorkflow::Show(args) => {
            let id = parse_id::<AgentHarnessId>(&args.id)?;
            let view = tools.agent_harness(actor, id).map_err(agent_tools_error)?;
            let allowed_actions = if actor.harness_id.is_none()
                && view.status == stumble_core::AgentHarnessStatus::Active
            {
                vec!["revoke"]
            } else {
                Vec::new()
            };
            serde_json::to_value(ResourceDetail {
                resource: view,
                allowed_actions,
            })
            .map_err(internal_error)
        }
        HarnessWorkflow::Register(args) => {
            require_owner_bootstrap(actor, "register")?;
            let issued = tools
                .register_agent_harness(
                    actor,
                    RegisterAgentHarnessRequest {
                        label: args.label,
                        kind: args.kind,
                        capabilities: args.capabilities,
                        pod_ids: args.pod_ids,
                    },
                )
                .map_err(agent_tools_error)?;
            Ok(json!({ "harness": issued.harness, "credential": issued.token.expose() }))
        }
        HarnessWorkflow::Revoke(args) => {
            require_owner_bootstrap(actor, "revoke")?;
            let id = parse_id::<AgentHarnessId>(&args.id)?;
            tools
                .revoke_agent_harness(actor, id)
                .map_err(agent_tools_error)?;
            Ok(json!({ "id": id, "status": "revoked" }))
        }
    }
}

fn execute_proposal(
    command: ProposalWorkflow,
    tools: &AgentTools,
    actor: &AuthContext,
) -> CliResult {
    match command {
        ProposalWorkflow::List(args) => {
            let items = tools
                .list_pending_proposals(actor, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            serde_json::to_value(page(items, &args)?).map_err(internal_error)
        }
        ProposalWorkflow::Show(args) => {
            let id = parse_id::<PendingProposalId>(&args.id)?;
            let proposal = tools
                .pending_proposal(actor, id, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            let allowed_actions = tools
                .pending_proposal_allowed_actions(actor, id)
                .map_err(agent_tools_error)?;
            serde_json::to_value(ResourceDetail {
                resource: proposal,
                allowed_actions,
            })
            .map_err(internal_error)
        }
        ProposalWorkflow::Approve(args) => {
            let id = parse_id::<PendingProposalId>(&args.id)?;
            serde_json::to_value(
                tools
                    .approve_pending_proposal(actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
        ProposalWorkflow::Reject(args) => {
            let id = parse_id::<PendingProposalId>(&args.id)?;
            serde_json::to_value(
                tools
                    .reject_pending_proposal(actor, id, chrono::Utc::now(), args.reason)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error)
        }
    }
}

fn require_owner_bootstrap(
    actor: &AuthContext,
    action: &str,
) -> Result<(), (ErrorBody, ExitStatusCategory)> {
    if actor.harness_id.is_none() {
        return Ok(());
    }
    Err((
        ErrorBody::new(
            "forbidden",
            format!("only the Home Node Owner may {action} an Agent Harness directly"),
        ),
        ExitStatusCategory::Authorization,
    ))
}
