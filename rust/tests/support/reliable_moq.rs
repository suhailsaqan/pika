use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use pika_media::network::NetworkRelay;
use pika_media::session::MediaFrame;
use pika_media::tracks::TrackAddress;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, oneshot};

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1000;
const DEFAULT_MAX_PAYLOAD_BYTES: usize = 64 * 1024;
const DEFAULT_RATE_LIMIT_PER_SEC: usize = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McrEnvelope {
    pub v: u8,
    pub room_id: String,
    pub seq: u64,
    pub msg_id: String,
    pub sender_id: String,
    pub sent_at_ms: u64,
    pub payload_type: String,
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishRequest {
    pub v: u8,
    pub room_id: String,
    pub msg_id: String,
    pub sender_id: String,
    pub sent_at_ms: u64,
    pub payload_type: String,
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishReceipt {
    pub v: u8,
    pub status: String,
    pub room_id: String,
    pub msg_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persisted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadResponse {
    pub room_id: String,
    pub head_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeResponse {
    pub room_id: String,
    pub from_seq: u64,
    pub to_seq: u64,
    pub has_more: bool,
    pub items: Vec<McrEnvelope>,
}

#[derive(Debug, Clone)]
pub struct MoqMirrorConfig {
    pub moq_url: String,
    pub broadcast_base: String,
}

#[derive(Debug, Clone)]
pub struct McrRelayOptions {
    pub max_payload_bytes: usize,
    pub max_publishes_per_sec: usize,
    pub required_bearer_token: Option<String>,
    pub moq_mirror: Option<MoqMirrorConfig>,
}

impl Default for McrRelayOptions {
    fn default() -> Self {
        Self {
            max_payload_bytes: DEFAULT_MAX_PAYLOAD_BYTES,
            max_publishes_per_sec: DEFAULT_RATE_LIMIT_PER_SEC,
            required_bearer_token: None,
            moq_mirror: None,
        }
    }
}

#[derive(Clone)]
struct MoqMirror {
    relay: NetworkRelay,
    broadcast_base: String,
}

impl MoqMirror {
    fn track_address(&self, room_id: &str) -> TrackAddress {
        TrackAddress {
            broadcast_path: format!("{}/{}", self.broadcast_base, room_id),
            track_name: "chat.live".to_string(),
        }
    }

    fn publish_envelope(&self, env: &McrEnvelope) {
        if let Ok(payload) = serde_json::to_vec(env) {
            let frame = MediaFrame {
                seq: env.seq,
                timestamp_us: now_ms().saturating_mul(1000),
                keyframe: true,
                payload,
            };
            let _ = self.relay.publish(&self.track_address(&env.room_id), frame);
        }
    }

    fn warmup(&self, room_id: &str) {
        let env = McrEnvelope {
            v: 1,
            room_id: room_id.to_string(),
            seq: 0,
            msg_id: format!("warmup-{}", rand::random::<u64>()),
            sender_id: "relay".to_string(),
            sent_at_ms: now_ms(),
            payload_type: "warmup".to_string(),
            payload: serde_json::json!({"warmup":true}),
            meta: None,
        };
        self.publish_envelope(&env);
    }
}

#[derive(Clone)]
struct RoomState {
    items: Vec<McrEnvelope>,
    by_msg_id: HashMap<String, u64>,
    live_tx: broadcast::Sender<McrEnvelope>,
}

impl RoomState {
    fn new() -> Self {
        let (live_tx, _) = broadcast::channel(8192);
        Self {
            items: Vec::new(),
            by_msg_id: HashMap::new(),
            live_tx,
        }
    }

    fn head_seq(&self) -> u64 {
        self.items.last().map(|e| e.seq).unwrap_or(0)
    }
}

struct RelayState {
    rooms: HashMap<String, RoomState>,
    publish_times_by_sender: HashMap<String, VecDeque<Instant>>,
    max_payload_bytes: usize,
    max_publishes_per_sec: usize,
    required_bearer_token: Option<String>,
    moq_mirror: Option<MoqMirror>,
}

#[derive(Clone)]
pub struct McrRelayHandle {
    state: Arc<Mutex<RelayState>>,
}

impl McrRelayHandle {
    pub fn new(opts: McrRelayOptions) -> Result<Self, String> {
        let moq_mirror = if let Some(cfg) = opts.moq_mirror {
            let relay = NetworkRelay::new(&cfg.moq_url)
                .map_err(|e| format!("mirror relay init failed: {e}"))?;
            relay
                .connect()
                .map_err(|e| format!("mirror relay connect failed: {e}"))?;
            Some(MoqMirror {
                relay,
                broadcast_base: cfg.broadcast_base,
            })
        } else {
            None
        };

        Ok(Self {
            state: Arc::new(Mutex::new(RelayState {
                rooms: HashMap::new(),
                publish_times_by_sender: HashMap::new(),
                max_payload_bytes: opts.max_payload_bytes,
                max_publishes_per_sec: opts.max_publishes_per_sec,
                required_bearer_token: opts.required_bearer_token,
                moq_mirror,
            })),
        })
    }

    pub fn track_for_room(&self, room_id: &str) -> Option<TrackAddress> {
        let st = self.state.lock().expect("relay state poisoned");
        st.moq_mirror.as_ref().map(|m| m.track_address(room_id))
    }

    pub fn warmup_live_track(&self, room_id: &str) {
        let mirror = {
            let st = self.state.lock().expect("relay state poisoned");
            st.moq_mirror.clone()
        };
        if let Some(mirror) = mirror {
            mirror.warmup(room_id);
        }
    }

    pub fn subscribe_live(&self, room_id: &str) -> broadcast::Receiver<McrEnvelope> {
        let mut st = self.state.lock().expect("relay state poisoned");
        let room = st
            .rooms
            .entry(room_id.to_string())
            .or_insert_with(RoomState::new);
        room.live_tx.subscribe()
    }

    pub fn head(&self, room_id: &str) -> HeadResponse {
        let mut st = self.state.lock().expect("relay state poisoned");
        let room = st
            .rooms
            .entry(room_id.to_string())
            .or_insert_with(RoomState::new);
        HeadResponse {
            room_id: room_id.to_string(),
            head_seq: room.head_seq(),
        }
    }

    pub fn range(&self, room_id: &str, from_seq: u64, limit: usize) -> RangeResponse {
        let mut st = self.state.lock().expect("relay state poisoned");
        let room = st
            .rooms
            .entry(room_id.to_string())
            .or_insert_with(RoomState::new);

        let capped_limit = limit.clamp(1, MAX_LIMIT);
        let head_seq = room.head_seq();

        let mut items = Vec::new();
        for env in room
            .items
            .iter()
            .filter(|e| e.seq >= from_seq)
            .take(capped_limit)
        {
            items.push(env.clone());
        }

        let to_seq = items
            .last()
            .map(|e| e.seq)
            .unwrap_or(from_seq.saturating_sub(1));
        let has_more = !items.is_empty() && to_seq < head_seq;

        RangeResponse {
            room_id: room_id.to_string(),
            from_seq,
            to_seq,
            has_more,
            items,
        }
    }

    fn parse_bearer(headers: &HeaderMap) -> Option<String> {
        let value = headers.get(axum::http::header::AUTHORIZATION)?;
        let value = value.to_str().ok()?.trim();
        value.strip_prefix("Bearer ").map(|s| s.trim().to_string())
    }

    pub fn publish(
        &self,
        room_id: &str,
        req: PublishRequest,
        auth_header: Option<String>,
    ) -> (StatusCode, PublishReceipt) {
        let to_fanout: Option<(
            broadcast::Sender<McrEnvelope>,
            McrEnvelope,
            Option<MoqMirror>,
        )>;

        let out = {
            let mut st = self.state.lock().expect("relay state poisoned");

            if let Some(expected) = st.required_bearer_token.as_ref() {
                let ok = auth_header.as_deref() == Some(expected.as_str());
                if !ok {
                    return (
                        StatusCode::UNAUTHORIZED,
                        PublishReceipt {
                            v: 1,
                            status: "REJECTED".to_string(),
                            room_id: room_id.to_string(),
                            msg_id: req.msg_id,
                            seq: None,
                            code: Some("REQUIRES_AUTHENTICATION".to_string()),
                            reason: Some("missing or invalid bearer token".to_string()),
                            persisted_at_ms: None,
                            retry_after_ms: None,
                        },
                    );
                }
            }

            if req.room_id != room_id {
                return (
                    StatusCode::BAD_REQUEST,
                    PublishReceipt {
                        v: 1,
                        status: "REJECTED".to_string(),
                        room_id: room_id.to_string(),
                        msg_id: req.msg_id,
                        seq: None,
                        code: Some("INVALID".to_string()),
                        reason: Some("room_id mismatch".to_string()),
                        persisted_at_ms: None,
                        retry_after_ms: None,
                    },
                );
            }

            let payload_size = serde_json::to_vec(&req.payload)
                .map(|b| b.len())
                .unwrap_or(usize::MAX);
            if payload_size > st.max_payload_bytes {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    PublishReceipt {
                        v: 1,
                        status: "REJECTED".to_string(),
                        room_id: room_id.to_string(),
                        msg_id: req.msg_id,
                        seq: None,
                        code: Some("TOO_LARGE".to_string()),
                        reason: Some("payload too large".to_string()),
                        persisted_at_ms: None,
                        retry_after_ms: None,
                    },
                );
            }

            let now = Instant::now();
            let sender_id = req.sender_id.clone();
            let max_publishes_per_sec = st.max_publishes_per_sec;
            {
                let sender_times = st
                    .publish_times_by_sender
                    .entry(sender_id.clone())
                    .or_default();
                while let Some(front) = sender_times.front() {
                    if now.duration_since(*front) > Duration::from_secs(1) {
                        sender_times.pop_front();
                    } else {
                        break;
                    }
                }
                if sender_times.len() >= max_publishes_per_sec {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        PublishReceipt {
                            v: 1,
                            status: "REJECTED".to_string(),
                            room_id: room_id.to_string(),
                            msg_id: req.msg_id,
                            seq: None,
                            code: Some("TOO_FAST".to_string()),
                            reason: Some("rate limit exceeded".to_string()),
                            persisted_at_ms: None,
                            retry_after_ms: Some(1_000),
                        },
                    );
                }
            }

            let mirror = st.moq_mirror.clone();
            let (live_tx, env) = {
                let room = st
                    .rooms
                    .entry(room_id.to_string())
                    .or_insert_with(RoomState::new);

                if let Some(existing_seq) = room.by_msg_id.get(&req.msg_id).copied() {
                    return (
                        StatusCode::OK,
                        PublishReceipt {
                            v: 1,
                            status: "PERSISTED".to_string(),
                            room_id: room_id.to_string(),
                            msg_id: req.msg_id,
                            seq: Some(existing_seq),
                            code: Some("DUPLICATE".to_string()),
                            reason: Some("already persisted".to_string()),
                            persisted_at_ms: Some(now_ms()),
                            retry_after_ms: None,
                        },
                    );
                }

                let seq = room.head_seq() + 1;
                let env = McrEnvelope {
                    v: req.v,
                    room_id: room_id.to_string(),
                    seq,
                    msg_id: req.msg_id,
                    sender_id,
                    sent_at_ms: req.sent_at_ms,
                    payload_type: req.payload_type,
                    payload: req.payload,
                    meta: req.meta,
                };
                room.by_msg_id.insert(env.msg_id.clone(), seq);
                room.items.push(env.clone());
                (room.live_tx.clone(), env)
            };

            st.publish_times_by_sender
                .entry(env.sender_id.clone())
                .or_default()
                .push_back(now);

            to_fanout = Some((live_tx, env.clone(), mirror));

            (
                StatusCode::OK,
                PublishReceipt {
                    v: 1,
                    status: "PERSISTED".to_string(),
                    room_id: room_id.to_string(),
                    msg_id: env.msg_id.clone(),
                    seq: Some(env.seq),
                    code: Some("SUCCESS".to_string()),
                    reason: None,
                    persisted_at_ms: Some(now_ms()),
                    retry_after_ms: None,
                },
            )
        };

        if let Some((live_tx, env, mirror)) = to_fanout {
            let _ = live_tx.send(env.clone());
            if let Some(mirror) = mirror {
                mirror.publish_envelope(&env);
            }
        }

        out
    }
}

pub struct McrHttpServer {
    pub base_url: String,
    pub relay: McrRelayHandle,
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for McrHttpServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

#[derive(Debug, Deserialize)]
struct RangeQuery {
    from_seq: Option<u64>,
    limit: Option<usize>,
}

async fn head_handler(
    State(relay): State<McrRelayHandle>,
    Path(room_id): Path<String>,
) -> Json<HeadResponse> {
    Json(relay.head(&room_id))
}

async fn range_handler(
    State(relay): State<McrRelayHandle>,
    Path(room_id): Path<String>,
    Query(query): Query<RangeQuery>,
) -> Json<RangeResponse> {
    let from_seq = query.from_seq.unwrap_or(1);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    Json(relay.range(&room_id, from_seq, limit))
}

async fn publish_handler(
    State(relay): State<McrRelayHandle>,
    Path(room_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<PublishRequest>,
) -> (StatusCode, Json<PublishReceipt>) {
    let auth = McrRelayHandle::parse_bearer(&headers);
    let (status, receipt) = relay.publish(&room_id, req, auth);
    (status, Json(receipt))
}

pub async fn spawn_mcr_http_server(opts: McrRelayOptions) -> Result<McrHttpServer, String> {
    let relay = McrRelayHandle::new(opts)?;
    let app = Router::new()
        .route("/mcr/v1/rooms/:room_id/head", get(head_handler))
        .route("/mcr/v1/rooms/:room_id/messages", get(range_handler))
        .route("/mcr/v1/rooms/:room_id/publish", post(publish_handler))
        .with_state(relay.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind: {e}"))?;
    let addr: SocketAddr = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?;
    let base_url = format!("http://{addr}");

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        let _ = server.await;
    });

    Ok(McrHttpServer {
        base_url,
        relay,
        shutdown: Some(shutdown_tx),
        join: Some(join),
    })
}

#[derive(Debug)]
pub struct McrClient {
    http: reqwest::Client,
    pub base_url: String,
    pub room_id: String,
    pub sender_id: String,
    pub bearer_token: Option<String>,
    pub last_seq: u64,
    seen_msg_ids: HashSet<String>,
    seen_order: VecDeque<String>,
    seen_cap: usize,
    holdback: BTreeMap<u64, McrEnvelope>,
    applied: Vec<McrEnvelope>,
}

impl McrClient {
    pub fn new(
        base_url: impl Into<String>,
        room_id: impl Into<String>,
        sender_id: impl Into<String>,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            room_id: room_id.into(),
            sender_id: sender_id.into(),
            bearer_token: None,
            last_seq: 0,
            seen_msg_ids: HashSet::new(),
            seen_order: VecDeque::new(),
            seen_cap: 8_192,
            holdback: BTreeMap::new(),
            applied: Vec::new(),
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    pub fn applied(&self) -> &[McrEnvelope] {
        &self.applied
    }

    pub async fn head(&self) -> Result<HeadResponse, String> {
        let url = format!("{}/mcr/v1/rooms/{}/head", self.base_url, self.room_id);
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("head request failed: {e}"))?;
        resp.json::<HeadResponse>()
            .await
            .map_err(|e| format!("head decode failed: {e}"))
    }

    pub async fn range(&self, from_seq: u64, limit: usize) -> Result<RangeResponse, String> {
        let url = format!(
            "{}/mcr/v1/rooms/{}/messages?from_seq={}&limit={}",
            self.base_url,
            self.room_id,
            from_seq,
            limit.clamp(1, MAX_LIMIT)
        );
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("range request failed: {e}"))?;
        resp.json::<RangeResponse>()
            .await
            .map_err(|e| format!("range decode failed: {e}"))
    }

    pub async fn publish_with_msg_id(
        &self,
        msg_id: impl Into<String>,
        payload_type: impl Into<String>,
        payload: serde_json::Value,
    ) -> Result<PublishReceipt, String> {
        let req = PublishRequest {
            v: 1,
            room_id: self.room_id.clone(),
            msg_id: msg_id.into(),
            sender_id: self.sender_id.clone(),
            sent_at_ms: now_ms(),
            payload_type: payload_type.into(),
            payload,
            meta: Some(serde_json::json!({"content_type":"application/json"})),
        };
        let url = format!("{}/mcr/v1/rooms/{}/publish", self.base_url, self.room_id);

        let mut builder = self.http.post(url).json(&req);
        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }
        let resp = builder
            .send()
            .await
            .map_err(|e| format!("publish request failed: {e}"))?;
        resp.json::<PublishReceipt>()
            .await
            .map_err(|e| format!("publish decode failed: {e}"))
    }

    pub async fn publish_json(
        &self,
        payload_type: impl Into<String>,
        payload: serde_json::Value,
    ) -> Result<PublishReceipt, String> {
        let msg_id = format!(
            "{:016x}-{:016x}",
            rand::random::<u64>(),
            rand::random::<u64>()
        );
        self.publish_with_msg_id(msg_id, payload_type, payload)
            .await
    }

    pub async fn initial_attach(&mut self) -> Result<usize, String> {
        let head = self.head().await?;
        if self.last_seq >= head.head_seq {
            return Ok(0);
        }
        self.catch_up_from(self.last_seq + 1).await
    }

    pub async fn handle_live(&mut self, env: McrEnvelope) -> Result<usize, String> {
        if env.seq <= self.last_seq {
            return Ok(0);
        }

        if env.seq == self.last_seq + 1 {
            let mut applied = self.apply_env(env);
            applied += self.drain_holdback();
            return Ok(applied);
        }

        self.holdback.insert(env.seq, env);
        let mut applied = self.catch_up_from(self.last_seq + 1).await?;
        applied += self.drain_holdback();
        Ok(applied)
    }

    fn remember_msg_id(&mut self, msg_id: &str) {
        if self.seen_msg_ids.contains(msg_id) {
            return;
        }
        self.seen_msg_ids.insert(msg_id.to_string());
        self.seen_order.push_back(msg_id.to_string());
        while self.seen_order.len() > self.seen_cap {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen_msg_ids.remove(&old);
            }
        }
    }

    fn apply_env(&mut self, env: McrEnvelope) -> usize {
        if env.seq <= self.last_seq {
            return 0;
        }
        if env.seq != self.last_seq + 1 {
            return 0;
        }

        self.last_seq = env.seq;
        if self.seen_msg_ids.contains(&env.msg_id) {
            return 0;
        }
        self.remember_msg_id(&env.msg_id);
        self.applied.push(env);
        1
    }

    fn drain_holdback(&mut self) -> usize {
        let mut applied = 0usize;
        loop {
            let next_seq = self.last_seq + 1;
            let Some(next) = self.holdback.remove(&next_seq) else {
                break;
            };
            applied += self.apply_env(next);
        }
        applied
    }

    async fn catch_up_from(&mut self, mut from_seq: u64) -> Result<usize, String> {
        let mut applied = 0usize;
        loop {
            let page = self.range(from_seq, DEFAULT_LIMIT).await?;
            let mut prev = from_seq.saturating_sub(1);
            for item in page.items {
                if item.seq <= prev {
                    return Err("range response not strictly ascending".to_string());
                }
                prev = item.seq;
                if item.seq <= self.last_seq {
                    continue;
                }
                if item.seq > self.last_seq + 1 {
                    self.holdback.insert(item.seq, item);
                    continue;
                }
                applied += self.apply_env(item);
            }

            if !page.has_more {
                break;
            }
            from_seq = self.last_seq + 1;
            if from_seq == 0 {
                break;
            }
        }
        Ok(applied)
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
