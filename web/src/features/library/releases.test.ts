import type { ReleaseDetails } from '@bopsie/client/api/schemas/admin';
import type { Album } from '@bopsie/client/api/schemas/media';
import { formats, releaseFacts } from './releases';

const album = (name: string, artist: string, year: number | null) =>
  ({ name, year, artists: [{ name: artist }] }) as unknown as Album;

const release = (extra: Partial<ReleaseDetails> = {}): ReleaseDetails => ({
  artist: 'Nightwish',
  title: 'Nemo',
  year: 2005,
  country: 'FI',
  formats: ['Digital Media'],
  track_count: 4,
  disc_count: 1,
  ...extra,
});

test('a year other than the album’s is marked, as is another title', () => {
  const facts = releaseFacts(album('Nemo', 'Nightwish', 2004), release({ title: 'Nemo (Single)' }));
  expect(facts.title).toEqual({ kind: 'title', text: 'Nemo (Single)', differs: true });
  expect(facts.details).toEqual([
    { kind: 'artist', text: 'Nightwish', differs: false },
    { kind: 'year', text: '2005', differs: true },
    { kind: 'country', text: 'FI', differs: false },
    { kind: 'formats', text: 'Digital Media', differs: false },
    { kind: 'tracks', text: '4 tracks', differs: false },
  ]);
});

test('the same words written otherwise are the same', () => {
  const facts = releaseFacts(
    album('3486 Ord Från...', 'Lars Winnerbäck', 1995),
    release({ artist: 'Lars Winnerback', title: '3486 ord från', year: 1995 }),
  );
  expect(facts.title.differs).toBe(false);
  expect(facts.details[0]?.differs).toBe(false);
  expect(facts.details[1]?.differs).toBe(false);
});

test('formats count their discs', () => {
  expect(formats(['CD', 'CD'])).toBe('2×CD');
  expect(formats(['CD', 'DVD'])).toBe('CD + DVD');
});

test('regions read as words', () => {
  const facts = releaseFacts(album('Nemo', 'Nightwish', 2005), release({ country: 'XW' }));
  expect(facts.details.map((f) => f.text)).toContain('Worldwide');
});
