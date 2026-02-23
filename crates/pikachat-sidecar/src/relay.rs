use std::time::Duration;

use anyhow::{Context, anyhow};
use nostr_sdk::prelude::*;
use tokio::time::Instant;

pub async fn connect_client(keys: &Keys, relay: &str) -> anyhow::Result<Client> {
    let client = Client::new(keys.clone());
    client.add_relay(relay).await.context("add relay")?;
    client.connect().await;
    Ok(client)
}

pub async fn subscribe_group_msgs(
    client: &Client,
    nostr_group_id_hex: &str,
) -> anyhow::Result<SubscriptionId> {
    let mut filter = Filter::new()
        .kind(Kind::MlsGroupMessage)
        .custom_tag(SingleLetterTag::lowercase(Alphabet::H), nostr_group_id_hex)
        .limit(200);

    // Be generous about time skews; exact-token matching will disambiguate.
    filter = filter.since(Timestamp::now() - Duration::from_secs(60 * 60));

    let out = client.subscribe(filter, None).await?;
    Ok(out.val)
}

pub async fn check_relay_ready(relay_url: &str, timeout: Duration) -> anyhow::Result<()> {
    let relay_url = RelayUrl::parse(relay_url).context("parse relay url")?;
    let deadline = Instant::now() + timeout;
    let mut attempt: usize = 0;
    let mut last_detail = String::new();

    loop {
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "timeout waiting for relay websocket to become connected (attempts={attempt}, last={last_detail})"
            ));
        }

        attempt += 1;

        // Build a fresh nostr-sdk client each attempt. In CI we can hit a transient startup race
        // where the relay process is up but not yet ready for websocket handshakes; a single early
        // connect attempt may never transition to connected within the overall timeout.
        let client = Client::new(Keys::generate());
        match client.add_relay(relay_url.clone()).await {
            Ok(_) => {}
            Err(err) => {
                last_detail = format!("add_relay: {err}");
                tokio::time::sleep(Duration::from_millis(250)).await;
                continue;
            }
        }

        client.connect().await;
        let connect_deadline = Instant::now() + Duration::from_secs(3);
        let mut connected = false;
        while Instant::now() < connect_deadline {
            if let Ok(relay) = client.relay(relay_url.clone()).await
                && relay.is_connected()
            {
                connected = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if connected {
            client.shutdown().await;
            return Ok(());
        }

        last_detail = "not connected yet".to_string();
        client.shutdown().await;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
