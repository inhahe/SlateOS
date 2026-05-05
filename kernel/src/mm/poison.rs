//! Kernel memory poisoning — detect use-after-free and uninitialized reads.
//!
//! When enabled, freed memory is filled with a recognizable poison pattern.
//! If code reads a poisoned value, it's accessing freed memory (use-after-free).
//! Similarly, newly allocated memory is filled with a different pattern to
//! detect reads of uninitialized memory.
//!
//! ## Patterns
//!
//! | State | Pattern | Description |
//! |-------|---------|-------------|
//! | Freed | `0xDE` repeated | "DEad" memory — any access is a bug |
//! | Uninit | `0xCD` repeated | "Clean/Dirty" — allocated but not written |
//! | Red zone | `0xFD` repeated | Guard region around allocations |
//!
//! ## Detection
//!
//! Poisoning doesn't catch bugs immediately (unlike hardware watchpoints).
//! Instead, it makes bugs more reproducible and diagnosable:
//! - A crash with `0xDEDEDEDE` in a register → use-after-free.
//! - A crash with `0xCDCDCDCD` → uninitialized read.
//! - A crash with `0xFDFDFDFD` → buffer overflow into red zone.
//!
//! ## Performance
//!
//! Poisoning adds a memset on every alloc/free.  It's enabled by default
//! in debug builds and disabled in release.  Can be toggled at runtime via
//! the `enabled` flag.
//!
//! ## References
//!
//! - Linux SLUB_DEBUG: `mm/slub.c` poison/redzone support
//! - Windows Debug Heap: 0xFD guard bytes, 0xCD init fill, 0xDD free fill
//! - Electric Fence / DUMA: guard page-based overflow detection

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Poison patterns
// ---------------------------------------------------------------------------

/// Pattern for freed memory.  If you see 0xDE in a dereferenced pointer
/// or register value, the code is accessing freed memory.
pub const POISON_FREE: u8 = 0xDE;

/// Pattern for newly allocated (uninitialized) memory.  If you see 0xCD
/// in data that should have been written, the code is reading uninitialized memory.
pub const POISON_ALLOC: u8 = 0xCD;

/// Pattern for red zone (guard) bytes around allocations.  If you see 0xFD
/// being overwritten, a buffer overflow or underflow has occurred.
pub const POISON_REDZONE: u8 = 0xFD;

/// Pattern for freed kernel stack space.  Distinguishable from heap poison.
pub const POISON_STACK: u8 = 0x6B;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Whether memory poisoning is currently enabled.
/// Can be toggled at runtime via sysctl or kshell.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Whether to poison on allocation (fill with POISON_ALLOC).
/// Separate from free-poison because alloc-poison is slightly less useful
/// (the zero_on_alloc sysctl already covers most use cases).
static ALLOC_POISON: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total bytes poisoned on free.
static FREE_POISON_BYTES: AtomicU64 = AtomicU64::new(0);

/// Total bytes poisoned on alloc.
static ALLOC_POISON_BYTES: AtomicU64 = AtomicU64::new(0);

/// Total poison violations detected (freed memory still has non-poison values
/// when re-checked).
static VIOLATIONS_DETECTED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check if memory poisoning is enabled.
#[inline]
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Enable memory poisoning.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Disable memory poisoning.
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// Set whether alloc-time poisoning is active.
pub fn set_alloc_poison(enabled: bool) {
    ALLOC_POISON.store(enabled, Ordering::Relaxed);
}

/// Poison a memory region on free.
///
/// Fills `ptr[0..len]` with `POISON_FREE`.  The caller is responsible for
/// ensuring the pointer and length are valid (the memory is about to be freed,
/// so it must still be mapped and accessible).
///
/// # Safety
///
/// `ptr` must be a valid pointer to at least `len` bytes of writable memory.
#[inline]
pub unsafe fn poison_free(ptr: *mut u8, len: usize) {
    if !is_enabled() || len == 0 {
        return;
    }
    // SAFETY: Caller guarantees ptr is valid for len bytes.
    unsafe {
        core::ptr::write_bytes(ptr, POISON_FREE, len);
    }
    FREE_POISON_BYTES.fetch_add(len as u64, Ordering::Relaxed);
}

/// Poison a memory region on allocation (fill with POISON_ALLOC).
///
/// Only active if both `ENABLED` and `ALLOC_POISON` are set.
///
/// # Safety
///
/// `ptr` must be a valid pointer to at least `len` bytes of writable memory.
#[inline]
pub unsafe fn poison_alloc(ptr: *mut u8, len: usize) {
    if !is_enabled() || !ALLOC_POISON.load(Ordering::Relaxed) || len == 0 {
        return;
    }
    // SAFETY: Caller guarantees ptr is valid for len bytes.
    unsafe {
        core::ptr::write_bytes(ptr, POISON_ALLOC, len);
    }
    ALLOC_POISON_BYTES.fetch_add(len as u64, Ordering::Relaxed);
}

/// Fill a red zone region with the guard pattern.
///
/// # Safety
///
/// `ptr` must be a valid pointer to at least `len` bytes of writable memory.
#[inline]
pub unsafe fn poison_redzone(ptr: *mut u8, len: usize) {
    if !is_enabled() || len == 0 {
        return;
    }
    // SAFETY: Caller guarantees validity.
    unsafe {
        core::ptr::write_bytes(ptr, POISON_REDZONE, len);
    }
}

/// Verify that a red zone is intact (all bytes == POISON_REDZONE).
///
/// Returns `true` if intact, `false` if corrupted (buffer overflow detected).
///
/// # Safety
///
/// `ptr` must be a valid pointer to at least `len` bytes of readable memory.
#[must_use]
pub unsafe fn verify_redzone(ptr: *const u8, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    for i in 0..len {
        // SAFETY: Caller guarantees ptr+i is valid.
        let byte = unsafe { *ptr.add(i) };
        if byte != POISON_REDZONE {
            VIOLATIONS_DETECTED.fetch_add(1, Ordering::Relaxed);
            return false;
        }
    }
    true
}

/// Verify that freed memory is still poisoned (not overwritten).
///
/// Returns `true` if all bytes are `POISON_FREE`, `false` if any byte
/// has been modified (indicating a write-after-free).
///
/// # Safety
///
/// `ptr` must be a valid pointer to at least `len` bytes of readable memory.
#[must_use]
pub unsafe fn verify_freed(ptr: *const u8, len: usize) -> bool {
    if !is_enabled() || len == 0 {
        return true;
    }
    for i in 0..len {
        // SAFETY: Caller guarantees ptr+i is valid.
        let byte = unsafe { *ptr.add(i) };
        if byte != POISON_FREE {
            VIOLATIONS_DETECTED.fetch_add(1, Ordering::Relaxed);
            return false;
        }
    }
    true
}

/// Check if a value looks like it came from poisoned memory.
///
/// Returns `Some(pattern)` if the value matches a known poison pattern,
/// `None` otherwise.  Useful for diagnostic messages in crash handlers.
#[must_use]
pub fn identify_poison(value: u64) -> Option<&'static str> {
    let bytes = value.to_le_bytes();

    // Check if all bytes match a single poison pattern.
    if bytes.iter().all(|&b| b == POISON_FREE) {
        return Some("freed memory (use-after-free)");
    }
    if bytes.iter().all(|&b| b == POISON_ALLOC) {
        return Some("uninitialized memory");
    }
    if bytes.iter().all(|&b| b == POISON_REDZONE) {
        return Some("red zone (buffer overflow)");
    }
    if bytes.iter().all(|&b| b == POISON_STACK) {
        return Some("freed stack space");
    }

    // Partial match (e.g., 0xDEDE_DEDE_xxxx_xxxx).
    let de_count = bytes.iter().filter(|&&b| b == POISON_FREE).count();
    if de_count >= 6 {
        return Some("likely freed memory (partial poison)");
    }
    let cd_count = bytes.iter().filter(|&&b| b == POISON_ALLOC).count();
    if cd_count >= 6 {
        return Some("likely uninitialized memory (partial poison)");
    }

    None
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Memory poison subsystem statistics.
#[derive(Debug, Clone, Copy)]
pub struct PoisonStats {
    /// Whether poisoning is currently enabled.
    pub enabled: bool,
    /// Whether alloc-time poisoning is active.
    pub alloc_poison: bool,
    /// Total bytes poisoned on free.
    pub free_bytes: u64,
    /// Total bytes poisoned on alloc.
    pub alloc_bytes: u64,
    /// Number of violations detected (corruption / use-after-free).
    pub violations: u64,
}

/// Get poison statistics.
#[must_use]
pub fn stats() -> PoisonStats {
    PoisonStats {
        enabled: is_enabled(),
        alloc_poison: ALLOC_POISON.load(Ordering::Relaxed),
        free_bytes: FREE_POISON_BYTES.load(Ordering::Relaxed),
        alloc_bytes: ALLOC_POISON_BYTES.load(Ordering::Relaxed),
        violations: VIOLATIONS_DETECTED.load(Ordering::Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the poison subsystem.
pub fn self_test() {
    serial_println!("[poison] Running self-test...");

    // Test 1: Poison free fills correctly.
    let mut buf = [0u8; 64];
    unsafe { poison_free(buf.as_mut_ptr(), buf.len()); }
    assert!(buf.iter().all(|&b| b == POISON_FREE));
    serial_println!("[poison]   Free poison fill: OK");

    // Test 2: Verify freed detects intact poison.
    assert!(unsafe { verify_freed(buf.as_ptr(), buf.len()) });
    serial_println!("[poison]   Verify freed (intact): OK");

    // Test 3: Verify freed detects corruption.
    buf[32] = 0x42; // Corrupt one byte.
    assert!(!unsafe { verify_freed(buf.as_ptr(), buf.len()) });
    serial_println!("[poison]   Verify freed (corrupted): OK");

    // Test 4: Redzone fill and verify.
    let mut rz = [0u8; 16];
    unsafe { poison_redzone(rz.as_mut_ptr(), rz.len()); }
    assert!(unsafe { verify_redzone(rz.as_ptr(), rz.len()) });
    rz[15] = 0x00; // Corrupt last byte.
    assert!(!unsafe { verify_redzone(rz.as_ptr(), rz.len()) });
    serial_println!("[poison]   Redzone fill + verify: OK");

    // Test 5: identify_poison.
    assert_eq!(identify_poison(0xDEDE_DEDE_DEDE_DEDE), Some("freed memory (use-after-free)"));
    assert_eq!(identify_poison(0xCDCD_CDCD_CDCD_CDCD), Some("uninitialized memory"));
    assert_eq!(identify_poison(0xFDFD_FDFD_FDFD_FDFD), Some("red zone (buffer overflow)"));
    assert_eq!(identify_poison(0x1234_5678_9ABC_DEF0), None);
    serial_println!("[poison]   identify_poison: OK");

    // Test 6: Disabled poisoning skips work.
    disable();
    let mut buf2 = [0u8; 32];
    unsafe { poison_free(buf2.as_mut_ptr(), buf2.len()); }
    assert!(buf2.iter().all(|&b| b == 0)); // Should NOT have been filled.
    enable(); // Re-enable.
    serial_println!("[poison]   Disabled bypass: OK");

    // Test 7: Stats updated.
    let st = stats();
    assert!(st.free_bytes > 0);
    assert!(st.violations > 0); // From tests 3 and 4.
    serial_println!("[poison]   Stats: free_bytes={}, violations={}", st.free_bytes, st.violations);

    serial_println!("[poison] Self-test PASSED");
}
