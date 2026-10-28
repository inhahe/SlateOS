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

use crate::error::{KernelError, KernelResult};
use crate::proc::{pcb, spawn};
use crate::serial_println;

/// The name the boot test gives the hello process (`module_cmdline:`).
const HELLO: &[u8] = b"linuxdrv/hello";

/// Exit codes a driver process ends with (`SLX_EXIT_*` in
/// `linuxdrv/emul/slx.h`; `lxkit::exit`).
const EXIT_OK: i32 = 0;
const EXIT_DEADLOCK: i32 = 124;
const EXIT_STOP: i32 = 125;
const EXIT_INIT_FAILED: i32 = 126;

/// How a process the kernel killed for touching memory it may not ends: a
/// driver process overflowing a task's stack into the guard page below it.
const KILLED_ACCESS_VIOLATION: i32 =
    pcb::crash_exit_code(crate::proc::exception::ExceptionCode::AccessViolation as u64);

/// Run `elf` with `argv`, and answer how it ended: its exit code, or `None`
/// when it did not end within 10 s.
fn run(elf: &[u8], argv: &[&[u8]]) -> Result<Option<i32>, KernelError> {
    let options = spawn::SpawnOptions {
        name: "lx-hello",
        parent: 0,
        priority: crate::sched::task::DEFAULT_PRIORITY,
        capabilities: &[],
        fd_map: &[],
        argv,
        envp: &[],
        exe_path: None,
        cwd: None,
        identity: None,
    };
    let process = spawn::spawn_process(elf, &options)?;
    let deadline = crate::hrtimer::now_ns().saturating_add(10_000_000_000);
    let finished = spawn::wait_for_fixture(process.pid, deadline, 0);
    let code = pcb::exit_code(process.pid);
    spawn::teardown_fixture(process.pid, process.task_id);
    Ok(if finished { code } else { None })
}

/// The layer's first process, `linuxdrv/hello`: Linux's headers and its own
/// vsprintf in a process of their own. Run plainly, its initcalls allocate,
/// take a spinlock with interrupts "off", print a line formatted by Linux,
/// run a kthread against a mutex held across a sleep and a completion, and
/// try the scheduler's edges, and the process exits 0. With one argument
/// the first initcall ends it otherwise: "stop" calls a function nothing
/// defines, which the build made a generated stop ([`EXIT_STOP`]);
/// "deadlock" waits for what never comes ([`EXIT_DEADLOCK`]); "overflow"
/// runs past its task's stack, into the guard page below it
/// ([`KILLED_ACCESS_VIOLATION`]); "fail" starts a kthread that never ends
/// and fails, which must end the process at once ([`EXIT_INIT_FAILED`]).
/// Skipped, and recorded so, where the
/// bootloader did not load it -- anywhere but the boot test, and after a
/// kexec.
///
/// # Errors
///
/// `InternalError` when either run ends otherwise.
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
    let cases: [(&[&[u8]], i32, &str); 5] = [
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
    ];
    for (argv, want, what) in cases {
        match run(elf, argv) {
            Ok(Some(code)) if code == want => {}
            outcome => {
                serial_println!(
                    "[linuxdrv]   FAIL: the hello process, which should have {}, ended {:?} (Ok(Some(code)), Ok(None) for not within 10 s)",
                    what,
                    outcome
                );
                return Err(KernelError::InternalError);
            }
        }
    }
    serial_println!(
        "[linuxdrv]   hello process ({} bytes): Linux's headers and vsprintf in a process of their own, its initcalls run (a kthread, a mutex held across a sleep, a completion, the scheduler's edges), a generated stop, a deadlock, a stack overflow caught by its guard page, and a failed initcall: OK",
        elf.len()
    );
    Ok(())
}
