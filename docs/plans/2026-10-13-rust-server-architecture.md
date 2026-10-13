# A Rust music server: architecture and first implementation plan

A server that scans a local music library, plays it to AirPlay and Sendspin speakers in sync, and
speaks the subset of the Music Assistant (MA) WebSocket API that Bopsie uses, so Bopsie works
against it unchanged. It is designed so that more player protocols and more music sources can be
added without touching the core.

The facts this design rests on are in
[the ecosystem research](../research/rust-server-ecosystem.md) and
[the MA and Sendspin research](../research/music-assistant-and-sendspin.md). This document
decides; it does not re-argue. The working name of the project is left open; crate names below
are relative to the workspace.

## Goals and non-goals

Goals, in priority order:

1. Play a local library to AirPlay 1 and 2 speakers and to Sendspin players (Bopsie on a phone),
   alone or in synchronized groups, from queues with MA semantics.
2. Handle speaker quirks as data: a per-device record of what a speaker can do and how it
   deviates, which the rest of the system consults instead of special-casing.
3. Keep the player and music-source layers pluggable. Adding a protocol or a source is a new
   crate that implements a trait, plus registration.
4. Serve enough of the MA API that Bopsie needs no changes. MA compatibility is a means, not a
   goal: commands Bopsie does not send are out of scope until something needs them.
5. Pure, tested core logic: the group planner, quirk resolution, timeline arithmetic, Sendspin
   framing, buffer accounting and the queue state machine are side-effect-free modules.

Non-goals for the first scope: streaming services, Chromecast, DLNA, Sendspin Noise encryption
and pairing, LinkPlay native grouping (designed for, not built), a web UI, server administration
beyond a config file, and the parts of the MA API Bopsie does not use.

## Decisions carried over from the research

- **AirPlay goes through `cliairplay` as a subprocess**, one process per speaker plus one shared
  PTP daemon. It covers RAOP, AirPlay 2, both pairing styles and start-at-instant group sync
  behind a stdin, FIFO and `[STATUS]` line contract. GPL-3.0 does not reach a separate process.
- **The Sendspin server role is written in Rust**, ported from aiosendspin, on the legacy
  unencrypted wire first. Noise and the three pairing methods are a later milestone.
- **Decoding uses Symphonia**, with ffmpeg as an optional subprocess for what Symphonia cannot do
  gaplessly or at all (M4A edit lists, Opus, HE-AAC).
- **Library and search use rusqlite with bundled SQLite and FTS5.** No sqlx.
- **Discovery uses mdns-sd.** It owns a thread and talks over channels.
- **The API is axum with tokio-tungstenite**, MA's JSON envelopes verbatim.
- **Linux is the deployment target.** Being the PTP grandmaster needs UDP 319 and 320, which
  macOS does not give up. Development on macOS works for everything but PTP timing, which falls
  back to NTP.

## System shape

```
                         ┌──────────────────────────────────────────┐
  Bopsie, other clients  │ api        axum: /ws /api /info /setup    │
  ──────────────────────▶│            /imageproxy /sendspin (proxy)  │
                         └───────┬───────────────────────┬──────────┘
                                 │ commands               │ events
                         ┌───────▼───────────────────────▼──────────┐
                         │ core       ids, events, errors, timeline  │
                         │            event bus, config              │
                         └──┬──────────────┬──────────────┬─────────┘
                            │              │              │
                  ┌─────────▼───┐   ┌──────▼──────┐  ┌────▼───────────────────┐
                  │ library     │   │ queues      │  │ players                │
                  │ sqlite,     │   │ per-device  │  │ registry, linking,     │
                  │ scan, fts5  │   │ queue state │  │ quirks, planner, health│
                  └─────────┬───┘   └──────┬──────┘  └────┬───────────┬───────┘
                            │              │              │           │
                  ┌─────────▼───┐   ┌──────▼──────┐  ┌────▼─────┐ ┌───▼────────┐
                  │ providers/  │   │ audio       │  │ players/ │ │ players/   │
                  │ filesystem  │   │ decode,     │  │ airplay  │ │ sendspin   │
                  │             │   │ timeline,   │  │ (cli-    │ │ (server    │
                  │             │   │ fan-out     │  │ airplay) │ │ role)      │
                  └─────────────┘   └─────────────┘  └──────────┘ └────────────┘
```

One binary, one tokio runtime. Audio decode and fan-out run on dedicated threads with bounded
channels, never on the async executor. mdns-sd and rusqlite each have their own thread.

### Cargo workspace

| Crate | Owns | Depends on |
|---|---|---|
| `core` | Ids, media and player models, events, errors, the `Timeline`, config loading, the event bus | nothing internal |
| `audio` | Symphonia and ffmpeg decoding, resampling, the PCM ring buffer, per-output transformers (format, FLAC and Opus encode), the `StreamEngine` | `core` |
| `library` | SQLite schema and migrations, the scanner, FTS5 search, artwork cache, the `MusicProvider` trait and registry | `core` |
| `providers/filesystem` | The local filesystem `MusicProvider`: walk, tag with lofty, open files for `audio` | `core`, `library` |
| `players` | `PlayerProvider` and `Output` traits, device registry, endpoint linking, quirk rules, group planner, health, group sessions | `core`, `audio` |
| `players/airplay` | mDNS browse for `_raop` and `_airplay`, pairing flows, the `cliairplay` supervisor, PTP daemon lifecycle | `players` |
| `players/sendspin` | mDNS browse and advertise, WebSocket listener, the legacy wire, per-client state, buffer accounting, chunk scheduler | `players` |
| `queues` | Queue state machine, enqueue options, shuffle and repeat, the bridge from queue to `StreamEngine` | `core`, `library`, `players` |
| `api` | axum routes, MA envelopes, auth and JWT, scopes, the command table, event forwarding, image proxy, Sendspin proxy | everything above |
| `server` | The binary: wiring, startup, shutdown, provider registration | everything |

Traits live in `players` and `library`, not in the protocol crates, so a new protocol crate
depends inward only. `server` is the only crate that knows the concrete list of providers.

## Core concepts

### Devices, endpoints and outputs

MA's lesson is that one physical speaker often reaches the network over several protocols, and
the thing a user groups and names is the speaker, not a protocol. The model separates three
things:

- **Device.** What the user sees: a name, an icon, a volume, a queue. Has one or more endpoints.
  Exposed through the API as the MA `Player`.
- **Endpoint.** One protocol reachable at one address: an AirPlay 2 service, a Sendspin client,
  later a LinkPlay HTTP API. Carries its own capabilities and quirks. Discovered by a
  `PlayerProvider`. Hidden from the API unless `return_protocol_players` is set, matching MA.
- **Output.** A live session on one endpoint: a `cliairplay` process, a Sendspin connection
  that has been started. Consumes PCM from a `Timeline`, accepts commands, reports telemetry.

**Linking** decides which endpoints belong to the same device. The rule order is MA's: MAC, then
serial, then UUID, then IP. The AirPlay TXT record carries a MAC in `deviceid`; a Sendspin
`client/hello` from Bopsie carries a persistent `client_id` and `device_info`. A Bopsie phone is
a device with one Sendspin endpoint. A WiiM with AirPlay is, in the first scope, a device with one
AirPlay endpoint; when a LinkPlay provider lands, it gains a second endpoint under the same device
without any change to grouping code. Linking is a pure function over the endpoint table and is
re-run on every discovery change.

### Capabilities and quirks

Two per-endpoint records, both plain data, both visible in the API and the config file:

- **Capabilities** state what the endpoint can do: sample formats, whether it can join a
  synchronized group over this protocol, whether it can lead a native group, volume and mute
  support, seek, pairing requirement, maximum group size (the PTP peer cap is eight).
- **Quirks** state where the endpoint deviates from the protocol's promise, each with a cause.
  Known entries from the research:
  - `max_sample_format`: Sonos accepts 48 kHz 24-bit and plays silence.
  - `needs_metadata_before_audio`: Sonos withholds audio until metadata arrives.
  - `sync_adjust_ms`: a manual per-speaker offset added to the group anchor.
  - `min_lead_ms`: a floor on start lead above the protocol default.
  - `timing`: force NTP where PTP is unreliable (VLAN crossing, HomePod insisting on being
    grandmaster).
  - `protocol`: pin `raop`, `airplay2` or `airplay2-compat` instead of auto.
  - `pairing`: `transient`, `pin` or `password`, when detection from status flags is wrong.
  - `volume_curve`: linear versus the Sendspin loudness curve.
  - `disconnect_on_idle_s`: tear the session down after idle, for speakers that drop it anyway.

Resolution is layered, last wins: protocol defaults, then **quirk rules** matched on discovery
facts (manufacturer, model, firmware, TXT features and status flags, `device_info`), then a
per-endpoint override in the config file. Rules ship as a data file inside the `players` crate so
that adding a speaker is a rule, not a branch. Every resolved quirk records which layer set it,
so the API and logs can say why a speaker behaves as it does.

The rest of the system consumes quirks in exactly three places: the output when building its
session (format, protocol, pairing, timing), the planner when choosing a strategy and a lead,
and the API when reporting features. Nothing else may look at a model name.

### Health and telemetry

Speakers report almost nothing, so the server measures. Each output emits a `Telemetry` stream
of typed events, and `players` folds it into a per-endpoint `Health` record:

- AirPlay: `clock_ready` and stall transitions, `/feedback` timeouts, reconnects, process exits
  by error slug, and resend counts once `cliairplay` emits them as `[STATUS]` lines. That emit
  is a small upstream change the project should contribute.
- Sendspin: late chunks dropped, slow socket writes, buffer fill against capacity, changes in
  `min_buffer_ms` and `required_lead_time_ms`, reconnects, time-request cadence.
- Both: the server's own RTT and jitter from the protocol's keepalive.

Health is a small enum (`good`, `degraded`, `failing`) plus the raw counters. The planner reads
the enum. The API exposes the counters under the player's `extra_attributes` so Bopsie can show
them later without a schema change.

### The timeline

Every group session has one `Timeline`: a sample counter at the group's working rate, anchored
to the server's monotonic clock at the moment the session started. Everything else translates:

- Sendspin chunks carry the timeline instant converted to server monotonic microseconds.
- `cliairplay` wants a unix-epoch millisecond anchor for its first sample; the AirPlay output
  converts once at start, then streams PCM continuously, so later timing is implied.
- Seeks and track jumps are a new segment on the same timeline: the engine flushes outputs,
  moves the write head forward by the group lead, and resumes. Natural track transitions are
  gapless and never touch the outputs.

Group lead is computed by the planner as the maximum over members of their required lead, each
output's protocol minimum plus its quirk floor, plus the warm-up class MA uses: solo, warm group,
or cold group. Late joiners get `now + lead` and, for AirPlay, the ring-buffer replay that
`cliairplay` offers.

### The group planner

A pure function:

```
plan(members: &[DeviceSnapshot], current: Option<&Plan>, policy: &Policy) -> Plan
```

A `DeviceSnapshot` is a device's endpoints with their capabilities, quirks and health. A `Plan`
names one strategy for the group and, per member, which endpoint carries it:

1. **AirPlay direct.** One `cliairplay` per member on the shared PTP daemon. Needs every member to
   have an AirPlay endpoint; prefers it when all do.
2. **Sendspin direct.** One stream per member on one timeline, one encoder per distinct format.
   Needs every member to have a Sendspin endpoint.
3. **Native leader and followers.** For later: the leader's endpoint carries the followers'
   audio over the vendor's own multiroom, so the server streams only to the leader, chosen by
   health. Not built in the first scope, but the `Plan` type and the planner tests include it
   so the shape is settled.
4. **No common strategy.** The planner returns the largest subset it can serve and lists the
   members it dropped with a reason. The caller decides whether to proceed; the API surfaces
   the reason as an error detail.

Member order in the policy is MA's protocol priority: AirPlay, then Sendspin, then native. A
plan is recomputed on every membership, capability or health change. Switching strategy during
playback follows MA's `dev` behaviour: a change on join happens immediately with a flush and
re-anchor; a change on leave waits for the next fresh start. The planner has no I/O and is
tested with tables of snapshots.

## Audio path

```
MusicProvider ──open──▶ Decoder (Symphonia | ffmpeg) ──PCM f32──▶ Timeline ring
                                                                      │
                                              per-output Transformer ◀┘ (resample, dither,
                                              pack s16/s24, FLAC/Opus encode; one per
                                              distinct format, shared by outputs that match)
                                                                      │
                                                        Output::write(chunk @ instant)
```

- The engine decodes one track ahead so gapless joins are ready before they are due.
- The ring holds enough audio to cover the largest lead plus the Sendspin replay window.
- Transformers are keyed by output format, like aiosendspin's pool, so two Sendspin phones that
  both want 48 kHz FLAC share one encoder.
- Fill rate is capped at four times real time toward Sendspin clients, matching aiosendspin
  10.0.2, and never exceeds a client's `buffer_capacity` counted as header plus payload.
- Decoder choice is per file: Symphonia for FLAC, Vorbis, WAV, MP3, AIFF and ALAC in native
  containers; ffmpeg for `.m4a` AAC and ALAC, Opus and HE-AAC. If ffmpeg is absent those files
  still play through Symphonia, without gapless trimming, and the scanner flags them.

## Library

- **Schema.** Artists, albums, tracks, playlists, playlist items, images, provider mappings, and a
  scan journal keyed by path, size and mtime. Ids are integers; the API exposes
  `library://<type>/<id>` URIs and `"library"` as the provider, matching MA.
- **Scanner.** Walks the provider root, tags with lofty, extracts embedded art to a content-hashed
  cache, writes in batches inside one transaction per batch, and emits `media_item_added` and
  `media_item_updated`. Incremental by journal; a full rescan is a command.
- **Search.** FTS5 with the trigram tokenizer for substring matches and a unicode61 index with
  `remove_diacritics 2` for word matches, unioned and ranked. Both indexes are external-content
  tables over the same rows. Queries under three characters go to the unicode61 index only.
- **Providers.** `MusicProvider` has `scan`, `resolve(uri) -> MediaItem`, `open(track) ->
  AudioSource` and `search`. The library is the merge point: items from any provider land in
  the same tables with a provider mapping, so a future streaming provider adds rows, not
  schemas.

## Queues

One queue per device, keyed by the device id, as in MA. The queue state machine is pure: items,
current index, index in buffer, shuffle, repeat, enqueue options (`play`, `replace`, `next`,
`replace_next`, `add`), `play_index`, `move_item`, `delete_item`, `transfer`. It emits
`queue_updated`, `queue_items_updated` and `queue_time_updated` at the same points MA does.
When a device joins a group, its queue becomes inactive and the group leader's queue drives the
session, exactly as `player_queues/get_active_queue` reports it. The bridge from queue to engine
is thin: the queue says which track and position to play next, the engine reports elapsed time
and track end.

## The API

The `api` crate serves MA's envelopes and routes so that Bopsie's existing client works. First
scope is exactly what Bopsie sends and listens to.

| Area | Commands |
|---|---|
| Server | `GET /info`, `POST /setup`, `POST /auth/login`, `/ws`, `POST /api` |
| Auth | `auth/login`, `auth`, `auth/token/create`, `auth/logout`, `auth/me` |
| Library | `music/{artists,albums,tracks,playlists}/library_items`, `music/<type>/count`, `music/albums/get`, `music/albums/album_tracks`, `music/artists/get`, `music/artists/artist_albums`, `music/artists/artist_tracks`, `music/playlists/get`, `music/playlists/playlist_tracks` (streamed with `partial`), `music/search`, `music/item_by_uri` |
| Players | `players/all`, `players/get`, `players/cmd/volume_set`, `players/cmd/volume_mute`, `players/cmd/group`, `players/cmd/ungroup`, `players/cmd/group_volume` |
| Queues | `player_queues/get`, `player_queues/get_active_queue`, `player_queues/items`, `player_queues/play_media`, `player_queues/play_index`, `player_queues/play`, `pause`, `play_pause`, `stop`, `next`, `previous`, `seek`, `skip`, `shuffle`, `repeat`, `move_item`, `delete_item`, `transfer`, `clear` |
| Events | `player_added`, `player_updated`, `player_removed`, `queue_added`, `queue_updated`, `queue_items_updated`, `queue_time_updated`, `media_item_added`, `media_item_updated`, `media_item_deleted`, `music_sync_completed` |
| Images | `GET /imageproxy/<proxy_id>?size=&fmt=` with the fixed size set |
| Sendspin | `/sendspin` on the API port with the `auth` first frame, proxied to the Sendspin listener; the direct listener on 8927 |

Design points:

- **Server info** reports the schema version MA 2.10.6 reports, so clients that gate on the
  schema behave as they do against MA. The `server_version` string carries this project's own
  version. The subset is honest: unknown commands return error 12 with MA's wording.
- **Command table.** Each command is a struct implementing `Command` with a name, required scope,
  typed args (serde, snake_case, MA field names), and a handler. A build step emits
  `/api-docs/commands.json` from the table so the published list is the implemented list.
- **Auth.** Users and tokens in SQLite, JWT HS256 with `sub` and `jti`, the database as the source
  of truth for expiry and revocation, MA's session and long-lived lifetimes. `auth/login` failure
  is a success-shaped result, as MA does it.
- **Events** fan out from the core bus to every authenticated `/ws` connection. The `/sendspin`
  proxy marks the `client_id` from its auth frame as that session's own player.
- **Models** mirror `music-assistant-models` field by field for the types in scope. Fields the
  server has no value for are present and null, never omitted, so Bopsie's parsers hold.

## The two player providers in the first scope

### AirPlay

- **Discovery.** Browse `_raop._tcp` and `_airplay._tcp`, merge by MAC, read `features`,
  `statusFlags`, `pw` and `model` into capabilities and discovery facts for quirk rules.
- **Pairing.** Transient by default. When status flags call for PIN pairing, the device is
  listed as unavailable with a `pairing_required` reason, and a `players/cmd/pair` command runs
  `cliairplay --pair-setup` and stores the credentials against the endpoint. Password speakers
  take the password from config.
- **Supervisor.** One task per output owns the process, its stdin PCM pipe, its command FIFO
  and the `[STATUS]` parser. The parser is a pure module that turns lines into typed events and
  is tested against captured output. The supervisor restarts the process on exit with backoff
  and reports each exit as telemetry.
- **PTP daemon.** Started once when the first AirPlay 2 output starts and stopped when the last
  one ends. Outputs attach with `--ptp-shared`. On macOS, or when the ports cannot be bound, the
  provider marks capabilities as NTP-only and the planner lowers its expectations.
- **Group start.** The output exposes `prepare`, `anchor(instant)` and `confirm` so the group
  session can run MA's four-step start: spawn and wait for `connected`, feed and wait for
  `audio` and `clock_ready`, compute the anchor from the latest readiness plus lead, send, and
  re-anchor everyone if any ack lands later, up to four rounds.

### Sendspin

- **Listener.** WebSocket on 8927 at `/sendspin`, advertised as `_sendspin-server._tcp` with
  the `path` TXT record; browse `_sendspin._tcp` and dial clients that advertise.
- **Wire.** The legacy hello path first: `client/hello` with `player@v1_support`, `server/hello`
  with `active_roles`, `client/state` with both `available` and the older `state` key accepted,
  `client/time` answered with receive and transmit stamps taken at the transport, the nine-byte
  binary header for legacy clients and the thirteen-byte header reserved for encrypted ones.
  Framing and parsing are a pure module with golden tests from the research captures.
- **Clients.** Each connection is an endpoint; `device_info.product_name` in the MA-approved set
  makes it a standalone device with its own queue, anything else a protocol-only endpoint
  awaiting linking, as MA does.
- **Scheduling.** A per-group `PushStream` with send-ahead equal to the maximum over members of
  `min_buffer + encoder hold-back`, or `required_lead_time` when buffered, floored at 50 ms,
  plus output delay. Buffer accounting counts every chunk whose end has not passed. Late joiners
  are replayed from the encoded cache at `now + max(100 ms, send-ahead)`.
- **Groups.** Every client is in exactly one Sendspin group. `available: false` or
  `client/leave` moves it to a stopped solo group and it is never rejoined automatically.
- **Commands.** `server/command` for volume, mute and output delay; `stream/clear` for seeks and
  jumps, `stream/end` only when playback stops. Bopsie handles both, so the server does the
  spec-correct thing.

## Configuration and persistence

- One TOML config file: bind addresses, library roots, the optional ffmpeg path, per-endpoint
  overrides (name, enabled, quirks, password, sync adjust), and the planner policy.
- One SQLite database: library, users and tokens, discovered endpoints and their pairing
  credentials, per-device settings that the API can change. Migrations are embedded and run at
  startup.
- Logging through `tracing` with per-output spans so a speaker's whole session can be filtered.

## Testing strategy

- **Pure modules first.** Planner, linking, quirk resolution, timeline arithmetic, Sendspin
  framing, buffer accounting, the `[STATUS]` parser, the queue state machine and FTS query
  building are plain functions with table-driven tests.
- **A fake `cliairplay`.** A small program in the repository that speaks the stdin, FIFO and
  `[STATUS]` contract with configurable delays and failures. Integration tests run the real
  supervisor and group start against it, including the re-anchor rounds and a stalled clock.
- **A Sendspin test client** in Rust: hello, time sync with the time filter, state, chunk
  receipt with timestamp checks. Used to assert send-ahead, buffer caps, late join and clear.
- **API contract tests.** Golden JSON for every command in scope, derived from the research
  captures of MA 2.10.6, so a field rename breaks a test before it breaks Bopsie.
- **Bopsie end to end.** Bopsie's existing integration suite points at this server instead of
  the MA container. Passing it is the acceptance test for each milestone from M3 on.
- **Hardware.** A checklist for real speakers lives in the repository and is run by hand before
  a release: solo play, pair, group of two, join during playback, leave, seek, volume.

## Milestones

Each milestone ends with something that runs. Estimates are rough and assume one person.

**M0, skeleton (1 week).** Workspace, config, logging, SQLite with migrations, `/info`,
`/setup`, auth commands, the `/ws` envelope with the command table, error codes, and
`/api-docs/commands.json`. Bopsie can sign in and sees an empty library.

**M1, library (2 weeks).** Filesystem provider, scanner, artwork cache, FTS5 search, every
library command in scope, image proxy, media events, `music/sync`. Bopsie browses and searches.

**M2, audio engine (2 weeks).** Decoders, timeline, ring, transformers, a file output that
writes WAV for tests, the queue state machine wired to the engine. Gapless playback to a file is
sample-exact in tests.

**M3, Sendspin (3 weeks).** Listener, legacy wire, client state, time replies, push stream,
buffer accounting, groups, volume and mute, `stream/clear`, the `/sendspin` proxy. Bopsie plays
the library on a phone. Two phones play in sync.

**M4, AirPlay (3 weeks).** Discovery, transient and PIN pairing, the supervisor, solo playback,
the PTP daemon, the four-step group start, late join. A pair of AirPlay 2 speakers plays in
sync. First quirk rules land here, from the `cliairplay` design notes.

**M5, devices and planner (2 weeks).** Linking, capabilities, quirk resolution with the layered
sources, health from telemetry, the planner, mixed-protocol group handling with the drop list,
`group`, `ungroup`, `group_volume` and `transfer`. Bopsie's output sheet works for every
combination the planner can serve.

**M6, hardening (2 weeks).** Reconnect and restart paths, backoff, the upstream `cliairplay`
patch for resend counts, telemetry in `extra_attributes`, the hardware checklist, packaging as a
container and a systemd unit with `CAP_NET_BIND_SERVICE`.

After the first scope, in no fixed order: a LinkPlay provider with native leader and follower
grouping, Sendspin Noise encryption with the three pairing methods, a second music provider to
prove the trait, Chromecast, crossfade and volume normalization.

## Risks and open questions

- **PTP needs Linux and low ports.** Development on macOS cannot verify AirPlay 2 group timing.
  Budget for a Linux test box from M4 on.
- **`cliairplay` is young.** It was created in July and releases often. Pin a version, vendor
  the checksum, and keep the supervisor's parser tolerant of new `[STATUS]` keys.
- **Sendspin's legacy wire is on its way out.** Bopsie and this server both speak it today; the
  Noise milestone should start before any client drops legacy support.
- **LinkPlay is unresearched.** The native strategy is designed in but its HTTP API, group
  commands and failure modes are unknown. It is the first research task after the first scope.
- **MA compatibility drift.** Reporting MA's schema version while serving a subset is a bet that
  clients only probe for the commands they use. Bopsie does; other clients may not.
- **Quirk rules need hardware.** The initial table comes from documentation. Expect the first
  weeks with real speakers to add rules and to change which layer sets them.
