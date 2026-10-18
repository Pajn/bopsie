# Bopsie

A music app for iOS and Android, and the music server in Rust it plays from. They talk over the
[Music Assistant](https://www.music-assistant.io/) API and Sendspin, reused as Bopsie's protocol;
the app is built for Bopsie's server, not as a Music Assistant client.

- [`mobile`](mobile) — the app, built with React Native and Expo.
- [`server`](server) — the music server, a Cargo workspace.
- [`web`](web) — the server's web UI, for managing it, served at `/`.
- [`player`](player) — the Sendspin player desktop and headless clients share, in Rust, and a
  headless player built on it.
- [`macos`](macos) — Bopsie for Mac, a menu bar player on the shared player.
- [`packages/client`](packages/client) — the API client the app and the web UI share.
- [`docs/plans`](docs/plans) — architecture, stack choices and roadmaps.
- [`docs/research`](docs/research) — what the protocols and libraries look like, with sources.

The Nix flake at the root provides the development shell (`nix develop`, or `direnv allow`).

The JavaScript packages are one pnpm workspace: run `pnpm install` once, at the root.
