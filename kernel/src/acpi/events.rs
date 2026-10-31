//! ACPI fixed events: the power button.
//!
//! The firmware hands the chipset over in one of two modes, which PM1
//! control's `SCI_EN` bit says. In legacy mode the firmware itself acts on
//! the power button -- on most PCs by powering off at once, a power cut as
//! far as the OS knows. In ACPI mode the press is the OS's: it sets the
//! button's status bit in PM1 status and, the button's event enabled,
//! raises the SCI, the interrupt ACPI's fixed and general-purpose events
//! share.
//!
//! [`init`] says which mode the firmware handed over and, where the FADT
//! describes a fixed power button, makes it the OS's: everything that can
//! fail is settled first -- the SCI's pin and its sense, the work queue --
//! then ACPI mode is taken where the firmware kept it, as ACPICA's
//! `acpi_enable` takes it, every general-purpose event disabled (SlateOS
//! runs no AML that could answer one, and a level-triggered SCI left
//! asserted by one would never stop), every fixed event's status cleared,
//! only the button's event enabled, and the SCI unmasked. Should the last
//! step fail, ACPI mode taken is given back, so that the firmware has the
//! button again.
//!
//! The SCI's handler ([`handle_sci`], from `ioapic::handle_device_irq`)
//! clears the button's status and hands the press to a kernel worker
//! ([`crate::workqueue`]), which does what the power policy says
//! ([`crate::fs::power::handle_power_button`]): to shut down is
//! [`crate::initproc::shutdown`] -- the kernel's service table marked
//! stopped, filesystems synced, logs flushed; no running program is asked
//! to stop, which needs a userspace init that handles shutdown -- then
//! [`crate::power::shutdown`], the S5 power-off (design-decisions 1597).
//!
//! Under the boot test (`acpi.pwrbtn=hmp` on the command line) every press
//! is the harness's: recorded, never acted on ([`self_test`]). A
//! hardware-reduced platform, or one whose FADT makes the button a device
//! the AML describes, is said and left to the firmware.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};

use crate::acpi::fadt::PowerInfo;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use crate::sync::Mutex;

/// PM1 status and enable: the power button's bit (`PWRBTN_STS`,
/// `PWRBTN_EN`).
const PWRBTN: u16 = 1 << 8;
/// PM1 status: every fixed event's status bit, which writing 1 clears --
/// timer, bus master, global lock, power and sleep buttons, RTC, PCIe wake,
/// wake (ACPI 6.5 table 4.13).
const PM1_ALL_STATUS: u16 = 0xC731;
/// PM1 control: `SCI_EN`, set while the OS has ACPI mode.
const SCI_EN: u16 = 1;
/// How long the firmware has to change `SCI_EN` once asked.
const MODE_WAIT_MS: u64 = 3000;
/// How long [`self_test`] waits for the harness's press.
const PRESS_WAIT_MS: u64 = 20_000;
/// The kernel command line's word that says a harness presses the button
/// (`scripts/boot-test.sh`'s `pwrbtn_responder`), and every press is its.
const HARNESS_WORD: &str = "acpi.pwrbtn";

/// The FADT's power-management registers, from `acpi::init` ([`note_fadt`]).
static FADT: Mutex<Option<PowerInfo>> = Mutex::new(None);
/// The PM1 event blocks and their length, for [`handle_sci`], which takes
/// no lock of its own: 0 until [`init`] has enabled the button.
static PM1A_EVT: AtomicU16 = AtomicU16::new(0);
static PM1B_EVT: AtomicU16 = AtomicU16::new(0);
static PM1_EVT_LEN: AtomicU8 = AtomicU8::new(0);
/// The GPE blocks and their lengths, for [`handle_sci`]: 0 until [`init`].
static GPE0_BLK: AtomicU16 = AtomicU16::new(0);
static GPE0_LEN: AtomicU8 = AtomicU8::new(0);
static GPE1_BLK: AtomicU16 = AtomicU16::new(0);
static GPE1_LEN: AtomicU8 = AtomicU8::new(0);
/// General-purpose events found firing and turned off by [`handle_sci`]:
/// enabled again, by the firmware, after [`init`] disabled them all.
static GPES_QUIETED: AtomicU64 = AtomicU64::new(0);
/// The SCI's I/O APIC pin, [`u32::MAX`] while it is not routed.
static SCI_PIN: AtomicU32 = AtomicU32::new(u32::MAX);
/// Presses handled since boot.
static PRESSES: AtomicU64 = AtomicU64::new(0);
/// Whether a harness presses the button (`acpi.pwrbtn=hmp`): every press is
/// recorded and none acted on. Read once, by [`init`].
static HARNESS: AtomicBool = AtomicBool::new(false);

/// The FADT's power-management registers, kept for [`init`]: `acpi::init`'s,
/// once it has parsed the FADT.
pub fn note_fadt(info: &PowerInfo) {
    *FADT.lock() = Some(*info);
}

/// The I/O APIC pin the SCI is routed to, `None` while it is not: what
/// `ioapic::handle_device_irq` hands to [`handle_sci`].
#[must_use]
pub fn sci_pin() -> Option<u32> {
    let pin = SCI_PIN.load(Ordering::Acquire);
    (pin != u32::MAX).then_some(pin)
}

/// How many presses of the power button have been handled since boot.
#[must_use]
pub fn presses() -> u64 {
    PRESSES.load(Ordering::Acquire)
}

/// The power button made the OS's, as the module doc says: what can fail
/// settled first, then ACPI mode, the events, and the SCI. Said either way.
/// After the I/O APIC, interrupt remapping and the work queue are up.
pub fn init() {
    HARNESS.store(
        crate::fs::kernparam::get(HARNESS_WORD).is_some_and(|v| v == "hmp"),
        Ordering::Release,
    );
    let Some(f) = *FADT.lock() else {
        serial_println!("[acpi] power button: no FADT, so not handled");
        return;
    };
    // SAFETY: PM1a_CNT is the FADT's own port for the chipset's PM1
    // control register; reading it changes nothing.
    let sci_en = f.pm1a_cnt_blk != 0 && unsafe { crate::port::inw(f.pm1a_cnt_blk) } & SCI_EN != 0;
    serial_println!(
        "[acpi] ACPI mode at handoff: {}",
        if sci_en {
            "on (SCI_EN set by the firmware)"
        } else if f.smi_command == 0 {
            "the OS's from the start (no SMI command port)"
        } else {
            "off -- the firmware has the power button"
        }
    );
    if let Some(why) = not_handled(&f) {
        serial_println!("[acpi] power button: {why}; left to the firmware");
        return;
    }
    let Some((pin, level, active_low)) = sci_route(f.sci_irq) else {
        return;
    };
    if !crate::workqueue::is_running() {
        serial_println!(
            "[acpi] power button: the work queue, which acts on a press, is not running; left to the firmware"
        );
        return;
    }
    let taken = if sci_en || f.smi_command == 0 {
        false
    } else if set_acpi_mode(&f, true) {
        serial_println!(
            "[acpi] ACPI mode taken (ACPI_ENABLE {:#x} written to SMI_CMD {:#x})",
            f.acpi_enable,
            f.smi_command
        );
        true
    } else {
        serial_println!(
            "[acpi] power button: ACPI mode still off {MODE_WAIT_MS} ms after ACPI_ENABLE was written; left to the firmware"
        );
        return;
    };
    // SAFETY: the FADT's GPE and PM1 event blocks, in ACPI mode: their
    // status bits clear when written 1, their enable bits are the OS's.
    unsafe {
        disable_gpes(f.gpe0_blk, f.gpe0_blk_len);
        disable_gpes(f.gpe1_blk, f.gpe1_blk_len);
        for evt in [f.pm1a_evt_blk, f.pm1b_evt_blk] {
            if evt != 0 {
                crate::port::outw(evt, PM1_ALL_STATUS);
                crate::port::outw(enable_port(evt, f.pm1_evt_len), PWRBTN);
            }
        }
    }
    PM1A_EVT.store(f.pm1a_evt_blk, Ordering::Release);
    PM1B_EVT.store(f.pm1b_evt_blk, Ordering::Release);
    PM1_EVT_LEN.store(f.pm1_evt_len, Ordering::Release);
    GPE0_BLK.store(f.gpe0_blk, Ordering::Release);
    GPE0_LEN.store(f.gpe0_blk_len, Ordering::Release);
    GPE1_BLK.store(f.gpe1_blk, Ordering::Release);
    GPE1_LEN.store(f.gpe1_blk_len, Ordering::Release);
    // SAFETY: the SCI's own pin, still masked: its trigger mode and
    // polarity, as `sci_route` worked them out.
    unsafe { crate::ioapic::set_trigger_polarity(pin, level, active_low) };
    SCI_PIN.store(u32::from(pin), Ordering::Release);
    // SAFETY: the pin's handler is in place (`sci_pin`, read by
    // `ioapic::handle_device_irq`), and every status that could hold it
    // asserted is cleared.
    if unsafe { crate::ioapic::unmask_irq(pin) } {
        serial_println!(
            "[acpi] power button: handled -- the SCI (IRQ {}, I/O APIC pin {pin}, {}, {}) taken, the button's event alone enabled",
            f.sci_irq,
            if level { "level" } else { "edge" },
            if active_low {
                "active-low"
            } else {
                "active-high"
            }
        );
        return;
    }
    SCI_PIN.store(u32::MAX, Ordering::Release);
    PM1A_EVT.store(0, Ordering::Release);
    PM1B_EVT.store(0, Ordering::Release);
    // SAFETY: as above; the button's event disabled again.
    unsafe {
        for evt in [f.pm1a_evt_blk, f.pm1b_evt_blk] {
            if evt != 0 {
                crate::port::outw(enable_port(evt, f.pm1_evt_len), 0);
            }
        }
    }
    let given_back = taken && set_acpi_mode(&f, false);
    serial_println!(
        "[acpi] power button: the SCI's pin {pin} could not be routed; {}",
        if given_back {
            "ACPI mode given back, the firmware has the button again"
        } else if taken {
            "ACPI mode could not be given back: no one has the button"
        } else {
            "the button is no one's"
        }
    );
}

/// Why the FADT's power button cannot be the kernel's, `None` when it can:
/// a hardware-reduced platform (no fixed hardware), a button the FADT makes
/// a control-method device (flag `PWR_BUTTON`), or no PM1 blocks.
fn not_handled(f: &PowerInfo) -> Option<&'static str> {
    if f.hw_reduced {
        Some("hardware-reduced ACPI, which has no fixed power button")
    } else if !f.fixed_power_button {
        Some("the FADT makes it a control-method device, which needs AML")
    } else if f.pm1a_evt_blk == 0 || f.pm1a_cnt_blk == 0 || f.pm1_evt_len < 4 {
        Some("the FADT names no PM1 event and control blocks")
    } else {
        None
    }
}

/// Take ACPI mode (`on`) or give it back: the FADT's `ACPI_ENABLE` or
/// `ACPI_DISABLE` written to `SMI_CMD`, and up to [`MODE_WAIT_MS`] waited
/// for `SCI_EN` to follow. Whether it did.
fn set_acpi_mode(f: &PowerInfo, on: bool) -> bool {
    let value = if on { f.acpi_enable } else { f.acpi_disable };
    // SAFETY: SMI_CMD and the value are the FADT's own for this request;
    // the write raises an SMI the firmware answers by changing SCI_EN.
    unsafe { crate::port::outb(f.smi_command, value) };
    let until = crate::hrtimer::now_ns().saturating_add(MODE_WAIT_MS * 1_000_000);
    loop {
        // SAFETY: as in `init`, reading PM1a_CNT changes nothing.
        if (unsafe { crate::port::inw(f.pm1a_cnt_blk) } & SCI_EN != 0) == on {
            return true;
        }
        if crate::hrtimer::now_ns() >= until {
            return false;
        }
        crate::sched::sleep_ms(10);
    }
}

/// The enable register of the PM1 event block at `evt`: its second half.
fn enable_port(evt: u16, len: u8) -> u16 {
    evt.saturating_add(u16::from(len / 2))
}

/// Every event of the GPE block at `blk`, `len` bytes long -- status bytes,
/// then as many enable bytes -- disabled and its status cleared. Nothing
/// for a block the FADT does not name.
///
/// # Safety
///
/// `blk` and `len` are the FADT's for a GPE block, in ACPI mode.
unsafe fn disable_gpes(blk: u16, len: u8) {
    if blk == 0 || len < 2 {
        return;
    }
    let half = u16::from(len / 2);
    for i in 0..half {
        // SAFETY: `blk + half + i` is an enable byte and `blk + i` a status
        // byte of the block (the caller's contract).
        unsafe {
            crate::port::outb(blk.saturating_add(half).saturating_add(i), 0);
            crate::port::outb(blk.saturating_add(i), 0xFF);
        }
    }
}

/// Every event of the GPE block at `blk` (`len` bytes) both enabled and
/// firing turned off and its status cleared: one the firmware enabled again
/// after [`init`] disabled them all, as its SMM may. Counted
/// ([`GPES_QUIETED`]); nothing for a block of none.
///
/// # Safety
///
/// As [`disable_gpes`].
unsafe fn quiet_gpes(blk: u16, len: u8) {
    if blk == 0 || len < 2 {
        return;
    }
    let half = u16::from(len / 2);
    for i in 0..half {
        let (status_port, enable_port) = (
            blk.saturating_add(i),
            blk.saturating_add(half).saturating_add(i),
        );
        // SAFETY: a status byte and its enable byte of the block (the
        // caller's contract); reading them changes nothing.
        let (status, enabled) =
            unsafe { (crate::port::inb(status_port), crate::port::inb(enable_port)) };
        let firing = status & enabled;
        if firing != 0 {
            // SAFETY: as above; the firing events' enable bits cleared, then
            // their status bits, which writing 1 clears.
            unsafe {
                crate::port::outb(enable_port, enabled & !firing);
                crate::port::outb(status_port, firing);
            }
            GPES_QUIETED.fetch_add(u64::from(firing.count_ones()), Ordering::AcqRel);
        }
    }
}

/// Where the SCI -- ISA IRQ `irq` -- comes in, and how: the I/O APIC pin a
/// MADT override names for it, else the IRQ itself, and its trigger mode
/// and polarity ([`sci_sense`]). `None`, said, for an IRQ or a pin the I/O
/// APIC cannot take.
fn sci_route(irq: u16) -> Option<(u8, bool, bool)> {
    let Ok(source) = u8::try_from(irq) else {
        serial_println!(
            "[acpi] power button: the SCI's IRQ {irq} is no ISA IRQ; left to the firmware"
        );
        return None;
    };
    let over = crate::acpi::interrupt_overrides()
        .into_iter()
        .find(|o| o.bus == 0 && o.source_irq == source);
    let pin = over.as_ref().map_or(u32::from(source), |o| o.gsi);
    let Some(pin) = u8::try_from(pin)
        .ok()
        .filter(|&p| usize::from(p) < crate::ioapic::MAX_IRQ)
    else {
        serial_println!(
            "[acpi] power button: the SCI's pin {pin} is past the I/O APIC's; left to the firmware"
        );
        return None;
    };
    let (level, active_low) = sci_sense(over.map(|o| o.flags));
    Some((pin, level, active_low))
}

/// The SCI's trigger mode and polarity -- `(level, active_low)` -- from a
/// MADT override's `flags`, if one names it: each field the override leaves
/// "conforming" (0) is ACPI's own for the SCI, level-triggered and
/// active-low, as ACPICA reads it; a reserved value (2) is read so too.
fn sci_sense(flags: Option<u16>) -> (bool, bool) {
    let flags = flags.unwrap_or(0);
    let level = (flags >> 2) & 0b11 != 0b01;
    let active_low = flags & 0b11 != 0b01;
    (level, active_low)
}

/// The SCI, from `ioapic::handle_device_irq`, in interrupt context: the
/// power button's status, where set and enabled, cleared -- before the
/// EOI, or the level-triggered line would raise it again -- and the press
/// handed to a kernel worker ([`pressed`]); and any general-purpose event
/// found firing turned off ([`quiet_gpes`]), since nothing here can answer
/// one and its level would hold the SCI asserted. It takes only the work
/// queue's lock, which every holder takes with interrupts off.
pub fn handle_sci() {
    // SAFETY: the GPE blocks [`init`] took from the FADT and disabled.
    unsafe {
        quiet_gpes(
            GPE0_BLK.load(Ordering::Acquire),
            GPE0_LEN.load(Ordering::Acquire),
        );
        quiet_gpes(
            GPE1_BLK.load(Ordering::Acquire),
            GPE1_LEN.load(Ordering::Acquire),
        );
    }
    let len = PM1_EVT_LEN.load(Ordering::Acquire);
    let mut pressed_now = false;
    for evt in [
        PM1A_EVT.load(Ordering::Acquire),
        PM1B_EVT.load(Ordering::Acquire),
    ] {
        if evt == 0 {
            continue;
        }
        // SAFETY: a PM1 event block [`init`] took from the FADT: reading
        // its status register changes nothing.
        let status = unsafe { crate::port::inw(evt) };
        // SAFETY: the same block's enable register, its second half:
        // reading it changes nothing.
        let enabled = unsafe { crate::port::inw(enable_port(evt, len)) };
        if status & enabled & PWRBTN != 0 {
            // SAFETY: writing 1 to the button's status bit clears it alone.
            unsafe { crate::port::outw(evt, PWRBTN) };
            pressed_now = true;
        }
    }
    if pressed_now && !crate::workqueue::submit(pressed, 0) {
        // The worker's queue is full: the press is lost, and said.
        crate::serial_println!(
            "[acpi] power button pressed, but the work queue is full: not acted on"
        );
    }
}

/// A press of the power button, in a kernel worker: recorded, and -- unless
/// a harness presses the button -- what the power policy says done.
/// Shutting down is done in order and ends in power-off. Sleep and
/// hibernation, which SlateOS cannot do yet, shut down instead, as the
/// press must do something (design-decisions 1597; open-questions A-Q32);
/// actions it cannot do and that are no way of stopping -- the screen off,
/// a log-out, the lock screen -- do nothing, said. A press while a shutdown
/// is already under way adds nothing to it.
fn pressed(_arg: u64) {
    use crate::fs::power::PowerAction;
    PRESSES.fetch_add(1, Ordering::AcqRel);
    // A count, for /proc's ACPI event statistics: its failure costs only that.
    let _ = crate::fs::acpistat::record_event(crate::fs::acpistat::AcpiEvent::PowerButton);
    if HARNESS.load(Ordering::Acquire) {
        serial_println!(
            "[acpi] power button pressed -- the harness's press: recorded, not acted on"
        );
        return;
    }
    let action = crate::fs::power::handle_power_button();
    match action {
        PowerAction::Nothing => {
            serial_println!("[acpi] power button pressed: the policy says do nothing");
        }
        PowerAction::ScreenOff | PowerAction::LogOut | PowerAction::LockScreen => {
            serial_println!(
                "[acpi] power button pressed: the policy says {}, which SlateOS cannot do yet; nothing done",
                action.label()
            );
        }
        PowerAction::ShutDown
        | PowerAction::Sleep
        | PowerAction::Hibernate
        | PowerAction::RunThenSleep(_)
        | PowerAction::RunThenShutdown(_) => {
            if matches!(action, PowerAction::ShutDown) {
                serial_println!("[acpi] power button pressed: shutting down");
            } else {
                serial_println!(
                    "[acpi] power button pressed: the policy says {}, which SlateOS cannot do yet; shutting down instead",
                    action.label()
                );
            }
            // Its one failure is a shutdown already under way, which goes
            // on as it was asked to -- a reboot stays a reboot.
            if crate::initproc::shutdown(crate::initproc::ShutdownReason::PowerButton).is_err() {
                serial_println!(
                    "[acpi] a shutdown is already under way: the press adds nothing to it"
                );
                return;
            }
            crate::power::shutdown();
        }
    }
}

/// The power button under the boot test's harness (`acpi.pwrbtn=hmp`): the
/// SCI's sense worked out right from every form a MADT override takes; in
/// QEMU, ACPI mode given back to the firmware and taken again
/// (`ACPI_DISABLE`, then `ACPI_ENABLE`, the handoff a PC in legacy mode
/// needs); and the harness's press -- asked for on the serial line, made by
/// QEMU's `system_powerdown` -- reaching [`pressed`] within
/// [`PRESS_WAIT_MS`]. Skipped, and said, without a harness.
///
/// # Errors
///
/// `InternalError`, said, for anything not so.
pub fn self_test() -> KernelResult<()> {
    let fail = |what: &str| {
        serial_println!("[acpi]   FAIL: power button: {what}");
        KernelError::InternalError
    };
    // (flags, level, active-low): conforming fields are ACPI's SCI default.
    let senses: [(Option<u16>, bool, bool); 6] = [
        (None, true, true),
        (Some(0x0), true, true),
        (Some(0xD), true, false),
        (Some(0xC), true, true),
        (Some(0x1), true, false),
        (Some(0x7), false, true),
    ];
    // The pure checks' failures are said and kept, and the rest still run:
    // one failure does not hide another.
    let mut pure = Ok(());
    if senses
        .iter()
        .any(|&(flags, level, low)| sci_sense(flags) != (level, low))
    {
        pure = Err(fail(
            "the SCI's sense is not read from an override's flags as ACPI says",
        ));
    }
    // A PM1 event block of 4 bytes has its enable register at +2; every
    // fixed status bit, the button's among them, is cleared at once.
    if enable_port(0x600, 4) != 0x602 || PM1_ALL_STATUS & PWRBTN == 0 {
        pure = Err(fail(
            "a PM1 event block's enable register, or its status bits, misread",
        ));
    }
    if !HARNESS.load(Ordering::Acquire) {
        serial_println!(
            "[acpi] power button self-test: skipped beyond the SCI's sense, no harness to press it ({HARNESS_WORD}=hmp)"
        );
        return pure;
    }
    if sci_pin().is_none() {
        return Err(fail("not handled, so the harness's press cannot be"));
    }
    let f = (*FADT.lock()).ok_or_else(|| fail("no FADT"))?;
    if f.smi_command != 0 && f.acpi_disable != 0 {
        if !set_acpi_mode(&f, false) {
            return Err(fail("ACPI mode could not be given back to the firmware"));
        }
        if !set_acpi_mode(&f, true) {
            return Err(fail("ACPI mode, given back, could not be taken again"));
        }
        serial_println!("[acpi] power button: ACPI mode given back and taken again");
    }
    let before = presses();
    serial_println!("[acpi] power button: press it");
    let until = crate::hrtimer::now_ns().saturating_add(PRESS_WAIT_MS * 1_000_000);
    while presses() == before && crate::hrtimer::now_ns() < until {
        crate::sched::sleep_ms(20);
    }
    if presses() == before {
        return Err(fail(&alloc::format!(
            "no press reached the kernel within {PRESS_WAIT_MS} ms of asking"
        )));
    }
    serial_println!(
        "[acpi] power button: the harness's press raised the SCI, which took it to the worker: OK"
    );
    pure
}
