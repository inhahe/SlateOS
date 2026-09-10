//! Slate OS Process Termination Utility
//!
//! Sends termination messages to processes. In Slate OS, process control uses
//! IPC messages rather than Unix signals. The `kill` utility first attempts
//! a graceful shutdown via IPC (giving the target process a chance to clean
//! up), then falls back to the `SYS_PROCESS_KILL` syscall for forceful
//! termination.
//!
//! Also provides `killall` mode (invoked as `killall` or via `kill --name`)
//! to terminate processes by name rather than PID.
//!
//! # Usage
//!
//! ```text
//! kill <pid> [pid...]              Graceful terminate (default)
//! kill -9 <pid>                    Force kill (immediate, no IPC)
//! kill -KILL <pid>                 Same as -9
//! kill -TERM <pid>                 Graceful terminate (default)
//! kill -STOP <pid>                 Pause process
//! kill -CONT <pid>                 Resume process
//! kill -HUP <pid>                  Hangup / restart
//! kill -INT <pid>                  Interrupt
//! kill -0 <pid>                    Check if process exists
//! kill -l                          List available signal names
//! kill -v <pid>                    Verbose output
//! kill -q <pid>                    Quiet (suppress errors)
//! kill -w <pid>                    Wait for process to die
//! kill --timeout <secs> <pid>      Force kill after timeout
//! killall <name>                   Kill processes by name
//! killall -i <name>               Interactive confirmation
//! ```

use quoting::quoteaf_os;
use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall numbers (from kernel/src/syscall/number.rs)
// ============================================================================

/// Connect to a named service, returns a channel handle.
///
/// arg0 = pointer to NUL-terminated service name
/// arg1 = name length
/// arg2 = flags (0)
const SYS_SERVICE_CONNECT: u64 = 281;

/// Send a message on an open IPC channel.
///
/// arg0 = channel handle
/// arg1 = pointer to message buffer
/// arg2 = message length
const SYS_CHANNEL_SEND: u64 = 201;

/// Receive a message from an IPC channel (blocking).
///
/// arg0 = channel handle
/// arg1 = pointer to receive buffer
/// arg2 = buffer length
const SYS_CHANNEL_RECV: u64 = 202;

/// Close an IPC channel handle.
///
/// arg0 = channel handle
const SYS_CHANNEL_CLOSE: u64 = 204;

/// Force-terminate a process and all its threads.
///
/// arg0 = target process ID
/// arg1 = exit code to set
///
/// Authority: caller must be parent of target, or PID 0.
/// Returns: number of threads killed on success, negative error on failure.
const SYS_PROCESS_KILL: u64 = 506;

/// Query process existence / basic info.
///
/// arg0 = target PID
/// Returns: 0 if exists, negative error if not.
const SYS_PROCESS_ID: u64 = 502;

/// Suspend (pause) a thread.
///
/// arg0 = task ID
/// Returns: 0 on success.
const SYS_THREAD_SUSPEND: u64 = 513;

/// Resume a suspended thread.
///
/// arg0 = task ID
/// Returns: 0 on success.
const SYS_THREAD_RESUME: u64 = 514;

// ============================================================================
// Low-level syscall interface
// ============================================================================

/// Issue a three-argument syscall using the x86-64 `syscall` instruction.
///
/// Register mapping follows the Slate OS syscall ABI:
///   rax = syscall number, rdi = arg1, rsi = arg2, rdx = arg3
///   Return value in rax. rcx and r11 are clobbered by the CPU.
#[cfg(target_vendor = "slateos")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    // The `syscall` instruction is the defined kernel entry point on x86-64.
    // rcx and r11 are marked as clobbered per the hardware specification.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr as i64 => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

/// Host stub for `syscall3` — see the gated definition above.
///
/// On a development host there is no SlateOS kernel to talk to, and a raw
/// `syscall` instruction does not fail cleanly: it enters whatever kernel is
/// actually running, with this crate's SlateOS call number in RAX. Those
/// numbers mean unrelated things elsewhere, so the call is not a no-op — it
/// is someone else's syscall. Returning `ENOSYS` keeps `cargo test`, `cargo
/// run` and `clippy` on the host honest instead of dangerous.
///
/// See known-issues.md
/// `B-FORTY-SIX-USERSPACE-CRATES-CAN-ISSUE-A-RAW-SYSCALL-ON-THE-DEV-HOST`.
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -38 // ENOSYS
}

// ============================================================================
// Signal compatibility mapping
// ============================================================================

/// Signal entry for the compatibility table. Slate OS does not use Unix signals,
/// but we map traditional signal names/numbers to IPC actions for familiarity.
struct SignalEntry {
    number: u32,
    name: &'static str,
    description: &'static str,
    action: Action,
}

/// What action to take when a particular "signal" is requested.
#[derive(Clone, Copy, PartialEq)]
enum Action {
    /// Check process existence only, do not send anything.
    Probe,
    /// Send an IPC message requesting graceful shutdown.
    GracefulTerm,
    /// Send an IPC message requesting restart.
    Hangup,
    /// Send an IPC message requesting interrupt.
    Interrupt,
    /// Force-kill via SYS_PROCESS_KILL, no IPC attempt.
    ForceKill,
    /// Suspend all threads via SYS_THREAD_SUSPEND.
    Stop,
    /// Resume all threads via SYS_THREAD_RESUME.
    Continue,
    /// Terminal stop (same as Stop for our purposes).
    TerminalStop,
}

/// The compatibility signal table. Numbers follow traditional POSIX values
/// where practical, but the underlying mechanism is always IPC or syscalls.
const SIGNAL_TABLE: &[SignalEntry] = &[
    SignalEntry {
        number: 0,
        name: "NULL",
        description: "Existence check (no action)",
        action: Action::Probe,
    },
    SignalEntry {
        number: 1,
        name: "HUP",
        description: "Hangup / restart",
        action: Action::Hangup,
    },
    SignalEntry {
        number: 2,
        name: "INT",
        description: "Interrupt",
        action: Action::Interrupt,
    },
    SignalEntry {
        number: 9,
        name: "KILL",
        description: "Force kill (immediate)",
        action: Action::ForceKill,
    },
    SignalEntry {
        number: 15,
        name: "TERM",
        description: "Graceful terminate (default)",
        action: Action::GracefulTerm,
    },
    SignalEntry {
        number: 17,
        name: "STOP",
        description: "Pause process",
        action: Action::Stop,
    },
    SignalEntry {
        number: 18,
        name: "CONT",
        description: "Resume process",
        action: Action::Continue,
    },
    SignalEntry {
        number: 19,
        name: "TSTP",
        description: "Terminal stop",
        action: Action::TerminalStop,
    },
];

/// Look up a signal entry by number.
fn signal_by_number(num: u32) -> Option<&'static SignalEntry> {
    SIGNAL_TABLE.iter().find(|s| s.number == num)
}

/// Look up a signal entry by name (case-insensitive, with or without "SIG" prefix).
fn signal_by_name(name: &str) -> Option<&'static SignalEntry> {
    let upper = name.to_uppercase();
    // Strip optional "SIG" prefix: "SIGKILL" -> "KILL"
    let stripped = upper.strip_prefix("SIG").unwrap_or(&upper);
    SIGNAL_TABLE.iter().find(|s| s.name == stripped)
}

// ============================================================================
// IPC helpers
// ============================================================================

/// The process manager's well-known IPC endpoint. Used for sending graceful
/// shutdown/restart requests to processes via the service infrastructure.
const PROCESS_MANAGER_NAME: &[u8] = b"org.slateos.ProcessManager\0";

/// Send an IPC command to the process manager requesting an action on a PID.
///
/// Returns `Ok(response)` if the service acknowledged, `Err(msg)` on failure.
fn send_process_command(command: &str) -> Result<String, String> {
    let msg = format!("{command}\0");

    // SAFETY: SYS_SERVICE_CONNECT takes a pointer to a NUL-terminated service
    // name, its length, and flags (0). The pointer is valid for the duration
    // of the syscall because `PROCESS_MANAGER_NAME` is a static byte string.
    let channel = unsafe {
        syscall3(
            SYS_SERVICE_CONNECT,
            PROCESS_MANAGER_NAME.as_ptr() as u64,
            PROCESS_MANAGER_NAME.len() as u64,
            0,
        )
    };

    if channel < 0 {
        return Err(format!(
            "cannot connect to process manager (error {channel})"
        ));
    }

    let ch = channel as u64;

    // SAFETY: SYS_CHANNEL_SEND takes the channel handle, a pointer to the
    // message buffer, and its length. `msg` lives on the stack and outlives
    // the syscall.
    let send_ret = unsafe { syscall3(SYS_CHANNEL_SEND, ch, msg.as_ptr() as u64, msg.len() as u64) };

    if send_ret < 0 {
        // SAFETY: SYS_CHANNEL_CLOSE takes the handle and two unused args.
        let _ = unsafe { syscall3(SYS_CHANNEL_CLOSE, ch, 0, 0) };
        return Err(format!("IPC send failed (error {send_ret})"));
    }

    // Receive the response.
    let mut buf = [0u8; 4096];

    // SAFETY: SYS_CHANNEL_RECV takes the channel handle, a pointer to a
    // writable buffer, and the buffer length. `buf` is stack-allocated and
    // outlives the syscall.
    let recv_ret = unsafe {
        syscall3(
            SYS_CHANNEL_RECV,
            ch,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };

    // Always close the channel, even if recv failed.
    // SAFETY: ch is a valid channel handle from a successful connect.
    let _ = unsafe { syscall3(SYS_CHANNEL_CLOSE, ch, 0, 0) };

    if recv_ret < 0 {
        return Err(format!("IPC recv failed (error {recv_ret})"));
    }

    let len = (recv_ret as usize).min(buf.len());
    let response = String::from_utf8_lossy(&buf[..len]).to_string();
    Ok(response)
}

/// Send a graceful shutdown request for a process via IPC.
fn ipc_graceful_terminate(pid: u64) -> Result<(), String> {
    let cmd = format!("PROCESS_TERMINATE {pid}");
    let resp = send_process_command(&cmd)?;
    let trimmed = resp.trim().trim_end_matches('\0');
    if trimmed.starts_with("OK") || trimmed.starts_with("ACK") {
        Ok(())
    } else {
        Err(format!("process manager: {trimmed}"))
    }
}

/// Send a hangup/restart request for a process via IPC.
fn ipc_hangup(pid: u64) -> Result<(), String> {
    let cmd = format!("PROCESS_HANGUP {pid}");
    let resp = send_process_command(&cmd)?;
    let trimmed = resp.trim().trim_end_matches('\0');
    if trimmed.starts_with("OK") || trimmed.starts_with("ACK") {
        Ok(())
    } else {
        Err(format!("process manager: {trimmed}"))
    }
}

/// Send an interrupt request for a process via IPC.
fn ipc_interrupt(pid: u64) -> Result<(), String> {
    let cmd = format!("PROCESS_INTERRUPT {pid}");
    let resp = send_process_command(&cmd)?;
    let trimmed = resp.trim().trim_end_matches('\0');
    if trimmed.starts_with("OK") || trimmed.starts_with("ACK") {
        Ok(())
    } else {
        Err(format!("process manager: {trimmed}"))
    }
}

// ============================================================================
// Process operations via syscall
// ============================================================================

/// Check whether a process exists by attempting to query its PID.
fn process_exists(pid: u64) -> bool {
    // SAFETY: SYS_PROCESS_ID with the target PID, two unused args.
    // Returns 0 (or the PID itself) on success, negative on not-found.
    let ret = unsafe { syscall3(SYS_PROCESS_ID, pid, 0, 0) };
    ret >= 0
}

/// Force-terminate a process via `SYS_PROCESS_KILL`.
///
/// Returns `Ok(threads_killed)` on success, `Err(errno)` on failure.
fn force_kill(pid: u64, exit_code: u64) -> Result<i64, i64> {
    // SAFETY: SYS_PROCESS_KILL takes the target PID and exit code.
    // The caller must be the parent or PID 0. On success it returns the
    // number of threads killed; on failure a negative error code.
    let ret = unsafe { syscall3(SYS_PROCESS_KILL, pid, exit_code, 0) };
    if ret >= 0 { Ok(ret) } else { Err(ret) }
}

/// Suspend all threads of a process.
///
/// Currently this operates at the thread level. We read /proc/<pid>/task/
/// to enumerate threads and suspend each one.
fn stop_process(pid: u64) -> Result<u32, String> {
    let task_dir = format!("/proc/{pid}/task");
    let entries = fs::read_dir(&task_dir).map_err(|e| format!("cannot read {task_dir}: {e}"))?;

    let mut count = 0u32;
    let mut last_err: Option<String> = None;

    for entry in entries.flatten() {
        let tid_str = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let tid: u64 = match tid_str.parse() {
            Ok(t) => t,
            Err(_) => continue,
        };

        // SAFETY: SYS_THREAD_SUSPEND takes the task ID. The target thread
        // must belong to a process the caller owns.
        let ret = unsafe { syscall3(SYS_THREAD_SUSPEND, tid, 0, 0) };
        if ret < 0 {
            last_err = Some(format!("suspend tid {tid} failed (error {ret})"));
        } else {
            count = count.saturating_add(1);
        }
    }

    if count == 0 {
        Err(last_err.unwrap_or_else(|| "no threads found".to_string()))
    } else {
        Ok(count)
    }
}

/// Resume all threads of a process.
fn continue_process(pid: u64) -> Result<u32, String> {
    let task_dir = format!("/proc/{pid}/task");
    let entries = fs::read_dir(&task_dir).map_err(|e| format!("cannot read {task_dir}: {e}"))?;

    let mut count = 0u32;
    let mut last_err: Option<String> = None;

    for entry in entries.flatten() {
        let tid_str = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let tid: u64 = match tid_str.parse() {
            Ok(t) => t,
            Err(_) => continue,
        };

        // SAFETY: SYS_THREAD_RESUME takes the task ID. The target thread
        // must belong to a process the caller owns and be in Suspended state.
        let ret = unsafe { syscall3(SYS_THREAD_RESUME, tid, 0, 0) };
        if ret < 0 {
            last_err = Some(format!("resume tid {tid} failed (error {ret})"));
        } else {
            count = count.saturating_add(1);
        }
    }

    if count == 0 {
        Err(last_err.unwrap_or_else(|| "no threads found".to_string()))
    } else {
        Ok(count)
    }
}

// ============================================================================
// Process name resolution (for killall mode)
// ============================================================================

/// The process name, **as bytes**, through [`procinfo`].
///
/// `read_to_string` on `/proc/<pid>/stat` *fails* when the name is not UTF-8,
/// and the `.ok()` made that a `None` -- so `killall <name>` could not see such
/// a process at all, and it could not be killed by name. Exactly the shape
/// `pgrep`/`pkill` had.
///
/// Bytes rather than a `String` for the comparison as well as the read: the
/// target comes from `argv`, which is bytes on this system, and comparing what
/// the kernel reported against what the user typed is a byte comparison or it
/// is a guess.
fn read_process_name(pid: u64) -> Option<Vec<u8>> {
    Some(
        procinfo::ProcFs::new()
            .process_stat(pid)
            .ok()
            .flatten()?
            .comm,
    )
}

/// Find all PIDs whose process name matches the given target name.
fn find_pids_by_name(target: &str) -> Vec<u64> {
    let mut pids = Vec::new();

    let entries = match fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return pids,
    };

    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        // Only consider numeric directory names (PIDs).
        let pid: u64 = match name.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Bytes on both sides. `target` is what the user typed and
        // `proc_name` is what the kernel reported; comparing them after
        // decoding either one is a comparison of two guesses.
        if let Some(proc_name) = read_process_name(pid)
            && proc_name == target.as_bytes()
        {
            pids.push(pid);
        }
    }

    pids.sort_unstable();
    pids
}

// ============================================================================
// Waiting for process death
// ============================================================================

/// Poll /proc/<pid>/stat until the process disappears or a timeout expires.
///
/// Returns `true` if the process died within the timeout, `false` if it is
/// still alive.
fn wait_for_death(pid: u64, timeout_secs: u64) -> bool {
    // Each poll sleeps ~50ms. Compute the number of polls from the timeout.
    let polls = timeout_secs.saturating_mul(20).max(1);
    let stat_path = format!("/proc/{pid}/stat");

    for _ in 0..polls {
        if fs::metadata(&stat_path).is_err() {
            return true; // process is gone
        }
        // Busy-wait briefly. We do not have usleep/nanosleep as a direct
        // syscall wrapper yet, so we use a small spin. In practice the
        // kernel's scheduler will preempt us.
        spin_wait_ms(50);
    }

    // Final check after the loop.
    fs::metadata(&stat_path).is_err()
}

/// Crude spin-wait for approximately `ms` milliseconds.
///
/// This is a stopgap until a proper sleep syscall wrapper is available.
/// The volatile read prevents the compiler from optimizing the loop away.
fn spin_wait_ms(ms: u64) {
    // Approximate iteration count calibrated for a ~2GHz CPU.
    // This does not need to be precise — we are just yielding time.
    let iterations = ms.saturating_mul(200_000);
    for i in 0..iterations {
        // SAFETY: Reading from a valid stack variable. The volatile read
        // prevents dead-code elimination of the loop body.
        unsafe {
            core::ptr::read_volatile(&i);
        }
    }
}

// ============================================================================
// Interactive confirmation (for killall -i)
// ============================================================================

/// Ask the user for yes/no confirmation on stderr/stdin.
///
/// Returns `true` if the user answers yes.
fn confirm(prompt: &str) -> bool {
    eprint!("{prompt} [y/N] ");

    let mut buf = String::new();
    match std::io::stdin().read_line(&mut buf) {
        Ok(_) => {
            let answer = buf.trim().to_lowercase();
            answer == "y" || answer == "yes"
        }
        Err(_) => false,
    }
}

// ============================================================================
// Kill execution
// ============================================================================

/// Describes the outcome of killing a single process.
struct KillResult {
    success: bool,
    message: String,
}

/// Execute the requested action on a single PID.
fn execute_kill(pid: u64, action: Action, verbose: bool) -> KillResult {
    match action {
        Action::Probe => {
            if process_exists(pid) {
                KillResult {
                    success: true,
                    message: format!("process {pid} exists"),
                }
            } else {
                KillResult {
                    success: false,
                    message: format!("no such process: {pid}"),
                }
            }
        }

        Action::GracefulTerm => {
            if verbose {
                eprintln!("kill: sending graceful terminate to {pid}");
            }

            // Try IPC first for clean shutdown.
            match ipc_graceful_terminate(pid) {
                Ok(()) => {
                    if verbose {
                        eprintln!("kill: IPC terminate acknowledged for {pid}");
                    }
                    return KillResult {
                        success: true,
                        message: format!("graceful terminate sent to {pid}"),
                    };
                }
                Err(e) => {
                    if verbose {
                        eprintln!("kill: IPC failed for {pid} ({e}), falling back to syscall");
                    }
                }
            }

            // Fallback: direct kill with exit code 143 (SIGTERM equivalent).
            match force_kill(pid, killconv::exit_code_for_signal(killconv::SIGTERM)) {
                Ok(threads) => KillResult {
                    success: true,
                    message: format!("terminated {pid} ({threads} thread(s))"),
                },
                Err(errno) => KillResult {
                    success: false,
                    message: format!("failed to terminate {pid}: {}", errno_to_string(errno)),
                },
            }
        }

        Action::ForceKill => {
            if verbose {
                eprintln!("kill: force killing {pid}");
            }
            // No IPC attempt — go straight to the kernel.
            match force_kill(pid, killconv::exit_code_for_signal(killconv::SIGKILL)) {
                Ok(threads) => KillResult {
                    success: true,
                    message: format!("killed {pid} ({threads} thread(s))"),
                },
                Err(errno) => KillResult {
                    success: false,
                    message: format!("failed to kill {pid}: {}", errno_to_string(errno)),
                },
            }
        }

        Action::Hangup => {
            if verbose {
                eprintln!("kill: sending HUP to {pid}");
            }
            match ipc_hangup(pid) {
                Ok(()) => KillResult {
                    success: true,
                    message: format!("HUP sent to {pid}"),
                },
                Err(e) => {
                    if verbose {
                        eprintln!("kill: IPC HUP failed ({e}), falling back to terminate");
                    }
                    // Fallback: terminate with HUP-equivalent code.
                    match force_kill(pid, killconv::exit_code_for_signal(killconv::SIGHUP)) {
                        Ok(threads) => KillResult {
                            success: true,
                            message: format!("terminated {pid} with HUP ({threads} thread(s))"),
                        },
                        Err(errno) => KillResult {
                            success: false,
                            message: format!("failed to HUP {pid}: {}", errno_to_string(errno)),
                        },
                    }
                }
            }
        }

        Action::Interrupt => {
            if verbose {
                eprintln!("kill: sending INT to {pid}");
            }
            match ipc_interrupt(pid) {
                Ok(()) => KillResult {
                    success: true,
                    message: format!("INT sent to {pid}"),
                },
                Err(e) => {
                    if verbose {
                        eprintln!("kill: IPC INT failed ({e}), falling back to terminate");
                    }
                    match force_kill(pid, killconv::exit_code_for_signal(killconv::SIGINT)) {
                        Ok(threads) => KillResult {
                            success: true,
                            message: format!("terminated {pid} with INT ({threads} thread(s))"),
                        },
                        Err(errno) => KillResult {
                            success: false,
                            message: format!("failed to INT {pid}: {}", errno_to_string(errno)),
                        },
                    }
                }
            }
        }

        Action::Stop => {
            if verbose {
                eprintln!("kill: stopping {pid}");
            }
            match stop_process(pid) {
                Ok(count) => KillResult {
                    success: true,
                    message: format!("stopped {pid} ({count} thread(s) suspended)"),
                },
                Err(e) => KillResult {
                    success: false,
                    message: format!("failed to stop {pid}: {e}"),
                },
            }
        }

        Action::Continue => {
            if verbose {
                eprintln!("kill: resuming {pid}");
            }
            match continue_process(pid) {
                Ok(count) => KillResult {
                    success: true,
                    message: format!("resumed {pid} ({count} thread(s))"),
                },
                Err(e) => KillResult {
                    success: false,
                    message: format!("failed to resume {pid}: {e}"),
                },
            }
        }

        Action::TerminalStop => {
            // Identical to Stop for our purposes.
            if verbose {
                eprintln!("kill: terminal-stopping {pid}");
            }
            match stop_process(pid) {
                Ok(count) => KillResult {
                    success: true,
                    message: format!("stopped {pid} ({count} thread(s) suspended)"),
                },
                Err(e) => KillResult {
                    success: false,
                    message: format!("failed to stop {pid}: {e}"),
                },
            }
        }
    }
}

/// Map a negative syscall return code to a human-readable error string.
fn errno_to_string(errno: i64) -> String {
    match errno {
        -1 => "operation not permitted (EPERM)".to_string(),
        -2 => "no such file or directory (ENOENT)".to_string(),
        -3 => "no such process (ESRCH)".to_string(),
        -13 => "permission denied (EACCES)".to_string(),
        -22 => "invalid argument (EINVAL)".to_string(),
        other => format!("error {other}"),
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parsed command-line options.
struct Options {
    /// The action to perform (default: GracefulTerm).
    action: Action,
    /// Target PIDs to operate on.
    pids: Vec<u64>,
    /// If true, we are in killall mode (match by name).
    killall_mode: bool,
    /// Process name to match in killall mode.
    target_name: Option<String>,
    /// Interactive confirmation for killall.
    interactive: bool,
    /// Wait for processes to actually terminate.
    wait: bool,
    /// Timeout (seconds) before escalating to force kill.
    timeout: Option<u64>,
    /// Verbose output.
    verbose: bool,
    /// Quiet mode (suppress errors).
    quiet: bool,
    /// Just list signals.
    list_signals: bool,
}

/// Detect whether we were invoked as "killall" (by argv[0]).
fn is_killall_invocation(argv0: &str) -> bool {
    // Check the basename of argv[0].
    let basename = argv0.rsplit('/').next().unwrap_or(argv0);
    basename == "killall"
}

/// Parse command-line arguments into an `Options` struct.
///
/// Returns `Err(message)` for usage errors.
fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        action: Action::GracefulTerm,
        pids: Vec::new(),
        killall_mode: false,
        target_name: None,
        interactive: false,
        wait: false,
        timeout: None,
        verbose: false,
        quiet: false,
        list_signals: false,
    };

    if args.is_empty() {
        return Err("no arguments provided".to_string());
    }

    // Check if invoked as killall.
    opts.killall_mode = is_killall_invocation(&args[0]);

    let mut i = 1; // skip argv[0]
    let mut explicit_signal = false;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--help" || arg == "-h" {
            return Err(String::new()); // triggers usage display
        }

        if arg == "-l" || arg == "--list" {
            opts.list_signals = true;
            return Ok(opts);
        }

        if arg == "-v" || arg == "--verbose" {
            opts.verbose = true;
            i += 1;
            continue;
        }

        if arg == "-q" || arg == "--quiet" {
            opts.quiet = true;
            i += 1;
            continue;
        }

        if arg == "-w" || arg == "--wait" {
            opts.wait = true;
            i += 1;
            continue;
        }

        if arg == "-i" || arg == "--interactive" {
            opts.interactive = true;
            i += 1;
            continue;
        }

        if arg == "--name" {
            // Switch to killall mode even if invoked as "kill".
            opts.killall_mode = true;
            i += 1;
            continue;
        }

        if arg == "--timeout" {
            i += 1;
            if i >= args.len() {
                return Err("--timeout requires a value".to_string());
            }
            opts.timeout = Some(
                args[i]
                    .parse::<u64>()
                    .map_err(|_| format!("invalid timeout: {}", args[i]))?,
            );
            i += 1;
            continue;
        }

        // Signal specification: -<number> or -<NAME>
        if let Some(stripped) = arg.strip_prefix('-')
            && !stripped.is_empty()
            && !explicit_signal
        {
            // Try as a number first.
            if let Ok(num) = stripped.parse::<u32>() {
                if let Some(entry) = signal_by_number(num) {
                    opts.action = entry.action;
                    explicit_signal = true;
                    i += 1;
                    continue;
                }
                return Err(format!("unknown signal number: {num}"));
            }

            // Try as a name.
            if let Some(entry) = signal_by_name(stripped) {
                opts.action = entry.action;
                explicit_signal = true;
                i += 1;
                continue;
            }

            return Err(format!("unknown signal: {stripped}"));
        }

        // Remaining arguments are PIDs or process names.
        if opts.killall_mode {
            opts.target_name = Some(arg.clone());
        } else {
            // Parse as PID.
            match arg.parse::<u64>() {
                Ok(pid) => opts.pids.push(pid),
                Err(_) => {
                    // If it looks like a name, suggest killall.
                    return Err(format!("invalid PID: {arg} (use killall to kill by name)"));
                }
            }
        }

        i += 1;
    }

    // Validate: need at least one target.
    if !opts.list_signals && opts.pids.is_empty() && opts.target_name.is_none() {
        return Err("no process specified".to_string());
    }

    Ok(opts)
}

// ============================================================================
// Signal listing
// ============================================================================

fn print_signal_list() {
    println!("Available signal names (Slate OS compatibility mapping):");
    println!();
    println!(" {:>3}  {:<6}  Description", "Num", "Name");
    println!(" {:>3}  {:<6}  -----------", "---", "------");
    for entry in SIGNAL_TABLE {
        println!(
            " {:>3}  {:<6}  {}",
            entry.number, entry.name, entry.description
        );
    }
    println!();
    println!("Note: Slate OS uses IPC messages, not Unix signals.");
    println!("Signal names are provided for familiarity only.");
}

// ============================================================================
// Usage / help
// ============================================================================

fn print_usage(is_killall: bool) {
    if is_killall {
        println!("Slate OS killall v0.1.0 -- Kill processes by name");
        println!();
        println!("USAGE:");
        println!("  killall [options] <name>");
        println!();
        println!("OPTIONS:");
        println!("  -TERM, -15          Graceful terminate (default)");
        println!("  -KILL, -9           Force kill (no IPC attempt)");
        println!("  -HUP, -1            Hangup / restart");
        println!("  -INT, -2            Interrupt");
        println!("  -STOP, -17          Pause process");
        println!("  -CONT, -18          Resume process");
        println!("  -0                  Check if any matching process exists");
        println!("  -i, --interactive   Confirm before each kill");
        println!("  -w, --wait          Wait for processes to terminate");
        println!("  --timeout <secs>    Force kill if not dead after timeout");
        println!("  -v, --verbose       Show detailed output");
        println!("  -q, --quiet         Suppress error messages");
        println!("  -l, --list          List signal names");
        println!();
        println!("EXAMPLES:");
        println!("  killall myserver           Graceful terminate all 'myserver'");
        println!("  killall -9 myserver        Force kill all 'myserver'");
        println!("  killall -i -w myserver     Confirm and wait for each");
    } else {
        println!("Slate OS kill v0.1.0 -- Send termination messages to processes");
        println!();
        println!("USAGE:");
        println!("  kill [options] <pid> [pid...]");
        println!("  kill --name [options] <name>");
        println!();
        println!("OPTIONS:");
        println!("  -TERM, -15          Graceful terminate (default)");
        println!("  -KILL, -9           Force kill (no IPC attempt)");
        println!("  -HUP, -1            Hangup / restart");
        println!("  -INT, -2            Interrupt");
        println!("  -STOP, -17          Pause process");
        println!("  -CONT, -18          Resume process");
        println!("  -0                  Check if process exists");
        println!("  --name              Kill by process name (killall mode)");
        println!("  -w, --wait          Wait for processes to terminate");
        println!("  --timeout <secs>    Force kill if not dead after timeout");
        println!("  -v, --verbose       Show detailed output");
        println!("  -q, --quiet         Suppress error messages");
        println!("  -l, --list          List signal names");
        println!();
        println!("EXAMPLES:");
        println!("  kill 42                    Graceful terminate PID 42");
        println!("  kill -9 42 43              Force kill PIDs 42 and 43");
        println!("  kill -0 42                 Check if PID 42 exists");
        println!("  kill -w --timeout 5 42     Kill 42, wait up to 5s, then force");
        println!("  kill --name myserver       Kill all processes named 'myserver'");
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let is_killall = args
        .first()
        .map(|a| is_killall_invocation(a))
        .unwrap_or(false);

    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(msg) => {
            if msg.is_empty() {
                // --help was requested
                print_usage(is_killall);
                process::exit(0);
            }
            eprintln!("kill: {msg}");
            eprintln!("Try 'kill --help' for usage information.");
            process::exit(1);
        }
    };

    if opts.list_signals {
        print_signal_list();
        process::exit(0);
    }

    // Resolve target PIDs. In killall mode, we look up by name.
    let target_pids: Vec<u64> = if opts.killall_mode {
        let name = match &opts.target_name {
            Some(n) => n,
            None => {
                eprintln!("kill: no process name specified");
                process::exit(1);
            }
        };

        let pids = find_pids_by_name(name);
        if pids.is_empty() {
            if !opts.quiet {
                eprintln!("kill: no process found with name {}", quoteaf_os(name));
            }
            process::exit(1);
        }

        if opts.verbose {
            eprintln!(
                "kill: found {} process(es) named {}: {:?}",
                pids.len(),
                quoteaf_os(name),
                pids
            );
        }

        pids
    } else {
        opts.pids.clone()
    };

    let mut any_failed = false;

    for &pid in &target_pids {
        // Interactive confirmation for killall -i.
        if opts.interactive {
            let name = opts.target_name.as_deref().unwrap_or("?");
            if !confirm(&format!("Kill {name} (pid {pid})?")) {
                continue;
            }
        }

        let result = execute_kill(pid, opts.action, opts.verbose);

        if result.success {
            if opts.verbose {
                eprintln!("kill: {}", result.message);
            }
        } else {
            any_failed = true;
            if !opts.quiet {
                eprintln!("kill: {}", result.message);
            }
        }

        // Wait for process death if requested.
        if opts.wait && result.success && opts.action != Action::Probe {
            let timeout = opts.timeout.unwrap_or(30);

            if opts.verbose {
                eprintln!("kill: waiting for {pid} to terminate (timeout {timeout}s)...");
            }

            if !wait_for_death(pid, timeout) {
                // Process is still alive after timeout. Escalate to force kill
                // if we were not already force-killing.
                if opts.action != Action::ForceKill {
                    if opts.verbose {
                        eprintln!(
                            "kill: {pid} still alive after {timeout}s, escalating to force kill"
                        );
                    }

                    let escalated = execute_kill(pid, Action::ForceKill, opts.verbose);
                    if !escalated.success {
                        any_failed = true;
                        if !opts.quiet {
                            eprintln!("kill: {}", escalated.message);
                        }
                    } else if opts.verbose {
                        eprintln!("kill: {}", escalated.message);
                    }
                } else {
                    // Already was a force kill and it didn't die. Report failure.
                    any_failed = true;
                    if !opts.quiet {
                        eprintln!(
                            "kill: {pid} did not terminate within {timeout}s after force kill"
                        );
                    }
                }
            } else if opts.verbose {
                eprintln!("kill: {pid} terminated");
            }
        }

        // --timeout without --wait: escalate after timeout if the process
        // is still alive. This is a convenience shorthand.
        if !opts.wait
            && opts.timeout.is_some()
            && result.success
            && opts.action != Action::ForceKill
            && opts.action != Action::Probe
        {
            let timeout = opts.timeout.unwrap_or(5);

            if !wait_for_death(pid, timeout) {
                if opts.verbose {
                    eprintln!("kill: {pid} still alive after {timeout}s, escalating to force kill");
                }

                let escalated = execute_kill(pid, Action::ForceKill, opts.verbose);
                if !escalated.success {
                    any_failed = true;
                    if !opts.quiet {
                        eprintln!("kill: {}", escalated.message);
                    }
                } else if opts.verbose {
                    eprintln!("kill: {}", escalated.message);
                }
            }
        }
    }

    if any_failed {
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

/// **This crate had no tests at all before 2026-09-10.** A program whose whole
/// job is to send signals, with a signal-name table, a `killall` personality
/// selected by `argv[0]`, and an argument parser -- and nothing asserted about
/// any of it.
///
/// What is covered is the part that is a pure function of its input. Sending a
/// signal is not: it needs a live process, and a test that sent one would be
/// testing the kernel.
#[cfg(test)]
mod tests {
    use super::{is_killall_invocation, signal_by_name, signal_by_number};

    /// Both spellings reach the same signal. `kill -TERM` and `kill -SIGTERM`
    /// are both in every script anyone has ever written.
    #[test]
    fn a_signal_is_found_with_or_without_the_sig_prefix() {
        let bare = signal_by_name("TERM").expect("TERM is a signal");
        let prefixed = signal_by_name("SIGTERM").expect("SIGTERM is the same signal");
        assert_eq!(bare.number, prefixed.number);
        assert_eq!(bare.name, "TERM");
    }

    /// And case does not matter, which is what `to_uppercase` is for.
    #[test]
    fn a_signal_name_is_case_insensitive() {
        let lower = signal_by_name("sigkill").expect("sigkill is SIGKILL");
        assert_eq!(lower.name, "KILL");
    }

    /// A name that is not a signal is `None`, not a default.
    ///
    /// The direction that matters: `kill -SIGFOO 123` must refuse rather than
    /// quietly picking a signal, because the caller believes they asked for
    /// something specific.
    #[test]
    fn an_unknown_signal_name_is_refused() {
        assert!(signal_by_name("NOTASIGNAL").is_none());
        assert!(signal_by_name("").is_none());
        // "SIG" alone strips to the empty string, which must not match the
        // first entry or anything else.
        assert!(signal_by_name("SIG").is_none());
    }

    /// Signal 0 is a real signal -- the existence check -- and must not be
    /// confused with "no signal".
    #[test]
    fn signal_zero_is_the_existence_check() {
        let null = signal_by_number(0).expect("0 is a signal");
        assert_eq!(null.name, "NULL");
    }

    /// Every entry is reachable by both of its keys, and the two agree. A
    /// table is exactly the kind of thing that grows an entry reachable by
    /// number and not by name.
    #[test]
    fn every_table_entry_is_reachable_by_number_and_by_name() {
        for entry in super::SIGNAL_TABLE {
            let by_number = signal_by_number(entry.number).expect("reachable by number");
            let by_name = signal_by_name(entry.name).expect("reachable by name");
            assert_eq!(by_number.name, entry.name);
            assert_eq!(by_name.number, entry.number);
        }
    }

    /// The `killall` personality is chosen by the **basename** of `argv[0]`,
    /// so an absolute path selects it and a longer name does not.
    #[test]
    fn killall_is_selected_by_the_basename_of_argv0() {
        assert!(is_killall_invocation("killall"));
        assert!(is_killall_invocation("/usr/bin/killall"));
        assert!(!is_killall_invocation("kill"));
        assert!(!is_killall_invocation("/usr/bin/kill"));
        // Not a prefix or suffix match: these are different programs.
        assert!(!is_killall_invocation("killall5"));
        assert!(!is_killall_invocation("mykillall"));
    }
}
