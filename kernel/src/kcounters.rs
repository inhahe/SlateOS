//! Kernel event counters — unified statistics tracking.
//!
//! Provides a centralized registry of named atomic counters for tracking
//! kernel events.  Any subsystem can define counters and increment them
//! on the hot path with minimal overhead (one atomic add).
//!
//! ## Design
//!
//! - **Static registration**: counters are defined at compile time via
//!   `define_counter!()`.  No dynamic allocation.
//! - **Lock-free**: all counters are `AtomicU64`, safe from any context.
//! - **Grouped**: counters are organized by subsystem (mm, sched, ipc, etc.).
//! - **Queryable**: the `counters` kshell command shows all counters with
//!   their current values.
//!
//! ## Usage
//!
//! ```ignore
//! // Define counters (module level):
//! use crate::kcounters;
//!
//! kcounters::define_counter!(PF_HANDLED, PF_HANDLED_DESC, "mm", "page_faults_handled");
//! kcounters::define_counter!(TLB_SHOOTDOWNS, TLB_SHOOTDOWNS_DESC, "mm", "tlb_shootdowns_sent");
//!
//! // Increment in hot path:
//! counter_inc!(PF_HANDLED);
//!
//! // Or add a specific value:
//! counter_add!(TLB_SHOOTDOWNS, 4);
//! ```
//!
//! ## References
//!
//! - Linux `/proc/vmstat` — aggregated VM counters
//! - Fuchsia `kcounters` — kernel counters infrastructure
//! - FreeBSD `kern.stats` — sysctl-based counters

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Counter registry
// ---------------------------------------------------------------------------

/// Maximum number of registered counters.
const MAX_COUNTERS: usize = 64;

/// A single counter descriptor.
pub struct CounterDesc {
    /// Subsystem group (e.g., "mm", "sched", "ipc").
    pub group: &'static str,
    /// Counter name (e.g., "page_faults", "ctx_switches").
    pub name: &'static str,
    /// Pointer to the AtomicU64 value.
    pub value: &'static AtomicU64,
}

/// Registry of all counters.
struct Registry {
    counters: [Option<&'static CounterDesc>; MAX_COUNTERS],
    count: usize,
}

impl Registry {
    const fn new() -> Self {
        Self {
            counters: [None; MAX_COUNTERS],
            count: 0,
        }
    }
}

/// Global counter registry (initialized at boot, read-only after).
static mut REGISTRY: Registry = Registry::new();

/// Flag indicating registration is complete (after boot, no more changes).
static REGISTRATION_DONE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register a counter descriptor.
///
/// Call during boot initialization only (before `seal()`).
/// Thread-safety: only called from the BSP before APs are started.
///
/// # Safety
///
/// Must only be called during single-threaded boot initialization.
pub unsafe fn register(desc: &'static CounterDesc) {
    // SAFETY: Called only during single-threaded boot.
    let reg = unsafe { &mut *core::ptr::addr_of_mut!(REGISTRY) };
    if reg.count < MAX_COUNTERS {
        reg.counters[reg.count] = Some(desc);
        reg.count += 1;
    }
}

/// Seal the registry — no more registrations allowed.
///
/// Call after all subsystems have registered their counters.
pub fn seal() {
    REGISTRATION_DONE.store(true, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Snapshot of a single counter.
#[derive(Debug, Clone)]
pub struct CounterSnapshot {
    /// Subsystem group.
    pub group: &'static str,
    /// Counter name.
    pub name: &'static str,
    /// Current value.
    pub value: u64,
}

/// Get a snapshot of all registered counters.
pub fn snapshot() -> alloc::vec::Vec<CounterSnapshot> {
    // SAFETY: After seal(), the registry is read-only.
    let reg = unsafe { &*core::ptr::addr_of!(REGISTRY) };
    let mut result = alloc::vec::Vec::with_capacity(reg.count);
    for i in 0..reg.count {
        if let Some(desc) = reg.counters[i] {
            result.push(CounterSnapshot {
                group: desc.group,
                name: desc.name,
                value: desc.value.load(Ordering::Relaxed),
            });
        }
    }
    result
}

/// Get the number of registered counters.
#[must_use]
pub fn count() -> usize {
    // SAFETY: count field is only incremented during boot.
    unsafe { (*core::ptr::addr_of!(REGISTRY)).count }
}

// ---------------------------------------------------------------------------
// Macros
// ---------------------------------------------------------------------------

/// Define a kernel event counter.
///
/// Creates a static `AtomicU64` and a `CounterDesc` that can be registered.
///
/// Usage: `define_counter!(COUNTER_NAME, COUNTER_DESC, "group", "counter_name");`
///
/// The first ident is the counter itself (an `AtomicU64`), the second is the
/// descriptor (a `CounterDesc`).  Both are `pub static` items.
#[macro_export]
macro_rules! define_counter {
    ($name:ident, $desc:ident, $group:expr, $cname:expr) => {
        pub static $name: core::sync::atomic::AtomicU64 =
            core::sync::atomic::AtomicU64::new(0);

        #[doc(hidden)]
        pub static $desc: $crate::kcounters::CounterDesc =
            $crate::kcounters::CounterDesc {
                group: $group,
                name: $cname,
                value: &$name,
            };
    };
}

/// Increment a counter by 1.
#[macro_export]
macro_rules! counter_inc {
    ($name:expr) => {
        $name.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    };
}

/// Add a value to a counter.
#[macro_export]
macro_rules! counter_add {
    ($name:expr, $val:expr) => {
        $name.fetch_add($val, core::sync::atomic::Ordering::Relaxed);
    };
}

// ---------------------------------------------------------------------------
// Built-in counters (aggregated from existing subsystems)
// ---------------------------------------------------------------------------

/// Collect all built-in kernel counters into a single snapshot.
///
/// This pulls values from various subsystem-specific atomics that already
/// exist (rather than requiring every subsystem to use our macro).
/// A bridge between existing per-subsystem counters and the unified view.
pub fn builtin_snapshot() -> alloc::vec::Vec<CounterSnapshot> {
    let mut result = alloc::vec::Vec::new();

    // --- Memory subsystem ---
    let mem = crate::mm::memory_info();
    result.push(CounterSnapshot { group: "mm", name: "total_frames", value: mem.total_frames as u64 });
    result.push(CounterSnapshot { group: "mm", name: "free_frames", value: mem.free_frames as u64 });
    result.push(CounterSnapshot { group: "mm", name: "fragmentation_pct", value: mem.fragmentation_pct as u64 });
    result.push(CounterSnapshot { group: "mm", name: "pcpu_cache_hits", value: mem.pcpu_cache_hits });
    result.push(CounterSnapshot { group: "mm", name: "pcpu_cache_misses", value: mem.pcpu_cache_misses });
    result.push(CounterSnapshot { group: "mm", name: "heap_slab_allocs", value: mem.heap_slab_allocs });
    result.push(CounterSnapshot { group: "mm", name: "heap_slab_frees", value: mem.heap_slab_frees });
    result.push(CounterSnapshot { group: "mm", name: "heap_large_allocs", value: mem.heap_large_allocs });
    result.push(CounterSnapshot { group: "mm", name: "oom_events", value: mem.oom_events });

    // --- Scheduler subsystem ---
    let sched = crate::sched::sched_stats();
    result.push(CounterSnapshot { group: "sched", name: "ctx_switches", value: sched.total_ctx_switches });
    result.push(CounterSnapshot { group: "sched", name: "work_steals", value: sched.total_work_steals });
    result.push(CounterSnapshot { group: "sched", name: "tasks_spawned", value: sched.total_tasks_spawned });
    result.push(CounterSnapshot { group: "sched", name: "tasks_exited", value: sched.total_tasks_exited });
    result.push(CounterSnapshot { group: "sched", name: "load_avg_x100", value: sched.load_avg_x100 });

    // --- Interrupt subsystem ---
    let irq_counts = crate::idt::vector_counts();
    let total_irqs: u64 = irq_counts.iter().sum();
    result.push(CounterSnapshot { group: "irq", name: "total_interrupts", value: total_irqs });
    result.push(CounterSnapshot { group: "irq", name: "timer_irqs", value: irq_counts[32] });
    result.push(CounterSnapshot { group: "irq", name: "storms_detected", value: u64::from(crate::irq_storm::total_storms()) });

    // --- Softirq ---
    let softirq = crate::softirq::stats();
    result.push(CounterSnapshot { group: "softirq", name: "total_runs", value: u64::from(softirq.total_runs) });
    result.push(CounterSnapshot { group: "softirq", name: "total_handlers", value: u64::from(softirq.total_handlers) });
    result.push(CounterSnapshot { group: "softirq", name: "reentry_prevented", value: u64::from(softirq.reentry_prevented) });

    // --- Syscall latency ---
    let slat = crate::sclatency::stats();
    result.push(CounterSnapshot { group: "syscall", name: "total_calls", value: slat.total_calls });
    result.push(CounterSnapshot { group: "syscall", name: "mean_ns", value: slat.mean_ns });

    // --- Process accounting ---
    result.push(CounterSnapshot { group: "pacct", name: "exits_recorded", value: crate::pacct::total_recorded() });

    result
}

extern crate alloc;
