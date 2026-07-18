//! System-wide event logging service — structured, hierarchical, queryable.
//!
//! Provides the kernel-resident event collection infrastructure that all
//! subsystems and services use to log significant events.  This is the
//! foundation for the Event Viewer application and the `eventlog` kshell
//! command.
//!
//! ## Design (from roadmap 2.6)
//!
//! - **Hierarchical event namespace taxonomy** mirroring hook namespaces:
//!   `system.*`, `process.*`, `security.*`, `network.*`, `storage.*`,
//!   `filesystem.*`, `service.*`, `driver.*`, `application.*`.
//! - **Six severity levels**: debug, info, notice, warning, error, critical.
//! - **Structured fields**: timestamp (ns), namespace, severity, source PID,
//!   source service name, source executable path, message, key-value payload.
//! - **Ring buffer** in kernel for early-boot events (before logging service
//!   starts) — 4096 entries.
//! - **Query/filter API**: filter by namespace prefix, severity range, time
//!   range, source PID/service, full-text search in message and payload.
//! - **Streaming mode**: consumers can tail new events matching a filter.
//!
//! ## Relationship to klog
//!
//! `klog` is a lightweight kernel debug log (200-byte messages, 256-entry
//! buffer, module-level granularity).  `eventlog` is the system-wide event
//! service — larger entries, richer metadata, hierarchical namespaces, and
//! query capabilities.  Both coexist: klog for kernel printf-style debugging,
//! eventlog for structured system events visible to userspace.
//!
//! ## Thread Safety
//!
//! The event ring buffer is protected by a spinlock.  The lock is held
//! only for the duration of a single write or read operation.  Event
//! emission from interrupt context is safe (spinlock, not sleeping mutex).
//!
//! ## References
//!
//! - Windows Event Log (structured events with providers, levels, channels)
//! - systemd journal (structured fields, cursor-based queries)
//! - macOS Unified Logging (subsystem + category hierarchy)

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Severity levels
// ---------------------------------------------------------------------------

/// Event severity level, ordered from least to most severe.
///
/// Six levels per the design spec — more granular than klog's five:
/// `Notice` fills the gap between informational and warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Severity {
    /// Very detailed debugging information.  Typically disabled in
    /// production.  Use for tracing internal state transitions.
    Debug = 0,
    /// Normal operational messages.  "Service started", "DHCP lease
    /// acquired", "User logged in".
    Info = 1,
    /// Noteworthy but non-problematic conditions.  "Unusual but valid
    /// configuration detected", "Automatic failover occurred".
    Notice = 2,
    /// Something unexpected but the system continues normally.
    /// "Retry succeeded after transient failure", "Disk nearing capacity".
    Warning = 3,
    /// A significant failure affecting one operation or service.
    /// "Service crashed", "DNS resolution failed", "Disk I/O error".
    Error = 4,
    /// A failure that may affect the entire system.  "Out of memory",
    /// "Root filesystem corruption detected", "Kernel panic imminent".
    Critical = 5,
}

impl Severity {
    /// String representation for display and JSON output.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Notice => "notice",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
        }
    }

    /// Parse a severity from its string name (case-insensitive first char).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        let first = s.as_bytes().first().copied()?;
        match first {
            b'd' | b'D' => Some(Self::Debug),
            b'i' | b'I' => Some(Self::Info),
            b'n' | b'N' => Some(Self::Notice),
            b'w' | b'W' => Some(Self::Warning),
            b'e' | b'E' => Some(Self::Error),
            b'c' | b'C' => Some(Self::Critical),
            _ => None,
        }
    }

    /// Numeric value for comparison and serialization.
    pub const fn numeric(self) -> u8 {
        self as u8
    }
}

// ---------------------------------------------------------------------------
// Event namespace
// ---------------------------------------------------------------------------

/// Maximum length of a namespace string (e.g. "filesystem.corruption").
const MAX_NAMESPACE_LEN: usize = 64;

/// Maximum length of a service name.
const MAX_SERVICE_LEN: usize = 48;

/// Maximum length of the message field.
const MAX_MESSAGE_LEN: usize = 256;

/// Maximum number of key-value payload pairs per event.
const MAX_PAYLOAD_PAIRS: usize = 8;

/// Maximum length of a payload key.
const MAX_PAYLOAD_KEY_LEN: usize = 32;

/// Maximum length of a payload value.
const MAX_PAYLOAD_VAL_LEN: usize = 64;

/// Top-level namespace categories per the design spec.
///
/// Used for validation and documentation — actual namespaces are
/// stored as strings to allow arbitrary sub-categories.
pub const NAMESPACE_ROOTS: &[&str] = &[
    "system",      // boot, shutdown, sleep/wake, OOM, hardware errors
    "process",     // launch, exit, suspend/resume, priority change
    "security",    // login/logout, capability grant/revoke, auth failures
    "network",     // interface up/down, DHCP, DNS, firewall, connections
    "storage",     // mount/unmount, partition, disk errors, SMART
    "filesystem",  // permission changes, quota exceeded, corruption
    "service",     // start/stop/crash/restart, dependency, activation
    "driver",      // load/unload, device attach/detach, errors
    "application", // app-defined events via logging API
];

/// Validate that a namespace string starts with a known root.
///
/// Returns `true` for any string that begins with a valid root
/// followed by nothing or a dot separator.
pub fn is_valid_namespace(ns: &str) -> bool {
    for root in NAMESPACE_ROOTS {
        if ns == *root {
            return true;
        }
        if ns.starts_with(root) {
            // Must be followed by '.' for sub-category
            if ns.as_bytes().get(root.len()).copied() == Some(b'.') {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Payload key-value pair
// ---------------------------------------------------------------------------

/// A single key-value pair in an event's structured payload.
#[derive(Clone)]
pub struct PayloadPair {
    /// Key bytes (UTF-8).
    key: [u8; MAX_PAYLOAD_KEY_LEN],
    /// Key length.
    key_len: u8,
    /// Value bytes (UTF-8).
    value: [u8; MAX_PAYLOAD_VAL_LEN],
    /// Value length.
    value_len: u8,
}

impl PayloadPair {
    const fn zeroed() -> Self {
        Self {
            key: [0; MAX_PAYLOAD_KEY_LEN],
            key_len: 0,
            value: [0; MAX_PAYLOAD_VAL_LEN],
            value_len: 0,
        }
    }

    fn set(&mut self, key: &str, value: &str) {
        let kb = key.as_bytes();
        let kl = kb.len().min(MAX_PAYLOAD_KEY_LEN);
        self.key[..kl].copy_from_slice(kb.get(..kl).unwrap_or(&[]));
        self.key_len = kl as u8;

        let vb = value.as_bytes();
        let vl = vb.len().min(MAX_PAYLOAD_VAL_LEN);
        self.value[..vl].copy_from_slice(vb.get(..vl).unwrap_or(&[]));
        self.value_len = vl as u8;
    }

    fn key_str(&self) -> &str {
        let len = self.key_len as usize;
        // SAFETY: We only write valid UTF-8.
        unsafe { core::str::from_utf8_unchecked(self.key.get(..len).unwrap_or(&[])) }
    }

    fn value_str(&self) -> &str {
        let len = self.value_len as usize;
        // SAFETY: We only write valid UTF-8.
        unsafe { core::str::from_utf8_unchecked(self.value.get(..len).unwrap_or(&[])) }
    }

    fn is_empty(&self) -> bool {
        self.key_len == 0
    }
}

// ---------------------------------------------------------------------------
// Event entry
// ---------------------------------------------------------------------------

/// A single structured event entry in the ring buffer.
#[derive(Clone)]
pub struct EventEntry {
    /// Monotonically increasing sequence number.
    seq: u64,
    /// Timestamp in nanoseconds since boot (from HPET if available,
    /// else APIC tick-derived).
    timestamp_ns: u64,
    /// Severity level.
    severity: Severity,
    /// Hierarchical namespace (e.g. "security.login", "service.crash").
    namespace: [u8; MAX_NAMESPACE_LEN],
    namespace_len: u8,
    /// Source process ID (0 = kernel).
    source_pid: u32,
    /// Source service name (empty for non-service sources).
    source_service: [u8; MAX_SERVICE_LEN],
    source_service_len: u8,
    /// Human-readable message.
    message: [u8; MAX_MESSAGE_LEN],
    message_len: u16,
    /// Structured key-value payload pairs.
    payload: [PayloadPair; MAX_PAYLOAD_PAIRS],
    /// Number of valid payload pairs.
    payload_count: u8,
}

impl EventEntry {
    const fn zeroed() -> Self {
        Self {
            seq: 0,
            timestamp_ns: 0,
            severity: Severity::Debug,
            namespace: [0; MAX_NAMESPACE_LEN],
            namespace_len: 0,
            source_pid: 0,
            source_service: [0; MAX_SERVICE_LEN],
            source_service_len: 0,
            message: [0; MAX_MESSAGE_LEN],
            message_len: 0,
            payload: [const { PayloadPair::zeroed() }; MAX_PAYLOAD_PAIRS],
            payload_count: 0,
        }
    }

    /// Get the namespace as a string slice.
    pub fn namespace_str(&self) -> &str {
        let len = self.namespace_len as usize;
        // SAFETY: We only write valid UTF-8.
        unsafe { core::str::from_utf8_unchecked(self.namespace.get(..len).unwrap_or(&[])) }
    }

    /// Get the service name as a string slice (empty if not a service).
    pub fn service_str(&self) -> &str {
        let len = self.source_service_len as usize;
        // SAFETY: We only write valid UTF-8.
        unsafe { core::str::from_utf8_unchecked(self.source_service.get(..len).unwrap_or(&[])) }
    }

    /// Get the message as a string slice.
    pub fn message_str(&self) -> &str {
        let len = self.message_len as usize;
        // SAFETY: We only write valid UTF-8.
        unsafe { core::str::from_utf8_unchecked(self.message.get(..len).unwrap_or(&[])) }
    }

    /// Get the severity.
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// Get the sequence number.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Get the timestamp in nanoseconds.
    pub fn timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }

    /// Get the source PID.
    pub fn source_pid(&self) -> u32 {
        self.source_pid
    }

    /// Iterate over non-empty payload pairs.
    pub fn payload_iter(&self) -> impl Iterator<Item = (&str, &str)> {
        let count = self.payload_count as usize;
        self.payload.iter().take(count).filter(|p| !p.is_empty()).map(|p| (p.key_str(), p.value_str()))
    }

    /// Check if namespace matches a prefix (for filtering).
    ///
    /// `"security"` matches `"security"`, `"security.login"`, etc.
    /// `"security.login"` matches `"security.login"` but not `"security.logout"`.
    pub fn namespace_matches(&self, prefix: &str) -> bool {
        let ns = self.namespace_str();
        if ns == prefix {
            return true;
        }
        // prefix must be a proper prefix followed by '.'
        if ns.starts_with(prefix) {
            ns.as_bytes().get(prefix.len()).copied() == Some(b'.')
        } else {
            false
        }
    }

    /// Check if the message contains a substring (case-insensitive).
    pub fn message_contains(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let msg = self.message_str();
        // Simple case-insensitive substring search
        let needle_lower: Vec<u8> = needle.bytes().map(|b| {
            if b.is_ascii_uppercase() { b.wrapping_add(32) } else { b }
        }).collect();
        let msg_lower: Vec<u8> = msg.bytes().map(|b| {
            if b.is_ascii_uppercase() { b.wrapping_add(32) } else { b }
        }).collect();
        // Sliding window search
        if needle_lower.len() > msg_lower.len() {
            return false;
        }
        let limit = msg_lower.len().saturating_sub(needle_lower.len());
        for i in 0..=limit {
            if msg_lower.get(i..i.wrapping_add(needle_lower.len())) == Some(needle_lower.as_slice()) {
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Event builder (ergonomic API for constructing events)
// ---------------------------------------------------------------------------

/// Builder for constructing event entries before submission.
///
/// # Usage
///
/// ```ignore
/// use crate::eventlog::{EventBuilder, Severity};
///
/// EventBuilder::new("service.crash", Severity::Error)
///     .pid(42)
///     .service("network-manager")
///     .message("Service exited with code 137")
///     .kv("exit_code", "137")
///     .kv("restart_count", "3")
///     .emit();
/// ```
pub struct EventBuilder {
    entry: EventEntry,
}

impl EventBuilder {
    /// Create a new event builder with the given namespace and severity.
    pub fn new(namespace: &str, severity: Severity) -> Self {
        let mut entry = EventEntry::zeroed();
        entry.severity = severity;

        let ns_bytes = namespace.as_bytes();
        let ns_len = ns_bytes.len().min(MAX_NAMESPACE_LEN);
        entry.namespace[..ns_len].copy_from_slice(ns_bytes.get(..ns_len).unwrap_or(&[]));
        entry.namespace_len = ns_len as u8;

        Self { entry }
    }

    /// Set the source process ID.
    pub fn pid(mut self, pid: u32) -> Self {
        self.entry.source_pid = pid;
        self
    }

    /// Set the source service name.
    pub fn service(mut self, name: &str) -> Self {
        let bytes = name.as_bytes();
        let len = bytes.len().min(MAX_SERVICE_LEN);
        self.entry.source_service[..len].copy_from_slice(bytes.get(..len).unwrap_or(&[]));
        self.entry.source_service_len = len as u8;
        self
    }

    /// Set the human-readable message.
    pub fn message(mut self, msg: &str) -> Self {
        let bytes = msg.as_bytes();
        let len = bytes.len().min(MAX_MESSAGE_LEN);
        self.entry.message[..len].copy_from_slice(bytes.get(..len).unwrap_or(&[]));
        self.entry.message_len = len as u16;
        self
    }

    /// Set the message from format arguments (avoids heap allocation).
    pub fn message_fmt(mut self, args: core::fmt::Arguments<'_>) -> Self {
        let mut writer = MsgWriter {
            buf: &mut self.entry.message,
            pos: 0,
        };
        let _ = core::fmt::write(&mut writer, args);
        self.entry.message_len = writer.pos.min(MAX_MESSAGE_LEN) as u16;
        self
    }

    /// Add a key-value pair to the structured payload.
    pub fn kv(mut self, key: &str, value: &str) -> Self {
        let idx = self.entry.payload_count as usize;
        if idx < MAX_PAYLOAD_PAIRS {
            self.entry.payload[idx].set(key, value);
            self.entry.payload_count = self.entry.payload_count.wrapping_add(1);
        }
        self
    }

    /// Submit the event to the global event log.
    pub fn emit(self) {
        emit_event(self.entry);
    }
}

/// Helper for writing formatted messages into a fixed buffer.
struct MsgWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl core::fmt::Write for MsgWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let avail = self.buf.len().saturating_sub(self.pos);
        let len = bytes.len().min(avail);
        if let Some(dest) = self.buf.get_mut(self.pos..self.pos.wrapping_add(len)) {
            if let Some(src) = bytes.get(..len) {
                dest.copy_from_slice(src);
            }
        }
        #[allow(clippy::arithmetic_side_effects)]
        { self.pos += len; }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

/// Number of entries in the event ring buffer.
///
/// 4096 entries — large enough for sustained logging during boot and
/// normal operation.  At ~600 bytes per entry, this uses ~2.4 MiB.
const RING_SIZE: usize = 4096;

/// The global event ring buffer.
struct EventRing {
    entries: Vec<EventEntry>,
    /// Write index (next slot to write).
    write_idx: usize,
    /// Total events written (for sequence numbers and overflow detection).
    total_written: u64,
    /// Total events dropped due to rate limiting or buffer full.
    total_dropped: u64,
    /// Per-severity counters.
    severity_counts: [u64; 6],
    /// Per-namespace-root counters (indexed by position in NAMESPACE_ROOTS).
    namespace_counts: [u64; 9],
    /// Minimum severity for serial echo (events at or above this level
    /// are also printed to serial for early debugging).
    serial_echo_level: Severity,
    /// Whether the ring has been initialized (lazy init on first use).
    initialized: bool,
}

impl EventRing {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
            write_idx: 0,
            total_written: 0,
            total_dropped: 0,
            severity_counts: [0; 6],
            namespace_counts: [0; 9],
            serial_echo_level: Severity::Warning,
            initialized: false,
        }
    }

    /// Ensure the ring buffer is allocated.
    fn ensure_init(&mut self) {
        if !self.initialized {
            self.entries.reserve(RING_SIZE);
            for _ in 0..RING_SIZE {
                self.entries.push(EventEntry::zeroed());
            }
            self.initialized = true;
        }
    }

    /// Write an event entry.  Returns the assigned sequence number.
    fn write(&mut self, mut entry: EventEntry) -> u64 {
        self.ensure_init();

        let seq = self.total_written;
        entry.seq = seq;
        entry.timestamp_ns = current_timestamp_ns();

        // Update severity counter.
        let sev_idx = entry.severity.numeric() as usize;
        if sev_idx < self.severity_counts.len() {
            #[allow(clippy::arithmetic_side_effects)]
            { self.severity_counts[sev_idx] = self.severity_counts[sev_idx].wrapping_add(1); }
        }

        // Update namespace root counter.
        let ns = entry.namespace_str();
        for (i, root) in NAMESPACE_ROOTS.iter().enumerate() {
            if ns == *root || (ns.starts_with(root) && ns.as_bytes().get(root.len()).copied() == Some(b'.')) {
                if i < self.namespace_counts.len() {
                    #[allow(clippy::arithmetic_side_effects)]
                    { self.namespace_counts[i] = self.namespace_counts[i].wrapping_add(1); }
                }
                break;
            }
        }

        // Echo to serial if severity is high enough.
        if entry.severity >= self.serial_echo_level {
            echo_serial(&entry);
        }

        // Store in ring buffer.
        if let Some(slot) = self.entries.get_mut(self.write_idx) {
            *slot = entry;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            self.write_idx = (self.write_idx + 1) % RING_SIZE;
            self.total_written = self.total_written.wrapping_add(1);
        }

        seq
    }
}

/// Echo a high-severity event to the serial console.
fn echo_serial(entry: &EventEntry) {
    crate::serial_println!(
        "[eventlog] [{}/{}] pid={} svc={}: {}",
        entry.severity().as_str(),
        entry.namespace_str(),
        entry.source_pid(),
        if entry.source_service_len > 0 { entry.service_str() } else { "-" },
        entry.message_str(),
    );
}

/// Get the current timestamp in nanoseconds since boot.
fn current_timestamp_ns() -> u64 {
    // HPET provides nanosecond-precision elapsed time.
    // It returns 0 before HPET is initialized, so fall back to APIC ticks.
    let ns = crate::hpet::elapsed_ns();
    if ns > 0 {
        ns
    } else {
        // APIC timer at 100 Hz → each tick = 10 ms = 10_000_000 ns.
        #[allow(clippy::arithmetic_side_effects)]
        { crate::apic::tick_count() * 10_000_000 }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static EVENT_RING: Mutex<EventRing> = Mutex::new(EventRing::new());

/// Global sequence counter — atomic for lock-free checks by consumers
/// that want to know if new events are available without locking.
static GLOBAL_SEQ: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — emit events
// ---------------------------------------------------------------------------

/// Submit a pre-built event entry to the global event log.
pub fn emit_event(entry: EventEntry) {
    let mut ring = EVENT_RING.lock();
    let seq = ring.write(entry);
    drop(ring);
    GLOBAL_SEQ.store(seq.wrapping_add(1), Ordering::Relaxed);
}

/// Convenience: emit a simple event with just namespace, severity, and message.
pub fn emit(namespace: &str, severity: Severity, message: &str) {
    EventBuilder::new(namespace, severity)
        .message(message)
        .emit();
}

/// Convenience: emit a formatted event.
pub fn emit_fmt(namespace: &str, severity: Severity, args: core::fmt::Arguments<'_>) {
    EventBuilder::new(namespace, severity)
        .message_fmt(args)
        .emit();
}

// ---------------------------------------------------------------------------
// Convenience macros
// ---------------------------------------------------------------------------

/// Emit a system event with format arguments.
///
/// ```ignore
/// syslog!("service.crash", Error, "Service {} crashed (code {})", name, code);
/// syslog!("system.boot", Info, "Kernel boot complete in {} ms", ms);
/// ```
#[macro_export]
macro_rules! syslog {
    ($ns:expr, $severity:ident, $($arg:tt)*) => {{
        $crate::eventlog::emit_fmt(
            $ns,
            $crate::eventlog::Severity::$severity,
            format_args!($($arg)*),
        );
    }};
}

// ---------------------------------------------------------------------------
// Public API — query events
// ---------------------------------------------------------------------------

/// Filter criteria for event queries.
#[derive(Clone)]
pub struct EventFilter {
    /// Namespace prefix (empty = all namespaces).
    pub namespace_prefix: Option<String>,
    /// Minimum severity (inclusive).  `None` = all severities.
    pub min_severity: Option<Severity>,
    /// Maximum severity (inclusive).  `None` = all severities.
    pub max_severity: Option<Severity>,
    /// Only events with `timestamp_ns >= since_ns`.
    pub since_ns: Option<u64>,
    /// Only events with `timestamp_ns <= until_ns`.
    pub until_ns: Option<u64>,
    /// Only events from this PID.
    pub source_pid: Option<u32>,
    /// Only events from this service name (prefix match).
    pub source_service: Option<String>,
    /// Case-insensitive substring search in message field.
    pub text_search: Option<String>,
    /// Only events with `seq > after_seq` (for pagination / streaming).
    pub after_seq: Option<u64>,
}

impl EventFilter {
    /// Create a filter that matches everything.
    pub fn all() -> Self {
        Self {
            namespace_prefix: None,
            min_severity: None,
            max_severity: None,
            since_ns: None,
            until_ns: None,
            source_pid: None,
            source_service: None,
            text_search: None,
            after_seq: None,
        }
    }

    /// Filter by namespace prefix.
    pub fn namespace(mut self, prefix: &str) -> Self {
        self.namespace_prefix = Some(String::from(prefix));
        self
    }

    /// Filter by minimum severity.
    pub fn min_severity(mut self, sev: Severity) -> Self {
        self.min_severity = Some(sev);
        self
    }

    /// Filter events after a given sequence number.
    pub fn after(mut self, seq: u64) -> Self {
        self.after_seq = Some(seq);
        self
    }

    /// Filter by source PID.
    pub fn pid(mut self, pid: u32) -> Self {
        self.source_pid = Some(pid);
        self
    }

    /// Filter by service name.
    pub fn service(mut self, name: &str) -> Self {
        self.source_service = Some(String::from(name));
        self
    }

    /// Full-text search in message.
    pub fn search(mut self, text: &str) -> Self {
        self.text_search = Some(String::from(text));
        self
    }

    /// Check if an event matches this filter.
    fn matches(&self, entry: &EventEntry) -> bool {
        // Namespace prefix match.
        if let Some(ref prefix) = self.namespace_prefix {
            if !entry.namespace_matches(prefix) {
                return false;
            }
        }

        // Severity range.
        if let Some(min) = self.min_severity {
            if entry.severity < min {
                return false;
            }
        }
        if let Some(max) = self.max_severity {
            if entry.severity > max {
                return false;
            }
        }

        // Time range.
        if let Some(since) = self.since_ns {
            if entry.timestamp_ns < since {
                return false;
            }
        }
        if let Some(until) = self.until_ns {
            if entry.timestamp_ns > until {
                return false;
            }
        }

        // Source PID.
        if let Some(pid) = self.source_pid {
            if entry.source_pid != pid {
                return false;
            }
        }

        // Source service name (prefix match).
        if let Some(ref svc) = self.source_service {
            let entry_svc = entry.service_str();
            if !entry_svc.starts_with(svc.as_str()) {
                return false;
            }
        }

        // Sequence number.
        if let Some(after) = self.after_seq {
            if entry.seq <= after {
                return false;
            }
        }

        // Text search in message.
        if let Some(ref text) = self.text_search {
            if !entry.message_contains(text) {
                return false;
            }
        }

        true
    }
}

/// Query result: a collection of matching events.
pub struct QueryResult {
    /// Matching events (oldest first).
    pub events: Vec<EventEntry>,
    /// Newest sequence number seen (for subsequent streaming queries).
    pub newest_seq: u64,
    /// Total events scanned.
    pub scanned: u64,
    /// Total events matching the filter.
    pub matched: u64,
}

/// Query events matching the given filter.
///
/// Returns up to `max_results` events, oldest first.
pub fn query(filter: &EventFilter, max_results: usize) -> QueryResult {
    let ring = EVENT_RING.lock();

    if !ring.initialized || ring.total_written == 0 {
        return QueryResult {
            events: Vec::new(),
            newest_seq: 0,
            scanned: 0,
            matched: 0,
        };
    }

    // Determine scan range.
    let oldest_seq = ring.total_written.saturating_sub(RING_SIZE as u64);

    let start_seq = if let Some(after) = filter.after_seq {
        // Start scanning from after_seq + 1, but not before oldest.
        after.wrapping_add(1).max(oldest_seq)
    } else {
        oldest_seq
    };

    let mut result = QueryResult {
        events: Vec::new(),
        newest_seq: start_seq.saturating_sub(1),
        scanned: 0,
        matched: 0,
    };

    let mut seq = start_seq;
    while seq < ring.total_written {
        let idx = (seq as usize) % RING_SIZE;
        if let Some(entry) = ring.entries.get(idx) {
            if entry.seq == seq {
                #[allow(clippy::arithmetic_side_effects)]
                { result.scanned += 1; }

                if filter.matches(entry) {
                    #[allow(clippy::arithmetic_side_effects)]
                    { result.matched += 1; }

                    if result.events.len() < max_results {
                        result.events.push(entry.clone());
                    }
                }

                result.newest_seq = seq;
            }
        }

        #[allow(clippy::arithmetic_side_effects)]
        { seq += 1; }
    }

    result
}

/// Get the total number of events ever written (including overwritten).
pub fn total_events() -> u64 {
    let ring = EVENT_RING.lock();
    ring.total_written
}

/// Get the current global sequence number (lock-free).
///
/// Consumers can poll this to detect new events without locking.
pub fn current_seq() -> u64 {
    GLOBAL_SEQ.load(Ordering::Relaxed)
}

/// Get the number of events currently in the ring buffer.
pub fn buffered_count() -> u64 {
    let ring = EVENT_RING.lock();
    if ring.total_written > RING_SIZE as u64 {
        RING_SIZE as u64
    } else {
        ring.total_written
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Aggregate statistics about the event log.
pub struct EventLogStats {
    /// Total events ever written.
    pub total_written: u64,
    /// Events currently in ring buffer.
    pub buffered: u64,
    /// Events dropped.
    pub dropped: u64,
    /// Per-severity counts.
    pub by_severity: [(Severity, u64); 6],
    /// Per-namespace-root counts.
    pub by_namespace: Vec<(&'static str, u64)>,
    /// Serial echo threshold.
    pub serial_echo_level: Severity,
}

/// Get aggregate statistics.
pub fn stats() -> EventLogStats {
    let ring = EVENT_RING.lock();

    let buffered = if ring.total_written > RING_SIZE as u64 {
        RING_SIZE as u64
    } else {
        ring.total_written
    };

    let by_severity = [
        (Severity::Debug, ring.severity_counts[0]),
        (Severity::Info, ring.severity_counts[1]),
        (Severity::Notice, ring.severity_counts[2]),
        (Severity::Warning, ring.severity_counts[3]),
        (Severity::Error, ring.severity_counts[4]),
        (Severity::Critical, ring.severity_counts[5]),
    ];

    let mut by_namespace = Vec::new();
    for (i, root) in NAMESPACE_ROOTS.iter().enumerate() {
        if let Some(&count) = ring.namespace_counts.get(i) {
            if count > 0 {
                by_namespace.push((*root, count));
            }
        }
    }

    EventLogStats {
        total_written: ring.total_written,
        buffered,
        dropped: ring.total_dropped,
        by_severity,
        by_namespace,
        serial_echo_level: ring.serial_echo_level,
    }
}

/// Set the minimum severity for serial echo.
pub fn set_serial_echo_level(level: Severity) {
    let mut ring = EVENT_RING.lock();
    ring.serial_echo_level = level;
}

/// Get the current serial echo level.
pub fn serial_echo_level() -> Severity {
    let ring = EVENT_RING.lock();
    ring.serial_echo_level
}

/// Clear all events from the ring buffer (for testing or maintenance).
pub fn clear() {
    let mut ring = EVENT_RING.lock();
    ring.write_idx = 0;
    ring.total_written = 0;
    ring.total_dropped = 0;
    ring.severity_counts = [0; 6];
    ring.namespace_counts = [0; 9];
    if ring.initialized {
        for entry in ring.entries.iter_mut() {
            *entry = EventEntry::zeroed();
        }
    }
    GLOBAL_SEQ.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Procfs content generation
// ---------------------------------------------------------------------------

/// Generate content for /proc/eventlog (statistics summary).
pub fn procfs_content() -> String {
    let st = stats();
    let mut out = String::with_capacity(512);

    out.push_str("Event Log Statistics\n");
    out.push_str("====================\n");
    out.push_str(&alloc::format!("Total events written: {}\n", st.total_written));
    out.push_str(&alloc::format!("Events in buffer:     {}\n", st.buffered));
    out.push_str(&alloc::format!("Buffer capacity:      {}\n", RING_SIZE));
    out.push_str(&alloc::format!("Events dropped:       {}\n", st.dropped));
    out.push_str(&alloc::format!("Serial echo level:    {}\n", st.serial_echo_level.as_str()));
    out.push_str("\nBy Severity:\n");
    for (sev, count) in &st.by_severity {
        if *count > 0 {
            out.push_str(&alloc::format!("  {:8} : {}\n", sev.as_str(), count));
        }
    }
    out.push_str("\nBy Namespace:\n");
    for (ns, count) in &st.by_namespace {
        out.push_str(&alloc::format!("  {:12} : {}\n", ns, count));
    }

    out
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run event logging self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[eventlog] Running event logging self-tests...");

    // Save state and clear for testing.
    let _saved_total = total_events();

    // Test 1: Basic event emission.
    clear();
    EventBuilder::new("system.boot", Severity::Info)
        .message("Test boot event")
        .pid(0)
        .emit();
    let t = total_events();
    if t != 1 {
        crate::serial_println!("[eventlog]   FAIL: expected 1 event, got {}", t);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   1. Basic emission: OK");

    // Test 2: Query with no filter.
    let result = query(&EventFilter::all(), 100);
    if result.matched != 1 {
        crate::serial_println!("[eventlog]   FAIL: expected 1 match, got {}", result.matched);
        return Err(KernelError::InternalError);
    }
    if let Some(ev) = result.events.first() {
        if ev.namespace_str() != "system.boot" {
            crate::serial_println!("[eventlog]   FAIL: wrong namespace: {}", ev.namespace_str());
            return Err(KernelError::InternalError);
        }
        if ev.message_str() != "Test boot event" {
            crate::serial_println!("[eventlog]   FAIL: wrong message: {}", ev.message_str());
            return Err(KernelError::InternalError);
        }
    } else {
        crate::serial_println!("[eventlog]   FAIL: no events returned");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   2. Query all: OK");

    // Test 3: Namespace prefix filtering.
    EventBuilder::new("security.login", Severity::Info)
        .message("User logged in")
        .pid(100)
        .service("auth")
        .emit();
    EventBuilder::new("security.logout", Severity::Info)
        .message("User logged out")
        .pid(100)
        .emit();
    EventBuilder::new("network.dhcp", Severity::Notice)
        .message("DHCP lease acquired")
        .kv("ip", "10.0.2.15")
        .kv("lease_time", "3600")
        .emit();

    let sec_result = query(&EventFilter::all().namespace("security"), 100);
    if sec_result.matched != 2 {
        crate::serial_println!("[eventlog]   FAIL: expected 2 security events, got {}", sec_result.matched);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   3. Namespace filtering: OK");

    // Test 4: Severity filtering.
    EventBuilder::new("system.error", Severity::Error)
        .message("Disk I/O error")
        .emit();
    EventBuilder::new("system.critical", Severity::Critical)
        .message("Out of memory")
        .emit();

    let warn_plus = query(
        &EventFilter::all().min_severity(Severity::Warning),
        100,
    );
    // Should get: 1 Notice (network.dhcp) + 1 Error + 1 Critical = 3
    // Wait — Notice is below Warning in our enum.  Let me check:
    // Debug=0, Info=1, Notice=2, Warning=3, Error=4, Critical=5
    // min_severity(Warning) → severity >= 3 → Error + Critical = 2
    if warn_plus.matched != 2 {
        crate::serial_println!(
            "[eventlog]   FAIL: expected 2 warning+ events, got {}",
            warn_plus.matched,
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   4. Severity filtering: OK");

    // Test 5: PID filtering.
    let pid_result = query(
        &EventFilter::all().pid(100),
        100,
    );
    if pid_result.matched != 2 {
        crate::serial_println!("[eventlog]   FAIL: expected 2 pid=100 events, got {}", pid_result.matched);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   5. PID filtering: OK");

    // Test 6: Text search.
    let text_result = query(
        &EventFilter::all().search("logged"),
        100,
    );
    if text_result.matched != 2 {
        crate::serial_println!("[eventlog]   FAIL: expected 2 'logged' matches, got {}", text_result.matched);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   6. Text search: OK");

    // Test 7: Sequence-based streaming (after_seq).
    let seq_before = current_seq();
    EventBuilder::new("application.test", Severity::Debug)
        .message("Streaming test event")
        .emit();
    let stream_result = query(
        &EventFilter::all().after(seq_before.saturating_sub(1)),
        100,
    );
    // Should get at least the new event.
    let found_streaming = stream_result.events.iter().any(|e| e.message_str() == "Streaming test event");
    if !found_streaming {
        crate::serial_println!("[eventlog]   FAIL: streaming event not found");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   7. Sequence-based streaming: OK");

    // Test 8: Payload key-value pairs.
    let dhcp_events = query(
        &EventFilter::all().namespace("network.dhcp"),
        100,
    );
    if let Some(ev) = dhcp_events.events.first() {
        let pairs: Vec<_> = ev.payload_iter().collect();
        if pairs.len() != 2 {
            crate::serial_println!("[eventlog]   FAIL: expected 2 payload pairs, got {}", pairs.len());
            return Err(KernelError::InternalError);
        }
        if pairs[0] != ("ip", "10.0.2.15") {
            crate::serial_println!("[eventlog]   FAIL: wrong payload[0]: {:?}", pairs[0]);
            return Err(KernelError::InternalError);
        }
    } else {
        crate::serial_println!("[eventlog]   FAIL: DHCP event not found");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   8. Payload key-value pairs: OK");

    // Test 9: Statistics.
    let st = stats();
    if st.total_written < 7 {
        crate::serial_println!("[eventlog]   FAIL: stats total_written < 7: {}", st.total_written);
        return Err(KernelError::InternalError);
    }
    // Check that severity counts make sense.
    let total_by_sev: u64 = st.by_severity.iter().map(|(_, c)| c).sum();
    if total_by_sev != st.total_written {
        crate::serial_println!(
            "[eventlog]   FAIL: severity sum {} != total_written {}",
            total_by_sev,
            st.total_written,
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]   9. Statistics consistency: OK");

    // Test 10: Namespace validation.
    if !is_valid_namespace("system") {
        crate::serial_println!("[eventlog]   FAIL: 'system' should be valid");
        return Err(KernelError::InternalError);
    }
    if !is_valid_namespace("security.login") {
        crate::serial_println!("[eventlog]   FAIL: 'security.login' should be valid");
        return Err(KernelError::InternalError);
    }
    if is_valid_namespace("foobar") {
        crate::serial_println!("[eventlog]   FAIL: 'foobar' should be invalid");
        return Err(KernelError::InternalError);
    }
    if is_valid_namespace("systemfoo") {
        crate::serial_println!("[eventlog]   FAIL: 'systemfoo' should be invalid");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[eventlog]  10. Namespace validation: OK");

    // Clean up.
    clear();
    crate::serial_println!("[eventlog] All 10 self-tests passed.");
    Ok(())
}
