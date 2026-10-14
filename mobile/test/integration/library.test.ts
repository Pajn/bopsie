import type { MaConnection } from '@bopsie/client/api/client';
import {
  albumTracks,
  artistAlbums,
  getAlbum,
  libraryItems,
  search,
} from '@bopsie/client/api/endpoints/library';
import { imageUrl } from '@bopsie/client/api/images';
import { connect, describeServer, serverUrl } from './server';

describeServer('library', () => {
  let connection: MaConnection;
  beforeAll(async () => {
    ({ connection } = await connect());
  });
  afterAll(() => connection.close());

  it('lists albums and artists in pages', async () => {
    const albums = await libraryItems(connection, 'albums', { limit: 10, offset: 0 });
    expect(albums.map((album) => album.name)).toEqual(['First Album', 'Second Album']);
    expect(albums[0]?.artists[0]?.name).toBe('Test Artist');

    const second = await libraryItems(connection, 'albums', { limit: 1, offset: 1 });
    expect(second.map((album) => album.name)).toEqual(['Second Album']);

    const artists = await libraryItems(connection, 'artists', { limit: 10, offset: 0 });
    expect(artists.map((artist) => artist.name)).toEqual(['Other Artist', 'Test Artist']);
  });

  it('drills into an album and an artist', async () => {
    const [first] = await libraryItems(connection, 'albums', { limit: 1, offset: 0 });
    if (!first) throw new Error('No albums');
    const ref = { itemId: first.item_id, provider: first.provider };
    expect((await getAlbum(connection, ref)).year).toBe(2021);
    const tracks = await albumTracks(connection, ref);
    expect(tracks.map((track) => track.name)).toEqual(['Opening', 'Middle', 'Closing']);
    expect(tracks[0]?.duration).toBeCloseTo(30, 0);

    const artist = first.artists[0];
    if (!artist) throw new Error('No artist');
    const albums = await artistAlbums(connection, {
      itemId: artist.item_id,
      provider: artist.provider,
    });
    expect(albums.map((album) => album.name)).toContain('First Album');
  });

  it('searches the library', async () => {
    const results = await search(connection, 'Gamma');
    expect(results.tracks.map((track) => track.name)).toContain('Gamma');
  });

  it('serves artwork through the image proxy without a token', async () => {
    const [album] = await libraryItems(connection, 'albums', { limit: 1, offset: 0 });
    if (!album) throw new Error('No albums');
    const url = imageUrl(serverUrl(), album, 160);
    if (!url) throw new Error('No artwork');
    const response = await fetch(url);
    expect(response.status).toBe(200);
    expect(response.headers.get('content-type')).toMatch(/^image\//);
  });
});
