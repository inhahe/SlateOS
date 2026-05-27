//! Kernel watchpoints — monitor memory addresses for changes.
//!
//! Allows setting software watchpoints on kernel virtual addresses.
//! When a watched address is polled (via periodic check), if its value
//! has changed since last sample, the event is logged.
//!
//! ## Design
//!
//! Software watchpoints (no hardware debug registers used):
//! - Store the address and last-known value.
//! - Periodically check (e.g., on timer tick or manual poll) whether
//!   the value has changed.
//! - If changed, log the old/new values and update the stored value.
//!
//! This is a polling approach — it won't catch every intermediate change,
//! but it's useful for finding which subsystem is modifying a value and
//! approximately when.
//!
//! ## Limitations
//!
//! - Maximum 8 active watchpoints (minimal memory footprint).
//! - Only watches aligned u64 values (8-byte aligned addresses).
//! - Polling-based: may miss rapid changes between polls.
//! - For precise hardware watchpoints, use x86 debug registers (DR0-DR3)
//!   — that's a future enhancement.
//!
//! ## Usage
//!
//! ```text
//! kshell> watch add 0xffff800000100000    — watch a kernel address
//! kshell> watch list                       — show active watchpoints
//! kshell> watch poll                       — check for changes now
//! kshell> watch del 0                      — remove watchpoint #0
//! kshell> watch clear                      — remove all watchpoints
//! ```
//!
//! ## References
//!
//! - GDB watchpoints (`watch *addr`) — hardware debug register approach
//! - Linux kprobes — dynamic kernel instrumentation
//! - Valgrind memcheck — shadow memory tracking

use core::sync::atomic::{AtomicU8, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of active watchpoints.
const MAX_WATCHPOINTS: usize = 8;

// ---------------------------------------------------------------------------
// Watchpoint data
// ---------------------------------------------------------------------------

/// A single watchpoint entry.
#[derive(Debug, Clone, Copy)]
pub struct Watchpoint {
    /// Virtual address being watched (must be 8-byte aligned, kernel space).
    pub address: u64,
    /// Last sampled value at this address.
    pub last_value: u64,
    /// Number of times the value has changed.
    pub change_count: u32,
    /// APIC tick when last change was detected.
    pub last_change_tick: u64,
    /// Whether this watchpoint slot is active.
    pub active: bool,
    /// Optional label (first 7 bytes + null).
    pub label: [u8; 8],
}

impl Watchpoint {
    const fn empty() -> Self {
        Self {
            address: 0,
            last_value: 0,
            change_count: 0,
            last_change_tick: 0,
            active: false,
            label: [0; 8],
        }
    }
}

/// Event logged when a watchpoint fires.
#[derive(Debug, Clone, Copy)]
pub struct WatchEvent {
    /// Which watchpoint (slot index).
    pub slot: u8,
    /// Address that changed.
    pub address: u64,
    /// Old value.
    pub old_value: u64,
    /// New value.
    pub new_value: u64,
    /// Tick when detected.
    pub tick: u64,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

struct WatchStore(core::cell::UnsafeCell<[Watchpoint; MAX_WATCHPOINTS]>);
unsafe impl Sync for WatchStore {}

static STORE: WatchStore = WatchStore(
    core::cell::UnsafeCell::new([Watchpoint::empty(); MAX_WATCHPOINTS])
);

/// Number of active watchpoints.
static ACTIVE_COUNT: AtomicU8 = AtomicU8::new(0);

/// Event log (last 16 events).
const EVENT_LOG_SIZE: usize = 16;
const EVENT_LOG_MASK: usize = EVENT_LOG_SIZE - 1;

struct EventLog(core::cell::UnsafeCell<[WatchEvent; EVENT_LOG_SIZE]>);
unsafe impl Sync for EventLog {}

static EVENTS: EventLog = EventLog(core::cell::UnsafeCell::new(
    [WatchEvent { slot: 0, address: 0, old_value: 0, new_value: 0, tick: 0 }; EVENT_LOG_SIZE]
));

static EVENT_POS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Add a watchpoint on the given kernel virtual address.
///
/// Returns the slot index on success, or `None` if all slots are full
/// or the address is invalid.
pub fn add(address: u64, label: &[u8]) -> Option<usize> {
    // Validate: must be in kernel space (above 0xffff800000000000) and aligned.
    if address < 0xffff_8000_0000_0000 {
        return None;
    }
    if address & 0x7 != 0 {
        return None; // Must be 8-byte aligned.
    }

    // Find a free slot.
    for i in 0..MAX_WATCHPOINTS {
        let wp = unsafe {
            let ptr = STORE.0.get() as *const Watchpoint;
            ptr.add(i).read()
        };
        if !wp.active {
            // Read current value at the address.
            // SAFETY: We verified it's a kernel-space aligned address.
            let current_value = unsafe {
                core::ptr::read_volatile(address as *const u64)
            };

            let mut wp_label = [0u8; 8];
            let copy_len = label.len().min(7);
            wp_label[..copy_len].copy_from_slice(&label[..copy_len]);

            let new_wp = Watchpoint {
                address,
                last_value: current_value,
                change_count: 0,
                last_change_tick: 0,
                active: true,
                label: wp_label,
            };

            unsafe {
                let ptr = STORE.0.get() as *mut Watchpoint;
                ptr.add(i).write(new_wp);
            }

            ACTIVE_COUNT.fetch_add(1, Ordering::Relaxed);
            return Some(i);
        }
    }
    None // All slots full.
}

/// Remove a watchpoint by slot index.
pub fn remove(slot: usize) -> bool {
    if slot >= MAX_WATCHPOINTS {
        return false;
    }
    let wp = unsafe {
        let ptr = STORE.0.get() as *const Watchpoint;
        ptr.add(slot).read()
    };
    if !wp.active {
        return false;
    }

    unsafe {
        let ptr = STORE.0.get() as *mut Watchpoint;
        ptr.add(slot).write(Watchpoint::empty());
    }
    ACTIVE_COUNT.fetch_sub(1, Ordering::Relaxed);
    true
}

/// Clear all watchpoints.
pub fn clear() {
    for i in 0..MAX_WATCHPOINTS {
        unsafe {
            let ptr = STORE.0.get() as *mut Watchpoint;
            ptr.add(i).write(Watchpoint::empty());
        }
    }
    ACTIVE_COUNT.store(0, Ordering::Relaxed);
}

/// Poll all active watchpoints, returning how many have changed.
///
/// For each changed watchpoint, logs the event and updates the stored value.
pub fn poll() -> usize {
    let mut changes = 0;
    let tick = crate::apic::tick_count();

    for i in 0..MAX_WATCHPOINTS {
        let wp = unsafe {
            let ptr = STORE.0.get() as *const Watchpoint;
            ptr.add(i).read()
        };
        if !wp.active {
            continue;
        }

        // Read current value.
        // SAFETY: Address was validated on add().
        let current = unsafe {
            core::ptr::read_volatile(wp.address as *const u64)
        };

        if current != wp.last_value {
            // Value changed!
            changes += 1;

            // Log the event.
            let event = WatchEvent {
                slot: i as u8,
                address: wp.address,
                old_value: wp.last_value,
                new_value: current,
                tick,
            };
            let pos = EVENT_POS.fetch_add(1, Ordering::Relaxed);
            let event_slot = (pos as usize) & EVENT_LOG_MASK;
            unsafe {
                let ptr = EVENTS.0.get() as *mut WatchEvent;
                ptr.add(event_slot).write(event);
            }

            // Update the watchpoint.
            let updated = Watchpoint {
                address: wp.address,
                last_value: current,
                change_count: wp.change_count.saturating_add(1),
                last_change_tick: tick,
                active: true,
                label: wp.label,
            };
            unsafe {
                let ptr = STORE.0.get() as *mut Watchpoint;
                ptr.add(i).write(updated);
            }
        }
    }

    changes
}

/// Get all active watchpoints.
pub fn list() -> [(usize, Watchpoint); MAX_WATCHPOINTS] {
    let mut result = [(0usize, Watchpoint::empty()); MAX_WATCHPOINTS];
    for i in 0..MAX_WATCHPOINTS {
        let wp = unsafe {
            let ptr = STORE.0.get() as *const Watchpoint;
            ptr.add(i).read()
        };
        result[i] = (i, wp);
    }
    result
}

/// Get a specific watchpoint.
pub fn get(slot: usize) -> Option<Watchpoint> {
    if slot >= MAX_WATCHPOINTS {
        return None;
    }
    let wp = unsafe {
        let ptr = STORE.0.get() as *const Watchpoint;
        ptr.add(slot).read()
    };
    if wp.active { Some(wp) } else { None }
}

/// Number of active watchpoints.
#[must_use]
pub fn active_count() -> u8 {
    ACTIVE_COUNT.load(Ordering::Relaxed)
}

/// Get recent watchpoint events (newest first).
pub fn recent_events(buf: &mut [WatchEvent]) -> usize {
    let pos = EVENT_POS.load(Ordering::Acquire) as usize;
    let available = pos.min(EVENT_LOG_SIZE);
    let to_copy = buf.len().min(available);

    for i in 0..to_copy {
        let idx = (pos.wrapping_sub(1).wrapping_sub(i)) & EVENT_LOG_MASK;
        unsafe {
            let ptr = EVENTS.0.get() as *const WatchEvent;
            buf[i] = ptr.add(idx).read();
        }
    }
    to_copy
}

/// Total events recorded.
#[must_use]
pub fn total_events() -> u32 {
    EVENT_POS.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for watchpoints.
pub fn self_test() {
    serial_println!("[watchpoint] Running self-test...");

    // Test 1: Clear state.
    clear();
    assert_eq!(active_count(), 0);
    serial_println!("[watchpoint]   Clear: OK");

    // Test 2: Add a watchpoint on a known kernel variable.
    // Use EVENT_POS — it's an AtomicU32 at a stable, 8-byte-aligned address.
    // (ACTIVE_COUNT is AtomicU8 and might not be 8-byte aligned.)
    let test_addr = &EVENT_POS as *const core::sync::atomic::AtomicU32 as u64;

    // Make sure it's in kernel space and aligned.
    if test_addr >= 0xffff_8000_0000_0000 && test_addr & 0x7 == 0 {
        let slot = add(test_addr, b"test");
        assert!(slot.is_some());
        let slot = slot.unwrap();
        assert_eq!(active_count(), 1);
        serial_println!("[watchpoint]   Add: OK (slot={}, addr={:#x})", slot, test_addr);

        // Test 3: Poll with no change.
        let changes = poll();
        assert_eq!(changes, 0);
        serial_println!("[watchpoint]   Poll (no change): OK");

        // Test 4: Change the value and poll.
        // We need to change the value at test_addr.
        // EVENT_POS was already incremented by poll events from earlier,
        // but let's modify it explicitly.
        let before = EVENT_POS.load(Ordering::Relaxed);
        EVENT_POS.store(before.wrapping_add(100), Ordering::Relaxed);
        let changes = poll();
        // Should detect the change.
        assert_eq!(changes, 1);
        serial_println!("[watchpoint]   Poll (detected change): OK");

        // Test 5: Check event log.
        let mut events = [WatchEvent { slot: 0, address: 0, old_value: 0, new_value: 0, tick: 0 }; 4];
        let n = recent_events(&mut events);
        assert!(n >= 1);
        assert_eq!(events[0].address, test_addr);
        serial_println!("[watchpoint]   Event log: OK ({} events)", n);

        // Test 6: Remove watchpoint.
        assert!(remove(slot));
        assert_eq!(active_count(), 0);
        serial_println!("[watchpoint]   Remove: OK");

        // Restore EVENT_POS.
        EVENT_POS.store(before, Ordering::Relaxed);
    } else {
        serial_println!("[watchpoint]   (skipped address tests — alignment issue)");
    }

    // Test 7: Invalid addresses rejected.
    assert!(add(0x1000, b"user").is_none()); // Userspace addr.
    assert!(add(0xffff_8000_0000_0001, b"odd").is_none()); // Unaligned.
    serial_println!("[watchpoint]   Reject invalid: OK");

    // Test 8: Multiple watchpoints.
    clear();
    // Add the maximum number (if we have valid addresses).
    // Just test slot management.
    let valid_addr = &EVENT_POS as *const core::sync::atomic::AtomicU32 as u64;
    if valid_addr >= 0xffff_8000_0000_0000 && valid_addr & 0x7 == 0 {
        for i in 0..MAX_WATCHPOINTS {
            let result = add(valid_addr, b"bulk");
            // First one succeeds, rest may or may not (same addr is fine).
            if i == 0 {
                assert!(result.is_some());
            }
        }
        let count = active_count();
        assert!(count > 0);
        serial_println!("[watchpoint]   Multiple slots: OK ({} active)", count);
    }

    // Cleanup.
    clear();

    serial_println!("[watchpoint] Self-test PASSED");
}
