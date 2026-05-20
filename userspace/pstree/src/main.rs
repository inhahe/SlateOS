//! OurOS process tree display utilities.
//!
//! Multi-personality binary providing:
//! - **pstree** — display process tree
//! - **pgrep** variant with tree view
//!
//! Reads `/proc/<pid>/stat` and `/proc/<pid>/status` to build the process
//! hierarchy and display it as an ASCII/Unicode tree.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// Unicode box-drawing characters.
const TREE_BRANCH: &str = "├── ";
const TREE_LAST: &str = "└── ";
const TREE_PIPE: &str = "│   ";
const TREE_SPACE: &str = "    ";

// ASCII alternatives.
const ASCII_BRANCH: &str = "|-- ";
const ASCII_LAST: &str = "`-- ";
const ASCII_PIPE: &str = "|   ";
const ASCII_SPACE: &str = "    ";

// ============================================================================
// Data structures
// ============================================================================

/// Process information from /proc.
#[derive(Clone, Debug)]
struct ProcessInfo {
    pid: u32,
    ppid: u32,
    name: String,
    uid: u32,
    threads: u32,
    state: char,
    username: String,
}

/// Display options.
struct Options {
    /// Show PIDs.
    show_pids: bool,
    /// Show UIDs/usernames.
    show_uid: bool,
    /// Show thread counts.
    show_threads: bool,
    /// Use ASCII characters instead of Unicode.
    ascii: bool,
    /// Compact display (merge identical subtrees).
    compact: bool,
    /// Show only a specific PID's subtree.
    root_pid: Option<u32>,
    /// Highlight a specific PID.
    highlight_pid: Option<u32>,
    /// Show kernel threads.
    show_kernel: bool,
    /// Long format (show command line args).
    long_format: bool,
    /// Sort by PID (default) or name.
    sort_by_name: bool,
    /// Show arguments.
    show_args: bool,
    /// Numeric UIDs only.
    numeric_uid: bool,
}

// ============================================================================
// Process reading
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Read process info from /proc/<pid>/stat.
fn read_proc_stat(pid: u32) -> Option<ProcessInfo> {
    let stat_content = read_file(&format!("/proc/{pid}/stat"))?;

    // Parse comm field (in parens, may contain spaces).
    let open_paren = stat_content.find('(')?;
    let close_paren = stat_content.rfind(')')?;
    let name = stat_content[open_paren + 1..close_paren].to_string();

    // Fields after the closing paren.
    let rest = stat_content[close_paren + 2..].trim();
    let fields: Vec<&str> = rest.split_whitespace().collect();

    // Field 0 = state, Field 1 = ppid, Field 17 = num_threads.
    let state = fields.first()
        .and_then(|s| s.chars().next())
        .unwrap_or('?');
    let ppid: u32 = fields.get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let threads: u32 = fields.get(17)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    // Read UID from /proc/<pid>/status.
    let uid = read_proc_uid(pid);
    let username = uid_to_name(uid);

    Some(ProcessInfo {
        pid,
        ppid,
        name,
        uid,
        threads,
        state,
        username,
    })
}

fn read_proc_uid(pid: u32) -> u32 {
    if let Some(content) = read_file(&format!("/proc/{pid}/status")) {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("Uid:") {
                return val.trim().split_whitespace().next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
            }
        }
    }
    0
}

fn read_proc_cmdline(pid: u32) -> String {
    if let Some(content) = read_file(&format!("/proc/{pid}/cmdline")) {
        let args: String = content.replace('\0', " ");
        let args = args.trim().to_string();
        if !args.is_empty() {
            return args;
        }
    }
    String::new()
}

/// Resolve UID to username via /etc/passwd.
fn uid_to_name(uid: u32) -> String {
    if let Some(content) = read_file("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3 {
                if let Ok(file_uid) = fields[2].parse::<u32>() {
                    if file_uid == uid {
                        return fields[0].to_string();
                    }
                }
            }
        }
    }
    uid.to_string()
}

/// Enumerate all PIDs from /proc.
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
    pids.sort_unstable();
    pids
}

/// Build the full process map.
fn build_process_map() -> HashMap<u32, ProcessInfo> {
    let mut map = HashMap::new();
    for pid in enumerate_pids() {
        if let Some(info) = read_proc_stat(pid) {
            map.insert(pid, info);
        }
    }
    map
}

/// Build parent → children map.
fn build_children_map(procs: &HashMap<u32, ProcessInfo>) -> HashMap<u32, Vec<u32>> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for info in procs.values() {
        children.entry(info.ppid).or_default().push(info.pid);
    }
    // Sort children by PID (default) for stable output.
    for kids in children.values_mut() {
        kids.sort_unstable();
    }
    children
}

// ============================================================================
// Tree rendering
// ============================================================================

fn format_process(info: &ProcessInfo, opts: &Options) -> String {
    let mut parts = Vec::new();

    // Process name (or command line if long format).
    let name = if opts.long_format || opts.show_args {
        let cmdline = read_proc_cmdline(info.pid);
        if cmdline.is_empty() {
            format!("{{{}}}", info.name) // Kernel thread: {name}
        } else {
            cmdline
        }
    } else {
        info.name.clone()
    };
    parts.push(name);

    // PID.
    if opts.show_pids {
        parts.push(format!("({})", info.pid));
    }

    // UID/username.
    if opts.show_uid {
        if opts.numeric_uid {
            parts.push(format!("[{}]", info.uid));
        } else {
            parts.push(format!("[{}]", info.username));
        }
    }

    // Thread count.
    if opts.show_threads && info.threads > 1 {
        parts.push(format!("{{{} threads}}", info.threads));
    }

    parts.join("")
}

fn render_tree(
    out: &mut io::StdoutLock<'_>,
    pid: u32,
    procs: &HashMap<u32, ProcessInfo>,
    children: &HashMap<u32, Vec<u32>>,
    opts: &Options,
    prefix: &str,
    is_last: bool,
    is_root: bool,
) {
    let info = match procs.get(&pid) {
        Some(i) => i,
        None => return,
    };

    // Skip kernel threads if not requested.
    if !opts.show_kernel && info.ppid == 2 && pid != 2 {
        return;
    }

    let (branch, last, pipe, space) = if opts.ascii {
        (ASCII_BRANCH, ASCII_LAST, ASCII_PIPE, ASCII_SPACE)
    } else {
        (TREE_BRANCH, TREE_LAST, TREE_PIPE, TREE_SPACE)
    };

    // Print this node.
    let display = format_process(info, opts);

    if is_root {
        let _ = writeln!(out, "{display}");
    } else {
        let connector = if is_last { last } else { branch };
        let _ = writeln!(out, "{prefix}{connector}{display}");
    }

    // Print children.
    let mut kids = children.get(&pid).cloned().unwrap_or_default();

    // Sort by name if requested.
    if opts.sort_by_name {
        kids.sort_by(|a, b| {
            let name_a = procs.get(a).map(|p| &p.name).unwrap_or(&String::new()).clone();
            let name_b = procs.get(b).map(|p| &p.name).unwrap_or(&String::new()).clone();
            name_a.cmp(&name_b)
        });
    }

    // Compact mode: merge children with same name.
    if opts.compact {
        let mut merged: Vec<(u32, u32)> = Vec::new(); // (pid, count)
        let mut prev_name = String::new();
        for &kid_pid in &kids {
            let kid_name = procs.get(&kid_pid).map(|p| &p.name).cloned().unwrap_or_default();
            if kid_name == prev_name && !merged.is_empty() {
                if let Some(last_entry) = merged.last_mut() {
                    last_entry.1 += 1;
                }
            } else {
                merged.push((kid_pid, 1));
                prev_name = kid_name;
            }
        }

        let child_prefix = if is_root {
            String::new()
        } else if is_last {
            format!("{prefix}{space}")
        } else {
            format!("{prefix}{pipe}")
        };

        for (idx, &(kid_pid, count)) in merged.iter().enumerate() {
            let kid_is_last = idx + 1 == merged.len();
            if count > 1 {
                let kid_info = procs.get(&kid_pid);
                let name = kid_info.map(|p| &p.name).cloned().unwrap_or_default();
                let connector = if kid_is_last { last } else { branch };
                let _ = writeln!(out, "{child_prefix}{connector}{count}*[{name}]");
            } else {
                render_tree(out, kid_pid, procs, children, opts, &child_prefix, kid_is_last, false);
            }
        }
    } else {
        let child_prefix = if is_root {
            String::new()
        } else if is_last {
            format!("{prefix}{space}")
        } else {
            format!("{prefix}{pipe}")
        };

        for (idx, &kid_pid) in kids.iter().enumerate() {
            let kid_is_last = idx + 1 == kids.len();
            render_tree(out, kid_pid, procs, children, opts, &child_prefix, kid_is_last, false);
        }
    }
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_usage() {
    println!("Usage: pstree [options] [PID|USER]");
    println!();
    println!("Display a tree of processes.");
    println!();
    println!("Options:");
    println!("  -p, --show-pids     Show PIDs");
    println!("  -u, --uid-changes   Show UID/username changes");
    println!("  -t, --threads       Show thread counts");
    println!("  -a, --arguments     Show command line arguments");
    println!("  -l, --long          Long format (full command line)");
    println!("  -c, --compact=no    Don't compact identical subtrees");
    println!("  -A, --ascii         Use ASCII line drawing");
    println!("  -n, --numeric-sort  Sort by PID (default)");
    println!("  -N, --name-sort     Sort by process name");
    println!("  -k, --show-kernel   Show kernel threads");
    println!("  -g, --numeric-uid   Show numeric UIDs");
    println!("  -H PID, --highlight-pid=PID  Highlight a PID");
    println!("  -h, --help          Show this help");
    println!("  -V, --version       Show version");
}

fn parse_args() -> Options {
    let args: Vec<String> = env::args().collect();
    let mut opts = Options {
        show_pids: false,
        show_uid: false,
        show_threads: false,
        ascii: false,
        compact: true,
        root_pid: None,
        highlight_pid: None,
        show_kernel: false,
        long_format: false,
        sort_by_name: false,
        show_args: false,
        numeric_uid: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" | "help" => {
                print_usage();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("pstree {VERSION}");
                process::exit(0);
            }
            "-p" | "--show-pids" => opts.show_pids = true,
            "-u" | "--uid-changes" => opts.show_uid = true,
            "-t" | "--threads" => opts.show_threads = true,
            "-a" | "--arguments" => opts.show_args = true,
            "-l" | "--long" => opts.long_format = true,
            "-c" => opts.compact = false,
            "--compact=no" => opts.compact = false,
            "-A" | "--ascii" => opts.ascii = true,
            "-n" | "--numeric-sort" => opts.sort_by_name = false,
            "-N" | "--name-sort" => opts.sort_by_name = true,
            "-k" | "--show-kernel" => opts.show_kernel = true,
            "-g" | "--numeric-uid" => {
                opts.numeric_uid = true;
                opts.show_uid = true;
            }
            "-H" | "--highlight-pid" => {
                i += 1;
                if i < args.len() {
                    opts.highlight_pid = args[i].parse().ok();
                }
            }
            s if s.starts_with("--highlight-pid=") => {
                opts.highlight_pid = s.strip_prefix("--highlight-pid=")
                    .and_then(|v| v.parse().ok());
            }
            s if !s.starts_with('-') => {
                // Could be a PID or username.
                if let Ok(pid) = s.parse::<u32>() {
                    opts.root_pid = Some(pid);
                } else {
                    // Treat as username — find their processes.
                    // We'll handle this by finding the user's login process.
                    if let Some(uid) = resolve_username(s) {
                        opts.root_pid = find_first_pid_for_uid(uid);
                        opts.show_uid = true;
                    } else {
                        eprintln!("pstree: user '{s}' not found");
                        process::exit(1);
                    }
                }
            }
            other => {
                eprintln!("pstree: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    opts
}

fn resolve_username(name: &str) -> Option<u32> {
    if let Some(content) = read_file("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3 && fields[0] == name {
                return fields[2].parse().ok();
            }
        }
    }
    None
}

fn find_first_pid_for_uid(uid: u32) -> Option<u32> {
    for pid in enumerate_pids() {
        if read_proc_uid(pid) == uid {
            return Some(pid);
        }
    }
    None
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let opts = parse_args();
    let procs = build_process_map();
    let children = build_children_map(&procs);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let root = opts.root_pid.unwrap_or(1);

    if procs.contains_key(&root) {
        render_tree(&mut out, root, &procs, &children, &opts, "", true, true);
    } else if procs.is_empty() {
        eprintln!("pstree: no processes found");
    } else {
        eprintln!("pstree: PID {root} not found");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_proc(pid: u32, ppid: u32, name: &str) -> ProcessInfo {
        ProcessInfo {
            pid,
            ppid,
            name: name.to_string(),
            uid: 0,
            threads: 1,
            state: 'S',
            username: "root".to_string(),
        }
    }

    #[test]
    fn test_build_children_map() {
        let mut procs = HashMap::new();
        procs.insert(1, make_proc(1, 0, "init"));
        procs.insert(2, make_proc(2, 1, "bash"));
        procs.insert(3, make_proc(3, 1, "sshd"));
        procs.insert(4, make_proc(4, 2, "vim"));

        let children = build_children_map(&procs);
        assert_eq!(children.get(&1).unwrap(), &[2, 3]);
        assert_eq!(children.get(&2).unwrap(), &[4]);
        assert!(children.get(&3).is_none() || children.get(&3).unwrap().is_empty());
    }

    #[test]
    fn test_format_process_basic() {
        let info = make_proc(42, 1, "bash");
        let opts = Options {
            show_pids: false, show_uid: false, show_threads: false,
            ascii: false, compact: true, root_pid: None, highlight_pid: None,
            show_kernel: false, long_format: false, sort_by_name: false,
            show_args: false, numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "bash");
    }

    #[test]
    fn test_format_process_with_pid() {
        let info = make_proc(42, 1, "bash");
        let opts = Options {
            show_pids: true, show_uid: false, show_threads: false,
            ascii: false, compact: true, root_pid: None, highlight_pid: None,
            show_kernel: false, long_format: false, sort_by_name: false,
            show_args: false, numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "bash(42)");
    }

    #[test]
    fn test_format_process_with_uid() {
        let info = ProcessInfo {
            pid: 42, ppid: 1, name: "bash".to_string(),
            uid: 1000, threads: 1, state: 'S',
            username: "alice".to_string(),
        };
        let opts = Options {
            show_pids: false, show_uid: true, show_threads: false,
            ascii: false, compact: true, root_pid: None, highlight_pid: None,
            show_kernel: false, long_format: false, sort_by_name: false,
            show_args: false, numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "bash[alice]");
    }

    #[test]
    fn test_format_process_with_numeric_uid() {
        let info = ProcessInfo {
            pid: 42, ppid: 1, name: "bash".to_string(),
            uid: 1000, threads: 1, state: 'S',
            username: "alice".to_string(),
        };
        let opts = Options {
            show_pids: false, show_uid: true, show_threads: false,
            ascii: false, compact: true, root_pid: None, highlight_pid: None,
            show_kernel: false, long_format: false, sort_by_name: false,
            show_args: false, numeric_uid: true,
        };
        assert_eq!(format_process(&info, &opts), "bash[1000]");
    }

    #[test]
    fn test_format_process_with_threads() {
        let mut info = make_proc(42, 1, "java");
        info.threads = 16;
        let opts = Options {
            show_pids: false, show_uid: false, show_threads: true,
            ascii: false, compact: true, root_pid: None, highlight_pid: None,
            show_kernel: false, long_format: false, sort_by_name: false,
            show_args: false, numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "java{16 threads}");
    }

    #[test]
    fn test_format_process_single_thread() {
        let info = make_proc(42, 1, "bash");
        let opts = Options {
            show_pids: false, show_uid: false, show_threads: true,
            ascii: false, compact: true, root_pid: None, highlight_pid: None,
            show_kernel: false, long_format: false, sort_by_name: false,
            show_args: false, numeric_uid: false,
        };
        // Single thread = no thread count shown.
        assert_eq!(format_process(&info, &opts), "bash");
    }

    #[test]
    fn test_format_process_all_options() {
        let mut info = ProcessInfo {
            pid: 42, ppid: 1, name: "httpd".to_string(),
            uid: 33, threads: 4, state: 'S',
            username: "www-data".to_string(),
        };
        info.threads = 4;
        let opts = Options {
            show_pids: true, show_uid: true, show_threads: true,
            ascii: false, compact: true, root_pid: None, highlight_pid: None,
            show_kernel: false, long_format: false, sort_by_name: false,
            show_args: false, numeric_uid: false,
        };
        assert_eq!(format_process(&info, &opts), "httpd(42)[www-data]{4 threads}");
    }

    #[test]
    fn test_tree_characters() {
        assert_eq!(TREE_BRANCH, "├── ");
        assert_eq!(TREE_LAST, "└── ");
        assert_eq!(TREE_PIPE, "│   ");
        assert_eq!(TREE_SPACE, "    ");
    }

    #[test]
    fn test_ascii_characters() {
        assert_eq!(ASCII_BRANCH, "|-- ");
        assert_eq!(ASCII_LAST, "`-- ");
        assert_eq!(ASCII_PIPE, "|   ");
        assert_eq!(ASCII_SPACE, "    ");
    }

    #[test]
    fn test_enumerate_pids() {
        let pids = enumerate_pids();
        // Should not panic. May be empty on non-Linux systems.
        for &pid in &pids {
            assert!(pid > 0);
        }
    }

    #[test]
    fn test_children_map_sorted() {
        let mut procs = HashMap::new();
        procs.insert(1, make_proc(1, 0, "init"));
        procs.insert(5, make_proc(5, 1, "e"));
        procs.insert(3, make_proc(3, 1, "c"));
        procs.insert(7, make_proc(7, 1, "g"));
        procs.insert(2, make_proc(2, 1, "b"));

        let children = build_children_map(&procs);
        let kids = children.get(&1).unwrap();
        // Should be sorted by PID.
        assert_eq!(kids, &[2, 3, 5, 7]);
    }

    #[test]
    fn test_process_info_clone() {
        let info = make_proc(1, 0, "init");
        let cloned = info.clone();
        assert_eq!(cloned.pid, 1);
        assert_eq!(cloned.name, "init");
    }

    #[test]
    fn test_uid_to_name_fallback() {
        // Non-existent UID should return the numeric string.
        let name = uid_to_name(99999);
        // Either resolves or falls back to "99999".
        assert!(!name.is_empty());
    }
}
