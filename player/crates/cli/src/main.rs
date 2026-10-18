//! A headless Bopsie player. `login` signs in and keeps the token; `play`, the default, plays to
//! the default output device until interrupted.

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;

use anyhow::{Context, bail};
use bopsie_player::{ConnectionState, PRODUCT_NAME, Player, PlayerConfig, Snapshot, sign_in};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "bopsie-player", version, about)]
struct Cli {
    /// Where the sign-in is kept. Defaults to bopsie-player/config.json in the user's
    /// configuration directory.
    #[arg(long, global = true, env = "BOPSIE_PLAYER_CONFIG")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Sign in to a server. The password is asked for, or read from standard input when that is
    /// not a terminal.
    Login {
        /// The server's address, such as music.local or https://music.example.com.
        server: String,
        #[arg(short, long)]
        username: String,
        /// The player's name as the server shows it.
        #[arg(short, long, default_value = "Bopsie Player")]
        name: String,
    },
    /// Forget the sign-in.
    Logout,
    /// Play until interrupted.
    Play,
}

/// What `login` keeps for `play`.
#[derive(Serialize, Deserialize)]
struct Config {
    server_url: String,
    username: String,
    token: String,
    client_id: String,
    name: String,
    #[serde(default = "full_volume")]
    volume: u8,
}

fn full_volume() -> u8 {
    100
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,bopsie_player=info")),
        )
        .init();
    let cli = Cli::parse();
    let path = match cli.config {
        Some(path) => path,
        None => dirs::config_dir()
            .context("no configuration directory for this user; pass --config")?
            .join("bopsie-player/config.json"),
    };
    match cli.command.unwrap_or(Command::Play) {
        Command::Login {
            server,
            username,
            name,
        } => login(&path, &server, &username, name).await,
        Command::Logout => {
            if path.exists() {
                std::fs::remove_file(&path)
                    .with_context(|| format!("could not remove {}", path.display()))?;
            }
            Ok(())
        }
        Command::Play => play(&path).await,
    }
}

async fn login(path: &PathBuf, server: &str, username: &str, name: String) -> anyhow::Result<()> {
    let password = if std::io::stdin().is_terminal() {
        rpassword::prompt_password("Password: ")?
    } else {
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        line.trim_end_matches(['\r', '\n']).to_owned()
    };
    let signed_in = sign_in(
        server,
        username,
        &password,
        &format!("{name} (Bopsie Player)"),
    )
    .await?;
    // A player keeps its id across sign-ins, so the server sees the same player and queue.
    let previous = read_config(path).ok();
    let config = Config {
        server_url: signed_in.server_url,
        username: signed_in.username,
        token: signed_in.token,
        client_id: previous
            .as_ref()
            .map(|config| config.client_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name,
        volume: previous.map_or(100, |config| config.volume),
    };
    write_config(path, &config)?;
    println!("Signed in to {} as {}.", config.server_url, config.username);
    Ok(())
}

async fn play(path: &PathBuf) -> anyhow::Result<()> {
    let config = read_config(path).context("not signed in; run `bopsie-player login` first")?;
    let player = Player::start(PlayerConfig {
        server_url: config.server_url.clone(),
        token: config.token.clone(),
        client_id: config.client_id.clone(),
        name: config.name.clone(),
        product_name: PRODUCT_NAME.to_owned(),
        software_version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        volume: config.volume,
        muted: false,
    })?;
    let mut changes = player.watch();
    let mut last = changes.borrow_and_update().clone();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            changed = changes.changed() => {
                if changed.is_err() {
                    break;
                }
                let snapshot = changes.borrow_and_update().clone();
                report(&last, &snapshot);
                if snapshot.connection == ConnectionState::Unauthorized {
                    player.stop();
                    bail!("the server no longer accepts this sign-in; run `bopsie-player login` again");
                }
                last = snapshot;
            }
        }
    }
    let volume = player.snapshot().volume;
    tokio::task::spawn_blocking(move || player.stop()).await?;
    if volume != config.volume {
        write_config(path, &Config { volume, ..config })?;
    }
    Ok(())
}

/// Prints what changed that a person watching would care about.
fn report(last: &Snapshot, now: &Snapshot) {
    if now.connection != last.connection {
        match &now.connection {
            ConnectionState::Connected => tracing::info!("connected"),
            ConnectionState::Reconnecting { attempt, error } => {
                tracing::info!(attempt, error, "reconnecting");
            }
            _ => {}
        }
    }
    if now.playing != last.playing {
        tracing::info!(playing = now.playing, "playback");
    }
    let track = |s: &Snapshot| {
        s.now_playing
            .as_ref()
            .map(|n| (n.title.clone(), n.artist.clone()))
    };
    if track(now) != track(last)
        && let Some(playing) = &now.now_playing
    {
        tracing::info!(
            title = playing.title.as_deref().unwrap_or("?"),
            artist = playing.artist.as_deref().unwrap_or("?"),
            "now playing"
        );
    }
    if now.volume != last.volume || now.muted != last.muted {
        tracing::info!(volume = now.volume, muted = now.muted, "volume");
    }
}

fn read_config(path: &PathBuf) -> anyhow::Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("could not read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("could not parse {}", path.display()))
}

/// Writes the configuration readable by its owner only, since it holds the token.
fn write_config(path: &PathBuf, config: &Config) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(config)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("could not write {}", path.display()))?;
    std::io::Write::write_all(&mut file, text.as_bytes())?;
    Ok(())
}
