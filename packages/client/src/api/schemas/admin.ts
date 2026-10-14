import * as z from 'zod/mini';

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
});
export type AdminEndpoint = z.infer<typeof adminEndpoint>;

export const adminDevice = z.looseObject({
  id: z.string(),
  name: z.string(),
  enabled: z.boolean(),
  available: z.boolean(),
  members: z.array(adminEndpoint),
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
