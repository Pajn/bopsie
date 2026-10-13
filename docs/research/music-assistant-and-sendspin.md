# Music Assistant API and Sendspin for a mobile client

Research notes, 2026-10-12. Scope: what a mobile client needs to (a) sign in to a Music
Assistant (MA) 2.7+ server and drive it over the WebSocket command API, and (b) register as a
Sendspin player and play audio locally. Primary sources only (MA server/models/frontend source,
the Sendspin spec and its reference library). Where a statement was confirmed against a running
server, it is marked **(verified on 2.10.6)**.

## Versions this is based on

| Component | Version | Commit |
|---|---|---|
| MA server, latest stable | [2.10.6](https://github.com/music-assistant/server/releases/tag/2.10.6) (2026-10-09) | [`c248664`](https://github.com/music-assistant/server/tree/c24866405bd2068c25a60e1014e5ef8c3f6445bb) |
| MA server, `dev` (2.11 nightlies) | `2.11.0.dev2026101203` | [`4f1a86c`](https://github.com/music-assistant/server/tree/4f1a86cd558cded72995fac9edc926a2e7c955df) |
| `music-assistant-models` pinned by 2.10.6 | [1.1.205](https://github.com/music-assistant/models/tree/1.1.205) | [`99df2a5`](https://github.com/music-assistant/models/tree/99df2a566be5a58150d13c08823069e382b516ce) (dev pins 1.1.221) |
| `aiosendspin` pinned by 2.10.6 | [9.1.1](https://github.com/Sendspin/aiosendspin/tree/9.1.1) | [`5c024b4`](https://github.com/Sendspin/aiosendspin/tree/5c024b42893bc6e372f6c87bb8504552051daf24) (dev pins [10.0.1](https://github.com/Sendspin/aiosendspin/tree/f6fd47fe5cd8c94d9cab59efd07e536b2066c035)) |
| Sendspin spec | [1.0.0-rc1](https://github.com/Sendspin/spec/releases/tag/v1.0.0-rc1) (2026-09-17) | [`671a34d`](https://github.com/Sendspin/spec/tree/671a34d408581fc25ea56b3528a4a3f13e3be901); `main` at [`ee8aad9`](https://github.com/Sendspin/spec/tree/ee8aad9660fc57345e726a888486f47f66a41f97) |
| Sendspin time filter | `main` | [`39dd3f4`](https://github.com/Sendspin/time-filter/tree/39dd3f4a7c37bcd63d8e2ee3885bf4d76fcaa61a) |
| MA frontend shipped with 2.10.6 | 2.17.297 | — |

`aiosendspin` versions shipped per MA release, which matters for Sendspin compatibility:

| MA | aiosendspin | Notes |
|---|---|---|
| 2.7.0 | 1.1.4 | no `/sendspin` proxy route yet |
| 2.7.1 | 1.1.4 | `/sendspin` proxy added |
| 2.7.11 | 2.0.0 | |
| 2.8.0 | 4.4.0 | |
| 2.9.x | 6.0.5 | |
| 2.10.0 – 2.10.6 | 9.1.1 | Noise-encrypted protocol introduced; legacy unencrypted clients still accepted |
| dev (2.11) | 10.0.1 | 13-byte audio header for encrypted clients; legacy clients still get 9 bytes |

---

## Part 1 — Music Assistant API

### 1.1 Endpoints and ports

| Purpose | Path / port |
|---|---|
| WebSocket command API | `ws(s)://<host>:8095/ws` |
| HTTP JSON-RPC (same commands, one per request) | `POST /api` on 8095 |
| Server info (unauthenticated) | `GET /info` on 8095 |
| HTTP login | `POST /auth/login` on 8095 |
| First-run setup (create admin) | `POST /setup` on 8095 |
| Image proxy | `GET /imageproxy/<proxy_id>?size=&fmt=` on 8095 (also on stream server 8097) |
| Sendspin proxy (authenticated) | `ws(s)://<host>:8095/sendspin` (2.7.1+) |
| Sendspin server (direct) | `ws://<host>:8927/sendspin` |
| Live API docs | `GET /api-docs`, `/api-docs/commands.json`, `/api-docs/schemas.json`, `/api-docs/openapi.json` |

Routes: [`controllers/webserver/controller.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/webserver/controller.py) (`/ws`, `/api`, `/auth/*`, `/setup`, `/sendspin`, `/api-docs/*`). Port 8095 is `CONF_BIND_PORT`'s default; the Docker image `EXPOSE`s 8095. `/api-docs/commands.json` returns every command with its parameters, return type, `authenticated` flag and `required_scope`, and is the most practical source for exact signatures. 2.10.6 registers 309 commands.

### 1.2 Message envelopes

Defined in [`music_assistant_models/api.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/api.py). All frames are JSON text frames.

**Server info**, sent unsolicited as the first frame on every `/ws` connection (`ServerInfoMessage`) **(verified on 2.10.6)**:

```json
{"server_id":"9939483a70104c87b36aa8a578ee28bc","server_version":"2.10.6","schema_version":65,
 "min_supported_schema_version":28,"base_url":"http://10.88.0.33:8095","homeassistant_addon":false,
 "onboard_done":true,"name":"Music Assistant (e8bd08acf5fe)","status":"running",
 "internal_url":"http://10.88.0.33:8095","external_url":null,"has_remote_access":false}
```

- `name`/`status` were added in schema 29 (MA 2.8); `internal_url`/`external_url`/`has_remote_access` in schema 32 (MA 2.10). `base_url` is deprecated but still emitted as an alias of `internal_url`.
- `internal_url`/`base_url` is the address the server *thinks* it has. In a container this is often an address the client cannot reach, so build URLs (imageproxy, `/sendspin`) from the URL the client actually connected to.
- Identify this frame by the presence of `server_version`. (The models' own `parse_message()` keys on `sdk_version`, which the server never sends; do not copy that.)
- If no user exists yet, the server sends server info, then `{"message_id":"connection","error_code":503,"details":"Setup required"}` and closes ([`websocket_client.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/webserver/websocket_client.py)).

**Command** (client → server), `CommandMessage`:

```json
{"message_id": "42", "command": "player_queues/play_media", "args": {"queue_id": "...", "media": "library://album/1"}}
```

`message_id` is any string chosen by the client; `args` keys are the Python parameter names (see `commands.json`).

**Success result**, `SuccessResultMessage`:

```json
{"message_id": "42", "result": <any>, "partial": false}
```

Commands implemented as async generators (e.g. `music/playlists/playlist_tracks`) stream: the server sends batches of 500 items as `{"message_id":..., "result":[...], "partial": true}` and a final message with `partial: false` holding the remainder. The client must concatenate all batches for one `message_id` and only resolve on `partial: false` ([`websocket_client.py` `_run_handler`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/webserver/websocket_client.py)). Over HTTP `/api` the server collects the whole list instead.

**Error result**, `ErrorResultMessage`:

```json
{"message_id": "5", "error_code": 12, "details": "Invalid or unsupported command."}
```

`details` is localised when the connection has a locale. `translation_key`/`translation_args`/`translation_owner` are stripped when the server is localising. Error codes ([`errors.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/errors.py)):

| code | class | code | class |
|---|---|---|---|
| 0 | MusicAssistantError | 14 | InvalidProviderURI |
| 1 | ProviderUnavailableError | 15 | InvalidProviderID |
| 2 | MediaNotFoundError | 16 | RetriesExhausted |
| 3 | InvalidDataError | 17 | ResourceTemporarilyUnavailable |
| 4 | AlreadyRegisteredError | 18 | ProviderPermissionDenied |
| 5 | SetupFailedError | 19 | ActionUnavailable |
| 6 | LoginFailed | 20 | AuthenticationRequired |
| 7 | AudioError | 21 | AuthenticationFailed |
| 8 | QueueEmpty | 22 | InsufficientPermissions |
| 9 | UnsupportedFeaturedException | 23 | InvalidToken |
| 10 | PlayerUnavailableError | 24 | ResourceBusyError |
| 11 | PlayerCommandFailed | 25 | RateLimited |
| 12 | InvalidCommand | 26 | UnsupportedSystemError |
| 13 | UnplayableMediaError | 27 | UserNotFoundError |

Unexpected server exceptions use `999` (or the exception's own `error_code`).

**Event** (server → client), `MassEvent` ([`event.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/event.py)):

```json
{"event": "queue_updated", "object_id": "<queue_id>", "data": { ...PlayerQueue... }}
```

There is no subscribe command: a `/ws` connection receives all events once it has authenticated. Events for players and queues are filtered by the user's `player_filter` (if non-empty), except for the private Sendspin player the session registered via the proxy (see 2.3).

### 1.3 Authentication (2.7+)

Auth arrived with schema 28 (MA 2.7.0); `min_supported_schema_version` is 28. Source: [`controllers/webserver/auth.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/webserver/auth.py), [`websocket_client.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/webserver/websocket_client.py), [`helpers/auth_middleware.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/webserver/helpers/auth_middleware.py).

**Login over WebSocket** (`auth/login`, `authenticated=False`):

```json
{"message_id":"1","command":"auth/login","args":{"username":"alice","password":"…","device_name":"Bopsie on iPhone"}}
```

- Optional `provider_id` (default `"builtin"`); extra keys are passed to the provider as credentials.
- Success result: `{"success": true, "access_token": "<JWT>", "user": {"user_id","username","display_name","role"}}`.
- **Failure is a success-shaped result, not an error frame** **(verified on 2.10.6)**: `{"result": {"success": false, "error": "Invalid username or password"}}`. Check `result.success`.
- Logging in does **not** authenticate the connection. Send `auth` next.

**Authenticate the connection** (special-cased, `auth`):

```json
{"message_id":"2","command":"auth","args":{"token":"<JWT>","locale":"en"}}
```

- `access_token` is accepted as an alias for `token`. `locale` is optional and sets the language for localised `details`/titles.
- Result: `{"authenticated": true, "user": { ...User... }}`. Failure: error frame with code 20 (no token) or 23 (invalid/expired).
- Any command whose handler needs auth, sent before `auth`, returns error 20 `"Authentication required. Please send auth command first."`. Scope failures return 22.
- After a successful `auth` the connection is subscribed to events.

**Login over HTTP**: `POST /auth/login` with body `{"provider_id":"builtin","credentials":{"username":"…","password":"…"},"device_name":"…"}` → `{"success": true, "token": "<JWT>", "user": {...}}`; 401 with `{"success": false, "error": ...}` on bad credentials **(verified on 2.10.6)**. Note the nested `credentials` object here versus flat args on the WS command.

**Token kinds** (`AuthToken.is_long_lived`, [`auth.py` model](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/auth.py)):

| Kind | Created by | Lifetime |
|---|---|---|
| Session ("short-lived") | `auth/login`, `POST /auth/login`, `POST /setup` | 30 days, sliding (renewed on each use), capped at 90 days from creation (`TOKEN_ABSOLUTE_MAX_EXPIRATION`) |
| Guest session | login of a `guest`-role user | 1 day, no renewal |
| Long-lived | `auth/token/create` `{"name": "...", "user_id"?: "..."}` → returns the token **string** | 365 days, no renewal; cannot be created for guest users |

Tokens are JWTs (HS256, `sub` = user_id, `jti` = token_id); the server's database is the source of truth for expiry and revocation. Other useful commands: `auth/me`, `auth/tokens`, `auth/token/revoke {token_id}`, `auth/logout`, `auth/providers`. A mobile app should keep the session token from login (it slides while the app is used) or explicitly mint a long-lived token after login; both are presented the same way.

**Presenting a token**:
- WebSocket `/ws`: the `auth` command (above).
- HTTP (`/api`, `/auth/me`, …): `Authorization: Bearer <token>`. `/api` without it returns `401 Authentication required` **(verified on 2.10.6)**.
- Sendspin proxy `/sendspin`: first text frame `{"type":"auth","token":"<token>","client_id":"<sendspin client_id>"}` (see 2.3).
- The `homeassistant` system user is rejected on the regular webserver. HA Ingress uses `X-Remote-User-*` headers and is out of scope for a mobile client.

**Scopes** (`Scope` enum) gate commands: `library.read`, `players.read`, `players.control`, `queues.read`, `queues.control`, `config.providers.write`, … Every command's `required_scope` is listed in `commands.json`.

### 1.4 Artwork (imageproxy)

`MediaItemImage` ([`media_items/metadata.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/media_items/metadata.py)) has `type`, `path`, `provider`, `remotely_accessible`, and — when serialized by the server — `proxy_id` (64 lowercase hex chars, `sha256(provider+path)`). Images live in `item.metadata.images[]` (and `QueueItem.image`).

Current form (2.10.x, [`controllers/metadata/images.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/metadata/images.py)):

```
GET <base>/imageproxy/<proxy_id>?size=<0|80|160|256|512|1024>&fmt=<jpeg|png|…>
```

- Registered on both the webserver (8095) and the stream server (8097). No `Authorization` needed: the id is opaque and resolved server-side.
- `size` must be one of `0, 80, 160, 256, 512, 1024` (0 = original); anything else → 400. A malformed id → 400, an unknown id → 404.
- If `remotely_accessible` is true the raw `path` is a public URL, but the proxy is still the way to get a resized thumbnail.
- `metadata/get_image_palette {image_id}` returns a colour palette (`MediaItemPalette`) for a proxy id.

Legacy form, 2.7.x ([`metadata.py` at 2.7.11](https://github.com/music-assistant/server/blob/2.7.11/music_assistant/controllers/metadata.py)): `GET <stream_base>/imageproxy?provider=<provider>&size=<n>&fmt=<jpg|png>&path=<path, URL-encoded twice>`, registered **only on the stream server (8097)**. On 2.10.6 `GET /imageproxy?provider=…&path=…` returns 400 **(verified on 2.10.6)**. A client that supports both: use `proxy_id` when present, otherwise fall back to the legacy query form on the stream port.

### 1.5 Commands a library/player client needs

Signatures are taken from `/api-docs/commands.json` on 2.10.6. `?` = optional. MA `item_id` + `provider_instance_id_or_domain` pairs identify media items. For library items, use `"library"` as the provider. Every item also has a `uri` (`library://album/1`, `spotify://track/…`) that `play_media` and `music/item_by_uri` accept.

**Library listings** (`library.read`). All four share the signature from [`controllers/music/media/base.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/music/media/base.py):

```
music/artists/library_items   (favorite?, search?, limit?=500, offset?=0, order_by?="sort_name", provider?, genre?, played_only?, album_artists_only?, artist_type?, summary?, reachable_via?)
music/albums/library_items    (… album_types?, …)
music/tracks/library_items    (… explicit?, …)
music/playlists/library_items (… collapse_collections?, …)
music/<type>/count            (favorite?, album_artists_only? …) -> int
```

- Pagination is `limit` + `offset`; the result is a plain array (no total). Use `music/<type>/count` for totals, or page until a short page comes back.
- `order_by` keys: `name`, `sort_name`, `duration`, `timestamp_added`, `timestamp_modified`, `last_played`, `play_count`, `year`, `position`, `album_artist_name`, `track_artist_name` (each also with `_desc`), plus `random` and `random_play_count`.
- `summary: true` returns lighter objects suitable for grids.

**Drill-down**:

```
music/albums/get             (item_id, provider_instance_id_or_domain, allow_update_metadata?, recursive?) -> Album
music/albums/album_tracks    (item_id, provider_instance_id_or_domain, in_library_only?) -> Track[]
music/artists/artist_albums  (item_id, provider_instance_id_or_domain, provider_filter?) -> Album[]
music/artists/artist_tracks  (item_id, provider_instance_id_or_domain, …) -> Track[]
music/playlists/playlist_tracks (item_id, provider_instance_id_or_domain, force_refresh?, allow_dynamic_tracks?) -> streamed (partial) Track[]
music/item_by_uri            (uri, allow_update_metadata?) -> MediaItem
```

**Search**: `music/search (search_query, media_types?, limit?, library_only?, providers?)` → `SearchResults` object with arrays `artists`, `albums`, `tracks`, `playlists`, `genres`, `radio`, `audiobooks`, `podcasts`, … **(verified on 2.10.6)**. `media_types` are `MediaType` strings (`"artist"`, `"album"`, `"track"`, `"playlist"`, …).

**Players** (`players.read` / `players.control`):

```
players/all (return_unavailable?, return_disabled?, provider_filter?, return_protocol_players?=false) -> Player[]
players/get (player_id, raise_unavailable?) -> Player
players/cmd/volume_set (player_id, volume_level:int 0-100)
players/cmd/volume_mute (player_id, muted:bool)
players/cmd/play|pause|play_pause|stop|next|previous (player_id)
players/cmd/seek (player_id, position)
```

`Player` key fields ([`player.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/player.py)): `player_id`, `provider`, `type` (`player`, `protocol`, `group`, …), `name`, `available`, `supported_features`, `playback_state` (`idle|paused|playing`), `elapsed_time`, `volume_level`, `volume_muted`, `group_members`, `synced_to`, `active_source`, `current_media`, `enabled`, `icon`. `protocol`-type players are hidden from `players/all` by default.

**Queues** (`queues.read` / `queues.control`). A player's queue id equals its `player_id` (unless it is synced to another player; then use `player_queues/get_active_queue {player_id}`).

```
player_queues/all () -> PlayerQueue[]
player_queues/get (queue_id) -> PlayerQueue | null
player_queues/get_active_queue (player_id) -> PlayerQueue
player_queues/items (queue_id, limit?, offset?) -> QueueItem[]
player_queues/play_media (queue_id, media, option?, radio_mode?, start_item?, sort_by?, start_from_beginning?, shuffle?)
player_queues/play | pause | play_pause | stop | next | previous (queue_id)
player_queues/seek (queue_id, position:int)       # absolute, in SECONDS
player_queues/skip (queue_id, seconds?=10)         # relative, seconds (negative = back)
player_queues/play_index (queue_id, index:int|queue_item_id, seek_position?, fade_in?)
player_queues/shuffle (queue_id, shuffle_enabled:bool)
player_queues/repeat (queue_id, repeat_mode:"off"|"one"|"all")
player_queues/clear | delete_item | move_item | resume | transfer …
```

`play_media`:
- `media`: one item or a list; each element may be a full media item object, an `ItemMapping`, or a URI string (`"library://album/1"`).
- `option` (`QueueOption`): `play` (insert at current position and play), `replace` (replace queue, play from index 0), `next` (insert after current), `replace_next` (replace everything after current), `add` (append). If omitted, the server applies the user's default enqueue option.
- `start_item`: a track (or its URI/id) inside the album/playlist to start from; `sort_by` orders tracks first; `shuffle` enables shuffle for this request.
- Returns `null` once the request is accepted. Playback starts asynchronously; watch events.

`PlayerQueue` fields ([`player_queue.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/player_queue.py)): `queue_id`, `active`, `display_name`, `available`, `items` (count), `shuffle_enabled`, `repeat_mode`, `current_index`, `index_in_buffer`, `elapsed_time` (float seconds) + `elapsed_time_last_updated` (epoch seconds), `state` (`idle|paused|playing`), `current_item`, `next_item`, `flow_mode`, `extra_attributes`. `QueueItem` ([`queue_item.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/queue_item.py)): `queue_id`, `queue_item_id`, `name`, `duration` (seconds), `sort_index`, `streamdetails`, `media_item`, `image`, `index`, `available`.

Progress display: `elapsed_time + (now - elapsed_time_last_updated)` while `state == "playing"`.

### 1.6 Events for live UI

`EventType` values ([`enums.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/enums.py)) relevant to a player UI, with observed payloads **(verified on 2.10.6)**:

| event | object_id | data |
|---|---|---|
| `player_added` / `player_updated` | player_id | full `Player` |
| `player_removed` | player_id | — |
| `queue_added` / `queue_updated` | queue_id | full `PlayerQueue` |
| `queue_items_updated` | queue_id | full `PlayerQueue` (refetch items with `player_queues/items`) |
| `queue_time_updated` | queue_id | float: elapsed seconds of the current item |
| `media_item_added/updated/deleted` | item uri | media item |
| `providers_updated`, `music_sync_completed`, `core_state_updated` | | |

`queue_time_updated` is frequent. Use it (or extrapolation from `elapsed_time_last_updated`) for the progress bar rather than re-rendering on each `queue_updated`.

### 1.7 Where the types live

Hand-written client types should mirror [`music-assistant/models`](https://github.com/music-assistant/models) (`music_assistant_models/`): `api.py` (envelopes), `event.py`, `enums.py` (`EventType`, `MediaType`, `QueueOption`, `RepeatMode`, `PlaybackState`, `ImageType`, …), `errors.py`, `auth.py`, `player.py`, `player_queue.py`, `queue_item.py`, `media_items/` (`media_item.py`, `metadata.py`, `provider_mapping.py`, `audio_format.py`), `setup_flow.py`, `config_entries.py`. JSON is produced by mashumaro, so field names equal the Python attribute names (snake_case). Enums are strings. `/api-docs/schemas.json` on a running server gives JSON schemas of the same models. The official TS reference is the frontend's [`src/plugins/api/interfaces.ts`](https://github.com/music-assistant/frontend/blob/main/src/plugins/api/interfaces.ts).

---

## Part 2 — Sendspin

### 2.1 Two protocol generations: pick the legacy (unencrypted) path for MA 2.7+

The spec in [Sendspin/spec](https://github.com/Sendspin/spec) (`README.md` is generated from `connection.md`, `messaging.md`, `pairing.md`, `roles/*`) at **1.0.0-rc1** mandates end-to-end **Noise** encryption (`KKpsk2`, Curve25519 identities as `client_id`, `client/init` → `server/init` → `noise/handshake` ×2, then encrypted binary frames carrying `server/hello` → `client/hello` → `server/activate`), plus pairing (PSK / dynamic or static pairing code, CPace). MA only gained this in **2.10.0** (aiosendspin 9.x), and 9.1.1 predates rc1 (it uses earlier names such as `dynamic_pin`, `static_delay_ms` and a 9-byte audio header).

Every MA release from 2.7.1 to current `dev` accepts a **legacy, unencrypted ("transition mode")** connection: if the first text frame is `client/hello` rather than `client/init`, aiosendspin admits it without Noise ([`server/connection.py` `_establish_transport`](https://github.com/Sendspin/aiosendspin/blob/9.1.1/aiosendspin/server/connection.py)). MA enables this through a hidden provider option `allow_legacy_clients`, default `True` on 2.10.6 and on `dev` ([`providers/sendspin/provider.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/providers/sendspin/provider.py)). Restrictions: a legacy hello may not claim a `client_id` that is already paired or approved for encrypted access (downgrade protection), and pairing requires encryption.

**Recommendation for a client targeting MA 2.7+:** implement the legacy wire described in 2.2–2.9, which is what MA's own servers speak to unencrypted clients today. Treat the Noise path as a later upgrade gated on server version ≥ 2.10. The rest of this part describes the legacy wire as MA 2.10.6 serves it **(verified on 2.10.6)**, noting where the rc1 spec differs.

### 2.2 Where to connect

| URL | Auth | Notes |
|---|---|---|
| `ws(s)://<ma-host>:8095/sendspin` | first frame `{"type":"auth",...}` | 2.7.1+. Same origin as the API, so it works through HTTPS reverse proxies and remote access. Recommended. |
| `ws://<ma-host>:8927/sendspin` | none | Direct to aiosendspin (`SENDSPIN_SERVER_PORT` 8927). LAN only, plain `ws`. |
| mDNS `_sendspin-server._tcp.local.` | — | Client-initiated discovery (TXT `path`, `name`). The server can also connect *to* clients advertising `_sendspin._tcp.local.` on port 8928; a phone app should not do that. |

**Proxy handshake** ([`controllers/webserver/sendspin_proxy.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/webserver/sendspin_proxy.py)):

1. Open `wss://host/sendspin`.
2. Within 10 s send text `{"type":"auth","token":"<MA token>","client_id":"<your Sendspin client_id>"}`.
3. Server replies `{"type": "auth_ok"}`, then transparently pipes frames (text and binary) to `ws://<internal>:8927/sendspin`.
4. Failures close with code **4001** and a reason (`Expected text message for auth`, `First message must be auth`, `Token required in auth message`, `Invalid or expired token`, `Authentication timeout`); an internal connect failure closes with 1011. The proxy sends WebSocket pings every 25 s.

`client_id` in the auth frame is optional but useful: the server marks it as that session's own player, so it is visible and controllable even when the user has a restrictive `player_filter` (`is_own_client_player` in [`controllers/players/helpers.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/players/helpers.py)). The association is applied to `/ws` connections that already hold the same token, so connect and authenticate `/ws` first.

### 2.3 Handshake (legacy wire)

All JSON messages are `{"type": "...", "payload": {...}}` text frames.

**Client → `client/hello`** (fields per [`aiosendspin/models/core.py` 9.1.1](https://github.com/Sendspin/aiosendspin/blob/9.1.1/aiosendspin/models/core.py), [`player.py`](https://github.com/Sendspin/aiosendspin/blob/9.1.1/aiosendspin/models/player.py)):

```json
{"type":"client/hello","payload":{
  "client_id":"<stable unique id, persisted>",
  "name":"Kitchen iPhone",
  "version":1,
  "supported_roles":["player@v1","controller@v1","metadata@v1"],
  "device_info":{"product_name":"Mobile Application","manufacturer":"<app vendor>","software_version":"1.0.0"},
  "player@v1_support":{
    "supported_formats":[
      {"codec":"flac","channels":2,"sample_rate":48000,"bit_depth":16},
      {"codec":"pcm","channels":2,"sample_rate":48000,"bit_depth":16}
    ],
    "buffer_capacity":1048576,
    "supported_commands":["volume","mute"]
  }}}
```

- `client_id` and `version: 1` are required on the legacy path (under Noise they come from `client/init`). Persist `client_id`: it becomes the MA `player_id` and queue id.
- **`device_info.product_name` decides how MA registers the player.** It must be one of `"Web Browser"`, `"Web Player"`, `"Mobile Application"`, `"PWA"` (or `manufacturer == "Music Assistant"`) for MA to register a standalone, private `PlayerType.PLAYER` with its own queue. Anything else (including no `device_info`) registers a hidden `protocol` player meant to be linked under a hardware device, and `play_media` on it fails with `PlayerUnavailableError` ("The player is not available") ([`providers/sendspin/player.py` `_refresh_client_info`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/providers/sendspin/player.py)) **(verified on 2.10.6)**.
- Support-object key: MA ≥ 2.7.11 accepts `player@v1_support` (spec name). MA 2.7.0–2.7.10 (aiosendspin 1.1.4) only understood `player_support`. Later versions rewrite `player_support` to the versioned key, so sending only `player@v1_support` is right for 2.7.11+.
- `supported_formats` is in priority order. The server picks the first entry it can produce: with FLAC listed first, MA streamed FLAC; with PCM first, PCM **(verified on 2.10.6)**. Players must list `flac` or `pcm`; `opus` is optional.
- `buffer_capacity`: max bytes of not-yet-played audio (header + payload per chunk) the server may have outstanding.
- `supported_commands` in hello is the legacy place for `volume`/`mute` (rc1 moved it into `client/state.player.supported_commands`).

**Server → `server/hello`** (legacy variant, `LegacyServerHelloPayload`):

```json
{"type":"server/hello","payload":{"server_id":"dP5BYQ9C…","name":"Music Assistant (…)","version":1,
 "connection_reason":"discovery","active_roles":["player@v1","controller@v1","metadata@v1"]}}
```

`active_roles` replaces rc1's `server/activate`; only send state/commands for roles listed. `connection_reason` is `discovery` or `playback`. MA then immediately sends `server/state` (for controller/metadata roles) and `group/update`.

**Client → initial `client/state`** right after `server/hello`, then on every change:

```json
{"type":"client/state","payload":{"available":true,
  "player":{"volume":50,"muted":false,"static_delay_ms":0,"required_lead_time_ms":200,"min_buffer_ms":200}}}
```

- `available` (bool) is the 2.10+ field. MA 2.7–2.9 expect the older top-level `"state": "synchronized" | "error" | "external_source"`. 2.10 still accepts `state` and maps `available = state != "external_source"`. Sending both keys works on all versions (unknown keys are ignored).
- `static_delay_ms` (0–5000), `required_lead_time_ms` (0–30000), `min_buffer_ms` (0–30000) exist from aiosendspin 6.x (MA 2.9). rc1 renames `static_delay_ms` to `output_delay_ms` (aiosendspin 10 accepts both).
- The server waits up to 5 s for the initial `client/state` before it treats the client as connected, and it will not send `stream/start` until `available` is true.

**Messages the server sends** (handle all, ignore unknown types and unknown fields):

| type | payload |
|---|---|
| `server/time` | `{client_transmitted, server_received, server_transmitted}` (µs) |
| `server/state` | `{metadata?, controller?, color?}`. MA sends **only changed fields** within `metadata` (e.g. just `title`, `track`, `progress` on a track change), so merge into the current state. rc1 requires full objects; aiosendspin 9.1.1 merges. |
| `group/update` | `{playback_state: "playing"|"stopped"(|"paused"), group_id, group_name?}`. Partial payloads are possible. |
| `stream/start` | `{server_transmitted, player: {codec, sample_rate, channels, bit_depth, codec_header?}}` |
| `stream/clear` | `{server_transmitted, roles?: ["player", …]}` |
| `stream/end` | `{server_transmitted, roles?: […]}` |
| `server/command` | `{player: {command: "volume"|"mute"|"set_static_delay", volume?, mute?, static_delay_ms?}}` |

### 2.4 Clock synchronization

- Client sends `{"type":"client/time","payload":{"client_transmitted": <client monotonic µs>}}`; server answers `server/time` echoing `client_transmitted` plus `server_received` and `server_transmitted` (server monotonic µs, not epoch).
- With T1 = client_transmitted, T2 = server_received, T3 = server_transmitted, T4 = client receive time (taken when the last byte arrived, before any app-level queueing): `offset = ((T2−T1)+(T3−T4))/2`, `max_error = ((T4−T1)−(T3−T2))/2`.
- The spec requires the [time-filter](https://github.com/Sendspin/time-filter) algorithm: a 2-D Kalman filter over offset and drift. API: `update(measurement=offset, max_error, time_added=T4)`, `compute_client_time(server_time)`, `compute_server_time(client_time)`, `get_error()`. Default config: `process_std_dev = 0`, `drift_process_std_dev = 1e-11`, `forget_factor = 2.0`, `adaptive_cutoff = 3.0`, `min_samples = 100`, `drift_significance_threshold = 2.0`, `max_error_scale = 0.5`. The math is in [`docs/theory.md`](https://github.com/Sendspin/time-filter/blob/main/docs/theory.md), the C++ reference in `cpp/`, and a Python port in [aiosendspin `client/time_sync.py`](https://github.com/Sendspin/aiosendspin/blob/10.0.1/aiosendspin/client/time_sync.py).
- Recommended cadence (time-filter README): bursts of ~8 sequential exchanges (each waiting for its reply) every ~10 s, feeding only the lowest-`max_error` sample of each burst into `update()`.
- Use a monotonic clock (`CLOCK_MONOTONIC`/`mach_continuous_time`/`SystemClock.elapsedRealtimeNanos`), never wall-clock time.
- A player must not report `available: true` until the filter has converged enough to schedule. aiosendspin's own criterion is ≥ 2 samples and finite covariance.

### 2.5 Binary audio chunks

Type 4, server → client, binary frame **(verified on 2.10.6)**:

| Bytes | Legacy wire (all MA 2.7–2.10; legacy clients on 2.11-dev) | rc1 / encrypted clients on aiosendspin ≥ 10 |
|---|---|---|
| 0 | `uint8` message type = `4` | same |
| 1–8 | `int64` **big-endian**, server-clock **µs** when the chunk's first sample must leave the audio port | same |
| 9–12 | — | `uint32` big-endian `send_ahead` µs (transmit → timestamp; 0 and 0xFFFFFFFF = "not measured") |
| rest | encoded audio | same (from byte 13) |

aiosendspin: `BINARY_HEADER_FORMAT = ">Bq"` (9 bytes) in 1.1.4 through 9.1.1; 10.0.1 adds `PLAYER_AUDIO_HEADER_FORMAT = ">BqI"` but still sends `>Bq` to legacy-hello clients (`uses_pre_spec_177_wire`). Observed on 2.10.6: PCM 16-bit stereo 44.1 kHz chunks of 4417 bytes = 9 + 4408 (1102 frames ≈ 25 ms). The rc1 bounds are 15–150 ms per chunk.

Other binary IDs (ignore if the role is not active): 8–11 artwork channels 0–3, 16–20 visualizer, 1 fragmentation (Noise path only).

### 2.6 Codecs and `codec_header`

Per spec rc1 "Codec framing" (the observed 2.10.6 behaviour matches):

- **pcm**: little-endian signed integers, interleaved, 24-bit packed as 3 bytes. Each chunk is a whole number of frames. No `codec_header`.
- **flac**: each chunk is one or more complete FLAC frames (starts with sync `0xFFF8…`). `codec_header` is **standard Base64** of the `fLaC` marker followed by the STREAMINFO metadata block. Observed: `ZkxhQ4AAACIS…` = `66 4C 61 43` (`fLaC`) + `80 00 00 22` (last-block flag, type 0 STREAMINFO, length 34) + 34 bytes. Feed header + frames to a FLAC decoder (e.g. libFLAC stream decoder, or prepend the header to make a valid FLAC stream).
- **opus**: exactly one raw Opus packet per chunk (no Ogg). No `codec_header`; configure the decoder from `sample_rate`/`channels`.
- When `stream/start` arrives during an active stream, it changes format in place. Decode each chunk with the format that was current when it arrived, and keep the timeline continuous.

### 2.7 Scheduling, buffer capacity, sync

From spec rc1 "Playback Synchronization" and "Player Buffer Accounting":

- Play time on the local clock: `local = time_filter.compute_client_time(chunk.timestamp) − output_delay` (legacy: `static_delay_ms`). Also compensate for known output latency (e.g. `AVAudioSession.outputLatency` + IO buffer, Android `AudioTrack` latency/timestamps) so the sample *leaves the device* at that time.
- Drop chunks whose time is already past. Startup / `stream/start` / `stream/clear` / underrun: snap in one shot (drop the late prefix, or pad with silence). No pitch warble at startup.
- Steady state: keep error within ±1 ms (target ±0.5 ms) of the filter's prediction; speed deviation ≤ ±0.5 % over 150 ms. Suggested strategy: per chunk, if |error| > ~100 µs, drop or duplicate `N = max(1, round(21 µs × rate / 1e6))` frames (1 at 44.1/48 kHz), capped at 0.5 % of the chunk. ASRC is also allowed.
- Server side (informative): first chunk after `stream/start`/`stream/clear` is scheduled at least `min_buffer_ms + output_delay` ahead (MA used ≈ `required_lead_time_ms`: lead 197 ms with `required_lead_time_ms: 200`, **verified on 2.10.6**). For buffered sources it fills toward `buffer_capacity`.
- **Buffer accounting**: the server counts (header + payload) bytes of every chunk whose end time (`timestamp + duration − output_delay`) is still in the future, and never exceeds `buffer_capacity`. The player must therefore be able to hold `buffer_capacity` bytes of *encoded* data plus its decoded backlog. Size it from the codec byte rate: PCM 48 kHz/16-bit stereo is 192 kB/s, so 1 MiB ≈ 5.4 s.
- `stream/clear` (player): drop all buffered audio, keep the decoder/stream configuration, and continue with the chunks that follow.
- `stream/end`: stop output and clear buffers for the listed roles (no `roles` = all).
- **MA 2.10.6 behaviour**: for queue `next`, `seek` and controller `pause`, MA sent `stream/end` followed by a fresh `stream/start` (or a `group/update` to `stopped`) rather than `stream/clear` **(verified on 2.10.6)**. Natural track transitions keep one stream (gapless) and only update `server/state.metadata`. Implement `stream/clear` anyway, since the spec and future servers use it for seeks and track jumps.

### 2.8 Volume, mute, state reporting

- `server/command` `{"player":{"command":"volume","volume":70}}` arrives when the user changes volume in MA (`players/cmd/volume_set`) **(verified on 2.10.6)**. Similarly `mute` with `mute: bool`, and `set_static_delay` with `static_delay_ms`.
- Apply the change locally, then send a full `client/state` with the new `volume`/`muted` (required "whenever a field changes, including via server/command or device controls").
- Spec: volume 0–100 is perceived loudness. Gain = `(volume/100)^1.5`, ramped to avoid clicks. `volume` and `muted` are independent (do not implement mute as volume 0).
- Leaving: send `{"type":"client/goodbye","payload":{"reason":"shutdown"|"restart"|"user_request"|"another_server"}}` before closing. Without it the server assumes `restart` and keeps the player around.

### 2.9 Play / pause / next from the phone: controller role vs MA API

Two options, both observed working on 2.10.6:

1. **Sendspin `controller@v1`**: `{"type":"client/command","payload":{"controller":{"command":"pause"}}}`. Commands: `play`, `pause`, `stop`, `next`, `previous`, `volume` (+`volume`), `mute` (+`mute`), `repeat_off|repeat_one|repeat_all`, `shuffle|unshuffle`, `switch`, `seek` (+`position_ms`), `seek_relative` (+`offset_ms`). Only send commands listed in the latest `server/state.controller.supported_commands`. MA's list changes with state: `seek` and `seek_max_ms` appear only while a seekable item plays. Controller commands act on the Sendspin *group* this client belongs to.
2. **MA API** on `/ws`: `player_queues/play|pause|next|previous|seek` with `queue_id = client_id`. This is needed anyway for browsing, `play_media`, queue editing and controlling *other* players, and it reports richer state (queue, items, images).

Practical split: use the MA API for everything user-facing, including controlling the phone's own player, and use the Sendspin connection purely as the audio sink (`player@v1`, optionally `metadata@v1` for lock-screen metadata timed to the audio). The controller role is useful only if the app must work without an MA API session.

### 2.10 The encrypted path (later work, 2.10+)

Summary of what an rc1-conformant client adds: Curve25519 identity (`client_id` = base64url public key, 43 chars), `client/init {client_id, version:1, suite:"25519_ChaChaPoly_SHA256"|"25519_AESGCM_SHA256"}`, Noise `KKpsk2` with the server as initiator, `psk_id = base64url(SHA-256("sendspin-psk-id-v1" || PSK))`, Sentinel PSK `SHA-256("sendspin-sentinel-psk-v1")`, `server/activate {activities, active_roles}`, `supported_pair_methods` + `unpaired_access` in `client/hello`, fragmentation (type 1), the 13-byte audio header, and artwork configured in `client/state`. MA auto-approves an encrypted client that advertises `unpaired_access.enabled` ("guest access", `_auto_trust_guest_access` in the provider), and MA's web player pairs itself via the `sendspin/pair_web_player {pairing_token}` command. The wire names in aiosendspin 9.1.1 differ from rc1 (`dynamic_pin` vs `dynamic_pairing_code`, `pin_length`, …), so an encrypted client would have to target a specific aiosendspin generation. That is the main reason to stay on the legacy wire for now.

---

## Part 3 — Disposable test server

### 3.1 Image and start

Official image: **`ghcr.io/music-assistant/server:<version>`** (published by [`.github/workflows/release.yml`](https://github.com/music-assistant/server/blob/2.10.6/.github/workflows/release.yml); [`Dockerfile`](https://github.com/music-assistant/server/blob/2.10.6/Dockerfile): `VOLUME /data`, `EXPOSE 8095`, entrypoint `--data-dir /data --cache-dir /data/.cache`). Pin an exact version in tests.

```sh
docker run -d --rm --name ma-test \
  -p 8095:8095 -p 8097:8097 -p 8927:8927 \
  -v "$PWD/test-music:/music:ro" \
  ghcr.io/music-assistant/server:2.10.6
# readiness: poll until GET /info answers
until curl -fsS http://localhost:8095/info >/dev/null; do sleep 1; done
```

No `/data` volume means a fresh, empty instance every run. Production installs use `--network host` (mDNS discovery). Tests only need the mapped ports: 8095 API/proxy, 8097 stream server (legacy imageproxy, audio streams for other player types), 8927 direct Sendspin. Startup to a responding `/info` took a few seconds on 2.10.6.

### 3.2 Non-interactive onboarding

There is no environment variable for an initial user. Use the setup endpoint ([`_handle_setup`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/controllers/webserver/controller.py)) **(verified on 2.10.6)**:

```sh
curl -sS -X POST http://localhost:8095/setup -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"test-password-123","device_name":"integration-tests"}'
# -> {"success": true, "token": "<JWT session token>", "user": {... "role": "admin" ...}}
```

- Only works while no user exists (400 `Setup already completed` otherwise). Username ≥ 2 chars, password ≥ 8 chars. The user gets the `admin` role.
- `onboard_done` stays `false` until `config/onboard_complete` is called or the first provider is added. Adding the filesystem provider (below) completes onboarding automatically. Until a user exists, `/ws` closes with error 503 and `/api` returns 503.

### 3.3 Add a local filesystem music provider via the API

In 2.10 providers are created through **setup flows**. `config/providers/save` for a new provider fails with `Adding a provider is only possible through the setup flow` **(verified on 2.10.6)**. Flow, using `/api` with `Authorization: Bearer <token>` (or the same commands over `/ws`):

```jsonc
// 1. start the flow
{"message_id":"1","command":"config/providers/setup","args":{"provider_domain":"filesystem_local"}}
// -> SetupFlowStep {"flow_id":"82b4…","step_id":"user","type":"form",
//                   "entries":[{"key":"content_type","default_value":"music"},{"key":"path","default_value":"/media"}], ...}

// 2. submit the form
{"message_id":"2","command":"config/flows/submit",
 "args":{"flow_id":"82b4…","values":{"content_type":"music","path":"/music"}}}
// -> {"step_id":"finish_library_sync","type":"finish","result":{"instance_id":"filesystem_local--KgndjQQ2"}, ...}
```

- Steps can be `form`, `external`, `progress`, `finish` or `abort`; on a `form` with `errors`, resubmit. `config/flows/get {flow_id}` re-renders and `config/flows/abort {flow_id}` cancels ([`setup_flow.py`](https://github.com/music-assistant/models/blob/1.1.205/music_assistant_models/setup_flow.py)). The filesystem flow is a single form ([`providers/filesystem_local/setup_flow.py`](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/providers/filesystem_local/setup_flow.py)). `path` is a path *inside the container*.
- The initial library sync starts by itself in the background. Tests should poll `music/albums/library_items` (or wait for the `music_sync_completed` event) before asserting. With 3 tagged FLAC files, the album appeared within about 10 s on 2.10.6. `music/sync {providers?: [instance_id]}` forces a rescan.
- On MA 2.7–2.9 (before setup flows) the equivalent was `config/providers/save {provider_domain:"filesystem_local", values:{path:"/music"}}`. This was not re-verified here.
- Test media: tagged files in `<root>/<Artist>/<Album>/NN - Title.flac` work. The image ships `ffmpeg`, so fixtures can be generated inside the container: `docker exec ma-test ffmpeg -f lavfi -i "sine=frequency=440:duration=20" -ac 2 -ar 44100 -metadata title=… -metadata artist=… -metadata album=… -metadata track=1 "/music/A/B/01 - T.flac"`.

### 3.4 End-to-end smoke test that passed against 2.10.6

1. `POST /setup` → token. 2. Setup flow for `filesystem_local` at `/music`. 3. Poll `music/albums/library_items` until the album appears. 4. Open `ws://localhost:8095/sendspin`, send `auth` with `client_id`, get `auth_ok`, send `client/hello` (with `device_info.product_name: "Mobile Application"`), get `server/hello`, send `client/time` + `client/state`. 5. On `/ws`, `players/all` lists the Sendspin player (`provider: "sendspin"`, `type: "player"`). 6. `player_queues/play_media {queue_id: client_id, media: "library://album/1", option: "replace"}` → `group/update playing`, `server/state.metadata` (title/artist/album/progress), `stream/start {codec: pcm, 44100, 2, 16}`, then type-4 binary chunks. Events `queue_updated`, `queue_items_updated`, `player_updated`, `queue_time_updated` arrive on `/ws`. 7. `player_queues/next` → metadata for track 2, `stream/end` + `stream/start`. 8. `player_queues/pause` → `stream/end`, `group/update stopped`. 9. `players/cmd/volume_set 70` → `server/command {player:{command:"volume",volume:70}}`.

---

## Part 4 — Ensemble (Flutter MA client): practical notes and divergences

Read at Ensemble commit `28a53ac` (2026-07-07): `lib/services/sendspin_service.dart`, `lib/services/music_assistant_api.dart`.

Useful practices worth copying:
- It tries the `/sendspin` proxy with token auth for HTTPS servers and falls back to the direct `:8927` URL on the LAN.
- It waits for `auth_ok` before sending `client/hello`, and for `server/hello` before treating the player as registered.
- It persists a device player id across launches and de-duplicates concurrent connect attempts.
- It detects server info by the `server_version` key and authenticates with `auth/login` followed by `auth`.

Where it diverges from the server/spec (do not copy):

| Ensemble | Authoritative behaviour |
|---|---|
| Reads the audio timestamp as **little-endian** (`getInt64(0, Endian.little)`) | Big-endian int64 µs (spec; aiosendspin `>Bq`). |
| No clock sync: `client/time` every 30 s with **wall-clock** `microsecondsSinceEpoch`, `server/time` only logged; PCM is played as it arrives | Monotonic clock, time-filter Kalman filter, burst sampling; schedule every chunk against the server timestamp; drop late chunks. |
| `client/hello` has no `device_info` | MA 2.10 then registers a hidden `protocol` player, not a standalone app player. Send `product_name: "Mobile Application"`. |
| `client/state` sends `player.state: "synchronized"` (deprecated nested field) and no timing fields | `available` at top level (2.10+) or top-level `state` (≤ 2.9); include `static_delay_ms`/`required_lead_time_ms`/`min_buffer_ms`. |
| Proxy auth frame omits `client_id` | Include it so the player is tied to the user session. |
| Handles no `stream/clear`, `server/state` or `group/update` semantics; handles non-spec `play`/`pause`/`seek`/`ping` messages | Implement `stream/clear`; ignore unknown types. |
| `mute` is applied as volume 0 | Mute and volume are independent. |
| Image URLs use `/imageproxy?provider=…&path=…` on the API port | That form only exists on 2.7.x and only on the stream port 8097. 2.10 needs `/imageproxy/<proxy_id>`; the query form returns 400. |
| `auth/create_token` expecting `{token}` | The command is `auth/token/create {name}` and returns the token string. |
| Resolves a request on the first frame with its `message_id` | Streaming commands send several `partial: true` frames; accumulate until `partial: false`. |
| Infers "auth required" from `schema_version >= 28` | Correct for 2.7+. The server never sends `needs_auth`/`auth_enabled`. |

---

## Uncertainties and gaps

- The legacy unencrypted Sendspin path is a "transition mode" (`DEPRECATED`/`spec-pr-*` markers throughout aiosendspin). It is still default-on in MA `dev`, but there is no published removal date. A long-lived app should plan the Noise/pairing upgrade.
- aiosendspin 9.1.1 (MA 2.10.6) predates spec 1.0.0-rc1. Field names differ (`static_delay_ms` vs `output_delay_ms`, `dynamic_pin` vs `dynamic_pairing_code`, support objects, artwork configuration location). The tables above describe what 2.10.6 actually accepts and emits.
- Legacy client details for MA 2.7.0–2.7.10 (`player_support` key, `state` instead of `available`, no timing fields) come from reading aiosendspin 1.1.4/2.0.0 source, not from running those servers.
- Adding a provider on MA 2.7–2.9 via `config/providers/save` is inferred from the 2.10 error message and history, not run.
- MA 2.10.6 used `stream/end` + `stream/start` for seek and skip rather than `stream/clear`; this may change with aiosendspin 10 / MA 2.11.
- The Sendspin provider's own [README](https://github.com/music-assistant/server/blob/2.10.6/music_assistant/providers/sendspin/README.md) still documents `sendspin/ice_servers`/`sendspin/connect` WebRTC commands, which 2.10.6 does not register (WebRTC moved to the remote-access gateway). Treat that README as stale.
- Opus was not exercised against the server.

## Sources

- MA server 2.10.6: https://github.com/music-assistant/server/tree/2.10.6 — `music_assistant/controllers/webserver/{controller.py,websocket_client.py,auth.py,sendspin_proxy.py,helpers/auth_middleware.py}`, `controllers/metadata/images.py`, `controllers/music/media/*.py`, `controllers/player_queues/controller.py`, `controllers/players/*.py`, `controllers/config/*.py`, `providers/sendspin/*`, `providers/filesystem_local/*`, `Dockerfile`, `.github/workflows/release.yml`
- MA server 2.7.11 metadata controller (legacy imageproxy): https://github.com/music-assistant/server/blob/2.7.11/music_assistant/controllers/metadata.py
- MA server dev: https://github.com/music-assistant/server/tree/4f1a86cd558cded72995fac9edc926a2e7c955df
- MA models 1.1.205: https://github.com/music-assistant/models/tree/1.1.205/music_assistant_models
- MA frontend (reference client): https://github.com/music-assistant/frontend — `src/plugins/api/index.ts`, `src/plugins/sendspin-connection.ts`
- Sendspin spec 1.0.0-rc1: https://github.com/Sendspin/spec/blob/v1.0.0-rc1/README.md (and `main`)
- Sendspin implementation guide (non-normative): https://www.sendspin-audio.com/build/guide/
- aiosendspin 9.1.1 / 10.0.1 / 1.1.4: https://github.com/Sendspin/aiosendspin
- Sendspin time-filter: https://github.com/Sendspin/time-filter (README, `docs/theory.md`)
- Live verification: `ghcr.io/music-assistant/server:2.10.6`, `/api-docs/commands.json` and the WebSocket/HTTP exchanges quoted above.
