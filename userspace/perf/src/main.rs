//! OurOS Performance Monitoring and Analysis Tool
//!
//! Multi-personality binary providing Linux-perf-compatible performance
//! analysis.  Personality is detected from `argv[0]`:
//!
//!   - `perf`        — dispatcher for subcommands
//!   - `perf-stat`   — collect performance counter statistics
//!   - `perf-record` — record performance event samples to perf.data
//!   - `perf-report` — analyze recorded perf.data files
//!   - `perf-top`    — real-time function profiling
//!
//! All counters are read from simulated `/proc/<pid>/perf_events` and
//! `/sys/kernel/perf/` paths.  When those files are absent the tool
//! returns graceful defaults so it remains usable during early OS
//! bring-up.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::process;

// ============================================================================
// Version
// ============================================================================

const VERSION: &str = "0.1.0";

// ============================================================================
// Personality detection
// ============================================================================

/// Identify which tool personality to run based on `argv[0]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    /// Main dispatcher — `perf <subcommand> ...`
    Perf,
    /// `perf-stat` — counter statistics
    Stat,
    /// `perf-record` — event recording
    Record,
    /// `perf-report` — analysis of recorded data
    Report,
    /// `perf-top` — real-time profiling
    Top,
}

/// Extract the tool basename from the first argument, stripping any directory
/// prefix and `.exe` suffix, then map to a `Personality`.
fn detect_personality(argv0: &str) -> Personality {
    let bytes = argv0.as_bytes();
    let mut last_sep = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i + 1;
        }
    }
    let base = &argv0[last_sep..];
    let base = base.strip_suffix(".exe").unwrap_or(base);

    match base {
        "perf-stat" => Personality::Stat,
        "perf-record" => Personality::Record,
        "perf-report" => Personality::Report,
        "perf-top" => Personality::Top,
        _ => Personality::Perf,
    }
}

// ============================================================================
// Event catalogue
// ============================================================================

/// Classification of a performance counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    Hardware,
    Software,
}

/// A single event descriptor.
#[derive(Debug, Clone)]
struct EventDesc {
    name: &'static str,
    kind: EventKind,
    /// Path suffix under `/sys/kernel/perf/` for system-wide counters.
    sys_path: &'static str,
    /// Key inside `/proc/<pid>/perf_events` for per-process counters.
    proc_key: &'static str,
}

/// Complete list of supported events.
const EVENTS: &[EventDesc] = &[
    // -- hardware --
    EventDesc { name: "cycles",              kind: EventKind::Hardware, sys_path: "hw/cycles",              proc_key: "cycles" },
    EventDesc { name: "instructions",        kind: EventKind::Hardware, sys_path: "hw/instructions",        proc_key: "instructions" },
    EventDesc { name: "cache-references",    kind: EventKind::Hardware, sys_path: "hw/cache_references",    proc_key: "cache_references" },
    EventDesc { name: "cache-misses",        kind: EventKind::Hardware, sys_path: "hw/cache_misses",        proc_key: "cache_misses" },
    EventDesc { name: "branch-instructions", kind: EventKind::Hardware, sys_path: "hw/branch_instructions", proc_key: "branch_instructions" },
    EventDesc { name: "branch-misses",       kind: EventKind::Hardware, sys_path: "hw/branch_misses",       proc_key: "branch_misses" },
    EventDesc { name: "bus-cycles",          kind: EventKind::Hardware, sys_path: "hw/bus_cycles",          proc_key: "bus_cycles" },
    EventDesc { name: "ref-cycles",          kind: EventKind::Hardware, sys_path: "hw/ref_cycles",          proc_key: "ref_cycles" },
    // -- software --
    EventDesc { name: "task-clock",          kind: EventKind::Software, sys_path: "sw/task_clock",          proc_key: "task_clock" },
    EventDesc { name: "context-switches",    kind: EventKind::Software, sys_path: "sw/context_switches",    proc_key: "context_switches" },
    EventDesc { name: "cpu-migrations",      kind: EventKind::Software, sys_path: "sw/cpu_migrations",      proc_key: "cpu_migrations" },
    EventDesc { name: "page-faults",         kind: EventKind::Software, sys_path: "sw/page_faults",         proc_key: "page_faults" },
    EventDesc { name: "minor-faults",        kind: EventKind::Software, sys_path: "sw/minor_faults",        proc_key: "minor_faults" },
    EventDesc { name: "major-faults",        kind: EventKind::Software, sys_path: "sw/major_faults",        proc_key: "major_faults" },
];

/// Find an event by user-supplied name (case-insensitive).
fn find_event(name: &str) -> Option<&'static EventDesc> {
    let lower = name.to_ascii_lowercase();
    EVENTS.iter().find(|e| e.name == lower)
}

/// Return the default set of stat events (all of them).
fn default_stat_events() -> Vec<&'static EventDesc> {
    EVENTS.iter().collect()
}

// ============================================================================
// Counter reading helpers
// ============================================================================

/// Read a single integer from a filesystem path, returning 0 on any error.
fn read_u64_file(path: &str) -> u64 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Read all counters from `/proc/<pid>/perf_events`.
///
/// Expected file format — one `key: value` pair per line:
///
/// ```text
/// cycles: 123456
/// instructions: 654321
/// ```
///
/// Returns an empty map when the file is absent.
fn read_proc_perf_events(pid: u32) -> HashMap<String, u64> {
    let path = format!("/proc/{pid}/perf_events");
    let mut map = HashMap::new();
    if let Ok(contents) = fs::read_to_string(&path) {
        for line in contents.lines() {
            if let Some((key, val)) = line.split_once(':') {
                let key = key.trim().to_string();
                if let Ok(v) = val.trim().parse::<u64>() {
                    map.insert(key, v);
                }
            }
        }
    }
    map
}

/// Read a system-wide counter from `/sys/kernel/perf/<suffix>`.
fn read_sys_counter(suffix: &str) -> u64 {
    let path = format!("/sys/kernel/perf/{suffix}");
    read_u64_file(&path)
}

/// Read a counter value for the given event, either system-wide (`pid=None`)
/// or per-process.
fn read_counter(event: &EventDesc, pid: Option<u32>) -> u64 {
    match pid {
        Some(p) => {
            let map = read_proc_perf_events(p);
            map.get(event.proc_key).copied().unwrap_or(0)
        }
        None => read_sys_counter(event.sys_path),
    }
}

// ============================================================================
// perf.data binary format
// ============================================================================

/// Magic bytes at the start of every perf.data file.
const PERF_MAGIC: &[u8; 8] = b"PERFDATA";

/// Current format version.
const PERF_VERSION: u32 = 1;

/// Header that starts every perf.data file.
#[derive(Debug, Clone)]
struct PerfFileHeader {
    /// Offset (from file start) where sample data begins.
    data_offset: u64,
    /// Total byte size of sample data section.
    data_size: u64,
    /// Number of samples stored.
    sample_count: u64,
    /// Name of the event being sampled.
    event_name: String,
    /// Sample frequency (Hz).
    frequency: u32,
    /// Whether call-graph data is included.
    has_callgraph: bool,
}

impl PerfFileHeader {
    /// Fixed on-disk size of the header (magic + version + fields).
    /// 8 (magic) + 4 (version) + 8 + 8 + 8 + 64 (event name padded) + 4 + 1 + 3 padding = 108
    const DISK_SIZE: u64 = 108;

    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        w.write_all(PERF_MAGIC)?;
        w.write_all(&PERF_VERSION.to_le_bytes())?;
        w.write_all(&self.data_offset.to_le_bytes())?;
        w.write_all(&self.data_size.to_le_bytes())?;
        w.write_all(&self.sample_count.to_le_bytes())?;
        // Event name — 64 bytes, zero-padded.
        let mut name_buf = [0u8; 64];
        let name_bytes = self.event_name.as_bytes();
        let copy_len = name_bytes.len().min(63);
        name_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        w.write_all(&name_buf)?;
        w.write_all(&self.frequency.to_le_bytes())?;
        w.write_all(&[if self.has_callgraph { 1 } else { 0 }])?;
        // 3 bytes padding for alignment.
        w.write_all(&[0u8; 3])?;
        Ok(())
    }

    fn read_from(r: &mut dyn Read) -> io::Result<Self> {
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != PERF_MAGIC {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a perf.data file"));
        }
        let mut buf4 = [0u8; 4];
        r.read_exact(&mut buf4)?;
        let version = u32::from_le_bytes(buf4);
        if version != PERF_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported perf.data version {version}"),
            ));
        }
        let mut buf8 = [0u8; 8];
        r.read_exact(&mut buf8)?;
        let data_offset = u64::from_le_bytes(buf8);
        r.read_exact(&mut buf8)?;
        let data_size = u64::from_le_bytes(buf8);
        r.read_exact(&mut buf8)?;
        let sample_count = u64::from_le_bytes(buf8);
        let mut name_buf = [0u8; 64];
        r.read_exact(&mut name_buf)?;
        let name_end = name_buf.iter().position(|&b| b == 0).unwrap_or(64);
        let event_name = String::from_utf8_lossy(&name_buf[..name_end]).to_string();
        r.read_exact(&mut buf4)?;
        let frequency = u32::from_le_bytes(buf4);
        let mut flag = [0u8; 1];
        r.read_exact(&mut flag)?;
        let has_callgraph = flag[0] != 0;
        let mut _pad = [0u8; 3];
        r.read_exact(&mut _pad)?;
        Ok(Self {
            data_offset,
            data_size,
            sample_count,
            event_name,
            frequency,
            has_callgraph,
        })
    }
}

/// Maximum call-chain depth recorded per sample.
const MAX_CALLCHAIN_DEPTH: usize = 16;

/// A single recorded performance sample.
#[derive(Debug, Clone)]
struct PerfSample {
    /// Timestamp in nanoseconds since boot.
    timestamp_ns: u64,
    /// Process ID.
    pid: u32,
    /// Thread ID.
    tid: u32,
    /// Instruction pointer at sample time.
    ip: u64,
    /// Number of valid entries in `callchain`.
    callchain_len: u32,
    /// Call-chain addresses (instruction pointers), deepest first.
    callchain: [u64; MAX_CALLCHAIN_DEPTH],
}

impl PerfSample {
    /// On-disk size: 8 + 4 + 4 + 8 + 4 + (16 * 8) + 4 padding = 160 bytes.
    const DISK_SIZE: u64 = 160;

    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        w.write_all(&self.timestamp_ns.to_le_bytes())?;
        w.write_all(&self.pid.to_le_bytes())?;
        w.write_all(&self.tid.to_le_bytes())?;
        w.write_all(&self.ip.to_le_bytes())?;
        w.write_all(&self.callchain_len.to_le_bytes())?;
        for addr in &self.callchain {
            w.write_all(&addr.to_le_bytes())?;
        }
        // 4 bytes padding.
        w.write_all(&[0u8; 4])?;
        Ok(())
    }

    fn read_from(r: &mut dyn Read) -> io::Result<Self> {
        let mut b8 = [0u8; 8];
        let mut b4 = [0u8; 4];
        r.read_exact(&mut b8)?;
        let timestamp_ns = u64::from_le_bytes(b8);
        r.read_exact(&mut b4)?;
        let pid = u32::from_le_bytes(b4);
        r.read_exact(&mut b4)?;
        let tid = u32::from_le_bytes(b4);
        r.read_exact(&mut b8)?;
        let ip = u64::from_le_bytes(b8);
        r.read_exact(&mut b4)?;
        let callchain_len = u32::from_le_bytes(b4);
        let mut callchain = [0u64; MAX_CALLCHAIN_DEPTH];
        for slot in &mut callchain {
            r.read_exact(&mut b8)?;
            *slot = u64::from_le_bytes(b8);
        }
        let mut _pad = [0u8; 4];
        r.read_exact(&mut _pad)?;
        Ok(Self { timestamp_ns, pid, tid, ip, callchain_len, callchain })
    }

    fn new() -> Self {
        Self {
            timestamp_ns: 0,
            pid: 0,
            tid: 0,
            ip: 0,
            callchain_len: 0,
            callchain: [0u64; MAX_CALLCHAIN_DEPTH],
        }
    }
}

// ============================================================================
// Symbol resolution (simulated)
// ============================================================================

/// Mapping from instruction-pointer ranges to symbol names.
/// Read from `/proc/<pid>/maps` and `/proc/<pid>/symbols`.
#[derive(Debug, Clone)]
struct SymbolEntry {
    start: u64,
    end: u64,
    name: String,
    dso: String,
}

/// Read a simple symbol table from `/proc/<pid>/symbols`.
///
/// Expected format (one entry per line):
/// ```text
/// <start_hex> <end_hex> <name> <dso>
/// ```
fn read_symbols(pid: u32) -> Vec<SymbolEntry> {
    let path = format!("/proc/{pid}/symbols");
    let mut syms = Vec::new();
    if let Ok(contents) = fs::read_to_string(&path) {
        for line in contents.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4
                && let (Ok(start), Ok(end)) = (
                    u64::from_str_radix(parts[0], 16),
                    u64::from_str_radix(parts[1], 16),
                ) {
                    syms.push(SymbolEntry {
                        start,
                        end,
                        name: parts[2].to_string(),
                        dso: parts[3].to_string(),
                    });
                }
        }
    }
    syms
}

/// Resolve an instruction pointer to a symbol name.
fn resolve_symbol(ip: u64, syms: &[SymbolEntry]) -> (&str, &str) {
    for s in syms {
        if ip >= s.start && ip < s.end {
            return (&s.name, &s.dso);
        }
    }
    ("[unknown]", "[unknown]")
}

/// Resolve an IP to a symbol name, returning owned strings when no match.
fn resolve_symbol_owned(ip: u64, syms: &[SymbolEntry]) -> (String, String) {
    let (name, dso) = resolve_symbol(ip, syms);
    (name.to_string(), dso.to_string())
}

// ============================================================================
// Statistics helpers
// ============================================================================

/// Compute mean and standard deviation of a slice of f64 values.
fn mean_stddev(values: &[f64]) -> (f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    if values.len() == 1 {
        return (mean, 0.0);
    }
    let variance = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
    (mean, variance.sqrt())
}

/// Format a large number with grouping separators.
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len <= 3 {
        return s;
    }
    let mut result = String::with_capacity(len + len / 3);
    let first_group = len % 3;
    if first_group > 0 {
        // SAFETY: s is valid utf8 digits.
        result.push_str(&s[..first_group]);
    }
    for i in 0..((len - first_group) / 3) {
        if !result.is_empty() {
            result.push(',');
        }
        let start = first_group + i * 3;
        result.push_str(&s[start..start + 3]);
    }
    result
}

// ============================================================================
// perf list — enumerate available events
// ============================================================================

fn cmd_list() {
    println!("List of pre-defined events (to be used in -e):\n");
    println!("  Hardware events:");
    for ev in EVENTS {
        if ev.kind == EventKind::Hardware {
            println!("    {}", ev.name);
        }
    }
    println!("\n  Software events:");
    for ev in EVENTS {
        if ev.kind == EventKind::Software {
            println!("    {}", ev.name);
        }
    }
    println!();
}

// ============================================================================
// perf version
// ============================================================================

fn cmd_version() {
    println!("perf version {VERSION} (OurOS)");
}

// ============================================================================
// perf stat
// ============================================================================

/// Options for `perf stat`.
#[derive(Debug)]
struct StatOpts {
    events: Vec<&'static EventDesc>,
    system_wide: bool,
    pid: Option<u32>,
    repeat: u32,
    /// Command to execute and measure (used on OurOS with process spawning).
    #[allow(dead_code)]
    command: Vec<String>,
}

fn parse_stat_args(args: &[String]) -> Result<StatOpts, String> {
    let mut events: Vec<&'static EventDesc> = Vec::new();
    let mut system_wide = false;
    let mut pid: Option<u32> = None;
    let mut repeat: u32 = 1;
    let mut command: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-e" | "--event" => {
                i += 1;
                if i >= args.len() {
                    return Err("-e requires an argument".to_string());
                }
                for name in args[i].split(',') {
                    let name = name.trim();
                    match find_event(name) {
                        Some(ev) => events.push(ev),
                        None => return Err(format!("unknown event: {name}")),
                    }
                }
            }
            "-a" => system_wide = true,
            "-p" => {
                i += 1;
                if i >= args.len() {
                    return Err("-p requires a PID".to_string());
                }
                pid = Some(
                    args[i].parse::<u32>().map_err(|_| format!("invalid PID: {}", args[i]))?,
                );
            }
            "-r" => {
                i += 1;
                if i >= args.len() {
                    return Err("-r requires a repeat count".to_string());
                }
                repeat = args[i]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid repeat count: {}", args[i]))?;
                if repeat == 0 {
                    return Err("repeat count must be >= 1".to_string());
                }
            }
            "-h" | "--help" => {
                print_stat_help();
                process::exit(0);
            }
            "--" => {
                command.extend_from_slice(&args[i + 1..]);
                break;
            }
            other => {
                if other.starts_with('-') {
                    return Err(format!("unknown option: {other}"));
                }
                // Start of the command to run.
                command.extend_from_slice(&args[i..]);
                break;
            }
        }
        i += 1;
    }

    if events.is_empty() {
        events = default_stat_events();
    }

    // Need either -p, -a, or a command.
    if pid.is_none() && !system_wide && command.is_empty() {
        return Err("specify a command, -p PID, or -a for system-wide".to_string());
    }

    Ok(StatOpts { events, system_wide, pid, repeat, command })
}

fn print_stat_help() {
    println!(
        "\
Usage: perf stat [options] [<command>]

Options:
  -e, --event <event,...>  Select events to count
  -a                       System-wide collection
  -p <pid>                 Monitor existing process
  -r <n>                   Repeat command n times
  -h, --help               Show this help

Supported events:
  Hardware: cycles, instructions, cache-references, cache-misses,
            branch-instructions, branch-misses, bus-cycles, ref-cycles
  Software: task-clock, context-switches, cpu-migrations, page-faults,
            minor-faults, major-faults"
    );
}

/// Run one iteration of stat collection.  Returns counter values keyed by
/// event name.
fn stat_collect_once(
    opts: &StatOpts,
) -> HashMap<String, u64> {
    let target_pid = if opts.system_wide { None } else { opts.pid };
    let mut values = HashMap::new();
    for ev in &opts.events {
        let val = read_counter(ev, target_pid);
        values.insert(ev.name.to_string(), val);
    }
    values
}

/// Print stat results.  If there are multiple repeats, also show stddev.
fn stat_print_results(
    all_runs: &[HashMap<String, u64>],
    events: &[&'static EventDesc],
) {
    println!();
    println!(" Performance counter stats:\n");

    for ev in events {
        let vals: Vec<f64> = all_runs
            .iter()
            .map(|run| run.get(ev.name).copied().unwrap_or(0) as f64)
            .collect();
        let (mean, stddev) = mean_stddev(&vals);
        let mean_u64 = mean as u64;

        if all_runs.len() > 1 {
            println!(
                "  {:>18}  {:<24}  ( +/- {:>6.2}% )",
                format_number(mean_u64),
                ev.name,
                if mean > 0.0 { stddev / mean * 100.0 } else { 0.0 },
            );
        } else {
            println!("  {:>18}  {:<24}", format_number(mean_u64), ev.name);
        }
    }

    // Compute IPC if both cycles and instructions are present.
    let cycles_vals: Vec<f64> = all_runs
        .iter()
        .map(|run| run.get("cycles").copied().unwrap_or(0) as f64)
        .collect();
    let insn_vals: Vec<f64> = all_runs
        .iter()
        .map(|run| run.get("instructions").copied().unwrap_or(0) as f64)
        .collect();
    let (cycles_mean, _) = mean_stddev(&cycles_vals);
    let (insn_mean, _) = mean_stddev(&insn_vals);
    if cycles_mean > 0.0 && insn_mean > 0.0 {
        println!("\n  {:>18.4}  insn per cycle", insn_mean / cycles_mean);
    }

    println!();
}

fn run_stat(args: &[String]) {
    let opts = match parse_stat_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("perf stat: {e}");
            process::exit(1);
        }
    };

    let mut all_runs = Vec::new();
    for _ in 0..opts.repeat {
        let vals = stat_collect_once(&opts);
        all_runs.push(vals);
    }
    stat_print_results(&all_runs, &opts.events);
}

// ============================================================================
// perf record
// ============================================================================

/// Options for `perf record`.
#[derive(Debug)]
struct RecordOpts {
    event: String,
    frequency: u32,
    callgraph: bool,
    /// Call-graph unwinding mode (dwarf|fp|lbr) — stored for header metadata.
    #[allow(dead_code)]
    callgraph_mode: String,
    pid: Option<u32>,
    system_wide: bool,
    output: String,
    /// Command to execute and record (used on OurOS with process spawning).
    #[allow(dead_code)]
    command: Vec<String>,
}

fn parse_record_args(args: &[String]) -> Result<RecordOpts, String> {
    let mut event = "cycles".to_string();
    let mut frequency: u32 = 4000;
    let mut callgraph = false;
    let mut callgraph_mode = "fp".to_string();
    let mut pid: Option<u32> = None;
    let mut system_wide = false;
    let mut output = "perf.data".to_string();
    let mut command: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-e" | "--event" => {
                i += 1;
                if i >= args.len() {
                    return Err("-e requires an argument".to_string());
                }
                // Validate.
                if find_event(&args[i]).is_none() {
                    return Err(format!("unknown event: {}", args[i]));
                }
                event = args[i].clone();
            }
            "-F" | "--freq" => {
                i += 1;
                if i >= args.len() {
                    return Err("-F requires a frequency".to_string());
                }
                frequency = args[i]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid frequency: {}", args[i]))?;
            }
            "-g" => callgraph = true,
            "--call-graph" => {
                callgraph = true;
                i += 1;
                if i >= args.len() {
                    return Err("--call-graph requires a mode (dwarf|fp|lbr)".to_string());
                }
                match args[i].as_str() {
                    "dwarf" | "fp" | "lbr" => callgraph_mode = args[i].clone(),
                    other => return Err(format!("unknown call-graph mode: {other}")),
                }
            }
            "-p" => {
                i += 1;
                if i >= args.len() {
                    return Err("-p requires a PID".to_string());
                }
                pid = Some(
                    args[i].parse::<u32>().map_err(|_| format!("invalid PID: {}", args[i]))?,
                );
            }
            "-a" => system_wide = true,
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("-o requires a filename".to_string());
                }
                output = args[i].clone();
            }
            "-h" | "--help" => {
                print_record_help();
                process::exit(0);
            }
            "--" => {
                command.extend_from_slice(&args[i + 1..]);
                break;
            }
            other => {
                if other.starts_with('-') {
                    return Err(format!("unknown option: {other}"));
                }
                command.extend_from_slice(&args[i..]);
                break;
            }
        }
        i += 1;
    }

    if pid.is_none() && !system_wide && command.is_empty() {
        return Err("specify a command, -p PID, or -a for system-wide".to_string());
    }

    Ok(RecordOpts {
        event,
        frequency,
        callgraph,
        callgraph_mode,
        pid,
        system_wide,
        output,
        command,
    })
}

fn print_record_help() {
    println!(
        "\
Usage: perf record [options] [<command>]

Options:
  -e, --event <event>        Event to record (default: cycles)
  -F, --freq <hz>            Sample frequency in Hz (default: 4000)
  -g                         Enable call-graph recording (default: fp)
  --call-graph <mode>        dwarf, fp, or lbr
  -p <pid>                   Record existing process
  -a                         System-wide recording
  -o, --output <file>        Output file (default: perf.data)
  -h, --help                 Show this help"
    );
}

/// Read sample data from `/proc/<pid>/perf_samples` or
/// `/sys/kernel/perf/samples`.
///
/// Expected format (one sample per line):
/// ```text
/// <timestamp_ns> <pid> <tid> <ip_hex> [<callchain_ip_hex> ...]
/// ```
fn read_raw_samples(target_pid: Option<u32>) -> Vec<PerfSample> {
    let path = match target_pid {
        Some(pid) => format!("/proc/{pid}/perf_samples"),
        None => "/sys/kernel/perf/samples".to_string(),
    };
    let mut samples = Vec::new();
    if let Ok(contents) = fs::read_to_string(&path) {
        for line in contents.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }
            let ts = match parts[0].parse::<u64>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let pid = match parts[1].parse::<u32>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let tid = match parts[2].parse::<u32>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let ip = match u64::from_str_radix(parts[3], 16) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let mut sample = PerfSample::new();
            sample.timestamp_ns = ts;
            sample.pid = pid;
            sample.tid = tid;
            sample.ip = ip;
            // Remaining entries are call-chain addresses.
            let chain_count = (parts.len() - 4).min(MAX_CALLCHAIN_DEPTH);
            sample.callchain_len = chain_count as u32;
            for j in 0..chain_count {
                if let Ok(addr) = u64::from_str_radix(parts[4 + j], 16) {
                    sample.callchain[j] = addr;
                }
            }
            samples.push(sample);
        }
    }
    samples
}

/// Write collected samples to a perf.data file.
fn write_perf_data(
    path: &str,
    event_name: &str,
    frequency: u32,
    has_callgraph: bool,
    samples: &[PerfSample],
) -> io::Result<()> {
    let data_size = samples.len() as u64 * PerfSample::DISK_SIZE;
    let header = PerfFileHeader {
        data_offset: PerfFileHeader::DISK_SIZE,
        data_size,
        sample_count: samples.len() as u64,
        event_name: event_name.to_string(),
        frequency,
        has_callgraph,
    };

    let mut file = File::create(path)?;
    header.write_to(&mut file)?;
    for sample in samples {
        sample.write_to(&mut file)?;
    }
    Ok(())
}

/// Read a perf.data file, returning (header, samples).
fn read_perf_data(path: &str) -> io::Result<(PerfFileHeader, Vec<PerfSample>)> {
    let mut file = File::open(path)?;
    let header = PerfFileHeader::read_from(&mut file)?;
    let count = header.sample_count;
    let mut samples = Vec::with_capacity(count as usize);
    for _ in 0..count {
        samples.push(PerfSample::read_from(&mut file)?);
    }
    Ok((header, samples))
}

fn run_record(args: &[String]) {
    let opts = match parse_record_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("perf record: {e}");
            process::exit(1);
        }
    };

    let target_pid = if opts.system_wide { None } else { opts.pid };
    let samples = read_raw_samples(target_pid);

    match write_perf_data(
        &opts.output,
        &opts.event,
        opts.frequency,
        opts.callgraph,
        &samples,
    ) {
        Ok(()) => {
            eprintln!(
                "[ perf record: Captured {} samples to {} ]",
                samples.len(),
                opts.output,
            );
        }
        Err(e) => {
            eprintln!("perf record: failed to write {}: {e}", opts.output);
            process::exit(1);
        }
    }
}

// ============================================================================
// perf report
// ============================================================================

/// Options for `perf report`.
#[derive(Debug)]
struct ReportOpts {
    input: String,
    /// Whether to use text output (always true for now; future TUI mode).
    #[allow(dead_code)]
    stdio: bool,
    sort_keys: Vec<String>,
    no_children: bool,
    percent_limit: f64,
}

fn parse_report_args(args: &[String]) -> Result<ReportOpts, String> {
    let mut input = "perf.data".to_string();
    // Currently always true — we only support text output.  The flag is
    // accepted for forward-compatibility with a future TUI mode.
    let stdio = true;
    let mut sort_keys: Vec<String> = Vec::new();
    let mut no_children = false;
    let mut percent_limit: f64 = 0.0;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-i" | "--input" => {
                i += 1;
                if i >= args.len() {
                    return Err("-i requires a filename".to_string());
                }
                input = args[i].clone();
            }
            "--stdio" => { /* already true; accepted for compat */ }
            "--sort" => {
                i += 1;
                if i >= args.len() {
                    return Err("--sort requires an argument".to_string());
                }
                for key in args[i].split(',') {
                    let key = key.trim().to_string();
                    match key.as_str() {
                        "comm" | "dso" | "symbol" => sort_keys.push(key),
                        other => return Err(format!("unknown sort key: {other}")),
                    }
                }
            }
            "--no-children" => no_children = true,
            "--percent-limit" => {
                i += 1;
                if i >= args.len() {
                    return Err("--percent-limit requires a value".to_string());
                }
                percent_limit = args[i]
                    .parse::<f64>()
                    .map_err(|_| format!("invalid percent-limit: {}", args[i]))?;
            }
            "-h" | "--help" => {
                print_report_help();
                process::exit(0);
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
        i += 1;
    }

    if sort_keys.is_empty() {
        sort_keys.push("symbol".to_string());
    }

    Ok(ReportOpts { input, stdio, sort_keys, no_children, percent_limit })
}

fn print_report_help() {
    println!(
        "\
Usage: perf report [options]

Options:
  -i, --input <file>         Input file (default: perf.data)
  --stdio                    Use text output (default)
  --sort <key,...>           Sort by: comm, dso, symbol
  --no-children              Show flat profile (no call-graph overhead)
  --percent-limit <pct>      Minimum overhead% to display
  -h, --help                 Show this help"
    );
}

/// Aggregate an overhead entry.
#[derive(Debug, Clone)]
struct OverheadEntry {
    symbol: String,
    dso: String,
    comm: String,
    count: u64,
}

fn run_report(args: &[String]) {
    let opts = match parse_report_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("perf report: {e}");
            process::exit(1);
        }
    };

    let (header, samples) = match read_perf_data(&opts.input) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("perf report: failed to read {}: {e}", opts.input);
            process::exit(1);
        }
    };

    if samples.is_empty() {
        println!("# No samples found in {}", opts.input);
        return;
    }

    // Collect unique PIDs so we can load symbol tables.
    let mut pid_set: Vec<u32> = samples.iter().map(|s| s.pid).collect();
    pid_set.sort_unstable();
    pid_set.dedup();
    let mut symbols_by_pid: HashMap<u32, Vec<SymbolEntry>> = HashMap::new();
    for &pid in &pid_set {
        symbols_by_pid.insert(pid, read_symbols(pid));
    }

    // Read process names from /proc/<pid>/comm.
    let mut comm_by_pid: HashMap<u32, String> = HashMap::new();
    for &pid in &pid_set {
        let comm_path = format!("/proc/{pid}/comm");
        let comm = fs::read_to_string(&comm_path)
            .unwrap_or_else(|_| format!("[pid:{pid}]"));
        comm_by_pid.insert(pid, comm.trim().to_string());
    }

    // Build overhead table.
    let total_samples = samples.len() as u64;
    let mut overhead_map: HashMap<String, OverheadEntry> = HashMap::new();

    for sample in &samples {
        let syms = symbols_by_pid
            .get(&sample.pid)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        let (sym_name, dso_name) = resolve_symbol_owned(sample.ip, syms);
        let comm = comm_by_pid
            .get(&sample.pid)
            .cloned()
            .unwrap_or_else(|| format!("[pid:{}]", sample.pid));

        // Build a sort key from the requested sort order.
        let mut sort_key = String::new();
        for k in &opts.sort_keys {
            if !sort_key.is_empty() {
                sort_key.push(':');
            }
            match k.as_str() {
                "comm" => sort_key.push_str(&comm),
                "dso" => sort_key.push_str(&dso_name),
                "symbol" => sort_key.push_str(&sym_name),
                _ => {}
            }
        }

        let entry = overhead_map.entry(sort_key.clone()).or_insert_with(|| {
            OverheadEntry {
                symbol: sym_name.clone(),
                dso: dso_name.clone(),
                comm: comm.clone(),
                count: 0,
            }
        });
        entry.count += 1;

        // If not --no-children, also attribute to callchain parents.
        if !opts.no_children && header.has_callgraph {
            let chain_len = (sample.callchain_len as usize).min(MAX_CALLCHAIN_DEPTH);
            for ci in 0..chain_len {
                let chain_ip = sample.callchain[ci];
                if chain_ip == 0 {
                    break;
                }
                let (csym, cdso) = resolve_symbol_owned(chain_ip, syms);
                let mut ckey = String::new();
                for k in &opts.sort_keys {
                    if !ckey.is_empty() {
                        ckey.push(':');
                    }
                    match k.as_str() {
                        "comm" => ckey.push_str(&comm),
                        "dso" => ckey.push_str(&cdso),
                        "symbol" => ckey.push_str(&csym),
                        _ => {}
                    }
                }
                // Only add to children overhead if it's a different symbol.
                if ckey != sort_key {
                    let centry = overhead_map.entry(ckey).or_insert_with(|| {
                        OverheadEntry {
                            symbol: csym,
                            dso: cdso,
                            comm: comm.clone(),
                            count: 0,
                        }
                    });
                    centry.count += 1;
                }
            }
        }
    }

    // Sort by overhead descending.
    let mut entries: Vec<OverheadEntry> = overhead_map.into_values().collect();
    entries.sort_by(|a, b| b.count.cmp(&a.count));

    // Print.
    println!(
        "# Event: {}, {} samples",
        header.event_name, total_samples,
    );
    println!("#");

    // Header line.
    print!("# Overhead");
    for k in &opts.sort_keys {
        match k.as_str() {
            "comm" => print!("  Command"),
            "dso" => print!("  Shared Object"),
            "symbol" => print!("  Symbol"),
            _ => {}
        }
    }
    println!();

    print!("# ........");
    for k in &opts.sort_keys {
        match k.as_str() {
            "comm" => print!("  ......."),
            "dso" => print!("  .............."),
            "symbol" => print!("  ......"),
            _ => {}
        }
    }
    println!();

    for entry in &entries {
        let pct = if total_samples > 0 {
            entry.count as f64 / total_samples as f64 * 100.0
        } else {
            0.0
        };
        if pct < opts.percent_limit {
            continue;
        }
        print!("  {:>6.2}%", pct);
        for k in &opts.sort_keys {
            match k.as_str() {
                "comm" => print!("  {:<16}", entry.comm),
                "dso" => print!("  {:<24}", entry.dso),
                "symbol" => print!("  {}", entry.symbol),
                _ => {}
            }
        }
        println!();
    }
}

// ============================================================================
// perf top
// ============================================================================

/// Options for `perf top`.
#[derive(Debug)]
struct TopOpts {
    event: String,
    pid: Option<u32>,
    count: u32,
}

fn parse_top_args(args: &[String]) -> Result<TopOpts, String> {
    let mut event = "cycles".to_string();
    let mut pid: Option<u32> = None;
    let mut count: u32 = 20;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-e" | "--event" => {
                i += 1;
                if i >= args.len() {
                    return Err("-e requires an argument".to_string());
                }
                if find_event(&args[i]).is_none() {
                    return Err(format!("unknown event: {}", args[i]));
                }
                event = args[i].clone();
            }
            "-p" => {
                i += 1;
                if i >= args.len() {
                    return Err("-p requires a PID".to_string());
                }
                pid = Some(
                    args[i].parse::<u32>().map_err(|_| format!("invalid PID: {}", args[i]))?,
                );
            }
            "-n" | "--count" => {
                i += 1;
                if i >= args.len() {
                    return Err("-n requires a count".to_string());
                }
                count = args[i]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid count: {}", args[i]))?;
            }
            "-h" | "--help" => {
                print_top_help();
                process::exit(0);
            }
            other => {
                return Err(format!("unknown option: {other}"));
            }
        }
        i += 1;
    }

    Ok(TopOpts { event, pid, count })
}

fn print_top_help() {
    println!(
        "\
Usage: perf top [options]

Options:
  -e, --event <event>  Event to monitor (default: cycles)
  -p <pid>             Monitor specific process
  -n, --count <n>      Number of entries to show (default: 20)
  -h, --help           Show this help"
    );
}

/// A single line from the kernel's real-time profiling output.
#[derive(Debug, Clone)]
struct TopEntry {
    overhead_pct: f64,
    symbol: String,
    dso: String,
    pid: u32,
}

/// Read top entries from `/proc/perf_top` or `/proc/<pid>/perf_top`.
///
/// Expected format:
/// ```text
/// <overhead_pct> <pid> <symbol> <dso>
/// ```
fn read_top_entries(pid: Option<u32>) -> Vec<TopEntry> {
    let path = match pid {
        Some(p) => format!("/proc/{p}/perf_top"),
        None => "/proc/perf_top".to_string(),
    };
    let mut entries = Vec::new();
    if let Ok(contents) = fs::read_to_string(&path) {
        for line in contents.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }
            let pct = match parts[0].parse::<f64>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let pid = match parts[1].parse::<u32>() {
                Ok(v) => v,
                Err(_) => continue,
            };
            entries.push(TopEntry {
                overhead_pct: pct,
                symbol: parts[2].to_string(),
                dso: parts[3].to_string(),
                pid,
            });
        }
    }
    entries
}

fn run_top(args: &[String]) {
    let opts = match parse_top_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("perf top: {e}");
            process::exit(1);
        }
    };

    let mut entries = read_top_entries(opts.pid);
    entries.sort_by(|a, b| b.overhead_pct.partial_cmp(&a.overhead_pct).unwrap_or(std::cmp::Ordering::Equal));
    entries.truncate(opts.count as usize);

    println!(
        "PerfTop — event: {}, showing top {} functions",
        opts.event,
        opts.count,
    );
    println!();
    println!(
        "{:>8}  {:>6}  {:<30}  Shared Object",
        "Overhead", "PID", "Symbol",
    );
    println!(
        "{:>8}  {:>6}  {:<30}  -------------",
        "--------", "------", "------------------------------",
    );

    if entries.is_empty() {
        println!("  (no data — /proc/perf_top not available)");
    } else {
        for entry in &entries {
            println!(
                "{:>7.2}%  {:>6}  {:<30}  {}",
                entry.overhead_pct, entry.pid, entry.symbol, entry.dso,
            );
        }
    }
}

// ============================================================================
// Main dispatcher
// ============================================================================

fn print_main_help() {
    println!(
        "\
Usage: perf <command> [options]

Commands:
  stat     Collect performance counter statistics
  record   Record performance events to perf.data
  report   Analyze recorded perf.data
  top      Real-time function profiling
  list     List available events
  version  Show version

Run 'perf <command> --help' for command-specific options."
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let personality = {
        let argv0 = args.first().map(|s| s.as_str()).unwrap_or("perf");
        detect_personality(argv0)
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    match personality {
        Personality::Stat => run_stat(&rest),
        Personality::Record => run_record(&rest),
        Personality::Report => run_report(&rest),
        Personality::Top => run_top(&rest),
        Personality::Perf => {
            // Dispatcher: first arg is the subcommand.
            if rest.is_empty() {
                print_main_help();
                return;
            }
            match rest[0].as_str() {
                "stat" => run_stat(&rest[1..]),
                "record" => run_record(&rest[1..]),
                "report" => run_report(&rest[1..]),
                "top" => run_top(&rest[1..]),
                "list" => cmd_list(),
                "version" | "--version" => cmd_version(),
                "-h" | "--help" | "help" => print_main_help(),
                other => {
                    eprintln!("perf: unknown command '{other}'");
                    eprintln!("Run 'perf --help' for available commands.");
                    process::exit(1);
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ----------------------------------------------------------------
    // Personality detection
    // ----------------------------------------------------------------

    #[test]
    fn personality_bare_perf() {
        assert_eq!(detect_personality("perf"), Personality::Perf);
    }

    #[test]
    fn personality_perf_stat() {
        assert_eq!(detect_personality("perf-stat"), Personality::Stat);
    }

    #[test]
    fn personality_perf_record() {
        assert_eq!(detect_personality("perf-record"), Personality::Record);
    }

    #[test]
    fn personality_perf_report() {
        assert_eq!(detect_personality("perf-report"), Personality::Report);
    }

    #[test]
    fn personality_perf_top() {
        assert_eq!(detect_personality("perf-top"), Personality::Top);
    }

    #[test]
    fn personality_with_path_unix() {
        assert_eq!(detect_personality("/usr/bin/perf-stat"), Personality::Stat);
    }

    #[test]
    fn personality_with_path_windows() {
        assert_eq!(detect_personality("C:\\bin\\perf-record.exe"), Personality::Record);
    }

    #[test]
    fn personality_exe_suffix() {
        assert_eq!(detect_personality("perf-top.exe"), Personality::Top);
    }

    #[test]
    fn personality_unknown_defaults_to_perf() {
        assert_eq!(detect_personality("whatever"), Personality::Perf);
    }

    #[test]
    fn personality_empty_string() {
        assert_eq!(detect_personality(""), Personality::Perf);
    }

    #[test]
    fn personality_nested_path() {
        assert_eq!(detect_personality("/a/b/c/perf-report"), Personality::Report);
    }

    #[test]
    fn personality_trailing_slash() {
        // Unusual but should not panic.
        assert_eq!(detect_personality("/usr/bin/"), Personality::Perf);
    }

    // ----------------------------------------------------------------
    // Event catalogue
    // ----------------------------------------------------------------

    #[test]
    fn find_event_cycles() {
        let ev = find_event("cycles");
        assert!(ev.is_some());
        assert_eq!(ev.unwrap().name, "cycles");
        assert_eq!(ev.unwrap().kind, EventKind::Hardware);
    }

    #[test]
    fn find_event_case_insensitive() {
        // find_event lowercases the input, so uppercase names match.
        assert!(find_event("CYCLES").is_some());
        assert!(find_event("Cycles").is_some());
        assert!(find_event("cycles").is_some());
    }

    #[test]
    fn find_event_instructions() {
        let ev = find_event("instructions").unwrap();
        assert_eq!(ev.kind, EventKind::Hardware);
    }

    #[test]
    fn find_event_cache_references() {
        let ev = find_event("cache-references").unwrap();
        assert_eq!(ev.kind, EventKind::Hardware);
    }

    #[test]
    fn find_event_cache_misses() {
        assert!(find_event("cache-misses").is_some());
    }

    #[test]
    fn find_event_branch_instructions() {
        assert!(find_event("branch-instructions").is_some());
    }

    #[test]
    fn find_event_branch_misses() {
        assert!(find_event("branch-misses").is_some());
    }

    #[test]
    fn find_event_bus_cycles() {
        assert!(find_event("bus-cycles").is_some());
    }

    #[test]
    fn find_event_ref_cycles() {
        assert!(find_event("ref-cycles").is_some());
    }

    #[test]
    fn find_event_task_clock() {
        let ev = find_event("task-clock").unwrap();
        assert_eq!(ev.kind, EventKind::Software);
    }

    #[test]
    fn find_event_context_switches() {
        assert!(find_event("context-switches").is_some());
    }

    #[test]
    fn find_event_cpu_migrations() {
        assert!(find_event("cpu-migrations").is_some());
    }

    #[test]
    fn find_event_page_faults() {
        assert!(find_event("page-faults").is_some());
    }

    #[test]
    fn find_event_minor_faults() {
        assert!(find_event("minor-faults").is_some());
    }

    #[test]
    fn find_event_major_faults() {
        assert!(find_event("major-faults").is_some());
    }

    #[test]
    fn find_event_nonexistent() {
        assert!(find_event("nonexistent").is_none());
    }

    #[test]
    fn default_stat_events_returns_all() {
        let all = default_stat_events();
        assert_eq!(all.len(), EVENTS.len());
    }

    #[test]
    fn event_catalogue_has_8_hardware() {
        let hw_count = EVENTS.iter().filter(|e| e.kind == EventKind::Hardware).count();
        assert_eq!(hw_count, 8);
    }

    #[test]
    fn event_catalogue_has_6_software() {
        let sw_count = EVENTS.iter().filter(|e| e.kind == EventKind::Software).count();
        assert_eq!(sw_count, 6);
    }

    #[test]
    fn event_names_unique() {
        let mut names: Vec<&str> = EVENTS.iter().map(|e| e.name).collect();
        let original_len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), original_len);
    }

    // ----------------------------------------------------------------
    // Number formatting
    // ----------------------------------------------------------------

    #[test]
    fn format_number_zero() {
        assert_eq!(format_number(0), "0");
    }

    #[test]
    fn format_number_small() {
        assert_eq!(format_number(42), "42");
    }

    #[test]
    fn format_number_hundreds() {
        assert_eq!(format_number(999), "999");
    }

    #[test]
    fn format_number_thousands() {
        assert_eq!(format_number(1000), "1,000");
    }

    #[test]
    fn format_number_millions() {
        assert_eq!(format_number(1_234_567), "1,234,567");
    }

    #[test]
    fn format_number_billions() {
        assert_eq!(format_number(1_000_000_000), "1,000,000,000");
    }

    #[test]
    fn format_number_ten_thousand() {
        assert_eq!(format_number(12345), "12,345");
    }

    #[test]
    fn format_number_exact_boundary() {
        assert_eq!(format_number(100_000), "100,000");
    }

    // ----------------------------------------------------------------
    // Statistics helpers
    // ----------------------------------------------------------------

    #[test]
    fn mean_stddev_empty() {
        let (m, s) = mean_stddev(&[]);
        assert_eq!(m, 0.0);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn mean_stddev_single() {
        let (m, s) = mean_stddev(&[42.0]);
        assert_eq!(m, 42.0);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn mean_stddev_uniform() {
        let (m, s) = mean_stddev(&[5.0, 5.0, 5.0, 5.0]);
        assert_eq!(m, 5.0);
        assert_eq!(s, 0.0);
    }

    #[test]
    fn mean_stddev_known_values() {
        // Values: 2, 4, 4, 4, 5, 5, 7, 9
        // Mean = 5.0, sample stddev ~ 2.138
        let vals = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let (m, s) = mean_stddev(&vals);
        assert!((m - 5.0).abs() < 0.001);
        assert!((s - 2.138).abs() < 0.01);
    }

    #[test]
    fn mean_stddev_two_values() {
        let (m, s) = mean_stddev(&[10.0, 20.0]);
        assert!((m - 15.0).abs() < 0.001);
        // Sample stddev of [10,20] = sqrt((25+25)/1) = sqrt(50) ~ 7.071
        assert!((s - 7.071).abs() < 0.01);
    }

    // ----------------------------------------------------------------
    // PerfSample serialization
    // ----------------------------------------------------------------

    #[test]
    fn sample_new_is_zeroed() {
        let s = PerfSample::new();
        assert_eq!(s.timestamp_ns, 0);
        assert_eq!(s.pid, 0);
        assert_eq!(s.tid, 0);
        assert_eq!(s.ip, 0);
        assert_eq!(s.callchain_len, 0);
        for &addr in &s.callchain {
            assert_eq!(addr, 0);
        }
    }

    #[test]
    fn sample_disk_size_is_160() {
        assert_eq!(PerfSample::DISK_SIZE, 160);
    }

    #[test]
    fn sample_round_trip() {
        let mut s = PerfSample::new();
        s.timestamp_ns = 123_456_789;
        s.pid = 42;
        s.tid = 43;
        s.ip = 0xDEADBEEF;
        s.callchain_len = 3;
        s.callchain[0] = 0xAAAA;
        s.callchain[1] = 0xBBBB;
        s.callchain[2] = 0xCCCC;

        let mut buf = Vec::new();
        s.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), PerfSample::DISK_SIZE as usize);

        let mut cursor = Cursor::new(buf);
        let s2 = PerfSample::read_from(&mut cursor).unwrap();
        assert_eq!(s2.timestamp_ns, 123_456_789);
        assert_eq!(s2.pid, 42);
        assert_eq!(s2.tid, 43);
        assert_eq!(s2.ip, 0xDEADBEEF);
        assert_eq!(s2.callchain_len, 3);
        assert_eq!(s2.callchain[0], 0xAAAA);
        assert_eq!(s2.callchain[1], 0xBBBB);
        assert_eq!(s2.callchain[2], 0xCCCC);
    }

    #[test]
    fn sample_max_callchain() {
        let mut s = PerfSample::new();
        s.callchain_len = MAX_CALLCHAIN_DEPTH as u32;
        for (i, slot) in s.callchain.iter_mut().enumerate() {
            *slot = (i as u64 + 1) * 0x1000;
        }
        let mut buf = Vec::new();
        s.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let s2 = PerfSample::read_from(&mut cursor).unwrap();
        assert_eq!(s2.callchain_len, MAX_CALLCHAIN_DEPTH as u32);
        for i in 0..MAX_CALLCHAIN_DEPTH {
            assert_eq!(s2.callchain[i], (i as u64 + 1) * 0x1000);
        }
    }

    #[test]
    fn sample_write_read_preserves_all_fields() {
        let mut s = PerfSample::new();
        s.timestamp_ns = u64::MAX;
        s.pid = u32::MAX;
        s.tid = u32::MAX;
        s.ip = u64::MAX;
        s.callchain_len = 0;
        let mut buf = Vec::new();
        s.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let s2 = PerfSample::read_from(&mut cursor).unwrap();
        assert_eq!(s2.timestamp_ns, u64::MAX);
        assert_eq!(s2.pid, u32::MAX);
        assert_eq!(s2.tid, u32::MAX);
        assert_eq!(s2.ip, u64::MAX);
    }

    // ----------------------------------------------------------------
    // PerfFileHeader serialization
    // ----------------------------------------------------------------

    #[test]
    fn header_disk_size_is_108() {
        assert_eq!(PerfFileHeader::DISK_SIZE, 108);
    }

    #[test]
    fn header_round_trip() {
        let hdr = PerfFileHeader {
            data_offset: 108,
            data_size: 320,
            sample_count: 2,
            event_name: "cycles".to_string(),
            frequency: 4000,
            has_callgraph: true,
        };
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), PerfFileHeader::DISK_SIZE as usize);

        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr2.data_offset, 108);
        assert_eq!(hdr2.data_size, 320);
        assert_eq!(hdr2.sample_count, 2);
        assert_eq!(hdr2.event_name, "cycles");
        assert_eq!(hdr2.frequency, 4000);
        assert!(hdr2.has_callgraph);
    }

    #[test]
    fn header_round_trip_no_callgraph() {
        let hdr = PerfFileHeader {
            data_offset: 108,
            data_size: 0,
            sample_count: 0,
            event_name: "instructions".to_string(),
            frequency: 1000,
            has_callgraph: false,
        };
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr2.event_name, "instructions");
        assert_eq!(hdr2.frequency, 1000);
        assert!(!hdr2.has_callgraph);
    }

    #[test]
    fn header_long_event_name_truncated() {
        let hdr = PerfFileHeader {
            data_offset: 108,
            data_size: 0,
            sample_count: 0,
            event_name: "a".repeat(100),
            frequency: 99,
            has_callgraph: false,
        };
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        // Truncated to 63 characters.
        assert_eq!(hdr2.event_name.len(), 63);
    }

    #[test]
    fn header_bad_magic_rejected() {
        let mut buf = vec![0u8; 108];
        buf[..8].copy_from_slice(b"BADMAGIC");
        let mut cursor = Cursor::new(buf);
        let result = PerfFileHeader::read_from(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn header_bad_version_rejected() {
        let hdr = PerfFileHeader {
            data_offset: 108,
            data_size: 0,
            sample_count: 0,
            event_name: "x".to_string(),
            frequency: 1,
            has_callgraph: false,
        };
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        // Corrupt the version field (bytes 8..12).
        buf[8] = 99;
        let mut cursor = Cursor::new(buf);
        let result = PerfFileHeader::read_from(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn header_empty_event_name() {
        let hdr = PerfFileHeader {
            data_offset: 108,
            data_size: 0,
            sample_count: 0,
            event_name: String::new(),
            frequency: 1,
            has_callgraph: false,
        };
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr2.event_name, "");
    }

    // ----------------------------------------------------------------
    // Full perf.data round-trip
    // ----------------------------------------------------------------

    #[test]
    fn perf_data_round_trip_empty() {
        let mut buf = Vec::new();
        let hdr = PerfFileHeader {
            data_offset: PerfFileHeader::DISK_SIZE,
            data_size: 0,
            sample_count: 0,
            event_name: "cycles".to_string(),
            frequency: 4000,
            has_callgraph: false,
        };
        hdr.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr2.sample_count, 0);
    }

    #[test]
    fn perf_data_round_trip_multiple_samples() {
        let mut buf = Vec::new();
        let samples: Vec<PerfSample> = (0..5)
            .map(|i| {
                let mut s = PerfSample::new();
                s.timestamp_ns = i * 1000;
                s.pid = 100 + i as u32;
                s.tid = 200 + i as u32;
                s.ip = 0x40_0000 + i * 0x100;
                s
            })
            .collect();
        let hdr = PerfFileHeader {
            data_offset: PerfFileHeader::DISK_SIZE,
            data_size: 5 * PerfSample::DISK_SIZE,
            sample_count: 5,
            event_name: "instructions".to_string(),
            frequency: 1000,
            has_callgraph: false,
        };
        hdr.write_to(&mut buf).unwrap();
        for s in &samples {
            s.write_to(&mut buf).unwrap();
        }
        let expected_size = PerfFileHeader::DISK_SIZE as usize + 5 * PerfSample::DISK_SIZE as usize;
        assert_eq!(buf.len(), expected_size);

        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr2.sample_count, 5);
        for i in 0..5u64 {
            let s = PerfSample::read_from(&mut cursor).unwrap();
            assert_eq!(s.timestamp_ns, i * 1000);
            assert_eq!(s.pid, 100 + i as u32);
        }
    }

    // ----------------------------------------------------------------
    // Symbol resolution
    // ----------------------------------------------------------------

    #[test]
    fn resolve_symbol_found() {
        let syms = vec![
            SymbolEntry { start: 0x1000, end: 0x2000, name: "foo".to_string(), dso: "libc.so".to_string() },
            SymbolEntry { start: 0x2000, end: 0x3000, name: "bar".to_string(), dso: "libm.so".to_string() },
        ];
        let (name, dso) = resolve_symbol(0x1500, &syms);
        assert_eq!(name, "foo");
        assert_eq!(dso, "libc.so");
    }

    #[test]
    fn resolve_symbol_second_entry() {
        let syms = vec![
            SymbolEntry { start: 0x1000, end: 0x2000, name: "foo".to_string(), dso: "a.so".to_string() },
            SymbolEntry { start: 0x2000, end: 0x3000, name: "bar".to_string(), dso: "b.so".to_string() },
        ];
        let (name, dso) = resolve_symbol(0x2500, &syms);
        assert_eq!(name, "bar");
        assert_eq!(dso, "b.so");
    }

    #[test]
    fn resolve_symbol_not_found() {
        let syms = vec![
            SymbolEntry { start: 0x1000, end: 0x2000, name: "foo".to_string(), dso: "a.so".to_string() },
        ];
        let (name, dso) = resolve_symbol(0x9999, &syms);
        assert_eq!(name, "[unknown]");
        assert_eq!(dso, "[unknown]");
    }

    #[test]
    fn resolve_symbol_empty_table() {
        let (name, dso) = resolve_symbol(0x1000, &[]);
        assert_eq!(name, "[unknown]");
        assert_eq!(dso, "[unknown]");
    }

    #[test]
    fn resolve_symbol_exact_start() {
        let syms = vec![
            SymbolEntry { start: 0x1000, end: 0x2000, name: "exact".to_string(), dso: "x".to_string() },
        ];
        let (name, _) = resolve_symbol(0x1000, &syms);
        assert_eq!(name, "exact");
    }

    #[test]
    fn resolve_symbol_one_before_end() {
        let syms = vec![
            SymbolEntry { start: 0x1000, end: 0x2000, name: "last".to_string(), dso: "x".to_string() },
        ];
        let (name, _) = resolve_symbol(0x1FFF, &syms);
        assert_eq!(name, "last");
    }

    #[test]
    fn resolve_symbol_at_end_is_miss() {
        let syms = vec![
            SymbolEntry { start: 0x1000, end: 0x2000, name: "miss".to_string(), dso: "x".to_string() },
        ];
        let (name, _) = resolve_symbol(0x2000, &syms);
        assert_eq!(name, "[unknown]");
    }

    #[test]
    fn resolve_symbol_owned_returns_owned_strings() {
        let syms = vec![
            SymbolEntry { start: 0x1000, end: 0x2000, name: "own".to_string(), dso: "d.so".to_string() },
        ];
        let (name, dso) = resolve_symbol_owned(0x1500, &syms);
        assert_eq!(name, "own");
        assert_eq!(dso, "d.so");
    }

    #[test]
    fn resolve_symbol_owned_unknown() {
        let (name, dso) = resolve_symbol_owned(0x9999, &[]);
        assert_eq!(name, "[unknown]");
        assert_eq!(dso, "[unknown]");
    }

    // ----------------------------------------------------------------
    // Stat argument parsing
    // ----------------------------------------------------------------

    #[test]
    fn stat_parse_system_wide() {
        let args: Vec<String> = vec!["-a".to_string()];
        let opts = parse_stat_args(&args).unwrap();
        assert!(opts.system_wide);
        assert!(opts.pid.is_none());
    }

    #[test]
    fn stat_parse_pid() {
        let args: Vec<String> = vec!["-p".to_string(), "123".to_string()];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.pid, Some(123));
    }

    #[test]
    fn stat_parse_repeat() {
        let args: Vec<String> = vec!["-a".to_string(), "-r".to_string(), "5".to_string()];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.repeat, 5);
    }

    #[test]
    fn stat_parse_single_event() {
        let args: Vec<String> = vec![
            "-e".to_string(), "cycles".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.events.len(), 1);
        assert_eq!(opts.events[0].name, "cycles");
    }

    #[test]
    fn stat_parse_multiple_events() {
        let args: Vec<String> = vec![
            "-e".to_string(), "cycles,instructions,cache-misses".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.events.len(), 3);
    }

    #[test]
    fn stat_parse_command() {
        let args: Vec<String> = vec!["ls".to_string(), "-la".to_string()];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.command, vec!["ls", "-la"]);
    }

    #[test]
    fn stat_parse_command_after_double_dash() {
        let args: Vec<String> = vec![
            "-a".to_string(), "--".to_string(), "ls".to_string(),
        ];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.command, vec!["ls"]);
    }

    #[test]
    fn stat_parse_default_events() {
        let args: Vec<String> = vec!["-a".to_string()];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.events.len(), EVENTS.len());
    }

    #[test]
    fn stat_parse_unknown_event_fails() {
        let args: Vec<String> = vec!["-e".to_string(), "bogus".to_string(), "-a".to_string()];
        assert!(parse_stat_args(&args).is_err());
    }

    #[test]
    fn stat_parse_missing_pid_arg() {
        let args: Vec<String> = vec!["-p".to_string()];
        assert!(parse_stat_args(&args).is_err());
    }

    #[test]
    fn stat_parse_invalid_pid() {
        let args: Vec<String> = vec!["-p".to_string(), "abc".to_string()];
        assert!(parse_stat_args(&args).is_err());
    }

    #[test]
    fn stat_parse_missing_repeat_arg() {
        let args: Vec<String> = vec!["-r".to_string()];
        assert!(parse_stat_args(&args).is_err());
    }

    #[test]
    fn stat_parse_zero_repeat_fails() {
        let args: Vec<String> = vec!["-r".to_string(), "0".to_string(), "-a".to_string()];
        assert!(parse_stat_args(&args).is_err());
    }

    #[test]
    fn stat_parse_missing_event_arg() {
        let args: Vec<String> = vec!["-e".to_string()];
        assert!(parse_stat_args(&args).is_err());
    }

    #[test]
    fn stat_parse_no_target_fails() {
        let args: Vec<String> = vec!["-e".to_string(), "cycles".to_string()];
        assert!(parse_stat_args(&args).is_err());
    }

    #[test]
    fn stat_parse_unknown_option_fails() {
        let args: Vec<String> = vec!["--bogus".to_string(), "-a".to_string()];
        assert!(parse_stat_args(&args).is_err());
    }

    #[test]
    fn stat_default_repeat_is_1() {
        let args: Vec<String> = vec!["-a".to_string()];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.repeat, 1);
    }

    // ----------------------------------------------------------------
    // Record argument parsing
    // ----------------------------------------------------------------

    #[test]
    fn record_parse_system_wide() {
        let args: Vec<String> = vec!["-a".to_string()];
        let opts = parse_record_args(&args).unwrap();
        assert!(opts.system_wide);
    }

    #[test]
    fn record_parse_pid() {
        let args: Vec<String> = vec!["-p".to_string(), "42".to_string()];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.pid, Some(42));
    }

    #[test]
    fn record_parse_event() {
        let args: Vec<String> = vec![
            "-e".to_string(), "instructions".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.event, "instructions");
    }

    #[test]
    fn record_parse_frequency() {
        let args: Vec<String> = vec![
            "-F".to_string(), "999".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.frequency, 999);
    }

    #[test]
    fn record_parse_callgraph_short() {
        let args: Vec<String> = vec!["-g".to_string(), "-a".to_string()];
        let opts = parse_record_args(&args).unwrap();
        assert!(opts.callgraph);
        assert_eq!(opts.callgraph_mode, "fp");
    }

    #[test]
    fn record_parse_callgraph_dwarf() {
        let args: Vec<String> = vec![
            "--call-graph".to_string(), "dwarf".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_record_args(&args).unwrap();
        assert!(opts.callgraph);
        assert_eq!(opts.callgraph_mode, "dwarf");
    }

    #[test]
    fn record_parse_callgraph_lbr() {
        let args: Vec<String> = vec![
            "--call-graph".to_string(), "lbr".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.callgraph_mode, "lbr");
    }

    #[test]
    fn record_parse_output_file() {
        let args: Vec<String> = vec![
            "-o".to_string(), "my.data".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.output, "my.data");
    }

    #[test]
    fn record_default_output() {
        let args: Vec<String> = vec!["-a".to_string()];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.output, "perf.data");
    }

    #[test]
    fn record_default_event() {
        let args: Vec<String> = vec!["-a".to_string()];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.event, "cycles");
    }

    #[test]
    fn record_default_frequency() {
        let args: Vec<String> = vec!["-a".to_string()];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.frequency, 4000);
    }

    #[test]
    fn record_parse_unknown_event_fails() {
        let args: Vec<String> = vec!["-e".to_string(), "bogus".to_string(), "-a".to_string()];
        assert!(parse_record_args(&args).is_err());
    }

    #[test]
    fn record_parse_bad_callgraph_mode() {
        let args: Vec<String> = vec![
            "--call-graph".to_string(), "none".to_string(),
            "-a".to_string(),
        ];
        assert!(parse_record_args(&args).is_err());
    }

    #[test]
    fn record_parse_no_target_fails() {
        let args: Vec<String> = vec!["-e".to_string(), "cycles".to_string()];
        assert!(parse_record_args(&args).is_err());
    }

    #[test]
    fn record_parse_command() {
        let args: Vec<String> = vec!["sleep".to_string(), "1".to_string()];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.command, vec!["sleep", "1"]);
    }

    #[test]
    fn record_parse_missing_event_arg() {
        let args: Vec<String> = vec!["-e".to_string()];
        assert!(parse_record_args(&args).is_err());
    }

    #[test]
    fn record_parse_missing_freq_arg() {
        let args: Vec<String> = vec!["-F".to_string()];
        assert!(parse_record_args(&args).is_err());
    }

    #[test]
    fn record_parse_missing_output_arg() {
        let args: Vec<String> = vec!["-o".to_string()];
        assert!(parse_record_args(&args).is_err());
    }

    #[test]
    fn record_parse_missing_callgraph_mode() {
        let args: Vec<String> = vec!["--call-graph".to_string()];
        assert!(parse_record_args(&args).is_err());
    }

    #[test]
    fn record_parse_missing_pid_arg() {
        let args: Vec<String> = vec!["-p".to_string()];
        assert!(parse_record_args(&args).is_err());
    }

    // ----------------------------------------------------------------
    // Report argument parsing
    // ----------------------------------------------------------------

    #[test]
    fn report_parse_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_report_args(&args).unwrap();
        assert_eq!(opts.input, "perf.data");
        assert!(opts.stdio);
        assert_eq!(opts.sort_keys, vec!["symbol"]);
        assert_eq!(opts.percent_limit, 0.0);
    }

    #[test]
    fn report_parse_input() {
        let args: Vec<String> = vec!["-i".to_string(), "other.data".to_string()];
        let opts = parse_report_args(&args).unwrap();
        assert_eq!(opts.input, "other.data");
    }

    #[test]
    fn report_parse_sort_comm() {
        let args: Vec<String> = vec!["--sort".to_string(), "comm".to_string()];
        let opts = parse_report_args(&args).unwrap();
        assert_eq!(opts.sort_keys, vec!["comm"]);
    }

    #[test]
    fn report_parse_sort_multiple() {
        let args: Vec<String> = vec!["--sort".to_string(), "comm,dso,symbol".to_string()];
        let opts = parse_report_args(&args).unwrap();
        assert_eq!(opts.sort_keys, vec!["comm", "dso", "symbol"]);
    }

    #[test]
    fn report_parse_no_children() {
        let args: Vec<String> = vec!["--no-children".to_string()];
        let opts = parse_report_args(&args).unwrap();
        assert!(opts.no_children);
    }

    #[test]
    fn report_parse_percent_limit() {
        let args: Vec<String> = vec!["--percent-limit".to_string(), "1.5".to_string()];
        let opts = parse_report_args(&args).unwrap();
        assert!((opts.percent_limit - 1.5).abs() < 0.001);
    }

    #[test]
    fn report_parse_bad_sort_key() {
        let args: Vec<String> = vec!["--sort".to_string(), "bogus".to_string()];
        assert!(parse_report_args(&args).is_err());
    }

    #[test]
    fn report_parse_missing_input_arg() {
        let args: Vec<String> = vec!["-i".to_string()];
        assert!(parse_report_args(&args).is_err());
    }

    #[test]
    fn report_parse_missing_sort_arg() {
        let args: Vec<String> = vec!["--sort".to_string()];
        assert!(parse_report_args(&args).is_err());
    }

    #[test]
    fn report_parse_missing_percent_arg() {
        let args: Vec<String> = vec!["--percent-limit".to_string()];
        assert!(parse_report_args(&args).is_err());
    }

    #[test]
    fn report_parse_unknown_option() {
        let args: Vec<String> = vec!["--bogus".to_string()];
        assert!(parse_report_args(&args).is_err());
    }

    // ----------------------------------------------------------------
    // Top argument parsing
    // ----------------------------------------------------------------

    #[test]
    fn top_parse_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_top_args(&args).unwrap();
        assert_eq!(opts.event, "cycles");
        assert!(opts.pid.is_none());
        assert_eq!(opts.count, 20);
    }

    #[test]
    fn top_parse_event() {
        let args: Vec<String> = vec!["-e".to_string(), "instructions".to_string()];
        let opts = parse_top_args(&args).unwrap();
        assert_eq!(opts.event, "instructions");
    }

    #[test]
    fn top_parse_pid() {
        let args: Vec<String> = vec!["-p".to_string(), "99".to_string()];
        let opts = parse_top_args(&args).unwrap();
        assert_eq!(opts.pid, Some(99));
    }

    #[test]
    fn top_parse_count() {
        let args: Vec<String> = vec!["-n".to_string(), "50".to_string()];
        let opts = parse_top_args(&args).unwrap();
        assert_eq!(opts.count, 50);
    }

    #[test]
    fn top_parse_unknown_event_fails() {
        let args: Vec<String> = vec!["-e".to_string(), "bogus".to_string()];
        assert!(parse_top_args(&args).is_err());
    }

    #[test]
    fn top_parse_missing_event_arg() {
        let args: Vec<String> = vec!["-e".to_string()];
        assert!(parse_top_args(&args).is_err());
    }

    #[test]
    fn top_parse_missing_pid_arg() {
        let args: Vec<String> = vec!["-p".to_string()];
        assert!(parse_top_args(&args).is_err());
    }

    #[test]
    fn top_parse_missing_count_arg() {
        let args: Vec<String> = vec!["-n".to_string()];
        assert!(parse_top_args(&args).is_err());
    }

    #[test]
    fn top_parse_unknown_option() {
        let args: Vec<String> = vec!["--bogus".to_string()];
        assert!(parse_top_args(&args).is_err());
    }

    // ----------------------------------------------------------------
    // Counter reading (graceful defaults when /proc is absent)
    // ----------------------------------------------------------------

    #[test]
    fn read_u64_file_missing_returns_zero() {
        assert_eq!(read_u64_file("/nonexistent/path/to/file"), 0);
    }

    #[test]
    fn read_proc_perf_events_missing_returns_empty() {
        let map = read_proc_perf_events(999_999);
        assert!(map.is_empty());
    }

    #[test]
    fn read_sys_counter_missing_returns_zero() {
        assert_eq!(read_sys_counter("nonexistent/counter"), 0);
    }

    #[test]
    fn read_counter_missing_proc_returns_zero() {
        let ev = find_event("cycles").unwrap();
        assert_eq!(read_counter(ev, Some(999_999)), 0);
    }

    #[test]
    fn read_counter_missing_sys_returns_zero() {
        let ev = find_event("cycles").unwrap();
        assert_eq!(read_counter(ev, None), 0);
    }

    #[test]
    fn read_raw_samples_missing_returns_empty() {
        let samples = read_raw_samples(Some(999_999));
        assert!(samples.is_empty());
    }

    #[test]
    fn read_raw_samples_system_wide_missing() {
        let samples = read_raw_samples(None);
        assert!(samples.is_empty());
    }

    #[test]
    fn read_symbols_missing_returns_empty() {
        let syms = read_symbols(999_999);
        assert!(syms.is_empty());
    }

    #[test]
    fn read_top_entries_missing_returns_empty() {
        let entries = read_top_entries(None);
        assert!(entries.is_empty());
    }

    #[test]
    fn read_top_entries_pid_missing_returns_empty() {
        let entries = read_top_entries(Some(999_999));
        assert!(entries.is_empty());
    }

    // ----------------------------------------------------------------
    // stat_collect_once with missing data
    // ----------------------------------------------------------------

    #[test]
    fn stat_collect_once_system_wide_graceful() {
        let opts = StatOpts {
            events: default_stat_events(),
            system_wide: true,
            pid: None,
            repeat: 1,
            command: vec![],
        };
        let vals = stat_collect_once(&opts);
        // All values should be 0 since /sys/kernel/perf/ doesn't exist.
        for ev in &opts.events {
            assert_eq!(*vals.get(ev.name).unwrap_or(&0), 0);
        }
    }

    #[test]
    fn stat_collect_once_pid_graceful() {
        let opts = StatOpts {
            events: vec![find_event("cycles").unwrap()],
            system_wide: false,
            pid: Some(999_999),
            repeat: 1,
            command: vec![],
        };
        let vals = stat_collect_once(&opts);
        assert_eq!(*vals.get("cycles").unwrap_or(&0), 0);
    }

    // ----------------------------------------------------------------
    // stat_print_results (smoke tests — just verify no panic)
    // ----------------------------------------------------------------

    #[test]
    fn stat_print_single_run() {
        let mut run = HashMap::new();
        run.insert("cycles".to_string(), 100_000u64);
        run.insert("instructions".to_string(), 200_000u64);
        let events = vec![
            find_event("cycles").unwrap(),
            find_event("instructions").unwrap(),
        ];
        // Should not panic.
        stat_print_results(&[run], &events);
    }

    #[test]
    fn stat_print_multiple_runs() {
        let mut run1 = HashMap::new();
        run1.insert("cycles".to_string(), 100_000u64);
        let mut run2 = HashMap::new();
        run2.insert("cycles".to_string(), 110_000u64);
        let events = vec![find_event("cycles").unwrap()];
        stat_print_results(&[run1, run2], &events);
    }

    #[test]
    fn stat_print_ipc_computed() {
        let mut run = HashMap::new();
        run.insert("cycles".to_string(), 1_000u64);
        run.insert("instructions".to_string(), 2_000u64);
        let events = vec![
            find_event("cycles").unwrap(),
            find_event("instructions").unwrap(),
        ];
        stat_print_results(&[run], &events);
    }

    #[test]
    fn stat_print_zero_cycles_no_ipc() {
        let mut run = HashMap::new();
        run.insert("cycles".to_string(), 0u64);
        run.insert("instructions".to_string(), 100u64);
        let events = vec![
            find_event("cycles").unwrap(),
            find_event("instructions").unwrap(),
        ];
        // Should not print IPC when cycles == 0.
        stat_print_results(&[run], &events);
    }

    // ----------------------------------------------------------------
    // OverheadEntry basic tests
    // ----------------------------------------------------------------

    #[test]
    fn overhead_entry_creation() {
        let entry = OverheadEntry {
            symbol: "main".to_string(),
            dso: "a.out".to_string(),
            comm: "test".to_string(),
            count: 42,
        };
        assert_eq!(entry.symbol, "main");
        assert_eq!(entry.count, 42);
    }

    // ----------------------------------------------------------------
    // TopEntry basic tests
    // ----------------------------------------------------------------

    #[test]
    fn top_entry_creation() {
        let entry = TopEntry {
            overhead_pct: 12.5,
            symbol: "hot_func".to_string(),
            dso: "myapp".to_string(),
            pid: 100,
        };
        assert_eq!(entry.symbol, "hot_func");
        assert!((entry.overhead_pct - 12.5).abs() < 0.001);
    }

    // ----------------------------------------------------------------
    // MAX_CALLCHAIN_DEPTH
    // ----------------------------------------------------------------

    #[test]
    fn max_callchain_depth_is_16() {
        assert_eq!(MAX_CALLCHAIN_DEPTH, 16);
    }

    // ----------------------------------------------------------------
    // PERF_MAGIC / PERF_VERSION
    // ----------------------------------------------------------------

    #[test]
    fn perf_magic_is_8_bytes() {
        assert_eq!(PERF_MAGIC.len(), 8);
    }

    #[test]
    fn perf_magic_value() {
        assert_eq!(PERF_MAGIC, b"PERFDATA");
    }

    #[test]
    fn perf_version_is_1() {
        assert_eq!(PERF_VERSION, 1);
    }

    // ----------------------------------------------------------------
    // Edge cases in number formatting
    // ----------------------------------------------------------------

    #[test]
    fn format_number_one() {
        assert_eq!(format_number(1), "1");
    }

    #[test]
    fn format_number_max_u64() {
        let s = format_number(u64::MAX);
        // Should contain commas and not panic.
        assert!(s.contains(','));
    }

    #[test]
    fn format_number_exact_thousands() {
        assert_eq!(format_number(1_000_000), "1,000,000");
    }

    // ----------------------------------------------------------------
    // Version string
    // ----------------------------------------------------------------

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
        assert_eq!(VERSION, "0.1.0");
    }

    // ----------------------------------------------------------------
    // Multiple samples write/read cycle consistency
    // ----------------------------------------------------------------

    #[test]
    fn ten_samples_round_trip() {
        let samples: Vec<PerfSample> = (0..10u64)
            .map(|i| {
                let mut s = PerfSample::new();
                s.timestamp_ns = i * 100;
                s.pid = i as u32;
                s.tid = i as u32 + 100;
                s.ip = 0x1000 + i * 4;
                s.callchain_len = 2;
                s.callchain[0] = 0xA000 + i;
                s.callchain[1] = 0xB000 + i;
                s
            })
            .collect();

        let mut buf = Vec::new();
        for s in &samples {
            s.write_to(&mut buf).unwrap();
        }
        assert_eq!(buf.len(), 10 * PerfSample::DISK_SIZE as usize);

        let mut cursor = Cursor::new(buf);
        for i in 0..10u64 {
            let s = PerfSample::read_from(&mut cursor).unwrap();
            assert_eq!(s.timestamp_ns, i * 100);
            assert_eq!(s.pid, i as u32);
            assert_eq!(s.tid, i as u32 + 100);
            assert_eq!(s.ip, 0x1000 + i * 4);
            assert_eq!(s.callchain_len, 2);
            assert_eq!(s.callchain[0], 0xA000 + i);
            assert_eq!(s.callchain[1], 0xB000 + i);
        }
    }

    // ----------------------------------------------------------------
    // Header field validation edge cases
    // ----------------------------------------------------------------

    #[test]
    fn header_zero_frequency() {
        let hdr = PerfFileHeader {
            data_offset: PerfFileHeader::DISK_SIZE,
            data_size: 0,
            sample_count: 0,
            event_name: "x".to_string(),
            frequency: 0,
            has_callgraph: false,
        };
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr2.frequency, 0);
    }

    #[test]
    fn header_max_frequency() {
        let hdr = PerfFileHeader {
            data_offset: PerfFileHeader::DISK_SIZE,
            data_size: 0,
            sample_count: 0,
            event_name: "y".to_string(),
            frequency: u32::MAX,
            has_callgraph: true,
        };
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr2.frequency, u32::MAX);
    }

    #[test]
    fn header_max_sample_count() {
        let hdr = PerfFileHeader {
            data_offset: PerfFileHeader::DISK_SIZE,
            data_size: 0,
            sample_count: u64::MAX,
            event_name: "z".to_string(),
            frequency: 1,
            has_callgraph: false,
        };
        let mut buf = Vec::new();
        hdr.write_to(&mut buf).unwrap();
        let mut cursor = Cursor::new(buf);
        let hdr2 = PerfFileHeader::read_from(&mut cursor).unwrap();
        assert_eq!(hdr2.sample_count, u64::MAX);
    }

    // ----------------------------------------------------------------
    // stat_print_results edge: empty runs
    // ----------------------------------------------------------------

    #[test]
    fn stat_print_empty_events_no_panic() {
        stat_print_results(&[], &[]);
    }

    #[test]
    fn stat_print_empty_run_single() {
        let run = HashMap::new();
        let events = vec![find_event("cycles").unwrap()];
        stat_print_results(&[run], &events);
    }

    // ----------------------------------------------------------------
    // Data section size consistency
    // ----------------------------------------------------------------

    #[test]
    fn data_section_size_matches_sample_count() {
        let sample_count: u64 = 7;
        let data_size = sample_count * PerfSample::DISK_SIZE;
        let hdr = PerfFileHeader {
            data_offset: PerfFileHeader::DISK_SIZE,
            data_size,
            sample_count,
            event_name: "cycles".to_string(),
            frequency: 100,
            has_callgraph: false,
        };
        assert_eq!(hdr.data_size, 7 * 160);
    }

    // ----------------------------------------------------------------
    // SymbolEntry construction
    // ----------------------------------------------------------------

    #[test]
    fn symbol_entry_fields() {
        let se = SymbolEntry {
            start: 0x400000,
            end: 0x401000,
            name: "main".to_string(),
            dso: "a.out".to_string(),
        };
        assert_eq!(se.start, 0x400000);
        assert_eq!(se.end, 0x401000);
        assert_eq!(se.name, "main");
        assert_eq!(se.dso, "a.out");
    }

    // ----------------------------------------------------------------
    // Personality variants are all distinct
    // ----------------------------------------------------------------

    #[test]
    fn personality_variants_distinct() {
        let variants = [
            Personality::Perf,
            Personality::Stat,
            Personality::Record,
            Personality::Report,
            Personality::Top,
        ];
        for i in 0..variants.len() {
            for j in (i + 1)..variants.len() {
                assert_ne!(variants[i], variants[j]);
            }
        }
    }

    // ----------------------------------------------------------------
    // EventKind variants
    // ----------------------------------------------------------------

    #[test]
    fn event_kind_hardware_ne_software() {
        assert_ne!(EventKind::Hardware, EventKind::Software);
    }

    // ----------------------------------------------------------------
    // record option: callgraph_mode fp preserved
    // ----------------------------------------------------------------

    #[test]
    fn record_callgraph_fp_mode() {
        let args: Vec<String> = vec![
            "--call-graph".to_string(), "fp".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.callgraph_mode, "fp");
    }

    // ----------------------------------------------------------------
    // stat: -e with long-form flag
    // ----------------------------------------------------------------

    #[test]
    fn stat_parse_long_event_flag() {
        let args: Vec<String> = vec![
            "--event".to_string(), "page-faults".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_stat_args(&args).unwrap();
        assert_eq!(opts.events.len(), 1);
        assert_eq!(opts.events[0].name, "page-faults");
    }

    // ----------------------------------------------------------------
    // record: --output long form
    // ----------------------------------------------------------------

    #[test]
    fn record_parse_long_output_flag() {
        let args: Vec<String> = vec![
            "--output".to_string(), "out.data".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.output, "out.data");
    }

    // ----------------------------------------------------------------
    // record: --freq long form
    // ----------------------------------------------------------------

    #[test]
    fn record_parse_long_freq_flag() {
        let args: Vec<String> = vec![
            "--freq".to_string(), "2000".to_string(),
            "-a".to_string(),
        ];
        let opts = parse_record_args(&args).unwrap();
        assert_eq!(opts.frequency, 2000);
    }

    // ----------------------------------------------------------------
    // report: --input long form
    // ----------------------------------------------------------------

    #[test]
    fn report_parse_long_input_flag() {
        let args: Vec<String> = vec![
            "--input".to_string(), "alt.data".to_string(),
        ];
        let opts = parse_report_args(&args).unwrap();
        assert_eq!(opts.input, "alt.data");
    }

    // ----------------------------------------------------------------
    // top: --event long form
    // ----------------------------------------------------------------

    #[test]
    fn top_parse_long_event_flag() {
        let args: Vec<String> = vec![
            "--event".to_string(), "cache-misses".to_string(),
        ];
        let opts = parse_top_args(&args).unwrap();
        assert_eq!(opts.event, "cache-misses");
    }

    // ----------------------------------------------------------------
    // top: --count long form
    // ----------------------------------------------------------------

    #[test]
    fn top_parse_long_count_flag() {
        let args: Vec<String> = vec![
            "--count".to_string(), "10".to_string(),
        ];
        let opts = parse_top_args(&args).unwrap();
        assert_eq!(opts.count, 10);
    }

    // ----------------------------------------------------------------
    // report: sort by dso alone
    // ----------------------------------------------------------------

    #[test]
    fn report_parse_sort_dso() {
        let args: Vec<String> = vec!["--sort".to_string(), "dso".to_string()];
        let opts = parse_report_args(&args).unwrap();
        assert_eq!(opts.sort_keys, vec!["dso"]);
    }

    // ----------------------------------------------------------------
    // report: --stdio flag
    // ----------------------------------------------------------------

    #[test]
    fn report_parse_stdio_flag() {
        let args: Vec<String> = vec!["--stdio".to_string()];
        let opts = parse_report_args(&args).unwrap();
        assert!(opts.stdio);
    }

    // ----------------------------------------------------------------
    // Comprehensive file header read from truncated buffer fails
    // ----------------------------------------------------------------

    #[test]
    fn header_truncated_read_fails() {
        let buf = vec![0u8; 10]; // too short
        let mut cursor = Cursor::new(buf);
        assert!(PerfFileHeader::read_from(&mut cursor).is_err());
    }

    #[test]
    fn sample_truncated_read_fails() {
        let buf = vec![0u8; 10]; // too short
        let mut cursor = Cursor::new(buf);
        assert!(PerfSample::read_from(&mut cursor).is_err());
    }
}
