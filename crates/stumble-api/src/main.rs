use clap::{Parser, ValueEnum};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use stumble_api::{
    bind_with_port, dev_tokens_allowed_for_bind, router_with_options, RouterOptions,
};
use stumble_core::{seed_store, AgentTools};
use tokio::{sync::watch, task::JoinHandle};

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
    #[arg(
        long,
        env = "STUMBLE_HUB_REFRESH_INTERVAL_SECONDS",
        default_value_t = 86_400
    )]
    hub_refresh_interval_seconds: u64,
    #[arg(long, env = "STUMBLE_DISABLE_HUB_REFRESH")]
    disable_hub_refresh: bool,
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
    let tools = AgentTools::open_home_node(data_dir, seed_store)?;
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
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let hub_refresh_daemon = if args.disable_hub_refresh {
        eprintln!("stumble-api hub refresh worker disabled");
        None
    } else {
        Some(spawn_hub_refresh_daemon(
            tools.clone(),
            Duration::from_secs(args.hub_refresh_interval_seconds.max(1)),
            shutdown_rx,
        ))
    };
    let app = router_with_options(
        tools,
        base_url,
        RouterOptions {
            dev_tokens_allowed,
            owner_access_allowed: listener.local_addr()?.ip().is_loopback(),
        },
    );
    let shutdown_signal = {
        let shutdown_tx = shutdown_tx.clone();
        async move {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("stumble-api shutdown signal error: {error}");
            }
            let _ = shutdown_tx.send(true);
        }
    };
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;
    let _ = shutdown_tx.send(true);
    if let Some(handle) = hub_refresh_daemon {
        if let Err(error) = handle.await {
            eprintln!("stumble-api hub refresh daemon join failed: {error}");
        }
    }
    Ok(())
}

fn spawn_hub_refresh_daemon(
    tools: AgentTools,
    interval: Duration,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if wait_or_cancel(Duration::from_secs(2), &mut shutdown_rx).await {
            eprintln!("stumble-api hub refresh daemon stopped before first run");
            return;
        }
        loop {
            match tools.default_auth_context() {
                Ok(ctx) => match stumble_sync::refresh_hub_index(&tools, &ctx).await {
                    Ok(report) => {
                        if report.checked_nodes > 0 || !report.errors.is_empty() {
                            eprintln!(
                                "stumble-api hub refresh checked={} refreshed_nodes={} refreshed_pods={} fetched_events={} imported_events={} errors={}",
                                report.checked_nodes,
                                report.refreshed_nodes,
                                report.refreshed_pods,
                                report.fetched_events,
                                report.imported_events,
                                report.errors.len()
                            );
                        }
                    }
                    Err(error) => eprintln!("stumble-api hub refresh failed: {error}"),
                },
                Err(error) => eprintln!("stumble-api hub refresh skipped: {error}"),
            }
            if wait_or_cancel(interval, &mut shutdown_rx).await {
                eprintln!("stumble-api hub refresh daemon stopped");
                return;
            }
        }
    })
}

async fn wait_or_cancel(duration: Duration, shutdown_rx: &mut watch::Receiver<bool>) -> bool {
    if *shutdown_rx.borrow() {
        return true;
    }
    let sleep = tokio::time::sleep(duration);
    tokio::pin!(sleep);
    tokio::select! {
        _ = &mut sleep => false,
        _ = wait_for_shutdown(shutdown_rx) => true,
    }
}

async fn wait_for_shutdown(shutdown_rx: &mut watch::Receiver<bool>) {
    while shutdown_rx.changed().await.is_ok() {
        if *shutdown_rx.borrow() {
            break;
        }
    }
}
