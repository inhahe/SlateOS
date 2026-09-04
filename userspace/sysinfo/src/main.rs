//! Slate OS System Information Utility
//!
//! Queries and displays system information from `/proc`, `/sys` and other
//! kernel interfaces. Similar to `neofetch`, `inxi`, or Windows `systeminfo` —
//! a single command for a quick system overview.
//!
//! # Commands
//!
//! ```text
//! sysinfo              Full system summary
//! sysinfo cpu          CPU information
//! sysinfo memory       Memory statistics
//! sysinfo disk         Disk/filesystem usage
//! sysinfo network      Network configuration
//! sysinfo os           OS version and kernel info
//! sysinfo process      Process summary
//! sysinfo all          Everything (verbose)
//! sysinfo json         Full system info as JSON
//! ```
//!
//! # Where the reading happens
//!
//! Nowhere in this file. Every `/proc` open, parse and field extraction lives
//! in the `procinfo` crate, which `apps/sysinfo` — the graphical system
//! information program — also depends on, so the two agree about what the
//! kernel said by construction rather than by coincidence. This binary is the
//! formatting half: columns, units, JSON, and the decision about what to do
//! when a file cannot be read.
//!
//! Requested by lane C in
//! `requests/c-b-the-proc-readers-in-userspace-sysinfo-should-be-a-crate-both-sysinfos-can-use.md`.
//!
//! # Two behaviours that changed with the split, and why
//!
//! **A file we cannot read is no longer reported as a file that does not
//! exist.** The old reader was `fs::read_to_string(path).ok()`, so a
//! permission error and an absent file both printed
//! `(cpuinfo not available)`. Now an absent file prints that — because it is
//! true — and any other error prints the error on stderr and makes the
//! process exit non-zero. A system-information tool that cannot tell you it
//! failed is worse than one that fails.
//!
//! **Paths are written as bytes.** A mount point is a path, and a SlateOS path
//! is any bytes except `/` and NUL. The old code went through `String`, which
//! drops or corrupts a mount point that is not UTF-8, and printed the kernel's
//! `\040` escaping literally, so a mount at `/mnt/my backup` displayed as
//! `/mnt/my\040backup`.

use std::env;
use std::fs;
use std::io::{self, Write as _};
use std::process::ExitCode;

use procinfo::{CpuInfo, LoadAvg, MemInfo, Mount, NetDevice, ProcFs, SchedCounters, Uptime};

// ============================================================================
// Output primitives
// ============================================================================

/// A line being assembled out of a mix of program text and raw kernel bytes.
///
/// Exists because half the values on a `sysinfo` line are paths, and a path
/// cannot be formatted through `println!` without first claiming it is UTF-8.
/// A `Row` is a byte buffer, so the claim is never made.
struct Row(Vec<u8>);

impl Row {
    fn new() -> Self {
        Self(Vec::new())
    }

    /// Append program text — a label, a separator, a number we formatted.
    fn text(&mut self, text: &str) -> &mut Self {
        self.0.extend_from_slice(text.as_bytes());
        self
    }

    /// Append kernel bytes verbatim.
    fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.extend_from_slice(bytes);
        self
    }

    /// Append kernel bytes and pad to `width` columns with spaces.
    ///
    /// Never truncates. The table this serves sizes its columns from the data
    /// (see [`column_width`]), so a value wider than the column means the
    /// column was computed from a different set of rows — and a row that
    /// pushes the table one column wide is a far smaller problem than a mount
    /// point silently cut in half, which is what the code this replaces did.
    fn padded(&mut self, bytes: &[u8], width: usize) -> &mut Self {
        self.raw(bytes);
        for _ in 0..width.saturating_sub(display_width(bytes)) {
            self.0.push(b' ');
        }
        self
    }

    /// Write the row and a newline to stdout.
    fn emit(&self) {
        let mut out = io::stdout().lock();
        // A closed stdout (`sysinfo | head`) is not a failure of sysinfo, and
        // there is nowhere left to report it to anyway.
        let _ = out.write_all(&self.0);
        let _ = out.write_all(b"\n");
    }
}

/// How many terminal columns a field occupies, as well as we can know.
///
/// Character count when the bytes are UTF-8, byte count when they are not.
/// Neither is the true display width — a combining mark takes no column and a
/// CJK ideograph takes two — but a column table is a display convenience, and
/// the alternative (a Unicode width table in a system-information tool) buys
/// alignment for cases that do not arise in `/proc` at the cost of a second
/// place the tree keeps such a table.
fn display_width(bytes: &[u8]) -> usize {
    std::str::from_utf8(bytes).map_or(bytes.len(), |text| text.chars().count())
}

/// The width of a column: the widest value in it, and at least the heading.
fn column_width<'a>(heading: &str, values: impl Iterator<Item = &'a [u8]>) -> usize {
    values.fold(heading.len(), |widest, value| {
        widest.max(display_width(value))
    })
}

// ============================================================================
// Error reporting
// ============================================================================

/// Whether anything failed to be read, as distinct from being absent.
///
/// Threaded through every subject so `main` can exit non-zero. The previous
/// version had no such concept: it exited 0 whether it had read the machine or
/// nothing at all.
#[derive(Default)]
struct Status {
    failed: bool,
}

impl Status {
    /// Unwrap a collector result, reporting and recording any real error.
    ///
    /// `Ok(Some(v))` is a value; `Ok(None)` is "this kernel does not export
    /// it", which is an answer and not a fault; `Err` is a fault.
    fn take<T>(&mut self, what: &str, result: io::Result<Option<T>>) -> Option<T> {
        match result {
            Ok(value) => value,
            Err(err) => {
                self.failed = true;
                let mut row = Row::new();
                row.text("sysinfo: ").text(what).text(": ");
                row.text(&err.to_string());
                let mut errout = io::stderr().lock();
                let _ = errout.write_all(&row.0);
                let _ = errout.write_all(b"\n");
                None
            }
        }
    }
}

// ============================================================================
// CPU
// ============================================================================

fn show_cpu(proc: &ProcFs, status: &mut Status) {
    println!("=== CPU ===");

    match status.take("/proc/cpuinfo", proc.cpu()) {
        Some(cpu) => print_cpu(&cpu),
        None => println!("  (cpuinfo not available)"),
    }

    if let Some(load) = status.take("/proc/loadavg", proc.load_average()) {
        print_load(&load);
    }
}

fn print_cpu(cpu: &CpuInfo) {
    let mut row = Row::new();
    row.text("  Model:     ")
        .raw(cpu.model.as_deref().unwrap_or(b"Unknown"))
        .emit();

    let mut row = Row::new();
    row.text("  Vendor:    ")
        .raw(cpu.vendor.as_deref().unwrap_or(b"Unknown"))
        .emit();

    println!("  Cores:     {}", cpu.logical_cpus);

    let mut row = Row::new();
    row.text("  Frequency: ")
        .raw(cpu.mhz.as_deref().unwrap_or(b"?"))
        .text(" MHz")
        .emit();

    let mut row = Row::new();
    row.text("  Cache:     ")
        .raw(cpu.cache.as_deref().unwrap_or(b"?"))
        .emit();
}

fn print_load(load: &LoadAvg) {
    print!(
        "  Load avg:  {:.2} {:.2} {:.2}",
        load.one, load.five, load.fifteen
    );
    if let (Some(runnable), Some(total)) = (load.runnable, load.total) {
        print!("  ({runnable}/{total} runnable)");
    }
    println!();
}

// ============================================================================
// Memory
// ============================================================================

fn show_memory(proc: &ProcFs, status: &mut Status) {
    println!("=== Memory ===");

    match status.take("/proc/meminfo", proc.memory()) {
        Some(mem) => print_memory(&mem),
        None => println!("  (meminfo not available)"),
    }

    match status.take("/proc/swaps", proc.swaps()) {
        Some(lines) if lines.is_empty() => println!("  Swap:      none"),
        Some(lines) => {
            println!("  Swap:");
            for line in lines {
                let mut row = Row::new();
                row.text("    ").raw(&line).emit();
            }
        }
        None => {}
    }
}

fn print_memory(mem: &MemInfo) {
    let field = |label: &str, value: Option<u64>| match value {
        Some(kib) => println!("  {label:10} {kib} kB"),
        // A field the kernel did not write is different from one that is zero,
        // and this is the line where the difference is visible.
        None => println!("  {label:10} (not reported)"),
    };
    field("Total:", mem.total_kib);
    field("Free:", mem.free_kib);
    field("Available:", mem.available_kib);
    field("Buffers:", mem.buffers_kib);
    field("Cached:", mem.cached_kib);

    // Only when both figures are real: a used-memory percentage derived from a
    // missing total is not a number worth showing.
    if let (Some(used), Some(pct)) = (mem.used_kib(), mem.used_percent()) {
        println!("  Used:      {used} kB ({pct:.1}%)");
    }
}

// ============================================================================
// Filesystems
// ============================================================================

fn show_disk(proc: &ProcFs, status: &mut Status) {
    println!("=== Filesystems ===");

    let Some(mounts) = status.take("/proc/mounts", proc.mounts()) else {
        println!("  (mount info not available)");
        return;
    };
    if mounts.is_empty() {
        println!("  (nothing mounted)");
        return;
    }
    print_mounts(&mounts);
}

/// Print the mount table.
///
/// Three defects in the version this replaces, all in these twenty lines:
///
/// 1. **It truncated the options with `&parts[3][..20]`.** That is a byte
///    slice of a `&str`, so it panics if byte 20 lands inside a multi-byte
///    character — and it truncated silently, so a reader could not tell a
///    short option list from a cut one.
/// 2. **It never truncated the device**, but padded it to 20, so one long
///    device name pushed every later column out of alignment for the whole
///    table.
/// 3. **It printed the kernel's escaping literally.** `/proc/mounts` is
///    whitespace-separated, so a space in a path arrives as `\040`; the mount
///    displayed as `/mnt/my\040backup`. `procinfo` now undoes it.
///
/// The fix for 1 and 2 together is to stop guessing the widths: the columns
/// are sized from the data, which is what `df` and `mount` do, and nothing is
/// ever cut. Options go last and unbounded, because they are the one field
/// with no useful upper bound and the one a reader scans rather than aligns.
fn print_mounts(mounts: &[Mount]) {
    for row in mount_table(mounts) {
        row.emit();
    }
}

/// The mount table as rows, heading first.
///
/// Separate from [`print_mounts`] so the alignment claim above can be checked
/// by a test rather than by reading the output of a program on a machine that
/// happens to have the interesting mount. The rows are bytes, so a test can
/// assert on a mount point that is not UTF-8.
fn mount_table(mounts: &[Mount]) -> Vec<Row> {
    const MOUNT: &str = "Mount";
    const DEVICE: &str = "Device";
    const TYPE: &str = "Type";

    let mount_w = column_width(MOUNT, mounts.iter().map(|m| m.mount_point.as_slice()));
    let device_w = column_width(DEVICE, mounts.iter().map(|m| m.device.as_slice()));
    let type_w = column_width(TYPE, mounts.iter().map(|m| m.fstype.as_slice()));

    let mut rows = Vec::with_capacity(mounts.len().saturating_add(1));

    let mut header = Row::new();
    header
        .text("  ")
        .padded(MOUNT.as_bytes(), mount_w)
        .text(" ")
        .padded(DEVICE.as_bytes(), device_w)
        .text(" ")
        .padded(TYPE.as_bytes(), type_w)
        .text(" Options");
    rows.push(header);

    for mount in mounts {
        let mut row = Row::new();
        row.text("  ")
            .padded(&mount.mount_point, mount_w)
            .text(" ")
            .padded(&mount.device, device_w)
            .text(" ")
            .padded(&mount.fstype, type_w)
            .text(" ")
            .raw(&mount.options);
        rows.push(row);
    }
    rows
}

// ============================================================================
// Network
// ============================================================================

fn show_network(proc: &ProcFs, status: &mut Status) {
    println!("=== Network ===");

    if let Some(hostname) = status.take("/proc/sys/kernel/hostname", proc.hostname()) {
        let mut row = Row::new();
        row.text("  Hostname:  ").raw(&hostname).emit();
    }

    if let Some(devices) = status.take("/proc/net/dev", proc.net_devices())
        && !devices.is_empty()
    {
        println!();
        println!("  Interfaces:");
        print_interfaces(&devices);
    }

    print_resolv_conf(status);
}

fn print_interfaces(devices: &[NetDevice]) {
    for row in interface_rows(devices) {
        row.emit();
    }
}

/// One row per interface, names aligned against each other.
fn interface_rows(devices: &[NetDevice]) -> Vec<Row> {
    let name_w = column_width("", devices.iter().map(|d| d.name.as_slice()));
    devices
        .iter()
        .map(|device| {
            let mut row = Row::new();
            row.text("    ").padded(&device.name, name_w).text("  ");
            match (device.rx_bytes, device.tx_bytes) {
                (Some(rx), Some(tx)) => row.text(&format!("RX: {rx} bytes, TX: {tx} bytes")),
                // A counter the kernel did not write is not zero traffic.
                _ => row.text("(counters not reported)"),
            };
            row
        })
        .collect()
}

/// DNS servers, from `/etc/resolv.conf`.
///
/// Not `/proc`, so not `procinfo`'s: that crate is the kernel's interfaces,
/// and `resolv.conf` is a configuration file the resolver reads. Read here as
/// bytes for the same reason everything else is — a `nameserver` line is not
/// guaranteed text, and a missing file is not an error.
fn print_resolv_conf(status: &mut Status) {
    let content = match fs::read("/etc/resolv.conf") {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return,
        Err(err) => {
            status.failed = true;
            eprintln!("sysinfo: /etc/resolv.conf: {err}");
            return;
        }
    };
    for line in content.split(|&b| b == b'\n') {
        let Some(rest) = line.strip_prefix(b"nameserver") else {
            continue;
        };
        let server: Vec<u8> = rest
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .take_while(|b| !b.is_ascii_whitespace())
            .collect();
        if server.is_empty() {
            continue;
        }
        let mut row = Row::new();
        row.text("  DNS:       ").raw(&server).emit();
    }
}

// ============================================================================
// Operating system
// ============================================================================

fn show_os(proc: &ProcFs, status: &mut Status) {
    println!("=== Operating System ===");

    match status.take("/proc/version", proc.version()) {
        Some(version) => {
            let mut row = Row::new();
            row.text("  Version:   ").raw(&version).emit();
        }
        None => println!("  Version:   Slate OS (version info not available)"),
    }

    if let Some(uptime) = status.take("/proc/uptime", proc.uptime()) {
        print_uptime(&uptime);
    }

    if let Some(cmdline) = status.take("/proc/cmdline", proc.cmdline()) {
        let mut row = Row::new();
        row.text("  Cmdline:   ").raw(&cmdline).emit();
    }

    println!("  Arch:      x86_64");
    println!("  Page size: 16 KiB");
}

fn print_uptime(uptime: &Uptime) {
    let (days, hours, mins, secs) = uptime.dhms();
    println!("  Uptime:    {days}d {hours}h {mins}m {secs}s");
}

// ============================================================================
// Processes
// ============================================================================

fn show_process(proc: &ProcFs, status: &mut Status) {
    println!("=== Processes ===");

    match proc.process_ids() {
        Ok(pids) => println!("  Present:   {} processes", pids.len()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            println!("  (process info not available)");
        }
        Err(err) => {
            status.failed = true;
            eprintln!("sysinfo: /proc: {err}");
        }
    }

    if let Some(load) = status.take("/proc/loadavg", proc.load_average()) {
        print_load(&load);
    }

    if let Some(counters) = status.take("/proc/stat", proc.sched_counters()) {
        print_sched(&counters);
    }
}

fn print_sched(counters: &SchedCounters) {
    // `Running` here is the kernel's `procs_running`, which counts tasks on a
    // run queue -- not the same number as the process count above, which
    // counts every task that exists. The old output labelled both "Running:",
    // one line apart, and the second silently overwrote the reader's
    // understanding of the first.
    if let Some(running) = counters.running {
        println!("  Runnable:  {running}");
    }
    if let Some(blocked) = counters.blocked {
        println!("  Blocked:   {blocked}");
    }
    if let Some(forks) = counters.forks {
        println!("  Forks:     {forks} since boot");
    }
}

// ============================================================================
// JSON
// ============================================================================

fn show_json(proc: &ProcFs, status: &mut Status) {
    let mut parts: Vec<String> = Vec::new();

    let version = status.take("/proc/version", proc.version());
    let uptime = status.take("/proc/uptime", proc.uptime());
    parts.push(format!(
        "\"os\":{{\"version\":{},\"uptime_seconds\":{},\"arch\":\"x86_64\",\"page_size\":16384}}",
        json_bytes(version.as_deref()),
        uptime.map_or_else(|| "null".to_string(), |u| u.up.as_secs().to_string())
    ));

    if let Some(cpu) = status.take("/proc/cpuinfo", proc.cpu()) {
        parts.push(format!(
            "\"cpu\":{{\"model\":{},\"cores\":{}}}",
            json_bytes(cpu.model.as_deref()),
            cpu.logical_cpus
        ));
    }

    if let Some(mem) = status.take("/proc/meminfo", proc.memory()) {
        parts.push(format!(
            "\"memory\":{{\"total_kib\":{},\"free_kib\":{},\"available_kib\":{}}}",
            json_number(mem.total_kib),
            json_number(mem.free_kib),
            json_number(mem.available_kib)
        ));
    }

    let hostname = status.take("/proc/sys/kernel/hostname", proc.hostname());
    parts.push(format!("\"hostname\":{}", json_bytes(hostname.as_deref())));

    println!("{{{}}}", parts.join(","));
}

/// A `u64` field, or JSON `null` when the kernel did not report it.
///
/// `null`, not `0`: a machine-readable report that says a kernel has zero
/// bytes of memory is worse than one that says it does not know.
fn json_number(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_string(), |v| v.to_string())
}

/// A byte field as a JSON string, or `null` when absent.
///
/// Bytes that are not valid UTF-8 are emitted as `\u00XX` escapes of the
/// individual bytes. That is not a faithful round-trip — nothing in JSON is,
/// for arbitrary bytes — but it is lossless in the sense that matters here:
/// every byte is present and distinguishable, where a lossy UTF-8 conversion
/// would replace a run of them with one U+FFFD and lose the count.
fn json_bytes(value: Option<&[u8]>) -> String {
    let Some(bytes) = value else {
        return "null".to_string();
    };
    let mut out = String::with_capacity(bytes.len().saturating_add(2));
    out.push('"');
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            for ch in text.chars() {
                push_json_char(&mut out, ch);
            }
        }
        Err(_) => {
            for &byte in bytes {
                if byte.is_ascii() {
                    push_json_char(&mut out, char::from(byte));
                } else {
                    out.push_str(&format!("\\u{byte:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn push_json_char(out: &mut String, ch: char) {
    match ch {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        // Every other control character must be escaped for the output to be
        // JSON at all (RFC 8259 §7); the old escaper emitted them raw.
        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
        c => out.push(c),
    }
}

// ============================================================================
// Summary
// ============================================================================

fn show_summary(proc: &ProcFs, status: &mut Status) {
    show_os(proc, status);
    println!();
    show_cpu(proc, status);
    println!();
    show_memory(proc, status);
    println!();
    show_disk(proc, status);
    println!();
    show_network(proc, status);
    println!();
    show_process(proc, status);
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("Slate OS System Information v0.1.0");
    println!();
    println!("Query and display comprehensive system information.");
    println!();
    println!("USAGE:");
    println!("  sysinfo [command]");
    println!();
    println!("COMMANDS:");
    println!("  (no args)    Full system summary");
    println!("  cpu          CPU information");
    println!("  memory       Memory statistics");
    println!("  disk         Disk and filesystem usage");
    println!("  network      Network configuration");
    println!("  os           OS and kernel version");
    println!("  process      Process summary");
    println!("  all          Everything (verbose)");
    println!("  json         Full info as JSON");
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let proc = ProcFs::new();
    let mut status = Status::default();

    match args.get(1).map(String::as_str) {
        None | Some("all" | "full") => show_summary(&proc, &mut status),
        Some("cpu") => show_cpu(&proc, &mut status),
        Some("memory" | "mem" | "ram") => show_memory(&proc, &mut status),
        Some("disk" | "fs" | "filesystems") => show_disk(&proc, &mut status),
        Some("network" | "net") => show_network(&proc, &mut status),
        Some("os" | "version") => show_os(&proc, &mut status),
        Some("process" | "proc" | "ps") => show_process(&proc, &mut status),
        Some("json") => show_json(&proc, &mut status),
        Some("help" | "--help" | "-h") => print_usage(),
        Some(other) => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'sysinfo help' for usage.");
            return ExitCode::from(1);
        }
    }

    // Non-zero when something could not be read, as distinct from not existing.
    // The previous version exited 0 whether it had described the machine or
    // printed six "(not available)" lines.
    if status.failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests;
