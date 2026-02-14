//! Network transport for pika-media using moq-native over QUIC.
//!
//! Bridges the async moq-native pub/sub into the sync `mpsc` interface
//! that `call_runtime.rs` expects via `MediaFrame` + `Receiver<MediaFrame>`.
//!
//! Design note: QUIC setup is async, but some callers run a tight sync tick loop.
//! `NetworkRelay` keeps a sync API by offloading all async work onto a dedicated
//! background thread that owns a Tokio runtime. This avoids calling `block_on()`
//! from within an ambient Tokio runtime (which panics).

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use moq_lite::{BroadcastProducer, Origin, Track, TrackProducer};
use moq_native::rustls;
use tokio::runtime::Runtime;
use url::Url;

use crate::session::{MediaFrame, MediaSessionError};
use crate::tracks::TrackAddress;

struct BroadcastAndTrack {
    _broadcast: BroadcastProducer,
    track: TrackProducer,
}

#[derive(Clone)]
pub struct NetworkRelay {
    worker: Arc<NetworkRelayWorker>,
}

impl NetworkRelay {
    pub fn new(moq_url: &str) -> Result<Self, MediaSessionError> {
        let url = Url::parse(moq_url)
            .map_err(|e| MediaSessionError::InvalidTrack(format!("invalid moq url: {e}")))?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), MediaSessionError>>();

        let join = thread::Builder::new()
            .name("pika-network-relay".to_string())
            .spawn(move || {
                // Install crypto provider if not already done
                let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

                let rt = match Runtime::new() {
                    Ok(rt) => rt,
                    Err(_) => {
                        let _ = ready_tx.send(Err(MediaSessionError::NotConnected));
                        return;
                    }
                };

                let mut state = NetworkRelayState {
                    rt,
                    url,
                    origin: Origin::produce(),
                    session: None,
                    broadcasts: HashMap::new(),
                };

                let _ = ready_tx.send(Ok(()));
                state.run(cmd_rx);
            })
            .map_err(|_| MediaSessionError::NotConnected)?;

        ready_rx
            .recv()
            .map_err(|_| MediaSessionError::NotConnected)??;

        let thread_id = join.thread().id();

        Ok(Self {
            worker: Arc::new(NetworkRelayWorker {
                tx: cmd_tx,
                join: Some(join),
                thread_id,
            }),
        })
    }

    pub fn connect(&self) -> Result<(), MediaSessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.worker
            .tx
            .send(Command::Connect { reply: reply_tx })
            .map_err(|_| MediaSessionError::NotConnected)?;
        reply_rx
            .recv()
            .map_err(|_| MediaSessionError::NotConnected)?
    }

    pub fn publish(
        &self,
        track_addr: &TrackAddress,
        frame: MediaFrame,
    ) -> Result<usize, MediaSessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.worker
            .tx
            .send(Command::Publish {
                track_addr: track_addr.clone(),
                frame,
                reply: reply_tx,
            })
            .map_err(|_| MediaSessionError::NotConnected)?;
        reply_rx
            .recv()
            .map_err(|_| MediaSessionError::NotConnected)?
    }

    pub fn subscribe(
        &self,
        track_addr: &TrackAddress,
    ) -> Result<Receiver<MediaFrame>, MediaSessionError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.worker
            .tx
            .send(Command::Subscribe {
                track_addr: track_addr.clone(),
                reply: reply_tx,
            })
            .map_err(|_| MediaSessionError::NotConnected)?;
        reply_rx
            .recv()
            .map_err(|_| MediaSessionError::NotConnected)?
    }

    pub fn disconnect(&self) {
        let (reply_tx, reply_rx) = mpsc::channel();
        let _ = self.worker.tx.send(Command::Disconnect { reply: reply_tx });
        let _ = reply_rx.recv();
    }
}

enum Command {
    Connect {
        reply: Sender<Result<(), MediaSessionError>>,
    },
    Publish {
        track_addr: TrackAddress,
        frame: MediaFrame,
        reply: Sender<Result<usize, MediaSessionError>>,
    },
    Subscribe {
        track_addr: TrackAddress,
        reply: Sender<Result<Receiver<MediaFrame>, MediaSessionError>>,
    },
    Disconnect {
        reply: Sender<()>,
    },
    Shutdown,
}

struct NetworkRelayWorker {
    tx: Sender<Command>,
    join: Option<JoinHandle<()>>,
    thread_id: thread::ThreadId,
}

impl Drop for NetworkRelayWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if thread::current().id() == self.thread_id {
            // Avoid deadlocking by joining ourselves.
            let _ = self.join.take();
            return;
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct NetworkRelayState {
    rt: Runtime,
    url: Url,
    origin: moq_lite::OriginProducer,
    session: Option<moq_lite::Session>,
    broadcasts: HashMap<String, BroadcastAndTrack>,
}

impl NetworkRelayState {
    fn run(&mut self, rx: Receiver<Command>) {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                Command::Connect { reply } => {
                    let _ = reply.send(self.connect());
                }
                Command::Publish {
                    track_addr,
                    frame,
                    reply,
                } => {
                    let _ = reply.send(self.publish(&track_addr, frame));
                }
                Command::Subscribe { track_addr, reply } => {
                    let _ = reply.send(self.subscribe(&track_addr));
                }
                Command::Disconnect { reply } => {
                    self.disconnect();
                    let _ = reply.send(());
                }
                Command::Shutdown => {
                    self.disconnect();
                    break;
                }
            }
        }
        self.disconnect();
    }

    fn connect(&mut self) -> Result<(), MediaSessionError> {
        if self.session.is_some() {
            return Ok(());
        }

        let url = self.url.clone();
        let origin_cons = self.origin.consume();

        // Both client init and connect must happen within the tokio runtime
        // because quinn::Endpoint binds UDP sockets on the tokio reactor.
        let session = self.rt.block_on(async {
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

        self.session = Some(session);
        Ok(())
    }

    fn ensure_broadcast_and_track(&mut self, track_addr: &TrackAddress) -> TrackProducer {
        // Must be called with runtime guard entered (web_async::spawn requirement).
        let key = track_addr.key();
        if let Some(bt) = self.broadcasts.get(&key) {
            return bt.track.clone();
        }

        let mut broadcast = BroadcastProducer::default();
        let track = Track::new(&track_addr.track_name).produce();
        broadcast.insert_track(track.clone());

        self.origin
            .publish_broadcast(&track_addr.broadcast_path, broadcast.consume());

        self.broadcasts.insert(
            key,
            BroadcastAndTrack {
                _broadcast: broadcast,
                track: track.clone(),
            },
        );

        track
    }

    fn publish(
        &mut self,
        track_addr: &TrackAddress,
        frame: MediaFrame,
    ) -> Result<usize, MediaSessionError> {
        if self.session.is_none() {
            return Err(MediaSessionError::NotConnected);
        }

        let _guard = self.rt.enter();
        let mut track = self.ensure_broadcast_and_track(track_addr);
        track.write_frame(bytes::Bytes::from(frame.payload));

        Ok(1)
    }

    fn subscribe(
        &mut self,
        track_addr: &TrackAddress,
    ) -> Result<Receiver<MediaFrame>, MediaSessionError> {
        if self.session.is_none() {
            return Err(MediaSessionError::NotConnected);
        }

        let (tx, rx) = mpsc::channel::<MediaFrame>();

        let url = self.url.clone();
        let broadcast_path = track_addr.broadcast_path.clone();
        let track_name = track_addr.track_name.clone();

        self.rt.spawn(async move {
            let sub_origin = Origin::produce();
            let client_config = moq_native::ClientConfig::default();
            let client = match client_config.init() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("subscriber client init failed: {e}");
                    return;
                }
            };

            let _session = match client.with_consume(sub_origin.clone()).connect(url).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("subscriber connect failed: {e}");
                    return;
                }
            };

            // Wait for broadcast to be announced.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let origin_cons = sub_origin.consume();
            let broadcast_cons = match origin_cons.consume_broadcast(&broadcast_path) {
                Some(b) => b,
                None => {
                    tracing::warn!("broadcast not found yet, waiting for announcement...");
                    let mut consumer = origin_cons;
                    let announced = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        consumer.announced(),
                    )
                    .await;

                    match announced {
                        Ok(Some(_announce)) => match consumer.consume_broadcast(&broadcast_path) {
                            Some(b) => b,
                            None => {
                                tracing::error!("broadcast still not found after announcement");
                                return;
                            }
                        },
                        _ => {
                            tracing::error!("timed out waiting for broadcast announcement");
                            return;
                        }
                    }
                }
            };

            let track = Track::new(&track_name);
            let mut track_cons = broadcast_cons.subscribe_track(&track);

            tracing::info!("subscriber: starting group receive loop");

            let mut seq = 0u64;
            loop {
                match track_cons.next_group().await {
                    Ok(Some(mut group)) => match group.read_frame().await {
                        Ok(Some(data)) => {
                            let frame = MediaFrame {
                                seq,
                                timestamp_us: seq * 20_000,
                                keyframe: true,
                                payload: data.to_vec(),
                            };
                            seq += 1;
                            if tx.send(frame).is_err() {
                                tracing::info!("subscriber: receiver dropped, stopping");
                                break;
                            }
                        }
                        Ok(None) => continue,
                        Err(e) => {
                            tracing::debug!("subscriber: read_frame error (continuing): {e}");
                            continue;
                        }
                    },
                    Ok(None) => {
                        tracing::info!("subscriber: track closed (no more groups)");
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("subscriber: next_group error: {e}");
                        break;
                    }
                }
            }

            tracing::info!("subscriber: loop ended after {seq} frames");
        });

        Ok(rx)
    }

    fn disconnect(&mut self) {
        if let Some(session) = self.session.take() {
            session.close(moq_lite::Error::Cancel);
        }
        self.broadcasts.clear();
    }
}
