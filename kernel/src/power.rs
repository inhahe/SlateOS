//! System power management — shutdown, reboot, and power state control.
//!
//! Provides ACPI-based system shutdown (S5) and reboot functionality
//! with multiple fallback mechanisms for different hardware.
//!
//! ## Shutdown Methods (tried in order)
//!
//! 1. **ACPI S5**: Write (SLP_TYP | SLP_EN) to PM1a_CNT_BLK register.
//!    Requires FADT parsing and DSDT scan for the \_S5_ sleep type value.
//! 2. **QEMU/Bochs exit**: Write to port 0x604 (QEMU isa-debug-exit) or
//!    port 0xB004 (Bochs/older QEMU shutdown port).
//! 3. **Halt loop**: If nothing else works, disable interrupts and halt
//!    in an infinite loop. The system is frozen but not powered off.
//!
//! ## Reboot Methods (tried in order)
//!
//! 1. **ACPI reset register**: Write reset value to the FADT-specified
//!    reset register (ACPI 2.0+ with RESET_REG_SUP flag).
//! 2. **Keyboard controller**: Pulse 0xFE to port 0x64 (standard 8042
//!    reset line). Works on nearly all x86 hardware.
//! 3. **Triple fault**: Load a null IDT and trigger an exception.
//!    The CPU has no handler → triple fault → hardware reset.
//!
//! ## Integration
//!
//! - ACPI init stores the FADT data via `set_power_info()`.
//! - Kshell provides `shutdown` and `reboot` commands.
//! - The kernel panic handler can call `emergency_reboot()`.
//!
//! ## References
//!
//! - ACPI Specification 6.5, §4.8.3.4 (Sleep State transitions)
//! - ACPI Specification 6.5, §4.8.3.6 (System Reset)
//! - Linux `kernel/reboot.c`, `drivers/acpi/sleep.c`
//! - OSDev Wiki: Shutdown, Reboot

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;
use crate::serial_println;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Cached power management info from FADT parsing.
static POWER_INFO: Mutex<Option<PowerState>> = Mutex::new(None);

/// Whether initialization has completed.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Power management state extracted from ACPI tables.
#[derive(Debug, Clone, Copy)]
struct PowerState {
    /// PM1a Control Block I/O port address.
    pm1a_cnt: u16,
    /// PM1b Control Block I/O port address (0 if absent).
    pm1b_cnt: u16,
    /// SLP_TYP value for S5 state (extracted from DSDT or default).
    slp_typ_s5: u8,
    /// Whether ACPI reset register is available.
    has_reset_reg: bool,
    /// Reset register address space (1 = system I/O).
    reset_addr_space: u8,
    /// Reset register address.
    reset_address: u64,
    /// Value to write to trigger reset.
    reset_value: u8,
}

// ---------------------------------------------------------------------------
// ACPI PM1 Control Register bits
// ---------------------------------------------------------------------------

/// SLP_EN bit (bit 13) — triggers the sleep state transition.
const SLP_EN: u16 = 1 << 13;

/// SLP_TYP shift — bits 10-12 encode the sleep type.
const SLP_TYP_SHIFT: u16 = 10;

// ---------------------------------------------------------------------------
// Well-known I/O ports
// ---------------------------------------------------------------------------

/// QEMU isa-debug-exit device port (default when `-device isa-debug-exit` used).
const QEMU_EXIT_PORT: u16 = 0x604;

/// Bochs/older QEMU shutdown port.
const BOCHS_SHUTDOWN_PORT: u16 = 0xB004;

/// PS/2 keyboard controller command port.
const KBD_CTRL_PORT: u16 = 0x64;

/// PS/2 keyboard controller data port.
const KBD_DATA_PORT: u16 = 0x60;

/// Keyboard controller reset command.
const KBD_RESET_CMD: u8 = 0xFE;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Store power management info (called from ACPI init after FADT parsing).
pub fn set_power_info(info: &crate::acpi::fadt::PowerInfo) {
    let state = PowerState {
        pm1a_cnt: info.pm1a_cnt_blk,
        pm1b_cnt: info.pm1b_cnt_blk,
        slp_typ_s5: info.slp_typ_s5,
        has_reset_reg: info.has_reset_reg,
        reset_addr_space: info.reset_addr_space,
        reset_address: info.reset_address,
        reset_value: info.reset_value,
    };
    *POWER_INFO.lock() = Some(state);
    INITIALIZED.store(true, Ordering::Release);
    serial_println!("[power] Power management initialized (PM1a={:#x}, SLP_TYP_S5={})",
        state.pm1a_cnt, state.slp_typ_s5);
}

/// Check if power management is initialized.
#[must_use]
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Attempt to shut down the system (enter ACPI S5 state).
///
/// This function tries multiple methods and does not return if successful.
/// If all methods fail, it enters a halt loop (system frozen but not off).
pub fn shutdown() -> ! {
    serial_println!("[power] Initiating system shutdown...");

    // Flush any buffered I/O.
    // (Future: call VFS sync here.)

    // Disable interrupts — we don't want anything interfering.
    // SAFETY: We're shutting down — disabling interrupts prevents any ISR
    // from interfering with the power transition sequence.
    unsafe { crate::cpu::cli(); }

    // Method 1: ACPI S5 via PM1a/PM1b control registers.
    if let Some(state) = *POWER_INFO.lock() {
        if state.pm1a_cnt != 0 {
            serial_println!("[power] Trying ACPI S5 (PM1a={:#x}, SLP_TYP={})",
                state.pm1a_cnt, state.slp_typ_s5);
            let val = (u16::from(state.slp_typ_s5) << SLP_TYP_SHIFT) | SLP_EN;
            // SAFETY: pm1a_cnt/pm1b_cnt are ACPI PM1 control register
            // ports parsed from the FADT.  Writing SLP_TYP+SLP_EN triggers S5.
            unsafe { outw(state.pm1a_cnt, val); }

            // If PM1b is present, write there too.
            if state.pm1b_cnt != 0 {
                unsafe { outw(state.pm1b_cnt, val); }
            }

            // Give hardware time to respond.
            io_delay();
        }
    }

    // Method 2: QEMU exit port.
    serial_println!("[power] ACPI S5 failed, trying QEMU exit port...");
    // SAFETY: QEMU_EXIT_PORT is the well-known QEMU debug exit port (0x604).
    unsafe { outw(QEMU_EXIT_PORT, 0x2000); }
    io_delay();

    // Method 3: Bochs/older QEMU shutdown port.
    // SAFETY: BOCHS_SHUTDOWN_PORT is the Bochs/old-QEMU shutdown port (0xB004).
    unsafe { outw(BOCHS_SHUTDOWN_PORT, 0x2000); }
    io_delay();

    // Method 4: Nothing worked — halt loop.
    serial_println!("[power] All shutdown methods failed — halting CPU");
    halt_loop()
}

/// Attempt to reboot the system.
///
/// Tries multiple methods in sequence. Does not return if successful.
/// Falls back to triple fault which should always trigger a hardware reset.
pub fn reboot() -> ! {
    serial_println!("[power] Initiating system reboot...");

    // Disable interrupts.
    // SAFETY: We're rebooting — no ISRs should fire during reset.
    unsafe { crate::cpu::cli(); }

    // Method 1: ACPI reset register (ACPI 2.0+).
    if let Some(state) = *POWER_INFO.lock() {
        if state.has_reset_reg && state.reset_address != 0 {
            serial_println!("[power] Trying ACPI reset register (space={}, addr={:#x}, val={:#x})",
                state.reset_addr_space, state.reset_address, state.reset_value);

            match state.reset_addr_space {
                1 => {
                    // System I/O space.
                    let port = state.reset_address as u16;
                    // SAFETY: Port from FADT reset register in I/O space.
                    unsafe { outb(port, state.reset_value); }
                }
                0 => {
                    // System memory space — translate via HHDM.
                    if let Some(hhdm) = crate::mm::page_table::hhdm() {
                        let addr = (state.reset_address.wrapping_add(hhdm)) as *mut u8;
                        // SAFETY: FADT-specified reset register mapped via HHDM.
                        unsafe { core::ptr::write_volatile(addr, state.reset_value); }
                    }
                }
                _ => {
                    serial_println!("[power] Unknown reset address space: {}",
                        state.reset_addr_space);
                }
            }
            io_delay();
        }
    }

    // Method 2: Keyboard controller reset (pulse reset line).
    serial_println!("[power] Trying keyboard controller reset...");
    // Wait for controller input buffer to be empty.
    // SAFETY: KBD_CTRL_PORT (0x64) is the standard x86 keyboard controller
    // port.  Reading its status and sending the reset command (0xFE) pulses
    // the CPU reset line — standard x86 reboot mechanism.
    for _ in 0..100_000 {
        let status = unsafe { inb(KBD_CTRL_PORT) };
        if status & 0x02 == 0 {
            break;
        }
    }
    unsafe { outb(KBD_CTRL_PORT, KBD_RESET_CMD); }
    io_delay();

    // Method 3: Triple fault — guaranteed hardware reset.
    serial_println!("[power] Keyboard reset failed, triggering triple fault...");
    triple_fault()
}

/// Emergency reboot — minimal path, no logging, for use in panic handlers.
///
/// Goes straight to keyboard controller reset, then triple fault.
pub fn emergency_reboot() -> ! {
    // SAFETY: Emergency path — disable interrupts and send keyboard
    // controller reset command.  Minimal code path for panic handlers.
    unsafe {
        crate::cpu::cli();
        // Keyboard controller reset.
        outb(KBD_CTRL_PORT, KBD_RESET_CMD);
    }
    // Brief delay.
    for _ in 0..1_000_000u64 {
        core::hint::spin_loop();
    }
    triple_fault()
}

/// Get a summary of power management capabilities.
#[must_use]
pub fn capabilities() -> PowerCapabilities {
    let guard = POWER_INFO.lock();
    match *guard {
        Some(state) => PowerCapabilities {
            acpi_shutdown: state.pm1a_cnt != 0,
            acpi_reboot: state.has_reset_reg,
            kbd_reboot: true, // Always available on x86.
            triple_fault_reboot: true, // Always works.
            pm1a_port: state.pm1a_cnt,
            slp_typ_s5: state.slp_typ_s5,
        },
        None => PowerCapabilities {
            acpi_shutdown: false,
            acpi_reboot: false,
            kbd_reboot: true,
            triple_fault_reboot: true,
            pm1a_port: 0,
            slp_typ_s5: 0,
        },
    }
}

/// Power management capabilities summary.
#[derive(Debug, Clone, Copy)]
pub struct PowerCapabilities {
    /// Whether ACPI S5 shutdown is available.
    pub acpi_shutdown: bool,
    /// Whether ACPI reset register is available.
    pub acpi_reboot: bool,
    /// Whether keyboard controller reboot is available.
    pub kbd_reboot: bool,
    /// Whether triple-fault reboot is available (always true on x86).
    pub triple_fault_reboot: bool,
    /// PM1a control port (0 if not available).
    pub pm1a_port: u16,
    /// S5 sleep type value.
    pub slp_typ_s5: u8,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Write a byte to an I/O port.
///
/// # Safety
///
/// Port must be a valid I/O port for the intended operation.
#[inline]
unsafe fn outb(port: u16, val: u8) {
    // SAFETY: Caller guarantees port is valid for the intended operation.
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") val,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// Read a byte from an I/O port.
///
/// # Safety
///
/// Port must be a valid I/O port.
#[inline]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    // SAFETY: Caller guarantees port is valid.
    unsafe {
        core::arch::asm!(
            "in al, dx",
            out("al") val,
            in("dx") port,
            options(nomem, nostack, preserves_flags),
        );
    }
    val
}

/// Write a 16-bit value to an I/O port.
///
/// # Safety
///
/// Port must be a valid I/O port for 16-bit access.
#[inline]
unsafe fn outw(port: u16, val: u16) {
    // SAFETY: Caller guarantees port is valid for 16-bit I/O.
    unsafe {
        core::arch::asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") val,
            options(nomem, nostack, preserves_flags),
        );
    }
}

/// Small I/O delay to give hardware time to process commands.
#[inline]
fn io_delay() {
    for _ in 0..1_000_000u64 {
        core::hint::spin_loop();
    }
}

/// Trigger a triple fault to force a CPU reset.
///
/// Loads a null IDT (limit=0, base=0) then triggers a software interrupt.
/// With no interrupt handler mapped, this causes a double fault, and with
/// no double-fault handler either, a triple fault → hardware reset.
fn triple_fault() -> ! {
    // Null IDT descriptor: limit=0, base=0.
    #[repr(C, packed)]
    struct NullIdtDescriptor {
        limit: u16,
        base: u64,
    }

    let null_idt = NullIdtDescriptor { limit: 0, base: 0 };

    // SAFETY: Loading a null IDT then triggering int3 causes a guaranteed
    // triple fault → hardware reset.  This is the reset mechanism of last resort.
    unsafe {
        // Load the null IDT.
        core::arch::asm!(
            "lidt [{}]",
            in(reg) &null_idt as *const NullIdtDescriptor,
            options(nostack),
        );
        // Trigger an interrupt — with no IDT, this triple faults.
        core::arch::asm!("int3", options(nostack, nomem));
    }

    // Should never reach here, but just in case...
    loop {
        // SAFETY: hlt stops the CPU until the next interrupt (none will
        // come since interrupts are disabled — this is an infinite halt).
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

/// Infinite halt loop — CPU stops executing (interrupts disabled).
fn halt_loop() -> ! {
    loop {
        // SAFETY: hlt stops the CPU; with interrupts disabled this is
        // the intended "system frozen" terminal state.
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the power management subsystem.
///
/// Tests API availability and capability reporting only — does NOT
/// actually trigger shutdown or reboot!
pub fn self_test() {
    serial_println!("[power] Running self-test...");

    let caps = capabilities();
    serial_println!("[power]   ACPI shutdown: {} (PM1a={:#x}, SLP_TYP_S5={})",
        caps.acpi_shutdown, caps.pm1a_port, caps.slp_typ_s5);
    serial_println!("[power]   ACPI reboot: {}", caps.acpi_reboot);
    serial_println!("[power]   KBD reboot: {}", caps.kbd_reboot);
    serial_println!("[power]   Triple-fault reboot: {}", caps.triple_fault_reboot);

    // Verify at least one reboot method is available.
    assert!(caps.kbd_reboot || caps.acpi_reboot || caps.triple_fault_reboot);
    serial_println!("[power]   At least one reboot method available: OK");

    serial_println!("[power] Self-test PASSED");
}
