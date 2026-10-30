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
//! boot test's spare 82574 -- or on the card `linuxdrv.card` names, on a real
//! network (the test PC's I219-V) -- serving the card to the netstack daemon
//! (design-decisions 1589), which also times it; then the daemon times the
//! kernel's own e1000 driver the same way, for the two to be read side by
//! side (design-decisions 1588, measured). Then Linux's 802.11 stack: its
//! mt7925e, started with no card or scanning with the one `linuxdrv.wifi`
//! names (the test PC's RZ717), and its mac80211_hwsim, whose simulated
//! radios test the stack end to end where there is no card (design-decisions
//! 1593). The boot medium's firmware is placed where every driver process is
//! told to look ([`mount_boot_firmware`], design-decisions 1592).

use netipc::nicport::check;

use crate::cap::{ResourceType, Rights};
use crate::error::{KernelError, KernelResult};
use crate::proc::{pcb, spawn};
use crate::serial_println;

/// The name the boot test gives the hello process (`module_cmdline:`).
const HELLO: &[u8] = b"linuxdrv/hello";
/// ... and Linux's e1000e, the layer's first real driver.
const E1000E: &[u8] = b"linuxdrv/e1000e";
/// ... and Linux's mt7925e, its first WiFi driver, with Linux's 802.11
/// stack (cfg80211, mac80211).
const MT7925E: &[u8] = b"linuxdrv/mt7925e";
/// ... and Linux's mac80211_hwsim, simulated radios on the same stack.
const HWSIM: &[u8] = b"linuxdrv/hwsim";

/// Exit codes a driver process ends with (`SLX_EXIT_*` in
/// `linuxdrv/emul/slx.h`; `lxkit::exit`).
const EXIT_OK: i32 = 0;
/// An argument naming a module parameter the process does not have,
/// giving one a value its kind refuses, or holding both a space and a
/// quote.
const EXIT_ARGUMENT: i32 = 118;
/// A run in which Linux code warned -- a network driver's serving of one
/// stack, or a WiFi run (`lx_end_run` in `linuxdrv/emul/lx_emul.c`): a
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

/// What a driver process's exit `code` says of how it ended, for a FAIL
/// line to say beside the number: `None` for a code with no meaning of its
/// own (0, or one nothing here gives).
const fn exit_meaning(code: i32) -> Option<&'static str> {
    match code {
        EXIT_ARGUMENT => Some("it refused an argument"),
        EXIT_WARNED => Some("Linux code warned in it (its log says where)"),
        EXIT_BUG => Some("Linux code hit a BUG in it (its log says where)"),
        EXIT_DEADLOCK => Some("its tasks were all waiting for what never comes"),
        EXIT_STOP => Some("it called a function the layer does not have (its log names it)"),
        EXIT_INIT_FAILED => Some("an initcall failed (its log says which)"),
        KILLED_ACCESS_VIOLATION => Some("the kernel killed it for touching memory it may not"),
        _ => None,
    }
}

/// A driver process's program, as the kernel runs it: its image, the name
/// its process gets, and how long a run may take before it counts as hung.
struct Program<'a> {
    elf: &'a [u8],
    name: &'static str,
    /// A Linux driver process -- told where the boot medium's firmware is
    /// ([`start`]) -- rather than one of the system's own (the netstack
    /// daemon), which takes no such argument.
    driver: bool,
    limit_s: u64,
    /// How often its end is looked for: 0 after every yield of the CPU,
    /// else that many milliseconds' sleep between looks.
    poll_ms: u64,
}

/// `program` started with `argv`, holding `capabilities` -- and, a driver
/// process once the boot medium's firmware is placed
/// ([`BOOT_FIRMWARE_PLACED`]), told where
/// (`firmware_class.path=`[`BOOT_FIRMWARE_DIR`]), unless `argv` names a
/// directory of its own.
fn start(
    program: &Program<'_>,
    argv: &[&[u8]],
    capabilities: &[(ResourceType, u64, Rights)],
) -> KernelResult<spawn::SpawnResult> {
    const PATH_KEY: &[u8] = b"firmware_class.path=";
    // Outlives `args`, which borrows it; made only when it is passed.
    let path_arg;
    let mut args = argv.to_vec();
    if program.driver
        && BOOT_FIRMWARE_PLACED.load(core::sync::atomic::Ordering::Acquire)
        && !argv.iter().any(|a| a.starts_with(PATH_KEY))
    {
        path_arg = alloc::format!("firmware_class.path={BOOT_FIRMWARE_DIR}");
        args.push(path_arg.as_bytes());
    }
    let options = spawn::SpawnOptions {
        name: program.name,
        parent: 0,
        priority: crate::sched::task::DEFAULT_PRIORITY,
        capabilities,
        fd_map: &[],
        argv: &args,
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

/// The Linux driver layer, tested through its processes. First the checks
/// that need no process: the operator's options (`linuxdrv.card`,
/// `linuxdrv.wifi`, and `linuxdrv.serve` as [`serve_at_boot`] reads it) read
/// and their cards and processes found, and firmware's paths and placing
/// ([`check_place_firmware`]). Then each
/// process's runs, which stand alone: one the bootloader did not load --
/// anywhere but the boot test, and after a kexec -- skips its own runs,
/// recorded so, and never another's.
///
/// - [`hello_runs`]: the layer's own test process, `linuxdrv/hello`, no
///   driver -- Linux's headers and vsprintf in a process of their own, its
///   initcalls run plainly and in each way that must end it otherwise; then
///   module parameters and firmware ([`firmware_runs`]), the boot medium's
///   firmware ([`boot_firmware_run`]), and the boot test's spare 82574
///   probed through Linux's PCI core ([`pci_runs`]).
/// - [`e1000e_run`]: Linux's e1000e on that card -- or on the one
///   `linuxdrv.card` names -- serving it to the netstack daemon.
/// - [`wifi_runs`]: Linux's mt7925e with its 802.11 stack, started with no
///   card, or scanning with the one `linuxdrv.wifi` names.
/// - [`hwsim_run`]: Linux's mac80211_hwsim, two simulated radios on that
///   stack, one made an access point the other must find, join and pass a
///   frame each way across -- open, then protected by WPA2.
///
/// Then the verdicts of what ran, one line a process, which the boot test
/// requires of each process it put on the medium (`scripts/boot-test.sh`,
/// `check_linuxdrv_ran`).
///
/// # Errors
///
/// `InternalError` when any of the runs ends otherwise; `InvalidArgument`
/// when `linuxdrv.card` or `linuxdrv.wifi` asks for a run this boot cannot
/// make ([`named_card`]).
pub fn self_test() -> KernelResult<()> {
    check_parse_pci_address()?;
    check_parse_serve()?;
    check_card_choice()?;
    check_firmware_module_path()?;
    // An operator's run, asked for by name: refused before the hello runs
    // for a card it cannot have or a driver process not loaded -- never
    // skipped, which would read as the card's absence.
    let named = named_card(CardKind::IntelEthernet)?;
    let named_wifi = named_card(CardKind::MediatekWifi)?;
    // The card the operator has a driver process serve for the system's life
    // (`linuxdrv.serve`): refused here as the others are, not only warned of
    // when it is started past the self-tests.
    served_card()?;
    let needs: [(bool, &str, &[u8]); 2] = [
        (named.is_some(), "linuxdrv/e1000e", E1000E),
        (named_wifi.is_some(), "linuxdrv/mt7925e", MT7925E),
    ];
    for (asked, name, module) in needs {
        if asked && crate::boot::module(module).is_none() {
            serial_println!(
                "[linuxdrv]   FAIL: a card is named for {name}, but it is not loaded beside the kernel"
            );
            return Err(KernelError::InvalidArgument);
        }
    }
    let mut skips = crate::fs::selftest::Skips::new();
    check_place_firmware(&mut skips)?;
    // Each driver process's runs on their own: one not loaded is a skip of
    // its runs alone, never of another's.
    let hello = hello_runs(&mut skips)?;
    let e1000e = e1000e_run(&mut skips, named)?;
    let wifi = wifi_runs(&mut skips, named_wifi)?;
    let hwsim = hwsim_run(&mut skips)?;
    skips.report("[linuxdrv]");
    if let Some(ran) = hello {
        serial_println!(
            "[linuxdrv]   hello process ({} bytes): Linux's headers and vsprintf in a process of their own, its initcalls run (a kthread, a mutex held across a sleep, a completion, the scheduler's edges, timers and softirqs, workqueues, the driver core, the layer's memory helpers), a generated stop, a deadlock, a stack overflow caught by its guard page, a failed initcall, early and late, a stranded kthread, a sleep while atomic, module parameters set from its arguments -- an unknown one and a refused value ending it -- and Linux's firmware loader reading files, none without the capability to{}{}: OK",
            ran.bytes,
            if ran.boot_firmware {
                ", and finding the boot medium's where the kernel placed it, read-only"
            } else {
                ""
            },
            if ran.pci {
                ", a PCI function probed through Linux's PCI core, its interrupts handled -- on its legacy line, then by message -- and its DMA done, and one no driver took"
            } else {
                ""
            }
        );
    }
    say_runs(e1000e, wifi, hwsim);
    Ok(())
}

/// What [`hello_runs`] ran: the process's size, and whether its boot
/// firmware and PCI runs were among them.
struct HelloRan {
    bytes: usize,
    boot_firmware: bool,
    pci: bool,
}

/// The hello process's runs (`linuxdrv/hello`): what ran, or `None`,
/// skipped and recorded so, where the bootloader did not load it.
///
/// Run with no mode -- its first argument without an `=`; the rest set
/// module parameters, and [`start`] adds the firmware's directory -- its
/// initcalls allocate, take a spinlock with interrupts "off", print a line
/// formatted by Linux, run a kthread against a mutex held across a sleep and
/// a completion, try the scheduler's edges, arm timers, schedule tasklets,
/// queue work, probe a device of their own through Linux's driver core and
/// try the layer's memory helpers, and the process exits 0. Given a mode,
/// the first initcall ends it otherwise: "stop" calls a function nothing
/// defines, which the build made a generated stop ([`EXIT_STOP`]);
/// "deadlock" waits for what never comes ([`EXIT_DEADLOCK`]); "overflow"
/// runs past its task's stack, into the guard page below it
/// ([`KILLED_ACCESS_VIOLATION`]); "fail" starts a kthread that never ends
/// and fails, which must end the process at once ([`EXIT_INIT_FAILED`]);
/// "earlyfail" fails a core initcall of the driver's own, which must end it
/// too ([`EXIT_INIT_FAILED`]) -- a failure is carried past only when it is
/// a Linux subsystem's, whatever its level; "stranded" succeeds but leaves a
/// kthread waiting for what never comes, which must end it as deadlocked
/// ([`EXIT_DEADLOCK`]), not as done; "atomic" sleeps with bottom halves
/// off, a BUG ([`EXIT_BUG`]). Then [`firmware_runs`], [`boot_firmware_run`]
/// and [`pci_runs`], each skipped, and recorded so, where what it needs is
/// not there.
///
/// # Errors
///
/// `InternalError` when a run ends otherwise.
fn hello_runs(skips: &mut crate::fs::selftest::Skips) -> KernelResult<Option<HelloRan>> {
    let Some(elf) = crate::boot::module(HELLO) else {
        skips.record(
            "Linux driver layer's hello process",
            "not loaded beside the kernel (no limine.conf module named linuxdrv/hello)",
        );
        return Ok(None);
    };
    let hello = Program {
        elf,
        name: "lx-hello",
        driver: true,
        limit_s: 10,
        poll_ms: 0,
    };
    let cases: [(&[&[u8]], i32, &str); 8] = [
        (&[b"hello"], EXIT_OK, "ran its initcalls and exited 0"),
        // Its mode after a parameter: a mode is the first word wherever it
        // stands, and one missed would run the plain initcalls, exiting 0.
        (
            &[b"hello", b"hello.answer=42", b"stop"],
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
    firmware_runs(&hello, skips)?;
    let boot_firmware = boot_firmware_run(&hello, skips)?;
    let pci = pci_runs(&hello, skips)?;
    Ok(Some(HelloRan {
        bytes: elf.len(),
        boot_firmware,
        pci,
    }))
}

/// The verdicts of the runs after hello's, for what ran: e1000e's on its
/// network, mt7925e's with or without a card, hwsim's.
fn say_runs(e1000e: Option<Network>, wifi: Option<bool>, hwsim: bool) {
    if let Some(network) = e1000e {
        let lan = network == Network::Lan;
        serial_println!(
            "[linuxdrv]   e1000e process: Linux's e1000e bound {}, brought its link up and served it to the network stack -- refusing a daemon without the card's key, for that reason, and the daemon refusing a mode it does not know -- whose daemon attached to it, took an address by DHCP {}, resolved the gateway and had 400 full-size pings answered 16 at a time{}, payloads as sent, was told the link, {}{}, timed the card -- then, unless skipped above, the kernel's own e1000 the same way -- and was let go for breaking the protocol; then a daemon served sockets over the card, a name asked through it answered; the driver warning nowhere: OK",
            if lan {
                "the card linuxdrv.card names"
            } else {
                "the spare 82574"
            },
            if lan {
                "on the network beyond it"
            } else {
                "through it"
            },
            if lan { " (a few allowed lost)" } else { "" },
            if lan {
                "left multicast unchecked (its router need not answer the check's solicitations)"
            } else {
                "found multicast passing as listed"
            },
            if !lan && cable_puller() {
                ", the link following the card's cable -- a ping sent while it was down dropped, never sent later; the lease confirmed by INIT-REBOOT, the addresses announced and the lease renewed when it came back -- and the serving daemon's lease confirmed after a pull of its own"
            } else {
                ""
            }
        );
    }
    match wifi {
        Some(true) => serial_println!(
            "[linuxdrv]   mt7925e process: Linux's mt7925e bound the card linuxdrv.wifi names, loaded its firmware from the boot medium, brought its interface up and had nl80211 scan, the networks found said above: OK"
        ),
        Some(false) => serial_println!(
            "[linuxdrv]   mt7925e process: Linux's 802.11 stack (cfg80211, nl80211, mac80211) and the mt7925e driver started with no card, and the process exited 0: OK"
        ),
        None => {}
    }
    if hwsim {
        serial_println!(
            "[linuxdrv]   hwsim process: Linux's mac80211_hwsim, two simulated radios on one medium -- the first made an open access point through nl80211, the second's scan finding it, its name, address and frequency as beaconed, and joining it, the layer answering its authentication and association as hostapd would, and a frame crossing the network each way, in clear on the air; then the network made WPA2's, joined again, the four-way handshake made by the layer as both sides, and a frame crossing each way encrypted by mac80211's CCMP, protected on the air, its CCM checked against RFC 3610 and the handshake's keys against 802.11's vectors: OK"
        );
    }
}

/// Linux's mac80211_hwsim (`linuxdrv/hwsim`): two simulated radios, which
/// mac80211 drives as it drives a card, on one medium. Its "hwsim" run
/// (`linuxdrv/emul/lx_wifi.c`) must make the first an open access point
/// through nl80211, have the second's scan find it and the second join it
/// -- the layer answering its authentication and association on the first
/// as hostapd would -- and a frame cross the network each way, in clear on
/// the air; then make the network WPA2's, have the second join it again,
/// make the four-way handshake as both sides, and a frame cross each way
/// encrypted, protected on the air; and exit 0: the 802.11 stack, its
/// ciphers and the layer's use of them tested end to end where there is no
/// WiFi card. Whether it ran: skipped, and recorded so, where the
/// bootloader did not load it.
///
/// # Errors
///
/// `InternalError` when the run ends otherwise.
fn hwsim_run(skips: &mut crate::fs::selftest::Skips) -> KernelResult<bool> {
    let Some(elf) = crate::boot::module(HWSIM) else {
        skips.record(
            "Linux's mac80211_hwsim through the driver layer",
            "not loaded beside the kernel (no limine.conf module named linuxdrv/hwsim)",
        );
        return Ok(false);
    };
    // The process's own waits, each at its limit (lx_wifi.c): 30 s for the
    // radios to register; 30 s for each of the two scans; 20 s for each of
    // the two joins and for the open network's leaving; 10 s for the
    // handshake; 2 s for each of the four links to come up, each of the
    // four frames to cross and each of the two watches to see them -- 180 s
    // all told -- and the crypto manager's own wait to make "ccm(aes)" on
    // top (crypto/api.c's larval wait, 60 s): 240 s. A minute past those,
    // for the process to say which ran out before the kernel cuts it off.
    let program = Program {
        elf,
        name: "lx-hwsim",
        driver: true,
        limit_s: 300,
        poll_ms: 0,
    };
    expect(
        &program,
        &[b"hwsim", b"hwsim"],
        &[],
        EXIT_OK,
        "made one simulated radio an access point, found it from the other, joined it and passed a frame each way, open and then protected by WPA2",
        || None,
    )?;
    Ok(true)
}

/// Linux's mt7925e (`linuxdrv/mt7925e`), the layer's first WiFi driver.
/// With no card -- QEMU has none -- its initcalls, cfg80211's, nl80211's
/// and mac80211's among them, must run and the process exit 0, having
/// probed nothing. Given `named`, the card `linuxdrv.wifi` names
/// ([`named_card`]), and the right to read its firmware ([`firmware_reading`]), its
/// "scan" run must bind the card, bring its interface up, have nl80211 scan
/// and say what it found (`linuxdrv/emul/lx_wifi.c`), and exit 0. Which ran
/// -- `Some(true)` the card's scan -- or `None`, skipped and recorded so,
/// where the bootloader did not load it (never with a card named:
/// [`self_test`] refuses that first).
///
/// # Errors
///
/// `InternalError` when a run ends otherwise.
fn wifi_runs(
    skips: &mut crate::fs::selftest::Skips,
    named: Option<crate::pci::PciDevice>,
) -> KernelResult<Option<bool>> {
    let Some(elf) = crate::boot::module(MT7925E) else {
        skips.record(
            "Linux's mt7925e through the driver layer",
            "not loaded beside the kernel (no limine.conf module named linuxdrv/mt7925e)",
        );
        return Ok(None);
    };
    let Some(card) = named else {
        let program = Program {
            elf,
            name: "lx-mt7925e",
            driver: true,
            limit_s: 30,
            poll_ms: 0,
        };
        expect(
            &program,
            &[b"mt7925e"],
            &[],
            EXIT_OK,
            "ran Linux's 802.11 stack's initcalls with no card and exited 0",
            || None,
        )?;
        return Ok(Some(false));
    };
    let a = card.address;
    let [boot_fw, system_fw] = firmware_reading()?;
    let caps = [
        (
            ResourceType::PciDevice,
            crate::cap::pci_device_id(a.bus, a.device, a.function),
            Rights::WRITE,
        ),
        boot_fw,
        system_fw,
    ];
    let arg = alloc::format!("pci={:02x}:{:02x}.{:x}", a.bus, a.device, a.function);
    // The card's firmware loaded -- from the boot medium's, which start()
    // names -- then up to 30 s for it to register and 30 s for the scan
    // (lx_wifi.c).
    let program = Program {
        elf,
        name: "lx-mt7925e",
        driver: true,
        limit_s: 90,
        poll_ms: 0,
    };
    expect(
        &program,
        &[b"mt7925e", arg.as_bytes(), b"scan"],
        &caps,
        EXIT_OK,
        "bound the card, brought its interface up, had nl80211 scan and said what it found",
        || None,
    )?;
    Ok(Some(true))
}

/// `parse_pci_address` at its edges, as the operator's `linuxdrv.card`
/// reaches it.
///
/// # Errors
///
/// `InternalError`, said, for an answer that is not right.
fn check_parse_pci_address() -> KernelResult<()> {
    let at = |bus, device, function| {
        Some(crate::pci::PciAddress {
            bus,
            device,
            function,
        })
    };
    let cases = [
        ("00:1f.6", at(0, 0x1f, 6)),
        ("3:0.1", at(3, 0, 1)),
        ("ff:1f.7", at(0xff, 0x1f, 7)),
        ("00:20.0", None),
        ("00:1f.8", None),
        ("00:1f", None),
        ("00.1f.6", None),
        ("00:1f.6x", None),
        ("100:00.0", None),
        ("", None),
    ];
    for (text, want) in cases {
        if parse_pci_address(text) != want {
            serial_println!(
                "[linuxdrv]   FAIL: linuxdrv.card={text} read as {:?}, not {want:?}",
                parse_pci_address(text)
            );
            return Err(KernelError::InternalError);
        }
    }
    Ok(())
}

/// Where the boot medium's firmware is put ([`mount_boot_firmware`]), and
/// what every driver process is told as `firmware_class.path` once it is
/// ([`start`]): a mount point of its own, at the top of the tree -- whose
/// parent, `/`, always exists and is never replaced -- so that placing the
/// firmware writes nothing to whatever root there is, and hides no
/// `/lib/firmware` an installed system carries, which Linux's loader
/// searches after this one.
pub const BOOT_FIRMWARE_DIR: &str = "/.boot-firmware";

/// Where an installed system keeps its firmware, which Linux's loader
/// searches after [`BOOT_FIRMWARE_DIR`].
const SYSTEM_FIRMWARE_DIR: &str = "/lib/firmware";

/// The right a driver process reads its firmware with: to read beneath the
/// boot medium's firmware and the system's, and nothing else
/// (`ResourceType::FileTree`, design-decisions 1595) -- never the File
/// capability, which is every file
/// (known-issues A-LINUX-DRIVER-PROCESS-READS-FILES-UNCONFINED).
///
/// # Errors
///
/// The tree registry's (`fs::trees::register`): full.
fn firmware_reading() -> KernelResult<[(ResourceType, u64, Rights); 2]> {
    let tree = |dir: &str| {
        crate::fs::trees::register(crate::fs::path::Path::new(dir))
            .map(|id| (ResourceType::FileTree, id, Rights::READ))
    };
    Ok([tree(BOOT_FIRMWARE_DIR)?, tree(SYSTEM_FIRMWARE_DIR)?])
}

/// Whether [`mount_boot_firmware`] placed the boot medium's firmware, whole
/// and sealed read-only. Only then is a driver process told
/// [`BOOT_FIRMWARE_DIR`]: without the mount, the name is anyone's to make on
/// the root, and a placement that failed may have left one half-written.
static BOOT_FIRMWARE_PLACED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
/// The `module_cmdline:` prefix that makes a module firmware, what follows
/// it the file's path under [`BOOT_FIRMWARE_DIR`].
const FIRMWARE_MODULE: &[u8] = b"firmware/";

/// `name`, a module's, as a firmware file's path under
/// [`BOOT_FIRMWARE_DIR`]: what follows [`FIRMWARE_MODULE`], as bytes, as a
/// path is. `None` for a module that is not firmware; why not, for one whose
/// path is no path -- empty, or with an empty, `.` or `..` component, any of
/// which would put the file elsewhere or nowhere.
fn firmware_module_path(name: &[u8]) -> Option<Result<&[u8], &'static str>> {
    let path = name.strip_prefix(FIRMWARE_MODULE)?;
    if path
        .split(|&b| b == b'/')
        .any(|c| c.is_empty() || c == b"." || c == b"..")
    {
        return Some(Err("its path is empty, or has an empty, . or .. component"));
    }
    Some(Ok(path))
}

/// A firmware file the boot medium carries: its path under
/// [`BOOT_FIRMWARE_DIR`], and its bytes.
type FirmwareFile<'a> = (&'a [u8], &'a [u8]);

/// A module [`firmware_files`] refused: its name, and why.
#[derive(Debug, PartialEq, Eq)]
struct FirmwareRefused<'a> {
    name: &'a [u8],
    why: &'static str,
}

/// The firmware files `modules` -- names and bytes, as
/// [`crate::boot::modules`] gives them -- carry, in their order: each module
/// named `firmware/<path>` as its path ([`firmware_module_path`]) and bytes,
/// the rest passed over. The first module refused, for a path that is none,
/// or that an earlier module's path names too or has as a directory, or the
/// reverse -- any of which would leave one of them unplaced.
fn firmware_files<'a>(
    modules: impl Iterator<Item = (&'a [u8], &'a [u8])>,
) -> Result<alloc::vec::Vec<FirmwareFile<'a>>, FirmwareRefused<'a>> {
    use crate::fs::path::Path;
    let mut files: alloc::vec::Vec<FirmwareFile<'a>> = alloc::vec::Vec::new();
    for (name, bytes) in modules {
        let why = match firmware_module_path(name) {
            None => continue,
            Some(Err(why)) => why,
            Some(Ok(path)) if files.iter().any(|&(p, _)| p == path) => {
                "another module names the same file"
            }
            // Equal paths are caught above; a component-aligned prefix
            // either way is a file where the other wants a directory.
            Some(Ok(path))
                if files.iter().any(|&(p, _)| {
                    let (p, path) = (Path::new(p), Path::new(path));
                    p.starts_with(path) || path.starts_with(p)
                }) =>
            {
                "another module's file is a directory on its path, or the reverse"
            }
            Some(Ok(path)) => {
                files.push((path, bytes));
                continue;
            }
        };
        return Err(FirmwareRefused { name, why });
    }
    Ok(files)
}

/// `files` at their paths under `at` -- a mount point whose parent exists --
/// on a memfs of its own mounted there, then made read-only: a driver
/// process loads what the boot medium carried, and nothing another process
/// put there. Nothing is mounted for no files. How many files were placed,
/// and their bytes.
///
/// # Errors
///
/// The VFS's, when the memfs cannot be mounted, a file written or the mount
/// sealed: then nothing is left -- the memfs unmounted, as no firmware
/// rather than some.
fn place_firmware(files: &[FirmwareFile<'_>], at: &str) -> KernelResult<(usize, usize)> {
    use crate::fs::path::{Path, PathBuf};
    use crate::fs::vfs::Vfs;
    if files.is_empty() {
        return Ok((0, 0));
    }
    crate::fs::memfs::mount(at)?;
    let placed = (|| {
        let mut bytes = 0usize;
        for &(path, data) in files {
            let mut full = PathBuf::from(at);
            full.push(Path::new(path));
            if let Some(dir) = full.parent() {
                Vfs::mkdir_all(dir)?;
            }
            Vfs::write_file(&full, data)?;
            bytes = bytes.saturating_add(data.len());
        }
        let mut options = Vfs::mount_options(at)?;
        options.read_only = true;
        Vfs::remount(at, options)?;
        Ok::<usize, KernelError>(bytes)
    })();
    match placed {
        Ok(bytes) => Ok((files.len(), bytes)),
        Err(e) => {
            // The placement's error is the answer. An unmount that fails as
            // well leaves the half-written memfs at `at`, said here -- which
            // no driver process is then told of: mount_boot_firmware marks
            // the firmware placed only on success (BOOT_FIRMWARE_PLACED).
            if let Err(unmounted) = Vfs::unmount(at) {
                serial_println!(
                    "[boot] the firmware's mount at {at} stays, half-placed: its unmount failed: {unmounted:?}"
                );
            }
            Err(e)
        }
    }
}

/// The boot medium's firmware -- every module limine.conf names
/// `firmware/<path>` -- placed at `<path>` under [`BOOT_FIRMWARE_DIR`]
/// ([`place_firmware`]): as an initramfs carries a machine's firmware on
/// Linux, for the driver processes' loader, on a machine whose root holds
/// none (the test PC, booted from a medium with only an ESP). Modules the
/// bootloader loaded that cannot be read whole are said, since one may be
/// firmware a driver will then not find. How many files were placed, and
/// their bytes.
///
/// # Errors
///
/// `InvalidArgument`, said, for a firmware module [`firmware_files`]
/// refuses: nothing is placed. [`place_firmware`]'s.
pub fn mount_boot_firmware() -> KernelResult<(usize, usize)> {
    let loaded = crate::boot::module_count();
    let read = crate::boot::modules().count();
    if read < loaded {
        serial_println!(
            "[boot] {} of the bootloader's {loaded} module(s) cannot be read: no name ending within the bound, or past the most read -- neither run nor placed",
            loaded.saturating_sub(read)
        );
    }
    let files = firmware_files(crate::boot::modules()).map_err(|refused| {
        serial_println!(
            "[boot] firmware module \"{}\": {}; no firmware placed",
            refused.name.escape_ascii(),
            refused.why
        );
        KernelError::InvalidArgument
    })?;
    let placed = place_firmware(&files, BOOT_FIRMWARE_DIR)?;
    if placed.0 > 0 {
        BOOT_FIRMWARE_PLACED.store(true, core::sync::atomic::Ordering::Release);
    }
    Ok(placed)
}

/// [`firmware_module_path`] at its edges: each answer `None` (not
/// firmware), `Some(None)` (refused) or the path.
///
/// # Errors
///
/// `InternalError`, said, for an answer that is not right.
fn check_firmware_module_path() -> KernelResult<()> {
    /// A module's name, and how it must be read.
    type Case = (&'static [u8], Option<Option<&'static [u8]>>);
    let cases: [Case; 10] = [
        (b"linuxdrv/hello", None),
        (b"firmware", None),
        (b"firmware/x.bin", Some(Some(b"x.bin"))),
        (
            b"firmware/mediatek/mt7925/WIFI_RAM_CODE_MT7925_1_1.bin",
            Some(Some(b"mediatek/mt7925/WIFI_RAM_CODE_MT7925_1_1.bin")),
        ),
        // A path is bytes: any but '/' and NUL make a name.
        (b"firmware/caf\xe9.bin", Some(Some(b"caf\xe9.bin"))),
        (b"firmware/", Some(None)),
        (b"firmware//x.bin", Some(None)),
        (b"firmware/a/", Some(None)),
        (b"firmware/../x.bin", Some(None)),
        (b"firmware/a/./x.bin", Some(None)),
    ];
    for (name, want) in cases {
        let got = firmware_module_path(name).map(Result::ok);
        if got != want {
            serial_println!(
                "[linuxdrv]   FAIL: module \"{}\" read as firmware path {:?}, not {:?}",
                name.escape_ascii(),
                got.map(|p| p.map(<[u8]>::escape_ascii).map(|e| alloc::format!("{e}"))),
                want.map(|p| p.map(<[u8]>::escape_ascii).map(|e| alloc::format!("{e}")))
            );
            return Err(KernelError::InternalError);
        }
    }
    check_firmware_files()
}

/// [`firmware_files`] over made-up module lists: a driver passed over, the
/// firmware in its order; nothing at all; a file two modules name, a file
/// that is a directory on another's path -- either way round -- and a path
/// that is none, each refused as the module that did it; a name that only
/// begins as another's does, which is no clash.
///
/// # Errors
///
/// `InternalError`, said, for an answer that is not right.
fn check_firmware_files() -> KernelResult<()> {
    // Each module's bytes are its name, so a file is seen to keep its own.
    const MIXED: &[&[u8]] = &[b"linuxdrv/hello", b"firmware/a.bin", b"firmware/b/c.bin"];
    const NONE: &[&[u8]] = &[b"linuxdrv/hello"];
    const TWICE: &[&[u8]] = &[b"firmware/a.bin", b"firmware/b.bin", b"firmware/a.bin"];
    const FILE_THEN_DIR: &[&[u8]] = &[b"firmware/rtl/fw", b"firmware/rtl/fw/v2.bin"];
    const DIR_THEN_FILE: &[&[u8]] = &[b"firmware/rtl/fw/v2.bin", b"firmware/rtl/fw"];
    const ALIKE: &[&[u8]] = &[b"firmware/rtl/fw", b"firmware/rtl/fwx.bin"];
    const ESCAPING: &[&[u8]] = &[b"firmware/a.bin", b"firmware/../b.bin"];
    const CLASH: &str = "another module's file is a directory on its path, or the reverse";
    let refused = |name: &'static [u8], why| Err(FirmwareRefused { name, why });
    let cases = [
        (
            MIXED,
            Ok(alloc::vec![
                (b"a.bin" as &[u8], b"firmware/a.bin" as &[u8]),
                (b"b/c.bin", b"firmware/b/c.bin"),
            ]),
        ),
        (NONE, Ok(alloc::vec![])),
        (
            TWICE,
            refused(b"firmware/a.bin", "another module names the same file"),
        ),
        (FILE_THEN_DIR, refused(b"firmware/rtl/fw/v2.bin", CLASH)),
        (DIR_THEN_FILE, refused(b"firmware/rtl/fw", CLASH)),
        (
            ALIKE,
            Ok(alloc::vec![
                (b"rtl/fw" as &[u8], b"firmware/rtl/fw" as &[u8]),
                (b"rtl/fwx.bin", b"firmware/rtl/fwx.bin"),
            ]),
        ),
        (
            ESCAPING,
            refused(
                b"firmware/../b.bin",
                "its path is empty, or has an empty, . or .. component",
            ),
        ),
    ];
    for (names, want) in cases {
        let got = firmware_files(names.iter().map(|&n| (n, n)));
        if got != want {
            let shown = |r: &Result<alloc::vec::Vec<FirmwareFile<'_>>, FirmwareRefused<'_>>| match r
            {
                Ok(files) => alloc::format!(
                    "{:?}",
                    files
                        .iter()
                        .map(|&(p, _)| alloc::format!("{}", p.escape_ascii()))
                        .collect::<alloc::vec::Vec<_>>()
                ),
                Err(r) => alloc::format!("\"{}\" refused: {}", r.name.escape_ascii(), r.why),
            };
            let names: alloc::vec::Vec<_> = names
                .iter()
                .map(|n| alloc::format!("{}", n.escape_ascii()))
                .collect();
            serial_println!(
                "[linuxdrv]   FAIL: firmware modules {names:?} gave {}, not {}",
                shown(&got),
                shown(&want)
            );
            return Err(KernelError::InternalError);
        }
    }
    Ok(())
}

/// [`place_firmware`] at a scratch mount point under `/tmp`: nothing
/// mounted for no files; two files placed, their bytes as given, where
/// nothing may then write; and a placement that fails part-way -- a file
/// where a later one wants a directory, which [`firmware_files`] refuses
/// but this is not given through it -- leaving nothing mounted. Skipped,
/// and recorded so, where `/tmp` is not mounted.
///
/// # Errors
///
/// `InternalError`, said, for an outcome that is not right.
fn check_place_firmware(skips: &mut crate::fs::selftest::Skips) -> KernelResult<()> {
    use crate::fs::selftest::is_mounted;
    use crate::fs::vfs::Vfs;
    const AT: &str = "/tmp/linuxdrv-fwplace";
    if !is_mounted("/tmp") {
        skips.record(
            "Linux driver layer's placing of firmware",
            "/tmp is not mounted",
        );
        return Ok(());
    }
    let fail = |what: &str| {
        serial_println!("[linuxdrv]   FAIL: placing firmware: {what}");
        KernelError::InternalError
    };
    // Whatever a case left mounted taken down before the next, or before a
    // failure is answered: true when nothing is left. A scratch mount that
    // will not go stays in /tmp, which the next boot does not see.
    let gone = || !is_mounted(AT) || Vfs::unmount(AT).is_ok();
    if place_firmware(&[], AT) != Ok((0, 0)) || is_mounted(AT) {
        gone();
        return Err(fail("no files mounted something"));
    }
    let files: [FirmwareFile<'_>; 2] = [(b"a/b.bin", b"one"), (b"c.bin", b"two!")];
    let placed = place_firmware(&files, AT);
    let found = (
        Vfs::read_file("/tmp/linuxdrv-fwplace/a/b.bin"),
        Vfs::read_file("/tmp/linuxdrv-fwplace/c.bin"),
    );
    let written = Vfs::write_file("/tmp/linuxdrv-fwplace/d.bin", b"x").is_ok();
    if !gone() {
        return Err(fail("the placed files' scratch mount would not go"));
    }
    if placed != Ok((2, 7)) || found != (Ok(b"one".to_vec()), Ok(b"two!".to_vec())) || written {
        return Err(fail(
            "two files were not placed as given, or a third could be written beside them",
        ));
    }
    let clash: [FirmwareFile<'_>; 2] = [(b"x", b"1"), (b"x/y", b"2")];
    let refused = place_firmware(&clash, AT).is_err();
    let left = is_mounted(AT);
    if left {
        gone();
    }
    match (refused, left) {
        (true, false) => Ok(()),
        (false, _) => Err(fail(
            "a file and a directory of the same name were both placed",
        )),
        (true, true) => Err(fail("a placement that failed part-way left a mount")),
    }
}

/// The boot test's firmware module, as [`mount_boot_firmware`] must have
/// placed it and the hello process's "bootfw" run must load it: at
/// [`BOOT_FIRMWARE_DIR`]`/slateos/hello.fw`, the module's bytes, where
/// nothing may write -- loaded through Linux's loader from the directory
/// every driver process is told (`firmware_class.path`), holding only the
/// right every firmware-loading driver holds ([`firmware_reading`]); and,
/// given no capability to read files, or only one to read beneath another
/// directory, not loaded, the run ending as a failed initcall: so that a run
/// passing for a hello that did nothing is caught, and a tree that let its
/// holder out of it too.
/// Whether it ran: skipped, and recorded so, where the boot medium carries
/// no such module.
///
/// # Errors
///
/// `InternalError`, said, when the file is not there as the module was, a
/// write to its directory is let through, or a run ends otherwise.
fn boot_firmware_run(
    hello: &Program<'_>,
    skips: &mut crate::fs::selftest::Skips,
) -> KernelResult<bool> {
    use crate::fs::vfs::Vfs;
    let Some(module) = crate::boot::module(b"firmware/slateos/hello.fw") else {
        skips.record(
            "Linux driver layer's firmware from the boot medium",
            "no firmware/slateos/hello.fw module (limine.conf; the boot test stages it)",
        );
        return Ok(false);
    };
    let fail = |what: &str| {
        serial_println!("[linuxdrv]   FAIL: boot firmware: {what}");
        KernelError::InternalError
    };
    // The module is there, so the boot placed it -- or said why not, a
    // WARNING from boot long before this.
    if !BOOT_FIRMWARE_PLACED.load(core::sync::atomic::Ordering::Acquire) {
        return Err(fail(
            "the boot medium's firmware was not placed (the boot's WARNING says why)",
        ));
    }
    let path = alloc::format!("{BOOT_FIRMWARE_DIR}/slateos/hello.fw");
    if Vfs::read_file(path.as_str()).ok().as_deref() != Some(module) {
        return Err(fail("slateos/hello.fw is not the module's bytes"));
    }
    if Vfs::write_file(
        alloc::format!("{BOOT_FIRMWARE_DIR}/slateos/written.fw").as_str(),
        b"x",
    )
    .is_ok()
    {
        return Err(fail("a file was written beside the boot medium's"));
    }
    // Told where the firmware is as every driver process is (start()).
    let argv: [&[u8]; 2] = [b"hello", b"bootfw"];
    // With the right every firmware-loading driver has, to read beneath
    // the firmware directories alone.
    expect(
        hello,
        &argv,
        &firmware_reading()?,
        EXIT_OK,
        "loaded the boot medium's firmware through Linux's loader, holding only the right to read beneath the firmware directories",
        || None,
    )?;
    expect(
        hello,
        &argv,
        &[],
        EXIT_INIT_FAILED,
        "failed its firmware initcall, given no capability to read the boot medium's firmware",
        || None,
    )?;
    // A tree is that directory alone: holding one for another, the boot
    // medium's firmware is as unreadable as with none.
    let elsewhere = crate::fs::trees::register(crate::fs::path::Path::new(FIRMWARE_DIR))?;
    expect(
        hello,
        &argv,
        &[(ResourceType::FileTree, elsewhere, Rights::READ)],
        EXIT_INIT_FAILED,
        "failed its firmware initcall, holding only the right to read beneath another directory",
        || None,
    )?;
    Ok(true)
}

/// Where [`firmware_runs`] puts the hello process's firmware.
const FIRMWARE_DIR: &str = "/tmp/linuxdrv-fw";
/// A directory with a size of its own, for [`firmware_runs`] to have read
/// as a file: the rootfs's ext4, where the boot test mounts it.
const SIZED_DIR: &str = "/mnt";
/// The test firmware's size: several pages, and none of them whole at its
/// end. (One `SYS_FS_PREAD` reads all of it: a read cut short, which the
/// layer's loop would continue, does not happen here.)
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
/// nothing; an argument naming a parameter hello lacks, giving one a value
/// its kind refuses -- one of its own before any initcall, one of the
/// device level's there -- or holding both a space and a quote, which no
/// command line carries, must end the process ([`EXIT_ARGUMENT`]). The
/// directory is removed whatever the runs found. Skipped, and recorded so,
/// where `/tmp` is not mounted.
///
/// # Errors
///
/// `InternalError` when the fixture cannot be written or a run ends
/// otherwise.
fn firmware_runs(hello: &Program<'_>, skips: &mut crate::fs::selftest::Skips) -> KernelResult<()> {
    use crate::fs::vfs::Vfs;
    if !crate::fs::selftest::is_mounted("/tmp") {
        skips.record(
            "Linux driver layer's firmware and parameter runs",
            "/tmp is not mounted",
        );
        return Ok(());
    }
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
    // As the process sees it: its fstat reads the file's metadata, which
    // stat's directory entry need not agree with.
    let sized = Vfs::metadata(SIZED_DIR)
        .is_ok_and(|m| m.entry_type == crate::fs::vfs::EntryType::Directory && m.size > 0);
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
        let refused: [(&[u8], &str); 4] = [
            (b"hello.nonesuch=1", "ended on a parameter it does not have"),
            (
                b"hello.answer=forty",
                "ended, before any initcall, on a value its parameter's kind refuses",
            ),
            (
                b"hello.device_level=six",
                "ended, at the device initcalls, on a value its device-level parameter's kind refuses",
            ),
            (
                b"hello.motto=say \"hi\" now",
                "ended on a value holding both a space and a quote",
            ),
        ];
        for (arg, what) in refused {
            expect(hello, &[b"hello", arg], &[], EXIT_ARGUMENT, what, || None)?;
        }
        Ok(())
    });
    // The fixture's own: a removal that fails leaves files in /tmp, which
    // the next boot does not see, and the runs' verdict stands either way.
    let _ = Vfs::remove_recursive(FIRMWARE_DIR);
    ran.map_err(|e| {
        serial_println!(
            "[linuxdrv]   FAIL: firmware runs: {e:?} (the fixture in {FIRMWARE_DIR} written: {written:?})"
        );
        KernelError::InternalError
    })
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

/// The network beyond the card Linux's e1000e is given: the boot test's
/// QEMU network, or a real one -- the card `linuxdrv.card` names, on a
/// machine with no boot-test spare.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Network {
    Qemu,
    Lan,
}

/// `BB:DD.F` in hex, as `lspci` writes a PCI function.
fn parse_pci_address(text: &str) -> Option<crate::pci::PciAddress> {
    let (bus, rest) = text.split_once(':')?;
    let (device, function) = rest.split_once('.')?;
    let address = crate::pci::PciAddress {
        bus: u8::from_str_radix(bus, 16).ok()?,
        device: u8::from_str_radix(device, 16).ok()?,
        function: u8::from_str_radix(function, 16).ok()?,
    };
    (address.device < 32 && address.function < 8).then_some(address)
}

/// MediaTek's PCI vendor id.
const MEDIATEK: u16 = 0x14c3;

/// The kind of card an operator's option names for a Linux driver run on a
/// machine with no boot-test spare, which the function named must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardKind {
    /// `linuxdrv.card`'s, for Linux's e1000e: an Intel Ethernet controller
    /// (class 02, subclass 00) -- the test PC's I219-V, which the kernel's
    /// own e1000 does not drive.
    IntelEthernet,
    /// `linuxdrv.wifi`'s, for Linux's mt7925e: a MediaTek network
    /// controller of subclass 80, "other", which is WiFi's (class 02) --
    /// the test PC's RZ717.
    MediatekWifi,
    /// `linuxdrv.serve`'s ([`serve_at_boot`]): any network controller
    /// (class 02), the driver process named being the one to drive it.
    Network,
}

impl CardKind {
    /// The kernel parameter that names such a card.
    fn option(self) -> &'static str {
        match self {
            CardKind::IntelEthernet => "linuxdrv.card",
            CardKind::MediatekWifi => "linuxdrv.wifi",
            CardKind::Network => "linuxdrv.serve",
        }
    }

    /// Whether `card` is one.
    fn is(self, card: &crate::pci::PciDevice) -> bool {
        let (vendor, subclass) = match self {
            CardKind::IntelEthernet => (devprobe::INTEL, 0x00),
            CardKind::MediatekWifi => (MEDIATEK, 0x80),
            CardKind::Network => return card.class == 0x02,
        };
        card.vendor_id == vendor && card.class == 0x02 && card.subclass == subclass
    }
}

impl core::fmt::Display for CardKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            CardKind::IntelEthernet => "an Intel Ethernet controller",
            CardKind::MediatekWifi => "a MediaTek WiFi card",
            CardKind::Network => "a network controller",
        })
    }
}

/// The card `kind`'s option (`<option>=BB:DD.F`) names: a function of that
/// kind no driver holds -- so that a slip naming the GPU whose framebuffer
/// carries the screen log, or a bridge, is refused before its decoding is
/// touched. `None` without the option.
///
/// # Errors
///
/// `InvalidArgument`, said with the check that failed, for an option naming
/// no such function: an operator's run asked for it, and a skip would read
/// as the card's absence.
fn named_card(kind: CardKind) -> KernelResult<Option<crate::pci::PciDevice>> {
    let Some(named) = crate::fs::kernparam::get(kind.option()) else {
        return Ok(None);
    };
    match card_choice(
        &named,
        kind,
        &crate::pci::scan_all(),
        crate::pci::bound_driver,
    ) {
        Ok(card) => Ok(Some(card)),
        Err(refused) => {
            serial_println!("[linuxdrv]   FAIL: {}={named}: {refused}", kind.option());
            Err(KernelError::InvalidArgument)
        }
    }
}

/// Why [`card_choice`] refused the card an operator named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardRefused {
    NotAnAddress,
    NothingThere,
    /// Not the kind of card asked for.
    NotA(CardKind),
    /// The driver of this name (`pci::bound_driver`'s) holds it.
    Held(&'static str),
}

impl core::fmt::Display for CardRefused {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CardRefused::NotAnAddress => f.write_str("not a PCI function as BB:DD.F, in hex"),
            CardRefused::NothingThere => f.write_str("no PCI function there"),
            CardRefused::NotA(kind) => write!(f, "not {kind}"),
            CardRefused::Held(crate::devhandle::USERSPACE_DRIVER) => {
                f.write_str("a process's driver holds it")
            }
            CardRefused::Held(driver) => write!(f, "the kernel's {driver} driver holds it"),
        }
    }
}

/// The function `named` names among `functions`, if a Linux driver for
/// `kind` may be given it: one of that kind that no kernel driver holds
/// (`bound`, the PCI layer's `bound_driver`); or why not.
fn card_choice(
    named: &str,
    kind: CardKind,
    functions: &[crate::pci::PciDevice],
    bound: impl Fn(crate::pci::PciAddress) -> Option<&'static str>,
) -> Result<crate::pci::PciDevice, CardRefused> {
    let address = parse_pci_address(named).ok_or(CardRefused::NotAnAddress)?;
    let card = functions
        .iter()
        .find(|d| d.address == address)
        .ok_or(CardRefused::NothingThere)?;
    if !kind.is(card) {
        return Err(CardRefused::NotA(kind));
    }
    if let Some(driver) = bound(address) {
        return Err(CardRefused::Held(driver));
    }
    Ok(card.clone())
}

/// [`card_choice`] at each of its refusals and its acceptances, over a
/// made-up bus: the test PC's I219-V at 00:1f.6, its UHD 630 at 00:02.0, an
/// Intel function no Ethernet controller at 00:1f.3, another vendor's NIC
/// at 00:03.0, a held Intel NIC at 00:19.0, and its RZ717 WiFi card at
/// 00:05.0, beside a MediaTek function no network controller at 00:06.0.
///
/// # Errors
///
/// `InternalError`, said, for an answer that is not right.
fn check_card_choice() -> KernelResult<()> {
    use crate::pci::{PciAddress, PciDevice};
    use CardKind::{IntelEthernet, MediatekWifi, Network};
    let function = |device, function, vendor_id, class, subclass| PciDevice {
        address: PciAddress {
            bus: 0,
            device,
            function,
        },
        vendor_id,
        device_id: 0x15b8,
        class,
        subclass,
        irq_line: 0,
        bars: [0; 6],
    };
    let bus = [
        function(0x1f, 6, devprobe::INTEL, 0x02, 0x00),
        function(0x02, 0, devprobe::INTEL, 0x03, 0x00),
        function(0x1f, 3, devprobe::INTEL, 0x04, 0x03),
        function(0x03, 0, 0x10ec, 0x02, 0x00),
        function(0x19, 0, devprobe::INTEL, 0x02, 0x00),
        function(0x05, 0, MEDIATEK, 0x02, 0x80),
        function(0x06, 0, MEDIATEK, 0x04, 0x03),
    ];
    let bound = |a: PciAddress| (a.device == 0x19).then_some("e1000");
    let cases = [
        ("00:1f.6", IntelEthernet, Ok((0x1f, 6))),
        ("00:1f:6", IntelEthernet, Err(CardRefused::NotAnAddress)),
        ("00:1c.0", IntelEthernet, Err(CardRefused::NothingThere)),
        (
            "00:02.0",
            IntelEthernet,
            Err(CardRefused::NotA(IntelEthernet)),
        ),
        (
            "00:1f.3",
            IntelEthernet,
            Err(CardRefused::NotA(IntelEthernet)),
        ),
        (
            "00:03.0",
            IntelEthernet,
            Err(CardRefused::NotA(IntelEthernet)),
        ),
        ("00:19.0", IntelEthernet, Err(CardRefused::Held("e1000"))),
        (
            "00:05.0",
            IntelEthernet,
            Err(CardRefused::NotA(IntelEthernet)),
        ),
        ("00:05.0", MediatekWifi, Ok((0x05, 0))),
        (
            "00:06.0",
            MediatekWifi,
            Err(CardRefused::NotA(MediatekWifi)),
        ),
        (
            "00:1f.6",
            MediatekWifi,
            Err(CardRefused::NotA(MediatekWifi)),
        ),
        // `linuxdrv.serve`: any network controller, wired or not, of any maker
        // -- never the GPU, a bridge or an audio function a slip names.
        ("00:1f.6", Network, Ok((0x1f, 6))),
        ("00:05.0", Network, Ok((0x05, 0))),
        ("00:03.0", Network, Ok((0x03, 0))),
        ("00:02.0", Network, Err(CardRefused::NotA(Network))),
        ("00:06.0", Network, Err(CardRefused::NotA(Network))),
        ("00:19.0", Network, Err(CardRefused::Held("e1000"))),
    ];
    for (named, kind, want) in cases {
        let got =
            card_choice(named, kind, &bus, bound).map(|d| (d.address.device, d.address.function));
        if got != want {
            serial_println!(
                "[linuxdrv]   FAIL: {}={named} chose {got:?}, not {want:?}",
                kind.option()
            );
            return Err(KernelError::InternalError);
        }
    }
    let said = [
        (
            CardRefused::Held("e1000"),
            "the kernel's e1000 driver holds it",
        ),
        (
            CardRefused::Held(crate::devhandle::USERSPACE_DRIVER),
            "a process's driver holds it",
        ),
        (CardRefused::NotA(MediatekWifi), "not a MediaTek WiFi card"),
        (CardRefused::NotA(Network), "not a network controller"),
    ];
    for (refused, want) in said {
        let got = alloc::format!("{refused}");
        if got != want {
            serial_println!("[linuxdrv]   FAIL: {refused:?} reads \"{got}\", not \"{want}\"");
            return Err(KernelError::InternalError);
        }
    }
    Ok(())
}

/// The layer's first real driver, Linux's own e1000e (`linuxdrv/e1000e`),
/// given a card to serve to the network stack ([`e1000e_serve`]): `named`,
/// the one `linuxdrv.card` names, on a real network ([`named_card`]), or
/// else the boot test's spare 82574 as hello's PCI runs are -- then the
/// kernel's own e1000 timed as the daemon timed that card ([`native_bench`]).
/// Which network it ran on; `None`, skipped and recorded so, where the
/// bootloader did not load it (never with a card named: [`self_test`]
/// refuses that first) or there is no such card. On a named card's network
/// the checks it cannot make there -- the multicast filter, and the cable a
/// harness would pull -- are recorded as skipped.
///
/// # Errors
///
/// `InternalError` when a run ends otherwise.
fn e1000e_run(
    skips: &mut crate::fs::selftest::Skips,
    named: Option<crate::pci::PciDevice>,
) -> KernelResult<Option<Network>> {
    let Some(elf) = crate::boot::module(E1000E) else {
        skips.record(
            "Linux's e1000e through the driver layer",
            "not loaded beside the kernel (no limine.conf module named linuxdrv/e1000e)",
        );
        return Ok(None);
    };
    let (card, network) = match named {
        Some(card) => (card, Network::Lan),
        None => match crate::devhandle::probe_card() {
            Some(card) => (card, Network::Qemu),
            None => {
                skips.record(
                    "Linux's e1000e through the driver layer",
                    "no unbound 82574 on bus 0 with the boot test's MAC address, and no linuxdrv.card",
                );
                return Ok(None);
            }
        },
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
        driver: true,
        limit_s: 30,
        poll_ms: 0,
    };
    e1000e_serve(&program, &caps, a, &arg, network)?;
    if network == Network::Lan {
        skips.record(
            "Linux's e1000e's multicast filter",
            "on the network beyond the card linuxdrv.card names, whose router need not answer the check's solicitations",
        );
        if cable_puller() {
            skips.record(
                "Linux's e1000e following its card's cable",
                "the harness's cable is not the card linuxdrv.card names",
            );
        }
    }
    native_bench(skips)?;
    Ok(Some(network))
}

/// The kernel's own e1000 driver timed by the netstack daemon's `nic-bench`
/// -- the first 82574, claimed alone by its MAC address (the kernel's stack
/// keeping its primary card), an address taken on it by DHCP -- as
/// `nic-check` has just timed Linux's e1000e serving its card: on QEMU's
/// network the spare 82574, the same model behind the same kind of
/// network, its native twin; on a named card's (`linuxdrv.card`), another
/// card on another network, which these numbers only stand beside. The
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
        driver: false,
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

/// What [`parse_serve`] reads in `linuxdrv.serve`: the driver's name and the
/// card's PCI address as written, or why not.
type Serve<'a> = Result<(&'a str, &'a str), &'static str>;

/// `linuxdrv.serve`'s value read: the driver's name -- a directory name under
/// `linuxdrv/`, letters, digits, `_` and `-` -- and the PCI address after its
/// `@`, as written ([`card_choice`] reads it); else why not.
fn parse_serve(value: &str) -> Serve<'_> {
    let (driver, at) = value.split_once('@').ok_or("not <driver>@<BB:DD.F>")?;
    let named = !driver.is_empty()
        && driver
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !named {
        return Err("not a driver's name before the @");
    }
    Ok((driver, at))
}

/// [`parse_serve`] at its edges, as the operator's `linuxdrv.serve` reaches it.
///
/// # Errors
///
/// `InternalError`, said, for an answer that is not right.
fn check_parse_serve() -> KernelResult<()> {
    let cases: [(&str, Serve<'_>); 6] = [
        ("e1000e@00:05.0", Ok(("e1000e", "00:05.0"))),
        ("mt7925e@06:00.0", Ok(("mt7925e", "06:00.0"))),
        ("e1000e", Err("not <driver>@<BB:DD.F>")),
        ("@00:05.0", Err("not a driver's name before the @")),
        ("../hello@00:05.0", Err("not a driver's name before the @")),
        ("e1000e@", Ok(("e1000e", ""))),
    ];
    for (value, want) in cases {
        let got = parse_serve(value);
        if got != want {
            serial_println!(
                "[linuxdrv]   FAIL: linuxdrv.serve={value} read as {got:?}, not {want:?}"
            );
            return Err(KernelError::InternalError);
        }
    }
    Ok(())
}

/// The driver processes that read their card's firmware, and are given the
/// right to ([`firmware_reading`]) -- e1000e reads none.
const FIRMWARE_DRIVERS: &[&str] = &["mt7925e"];

/// The card `linuxdrv.serve=<driver>@<BB:DD.F>` names for its driver
/// process: the driver's name, and a network controller no driver holds
/// ([`card_choice`], [`CardKind::Network`]) -- so that a slip naming the GPU,
/// a bridge or an audio function is refused before the function is touched.
/// `None` without the option.
///
/// # Errors
///
/// `InvalidArgument`, said with what was wrong, for an option naming no such
/// card or no driver's name.
fn served_card() -> KernelResult<Option<(alloc::string::String, crate::pci::PciDevice)>> {
    let Some(value) = crate::fs::kernparam::get(CardKind::Network.option()) else {
        return Ok(None);
    };
    let refuse = |why: &dyn core::fmt::Display| {
        serial_println!("[linuxdrv]   FAIL: linuxdrv.serve={value}: {why}");
        KernelError::InvalidArgument
    };
    let (driver, at) = parse_serve(&value).map_err(|why| refuse(&why))?;
    let card = card_choice(
        at,
        CardKind::Network,
        &crate::pci::scan_all(),
        crate::pci::bound_driver,
    )
    .map_err(|refused| refuse(&refused))?;
    if crate::boot::module(alloc::format!("linuxdrv/{driver}").as_bytes()).is_none() {
        return Err(refuse(
            &"no such driver process is loaded beside the kernel",
        ));
    }
    Ok(Some((alloc::string::String::from(driver), card)))
}

/// A Linux driver process kept running for the system's life: the one
/// `linuxdrv.serve=<driver>@<BB:DD.F>` names ([`served_card`]) --
/// `linuxdrv/<driver>`, serving its card to the network stack as
/// `nic.<driver>` for as long as the system runs (its `serve=` mode with no
/// `stacks=` limit). It holds the card, the right to register that one name,
/// and, for a driver that reads its card's firmware ([`FIRMWARE_DRIVERS`]),
/// the right to read it there ([`firmware_reading`]). Started past the
/// kernel's self-tests,
/// before the persistent netstack daemon, which `net.nic` points at the card.
/// The name it serves, or `None` when the option is absent: no card is a
/// driver process's unless the machine is configured so.
///
/// Nothing restarts it should it end: known-issues
/// `A-PERSISTENT-NETWORK-PROCESSES-ARE-NOT-RESTARTED`.
///
/// # Errors
///
/// `InvalidArgument`, said, for an option [`served_card`] refuses; the
/// spawn's error otherwise.
pub fn serve_at_boot() -> KernelResult<Option<alloc::string::String>> {
    let Some((driver, card)) = served_card()? else {
        return Ok(None);
    };
    let Some(elf) = crate::boot::module(alloc::format!("linuxdrv/{driver}").as_bytes()) else {
        return Err(KernelError::InvalidArgument);
    };
    let a = card.address;
    let name = alloc::format!("nic.{driver}");
    let mut caps = alloc::vec![
        (
            ResourceType::PciDevice,
            crate::cap::pci_device_id(a.bus, a.device, a.function),
            Rights::WRITE,
        ),
        (
            ResourceType::Service,
            crate::ipc::service::key_id(name.as_bytes()),
            Rights::WRITE,
        ),
    ];
    if FIRMWARE_DRIVERS.contains(&driver.as_str()) {
        caps.extend(firmware_reading()?);
    }
    let serve = alloc::format!("serve={driver}");
    let pci = alloc::format!("pci={:02x}:{:02x}.{:x}", a.bus, a.device, a.function);
    // Never waited for: its limit is no one's to enforce.
    let program = Program {
        elf,
        name: "lx-serve",
        driver: true,
        limit_s: 0,
        poll_ms: 0,
    };
    let process = start(
        &program,
        &[driver.as_bytes(), serve.as_bytes(), pci.as_bytes()],
        &caps,
    )?;
    serial_println!(
        "[linuxdrv] {driver} started serving the card at {:02x}:{:02x}.{:x} as {name} (pid {})",
        a.bus,
        a.device,
        a.function,
        process.pid
    );
    Ok(Some(name))
}

/// The card the persistent netstack daemon is to serve: the one `net.nic`
/// names, when it is the card [`serve_at_boot`] started serving (`served`);
/// else -- no `net.nic`, an empty one, or one naming a card no driver
/// process serves -- `None`, the kernel's NIC, which is said when `net.nic`
/// asked for another.
pub fn netstack_card(served: Option<&str>) -> Option<alloc::string::String> {
    let named = crate::fs::kernparam::get("net.nic").filter(|n| !n.is_empty())?;
    if served == Some(named.as_str()) {
        return Some(named);
    }
    serial_println!(
        "WARNING: net.nic={named} names no card a driver process serves ({}): the netstack daemon serves the kernel's NIC",
        served.map_or_else(
            || alloc::string::String::from("linuxdrv.serve started none"),
            |s| alloc::format!("linuxdrv.serve serves {s}")
        )
    );
    None
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

/// Linux's e1000e serving its card -- the spare 82574, or the one
/// `linuxdrv.card` names ([`named_card`]) -- to the network stack
/// (design-decisions 1589): the driver process -- `program`, holding
/// `capabilities` (the card's) and a `Service` capability for [`SERVICE`]
/// alone, the card at `a`, `pci` its argument naming it -- in its
/// "serve=spare stacks=2" mode, and beside it the netstack daemon in its
/// "nic-check nic.spare" mode -- first without the card's key, when the
/// driver must refuse the daemon for want of it, the daemon then exiting
/// [`check::TURNED_AWAY`]; then holding it, when it
/// must attach to the card, be refused passing on a region it was never
/// granted, take an
/// address by DHCP from the `network` beyond it, resolve the gateway by
/// ARP, have 400 full-size pings answered 16 at a time and 8 more sent to an
/// idle driver, payloads as sent, be told the card's link is up, find the
/// card passing a multicast group exactly while it lists it -- on QEMU's
/// network, whose router answers the check's solicitations; a real one's
/// need not (`nic-check`'s `lan`) -- and -- where the harness can pull the
/// card's cable on QEMU's network ([`cable_puller`]) -- be told the
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
    network: Network,
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
    let cable = network == Network::Qemu && cable_puller();
    // What the daemon may take of the network: its third argument, none for
    // QEMU's with the cable left alone.
    let mode = match network {
        Network::Lan => Some(check::mode::LAN),
        Network::Qemu if cable => Some(check::mode::CABLE),
        Network::Qemu => None,
    };
    let keyed: alloc::vec::Vec<&[u8]> = plain.iter().copied().chain(mode).collect();
    let netstack = |limit_s| Program {
        elf: spawn::NETSTACK_ELF,
        name: "netstack-nic",
        driver: false,
        limit_s,
        poll_ms: DAEMON_POLL_MS,
    };
    let keyless_stack = netstack(60);
    let stack = netstack(if cable { 150 } else { 90 });
    let run_daemon = |stack: &Program<'_>, args: &[&[u8]], key: &[(ResourceType, u64, Rights)]| {
        let daemon = start(stack, args, key);
        daemon.map(|d| finish(stack, &d))
    };
    // A mode the daemon does not know is refused before it attaches.
    let unknown = run_daemon(
        &keyless_stack,
        &[b"netstack", b"nic-check", SERVICE, b"wifi"],
        &[],
    );
    if unknown != Ok(Some(check::USAGE)) {
        serial_println!(
            "[linuxdrv]   FAIL: the netstack daemon given a mode it does not know ended {:?} ({}): it should have refused it, exit {}",
            unknown,
            nic_check_outcome(&unknown),
            check::USAGE
        );
        spawn::teardown_fixture(driver.pid, driver.task_id);
        return Err(KernelError::InternalError);
    }
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
        &keyed,
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
    let sockets = checked == Some(check::OK) && serve_sockets(service, cable);
    let served = if sockets {
        finish(program, &driver)
    } else {
        stop(&driver)
    };
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
    say_exit_meaning("e1000e", served);
    Err(KernelError::InternalError)
}

/// Sockets served over the card: the netstack daemon as `serve-dns
/// nic.spare` -- the driver's second stack -- holding the card's key
/// (`card_key`) and the right to register `net.stack`. Once it has, attached
/// and given an address by DHCP on the card, the kernel asks it to resolve a
/// name, as a socket client asks, and it must answer: the name resolved, or
/// not -- the boot test's host may have no upstream DNS, which is said, the
/// card's frames having been proved by `nic-check` already. With `cable`
/// -- the harness pulling the card's cable on QEMU's network
/// ([`cable_puller`]) -- it is pulled and plugged back under the daemon,
/// asked all the while ([`keep_asking`]),
/// which must then confirm its lease and announce its addresses. Idle after,
/// the daemon lets the card go and ends 0 -- not `check::LEASE`, its exit when
/// a link came back and its lease was never confirmed. Whether all of that
/// held; what did not is said.
fn serve_sockets(card_key: u64, cable: bool) -> bool {
    let program = Program {
        elf: spawn::NETSTACK_ELF,
        name: "netstack-serve",
        driver: false,
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
    let relinking = cable;
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
            if let Ok(Some(code)) = outcome
                && code != want
            {
                say_exit_meaning(argv.first().map_or("?", text), Some(code));
            }
            Err(KernelError::InternalError)
        }
    }
}

/// Under a FAIL line, what the `who` process's exit code `code` says of how
/// it ended ([`exit_meaning`]), when it says anything.
fn say_exit_meaning(who: &str, code: Option<i32>) {
    if let Some(why) = code.and_then(exit_meaning) {
        serial_println!("[linuxdrv]   ... the {who} process ended so because {why}");
    }
}
