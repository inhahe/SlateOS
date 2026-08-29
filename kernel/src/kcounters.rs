//! Kernel event counters — one unified view over counters the subsystems
//! already keep.
//!
//! [`builtin_snapshot`] walks the atomics that `mm`, `sched`, `idt`, `softirq`,
//! `sclatency` and `pacct` maintain for their own purposes and presents them
//! under `(group, name, value)` triples.  It is the whole module; the `counters`
//! kshell command is its only caller.
//!
//! ## There is deliberately no registration API
//!
//! This module was originally a *registry*: subsystems would declare counters
//! with a `define_counter!` macro, register the descriptors during boot, and a
//! `snapshot()` would walk the registered set.  That design was bypassed rather
//! than used — every subsystem already had its own atomics, so the aggregator
//! below was written to read those directly, and in the life of the kernel not
//! one caller of the macro ever appeared.
//!
//! The registry was deleted (2026-08-29) rather than left in place, because an
//! unused half is not inert here: `snapshot()` returned an empty vector on every
//! call, `cmd_counters` chained that onto the real list, and the command printed
//! a correct, healthy-looking table in which the dead half was invisible. A
//! plausible zero is not reportable by any test that asks only "did it print
//! something sensible?" — so the mechanism could not have decayed *louder* than
//! it did. Deleting it also removed a `static mut` and an `unsafe fn` whose
//! `# Safety` contract about boot ordering had never been exercised by a call.
//!
//! **If a subsystem ever needs a counter this module cannot reach**, add the
//! atomic where the event happens, expose it through that subsystem's existing
//! stats accessor, and add a row to [`builtin_snapshot`]. That is one place to
//! change and it cannot silently contribute nothing — a row that stops being
//! read stops appearing, rather than appearing as a zero.
//!
//! ## References
//!
//! - Linux `/proc/vmstat` — aggregated VM counters
//! - Fuchsia `kcounters` — kernel counters infrastructure
//! - FreeBSD `kern.stats` — sysctl-based counters

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

// ---------------------------------------------------------------------------
// Built-in counters (aggregated from existing subsystems)
// ---------------------------------------------------------------------------

/// Collect every kernel counter into a single snapshot.
///
/// Pulls values from the subsystem-specific atomics that already exist, which
/// is the whole of this module's job — see the module docs for why there is no
/// registration path beside it.
pub fn builtin_snapshot() -> alloc::vec::Vec<CounterSnapshot> {
    let mut result = alloc::vec::Vec::new();

    // --- Memory subsystem ---
    let mem = crate::mm::memory_info();
    result.push(CounterSnapshot {
        group: "mm",
        name: "total_frames",
        value: mem.total_frames as u64,
    });
    result.push(CounterSnapshot {
        group: "mm",
        name: "free_frames",
        value: mem.free_frames as u64,
    });
    result.push(CounterSnapshot {
        group: "mm",
        name: "fragmentation_pct",
        value: mem.fragmentation_pct as u64,
    });
    result.push(CounterSnapshot {
        group: "mm",
        name: "pcpu_cache_hits",
        value: mem.pcpu_cache_hits,
    });
    result.push(CounterSnapshot {
        group: "mm",
        name: "pcpu_cache_misses",
        value: mem.pcpu_cache_misses,
    });
    result.push(CounterSnapshot {
        group: "mm",
        name: "heap_slab_allocs",
        value: mem.heap_slab_allocs,
    });
    result.push(CounterSnapshot {
        group: "mm",
        name: "heap_slab_frees",
        value: mem.heap_slab_frees,
    });
    result.push(CounterSnapshot {
        group: "mm",
        name: "heap_large_allocs",
        value: mem.heap_large_allocs,
    });
    result.push(CounterSnapshot {
        group: "mm",
        name: "oom_events",
        value: mem.oom_events,
    });

    // --- Scheduler subsystem ---
    let sched = crate::sched::sched_stats();
    result.push(CounterSnapshot {
        group: "sched",
        name: "ctx_switches",
        value: sched.total_ctx_switches,
    });
    result.push(CounterSnapshot {
        group: "sched",
        name: "work_steals",
        value: sched.total_work_steals,
    });
    result.push(CounterSnapshot {
        group: "sched",
        name: "tasks_spawned",
        value: sched.total_tasks_spawned,
    });
    result.push(CounterSnapshot {
        group: "sched",
        name: "tasks_exited",
        value: sched.total_tasks_exited,
    });
    result.push(CounterSnapshot {
        group: "sched",
        name: "load_avg_x100",
        value: sched.load_avg_x100,
    });

    // --- Interrupt subsystem ---
    let irq_counts = crate::idt::vector_counts();
    let total_irqs: u64 = irq_counts.iter().sum();
    result.push(CounterSnapshot {
        group: "irq",
        name: "total_interrupts",
        value: total_irqs,
    });
    // Slot 32 is the APIC timer.  This reported a hard zero for the life of the
    // kernel until `dispatch_vector` started counting -- on a machine whose timer
    // had fired millions of times, which is the exact reading an operator would
    // take as evidence of a wedged timer.  `.get()` rather than `[]` so that
    // shrinking the array below 33 could never turn a diagnostic into a panic.
    result.push(CounterSnapshot {
        group: "irq",
        name: "timer_irqs",
        value: irq_counts.get(32).copied().unwrap_or(0),
    });
    result.push(CounterSnapshot {
        group: "irq",
        name: "storms_detected",
        value: u64::from(crate::irq_storm::total_storms()),
    });

    // --- Softirq ---
    let softirq = crate::softirq::stats();
    result.push(CounterSnapshot {
        group: "softirq",
        name: "total_runs",
        value: u64::from(softirq.total_runs),
    });
    result.push(CounterSnapshot {
        group: "softirq",
        name: "total_handlers",
        value: u64::from(softirq.total_handlers),
    });
    result.push(CounterSnapshot {
        group: "softirq",
        name: "reentry_prevented",
        value: u64::from(softirq.reentry_prevented),
    });

    // --- Syscall latency ---
    let slat = crate::sclatency::stats();
    result.push(CounterSnapshot {
        group: "syscall",
        name: "total_calls",
        value: slat.total_calls,
    });
    result.push(CounterSnapshot {
        group: "syscall",
        name: "mean_ns",
        value: slat.mean_ns,
    });
    // Exported so a monitoring consumer can tell "the histogram is empty" from
    // "the histogram could not measure": non-zero means that many calls have
    // no known duration and are absent from every bucket.
    result.push(CounterSnapshot {
        group: "syscall",
        name: "latency_unbucketed",
        value: slat.uncalibrated,
    });

    // --- Process accounting ---
    result.push(CounterSnapshot {
        group: "pacct",
        name: "exits_recorded",
        value: crate::pacct::total_recorded(),
    });

    result
}

extern crate alloc;
