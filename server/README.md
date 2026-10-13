# Bopsie server

A music server in Rust. It scans a local library, plays it to AirPlay and Sendspin speakers in
sync, and serves the part of the Music Assistant API that Bopsie uses, so the app works against it
unchanged. The design and the milestones are in
[the server architecture plan](../docs/plans/2026-10-13-rust-server-architecture.md).

## Layout

| Crate | Directory | Owns |
|---|---|---|
| `bopsie-core` | `crates/core` | Ids, models, events, errors, the timeline, config, the event bus |
| `bopsie-audio` | `crates/audio` | Decoding, the PCM ring, transformers, the stream engine |
| `bopsie-library` | `crates/library` | SQLite, the scanner, search, artwork, the `MusicProvider` trait |
| `bopsie-providers-filesystem` | `crates/providers/filesystem` | The local filesystem music provider |
| `bopsie-players` | `crates/players` | Player traits, devices, linking, quirks, the group planner, health |
| `bopsie-players-airplay` | `crates/players/airplay` | AirPlay through `cliairplay` |
| `bopsie-players-sendspin` | `crates/players/sendspin` | The Sendspin server role |
| `bopsie-queues` | `crates/queues` | The queue state machine |
| `bopsie-api` | `crates/api` | The Music Assistant compatible API |
| `bopsie-server` | `crates/server` | The binary that wires it all together |
| `bopsie-fixtures` | `crates/fixtures` | The test library, for tests and for Bopsie's integration suites |

Traits live in `bopsie-players` and `bopsie-library`, so protocol and provider crates depend
inward only. `bopsie-server` is the only crate that knows the concrete list of providers.

## Running

```sh
cargo run -p bopsie-server -- --config config.toml
```

The config file is optional; [`config.example.toml`](config.example.toml) lists every key with
its default. The server listens on port 8095 and keeps its database in `data/`. Logging follows
`RUST_LOG`, at `info` by default.

Music comes from the folders under `[library]`. The server scans them at startup and on
`music/sync`; a rescan reads only files whose size or modification time changed. Album art is a
`cover`, `folder`, `front` or `album` image beside the tracks, or else the art embedded in them.
Playlists are `.m3u` and `.m3u8` files.

Audio decodes with Symphonia. When ffmpeg is installed, the server uses it for AAC and ALAC in
MP4 containers and for Opus, which Symphonia cannot play gaplessly or at all.

A new server has no users. Create the first, an admin, before signing in:

```sh
curl -X POST http://localhost:8095/setup -H 'Content-Type: application/json' \
  -d '{"username": "admin", "password": "at-least-8-characters"}'
```

To run Bopsie's integration suites against this server, give it the library they expect:

```sh
cargo run -p bopsie-fixtures -- /tmp/bopsie-music   # then add the folder under [library]
cd ../mobile
MA_URL=http://localhost:8095 MA_USERNAME=admin MA_PASSWORD=at-least-8-characters \
  pnpm test:integration
```

## Development

Requires a stable Rust toolchain with clippy and rustfmt, and ffmpeg for the tests that decode
through it (they are skipped without it). The Nix dev shell at the repository root provides
both. Every command below runs from `server/`.

```sh
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```
