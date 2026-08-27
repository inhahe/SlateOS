//! IRQ balancer — automatic interrupt load distribution across CPUs.
//!
//! On multi-core systems, interrupts are initially routed to the BSP
//! (CPU 0).  Under high interrupt load, this creates a bottleneck:
//! one CPU handles all device interrupts while others sit idle.
//!
//! The IRQ balancer periodically evaluates per-IRQ interrupt rates and
//! redistributes them across online CPUs to minimize imbalance.
//!
//! ## Algorithm
//!
//! 1. Every [`BALANCE_INTERVAL_TICKS`] ticks (~10 seconds), the balancer
//!    wakes and samples per-vector interrupt counts.
//! 2. Compute the delta (interrupts since last sample) for each IRQ.
//! 3. Sort IRQs by load (highest first).
//! 4. Assign IRQs to CPUs using a greedy bin-packing approach:
//!    assign each IRQ to the CPU with the lowest current total load.
//! 5. If the new assignment differs from the current one, reprogram
//!    the IOAPIC redirection entry.
//!
//! ## Affinity Hints and Pinning
//!
//! - **Pinned IRQs**: An IRQ can be pinned to a specific CPU (e.g., for
//!   NUMA locality or device-specific requirements).  Pinned IRQs are
//!   never moved by the balancer.
//! - **Affinity hints**: Drivers can suggest a preferred CPU set.  The
//!   balancer prefers the hinted CPUs but may override under heavy
//!   imbalance.
//!
//! ## Integration
//!
//! The balancer runs as a check from the softirq/periodic tick path on
//! the BSP.  It doesn't require a dedicated task — just a periodic
//! function call gated by a tick counter.
//!
//! ## References
//!
//! - Linux `kernel/irq/affinity.c` — irq_set_affinity_hint()
//! - Linux userspace `irqbalance` — policy daemon
//! - Windows NDIS RSS (Receive Side Scaling) — NIC interrupt distribution

#![allow(dead_code)]

use crate::serial_println;
use crate::smp;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// How often to run the balancer (in timer ticks at 100 Hz).
/// 1000 ticks = 10 seconds.
const BALANCE_INTERVAL_TICKS: u64 = 1000;

/// Minimum interrupt rate (per interval) for an IRQ to be considered
/// for balancing.  Very low-rate IRQs aren't worth moving.
const MIN_RATE_THRESHOLD: u64 = 10;

/// Maximum number of IRQs we track for balancing.
/// IOAPIC typically has 24 entries; we support up to 48.
const MAX_IRQS: usize = 48;

/// Maximum CPUs for load tracking.
const MAX_CPUS: usize = smp::MAX_CPUS;

// ---------------------------------------------------------------------------
// IRQ state
// ---------------------------------------------------------------------------

/// Per-IRQ balancing state.
struct IrqState {
    /// Whether this IRQ is active (has received at least one interrupt).
    active: AtomicBool,
    /// Current CPU assignment (LAPIC ID).
    current_cpu: AtomicU8,
    /// Whether this IRQ is pinned (exempt from balancing).
    pinned: AtomicBool,
    /// Affinity hint (preferred CPU, 0xFF = no preference).
    hint_cpu: AtomicU8,
    /// Last sampled interrupt count (for computing deltas).
    last_count: AtomicU64,
    /// Interrupts in the last balance interval.
    rate: AtomicU64,
}

impl IrqState {
    const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            current_cpu: AtomicU8::new(0),
            pinned: AtomicBool::new(false),
            hint_cpu: AtomicU8::new(0xFF),
            last_count: AtomicU64::new(0),
            rate: AtomicU64::new(0),
        }
    }
}

/// All tracked IRQ states.
static IRQ_STATES: [IrqState; MAX_IRQS] = {
    const INIT: IrqState = IrqState::new();
    [INIT; MAX_IRQS]
};

/// Global enable flag for the balancer.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Tick counter for scheduling balance runs.
static TICK_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Total balance *passes* run — including those that found nothing to do.
///
/// Not a measure of work done: it is incremented on the "nothing to balance"
/// early return as well as on a completed pass, so it climbs at a steady rate
/// on a perfectly idle machine.  For years it did exactly that while the
/// balancer was incapable of balancing anything, and a rising number under a
/// heading that read "operations performed" is why nobody looked.  `MIGRATIONS`
/// is the counter that answers "did this subsystem ever actually do anything".
static BALANCE_OPS: AtomicU64 = AtomicU64::new(0);

/// Total IRQ migrations performed.
static MIGRATIONS: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the IRQ balancer.
///
/// Call after IOAPIC and SMP are both initialized.
pub fn init() {
    let cpu_count = smp::cpu_count();
    if cpu_count <= 1 {
        serial_println!("[irqbalance] Single CPU — balancer disabled");
        return;
    }

    // Mark all I/O APIC entries as initially routed to CPU 0.
    for state in &IRQ_STATES {
        state.current_cpu.store(0, Ordering::Relaxed);
    }

    ENABLED.store(true, Ordering::Release);
    serial_println!(
        "[irqbalance] Initialized: {} CPUs, interval={}s",
        cpu_count,
        BALANCE_INTERVAL_TICKS / 100
    );
}

/// Called on every timer tick (from BSP softirq context).
///
/// Checks if it's time to run a balance pass.  If so, performs
/// the balancing algorithm.  This is cheap (~1 atomic load) when
/// it's not time to balance.
pub fn tick() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let count = TICK_COUNTER.fetch_add(1, Ordering::Relaxed);
    if !count.is_multiple_of(BALANCE_INTERVAL_TICKS) || count == 0 {
        return;
    }

    balance();
}

/// Pin an IRQ to a specific CPU (prevents balancer from moving it).
///
/// `cpu` is the logical CPU index (0-based), not LAPIC ID.
pub fn pin_irq(irq: u8, cpu: usize) {
    if let Some(state) = IRQ_STATES.get(irq as usize) {
        #[allow(clippy::cast_possible_truncation)]
        state.current_cpu.store(cpu as u8, Ordering::Relaxed);
        state.pinned.store(true, Ordering::Release);
        serial_println!("[irqbalance] IRQ {} pinned to CPU {}", irq, cpu);
    }
}

/// Unpin an IRQ (allow balancer to move it again).
pub fn unpin_irq(irq: u8) {
    if let Some(state) = IRQ_STATES.get(irq as usize) {
        state.pinned.store(false, Ordering::Release);
        serial_println!("[irqbalance] IRQ {} unpinned", irq);
    }
}

/// Set an affinity hint for an IRQ (preferred CPU, non-binding).
///
/// The balancer will prefer this CPU but may override under heavy load.
/// Pass `cpu = 0xFF` (or call `clear_hint`) to remove the hint.
pub fn set_hint(irq: u8, cpu: u8) {
    if let Some(state) = IRQ_STATES.get(irq as usize) {
        state.hint_cpu.store(cpu, Ordering::Relaxed);
    }
}

/// Clear the affinity hint for an IRQ.
pub fn clear_hint(irq: u8) {
    set_hint(irq, 0xFF);
}

/// Enable or disable the balancer at runtime.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
    serial_println!(
        "[irqbalance] {}",
        if enabled { "enabled" } else { "disabled" }
    );
}

/// Check if the balancer is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Get balancer statistics.
#[must_use]
pub fn stats() -> BalanceStats {
    BalanceStats {
        enabled: ENABLED.load(Ordering::Relaxed),
        balance_ops: BALANCE_OPS.load(Ordering::Relaxed),
        migrations: MIGRATIONS.load(Ordering::Relaxed),
        cpu_count: smp::cpu_count(),
    }
}

/// Get per-IRQ information for diagnostics.
#[must_use]
pub fn irq_info() -> alloc::vec::Vec<IrqInfo> {
    let mut result = alloc::vec::Vec::new();
    for (i, state) in IRQ_STATES.iter().enumerate() {
        if !state.active.load(Ordering::Relaxed) {
            continue;
        }
        result.push(IrqInfo {
            irq: i as u8,
            cpu: state.current_cpu.load(Ordering::Relaxed),
            pinned: state.pinned.load(Ordering::Relaxed),
            hint: state.hint_cpu.load(Ordering::Relaxed),
            rate: state.rate.load(Ordering::Relaxed),
        });
    }
    result
}

/// Notify the balancer that an IRQ fired (call from ISR path).
///
/// This marks the IRQ as active so the balancer knows it exists.
/// The actual count comes from `idt::vector_counts()`.
#[inline]
pub fn notify_irq(irq: u8) {
    if let Some(state) = IRQ_STATES.get(irq as usize) {
        // Relaxed store is fine — worst case we miss one interval.
        state.active.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Balance algorithm
// ---------------------------------------------------------------------------

/// IDT vector carrying IOAPIC input `irq`.
///
/// Derived from `ioapic::IRQ_VECTOR_BASE` rather than restated, because
/// restating it is how this went wrong: `balance()` computed `irq + 32`, which
/// is off by one against the mapping `ioapic.rs` actually programs. Vector 32
/// is the LAPIC timer and is not an IOAPIC input at all, so IRQ 0's rate was
/// read from the timer's slot, IRQ 1's from IRQ 0's, and so on down the line.
///
/// The error was undetectable for as long as nothing counted hardware vectors:
/// every slot read zero, so a shifted index read zero too. It stops being
/// undetectable the moment those counts are real — slot 32 then carries ~1000
/// ticks a second, far above `MIN_RATE_THRESHOLD`, so the balancer would have
/// concluded IRQ 0 was the busiest line on the machine and begun migrating it
/// on the strength of the timer's traffic.
#[inline]
fn vector_for_irq(irq: usize) -> usize {
    usize::from(crate::ioapic::IRQ_VECTOR_BASE).saturating_add(irq)
}

/// Perform a balance pass.
///
/// Reads current interrupt counts, computes deltas, and redistributes
/// IRQs across CPUs using greedy bin-packing.
fn balance() {
    let cpu_count = smp::cpu_count();
    if cpu_count <= 1 {
        return;
    }

    // Get current vector counts from IDT.
    let vector_counts = crate::idt::vector_counts();

    // Phase 1: Compute per-IRQ rates (delta since last sample).
    // IRQ vectors start at 32 (hardware interrupts after exceptions).
    let mut irq_rates: [(u8, u64); MAX_IRQS] = [(0, 0); MAX_IRQS];
    let mut active_count = 0usize;

    for (i, state) in IRQ_STATES.iter().enumerate() {
        if !state.active.load(Ordering::Relaxed) {
            continue;
        }

        let vector = vector_for_irq(i);
        let current = vector_counts.get(vector).copied().unwrap_or(0);
        let last = state.last_count.swap(current, Ordering::Relaxed);
        let delta = current.saturating_sub(last);
        state.rate.store(delta, Ordering::Relaxed);

        if delta >= MIN_RATE_THRESHOLD && !state.pinned.load(Ordering::Relaxed) {
            if active_count < MAX_IRQS {
                irq_rates[active_count] = (i as u8, delta);
                active_count += 1;
            }
        }
    }

    if active_count == 0 {
        BALANCE_OPS.fetch_add(1, Ordering::Relaxed);
        return; // Nothing to balance.
    }

    // Phase 2: Sort by rate descending (simple insertion sort — small N).
    let rates = &mut irq_rates[..active_count];
    for i in 1..rates.len() {
        let mut j = i;
        while j > 0 && rates[j].1 > rates[j - 1].1 {
            rates.swap(j, j - 1);
            j -= 1;
        }
    }

    // Phase 3: Greedy bin-packing — assign each IRQ to least-loaded CPU.
    let mut cpu_load = [0u64; MAX_CPUS];
    let online_cpus = online_cpu_list(cpu_count);

    for &(irq, rate) in rates.iter() {
        let state = &IRQ_STATES[irq as usize];

        // Check for affinity hint.
        let hint = state.hint_cpu.load(Ordering::Relaxed);
        let target_cpu = if hint != 0xFF
            && (hint as usize) < cpu_count
            && crate::cpu_hotplug::is_online(hint as usize)
        {
            // Honor hint if the hinted CPU isn't overloaded (< 2x average).
            let avg_load = cpu_load
                .iter()
                .take(online_cpus.len())
                .sum::<u64>()
                .checked_div(online_cpus.len() as u64)
                .unwrap_or(0);
            if cpu_load.get(hint as usize).copied().unwrap_or(u64::MAX) < avg_load.saturating_mul(2)
            {
                hint as usize
            } else {
                find_least_loaded(&cpu_load, &online_cpus)
            }
        } else {
            find_least_loaded(&cpu_load, &online_cpus)
        };

        // Update the CPU's virtual load.
        if let Some(load) = cpu_load.get_mut(target_cpu) {
            *load = load.saturating_add(rate);
        }

        // Check if we need to actually move this IRQ.
        let current = state.current_cpu.load(Ordering::Relaxed) as usize;
        if current != target_cpu {
            // Move the IRQ to the new CPU.
            #[allow(clippy::cast_possible_truncation)]
            let target_lapic = cpu_to_lapic(target_cpu);
            if let Some(lapic_id) = target_lapic {
                // SAFETY: target CPU is online, IOAPIC is initialized.
                unsafe {
                    crate::ioapic::set_irq_affinity(irq, lapic_id);
                }
                #[allow(clippy::cast_possible_truncation)]
                state.current_cpu.store(target_cpu as u8, Ordering::Relaxed);
                MIGRATIONS.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    BALANCE_OPS.fetch_add(1, Ordering::Relaxed);
}

/// Find the CPU with the lowest current load from the online set.
fn find_least_loaded(cpu_load: &[u64; MAX_CPUS], online_cpus: &[usize]) -> usize {
    let mut min_load = u64::MAX;
    let mut min_cpu = 0;
    for &cpu in online_cpus {
        let load = cpu_load.get(cpu).copied().unwrap_or(u64::MAX);
        if load < min_load {
            min_load = load;
            min_cpu = cpu;
        }
    }
    min_cpu
}

/// Get list of online CPUs.
fn online_cpu_list(cpu_count: usize) -> alloc::vec::Vec<usize> {
    let mut list = alloc::vec::Vec::with_capacity(cpu_count);
    for i in 0..cpu_count {
        if crate::cpu_hotplug::is_online(i) {
            list.push(i);
        }
    }
    // Always include at least CPU 0 as fallback.
    if list.is_empty() {
        list.push(0);
    }
    list
}

/// Convert logical CPU index to LAPIC ID.
///
/// Uses the SMP module's CPU-to-LAPIC mapping.
fn cpu_to_lapic(cpu: usize) -> Option<u8> {
    crate::smp::cpu_apic_id(cpu)
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Balancer statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct BalanceStats {
    /// Whether the balancer is enabled.
    pub enabled: bool,
    /// Total balance operations performed.
    pub balance_ops: u64,
    /// Total IRQ migrations (moves from one CPU to another).
    pub migrations: u64,
    /// Number of CPUs in the system.
    pub cpu_count: usize,
}

/// Per-IRQ information for diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct IrqInfo {
    /// IRQ number.
    pub irq: u8,
    /// Currently assigned CPU.
    pub cpu: u8,
    /// Whether this IRQ is pinned.
    pub pinned: bool,
    /// Affinity hint (0xFF = none).
    pub hint: u8,
    /// Interrupt rate in the last balance interval.
    pub rate: u64,
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the IRQ balancer.
pub fn self_test() {
    serial_println!("[irqbalance] Running self-test...");

    // Test 1: Init doesn't panic.
    // (Already called from main if multi-CPU.)
    serial_println!("[irqbalance]   Init: OK");

    // Test 2: Pin/unpin.
    pin_irq(1, 0);
    assert!(IRQ_STATES[1].pinned.load(Ordering::Relaxed));
    assert_eq!(IRQ_STATES[1].current_cpu.load(Ordering::Relaxed), 0);
    unpin_irq(1);
    assert!(!IRQ_STATES[1].pinned.load(Ordering::Relaxed));
    serial_println!("[irqbalance]   Pin/unpin: OK");

    // Test 3: Hint set/clear.
    set_hint(5, 2);
    assert_eq!(IRQ_STATES[5].hint_cpu.load(Ordering::Relaxed), 2);
    clear_hint(5);
    assert_eq!(IRQ_STATES[5].hint_cpu.load(Ordering::Relaxed), 0xFF);
    serial_println!("[irqbalance]   Hint set/clear: OK");

    // Test 4: Notify marks active.
    assert!(!IRQ_STATES[10].active.load(Ordering::Relaxed));
    notify_irq(10);
    assert!(IRQ_STATES[10].active.load(Ordering::Relaxed));
    // Clean up.  This is load-bearing for Test 9, not mere tidiness: Test 9
    // asserts that every line marked active was counted by `idt`, and IRQ 10
    // was marked active here by a direct call with no interrupt behind it.
    // Leaving it set would fail Test 9 with a message blaming the ISR path.
    IRQ_STATES[10].active.store(false, Ordering::Relaxed);
    serial_println!("[irqbalance]   Notify: OK");

    // Test 5: Enable/disable.
    let was = is_enabled();
    set_enabled(false);
    assert!(!is_enabled());
    set_enabled(was);
    serial_println!("[irqbalance]   Enable/disable: OK");

    // Test 6: Stats.
    let st = stats();
    assert_eq!(st.cpu_count, smp::cpu_count());
    serial_println!(
        "[irqbalance]   Stats: OK (ops={}, migrations={}, cpus={})",
        st.balance_ops,
        st.migrations,
        st.cpu_count
    );

    // Test 7: Out-of-range IRQ doesn't panic.
    pin_irq(255, 0);
    notify_irq(255);
    set_hint(255, 0);
    serial_println!("[irqbalance]   Out-of-range safety: OK");

    // Test 8: the IRQ -> vector mapping agrees with the one the IOAPIC
    // programs.  This is the deterministic half of the off-by-one regression:
    // `balance()` used to compute `irq + 32`, reading the LAPIC timer's slot as
    // IRQ 0's rate.  Test 4 above cannot catch that, or anything like it -- it
    // calls `notify_irq` directly and asserts the flag it just set, so it
    // passes identically whether or not anything in the kernel ever calls it.
    assert_eq!(
        vector_for_irq(0),
        crate::ioapic::IRQ_VECTOR_BASE as usize,
        "IRQ 0 must land on IRQ_VECTOR_BASE, not on the LAPIC timer's vector"
    );
    assert_ne!(
        vector_for_irq(0),
        32,
        "vector 32 is the LAPIC timer and is not an IOAPIC input"
    );
    serial_println!(
        "[irqbalance]   IRQ->vector mapping: OK (IRQ 0 -> vector {})",
        vector_for_irq(0)
    );

    // Test 9: evidence that the ISR path actually reaches this module -- and
    // that the vector it reaches through is the one `vector_for_irq` names.
    //
    // Which IOAPIC lines have fired by now is a property of the emulated
    // hardware and of boot ordering, not of this module, so the *count* is
    // reported rather than asserted: requiring that some device have
    // interrupted would be flaky.  What IS asserted is the agreement between
    // the two records of whatever did fire (see below), which is a property of
    // this kernel and holds no matter what the hardware did.
    //
    // If `notify_irq`'s call in `ioapic::handle_device_irq` is ever removed,
    // this prints `none` forever after -- visible and greppable rather than
    // silent.  Note that this self-test is sequenced late enough in boot for
    // that to be meaningful: it runs after `keyboard::init` unmasks IRQ 1,
    // whereas `fs::irqstat::self_test` runs before any device IRQ is unmasked
    // at all and so can only ever observe the timer.
    let mut active: [u8; MAX_IRQS] = [0; MAX_IRQS];
    let mut n_active = 0usize;
    for (i, state) in IRQ_STATES.iter().enumerate() {
        if state.active.load(Ordering::Relaxed) {
            if let (Some(slot), Ok(irq_u8)) = (active.get_mut(n_active), u8::try_from(i)) {
                *slot = irq_u8;
                n_active = n_active.saturating_add(1);
            }
        }
    }
    if n_active == 0 {
        serial_println!("[irqbalance]   Live IRQ lines: none have fired yet");
    } else {
        // Two independent instruments record the same event, and until now
        // neither checked the other.  This module sets `active` from
        // `notify_irq`, called by `ioapic::handle_device_irq`; `idt` counts the
        // raw vector in `dispatch_vector`, one frame earlier on the same path.
        // An IRQ that reached here therefore MUST have been counted there, and
        // in the slot `vector_for_irq` names -- which is the off-by-one this
        // function's doc comment describes, re-checked against live traffic
        // rather than against arithmetic.
        //
        // This is an assertion where the count alone could only ever be a
        // report: it is conditional on what actually fired, so it neither
        // depends on a device happening to interrupt nor on where in the boot
        // this self-test is sequenced.  Nothing fired, nothing is claimed.
        for &irq in active.get(..n_active).unwrap_or(&[]) {
            let vector = vector_for_irq(usize::from(irq));
            let counted = crate::idt::vector_count(vector);
            assert!(
                counted > 0,
                "IRQ {irq} is marked active but vector {vector} was never counted by idt::dispatch_vector. Either the two instruments disagree, or vector_for_irq names the wrong slot, or an earlier test in this function called notify_irq directly and failed to clear the flag (see Test 4's cleanup)"
            );
        }
        serial_println!(
            "[irqbalance]   Live IRQ lines: {} seen via the ISR path {:?} (all cross-checked against idt vector counts): OK",
            n_active,
            active.get(..n_active).unwrap_or(&[])
        );
    }

    serial_println!("[irqbalance] Self-test PASSED");
}
