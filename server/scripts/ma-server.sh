#!/usr/bin/env bash
# Runs a throwaway Music Assistant server in a container, with the test library, for recording the
# golden responses in crates/api/tests/golden: the API this server follows, as Music Assistant
# answers it.
#
#   scripts/ma-server.sh start   # start (or reuse) the server, create the user, add a library
#   scripts/ma-server.sh stop    # remove the container and its data
#   scripts/ma-server.sh env     # print the server's address and credentials as TEST_SERVER_*
#
# The library is generated inside the container with ffmpeg: two albums of short tones with
# tags and cover art, enough to browse and to play.
#
# Requires docker or podman. Override the defaults through the environment:
#   MA_IMAGE     container image (default ghcr.io/music-assistant/server:2.10.6)
#   MA_PORT      host port for the API (default 18095)
#   MA_USERNAME  user created on first start (default bopsie)
#   MA_PASSWORD  its password (default bopsie-test)
set -euo pipefail

MA_IMAGE="${MA_IMAGE:-ghcr.io/music-assistant/server:2.10.6}"
MA_PORT="${MA_PORT:-18095}"
MA_USERNAME="${MA_USERNAME:-bopsie}"
MA_PASSWORD="${MA_PASSWORD:-bopsie-test}"
CONTAINER="bopsie-ma"
URL="http://127.0.0.1:${MA_PORT}"

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  RUNTIME=docker
elif command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
  RUNTIME=podman
else
  echo "Neither docker nor podman is available and running." >&2
  exit 1
fi

# Calls a command on the API with the admin token: api <token> <command> <args-json>
api() {
  curl -fsS -X POST "$URL/api" \
    -H "Authorization: Bearer $1" -H 'Content-Type: application/json' \
    -d "{\"command\":\"$2\",\"args\":$3}"
}

json_field() {
  python3 -c "import json,sys; print(json.load(sys.stdin)$1)"
}

generate_library() {
  # artist|album|year|track titles, separated by commas
  local albums=(
    "Test Artist|First Album|2021|Opening,Middle,Closing"
    "Other Artist|Second Album|2023|Alpha,Beta,Gamma"
  )
  local frequency=330
  for entry in "${albums[@]}"; do
    IFS='|' read -r artist album year titles <<<"$entry"
    local dir="/music/$artist/$album"
    "$RUNTIME" exec "$CONTAINER" mkdir -p "$dir"
    "$RUNTIME" exec "$CONTAINER" ffmpeg -loglevel error -y -f lavfi \
      -i "color=c=0x$(printf '%06x' $((frequency * 4099 % 16777215))):s=300x300" \
      -frames:v 1 "$dir/cover.jpg"
    local number=1
    IFS=',' read -ra names <<<"$titles"
    for title in "${names[@]}"; do
      "$RUNTIME" exec "$CONTAINER" ffmpeg -loglevel error -y -f lavfi \
        -i "sine=frequency=${frequency}:duration=30" -ac 2 -ar 44100 \
        -metadata title="$title" -metadata artist="$artist" -metadata album_artist="$artist" \
        -metadata album="$album" -metadata date="$year" -metadata track="$number" \
        "$dir/$(printf '%02d' "$number") - $title.flac"
      number=$((number + 1))
      frequency=$((frequency + 55))
    done
  done
}

start() {
  if ! "$RUNTIME" container inspect "$CONTAINER" >/dev/null 2>&1; then
    "$RUNTIME" run -d --name "$CONTAINER" -p "${MA_PORT}:8095" "$MA_IMAGE" >/dev/null
  elif [ "$("$RUNTIME" container inspect -f '{{.State.Running}}' "$CONTAINER")" != "true" ]; then
    "$RUNTIME" start "$CONTAINER" >/dev/null
  fi

  echo "Waiting for Music Assistant at $URL ..."
  for _ in $(seq 1 120); do
    curl -fsS "$URL/info" >/dev/null 2>&1 && break
    sleep 1
  done
  curl -fsS "$URL/info" >/dev/null

  if [ "$(curl -fsS "$URL/info" | json_field "['onboard_done']")" = "True" ]; then
    echo "Already set up."
    return
  fi

  local token
  token="$(curl -fsS -X POST "$URL/setup" -H 'Content-Type: application/json' \
    -d "{\"username\":\"$MA_USERNAME\",\"password\":\"$MA_PASSWORD\",\"device_name\":\"bopsie-tests\"}" \
    | json_field "['token']")"

  generate_library

  local flow
  flow="$(api "$token" config/providers/setup '{"provider_domain":"filesystem_local"}' | json_field "['flow_id']")"
  api "$token" config/flows/submit \
    "{\"flow_id\":\"$flow\",\"values\":{\"content_type\":\"music\",\"path\":\"/music\"}}" >/dev/null

  echo "Waiting for the library to sync ..."
  for _ in $(seq 1 120); do
    local count
    count="$(api "$token" music/tracks/count '{}' 2>/dev/null || echo 0)"
    [ "$count" = "6" ] && break
    sleep 1
  done
  echo "Ready: $URL ($MA_USERNAME / $MA_PASSWORD)"
}

case "${1:-}" in
  start) start ;;
  stop) "$RUNTIME" rm -f "$CONTAINER" >/dev/null 2>&1 || true ;;
  env)
    echo "export TEST_SERVER_URL=$URL"
    echo "export TEST_SERVER_USERNAME=$MA_USERNAME"
    echo "export TEST_SERVER_PASSWORD=$MA_PASSWORD"
    ;;
  *)
    echo "usage: $0 start|stop|env" >&2
    exit 2
    ;;
esac
