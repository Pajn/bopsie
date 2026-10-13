//! Where the engine's audio goes. Protocol outputs implement [`Sink`]; [`WavSink`] writes a
//! file, which is how tests listen.

use std::fs::File;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use crate::format::OutputFormat;

pub trait Sink: Send {
    fn format(&self) -> OutputFormat;
    /// Plays `data`, packed in [`Sink::format`], starting at timeline frame `at`. May block
    /// while the sink is full, which is how a sink paces the engine.
    fn write(&mut self, at: u64, data: &[u8]) -> io::Result<()>;
    /// Drops what was written but has not played, as for a seek or a jump to another track.
    /// Playback continues with the next write.
    fn clear(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// Stops at once, dropping what has not played.
    fn stop(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// Nothing more is coming for now: stop once everything written has played.
    fn drain(&mut self) -> io::Result<()> {
        Ok(())
    }
    /// The session is over.
    fn finish(&mut self) -> io::Result<()>;
}

/// Writes everything it is given to a WAV file, as fast as it comes. A file has already
/// "played" everything written to it, so clearing and stopping drop nothing.
pub struct WavSink {
    format: OutputFormat,
    file: BufWriter<File>,
    data_bytes: u64,
}

const HEADER_BYTES: u64 = 44;

impl WavSink {
    pub fn create(path: &Path, format: OutputFormat) -> io::Result<Self> {
        let mut file = BufWriter::new(File::create(path)?);
        write_header(&mut file, format, 0)?;
        Ok(Self {
            format,
            file,
            data_bytes: 0,
        })
    }
}

/// A canonical 44-byte PCM WAV header.
pub fn write_header(out: &mut impl Write, format: OutputFormat, data_bytes: u32) -> io::Result<()> {
    let block_align = format.bytes_per_frame() as u16;
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_bytes).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?;
    out.write_all(&format.channels.to_le_bytes())?;
    out.write_all(&format.sample_rate.to_le_bytes())?;
    out.write_all(&(format.sample_rate * u32::from(block_align)).to_le_bytes())?;
    out.write_all(&block_align.to_le_bytes())?;
    out.write_all(&format.bits.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_bytes.to_le_bytes())
}

impl Sink for WavSink {
    fn format(&self) -> OutputFormat {
        self.format
    }

    fn write(&mut self, _at: u64, data: &[u8]) -> io::Result<()> {
        self.file.write_all(data)?;
        self.data_bytes += data.len() as u64;
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        let data_bytes = u32::try_from(self.data_bytes)
            .map_err(|_| io::Error::other("WAV files hold at most 4 GiB"))?;
        self.file.seek(SeekFrom::Start(0))?;
        write_header(&mut self.file, self.format, data_bytes)?;
        self.file
            .seek(SeekFrom::Start(HEADER_BYTES + self.data_bytes))?;
        self.file.flush()
    }
}
