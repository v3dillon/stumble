use anyhow::Context;
use clap::{Parser, ValueEnum};
use std::{net::SocketAddr, path::PathBuf};
use stumble_core::AgentTools;
use stumble_mcp::{serve_stdio, streamable_http_router};
use tracing::error;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(about = "Authenticated MCP server for Stumble")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8790")]
    bind: SocketAddr,
    #[arg(long, env = "STUMBLE_DATA_DIR")]
    data_dir: PathBuf,
    #[arg(long, value_enum, default_value_t = Transport::Http)]
    transport: Transport,
    #[arg(long, env = "STUMBLE_MCP_TOKEN", hide_env_values = true)]
    token: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Transport {
    Http,
    Stdio,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("stumble_mcp=info")),
        )
        .init();
    let args = Args::parse();
    match args.transport {
        Transport::Http => serve_http(args).await,
        Transport::Stdio => serve_standard_io(args).await,
    }
}

async fn serve_http(args: Args) -> anyhow::Result<()> {
    anyhow::ensure!(args.bind.ip().is_loopback(), "MCP must bind to loopback");

    let tools = AgentTools::open_initialized_home_node(&args.data_dir)
        .with_context(|| format!("open Home Node at {}", args.data_dir.display()))?;
    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .with_context(|| format!("bind MCP listener at {}", args.bind))?;
    let address = listener.local_addr().context("read MCP listener address")?;
    eprintln!("stumble-mcp HTTP server listening at http://{address}/mcp");
    if let Some(path) = tools.persistence_path() {
        eprintln!("stumble-mcp Home Node store opened at {}", path.display());
    }

    axum::serve(listener, streamable_http_router(tools))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve MCP requests")
}

async fn serve_standard_io(args: Args) -> anyhow::Result<()> {
    let token = args
        .token
        .context("stumble-mcp stdio requires a Harness token")?;
    eprintln!("stumble-mcp stdio transport ready");
    let input = std::io::stdin();
    let output = std::io::stdout();
    serve_stdio(
        move || {
            let tools = AgentTools::open_initialized_home_node(&args.data_dir)
                .with_context(|| format!("open Home Node at {}", args.data_dir.display()))?;
            let context = tools
                .authenticate_token(&token)?
                .context("invalid or revoked Harness token")?;
            Ok((tools, context))
        },
        input.lock(),
        output.lock(),
    )
    .await
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        error!(%error, "shutdown signal failed");
    }
}
