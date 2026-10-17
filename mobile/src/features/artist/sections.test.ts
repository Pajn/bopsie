import {
  album as albumSchema,
  audiobook,
  track as trackSchema,
} from '@bopsie/client/api/schemas/media';
import { artistRows } from './sections';

const album = (id: string, year: number | null, albumType = 'unknown') =>
  albumSchema.parse({
    item_id: id,
    provider: 'library',
    name: `Album ${id}`,
    uri: `library://album/${id}`,
    media_type: 'album',
    year,
    album_type: albumType,
  });
const track = (id: string) =>
  trackSchema.parse({
    item_id: id,
    provider: 'library',
    name: `Track ${id}`,
    uri: `library://track/${id}`,
    media_type: 'track',
  });
const tracks = (count: number) => Array.from({ length: count }, (_, i) => track(String(i)));
const summary = (rows: ReturnType<typeof artistRows>) =>
  rows.map((row) =>
    row.type === 'heading'
      ? `# ${row.title}`
      : row.type === 'track'
        ? `t${row.track.item_id}`
        : row.type === 'album'
          ? `a${row.album.item_id}`
          : row.type === 'book'
            ? `b${row.book.item_id}`
            : `+${row.hidden}`,
  );

describe('artistRows', () => {
  it('lists the first tracks, then the albums newest first, with no year last', () => {
    const rows = artistRows([album('1', 2001), album('2', null), album('3', 2010)], tracks(7), {
      popular: false,
      expanded: false,
    });
    expect(summary(rows)).toEqual([
      '# Tracks',
      't0',
      't1',
      't2',
      't3',
      't4',
      '+2',
      '# Albums',
      'a3',
      'a1',
      'a2',
    ]);
  });

  it('lists every track once expanded', () => {
    const rows = artistRows([], tracks(7), { popular: false, expanded: true });
    expect(summary(rows)).toHaveLength(8);
    expect(summary(rows)).not.toContain('+0');
  });

  it("calls a catalog's tracks popular", () => {
    const rows = artistRows([], tracks(1), { popular: true, expanded: false });
    expect(summary(rows)[0]).toBe('# Popular');
  });

  it('groups albums by the type a catalog gives', () => {
    const rows = artistRows(
      [
        album('1', 2001, 'single'),
        album('2', 2002, 'album'),
        album('3', 2003, 'compilation'),
        album('4', 2004, 'ep'),
      ],
      [],
      { popular: true, expanded: false },
    );
    expect(summary(rows)).toEqual([
      '# Albums',
      'a2',
      '# Singles and EPs',
      'a4',
      'a1',
      '# Compilations',
      'a3',
    ]);
  });

  it('is empty for an artist with neither', () => {
    expect(artistRows([], [], { popular: false, expanded: false })).toEqual([]);
  });
});

describe('an author', () => {
  it('lists their audiobooks first', () => {
    const book = audiobook.parse({
      item_id: 'li_h',
      provider: 'audiobookshelf',
      name: 'The Hobbit',
      uri: 'audiobookshelf://audiobook/li_h',
      media_type: 'audiobook',
    });
    const rows = artistRows([], tracks(1), { popular: true, expanded: false }, [book]);
    expect(summary(rows)).toEqual(['# Audiobooks', 'bli_h', '# Popular', 't0']);
  });
});
