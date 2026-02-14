//! Network transport for pika-media using moq-native over QUIC.
//!
//! Bridges the async moq-native pub/sub into the sync `mpsc` interface
//! that `call_runtime.rs` expects via `MediaFrame` + `Receiver<MediaFrame>`.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use moq_lite::{BroadcastProducer, Origin, Track, TrackProducer};
use moq_native::rustls;
use tokio::runtime::Runtime;
use url::Url;

use crate::session::{MediaFrame, MediaSessionError};
use crate::tracks::TrackAddress;

struct NetworkRelayInner {
    rt: Arc<Runtime>,
    url: Url,
    origin: moq_lite::OriginProducer,
    session: Option<moq_lite::Session>,
    broadcasts: std::collections::HashMap<String, BroadcastAndTrack>,
}

struct BroadcastAndTrack {
    _broadcast: BroadcastProducer,
    track: TrackProducer,
}

#[derive(Clone)]
pub struct NetworkRelay {
    inner: Arc<Mutex<NetworkRelayInner>>,
}

impl NetworkRelay {
    pub fn new(moq_url: &str) -> Result<Self, MediaSessionError> {
        let url = Url::parse(moq_url).map_err(|e| {
            MediaSessionError::InvalidTrack(format!("invalid moq url: {e}"))
        })?;

        let rt = Runtime::new().map_err(|_| {
            MediaSessionError::NotConnected
        })?;

        let origin = Origin::produce();

        Ok(Self {
            inner: Arc::new(Mutex::new(NetworkRelayInner {
                rt: Arc::new(rt),
                url,
                origin,
                session: None,
                broadcasts: std::collections::HashMap::new(),
            })),
        })
    }

    pub fn connect(&self) -> Result<(), MediaSessionError> {
        let mut inner = self.inner.lock().map_err(|_| MediaSessionError::NotConnected)?;

        if inner.session.is_some() {
            return Ok(());
        }

        // Install crypto provider if not already done
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        let url = inner.url.clone();
        let origin_cons = inner.origin.consume();

        // Both client init and connect must happen within the tokio runtime
        // because quinn::Endpoint binds UDP sockets on the tokio reactor.
        let session = inner.rt.block_on(async {
            let client_config = moq_native::ClientConfig::default();
            let client = client_config.init().map_err(|e| {
                MediaSessionError::Unauthorized(format!("moq client init failed: {e}"))
            })?;

            client
                .with_publish(origin_cons)
                .connect(url)
                .await
                .map_err(|_| MediaSessionError::NotConnected)
        })?;

        inner.session = Some(session);
        Ok(())
    }

    fn ensure_broadcast_and_track(
        inner: &mut NetworkRelayInner,
        track_addr: &TrackAddress,
    ) -> TrackProducer {
        // Must be called with runtime guard entered (web_async::spawn requirement)
        let key = track_addr.key();
        if let Some(bt) = inner.broadcasts.get(&key) {
            return bt.track.clone();
        }

        let mut broadcast = BroadcastProducer::default();
        let track = Track::new(&track_addr.track_name).produce();
        broadcast.insert_track(track.clone());

        inner.origin.publish_broadcast(
            &track_addr.broadcast_path,
            broadcast.consume(),
        );

        inner.broadcasts.insert(key, BroadcastAndTrack {
            _broadcast: broadcast,
            track: track.clone(),
        });

        track
    }

    pub fn publish(
        &self,
        track_addr: &TrackAddress,
        frame: MediaFrame,
    ) -> Result<usize, MediaSessionError> {
        let mut inner = self.inner.lock().map_err(|_| MediaSessionError::NotConnected)?;

        if inner.session.is_none() {
            return Err(MediaSessionError::NotConnected);
        }

        // Enter the tokio runtime context so web-async::spawn works in moq-lite
        let _guard = inner.rt.enter();

        let mut track = Self::ensure_broadcast_and_track(&mut inner, track_addr);
        track.write_frame(bytes::Bytes::from(frame.payload));

        Ok(1)
    }

    pub fn subscribe(
        &self,
        track_addr: &TrackAddress,
    ) -> Result<Receiver<MediaFrame>, MediaSessionError> {
        let inner = self.inner.lock().map_err(|_| MediaSessionError::NotConnected)?;

        if inner.session.is_none() {
            return Err(MediaSessionError::NotConnected);
        }

        let (tx, rx) = mpsc::channel::<MediaFrame>();

        // We need a second connection for subscribing
        let url = inner.url.clone();
        let broadcast_path = track_addr.broadcast_path.clone();
        let track_name = track_addr.track_name.clone();
        let rt = inner.rt.clone();

        // Spawn a background task that subscribes and forwards frames
        std::thread::spawn(move || {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

            rt.block_on(async {
                let sub_origin = Origin::produce();
                let client_config = moq_native::ClientConfig::default();
                let client = match client_config.init() {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("subscriber client init failed: {e}");
                        return;
                    }
                };

                let _session = match client
                    .with_consume(sub_origin.clone())
                    .connect(url)
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("subscriber connect failed: {e}");
                        return;
                    }
                };

                // Wait for broadcast to be announced
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                let origin_cons = sub_origin.consume();
                let broadcast_cons = match origin_cons.consume_broadcast(&broadcast_path) {
                    Some(b) => b,
                    None => {
                        // Try waiting a bit more and use announced() instead
                        tracing::warn!("broadcast not found yet, waiting for announcement...");
                        let mut consumer = origin_cons;
                        let announced = tokio::time::timeout(
                            std::time::Duration::from_secs(10),
                            consumer.announced(),
                        )
                        .await;

                        match announced {
                            Ok(Some(_announce)) => {
                                match consumer.consume_broadcast(&broadcast_path) {
                                    Some(b) => b,
                                    None => {
                                        tracing::error!(
                                            "broadcast still not found after announcement"
                                        );
                                        return;
                                    }
                                }
                            }
                            _ => {
                                tracing::error!("timed out waiting for broadcast announcement");
                                return;
                            }
                        }
                    }
                };

                let track = Track::new(&track_name);
                let mut track_cons = broadcast_cons.subscribe_track(&track);

                let mut seq = 0u64;
                loop {
                    match track_cons.next_group().await {
                        Ok(Some(mut group)) => {
                            match group.read_frame().await {
                                Ok(Some(data)) => {
                                    let frame = MediaFrame {
                                        seq,
                                        timestamp_us: seq * 20_000,
                                        keyframe: true,
                                        payload: data.to_vec(),
                                    };
                                    seq += 1;
                                    if tx.send(frame).is_err() {
                                        break; // receiver dropped
                                    }
                                }
                                Ok(None) => continue,
                                Err(_) => break,
                            }
                        }
                        Ok(None) => break, // track closed
                        Err(_) => break,
                    }
                }
            });
        });

        Ok(rx)
    }

    pub fn disconnect(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if let Some(session) = inner.session.take() {
                session.close(moq_lite::Error::Cancel);
            }
            inner.broadcasts.clear();
        }
    }
}
