//! SlateOS namespace listing utility.
//!
//! Displays information about Linux namespaces from `/proc/<pid>/ns/`.
//!
//! Supports: mnt, uts, ipc, pid, net, user, cgroup, time namespaces.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

/// Namespace info aggregated from /proc.
#[derive(Clone, Debug)]
struct NsInfo {
    /// Namespace type (mnt, uts, ipc, pid, net, user, cgroup, time).
    ns_type: String,
    /// Namespace inode number (unique ID).
    ns_inode: u64,
    /// Number of processes in this namespace.
    nprocs: u32,
    /// PID of the first (lowest) process found.
    pid: u32,
    /// Owner UID.
    uid: u32,
    /// Username.
    user: String,
    /// Process command name.
    command: String,
}

struct Options {
    /// Filter by namespace type.
    type_filter: Option<String>,
    /// Show only namespaces for a specific PID.
    pid_filter: Option<u32>,
    /// JSON output.
    json: bool,
    /// Raw output (no alignment).
    raw: bool,
    /// No header.
    no_header: bool,
    /// Show specific columns.
    columns: Vec<String>,
    /// Show only user-owned namespaces. Wired through CLI parsing; the
    /// filter pass that consumes it is still TODO, so allow dead_code.
    #[allow(dead_code)]
    owned_only: bool,
}

// ============================================================================
// Namespace enumeration
// ============================================================================

const NS_TYPES: &[&str] = &["mnt", "uts", "ipc", "pid", "net", "user", "cgroup", "time"];

/// Map key: (namespace type, inode number).
type NsKey = (String, u64);
/// Map value: (process count, first pid, uid, user name, command).
type NsAgg = (u32, u32, u32, String, String);

fn enumerate_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Ok(pid) = name.parse::<u32>() {
                    pids.push(pid);
                }
        }
    }
    pids.sort_unstable();
    pids
}

fn read_ns_inode(pid: u32, ns_type: &str) -> Option<u64> {
    let path = format!("/proc/{pid}/ns/{ns_type}");
    let link = fs::read_link(&path).ok()?;
    let link_str = link.to_str()?;
    // Link target format: "type:[inode]"
    let bracket_start = link_str.find('[')?;
    let bracket_end = link_str.find(']')?;
    link_str[bracket_start + 1..bracket_end].parse().ok()
}

fn read_proc_comm(pid: u32) -> String {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "?".to_string())
}

fn read_proc_uid(pid: u32) -> u32 {
    if let Ok(content) = fs::read_to_string(format!("/proc/{pid}/status")) {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("Uid:") {
                return val.split_whitespace().next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
            }
        }
    }
    0
}

fn uid_to_name(uid: u32) -> String {
    if let Ok(content) = fs::read_to_string("/etc/passwd") {
        for line in content.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 3
                && let Ok(file_uid) = fields[2].parse::<u32>()
                    && file_uid == uid {
                        return fields[0].to_string();
                    }
        }
    }
    uid.to_string()
}

/// Collect all namespaces across all processes.
fn collect_namespaces(opts: &Options) -> Vec<NsInfo> {
    let pids = if let Some(pid) = opts.pid_filter {
        vec![pid]
    } else {
        enumerate_pids()
    };

    let types: Vec<&str> = if let Some(ref t) = opts.type_filter {
        vec![t.as_str()]
    } else {
        NS_TYPES.to_vec()
    };

    // Map: (ns_type, inode) -> (count, first_pid, uid, user, command)
    let mut ns_map: HashMap<NsKey, NsAgg> = HashMap::new();

    for &pid in &pids {
        for &ns_type in &types {
            if let Some(inode) = read_ns_inode(pid, ns_type) {
                let key = (ns_type.to_string(), inode);
                let entry = ns_map.entry(key).or_insert_with(|| {
                    let uid = read_proc_uid(pid);
                    let user = uid_to_name(uid);
                    let command = read_proc_comm(pid);
                    (0, pid, uid, user, command)
                });
                entry.0 += 1;
                // Keep the lowest PID.
                if pid < entry.1 {
                    entry.1 = pid;
                    entry.4 = read_proc_comm(pid);
                }
            }
        }
    }

    let mut result: Vec<NsInfo> = ns_map.into_iter().map(|((ns_type, ns_inode), (nprocs, pid, uid, user, command))| {
        NsInfo { ns_type, ns_inode, nprocs, pid, uid, user, command }
    }).collect();

    // Sort by namespace type, then inode.
    result.sort_by(|a, b| {
        a.ns_type.cmp(&b.ns_type).then(a.ns_inode.cmp(&b.ns_inode))
    });

    result
}

// ============================================================================
// Output
// ============================================================================

fn default_columns() -> Vec<String> {
    vec!["NS".into(), "TYPE".into(), "NPROCS".into(), "PID".into(), "USER".into(), "COMMAND".into()]
}

fn column_value(ns: &NsInfo, col: &str) -> String {
    match col.to_uppercase().as_str() {
        "NS" | "INODE" => ns.ns_inode.to_string(),
        "TYPE" => ns.ns_type.clone(),
        "NPROCS" => ns.nprocs.to_string(),
        "PID" => ns.pid.to_string(),
        "USER" => ns.user.clone(),
        "UID" => ns.uid.to_string(),
        "COMMAND" | "CMD" => ns.command.clone(),
        _ => String::new(),
    }
}

fn print_table(out: &mut io::StdoutLock<'_>, entries: &[NsInfo], opts: &Options) {
    let cols = if opts.columns.is_empty() { default_columns() } else { opts.columns.clone() };
    let mut widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();

    for ns in entries {
        for (i, col) in cols.iter().enumerate() {
            let val = column_value(ns, col);
            if val.len() > widths[i] { widths[i] = val.len(); }
        }
    }

    if !opts.no_header {
        for (i, col) in cols.iter().enumerate() {
            if i > 0 { let _ = write!(out, " "); }
            let _ = write!(out, "{:>width$}", col, width = widths[i]);
        }
        let _ = writeln!(out);
    }

    for ns in entries {
        for (i, col) in cols.iter().enumerate() {
            if i > 0 { let _ = write!(out, " "); }
            let val = column_value(ns, col);
            let _ = write!(out, "{:>width$}", val, width = widths[i]);
        }
        let _ = writeln!(out);
    }
}

fn print_json(out: &mut io::StdoutLock<'_>, entries: &[NsInfo]) {
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"namespaces\": [");
    for (i, ns) in entries.iter().enumerate() {
        let comma = if i + 1 < entries.len() { "," } else { "" };
        let _ = writeln!(out, "    {{\"ns\": {}, \"type\": \"{}\", \"nprocs\": {}, \"pid\": {}, \"user\": \"{}\", \"command\": \"{}\"}}{comma}",
            ns.ns_inode, ns.ns_type, ns.nprocs, ns.pid, ns.user, ns.command);
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
}

fn print_raw(out: &mut io::StdoutLock<'_>, entries: &[NsInfo], opts: &Options) {
    let cols = if opts.columns.is_empty() { default_columns() } else { opts.columns.clone() };
    if !opts.no_header {
        let _ = writeln!(out, "{}", cols.join(" "));
    }
    for ns in entries {
        let vals: Vec<String> = cols.iter().map(|c| column_value(ns, c)).collect();
        let _ = writeln!(out, "{}", vals.join(" "));
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut opts = Options {
        type_filter: None,
        pid_filter: None,
        json: false,
        raw: false,
        no_header: false,
        columns: Vec::new(),
        owned_only: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: lsns [options]");
                println!();
                println!("List information about namespaces.");
                println!();
                println!("Options:");
                println!("  -t, --type TYPE    Show only namespaces of TYPE (mnt,uts,ipc,pid,net,user,cgroup,time)");
                println!("  -p, --task PID     Show namespaces for PID only");
                println!("  -J, --json         JSON output");
                println!("  -r, --raw          Raw output");
                println!("  -n, --noheadings   No headers");
                println!("  -o, --output COLS  Specify columns (NS,TYPE,NPROCS,PID,USER,COMMAND)");
                println!("  -h, --help         Show this help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("lsns {VERSION}");
                process::exit(0);
            }
            "-t" | "--type" => {
                i += 1;
                if i < args.len() { opts.type_filter = Some(args[i].clone()); }
            }
            "-p" | "--task" => {
                i += 1;
                if i < args.len() {
                    opts.pid_filter = Some(args[i].parse().unwrap_or_else(|_| {
                        eprintln!("lsns: invalid PID: {}", args[i]);
                        process::exit(1);
                    }));
                }
            }
            "-J" | "--json" => opts.json = true,
            "-r" | "--raw" => opts.raw = true,
            "-n" | "--noheadings" => opts.no_header = true,
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    opts.columns = args[i].split(',').map(|s| s.trim().to_uppercase()).collect();
                }
            }
            other => {
                eprintln!("lsns: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    let namespaces = collect_namespaces(&opts);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.json {
        print_json(&mut out, &namespaces);
    } else if opts.raw {
        print_raw(&mut out, &namespaces, &opts);
    } else {
        print_table(&mut out, &namespaces, &opts);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ns(ns_type: &str, inode: u64, nprocs: u32, pid: u32) -> NsInfo {
        NsInfo {
            ns_type: ns_type.to_string(),
            ns_inode: inode,
            nprocs,
            pid,
            uid: 0,
            user: "root".to_string(),
            command: "init".to_string(),
        }
    }

    #[test]
    fn test_column_value_ns() {
        let ns = make_ns("pid", 12345, 5, 1);
        assert_eq!(column_value(&ns, "NS"), "12345");
        assert_eq!(column_value(&ns, "TYPE"), "pid");
        assert_eq!(column_value(&ns, "NPROCS"), "5");
        assert_eq!(column_value(&ns, "PID"), "1");
        assert_eq!(column_value(&ns, "USER"), "root");
        assert_eq!(column_value(&ns, "COMMAND"), "init");
    }

    #[test]
    fn test_column_value_case_insensitive() {
        let ns = make_ns("net", 99, 2, 42);
        assert_eq!(column_value(&ns, "type"), "net");
        assert_eq!(column_value(&ns, "Type"), "net");
        assert_eq!(column_value(&ns, "nprocs"), "2");
    }

    #[test]
    fn test_column_value_unknown() {
        let ns = make_ns("mnt", 1, 1, 1);
        assert_eq!(column_value(&ns, "UNKNOWN"), "");
    }

    #[test]
    fn test_default_columns() {
        let cols = default_columns();
        assert_eq!(cols.len(), 6);
        assert_eq!(cols[0], "NS");
        assert_eq!(cols[1], "TYPE");
    }

    #[test]
    fn test_ns_types_list() {
        assert_eq!(NS_TYPES.len(), 8);
        assert!(NS_TYPES.contains(&"mnt"));
        assert!(NS_TYPES.contains(&"pid"));
        assert!(NS_TYPES.contains(&"net"));
        assert!(NS_TYPES.contains(&"user"));
        assert!(NS_TYPES.contains(&"cgroup"));
        assert!(NS_TYPES.contains(&"time"));
    }

    #[test]
    fn test_ns_info_clone() {
        let ns = make_ns("uts", 42, 3, 7);
        let cloned = ns.clone();
        assert_eq!(cloned.ns_type, "uts");
        assert_eq!(cloned.ns_inode, 42);
        assert_eq!(cloned.nprocs, 3);
    }

    #[test]
    fn test_enumerate_pids_no_panic() {
        let pids = enumerate_pids();
        // Shouldn't panic, may be empty on non-Linux.
        let _ = pids.len();
    }

    #[test]
    fn test_uid_to_name_fallback() {
        let name = uid_to_name(99999);
        assert!(!name.is_empty());
    }

    #[test]
    fn test_sort_order() {
        let mut nss = [
            make_ns("pid", 200, 1, 1),
            make_ns("mnt", 100, 1, 1),
            make_ns("mnt", 50, 1, 1),
            make_ns("pid", 150, 1, 1),
        ];
        nss.sort_by(|a, b| a.ns_type.cmp(&b.ns_type).then(a.ns_inode.cmp(&b.ns_inode)));
        assert_eq!(nss[0].ns_type, "mnt");
        assert_eq!(nss[0].ns_inode, 50);
        assert_eq!(nss[1].ns_inode, 100);
        assert_eq!(nss[2].ns_type, "pid");
    }
}
