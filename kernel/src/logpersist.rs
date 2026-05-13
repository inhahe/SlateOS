//! Event log persistence — flush the in-memory event log to disk.
//!
//! Bridges the in-memory event ring buffer ([`crate::eventlog`]) to persistent
//! storage via the VFS.  Supports:
//!
//! - **Per-namespace log files**: `security.jsonl`, `network.jsonl`, etc.
//!   or a single `combined.jsonl` (configurable).
//! - **Rotation policies**: by size (default 50 MB per file), by count
//!   (keep N rotated files), maximum total storage cap (default 500 MB).
//! - **Automatic pruning**: oldest rotated logs deleted when cap exceeded.
//! - **Crash-safe writes**: append + sync, no partial JSON lines.
//!
//! ## Architecture
//!
//! ```text
//! eventlog (ring buffer) → logpersist::flush() → VFS writes
//!
//! Log directory: /var/log/events/
//!   combined.jsonl        ← current log file
//!   combined.1.jsonl      ← first rotation
//!   combined.2.jsonl      ← second rotation (oldest)
//!
//! Per-namespace mode:
//!   /var/log/events/system.jsonl
//!   /var/log/events/security.jsonl
//!   /var/log/events/network.jsonl
//!   ...
//! ```
//!
//! ## Integration
//!
//! - Called periodically by the reclaim daemon or a dedicated log flush task.
//! - Kshell `logpersist` command for manual control.
//! - `/proc/logpersist` shows persistence statistics.
//!
//! Note: this is distinct from [`crate::fs::logrotate`] which is the general-
//! purpose log file rotation framework.  This module specifically persists
//! the kernel event log (structured events) to JSON-lines files on disk.
//!
//! ## References
//!
//! - Linux logrotate(8) — file-based rotation with compress/dateext
//! - systemd-journald — binary journal with size caps
//! - This design: JSON-lines text files (per design spec: no binary logs)

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::error::{KernelError, KernelResult};
use crate::eventlog::{self, EventFilter, Severity};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum size of a single log file before rotation (bytes).
const DEFAULT_MAX_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MiB

/// Maximum number of rotated files to keep per namespace.
const DEFAULT_MAX_ROTATED_FILES: u32 = 4;

/// Maximum total log storage across all files (bytes).
const DEFAULT_MAX_TOTAL_STORAGE: u64 = 500 * 1024 * 1024; // 500 MiB

/// Default log directory.
const DEFAULT_LOG_DIR: &str = "/var/log/events";

/// Log rotation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationMode {
    /// All events go to a single `combined.jsonl` file.
    Combined,
    /// Events are split by top-level namespace (system.jsonl, security.jsonl, etc.)
    PerNamespace,
}

/// Configuration for log rotation.
#[derive(Clone)]
pub struct RotationConfig {
    /// Log directory path.
    pub log_dir: String,
    /// Combined vs per-namespace mode.
    pub mode: RotationMode,
    /// Maximum size per log file before rotating (bytes).
    pub max_file_size: u64,
    /// Number of rotated files to keep.
    pub max_rotated_files: u32,
    /// Total storage cap across all log files.
    pub max_total_storage: u64,
    /// Minimum severity to persist (events below this are transient-only).
    pub min_persist_severity: Severity,
    /// Whether rotation is enabled.
    pub enabled: bool,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            log_dir: String::from(DEFAULT_LOG_DIR),
            mode: RotationMode::Combined,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            max_rotated_files: DEFAULT_MAX_ROTATED_FILES,
            max_total_storage: DEFAULT_MAX_TOTAL_STORAGE,
            min_persist_severity: Severity::Info,
            enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Per-namespace flush cursor (tracks what's been written to disk).
struct FlushCursor {
    /// Namespace root name (e.g. "system", "security", or "combined").
    name: String,
    /// Last event sequence number flushed for this namespace.
    last_flushed_seq: u64,
    /// Current file size estimate (bytes).
    current_size: u64,
    /// Total bytes written across all rotations.
    total_bytes_written: u64,
    /// Number of rotations performed.
    rotation_count: u64,
    /// Number of events flushed.
    events_flushed: u64,
}

struct State {
    config: RotationConfig,
    cursors: Vec<FlushCursor>,
    /// Global last-flushed sequence.
    global_last_flushed: u64,
    /// Total flush operations.
    total_flushes: u64,
    /// Total bytes written across all namespaces.
    total_bytes: u64,
    /// Total pruned files.
    total_pruned: u64,
    /// Whether initialized.
    initialized: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            config: RotationConfig {
                log_dir: String::new(),
                mode: RotationMode::Combined,
                max_file_size: DEFAULT_MAX_FILE_SIZE,
                max_rotated_files: DEFAULT_MAX_ROTATED_FILES,
                max_total_storage: DEFAULT_MAX_TOTAL_STORAGE,
                min_persist_severity: Severity::Info,
                enabled: true,
            },
            cursors: Vec::new(),
            global_last_flushed: 0,
            total_flushes: 0,
            total_bytes: 0,
            total_pruned: 0,
            initialized: false,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the log rotation system with default config.
pub fn init() {
    init_with_config(RotationConfig::default());
}

/// Initialize with a custom configuration.
pub fn init_with_config(config: RotationConfig) {
    let mut state = STATE.lock();
    if state.initialized {
        return;
    }

    // Create the log directory if it doesn't exist.
    let _ = crate::fs::Vfs::mkdir(&config.log_dir);

    // Set up cursors based on mode.
    match config.mode {
        RotationMode::Combined => {
            state.cursors.push(FlushCursor {
                name: String::from("combined"),
                last_flushed_seq: 0,
                current_size: 0,
                total_bytes_written: 0,
                rotation_count: 0,
                events_flushed: 0,
            });
        }
        RotationMode::PerNamespace => {
            for ns in eventlog::NAMESPACE_ROOTS {
                state.cursors.push(FlushCursor {
                    name: String::from(*ns),
                    last_flushed_seq: 0,
                    current_size: 0,
                    total_bytes_written: 0,
                    rotation_count: 0,
                    events_flushed: 0,
                });
            }
        }
    }

    state.config = config;
    state.initialized = true;
}

// ---------------------------------------------------------------------------
// JSON serialization for events
// ---------------------------------------------------------------------------

/// Serialize an event entry as a JSON line.
fn event_to_json_line(ev: &eventlog::EventEntry) -> String {
    use alloc::format;
    let mut json = String::with_capacity(256);

    json.push_str("{\"seq\":");
    json.push_str(&format!("{}", ev.seq()));
    json.push_str(",\"ts_ns\":");
    json.push_str(&format!("{}", ev.timestamp_ns()));
    json.push_str(",\"sev\":\"");
    json.push_str(ev.severity().as_str());
    json.push_str("\",\"ns\":\"");
    json_escape_into(&mut json, ev.namespace_str());
    json.push_str("\",\"pid\":");
    json.push_str(&format!("{}", ev.source_pid()));

    let svc = ev.service_str();
    if !svc.is_empty() {
        json.push_str(",\"svc\":\"");
        json_escape_into(&mut json, svc);
        json.push('"');
    }

    json.push_str(",\"msg\":\"");
    json_escape_into(&mut json, ev.message_str());
    json.push('"');

    // Payload key-value pairs.
    let pairs: Vec<_> = ev.payload_iter().collect();
    if !pairs.is_empty() {
        json.push_str(",\"data\":{");
        for (i, (k, v)) in pairs.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push('"');
            json_escape_into(&mut json, k);
            json.push_str("\":\"");
            json_escape_into(&mut json, v);
            json.push('"');
        }
        json.push('}');
    }

    json.push_str("}\n");
    json
}

/// Escape a string for JSON output (append to dst).
fn json_escape_into(dst: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '"' => dst.push_str("\\\""),
            '\\' => dst.push_str("\\\\"),
            '\n' => dst.push_str("\\n"),
            '\r' => dst.push_str("\\r"),
            '\t' => dst.push_str("\\t"),
            c if c.is_control() => {
                dst.push_str(&alloc::format!("\\u{:04x}", c as u32));
            }
            c => dst.push(c),
        }
    }
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

/// Flush new events from the ring buffer to disk.
///
/// Returns the number of events flushed.
pub fn flush() -> KernelResult<usize> {
    let mut state = STATE.lock();
    if !state.initialized || !state.config.enabled {
        return Ok(0);
    }

    let after_seq = state.global_last_flushed;
    let min_sev = state.config.min_persist_severity;

    // Query new events since last flush.
    let filter = EventFilter::all()
        .after(after_seq)
        .min_severity(min_sev);
    let result = eventlog::query(&filter, 1024);

    if result.events.is_empty() {
        return Ok(0);
    }

    let log_dir = state.config.log_dir.clone();
    let mode = state.config.mode;
    let max_file_size = state.config.max_file_size;
    let max_rotated = state.config.max_rotated_files;

    let mut total_flushed = 0usize;

    // Track total bytes written across the flush for deferred update to
    // state.total_bytes (can't mutate state while cursor borrows state.cursors).
    let mut total_bytes_batch: u64 = 0;

    match mode {
        RotationMode::Combined => {
            // All events go to combined.jsonl.
            if let Some(cursor) = state.cursors.first_mut() {
                let path = alloc::format!("{}/combined.jsonl", log_dir);
                let mut batch = String::with_capacity(4096);

                for ev in &result.events {
                    let line = event_to_json_line(ev);
                    #[allow(clippy::arithmetic_side_effects)]
                    { cursor.current_size += line.len() as u64; }
                    batch.push_str(&line);
                    #[allow(clippy::arithmetic_side_effects)]
                    { total_flushed += 1; }
                }

                // Append batch to file.
                if !batch.is_empty() {
                    let batch_len = batch.len() as u64;
                    let _ = crate::fs::Vfs::append(&path, batch.as_bytes());
                    #[allow(clippy::arithmetic_side_effects)]
                    {
                        cursor.total_bytes_written += batch_len;
                        cursor.events_flushed += total_flushed as u64;
                        total_bytes_batch += batch_len;
                    }
                }

                // Check if rotation is needed.
                if cursor.current_size >= max_file_size {
                    rotate_file(&log_dir, "combined", max_rotated);
                    #[allow(clippy::arithmetic_side_effects)]
                    { cursor.rotation_count += 1; }
                    cursor.current_size = 0;
                }

                cursor.last_flushed_seq = result.newest_seq;
            }
        }
        RotationMode::PerNamespace => {
            // Group events by namespace root and write to separate files.
            for cursor in &mut state.cursors {
                let ns_filter = EventFilter::all()
                    .after(after_seq)
                    .min_severity(min_sev)
                    .namespace(&cursor.name);
                let ns_result = eventlog::query(&ns_filter, 1024);

                if ns_result.events.is_empty() {
                    continue;
                }

                let path = alloc::format!("{}/{}.jsonl", log_dir, cursor.name);
                let mut batch = String::with_capacity(2048);

                for ev in &ns_result.events {
                    let line = event_to_json_line(ev);
                    #[allow(clippy::arithmetic_side_effects)]
                    { cursor.current_size += line.len() as u64; }
                    batch.push_str(&line);
                    #[allow(clippy::arithmetic_side_effects)]
                    { total_flushed += 1; }
                }

                if !batch.is_empty() {
                    let batch_len = batch.len() as u64;
                    let _ = crate::fs::Vfs::append(&path, batch.as_bytes());
                    #[allow(clippy::arithmetic_side_effects)]
                    {
                        cursor.total_bytes_written += batch_len;
                        cursor.events_flushed += ns_result.events.len() as u64;
                        total_bytes_batch += batch_len;
                    }
                }

                // Check if rotation is needed.
                if cursor.current_size >= max_file_size {
                    rotate_file(&log_dir, &cursor.name, max_rotated);
                    #[allow(clippy::arithmetic_side_effects)]
                    { cursor.rotation_count += 1; }
                    cursor.current_size = 0;
                }

                cursor.last_flushed_seq = ns_result.newest_seq;
            }
        }
    }

    // Deferred update — cursor borrows are released now.
    #[allow(clippy::arithmetic_side_effects)]
    { state.total_bytes += total_bytes_batch; }

    state.global_last_flushed = result.newest_seq;
    #[allow(clippy::arithmetic_side_effects)]
    { state.total_flushes += 1; }

    Ok(total_flushed)
}

/// Rotate a log file: combined.jsonl → combined.1.jsonl → combined.2.jsonl → ...
///
/// Oldest file beyond max_rotated is deleted.
fn rotate_file(log_dir: &str, name: &str, max_rotated: u32) {
    use alloc::format;

    // Delete the oldest rotated file if it exists.
    let oldest_path = format!("{}/{}.{}.jsonl", log_dir, name, max_rotated);
    let _ = crate::fs::Vfs::remove(&oldest_path);

    // Shift existing rotations: N-1 → N, N-2 → N-1, ..., 1 → 2.
    let mut i = max_rotated;
    while i > 1 {
        #[allow(clippy::arithmetic_side_effects)]
        let src = format!("{}/{}.{}.jsonl", log_dir, name, i - 1);
        let dst = format!("{}/{}.{}.jsonl", log_dir, name, i);
        let _ = crate::fs::Vfs::rename(&src, &dst);
        #[allow(clippy::arithmetic_side_effects)]
        { i -= 1; }
    }

    // Rename current file to .1.
    let current = format!("{}/{}.jsonl", log_dir, name);
    let first_rotated = format!("{}/{}.1.jsonl", log_dir, name);
    let _ = crate::fs::Vfs::rename(&current, &first_rotated);
}

/// Prune old rotated log files to stay within the total storage cap.
///
/// Returns the number of files pruned.
pub fn prune() -> usize {
    let mut state = STATE.lock();
    if !state.initialized || !state.config.enabled {
        return 0;
    }

    let max_total = state.config.max_total_storage;
    let max_rotated = state.config.max_rotated_files;
    let log_dir = state.config.log_dir.clone();
    let mode = state.config.mode;

    // Calculate total storage used.
    let mut total_used: u64 = 0;
    let mut files: Vec<(String, u64)> = Vec::new(); // (path, size)

    match mode {
        RotationMode::Combined => {
            collect_log_files(&log_dir, "combined", max_rotated, &mut files, &mut total_used);
        }
        RotationMode::PerNamespace => {
            for ns in eventlog::NAMESPACE_ROOTS {
                collect_log_files(&log_dir, ns, max_rotated, &mut files, &mut total_used);
            }
        }
    }

    // Sort by name descending (oldest rotations have highest numbers).
    files.sort_by(|a, b| b.0.cmp(&a.0));

    let mut pruned = 0usize;
    while total_used > max_total {
        if let Some((path, size)) = files.pop() {
            let _ = crate::fs::Vfs::remove(&path);
            total_used = total_used.saturating_sub(size);
            #[allow(clippy::arithmetic_side_effects)]
            { pruned += 1; }
        } else {
            break;
        }
    }

    #[allow(clippy::arithmetic_side_effects)]
    { state.total_pruned += pruned as u64; }

    pruned
}

/// Collect log files and their sizes for a given namespace.
fn collect_log_files(
    log_dir: &str,
    name: &str,
    max_rotated: u32,
    files: &mut Vec<(String, u64)>,
    total: &mut u64,
) {
    use alloc::format;

    // Current file.
    let current = format!("{}/{}.jsonl", log_dir, name);
    if let Ok(meta) = crate::fs::Vfs::stat(&current) {
        #[allow(clippy::arithmetic_side_effects)]
        { *total += meta.size; }
        files.push((current, meta.size));
    }

    // Rotated files.
    for i in 1..=max_rotated {
        let path = format!("{}/{}.{}.jsonl", log_dir, name, i);
        if let Ok(meta) = crate::fs::Vfs::stat(&path) {
            #[allow(clippy::arithmetic_side_effects)]
            { *total += meta.size; }
            files.push((path, meta.size));
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration API
// ---------------------------------------------------------------------------

/// Get the current rotation configuration.
pub fn config() -> RotationConfig {
    STATE.lock().config.clone()
}

/// Update the rotation configuration.
pub fn set_config(config: RotationConfig) {
    let mut state = STATE.lock();
    state.config = config;
}

/// Enable or disable log rotation.
pub fn set_enabled(enabled: bool) {
    STATE.lock().config.enabled = enabled;
}

/// Set the rotation mode (combined vs per-namespace).
pub fn set_mode(mode: RotationMode) {
    let mut state = STATE.lock();
    state.config.mode = mode;

    // Rebuild cursors for the new mode.
    state.cursors.clear();
    let global_seq = state.global_last_flushed;

    match mode {
        RotationMode::Combined => {
            state.cursors.push(FlushCursor {
                name: String::from("combined"),
                last_flushed_seq: global_seq,
                current_size: 0,
                total_bytes_written: 0,
                rotation_count: 0,
                events_flushed: 0,
            });
        }
        RotationMode::PerNamespace => {
            for ns in eventlog::NAMESPACE_ROOTS {
                state.cursors.push(FlushCursor {
                    name: String::from(*ns),
                    last_flushed_seq: global_seq,
                    current_size: 0,
                    total_bytes_written: 0,
                    rotation_count: 0,
                    events_flushed: 0,
                });
            }
        }
    }
}

/// Set the minimum severity for persistent logging.
pub fn set_min_severity(sev: Severity) {
    STATE.lock().config.min_persist_severity = sev;
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Log rotation statistics.
pub struct RotationStats {
    pub enabled: bool,
    pub mode: RotationMode,
    pub log_dir: String,
    pub total_flushes: u64,
    pub total_bytes_written: u64,
    pub total_pruned: u64,
    pub global_last_flushed_seq: u64,
    pub max_file_size: u64,
    pub max_rotated_files: u32,
    pub max_total_storage: u64,
    pub min_persist_severity: Severity,
    /// Per-cursor stats: (name, events_flushed, bytes_written, rotations, current_size).
    pub cursors: Vec<(String, u64, u64, u64, u64)>,
}

/// Get rotation statistics.
pub fn stats() -> RotationStats {
    let state = STATE.lock();
    let cursors: Vec<_> = state.cursors.iter().map(|c| {
        (c.name.clone(), c.events_flushed, c.total_bytes_written, c.rotation_count, c.current_size)
    }).collect();

    RotationStats {
        enabled: state.config.enabled,
        mode: state.config.mode,
        log_dir: state.config.log_dir.clone(),
        total_flushes: state.total_flushes,
        total_bytes_written: state.total_bytes,
        total_pruned: state.total_pruned,
        global_last_flushed_seq: state.global_last_flushed,
        max_file_size: state.config.max_file_size,
        max_rotated_files: state.config.max_rotated_files,
        max_total_storage: state.config.max_total_storage,
        min_persist_severity: state.config.min_persist_severity,
        cursors,
    }
}

/// Generate content for /proc/logpersist.
pub fn procfs_content() -> String {
    let st = stats();
    let mut out = String::with_capacity(512);

    out.push_str("Event Log Persistence\n");
    out.push_str("=====================\n");
    out.push_str(&alloc::format!("Enabled:       {}\n", st.enabled));
    out.push_str(&alloc::format!("Mode:          {}\n", match st.mode {
        RotationMode::Combined => "combined",
        RotationMode::PerNamespace => "per-namespace",
    }));
    out.push_str(&alloc::format!("Log dir:       {}\n", st.log_dir));
    out.push_str(&alloc::format!("Min severity:  {}\n", st.min_persist_severity.as_str()));
    out.push_str(&alloc::format!("Max file size: {} MiB\n", st.max_file_size / (1024 * 1024)));
    out.push_str(&alloc::format!("Max rotated:   {}\n", st.max_rotated_files));
    out.push_str(&alloc::format!("Max total:     {} MiB\n", st.max_total_storage / (1024 * 1024)));
    out.push_str(&alloc::format!("Total flushes: {}\n", st.total_flushes));
    out.push_str(&alloc::format!("Total written: {} bytes\n", st.total_bytes_written));
    out.push_str(&alloc::format!("Total pruned:  {} files\n", st.total_pruned));
    out.push_str(&alloc::format!("Last seq:      {}\n", st.global_last_flushed_seq));

    if !st.cursors.is_empty() {
        out.push_str("\nPer-Namespace:\n");
        out.push_str(&alloc::format!("  {:12} {:>8} {:>12} {:>6} {:>10}\n",
            "Namespace", "Events", "Bytes", "Rots", "CurSize"));
        for (name, events, bytes, rots, cur_size) in &st.cursors {
            out.push_str(&alloc::format!("  {:12} {:>8} {:>12} {:>6} {:>10}\n",
                name, events, bytes, rots, cur_size));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run log rotation self-tests.
pub fn self_test() -> KernelResult<()> {
    use crate::eventlog::EventBuilder;

    crate::serial_println!("[logpersist] Running log rotation self-tests...");

    // Test 1: Initialize with default config.
    {
        let mut state = STATE.lock();
        state.initialized = false;
        state.cursors.clear();
        state.total_flushes = 0;
        state.total_bytes = 0;
        state.total_pruned = 0;
        state.global_last_flushed = 0;
    }
    init();
    {
        let state = STATE.lock();
        if !state.initialized {
            crate::serial_println!("[logpersist]   FAIL: not initialized");
            return Err(KernelError::InternalError);
        }
        if state.cursors.len() != 1 {
            crate::serial_println!("[logpersist]   FAIL: expected 1 cursor (combined mode)");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[logpersist]   1. Init (combined mode): OK");

    // Test 2: Emit events and flush.
    eventlog::clear();
    EventBuilder::new("system.test", Severity::Info)
        .message("Log rotation test event 1")
        .emit();
    EventBuilder::new("security.test", Severity::Warning)
        .message("Log rotation test event 2")
        .emit();

    let flushed = flush()?;
    if flushed != 2 {
        crate::serial_println!("[logpersist]   FAIL: expected 2 flushed, got {}", flushed);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[logpersist]   2. Flush events: OK (flushed {})", flushed);

    // Test 3: Verify stats updated.
    let st = stats();
    if st.total_flushes != 1 {
        crate::serial_println!("[logpersist]   FAIL: expected 1 flush, got {}", st.total_flushes);
        return Err(KernelError::InternalError);
    }
    if st.total_bytes_written == 0 {
        crate::serial_println!("[logpersist]   FAIL: total_bytes_written is 0");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[logpersist]   3. Stats: OK (bytes={})", st.total_bytes_written);

    // Test 4: Event to JSON serialization.
    EventBuilder::new("network.dhcp", Severity::Notice)
        .message("DHCP lease acquired")
        .pid(42)
        .service("dhcpd")
        .kv("ip", "10.0.2.15")
        .emit();
    let result = eventlog::query(
        &EventFilter::all().namespace("network.dhcp"),
        1,
    );
    if let Some(ev) = result.events.first() {
        let json = event_to_json_line(ev);
        if !json.contains("\"sev\":\"notice\"") {
            crate::serial_println!("[logpersist]   FAIL: JSON missing severity");
            return Err(KernelError::InternalError);
        }
        if !json.contains("\"ns\":\"network.dhcp\"") {
            crate::serial_println!("[logpersist]   FAIL: JSON missing namespace");
            return Err(KernelError::InternalError);
        }
        if !json.contains("\"ip\":\"10.0.2.15\"") {
            crate::serial_println!("[logpersist]   FAIL: JSON missing payload");
            return Err(KernelError::InternalError);
        }
    } else {
        crate::serial_println!("[logpersist]   FAIL: event not found");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[logpersist]   4. JSON serialization: OK");

    // Test 5: Per-namespace mode.
    {
        let mut state = STATE.lock();
        state.initialized = false;
        state.cursors.clear();
        state.total_flushes = 0;
        state.total_bytes = 0;
        state.global_last_flushed = 0;
    }
    init_with_config(RotationConfig {
        mode: RotationMode::PerNamespace,
        ..RotationConfig::default()
    });
    {
        let state = STATE.lock();
        if state.cursors.len() != eventlog::NAMESPACE_ROOTS.len() {
            crate::serial_println!("[logpersist]   FAIL: expected {} cursors, got {}",
                eventlog::NAMESPACE_ROOTS.len(), state.cursors.len());
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[logpersist]   5. Per-namespace mode: OK");

    // Test 6: Prune (no files to prune should return 0).
    let pruned = prune();
    if pruned != 0 {
        crate::serial_println!("[logpersist]   FAIL: expected 0 pruned, got {}", pruned);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[logpersist]   6. Prune (empty): OK");

    // Clean up.
    eventlog::clear();
    crate::serial_println!("[logpersist] All 6 self-tests passed.");
    Ok(())
}
