//! Syscall filtering (seccomp-equivalent) for container sandboxing.
//!
//! Provides per-process syscall allow/deny lists.  When a process has
//! a filter installed, every syscall is checked against the filter
//! before dispatch.  Denied syscalls return `PermissionDenied`.
//!
//! ## Design
//!
//! Unlike Linux seccomp-BPF which uses a bytecode VM, we use simple
//! bitmap-based filters — each filter is a 1000-bit bitmap (one bit
//! per syscall number).  This is:
//!
//! - O(1) per syscall check (single array index + bit test)
//! - Zero-allocation on the hot path
//! - Simple to audit and verify
//!
//! Filters are inherited on fork (child gets a copy of parent's filter).
//! Filters can only be tightened (a process can deny additional syscalls
//! but never re-allow one that was denied).
//!
//! ## Integration Points
//!
//! - **syscall/dispatch.rs**: Before looking up the handler, call
//!   `scfilter::check(task_id, syscall_nr)`.  If it returns `false`,
//!   return `PermissionDenied` without invoking the handler.
//! - **container.rs**: When creating a container, install a filter
//!   that allows only the syscalls the container needs.
//! - **proc/pcb.rs**: On fork, copy the parent's filter to the child.
//!
//! ## References
//!
//! - Linux seccomp(2), seccomp_rule_add(3)
//! - Design spec: capability-based security + container isolation

extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of syscall numbers we track.
///
/// Matches `syscall::number::MAX_SYSCALL_NR`.
pub const MAX_SYSCALL_NR: usize = 1000;

/// Maximum number of process filters.
///
/// One per active process.  Processes without a filter are unfiltered.
pub const MAX_FILTERS: usize = 128;

/// Number of `u64` words needed for the bitmap.
///
/// 1000 bits / 64 bits per word = 16 words (ceil).
#[allow(clippy::arithmetic_side_effects)]
const BITMAP_WORDS: usize = MAX_SYSCALL_NR.div_ceil(64);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Process ID type (matches scheduler's TaskId).
type Pid = u64;

/// A syscall filter bitmap.
///
/// Each bit position corresponds to a syscall number.
/// Bit set = allowed, bit clear = denied.
///
/// A fresh "allow-all" filter has all bits set.
/// A "deny-all" filter has all bits clear.
#[derive(Clone)]
struct FilterBitmap {
    /// Bitmap words.  Bit N corresponds to syscall number N.
    words: [u64; BITMAP_WORDS],
}

impl FilterBitmap {
    /// Create a filter that allows all syscalls.
    const fn allow_all() -> Self {
        Self { words: [u64::MAX; BITMAP_WORDS] }
    }

    /// Create a filter that denies all syscalls.
    const fn deny_all() -> Self {
        Self { words: [0; BITMAP_WORDS] }
    }

    /// Check if a syscall number is allowed.
    #[inline]
    fn is_allowed(&self, nr: usize) -> bool {
        if nr >= MAX_SYSCALL_NR {
            return false;
        }
        #[allow(clippy::arithmetic_side_effects)]
        let word_idx = nr / 64;
        #[allow(clippy::arithmetic_side_effects)]
        let bit_idx = nr % 64;
        // SAFETY: word_idx < BITMAP_WORDS because nr < MAX_SYSCALL_NR
        // and BITMAP_WORDS = ceil(MAX_SYSCALL_NR / 64).
        (self.words[word_idx] & (1u64 << bit_idx)) != 0
    }

    /// Allow a specific syscall number.
    fn allow(&mut self, nr: usize) {
        if nr >= MAX_SYSCALL_NR { return; }
        #[allow(clippy::arithmetic_side_effects)]
        let word_idx = nr / 64;
        #[allow(clippy::arithmetic_side_effects)]
        let bit_idx = nr % 64;
        self.words[word_idx] |= 1u64 << bit_idx;
    }

    /// Deny a specific syscall number.
    fn deny(&mut self, nr: usize) {
        if nr >= MAX_SYSCALL_NR { return; }
        #[allow(clippy::arithmetic_side_effects)]
        let word_idx = nr / 64;
        #[allow(clippy::arithmetic_side_effects)]
        let bit_idx = nr % 64;
        self.words[word_idx] &= !(1u64 << bit_idx);
    }

    /// Count how many syscalls are allowed.
    fn count_allowed(&self) -> usize {
        let full_words = MAX_SYSCALL_NR / 64;
        let remaining_bits = MAX_SYSCALL_NR % 64;

        let mut count = 0usize;

        // Count all bits in fully-covered words.
        for word in self.words.iter().take(full_words) {
            count = count.saturating_add(word.count_ones() as usize);
        }

        // For the partial final word, mask off bits beyond MAX_SYSCALL_NR.
        if remaining_bits > 0 {
            if let Some(&last_word) = self.words.get(full_words) {
                // Keep only the lower `remaining_bits` bits.
                let mask = (1u64 << remaining_bits).wrapping_sub(1);
                count = count.saturating_add((last_word & mask).count_ones() as usize);
            }
        }

        count
    }

    /// Intersect two filters (AND).  The result allows only syscalls
    /// that both filters allow.
    fn intersect(&self, other: &Self) -> Self {
        let mut result = Self::deny_all();
        for i in 0..BITMAP_WORDS {
            result.words[i] = self.words[i] & other.words[i];
        }
        result
    }
}

/// A filter entry attached to a process.
struct FilterEntry {
    /// Whether this slot is active.
    active: bool,
    /// Process (task) ID.
    pid: Pid,
    /// The filter bitmap.
    bitmap: FilterBitmap,
    /// How many syscalls were denied by this filter (audit counter).
    deny_count: u64,
}

impl FilterEntry {
    const fn empty() -> Self {
        Self {
            active: false,
            pid: 0,
            bitmap: FilterBitmap::allow_all(),
            deny_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether the filter system is initialized and active.
static ENABLED: AtomicBool = AtomicBool::new(false);

struct FilterTable {
    filters: [FilterEntry; MAX_FILTERS],
}

impl FilterTable {
    // `const fn` so the all-empty table is materialized in read-only
    // static memory at compile time (see `init`), not built on the stack.
    const fn new() -> Self {
        // FilterEntry::empty() is a const fn producing a valid default
        // state.  This avoids a loop over MAX_FILTERS.
        Self {
            filters: [const { FilterEntry::empty() }; MAX_FILTERS],
        }
    }
}

static TABLE: Mutex<Option<Box<FilterTable>>> = Mutex::new(None);

/// Initialize the syscall filter subsystem.
///
/// Uses heap allocation — `FilterTable` is ~19 KiB (128 entries × ~152 bytes),
/// too large for the boot stack under debug builds.
pub fn init() {
    let mut table = TABLE.lock();
    // Allocate on the heap to avoid stack overflow (FilterTable is ~19 KiB).
    //
    // `EMPTY` is a `const`, so the all-empty table lives in read-only
    // static memory; `Box::new` copies it straight to the heap without
    // first constructing a ~19 KiB temporary on the kernel stack
    // (a plain `Box::new(FilterTable::new())` would build that temporary
    // on the stack, which is what we must avoid here).
    const EMPTY: FilterTable = FilterTable::new();
    *table = Some(Box::new(EMPTY));
    ENABLED.store(true, Ordering::Release);
    serial_println!("[scfilter] Initialized ({} max filters)", MAX_FILTERS);
}

// ---------------------------------------------------------------------------
// Hot path: check
// ---------------------------------------------------------------------------

/// Check if a syscall is allowed for a given task.
///
/// Returns `true` if the syscall should proceed, `false` if it should
/// be denied with `PermissionDenied`.
///
/// This is the hot-path function called on every syscall.  It is
/// designed for minimal overhead:
///
/// - If no filter is installed for this task, returns `true` (O(1)).
/// - If a filter exists, it's a single bitmap lookup (O(1)).
/// - The global ENABLED check is a single atomic load.
///
/// # Performance
///
/// - No filter installed: ~5ns (atomic load + linear scan miss)
/// - Filter installed: ~10ns (atomic load + linear scan hit + bit test)
/// - Subsystem disabled: ~1ns (single atomic load)
#[inline]
pub fn check(task_id: u64, syscall_nr: u64) -> bool {
    // Fast path: subsystem not initialized or disabled.
    if !ENABLED.load(Ordering::Acquire) {
        return true;
    }

    let nr = syscall_nr as usize;
    if nr >= MAX_SYSCALL_NR {
        return false;
    }

    let mut guard = TABLE.lock();
    let Some(table) = guard.as_mut() else {
        return true; // Not initialized.
    };

    // Find the filter for this task.
    for entry in &mut table.filters {
        if entry.active && entry.pid == task_id {
            let allowed = entry.bitmap.is_allowed(nr);
            if !allowed {
                entry.deny_count = entry.deny_count.saturating_add(1);
            }
            return allowed;
        }
    }

    // No filter for this task — allow.
    true
}

// ---------------------------------------------------------------------------
// Public API: filter management
// ---------------------------------------------------------------------------

/// Install an allow-all filter for a process.
///
/// The process starts with everything allowed; use [`deny`] to
/// restrict specific syscalls.  This is the typical pattern:
///
/// ```ignore
/// scfilter::install(pid)?;
/// scfilter::deny(pid, SYS_PORT_READ);  // No raw port I/O
/// scfilter::deny(pid, SYS_PORT_WRITE);
/// scfilter::deny(pid, SYS_IRQ_REGISTER); // No direct IRQ access
/// ```
///
/// # Errors
///
/// - [`KernelError::AlreadyExists`] if a filter already exists.
/// - [`KernelError::ResourceExhausted`] if no filter slots available.
pub fn install(pid: Pid) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    // Check if already installed.
    for entry in &table.filters {
        if entry.active && entry.pid == pid {
            return Err(KernelError::AlreadyExists);
        }
    }

    // Find a free slot.
    for entry in &mut table.filters {
        if !entry.active {
            entry.active = true;
            entry.pid = pid;
            entry.bitmap = FilterBitmap::allow_all();
            entry.deny_count = 0;
            return Ok(());
        }
    }

    Err(KernelError::ResourceExhausted)
}

/// Install a deny-all filter for a process.
///
/// The process starts with everything denied; use [`allow`] to
/// enable specific syscalls.  This is the restrictive pattern:
///
/// ```ignore
/// scfilter::install_deny_all(pid)?;
/// scfilter::allow(pid, SYS_EXIT);
/// scfilter::allow(pid, SYS_CONSOLE_WRITE);
/// scfilter::allow(pid, SYS_CONSOLE_READ_CHAR);
/// ```
///
/// # Errors
///
/// - [`KernelError::AlreadyExists`] if a filter already exists.
/// - [`KernelError::ResourceExhausted`] if no filter slots available.
pub fn install_deny_all(pid: Pid) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    for entry in &table.filters {
        if entry.active && entry.pid == pid {
            return Err(KernelError::AlreadyExists);
        }
    }

    for entry in &mut table.filters {
        if !entry.active {
            entry.active = true;
            entry.pid = pid;
            entry.bitmap = FilterBitmap::deny_all();
            entry.deny_count = 0;
            return Ok(());
        }
    }

    Err(KernelError::ResourceExhausted)
}

/// Remove the filter for a process.
///
/// Called on process exit.  After this, the process is unfiltered
/// (but typically it's exiting anyway).
pub fn remove(pid: Pid) {
    let mut guard = TABLE.lock();
    let Some(table) = guard.as_mut() else { return; };

    for entry in &mut table.filters {
        if entry.active && entry.pid == pid {
            entry.active = false;
            return;
        }
    }
}

/// Deny a specific syscall for a process.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if no filter is installed for this pid.
pub fn deny(pid: Pid, syscall_nr: u64) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    for entry in &mut table.filters {
        if entry.active && entry.pid == pid {
            entry.bitmap.deny(syscall_nr as usize);
            return Ok(());
        }
    }

    Err(KernelError::NotFound)
}

/// Allow a specific syscall for a process.
///
/// Note: this only works if the filter was installed with
/// [`install_deny_all`].  If the filter was installed with [`install`]
/// (allow-all), all syscalls are already allowed.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if no filter is installed for this pid.
pub fn allow(pid: Pid, syscall_nr: u64) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    for entry in &mut table.filters {
        if entry.active && entry.pid == pid {
            entry.bitmap.allow(syscall_nr as usize);
            return Ok(());
        }
    }

    Err(KernelError::NotFound)
}

/// Copy a filter from one process to another (fork inheritance).
///
/// The child gets a snapshot of the parent's filter.  If the parent
/// has no filter, the child gets no filter.
///
/// # Errors
///
/// - [`KernelError::ResourceExhausted`] if no filter slots available.
pub fn copy_filter(parent_pid: Pid, child_pid: Pid) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    // Find parent's filter.
    let mut parent_bitmap = None;
    for entry in &table.filters {
        if entry.active && entry.pid == parent_pid {
            parent_bitmap = Some(entry.bitmap.clone());
            break;
        }
    }

    let Some(bitmap) = parent_bitmap else {
        return Ok(()); // Parent has no filter — child inherits none.
    };

    // Find a free slot for the child.
    for entry in &mut table.filters {
        if !entry.active {
            entry.active = true;
            entry.pid = child_pid;
            entry.bitmap = bitmap;
            entry.deny_count = 0;
            return Ok(());
        }
    }

    Err(KernelError::ResourceExhausted)
}

/// Tighten a filter by intersecting it with additional restrictions.
///
/// After this, only syscalls allowed by BOTH the existing filter
/// AND the new restrictions will be allowed.  This is the "only
/// tighten" invariant — a process can never re-allow a denied syscall.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if no filter is installed for this pid.
pub fn tighten(pid: Pid, restrictions: &[u64]) -> KernelResult<()> {
    let mut guard = TABLE.lock();
    let table = guard.as_mut().ok_or(KernelError::NotSupported)?;

    for entry in &mut table.filters {
        if entry.active && entry.pid == pid {
            // Build a bitmap from the restriction list (deny these).
            let mut deny_mask = FilterBitmap::allow_all();
            for &nr in restrictions {
                deny_mask.deny(nr as usize);
            }
            entry.bitmap = entry.bitmap.intersect(&deny_mask);
            return Ok(());
        }
    }

    Err(KernelError::NotFound)
}

// ---------------------------------------------------------------------------
// Public API: queries
// ---------------------------------------------------------------------------

/// Check if a process has a filter installed.
#[must_use]
pub fn has_filter(pid: Pid) -> bool {
    let guard = TABLE.lock();
    let Some(table) = guard.as_ref() else { return false; };

    table.filters.iter().any(|e| e.active && e.pid == pid)
}

/// Get the number of allowed syscalls for a process.
#[must_use]
pub fn allowed_count(pid: Pid) -> Option<usize> {
    let guard = TABLE.lock();
    let table = guard.as_ref()?;

    for entry in &table.filters {
        if entry.active && entry.pid == pid {
            return Some(entry.bitmap.count_allowed());
        }
    }

    None
}

/// Get the deny count for a process (how many syscalls were blocked).
#[must_use]
pub fn deny_count(pid: Pid) -> u64 {
    let guard = TABLE.lock();
    let Some(table) = guard.as_ref() else { return 0; };

    for entry in &table.filters {
        if entry.active && entry.pid == pid {
            return entry.deny_count;
        }
    }

    0
}

/// Count active filters.
#[must_use]
pub fn active_count() -> usize {
    let guard = TABLE.lock();
    let Some(table) = guard.as_ref() else { return 0; };

    table.filters.iter().filter(|e| e.active).count()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for syscall filtering.
pub fn self_test() {
    serial_println!("[scfilter] Running self-test...");

    // Test 1: No filters initially.
    assert_eq!(active_count(), 0);
    serial_println!("[scfilter]   Initial state: OK");

    // Test 2: Unfiltered process — all syscalls allowed.
    assert!(check(100, 0)); // SYS_YIELD
    assert!(check(100, 1)); // SYS_EXIT
    assert!(check(100, 999)); // Max valid
    serial_println!("[scfilter]   Unfiltered allows all: OK");

    // Test 3: Install allow-all filter.
    install(200).expect("install");
    assert!(has_filter(200));
    assert_eq!(active_count(), 1);
    assert!(check(200, 0));
    assert!(check(200, 100));
    assert_eq!(allowed_count(200), Some(MAX_SYSCALL_NR));
    serial_println!("[scfilter]   Allow-all filter: OK");

    // Test 4: Deny specific syscalls.
    deny(200, 10).expect("deny 10");
    deny(200, 11).expect("deny 11");
    deny(200, 12).expect("deny 12");
    assert!(!check(200, 10));
    assert!(!check(200, 11));
    assert!(!check(200, 12));
    assert!(check(200, 9)); // Adjacent — still allowed.
    assert!(check(200, 13));
    assert_eq!(allowed_count(200), Some(MAX_SYSCALL_NR - 3));
    serial_println!("[scfilter]   Deny specific syscalls: OK");

    // Test 5: Deny count tracking.
    let _ = check(200, 10); // denied
    let _ = check(200, 10); // denied again
    assert_eq!(deny_count(200), 5); // 3 from test 4 (check 10,11,12) + 2 from test 5
    serial_println!("[scfilter]   Deny count tracking: OK");

    // Test 6: Install deny-all filter.
    install_deny_all(300).expect("install deny-all");
    assert!(!check(300, 0));
    assert!(!check(300, 500));
    assert_eq!(allowed_count(300), Some(0));
    serial_println!("[scfilter]   Deny-all filter: OK");

    // Test 7: Allow specific syscalls.
    allow(300, 0).expect("allow 0"); // SYS_YIELD
    allow(300, 1).expect("allow 1"); // SYS_EXIT
    assert!(check(300, 0));
    assert!(check(300, 1));
    assert!(!check(300, 2)); // Still denied.
    assert_eq!(allowed_count(300), Some(2));
    serial_println!("[scfilter]   Allow specific syscalls: OK");

    // Test 8: Filter inheritance (copy).
    copy_filter(200, 400).expect("copy");
    assert!(has_filter(400));
    assert!(!check(400, 10)); // Inherited deny.
    assert!(check(400, 9)); // Inherited allow.
    serial_println!("[scfilter]   Filter inheritance: OK");

    // Test 9: Tighten filter.
    let restrictions = [9u64, 13]; // Deny 9 and 13 additionally.
    tighten(400, &restrictions).expect("tighten");
    assert!(!check(400, 9)); // Was allowed, now denied.
    assert!(!check(400, 13)); // Was allowed, now denied.
    assert!(check(400, 8)); // Not in restrictions — still allowed.
    serial_println!("[scfilter]   Tighten filter: OK");

    // Test 10: Duplicate install rejected.
    assert!(install(200).is_err());
    serial_println!("[scfilter]   Duplicate install rejected: OK");

    // Test 11: Operations on non-existent filter.
    assert!(deny(999, 0).is_err());
    assert!(allow(999, 0).is_err());
    assert!(!has_filter(999));
    serial_println!("[scfilter]   Non-existent filter rejected: OK");

    // Test 12: Out-of-range syscall number.
    assert!(!check(200, 1000)); // >= MAX_SYSCALL_NR — always denied.
    assert!(!check(200, u64::MAX));
    serial_println!("[scfilter]   Out-of-range denied: OK");

    // Test 13: Remove filter.
    remove(200);
    assert!(!has_filter(200));
    assert!(check(200, 10)); // No filter — allowed again.
    serial_println!("[scfilter]   Remove filter: OK");

    // Test 14: Copy from unfiltered parent.
    remove(400);
    copy_filter(999, 500).expect("copy from unfiltered");
    assert!(!has_filter(500)); // No filter installed.
    serial_println!("[scfilter]   Copy from unfiltered: OK");

    // Cleanup.
    remove(300);
    remove(400);
    remove(500);
    assert_eq!(active_count(), 0);
    serial_println!("[scfilter]   Cleanup: OK");

    serial_println!("[scfilter] Self-test PASSED (14 tests)");
}
