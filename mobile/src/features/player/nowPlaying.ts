import type { WithImages } from '@bopsie/client/api/schemas/media';
import {
  type Player,
  type PlayerQueue,
  playerQueue,
  type QueueItem,
} from '@bopsie/client/api/schemas/player';
import { type ItemLink, mediaLinks } from '@/features/library/links';

export type { ItemLink } from '@/features/library/links';

export interface NowPlaying {
  title: string;
  subtitle: string;
  artwork: WithImages;
  /** Seconds, when known. */
  duration: number | null;
}

/** A source playing to the group in place of its queue, such as Spotify Connect. */
export interface SourcePlaying {
  id: string;
  /** Such as `Spotify`. */
  name: string;
  subtitle: string;
}

/** The active group's queue as the listener hears it: its own, or a source's in its place. */
export type HeardQueue = PlayerQueue & { source: SourcePlaying | null };

/**
 * The queue as the listener hears it. While the group's player plays a source
 * rather than its queue, the source's state, position and media stand in for
 * the queue's, so the player shows and controls what is heard; the server
 * sends the queue's transport commands on to the source.
 */
export function asHeard(queue: PlayerQueue, player: Player | undefined): HeardQueue {
  const sourceId = player?.active_source;
  const media = player?.current_media;
  if (!sourceId || sourceId === queue.queue_id || !media) return { ...queue, source: null };
  const name =
    player.source_list.find((source) => source.id === sourceId)?.name ?? 'Another source';
  const title = media.title ?? name;
  return {
    ...queue,
    state: player.playback_state,
    elapsed_time: media.elapsed_time ?? 0,
    elapsed_time_last_updated: media.elapsed_time_last_updated ?? 0,
    playback_speed: 1,
    current_item: {
      queue_id: queue.queue_id,
      queue_item_id: `source:${sourceId}`,
      name: title,
      duration: media.duration ?? null,
      image: media.image_url
        ? {
            type: 'thumb',
            path: media.image_url,
            provider: sourceId,
            remotely_accessible: true,
            proxy_id: null,
          }
        : null,
      media_item: null,
      available: true,
    },
    next_item: null,
    source: {
      id: sourceId,
      name,
      subtitle: [media.artist, media.album].filter(Boolean).join(' · '),
    },
  };
}

/** A queue with nothing in it, for a group whose queue is empty, still loading, or not chosen. */
export function emptyQueue(queueId: string): HeardQueue {
  return { ...playerQueue.parse({ queue_id: queueId }), source: null };
}

/** What the player shows while nothing is loaded. */
export const NOTHING_PLAYING: NowPlaying = {
  title: 'Nothing playing',
  subtitle: 'Play something from your library or a search.',
  artwork: { image: null },
  duration: null,
};

/** Display fields for the queue's current item, or null when nothing is loaded. */
export function nowPlaying(queue: PlayerQueue | HeardQueue): NowPlaying | null {
  if (!queue.current_item) return null;
  const described = describeItem(queue.current_item);
  const source = (queue as Partial<HeardQueue>).source ?? null;
  return source ? { ...described, subtitle: source.subtitle } : described;
}

/** Display fields for any item in a queue. */
export function describeItem(item: QueueItem): NowPlaying {
  const media = item.media_item;
  const artists = media ? media.artists.map((artist) => artist.name).join(', ') : '';
  const album = media?.album?.name;
  return {
    title: media?.name ?? item.name,
    subtitle: [artists, album].filter(Boolean).join(' · '),
    artwork: item.image ? { image: item.image } : (media ?? { image: null }),
    duration: item.duration ?? null,
  };
}

/** The album and artists of a track, for the player's menu. Radio and the like have none. */
export function itemLinks(item: QueueItem): ItemLink[] {
  return item.media_item ? mediaLinks(item.media_item) : [];
}
