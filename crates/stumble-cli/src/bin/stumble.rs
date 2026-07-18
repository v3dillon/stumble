use clap::{Arg, ArgMatches, Command, ValueHint};
use serde_json::{json, Value};
use std::{path::PathBuf, process::ExitCode};
use stumble_cli::{
    owner_credential_store, read_json_input, render_text, resolve_existing_data_dir,
    resolve_initialized_data_dir, selected_data_dir, CursorPage, ErrorBody, ErrorEnvelope,
    ExitStatusCategory, OwnerCredentialStore, SuccessEnvelope,
};
use stumble_core::{new_plaintext_api_token, seed_store, AgentTools, AgentToolsError};

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
    let _owner_credential = credentials
        .load(&data_dir)
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
    let tools = AgentTools::open_initialized_home_node(&data_dir).map_err(agent_tools_error)?;
    let owner = tools
        .local_owner_auth_context()
        .map_err(agent_tools_error)?;

    if path == "node show" {
        let node = tools.node_info(&owner).map_err(agent_tools_error)?;
        return Ok(json!({ "data_dir": data_dir, "node": node }));
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
        .subcommand(resource("harness", &["list", "show", "register", "revoke"]))
        .subcommand(resource("proposal", &["list", "show", "approve", "reject"]))
}

fn pod() -> Command {
    Command::new("pod")
        .subcommand_required(true)
        .subcommand(list_leaf("list"))
        .subcommand(Command::new("show"))
        .subcommand(Command::new("create"))
        .subcommand(list_leaf("explore"))
        .subcommand(Command::new("subscribe"))
        .subcommand(Command::new("unsubscribe"))
        .subcommand(resource("subscription", &["set"]))
        .subcommand(resource("visibility", &["set"]))
        .subcommand(resource("role", &["list", "grant", "revoke"]))
        .subcommand(resource("content", &["list", "show", "add", "remove"]))
        .subcommand(resource("policy", &["show", "set"]))
        .subcommand(resource(
            "package",
            &["show", "export", "validate", "revise"],
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
