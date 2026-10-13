/// Interleaved `f32` PCM: what decoders produce and the engine works in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// What an output consumes: interleaved little-endian signed integer PCM at the session's
/// rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputFormat {
    pub sample_rate: u32,
    pub channels: u16,
    /// 16, 24 or 32.
    pub bits: u16,
}

impl OutputFormat {
    pub fn bytes_per_frame(&self) -> usize {
        usize::from(self.channels) * usize::from(self.bits / 8)
    }
}
