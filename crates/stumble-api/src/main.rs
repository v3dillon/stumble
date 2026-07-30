use clap::{Parser, ValueEnum};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use stumble_api::{
    bind_with_port, router_with_options, ReqwestDiscoveryPeerProbe, ReqwestOriginProbe,
    RouterOptions,
};
use stumble_core::AgentTools;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long, default_value = "local")]
    mode: Mode,
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: SocketAddr,
    #[arg(long)]
    port: Option<u16>,
    #[arg(long, env = "STUMBLE_BASE_URL")]
    base_url: Option<String>,
    /// Serve open Bootstrap admission and Announcement Streams (network role)
    #[arg(long, env = "STUMBLE_BOOTSTRAP")]
    bootstrap: bool,
    /// Serve public Index search over admitted announcements (network role)
    #[arg(long, env = "STUMBLE_INDEX")]
    index: bool,
    #[arg(long, env = "STUMBLE_DATA_DIR")]
    data_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, ValueEnum)]
enum Mode {
    Local,
    Hosted,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    // Match the `stumble` CLI default so serving never opens a different node.
    let data_dir = match args.data_dir {
        Some(path) => path,
        None => {
            let home = std::env::var_os("HOME")
                .ok_or_else(|| anyhow::anyhow!("HOME is not set; pass --data-dir"))?;
            PathBuf::from(home).join(".stumble/nodes/home")
        }
    };
    let tools = AgentTools::open_initialized_home_node(&data_dir)
        .map_err(|error| anyhow::anyhow!("open Home Node at {}: {error}", data_dir.display()))?
        .with_discovery_peer_probe(Arc::new(ReqwestDiscoveryPeerProbe))
        .with_bootstrap_capability(args.bootstrap, Arc::new(ReqwestOriginProbe))
        .with_index_capability(args.index);
    let bind = bind_with_port(args.bind, args.port);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let base_url = args
        .base_url
        .unwrap_or_else(|| format!("http://{}", listener.local_addr().expect("listener addr")));
    eprintln!(
        "stumble-api running in {:?} mode at http://{}",
        args.mode,
        listener.local_addr()?
    );
    eprintln!("stumble-api public base URL {}", base_url);
    if args.bootstrap {
        eprintln!("stumble-api serving the open Bootstrap role");
    }
    if args.index {
        eprintln!("stumble-api serving the public Index role");
    }
    if let Some(path) = tools.persistence_path() {
        eprintln!("stumble-api durable store at {}", path.display());
    }
    let app = router_with_options(
        tools,
        base_url,
        RouterOptions {
            owner_access_allowed: listener.local_addr()?.ip().is_loopback(),
        },
    );
    let shutdown_signal = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            eprintln!("stumble-api shutdown signal error: {error}");
        }
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;
    Ok(())
}
