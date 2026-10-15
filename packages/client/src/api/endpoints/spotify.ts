import * as z from 'zod/mini';
import type { MaConnection } from '../client';
import { spotifyStatus } from '../schemas/spotify';

/** How the server's Spotify account stands. Admins only. */
export const getSpotifyStatus = (connection: MaConnection) =>
  connection.command('admin/spotify/status', {}, { schema: spotifyStatus });

/** Uses the Spotify developer app with this client ID for the Web API; `null` forgets it. */
export const setSpotifyClientId = (connection: MaConnection, clientId: string | null) =>
  connection.command<void>('admin/spotify/set_client_id', { client_id: clientId });

/**
 * Starts logging in to Spotify's Web API: the Spotify page to open, which comes back to
 * `redirectUri`, the server's `/spotify/callback`.
 */
export const startSpotifyLogin = (connection: MaConnection, redirectUri: string) =>
  connection.command(
    'admin/spotify/login',
    { redirect_uri: redirectUri },
    { schema: z.object({ url: z.string() }) },
  );

/** Announces a device to pick in the Spotify app, which signs the server in for playback. */
export const startSpotifyPairing = (connection: MaConnection) =>
  connection.command<void>('admin/spotify/pair_start', {});

export const cancelSpotifyPairing = (connection: MaConnection) =>
  connection.command<void>('admin/spotify/pair_cancel', {});

/** Ends and forgets one of the server's Spotify logins. */
export const signOutOfSpotify = (connection: MaConnection, login: 'web' | 'playback') =>
  connection.command<void>('admin/spotify/sign_out', { login });
