//! Which `cliairplay` processes this server started, kept on disk so that those a crashed run
//! left behind can be ended at the next start. The PTP daemon among them holds UDP ports 319
//! and 320, which no new one can have while it runs.

use std::path::Path;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// How long a process that was asked to end gets before it is killed.
const TERM_GRACE: Duration = Duration::from_secs(1);

/// Notes that `pid` was started, in `path`.
pub fn record(path: &Path, pid: u32) {
    if let Err(error) = std::fs::write(path, pid.to_string()) {
        tracing::warn!(path = %path.display(), %error, "cannot record a cliairplay process");
    }
}

pub fn forget(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Ends every `cliairplay` process recorded in `dir` that still runs, and forgets them all.
/// Returns how many were ended.
pub fn end_leftovers(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut ended = 0;
    for path in entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "pid"))
    {
        let pid = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| text.trim().parse::<i32>().ok());
        forget(&path);
        let Some(pid) = pid.filter(|pid| *pid > 0).map(Pid::from_raw) else {
            continue;
        };
        if !is_cliairplay(pid) {
            continue;
        }
        tracing::info!(
            pid = pid.as_raw(),
            "ending a cliairplay process an earlier run left"
        );
        let _ = kill(pid, Signal::SIGTERM);
        let deadline = Instant::now() + TERM_GRACE;
        while running(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if running(pid) {
            let _ = kill(pid, Signal::SIGKILL);
        }
        ended += 1;
    }
    ended
}

fn running(pid: Pid) -> bool {
    kill(pid, None).is_ok() && !is_zombie(pid)
}

/// What `ps` says of `pid`: its state and command name.
fn ps(pid: Pid, field: &str) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-p", &pid.as_raw().to_string(), "-o", &format!("{field}=")])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn is_zombie(pid: Pid) -> bool {
    ps(pid, "stat").is_some_and(|stat| stat.starts_with('Z'))
}

/// Whether `pid` is a `cliairplay`: a recorded pid may since have gone to another program.
fn is_cliairplay(pid: Pid) -> bool {
    ps(pid, "comm").is_some_and(|command| {
        Path::new(&command)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("cliairplay"))
    })
}
