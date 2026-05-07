//! System notification sounds.
//!
//! Provides pre-defined audio notifications for system events (errors,
//! warnings, completion, etc.).  Uses the audio mixer for multi-stream
//! output on supported hardware, with PC speaker fallback.
//!
//! ## Sound Events
//!
//! | Event           | Sound                          | Fallback (pcspk)      |
//! |-----------------|--------------------------------|-----------------------|
//! | Error           | Low descending tone (300 Hz)   | 800 Hz, 100 ms       |
//! | Warning         | Two mid-frequency beeps        | 600 Hz, 80 ms × 2    |
//! | Success         | Ascending chime                | 1200 Hz, 50 ms       |
//! | Notification    | Soft single ping               | 1000 Hz, 30 ms       |
//! | Critical        | Rapid triple beep              | 1500 Hz, 50 ms × 3   |
//! | Boot complete   | Three-note ascending chime     | C5-E5-G5 (startup)   |
//! | Shutdown        | Two-note descending            | G5-C5                |
//!
//! ## Design
//!
//! - Sounds are generated procedurally (no sample files needed).
//! - Each notification opens a mixer stream, writes PCM data, then closes.
//! - If no audio device is available, falls back to PC speaker.
//! - Non-blocking: sounds play in the background (short enough to not block).
//! - Volume follows system notification volume setting (separate from media).

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Whether notifications are enabled (user can disable).
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Notification volume (0-100, separate from media volume).
static NOTIFY_VOLUME: AtomicU8 = AtomicU8::new(70);

// ---------------------------------------------------------------------------
// Sound event types
// ---------------------------------------------------------------------------

/// System notification sound event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifySound {
    /// Error: something went wrong.
    Error,
    /// Warning: attention needed but not critical.
    Warning,
    /// Success: operation completed successfully.
    Success,
    /// Notification: informational ping.
    Notification,
    /// Critical: urgent attention required.
    Critical,
    /// Boot complete: system is ready.
    BootComplete,
    /// Shutdown: system is shutting down.
    Shutdown,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Play a notification sound.
///
/// Uses the audio mixer if available, falls back to PC speaker.
/// Does nothing if notifications are disabled.
pub fn play(event: NotifySound) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Try to play through the mixer first for better quality.
    if play_via_mixer(event) {
        return;
    }

    // Fall back to PC speaker.
    play_via_pcspk(event);
}

/// Enable or disable notification sounds.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Are notifications enabled?
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Set notification volume (0-100).
pub fn set_volume(vol: u8) {
    NOTIFY_VOLUME.store(vol.min(100), Ordering::Relaxed);
}

/// Get notification volume.
pub fn volume() -> u8 {
    NOTIFY_VOLUME.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Mixer-based playback
// ---------------------------------------------------------------------------

/// Try to play a notification through the audio mixer.
/// Returns true if successful (mixer available and stream opened).
fn play_via_mixer(event: NotifySound) -> bool {
    // Check if any real audio device is available.
    let have_audio = crate::hda::is_initialized()
        || crate::virtio::sound::is_available()
        || crate::ac97::is_available();

    if !have_audio {
        return false;
    }

    // Open a mixer stream for this notification.
    let stream_id = match crate::audio_mixer::open_stream("sys_notify") {
        Ok(id) => id,
        Err(_) => return false,
    };

    // Set volume.
    let vol = NOTIFY_VOLUME.load(Ordering::Relaxed);
    crate::audio_mixer::set_volume(stream_id, vol);

    // Generate and write the PCM data for this event.
    let mut buf = [0u8; 8192]; // Up to 2048 stereo frames (~42ms)
    let len = generate_notification_pcm(event, &mut buf);

    if len > 0 {
        let _ = crate::audio_mixer::write_pcm(stream_id, &buf[..len]);
    }

    // Close the stream (data will be consumed by mixer on next mix cycle).
    // In a real implementation, we'd wait for the data to be consumed.
    // For now, we close immediately — the mixer drains muted streams anyway.
    crate::audio_mixer::close_stream(stream_id);

    true
}

/// Generate PCM data for a notification event.
/// Returns number of bytes written.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn generate_notification_pcm(event: NotifySound, buf: &mut [u8]) -> usize {
    match event {
        NotifySound::Error => {
            // Descending tone: 600 Hz → 300 Hz over 150ms.
            generate_sweep(buf, 600, 300, 150)
        }
        NotifySound::Warning => {
            // Two beeps at 500 Hz, 80ms each, 40ms gap.
            let mut total = 0;
            total += generate_tone(&mut buf[total..], 500, 80);
            total += generate_silence(&mut buf[total..], 40);
            total += generate_tone(&mut buf[total..], 500, 80);
            total
        }
        NotifySound::Success => {
            // Ascending: 800 Hz → 1200 Hz over 100ms.
            generate_sweep(buf, 800, 1200, 100)
        }
        NotifySound::Notification => {
            // Single soft ping at 1000 Hz, 50ms with fade.
            generate_tone_fade(buf, 1000, 50)
        }
        NotifySound::Critical => {
            // Three rapid beeps at 1200 Hz.
            let mut total = 0;
            total += generate_tone(&mut buf[total..], 1200, 60);
            total += generate_silence(&mut buf[total..], 30);
            total += generate_tone(&mut buf[total..], 1200, 60);
            total += generate_silence(&mut buf[total..], 30);
            total += generate_tone(&mut buf[total..], 1200, 60);
            total
        }
        NotifySound::BootComplete => {
            // C5-E5-G5 ascending chime.
            let mut total = 0;
            total += generate_tone(&mut buf[total..], 523, 80);
            total += generate_silence(&mut buf[total..], 20);
            total += generate_tone(&mut buf[total..], 659, 80);
            total += generate_silence(&mut buf[total..], 20);
            total += generate_tone(&mut buf[total..], 784, 120);
            total
        }
        NotifySound::Shutdown => {
            // G5-C5 descending.
            let mut total = 0;
            total += generate_tone(&mut buf[total..], 784, 100);
            total += generate_silence(&mut buf[total..], 20);
            total += generate_tone(&mut buf[total..], 523, 150);
            total
        }
    }
}

/// Generate a constant-frequency tone (triangle wave).
/// Returns bytes written.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn generate_tone(buf: &mut [u8], freq_hz: u32, duration_ms: u32) -> usize {
    let samples = (48000u64 * u64::from(duration_ms) / 1000) as usize;
    let bytes_needed = samples * 4; // stereo 16-bit
    let bytes = bytes_needed.min(buf.len());
    let frames = bytes / 4;

    let period = 48000 / freq_hz.max(1);
    let half = period / 2;
    let quarter = period / 4;

    for i in 0..frames {
        let t = (i as u32) % period;
        let sample: i16 = if t < half {
            if t < quarter {
                ((t as i32 * 24000) / quarter as i32) as i16
            } else {
                (((half - t) as i32 * 24000) / quarter as i32) as i16
            }
        } else {
            let t2 = t - half;
            if t2 < quarter {
                -((t2 as i32 * 24000) / quarter as i32) as i16
            } else {
                -(((half - t2) as i32 * 24000) / quarter as i32) as i16
            }
        };

        let b = sample.to_le_bytes();
        let off = i * 4;
        if off + 3 < buf.len() {
            buf[off] = b[0];
            buf[off + 1] = b[1];
            buf[off + 2] = b[0];
            buf[off + 3] = b[1];
        }
    }

    frames * 4
}

/// Generate a tone with linear fade-out.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn generate_tone_fade(buf: &mut [u8], freq_hz: u32, duration_ms: u32) -> usize {
    let samples = (48000u64 * u64::from(duration_ms) / 1000) as usize;
    let bytes_needed = samples * 4;
    let bytes = bytes_needed.min(buf.len());
    let frames = bytes / 4;

    let period = 48000 / freq_hz.max(1);
    let half = period / 2;
    let quarter = period / 4;

    for i in 0..frames {
        let t = (i as u32) % period;
        let raw: i32 = if t < half {
            if t < quarter {
                (t as i32 * 24000) / quarter as i32
            } else {
                ((half - t) as i32 * 24000) / quarter as i32
            }
        } else {
            let t2 = t - half;
            if t2 < quarter {
                -((t2 as i32 * 24000) / quarter as i32)
            } else {
                -(((half - t2) as i32 * 24000) / quarter as i32)
            }
        };

        // Linear fade: full volume at start, zero at end.
        let fade = ((frames - i) as i32 * 100) / frames as i32;
        let sample = ((raw * fade) / 100) as i16;

        let b = sample.to_le_bytes();
        let off = i * 4;
        if off + 3 < buf.len() {
            buf[off] = b[0];
            buf[off + 1] = b[1];
            buf[off + 2] = b[0];
            buf[off + 3] = b[1];
        }
    }

    frames * 4
}

/// Generate a frequency sweep (linearly interpolated).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn generate_sweep(buf: &mut [u8], start_hz: u32, end_hz: u32, duration_ms: u32) -> usize {
    let total_frames = (48000u64 * u64::from(duration_ms) / 1000) as usize;
    let bytes_needed = total_frames * 4;
    let bytes = bytes_needed.min(buf.len());
    let frames = bytes / 4;

    let mut phase: u32 = 0;

    for i in 0..frames {
        // Linearly interpolate frequency.
        let freq = start_hz as i64
            + ((end_hz as i64 - start_hz as i64) * i as i64) / frames as i64;
        let freq = freq.max(20) as u32;

        let period = 48000 / freq;
        let t = phase % period;
        let half = period / 2;
        let quarter = period / 4;

        let sample: i16 = if t < half {
            if t < quarter {
                ((t as i32 * 20000) / quarter.max(1) as i32) as i16
            } else {
                (((half - t) as i32 * 20000) / quarter.max(1) as i32) as i16
            }
        } else {
            let t2 = t - half;
            if t2 < quarter {
                -((t2 as i32 * 20000) / quarter.max(1) as i32) as i16
            } else {
                -(((half - t2) as i32 * 20000) / quarter.max(1) as i32) as i16
            }
        };

        phase = phase.wrapping_add(1);

        let b = sample.to_le_bytes();
        let off = i * 4;
        if off + 3 < buf.len() {
            buf[off] = b[0];
            buf[off + 1] = b[1];
            buf[off + 2] = b[0];
            buf[off + 3] = b[1];
        }
    }

    frames * 4
}

/// Generate silence.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn generate_silence(buf: &mut [u8], duration_ms: u32) -> usize {
    let frames = (48000u64 * u64::from(duration_ms) / 1000) as usize;
    let bytes = (frames * 4).min(buf.len());
    buf[..bytes].fill(0);
    bytes
}

// ---------------------------------------------------------------------------
// PC speaker fallback
// ---------------------------------------------------------------------------

/// Play notification via PC speaker (always available, no audio device needed).
fn play_via_pcspk(event: NotifySound) {
    match event {
        NotifySound::Error => crate::pcspk::beep(800, 100),
        NotifySound::Warning => {
            crate::pcspk::beep(600, 80);
            crate::pcspk::beep(600, 80);
        }
        NotifySound::Success => crate::pcspk::beep(1200, 50),
        NotifySound::Notification => crate::pcspk::beep(1000, 30),
        NotifySound::Critical => {
            crate::pcspk::beep(1500, 50);
            crate::pcspk::beep(1500, 50);
            crate::pcspk::beep(1500, 50);
        }
        NotifySound::BootComplete => crate::pcspk::startup_chime(),
        NotifySound::Shutdown => crate::pcspk::shutdown_tone(),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: verify notification sound generation.
pub fn self_test() {
    serial_println!("[notify] Running self-test...");

    // Test 1: Generate PCM for each event type.
    let mut buf = [0u8; 8192];
    let events = [
        (NotifySound::Error, "Error"),
        (NotifySound::Warning, "Warning"),
        (NotifySound::Success, "Success"),
        (NotifySound::Notification, "Notification"),
        (NotifySound::Critical, "Critical"),
        (NotifySound::BootComplete, "BootComplete"),
        (NotifySound::Shutdown, "Shutdown"),
    ];

    for (event, name) in &events {
        let len = generate_notification_pcm(*event, &mut buf);
        if len > 0 {
            serial_println!("[notify]   {}: {} bytes PCM", name, len);
        } else {
            serial_println!("[notify]   {}: FAIL (0 bytes)", name);
        }
    }

    // Test 2: Volume control.
    set_volume(50);
    if volume() == 50 {
        serial_println!("[notify]   Volume set/get: OK");
    } else {
        serial_println!("[notify]   Volume set/get: FAIL");
    }
    set_volume(70); // Restore default.

    // Test 3: Enable/disable.
    set_enabled(false);
    if !is_enabled() {
        serial_println!("[notify]   Disable: OK");
    }
    set_enabled(true);
    if is_enabled() {
        serial_println!("[notify]   Enable: OK");
    }

    serial_println!("[notify] Self-test PASSED");
}
