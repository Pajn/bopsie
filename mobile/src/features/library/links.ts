import type { ItemMapping } from '@bopsie/client/api/schemas/media';

/** A library page an item leads to. */
export interface ItemLink {
  kind: 'album' | 'artist';
  id: string;
  provider: string;
  name: string;
}

/**
 * The album and artists an item leads to, each once: a track's album and
 * artists, or an album's artists.
 */
export function mediaLinks(media: {
  album?: ItemMapping | null | undefined;
  artists?: ItemMapping[] | undefined;
}): ItemLink[] {
  const link = (kind: ItemLink['kind'], ref: ItemMapping): ItemLink => ({
    kind,
    id: ref.item_id,
    provider: ref.provider,
    name: ref.name,
  });
  const links = [
    ...(media.album ? [link('album', media.album)] : []),
    ...(media.artists ?? []).map((artist) => link('artist', artist)),
  ];
  return links.filter(
    (candidate, index) =>
      links.findIndex(
        (other) =>
          other.kind === candidate.kind &&
          other.provider === candidate.provider &&
          other.id === candidate.id,
      ) === index,
  );
}

/** Where a link goes, for the router. */
export const linkHref = (link: ItemLink) => ({
  pathname: link.kind === 'album' ? ('/album/[id]' as const) : ('/artist/[id]' as const),
  params: { id: link.id, provider: link.provider, name: link.name },
});
