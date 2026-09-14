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

    // A configuration error stops the server rather than being discarded.
    //
    // This used to be `unwrap_or_else(|_| Config::default())`, which threw the
    // error away — not even logged — and started with empty roots. Every
    // request was then refused, and the only clue was a warning about `ROOTS`
    // being empty when `ROOTS` had in fact been set perfectly well and
    // something else entirely was wrong: an unparseable threshold, a path that
    // would not canonicalise, a publishing folder pointed at the library.
    //
    // Configuration is what an operator gets wrong at three in the morning on
    // a NAS with no terminal. Refusing to start, loudly, is a better answer
    // than running in a state where nothing works for reasons nobody can see
    // (G10, §9.2 invariant 6).
    let config = match Config::load() {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("Configuration is not usable, so the server will not start: {e}");
            return Err(e.into());
        }
    };

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

    // The clock that turns a day of reported positions into a track
    // (`docs/owntracks-plan.md`).
    //
    // Hourly rather than at a fixed midnight: a NAS that was off at 02:00 must
    // still write up yesterday when it comes back, and a task that fires at one
    // moment quietly never does. Writing up a day that is already written up is
    // a no-op, so running often costs nothing.
    //
    // Lifecycle belongs to the binary (G1); the rollup itself is a `core`
    // function that a test drives with no server present.
    if state.auth.device.is_some() {
        let offset_minutes: i64 = std::env::var("TIMELINE_OFFSET_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let rollup_state = state.clone();
        tokio::spawn(async move {
            let mut hourly = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                hourly.tick().await;
                let ledger = rollup_state.jobs.ledger();
                let now = chrono::Utc::now().timestamp();
                let rolled = match ledger.lock() {
                    Ok(guard) => phototools_core::tools::geotag::owntracks::roll_up_due(
                        &guard,
                        now,
                        offset_minutes * 60,
                        phototools_core::tools::geotag::owntracks::GRACE_SECONDS,
                    ),
                    Err(_) => {
                        tracing::error!("the ledger mutex is poisoned; skipping this rollup");
                        continue;
                    }
                };
                match rolled {
                    Ok(days) => {
                        for day in days {
                            tracing::info!(
                                track = %day.name,
                                fixes = day.fixes,
                                added = day.added,
                                "a day of reported positions became a track"
                            );
                        }
                    }
                    Err(e) => tracing::error!(error = %e, "could not write up reported positions"),
                }
            }
        });
        tracing::info!(
            "accepting position reports at /api/timeline/owntracks; \
             each day becomes a track {} hours after it ends",
            phototools_core::tools::geotag::owntracks::GRACE_SECONDS / 3600
        );
    }

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
