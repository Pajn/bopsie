# Player

Bopsie's Sendspin player in Rust, which every client that plays audio builds on, and a headless
player made from it. The plan behind it is
[the shared Sendspin client plan](../docs/plans/2026-10-18-shared-sendspin-client.md).

- [`crates/client`](crates/client) (`bopsie-player`) — sign-in, the connection to a Bopsie
  server's `/sendspin` route, reconnection, playback, and the state a user interface shows.
- [`crates/cli`](crates/cli) (`bopsie-player`, the binary) — a headless player.

The protocol, clock synchronization and synchronized output come from the
[`sendspin`](https://github.com/Sendspin/sendspin-rs) crate, through a fork that carries changes on
their way upstream. The workspace pins the fork to a commit.

## The headless player

```sh
cargo run --release -- login music.local --username alice
cargo run --release
```

`login` asks for the password, or reads it from standard input when that is not a terminal, and
keeps a long-lived token in `bopsie-player/config.json` in the user's configuration directory
(`--config` or `BOPSIE_PLAYER_CONFIG` to put it elsewhere). The player then shows up on the server
under its name (`--name`, `Bopsie Player` by default) and plays to the default output device until
interrupted. `logout` forgets the sign-in. `RUST_LOG` sets what it logs.

On Linux, building needs the ALSA development headers (`libasound2-dev` on Debian and Ubuntu).

## Tests

```sh
cargo test
```

The tests that need a server run against the throwaway one the mobile integration tests use, and
pass without running when it is not there:

```sh
../mobile/scripts/test-server.sh start
eval "$(../mobile/scripts/test-server.sh env)"
cargo test
```

They play muted.
