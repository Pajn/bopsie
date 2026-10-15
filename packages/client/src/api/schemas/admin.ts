import * as z from 'zod/mini';
import { album } from './media';

/**
 * What the server's admin commands answer: this server's own, not Music Assistant's. Fields the
 * server leaves free in shape, such as a protocol's diagnostics, read as unknown.
 */

export const format = z.object({
  sample_rate: z.number(),
  channels: z.number(),
  bits: z.number(),
});
export type Format = z.infer<typeof format>;

export const healthEvent = z.object({ event: z.string(), ago_s: z.number() });

/** A setting's value and the layer it comes from: `default`, `config` or `saved`. */
const sourced = <T extends z.ZodMiniType>(value: T) => z.object({ value, layer: z.string() });

export const adminEndpoint = z.looseObject({
  id: z.string(),
  provider: z.string(),
  name: z.string(),
  available: z.boolean(),
  volume: z.nullable(z.number()),
  muted: z.nullable(z.boolean()),
  model: z.nullable(z.string()),
  manufacturer: z.nullable(z.string()),
  software_version: z.nullable(z.string()),
  app: z.boolean(),
  native_leader: z.nullable(z.string()),
  facts: z.looseObject({
    mac: z.nullable(z.string()),
    serial: z.nullable(z.string()),
    uuid: z.nullable(z.string()),
    address: z.nullable(z.string()),
    features: z.nullable(z.string()),
    status_flags: z.nullable(z.string()),
  }),
  capabilities: z.looseObject({
    formats: z.array(format),
    lead_ms: z.number(),
    sync_group: z.boolean(),
    max_group_size: z.nullable(z.number()),
    volume: z.boolean(),
    mute: z.boolean(),
    pairing: z.string(),
    native_group: z.nullable(z.object({ family: z.string(), version: z.number() })),
    withdrawn: z.array(z.string()),
  }),
  quirks: z.record(z.string(), z.unknown()),
  /** Each quirk's layer: `{layer: 'default' | 'rule' | 'config', name?}`. */
  quirk_sources: z.record(
    z.string(),
    z.looseObject({ layer: z.string(), name: z.optional(z.string()) }),
  ),
  health: z.object({
    level: z.string(),
    counts: z.record(z.string(), z.number()),
    recent: z.array(healthEvent),
  }),
  diagnostics: z.unknown(),
  /** Turned off in the player's settings. */
  disabled: z.boolean(),
  settings: z.object({
    enabled: sourced(z.boolean()),
    /** Whether a password is set; it is never shown. */
    password: sourced(z.boolean()),
  }),
});
export type AdminEndpoint = z.infer<typeof adminEndpoint>;

/** A source announced for a device, as it stands. */
export const adminSource = z.looseObject({
  id: z.string(),
  name: z.string(),
  /** Someone picked it, as in the Spotify app, and it has a session. */
  connected: z.boolean(),
  playing: z.boolean(),
  title: z.nullish(z.string()),
  artist: z.nullish(z.string()),
});
export type AdminSource = z.infer<typeof adminSource>;

export const adminDevice = z.looseObject({
  id: z.string(),
  name: z.string(),
  enabled: z.boolean(),
  available: z.boolean(),
  max_volume: z.nullable(z.number()),
  /** What it is called in the Spotify app. */
  spotify_name: z.string(),
  /** What it is called in the Spotify app without a Spotify name of its own. */
  spotify_default_name: z.string(),
  settings: z.object({
    name: sourced(z.nullable(z.string())),
    enabled: sourced(z.boolean()),
    max_volume: sourced(z.nullable(z.number())),
    /** Whether it shows in the Spotify app as a Spotify Connect device. */
    spotify_connect: sourced(z.boolean()),
    spotify_name: sourced(z.nullable(z.string())),
  }),
  members: z.array(adminEndpoint),
  /** The sources announced for it, such as its Spotify Connect device. */
  sources: z.array(adminSource),
});
export type AdminDevice = z.infer<typeof adminDevice>;

export const adminGroup = z.looseObject({
  leader: z.string(),
  members: z.array(z.string()),
  plan: z.nullable(
    z.looseObject({
      strategy: z.string(),
      carriers: z.array(
        z.object({ device: z.string(), endpoint: z.string(), format, native: z.boolean() }),
      ),
      sample_rate: z.number(),
      channels: z.number(),
      lead_ms: z.number(),
      dropped: z.array(z.object({ device: z.string(), name: z.string(), message: z.string() })),
    }),
  ),
});
export type AdminGroup = z.infer<typeof adminGroup>;

export const adminServer = z.looseObject({
  version: z.string(),
  uptime_s: z.number(),
  providers: z.looseObject({
    airplay: z.boolean(),
    airplay_ptp_daemon: z.nullable(z.number()),
    linkplay: z.boolean(),
    sendspin: z.boolean(),
  }),
  ffmpeg: z.boolean(),
  library: z.looseObject({
    providers: z.array(z.string()),
    artists: z.number(),
    albums: z.number(),
    tracks: z.number(),
    playlists: z.number(),
    syncing: z.boolean(),
    last_sync: z.nullable(
      z.looseObject({
        started: z.number(),
        finished: z.number(),
        added: z.number(),
        updated: z.number(),
        deleted: z.number(),
        failed: z.array(z.object({ provider: z.string(), error: z.string() })),
      }),
    ),
  }),
  config: z.unknown(),
});
export type AdminServer = z.infer<typeof adminServer>;

export const logLine = z.object({
  seq: z.number(),
  unix_ms: z.number(),
  level: z.string(),
  target: z.string(),
  message: z.string(),
  fields: z.record(z.string(), z.string()),
});
export type LogLine = z.infer<typeof logLine>;

/** Two library items from different providers that may be the same, for a person to decide. */
export const librarySuggestion = z.looseObject({
  id: z.number(),
  media_type: z.string(),
  a: album,
  b: album,
  reason: z.string(),
  decision: z.nullable(z.enum(['merge', 'apart'])),
});
export type LibrarySuggestion = z.infer<typeof librarySuggestion>;
