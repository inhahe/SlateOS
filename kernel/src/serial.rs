//! Serial port (UART 16550) driver for debug output.
//!
//! The serial console is the primary debug output channel during early
//! boot and kernel development.  Output goes to COM1 (0x3F8), which
//! QEMU maps to the host terminal when started with `-serial stdio`.
//!
//! This module provides:
//! - A global `SERIAL` writer protected by a spinlock
//! - `serial_print!` and `serial_println!` macros
//! - Initialization of the UART hardware

use core::fmt;
use spin::Mutex;

use crate::port;

/// Standard I/O port addresses for COM1.
const COM1_BASE: u16 = 0x3F8;

/// UART register offsets from the base port.
mod regs {
    /// Transmit Holding Register (write) / Receive Buffer Register (read).
    /// When DLAB=1: Divisor Latch Low byte.
    pub const THR: u16 = 0;
    /// Interrupt Enable Register.
    /// When DLAB=1: Divisor Latch High byte.
    pub const IER: u16 = 1;
    /// FIFO Control Register (write) / Interrupt Identification (read).
    pub const FCR: u16 = 2;
    /// Line Control Register.
    pub const LCR: u16 = 3;
    /// Modem Control Register.
    pub const MCR: u16 = 4;
    /// Line Status Register.
    pub const LSR: u16 = 5;
}

/// A serial port (UART 16550) attached to a specific I/O base address.
pub struct SerialPort {
    base: u16,
}

impl SerialPort {
    /// Create a new serial port handle for the given base address.
    ///
    /// Does NOT initialize the hardware — call [`init`](Self::init) first.
    #[must_use]
    pub const fn new(base: u16) -> Self {
        Self { base }
    }

    /// Create a throwaway COM1 handle for **lock-free emergency output**.
    ///
    /// Returns a bare [`SerialPort`] for COM1 that is *not* the Mutex-protected
    /// global [`SERIAL`]. Writing through it polls the UART LSR and pushes bytes
    /// to the THR directly, taking **no lock**, so it can never deadlock on the
    /// global serial spinlock — the essential property for reporting from an
    /// NMI / panic / hard-lockup context where the wedged code may be holding
    /// that lock. Assumes the UART was already initialized by [`init`] (true
    /// after early boot). See the [`emergency_println!`] macro.
    #[must_use]
    pub const fn emergency() -> Self {
        Self::new(COM1_BASE)
    }

    /// Initialize the UART hardware.
    ///
    /// Configures: 115200 baud, 8N1, FIFO enabled, no interrupts.
    ///
    /// # Safety
    ///
    /// Must only be called once per port, and the port address must be
    /// valid hardware.
    pub unsafe fn init(&self) {
        // SAFETY: All port writes target COM1 registers which are
        // standard PC hardware.  We write them in the canonical UART
        // initialization sequence.
        unsafe {
            // Disable all interrupts.
            port::outb(self.base + regs::IER, 0x00);

            // Enable DLAB (Divisor Latch Access Bit) to set baud rate.
            port::outb(self.base + regs::LCR, 0x80);

            // Set divisor to 1 (115200 baud).
            // Divisor Latch Low byte.
            port::outb(self.base + regs::THR, 0x01);
            // Divisor Latch High byte.
            port::outb(self.base + regs::IER, 0x00);

            // 8 bits, no parity, one stop bit (8N1).  Clears DLAB.
            port::outb(self.base + regs::LCR, 0x03);

            // Enable FIFO, clear TX/RX queues, 14-byte threshold.
            port::outb(self.base + regs::FCR, 0xC7);

            // IRQs enabled, RTS/DSR set.
            port::outb(self.base + regs::MCR, 0x0B);
        }
    }

    /// Check if the transmit holding register is empty (ready to send).
    fn is_transmit_empty(&self) -> bool {
        // SAFETY: Reading the LSR of a valid COM port is safe.
        let lsr = unsafe { port::inb(self.base + regs::LSR) };
        lsr & 0x20 != 0
    }

    /// Write a single byte, blocking until the UART is ready.
    pub fn write_byte(&self, byte: u8) {
        // Busy-wait for the transmit buffer to be empty.
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }
        // SAFETY: Writing to the THR of a valid, initialized COM port
        // is the standard way to transmit a byte.
        unsafe {
            port::outb(self.base + regs::THR, byte);
        }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            // Convert \n to \r\n for terminal compatibility.
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

/// Global serial port writer, protected by a spinlock.
///
/// Initialized lazily on first use by [`init`].
pub static SERIAL: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1_BASE));

/// Initialize the global serial port.
///
/// Must be called exactly once, early in the boot process, before any
/// serial output macros are used.
///
/// # Safety
///
/// The COM1 hardware must be present (it always is on standard PC
/// hardware and in QEMU).
pub unsafe fn init() {
    // SAFETY: COM1 is present on standard PC hardware and QEMU.
    // We call init exactly once during early boot.
    unsafe {
        SERIAL.lock().init();
    }
}

/// Print to the serial console (COM1).
///
/// Usage is identical to the standard `print!` macro.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {{
        #[allow(unused_imports)]
        use core::fmt::Write;
        // Lock the serial port and write.  If the lock is poisoned
        // (shouldn't happen with spinlocks), we silently drop the output
        // rather than panicking inside a print macro.
        let mut serial = $crate::serial::SERIAL.lock();
        let _ = write!(serial, $($arg)*);
    }};
}

/// Print to the serial console (COM1) with a trailing newline.
///
/// Usage is identical to the standard `println!` macro.
#[macro_export]
macro_rules! serial_println {
    ()            => { $crate::serial_print!("\n") };
    ($($arg:tt)*) => { $crate::serial_print!("{}\n", format_args!($($arg)*)) };
}

/// Lock-free "emergency" serial output for NMI / panic / hard-lockup contexts.
///
/// Identical usage to [`serial_print!`], but writes through a throwaway COM1
/// handle ([`crate::serial::SerialPort::emergency`]) instead of the global
/// Mutex-protected [`SERIAL`]. It therefore acquires **no lock** and can never
/// deadlock even if the wedged/interrupted code is holding the serial spinlock
/// — the exact scenario a hard-lockup watchdog must survive to report the wedge.
///
/// On a uniprocessor the interrupted context is frozen while an NMI handler
/// runs, so bytes cannot interleave; on SMP a concurrent normal writer could
/// garble output, an acceptable tradeoff for guaranteed emergency visibility.
#[macro_export]
macro_rules! emergency_print {
    ($($arg:tt)*) => {{
        #[allow(unused_imports)]
        use core::fmt::Write;
        let mut serial = $crate::serial::SerialPort::emergency();
        let _ = write!(serial, $($arg)*);
    }};
}

/// Lock-free "emergency" serial output with a trailing newline.
///
/// See [`emergency_print!`]. Use this for the single most important line from a
/// wedge — the watchdog "FIRED … rip=…" marker — so it always escapes even when
/// the global serial lock is held.
#[macro_export]
macro_rules! emergency_println {
    ()            => { $crate::emergency_print!("\n") };
    ($($arg:tt)*) => { $crate::emergency_print!("{}\n", format_args!($($arg)*)) };
}
