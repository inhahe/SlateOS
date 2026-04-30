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
const TICK_RATE_HZ: u32 = 100;

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Virtual address of the APIC base (set during init).
static APIC_BASE_VIRT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Whether the APIC timer is running.
static TIMER_ACTIVE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Tick counter — incremented on every timer interrupt.
static TICK_COUNT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

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

/// Initialize the Local APIC and start the periodic timer.
///
/// # Safety
///
/// - Must be called exactly once during boot.
/// - The GDT, IDT, and heap must already be initialized.
/// - Interrupts must be disabled.
pub unsafe fn init() {
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
    let hhdm = page_table::hhdm().expect("HHDM not initialized");
    let apic_base_virt = apic_base_phys + hhdm;

    // Map the APIC MMIO page (16 KiB frame covering 0xFEE00000).
    // Use PRESENT | WRITABLE | NO_CACHE flags for MMIO.
    let apic_frame = PhysFrame::from_addr(apic_base_phys)
        .expect("APIC base not frame-aligned");
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

    if ticks_per_10ms == 0 {
        serial_println!("[apic] WARNING: Timer calibration failed (0 ticks). Using fallback.");
        // Fallback: set a reasonable default for QEMU (~100 MHz bus / 16 divider).
        configure_periodic_timer(625_000);
    } else {
        // Configure periodic mode at TICK_RATE_HZ.
        // ticks_per_10ms gives us the count for 10 ms (100 Hz).
        // For other rates: initial_count = ticks_per_10ms * 100 / TICK_RATE_HZ.
        // But since we want 100 Hz (= 10 ms), just use ticks_per_10ms directly.
        configure_periodic_timer(ticks_per_10ms);
    }

    serial_println!(
        "[apic] Timer configured: {} Hz periodic, vector {}",
        TICK_RATE_HZ,
        TIMER_VECTOR
    );
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
// Timer interrupt handler
// ---------------------------------------------------------------------------

/// Called from the timer ISR (vector 32) assembly stub.
///
/// This function:
/// 1. Increments the global tick counter.
/// 2. Calls the scheduler's `timer_tick()`.
/// 3. Sends EOI to the APIC.
/// 4. If the scheduler says "reschedule", triggers a context switch.
///
/// # Safety
///
/// Called from an interrupt handler with interrupts disabled.  Must
/// not acquire locks that could be held by the interrupted code.
#[unsafe(no_mangle)]
pub extern "C" fn handle_timer_irq(_frame: &crate::idt::InterruptStackFrame, _error: u64) {
    TICK_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    // Tick the scheduler and check if a reschedule is needed.
    let needs_reschedule = crate::sched::timer_tick();

    // Send EOI before potentially context-switching.
    // SAFETY: We're in an interrupt handler, APIC is initialized.
    unsafe {
        eoi();
    }

    // If the scheduler says the time slice expired, reschedule.
    if needs_reschedule {
        crate::sched::preempt();
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
