import type { ItemMapping } from '@bopsie/client/api/schemas/media';

/** The library page of an item a track or album names, such as its artist or album. */
export function itemHref(item: Pick<ItemMapping, 'media_type' | 'provider' | 'item_id'>): string {
  return `/library/${item.media_type}/${encodeURIComponent(item.provider)}/${encodeURIComponent(item.item_id)}`;
}
