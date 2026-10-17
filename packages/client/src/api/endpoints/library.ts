import * as z from 'zod/mini';
import type { MaConnection } from '../client';
import {
  album,
  artist,
  audiobook,
  musicProvider,
  playlist,
  searchResults,
  track,
} from '../schemas/media';

export type LibraryKind = 'artists' | 'albums' | 'tracks' | 'playlists';

export interface PageOptions {
  limit: number;
  offset: number;
  /** One of the server's `order_by` keys, such as `sort_name` or `timestamp_added_desc`. */
  orderBy?: string;
  search?: string;
}

const listSchemas = {
  artists: z.array(artist),
  albums: z.array(album),
  tracks: z.array(track),
  playlists: z.array(playlist),
};

type ListResult<K extends LibraryKind> = z.output<(typeof listSchemas)[K]>;

/** One page of the library. Pages are plain arrays; a short page is the last. */
export function libraryItems<K extends LibraryKind>(
  connection: MaConnection,
  kind: K,
  { limit, offset, orderBy = 'sort_name', search }: PageOptions,
): Promise<ListResult<K>> {
  return connection.command(
    `music/${kind}/library_items`,
    {
      limit,
      offset,
      order_by: orderBy,
      ...(search ? { search } : {}),
      // Lists only show artists that have albums, as the server's own interface does.
      ...(kind === 'artists' ? { album_artists_only: true } : {}),
    },
    { schema: listSchemas[kind] as unknown as z.ZodMiniType<ListResult<K>> },
  );
}

/** How many items of a kind the library holds, counted as `libraryItems` lists them. */
export function libraryCount(connection: MaConnection, kind: LibraryKind): Promise<number> {
  return connection.command(
    `music/${kind}/count`,
    kind === 'artists' ? { album_artists_only: true } : {},
    { schema: z.number() },
  );
}

interface ItemRef {
  itemId: string;
  provider: string;
}

const refArgs = ({ itemId, provider }: ItemRef) => ({
  item_id: itemId,
  provider_instance_id_or_domain: provider,
});

export const getAlbum = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/albums/get', refArgs(ref), { schema: album });

export const albumTracks = (connection: MaConnection, ref: ItemRef) =>
  connection.command(
    'music/albums/album_tracks',
    { ...refArgs(ref), in_library_only: false },
    { schema: z.array(track) },
  );

export const getTrack = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/tracks/get', refArgs(ref), { schema: track });

export const getArtist = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/artists/get', refArgs(ref), { schema: artist });

export const artistAlbums = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/artists/artist_albums', refArgs(ref), { schema: z.array(album) });

export const artistTracks = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/artists/artist_tracks', refArgs(ref), { schema: z.array(track) });

export const getPlaylist = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/playlists/get', refArgs(ref), { schema: playlist });

/** Streamed by the server in batches, which the connection reassembles. */
export const playlistTracks = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/playlists/playlist_tracks', refArgs(ref), {
    schema: z.array(track),
    timeoutMs: 60_000,
  });

/**
 * Searches by name. `providers` narrows it to some sources, by provider id: `library`, or a
 * catalog such as `spotify`; left out, every source is searched.
 */
export const search = (connection: MaConnection, query: string, limit = 25, providers?: string[]) =>
  connection.command(
    'music/search',
    {
      search_query: query,
      media_types: ['artist', 'album', 'track', 'playlist', 'audiobook'],
      limit,
      ...(providers && { providers }),
    },
    { schema: searchResults },
  );

/**
 * A page of the audiobooks the sources serve, such as Audiobookshelf's, in their order. A short
 * page is the last.
 */
export const audiobooks = (connection: MaConnection, { limit, offset }: PageOptions) =>
  connection.command(
    'music/audiobooks/library_items',
    { limit, offset },
    { schema: z.array(audiobook) },
  );

export const audiobookCount = (connection: MaConnection) =>
  connection.command('music/audiobooks/count', {}, { schema: z.number() });

/** Audiobooks found by title or author. */
export const searchAudiobooks = async (connection: MaConnection, query: string, limit = 50) =>
  (
    await connection.command(
      'music/search',
      { search_query: query, media_types: ['audiobook'], limit },
      { schema: searchResults },
    )
  ).audiobooks;

/**
 * The audiobooks listening has started and not finished, the most recently listened to first,
 * as the sources keep it: Audiobookshelf's continue listening.
 */
export const inProgressItems = (connection: MaConnection, limit = 12) =>
  connection.command('music/in_progress_items', { limit }, { schema: z.array(audiobook) });

export const getAudiobook = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/audiobooks/get', refArgs(ref), { schema: audiobook });

/** An author's books, from the source that serves them. */
export const artistAudiobooks = (connection: MaConnection, ref: ItemRef) =>
  connection.command('music/artists/artist_audiobooks', refArgs(ref), {
    schema: z.array(audiobook),
  });

/** The music providers: the library's sources, and catalogs such as Spotify. */
export const listProviders = (connection: MaConnection) =>
  connection.command('providers', {}, { schema: z.array(musicProvider) });
