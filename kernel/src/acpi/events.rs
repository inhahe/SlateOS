//! ACPI fixed events: the power button.
//!
//! The firmware hands the chipset over in one of two modes, which PM1
//! control's `SCI_EN` bit says. In legacy mode the firmware itself acts on
//! the power button -- on most PCs by powering off at once, a power cut as
//! far as the OS knows. In ACPI mode the press is the OS's: it sets the
//! button's status bit in PM1 status and, the button's event enabled,
//! raises the SCI, the interrupt ACPI's fixed and general-purpose events
//! share; the firmware no longer acts on it.
//!
//! [`init`] says which mode the firmware handed over and makes the button
//! the OS's: what can be checked first is -- the FADT's blocks, the SCI's
//! pin, the work queue -- then ACPI mode is taken where the firmware kept
//! it, as ACPICA's `acpi_enable` takes it, and the events are armed
//! ([`arm_events`]): every general-purpose event disabled (SlateOS runs no
//! AML that could answer one, and a level-triggered SCI left asserted by
//! one would never stop), every fixed event's status cleared, and of the
//! fixed events that raise the SCI only the button's enabled. Unmasking the
//! SCI is last; should it fail (its interrupt-remapping entry, say), every
//! enable register `arm_events` rewrote is put back as the firmware had it
//! and ACPI mode given back. The SCI's pin was given its sense by
//! `ioapic::init`, from the MADT and [`sci_sense`].
//!
//! The SCI's handler also turns off any other event, fixed or
//! general-purpose, it finds enabled and firing -- one the firmware enabled
//! again -- since nothing here answers one and a level-triggered SCI it held
//! asserted would storm until the pin was masked, the button's with it.
//!
//! On a press the SCI's handler ([`handle_sci`], from
//! `ioapic::handle_device_irq`) clears the button's status and hands the
//! press to a kernel worker ([`crate::workqueue`]), which does what the
//! power policy says ([`crate::fs::power::handle_power_button`]): to shut
//! down is [`crate::initproc::shutdown`] -- the kernel's service table
//! marked stopped, filesystems synced, logs flushed; no running program is
//! asked to stop, which needs a userspace init that handles shutdown
//! (known-issues A-POWER-BUTTON-SHUTDOWN-ASKS-NO-PROGRAM-TO-STOP) -- then
//! [`crate::power::shutdown`], the S5 power-off (design-decisions 1597).
//!
//! Under the boot test (`acpi.pwrbtn=hmp` on the command line) every press
//! is the harness's: recorded, never acted on ([`self_test`]). A
//! hardware-reduced platform has no fixed button. One whose FADT makes the
//! button a control-method device (flag `PWR_BUTTON`) is left to the
//! firmware where the firmware kept legacy mode; where ACPI mode is the
//! OS's already, the fixed event is armed all the same, since the firmware
//! will not act and the button's line usually sets `PWRBTN_STS` anyway.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

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
/// PM1 enable: the bits that let a fixed event raise the SCI -- timer,
/// global lock, power and sleep buttons, RTC (ACPI 6.5 table 4.14). The
/// only ones the kernel writes: bit 14, `PCIEXP_WAKE_DIS`, works the other
/// way about (set, it disables a wake) and raises nothing, and the rest are
/// reserved, so each write keeps them as they were, as ACPICA's do.
const PM1_SCI_ENABLES: u16 = 0x0721;
/// PM1 control: `SCI_EN`, set while the OS has ACPI mode.
const SCI_EN: u16 = 1;
/// How long the firmware has to change `SCI_EN` once asked.
const MODE_WAIT_MS: u64 = 3000;
/// How long [`self_test`] waits for the harness's press.
const PRESS_WAIT_MS: u64 = 20_000;
/// PM1 status and enable: the PM timer's bit (`TMR_STS`, `TMR_EN`), which
/// [`self_test`] enables behind the kernel's back.
const TMR: u16 = 1;
/// How long [`self_test`] waits for the PM timer's event to fire and be
/// turned off: over twice the 2.3 s QEMU's timer takes to set it.
const TIMER_WAIT_MS: u64 = 5000;
/// The kernel command line's word that says a harness presses the button
/// (`scripts/boot-test.sh`'s `pwrbtn_responder`), and every press is its.
const HARNESS_WORD: &str = "acpi.pwrbtn";

// A 4-byte PM1 event block has its enable register at +2; clearing every
// fixed status bit clears the button's.
const _: () = assert!(enable_port(0x600, 4) == 0x602);
const _: () = assert!(PM1_ALL_STATUS & PWRBTN != 0);
const _: () = assert!(PM1_SCI_ENABLES & (PWRBTN | TMR) == PWRBTN | TMR);

/// The FADT's power-management registers, from `acpi::init` ([`note_fadt`]).
static FADT: Mutex<Option<PowerInfo>> = Mutex::new(None);
/// The FADT's registers as [`init`] armed them, for [`handle_sci`], which
/// may take no lock: set once, before the SCI is unmasked, and kept should
/// the unmask fail -- what says the button is handled is [`SCI_PIN`], which
/// `init` then clears again, so that `handle_sci` is never called.
static ARMED: spin::Once<PowerInfo> = spin::Once::new();
/// The SCI's I/O APIC pin, [`u32::MAX`] while it is not routed: set only
/// while the button is handled.
static SCI_PIN: AtomicU32 = AtomicU32::new(u32::MAX);
/// Presses handled since boot.
static PRESSES: AtomicU64 = AtomicU64::new(0);
/// Whether a harness presses the button (`acpi.pwrbtn=hmp`): every press is
/// recorded and none acted on. Read once, by [`init`].
static HARNESS: AtomicBool = AtomicBool::new(false);

/// The FADT's power-management registers, kept for [`init`] and for
/// `ioapic::init`'s SCI sense ([`sci_irq`]): `acpi::init`'s, once it has
/// parsed the FADT.
pub fn note_fadt(info: &PowerInfo) {
    *FADT.lock() = Some(*info);
}

/// The SCI's ISA IRQ, as the FADT names it; `None` without a FADT or on a
/// hardware-reduced platform, which has no SCI of this kind.
#[must_use]
pub fn sci_irq() -> Option<u16> {
    FADT.lock()
        .as_ref()
        .filter(|f| !f.hw_reduced)
        .map(|f| f.sci_irq)
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

/// The SCI's trigger mode and polarity -- `(level, active_low)` -- from a
/// MADT override's `flags`, if one names it: each field the override leaves
/// "conforming" (0) is ACPI's own for the SCI, level-triggered and
/// active-low, as ACPICA and Linux's `acpi_sci_ioapic_setup` read it, not
/// the ISA bus's edge and active-high; a reserved value (2) is read so too.
/// `ioapic::init`'s, as it programs the SCI's pin.
#[must_use]
pub fn sci_sense(flags: Option<u16>) -> (bool, bool) {
    let flags = flags.unwrap_or(0);
    let level = (flags >> 2) & 0b11 != 0b01;
    let active_low = flags & 0b11 != 0b01;
    (level, active_low)
}

/// The power button made the OS's, as the module doc says. Said either
/// way. After the I/O APIC, interrupt remapping and the work queue are up.
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
    // ACPI mode the OS's already: the firmware acts on no press.
    let os_has_it = sci_en || f.smi_command == 0;
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
    let whose = if os_has_it {
        "no one has it, ACPI mode being the OS's"
    } else {
        "left to the firmware"
    };
    if let Some(why) = not_handled(&f, os_has_it) {
        serial_println!("[acpi] power button: {why}; {whose}");
        return;
    }
    let Some(pin) = sci_route(f.sci_irq) else {
        serial_println!("[acpi] power button: the SCI cannot be routed; {whose}");
        return;
    };
    if !crate::workqueue::is_running() {
        serial_println!(
            "[acpi] power button: the work queue, which acts on a press, is not running; {whose}"
        );
        return;
    }
    // Read before ACPI mode is taken, which firmware may answer by
    // rewriting them: what is put back for the firmware is what it had.
    // SAFETY: the FADT's blocks.
    let firmware_enables = unsafe { FirmwareEnables::read(&f) };
    let taken = if os_has_it {
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
            "[acpi] power button: ACPI mode not taken (no mode switch, or SCI_EN still off {MODE_WAIT_MS} ms after ACPI_ENABLE); left to the firmware"
        );
        return;
    };
    // SAFETY: the FADT's blocks, in ACPI mode.
    unsafe { arm_events(&f) };
    ARMED.call_once(|| f);
    SCI_PIN.store(u32::from(pin), Ordering::Release);
    // SAFETY: the pin's handler is in place (`sci_pin`, read by
    // `ioapic::handle_device_irq`), its sense set by `ioapic::init`, and
    // every status that could hold it asserted is cleared.
    if unsafe { crate::ioapic::unmask_irq(pin) } {
        serial_println!(
            "[acpi] power button: handled -- the SCI (IRQ {}, I/O APIC pin {pin}) taken, the button's event alone enabled",
            f.sci_irq
        );
        return;
    }
    SCI_PIN.store(u32::MAX, Ordering::Release);
    // SAFETY: the blocks they were read from, ACPI mode still the OS's.
    unsafe { firmware_enables.restore(&f) };
    let given_back = taken && set_acpi_mode(&f, false);
    serial_println!(
        "[acpi] power button: the SCI's pin {pin} could not be unmasked; {}",
        if given_back {
            "ACPI mode given back, the firmware has the button again"
        } else if taken {
            "ACPI mode could not be given back: no one has the button"
        } else {
            "no one has the button"
        }
    );
}

/// Why the FADT's power button cannot be the kernel's, `None` when it can:
/// a hardware-reduced platform (no fixed hardware), no PM1 blocks, or --
/// while the firmware still has legacy mode (`!os_has_it`) -- a button the
/// FADT makes a control-method device (flag `PWR_BUTTON`), which the
/// firmware then handles itself.
fn not_handled(f: &PowerInfo, os_has_it: bool) -> Option<&'static str> {
    if f.hw_reduced {
        Some("hardware-reduced ACPI, which has no fixed power button")
    } else if f.pm1a_evt_blk == 0 || f.pm1a_cnt_blk == 0 || f.pm1_evt_len < 4 {
        Some("the FADT names no PM1 event and control blocks")
    } else if !f.fixed_power_button && !os_has_it {
        Some("the FADT makes it a control-method device, which needs AML")
    } else {
        None
    }
}

/// Whether the FADT gives a way to switch ACPI mode: an SMI command port,
/// and an `ACPI_ENABLE` and `ACPI_DISABLE` not both 0 -- both 0 meaning
/// none, as ACPICA reads them.
fn can_switch(f: &PowerInfo) -> bool {
    f.smi_command != 0 && !(f.acpi_enable == 0 && f.acpi_disable == 0)
}

/// Take ACPI mode (`on`) or give it back: the FADT's `ACPI_ENABLE` or
/// `ACPI_DISABLE` written to `SMI_CMD`, and up to [`MODE_WAIT_MS`] waited
/// for `SCI_EN` to follow. Whether it did; `false`, nothing written, where
/// the FADT gives no way to switch ([`can_switch`]).
fn set_acpi_mode(f: &PowerInfo, on: bool) -> bool {
    if !can_switch(f) {
        return false;
    }
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

/// The events armed, in ACPI mode: every GPE disabled and its status
/// cleared, every fixed event's status cleared, and of the fixed events
/// that raise the SCI ([`PM1_SCI_ENABLES`]) the button's alone enabled.
/// After ACPI mode is taken, which firmware may answer by disabling every
/// event.
///
/// # Safety
///
/// `f`'s blocks are the FADT's, and ACPI mode is the OS's.
unsafe fn arm_events(f: &PowerInfo) {
    for (status, enable) in gpe_ports(f) {
        // SAFETY: an enable byte and a status byte of a GPE block (the
        // caller's contract): disabled, and cleared by writing 1s.
        unsafe {
            crate::port::outb(enable, 0);
            crate::port::outb(status, 0xFF);
        }
    }
    for evt in pm1_blocks(f) {
        let enables = enable_port(evt, f.pm1_evt_len);
        // SAFETY: a PM1 event block's status register, cleared by writing
        // 1s, and its enable register, the OS's in ACPI mode: of the bits
        // that raise the SCI, the button's alone set.
        unsafe {
            crate::port::outw(evt, PM1_ALL_STATUS);
            let kept = crate::port::inw(enables) & !PM1_SCI_ENABLES;
            crate::port::outw(enables, kept | PWRBTN);
        }
    }
}

/// The enable registers [`arm_events`] rewrites, as the firmware handed them
/// over -- read before ACPI mode is taken: each PM1 event block's
/// ([`pm1_blocks`]' order), then each GPE enable byte ([`gpe_ports`]').
/// What [`init`] puts back when it cannot take the SCI after all.
struct FirmwareEnables {
    pm1: alloc::vec::Vec<u16>,
    gpe: alloc::vec::Vec<u8>,
}

impl FirmwareEnables {
    /// The enable registers of `f`'s blocks, read.
    ///
    /// # Safety
    ///
    /// `f`'s blocks are the FADT's.
    unsafe fn read(f: &PowerInfo) -> Self {
        Self {
            pm1: pm1_blocks(f)
                // SAFETY: a PM1 event block's enable register (the
                // caller's contract); reading it changes nothing.
                .map(|evt| unsafe { crate::port::inw(enable_port(evt, f.pm1_evt_len)) })
                .collect(),
            gpe: gpe_ports(f)
                // SAFETY: a GPE block's enable byte (the caller's
                // contract); reading it changes nothing.
                .map(|(_, enable)| unsafe { crate::port::inb(enable) })
                .collect(),
        }
    }

    /// The enable registers put back as [`FirmwareEnables::read`] found them.
    ///
    /// # Safety
    ///
    /// `f` is the one they were read from, and ACPI mode is the OS's.
    unsafe fn restore(&self, f: &PowerInfo) {
        for (evt, &value) in pm1_blocks(f).zip(&self.pm1) {
            // SAFETY: the PM1 event block's enable register it was read
            // from (the caller's contract), the OS's in ACPI mode.
            unsafe { crate::port::outw(enable_port(evt, f.pm1_evt_len), value) };
        }
        for ((_, enable), &value) in gpe_ports(f).zip(&self.gpe) {
            // SAFETY: as above, a GPE block's enable byte.
            unsafe { crate::port::outb(enable, value) };
        }
    }
}

/// The PM1 event blocks the FADT names: PM1a's, then PM1b's if it has one.
fn pm1_blocks(f: &PowerInfo) -> impl Iterator<Item = u16> {
    [f.pm1a_evt_blk, f.pm1b_evt_blk]
        .into_iter()
        .filter(|&evt| evt != 0)
}

/// The enable register of the PM1 event block at `evt`: its second half.
const fn enable_port(evt: u16, len: u8) -> u16 {
    evt.saturating_add(len as u16 / 2)
}

/// The FADT's GPE blocks -- each status bytes, then as many enable bytes --
/// as `(status, enable)` port pairs, GPE0's then GPE1's; none for a block
/// the FADT does not name.
fn gpe_ports(f: &PowerInfo) -> impl Iterator<Item = (u16, u16)> {
    [(f.gpe0_blk, f.gpe0_blk_len), (f.gpe1_blk, f.gpe1_blk_len)]
        .into_iter()
        .flat_map(|(blk, len)| {
            let half = if blk == 0 { 0 } else { u16::from(len / 2) };
            (0..half).map(move |i| {
                (
                    blk.saturating_add(i),
                    blk.saturating_add(half).saturating_add(i),
                )
            })
        })
}

/// The I/O APIC pin the SCI -- ISA IRQ `irq` -- comes in on: the one a
/// MADT override names for it, else the IRQ itself. `None` for an IRQ or a
/// pin the I/O APIC does not have ([`crate::ioapic::pins`]), so that
/// unmasking it is the one step of [`init`] left to fail once ACPI mode is
/// taken.
fn sci_route(irq: u16) -> Option<u8> {
    let source = u8::try_from(irq).ok()?;
    let pin = crate::acpi::interrupt_overrides()
        .into_iter()
        .find(|o| o.bus == 0 && o.source_irq == source)
        .map_or(u32::from(source), |o| o.gsi);
    u8::try_from(pin)
        .ok()
        .filter(|&p| usize::from(p) < crate::ioapic::pins())
}

/// The SCI, from `ioapic::handle_device_irq`, in interrupt context: the
/// power button's status, where set and enabled, cleared -- before the
/// EOI, or the level-triggered line would raise it again -- and the press
/// handed to a kernel worker ([`pressed`]); and any other event, fixed or
/// general-purpose, found both enabled and firing -- one the firmware
/// enabled again after [`arm_events`], as its SMM may -- turned off and its
/// status cleared, since nothing here can answer one and its level would
/// hold the SCI asserted until the storm guard masked the pin, the button's
/// with it. It takes only the work queue's lock, which every holder takes
/// with interrupts off.
pub fn handle_sci() {
    let Some(f) = ARMED.get() else {
        return;
    };
    for (status_port, enable_port) in gpe_ports(f) {
        // SAFETY: a status byte and its enable byte of a GPE block `init`
        // armed; reading them changes nothing.
        let (status, enabled) =
            unsafe { (crate::port::inb(status_port), crate::port::inb(enable_port)) };
        let firing = status & enabled;
        if firing != 0 {
            // SAFETY: as above: the firing events disabled, then their
            // status cleared by writing 1s.
            unsafe {
                crate::port::outb(enable_port, enabled & !firing);
                crate::port::outb(status_port, firing);
            }
        }
    }
    let mut pressed_now = false;
    for evt in pm1_blocks(f) {
        // SAFETY: a PM1 event block `init` armed: reading its status
        // register changes nothing.
        let status = unsafe { crate::port::inw(evt) };
        // SAFETY: the same block's enable register, its second half:
        // reading it changes nothing.
        let enabled = unsafe { crate::port::inw(enable_port(evt, f.pm1_evt_len)) };
        let firing = status & enabled & PM1_SCI_ENABLES;
        if firing == 0 {
            continue;
        }
        // A fixed event other than the button's -- one the firmware enabled
        // again, as for a GPE above -- is turned off: its status would hold
        // the SCI asserted as surely.
        let others = firing & !PWRBTN;
        if others != 0 {
            // SAFETY: the same enable register, the OS's in ACPI mode: every
            // event but the firing others left as it was.
            unsafe { crate::port::outw(enable_port(evt, f.pm1_evt_len), enabled & !others) };
        }
        // SAFETY: writing 1s to the firing events' status bits clears them
        // alone.
        unsafe { crate::port::outw(evt, firing) };
        pressed_now |= firing & PWRBTN != 0;
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
/// is under way adds nothing to it; one after a shutdown that halted
/// without powering off powers off.
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
            // Its one failure is a shutdown already asked for: one under
            // way goes on as it was asked to (a reboot stays a reboot); one
            // that halted without powering off, nothing will follow, so the
            // press powers off.
            if crate::initproc::shutdown(crate::initproc::ShutdownReason::PowerButton).is_err()
                && crate::initproc::system_state() != crate::initproc::SystemState::Halted
            {
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
/// needs) and the events armed again, as `init` arms them after taking it;
/// the PM timer's fixed event, enabled behind the kernel's back, turned off
/// by [`handle_sci`] when it fires;
/// and the harness's press -- asked for on the serial line, made by QEMU's
/// `system_powerdown` -- reaching [`pressed`] within [`PRESS_WAIT_MS`].
/// Skipped beyond the sense, and said, without a harness.
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
    // A pure check's failure is said and kept, and the rest still run: one
    // failure does not hide another.
    let mut pure = Ok(());
    if senses
        .iter()
        .any(|&(flags, level, low)| sci_sense(flags) != (level, low))
    {
        pure = Err(fail(
            "the SCI's sense is not read from an override's flags as ACPI says",
        ));
    }
    if !HARNESS.load(Ordering::Acquire) {
        serial_println!(
            "[acpi] power button self-test: skipped beyond the SCI's sense, no harness to press it ({HARNESS_WORD}=hmp)"
        );
        return pure;
    }
    let f = match (ARMED.get(), sci_pin()) {
        (Some(f), Some(_)) => *f,
        _ => return Err(fail("not handled, so the harness's press cannot be")),
    };
    if can_switch(&f) {
        if !set_acpi_mode(&f, false) {
            return Err(fail("ACPI mode could not be given back to the firmware"));
        }
        if !set_acpi_mode(&f, true) {
            return Err(fail("ACPI mode, given back, could not be taken again"));
        }
        // SAFETY: the FADT's blocks, in ACPI mode taken again.
        unsafe { arm_events(&f) };
        serial_println!(
            "[acpi] power button: ACPI mode given back, taken again, the events armed again"
        );
    }
    // A fixed event enabled behind the kernel's back, as a firmware may: the
    // PM timer's, whose status QEMU sets each time the timer's bit 23 turns
    // over, every 2.3 s. `handle_sci` must turn it off when it fires.
    let enables = enable_port(f.pm1a_evt_blk, f.pm1_evt_len);
    // SAFETY: PM1a's enable register, the OS's in ACPI mode: the timer's
    // event enabled beside the button's.
    unsafe { crate::port::outw(enables, crate::port::inw(enables) | TMR) };
    let until = crate::hrtimer::now_ns().saturating_add(TIMER_WAIT_MS * 1_000_000);
    // SAFETY: the same register; reading it changes nothing.
    while unsafe { crate::port::inw(enables) } & TMR != 0 && crate::hrtimer::now_ns() < until {
        crate::sched::sleep_ms(20);
    }
    // SAFETY: as above.
    if unsafe { crate::port::inw(enables) } & TMR != 0 {
        // SAFETY: as above: the timer's event disabled again.
        unsafe { crate::port::outw(enables, crate::port::inw(enables) & !TMR) };
        return Err(fail(&alloc::format!(
            "the PM timer's event, enabled behind the kernel's back, was still on {TIMER_WAIT_MS} ms later"
        )));
    }
    serial_println!(
        "[acpi] power button: a fixed event enabled behind the kernel's back was turned off when it fired: OK"
    );
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
