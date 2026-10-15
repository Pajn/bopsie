# Bopsie

A [Music Assistant](https://www.music-assistant.io/) client for iOS and Android, built with
React Native and Expo. A sister project to Shelfie, the Audiobookshelf client, sharing its stack,
structure and testing strategy.

Architecture, stack choices and the roadmap live in [`docs/plans`](../docs/plans). What the
Music Assistant API and the Sendspin protocol look like on the wire, with sources, is in
[`docs/research`](../docs/research).

What works today: sign in to Music Assistant 2.7.1 or later with a username and password; open on a
home of what came into the library lately, its playlists and artists, and albums to rediscover;
browse albums, artists, tracks and playlists, sorted, filtered and as a grid; search; and, on
Android, play on the phone itself. The phone joins Music Assistant as a player over Sendspin, so the
server keeps its queue and other clients see it, with notification and lock-screen controls. iOS
browses but does not play yet.

The app also controls any other Music Assistant player. It works on one active group at a time — the
phone until you choose another — and the speaker button on the mini player, or the pill under the
full player's track, which grows into it, opens the list of players: set each member's volume or the
group's, remove members, and for any other player choose to join it to the group, move the queue to
it, or control it instead. On iOS this is how music plays. On Android, the player the app controls
shows in the notification and on the lock screen, as the phone's own playing does, and its volume
follows the volume keys while it plays.

Tap the mini player, or pull it up, to draw the full player up over the screen; drag its header
or artwork down, or press back, to put it away again.

The bar at the foot of the full player says what plays next, and grows into the group's queue
over the player when tapped or dragged up; drag its header down, or press back, to close it. In
the queue, play any track, drag tracks into a new order, move one up to play next, or remove it. The track playing, and the next one once the player has it, stay
where they are, as Music Assistant requires.

Press and hold a track, album, artist or playlist, or use its more button, to play it next, add it
to the end of the queue, or go to its album or artists.

The full player shuffles and repeats the queue, or the track playing. Shuffle beside an album's,
artist's or playlist's Play button turns shuffle on and plays it.

An artist's page lists the artist's tracks, or for a Spotify artist the most popular, then the
albums, newest first. A Spotify artist's singles and EPs, and compilations, are listed apart.

## Requirements

- Node 24 and pnpm 12. The Nix dev shell provides both.
- Android: the JDK and Android SDK from the Nix dev shell (`nix develop`, or `direnv allow`).
  `nix develop .#emulator` adds an emulator and a system image.
- iOS: Xcode with an iOS simulator.
- A Music Assistant server, 2.7.1 or later. Integration and end-to-end tests start their own.

## Development

Every command in this README runs from `mobile/`, apart from `pnpm install`, which runs once
for the whole workspace from the repository's root.

```sh
pnpm install           # from the root
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

## Startup bench

`scripts/perf/startup.ts` measures cold starts on an Android device or emulator: it installs a
release APK, launches the app repeatedly with the process killed in between, samples the process's
CPU per thread on the device, and reads the app's startup marks (`[bench] mark …` lines from
`src/lib/bench.ts`) and the activity manager's milestones from the device log. It reports medians
with their interquartile range and can compare two results. Results and per-launch logs go in
`perf/`, which is not committed.

```sh
pnpm perf:startup:android -- --apk android/app/build/outputs/apk/release/app-release.apk --out perf/before.json
pnpm perf:startup:android -- --compare perf/before.json perf/after.json
```

`--runs` sets the number of launches (10 by default) and `--device` the device's serial when
several are attached. The app has to be signed in on the device already, and the device unlocked:
a launch behind the lock screen never draws, so the bench refuses to measure one.

The marks follow a launch from the JavaScript entry through the account and its token being read,
the API connection authenticating, and Home rendering, receiving its shelves and drawing their
artwork.

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
