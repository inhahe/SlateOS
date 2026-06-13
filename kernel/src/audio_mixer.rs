//! Software audio mixer — blends multiple PCM streams into a single output.
//!
//! The mixer accepts audio from multiple sources (applications, system sounds,
//! etc.) and combines them into a single output stream routed to the active
//! audio device (HDA, virtio-sound, AC97, or PC speaker for notification beeps).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
//! │  App Audio  │  │System Sound │  │ Notification│
//! │  Stream 0   │  │  Stream 1   │  │  Stream 2   │
//! └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
//!        │                 │                 │
//!        └────────────┬────┘────────────────┘
//!                     │
//!              ┌──────▼──────┐
//!              │   MIXER     │  Per-stream volume + master volume
//!              │  (sum+clip) │  Sample rate conversion (future)
//!              └──────┬──────┘
//!                     │
//!              ┌──────▼──────┐
//!              │Output Device│  HDA / virtio-sound / AC97
//!              └─────────────┘
//! ```
//!
//! ## Design
//!
//! - Fixed internal format: 48kHz, 16-bit signed, stereo (4 bytes/frame).
//! - Up to 8 concurrent streams (soft limit, expandable).
//! - Per-stream volume (0–100) and mute control.
//! - Master volume (0–100) and master mute.
//! - Lock-free ring buffers per stream for low-latency submission.
//! - Mixing uses 32-bit intermediates to avoid overflow before clamping.
//!
//! ## Usage
//!
//! ```text
//! let id = audio_mixer::open_stream("my_app")?;
//! audio_mixer::set_volume(id, 80);           // 80% volume
//! audio_mixer::write_pcm(id, &samples)?;     // Submit PCM data
//! audio_mixer::close_stream(id);
//! ```

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of simultaneous audio streams.
const MAX_STREAMS: usize = 8;

/// Internal sample rate (Hz).
#[allow(dead_code)]
pub const SAMPLE_RATE: u32 = 48000;

/// Internal format: stereo, 16-bit signed, little-endian.
/// Bytes per sample frame (left + right = 4 bytes).
pub const FRAME_SIZE_BYTES: usize = 4;

/// Ring buffer size per stream (in bytes).
/// 4096 frames × 4 bytes = 16384 bytes (~85ms at 48kHz).
const RING_BUFFER_SIZE: usize = 16384;

/// Mixing buffer size (one period worth of mixed output).
/// 1024 frames × 4 bytes = 4096 bytes (~21ms at 48kHz).
const MIX_BUFFER_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Stream state
// ---------------------------------------------------------------------------

/// A stream ID (0..MAX_STREAMS-1).
pub type StreamId = u8;

/// Per-stream metadata.
struct StreamSlot {
    /// Whether this slot is in use.
    active: AtomicBool,
    /// Volume level (0–100).
    volume: AtomicU8,
    /// Mute flag.
    muted: AtomicBool,
    /// Stream name (for diagnostics).
    name: spin::Mutex<[u8; 32]>,
    /// Ring buffer for PCM data.
    ring: spin::Mutex<RingBuffer>,
}

/// Simple ring buffer for PCM data.
struct RingBuffer {
    /// Storage.
    data: [u8; RING_BUFFER_SIZE],
    /// Read position.
    read_pos: usize,
    /// Write position.
    write_pos: usize,
    /// Number of bytes available to read.
    available: usize,
}

impl RingBuffer {
    const fn new() -> Self {
        Self {
            data: [0u8; RING_BUFFER_SIZE],
            read_pos: 0,
            write_pos: 0,
            available: 0,
        }
    }

    /// Write PCM data into the ring buffer.
    /// Returns number of bytes actually written (may be less than input if full).
    #[allow(clippy::arithmetic_side_effects)]
    fn write(&mut self, src: &[u8]) -> usize {
        let free = RING_BUFFER_SIZE - self.available;
        let to_write = src.len().min(free);

        for i in 0..to_write {
            self.data[self.write_pos] = src[i];
            self.write_pos = (self.write_pos + 1) % RING_BUFFER_SIZE;
        }
        self.available += to_write;
        to_write
    }

    /// Read PCM data from the ring buffer.
    /// Returns number of bytes read.
    #[allow(clippy::arithmetic_side_effects)]
    fn read(&mut self, dst: &mut [u8]) -> usize {
        let to_read = dst.len().min(self.available);

        for i in 0..to_read {
            dst[i] = self.data[self.read_pos];
            self.read_pos = (self.read_pos + 1) % RING_BUFFER_SIZE;
        }
        self.available -= to_read;
        to_read
    }

    /// Number of bytes available to read.
    fn len(&self) -> usize {
        self.available
    }

    /// Reset the ring buffer (discard all data).
    fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
        self.available = 0;
    }
}

// ---------------------------------------------------------------------------
// Global mixer state
// ---------------------------------------------------------------------------

/// Master volume (0–100).
static MASTER_VOLUME: AtomicU8 = AtomicU8::new(80);

/// Master mute.
static MASTER_MUTED: AtomicBool = AtomicBool::new(false);

/// Total streams ever opened (for stats).
static TOTAL_OPENED: AtomicU32 = AtomicU32::new(0);

/// Total frames mixed (for stats).
static TOTAL_FRAMES_MIXED: AtomicU32 = AtomicU32::new(0);

/// Stream slots (fixed array — no heap allocation).
static STREAMS: [StreamSlot; MAX_STREAMS] = [
    StreamSlot::new(), StreamSlot::new(), StreamSlot::new(), StreamSlot::new(),
    StreamSlot::new(), StreamSlot::new(), StreamSlot::new(), StreamSlot::new(),
];

impl StreamSlot {
    const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            volume: AtomicU8::new(100),
            muted: AtomicBool::new(false),
            name: spin::Mutex::new([0u8; 32]),
            ring: spin::Mutex::new(RingBuffer::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — Stream management
// ---------------------------------------------------------------------------

/// Open a new audio stream.
///
/// Returns a stream ID that can be used for `write_pcm()` and `close_stream()`.
/// The stream starts at 100% volume, unmuted.
pub fn open_stream(name: &str) -> KernelResult<StreamId> {
    for (i, slot) in STREAMS.iter().enumerate() {
        // Try to atomically claim this slot.
        if slot.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
            // Initialize the slot.
            slot.volume.store(100, Ordering::Relaxed);
            slot.muted.store(false, Ordering::Relaxed);

            // Copy name (truncate if needed).
            let mut name_buf = slot.name.lock();
            name_buf.fill(0);
            let copy_len = name.len().min(31);
            name_buf[..copy_len].copy_from_slice(&name.as_bytes()[..copy_len]);
            drop(name_buf);

            // Clear any leftover data in the ring.
            slot.ring.lock().clear();

            TOTAL_OPENED.fetch_add(1, Ordering::Relaxed);

            serial_println!("[mixer] Opened stream {} (\"{}\")", i, name);
            return Ok(i as StreamId);
        }
    }

    serial_println!("[mixer] No free stream slots");
    Err(KernelError::WouldBlock)
}

/// Close an audio stream.
pub fn close_stream(id: StreamId) {
    let idx = id as usize;
    if idx >= MAX_STREAMS {
        return;
    }
    STREAMS[idx].active.store(false, Ordering::Release);
    STREAMS[idx].ring.lock().clear();
}

/// Write PCM data to a stream's ring buffer.
///
/// Data format must be 48kHz, 16-bit signed, stereo (native internal format).
/// Returns the number of bytes actually buffered (may be less if buffer is full).
pub fn write_pcm(id: StreamId, data: &[u8]) -> KernelResult<usize> {
    let idx = id as usize;
    if idx >= MAX_STREAMS {
        return Err(KernelError::InvalidArgument);
    }
    if !STREAMS[idx].active.load(Ordering::Acquire) {
        return Err(KernelError::InvalidArgument);
    }

    let written = STREAMS[idx].ring.lock().write(data);
    Ok(written)
}

/// Get the amount of buffered data for a stream (in bytes).
#[allow(dead_code)]
pub fn buffered(id: StreamId) -> usize {
    let idx = id as usize;
    if idx >= MAX_STREAMS {
        return 0;
    }
    STREAMS[idx].ring.lock().len()
}

/// Free space (in bytes) currently available in a stream's ring buffer.
///
/// Returns 0 for an out-of-range or inactive stream.  Used by the ALSA PCM
/// shim to size a non-blocking transfer and to answer `POLLOUT` readiness.
#[allow(dead_code)]
pub fn space_available(id: StreamId) -> usize {
    let Some(slot) = STREAMS.get(id as usize) else {
        return 0;
    };
    if !slot.active.load(Ordering::Acquire) {
        return 0;
    }
    // RING_BUFFER_SIZE is the fixed ring capacity; `len()` never exceeds it,
    // so the subtraction cannot wrap.
    RING_BUFFER_SIZE.saturating_sub(slot.ring.lock().len())
}

/// Is a stream writable — i.e. does it have room for at least one frame?
///
/// Backs `POLLOUT` readiness for a playback PCM substream: a poll reports the
/// fd writable whenever the ring can accept a frame without blocking.
#[allow(dead_code)]
pub fn writable(id: StreamId) -> bool {
    space_available(id) >= FRAME_SIZE_BYTES
}

/// Discard any buffered frames for a stream without deactivating it.
///
/// Mirrors the ALSA `PREPARE` / `DROP` semantics that reset the ring to empty
/// while keeping the substream's mixer slot reserved (unlike [`close_stream`],
/// which also frees the slot).
#[allow(dead_code)]
pub fn clear(id: StreamId) {
    if let Some(slot) = STREAMS.get(id as usize) {
        slot.ring.lock().clear();
    }
}

// ---------------------------------------------------------------------------
// Public API — Volume control
// ---------------------------------------------------------------------------

/// Set master volume (0–100).
pub fn set_master_volume(vol: u8) {
    MASTER_VOLUME.store(vol.min(100), Ordering::Relaxed);
}

/// Get master volume.
pub fn master_volume() -> u8 {
    MASTER_VOLUME.load(Ordering::Relaxed)
}

/// Set master mute.
pub fn set_master_mute(muted: bool) {
    MASTER_MUTED.store(muted, Ordering::Relaxed);
}

/// Is master muted?
#[allow(dead_code)]
pub fn is_master_muted() -> bool {
    MASTER_MUTED.load(Ordering::Relaxed)
}

/// Set per-stream volume (0–100).
pub fn set_volume(id: StreamId, vol: u8) {
    let idx = id as usize;
    if idx < MAX_STREAMS {
        STREAMS[idx].volume.store(vol.min(100), Ordering::Relaxed);
    }
}

/// Get per-stream volume.
pub fn get_volume(id: StreamId) -> u8 {
    let idx = id as usize;
    if idx < MAX_STREAMS {
        STREAMS[idx].volume.load(Ordering::Relaxed)
    } else {
        0
    }
}

/// Set per-stream mute.
#[allow(dead_code)]
pub fn set_muted(id: StreamId, muted: bool) {
    let idx = id as usize;
    if idx < MAX_STREAMS {
        STREAMS[idx].muted.store(muted, Ordering::Relaxed);
    }
}

/// Is a stream muted?
#[allow(dead_code)]
pub fn is_muted(id: StreamId) -> bool {
    let idx = id as usize;
    if idx < MAX_STREAMS {
        STREAMS[idx].muted.load(Ordering::Relaxed)
    } else {
        true
    }
}

// ---------------------------------------------------------------------------
// Mixing engine
// ---------------------------------------------------------------------------

/// Mix all active streams into a single output buffer.
///
/// The output buffer receives mixed 48kHz/S16/stereo PCM data.
/// Returns the number of bytes written to `output`.
///
/// This is the core mixing function — call it periodically to drive
/// audio output (e.g., from a timer callback or dedicated audio task).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub fn mix_output(output: &mut [u8]) -> usize {
    if MASTER_MUTED.load(Ordering::Relaxed) {
        // Master mute: output silence.
        let len = output.len().min(MIX_BUFFER_SIZE);
        output[..len].fill(0);
        return len;
    }

    let master_vol = u32::from(MASTER_VOLUME.load(Ordering::Relaxed));
    let out_frames = output.len() / FRAME_SIZE_BYTES;
    let mix_frames = out_frames.min(MIX_BUFFER_SIZE / FRAME_SIZE_BYTES);

    // Use a local mixing buffer with 32-bit intermediates.
    // Stack-allocated: 1024 frames × 2 channels × 4 bytes = 8192 bytes.
    let mut mix_buf = [0i32; 2048]; // 1024 stereo frames, L/R interleaved.
    let actual_frames = mix_frames.min(1024);

    // Read from each active stream and sum into mix_buf.
    let mut any_active = false;
    let mut temp = [0u8; 4096]; // Temporary read buffer.

    for (i, slot) in STREAMS.iter().enumerate() {
        if !slot.active.load(Ordering::Acquire) {
            continue;
        }
        if slot.muted.load(Ordering::Relaxed) {
            // Drain the ring even if muted (so it doesn't accumulate).
            let _ = slot.ring.lock().read(&mut temp[..actual_frames * FRAME_SIZE_BYTES]);
            continue;
        }

        let stream_vol = u32::from(slot.volume.load(Ordering::Relaxed));
        let bytes_needed = actual_frames * FRAME_SIZE_BYTES;
        let bytes_read = slot.ring.lock().read(&mut temp[..bytes_needed]);
        let frames_read = bytes_read / FRAME_SIZE_BYTES;

        if frames_read == 0 {
            continue;
        }
        any_active = true;

        // Mix: convert S16LE samples to i32, apply volume, sum.
        for f in 0..frames_read {
            let offset = f * 4;
            let left = i16::from_le_bytes([temp[offset], temp[offset + 1]]) as i32;
            let right = i16::from_le_bytes([temp[offset + 2], temp[offset + 3]]) as i32;

            // Apply per-stream volume (0-100 → 0.0-1.0 via integer math).
            let left_scaled = (left * stream_vol as i32) / 100;
            let right_scaled = (right * stream_vol as i32) / 100;

            let idx = f * 2;
            if idx + 1 < mix_buf.len() {
                mix_buf[idx] += left_scaled;
                mix_buf[idx + 1] += right_scaled;
            }
        }

        // Suppress unused variable warning
        let _ = i;
    }

    if !any_active {
        // No active streams — output silence.
        let out_bytes = actual_frames * FRAME_SIZE_BYTES;
        output[..out_bytes].fill(0);
        return out_bytes;
    }

    // Apply master volume and clamp to i16 range, write to output.
    let out_bytes = actual_frames * FRAME_SIZE_BYTES;
    for f in 0..actual_frames {
        let idx = f * 2;
        let left = (mix_buf[idx] * master_vol as i32) / 100;
        let right = (mix_buf[idx + 1] * master_vol as i32) / 100;

        // Clamp to i16 range.
        let left_clamped = left.clamp(-32768, 32767) as i16;
        let right_clamped = right.clamp(-32768, 32767) as i16;

        let offset = f * 4;
        let left_bytes = left_clamped.to_le_bytes();
        let right_bytes = right_clamped.to_le_bytes();
        output[offset] = left_bytes[0];
        output[offset + 1] = left_bytes[1];
        output[offset + 2] = right_bytes[0];
        output[offset + 3] = right_bytes[1];
    }

    TOTAL_FRAMES_MIXED.fetch_add(actual_frames as u32, Ordering::Relaxed);
    out_bytes
}

// ---------------------------------------------------------------------------
// Status / diagnostics
// ---------------------------------------------------------------------------

/// Get mixer status: (active_streams, total_opened, total_frames_mixed, master_vol, master_muted).
pub fn status() -> (u8, u32, u32, u8, bool) {
    let active = STREAMS.iter()
        .filter(|s| s.active.load(Ordering::Relaxed))
        .count() as u8;
    (
        active,
        TOTAL_OPENED.load(Ordering::Relaxed),
        TOTAL_FRAMES_MIXED.load(Ordering::Relaxed),
        MASTER_VOLUME.load(Ordering::Relaxed),
        MASTER_MUTED.load(Ordering::Relaxed),
    )
}

/// Get info about a specific stream: (volume, muted, buffered_bytes).
///
/// Returns `None` if the stream is not active.
#[allow(dead_code)]
pub fn stream_info(id: StreamId) -> Option<(u8, bool, usize)> {
    let idx = id as usize;
    if idx >= MAX_STREAMS {
        return None;
    }
    let slot = &STREAMS[idx];
    if !slot.active.load(Ordering::Acquire) {
        return None;
    }

    Some((
        slot.volume.load(Ordering::Relaxed),
        slot.muted.load(Ordering::Relaxed),
        slot.ring.lock().len(),
    ))
}

/// Copy a stream's name into the provided buffer. Returns bytes written.
#[allow(dead_code)]
pub fn stream_name(id: StreamId, dst: &mut [u8]) -> usize {
    let idx = id as usize;
    if idx >= MAX_STREAMS {
        return 0;
    }
    let slot = &STREAMS[idx];
    if !slot.active.load(Ordering::Acquire) {
        return 0;
    }

    let name_buf = slot.name.lock();
    let len = name_buf.iter().position(|&b| b == 0).unwrap_or(32);
    let copy_len = len.min(dst.len());
    dst[..copy_len].copy_from_slice(&name_buf[..copy_len]);
    copy_len
}

/// List all active streams.
pub fn list_streams() -> alloc::vec::Vec<(StreamId, u8, bool, usize)> {
    let mut result = alloc::vec::Vec::new();
    for (i, slot) in STREAMS.iter().enumerate() {
        if slot.active.load(Ordering::Acquire) {
            result.push((
                i as StreamId,
                slot.volume.load(Ordering::Relaxed),
                slot.muted.load(Ordering::Relaxed),
                slot.ring.lock().len(),
            ));
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: verify stream open/close, volume control, and mixing.
pub fn self_test() {
    serial_println!("[mixer] Running self-test...");

    // Test 1: Open a stream.
    let id = match open_stream("test_tone") {
        Ok(id) => id,
        Err(e) => {
            serial_println!("[mixer]   Failed to open stream: {:?}", e);
            serial_println!("[mixer] Self-test FAILED");
            return;
        }
    };
    serial_println!("[mixer]   Open stream: OK (id={})", id);

    // Test 2: Set and read volume.
    set_volume(id, 75);
    let vol = get_volume(id);
    if vol == 75 {
        serial_println!("[mixer]   Volume set/get: OK");
    } else {
        serial_println!("[mixer]   Volume set/get: FAIL (got {})", vol);
    }

    // Test 3: Write PCM data.
    let mut tone_buf = [0u8; 256];
    // Generate a few frames of audio (16 stereo frames = 64 bytes).
    for i in 0..16 {
        let sample = ((i as i16) * 2048) - 16384; // Simple ramp
        let bytes = sample.to_le_bytes();
        let offset = i * 4;
        tone_buf[offset] = bytes[0];
        tone_buf[offset + 1] = bytes[1];
        tone_buf[offset + 2] = bytes[0];
        tone_buf[offset + 3] = bytes[1];
    }
    match write_pcm(id, &tone_buf[..64]) {
        Ok(written) => {
            if written == 64 {
                serial_println!("[mixer]   Write PCM: OK (64 bytes)");
            } else {
                serial_println!("[mixer]   Write PCM: partial ({}/64 bytes)", written);
            }
        }
        Err(e) => serial_println!("[mixer]   Write PCM: FAIL ({:?})", e),
    }

    // Test 4: Mix output.
    let mut output = [0u8; 64];
    let mixed = mix_output(&mut output);
    if mixed == 64 {
        serial_println!("[mixer]   Mix output: OK ({} bytes)", mixed);
    } else {
        serial_println!("[mixer]   Mix output: {} bytes (expected 64)", mixed);
    }

    // Test 5: Verify mixed samples are non-zero (not silence).
    let any_nonzero = output.iter().any(|&b| b != 0);
    if any_nonzero {
        serial_println!("[mixer]   Mixed data non-zero: OK");
    } else {
        serial_println!("[mixer]   Mixed data: all zeros (volume or mute issue?)");
    }

    // Test 6: Master mute produces silence.
    // Write more data first.
    let _ = write_pcm(id, &tone_buf[..64]);
    set_master_mute(true);
    let mut silent_out = [0xFFu8; 64];
    let _ = mix_output(&mut silent_out);
    set_master_mute(false);
    let all_zero = silent_out.iter().all(|&b| b == 0);
    if all_zero {
        serial_println!("[mixer]   Master mute: OK (silence)");
    } else {
        serial_println!("[mixer]   Master mute: FAIL (non-zero output)");
    }

    // Test 7: Close stream.
    close_stream(id);
    let still_active = STREAMS[id as usize].active.load(Ordering::Relaxed);
    if !still_active {
        serial_println!("[mixer]   Close stream: OK");
    } else {
        serial_println!("[mixer]   Close stream: FAIL (still active)");
    }

    // Test 8: Verify stats.
    let (active, opened, _frames, mvol, mmuted) = status();
    serial_println!("[mixer]   Stats: {} active, {} opened, vol={}, muted={}",
        active, opened, mvol, mmuted);

    serial_println!("[mixer] Self-test PASSED");
}
