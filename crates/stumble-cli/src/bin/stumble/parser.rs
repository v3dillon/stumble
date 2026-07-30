use clap::ValueHint;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use stumble_core::{
    AgentHarnessKind, DiscoveryLeaseSeconds, FeedbackKind, HarnessCapability, PodRole, Visibility,
};

#[derive(Parser)]
#[command(
    name = "stumble",
    about = "Operate a local Stumble Home Node",
    disable_help_subcommand = true
)]
pub(super) struct Cli {
    #[arg(long, global = true, default_value = "json", value_parser = ["json", "text"])]
    pub(super) format: String,
    #[arg(long, global = true, env = "STUMBLE_DATA_DIR", value_hint = ValueHint::DirPath)]
    pub(super) data_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub(super) workflow: Workflow,
}

#[derive(Subcommand)]
pub(super) enum Workflow {
    /// Add a shared link to a Pod and your Feed in one step
    Add(AddArgs),
    Node {
        #[command(subcommand)]
        command: NodeWorkflow,
    },
    Pod {
        #[command(subcommand)]
        command: PodWorkflow,
    },
    Discover {
        #[command(subcommand)]
        command: DiscoverWorkflow,
    },
    Feed {
        #[command(subcommand)]
        command: FeedWorkflow,
    },
    Sync {
        #[command(subcommand)]
        command: SyncWorkflow,
    },
}

#[derive(Subcommand)]
pub(super) enum NodeWorkflow {
    /// Initialize a new Home Node (empty by default; pass --demo for seed fixtures)
    Init {
        /// Seed demo users, peers, hosted tenant, and a development API token
        #[arg(long)]
        demo: bool,
    },
    Show,
    Harness {
        #[command(subcommand)]
        command: HarnessWorkflow,
    },
    Proposal {
        #[command(subcommand)]
        command: ProposalWorkflow,
    },
}

#[derive(Subcommand)]
pub(super) enum HarnessWorkflow {
    List(ListArgs),
    Show(IdArgs),
    Register(RegisterHarnessArgs),
    Revoke(IdArgs),
}

#[derive(Subcommand)]
pub(super) enum ProposalWorkflow {
    List(ListArgs),
    Show(IdArgs),
    Approve(IdArgs),
    Reject(RejectProposalArgs),
}

#[derive(Subcommand)]
pub(super) enum PodWorkflow {
    List(ListArgs),
    Show(PodArgs),
    Create(CreatePodArgs),
    Explore(ExploreArgs),
    /// Make a Pod public and print its shareable federation URL
    Publish(PublishPodArgs),
    /// Subscribe to a local Pod by slug or a public Pod by its federation URL
    Subscribe(PodArgs),
    Unsubscribe(PodArgs),
    Subscription {
        #[command(subcommand)]
        command: SubscriptionWorkflow,
    },
    Visibility {
        #[command(subcommand)]
        command: VisibilityWorkflow,
    },
    Role {
        #[command(subcommand)]
        command: RoleWorkflow,
    },
    Content {
        #[command(subcommand)]
        command: ContentWorkflow,
    },
    Policy {
        #[command(subcommand)]
        command: PolicyWorkflow,
    },
    Package {
        #[command(subcommand)]
        command: PackageWorkflow,
    },
    /// Install a Pod's SKILL.md into an agent harness skills directory
    Skill {
        #[command(subcommand)]
        command: SkillWorkflow,
    },
}

#[derive(Subcommand)]
pub(super) enum SkillWorkflow {
    /// Write or update the Pod's skill folder inside a skills directory
    Install(SkillInstallArgs),
}

#[derive(Args)]
pub(super) struct SkillInstallArgs {
    pub(super) pod: String,
    /// Skills directory to install into (default: ~/.agents/skills)
    #[arg(long, env = "STUMBLE_SKILLS_DIR", value_hint = ValueHint::DirPath)]
    pub(super) dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub(super) enum SubscriptionWorkflow {
    Set(SubscriptionSetArgs),
}

#[derive(Subcommand)]
pub(super) enum VisibilityWorkflow {
    Set(VisibilitySetArgs),
}

#[derive(Subcommand)]
pub(super) enum RoleWorkflow {
    List(RoleListArgs),
    Grant(RoleChangeArgs),
    Revoke(RoleChangeArgs),
}

#[derive(Subcommand)]
pub(super) enum ContentWorkflow {
    List(PodListArgs),
    Show(ContentItemArgs),
    Add(ContentAddArgs),
    Remove(ContentRemoveArgs),
}

#[derive(Subcommand)]
pub(super) enum PolicyWorkflow {
    Show(PodArgs),
    Set(PolicySetArgs),
}

#[derive(Subcommand)]
pub(super) enum PackageWorkflow {
    Show(PackageShowArgs),
    Export(PackageExportArgs),
    Validate(PackageDirectoryArgs),
    Revise(PackageReviseArgs),
}

#[derive(Subcommand)]
pub(super) enum DiscoverWorkflow {
    Personal {
        #[command(subcommand)]
        command: PersonalDiscoveryWorkflow,
    },
    Task {
        #[command(subcommand)]
        command: TaskWorkflow,
    },
    Candidate {
        #[command(subcommand)]
        command: CandidateWorkflow,
    },
}

#[derive(Subcommand)]
pub(super) enum PersonalDiscoveryWorkflow {
    Readiness,
    Request(InputArgs),
    Plan(IdArgs),
    CompleteBatch(InputArgs),
    Batches,
    Batch(IdArgs),
    DismissBatch(IdArgs),
    ReviewBatch(IdArgs),
    ReviewItem(InputArgs),
    Schedule {
        #[command(subcommand)]
        command: PersonalScheduleWorkflow,
    },
    NotifyBatch(IdArgs),
}

#[derive(Subcommand)]
pub(super) enum PersonalScheduleWorkflow {
    Create(InputArgs),
    List,
    Show(IdArgs),
    Update(UpdateScheduleArgs),
    Disable(IdArgs),
    Remove(IdArgs),
}

#[derive(Args)]
pub(super) struct UpdateScheduleArgs {
    pub(super) id: String,
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub(super) input: PathBuf,
}

#[derive(Subcommand)]
pub(super) enum TaskWorkflow {
    List(TaskListArgs),
    Show(IdArgs),
    Claim(LeaseTaskArgs),
    Renew(LeaseTaskArgs),
    Complete(IdArgs),
    Fail(FailTaskArgs),
}

#[derive(Subcommand)]
pub(super) enum CandidateWorkflow {
    List(CandidateListArgs),
    Submit(CandidateSubmitArgs),
    Show(CandidateIdArgs),
    Evaluate(CandidateIdArgs),
    Route(CandidateRouteArgs),
    Review(CandidateReviewArgs),
}

#[derive(Subcommand)]
pub(super) enum FeedWorkflow {
    Batch {
        #[command(subcommand)]
        command: BatchWorkflow,
    },
    Feedback {
        #[command(subcommand)]
        command: FeedbackWorkflow,
    },
    Taste {
        #[command(subcommand)]
        command: TasteWorkflow,
    },
}

#[derive(Subcommand)]
pub(super) enum BatchWorkflow {
    Get(OptionalInputArgs),
    Complete(IdArgs),
}

#[derive(Subcommand)]
pub(super) enum FeedbackWorkflow {
    Record(FeedbackRecordArgs),
}

#[derive(Subcommand)]
pub(super) enum TasteWorkflow {
    Show,
    Set(InputArgs),
    Reset(OptionalInputArgs),
    Retract(CandidateIdArgs),
}

#[derive(Subcommand)]
pub(super) enum SyncWorkflow {
    Peer {
        #[command(subcommand)]
        command: PeerWorkflow,
    },
    Pod {
        #[command(subcommand)]
        command: SyncPodWorkflow,
    },
    /// Inspect and synchronize User-controlled Bootstrap endpoints.
    Bootstrap {
        #[command(subcommand)]
        command: BootstrapWorkflow,
    },
    /// Inspect discovery readiness and manage Discovery Peer relationships.
    Discovery {
        #[command(subcommand)]
        command: DiscoveryWorkflow,
    },
}

#[derive(Subcommand)]
pub(super) enum DiscoveryWorkflow {
    /// Report discovery readiness including Bootstrap-outage degraded mode.
    Status,
    /// Manage opt-in inbound Discovery Peer announcement serving.
    Serve {
        #[command(subcommand)]
        command: DiscoveryServeWorkflow,
    },
    /// List the rotating outbound Discovery Peer set with health state.
    Peers,
    /// Enable or disable automatic outbound peer gossip.
    Gossip(DiscoveryGossipArgs),
    /// Learn Discovery Peers and synchronize their Announcement Streams.
    Run(DiscoveryRunArgs),
}

#[derive(Subcommand)]
pub(super) enum DiscoveryServeWorkflow {
    /// Show the opt-in inbound serving state.
    Show,
    /// Enable inbound announcement serving after reachability verification.
    Enable(DiscoveryServeEnableArgs),
    /// Disable inbound serving without affecting outbound discovery.
    Disable,
}

#[derive(Args)]
pub(super) struct DiscoveryServeEnableArgs {
    /// Publicly reachable endpoint to advertise for announcement serving
    #[arg(long)]
    pub(super) public_endpoint: String,
}

#[derive(Args)]
pub(super) struct DiscoveryGossipArgs {
    #[arg(long, required = true, action = clap::ArgAction::Set)]
    pub(super) enabled: bool,
}

#[derive(Args)]
pub(super) struct DiscoveryRunArgs {
    /// Learn peer samples and rotate the outbound set before syncing
    #[arg(long)]
    pub(super) learn: bool,
    /// Skip Announcement Stream synchronization (learn only)
    #[arg(long, requires = "learn")]
    pub(super) no_sync: bool,
}

#[derive(Subcommand)]
pub(super) enum BootstrapWorkflow {
    /// List configured Bootstrap endpoints in order.
    List,
    /// Report endpoints with cursor, last success, and typed failure.
    Status,
    /// Synchronize Announcement Streams from enabled Bootstrap endpoints.
    Run,
    /// Add a replaceable Bootstrap endpoint.
    Add(BootstrapAddArgs),
    /// Disable a Bootstrap endpoint without deleting audit state.
    Disable(BootstrapIdArgs),
    /// Re-enable a Bootstrap endpoint.
    Enable(BootstrapIdArgs),
    /// Remove a Bootstrap endpoint from configuration.
    Remove(BootstrapIdArgs),
}

#[derive(Args)]
pub(super) struct BootstrapAddArgs {
    #[arg(long)]
    pub(super) label: String,
    #[arg(long)]
    pub(super) base_url: String,
}

#[derive(Args)]
pub(super) struct BootstrapIdArgs {
    pub(super) id: String,
}

#[derive(Subcommand)]
pub(super) enum PeerWorkflow {
    List(ListArgs),
    Add(PeerAddArgs),
    Remove(PeerRemoveArgs),
}

#[derive(Subcommand)]
pub(super) enum SyncPodWorkflow {
    Run(SyncPodRunArgs),
    Status(PodArgs),
}

#[derive(Args)]
pub(super) struct AddArgs {
    /// Source URL to add
    pub(super) url: String,
    /// Target Pod slug; the default private `saved` Pod is used when omitted
    #[arg(long)]
    pub(super) pod: Option<String>,
    /// Source title when known
    #[arg(long)]
    pub(super) title: Option<String>,
    /// Short generated understanding of the source
    #[arg(long)]
    pub(super) summary: Option<String>,
    /// Excerpt that source policy permits Stumble to retain
    #[arg(long)]
    pub(super) excerpt: Option<String>,
    /// Descriptive subject tag; repeatable
    #[arg(long = "tag")]
    pub(super) tags: Vec<String>,
    /// Why this belongs in the Pod
    #[arg(long)]
    pub(super) note: Option<String>,
}

#[derive(Args)]
pub(super) struct ListArgs {
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..=100))]
    pub(super) limit: u16,
    #[arg(long)]
    pub(super) cursor: Option<String>,
}

#[derive(Args)]
pub(super) struct IdArgs {
    pub(super) id: String,
}

#[derive(Args)]
pub(super) struct PodArgs {
    pub(super) pod: String,
}

#[derive(Args)]
pub(super) struct PodListArgs {
    pub(super) pod: String,
    #[command(flatten)]
    pub(super) page: ListArgs,
}

#[derive(Args)]
pub(super) struct RegisterHarnessArgs {
    #[arg(long)]
    pub(super) label: String,
    #[arg(long)]
    pub(super) kind: AgentHarnessKind,
    #[arg(long = "capability", required = true)]
    pub(super) capabilities: Vec<HarnessCapability>,
    #[arg(long = "pod-id")]
    pub(super) pod_ids: Option<Vec<stumble_core::PodId>>,
}

#[derive(Args)]
pub(super) struct RejectProposalArgs {
    pub(super) id: String,
    #[arg(long)]
    pub(super) reason: String,
}

#[derive(Args)]
pub(super) struct CreatePodArgs {
    #[arg(long)]
    pub(super) name: String,
    #[arg(long)]
    pub(super) slug: String,
    #[arg(long)]
    pub(super) description: Option<String>,
    #[arg(long, value_parser = parse_visibility)]
    pub(super) visibility: Visibility,
    #[arg(long, value_hint = ValueHint::DirPath, conflicts_with = "from_pod")]
    pub(super) package: Option<PathBuf>,
    #[arg(long)]
    pub(super) from_pod: Option<String>,
}

#[derive(Args)]
pub(super) struct PublishPodArgs {
    pub(super) pod: String,
    /// Public base URL this node is served at (e.g. https://node.example);
    /// used to build the share URL and issue the Pod Announcement
    #[arg(long, env = "STUMBLE_BASE_URL")]
    pub(super) base_url: Option<String>,
}

#[derive(Args)]
pub(super) struct ExploreArgs {
    #[arg(long)]
    pub(super) query: Option<String>,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u8).range(0..=10))]
    pub(super) sample_size: u8,
    #[command(flatten)]
    pub(super) page: ListArgs,
}

#[derive(Args)]
pub(super) struct SubscriptionSetArgs {
    pub(super) pod: String,
    #[arg(long, required = true, action = clap::ArgAction::Set)]
    pub(super) priority: bool,
}

#[derive(Args)]
pub(super) struct VisibilitySetArgs {
    pub(super) pod: String,
    #[arg(long, value_parser = parse_visibility)]
    pub(super) visibility: Visibility,
}

#[derive(Args)]
pub(super) struct RoleListArgs {
    pub(super) pod: String,
    #[command(flatten)]
    pub(super) page: ListArgs,
}

#[derive(Args)]
pub(super) struct RoleChangeArgs {
    pub(super) pod: String,
    #[arg(long)]
    pub(super) user_id: String,
    #[arg(long, value_parser = parse_role)]
    pub(super) role: PodRole,
}

#[derive(Args)]
pub(super) struct ContentItemArgs {
    pub(super) pod: String,
    pub(super) content_item_id: String,
}

#[derive(Args)]
pub(super) struct ContentAddArgs {
    pub(super) pod: String,
    pub(super) content_item_id: String,
    #[arg(long)]
    pub(super) note: Option<String>,
}

#[derive(Args)]
pub(super) struct ContentRemoveArgs {
    pub(super) pod: String,
    pub(super) content_item_id: String,
    #[arg(long)]
    pub(super) reason: String,
}

#[derive(Args)]
pub(super) struct PolicySetArgs {
    pub(super) pod: String,
    #[arg(long)]
    pub(super) mode: PolicyMode,
    #[arg(long, required_if_eq_any = [("mode", "assisted"), ("mode", "autonomous")])]
    pub(super) confidence_threshold: Option<f32>,
}

#[derive(Args)]
pub(super) struct PackageShowArgs {
    pub(super) pod: String,
    #[arg(long, value_parser = clap::value_parser!(i32).range(1..))]
    pub(super) version: Option<i32>,
}

#[derive(Args)]
pub(super) struct PackageExportArgs {
    pub(super) pod: String,
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub(super) output: PathBuf,
}

#[derive(Args)]
pub(super) struct PackageDirectoryArgs {
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub(super) package: PathBuf,
}

#[derive(Args)]
pub(super) struct PackageReviseArgs {
    pub(super) pod: String,
    #[arg(long, value_parser = clap::value_parser!(i32).range(1..))]
    pub(super) base_version: i32,
    #[arg(long, value_hint = ValueHint::DirPath)]
    pub(super) package: PathBuf,
}

#[derive(Args)]
pub(super) struct TaskListArgs {
    #[arg(long)]
    pub(super) pod: Option<String>,
    #[arg(long)]
    pub(super) state: Option<TaskStateFilter>,
    #[command(flatten)]
    pub(super) page: ListArgs,
}

#[derive(Args)]
pub(super) struct LeaseTaskArgs {
    pub(super) id: String,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..=u64::from(DiscoveryLeaseSeconds::MAX)))]
    pub(super) lease_seconds: u64,
}

#[derive(Args)]
pub(super) struct FailTaskArgs {
    pub(super) id: String,
    #[arg(long)]
    pub(super) reason: String,
}

#[derive(Args)]
pub(super) struct CandidateListArgs {
    #[arg(long)]
    pub(super) status: Option<CandidateStatus>,
    #[command(flatten)]
    pub(super) page: ListArgs,
}

#[derive(Args)]
pub(super) struct CandidateSubmitArgs {
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub(super) input: PathBuf,
    #[arg(long)]
    pub(super) idempotency_key: String,
}

#[derive(Args)]
pub(super) struct CandidateIdArgs {
    pub(super) candidate_id: String,
}

#[derive(Args)]
pub(super) struct CandidateRouteArgs {
    pub(super) candidate_id: String,
    pub(super) pod: String,
    #[arg(long)]
    pub(super) reason: String,
    #[arg(long)]
    pub(super) confidence: f32,
}

#[derive(Args)]
pub(super) struct CandidateReviewArgs {
    pub(super) candidate_id: String,
    pub(super) pod: String,
    #[arg(long)]
    pub(super) decision: ReviewDecision,
    #[arg(long)]
    pub(super) note: Option<String>,
}

#[derive(Args)]
pub(super) struct OptionalInputArgs {
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub(super) input: Option<PathBuf>,
}

#[derive(Args)]
pub(super) struct InputArgs {
    #[arg(long, value_hint = ValueHint::FilePath)]
    pub(super) input: PathBuf,
}

#[derive(Args)]
pub(super) struct FeedbackRecordArgs {
    pub(super) content_item_id: String,
    #[arg(long)]
    pub(super) kind: FeedbackKind,
    #[arg(long)]
    pub(super) topic: Option<String>,
    #[arg(long)]
    pub(super) reason: Option<String>,
}

#[derive(Args)]
pub(super) struct PeerAddArgs {
    #[arg(long)]
    pub(super) node_id: String,
    #[arg(long)]
    pub(super) display_name: String,
    #[arg(long)]
    pub(super) base_url: String,
    #[arg(long)]
    pub(super) public_key: String,
}

#[derive(Args)]
pub(super) struct PeerRemoveArgs {
    pub(super) peer_id: String,
}

#[derive(Args)]
pub(super) struct SyncPodRunArgs {
    pub(super) pod: String,
    /// Trusted peer to sync through; defaults to the subscription's Origin Node
    #[arg(long)]
    pub(super) peer: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum PolicyMode {
    Manual,
    Assisted,
    Autonomous,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum TaskStateFilter {
    Ready,
    Pending,
    Leased,
    Completed,
    TerminalFailure,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum CandidateStatus {
    Pending,
    Accepted,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum ReviewDecision {
    Accept,
    Reject,
}

fn parse_visibility(value: &str) -> Result<Visibility, String> {
    match value {
        "private" => Ok(Visibility::Private),
        "invite-only" => Ok(Visibility::InviteOnly),
        "public" => Ok(Visibility::Public),
        _ => Err("expected private, invite-only, or public".to_string()),
    }
}

fn parse_role(value: &str) -> Result<PodRole, String> {
    match value {
        "owner" => Ok(PodRole::Owner),
        "curator" => Ok(PodRole::Curator),
        _ => Err("expected owner or curator".to_string()),
    }
}
