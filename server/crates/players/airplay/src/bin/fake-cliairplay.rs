//! A stand-in for `cliairplay`, for tests: it speaks the same process contract — audio on
//! stdin, `KEY=VALUE` commands on the `--cmdpipe` pipe, `[STATUS]` lines on stdout and stderr
//! — with no speaker behind it.
//!
//! It records what it is given in `FAKE_CLIAIRPLAY_DIR`: `args` (one argument per line),
//! `audio.pcm` (everything read from stdin) and `commands` (each command line, prefixed by how
//! many audio bytes had arrived when it did). Other variables shape its behaviour:
//! `FAKE_CLIAIRPLAY_FAIL=<code>` fails to connect with that error code,
//! `FAKE_CLIAIRPLAY_CONNECT_MS` delays the connection, and `FAKE_CLIAIRPLAY_START_LATE_MS`
//! moves every START that much later than asked.
//!
//! Like the real binary, it takes audio no faster than it would play it, after a buffer's
//! worth, so a caller writing audio is paced by it.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn env_u64(name: &str) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// The number after `flag` in `args`, or `default`.
fn env_u64_or(args: &[String], flag: &str, default: u64) -> u64 {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A status line on stderr, where the lifecycle lines go.
fn status(line: &str) {
    eprintln!("[STATUS] {line}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--check") {
        println!("cliairplay fake check");
        return;
    }
    let dir = PathBuf::from(std::env::var("FAKE_CLIAIRPLAY_DIR").unwrap_or_else(|_| ".".into()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("args"), args.join("\n")).unwrap();
    let cmdpipe = args
        .iter()
        .position(|a| a == "--cmdpipe")
        .and_then(|i| args.get(i + 1))
        .expect("--cmdpipe is required")
        .clone();

    println!("[STATUS] route protocol=airplay2 flow=native timing=ntp buffered=0");
    if let Ok(code) = std::env::var("FAKE_CLIAIRPLAY_FAIL") {
        status(&format!(
            "error code={code} http=401 detail=\"fake failure\""
        ));
        eprintln!("[ERROR] fake failure");
        std::process::exit(1);
    }
    std::thread::sleep(Duration::from_millis(env_u64("FAKE_CLIAIRPLAY_CONNECT_MS")));
    status("connected");
    status(
        "clock_ready mode=ntp state=cold streak_ms=0 exchanges=0 ready_in_ms=0 ready_at_unix_ms=0",
    );
    println!(
        "[STATUS] latency lead_ms=2000 device_min_frames=11025 device_max_frames=88200 device_render_ms=0 warm_lead_ms=600"
    );

    let received = Arc::new(AtomicU64::new(0));
    // Whether the next audio should be reported as buffered.
    let armed = Arc::new(AtomicBool::new(true));

    let rate = env_u64_or(&args, "--samplerate", 44_100) * 2 * 2;
    let audio_path = dir.join("audio.pcm");
    {
        let (received, armed) = (received.clone(), armed.clone());
        std::thread::spawn(move || {
            let mut out = File::create(audio_path).unwrap();
            let mut stdin = std::io::stdin().lock();
            let mut buffer = vec![0u8; 16 * 1024];
            let begun = std::time::Instant::now();
            loop {
                // A second ahead of real time at most.
                let allowed = rate + (begun.elapsed().as_secs_f64() * rate as f64) as u64;
                if received.load(Ordering::SeqCst) >= allowed {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                let n = match stdin.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                out.write_all(&buffer[..n]).unwrap();
                out.flush().unwrap();
                received.fetch_add(n as u64, Ordering::SeqCst);
                if armed.swap(false, Ordering::SeqCst) {
                    status("audio buffered_ms=20");
                }
            }
            status("eof");
            std::process::exit(0);
        });
    }

    let pipe = File::open(&cmdpipe).expect("opening the command pipe");
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("commands"))
        .unwrap();
    let late = env_u64("FAKE_CLIAIRPLAY_START_LATE_MS");
    let mut requested = 0u64;
    for line in BufReader::new(pipe).lines() {
        let Ok(line) = line else { break };
        writeln!(log, "{} {line}", received.load(Ordering::SeqCst)).unwrap();
        log.flush().unwrap();
        match line.split_once('=') {
            Some(("START_UNIX_MS", ms)) => requested = ms.parse().unwrap_or(0),
            Some(("ACTION", "START")) => {
                let at = requested.max(now_ms() + 250) + late;
                status(&format!(
                    "started requested_unix_ms={requested} at_unix_ms={at}"
                ));
            }
            Some(("ACTION", "FLUSH")) => {
                armed.store(true, Ordering::SeqCst);
                status("flushed");
            }
            Some(("ACTION", "STOP")) => {
                status("stopped");
                std::process::exit(0);
            }
            Some(("ACTION", "DISCONNECT")) => std::process::exit(0),
            _ => {}
        }
    }
}
