mod mdk_util;
mod relay_util;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context};
use clap::{Parser, Subcommand};
use mdk_core::prelude::*;
use nostr_sdk::prelude::*;
use serde_json::json;

// Same defaults as the Pika app (rust/src/core/config.rs).
const DEFAULT_RELAY_URLS: &[&str] = &[
    "wss://relay.damus.io",
    "wss://relay.primal.net",
    "wss://nos.lol",
];

// Key packages (kind 443) are NIP-70 "protected" — many popular relays reject them.
// These relays are known to accept protected kind 443 publishes.
const DEFAULT_KP_RELAY_URLS: &[&str] = &[
    "wss://nostr-pub.wellorder.net",
    "wss://nostr-01.yakihonne.com",
    "wss://nostr-02.yakihonne.com",
];

#[derive(Debug, Parser)]
#[command(name = "pika-cli")]
#[command(about = "Pika CLI — encrypted messaging over Nostr + MLS")]
#[command(after_help = "\x1b[1mQuickstart:\x1b[0m
  1. pika-cli init
  2. pika-cli update-profile --name \"Alice\"
  3. pika-cli send --to npub1... --content \"hello!\"
  4. pika-cli listen")]
struct Cli {
    /// State directory (identity + MLS database persist here between runs)
    #[arg(long, default_value = ".pika-cli")]
    state_dir: PathBuf,

    /// Relay websocket URLs (default: relay.damus.io, relay.primal.net, nos.lol)
    #[arg(long)]
    relay: Vec<String>,

    /// Key-package relay URLs (default: wellorder.net, yakihonne x2, satlantis)
    #[arg(long)]
    kp_relay: Vec<String>,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize your identity and publish a key package so peers can invite you
    #[command(after_help = "Examples:
  pika-cli init
  pika-cli init --nsec nsec1abc...
  pika-cli init --nsec <64-char-hex>")]
    Init {
        /// Nostr secret key to import (nsec1... or hex). Omit to generate a fresh keypair.
        #[arg(long)]
        nsec: Option<String>,
    },

    /// Show (or create) identity for this state dir
    #[command(after_help = "Example:
  pika-cli identity")]
    Identity,

    /// Publish a key package (kind 443) so peers can invite you
    #[command(after_help = "Example:
  pika-cli publish-kp

Note: 'pika-cli init' publishes a key package automatically.
You only need this command to refresh an expired key package.")]
    PublishKp,

    /// Create a group with a peer and send them a welcome
    #[command(after_help = "Examples:
  pika-cli invite --peer npub1xyz...
  pika-cli invite --peer <hex-pubkey> --name \"Book Club\"

Tip: 'pika-cli send --to npub1...' does this automatically for 1:1 DMs.")]
    Invite {
        /// Peer public key (hex or npub)
        #[arg(long)]
        peer: String,

        /// Group name
        #[arg(long, default_value = "DM")]
        name: String,
    },

    /// List pending welcome invitations
    #[command(after_help = "Example:
  pika-cli welcomes")]
    Welcomes,

    /// Accept a pending welcome and join the group
    #[command(after_help = "Example:
  pika-cli welcomes   # find the wrapper_event_id
  pika-cli accept-welcome --wrapper-event-id abc123...")]
    AcceptWelcome {
        /// Wrapper event ID (hex) from the welcomes list
        #[arg(long)]
        wrapper_event_id: String,
    },

    /// List groups you are a member of
    #[command(after_help = "Example:
  pika-cli groups")]
    Groups,

    /// Send a message to a group or a peer
    #[command(after_help = "Examples:
  pika-cli send --to npub1xyz... --content \"hey!\"
  pika-cli send --group <hex-group-id> --content \"hello\"

When using --to, pika-cli searches your groups for an existing 1:1 DM.
If none exists, it automatically creates one and sends your message.")]
    Send {
        /// Nostr group ID (hex) — send directly to this group
        #[arg(long, conflicts_with = "to")]
        group: Option<String>,

        /// Peer public key (npub or hex) — find or create a 1:1 DM with this peer
        #[arg(long, conflicts_with = "group")]
        to: Option<String>,

        /// Message content
        #[arg(long)]
        content: String,
    },

    /// Fetch and decrypt recent messages from a group
    #[command(after_help = "Example:
  pika-cli messages --group <hex-group-id>
  pika-cli messages --group <hex-group-id> --limit 10")]
    Messages {
        /// Nostr group ID (hex)
        #[arg(long)]
        group: String,

        /// Max messages to return
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },

    /// View your Nostr profile (kind-0 metadata)
    #[command(after_help = "Example:
  pika-cli profile")]
    Profile,

    /// Update your Nostr profile (kind-0 metadata)
    #[command(after_help = "Examples:
  pika-cli update-profile --name \"Alice\"
  pika-cli update-profile --picture ./avatar.jpg
  pika-cli update-profile --name \"Alice\" --picture ./avatar.jpg")]
    UpdateProfile {
        /// Set display name
        #[arg(long)]
        name: Option<String>,

        /// Upload a profile picture from a local file (JPEG/PNG, max 8 MB)
        #[arg(long)]
        picture: Option<PathBuf>,
    },

    /// Listen for incoming messages (runs until interrupted or --timeout)
    #[command(after_help = "Examples:
  pika-cli listen                    # listen for 60 seconds
  pika-cli listen --timeout 0        # listen forever (ctrl-c to stop)
  pika-cli listen --timeout 300      # listen for 5 minutes")]
    Listen {
        /// Timeout in seconds (0 = run forever)
        #[arg(long, default_value_t = 60)]
        timeout: u64,

        /// Giftwrap lookback in seconds
        #[arg(long, default_value_t = 86400)]
        lookback: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .without_time()
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.state_dir)
        .with_context(|| format!("create state dir {}", cli.state_dir.display()))?;

    match &cli.cmd {
        Command::Init { nsec } => cmd_init(&cli, nsec.as_deref()).await,
        Command::Identity => cmd_identity(&cli),
        Command::PublishKp => cmd_publish_kp(&cli).await,
        Command::Invite { peer, name } => cmd_invite(&cli, peer, name).await,
        Command::Welcomes => cmd_welcomes(&cli),
        Command::AcceptWelcome { wrapper_event_id } => cmd_accept_welcome(&cli, wrapper_event_id),
        Command::Groups => cmd_groups(&cli),
        Command::Send { group, to, content } => {
            cmd_send(&cli, group.as_deref(), to.as_deref(), content).await
        }
        Command::Messages { group, limit } => cmd_messages(&cli, group, *limit),
        Command::Profile => cmd_profile(&cli).await,
        Command::UpdateProfile { name, picture } => {
            cmd_update_profile(&cli, name.as_deref(), picture.as_deref()).await
        }
        Command::Listen { timeout, lookback } => cmd_listen(&cli, *timeout, *lookback).await,
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn open(cli: &Cli) -> anyhow::Result<(Keys, mdk_util::PikaMdk)> {
    let keys = mdk_util::load_or_create_keys(&cli.state_dir.join("identity.json"))?;
    let mdk = mdk_util::open_mdk(&cli.state_dir)?;
    Ok((keys, mdk))
}

/// Resolve message relay URLs: use --relay if provided, otherwise defaults.
fn resolve_relays(cli: &Cli) -> Vec<String> {
    if cli.relay.is_empty() {
        DEFAULT_RELAY_URLS.iter().map(|s| s.to_string()).collect()
    } else {
        cli.relay.clone()
    }
}

/// Resolve key-package relay URLs: use --kp-relay if provided, otherwise defaults.
fn resolve_kp_relays(cli: &Cli) -> Vec<String> {
    if cli.kp_relay.is_empty() {
        DEFAULT_KP_RELAY_URLS
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        cli.kp_relay.clone()
    }
}

/// Union of message + key-package relays (deduped).
fn resolve_all_relays(cli: &Cli) -> Vec<String> {
    let mut all = resolve_relays(cli);
    for kp in resolve_kp_relays(cli) {
        if !all.contains(&kp) {
            all.push(kp);
        }
    }
    all
}

async fn client(cli: &Cli, keys: &Keys) -> anyhow::Result<Client> {
    let relays = resolve_relays(cli);
    relay_util::connect_client(keys, &relays).await
}

/// Connect to both message and key-package relays.
async fn client_all(cli: &Cli, keys: &Keys) -> anyhow::Result<Client> {
    let relays = resolve_all_relays(cli);
    relay_util::connect_client(keys, &relays).await
}

fn find_group(
    mdk: &mdk_util::PikaMdk,
    nostr_group_id_hex: &str,
) -> anyhow::Result<mdk_storage_traits::groups::types::Group> {
    let gid_bytes = hex::decode(nostr_group_id_hex).context("decode group id hex")?;
    let groups = mdk.get_groups().context("get_groups")?;
    groups
        .into_iter()
        .find(|g| g.nostr_group_id.as_slice() == gid_bytes.as_slice())
        .ok_or_else(|| {
            anyhow!(
                "no group with ID {nostr_group_id_hex}. Run 'pika-cli groups' to list your groups."
            )
        })
}

fn print(v: serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(&v).expect("json encode"));
}

// ── Commands ────────────────────────────────────────────────────────────────

async fn cmd_init(cli: &Cli, nsec: Option<&str>) -> anyhow::Result<()> {
    let identity_path = cli.state_dir.join("identity.json");
    let db_path = cli.state_dir.join("mdk.sqlite");

    // Resolve or generate keys.
    let keys = match nsec {
        Some(s) => Keys::parse(s.trim())
            .context("invalid nsec — expected nsec1... (bech32) or 64-char hex secret key")?,
        None => Keys::generate(),
    };

    let new_pubkey = keys.public_key().to_hex();

    // Check for conflicts with existing state.
    let mut warnings: Vec<String> = Vec::new();

    if identity_path.exists() {
        let raw = std::fs::read_to_string(&identity_path).context("read existing identity.json")?;
        let existing: mdk_util::IdentityFile =
            serde_json::from_str(&raw).context("parse existing identity.json")?;

        if existing.public_key_hex == new_pubkey {
            eprintln!("[pika-cli] identity.json already matches this pubkey — no changes needed.");
            // Still publish key package (idempotent).
        } else {
            warnings.push(format!(
                "identity.json exists with a DIFFERENT pubkey (existing={}, new={})",
                existing.public_key_hex, new_pubkey,
            ));
        }
    }

    if db_path.exists() {
        warnings.push(format!(
            "mdk.sqlite exists at {}; it may contain MLS state from a previous identity. \
             Consider removing it if you are switching keys.",
            db_path.display(),
        ));
    }

    // Prompt for confirmation if there are warnings.
    if !warnings.is_empty() {
        for w in &warnings {
            eprintln!("[pika-cli] WARNING: {w}");
        }
        eprint!("[pika-cli] Continue anyway? (yes/abort): ");
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .context("read user input")?;
        if answer.trim().to_lowercase() != "yes" {
            anyhow::bail!("aborted by user");
        }
    }

    // Write identity.json.
    let id_file = mdk_util::IdentityFile {
        secret_key_hex: keys.secret_key().to_secret_hex(),
        public_key_hex: new_pubkey.clone(),
    };
    std::fs::write(
        &identity_path,
        format!("{}\n", serde_json::to_string_pretty(&id_file)?),
    )
    .context("write identity.json")?;

    // Publish a key package so the user is immediately invitable.
    let mdk = mdk_util::open_mdk(&cli.state_dir)?;
    let kp_relays_str = resolve_kp_relays(cli);
    let kp_relays = relay_util::parse_relay_urls(&kp_relays_str)?;
    let client = relay_util::connect_client(&keys, &kp_relays_str).await?;

    let (content, tags, _hash_ref) = mdk
        .create_key_package_for_event(&keys.public_key(), kp_relays.clone())
        .context("create key package")?;

    let tags: Tags = tags
        .into_iter()
        .filter(|t: &Tag| !matches!(t.kind(), TagKind::Protected))
        .collect();

    let event = EventBuilder::new(Kind::MlsKeyPackage, content)
        .tags(tags)
        .sign_with_keys(&keys)
        .context("sign key package event")?;

    relay_util::publish_and_confirm(&client, &kp_relays, &event, "keypackage").await?;
    client.shutdown().await;

    print(json!({
        "pubkey": keys.public_key().to_hex(),
        "npub": keys.public_key().to_bech32().unwrap_or_default(),
        "key_package_event_id": event.id.to_hex(),
    }));
    Ok(())
}

fn cmd_identity(cli: &Cli) -> anyhow::Result<()> {
    let keys = mdk_util::load_or_create_keys(&cli.state_dir.join("identity.json"))?;
    print(json!({
        "pubkey": keys.public_key().to_hex(),
        "npub": keys.public_key().to_bech32().unwrap_or_default(),
    }));
    Ok(())
}

async fn cmd_publish_kp(cli: &Cli) -> anyhow::Result<()> {
    let (keys, mdk) = open(cli)?;
    let kp_relays_str = resolve_kp_relays(cli);
    let client = relay_util::connect_client(&keys, &kp_relays_str).await?;
    let relays = relay_util::parse_relay_urls(&kp_relays_str)?;

    let (content, tags, _hash_ref) = mdk
        .create_key_package_for_event(&keys.public_key(), relays.clone())
        .context("create key package")?;

    // Strip NIP-70 "protected" tag — many popular relays reject protected events.
    let tags: Tags = tags
        .into_iter()
        .filter(|t: &Tag| !matches!(t.kind(), TagKind::Protected))
        .collect();

    let event = EventBuilder::new(Kind::MlsKeyPackage, content)
        .tags(tags)
        .sign_with_keys(&keys)
        .context("sign key package event")?;

    relay_util::publish_and_confirm(&client, &relays, &event, "keypackage").await?;
    client.shutdown().await;

    print(json!({
        "event_id": event.id.to_hex(),
        "kind": 443,
    }));
    Ok(())
}

async fn cmd_invite(cli: &Cli, peer_str: &str, group_name: &str) -> anyhow::Result<()> {
    let (keys, mdk) = open(cli)?;
    let client = client_all(cli, &keys).await?;
    let relays = relay_util::parse_relay_urls(&resolve_relays(cli))?;
    let kp_relays = relay_util::parse_relay_urls(&resolve_kp_relays(cli))?;

    let peer_pubkey =
        PublicKey::parse(peer_str.trim()).with_context(|| format!("parse peer key: {peer_str}"))?;

    // Fetch peer key package from key-package relays.
    let peer_kp = relay_util::fetch_latest_key_package(
        &client,
        &peer_pubkey,
        &kp_relays,
        Duration::from_secs(10),
    )
    .await
    .context("fetch peer key package — has the peer run `publish-kp`?")?;

    // Create group.
    let config = NostrGroupConfigData::new(
        group_name.to_string(),
        String::new(),
        None,
        None,
        None,
        relays.clone(),
        vec![keys.public_key(), peer_pubkey],
    );

    let result = mdk
        .create_group(&keys.public_key(), vec![peer_kp], config)
        .context("create group")?;

    let ngid = hex::encode(result.group.nostr_group_id);

    // Send welcome giftwraps.
    for rumor in result.welcome_rumors {
        let giftwrap = EventBuilder::gift_wrap(&keys, &peer_pubkey, rumor, [])
            .await
            .context("build giftwrap")?;
        relay_util::publish_and_confirm(&client, &relays, &giftwrap, "welcome").await?;
    }

    client.shutdown().await;

    print(json!({
        "nostr_group_id": ngid,
        "mls_group_id": hex::encode(result.group.mls_group_id.as_slice()),
        "peer_pubkey": peer_pubkey.to_hex(),
    }));
    Ok(())
}

fn cmd_welcomes(cli: &Cli) -> anyhow::Result<()> {
    let (_keys, mdk) = open(cli)?;
    let pending = mdk
        .get_pending_welcomes(None)
        .context("get pending welcomes")?;
    let out: Vec<serde_json::Value> = pending
        .iter()
        .map(|w| {
            json!({
                "wrapper_event_id": w.wrapper_event_id.to_hex(),
                "from_pubkey": w.welcomer.to_hex(),
                "nostr_group_id": hex::encode(w.nostr_group_id),
                "group_name": w.group_name,
            })
        })
        .collect();
    print(json!({ "welcomes": out }));
    Ok(())
}

fn cmd_accept_welcome(cli: &Cli, wrapper_event_id_hex: &str) -> anyhow::Result<()> {
    let (_keys, mdk) = open(cli)?;
    let wrapper_id = EventId::from_hex(wrapper_event_id_hex).context("parse wrapper event id")?;

    let pending = mdk
        .get_pending_welcomes(None)
        .context("get pending welcomes")?;
    let welcome = pending
        .into_iter()
        .find(|w| w.wrapper_event_id == wrapper_id)
        .ok_or_else(|| anyhow!("no pending welcome with that wrapper_event_id"))?;

    let ngid = hex::encode(welcome.nostr_group_id);
    let mls_gid = hex::encode(welcome.mls_group_id.as_slice());

    mdk.accept_welcome(&welcome).context("accept welcome")?;

    print(json!({
        "nostr_group_id": ngid,
        "mls_group_id": mls_gid,
    }));
    Ok(())
}

fn cmd_groups(cli: &Cli) -> anyhow::Result<()> {
    let (_keys, mdk) = open(cli)?;
    let groups = mdk.get_groups().context("get groups")?;
    let out: Vec<serde_json::Value> = groups
        .iter()
        .map(|g| {
            json!({
                "nostr_group_id": hex::encode(g.nostr_group_id),
                "mls_group_id": hex::encode(g.mls_group_id.as_slice()),
                "name": g.name,
                "description": g.description,
            })
        })
        .collect();
    print(json!({ "groups": out }));
    Ok(())
}

async fn cmd_send(
    cli: &Cli,
    group_hex: Option<&str>,
    to_str: Option<&str>,
    content: &str,
) -> anyhow::Result<()> {
    if group_hex.is_none() && to_str.is_none() {
        anyhow::bail!(
            "either --group or --to is required.\n\
             Use --group <HEX> to send to a known group, or --to <NPUB> to send to a peer."
        );
    }

    let (keys, mdk) = open(cli)?;

    match (group_hex, to_str) {
        (Some(gid), _) => {
            // Direct send to a known group — original behavior.
            let client = client(cli, &keys).await?;
            let relays = relay_util::parse_relay_urls(&resolve_relays(cli))?;
            let group = find_group(&mdk, gid)?;
            let ngid = hex::encode(group.nostr_group_id);

            let rumor = EventBuilder::new(Kind::ChatMessage, content).build(keys.public_key());
            let msg_event = mdk
                .create_message(&group.mls_group_id, rumor)
                .context("create message")?;
            relay_util::publish_and_confirm(&client, &relays, &msg_event, "send_message").await?;
            client.shutdown().await;

            print(json!({
                "event_id": msg_event.id.to_hex(),
                "nostr_group_id": ngid,
            }));
        }
        (_, Some(peer_str)) => {
            // Smart send: find existing 1:1 DM or auto-create one.
            let peer_pubkey = PublicKey::parse(peer_str.trim())
                .with_context(|| format!("parse peer key: {peer_str}"))?;
            let my_pubkey = keys.public_key();

            // Search for an existing 1:1 DM with this peer.
            let groups = mdk.get_groups().context("get groups")?;
            let mut found_group = None;
            for g in &groups {
                let members = mdk.get_members(&g.mls_group_id).unwrap_or_default();
                let others: Vec<_> = members.iter().filter(|p| *p != &my_pubkey).collect();
                if others.len() == 1 && *others[0] == peer_pubkey {
                    found_group = Some(g.clone());
                    break;
                }
            }

            if let Some(group) = found_group {
                // Existing DM found — send to it.
                let client = client(cli, &keys).await?;
                let relays = relay_util::parse_relay_urls(&resolve_relays(cli))?;
                let ngid = hex::encode(group.nostr_group_id);

                let rumor = EventBuilder::new(Kind::ChatMessage, content).build(keys.public_key());
                let msg_event = mdk
                    .create_message(&group.mls_group_id, rumor)
                    .context("create message")?;
                relay_util::publish_and_confirm(&client, &relays, &msg_event, "send_message")
                    .await?;
                client.shutdown().await;

                print(json!({
                    "event_id": msg_event.id.to_hex(),
                    "nostr_group_id": ngid,
                }));
            } else {
                // No existing DM — create one, send welcome + message.
                let client = client_all(cli, &keys).await?;
                let relays = relay_util::parse_relay_urls(&resolve_relays(cli))?;
                let kp_relays = relay_util::parse_relay_urls(&resolve_kp_relays(cli))?;

                let peer_kp = relay_util::fetch_latest_key_package(
                    &client,
                    &peer_pubkey,
                    &kp_relays,
                    Duration::from_secs(10),
                )
                .await
                .context("fetch peer key package — has the peer run `pika-cli init`?")?;

                let config = NostrGroupConfigData::new(
                    "DM".to_string(),
                    String::new(),
                    None,
                    None,
                    None,
                    relays.clone(),
                    vec![my_pubkey, peer_pubkey],
                );

                let result = mdk
                    .create_group(&my_pubkey, vec![peer_kp], config)
                    .context("create group")?;

                let ngid = hex::encode(result.group.nostr_group_id);

                // Send welcome giftwraps.
                for rumor in result.welcome_rumors {
                    let giftwrap = EventBuilder::gift_wrap(&keys, &peer_pubkey, rumor, [])
                        .await
                        .context("build giftwrap")?;
                    relay_util::publish_and_confirm(&client, &relays, &giftwrap, "welcome").await?;
                }

                // Send the message.
                let rumor = EventBuilder::new(Kind::ChatMessage, content).build(keys.public_key());
                let msg_event = mdk
                    .create_message(&result.group.mls_group_id, rumor)
                    .context("create message")?;
                relay_util::publish_and_confirm(&client, &relays, &msg_event, "send_message")
                    .await?;
                client.shutdown().await;

                print(json!({
                    "event_id": msg_event.id.to_hex(),
                    "nostr_group_id": ngid,
                    "auto_created_group": true,
                }));
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

fn cmd_messages(cli: &Cli, nostr_group_id_hex: &str, limit: usize) -> anyhow::Result<()> {
    let (_keys, mdk) = open(cli)?;
    let group = find_group(&mdk, nostr_group_id_hex)?;

    let pagination = mdk_storage_traits::groups::Pagination::new(Some(limit), None);
    let msgs = mdk
        .get_messages(&group.mls_group_id, Some(pagination))
        .context("get messages")?;

    let out: Vec<serde_json::Value> = msgs
        .iter()
        .map(|m| {
            json!({
                "message_id": m.id.to_hex(),
                "from_pubkey": m.pubkey.to_hex(),
                "content": m.content,
                "created_at": m.created_at.as_secs(),
            })
        })
        .collect();
    print(json!({ "messages": out }));
    Ok(())
}

const DEFAULT_BLOSSOM_SERVER: &str = "https://blossom.yakihonne.com";
const MAX_PROFILE_IMAGE_BYTES: usize = 8 * 1024 * 1024;

async fn cmd_profile(cli: &Cli) -> anyhow::Result<()> {
    let (keys, _mdk) = open(cli)?;
    let client = client(cli, &keys).await?;

    client.wait_for_connection(Duration::from_secs(4)).await;
    let metadata = client
        .fetch_metadata(keys.public_key(), Duration::from_secs(8))
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    client.shutdown().await;

    print(json!({
        "pubkey": keys.public_key().to_hex(),
        "npub": keys.public_key().to_bech32().unwrap_or_default(),
        "name": metadata.name,
        "about": metadata.about,
        "picture_url": metadata.picture,
    }));
    Ok(())
}

async fn cmd_update_profile(
    cli: &Cli,
    name: Option<&str>,
    picture: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    if name.is_none() && picture.is_none() {
        anyhow::bail!(
            "at least one of --name or --picture is required.\n\
             Use 'pika-cli profile' to view your current profile."
        );
    }

    let (keys, _mdk) = open(cli)?;
    let client = client(cli, &keys).await?;

    // Fetch current metadata to preserve fields we don't edit.
    client.wait_for_connection(Duration::from_secs(4)).await;
    let mut metadata = client
        .fetch_metadata(keys.public_key(), Duration::from_secs(8))
        .await
        .ok()
        .flatten()
        .unwrap_or_default();

    // Apply name update.
    if let Some(n) = name {
        let trimmed = n.trim();
        if trimmed.is_empty() {
            metadata.name = None;
            metadata.display_name = None;
        } else {
            metadata.name = Some(trimmed.to_string());
            metadata.display_name = Some(trimmed.to_string());
        }
    }

    // Upload picture if provided.
    if let Some(path) = picture {
        let image_bytes =
            std::fs::read(path).with_context(|| format!("read image file: {}", path.display()))?;
        if image_bytes.is_empty() {
            anyhow::bail!("image file is empty");
        }
        if image_bytes.len() > MAX_PROFILE_IMAGE_BYTES {
            anyhow::bail!("image too large ({} bytes, max 8 MB)", image_bytes.len());
        }

        // Infer MIME type from extension.
        let mime_type = match path.extension().and_then(|e| e.to_str()) {
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("png") => "image/png",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => "image/jpeg", // default fallback
        };

        let base_url =
            nostr_sdk::Url::parse(DEFAULT_BLOSSOM_SERVER).context("parse blossom server URL")?;
        let blossom = nostr_blossom::client::BlossomClient::new(base_url);
        let descriptor = blossom
            .upload_blob(image_bytes, Some(mime_type.to_string()), None, Some(&keys))
            .await
            .context("blossom upload failed — is the server reachable?")?;
        metadata.picture = Some(descriptor.url.to_string());
    }

    // Publish updated metadata.
    let output = client
        .set_metadata(&metadata)
        .await
        .context("publish metadata")?;
    if output.success.is_empty() {
        let reasons: Vec<String> = output.failed.values().cloned().collect();
        anyhow::bail!("no relay accepted profile update: {reasons:?}");
    }
    client.shutdown().await;

    print(json!({
        "pubkey": keys.public_key().to_hex(),
        "npub": keys.public_key().to_bech32().unwrap_or_default(),
        "name": metadata.name,
        "about": metadata.about,
        "picture_url": metadata.picture,
    }));
    Ok(())
}

/// Listen for new incoming messages and welcomes. Prints each as a JSON line to stdout.
/// This is the one subcommand that *does* stay running — it's an event tail.
async fn cmd_listen(cli: &Cli, timeout_sec: u64, lookback_sec: u64) -> anyhow::Result<()> {
    let (keys, mdk) = open(cli)?;
    let client = client(cli, &keys).await?;

    let mut rx = client.notifications();

    // Subscribe to giftwrap (welcomes).
    let since = Timestamp::now() - Duration::from_secs(lookback_sec);
    // Giftwraps are authored by the sender; recipients are indicated via the `p` tag.
    // Filtering by `pubkey(...)` would only match events *we* authored and would miss inbound invites.
    let gift_filter = Filter::new()
        .kind(Kind::GiftWrap)
        .custom_tag(
            SingleLetterTag::lowercase(Alphabet::P),
            keys.public_key().to_hex(),
        )
        .since(since)
        .limit(200);
    let gift_sub = client.subscribe(gift_filter, None).await?;

    // Subscribe to all known groups.
    let mut group_subs = std::collections::HashMap::<SubscriptionId, String>::new();
    if let Ok(groups) = mdk.get_groups() {
        for g in &groups {
            let ngid = hex::encode(g.nostr_group_id);
            let filter = Filter::new()
                .kind(Kind::MlsGroupMessage)
                .custom_tag(SingleLetterTag::lowercase(Alphabet::H), &ngid)
                .since(Timestamp::now() - Duration::from_secs(lookback_sec))
                .limit(200);
            if let Ok(out) = client.subscribe(filter, None).await {
                group_subs.insert(out.val, ngid);
            }
        }
    }

    let mut seen = std::collections::HashSet::<EventId>::new();

    let deadline = if timeout_sec == 0 {
        None
    } else {
        Some(tokio::time::Instant::now() + Duration::from_secs(timeout_sec))
    };

    loop {
        let recv_fut = rx.recv();
        let notification = if let Some(dl) = deadline {
            let remaining = dl.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, recv_fut).await {
                Ok(Ok(n)) => n,
                Ok(Err(_)) => break,
                Err(_) => break, // timeout
            }
        } else {
            match recv_fut.await {
                Ok(n) => n,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        };

        let RelayPoolNotification::Event {
            subscription_id,
            event,
            ..
        } = notification
        else {
            continue;
        };
        let event = *event;

        if !seen.insert(event.id) {
            continue;
        }

        // Welcome.
        if subscription_id == gift_sub.val && event.kind == Kind::GiftWrap {
            let unwrapped = match nostr_sdk::nostr::nips::nip59::extract_rumor(&keys, &event).await
            {
                Ok(u) => u,
                Err(_) => continue,
            };
            if unwrapped.rumor.kind != Kind::MlsWelcome {
                continue;
            }
            let rumor = unwrapped.rumor;
            if mdk.process_welcome(&event.id, &rumor).is_err() {
                continue;
            }
            let (ngid, group_name) = match mdk.get_pending_welcomes(None) {
                Ok(list) => list
                    .into_iter()
                    .find(|w| w.wrapper_event_id == event.id)
                    .map(|w| (hex::encode(w.nostr_group_id), w.group_name))
                    .unwrap_or_default(),
                Err(_) => (String::new(), String::new()),
            };
            let line = json!({
                "type": "welcome",
                "wrapper_event_id": event.id.to_hex(),
                "from_pubkey": unwrapped.sender.to_hex(),
                "nostr_group_id": ngid,
                "group_name": group_name,
            });
            println!("{}", serde_json::to_string(&line).unwrap());
            continue;
        }

        // Group message.
        if event.kind == Kind::MlsGroupMessage && group_subs.contains_key(&subscription_id) {
            if let Ok(MessageProcessingResult::ApplicationMessage(msg)) =
                mdk.process_message(&event)
            {
                let ngid = group_subs
                    .get(&subscription_id)
                    .cloned()
                    .unwrap_or_default();
                let line = json!({
                    "type": "message",
                    "nostr_group_id": ngid,
                    "from_pubkey": msg.pubkey.to_hex(),
                    "content": msg.content,
                    "created_at": msg.created_at.as_secs(),
                    "message_id": msg.id.to_hex(),
                });
                println!("{}", serde_json::to_string(&line).unwrap());
            }
        }
    }

    client.unsubscribe_all().await;
    client.shutdown().await;
    Ok(())
}
