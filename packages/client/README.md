# @bopsie/client

The Music Assistant API client shared by Bopsie (`mobile/`) and the server's web UI (`web/`):
the WebSocket connection and its commands, the response schemas, signing in and the stored
accounts, and the query keys and event handling for TanStack Query. It is plain TypeScript with
no React Native or DOM dependency; each app supplies its own storage and WebSocket.

It ships as source. Import modules by path, for example `@bopsie/client/api/client`.

```sh
pnpm typecheck
pnpm lint
pnpm test
```
