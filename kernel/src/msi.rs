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
//! Vectors 48-223 are reserved for device MSI (below 48: exceptions and
//! legacy IRQs; above 223: system vectors like IPI, TLB shootdown, etc.).
//!
//! ## References
//!
//! - PCI Local Bus Spec 3.0, §6.8 (MSI)
//! - PCI Express Base Spec 4.0, §6.1.4 (MSI-X)
//! - Intel SDM Vol. 3, §10.11 (Message Signalled Interrupts)
//! - Linux `drivers/pci/msi/` — MSI infrastructure

#![allow(dead_code)]

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
const MSI_VECTOR_BASE: u8 = 64;

/// Last IDT vector available for MSI allocation: below the IPIs (250-252)
/// and the spurious vector (255).
const MSI_VECTOR_MAX: u8 = 223;

/// Total MSI vectors available.
const MSI_VECTOR_COUNT: usize = (MSI_VECTOR_MAX - MSI_VECTOR_BASE + 1) as usize;

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

/// Interrupts counted per vector since its owner last took them: added to by
/// the interrupt handler ([`dispatch`]), taken by the owner ([`take`]).
static PENDING: [AtomicU64; MSI_VECTOR_COUNT] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MSI_VECTOR_COUNT]
};

/// The task woken when a vector fires, `u64::MAX` for none.
static WAITER: [AtomicU64; MSI_VECTOR_COUNT] = {
    const NONE: AtomicU64 = AtomicU64::new(u64::MAX);
    [NONE; MSI_VECTOR_COUNT]
};

/// The index of `vector` in the pool, if it is one.
fn pool_index(vector: u8) -> Option<usize> {
    (MSI_VECTOR_BASE..=MSI_VECTOR_MAX)
        .contains(&vector)
        .then(|| usize::from(vector.wrapping_sub(MSI_VECTOR_BASE)))
}

/// An interrupt arrived on MSI `vector` (the IDT dispatcher's arm for the
/// pool): counted, its waiter woken, and the local APIC told it is done.
/// Interrupt context: atomics and a try-lock wake only. A wake the scheduler
/// lock refused is retried by [`process_deferred_wakes`].
pub fn dispatch(vector: u8) {
    if let Some(index) = pool_index(vector)
        && let (Some(pending), Some(waiter)) = (PENDING.get(index), WAITER.get(index))
    {
        pending.fetch_add(1, Ordering::Release);
        let task = waiter.load(Ordering::Acquire);
        if task != u64::MAX && !crate::sched::try_wake(task) {
            crate::softirq::raise(crate::softirq::IRQ_POLL_SOFTIRQ);
        }
    }
    // SAFETY: called from the interrupt handler for this vector, which the
    // local APIC delivered and now expects to be told is finished.
    unsafe { crate::apic::eoi() };
}

/// Take, and clear, the interrupts counted on `vector`.
pub fn take(vector: u8) -> u64 {
    pool_index(vector)
        .and_then(|index| PENDING.get(index))
        .map_or(0, |pending| pending.swap(0, Ordering::AcqRel))
}

/// Whether `vector` has interrupts counted, without taking them.
#[must_use]
pub fn is_pending(vector: u8) -> bool {
    pool_index(vector)
        .and_then(|index| PENDING.get(index))
        .is_some_and(|pending| pending.load(Ordering::Acquire) > 0)
}

/// Make `task` the one woken when `vector` fires (`u64::MAX`: no one).
pub fn set_waiter(vector: u8, task: u64) {
    if let Some(waiter) = pool_index(vector).and_then(|index| WAITER.get(index)) {
        waiter.store(task, Ordering::Release);
    }
}

/// Let go of `vector`'s delivery before the vector is freed: its waiter is
/// woken (to find its device gone) and forgotten, and its count dropped, so
/// the next owner of the vector starts from nothing.
pub fn release_delivery(vector: u8) {
    let Some(index) = pool_index(vector) else {
        return;
    };
    if let Some(waiter) = WAITER.get(index) {
        let task = waiter.swap(u64::MAX, Ordering::AcqRel);
        if task != u64::MAX {
            let _woken = crate::sched::try_wake(task);
        }
    }
    if let Some(pending) = PENDING.get(index) {
        pending.store(0, Ordering::Release);
    }
}

/// Whether `vector` is allocated from the pool.
#[must_use]
pub fn is_allocated(vector: u8) -> bool {
    pool_index(vector)
        .and_then(|index| VECTOR_ALLOCATED.get(index))
        .is_some_and(|allocated| allocated.load(Ordering::Acquire))
}

/// Retry the wakes [`dispatch`] could not make: each vector with interrupts
/// counted wakes its waiter. Called with the I/O APIC lines' deferred wakes.
pub fn process_deferred_wakes() {
    for (pending, waiter) in PENDING.iter().zip(WAITER.iter()) {
        if pending.load(Ordering::Acquire) == 0 {
            continue;
        }
        let task = waiter.load(Ordering::Acquire);
        if task != u64::MAX {
            let _woken = crate::sched::try_wake(task);
        }
    }
}

/// An interrupt sent to this CPU on a pool vector reaches [`dispatch`] and is
/// counted, and [`take`] hands the count over once. Sent as an IPI to the
/// local APIC itself, so it needs no device; run once interrupts are on.
pub fn self_test_delivery() -> crate::error::KernelResult<()> {
    let Some(vector) = alloc_vector() else {
        serial_println!("[msi]   FAIL: delivery: no vector free");
        return Err(crate::error::KernelError::InternalError);
    };
    let _stale = take(vector);
    let Some(apic_id) = crate::smp::cpu_apic_id(crate::smp::fast_cpu_index()) else {
        free_vector(vector);
        serial_println!("[msi]   FAIL: delivery: this CPU has no APIC id");
        return Err(crate::error::KernelError::InternalError);
    };
    // SAFETY: a fixed IPI to this CPU's own APIC on a vector the IDT has a
    // stub for (the pool's), which only counts it.
    unsafe { crate::apic::send_fixed_ipi(apic_id, vector) };
    let deadline = crate::hpet::elapsed_ns().saturating_add(100_000_000);
    while !is_pending(vector) && crate::hpet::elapsed_ns() < deadline {
        core::hint::spin_loop();
    }
    let first = take(vector);
    let second = take(vector);
    free_vector(vector);
    if first != 1 || second != 0 {
        serial_println!(
            "[msi]   FAIL: delivery: vector {} counted {} then {} (want 1 then 0)",
            vector,
            first,
            second
        );
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!(
        "[msi]   delivery: OK (vector {} reached dispatch once)",
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
        msg_control | MSI_CONTROL_ENABLE,
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
