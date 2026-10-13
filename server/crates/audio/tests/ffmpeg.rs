//! Decoding through ffmpeg. These tests need ffmpeg: the Nix dev shell and CI provide it.
//! Without it they are skipped, unless `BOPSIE_REQUIRE_FFMPEG` is set, as it is in CI.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::time::Duration;

use bopsie_audio::decode::{Decoder, FfmpegDecoder, find_ffmpeg};
use bopsie_audio::engine::{self, Command, Event, Settings, TrackRequest, TrackSource};
use bopsie_audio::sink::{Sink, WavSink};
use bopsie_audio::{OutputFormat, PcmFormat};
use bopsie_fixtures::{noise, write_wav};

const SESSION: PcmFormat = PcmFormat {
    sample_rate: 44_100,
    channels: 2,
};

fn ffmpeg() -> Option<PathBuf> {
    let found = find_ffmpeg(None);
    if found.is_none() {
        assert!(
            std::env::var_os("BOPSIE_REQUIRE_FFMPEG").is_none(),
            "ffmpeg is required but not installed"
        );
        eprintln!("skipping: ffmpeg is not installed");
    }
    found
}

fn encode_alac(ffmpeg: &Path, wav: &Path, m4a: &Path) {
    let status = Process::new(ffmpeg)
        .args(["-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(wav)
        .args(["-c:a", "alac"])
        .arg(m4a)
        .status()
        .unwrap();
    assert!(status.success());
}

fn decode_all(decoder: &mut dyn Decoder) -> Vec<f32> {
    let mut samples = Vec::new();
    while decoder.read(&mut samples).unwrap() {}
    samples
}

fn floats(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| f32::from(s) / 32_768.0).collect()
}

#[test]
fn decodes_and_seeks_exactly() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().unwrap();
    let samples = noise(11, 10_007 * 2);
    let wav = dir.path().join("a.wav");
    write_wav(&wav, 44_100, 2, &samples).unwrap();

    let mut decoder = FfmpegDecoder::open(&ffmpeg, &wav, SESSION).unwrap();
    assert_eq!(decode_all(&mut decoder), floats(&samples));

    decoder.seek(5_000).unwrap();
    assert_eq!(decode_all(&mut decoder), floats(&samples[5_000 * 2..]));
}

struct Tracks(VecDeque<TrackRequest>);

impl TrackSource for Tracks {
    fn next(&mut self) -> Option<TrackRequest> {
        self.0.pop_front()
    }
}

/// ALAC is lossless and in an MP4 container, so the engine sends it through ffmpeg, and the
/// result must match the source exactly, with no encoder padding at the joins.
#[test]
fn mp4_tracks_play_gaplessly_through_ffmpeg() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let dir = tempfile::tempdir().unwrap();
    let a = noise(12, 9_999 * 2);
    let b = noise(13, 3_333 * 2);
    for (name, samples) in [("a", &a), ("b", &b)] {
        let wav = dir.path().join(format!("{name}.wav"));
        write_wav(&wav, 44_100, 2, samples).unwrap();
        encode_alac(&ffmpeg, &wav, &dir.path().join(format!("{name}.m4a")));
    }

    let out = dir.path().join("out.wav");
    let format = OutputFormat {
        sample_rate: 44_100,
        channels: 2,
        bits: 16,
    };
    let sinks: Vec<(String, Box<dyn Sink>)> = vec![(
        "out".to_owned(),
        Box::new(WavSink::create(&out, format).unwrap()),
    )];
    let tracks = ["a", "b"].map(|name| TrackRequest {
        id: name.to_owned(),
        path: dir.path().join(format!("{name}.m4a")),
        start: Duration::ZERO,
    });
    let settings = Settings {
        format: SESSION,
        ffmpeg: Some(ffmpeg),
        ring_frames: 0,
        realtime: None,
    };
    let mut handle = engine::spawn(settings, Box::new(Tracks(tracks.into())), sinks);
    handle.send(Command::Restart {
        epoch: 1,
        not_before: None,
    });
    while !matches!(
        handle.events.blocking_recv().map(|t| t.event),
        Some(Event::Idle { .. }) | None
    ) {}
    handle.shutdown();

    let expected: Vec<u8> = a.iter().chain(&b).flat_map(|s| s.to_le_bytes()).collect();
    let played = std::fs::read(&out).unwrap()[44..].to_vec();
    assert_eq!(played.len(), expected.len());
    assert!(
        played == expected,
        "the output differs from the tracks back to back"
    );
}
