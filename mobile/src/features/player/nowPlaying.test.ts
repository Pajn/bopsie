import { player, playerQueue, queueItem } from '@bopsie/client/api/schemas/player';
import { asHeard, itemLinks, nowPlaying } from './nowPlaying';

const queue = (currentItem: object | null) =>
  playerQueue.parse({ queue_id: 'q', current_item: currentItem });

describe('nowPlaying', () => {
  it('is null without a current item', () => {
    expect(nowPlaying(queue(null))).toBeNull();
  });

  it('names the track with its artists and album', () => {
    const result = nowPlaying(
      queue({
        queue_id: 'q',
        queue_item_id: 'i',
        name: 'Artist - Song',
        duration: 181,
        media_item: {
          item_id: '1',
          provider: 'library',
          name: 'Song',
          uri: 'library://track/1',
          media_type: 'track',
          artists: [
            {
              item_id: 'a',
              provider: 'library',
              name: 'Ada',
              uri: 'library://artist/a',
              media_type: 'artist',
            },
            {
              item_id: 'b',
              provider: 'library',
              name: 'Bo',
              uri: 'library://artist/b',
              media_type: 'artist',
            },
          ],
          album: {
            item_id: 'x',
            provider: 'library',
            name: 'Record',
            uri: 'library://album/x',
            media_type: 'album',
          },
        },
      }),
    );
    expect(result).toMatchObject({ title: 'Song', subtitle: 'Ada, Bo · Record', duration: 181 });
  });

  it('falls back to the queue item name for media that is not a track', () => {
    const result = nowPlaying(queue({ queue_id: 'q', queue_item_id: 'i', name: 'Radio One' }));
    expect(result).toMatchObject({ title: 'Radio One', subtitle: '', duration: null });
  });
});

describe('asHeard', () => {
  const kitchen = (extra: object) =>
    player.parse({ player_id: 'q', provider: 'airplay', name: 'Kitchen', ...extra });
  const paused = playerQueue.parse({
    queue_id: 'q',
    state: 'paused',
    elapsed_time: 12,
    current_item: { queue_id: 'q', queue_item_id: 'i', name: 'Song' },
  });

  it('is the queue while the player plays its queue', () => {
    const heard = asHeard(paused, kitchen({ active_source: 'q' }));
    expect(heard.source).toBeNull();
    expect(heard.current_item?.name).toBe('Song');
    expect(asHeard(paused, undefined).source).toBeNull();
  });

  it('is the source while one plays in place of the queue', () => {
    const heard = asHeard(
      paused,
      kitchen({
        playback_state: 'playing',
        active_source: 'spotify_connect--q',
        source_list: [{ id: 'spotify_connect--q', name: 'Spotify' }],
        current_media: {
          title: 'Teardrop',
          artist: 'Massive Attack',
          album: 'Mezzanine',
          image_url: 'https://i.scdn.co/image/abc',
          duration: 330,
          elapsed_time: 40,
          elapsed_time_last_updated: 1_000,
        },
      }),
    );
    expect(heard.state).toBe('playing');
    expect(heard.elapsed_time).toBe(40);
    expect(heard.source?.name).toBe('Spotify');
    expect(heard.current_item?.image?.path).toBe('https://i.scdn.co/image/abc');
    expect(nowPlaying(heard)).toMatchObject({
      title: 'Teardrop',
      subtitle: 'Massive Attack · Mezzanine',
      duration: 330,
    });
  });
});

describe('itemLinks', () => {
  const ref = (kind: string, id: string, name: string) => ({
    item_id: id,
    provider: 'library',
    name,
    uri: `library://${kind}/${id}`,
    media_type: kind,
  });
  const item = (mediaItem: object | null) =>
    queueItem.parse({ queue_id: 'q', queue_item_id: 'i', name: 'Song', media_item: mediaItem });

  it('leads to the album, then each artist', () => {
    const links = itemLinks(
      item({
        ...ref('track', '1', 'Song'),
        album: ref('album', '7', 'Record'),
        artists: [ref('artist', 'a', 'Ada'), ref('artist', 'b', 'Bo')],
      }),
    );
    expect(links).toEqual([
      { kind: 'album', id: '7', provider: 'library', name: 'Record' },
      { kind: 'artist', id: 'a', provider: 'library', name: 'Ada' },
      { kind: 'artist', id: 'b', provider: 'library', name: 'Bo' },
    ]);
  });

  it('leads to the artists of a track with no album', () => {
    const links = itemLinks(
      item({ ...ref('track', '1', 'Song'), artists: [ref('artist', 'a', 'Ada')] }),
    );
    expect(links.map((link) => link.kind)).toEqual(['artist']);
  });

  it('leads nowhere for an item without media, such as a radio stream', () => {
    expect(itemLinks(item(null))).toEqual([]);
  });
});
