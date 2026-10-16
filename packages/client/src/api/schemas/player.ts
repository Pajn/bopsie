import * as z from 'zod/mini';
import { arrayOr, booleanOr, numberOr, stringOr } from './common';
import { image, track } from './media';

export const playbackState = z.pipe(
  z.nullish(z.string()),
  z.transform((value): 'idle' | 'paused' | 'playing' =>
    value === 'playing' || value === 'paused' ? value : 'idle',
  ),
);
export type PlaybackState = z.output<typeof playbackState>;

/** `RepeatMode`, with anything unknown read as off. */
export const repeatMode = z.pipe(
  z.nullish(z.string()),
  z.transform((value): 'off' | 'one' | 'all' =>
    value === 'one' || value === 'all' ? value : 'off',
  ),
);
export type RepeatMode = z.output<typeof repeatMode>;

/** `PlayerMedia`: what a player has loaded, whatever its source. */
export const playerMedia = z.looseObject({
  title: z.nullish(z.string()),
  artist: z.nullish(z.string()),
  album: z.nullish(z.string()),
  /** A public URL, for what a source such as Spotify Connect plays. */
  image_url: z.nullish(z.string()),
  /** Seconds. */
  duration: z.nullish(z.number()),
  /** Seconds into the media at `elapsed_time_last_updated`. */
  elapsed_time: z.nullish(z.number()),
  /** Unix seconds. */
  elapsed_time_last_updated: z.nullish(z.number()),
});
export type PlayerMedia = z.infer<typeof playerMedia>;

/** `PlayerSource`: something other than the queue that can play to the player, such as Spotify Connect. */
export const playerSource = z.looseObject({
  id: z.string(),
  name: z.string(),
});

export const player = z.looseObject({
  player_id: z.string(),
  provider: z.string(),
  /** `player`, `stereo_pair` or `group` play audio; `display`, `visualizer`, `light` and `source` do not. */
  type: stringOr('player'),
  name: z.string(),
  available: booleanOr(false),
  enabled: booleanOr(true),
  hide_in_ui: booleanOr(false),
  /** Another client's own player, not to be offered as a target. */
  private: booleanOr(false),
  supported_features: arrayOr(z.string()),
  playback_state: playbackState,
  volume_level: z.nullish(z.number()),
  volume_muted: z.nullish(z.boolean()),
  /** The loudest member's volume, for a group player or a sync leader. */
  group_volume: z.nullish(z.number()),
  /**
   * A group player's members, or a sync leader's followers with the leader
   * itself first. Empty for a player that is not grouped.
   */
  group_members: arrayOr(z.string()),
  static_group_members: arrayOr(z.string()),
  /** Player ids, or whole provider instance ids, this player can be grouped with. */
  can_group_with: arrayOr(z.string()),
  /** The sync leader this player follows. */
  synced_to: z.nullish(z.string()),
  /** The group player this player is playing as part of. */
  active_group: z.nullish(z.string()),
  current_media: z.nullish(playerMedia),
  /**
   * What the player plays: its queue, by the queue's id, or a source such as
   * Spotify Connect, by the source's id.
   */
  active_source: z.nullish(z.string()),
  source_list: arrayOr(playerSource),
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
  repeat_mode: repeatMode,
  current_index: z.nullish(z.number()),
  /**
   * The item the player has been handed, which is the current one or, near
   * the end of it, the next. The server refuses to move or remove anything up
   * to the later of this and `current_index`.
   */
  index_in_buffer: z.nullish(z.number()),
  /** Seconds into the current item at `elapsed_time_last_updated`. */
  elapsed_time: numberOr(0),
  /** Unix seconds. */
  elapsed_time_last_updated: numberOr(0),
  state: playbackState,
  /**
   * Bopsie's own: meant to be playing, but nothing heard yet, so `elapsed_time` stands still.
   * Music Assistant's states have no such state.
   */
  buffering: booleanOr(false),
  current_item: z.nullish(queueItem),
  next_item: z.nullish(queueItem),
});
export type PlayerQueue = z.infer<typeof playerQueue>;
