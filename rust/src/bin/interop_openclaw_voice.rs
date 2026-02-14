//! Laptop-to-deployed-bot call smoke test.
//!
//! Purpose: validate MLS/Nostr signaling + real MOQ media transport end-to-end against the
//! deployed OpenClaw Marmot bot, without requiring a microphone (uses synthetic audio).
//!
//! Usage:
//!   cargo run -p pika_core --bin interop_openclaw_voice -- <bot_npub>
//!
//! Env:
//!   PIKA_TEST_NSEC: required (nsec1...)
//!   PIKA_E2E_RELAYS / PIKA_E2E_KP_RELAYS: optional comma-separated relay URL lists
//!   PIKA_CALL_MOQ_URL: optional (default: https://moq.justinmoon.com/anon)
//!   PIKA_CALL_BROADCAST_PREFIX: optional (default: pika/calls)
//!
//! Exit code:
//!   0 on PASS
//!   1 on failure

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nostr_sdk::prelude::{Client, Filter, Keys, Kind, PublicKey, RelayPoolNotification};
use pika_core::{AppAction, AppReconciler, AppUpdate, AuthState, CallStatus, FfiApp};

const DEFAULT_RELAYS: &[&str] = &[
    "wss://relay.primal.net",
    "wss://nos.lol",
    "wss://relay.damus.io",
];

// Match rust/src/core/config.rs defaults: protected kind 443 publishes require relays that accept them.
const DEFAULT_KEY_PACKAGE_RELAYS: &[&str] = &[
    "wss://nostr-pub.wellorder.net",
    "wss://nostr-01.yakihonne.com",
    "wss://nostr-02.yakihonne.com",
    "wss://relay.satlantis.io",
];

const DEFAULT_MOQ_URL: &str = "https://moq.justinmoon.com/anon";
const DEFAULT_BROADCAST_PREFIX: &str = "pika/calls";

fn usage() -> ! {
    eprintln!(
        "usage: interop_openclaw_voice <bot_npub>\n\
         \n\
         requires env: PIKA_TEST_NSEC\n\
         optional env: PIKA_E2E_RELAYS, PIKA_E2E_KP_RELAYS, PIKA_CALL_MOQ_URL, PIKA_CALL_BROADCAST_PREFIX\n"
    );
    std::process::exit(2);
}

fn wait_until(what: &str, timeout: Duration, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("{what}: condition not met within {timeout:?}");
}

fn parse_csv_env(key: &str) -> Option<Vec<String>> {
    let s = std::env::var(key).ok()?;
    let v: Vec<String> = s
        .split(',')
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(|x| x.to_string())
        .collect();
    if v.is_empty() { None } else { Some(v) }
}

fn default_relays() -> Vec<String> {
    DEFAULT_RELAYS.iter().map(|s| s.to_string()).collect()
}

fn relays() -> Vec<String> {
    parse_csv_env("PIKA_E2E_RELAYS").unwrap_or_else(default_relays)
}

fn kp_relays() -> Vec<String> {
    parse_csv_env("PIKA_E2E_KP_RELAYS").unwrap_or_else(|| {
        DEFAULT_KEY_PACKAGE_RELAYS
            .iter()
            .map(|s| s.to_string())
            .collect()
    })
}

fn call_moq_url() -> String {
    std::env::var("PIKA_CALL_MOQ_URL").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_MOQ_URL.to_string())
}

fn call_broadcast_prefix() -> String {
    std::env::var("PIKA_CALL_BROADCAST_PREFIX")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BROADCAST_PREFIX.to_string())
}

fn find_repo_env_file() -> Option<PathBuf> {
    // Prefer explicit override.
    if let Ok(p) = std::env::var("PIKA_ENV_PATH") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }

    // Try a few common locations relative to CWD.
    let mut dir = std::env::current_dir().ok()?;
    for _ in 0..6 {
        let candidate = dir.join(".env");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn read_env_var_from_file(path: &Path, key: &str) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        if k.trim() != key {
            continue;
        }
        let mut v = v.trim().to_string();
        // Strip single/double quotes if present.
        if (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')) {
            v = v[1..v.len() - 1].to_string();
        }
        if !v.is_empty() {
            return Some(v);
        }
    }
    None
}

fn test_nsec() -> String {
    if let Ok(v) = std::env::var("PIKA_TEST_NSEC") {
        let t = v.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    if let Some(path) = find_repo_env_file() {
        if let Some(v) = read_env_var_from_file(&path, "PIKA_TEST_NSEC") {
            return v;
        }
    }
    eprintln!("missing PIKA_TEST_NSEC (set env var, or put it in a .env file)");
    std::process::exit(1);
}

async fn wait_for_keypackage_on_any_relay(
    relays: &[String],
    pubkey: PublicKey,
    timeout: Duration,
) -> bool {
    let keys = Keys::generate();
    let client = Client::new(keys);
    for r in relays {
        let _ = client.add_relay(r).await;
    }
    client.connect().await;

    let filter = Filter::new()
        .author(pubkey)
        .kind(Kind::MlsKeyPackage)
        .limit(1);
    let _ = client.subscribe(filter, None).await;

    let mut rx = client.notifications();
    let start = Instant::now();
    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            Ok(Ok(RelayPoolNotification::Event { event, .. })) => {
                if event.kind == Kind::MlsKeyPackage && event.pubkey == pubkey {
                    return true;
                }
            }
            Ok(Ok(_other)) => {}
            Ok(Err(_closed)) => return false,
            Err(_elapsed) => {}
        }
    }
    false
}

#[derive(Clone)]
struct Collector(std::sync::Arc<std::sync::Mutex<Vec<AppUpdate>>>);

impl Collector {
    fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())))
    }

    fn last_toast(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|u| match u {
                AppUpdate::FullState(s) => s.toast.clone(),
                _ => None,
            })
    }
}

impl AppReconciler for Collector {
    fn reconcile(&self, update: AppUpdate) {
        self.0.lock().unwrap().push(update);
    }
}

fn write_config(
    data_dir: &Path,
    relays: &[String],
    kp_relays: &[String],
    moq_url: &str,
    broadcast_prefix: &str,
) {
    let path = data_dir.join("pika_config.json");
    let v = serde_json::json!({
        "disable_network": false,
        "relay_urls": relays,
        "key_package_relay_urls": kp_relays,
        "call_moq_url": moq_url,
        "call_broadcast_prefix": broadcast_prefix,
        "call_audio_backend": "synthetic",
    });
    fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
}

fn main() {
    // Must run before any rustls users initialize (nostr-sdk websockets, quinn/moq, etc).
    pika_core::init_rustls_crypto_provider();

    let mut args = std::env::args().skip(1);
    let bot_npub = args.next().unwrap_or_else(|| usage());
    if args.next().is_some() {
        usage();
    }

    let relays = relays();
    let kp_relays = kp_relays();
    let moq_url = call_moq_url();
    let broadcast_prefix = call_broadcast_prefix();
    let nsec = test_nsec();

    eprintln!("relays={relays:?}");
    eprintln!("kp_relays={kp_relays:?}");
    eprintln!("call_moq_url={moq_url}");
    eprintln!("call_broadcast_prefix={broadcast_prefix}");

    let bot_pubkey = match PublicKey::parse(&bot_npub) {
        Ok(pk) => pk,
        Err(err) => {
            eprintln!("invalid bot npub: {bot_npub} ({err})");
            std::process::exit(1);
        }
    };

    // Unique state dir (keep it around on failure for inspection).
    let data_dir = std::env::temp_dir().join(format!("pika-interop-openclaw-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&data_dir).expect("create data dir");
    write_config(&data_dir, &relays, &kp_relays, &moq_url, &broadcast_prefix);

    // Best-effort: confirm the bot key package is visible before attempting CreateChat.
    // If this fails we still proceed, but CreateChat will likely toast an error.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let kp_ok = rt.block_on(wait_for_keypackage_on_any_relay(
        &kp_relays,
        bot_pubkey,
        Duration::from_secs(60),
    ));
    if !kp_ok {
        eprintln!("warn: bot key package (kind 443) not observed on kp_relays within timeout; continuing anyway");
    }

    let app = FfiApp::new(data_dir.to_string_lossy().to_string());
    let collector = Collector::new();
    app.listen_for_updates(Box::new(collector.clone()));

    app.dispatch(AppAction::Login { nsec });
    wait_until("logged in", Duration::from_secs(20), || {
        matches!(app.state().auth, AuthState::LoggedIn { .. })
    });

    app.dispatch(AppAction::CreateChat {
        peer_npub: bot_npub.clone(),
    });
    wait_until("chat opened", Duration::from_secs(120), || app.state().current_chat.is_some());

    let chat_id = app.state().current_chat.as_ref().unwrap().chat_id.clone();
    eprintln!("chat_id={chat_id}");

    // Deterministic readiness check: bot should reply pong:<nonce> without invoking an LLM.
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let ping = format!("ping:{nonce}");
    let pong = format!("pong:{nonce}");
    app.dispatch(AppAction::SendMessage {
        chat_id: chat_id.clone(),
        content: ping,
    });
    wait_until("bot pong received", Duration::from_secs(120), || {
        app.state()
            .current_chat
            .as_ref()
            .and_then(|c| c.messages.iter().find(|m| m.content == pong))
            .is_some()
    });

    app.dispatch(AppAction::StartCall { chat_id });
    wait_until("call active", Duration::from_secs(120), || {
        app.state()
            .active_call
            .as_ref()
            .map(|c| matches!(c.status, CallStatus::Active))
            .unwrap_or(false)
    });

    // Wait for debug to show up.
    wait_until("call debug present", Duration::from_secs(30), || {
        app.state()
            .active_call
            .as_ref()
            .and_then(|c| c.debug.as_ref())
            .is_some()
    });

    let start = app
        .state()
        .active_call
        .as_ref()
        .and_then(|c| c.debug.as_ref())
        .cloned()
        .unwrap();

    // Let media run for a short window; in synthetic mode this should push tx_frames.
    let window = Duration::from_secs(10);
    let mut last_toast = collector.last_toast();
    let t0 = Instant::now();
    while t0.elapsed() < window {
        if let Some(t) = collector.last_toast() {
            if Some(t.clone()) != last_toast {
                eprintln!("toast={t:?}");
                last_toast = Some(t);
            }
        }
        if let Some(dbg) = app.state().active_call.as_ref().and_then(|c| c.debug.as_ref()) {
            eprintln!(
                "call_debug tx={} rx={} drop={} jitter={}ms rtt={:?}",
                dbg.tx_frames, dbg.rx_frames, dbg.rx_dropped, dbg.jitter_buffer_ms, dbg.last_rtt_ms
            );
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    let end = app
        .state()
        .active_call
        .as_ref()
        .and_then(|c| c.debug.as_ref())
        .cloned()
        .unwrap();

    // Best-effort cleanup.
    app.dispatch(AppAction::EndCall);
    wait_until("call ended", Duration::from_secs(30), || {
        app.state()
            .active_call
            .as_ref()
            .map(|c| matches!(c.status, CallStatus::Ended { .. }))
            .unwrap_or(false)
    });

    let tx_delta = end.tx_frames.saturating_sub(start.tx_frames);
    let rx_delta = end.rx_frames.saturating_sub(start.rx_frames);

    if tx_delta < 10 {
        eprintln!(
            "fail: expected tx_frames to increase by >=10 over {:?}, got delta={tx_delta} (start={}, end={})",
            window, start.tx_frames, end.tx_frames
        );
        if let Some(t) = collector.last_toast() {
            eprintln!("last_toast={t:?}");
        }
        eprintln!("state_dir={}", data_dir.to_string_lossy());
        std::process::exit(1);
    }

    if rx_delta == 0 {
        eprintln!("warn: rx_frames did not increase (bot may not be publishing response audio yet)");
    }

    println!("ok: interop openclaw voice PASS");
}
