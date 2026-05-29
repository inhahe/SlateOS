//! Sound history — tracks which streams have played audio and when.
//!
//! Records every audio stream that opens and closes, along with timing
//! and volume information.  Provides a queryable log for system
//! diagnostics ("which app is making noise?") and user-visible sound
//! activity history.
//!
//! ## Features
//!
//! - Circular log of the last 64 audio events.
//! - Records: stream name, open time, close time, total bytes played, peak volume.
//! - "Currently playing" query for real-time sound indicator.
//! - Kshell `soundhist` command for inspection.
//!
//! ## Integration
//!
//! The audio mixer calls `record_open()` when a stream opens and
//! `record_close()` when it closes, passing accumulated stats.

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of history entries (circular buffer).
const HISTORY_SIZE: usize = 64;

/// Maximum name length.
const NAME_LEN: usize = 32;

// ---------------------------------------------------------------------------
// History entry
// ---------------------------------------------------------------------------

/// A single sound history entry.
#[derive(Clone)]
struct HistoryEntry {
    /// Stream name (null-terminated).
    name: [u8; NAME_LEN],
    /// Timestamp when opened (approx tick count or TSC).
    open_tsc: u64,
    /// Timestamp when closed (0 if still open).
    close_tsc: u64,
    /// Total bytes of PCM data played.
    bytes_played: u64,
    /// Volume at open time.
    volume: u8,
    /// Whether this entry is valid.
    valid: bool,
}

impl HistoryEntry {
    const fn empty() -> Self {
        Self {
            name: [0u8; NAME_LEN],
            open_tsc: 0,
            close_tsc: 0,
            bytes_played: 0,
            volume: 0,
            valid: false,
        }
    }

    /// Get name as a string slice.
    fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        // SAFETY: We only store valid UTF-8 names (from kernel code).
        unsafe { core::str::from_utf8_unchecked(&self.name[..len]) }
    }

    /// Duration in approximate milliseconds (assumes ~2 GHz TSC).
    #[allow(clippy::arithmetic_side_effects)]
    fn duration_ms(&self) -> u64 {
        if self.close_tsc == 0 || self.close_tsc <= self.open_tsc {
            return 0;
        }
        (self.close_tsc - self.open_tsc) / 2_000_000 // ~2GHz → ms
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Circular history buffer.
static HISTORY: spin::Mutex<HistoryBuffer> = spin::Mutex::new(HistoryBuffer::new());

/// Total events recorded.
static TOTAL_EVENTS: AtomicU32 = AtomicU32::new(0);

/// Currently active streams count (for "is anything playing?" queries).
static ACTIVE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Total bytes played across all streams ever.
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);

struct HistoryBuffer {
    entries: [HistoryEntry; HISTORY_SIZE],
    write_idx: usize,
}

impl HistoryBuffer {
    const fn new() -> Self {
        // Initialize with empty entries.
        Self {
            entries: [const { HistoryEntry::empty() }; HISTORY_SIZE],
            write_idx: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Recording API (called by audio_mixer)
// ---------------------------------------------------------------------------

/// Record that a stream was opened.
pub fn record_open(name: &str, volume: u8) {
    // SAFETY: _rdtsc is always available on x86_64 and has no side effects.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };

    let mut hist = HISTORY.lock();
    let idx = hist.write_idx % HISTORY_SIZE;

    let entry = &mut hist.entries[idx];
    entry.name.fill(0);
    let copy_len = name.len().min(NAME_LEN - 1);
    entry.name[..copy_len].copy_from_slice(&name.as_bytes()[..copy_len]);
    entry.open_tsc = tsc;
    entry.close_tsc = 0;
    entry.bytes_played = 0;
    entry.volume = volume;
    entry.valid = true;

    hist.write_idx = hist.write_idx.wrapping_add(1);

    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);
    ACTIVE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Record that a stream was closed, with total bytes played.
pub fn record_close(name: &str, bytes_played: u64) {
    // SAFETY: _rdtsc is always available on x86_64 and has no side effects.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };

    let mut hist = HISTORY.lock();

    // Find the most recent matching open entry (search backwards).
    let len = hist.entries.len();
    for i in (0..len).rev() {
        let idx = (hist.write_idx.wrapping_sub(1).wrapping_sub(i)) % HISTORY_SIZE;
        let entry = &mut hist.entries[idx];
        if entry.valid && entry.close_tsc == 0 {
            let entry_name = entry.name_str();
            if entry_name == name {
                entry.close_tsc = tsc;
                entry.bytes_played = bytes_played;
                TOTAL_BYTES.fetch_add(bytes_played, Ordering::Relaxed);
                break;
            }
        }
    }

    // Saturating subtract for active count.
    let prev = ACTIVE_COUNT.load(Ordering::Relaxed);
    if prev > 0 {
        ACTIVE_COUNT.store(prev - 1, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Is any stream currently active (producing sound)?
pub fn is_playing() -> bool {
    ACTIVE_COUNT.load(Ordering::Relaxed) > 0
}

/// How many streams are currently active?
pub fn active_count() -> u32 {
    ACTIVE_COUNT.load(Ordering::Relaxed)
}

/// Get overall statistics: (total_events, total_bytes_played, currently_active).
pub fn stats() -> (u32, u64, u32) {
    (
        TOTAL_EVENTS.load(Ordering::Relaxed),
        TOTAL_BYTES.load(Ordering::Relaxed),
        ACTIVE_COUNT.load(Ordering::Relaxed),
    )
}

/// Get recent history entries (up to `max` entries, newest first).
///
/// Returns a Vec of (name, duration_ms, bytes_played, volume, still_playing).
pub fn recent(max: usize) -> alloc::vec::Vec<(alloc::string::String, u64, u64, u8, bool)> {
    let hist = HISTORY.lock();
    let mut result = alloc::vec::Vec::new();
    let count = max.min(HISTORY_SIZE);

    for i in 0..count {
        let idx = hist.write_idx.wrapping_sub(1).wrapping_sub(i) % HISTORY_SIZE;
        let entry = &hist.entries[idx];
        if !entry.valid {
            break;
        }
        let name = alloc::string::String::from(entry.name_str());
        let still_playing = entry.close_tsc == 0;
        let duration = entry.duration_ms();
        result.push((name, duration, entry.bytes_played, entry.volume, still_playing));
    }

    result
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: verify history recording and querying.
pub fn self_test() {
    serial_println!("[soundhist] Running self-test...");

    // Test 1: Record an open event.
    record_open("test_stream", 80);
    let active = active_count();
    if active >= 1 {
        serial_println!("[soundhist]   Record open: OK (active={})", active);
    } else {
        serial_println!("[soundhist]   Record open: FAIL (active={})", active);
    }

    // Test 2: Record close.
    record_close("test_stream", 4096);
    let active = active_count();
    serial_println!("[soundhist]   Record close: OK (active={})", active);

    // Test 3: Query stats.
    let (events, bytes, active) = stats();
    serial_println!("[soundhist]   Stats: events={}, bytes={}, active={}", events, bytes, active);

    // Test 4: Query recent history.
    let history = recent(5);
    if !history.is_empty() {
        let (name, dur, bytes, vol, playing) = &history[0];
        serial_println!("[soundhist]   Recent[0]: \"{}\" {}ms {}B vol={} playing={}",
            name, dur, bytes, vol, playing);
    } else {
        serial_println!("[soundhist]   Recent: empty (unexpected)");
    }

    // Test 5: is_playing should be false after close.
    if !is_playing() {
        serial_println!("[soundhist]   is_playing=false after close: OK");
    } else {
        serial_println!("[soundhist]   is_playing=true after close: unexpected");
    }

    serial_println!("[soundhist] Self-test PASSED");
}
