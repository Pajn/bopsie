# A shared Sendspin client, and a macOS menu bar player

A Sendspin player written once in Rust and used by every Bopsie client that plays audio: a macOS
menu bar app first, then a headless Linux player, iOS playback and, in time, Android in place of
the Kotlin client in `modules/cratie-player`. The macOS app is a Swift shell around it: a menu
bar item and a settings window to sign in.

The protocol facts are in [the MA and Sendspin research](../research/music-assistant-and-sendspin.md).
This document decides; it does not re-argue.

## Why one client

Bopsie needs a Sendspin player on four platforms. The hard parts are the same on all of them: the
clock filter, scheduling against server time, drift correction, decoding. Written once, they are
tested once, and the move to Sendspin 1.0 (Noise encryption and pairing) is one migration instead
of one per platform. Rust reaches every target: natively on macOS and Linux, through
[UniFFI](https://mozilla.github.io/uniffi-rs/) bindings for Swift and Kotlin.

## Building on sendspin-rs

The Sendspin project publishes [sendspin-rs](https://github.com/Sendspin/sendspin-rs) (the
`sendspin` crate, MIT or Apache-2.0). It is further along than its README says: a Kalman clock
filter with drift tracking, a synchronized player with sync correction, PCM, FLAC and Opus
decoding, a client and a server side, and adapters in the Sendspin conformance suite. It speaks
the same unencrypted wire as `players/sendspin` in the server. Its Swift sibling, SendspinKit, is
Noise-only and Apple-only, so it can neither talk to Bopsie today nor reach Linux and Android; it
stays a reference for Apple audio output and for how 1.0 pairing looks to an app.

Bopsie builds on a fork, [Pajn/sendspin-rs](https://github.com/Pajn/sendspin-rs), so changes can
be tested in Bopsie before they are offered upstream. Every change on the fork is written as an
upstream pull request would be: general rather than Bopsie-specific, behind the crate's existing
conventions, with tests, one concern per commit. Bopsie-specific behaviour lives in Bopsie's own
crates, never in the fork. The fork follows upstream, and its changes go upstream once they have
proved themselves, so that eventually Bopsie depends on a release of `sendspin` again.

Bopsie depends on the fork as a git dependency pinned to a commit. Working on both at once uses a
local Cargo `[patch]` that points at a checkout of the fork; it is not committed.

### Changes the fork needs

1. **Cargo features.** Today every dependency is unconditional, including `cpal` (and ALSA on
   Linux) and the codecs. The crate splits into: the message types, always built, needing only
   `serde` and `serde_json`; `client`, for the connection, clock and connection manager; and
   `player`, for `cpal`, the codecs and `SyncedPlayer`. `default` enables all of them, so nothing
   changes for existing users.
2. **A hook before the handshake.** Bopsie's `/sendspin` route, like Music Assistant's proxy,
   expects an `auth` frame carrying an API token before `client/hello`. The builder accepts custom
   HTTP headers but cannot send a frame first. The hook is general: any message the transport
   must exchange before the Sendspin handshake.
3. **Lenient field names.** Accept `output_delay_ms` as an alias of `static_delay_ms` in
   `client/state`, as the server's `wire.rs` does for clients that send the older name.
4. **An output sink trait, when iOS starts.** Described below; not needed for macOS or Linux,
   where `cpal` is the right output.

### The output sink, for iOS and Android

`SyncedPlayer` opens a `cpal` stream itself, and its process callback only observes samples; it
cannot replace the output. That is right on macOS and Linux. On iOS the output has to live inside
an `AVAudioSession` the app manages (interruptions, route changes, background audio), and on
Android inside the `AudioTrack` and audio focus handling the media session owns. Driving those
from `cpal` fights the platform.

The fork therefore separates the synchronized renderer from the device that pulls from it. The
renderer keeps everything that makes playback correct: the buffer, scheduling against server time,
sync correction and gain. A device asks it for a number of frames and says when the first of them
will be heard:

- `cpal` stays the default device, so `SyncedPlayer` behaves as it does today.
- A platform supplies its own device by calling the renderer from its audio callback:
  `AVAudioEngine`'s source node on iOS, an `AudioTrack` write loop on Android. Across UniFFI this
  is a render function the platform calls, not a callback Rust calls, so the real-time path never
  crosses into a garbage-collected runtime.

This is designed when the iOS work begins, against what the iOS output actually needs, and goes
upstream on its own.

## The server shares the message types

The server's `players/sendspin` crate has its own copy of the wire in `wire.rs`. Once the fork has
its feature split, the server depends on `sendspin` with `default-features = false` and uses its
message types, so the 1.0 migration changes one set of types for client and server alike. What is
Music Assistant or Bopsie policy rather than protocol, such as `APP_PRODUCTS` and the conversion
into `bopsie-players` types, stays in the server crate.

This is evaluated before it is adopted: compare `cargo tree` and a clean build of the server
before and after. If the features do not keep client dependencies out of the server build, the
server keeps `wire.rs`.

## Layout

| Path | What |
|---|---|
| `player/` | A Cargo workspace for the client side. |
| `player/crates/client` | Bopsie's player: sign-in, the `auth` frame, reconnects, the persisted `client_id`, and a state and metadata model. Built on `sendspin`. |
| `player/crates/ffi` | The UniFFI surface over `client`: the only API Swift and Kotlin see. Small and stable, so changes in `sendspin` stop at this crate. |
| `player/crates/cli` | A headless player for Linux and for development. |
| `macos/` | The menu bar app: an XcodeGen project, Swift sources, and the script that builds the Rust XCFramework. |

The client is its own workspace rather than part of `server/`, so building the server never
compiles audio output and building the player never compiles the server.

### The client crate

Sign-in follows the app's flow in `packages/client`: `auth/login` over `/ws`, then
`auth/token/create` for a long-lived token, which is what the player stores and sends in its
`auth` frame. The client says hello with the `player`, `controller` and `metadata` roles.

The `product_name` decides whether the server treats the player as one that can be played to.
Today the server's `APP_PRODUCTS` lists the browser, web player, mobile and PWA products; the
desktop and headless players need a product of their own added there.

The FFI surface is roughly: sign in, connect with a stored token, disconnect, a stream of player
state and now-playing metadata, transport controls sent as `controller` commands, and volume.

## The macOS app

Swift and SwiftUI, macOS 14 or later:

- A `MenuBarExtra` with now playing, transport controls and volume.
- A settings window to choose a server and sign in, and to sign out.
- The token in the Keychain, launch at login through `SMAppService`.
- Now Playing and media keys through `MPNowPlayingInfoCenter` and `MPRemoteCommandCenter`.

Audio plays through `cpal` inside the Rust client.

**The project is generated by [XcodeGen](https://github.com/yonaskolb/XcodeGen)** from a
`project.yml`, and the generated `.xcodeproj` is not committed. The YAML is short and reviewable
as text, builds run from the command line with `xcodebuild`, and Xcode still opens the project for
debugging. A checked-in project was rejected because `project.pbxproj` is unreadable and edited
only through Xcode; Tuist because it is more tool than one app needs; a SwiftPM executable with a
hand-assembled bundle because signing, entitlements and assets would all be done by hand.

The Rust side reaches the app as an XCFramework: `cargo` builds both macOS architectures,
`uniffi-bindgen` generates the Swift bindings, `xcodebuild -create-xcframework` packages them. A
script in `macos/` does all three, and `project.yml` runs it before compiling. The same script
extends to iOS targets later.

Building requires Xcode and the Rust toolchain from the development shell.

### Signing and distribution

Ad-hoc signing first, enough to run the app on the machine that built it. Distribution follows
once the player works: Developer ID signing, notarization and an update mechanism.

## Testing

| Layer | What runs |
|---|---|
| Fork | The crate's own suites, plus tests for each change, as upstream would require |
| Client | Rust unit tests for state and reconnects; integration tests against the disposable Bopsie server container the mobile integration tests use |
| Server | The existing `players/sendspin` tests, unchanged, after the switch to shared message types |
| macOS | Manual at first: the app is a thin shell over the tested client |

## Roadmap

1. **Fork: features.** The feature split, offered upstream.
2. **Fork: handshake hook and field aliases.**
3. **Server: shared message types**, if the evaluation above holds.
4. **Client crate and CLI.** A headless player that signs in and plays from a Bopsie server.
5. **macOS app.** UniFFI surface, XcodeGen project, menu bar and settings window, ad-hoc signed.
6. **Distribution.** Developer ID, notarization, updates.
7. **iOS.** The output sink in the fork, then iOS playback on the shared client, replacing the
   planned Swift Sendspin output.
8. **Android.** Move `cratie-player` onto the shared client with an `AudioTrack` sink.
9. **Sendspin 1.0.** Noise and pairing, once upstream implements the finalized specification:
   one change in `sendspin`, picked up by every client and the server.
