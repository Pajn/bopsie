# LinkPlay: discovery, the HTTP API, native playback and native multiroom

Research notes, 2026-10-14. Scope: what a Rust server needs in order to drive LinkPlay-based
speakers (WiiM, Audio Pro, Arylic and other OEMs) directly. It covers (1) discovery and identity,
(2) the HTTP API and its transport, (3) playing a server URL on a speaker, (4) native multiroom,
(5) groups that mix LinkPlay with AirPlay or Sendspin, and (6) risks, with the WiiM Amp Pro and the
Audio Pro A28 in mind. It fills the gap left by
[rust-server-ecosystem.md](rust-server-ecosystem.md) §3 and the "LinkPlay is unresearched" risk in
the [architecture plan](../plans/2026-10-13-rust-server-architecture.md).

Primary sources only: library and integration source at pinned commits, the two published HTTP API
documents, and manufacturer pages. Where a fact comes from an issue thread, a library's own
design notes or observation of two devices, it says so. LinkPlay itself publishes no
specification; both API documents are partial and each library encodes its own firmware folklore.

## Versions this is based on

| Component | Version | Commit | Licence |
|---|---|---|---|
| MA server, stable | 2.10.6 | [`c248664`](https://github.com/music-assistant/server/tree/c24866405bd2068c25a60e1014e5ef8c3f6445bb) | Apache-2.0 |
| MA server, `dev` | 2.11 nightlies | [`4f1a86c`](https://github.com/music-assistant/server/tree/4f1a86cd558cded72995fac9edc926a2e7c955df) | Apache-2.0 |
| `wiim` (Linkplay2020/wiim, LinkPlay's own SDK), pinned by MA | 0.1.9 | [`03b1857`](https://github.com/Linkplay2020/wiim/tree/03b18570d37d8cdeeb945b805b9e9e63976ceb37) | MIT |
| `wiim`, newest | 0.3.0 (2026-10-14) | [`f13a6d5`](https://github.com/Linkplay2020/wiim/tree/f13a6d5b9cd56fb643a5e5b63aee6fff2a068aca) | MIT |
| `pywiim` (mjcumming/pywiim), pinned by MA | 2.3.0 | [`c67d239`](https://github.com/mjcumming/pywiim/tree/c67d23953dd2c3189e60155bcc9e40db26a87870) | MIT |
| `pywiim`, newest | 2.3.10 (2026-10-13) | [`846dd24`](https://github.com/mjcumming/pywiim/tree/846dd2403200414f2a2300abdd250a7e2654ee8b) | MIT |
| `python-linkplay` (Velleman), pinned by HA | 0.2.14 | [`0531dd6`](https://github.com/Velleman/python-linkplay/tree/0531dd61286686d23a74f51af7367094cb140e0c) | MIT |
| Home Assistant core (`linkplay`, `wiim` integrations) | 2026.10.1 | [`abe1fdd`](https://github.com/home-assistant/core/tree/abe1fdd7cd6b4de1715c87b6fd87d92699514ef9) | Apache-2.0 |
| Arylic "Linkplay HTTP API" | v1.0.2 (undated; firmware 4.6.x) | [developer.arylic.com/httpapi](https://developer.arylic.com/httpapi/) | — |
| WiiM "HTTP API for WiiM Products" | v1.2 (PDF created 2024-07-10, served Last-Modified 2024-12-05) | [PDF](https://www.wiimhome.com/pdf/HTTP%20API%20for%20WiiM%20Products.pdf) | — |
| MA docs (music-assistant.io), WiiM page | `main` | [`71f9cac`](https://github.com/music-assistant/music-assistant.io/blob/71f9cac8c37a9f583feda0faf4e300837fa0ca4e/src/content/docs/player-support/wiim.md) | — |

MA has no `providers/linkplay`; everything LinkPlay lives in `providers/wiim` ("WiiM / LinkPlay"),
which is identical at both pins apart from one comment in `player.py`. Its manifest requires
`wiim==0.1.9` and `pywiim==2.3.0` and discovers on `_linkplay._tcp.local.`.

Link prefixes used below:

- `S` = `https://github.com/music-assistant/server/blob/c24866405bd2068c25a60e1014e5ef8c3f6445bb/music_assistant/`
- `D` = `https://github.com/music-assistant/server/blob/4f1a86cd558cded72995fac9edc926a2e7c955df/music_assistant/`
- `W` = `https://github.com/Linkplay2020/wiim/blob/03b18570d37d8cdeeb945b805b9e9e63976ceb37/src/wiim/`
- `PW` = `https://github.com/mjcumming/pywiim/blob/c67d23953dd2c3189e60155bcc9e40db26a87870/`
- `PL` = `https://github.com/Velleman/python-linkplay/blob/0531dd61286686d23a74f51af7367094cb140e0c/src/linkplay/`
- `HA` = `https://github.com/home-assistant/core/blob/abe1fdd7cd6b4de1715c87b6fd87d92699514ef9/homeassistant/components/`

Two devices were also checked on a local network and are cited as "observed": a WiiM Amp Pro on
firmware `Linkplay.5.2.830031` (Release 20260921), and an Audio Pro A28 whose UPnP `modelNumber`
is `V01-Nov  1 2024` (whether it is the original A28 or the A28 W is not confirmed).

## Part 1 — Discovery and identity

### 1.1 What the devices announce

- **mDNS `_linkplay._tcp`.** MA and both HA integrations discover on it (`S/providers/wiim/manifest.json`;
  `HA/linkplay/manifest.json`, `HA/wiim/manifest.json`). Observed on both devices: port 59152, TXT
  `uuid=uuid:<dashed UDN>`, `MAC=<..>`, `security=https 3.0`, `upnp=1.0.0`, `bootid=`. Neither
  advertises `_wiim._tcp`.
- **SSDP / UPnP.** Both are `urn:schemas-upnp-org:device:MediaRenderer:1` (`W/consts.py` L10,
  `PL/consts.py` L9). The description is at `http://<ip>:49152/description.xml` (observed; HA's
  `wiim` hard-codes `UPNP_PORT = 49152`, `HA/wiim/const.py` L25). MA tries the mDNS port, then
  49152, 59152 and port 80 (`S/providers/wiim/provider.py` L40, `_candidate_locations` L205).
  Services: AVTransport, RenderingControl, ConnectionManager, `urn:schemas-wiimu-com:service:PlayQueue:1`
  and a Tencent QPlay service (description.xml quoted in
  [wiim#15](https://github.com/Linkplay2020/wiim/issues/15), issue thread). The SSDP `SERVER`
  header is often a bare "Linux" on OEM devices, so pywiim confirms LinkPlay by probing the HTTP
  API (`PW/pywiim/discovery.py` L44–95, L271).

### 1.2 Identifiers

| Identifier | Where | Form |
|---|---|---|
| UDN | description.xml `<UDN>`, mDNS TXT `uuid` | `uuid:FF98F69B-0385-D8D7-A52C-CD2BFF98F69B` |
| LinkPlay UUID | `getStatusEx` `uuid`, description.xml `<uuid>`, slave lists | 24 hex, `FF98F69B0385D8D7A52CCD2B` |
| MAC | mDNS TXT `MAC`; `getStatusEx` `MAC`, `STA_MAC`, `ETH_MAC`, `BT_MAC`, `AP_MAC` | `08:E9:F6:8F:8F:A2` |
| Model | description.xml `manufacturer`, `modelName`, `modelNumber`; `getStatusEx` `project`, `priv_prj`, `hardware` | e.g. `project=WiiM_Amp_Pro`, `hardware=AmlogicA113` (observed) |
| Firmware | `getStatusEx` `firmware`, `Release` | `Linkplay.5.2.830031`, `20260921` |
| Multiroom generation | `getStatusEx` / `getSlaveList` `wmrm_version` | `4.2`, `4.3` (observed on the Amp Pro) |

- **UDN from UUID.** LinkPlay builds the UDN by appending the UUID's first 8 hex digits and
  dashing it (`S/providers/wiim/helpers.py` `linkplay_slave_uuid_to_udn`, L91; WiiM PDF §2.1 shows
  `uuid` `FF970016A6FE22C1660AB4D8` with `upnp_uuid` `uuid:FF970016-A6FE-22C1-660A-B4D8FF970016`).
  The wiim SDK strips the suffix the other way to build join commands (`W/controller.py` L352).
- **Which MAC.** The WiiM PDF calls `MAC` "The WiFi MAC address of the device"; Arylic's doc calls
  it the hotspot-mode MAC and gives `STA_MAC` as the station MAC. On a wired device these may all
  differ from the address the speaker is on.
- **Telling vendor and model.** MA routes on UPnP `manufacturer`: `"Linkplay"` with a WiiM model
  name means WiiM; `"Audio Pro AB"` means Audio Pro; any other device that answers the HTTP API is a
  generic LinkPlay OEM (`S/providers/wiim/helpers.py` `is_official_device` L62; `W/consts.py`
  L193–194). WiiM `project` values have raw aliases such as `Muzo_Mini`
  (`PW/pywiim/model_names.py`). pywiim splits Audio Pro into "original", "W-generation" and
  "MkII" by model string and a firmware regex, and treats any A10/A15/A28/C10 as MkII by default
  (`PW/pywiim/profiles.py` L411–442).

### 1.3 Linking with the same speaker's AirPlay endpoint

MA gives `WiimPlayer` identifiers UUID (UDN without `uuid:`), IP, and the mDNS TXT `MAC`
(`S/providers/wiim/player.py` L85–94, `provider.py` L107). The AirPlay provider adds a MAC from
the AirPlay TXT `deviceid` (`S/providers/airplay/provider.py` L311). Protocol linking matches MAC,
then serial, then UUID, then IP ([rust-server-ecosystem.md](rust-server-ecosystem.md) §3.4), so in
MA a LinkPlay speaker and its AirPlay endpoint join on MAC, falling back to IP. Bopsie's linking
rule is the same. Whether `deviceid` equals the `_linkplay` TXT `MAC` on these devices (rather than
`STA_MAC` or `ETH_MAC`) is not stated in any source; the IP fallback covers it either way.

## Part 2 — The HTTP API

### 2.1 Transport

Every command is `GET /httpapi.asp?command=<command>`. The response is JSON for status commands
and a plain `OK` (or `unknown command`) for most others (Arylic doc, "Abstract"). Neither document
mentions authentication beyond TLS.

The transport varies by generation, and this is the first thing a client has to solve:

| Device class | Transport | Source |
|---|---|---|
| Older LinkPlay / Arylic | `http://` on 80 | Arylic doc |
| WiiM | `https://` on 443, self-signed, no client certificate | WiiM PDF §1.1 ("It supports the https based API"); observed on the Amp Pro |
| Audio Pro "MkII" class | `https://` on 4443, **mutual TLS**: the server sends a CertificateRequest | pywiim profile `PROFILE_AUDIO_PRO_MKII` (`PW/pywiim/profiles.py` L234–275); observed on the A28 (80 and 443 closed, 4443 asks for a client cert) |
| Audio Pro W-generation | HTTPS, port 8443 in pywiim's list | `PW/docs/design/LESSONS_LEARNED.md` |

**How the clients pick a port and scheme:**

- **python-linkplay / HA `linkplay`:** `https:443`, then `https:4443`, then `http:80`
  (`PL/discovery.py` `linkplay_factory_httpapi_bridge`, L44–66), always with an unverified TLS
  context that presents the client certificate below (`PL/utils.py` L163–206; HA builds its
  session with `async_create_unverified_client_session`, `HA/linkplay/utils.py`). HA's `linkplay`
  aborts any device whose manufacturer is WiiM and leaves it to HA's `wiim` integration
  (`HA/linkplay/config_flow.py` L54–62).
- **`wiim` SDK / HA `wiim` / MA's official backend:** UPnP description over HTTP, and the HTTP API
  fixed at `https:443` with verification off and **no client certificate**. If that fails it logs
  "some features might be limited" and runs without an HTTP API (`W/discovery.py`
  `async_create_http_api_endpoint`, L96–127). This is unchanged up to `wiim` 0.3.0.
- **pywiim (MA's generic backend and every MA grouping command):** with no port given, probes
  `https:443`, `https:4443`, `https:8443`, `http:80`, `http:8080` in that order
  (`PW/pywiim/api/base.py` `_build_standard_probe_list`, L888) and caches the first that answers
  ([ADR 015](https://github.com/mjcumming/pywiim/blob/c67d23953dd2c3189e60155bcc9e40db26a87870/docs/design/adr/015-https-default-and-protocol-port-fallback.md)).
  Once it knows a device is Audio Pro MkII it switches to `https` on 4443, 8443, 443 only
  (`PW/pywiim/capabilities.py` L669–676). Its TLS context is TLS 1.0–1.3, `ALL:@SECLEVEL=0`,
  verification off, and always loads the client certificate (`PW/pywiim/api/ssl.py` L20–85). Its
  constants note that "the mTLS exchange can take several seconds" (`PW/pywiim/api/constants.py`
  L312).

pywiim's own `LINKPLAY_ARCHITECTURE.md` still says the API is "port 80, plain HTTP only" and that
integrations "must explicitly force plain HTTP". That contradicts its ADR 015, its code, the WiiM
PDF and both observed devices; treat that document as unreliable.

### 2.2 The client certificate

- **What it is.** A PEM private key plus a self-signed certificate, subject and issuer
  `C=CN, ST=Shanghai, L=Shanghai, O=Linkplay, OU=inc, CN=www.linkplay.com,
  emailAddress=mail@linkplay.com`, serial 1, valid 2018-11-15 to 2046-04-01, SHA-256 fingerprint
  `C7:2B:BC:07:6D:AD:BB:F9:9B:62:84:6A:DD:1E:9D:0E:15:95:43:A7:82:27:00:30:01:F9:62:1C:01:BB:CF:93`.
- **Who ships it.** python-linkplay (`PL/consts.py` `MTLS_CERTIFICATE_CONTENTS`, L13; added in
  PR [#16](https://github.com/Velleman/python-linkplay/pull/16), 2024-08-12, with no note of its
  origin) and pywiim (`PW/pywiim/api/constants.py` `AUDIO_PRO_CLIENT_CERT`, L33–89). Both copies
  have the same fingerprint, and pywiim also loads the same certificate as a CA (`WIIM_CA_CERT`).
  pywiim credits it to [ramikg/linkplay-cli](https://github.com/ramikg/linkplay-cli)
  (`linkplay_cli/certs/linkplay_client.pem`, Unlicense) and, before that,
  [osk2/yamaha-soundbar](https://github.com/osk2/yamaha-soundbar) (`client.pem`, repository has no
  licence).
- **Licensing.** No source shows LinkPlay publishing or licensing this key. It appears to have been
  extracted from LinkPlay software and has been passed between MIT, Unlicense and unlicensed
  repositories; those licences cannot grant rights their authors never held. A server that bundles
  it redistributes a vendor's private key on the same footing as HA does. Loading it from a
  user-supplied file instead keeps it out of the repository.
- **MA's position.** MA's official backend (used for Audio Pro) never presents it, because the
  `wiim` SDK does not. MA's grouping commands go through pywiim, which does.

### 2.3 Commands a server needs

From the Arylic doc and WiiM PDF, cross-checked against the three libraries
(`PL/consts.py` `LinkPlayCommand` L67; `W/consts.py` `WiimHttpCommand` L19;
`PW/pywiim/api/constants.py` L351–355):

| Purpose | Command | Reply |
|---|---|---|
| Device info | `getStatusEx` | JSON, fields in §1.2 plus `group`, `master_uuid`, `master_ip`, `streams`, `streams_all`, `plm_support`, `RSSI`, `wlanFreq`, `security`, `GroupName` |
| Player state | `getPlayerStatus`, `getPlayerStatusEx` | JSON: `type` (0 leader/solo, 1 follower), `ch`, `mode`, `loop`, `eq`, `status` (`stop`, `play`, `load`/`loading`, `pause`), `curpos` and `totlen` in **ms**, `offset_pts`, `plicount`, `plicurr`, `vol` 0–100, `mute` 0/1, and `Title`, `Artist`, `Album` as **hex-encoded UTF-8** |
| Track metadata | `getMetaInfo` | JSON `{"metaData": {album, title, artist, albumArtURI, sampleRate, bitDepth, ...}}` (WiiM PDF §2.9; note the stray spaces in `"albumArtURI "` and `"sampleRate "` in the PDF's example) |
| Play a URL | `setPlayerCmd:play:<url>` | `OK` |
| Play a playlist | `setPlayerCmd:m3u:play:<url>` (Arylic) or `setPlayerCmd:playlist:<url>:<index>` (WiiM PDF) | `OK` |
| Transport | `setPlayerCmd:pause`, `:resume`, `:onepause`, `:stop`, `:prev`, `:next` | `OK` |
| Seek | `setPlayerCmd:seek:<seconds>` | `OK`; "no use when playing radio link" (Arylic) |
| Volume, mute | `setPlayerCmd:vol:<0-100>`, `setPlayerCmd:mute:<0\|1>` | `OK` |
| Input | `setPlayerCmd:switchmode:<wifi\|line-in\|bluetooth\|optical\|udisk\|...>` | `OK` |
| Clock | `timeSync:YYYYMMDDHHMMSS` (UTC) | `OK` |

- **`mode` values** (WiiM PDF §2.3.1): 0 none, 1 AirPlay, 2 third-party DLNA, 10–19 LinkPlay
  playlist (10 default, 11 USB), 31 Spotify Connect, 32 TIDAL Connect, 40 AUX, 41 BT, 43 optical,
  99 follower. Arylic adds 20 "playback started by HTTPAPI". `mode` can stay at 99 after leaving a
  group; `group` is authoritative
  ([pywiim ADR 009](https://github.com/mjcumming/pywiim/blob/c67d23953dd2c3189e60155bcc9e40db26a87870/docs/design/adr/009-group-role-and-slave-detection.md)).
- **Hex fields** are still hex on WiiM 5.2 firmware (`Title: 536C6565702041706E6561` in a
  `getPlayerStatusEx` log in [MA support #6557](https://github.com/music-assistant/support/issues/6557),
  issue thread). `getStatusEx` `essid` is hex too.
- **URL argument.** pywiim percent-encodes the whole URL (`quote(url, safe="")`,
  `PW/pywiim/api/playback.py` L330–346); python-linkplay passes it raw (`PL/bridge.py` L191).
- **Audio Pro MkII** does not support `getPlayerStatusEx`, `getMetaInfo` or EQ, and its HTTP status
  does not carry play state or volume; pywiim reads those from UPnP on that profile
  (`PW/pywiim/profiles.py` L234–275).
- **Loop modes** disagree between vendors and between the WiiM PDF's tables; nothing here needs them.

### 2.4 Polling, events and rate

- **No push over HTTP.** Neither document describes events. State changes arrive only as UPnP GENA
  events on AVTransport, RenderingControl and PlayQueue, which need the device to reach a callback
  port on the server. The `wiim` SDK subscribes to all three (`W/wiim_device.py` L272–430); MA
  re-reads group topology when a RenderingControl `LastChange` mentions `Slave`
  (`S/providers/wiim/player.py` L415–425). Missed events across VLANs or a host firewall are a
  recurring cause of stale state in MA ([#6557](https://github.com/music-assistant/support/issues/6557),
  [#5900](https://github.com/music-assistant/support/issues/5900), issue threads).
- **Polling rates in use.** MA: every 5 s while playing, 30 s idle (`S/providers/wiim/player.py`
  L188–197). pywiim: 1 s playing, 5 s idle, 3 s / 15 s on legacy devices
  (`PW/pywiim/polling.py` L64–74). HA `linkplay`: 5 s with `PARALLEL_UPDATES = 1`.
- **Rate limits** are not documented. Every client serializes per device; none was found that
  issues concurrent requests to one speaker.
- A TCP "UART pass-through" on port 8899 also exists (`PL/consts.py` `TCPPORT`, `getStatusEx`
  `uart_pass_port`); nothing needed here uses it.

## Part 3 — Native playback from a server

### 3.1 Two ways to start a URL

1. **HTTP:** `setPlayerCmd:play:<url>`. Simple, but has no "next URI", so gapless needs a
   continuous stream.
2. **UPnP AVTransport:** `SetAVTransportURI` (with DIDL-Lite metadata), `Play`, and
   `SetNextAVTransportURI` for the next track. This is what MA does on WiiM and Audio Pro:
   `play_media` → `device.async_play` (`S/providers/wiim/player.py` L228; `W/wiim_device.py`
   L1390–1414), `enqueue_next_media` → `SetNextAVTransportURI` (`player.py` L254–272), and pause,
   stop and seek also go over AVTransport (`W/wiim_device.py` L1416–1464). Volume goes over HTTP
   (`setPlayerCmd:vol`, L1466) and mute over RenderingControl.

MA's generic LinkPlay OEM backend does not play natively at all: it is a grouping shell and hands
audio to the device's linked DLNA (default) or AirPlay endpoint
(`S/providers/wiim/linkplay_player.py` L1–8, `default_output_protocol_domain` L128).

### 3.2 Formats and rates

- **No codec list** exists in either API document. The Arylic examples are a FLAC file and a radio
  stream; WiiM's preset examples include an HLS `.m3u8` and Radio Paradise's continuous FLAC
  stream (WiiM PDF §2.8.2).
- **MA's defaults.** FLAC (`CONF_OUTPUT_CODEC` default `"flac"`,
  `S/controllers/streams/controller.py` L593–597), per-track URLs, no `Content-Length` (HTTP
  profile default `no_content_length`, `S/constants.py` L646–658; `chunked` and
  `forced_content_length` are options). `WiimPlayer` advertises up to 192 kHz / 24-bit
  (`S/providers/wiim/player.py` L215–224).
- **Per-model limits.** MA's docs: "Older Generation 1 devices only support up to 48 kHz / 16 bit".
  Audio Pro lists the A28 W as "Streaming quality: Up to 24/96"
  ([product page](https://audiopro.com/products/a28-w)). WiiM's Amp / Amp Pro spec table gives only
  the ADC (192 kHz / 24-bit) and no streaming limit. **Multiroom playback is limited to
  48 kHz / 16-bit** ([WiiM FAQ](https://faq.wiimhome.com/en/support/solutions/articles/72000644202-how-to-use-wiim-multi-room-music-to-enhance-your-listening-experience),
  modified 2026-07-23).
- **Live / infinite streams** work in the sense that the device plays radio URLs and HLS, and MA
  serves radio and live sources without a length, with DLNA "realtime" flags
  (`S/controllers/streams/controller.py` L851–879). Whether a server-paced, never-ending FLAC
  stream (MA's flow mode) plays without drift or buffer underrun on these devices is not
  documented.

### 3.3 Timing, buffering, seeking

- **Start latency and buffer depth** are not documented anywhere found. No source gives the time
  from `play` to first audible sample, how much the device prefetches, or whether that depends on
  codec or sample rate.
- **Knowing when audio is audible.** Nothing reports it. The proxies are: UPnP `TransportState`
  passing `TRANSITIONING` to `PLAYING`, HTTP `status` passing `load` to `play`, and `curpos`
  starting to advance. HTTP `curpos` is in ms; UPnP `GetPositionInfo` `RelTime` is `HH:MM:SS`.
  MA treats `TRANSITIONING` as playing (`S/providers/wiim/player.py` L42–47) and ignores the
  device's position until it reports MA's own URI, because "the device reports its previous
  content's position until it loads our stream URI" (L586–601). The device also reports a brief
  false `PLAYING` with no URI loaded during group setup (L532–540).
- **Seek.** The device supports `setPlayerCmd:seek:<s>` and AVTransport `Seek` with `REL_TIME`.
  MA does not rely on it for its own streams: a queue seek restarts the item stream at the offset
  (`S/controllers/player_queues/controller.py` `seek`, L859–884). A server that encodes on the fly
  without `Content-Length` or range support should do the same.
- **Gapless** is via `SetNextAVTransportURI`; MA declares `GAPLESS_PLAYBACK` and `ENQUEUE` on
  WiiM. MA's docs report that the first time a WiiM is grouped "a song is restarted", apparently
  the firmware switching to group mode.

## Part 4 — Native multiroom

### 4.1 Commands

| Action | Sent to | Command | Source |
|---|---|---|---|
| Join, router mode | follower | `ConnectMasterAp:JoinGroupMaster:eth<leader_ip>:wifi0.0.0.0` | Arylic doc; `PL/consts.py` L99; `PW/pywiim/api/group.py` L313 |
| Join, WiFi Direct (legacy) | follower | `ConnectMasterAp:ssid=<hex ssid>:ch=<channel>:auth=OPEN:encry=NONE:pwd=:chext=0` | `PW/pywiim/api/group.py` L297 |
| Join (LinkPlay SDK form) | follower | `multiroom:JoinGroup:IP=<leader_ip>:uuid=<leader 24-hex uuid>` | `W/consts.py` L34, `W/controller.py` L340–409 |
| List followers | leader | `multiroom:getSlaveList` → `{"slaves": n, "wmrm_version": "4.2", "slave_list": [{name, uuid, ip, version, type, channel, volume, mute, ...}]}` | Arylic doc |
| Remove one follower | leader | `multiroom:SlaveKickout:<follower_ip>` | Arylic doc |
| Leave | follower | `multiroom:LeaveGroup` (SDK) or `multiroom:Ungroup` on the follower (pywiim) | `W/consts.py` L32; `PW/pywiim/api/group.py` L321 |
| Dissolve the group | leader | `multiroom:Ungroup` | Arylic doc |
| Follower volume / mute | leader | `multiroom:SlaveVolume:<ip>:<vol>`, `multiroom:SlaveMute:<ip>:<0\|1>` | `PW/pywiim/api/constants.py` L354–355; Arylic gives no SlaveVolume format |
| Whole-group volume / mute | leader | `setPlayerCmd:slave_vol:<vol>`, `setPlayerCmd:slave_mute:mute\|unmute` | `PL/consts.py`; `PL/bridge.py` L575–590 |

Role detection: `getStatusEx` `group` != `"0"` with `master_uuid` / `master_ip` pointing elsewhere
means follower; otherwise `getSlaveList` decides leader or solo
(`PW/pywiim/api/group.py` `get_device_group_info`, L427–520).

### 4.2 Router mode and WiFi Direct

- **Router mode** (`wmrm_version` 4.2+, firmware ≥ 4.2.8020): every member stays on the LAN.
- **WiFi Direct** (`wmrm_version` 2.0): followers move onto the leader's private network
  (`10.10.10.x`), become unreachable from the LAN, and appear in `getSlaveList` with internal IPs, so
  they must be matched by UUID and removed with a leader-side kick
  ([pywiim ADR 010](https://github.com/mjcumming/pywiim/blob/c67d23953dd2c3189e60155bcc9e40db26a87870/docs/design/adr/010-wifi-direct-multiroom-player-resolution.md);
  `PW/pywiim/models.py` `needs_wifi_direct_multiroom`, L88).
- **Compatibility.** "version 4.2 or above won't work with any version below 4.2" (WiiM PDF §2.1,
  `wmrm_version`). MA only groups two generic devices when both report the same `wmrm_version`
  major and neither needs WiFi Direct (`S/providers/wiim/helpers.py` `linkplay_group_compatible`,
  L25); pairs that involve WiiM or Audio Pro are offered and validated at command time.

### 4.3 Behaviour

- **What the leader sends.** "This device receives audio from its input source and broadcasts it
  to other WiiM devices simultaneously", for every input: Wi-Fi, Bluetooth, line in, optical, HDMI
  ARC (WiiM FAQ above). So the leader's input can be an HTTP URL, AirPlay, DLNA or an analog input.
  The transport between leader and followers is not documented by LinkPlay. pywiim's design notes
  claim "uncompressed PCM via multicast"; no primary source supports that.
- **Quality and sync.** 48 kHz / 16-bit (above). WiiM claims "All speakers stay in sync, free of
  delay or echo" and has a "Group Audio Delay" setting. No measured figure was found.
- **Size.** No fixed limit; WiiM's FAQ says it depends on the router.
- **Volume.** Per member (`multiroom:SlaveVolume`, or the follower's own `setPlayerCmd:vol`) and
  for the group (`setPlayerCmd:slave_vol` on the leader). Mute on the leader also mutes followers
  in group play (WiiM PDF §2.3.12).
- **Followers' own playback** stops: a follower reports `mode` 99 / `type` 1 and plays only the
  leader's audio. MA publishes a follower as idle with only its own volume
  (`S/providers/wiim/player.py` `_publish_follower_state`, L615–629).
- **Settling.** Devices accept a grouping command before the role changes; MA polls the leader's
  slave list for up to 10 s to verify (`S/providers/wiim/grouping.py` `VERIFY_MAX_WAIT`, L36;
  `_verify_membership`, L814).

### 4.4 How MA groups natively

- `WiimPlayer` and `LinkPlayPlayer` both declare `SET_MEMBERS`; a single `NativeGroupCoordinator`
  owns topology for both backends and rebuilds it from leaders' slave lists (re-read every 60 s,
  or immediately on commands and `Slave` events; `S/providers/wiim/grouping.py` L32, L56).
- **Joins.** Two official devices (WiiM or Audio Pro) join through the SDK's
  `multiroom:JoinGroup:IP=…:uuid=…`, which first ungroups a follower that is already grouped. Any
  other pair uses pywiim's `ConnectMasterAp:JoinGroupMaster`, after dissolving the joining
  member's own followers (`grouping.py` `_join` L659, `_low_level_join` L684).
- **Leaves.** Official pairs ungroup through the SDK and fall back to a leader-side
  `SlaveKickout`; generic pairs send `multiroom:Ungroup` on the follower; mixed pairs kick from the
  leader (`_leave` L704, `_leader_side_detach` L757).
- **Priority.** `WiimPlayer` keeps the default `prefer_native_grouping = False`; `LinkPlayPlayer`
  returns True while its API is reachable (`S/providers/wiim/linkplay_player.py` L197). In MA's
  member selection (`D/controllers/players/protocol_linking.py` `_select_grouping_for_member`,
  L2741) a child first joins the parent's live native session if it can, then native grouping if it
  prefers it, then its preferred protocol, the group's active protocol, native grouping, and
  finally any common protocol with `SET_MEMBERS`.
- **Non-LinkPlay members** cannot join a native group. LinkPlay multiroom is between LinkPlay
  devices only; WiiM's FAQ speaks only of WiiM devices, and MA refuses any member whose backend is
  not one of its two LinkPlay backends (`grouping.py` `_require_member`, L648–657). An AirPlay or
  Sendspin speaker can only be synchronised with a LinkPlay group at the leader's input (Part 5).

## Part 5 — Mixed groups

- **A LinkPlay leader can be fed over AirPlay and still carry LinkPlay followers.** MA's base player
  says "device-side grouping keeps working whatever feeds the leader, so members can stay natively
  grouped while the leader streams over another protocol"
  (`D/models/player.py` `native_grouping_requires_own_stream`, L785, default False; only the
  AirPlay provider overrides it). MA's docs state the result: when a WiiM with AirPlay plays in an
  AirPlay group, "other WiiM players can be synced with this player (even if they themselves do
  not have AirPlay, as the native WiiM will be used for that connection)", with "a small silence
  due to the stream restart" when grouping forces the switch.
- **What MA does on a mixed join.** If the joining member shares no protocol with the group's
  current one, MA picks a common protocol (AirPlay for WiiM + AirPlay), stops the old stream and
  restarts on the new one ([rust-server-ecosystem.md](rust-server-ecosystem.md) §3.4). LinkPlay
  members without that protocol stay natively grouped under the leader.
- **Known failure.** MA's docs: grouping a WiiM without AirPlay into an AirPlay group via a WiiM
  that has it can give silence until the bridge device's AirPlay setting is "Prefer Airplay 2".
- **Sync across the boundary is unmeasured.** In this shape the AirPlay members are timed by the
  server's PTP anchor, the LinkPlay leader by its AirPlay receiver, and the followers by LinkPlay's
  own multiroom relative to the leader. Whether a follower lands within a few ms of the AirPlay
  members depends on the leader compensating for its multiroom latency inside its AirPlay latency.
  No source says it does.
- **Sendspin.** No LinkPlay device speaks Sendspin ([rust-server-ecosystem.md](rust-server-ecosystem.md)
  §3.3). A LinkPlay group can share a timeline with Sendspin players only through a leader input
  the server times, which today means AirPlay.

For the two devices in question: the WiiM Amp Pro has no AirPlay; the Audio Pro A28 has it but
withdraws it while it follows another LinkPlay device (Part 7), as it did when first browsed. Without AirPlay on either, they can share a timeline
only with each other, through native multiroom, and the server reaches them only through LinkPlay
itself: UPnP for audio, the HTTP API for control and grouping.

## Part 6 — Risks and unknowns

- **MA's official backend and mTLS-only Audio Pro (inference from source).** The `wiim` SDK only
  tries `https:443` without a client certificate. On a device like the observed A28 (only 4443,
  mutual TLS) it runs without an HTTP API, so: volume (`setPlayerCmd:vol`) fails; the availability
  loop marks the device unavailable whenever no UPnP event arrived within the 60 s interval MA
  configures (`W/wiim_device.py` L2410–2440, `S/providers/wiim/provider.py` L134); and an official
  join raises "HTTP API not available" for leader or follower (`W/controller.py` L344, L376). A
  Rust server should not copy that path for Audio Pro; pywiim's 4443-with-certificate path is the
  one that can work.
- **Firmware variance is the norm.** pywiim encodes three Audio Pro generations, vendor-specific
  loop modes, endpoints that answer `unknown command` on some firmware (e.g. `EQGetStat` on WiiM
  Pro, `PW/docs/design/API_DESIGN_PATTERNS.md` L135), a stale `mode` after leaving a group, and a
  `getStatusEx` multiroom section that is sometimes empty on a leader (`PW/pywiim/api/group.py`
  L150–178). The Arylic doc itself warns responses on older firmware may differ and shows
  malformed JSON in an example.
- **Group mode costs quality**: 48 kHz / 16-bit for every member.
- **Group start restarts the track** the first time a WiiM is grouped (MA docs).
- **Events depend on reachability** of a callback port on the server, which VLANs and host
  firewalls break.
- **The client certificate** is a vendor private key of unclear provenance (§2.2).
- **WiiM Amp Pro.** No AirPlay per WiiM's spec table (modified 2025-04-30); the newest firmware
  release notes don't mention it. On firmware 5.2.830031 it answers `getStatusEx` on 443 without a
  client certificate (observed), reports `wmrm_version` 4.3, and announces neither `_airplay._tcp`
  nor `_raop._tcp` (observed).
- **Audio Pro A28.** Announces neither `_airplay._tcp` nor `_raop._tcp` (observed), whatever the
  product page says. HTTP API only on 4443 with mutual TLS (observed); pywiim's MkII profile says
  play state and volume must come from UPnP on such devices. Its `wmrm_version`, and so whether it
  can group with a 4.3 WiiM at all, is unknown until the API is reached with the certificate.
  Audio Pro's app is WiiM-based (`com.wifiaudio.audioprobywiim`), which suggests but does not show
  that the two can group.

## Part 7 — Observed on the two devices

Measured against a WiiM Amp Pro (firmware Linkplay.5.2.830031, `wmrm_version` 4.3) leading an
Audio Pro A28 (firmware 1.56.634247, `wmrm_version` 4.2) in a native group the owner had set up.

- **The A28's API works over mutual TLS.** With the LinkPlay client certificate (§2.2) it
  answers on `https:4443`; `getStatusEx` reports `MAC` equal to its `_linkplay` TXT `MAC`, and
  `uuid` equal to its UDN without dashes or the trailing repeat. `getPlayerStatus` answers
  `unknown command` on it, as pywiim's MkII profile says.
- **A 4.3 leader carries a 4.2 follower.** The follower's `getStatusEx` shows `group: 1`,
  `master_uuid` and `master_ip`; the leader's `multiroom:getSlaveList` lists it with its volume.
- **A follower withdraws AirPlay.** The A28 announced neither `_airplay._tcp` nor `_raop._tcp`
  while following; taken out of the group it announced both within 13 s, and withdrew them again
  on rejoining. Its `streams` (`0x1edf3bfd`, bit 0 AirPlay set) was the same either way, so
  `streams` bit 0 tells that a device has AirPlay even while it is hidden. The Amp Pro's has bit
  0 clear, matching its lack of AirPlay. Its AirPlay `deviceid` equals its `_linkplay` TXT `MAC`.
- **Start, by `setPlayerCmd:play:<url>`** of a WAV over plain HTTP to the leader of the group:
  `status` turned `play` after 1.3 s; `curpos` then counted up from about −600 ms, crossing 0
  about 2.55 s after the command, and advanced at real time from there. A negative `curpos` is
  the device's countdown to audible playback: the signal §3.3 found no source for. One
  measurement, of a group of two; solo and over AVTransport not measured.
- **Grouping is quick.** `multiroom:SlaveKickout:<follower_ip>` on the leader took effect within
  0.9 s, seen from both sides; `ConnectMasterAp:JoinGroupMaster:eth<leader_ip>:wifi0.0.0.0` on
  the follower within 0.6 s. The follower's volume in the group was kept by the leader and had
  to be set again with `multiroom:SlaveVolume` after rejoining.
- **UPnP's position as a start signal.** Where `getPlayerStatus` is missing, as on the A28,
  `GetPositionInfo` `RelTime` counts whole seconds from when the stream is audible. On the Amp
  Pro, which has both, the moment `RelTime` first turned from 0 to 1, read every 50 ms, came
  143, 128 and 126 ms after a second past the countdown's audible instant in three trials; so a
  device without a countdown can be timed to within about 50 ms from that tick, less 1.13 s.
  Measured this way, the A28 alone started 0.36 to 1.25 s from where it was aimed, varying from
  start to start more than the Amp Pro did.
- **Joining an AirPlay group from a native one.** Asked to join an AirPlay group while it
  followed the Amp Pro, the A28 was removed from its native group with `SlaveKickout`, announced
  AirPlay 1.2 s later, and started on time in the AirPlay group, 1.5 s after the request.
  Rejoined to the Amp Pro afterwards, it ended its AirPlay session (`cliairplay` reported its
  control channel failed) and withdrew AirPlay again.
- **Playing a URL switches the leader's input** (`mode` from 49, HDMI, to 10, network);
  `setPlayerCmd:switchmode:HDMI` switches it back. A server that plays to a LinkPlay device
  takes it away from whatever input it was on.

## Implications for the design

- **Endpoint shape.** A LinkPlay endpoint is a UPnP MediaRenderer plus an HTTP API whose transport
  must be probed: `https:443`, `https:4443` (with the client certificate), `https:8443`, `http:80`;
  cache the result per device. Identity is the UDN, derivable from the 24-hex UUID.
- **Playback over UPnP, control over HTTP.** AVTransport gives `SetNextAVTransportURI` for
  gapless and works on Audio Pro MkII where HTTP status is thin; HTTP gives grouping, `getStatusEx`
  and millisecond position. Subscribe to GENA events and poll as a fallback.
- **Native leader/follower.** The leader can take its audio from any input, so the planner's
  third strategy has two variants: the server streams an HTTP URL to the leader, or the leader is
  an AirPlay member of a server-timed AirPlay group and carries LinkPlay followers. The second is
  the only way an AirPlay-less LinkPlay speaker (such as the Amp Pro) shares a timeline with
  AirPlay or Sendspin speakers.
- **Capabilities to record per endpoint:** transport (scheme, port, mTLS), `wmrm_version`,
  router vs WiFi Direct, max rate and depth solo, 48/16 in a group, AirPlay present, HTTP status
  completeness (MkII), and whether it can lead.
- **Seek by restarting the stream** at an offset; don't depend on device seek.

## Open questions

1. How long from `SetAVTransportURI` + `Play` (or `setPlayerCmd:play`) to the first audible sample
   on each device, and how much does it buffer? Part 7 has one figure (2.55 s, `setPlayerCmd`, a
   group of two); solo, AVTransport and repeatability remain.
2. Does a server-paced, never-ending FLAC or WAV stream play indefinitely without underrun or
   drift on the Amp Pro and the A28?
3. For a LinkPlay speaker that does have AirPlay, leading an AirPlay group with AirPlay-less
   LinkPlay followers: how far are the followers from the other AirPlay members? Does "Group Audio
   Delay" need setting? (Neither observed device has AirPlay, so this needs other hardware.)
4. Settled (Part 7): yes; `wmrm_version` 4.2, and `MAC` matches the `_linkplay` TXT.
5. Settled (Part 7): yes, from `ConnectMasterAp:JoinGroupMaster`, with the Amp Pro leading.
6. Settled (Part 7): on the A28 the AirPlay `deviceid` equals the `_linkplay` TXT `MAC`.
7. Settled: the Amp Pro on firmware 5.2.830031 has no AirPlay; the A28 has it but withdraws it
   while following another device (Part 7).
8. What sample rates does each device accept from an HTTP URL solo (24/96 on the A28 W per Audio
   Pro; untested above 48 kHz on the Amp Pro)?
9. What happens to a follower's own UPnP or AirPlay session when it is kicked: does it go idle, or
   resume what it played before?
10. Should the server ship the LinkPlay client certificate as HA does, or read it from a configured
    file?
11. Is there any request rate at which these devices start failing? No source states one.
