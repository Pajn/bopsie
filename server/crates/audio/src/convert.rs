//! Converting a track's PCM to the session's format: channels first, then sample rate.

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::format::PcmFormat;

/// Maps interleaved samples from `from` channels to `to`. Mono is copied to every channel,
/// anything to mono is averaged, and otherwise the first channels are kept and missing ones
/// are silent.
pub fn map_channels(samples: &[f32], from: u16, to: u16) -> Vec<f32> {
    let (from, to) = (usize::from(from), usize::from(to));
    if from == to {
        return samples.to_vec();
    }
    let frames = samples.chunks_exact(from);
    let mut out = Vec::with_capacity(frames.len() * to);
    for frame in frames {
        if from == 1 {
            out.extend(std::iter::repeat_n(frame[0], to));
        } else if to == 1 {
            out.push(frame.iter().sum::<f32>() / from as f32);
        } else {
            out.extend((0..to).map(|c| frame.get(c).copied().unwrap_or(0.0)));
        }
    }
    out
}

/// Input frames the resampler takes per call.
const CHUNK: usize = 1024;

/// Changes the sample rate of a stream, producing exactly as many frames as the rates imply
/// (`input × to / from`, rounded): the resampler's delay is trimmed from the start and its
/// tail flushed at the end, so tracks join without a gap or overlap.
pub struct StreamResampler {
    resampler: Fft<f32>,
    channels: usize,
    ratio: f64,
    pending: Vec<f32>,
    out: Vec<f32>,
    delay_left: usize,
    input_frames: u64,
    output_frames: u64,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32, channels: u16) -> Self {
        let resampler = Fft::<f32>::new(
            from as usize,
            to as usize,
            CHUNK,
            usize::from(channels),
            FixedSync::Input,
        )
        .expect("resampler parameters are valid");
        let delay_left = resampler.output_delay();
        let out = vec![0.0; resampler.output_frames_max() * usize::from(channels)];
        Self {
            resampler,
            channels: usize::from(channels),
            ratio: f64::from(to) / f64::from(from),
            pending: Vec::new(),
            out,
            delay_left,
            input_frames: 0,
            output_frames: 0,
        }
    }

    fn process_chunk(&mut self, emit: &mut Vec<f32>) {
        let frames = CHUNK;
        let input = InterleavedSlice::new(
            &self.pending[..frames * self.channels],
            self.channels,
            frames,
        )
        .expect("the chunk holds whole frames");
        let capacity = self.out.len() / self.channels;
        let mut output = InterleavedSlice::new_mut(&mut self.out, self.channels, capacity)
            .expect("the buffer holds whole frames");
        let (read, written) = self
            .resampler
            .process_into_buffer(&input, &mut output, None)
            .expect("buffers are sized for the resampler");
        self.pending.drain(..read * self.channels);
        let skip = self.delay_left.min(written);
        self.delay_left -= skip;
        emit.extend_from_slice(&self.out[skip * self.channels..written * self.channels]);
    }

    /// Resamples `samples`, returning what is ready.
    pub fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        self.input_frames += (samples.len() / self.channels) as u64;
        self.pending.extend_from_slice(samples);
        let mut emit = Vec::new();
        while self.pending.len() >= CHUNK * self.channels {
            self.process_chunk(&mut emit);
        }
        self.output_frames += (emit.len() / self.channels) as u64;
        emit
    }

    /// The rest of the stream, ending exactly where the input does.
    pub fn finish(&mut self) -> Vec<f32> {
        let target = (self.input_frames as f64 * self.ratio).round() as u64;
        let mut emit = Vec::new();
        let chunk = CHUNK * self.channels;
        // Less than a chunk is ever pending here; pad it with silence until the resampler has
        // given up everything the real input implies.
        while self.output_frames + ((emit.len() / self.channels) as u64) < target {
            let silence = chunk - self.pending.len();
            self.pending.extend(std::iter::repeat_n(0.0, silence));
            self.process_chunk(&mut emit);
        }
        let wanted = (target - self.output_frames) as usize * self.channels;
        emit.truncate(wanted);
        self.output_frames = target;
        emit
    }
}

/// Converts one track to the session's format.
pub struct Converter {
    from: PcmFormat,
    to: PcmFormat,
    resampler: Option<StreamResampler>,
}

impl Converter {
    pub fn new(from: PcmFormat, to: PcmFormat) -> Self {
        let resampler = (from.sample_rate != to.sample_rate)
            .then(|| StreamResampler::new(from.sample_rate, to.sample_rate, to.channels));
        Self {
            from,
            to,
            resampler,
        }
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        let mapped = map_channels(samples, self.from.channels, self.to.channels);
        match &mut self.resampler {
            Some(resampler) => resampler.push(&mapped),
            None => mapped,
        }
    }

    pub fn finish(&mut self) -> Vec<f32> {
        match &mut self.resampler {
            Some(resampler) => resampler.finish(),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_mapping() {
        assert_eq!(map_channels(&[0.5, -0.5], 1, 2), [0.5, 0.5, -0.5, -0.5]);
        assert_eq!(map_channels(&[0.2, 0.4, 0.6, 0.8], 2, 1), [0.3, 0.70000005]);
        assert_eq!(
            map_channels(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3, 2),
            [1.0, 2.0, 4.0, 5.0]
        );
        assert_eq!(map_channels(&[1.0, 2.0], 2, 3), [1.0, 2.0, 0.0]);
    }

    fn sine(rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn resampling_yields_exactly_the_implied_length() {
        for (from, to, frames) in [
            (44_100, 48_000, 44_100),
            (48_000, 44_100, 12_345),
            (96_000, 48_000, 999),
        ] {
            let input = sine(from, frames);
            let mut resampler = StreamResampler::new(from, to, 2);
            // Feed in uneven pieces, as a decoder does.
            let mut output = Vec::new();
            for piece in input.chunks(2 * 777) {
                output.extend(resampler.push(piece));
            }
            output.extend(resampler.finish());
            let expected = (frames as f64 * f64::from(to) / f64::from(from)).round() as usize;
            assert_eq!(output.len() / 2, expected, "{from} → {to}");
        }
    }

    #[test]
    fn resampling_keeps_the_signal_in_place() {
        // A click at a known time stays at the same time after conversion.
        let (from, to) = (44_100u32, 48_000u32);
        let mut input = vec![0.0f32; 2 * 44_100];
        input[2 * 22_050] = 1.0;
        input[2 * 22_050 + 1] = 1.0;
        let mut resampler = StreamResampler::new(from, to, 2);
        let mut output = resampler.push(&input);
        output.extend(resampler.finish());
        let peak = output
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .max_by(|a, b| a.1[0].total_cmp(&b.1[0]))
            .unwrap()
            .0;
        assert!((peak as i64 - 24_000).abs() <= 2, "peak at {peak}");
    }

    #[test]
    fn same_rate_conversion_is_untouched() {
        let format = PcmFormat {
            sample_rate: 44_100,
            channels: 2,
        };
        let mut converter = Converter::new(format, format);
        let input = sine(44_100, 100);
        assert_eq!(converter.push(&input), input);
        assert!(converter.finish().is_empty());
    }
}
