use clap::{Parser, Subcommand, ValueEnum};
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};
use stumble_core::*;

#[derive(Debug, Parser)]
#[command(name = "podctl", about = "Stumble local/admin CLI")]
struct Cli {
    #[arg(long)]
    api: Option<String>,
    #[arg(long)]
    token: Option<String>,
    #[arg(long, env = "STUMBLE_DATA_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    InitNode,
    Serve {
        #[arg(long, default_value = "local")]
        mode: ServeMode,
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: SocketAddr,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        allow_public_dev_tokens: bool,
    },
    CreatePod {
        #[arg(long)]
        name: String,
        #[arg(long)]
        slug: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    CreatePodPackage {
        #[arg(long)]
        name: String,
        #[arg(long)]
        slug: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        from: PathBuf,
    },
    ListPods,
    JoinPod {
        pod: String,
    },
    Submit {
        #[arg(long)]
        pod: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        note: Option<String>,
    },
    SubmitCandidate {
        #[arg(long)]
        from: PathBuf,
    },
    InspectCandidate {
        id: CandidateId,
    },
    AddSource {
        #[arg(long)]
        pod: String,
        #[arg(long)]
        url: String,
    },
    Crawl {
        pod: String,
    },
    Discover {
        #[arg(long)]
        pod: String,
        #[arg(long)]
        query: String,
        #[arg(long = "avoid")]
        avoid: Vec<String>,
    },
    Stumble {
        #[arg(long)]
        pod: String,
        #[arg(long, default_value = "surprise me")]
        query: String,
    },
    Brief {
        #[arg(long = "pod")]
        pods: Vec<String>,
        #[arg(long)]
        query: Option<String>,
    },
    BlockSource {
        source: String,
    },
    BlockTopic {
        topic: String,
    },
    GetSkillPack {
        pod: String,
    },
    ExportSkillPack {
        pod: String,
        out: PathBuf,
    },
    ImportSkillPack {
        pod: String,
        from: PathBuf,
    },
    ForkSkillPack {
        #[arg(long)]
        source_pod: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        slug: String,
    },
    ValidateSkillPack {
        pod: String,
    },
    CreateTenant {
        slug: String,
        name: String,
    },
    CreateApiToken {
        #[arg(long)]
        user: Option<uuid::Uuid>,
        #[arg(long)]
        tenant: Option<String>,
        #[arg(long, default_value = "cli")]
        label: String,
    },
    ListApiTokens,
    RevokeApiToken {
        id: uuid::Uuid,
    },
    RegisterHarness {
        #[arg(long)]
        label: String,
        #[arg(long, value_enum)]
        kind: HarnessKindArg,
        #[arg(long = "capability", required = true)]
        capabilities: Vec<HarnessCapability>,
        #[arg(long = "pod-id")]
        pod_ids: Vec<uuid::Uuid>,
    },
    RevokeHarness {
        id: AgentHarnessId,
    },
    MaterializeDiscoveryTasks,
    ListDiscoveryTasks,
    ListReadyDiscoveryTasks,
    CreateDiscoveryTask {
        pod_id: PodId,
        #[arg(long)]
        instructions: String,
        #[arg(long)]
        idempotency_key: String,
    },
    DiscoveryTaskStatus {
        id: DiscoveryTaskId,
    },
    ClaimDiscoveryTask {
        id: DiscoveryTaskId,
        #[arg(long, default_value = "300")]
        lease_seconds: DiscoveryLeaseSeconds,
    },
    RenewDiscoveryTask {
        id: DiscoveryTaskId,
        #[arg(long, default_value = "300")]
        lease_seconds: DiscoveryLeaseSeconds,
    },
    CompleteDiscoveryTask {
        id: DiscoveryTaskId,
    },
    FailDiscoveryTask {
        id: DiscoveryTaskId,
        #[arg(long)]
        reason: String,
    },
    NodeInfo,
    AddPeer {
        #[arg(long)]
        display_name: String,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        public_key: String,
    },
    ListPeers,
    SyncPeer {
        peer_id: uuid::Uuid,
    },
    SyncPod {
        pod: String,
        peer_id: uuid::Uuid,
    },
    ExportEvents {
        pod: String,
    },
    ImportEvents {
        pod: String,
        peer_id: uuid::Uuid,
        file: PathBuf,
    },
    VerifyEvents {
        pod: String,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum ServeMode {
    Local,
    Hosted,
}

#[derive(Debug, Clone, ValueEnum)]
enum HarnessKindArg {
    Interactive,
    Unattended,
}

impl From<HarnessKindArg> for AgentHarnessKind {
    fn from(value: HarnessKindArg) -> Self {
        match value {
            HarnessKindArg::Interactive => Self::Interactive,
            HarnessKindArg::Unattended => Self::Unattended,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.api.is_some() {
        eprintln!("Remote HTTP mode is documented in README; this MVP CLI executes against the local AgentTools store.");
    }
    let data_dir = cli.data_dir.unwrap_or_else(|| PathBuf::from(".stumble"));
    let tools = AgentTools::open_home_node(data_dir, seed_store)?;
    let store = tools.store();
    let default_ctx = {
        let store = store.read().unwrap();
        AuthContext {
            user_id: store.users.keys().next().copied(),
            tenant_id: None,
            node_id: store.default_node()?.id,
            harness_id: None,
        }
    };
    let ctx = if let Some(token) = cli.token.as_deref() {
        tools
            .authenticate_token(token)?
            .ok_or_else(|| anyhow::anyhow!("invalid or revoked Harness token"))?
    } else {
        default_ctx
    };

    match cli.command {
        Command::InitNode => {
            let info = tools.node_info(&ctx)?;
            print_json(&info)?;
        }
        Command::Serve {
            mode: _,
            bind,
            port,
            allow_public_dev_tokens,
        } => {
            let bind = stumble_api::bind_with_port(bind, port);
            let listener = tokio::net::TcpListener::bind(bind).await?;
            let base_url = format!("http://{}", listener.local_addr()?);
            let dev_tokens_allowed = stumble_api::dev_tokens_allowed_for_bind(
                listener.local_addr()?,
                allow_public_dev_tokens,
            );
            let owner_access_allowed = listener.local_addr()?.ip().is_loopback();
            eprintln!(
                "podctl serving HTTP API at http://{}",
                listener.local_addr()?
            );
            if !dev_tokens_allowed {
                eprintln!("podctl dev token minting disabled because bind address is not loopback");
            }
            if let Some(path) = tools.persistence_path() {
                eprintln!("podctl durable store at {}", path.display());
            }
            axum::serve(
                listener,
                stumble_api::router_with_options(
                    tools,
                    base_url,
                    stumble_api::RouterOptions {
                        dev_tokens_allowed,
                        owner_access_allowed,
                    },
                ),
            )
            .await?;
        }
        Command::CreatePod {
            name,
            slug,
            description,
        } => {
            let pod = tools.create_pod(
                &ctx,
                CreatePodRequest {
                    name,
                    slug,
                    description,
                    visibility: Visibility::Public,
                },
            )?;
            print_json(&pod)?;
        }
        Command::CreatePodPackage {
            name,
            slug,
            description,
            from,
        } => {
            let files = read_portable_package_directory(&from)?;
            let package = pod_package_contents_from_files(&files)?;
            print_json(&tools.create_private_pod_with_package(
                &ctx,
                CreatePrivatePodWithPackageRequest {
                    name,
                    slug,
                    description,
                    package,
                },
            )?)?;
        }
        Command::ListPods => print_json(&tools.list_pods_for_harness(&ctx)?)?,
        Command::JoinPod { pod } => {
            tools.join_pod(&ctx, &pod)?;
            println!("joined {pod}");
        }
        Command::Submit {
            pod,
            url,
            title,
            note,
        } => {
            let submission = tools.submit_link_to_pod(
                &ctx,
                &pod,
                SubmitLinkRequest {
                    url,
                    title,
                    description: None,
                    note,
                    tags: vec![],
                    discovered_by_crawler: false,
                },
            )?;
            print_json(&submission)?;
        }
        Command::SubmitCandidate { from } => {
            let request = serde_json::from_str::<CandidateSubmissionRequest>(
                &std::fs::read_to_string(from)?,
            )?;
            print_json(&tools.submit_candidate(&ctx, request)?)?;
        }
        Command::InspectCandidate { id } => {
            print_json(&tools.inspect_candidate(&ctx, id)?)?;
        }
        Command::AddSource { pod, url } => {
            print_json(&tools.add_source_to_pod(&ctx, &pod, CrawlerSourceType::Rss, url)?)?;
        }
        Command::Crawl { pod } => {
            let manifest = tools.pod_manifest(&ctx, &pod)?;
            print_json(&serde_json::json!({"status":"queued","pod": manifest.pod.slug}))?;
        }
        Command::Discover { pod, query, avoid } => {
            let items = tools.discover_in_pod(
                &ctx,
                &pod,
                DiscoverRequest {
                    query,
                    avoid,
                    limit: 7,
                    mode: DiscoveryMode::DeepMatch,
                    user_id: ctx.user_id,
                },
            )?;
            print_json(&items)?;
        }
        Command::Stumble { pod, query } => {
            let items = tools.discover_in_pod(
                &ctx,
                &pod,
                DiscoverRequest {
                    query,
                    avoid: vec![],
                    limit: 7,
                    mode: DiscoveryMode::Stumble,
                    user_id: ctx.user_id,
                },
            )?;
            print_json(&items)?;
        }
        Command::Brief { pods, query } => {
            let pod_slugs = if pods.is_empty() {
                vec!["beautiful-interfaces".to_string()]
            } else {
                pods
            };
            print_json(&tools.generate_brief(
                &ctx,
                GenerateBriefRequest {
                    pod_slugs,
                    query,
                    user_id: ctx.user_id,
                },
            )?)?;
        }
        Command::BlockSource { source } => {
            tools.block_source(&ctx, source)?;
        }
        Command::BlockTopic { topic } => {
            tools.block_topic(&ctx, topic)?;
        }
        Command::GetSkillPack { pod } => print_json(&tools.get_skill_pack(&ctx, &pod)?)?,
        Command::ExportSkillPack { pod, out } => {
            let export = tools.export_skill_pack(&ctx, &pod)?;
            std::fs::create_dir_all(&out)?;
            for (name, contents) in export.files {
                std::fs::write(out.join(name), contents)?;
            }
            println!("exported {pod}");
        }
        Command::ImportSkillPack { pod, from } => {
            let files = read_portable_package_directory(&from)?;
            print_json(&tools.import_skill_pack(&ctx, &pod, files)?)?;
        }
        Command::ForkSkillPack {
            source_pod,
            name,
            slug,
        } => {
            print_json(&tools.fork_skill_pack(
                &ctx,
                &source_pod,
                CreatePodRequest {
                    name,
                    slug,
                    description: "Forked skill pack pod.".to_string(),
                    visibility: Visibility::Public,
                },
            )?)?;
        }
        Command::ValidateSkillPack { pod } => {
            print_json(&tools.validate_pod_skill_pack(&ctx, &pod)?)?
        }
        Command::CreateTenant { slug, name } => {
            print_json(&tools.create_tenant_as(&ctx, CreateTenantRequest { name, slug })?)?
        }
        Command::CreateApiToken {
            user,
            tenant,
            label,
        } => print_json(&tools.create_dev_token_as(
            &ctx,
            DevTokenRequest {
                user_id: user,
                tenant_slug: tenant,
                label,
            },
        )?)?,
        Command::ListApiTokens => {
            tools.require_harness_capability(&ctx, HarnessCapability::Administration)?;
            let store = tools.store();
            let store = store.read().unwrap();
            print_json(&store.api_tokens.values().collect::<Vec<_>>())?;
        }
        Command::RevokeApiToken { id } => println!("revocation placeholder accepted for {id}"),
        Command::RegisterHarness {
            label,
            kind,
            capabilities,
            pod_ids,
        } => print_json(&tools.register_agent_harness(
            &ctx,
            RegisterAgentHarnessRequest {
                label,
                kind: kind.into(),
                capabilities,
                pod_ids: (!pod_ids.is_empty()).then_some(pod_ids),
            },
        )?)?,
        Command::RevokeHarness { id } => {
            tools.revoke_agent_harness(&ctx, id)?;
            println!("revoked {id}");
        }
        Command::MaterializeDiscoveryTasks => {
            print_json(&tools.materialize_due_discovery_tasks(&ctx, chrono::Utc::now())?)?
        }
        Command::ListDiscoveryTasks => {
            print_json(&tools.list_discovery_tasks(&ctx, chrono::Utc::now())?)?
        }
        Command::ListReadyDiscoveryTasks => {
            print_json(&tools.list_ready_discovery_tasks(&ctx, chrono::Utc::now())?)?
        }
        Command::CreateDiscoveryTask {
            pod_id,
            instructions,
            idempotency_key,
        } => print_json(&tools.create_immediate_discovery_task(
            &ctx,
            CreateImmediateDiscoveryTaskRequest {
                pod_id,
                instructions,
                idempotency_key,
            },
            chrono::Utc::now(),
        )?)?,
        Command::DiscoveryTaskStatus { id } => {
            print_json(&tools.discovery_task_status(&ctx, id, chrono::Utc::now())?)?
        }
        Command::ClaimDiscoveryTask { id, lease_seconds } => {
            print_json(&tools.claim_discovery_task(&ctx, id, chrono::Utc::now(), lease_seconds)?)?
        }
        Command::RenewDiscoveryTask { id, lease_seconds } => print_json(
            &tools.renew_discovery_task_lease(&ctx, id, chrono::Utc::now(), lease_seconds)?,
        )?,
        Command::CompleteDiscoveryTask { id } => {
            print_json(&tools.complete_discovery_task(&ctx, id, chrono::Utc::now())?)?
        }
        Command::FailDiscoveryTask { id, reason } => {
            print_json(&tools.fail_discovery_task(&ctx, id, chrono::Utc::now(), reason)?)?
        }
        Command::NodeInfo => print_json(&tools.node_info(&ctx)?)?,
        Command::AddPeer {
            display_name,
            base_url,
            public_key,
        } => {
            let peer = tools.add_trusted_peer(&ctx, display_name, base_url, public_key)?;
            println!("added peer {}", peer.id);
        }
        Command::ListPeers => {
            tools.require_harness_capability(&ctx, HarnessCapability::Administration)?;
            let store = tools.store();
            let store = store.read().unwrap();
            print_json(&store.trusted_peers.values().collect::<Vec<_>>())?;
        }
        Command::SyncPeer { peer_id } => println!("sync queued for peer {peer_id}"),
        Command::SyncPod { pod, peer_id } => {
            println!("sync queued for pod {pod} with peer {peer_id}")
        }
        Command::ExportEvents { pod } => print_json(&tools.export_pod_events(&ctx, &pod)?)?,
        Command::ImportEvents {
            pod: _,
            peer_id,
            file,
        } => {
            let text = std::fs::read_to_string(file)?;
            let events = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(serde_json::from_str)
                .collect::<Result<Vec<EventLog>, _>>()?;
            let imported = tools.import_pod_events(&ctx, peer_id, events)?;
            println!("imported {imported}");
        }
        Command::VerifyEvents { pod } => {
            let events = tools.export_pod_events(&ctx, &pod)?;
            print_json(
                &serde_json::json!({"pod": pod, "public_events": events.len(), "verified": events.iter().filter(|e| e.verified).count()}),
            )?;
        }
    }

    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn read_portable_package_directory(
    path: &std::path::Path,
) -> anyhow::Result<BTreeMap<String, String>> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !PORTABLE_PACKAGE_FILES.contains(&name.as_str()) {
            anyhow::bail!("unsupported portable Pod Package file {name}");
        }
    }
    let mut files = BTreeMap::new();
    for name in PORTABLE_PACKAGE_FILES {
        files.insert(name.to_string(), std::fs::read_to_string(path.join(name))?);
    }
    Ok(files)
}
