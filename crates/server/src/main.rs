use phototools_core::config::Config;
use phototools_core::ledger::Ledger;
use phototools_server::{auth, build_router, jobs, AppState};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "phototools_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load().unwrap_or_else(|_| Config::default());
    if config.roots.is_empty() {
        tracing::warn!(
            "ROOTS is empty, so every filesystem request will be refused. \
             Set ROOTS to the directories this server may touch."
        );
    }

    let auth_config = auth::AuthConfig::from_env();
    if auth_config.allowed_uids.is_empty() {
        tracing::warn!(
            "ALLOWED_UIDS is empty, so no Firebase account can use this server. \
             The allow-list is the only thing restricting access to the library."
        );
    }

    let ledger = Ledger::open(&config.database)?;
    let manager = jobs::JobManager::new(ledger);

    // F17: a job interrupted by a previous process must not silently disappear.
    match manager.recover() {
        Ok(recovered) if !recovered.is_empty() => {
            tracing::warn!(
                count = recovered.len(),
                "marked jobs interrupted after an unclean shutdown"
            );
        }
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, "could not recover interrupted jobs"),
    }

    let state = AppState {
        config: Arc::new(config),
        auth: Arc::new(auth_config),
        jobs: Arc::new(manager),
    };

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on {addr}");

    axum::serve(listener, build_router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, draining");
}
