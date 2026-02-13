use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use flume::Sender;
use pika_media::codec_opus::{OpusCodec, OpusPacket};
use pika_media::jitter::JitterBuffer;
use pika_media::session::{
    InMemoryRelay, MediaFrame, MediaSession, MediaSessionError, SessionConfig,
};
use pika_media::tracks::{broadcast_path, TrackAddress};

use crate::updates::{CoreMsg, InternalEvent};

use super::call_control::CallSessionParams;

const SAMPLE_RATE: u32 = 48_000;
const FRAME_SAMPLES: usize = 960; // 20ms @ 48kHz mono.

#[derive(Debug)]
struct CallWorker {
    stop: Arc<AtomicBool>,
    muted: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
pub(super) struct CallRuntime {
    workers: HashMap<String, CallWorker>, // call_id -> worker
}

fn relay_pool() -> &'static Mutex<HashMap<String, InMemoryRelay>> {
    static RELAYS: OnceLock<Mutex<HashMap<String, InMemoryRelay>>> = OnceLock::new();
    RELAYS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn relay_key(session: &CallSessionParams) -> String {
    format!("{}|{}", session.moq_url, session.broadcast_base)
}

fn shared_relay_for(session: &CallSessionParams) -> InMemoryRelay {
    let key = relay_key(session);
    let mut map = relay_pool().lock().expect("relay pool lock poisoned");
    map.entry(key).or_insert_with(InMemoryRelay::new).clone()
}

impl CallRuntime {
    pub(super) fn on_call_connecting(
        &mut self,
        call_id: &str,
        session: &CallSessionParams,
        local_pubkey_hex: &str,
        peer_pubkey_hex: &str,
        audio_backend_mode: Option<&str>,
        tx: Sender<CoreMsg>,
    ) -> Result<(), String> {
        self.on_call_ended(call_id);

        let relay = shared_relay_for(session);
        let mut media = MediaSession::with_relay(
            SessionConfig {
                moq_url: session.moq_url.clone(),
            },
            relay,
        );
        media.connect();

        let local_path = broadcast_path(&session.broadcast_base, local_pubkey_hex)?;
        let peer_path = broadcast_path(&session.broadcast_base, peer_pubkey_hex)?;
        let publish_track = TrackAddress {
            broadcast_path: local_path,
            track_name: "audio0".to_string(),
        };
        let subscribe_track = TrackAddress {
            broadcast_path: peer_path,
            track_name: "audio0".to_string(),
        };
        let rx = media.subscribe(&subscribe_track).map_err(to_string_error)?;

        let call_id_owned = call_id.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = stop.clone();
        let muted = Arc::new(AtomicBool::new(false));
        let muted_for_thread = muted.clone();
        let tx_for_thread = tx.clone();
        let mut audio_backend = match AudioBackend::try_new(audio_backend_mode) {
            Ok(v) => v,
            Err(err) => {
                let _ = tx_for_thread.send(CoreMsg::Internal(Box::new(InternalEvent::Toast(
                    format!("Audio backend fallback: {err}"),
                ))));
                AudioBackend::synthetic()
            }
        };
        thread::spawn(move || {
            let _ = tx_for_thread.send(CoreMsg::Internal(Box::new(
                InternalEvent::CallRuntimeConnected {
                    call_id: call_id_owned.clone(),
                },
            )));

            let codec = OpusCodec;
            let mut seq = 0u64;
            let mut tx_frames = 0u64;
            let mut rx_frames = 0u64;
            let mut jitter = JitterBuffer::<Vec<i16>>::new(8);
            let mut tick = 0u64;

            while !stop_for_thread.load(Ordering::Relaxed) {
                if !muted_for_thread.load(Ordering::Relaxed) {
                    let pcm = audio_backend.capture_pcm_frame();
                    let packet = codec.encode_pcm_i16(&pcm);
                    let frame = MediaFrame {
                        seq,
                        timestamp_us: seq.saturating_mul(20_000),
                        keyframe: true,
                        payload: packet.0,
                    };
                    if media.publish(&publish_track, frame).is_ok() {
                        tx_frames = tx_frames.saturating_add(1);
                        seq = seq.saturating_add(1);
                    }
                }

                while let Ok(inbound) = rx.try_recv() {
                    rx_frames = rx_frames.saturating_add(1);
                    let pcm = codec.decode_to_pcm_i16(&OpusPacket(inbound.payload));
                    let _ = jitter.push(pcm);
                }
                if let Some(playback_pcm) = jitter.pop() {
                    audio_backend.play_pcm_frame(&playback_pcm);
                }

                tick = tick.saturating_add(1);
                if tick % 5 == 0 {
                    let _ = tx_for_thread.send(CoreMsg::Internal(Box::new(
                        InternalEvent::CallRuntimeStats {
                            call_id: call_id_owned.clone(),
                            tx_frames,
                            rx_frames,
                            rx_dropped: jitter.dropped(),
                            // 20ms packets.
                            jitter_buffer_ms: (jitter.len() as u32).saturating_mul(20),
                            last_rtt_ms: None,
                        },
                    )));
                }

                thread::sleep(Duration::from_millis(20));
            }
        });

        self.workers
            .insert(call_id.to_string(), CallWorker { stop, muted });
        Ok(())
    }

    pub(super) fn set_muted(&mut self, call_id: &str, muted: bool) {
        if let Some(worker) = self.workers.get(call_id) {
            worker.muted.store(muted, Ordering::Relaxed);
        }
    }

    pub(super) fn on_call_ended(&mut self, call_id: &str) {
        if let Some(worker) = self.workers.remove(call_id) {
            worker.stop.store(true, Ordering::Relaxed);
        }
    }

    pub(super) fn stop_all(&mut self) {
        let call_ids: Vec<String> = self.workers.keys().cloned().collect();
        for call_id in call_ids {
            self.on_call_ended(&call_id);
        }
    }
}

fn to_string_error(err: MediaSessionError) -> String {
    err.to_string()
}

#[derive(Debug)]
enum AudioBackend {
    Synthetic(SyntheticAudio),
    #[cfg(target_os = "ios")]
    Cpal(CpalAudio),
}

impl AudioBackend {
    fn synthetic() -> Self {
        Self::Synthetic(SyntheticAudio::new())
    }

    fn try_new(mode: Option<&str>) -> Result<Self, String> {
        let normalized = mode
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(default_backend_mode())
            .to_ascii_lowercase();
        match normalized.as_str() {
            "synthetic" => Ok(Self::synthetic()),
            "cpal" => {
                #[cfg(target_os = "ios")]
                {
                    CpalAudio::new().map(Self::Cpal)
                }
                #[cfg(not(target_os = "ios"))]
                {
                    Err("cpal backend is currently iOS-only; using synthetic".to_string())
                }
            }
            other => Err(format!(
                "unknown call audio backend '{other}'; using synthetic"
            )),
        }
    }

    fn capture_pcm_frame(&mut self) -> Vec<i16> {
        match self {
            Self::Synthetic(v) => v.capture_pcm_frame(),
            #[cfg(target_os = "ios")]
            Self::Cpal(v) => v.capture_pcm_frame(),
        }
    }

    fn play_pcm_frame(&mut self, pcm: &[i16]) {
        match self {
            Self::Synthetic(v) => v.play_pcm_frame(pcm),
            #[cfg(target_os = "ios")]
            Self::Cpal(v) => v.play_pcm_frame(pcm),
        }
    }
}

fn default_backend_mode() -> &'static str {
    #[cfg(target_os = "ios")]
    {
        "cpal"
    }
    #[cfg(not(target_os = "ios"))]
    {
        "synthetic"
    }
}

#[derive(Debug)]
struct SyntheticAudio {
    phase: f32,
}

impl SyntheticAudio {
    fn new() -> Self {
        Self { phase: 0.0 }
    }

    fn capture_pcm_frame(&mut self) -> Vec<i16> {
        let mut out = Vec::with_capacity(FRAME_SAMPLES);
        let freq = 220.0f32;
        let step = (2.0f32 * std::f32::consts::PI * freq) / SAMPLE_RATE as f32;
        for _ in 0..FRAME_SAMPLES {
            let sample = (self.phase.sin() * (i16::MAX as f32 * 0.15f32)) as i16;
            out.push(sample);
            self.phase += step;
            if self.phase > 2.0f32 * std::f32::consts::PI {
                self.phase -= 2.0f32 * std::f32::consts::PI;
            }
        }
        out
    }

    fn play_pcm_frame(&mut self, _pcm: &[i16]) {
        // No-op in synthetic mode.
    }
}

#[cfg(target_os = "ios")]
#[derive(Debug)]
struct CpalAudio {
    capture: Arc<Mutex<std::collections::VecDeque<i16>>>,
    playback: Arc<Mutex<std::collections::VecDeque<i16>>>,
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
}

#[cfg(target_os = "ios")]
impl CpalAudio {
    fn new() -> Result<Self, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let input_device = host
            .default_input_device()
            .ok_or_else(|| "no input audio device available".to_string())?;
        let output_device = host
            .default_output_device()
            .ok_or_else(|| "no output audio device available".to_string())?;

        let input_cfg = input_device
            .default_input_config()
            .map_err(|e| format!("input config error: {e}"))?;
        let output_cfg = output_device
            .default_output_config()
            .map_err(|e| format!("output config error: {e}"))?;

        let capture = Arc::new(Mutex::new(std::collections::VecDeque::<i16>::new()));
        let playback = Arc::new(Mutex::new(std::collections::VecDeque::<i16>::new()));

        let capture_for_input = capture.clone();
        let input_stream = match input_cfg.sample_format() {
            cpal::SampleFormat::I16 => {
                let channels = input_cfg.channels() as usize;
                input_device
                    .build_input_stream(
                        &input_cfg.config(),
                        move |data: &[i16], _| {
                            push_mono_i16_from_i16(data, channels, &capture_for_input);
                        },
                        |_| {},
                        None,
                    )
                    .map_err(|e| format!("build input stream failed: {e}"))?
            }
            cpal::SampleFormat::U16 => {
                let channels = input_cfg.channels() as usize;
                input_device
                    .build_input_stream(
                        &input_cfg.config(),
                        move |data: &[u16], _| {
                            push_mono_i16_from_u16(data, channels, &capture_for_input);
                        },
                        |_| {},
                        None,
                    )
                    .map_err(|e| format!("build input stream failed: {e}"))?
            }
            cpal::SampleFormat::F32 => {
                let channels = input_cfg.channels() as usize;
                input_device
                    .build_input_stream(
                        &input_cfg.config(),
                        move |data: &[f32], _| {
                            push_mono_i16_from_f32(data, channels, &capture_for_input);
                        },
                        |_| {},
                        None,
                    )
                    .map_err(|e| format!("build input stream failed: {e}"))?
            }
            other => {
                return Err(format!("unsupported input sample format: {other:?}"));
            }
        };

        let playback_for_output = playback.clone();
        let output_stream = match output_cfg.sample_format() {
            cpal::SampleFormat::I16 => {
                let channels = output_cfg.channels() as usize;
                output_device
                    .build_output_stream(
                        &output_cfg.config(),
                        move |data: &mut [i16], _| {
                            pop_playback_to_i16(data, channels, &playback_for_output);
                        },
                        |_| {},
                        None,
                    )
                    .map_err(|e| format!("build output stream failed: {e}"))?
            }
            cpal::SampleFormat::U16 => {
                let channels = output_cfg.channels() as usize;
                output_device
                    .build_output_stream(
                        &output_cfg.config(),
                        move |data: &mut [u16], _| {
                            pop_playback_to_u16(data, channels, &playback_for_output);
                        },
                        |_| {},
                        None,
                    )
                    .map_err(|e| format!("build output stream failed: {e}"))?
            }
            cpal::SampleFormat::F32 => {
                let channels = output_cfg.channels() as usize;
                output_device
                    .build_output_stream(
                        &output_cfg.config(),
                        move |data: &mut [f32], _| {
                            pop_playback_to_f32(data, channels, &playback_for_output);
                        },
                        |_| {},
                        None,
                    )
                    .map_err(|e| format!("build output stream failed: {e}"))?
            }
            other => {
                return Err(format!("unsupported output sample format: {other:?}"));
            }
        };

        input_stream
            .play()
            .map_err(|e| format!("start input stream failed: {e}"))?;
        output_stream
            .play()
            .map_err(|e| format!("start output stream failed: {e}"))?;

        Ok(Self {
            capture,
            playback,
            _input_stream: input_stream,
            _output_stream: output_stream,
        })
    }

    fn capture_pcm_frame(&mut self) -> Vec<i16> {
        let mut out = vec![0i16; FRAME_SAMPLES];
        let mut q = self.capture.lock().expect("capture queue lock poisoned");
        for sample in out.iter_mut() {
            if let Some(v) = q.pop_front() {
                *sample = v;
            } else {
                break;
            }
        }
        out
    }

    fn play_pcm_frame(&mut self, pcm: &[i16]) {
        let mut q = self.playback.lock().expect("playback queue lock poisoned");
        for sample in pcm {
            q.push_back(*sample);
        }
        while q.len() > (SAMPLE_RATE as usize * 2) {
            q.pop_front();
        }
    }
}

#[cfg(target_os = "ios")]
fn push_capture_sample(queue: &Arc<Mutex<std::collections::VecDeque<i16>>>, sample: i16) {
    let mut q = queue.lock().expect("capture queue lock poisoned");
    q.push_back(sample);
    while q.len() > (SAMPLE_RATE as usize * 2) {
        q.pop_front();
    }
}

#[cfg(target_os = "ios")]
fn push_mono_i16_from_i16(
    data: &[i16],
    channels: usize,
    queue: &Arc<Mutex<std::collections::VecDeque<i16>>>,
) {
    for frame in data.chunks(channels.max(1)) {
        if let Some(s) = frame.first() {
            push_capture_sample(queue, *s);
        }
    }
}

#[cfg(target_os = "ios")]
fn push_mono_i16_from_u16(
    data: &[u16],
    channels: usize,
    queue: &Arc<Mutex<std::collections::VecDeque<i16>>>,
) {
    for frame in data.chunks(channels.max(1)) {
        if let Some(s) = frame.first() {
            push_capture_sample(queue, (*s as i32 - 32_768) as i16);
        }
    }
}

#[cfg(target_os = "ios")]
fn push_mono_i16_from_f32(
    data: &[f32],
    channels: usize,
    queue: &Arc<Mutex<std::collections::VecDeque<i16>>>,
) {
    for frame in data.chunks(channels.max(1)) {
        if let Some(s) = frame.first() {
            let clamped = s.clamp(-1.0, 1.0);
            push_capture_sample(queue, (clamped * i16::MAX as f32) as i16);
        }
    }
}

#[cfg(target_os = "ios")]
fn pop_playback_sample(queue: &Arc<Mutex<std::collections::VecDeque<i16>>>) -> i16 {
    let mut q = queue.lock().expect("playback queue lock poisoned");
    q.pop_front().unwrap_or(0)
}

#[cfg(target_os = "ios")]
fn pop_playback_to_i16(
    data: &mut [i16],
    channels: usize,
    queue: &Arc<Mutex<std::collections::VecDeque<i16>>>,
) {
    for frame in data.chunks_mut(channels.max(1)) {
        let s = pop_playback_sample(queue);
        for dst in frame.iter_mut() {
            *dst = s;
        }
    }
}

#[cfg(target_os = "ios")]
fn pop_playback_to_u16(
    data: &mut [u16],
    channels: usize,
    queue: &Arc<Mutex<std::collections::VecDeque<i16>>>,
) {
    for frame in data.chunks_mut(channels.max(1)) {
        let s = pop_playback_sample(queue) as i32 + 32_768;
        let s = s.clamp(0, u16::MAX as i32) as u16;
        for dst in frame.iter_mut() {
            *dst = s;
        }
    }
}

#[cfg(target_os = "ios")]
fn pop_playback_to_f32(
    data: &mut [f32],
    channels: usize,
    queue: &Arc<Mutex<std::collections::VecDeque<i16>>>,
) {
    for frame in data.chunks_mut(channels.max(1)) {
        let s = pop_playback_sample(queue) as f32 / i16::MAX as f32;
        for dst in frame.iter_mut() {
            *dst = s;
        }
    }
}
