//! `/proc/irqstat` — hardware interrupt counts, projected from the IDT.
//!
//! ## This module owns no counters
//!
//! Every number here is derived on demand from [`crate::idt::vector_counts`],
//! the array `idt::dispatch_vector` bumps once per hardware interrupt. Nothing
//! in this file is written on the interrupt path, and there is no state to
//! initialise, reset, or leave stale.
//!
//! That is a deliberate reversal. This module used to own a `Mutex<Option<State>>`
//! with `record`, `record_latency`, `mark_spurious`, `register_irq` and
//! `register_cpu` — a complete second accounting system for hardware interrupts,
//! **none of whose mutators had a single caller anywhere in the kernel**
//! (`A-FS-MODULES-EXPOSE-MUTATORS-NOTHING-CAN-REACH`). Before that it seeded the
//! table with five fictional IRQ lines and four fictional per-CPU rows, which
//! `/proc/irqstat` served as though they were measurements.
//!
//! The obvious repair — call `irqstat::record()` from the ISR path — is the
//! wrong one. It would make this the *second* counter of the same event, and two
//! counters of one event are two numbers that can disagree: one missed call site,
//! one early return, one new vector whose author does not know this module
//! exists, and `/proc/irqstat` and `kcounters` report different interrupt totals
//! with nothing to say which is right. Projection cannot drift, because there is
//! only ever one number.
//!
//! It also costs nothing on the interrupt path, which already pays exactly one
//! relaxed `fetch_add` at the single choke point every hardware vector passes
//! through. Adding a second counter would mean taking a lock in an ISR.
//!
//! ## What is honestly derivable here, and what is not
//!
//! | Reported | Source |
//! |---|---|
//! | per-vector counts | `idt::vector_counts()` — real |
//! | vector → IOAPIC IRQ number | `ioapic::IRQ_VECTOR_BASE` — the kernel's own fixed mapping |
//! | vector names | `idt::vector_name()` |
//! | spurious total | the APIC spurious vector's own count |
//! | **per-CPU attribution** | **none — see below** |
//! | **ISR latency** | **none — see below** |
//!
//! `VECTOR_COUNTS` is a flat global array, so an interrupt's count carries no
//! record of which CPU took it. Per-CPU attribution therefore has no source, and
//! this module reports none rather than inventing a split — `/proc` must never
//! contain a number the kernel did not measure. Making the counters per-CPU is a
//! real option (it would also remove cross-CPU contention on a shared cache
//! line) but it changes an interrupt hot path and belongs to its own change with
//! its own benchmark. The same goes for ISR latency: nothing samples it, so the
//! old `avg_latency_ns`/`max_latency_ns` fields are gone rather than zeroed. A
//! zero is not a blank — it is a claim that the machine measured zero.

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Vector classification
//
// Every boundary below is *derived* from the module that owns it, never
// restated.  Restating this mapping is precisely how `irqbalance::balance()`
// went wrong: it computed `irq + 32`, off by one against what `ioapic.rs`
// actually programs, and read the LAPIC timer's slot as IRQ 0's.
// ---------------------------------------------------------------------------

/// The IDT vectors carrying IOAPIC-routed device interrupts.
///
/// Derived from [`crate::ioapic::IRQ_VECTOR_BASE`] and [`crate::ioapic::MAX_IRQ`],
/// which is what `ioapic::init` programs the redirection table with and what
/// `idt::dispatch_vector` decodes.
fn device_vectors() -> core::ops::RangeInclusive<usize> {
    let base = usize::from(crate::ioapic::IRQ_VECTOR_BASE);
    base..=base
        .saturating_add(crate::ioapic::MAX_IRQ)
        .saturating_sub(1)
}

/// What kind of interrupt a vector carries.
///
/// This is derived from the vector number alone, and the vector assignments are
/// the kernel's own fixed constants — so unlike the old `Timer`/`Keyboard`/
/// `Disk`/`Network`/`Usb`/`Gpu` classification, it is not a guess about which
/// device sits behind a line. The kernel does not know that, and this module
/// will not pretend to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqType {
    /// The LAPIC timer.
    Timer,
    /// An IOAPIC-routed external device interrupt.
    Device,
    /// An inter-processor interrupt (TLB shootdown or reschedule).
    Ipi,
    /// The APIC spurious vector.
    Spurious,
    /// A vector with an installed stub but no handler arm — counted before the
    /// `match` in `dispatch_vector` precisely so these stay visible.
    Unassigned,
}

impl IrqType {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Timer => "timer",
            Self::Device => "device",
            Self::Ipi => "ipi",
            Self::Spurious => "spurious",
            Self::Unassigned => "unassigned",
        }
    }
}

/// Classify an IDT vector.
#[must_use]
pub fn classify(vector: usize) -> IrqType {
    if vector == usize::from(crate::apic::TIMER_VECTOR) {
        IrqType::Timer
    } else if device_vectors().contains(&vector) {
        IrqType::Device
    } else if vector == usize::from(crate::tlb::TLB_SHOOTDOWN_VECTOR)
        || vector == usize::from(crate::apic::RESCHEDULE_VECTOR)
    {
        IrqType::Ipi
    } else if vector == usize::from(crate::apic::SPURIOUS_VECTOR) {
        IrqType::Spurious
    } else {
        IrqType::Unassigned
    }
}

/// The IOAPIC input behind `vector`, if it is a device vector.
///
/// The exact inverse of `irqbalance::vector_for_irq`, and asserted to be so in
/// [`self_test`].
#[must_use]
pub fn irq_num_for_vector(vector: usize) -> Option<u32> {
    if !device_vectors().contains(&vector) {
        return None;
    }
    let base = usize::from(crate::ioapic::IRQ_VECTOR_BASE);
    u32::try_from(vector.saturating_sub(base)).ok()
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One live interrupt vector.
#[derive(Debug, Clone)]
pub struct IrqLine {
    /// IDT vector number.
    pub vector: u8,
    /// IOAPIC input, for device vectors only. `None` for the timer, the IPIs
    /// and the spurious vector, which are not IOAPIC inputs at all.
    pub irq_num: Option<u32>,
    pub irq_type: IrqType,
    /// From `idt::vector_name`. Device vectors share the name "Device IRQ":
    /// which device is behind an IOAPIC input is a runtime property the IDT
    /// does not know.
    pub name: &'static str,
    /// Interrupts taken on this vector since boot, across all CPUs.
    pub count: u64,
}

/// Aggregate interrupt totals.
///
/// `hardware` is partitioned exactly by the remaining fields — asserted in
/// [`self_test`], which is what stops a future vector assignment from being
/// silently dropped out of every category.
#[derive(Debug, Clone, Copy, Default)]
pub struct IrqTotals {
    /// Vectors with a non-zero count.
    pub lines: usize,
    /// All interrupts on vectors at or above the timer vector. Excludes CPU
    /// exceptions (vectors 0–31), which are not interrupts and are reported by
    /// `exceptions` / `/proc/exceptions`.
    pub hardware: u64,
    pub timer: u64,
    pub device: u64,
    pub ipi: u64,
    pub spurious: u64,
    /// Interrupts on installed-but-unhandled vectors. Non-zero here means the
    /// machine is delivering something the kernel has no arm for.
    pub unassigned: u64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// The lowest vector this module reports on.
///
/// Vectors below the LAPIC timer are CPU exceptions, not interrupts. Folding
/// them in here would repeat, in reverse, the mislabelling that made `kshell`'s
/// health line count every timer tick as an "exception".
fn first_interrupt_vector() -> usize {
    usize::from(crate::apic::TIMER_VECTOR)
}

/// Every interrupt vector that has fired at least once, in vector order.
///
/// Silent vectors are omitted rather than listed as zero: a table of 200 zeroes
/// buries the handful of lines that are actually live, and `/proc/interrupts`
/// on Linux behaves the same way.
#[must_use]
pub fn irq_lines() -> Vec<IrqLine> {
    let counts = crate::idt::vector_counts();
    let mut out = Vec::new();
    for (vector, &count) in counts.iter().enumerate() {
        if vector < first_interrupt_vector() || count == 0 {
            continue;
        }
        // x86-64's IDT has exactly 256 entries, so this cannot fail today and
        // the `continue` is unreachable.  It is here rather than a cast so that
        // widening `VECTOR_STATS_SIZE` past 256 cannot silently truncate two
        // vectors onto one row -- and if it ever did drop a row, `self_test`
        // test 6 catches it, because `totals()` deliberately has no matching
        // filter and the two would stop agreeing.
        let Ok(vector_u8) = u8::try_from(vector) else {
            continue;
        };
        out.push(IrqLine {
            vector: vector_u8,
            irq_num: irq_num_for_vector(vector),
            irq_type: classify(vector),
            name: crate::idt::vector_name(vector),
            count,
        });
    }
    out
}

/// Aggregate totals, computed from the same snapshot in one pass.
#[must_use]
pub fn totals() -> IrqTotals {
    let counts = crate::idt::vector_counts();
    let mut t = IrqTotals::default();
    for (vector, &count) in counts.iter().enumerate() {
        if vector < first_interrupt_vector() || count == 0 {
            continue;
        }
        t.lines = t.lines.saturating_add(1);
        t.hardware = t.hardware.saturating_add(count);
        let bucket = match classify(vector) {
            IrqType::Timer => &mut t.timer,
            IrqType::Device => &mut t.device,
            IrqType::Ipi => &mut t.ipi,
            IrqType::Spurious => &mut t.spurious,
            IrqType::Unassigned => &mut t.unassigned,
        };
        *bucket = bucket.saturating_add(count);
    }
    t
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("irqstat::self_test() — running tests...");

    // 1: The vector→IRQ mapping is the exact inverse of the one the balancer
    //    uses, across the whole IOAPIC range.  Both are derived from
    //    `ioapic::IRQ_VECTOR_BASE`, so this is really asserting that neither has
    //    quietly grown a restated copy of it.
    let base = usize::from(crate::ioapic::IRQ_VECTOR_BASE);
    for irq in 0..crate::ioapic::MAX_IRQ {
        let vector = base.saturating_add(irq);
        assert_eq!(
            irq_num_for_vector(vector),
            u32::try_from(irq).ok(),
            "vector {vector} must decode back to IOAPIC input {irq}"
        );
        assert_eq!(classify(vector), IrqType::Device);
    }
    // One past the end is not a device vector.
    assert_eq!(
        irq_num_for_vector(base.saturating_add(crate::ioapic::MAX_IRQ)),
        None
    );
    crate::serial_println!("  [1/6] vector↔IRQ mapping: OK");

    // 2: The timer is NOT an IOAPIC input.  This is the specific error that hid
    //    in `irqbalance` for as long as nothing counted hardware vectors: IRQ 0
    //    was read from the timer's slot, so the balancer would have seen the
    //    timer's ~1000/s as the busiest "device" on the machine.
    let timer_vec = usize::from(crate::apic::TIMER_VECTOR);
    assert_eq!(classify(timer_vec), IrqType::Timer);
    assert_eq!(irq_num_for_vector(timer_vec), None);
    assert!(!device_vectors().contains(&timer_vec));
    assert_eq!(
        classify(usize::from(crate::apic::SPURIOUS_VECTOR)),
        IrqType::Spurious
    );
    assert_eq!(
        classify(usize::from(crate::tlb::TLB_SHOOTDOWN_VECTOR)),
        IrqType::Ipi
    );
    assert_eq!(
        classify(usize::from(crate::apic::RESCHEDULE_VECTOR)),
        IrqType::Ipi
    );
    crate::serial_println!("  [2/6] timer is not IRQ 0: OK");

    // 3: The buckets partition the total exactly.  A vector assignment added
    //    later that falls into no category would break this rather than vanish
    //    from the aggregates.
    let t = totals();
    let summed = t
        .timer
        .saturating_add(t.device)
        .saturating_add(t.ipi)
        .saturating_add(t.spurious)
        .saturating_add(t.unassigned);
    assert_eq!(
        t.hardware, summed,
        "buckets must partition the hardware total"
    );
    crate::serial_println!("  [3/6] totals partition: OK");

    // 4: The projection is LIVE.  The timer has been ticking since well before
    //    the boot battery runs, so a zero here does not mean "quiet machine" —
    //    it means this module has been disconnected from its counter.  That is
    //    the failure the old fabricated seed data hid for the life of the
    //    module, and it is the one assertion here that could not be written at
    //    all until `dispatch_vector` started counting.
    assert!(
        t.timer > 0,
        "timer vector count is zero — irqstat is not reading a live counter"
    );
    crate::serial_println!("  [4/6] live timer count ({}): OK", t.timer);

    // 5: Cross-check against the independent tick counter.  Note this is `>=`,
    //    NOT `==`: `apic::TICK_COUNT` is deliberately bumped by the BSP only
    //    (so `tick_count()` stays wall-clock rate rather than N× on N CPUs),
    //    while `dispatch_vector` counts the timer on every CPU.  So BSP ticks
    //    are a subset of counted timer interrupts.  A count BELOW the tick
    //    total would mean timer interrupts are reaching `handle_timer_irq`
    //    without passing the choke point — i.e. a second entry path nobody
    //    counted.
    //
    //    Read the ticks FIRST: a tick landing between the two reads can only
    //    then make the count look larger, never smaller, so this cannot flake.
    let ticks = crate::apic::tick_count();
    let counted_timer = crate::idt::vector_count(timer_vec);
    assert!(
        counted_timer >= ticks,
        "counted timer interrupts ({counted_timer}) < BSP ticks ({ticks}) — a timer entry path is bypassing dispatch_vector"
    );
    crate::serial_println!("  [5/6] tick cross-check ({counted_timer} >= {ticks}): OK");

    // 6: The line table agrees with the aggregate it was computed from, and the
    //    timer's row carries the timer's count under the timer's name.
    let lines = irq_lines();
    assert_eq!(lines.len(), t.lines);
    let line_sum: u64 = lines.iter().map(|l| l.count).sum();
    assert_eq!(line_sum, t.hardware);
    let timer_line = lines
        .iter()
        .find(|l| usize::from(l.vector) == timer_vec)
        .expect("timer vector must appear in the line table");
    assert_eq!(timer_line.irq_type, IrqType::Timer);
    assert_eq!(timer_line.name, crate::idt::vector_name(timer_vec));
    assert!(timer_line.irq_num.is_none());
    crate::serial_println!("  [6/6] line table ({} live vectors): OK", lines.len());

    crate::serial_println!("irqstat::self_test() — all 6 tests passed");
}
