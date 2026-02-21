use std::env;
use std::time::Duration;

use anyhow::Context;
use nostr_sdk::prelude::*;

#[derive(Debug, Clone)]
struct Opts {
    relay: String,
    kind: u16,
    client_nsecs: Vec<String>,
}

fn usage() {
    eprintln!(
        "usage: cargo run -p pika_core --bin nostr_connect_tap -- \
  --relay <ws://127.0.0.1:7777> \
  [--kind 24133] \
  [--client-nsec <nsec1> --client-nsec <nsec2> ...]"
    );
}

fn parse_opts() -> anyhow::Result<Opts> {
    let mut relay: Option<String> = None;
    let mut kind: u16 = 24133;
    let mut client_nsecs: Vec<String> = Vec::new();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--relay" => {
                let v = args
                    .next()
                    .context("missing value after --relay")?
                    .trim()
                    .to_string();
                if v.is_empty() {
                    anyhow::bail!("--relay cannot be empty");
                }
                relay = Some(v);
            }
            "--kind" => {
                let raw = args.next().context("missing value after --kind")?;
                kind = raw
                    .parse::<u16>()
                    .with_context(|| format!("invalid --kind value: {raw}"))?;
            }
            "--client-nsec" => {
                let nsec = args
                    .next()
                    .context("missing value after --client-nsec")?
                    .trim()
                    .to_string();
                if !nsec.is_empty() {
                    client_nsecs.push(nsec);
                }
            }
            "-h" | "--help" => {
                usage();
                std::process::exit(0);
            }
            other => {
                anyhow::bail!("unknown arg: {other}");
            }
        }
    }

    let relay = relay.context("missing --relay")?;
    Ok(Opts {
        relay,
        kind,
        client_nsecs,
    })
}

fn decode_with_client_keys(event: &Event, client_keys: &Keys) -> Option<(&'static str, String)> {
    if let Ok(plaintext) = nip44::decrypt(
        client_keys.secret_key(),
        &event.pubkey,
        event.content.as_str(),
    ) {
        return Some(("nip44", plaintext));
    }

    if let Ok(plaintext) = nip04::decrypt(
        client_keys.secret_key(),
        &event.pubkey,
        event.content.as_str(),
    ) {
        return Some(("nip04", plaintext));
    }

    None
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = parse_opts().inspect_err(|e| {
        eprintln!("error: {e:#}");
        usage();
    })?;

    let relay = RelayUrl::parse(&opts.relay)
        .with_context(|| format!("invalid relay url: {}", opts.relay))?;
    let observer_keys = Keys::generate();
    let client = Client::new(observer_keys);
    client.add_relay(relay.clone()).await?;
    client.connect().await;

    let mut decode_keys: Vec<Keys> = Vec::new();
    for nsec in &opts.client_nsecs {
        match Keys::parse(nsec) {
            Ok(keys) => decode_keys.push(keys),
            Err(e) => eprintln!("warn: invalid --client-nsec ignored: {e}"),
        }
    }

    let filter = Filter::new()
        .kind(Kind::from(opts.kind))
        .since(Timestamp::now());
    client.subscribe(filter, None).await?;

    eprintln!(
        "nostr_connect_tap: relay={} kind={} decode_keys={}",
        relay.as_str(),
        opts.kind,
        decode_keys.len()
    );
    eprintln!("nostr_connect_tap: waiting for events...");

    let mut notifications = client.notifications();
    loop {
        let notif = match tokio::time::timeout(Duration::from_secs(60), notifications.recv()).await
        {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                eprintln!("nostr_connect_tap: notifications closed: {e}");
                break;
            }
            Err(_) => continue,
        };

        let RelayPoolNotification::Event {
            event, relay_url, ..
        } = notif
        else {
            continue;
        };

        if event.kind != Kind::from(opts.kind) {
            continue;
        }

        println!(
            "[event] relay={} id={} pubkey={} created_at={} kind={} tags={} content_len={}",
            relay_url,
            event.id,
            event.pubkey,
            event.created_at.as_secs(),
            event.kind.as_u16(),
            event.tags.len(),
            event.content.len()
        );

        for (idx, key) in decode_keys.iter().enumerate() {
            if let Some((transport, plaintext)) = decode_with_client_keys(&event, key) {
                println!(
                    "[decode] key_index={} transport={} plaintext={}",
                    idx, transport, plaintext
                );
                if let Ok(msg) = NostrConnectMessage::from_json(plaintext.clone()) {
                    println!(
                        "[decode] key_index={} parsed_message={}",
                        idx,
                        msg.as_json()
                    );
                }
            }
        }
    }

    client.shutdown().await;
    Ok(())
}
