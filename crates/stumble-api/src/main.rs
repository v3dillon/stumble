use clap::{Parser, ValueEnum};
use std::net::SocketAddr;
use std::path::PathBuf;
use stumble_api::{
    bind_with_port, dev_tokens_allowed_for_bind, router_with_options, RouterOptions,
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
    #[arg(long)]
    allow_public_dev_tokens: bool,
    #[arg(long, env = "STUMBLE_BASE_URL")]
    base_url: Option<String>,
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
    let data_dir = args.data_dir.unwrap_or_else(|| PathBuf::from(".stumble"));
    let tools = AgentTools::open_initialized_home_node(data_dir)?;
    let bind = bind_with_port(args.bind, args.port);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let base_url = args
        .base_url
        .unwrap_or_else(|| format!("http://{}", listener.local_addr().expect("listener addr")));
    let dev_tokens_allowed =
        dev_tokens_allowed_for_bind(listener.local_addr()?, args.allow_public_dev_tokens);
    eprintln!(
        "stumble-api running in {:?} mode at http://{}",
        args.mode,
        listener.local_addr()?
    );
    eprintln!("stumble-api public base URL {}", base_url);
    if !dev_tokens_allowed {
        eprintln!("stumble-api dev token minting disabled because bind address is not loopback");
    }
    if let Some(path) = tools.persistence_path() {
        eprintln!("stumble-api durable store at {}", path.display());
    }
    let app = router_with_options(
        tools,
        base_url,
        RouterOptions {
            dev_tokens_allowed,
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
