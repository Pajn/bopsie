//! A book's files played one after another as one stream, from a stand-in server over HTTP.
//! These tests need ffmpeg; without it they are skipped, unless `BOPSIE_REQUIRE_FFMPEG` is set.

mod common;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bopsie_audio::PcmFormat;
use bopsie_audio::decode::{Decoder, Read, find_ffmpeg};
use bopsie_audio::engine::Media;
use bopsie_core::db::Database;
use bopsie_fixtures::{noise, write_wav};
use bopsie_sources_audiobookshelf::Account;
use bopsie_sources_audiobookshelf::media::BookMedia;

const FORMAT: PcmFormat = PcmFormat {
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

/// Reads `frames` frames, waiting for the network.
fn read(decoder: &mut Box<dyn Decoder>, frames: usize) -> Vec<f32> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut out = Vec::new();
    while out.len() < frames * 2 {
        assert!(
            Instant::now() < deadline,
            "the book stalled at {}",
            out.len()
        );
        match decoder.read(&mut out).unwrap() {
            Read::End => break,
            Read::Pending | Read::Data => {}
        }
    }
    out.truncate(frames * 2);
    out
}

fn as_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| f32::from(s) / 32_768.0).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_books_files_play_as_one_stream_and_seek_into_the_right_one() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let (url, stand) = common::start().await;
    let dir = tempfile::tempdir().unwrap();
    // Three files of 1 s, 0.5 s and 1 s, each its own noise.
    let lengths = [44_100usize, 22_050, 44_100];
    let mut all = Vec::new();
    for (i, &frames) in lengths.iter().enumerate() {
        let samples = noise(10 + i as u32, frames * 2);
        let path = dir.path().join(format!("{i}.wav"));
        write_wav(&path, 44_100, 2, &samples).unwrap();
        stand
            .lock()
            .unwrap()
            .files
            .push((path, frames as f64 / 44_100.0));
        all.extend(samples);
    }
    let db = Database::open_in_memory(&[]).unwrap();
    let account = Account::open(db).await.unwrap();
    account
        .sign_in(&url, "reader", common::PASSWORD)
        .await
        .unwrap();
    let media = BookMedia::new(
        account,
        "li_h".to_owned(),
        ffmpeg,
        tokio::runtime::Handle::current(),
        Default::default(),
    );

    let played = tokio::task::spawn_blocking(move || {
        let mut book = media.open(FORMAT).unwrap();
        let whole = read(&mut book, 110_250 + 100);
        // Into the middle of the second file: 1.25 s in.
        book.seek(55_125).unwrap();
        let after = read(&mut book, 2_000);
        (whole, after)
    })
    .await
    .unwrap();
    let (whole, after) = played;
    let expected = as_f32(&all);
    assert_eq!(whole.len(), expected.len(), "every file, and nothing more");
    assert!(
        whole == expected,
        "the files differ from the book back to back"
    );
    // Seeking by jumping lands within a packet: find where it landed, near 55 125.
    let landed = (55_125 - 500..55_125 + 500)
        .find(|&frame| after[..200] == expected[frame * 2..frame * 2 + 200])
        .expect("the seek landed in the second file");
    assert!((landed as i64 - 55_125).abs() < 500, "{landed}");
    assert_eq!(stand.lock().unwrap().sessions, 1);
}
