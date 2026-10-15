import type {
  Album,
  Artist,
  Playlist,
  SearchResults,
  Track,
} from '@bopsie/client/api/schemas/media';

export type TopResult =
  | { kind: 'artist'; item: Artist }
  | { kind: 'album'; item: Album }
  | { kind: 'track'; item: Track }
  | { kind: 'playlist'; item: Playlist };

const normal = (text: string) => text.trim().toLocaleLowerCase();

/**
 * The result a search most likely meant: one named exactly what was searched for, an artist
 * before an album, a track or a playlist, and the library's own before a catalog's; failing
 * that, the first artist, else the first album. Null for no results.
 */
export function topResult(results: SearchResults, query: string): TopResult | null {
  const all: TopResult[] = [
    ...results.artists.map((item) => ({ kind: 'artist' as const, item })),
    ...results.albums.map((item) => ({ kind: 'album' as const, item })),
    ...results.tracks.map((item) => ({ kind: 'track' as const, item })),
    ...results.playlists.map((item) => ({ kind: 'playlist' as const, item })),
  ];
  const exact = all.filter((result) => normal(result.item.name) === normal(query));
  const library = (result: TopResult) => result.item.provider === 'library';
  return (
    exact.find(library) ??
    exact[0] ??
    all.find((result) => result.kind === 'artist') ??
    all.find((result) => result.kind === 'album') ??
    all[0] ??
    null
  );
}

/** At most this many searches are remembered. */
export const RECENT_LIMIT = 10;

/** The recent searches with `query` first, once, and the oldest dropped past the limit. */
export function remember(recent: string[], query: string): string[] {
  const text = query.trim();
  if (!text) return recent;
  return [text, ...recent.filter((past) => normal(past) !== normal(text))].slice(0, RECENT_LIMIT);
}
