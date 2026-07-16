//! Memory pressure notification system.
//!
//! Provides a mechanism for kernel subsystems to register interest in
//! memory pressure events and respond by shrinking their caches.  This
//! is the kernel-internal analogue of Linux's `shrinker` infrastructure
//! (`mm/shrinker.c`).
//!
//! ## Design
//!
//! Three pressure levels correspond to increasing urgency:
//!
//! - **Low**: free memory has dropped below the kswapd low watermark.
//!   Background reclamation is active.  Caches should start trimming
//!   cold entries opportunistically.
//!
//! - **Medium**: free memory is critically low.  Direct reclamation
//!   (synchronous, in the allocating task's context) is happening.
//!   Caches should aggressively shrink to their minimum useful size.
//!
//! - **Critical**: OOM is imminent.  All non-essential memory must be
//!   freed immediately.  Caches should be flushed entirely if possible.
//!
//! ## Usage
//!
//! Subsystems register a shrinker callback via [`register_shrinker`]:
//!
//! ```ignore
//! mm::pressure::register_shrinker(
//!     "buffer-cache",
//!     |level| {
//!         match level {
//!             PressureLevel::Low => cache::shrink(25),     // trim 25%
//!             PressureLevel::Medium => cache::shrink(50),  // trim 50%
//!             PressureLevel::Critical => cache::shrink(90), // flush most
//!         }
//!     },
//! );
//! ```
//!
//! The pressure notification system is invoked by:
//! - kswapd when it detects low watermark breach
//! - Direct reclaim when allocation fails
//! - OOM handler before killing processes
//!
//! ## Performance
//!
//! Shrinker callbacks run in the caller's context (kswapd task or the
//! allocating task during direct reclaim).  They must not:
//! - Block for extended periods
//! - Allocate memory (would recurse)
//! - Hold locks that the frame allocator needs
//!
//! ## References
//!
//! - Linux `mm/shrinker.c` — shrinker infrastructure
//! - Linux `include/linux/shrinker.h` — shrinker interface
//! - FreeBSD `vm/uma_core.c` — zone_reclaim

use crate::serial_println;
use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use crate::sync::Mutex;

// ---------------------------------------------------------------------------
// Pressure levels
// ---------------------------------------------------------------------------

/// Memory pressure severity levels.
///
/// Higher levels indicate more urgent need to free memory.  Shrinker
/// callbacks receive the current level and should scale their response
/// accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PressureLevel {
    /// No pressure — system is healthy.  Never sent to shrinkers.
    None = 0,

    /// Low pressure: free memory below kswapd watermark.
    ///
    /// Background reclamation is active.  Caches should trim cold/LRU
    /// entries that haven't been accessed recently.  Target: free ~25%
    /// of reclaimable cache entries.
    Low = 1,

    /// Medium pressure: direct reclaim path triggered.
    ///
    /// An allocation is waiting for free frames and the allocator is
    /// synchronously trying to reclaim.  Caches should shrink
    /// aggressively.  Target: free ~50% of cache.
    Medium = 2,

    /// Critical pressure: OOM imminent.
    ///
    /// The OOM handler is about to start killing processes.  All
    /// non-essential cached data should be discarded.  This is the
    /// last chance to avoid process termination.  Target: free
    /// everything that can be freed.
    Critical = 3,
}

impl PressureLevel {
    /// Convert from raw u8 (clamped).
    #[must_use]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Low,
            2 => Self::Medium,
            3 => Self::Critical,
            _ => Self::Critical, // Saturate.
        }
    }
}

impl core::fmt::Display for PressureLevel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

// ---------------------------------------------------------------------------
// Shrinker registration
// ---------------------------------------------------------------------------

/// Maximum number of registered shrinkers.
///
/// Generous for a desktop OS: buffer cache, inode cache, dcache,
/// extent cache, page cache, VFS metadata, network buffers, etc.
const MAX_SHRINKERS: usize = 16;

/// A registered shrinker callback.
struct Shrinker {
    /// Human-readable name (for diagnostics).
    name: &'static str,
    /// Callback invoked under memory pressure.
    ///
    /// Receives the current pressure level.  Should return the number
    /// of "objects" (pages, entries, etc.) freed.  The interpretation
    /// of the count is subsystem-specific — it's used for diagnostics
    /// only, not for allocation accounting.
    callback: fn(PressureLevel) -> usize,
    /// Whether this slot is occupied.
    active: bool,
}

impl Shrinker {
    const EMPTY: Self = Self {
        name: "",
        callback: |_| 0,
        active: false,
    };
}

/// Global shrinker registry.
///
/// Protected by a spinlock.  Only modified during init (register) and
/// queried during reclaim (notify).  The lock is held briefly.
static SHRINKERS: Mutex<[Shrinker; MAX_SHRINKERS]> = Mutex::named(
    [Shrinker::EMPTY; MAX_SHRINKERS], b"SHRINK"
);

// ---------------------------------------------------------------------------
// Pressure state tracking
// ---------------------------------------------------------------------------

/// Current system-wide pressure level (atomic for lock-free read).
static CURRENT_LEVEL: AtomicU8 = AtomicU8::new(0);

/// Total number of pressure notifications sent since boot.
static NOTIFY_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total objects freed across all shrinker callbacks since boot.
static TOTAL_FREED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register a shrinker callback.
///
/// `name` is used for diagnostic logging.  `callback` is called with
/// the current pressure level when memory is scarce.  It should free
/// cached objects and return the count freed.
///
/// Returns `Some(slot_index)` on success, `None` if the registry is full.
///
/// # Panics
///
/// Does not panic.  If the table is full, returns `None` and logs a
/// warning.
#[allow(dead_code)] // Public API for subsystem init.
pub fn register_shrinker(
    name: &'static str,
    callback: fn(PressureLevel) -> usize,
) -> Option<usize> {
    let mut table = SHRINKERS.lock();
    for (i, slot) in table.iter_mut().enumerate() {
        if !slot.active {
            slot.name = name;
            slot.callback = callback;
            slot.active = true;
            serial_println!("[pressure] Registered shrinker: {}", name);
            return Some(i);
        }
    }
    serial_println!(
        "[pressure] WARNING: shrinker table full, cannot register '{}'",
        name,
    );
    None
}

/// Unregister a shrinker by slot index.
///
/// Returns `true` if the slot was active and is now freed.
#[allow(dead_code)] // Public API for subsystem teardown.
pub fn unregister_shrinker(slot: usize) -> bool {
    let mut table = SHRINKERS.lock();
    if let Some(entry) = table.get_mut(slot) {
        if entry.active {
            serial_println!(
                "[pressure] Unregistered shrinker: {}",
                entry.name,
            );
            *entry = Shrinker::EMPTY;
            return true;
        }
    }
    false
}

/// Notify all registered shrinkers of memory pressure.
///
/// Called by:
/// - kswapd when entering the reclaim loop (level = Low)
/// - Direct reclaim in `alloc_order` slow path (level = Medium)
/// - OOM handler before killing (level = Critical)
///
/// Returns the total number of objects freed across all shrinkers.
pub fn notify(level: PressureLevel) {
    if level == PressureLevel::None {
        return;
    }

    // Update current level (visible to query API).
    let prev_level = PressureLevel::from_u8(CURRENT_LEVEL.load(Ordering::Acquire));
    CURRENT_LEVEL.store(level as u8, Ordering::Release);
    NOTIFY_COUNT.fetch_add(1, Ordering::Relaxed);

    // Log level transitions (avoid spamming on repeated same-level notifications).
    if level != prev_level {
        crate::klog!(Warn, "mm.pressure",
            "level transition: {} -> {}",
            prev_level, level
        );
        // Record pressure transition in trace buffer for timing analysis.
        crate::ktrace::record(
            crate::ktrace::Category::Mm,
            crate::ktrace::event::RECLAIM,
            prev_level as u64,
            level as u64,
        );
    }

    let table = SHRINKERS.lock();
    let mut total_freed: u64 = 0;

    for shrinker in table.iter() {
        if shrinker.active {
            // Defense-in-depth: validate the stored shrinker pointer against
            // real kernel `.text` before calling it.  A registered callback
            // always points into code; a value that doesn't means this table's
            // heap backing was corrupted (the B-KNULLJUMP-SIGNAL class — a wild
            // `call` through a clobbered code-pointer field).  Log + skip.
            let cb_addr = shrinker.callback as *const () as u64;
            if !crate::idt::is_kernel_text(cb_addr) {
                serial_println!(
                    "[pressure] CRITICAL: refusing to run corrupt shrinker '{}' callback={:#x} \
                     — table corruption; skipping (see B-KNULLJUMP-SIGNAL)",
                    shrinker.name, cb_addr
                );
                continue;
            }
            let freed = (shrinker.callback)(level);
            if freed > 0 {
                serial_println!(
                    "[pressure] {} freed {} objects (level={})",
                    shrinker.name, freed, level,
                );
                total_freed = total_freed.saturating_add(freed as u64);
            }
        }
    }

    TOTAL_FREED.fetch_add(total_freed, Ordering::Relaxed);

    // If pressure drops (e.g., shrinkers freed enough), clear the level.
    // The caller (kswapd, direct reclaim) will re-raise if still needed.
    // We don't clear here because the caller knows the actual free count.
}

/// Clear the pressure level back to None.
///
/// Called by kswapd when free memory rises above the high watermark.
pub fn clear_pressure() {
    CURRENT_LEVEL.store(PressureLevel::None as u8, Ordering::Release);
}

/// Query the current pressure level (lock-free).
#[must_use]
pub fn current_level() -> PressureLevel {
    PressureLevel::from_u8(CURRENT_LEVEL.load(Ordering::Acquire))
}

/// Total notification events since boot.
#[must_use]
#[allow(dead_code)] // Diagnostic API.
pub fn notification_count() -> u64 {
    NOTIFY_COUNT.load(Ordering::Relaxed)
}

/// Total objects freed by all shrinkers since boot.
#[must_use]
#[allow(dead_code)] // Diagnostic API.
pub fn total_objects_freed() -> u64 {
    TOTAL_FREED.load(Ordering::Relaxed)
}

/// Number of active shrinkers registered.
#[must_use]
#[allow(dead_code)] // Diagnostic API.
pub fn active_shrinker_count() -> usize {
    let table = SHRINKERS.lock();
    table.iter().filter(|s| s.active).count()
}

/// Diagnostic summary of pressure state.
#[must_use]
#[allow(dead_code)] // Public API for procfs/meminfo.
pub fn pressure_info() -> PressureInfo {
    let table = SHRINKERS.lock();
    let active = table.iter().filter(|s| s.active).count();
    PressureInfo {
        level: current_level(),
        active_shrinkers: active,
        total_notifications: NOTIFY_COUNT.load(Ordering::Relaxed),
        total_freed: TOTAL_FREED.load(Ordering::Relaxed),
    }
}

/// Snapshot of memory pressure state for diagnostics.
#[derive(Debug, Clone)]
pub struct PressureInfo {
    /// Current pressure level.
    pub level: PressureLevel,
    /// Number of registered shrinkers.
    pub active_shrinkers: usize,
    /// Total notification events since boot.
    pub total_notifications: u64,
    /// Total objects freed by shrinkers since boot.
    pub total_freed: u64,
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the memory pressure notification system.
#[allow(clippy::expect_used)] // Tests panic on unexpected state.
pub fn self_test() {
    use core::sync::atomic::AtomicUsize;

    serial_println!("[pressure] Running self-test...");

    // --- 1. Registration ---
    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_shrinker(level: PressureLevel) -> usize {
        let freed = match level {
            PressureLevel::Low => 10,
            PressureLevel::Medium => 50,
            PressureLevel::Critical => 100,
            PressureLevel::None => 0,
        };
        TEST_COUNTER.fetch_add(freed, Ordering::Relaxed);
        freed
    }

    let slot = register_shrinker("test-shrinker", test_shrinker);
    assert!(slot.is_some(), "Registration should succeed");
    let slot_idx = slot.expect("checked");
    serial_println!("[pressure]   Registration: OK (slot {})", slot_idx);

    // --- 2. Level query (initially None) ---
    // Note: level might not be None if other tests ran first,
    // but clear it for our test.
    clear_pressure();
    assert_eq!(current_level(), PressureLevel::None);
    serial_println!("[pressure]   Initial level: OK (None)");

    // --- 3. Notify Low ---
    TEST_COUNTER.store(0, Ordering::Relaxed);
    notify(PressureLevel::Low);
    assert_eq!(current_level(), PressureLevel::Low);
    assert_eq!(TEST_COUNTER.load(Ordering::Relaxed), 10);
    serial_println!("[pressure]   Notify Low: OK (freed 10)");

    // --- 4. Notify Critical ---
    TEST_COUNTER.store(0, Ordering::Relaxed);
    notify(PressureLevel::Critical);
    assert_eq!(current_level(), PressureLevel::Critical);
    assert_eq!(TEST_COUNTER.load(Ordering::Relaxed), 100);
    serial_println!("[pressure]   Notify Critical: OK (freed 100)");

    // --- 5. Clear ---
    clear_pressure();
    assert_eq!(current_level(), PressureLevel::None);
    serial_println!("[pressure]   Clear: OK");

    // --- 6. Notify None is no-op ---
    TEST_COUNTER.store(0, Ordering::Relaxed);
    notify(PressureLevel::None);
    assert_eq!(TEST_COUNTER.load(Ordering::Relaxed), 0);
    serial_println!("[pressure]   Notify None (no-op): OK");

    // --- 7. Unregister ---
    assert!(unregister_shrinker(slot_idx));
    TEST_COUNTER.store(0, Ordering::Relaxed);
    notify(PressureLevel::Medium);
    assert_eq!(TEST_COUNTER.load(Ordering::Relaxed), 0);
    clear_pressure();
    serial_println!("[pressure]   Unregister: OK");

    // --- 8. Stats ---
    let info = pressure_info();
    // We had at least 3 notifications (Low, Critical, Medium after unregister).
    assert!(info.total_notifications >= 3);
    serial_println!("[pressure]   Stats: OK (notifications={})", info.total_notifications);

    serial_println!("[pressure] Self-test PASSED");
}
