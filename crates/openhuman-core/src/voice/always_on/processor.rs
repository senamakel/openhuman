//! The async processor: owns the on/off gates, opens the VAD session, turns
//! raw capture chunks into segmented utterances, and hands finished
//! utterances to transcription.

use super::capture::{spawn_capture_thread, RawChunk};
use super::lock_watcher::spawn_lock_watcher;
use super::transcribe::transcribe_and_deliver;
use super::LOG_PREFIX;
use crate::config::Config;
use crate::modules::voice as tinyvoice;
use crate::voice::audio_capture::TARGET_SAMPLE_RATE;
use std::sync::atomic::{AtomicBool, Ordering};

/// How long to wait before retrying a VAD session that would not open.
///
/// Long enough that a persistently unavailable module does not produce a call
/// per audio chunk, short enough that a module which finishes downloading
/// mid-session starts segmenting without the user restarting anything.
const SESSION_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// How many raw capture chunks may queue for the processor.
///
/// The channel was unbounded, which was survivable when the callback did the
/// downmix and resample itself: the processor's work was pure arithmetic and it
/// always outran the microphone. It no longer does — each chunk now costs three
/// module round trips — so a stalled or slow processor could let the queue grow
/// without limit behind a producer that never blocks.
///
/// A few seconds of chunks at typical `cpal` buffer sizes. Deliberately a
/// *count* rather than a byte budget: the callback must not do arithmetic to
/// decide whether to send.
const CAPTURE_QUEUE_CHUNKS: usize = 256;

/// The capture thread + processor have been spawned (once per process).
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Runtime on/off, mirrors `config.voice_server.always_on_enabled`. Toggling it
/// at runtime takes effect immediately: when false the processor drops all audio
/// (nothing is transcribed or sent). Lets the Settings toggle work without a
/// restart. (The mic stream itself stays open until the next launch.)
pub(super) static ENABLED: AtomicBool = AtomicBool::new(false);

/// When true, the processor drops audio and resets the segmenter (privacy hook:
/// screen locked). Driven by [`spawn_lock_watcher`] on macOS.
pub(super) static PAUSED: AtomicBool = AtomicBool::new(false);

/// VAD frame size. 20 ms at 16 kHz = 320 samples — small enough for responsive
/// onset/hangover detection, large enough for a stable RMS estimate.
const FRAME_MS: u32 = 20;
const FRAME_SAMPLES: usize = (TARGET_SAMPLE_RATE as usize / 1000) * FRAME_MS as usize;

/// Hard cap on a buffered utterance (defensive — the segmenter's
/// `max_utterance_ms` should flush first; this bounds memory if it doesn't).
const MAX_UTTERANCE_SAMPLES: usize = TARGET_SAMPLE_RATE as usize * 60;

/// Apply the always-on config: set the runtime ENABLED gate and, when enabled,
/// open the continuous microphone stream (once per process). Safe to call at
/// boot **and** at runtime (the Settings toggle calls it via the config RPC):
/// toggling off flips `ENABLED` so the processor immediately stops transcribing/
/// delivering; toggling on starts capture live without a restart.
///
/// Opens a continuous mic stream, segments it through the `tinyvoice` module, and
/// routes each finished utterance through STT and the dictation delivery bus (so
/// it reaches the agent exactly like a hotkey dictation, and lights up the notch).
pub async fn start_if_enabled(app_config: &Config) {
    let on = app_config.voice_server.always_on_enabled;
    ENABLED.store(on, Ordering::SeqCst);
    if !on {
        log::info!("{LOG_PREFIX} disabled — capture idle (toggle off)");
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        log::info!("{LOG_PREFIX} re-enabled; capture already running");
        return;
    }

    let vad = tinyvoice::vad_config_from_server_config(&app_config.voice_server);
    let config = app_config.clone();
    log::info!(
        "{LOG_PREFIX} enabled — onset={:.4} hangover={}ms min_speech={}ms max_utt={}ms",
        vad.onset_threshold,
        vad.hangover_ms,
        vad.min_speech_ms,
        vad.max_utterance_ms
    );

    // The cpal stream is `!Send`, so it lives on a dedicated thread that pushes
    // RAW interleaved chunks over a channel to the async processor below —
    // deliberately raw: every transform now happens off the audio callback.
    // `spawn_capture_thread` blocks on a synchronous readiness handshake while
    // the OS builds the input stream — cold WASAPI init on Windows can take a
    // while — so run it on the blocking pool. This function is polled
    // concurrently with the other login-gated services (#3490), and blocking an
    // async worker here would stall them.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RawChunk>(CAPTURE_QUEUE_CHUNKS);
    log::debug!(
        "{LOG_PREFIX} starting microphone capture (blocking readiness handshake on the blocking pool)"
    );
    // Distinguish a Tokio join failure (the blocking task itself panicked) from a
    // `spawn_capture_thread` setup error (e.g. no input device), so the log points
    // at the right layer instead of flattening both into one message.
    let format = match tokio::task::spawn_blocking(move || spawn_capture_thread(tx)).await {
        Ok(Ok(format)) => {
            log::debug!("{LOG_PREFIX} microphone capture stream ready");
            format
        }
        Ok(Err(e)) => {
            log::warn!("{LOG_PREFIX} could not start microphone capture: {e}");
            RUNNING.store(false, Ordering::SeqCst);
            return;
        }
        Err(join_err) => {
            log::error!(
                "{LOG_PREFIX} microphone capture setup task failed to join (panicked): {join_err}"
            );
            RUNNING.store(false, Ordering::SeqCst);
            return;
        }
    };

    // Privacy hook: pause capture while the screen is locked.
    spawn_lock_watcher();

    let onset_threshold = vad.onset_threshold;
    tokio::spawn(async move {
        // The segmenter lives in the module now, so opening it can fail for
        // reasons unrelated to audio — a download that has not happened yet, a
        // host with no published artifact.
        //
        // That failure must NOT end this task. The capture thread is already
        // running and owns the microphone for the process lifetime; returning
        // here would clear `RUNNING` while the stream stays live, and the next
        // `start_if_enabled` would sail past the `RUNNING` guard and open a
        // *second* microphone stream. So the session is opened lazily and
        // retried, and audio is dropped until there is one.
        let mut session: Option<tinyvoice::VadSession> = None;
        let mut last_open_attempt: Option<std::time::Instant> = None;

        let mut pending: Vec<f32> = Vec::new();
        let mut utterance: Vec<f32> = Vec::new();
        // Test-build diagnostics: confirm audio actually flows from the mic and
        // surface live input levels vs the onset threshold (every ~5s) so the VAD
        // can be tuned per mic/room without guessing. Levels are loudness, not PII.
        let mut first_chunk_logged = false;
        let mut level_peak: f32 = 0.0;
        let mut level_frames: u32 = 0;
        let mut last_level_log = std::time::Instant::now();

        while let Some(chunk) = rx.recv().await {
            if !first_chunk_logged {
                first_chunk_logged = true;
                log::info!(
                    "{LOG_PREFIX} first audio chunk received from mic (samples={}) — capture pipeline live",
                    chunk.samples.len()
                );
            }
            // Drop audio and abandon any in-flight utterance while paused
            // (screen locked) or toggled off — nothing is captured or sent.
            if PAUSED.load(Ordering::Relaxed) || !ENABLED.load(Ordering::Relaxed) {
                // Reset unconditionally rather than checking `is_speaking`
                // first: that check would be a second bus call to save a cheap
                // idempotent one, and the privacy path should be the shortest
                // path, not the cleverest.
                if let Some(open) = session.as_ref() {
                    if let Err(error) = open.reset(&config).await {
                        // Same reasoning as a failed push: a reset that did not
                        // land leaves a segmenter we cannot vouch for, and this
                        // is the privacy path, so discard it rather than trust
                        // it to have dropped the partial utterance.
                        log::warn!(
                            "{LOG_PREFIX} could not reset the VAD session ({error}); reopening"
                        );
                        session = None;
                        last_open_attempt = Some(std::time::Instant::now());
                    }
                }
                pending.clear();
                utterance.clear();
                continue;
            }

            // Open the segmenter on first use, retrying on a cooldown so a
            // module that becomes available later heals this without a restart.
            if session.is_none() {
                let due = last_open_attempt.is_none_or(|at| at.elapsed() >= SESSION_RETRY_INTERVAL);
                if !due {
                    continue;
                }
                last_open_attempt = Some(std::time::Instant::now());
                match tinyvoice::VadSession::open(&config, vad).await {
                    Ok(opened) => {
                        log::info!("{LOG_PREFIX} VAD session open; segmenting live audio");
                        session = Some(opened);
                    }
                    Err(error) => {
                        log::warn!(
                            "{LOG_PREFIX} could not open a VAD session ({error}); \
                             dropping audio and retrying in {}s",
                            SESSION_RETRY_INTERVAL.as_secs()
                        );
                        continue;
                    }
                }
            }

            // Downmix + resample in the module. This is the work that used to
            // happen inside the cpal callback.
            let mono16k = match tinyvoice::prepare_frames(
                &config,
                &chunk.samples,
                format.source_rate,
                format.channels,
            )
            .await
            {
                Ok(samples) => samples,
                Err(error) => {
                    log::warn!("{LOG_PREFIX} could not prepare capture frames: {error}");
                    continue;
                }
            };
            pending.extend_from_slice(&mono16k);

            // Whole frames only; the remainder stays in `pending` for the next
            // chunk so no audio is dropped at a chunk boundary.
            let whole = pending.len() / FRAME_SAMPLES * FRAME_SAMPLES;
            if whole == 0 {
                continue;
            }
            // Measure BEFORE draining. Draining first and then failing would
            // discard the frames outright — a module hiccup would eat the
            // user's audio rather than delay it.
            let energies =
                match tinyvoice::frame_energies(&config, &pending[..whole], FRAME_SAMPLES as u32)
                    .await
                {
                    Ok(energies) => energies,
                    Err(error) => {
                        log::warn!(
                            "{LOG_PREFIX} could not measure frame energies ({error}); \
                         retrying these frames on the next chunk"
                        );
                        continue;
                    }
                };
            let frames: Vec<f32> = pending.drain(..whole).collect();

            for rms in &energies {
                level_peak = level_peak.max(*rms);
            }
            level_frames += energies.len() as u32;
            if last_level_log.elapsed() >= std::time::Duration::from_secs(5) {
                log::info!(
                    "{LOG_PREFIX} mic level peak_rms={level_peak:.4} onset={onset_threshold:.4} frames={level_frames} ({})",
                    if level_peak >= onset_threshold {
                        "speech would trigger"
                    } else {
                        "below onset — lower vad_onset_threshold or check mic gain"
                    }
                );
                level_peak = 0.0;
                level_frames = 0;
                last_level_log = std::time::Instant::now();
            }

            // One push per chunk rather than per frame: same events, same
            // order, one round trip instead of N.
            let push = match session.as_ref() {
                Some(open) => open.push(&config, FRAME_MS, &energies).await,
                None => continue,
            };
            let events = match push {
                Ok(events) => events,
                Err(error) => {
                    // Drop the handle, do not just skip the chunk. A push fails
                    // when the module went away or the session is no longer
                    // open, and neither heals by itself — keeping the handle
                    // would reuse a dead session forever, because the lazy-open
                    // retry above only runs while `session` is `None`.
                    log::warn!("{LOG_PREFIX} VAD push failed ({error}); reopening the session");
                    session = None;
                    last_open_attempt = Some(std::time::Instant::now());
                    // The partial utterance belonged to the dead segmenter, so
                    // its boundaries mean nothing to the next one.
                    pending.clear();
                    utterance.clear();
                    continue;
                }
            };

            // `frame` indexes `frames`; slice the audio at the same boundaries
            // the segmenter reported so an utterance carries exactly the
            // samples it was measured from.
            let mut cursor = 0usize;
            for indexed in events {
                let frame = indexed.frame;
                match indexed.event {
                    tinyvoice::VadEvent::SpeechStart => {
                        let at = frame * FRAME_SAMPLES;
                        log::info!(
                            "{LOG_PREFIX} speech onset rms={:.4} (onset={onset_threshold:.4})",
                            energies.get(frame).copied().unwrap_or_default()
                        );
                        utterance.clear();
                        cursor = at;
                        notch_status("Listening", 2500); // pill: capturing speech
                    }
                    tinyvoice::VadEvent::SpeechEnd {
                        emit, voiced_ms, ..
                    } => {
                        let upto = ((frame + 1) * FRAME_SAMPLES).min(frames.len());
                        if upto > cursor && utterance.len() < MAX_UTTERANCE_SAMPLES {
                            utterance.extend_from_slice(&frames[cursor..upto]);
                        }
                        cursor = upto;
                        let captured = std::mem::take(&mut utterance);
                        log::info!(
                            "{LOG_PREFIX} utterance end voiced_ms={voiced_ms} emit={emit} samples={}",
                            captured.len()
                        );
                        if emit {
                            let cfg = config.clone();
                            tokio::spawn(async move {
                                transcribe_and_deliver(&cfg, captured).await;
                            });
                        }
                    }
                }
            }

            // Whatever is still open after the reported events belongs to the
            // utterance in progress.
            if cursor < frames.len()
                && !frames.is_empty()
                && utterance.len() < MAX_UTTERANCE_SAMPLES
                && match session.as_ref() {
                    Some(open) => open.is_speaking(&config).await.unwrap_or(false),
                    None => false,
                }
            {
                utterance.extend_from_slice(&frames[cursor..]);
            }
        }

        if let Some(open) = session.as_ref() {
            if let Err(error) = open.close(&config).await {
                log::warn!("{LOG_PREFIX} could not close the VAD session: {error}");
            }
        }
        log::info!("{LOG_PREFIX} capture channel closed; processor exiting");
        RUNNING.store(false, Ordering::SeqCst);
    });
}

/// Disable always-on listening at runtime (logout). Flips the `ENABLED` gate so
/// the processor immediately drops all audio — nothing is transcribed or sent —
/// the symmetric counterpart to [`start_if_enabled`]. The cpal stream itself
/// stays open (it's spawned once per process and reused if the user logs back in
/// and re-enables), but no audio is processed while disabled.
pub fn stop() {
    if ENABLED.swap(false, Ordering::SeqCst) {
        log::info!("{LOG_PREFIX} stopped (logout) — capture idle, audio dropped");
    }
}

/// Push a listener status to the always-visible notch pill via the
/// `overlay:attention` channel. The notch maps "Listening" / "Processing" to the
/// right icon; when the message expires it falls back to "Ready". Fire-and-forget.
pub(super) fn notch_status(status: &str, ttl_ms: u32) {
    let _ = crate::desktop::overlay::publish_attention(
        crate::desktop::overlay::OverlayAttentionEvent::new(status)
            .with_source("voice")
            .with_ttl_ms(ttl_ms),
    );
}
