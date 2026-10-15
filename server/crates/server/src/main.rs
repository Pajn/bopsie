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
use bopsie_api::{Api, Services, Web, commands, router};
use bopsie_audio::decode::find_ffmpeg;
use bopsie_core::config::Config;
use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_core::logtail::Tail;
use bopsie_library::Library;
use bopsie_library::provider::MusicProvider;
use bopsie_players::planner::Policy;
use bopsie_players::{Devices, Players};
use bopsie_players_airplay::{AirplayProvider, Config as AirplayConfig};
use bopsie_players_linkplay::{
    Config as LinkplayConfig, LinkplayProvider, ServerAddress as LinkplayServer,
};
use bopsie_providers_filesystem::FilesystemProvider;
use bopsie_queues::{Config as PlaybackConfig, Playback};
use bopsie_sources::Sources;
use bopsie_sources_spotify::{Config as SpotifyConfig, Endpoints, SpotifyAccount, SpotifyConnect};
use clap::Parser;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// How long shutdown may take, from stopping playback to closing the last connection.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

/// A music server that speaks the part of the Music Assistant API Bopsie uses.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// The TOML config file. Without one, every setting has its default.
    #[arg(long, short)]
    config: Option<PathBuf>,
}

/// What is logged unless `RUST_LOG` says otherwise. The tag reader warns about every odd tag it
/// skips, which buries everything else in a large library, and Spotify Connect's mDNS
/// responder about every packet on the network it cannot read.
const DEFAULT_LOG: &str = "info,lofty=error,libmdns=error";

#[tokio::main]
async fn main() -> ExitCode {
    // The web UI tails the log: it keeps what the log prints.
    let logs = Tail::default();
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG)))
        .with(tracing_subscriber::fmt::layer())
        .with(logs.clone())
        .init();
    choose_tls();
    match run(Args::parse(), logs).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Chooses rustls's aws-lc-rs backend for the whole process. librespot brings the ring backend
/// as well as the aws-lc-rs one the server uses, and with both rustls will not pick one itself.
fn choose_tls() {
    // Already chosen, as by an earlier call, is as good.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

async fn run(args: Args, logs: Tail) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load(args.config.as_deref())?;
    std::fs::create_dir_all(&config.storage.data_dir)?;
    let migrations = [
        bopsie_api::MIGRATIONS,
        &[
            bopsie_library::MIGRATIONS,
            bopsie_players_airplay::MIGRATIONS,
            bopsie_players::devices::MIGRATIONS,
            bopsie_queues::store::MIGRATIONS,
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
    let ffmpeg_found = ffmpeg.is_some();
    match &ffmpeg {
        Some(path) => tracing::info!(path = %path.display(), "using ffmpeg"),
        None => tracing::info!("ffmpeg not found; MP4 audio plays without gapless trimming"),
    }
    let players = Players::new(config.players.clone());
    players.load(db.clone()).await?;
    let devices = Devices::new(players.clone(), db.clone()).await?;
    let playback = Playback::start(
        devices.clone(),
        events.clone(),
        PlaybackConfig {
            ffmpeg,
            base_url: config.server.base_url.clone(),
            policy: Policy {
                protocols: config.planner.protocols.clone(),
            },
        },
    );
    playback.persist(db.clone(), library.clone()).await?;
    let sources = Sources::default();
    playback.follow_sources(sources.clone());
    let airplay = start_airplay(&config, players.clone(), db.clone()).await;
    let linkplay = start_linkplay(&config, players.clone());
    let spotify_account = if config.spotify.enabled {
        Some(
            SpotifyAccount::open(
                db.clone(),
                config.storage.data_dir.clone(),
                Endpoints::default(),
            )
            .await?,
        )
    } else {
        None
    };
    let services = Services {
        library: library.clone(),
        events,
        players,
        playback: playback.clone(),
        sources,
        spotify: spotify_account,
        airplay: airplay.clone(),
        linkplay: linkplay.clone(),
        web: config.web.dir.clone().map_or(Web::Embedded, Web::Dir),
        ffmpeg: ffmpeg_found,
        logs,
    };
    let api = Api::new(db, config.clone(), services, commands::registry()).await?;
    let spotify = config.spotify.enabled.then(|| {
        SpotifyConnect::start(
            SpotifyConfig {
                data_dir: config.storage.data_dir.clone(),
                server_id: api.server_id.clone(),
                addresses: config.spotify.addresses.clone(),
                names: config.spotify.clone(),
            },
            devices,
            api.sources.clone(),
        )
    });

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
    let (closing, closed) = tokio::sync::watch::channel(false);
    let until_closed = move || {
        let mut closed = closed.clone();
        async move {
            let _ = closed.wait_for(|closed| *closed).await;
        }
    };
    let sendspin_listener = TcpListener::bind(config.sendspin.bind).await?;
    tracing::info!(address = %sendspin_listener.local_addr()?, "Sendspin listening");
    let direct_server = tokio::spawn(
        axum::serve(
            sendspin_listener,
            direct.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(until_closed())
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

    let server = tokio::spawn(
        axum::serve(
            listener,
            router(api).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(until_closed())
        .into_future(),
    );
    shutdown_signal().await;
    tracing::info!("shutting down");
    // Spotify Connect first, so no one picks a speaker now; then playback, while speakers can
    // still be told; then AirPlay's processes; then the listeners. WebSocket clients stay connected until told otherwise, so all of it is
    // bounded.
    let shutdown = async {
        if let Some(spotify) = spotify {
            spotify.shutdown();
        }
        playback.shutdown().await;
        if let Some(airplay) = airplay {
            let _ = tokio::task::spawn_blocking(move || airplay.shutdown()).await;
        }
        if let Some(linkplay) = linkplay {
            let _ = tokio::task::spawn_blocking(move || linkplay.shutdown()).await;
        }
        let _ = closing.send(true);
        let _ = direct_server.await;
        server.await
    };
    match tokio::time::timeout(SHUTDOWN_GRACE, shutdown).await {
        Ok(result) => result??,
        Err(_) => tracing::info!("closing what is still open"),
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
    // Before discovery, which announces where speakers send their own controls.
    if let Err(error) = provider.listen_for_controls().await {
        tracing::warn!(%error, "speakers' own controls will do nothing");
    }
    if let Err(error) = provider.discover() {
        tracing::error!(%error, "AirPlay discovery could not start");
    }
    Some(provider)
}

/// LinkPlay, unless the config file turns it off.
fn start_linkplay(config: &Config, players: Players) -> Option<LinkplayProvider> {
    if !config.linkplay.enabled {
        return None;
    }
    let mut linkplay_config = LinkplayConfig::new(LinkplayServer {
        port: config.server.bind.port(),
        base_url: config.server.base_url.clone(),
    });
    if let Some(path) = &config.linkplay.certificate {
        match std::fs::read(path) {
            Ok(pem) => linkplay_config.certificate = pem,
            Err(error) => {
                tracing::error!(path = %path.display(), %error, "LinkPlay certificate unreadable; LinkPlay is off");
                return None;
            }
        }
    }
    let provider = match LinkplayProvider::new(players, linkplay_config) {
        Ok(provider) => provider,
        Err(error) => {
            tracing::error!(%error, "LinkPlay could not start");
            return None;
        }
    };
    if let Err(error) = provider.discover() {
        tracing::error!(%error, "LinkPlay discovery could not start");
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

#[cfg(test)]
mod tests {
    #[test]
    fn tls_clients_can_be_made_with_every_backend_linked_in() {
        super::choose_tls();
        let _ = rustls::ClientConfig::builder();
    }
}
