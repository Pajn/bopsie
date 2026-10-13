//! The server binary: the only crate that knows the concrete list of providers. It wires them
//! together, starts the runtime and shuts it down.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use bopsie_api::{Api, commands, router};
use bopsie_core::config::Config;
use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_library::Library;
use bopsie_library::provider::MusicProvider;
use bopsie_providers_filesystem::FilesystemProvider;
use clap::Parser;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

/// How long open connections get to finish once shutdown starts.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// A music server that speaks the part of the Music Assistant API Bopsie uses.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// The TOML config file. Without one, every setting has its default.
    #[arg(long, short)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(args.config.as_deref())?;
    std::fs::create_dir_all(&config.storage.data_dir)?;
    let migrations = [bopsie_api::MIGRATIONS, &[bopsie_library::MIGRATIONS]].concat();
    let db = Database::open(&config.storage.data_dir.join("bopsie.db"), &migrations)?;
    let events = EventBus::default();

    let providers: Vec<Arc<dyn MusicProvider>> = config
        .library
        .folders
        .iter()
        .map(|folder| Arc::new(FilesystemProvider::new(folder.clone())) as Arc<dyn MusicProvider>)
        .collect();
    let library = Library::new(
        db.clone(),
        events.clone(),
        providers,
        config.storage.data_dir.join("artwork"),
    );
    let api = Api::new(
        db,
        config.server.clone(),
        library.clone(),
        events,
        commands::registry(),
    )
    .await?;

    // The API serves the library as it was while the scan catches up.
    tokio::spawn(async move {
        if let Err(error) = library.sync().await {
            tracing::error!(%error, "library sync failed");
        }
    });

    let listener = TcpListener::bind(config.server.bind).await?;
    tracing::info!(address = %listener.local_addr()?, "listening");

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(
        axum::serve(listener, router(api))
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .into_future(),
    );
    shutdown_signal().await;
    tracing::info!("shutting down");
    let _ = stop.send(());
    // WebSocket clients stay connected until told otherwise, so the wait is bounded.
    match tokio::time::timeout(SHUTDOWN_GRACE, server).await {
        Ok(result) => result??,
        Err(_) => tracing::info!("closing the connections still open"),
    }
    Ok(())
}

async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
}
