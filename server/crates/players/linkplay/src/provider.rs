//! The LinkPlay provider: devices found on the network become players, each reached through
//! its HTTP API and UPnP renderer, and played through a stream the server serves.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bopsie_core::backoff::{self, Backoff};
use bopsie_core::{Error, ErrorCode, Result};
use bopsie_players::{Endpoint, Players};

use crate::discovery::{self, Sighting};
use crate::endpoint::{Context, Directory, LinkplayEndpoint, ServerAddress};
use crate::stream::StreamHub;
use crate::transport::{self, Api};
use crate::upnp::{self, Upnp};

#[derive(Debug, Clone)]
pub struct Config {
    /// LinkPlay's client certificate and key, as PEM: the bundled one unless the config file
    /// names another.
    pub certificate: Vec<u8>,
    /// Where devices reach the server for streams.
    pub server: ServerAddress,
    /// Where each device's HTTP API may answer, in the order tried.
    pub probes: Vec<(&'static str, u16)>,
    pub upnp_port: u16,
    /// How long a device that left mDNS may come back before it is given up.
    pub lost_grace: Duration,
}

impl Config {
    pub fn new(server: ServerAddress) -> Self {
        Self {
            certificate: transport::CLIENT_CERTIFICATE.to_vec(),
            server,
            probes: transport::PROBES.to_vec(),
            upnp_port: upnp::PORT,
            lost_grace: Duration::from_secs(30),
        }
    }
}

struct Inner {
    players: Players,
    config: Config,
    hub: StreamHub,
    endpoints: Arc<Directory>,
    /// Which player id each mDNS service is.
    services: Mutex<HashMap<String, String>>,
    mdns: Mutex<Option<mdns_sd::ServiceDaemon>>,
    watcher: Mutex<Option<std::thread::JoinHandle<()>>>,
    stopping: AtomicBool,
    runtime: tokio::runtime::Handle,
}

#[derive(Clone)]
pub struct LinkplayProvider {
    inner: Arc<Inner>,
}

impl LinkplayProvider {
    /// Checks the certificate. Discovery starts with [`LinkplayProvider::discover`]. Needs a
    /// Tokio runtime.
    pub fn new(players: Players, config: Config) -> Result<Self> {
        // Fails here, with the reason, rather than for every device.
        Api::new(IpAddr::from([127, 0, 0, 1]), &config.certificate)
            .map_err(|e| Error::new(ErrorCode::SetupFailed, format!("LinkPlay: {e}")))?;
        Ok(Self {
            inner: Arc::new(Inner {
                players,
                config,
                hub: StreamHub::default(),
                endpoints: Arc::default(),
                services: Mutex::default(),
                mdns: Mutex::default(),
                watcher: Mutex::default(),
                stopping: AtomicBool::new(false),
                runtime: tokio::runtime::Handle::current(),
            }),
        })
    }

    /// Where devices pull their streams from: to be served on the API's port.
    pub fn hub(&self) -> StreamHub {
        self.inner.hub.clone()
    }

    /// Browses the network for devices, browsing again whenever browsing ends.
    pub fn discover(&self) -> Result<()> {
        let first = self.browse()?;
        let provider = self.clone();
        let watcher = std::thread::Builder::new()
            .name("linkplay-mdns".into())
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
            tracing::warn!(
                ?wait,
                "LinkPlay discovery stopped; browsing again after a wait"
            );
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
                        tracing::warn!(%error, "cannot browse for LinkPlay devices");
                        std::thread::sleep(backoff.failed(Instant::now()));
                    }
                }
            }
        }
    }

    /// A device was found, through mDNS.
    pub fn see(&self, sighting: Sighting) {
        let Some(id) = sighting.id() else {
            return;
        };
        let api = match Api::with_probes(
            sighting.address,
            &self.inner.config.certificate,
            &self.inner.config.probes,
        ) {
            Ok(api) => api,
            Err(error) => {
                tracing::warn!(%error, "cannot reach LinkPlay devices");
                return;
            }
        };
        let upnp = Upnp::new(sighting.address, self.inner.config.upnp_port);
        self.add(sighting, id, api, upnp);
    }

    /// A device, reached through `api` and `upnp`. Tests use this to point at stand-ins.
    pub fn add(&self, sighting: Sighting, id: String, api: Api, upnp: Upnp) {
        self.inner
            .services
            .lock()
            .unwrap()
            .insert(sighting.fullname.clone(), id.clone());
        let mut endpoints = self.inner.endpoints.lock().unwrap();
        if let Some(endpoint) = endpoints.get(&id).cloned() {
            drop(endpoints);
            endpoint.rediscovered(&sighting);
            return;
        }
        tracing::info!(device = id, name = sighting.name, address = %sighting.address, "LinkPlay device found");
        let _entered = self.inner.runtime.enter();
        let endpoint = LinkplayEndpoint::new(
            &sighting,
            id.clone(),
            api,
            upnp,
            Context {
                players: self.inner.players.clone(),
                hub: self.inner.hub.clone(),
                server: self.inner.config.server.clone(),
                directory: Arc::downgrade(&self.inner.endpoints),
            },
        );
        endpoints.insert(id, endpoint.clone());
        drop(endpoints);
        self.inner.players.register(endpoint);
    }

    /// A device's mDNS service went away: it is given up unless it comes back in time.
    pub fn lose(&self, fullname: &str) {
        let Some(id) = self.inner.services.lock().unwrap().remove(fullname) else {
            return;
        };
        let provider = self.clone();
        let grace = self.inner.config.lost_grace;
        let lose = move || {
            if provider
                .inner
                .services
                .lock()
                .unwrap()
                .values()
                .any(|known| *known == id)
            {
                return;
            }
            if let Some(endpoint) = provider.inner.endpoints.lock().unwrap().get(&id) {
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

    pub fn endpoint(&self, id: &str) -> Option<Arc<LinkplayEndpoint>> {
        self.inner.endpoints.lock().unwrap().get(id).cloned()
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

    /// Ends discovery and stops reading devices.
    pub fn shutdown(&self) {
        self.inner.stopping.store(true, Ordering::SeqCst);
        if let Some(daemon) = self.inner.mdns.lock().unwrap().take() {
            let _ = daemon.shutdown();
        }
        for endpoint in self.inner.endpoints.lock().unwrap().values() {
            endpoint.shutdown();
        }
        if let Some(watcher) = self.inner.watcher.lock().unwrap().take() {
            let _ = watcher.join();
        }
    }
}
