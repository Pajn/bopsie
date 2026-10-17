import { playerQueue, player as playerSchema } from '@bopsie/client/api/schemas/player';
import type { ActiveGroup } from '@bopsie/client/outputs/outputs';
import type { RemoteNowPlaying } from '@/player/Player';
import { asHeard } from './nowPlaying';
import { changed, remoteNowPlaying } from './remoteState';

const ref = (kind: string, id: string, name: string) => ({
  item_id: id,
  provider: 'library',
  name,
  uri: `library://${kind}/${id}`,
  media_type: kind,
});

const queue = (overrides: object = {}) =>
  asHeard(
    playerQueue.parse({
      queue_id: 'bedroom',
      state: 'playing',
      elapsed_time: 30,
      elapsed_time_last_updated: 1_000,
      current_item: {
        queue_id: 'bedroom',
        queue_item_id: 'i',
        name: 'BABYMETAL - Algorism',
        duration: 217,
        media_item: {
          ...ref('track', '1', 'Algorism'),
          artists: [ref('artist', 'a', 'BABYMETAL')],
          album: ref('album', '7', 'METAL FORTH'),
        },
      },
      ...overrides,
    }),
    playerSchema.parse({ player_id: 'bedroom', provider: 'airplay', name: 'Bedroom' }),
  );

const group = (memberIds: string[], volume: number | null = 30): ActiveGroup => ({
  id: 'bedroom',
  name: 'Bedroom + Philips',
  available: true,
  volume,
  muted: null,
  members: memberIds.map((id) => ({
    id,
    name: id,
    volume,
    muted: null,
    leads: false,
    removable: true,
  })),
});

describe('remoteNowPlaying', () => {
  it('shows the track, with the group on the artist line, and where it was when', () => {
    expect(remoteNowPlaying(queue(), group(['bedroom', 'philips']), 'phone', 'https://m')).toEqual({
      title: 'Algorism',
      artist: 'BABYMETAL · Bedroom + Philips',
      album: 'METAL FORTH',
      artworkUrl: null,
      durationMs: 217_000,
      positionMs: 30_000,
      positionAt: 1_000_000,
      playing: true,
      volume: 30,
    });
  });

  it('leaves a group the phone plays in to the phone', () => {
    expect(remoteNowPlaying(queue(), group(['bedroom', 'phone']), 'phone', 'https://m')).toBeNull();
  });

  it('shows nothing with nothing loaded', () => {
    expect(
      remoteNowPlaying(queue({ current_item: null }), group(['bedroom']), 'phone', 'https://m'),
    ).toBeNull();
  });

  it('has no volume for a group without volume control', () => {
    expect(remoteNowPlaying(queue(), group(['bedroom'], null), 'phone', 'https://m')?.volume).toBe(
      null,
    );
  });
});

describe('changed', () => {
  const shown: RemoteNowPlaying = {
    title: 'Algorism',
    artist: 'BABYMETAL · Bedroom',
    album: null,
    artworkUrl: null,
    durationMs: 217_000,
    positionMs: 30_000,
    positionAt: 1_000_000,
    playing: true,
    volume: 30,
  };

  it('lets the position run on as the session expects it to', () => {
    expect(changed(shown, { ...shown, positionMs: 35_000, positionAt: 1_005_000 })).toBe(false);
  });

  it('tells the session of a seek', () => {
    expect(changed(shown, { ...shown, positionMs: 90_000, positionAt: 1_005_000 })).toBe(true);
  });

  it('tells the session of anything else', () => {
    expect(changed(shown, { ...shown, playing: false })).toBe(true);
    expect(changed(shown, { ...shown, volume: 35 })).toBe(true);
  });

  it('tells the session to start and to stop showing a player', () => {
    expect(changed(null, shown)).toBe(true);
    expect(changed(shown, null)).toBe(true);
    expect(changed(null, null)).toBe(false);
  });
});
