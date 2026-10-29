//! The Linux driver layer's processes, as the kernel meets them
//! (design-decisions 1577, 1578, 1580).
//!
//! A Linux driver runs in a process of its own: Linux's own code, the
//! layer's C and Rust halves (`linuxdrv/`), linked into a native program by
//! `scripts/linux-tree.sh driver`. The program is GPL-2.0 as a whole and the
//! kernel is MIT, so it is not embedded in the kernel: the bootloader loads
//! it beside the kernel (a `module_path:` line in limine.conf, named by its
//! `module_cmdline:`), and the kernel finds it with [`crate::boot::module`].
//!
//! For now the kernel only tests the layer: [`self_test`] runs its first
//! process, `linuxdrv/hello`, which is no driver.

use crate::cap::{ResourceType, Rights};
use crate::error::{KernelError, KernelResult};
use crate::proc::{pcb, spawn};
use crate::serial_println;

/// The name the boot test gives the hello process (`module_cmdline:`).
const HELLO: &[u8] = b"linuxdrv/hello";
/// ... and Linux's e1000e, the layer's first real driver.
const E1000E: &[u8] = b"linuxdrv/e1000e";

/// Exit codes a driver process ends with (`SLX_EXIT_*` in
/// `linuxdrv/emul/slx.h`; `lxkit::exit`).
const EXIT_OK: i32 = 0;
const EXIT_BUG: i32 = 121;
const EXIT_DEADLOCK: i32 = 124;
const EXIT_STOP: i32 = 125;
const EXIT_INIT_FAILED: i32 = 126;

/// How a process the kernel killed for touching memory it may not ends: a
/// driver process overflowing a task's stack into the guard page below it.
const KILLED_ACCESS_VIOLATION: i32 =
    pcb::crash_exit_code(crate::proc::exception::ExceptionCode::AccessViolation as u64);

/// A driver process's program, as the kernel runs it: its image, the name
/// its process gets, and how long a run may take before it counts as hung.
struct Program<'a> {
    elf: &'a [u8],
    name: &'static str,
    limit_s: u64,
}

/// Run `program` with `argv`, holding `capabilities`, and answer how it
/// ended: its exit code, or `None` when it did not end within its limit.
fn run(
    program: &Program<'_>,
    argv: &[&[u8]],
    capabilities: &[(ResourceType, u64, Rights)],
) -> Result<Option<i32>, KernelError> {
    let options = spawn::SpawnOptions {
        name: program.name,
        parent: 0,
        priority: crate::sched::task::DEFAULT_PRIORITY,
        capabilities,
        fd_map: &[],
        argv,
        envp: &[],
        exe_path: None,
        cwd: None,
        identity: None,
    };
    let process = spawn::spawn_process(program.elf, &options)?;
    let deadline =
        crate::hrtimer::now_ns().saturating_add(program.limit_s.saturating_mul(1_000_000_000));
    let finished = spawn::wait_for_fixture(process.pid, deadline, 0);
    let code = pcb::exit_code(process.pid);
    spawn::teardown_fixture(process.pid, process.task_id);
    Ok(if finished { code } else { None })
}

/// The layer's first process, `linuxdrv/hello`: Linux's headers and its own
/// vsprintf in a process of their own. Run plainly, its initcalls allocate,
/// take a spinlock with interrupts "off", print a line formatted by Linux,
/// run a kthread against a mutex held across a sleep and a completion, try
/// the scheduler's edges, arm timers, schedule tasklets, queue work and
/// probe a device of their own through Linux's driver core, and
/// the process exits 0. With one argument the first initcall ends it otherwise:
/// "stop" calls a function nothing defines, which the build made a
/// generated stop ([`EXIT_STOP`]);
/// "deadlock" waits for what never comes ([`EXIT_DEADLOCK`]); "overflow"
/// runs past its task's stack, into the guard page below it
/// ([`KILLED_ACCESS_VIOLATION`]); "fail" starts a kthread that never ends
/// and fails, which must end the process at once ([`EXIT_INIT_FAILED`]);
/// "earlyfail" fails a core initcall of the driver's own, which must end it
/// too ([`EXIT_INIT_FAILED`]) -- a failure is carried past only when it is
/// a Linux subsystem's, whatever its level;
/// "stranded" succeeds but leaves a kthread waiting for what never comes,
/// which must end it as deadlocked ([`EXIT_DEADLOCK`]), not as done;
/// "atomic" sleeps with bottom halves off, a BUG ([`EXIT_BUG`]). Then
/// [`pci_runs`]: given the boot test's spare 82574, its driver probes it
/// through Linux's PCI core. Skipped, and recorded so, where the bootloader
/// did not load it -- anywhere but the boot test, and after a kexec.
///
/// # Errors
///
/// `InternalError` when any of the runs ends otherwise.
pub fn self_test() -> KernelResult<()> {
    let mut skips = crate::fs::selftest::Skips::new();
    let Some(elf) = crate::boot::module(HELLO) else {
        skips.record(
            "Linux driver layer's hello process",
            "not loaded beside the kernel (no limine.conf module named linuxdrv/hello)",
        );
        skips.report("[linuxdrv]");
        return Ok(());
    };
    let hello = Program {
        elf,
        name: "lx-hello",
        limit_s: 10,
    };
    let cases: [(&[&[u8]], i32, &str); 8] = [
        (&[b"hello"], EXIT_OK, "ran its initcalls and exited 0"),
        (
            &[b"hello", b"stop"],
            EXIT_STOP,
            "stopped at a generated stop",
        ),
        (
            &[b"hello", b"deadlock"],
            EXIT_DEADLOCK,
            "ended as deadlocked, its one task waiting for what never comes",
        ),
        (
            &[b"hello", b"overflow"],
            KILLED_ACCESS_VIOLATION,
            "been killed at its task stack's guard page",
        ),
        (
            &[b"hello", b"fail"],
            EXIT_INIT_FAILED,
            "exited at once on its failed initcall, its kthread still running",
        ),
        (
            &[b"hello", b"earlyfail"],
            EXIT_INIT_FAILED,
            "exited on its own failed core initcall",
        ),
        (
            &[b"hello", b"stranded"],
            EXIT_DEADLOCK,
            "ended as deadlocked, its initcalls done but a kthread waiting for what never comes",
        ),
        (
            &[b"hello", b"atomic"],
            EXIT_BUG,
            "ended as a BUG, having slept with bottom halves off",
        ),
    ];
    for (argv, want, what) in cases {
        expect(&hello, argv, &[], want, what, || None)?;
    }
    let pci = pci_runs(&hello, &mut skips)?;
    let e1000e = e1000e_run(&mut skips)?;
    skips.report("[linuxdrv]");
    serial_println!(
        "[linuxdrv]   hello process ({} bytes): Linux's headers and vsprintf in a process of their own, its initcalls run (a kthread, a mutex held across a sleep, a completion, the scheduler's edges, timers and softirqs, workqueues, the driver core), a generated stop, a deadlock, a stack overflow caught by its guard page, a failed initcall, early and late, a stranded kthread, and a sleep while atomic{}: OK",
        elf.len(),
        if pci {
            ", a PCI function probed through Linux's PCI core, its interrupts handled and its DMA done, and one no driver took"
        } else {
            ""
        }
    );
    if e1000e {
        serial_println!(
            "[linuxdrv]   e1000e process: Linux's e1000e drove the spare 82574 -- bound it, brought its link up, and had QEMU's gateway answer an ARP request through it: OK"
        );
    }
    Ok(())
}

/// The boot test's spare 82574 ([`crate::devhandle::probe_card`]) given to
/// the hello process -- an argument `pci=BB:DD.F` and a capability for that
/// function alone: "pci" must probe it through Linux's PCI core, handle the
/// interrupts it has the card raise (lx_irq.c), have it DMA, and exit 0,
/// "pcifail" must have its probe fail and so end as an initcall failure,
/// the function left without a driver. "pci" is given QEMU's edu as well,
/// after the card, where there is one no driver holds: a second function,
/// whose DMA must all be refused (the DMA window is the first's). Each exit
/// must release what it was given. Whether they ran: skipped, and recorded
/// so, where there is no such card.
fn pci_runs(hello: &Program<'_>, skips: &mut crate::fs::selftest::Skips) -> KernelResult<bool> {
    let Some(card) = crate::devhandle::probe_card() else {
        skips.record(
            "Linux driver layer's PCI runs",
            "no unbound 82574 on bus 0 with the boot test's MAC address",
        );
        return Ok(false);
    };
    let edu = crate::pci::find_device(0x1234, 0x11E8)
        .map(|d| d.address)
        .filter(|&e| crate::pci::bound_driver(e).is_none());
    if edu.is_none() {
        skips.record(
            "Linux driver layer's second-function DMA refusal",
            "no unbound QEMU edu device",
        );
    }
    let given = |with_edu: bool| core::iter::once(card.address).chain(edu.filter(|_| with_edu));
    let cases: [(&[u8], bool, i32, &str); 2] = [
        (
            b"pci",
            true,
            EXIT_OK,
            "probed the card through Linux's PCI core, handled its interrupts, had it DMA, \
             refused a second function DMA and exited 0",
        ),
        (
            b"pcifail",
            false,
            EXIT_INIT_FAILED,
            "ended as an initcall failure, its probe having failed and the card no driver's",
        ),
    ];
    for (mode, with_edu, want, what) in cases {
        let caps: alloc::vec::Vec<_> = given(with_edu)
            .map(|a| {
                (
                    ResourceType::PciDevice,
                    crate::cap::pci_device_id(a.bus, a.device, a.function),
                    Rights::WRITE,
                )
            })
            .collect();
        let args: alloc::vec::Vec<alloc::string::String> = given(with_edu)
            .map(|a| alloc::format!("pci={:02x}:{:02x}.{:x}", a.bus, a.device, a.function))
            .collect();
        let argv: alloc::vec::Vec<&[u8]> = [b"hello".as_slice(), mode]
            .into_iter()
            .chain(args.iter().map(alloc::string::String::as_bytes))
            .collect();
        expect(hello, &argv, &caps, want, what, || {
            (!crate::devhandle::yield_until(|| {
                given(with_edu).all(|a| crate::pci::bound_driver(a).is_none())
            }))
            .then_some("a function still bound after it")
        })?;
    }
    Ok(true)
}

/// The layer's first real driver, Linux's own e1000e (`linuxdrv/e1000e`),
/// given the boot test's spare 82574 as hello's PCI runs are, in its "arp"
/// mode: it must bind the card, bring its link up, ask QEMU's user-network
/// gateway for its address and have the answer -- a frame sent and one
/// received through Linux's driver, its interrupts and its DMA -- then exit
/// 0, the card released. Whether it ran: skipped, and recorded so, where
/// the bootloader did not load it or there is no such card.
fn e1000e_run(skips: &mut crate::fs::selftest::Skips) -> KernelResult<bool> {
    let Some(elf) = crate::boot::module(E1000E) else {
        skips.record(
            "Linux's e1000e through the driver layer",
            "not loaded beside the kernel (no limine.conf module named linuxdrv/e1000e)",
        );
        return Ok(false);
    };
    let Some(card) = crate::devhandle::probe_card() else {
        skips.record(
            "Linux's e1000e through the driver layer",
            "no unbound 82574 on bus 0 with the boot test's MAC address",
        );
        return Ok(false);
    };
    let a = card.address;
    let caps = [(
        ResourceType::PciDevice,
        crate::cap::pci_device_id(a.bus, a.device, a.function),
        Rights::WRITE,
    )];
    let arg = alloc::format!("pci={:02x}:{:02x}.{:x}", a.bus, a.device, a.function);
    let program = Program {
        elf,
        name: "lx-e1000e",
        limit_s: 30,
    };
    expect(
        &program,
        &[b"e1000e", b"arp", arg.as_bytes()],
        &caps,
        EXIT_OK,
        "bound the card with Linux's e1000e, brought its link up, had QEMU's gateway answer \
         an ARP request through it, and exited 0",
        || {
            (!crate::devhandle::yield_until(|| crate::pci::bound_driver(a).is_none()))
                .then_some("the card still bound after it")
        },
    )?;
    Ok(true)
}

/// One run of a driver process -- `argv`, holding `capabilities` -- which
/// should have ended with `want`, as `what` says, and passed `after`: `None`
/// when what it left is right, else what is wrong. Said and an
/// `InternalError` otherwise.
fn expect(
    program: &Program<'_>,
    argv: &[&[u8]],
    capabilities: &[(ResourceType, u64, Rights)],
    want: i32,
    what: &str,
    after: impl FnOnce() -> Option<&'static str>,
) -> KernelResult<()> {
    let outcome = run(program, argv, capabilities);
    let wrong = after();
    match (outcome, wrong) {
        (Ok(Some(code)), None) if code == want => Ok(()),
        (outcome, wrong) => {
            fn text<'a>(a: &&'a [u8]) -> &'a str {
                core::str::from_utf8(a).unwrap_or("?")
            }
            serial_println!(
                "[linuxdrv]   FAIL: the {} process {:?}, which should have {}, ended {:?} (Ok(Some(code)), Ok(None) for not within {} s){}{}",
                argv.first().map_or("?", text),
                argv.get(1..)
                    .unwrap_or(&[])
                    .iter()
                    .map(text)
                    .collect::<alloc::vec::Vec<_>>(),
                what,
                outcome,
                program.limit_s,
                if wrong.is_some() { ", " } else { "" },
                wrong.unwrap_or("")
            );
            Err(KernelError::InternalError)
        }
    }
}
