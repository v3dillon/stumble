use clap::{Arg, ArgMatches, Command, ValueHint};
use serde_json::{json, Value};
use std::{path::PathBuf, process::ExitCode};
use stumble_cli::{
    read_json_input, render_text, CursorPage, ErrorBody, ErrorEnvelope, ExitStatusCategory,
    SuccessEnvelope,
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
    match dispatch(&matches) {
        Ok(data) => succeed(data, format),
        Err((error, category)) => fail(error, category),
    }
}

fn dispatch(matches: &ArgMatches) -> Result<Value, (ErrorBody, ExitStatusCategory)> {
    let (path, leaf) = command_path(matches);
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
