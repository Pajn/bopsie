import * as z from 'zod/mini';
import type { MaConnection } from '../client';
import { player, playerQueue, type QueueItem, queueItem } from '../schemas/player';

/** `QueueOption`: where new media goes in the queue. */
export type QueueOption = 'play' | 'replace' | 'next' | 'replace_next' | 'add';

export type QueueCommand = 'play' | 'pause' | 'play_pause' | 'stop' | 'next' | 'previous' | 'clear';

export const getPlayer = (connection: MaConnection, playerId: string) =>
  connection.command(
    'players/get',
    { player_id: playerId, raise_unavailable: false },
    { schema: z.nullable(player) },
  );

/** Every player the server knows, including unavailable ones, so they can be shown as such. */
export const listPlayers = (connection: MaConnection) =>
  connection.command(
    'players/all',
    { return_unavailable: true, return_disabled: false },
    { schema: z.array(player) },
  );

/** Adds `playerId` to the group led by `leaderId`. The player drops whatever it was playing. */
export const joinGroup = (connection: MaConnection, playerId: string, leaderId: string) =>
  connection.command<void>('players/cmd/group', { player_id: playerId, target_player: leaderId });

/**
 * Takes `playerId` out of its group and stops it. Removing a sync leader
 * hands the lead, and the music, to a remaining member.
 */
export const leaveGroup = (connection: MaConnection, playerId: string) =>
  connection.command<void>('players/cmd/ungroup', { player_id: playerId });

/** One player's volume, 0–100. */
export const setVolume = (connection: MaConnection, playerId: string, level: number) =>
  connection.command<void>('players/cmd/volume_set', {
    player_id: playerId,
    volume_level: Math.round(level),
  });

/**
 * A group's volume, 0–100. The server moves every member by the same
 * proportion; for a player that is not grouped it sets that player's volume.
 */
export const setGroupVolume = (connection: MaConnection, playerId: string, level: number) =>
  connection.command<void>('players/cmd/group_volume', {
    player_id: playerId,
    volume_level: Math.round(level),
  });

/**
 * Moves a queue's items, position, shuffle and repeat to another player's
 * queue and stops the source. A target that is grouped leaves its group
 * first. Playback resumes on the target if the source was playing.
 */
export const transferQueue = (
  connection: MaConnection,
  sourceQueueId: string,
  targetQueueId: string,
) =>
  connection.command<void>('player_queues/transfer', {
    source_queue_id: sourceQueueId,
    target_queue_id: targetQueueId,
  });

/** A player's queue; its id is the player's id. Null when the server has none. */
export const getQueue = (connection: MaConnection, queueId: string) =>
  connection.command(
    'player_queues/get',
    { queue_id: queueId },
    { schema: z.nullable(playerQueue) },
  );

const ITEMS_PAGE = 500;

/** Every item in a queue, in play order, a page at a time. */
export async function getQueueItems(connection: MaConnection, queueId: string) {
  const items: QueueItem[] = [];
  for (;;) {
    const page = await connection.command(
      'player_queues/items',
      { queue_id: queueId, limit: ITEMS_PAGE, offset: items.length },
      { schema: z.array(queueItem) },
    );
    items.push(...page);
    if (page.length < ITEMS_PAGE) return items;
  }
}

/** Plays the queue from one of its items. */
export const playQueueItem = (connection: MaConnection, queueId: string, queueItemId: string) =>
  connection.command<void>('player_queues/play_index', { queue_id: queueId, index: queueItemId });

/**
 * Moves an item `shift` places later in the queue, or earlier when negative.
 * The server refuses items the player already holds and ignores a move that
 * would land among them.
 */
export const moveQueueItem = (
  connection: MaConnection,
  queueId: string,
  queueItemId: string,
  shift: number,
) =>
  connection.command<void>('player_queues/move_item', {
    queue_id: queueId,
    queue_item_id: queueItemId,
    pos_shift: shift,
  });

/** Removes an item. The server ignores items the player already holds. */
export const removeQueueItem = (connection: MaConnection, queueId: string, queueItemId: string) =>
  connection.command<void>('player_queues/delete_item', {
    queue_id: queueId,
    item_id_or_index: queueItemId,
  });

/**
 * Plays media on a queue. `media` is one or more item URIs, such as
 * `library://album/1`. `startItem` names a track inside an album or playlist
 * to start from. Playback starts asynchronously; queue events follow.
 */
export function playMedia(
  connection: MaConnection,
  queueId: string,
  media: string | string[],
  options: { option?: QueueOption; startItem?: string } = {},
): Promise<void> {
  return connection.command('player_queues/play_media', {
    queue_id: queueId,
    media,
    option: options.option ?? 'replace',
    ...(options.startItem ? { start_item: options.startItem } : {}),
  });
}

export const queueCommand = (connection: MaConnection, queueId: string, command: QueueCommand) =>
  connection.command<void>(`player_queues/${command}`, { queue_id: queueId });

/** Absolute position in seconds. */
export const seek = (connection: MaConnection, queueId: string, seconds: number) =>
  connection.command<void>('player_queues/seek', {
    queue_id: queueId,
    position: Math.max(0, Math.round(seconds)),
  });
