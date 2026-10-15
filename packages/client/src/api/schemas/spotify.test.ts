import { spotifyStatus } from './spotify';

test('the Spotify status reads as the server writes it', () => {
  const status = spotifyStatus.parse({
    enabled: true,
    client_id: 'abc',
    web_user: 'Listener',
    playback_user: null,
    pairing: true,
    sign_in_name: 'Bopsie sign-in',
    error: null,
  });
  expect(status.pairing).toBe(true);
  expect(status.playback_user).toBeNull();
});
