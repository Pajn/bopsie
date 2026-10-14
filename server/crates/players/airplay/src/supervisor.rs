//! One `cliairplay` process per speaker, kept running across tracks and sessions.
//!
//! Audio goes to the process's stdin, a single stream for its whole life. Commands go to a
//! named pipe as `KEY=VALUE` lines. What the process says on stdout and stderr is parsed into
//! the state the stream waits on: connected, audio buffered, a START acknowledged, a FLUSH
//! done. A process that exits is started again on the next use, after a short back-off if it
//! failed.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use nix::fcntl::OFlag;
use nix::sys::stat::Mode;

use crate::status::{self, Status};

/// How long a process has to connect to its speaker.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long the command pipe has to be opened by the process.
const PIPE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long to wait for a START to be acknowledged.
const START_TIMEOUT: Duration = Duration::from_secs(8);
/// How long to wait for buffered audio before starting anyway.
const AUDIO_TIMEOUT: Duration = Duration::from_secs(2);
const FLUSH_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a process gets to end its session cleanly when told to.
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How much later than asked a start may be and still count as on time.
pub const LATE_TOLERANCE_MS: u64 = 20;
/// How long after a failed start the next attempt waits.
const BACKOFF: Duration = Duration::from_secs(5);

/// How to start the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub binary: PathBuf,
    /// Every argument, including `--cmdpipe <cmdpipe>` and the speaker's address.
    pub args: Vec<String>,
    pub cmdpipe: PathBuf,
    /// Extra environment, such as `cliairplay`'s own `CLIAIRPLAY_*` switches.
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Failure {
    #[error("{code}: {detail}")]
    Reported { code: String, detail: String },
    #[error("cliairplay exited")]
    Exited,
    #[error("timed out waiting for {0}")]
    Timeout(&'static str),
    #[error("cannot start cliairplay: {0}")]
    Spawn(String),
    #[error("waiting {0:?} after a failure before trying again")]
    BackingOff(Duration),
}

impl From<Failure> for io::Error {
    fn from(failure: Failure) -> Self {
        io::Error::other(failure.to_string())
    }
}

/// What the process has said, since it started.
#[derive(Debug, Clone, Default)]
pub struct Signals {
    pub connected: bool,
    /// Audio is buffered for the next START. Re-armed by a FLUSH.
    pub audio: bool,
    /// The last START acknowledged: requested and real instant.
    pub started: Option<(u64, u64)>,
    /// FLUSHes acknowledged.
    pub flushes: u64,
    /// The speaker's clock: `ptp` or `ntp`, its state, and when it will be ready.
    pub clock: Option<Clock>,
    pub error: Option<(String, String)>,
    pub exited: bool,
    /// STARTs the process moved later than asked.
    pub corrections: u64,
}

/// When a speaker can start, by its clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStart {
    /// It has not said yet.
    Unknown,
    /// Its clock is not measured (NTP), or does not follow ours: start when the timeline says.
    NotMeasured,
    /// From this Unix time.
    From(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    pub mode: String,
    pub state: String,
    pub ready_at_unix_ms: Option<u64>,
}

/// How long after its clock is ready a speaker can start, as Music Assistant plans it.
const CLOCK_MARGIN_MS: u64 = 500;

struct Running {
    child: Child,
    generation: u64,
}

struct Inner {
    id: String,
    make_launch: Box<dyn Fn() -> Result<Launch, String> + Send + Sync>,
    on_status: Box<dyn Fn(&Status) + Send + Sync>,
    on_exit: Box<dyn Fn(bool) + Send + Sync>,
    running: Mutex<Option<Running>>,
    stdin: Mutex<Option<ChildStdin>>,
    pipe: Mutex<Option<File>>,
    signals: Mutex<Signals>,
    changed: Condvar,
    generation: Mutex<u64>,
    /// A START has been sent and not flushed since.
    streaming: Mutex<bool>,
    failed_at: Mutex<Option<Instant>>,
    /// A background connection is under way.
    connecting: std::sync::atomic::AtomicBool,
    /// Why the last connection failed, until one succeeds.
    last_failure: Mutex<Option<Failure>>,
    /// When the process last took audio, or started.
    last_active: Mutex<Instant>,
    /// The running process was told to end, so its exit is expected.
    ending: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

impl Supervisor {
    /// `make_launch` builds the launch when a process starts, so that it carries the volume,
    /// credentials and password current at that moment. `on_status` hears every line, and
    /// `on_exit` that a process ended, and whether it was told to.
    pub fn new(
        id: &str,
        make_launch: impl Fn() -> Result<Launch, String> + Send + Sync + 'static,
        on_status: impl Fn(&Status) + Send + Sync + 'static,
        on_exit: impl Fn(bool) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                id: id.to_owned(),
                make_launch: Box::new(make_launch),
                on_status: Box::new(on_status),
                on_exit: Box::new(on_exit),
                running: Mutex::default(),
                stdin: Mutex::default(),
                pipe: Mutex::default(),
                signals: Mutex::default(),
                changed: Condvar::new(),
                generation: Mutex::new(0),
                streaming: Mutex::new(false),
                failed_at: Mutex::default(),
                connecting: std::sync::atomic::AtomicBool::new(false),
                last_failure: Mutex::default(),
                last_active: Mutex::new(Instant::now()),
                ending: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    pub fn signals(&self) -> Signals {
        self.inner.signals.lock().unwrap().clone()
    }

    pub fn is_running(&self) -> bool {
        self.inner.running.lock().unwrap().is_some() && !self.signals().exited
    }

    /// Whether a START has been sent since the last FLUSH, so the next stream must flush first.
    pub fn is_streaming(&self) -> bool {
        *self.inner.streaming.lock().unwrap()
    }

    /// Waits until `holds` is true of the signals, failing early if the process reports an
    /// error or exits.
    fn wait(
        &self,
        what: &'static str,
        timeout: Duration,
        holds: impl Fn(&Signals) -> bool,
    ) -> Result<Signals, Failure> {
        let deadline = Instant::now() + timeout;
        let mut signals = self.inner.signals.lock().unwrap();
        loop {
            if holds(&signals) {
                return Ok(signals.clone());
            }
            if let Some((code, detail)) = signals.error.clone() {
                return Err(Failure::Reported { code, detail });
            }
            if signals.exited {
                return Err(Failure::Exited);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Failure::Timeout(what));
            }
            signals = self.inner.changed.wait_timeout(signals, left).unwrap().0;
        }
    }

    /// Starts the process if it is not running, and waits until it has connected.
    pub fn ensure_connected(&self) -> Result<(), Failure> {
        if !self.is_running() {
            self.spawn()?;
        }
        let result = self
            .wait("the connection", CONNECT_TIMEOUT, |s| s.connected)
            .inspect_err(|_| self.failed())
            .map(drop);
        *self.inner.last_failure.lock().unwrap() = result.clone().err();
        result
    }

    /// Whether the process runs, has connected, and takes commands.
    pub fn is_connected(&self) -> bool {
        self.is_running() && self.inner.pipe.lock().unwrap().is_some() && self.signals().connected
    }

    /// Connects on another thread, unless that is already under way, so the caller need not
    /// wait. [`Supervisor::failure`] says how it went if it failed.
    pub fn connect_in_background(&self) {
        use std::sync::atomic::Ordering;
        if self.inner.connecting.swap(true, Ordering::SeqCst) {
            return;
        }
        let supervisor = self.clone();
        std::thread::Builder::new()
            .name("cliairplay-connect".into())
            .spawn(move || {
                let _ = supervisor.ensure_connected();
                supervisor.inner.connecting.store(false, Ordering::SeqCst);
            })
            .expect("spawning a connecting thread");
    }

    /// Whether a connection is under way.
    pub fn is_connecting(&self) -> bool {
        self.inner
            .connecting
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Why the last connection failed, if it did and none has succeeded since.
    pub fn failure(&self) -> Option<Failure> {
        self.inner.last_failure.lock().unwrap().clone()
    }

    fn failed(&self) {
        *self.inner.failed_at.lock().unwrap() = Some(Instant::now());
    }

    fn spawn(&self) -> Result<(), Failure> {
        if let Some(at) = *self.inner.failed_at.lock().unwrap() {
            let since = at.elapsed();
            if since < BACKOFF {
                return Err(Failure::BackingOff(BACKOFF - since));
            }
        }
        self.kill();
        let launch = (self.inner.make_launch)().map_err(Failure::Spawn)?;
        let _ = std::fs::remove_file(&launch.cmdpipe);
        nix::unistd::mkfifo(&launch.cmdpipe, Mode::S_IRUSR | Mode::S_IWUSR)
            .map_err(|e| Failure::Spawn(format!("cannot create the command pipe: {e}")))?;
        let mut child = Command::new(&launch.binary)
            .args(&launch.args)
            .envs(launch.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                self.failed();
                Failure::Spawn(e.to_string())
            })?;
        tracing::info!(
            speaker = self.inner.id,
            pid = child.id(),
            "cliairplay started"
        );
        let generation = {
            let mut generation = self.inner.generation.lock().unwrap();
            *generation += 1;
            *generation
        };
        *self.inner.signals.lock().unwrap() = Signals::default();
        *self.inner.streaming.lock().unwrap() = false;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        self.read_lines(stdout, generation, false);
        self.read_lines(stderr, generation, true);
        *self.inner.stdin.lock().unwrap() = child.stdin.take();
        self.inner
            .ending
            .store(false, std::sync::atomic::Ordering::SeqCst);
        *self.inner.running.lock().unwrap() = Some(Running { child, generation });
        *self.inner.last_active.lock().unwrap() = Instant::now();

        let pipe = self.open_pipe(&launch.cmdpipe)?;
        *self.inner.pipe.lock().unwrap() = Some(pipe);
        Ok(())
    }

    /// Opens the write end of the command pipe once the process has opened the read end.
    fn open_pipe(&self, path: &std::path::Path) -> Result<File, Failure> {
        let deadline = Instant::now() + PIPE_TIMEOUT;
        loop {
            match OpenOptions::new()
                .write(true)
                .custom_flags(OFlag::O_NONBLOCK.bits())
                .open(path)
            {
                Ok(file) => {
                    // Back to blocking: commands are short and must not be dropped.
                    let _ = nix::fcntl::fcntl(&file, nix::fcntl::FcntlArg::F_SETFL(OFlag::empty()));
                    return Ok(file);
                }
                Err(_) if Instant::now() < deadline && !self.signals().exited => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) if self.signals().exited => {
                    self.failed();
                    return Err(self
                        .signals()
                        .error
                        .map_or(Failure::Exited, |(code, detail)| Failure::Reported {
                            code,
                            detail,
                        }));
                }
                Err(error) => {
                    self.failed();
                    return Err(Failure::Spawn(format!(
                        "the command pipe was never opened: {error}"
                    )));
                }
            }
        }
    }

    fn read_lines(&self, stream: impl Read + Send + 'static, generation: u64, is_stderr: bool) {
        let supervisor = self.clone();
        std::thread::Builder::new()
            .name(format!(
                "cliairplay-{}",
                if is_stderr { "stderr" } else { "stdout" }
            ))
            .spawn(move || {
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    if !line.trim().is_empty() {
                        tracing::debug!(speaker = supervisor.inner.id, "{line}");
                    }
                    if let Some(status) = status::parse(&line) {
                        supervisor.observe(generation, status);
                    }
                }
                // stderr closes when the process exits.
                if is_stderr && *supervisor.inner.generation.lock().unwrap() == generation {
                    supervisor.signals_mut().exited = true;
                    supervisor.inner.changed.notify_all();
                    tracing::info!(speaker = supervisor.inner.id, "cliairplay exited");
                    let expected = supervisor
                        .inner
                        .ending
                        .load(std::sync::atomic::Ordering::SeqCst);
                    (supervisor.inner.on_exit)(expected);
                }
            })
            .expect("spawning a reader thread");
    }

    fn signals_mut(&self) -> MutexGuard<'_, Signals> {
        self.inner.signals.lock().unwrap()
    }

    fn observe(&self, generation: u64, status: Status) {
        if *self.inner.generation.lock().unwrap() != generation {
            return;
        }
        {
            let mut signals = self.signals_mut();
            match &status {
                Status::Connected => signals.connected = true,
                Status::Audio { .. } => signals.audio = true,
                Status::Started {
                    requested_unix_ms,
                    at_unix_ms,
                } => signals.started = Some((*requested_unix_ms, *at_unix_ms)),
                Status::Flushed { .. } => {
                    signals.flushes += 1;
                    signals.audio = false;
                }
                Status::ClockReady {
                    mode,
                    state,
                    ready_at_unix_ms,
                } => {
                    signals.clock = Some(Clock {
                        mode: mode.clone(),
                        state: state.clone(),
                        ready_at_unix_ms: *ready_at_unix_ms,
                    });
                }
                Status::Error { code, detail, .. } => {
                    tracing::warn!(
                        speaker = self.inner.id,
                        code,
                        detail,
                        "cliairplay reported an error"
                    );
                    if status.is_terminal() {
                        signals.error = Some((code.clone(), detail.clone()));
                    }
                }
                Status::Reanchor { shifted_frames } => {
                    tracing::warn!(
                        speaker = self.inner.id,
                        shifted_frames,
                        "the stream was re-anchored after a gap"
                    );
                }
                _ => {}
            }
        }
        self.inner.changed.notify_all();
        (self.inner.on_status)(&status);
    }

    /// Sends command lines, newline-terminated, as one write.
    pub fn command(&self, lines: &[String]) -> io::Result<()> {
        let mut pipe = self.inner.pipe.lock().unwrap();
        let pipe = pipe
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
        let mut text = lines.join("\n");
        text.push('\n');
        pipe.write_all(text.as_bytes())
    }

    /// Writes audio to the stream. Blocks while the process's input is full, which paces the
    /// caller.
    pub fn write_audio(&self, data: &[u8]) -> io::Result<()> {
        let mut stdin = self.inner.stdin.lock().unwrap();
        let stdin = stdin
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
        stdin.write_all(data)?;
        *self.inner.last_active.lock().unwrap() = Instant::now();
        Ok(())
    }

    /// How long a running process has gone without audio.
    pub fn idle_for(&self) -> Option<Duration> {
        self.is_running()
            .then(|| self.inner.last_active.lock().unwrap().elapsed())
    }

    /// When the speaker can start playing in step with our clock, as far as is known now.
    ///
    /// A speaker that has just connected starts following our PTP clock a few seconds later,
    /// on its own schedule; a start before then plays out of step or not at all.
    pub fn clock_start(&self) -> ClockStart {
        let signals = self.signals();
        let Some(clock) = signals.clock else {
            return ClockStart::Unknown;
        };
        match (
            clock.mode.as_str(),
            clock.state.as_str(),
            clock.ready_at_unix_ms,
        ) {
            ("ntp", _, _) => ClockStart::NotMeasured,
            (_, "stalled", _) => {
                tracing::warn!(
                    speaker = self.inner.id,
                    "the speaker does not follow our clock"
                );
                ClockStart::NotMeasured
            }
            (_, _, Some(ms)) => ClockStart::From(ms + CLOCK_MARGIN_MS),
            _ => ClockStart::Unknown,
        }
    }

    /// Commands the first sample buffered since the last FLUSH to be audible at `unix_ms`,
    /// and returns when it really will be: later when the speaker could not make it.
    pub fn start(&self, unix_ms: u64) -> Result<u64, Failure> {
        // Start once audio is buffered; a speaker that never says so is started anyway.
        let _ = self.wait("buffered audio", AUDIO_TIMEOUT, |s| s.audio);
        self.signals_mut().started = None;
        self.command(&[format!("START_UNIX_MS={unix_ms}"), "ACTION=START".into()])
            .map_err(|e| Failure::Spawn(e.to_string()))?;
        *self.inner.streaming.lock().unwrap() = true;
        let signals = self.wait("the start", START_TIMEOUT, |s| s.started.is_some())?;
        let (_, at) = signals.started.expect("waited for it");
        if at > unix_ms + LATE_TOLERANCE_MS {
            self.signals_mut().corrections += 1;
            tracing::warn!(
                speaker = self.inner.id,
                late_ms = at - unix_ms,
                "the speaker starts later than asked"
            );
        }
        Ok(at)
    }

    /// Discards what is buffered, keeping the connection, and waits until it is done.
    pub fn flush(&self) -> Result<(), Failure> {
        let before = self.signals().flushes;
        self.command(&["ACTION=FLUSH".into()])
            .map_err(|e| Failure::Spawn(e.to_string()))?;
        *self.inner.streaming.lock().unwrap() = false;
        self.wait("the flush", FLUSH_TIMEOUT, |s| s.flushes > before)
            .map(drop)
    }

    /// Ends the session cleanly, so the speaker knows it is over, then the process.
    pub fn disconnect(&self) {
        self.inner
            .ending
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if self.command(&["ACTION=DISCONNECT".into()]).is_ok() {
            let _ = self.wait("the disconnection", DISCONNECT_TIMEOUT, |s| s.exited);
        }
        self.kill();
    }

    /// Ends the process, if one runs.
    pub fn kill(&self) {
        self.inner
            .ending
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let running = self.inner.running.lock().unwrap().take();
        *self.inner.pipe.lock().unwrap() = None;
        *self.inner.stdin.lock().unwrap() = None;
        if let Some(mut running) = running {
            let _ = running.child.kill();
            let _ = running.child.wait();
            tracing::debug!(
                speaker = self.inner.id,
                generation = running.generation,
                "cliairplay ended"
            );
        }
    }
}
