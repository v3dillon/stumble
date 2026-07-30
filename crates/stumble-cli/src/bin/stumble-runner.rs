use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use stumble_core::{AgentTools, AgentToolsError, DiscoveryTaskOrigin};

#[derive(Debug, Parser)]
#[command(about = "Unified, harness-neutral Stumble runtime")]
struct Args {
    #[arg(long)]
    config: PathBuf,
    #[command(subcommand)]
    command: RunnerCommand,
}

#[derive(Debug, Subcommand)]
enum RunnerCommand {
    /// Serve authenticated MCP requests from one loopback process.
    Serve,
    /// Serve one authenticated Stumble MCP profile over stdio.
    Mcp { profile: String },
    /// Materialize due discovery work and dispatch an external agent harness.
    Discovery {
        profile: String,
        /// Override the worker's default agent command.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Evaluate pending Candidate placements for one curator profile.
    Curate { profile: String },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunnerConfig {
    version: u8,
    data_dir: PathBuf,
    #[serde(default = "default_bind")]
    bind: SocketAddr,
    credentials: BTreeMap<String, CredentialProfile>,
    #[serde(default)]
    agents: BTreeMap<String, AgentCommand>,
    #[serde(default)]
    mcp: BTreeMap<String, CredentialSelection>,
    #[serde(default)]
    workers: BTreeMap<String, WorkerProfile>,
    #[serde(default)]
    curators: BTreeMap<String, CredentialSelection>,
    #[serde(default)]
    schedules: BTreeMap<String, Schedule>,
    /// Interval for passive network sync (Bootstrap Announcement Streams and
    /// outbound Discovery Peer streams). Zero disables it.
    #[serde(default = "default_network_sync_seconds")]
    network_sync_every_seconds: u64,
}

fn default_network_sync_seconds() -> u64 {
    900
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialProfile {
    command: ProcessCommand,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialSelection {
    credential: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessCommand {
    program: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentCommand {
    program: PathBuf,
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerProfile {
    credential: String,
    agent: String,
    prompt: String,
    #[serde(default)]
    working_directory: Option<PathBuf>,
    #[serde(default)]
    event_path: Option<PathBuf>,
    #[serde(default)]
    skip_source_rule_indexes: Vec<usize>,
    #[serde(default)]
    agent_env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Schedule {
    every_seconds: u64,
    #[serde(flatten)]
    workflow: ScheduledWorkflow,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "workflow", rename_all = "snake_case")]
enum ScheduledWorkflow {
    Discovery {
        profile: String,
        #[serde(default)]
        agent: Option<String>,
    },
    Curate {
        profile: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = load_config(&args.config)?;
    match args.command {
        RunnerCommand::Serve => serve_all_mcp(&config).await,
        RunnerCommand::Mcp { profile } => serve_mcp(&config, &profile).await,
        RunnerCommand::Discovery { profile, agent } => {
            run_discovery(&config, &profile, agent.as_deref())
        }
        RunnerCommand::Curate { profile } => run_curator(&config, &profile),
    }
}

fn default_bind() -> SocketAddr {
    "127.0.0.1:8790".parse().expect("valid default bind")
}

fn load_config(path: &Path) -> anyhow::Result<RunnerConfig> {
    let bytes = fs::read(path).with_context(|| format!("read runner config {}", path.display()))?;
    let config: RunnerConfig = serde_yaml::from_slice(&bytes)
        .with_context(|| format!("parse runner config {}", path.display()))?;
    if config.version != 1 {
        bail!("unsupported runner config version {}", config.version);
    }
    Ok(config)
}

fn named<'a, T>(values: &'a BTreeMap<String, T>, kind: &str, name: &str) -> anyhow::Result<&'a T> {
    values
        .get(name)
        .with_context(|| format!("unknown {kind} profile {name:?}"))
}

fn credential(config: &RunnerConfig, name: &str) -> anyhow::Result<String> {
    let profile = named(&config.credentials, "credential", name)?;
    let output = configured_command(&profile.command)
        .output()
        .with_context(|| format!("run credential command for {name:?}"))?;
    if !output.status.success() {
        bail!(
            "credential command for {name:?} exited with {}",
            output.status
        );
    }
    let token = String::from_utf8(output.stdout).context("credential output is not UTF-8")?;
    let token = token.trim().to_owned();
    if token.is_empty() {
        bail!("credential command for {name:?} returned an empty token");
    }
    Ok(token)
}

fn configured_command(spec: &ProcessCommand) -> Command {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args).envs(&spec.env);
    command
}

async fn serve_mcp(config: &RunnerConfig, profile: &str) -> anyhow::Result<()> {
    let selection = named(&config.mcp, "MCP", profile)?;
    let token = credential(config, &selection.credential)?;
    let data_dir = config.data_dir.clone();
    stumble_mcp::serve_stdio(
        move || {
            let tools = AgentTools::open_initialized_home_node(&data_dir)
                .with_context(|| format!("open Home Node at {}", data_dir.display()))?;
            let context = tools
                .authenticate_token(&token)?
                .context("invalid or revoked Harness token")?;
            Ok((tools, context))
        },
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
    .await
}

async fn serve_all_mcp(config: &RunnerConfig) -> anyhow::Result<()> {
    if !config.bind.ip().is_loopback() {
        bail!("runner MCP daemon must bind to loopback");
    }
    let tools = AgentTools::open_initialized_home_node(&config.data_dir)
        .with_context(|| format!("open Home Node at {}", config.data_dir.display()))?;
    let router = stumble_api::router_with_options(
        tools.clone(),
        format!("http://{}", config.bind),
        stumble_api::RouterOptions {
            owner_access_allowed: false,
        },
    )
    .merge(stumble_mcp::streamable_http_router(tools.clone()));
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("bind unified runner at {}", config.bind))?;
    start_schedules(config, tools.clone())?;
    start_network_sync(config, tools);
    axum::serve(listener, router)
        .await
        .context("serve unified runner")
}

fn start_schedules(config: &RunnerConfig, tools: AgentTools) -> anyhow::Result<()> {
    for (name, schedule) in &config.schedules {
        if schedule.every_seconds == 0 {
            bail!("schedule {name:?} every_seconds must be positive");
        }
        let name = name.clone();
        let schedule = schedule.clone();
        let config = config.clone();
        let tools = tools.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(schedule.every_seconds));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let run_config = config.clone();
                let run_tools = tools.clone();
                let workflow = schedule.workflow.clone();
                let result = tokio::task::spawn_blocking(move || match workflow {
                    ScheduledWorkflow::Discovery { profile, agent } => run_discovery_with_tools(
                        &run_config,
                        &run_tools,
                        &profile,
                        agent.as_deref(),
                    ),
                    ScheduledWorkflow::Curate { profile } => {
                        run_curator_with_tools(&run_config, &run_tools, &profile)
                    }
                })
                .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => eprintln!("schedule {name:?} failed: {error:#}"),
                    Err(error) => eprintln!("schedule {name:?} task failed: {error}"),
                }
            }
        });
    }
    Ok(())
}

/// Ticks the passive discovery loop: pull Bootstrap Announcement Streams and
/// outbound Discovery Peer streams so the local catalog of public Pods stays
/// current without any manual `stumble sync` invocations.
fn start_network_sync(config: &RunnerConfig, tools: AgentTools) {
    if config.network_sync_every_seconds == 0 {
        return;
    }
    let every = std::time::Duration::from_secs(config.network_sync_every_seconds);
    let handle = tokio::runtime::Handle::current();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let tools = tools.clone();
            let handle = handle.clone();
            let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                let _ = tools.refresh_if_stale();
                let actor = tools.local_owner_auth_context()?;
                let now = chrono::Utc::now();
                let bootstrap_client =
                    stumble_api::ReqwestAnnouncementStreamClient::new(handle.clone());
                let report = tools.sync_bootstrap_endpoints(&actor, &bootstrap_client, now)?;
                if report.retained_announcements > 0 || report.retained_withdrawals > 0 {
                    eprintln!(
                        "network sync retained {} announcement(s), {} withdrawal(s)",
                        report.retained_announcements, report.retained_withdrawals
                    );
                }
                let peer_client =
                    stumble_api::ReqwestDiscoveryPeerStreamClient::new(handle.clone());
                let _ = tools.sync_outbound_discovery_peers(&actor, &peer_client, now)?;
                // Re-assert current signed state for published Pods: renews
                // Announcement Leases and propagates content changes, since
                // announcements bind the latest federated event pointer.
                let refreshed = tools.refresh_origin_pod_announcements(&actor, now)?;
                if !refreshed.is_empty() {
                    let endpoints: Vec<_> = tools
                        .list_bootstrap_endpoints(&actor)?
                        .into_iter()
                        .filter(|endpoint| endpoint.enabled)
                        .collect();
                    for announcement in &refreshed {
                        for endpoint in &endpoints {
                            if let Err(reason) = handle.block_on(
                                stumble_api::submit_pod_announcement_to_bootstrap(
                                    &endpoint.base_url,
                                    announcement,
                                ),
                            ) {
                                eprintln!(
                                    "re-announce {} to {} failed: {reason}",
                                    announcement.pod_slug, endpoint.base_url
                                );
                            }
                        }
                    }
                }
                Ok(())
            })
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!("network sync failed: {error:#}"),
                Err(error) => eprintln!("network sync task failed: {error}"),
            }
        }
    });
}

fn run_discovery(
    config: &RunnerConfig,
    profile_name: &str,
    agent_override: Option<&str>,
) -> anyhow::Result<()> {
    let worker = named(&config.workers, "worker", profile_name)?;
    let agent_name = agent_override.unwrap_or(&worker.agent);
    validate_agent(named(&config.agents, "agent", agent_name)?)?;
    let tools = AgentTools::open_initialized_home_node(&config.data_dir)
        .with_context(|| format!("open Home Node at {}", config.data_dir.display()))?;
    run_discovery_with_tools(config, &tools, profile_name, agent_override)
}

fn run_discovery_with_tools(
    config: &RunnerConfig,
    tools: &AgentTools,
    profile_name: &str,
    agent_override: Option<&str>,
) -> anyhow::Result<()> {
    let worker = named(&config.workers, "worker", profile_name)?;
    let agent_name = agent_override.unwrap_or(&worker.agent);
    let agent = named(&config.agents, "agent", agent_name)?;
    validate_agent(agent)?;

    let token = credential(config, &worker.credential)?;
    let context = tools
        .authenticate_token(&token)?
        .context("invalid or revoked Harness token")?;
    let now = chrono::Utc::now();
    if let Err(error) = tools.materialize_due_discovery_tasks(&context, now) {
        if !matches!(error, AgentToolsError::Forbidden { .. }) {
            return Err(error.into());
        }
    }
    let mut tasks = tools.list_ready_discovery_tasks(&context, now)?;
    tasks.sort_by_key(|task| (task.due_at, task.id));
    let schedule_backpressure = tools
        .list_personal_discovery_schedules(&context, now)
        .unwrap_or_default();
    tasks.retain(|task| match task.origin {
        DiscoveryTaskOrigin::Scheduled { source_rule_index } => {
            !worker.skip_source_rule_indexes.contains(&source_rule_index)
        }
        _ => true,
    });
    let event = json!({
        "type": if tasks.is_empty() { "discovery_idle" } else { "discovery_ready" },
        "tasks": tasks,
        "schedule_backpressure": schedule_backpressure,
    });
    let event_bytes = serde_json::to_vec(&event)?;
    let event_path = worker.event_path.clone().unwrap_or_else(|| {
        config
            .data_dir
            .join(format!("{profile_name}-discovery.json"))
    });
    write_private_atomic(&event_path, &event_bytes)?;

    if tasks.is_empty() {
        return Ok(());
    }
    run_agent(agent, worker, &token, &event_bytes)
}

fn validate_agent(agent: &AgentCommand) -> anyhow::Result<()> {
    if !agent
        .args
        .iter()
        .any(|argument| argument.contains("{prompt}"))
    {
        bail!("agent command args must contain the {{prompt}} placeholder");
    }
    Ok(())
}

fn run_agent(
    agent: &AgentCommand,
    worker: &WorkerProfile,
    worker_credential: &str,
    event: &[u8],
) -> anyhow::Result<()> {
    let working_directory = worker
        .working_directory
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(working_directory).with_context(|| {
        format!(
            "create agent working directory {}",
            working_directory.display()
        )
    })?;
    let working_directory_text = working_directory.to_string_lossy();
    let args = agent.args.iter().map(|argument| {
        argument
            .replace("{prompt}", &worker.prompt)
            .replace("{working_directory}", &working_directory_text)
    });
    let mut command = Command::new(&agent.program);
    command.env_clear();
    for name in ["HOME", "PATH", "TMPDIR", "LANG", "LC_ALL"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.envs(&agent.env);
    for (name, value) in &worker.agent_env {
        command.env(
            name,
            value.replace("{credential}", worker_credential).replace(
                "{bearer_credential}",
                &format!("Bearer {worker_credential}"),
            ),
        );
    }
    let mut child = command
        .args(args)
        .current_dir(working_directory)
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("launch agent {}", agent.program.display()))?;
    child
        .stdin
        .take()
        .context("open agent stdin")?
        .write_all(event)?;
    let status = child.wait().context("wait for agent command")?;
    if !status.success() {
        bail!("agent command exited with {status}");
    }
    Ok(())
}

fn run_curator(config: &RunnerConfig, profile_name: &str) -> anyhow::Result<()> {
    let tools = AgentTools::open_initialized_home_node(&config.data_dir)
        .with_context(|| format!("open Home Node at {}", config.data_dir.display()))?;
    run_curator_with_tools(config, &tools, profile_name)
}

fn run_curator_with_tools(
    config: &RunnerConfig,
    tools: &AgentTools,
    profile_name: &str,
) -> anyhow::Result<()> {
    let curator = named(&config.curators, "curator", profile_name)?;
    let token = credential(config, &curator.credential)?;
    let context = tools
        .authenticate_token(&token)?
        .context("invalid or revoked Harness token")?;
    let mut evaluated = 0usize;
    for candidate in tools.list_candidates(&context)? {
        if !tools
            .inspect_candidate(&context, candidate.id)?
            .placements
            .is_empty()
        {
            continue;
        }
        tools.curate_candidate(&context, candidate.id, chrono::Utc::now())?;
        evaluated += 1;
    }
    if evaluated > 0 {
        println!("{}", json!({ "evaluated": evaluated }));
    }
    Ok(())
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().context("event path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("event"),
        uuid::Uuid::now_v7()
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
    }
    #[cfg(not(unix))]
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}
