//! Packing `f32` samples into the integer PCM outputs take. The scale is the one decoders use
//! in the other direction (`i16 / 32768`), so integer sources pass through unchanged.

/// Appends `samples` as little-endian signed integers of `bits` bits (16, 24 or 32).
pub fn pack(samples: &[f32], bits: u16, out: &mut Vec<u8>) {
    out.reserve(samples.len() * usize::from(bits / 8));
    match bits {
        16 => {
            for &s in samples {
                let v = (f64::from(s) * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16;
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        24 => {
            for &s in samples {
                let v = (f64::from(s) * 8_388_608.0)
                    .round()
                    .clamp(-8_388_608.0, 8_388_607.0) as i32;
                out.extend_from_slice(&v.to_le_bytes()[..3]);
            }
        }
        32 => {
            for &s in samples {
                let v = (f64::from(s) * 2_147_483_648.0)
                    .round()
                    .clamp(-2_147_483_648.0, 2_147_483_647.0) as i32;
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        other => panic!("unsupported bit depth {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_bit_round_trips_and_clips() {
        let ints: [i16; 5] = [i16::MIN, -1, 0, 1, i16::MAX];
        let floats: Vec<f32> = ints.iter().map(|&i| f32::from(i) / 32_768.0).collect();
        let mut out = Vec::new();
        pack(&floats, 16, &mut out);
        let back: Vec<i16> = out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&b| i16::from_le_bytes(b))
            .collect();
        assert_eq!(back, ints);

        let mut clipped = Vec::new();
        pack(&[1.5, -1.5], 16, &mut clipped);
        assert_eq!(clipped, [0xff, 0x7f, 0x00, 0x80]);
    }

    #[test]
    fn twenty_four_bit_is_three_bytes() {
        let mut out = Vec::new();
        pack(&[0.5, -1.0], 24, &mut out);
        assert_eq!(out, [0x00, 0x00, 0x40, 0x00, 0x00, 0x80]);
    }
}
