//! The Cast provider: devices found on the network become players, each with its Cast channel
//! kept open, and played through the Sendspin receiver launched on it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bopsie_core::backoff::{self, Backoff};
use bopsie_core::{Error, ErrorCode, Result};
use bopsie_players::Players;
use bopsie_players_sendspin::Hosted;
use tokio::sync::mpsc;

use crate::channel;
use crate::discovery::{self, Sighting};
use crate::endpoint::{CastEndpoint, Command, Settings, Shared, Status};

struct Inner {
    players: Players,
    hosted: Hosted,
    settings: Settings,
    endpoints: Mutex<HashMap<String, Arc<CastEndpoint>>>,
    /// Which player id each mDNS service is.
    services: Mutex<HashMap<String, String>>,
    mdns: Mutex<Option<mdns_sd::ServiceDaemon>>,
    watcher: Mutex<Option<std::thread::JoinHandle<()>>>,
    stopping: AtomicBool,
    runtime: tokio::runtime::Handle,
}

#[derive(Clone)]
pub struct CastProvider {
    inner: Arc<Inner>,
}

impl CastProvider {
    /// Discovery starts with [`CastProvider::discover`]. `hosted` is the Sendspin server's, which
    /// the receivers connect to. Needs a Tokio runtime.
    pub fn new(players: Players, hosted: Hosted, settings: Settings) -> Self {
        Self {
            inner: Arc::new(Inner {
                players,
                hosted,
                settings,
                endpoints: Mutex::default(),
                services: Mutex::default(),
                mdns: Mutex::default(),
                watcher: Mutex::default(),
                stopping: AtomicBool::new(false),
                runtime: tokio::runtime::Handle::current(),
            }),
        }
    }

    /// Browses the network for devices, browsing again whenever browsing ends.
    pub fn discover(&self) -> Result<()> {
        let first = self.browse()?;
        let provider = self.clone();
        let watcher = std::thread::Builder::new()
            .name("cast-mdns".into())
            .spawn(move || provider.keep_browsing(first))
            .expect("spawning a thread");
        *self.inner.watcher.lock().unwrap() = Some(watcher);
        Ok(())
    }

    fn browse(&self) -> Result<std::thread::JoinHandle<()>> {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| Error::new(ErrorCode::SetupFailed, format!("cannot start mDNS: {e}")))?;
        let (seer, loser) = (self.clone(), self.clone());
        let thread = discovery::browse(
            &daemon,
            move |sighting| seer.see(sighting),
            move |fullname| loser.lose(fullname),
        )
        .map_err(|e| Error::new(ErrorCode::SetupFailed, format!("cannot browse: {e}")))?;
        if let Some(old) = self.inner.mdns.lock().unwrap().replace(daemon) {
            let _ = old.shutdown();
        }
        Ok(thread)
    }

    fn keep_browsing(&self, mut thread: std::thread::JoinHandle<()>) {
        let mut backoff = Backoff::new(backoff::Policy::RECONNECT);
        loop {
            let _ = thread.join();
            if self.inner.stopping.load(Ordering::SeqCst) {
                return;
            }
            let wait = backoff.failed(Instant::now());
            tracing::warn!(?wait, "Cast discovery stopped; browsing again after a wait");
            std::thread::sleep(wait);
            loop {
                if self.inner.stopping.load(Ordering::SeqCst) {
                    return;
                }
                match self.browse() {
                    Ok(started) => {
                        backoff.working(Instant::now());
                        thread = started;
                        break;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "cannot browse for Cast devices");
                        std::thread::sleep(backoff.failed(Instant::now()));
                    }
                }
            }
        }
    }

    /// A device was found, through mDNS. Tests use this to point at stand-ins.
    pub fn see(&self, sighting: Sighting) {
        let id = sighting.id();
        self.inner
            .services
            .lock()
            .unwrap()
            .insert(sighting.fullname.clone(), id.clone());
        let mut endpoints = self.inner.endpoints.lock().unwrap();
        if let Some(endpoint) = endpoints.get(&id) {
            let shared = &endpoint.shared;
            *shared.sighting.lock().unwrap() = sighting;
            shared.discovered.store(true, Ordering::SeqCst);
            shared.players.changed(&id);
            return;
        }
        tracing::info!(device = id, name = sighting.name, address = %sighting.address, "Cast device found");
        let (commands, inbox) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            id: id.clone(),
            sighting: Mutex::new(sighting),
            discovered: AtomicBool::new(true),
            status: Mutex::new(Status::default()),
            client: self.inner.hosted.host(&id),
            commands,
            settings: self.inner.settings.clone(),
            players: self.inner.players.clone(),
            runtime: self.inner.runtime.clone(),
            release: Mutex::default(),
        });
        self.inner
            .runtime
            .spawn(channel::run(shared.clone(), inbox));
        let endpoint = Arc::new(CastEndpoint { shared });
        endpoints.insert(id, endpoint.clone());
        drop(endpoints);
        self.inner.players.register(endpoint);
    }

    /// A device's mDNS service went away: it is unavailable until it is seen again, though its
    /// channel keeps trying.
    pub fn lose(&self, fullname: &str) {
        let Some(id) = self.inner.services.lock().unwrap().remove(fullname) else {
            return;
        };
        if let Some(endpoint) = self.inner.endpoints.lock().unwrap().get(&id) {
            endpoint.shared.discovered.store(false, Ordering::SeqCst);
            self.inner.players.changed(&id);
        }
    }

    pub fn endpoint(&self, id: &str) -> Option<Arc<CastEndpoint>> {
        self.inner.endpoints.lock().unwrap().get(id).cloned()
    }

    /// Ends discovery and closes every device's channel.
    pub fn shutdown(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        if let Some(daemon) = self.inner.mdns.lock().unwrap().take() {
            let _ = daemon.shutdown();
        }
        for endpoint in self.inner.endpoints.lock().unwrap().values() {
            endpoint.shared.send(Command::Shutdown);
        }
        if let Some(watcher) = self.inner.watcher.lock().unwrap().take() {
            let _ = watcher.join();
        }
    }
}
