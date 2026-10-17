# Bopsie web

The server's web UI: a management view of what the server knows. It lists the players with
everything the server knows of each (facts, capabilities, quirks and where each came from,
health, the protocol's own state, and the plan a group plays by), browses the library and the
queues, tails the server's log, and has every control the API has. It is for admins: it reads
the server's `admin/*` commands, which other users may not.

It is an Expo app, built for the web only, on Bopsie's stack (Expo Router, unistyles, TanStack
Query) and its API client, [`@bopsie/client`](../packages/client), so screens can move between
the two. Parts the browser does better than views, such as the log and raw JSON, are DOM
components (`"use dom"`).

The server serves it at `/` and carries it in its binary, from `dist/`. See
[the server's README](../server/README.md#running).

## Development

`pnpm install` once at the repository's root, then, from `web/`:

```sh
pnpm export        # build dist/, which a debug build of the server serves on reload
pnpm typecheck
pnpm lint
pnpm test
```

The app signs in to the server that serves it. To work on it with a development server, which
reloads as you edit, name a server for it to sign in to instead:

```sh
EXPO_PUBLIC_SERVER_URL=https://music.example.com pnpm start
```

It talks to the server over its WebSocket and loads artwork from it, neither of which the
browser restricts across origins, so any server you can sign in to will do.

### Smoke tests

`e2e/` drives the UI in Chromium, at a desktop's width and a phone's: signing in, playing an
album, adding one to a queue and moving a track, grouping two speakers, and opening an artist.
They run against the throwaway test server the app's integration tests use, whose image carries
this checkout's web UI, with two Sendspin speakers the tests connect from Node. They need
docker or podman.

```sh
pnpm exec playwright install chromium     # once
../mobile/scripts/test-server.sh start
eval "$(../mobile/scripts/test-server.sh env)" && pnpm test:e2e
../mobile/scripts/test-server.sh stop
```

Without `TEST_SERVER_URL` they are skipped.
