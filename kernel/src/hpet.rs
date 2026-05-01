//! High Precision Event Timer (HPET) driver.
//!
//! The HPET provides a high-resolution monotonic counter that can be
//! read at any time without a syscall, running at a fixed frequency
//! defined by the hardware (typically 10-25 MHz, giving ~40-100 ns
//! resolution).
//!
//! ## Purpose
//!
//! The APIC timer provides 10 ms resolution (100 Hz periodic).  For
//! high-resolution timing — profiling, precise SYS_SLEEP,
//! SYS_CLOCK_MONOTONIC — we need sub-microsecond precision.
//!
//! We use the TSC (rdtsc) for cycle-accurate micro-benchmarks, but
//! the TSC frequency varies between CPUs and can change with power
//! states.  The HPET runs at a fixed frequency defined in the
//! capabilities register, making it a reliable monotonic clock source.
//!
//! ## Registers
//!
//! The HPET is accessed via MMIO.  We only use the main counter (we
//! don't configure comparators or interrupts — the APIC timer handles
//! scheduling).
//!
//! | Offset | Register                        | Used |
//! |--------|---------------------------------|------|
//! | 0x000  | General Capabilities and ID     | Yes  |
//! | 0x010  | General Configuration           | Yes  |
//! | 0x020  | General Interrupt Status         | No   |
//! | 0x0F0  | Main Counter Value              | Yes  |
//!
//! ## References
//!
//! - IA-PC HPET Specification, Rev 1.0a (October 2004)
//! - Linux `arch/x86/kernel/hpet.c`
//! - ACPI Specification 6.5, section 5.2.28 (HPET Description Table)

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::PhysFrame;
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::serial_println;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// ACPI HPET Description Table layout
// ---------------------------------------------------------------------------

/// HPET ACPI table structure (after the standard 36-byte SDT header).
///
/// Per ACPI spec 6.5, section 5.2.28.
#[repr(C, packed)]
struct HpetAcpiTable {
    /// Event Timer Block ID.
    ///
    /// Bits 31-16: PCI vendor ID of the HPET.
    /// Bits 15:    Legacy replacement IRQ routing capable.
    /// Bits 14:    Counter size (1 = 64-bit, 0 = 32-bit).
    /// Bits 12-8:  Number of comparators in the first timer block.
    /// Bits 7-0:   Hardware revision ID.
    event_timer_block_id: u32,

    /// Base address of the HPET register set (ACPI Generic Address Structure).
    ///
    /// The GAS has: address_space (1 byte), bit_width (1 byte),
    /// bit_offset (1 byte), access_size (1 byte), address (8 bytes).
    /// We only need the 8-byte address at offset 4 within the GAS.
    address_space_id: u8,
    register_bit_width: u8,
    register_bit_offset: u8,
    #[allow(dead_code)]
    reserved: u8,
    base_address: u64,

    /// HPET sequence number (for systems with multiple HPETs).
    hpet_number: u8,

    /// Minimum tick count for periodic mode.
    min_tick: u16,

    /// Page protection and OEM attribute.
    #[allow(dead_code)]
    page_protection: u8,
}

// ---------------------------------------------------------------------------
// HPET MMIO register offsets
// ---------------------------------------------------------------------------

/// General Capabilities and ID Register (read-only).
///
/// Bits 63-32: Counter clock period (in femtoseconds).
/// Bit 13:     COUNT_SIZE_CAP (1 = 64-bit main counter).
/// Bits 12-8:  NUM_TIM_CAP (number of timers minus 1).
/// Bit 15:     LEG_RT_CAP (legacy replacement routing capable).
/// Bits 7-0:   REV_ID (revision, must be non-zero).
const REG_CAP_ID: usize = 0x000;

/// General Configuration Register.
///
/// Bit 0: ENABLE_CNF — overall enable (starts/stops main counter).
/// Bit 1: LEG_RT_CNF — enable legacy replacement routing.
const REG_CONFIG: usize = 0x010;

/// Main Counter Value Register (64-bit).
const REG_COUNTER: usize = 0x0F0;

// ---------------------------------------------------------------------------
// Configuration bits
// ---------------------------------------------------------------------------

/// Bit 0 of the General Configuration register: overall enable.
const CONFIG_ENABLE: u64 = 1 << 0;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Whether the HPET has been initialized successfully.
static HPET_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// HHDM virtual address of the HPET MMIO register block.
static HPET_BASE_VIRT: AtomicU64 = AtomicU64::new(0);

/// Counter clock period in femtoseconds (10^-15 seconds).
///
/// Stored at init time from the capabilities register.  This is fixed
/// by hardware and never changes.  Used to convert counter ticks to
/// nanoseconds: `ns = ticks * period_fs / 1_000_000`.
static PERIOD_FS: AtomicU64 = AtomicU64::new(0);

/// HPET counter frequency in Hz.
///
/// Derived from `PERIOD_FS`: `freq_hz = 10^15 / period_fs`.
/// Stored for convenience and logging.
static FREQ_HZ: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// MMIO helpers
// ---------------------------------------------------------------------------

/// Read a 64-bit HPET register via volatile MMIO read.
///
/// # Safety
///
/// `base` must be a valid HHDM virtual address pointing to the HPET
/// register block.  `offset` must be a valid register offset.
#[inline]
unsafe fn mmio_read64(base: u64, offset: usize) -> u64 {
    // SAFETY: Caller guarantees base + offset points to a valid MMIO
    // register in HHDM-mapped space.
    unsafe { ((base + offset as u64) as *const u64).read_volatile() }
}

/// Write a 64-bit HPET register via volatile MMIO write.
///
/// # Safety
///
/// Same preconditions as `mmio_read64`, plus the register must be writable.
#[inline]
unsafe fn mmio_write64(base: u64, offset: usize, value: u64) {
    // SAFETY: Caller guarantees base + offset points to a valid writable
    // MMIO register in HHDM-mapped space.
    unsafe { ((base + offset as u64) as *mut u64).write_volatile(value); }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the HPET from the ACPI HPET Description Table.
///
/// Parses the ACPI table to find the MMIO base address, maps the
/// register block, reads hardware capabilities, and enables the main
/// counter.
///
/// # Safety
///
/// - Must be called after `acpi::init()` and `mm::page_table::init()`.
/// - Must be called exactly once during early boot (single-threaded,
///   interrupts disabled).
pub unsafe fn init() {
    serial_println!("[hpet] Initializing HPET...");

    // Get the HPET ACPI table physical address from the ACPI subsystem.
    let table_phys = match crate::acpi::hpet_table_phys() {
        Some(phys) => phys,
        None => {
            serial_println!("[hpet] No HPET ACPI table found (non-fatal)");
            return;
        }
    };

    let hhdm = match crate::mm::page_table::hhdm() {
        Some(offset) => offset,
        None => {
            serial_println!("[hpet] ERROR: HHDM not initialized");
            return;
        }
    };

    // The HPET ACPI table is: 36-byte SDT header + HpetAcpiTable fields.
    // The ACPI subsystem already validated the header and checksum.
    //
    // SAFETY: table_phys is from the RSDT/XSDT, validated by acpi::init().
    // The HHDM maps all physical memory.
    let table_virt = table_phys.wrapping_add(hhdm);
    let sdt_header_size = 36u64;
    let hpet_table = unsafe {
        &*((table_virt + sdt_header_size) as *const HpetAcpiTable)
    };

    // The base address must be in system memory space (address_space_id == 0).
    if hpet_table.address_space_id != 0 {
        serial_println!(
            "[hpet] HPET base in non-memory address space ({}), skipping",
            hpet_table.address_space_id
        );
        return;
    }

    let base_phys = hpet_table.base_address;
    let hpet_number = hpet_table.hpet_number;
    let min_tick = hpet_table.min_tick;

    serial_println!(
        "[hpet] HPET {} at phys={:#x} (min_tick={})",
        hpet_number, base_phys, min_tick
    );

    // Map the HPET MMIO register block.  The register space is 1024 bytes
    // (fits in one 4 KiB hardware page), but our frames are 16 KiB aligned.
    // Align the physical address down to a frame boundary and compute the
    // virtual address via HHDM.
    //
    // The HHDM (from Limine) may not cover MMIO regions like the HPET
    // (which is device memory, not RAM).  We explicitly map a frame
    // covering the register block, following the same pattern as apic::init().
    let frame_phys = base_phys & !(crate::mm::frame::FRAME_SIZE as u64 - 1);
    let base_virt = base_phys.wrapping_add(hhdm);

    let hpet_frame = match PhysFrame::from_addr(frame_phys) {
        Some(f) => f,
        None => {
            serial_println!("[hpet] HPET base {:#x} not frame-alignable", base_phys);
            return;
        }
    };
    let hpet_virt = VirtAddr::new(frame_phys.wrapping_add(hhdm));
    let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
    let mmio_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;

    // SAFETY: HPET physical address is valid MMIO (from ACPI table).
    // We're mapping it into the HHDM range.  If Limine already mapped
    // this region, map_frame may fail — that's fine, the page is accessible.
    if let Err(e) = unsafe {
        page_table::map_frame(pml4_phys, hpet_virt, hpet_frame, mmio_flags)
    } {
        serial_println!("[hpet] MMIO map returned {:?} (may already be mapped)", e);
        // Proceed anyway — the HHDM might cover it.
    } else {
        // Flush TLB for the new mapping.
        // SAFETY: Standard invlpg for the virtual address we just mapped.
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) base_virt,
                options(nostack, preserves_flags),
            );
        }
        serial_println!("[hpet] MMIO mapped at {:#x}", base_virt);
    }

    // Read capabilities register.
    // SAFETY: base_virt is mapped, REG_CAP_ID is a valid read-only register.
    let cap = unsafe { mmio_read64(base_virt, REG_CAP_ID) };

    // Extract fields from the capabilities register.
    let rev_id = (cap & 0xFF) as u8;
    #[allow(clippy::cast_possible_truncation)]
    let num_timers = ((cap >> 8) & 0x1F) as u8 + 1; // NUM_TIM_CAP + 1
    let count_size_64 = (cap >> 13) & 1 != 0;
    let period_fs = cap >> 32;

    if rev_id == 0 {
        serial_println!("[hpet] Invalid revision ID (0), skipping");
        return;
    }

    if period_fs == 0 {
        serial_println!("[hpet] Invalid counter period (0 fs), skipping");
        return;
    }

    // The spec requires period_fs <= 0x05F5_E100 (100 ns = 10 MHz minimum).
    // Values outside this range indicate a broken HPET.
    const MAX_PERIOD_FS: u64 = 0x05F5_E100; // 100,000,000 fs = 100 ns
    if period_fs > MAX_PERIOD_FS {
        serial_println!(
            "[hpet] Counter period {} fs exceeds maximum ({} fs), skipping",
            period_fs, MAX_PERIOD_FS
        );
        return;
    }

    // Compute frequency in Hz: freq = 10^15 / period_fs.
    // period_fs is at most 100M, so 10^15 / 100M = 10M Hz minimum.
    // This division won't overflow (10^15 fits in u64).
    #[allow(clippy::arithmetic_side_effects)]
    let freq_hz = 1_000_000_000_000_000u64 / period_fs;

    serial_println!(
        "[hpet]   Revision: {}, Timers: {}, 64-bit: {}, Period: {} fs ({}.{:03} MHz)",
        rev_id,
        num_timers,
        count_size_64,
        period_fs,
        freq_hz / 1_000_000,
        (freq_hz % 1_000_000) / 1_000
    );

    // Halt the counter before reconfiguring.
    // SAFETY: base_virt is mapped, REG_CONFIG is a valid register.
    unsafe {
        let config = mmio_read64(base_virt, REG_CONFIG);
        mmio_write64(base_virt, REG_CONFIG, config & !CONFIG_ENABLE);
    }

    // Reset the main counter to zero.
    // SAFETY: Counter is halted (ENABLE_CNF == 0).
    unsafe {
        mmio_write64(base_virt, REG_COUNTER, 0);
    }

    // Enable the counter.
    // We do NOT enable legacy replacement routing (bit 1) — the APIC
    // timer handles scheduling.  We just want the free-running counter
    // for high-resolution time queries.
    //
    // SAFETY: base_virt is mapped, writing CONFIG_ENABLE starts the counter.
    unsafe {
        mmio_write64(base_virt, REG_CONFIG, CONFIG_ENABLE);
    }

    // Verify the counter is incrementing.
    // SAFETY: Counter is enabled, register is readable.
    let val1 = unsafe { mmio_read64(base_virt, REG_COUNTER) };
    // Small busy-wait to let the counter advance.
    for _ in 0..1000u32 {
        core::hint::spin_loop();
    }
    let val2 = unsafe { mmio_read64(base_virt, REG_COUNTER) };

    if val2 <= val1 {
        serial_println!("[hpet] WARNING: Counter not advancing ({} → {})", val1, val2);
        // Don't mark as available — fall back to TSC/APIC.
        return;
    }

    // Store global state.
    HPET_BASE_VIRT.store(base_virt, Ordering::Release);
    PERIOD_FS.store(period_fs, Ordering::Release);
    FREQ_HZ.store(freq_hz, Ordering::Release);
    HPET_AVAILABLE.store(true, Ordering::Release);

    serial_println!(
        "[hpet] HPET initialized: counter running at {}.{:03} MHz",
        freq_hz / 1_000_000,
        (freq_hz % 1_000_000) / 1_000
    );
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Whether the HPET is available and running.
#[must_use]
#[inline]
pub fn is_available() -> bool {
    HPET_AVAILABLE.load(Ordering::Acquire)
}

/// Read the HPET main counter value.
///
/// Returns the raw counter ticks since the HPET was initialized.
/// Use [`ticks_to_ns`] to convert to nanoseconds.
///
/// Returns 0 if the HPET is not available.
#[must_use]
#[inline]
pub fn read_counter() -> u64 {
    let base = HPET_BASE_VIRT.load(Ordering::Acquire);
    if base == 0 {
        return 0;
    }
    // SAFETY: HPET_BASE_VIRT is set only after successful init(),
    // which verified the MMIO mapping and register accessibility.
    // The main counter register is always readable when the HPET
    // is enabled.
    unsafe { mmio_read64(base, REG_COUNTER) }
}

/// Convert HPET counter ticks to nanoseconds.
///
/// Uses the hardware-defined period from the capabilities register.
/// Formula: `ns = ticks * period_fs / 1_000_000`.
///
/// For typical HPET frequencies (10-25 MHz), this handles up to
/// ~584 years of uptime before overflow (u64 max / 100ns period).
#[must_use]
#[inline]
pub fn ticks_to_ns(ticks: u64) -> u64 {
    let period = PERIOD_FS.load(Ordering::Relaxed);
    if period == 0 {
        return 0;
    }
    // ticks * period_fs could overflow for very large tick counts.
    // Use checked_mul and fall back to scaled division if needed.
    match ticks.checked_mul(period) {
        Some(product) => product / 1_000_000,
        None => {
            // Overflow path: divide first, losing some precision.
            // period_fs / 1_000_000 gives ns per tick (may be 0 for
            // very fast HPETs, so handle that).
            let ns_per_tick = period / 1_000_000;
            if ns_per_tick > 0 {
                ticks.saturating_mul(ns_per_tick)
            } else {
                // Extremely fast HPET (> 1 GHz, unlikely).
                // Use 128-bit intermediate.
                let product = (ticks as u128) * (period as u128);
                (product / 1_000_000) as u64
            }
        }
    }
}

/// Read the HPET counter and return the elapsed time in nanoseconds
/// since the HPET was initialized.
///
/// Convenience function combining [`read_counter`] and [`ticks_to_ns`].
/// Returns 0 if the HPET is not available.
#[must_use]
#[inline]
pub fn elapsed_ns() -> u64 {
    ticks_to_ns(read_counter())
}

/// Get the HPET counter frequency in Hz.
///
/// Returns 0 if the HPET is not available.
#[must_use]
#[inline]
pub fn frequency_hz() -> u64 {
    FREQ_HZ.load(Ordering::Relaxed)
}

/// Get the counter period in femtoseconds.
///
/// Returns 0 if the HPET is not available.
#[must_use]
#[inline]
#[allow(dead_code)] // Public API for timer calibration and diagnostics.
pub fn period_femtoseconds() -> u64 {
    PERIOD_FS.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify HPET is working correctly.
///
/// Checks counter monotonicity and approximate timing accuracy.
pub fn self_test() -> KernelResult<()> {
    if !is_available() {
        serial_println!("[hpet] Self-test skipped (HPET not available)");
        return Ok(());
    }

    serial_println!("[hpet] Running self-test...");

    let freq = frequency_hz();
    serial_println!("[hpet]   Frequency: {} Hz ({}.{:03} MHz)",
        freq, freq / 1_000_000, (freq % 1_000_000) / 1_000);

    // Test 1: Counter is monotonically increasing.
    let a = read_counter();
    for _ in 0..100u32 {
        core::hint::spin_loop();
    }
    let b = read_counter();
    if b <= a {
        serial_println!("[hpet]   FAIL: Counter not monotonic ({} → {})", a, b);
        return Err(KernelError::InternalError);
    }
    serial_println!("[hpet]   Monotonicity: OK ({} → {}, delta={})", a, b, b - a);

    // Test 2: ticks_to_ns produces reasonable values.
    // One second of ticks should convert to ~1_000_000_000 ns.
    let one_second_ticks = freq;
    let one_second_ns = ticks_to_ns(one_second_ticks);
    // Allow 1% tolerance (hardware period rounding).
    let expected = 1_000_000_000u64;
    let lower = expected - expected / 100;
    let upper = expected + expected / 100;
    if one_second_ns < lower || one_second_ns > upper {
        serial_println!(
            "[hpet]   FAIL: {} ticks → {} ns (expected ~{} ns)",
            one_second_ticks, one_second_ns, expected
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[hpet]   Tick conversion: {} ticks → {} ns (expected ~{}): OK",
        one_second_ticks, one_second_ns, expected);

    // Test 3: Elapsed time is positive.
    let t1 = elapsed_ns();
    for _ in 0..10_000u32 {
        core::hint::spin_loop();
    }
    let t2 = elapsed_ns();
    if t2 <= t1 {
        serial_println!("[hpet]   FAIL: elapsed_ns not advancing ({} → {})", t1, t2);
        return Err(KernelError::InternalError);
    }
    serial_println!("[hpet]   Elapsed time advancing: {} ns delta: OK", t2 - t1);

    serial_println!("[hpet] Self-test PASSED");
    Ok(())
}
