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

use crate::serial_println;
use crate::smp;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

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

/// Whether [`init`] has run.
///
/// Only used to distinguish a CPU that registered itself *before* the
/// framework was initialized (the ordinary case: every AP that made
/// `smp::init`'s bounded wait) from one that registered *after* (a straggler
/// that missed the window). Both are handled identically; the second is worth
/// a line in the serial log because it is the case that used to be silently
/// lost, and because seeing it means the AP bring-up wait is running tight on
/// this host.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

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
/// Marks every CPU the SMP layer has brought up as `Online`. Call after SMP
/// initialization.
///
/// **This is a floor, not the whole truth, and must not be the only way a CPU
/// becomes online.** `smp::init()` waits for application processors on a
/// *bounded* spin (~50 ms), so an AP that misses that window finishes booting
/// after `init()` has already run and would never be marked online here. That
/// is why [`mark_online_self`] exists and why this function recomputes
/// `ONLINE_COUNT` from the states rather than storing `cpu_count()` over it:
/// an AP that self-registered before this ran must not be counted twice, and
/// one that self-registers after must not be erased.
pub fn init() {
    let cpus = smp::cpu_count();
    for i in 0..cpus {
        if let Some(s) = CPU_STATES.get(i) {
            // `compare_exchange` rather than `store`: an AP that already
            // called `mark_online_self` is *also* already counted in
            // `ONLINE_COUNT`, and a blind store would let the recount below
            // agree while silently discarding a `GoingOffline` in progress.
            let _ = s.compare_exchange(
                CpuState::NotPresent as u8,
                CpuState::Online as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }

    // Recount rather than assume. `cpus` is a snapshot that a late AP can
    // already have invalidated by the time this line runs.
    let online = (0..smp::MAX_CPUS)
        .filter(|&i| {
            CPU_STATES
                .get(i)
                .is_some_and(|s| CpuState::from_u8(s.load(Ordering::Acquire)) == CpuState::Online)
        })
        .count();
    ONLINE_COUNT.store(online as u64, Ordering::Release);
    INITIALIZED.store(true, Ordering::Release);

    serial_println!("[hotplug] CPU hotplug framework initialized ({online} CPUs online)");
}

/// Register the calling CPU as online, from the CPU itself, during its own
/// bring-up.
///
/// # Why this exists
///
/// `smp::init()` waits for APs on a bounded ~50 ms spin and then returns
/// regardless. An AP that finishes after that window bumps `NUM_CPUS_ONLINE`
/// itself and proceeds to register an idle task, install its IRQ stack and
/// `sti()` — it is, from that moment, a CPU running real work. Before this
/// function existed, nothing told `cpu_hotplug`, so such a CPU stayed
/// `NotPresent` for the rest of the boot while actually executing tasks.
///
/// That was not cosmetic. Three subsystems consult [`is_online`] and would
/// each have drawn the wrong conclusion about a live CPU:
///
/// | Consumer | What it does with an "offline" CPU |
/// |---|---|
/// | `rcu::synchronize_rcu` | **Skips waiting for it.** A grace period could complete while that CPU was inside an RCU read-side critical section, and the caller would then free memory still being read. |
/// | `sched` load balancing | Never migrates or steals work to it — a whole core stays idle under load. |
/// | `irqbalance` | Never routes an interrupt to it. |
///
/// The RCU one is a use-after-free window, which is why an AP announces
/// itself here rather than leaving `init()`'s snapshot to be corrected later.
///
/// # Ordering
///
/// Safe to call before or after [`init`]: the transition is a
/// `compare_exchange` from a non-`Online` state, so whichever runs second is
/// a no-op and `ONLINE_COUNT` is incremented exactly once either way.
///
/// Returns `true` if this call performed the transition.
pub fn mark_online_self(cpu: usize) -> bool {
    let Some(slot) = CPU_STATES.get(cpu) else {
        return false;
    };

    // Only ever NotPresent -> Online here. An AP must not be able to undo a
    // deliberate `offline()` that raced with its bring-up: `Parked` and
    // `GoingOffline` are decisions made by the BSP and are not ours to revert.
    if slot
        .compare_exchange(
            CpuState::NotPresent as u8,
            CpuState::Online as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return false;
    }

    ONLINE_COUNT.fetch_add(1, Ordering::AcqRel);

    if INITIALIZED.load(Ordering::Acquire) {
        // The case this function was written for. Not an error -- the CPU is
        // now correctly accounted for -- but worth seeing, because it means
        // `smp::init`'s bounded AP wait expired before this core reported in.
        serial_println!("[hotplug] CPU {cpu} registered after init (late AP), now online");
    }

    // Tell the subsystems that keep per-CPU views. `PostOnline` rather than
    // `PreOnline` because the CPU is already running by the time it gets
    // here -- there is nothing left to veto, and a notifier returning `false`
    // could not un-start it. The return value is deliberately ignored for
    // that reason.
    let _ = notify_all(cpu, HotplugEvent::PostOnline);
    true
}

/// Get the current state of a CPU.
#[must_use]
pub fn cpu_state(cpu: usize) -> CpuState {
    CPU_STATES
        .get(cpu)
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

    serial_println!(
        "[hotplug] CPU {} offlined (migrated {} tasks)",
        cpu,
        migrated
    );
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
    CPU_STATES
        .get(cpu)
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
    let mut skips = crate::fs::selftest::Skips::new();

    // Test 1: `ONLINE_COUNT` is exactly the number of CPUs in state `Online`.
    //
    // This is the invariant that makes the counter trustworthy, and it is the
    // one the old version of this test could not state. It used to walk
    // `0..online_count()` asserting each index was online, which is only
    // equivalent while the online set happens to be a contiguous prefix -- true
    // at boot, false the moment any CPU in the middle is offlined. Count the
    // states instead, so the test does not quietly depend on that layout.
    //
    // Two independent counters feed this and they are *different*:
    // `smp::NUM_CPUS_ONLINE`, bumped by each AP as it finishes coming up,
    // versus this module's `ONLINE_COUNT`. `init()` no longer copies the former
    // into the latter -- it derives it from the states, exactly as this test
    // does -- because a straggler AP can bump smp's counter at any moment.
    // (`smp::init` waits for APs on a *bounded* ~50 ms spin and then proceeds
    // regardless; a bounded wait is a race window, not a barrier.) Such an AP
    // now calls `mark_online_self`, so it is present in both.
    let recorded = online_count();
    let actually_online = (0..smp::MAX_CPUS).filter(|&i| is_online(i)).count();
    assert_eq!(
        recorded, actually_online,
        "ONLINE_COUNT says {recorded} but {actually_online} CPUs are in state Online"
    );
    assert!(is_online(BSP_CPU), "BSP must always be online");

    // The framework can never have recorded more CPUs than smp knows about:
    // every path that marks a CPU online runs after that CPU has already bumped
    // `NUM_CPUS_ONLINE` (`mark_online_self`) or after `init()` read it. smp's
    // counter only grows during boot, so a straggler widens the gap between the
    // two readings; it cannot invert it.
    assert!(
        recorded <= smp::cpu_count(),
        "hotplug recorded {} CPUs, more than smp's {}",
        recorded,
        smp::cpu_count()
    );

    // Test 1b: `mark_online_self` is idempotent.
    //
    // The whole scheme rests on it: an AP calls it during bring-up and `init()`
    // may have already marked that CPU, in either order. A second call must not
    // double-count. Re-announcing the BSP is the safe way to check -- it is
    // unconditionally online already, so the call must be refused outright.
    assert!(
        !mark_online_self(BSP_CPU),
        "re-announcing an already-online CPU must not perform a transition"
    );
    assert_eq!(
        online_count(),
        recorded,
        "a refused mark_online_self must leave ONLINE_COUNT alone"
    );
    // Out of range is refused rather than panicking or corrupting the count.
    assert!(
        !mark_online_self(smp::MAX_CPUS),
        "out-of-range CPU index must be refused"
    );
    assert_eq!(
        online_count(),
        recorded,
        "refusal must not change the count"
    );
    serial_println!("[hotplug]   mark_online_self idempotent + range-checked: OK");
    serial_println!("[hotplug]   ONLINE_COUNT == {recorded} CPUs in state Online: OK");

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
    fn test_notifier(_cpu: usize, _event: HotplugEvent) -> bool {
        true
    }
    let slot = register_notifier(test_notifier);
    assert!(slot.is_some());
    serial_println!(
        "[hotplug]   Notifier registration: OK (slot={})",
        slot.unwrap()
    );
    unregister_notifier(slot.unwrap());

    // Test 6: Statistics.
    //
    // `st.online_cpus` is this module's own `ONLINE_COUNT`, so comparing it to
    // `recorded` is a comparison of one counter with itself and is exact --
    // nothing between the two has offlined anything. `st.total_cpus` is a fresh
    // `smp::cpu_count()` taken inside `stats()`, which is the live one, so it
    // gets the inequality that holds no matter when a straggler AP lands.
    let st = stats();
    assert_eq!(st.online_cpus, recorded);
    assert!(
        st.total_cpus >= recorded,
        "stats().total_cpus = {} below the {} CPUs hotplug recorded",
        st.total_cpus,
        recorded
    );
    serial_println!(
        "[hotplug]   Stats: OK (online={}, total={})",
        st.online_cpus,
        st.total_cpus
    );

    // Test 7: On multi-CPU systems, test actual offline/online cycle.
    //
    // The target is the highest index actually in state `Online`, not
    // `recorded - 1`: those coincide only while the online set is a contiguous
    // prefix, and offlining a CPU that is already parked would fail for the
    // wrong reason. Searching the states also keeps this correct for a
    // straggler AP, which `mark_online_self` has now put in the set.
    if recorded > 1 {
        // `unwrap_or(BSP_CPU)` rather than an unwrap: the assertion below is
        // the real check, and it catches the empty case too, since `BSP_CPU`
        // is the one index this branch must never select.
        let target = (0..smp::MAX_CPUS)
            .rev()
            .find(|&i| is_online(i))
            .unwrap_or(BSP_CPU);
        assert_ne!(
            target, BSP_CPU,
            "recorded {recorded} online CPUs but found no offlinable one"
        );
        let result = offline(target);
        assert!(result.is_ok(), "offline should succeed on CPU {}", target);
        let migrated = result.unwrap();
        assert!(!is_online(target));
        assert_eq!(online_count(), recorded - 1);
        serial_println!(
            "[hotplug]   CPU {} offline: OK (migrated {} tasks)",
            target,
            migrated
        );

        // Online it again.
        let result = online(target);
        assert!(result.is_ok());
        assert!(is_online(target));
        assert_eq!(online_count(), recorded);
        serial_println!("[hotplug]   CPU {} online again: OK", target);
    } else {
        // A fact about the machine, not a swallowed error: `cpu_count` is the
        // SMP topology, so this genuinely cannot be exercised here.  It still
        // has to reach the summary, because "PASSED" after a single-CPU boot
        // otherwise looks identical to "PASSED" after a real offline/online.
        skips.record("offline/online cycle", "single-CPU system");
        serial_println!("[hotplug]   Single-CPU: skipping offline/online cycle");
    }

    skips.report("[hotplug]");
    serial_println!("[hotplug] Self-test PASSED{}", skips.suffix());
}
