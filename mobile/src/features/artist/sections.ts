import type { Album, Audiobook, Track } from '@bopsie/client/api/schemas/media';

/** Tracks shown before "Show all". */
export const TRACKS_SHOWN = 5;

/** A row of an artist's page: a heading, a track, an album, or the button showing every track. */
export type ArtistRow =
  | { type: 'heading'; key: string; title: string }
  | { type: 'track'; key: string; track: Track }
  | { type: 'album'; key: string; album: Album }
  | { type: 'book'; key: string; book: Audiobook }
  | { type: 'more'; key: string; hidden: number };

const GROUPS = ['Albums', 'Singles and EPs', 'Compilations'] as const;

/**
 * Where an album goes on an artist's page. The library does not know an album's type, so
 * its albums are all albums; catalogs such as Spotify say.
 */
function groupOf(album: Album): (typeof GROUPS)[number] {
  switch (album.album_type) {
    case 'single':
    case 'ep':
      return 'Singles and EPs';
    case 'compilation':
      return 'Compilations';
    default:
      return 'Albums';
  }
}

/** Newest first; albums with no year last. */
const byYear = (a: Album, b: Album) =>
  // Two albums with no year are equal, not NaN apart.
  (b.year ?? Number.NEGATIVE_INFINITY) - (a.year ?? Number.NEGATIVE_INFINITY) || 0;

/**
 * An artist's page below its header: the audiobooks of an author, its tracks, the first few
 * unless `expanded`, then its albums by type, each newest first. A catalog's tracks are the
 * artist's most popular.
 */
export function artistRows(
  albums: Album[],
  tracks: Track[],
  { popular, expanded }: { popular: boolean; expanded: boolean },
  books: Audiobook[] = [],
): ArtistRow[] {
  const rows: ArtistRow[] = [];
  if (books.length > 0) {
    rows.push({ type: 'heading', key: 'heading-books', title: 'Audiobooks' });
    rows.push(
      ...books.map((book) => ({
        type: 'book' as const,
        key: `book-${book.provider}/${book.item_id}`,
        book,
      })),
    );
  }
  if (tracks.length > 0) {
    rows.push({ type: 'heading', key: 'heading-tracks', title: popular ? 'Popular' : 'Tracks' });
    const shown = expanded ? tracks : tracks.slice(0, TRACKS_SHOWN);
    rows.push(
      ...shown.map((track, index) => ({
        type: 'track' as const,
        key: `track-${index}-${track.provider}/${track.item_id}`,
        track,
      })),
    );
    if (shown.length < tracks.length) {
      rows.push({ type: 'more', key: 'more', hidden: tracks.length - shown.length });
    }
  }
  for (const title of GROUPS) {
    const group = albums.filter((album) => groupOf(album) === title).sort(byYear);
    if (group.length === 0) continue;
    rows.push({ type: 'heading', key: `heading-${title}`, title });
    rows.push(
      ...group.map((album) => ({
        type: 'album' as const,
        key: `album-${album.provider}/${album.item_id}`,
        album,
      })),
    );
  }
  return rows;
}
