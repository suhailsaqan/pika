#!/usr/bin/env bash
set -euo pipefail

STATE_DIR="${STATE_DIR:-/app/state}"
mkdir -p "$STATE_DIR"

if [ -n "${NOSTR_SECRET_KEY:-}" ]; then
  cat > "$STATE_DIR/identity.json" <<IDENTITY
{"secret_key_hex":"$NOSTR_SECRET_KEY","public_key_hex":""}
IDENTITY
fi

exec /app/pikachat daemon \
  --relay wss://us-east.nostr.pikachat.org \
  --relay wss://eu.nostr.pikachat.org \
  --state-dir "$STATE_DIR" \
  --auto-accept-welcomes \
  --exec "python3 /app/pi-bridge.py"
