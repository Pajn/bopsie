//! Checks the reference Spotify's loudness figures are to: for each track, the gain its header
//! gives as the engine reads it, taken from Spotify's reference to ReplayGain's, beside the
//! gain measuring its decoded audio with EBU R128 gives. They agree when the reference is
//! right. Uses the account the server signed in with, under the data directory given.
//!
//! ```sh
//! cargo run --release -p bopsie-sources-spotify --example spotify-loudness-probe -- \
//!     <data dir> <track id>...
//! ```

use std::path::PathBuf;

use bopsie_audio::decode::Read;
use bopsie_audio::engine::Media;
use bopsie_audio::loudness::REFERENCE_LUFS;
use bopsie_core::db::Database;
use bopsie_sources_spotify::media::{FORMAT, SpotifyMedia};
use bopsie_sources_spotify::{Endpoints, SpotifyAccount};
use librespot_core::SpotifyId;

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let mut args = std::env::args().skip(1);
    let data_dir = PathBuf::from(args.next().expect("a data directory"));
    let db = Database::open_in_memory(&[]).expect("a database");
    let account = SpotifyAccount::open(db, data_dir, Endpoints::default())
        .await
        .expect("the account");
    println!("track                   header dB  measured dB  difference");
    let mut differences = Vec::new();
    for id in args {
        let track = SpotifyId::from_base62(&id).expect("a track id");
        let media = SpotifyMedia::new(account.clone(), track, tokio::runtime::Handle::current());
        let result = tokio::task::spawn_blocking(move || {
            let mut decoder = media.open(FORMAT).ok()?;
            let mut state = ebur128::EbuR128::new(
                2,
                FORMAT.sample_rate,
                ebur128::Mode::I | ebur128::Mode::HISTOGRAM,
            )
            .ok()?;
            let mut out = Vec::new();
            loop {
                out.clear();
                match decoder.read(&mut out).ok()? {
                    Read::End => break,
                    Read::Pending => continue,
                    Read::Data => state.add_frames_f32(&out).ok()?,
                }
            }
            let header = decoder.loudness()?.track_gain_db;
            let measured = REFERENCE_LUFS - state.loudness_global().ok()? as f32;
            Some((header, measured))
        })
        .await
        .unwrap();
        match result {
            Some((header, measured)) => {
                println!(
                    "{id}  {header:>9.2}  {measured:>11.2}  {:>10.2}",
                    measured - header
                );
                differences.push(measured - header);
            }
            None => println!("{id}  could not be measured"),
        }
    }
    if !differences.is_empty() {
        let mean = differences.iter().sum::<f32>() / differences.len() as f32;
        println!(
            "mean difference {mean:.2} dB over {} tracks",
            differences.len()
        );
    }
}
