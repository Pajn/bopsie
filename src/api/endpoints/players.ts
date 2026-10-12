import * as z from 'zod/mini';
import type { MaConnection } from '../client';
import { player, playerQueue } from '../schemas/player';

/** `QueueOption`: where new media goes in the queue. */
export type QueueOption = 'play' | 'replace' | 'next' | 'replace_next' | 'add';

export type QueueCommand = 'play' | 'pause' | 'play_pause' | 'stop' | 'next' | 'previous';

export const getPlayer = (connection: MaConnection, playerId: string) =>
  connection.command(
    'players/get',
    { player_id: playerId, raise_unavailable: false },
    { schema: z.nullable(player) },
  );

/** A player's queue; its id is the player's id. Null when the server has none. */
export const getQueue = (connection: MaConnection, queueId: string) =>
  connection.command(
    'player_queues/get',
    { queue_id: queueId },
    { schema: z.nullable(playerQueue) },
  );

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
