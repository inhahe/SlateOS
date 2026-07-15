//! Local APIC driver — timer interrupts for preemptive scheduling.
//!
//! The Local Advanced Programmable Interrupt Controller (LAPIC) is the
//! per-CPU interrupt controller on modern x86_64 systems.  We use it
//! for:
//!
//! 1. **Timer interrupts** — periodic ticks that drive the scheduler's
//!    preemptive time-slice enforcement.
//! 2. **End-of-interrupt (EOI)** signaling.
//! 3. **Spurious interrupt** handling.
//!
//! ## APIC Timer
//!
//! The APIC timer counts down from an initial value at a frequency
//! derived from the bus clock divided by a configurable divisor.  We
//! calibrate the frequency at boot using the legacy PIT (Programmable
//! Interval Timer) as a reference, then program periodic mode at the
//! desired tick rate (default: 100 Hz = 10 ms per tick).
//!
//! ## Calibration
//!
//! 1. Program PIT channel 2 for a one-shot ~10 ms countdown.
//! 2. Start the APIC timer with `initial_count = 0xFFFF_FFFF`.
//! 3. Busy-wait for the PIT countdown to complete.
//! 4. Read the APIC timer's current count.
//! 5. `ticks_per_10ms = 0xFFFF_FFFF - current_count`.
//! 6. `initial_count = ticks_per_10ms` for 100 Hz periodic mode.
//!
//! ## Memory-Mapped Registers
//!
//! APIC registers are memory-mapped at the physical address in the
//! `IA32_APIC_BASE` MSR (typically `0xFEE0_0000`).  We access them
//! through the HHDM (Higher Half Direct Map).
//!
//! ## References
//!
//! - Intel SDM Vol. 3A, Chapter 10 "APIC"
//! - OSDev wiki: <https://wiki.osdev.org/APIC>
//! - OSDev wiki: <https://wiki.osdev.org/APIC_timer>

use crate::cpu;
use crate::error::{KernelError, KernelResult};
use crate::mm::frame::PhysFrame;
use crate::mm::page_table::{self, PageFlags, VirtAddr};
use crate::port;
use crate::serial_println;

// ---------------------------------------------------------------------------
// APIC register offsets (from APIC base)
// ---------------------------------------------------------------------------

/// APIC ID register.
const APIC_ID: u32 = 0x020;
/// APIC version register.
const APIC_VERSION: u32 = 0x030;
/// End-of-interrupt register (write 0 to signal EOI).
const APIC_EOI: u32 = 0x0B0;
/// Spurious Interrupt Vector register (also contains APIC enable bit).
const APIC_SPURIOUS: u32 = 0x0F0;
/// Interrupt Command Register — low 32 bits (trigger IPI).
const APIC_ICR_LOW: u32 = 0x300;
/// Interrupt Command Register — high 32 bits (destination).
const APIC_ICR_HIGH: u32 = 0x310;
/// Timer Local Vector Table entry.
const APIC_TIMER_LVT: u32 = 0x320;
/// Timer initial count register.
const APIC_TIMER_INITIAL: u32 = 0x380;
/// Timer current count register (read-only).
const APIC_TIMER_CURRENT: u32 = 0x390;
/// Timer divide configuration register.
const APIC_TIMER_DIVIDE: u32 = 0x3E0;

// ---------------------------------------------------------------------------
// LVT timer mode bits
// ---------------------------------------------------------------------------

/// Timer mode: one-shot (bit 17 = 0, bit 16 = 0).
#[allow(dead_code)]
const TIMER_MODE_ONESHOT: u32 = 0;
/// Timer mode: periodic (bit 17 = 1).
const TIMER_MODE_PERIODIC: u32 = 1 << 17;
/// LVT mask bit — when set, the interrupt is inhibited.
const LVT_MASKED: u32 = 1 << 16;

// ---------------------------------------------------------------------------
// Interrupt vectors
// ---------------------------------------------------------------------------

/// Vector for the APIC timer interrupt.
pub const TIMER_VECTOR: u8 = 32;
/// Vector for spurious interrupts.
pub const SPURIOUS_VECTOR: u8 = 255;

// ---------------------------------------------------------------------------
// MSR addresses
// ---------------------------------------------------------------------------

/// IA32_APIC_BASE MSR — contains the physical base address and enable
/// bits for the local APIC.
const IA32_APIC_BASE_MSR: u32 = 0x1B;

// ---------------------------------------------------------------------------
// PIT (8254) constants for calibration
// ---------------------------------------------------------------------------

/// PIT oscillator frequency in Hz.
const PIT_FREQUENCY: u32 = 1_193_182;

/// PIT channel 2 data port.
const PIT_CH2_DATA: u16 = 0x42;
/// PIT command register.
const PIT_COMMAND: u16 = 0x43;
/// NMI status and control register (contains PIT channel 2 gate).
const NMI_STATUS: u16 = 0x61;

/// Desired tick rate in Hz (10 ms period).
pub const TICK_RATE_HZ: u32 = 100;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Virtual address of the APIC base (set during init).
static APIC_BASE_VIRT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Whether the APIC timer is running.
static TIMER_ACTIVE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// ISR latency measurement (for benchmarking)
// ---------------------------------------------------------------------------

/// Whether ISR latency measurement is active.
static ISR_MEASURE_ACTIVE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Minimum hard-IRQ cycles observed (entry → EOI, interrupts disabled).
static ISR_HARD_MIN: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(u64::MAX);

/// Maximum hard-IRQ cycles observed.
static ISR_HARD_MAX: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Total hard-IRQ cycles accumulated during measurement window.
static ISR_HARD_TOTAL: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Number of ticks measured.
static ISR_MEASURE_COUNT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Timer jitter tracking (always-on, measures inter-tick interval variance)
// ---------------------------------------------------------------------------

/// TSC value at the previous timer tick (BSP only).
///
/// Used to compute the interval between consecutive timer interrupts.
/// Zero means "not yet initialized" (first tick hasn't fired).
static JITTER_LAST_TSC: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Minimum inter-tick interval in TSC cycles observed since boot.
static JITTER_MIN: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(u64::MAX);

/// Maximum inter-tick interval in TSC cycles observed since boot.
static JITTER_MAX: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Sum of all inter-tick intervals (for average computation).
/// Wraps on overflow, but that takes centuries at typical TSC rates.
static JITTER_SUM: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Number of inter-tick intervals recorded.
static JITTER_COUNT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Tick counter
// ---------------------------------------------------------------------------

/// Tick counter — incremented on every timer interrupt.
static TICK_COUNT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Calibrated APIC timer initial count (ticks per 10 ms).
///
/// Saved by BSP during calibration so APs can reuse the same value
/// without each needing PIT access (PIT is shared hardware).
static CALIBRATED_TIMER_COUNT: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// BSP's Local APIC ID (set during init).
static BSP_APIC_ID: core::sync::atomic::AtomicU8 =
    core::sync::atomic::AtomicU8::new(0);

// ---------------------------------------------------------------------------
// Register access helpers
// ---------------------------------------------------------------------------

/// Read a 32-bit APIC register.
///
/// # Safety
///
/// `APIC_BASE_VIRT` must be initialized and the offset must be a valid
/// APIC register.
unsafe fn apic_read(offset: u32) -> u32 {
    let base = APIC_BASE_VIRT.load(core::sync::atomic::Ordering::Relaxed);
    debug_assert!(base != 0, "APIC not initialized");
    // SAFETY: The APIC registers are memory-mapped at this address.
    // Caller guarantees offset is valid.  Volatile read required because
    // hardware may change the value.
    unsafe {
        let ptr = (base + u64::from(offset)) as *const u32;
        core::ptr::read_volatile(ptr)
    }
}

/// Write a 32-bit APIC register.
///
/// # Safety
///
/// `APIC_BASE_VIRT` must be initialized and the offset must be a valid
/// APIC register.
unsafe fn apic_write(offset: u32, value: u32) {
    let base = APIC_BASE_VIRT.load(core::sync::atomic::Ordering::Relaxed);
    debug_assert!(base != 0, "APIC not initialized");
    // SAFETY: Same as apic_read — volatile write to MMIO register.
    unsafe {
        let ptr = (base + u64::from(offset)) as *mut u32;
        core::ptr::write_volatile(ptr, value);
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Send End-of-Interrupt to the local APIC.
///
/// Must be called at the end of every interrupt handler for APIC-
/// delivered interrupts.
///
/// # Safety
///
/// Must only be called from an interrupt handler context after the
/// APIC has been initialized.
pub unsafe fn eoi() {
    // SAFETY: EOI register accepts any write; 0 is conventional.
    unsafe {
        apic_write(APIC_EOI, 0);
    }
}

/// Get the current tick count since APIC timer was started.
#[must_use]
pub fn tick_count() -> u64 {
    TICK_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// ISR latency measurement — public API
// ---------------------------------------------------------------------------

/// Start ISR latency measurement.
///
/// Resets all counters and enables per-tick TSC sampling in
/// [`handle_timer_irq`].  Each timer ISR records entry → post-EOI
/// cycles, giving the hard-IRQ phase duration (the time interrupts
/// are disabled and other devices are blocked).
///
/// Call [`stop_isr_measurement`] to end the measurement window, then
/// [`isr_measurement_results`] to read min/mean/max cycles.
pub fn start_isr_measurement() {
    // Reset counters before enabling measurement.
    ISR_HARD_MIN.store(u64::MAX, core::sync::atomic::Ordering::Relaxed);
    ISR_HARD_MAX.store(0, core::sync::atomic::Ordering::Relaxed);
    ISR_HARD_TOTAL.store(0, core::sync::atomic::Ordering::Relaxed);
    ISR_MEASURE_COUNT.store(0, core::sync::atomic::Ordering::Relaxed);
    // Enable sampling — the next timer ISR will start recording.
    ISR_MEASURE_ACTIVE.store(true, core::sync::atomic::Ordering::Release);
}

/// Stop ISR latency measurement.
///
/// Disables per-tick TSC sampling.  Results remain readable via
/// [`isr_measurement_results`] until the next [`start_isr_measurement`].
pub fn stop_isr_measurement() {
    ISR_MEASURE_ACTIVE.store(false, core::sync::atomic::Ordering::Release);
}

/// ISR latency measurement results.
#[derive(Debug, Clone, Copy)]
pub struct IsrMeasurement {
    /// Minimum hard-IRQ phase cycles (entry → post-EOI).
    pub min_cycles: u64,
    /// Maximum hard-IRQ phase cycles.
    pub max_cycles: u64,
    /// Mean hard-IRQ phase cycles.
    pub mean_cycles: u64,
    /// Total ticks sampled.
    pub count: u64,
}

/// Read ISR latency measurement results.
///
/// Returns `None` if no samples were collected (either measurement was
/// never started, or no timer interrupts fired during the window).
#[must_use]
pub fn isr_measurement_results() -> Option<IsrMeasurement> {
    let count = ISR_MEASURE_COUNT.load(core::sync::atomic::Ordering::Acquire);
    if count == 0 {
        return None;
    }
    let total = ISR_HARD_TOTAL.load(core::sync::atomic::Ordering::Acquire);
    let mean = total.checked_div(count).unwrap_or(0);
    Some(IsrMeasurement {
        min_cycles: ISR_HARD_MIN.load(core::sync::atomic::Ordering::Acquire),
        max_cycles: ISR_HARD_MAX.load(core::sync::atomic::Ordering::Acquire),
        mean_cycles: mean,
        count,
    })
}

// ---------------------------------------------------------------------------
// Timer jitter — public API
// ---------------------------------------------------------------------------

/// Timer jitter statistics (inter-tick interval variance).
///
/// Measures the TSC-cycle interval between consecutive APIC timer
/// interrupts on the BSP.  Jitter indicates that something delayed
/// interrupt delivery: long critical sections, NMIs, SMIs, or
/// hyper-visor VM exits.
#[derive(Debug, Clone, Copy)]
pub struct TimerJitter {
    /// Minimum inter-tick interval (TSC cycles).
    pub min_cycles: u64,
    /// Maximum inter-tick interval (TSC cycles).
    pub max_cycles: u64,
    /// Mean inter-tick interval (TSC cycles).
    pub mean_cycles: u64,
    /// Number of intervals measured.
    pub count: u64,
    /// Expected interval (TSC cycles per tick).  Estimated from mean
    /// if enough samples exist; zero if unknown.
    pub expected_cycles: u64,
}

/// Read timer jitter statistics.
///
/// Returns `None` if fewer than 2 timer ticks have fired (need at least
/// one interval to measure).
#[must_use]
pub fn timer_jitter() -> Option<TimerJitter> {
    let count = JITTER_COUNT.load(core::sync::atomic::Ordering::Relaxed);
    if count == 0 {
        return None;
    }
    let sum = JITTER_SUM.load(core::sync::atomic::Ordering::Relaxed);
    let mean = sum.checked_div(count).unwrap_or(0);
    Some(TimerJitter {
        min_cycles: JITTER_MIN.load(core::sync::atomic::Ordering::Relaxed),
        max_cycles: JITTER_MAX.load(core::sync::atomic::Ordering::Relaxed),
        mean_cycles: mean,
        count,
        // The mean *is* the best estimate of the expected interval
        // (it converges to TSC_freq / TICK_RATE_HZ over many samples).
        expected_cycles: mean,
    })
}

/// Get the configured tick rate in Hz.
#[must_use]
#[allow(dead_code)] // exposed for kshell `apic` command; not called from kernel hot path
pub fn tick_rate_hz() -> u32 {
    TICK_RATE_HZ
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the Local APIC and start the periodic timer.
///
/// # Errors
///
/// Returns [`KernelError::NotSupported`] if the HHDM is not initialized,
/// or [`KernelError::BadAlignment`] if the APIC base address is not
/// frame-aligned (should never happen on real hardware).
///
/// # Safety
///
/// - Must be called exactly once during boot.
/// - The GDT, IDT, and heap must already be initialized.
/// - Interrupts must be disabled.
pub unsafe fn init() -> KernelResult<()> {
    serial_println!("[apic] Initializing Local APIC...");

    // Step 1: Read the APIC base address from the MSR.
    // SAFETY: IA32_APIC_BASE is a valid MSR on all x86_64 CPUs.
    let apic_base_msr = unsafe { cpu::rdmsr(IA32_APIC_BASE_MSR) };
    let apic_base_phys = apic_base_msr & 0xFFFF_F000; // Bits [35:12] = base address
    let bsp = (apic_base_msr >> 8) & 1; // Bit 8 = BSP flag
    let global_enable = (apic_base_msr >> 11) & 1; // Bit 11 = global enable

    serial_println!(
        "[apic] APIC base: {:#x}, BSP={}, global_enable={}",
        apic_base_phys, bsp, global_enable
    );

    // Step 2: Map the APIC MMIO region into the kernel address space.
    // The HHDM (Higher Half Direct Map) set up by Limine may not cover
    // MMIO regions like the APIC (which is device memory, not RAM).
    // We explicitly map the APIC page so it's accessible.
    let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
    let apic_base_virt = apic_base_phys + hhdm;

    // Map the APIC MMIO page (16 KiB frame covering 0xFEE00000).
    // Use PRESENT | WRITABLE | NO_CACHE flags for MMIO.
    let apic_frame = PhysFrame::from_addr(apic_base_phys)
        .ok_or(KernelError::BadAlignment)?;
    let apic_virt = VirtAddr::new(apic_base_virt);
    let pml4_phys = page_table::cr3_to_pml4(page_table::read_cr3());
    let mmio_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_CACHE;

    // SAFETY: APIC physical address is valid MMIO. We're mapping it
    // into the HHDM range where it would naturally live.  No existing
    // mapping conflicts because Limine didn't map this region.
    if let Err(e) = unsafe {
        page_table::map_frame(pml4_phys, apic_virt, apic_frame, mmio_flags)
    } {
        serial_println!("[apic] WARNING: Failed to map APIC MMIO: {:?}", e);
        serial_println!("[apic] Attempting access via existing HHDM mapping...");
        // If mapping fails (e.g., already mapped), try to proceed anyway —
        // the HHDM might cover it on some configurations.
    } else {
        // Flush TLB for the new mapping.
        // SAFETY: Standard invlpg for the virtual address we just mapped.
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) apic_base_virt,
                options(nostack, preserves_flags),
            );
        }
        serial_println!("[apic] MMIO mapped at {:#x}", apic_base_virt);
    }

    APIC_BASE_VIRT.store(apic_base_virt, core::sync::atomic::Ordering::Release);

    // Step 3: Enable the APIC if not already enabled.
    if global_enable == 0 {
        // Set bit 11 (global enable) in IA32_APIC_BASE.
        // SAFETY: Valid MSR write to enable the APIC.
        unsafe {
            cpu::wrmsr(IA32_APIC_BASE_MSR, apic_base_msr | (1 << 11));
        }
        serial_println!("[apic] Enabled APIC via MSR");
    }

    // Read APIC ID and version for diagnostics.
    // SAFETY: APIC base is set and the APIC is enabled.
    let apic_id = unsafe { apic_read(APIC_ID) } >> 24;
    let apic_ver = unsafe { apic_read(APIC_VERSION) };
    serial_println!(
        "[apic] APIC ID={}, version={:#x}",
        apic_id,
        apic_ver & 0xFF
    );

    // Step 4: Set the spurious interrupt vector and enable the APIC.
    // Bit 8 = APIC software enable, bits [7:0] = spurious vector.
    // SAFETY: Writing the spurious vector register.
    unsafe {
        apic_write(APIC_SPURIOUS, 0x100 | u32::from(SPURIOUS_VECTOR));
    }

    // Step 5: Calibrate the timer using the PIT.
    // SAFETY: PIT ports are standard PC hardware, safe in QEMU.
    let ticks_per_10ms = unsafe { calibrate_timer_with_pit() };
    serial_println!(
        "[apic] Timer calibration: {} APIC ticks per 10ms",
        ticks_per_10ms
    );

    // Save BSP's APIC ID for SMP bootstrap.
    #[allow(clippy::cast_possible_truncation)]
    let bsp_id = (unsafe { apic_read(APIC_ID) } >> 24) as u8;
    BSP_APIC_ID.store(bsp_id, core::sync::atomic::Ordering::Release);

    if ticks_per_10ms == 0 {
        serial_println!("[apic] WARNING: Timer calibration failed (0 ticks). Using fallback.");
        let fallback = 625_000_u32;
        CALIBRATED_TIMER_COUNT.store(fallback, core::sync::atomic::Ordering::Release);
        configure_periodic_timer(fallback);
    } else {
        // Configure periodic mode at TICK_RATE_HZ.
        // ticks_per_10ms gives us the count for 10 ms (100 Hz).
        // For other rates: initial_count = ticks_per_10ms * 100 / TICK_RATE_HZ.
        // But since we want 100 Hz (= 10 ms), just use ticks_per_10ms directly.
        CALIBRATED_TIMER_COUNT.store(ticks_per_10ms, core::sync::atomic::Ordering::Release);
        configure_periodic_timer(ticks_per_10ms);
    }

    serial_println!(
        "[apic] Timer configured: {} Hz periodic, vector {}",
        TICK_RATE_HZ,
        TIMER_VECTOR
    );

    Ok(())
}

/// Configure the APIC timer for periodic mode.
///
/// # Safety
///
/// APIC must be initialized.
fn configure_periodic_timer(initial_count: u32) {
    // SAFETY: APIC is initialized, we're writing valid register values.
    unsafe {
        // Set divide value to 16.
        // Divide configuration encoding: 0b0011 = divide by 16.
        apic_write(APIC_TIMER_DIVIDE, 0x03);

        // Set the LVT timer entry: periodic mode, vector TIMER_VECTOR, unmasked.
        apic_write(APIC_TIMER_LVT, TIMER_MODE_PERIODIC | u32::from(TIMER_VECTOR));

        // Set the initial count — this starts the timer.
        apic_write(APIC_TIMER_INITIAL, initial_count);
    }

    TIMER_ACTIVE.store(true, core::sync::atomic::Ordering::Release);
}

/// Calibrate the APIC timer frequency using PIT channel 2.
///
/// Returns the number of APIC timer ticks in approximately 10 ms.
///
/// # Safety
///
/// APIC must be initialized.  PIT I/O ports must be accessible.
unsafe fn calibrate_timer_with_pit() -> u32 {
    // We use PIT channel 2 because it can be gated via the NMI
    // status register (port 0x61) without affecting the system timer.

    // Calculate PIT reload value for ~10 ms.
    // PIT frequency = 1,193,182 Hz.
    // 10 ms = PIT_FREQUENCY / 100 = 11,932 ticks.
    #[allow(clippy::arithmetic_side_effects)]
    let pit_reload: u16 = (PIT_FREQUENCY / 100) as u16; // ~11,932

    // SAFETY: All PIT operations use standard x86 I/O ports.
    unsafe {
        // Disable PIT channel 2 gate (speaker) and read current status.
        let nmi_val = port::inb(NMI_STATUS);
        // Clear bits 0 (gate) and 1 (speaker data) — disable gate.
        port::outb(NMI_STATUS, (nmi_val & 0xFC) | 0x01);

        // Program PIT channel 2: mode 0 (interrupt on terminal count),
        // binary, lobyte/hibyte access.
        // Command: 0b1011_0000 = channel 2, lobyte/hibyte, mode 0, binary.
        port::outb(PIT_COMMAND, 0xB0);

        // Write the reload value (low byte first, then high byte).
        port::outb(PIT_CH2_DATA, (pit_reload & 0xFF) as u8);
        port::outb(PIT_CH2_DATA, (pit_reload >> 8) as u8);

        // Set up APIC timer: one-shot, divide by 16, maximum count.
        apic_write(APIC_TIMER_DIVIDE, 0x03); // divide by 16
        apic_write(APIC_TIMER_LVT, LVT_MASKED | u32::from(TIMER_VECTOR)); // masked one-shot

        // Start APIC timer with maximum initial count.
        apic_write(APIC_TIMER_INITIAL, 0xFFFF_FFFF);

        // Re-enable PIT channel 2 gate to start the countdown.
        let nmi_val = port::inb(NMI_STATUS);
        port::outb(NMI_STATUS, (nmi_val & 0xFC) | 0x01);

        // Busy-wait for PIT channel 2 to finish counting.
        // Bit 5 of port 0x61 = channel 2 output (goes high when count
        // reaches zero in mode 0).
        loop {
            let status = port::inb(NMI_STATUS);
            if status & 0x20 != 0 {
                break; // PIT channel 2 output went high — countdown complete.
            }
        }

        // Stop the APIC timer by writing 0 to initial count.
        let current = apic_read(APIC_TIMER_CURRENT);
        apic_write(APIC_TIMER_INITIAL, 0);

        // Calculate elapsed APIC ticks.
        0xFFFF_FFFF_u32.wrapping_sub(current)
    }
}

// ---------------------------------------------------------------------------
// SMP support — IPI sending and AP initialization
// ---------------------------------------------------------------------------

/// Read the Local APIC ID for the current CPU.
///
/// The APIC ID is in bits [31:24] of the APIC ID register.
#[must_use]
pub fn read_id() -> u8 {
    // SAFETY: APIC must be initialized.
    #[allow(clippy::cast_possible_truncation)]
    let id = unsafe { apic_read(APIC_ID) } >> 24;
    id as u8
}

/// Get the BSP's APIC ID (set during `init()`).
#[must_use]
pub fn bsp_id() -> u8 {
    BSP_APIC_ID.load(core::sync::atomic::Ordering::Relaxed)
}

/// Get the calibrated APIC timer count (ticks per 10 ms).
///
/// Returns 0 if calibration hasn't been done yet.
#[must_use]
#[allow(dead_code)] // Public API for timer frequency inspection.
pub fn calibrated_count() -> u32 {
    CALIBRATED_TIMER_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}

/// Wait for the ICR delivery status bit to clear (IPI accepted).
///
/// Spins until the APIC reports the ICR is idle.  On real hardware
/// this is typically immediate; under QEMU it may take a few cycles.
fn wait_icr_idle() {
    // Bit 12 of ICR low = delivery status (1 = pending).
    loop {
        // SAFETY: APIC is initialized, ICR_LOW is a valid register.
        let icr = unsafe { apic_read(APIC_ICR_LOW) };
        if icr & (1 << 12) == 0 {
            break;
        }
        core::hint::spin_loop();
    }
}

/// Send an INIT IPI to a specific AP (by APIC ID).
///
/// The INIT IPI resets the target processor to its INIT state.
/// After sending INIT, wait 10 ms before sending a SIPI.
///
/// # Safety
///
/// APIC must be initialized.  Must only be called from the BSP.
pub unsafe fn send_init_ipi(apic_id: u8) {
    wait_icr_idle();

    // ICR high: destination APIC ID in bits [31:24].
    // SAFETY: Valid APIC register write.
    unsafe {
        apic_write(APIC_ICR_HIGH, u32::from(apic_id) << 24);
    }

    // ICR low: INIT delivery mode (101), level assert, edge trigger.
    // Bits: vector=0, delivery=INIT(0b101=5), dest_mode=physical(0),
    //       level=assert(1), trigger=level(1).
    // = 0x0000_C500
    // SAFETY: Valid APIC register write, triggers the IPI.
    unsafe {
        apic_write(APIC_ICR_LOW, 0x0000_C500);
    }

    wait_icr_idle();

    // De-assert INIT (required sequence).
    // SAFETY: Valid APIC register write.
    unsafe {
        apic_write(APIC_ICR_HIGH, u32::from(apic_id) << 24);
        apic_write(APIC_ICR_LOW, 0x0000_8500); // INIT, de-assert, level
    }

    wait_icr_idle();
}

/// Send a Startup IPI (SIPI) to a specific AP.
///
/// `vector` is the page number of the real-mode entry point.
/// For example, if the trampoline is at physical 0x8000, vector = 0x08.
///
/// # Safety
///
/// APIC must be initialized.  The trampoline code must be in place at
/// `vector * 0x1000`.  Must only be called from the BSP.
pub unsafe fn send_sipi(apic_id: u8, vector: u8) {
    wait_icr_idle();

    // ICR high: destination APIC ID.
    // SAFETY: Valid APIC register write.
    unsafe {
        apic_write(APIC_ICR_HIGH, u32::from(apic_id) << 24);
    }

    // ICR low: SIPI delivery mode (110), vector = page number.
    // = 0x0000_0600 | vector
    // SAFETY: Valid APIC register write, triggers the SIPI.
    unsafe {
        apic_write(APIC_ICR_LOW, 0x0000_0600 | u32::from(vector));
    }

    wait_icr_idle();
}

/// Send a fixed-mode IPI with the given vector to all CPUs except self.
///
/// Uses the "all excluding self" shorthand destination (ICR bits 19:18 = 11)
/// so no specific APIC ID is needed.  The receiving CPUs will execute the
/// ISR registered at `vector`.
///
/// # Safety
///
/// APIC must be initialized.  The vector must have a valid ISR in the IDT.
pub unsafe fn send_ipi_all_excluding_self(vector: u8) {
    wait_icr_idle();

    // ICR low: fixed delivery (000), physical dest, edge, all-excl-self.
    // Bits 19:18 = 11 (all excluding self), delivery = fixed (000).
    // = 0x000C_0000 | vector
    // SAFETY: Valid APIC register write, triggers the IPI.
    unsafe {
        apic_write(APIC_ICR_LOW, 0x000C_0000 | u32::from(vector));
    }

    wait_icr_idle();
}

/// Send a fixed-mode IPI to a specific CPU (by APIC ID).
///
/// The target CPU's ISR at `vector` will fire.  Used for targeted
/// wake-ups such as reschedule IPIs (only wake the CPU that has new
/// work, not all CPUs).
///
/// # Safety
///
/// APIC must be initialized.  The vector must have a valid ISR in the IDT.
/// Must not send to the current CPU (self-IPI has a different mechanism
/// and could cause re-entrancy issues in ISR context).
pub unsafe fn send_fixed_ipi(apic_id: u8, vector: u8) {
    wait_icr_idle();

    // ICR high: destination APIC ID in bits [31:24].
    // SAFETY: Valid APIC register write.
    unsafe {
        apic_write(APIC_ICR_HIGH, u32::from(apic_id) << 24);
    }

    // ICR low: fixed delivery (000), physical dest, edge trigger.
    // Bits 19:18 = 00 (no shorthand — use specific destination).
    // SAFETY: Valid APIC register write, triggers the IPI.
    unsafe {
        apic_write(APIC_ICR_LOW, u32::from(vector));
    }

    wait_icr_idle();
}

/// Reschedule IPI vector.
///
/// Sent to wake an idle CPU from HLT when new work is enqueued on its
/// queue.  The ISR handler just does EOI and returns — the idle loop
/// then checks the `RESCHEDULE_PENDING` flag and yields.
pub const RESCHEDULE_VECTOR: u8 = 252;

// ---------------------------------------------------------------------------
// Tickless idle — stop/restart timer for idle CPUs
// ---------------------------------------------------------------------------

/// Stop the APIC timer on the current CPU (for tickless idle).
///
/// Masks the timer LVT entry so no more timer interrupts are delivered
/// to this CPU.  The CPU can still be woken by the reschedule IPI
/// (vector 252) when new work is enqueued.
///
/// This should only be called on APs entering idle.  The BSP (CPU 0)
/// must keep its timer running because it drives the global `tick_count`
/// and fires `TIMER_SOFTIRQ` for kernel timer expirations.
///
/// Call [`restart_timer`] when the CPU picks up a task and needs
/// preemptive time-slice enforcement again.
///
/// # Safety
///
/// APIC must be initialized on this CPU.  Must be called with interrupts
/// disabled or from a context where a timer interrupt won't race.
pub unsafe fn stop_timer() {
    // Mask the timer LVT entry (set bit 16).  The timer counter keeps
    // running but no interrupt is delivered.  This is cheaper than
    // setting initial_count=0 because we don't need to recalibrate
    // when restarting.
    //
    // SAFETY: APIC is initialized, timer LVT is a valid register.
    unsafe {
        apic_write(APIC_TIMER_LVT, LVT_MASKED | TIMER_MODE_PERIODIC | u32::from(TIMER_VECTOR));
    }
}

/// Restart the APIC timer on the current CPU (leaving tickless idle).
///
/// Unmasks the timer LVT entry and reprograms the initial count so
/// the periodic timer resumes.  Call this when the CPU transitions
/// from idle to running a task.
///
/// # Safety
///
/// APIC must be initialized on this CPU.
pub unsafe fn restart_timer() {
    let count = CALIBRATED_TIMER_COUNT.load(core::sync::atomic::Ordering::Relaxed);
    if count == 0 {
        return; // No calibration — timer was never started.
    }

    // SAFETY: APIC is initialized, writing valid register values.
    unsafe {
        // Unmask and set periodic mode.
        apic_write(APIC_TIMER_LVT, TIMER_MODE_PERIODIC | u32::from(TIMER_VECTOR));
        // Restart the countdown from the calibrated 10 ms value.
        // Writing initial_count restarts the counter from this value.
        apic_write(APIC_TIMER_INITIAL, count);
    }
}

/// Reprogram the APIC timer for an earlier deadline (nanoseconds from now).
///
/// Used by the hrtimer subsystem to get sub-tick precision: if the next
/// timer expires in less than one full tick (10ms), shorten the current
/// periodic interval.  On the next interrupt, the timer fires, hrtimers
/// process the expired entry, and the full periodic rate is restored.
///
/// This does NOT switch to one-shot mode — it merely shortens the current
/// period.  The next `handle_timer_irq` will call `restore_periodic_rate`
/// to reset the full 10ms period.
///
/// # Arguments
///
/// - `delay_ns`: Nanoseconds until the timer should fire.  Clamped to
///   a minimum of 1µs (avoid programming a count of 0 which would stop
///   the timer on some hardware).
///
/// # Safety
///
/// APIC must be initialized.  Should only be called from contexts where
/// the APIC timer LVT won't be concurrently modified (e.g., from the
/// timer ISR itself or with interrupts disabled).
pub fn shorten_tick_for_hrtimer(delay_ns: u64) {
    let cal = CALIBRATED_TIMER_COUNT.load(core::sync::atomic::Ordering::Relaxed);
    if cal == 0 {
        return; // Not calibrated yet.
    }

    // Convert delay_ns to APIC timer ticks.
    // cal ticks = 10_000_000 ns, so ticks_per_ns = cal / 10_000_000.
    // target_count = delay_ns * cal / 10_000_000.
    // Use u64 math to avoid overflow for large delays.
    let target = (delay_ns.saturating_mul(u64::from(cal))) / 10_000_000;

    // Clamp: minimum 100 ticks (~1µs at typical APIC frequencies) to
    // avoid races where the timer expires before we finish reprogramming.
    // Maximum: don't exceed the full period (no point programming longer).
    let target_clamped = target.clamp(100, u64::from(cal)) as u32;

    // If the shortened interval would be >= full period, don't bother.
    if target_clamped >= cal {
        return;
    }

    // Reprogram: writing APIC_TIMER_INITIAL restarts the countdown
    // from the new value immediately.  The timer remains in periodic mode.
    //
    // SAFETY: APIC is initialized, we're programming a valid count.
    unsafe {
        apic_write(APIC_TIMER_INITIAL, target_clamped);
    }

    // Track that we shortened this tick so the ISR knows to restore.
    TICK_SHORTENED.store(true, core::sync::atomic::Ordering::Release);

    // Trace: tick shortened (arg0 = delay_ns requested, arg1 = actual ticks programmed).
    crate::ktrace::record(
        crate::ktrace::Category::Timer,
        crate::ktrace::event::TIMER_TICK_SHORT,
        delay_ns,
        u64::from(target_clamped),
    );
}

/// Restore the APIC timer to its full periodic rate (10ms / 100 Hz).
///
/// Called at the top of `handle_timer_irq` when a shortened tick was
/// programmed.  This ensures subsequent ticks are at the normal 100 Hz
/// rate even if no more hrtimers are pending.
#[inline]
fn restore_periodic_rate() {
    let cal = CALIBRATED_TIMER_COUNT.load(core::sync::atomic::Ordering::Relaxed);
    if cal > 0 {
        // SAFETY: APIC is initialized, restoring the calibrated count.
        unsafe {
            apic_write(APIC_TIMER_INITIAL, cal);
        }
    }
    TICK_SHORTENED.store(false, core::sync::atomic::Ordering::Release);
}

/// Whether the current tick period has been shortened for an hrtimer.
static TICK_SHORTENED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Initialize the Local APIC on an Application Processor.
///
/// Reuses the BSP's calibrated timer count (APs don't recalibrate via PIT).
/// Sets up the spurious vector, enables the APIC, and starts the periodic
/// timer with the same configuration as the BSP.
///
/// # Safety
///
/// Must be called exactly once per AP during SMP bootstrap, after the
/// AP has loaded its GDT and IDT.  Interrupts must be disabled.
pub unsafe fn init_ap() {
    let apic_base_virt = APIC_BASE_VIRT.load(core::sync::atomic::Ordering::Acquire);
    debug_assert!(apic_base_virt != 0, "BSP must init APIC first");

    // Read this AP's APIC ID for diagnostics.
    let apic_id = read_id();
    serial_println!("[apic] AP LAPIC ID={} initializing", apic_id);

    // Enable the APIC via the spurious vector register.
    // SAFETY: APIC base is valid (shared with BSP — same physical MMIO).
    unsafe {
        apic_write(APIC_SPURIOUS, 0x100 | u32::from(SPURIOUS_VECTOR));
    }

    // Start the periodic timer with the BSP's calibrated count.
    let count = CALIBRATED_TIMER_COUNT.load(core::sync::atomic::Ordering::Acquire);
    if count > 0 {
        configure_periodic_timer(count);
        serial_println!(
            "[apic] AP {} timer started (count={}, {} Hz)",
            apic_id, count, TICK_RATE_HZ
        );
    } else {
        serial_println!("[apic] WARNING: AP {} — no calibrated timer count", apic_id);
    }
}

// ---------------------------------------------------------------------------
// Timer interrupt handler
// ---------------------------------------------------------------------------

/// Called from the timer ISR (vector 32) assembly stub.
///
/// This function:
/// 1. Increments the global tick counter (BSP only).
/// 2. Calls the scheduler's `timer_tick()` (per-CPU, no global lock).
/// 3. Sends LAPIC EOI.
/// 4. Raises softirqs for deferred work (timer expirations, IRQ poll).
/// 5. Processes pending softirqs with interrupts re-enabled.
/// 6. If the scheduler says "reschedule", triggers a context switch.
///
/// ## Softirq integration
///
/// Previously this handler ran deferred wake-ups, sleep-queue scans,
/// and timer expirations inline with interrupts disabled.  Now those
/// are deferred to softirq handlers that run with interrupts enabled,
/// so device IRQs are not blocked during the processing.
///
/// # Safety
///
/// Called from an interrupt handler with interrupts disabled.  Must
/// not acquire locks that could be held by the interrupted code.
#[unsafe(no_mangle)]
pub extern "C" fn handle_timer_irq(frame: &crate::idt::InterruptStackFrame, _error: u64) {
    // --- CPU time accounting: entering IRQ context ---
    crate::cputime::enter_irq();

    // Are we a *nested* timer IRQ?  `enter_irq` has just bumped this CPU's
    // hardirq depth, so depth > 1 means an outer IRQ handler was already
    // running (with interrupts re-enabled) when this timer fired and nested
    // on the same per-CPU IRQ stack.
    //
    // A nested timer handler MUST NOT re-enable interrupts: it skips softirq
    // processing (the outer frame owns that) and skips the pre-preempt `sti`
    // below, running its whole body with IF=0.  Because the timer IDT entry
    // is an interrupt gate (IF cleared on entry) and we never set IF back,
    // no further timer can fire until this nested frame returns — so
    // timer-on-timer nesting is capped at depth 2 no matter how slow an
    // individual handler is (e.g. the poison-debug heap).  Without this cap,
    // a handler that exceeds the ~10 ms tick period lets timer IRQs pile up
    // on the fixed 16 KiB IRQ stack until it overflows the guard page — a
    // fatal kernel #PF (root cause of the intermittent boot wedge).
    let nested = crate::cputime::irq_depth() > 1;

    // If the previous tick was shortened for hrtimer precision, restore
    // the full periodic rate immediately so subsequent ticks are normal.
    if TICK_SHORTENED.load(core::sync::atomic::Ordering::Relaxed) {
        restore_periodic_rate();
    }

    // --- RIP sampling: record where the CPU was when interrupted ---
    // This is the core of the statistical profiler — captures the
    // instruction pointer at each timer tick for performance analysis.
    crate::rip_sample::record(frame.rip, crate::smp::current_cpu_index() as u8);

    // Always-on per-CPU last-RIP snapshot for hang diagnostics (independent of
    // the opt-in profiler above).  The liveness watchdog reads this to report
    // *where* each CPU was executing at the moment the system wedged.
    crate::rip_sample::record_last_rip(frame.rip, crate::smp::current_cpu_index());

    // Always-on per-CPU recent-RIP *history* ring, paired with the single-RIP
    // snapshot above.  A lone RIP/RBP is a known red herring for livelocks (the
    // async tick rarely lands on a frame boundary, so the walked stack is
    // stale); the set of the last N RIPs instead reveals a spin loop directly.
    crate::rip_sample::record_rip_history(frame.rip, crate::smp::current_cpu_index());

    // Always-on per-CPU last-RBP (frame pointer) snapshot, paired with the RIP
    // above.  The liveness SYSTEM-HANG dump feeds this to `backtrace::print_from`
    // to walk the wedged CPU's call stack — turning an inconclusive single RIP
    // into a full backtrace (the same diagnostic the NMI hard-lockup path emits).
    //
    // The interrupted RBP is not in the CPU-pushed `InterruptStackFrame`; it is a
    // general register the IRQ stub saved.  The `irq_stub!` macro pushes, below
    // the CPU frame: dummy-error, rax, rcx, rdx, rbx, rbp, … and loads
    // `rdi = rsp + 128` (the `frame` pointer).  Counting down from `frame`:
    // [-8]=error, [-16]=rax, [-24]=rcx, [-32]=rdx, [-40]=rbx, [-48]=rbp — so the
    // saved RBP is 6 words below `frame`.  `frame` still points at the
    // interrupted task stack even though this handler runs on the IRQ stack
    // (`run_on_irq_stack` relocates RSP, not the frame), so the save area is
    // valid, mapped kernel stack.
    let frame_ptr = frame as *const crate::idt::InterruptStackFrame as *const u64;
    // SAFETY: `frame_ptr` = interrupted `rsp + 128` (set by the IRQ stub), so the
    // six words below it are the pushed dummy-error + rax/rcx/rdx/rbx/rbp save
    // slots — all within the mapped interrupted kernel stack.  8-byte aligned;
    // volatile so the read is not elided/reordered.  For a ring-3 interrupt this
    // reads a user RBP value, which is only ever *stored* here (never walked
    // unless it validates as a kernel frame pointer in the dump), so it is safe.
    let interrupted_rbp = unsafe { core::ptr::read_volatile(frame_ptr.sub(6)) };
    crate::rip_sample::record_last_rbp(interrupted_rbp, crate::smp::current_cpu_index());

    // --- ISR latency measurement: record entry TSC ---
    //
    // When benchmarking is active, capture the TSC at ISR entry and after
    // EOI to measure the hard-IRQ phase.  The Relaxed load is ~1 cycle
    // when measurement is inactive (branch not taken).
    let measure_start = if ISR_MEASURE_ACTIVE.load(core::sync::atomic::Ordering::Relaxed) {
        crate::bench::rdtsc()
    } else {
        0
    };

    // Per-CPU heartbeat for soft lockup detection.  Every CPU increments
    // its own counter so the BSP can detect stalled APs.
    crate::watchdog::heartbeat();

    // Only the BSP (CPU 0) increments the global tick counter.
    // Without this guard, each online CPU increments independently,
    // causing tick_count() to advance at N× rate (e.g., 200 Hz with
    // 2 CPUs instead of the expected 100 Hz).  All timing code
    // (sleep, timers, uptime) depends on tick_count() being wall-clock
    // rate.  APs still get preemption from their timer ISR — they just
    // don't double-count the global tick.
    if crate::smp::current_cpu_index() == 0 {
        TICK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        // --- Timer jitter tracking ---
        //
        // Measure the TSC interval between consecutive timer interrupts
        // on the BSP.  Ideal interval is constant (TSC_freq / TICK_RATE_HZ).
        // Variance indicates long critical sections, NMIs, or SMIs that
        // delayed interrupt delivery.  Cost: 1 rdtsc + 2 atomic stores
        // per tick (~30 cycles total on modern x86).
        let now_tsc = crate::bench::rdtsc();
        let prev_tsc = JITTER_LAST_TSC.swap(now_tsc, core::sync::atomic::Ordering::Relaxed);
        if prev_tsc != 0 {
            let delta = now_tsc.saturating_sub(prev_tsc);
            JITTER_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            JITTER_SUM.fetch_add(delta, core::sync::atomic::Ordering::Relaxed);
            // Update min (CAS loop).
            let _ = JITTER_MIN.fetch_update(
                core::sync::atomic::Ordering::Relaxed,
                core::sync::atomic::Ordering::Relaxed,
                |cur| if delta < cur { Some(delta) } else { None },
            );
            // Update max (CAS loop).
            let _ = JITTER_MAX.fetch_update(
                core::sync::atomic::Ordering::Relaxed,
                core::sync::atomic::Ordering::Relaxed,
                |cur| if delta > cur { Some(delta) } else { None },
            );
        }
    }

    // Tick the scheduler and check if a reschedule is needed.
    // This is the minimal hard-IRQ work: per-CPU lock only, no global
    // lock, O(1) time-slice decrement.
    //
    // The CPL bits (low 2) of the interrupted frame's CS tell us whether
    // the timer preempted ring-3 (user) or ring-0 (kernel) code, so the
    // scheduler can charge this tick to the current task's user- or
    // system-time bucket (Linux tick-sampling CPU accounting).
    let from_user = (frame.cs & 0x3) == 0x3;
    let needs_reschedule = crate::sched::timer_tick(from_user);

    // Fire any high-resolution timers that have expired.
    // Checked every tick (~10 ms); actual precision depends on HPET timestamps.
    crate::hrtimer::process_expired();

    // --- hrtimer precision: shorten tick for imminent deadlines ---
    //
    // If the next hrtimer expires before the next full tick (~10ms),
    // reprogram the APIC timer to fire earlier.  This gives hrtimers
    // sub-millisecond precision without switching to full one-shot mode.
    if let Some(next_ns) = crate::hrtimer::next_expiry_ns() {
        let now_ns = crate::hrtimer::now_ns();
        if next_ns > now_ns {
            let delta_ns = next_ns.saturating_sub(now_ns);
            // Only shorten if the deadline is less than one full tick away.
            // 10_000_000 ns = 10 ms = one tick at 100 Hz.
            if delta_ns < 10_000_000 {
                shorten_tick_for_hrtimer(delta_ns);
            }
        }
    }

    // Send EOI before softirq processing — this allows the LAPIC to
    // deliver new interrupts (including on other CPUs).
    //
    // SAFETY: We're in an interrupt handler, APIC is initialized.
    unsafe {
        eoi();
    }

    // --- ISR latency measurement: record hard-IRQ duration ---
    //
    // Captures entry → post-EOI cycles.  This is the time interrupts
    // were disabled and other devices were blocked.  We measure after
    // EOI because that's when the LAPIC is unblocked.
    if measure_start != 0 {
        let measure_end = crate::bench::rdtsc();
        let elapsed = measure_end.saturating_sub(measure_start);
        // Update min (CAS loop to atomically compute min).
        let _ = ISR_HARD_MIN.fetch_update(
            core::sync::atomic::Ordering::Relaxed,
            core::sync::atomic::Ordering::Relaxed,
            |current| {
                if elapsed < current { Some(elapsed) } else { None }
            },
        );
        // Update max (CAS loop to atomically compute max).
        let _ = ISR_HARD_MAX.fetch_update(
            core::sync::atomic::Ordering::Relaxed,
            core::sync::atomic::Ordering::Relaxed,
            |current| {
                if elapsed > current { Some(elapsed) } else { None }
            },
        );
        ISR_HARD_TOTAL.fetch_add(elapsed, core::sync::atomic::Ordering::Relaxed);
        ISR_MEASURE_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    // Feed interrupt timing jitter into the kernel CSPRNG entropy pool.
    // The TSC value at ISR entry contains genuine hardware noise from
    // pipeline effects, cache misses, and inter-interrupt timing.
    // Cost: one atomic XOR + one atomic add (~2 cycles).
    crate::rng::add_interrupt_entropy(crate::bench::rdtsc());

    // Raise softirqs for deferred work.  These will be processed below
    // with interrupts re-enabled, so device IRQs can preempt.
    //
    // TIMER_SOFTIRQ: sleep-queue wakeups + IPC timer expirations.
    // IRQ_POLL_SOFTIRQ: retry deferred IRQ wakes for userspace drivers.
    crate::softirq::raise(
        crate::softirq::TIMER_SOFTIRQ | crate::softirq::IRQ_POLL_SOFTIRQ,
    );

    // Process all pending softirqs (including any raised by device ISRs
    // that fired on this CPU since the last timer tick).  This re-enables
    // interrupts internally (STI), runs handlers, then disables them
    // again (CLI) before returning.
    //
    // Only the OUTERMOST timer handler processes softirqs.  A nested timer
    // must not re-enable interrupts (see the `nested` computation above),
    // so we skip this entirely when nested — any bits we raised will be
    // drained by the outer frame's own process_pending loop, exactly as if
    // process_pending's internal IN_SOFTIRQ re-entry guard had short-
    // circuited us, but without ever toggling IF.
    //
    // SAFETY: EOI has been sent, assembly stub expects CLI on return
    // (process_pending guarantees this; when skipped IF is already clear).
    if !nested {
        unsafe {
            crate::softirq::process_pending();
        }
    }

    // Re-enable interrupts before potential preemption — OUTERMOST timer
    // only.  A nested timer handler must stay IF=0 through its return so
    // no further timer can fire before it unwinds (bounding IRQ-stack
    // nesting to depth 2); it also never runs do_deferred_preempt (the
    // outer IRQ frame owns preemption), so it has no reason to enable
    // interrupts here.
    //
    // process_pending() returns with interrupts disabled (CLI).  If we
    // context-switch via preempt() below, switch_context saves the
    // current task's RFLAGS — including IF.  Without this STI, the
    // preempted task would be saved with IF=0, and when later resumed
    // via a voluntary yield path (no IRETQ), it would run with
    // interrupts permanently disabled on that CPU.
    //
    // Re-enabling here is safe: EOI has been sent (LAPIC won't re-
    // deliver this vector), and IRETQ atomically restores the original
    // RFLAGS regardless of the current IF state.
    //
    // SAFETY: Interrupts are safe to enable — all ISR-critical work is
    // done, and the remaining code (preempt check + context switch) is
    // designed to run with interrupts enabled.
    if !nested {
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
        }
    }

    // --- CPU time accounting: leaving IRQ context ---
    crate::cputime::exit_irq();

    // If the scheduler says the time slice expired, request a *deferred*
    // preemption rather than context-switching here.
    //
    // The IRQ entry path (idt::irq_common_dispatch) runs hardware IRQ
    // handlers on a dedicated per-CPU IRQ stack (B-DF1 / open-questions Q7,
    // option A).  A context switch performed *inside* this handler would
    // therefore record the transient IRQ-stack RSP as the task's resume
    // point — corrupting it.  Instead we set the NEED_RESCHED flag here; the
    // outermost IRQ frame services it via sched::do_deferred_preempt() after
    // RSP has been restored to the interrupted task's kernel stack.
    //
    // request_preempt() only sets a flag; do_deferred_preempt() applies the
    // same guards the in-handler check used to (skip the idle fallback, skip
    // softirq context) before actually calling preempt().
    if needs_reschedule {
        crate::sched::request_preempt();
    }
}

/// Handler for reschedule IPI (vector 252).
///
/// Sent by [`crate::sched::signal_cpu`] when work is enqueued on this
/// CPU's run queue.  The ISR's only job is to send EOI and return —
/// its purpose is to break the CPU out of HLT.  The idle loop then
/// checks [`crate::sched::reschedule_pending`] and yields.
///
/// No scheduling is done in the ISR itself to avoid deadlock with code
/// that holds the SCHED lock when interrupted.
#[unsafe(no_mangle)]
pub extern "C" fn handle_reschedule_irq(
    _frame: &crate::idt::InterruptStackFrame,
    _error: u64,
) {
    // SAFETY: APIC is initialized.  EOI is required for all non-spurious
    // interrupts to clear the in-service bit and allow further interrupts.
    unsafe {
        eoi();
    }
}

/// Handler for spurious interrupts (vector 255).
///
/// Spurious interrupts can occur when the APIC signals an interrupt
/// that was retracted before delivery.  No EOI is sent for spurious
/// interrupts (per Intel SDM).
#[unsafe(no_mangle)]
pub extern "C" fn handle_spurious_irq(
    _frame: &crate::idt::InterruptStackFrame,
    _error: u64,
) {
    // No EOI for spurious interrupts.
    // Optionally count them for diagnostics.
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run APIC self-test: verify timer is ticking.
///
/// This should be called after `init()` and after interrupts are enabled.
/// It verifies that timer interrupts are actually firing by checking
/// the tick counter.
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[apic] Running APIC timer self-test...");

    let start = tick_count();

    // Spin for a short while — if the timer is running at 100 Hz,
    // we should see several ticks in ~50 ms of spinning.
    // We can't use proper timing without a calibrated delay, so
    // just spin for a large number of iterations and check.
    for _ in 0..10_000_000_u64 {
        core::hint::spin_loop();
    }

    let end = tick_count();
    let ticks = end.wrapping_sub(start);

    if ticks == 0 {
        serial_println!("[apic]   FAIL: No timer ticks observed");
        return Err(crate::error::KernelError::InternalError);
    }

    serial_println!("[apic]   Timer ticks observed: {} (OK)", ticks);
    serial_println!("[apic] APIC timer self-test PASSED");
    Ok(())
}
