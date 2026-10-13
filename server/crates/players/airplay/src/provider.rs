//! The AirPlay provider: speakers found on the network become players, PIN pairing stores
//! their credentials, and one shared PTP daemon gives AirPlay 2 groups a common clock.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_core::db::{self, Database, Migrations};
use bopsie_core::{Error, ErrorCode, Result};
use bopsie_players::{Endpoint, Players};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::discovery::{self, Directory, Sighting};
use crate::endpoint::{AirplayEndpoint, Launcher};

pub const MIGRATIONS: Migrations = Migrations {
    component: "airplay",
    steps: &["
        CREATE TABLE airplay_credentials (
            player_id TEXT PRIMARY KEY,
            credentials TEXT NOT NULL
        ) STRICT;
    "],
};

/// How long the PTP daemon gets to show it can run.
const PTP_GRACE: Duration = Duration::from_millis(500);
/// How long pairing waits for the speaker once the PIN is in.
const PAIRING_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub binary: PathBuf,
    /// Passwords by player id, for speakers that ask for one.
    pub passwords: BTreeMap<String, String>,
    /// Extra environment for every `cliairplay` process.
    pub env: Vec<(String, String)>,
    /// Run the shared PTP daemon for AirPlay 2 groups.
    pub ptp: bool,
    /// Where command pipes are made.
    pub pipes: PathBuf,
}

struct Pairing {
    child: Child,
    stdin: ChildStdin,
}

struct Inner {
    players: Players,
    db: Database,
    config: Config,
    launcher: Launcher,
    directory: Mutex<Directory>,
    endpoints: Mutex<HashMap<String, Arc<AirplayEndpoint>>>,
    credentials: Mutex<HashMap<String, String>>,
    pairing: Mutex<HashMap<String, Pairing>>,
    ptp: Mutex<Option<Child>>,
    mdns: Mutex<Option<mdns_sd::ServiceDaemon>>,
}

#[derive(Clone)]
pub struct AirplayProvider {
    inner: Arc<Inner>,
}

fn not_found(id: &str) -> Error {
    Error::new(
        ErrorCode::PlayerUnavailable,
        format!("No AirPlay speaker {id}"),
    )
}

impl AirplayProvider {
    /// Loads the server's DACP id and stored credentials. Discovery starts with
    /// [`AirplayProvider::discover`].
    pub async fn new(players: Players, db: Database, config: Config) -> Result<Self> {
        let (dacp_id, credentials) = db
            .call(|conn| {
                let dacp_id = db::setting_or_init(conn, "airplay_dacp_id", || {
                    format!("{:016X}", rand::random::<u64>())
                })?;
                let mut statement =
                    conn.prepare("SELECT player_id, credentials FROM airplay_credentials")?;
                let credentials = statement
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<rusqlite::Result<HashMap<String, String>>>()?;
                Ok((dacp_id, credentials))
            })
            .await?;
        std::fs::create_dir_all(&config.pipes).map_err(|e| {
            Error::new(
                ErrorCode::SetupFailed,
                format!("cannot create {}: {e}", config.pipes.display()),
            )
        })?;
        let launcher = Launcher {
            binary: config.binary.clone(),
            pipes: config.pipes.clone(),
            dacp_id,
            env: config.env.clone(),
            ptp_shared: Arc::new(Mutex::new(false)),
        };
        Ok(Self {
            inner: Arc::new(Inner {
                players,
                db,
                config,
                launcher,
                directory: Mutex::default(),
                endpoints: Mutex::default(),
                credentials: Mutex::new(credentials),
                pairing: Mutex::default(),
                ptp: Mutex::default(),
                mdns: Mutex::default(),
            }),
        })
    }

    /// Starts the PTP daemon, if configured, and browses the network for speakers.
    pub fn discover(&self) -> Result<()> {
        if self.inner.config.ptp {
            self.start_ptp();
        }
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| Error::new(ErrorCode::SetupFailed, format!("cannot start mDNS: {e}")))?;
        let (seer, loser) = (self.clone(), self.clone());
        discovery::browse(
            &daemon,
            move |sighting| seer.see(sighting),
            move |fullname| loser.lose(fullname),
        )
        .map_err(|e| Error::new(ErrorCode::SetupFailed, format!("cannot browse: {e}")))?;
        *self.inner.mdns.lock().unwrap() = Some(daemon);
        Ok(())
    }

    /// One `cliairplay --ptp-daemon` for the host. It needs UDP ports 319 and 320; where it
    /// cannot have them, as on macOS, each stream keeps its own clock.
    fn start_ptp(&self) {
        let spawned = Command::new(&self.inner.config.binary)
            .arg("--ptp-daemon")
            .envs(self.inner.config.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                tracing::warn!(%error, "cannot start the PTP daemon");
                return;
            }
        };
        std::thread::sleep(PTP_GRACE);
        if let Ok(Some(status)) = child.try_wait() {
            tracing::warn!(
                %status,
                "the PTP daemon could not run; AirPlay 2 groups will not share a clock (it needs UDP ports 319 and 320)"
            );
            return;
        }
        tracing::info!("PTP daemon running");
        *self.inner.launcher.ptp_shared.lock().unwrap() = true;
        *self.inner.ptp.lock().unwrap() = Some(child);
    }

    /// A speaker's service was resolved.
    pub fn see(&self, sighting: Sighting) {
        let Some(speaker) = self.inner.directory.lock().unwrap().see(sighting) else {
            return;
        };
        // Its services resolve on different threads: decide under one lock which of them
        // creates the endpoint.
        let mut endpoints = self.inner.endpoints.lock().unwrap();
        if let Some(endpoint) = endpoints.get(&speaker.id).cloned() {
            drop(endpoints);
            endpoint.rediscovered(speaker);
            return;
        }
        tracing::info!(speaker = speaker.id, name = speaker.name, address = %speaker.address, "AirPlay speaker found");
        let credentials = self
            .inner
            .credentials
            .lock()
            .unwrap()
            .get(&speaker.id)
            .cloned();
        let password = self.inner.config.passwords.get(&speaker.id).cloned();
        let id = speaker.id.clone();
        let endpoint = AirplayEndpoint::new(
            speaker,
            self.inner.launcher.clone(),
            credentials,
            password,
            self.inner.players.clone(),
        );
        endpoints.insert(id, endpoint.clone());
        drop(endpoints);
        self.inner.players.register(endpoint);
    }

    /// A speaker's service went away.
    pub fn lose(&self, fullname: &str) {
        let Some(id) = self.inner.directory.lock().unwrap().lose(fullname) else {
            return;
        };
        if let Some(endpoint) = self.inner.endpoints.lock().unwrap().get(&id) {
            tracing::info!(speaker = id, "AirPlay speaker gone");
            endpoint.lost();
        }
    }

    fn endpoint(&self, id: &str) -> Result<Arc<AirplayEndpoint>> {
        self.inner
            .endpoints
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| not_found(id))
    }

    /// Asks the speaker to show a PIN, for [`AirplayProvider::pair_finish`].
    pub fn pair_start(&self, id: &str) -> Result<()> {
        let speaker = self.endpoint(id)?.speaker();
        let port = speaker.airplay.as_ref().map_or(7000, |(port, _)| *port);
        let mut child = Command::new(&self.inner.config.binary)
            .args(["--pair-setup", &speaker.address.to_string()])
            .args(["--port", &port.to_string()])
            .args(["--dacp", &self.inner.launcher.dacp_id])
            .envs(self.inner.config.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                Error::new(ErrorCode::SetupFailed, format!("cannot start pairing: {e}"))
            })?;
        let stdin = child.stdin.take().expect("stdin is piped");
        if let Some(mut old) = self
            .inner
            .pairing
            .lock()
            .unwrap()
            .insert(id.to_owned(), Pairing { child, stdin })
        {
            let _ = old.child.kill();
            let _ = old.child.wait();
        }
        Ok(())
    }

    /// Completes pairing with the PIN the speaker shows, and stores the credentials.
    pub async fn pair_finish(&self, id: &str, pin: &str) -> Result<()> {
        let Some(mut pairing) = self.inner.pairing.lock().unwrap().remove(id) else {
            return Err(Error::invalid_data(format!(
                "Pairing with {id} has not started"
            )));
        };
        let pin = pin.trim().to_owned();
        let credentials = tokio::task::spawn_blocking(move || -> Option<String> {
            writeln!(pairing.stdin, "{pin}").ok()?;
            let stdout = pairing.child.stdout.take()?;
            let (found, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout)
                    .lines()
                    .map_while(std::result::Result::ok)
                {
                    if let Some(credentials) = line.strip_prefix("CREDENTIALS:") {
                        let _ = found.send(credentials.trim().to_owned());
                        return;
                    }
                }
            });
            let credentials = receive.recv_timeout(PAIRING_TIMEOUT).ok();
            let _ = pairing.child.kill();
            let _ = pairing.child.wait();
            credentials
        })
        .await
        .map_err(|_| Error::unexpected())?
        .filter(|c| c.len() == 192 && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::AuthenticationFailed,
                "Pairing failed: check the PIN and try again",
            )
        })?;

        let (player, stored) = (id.to_owned(), credentials.clone());
        self.inner
            .db
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO airplay_credentials (player_id, credentials) VALUES (?1, ?2)
                     ON CONFLICT (player_id) DO UPDATE SET credentials = excluded.credentials",
                    (&player, &stored),
                )?;
                Ok(())
            })
            .await?;
        self.inner
            .credentials
            .lock()
            .unwrap()
            .insert(id.to_owned(), credentials.clone());
        self.endpoint(id)?.set_credentials(credentials);
        tracing::info!(speaker = id, "paired");
        Ok(())
    }

    /// Whether `id` is an AirPlay speaker, for commands that only apply to those.
    pub fn knows(&self, id: &str) -> bool {
        self.inner.endpoints.lock().unwrap().contains_key(id)
    }

    /// Ends the PTP daemon and every speaker's process.
    pub fn shutdown(&self) {
        for endpoint in self.inner.endpoints.lock().unwrap().values() {
            if endpoint.supervisor().is_running() {
                let _ = endpoint.supervisor().command(&["ACTION=DISCONNECT".into()]);
            }
        }
        if let Some(mut ptp) = self.inner.ptp.lock().unwrap().take() {
            let _ = kill(Pid::from_raw(ptp.id() as i32), Signal::SIGTERM);
            let _ = ptp.wait();
        }
        if let Some(daemon) = self.inner.mdns.lock().unwrap().take() {
            let _ = daemon.shutdown();
        }
    }

    pub fn endpoints(&self) -> Vec<Arc<dyn Endpoint>> {
        self.inner
            .endpoints
            .lock()
            .unwrap()
            .values()
            .map(|e| e.clone() as Arc<dyn Endpoint>)
            .collect()
    }
}
