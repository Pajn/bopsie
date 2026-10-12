import * as z from 'zod/mini';
import { booleanOr, numberOr, stringOr } from './common';
import { image, track } from './media';

export const playbackState = z.pipe(
  z.nullish(z.string()),
  z.transform((value): 'idle' | 'paused' | 'playing' =>
    value === 'playing' || value === 'paused' ? value : 'idle',
  ),
);
export type PlaybackState = z.output<typeof playbackState>;

export const player = z.looseObject({
  player_id: z.string(),
  provider: z.string(),
  type: stringOr('player'),
  name: z.string(),
  available: booleanOr(false),
  playback_state: playbackState,
  volume_level: z.nullish(z.number()),
  volume_muted: z.nullish(z.boolean()),
});
export type Player = z.infer<typeof player>;

/**
 * `QueueItem`. `media_item` is read as a track; the track schema leaves its
 * track-only fields optional, so radio and other items read too.
 */
export const queueItem = z.looseObject({
  queue_id: z.string(),
  queue_item_id: z.string(),
  name: z.string(),
  duration: z.nullish(z.number()),
  image: z.nullish(image),
  media_item: z.nullish(track),
  available: booleanOr(true),
});
export type QueueItem = z.infer<typeof queueItem>;

export const playerQueue = z.looseObject({
  queue_id: z.string(),
  active: booleanOr(false),
  display_name: stringOr(''),
  available: booleanOr(false),
  items: numberOr(0),
  shuffle_enabled: booleanOr(false),
  repeat_mode: stringOr('off'),
  current_index: z.nullish(z.number()),
  /** Seconds into the current item at `elapsed_time_last_updated`. */
  elapsed_time: numberOr(0),
  /** Unix seconds. */
  elapsed_time_last_updated: numberOr(0),
  state: playbackState,
  current_item: z.nullish(queueItem),
  next_item: z.nullish(queueItem),
});
export type PlayerQueue = z.infer<typeof playerQueue>;
