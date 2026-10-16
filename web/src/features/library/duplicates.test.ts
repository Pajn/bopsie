import type { DuplicateFile } from '@bopsie/client/api/schemas/admin';
import { byFolders } from './duplicates';

const duplicate = (name: string, ...files: string[]) =>
  ({ track: { name, uri: `library://track/${name}` }, files }) as unknown as DuplicateFile;

test('copies in the same folders are one group, the largest first', () => {
  const groups = byFolders([
    duplicate('Regrets', '/music/A/Chocolate/05.flac', '/music/Mix/A/Chocolate/27.flac'),
    duplicate('Road', '/music/B/METAL (2016)/01.flac', '/music/B/Metal (2016)/01.flac'),
    duplicate('Karate', '/music/B/METAL (2016)/02.flac', '/music/B/Metal (2016)/02.flac'),
  ]);
  expect(groups.map((g) => [g.folders, g.tracks.map((t) => t.track.name)])).toEqual([
    [
      ['/music/B/METAL (2016)', '/music/B/Metal (2016)'],
      ['Road', 'Karate'],
    ],
    [['/music/A/Chocolate', '/music/Mix/A/Chocolate'], ['Regrets']],
  ]);
});

test('two copies in one folder are a group of that folder alone', () => {
  const [group] = byFolders([duplicate('Twice', '/music/A/01.flac', '/music/A/01 (1).flac')]);
  expect(group?.folders).toEqual(['/music/A']);
});
