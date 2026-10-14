//! The AirPlay provider: speakers found on the network become players, PIN pairing stores
//! their credentials, and one shared PTP daemon gives AirPlay 2 groups a common clock.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bopsie_core::backoff::{self, Backoff};
use bopsie_core::db::{self, Database, Migrations};
use bopsie_core::{Error, ErrorCode, Result};
use bopsie_players::{Endpoint, Players};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::dacp;
use crate::discovery::{self, Directory, Sighting};
use crate::endpoint::{AirplayEndpoint, Launcher};
use crate::pidfile;

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
/// How often the PTP daemon is checked on.
const PTP_CHECK: Duration = Duration::from_millis(200);
/// How long the PTP daemon gets to end when told to.
const PTP_STOP: Duration = Duration::from_secs(2);
/// How long pairing waits for the speaker once the PIN is in.
const PAIRING_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Config {
    pub binary: PathBuf,
    /// Extra environment for every `cliairplay` process.
    pub env: Vec<(String, String)>,
    /// Run the shared PTP daemon for AirPlay 2 groups.
    pub ptp: bool,
    /// Where command pipes are made, and processes recorded.
    pub pipes: PathBuf,
    /// How long a speaker that left mDNS may come back before it is given up: a moment's
    /// network trouble does not end its stream.
    pub lost_grace: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            binary: PathBuf::new(),
            env: Vec::new(),
            ptp: false,
            pipes: PathBuf::new(),
            lost_grace: Duration::from_secs(30),
        }
    }
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
    /// The PTP daemon's process id, while it runs.
    ptp: Mutex<Option<u32>>,
    mdns: Mutex<Option<mdns_sd::ServiceDaemon>>,
    /// Where speakers send what their own controls ask for, once listening.
    dacp_port: Mutex<Option<u16>>,
    /// The threads that keep the PTP daemon and discovery running.
    watchers: Mutex<Vec<std::thread::JoinHandle<()>>>,
    stopping: AtomicBool,
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
        let pipes = config.pipes.clone();
        let ended = tokio::task::spawn_blocking(move || pidfile::end_leftovers(&pipes))
            .await
            .unwrap_or(0);
        if ended > 0 {
            tracing::info!(ended, "ended cliairplay processes an earlier run left");
        }
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
                dacp_port: Mutex::default(),
                watchers: Mutex::default(),
                stopping: AtomicBool::new(false),
            }),
        })
    }

    /// Listens for what speakers' own controls ask for: their buttons, remotes and apps. A
    /// request names the stream it is about by its `Active-Remote` id, which finds the speaker.
    /// Needs a Tokio runtime; announced when discovery starts.
    pub async fn listen_for_controls(&self) -> Result<u16> {
        let listener = dacp::bind().await.map_err(|e| {
            Error::new(
                ErrorCode::SetupFailed,
                format!("cannot listen for DACP: {e}"),
            )
        })?;
        let port = listener.local_addr().map(|a| a.port()).unwrap_or_default();
        *self.inner.dacp_port.lock().unwrap() = Some(port);
        let provider = Arc::downgrade(&self.inner);
        tokio::spawn(dacp::serve(
            listener,
            move |active_remote: &str, path: &str| {
                let Some(inner) = provider.upgrade() else {
                    return;
                };
                let endpoint = inner
                    .endpoints
                    .lock()
                    .unwrap()
                    .values()
                    .find(|e| e.active_remote() == active_remote)
                    .cloned();
                match (endpoint, dacp::parse(path)) {
                    (Some(endpoint), Some(request)) => endpoint.dacp(request),
                    (endpoint, _) => tracing::debug!(
                        speaker = endpoint.map(|e| e.id().to_owned()),
                        active_remote,
                        path,
                        "a DACP request not acted on"
                    ),
                }
            },
        ));
        tracing::info!(port, "listening for speakers' own controls");
        Ok(port)
    }

    /// Announces the DACP listener on `daemon`, so speakers find where to send their controls.
    fn announce_controls(&self, daemon: &mdns_sd::ServiceDaemon) {
        let Some(port) = *self.inner.dacp_port.lock().unwrap() else {
            return;
        };
        let dacp_id = &self.inner.launcher.dacp_id;
        let service = mdns_sd::ServiceInfo::new(
            dacp::SERVICE,
            &format!("iTunes_Ctrl_{dacp_id}"),
            &format!("bopsie-{}.local.", dacp_id.to_lowercase()),
            (),
            port,
            [
                ("txtvers", "1"),
                ("Ver", "63B5E5C0C201542E"),
                ("DbId", "63B5E5C0C201542E"),
                ("OSsi", "0x1F5"),
            ]
            .as_slice(),
        )
        .map(mdns_sd::ServiceInfo::enable_addr_auto);
        match service.map(|service| daemon.register(service)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) | Err(error) => {
                tracing::warn!(%error, "cannot announce where speakers send their controls");
            }
        }
    }

    /// Starts the PTP daemon, if configured, and browses the network for speakers. Both are
    /// started again, after a wait, whenever they stop.
    pub fn discover(&self) -> Result<()> {
        if self.inner.config.ptp {
            let provider = self.clone();
            let watcher = std::thread::Builder::new()
                .name("airplay-ptp".into())
                .spawn(move || provider.keep_ptp())
                .expect("spawning a thread");
            self.inner.watchers.lock().unwrap().push(watcher);
        }
        // The first browse fails here, so a server that cannot browse says so at once.
        let threads = self.browse()?;
        let provider = self.clone();
        let watcher = std::thread::Builder::new()
            .name("airplay-mdns".into())
            .spawn(move || provider.keep_browsing(threads))
            .expect("spawning a thread");
        self.inner.watchers.lock().unwrap().push(watcher);
        Ok(())
    }

    fn stopping(&self) -> bool {
        self.inner.stopping.load(Ordering::SeqCst)
    }

    /// Sleeps for `wait`, or until the provider stops. Returns whether it is stopping.
    fn sleep(&self, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            if self.stopping() {
                return true;
            }
            std::thread::sleep(PTP_CHECK.min(deadline - Instant::now()));
        }
        self.stopping()
    }

    fn browse(&self) -> Result<Vec<std::thread::JoinHandle<()>>> {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| Error::new(ErrorCode::SetupFailed, format!("cannot start mDNS: {e}")))?;
        let (seer, loser) = (self.clone(), self.clone());
        let threads = discovery::browse(
            &daemon,
            move |sighting| seer.see(sighting),
            move |fullname| loser.lose(fullname),
        )
        .map_err(|e| Error::new(ErrorCode::SetupFailed, format!("cannot browse: {e}")))?;
        self.announce_controls(&daemon);
        if let Some(old) = self.inner.mdns.lock().unwrap().replace(daemon) {
            let _ = old.shutdown();
        }
        Ok(threads)
    }

    /// Browses again whenever browsing ends, until the provider stops.
    fn keep_browsing(&self, mut threads: Vec<std::thread::JoinHandle<()>>) {
        let mut backoff = Backoff::new(backoff::Policy::RECONNECT);
        loop {
            for thread in threads.drain(..) {
                let _ = thread.join();
            }
            if self.stopping() {
                return;
            }
            let wait = backoff.failed(Instant::now());
            tracing::warn!(
                ?wait,
                "AirPlay discovery stopped; browsing again after a wait"
            );
            if self.sleep(wait) {
                return;
            }
            match self.browse() {
                Ok(started) => {
                    backoff.working(Instant::now());
                    threads = started;
                }
                Err(error) => tracing::warn!(%error, "cannot browse for AirPlay speakers"),
            }
        }
    }

    /// One `cliairplay --ptp-daemon` for the host, started again whenever it ends. It needs UDP
    /// ports 319 and 320; where it cannot have them, as on macOS, each stream keeps its own
    /// clock.
    fn keep_ptp(&self) {
        let pidfile = self.inner.config.pipes.join("ptp-daemon.pid");
        let mut backoff = Backoff::new(backoff::Policy::RECONNECT);
        let mut warned = false;
        while !self.stopping() {
            let spawned = Command::new(&self.inner.config.binary)
                .arg("--ptp-daemon")
                .envs(self.inner.config.env.iter().map(|(k, v)| (k, v)))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match spawned {
                Ok(child) => child,
                Err(error) => {
                    tracing::warn!(%error, "cannot start the PTP daemon");
                    if self.sleep(backoff.failed(Instant::now())) {
                        return;
                    }
                    continue;
                }
            };
            pidfile::record(&pidfile, child.id());
            if let Some(stderr) = child.stderr.take() {
                std::thread::spawn(move || {
                    for line in BufReader::new(stderr)
                        .lines()
                        .map_while(std::result::Result::ok)
                    {
                        tracing::debug!(target: "cliairplay::ptp", "{line}");
                    }
                });
            }
            let ran = self.watch_ptp(&mut child, &mut backoff, &mut warned);
            self.set_ptp(None);
            pidfile::forget(&pidfile);
            if !ran || self.stopping() {
                return;
            }
            let wait = backoff.failed(Instant::now());
            if self.sleep(wait) {
                return;
            }
        }
    }

    /// Watches the PTP daemon until it ends, or ends it when the provider stops. Returns
    /// whether it should be started again.
    fn watch_ptp(&self, child: &mut Child, backoff: &mut Backoff, warned: &mut bool) -> bool {
        let started = Instant::now();
        let mut serving = false;
        loop {
            if self.stopping() {
                let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGTERM);
                let deadline = Instant::now() + PTP_STOP;
                while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            if let Ok(Some(status)) = child.try_wait() {
                if serving {
                    tracing::warn!(%status, "the PTP daemon stopped; starting it again");
                } else if !*warned {
                    *warned = true;
                    tracing::warn!(
                        %status,
                        "the PTP daemon could not run; AirPlay 2 groups will not share a clock (it needs UDP ports 319 and 320)"
                    );
                }
                return true;
            }
            if !serving && started.elapsed() >= PTP_GRACE {
                serving = true;
                *warned = false;
                backoff.working(Instant::now());
                tracing::info!("PTP daemon running");
                self.set_ptp(Some(child.id()));
            }
            std::thread::sleep(PTP_CHECK);
        }
    }

    /// Whether streams share the PTP daemon's clock: only while it runs.
    fn set_ptp(&self, pid: Option<u32>) {
        *self.inner.launcher.ptp_shared.lock().unwrap() = pid.is_some();
        *self.inner.ptp.lock().unwrap() = pid;
    }

    /// The PTP daemon's process id, while it runs.
    pub fn ptp_daemon(&self) -> Option<u32> {
        *self.inner.ptp.lock().unwrap()
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
        let id = speaker.id.clone();
        let endpoint = AirplayEndpoint::new(
            speaker,
            self.inner.launcher.clone(),
            credentials,
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
        let Some(endpoint) = self.inner.endpoints.lock().unwrap().get(&id).cloned() else {
            return;
        };
        // Given up only if it has not been found again by the end of the grace period.
        let seen = endpoint.sightings();
        let grace = self.inner.config.lost_grace;
        let lose = move || {
            if endpoint.sightings() == seen {
                tracing::info!(speaker = id, "AirPlay speaker gone");
                endpoint.lost();
            }
        };
        if grace.is_zero() {
            lose();
        } else {
            std::thread::spawn(move || {
                std::thread::sleep(grace);
                lose();
            });
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

    /// Speaker `id`'s settings changed: see [`AirplayEndpoint::settings_changed`].
    pub fn settings_changed(&self, id: &str) -> Result<()> {
        self.endpoint(id)?.settings_changed();
        Ok(())
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

    /// Ends discovery, the PTP daemon and every speaker's process, telling each speaker its
    /// session is over first.
    pub fn shutdown(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        if let Some(daemon) = self.inner.mdns.lock().unwrap().take() {
            let _ = daemon.shutdown();
        }
        let endpoints: Vec<_> = self
            .inner
            .endpoints
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect();
        let ending: Vec<_> = endpoints
            .into_iter()
            .map(|endpoint| std::thread::spawn(move || endpoint.supervisor().disconnect()))
            .collect();
        for thread in ending {
            let _ = thread.join();
        }
        for (_, mut pairing) in self.inner.pairing.lock().unwrap().drain() {
            let _ = pairing.child.kill();
            let _ = pairing.child.wait();
        }
        let watchers: Vec<_> = self.inner.watchers.lock().unwrap().drain(..).collect();
        for watcher in watchers {
            let _ = watcher.join();
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
