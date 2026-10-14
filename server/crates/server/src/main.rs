//! The server binary: the only crate that knows the concrete list of providers. It wires them
//! together, starts the runtime and shuts it down.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{ConnectInfo, WebSocketUpgrade};
use axum::routing::get;
use bopsie_api::{Api, Services, commands, router};
use bopsie_audio::decode::find_ffmpeg;
use bopsie_core::config::Config;
use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_library::Library;
use bopsie_library::provider::MusicProvider;
use bopsie_players::planner::Policy;
use bopsie_players::{Devices, Players};
use bopsie_players_airplay::{AirplayProvider, Config as AirplayConfig};
use bopsie_providers_filesystem::FilesystemProvider;
use bopsie_queues::{Config as PlaybackConfig, Playback};
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
    let migrations = [
        bopsie_api::MIGRATIONS,
        &[
            bopsie_library::MIGRATIONS,
            bopsie_players_airplay::MIGRATIONS,
            bopsie_players::devices::MIGRATIONS,
        ],
    ]
    .concat();
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
    let ffmpeg = find_ffmpeg(config.audio.ffmpeg.as_deref());
    match &ffmpeg {
        Some(path) => tracing::info!(path = %path.display(), "using ffmpeg"),
        None => tracing::info!("ffmpeg not found; MP4 audio plays without gapless trimming"),
    }
    let players = Players::new(config.players.clone());
    let devices = Devices::new(players.clone(), db.clone()).await?;
    let playback = Playback::start(
        devices,
        events.clone(),
        PlaybackConfig {
            ffmpeg,
            base_url: config.server.base_url.clone(),
            policy: Policy {
                protocols: config.planner.protocols.clone(),
            },
        },
    );
    let airplay = start_airplay(&config, players.clone(), db.clone()).await;
    let services = Services {
        library: library.clone(),
        events,
        players,
        playback,
        airplay: airplay.clone(),
    };
    let api = Api::new(db, config.server.clone(), services, commands::registry()).await?;

    // Sendspin clients that connect directly, without a token.
    let sendspin = api.sendspin.clone();
    let direct = Router::new().route(
        "/sendspin",
        get(
            |ConnectInfo(peer): ConnectInfo<SocketAddr>, upgrade: WebSocketUpgrade| async move {
                upgrade.on_upgrade(move |socket| async move {
                    sendspin.serve(socket, None, Some(peer.ip())).await
                })
            },
        ),
    );
    let sendspin_listener = TcpListener::bind(config.sendspin.bind).await?;
    tracing::info!(address = %sendspin_listener.local_addr()?, "Sendspin listening");
    tokio::spawn(
        axum::serve(
            sendspin_listener,
            direct.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .into_future(),
    );

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
        axum::serve(
            listener,
            router(api).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .into_future(),
    );
    shutdown_signal().await;
    tracing::info!("shutting down");
    if let Some(airplay) = &airplay {
        airplay.shutdown();
    }
    let _ = stop.send(());
    // WebSocket clients stay connected until told otherwise, so the wait is bounded.
    match tokio::time::timeout(SHUTDOWN_GRACE, server).await {
        Ok(result) => result??,
        Err(_) => tracing::info!("closing the connections still open"),
    }
    Ok(())
}

/// AirPlay, when `cliairplay` can be found. Without it, the server runs without AirPlay.
async fn start_airplay(config: &Config, players: Players, db: Database) -> Option<AirplayProvider> {
    let binary = config
        .airplay
        .binary
        .clone()
        .unwrap_or_else(|| PathBuf::from("cliairplay"));
    let works = std::process::Command::new(&binary)
        .arg("--check")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !works {
        tracing::info!(
            "cliairplay not found; AirPlay is off (scripts/fetch-cliairplay.sh installs it)"
        );
        return None;
    }
    let airplay_config = AirplayConfig {
        binary,
        passwords: config
            .players
            .iter()
            .filter_map(|(id, player)| Some((id.clone(), player.password.clone()?)))
            .collect(),
        env: Vec::new(),
        ptp: config.airplay.ptp,
        pipes: config.storage.data_dir.join("airplay"),
        ..AirplayConfig::default()
    };
    let provider = match AirplayProvider::new(players, db, airplay_config).await {
        Ok(provider) => provider,
        Err(error) => {
            tracing::error!(%error, "AirPlay could not start");
            return None;
        }
    };
    if let Err(error) = provider.discover() {
        tracing::error!(%error, "AirPlay discovery could not start");
    }
    Some(provider)
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
