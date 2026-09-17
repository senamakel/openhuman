//! Raw microphone capture: owns the `cpal` stream and forwards untouched
//! interleaved chunks to the async processor. Downmixing and resampling live
//! in the processor, not here — this runs on a realtime audio thread where
//! the right amount of work is the least possible.

use super::LOG_PREFIX;
use crate::voice::audio_capture::TARGET_SAMPLE_RATE;
use std::sync::atomic::Ordering;

/// One chunk of raw capture, exactly as the device delivered it.
///
/// Interleaved and at the device's own rate: the callback converts the sample
/// format and nothing else, so `channels` and `source_rate` travel with the
/// samples for the processor to hand to the module.
pub(super) struct RawChunk {
    /// Interleaved `f32` samples.
    pub(super) samples: Vec<f32>,
}

/// The device format, learned once when the stream is built.
#[derive(Debug, Clone, Copy)]
pub(super) struct CaptureFormat {
    /// Device sample rate, before resampling to [`TARGET_SAMPLE_RATE`].
    pub(super) source_rate: u32,
    /// Interleaved channel count.
    pub(super) channels: u16,
}

/// Chunks the capture callback had to drop because the queue was full.
///
/// A process-wide counter rather than closure state: the callback is built once
/// per sample format and each closure must stay `Fn`, so the count cannot live
/// in a captured local. One always-on stream exists per process, so a single
/// counter is not an aggregation of unrelated streams.
static DROPPED_CHUNKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Spawn the dedicated cpal capture thread. Blocks until the stream is set up
/// (or fails), mirroring `audio_capture::start_recording`'s readiness handshake.
pub(super) fn spawn_capture_thread(
    tx: tokio::sync::mpsc::Sender<RawChunk>,
) -> Result<CaptureFormat, String> {
    let (setup_tx, setup_rx) = std::sync::mpsc::sync_channel::<Result<CaptureFormat, String>>(1);
    std::thread::Builder::new()
        .name("voice-always-on".into())
        .spawn(move || {
            if let Err(e) = capture_on_thread(tx, &setup_tx) {
                log::warn!("{LOG_PREFIX} capture thread error: {e}");
                let _ = setup_tx.send(Err(e));
            }
        })
        .map_err(|e| format!("failed to spawn always-on capture thread: {e}"))?;
    match setup_rx.recv() {
        Ok(Ok(format)) => Ok(format),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("always-on capture thread exited before signalling readiness".to_string()),
    }
}

/// Owns the cpal stream for the process lifetime.
///
/// Each callback converts the device's sample format to `f32` and forwards the
/// interleaved buffer untouched. Downmixing and resampling used to happen here;
/// they now happen in the async processor, because this runs on a realtime
/// audio thread where the right amount of work is the least possible.
fn capture_on_thread(
    tx: tokio::sync::mpsc::Sender<RawChunk>,
    setup_tx: &std::sync::mpsc::SyncSender<Result<CaptureFormat, String>>,
) -> Result<(), String> {
    use crate::desktop::accessibility::{detect_microphone_permission, PermissionState};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, StreamConfig};

    // Surface the mic permission state explicitly — a denied/Unknown state is the
    // most common reason always-on "does nothing" and it differs per OS (macOS TCC
    // prompt, Windows privacy settings), so log it on every test build.
    let permission = detect_microphone_permission();
    log::info!("{LOG_PREFIX} microphone permission: {permission:?}");
    if matches!(permission, PermissionState::Denied) {
        log::warn!("{LOG_PREFIX} microphone permission denied — always-on cannot capture audio");
        return Err("microphone permission denied".to_string());
    }

    let host = cpal::default_host();
    log::info!("{LOG_PREFIX} audio host: {:?}", host.id());
    let device = host
        .default_input_device()
        .ok_or_else(|| "no default audio input device".to_string())?;
    let device_name = device.name().unwrap_or_else(|e| format!("<unknown: {e}>"));
    let supported = device
        .default_input_config()
        .map_err(|e| format!("no default input config: {e}"))?;
    let source_rate = supported.sample_rate().0;
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let stream_config: StreamConfig = supported.into();
    // Name + source rate/channels/format vary across M-chip, Intel, and Windows
    // mics; capturing them makes a "wrong device" or "unsupported format" failure
    // obvious from the log alone. We resample everything to 16 kHz mono downstream.
    log::info!(
        "{LOG_PREFIX} capture device ready name='{device_name}' rate={source_rate}->{TARGET_SAMPLE_RATE} channels={channels} format={sample_format:?}"
    );

    // Forward one raw interleaved chunk per callback.
    //
    // `try_send`, never `send`: this runs on a realtime audio thread where
    // blocking is a dropout, so a full queue drops the chunk rather than
    // waiting for the processor to catch up. Dropping the newest chunk is the
    // right end to lose — the queue ahead of it is older speech that is closer
    // to being transcribed.
    //
    // A send error also covers the processor being gone (shutdown), which is
    // why neither case is fatal here.
    let forward = move |samples: Vec<f32>| {
        if tx.try_send(RawChunk { samples }).is_err() {
            let dropped = DROPPED_CHUNKS.fetch_add(1, Ordering::Relaxed) + 1;
            // Log on a power-of-two schedule: a persistently overloaded
            // processor should be visible without logging inside every
            // callback once it starts.
            if dropped.is_power_of_two() {
                log::warn!("{LOG_PREFIX} capture queue full; dropped {dropped} chunk(s) so far");
            }
        }
    };

    let err_fn = |e| log::warn!("{LOG_PREFIX} cpal stream error: {e}");
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| forward(data.to_vec()),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                forward(data.iter().map(|&s| f32::from(s) / 32768.0).collect());
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                forward(data.iter().map(|&s| f32::from(s) / 32768.0 - 1.0).collect());
            },
            err_fn,
            None,
        ),
        other => return Err(format!("unsupported sample format: {other:?}")),
    }
    .map_err(|e| format!("failed to build input stream: {e}"))?;

    stream
        .play()
        .map_err(|e| format!("failed to start stream: {e}"))?;
    let _ = setup_tx.send(Ok(CaptureFormat {
        source_rate,
        channels,
    }));
    log::info!("{LOG_PREFIX} microphone stream live");

    // Keep the stream (and thus this thread) alive for the process lifetime.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
