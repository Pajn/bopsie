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

The app signs in to the server that serves it, so open it through the server rather than a
separate dev server.
