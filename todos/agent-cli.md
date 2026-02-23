# Plan: `pika-cli agent new` — vibecoding over Marmot messages

## Context

Pika is an encrypted messaging app built on Nostr + MLS (via the Marmot protocol). The `pika-cli` binary (`cli/src/main.rs`) is a CLI for sending/receiving encrypted messages. On a separate worktree (`/Users/justin/code/pika/worktrees/fly-sprites`), we built a full bot orchestration system for running AI agents on Fly.io machines — warm pools, lease queues, Stripe billing, DB tables, etc. That's too much to merge. We want to extract **only** the bare minimum: create a Fly machine running the `pi` coding agent, invite it to an MLS group, and drop into a chat TUI — all from `pika-cli`.

The goal is to get a working `pika-cli agent new` command on master that lets you vibecode by chatting with a pi coding agent over Marmot-encrypted messages.

## How the bot works

Each bot machine runs `marmotd daemon --exec "python3 pi-bridge.py"`:
- **marmotd** is the Marmot MLS sidecar. It generates/loads a Nostr identity, connects to relays, handles MLS key packages/welcomes/group messages, and speaks a JSONL protocol over stdin/stdout with its `--exec` child.
- **pi-bridge.py** reads `message_received` events from marmotd, sends the text to the `pi` coding agent (in RPC mode), collects the response, and writes `send_message` back.
- Communication path: `pika-cli → Nostr relay → marmotd (MLS decrypt) → pi-bridge.py → pi → pi-bridge.py → marmotd (MLS encrypt) → Nostr relay → pika-cli`

## End-to-end flow for `pika-cli agent new`

```
$ export FLY_API_TOKEN=... ANTHROPIC_API_KEY=...
$ pika-cli --relay wss://us-east.nostr.pikachat.org agent new

Creating Fly volume... done (vol_abc123)
Creating Fly machine... done (d8e9f0a1)
Waiting for bot to publish key package....... done
Creating MLS group and inviting bot... done

Connected to pi agent (npub1abc...)
Type messages below. Ctrl-C to exit.

you> list the files in the current directory
pi> Here's what I see in /app:
    marmotd
    pi-bridge.py
    state/

you> ^C
Machine d8e9f0a1 is still running.
Stop with: fly machine stop d8e9f0a1 -a pika-bot
```

### Detailed steps inside `cmd_agent_new`:

1. **Read config from env** — `FLY_API_TOKEN` (required), `FLY_BOT_APP_NAME` (default `pika-bot`), `FLY_BOT_REGION` (default `iad`), `FLY_BOT_IMAGE` (default `registry.fly.io/pika-bot:latest`), `ANTHROPIC_API_KEY` (required for pi)
2. **Load or create the CLI user's own identity** — reuse existing `mdk_util::load_or_create_keys()` and `mdk_util::open_mdk()`
3. **Generate bot keypair** — `Keys::generate()`, extract secret hex and pubkey hex. We pass the secret into the machine so we know the bot's pubkey deterministically (no need to discover it).
4. **Create Fly volume** — POST to Fly Machines API, 1GB, named `agent_<8_random_chars>`
5. **Create Fly machine** — POST to Fly Machines API with env vars:
   - `STATE_DIR=/app/state`
   - `NOSTR_SECRET_KEY=<generated_secret_hex>` (entrypoint seeds identity.json from this)
   - `ANTHROPIC_API_KEY=<from_env>`
6. **Poll for bot's key package** — use existing `relay_util::fetch_latest_key_package()` with the known bot pubkey. Retry with backoff up to ~120s. The bot publishes kind 443 on startup.
7. **Create MLS group + send welcome** — reuse patterns from existing `cmd_invite()`: call `mdk.create_group()`, wrap welcome in giftwrap via `EventBuilder::gift_wrap()`, publish to relays.
8. **Enter chat loop** — `tokio::select!` over:
   - `tokio::io::BufReader::new(tokio::io::stdin()).lines()` → encrypt via `mdk.create_message()` → publish to relay
   - nostr-sdk notification stream (subscribe to kind 445 for the group) → decrypt via `mdk.process_message()` → print to stdout with `pi> ` prefix
   - `tokio::signal::ctrl_c()` → print cleanup message with machine ID, exit

## Files to create

### 1. `cli/src/fly_machines.rs` — Minimal Fly Machines API client

Extract from `crates/pika-server/src/bots/fly_machines.rs` on fly-sprites branch. Simplified — no DB, no warm pool concepts.

```rust
use serde::{Deserialize, Serialize};

pub struct FlyClient {
    client: reqwest::Client,
    api_token: String,
    app_name: String,
    region: String,
    image: String,
}

// Request/response structs for the Fly Machines API
#[derive(Serialize)] struct CreateVolumeRequest { name: String, region: String, size_gb: u32 }
#[derive(Deserialize)] pub struct Volume { pub id: String }
#[derive(Serialize)] struct CreateMachineRequest { name: String, region: String, config: MachineConfig }
#[derive(Serialize)] struct MachineConfig { image: String, env: HashMap<String, String>, guest: GuestConfig, mounts: Vec<MachineMount> }
#[derive(Serialize)] struct GuestConfig { cpu_kind: String, cpus: u32, memory_mb: u32 }
#[derive(Serialize)] struct MachineMount { volume: String, path: String }
#[derive(Deserialize)] pub struct Machine { pub id: String, pub state: String }

impl FlyClient {
    pub fn from_env() -> anyhow::Result<Self> {
        // Read FLY_API_TOKEN (required), FLY_BOT_APP_NAME (default "pika-bot"),
        // FLY_BOT_REGION (default "iad"), FLY_BOT_IMAGE (default "registry.fly.io/pika-bot:latest")
    }
    fn base_url(&self) -> String { format!("https://api.machines.dev/v1/apps/{}", self.app_name) }
    pub async fn create_volume(&self, name: &str) -> anyhow::Result<Volume> { ... }
    pub async fn create_machine(&self, name: &str, volume_id: &str, env: HashMap<String,String>) -> anyhow::Result<Machine> { ... }
    pub async fn get_machine(&self, machine_id: &str) -> anyhow::Result<Machine> { ... }
}
```

The Fly Machines REST API:
- Base: `https://api.machines.dev/v1/apps/{app_name}`
- Auth: `Authorization: Bearer {api_token}`
- Create volume: `POST /volumes` with `{name, region, size_gb}`
- Create machine: `POST /machines` with `{name, region, config: {image, env, guest: {cpu_kind: "shared", cpus: 1, memory_mb: 256}, mounts: [{volume, path: "/app/state"}]}}`
- Get machine: `GET /machines/{id}` returns `{id, state}`

### 2. `crates/pika-bot/entrypoint.sh` — Bot startup wrapper

```bash
#!/usr/bin/env bash
set -euo pipefail
STATE_DIR="${STATE_DIR:-/app/state}"
mkdir -p "$STATE_DIR"

# If CLI passed a secret key, seed the identity so marmotd uses it
if [ -n "${NOSTR_SECRET_KEY:-}" ]; then
  cat > "$STATE_DIR/identity.json" <<IDENTITY
{"secret_key_hex":"$NOSTR_SECRET_KEY","public_key_hex":""}
IDENTITY
fi

exec /app/marmotd daemon \
  --relay wss://us-east.nostr.pikachat.org \
  --relay wss://eu.nostr.pikachat.org \
  --state-dir "$STATE_DIR" \
  --auto-accept-welcomes \
  --exec "python3 /app/pi-bridge.py"
```

The `public_key_hex` field can be empty — `marmotd`'s `load_or_create_keys()` in `crates/marmotd/src/main.rs` derives it from the secret key. The identity file format is:
```json
{"secret_key_hex": "64-char-hex", "public_key_hex": "64-char-hex"}
```
(See `IdentityFile` struct at `crates/marmotd/src/main.rs:179`)

### 3. `crates/pika-bot/Dockerfile`

Based on fly-sprites version but using entrypoint.sh:

```dockerfile
FROM rust:1.90-bookworm AS builder
WORKDIR /usr/src/app
COPY . .
RUN echo "$(rustc -vV | sed -n 's|host: ||p')" > rust_target
ENV CARGO_NET_GIT_FETCH_WITH_CLI true
RUN --mount=type=cache,target=/usr/local/cargo,from=rust:latest,source=/usr/local/cargo \
    --mount=type=cache,target=target \
    cargo build --target $(cat rust_target) -p marmotd --release && \
    mv ./target/$(cat rust_target)/release/marmotd ./marmotd

FROM node:22-bookworm-slim
RUN apt-get update && apt-get install -y python3 ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -ms /bin/bash app
RUN npm install -g @mariozechner/pi-coding-agent
USER app
WORKDIR /app
COPY --from=builder /usr/src/app/marmotd /app/marmotd
COPY bots/pi-bridge.py /app/pi-bridge.py
COPY crates/pika-bot/entrypoint.sh /app/entrypoint.sh
ENV RUST_LOG=info
CMD ["/app/entrypoint.sh"]
```

Note: The entrypoint.sh needs to be executable (`chmod +x`). Either set in Dockerfile with `RUN chmod +x /app/entrypoint.sh` or ensure it's committed with execute bit.

### 4. `bots/pi-bridge.py`

Copy from fly-sprites as-is. This is the bridge between marmotd JSONL and pi RPC:

```python
#!/usr/bin/env python3
"""Bridge between marmotd JSONL (stdin/stdout) and pi coding agent RPC mode."""
import json, os, subprocess, sys, threading

my_pubkey = None
pi_proc = None

def log(msg):
    print(f"[pi-bridge] {msg}", file=sys.stderr, flush=True)

def start_pi():
    env = os.environ.copy()
    cmd = ["pi", "--mode", "rpc", "--no-session", "--provider", "anthropic"]
    model = os.environ.get("PI_MODEL")
    if model:
        cmd.extend(["--model", model])
    log(f"starting pi: {' '.join(cmd)}")
    return subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                           stderr=sys.stderr, env=env, bufsize=0)

def send_to_pi(pi_proc, msg):
    line = json.dumps(msg) + "\n"
    pi_proc.stdin.write(line.encode())
    pi_proc.stdin.flush()

def collect_pi_response(pi_proc):
    text_parts = []
    for raw in pi_proc.stdout:
        raw = raw.decode().strip()
        if not raw: continue
        try: event = json.loads(raw)
        except json.JSONDecodeError: continue
        etype = event.get("type")
        if etype == "message_update":
            delta_event = event.get("assistantMessageEvent", {})
            if delta_event.get("type") == "text_delta":
                text_parts.append(delta_event["delta"])
        elif etype == "agent_end": break
        elif etype == "response" and not event.get("success"): break
    return "".join(text_parts)

def send_to_marmotd(cmd):
    print(json.dumps(cmd), flush=True)

def main():
    global my_pubkey, pi_proc
    pi_proc = start_pi()
    for line in sys.stdin:
        line = line.strip()
        if not line: continue
        try: msg = json.loads(line)
        except json.JSONDecodeError: continue
        msg_type = msg.get("type")
        if msg_type == "ready":
            my_pubkey = msg.get("pubkey")
            log(f"marmotd ready, pubkey={my_pubkey}")
            send_to_marmotd({"cmd": "publish_keypackage"})
        elif msg_type == "message_received":
            if msg.get("from_pubkey") == my_pubkey: continue
            content = msg.get("content", "")
            group_id = msg.get("nostr_group_id", "")
            send_to_pi(pi_proc, {"type": "prompt", "message": content})
            response = collect_pi_response(pi_proc)
            if response.strip():
                send_to_marmotd({"cmd": "send_message", "nostr_group_id": group_id, "content": response})
    if pi_proc and pi_proc.poll() is None: pi_proc.terminate()

if __name__ == "__main__": main()
```

### 5. `fly.pika-bot.toml`

```toml
app = "pika-bot"
primary_region = "ewr"

[build]
  dockerfile = "crates/pika-bot/Dockerfile"

[env]
  RUST_LOG = "info"
```

## Files to modify

### 6. `cli/Cargo.toml`

Add `reqwest` dependency:
```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

### 7. `cli/src/main.rs`

Add `mod fly_machines;` at the top.

Add to the `Command` enum:
```rust
/// Manage AI agents on Fly.io
Agent {
    #[command(subcommand)]
    cmd: AgentCommand,
},
```

Add new enum:
```rust
#[derive(Debug, Subcommand)]
enum AgentCommand {
    /// Create a new pi agent on Fly.io and start chatting
    New {
        /// Machine name (default: agent-<random>)
        #[arg(long)]
        name: Option<String>,
    },
}
```

Add to match in main():
```rust
Command::Agent { cmd } => match cmd {
    AgentCommand::New { name } => cmd_agent_new(&cli, name.as_deref()).await,
},
```

Implement `cmd_agent_new()`:

```rust
async fn cmd_agent_new(cli: &Cli, name: Option<&str>) -> anyhow::Result<()> {
    // 1. Read config
    let fly = fly_machines::FlyClient::from_env()?;
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY")
        .context("ANTHROPIC_API_KEY must be set")?;

    // 2. Load/create user identity + mdk
    let (keys, mdk) = open(cli)?;
    eprintln!("Your pubkey: {}", keys.public_key().to_hex());

    // 3. Generate bot keypair
    let bot_keys = Keys::generate();
    let bot_pubkey = bot_keys.public_key();
    let bot_secret_hex = bot_keys.secret_key().to_secret_hex();
    eprintln!("Bot pubkey: {}", bot_pubkey.to_hex());

    // 4. Create Fly volume
    let suffix = &uuid_or_random_hex()[..8]; // use rand to generate 8 hex chars
    let vol_name = format!("agent_{suffix}");
    let machine_name = name.map(|s| s.to_string()).unwrap_or_else(|| format!("agent-{suffix}"));
    eprint!("Creating Fly volume...");
    let volume = fly.create_volume(&vol_name).await?;
    eprintln!(" done ({})", volume.id);

    // 5. Create Fly machine with env vars
    let mut env = std::collections::HashMap::new();
    env.insert("STATE_DIR".into(), "/app/state".into());
    env.insert("NOSTR_SECRET_KEY".into(), bot_secret_hex);
    env.insert("ANTHROPIC_API_KEY".into(), anthropic_key);
    eprint!("Creating Fly machine...");
    let machine = fly.create_machine(&machine_name, &volume.id, env).await?;
    eprintln!(" done ({})", machine.id);

    // 6. Connect to relays
    let client = client(cli, &keys).await?;
    let relays = relay_util::parse_relay_urls(&cli.relay)?;

    // 7. Poll for bot key package (kind 443)
    eprint!("Waiting for bot to publish key package");
    let bot_kp = loop {
        match relay_util::fetch_latest_key_package(&client, &bot_pubkey, &relays, Duration::from_secs(5)).await {
            Ok(kp) => break kp,
            Err(_) => {
                eprint!(".");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
        // TODO: add a timeout (~120s)
    };
    eprintln!(" done");

    // 8. Create MLS group and invite bot
    eprint!("Creating MLS group and inviting bot...");
    let config = NostrGroupConfigData::new(
        "Agent Chat".to_string(), String::new(), None, None, None,
        relays.clone(), vec![keys.public_key(), bot_pubkey],
    );
    let result = mdk.create_group(&keys.public_key(), vec![bot_kp], config)?;
    let mls_group_id = result.group.mls_group_id.clone();
    let nostr_group_id_hex = hex::encode(result.group.nostr_group_id);

    // Send welcome giftwraps
    for rumor in result.welcome_rumors {
        let giftwrap = EventBuilder::gift_wrap(&keys, &bot_pubkey, rumor, []).await?;
        relay_util::publish_and_confirm(&client, &relays, &giftwrap, "welcome").await?;
    }
    eprintln!(" done");

    // 9. Subscribe to group messages
    let group_filter = Filter::new()
        .kind(Kind::MlsGroupMessage)
        .custom_tag(SingleLetterTag::lowercase(Alphabet::H), &nostr_group_id_hex)
        .since(Timestamp::now());
    let sub = client.subscribe(group_filter, None).await?;
    let mut rx = client.notifications();

    eprintln!("\nConnected to pi agent ({})", bot_pubkey.to_bech32().unwrap_or_default());
    eprintln!("Type messages below. Ctrl-C to exit.\n");

    // 10. Chat loop
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    loop {
        // Print prompt
        eprint!("you> ");
        tokio::select! {
            line = stdin.next_line() => {
                let Some(line) = line? else { break };
                let line = line.trim().to_string();
                if line.is_empty() { continue; }

                // Encrypt and send
                let rumor = EventBuilder::new(Kind::ChatMessage, &line).build(keys.public_key());
                let msg_event = mdk.create_message(&mls_group_id, rumor)?;
                relay_util::publish_and_confirm(&client, &relays, &msg_event, "chat").await?;
            }
            notification = rx.recv() => {
                let Ok(notification) = notification else { continue };
                let RelayPoolNotification::Event { subscription_id, event, .. } = notification else { continue };
                if subscription_id != sub.val { continue; }
                let event = *event;
                if event.kind != Kind::MlsGroupMessage { continue; }
                if let Ok(MessageProcessingResult::ApplicationMessage(msg)) = mdk.process_message(&event) {
                    if msg.pubkey == bot_pubkey {
                        // Clear the "you> " prompt line, print bot response
                        eprint!("\r");
                        println!("pi> {}", msg.content);
                        println!();
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    client.shutdown().await;
    eprintln!("\nMachine {} is still running.", machine.id);
    eprintln!("Stop with: fly machine stop {} -a pika-bot", machine.id);
    Ok(())
}
```

## Existing code to reuse

These all exist on master in `cli/src/`:
- `mdk_util::load_or_create_keys()` — loads or generates Nostr keypair from identity.json
- `mdk_util::open_mdk()` — opens MLS database
- `relay_util::connect_client()` — connects nostr-sdk Client to relays
- `relay_util::parse_relay_urls()` — parses string URLs to RelayUrl
- `relay_util::fetch_latest_key_package()` — fetches kind 443 from relays for a given pubkey
- `relay_util::publish_and_confirm()` — publishes event and confirms relay acceptance
- `cmd_invite()` patterns — creating MLS group, sending welcome giftwraps
- `cmd_listen()` patterns — subscribing to group messages, decrypting with mdk.process_message()
- `cmd_send()` patterns — encrypting with mdk.create_message(), publishing

## What is NOT included

- No warm pool, lease queue, usage events, Stripe billing
- No DB tables or Diesel
- No pika-server rename (pika-notifications stays as-is)
- No HTTP endpoints
- No `agent stop` / `agent list` commands (use `fly` CLI directly for now)
- No LLM proxy

## Verification

1. Build and push the pika-bot image: `fly deploy -c fly.pika-bot.toml`
2. Set env: `export FLY_API_TOKEN=... ANTHROPIC_API_KEY=...`
3. Run: `pika-cli --relay wss://us-east.nostr.pikachat.org agent new`
4. Wait for machine creation + bot key package (~30-60s)
5. Type a message, get a response from pi
6. Ctrl-C to exit

## Implementation order

1. Create `bots/pi-bridge.py` (copy from fly-sprites)
2. Create `crates/pika-bot/entrypoint.sh`
3. Create `crates/pika-bot/Dockerfile`
4. Create `fly.pika-bot.toml`
5. Create `cli/src/fly_machines.rs`
6. Add `reqwest` to `cli/Cargo.toml`
7. Add `Agent` subcommand + `cmd_agent_new` to `cli/src/main.rs`
8. `cargo check -p pika-cli` to verify compilation
