//! Slate OS Process Listing Utility
//!
//! Lists running processes by reading the `/proc` virtual filesystem.
//! Supports multiple output formats (default, full, long, user-oriented),
//! filtering, sorting, tree view, and JSON output.
//!
//! # Usage
//!
//! ```text
//! ps                     Default: PID, TTY, STATE, TIME, CMD
//! ps -e / -A             Show all processes (same as default)
//! ps -f                  Full format (UID, PID, PPID, C, STIME, TTY, TIME, CMD)
//! ps -l                  Long format (F, S, UID, PID, PPID, C, PRI, NI, SZ, RSS, ...)
//! ps -u [user]           User-oriented format
//! ps -p <pid>            Show specific PID only
//! ps -o <cols>           Custom columns (comma-separated)
//! ps --sort <field>      Sort by field (pid, name, cpu, mem, time)
//! ps --json              JSON output
//! ps --no-header         Suppress column headers
//! ps -t                  Tree view (parent-child relationships)
//! ```

use std::env;
use std::process;

// ============================================================================
// Constants
// ============================================================================

// `PAGE_SIZE_KB` used to be declared here, a second private copy of a fact
// about the kernel -- `userspace/htop` had the other. It is
// `procinfo::PAGE_SIZE_KIB` now, and this program no longer converts pages
// to KiB itself.

/// Scheduler ticks per second. `procinfo::TICKS_PER_SEC` is the same
/// number; this alias keeps the arithmetic below readable.
const TICKS_PER_SEC: u64 = procinfo::TICKS_PER_SEC;

// ANSI colour codes for state display.
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

// ============================================================================
// Data structures
// ============================================================================

/// Per-process information scraped from /proc/<pid>/.
#[derive(Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    state: char,
    state_long: String,
    ppid: u32,
    /// Process group.
    pgrp: u32,
    /// Session ID.
    session: u32,
    /// TTY number.
    tty: i32,
    /// User time in ticks.
    utime: u64,
    /// System time in ticks.
    stime: u64,
    /// Priority.
    priority: i32,
    /// Nice value.
    nice: i32,
    /// Number of threads.
    threads: u32,
    /// Process start time (in ticks since boot).
    starttime: u64,
    /// Virtual memory size in bytes.
    vsize: u64,
    /// Resident set size in pages.
    rss: u64,
    /// Owner UID.
    uid: u32,
    /// Owner GID.
    gid: u32,
    /// Groups list.
    groups: Vec<u32>,
    /// VmSize from /proc/<pid>/status (in kB), if available.
    /// Kept for future per-process detail views.
    #[allow(dead_code)]
    vm_size_kb: u64,
    /// VmRSS from /proc/<pid>/status (in kB), if available.
    /// Kept for future per-process detail views.
    #[allow(dead_code)]
    vm_rss_kb: u64,
}

/// Which output format the user requested.
#[derive(Clone, Copy, PartialEq)]
enum OutputFormat {
    Default,
    Full,
    Long,
    User,
    Custom,
}

/// Which column to display in custom (-o) output.
#[derive(Clone, Copy, PartialEq)]
enum Column {
    Pid,
    Ppid,
    Uid,
    Gid,
    State,
    Name,
    Nice,
    Priority,
    Threads,
    Vsize,
    Rss,
    Time,
    Cpu,
    Tty,
    Stime,
    Groups,
    Session,
    Pgrp,
}

#[derive(Clone, Copy, PartialEq)]
enum SortField {
    Pid,
    Name,
    Cpu,
    Mem,
    Time,
}

struct Config {
    format: OutputFormat,
    filter_pid: Option<u32>,
    filter_uid: Option<u32>,
    custom_columns: Vec<Column>,
    sort_field: SortField,
    sort_reverse: bool,
    json_output: bool,
    no_header: bool,
    tree_view: bool,
    show_all: bool,
}

// ============================================================================
// /proc readers
// ============================================================================

/// System uptime in ticks, through [`procinfo`].
///
/// `procinfo` gives a `Duration`; the conversion to ticks is here because the
/// tick is what `/proc/<pid>/stat`'s `starttime` is measured in, and pairing
/// the two is this program's business.
fn system_uptime_ticks() -> u64 {
    procinfo::ProcFs::new()
        .uptime()
        .ok()
        .flatten()
        .map_or(0, |u| u.up.as_secs().saturating_mul(TICKS_PER_SEC))
}

/// Total physical memory in KiB, through [`procinfo`].
///
/// The reader this replaces reassembled the line it had just taken apart --
/// `strip_prefix("MemTotal:")` and then `format!("MemTotal:{rest}")` so that a
/// helper could split it again -- which is the shape code takes when the
/// parsing has nowhere to live.
fn total_memory_kb() -> u64 {
    procinfo::ProcFs::new()
        .memory()
        .ok()
        .flatten()
        .and_then(|m| m.total_kib)
        .unwrap_or(0)
}

/// Parse a short state character into a longer description.
fn state_description(c: char) -> String {
    match c {
        'R' => "running".to_string(),
        'S' => "sleeping".to_string(),
        'D' => "disk sleep".to_string(),
        'Z' => "zombie".to_string(),
        'T' => "stopped".to_string(),
        't' => "tracing".to_string(),
        'X' => "dead".to_string(),
        'I' => "idle".to_string(),
        _ => format!("unknown({c})"),
    }
}

/// Coloured state character for terminal display.
fn coloured_state(c: char) -> String {
    match c {
        'R' => format!("{GREEN}{c}{RESET}"),
        'S' | 'D' | 'I' => format!("{YELLOW}{c}{RESET}"),
        'Z' | 'X' => format!("{RED}{c}{RESET}"),
        _ => format!("{c}"),
    }
}

/// Read information about a single process, through [`procinfo`].
///
/// The parsing this used to do itself now lives in `procinfo`, shared with
/// `userspace/htop` and `apps/procexplorer`. Two things it got wrong on its own
/// and no longer can:
///
/// * a name that is not UTF-8 dropped the whole process, because `read_file`
///   goes through `read_to_string`. `comm` is bytes now.
/// * `VmSize`/`VmRSS` absent -- which is every kernel thread, since a kernel
///   thread has no address space -- was indistinguishable from zero.
fn read_process(pid: u32) -> Option<ProcessInfo> {
    let procfs = procinfo::ProcFs::new();
    let stat = procfs.process_stat(u64::from(pid)).ok()??;
    let status = procfs
        .process_status(u64::from(pid))
        .ok()
        .flatten()
        .unwrap_or_default();

    let name = procinfo::display_bytes(&stat.comm);
    let state = char::from(stat.state);
    let state_long = state_description(state);

    Some(ProcessInfo {
        pid,
        name,
        state,
        state_long,
        ppid: u32::try_from(stat.ppid).unwrap_or(0),
        pgrp: u32::try_from(stat.pgrp).unwrap_or(0),
        session: u32::try_from(stat.session).unwrap_or(0),
        tty: i32::try_from(stat.tty_nr).unwrap_or(0),
        utime: stat.utime_ticks,
        stime: stat.stime_ticks,
        priority: i32::try_from(stat.priority).unwrap_or(0),
        nice: i32::try_from(stat.nice).unwrap_or(0),
        threads: u32::try_from(stat.num_threads).unwrap_or(1),
        starttime: stat.starttime_ticks,
        vsize: stat.vsize_bytes,
        rss: stat.rss_pages,
        uid: status.uid.unwrap_or(0),
        gid: status.gid.unwrap_or(0),
        groups: status.groups,
        vm_size_kb: status.vm_size_kib.unwrap_or(0),
        vm_rss_kb: status.vm_rss_kib.unwrap_or(0),
    })
}

fn read_all_processes() -> Vec<ProcessInfo> {
    let Ok(pids) = procinfo::ProcFs::new().process_ids() else {
        return Vec::new();
    };
    pids.into_iter()
        .filter_map(|pid| u32::try_from(pid).ok())
        .filter_map(read_process)
        .collect()
}

// ============================================================================
// Formatting helpers
// ============================================================================

/// Format a byte count (given in kB) as a human-readable size string.
fn format_size(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1} GiB", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{:.1} MiB", kb as f64 / 1024.0)
    } else {
        format!("{kb} KiB")
    }
}

/// Format tick count as HH:MM:SS.
fn format_time(ticks: u64) -> String {
    let total_secs = ticks / TICKS_PER_SEC;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    format!("{hours:02}:{mins:02}:{secs:02}")
}

/// Format TTY number as a short name. 0 means no controlling terminal.
fn format_tty(tty: i32) -> String {
    if tty <= 0 {
        "?".to_string()
    } else {
        format!("tty{tty}")
    }
}

/// Compute a rough CPU% for a process given the system uptime in ticks.
/// This is cumulative CPU usage over the process lifetime, not instantaneous.
fn cpu_percent(proc_info: &ProcessInfo, uptime_ticks: u64) -> f64 {
    let total_time = proc_info.utime.saturating_add(proc_info.stime);
    let elapsed = uptime_ticks.saturating_sub(proc_info.starttime);
    if elapsed == 0 {
        return 0.0;
    }
    (total_time as f64 / elapsed as f64) * 100.0
}

/// Compute memory% for a process.
fn mem_percent(proc_info: &ProcessInfo, mem_total_kb: u64) -> f64 {
    if mem_total_kb == 0 {
        return 0.0;
    }
    let rss_kb = proc_info.rss.saturating_mul(procinfo::PAGE_SIZE_KIB);
    (rss_kb as f64 / mem_total_kb as f64) * 100.0
}

// ============================================================================
// Sorting
// ============================================================================

fn sort_processes(
    procs: &mut [ProcessInfo],
    field: SortField,
    reverse: bool,
    uptime_ticks: u64,
    mem_total_kb: u64,
) {
    procs.sort_by(|a, b| {
        let cmp = match field {
            SortField::Pid => a.pid.cmp(&b.pid),
            SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortField::Cpu => {
                let ca = cpu_percent(a, uptime_ticks);
                let cb = cpu_percent(b, uptime_ticks);
                ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
            }
            SortField::Mem => {
                let ma = mem_percent(a, mem_total_kb);
                let mb = mem_percent(b, mem_total_kb);
                ma.partial_cmp(&mb).unwrap_or(std::cmp::Ordering::Equal)
            }
            SortField::Time => {
                let ta = a.utime.saturating_add(a.stime);
                let tb = b.utime.saturating_add(b.stime);
                ta.cmp(&tb)
            }
        };

        if reverse { cmp.reverse() } else { cmp }
    });
}

// ============================================================================
// Output: Default format
// ============================================================================

fn print_default(procs: &[ProcessInfo], no_header: bool) {
    if !no_header {
        println!(
            "{:>7}  {:<8}  {:<5}  {:>10}  CMD",
            "PID", "TTY", "STATE", "TIME"
        );
    }
    for p in procs {
        println!(
            "{:>7}  {:<8}  {:<5}  {:>10}  {}",
            p.pid,
            format_tty(p.tty),
            coloured_state(p.state),
            format_time(p.utime.saturating_add(p.stime)),
            p.name,
        );
    }
}

// ============================================================================
// Output: Full format (-f)
// ============================================================================

fn print_full(procs: &[ProcessInfo], no_header: bool, uptime_ticks: u64) {
    if !no_header {
        println!(
            "{:>7}  {:>7}  {:>7}  {:>3}  {:>10}  {:<8}  {:>10}  CMD",
            "UID", "PID", "PPID", "C", "STIME", "TTY", "TIME"
        );
    }
    for p in procs {
        let c = cpu_percent(p, uptime_ticks);
        println!(
            "{:>7}  {:>7}  {:>7}  {:>3.0}  {:>10}  {:<8}  {:>10}  {}",
            p.uid,
            p.pid,
            p.ppid,
            c,
            format_start_time(p.starttime),
            format_tty(p.tty),
            format_time(p.utime.saturating_add(p.stime)),
            p.name,
        );
    }
}

/// Format a process start time (in ticks since boot) as a clock time string.
/// Since we don't have wall-clock info, show relative time since boot.
fn format_start_time(starttime_ticks: u64) -> String {
    let secs = starttime_ticks / TICKS_PER_SEC;
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    format!("{hours:02}:{mins:02}")
}

// ============================================================================
// Output: Long format (-l)
// ============================================================================

fn print_long(procs: &[ProcessInfo], no_header: bool, uptime_ticks: u64) {
    if !no_header {
        println!(
            "{:>2}  {:<1}  {:>7}  {:>7}  {:>7}  {:>3}  {:>4}  {:>4}  {:>9}  {:>9}  {:<8}  {:<8}  {:>10}  CMD",
            "F", "S", "UID", "PID", "PPID", "C", "PRI", "NI", "SZ", "RSS", "WCHAN", "TTY", "TIME"
        );
    }
    for p in procs {
        let c = cpu_percent(p, uptime_ticks);
        let sz_kb = p.vsize / 1024;
        let rss_kb = p.rss.saturating_mul(procinfo::PAGE_SIZE_KIB);
        println!(
            "{:>2}  {}  {:>7}  {:>7}  {:>7}  {:>3.0}  {:>4}  {:>4}  {:>9}  {:>9}  {:<8}  {:<8}  {:>10}  {}",
            0, // flags (not tracked yet)
            coloured_state(p.state),
            p.uid,
            p.pid,
            p.ppid,
            c,
            p.priority,
            p.nice,
            format_size(sz_kb),
            format_size(rss_kb),
            "-", // wchan not available yet
            format_tty(p.tty),
            format_time(p.utime.saturating_add(p.stime)),
            p.name,
        );
    }
}

// ============================================================================
// Output: User-oriented format (-u)
// ============================================================================

fn print_user(procs: &[ProcessInfo], no_header: bool, uptime_ticks: u64, mem_total_kb: u64) {
    if !no_header {
        println!(
            "{:>7}  {:>7}  {:>5}  {:>5}  {:>9}  {:>9}  {:<8}  {:<5}  {:>10}  COMMAND",
            "UID", "PID", "%CPU", "%MEM", "VSZ", "RSS", "TTY", "STAT", "TIME"
        );
    }
    for p in procs {
        let cpu = cpu_percent(p, uptime_ticks);
        let mem = mem_percent(p, mem_total_kb);
        let sz_kb = p.vsize / 1024;
        let rss_kb = p.rss.saturating_mul(procinfo::PAGE_SIZE_KIB);
        println!(
            "{:>7}  {:>7}  {:>5.1}  {:>5.1}  {:>9}  {:>9}  {:<8}  {:<5}  {:>10}  {}",
            p.uid,
            p.pid,
            cpu,
            mem,
            format_size(sz_kb),
            format_size(rss_kb),
            format_tty(p.tty),
            coloured_state(p.state),
            format_time(p.utime.saturating_add(p.stime)),
            p.name,
        );
    }
}

// ============================================================================
// Output: Custom columns (-o)
// ============================================================================

fn column_header(col: Column) -> &'static str {
    match col {
        Column::Pid => "PID",
        Column::Ppid => "PPID",
        Column::Uid => "UID",
        Column::Gid => "GID",
        Column::State => "STATE",
        Column::Name => "CMD",
        Column::Nice => "NI",
        Column::Priority => "PRI",
        Column::Threads => "THR",
        Column::Vsize => "VSZ",
        Column::Rss => "RSS",
        Column::Time => "TIME",
        Column::Cpu => "%CPU",
        Column::Tty => "TTY",
        Column::Stime => "STIME",
        Column::Groups => "GROUPS",
        Column::Session => "SID",
        Column::Pgrp => "PGRP",
    }
}

fn column_width(col: Column) -> usize {
    match col {
        Column::Pid | Column::Ppid | Column::Uid | Column::Gid | Column::Session | Column::Pgrp => {
            8
        }
        Column::State => 6,
        Column::Name => 20,
        Column::Nice | Column::Priority => 5,
        Column::Threads => 5,
        Column::Vsize | Column::Rss => 10,
        Column::Time | Column::Stime => 11,
        Column::Cpu => 6,
        Column::Tty => 9,
        Column::Groups => 20,
    }
}

fn format_column_value(
    col: Column,
    p: &ProcessInfo,
    uptime_ticks: u64,
    _mem_total_kb: u64,
) -> String {
    match col {
        Column::Pid => format!("{}", p.pid),
        Column::Ppid => format!("{}", p.ppid),
        Column::Uid => format!("{}", p.uid),
        Column::Gid => format!("{}", p.gid),
        Column::State => coloured_state(p.state),
        Column::Name => p.name.clone(),
        Column::Nice => format!("{}", p.nice),
        Column::Priority => format!("{}", p.priority),
        Column::Threads => format!("{}", p.threads),
        Column::Vsize => format_size(p.vsize / 1024),
        Column::Rss => format_size(p.rss.saturating_mul(procinfo::PAGE_SIZE_KIB)),
        Column::Time => format_time(p.utime.saturating_add(p.stime)),
        Column::Cpu => format!("{:.1}", cpu_percent(p, uptime_ticks)),
        Column::Tty => format_tty(p.tty),
        Column::Stime => format_start_time(p.starttime),
        Column::Groups => {
            let g: Vec<String> = p.groups.iter().map(|g| g.to_string()).collect();
            g.join(",")
        }
        Column::Session => format!("{}", p.session),
        Column::Pgrp => format!("{}", p.pgrp),
        // Compute mem% using available data.
        // (not a separate column variant, so unreachable here, but kept for safety)
    }
}

fn print_custom(
    procs: &[ProcessInfo],
    columns: &[Column],
    no_header: bool,
    uptime_ticks: u64,
    mem_total_kb: u64,
) {
    if !no_header {
        let mut header = String::new();
        for (i, col) in columns.iter().enumerate() {
            if i > 0 {
                header.push_str("  ");
            }
            let w = column_width(*col);
            let h = column_header(*col);
            // Right-align numeric columns, left-align text.
            match col {
                Column::Name | Column::State | Column::Tty | Column::Groups => {
                    header.push_str(&format!("{h:<w$}"));
                }
                _ => {
                    header.push_str(&format!("{h:>w$}"));
                }
            }
        }
        println!("{header}");
    }

    for p in procs {
        let mut line = String::new();
        for (i, col) in columns.iter().enumerate() {
            if i > 0 {
                line.push_str("  ");
            }
            let w = column_width(*col);
            let val = format_column_value(*col, p, uptime_ticks, mem_total_kb);
            match col {
                Column::Name | Column::State | Column::Tty | Column::Groups => {
                    line.push_str(&format!("{val:<w$}"));
                }
                _ => {
                    line.push_str(&format!("{val:>w$}"));
                }
            }
        }
        println!("{line}");
    }
}

// ============================================================================
// Output: Tree view (-t)
// ============================================================================

/// Build and print a process tree showing parent-child relationships.
fn print_tree(procs: &[ProcessInfo], no_header: bool) {
    if !no_header {
        println!("{:>7}  {:<5}  {:>10}  CMD", "PID", "STATE", "TIME");
    }

    // Build a children map: ppid -> list of indices into procs.
    let mut children: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
    for (i, p) in procs.iter().enumerate() {
        children.entry(p.ppid).or_default().push(i);
    }

    // Find root processes (ppid == 0 or ppid not in our pid set).
    let pid_set: std::collections::HashSet<u32> = procs.iter().map(|p| p.pid).collect();
    let mut roots: Vec<usize> = Vec::new();
    for (i, p) in procs.iter().enumerate() {
        if p.ppid == 0 || !pid_set.contains(&p.ppid) {
            roots.push(i);
        }
    }
    roots.sort_by_key(|&i| procs[i].pid);

    // Recursive tree printer.
    fn print_subtree(
        procs: &[ProcessInfo],
        children: &std::collections::HashMap<u32, Vec<usize>>,
        idx: usize,
        prefix: &str,
        is_last: bool,
    ) {
        let p = &procs[idx];
        let connector = if prefix.is_empty() {
            "".to_string()
        } else if is_last {
            format!("{prefix}`-- ")
        } else {
            format!("{prefix}|-- ")
        };

        println!(
            "{:>7}  {:<5}  {:>10}  {connector}{}",
            p.pid,
            coloured_state(p.state),
            format_time(p.utime.saturating_add(p.stime)),
            p.name,
        );

        let child_prefix = if prefix.is_empty() {
            String::new()
        } else if is_last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}|   ")
        };

        if let Some(child_indices) = children.get(&p.pid) {
            let mut sorted_children = child_indices.clone();
            sorted_children.sort_by_key(|&i| procs[i].pid);
            let count = sorted_children.len();
            for (ci, &child_idx) in sorted_children.iter().enumerate() {
                let last = ci + 1 == count;
                let next_prefix = if prefix.is_empty() {
                    "    ".to_string()
                } else {
                    child_prefix.clone()
                };
                print_subtree(procs, children, child_idx, &next_prefix, last);
            }
        }
    }

    for (ri, &root_idx) in roots.iter().enumerate() {
        let is_last = ri + 1 == roots.len();
        print_subtree(procs, &children, root_idx, "", is_last);
    }
}

// ============================================================================
// Output: JSON (--json)
// ============================================================================

fn print_json(procs: &[ProcessInfo], uptime_ticks: u64, mem_total_kb: u64) {
    println!("[");
    for (i, p) in procs.iter().enumerate() {
        let cpu = cpu_percent(p, uptime_ticks);
        let mem = mem_percent(p, mem_total_kb);
        let rss_kb = p.rss.saturating_mul(procinfo::PAGE_SIZE_KIB);
        let vsize_kb = p.vsize / 1024;
        let total_time = p.utime.saturating_add(p.stime);

        // Manual JSON construction to avoid needing serde.
        let groups_str: Vec<String> = p.groups.iter().map(|g| g.to_string()).collect();
        let groups_json = groups_str.join(", ");

        // Escape the process name for JSON (handle quotes and backslashes).
        let escaped_name = p
            .name
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        let escaped_state = p.state_long.replace('\\', "\\\\").replace('"', "\\\"");

        let comma = if i + 1 < procs.len() { "," } else { "" };

        println!("  {{");
        println!("    \"pid\": {}", p.pid);
        println!("    ,\"ppid\": {}", p.ppid);
        println!("    ,\"name\": \"{escaped_name}\"");
        println!("    ,\"state\": \"{escaped_state}\"");
        println!("    ,\"state_code\": \"{}\"", p.state);
        println!("    ,\"uid\": {}", p.uid);
        println!("    ,\"gid\": {}", p.gid);
        println!("    ,\"groups\": [{groups_json}]");
        println!("    ,\"tty\": {}", p.tty);
        println!("    ,\"priority\": {}", p.priority);
        println!("    ,\"nice\": {}", p.nice);
        println!("    ,\"threads\": {}", p.threads);
        println!("    ,\"vsize_kb\": {vsize_kb}");
        println!("    ,\"rss_kb\": {rss_kb}");
        println!("    ,\"cpu_percent\": {cpu:.2}");
        println!("    ,\"mem_percent\": {mem:.2}");
        println!("    ,\"time_ticks\": {total_time}");
        println!("    ,\"time_formatted\": \"{}\"", format_time(total_time));
        println!("    ,\"session\": {}", p.session);
        println!("    ,\"pgrp\": {}", p.pgrp);
        println!("  }}{comma}");
    }
    println!("]");
}

// ============================================================================
// CLI parsing
// ============================================================================

fn parse_column(name: &str) -> Option<Column> {
    match name.to_lowercase().as_str() {
        "pid" => Some(Column::Pid),
        "ppid" => Some(Column::Ppid),
        "uid" => Some(Column::Uid),
        "gid" => Some(Column::Gid),
        "state" | "stat" | "s" => Some(Column::State),
        "name" | "cmd" | "command" | "comm" => Some(Column::Name),
        "nice" | "ni" => Some(Column::Nice),
        "pri" | "priority" => Some(Column::Priority),
        "threads" | "thr" | "nlwp" => Some(Column::Threads),
        "vsize" | "vsz" | "virt" => Some(Column::Vsize),
        "rss" | "res" => Some(Column::Rss),
        "time" | "cputime" => Some(Column::Time),
        "cpu" | "%cpu" | "pcpu" => Some(Column::Cpu),
        "tty" | "tt" => Some(Column::Tty),
        "stime" | "start" | "lstart" => Some(Column::Stime),
        "groups" | "group" => Some(Column::Groups),
        "sid" | "session" | "sess" => Some(Column::Session),
        "pgrp" | "pgid" => Some(Column::Pgrp),
        _ => None,
    }
}

fn parse_sort_field(name: &str) -> Option<SortField> {
    match name.to_lowercase().as_str() {
        "pid" => Some(SortField::Pid),
        "name" | "cmd" | "command" => Some(SortField::Name),
        "cpu" | "%cpu" => Some(SortField::Cpu),
        "mem" | "%mem" | "memory" | "rss" => Some(SortField::Mem),
        "time" | "cputime" => Some(SortField::Time),
        _ => None,
    }
}

fn print_usage() {
    println!("Slate OS Process Listing Utility v0.1.0");
    println!();
    println!("USAGE:");
    println!("  ps [options]");
    println!();
    println!("OPTIONS:");
    println!("  -e, -A             Show all processes");
    println!("  -f                 Full format output");
    println!("  -l                 Long format output");
    println!("  -u [user]          User-oriented format (optional UID filter)");
    println!("  -p <pid>           Show only the specified PID");
    println!("  -o <columns>       Custom columns (comma-separated)");
    println!("  -t                 Tree view (parent-child hierarchy)");
    println!("  --sort <field>     Sort by: pid, name, cpu, mem, time");
    println!("  --reverse          Reverse sort order");
    println!("  --json             JSON output");
    println!("  --no-header        Suppress column headers");
    println!("  --help, -h         Show this help");
    println!();
    println!("CUSTOM COLUMNS (-o):");
    println!("  pid, ppid, uid, gid, state, name, nice, priority, threads,");
    println!("  vsize, rss, time, cpu, tty, stime, groups, session, pgrp");
    println!();
    println!("EXAMPLES:");
    println!("  ps -ef              All processes, full format");
    println!("  ps -l               Long format");
    println!("  ps -p 1             Show only PID 1");
    println!("  ps -o pid,ppid,name,cpu,rss  Custom columns");
    println!("  ps --sort cpu       Sort by CPU usage");
    println!("  ps -t               Tree view");
    println!("  ps --json           Machine-readable JSON output");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        format: OutputFormat::Default,
        filter_pid: None,
        filter_uid: None,
        custom_columns: Vec::new(),
        sort_field: SortField::Pid,
        sort_reverse: false,
        json_output: false,
        no_header: false,
        tree_view: false,
        show_all: false,
    };

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-e" | "-A" => {
                config.show_all = true;
                i += 1;
            }
            "-f" => {
                config.format = OutputFormat::Full;
                i += 1;
            }
            "-l" => {
                config.format = OutputFormat::Long;
                i += 1;
            }
            "-u" => {
                config.format = OutputFormat::User;
                // Optional UID argument: if the next arg is a number, treat
                // it as a UID filter.
                if i + 1 < args.len()
                    && let Ok(uid) = args[i + 1].parse::<u32>()
                {
                    config.filter_uid = Some(uid);
                    i += 2;
                    continue;
                }
                i += 1;
            }
            "-p" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -p requires a PID value");
                    process::exit(1);
                }
                match args[i + 1].parse::<u32>() {
                    Ok(pid) => config.filter_pid = Some(pid),
                    Err(_) => {
                        eprintln!("error: invalid PID: {}", args[i + 1]);
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -o requires a column list");
                    process::exit(1);
                }
                config.format = OutputFormat::Custom;
                for col_name in args[i + 1].split(',') {
                    let col_name = col_name.trim();
                    if col_name.is_empty() {
                        continue;
                    }
                    match parse_column(col_name) {
                        Some(col) => config.custom_columns.push(col),
                        None => {
                            eprintln!("error: unknown column: {col_name}");
                            eprintln!("Run 'ps --help' for available columns.");
                            process::exit(1);
                        }
                    }
                }
                if config.custom_columns.is_empty() {
                    eprintln!("error: -o requires at least one column");
                    process::exit(1);
                }
                i += 2;
            }
            "-t" => {
                config.tree_view = true;
                i += 1;
            }
            "--sort" => {
                if i + 1 >= args.len() {
                    eprintln!("error: --sort requires a field name");
                    process::exit(1);
                }
                match parse_sort_field(&args[i + 1]) {
                    Some(f) => config.sort_field = f,
                    None => {
                        eprintln!("error: unknown sort field: {}", args[i + 1]);
                        eprintln!("Valid: pid, name, cpu, mem, time");
                        process::exit(1);
                    }
                }
                i += 2;
            }
            "--reverse" => {
                config.sort_reverse = true;
                i += 1;
            }
            "--json" => {
                config.json_output = true;
                i += 1;
            }
            "--no-header" => {
                config.no_header = true;
                i += 1;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            // Handle combined short flags like "-ef", "-el", "-efl".
            combined
                if combined.starts_with('-')
                    && combined.len() > 2
                    && !combined.starts_with("--") =>
            {
                // Expand combined flags and reprocess.
                let flags = &combined[1..];
                for ch in flags.chars() {
                    match ch {
                        'e' | 'A' => config.show_all = true,
                        'f' => config.format = OutputFormat::Full,
                        'l' => config.format = OutputFormat::Long,
                        't' => config.tree_view = true,
                        _ => {
                            eprintln!("error: unknown flag in combined option: -{ch}");
                            eprintln!("Run 'ps --help' for usage.");
                            process::exit(1);
                        }
                    }
                }
                i += 1;
            }
            other => {
                eprintln!("error: unknown option: {other}");
                eprintln!("Run 'ps --help' for usage.");
                process::exit(1);
            }
        }
    }

    // Gather data.
    let mut procs = if let Some(pid) = config.filter_pid {
        // Only read the specific PID.
        match read_process(pid) {
            Some(p) => vec![p],
            None => {
                eprintln!("error: no process with PID {pid}");
                process::exit(1);
            }
        }
    } else {
        read_all_processes()
    };

    // Apply UID filter if specified.
    if let Some(uid) = config.filter_uid {
        procs.retain(|p| p.uid == uid);
    }

    // Gather system info for CPU% and MEM% calculations.
    let uptime_ticks = system_uptime_ticks();
    let mem_total_kb = total_memory_kb();

    // Sort.
    sort_processes(
        &mut procs,
        config.sort_field,
        config.sort_reverse,
        uptime_ticks,
        mem_total_kb,
    );

    // Output.
    if config.json_output {
        print_json(&procs, uptime_ticks, mem_total_kb);
    } else if config.tree_view {
        print_tree(&procs, config.no_header);
    } else {
        match config.format {
            OutputFormat::Default => print_default(&procs, config.no_header),
            OutputFormat::Full => print_full(&procs, config.no_header, uptime_ticks),
            OutputFormat::Long => print_long(&procs, config.no_header, uptime_ticks),
            OutputFormat::User => {
                print_user(&procs, config.no_header, uptime_ticks, mem_total_kb);
            }
            OutputFormat::Custom => {
                print_custom(
                    &procs,
                    &config.custom_columns,
                    config.no_header,
                    uptime_ticks,
                    mem_total_kb,
                );
            }
        }
    }
}
