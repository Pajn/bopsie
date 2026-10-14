# Bopsie

A [Music Assistant](https://www.music-assistant.io/) client for iOS and Android, and a music
server in Rust that speaks the part of the Music Assistant API the client uses.

- [`mobile`](mobile) — the app, built with React Native and Expo.
- [`server`](server) — the music server, a Cargo workspace.
- [`web`](web) — the server's web UI, for managing it, served at `/`.
- [`packages/client`](packages/client) — the API client the app and the web UI share.
- [`docs/plans`](docs/plans) — architecture, stack choices and roadmaps.
- [`docs/research`](docs/research) — what the protocols and libraries look like, with sources.

The Nix flake at the root provides the development shell (`nix develop`, or `direnv allow`).

The JavaScript packages are one pnpm workspace: run `pnpm install` once, at the root.
