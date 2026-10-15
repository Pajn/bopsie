import type { Calibration, Chirp } from '@bopsie/client/api/schemas/admin';
import type { MicRecording } from '@/player/Player';
import { correlationEnvelope } from './fft';

/** How early before it was due a chirp is looked for. */
const SEARCH_BEFORE_MS = 300;
/** How late after it was due. Less than a slot, so that the next speaker's chirp is not found. */
const SEARCH_AFTER_MS = 500;
/** How much a peak must stand out from the correlation around it to count as heard. */
const MIN_CLARITY = 8;
/** An earlier peak this close to the strongest is the sound itself, and the strongest an echo. */
const DIRECT_PATH_MS = 25;
const DIRECT_PATH_SHARE = 0.5;
/** How far apart a speaker's rounds may be and still be trusted. */
const MAX_SPREAD_MS = 5;

/** The sweep a speaker plays, at `rate`: the server's, sample for sample. */
export function chirpSamples(chirp: Chirp, rate: number): Float64Array {
  const length = Math.round((chirp.duration_ms / 1000) * rate);
  const duration = length / rate;
  const fade = Math.round((chirp.fade_ms / 1000) * rate);
  const sweep = duration / Math.log(chirp.to_hz / chirp.from_hz);
  const out = new Float64Array(length);
  for (let i = 0; i < length; i++) {
    const t = i / rate;
    const phase = 2 * Math.PI * chirp.from_hz * sweep * (Math.exp(t / sweep) - 1);
    const edge = Math.min(i, length - 1 - i);
    const envelope = edge < fade ? 0.5 - 0.5 * Math.cos((Math.PI * edge) / fade) : 1;
    out[i] = chirp.amplitude * envelope * Math.sin(phase);
  }
  return out;
}

/** One chirp, as heard. */
export interface Heard {
  endpointId: string;
  name: string;
  /** How long after it was due it arrived, in milliseconds; null where it was not heard. */
  delayMs: number | null;
  /** How far its peak stood out. */
  clarity: number;
}

/** When each of `calibration`'s chirps arrived in `recording`. */
export function hearChirps(recording: MicRecording, calibration: Calibration): Heard[] {
  const { sampleRate: rate, samples, firstSampleServerUs: first } = recording;
  if (first === null) {
    throw new Error("The phone's clock is not in step with the server");
  }
  const pattern = chirpSamples(calibration.chirp, rate);
  const before = Math.round((SEARCH_BEFORE_MS / 1000) * rate);
  const after = Math.round((SEARCH_AFTER_MS / 1000) * rate);
  return calibration.chirps.map((chirp) => {
    const due = Math.round(((chirp.at_us - first) / 1_000_000) * rate);
    const start = due - before;
    const end = due + after + pattern.length;
    const missing = { endpointId: chirp.endpoint_id, name: chirp.name, delayMs: null, clarity: 0 };
    if (start < 0 || end > samples.length) return missing;
    const window = new Float64Array(end - start);
    for (let i = 0; i < window.length; i++) window[i] = (samples[start + i] as number) / 32_768;
    const found = findPeak(correlationEnvelope(window, pattern), rate);
    if (found === null || found.clarity < MIN_CLARITY) {
      return { ...missing, clarity: found?.clarity ?? 0 };
    }
    return {
      ...missing,
      delayMs: ((found.lag - before) / rate) * 1000,
      clarity: found.clarity,
    };
  });
}

/** The sound's own peak in a correlation's envelope: the strongest, or an earlier one nearly
 * as strong, which the strongest echoes. Placed between samples by the parabola through its
 * neighbours. */
function findPeak(magnitude: Float64Array, rate: number): { lag: number; clarity: number } | null {
  if (magnitude.length < 3) return null;
  let strongest = 0;
  for (let i = 1; i < magnitude.length; i++) {
    if ((magnitude[i] as number) > (magnitude[strongest] as number)) strongest = i;
  }
  const top = magnitude[strongest] as number;
  let peak = strongest;
  const earliest = Math.max(1, strongest - Math.round((DIRECT_PATH_MS / 1000) * rate));
  for (let i = earliest; i < strongest; i++) {
    const m = magnitude[i] as number;
    if (
      m >= top * DIRECT_PATH_SHARE &&
      m >= (magnitude[i - 1] as number) &&
      m >= (magnitude[i + 1] as number)
    ) {
      peak = i;
      break;
    }
  }
  const sorted = Float64Array.from(magnitude).sort();
  const typical = (sorted[Math.floor(sorted.length / 2)] as number) || Number.MIN_VALUE;
  let lag = peak;
  if (peak > 0 && peak < magnitude.length - 1) {
    const a = magnitude[peak - 1] as number;
    const b = magnitude[peak] as number;
    const c = magnitude[peak + 1] as number;
    const curve = a - 2 * b + c;
    if (curve < 0) lag += (0.5 * (a - c)) / curve;
  }
  return { lag, clarity: (magnitude[peak] as number) / typical };
}

/** One speaker's result: its rounds' median delay, if they agree. */
export interface SpeakerResult {
  endpointId: string;
  name: string;
  delayMs: number | null;
  /** How many of its chirps were heard. */
  heard: number;
  rounds: number;
  /** How far apart the rounds it was heard in are, in milliseconds. */
  spreadMs: number;
  /** Why it has no delay, if it has none. */
  problem: 'not-heard' | 'unsteady' | null;
}

/** Each speaker's delay, from its rounds: the median, of at least two that agree. */
export function speakerResults(heard: Heard[]): SpeakerResult[] {
  const bySpeaker = new Map<string, Heard[]>();
  for (const chirp of heard) {
    bySpeaker.set(chirp.endpointId, [...(bySpeaker.get(chirp.endpointId) ?? []), chirp]);
  }
  return [...bySpeaker.values()].map((chirps) => {
    const delays = chirps
      .map((c) => c.delayMs)
      .filter((d): d is number => d !== null)
      .sort((a, b) => a - b);
    const first = chirps[0] as Heard;
    const spreadMs = delays.length > 0 ? (delays.at(-1) as number) - (delays[0] as number) : 0;
    const problem =
      delays.length < Math.min(2, chirps.length)
        ? 'not-heard'
        : spreadMs > MAX_SPREAD_MS
          ? 'unsteady'
          : null;
    return {
      endpointId: first.endpointId,
      name: first.name,
      delayMs: problem === null ? (delays[Math.floor(delays.length / 2)] as number) : null,
      heard: delays.length,
      rounds: chirps.length,
      spreadMs,
      problem,
    };
  });
}
