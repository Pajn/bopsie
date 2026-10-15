import * as z from 'zod/mini';
import type { MaConnection } from '../client';
import {
  adminDevice,
  adminGroup,
  adminServer,
  librarySuggestion,
  logLine,
  releaseSuggestion,
} from '../schemas/admin';

/** Every device and its endpoints, with all the server knows of each. Admins only. */
export const adminDevices = (connection: MaConnection) =>
  connection.command('admin/devices', {}, { schema: z.array(adminDevice) });

/** Each device that leads a group or plays, and the plan it plays by. Admins only. */
export const adminGroups = (connection: MaConnection) =>
  connection.command('admin/groups', {}, { schema: z.array(adminGroup) });

/** The server: version, what runs, the library and its last sync, the config. Admins only. */
export const adminServerInfo = (connection: MaConnection) =>
  connection.command('admin/server', {}, { schema: adminServer });

/** The log's lines after `after`, by sequence number, oldest first. Admins only. */
export const adminLogs = (connection: MaConnection, after?: number, limit?: number) =>
  connection.command(
    'admin/logs',
    { ...(after !== undefined && { after }), ...(limit !== undefined && { limit }) },
    { schema: z.array(logLine) },
  );

/**
 * Saves a player's settings over the config file. A field left out stays as it is; `null`
 * clears what was saved, so the config file's value or the default applies again.
 */
export const updatePlayerSettings = (
  connection: MaConnection,
  playerId: string,
  changes: {
    name?: string | null;
    enabled?: boolean | null;
    max_volume?: number | null;
    spotify_connect?: boolean | null;
    spotify_name?: string | null;
  },
) => connection.command<void>('admin/player/update', { player_id: playerId, ...changes });

/**
 * Saves an endpoint's settings over the config file: whether the player plays over it, its
 * password, and quirks by name. `null` clears a saved value.
 */
export const updateEndpointSettings = (
  connection: MaConnection,
  endpointId: string,
  changes: {
    enabled?: boolean | null;
    password?: string | null;
    quirks?: Record<string, unknown>;
  },
) => connection.command<void>('admin/endpoint/update', { endpoint_id: endpointId, ...changes });

export type LibraryKindName = 'artist' | 'album' | 'track' | 'playlist';

/** Edits a library item; `null` resets a field to what the scan found. */
export const updateLibraryItem = (
  connection: MaConnection,
  mediaType: LibraryKindName,
  itemId: string,
  changes: Record<string, unknown>,
) =>
  connection.command<void>('admin/library/update', {
    media_type: mediaType,
    item_id: itemId,
    changes,
  });

/** The fields of a library item edited by hand, with their values. */
export const libraryItemEdits = (
  connection: MaConnection,
  mediaType: LibraryKindName,
  itemId: string,
) =>
  connection.command(
    'admin/library/edits',
    { media_type: mediaType, item_id: itemId },
    { schema: z.record(z.string(), z.unknown()) },
  );

/** Items from different providers that may be the same; with decided ones too when `all`. */
export const librarySuggestions = (connection: MaConnection, all = false) =>
  connection.command('admin/library/suggestions', { all }, { schema: z.array(librarySuggestion) });

/** Merges a suggestion's two items at once, or keeps them apart; either holds at every sync. */
export const decideSuggestion = (
  connection: MaConnection,
  id: number,
  decision: 'merge' | 'apart',
) => connection.command<void>('admin/library/decide', { id, decision });

/**
 * Takes a provider back out of a merged album, into an album of its own that stays apart.
 * Answers with that album's id, or `null` when the provider did not share the album.
 */
export const separateAlbum = (connection: MaConnection, itemId: string, providerInstance: string) =>
  connection.command(
    'admin/library/separate',
    { item_id: itemId, provider_instance: providerInstance },
    { schema: z.object({ item_id: z.nullable(z.string()) }) },
  );

/** MusicBrainz releases albums look like; with decided ones too when `all`. */
export const releaseSuggestions = (connection: MaConnection, all = false) =>
  connection.command(
    'admin/library/release_suggestions',
    { all },
    { schema: z.array(releaseSuggestion) },
  );

/** Accepts the release suggested for an album, taken at its next lookup, or rejects it. */
export const decideRelease = (
  connection: MaConnection,
  albumId: string,
  decision: 'accept' | 'reject',
) => connection.command<void>('admin/library/decide_release', { item_id: albumId, decision });

/** Has an album looked up on MusicBrainz again now, as if it had never been. */
export const refreshMetadata = (connection: MaConnection, albumId: string) =>
  connection.command<void>('admin/library/refresh_metadata', { item_id: albumId });

/**
 * Forgets a decision to merge or keep two items apart, Separate's too, and compares them again
 * at once: the same release merges, and one that may be is suggested again.
 */
export const reconsider = (connection: MaConnection, id: number) =>
  connection.command<void>('admin/library/reconsider', { id });

/** Forgets a decision on the release suggested for an album, and looks it up again at once. */
export const reconsiderRelease = (connection: MaConnection, albumId: string) =>
  connection.command<void>('admin/library/reconsider_release', { item_id: albumId });

/** Asks an AirPlay speaker that pairs with a PIN to show one. */
export const pairStart = (connection: MaConnection, playerId: string) =>
  connection.command<void>('airplay/pair_start', { player_id: playerId });

/** Pairs with the PIN the speaker shows. */
export const pairFinish = (connection: MaConnection, playerId: string, pin: string) =>
  connection.command<void>('airplay/pair_finish', { player_id: playerId, pin });

/**
 * Uploads artwork, a JPEG or PNG, over HTTP since it is binary. Answers with the proxy id an
 * edit's `image` names it by.
 */
export async function uploadArtwork(
  serverUrl: string,
  token: string,
  image: Blob,
  fetcher: typeof fetch = fetch,
): Promise<string> {
  const response = await fetcher(`${serverUrl.replace(/\/+$/, '')}/artwork`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}` },
    body: image,
  });
  if (!response.ok)
    throw new Error((await response.text()) || `Upload failed (${response.status})`);
  const answer = (await response.json()) as { proxy_id?: unknown };
  if (typeof answer.proxy_id !== 'string') throw new Error('The server answered without an id');
  return answer.proxy_id;
}
