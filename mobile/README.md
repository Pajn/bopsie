# Bopsie

A [Music Assistant](https://www.music-assistant.io/) client for iOS and Android, built with
React Native and Expo. A sister project to Shelfie, the Audiobookshelf client, sharing its stack,
structure and testing strategy.

Architecture, stack choices and the roadmap live in [`docs/plans`](../docs/plans). What the
Music Assistant API and the Sendspin protocol look like on the wire, with sources, is in
[`docs/research`](../docs/research).

What works today: sign in to Music Assistant 2.7.1 or later with a username and password; browse
albums, artists and playlists; search; and, on Android, play on the phone itself. The phone joins
Music Assistant as a player over Sendspin, so the server keeps its queue and other clients see it,
with notification and lock-screen controls. iOS browses but does not play yet.

The app also controls any other Music Assistant player. It works on one active group at a time —
the phone until you choose another — and the speaker button on the mini player opens the list of
players: set each member's volume or the group's, remove members, and for any other player choose
to join it to the group, move the queue to it, or control it instead. On iOS this is how music
plays.

The full player opens the group's queue: play any track in it, drag tracks into a new order, move
one up to play next, or remove it. The track playing, and the next one once the player has it, stay
where they are, as Music Assistant requires.

## Requirements

- Node 24 and pnpm 12. The Nix dev shell provides both.
- Android: the JDK and Android SDK from the Nix dev shell (`nix develop`, or `direnv allow`).
  `nix develop .#emulator` adds an emulator and a system image.
- iOS: Xcode with an iOS simulator.
- A Music Assistant server, 2.7.1 or later. Integration and end-to-end tests start their own.

## Development

Every command in this README runs from `mobile/`.

```sh
pnpm install
pnpm prebuild          # generates ios/ and android/ (not committed)
pnpm android           # build and run the dev client on a device or emulator
pnpm ios               # build and run the dev client on a simulator
pnpm start             # Metro, once a dev client is installed
```

Run `pnpm prebuild` when something native changes — a dependency, a config plugin, `app.json`,
the player module's manifest — and not otherwise.

Checks:

```sh
pnpm typecheck
pnpm lint
pnpm test              # JavaScript unit tests
pnpm test:native       # Kotlin unit tests for the native modules, after prebuild
pnpm check:icons       # the generated icon assets match the installed Feather font
```

## App identity

Bopsie's launcher icon lives in [`assets/brand`](assets/brand). The app uses `bopsie://` links;
the former `cratie://` scheme remains supported for existing links. The iOS bundle identifier
and Android package remain `io.github.pajn.cratie` so updates retain installed app data.
The internal `cratie-player` and `cratie-icons` module names also remain stable.

## Integration tests

The API layer, and the server's side of Sendspin, are tested against a real Music Assistant server
without the app. A disposable server runs in a container (docker or podman), with a small library
of generated tones:

```sh
pnpm ma:start                          # start the server, create a user, add a library
eval "$(scripts/ma-server.sh env)"     # export MA_URL, MA_USERNAME, MA_PASSWORD
pnpm test:integration
pnpm ma:stop
```

Point `MA_URL`, `MA_USERNAME` and `MA_PASSWORD` at any other server to test against it instead.
Without `MA_URL` the integration suites are skipped.

## End-to-end tests

Detox drives a Release build on an Android emulator against the same container. Playback is
Android-only for now, so Android is the only configuration.

```sh
nix develop .#emulator
avdmanager create avd -n Pixel_8_API_36 -d pixel_8 -k "system-images;android-36;google_apis;arm64-v8a"
pnpm ma:start
pnpm e2e:build:android && pnpm e2e:android
```

Detox boots the virtual device itself; `E2E_AVD_NAME` selects a different one. On Intel hosts the
flake selects the `x86_64` image; use that name in place of `arm64-v8a`. The emulator reaches the
test server through the host alias `10.0.2.2`, which the tests use on their own.

Tests are written with the autowaiting actions in `e2e/actions.ts`. Detox synchronization is off
because the player keeps timers running, so every action and assertion waits for its own target.
