use clap::{Arg, ArgMatches, Command, ValueHint};
use serde_json::{json, Value};
use std::{path::PathBuf, process::ExitCode};
use stumble_cli::{
    owner_credential_store, paginate, read_json_input, render_text, resolve_existing_data_dir,
    resolve_initialized_data_dir, selected_data_dir, CursorPage, ErrorBody, ErrorEnvelope,
    ExitStatusCategory, OwnerCredentialStore, ResourceDetail, SuccessEnvelope,
};
use stumble_core::{
    new_plaintext_api_token, seed_store, AgentHarnessId, AgentHarnessKind, AgentTools,
    AgentToolsError, AuthContext, HarnessCapability, PendingProposalId, Pod, PodRole,
    RegisterAgentHarnessRequest, StoreError,
};

fn main() -> ExitCode {
    let matches = match cli().try_get_matches() {
        Ok(matches) => matches,
        Err(error) if error.use_stderr() => {
            return fail(
                ErrorBody::new("usage_error", error.to_string()),
                ExitStatusCategory::Usage,
            );
        }
        Err(error) => {
            print!("{error}");
            return ExitCode::SUCCESS;
        }
    };

    let format = matches
        .get_one::<String>("format")
        .map(String::as_str)
        .unwrap_or("json");
    let data_dir =
        match selected_data_dir(matches.get_one::<PathBuf>("data-dir").map(PathBuf::as_path)) {
            Ok(data_dir) => data_dir,
            Err(error) => return fail(error, ExitStatusCategory::Internal),
        };
    let credentials = owner_credential_store();
    match dispatch(&matches, &data_dir, credentials.as_ref()) {
        Ok(data) => succeed(data, format),
        Err((error, category)) => fail(error, category),
    }
}

fn dispatch(
    matches: &ArgMatches,
    selected_data_dir: &std::path::Path,
    credentials: &dyn OwnerCredentialStore,
) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let (path, leaf) = command_path(matches);
    if path == "node init" {
        return initialize_node(selected_data_dir, credentials);
    }

    let data_dir = resolve_existing_data_dir(selected_data_dir)
        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
    if !AgentTools::home_node_is_initialized(&data_dir).map_err(internal_error)? {
        return Err((
            ErrorBody::new(
                "node_not_initialized",
                format!("Home Node is not initialized at {}", data_dir.display()),
            ),
            ExitStatusCategory::ValidationOrConflict,
        ));
    }
    let tools = AgentTools::open_initialized_home_node(&data_dir).map_err(agent_tools_error)?;
    let actor = authenticate_actor(&tools, &data_dir, credentials)?;

    if path == "node show" {
        let node = tools.node_info(&actor).map_err(agent_tools_error)?;
        return Ok(json!({ "data_dir": data_dir, "node": node, "allowed_actions": [] }));
    }
    match path.as_str() {
        "pod list" => {
            let items = tools
                .list_pods_for_harness(&actor)
                .map_err(agent_tools_error)?
                .into_iter()
                .map(pod_result)
                .collect::<Result<Vec<_>, _>>()?;
            return serde_json::to_value(page(items, leaf)?).map_err(internal_error);
        }
        "pod show" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let allowed_actions = tools
                .pod_allowed_actions(&actor, pod.id)
                .map_err(agent_tools_error)?;
            let mut result = pod_result(pod)?;
            result["allowed_actions"] =
                serde_json::to_value(allowed_actions).map_err(internal_error)?;
            return Ok(result);
        }
        "pod subscribe" => {
            let reference = required_string(leaf, "pod")?;
            if reference.starts_with("https://") || reference.starts_with("http://") {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(internal_error)?;
                let result = runtime
                    .block_on(stumble_sync::subscribe_pod_from_url(
                        &tools, &actor, reference,
                    ))
                    .map_err(direct_subscription_error)?;
                return Ok(json!({
                    "pod_id": result.subscription.local_pod_id,
                    "slug": result.subscription.pod_slug,
                    "subscription": result.subscription,
                    "imported_events": result.imported_events,
                }));
            }
            let pod = resolve_pod(&tools, &actor, reference)?;
            let subscription = tools
                .subscribe_local_pod(&actor, pod.id)
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "subscription": subscription,
            }));
        }
        "pod unsubscribe" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let subscription = tools
                .unsubscribe_pod(&actor, pod.id)
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "subscription_id": subscription.id,
                "unsubscribed": true,
            }));
        }
        "pod subscription set" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let is_priority = *leaf.get_one::<bool>("priority").expect("required by clap");
            tools
                .set_priority_subscription(&actor, pod.id, is_priority)
                .map_err(agent_tools_error)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "is_priority": is_priority,
            }));
        }
        "pod role list" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let roles = tools
                .list_pod_roles(&actor, pod.id)
                .map_err(agent_tools_error)?;
            let page = page(roles, leaf)?;
            return Ok(json!({
                "pod_id": pod.id,
                "slug": pod.slug,
                "items": page.items,
                "next_cursor": page.next_cursor,
            }));
        }
        "pod role grant" | "pod role revoke" => {
            let pod = resolve_pod(&tools, &actor, required_string(leaf, "pod")?)?;
            let user_id = required_id::<uuid::Uuid>(leaf, "user-id")?;
            let role = leaf
                .get_one::<PodRole>("role")
                .expect("required by clap")
                .clone();
            let proposal = if path.ends_with(" grant") {
                tools.request_grant_pod_role(&actor, pod.id, user_id, role, chrono::Utc::now())
            } else {
                tools.request_revoke_pod_role(&actor, pod.id, user_id, role, chrono::Utc::now())
            }
            .map_err(agent_tools_error)?;
            return Ok(json!({ "pod_id": pod.id, "slug": pod.slug, "proposal": proposal }));
        }
        "node harness list" => {
            let items = tools
                .list_agent_harnesses(&actor)
                .map_err(agent_tools_error)?;
            return serde_json::to_value(page(items, leaf)?).map_err(internal_error);
        }
        "node harness show" => {
            let id = required_id::<AgentHarnessId>(leaf, "id")?;
            let view = tools.agent_harness(&actor, id).map_err(agent_tools_error)?;
            let allowed_actions = if actor.harness_id.is_none()
                && view.status == stumble_core::AgentHarnessStatus::Active
            {
                vec!["revoke"]
            } else {
                Vec::new()
            };
            return serde_json::to_value(ResourceDetail {
                resource: view,
                allowed_actions,
            })
            .map_err(internal_error);
        }
        "node harness register" => {
            if actor.harness_id.is_some() {
                return Err((
                    ErrorBody::new(
                        "forbidden",
                        "only the Home Node Owner may register an Agent Harness directly",
                    ),
                    ExitStatusCategory::Authorization,
                ));
            }
            let request = RegisterAgentHarnessRequest {
                label: leaf
                    .get_one::<String>("label")
                    .cloned()
                    .expect("required by clap"),
                kind: *leaf
                    .get_one::<AgentHarnessKind>("kind")
                    .expect("required by clap"),
                capabilities: leaf
                    .get_many::<HarnessCapability>("capability")
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect(),
                pod_ids: leaf
                    .get_many::<stumble_core::PodId>("pod-id")
                    .map(|values| values.copied().collect()),
            };
            let issued = tools
                .register_agent_harness(&actor, request)
                .map_err(agent_tools_error)?;
            return Ok(json!({ "harness": issued.harness, "credential": issued.token.expose() }));
        }
        "node harness revoke" => {
            if actor.harness_id.is_some() {
                return Err((
                    ErrorBody::new(
                        "forbidden",
                        "only the Home Node Owner may revoke an Agent Harness directly",
                    ),
                    ExitStatusCategory::Authorization,
                ));
            }
            let id = required_id::<AgentHarnessId>(leaf, "id")?;
            tools
                .revoke_agent_harness(&actor, id)
                .map_err(agent_tools_error)?;
            return Ok(json!({ "id": id, "status": "revoked" }));
        }
        "node proposal list" => {
            let items = tools
                .list_pending_proposals(&actor, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            return serde_json::to_value(page(items, leaf)?).map_err(internal_error);
        }
        "node proposal show" => {
            let id = required_id::<PendingProposalId>(leaf, "id")?;
            let proposal = tools
                .pending_proposal(&actor, id, chrono::Utc::now())
                .map_err(agent_tools_error)?;
            let allowed_actions = tools
                .pending_proposal_allowed_actions(&actor, id)
                .map_err(agent_tools_error)?;
            return serde_json::to_value(ResourceDetail {
                resource: proposal,
                allowed_actions,
            })
            .map_err(internal_error);
        }
        "node proposal approve" => {
            let id = required_id::<PendingProposalId>(leaf, "id")?;
            return serde_json::to_value(
                tools
                    .approve_pending_proposal(&actor, id, chrono::Utc::now())
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error);
        }
        "node proposal reject" => {
            let id = required_id::<PendingProposalId>(leaf, "id")?;
            let reason = leaf
                .get_one::<String>("reason")
                .cloned()
                .expect("required by clap");
            return serde_json::to_value(
                tools
                    .reject_pending_proposal(&actor, id, chrono::Utc::now(), reason)
                    .map_err(agent_tools_error)?,
            )
            .map_err(internal_error);
        }
        _ => {}
    }
    if let Some(input) = leaf.try_get_one::<PathBuf>("input").ok().flatten() {
        let input = read_json_input(input)
            .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
        return Ok(json!({
            "command": path,
            "status": "shell_available",
            "input": input
        }));
    }
    if path.ends_with(" list") || path == "pod list" || path == "pod explore" {
        return serde_json::to_value(CursorPage::<Value>::empty()).map_err(|error| {
            (
                ErrorBody::new("internal_error", error.to_string()),
                ExitStatusCategory::Internal,
            )
        });
    }
    Ok(json!({
        "command": path,
        "status": "shell_available",
        "allowed_actions": []
    }))
}

fn authenticate_actor(
    tools: &AgentTools,
    data_dir: &std::path::Path,
    credentials: &dyn OwnerCredentialStore,
) -> Result<AuthContext, (ErrorBody, ExitStatusCategory)> {
    if let Ok(credential) = std::env::var("STUMBLE_HARNESS_CREDENTIAL") {
        return tools
            .authenticate_token(&credential)
            .map_err(agent_tools_error)?
            .ok_or_else(|| {
                (
                    ErrorBody::new(
                        "invalid_harness_credential",
                        "Agent Harness credential is invalid or revoked",
                    ),
                    ExitStatusCategory::Authorization,
                )
            });
    }
    credentials
        .load(data_dir)
        .map_err(credential_error)?
        .ok_or_else(|| {
            (
                ErrorBody::new(
                    "owner_credential_not_found",
                    "Home Node Owner credential was not found in the credential store",
                ),
                ExitStatusCategory::Authorization,
            )
        })?;
    tools.local_owner_auth_context().map_err(agent_tools_error)
}

fn required_id<T>(matches: &ArgMatches, name: &str) -> Result<T, (ErrorBody, ExitStatusCategory)>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    matches
        .get_one::<String>(name)
        .expect("required by clap")
        .parse::<T>()
        .map_err(|error: T::Err| {
            (
                ErrorBody::new("invalid_id", error.to_string()),
                ExitStatusCategory::ValidationOrConflict,
            )
        })
}

fn required_string<'a>(
    matches: &'a ArgMatches,
    name: &str,
) -> Result<&'a str, (ErrorBody, ExitStatusCategory)> {
    matches
        .get_one::<String>(name)
        .map(String::as_str)
        .ok_or_else(|| internal_error(format!("missing required argument {name}")))
}

fn resolve_pod(
    tools: &AgentTools,
    actor: &AuthContext,
    reference: &str,
) -> Result<Pod, (ErrorBody, ExitStatusCategory)> {
    tools
        .list_pods_for_harness(actor)
        .map_err(agent_tools_error)?
        .into_iter()
        .find(|pod| pod.slug == reference || pod.id.to_string() == reference)
        .ok_or_else(|| {
            (
                ErrorBody::new("not_found", format!("Pod {reference} was not found")),
                ExitStatusCategory::ValidationOrConflict,
            )
        })
}

fn pod_result(pod: Pod) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let pod_id = pod.id;
    let mut result = serde_json::to_value(pod).map_err(internal_error)?;
    result["pod_id"] = json!(pod_id);
    Ok(result)
}

fn direct_subscription_error(
    error: stumble_sync::DirectSubscriptionError,
) -> (ErrorBody, ExitStatusCategory) {
    match error {
        stumble_sync::DirectSubscriptionError::Core(error) => agent_tools_error(error),
        stumble_sync::DirectSubscriptionError::InvalidAddress(error) => agent_tools_error(error),
        error => internal_error(error),
    }
}

fn page<T>(
    items: Vec<T>,
    matches: &ArgMatches,
) -> Result<CursorPage<T>, (ErrorBody, ExitStatusCategory)> {
    let limit = *matches.get_one::<u16>("limit").expect("defaulted by clap");
    let cursor = matches.get_one::<String>("cursor").map(String::as_str);
    paginate(items, limit, cursor)
        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))
}

fn initialize_node(
    selected_data_dir: &std::path::Path,
    credentials: &dyn OwnerCredentialStore,
) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let data_dir = resolve_initialized_data_dir(selected_data_dir)
        .map_err(|error| (error, ExitStatusCategory::ValidationOrConflict))?;
    if AgentTools::home_node_is_initialized(&data_dir).map_err(internal_error)? {
        return Err((
            ErrorBody::new(
                "node_already_initialized",
                format!("Home Node is already initialized at {}", data_dir.display()),
            ),
            ExitStatusCategory::ValidationOrConflict,
        ));
    }

    let credential = new_plaintext_api_token();
    credentials
        .store(&data_dir, &credential)
        .map_err(credential_error)?;
    let tools = match AgentTools::initialize_home_node(&data_dir, seed_store) {
        Ok(tools) => tools,
        Err(error) => {
            let _ = credentials.remove(&data_dir);
            return Err(agent_tools_error(error));
        }
    };
    let owner = tools
        .local_owner_auth_context()
        .map_err(agent_tools_error)?;
    let node = tools.node_info(&owner).map_err(agent_tools_error)?;
    Ok(json!({ "data_dir": data_dir, "node": node }))
}

fn credential_error(error: impl std::fmt::Display) -> (ErrorBody, ExitStatusCategory) {
    (
        ErrorBody::new("credential_store_error", error.to_string()),
        ExitStatusCategory::Internal,
    )
}

fn internal_error(error: impl std::fmt::Display) -> (ErrorBody, ExitStatusCategory) {
    (
        ErrorBody::new("internal_error", error.to_string()),
        ExitStatusCategory::Internal,
    )
}

fn agent_tools_error(error: AgentToolsError) -> (ErrorBody, ExitStatusCategory) {
    match error {
        AgentToolsError::NodeNotInitialized => (
            ErrorBody::new("node_not_initialized", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::NodeAlreadyInitialized => (
            ErrorBody::new("node_already_initialized", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::Forbidden { .. } => (
            ErrorBody::new("forbidden", error.to_string()),
            ExitStatusCategory::Authorization,
        ),
        AgentToolsError::Store(StoreError::Validation(_)) => (
            ErrorBody::new("validation_error", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::Store(StoreError::NotFound(_)) => (
            ErrorBody::new("not_found", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        AgentToolsError::Store(StoreError::Duplicate(_)) | AgentToolsError::BadUrl(_) => (
            ErrorBody::new("validation_error", error.to_string()),
            ExitStatusCategory::ValidationOrConflict,
        ),
        error => internal_error(error),
    }
}

fn command_path(matches: &ArgMatches) -> (String, &ArgMatches) {
    let mut names = Vec::new();
    let mut current = matches;
    while let Some((name, child)) = current.subcommand() {
        names.push(name);
        current = child;
    }
    (names.join(" "), current)
}

fn succeed(data: Value, format: &str) -> ExitCode {
    if format == "text" {
        print!("{}", render_text(&data));
    } else {
        println!(
            "{}",
            serde_json::to_string(&SuccessEnvelope::new(data))
                .expect("JSON values always serialize")
        );
    }
    ExitCode::SUCCESS
}

fn fail(error: ErrorBody, category: ExitStatusCategory) -> ExitCode {
    eprintln!(
        "{}",
        serde_json::to_string(&ErrorEnvelope::new(error))
            .expect("error envelope always serializes")
    );
    ExitCode::from(category as u8)
}

fn cli() -> Command {
    Command::new("stumble")
        .about("Operate a local Stumble Home Node")
        .arg(
            Arg::new("format")
                .long("format")
                .global(true)
                .default_value("json")
                .value_parser(["json", "text"]),
        )
        .arg(
            Arg::new("data-dir")
                .long("data-dir")
                .global(true)
                .env("STUMBLE_DATA_DIR")
                .value_parser(clap::value_parser!(PathBuf))
                .value_hint(ValueHint::DirPath),
        )
        .subcommand_required(true)
        .subcommand(node())
        .subcommand(pod())
        .subcommand(discover())
        .subcommand(feed())
        .subcommand(sync())
}

fn node() -> Command {
    Command::new("node")
        .subcommand_required(true)
        .subcommand(Command::new("init"))
        .subcommand(Command::new("show"))
        .subcommand(harness())
        .subcommand(proposal())
}

fn harness() -> Command {
    Command::new("harness")
        .subcommand_required(true)
        .subcommand(list_leaf("list"))
        .subcommand(Command::new("show").arg(Arg::new("id").required(true)))
        .subcommand(
            Command::new("register")
                .arg(Arg::new("label").long("label").required(true))
                .arg(
                    Arg::new("kind")
                        .long("kind")
                        .required(true)
                        .value_parser(clap::value_parser!(AgentHarnessKind)),
                )
                .arg(
                    Arg::new("capability")
                        .long("capability")
                        .required(true)
                        .action(clap::ArgAction::Append)
                        .value_parser(clap::value_parser!(HarnessCapability)),
                )
                .arg(
                    Arg::new("pod-id")
                        .long("pod-id")
                        .action(clap::ArgAction::Append)
                        .value_parser(clap::value_parser!(stumble_core::PodId)),
                ),
        )
        .subcommand(Command::new("revoke").arg(Arg::new("id").required(true)))
}

fn proposal() -> Command {
    Command::new("proposal")
        .subcommand_required(true)
        .subcommand(list_leaf("list"))
        .subcommand(Command::new("show").arg(Arg::new("id").required(true)))
        .subcommand(Command::new("approve").arg(Arg::new("id").required(true)))
        .subcommand(
            Command::new("reject")
                .arg(Arg::new("id").required(true))
                .arg(Arg::new("reason").long("reason").required(true)),
        )
}

fn pod() -> Command {
    Command::new("pod")
        .subcommand_required(true)
        .subcommand(list_leaf("list"))
        .subcommand(Command::new("show").arg(Arg::new("pod").required(true)))
        .subcommand(Command::new("create"))
        .subcommand(list_leaf("explore"))
        .subcommand(Command::new("subscribe").arg(Arg::new("pod").required(true)))
        .subcommand(Command::new("unsubscribe").arg(Arg::new("pod").required(true)))
        .subcommand(
            Command::new("subscription")
                .subcommand_required(true)
                .subcommand(
                    Command::new("set").arg(Arg::new("pod").required(true)).arg(
                        Arg::new("priority")
                            .long("priority")
                            .required(true)
                            .value_parser(clap::value_parser!(bool)),
                    ),
                ),
        )
        .subcommand(resource("visibility", &["set"]))
        .subcommand(
            Command::new("role")
                .subcommand_required(true)
                .subcommand(list_leaf("list").arg(Arg::new("pod").required(true)))
                .subcommand(role_change("grant"))
                .subcommand(role_change("revoke")),
        )
        .subcommand(resource("content", &["list", "show", "add", "remove"]))
        .subcommand(resource("policy", &["show", "set"]))
        .subcommand(resource(
            "package",
            &["show", "export", "validate", "revise"],
        ))
}

fn role_change(name: &'static str) -> Command {
    Command::new(name)
        .arg(Arg::new("pod").required(true))
        .arg(Arg::new("user-id").long("user-id").required(true))
        .arg(Arg::new("role").long("role").required(true).value_parser(
            clap::builder::TypedValueParser::map(
                clap::builder::PossibleValuesParser::new(["owner", "curator"]),
                |value| match value.as_str() {
                    "owner" => PodRole::Owner,
                    "curator" => PodRole::Curator,
                    _ => unreachable!("constrained by possible values"),
                },
            ),
        ))
}

fn discover() -> Command {
    Command::new("discover")
        .subcommand_required(true)
        .subcommand(resource(
            "task",
            &["list", "show", "claim", "renew", "complete", "fail"],
        ))
        .subcommand(
            resource(
                "candidate",
                &["list", "submit", "show", "evaluate", "route", "review"],
            )
            .mut_subcommand("submit", |command| {
                command.arg(
                    Arg::new("input")
                        .long("input")
                        .required(true)
                        .value_parser(clap::value_parser!(PathBuf))
                        .value_hint(ValueHint::FilePath),
                )
            }),
        )
}

fn feed() -> Command {
    Command::new("feed")
        .subcommand_required(true)
        .subcommand(resource("batch", &["get", "complete"]))
        .subcommand(resource("feedback", &["record"]))
        .subcommand(resource("taste", &["show", "set", "reset"]))
}

fn sync() -> Command {
    Command::new("sync")
        .subcommand_required(true)
        .subcommand(resource("peer", &["list", "add", "remove"]))
        .subcommand(resource("pod", &["run", "status"]))
}

fn resource(name: &'static str, operations: &[&'static str]) -> Command {
    operations.iter().fold(
        Command::new(name).subcommand_required(true),
        |command, operation| {
            command.subcommand(if *operation == "list" {
                list_leaf(operation)
            } else {
                Command::new(operation)
            })
        },
    )
}

fn list_leaf(name: &'static str) -> Command {
    Command::new(name)
        .arg(
            Arg::new("limit")
                .long("limit")
                .default_value("50")
                .value_parser(clap::value_parser!(u16).range(1..=100)),
        )
        .arg(Arg::new("cursor").long("cursor"))
}
