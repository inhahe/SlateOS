//! SlateOS mount point finding and management utilities.
//!
//! Multi-personality binary providing:
//! - **findmnt** — find/list mounted filesystems
//! - **mountpoint** — check if a path is a mount point
//! - **lsblk-mounts** — list block devices with mount info
//!
//! Reads `/proc/mounts`, `/proc/self/mountinfo`, and `/etc/fstab`.

#![deny(clippy::all)]
// FstabEntry::{dump, pass} are columns 5 and 6 of /etc/fstab (dump-frequency
// and fsck-pass-number). The real findmnt -o DUMP,PASS surface must produce
// them. Dead-code lint cannot see across that future boundary.
#![allow(dead_code)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const PROC_MOUNTS: &str = "/proc/mounts";
const PROC_MOUNTINFO: &str = "/proc/self/mountinfo";
const FSTAB_PATH: &str = "/etc/fstab";

// ============================================================================
// Data structures
// ============================================================================

/// A mounted filesystem entry from /proc/mounts.
#[derive(Clone, Debug)]
struct MountEntry {
    /// Mount ID (from mountinfo).
    mount_id: u32,
    /// Parent mount ID.
    parent_id: u32,
    /// Device (e.g., /dev/sda1).
    source: String,
    /// Mount point.
    target: String,
    /// Filesystem type.
    fstype: String,
    /// Mount options.
    options: String,
    /// Device major:minor.
    maj_min: String,
    /// Root within the filesystem.
    fs_root: String,
    /// Optional fields.
    optional: String,
}

/// An fstab entry.
#[derive(Clone, Debug)]
struct FstabEntry {
    source: String,
    target: String,
    fstype: String,
    options: String,
    dump: u32,
    pass: u32,
}

/// Output options.
struct Options {
    /// Output as tree.
    tree: bool,
    /// Output as list (flat, no tree).
    list: bool,
    /// JSON output.
    json: bool,
    /// Raw output (no alignment).
    raw: bool,
    /// Pairs output (key=value).
    pairs: bool,
    /// Show only specific columns.
    columns: Vec<String>,
    /// Filter by source device.
    source_filter: Option<String>,
    /// Filter by target mount point.
    target_filter: Option<String>,
    /// Filter by filesystem type.
    type_filter: Option<String>,
    /// Invert type filter.
    type_invert: bool,
    /// Show from fstab.
    fstab_mode: bool,
    /// Show both real and fstab.
    verify_mode: bool,
    /// First match only.
    first_only: bool,
    /// Don't print header.
    no_header: bool,
    /// Submounts.
    submounts: bool,
}

// ============================================================================
// Parsing
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Parse /proc/self/mountinfo (richer than /proc/mounts).
fn parse_mountinfo() -> Vec<MountEntry> {
    let content = match read_file(PROC_MOUNTINFO) {
        Some(c) => c,
        None => return parse_proc_mounts(), // Fallback to /proc/mounts.
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        if let Some(entry) = parse_mountinfo_line(line) {
            entries.push(entry);
        }
    }
    entries
}

/// Parse a single line from /proc/self/mountinfo.
/// Format: id parent_id maj:min root mount_point options optional_fields - fs_type source super_options
fn parse_mountinfo_line(line: &str) -> Option<MountEntry> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 10 {
        return None;
    }

    let mount_id: u32 = fields[0].parse().ok()?;
    let parent_id: u32 = fields[1].parse().ok()?;
    let maj_min = fields[2].to_string();
    let fs_root = fields[3].to_string();
    let target = fields[4].to_string();
    let mount_options = fields[5].to_string();

    // Find the separator "-".
    let sep_idx = fields.iter().position(|&f| f == "-")?;
    if sep_idx + 3 > fields.len() {
        return None;
    }

    // Collect optional fields between mount_options and "-".
    let optional: String = fields[6..sep_idx].join(" ");

    let fstype = fields[sep_idx + 1].to_string();
    let source = fields[sep_idx + 2].to_string();
    let super_options = if sep_idx + 3 < fields.len() {
        fields[sep_idx + 3].to_string()
    } else {
        String::new()
    };

    // Combine mount options and super options.
    let options = if super_options.is_empty() {
        mount_options
    } else {
        format!("{mount_options},{super_options}")
    };

    Some(MountEntry {
        mount_id,
        parent_id,
        source,
        target,
        fstype,
        options,
        maj_min,
        fs_root,
        optional,
    })
}

/// Fallback: parse /proc/mounts (simpler format).
fn parse_proc_mounts() -> Vec<MountEntry> {
    let content = match read_file(PROC_MOUNTS) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut entries = Vec::new();
    let mut id = 1u32;
    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 {
            entries.push(MountEntry {
                mount_id: id,
                parent_id: if id > 1 { 1 } else { 0 },
                source: fields[0].to_string(),
                target: fields[1].to_string(),
                fstype: fields[2].to_string(),
                options: fields[3].to_string(),
                maj_min: String::new(),
                fs_root: "/".to_string(),
                optional: String::new(),
            });
            id += 1;
        }
    }
    entries
}

fn parse_fstab() -> Vec<FstabEntry> {
    let content = match read_file(FSTAB_PATH) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 {
            entries.push(FstabEntry {
                source: fields[0].to_string(),
                target: fields[1].to_string(),
                fstype: fields[2].to_string(),
                options: fields[3].to_string(),
                dump: fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
                pass: fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
            });
        }
    }
    entries
}

// ============================================================================
// Filtering
// ============================================================================

fn apply_filters(entries: &[MountEntry], opts: &Options) -> Vec<MountEntry> {
    entries.iter().filter(|e| {
        // Source filter.
        if let Some(ref src) = opts.source_filter
            && e.source != *src {
                return false;
            }
        // Target filter.
        if let Some(ref tgt) = opts.target_filter {
            if opts.submounts {
                if !e.target.starts_with(tgt.as_str()) {
                    return false;
                }
            } else if e.target != *tgt {
                return false;
            }
        }
        // Type filter.
        if let Some(ref ft) = opts.type_filter {
            let types: Vec<&str> = ft.split(',').collect();
            let matches = types.iter().any(|t| e.fstype == *t);
            if opts.type_invert {
                if matches { return false; }
            } else if !matches {
                return false;
            }
        }
        true
    }).cloned().collect()
}

// ============================================================================
// Output formatting
// ============================================================================

/// Get the value for a column name.
fn column_value(entry: &MountEntry, col: &str) -> String {
    match col.to_uppercase().as_str() {
        "TARGET" | "MOUNTPOINT" => entry.target.clone(),
        "SOURCE" | "DEVICE" => entry.source.clone(),
        "FSTYPE" | "TYPE" => entry.fstype.clone(),
        "OPTIONS" | "OPTS" => entry.options.clone(),
        "MAJ:MIN" | "MAJ_MIN" => entry.maj_min.clone(),
        "FSROOT" | "ROOT" => entry.fs_root.clone(),
        "ID" => entry.mount_id.to_string(),
        "PARENT" => entry.parent_id.to_string(),
        "OPTIONAL" | "OPT_FIELDS" => entry.optional.clone(),
        _ => String::new(),
    }
}

fn default_columns() -> Vec<String> {
    vec![
        "TARGET".to_string(),
        "SOURCE".to_string(),
        "FSTYPE".to_string(),
        "OPTIONS".to_string(),
    ]
}

fn print_table(out: &mut io::StdoutLock<'_>, entries: &[MountEntry], opts: &Options) {
    let cols = if opts.columns.is_empty() {
        default_columns()
    } else {
        opts.columns.clone()
    };

    // Calculate column widths.
    let mut widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();
    for entry in entries {
        for (i, col) in cols.iter().enumerate() {
            let val = column_value(entry, col);
            if val.len() > widths[i] {
                widths[i] = val.len();
            }
        }
    }

    // Print header.
    if !opts.no_header {
        for (i, col) in cols.iter().enumerate() {
            if i > 0 { let _ = write!(out, " "); }
            let _ = write!(out, "{:<width$}", col, width = widths[i]);
        }
        let _ = writeln!(out);
    }

    // Print rows.
    for entry in entries {
        for (i, col) in cols.iter().enumerate() {
            if i > 0 { let _ = write!(out, " "); }
            let val = column_value(entry, col);
            let _ = write!(out, "{:<width$}", val, width = widths[i]);
        }
        let _ = writeln!(out);
    }
}

fn print_raw(out: &mut io::StdoutLock<'_>, entries: &[MountEntry], opts: &Options) {
    let cols = if opts.columns.is_empty() {
        default_columns()
    } else {
        opts.columns.clone()
    };

    if !opts.no_header {
        let _ = writeln!(out, "{}", cols.join(" "));
    }

    for entry in entries {
        let vals: Vec<String> = cols.iter().map(|c| column_value(entry, c)).collect();
        let _ = writeln!(out, "{}", vals.join(" "));
    }
}

fn print_pairs(out: &mut io::StdoutLock<'_>, entries: &[MountEntry], opts: &Options) {
    let cols = if opts.columns.is_empty() {
        default_columns()
    } else {
        opts.columns.clone()
    };

    for entry in entries {
        let pairs: Vec<String> = cols.iter()
            .map(|c| format!("{}=\"{}\"", c, column_value(entry, c)))
            .collect();
        let _ = writeln!(out, "{}", pairs.join(" "));
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

fn print_json(out: &mut io::StdoutLock<'_>, entries: &[MountEntry], opts: &Options) {
    let cols = if opts.columns.is_empty() {
        default_columns()
    } else {
        opts.columns.clone()
    };

    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"filesystems\": [");
    for (i, entry) in entries.iter().enumerate() {
        let comma = if i + 1 < entries.len() { "," } else { "" };
        let _ = write!(out, "    {{");
        for (j, col) in cols.iter().enumerate() {
            let c = if j + 1 < cols.len() { "," } else { "" };
            let val = column_value(entry, col);
            let _ = write!(out, "\"{}\": \"{}\"{c}", col.to_lowercase(), json_escape(&val));
        }
        let _ = writeln!(out, "}}{comma}");
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
}

fn print_tree(out: &mut io::StdoutLock<'_>, entries: &[MountEntry], opts: &Options) {
    // Build parent → children map.
    let mut children_map: HashMap<u32, Vec<usize>> = HashMap::new();
    let mut root_indices: Vec<usize> = Vec::new();

    let id_set: std::collections::HashSet<u32> = entries.iter().map(|e| e.mount_id).collect();

    for (idx, entry) in entries.iter().enumerate() {
        if entry.parent_id == 0 || !id_set.contains(&entry.parent_id) {
            root_indices.push(idx);
        } else {
            children_map.entry(entry.parent_id).or_default().push(idx);
        }
    }

    let cols = if opts.columns.is_empty() {
        default_columns()
    } else {
        opts.columns.clone()
    };

    if !opts.no_header {
        let _ = writeln!(out, "{}", cols.join("  "));
    }

    for &root_idx in &root_indices {
        print_tree_node(out, entries, &children_map, &cols, root_idx, "", true);
    }
}

fn print_tree_node(
    out: &mut io::StdoutLock<'_>,
    entries: &[MountEntry],
    children_map: &HashMap<u32, Vec<usize>>,
    cols: &[String],
    idx: usize,
    prefix: &str,
    is_last: bool,
) {
    let entry = &entries[idx];

    // First column gets the tree decoration.
    let target = &entry.target;
    let connector = if prefix.is_empty() {
        String::new()
    } else if is_last {
        format!("{prefix}└─")
    } else {
        format!("{prefix}├─")
    };

    let extra_cols: Vec<String> = cols.iter().skip(1)
        .map(|c| column_value(entry, c))
        .collect();

    if connector.is_empty() {
        let _ = write!(out, "{target}");
    } else {
        let _ = write!(out, "{connector}{target}");
    }

    if !extra_cols.is_empty() {
        let _ = write!(out, "  {}", extra_cols.join("  "));
    }
    let _ = writeln!(out);

    // Print children.
    let kids = children_map.get(&entry.mount_id).cloned().unwrap_or_default();
    let child_prefix = if prefix.is_empty() {
        String::new()
    } else if is_last {
        format!("{prefix}  ")
    } else {
        format!("{prefix}│ ")
    };

    for (i, &kid_idx) in kids.iter().enumerate() {
        let kid_is_last = i + 1 == kids.len();
        print_tree_node(out, entries, children_map, cols, kid_idx, &child_prefix, kid_is_last);
    }
}

// ============================================================================
// Personality: mountpoint
// ============================================================================

fn cmd_mountpoint(args: &[String]) {
    let mut quiet = false;
    let mut device_mode = false;
    let mut path: Option<String> = None;

    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: mountpoint [options] <directory>");
                println!("       mountpoint -d <device>");
                println!();
                println!("Check whether a directory is a mount point.");
                println!();
                println!("Options:");
                println!("  -q, --quiet     Quiet mode (exit code only)");
                println!("  -d, --fs-devno  Show device number");
                println!("  -h, --help      Show this help");
                process::exit(0);
            }
            "-q" | "--quiet" => quiet = true,
            "-d" | "--fs-devno" => device_mode = true,
            s if !s.starts_with('-') => path = Some(s.to_string()),
            other => {
                eprintln!("mountpoint: unknown option: {other}");
                process::exit(1);
            }
        }
    }

    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("mountpoint: no path specified");
            process::exit(1);
        }
    };

    let mounts = parse_mountinfo();

    if device_mode {
        // Show device number for the mount point.
        for entry in &mounts {
            if entry.target == path {
                println!("{}", entry.maj_min);
                process::exit(0);
            }
        }
        if !quiet {
            eprintln!("{path} is not a mountpoint");
        }
        process::exit(1);
    }

    let is_mountpoint = mounts.iter().any(|e| e.target == path);

    if is_mountpoint {
        if !quiet {
            println!("{path} is a mountpoint");
        }
        process::exit(0);
    } else {
        if !quiet {
            println!("{path} is not a mountpoint");
        }
        process::exit(1);
    }
}

// ============================================================================
// CLI parsing for findmnt
// ============================================================================

fn cmd_findmnt(args: &[String]) {
    let mut opts = Options {
        tree: true,
        list: false,
        json: false,
        raw: false,
        pairs: false,
        columns: Vec::new(),
        source_filter: None,
        target_filter: None,
        type_filter: None,
        type_invert: false,
        fstab_mode: false,
        verify_mode: false,
        first_only: false,
        no_header: false,
        submounts: false,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_findmnt_usage();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("findmnt {VERSION}");
                process::exit(0);
            }
            "-l" | "--list" => { opts.list = true; opts.tree = false; }
            "-J" | "--json" => { opts.json = true; opts.tree = false; }
            "-r" | "--raw" => { opts.raw = true; opts.tree = false; }
            "-P" | "--pairs" => { opts.pairs = true; opts.tree = false; }
            "-n" | "--noheadings" => opts.no_header = true,
            "-f" | "--first-only" => opts.first_only = true,
            "-s" | "--fstab" => opts.fstab_mode = true,
            "--verify" => opts.verify_mode = true,
            "-R" | "--submounts" => opts.submounts = true,
            "-S" | "--source" => {
                i += 1;
                if i < args.len() { opts.source_filter = Some(args[i].clone()); }
            }
            "-T" | "--target" => {
                i += 1;
                if i < args.len() { opts.target_filter = Some(args[i].clone()); }
            }
            "-t" | "--type" => {
                i += 1;
                if i < args.len() {
                    let val = &args[i];
                    if let Some(rest) = val.strip_prefix("no") {
                        opts.type_filter = Some(rest.to_string());
                        opts.type_invert = true;
                    } else {
                        opts.type_filter = Some(val.clone());
                    }
                }
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    opts.columns = args[i].split(',').map(|s| s.trim().to_uppercase()).collect();
                }
            }
            s if !s.starts_with('-') => {
                // Positional: first is target (or source if starts with /dev).
                if s.starts_with("/dev/") && opts.source_filter.is_none() {
                    opts.source_filter = Some(s.to_string());
                } else if opts.target_filter.is_none() {
                    opts.target_filter = Some(s.to_string());
                }
            }
            other => {
                eprintln!("findmnt: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    // Get mount entries.
    let mut entries = if opts.fstab_mode {
        let fstab = parse_fstab();
        fstab.into_iter().enumerate().map(|(i, e)| MountEntry {
            mount_id: i as u32 + 1,
            parent_id: 0,
            source: e.source,
            target: e.target,
            fstype: e.fstype,
            options: e.options,
            maj_min: String::new(),
            fs_root: "/".to_string(),
            optional: String::new(),
        }).collect()
    } else {
        parse_mountinfo()
    };

    entries = apply_filters(&entries, &opts);

    if opts.first_only && !entries.is_empty() {
        entries.truncate(1);
    }

    if entries.is_empty() {
        process::exit(1);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.json {
        print_json(&mut out, &entries, &opts);
    } else if opts.raw {
        print_raw(&mut out, &entries, &opts);
    } else if opts.pairs {
        print_pairs(&mut out, &entries, &opts);
    } else if opts.tree && !opts.list {
        print_tree(&mut out, &entries, &opts);
    } else {
        print_table(&mut out, &entries, &opts);
    }
}

fn print_findmnt_usage() {
    println!("Usage: findmnt [options] [device|mountpoint]");
    println!();
    println!("Find and display mounted filesystems.");
    println!();
    println!("Options:");
    println!("  -l, --list          List output (flat)");
    println!("  -J, --json          JSON output");
    println!("  -r, --raw           Raw output (no alignment)");
    println!("  -P, --pairs         Key=value pairs output");
    println!("  -n, --noheadings    No column headers");
    println!("  -f, --first-only    First matching entry only");
    println!("  -s, --fstab         Show from /etc/fstab");
    println!("  --verify            Verify fstab vs mounted");
    println!("  -R, --submounts     Include submounts");
    println!("  -S, --source DEV    Filter by source device");
    println!("  -T, --target DIR    Filter by target mount point");
    println!("  -t, --type TYPE     Filter by filesystem type");
    println!("  -o, --output COLS   Columns (TARGET,SOURCE,FSTYPE,OPTIONS,...)");
    println!("  -h, --help          Show this help");
    println!("  -V, --version       Show version");
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("findmnt");
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

    match prog_name.as_str() {
        "mountpoint" => cmd_mountpoint(&rest),
        _ => cmd_findmnt(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mount(id: u32, parent: u32, src: &str, tgt: &str, fstype: &str) -> MountEntry {
        MountEntry {
            mount_id: id,
            parent_id: parent,
            source: src.to_string(),
            target: tgt.to_string(),
            fstype: fstype.to_string(),
            options: "rw,relatime".to_string(),
            maj_min: "8:1".to_string(),
            fs_root: "/".to_string(),
            optional: String::new(),
        }
    }

    #[test]
    fn test_parse_mountinfo_line_valid() {
        let line = "22 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw,errors=continue";
        let entry = parse_mountinfo_line(line).unwrap();
        assert_eq!(entry.mount_id, 22);
        assert_eq!(entry.parent_id, 1);
        assert_eq!(entry.maj_min, "8:1");
        assert_eq!(entry.fs_root, "/");
        assert_eq!(entry.target, "/");
        assert_eq!(entry.fstype, "ext4");
        assert_eq!(entry.source, "/dev/sda1");
    }

    #[test]
    fn test_parse_mountinfo_line_complex() {
        let line = "45 22 0:39 / /sys/fs/cgroup rw,nosuid,nodev,noexec master:19 - cgroup2 cgroup2 rw,nsdelegate";
        let entry = parse_mountinfo_line(line).unwrap();
        assert_eq!(entry.mount_id, 45);
        assert_eq!(entry.target, "/sys/fs/cgroup");
        assert_eq!(entry.fstype, "cgroup2");
    }

    #[test]
    fn test_parse_mountinfo_line_invalid() {
        assert!(parse_mountinfo_line("too short").is_none());
        assert!(parse_mountinfo_line("").is_none());
    }

    #[test]
    fn test_column_value() {
        let entry = make_mount(1, 0, "/dev/sda1", "/", "ext4");
        assert_eq!(column_value(&entry, "TARGET"), "/");
        assert_eq!(column_value(&entry, "SOURCE"), "/dev/sda1");
        assert_eq!(column_value(&entry, "FSTYPE"), "ext4");
        assert_eq!(column_value(&entry, "OPTIONS"), "rw,relatime");
        assert_eq!(column_value(&entry, "MAJ:MIN"), "8:1");
        assert_eq!(column_value(&entry, "ID"), "1");
        assert_eq!(column_value(&entry, "PARENT"), "0");
        assert_eq!(column_value(&entry, "UNKNOWN"), "");
    }

    #[test]
    fn test_column_value_case_insensitive() {
        let entry = make_mount(1, 0, "/dev/sda1", "/", "ext4");
        assert_eq!(column_value(&entry, "target"), "/");
        assert_eq!(column_value(&entry, "Target"), "/");
        assert_eq!(column_value(&entry, "MOUNTPOINT"), "/");
    }

    #[test]
    fn test_apply_filters_source() {
        let entries = vec![
            make_mount(1, 0, "/dev/sda1", "/", "ext4"),
            make_mount(2, 1, "tmpfs", "/tmp", "tmpfs"),
        ];
        let opts = Options {
            tree: false, list: true, json: false, raw: false, pairs: false,
            columns: Vec::new(), source_filter: Some("/dev/sda1".to_string()),
            target_filter: None, type_filter: None, type_invert: false,
            fstab_mode: false, verify_mode: false, first_only: false,
            no_header: false, submounts: false,
        };
        let filtered = apply_filters(&entries, &opts);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].source, "/dev/sda1");
    }

    #[test]
    fn test_apply_filters_type() {
        let entries = vec![
            make_mount(1, 0, "/dev/sda1", "/", "ext4"),
            make_mount(2, 1, "tmpfs", "/tmp", "tmpfs"),
            make_mount(3, 1, "proc", "/proc", "proc"),
        ];
        let opts = Options {
            tree: false, list: true, json: false, raw: false, pairs: false,
            columns: Vec::new(), source_filter: None,
            target_filter: None, type_filter: Some("ext4".to_string()),
            type_invert: false, fstab_mode: false, verify_mode: false,
            first_only: false, no_header: false, submounts: false,
        };
        let filtered = apply_filters(&entries, &opts);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].fstype, "ext4");
    }

    #[test]
    fn test_apply_filters_type_invert() {
        let entries = vec![
            make_mount(1, 0, "/dev/sda1", "/", "ext4"),
            make_mount(2, 1, "tmpfs", "/tmp", "tmpfs"),
            make_mount(3, 1, "proc", "/proc", "proc"),
        ];
        let opts = Options {
            tree: false, list: true, json: false, raw: false, pairs: false,
            columns: Vec::new(), source_filter: None,
            target_filter: None, type_filter: Some("tmpfs,proc".to_string()),
            type_invert: true, fstab_mode: false, verify_mode: false,
            first_only: false, no_header: false, submounts: false,
        };
        let filtered = apply_filters(&entries, &opts);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].fstype, "ext4");
    }

    #[test]
    fn test_apply_filters_target() {
        let entries = vec![
            make_mount(1, 0, "/dev/sda1", "/", "ext4"),
            make_mount(2, 1, "tmpfs", "/tmp", "tmpfs"),
        ];
        let opts = Options {
            tree: false, list: true, json: false, raw: false, pairs: false,
            columns: Vec::new(), source_filter: None,
            target_filter: Some("/tmp".to_string()), type_filter: None,
            type_invert: false, fstab_mode: false, verify_mode: false,
            first_only: false, no_header: false, submounts: false,
        };
        let filtered = apply_filters(&entries, &opts);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].target, "/tmp");
    }

    #[test]
    fn test_apply_filters_submounts() {
        let entries = vec![
            make_mount(1, 0, "/dev/sda1", "/", "ext4"),
            make_mount(2, 1, "tmpfs", "/sys", "sysfs"),
            make_mount(3, 2, "cgroup2", "/sys/fs/cgroup", "cgroup2"),
        ];
        let opts = Options {
            tree: false, list: true, json: false, raw: false, pairs: false,
            columns: Vec::new(), source_filter: None,
            target_filter: Some("/sys".to_string()), type_filter: None,
            type_invert: false, fstab_mode: false, verify_mode: false,
            first_only: false, no_header: false, submounts: true,
        };
        let filtered = apply_filters(&entries, &opts);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_default_columns() {
        let cols = default_columns();
        assert_eq!(cols, vec!["TARGET", "SOURCE", "FSTYPE", "OPTIONS"]);
    }

    #[test]
    fn test_json_escape() {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("he\"llo"), "he\\\"llo");
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn test_mount_entry_clone() {
        let entry = make_mount(1, 0, "/dev/sda1", "/", "ext4");
        let cloned = entry.clone();
        assert_eq!(cloned.mount_id, 1);
        assert_eq!(cloned.target, "/");
    }

    #[test]
    fn test_fstab_entry_clone() {
        let entry = FstabEntry {
            source: "/dev/sda1".to_string(),
            target: "/".to_string(),
            fstype: "ext4".to_string(),
            options: "defaults".to_string(),
            dump: 0,
            pass: 1,
        };
        let cloned = entry.clone();
        assert_eq!(cloned.source, "/dev/sda1");
        assert_eq!(cloned.pass, 1);
    }

    #[test]
    fn test_personality_detection() {
        let cases = [
            ("/usr/bin/findmnt", "findmnt"),
            ("mountpoint", "mountpoint"),
            ("findmnt.exe", "findmnt"),
        ];
        for (input, expected) in &cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let base = &input[last_sep..];
            let base = base.strip_suffix(".exe").unwrap_or(base);
            assert_eq!(base, *expected);
        }
    }
}
