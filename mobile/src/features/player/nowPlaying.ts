import type { WithImages } from '@/api/schemas/media';
import type { PlayerQueue, QueueItem } from '@/api/schemas/player';

export interface NowPlaying {
  title: string;
  subtitle: string;
  artwork: WithImages;
  /** Seconds, when known. */
  duration: number | null;
}

/** Display fields for the queue's current item, or null when nothing is loaded. */
export function nowPlaying(queue: PlayerQueue): NowPlaying | null {
  return queue.current_item ? describeItem(queue.current_item) : null;
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

/** A library page an item leads to. */
export interface ItemLink {
  kind: 'album' | 'artist';
  id: string;
  provider: string;
  name: string;
}

/** The album and artists of a track, for the player's menu. Radio and the like have none. */
export function itemLinks(item: QueueItem): ItemLink[] {
  const media = item.media_item;
  if (!media) return [];
  const link = (
    kind: ItemLink['kind'],
    ref: { item_id: string; provider: string; name: string },
  ) => ({
    kind,
    id: ref.item_id,
    provider: ref.provider,
    name: ref.name,
  });
  return [
    ...(media.album ? [link('album', media.album)] : []),
    ...media.artists.map((artist) => link('artist', artist)),
  ];
}
