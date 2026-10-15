import type { RepeatMode } from '@bopsie/client/api/schemas/player';

/** Repeat cycles from off, to the whole queue, to the current track, and back. */
export function nextRepeatMode(mode: RepeatMode): RepeatMode {
  return mode === 'off' ? 'all' : mode === 'all' ? 'one' : 'off';
}
