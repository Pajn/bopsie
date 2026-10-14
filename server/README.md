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

Players connect over Sendspin. Apps that sign in to the API, like Bopsie, connect to
`/sendspin` on the API's port with their token, and their player is theirs alone; other
Sendspin clients connect directly on port 8927. The server plays queues to them alone or in sync
groups, as Music Assistant does.

AirPlay speakers on the network are found by mDNS and play through
[`cliairplay`](https://github.com/music-assistant/airplay-cli), the binary Music Assistant
uses, one process per speaker. It is GPL-3.0 and runs as a separate process, so it is
downloaded rather than built in: `scripts/fetch-cliairplay.sh` fetches the pinned release and
checks its checksum. Without it the server runs without AirPlay. AirPlay 2 groups share one PTP
clock, which needs UDP ports 319 and 320 (root, or `CAP_NET_BIND_SERVICE` on Linux). Speakers
that show a PIN, such as Apple TVs and HomePods, are paired with the `airplay/pair_start` and
`airplay/pair_finish` commands; speakers with a password take it from the config.

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

### In a container

The image has the server, ffmpeg and `cliairplay`. CI publishes it from `main` as
`ghcr.io/<owner>/<repository>-server`, tagged `main` and with each commit's short hash. To build
it yourself:

```sh
docker build -t bopsie-server .
docker run -d --name bopsie --network host \
  -v bopsie-data:/data -v /path/to/music:/music:ro bopsie-server
```

AirPlay discovery and streaming need the host's network, hence `--network host`. The image
keeps its data in the `/data` volume and plays the music mounted at `/music`; its config is
[`packaging/container.toml`](packaging/container.toml), and a file mounted over
`/etc/bopsie/config.toml` replaces it. It runs as an unprivileged user; `cliairplay` carries
`CAP_NET_BIND_SERVICE` for the PTP clock, which works as long as the container keeps that
capability, as Docker and Podman grant by default.

### As a service

On Linux with systemd, [`packaging/bopsie.service`](packaging/bopsie.service) runs the server
as a `bopsie` user with its data in `/var/lib/bopsie`, and gives it `CAP_NET_BIND_SERVICE` for
the PTP clock:

```sh
cargo build --release -p bopsie-server
sudo install -m 755 target/release/bopsie-server /usr/local/bin/
sudo scripts/fetch-cliairplay.sh /usr/local/bin/cliairplay
sudo useradd --system --home-dir /var/lib/bopsie bopsie
sudo install -d /etc/bopsie
sudo install -m 644 config.example.toml /etc/bopsie/config.toml   # then edit it
sudo install -m 644 packaging/bopsie.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now bopsie
```

In `/etc/bopsie/config.toml`, set `data_dir = "/var/lib/bopsie"`, the music folders, and
`binary = "/usr/local/bin/cliairplay"`. The service may read anywhere but writes only its data
directory.

Before a release, [`docs/hardware-checklist.md`](docs/hardware-checklist.md) is run by hand
against real speakers.

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
