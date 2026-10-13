//! A session's audio to one AirPlay speaker, through its `cliairplay` process.

use std::io;

use bopsie_audio::OutputFormat;
use bopsie_audio::sink::Sink;
use bopsie_audio::timeline::Timeline;
use bopsie_core::clock;

use crate::supervisor::Supervisor;

pub struct AirplaySink {
    supervisor: Supervisor,
    format: OutputFormat,
    timeline: Timeline,
    /// A START was sent for the stream being written.
    started: bool,
    /// The speaker may be playing audio this sink sent.
    live: bool,
}

impl AirplaySink {
    pub fn new(supervisor: Supervisor, format: OutputFormat, timeline: Timeline) -> Self {
        Self {
            supervisor,
            format,
            timeline,
            started: false,
            live: false,
        }
    }

    /// Starts a stream with `data`, the audio for timeline frame `at` on: connected, cleared of
    /// any earlier stream, and commanded to play the first sample when the timeline says.
    fn start(&mut self, at: u64, data: &[u8]) -> io::Result<()> {
        self.supervisor.ensure_connected()?;
        if self.supervisor.is_streaming() {
            self.supervisor.flush()?;
        }
        self.supervisor.write_audio(data)?;
        let unix_ms = clock::unix_ms(self.timeline.instant_of(at));
        self.supervisor.start(unix_ms)?;
        self.started = true;
        self.live = true;
        Ok(())
    }
}

impl Sink for AirplaySink {
    fn format(&self) -> OutputFormat {
        self.format
    }

    fn write(&mut self, at: u64, data: &[u8]) -> io::Result<()> {
        if !self.started {
            return self.start(at, data);
        }
        self.supervisor.write_audio(data)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.started = false;
        if std::mem::take(&mut self.live) {
            self.supervisor.flush()?;
        }
        Ok(())
    }

    fn stop(&mut self) -> io::Result<()> {
        self.started = false;
        if std::mem::take(&mut self.live) {
            self.supervisor.flush()?;
            self.supervisor.command(&["ACTION=STANDBY".into()])?;
        }
        Ok(())
    }

    fn drain(&mut self) -> io::Result<()> {
        // The speaker plays out what it holds, which a stop still cuts short. The next stream
        // flushes before it starts.
        self.started = false;
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        // The process outlives the session, so the next one starts warm.
        self.drain()
    }
}
