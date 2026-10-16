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
use bopsie_sources_spotify::media::{FORMAT, SPOTIFY_REFERENCE_LUFS, SpotifyMedia};
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
    println!("track                   header dB  measured dB  implied reference LUFS");
    let mut implied = Vec::new();
    for id in args {
        let track = SpotifyId::from_base62(&id).expect("a track id");
        let media = SpotifyMedia::new(account.clone(), track, tokio::runtime::Handle::current());
        let result = tokio::task::spawn_blocking(move || -> Result<(f32, f32), String> {
            let mut decoder = media.open(FORMAT).map_err(|e| e.to_string())?;
            let mut state = ebur128::EbuR128::new(
                2,
                FORMAT.sample_rate,
                ebur128::Mode::I | ebur128::Mode::HISTOGRAM,
            )
            .map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            loop {
                out.clear();
                match decoder.read(&mut out).map_err(|e| e.to_string())? {
                    Read::End => break,
                    Read::Pending => continue,
                    Read::Data => state.add_frames_f32(&out).map_err(|e| e.to_string())?,
                }
            }
            let header = decoder
                .loudness()
                .ok_or("no loudness in the header")?
                .track_gain_db;
            let lufs = state.loudness_global().map_err(|e| e.to_string())?;
            Ok((header, REFERENCE_LUFS - lufs as f32))
        })
        .await
        .unwrap();
        match result {
            Ok((header, measured)) => {
                // The header's gains are taken as being to SPOTIFY_REFERENCE_LUFS; they bring
                // the audio to this instead.
                let reference = SPOTIFY_REFERENCE_LUFS - (measured - header);
                println!("{id}  {header:>9.2}  {measured:>11.2}  {reference:>10.2}");
                implied.push(reference);
            }
            Err(error) => println!("{id}  {error}"),
        }
    }
    if !implied.is_empty() {
        let mean = implied.iter().sum::<f32>() / implied.len() as f32;
        println!(
            "Spotify's gains bring tracks to {mean:.2} LUFS, over {} tracks",
            implied.len()
        );
    }
}
