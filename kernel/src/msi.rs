//! MSI and MSI-X interrupt support for PCI/PCIe devices.
//!
//! Message Signaled Interrupts (MSI/MSI-X) are the modern interrupt
//! delivery mechanism for PCI and PCIe devices.  Instead of sharing
//! physical interrupt lines (INTx), each device writes a message to a
//! specific memory address to signal an interrupt.
//!
//! ## Advantages Over Legacy INTx
//!
//! - **No sharing**: Each interrupt vector is unique to a device (or even
//!   a device queue), eliminating spurious interrupt handling.
//! - **Edge-triggered semantics**: No level-triggered deassert issues.
//! - **Per-queue vectors**: NVMe/network devices can have one interrupt
//!   per completion queue, enabling per-CPU interrupt processing.
//! - **Lower latency**: Direct write to LAPIC, no I/O APIC routing delay.
//!
//! ## MSI Architecture (x86_64)
//!
//! The device writes a 32-bit data value to a specific address:
//! - **Address**: `0xFEE0_0000 | (LAPIC_ID << 12)` — targets a specific CPU.
//! - **Data**: Contains the interrupt vector number (bits 7:0), delivery
//!   mode (bits 10:8), and trigger mode.
//!
//! The LAPIC receives this write as an interrupt with the specified vector.
//!
//! ## MSI-X
//!
//! MSI-X is an extension supporting up to 2048 vectors per device (vs 32
//! for MSI).  Each vector has its own address/data pair in a BAR-mapped
//! table, plus a pending bit array for masking.
//!
//! ## Vector Allocation
//!
//! This module maintains a pool of IDT vectors available for MSI allocation.
//! Vectors 64-223 are reserved for device MSI (below: exceptions, the timer
//! and the I/O APIC's lines; above: the IPIs and the spurious vector). Each
//! has an IDT stub that calls [`dispatch`], which counts the interrupt for
//! the device handle owning the vector and wakes its waiters.
//!
//! ## References
//!
//! - PCI Local Bus Spec 3.0, §6.8 (MSI)
//! - PCI Express Base Spec 4.0, §6.1.4 (MSI-X)
//! - Intel SDM Vol. 3, §10.11 (Message Signalled Interrupts)
//! - Linux `drivers/pci/msi/` — MSI infrastructure

#![allow(dead_code)]

use crate::ipc::waiters::WaiterSet;
use crate::pci;
use crate::serial_println;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Base address for MSI messages (x86_64 LAPIC MSI address format).
/// Bits 31:20 = 0xFEE (fixed), bits 19:12 = destination LAPIC ID.
const MSI_ADDRESS_BASE: u32 = 0xFEE0_0000;

/// First IDT vector available for MSI allocation: above the I/O APIC's
/// lines (`ioapic::IRQ_VECTOR_BASE` 33 up to 56), which an MSI vector must
/// never share -- a device's message would run another line's handler.
pub const MSI_VECTOR_BASE: u8 = 64;

/// Last IDT vector available for MSI allocation: below the IPIs (250-252)
/// and the spurious vector (255).
pub const MSI_VECTOR_MAX: u8 = 223;

/// Total MSI vectors available.
pub const MSI_VECTOR_COUNT: usize = (MSI_VECTOR_MAX - MSI_VECTOR_BASE + 1) as usize;

/// PCI capability IDs.
const PCI_CAP_ID_MSI: u8 = 0x05;
const PCI_CAP_ID_MSIX: u8 = 0x11;

/// MSI message control register bits.
const MSI_CONTROL_ENABLE: u16 = 1 << 0;
const MSI_CONTROL_64BIT: u16 = 1 << 7;
const MSI_CONTROL_PERVECTOR_MASK: u16 = 1 << 8;

/// MSI-X message control register bits.
const MSIX_CONTROL_ENABLE: u16 = 1 << 15;
const MSIX_CONTROL_FUNCTION_MASK: u16 = 1 << 14;

// ---------------------------------------------------------------------------
// Delivery
// ---------------------------------------------------------------------------

/// The device handle each vector's interrupts are counted for (its raw
/// value; 0 for none). Interrupts on a vector no one owns are not counted, so
/// the next owner never inherits them.
static OWNER: [AtomicU64; MSI_VECTOR_COUNT] = {
    const NONE: AtomicU64 = AtomicU64::new(0);
    [NONE; MSI_VECTOR_COUNT]
};

/// Interrupts counted per vector since its owner last took them: added to by
/// the interrupt handler ([`dispatch`]), taken by the owner ([`take`]).
static PENDING: [AtomicU64; MSI_VECTOR_COUNT] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MSI_VECTOR_COUNT]
};

/// The tasks waiting for each vector's interrupts -- blocked in
/// `SYS_PCI_DEVICE_IRQ_WAIT`, or in `SYS_WAIT_MULTIPLE` on the device. Every
/// interrupt wakes them all: whoever takes the count has it, and the rest
/// look again. Changed only under [`DELIVERY`], with interrupts off on the
/// changing CPU ([`with_waiters`]); the interrupt handler reads a set only
/// through `try_lock`, never waiting, so a set busy when an interrupt comes
/// -- held on another CPU -- leaves its wake owed. A `spin::Mutex`, not the
/// kernel's lock-tracked one: it is the one lock an interrupt handler tries.
static WAITERS: [spin::Mutex<WaiterSet>; MSI_VECTOR_COUNT] = {
    const EMPTY: spin::Mutex<WaiterSet> = spin::Mutex::new(WaiterSet::new());
    [EMPTY; MSI_VECTOR_COUNT]
};

/// Set when [`dispatch`] could not wake every waiter of a vector (its set or
/// the scheduler's lock busy), cleared once [`process_deferred_wakes`] has.
static WAKE_OWED: [AtomicBool; MSI_VECTOR_COUNT] = {
    const NO: AtomicBool = AtomicBool::new(false);
    [NO; MSI_VECTOR_COUNT]
};

/// Serialises the owner's operations on a vector -- claiming, releasing,
/// taking, registering a waiter -- so none acts on a vector that has
/// meanwhile changed hands. The interrupt handler never takes it.
static DELIVERY: crate::sync::Mutex<()> = crate::sync::Mutex::new(());

/// The index of `vector` in the pool, if it is one.
fn pool_index(vector: u8) -> Option<usize> {
    (MSI_VECTOR_BASE..=MSI_VECTOR_MAX)
        .contains(&vector)
        .then(|| usize::from(vector.wrapping_sub(MSI_VECTOR_BASE)))
}

/// An interrupt arrived on MSI `vector` (the IDT dispatcher's arm for the
/// pool): counted for its owner, its waiters woken, the local APIC told it is
/// done, and the softirqs it may have raised run -- as an I/O APIC line's
/// handler does. Interrupt context: atomics and a try-lock wake only.
///
/// The count is added to before the waiters are looked at, and a waiter
/// registers itself before it looks at the count, so one of the two always
/// sees the other: an interrupt is never counted unseen by a waiter that
/// missed its wake. (A waiter registering at that very moment holds its set,
/// which the handler then finds busy, and owes the wake.)
pub fn dispatch(vector: u8) {
    crate::cputime::enter_irq();
    crate::ktrace::record(
        crate::ktrace::Category::Irq,
        crate::ktrace::event::IRQ_ENTER,
        u64::from(vector),
        crate::sched::current_cpu_id() as u64,
    );
    if let Some(index) = pool_index(vector)
        && OWNER
            .get(index)
            .is_some_and(|o| o.load(Ordering::SeqCst) != 0)
        && let (Some(pending), Some(owed)) = (PENDING.get(index), WAKE_OWED.get(index))
    {
        pending.fetch_add(1, Ordering::SeqCst);
        if !wake_waiters(index) {
            owed.store(true, Ordering::Release);
            crate::softirq::raise(crate::softirq::IRQ_POLL_SOFTIRQ);
        }
    }
    // SAFETY: called from the interrupt handler for this vector, which the
    // local APIC delivered and now expects to be told is finished.
    unsafe { crate::apic::eoi() };
    // SAFETY: after EOI, at the end of an interrupt handler, as
    // `ioapic::handle_device_irq` runs them.
    unsafe { crate::softirq::process_pending() };
    crate::ktrace::record(
        crate::ktrace::Category::Irq,
        crate::ktrace::event::IRQ_EXIT,
        u64::from(vector),
        crate::sched::current_cpu_id() as u64,
    );
    crate::cputime::exit_irq();
}

/// Make `owner` (a device handle's raw value, not 0) the one `vector`'s
/// interrupts are counted for, starting from nothing. Refused (`false`) for
/// a vector outside the pool or owned already.
#[must_use]
pub fn claim_delivery(vector: u8, owner: u64) -> bool {
    let _serial = DELIVERY.lock();
    let Some(index) = pool_index(vector) else {
        return false;
    };
    let (Some(slot), Some(pending), Some(owed)) =
        (OWNER.get(index), PENDING.get(index), WAKE_OWED.get(index))
    else {
        return false;
    };
    if owner == 0 || slot.load(Ordering::SeqCst) != 0 {
        return false;
    }
    pending.store(0, Ordering::SeqCst);
    owed.store(false, Ordering::Release);
    slot.store(owner, Ordering::SeqCst);
    // Empty: the last release took its waiters. Any left would wait on a
    // vector that is now another device's, so they are woken -- after the
    // lock, as every full wake is -- and said, rather than dropped.
    let left = with_waiters(index, WaiterSet::take_all).unwrap_or_default();
    drop(_serial);
    if !left.is_empty() {
        serial_println!(
            "[msi] BUG: vector {} was claimed with {} waiter(s) left by its last owner; woken",
            vector,
            left.len()
        );
        crate::ipc::waiters::wake_all(left);
    }
    true
}

/// Let go of `vector`'s delivery before the vector is freed, if `owner` has
/// it: no longer counted, its waiters woken to find it gone -- full wakes,
/// this being no interrupt -- and its count dropped.
pub fn release_delivery(vector: u8, owner: u64) {
    let tasks = {
        let _serial = DELIVERY.lock();
        let Some(index) = pool_index(vector) else {
            return;
        };
        if OWNER
            .get(index)
            .is_none_or(|o| o.load(Ordering::SeqCst) != owner)
        {
            return;
        }
        if let Some(slot) = OWNER.get(index) {
            slot.store(0, Ordering::SeqCst);
        }
        if let Some(pending) = PENDING.get(index) {
            pending.store(0, Ordering::SeqCst);
        }
        with_waiters(index, WaiterSet::take_all).unwrap_or_default()
    };
    crate::ipc::waiters::wake_all(tasks);
}

/// Take, and clear, the interrupts counted on `vector` for `owner`: `None`
/// when `owner` no longer has it.
#[must_use]
pub fn take(vector: u8, owner: u64) -> Option<u64> {
    let _serial = DELIVERY.lock();
    let index = pool_index(vector)?;
    if OWNER.get(index)?.load(Ordering::SeqCst) != owner {
        return None;
    }
    Some(PENDING.get(index)?.swap(0, Ordering::SeqCst))
}

/// The interrupts counted on `vector` for `owner`, left where they are: what
/// a readiness test reads. `None` when `owner` no longer has it.
#[must_use]
pub fn pending(vector: u8, owner: u64) -> Option<u64> {
    let index = pool_index(vector)?;
    if OWNER.get(index)?.load(Ordering::SeqCst) != owner {
        return None;
    }
    Some(PENDING.get(index)?.load(Ordering::SeqCst))
}

/// Register `task` as waiting for `vector`, owned by `owner`, beside any
/// other waiters: `InvalidHandle` when `owner` no longer has the vector.
/// Registering twice is registering once.
pub fn register_waiter(vector: u8, owner: u64, task: u64) -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    let _serial = DELIVERY.lock();
    let index = pool_index(vector).ok_or(KernelError::InvalidHandle)?;
    if OWNER
        .get(index)
        .is_none_or(|o| o.load(Ordering::SeqCst) != owner)
    {
        return Err(KernelError::InvalidHandle);
    }
    with_waiters(index, |w| w.insert(task)).ok_or(KernelError::InvalidHandle)
}

/// `task` stops waiting for `vector`, if it waits.
pub fn unregister_waiter(vector: u8, task: u64) {
    let _serial = DELIVERY.lock();
    if let Some(index) = pool_index(vector) {
        with_waiters(index, |w| w.remove(task));
    }
}

/// How many tasks wait for `vector`.
#[must_use]
pub fn waiter_count(vector: u8) -> usize {
    let _serial = DELIVERY.lock();
    pool_index(vector)
        .and_then(|index| with_waiters(index, |w| w.count()))
        .unwrap_or(0)
}

/// Run `f` on the waiter set of the vector at pool `index`, from thread
/// context: the caller holds [`DELIVERY`], which keeps it from being
/// preempted with the set held, and interrupts are off on this CPU for the
/// moment, so an MSI cannot land here to find its set held by the very
/// thread it interrupted -- whose wake would then wait for the next tick.
/// `None` for an index outside the pool.
fn with_waiters<R>(index: usize, f: impl FnOnce(&mut WaiterSet) -> R) -> Option<R> {
    let waiters = WAITERS.get(index)?;
    Some(crate::cpu::without_interrupts(|| f(&mut waiters.lock())))
}

/// Wake every task waiting on the vector at pool `index`, from interrupt
/// context: `false` when that could not all be done -- its set busy, or a
/// wake refused because the scheduler's lock was -- so the wake is owed. An
/// owed wake is made again to every waiter, which costs a waiter one more
/// look at a count it may have taken already.
fn wake_waiters(index: usize) -> bool {
    let Some(waiters) = WAITERS.get(index).and_then(spin::Mutex::try_lock) else {
        return false;
    };
    let mut all = true;
    for task in waiters.iter() {
        all &= crate::sched::try_wake(task);
    }
    all
}

/// Whether `vector` is allocated from the pool.
#[must_use]
pub fn is_allocated(vector: u8) -> bool {
    pool_index(vector)
        .and_then(|index| VECTOR_ALLOCATED.get(index))
        .is_some_and(|allocated| allocated.load(Ordering::Acquire))
}

/// Make the wakes [`dispatch`] owes: only for the vectors whose waiters it
/// could not all wake. Called with the I/O APIC lines' deferred wakes, on
/// each timer tick.
pub fn process_deferred_wakes() {
    for (index, owed) in WAKE_OWED.iter().enumerate() {
        // Cleared before the wakes, not after: an interrupt that finds the
        // set held by this very pass owes its wake afresh, and a clear
        // after the pass would erase that.
        if owed.swap(false, Ordering::AcqRel) && !wake_waiters(index) {
            owed.store(true, Ordering::Release);
        }
    }
}

/// How long a self-test waits for an interrupt it sent: 100 ms by the HPET,
/// and at most this many spins, so a machine without an HPET (whose clock
/// stays 0) fails the test rather than hangs the boot.
const SELF_TEST_SPINS: u32 = 50_000_000;

/// Spin until `ready` or the self-test's bound runs out.
fn wait_briefly(mut ready: impl FnMut() -> bool) -> bool {
    let deadline = crate::hpet::elapsed_ns().saturating_add(100_000_000);
    for _ in 0..SELF_TEST_SPINS {
        if ready() {
            return true;
        }
        if crate::hpet::elapsed_ns() > deadline {
            break;
        }
        core::hint::spin_loop();
    }
    ready()
}

/// An interrupt sent to this CPU on a pool vector reaches [`dispatch`] and is
/// counted for its owner, and [`take`] hands the count over once; an
/// interrupt on a vector no one owns is not counted. Sent as an IPI to the
/// local APIC itself, so it needs no device; run once interrupts are on.
pub fn self_test_delivery() -> crate::error::KernelResult<()> {
    use crate::error::KernelError;
    const OWNER_TAG: u64 = 0xD15_7E57;
    let Some(vector) = alloc_vector() else {
        serial_println!("[msi]   FAIL: delivery: no vector free");
        return Err(KernelError::InternalError);
    };
    // Unowned: the interrupt arrives and is not counted.
    // SAFETY: a self-IPI on a vector the IDT has a stub for (the pool's),
    // which only counts it; the APIC is up (interrupts are on).
    unsafe { crate::apic::send_self_ipi(vector) };
    let _settled = wait_briefly(|| false);
    if !claim_delivery(vector, OWNER_TAG) {
        free_vector(vector);
        serial_println!(
            "[msi]   FAIL: delivery: vector {} could not be claimed",
            vector
        );
        return Err(KernelError::InternalError);
    }
    let inherited = take(vector, OWNER_TAG);
    // Owned: counted once, taken once.
    // SAFETY: as above.
    unsafe { crate::apic::send_self_ipi(vector) };
    let mut first: u64 = 0;
    let _arrived = wait_briefly(|| {
        first = first.saturating_add(take(vector, OWNER_TAG).unwrap_or(0));
        first > 0
    });
    let second = take(vector, OWNER_TAG);
    let stranger = take(vector, OWNER_TAG ^ 1);
    release_delivery(vector, OWNER_TAG);
    let after = take(vector, OWNER_TAG);
    free_vector(vector);
    if inherited != Some(0)
        || first != 1
        || second != Some(0)
        || stranger.is_some()
        || after.is_some()
    {
        serial_println!(
            "[msi]   FAIL: delivery: vector {}: inherited {:?}, counted {} then {:?}, a stranger took {:?}, after release {:?}",
            vector,
            inherited,
            first,
            second,
            stranger,
            after
        );
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[msi]   delivery: OK (vector {}: nothing from before its claim, counted once)",
        vector
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Vector allocation
// ---------------------------------------------------------------------------

/// Bitmap tracking which MSI vectors are allocated.
static VECTOR_ALLOCATED: [AtomicBool; MSI_VECTOR_COUNT] = {
    const FALSE: AtomicBool = AtomicBool::new(false);
    [FALSE; MSI_VECTOR_COUNT]
};

/// Number of vectors currently allocated.
static VECTORS_USED: AtomicU32 = AtomicU32::new(0);

/// Allocate a single MSI vector from the pool.
///
/// Returns the IDT vector number, or `None` if all vectors are exhausted.
pub fn alloc_vector() -> Option<u8> {
    for i in 0..MSI_VECTOR_COUNT {
        if !VECTOR_ALLOCATED[i].swap(true, Ordering::AcqRel) {
            VECTORS_USED.fetch_add(1, Ordering::Relaxed);
            #[allow(clippy::cast_possible_truncation)]
            return Some(MSI_VECTOR_BASE + i as u8);
        }
    }
    None
}

/// Allocate N consecutive MSI vectors (for multi-message MSI).
///
/// Returns the first vector number, or `None` if not enough contiguous
/// vectors are available.
pub fn alloc_vectors(count: usize) -> Option<u8> {
    if count == 0 || count > MSI_VECTOR_COUNT {
        return None;
    }

    // Find a contiguous block of `count` free vectors.
    'outer: for start in 0..=(MSI_VECTOR_COUNT - count) {
        for j in 0..count {
            if VECTOR_ALLOCATED[start + j].load(Ordering::Relaxed) {
                continue 'outer;
            }
        }
        // Found a block — try to claim it.
        for j in 0..count {
            if VECTOR_ALLOCATED[start + j].swap(true, Ordering::AcqRel) {
                // Someone else got it — roll back.
                for k in 0..j {
                    VECTOR_ALLOCATED[start + k].store(false, Ordering::Release);
                }
                continue 'outer;
            }
        }
        VECTORS_USED.fetch_add(count as u32, Ordering::Relaxed);
        #[allow(clippy::cast_possible_truncation)]
        return Some(MSI_VECTOR_BASE + start as u8);
    }
    None
}

/// Free a previously allocated MSI vector.
pub fn free_vector(vector: u8) {
    if vector >= MSI_VECTOR_BASE && vector <= MSI_VECTOR_MAX {
        let idx = (vector - MSI_VECTOR_BASE) as usize;
        if VECTOR_ALLOCATED[idx].swap(false, Ordering::Release) {
            VECTORS_USED.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// Free N consecutive vectors starting at `base`.
pub fn free_vectors(base: u8, count: usize) {
    for i in 0..count {
        #[allow(clippy::cast_possible_truncation)]
        free_vector(base.wrapping_add(i as u8));
    }
}

// ---------------------------------------------------------------------------
// MSI message formatting
// ---------------------------------------------------------------------------

/// Format an MSI address value targeting a specific CPU.
///
/// `dest_lapic_id`: The LAPIC ID of the target CPU.
/// `redirect_hint`: If true, allows lowest-priority delivery.
#[must_use]
pub fn format_address(dest_lapic_id: u8, redirect_hint: bool) -> u32 {
    let mut addr = MSI_ADDRESS_BASE;
    addr |= (u32::from(dest_lapic_id)) << 12;
    if redirect_hint {
        addr |= 1 << 3; // Redirect hint bit.
    }
    addr
}

/// Format an MSI data value for a given vector.
///
/// `vector`: IDT vector number (0-255).
/// `edge_trigger`: If true, edge-triggered (always true for MSI).
/// `assert_level`: If true, assert (always true for MSI).
#[must_use]
pub fn format_data(vector: u8, edge_trigger: bool, assert_level: bool) -> u32 {
    let mut data = u32::from(vector);
    // Delivery mode: fixed (000).
    // Bits 10:8 = 000 (fixed delivery).
    if !edge_trigger {
        data |= 1 << 15; // Level trigger.
    }
    if assert_level {
        data |= 1 << 14; // Assert.
    }
    data
}

// ---------------------------------------------------------------------------
// PCI MSI Capability Configuration
// ---------------------------------------------------------------------------

/// MSI configuration for a PCI device.
#[derive(Debug, Clone, Copy)]
pub struct MsiConfig {
    /// BDF (bus/device/function) of the device.
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    /// Offset of the MSI capability in PCI config space.
    pub cap_offset: u8,
    /// Whether the device supports 64-bit MSI addresses.
    pub is_64bit: bool,
    /// Whether per-vector masking is supported.
    pub per_vector_mask: bool,
    /// Maximum number of vectors the device can use (1, 2, 4, 8, 16, or 32).
    pub max_vectors: u8,
    /// Currently allocated vector (base).
    pub allocated_vector: u8,
    /// Number of vectors allocated.
    pub allocated_count: u8,
}

/// Find and parse the MSI capability for a PCI device.
///
/// Returns `None` if the device doesn't support MSI.
pub fn find_msi_capability(bus: u8, device: u8, function: u8) -> Option<MsiConfig> {
    let cap_offset = find_pci_capability(bus, device, function, PCI_CAP_ID_MSI)?;

    // Read the MSI Message Control register (capability + 2).
    let msg_control = pci::config_read16(bus, device, function, cap_offset.wrapping_add(2));

    let is_64bit = msg_control & MSI_CONTROL_64BIT != 0;
    let per_vector_mask = msg_control & MSI_CONTROL_PERVECTOR_MASK != 0;

    // Multiple Message Capable: bits 3:1 encode log2(max vectors).
    let mmc = (msg_control >> 1) & 0x7;
    let max_vectors = 1u8 << mmc;

    Some(MsiConfig {
        bus,
        device,
        function,
        cap_offset,
        is_64bit,
        per_vector_mask,
        max_vectors,
        allocated_vector: 0,
        allocated_count: 0,
    })
}

/// Enable MSI for a device with a single vector targeting a specific CPU.
///
/// Allocates a vector, programs the MSI registers, and enables MSI.
/// Returns the allocated vector number on success.
pub fn enable_msi(config: &mut MsiConfig, target_cpu_lapic: u8) -> Option<u8> {
    // MSI-X off first, should firmware have left it on: the specification
    // forbids both enabled at once, and what a function then does is its own.
    let msix_at = pci::walk_capabilities(pci::PciAddress {
        bus: config.bus,
        device: config.device,
        function: config.function,
    })
    .into_iter()
    .find(|c| c.id == 0x11)
    .map(|c| c.offset.wrapping_add(2));
    if let Some(control) = msix_at {
        let value = pci::config_read16(config.bus, config.device, config.function, control);
        if value & (1 << 15) != 0 {
            pci::config_write16_native(
                config.bus,
                config.device,
                config.function,
                control,
                value & !(1 << 15),
            );
        }
    }

    // Allocate a vector.
    let vector = alloc_vector()?;
    config.allocated_vector = vector;
    config.allocated_count = 1;

    let (bus, dev, func) = (config.bus, config.device, config.function);
    let cap = config.cap_offset;

    // Format the MSI message.
    let address = format_address(target_cpu_lapic, false);
    let data = format_data(vector, true, true);

    // Write MSI Address (capability + 4).
    pci::config_write32(bus, dev, func, cap.wrapping_add(4), address);

    if config.is_64bit {
        // 64-bit: upper address at +8, data at +12.
        pci::config_write32(bus, dev, func, cap.wrapping_add(8), 0);
        #[allow(clippy::cast_possible_truncation)]
        pci::config_write16_native(bus, dev, func, cap.wrapping_add(12), data as u16);
    } else {
        // 32-bit: data at +8.
        #[allow(clippy::cast_possible_truncation)]
        pci::config_write16_native(bus, dev, func, cap.wrapping_add(8), data as u16);
    }

    // Enable MSI (set bit 0 of Message Control).
    let msg_control = pci::config_read16(bus, dev, func, cap.wrapping_add(2));
    pci::config_write16_native(
        bus,
        dev,
        func,
        cap.wrapping_add(2),
        // Multiple Message Enable (bits 6:4) to 0: one vector is allocated,
        // whatever the function held before.
        (msg_control & !(0b111 << 4)) | MSI_CONTROL_ENABLE,
    );

    // Disable legacy INTx (set bit 10 of PCI Command register).
    let cmd = pci::config_read16(bus, dev, func, 4);
    pci::config_write16_native(bus, dev, func, 4, cmd | (1 << 10));

    serial_println!(
        "[msi] Enabled MSI for {:02x}:{:02x}.{}: vector={}, cpu={}",
        config.bus,
        config.device,
        config.function,
        vector,
        target_cpu_lapic
    );

    Some(vector)
}

/// Disable MSI for a device and free the allocated vector.
pub fn disable_msi(config: &mut MsiConfig) {
    if config.allocated_count == 0 {
        return;
    }

    let (bus, dev, func) = (config.bus, config.device, config.function);
    let cap = config.cap_offset;

    // Clear MSI enable bit.
    let msg_control = pci::config_read16(bus, dev, func, cap.wrapping_add(2));
    pci::config_write16_native(
        bus,
        dev,
        func,
        cap.wrapping_add(2),
        msg_control & !MSI_CONTROL_ENABLE,
    );
    // Legacy INTx back on (Command bit 10), as `enable_msi` found it off:
    // the function goes back to whoever drives it next able to interrupt.
    let cmd = pci::config_read16(bus, dev, func, 4);
    pci::config_write16_native(bus, dev, func, 4, cmd & !(1 << 10));

    // Free allocated vectors.
    free_vectors(config.allocated_vector, config.allocated_count as usize);
    config.allocated_vector = 0;
    config.allocated_count = 0;
}

// ---------------------------------------------------------------------------
// PCI capability list traversal
// ---------------------------------------------------------------------------

/// Find a PCI capability by ID in the device's capability list.
///
/// Returns the config space offset of the capability, or `None`.
fn find_pci_capability(bus: u8, device: u8, function: u8, cap_id: u8) -> Option<u8> {
    // Check if the device has capabilities (Status register bit 4).
    let status = pci::config_read16(bus, device, function, 6);
    if status & (1 << 4) == 0 {
        return None; // No capabilities list.
    }

    // Capabilities pointer is at offset 0x34.
    let mut offset = pci::config_read8(bus, device, function, 0x34);
    offset &= 0xFC; // Must be DWORD-aligned.

    // Walk the capability linked list (max 48 entries to prevent loops).
    for _ in 0..48 {
        if offset == 0 {
            break;
        }
        let id = pci::config_read8(bus, device, function, offset);
        if id == cap_id {
            return Some(offset);
        }
        // Next pointer is at offset+1.
        offset = pci::config_read8(bus, device, function, offset.wrapping_add(1));
        offset &= 0xFC;
    }
    None
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// MSI subsystem statistics.
#[derive(Debug, Clone, Copy)]
pub struct MsiStats {
    /// Number of vectors currently allocated.
    pub vectors_used: u32,
    /// Total vectors available.
    pub vectors_total: usize,
}

/// Get MSI statistics.
#[must_use]
pub fn stats() -> MsiStats {
    MsiStats {
        vectors_used: VECTORS_USED.load(Ordering::Relaxed),
        vectors_total: MSI_VECTOR_COUNT,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the MSI subsystem.
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[msi] Running self-test...");

    // Test 0: the pool shares no vector with the I/O APIC's lines or the
    // kernel's own vectors.
    let ioapic_end = crate::ioapic::IRQ_VECTOR_BASE.saturating_add(crate::ioapic::MAX_IRQ as u8);
    let kernel_vectors = [
        crate::apic::TIMER_VECTOR,
        crate::apic::SPURIOUS_VECTOR,
        crate::apic::RESCHEDULE_VECTOR,
        crate::cpusync::CPU_SYNC_VECTOR,
        crate::tlb::TLB_SHOOTDOWN_VECTOR,
    ];
    if MSI_VECTOR_BASE < ioapic_end
        || kernel_vectors
            .iter()
            .any(|&v| (MSI_VECTOR_BASE..=MSI_VECTOR_MAX).contains(&v))
    {
        serial_println!(
            "[msi]   FAIL: the MSI pool {}..={} overlaps the I/O APIC (..{}) or a kernel vector",
            MSI_VECTOR_BASE,
            MSI_VECTOR_MAX,
            ioapic_end
        );
        return Err(crate::error::KernelError::InternalError);
    }

    // Test 1: Vector allocation.
    let v1 = alloc_vector();
    assert!(v1.is_some(), "first vector alloc should succeed");
    let v1 = v1.unwrap();
    assert!(v1 >= MSI_VECTOR_BASE && v1 <= MSI_VECTOR_MAX);
    serial_println!("[msi]   Single vector alloc: OK (vector={})", v1);

    // Test 2: Consecutive vector allocation.
    let v4 = alloc_vectors(4);
    assert!(v4.is_some(), "4-vector alloc should succeed");
    let v4_base = v4.unwrap();
    assert!(v4_base >= MSI_VECTOR_BASE);
    serial_println!("[msi]   Consecutive alloc (4): OK (base={})", v4_base);

    // Test 3: Free and re-alloc.
    free_vector(v1);
    let v1_again = alloc_vector();
    assert_eq!(v1_again, Some(v1), "freed vector should be re-allocatable");
    serial_println!("[msi]   Free + re-alloc: OK");

    // Test 4: Address/data formatting.
    let addr = format_address(0x01, false);
    assert_eq!(addr & 0xFFF0_0000, MSI_ADDRESS_BASE);
    assert_eq!((addr >> 12) & 0xFF, 0x01);
    let data = format_data(100, true, true);
    assert_eq!(data & 0xFF, 100);
    serial_println!(
        "[msi]   Address/data format: OK (addr={:#x}, data={:#x})",
        addr,
        data
    );

    // Test 5: Stats.
    let st = stats();
    assert!(st.vectors_used > 0);
    assert_eq!(st.vectors_total, MSI_VECTOR_COUNT);
    serial_println!(
        "[msi]   Stats: OK ({}/{} vectors used)",
        st.vectors_used,
        st.vectors_total
    );

    // Cleanup.
    free_vector(v1);
    free_vectors(v4_base, 4);

    serial_println!("[msi] Self-test PASSED");
    Ok(())
}
