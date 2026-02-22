#!/usr/bin/env bash
# hc -- Hetzner Cloud helper for pika infrastructure
#
# Wraps hcloud CLI for common server lifecycle operations.
# Server provisioning only -- NixOS install is done separately via nixos-anywhere.

set -euo pipefail

SERVER_NAME="${HC_SERVER_NAME:-pika-server}"
SERVER_TYPE="${HC_SERVER_TYPE:-cpx21}"
LOCATION="${HC_LOCATION:-ash}"
SSH_KEY_NAME="${HC_SSH_KEY_NAME:-default}"
STATE_FILE="${HOME}/.hc-pika-current"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${GREEN}==>${NC} $*"; }
warn() { echo -e "${YELLOW}Warning:${NC} $*"; }
error(){ echo -e "${RED}Error:${NC} $*" >&2; }

get_ip() {
  hcloud server ip "$1" 2>/dev/null || true
}

save_current() { echo "$1" > "$STATE_FILE"; }
clear_current() { rm -f "$STATE_FILE"; }
get_current() { cat "$STATE_FILE" 2>/dev/null || true; }

wait_for_ssh() {
  local ip="$1" max="${2:-60}" attempt=0
  log "Waiting for SSH at $ip..."
  while ! ssh -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new -o BatchMode=yes "root@$ip" true 2>/dev/null; do
    attempt=$((attempt + 1))
    if [[ $attempt -ge $max ]]; then
      error "Timeout waiting for SSH after $max attempts"
      return 1
    fi
    printf "."
    sleep 2
  done
  echo ""
  log "SSH available!"
}

cmd_new() {
  local current
  current=$(get_current)
  if [[ -n "$current" ]]; then
    local existing_ip
    existing_ip=$(get_ip "$current")
    if [[ -n "$existing_ip" ]]; then
      warn "Server $current already exists ($existing_ip)"
      echo "Use 'hc attach' to connect, or 'hc destroy' first."
      return 0
    fi
    clear_current
  fi

  log "Creating $SERVER_NAME (type: $SERVER_TYPE, location: $LOCATION)..."
  hcloud server create \
    --name "$SERVER_NAME" \
    --type "$SERVER_TYPE" \
    --location "$LOCATION" \
    --image debian-12 \
    --ssh-key "$SSH_KEY_NAME"

  local ip
  ip=$(get_ip "$SERVER_NAME")
  save_current "$SERVER_NAME"

  log "Server created!"
  echo ""
  echo "  Name: $SERVER_NAME"
  echo "  IP:   $ip"
  echo ""
  echo "Next steps:"
  echo "  1. Run 'just initial-deploy' to install NixOS via nixos-anywhere"
  echo "  2. Run 'just deploy' for subsequent config updates"
}

cmd_attach() {
  local current
  current=$(get_current)
  if [[ -z "$current" ]]; then
    error "No active server. Run 'hc new' first."
    return 1
  fi
  local ip
  ip=$(get_ip "$current")
  if [[ -z "$ip" ]]; then
    error "Server $current not found."
    clear_current
    return 1
  fi
  log "Connecting to $current ($ip)..."
  ssh "root@$ip"
}

cmd_destroy() {
  local current
  current=$(get_current)
  if [[ -z "$current" ]]; then
    error "No active server."
    return 1
  fi
  log "Destroying $current..."
  hcloud server delete "$current" 2>/dev/null || true
  clear_current
  log "Server destroyed."
}

cmd_list() {
  echo "=== SERVERS ==="
  hcloud server list -o columns=name,status,ipv4,server_type,created
}

cmd_status() {
  local current
  current=$(get_current)
  if [[ -z "$current" ]]; then
    echo "No active server tracked."
    echo "Checking Hetzner..."
    hcloud server list -o columns=name,status,ipv4,server_type
    return 0
  fi
  local ip
  ip=$(get_ip "$current")
  if [[ -z "$ip" ]]; then
    echo "Server $current no longer exists."
    clear_current
    return 0
  fi
  echo "Active server: $current"
  echo "IP: $ip"
  hcloud server describe "$current"
}

cmd_ip() {
  local current
  current=$(get_current)
  if [[ -z "$current" ]]; then
    error "No active server."
    return 1
  fi
  get_ip "$current"
}

cmd_help() {
  cat <<EOF
hc -- Hetzner Cloud helper for pika infrastructure

Usage: hc <command>

Commands:
  new           Create server (Debian base for nixos-anywhere)
  attach        SSH into server as root
  destroy       Delete server
  list          List all servers
  status        Show current server details
  ip            Print server IP
  help          Show this help

Environment:
  HC_SERVER_NAME    Server name (default: pika-server)
  HC_SERVER_TYPE    Server type (default: cpx21)
  HC_LOCATION       Datacenter (default: ash)
  HC_SSH_KEY_NAME   Hetzner SSH key name (default: default)
EOF
}

case "${1:-help}" in
  new)     cmd_new ;;
  attach)  cmd_attach ;;
  destroy) cmd_destroy ;;
  list)    cmd_list ;;
  status)  cmd_status ;;
  ip)      cmd_ip ;;
  help|--help|-h) cmd_help ;;
  *)       error "Unknown command: $1"; cmd_help; exit 1 ;;
esac
