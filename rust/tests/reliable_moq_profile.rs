//! Reliable MoQ Chat Profile (MCR-00) prototype tests.
//!
//! This suite validates deterministic behavior for:
//! - sequence ordering,
//! - gap repair,
//! - dedupe/idempotency,
//! - reconnect catch-up,
//! - deterministic error/result codes.

#[path = "support/mod.rs"]
mod support;

use std::time::Duration;

use support::reliable_moq::{spawn_mcr_http_server, McrClient, McrRelayOptions};

#[tokio::test(flavor = "multi_thread")]
async fn reconnect_catchup_recovers_all_messages_after_disconnect_burst() {
    let server = spawn_mcr_http_server(McrRelayOptions::default())
        .await
        .expect("spawn mcr http server");

    let room = "room-reconnect";
    let alice = McrClient::new(server.base_url.clone(), room, "alice");
    let mut bob = McrClient::new(server.base_url.clone(), room, "bob");

    bob.initial_attach().await.expect("initial attach");

    // Simulate disconnect: Bob does not process live traffic while Alice sends a burst.
    for i in 0..1_000u64 {
        let receipt = alice
            .publish_with_msg_id(
                format!("burst-{i}"),
                "marmot_app_event",
                serde_json::json!({"kind":"chat","body":format!("msg-{i}")}),
            )
            .await
            .expect("publish burst message");
        assert_eq!(receipt.status, "PERSISTED");
        assert_eq!(receipt.code.as_deref(), Some("SUCCESS"));
    }

    // Reconnect flow: initial attach must catch up deterministically.
    let applied = bob.initial_attach().await.expect("catch-up attach");
    assert_eq!(applied, 1_000);
    assert_eq!(bob.last_seq, 1_000);
    assert_eq!(bob.applied().len(), 1_000);

    // Ensure contiguous sequence with no holes.
    for (idx, env) in bob.applied().iter().enumerate() {
        assert_eq!(env.seq as usize, idx + 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_delivery_is_not_applied_twice() {
    let server = spawn_mcr_http_server(McrRelayOptions::default())
        .await
        .expect("spawn mcr http server");

    let room = "room-dedupe";
    let alice = McrClient::new(server.base_url.clone(), room, "alice");
    let mut bob = McrClient::new(server.base_url.clone(), room, "bob");

    let fixed_msg_id = "same-msg-id";
    let first = alice
        .publish_with_msg_id(
            fixed_msg_id,
            "marmot_app_event",
            serde_json::json!({"kind":"typing","typing":true}),
        )
        .await
        .expect("first publish");
    assert_eq!(first.code.as_deref(), Some("SUCCESS"));

    let duplicate = alice
        .publish_with_msg_id(
            fixed_msg_id,
            "marmot_app_event",
            serde_json::json!({"kind":"typing","typing":true}),
        )
        .await
        .expect("duplicate publish");
    assert_eq!(duplicate.code.as_deref(), Some("DUPLICATE"));
    assert_eq!(duplicate.seq, first.seq);

    bob.initial_attach().await.expect("bob initial attach");
    assert_eq!(bob.applied().len(), 1);

    // Re-deliver the same persisted envelope multiple times through live path.
    let page = bob.range(1, 10).await.expect("fetch page");
    let env = page.items.first().cloned().expect("one item");
    bob.handle_live(env.clone())
        .await
        .expect("live duplicate 1");
    bob.handle_live(env).await.expect("live duplicate 2");

    assert_eq!(bob.applied().len(), 1);
    assert_eq!(bob.last_seq, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn out_of_order_live_delivery_triggers_gap_repair_before_apply() {
    let server = spawn_mcr_http_server(McrRelayOptions::default())
        .await
        .expect("spawn mcr http server");

    let room = "room-gap";
    let alice = McrClient::new(server.base_url.clone(), room, "alice");
    let mut bob = McrClient::new(server.base_url.clone(), room, "bob");

    for i in 0..20u64 {
        let receipt = alice
            .publish_with_msg_id(
                format!("gap-{i}"),
                "marmot_app_event",
                serde_json::json!({"kind":"chat","body":format!("gap-{i}")}),
            )
            .await
            .expect("publish");
        assert_eq!(receipt.code.as_deref(), Some("SUCCESS"));
    }

    let page = bob.range(1, 32).await.expect("range");
    assert_eq!(page.items.len(), 20);

    // Deliver only seq=20 first; client must run catch-up and apply 1..20 in order.
    let tail = page.items.last().cloned().expect("tail");
    let applied = bob.handle_live(tail).await.expect("handle out-of-order");
    assert_eq!(applied, 20);
    assert_eq!(bob.last_seq, 20);
    assert_eq!(bob.applied().len(), 20);

    for (idx, env) in bob.applied().iter().enumerate() {
        assert_eq!(env.seq as usize, idx + 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn relay_returns_deterministic_error_codes() {
    let opts = McrRelayOptions {
        max_payload_bytes: 32,
        max_publishes_per_sec: 1,
        required_bearer_token: Some("token-123".to_string()),
        moq_mirror: None,
    };
    let server = spawn_mcr_http_server(opts)
        .await
        .expect("spawn mcr http server");

    let room = "room-errors";
    let no_auth = McrClient::new(server.base_url.clone(), room, "alice");
    let with_auth = McrClient::new(server.base_url.clone(), room, "alice").with_token("token-123");

    let unauth = no_auth
        .publish_json("marmot_app_event", serde_json::json!({"x":1}))
        .await
        .expect("unauth publish response");
    assert_eq!(unauth.status, "REJECTED");
    assert_eq!(unauth.code.as_deref(), Some("REQUIRES_AUTHENTICATION"));

    let too_large = with_auth
        .publish_json(
            "marmot_app_event",
            serde_json::json!({"body":"this payload is intentionally larger than 32 bytes"}),
        )
        .await
        .expect("too large response");
    assert_eq!(too_large.status, "REJECTED");
    assert_eq!(too_large.code.as_deref(), Some("TOO_LARGE"));

    let ok = with_auth
        .publish_json("marmot_app_event", serde_json::json!({"b":"ok"}))
        .await
        .expect("ok publish");
    assert_eq!(ok.status, "PERSISTED");
    assert_eq!(ok.code.as_deref(), Some("SUCCESS"));

    let too_fast = with_auth
        .publish_json("marmot_app_event", serde_json::json!({"b":"again"}))
        .await
        .expect("too fast response");
    assert_eq!(too_fast.status, "REJECTED");
    assert_eq!(too_fast.code.as_deref(), Some("TOO_FAST"));
    assert_eq!(too_fast.retry_after_ms, Some(1_000));

    tokio::time::sleep(Duration::from_millis(1100)).await;
    let ok_after_wait = with_auth
        .publish_json("marmot_app_event", serde_json::json!({"b":"later"}))
        .await
        .expect("ok after wait");
    assert_eq!(ok_after_wait.code.as_deref(), Some("SUCCESS"));
}
