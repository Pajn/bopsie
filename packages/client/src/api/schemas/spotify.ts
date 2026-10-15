import * as z from 'zod/mini';

/** How the server's Spotify account stands. */
export const spotifyStatus = z.looseObject({
  /** Whether the config file turns Spotify on. */
  enabled: z.boolean(),
  /** The developer app's client ID, once given. */
  client_id: z.nullable(z.string()),
  /** Who the Web API login is: search and the saved library. */
  web_user: z.nullable(z.string()),
  /** Who the playback login is: playing and playlists. */
  playback_user: z.nullable(z.string()),
  /** Waiting to be picked in the Spotify app, under `sign_in_name`. */
  pairing: z.boolean(),
  sign_in_name: z.string(),
  error: z.nullable(z.string()),
});
export type SpotifyStatus = z.infer<typeof spotifyStatus>;
