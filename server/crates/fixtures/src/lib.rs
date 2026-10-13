//! The test library: the same two albums of tones that `mobile/scripts/ma-server.sh` gives
//! Music Assistant, so tests make the same assertions against either server. The tracks are
//! WAV files with ID3v2 tags, which need no encoder to write; each album folder has a
//! `cover.jpg`. A playlist at the root lists two tracks from different albums.

use std::f32::consts::TAU;
use std::fs;
use std::io;
use std::path::Path;

use lofty::config::WriteOptions;
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag, TagType};

pub struct Album {
    pub artist: &'static str,
    pub album: &'static str,
    pub year: u16,
    pub titles: &'static [&'static str],
}

pub const ALBUMS: &[Album] = &[
    Album {
        artist: "Test Artist",
        album: "First Album",
        year: 2021,
        titles: &["Opening", "Middle", "Closing"],
    },
    Album {
        artist: "Other Artist",
        album: "Second Album",
        year: 2023,
        titles: &["Alpha", "Beta", "Gamma"],
    },
];

/// The playlist's file name and the tracks it lists, relative to the library root.
pub const PLAYLIST: (&str, &[&str]) = (
    "Mix.m3u",
    &[
        "Test Artist/First Album/01 - Opening.wav",
        "Other Artist/Second Album/03 - Gamma.wav",
    ],
);

const SAMPLE_RATE: u32 = 44_100;

/// Writes the library into `root`, each track `seconds` long.
pub fn write_library(root: &Path, seconds: u32) -> io::Result<()> {
    let mut frequency = 330.0;
    for (index, album) in ALBUMS.iter().enumerate() {
        let dir = root.join(album.artist).join(album.album);
        fs::create_dir_all(&dir)?;
        write_cover(&dir.join("cover.jpg"), index)?;
        for (number, title) in (1..).zip(album.titles) {
            let path = dir.join(format!("{number:02} - {title}.wav"));
            write_tone(&path, frequency, seconds)?;
            let mut tag = Tag::new(TagType::Id3v2);
            tag.set_title((*title).to_owned());
            tag.set_artist(album.artist.to_owned());
            tag.set_album(album.album.to_owned());
            tag.insert_text(ItemKey::AlbumArtist, album.artist.to_owned());
            tag.insert_text(ItemKey::RecordingDate, album.year.to_string());
            tag.set_track(number);
            tag.save_to_path(&path, WriteOptions::default())
                .map_err(io::Error::other)?;
            frequency += 55.0;
        }
    }
    let (name, entries) = PLAYLIST;
    fs::write(
        root.join(name),
        format!("#EXTM3U\n{}\n", entries.join("\n")),
    )
}

/// A sine tone as 16-bit stereo PCM.
fn write_tone(path: &Path, frequency: f32, seconds: u32) -> io::Result<()> {
    let samples: Vec<i16> = (0..SAMPLE_RATE * seconds)
        .flat_map(|frame| {
            let t = frame as f32 / SAMPLE_RATE as f32;
            let sample = ((TAU * frequency * t).sin() * 0.25 * f32::from(i16::MAX)) as i16;
            [sample, sample]
        })
        .collect();
    write_wav(path, SAMPLE_RATE, 2, &samples)
}

/// Writes interleaved 16-bit samples as a WAV file.
pub fn write_wav(path: &Path, sample_rate: u32, channels: u16, samples: &[i16]) -> io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let block_align = channels * 2;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    fs::write(path, bytes)
}

/// Deterministic noise: different in every sample, so a dropped or repeated sample shows.
pub fn noise(seed: u32, len: usize) -> Vec<i16> {
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 16) as i16
        })
        .collect()
}

/// A solid square, a different colour per album.
fn write_cover(path: &Path, index: usize) -> io::Result<()> {
    let colours = [[200, 80, 60], [60, 120, 200]];
    let image = image::RgbImage::from_pixel(300, 300, image::Rgb(colours[index % 2]));
    image.save(path).map_err(io::Error::other)
}
