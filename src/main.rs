mod ais140;
mod api;
mod config;
mod storage;
mod tcp;

use config::Config;
use std::net::SocketAddr;
use storage::AppState;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env();
    info!("Starting VLTD AIS-140 Gateway (TCP={}, HTTP={})", cfg.tcp_bind_addr, cfg.http_bind_addr);

    let state = AppState::new(cfg.clone()).await?;

    // 1. Spawn high-concurrency TCP ingestion listener
    let tcp_state = state.clone();
    tokio::spawn(async move {
        tcp::start_tcp_listener(tcp_state).await;
    });

    // 2. Start HTTP REST API & WebGL Dashboard
    let app = api::build_router(state);
    let http_addr: SocketAddr = cfg.http_bind_addr.parse()?;
    info!("HTTP API & WebGL Dashboard listening on http://{}", http_addr);

    let listener = TcpListener::bind(http_addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("VLTD Gateway shut down cleanly.");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("Shutdown signal received; draining connections...");
}
