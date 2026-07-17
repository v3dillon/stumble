use anyhow::Context;
use clap::Parser;
use std::{net::SocketAddr, path::PathBuf};
use stumble_core::{seed_store, AgentTools};
use stumble_mcp::streamable_http_router;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(about = "Authenticated Streamable HTTP MCP server for Stumble")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8790")]
    bind: SocketAddr,
    #[arg(long, env = "STUMBLE_DATA_DIR")]
    data_dir: PathBuf,
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
    anyhow::ensure!(args.bind.ip().is_loopback(), "MCP must bind to loopback");

    let tools = AgentTools::open_home_node(&args.data_dir, seed_store)
        .with_context(|| format!("open Home Node at {}", args.data_dir.display()))?;
    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .with_context(|| format!("bind MCP listener at {}", args.bind))?;
    let address = listener.local_addr().context("read MCP listener address")?;
    info!(%address, endpoint = %format_args!("http://{address}/mcp"), "MCP server listening");
    if let Some(path) = tools.persistence_path() {
        info!(path = %path.display(), "Home Node store opened");
    }

    axum::serve(listener, streamable_http_router(tools))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve MCP requests")
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        error!(%error, "shutdown signal failed");
    }
}
