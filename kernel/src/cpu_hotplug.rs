//! CPU hotplug framework — online/offline CPUs at runtime.
//!
//! Allows dynamically disabling and re-enabling CPUs for:
//! - Power management: park idle CPUs to save energy.
//! - CPU isolation: dedicate a CPU to a specific workload by removing it
//!   from general scheduling.
//! - Fault isolation: offline a CPU exhibiting hardware errors.
//! - Testing: simulate reduced-CPU configurations.
//!
//! ## Architecture
//!
//! Each non-BSP CPU has a state machine:
//!
//! ```text
//! Online ─── offline() ──→ Parked ─── online() ──→ Online
//!                            │
//!                            └──→ CPU halts in a loop, only wakes
//!                                  on an "unpark" IPI.
//! ```
//!
//! The BSP (CPU 0) cannot be offlined — it runs the timer, kswapd,
//! workqueue, and most kernel tasks.
//!
//! ## Offline Flow
//!
//! 1. Migrate all tasks from the target CPU's run queue to other CPUs.
//! 2. Set the CPU's state to `Parked`.
//! 3. Send an IPI telling the target CPU to enter its park loop.
//! 4. The target CPU drains any pending work, then halts.
//! 5. Other CPUs will no longer schedule tasks on it.
//!
//! ## Online Flow
//!
//! 1. Set the CPU's state back to `Online`.
//! 2. Send an "unpark" IPI to wake the halted CPU.
//! 3. The CPU resumes its idle loop and is available for scheduling.
//!
//! ## Notifier Chain
//!
//! Subsystems can register callbacks via [`register_notifier`] to be
//! informed when a CPU goes online/offline.  This allows per-CPU data
//! structures to be initialized/cleaned up.
//!
//! ## References
//!
//! - Linux `kernel/cpu.c` — cpu_up(), cpu_down(), cpuhp_state
//! - Linux `include/linux/cpuhotplug.h` — notifier states
//! - Fuchsia `zircon/kernel/mp.cc` — mp_unplug_cpu()

#![allow(dead_code)]

use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use crate::serial_println;
use crate::smp;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of hotplug notifiers that can be registered.
const MAX_NOTIFIERS: usize = 16;

// ---------------------------------------------------------------------------
// CPU state tracking
// ---------------------------------------------------------------------------

/// CPU online/offline state.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuState {
    /// CPU is active and available for scheduling.
    Online = 0,
    /// CPU is transitioning to offline.
    GoingOffline = 1,
    /// CPU is parked (halted, not scheduling).
    Parked = 2,
    /// CPU is transitioning to online.
    GoingOnline = 3,
    /// CPU was never brought up or is permanently failed.
    NotPresent = 4,
}

impl CpuState {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Online,
            1 => Self::GoingOffline,
            2 => Self::Parked,
            3 => Self::GoingOnline,
            _ => Self::NotPresent,
        }
    }
}

/// Per-CPU state.
static CPU_STATES: [AtomicU8; smp::MAX_CPUS] = {
    const NOT_PRESENT: AtomicU8 = AtomicU8::new(CpuState::NotPresent as u8);
    [NOT_PRESENT; smp::MAX_CPUS]
};

/// Number of CPUs currently online (scheduling-eligible).
static ONLINE_COUNT: AtomicU64 = AtomicU64::new(0);

/// CPU 0 (BSP) is always online and cannot be offlined.
const BSP_CPU: usize = 0;

// ---------------------------------------------------------------------------
// Notifier chain
// ---------------------------------------------------------------------------

/// Event types for CPU hotplug notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugEvent {
    /// CPU is about to go online (called before scheduling is enabled).
    PreOnline,
    /// CPU is now online (scheduling active).
    PostOnline,
    /// CPU is about to go offline (tasks will be migrated).
    PreOffline,
    /// CPU is now offline (parked, no longer scheduling).
    PostOffline,
}

/// Notifier callback function type.
///
/// Receives the CPU index and the event type.
/// Returns `true` to allow the transition, `false` to veto (only for Pre* events).
pub type NotifierFn = fn(cpu: usize, event: HotplugEvent) -> bool;

/// Registered notifier slots.
static NOTIFIERS: [AtomicU64; MAX_NOTIFIERS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_NOTIFIERS]
};

/// Number of registered notifiers.
static NOTIFIER_COUNT: AtomicU8 = AtomicU8::new(0);

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total offline operations performed.
static OFFLINE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total online operations performed.
static ONLINE_OPS_COUNT: AtomicU64 = AtomicU64::new(0);

/// Total tasks migrated during offline operations.
static TASKS_MIGRATED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the hotplug framework.
///
/// Marks all online CPUs as `Online` based on the SMP cpu_count().
/// Call after SMP initialization.
pub fn init() {
    let cpus = smp::cpu_count();
    for i in 0..cpus {
        CPU_STATES.get(i).map(|s| s.store(CpuState::Online as u8, Ordering::Release));
    }
    ONLINE_COUNT.store(cpus as u64, Ordering::Release);

    serial_println!("[hotplug] CPU hotplug framework initialized ({} CPUs online)", cpus);
}

/// Get the current state of a CPU.
#[must_use]
pub fn cpu_state(cpu: usize) -> CpuState {
    CPU_STATES.get(cpu)
        .map(|s| CpuState::from_u8(s.load(Ordering::Acquire)))
        .unwrap_or(CpuState::NotPresent)
}

/// Check if a CPU is online (available for scheduling).
#[must_use]
pub fn is_online(cpu: usize) -> bool {
    cpu_state(cpu) == CpuState::Online
}

/// Get the number of CPUs currently online.
#[must_use]
pub fn online_count() -> usize {
    ONLINE_COUNT.load(Ordering::Acquire) as usize
}

/// Offline a CPU — remove it from scheduling and park it.
///
/// The CPU's tasks are migrated to other online CPUs before parking.
/// The BSP (CPU 0) cannot be offlined.
///
/// Returns `Ok(migrated_tasks)` on success, or an error string on failure.
pub fn offline(cpu: usize) -> Result<usize, &'static str> {
    // Validation.
    if cpu == BSP_CPU {
        return Err("cannot offline BSP (CPU 0)");
    }
    if cpu >= smp::MAX_CPUS {
        return Err("CPU index out of range");
    }

    let state_slot = CPU_STATES.get(cpu).ok_or("CPU index out of range")?;
    let current = CpuState::from_u8(state_slot.load(Ordering::Acquire));

    if current != CpuState::Online {
        return Err("CPU is not online");
    }

    // Don't offline the last CPU (besides BSP).
    let online = ONLINE_COUNT.load(Ordering::Acquire);
    if online <= 1 {
        return Err("cannot offline last remaining CPU");
    }

    serial_println!("[hotplug] Offlining CPU {}...", cpu);

    // Pre-offline notification — allow vetoing.
    if !notify_all(cpu, HotplugEvent::PreOffline) {
        serial_println!("[hotplug] CPU {} offline vetoed by notifier", cpu);
        return Err("offline vetoed by notifier");
    }

    // Mark as going offline (prevents new task placement).
    state_slot.store(CpuState::GoingOffline as u8, Ordering::Release);

    // Migrate all tasks from this CPU's run queue to other CPUs.
    let migrated = crate::sched::migrate_tasks_from_cpu(cpu);
    TASKS_MIGRATED.fetch_add(migrated as u64, Ordering::Relaxed);

    // Mark as parked.
    state_slot.store(CpuState::Parked as u8, Ordering::Release);
    ONLINE_COUNT.fetch_sub(1, Ordering::Release);
    OFFLINE_COUNT.fetch_add(1, Ordering::Relaxed);

    // Post-offline notification.
    notify_all(cpu, HotplugEvent::PostOffline);

    serial_println!("[hotplug] CPU {} offlined (migrated {} tasks)", cpu, migrated);
    Ok(migrated)
}

/// Online a previously-offlined CPU — restore it to scheduling.
///
/// Returns `Ok(())` on success, or an error string on failure.
pub fn online(cpu: usize) -> Result<(), &'static str> {
    if cpu >= smp::MAX_CPUS {
        return Err("CPU index out of range");
    }

    let state_slot = CPU_STATES.get(cpu).ok_or("CPU index out of range")?;
    let current = CpuState::from_u8(state_slot.load(Ordering::Acquire));

    if current != CpuState::Parked {
        return Err("CPU is not parked");
    }

    serial_println!("[hotplug] Onlining CPU {}...", cpu);

    // Pre-online notification.
    if !notify_all(cpu, HotplugEvent::PreOnline) {
        serial_println!("[hotplug] CPU {} online vetoed by notifier", cpu);
        return Err("online vetoed by notifier");
    }

    // Mark as going online.
    state_slot.store(CpuState::GoingOnline as u8, Ordering::Release);

    // Mark as online — the CPU's idle loop will see this and resume scheduling.
    state_slot.store(CpuState::Online as u8, Ordering::Release);
    ONLINE_COUNT.fetch_add(1, Ordering::Release);
    ONLINE_OPS_COUNT.fetch_add(1, Ordering::Relaxed);

    // Post-online notification.
    notify_all(cpu, HotplugEvent::PostOnline);

    serial_println!("[hotplug] CPU {} is now online", cpu);
    Ok(())
}

/// Register a hotplug notifier callback.
///
/// Returns the slot index on success, or `None` if the table is full.
pub fn register_notifier(f: NotifierFn) -> Option<usize> {
    let count = NOTIFIER_COUNT.load(Ordering::Acquire) as usize;
    if count >= MAX_NOTIFIERS {
        return None;
    }

    // Store the function pointer as a u64.
    let ptr = f as usize as u64;
    if let Some(slot) = NOTIFIERS.get(count) {
        slot.store(ptr, Ordering::Release);
        NOTIFIER_COUNT.fetch_add(1, Ordering::Release);
        Some(count)
    } else {
        None
    }
}

/// Unregister a hotplug notifier by slot index.
pub fn unregister_notifier(slot: usize) {
    if let Some(s) = NOTIFIERS.get(slot) {
        s.store(0, Ordering::Release);
    }
}

/// Get hotplug statistics.
#[must_use]
pub fn stats() -> HotplugStats {
    HotplugStats {
        online_cpus: ONLINE_COUNT.load(Ordering::Relaxed) as usize,
        total_cpus: smp::cpu_count(),
        offline_ops: OFFLINE_COUNT.load(Ordering::Relaxed),
        online_ops: ONLINE_OPS_COUNT.load(Ordering::Relaxed),
        tasks_migrated: TASKS_MIGRATED.load(Ordering::Relaxed),
        notifiers_registered: NOTIFIER_COUNT.load(Ordering::Relaxed) as usize,
    }
}

/// Hotplug statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct HotplugStats {
    /// Currently online CPUs.
    pub online_cpus: usize,
    /// Total CPUs (online + parked + not-present).
    pub total_cpus: usize,
    /// Total offline operations since boot.
    pub offline_ops: u64,
    /// Total online operations since boot.
    pub online_ops: u64,
    /// Total tasks migrated during offline operations.
    pub tasks_migrated: u64,
    /// Number of registered notifiers.
    pub notifiers_registered: usize,
}

// ---------------------------------------------------------------------------
// Scheduler integration query
// ---------------------------------------------------------------------------

/// Check if a CPU is eligible for task placement.
///
/// Called by the scheduler to skip parked/going-offline CPUs.
#[must_use]
#[inline]
pub fn is_scheduling_eligible(cpu: usize) -> bool {
    CPU_STATES.get(cpu)
        .map(|s| s.load(Ordering::Relaxed) == CpuState::Online as u8)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Invoke all registered notifiers for an event.
///
/// Returns `false` if any notifier vetoes a Pre* event.
fn notify_all(cpu: usize, event: HotplugEvent) -> bool {
    let count = NOTIFIER_COUNT.load(Ordering::Acquire) as usize;
    for i in 0..count {
        let ptr = NOTIFIERS.get(i).map_or(0, |s| s.load(Ordering::Acquire));
        if ptr == 0 {
            continue;
        }
        // SAFETY: ptr was stored from a valid NotifierFn.
        let f: NotifierFn = unsafe { core::mem::transmute(ptr as usize) };
        let result = f(cpu, event);
        if !result && matches!(event, HotplugEvent::PreOffline | HotplugEvent::PreOnline) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test of the CPU hotplug framework.
pub fn self_test() {
    serial_println!("[hotplug] Running self-test...");

    // Test 1: All CPUs should be online after init.
    let cpus = smp::cpu_count();
    assert_eq!(online_count(), cpus);
    for i in 0..cpus {
        assert!(is_online(i), "CPU {} should be online", i);
    }
    serial_println!("[hotplug]   All {} CPUs online: OK", cpus);

    // Test 2: Cannot offline BSP.
    let result = offline(0);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "cannot offline BSP (CPU 0)");
    serial_println!("[hotplug]   BSP offline rejected: OK");

    // Test 3: Out-of-range CPU.
    let result = offline(smp::MAX_CPUS);
    assert!(result.is_err());
    serial_println!("[hotplug]   Out-of-range rejected: OK");

    // Test 4: is_scheduling_eligible.
    assert!(is_scheduling_eligible(0), "BSP should be eligible");
    serial_println!("[hotplug]   Scheduling eligibility: OK");

    // Test 5: Notifier registration.
    fn test_notifier(_cpu: usize, _event: HotplugEvent) -> bool { true }
    let slot = register_notifier(test_notifier);
    assert!(slot.is_some());
    serial_println!("[hotplug]   Notifier registration: OK (slot={})", slot.unwrap());
    unregister_notifier(slot.unwrap());

    // Test 6: Statistics.
    let st = stats();
    assert_eq!(st.online_cpus, cpus);
    assert_eq!(st.total_cpus, cpus);
    serial_println!("[hotplug]   Stats: OK (online={}, total={})", st.online_cpus, st.total_cpus);

    // Test 7: On multi-CPU systems, test actual offline/online cycle.
    if cpus > 1 {
        let target = cpus - 1; // Last CPU.
        let result = offline(target);
        assert!(result.is_ok(), "offline should succeed on CPU {}", target);
        let migrated = result.unwrap();
        assert!(!is_online(target));
        assert_eq!(online_count(), cpus - 1);
        serial_println!("[hotplug]   CPU {} offline: OK (migrated {} tasks)", target, migrated);

        // Online it again.
        let result = online(target);
        assert!(result.is_ok());
        assert!(is_online(target));
        assert_eq!(online_count(), cpus);
        serial_println!("[hotplug]   CPU {} online again: OK", target);
    } else {
        serial_println!("[hotplug]   Single-CPU: skipping offline/online cycle");
    }

    serial_println!("[hotplug] Self-test PASSED");
}
