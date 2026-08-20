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

// KASAN debug profile: this module is exempt from compiler instrumentation.
// Runs before `mm::kasan::early_init` installs the zero shadow — an
// instrumented access here would read an unmapped shadow byte with no IDT in
// place yet, i.e. a triple fault instead of a diagnosable panic.
// (`sanitize` is nightly-only, so it is gated on the `kasan_instrumented` cfg
// that `scripts/kasan-build.sh` sets; the ordinary build never sees it.)
#![cfg_attr(kasan_instrumented, sanitize(address = "off"))]

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

/// Backing function for the serial print macros.
///
/// Writes `args` to the global [`SERIAL`] port **with interrupts disabled on
/// this CPU** for the whole critical section.
///
/// DEADLOCK FIX (2026-07-15): `SERIAL` is a non-reentrant `spin::Mutex`. If a
/// task holds it mid-write and a timer/IRQ handler on the SAME CPU then tries
/// to print (e.g. the liveness watchdog breadcrumb, a hang dump, or any ISR log
/// line), the handler spins forever on the lock the interrupted task can no
/// longer release — a hard single-CPU wedge with RIP parked in
/// `spin_loop_hint`. This was the long-open non-deterministic "mid-serial-
/// write" boot hang (see known-issues.md). Disabling interrupts for the
/// duration of the critical section makes same-CPU re-entry impossible (the
/// standard console-lock discipline, cf. Linux `spin_lock_irqsave`). Cross-CPU
/// contention is still fine: the other CPU also runs IRQ-off and releases
/// promptly, so a waiter only spins briefly.
///
/// EXCEPTION RE-ENTRANCY FIX (2026-08-12): the interrupt-disabling above is
/// necessary but **not sufficient**, because `cli` does not mask *exceptions*.
/// A `#PF`, `#GP` or `#DF` taken while this CPU is mid-write re-enters `_print`
/// from the fault handler and spins on a lock only the interrupted frame can
/// release. That is not hypothetical: it is the observed shape of
/// `B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT`, where the
/// last thing on the wire was a half-formatted `EXCEPTION: Page Fault (#PF) at`
/// — the fault handler's own print, cut off mid-`{:#x}`. The wedge then
/// destroys the evidence, because the token the operator most needs (the
/// faulting RIP) is precisely the one that never made it out.
///
/// So `_print` records, per CPU, that it is inside the critical section, and a
/// nested call from the same CPU writes through the lock-free emergency port
/// instead of blocking. The nested line interleaves with the outer one, which
/// is ugly but is unambiguously better than silence: a garbled report can be
/// read, a deadlock cannot.
///
/// This is a function (not inline in the macro) so that `?`/`await`/`return`
/// used inside a `serial_println!(...)` format argument is evaluated in the
/// *caller's* context, exactly as with the standard-library `print!` family.
#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments<'_>) {
    use core::fmt::Write;
    use core::sync::atomic::Ordering;

    OUTPUT_COUNT.fetch_add(1, Ordering::Relaxed);
    crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();
        // `current_cpu_index` returns 0 before SMP init and is bounded by
        // MAX_CPUS after it, but index defensively: this is the deadlock-escape
        // path, and it must not become a new panic site.
        let Some(busy) = IN_PRINT.get(cpu) else {
            // No slot for this CPU — take the always-safe route rather than
            // risking the lock.
            let _ = SerialPort::emergency().write_fmt(args);
            return;
        };

        if busy.load(Ordering::Relaxed) {
            // Re-entered from an exception that interrupted this CPU's own
            // print. The lock is held by a frame below us and will not be
            // released until we return, so taking it would wedge forever.
            let _ = SerialPort::emergency().write_fmt(args);
            return;
        }

        // Claim the flag *before* taking the lock, not after. If it were set
        // afterwards there would be a window — however short — in which this
        // CPU holds the lock but a nested exception would not recognise it and
        // would spin. Claiming first is safe in the other direction too: while
        // this CPU merely *waits* for the lock, a nested print still correctly
        // takes the emergency path, because "this CPU is somewhere inside
        // `_print`" is exactly the condition that makes blocking unsafe. The
        // flag is per-CPU, so another CPU's concurrent print is unaffected.
        busy.store(true, Ordering::Relaxed);
        {
            // If the write fails, silently drop the output rather than
            // panicking inside a print path.
            let mut guard = SERIAL.lock();
            let _ = guard.write_fmt(args);
            // Clear before the guard drops, so the flag is never observably
            // stale while another CPU could already hold the lock.
            busy.store(false, Ordering::Relaxed);
            drop(guard);
        }
    });
}

/// Per-CPU "this CPU is inside [`_print`]" flags.
///
/// Only ever read and written by the CPU that owns the slot (under `cli`), so
/// `Relaxed` ordering suffices — there is no cross-CPU handoff to order against,
/// and the lock itself provides the memory barriers that matter.
static IN_PRINT: [core::sync::atomic::AtomicBool; crate::smp::MAX_CPUS] =
    [const { core::sync::atomic::AtomicBool::new(false) }; crate::smp::MAX_CPUS];

/// Monotonic count of completed [`_print`] calls.
///
/// A boot-progress signal for the liveness watchdog. The scheduler's own
/// progress counter (`USEFUL_WORK_TICKS`) only advances for timer ticks that
/// preempt ring-3 code or a CPU with a queued task, so it stays frozen through
/// the long *kernel-side* stretches of boot — ELF loading, page-fault storms
/// serving a starting process, filesystem scans — even though boot is plainly
/// advancing. Serial output is the signal that distinguishes those from a real
/// hang, and it is the same criterion `scripts/boot-test.sh --stall-secs` uses
/// from the outside: a wedged kernel goes silent, a merely-slow one keeps
/// printing. See `sched::liveness_check`.
///
/// Counts *calls*, not bytes — the watchdog only asks "did anything come out
/// during this interval?", so a single relaxed increment per call is enough and
/// costs nothing next to the ~87 µs/char the UART itself takes.
static OUTPUT_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Read the serial output-progress counter (see [`OUTPUT_COUNT`]).
pub fn output_count() -> u64 {
    OUTPUT_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}

/// A `Display` that raises `#BP` while it is being formatted.
///
/// This is the whole point of the self-test below: it reproduces the *exact*
/// shape of the real deadlock, which is not "an interrupt arrives during a
/// print" (that one is already excluded by the `cli`) but "the act of
/// formatting an argument faults". A fault is not maskable, so it re-enters
/// `_print` from a frame that sits on top of the lock holder.
struct FaultWhileFormatting;

impl fmt::Display for FaultWhileFormatting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The `int3` is on purpose; annotate its serial line.
        let _expected = crate::idt::ExpectedBreakpoint::new();
        // SAFETY: `int3` raises #BP, whose handler is installed by `idt::init`,
        // prints one line and returns normally. It clobbers nothing.
        unsafe {
            core::arch::asm!("int3", options(nostack, preserves_flags));
        }
        f.write_str("<survived>")
    }
}

/// Boot self-test: prove that a fault taken *during* a serial write does not
/// deadlock the printer.
///
/// Must run after `idt::init` (it relies on a working `#BP` handler, which
/// itself prints — that is what makes the re-entry happen).
///
/// **Failure mode is a hang, not an assertion.** That is unavoidable and is
/// the point: the bug being guarded against *is* a hang, so the only faithful
/// test is one that would hang. `scripts/boot-test.sh` catches it through its
/// stall detector, and the last line on the wire names this test, so a
/// regression is immediately attributable rather than appearing as a mystery
/// wedge thousands of lines later.
pub fn reentrancy_self_test() {
    crate::serial_println!("[serial] Running print re-entrancy self-test...");
    crate::serial_println!(
        "[serial]   (the #BP line below is emitted from inside this line's own \
         formatting, through the lock-free emergency port — the interleaving is \
         expected)"
    );

    let before = output_count();
    crate::serial_println!("[serial]   fault during formatting: {FaultWhileFormatting}");

    // Reaching here at all is the primary result. The counter check adds a
    // little: it confirms the nested print really happened rather than the
    // handler having been silently skipped, which would make the test vacuous.
    let after = output_count();
    assert!(
        after >= before.saturating_add(2),
        "serial: the nested print never ran — re-entrancy self-test is vacuous"
    );

    crate::serial_println!("[serial]   nested print completed without deadlock: OK");
    crate::serial_println!("[serial] Print re-entrancy self-test PASSED");
}

/// Print to the serial console (COM1).
///
/// Usage is identical to the standard `print!` macro.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {{
        $crate::serial::_print(::core::format_args!($($arg)*));
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
