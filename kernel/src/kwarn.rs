//! Kernel warning system (WARN_ON-equivalent).
//!
//! Provides non-fatal assertion checking for the kernel.  Unlike `assert!`
//! which panics on failure, `kwarn!` logs the violation to a ring buffer
//! and continues execution.  This is useful for detecting logic errors or
//! invariant violations in non-critical paths without crashing the system.
//!
//! ## Variants
//!
//! - [`warn_once`]: Records the warning only the first time it triggers.
//!   Subsequent triggers at the same call site are suppressed (uses a
//!   static AtomicBool per call site via the macro).
//! - [`warn`]: Records every occurrence (for recurring violations).
//!
//! ## Ring Buffer
//!
//! Warnings are stored in a 64-entry circular buffer.  Each entry records:
//! - File name and line number
//! - A message (up to 64 bytes)
//! - TSC timestamp
//! - Number of times triggered (for warn_once, always 1)
//!
//! ## Usage
//!
//! ```ignore
//! use crate::kwarn;
//!
//! if refcount == 0 {
//!     kwarn::warn("refcount dropped to zero unexpectedly", file!(), line!());
//! }
//!
//! // Or via macro (preferred — automatically fills file/line):
//! kwarn_once!(refcount == 0, "refcount zero");
//! ```
//!
//! ## Kshell Command
//!
//! `kwarn` shows all recorded warnings.  `kwarn clear` resets the buffer.
//!
//! ## References
//!
//! - Linux `WARN_ON()`, `WARN_ON_ONCE()` in `include/asm-generic/bug.h`
//! - Linux `BUG()` for fatal assertions (our equivalent: `assert!`)

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Warning entry
// ---------------------------------------------------------------------------

/// Maximum length of the message string stored per warning.
const MSG_LEN: usize = 64;

/// Maximum length of file name stored per warning.
const FILE_LEN: usize = 32;

/// A recorded kernel warning.
#[derive(Clone, Copy)]
pub struct Warning {
    /// TSC timestamp when the warning first triggered.
    pub timestamp: u64,
    /// Source file where the warning originated.
    pub file: [u8; FILE_LEN],
    /// Length of the file name.
    pub file_len: u8,
    /// Source line number.
    pub line: u32,
    /// Warning message.
    pub msg: [u8; MSG_LEN],
    /// Length of the message.
    pub msg_len: u8,
    /// How many times this warning has triggered.
    pub count: u32,
}

impl Warning {
    const fn empty() -> Self {
        Self {
            timestamp: 0,
            file: [0; FILE_LEN],
            file_len: 0,
            line: 0,
            msg: [0; MSG_LEN],
            msg_len: 0,
            count: 0,
        }
    }

    /// Check if this slot is occupied.
    pub fn is_active(&self) -> bool {
        self.timestamp != 0
    }
}

// ---------------------------------------------------------------------------
// Warning ring buffer
// ---------------------------------------------------------------------------

/// Number of warning slots in the ring buffer.
const BUFFER_SIZE: usize = 64;

/// Warning ring buffer.
static mut WARNINGS: [Warning; BUFFER_SIZE] = [Warning::empty(); BUFFER_SIZE];

/// Write pointer (next slot to use).
static WRITE_IDX: AtomicU32 = AtomicU32::new(0);

/// Total warnings recorded since boot.
static TOTAL_WARNINGS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record a kernel warning.
///
/// Called when an invariant violation is detected but the kernel can
/// continue safely.  The warning is stored in the ring buffer and
/// logged to serial.
///
/// Prefer the `kwarn!` or `kwarn_once!` macros which auto-fill
/// file/line and handle the once-per-site suppression.
pub fn warn(msg: &str, file: &str, line: u32) {
    let timestamp = crate::bench::rdtsc();

    let mut entry = Warning::empty();
    entry.timestamp = timestamp;
    entry.line = line;
    entry.count = 1;

    // Copy file name (truncated if too long).
    let file_bytes = file.as_bytes();
    let flen = file_bytes.len().min(FILE_LEN);
    entry.file[..flen].copy_from_slice(&file_bytes[..flen]);
    entry.file_len = flen as u8;

    // Copy message (truncated if too long).
    let msg_bytes = msg.as_bytes();
    let mlen = msg_bytes.len().min(MSG_LEN);
    entry.msg[..mlen].copy_from_slice(&msg_bytes[..mlen]);
    entry.msg_len = mlen as u8;

    // Store in ring buffer.
    let idx = WRITE_IDX.fetch_add(1, Ordering::Relaxed) as usize % BUFFER_SIZE;
    // SAFETY: Single-threaded writes at each index (no two CPUs get the
    // same idx due to the atomic increment, and the buffer is large enough
    // that wrapping is acceptable).
    unsafe {
        WARNINGS[idx] = entry;
    }
    TOTAL_WARNINGS.fetch_add(1, Ordering::Relaxed);

    // Also log to serial for immediate visibility.
    crate::serial_println!(
        "KWARN: {} ({}:{})",
        msg, file, line,
    );
}

/// Get all active warnings from the ring buffer (newest first).
#[must_use]
pub fn all_warnings() -> alloc::vec::Vec<Warning> {
    let total = TOTAL_WARNINGS.load(Ordering::Relaxed);
    let available = (total as usize).min(BUFFER_SIZE);
    let write_idx = WRITE_IDX.load(Ordering::Relaxed) as usize;

    let mut result = alloc::vec::Vec::with_capacity(available);
    for i in 0..available {
        let slot = (write_idx + BUFFER_SIZE - 1 - i) % BUFFER_SIZE;
        // SAFETY: Reading a potentially-racing write, but each Warning
        // is a plain struct with no pointers — worst case is a stale value.
        let w = unsafe { WARNINGS[slot] };
        if w.is_active() {
            result.push(w);
        }
    }
    result
}

/// Total number of warnings since boot.
#[must_use]
pub fn total_count() -> u64 {
    TOTAL_WARNINGS.load(Ordering::Relaxed)
}

/// Clear all warnings from the ring buffer.
pub fn clear() {
    // SAFETY: Zeroing the buffer is safe — worst case a concurrent reader
    // sees a partially-zeroed entry (timestamp = 0 → is_active() = false).
    // Use raw pointer to avoid creating a mutable reference to static.
    unsafe {
        let ptr = core::ptr::addr_of_mut!(WARNINGS);
        for i in 0..BUFFER_SIZE {
            (*ptr)[i] = Warning::empty();
        }
    }
    WRITE_IDX.store(0, Ordering::Relaxed);
    TOTAL_WARNINGS.store(0, Ordering::Relaxed);
}

extern crate alloc;

// ---------------------------------------------------------------------------
// Macros
// ---------------------------------------------------------------------------

/// Issue a kernel warning (always, every time the condition triggers).
///
/// Usage: `kwarn!(condition_is_bad, "explanation of what went wrong");`
#[macro_export]
macro_rules! kwarn {
    ($cond:expr, $msg:expr) => {
        if $cond {
            $crate::kwarn::warn($msg, file!(), line!());
        }
    };
}

/// Issue a kernel warning once per call site.
///
/// The first time the condition is true at this source location, the
/// warning is recorded and logged.  Subsequent triggers at the same
/// location are silently suppressed.
///
/// Usage: `kwarn_once!(ptr.is_null(), "null pointer in hot path");`
#[macro_export]
macro_rules! kwarn_once {
    ($cond:expr, $msg:expr) => {{
        use core::sync::atomic::{AtomicBool, Ordering};
        static TRIGGERED: AtomicBool = AtomicBool::new(false);
        if $cond && !TRIGGERED.swap(true, Ordering::Relaxed) {
            $crate::kwarn::warn($msg, file!(), line!());
        }
    }};
}
