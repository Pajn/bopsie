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
| `bopsie-metadata` | `crates/metadata` | MusicBrainz, the Cover Art Archive and AcoustID lookups |
| `bopsie-players` | `crates/players` | Player traits, devices, linking, quirks, the group planner, health |
| `bopsie-players-airplay` | `crates/players/airplay` | AirPlay through `cliairplay` |
| `bopsie-players-sendspin` | `crates/players/sendspin` | The Sendspin server role |
| `bopsie-players-linkplay` | `crates/players/linkplay` | LinkPlay speakers, over their HTTP API |
| `bopsie-players-cast` | `crates/players/cast` | Cast devices, through the Sendspin receiver |
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
`RUST_LOG`, at `info` by default, with the tag reader's warnings about odd tags and Spotify
Connect's about network packets it cannot read left out.

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

LinkPlay speakers, such as WiiM's and Audio Pro's, are found by mDNS and controlled over their
HTTP API. A LinkPlay device cannot be told when to start, only what to play, so the server
serves it a stream it pulls from the API's port, and tells it to play that as long before the
audio is due as it took last time. Several LinkPlay speakers in a group are grouped by their
own multiroom: the server streams to the leader, and the leader carries the rest, at 48 kHz
16-bit at most. A group made in the vendor's app shows as it is. Playing to a LinkPlay speaker
switches it to its network input, as it would for any app, and leaves it there. Devices that
ask for a client certificate, as Audio Pro's do, are shown LinkPlay's; see
[`crates/players/linkplay/NOTICE`](crates/players/linkplay/NOTICE).

Cast devices, such as Chromecasts, Nest speakers and WiiM's, are found by mDNS and played
through Bopsie's build of the [Sendspin receiver](https://github.com/Sendspin/cast)
([`Pajn/cast`, branch `bopsie`](https://github.com/Pajn/cast/tree/bopsie)), a Cast app the
server launches on a device when it is played to. The receiver connects back to the Sendspin listener
on port 8927, so that port must be reachable from the devices, and plays in step with other
Sendspin players. A Cast app stops once no sender is connected to it, so the server keeps a
channel open to each device, and stops the receiver itself once it has been idle for five
minutes. Launching takes a few seconds, so a device that also plays over another protocol, as a
WiiM does over LinkPlay, plays over that one unless the receiver serves more of a group. Another
app cast to a device takes it; the server does not take it back until it is played to again.

A WiiM sounds a Cast app only while it sees media play, and only once it has played over
LinkPlay: Bopsie's receiver plays the music through Web Audio and, beside it, a silent track in
a Cast media session, and before playing a WiiM through the receiver the server plays it a
moment of silence over LinkPlay, which wakes its amplifier. Its standby, in the WiiM Home app,
must be off (Never): it counts only its own player, so it sleeps in the middle of a Cast
receiver's music.

A group plays over one protocol when that reaches all of it: AirPlay, Sendspin, or a LinkPlay
leader's own multiroom. A group of AirPlay speakers and Sendspin players, including Cast devices
through their receiver, plays each over its own: both protocols play the session's timeline on
the server's clock, so they keep in step, and AirPlay's group limit counts only its own
speakers. A speaker that sounds ahead of or behind the rest is moved by its `sync_adjust_ms`
quirk; see [`config.example.toml`](config.example.toml). Bopsie on Android sets it by ear: with
`admin/calibration/start` each speaker the group streams to plays a chirp in turn, on the group's
timeline, and `admin/calibration/apply` takes how late the phone heard each one and makes the
early speakers wait for the latest. Calibrating pauses the group. AirPlay speakers keep in step
with each other on PTP, so beside other protocols they move together: calibrating a group that
mixes them sets one offset all AirPlay speakers wait in such groups, and leaves their own
`sync_adjust_ms` alone. A group of AirPlay speakers alone plays without it. The web UI's Sync
page shows each speaker's adjustment, the AirPlay offset and what the last calibration set, and
changes any of them by hand (`admin/sync`, `admin/sync/update`).

A speaker's own controls act on the group it plays in, as the app would: pause pressed on
one speaker pauses all of them, and play, next, previous and its volume buttons work the same
way. AirPlay speakers send their controls to the server, which announces itself for them over
mDNS (`_dacp._tcp`) and listens on the first free port from 39831. LinkPlay devices tell the
server nothing, so it reads a press from their status: a pause or a play it did not ask for,
or a switch to another input, seen within a second or so. A device that paused itself is left
paused, so that its own play resumes the group. Sendspin clients that take the controller
role, such as Bopsie's player on Android, control their group too, and are told its volume,
repeat and shuffle. A speaker switched to another source leaves its group, which plays on.

### Spotify Connect

Every speaker shows in the Spotify app as a Spotify Connect device, named "Kitchen (Bopsie)" and
so on, so it is told apart from a speaker's own Spotify Connect. `name` under `[spotify]` sets the
pattern, where `{name}` is the speaker's name, and a speaker's Spotify name in the web UI wins over
it. Picking one plays Spotify to the group the speaker is in, through the server, in sync across AirPlay,
LinkPlay and Sendspin as anything else plays. The group's queue rests, paused, meanwhile, and
playing something from Bopsie on the group takes it back from Spotify. Spotify's play, pause,
skip and seek act on the group, its volume slider moves the group's volume within each speaker's
maximum, and the group's controls, including the speakers' own buttons, reach Spotify. Bopsie
shows what Spotify plays.

It needs Spotify Premium. The devices are announced by mDNS (`_spotify-connect._tcp`), each with
a small HTTP server of its own on a free port, and play through
[librespot](https://github.com/librespot-org/librespot). The credentials of whoever picked a
device are kept in the data directory under `spotify/`, so it reconnects after a restart. A
speaker is announced once it has been on the network for ten seconds, under the name it settles
on, and one that drops off keeps its device for fifteen minutes.
Spotify has been changing how its audio is protected, and librespot no longer plays for every
account; `cargo run -p bopsie-sources-spotify --example spotify-probe` announces one device that
logs what it is given, to check an account before relying on it. A speaker's settings in the web
UI, or `spotify_connect = false` in the config file, turn it off for that speaker; `[spotify]`
turns it off for all.

### Spotify in Bopsie

Spotify can also be searched, browsed and played from Bopsie, and the listener's saved Spotify
music synced into the library beside local files. The server signs in to Spotify twice, from the
Spotify card on the web UI's server page, because Spotify keeps these apart:

- **Playback and playlists.** "Sign in from the Spotify app" announces a "Bopsie sign-in" device;
  picking it in the Spotify app hands the server a login, kept in the data directory under
  `spotify/account/`. Tracks play through librespot as Ogg Vorbis at up to 320 kbps, fetched as
  they play and never stored.
- **Search and the saved library.** Spotify's Web API is reached through the listener's own
  developer app: create one at [developer.spotify.com](https://developer.spotify.com/dashboard)
  with `https://<server>/spotify/callback` as a redirect URI, give its client ID on the card, and
  log in with Spotify. Its refresh token is kept in the database. librespot's own access to the
  Web API is shared by every librespot app and is used up. A developer app in development mode
  serves at most five Spotify users, and its search returns ten results per type.

Once both are there, a sync brings in liked songs and saved albums from the Web API, and the
listener's playlists, with their artwork, from librespot and the Web API. A playlist's tracks are
not brought into the library: opening or playing it asks Spotify for them, so that a followed
playlist does not fill the library with albums nobody saved. It runs right after logging in, at every
start and every six hours. An album saved on Spotify and kept as files is one album with both
as sources (see below); an artist is one, with both. Search covers the library and Spotify, the library's results first, and
a Spotify result the library holds comes back as the library's; `library_only` or `providers` on
`music/search` narrows it, and Bopsie offers Both, Library or Spotify. Web API calls are paced, at
most one a second after a burst, and wait out Spotify's rate limit when it answers with one.

### One album from several sources

After each sync, albums from different sources that share a name, but for a version such as
"(Deluxe)" or "(2011 Remaster)", and an album artist, are compared track by track, as are
albums tied by a MusicBrainz release or a link to the same Spotify album:

- **The same release** becomes one album: each track that is on both is one track with both
  sources, and the album keeps edits made to either. Its files' tags win, and the Spotify sync
  only fills in what they lack. An album plays from its files while they hold every track it
  plays, and from Spotify throughout otherwise, so that it stays gapless and evenly loud.
  Songs saved without the rest of their album count as that album.
- **Another recording** stays apart: a remaster, a live album, or a remix differs from the
  original even where MusicBrainz calls the recordings the same.
- **Maybe the same**, such as a deluxe edition of an album, or a shorter album that may be
  another edition of it, is suggested. The web UI's library page lists suggestions to merge
  or keep apart; either decision holds at every later sync.

A track kept as more than one file on its album, as in the album's folder and as a copy in a
playlist's, is one track with each file as a source. The library page's Duplicate files lists
them with their paths, to delete the copies not wanted; the server never removes files.

An item's page in the web UI lists its sources, the one it plays from first at the top.
Separate takes one back out of a merged album, with the tracks only it holds, into an album
of its own that is not merged again. Suggestions also lists what was decided, Separate
included, and Reconsider forgets a decision: the albums are compared again at once, and a
release decision has the album looked up again.

### Metadata from MusicBrainz

In the background, a minute after the server starts and every quarter of an hour after, the
library's albums are looked up on [MusicBrainz](https://musicbrainz.org), those with files
first, each again every 90 days. An album is found by the release its files
are tagged with, by its barcode, or by a search scored as [beets](https://beets.io) scores
releases. A close match is taken; a near one, such as a release with a track the files lack,
is suggested on the web UI's library page, to use or reject. What is taken:

- **Ids:** the release, its release group and barcode, the tracks' recordings and ISRCs, and the
  artists', served as each item's `external_ids`. A release's link to a Spotify album ties
  the album to the one saved on Spotify, and the two merge.
- **Art:** an album without any gets its release group's front cover from the
  [Cover Art Archive](https://coverartarchive.org). Art its own files bring later replaces it.
- **Artist photos:** from Spotify, for artists Spotify's tracks credit and those MusicBrainz
  links to Spotify, served as the artist's `thumb` image and fetched from Spotify as shown.
  An artist Spotify has no photo of gets the one Wikidata, as MusicBrainz links it, has on
  [Wikimedia Commons](https://commons.wikimedia.org), at 500 px; its licence is the file's.
- **Backgrounds, logos and banners:** with `fanarttv_key` under `[metadata]` set to a personal
  key from [fanart.tv](https://fanart.tv/get-an-api-key/), artists MusicBrainz identifies get
  up to three backgrounds, a logo and a banner from it, served as `fanart`, `logo` and
  `banner` images beside the photo, for an app to style an artist's page with.
- **Genres:** the release group's three most voted, served as the album's and its tracks'
  `metadata.genres`. An artist's are the most common among its albums'.

MusicBrainz takes one request a second, and the lookups keep to that, so a large library takes
a while the first time; answers are kept for 30 days. Each request names Bopsie, and
`contact` under `[metadata]` adds an address MusicBrainz can reach whoever runs the server at,
as it asks of the software that calls it.

Files tagged too poorly to be found are found by their sound when `acoustid_key` under
`[metadata]` is set to an application key from [acoustid.org](https://acoustid.org/new-application)
and `fpcalc` ([Chromaprint](https://acoustid.org/chromaprint)) is on the path: each track of an
album the search left unmatched is fingerprinted and looked up on AcoustID, and the release
most of them are on is taken, or suggested when it holds some but not all of them.

The web UI's server page says how far the lookups are and how the last run went. An album's
page lists its ids, linked to MusicBrainz, and Refresh metadata looks it up again at once.

Files are never written to. `[metadata]` turns each of these off.

### Volume normalisation

Every track plays at the same loudness, -18 LUFS unless `target_lufs` under `[audio]` says
otherwise. The server applies the gain as it decodes, so every speaker in a group hears the
same, whatever it is played through. How loud a track is comes from:

- **Its tags:** ReplayGain track and album gains and peaks, in any tag format, or an Opus file's
  `R128_TRACK_GAIN` and `R128_ALBUM_GAIN`.
- **Measuring:** files whose tags do not hold both gains are measured in the background with
  EBU R128, an album at a time, after each sync. A folder where any file needs it is measured
  whole, so one album's gains come from one method. A file is measured again only when it
  changes. The web UI's server page says how many are left.
- **Spotify:** each track's own figures, which Spotify keeps in its files.

A track plays at its album's gain when shuffle is off and the track before or after it in the
queue is from the same album, so an album keeps the difference between its quiet and loud
songs; otherwise it plays at its own. A track is never raised past its peak, so a quiet track
does not clip; that holds back more tracks at a higher target. A track nothing is known of
plays as it is. The queue's current item says what it got in its `streamdetails`.

Spotify Connect plays through librespot's own normalisation, at the same target.
`normalisation = false` under `[audio]` turns it all off.

A new server has no users. Create the first, an admin, before signing in:

```sh
curl -X POST http://localhost:8095/setup -H 'Content-Type: application/json' \
  -d '{"username": "admin", "password": "at-least-8-characters"}'
```

The server's web UI, for managing it, is at `/` (`http://localhost:8095/`); sign in there as an
admin. It is the app in [`../web`](../web), built into the server binary from `web/dist`, so
build it first: `pnpm install` at the repository's root, then `pnpm --filter bopsie-web export`.
A server built without it serves a page saying so. Debug builds read `web/dist` from disk, so a
new export shows on reload; `dir` under `[web]` in the config file serves another directory.

### Settings, users and metadata

The web UI changes what the config file sets for a player, and more:

- **A player:** its name, whether it is enabled, and a maximum volume. The server holds the
  player at that maximum whatever turns it up: an app, the speaker's own remote, or a
  protocol's volume at connecting.
- **Each of its endpoints:** whether the player may play over that protocol, the speaker's
  password, PIN pairing, and quirks such as sync adjust. Quirks apply from the next play.

What is saved there is kept in the database and wins over the config file; Reset clears it,
and the config file's value, or the default, applies again. Each setting says which of those
it comes from. The config file is still read at startup, so it can keep settings for a server
set up by hand.

Admins also manage users (create, change, disable, delete, and see and revoke their sign-ins)
and edit library metadata: names, sort names, track and disc numbers, a track's album and
artists, an album's year and artists, and artwork. An edit is kept apart from what a scan finds
and applied again after every sync, so it outlasts a change to its file. Resetting a field
brings back what the scan found.

The app's integration suites run against this server in CI: `mobile/scripts/test-server.sh`
builds its image, starts it with the library they expect and creates the user (see the app's
README). To run them against a server started another way, give it that library:

```sh
cargo run -p bopsie-fixtures -- /tmp/bopsie-music   # then add the folder under [library]
cd ../mobile
TEST_SERVER_URL=http://localhost:8095 TEST_SERVER_USERNAME=admin \
  TEST_SERVER_PASSWORD=at-least-8-characters pnpm test:integration
```

### In a container

The image has the server, ffmpeg, `cliairplay` and `fpcalc`. CI publishes it from `main` as
`ghcr.io/<owner>/<repository>-server`, tagged `main` and with each commit's short hash. It
carries the web UI, so it builds from the repository's root. To build it yourself, there:

```sh
docker build -t bopsie-server -f server/Dockerfile .
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
