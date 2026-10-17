import * as z from 'zod/mini';
import { album, track } from './media';

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
    /** Looking albums up on MusicBrainz. Older servers leave it out. */
    metadata: z.optional(
      z.looseObject({
        enabled: z.boolean(),
        running: z.boolean(),
        albums: z.number(),
        checked: z.number(),
        identified: z.number(),
        suggested: z.number(),
        last_run: z.nullable(
          z.looseObject({
            finished: z.number(),
            matched: z.number(),
            suggested: z.number(),
            unmatched: z.number(),
            failed: z.number(),
          }),
        ),
      }),
    ),
    /** How loud files are, for normalisation: from tags, or measured. Older servers leave it
     * out. */
    loudness: z.optional(
      z.looseObject({
        enabled: z.boolean(),
        running: z.boolean(),
        files: z.number(),
        tagged: z.number(),
        measured: z.number(),
        due: z.number(),
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

/** What a suggested release is, to compare with the album beside it. */
export const releaseDetails = z.looseObject({
  artist: z.string(),
  title: z.string(),
  year: z.nullable(z.number()),
  country: z.nullable(z.string()),
  formats: z.array(z.string()),
  track_count: z.number(),
  disc_count: z.number(),
});
export type ReleaseDetails = z.infer<typeof releaseDetails>;

/** A MusicBrainz release an album looks like but was not certain enough to take. */
export const releaseSuggestion = z.looseObject({
  album,
  release_id: z.string(),
  title: z.string(),
  /** Null for a suggestion made before details were kept, until the album is looked up again. */
  release: z.nullable(releaseDetails),
  url: z.string(),
  distance: z.number(),
  decision: z.nullable(z.enum(['accept', 'reject'])),
});
export type ReleaseSuggestion = z.infer<typeof releaseSuggestion>;

/** A track kept as more than one file, with its files. */
export const duplicateFile = z.looseObject({
  track,
  files: z.array(z.string()),
});
export type DuplicateFile = z.infer<typeof duplicateFile>;

/** The sweep each speaker plays in a calibration: an exponential sine sweep. */
export const chirp = z.looseObject({
  from_hz: z.number(),
  to_hz: z.number(),
  duration_ms: z.number(),
  fade_ms: z.number(),
  amplitude: z.number(),
});
export type Chirp = z.infer<typeof chirp>;

/** A calibration as the server scheduled it; times are microseconds on the server's clock. */
export const calibration = z.looseObject({
  chirp,
  chirps: z.array(
    z.looseObject({
      player_id: z.string(),
      name: z.string(),
      endpoint_id: z.string(),
      at_us: z.number(),
    }),
  ),
  ends_at_us: z.number(),
  skipped: z.array(z.string()),
  /** The server's clock as it answered. */
  now_us: z.number(),
});
export type Calibration = z.infer<typeof calibration>;

export const calibrationAdjustment = z.looseObject({
  endpoint_id: z.string(),
  previous_ms: z.number(),
  sync_adjust_ms: z.number(),
});
export type CalibrationAdjustment = z.infer<typeof calibrationAdjustment>;

/** A speaker the last calibration measured, and what it set for it. */
export const calibratedSpeaker = z.looseObject({
  endpoint_id: z.string(),
  device_id: z.nullable(z.string()),
  name: z.nullable(z.string()),
  /** How late its chirps were heard after they were due, in ms. */
  heard_ms: z.number(),
  previous_ms: z.number(),
  sync_adjust_ms: z.number(),
  /** What it set is the offset every AirPlay speaker shares beside other protocols. */
  shared_airplay: z.boolean(),
});
export type CalibratedSpeaker = z.infer<typeof calibratedSpeaker>;

/** How speakers are kept in step. */
export const syncView = z.looseObject({
  /** How much later every AirPlay speaker plays in a group that mixes protocols, in ms. */
  mixed_airplay_ms: z.number(),
  last_calibration: z.nullable(
    z.looseObject({ at: z.number(), speakers: z.array(calibratedSpeaker) }),
  ),
  endpoints: z.array(
    z.looseObject({
      endpoint_id: z.string(),
      device_id: z.string(),
      name: z.string(),
      provider: z.string(),
      available: z.boolean(),
      sync_adjust_ms: z.number(),
      /** Where its adjustment comes from; null for the default. */
      source: z.nullable(z.looseObject({ layer: z.string(), name: z.optional(z.string()) })),
    }),
  ),
});
export type SyncView = z.infer<typeof syncView>;
