//! A Spotify track's decode path, on an Ogg Vorbis file behind a header as Spotify's are,
//! made with ffmpeg. Skipped without ffmpeg.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use bopsie_audio::decode::{Decoder, Read as Decoded, find_ffmpeg};
use bopsie_fixtures::{noise, write_wav};
use bopsie_sources_spotify::media::{FORMAT, Fetching, Subfile};

/// Three seconds of noise as Ogg Vorbis, behind a header of 0xa7 bytes.
fn spotify_like(dir: &Path, ffmpeg: &Path) -> PathBuf {
    let wav = dir.join("a.wav");
    write_wav(&wav, 44_100, 2, &noise(7, 44_100 * 2 * 3)).unwrap();
    let ogg = dir.join("a.ogg");
    let made = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(&wav)
        .args(["-c:a", "libvorbis", "-q:a", "6"])
        .arg(&ogg)
        .status()
        .unwrap();
    assert!(made.success());
    let mut bytes = vec![0u8; 0xa7];
    bytes.extend(std::fs::read(&ogg).unwrap());
    let track = dir.join("track");
    std::fs::write(&track, bytes).unwrap();
    track
}

fn open(path: &Path) -> Fetching {
    let path = path.to_owned();
    Fetching::start(move || {
        let length = std::fs::metadata(&path)?.len();
        Ok(Box::new(Subfile::new(File::open(&path)?, 0xa7, length)?))
    })
}

/// Frames read until the end.
fn frames(decoder: &mut Fetching) -> u64 {
    let mut out = Vec::new();
    let mut frames = 0;
    loop {
        out.clear();
        match decoder.read(&mut out).unwrap() {
            Decoded::Data => frames += (out.len() / 2) as u64,
            Decoded::Pending => {}
            Decoded::End => return frames,
        }
    }
}

#[test]
fn a_track_plays_through_and_seeks() {
    let Some(ffmpeg) = find_ffmpeg(None) else {
        eprintln!("skipped: no ffmpeg");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let track = spotify_like(dir.path(), &ffmpeg);

    let mut decoder = open(&track);
    assert_eq!(decoder.format(), FORMAT);
    let all = frames(&mut decoder);
    assert!(all.abs_diff(44_100 * 3) <= 2_048, "{all} frames");

    let mut decoder = open(&track);
    assert_eq!(decoder.seek(44_100 * 2).unwrap(), 44_100 * 2);
    let rest = frames(&mut decoder);
    assert!(
        rest.abs_diff(44_100) <= 2_048,
        "{rest} frames after the seek"
    );
}
