# A Rust server in place of Music Assistant: the ecosystem

Research notes, 2026-10-13. Scope: what exists for a Rust server that scans a local library into
SQLite, plays to AirPlay and Sendspin speakers, and serves the subset of the Music Assistant (MA)
WebSocket API that Bopsie uses. Covers (1) AirPlay sending, (2) the Sendspin server role, (3) what
two common LinkPlay-family speakers support and how MA picks a protocol for a group, and (4) the
Rust crates for tags, decoding, search and discovery. Primary sources only: source code at pinned
commits, specs, crate metadata and manufacturer pages. Where a fact comes only from an issue
thread or a search summary, it says so. The client side of MA and Sendspin is in
[music-assistant-and-sendspin.md](music-assistant-and-sendspin.md) and is not repeated here.

## Versions this is based on

| Component | Version | Commit | Licence |
|---|---|---|---|
| MA server, stable | [2.10.6](https://github.com/music-assistant/server/releases/tag/2.10.6) | [`c248664`](https://github.com/music-assistant/server/tree/c24866405bd2068c25a60e1014e5ef8c3f6445bb) | Apache-2.0 |
| MA server, `dev` | 2.11 nightlies | [`4f1a86c`](https://github.com/music-assistant/server/tree/4f1a86cd558cded72995fac9edc926a2e7c955df) | Apache-2.0 |
| `cliairplay` (music-assistant/airplay-cli), pinned by 2.10.6 | v0.5.5 | [`d4d3c72`](https://github.com/music-assistant/airplay-cli/tree/d4d3c72c8ad6a2e3771d007c27c155d85ea0bafa) | GPL-3.0 (binary); per-file Apache-2.0 / GPL-2.0+ |
| `cliairplay`, pinned by `dev` | v0.5.6 | [`0974a31`](https://github.com/music-assistant/airplay-cli/tree/0974a3175913a8f19b47ccdd5ccb6fdf9f2f704a) | as above |
| libraop (submodule of airplay-cli) | — | [`81c2182`](https://github.com/philippe44/libraop/tree/81c2182649da8645ac2a58b78e9f370c79a4165b) | see §1.1 |
| OwnTone | [29.3](https://github.com/owntone/owntone-server/releases/tag/29.3) | [`d6fb3ed`](https://github.com/owntone/owntone-server/tree/d6fb3edf5831de38134ebd92fcf09a730ddd37aa) | GPL-2.0 |
| libairptp (in OwnTone) | 0.5 | [`7e2252e`](https://github.com/owntone/owntone-server/tree/d6fb3edf5831de38134ebd92fcf09a730ddd37aa/src/libairptp) | MIT |
| pair_ap | `master` | [`77b29bd`](https://github.com/ejurgensen/pair_ap/tree/77b29bd726b34466ecbbafad000faf52d928c255) | MIT |
| nqptp | 1.2.8 | [`c925f27`](https://github.com/mikebrady/nqptp/tree/c925f27c1fd12e4033ac477e5a405969b0b0260b) | GPL-2.0 |
| shairport-sync | 5.5.2 | [`7bad231`](https://github.com/mikebrady/shairport-sync/tree/7bad231c18368dbd26f298577f6210e36e4b0797) | per file |
| Sendspin spec | 1.0.0-rc1; `main` | [`671a34d`](https://github.com/Sendspin/spec/tree/671a34d408581fc25ea56b3528a4a3f13e3be901); [`ee8aad9`](https://github.com/Sendspin/spec/tree/ee8aad9660fc57345e726a888486f47f66a41f97) | Community Spec Licence |
| aiosendspin | [10.0.2](https://github.com/Sendspin/aiosendspin/tree/10.0.2) (2026-10-13) | [`d024f53`](https://github.com/Sendspin/aiosendspin/tree/d024f53895cdb6d13d804c8343ecdf3659dfbe62) | Apache-2.0 |
| sendspin-server-cpp | no tags (created 2026-10-13) | [`f077a80`](https://github.com/Sendspin/sendspin-server-cpp/tree/f077a805b79129c30465b6f37fb5c318e044fe66) | Apache-2.0 |
| sendspin-go | v1.8.2 | [`c61a405`](https://github.com/Sendspin/sendspin-go/tree/c61a405) | Apache-2.0 |
| sendspin-rs (crate `sendspin`) | 0.3.7 | [`0cae6bb`](https://github.com/Sendspin/sendspin-rs/tree/0cae6bbf5e794f87dfbc986dac2e812fefded12c) | MIT OR Apache-2.0 |
| lofty | 0.25.4 (2026-09-20) | [`f88b02c`](https://github.com/Serial-ATA/lofty-rs/tree/f88b02cfa787debffff99c341b54c3a77b83b832) | MIT OR Apache-2.0 |
| Symphonia | 0.6.1 (2026-08-13) | [`ee35874`](https://github.com/pdeljanov/Symphonia/tree/ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee) | MPL-2.0 |
| rusqlite / libsqlite3-sys | 0.40.2 / 0.38.2 (bundles SQLite 3.53.2) | [`0f491a0`](https://github.com/rusqlite/rusqlite/tree/0f491a0e19d7a956031ea4ce354e085680766b28) | MIT |
| mdns-sd | 0.21.5 (2026-10-05) | [`a25148c`](https://github.com/keepsimple1/mdns-sd/tree/a25148c4d7adea836316dac2eca6c5bbdd79aa15) | Apache-2.0 OR MIT |

Link prefixes used below:

- `S` = `https://github.com/music-assistant/server/blob/c24866405bd2068c25a60e1014e5ef8c3f6445bb/music_assistant/`
- `D` = `https://github.com/music-assistant/server/blob/4f1a86cd558cded72995fac9edc926a2e7c955df/music_assistant/`
- `C` = `https://github.com/music-assistant/airplay-cli/blob/d4d3c72c8ad6a2e3771d007c27c155d85ea0bafa/`
- `OT` = `https://github.com/owntone/owntone-server/blob/d6fb3edf5831de38134ebd92fcf09a730ddd37aa/`
- `SP` = `https://github.com/Sendspin/spec/blob/ee8aad9660fc57345e726a888486f47f66a41f97/`
- `AIO` = `https://github.com/Sendspin/aiosendspin/blob/d024f53895cdb6d13d804c8343ecdf3659dfbe62/aiosendspin/`

## Part 1 — AirPlay sending

### 1.1 What MA ships: one binary, `cliairplay`

MA no longer uses `cliraop` or the OwnTone-derived `cliap2`. Since server PR
[#4879](https://github.com/music-assistant/server/pull/4879) (merged 2026-07-21), a single binary,
`cliairplay` from [music-assistant/airplay-cli](https://github.com/music-assistant/airplay-cli),
sends both RAOP (AirPlay 1) and AirPlay 2, with its own PTP engine and no nqptp. The earlier AP2
helper ([music-assistant/cliairplay](https://github.com/music-assistant/cliairplay), GPL-2.0,
"Based on owntones") is archived. The new README says OwnTone was "Referenced … no source was
copied" (`C/README.md`, "Credits").

- **Licence.** The repo and the released binaries are GPL-3.0 (`C/LICENSE`). Files under `src/`
  are Apache-2.0 per file, except `src/cliairplay.c`, which derives from libraop's `cliraop.c` and
  is GPL-2.0-or-later. Static builds bundle OpenSSL 1.1.1u (`C/THIRD_PARTY_NOTICES.md`). libraop
  has no LICENSE file at the submodule pin (GitHub reports NOASSERTION); a later upstream HEAD
  ([`70dffcd`](https://github.com/philippe44/libraop/tree/70dffcd1b48c540c5d7ee063c54d6473ff86cbbb))
  carries an MIT `LICENSE.txt`.
- **Shipping.** MA's Dockerfile downloads `cliairplay-linux-{x86_64|aarch64}` from the release,
  checks `SHA256SUMS` and runs `--check`. Builds exist for Linux and macOS on x86_64 and arm64
  (`S/providers/airplay/helpers.py` `get_cli_binary()`, L301). airplay-cli CI opens a pin-update
  PR against server `dev` (`C/README.md`, "Platforms and CI").
- **Maintenance.** Repo created 2026-07-16; latest release v0.5.7 on 2026-10-13.

### 1.2 How MA drives it

One process per player, spawned by `AirPlayStream.connect()`
(`S/providers/airplay/stream.py` L301). Arguments are built in `_build_cli_args()` (L1019): protocol
(`auto|raop|airplay2|airplay2-compat`), DACP and active-remote IDs, a command FIFO path, sample
rate and bit depth, timing (`ptp|ntp`), port and TXT records from mDNS, stored credentials
(`--auth`, `--secret`), `--password`, `--latency`, `--ptp-shared`, and the device IP.

- **Audio in.** One persistent PCM stream on stdin for the life of the process: s16le 44.1 kHz,
  or s32le at 44.1 or 48 kHz for 24-bit. An ffmpeg per seek writes into that fd
  (`S/providers/airplay/stream_session.py` `_start_player_ffmpeg`, L1396). Seeks and track changes
  are `ACTION=FLUSH`, refill, then a new START.
- **Control in.** Newline-terminated `KEY=VALUE` lines on the FIFO (`_write_cli_command`, L1799):
  `START_UNIX_MS=<ms>` + `ACTION=START`, `ACTION=FLUSH|STOP|PLAY|PAUSE|DISCONNECT|…`,
  `VOLUME=<0-100>`, metadata (`TITLE=…` … `ACTION=SENDMETA`), `PROGRESS=<s>`, `ARTWORK=<path>`.
  The contract is in `C/README.md` "Runtime commands" and `C/DESIGN.md` §12.
- **State out.** `[STATUS]` lines on stderr and stdout: `connected`, `playing elapsed_ms=`,
  `started requested_unix_ms= at_unix_ms=`, `flushed`, `audio buffered_ms=`, `clock_ready`,
  `latency lead_ms= device_min_frames= …`, `REANCHOR`, `error code=<slug>`, and `[EVENT] remote
  command=` for DACP remote commands (`stream.py` `_handle_status_line` L1505, `_stdout_reader`
  L1195).
- **Protocol choice.** With `--protocol auto` the binary resolves the route from the TXT features
  (`C/DESIGN.md` §2): bit 38 or 48 means AirPlay 2; bit 41 (SupportsPTP) selects PTP over NTP;
  bit 40 selects the buffered stream type. MA mirrors the test in `supports_airplay2()`
  (`helpers.py` L128).

### 1.3 Synchronised start across several speakers

The binary's contract: "the first sample is audible exactly at that unix-epoch instant", with a
minimum lead of 250 ms (200 ms on RAOP); instants it can't meet are moved later
(`C/DESIGN.md` §6). MA's group start (`AirPlayStreamSession.start()`, `stream_session.py` L168):

1. Spawn every member, wait for `[STATUS] connected` from each.
2. Feed audio, wait for `[STATUS] audio` from each (5 s budget), then up to 2.5 s for `clock_ready`.
3. Anchor = now + lead (400 ms solo, 500 ms warm group, 2500 ms cold group), raised to at least
   the latest `ready_at` + 500 ms (`_anchor_start_unix_ms`, L1154).
4. Send each member anchor + its manual `sync_adjust`. If any ack reports a later instant,
   re-START everyone at the latest, up to 4 rounds (`_start_members`, L1305).

Every member gets its own process and stream; no speaker relays to another. Native AP2 members
share one PTP grandmaster: the provider spawns `cliairplay --ptp-daemon` once
(`S/providers/airplay/provider.py` L82, L825) and streams attach with `--ptp-shared` via POSIX shm
`/cliairplay-ptp` and UDP `127.0.0.1:9010`. The PTP engine caps timing peers at 8
(`C/src/ap2_ptp.c` L101, `PTP_MAX_PEERS`). Late joiners get a ring buffer of recent PCM plus
`START_JOIN=1` with 2500 ms minimum headroom.

OwnTone does the same thing differently: devices of the same format and timing mode share one
"master session", one ALAC encode and one RTP stream sent to all of them, with one time anchor in
every sync packet (`OT/src/outputs/airplay.c` `master_session_make` L1148, `packets_sync_send`
L2158). Speakers are never told about each other (`SETPEERS` lists only the device and OwnTone).

### 1.4 Pairing

- **Transient (most non-Apple AirPlay 2 speakers).** HomeKit pair-setup with the fixed PIN 3939,
  steps M1–M4 only, nothing stored (`C/DESIGN.md` §3; OwnTone `PAIR_CLIENT_HOMEKIT_TRANSIENT` in
  [`pair.h`](https://github.com/ejurgensen/pair_ap/blob/77b29bd726b34466ecbbafad000faf52d928c255/pair.h)).
- **PIN (Apple TV, HomePod).** Chosen when `statusFlags` has 0x8 or 0x200
  (`S/providers/airplay/player.py` `_requires_pin_pairing`, L1072). MA runs
  `cliairplay --pair-setup`, which POSTs `/pair-pin-start`, reads the PIN on stdin, runs SRP-6a and
  prints 96 bytes of credentials (`S/providers/airplay/pairing.py` L197; `C/src/ap2_hap.c`
  ~L1455). Later sessions pass them as `--auth` and do pair-verify. The DACP ID must match between
  pairing and streaming.
- **Password.** Detected from flags bit 0x80 or `_raop pw=true` (`player.py` L265). RAOP uses
  RTSP Digest; AP2 tries the password as the SRP secret, then falls back (`C/DESIGN.md` §3a).
- **Legacy RAOP Apple TV.** Three POSTs to `/pair-setup-pin` (`pairing.py` L436).

### 1.5 Timing

- **The sender is the PTP master.** `cliairplay`'s `ap2_ptp.c` is an in-process gPTP-style
  grandmaster: priority 128, clockClass 6, unicast Sync every 125 ms and Announce every 1 s on
  UDP 319/320, and it keeps the role even when a receiver announces a competing clock
  (`C/DESIGN.md` §4). The exception is a standalone HomePod that insists on being grandmaster,
  which it follows (§5). OwnTone's libairptp is the same design: "designed to always become
  master clock, which a sender like OwnTone needs, while nqptp wants to become slave"
  (`OT/src/libairptp/README.md`; `ptp_msg_handle.c` `announce_handle` L577).
- **nqptp is receiver-side.** "It is not a PTP clock" ([nqptp README](https://github.com/mikebrady/nqptp/blob/c925f27c1fd12e4033ac477e5a405969b0b0260b/README.md));
  shairport-sync uses it to follow whoever is master.
- **NTP fallback.** For RAOP, `--timing ntp`, or when ports 319/320 can't be bound. Binding them
  needs root or `CAP_NET_BIND_SERVICE` on Linux; on macOS the system already holds them
  ([shairport-sync AIRPLAY2.md](https://github.com/mikebrady/shairport-sync/blob/7bad231c18368dbd26f298577f6210e36e4b0797/AIRPLAY2.md#L72)).
- **Readiness.** `cliairplay` tracks each receiver's Delay_Req probes and marks it `stalled` after
  5 s without them (`C/src/ap2_client.c` L118, `AP2_CLOCK_STALL_MS`).

### 1.6 What a sender can observe per speaker

Speakers report very little. Neither codebase has a receiver reporting buffer fill, sync error or
packet loss to the sender. What exists:

- **Resend requests** on the control port (8 bytes, `80 D5 … <first seq> <count>`). `cliairplay`
  keeps a 512-slot ring of sent packets (about 4.1 s) and counts requested, resent and expired
  packets, but logs them only as `LOG_DEBUG` text every 5 s, not as a `[STATUS]` line
  (`C/src/ap2_client.c` L159–1095). MA passes `--debug` only when its own logging is at DEBUG, so
  by default the counts are never emitted. libraop counts RAOP resends in `p->retransmit` and logs
  them in an INFO line about once a minute (`raop_client.c` ~L590). OwnTone keeps no counter
  (`OT/src/outputs/airplay.c` `control_svc_cb` L2348).
- **PTP probe traffic** (Delay_Req) — the basis of `clock_ready` and `stalled`.
- **`POST /feedback`** keepalive. `cliairplay` sends it every 2 s; three consecutive timeouts end
  the control channel (`C/DESIGN.md` §10).
- **`GET /info`** `statusFlags` and features; the SETUP replies' ports and buffer size.
- **Latency.** `cliairplay` emits `[STATUS] latency` with the device's min and max latency; the
  receiver's reported render latency is informational only (`C/DESIGN.md` §6).

### 1.7 Rust AirPlay senders

None is usable today. Searches of crates.io for airplay, raop, airplay2 and homekit:

- **`airplay-rs`** 0.0.1 (2026-08-07, only release), AGPL-3.0-only, 42 downloads; source repo
  returns 404. Its `network_clock.rs` says "only NTP supported for now".
- **[lmcgartland/airplay2-rs](https://github.com/lmcgartland/airplay2-rs)** (GitHub only,
  [`f824007`](https://github.com/lmcgartland/airplay2-rs/tree/f824007798f71605307384829800273a4dbbcee2),
  GPL-3.0): RAOP and AP2, NTP and PTP, a multi-room test. Its README says "Alpha" and "many
  modules still contain `todo!()`".
- **[statime](https://github.com/pendulum-project/statime)** 0.4.0 (Apache-2.0 OR MIT) is a
  general IEEE 1588 implementation using multicast; there is no AirPlay-style unicast peer mode,
  so it is not a drop-in AirPlay master (inference from the source).
- The receiver crates (`shairplay`, `rairplay`, `openairplay2`) don't help a sender.

## Part 2 — The Sendspin server role

### 2.1 Implementations

| Implementation | Server? | Wire | Status |
|---|---|---|---|
| [aiosendspin](https://github.com/Sendspin/aiosendspin) (Python) | yes, complete | Noise and legacy | the reference: the [SDK page](https://github.com/Sendspin/sendspin-audio.com/blob/1a692c8/src/build/sdks.md) calls it "whatever it does is what clients in the field expect" |
| [sendspin-server-cpp](https://github.com/Sendspin/sendspin-server-cpp) | yes | Noise, all pairing methods | README: "Early development"; created 2026-10-13 |
| [sendspin-go](https://github.com/Sendspin/sendspin-go) / sendspin-go-server | yes | legacy only (no `client/init`/`server/activate`) | README announces a "hard cutover" to Noise |
| [sendspin-rs](https://github.com/Sendspin/sendspin-rs) | client only on `main` | legacy | crate README "THIS IS A WIP" |

The Rust server work is draft [PR #87](https://github.com/Sendspin/sendspin-rs/pull/87) (head
`d1a19cc`, last updated 2026-08-13): listener, mDNS, a `Group` with a fixed 250 ms send-ahead, PCM
only, one format per group, no per-client lead, no late-join replay, no buffer accounting, no
Noise (`src/server/mod.rs`, `group.rs` on that branch). It does contain a Rust port of the time
filter (`src/sync/clock.rs` on `main`). The [conformance suite](https://github.com/Sendspin/conformance/tree/48f3413)
has real server adapters only for aiosendspin and sendspin-go.

### 2.2 What the spec requires of a server

- **Clock.** Server monotonic µs. `server_received` is taken when the last byte reaches the
  transport (`SP/messaging.md`, "Receive timestamps", added by #298 after rc1);
  `server_transmitted` and `send_ahead` are stamped immediately before encryption, never at
  enqueue time. The server does not run the time filter.
- **Formats.** Servers MUST support `flac` and `pcm`; `opus` is optional. Encoding is per client
  ("allowing heterogeneous device capabilities within the same group"); a format change is an
  in-place `stream/start` on a continuous timeline (`SP/roles/player/v1.md`).
- **Sending.** Chunks 15–150 ms. The group's send-ahead is the maximum over its members,
  recomputed on join, leave and timing updates. Never exceed a player's `buffer_capacity`
  (count the 13-byte header plus payload of every chunk in flight or not yet played). Late joiners
  get only future timestamps. Seeks and track jumps use `stream/clear`, never `stream/end`.
- **Groups.** Every client is in exactly one group; grouping is entirely server-side, with no wire
  message to form one. `group/update` carries the full state. On `available:false` or
  `client/leave`, move the client to a stopped solo group and never rejoin it automatically
  (`SP/messaging.md`, "External Source Handling").
- **Controller.** Group volume uses the spec's delta, clamp and redistribute algorithm; reported
  group volume is the average (`SP/roles/controller/v1.md`).
- **Discovery.** A server MUST both browse `_sendspin._tcp` and dial, and advertise
  `_sendspin-server._tcp` (port 8927, TXT `path`) (`SP/connection.md`).
- **Encryption and pairing (1.0).** The server is the Noise KKpsk2 initiator, supports both
  cipher suites, keeps a persistent Curve25519 identity, and MUST implement all three pairing
  methods: PSK, dynamic code and static code (CPace) (`SP/pairing.md`). It must not play to an
  unpaired client without operator approval.

### 2.3 What clients report, and what a server can measure

The spec has no stats, buffer-level, underrun or RTT message. `client/state.player` carries
`volume`, `muted`, `output_delay_ms`, `required_lead_time_ms`, `min_buffer_ms`,
`supported_commands` and `format` (`SP/roles/player/v1.md`). `min_buffer_ms` is defined as the
upper tail of the player's measured arrival delay, so a rise in it reflects network jitter.

aiosendspin measures, server-side:

- per-player `BufferTracker`: buffered bytes and duration, horizon, capacity (`AIO/server/audio.py`);
- send gap, timestamp gap and buffer ms (min/avg/max), logged every 5 s at DEBUG only
  (`AIO/server/connection.py` ~L3390–3480);
- late chunks dropped before sending, with `late_by_us`, `queue_age_ms` and buffer state, at
  WARNING (`_check_late_binary`, ~L3082);
- slow socket writes (logged at ≥ 50 ms, counted at ≥ 500 ms);
- events for volume, output delay, required lead time and min buffer changes
  (`AIO/server/roles/player/events.py`).

It does not compute RTT or clock offset per client. sendspin-server-cpp's example CLI does keep
p50/p99 of time-reply hold and send-ahead per connection (`examples/server_cli/timing_stats.h`).

### 2.4 How aiosendspin implements it (10.0.2)

- `RawMonotonicClock` uses `CLOCK_MONOTONIC_RAW` on Linux to keep NTP slew out of client filters
  (`AIO/clock.py`).
- One `PushStream` per group; one encoder per distinct output format, shared by every member that
  needs it (`TransformerPool`, keyed by `TransformKey`, in `AIO/server/push_stream.py` and
  `audio_transformers.py`).
- Per-role send-ahead = max(min buffer + encoder hold-back, required lead if buffered, 50 ms) +
  output delay; the group takes the maximum.
- Buffer fill is capped at 4× real time (`MAX_FILL_RATE`, new in 10.0.2: "Line-rate bursts cause
  packet loss on weak WiFi links").
- Late joiners are replayed from the encoded cache at ≥ now + max(100 ms, send-ahead), or
  re-encoded from a PCM cache (`push_stream.py` `_do_role_join`, L2143–2275).

### 2.5 "Sendspin over AirPlay" is an MA bridge

The spec has nothing on relaying (a bridge-role proposal was closed as out of scope, per
[spec issue #76](https://github.com/Sendspin/spec/issues/76), issue thread only). MA defines its own
role, `player@_bridge` (`S/providers/sendspin/bridge_role.py`, fixed 44.1 kHz/16-bit stereo), and
registers each AirPlay speaker with a MAC address as a Sendspin client. Audio flows from the
Sendspin PushStream into the `cliairplay` process (`S/providers/airplay/sendspin_bridge.py`);
Chromecast has an equivalent. Sendspin's group timing drives the AirPlay output, but no Sendspin
traffic reaches the speaker.

## Part 3 — Two LinkPlay-family speakers, and how MA picks a group's protocol

### 3.1 WiiM Amp Pro

WiiM's [specification table](https://faq.wiimhome.com/en/support/solutions/articles/72000616467-technical-specification-of-the-wiim-amp-and-wiim-amp-pro)
(modified 2025-04-30) lists, for the Amp / Amp Pro: AirPlay 2 `Y` / `-`; Google Cast `Y` / `Y`;
DLNA, Roon Ready, Spotify and TIDAL Connect, and grouping with WiiM devices `Y` / `Y`. **The Amp
Pro has no AirPlay.** The [firmware release notes](https://faq.wiimhome.com/en/support/solutions/articles/72000635680-wiim-amp-pro-firmware-release-note)
(newest 5.2.830031, 2026-09-21) don't mention Sendspin or AirPlay.

### 3.2 Audio Pro A28

The [A28 W page](https://audiopro.com/products/a28-w) lists "Multiroom: Audio Pro, Airplay (1 and
2), Google Cast" and Wi-Fi 2.4 and 5 GHz, no Ethernet; the original A28's
[page](https://uk.audiopro.com/products/a28-multi-room-stereo-speaker-set) also lists AirPlay 2 and
Google Cast. Audio Pro's [FAQ](https://audiopro.com/blogs/faq/is-the-audio-pro-w-app-the-same-thing-as-the-wiim-home-app)
says its app "is based on Linkplay technology, and Linkplay is the parent company of WiiM".

### 3.3 Native Sendspin

No WiiM, LinkPlay or Audio Pro source mentions a Sendspin client. [sendspin-audio.com](https://www.sendspin-audio.com/)
names only the Home Assistant Voice PE as shipping with Sendspin. MA's
[Sendspin docs](https://github.com/music-assistant/music-assistant.io/blob/71f9cac8c37a9f583feda0faf4e300837fa0ca4e/src/content/docs/player-support/sendspin.md)
don't list WiiM or LinkPlay. A WiiM shown as "Sendspin (over AirPlay)" in an
[MA support issue](https://github.com/music-assistant/support/issues/6685) is MA's bridge (§2.5).

### 3.4 How MA chooses and switches a group's protocol

Logic is in `D/controllers/players/protocol_linking.py` (`ProtocolLinkingMixin`), described in
`D/controllers/players/README.md`.

- **Linking.** Generic endpoints (non-Apple AirPlay, non-Google Cast, DLNA) are hidden
  `PROTOCOL` players attached to a native vendor player as `output_protocols`, matched by MAC,
  serial, UUID, then IP (`_identifiers_match`, D L1866). Priority: AirPlay 10, Squeezelite 20,
  Chromecast 30, Sendspin 40, DLNA 50, native 0 (`D/constants.py` L1001).
- **Grouping, per joining member** (`_select_grouping_for_member`, D L2741): (1) join the parent's
  live native session if compatible; (2) native grouping if the child sets
  `prefer_native_grouping`; (3) the child's preferred output protocol if the parent has it;
  (4) the parent's active protocol if the child has it; (5) native grouping; (6) the first common
  protocol with `SET_MEMBERS`, in the parent's priority order; (7) otherwise no grouping.
  Chromecast has no `SET_MEMBERS` in MA, so it is never a shared group protocol.
- **WiiM and Audio Pro devices** use `WiimPlayer` with native `PLAY_MEDIA` and `SET_MEMBERS`;
  `prefer_native_grouping` is False. Native multiroom (`D/providers/wiim/grouping.py`,
  `NativeGroupCoordinator`) has the leader forward audio to followers.
- **Switching on join.** When a joining member shares no protocol with the group's current one,
  step 6 picks a common one, `_activate_group_output_protocol` (D L3125) stops the old protocol,
  resumes on the new one and re-adds members; members without it are dropped with a warning.
- **Switching back on leave.** Not in 2.10.6. On `dev`, `_regroup_off_unneeded_protocol` (D L2116)
  regroups the members as a fresh join would, but only when playback next starts fresh
  (`D/controllers/players/controller.py` L4531), never during playback
  ([PR #6675](https://github.com/music-assistant/server/pull/6675), merged 2026-10-04).
- **Settings.** Per player, "Preferred Output Protocol" (`native`, `auto` or a specific protocol)
  and per-protocol enable toggles. `prefer_native_grouping`, `default_output_protocol_domain` and
  the priority table are fixed in code.

## Part 4 — Rust building blocks

### 4.1 Tags: lofty

lofty 0.25.4, actively maintained (commit 2026-10-10), MSRV 1.89. Reads and writes ID3v1/v2, APE,
Vorbis comments, MP4 ilst, RIFF INFO and AIFF text chunks across AAC, AIFF, APE, FLAC, MP3, MP4,
MPC, Opus, Vorbis, Speex, WAV and WavPack ([README](https://github.com/Serial-ATA/lofty-rs/blob/main/README.md)).
Symphonia also reads tags but can't write them.

### 4.2 Decoding: Symphonia, with ffmpeg as fallback

Symphonia 0.6 (MPL-2.0) is pure Rust; decoders are "generally +/-15% the performance of FFMpeg"
([README](https://github.com/pdeljanov/Symphonia/blob/main/README.md)).

| Format | Status | Gapless | On by default |
|---|---|---|---|
| FLAC, Vorbis, PCM, WAV | Excellent | Yes | Yes |
| OGG | Great | Yes | Yes |
| MP3 | Excellent | Yes | No (`mp3`) |
| AIFF, ALAC | Great | Yes | No |
| AAC-LC, ISO/MP4 | Great | **No** | No |
| HE-AAC, Opus | **not implemented** | – | – |

- **Gapless** is a decoder option on by default in 0.6 (`AudioDecoderOptions::gapless`,
  `symphonia-core/src/codecs/audio.rs`). MP3 reads LAME delay and padding
  (`symphonia-bundle-mp3/src/demuxer.rs` ~L433).
- **M4A** edit lists are parsed but not applied, and iTunSMPB is never read, so AAC and ALAC in
  `.m4a` aren't trimmed.
- **Opus.** `symphonia-codec-opus` is an empty placeholder; the README points to the third-party
  `symphonia-adapter-libopus`.
- **ffmpeg** applies both MP4 edit lists and iTunSMPB
  ([`mov.c`](https://github.com/FFmpeg/FFmpeg/blob/master/libavformat/mov.c) ~L5836) and LAME
  padding (`mp3dec.c` ~L265). It is LGPL-2.1+, or GPL if optional parts are built in
  ([legal](https://ffmpeg.org/legal.html)); running it as a subprocess avoids linking.
- **Encoders and resampling.** `opus` 0.4.0 (libopus bindings, needs cmake), `ogg` 0.9.2 for
  muxing, `flacenc` 0.5.1 (pure Rust), `rubato` 5.0.1 (real-time-safe resampler).

### 4.3 Search: rusqlite with FTS5

rusqlite's `bundled` feature builds SQLite 3.53.2 with `SQLITE_ENABLE_FTS5` (libsqlite3-sys 0.38.2
`build.rs`). Tokenizers ([FTS5 docs](https://sqlite.org/fts5.html) §4.3):

- **unicode61** (default): case-insensitive; `remove_diacritics 2` strips all Latin diacritics
  correctly (1 misses some precomposed characters).
- **trigram**: substring matching; also speeds up `LIKE` and `GLOB`. Queries shorter than three
  characters match nothing. `remove_diacritics` exists from SQLite 3.45.0
  ([commit e186fe2](https://github.com/sqlite/sqlite/commit/e186fe20f5bd)).
- **porter** wraps another tokenizer with English stemming.

sqlx 0.9.0 requires `libsqlite3-sys <0.38`, and rusqlite 0.40 requires `^0.38.1`; Cargo allows only
one crate per `links = "sqlite3"`, so the two can't share a build.

### 4.4 Discovery: mdns-sd

mdns-sd 0.21.5: pure Rust, browse and register, macOS, Linux and Windows, IPv4 and IPv6, runs its own
thread and talks over channels, so it needs no async runtime. The README calls it "still beta
software". It sets `SO_REUSEADDR` and `SO_REUSEPORT` on port 5353 (`service_daemon.rs` ~L926),
so it should share the port with the system's mDNS service (from the source; not documented).
Alternatives: `zeroconf` 0.18.0 wraps Avahi or Bonjour; `libmdns` is responder-only and
unmaintained by its own README.

For the WebSocket API, axum 0.8.9 (MIT) uses `tokio-tungstenite` 0.29.

## Part 5 — Implications for the design

**Run, don't write:**

- **AirPlay: run `cliairplay` as a subprocess.** It already handles RAOP and AirPlay 2, transient
  and PIN pairing, a sender-side PTP master, and start-at-instant group sync, behind a plain
  stdin/FIFO/`[STATUS]` contract. No Rust alternative is close. GPL-3.0 doesn't constrain a server
  that runs it as a separate process.
- **Decoding: Symphonia first, ffmpeg for what it can't do** (gapless M4A, Opus, HE-AAC), or ffmpeg
  throughout for simplicity, as MA does.

**Write in Rust:**

- **The Sendspin server role.** Nothing reusable exists in Rust; aiosendspin is the reference to
  port. Start on the legacy unencrypted wire, which Bopsie speaks. Noise and the three pairing
  methods come later and are a substantial piece on their own.
- **The planner.** A pure function over members, capabilities and health, re-run on every change.
  It has three ways to sync a group:
  1. **AirPlay 2 direct**, one `cliairplay` per speaker on a shared PTP daemon.
  2. **Sendspin direct**, one stream per client on a shared timeline.
  3. **LinkPlay native leader/follower**, for speakers without a shared direct protocol, such as a
     WiiM Amp Pro (no AirPlay) with an Audio Pro A28. Here the leader carries the followers' audio,
     so choosing the leader by measured link quality matters.
- **A WiiM/LinkPlay provider** to start streams on a speaker and to form and break native groups.
- **Library, search and the MA-subset API** on lofty, rusqlite FTS5, mdns-sd and axum.

**Telemetry is mostly something the server builds itself.** Speakers report almost nothing. The
signals available are: AirPlay resend counts (needs `cliairplay` to emit them as `[STATUS]`
lines, a small upstream change), PTP readiness and stalls, `/feedback` timeouts, Sendspin buffer
level, late chunks and `min_buffer_ms` changes, and the server's own ping and jitter measurements
and reconnect counts.

**Main risks:**

- How LinkPlay devices behave in native groups, and how the WiiM and Audio Pro HTTP APIs differ.
  Not covered here.
- Running a PTP master needs ports 319/320: root or `CAP_NET_BIND_SERVICE` on Linux, and not
  possible on macOS. The server effectively targets Linux.
- Sendspin is moving to encrypted-only (sendspin-go's announced cutover); legacy-wire support
  in clients may not last.
- Speakers' quirks are many: `C/DESIGN.md` §11 lists Sonos playing silence after accepting 48/24,
  Sonos withholding audio until metadata arrives, and HomeKit pairing and PTP not crossing VLANs.

## Uncertainties and gaps

- The WiiM Amp Pro product page and manual PDF couldn't be read; its lack of AirPlay rests on
  WiiM's spec table (a search summary of the manual agrees, unverified).
- No Audio Pro firmware release notes were found on audiopro.com.
- Whether a WiiM device can run MA's Sendspin Cast receiver app is unknown.
- Whether these speakers advertise feature bit 41 (PTP) can only be read from their mDNS TXT
  records, which weren't checked.
- The LinkPlay HTTP API and native multiroom commands weren't researched.
- statime's unsuitability as an AirPlay PTP master is an inference from its source, not
  documented.
- mdns-sd's sharing of port 5353 is read from source, not documented.
