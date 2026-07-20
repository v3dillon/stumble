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
    Init,
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
    #[arg(long)]
    pub(super) peer: String,
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
