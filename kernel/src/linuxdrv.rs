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
//! process, `linuxdrv/hello`, which is no driver, then Linux's e1000e on the
//! boot test's spare 82574, serving the card to the netstack daemon
//! (design-decisions 1589).

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
/// A network driver's serving of one stack in which Linux code warned: a
/// bug, though the run otherwise passed.
const EXIT_WARNED: i32 = 119;
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

/// `program` started with `argv`, holding `capabilities`.
fn start(
    program: &Program<'_>,
    argv: &[&[u8]],
    capabilities: &[(ResourceType, u64, Rights)],
) -> KernelResult<spawn::SpawnResult> {
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
    spawn::spawn_process(program.elf, &options)
}

/// How `process`, started from `program`, ended within `program`'s limit
/// from now: its exit code, or `None` when it did not. Torn down either way.
fn finish(program: &Program<'_>, process: &spawn::SpawnResult) -> Option<i32> {
    let deadline =
        crate::hrtimer::now_ns().saturating_add(program.limit_s.saturating_mul(1_000_000_000));
    let finished = spawn::wait_for_fixture(process.pid, deadline, 0);
    let code = pcb::exit_code(process.pid);
    spawn::teardown_fixture(process.pid, process.task_id);
    if finished { code } else { None }
}

/// Run `program` with `argv`, holding `capabilities`, and answer how it
/// ended: its exit code, or `None` when it did not end within its limit.
fn run(
    program: &Program<'_>,
    argv: &[&[u8]],
    capabilities: &[(ResourceType, u64, Rights)],
) -> Result<Option<i32>, KernelError> {
    let process = start(program, argv, capabilities)?;
    Ok(finish(program, &process))
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
            "[linuxdrv]   e1000e process: Linux's e1000e bound the spare 82574, brought its link up and served it to the network stack -- turning away a daemon without the card's key -- whose daemon attached to it, took an address by DHCP through it, resolved the gateway and had 400 full-size pings answered 16 at a time, payloads as sent, the driver warning nowhere: OK"
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
/// given the boot test's spare 82574 as hello's PCI runs are, serving it to
/// the network stack ([`e1000e_serve`]). Whether it ran: skipped, and
/// recorded so, where the bootloader did not load it or there is no such
/// card.
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
    e1000e_serve(&program, &caps, a, &arg)?;
    Ok(true)
}

/// The service the boot test's card is served as: `serve=spare`.
const SERVICE: &[u8] = b"nic.spare";
/// What the netstack daemon's `nic-check` exits with when it cannot attach
/// to the card -- as it must not, without the card's key.
const NIC_NO_CARD: i32 = 10;

/// What the netstack daemon's `nic-check` exits with (`services/netstack`):
/// what failed.
fn nic_check_failure(code: i32) -> &'static str {
    match code {
        0 => "it passed",
        NIC_NO_CARD => "it could not attach to the card",
        11 => "the kernel let it pass on a region it was never granted",
        12 => "it took no address by DHCP",
        13 => "the gateway did not answer its ARP requests",
        14 => {
            "its pings went wrong: unanswered, answered otherwise or twice, sent again too often, or waited for with no doorbell"
        }
        _ => "it ended otherwise",
    }
}

/// Linux's e1000e serving the spare 82574 to the network stack
/// (design-decisions 1589): the driver process -- `program`, holding
/// `capabilities` (the card's) and a `Service` capability for [`SERVICE`]
/// alone, the card at `a`, `pci` its argument naming it -- in its
/// "serve=spare once" mode, and beside it the netstack daemon in its
/// "nic-check nic.spare" mode -- first without the card's key, when the
/// driver must turn it away (it exits [`NIC_NO_CARD`]), then holding it,
/// when it must attach to the
/// card, be refused passing on a region it was never granted, take an
/// address by DHCP from QEMU's network through it, resolve the gateway by
/// ARP, have 400 full-size pings answered 16 at a time and 8 more sent to an
/// idle driver, payloads as sent, and exit 0; the driver then ends, the
/// daemon gone, with 0, no warning from Linux code in it, the card released.
fn e1000e_serve(
    program: &Program<'_>,
    capabilities: &[(ResourceType, u64, Rights)],
    a: crate::pci::PciAddress,
    pci: &str,
) -> KernelResult<()> {
    // The driver may register its card's name and no other; the daemon
    // holds the card's key, which the driver asks of whoever connects.
    let service = crate::ipc::service::key_id(SERVICE);
    let mut caps = alloc::vec::Vec::from(capabilities);
    caps.push((ResourceType::Service, service, Rights::WRITE));
    let driver = start(
        program,
        &[b"e1000e", b"serve=spare", b"once", pci.as_bytes()],
        &caps,
    )?;
    let stack = Program {
        elf: spawn::NETSTACK_ELF,
        name: "netstack-nic",
        limit_s: 90,
    };
    // The daemon run first without the card's key, then with it.
    let check = |key: &[(ResourceType, u64, Rights)]| {
        let daemon = start(&stack, &[b"netstack", b"nic-check", SERVICE], key);
        daemon.map(|d| finish(&stack, &d))
    };
    let keyless = check(&[]);
    if keyless != Ok(Some(NIC_NO_CARD)) {
        serial_println!(
            "[linuxdrv]   FAIL: the netstack daemon without the card's key ended {:?}: it should have been turned away, exit {}",
            keyless,
            NIC_NO_CARD
        );
        // The driver waits for a stack that will not come, or served the
        // wrong one: no use waiting out its limit.
        spawn::teardown_fixture(driver.pid, driver.task_id);
        return Err(KernelError::InternalError);
    }
    let checked = match check(&[(ResourceType::Service, service, Rights::READ)]) {
        Ok(checked) => checked,
        Err(e) => {
            serial_println!(
                "[linuxdrv]   FAIL: the netstack daemon could not be started: {:?}",
                e
            );
            spawn::teardown_fixture(driver.pid, driver.task_id);
            return Err(e);
        }
    };
    let served = finish(program, &driver);
    let warned = served == Some(EXIT_WARNED);
    let released = crate::devhandle::yield_until(|| crate::pci::bound_driver(a).is_none());
    if checked == Some(EXIT_OK) && served == Some(EXIT_OK) && released {
        return Ok(());
    }
    serial_println!(
        "[linuxdrv]   FAIL: the e1000e process serving the card to the network stack ended {:?}, the netstack daemon checking it {:?} ({}) (an exit code, or None for not within {} s / {} s){}",
        served,
        checked,
        checked.map_or("it did not end", nic_check_failure),
        program.limit_s,
        stack.limit_s,
        if released {
            ""
        } else {
            ", the card still bound after them"
        }
    );
    if warned {
        serial_println!(
            "[linuxdrv]   ... the e1000e process ended so because Linux code warned in it (its log says where)"
        );
    }
    Err(KernelError::InternalError)
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
