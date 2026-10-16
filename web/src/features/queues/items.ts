import { type ImageSize, imageUrl } from '@bopsie/client/api/images';
import type { QueueItem } from '@bopsie/client/api/schemas/player';

/** A queue item's title and, below it, its artists and album. */
export function describeItem(item: QueueItem): { title: string; subtitle: string } {
  const media = item.media_item;
  const artists = media?.artists.map((artist) => artist.name).join(', ');
  return {
    title: media?.name ?? item.name,
    subtitle: [artists, media?.album?.name].filter(Boolean).join(' · '),
  };
}

/** A queue item's artwork: its own, as a radio's is, else its track's. */
export function itemArtwork(serverUrl: string, item: QueueItem, size: ImageSize): string | null {
  return imageUrl(
    serverUrl,
    item.image ? { image: item.image } : (item.media_item ?? { image: null }),
    size,
  );
}
