#!/usr/bin/env bash
# Shared helpers for scripts that use pikahub.
# Source this file: . "$(dirname "$0")/lib/pikahub.sh"

# pikahub_up PROFILE STATE_DIR [EXTRA_ARGS...]
#   Starts pikahub with the given profile and state dir, outputs manifest JSON.
#   Sets: PIKAHUB_MANIFEST, RELAY_URL, RELAY_PORT
pikahub_up() {
  local profile="$1" state_dir="$2"
  shift 2
  PIKAHUB_MANIFEST="$(cargo run -q -p pikahub -- up \
    --profile "$profile" \
    --background \
    --state-dir "$state_dir" \
    --relay-port 0 \
    "$@")"
  RELAY_URL="$(echo "$PIKAHUB_MANIFEST" | python3 -c "import json,sys; print(json.load(sys.stdin)['relay_url'])")"
  RELAY_PORT="$(pikahub_url_port "$RELAY_URL")"
}

# pikahub_down STATE_DIR
#   Stops pikahub and cleans up.
pikahub_down() {
  local state_dir="$1"
  cargo run -q -p pikahub -- down --state-dir "$state_dir" 2>/dev/null || true
  rm -rf "$state_dir" 2>/dev/null || true
}

# pikahub_url_port URL
#   Extracts port from a URL like ws://127.0.0.1:12345 or http://host:8080/path.
#   Uses python for reliable parsing instead of fragile sed.
pikahub_url_port() {
  python3 -c "from urllib.parse import urlparse; print(urlparse('$1').port)"
}

# pikahub_manifest_field FIELD
#   Reads a field from the last PIKAHUB_MANIFEST.
pikahub_manifest_field() {
  echo "$PIKAHUB_MANIFEST" | python3 -c "import json,sys; print(json.load(sys.stdin).get('$1',''))"
}
