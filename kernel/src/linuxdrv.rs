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
//! (design-decisions 1589), which also times it; then the daemon times the
//! kernel's own e1000 driver the same way, for the two to be read side by
//! side (design-decisions 1588, measured).

use netipc::nicport::check;

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
/// An argument naming a module parameter the process does not have, or
/// giving one a value its kind refuses.
const EXIT_ARGUMENT: i32 = 118;
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
    /// How often its end is looked for: 0 after every yield of the CPU,
    /// else that many milliseconds' sleep between looks.
    poll_ms: u64,
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
    let finished = spawn::wait_for_fixture(process.pid, deadline, program.poll_ms);
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
/// [`firmware_runs`]: module parameters set from its arguments, and
/// firmware read through Linux's own loader. Then
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
        poll_ms: 0,
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
    firmware_runs(&hello, &mut skips)?;
    let pci = pci_runs(&hello, &mut skips)?;
    let e1000e = e1000e_run(&mut skips)?;
    skips.report("[linuxdrv]");
    serial_println!(
        "[linuxdrv]   hello process ({} bytes): Linux's headers and vsprintf in a process of their own, its initcalls run (a kthread, a mutex held across a sleep, a completion, the scheduler's edges, timers and softirqs, workqueues, the driver core), a generated stop, a deadlock, a stack overflow caught by its guard page, a failed initcall, early and late, a stranded kthread, a sleep while atomic, module parameters set from its arguments -- an unknown one and a refused value ending it -- and Linux's firmware loader reading files, none without the capability to{}: OK",
        elf.len(),
        if pci {
            ", a PCI function probed through Linux's PCI core, its interrupts handled and its DMA done, and one no driver took"
        } else {
            ""
        }
    );
    if e1000e {
        serial_println!(
            "[linuxdrv]   e1000e process: Linux's e1000e bound the spare 82574, brought its link up and served it to the network stack -- refusing a daemon without the card's key, for that reason -- whose daemon attached to it, took an address by DHCP through it, resolved the gateway and had 400 full-size pings answered 16 at a time, payloads as sent, was told the link, found multicast passing as listed{}, timed the card -- then, unless skipped above, the kernel's own e1000 the same way -- and was let go for breaking the protocol; then a daemon served sockets over the card, a name asked through it answered; the driver warning nowhere: OK",
            if cable_puller() {
                ", the link following the card's cable -- a ping sent while it was down dropped, never sent later; the lease confirmed by INIT-REBOOT, the addresses announced and the lease renewed when it came back -- and the serving daemon's lease confirmed after a pull of its own"
            } else {
                ""
            }
        );
    }
    Ok(())
}

/// Where [`firmware_runs`] puts the hello process's firmware.
const FIRMWARE_DIR: &str = "/tmp/linuxdrv-fw";
/// A directory with a size of its own, for [`firmware_runs`] to have read
/// as a file: the rootfs's ext4, where the boot test mounts it.
const SIZED_DIR: &str = "/mnt";
/// The test firmware's size: more than one `SYS_FS_PREAD` gives, so that a
/// whole file takes several.
const FIRMWARE_BYTES: usize = 70_001;

/// The test firmware's `i`-th byte, as `hello_fw_byte()` in
/// `linuxdrv/hello/hello.c` has it.
#[allow(clippy::cast_possible_truncation)] // Truncation to a byte is the formula.
fn firmware_byte(i: usize) -> u8 {
    i.wrapping_mul(7).wrapping_add(i >> 8) as u8
}

/// Module parameters and firmware: the hello process's "firmware" run,
/// given a File capability to read with, the directory [`FIRMWARE_DIR`]
/// written for it (`slx-test.fw`, `slx-empty.fw`), a directory to read as
/// a file ([`SIZED_DIR`]; where it has no size of its own, `slx-dir.fw`,
/// recorded as a skip) and its own parameters, must read through the layer
/// and Linux's firmware loader what was written (`hello_firmware_init`) and
/// exit 0; its "firmware-denied" run, given no such capability, must read
/// nothing; an argument naming a parameter hello lacks, or giving one a
/// value its kind refuses, must end the process before any initcall
/// ([`EXIT_ARGUMENT`]). The directory is removed whatever the runs found.
fn firmware_runs(hello: &Program<'_>, skips: &mut crate::fs::selftest::Skips) -> KernelResult<()> {
    use crate::fs::vfs::Vfs;
    let file = |name: &str| alloc::format!("{FIRMWARE_DIR}/{name}");
    // Nothing should be there; whatever is, is a fixture's leftovers, and
    // the mkdir below refuses what this does not remove.
    let _ = Vfs::remove_recursive(FIRMWARE_DIR);
    let written = (|| {
        Vfs::mkdir(FIRMWARE_DIR)?;
        let firmware: alloc::vec::Vec<u8> = (0..FIRMWARE_BYTES).map(firmware_byte).collect();
        Vfs::write_file(file("slx-test.fw").as_str(), &firmware)?;
        Vfs::write_file(file("slx-empty.fw").as_str(), &[])?;
        Vfs::mkdir(file("slx-dir.fw").as_str())
    })();
    // A directory read as a file is refused for not being a regular file,
    // which only one whose size is not 0 shows: an empty file is refused
    // too, and memfs gives every directory the size 0. ext4 gives one its
    // blocks' -- the rootfs's, at /mnt in the boot test.
    let sized = Vfs::stat(SIZED_DIR)
        .is_ok_and(|e| e.entry_type == crate::fs::vfs::EntryType::Directory && e.size > 0);
    if !sized {
        skips.record(
            "Linux driver layer's refusal of a directory read as a file",
            "no directory with a size of its own at /mnt (no ext4 rootfs); a sizeless one tried",
        );
    }
    let dir_arg = alloc::format!(
        "dir={}",
        if sized {
            alloc::string::String::from(SIZED_DIR)
        } else {
            file("slx-dir.fw")
        }
    );
    let path_arg = alloc::format!("firmware_class.path={FIRMWARE_DIR}");
    let path_arg = path_arg.as_bytes();
    let read = [(ResourceType::File, 0, Rights::READ)];
    let ran = written.and_then(|()| {
        expect(
            hello,
            &[
                b"hello",
                b"firmware",
                path_arg,
                b"hello.answer=42",
                b"hello.motto=two words",
                b"hello.device_level=6",
                dir_arg.as_bytes(),
            ],
            &read,
            EXIT_OK,
            "set its parameters, read files through the layer and loaded firmware through Linux's loader",
            || None,
        )?;
        expect(
            hello,
            &[b"hello", b"firmware-denied", path_arg],
            &[],
            EXIT_OK,
            "read no firmware, having no capability to read files",
            || None,
        )?;
        expect(
            hello,
            &[b"hello", b"hello.nonesuch=1"],
            &[],
            EXIT_ARGUMENT,
            "ended on a parameter it does not have",
            || None,
        )?;
        expect(
            hello,
            &[b"hello", b"hello.answer=forty"],
            &[],
            EXIT_ARGUMENT,
            "ended on a value its parameter's kind refuses",
            || None,
        )
    });
    // The fixture's own: a removal that fails leaves files in /tmp, which
    // the next boot does not see, and the runs' verdict stands either way.
    let _ = Vfs::remove_recursive(FIRMWARE_DIR);
    if let Err(e) = &ran {
        serial_println!(
            "[linuxdrv]   firmware runs: {e:?} (the fixture in {FIRMWARE_DIR} written: {written:?})"
        );
    }
    ran
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
/// the network stack ([`e1000e_serve`]); then the kernel's own e1000 timed
/// as the daemon timed that card ([`native_bench`]). Whether it ran:
/// skipped, and recorded so, where the bootloader did not load it or there
/// is no such card.
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
        poll_ms: 0,
    };
    e1000e_serve(&program, &caps, a, &arg)?;
    native_bench(skips)?;
    Ok(true)
}

/// The kernel's own e1000 driver timed by the netstack daemon's `nic-bench`
/// -- the first 82574, claimed alone by its MAC address (the kernel's stack
/// keeping its primary card), an address taken on it by DHCP -- as
/// `nic-check` has just timed Linux's e1000e serving the spare 82574: the
/// same model, behind the same kind of QEMU network, its native twin. The
/// two `[bench] PROCESS nic_...` sets are read side by side
/// (design-decisions 1588, measured). After the e1000e run, which it cannot
/// keep from running; it fails only for what is wrong, not for being slow:
/// the card not claimed or not passing frames, an answer corrupted.
/// Skipped, and recorded so, where the kernel drives no e1000, or where its
/// e1000 is its primary card -- no virtio-net before it.
fn native_bench(skips: &mut crate::fs::selftest::Skips) -> KernelResult<()> {
    let Some(mac) = crate::e1000::with_device(|d| d.mac().0) else {
        skips.record(
            "the kernel's e1000 timed beside Linux's e1000e",
            "the kernel drives no e1000",
        );
        return Ok(());
    };
    if crate::net::raw::Card::alone_with_mac(mac).is_err() {
        skips.record(
            "the kernel's e1000 timed beside Linux's e1000e",
            "the e1000 is the kernel's primary card, which is not claimed alone",
        );
        return Ok(());
    }
    let hex = alloc::format!(
        "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
    let program = Program {
        elf: spawn::NETSTACK_ELF,
        name: "netstack-bench",
        limit_s: 60,
        poll_ms: DAEMON_POLL_MS,
    };
    let ran = run(
        &program,
        &[b"netstack", b"nic-bench", hex.as_bytes(), b"kernel_e1000"],
        &[(ResourceType::NetRaw, 0, Rights::WRITE)],
    );
    if ran == Ok(Some(check::OK)) {
        return Ok(());
    }
    serial_println!(
        "[linuxdrv]   FAIL: the netstack daemon timing the kernel's e1000 ended {:?} ({})",
        ran,
        nic_check_outcome(&ran)
    );
    Err(KernelError::InternalError)
}

/// How often the kernel looks whether a netstack daemon it runs has ended:
/// it sleeps between looks rather than yielding, so that on one CPU the
/// daemon's timings are not shared with a boot thread always runnable.
const DAEMON_POLL_MS: u64 = 10;

/// The service the boot test's card is served as: `serve=spare`.
const SERVICE: &[u8] = b"nic.spare";

/// How a run of the netstack daemon's `nic-check` went, for a FAIL line:
/// what its exit code says (`netipc::nicport::check`), or that it did not
/// end, or could not be started.
fn nic_check_outcome(run: &KernelResult<Option<i32>>) -> &'static str {
    match run {
        Ok(Some(code)) => check::failure(*code),
        Ok(None) => "it did not end within its limit",
        Err(_) => "it could not be started",
    }
}

/// Whether the harness pulls the spare card's cable when the netstack
/// daemon asks (`linuxdrv.cable=hmp`: the boot test, through QEMU's
/// monitor, at the daemon's line "cable: pull it").
fn cable_puller() -> bool {
    crate::fs::kernparam::get("linuxdrv.cable").is_some_and(|v| v == "hmp")
}

/// Linux's e1000e serving the spare 82574 to the network stack
/// (design-decisions 1589): the driver process -- `program`, holding
/// `capabilities` (the card's) and a `Service` capability for [`SERVICE`]
/// alone, the card at `a`, `pci` its argument naming it -- in its
/// "serve=spare stacks=2" mode, and beside it the netstack daemon in its
/// "nic-check nic.spare" mode -- first without the card's key, when the
/// driver must refuse the daemon for want of it, the daemon then exiting
/// [`check::TURNED_AWAY`]; then holding it, when it
/// must attach to the card, be refused passing on a region it was never
/// granted, take an
/// address by DHCP from QEMU's network through it, resolve the gateway by
/// ARP, have 400 full-size pings answered 16 at a time and 8 more sent to an
/// idle driver, payloads as sent, be told the card's link is up, find the
/// card passing a multicast group exactly while it lists it, and -- where
/// the harness can pull the card's cable ([`cable_puller`]) -- be told the
/// link down and up again as it does, a send refused at once while it is
/// down, its lease confirmed and its addresses announced once it is back, time the card (`nic-bench`'s pings,
/// failing only for too many lost or one corrupted), be let go for breaking
/// the protocol; and exit 0. Then the daemon serves sockets over the card
/// ([`serve_sockets`]), the driver's second stack. The driver then ends,
/// both daemons gone, with 0, no warning from Linux code in it, the card
/// released.
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
        &[b"e1000e", b"serve=spare", b"stacks=2", pci.as_bytes()],
        &caps,
    )?;
    // The daemon run first without the card's key, then with it. Refused, it
    // needs only to connect -- once the driver has probed the card and its
    // link is up, seconds under emulation; holding the key, its checks and
    // its timing take a minute at most, and the cable's two waits 20 s each.
    let plain: &[&[u8]] = &[b"netstack", b"nic-check", SERVICE];
    let with_cable: &[&[u8]] = &[b"netstack", b"nic-check", SERVICE, b"cable"];
    let cable = cable_puller();
    let netstack = |limit_s| Program {
        elf: spawn::NETSTACK_ELF,
        name: "netstack-nic",
        limit_s,
        poll_ms: DAEMON_POLL_MS,
    };
    let keyless_stack = netstack(60);
    let stack = netstack(if cable { 150 } else { 90 });
    let run_daemon = |stack: &Program<'_>, args: &[&[u8]], key: &[(ResourceType, u64, Rights)]| {
        let daemon = start(stack, args, key);
        daemon.map(|d| finish(stack, &d))
    };
    let keyed = if cable { with_cable } else { plain };
    let keyless = run_daemon(&keyless_stack, plain, &[]);
    if keyless != Ok(Some(check::TURNED_AWAY)) {
        serial_println!(
            "[linuxdrv]   FAIL: the netstack daemon without the card's key ended {:?} ({}): it should have been turned away, exit {}",
            keyless,
            nic_check_outcome(&keyless),
            check::TURNED_AWAY
        );
        // The driver waits for a stack that will not come, or served the
        // wrong one: no use waiting out its limit.
        spawn::teardown_fixture(driver.pid, driver.task_id);
        return Err(KernelError::InternalError);
    }
    let checked = match run_daemon(
        &stack,
        keyed,
        &[(ResourceType::Service, service, Rights::READ)],
    ) {
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
    // Then sockets served over the card: the driver's second stack. A driver
    // whose stacks did not both come and go waits for one: not for its limit.
    let sockets = checked == Some(check::OK) && serve_sockets(service);
    let served = if sockets {
        finish(program, &driver)
    } else {
        stop(&driver)
    };
    let warned = served == Some(EXIT_WARNED);
    let released = crate::devhandle::yield_until(|| crate::pci::bound_driver(a).is_none());
    if sockets && served == Some(EXIT_OK) && released {
        return Ok(());
    }
    serial_println!(
        "[linuxdrv]   FAIL: the e1000e process serving the card to the network stack ended {:?}, the netstack daemon checking it {:?} ({}), sockets served over it {} (an exit code; None: not within {} s / {} s, or stopped at once, its stacks not both come and gone){}",
        served,
        checked,
        nic_check_outcome(&Ok(checked)),
        if sockets {
            "as they should be"
        } else {
            "not, or not as they should be"
        },
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

/// Sockets served over the card: the netstack daemon as `serve-dns
/// nic.spare` -- the driver's second stack -- holding the card's key
/// (`card_key`) and the right to register `net.stack`. Once it has, attached
/// and given an address by DHCP on the card, the kernel asks it to resolve a
/// name, as a socket client asks, and it must answer: the name resolved, or
/// not -- the boot test's host may have no upstream DNS, which is said, the
/// card's frames having been proved by `nic-check` already. Where the
/// harness can pull the card's cable ([`cable_puller`]) it is pulled and
/// plugged back under the daemon, asked all the while ([`keep_asking`]),
/// which must then confirm its lease and announce its addresses. Idle after,
/// the daemon lets the card go and ends 0 -- not `check::LEASE`, its exit when
/// a link came back and its lease was never confirmed. Whether all of that
/// held; what did not is said.
fn serve_sockets(card_key: u64) -> bool {
    let program = Program {
        elf: spawn::NETSTACK_ELF,
        name: "netstack-serve",
        limit_s: 60,
        poll_ms: DAEMON_POLL_MS,
    };
    let caps = [
        (ResourceType::Service, card_key, Rights::READ),
        (
            ResourceType::Service,
            crate::ipc::service::key_id(b"net.stack"),
            Rights::WRITE,
        ),
    ];
    // With the cable to be pulled under it, the daemon is told to expect its
    // link back and to fail unless its lease is then confirmed.
    let relinking = cable_puller();
    let plain: &[&[u8]] = &[b"netstack", b"serve-dns", SERVICE];
    let relink: &[&[u8]] = &[b"netstack", b"serve-dns", SERVICE, b"relink"];
    let daemon = match start(&program, if relinking { relink } else { plain }, &caps) {
        Ok(daemon) => daemon,
        Err(e) => {
            serial_println!(
                "[linuxdrv]   FAIL: the netstack daemon serving sockets could not be started: {:?}",
                e
            );
            return false;
        }
    };
    // Attached, an address taken and `net.stack` registered: seconds, the
    // card being up already.
    let deadline = crate::hrtimer::now_ns().saturating_add(40_000_000_000);
    let registered = || crate::ipc::service::is_registered(b"net.stack");
    while !registered()
        && crate::hrtimer::now_ns() < deadline
        && pcb::state(daemon.pid) != Some(pcb::ProcessState::Zombie)
    {
        crate::sched::sleep_ms(DAEMON_POLL_MS);
    }
    let answer = registered().then(|| spawn::netstack_resolve_a(b"example.com"));
    // Where the harness can pull the card's cable, it is pulled and plugged
    // back under the serving daemon, which is asked all the while -- kept
    // from idling out -- and must, once the link is back, confirm its lease
    // and announce its addresses, its exit saying whether it did.
    if relinking && answer.is_some() {
        serial_println!("[linuxdrv]   cable: pull it");
        keep_asking(5_000_000_000);
        serial_println!("[linuxdrv]   cable: plug it back");
        keep_asking(10_000_000_000);
    }
    let ended = finish(&program, &daemon);
    match (answer, ended) {
        (Some(Ok(ip)), Some(0)) => {
            match ip {
                Some(ip) => serial_println!(
                    "[linuxdrv]   sockets served over the card: example.com resolved to {}.{}.{}.{} through it",
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                ),
                None => serial_println!(
                    "[linuxdrv]   sockets served over the card: example.com answered unresolved (no upstream DNS behind the emulator?)"
                ),
            }
            true
        }
        (answer, ended) => {
            serial_println!(
                "[linuxdrv]   FAIL: the netstack daemon serving sockets over the card answered {:?} (None: it never registered net.stack) and ended {:?} ({})",
                answer,
                ended,
                nic_check_outcome(&Ok(ended))
            );
            false
        }
    }
}

/// The netstack daemon serving `net.stack` asked to resolve a name every
/// half second for `ns`, the answers not looked at: what keeps a bounded
/// daemon from idling out while its card is being done to.
fn keep_asking(ns: u64) {
    let until = crate::hrtimer::now_ns().saturating_add(ns);
    while crate::hrtimer::now_ns() < until {
        // Only that it is asked matters here; the answer may well fail with
        // the cable out.
        let _ = spawn::netstack_resolve_a(b"example.com");
        crate::sched::sleep_ms(500);
    }
}

/// `driver` stopped: its exit code if it had ended by itself, `None` when it
/// had not and was torn down.
fn stop(driver: &spawn::SpawnResult) -> Option<i32> {
    let code = (pcb::state(driver.pid) == Some(pcb::ProcessState::Zombie))
        .then(|| pcb::exit_code(driver.pid))
        .flatten();
    spawn::teardown_fixture(driver.pid, driver.task_id);
    code
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
