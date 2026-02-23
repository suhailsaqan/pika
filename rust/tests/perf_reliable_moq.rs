//! Prototype benchmark: Reliable MoQ (MCR-00 style) vs Nostr relays.
//!
//! Run:
//! `cargo test -p pika_core --test perf_reliable_moq -- --ignored --nocapture`

#[path = "support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use nostr_sdk::nostr::{EventBuilder, EventId, Filter, Keys, Kind};
use nostr_sdk::{Client, RelayPoolNotification};
use pika_media::network::NetworkRelay;
use support::reliable_moq::{
    spawn_mcr_http_server, McrClient, McrEnvelope, McrRelayOptions, MoqMirrorConfig,
};
use tokio::runtime::Runtime;

const NOSTR_RELAYS: &[(&str, &str)] = &[
    (
        "us-east.nostr.pikachat.org",
        "wss://us-east.nostr.pikachat.org",
    ),
    ("eu.nostr.pikachat.org", "wss://eu.nostr.pikachat.org"),
    ("relay.primal.net", "wss://relay.primal.net"),
    ("nos.lol", "wss://nos.lol"),
    ("relay.damus.io", "wss://relay.damus.io"),
];

const MOQ_RELAYS: &[(&str, &str)] = &[
    (
        "us-east.moq.pikachat.org",
        "https://us-east.moq.pikachat.org/anon",
    ),
    ("eu.moq.pikachat.org", "https://eu.moq.pikachat.org/anon"),
    (
        "us-west.moq.pikachat.org",
        "https://us-west.moq.pikachat.org/anon",
    ),
    (
        "asia.moq.pikachat.org",
        "https://asia.moq.pikachat.org/anon",
    ),
    (
        "us-east.moq.logos.surf",
        "https://us-east.moq.logos.surf/anon",
    ),
    (
        "us-west.moq.logos.surf",
        "https://us-west.moq.logos.surf/anon",
    ),
    (
        "germany.moq.logos.surf",
        "https://germany.moq.logos.surf/anon",
    ),
    (
        "singapore.moq.logos.surf",
        "https://singapore.moq.logos.surf/anon",
    ),
];

const DEFAULT_MSG_COUNT: usize = 20;
const DEFAULT_RUNS: usize = 3;
const DEFAULT_PAIR_RUNS: usize = 8;
const PAYLOAD_SIZE: usize = 64;

#[derive(Clone, Copy)]
enum BenchPath {
    Nostr {
        name: &'static str,
        url: &'static str,
    },
    ReliableMoq {
        name: &'static str,
        url: &'static str,
    },
}

impl BenchPath {
    fn name(self) -> &'static str {
        match self {
            Self::Nostr { name, .. } => name,
            Self::ReliableMoq { name, .. } => name,
        }
    }

    fn kind(self) -> &'static str {
        match self {
            Self::Nostr { .. } => "nostr",
            Self::ReliableMoq { .. } => "mcr+moq",
        }
    }
}

fn measure_nostr_relay(relay_url: &str, msg_count: usize) -> Result<Vec<Duration>, String> {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let pub_keys = Keys::generate();
        let sub_keys = Keys::generate();
        let kind = Kind::from(20_444);
        let scope = format!("pika-mcr-bench-{}", rand::random::<u64>());

        let sub_client = Client::builder().signer(sub_keys.clone()).build();
        sub_client
            .add_relay(relay_url)
            .await
            .map_err(|e| format!("sub add_relay: {e}"))?;
        sub_client.connect().await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let filter = Filter::new()
            .kind(kind)
            .custom_tag(
                nostr_sdk::nostr::SingleLetterTag::lowercase(nostr_sdk::nostr::Alphabet::Z),
                scope.clone(),
            )
            .since(nostr_sdk::nostr::Timestamp::now());

        sub_client
            .subscribe(filter, None)
            .await
            .map_err(|e| format!("subscribe: {e}"))?;

        let (rx_tx, mut rx_rx) = tokio::sync::mpsc::unbounded_channel::<(EventId, Instant)>();
        let notifications = sub_client.notifications();
        let listener = tokio::spawn(async move {
            let mut notifications = notifications;
            loop {
                match notifications.recv().await {
                    Ok(RelayPoolNotification::Event { event, .. }) => {
                        let _ = rx_tx.send((event.id, Instant::now()));
                    }
                    Ok(RelayPoolNotification::Shutdown) => break,
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        let pub_client = Client::builder().signer(pub_keys.clone()).build();
        pub_client
            .add_relay(relay_url)
            .await
            .map_err(|e| format!("pub add_relay: {e}"))?;
        pub_client.connect().await;
        tokio::time::sleep(Duration::from_millis(500)).await;

        let payload = "x".repeat(PAYLOAD_SIZE);
        let mut send_times: HashMap<EventId, Instant> = HashMap::new();

        for i in 0..msg_count {
            let event = EventBuilder::new(kind, format!("{payload}:{i}"))
                .tag(nostr_sdk::nostr::Tag::custom(
                    nostr_sdk::nostr::TagKind::SingleLetter(
                        nostr_sdk::nostr::SingleLetterTag::lowercase(nostr_sdk::nostr::Alphabet::Z),
                    ),
                    vec![scope.clone()],
                ))
                .sign_with_keys(&pub_keys)
                .map_err(|e| format!("sign: {e}"))?;

            let eid = event.id;
            let t0 = Instant::now();
            pub_client
                .send_event_to(vec![relay_url], &event)
                .await
                .map_err(|e| format!("send: {e}"))?;
            send_times.insert(eid, t0);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut latencies: Vec<Duration> = Vec::new();
        let mut received = 0usize;

        while received < msg_count && Instant::now() < deadline {
            match tokio::time::timeout(
                deadline.saturating_duration_since(Instant::now()),
                rx_rx.recv(),
            )
            .await
            {
                Ok(Some((eid, recv_time))) => {
                    if let Some(send_time) = send_times.get(&eid) {
                        latencies.push(recv_time.duration_since(*send_time));
                        received += 1;
                    }
                }
                _ => break,
            }
        }

        listener.abort();
        let _ = pub_client.disconnect().await;
        let _ = sub_client.disconnect().await;

        if latencies.is_empty() {
            return Err("no messages received".to_string());
        }
        Ok(latencies)
    })
}

fn measure_reliable_moq(relay_url: &str, msg_count: usize) -> Result<Vec<Duration>, String> {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let run_id = format!("{:016x}", rand::random::<u64>());
        let room_id = format!("room-{run_id}");
        let broadcast_base = format!("pika/mcr-bench/{run_id}");

        let opts = McrRelayOptions {
            moq_mirror: Some(MoqMirrorConfig {
                moq_url: relay_url.to_string(),
                broadcast_base: broadcast_base.clone(),
            }),
            ..McrRelayOptions::default()
        };

        let server = spawn_mcr_http_server(opts)
            .await
            .map_err(|e| format!("spawn mcr relay: {e}"))?;

        let track = server
            .relay
            .track_for_room(&room_id)
            .ok_or_else(|| "missing moq track configuration".to_string())?;

        // Ensure the track exists before subscriber attach.
        server.relay.warmup_live_track(&room_id);
        tokio::time::sleep(Duration::from_millis(500)).await;

        let sub_relay = NetworkRelay::new(relay_url).map_err(|e| format!("sub relay init: {e}"))?;
        sub_relay
            .connect()
            .map_err(|e| format!("sub relay connect: {e}"))?;
        let rx = sub_relay
            .subscribe(&track)
            .map_err(|e| format!("sub relay subscribe: {e}"))?;

        let (env_tx, mut env_rx) = tokio::sync::mpsc::unbounded_channel::<(McrEnvelope, Instant)>();
        let reader = std::thread::spawn(move || loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(frame) => {
                    if let Ok(env) = serde_json::from_slice::<McrEnvelope>(&frame.payload) {
                        let _ = env_tx.send((env, Instant::now()));
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        });

        let alice = McrClient::new(server.base_url.clone(), &room_id, "alice");
        let mut bob = McrClient::new(server.base_url.clone(), &room_id, "bob");
        let _ = bob.initial_attach().await;

        let mut latencies = Vec::new();

        for i in 0..msg_count {
            let msg_id = format!("{run_id}-{i}");
            let payload = serde_json::json!({
                "kind":"typing",
                "body":"x".repeat(PAYLOAD_SIZE),
                "idx": i,
            });

            let t0 = Instant::now();
            let receipt = alice
                .publish_with_msg_id(msg_id.clone(), "marmot_app_event", payload)
                .await
                .map_err(|e| format!("publish: {e}"))?;
            if receipt.code.as_deref() != Some("SUCCESS") {
                continue;
            }

            let deadline = Instant::now() + Duration::from_secs(5);
            let mut matched = false;
            while Instant::now() < deadline {
                match tokio::time::timeout(
                    deadline.saturating_duration_since(Instant::now()),
                    env_rx.recv(),
                )
                .await
                {
                    Ok(Some((env, _recv_time))) => {
                        if env.room_id != room_id {
                            continue;
                        }
                        let _ = bob.handle_live(env.clone()).await;
                        if env.msg_id == msg_id {
                            latencies.push(t0.elapsed());
                            matched = true;
                            break;
                        }
                    }
                    _ => break,
                }
            }

            if !matched {
                // Keep run alive; delivery ratio is reported by count.
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        sub_relay.disconnect();
        drop(server);
        let _ = reader.join();

        if latencies.is_empty() {
            return Err("no messages received".to_string());
        }
        Ok(latencies)
    })
}

fn measure_path(path: BenchPath, msg_count: usize) -> Result<Vec<Duration>, String> {
    match path {
        BenchPath::Nostr { url, .. } => measure_nostr_relay(url, msg_count),
        BenchPath::ReliableMoq { url, .. } => measure_reliable_moq(url, msg_count),
    }
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn env_flag(key: &str) -> bool {
    matches!(
        std::env::var(key).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

fn median(vals: &mut [Duration]) -> Duration {
    vals.sort();
    if vals.is_empty() {
        return Duration::ZERO;
    }
    vals[vals.len() / 2]
}

fn mean(vals: &[Duration]) -> Duration {
    if vals.is_empty() {
        return Duration::ZERO;
    }
    let sum: Duration = vals.iter().sum();
    sum / vals.len() as u32
}

fn p95(vals: &mut [Duration]) -> Duration {
    vals.sort();
    if vals.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((vals.len() as f64) * 0.95).ceil() as usize - 1;
    vals[idx.min(vals.len() - 1)]
}

fn fmt_ms(d: Duration) -> String {
    format!("{:.1}ms", d.as_secs_f64() * 1000.0)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn mean_f64(vals: &[f64]) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    vals.iter().sum::<f64>() / vals.len() as f64
}

fn stddev_f64(vals: &[f64]) -> f64 {
    if vals.len() < 2 {
        return 0.0;
    }
    let mean = mean_f64(vals);
    let var = vals
        .iter()
        .map(|v| {
            let d = *v - mean;
            d * d
        })
        .sum::<f64>()
        / (vals.len() as f64 - 1.0);
    var.sqrt()
}

fn median_ms(vals: &[Duration]) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    Some(ms(median(&mut vals.to_vec())))
}

fn recv_pct(latencies: &[Duration], msg_count: usize) -> f64 {
    if msg_count == 0 {
        return 0.0;
    }
    (latencies.len() as f64 / msg_count as f64) * 100.0
}

fn run_pair_ab(label: &str, lhs: BenchPath, rhs: BenchPath, pair_runs: usize, msg_count: usize) {
    println!("--- Paired A/B: {label} ---");
    println!(
        "  lhs={}({})  rhs={}({})  pair-runs={}  msgs/run={}",
        lhs.name(),
        lhs.kind(),
        rhs.name(),
        rhs.kind(),
        pair_runs,
        msg_count
    );

    let mut deltas_ms: Vec<f64> = Vec::new(); // lhs - rhs; positive means rhs faster
    let mut lhs_medians: Vec<f64> = Vec::new();
    let mut rhs_medians: Vec<f64> = Vec::new();
    let mut rhs_wins = 0usize;
    let mut comparisons = 0usize;
    let mut lhs_reliable = 0usize;
    let mut rhs_reliable = 0usize;

    for run in 0..pair_runs {
        let flipped = run % 2 == 1;
        let (first, second) = if flipped { (rhs, lhs) } else { (lhs, rhs) };

        let first_out = measure_path(first, msg_count);
        std::thread::sleep(Duration::from_millis(250));
        let second_out = measure_path(second, msg_count);

        let (lhs_out, rhs_out) = if flipped {
            (second_out, first_out)
        } else {
            (first_out, second_out)
        };

        match (&lhs_out, &rhs_out) {
            (Ok(lhs_lats), Ok(rhs_lats)) => {
                let lhs_med = median_ms(lhs_lats).unwrap_or(0.0);
                let rhs_med = median_ms(rhs_lats).unwrap_or(0.0);
                let lhs_recv = recv_pct(lhs_lats, msg_count);
                let rhs_recv = recv_pct(rhs_lats, msg_count);

                if lhs_recv >= 95.0 {
                    lhs_reliable += 1;
                }
                if rhs_recv >= 95.0 {
                    rhs_reliable += 1;
                }

                println!(
                    "  run{} order={} lhs_med={:.1}ms rhs_med={:.1}ms lhs_recv={:.0}% rhs_recv={:.0}%",
                    run + 1,
                    if flipped { "rhs->lhs" } else { "lhs->rhs" },
                    lhs_med,
                    rhs_med,
                    lhs_recv,
                    rhs_recv
                );

                lhs_medians.push(lhs_med);
                rhs_medians.push(rhs_med);
                deltas_ms.push(lhs_med - rhs_med);
                comparisons += 1;
                if rhs_med < lhs_med {
                    rhs_wins += 1;
                }
            }
            (lhs_err, rhs_err) => {
                println!(
                    "  run{} order={} lhs={} rhs={}",
                    run + 1,
                    if flipped { "rhs->lhs" } else { "lhs->rhs" },
                    lhs_err
                        .as_ref()
                        .err()
                        .map(|e| format!("FAIL({e})"))
                        .unwrap_or_else(|| "ok".to_string()),
                    rhs_err
                        .as_ref()
                        .err()
                        .map(|e| format!("FAIL({e})"))
                        .unwrap_or_else(|| "ok".to_string())
                );
            }
        }
        std::thread::sleep(Duration::from_millis(400));
    }

    if comparisons == 0 {
        println!("  summary: no successful paired samples");
        println!(
            "PAIR|{}|{}|{}|{}|{}|0|FAIL|FAIL|FAIL|FAIL|FAIL|0|0|0",
            label,
            lhs.kind(),
            lhs.name(),
            rhs.kind(),
            rhs.name()
        );
        println!();
        return;
    }

    let lhs_mean_med = mean_f64(&lhs_medians);
    let rhs_mean_med = mean_f64(&rhs_medians);
    let delta_mean = mean_f64(&deltas_ms);
    let delta_sd = stddev_f64(&deltas_ms);
    let delta_sem = if comparisons > 0 {
        delta_sd / (comparisons as f64).sqrt()
    } else {
        0.0
    };
    let ci_margin = 1.96 * delta_sem;
    let ci_low = delta_mean - ci_margin;
    let ci_high = delta_mean + ci_margin;
    let rhs_win_pct = (rhs_wins as f64 / comparisons as f64) * 100.0;
    let lhs_rel_pct = (lhs_reliable as f64 / pair_runs as f64) * 100.0;
    let rhs_rel_pct = (rhs_reliable as f64 / pair_runs as f64) * 100.0;

    println!(
        "  summary: lhs_mean_med={lhs_mean_med:.1}ms rhs_mean_med={rhs_mean_med:.1}ms delta(lhs-rhs)={delta_mean:.1}ms 95%CI=[{ci_low:.1},{ci_high:.1}] rhs_win={rhs_win_pct:.0}% lhs_rel={lhs_rel_pct:.0}% rhs_rel={rhs_rel_pct:.0}%"
    );
    println!(
        "PAIR|{}|{}|{}|{}|{}|{}|{:.1}|{:.1}|{:.1}|{:.1}|{:.1}|{:.0}|{:.0}|{:.0}",
        label,
        lhs.kind(),
        lhs.name(),
        rhs.kind(),
        rhs.name(),
        comparisons,
        lhs_mean_med,
        rhs_mean_med,
        delta_mean,
        ci_low,
        ci_high,
        rhs_win_pct,
        lhs_rel_pct,
        rhs_rel_pct
    );
    println!();
}

#[test]
#[ignore]
fn reliable_moq_latency_comparison() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let msg_count = env_usize("PIKA_BENCH_MSG_COUNT", DEFAULT_MSG_COUNT);
    let runs = env_usize("PIKA_BENCH_RUNS", DEFAULT_RUNS);
    let pair_runs = env_usize("PIKA_BENCH_PAIR_RUNS", DEFAULT_PAIR_RUNS);
    let skip_matrix = env_flag("PIKA_BENCH_SKIP_MATRIX");
    let skip_pairs = env_flag("PIKA_BENCH_SKIP_PAIRS");

    println!();
    println!("========================================================");
    println!("  Reliable MoQ Prototype vs Nostr Relay Latency");
    println!(
        "  msgs/run: {msg_count}  |  runs: {runs}  |  pair-runs: {pair_runs}  |  payload: {PAYLOAD_SIZE}B"
    );
    println!("========================================================");
    println!();

    let mut nostr_results: Vec<(&str, Vec<Duration>)> = Vec::new();
    let mut mcr_results: Vec<(&str, Vec<Duration>)> = Vec::new();

    if !skip_matrix {
        println!("--- Nostr Relays (WebSocket) ---");
        println!();
        for (name, url) in NOSTR_RELAYS {
            print!("{name:<24}");
            let mut all_latencies = Vec::new();
            for run in 0..runs {
                match measure_nostr_relay(url, msg_count) {
                    Ok(lats) => {
                        let n = lats.len();
                        all_latencies.extend(lats);
                        print!("  run{}: {n}/{msg_count}", run + 1);
                    }
                    Err(e) => print!("  run{}: FAIL({e})", run + 1),
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            println!();

            if !all_latencies.is_empty() {
                println!(
                    "  {:<22} median={}  mean={}  p95={}  min={}  max={}  n={}",
                    "",
                    fmt_ms(median(&mut all_latencies.clone())),
                    fmt_ms(mean(&all_latencies)),
                    fmt_ms(p95(&mut all_latencies.clone())),
                    fmt_ms(*all_latencies.iter().min().unwrap()),
                    fmt_ms(*all_latencies.iter().max().unwrap()),
                    all_latencies.len(),
                );
            }
            nostr_results.push((name, all_latencies));
            println!();
        }

        println!("--- Reliable MoQ Prototype (HTTP persist + MoQ live) ---");
        println!();
        for (name, url) in MOQ_RELAYS {
            print!("{name:<24}");
            let mut all_latencies = Vec::new();
            for run in 0..runs {
                match measure_reliable_moq(url, msg_count) {
                    Ok(lats) => {
                        let n = lats.len();
                        all_latencies.extend(lats);
                        print!("  run{}: {n}/{msg_count}", run + 1);
                    }
                    Err(e) => print!("  run{}: FAIL({e})", run + 1),
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            println!();

            if !all_latencies.is_empty() {
                println!(
                    "  {:<22} median={}  mean={}  p95={}  min={}  max={}  n={}",
                    "",
                    fmt_ms(median(&mut all_latencies.clone())),
                    fmt_ms(mean(&all_latencies)),
                    fmt_ms(p95(&mut all_latencies.clone())),
                    fmt_ms(*all_latencies.iter().min().unwrap()),
                    fmt_ms(*all_latencies.iter().max().unwrap()),
                    all_latencies.len(),
                );
            }
            mcr_results.push((name, all_latencies));
            println!();
        }
        println!("========================================================");
        println!("  Summary");
        println!("========================================================");
        println!(
            "{:<44} {:>10} {:>10} {:>10} {:>10} {:>6}",
            "Relay", "Median", "Mean", "P95", "Min", "Recv%"
        );
        println!("{}", "-".repeat(96));

        for (name, lats) in nostr_results {
            if lats.is_empty() {
                println!("{:<44} {:>10}", format!("[nostr] {name}"), "FAIL");
                println!("RESULT|nostr|{name}|FAIL|FAIL|FAIL|FAIL|0");
                continue;
            }
            let total = runs * msg_count;
            let pct = (lats.len() as f64 / total as f64) * 100.0;
            let med = median(&mut lats.clone());
            let avg = mean(&lats);
            let p = p95(&mut lats.clone());
            let min = *lats.iter().min().unwrap();
            println!(
                "{:<44} {:>10} {:>10} {:>10} {:>10} {:>5.0}%",
                format!("[nostr] {name}"),
                fmt_ms(med),
                fmt_ms(avg),
                fmt_ms(p),
                fmt_ms(min),
                pct,
            );
            println!(
                "RESULT|nostr|{name}|{:.1}|{:.1}|{:.1}|{:.1}|{:.0}",
                ms(med),
                ms(avg),
                ms(p),
                ms(min),
                pct
            );
        }
        for (name, lats) in mcr_results {
            if lats.is_empty() {
                println!("{:<44} {:>10}", format!("[mcr+moq] {name}"), "FAIL");
                println!("RESULT|mcr+moq|{name}|FAIL|FAIL|FAIL|FAIL|0");
                continue;
            }
            let total = runs * msg_count;
            let pct = (lats.len() as f64 / total as f64) * 100.0;
            let med = median(&mut lats.clone());
            let avg = mean(&lats);
            let p = p95(&mut lats.clone());
            let min = *lats.iter().min().unwrap();
            println!(
                "{:<44} {:>10} {:>10} {:>10} {:>10} {:>5.0}%",
                format!("[mcr+moq] {name}"),
                fmt_ms(med),
                fmt_ms(avg),
                fmt_ms(p),
                fmt_ms(min),
                pct,
            );
            println!(
                "RESULT|mcr+moq|{name}|{:.1}|{:.1}|{:.1}|{:.1}|{:.0}",
                ms(med),
                ms(avg),
                ms(p),
                ms(min),
                pct
            );
        }
    } else {
        println!("--- Matrix benchmark skipped (PIKA_BENCH_SKIP_MATRIX=1) ---");
        println!();
    }

    if !skip_pairs && pair_runs > 0 {
        println!("========================================================");
        println!("  Paired A/B");
        println!("========================================================");
        println!();
        let pairs: [(&str, BenchPath, BenchPath); 3] = [
            (
                "pikachat us-east: nostr vs mcr+moq",
                BenchPath::Nostr {
                    name: "us-east.nostr.pikachat.org",
                    url: "wss://us-east.nostr.pikachat.org",
                },
                BenchPath::ReliableMoq {
                    name: "us-east.moq.pikachat.org",
                    url: "https://us-east.moq.pikachat.org/anon",
                },
            ),
            (
                "pikachat eu: nostr vs mcr+moq",
                BenchPath::Nostr {
                    name: "eu.nostr.pikachat.org",
                    url: "wss://eu.nostr.pikachat.org",
                },
                BenchPath::ReliableMoq {
                    name: "eu.moq.pikachat.org",
                    url: "https://eu.moq.pikachat.org/anon",
                },
            ),
            (
                "moq us-east: logos vs pikachat",
                BenchPath::ReliableMoq {
                    name: "us-east.moq.logos.surf",
                    url: "https://us-east.moq.logos.surf/anon",
                },
                BenchPath::ReliableMoq {
                    name: "us-east.moq.pikachat.org",
                    url: "https://us-east.moq.pikachat.org/anon",
                },
            ),
        ];
        for (label, lhs, rhs) in pairs {
            run_pair_ab(label, lhs, rhs, pair_runs, msg_count);
        }
    } else {
        println!("--- Paired A/B skipped (PIKA_BENCH_SKIP_PAIRS=1 or pair-runs=0) ---");
        println!();
    }

    println!();
}
