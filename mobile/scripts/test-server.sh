#!/usr/bin/env bash
# Runs a throwaway Bopsie server in a container for integration and end-to-end tests.
#
#   scripts/test-server.sh start   # build the image, start the server, create the user, add a library
#   scripts/test-server.sh stop    # remove the container and its data
#   scripts/test-server.sh env     # print the TEST_SERVER_* variables the tests read
#
# The image is built from this repository's server/Dockerfile, so the tests run against the
# server as it is in the checkout. The library is generated inside the container with its ffmpeg:
# two albums of short tones with tags and cover art, enough to browse and to play.
#
# Requires docker or podman. Override the defaults through the environment:
#   BOPSIE_IMAGE          an image to use as it is, built elsewhere, rather than building one
#   TEST_SERVER_PORT      host port for the API (default 18095)
#   TEST_SERVER_USERNAME  user created on first start (default bopsie)
#   TEST_SERVER_PASSWORD  its password (default bopsie-test)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IMAGE="${BOPSIE_IMAGE:-bopsie-server:test}"
PORT="${TEST_SERVER_PORT:-18095}"
USERNAME="${TEST_SERVER_USERNAME:-bopsie}"
PASSWORD="${TEST_SERVER_PASSWORD:-bopsie-test}"
CONTAINER="bopsie-test-server"
URL="http://127.0.0.1:${PORT}"
TRACKS=6

runtime() {
  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    echo docker
  elif command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
    echo podman
  else
    echo "Neither docker nor podman is available and running." >&2
    exit 1
  fi
}

# Calls a command on the API with a token: api <token> <command> <args-json>
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
    # The library folder belongs to root; the server only reads it.
    "$RUNTIME" exec -u 0 "$CONTAINER" mkdir -p "$dir"
    "$RUNTIME" exec -u 0 "$CONTAINER" ffmpeg -loglevel error -y -f lavfi \
      -i "color=c=0x$(printf '%06x' $((frequency * 4099 % 16777215))):s=300x300" \
      -frames:v 1 "$dir/cover.jpg"
    local number=1
    IFS=',' read -ra names <<<"$titles"
    for title in "${names[@]}"; do
      "$RUNTIME" exec -u 0 "$CONTAINER" ffmpeg -loglevel error -y -f lavfi \
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
  if [ -z "${BOPSIE_IMAGE:-}" ]; then
    echo "Building the server image ..."
    "$RUNTIME" build -q -t "$IMAGE" -f "$ROOT/server/Dockerfile" "$ROOT" >/dev/null
  fi
  if ! "$RUNTIME" container inspect "$CONTAINER" >/dev/null 2>&1; then
    "$RUNTIME" run -d --name "$CONTAINER" -p "${PORT}:8095" "$IMAGE" >/dev/null
  elif [ "$("$RUNTIME" container inspect -f '{{.State.Running}}' "$CONTAINER")" != "true" ]; then
    "$RUNTIME" start "$CONTAINER" >/dev/null
  fi

  echo "Waiting for the server at $URL ..."
  for _ in $(seq 1 60); do
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
    -d "{\"username\":\"$USERNAME\",\"password\":\"$PASSWORD\",\"device_name\":\"bopsie-tests\"}" \
    | json_field "['token']")"

  generate_library
  api "$token" music/sync '{}' >/dev/null

  echo "Waiting for the library to sync ..."
  for _ in $(seq 1 60); do
    [ "$(api "$token" music/tracks/count '{}' 2>/dev/null || echo 0)" = "$TRACKS" ] && break
    sleep 1
  done
  [ "$(api "$token" music/tracks/count '{}')" = "$TRACKS" ]
  echo "Ready: $URL ($USERNAME / $PASSWORD)"
}

case "${1:-}" in
  start)
    RUNTIME="$(runtime)"
    start
    ;;
  stop)
    RUNTIME="$(runtime)"
    "$RUNTIME" rm -f "$CONTAINER" >/dev/null 2>&1 || true
    ;;
  env)
    echo "export TEST_SERVER_URL=$URL"
    echo "export TEST_SERVER_USERNAME=$USERNAME"
    echo "export TEST_SERVER_PASSWORD=$PASSWORD"
    ;;
  *)
    echo "usage: $0 start|stop|env" >&2
    exit 2
    ;;
esac
