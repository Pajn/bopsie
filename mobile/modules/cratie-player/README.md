# Android player

A local Expo module that makes the phone a player of the Bopsie server. Expo autolinks it from
`modules/`; generated Android projects do not contain the source of truth.

`PlayerService` is a Media3 `MediaSessionService`. It owns:

- `SendspinClient`: the WebSocket to the server's `/sendspin` route. It authenticates with
  the account token, says hello as a `player`, `controller` and `metadata` client, and reports
  itself available only once clocks are synchronized. It reconnects with backoff, a bounded number
  of times, and stops for good when the server refuses the token.
- `TimeFilter`: a port of Sendspin's Kalman filter over clock offset and drift. It is fed by bursts
  of eight `client/time` exchanges every ten seconds; every sample of the first burst goes in, so
  the player is ready within a few round trips.
- `AudioOutput` and `Scheduler`: PCM to an `AudioTrack`. Each chunk names the server time at which
  its first sample must be heard. The scheduler converts that to local time and compares it with
  when the next written frame will be heard, from the track's hardware timestamp. Large gaps are
  closed in one step with silence or by dropping late audio; small ones by dropping or repeating
  frames, at most 0.5 % of a chunk. When the output is about to run dry it writes silence, so the
  timeline never stalls.
- A `SimpleBasePlayer` that shows the stream to the system: metadata from the `metadata` role,
  controls sent back as Sendspin controller commands. Notification, lock-screen and media-button
  controls therefore work without the JavaScript runtime.

When the app controls another player, and the phone plays nothing itself, the session shows that
player instead. The app follows it through the server's API and hands its state over with
`showRemote`; there is no Sendspin connection to that player for the service to follow. The
session declares remote playback with the player's volume in twenty steps of five, so Android
can give it the volume keys, and its controls come back to the app as `command` events for it to
send. These controls need the JavaScript runtime, so the service stops showing the other player
when the app's task is removed.

The service also takes audio focus when a stream starts, pauses on the server when focus is lost
or headphones are unplugged, and holds a partial wake lock and a Wi-Fi lock while streaming.

The client id is created once per install and stored in shared preferences. The server uses
it as the player id and the queue id, which is how the app addresses the phone through the API.

Where the wire format comes from is described in
[the research notes](../../../docs/research/music-assistant-and-sendspin.md).

## Verification

```sh
pnpm prebuild
pnpm test:native            # protocol, time filter, scheduler, metadata and remote state
pnpm test:integration       # the server's side of the same protocol, against a container
pnpm e2e:build:android && pnpm e2e:android
```
