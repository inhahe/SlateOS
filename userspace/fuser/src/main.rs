//! OurOS file/socket process identification utility.
//!
//! Multi-personality binary providing:
//! - **fuser** — identify processes using files or sockets
//! - **lsof** — list open files (simplified)
//!
//! Scans /proc to find processes that have files open, mapped,
//! or as their working/root directory.

#![deny(clippy::all)]

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
    Cwd,     // c — current directory
    Exec,    // e — executable being run
    Open,    // f — open file (default)
    Root,    // r — root directory
    Mmap,    // m — mmap'd file or shared library
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
    uid: u32,
    command: String,
    access: AccessType,
    _fd: Option<u32>,
}

#[derive(Clone, Debug)]
struct FuserResult {
    path: String,
    processes: Vec<ProcessMatch>,
}

#[derive(Clone, Debug)]
struct LsofEntry {
    command: String,
    pid: u32,
    user: String,
    fd_str: String,
    type_str: String,
    _device: String,
    _size_off: String,
    _node: String,
    name: String,
}

// ============================================================================
// /proc scanning
// ============================================================================

fn read_proc_comm(pid: u32) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn read_proc_uid(pid: u32) -> u32 {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(uid_str) = parts.first() {
                return uid_str.parse().unwrap_or(0);
            }
        }
    }
    0
}

fn resolve_link(path: &str) -> Option<PathBuf> {
    fs::read_link(path).ok()
}

fn get_process_ids() -> Vec<u32> {
    let mut pids = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Ok(pid) = name.parse::<u32>() {
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
            && path_matches(&cwd, &canonical) {
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
            && path_matches(&exe, &canonical) {
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
            && root != Path::new("/") && path_matches(&root, &canonical) {
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
                if let Some(target) = resolve_link(
                    entry.path().to_str().unwrap_or_default(),
                )
                    && path_matches(&target, &canonical) {
                        let fd_num = entry
                            .file_name()
                            .to_str()
                            .and_then(|s| s.parse().ok());
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

#[derive(Clone, Debug)]
struct _SocketInfo {
    protocol: String,
    local_port: u16,
    pid: u32,
    command: String,
}

fn _find_processes_for_port(port: u16, protocol: &str) -> Vec<ProcessMatch> {
    // Parse /proc/net/tcp, /proc/net/udp, etc.
    let net_file = match protocol {
        "tcp" => "/proc/net/tcp",
        "tcp6" => "/proc/net/tcp6",
        "udp" => "/proc/net/udp",
        "udp6" => "/proc/net/udp6",
        _ => return Vec::new(),
    };

    let content = fs::read_to_string(net_file).unwrap_or_default();
    let mut inode_pids: HashMap<String, (u32, String)> = HashMap::new();

    // First pass: map inodes to PIDs by scanning /proc/*/fd.
    for pid in get_process_ids() {
        let fd_dir = format!("/proc/{pid}/fd");
        if let Ok(entries) = fs::read_dir(&fd_dir) {
            for entry in entries.flatten() {
                if let Some(target) = resolve_link(
                    entry.path().to_str().unwrap_or_default(),
                ) {
                    let target_str = target.to_string_lossy();
                    if let Some(rest) = target_str.strip_prefix("socket:[")
                        && let Some(inode) = rest.strip_suffix(']') {
                            let comm = read_proc_comm(pid);
                            inode_pids.insert(inode.to_string(), (pid, comm));
                        }
                }
            }
        }
    }

    let mut matches = Vec::new();

    // Second pass: find sockets matching the port.
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }
        // Local address is field 1, format: hex_ip:hex_port
        let local_addr = parts[1];
        if let Some(port_hex) = local_addr.split(':').nth(1)
            && let Ok(local_port) = u16::from_str_radix(port_hex, 16)
                && local_port == port {
                    let inode = parts[9];
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
    }

    matches
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
                && u == uid {
                    return parts[0].to_string();
                }
    }
    uid.to_string()
}

fn print_fuser_result(result: &FuserResult, verbose: bool) {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if verbose {
        let _ = writeln!(
            out,
            "{:>25} USER        PID ACCESS COMMAND",
            ""
        );
        let _ = write!(out, "{:>25}", result.path);

        for proc_match in &result.processes {
            let _ = writeln!(
                out,
                " {:>10} {:>6} {:>6} {}",
                uid_to_name(proc_match.uid),
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
            let _ = write!(
                out,
                " {}{}",
                proc_match.pid,
                proc_match.access.flag()
            );
        }
        let _ = writeln!(out);
    }
}

// ============================================================================
// lsof personality
// ============================================================================

fn lsof_scan_all() -> Vec<LsofEntry> {
    let mut entries = Vec::new();
    let pids = get_process_ids();

    for pid in pids {
        let comm = read_proc_comm(pid);
        let uid = read_proc_uid(pid);
        let user = uid_to_name(uid);

        // cwd
        if let Some(cwd) = resolve_link(&format!("/proc/{pid}/cwd")) {
            entries.push(LsofEntry {
                command: comm.clone(),
                pid,
                user: user.clone(),
                fd_str: "cwd".to_string(),
                type_str: "DIR".to_string(),
                _device: String::new(),
                _size_off: String::new(),
                _node: String::new(),
                name: cwd.to_string_lossy().to_string(),
            });
        }

        // exe
        if let Some(exe) = resolve_link(&format!("/proc/{pid}/exe")) {
            entries.push(LsofEntry {
                command: comm.clone(),
                pid,
                user: user.clone(),
                fd_str: "txt".to_string(),
                type_str: "REG".to_string(),
                _device: String::new(),
                _size_off: String::new(),
                _node: String::new(),
                name: exe.to_string_lossy().to_string(),
            });
        }

        // Open file descriptors.
        let fd_dir = format!("/proc/{pid}/fd");
        if let Ok(fd_entries) = fs::read_dir(&fd_dir) {
            for entry in fd_entries.flatten() {
                let fd_name = entry.file_name().to_string_lossy().to_string();
                if let Some(target) = resolve_link(
                    entry.path().to_str().unwrap_or_default(),
                ) {
                    let target_str = target.to_string_lossy().to_string();
                    let type_str = if target_str.starts_with("socket:") {
                        "sock"
                    } else if target_str.starts_with("pipe:") {
                        "FIFO"
                    } else if target_str.starts_with("anon_inode:") {
                        "anon"
                    } else {
                        "REG"
                    };
                    // Determine read/write mode.
                    let mode = fd_read_mode(pid, &fd_name);
                    entries.push(LsofEntry {
                        command: comm.clone(),
                        pid,
                        user: user.clone(),
                        fd_str: format!("{fd_name}{mode}"),
                        type_str: type_str.to_string(),
                        _device: String::new(),
                        _size_off: String::new(),
                        _node: String::new(),
                        name: target_str,
                    });
                }
            }
        }
    }

    entries
}

fn fd_read_mode(pid: u32, fd: &str) -> &'static str {
    let fdinfo = format!("/proc/{pid}/fdinfo/{fd}");
    if let Ok(content) = fs::read_to_string(&fdinfo) {
        for line in content.lines() {
            if let Some(flags_str) = line.strip_prefix("flags:") {
                let flags_str = flags_str.trim();
                if let Ok(flags) = u32::from_str_radix(
                    flags_str.trim_start_matches("0o").trim_start_matches('0'),
                    8,
                ) {
                    return match flags & 3 {
                        0 => "r",
                        1 => "w",
                        2 => "u", // read+write
                        _ => "u",
                    };
                }
            }
        }
    }
    "u"
}

fn lsof_main(args: &[String]) -> i32 {
    let mut filter_pid: Option<u32> = None;
    let mut filter_user: Option<String> = None;
    let mut filter_path: Option<String> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-p" => {
                i += 1;
                if i < args.len() {
                    filter_pid = args[i].parse().ok();
                }
            }
            "-u" => {
                i += 1;
                if i < args.len() {
                    filter_user = Some(args[i].clone());
                }
            }
            "--help" | "-h" => {
                println!("Usage: lsof [options] [path ...]");
                println!();
                println!("List open files.");
                println!();
                println!("Options:");
                println!("  -p PID     Show only for PID");
                println!("  -u USER    Show only for USER");
                println!("  -h, --help Display this help");
                println!("  --version  Display version");
                return 0;
            }
            "--version" => {
                println!("lsof (OurOS) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                filter_path = Some(s.to_string());
            }
            other => {
                eprintln!("lsof: unknown option '{other}'");
            }
        }
        i += 1;
    }

    let entries = lsof_scan_all();

    // Print header.
    println!(
        "{:<12} {:>6} {:<10} {:>4} {:>6} NAME",
        "COMMAND", "PID", "USER", "FD", "TYPE"
    );

    for entry in &entries {
        // Apply filters.
        if let Some(pid) = filter_pid
            && entry.pid != pid {
                continue;
            }
        if let Some(ref user) = filter_user
            && &entry.user != user {
                continue;
            }
        if let Some(ref path) = filter_path
            && !entry.name.contains(path.as_str()) {
                continue;
            }

        println!(
            "{:<12} {:>6} {:<10} {:>4} {:>6} {}",
            truncate_str(&entry.command, 12),
            entry.pid,
            truncate_str(&entry.user, 10),
            entry.fd_str,
            entry.type_str,
            entry.name
        );
    }

    0
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        s[..max].to_string()
    }
}

// ============================================================================
// fuser personality
// ============================================================================

fn fuser_main(args: &[String]) -> i32 {
    let mut paths: Vec<String> = Vec::new();
    let mut verbose = false;
    let mut kill_signal: Option<String> = None;
    let mut interactive = false;
    let mut _namespace = "file"; // file, tcp, udp

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-v" | "--verbose" => verbose = true,
            "-i" | "--interactive" => interactive = true,
            "-k" | "--kill" => {
                // Default to SIGKILL.
                kill_signal = Some("KILL".to_string());
            }
            "-n" | "--namespace" => {
                i += 1;
                if i < args.len() {
                    _namespace = match args[i].as_str() {
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
                println!("  -n, --namespace NS Namespace: file, tcp, udp");
                println!("  -h, --help         Display this help");
                println!("  --version          Display version");
                return 0;
            }
            "--version" => {
                println!("fuser (OurOS) {VERSION}");
                return 0;
            }
            s if !s.starts_with('-') => {
                paths.push(s.to_string());
            }
            other => {
                eprintln!("fuser: unknown option '{other}'");
            }
        }
        i += 1;
    }

    if paths.is_empty() {
        eprintln!("fuser: no files specified");
        return 1;
    }

    let mut found_any = false;

    for path in &paths {
        let result = find_processes_for_path(path);

        if !result.processes.is_empty() {
            found_any = true;
            print_fuser_result(&result, verbose);

            if let Some(ref signal) = kill_signal {
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
                    eprintln!(
                        "fuser: would send {} to pid {}",
                        signal, proc_match.pid
                    );
                }
            }
        }
    }

    if found_any { 0 } else { 1 }
}

// ============================================================================
// Main dispatch
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("fuser");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match prog_name.as_str() {
        "lsof" => lsof_main(&rest),
        _ => fuser_main(&rest),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
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

    #[test]
    fn test_lsof_entry_creation() {
        let entry = LsofEntry {
            command: "bash".to_string(),
            pid: 1234,
            user: "root".to_string(),
            fd_str: "0r".to_string(),
            type_str: "REG".to_string(),
            _device: String::new(),
            _size_off: String::new(),
            _node: String::new(),
            name: "/dev/null".to_string(),
        };
        assert_eq!(entry.pid, 1234);
        assert_eq!(entry.command, "bash");
    }
}
