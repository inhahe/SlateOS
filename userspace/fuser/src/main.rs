//! Slate OS file/socket process identification utility.
//!
//! Multi-personality binary providing:
//! - **fuser** — identify processes using files or sockets
//! - **lsof** — list open files (simplified)
//!
//! Scans /proc to find processes that have files open, mapped,
//! or as their working/root directory.

#![deny(clippy::all)]

use quoting::quoteaf_os;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
enum AccessType {
    Cwd,      // c — current directory
    Exec,     // e — executable being run
    Open,     // f — open file (default)
    Root,     // r — root directory
    Mmap,     // m — mmap'd file or shared library
    _Fd(u32), // file descriptor number
}

impl AccessType {
    fn flag(&self) -> &str {
        match self {
            AccessType::Cwd => "c",
            AccessType::Exec => "e",
            AccessType::Open => "f",
            AccessType::Root => "r",
            AccessType::Mmap => "m",
            AccessType::_Fd(_) => "f",
        }
    }
}

#[derive(Clone, Debug)]
struct ProcessMatch {
    pid: u32,
    /// `None` when the process's owner could not be read.
    uid: Option<u32>,
    command: String,
    access: AccessType,
    _fd: Option<u32>,
}

#[derive(Clone, Debug)]
struct FuserResult {
    path: String,
    processes: Vec<ProcessMatch>,
}

// ============================================================================
// /proc scanning
// ============================================================================

/// The process's command name, or `?` where it could not be read.
///
/// `?` rather than the empty string, matching the owner column beside it: a
/// process that exited between the directory listing and this read is the
/// ordinary case on a busy machine, and it printed as a blank command in a
/// table whose other columns were filled in. A blank cell reads as "this
/// process has no name", which is not a thing.
/// `fs::read`, not `read_to_string`, and the difference is visible to a user.
///
/// `read_to_string` fails on a name that is not UTF-8, and that failure landed
/// on the same `"?"` as a process that had exited — which is what the `"?"` is
/// documented above to mean. The kernel now carries `comm` as bytes end to
/// end, so a name with an arbitrary byte in it is a thing that reaches here,
/// and it is a different thing from an absent process.
///
/// `trim_comm` strips the trailing newline only: `.trim()` also ate a leading
/// space, which is a legal part of a name.
fn read_proc_comm(pid: u32) -> String {
    match fs::read(format!("/proc/{pid}/comm")) {
        Ok(raw) => procinfo::display_bytes(procinfo::trim_comm(&raw)),
        Err(_) => "?".to_string(),
    }
}

/// The owner of `pid`, or `None` where it could not be read.
///
/// # Why `None` and not `0`
///
/// Every step of this used to answer `0`, and `uid_to_name(0)` is "root": an
/// unreadable `/proc/<pid>/status` returned 0, a `Uid:` line that would not
/// parse returned 0, and a status file with no `Uid:` line at all fell out of
/// the loop to a bare `0`. So a process this program could not identify was
/// reported as owned by the SUPERUSER, in a tool whose entire output is "who
/// is holding this file".
///
/// A process exiting between the directory listing and this read is the
/// ordinary case, not an exotic one, so this is a state the program reaches
/// on a busy machine rather than a corner.
///
/// `userspace/lsof` had the identical defect through
/// `read_uid(pid).unwrap_or(0)` and was repaired the same day.
fn read_proc_uid(pid: u32) -> Option<u32> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            // A `Uid:` line whose first field is not a number is a malformed
            // status file, which is not the same as uid 0 either.
            return parts.first().and_then(|s| s.parse().ok());
        }
    }
    None
}

fn resolve_link(path: &str) -> Option<PathBuf> {
    fs::read_link(path).ok()
}

fn get_process_ids() -> Vec<u32> {
    let mut pids = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Ok(pid) = name.parse::<u32>()
            {
                pids.push(pid);
            }
        }
    }
    pids
}

fn path_matches(link_target: &Path, search_path: &Path) -> bool {
    // Exact match or the link target starts with search_path (for directory matches).
    link_target == search_path || link_target.starts_with(search_path)
}

/// The signal number `spec` names, or `None` if it names nothing.
///
/// Accepts what `fuser -s` accepts: a decimal number, a bare name (`TERM`), or
/// a prefixed one (`SIGTERM`), case-insensitively. Numbers outside 1..=64 are
/// rejected rather than passed through -- `kill(pid, 0)` is a permission probe
/// that kills nothing, and silently turning a typo into one would report
/// success for a process still running.
fn signal_number(spec: &str) -> Option<i32> {
    let spec = spec.trim();
    if let Ok(n) = spec.parse::<i32>() {
        return if (1..=64).contains(&n) { Some(n) } else { None };
    }
    let name = spec
        .strip_prefix("SIG")
        .unwrap_or(spec)
        .to_ascii_uppercase();
    let name = name.strip_prefix("SIG").unwrap_or(&name);
    Some(match name {
        "HUP" => 1,
        "INT" => 2,
        "QUIT" => 3,
        "ILL" => 4,
        "TRAP" => 5,
        "ABRT" | "IOT" => 6,
        "BUS" => 7,
        "FPE" => 8,
        "KILL" => 9,
        "USR1" => 10,
        "SEGV" => 11,
        "USR2" => 12,
        "PIPE" => 13,
        "ALRM" => 14,
        "TERM" => 15,
        "STKFLT" => 16,
        "CHLD" | "CLD" => 17,
        "CONT" => 18,
        "STOP" => 19,
        "TSTP" => 20,
        "TTIN" => 21,
        "TTOU" => 22,
        "URG" => 23,
        "XCPU" => 24,
        "XFSZ" => 25,
        "VTALRM" => 26,
        "PROF" => 27,
        "WINCH" => 28,
        "IO" | "POLL" => 29,
        "PWR" => 30,
        "SYS" => 31,
        _ => return None,
    })
}

/// Send `sig` to `pid`.
///
/// # Why `kill(2)` and not `SYS_PROCESS_KILL`
///
/// The tree has two ways to end a process and they are not interchangeable.
/// The native `SYS_PROCESS_KILL` (506) takes a PID and an **exit code**, and
/// `userspace/kill` and `userspace/pgrep` use it with the shell's 128+signal
/// convention -- 143 for TERM, 137 for KILL. `fuser` is a Linux-compatible
/// tool whose `-s` argument is a **signal**, so it goes through `kill(2)`,
/// which takes the signal number the user actually named and lets the libc
/// decide how that maps. `posix/src/signal.rs` exports it as a C symbol on
/// this target and routes it to `SYS_SIGNAL_SEND`.
///
/// Until 2026-09-10 this printed `fuser: would send KILL to pid 1234` and
/// returned 0. With `-i` that was worse than a plain no-op: the program asked
/// "Kill process 1234? (y/N)", waited for the answer, and then did nothing
/// with it -- so a user who typed `y` was told the thing they had just
/// authorised had happened.
#[cfg(unix)]
fn send_signal(pid: u32, sig: i32) -> Result<(), io::Error> {
    // SAFETY: `kill` takes two integers by value and returns one. There are no
    // pointers, no lifetimes and no allocation; the only failure mode is the
    // documented -1 with errno set, which is read immediately below.
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let target = i32::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "pid does not fit in pid_t"))?;
    // SAFETY: as above.
    if unsafe { kill(target, sig) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The development host has no `kill(2)`.
///
/// Refusing is the only honest option and matches `design-decisions.md` 1019:
/// the caller asked for a process to be ended, and reporting success without
/// ending it is the failure this whole change exists to remove.
#[cfg(not(unix))]
fn send_signal(_pid: u32, _sig: i32) -> Result<(), io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "this host has no kill(2)",
    ))
}

fn find_processes_for_path(search_path: &str) -> FuserResult {
    let search = PathBuf::from(search_path);
    let canonical = fs::canonicalize(&search).unwrap_or_else(|_| search.clone());
    let mut processes = Vec::new();
    let pids = get_process_ids();

    for pid in pids {
        let uid = read_proc_uid(pid);
        let comm = read_proc_comm(pid);

        // Check cwd.
        if let Some(cwd) = resolve_link(&format!("/proc/{pid}/cwd"))
            && path_matches(&cwd, &canonical)
        {
            processes.push(ProcessMatch {
                pid,
                uid,
                command: comm.clone(),
                access: AccessType::Cwd,
                _fd: None,
            });
        }

        // Check exe.
        if let Some(exe) = resolve_link(&format!("/proc/{pid}/exe"))
            && path_matches(&exe, &canonical)
        {
            processes.push(ProcessMatch {
                pid,
                uid,
                command: comm.clone(),
                access: AccessType::Exec,
                _fd: None,
            });
        }

        // Check root.
        if let Some(root) = resolve_link(&format!("/proc/{pid}/root"))
            && root != Path::new("/")
            && path_matches(&root, &canonical)
        {
            processes.push(ProcessMatch {
                pid,
                uid,
                command: comm.clone(),
                access: AccessType::Root,
                _fd: None,
            });
        }

        // Check open fds.
        let fd_dir = format!("/proc/{pid}/fd");
        if let Ok(entries) = fs::read_dir(&fd_dir) {
            for entry in entries.flatten() {
                if let Some(target) = resolve_link(entry.path().to_str().unwrap_or_default())
                    && path_matches(&target, &canonical)
                {
                    let fd_num = entry.file_name().to_str().and_then(|s| s.parse().ok());
                    processes.push(ProcessMatch {
                        pid,
                        uid,
                        command: comm.clone(),
                        access: AccessType::Open,
                        _fd: fd_num,
                    });
                }
            }
        }

        // Check memory maps for mmap'd files.
        let maps_path = format!("/proc/{pid}/maps");
        if let Ok(maps) = fs::read_to_string(&maps_path) {
            let canonical_str = canonical.to_string_lossy();
            for line in maps.lines() {
                if line.contains(canonical_str.as_ref()) {
                    processes.push(ProcessMatch {
                        pid,
                        uid,
                        command: comm.clone(),
                        access: AccessType::Mmap,
                        _fd: None,
                    });
                    break; // Only report mmap once per process.
                }
            }
        }
    }

    // Deduplicate by (pid, access_type).
    let mut seen = std::collections::HashSet::new();
    processes.retain(|p| {
        let key = (p.pid, p.access.flag().to_string());
        seen.insert(key)
    });

    FuserResult {
        path: search_path.to_string(),
        processes,
    }
}

// ============================================================================
// Network socket matching
// ============================================================================

/// The socket inodes in a `/proc/net/{tcp,udp}` table bound to `port`.
///
/// Split out from the scan because it is the only part of a port lookup that
/// can be tested off a real `/proc`: everything else needs live processes.
///
/// The table's columns are fixed -- field 1 is `hex_ip:hex_port`, field 9 is
/// the inode -- and a short line is skipped rather than indexed into, since
/// the header and any future trailing column must not panic here.
fn inodes_bound_to(table: &str, port: u16) -> Vec<&str> {
    let mut inodes = Vec::new();
    for line in table.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }
        // Local address is field 1, format: hex_ip:hex_port
        let Some(local) = parts.get(1) else { continue };
        if let Some(port_hex) = local.split(':').nth(1)
            && let Ok(local_port) = u16::from_str_radix(port_hex, 16)
            && local_port == port
            && let Some(inode) = parts.get(9)
        {
            inodes.push(*inode);
        }
    }
    inodes
}

/// The processes holding a socket bound to `port` on `protocol`.
///
/// # Why this returns a `Result`
///
/// It read `/proc/net/tcp` with `.unwrap_or_default()` until this commit, so an
/// unreadable table became an empty one and the caller reported "no process
/// found" -- indistinguishable from a genuinely free port. For a value that
/// decides whether a port is in use, "I cannot tell" and "nothing is there"
/// must not be the same answer. That was survivable only because nothing
/// called this function; wiring `-n` up is what makes it reachable, so the fix
/// is part of the wiring rather than a tidy-up after it.
fn find_processes_for_port(port: u16, protocol: &str) -> Result<Vec<ProcessMatch>, String> {
    // Parse /proc/net/tcp, /proc/net/udp, etc.
    let net_file = match protocol {
        "tcp" => "/proc/net/tcp",
        "tcp6" => "/proc/net/tcp6",
        "udp" => "/proc/net/udp",
        "udp6" => "/proc/net/udp6",
        other => return Err(format!("unknown namespace {}", quoteaf_os(other))),
    };

    let content =
        fs::read_to_string(net_file).map_err(|e| format!("cannot read {net_file}: {e}"))?;
    let mut inode_pids: HashMap<String, (u32, String)> = HashMap::new();

    // First pass: map inodes to PIDs by scanning /proc/*/fd.
    for pid in get_process_ids() {
        let fd_dir = format!("/proc/{pid}/fd");
        if let Ok(entries) = fs::read_dir(&fd_dir) {
            for entry in entries.flatten() {
                if let Some(target) = resolve_link(entry.path().to_str().unwrap_or_default()) {
                    let target_str = target.to_string_lossy();
                    if let Some(rest) = target_str.strip_prefix("socket:[")
                        && let Some(inode) = rest.strip_suffix(']')
                    {
                        let comm = read_proc_comm(pid);
                        inode_pids.insert(inode.to_string(), (pid, comm));
                    }
                }
            }
        }
    }

    let mut matches = Vec::new();

    // Second pass: find sockets matching the port.
    for inode in inodes_bound_to(&content, port) {
        if let Some((pid, comm)) = inode_pids.get(inode) {
            matches.push(ProcessMatch {
                pid: *pid,
                uid: read_proc_uid(*pid),
                command: comm.clone(),
                access: AccessType::Open,
                _fd: None,
            });
        }
    }

    Ok(matches)
}

// ============================================================================
// Output formatting
// ============================================================================

fn uid_to_name(uid: u32) -> String {
    let passwd = fs::read_to_string("/etc/passwd").unwrap_or_default();
    for line in passwd.lines() {
        let parts: Vec<&str> = line.splitn(7, ':').collect();
        if parts.len() >= 3
            && let Ok(u) = parts[2].parse::<u32>()
            && u == uid
        {
            return parts[0].to_string();
        }
    }
    uid.to_string()
}

fn print_fuser_result(result: &FuserResult, verbose: bool) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if verbose {
        let _ = writeln!(out, "{:>25} USER        PID ACCESS COMMAND", "");
        let _ = write!(out, "{:>25}", result.path);

        for proc_match in &result.processes {
            let _ = writeln!(
                out,
                " {:>10} {:>6} {:>6} {}",
                match proc_match.uid {
                    Some(u) => uid_to_name(u),
                    // Not "root", and not a blank column either: the reader has
                    // to be able to tell this apart from a process genuinely
                    // owned by someone.
                    None => "?".to_string(),
                },
                proc_match.pid,
                proc_match.access.flag(),
                proc_match.command
            );
            let _ = write!(out, "{:>25}", "");
        }
        let _ = writeln!(out);
    } else {
        // Standard fuser output: path: pid(access)pid(access)...
        let _ = write!(out, "{}:", result.path);
        for proc_match in &result.processes {
            let _ = write!(out, " {}{}", proc_match.pid, proc_match.access.flag());
        }
        let _ = writeln!(out);
    }
}

// ============================================================================
// lsof personality
// ============================================================================

// ============================================================================
// fuser personality
// ============================================================================

fn fuser_main(args: &[String]) -> i32 {
    let mut paths: Vec<String> = Vec::new();
    let mut verbose = false;
    let mut kill_signal: Option<String> = None;
    let mut interactive = false;
    let mut show_all = false;
    let mut namespace = "file"; // file, tcp, udp

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-v" | "--verbose" => verbose = true,
            "-i" | "--interactive" => interactive = true,
            "-a" | "--all" => show_all = true,
            "-k" | "--kill" => {
                // Default to SIGKILL.
                kill_signal = Some("KILL".to_string());
            }
            "-n" | "--namespace" => {
                i += 1;
                if i < args.len() {
                    namespace = match args[i].as_str() {
                        "tcp" => "tcp",
                        "udp" => "udp",
                        _ => "file",
                    };
                }
            }
            "-s" | "--signal" => {
                i += 1;
                if i < args.len() {
                    kill_signal = Some(args[i].clone());
                }
            }
            "--help" | "-h" => {
                println!("Usage: fuser [options] file|port ...");
                println!();
                println!("Identify processes using files or sockets.");
                println!();
                println!("Options:");
                println!("  -v, --verbose      Verbose output");
                println!("  -k, --kill         Kill processes");
                println!("  -s, --signal SIG   Signal to send (default: KILL)");
                println!("  -i, --interactive  Confirm before killing");
                println!("  -a, --all          Show targets with no processes too");
                println!("  -n, --namespace NS Namespace: file, tcp, udp");
                println!("  -h, --help         Display this help");
                println!("  --version          Display version");
                return 0;
            }
            "--version" => {
                println!("fuser (Slate OS) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                paths.push(s.to_string());
            }
            other => {
                eprintln!("fuser: unknown option {}", quoteaf_os(other));
            }
        }
        i += 1;
    }

    if paths.is_empty() {
        eprintln!("fuser: no files specified");
        return 1;
    }

    let mut found_any = false;
    // A signal that could not be delivered is a failure of the whole run, and
    // the exit status is the only part of it a script reads.
    let mut kill_failed = false;

    for path in &paths {
        let processes = if namespace == "file" {
            find_processes_for_path(path).processes
        } else {
            // In a port namespace the operand is a port, not a path. Refusing
            // here rather than falling back to a file search is the point: a
            // fallback would search for a FILE named "80" and report nothing,
            // which reads exactly like "port 80 is free".
            let Ok(port) = path.parse::<u16>() else {
                eprintln!("fuser: {} is not a port number", quoteaf_os(path));
                return 1;
            };
            match find_processes_for_port(port, namespace) {
                Ok(found) => found,
                Err(e) => {
                    eprintln!("fuser: {e}");
                    return 1;
                }
            }
        };
        let result = FuserResult {
            path: path.clone(),
            processes,
        };

        // -a reports a target that nothing is using. The non-verbose format
        // already writes "path:" before the PID list, so an empty list prints
        // the bare name, which is what real fuser -a does.
        if result.processes.is_empty() {
            if show_all {
                print_fuser_result(&result, verbose);
            }
        } else {
            found_any = true;
            print_fuser_result(&result, verbose);

            if let Some(ref signal) = kill_signal {
                let Some(signum) = signal_number(signal) else {
                    eprintln!("fuser: unknown signal {}", quoteaf_os(signal));
                    return 1;
                };
                for proc_match in &result.processes {
                    if interactive {
                        eprint!(
                            "Kill process {} ({})? (y/N) ",
                            proc_match.pid, proc_match.command
                        );
                        let _ = io::stderr().flush();
                        let mut answer = String::new();
                        let _ = io::stdin().read_line(&mut answer);
                        if !answer.trim().eq_ignore_ascii_case("y") {
                            continue;
                        }
                    }
                    if let Err(e) = send_signal(proc_match.pid, signum) {
                        eprintln!("fuser: cannot send {signal} to pid {}: {e}", proc_match.pid);
                        kill_failed = true;
                    }
                }
            }
        }
    }

    if kill_failed {
        1
    } else if found_any {
        0
    } else {
        1
    }
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    // No personality probe: `lsof` is `userspace/lsof`, which this could
    // never out-rank -- it produces the executable, and its option set is a
    // strict superset of the one that stood here.
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    process::exit(fuser_main(&rest));
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_names_are_accepted_in_every_spelling() {
        assert_eq!(signal_number("TERM"), Some(15));
        assert_eq!(signal_number("SIGTERM"), Some(15));
        assert_eq!(signal_number("sigterm"), Some(15));
        assert_eq!(signal_number(" KILL "), Some(9));
        assert_eq!(signal_number("HUP"), Some(1));
        // Two names for one number, both real.
        assert_eq!(signal_number("ABRT"), signal_number("IOT"));
        assert_eq!(signal_number("CHLD"), signal_number("CLD"));
    }

    #[test]
    fn numeric_specs_are_accepted_within_range() {
        assert_eq!(signal_number("9"), Some(9));
        assert_eq!(signal_number("1"), Some(1));
        assert_eq!(signal_number("64"), Some(64));
    }

    /// `0` is the case that matters and it is REFUSED.
    ///
    /// `kill(pid, 0)` is a permission-and-existence probe that kills nothing.
    /// Accepting it here would let `fuser -k -s 0` report success for every
    /// process it "killed", all of which would still be running -- the exact
    /// shape of defect this crate was fixed to stop having.
    #[test]
    fn signal_zero_is_refused_because_it_kills_nothing() {
        assert_eq!(signal_number("0"), None);
    }

    #[test]
    fn nonsense_is_refused_rather_than_guessed() {
        assert_eq!(signal_number("65"), None);
        assert_eq!(signal_number("-1"), None);
        assert_eq!(signal_number("TERMINATE"), None);
        assert_eq!(signal_number(""), None);
        assert_eq!(signal_number("SIG"), None);
    }

    #[test]
    fn test_access_type_flags() {
        assert_eq!(AccessType::Cwd.flag(), "c");
        assert_eq!(AccessType::Exec.flag(), "e");
        assert_eq!(AccessType::Open.flag(), "f");
        assert_eq!(AccessType::Root.flag(), "r");
        assert_eq!(AccessType::Mmap.flag(), "m");
        assert_eq!(AccessType::_Fd(3).flag(), "f");
    }

    #[test]
    fn test_path_matches_exact() {
        let search = Path::new("/usr/bin/vim");
        let target = Path::new("/usr/bin/vim");
        assert!(path_matches(target, search));
    }

    #[test]
    fn test_path_matches_prefix() {
        let search = Path::new("/home/user");
        let target = Path::new("/home/user/documents/file.txt");
        assert!(path_matches(target, search));
    }

    #[test]
    fn test_path_no_match() {
        let search = Path::new("/usr/bin/vim");
        let target = Path::new("/usr/bin/emacs");
        assert!(!path_matches(target, search));
    }

    #[test]
    fn test_uid_to_name_fallback() {
        // Should fall back to numeric string for unknown UIDs.
        let name = uid_to_name(99999);
        assert_eq!(name, "99999");
    }

    #[test]
    fn test_get_process_ids() {
        // Should return at least our own process.
        let pids = get_process_ids();
        // On non-Linux systems this may be empty, which is fine.
        let _ = pids;
    }

    #[test]
    fn test_find_processes_for_nonexistent() {
        let result = find_processes_for_path("/nonexistent/path/that/does/not/exist");
        // Should return empty results, not crash.
        assert!(result.processes.is_empty());
    }

    // -- port lookup ---------------------------------------------------------

    /// Two rows from a real `/proc/net/tcp`: sshd on 22 (0016) and a listener
    /// on 80 (0050), plus the header every such table carries.
    const NET_TCP: &str = concat!(
        "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
",
        "   0: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 10501 1 ffff 100 0 0 10 0
",
        "   1: 0100007F:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 27183 1 ffff 100 0 0 10 0
",
    );

    #[test]
    fn a_bound_port_yields_its_socket_inode() {
        assert_eq!(inodes_bound_to(NET_TCP, 22), vec!["10501"]);
        assert_eq!(inodes_bound_to(NET_TCP, 80), vec!["27183"]);
    }

    #[test]
    fn an_unbound_port_yields_nothing() {
        assert!(inodes_bound_to(NET_TCP, 443).is_empty());
    }

    #[test]
    fn the_header_row_is_not_read_as_a_socket() {
        // "local_address" has no colon-separated hex port; parsing it as one
        // would put a garbage inode in front of the fd scan.
        assert!(inodes_bound_to(NET_TCP, 0).is_empty());
    }

    #[test]
    fn a_short_or_empty_table_is_skipped_rather_than_indexed() {
        // A truncated read must not panic: this ran under `unwrap_or_default`
        // until now, so malformed input was the expected case, not the odd one.
        assert!(inodes_bound_to("", 80).is_empty());
        assert!(
            inodes_bound_to(
                "header only
",
                80
            )
            .is_empty()
        );
        assert!(
            inodes_bound_to(
                "hdr
   0: 0100007F:0050 00000000:0000
",
                80
            )
            .is_empty()
        );
    }

    #[test]
    fn an_unknown_namespace_is_an_error_not_an_empty_result() {
        // It returned Vec::new() for an unknown protocol, which the caller
        // would have printed as "no process is using that port".
        assert!(find_processes_for_port(80, "sctp").is_err());
    }
}
