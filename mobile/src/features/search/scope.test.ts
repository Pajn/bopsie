import type { MusicProvider } from '@bopsie/client/api/schemas/media';
import { searchScopes, sourceName } from './scope';

const files: MusicProvider = {
  instance_id: 'filesystem_local--1',
  domain: 'filesystem_local',
  name: 'Local files',
  is_streaming_provider: false,
};
const spotify: MusicProvider = {
  instance_id: 'spotify',
  domain: 'spotify',
  name: 'Spotify',
  is_streaming_provider: true,
};

test('without a catalog there is nothing to choose', () => {
  expect(searchScopes([files])).toEqual([]);
});

test('with Spotify, a search covers both, the library or Spotify', () => {
  const scopes = searchScopes([files, spotify]);
  expect(scopes.map((scope) => scope.label)).toEqual(['Both', 'Library', 'Spotify']);
  expect(scopes.map((scope) => scope.providers)).toEqual([undefined, ['library'], ['spotify']]);
});

test("items are marked with their catalog's name, the library's not at all", () => {
  expect(sourceName([files, spotify], 'spotify')).toBe('Spotify');
  expect(sourceName([files, spotify], 'library')).toBeNull();
  expect(sourceName(undefined, 'spotify')).toBeNull();
});
