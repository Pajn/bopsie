import type { Calibration } from '@bopsie/client/api/schemas/admin';
import { chirpSamples, hearChirps, speakerResults } from './detect';
import { correlationEnvelope, fft } from './fft';

const CHIRP = { from_hz: 500, to_hz: 8000, duration_ms: 150, fade_ms: 10, amplitude: 0.3 };
const RATE = 48_000;

/** A small, repeatable noise. */
function noise(seed: number) {
  let state = seed;
  return () => {
    state = (state * 1_103_515_245 + 12_345) % 2_147_483_648;
    return state / 2_147_483_648 - 0.5;
  };
}

describe('fft', () => {
  it('transforms and transforms back', () => {
    const re = Float64Array.from([1, 2, 3, 4, 0, -1, 0.5, 2]);
    const im = new Float64Array(8);
    const original = re.slice();
    fft(re, im);
    expect(re[0]).toBeCloseTo(11.5);
    fft(re, im, true);
    original.forEach((value, i) => {
      expect(re[i]).toBeCloseTo(value);
      expect(im[i]).toBeCloseTo(0);
    });
  });

  it('finds where a pattern lines up, as a smooth peak', () => {
    const pattern = chirpSamples(CHIRP, RATE);
    const signal = new Float64Array(20_000);
    signal.set(pattern, 4_321);
    const envelope = correlationEnvelope(signal, pattern);
    let best = 0;
    envelope.forEach((value, i) => {
      if (value > (envelope[best] as number)) best = i;
    });
    expect(best).toBe(4_321);
    // No ripple: each side falls away from the peak.
    expect(envelope[4_320]).toBeLessThan(envelope[4_321] as number);
    expect(envelope[4_319]).toBeLessThan(envelope[4_320] as number);
  });
});

describe('chirpSamples', () => {
  it("is the server's sweep, sample for sample", () => {
    const samples = chirpSamples(CHIRP, RATE);
    expect(samples.length).toBe(7_200);
    // As computed apart from either implementation.
    expect(samples[100]).toBeCloseTo(0.011768940427152442, 9);
    expect(samples[1000]).toBeCloseTo(-0.2888986608905104, 9);
    expect(samples[5000]).toBeCloseTo(0.07330188107623128, 9);
    expect(samples[7000]).toBeCloseTo(-0.0985044810402668, 9);
  });
});

/** A recording of `speakers` chirping in turn for three rounds, a second apart, each arriving
 * its delay late, quieter, with noise and an echo. */
function scenario(delaysMs: Record<string, number | null>, slotUs = 1_000_000) {
  const ids = Object.keys(delaysMs);
  const firstSampleServerUs = 10_000_000;
  const start = firstSampleServerUs + 1_000_000;
  const chirps = [0, 1, 2].flatMap((round) =>
    ids.map((id, index) => ({
      player_id: id,
      name: id,
      endpoint_id: id,
      at_us: start + (round * ids.length + index) * slotUs,
    })),
  );
  const calibration: Calibration = {
    chirp: CHIRP,
    chirps,
    ends_at_us: start + 3 * ids.length * slotUs,
    skipped: [],
  };
  const length = Math.round(((calibration.ends_at_us - firstSampleServerUs) / 1e6 + 0.5) * RATE);
  const signal = new Float64Array(length);
  const random = noise(7);
  for (let i = 0; i < length; i++) signal[i] = random() * 0.02;
  const pattern = chirpSamples(CHIRP, RATE);
  for (const chirp of chirps) {
    const delay = delaysMs[chirp.endpoint_id];
    if (delay === null || delay === undefined) continue;
    const at = ((chirp.at_us - firstSampleServerUs) / 1e6 + delay / 1000) * RATE;
    const whole = Math.floor(at);
    const fraction = at - whole;
    // The sound itself, between samples, and a stronger echo 9 ms behind it.
    for (const [lateMs, gain] of [
      [0, 0.4],
      [9, 0.6],
    ] as const) {
      const offset = whole + Math.round((lateMs / 1000) * RATE);
      for (let i = 0; i < pattern.length; i++) {
        const here = (pattern[i] as number) * (1 - fraction);
        const next = i > 0 ? (pattern[i - 1] as number) * fraction : 0;
        signal[offset + i] = (signal[offset + i] as number) + gain * (here + next);
      }
    }
  }
  const samples = Int16Array.from(signal, (s) => Math.round(s * 32_767));
  return { recording: { sampleRate: RATE, samples, firstSampleServerUs }, calibration };
}

describe('hearChirps', () => {
  it('hears when each chirp arrived, the sound rather than its echo', () => {
    const { recording, calibration } = scenario({ kitchen: 42.3, wiim: 17.85 });
    const heard = hearChirps(recording, calibration);
    expect(heard).toHaveLength(6);
    for (const chirp of heard) {
      const expected = chirp.endpointId === 'kitchen' ? 42.3 : 17.85;
      expect(chirp.delayMs).not.toBeNull();
      expect(Math.abs((chirp.delayMs as number) - expected)).toBeLessThan(0.1);
    }
    const results = speakerResults(heard);
    expect(results.map((r) => [r.endpointId, r.problem, r.heard])).toEqual([
      ['kitchen', null, 3],
      ['wiim', null, 3],
    ]);
  });

  it('hears a speaker that plays late, up to where the next chirp is looked for', () => {
    const { recording, calibration } = scenario({ philips: 480, wiim: 900 }, 1_500_000);
    const results = speakerResults(hearChirps(recording, calibration));
    expect(results.map((r) => [r.endpointId, r.problem])).toEqual([
      ['philips', null],
      ['wiim', null],
    ]);
    expect(Math.abs((results[1]?.delayMs as number) - 900)).toBeLessThan(0.1);
  });

  it('says which speaker it did not hear', () => {
    const { recording, calibration } = scenario({ kitchen: 30, patio: null });
    const results = speakerResults(hearChirps(recording, calibration));
    const patio = results.find((r) => r.endpointId === 'patio');
    expect(patio?.problem).toBe('not-heard');
    expect(patio?.delayMs).toBeNull();
    expect(results.find((r) => r.endpointId === 'kitchen')?.problem).toBeNull();
  });

  it("needs the phone's clock in step with the server", () => {
    const { recording, calibration } = scenario({ kitchen: 30, wiim: 30 });
    expect(() => hearChirps({ ...recording, firstSampleServerUs: null }, calibration)).toThrow();
  });
});

describe('speakerResults', () => {
  it('distrusts a speaker whose rounds disagree', () => {
    const heard = [12, 30, 13].map((delayMs) => ({
      endpointId: 'tv',
      name: 'TV',
      delayMs,
      clarity: 20,
    }));
    expect(speakerResults(heard)[0]?.problem).toBe('unsteady');
    const steady = speakerResults(heard.map((h, i) => ({ ...h, delayMs: 12 + i * 0.5 })))[0];
    expect(steady?.delayMs).toBe(12.5);
  });
});
