Responses from Music Assistant 2.10.6 serving the library `server/scripts/ma-server.sh` creates,
fetched over `POST /api`. The contract test in `../api.rs` checks that this server's responses to
the same commands have the same keys at every depth; values are not compared.

| File | Command |
|---|---|
| `album_get.json` | `music/albums/get` for First Album |
| `album_tracks.json` | `music/albums/album_tracks` for First Album |
| `artist_get.json` | `music/artists/get` for Test Artist |
| `search.json` | `music/search` for "Gamma" over artists, albums, tracks and playlists |
