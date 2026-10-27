//! Serial port (UART 16550) driver for debug output.
//!
//! The serial console is the primary debug output channel during early
//! boot and kernel development.  Output goes to COM1 (0x3F8), which
//! QEMU maps to the host terminal when started with `-serial stdio`.
//! With `log.pci_serial=N` on the command line it goes to the Nth
//! 16550-compatible serial port on the PCI bus as well -- a serial card, for
//! a machine with no COM1 -- found and set up before the first line.
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
        // SAFETY: All port writes target the registers of the 16550 at
        // `self.base`, which the caller vouches for.  We write them in the
        // canonical UART initialization sequence.
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

            // DTR, RTS, OUT2.
            port::outb(self.base.wrapping_add(regs::MCR), MCR_NORMAL);
        }
    }

    /// Check if the transmit holding register is empty (ready to send).
    fn is_transmit_empty(&self) -> bool {
        // SAFETY: Reading the LSR of a valid COM port is safe.
        let lsr = unsafe { port::inb(self.base + regs::LSR) };
        lsr & LSR_THR_EMPTY != 0
    }

    /// Write a single byte once the transmitter has room, waiting at most
    /// `polls` line-status reads for it: whether it was written.
    fn try_write_byte(&self, byte: u8, polls: u32) -> bool {
        if !(0..polls).any(|_| {
            let room = self.is_transmit_empty();
            if !room {
                core::hint::spin_loop();
            }
            room
        }) {
            return false;
        }
        // SAFETY: the THR of the port `self` was made for, its transmitter
        // with room.
        unsafe { port::outb(self.base.wrapping_add(regs::THR), byte) };
        true
    }

    /// Write a single byte, blocking until the UART is ready. A byte for
    /// COM1, the log's port, also goes to the PCI serial port the log is
    /// copied to, if there is one (`log.pci_serial=`).
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
        if self.base == COM1_BASE {
            mirror_byte(byte);
        }
    }
}

/// The I/O port of the PCI serial port the log is copied to
/// (`log.pci_serial=N`), or 0 for none. Set once, by [`init`], after the
/// port has answered [`loopback_answers`]; cleared for good by
/// [`mirror_byte`] if the port stops taking bytes.
static MIRROR: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

/// Set when [`mirror_byte`] gives up on the port, until [`_print`] has said
/// so on the ports that are left.
static MIRROR_LOST: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Line status: a received byte is waiting in the receive buffer.
const LSR_DATA_READY: u8 = 0x01;
/// Line status: the transmitter has room for a byte.
const LSR_THR_EMPTY: u8 = 0x20;
/// Modem control: loopback, in which what is sent is received at once and
/// the wire is held idle.
const MCR_LOOPBACK: u8 = 0x10;
/// Modem control as [`SerialPort::init`] leaves it: DTR, RTS, OUT2.
const MCR_NORMAL: u8 = 0x0B;

/// How many line-status reads [`mirror_byte`] and [`loopback_answers`] wait
/// for a byte. One byte takes 87 µs at 115200 baud, and a port read on a
/// PCIe card takes upward of half a microsecond, so this is a quarter-second
/// at the least: a port that has not moved in that long is not working.
const PORT_POLLS_MAX: u32 = 500_000;

/// Write `byte` to the PCI serial port in [`MIRROR`], if there is one.
///
/// Its wait is bounded, unlike COM1's: the card is an add-in, and a port
/// whose transmitter never empties would otherwise hang every print in the
/// kernel, the panic's among them. (One that has gone from the bus reads as
/// all ones, which says the transmitter is empty: its bytes are lost, but
/// nothing waits.) Past the bound the port is dropped and [`MIRROR_LOST`] set.
fn mirror_byte(byte: u8) {
    use core::sync::atomic::Ordering;

    let base = MIRROR.load(Ordering::Relaxed);
    if base != 0 && !SerialPort::new(base).try_write_byte(byte, PORT_POLLS_MAX) {
        MIRROR.store(0, Ordering::Relaxed);
        MIRROR_LOST.store(true, Ordering::Relaxed);
    }
}

/// Whether a 16550 answers at `base`, which [`SerialPort::init`] has set up:
/// in loopback a byte sent is received at once, so one comes back. Nothing
/// reaches the wire meanwhile, so whatever the port is plugged into sees
/// nothing. Leaves the port out of loopback. `Err` holds what the line
/// status or the receive buffer read instead.
///
/// # Safety
///
/// `base` must be a 16550's first I/O port, with its decoding on.
unsafe fn loopback_answers(base: u16) -> Result<(), (u8, u8)> {
    const PROBE: u8 = 0xA5;
    let reg = |offset: u16| base.wrapping_add(offset);
    // SAFETY: the caller's; each access is one of a 16550's registers.
    unsafe {
        port::outb(reg(regs::MCR), MCR_NORMAL | MCR_LOOPBACK);
        // Empty what the receiver holds, so the byte read is the probe.
        let mut stale = 0u32;
        while port::inb(reg(regs::LSR)) & LSR_DATA_READY != 0 && stale < 64 {
            let _ = port::inb(reg(regs::THR));
            stale = stale.saturating_add(1);
        }
        port::outb(reg(regs::THR), PROBE);
        let mut lsr = port::inb(reg(regs::LSR));
        let mut polls = 0u32;
        while lsr & LSR_DATA_READY == 0 && polls < PORT_POLLS_MAX {
            core::hint::spin_loop();
            lsr = port::inb(reg(regs::LSR));
            polls = polls.saturating_add(1);
        }
        let got = port::inb(reg(regs::THR));
        port::outb(reg(regs::MCR), MCR_NORMAL);
        if lsr & LSR_DATA_READY != 0 && got == PROBE {
            Ok(())
        } else {
            Err((lsr, got))
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
    attach_pci_port();
}

/// Copy the log to a PCI serial port too, if the command line asks for one
/// (`log.pci_serial=N`, the Nth in the PCI scan's order, from 1), and say
/// what became of it. A desktop board often has no COM1 at all, and a
/// serial card is then the one place the log survives a hang or a dead
/// screen whole. Not done unasked: whatever is plugged into a serial port,
/// a UPS or a modem, would be sent the log.
///
/// Runs inside [`init`], before the kernel prints anything else, so the
/// port has the log from its first line.
fn attach_pci_port() {
    use crate::pci::UartSearch;

    let nth = match crate::boot::kernel_cmdline_bytes().and_then(pci_serial_choice) {
        None => return,
        Some(Ok(nth)) => nth,
        Some(Err(value)) => {
            crate::serial_println!(
                "[serial] log.pci_serial={} is not a port number (1 is the first PCI serial port); the log stays on COM1",
                value.escape_ascii()
            );
            return;
        }
    };
    let uart = match crate::pci::early_uart(nth) {
        UartSearch::Found(uart) => uart,
        UartSearch::TooFew(found) => {
            crate::serial_println!(
                "[serial] log.pci_serial={}: the PCI bus has {} 16550-compatible serial port(s)",
                nth,
                found
            );
            return;
        }
        UartSearch::NoIoPorts(at, bar) => {
            crate::serial_println!(
                "[serial] log.pci_serial={}: the serial port at {:02x}:{:02x}.{} has no I/O ports to drive (BAR0 {:#010x})",
                nth,
                at.bus,
                at.device,
                at.function,
                bar
            );
            return;
        }
        UartSearch::BridgeClosed(bridge, port) => {
            crate::serial_println!(
                "[serial] log.pci_serial={}: the bridge at {:02x}:{:02x}.{} does not pass the port's I/O {:#x} through (outside its I/O window); nothing switched on",
                nth,
                bridge.bus,
                bridge.device,
                bridge.function,
                port
            );
            return;
        }
        UartSearch::TooDeep(at) => {
            crate::serial_println!(
                "[serial] log.pci_serial={}: the serial port at {:02x}:{:02x}.{} is behind more bridges than are looked through; nothing switched on",
                nth,
                at.bus,
                at.device,
                at.function
            );
            return;
        }
    };
    let port = SerialPort::new(uart.port);
    // SAFETY: `early_uart` found a 16550-compatible function whose BAR0 is
    // these I/O ports, and switched its decoding on; this is the one place
    // that sets it up, and only the BSP is running.
    let answer = unsafe {
        port.init();
        loopback_answers(uart.port)
    };
    let at = uart.address;
    match answer {
        Ok(()) => {
            MIRROR.store(uart.port, core::sync::atomic::Ordering::Relaxed);
            crate::serial_println!(
                "[serial] log.pci_serial={}: the log goes to the serial port at {:02x}:{:02x}.{} (I/O {:#x}) as well, 115200 8N1; I/O decoding switched on in {} command register(s)",
                nth,
                at.bus,
                at.device,
                at.function,
                uart.port,
                uart.opened
            );
        }
        Err((lsr, got)) => {
            // Not the port it looked like: what was switched on to reach it
            // is switched off again.
            crate::pci::close_io_decode(&uart);
            crate::serial_println!(
                "[serial] log.pci_serial={}: the serial port at {:02x}:{:02x}.{} (I/O {:#x}) does not answer: in loopback, line status {:#04x}, byte {:#04x}; I/O decoding switched back off",
                nth,
                at.bus,
                at.device,
                at.function,
                uart.port,
                lsr,
                got
            );
        }
    }
}

/// What a command line says of `log.pci_serial=`: see [`pci_serial_choice`].
type PciSerialChoice<'a> = Option<Result<usize, &'a [u8]>>;

/// Which PCI serial port a command line asks the log to go to:
/// `log.pci_serial=N`, the last such word winning. `None` when it asks for
/// none (no such word, or `=0`); `Err` holds a value that is not a number.
fn pci_serial_choice(line: &[u8]) -> PciSerialChoice<'_> {
    let value = line
        .split(u8::is_ascii_whitespace)
        .filter_map(|word| word.strip_prefix(b"log.pci_serial="))
        .next_back()?;
    let parsed = core::str::from_utf8(value)
        .ok()
        .and_then(|text| text.parse::<usize>().ok());
    match parsed {
        Some(0) => None,
        Some(nth) => Some(Ok(nth)),
        None => Some(Err(value)),
    }
}

/// [`pci_serial_choice`] on made-up command lines.
pub fn self_test() -> crate::error::KernelResult<()> {
    let cases: [(&[u8], PciSerialChoice<'_>); 8] = [
        (b"", None),
        (b"log.screen=1 blk.writable=all", None),
        (b"log.pci_serial=1", Some(Ok(1))),
        (b"quiet log.pci_serial=2 log.screen=1", Some(Ok(2))),
        (b"log.pci_serial=2 log.pci_serial=1", Some(Ok(1))),
        (b"log.pci_serial=0", None),
        (b"log.pci_serial=yes", Some(Err(b"yes"))),
        (b"xlog.pci_serial=1 log.pci_serial", None),
    ];
    for (line, want) in cases {
        let got = pci_serial_choice(line);
        if got != want {
            crate::serial_println!(
                "[serial]   FAIL: log.pci_serial from \"{}\": {:?}, want {:?}",
                line.escape_ascii(),
                got,
                want
            );
            return Err(crate::error::KernelError::InternalError);
        }
    }
    crate::serial_println!("[serial]   log.pci_serial= parsing: OK");
    self_test_pci_port()
}

/// The first PCI serial port, found as `log.pci_serial=1` finds one --
/// through the bridges, their windows checked -- and answering in loopback.
/// The boot test's QEMU has one behind a root port; a machine with none says
/// so. A port the log already goes to is left as it is.
fn self_test_pci_port() -> crate::error::KernelResult<()> {
    use crate::pci::UartSearch;

    let uart = match crate::pci::early_uart(1) {
        UartSearch::Found(uart) => uart,
        UartSearch::TooFew(0) => {
            crate::serial_println!(
                "[serial]   no PCI serial port here: the search is not exercised"
            );
            return Ok(());
        }
        other => {
            crate::serial_println!("[serial]   FAIL: the first PCI serial port: {:?}", other);
            return Err(crate::error::KernelError::InternalError);
        }
    };
    if uart.port != MIRROR.load(core::sync::atomic::Ordering::Relaxed) {
        let port = SerialPort::new(uart.port);
        // SAFETY: `early_uart` found a 16550-compatible function whose BAR0
        // is these I/O ports, its decoding on; nothing else drives it (the log
        // goes to another port, or none), and loopback sends nothing out.
        if let Err((lsr, got)) = unsafe {
            port.init();
            loopback_answers(uart.port)
        } {
            crate::serial_println!(
                "[serial]   FAIL: the PCI serial port at I/O {:#x} does not answer: line status {:#04x}, byte {:#04x}",
                uart.port,
                lsr,
                got
            );
            return Err(crate::error::KernelError::InternalError);
        }
    }
    let at = uart.address;
    crate::serial_println!(
        "[serial]   the PCI serial port at {:02x}:{:02x}.{} (I/O {:#x}) found and answering: OK",
        at.bus,
        at.device,
        at.function,
        uart.port
    );
    Ok(())
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
/// used inside a `crate::serial_println!(...)` format argument is evaluated in the
/// *caller's* context, exactly as with the standard-library `print!` family.
#[doc(hidden)]
pub fn _print(args: core::fmt::Arguments<'_>) {
    use core::fmt::Write;

    // If the write fails, silently drop the output rather than panicking
    // inside a print path.
    with_serial(|port| {
        if crate::screenlog::active() {
            let _ = ScreenTee(port).write_fmt(args);
        } else {
            let _ = port.write_fmt(args);
        }
    });
    report_lost_mirror();
}

/// Say on COM1 and the screen, once, that the PCI serial port was dropped
/// ([`mirror_byte`]): otherwise its log would just stop, which reads as a hang.
fn report_lost_mirror() {
    if MIRROR_LOST.swap(false, core::sync::atomic::Ordering::Relaxed) {
        crate::serial_println!(
            "[serial] the PCI serial port stopped taking bytes; the log no longer goes to it"
        );
    }
}

/// The serial port, with each piece written also fed to the kernel's log
/// pane on the screen (`screenlog`, `log.screen=1`).
struct ScreenTee<'a>(&'a mut SerialPort);

impl fmt::Write for ScreenTee<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::screenlog::feed(s.as_bytes());
        self.0.write_str(s)
    }
}

/// Write raw bytes to the serial port, with no UTF-8 interpretation.
///
/// The byte-clean counterpart of [`_print`]. A filename may contain any byte
/// but `/` and NUL, so the path that mirrors shell output to the serial log
/// cannot require `&str` without either refusing such a name or rewriting it
/// as U+FFFD — and a log that silently disagrees with the filesystem is worse
/// than no log. Shares [`with_serial`]'s reentrancy protocol rather than
/// reimplementing it; see that function for why that matters.
///
/// `\n` is expanded to `\r\n` to match [`SerialPort`]'s `fmt::Write`, so the
/// two paths cannot produce differently-terminated lines in one log. That is
/// the *only* transformation applied — every other byte goes out verbatim.
pub fn _print_bytes(bytes: &[u8]) {
    with_serial(|port| {
        crate::screenlog::feed(bytes);
        for &byte in bytes {
            if byte == b'\n' {
                port.write_byte(b'\r');
            }
            port.write_byte(byte);
        }
    });
    report_lost_mirror();
}

/// Run `emit` against the serial port under the deadlock-escape protocol.
///
/// Extracted so that [`_print`] and [`_print_bytes`] share one copy of this
/// logic. It is subtle enough that two copies would be a liability: the
/// ordering of the flag store against the lock acquisition below is load-
/// bearing, and a second copy that drifted from it would deadlock only under
/// a nested exception — the case that is hardest to reproduce and most
/// urgent to survive. `reentrancy_self_test` covers this path.
fn with_serial(emit: impl FnOnce(&mut SerialPort)) {
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
            emit(&mut SerialPort::emergency());
            return;
        };

        if busy.load(Ordering::Relaxed) {
            // Re-entered from an exception that interrupted this CPU's own
            // print. The lock is held by a frame below us and will not be
            // released until we return, so taking it would wedge forever.
            emit(&mut SerialPort::emergency());
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
            let mut guard = SERIAL.lock();
            emit(&mut guard);
            // Release the lock *first*, then lower the flag — the flag must
            // outlive the guard, never the other way round. The condition it
            // encodes is "this CPU is inside the critical section", and this
            // CPU is still inside it until `guard` is gone; clearing first
            // would leave a window holding `SERIAL` with the escape hatch
            // down, which is precisely the state a nested exception must
            // never find (it would take the normal path and spin on a lock
            // only the frame below it can release).
            //
            // An earlier version cleared first, reasoning that the flag
            // should not look stale "while another CPU could already hold
            // the lock". No other CPU ever reads this slot: `IN_PRINT` is
            // indexed only by `current_cpu_index()`, as its own doc comment
            // below states. The concern was unreachable and the window it
            // bought was real.
            drop(guard);
            busy.store(false, Ordering::Relaxed);
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
pub fn reentrancy_self_test() -> crate::error::KernelResult<()> {
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
    Ok(())
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
