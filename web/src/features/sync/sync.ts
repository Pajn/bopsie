import type { CalibratedSpeaker } from '@bopsie/client/api/schemas/admin';

/** A delay as people read it: `+12 ms`, `0 ms`, `−5 ms`. */
export function ms(value: number): string {
  if (value > 0) return `+${value} ms`;
  if (value < 0) return `−${-value} ms`;
  return '0 ms';
}

/** What a typed delay means, in whole milliseconds; null for something that is not one. */
export function parseMs(text: string): number | null {
  const cleaned = text
    .trim()
    .replace('−', '-')
    .replace(/\s*ms$/i, '')
    .replace(/^\+/, '');
  if (!/^-?\d+(\.\d+)?$/.test(cleaned)) return null;
  return Math.round(Number(cleaned));
}

/** What the last calibration did to a speaker, in a line. */
export function describeCalibrated(speaker: CalibratedSpeaker): string {
  const heard = `heard ${Math.round(speaker.heard_ms)} ms late`;
  const what = speaker.shared_airplay ? 'AirPlay offset' : 'waits';
  const change =
    speaker.previous_ms === speaker.sync_adjust_ms
      ? `${what} ${speaker.sync_adjust_ms} ms, as before`
      : `${what} ${speaker.sync_adjust_ms} ms (was ${speaker.previous_ms} ms)`;
  return `${heard}; ${change}`;
}
