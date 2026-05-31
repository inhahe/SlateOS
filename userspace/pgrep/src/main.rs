//! OurOS Process Grep / Kill Utility
//!
//! Multi-personality binary: detects its invocation name from `argv[0]` to
//! act as either `pgrep` (find processes by pattern) or `pkill` (send signals
//! to processes matching a pattern).
//!
//! # Usage
//!
//! ```text
//! pgrep [options] <pattern>
//! pkill [options] <pattern>
//! ```
//!
//! ## Common options
//!
//! - `-f`  / `--full`          Match against full command line
//! - `-x`  / `--exact`         Require exact name match
//! - `-i`  / `--ignore-case`   Case-insensitive matching
//! - `-n`  / `--newest`        Select only the newest match
//! - `-o`  / `--oldest`        Select only the oldest match
//! - `-v`  / `--inverse`       Negate matching
//! - `-P`  / `--parent=PPID`   Match children of given parent
//! - `-u`  / `--euid=USER`     Match by effective UID
//! - `-U`  / `--uid=USER`      Match by real UID
//! - `-g`  / `--pgroup=PGRP`   Match by process group
//! - `-t`  / `--terminal=TTY`  Match by controlling terminal
//!
//! ## pgrep-specific
//!
//! - `-c`  / `--count`         Print match count instead of PIDs
//! - `-d`  / `--delimiter=D`   Output delimiter (default: newline)
//! - `-l`  / `--list-name`     Also print process name
//! - `-a`  / `--list-full`     Also print full command line
//!
//! ## pkill-specific
//!
//! - `--signal=SIG` / `-SIG`   Signal to send (default: TERM)
//! - `-e`  / `--echo`          Print what was killed

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Syscall numbers (from kernel/src/syscall/number.rs)
// ============================================================================

/// Connect to a named service, returns a channel handle.
const SYS_SERVICE_CONNECT: u64 = 281;

/// Send a message on an open IPC channel.
const SYS_CHANNEL_SEND: u64 = 201;

/// Receive a message from an IPC channel (blocking).
const SYS_CHANNEL_RECV: u64 = 202;

/// Close an IPC channel handle.
const SYS_CHANNEL_CLOSE: u64 = 204;

/// Force-terminate a process and all its threads.
const SYS_PROCESS_KILL: u64 = 506;

/// Suspend all threads of a process.
const SYS_THREAD_SUSPEND: u64 = 513;

/// Resume a suspended thread.
const SYS_THREAD_RESUME: u64 = 514;

// ============================================================================
// Mode detection
// ============================================================================

/// Operating mode determined by argv[0].
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Pgrep,
    Pkill,
}

/// Detect mode from the invocation name (basename of argv[0]).
fn detect_mode(argv0: &str) -> Mode {
    let basename = argv0.rsplit('/').next().unwrap_or(argv0);
    // Also strip .exe suffix for Windows-style paths during development.
    let basename = basename.strip_suffix(".exe").unwrap_or(basename);
    if basename.ends_with("pkill") {
        Mode::Pkill
    } else {
        Mode::Pgrep
    }
}

// ============================================================================
// Signal mapping (OurOS uses IPC, not Unix signals -- names kept for
// familiarity)
// ============================================================================

/// Action to perform when a "signal" is requested.
#[derive(Clone, Copy, PartialEq, Debug)]
enum SignalAction {
    /// IPC-based graceful terminate.
    Term,
    /// Force kill via SYS_PROCESS_KILL.
    Kill,
    /// IPC-based hangup/restart.
    Hup,
    /// IPC-based interrupt.
    Int,
    /// IPC-based quit.
    Quit,
    /// Suspend threads.
    Stop,
    /// Resume threads.
    Cont,
    /// User-defined 1 -- graceful terminate.
    Usr1,
    /// User-defined 2 -- graceful terminate.
    Usr2,
    /// Alarm -- graceful terminate.
    Alrm,
    /// Pipe -- graceful terminate.
    Pipe,
}

struct SignalDef {
    number: u32,
    name: &'static str,
    action: SignalAction,
}

/// Compatibility signal table mapping POSIX names/numbers to OurOS actions.
const SIGNALS: &[SignalDef] = &[
    SignalDef {
        number: 1,
        name: "HUP",
        action: SignalAction::Hup,
    },
    SignalDef {
        number: 2,
        name: "INT",
        action: SignalAction::Int,
    },
    SignalDef {
        number: 3,
        name: "QUIT",
        action: SignalAction::Quit,
    },
    SignalDef {
        number: 9,
        name: "KILL",
        action: SignalAction::Kill,
    },
    SignalDef {
        number: 10,
        name: "USR1",
        action: SignalAction::Usr1,
    },
    SignalDef {
        number: 12,
        name: "USR2",
        action: SignalAction::Usr2,
    },
    SignalDef {
        number: 13,
        name: "PIPE",
        action: SignalAction::Pipe,
    },
    SignalDef {
        number: 14,
        name: "ALRM",
        action: SignalAction::Alrm,
    },
    SignalDef {
        number: 15,
        name: "TERM",
        action: SignalAction::Term,
    },
    SignalDef {
        number: 17,
        name: "STOP",
        action: SignalAction::Stop,
    },
    SignalDef {
        number: 18,
        name: "CONT",
        action: SignalAction::Cont,
    },
];

/// Look up a signal by number (1-31).
fn signal_by_number(num: u32) -> Option<&'static SignalDef> {
    SIGNALS.iter().find(|s| s.number == num)
}

/// Look up a signal by name (case-insensitive, optional SIG prefix).
fn signal_by_name(name: &str) -> Option<&'static SignalDef> {
    let upper = name.to_uppercase();
    let stripped = upper.strip_prefix("SIG").unwrap_or(&upper);
    SIGNALS.iter().find(|s| s.name == stripped)
}

// ============================================================================
// Low-level syscall interface
// ============================================================================

/// Three-argument syscall via the x86-64 `syscall` instruction.
#[cfg(target_arch = "x86_64")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller ensures arguments are valid for the given syscall number.
    // The `syscall` instruction is the defined kernel entry point on x86-64.
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

// ============================================================================
// IPC helpers (process manager communication)
// ============================================================================

/// The process manager's well-known IPC endpoint.
const PROCESS_MANAGER_NAME: &[u8] = b"org.ouros.ProcessManager\0";

/// Send a command to the process manager and return its response.
fn send_process_command(command: &str) -> Result<String, String> {
    let msg = format!("{command}\0");

    // SAFETY: SYS_SERVICE_CONNECT takes a pointer to a NUL-terminated name,
    // its length, and flags. The static byte string outlives the syscall.
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
    // message buffer, and its length. `msg` lives on the stack for the
    // duration of the syscall.
    let send_ret = unsafe { syscall3(SYS_CHANNEL_SEND, ch, msg.as_ptr() as u64, msg.len() as u64) };

    if send_ret < 0 {
        // SAFETY: ch is a valid handle from a successful connect.
        let _ = unsafe { syscall3(SYS_CHANNEL_CLOSE, ch, 0, 0) };
        return Err(format!("IPC send failed (error {send_ret})"));
    }

    let mut buf = [0u8; 4096];

    // SAFETY: SYS_CHANNEL_RECV takes channel handle, writable buffer pointer,
    // and buffer length. `buf` is stack-allocated and outlives the syscall.
    let recv_ret = unsafe {
        syscall3(
            SYS_CHANNEL_RECV,
            ch,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };

    // Always close the channel.
    // SAFETY: ch is a valid channel handle.
    let _ = unsafe { syscall3(SYS_CHANNEL_CLOSE, ch, 0, 0) };

    if recv_ret < 0 {
        return Err(format!("IPC recv failed (error {recv_ret})"));
    }

    let len = (recv_ret as usize).min(buf.len());
    let response = String::from_utf8_lossy(&buf[..len]).to_string();
    Ok(response)
}

/// Check whether an IPC response indicates success.
fn ipc_response_ok(resp: &str) -> bool {
    let trimmed = resp.trim().trim_end_matches('\0');
    trimmed.starts_with("OK") || trimmed.starts_with("ACK")
}

// ============================================================================
// Process killing operations
// ============================================================================

/// Force-terminate a process via SYS_PROCESS_KILL.
fn force_kill(pid: u64, exit_code: u64) -> Result<i64, i64> {
    // SAFETY: SYS_PROCESS_KILL takes the target PID and exit code. Returns
    // thread count on success, negative error on failure.
    let ret = unsafe { syscall3(SYS_PROCESS_KILL, pid, exit_code, 0) };
    if ret >= 0 { Ok(ret) } else { Err(ret) }
}

/// Send the appropriate action to a process.
fn send_signal(pid: u64, action: SignalAction) -> Result<(), String> {
    match action {
        SignalAction::Term
        | SignalAction::Usr1
        | SignalAction::Usr2
        | SignalAction::Alrm
        | SignalAction::Pipe
        | SignalAction::Quit => {
            // Try IPC first, fall back to force kill.
            let cmd_name = match action {
                SignalAction::Term => "PROCESS_TERMINATE",
                SignalAction::Quit => "PROCESS_TERMINATE",
                SignalAction::Usr1 => "PROCESS_SIGNAL USR1",
                SignalAction::Usr2 => "PROCESS_SIGNAL USR2",
                SignalAction::Alrm => "PROCESS_SIGNAL ALRM",
                SignalAction::Pipe => "PROCESS_SIGNAL PIPE",
                _ => "PROCESS_TERMINATE",
            };
            let cmd = format!("{cmd_name} {pid}");
            match send_process_command(&cmd) {
                Ok(resp) if ipc_response_ok(&resp) => Ok(()),
                _ => {
                    // Fallback to direct kill.
                    force_kill(pid, 143)
                        .map(|_| ())
                        .map_err(|e| format!("failed to terminate {pid}: error {e}"))
                }
            }
        }
        SignalAction::Kill => force_kill(pid, 137)
            .map(|_| ())
            .map_err(|e| format!("failed to kill {pid}: error {e}")),
        SignalAction::Hup => {
            let cmd = format!("PROCESS_HANGUP {pid}");
            match send_process_command(&cmd) {
                Ok(resp) if ipc_response_ok(&resp) => Ok(()),
                _ => force_kill(pid, 129)
                    .map(|_| ())
                    .map_err(|e| format!("failed to HUP {pid}: error {e}")),
            }
        }
        SignalAction::Int => {
            let cmd = format!("PROCESS_INTERRUPT {pid}");
            match send_process_command(&cmd) {
                Ok(resp) if ipc_response_ok(&resp) => Ok(()),
                _ => force_kill(pid, 130)
                    .map(|_| ())
                    .map_err(|e| format!("failed to INT {pid}: error {e}")),
            }
        }
        SignalAction::Stop => stop_process(pid),
        SignalAction::Cont => continue_process(pid),
    }
}

/// Suspend all threads of a process by reading /proc/<pid>/task/.
fn stop_process(pid: u64) -> Result<(), String> {
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

        // SAFETY: SYS_THREAD_SUSPEND takes a task ID.
        let ret = unsafe { syscall3(SYS_THREAD_SUSPEND, tid, 0, 0) };
        if ret < 0 {
            last_err = Some(format!("suspend tid {tid}: error {ret}"));
        } else {
            count = count.saturating_add(1);
        }
    }

    if count == 0 {
        Err(last_err.unwrap_or_else(|| "no threads found".to_string()))
    } else {
        Ok(())
    }
}

/// Resume all threads of a process.
fn continue_process(pid: u64) -> Result<(), String> {
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

        // SAFETY: SYS_THREAD_RESUME takes a task ID.
        let ret = unsafe { syscall3(SYS_THREAD_RESUME, tid, 0, 0) };
        if ret < 0 {
            last_err = Some(format!("resume tid {tid}: error {ret}"));
        } else {
            count = count.saturating_add(1);
        }
    }

    if count == 0 {
        Err(last_err.unwrap_or_else(|| "no threads found".to_string()))
    } else {
        Ok(())
    }
}

// ============================================================================
// Process information (read from /proc)
// ============================================================================

/// Per-process information scraped from /proc/<pid>/.
struct ProcessInfo {
    pid: u32,
    /// Process name from /proc/<pid>/stat (field inside parentheses).
    name: String,
    /// Full command line from /proc/<pid>/cmdline.
    cmdline: String,
    /// Parent PID.
    ppid: u32,
    /// Process group.
    pgrp: u32,
    /// Controlling terminal number.
    tty: i32,
    /// Process start time in ticks since boot.
    starttime: u64,
    /// Real UID.
    ruid: u32,
    /// Effective UID.
    euid: u32,
}

/// Read a file to a trimmed string, returning None on failure.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Read information about a single process from /proc/<pid>/.
fn read_process_info(pid: u32) -> Option<ProcessInfo> {
    let stat_path = format!("/proc/{pid}/stat");
    let stat_content = read_file(&stat_path)?;

    // Format: <pid> (<name>) <state> <ppid> <pgrp> <session> <tty> ...
    // The name may contain spaces and parentheses.
    let comm_start = stat_content.find('(')?;
    let comm_end = stat_content.rfind(')')?;
    let name = stat_content.get(comm_start + 1..comm_end)?.to_string();
    let rest = stat_content.get(comm_end + 2..)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();

    if fields.len() < 20 {
        return None;
    }

    let ppid: u32 = fields.get(1)?.parse().unwrap_or(0);
    let pgrp: u32 = fields.get(2)?.parse().unwrap_or(0);
    let tty: i32 = fields.get(4)?.parse().unwrap_or(0);
    let starttime: u64 = fields.get(19)?.parse().unwrap_or(0);

    // Read UIDs from /proc/<pid>/status.
    let mut ruid = 0u32;
    let mut euid = 0u32;
    if let Some(status_content) = read_file(&format!("/proc/{pid}/status")) {
        for line in status_content.lines() {
            if let Some(val) = line.strip_prefix("Uid:") {
                let parts: Vec<&str> = val.trim().split_whitespace().collect();
                // Format: real effective saved fs
                if let Some(r) = parts.first() {
                    ruid = r.parse().unwrap_or(0);
                }
                if let Some(e) = parts.get(1) {
                    euid = e.parse().unwrap_or(0);
                }
                break;
            }
        }
    }

    // Read /proc/<pid>/cmdline (NUL-separated arguments).
    let cmdline = read_cmdline(pid);

    Some(ProcessInfo {
        pid,
        name,
        cmdline,
        ppid,
        pgrp,
        tty,
        starttime,
        ruid,
        euid,
    })
}

/// Read /proc/<pid>/cmdline and return it with NUL bytes replaced by spaces.
fn read_cmdline(pid: u32) -> String {
    let path = format!("/proc/{pid}/cmdline");
    match fs::read(&path) {
        Ok(bytes) => {
            // Replace NUL separators with spaces; trim trailing.
            let s: String = bytes
                .iter()
                .map(|&b| if b == 0 { ' ' } else { b as char })
                .collect();
            s.trim().to_string()
        }
        Err(_) => String::new(),
    }
}

/// Enumerate all process PIDs from /proc.
fn enumerate_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Ok(pid) = name.parse::<u32>() {
                    pids.push(pid);
                }
            }
        }
    }
    pids
}

/// Read all process information from /proc.
fn read_all_processes() -> Vec<ProcessInfo> {
    let pids = enumerate_pids();
    let mut procs = Vec::with_capacity(pids.len());
    for pid in pids {
        if let Some(info) = read_process_info(pid) {
            procs.push(info);
        }
    }
    procs
}

// ============================================================================
// Regex-style pattern matching
// ============================================================================

/// A basic regex engine supporting: `.` `*` `+` `?` `[]` `^` `$` `|` `\`
///
/// This avoids pulling in an external regex crate. The engine compiles a
/// pattern into a list of `Token`s and uses backtracking to match.
#[derive(Clone, Debug)]
enum Token {
    /// Match a single literal byte.
    Literal(u8),
    /// `.` -- match any single character.
    AnyChar,
    /// `[...]` character class. Contains (byte, byte) inclusive ranges.
    /// `negated` means `[^...]`.
    CharClass {
        ranges: Vec<(u8, u8)>,
        negated: bool,
    },
    /// `^` -- anchor to start of string.
    StartAnchor,
    /// `$` -- anchor to end of string.
    EndAnchor,
}

/// A compiled token with its repetition quantifier.
#[derive(Clone, Debug)]
struct PatternElement {
    token: Token,
    /// Minimum repetitions.
    min_rep: u32,
    /// Maximum repetitions (u32::MAX = unbounded).
    max_rep: u32,
}

/// A compiled pattern (one branch of a `|`-separated alternation).
type PatternBranch = Vec<PatternElement>;

/// A full compiled pattern (list of alternatives separated by `|`).
struct CompiledPattern {
    branches: Vec<PatternBranch>,
}

/// Compile a regex pattern string into a `CompiledPattern`.
fn compile_pattern(pattern: &str) -> Result<CompiledPattern, String> {
    let mut branches = Vec::new();
    // Split on unescaped, un-bracketed `|`.
    let raw_branches = split_alternatives(pattern);
    for branch_str in &raw_branches {
        let branch = compile_branch(branch_str)?;
        branches.push(branch);
    }
    if branches.is_empty() {
        branches.push(Vec::new());
    }
    Ok(CompiledPattern { branches })
}

/// Split a pattern on top-level `|` characters, respecting `\|` escapes
/// and `[...]` brackets.
fn split_alternatives(pattern: &str) -> Vec<String> {
    let bytes = pattern.as_bytes();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    let mut in_bracket = false;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() {
            // Escaped character -- consume both bytes.
            current.push(b as char);
            current.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }
        if b == b'[' && !in_bracket {
            in_bracket = true;
            current.push(b as char);
            i += 1;
            continue;
        }
        if b == b']' && in_bracket {
            in_bracket = false;
            current.push(b as char);
            i += 1;
            continue;
        }
        if b == b'|' && !in_bracket {
            parts.push(current.clone());
            current.clear();
            i += 1;
            continue;
        }
        current.push(b as char);
        i += 1;
    }
    parts.push(current);
    parts
}

/// Compile a single branch (no top-level `|`).
fn compile_branch(branch: &str) -> Result<PatternBranch, String> {
    let bytes = branch.as_bytes();
    let mut elements: PatternBranch = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        match b {
            b'^' => {
                elements.push(PatternElement {
                    token: Token::StartAnchor,
                    min_rep: 1,
                    max_rep: 1,
                });
                i += 1;
            }
            b'$' => {
                elements.push(PatternElement {
                    token: Token::EndAnchor,
                    min_rep: 1,
                    max_rep: 1,
                });
                i += 1;
            }
            b'.' => {
                i += 1;
                let (min_r, max_r, new_i) = parse_quantifier(bytes, i);
                elements.push(PatternElement {
                    token: Token::AnyChar,
                    min_rep: min_r,
                    max_rep: max_r,
                });
                i = new_i;
            }
            b'[' => {
                let (ranges, negated, end_idx) = parse_char_class(bytes, i)?;
                i = end_idx;
                let (min_r, max_r, new_i) = parse_quantifier(bytes, i);
                elements.push(PatternElement {
                    token: Token::CharClass { ranges, negated },
                    min_rep: min_r,
                    max_rep: max_r,
                });
                i = new_i;
            }
            b'\\' => {
                if i + 1 >= bytes.len() {
                    return Err("trailing backslash".to_string());
                }
                let escaped = bytes[i + 1];
                i += 2;
                let (min_r, max_r, new_i) = parse_quantifier(bytes, i);
                elements.push(PatternElement {
                    token: Token::Literal(escaped),
                    min_rep: min_r,
                    max_rep: max_r,
                });
                i = new_i;
            }
            // Quantifiers without a preceding element are literal.
            b'*' | b'+' | b'?' => {
                // Treat as literal if there is no preceding element to quantify.
                i += 1;
                elements.push(PatternElement {
                    token: Token::Literal(b),
                    min_rep: 1,
                    max_rep: 1,
                });
            }
            _ => {
                i += 1;
                let (min_r, max_r, new_i) = parse_quantifier(bytes, i);
                elements.push(PatternElement {
                    token: Token::Literal(b),
                    min_rep: min_r,
                    max_rep: max_r,
                });
                i = new_i;
            }
        }
    }

    Ok(elements)
}

/// Parse a `[...]` or `[^...]` character class starting at position `start`
/// (which points at the `[`). Returns the ranges, negated flag, and the
/// index just past the closing `]`.
fn parse_char_class(bytes: &[u8], start: usize) -> Result<(Vec<(u8, u8)>, bool, usize), String> {
    let mut i = start + 1; // skip `[`
    let mut negated = false;
    let mut ranges: Vec<(u8, u8)> = Vec::new();

    if i < bytes.len() && bytes[i] == b'^' {
        negated = true;
        i += 1;
    }

    // `]` as first character (after optional `^`) is literal.
    let first_in_class = i;

    while i < bytes.len() {
        let b = bytes[i];

        if b == b']' && i != first_in_class {
            // End of character class.
            return Ok((ranges, negated, i + 1));
        }

        if b == b'\\' && i + 1 < bytes.len() {
            // Escaped character inside class.
            let escaped = bytes[i + 1];
            i += 2;
            // Check for range: \x-y
            if i + 1 < bytes.len()
                && bytes[i] == b'-'
                && i + 1 < bytes.len()
                && bytes[i + 1] != b']'
            {
                let end = if bytes[i + 1] == b'\\' && i + 2 < bytes.len() {
                    i += 3;
                    bytes[i - 1]
                } else {
                    i += 2;
                    bytes[i - 1]
                };
                ranges.push((escaped, end));
            } else {
                ranges.push((escaped, escaped));
            }
            continue;
        }

        // Check for range: a-z
        if i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] != b']' {
            let range_start = b;
            let range_end = if bytes[i + 2] == b'\\' && i + 3 < bytes.len() {
                i += 4;
                bytes[i - 1]
            } else {
                i += 3;
                bytes[i - 1]
            };
            ranges.push((range_start, range_end));
            continue;
        }

        ranges.push((b, b));
        i += 1;
    }

    Err("unterminated character class".to_string())
}

/// Parse a quantifier (`*`, `+`, `?`) at the current position.
/// Returns (min, max, new_index).
fn parse_quantifier(bytes: &[u8], pos: usize) -> (u32, u32, usize) {
    if pos >= bytes.len() {
        return (1, 1, pos);
    }
    match bytes[pos] {
        b'*' => (0, u32::MAX, pos + 1),
        b'+' => (1, u32::MAX, pos + 1),
        b'?' => (0, 1, pos + 1),
        _ => (1, 1, pos),
    }
}

/// Test whether a single token matches a byte. For case-insensitive matching,
/// the caller should have already lowered the text.
fn token_matches(token: &Token, byte: u8) -> bool {
    match token {
        Token::Literal(lit) => byte == *lit,
        Token::AnyChar => true,
        Token::CharClass { ranges, negated } => {
            let in_class = ranges.iter().any(|&(lo, hi)| {
                if lo <= hi {
                    byte >= lo && byte <= hi
                } else {
                    byte >= hi && byte <= lo
                }
            });
            if *negated { !in_class } else { in_class }
        }
        Token::StartAnchor | Token::EndAnchor => false,
    }
}

/// Try to match a branch against `text[text_pos..]`. Returns the end position
/// on success, or None.
fn match_branch(
    branch: &PatternBranch,
    text: &[u8],
    text_pos: usize,
    elem_idx: usize,
) -> Option<usize> {
    if elem_idx >= branch.len() {
        return Some(text_pos);
    }

    let elem = &branch[elem_idx];

    match &elem.token {
        Token::StartAnchor => {
            if text_pos == 0 {
                match_branch(branch, text, text_pos, elem_idx + 1)
            } else {
                None
            }
        }
        Token::EndAnchor => {
            if text_pos == text.len() {
                match_branch(branch, text, text_pos, elem_idx + 1)
            } else {
                None
            }
        }
        _ => {
            // Greedy matching: try max repetitions first, then back off.
            let max_possible = if elem.max_rep == u32::MAX {
                // Count how many consecutive chars match from text_pos.
                let mut count = 0u32;
                let mut tp = text_pos;
                while tp < text.len() && token_matches(&elem.token, text[tp]) {
                    count = count.saturating_add(1);
                    tp += 1;
                    if count >= 10_000 {
                        break; // Safety limit.
                    }
                }
                count
            } else {
                let mut count = 0u32;
                let mut tp = text_pos;
                while tp < text.len()
                    && count < elem.max_rep
                    && token_matches(&elem.token, text[tp])
                {
                    count = count.saturating_add(1);
                    tp += 1;
                }
                count
            };

            // Try from max_possible down to min_rep (greedy backtracking).
            let mut reps = max_possible;
            loop {
                if reps < elem.min_rep {
                    return None;
                }
                let next_pos = text_pos + reps as usize;
                if let Some(end) = match_branch(branch, text, next_pos, elem_idx + 1) {
                    return Some(end);
                }
                if reps == 0 {
                    return None;
                }
                reps -= 1;
            }
        }
    }
}

/// Test whether `text` matches the compiled pattern (substring search --
/// the pattern does not have to match the entire text unless anchored).
fn pattern_matches(pattern: &CompiledPattern, text: &str) -> bool {
    let text_bytes = text.as_bytes();
    for branch in &pattern.branches {
        // Check if the branch is fully anchored at start.
        let anchored_start = branch
            .first()
            .is_some_and(|e| matches!(e.token, Token::StartAnchor));

        if anchored_start {
            // Only try matching from position 0.
            if match_branch(branch, text_bytes, 0, 0).is_some() {
                return true;
            }
        } else {
            // Try matching at every position (substring search).
            for start in 0..=text_bytes.len() {
                if match_branch(branch, text_bytes, start, 0).is_some() {
                    return true;
                }
            }
        }
    }
    false
}

/// Apply case-insensitive lowering to a pattern: lower all Literal bytes and
/// all CharClass range endpoints.
fn lower_pattern(pattern: &mut CompiledPattern) {
    for branch in &mut pattern.branches {
        for elem in branch.iter_mut() {
            match &mut elem.token {
                Token::Literal(b) => {
                    *b = to_ascii_lower(*b);
                }
                Token::CharClass { ranges, .. } => {
                    for range in ranges.iter_mut() {
                        range.0 = to_ascii_lower(range.0);
                        range.1 = to_ascii_lower(range.1);
                    }
                }
                _ => {}
            }
        }
    }
}

fn to_ascii_lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' { b + 32 } else { b }
}

// ============================================================================
// User/UID resolution
// ============================================================================

/// Resolve a user specification (numeric UID or username) to a UID.
fn resolve_uid(spec: &str) -> Option<u32> {
    // Try numeric first.
    if let Ok(uid) = spec.parse::<u32>() {
        return Some(uid);
    }

    // Try /etc/passwd lookup.
    if let Some(content) = read_file("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3 && fields[0] == spec {
                if let Ok(uid) = fields[2].parse::<u32>() {
                    return Some(uid);
                }
            }
        }
    }

    None
}

// ============================================================================
// TTY name resolution
// ============================================================================

/// Parse a terminal specification like "tty1", "pts/0", or just "1" into the
/// TTY device number used in /proc/<pid>/stat.
fn resolve_tty(spec: &str) -> Option<i32> {
    // Try direct numeric.
    if let Ok(n) = spec.parse::<i32>() {
        return Some(n);
    }
    // Strip "tty" prefix.
    if let Some(rest) = spec.strip_prefix("tty") {
        if let Ok(n) = rest.parse::<i32>() {
            return Some(n);
        }
    }
    // Strip "pts/" prefix -- map to negative numbers as a convention.
    if let Some(rest) = spec.strip_prefix("pts/") {
        if let Ok(n) = rest.parse::<i32>() {
            // Use negative as convention for pts devices.
            return Some(-(n + 1));
        }
    }
    // Return tty number 0 for "?" meaning no terminal.
    if spec == "?" || spec == "none" {
        return Some(0);
    }
    None
}

// ============================================================================
// Options parsing
// ============================================================================

struct Options {
    mode: Mode,
    /// Compiled match pattern.
    pattern: Option<CompiledPattern>,
    /// Raw pattern string (for error messages).
    pattern_str: String,
    /// Match against full command line instead of just process name.
    full_match: bool,
    /// Require exact match.
    exact_match: bool,
    /// Case-insensitive matching.
    ignore_case: bool,
    /// Select only the newest matching process.
    newest: bool,
    /// Select only the oldest matching process.
    oldest: bool,
    /// Negate matching.
    inverse: bool,
    /// Filter: parent PID.
    parent_pid: Option<u32>,
    /// Filter: effective UID.
    filter_euid: Option<u32>,
    /// Filter: real UID.
    filter_ruid: Option<u32>,
    /// Filter: process group.
    filter_pgrp: Option<u32>,
    /// Filter: controlling terminal.
    filter_tty: Option<i32>,
    // pgrep-specific:
    /// Print count instead of PIDs.
    count_only: bool,
    /// Output delimiter.
    delimiter: String,
    /// Also print process name.
    list_name: bool,
    /// Also print full command line.
    list_full: bool,
    // pkill-specific:
    /// Signal action to send.
    signal: SignalAction,
    /// Echo what was killed.
    echo: bool,
}

impl Options {
    fn new(mode: Mode) -> Self {
        Self {
            mode,
            pattern: None,
            pattern_str: String::new(),
            full_match: false,
            exact_match: false,
            ignore_case: false,
            newest: false,
            oldest: false,
            inverse: false,
            parent_pid: None,
            filter_euid: None,
            filter_ruid: None,
            filter_pgrp: None,
            filter_tty: None,
            count_only: false,
            delimiter: "\n".to_string(),
            list_name: false,
            list_full: false,
            signal: SignalAction::Term,
            echo: false,
        }
    }
}

/// Parse command-line arguments. Returns Err for usage errors.
fn parse_args(args: &[String]) -> Result<Options, String> {
    let mode = args.first().map(|a| detect_mode(a)).unwrap_or(Mode::Pgrep);

    let mut opts = Options::new(mode);
    let mut i = 1;
    let mut saw_pattern = false;

    while i < args.len() {
        let arg = args[i].as_str();

        // Handle --long=value forms.
        if let Some(rest) = arg.strip_prefix("--signal=") {
            if let Some(sig) = signal_by_name(rest) {
                opts.signal = sig.action;
            } else if let Ok(num) = rest.parse::<u32>() {
                if let Some(sig) = signal_by_number(num) {
                    opts.signal = sig.action;
                } else {
                    return Err(format!("unknown signal: {rest}"));
                }
            } else {
                return Err(format!("unknown signal: {rest}"));
            }
            i += 1;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--parent=") {
            opts.parent_pid = Some(
                rest.parse::<u32>()
                    .map_err(|_| format!("invalid parent PID: {rest}"))?,
            );
            i += 1;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--euid=") {
            opts.filter_euid =
                Some(resolve_uid(rest).ok_or_else(|| format!("unknown user: {rest}"))?);
            i += 1;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--uid=") {
            opts.filter_ruid =
                Some(resolve_uid(rest).ok_or_else(|| format!("unknown user: {rest}"))?);
            i += 1;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--pgroup=") {
            opts.filter_pgrp = Some(
                rest.parse::<u32>()
                    .map_err(|_| format!("invalid process group: {rest}"))?,
            );
            i += 1;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--terminal=") {
            opts.filter_tty =
                Some(resolve_tty(rest).ok_or_else(|| format!("invalid terminal: {rest}"))?);
            i += 1;
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--delimiter=") {
            opts.delimiter = rest.to_string();
            i += 1;
            continue;
        }

        match arg {
            "--help" | "-h" => {
                return Err(String::new());
            }
            "--full" | "-f" => {
                opts.full_match = true;
            }
            "--exact" | "-x" => {
                opts.exact_match = true;
            }
            "--ignore-case" | "-i" => {
                opts.ignore_case = true;
            }
            "--newest" | "-n" => {
                opts.newest = true;
            }
            "--oldest" | "-o" => {
                opts.oldest = true;
            }
            "--inverse" | "-v" => {
                opts.inverse = true;
            }
            "--count" | "-c" => {
                opts.count_only = true;
            }
            "--list-name" | "-l" => {
                opts.list_name = true;
            }
            "--list-full" | "-a" => {
                opts.list_full = true;
            }
            "--echo" | "-e" => {
                opts.echo = true;
            }
            "-P" | "--parent" => {
                i += 1;
                if i >= args.len() {
                    return Err("-P requires a parent PID".to_string());
                }
                opts.parent_pid = Some(
                    args[i]
                        .parse::<u32>()
                        .map_err(|_| format!("invalid parent PID: {}", args[i]))?,
                );
            }
            "-u" | "--euid" => {
                i += 1;
                if i >= args.len() {
                    return Err("-u requires a user/UID".to_string());
                }
                opts.filter_euid = Some(
                    resolve_uid(&args[i]).ok_or_else(|| format!("unknown user: {}", args[i]))?,
                );
            }
            "-U" | "--uid" => {
                i += 1;
                if i >= args.len() {
                    return Err("-U requires a user/UID".to_string());
                }
                opts.filter_ruid = Some(
                    resolve_uid(&args[i]).ok_or_else(|| format!("unknown user: {}", args[i]))?,
                );
            }
            "-g" | "--pgroup" => {
                i += 1;
                if i >= args.len() {
                    return Err("-g requires a process group ID".to_string());
                }
                opts.filter_pgrp = Some(
                    args[i]
                        .parse::<u32>()
                        .map_err(|_| format!("invalid process group: {}", args[i]))?,
                );
            }
            "-t" | "--terminal" => {
                i += 1;
                if i >= args.len() {
                    return Err("-t requires a terminal".to_string());
                }
                opts.filter_tty = Some(
                    resolve_tty(&args[i])
                        .ok_or_else(|| format!("invalid terminal: {}", args[i]))?,
                );
            }
            "-d" | "--delimiter" => {
                i += 1;
                if i >= args.len() {
                    return Err("-d requires a delimiter".to_string());
                }
                opts.delimiter = args[i].clone();
            }
            _ => {
                // Check for -<SIGNAL> (pkill mode).
                if mode == Mode::Pkill {
                    if let Some(stripped) = arg.strip_prefix('-') {
                        if !stripped.is_empty() && !saw_pattern {
                            // Try numeric signal.
                            if let Ok(num) = stripped.parse::<u32>() {
                                if let Some(sig) = signal_by_number(num) {
                                    opts.signal = sig.action;
                                    i += 1;
                                    continue;
                                }
                            }
                            // Try named signal.
                            if let Some(sig) = signal_by_name(stripped) {
                                opts.signal = sig.action;
                                i += 1;
                                continue;
                            }
                        }
                    }
                }

                // Unknown flag?
                if arg.starts_with('-') && !saw_pattern {
                    return Err(format!("unknown option: {arg}"));
                }

                // Must be the pattern.
                if saw_pattern {
                    return Err(format!("unexpected argument: {arg}"));
                }
                opts.pattern_str = arg.to_string();
                saw_pattern = true;
            }
        }
        i += 1;
    }

    if !saw_pattern {
        return Err("no pattern specified".to_string());
    }

    // Compile the pattern.
    let pattern_to_compile = if opts.ignore_case {
        opts.pattern_str.to_lowercase()
    } else {
        opts.pattern_str.clone()
    };

    let mut compiled = compile_pattern(&pattern_to_compile)?;
    if opts.ignore_case {
        lower_pattern(&mut compiled);
    }

    // If exact match, wrap with ^...$ anchors if not already present.
    if opts.exact_match {
        for branch in &mut compiled.branches {
            let has_start = branch
                .first()
                .is_some_and(|e| matches!(e.token, Token::StartAnchor));
            let has_end = branch
                .last()
                .is_some_and(|e| matches!(e.token, Token::EndAnchor));
            if !has_start {
                branch.insert(
                    0,
                    PatternElement {
                        token: Token::StartAnchor,
                        min_rep: 1,
                        max_rep: 1,
                    },
                );
            }
            if !has_end {
                branch.push(PatternElement {
                    token: Token::EndAnchor,
                    min_rep: 1,
                    max_rep: 1,
                });
            }
        }
    }

    opts.pattern = Some(compiled);
    Ok(opts)
}

// ============================================================================
// Matching logic
// ============================================================================

/// Test whether a process matches the options' filters and pattern.
fn process_matches(proc_info: &ProcessInfo, opts: &Options, my_pid: u32) -> bool {
    // Never match ourselves.
    if proc_info.pid == my_pid {
        return false;
    }

    // Apply non-pattern filters first.
    if let Some(ppid) = opts.parent_pid {
        if proc_info.ppid != ppid {
            return false;
        }
    }
    if let Some(euid) = opts.filter_euid {
        if proc_info.euid != euid {
            return false;
        }
    }
    if let Some(ruid) = opts.filter_ruid {
        if proc_info.ruid != ruid {
            return false;
        }
    }
    if let Some(pgrp) = opts.filter_pgrp {
        if proc_info.pgrp != pgrp {
            return false;
        }
    }
    if let Some(tty) = opts.filter_tty {
        if proc_info.tty != tty {
            return false;
        }
    }

    // Pattern matching.
    let pattern = match &opts.pattern {
        Some(p) => p,
        None => return true,
    };

    let text = if opts.full_match {
        &proc_info.cmdline
    } else {
        &proc_info.name
    };

    let text_to_match = if opts.ignore_case {
        text.to_lowercase()
    } else {
        text.clone()
    };

    let matched = pattern_matches(pattern, &text_to_match);

    if opts.inverse { !matched } else { matched }
}

// ============================================================================
// Help / usage
// ============================================================================

fn print_usage(mode: Mode) {
    match mode {
        Mode::Pgrep => {
            println!("OurOS pgrep v0.1.0 -- Find processes by pattern");
            println!();
            println!("USAGE:");
            println!("  pgrep [options] <pattern>");
            println!();
            println!("OPTIONS:");
            println!("  -f, --full          Match against full command line");
            println!("  -x, --exact         Require exact match of process name");
            println!("  -i, --ignore-case   Case-insensitive matching");
            println!("  -n, --newest        Select only the newest match");
            println!("  -o, --oldest        Select only the oldest match");
            println!("  -v, --inverse       Negate matching");
            println!("  -c, --count         Print count of matches");
            println!("  -d, --delimiter=D   Output delimiter (default: newline)");
            println!("  -l, --list-name     Also print process name");
            println!("  -a, --list-full     Also print full command line");
            println!("  -P, --parent=PPID   Match children of given parent PID");
            println!("  -u, --euid=USER     Match by effective user (name or UID)");
            println!("  -U, --uid=USER      Match by real user");
            println!("  -g, --pgroup=PGRP   Match by process group");
            println!("  -t, --terminal=TTY  Match by controlling terminal");
            println!("  -h, --help          Show this help");
            println!();
            println!("PATTERN:");
            println!("  Basic regex: . * + ? [] [^] ^ $ | \\");
            println!();
            println!("EXIT STATUS:");
            println!("  0  At least one process matched");
            println!("  1  No processes matched");
            println!("  2  Syntax error or fatal error");
            println!();
            println!("EXAMPLES:");
            println!("  pgrep ssh               Find processes matching 'ssh'");
            println!("  pgrep -l -u root ssh    List ssh processes owned by root");
            println!("  pgrep -c bash           Count bash processes");
            println!("  pgrep -f 'python.*srv'  Match full cmdline with regex");
            println!("  pgrep -x init           Only exact match 'init'");
        }
        Mode::Pkill => {
            println!("OurOS pkill v0.1.0 -- Signal processes by pattern");
            println!();
            println!("USAGE:");
            println!("  pkill [options] <pattern>");
            println!();
            println!("OPTIONS:");
            println!("  --signal=SIG / -SIG Signal to send (default: TERM)");
            println!("  -e, --echo          Echo what was killed");
            println!("  -f, --full          Match against full command line");
            println!("  -x, --exact         Require exact match of process name");
            println!("  -i, --ignore-case   Case-insensitive matching");
            println!("  -n, --newest        Select only the newest match");
            println!("  -o, --oldest        Select only the oldest match");
            println!("  -v, --inverse       Negate matching");
            println!("  -P, --parent=PPID   Match children of given parent PID");
            println!("  -u, --euid=USER     Match by effective user (name or UID)");
            println!("  -U, --uid=USER      Match by real user");
            println!("  -g, --pgroup=PGRP   Match by process group");
            println!("  -t, --terminal=TTY  Match by controlling terminal");
            println!("  -h, --help          Show this help");
            println!();
            println!("SIGNALS:");
            println!("  HUP(1) INT(2) QUIT(3) KILL(9) USR1(10) USR2(12)");
            println!("  PIPE(13) ALRM(14) TERM(15) STOP(17) CONT(18)");
            println!();
            println!("EXIT STATUS:");
            println!("  0  At least one process matched and was signalled");
            println!("  1  No processes matched");
            println!("  2  Syntax error or fatal error");
            println!();
            println!("EXAMPLES:");
            println!("  pkill ssh                Terminate all ssh processes");
            println!("  pkill -9 runaway         Force kill 'runaway'");
            println!("  pkill -STOP -u bob .     Suspend all of bob's processes");
            println!("  pkill -e -n myserver     Kill newest myserver, echo it");
        }
    }
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let mode = args.first().map(|a| detect_mode(a)).unwrap_or(Mode::Pgrep);

    let opts = match parse_args(&args) {
        Ok(o) => o,
        Err(msg) => {
            if msg.is_empty() {
                print_usage(mode);
                process::exit(0);
            }
            let prog = if mode == Mode::Pkill {
                "pkill"
            } else {
                "pgrep"
            };
            eprintln!("{prog}: {msg}");
            eprintln!("Try '{prog} --help' for usage.");
            process::exit(2);
        }
    };

    // Get our own PID to exclude from results.
    let my_pid = std::process::id();

    // Read all processes and filter.
    let all_procs = read_all_processes();
    let mut matches: Vec<&ProcessInfo> = all_procs
        .iter()
        .filter(|p| process_matches(p, &opts, my_pid))
        .collect();

    // Sort by starttime for newest/oldest selection.
    matches.sort_by_key(|p| p.starttime);

    // Apply newest/oldest selection.
    if opts.newest && !matches.is_empty() {
        let newest = matches.last().copied();
        matches.clear();
        if let Some(p) = newest {
            matches.push(p);
        }
    } else if opts.oldest && !matches.is_empty() {
        let oldest = matches.first().copied();
        matches.clear();
        if let Some(p) = oldest {
            matches.push(p);
        }
    }

    if matches.is_empty() {
        let prog = if mode == Mode::Pkill {
            "pkill"
        } else {
            "pgrep"
        };
        // pgrep/pkill exit 1 when no processes matched (not an error).
        if mode == Mode::Pgrep && opts.count_only {
            println!("0");
        }
        // pkill prints nothing on no match (just exit 1).
        let _ = prog; // suppress unused warning
        process::exit(1);
    }

    match opts.mode {
        Mode::Pgrep => {
            if opts.count_only {
                println!("{}", matches.len());
            } else {
                let mut output_parts: Vec<String> = Vec::new();
                for p in &matches {
                    let entry = if opts.list_full {
                        format!("{} {}", p.pid, p.cmdline)
                    } else if opts.list_name {
                        format!("{} {}", p.pid, p.name)
                    } else {
                        format!("{}", p.pid)
                    };
                    output_parts.push(entry);
                }
                println!("{}", output_parts.join(&opts.delimiter));
            }
            process::exit(0);
        }
        Mode::Pkill => {
            let mut any_failed = false;
            for p in &matches {
                let pid = p.pid as u64;
                match send_signal(pid, opts.signal) {
                    Ok(()) => {
                        if opts.echo {
                            println!("{} killed (pid {})", p.name, p.pid);
                        }
                    }
                    Err(e) => {
                        eprintln!("pkill: {e}");
                        any_failed = true;
                    }
                }
            }
            if any_failed {
                process::exit(2);
            }
            process::exit(0);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Pattern compilation and matching ----

    #[test]
    fn test_literal_match() {
        let p = compile_pattern("hello").unwrap();
        assert!(pattern_matches(&p, "hello"));
        assert!(pattern_matches(&p, "say hello world"));
        assert!(!pattern_matches(&p, "HELLO"));
        assert!(!pattern_matches(&p, "hell"));
    }

    #[test]
    fn test_dot_matches_any_char() {
        let p = compile_pattern("h.llo").unwrap();
        assert!(pattern_matches(&p, "hello"));
        assert!(pattern_matches(&p, "hallo"));
        assert!(pattern_matches(&p, "hxllo"));
        assert!(!pattern_matches(&p, "hllo"));
    }

    #[test]
    fn test_star_zero_or_more() {
        let p = compile_pattern("he*llo").unwrap();
        assert!(pattern_matches(&p, "hllo"));
        assert!(pattern_matches(&p, "hello"));
        assert!(pattern_matches(&p, "heeello"));
    }

    #[test]
    fn test_plus_one_or_more() {
        let p = compile_pattern("he+llo").unwrap();
        assert!(!pattern_matches(&p, "hllo"));
        assert!(pattern_matches(&p, "hello"));
        assert!(pattern_matches(&p, "heeello"));
    }

    #[test]
    fn test_question_zero_or_one() {
        let p = compile_pattern("he?llo").unwrap();
        assert!(pattern_matches(&p, "hllo"));
        assert!(pattern_matches(&p, "hello"));
        assert!(!pattern_matches(&p, "heeello"));
    }

    #[test]
    fn test_dot_star_matches_anything() {
        let p = compile_pattern("a.*b").unwrap();
        assert!(pattern_matches(&p, "ab"));
        assert!(pattern_matches(&p, "axyzb"));
        assert!(pattern_matches(&p, "a_long_string_b"));
        assert!(!pattern_matches(&p, "ba"));
    }

    #[test]
    fn test_start_anchor() {
        let p = compile_pattern("^hello").unwrap();
        assert!(pattern_matches(&p, "hello world"));
        assert!(!pattern_matches(&p, "say hello"));
    }

    #[test]
    fn test_end_anchor() {
        let p = compile_pattern("world$").unwrap();
        assert!(pattern_matches(&p, "hello world"));
        assert!(!pattern_matches(&p, "world hello"));
    }

    #[test]
    fn test_both_anchors() {
        let p = compile_pattern("^exact$").unwrap();
        assert!(pattern_matches(&p, "exact"));
        assert!(!pattern_matches(&p, "exactx"));
        assert!(!pattern_matches(&p, "xexact"));
    }

    #[test]
    fn test_character_class_basic() {
        let p = compile_pattern("[abc]").unwrap();
        assert!(pattern_matches(&p, "a"));
        assert!(pattern_matches(&p, "b"));
        assert!(pattern_matches(&p, "c"));
        assert!(!pattern_matches(&p, "d"));
    }

    #[test]
    fn test_character_class_range() {
        let p = compile_pattern("[a-z]").unwrap();
        assert!(pattern_matches(&p, "m"));
        assert!(pattern_matches(&p, "a"));
        assert!(pattern_matches(&p, "z"));
        assert!(!pattern_matches(&p, "A"));
        assert!(!pattern_matches(&p, "5"));
    }

    #[test]
    fn test_character_class_negated() {
        let p = compile_pattern("[^0-9]").unwrap();
        assert!(pattern_matches(&p, "a"));
        assert!(!pattern_matches(&p, "5"));
        assert!(pattern_matches(&p, "x5")); // has a non-digit
    }

    #[test]
    fn test_character_class_with_quantifier() {
        let p = compile_pattern("[0-9]+").unwrap();
        assert!(pattern_matches(&p, "123"));
        assert!(pattern_matches(&p, "abc123def"));
        assert!(!pattern_matches(&p, "abc"));
    }

    #[test]
    fn test_alternation_basic() {
        let p = compile_pattern("cat|dog").unwrap();
        assert!(pattern_matches(&p, "cat"));
        assert!(pattern_matches(&p, "dog"));
        assert!(!pattern_matches(&p, "fish"));
    }

    #[test]
    fn test_alternation_multiple() {
        let p = compile_pattern("a|b|c|d").unwrap();
        assert!(pattern_matches(&p, "a"));
        assert!(pattern_matches(&p, "d"));
        assert!(!pattern_matches(&p, "e"));
    }

    #[test]
    fn test_escape_special() {
        let p = compile_pattern(r"hello\.world").unwrap();
        assert!(pattern_matches(&p, "hello.world"));
        assert!(!pattern_matches(&p, "helloxworld"));
    }

    #[test]
    fn test_escape_star() {
        let p = compile_pattern(r"a\*b").unwrap();
        assert!(pattern_matches(&p, "a*b"));
        assert!(!pattern_matches(&p, "ab"));
        assert!(!pattern_matches(&p, "aab"));
    }

    #[test]
    fn test_complex_pattern() {
        let p = compile_pattern("^[a-z]+_[0-9]+$").unwrap();
        assert!(pattern_matches(&p, "proc_123"));
        assert!(pattern_matches(&p, "a_0"));
        assert!(!pattern_matches(&p, "Proc_123"));
        assert!(!pattern_matches(&p, "proc_"));
        assert!(!pattern_matches(&p, "_123"));
    }

    #[test]
    fn test_dot_plus() {
        let p = compile_pattern(".+").unwrap();
        assert!(pattern_matches(&p, "anything"));
        assert!(pattern_matches(&p, "x"));
        assert!(!pattern_matches(&p, ""));
    }

    #[test]
    fn test_empty_pattern() {
        let p = compile_pattern("").unwrap();
        assert!(pattern_matches(&p, "anything"));
        assert!(pattern_matches(&p, ""));
    }

    #[test]
    fn test_pattern_substring() {
        let p = compile_pattern("ssh").unwrap();
        assert!(pattern_matches(&p, "sshd"));
        assert!(pattern_matches(&p, "openssh-server"));
        assert!(!pattern_matches(&p, "bash"));
    }

    // ---- Case-insensitive matching ----

    #[test]
    fn test_case_insensitive() {
        let mut p = compile_pattern("hello").unwrap();
        lower_pattern(&mut p);
        assert!(pattern_matches(&p, "hello"));
        assert!(pattern_matches(&p, "hello")); // already lower
        // For case-insensitive, caller lowers the text:
        assert!(pattern_matches(&p, "hello"));
    }

    #[test]
    fn test_case_insensitive_class() {
        let mut p = compile_pattern("[A-Z]").unwrap();
        lower_pattern(&mut p);
        // After lowering, [A-Z] becomes [a-z].
        assert!(pattern_matches(&p, "m"));
        assert!(pattern_matches(&p, "a"));
    }

    // ---- Mode detection ----

    #[test]
    fn test_detect_pgrep_mode() {
        assert_eq!(detect_mode("pgrep"), Mode::Pgrep);
        assert_eq!(detect_mode("/usr/bin/pgrep"), Mode::Pgrep);
        assert_eq!(detect_mode("pgrep.exe"), Mode::Pgrep);
    }

    #[test]
    fn test_detect_pkill_mode() {
        assert_eq!(detect_mode("pkill"), Mode::Pkill);
        assert_eq!(detect_mode("/usr/bin/pkill"), Mode::Pkill);
        assert_eq!(detect_mode("pkill.exe"), Mode::Pkill);
    }

    #[test]
    fn test_detect_unknown_defaults_pgrep() {
        assert_eq!(detect_mode("something"), Mode::Pgrep);
        assert_eq!(detect_mode(""), Mode::Pgrep);
    }

    // ---- Signal lookup ----

    #[test]
    fn test_signal_by_number() {
        let s = signal_by_number(9).unwrap();
        assert_eq!(s.name, "KILL");

        let s = signal_by_number(15).unwrap();
        assert_eq!(s.name, "TERM");

        assert!(signal_by_number(99).is_none());
    }

    #[test]
    fn test_signal_by_name() {
        let s = signal_by_name("KILL").unwrap();
        assert_eq!(s.number, 9);

        let s = signal_by_name("SIGTERM").unwrap();
        assert_eq!(s.number, 15);

        let s = signal_by_name("hup").unwrap();
        assert_eq!(s.number, 1);

        assert!(signal_by_name("BOGUS").is_none());
    }

    #[test]
    fn test_signal_by_name_sig_prefix() {
        let s = signal_by_name("SIGKILL").unwrap();
        assert_eq!(s.number, 9);
    }

    // ---- Argument parsing ----

    #[test]
    fn test_parse_basic_pgrep() {
        let args = vec!["pgrep".to_string(), "ssh".to_string()];
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.mode, Mode::Pgrep);
        assert_eq!(opts.pattern_str, "ssh");
        assert!(!opts.full_match);
    }

    #[test]
    fn test_parse_pgrep_with_flags() {
        let args = vec![
            "pgrep".to_string(),
            "-f".to_string(),
            "-i".to_string(),
            "-l".to_string(),
            "ssh".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(opts.full_match);
        assert!(opts.ignore_case);
        assert!(opts.list_name);
    }

    #[test]
    fn test_parse_pkill_with_signal() {
        let args = vec!["pkill".to_string(), "-9".to_string(), "runaway".to_string()];
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.mode, Mode::Pkill);
        assert_eq!(opts.signal, SignalAction::Kill);
        assert_eq!(opts.pattern_str, "runaway");
    }

    #[test]
    fn test_parse_pkill_signal_name() {
        let args = vec!["pkill".to_string(), "-HUP".to_string(), "myapp".to_string()];
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.signal, SignalAction::Hup);
    }

    #[test]
    fn test_parse_signal_long_form() {
        let args = vec![
            "pkill".to_string(),
            "--signal=KILL".to_string(),
            "app".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.signal, SignalAction::Kill);
    }

    #[test]
    fn test_parse_parent_filter() {
        let args = vec![
            "pgrep".to_string(),
            "-P".to_string(),
            "42".to_string(),
            "child".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.parent_pid, Some(42));
    }

    #[test]
    fn test_parse_exact_match() {
        let args = vec!["pgrep".to_string(), "-x".to_string(), "init".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.exact_match);
    }

    #[test]
    fn test_parse_count() {
        let args = vec!["pgrep".to_string(), "-c".to_string(), "bash".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.count_only);
    }

    #[test]
    fn test_parse_delimiter() {
        let args = vec![
            "pgrep".to_string(),
            "-d".to_string(),
            ",".to_string(),
            "proc".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert_eq!(opts.delimiter, ",");
    }

    #[test]
    fn test_parse_newest_oldest() {
        let args = vec!["pgrep".to_string(), "-n".to_string(), "bash".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.newest);
        assert!(!opts.oldest);

        let args = vec!["pgrep".to_string(), "-o".to_string(), "bash".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.oldest);
        assert!(!opts.newest);
    }

    #[test]
    fn test_parse_inverse() {
        let args = vec!["pgrep".to_string(), "-v".to_string(), "bash".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.inverse);
    }

    #[test]
    fn test_parse_echo() {
        let args = vec!["pkill".to_string(), "-e".to_string(), "app".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(opts.echo);
    }

    #[test]
    fn test_parse_no_pattern_is_error() {
        let args = vec!["pgrep".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_unknown_option_is_error() {
        let args = vec!["pgrep".to_string(), "--bogus".to_string(), "x".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn test_parse_help_returns_empty_error() {
        let args = vec!["pgrep".to_string(), "--help".to_string()];
        match parse_args(&args) {
            Err(msg) => assert!(msg.is_empty()),
            Ok(_) => panic!("expected Err for --help"),
        }
    }

    // ---- Process matching logic ----

    fn make_proc(pid: u32, name: &str, cmdline: &str) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: name.to_string(),
            cmdline: cmdline.to_string(),
            ppid: 1,
            pgrp: 1,
            tty: 0,
            starttime: pid as u64 * 100, // higher pid = newer
            ruid: 1000,
            euid: 1000,
        }
    }

    #[test]
    fn test_match_by_name() {
        let proc_info = make_proc(10, "sshd", "/usr/sbin/sshd -D");
        let args = vec!["pgrep".to_string(), "ssh".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));
    }

    #[test]
    fn test_match_full_cmdline() {
        let proc_info = make_proc(10, "python3", "python3 /opt/server.py --port 8080");
        let args = vec![
            "pgrep".to_string(),
            "-f".to_string(),
            "server\\.py".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));
    }

    #[test]
    fn test_match_full_cmdline_no_match_in_name() {
        let proc_info = make_proc(10, "python3", "python3 /opt/server.py");
        let args = vec!["pgrep".to_string(), "server".to_string()];
        let opts = parse_args(&args).unwrap();
        // Without -f, matches only against name "python3".
        assert!(!process_matches(&proc_info, &opts, 999));
    }

    #[test]
    fn test_match_exact() {
        let proc_info = make_proc(10, "ssh", "ssh user@host");
        let args = vec!["pgrep".to_string(), "-x".to_string(), "ssh".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));

        let proc_sshd = make_proc(11, "sshd", "/usr/sbin/sshd");
        assert!(!process_matches(&proc_sshd, &opts, 999));
    }

    #[test]
    fn test_match_inverse() {
        let proc_info = make_proc(10, "bash", "bash");
        let args = vec!["pgrep".to_string(), "-v".to_string(), "ssh".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));

        let proc_ssh = make_proc(11, "sshd", "sshd");
        assert!(!process_matches(&proc_ssh, &opts, 999));
    }

    #[test]
    fn test_match_excludes_self() {
        let proc_info = make_proc(999, "pgrep", "pgrep ssh");
        let args = vec!["pgrep".to_string(), "pgrep".to_string()];
        let opts = parse_args(&args).unwrap();
        // my_pid = 999 should be excluded.
        assert!(!process_matches(&proc_info, &opts, 999));
    }

    #[test]
    fn test_match_parent_filter() {
        let mut proc_info = make_proc(10, "worker", "worker");
        proc_info.ppid = 42;
        let args = vec![
            "pgrep".to_string(),
            "-P".to_string(),
            "42".to_string(),
            "worker".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));

        proc_info.ppid = 1;
        assert!(!process_matches(&proc_info, &opts, 999));
    }

    #[test]
    fn test_match_euid_filter() {
        let mut proc_info = make_proc(10, "daemon", "daemon");
        proc_info.euid = 0;
        let args = vec![
            "pgrep".to_string(),
            "-u".to_string(),
            "0".to_string(),
            "daemon".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));

        proc_info.euid = 1000;
        assert!(!process_matches(&proc_info, &opts, 999));
    }

    #[test]
    fn test_match_ruid_filter() {
        let mut proc_info = make_proc(10, "app", "app");
        proc_info.ruid = 500;
        let args = vec![
            "pgrep".to_string(),
            "-U".to_string(),
            "500".to_string(),
            "app".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));

        proc_info.ruid = 1000;
        assert!(!process_matches(&proc_info, &opts, 999));
    }

    #[test]
    fn test_match_pgrp_filter() {
        let mut proc_info = make_proc(10, "job", "job");
        proc_info.pgrp = 77;
        let args = vec![
            "pgrep".to_string(),
            "-g".to_string(),
            "77".to_string(),
            "job".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));

        proc_info.pgrp = 1;
        assert!(!process_matches(&proc_info, &opts, 999));
    }

    #[test]
    fn test_match_tty_filter() {
        let mut proc_info = make_proc(10, "shell", "shell");
        proc_info.tty = 3;
        let args = vec![
            "pgrep".to_string(),
            "-t".to_string(),
            "3".to_string(),
            "shell".to_string(),
        ];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_info, &opts, 999));

        proc_info.tty = 0;
        assert!(!process_matches(&proc_info, &opts, 999));
    }

    // ---- Alternation matching in process context ----

    #[test]
    fn test_match_alternation() {
        let proc_a = make_proc(10, "httpd", "httpd -DFOREGROUND");
        let proc_b = make_proc(11, "nginx", "nginx: master");
        let proc_c = make_proc(12, "bash", "bash");

        let args = vec!["pgrep".to_string(), "httpd|nginx".to_string()];
        let opts = parse_args(&args).unwrap();
        assert!(process_matches(&proc_a, &opts, 999));
        assert!(process_matches(&proc_b, &opts, 999));
        assert!(!process_matches(&proc_c, &opts, 999));
    }

    // ---- UID resolution ----

    #[test]
    fn test_resolve_numeric_uid() {
        assert_eq!(resolve_uid("0"), Some(0));
        assert_eq!(resolve_uid("1000"), Some(1000));
    }

    #[test]
    fn test_resolve_invalid_uid() {
        // Non-numeric, non-existent user.
        assert!(resolve_uid("definitely_not_a_user_xyzzy").is_none());
    }

    // ---- TTY resolution ----

    #[test]
    fn test_resolve_tty_numeric() {
        assert_eq!(resolve_tty("1"), Some(1));
        assert_eq!(resolve_tty("0"), Some(0));
    }

    #[test]
    fn test_resolve_tty_named() {
        assert_eq!(resolve_tty("tty1"), Some(1));
        assert_eq!(resolve_tty("tty42"), Some(42));
    }

    #[test]
    fn test_resolve_tty_pts() {
        assert_eq!(resolve_tty("pts/0"), Some(-1));
        assert_eq!(resolve_tty("pts/3"), Some(-4));
    }

    #[test]
    fn test_resolve_tty_none() {
        assert_eq!(resolve_tty("?"), Some(0));
        assert_eq!(resolve_tty("none"), Some(0));
    }

    #[test]
    fn test_resolve_tty_invalid() {
        assert!(resolve_tty("bogus").is_none());
    }

    // ---- Split alternatives ----

    #[test]
    fn test_split_alternatives_basic() {
        let parts = split_alternatives("a|b|c");
        assert_eq!(parts, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_split_alternatives_escaped_pipe() {
        let parts = split_alternatives(r"a\|b|c");
        assert_eq!(parts, vec![r"a\|b", "c"]);
    }

    #[test]
    fn test_split_alternatives_brackets() {
        let parts = split_alternatives("[a|b]|c");
        assert_eq!(parts, vec!["[a|b]", "c"]);
    }

    #[test]
    fn test_split_no_alternatives() {
        let parts = split_alternatives("hello");
        assert_eq!(parts, vec!["hello"]);
    }
}
