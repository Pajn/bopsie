import type { ReleaseDetails } from '@bopsie/client/api/schemas/admin';
import type { Album } from '@bopsie/client/api/schemas/media';

/** One thing a release says of itself, and whether the album says otherwise. */
export interface ReleaseFact {
  kind: 'title' | 'artist' | 'year' | 'country' | 'formats' | 'tracks';
  text: string;
  differs: boolean;
}

/** The same words however they are written: case, accents and punctuation aside. */
function same(a: string, b: string): boolean {
  const plain = (s: string) =>
    s
      .normalize('NFKD')
      .replace(/\p{M}/gu, '')
      .toLowerCase()
      .replace(/[^\p{L}\p{N}]+/gu, ' ')
      .trim();
  return plain(a) === plain(b);
}

/** MusicBrainz's codes for releases beyond one country, which read as nothing otherwise. */
const REGIONS: Record<string, string> = { XW: 'Worldwide', XE: 'Europe' };

/** Formats as a release lists them, one per disc: `2×CD`, `CD + DVD`. */
export function formats(list: string[]): string {
  const counts = new Map<string, number>();
  for (const format of list) counts.set(format, (counts.get(format) ?? 0) + 1);
  return [...counts].map(([format, n]) => (n > 1 ? `${n}×${format}` : format)).join(' + ');
}

/**
 * What a suggested release is, beside the album it may be: its title and artist where they
 * read differently from the album's, its year, marked where it is not the album's, its country,
 * formats and tracks.
 */
export function releaseFacts(
  album: Album,
  release: ReleaseDetails,
): {
  title: ReleaseFact;
  details: ReleaseFact[];
} {
  const artists = album.artists.map((artist) => artist.name).join(', ');
  const details: ReleaseFact[] = [];
  details.push({ kind: 'artist', text: release.artist, differs: !same(release.artist, artists) });
  if (release.year !== null) {
    details.push({
      kind: 'year',
      text: String(release.year),
      differs: album.year !== null && album.year !== undefined && album.year !== release.year,
    });
  }
  if (release.country) {
    details.push({
      kind: 'country',
      text: REGIONS[release.country] ?? release.country,
      differs: false,
    });
  }
  if (release.formats.length > 0)
    details.push({ kind: 'formats', text: formats(release.formats), differs: false });
  details.push({
    kind: 'tracks',
    text: `${release.track_count} ${release.track_count === 1 ? 'track' : 'tracks'}`,
    differs: false,
  });
  return {
    title: { kind: 'title', text: release.title, differs: !same(release.title, album.name) },
    details,
  };
}
