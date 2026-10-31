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
//! [`init`] says which mode the firmware handed over, takes ACPI mode when
//! it was not -- writing `ACPI_ENABLE` to the SMI command port, as Linux's
//! `acpi_enable` does -- and enables the power button's event and nothing
//! else. Every general-purpose event is disabled: SlateOS runs no AML that
//! could answer one, and a level-triggered SCI left asserted by one would
//! never stop. The SCI's I/O APIC pin is routed to [`handle_sci`]
//! (`ioapic::handle_device_irq`), which clears the button's status and
//! hands the press to a kernel worker ([`crate::workqueue`]): what the power
//! policy says is done there ([`crate::fs::power::handle_power_button`]),
//! a shutdown in order -- services stopped, filesystems synced
//! ([`crate::initproc::shutdown`]) -- then power off
//! ([`crate::power::shutdown`]). A press the boot's self-test asks for
//! ([`self_test`]) is recorded and nothing more.
//!
//! Hardware-reduced ACPI platforms have no fixed power button (their button
//! is a device the AML describes) and are said and left alone.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};

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
/// How long the firmware has to set `SCI_EN` once asked.
const ACPI_ENABLE_WAIT_MS: u64 = 3000;
/// How long [`self_test`] waits for the harness's press.
const PRESS_WAIT_MS: u64 = 20_000;
/// The kernel command line's word that says the harness presses the button
/// when asked (`scripts/boot-test.sh`'s `pwrbtn_responder`).
const HARNESS_WORD: &str = "acpi.pwrbtn";

/// What [`init`] needs of the FADT, stored by `acpi::init`.
#[derive(Debug, Clone, Copy)]
struct Registers {
    pm1a_evt: u16,
    pm1b_evt: u16,
    pm1_evt_len: u8,
    pm1a_cnt: u16,
    gpe0: u16,
    gpe0_len: u8,
    gpe1: u16,
    gpe1_len: u8,
    smi_command: u16,
    acpi_enable: u8,
    sci_irq: u16,
    hw_reduced: bool,
}

/// The FADT's registers, from `acpi::init` ([`note_fadt`]).
static REGISTERS: Mutex<Option<Registers>> = Mutex::new(None);
/// The PM1 event blocks and their length, for [`handle_sci`], which may
/// take no lock: 0 until [`init`] has enabled the button.
static PM1A_EVT: AtomicU16 = AtomicU16::new(0);
static PM1B_EVT: AtomicU16 = AtomicU16::new(0);
static PM1_EVT_LEN: AtomicU8 = AtomicU8::new(0);
/// The SCI's I/O APIC pin, [`u32::MAX`] while it is not routed.
static SCI_PIN: AtomicU32 = AtomicU32::new(u32::MAX);
/// Presses handled since boot.
static PRESSES: AtomicU64 = AtomicU64::new(0);
/// Whether [`self_test`] is waiting for the harness's press: the next press
/// is recorded and not acted on.
static TEST_EXPECTING: AtomicBool = AtomicBool::new(false);
/// Whether the press [`self_test`] waited for came.
static TEST_SEEN: AtomicBool = AtomicBool::new(false);

/// The FADT's power-management registers, kept for [`init`]: `acpi::init`'s,
/// once it has parsed the FADT.
pub fn note_fadt(info: &crate::acpi::fadt::PowerInfo) {
    *REGISTERS.lock() = Some(Registers {
        pm1a_evt: info.pm1a_evt_blk,
        pm1b_evt: info.pm1b_evt_blk,
        pm1_evt_len: info.pm1_evt_len,
        pm1a_cnt: info.pm1a_cnt_blk,
        gpe0: info.gpe0_blk,
        gpe0_len: info.gpe0_blk_len,
        gpe1: info.gpe1_blk,
        gpe1_len: info.gpe1_blk_len,
        smi_command: info.smi_command,
        acpi_enable: info.acpi_enable,
        sci_irq: info.sci_irq,
        hw_reduced: info.hw_reduced,
    });
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

/// The power button made the OS's: the firmware's mode said, ACPI mode
/// taken when the firmware kept it, every general-purpose event disabled,
/// every fixed event's status cleared and only the power button's enabled,
/// and the SCI routed to [`handle_sci`]. Said either way; nothing done on a
/// platform with no fixed power button, or where the firmware will not give
/// ACPI mode up. After the I/O APIC, interrupt remapping and the work queue
/// are up.
pub fn init() {
    let Some(r) = *REGISTERS.lock() else {
        serial_println!("[acpi] power button: no FADT, so not handled");
        return;
    };
    if r.hw_reduced {
        serial_println!(
            "[acpi] power button: hardware-reduced ACPI, which has no fixed power button; not handled"
        );
        return;
    }
    if r.pm1a_evt == 0 || r.pm1a_cnt == 0 || r.pm1_evt_len < 4 {
        serial_println!(
            "[acpi] power button: the FADT names no PM1 event and control blocks; not handled"
        );
        return;
    }
    // SAFETY: PM1a_CNT is the FADT's own port for the chipset's PM1
    // control register; reading it changes nothing.
    let handed_over = unsafe { crate::port::inw(r.pm1a_cnt) } & SCI_EN != 0;
    if handed_over {
        serial_println!("[acpi] ACPI mode: on at handoff (SCI_EN set by the firmware)");
    } else if r.smi_command == 0 || r.acpi_enable == 0 {
        serial_println!(
            "[acpi] ACPI mode: off at handoff, and the FADT gives no way to take it (SMI_CMD {:#x}, ACPI_ENABLE {:#x}): the firmware keeps the power button",
            r.smi_command,
            r.acpi_enable
        );
        return;
    } else if take_acpi_mode(&r) {
        serial_println!(
            "[acpi] ACPI mode: off at handoff; taken (ACPI_ENABLE {:#x} written to SMI_CMD {:#x})",
            r.acpi_enable,
            r.smi_command
        );
    } else {
        serial_println!(
            "[acpi] ACPI mode: off at handoff, and still off {ACPI_ENABLE_WAIT_MS} ms after ACPI_ENABLE was written: the firmware keeps the power button"
        );
        return;
    }
    // SAFETY: the FADT's GPE and PM1 event blocks, in ACPI mode: their
    // status bits clear when written 1, their enable bits are the OS's.
    unsafe {
        disable_gpes(r.gpe0, r.gpe0_len);
        disable_gpes(r.gpe1, r.gpe1_len);
        for evt in [r.pm1a_evt, r.pm1b_evt] {
            if evt != 0 {
                crate::port::outw(evt, PM1_ALL_STATUS);
                crate::port::outw(enable_port(evt, r.pm1_evt_len), PWRBTN);
            }
        }
    }
    PM1A_EVT.store(r.pm1a_evt, Ordering::Release);
    PM1B_EVT.store(r.pm1b_evt, Ordering::Release);
    PM1_EVT_LEN.store(r.pm1_evt_len, Ordering::Release);
    route_sci(r.sci_irq);
}

/// Ask the firmware for ACPI mode -- `ACPI_ENABLE` written to `SMI_CMD` --
/// and wait up to [`ACPI_ENABLE_WAIT_MS`] for `SCI_EN`: whether it came.
fn take_acpi_mode(r: &Registers) -> bool {
    // SAFETY: SMI_CMD and ACPI_ENABLE are the FADT's own for this request;
    // the write raises an SMI the firmware answers by setting SCI_EN.
    unsafe { crate::port::outb(r.smi_command, r.acpi_enable) };
    let until = crate::hrtimer::now_ns().saturating_add(ACPI_ENABLE_WAIT_MS * 1_000_000);
    loop {
        // SAFETY: as in `init`, reading PM1a_CNT changes nothing.
        if unsafe { crate::port::inw(r.pm1a_cnt) } & SCI_EN != 0 {
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

/// The SCI -- ISA IRQ `irq`, or the pin a MADT override names for it --
/// routed to [`handle_sci`] and unmasked. Level-triggered and active-low,
/// ACPI's own SCI, unless an override says otherwise, which the I/O APIC's
/// setup has applied already.
fn route_sci(irq: u16) {
    let Ok(source) = u8::try_from(irq) else {
        serial_println!("[acpi] power button: the SCI's IRQ {irq} is no ISA IRQ; not handled");
        return;
    };
    let over = crate::acpi::interrupt_overrides()
        .into_iter()
        .find(|o| o.bus == 0 && o.source_irq == source);
    let pin = over.as_ref().map_or(u32::from(source), |o| o.gsi);
    let Some(pin8) = u8::try_from(pin)
        .ok()
        .filter(|&p| usize::from(p) < crate::ioapic::MAX_IRQ)
    else {
        serial_println!(
            "[acpi] power button: the SCI's pin {pin} is past the I/O APIC's; not handled"
        );
        return;
    };
    if over.is_none_or(|o| o.flags == 0) {
        // SAFETY: the SCI's own pin, masked still, made what ACPI says the
        // SCI is when nothing overrides it: level-triggered, active-low.
        unsafe { crate::ioapic::set_level_triggered(pin8) };
    }
    SCI_PIN.store(pin, Ordering::Release);
    // SAFETY: the pin's handler is in place (`sci_pin`, read by
    // `ioapic::handle_device_irq`), and every status that could hold it
    // asserted is cleared.
    if unsafe { crate::ioapic::unmask_irq(pin8) } {
        serial_println!(
            "[acpi] power button: handled -- the SCI (IRQ {irq}, I/O APIC pin {pin}) taken, the button's event alone enabled"
        );
    } else {
        SCI_PIN.store(u32::MAX, Ordering::Release);
        serial_println!(
            "[acpi] power button: the SCI's pin {pin} could not be routed; not handled"
        );
    }
}

/// The SCI, from `ioapic::handle_device_irq`, in interrupt context: the
/// power button's status, where set, cleared -- before the EOI, or the
/// level-triggered line would raise it again -- and the press handed to a
/// kernel worker ([`pressed`]). Takes no lock.
pub fn handle_sci() {
    let len = PM1_EVT_LEN.load(Ordering::Acquire);
    let mut pressed_now = false;
    for evt in [
        PM1A_EVT.load(Ordering::Acquire),
        PM1B_EVT.load(Ordering::Acquire),
    ] {
        if evt == 0 {
            continue;
        }
        // SAFETY: a PM1 event block [`init`] took from the FADT; reading
        // its status and writing 1 to the button's bit clears only that.
        let status = unsafe { crate::port::inw(evt) };
        let enabled = unsafe { crate::port::inw(enable_port(evt, len)) };
        if status & enabled & PWRBTN != 0 {
            // SAFETY: as above.
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
/// it is the one [`self_test`] asked for -- what the power policy says done.
/// Shutting down is done in order and ends in power-off. Sleep and
/// hibernation, which SlateOS cannot do yet, shut down instead, as the
/// press must do something; actions it cannot do and that are no way of
/// stopping (the screen off, a log-out, the lock screen) do nothing, said.
fn pressed(_arg: u64) {
    use crate::fs::power::PowerAction;
    PRESSES.fetch_add(1, Ordering::AcqRel);
    // A count, for /proc's ACPI event statistics: its failure costs only that.
    let _ = crate::fs::acpistat::record_event(crate::fs::acpistat::AcpiEvent::PowerButton);
    if TEST_EXPECTING.swap(false, Ordering::AcqRel) {
        serial_println!(
            "[acpi] power button pressed -- the self-test's press: recorded, not acted on"
        );
        TEST_SEEN.store(true, Ordering::Release);
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
            if let Err(e) = crate::initproc::shutdown(crate::initproc::ShutdownReason::PowerButton)
            {
                serial_println!(
                    "[acpi] the orderly shutdown answered {e:?}; powering off all the same"
                );
            }
            crate::power::shutdown();
        }
    }
}

/// The power button pressed by the boot test's harness and handled: the
/// kernel asks ("press it"), QEMU's `system_powerdown` raises the button's
/// event, and the press must reach [`pressed`] within [`PRESS_WAIT_MS`] --
/// recorded, not acted on. Skipped, and said, where the command line has
/// no `acpi.pwrbtn=hmp` (no harness to press it).
///
/// # Errors
///
/// `InternalError`, said, when the harness can press but the button is not
/// handled, or no press arrives.
pub fn self_test() -> KernelResult<()> {
    let harness = crate::fs::kernparam::get(HARNESS_WORD).is_some_and(|v| v == "hmp");
    if !harness {
        serial_println!(
            "[acpi] power button self-test: skipped, no harness to press it ({HARNESS_WORD}=hmp)"
        );
        return Ok(());
    }
    if sci_pin().is_none() {
        serial_println!(
            "[acpi]   FAIL: power button: not handled, so the harness's press cannot be"
        );
        return Err(KernelError::InternalError);
    }
    let before = presses();
    TEST_SEEN.store(false, Ordering::Release);
    TEST_EXPECTING.store(true, Ordering::Release);
    serial_println!("[acpi] power button: press it");
    let until = crate::hrtimer::now_ns().saturating_add(PRESS_WAIT_MS * 1_000_000);
    while !TEST_SEEN.load(Ordering::Acquire) && crate::hrtimer::now_ns() < until {
        crate::sched::sleep_ms(20);
    }
    TEST_EXPECTING.store(false, Ordering::Release);
    if TEST_SEEN.load(Ordering::Acquire) && presses() > before {
        serial_println!(
            "[acpi] power button: the harness's press raised the SCI, which took it to the worker: OK"
        );
        Ok(())
    } else {
        serial_println!(
            "[acpi]   FAIL: power button: no press reached the kernel within {PRESS_WAIT_MS} ms of asking"
        );
        Err(KernelError::InternalError)
    }
}
