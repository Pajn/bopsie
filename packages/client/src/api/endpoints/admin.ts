import * as z from 'zod/mini';
import type { MaConnection } from '../client';
import { adminDevice, adminGroup, adminServer, logLine } from '../schemas/admin';

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
