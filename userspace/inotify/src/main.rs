//! Slate OS filesystem event monitoring utility.
//!
//! Multi-personality binary providing:
//! - **inotifywait** — wait for filesystem events on files/directories
//! - **inotifywatch** — gather filesystem event statistics
//!
//! Monitors files and directories for changes using inotify.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::io::{self, Write};
use std::process;
use std::time::{Duration, Instant};

const VERSION: &str = "0.1.0";

// ============================================================================
// Inotify event flags
// ============================================================================

const IN_ACCESS: u32 = 0x0000_0001;
const IN_MODIFY: u32 = 0x0000_0002;
const IN_ATTRIB: u32 = 0x0000_0004;
const IN_CLOSE_WRITE: u32 = 0x0000_0008;
const IN_CLOSE_NOWRITE: u32 = 0x0000_0010;
const IN_OPEN: u32 = 0x0000_0020;
const IN_MOVED_FROM: u32 = 0x0000_0040;
const IN_MOVED_TO: u32 = 0x0000_0080;
const IN_CREATE: u32 = 0x0000_0100;
const IN_DELETE: u32 = 0x0000_0200;
const IN_DELETE_SELF: u32 = 0x0000_0400;
const IN_MOVE_SELF: u32 = 0x0000_0800;
const IN_UNMOUNT: u32 = 0x0000_2000;
const IN_ISDIR: u32 = 0x4000_0000;

const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;
const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
const IN_ALL_EVENTS: u32 = IN_ACCESS
    | IN_MODIFY
    | IN_ATTRIB
    | IN_CLOSE_WRITE
    | IN_CLOSE_NOWRITE
    | IN_OPEN
    | IN_MOVED_FROM
    | IN_MOVED_TO
    | IN_CREATE
    | IN_DELETE
    | IN_DELETE_SELF
    | IN_MOVE_SELF;

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct InotifyEvent {
    watch_path: String,
    mask: u32,
    name: String,
    _cookie: u32,
}

#[derive(Clone, Debug)]
struct EventStats {
    counts: HashMap<String, u64>,
    total: u64,
}

struct WaitOpts {
    monitor: bool,
    recursive: bool,
    quiet: bool,
    csv: bool,
    timefmt: Option<String>,
    format: Option<String>,
    timeout: Option<u64>,
    events: u32,
    exclude: Vec<String>,
    include: Vec<String>,
    paths: Vec<String>,
    from_file: Option<String>,
}

struct WatchOpts {
    recursive: bool,
    timeout: Option<u64>,
    events: u32,
    exclude: Vec<String>,
    paths: Vec<String>,
    verbose: bool,
}

// ============================================================================
// Event name helpers
// ============================================================================

fn event_flag_names(mask: u32) -> Vec<&'static str> {
    let mut names = Vec::new();
    if mask & IN_ACCESS != 0 {
        names.push("ACCESS");
    }
    if mask & IN_MODIFY != 0 {
        names.push("MODIFY");
    }
    if mask & IN_ATTRIB != 0 {
        names.push("ATTRIB");
    }
    if mask & IN_CLOSE_WRITE != 0 {
        names.push("CLOSE_WRITE");
    }
    if mask & IN_CLOSE_NOWRITE != 0 {
        names.push("CLOSE_NOWRITE");
    }
    if mask & IN_OPEN != 0 {
        names.push("OPEN");
    }
    if mask & IN_MOVED_FROM != 0 {
        names.push("MOVED_FROM");
    }
    if mask & IN_MOVED_TO != 0 {
        names.push("MOVED_TO");
    }
    if mask & IN_CREATE != 0 {
        names.push("CREATE");
    }
    if mask & IN_DELETE != 0 {
        names.push("DELETE");
    }
    if mask & IN_DELETE_SELF != 0 {
        names.push("DELETE_SELF");
    }
    if mask & IN_MOVE_SELF != 0 {
        names.push("MOVE_SELF");
    }
    if mask & IN_UNMOUNT != 0 {
        names.push("UNMOUNT");
    }
    if mask & IN_ISDIR != 0 {
        names.push("ISDIR");
    }
    names
}

fn parse_event_name(name: &str) -> Option<u32> {
    match name.to_uppercase().as_str() {
        "ACCESS" => Some(IN_ACCESS),
        "MODIFY" => Some(IN_MODIFY),
        "ATTRIB" => Some(IN_ATTRIB),
        "CLOSE_WRITE" => Some(IN_CLOSE_WRITE),
        "CLOSE_NOWRITE" => Some(IN_CLOSE_NOWRITE),
        "CLOSE" => Some(IN_CLOSE),
        "OPEN" => Some(IN_OPEN),
        "MOVED_FROM" => Some(IN_MOVED_FROM),
        "MOVED_TO" => Some(IN_MOVED_TO),
        "MOVE" => Some(IN_MOVE),
        "CREATE" => Some(IN_CREATE),
        "DELETE" => Some(IN_DELETE),
        "DELETE_SELF" => Some(IN_DELETE_SELF),
        "MOVE_SELF" => Some(IN_MOVE_SELF),
        "UNMOUNT" => Some(IN_UNMOUNT),
        "ALL_EVENTS" | "ALL" => Some(IN_ALL_EVENTS),
        _ => None,
    }
}

fn parse_event_list(list: &str) -> u32 {
    let mut mask = 0u32;
    for name in list.split(',') {
        if let Some(flag) = parse_event_name(name.trim()) {
            mask |= flag;
        }
    }
    mask
}

#[cfg(target_vendor = "slateos")]
unsafe fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    // SAFETY: Caller guarantees arguments are valid for the given syscall.
    // The `syscall` instruction clobbers rcx and r11 per the System V ABI.
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

// Stub for development hosts (where `cargo test` runs).
#[cfg(not(target_vendor = "slateos"))]
unsafe fn syscall3(_nr: u64, _a1: u64, _a2: u64, _a3: u64) -> i64 {
    -38 // ENOSYS
}

/// Syscall numbers, from `kernel/src/syscall/linux.rs`.
const SYS_READ: u64 = 0;
const SYS_CLOSE: u64 = 3;
const SYS_INOTIFY_INIT1: u64 = 294;
const SYS_INOTIFY_ADD_WATCH: u64 = 254;

/// Watch `paths` and return the first batch of events the kernel reports.
///
/// `None` means this build cannot watch at all -- the syscalls answered
/// ENOSYS, which is what the development-host stub above returns. The
/// caller says so rather than inventing events, which is what it used to
/// do: `generate_simulated_events` reported a `newfile.txt` creation for
/// every run, so `while inotifywait -e create dir; do rebuild; done` never
/// stopped rebuilding.
fn watch_once(paths: &[String], mask: u32) -> Option<(usize, Vec<(usize, RawEvent)>)> {
    // SAFETY: no pointer arguments; `inotify_init1(0)` takes flags only.
    let fd = unsafe { syscall3(SYS_INOTIFY_INIT1, 0, 0, 0) };
    if fd < 0 {
        return None;
    }
    let fd_u = fd as u64;
    let mut wds: Vec<(i32, usize)> = Vec::new();
    for (idx, path) in paths.iter().enumerate() {
        let mut c_path = path.as_bytes().to_vec();
        c_path.push(0);
        // SAFETY: `c_path` is NUL-terminated and outlives the call.
        let wd = unsafe {
            syscall3(
                SYS_INOTIFY_ADD_WATCH,
                fd_u,
                c_path.as_ptr() as u64,
                u64::from(mask),
            )
        };
        if wd >= 0 {
            wds.push((wd as i32, idx));
        }
    }

    let mut buf = [0u8; 4096];
    // SAFETY: `buf` is valid for `len` bytes for the duration of the call.
    let n = unsafe { syscall3(SYS_READ, fd_u, buf.as_mut_ptr() as u64, buf.len() as u64) };
    // SAFETY: closing a descriptor this function opened.
    unsafe {
        syscall3(SYS_CLOSE, fd_u, 0, 0);
    }
    if n < 0 {
        return None;
    }
    let read = usize::try_from(n).unwrap_or(0).min(buf.len());
    let events = parse_event_buffer(buf.get(..read).unwrap_or(&[]));
    Some((
        wds.len(),
        events
            .into_iter()
            .map(|e| {
                let idx = wds
                    .iter()
                    .find(|(wd, _)| *wd == e.wd)
                    .map_or(0, |(_, idx)| *idx);
                (idx, e)
            })
            .collect(),
    ))
}

/// The kernel's events, as this program's own type.
///
/// `None` propagates "cannot watch" so the caller can say so.
fn real_events(paths: &[String], mask: u32) -> Option<(usize, Vec<InotifyEvent>)> {
    let (established, raw) = watch_once(paths, mask)?;
    Some((
        established,
        raw.into_iter()
            .map(|(idx, e)| InotifyEvent {
                watch_path: paths.get(idx).cloned().unwrap_or_default(),
                mask: e.mask,
                // A name that is not UTF-8 is shown escaped rather than
                // mangled: `from_utf8_lossy` would report a change to a file
                // that does not exist.
                name: String::from_utf8(e.name.clone()).unwrap_or_else(|_| format!("{:?}", e.name)),
                _cookie: e.cookie,
            })
            .collect(),
    ))
}

// ============================================================================
// The kernel's event stream
// ============================================================================

/// Fixed part of `struct inotify_event`: `wd` (i32), `mask`, `cookie`,
/// `len` (u32 each).
const EVENT_HEADER_LEN: usize = 16;

/// One event exactly as the kernel wrote it.
///
/// The name is **bytes**. A watched directory can hold a file whose name is
/// not UTF-8 -- on this OS any byte but `/` and NUL is legal in one -- and a
/// watcher that mangles the name reports a change to a file that does not
/// exist.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RawEvent {
    wd: i32,
    mask: u32,
    cookie: u32,
    name: Vec<u8>,
}

/// Split a read from an inotify fd into events.
///
/// The kernel packs them back to back, each a 16-byte header followed by
/// `len` bytes of name. `len` is the *padded* length: the name is
/// NUL-terminated and then padded to an alignment boundary, so the trailing
/// NULs are not part of it. `len` of 0 means no name at all, which is what
/// a watch on a file rather than a directory reports.
///
/// A trailing partial event is dropped rather than guessed at -- `read` on
/// an inotify fd never returns one, so seeing it means the buffer is not
/// what we think it is, and inventing a name from it is exactly the habit
/// this crate is being cured of.
fn parse_event_buffer(buf: &[u8]) -> Vec<RawEvent> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at.saturating_add(EVENT_HEADER_LEN) <= buf.len() {
        let head = match buf.get(at..at.saturating_add(EVENT_HEADER_LEN)) {
            Some(h) => h,
            None => break,
        };
        let word = |o: usize| -> u32 {
            let mut b = [0u8; 4];
            if let Some(src) = head.get(o..o.saturating_add(4)) {
                b.copy_from_slice(src);
            }
            u32::from_ne_bytes(b)
        };
        let wd = word(0) as i32;
        let mask = word(4);
        let cookie = word(8);
        let len = word(12) as usize;

        let name_at = at.saturating_add(EVENT_HEADER_LEN);
        let name_end = name_at.saturating_add(len);
        if name_end > buf.len() {
            break;
        }
        let raw_name = buf.get(name_at..name_end).unwrap_or(&[]);
        // The padding is NULs after the terminator; the name is what
        // precedes the first one.
        let name = raw_name
            .iter()
            .position(|b| *b == 0)
            .map_or(raw_name, |z| raw_name.get(..z).unwrap_or(&[]))
            .to_vec();

        out.push(RawEvent {
            wd,
            mask,
            cookie,
            name,
        });
        at = name_end;
    }
    out
}

// ============================================================================
// Event filtering
// ============================================================================

fn should_exclude(name: &str, exclude: &[String], include: &[String]) -> bool {
    if !include.is_empty() {
        // If include patterns are set, name must match at least one.
        let matched = include.iter().any(|pat| simple_glob(pat, name));
        if !matched {
            return true;
        }
    }
    // Check exclude patterns.
    exclude.iter().any(|pat| simple_glob(pat, name))
}

/// Simple glob matching supporting * and ? wildcards.
fn simple_glob(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match(&pat, &txt, 0, 0)
}

fn glob_match(pat: &[char], txt: &[char], pi: usize, ti: usize) -> bool {
    if pi == pat.len() {
        return ti == txt.len();
    }
    if let Some(&pc) = pat.get(pi) {
        if pc == '*' {
            // Try matching zero or more characters.
            for skip in 0..=(txt.len() - ti) {
                if glob_match(pat, txt, pi + 1, ti + skip) {
                    return true;
                }
            }
            false
        } else if pc == '?' {
            if ti < txt.len() {
                glob_match(pat, txt, pi + 1, ti + 1)
            } else {
                false
            }
        } else if ti < txt.len() && txt[ti] == pc {
            glob_match(pat, txt, pi + 1, ti + 1)
        } else {
            false
        }
    } else {
        false
    }
}

// ============================================================================
// Format output
// ============================================================================

fn format_event(event: &InotifyEvent, format: &Option<String>, csv: bool) -> String {
    let flags = event_flag_names(event.mask);
    let flags_str = flags.join(",");

    if csv {
        return format!("{},{},{}", event.watch_path, flags_str, event.name);
    }

    if let Some(fmt) = format {
        let mut out = fmt.clone();
        out = out.replace("%w", &event.watch_path);
        out = out.replace("%f", &event.name);
        out = out.replace("%e", &flags_str);
        // %T requires timefmt — use current time placeholder.
        if out.contains("%T") {
            out = out.replace("%T", "00:00:00");
        }
        return out;
    }

    if event.name.is_empty() {
        format!("{} {}", event.watch_path, flags_str)
    } else {
        format!("{} {} {}", event.watch_path, flags_str, event.name)
    }
}

fn format_timestamp(timefmt: &Option<String>) -> String {
    if let Some(fmt) = timefmt {
        // Simplified timestamp formatting.
        let mut out = fmt.clone();
        out = out.replace("%H", "00");
        out = out.replace("%M", "00");
        out = out.replace("%S", "00");
        out = out.replace("%Y", "2025");
        out = out.replace("%m", "01");
        out = out.replace("%d", "01");
        out
    } else {
        String::new()
    }
}

// ============================================================================
// inotifywait command
// ============================================================================

fn cmd_inotifywait(args: &[String]) {
    let mut opts = WaitOpts {
        monitor: false,
        recursive: false,
        quiet: false,
        csv: false,
        timefmt: None,
        format: None,
        timeout: None,
        events: IN_ALL_EVENTS,
        exclude: Vec::new(),
        include: Vec::new(),
        paths: Vec::new(),
        from_file: None,
    };

    let mut custom_events = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: inotifywait [options] <path> [path ...]");
                println!();
                println!("Wait for filesystem events.");
                println!();
                println!("Options:");
                println!("  -m, --monitor          Keep listening (don't exit after first event)");
                println!("  -r, --recursive        Watch directories recursively");
                println!("  -q, --quiet            Print less output");
                println!("  --csv                  Output in CSV format");
                println!("  --timefmt FMT          Time format for %T in --format");
                println!("  --format FMT           Custom output format");
                println!("  -t, --timeout SEC      Timeout in seconds");
                println!("  -e, --event EVENTS     Comma-separated events to watch");
                println!("  --exclude REGEX        Exclude matching files");
                println!("  --include REGEX        Include only matching files");
                println!("  --fromfile FILE        Read paths from file (- for stdin)");
                println!("  -h, --help             Show help");
                println!("  -V, --version          Show version");
                println!();
                println!("Events: ACCESS, MODIFY, ATTRIB, CLOSE_WRITE, CLOSE_NOWRITE,");
                println!("        OPEN, MOVED_FROM, MOVED_TO, CREATE, DELETE,");
                println!("        DELETE_SELF, MOVE_SELF, CLOSE, MOVE, ALL_EVENTS");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("inotifywait {VERSION}");
                process::exit(0);
            }
            "-m" | "--monitor" => opts.monitor = true,
            "-r" | "--recursive" => opts.recursive = true,
            "-q" | "--quiet" => opts.quiet = true,
            "--csv" => opts.csv = true,
            "--timefmt" => {
                i += 1;
                if i < args.len() {
                    opts.timefmt = Some(args[i].clone());
                }
            }
            "--format" => {
                i += 1;
                if i < args.len() {
                    opts.format = Some(args[i].clone());
                }
            }
            "-t" | "--timeout" => {
                i += 1;
                if i < args.len() {
                    opts.timeout = args[i].parse().ok();
                }
            }
            "-e" | "--event" => {
                i += 1;
                if i < args.len() {
                    if !custom_events {
                        opts.events = 0;
                        custom_events = true;
                    }
                    opts.events |= parse_event_list(&args[i]);
                }
            }
            "--exclude" => {
                i += 1;
                if i < args.len() {
                    opts.exclude.push(args[i].clone());
                }
            }
            "--include" => {
                i += 1;
                if i < args.len() {
                    opts.include.push(args[i].clone());
                }
            }
            "--fromfile" => {
                i += 1;
                if i < args.len() {
                    opts.from_file = Some(args[i].clone());
                }
            }
            s if !s.starts_with('-') => {
                opts.paths.push(s.to_string());
            }
            _ => {
                eprintln!("inotifywait: unknown option: {}", args[i]);
                // Stop. It used to report the option and keep parsing, so the
                // watch ran with a command line the program had already said
                // it did not understand.
                process::exit(1);
            }
        }
        i += 1;
    }

    // Read paths from file if specified.
    if let Some(ref file) = opts.from_file {
        if file == "-" {
            // Read from stdin.
            let stdin = io::stdin();
            let mut line = String::new();
            while stdin.read_line(&mut line).unwrap_or(0) > 0 {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    opts.paths.push(trimmed);
                }
                line.clear();
            }
        } else if let Ok(contents) = std::fs::read_to_string(file) {
            for line in contents.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    opts.paths.push(trimmed.to_string());
                }
            }
        }
    }

    if opts.paths.is_empty() {
        eprintln!("inotifywait: no paths specified");
        process::exit(1);
    }

    if !opts.quiet {
        eprintln!("Setting up watches.");
        if opts.recursive {
            eprintln!("  Watching recursively.");
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.csv {
        // CSV header.
        let _ = writeln!(out, "path,events,filename");
    }

    let start = Instant::now();
    let timeout_dur = opts.timeout.map(Duration::from_secs);

    // The kernel's events, or none at all if this build cannot watch.
    let Some((established, events)) = real_events(&opts.paths, opts.events) else {
        eprintln!("inotifywait: this build cannot watch for filesystem events");
        process::exit(1);
    };
    // Said after the attempt, and counting what the kernel actually took.
    // It used to be printed beforehand from `paths.len()`, so it announced
    // watches that were never established.
    if !opts.quiet {
        eprintln!("Watches established ({established} total).");
    }

    for event in &events {
        // Check timeout.
        if let Some(dur) = timeout_dur
            && start.elapsed() >= dur
        {
            if !opts.quiet {
                eprintln!("inotifywait: timeout");
            }
            process::exit(2);
        }

        // Apply filters.
        if should_exclude(&event.name, &opts.exclude, &opts.include) {
            continue;
        }

        // Format timestamp prefix.
        let ts = format_timestamp(&opts.timefmt);
        let line = format_event(event, &opts.format, opts.csv);

        if !ts.is_empty() {
            let _ = writeln!(out, "{ts} {line}");
        } else {
            let _ = writeln!(out, "{line}");
        }

        if !opts.monitor {
            // Exit after first event in non-monitor mode.
            return;
        }
    }

    if !opts.monitor
        && events.is_empty()
        && let Some(_dur) = timeout_dur
    {
        eprintln!("inotifywait: timeout");
        process::exit(2);
    }
}

// ============================================================================
// inotifywatch command
// ============================================================================

fn cmd_inotifywatch(args: &[String]) {
    let mut opts = WatchOpts {
        recursive: false,
        timeout: None,
        events: IN_ALL_EVENTS,
        exclude: Vec::new(),
        paths: Vec::new(),
        verbose: false,
    };

    let mut custom_events = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: inotifywatch [options] <path> [path ...]");
                println!();
                println!("Gather filesystem event statistics.");
                println!();
                println!("Options:");
                println!("  -r, --recursive    Watch directories recursively");
                println!("  -t, --timeout SEC  Gathering period in seconds");
                println!("  -e, --event EVENTS Comma-separated events to watch");
                println!("  --exclude PATTERN  Exclude matching files");
                println!("  -v, --verbose      Verbose output");
                println!("  -h, --help         Show help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("inotifywatch {VERSION}");
                process::exit(0);
            }
            "-r" | "--recursive" => opts.recursive = true,
            "-v" | "--verbose" => opts.verbose = true,
            "-t" | "--timeout" => {
                i += 1;
                if i < args.len() {
                    opts.timeout = args[i].parse().ok();
                }
            }
            "-e" | "--event" => {
                i += 1;
                if i < args.len() {
                    if !custom_events {
                        opts.events = 0;
                        custom_events = true;
                    }
                    opts.events |= parse_event_list(&args[i]);
                }
            }
            "--exclude" => {
                i += 1;
                if i < args.len() {
                    opts.exclude.push(args[i].clone());
                }
            }
            s if !s.starts_with('-') => {
                opts.paths.push(s.to_string());
            }
            _ => {
                eprintln!("inotifywatch: unknown option: {}", args[i]);
                // Stop. It used to report the option and keep parsing, so the
                // watch ran with a command line the program had already said
                // it did not understand.
                process::exit(1);
            }
        }
        i += 1;
    }

    if opts.paths.is_empty() {
        eprintln!("inotifywatch: no paths specified");
        process::exit(1);
    }

    if opts.verbose {
        eprintln!("Setting up watches on {} path(s)...", opts.paths.len());
        if opts.recursive {
            eprintln!("  Recursive mode enabled.");
        }
    }

    let Some((_established, events)) = real_events(&opts.paths, opts.events) else {
        eprintln!("inotifywatch: this build cannot watch for filesystem events");
        process::exit(1);
    };

    let mut stats: HashMap<String, EventStats> = HashMap::new();

    for event in &events {
        if should_exclude(&event.name, &opts.exclude, &[]) {
            continue;
        }

        let path_key = event.watch_path.clone();
        let entry = stats.entry(path_key).or_insert_with(|| EventStats {
            counts: HashMap::new(),
            total: 0,
        });

        for flag_name in event_flag_names(event.mask) {
            *entry.counts.entry(flag_name.to_string()).or_insert(0) += 1;
        }
        entry.total += 1;
    }

    // Print statistics table.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Collect all event types that occurred.
    let mut all_event_types: Vec<String> = Vec::new();
    for stat in stats.values() {
        for key in stat.counts.keys() {
            if !all_event_types.contains(key) {
                all_event_types.push(key.clone());
            }
        }
    }
    all_event_types.sort();

    // Header.
    let _ = write!(out, "total\t");
    for et in &all_event_types {
        let _ = write!(out, "{et}\t");
    }
    let _ = writeln!(out, "filename");

    // Data rows.
    let mut sorted_paths: Vec<&String> = stats.keys().collect();
    sorted_paths.sort();

    for path in sorted_paths {
        if let Some(stat) = stats.get(path) {
            let _ = write!(out, "{}\t", stat.total);
            for et in &all_event_types {
                let count = stat.counts.get(et).copied().unwrap_or(0);
                let _ = write!(out, "{count}\t");
            }
            let _ = writeln!(out, "{path}");
        }
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("inotifywait");
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
        "inotifywatch" => cmd_inotifywatch(&rest),
        _ => cmd_inotifywait(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── the kernel's event buffer ──

    /// Build one event the way the kernel packs it: 16-byte header then a
    /// NUL-padded name of `len` bytes.
    fn packed(wd: i32, mask: u32, cookie: u32, name: &[u8], pad_to: usize) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&wd.to_ne_bytes());
        v.extend_from_slice(&mask.to_ne_bytes());
        v.extend_from_slice(&cookie.to_ne_bytes());
        v.extend_from_slice(&(pad_to as u32).to_ne_bytes());
        let mut field = name.to_vec();
        field.resize(pad_to, 0);
        v.extend_from_slice(&field);
        v
    }

    #[test]
    fn one_event_round_trips() {
        let buf = packed(1, IN_CREATE, 0, b"newfile.txt", 16);
        let got = parse_event_buffer(&buf);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].wd, 1);
        assert_eq!(got[0].mask, IN_CREATE);
        assert_eq!(got[0].name, b"newfile.txt".to_vec(), "padding is not name");
    }

    /// The kernel packs several into one read, and the second must start
    /// after the *padded* length of the first, not after its text.
    #[test]
    fn several_events_in_one_read() {
        let mut buf = packed(1, IN_CREATE, 0, b"a.txt", 8);
        buf.extend_from_slice(&packed(2, IN_DELETE, 0, b"bb.txt", 8));
        let got = parse_event_buffer(&buf);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].name, b"a.txt".to_vec());
        assert_eq!(got[1].wd, 2);
        assert_eq!(got[1].name, b"bb.txt".to_vec());
    }

    /// A watch on a file rather than a directory reports no name at all.
    #[test]
    fn a_nameless_event_is_not_an_error() {
        let buf = packed(3, IN_MODIFY, 0, b"", 0);
        let got = parse_event_buffer(&buf);
        assert_eq!(got.len(), 1);
        assert!(got[0].name.is_empty());
    }

    /// A name that is not UTF-8 is carried through as bytes. A watcher that
    /// mangles it reports a change to a file that does not exist.
    #[test]
    fn a_non_utf8_name_survives() {
        let buf = packed(1, IN_CREATE, 0, b"caf\xe9.txt", 12);
        let got = parse_event_buffer(&buf);
        assert_eq!(got[0].name, b"caf\xe9.txt".to_vec());
    }

    /// A trailing partial event is dropped rather than guessed at.
    #[test]
    fn a_truncated_tail_is_ignored() {
        let mut buf = packed(1, IN_CREATE, 0, b"a", 4);
        buf.extend_from_slice(&[0u8; 9]); // less than a header
        assert_eq!(parse_event_buffer(&buf).len(), 1);
        // A header promising more name than the buffer holds is also dropped.
        let mut short = packed(1, IN_CREATE, 0, b"", 0);
        short.truncate(EVENT_HEADER_LEN);
        let mut lying = short.clone();
        lying.splice(12..16, 99u32.to_ne_bytes());
        assert!(parse_event_buffer(&lying).is_empty());
    }

    #[test]
    fn test_event_flag_names_single() {
        let names = event_flag_names(IN_CREATE);
        assert_eq!(names, vec!["CREATE"]);
    }

    #[test]
    fn test_event_flag_names_multiple() {
        let names = event_flag_names(IN_CREATE | IN_DELETE | IN_MODIFY);
        assert!(names.contains(&"CREATE"));
        assert!(names.contains(&"DELETE"));
        assert!(names.contains(&"MODIFY"));
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn test_event_flag_names_isdir() {
        let names = event_flag_names(IN_CREATE | IN_ISDIR);
        assert!(names.contains(&"CREATE"));
        assert!(names.contains(&"ISDIR"));
    }

    #[test]
    fn test_parse_event_name() {
        assert_eq!(parse_event_name("ACCESS"), Some(IN_ACCESS));
        assert_eq!(parse_event_name("modify"), Some(IN_MODIFY));
        assert_eq!(parse_event_name("CLOSE"), Some(IN_CLOSE));
        assert_eq!(parse_event_name("MOVE"), Some(IN_MOVE));
        assert_eq!(parse_event_name("ALL_EVENTS"), Some(IN_ALL_EVENTS));
        assert_eq!(parse_event_name("nonsense"), None);
    }

    #[test]
    fn test_parse_event_list() {
        let mask = parse_event_list("CREATE,DELETE,MODIFY");
        assert_eq!(mask, IN_CREATE | IN_DELETE | IN_MODIFY);
    }

    #[test]
    fn test_parse_event_list_single() {
        let mask = parse_event_list("ACCESS");
        assert_eq!(mask, IN_ACCESS);
    }

    #[test]
    fn test_parse_event_list_with_spaces() {
        let mask = parse_event_list("CREATE, DELETE, MODIFY");
        assert_eq!(mask, IN_CREATE | IN_DELETE | IN_MODIFY);
    }

    #[test]
    fn test_simple_glob_exact() {
        assert!(simple_glob("hello", "hello"));
        assert!(!simple_glob("hello", "world"));
    }

    #[test]
    fn test_simple_glob_star() {
        assert!(simple_glob("*.txt", "file.txt"));
        assert!(!simple_glob("*.txt", "file.log"));
        assert!(simple_glob("file*", "filename.txt"));
    }

    #[test]
    fn test_simple_glob_question() {
        assert!(simple_glob("file?.txt", "file1.txt"));
        assert!(!simple_glob("file?.txt", "file12.txt"));
    }

    #[test]
    fn test_simple_glob_combined() {
        assert!(simple_glob("*.t?t", "file.txt"));
        assert!(simple_glob("*.t?t", "file.tnt"));
        assert!(!simple_glob("*.t?t", "file.log"));
    }

    #[test]
    fn test_should_exclude_no_patterns() {
        assert!(!should_exclude("file.txt", &[], &[]));
    }

    #[test]
    fn test_should_exclude_with_exclude() {
        let exclude = vec!["*.log".to_string()];
        assert!(should_exclude("test.log", &exclude, &[]));
        assert!(!should_exclude("test.txt", &exclude, &[]));
    }

    #[test]
    fn test_should_exclude_with_include() {
        let include = vec!["*.txt".to_string()];
        assert!(!should_exclude("test.txt", &[], &include));
        assert!(should_exclude("test.log", &[], &include));
    }

    #[test]
    fn test_should_exclude_include_overrides() {
        let exclude = vec!["*.txt".to_string()];
        let include = vec!["important*".to_string()];
        // "test.log" doesn't match include pattern, so excluded.
        assert!(should_exclude("test.log", &exclude, &include));
    }

    #[test]
    fn test_format_event_default() {
        let event = InotifyEvent {
            watch_path: "/tmp".to_string(),
            mask: IN_CREATE,
            name: "test.txt".to_string(),
            _cookie: 0,
        };
        let out = format_event(&event, &None, false);
        assert_eq!(out, "/tmp CREATE test.txt");
    }

    #[test]
    fn test_format_event_csv() {
        let event = InotifyEvent {
            watch_path: "/tmp".to_string(),
            mask: IN_MODIFY,
            name: "data.bin".to_string(),
            _cookie: 0,
        };
        let out = format_event(&event, &None, true);
        assert_eq!(out, "/tmp,MODIFY,data.bin");
    }

    #[test]
    fn test_format_event_custom_format() {
        let event = InotifyEvent {
            watch_path: "/var".to_string(),
            mask: IN_DELETE,
            name: "old.log".to_string(),
            _cookie: 0,
        };
        let fmt = Some("%w %e %f".to_string());
        let out = format_event(&event, &fmt, false);
        assert_eq!(out, "/var DELETE old.log");
    }

    #[test]
    fn test_format_event_no_name() {
        let event = InotifyEvent {
            watch_path: "/etc".to_string(),
            mask: IN_DELETE_SELF,
            name: String::new(),
            _cookie: 0,
        };
        let out = format_event(&event, &None, false);
        assert_eq!(out, "/etc DELETE_SELF");
    }

    #[test]
    fn test_format_timestamp_none() {
        let ts = format_timestamp(&None);
        assert!(ts.is_empty());
    }

    #[test]
    fn test_format_timestamp_custom() {
        let ts = format_timestamp(&Some("%H:%M:%S".to_string()));
        assert_eq!(ts, "00:00:00");
    }

    #[test]
    fn test_in_close_composite() {
        assert_eq!(IN_CLOSE, IN_CLOSE_WRITE | IN_CLOSE_NOWRITE);
    }

    #[test]
    fn test_in_move_composite() {
        assert_eq!(IN_MOVE, IN_MOVED_FROM | IN_MOVED_TO);
    }

    #[test]
    fn test_event_stats_tracking() {
        let mut stats = EventStats {
            counts: HashMap::new(),
            total: 0,
        };
        *stats.counts.entry("CREATE".to_string()).or_insert(0) += 1;
        *stats.counts.entry("CREATE".to_string()).or_insert(0) += 1;
        *stats.counts.entry("DELETE".to_string()).or_insert(0) += 1;
        stats.total = 3;

        assert_eq!(stats.counts["CREATE"], 2);
        assert_eq!(stats.counts["DELETE"], 1);
        assert_eq!(stats.total, 3);
    }

    #[test]
    fn test_multiple_flag_names_order() {
        // Flags should be returned in order of bit position.
        let names = event_flag_names(IN_ACCESS | IN_CREATE);
        assert_eq!(names[0], "ACCESS");
        assert_eq!(names[1], "CREATE");
    }

    #[test]
    fn test_event_clone() {
        let event = InotifyEvent {
            watch_path: "/test".to_string(),
            mask: IN_MODIFY,
            name: "file.txt".to_string(),
            _cookie: 42,
        };
        let cloned = event.clone();
        assert_eq!(cloned.watch_path, "/test");
        assert_eq!(cloned.mask, IN_MODIFY);
        assert_eq!(cloned.name, "file.txt");
        assert_eq!(cloned._cookie, 42);
    }
}
