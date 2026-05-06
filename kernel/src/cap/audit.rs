//! Capability audit log — tracks capability operations for security monitoring.
//!
//! Records capability-related events (grants, revocations, denials, transfers)
//! in a ring buffer for security auditing.  This enables:
//! - Detecting unauthorized access attempts (denied operations)
//! - Understanding privilege escalation chains (delegation history)
//! - Post-mortem security analysis (what capabilities did a compromised process have?)
//!
//! ## Event Types
//!
//! - **Grant**: a capability was granted to a process
//! - **Revoke**: a capability was revoked from a process
//! - **Deny**: a capability check failed (access denied)
//! - **Transfer**: a capability was transferred between processes
//! - **Duplicate**: a capability was duplicated (with potential rights reduction)
//! - **Delegate**: a capability was delegated to a child/subprocess
//!
//! ## Design
//!
//! Fixed-size ring buffer (128 entries), lock-free writes via atomic
//! sequence counter.  Events include timestamp, process ID, capability
//! handle, operation type, and outcome.
//!
//! The audit log is always-on with minimal overhead (one atomic increment
//! per event).  For high-security environments, events can be forwarded
//! to a persistent log file via klog.
//!
//! ## References
//!
//! - Linux audit subsystem (`kernel/audit.c`) — security event logging
//! - seL4 capability tracing — cap space debugging
//! - Windows Security Event Log — privilege audit events
//! - Capsicum audit integration — FreeBSD capability auditing

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Ring buffer capacity.
const RING_SIZE: usize = 128;
const RING_MASK: usize = RING_SIZE - 1;

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// Type of capability operation recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AuditOp {
    /// Capability granted to a process.
    Grant = 0,
    /// Capability revoked from a process.
    Revoke = 1,
    /// Capability check denied (access attempt failed).
    Deny = 2,
    /// Capability transferred between processes.
    Transfer = 3,
    /// Capability duplicated (possibly with reduced rights).
    Duplicate = 4,
    /// Capability delegated to child process.
    Delegate = 5,
    /// Capability used successfully (for high-security audit trails).
    Use = 6,
    /// Capability dropped/closed by holder.
    Drop = 7,
}

impl AuditOp {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Grant,
            1 => Self::Revoke,
            2 => Self::Deny,
            3 => Self::Transfer,
            4 => Self::Duplicate,
            5 => Self::Delegate,
            6 => Self::Use,
            7 => Self::Drop,
            _ => Self::Deny,
        }
    }

    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Grant => "GRANT",
            Self::Revoke => "REVOKE",
            Self::Deny => "DENY",
            Self::Transfer => "TRANSFER",
            Self::Duplicate => "DUP",
            Self::Delegate => "DELEGATE",
            Self::Use => "USE",
            Self::Drop => "DROP",
        }
    }
}

// ---------------------------------------------------------------------------
// Audit entry
// ---------------------------------------------------------------------------

/// A single audit event.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct AuditEntry {
    /// Timestamp (APIC ticks since boot).
    pub timestamp: u64,
    /// Process ID that initiated the operation.
    pub pid: u32,
    /// Capability handle involved.
    pub handle: u32,
    /// Operation type.
    pub op: u8,
    /// Rights mask involved (0 if not applicable).
    pub rights: u8,
    /// Target PID (for transfer/delegate, 0 otherwise).
    pub target_pid: u16,
    /// Result: 0 = success, non-zero = error code.
    pub result: u32,
}

impl AuditEntry {
    pub const fn empty() -> Self {
        Self {
            timestamp: 0,
            pid: 0,
            handle: 0,
            op: 0,
            rights: 0,
            target_pid: 0,
            result: 0,
        }
    }

    /// Whether this entry is valid (non-zero timestamp).
    pub fn is_valid(&self) -> bool {
        self.timestamp != 0
    }

    /// Get the operation type.
    pub fn operation(&self) -> AuditOp {
        AuditOp::from_u8(self.op)
    }
}

// ---------------------------------------------------------------------------
// Ring buffer
// ---------------------------------------------------------------------------

struct AuditRing(core::cell::UnsafeCell<[AuditEntry; RING_SIZE]>);
unsafe impl Sync for AuditRing {}

static RING: AuditRing = AuditRing(core::cell::UnsafeCell::new(
    [AuditEntry::empty(); RING_SIZE]
));

/// Write position.
static WRITE_POS: AtomicU32 = AtomicU32::new(0);

/// Whether auditing is enabled.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Total events recorded.
static TOTAL_EVENTS: AtomicU64 = AtomicU64::new(0);

/// Total denied operations (security-relevant).
static TOTAL_DENIALS: AtomicU64 = AtomicU64::new(0);

/// Total grant operations.
static TOTAL_GRANTS: AtomicU64 = AtomicU64::new(0);

/// Total revocation operations.
static TOTAL_REVOKES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API — recording
// ---------------------------------------------------------------------------

/// Record a capability audit event.
///
/// Called by the capability subsystem on every significant operation.
#[inline]
pub fn record(op: AuditOp, pid: u32, handle: u32, rights: u8, target_pid: u16, result: u32) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let timestamp = crate::apic::tick_count();

    let entry = AuditEntry {
        timestamp,
        pid,
        handle,
        op: op as u8,
        rights,
        target_pid,
        result,
    };

    // Write to ring buffer.
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed);
    let slot = (pos as usize) & RING_MASK;
    unsafe {
        let ptr = RING.0.get() as *mut AuditEntry;
        ptr.add(slot).write(entry);
    }

    // Update counters.
    TOTAL_EVENTS.fetch_add(1, Ordering::Relaxed);
    match op {
        AuditOp::Deny => {
            TOTAL_DENIALS.fetch_add(1, Ordering::Relaxed);
            // Security-relevant: log denials to the structured klog.
            crate::klog::log_fmt(
                crate::klog::Level::Warn,
                "cap_audit",
                format_args!("DENY pid={} handle={} rights={:#x}", pid, handle, rights),
            );
        }
        AuditOp::Grant => { TOTAL_GRANTS.fetch_add(1, Ordering::Relaxed); }
        AuditOp::Revoke => { TOTAL_REVOKES.fetch_add(1, Ordering::Relaxed); }
        _ => {}
    }
}

/// Convenience: record a successful capability use.
#[inline]
pub fn record_use(pid: u32, handle: u32, rights: u8) {
    record(AuditOp::Use, pid, handle, rights, 0, 0);
}

/// Convenience: record a denied capability check.
#[inline]
pub fn record_deny(pid: u32, handle: u32, rights: u8, error: u32) {
    record(AuditOp::Deny, pid, handle, rights, 0, error);
}

/// Convenience: record a capability grant.
#[inline]
pub fn record_grant(pid: u32, handle: u32, rights: u8) {
    record(AuditOp::Grant, pid, handle, rights, 0, 0);
}

/// Convenience: record a capability revocation.
#[inline]
pub fn record_revoke(pid: u32, handle: u32) {
    record(AuditOp::Revoke, pid, handle, 0, 0, 0);
}

/// Convenience: record a capability transfer.
#[inline]
pub fn record_transfer(from_pid: u32, to_pid: u32, handle: u32, rights: u8) {
    record(AuditOp::Transfer, from_pid, handle, rights, to_pid as u16, 0);
}

// ---------------------------------------------------------------------------
// Public API — control
// ---------------------------------------------------------------------------

/// Enable capability auditing.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable capability auditing.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether auditing is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Reset the audit log.
pub fn reset() {
    WRITE_POS.store(0, Ordering::Release);
    TOTAL_EVENTS.store(0, Ordering::Relaxed);
    TOTAL_DENIALS.store(0, Ordering::Relaxed);
    TOTAL_GRANTS.store(0, Ordering::Relaxed);
    TOTAL_REVOKES.store(0, Ordering::Relaxed);
    for i in 0..RING_SIZE {
        unsafe {
            let ptr = RING.0.get() as *mut AuditEntry;
            ptr.add(i).write(AuditEntry::empty());
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — querying
// ---------------------------------------------------------------------------

/// Audit statistics.
#[derive(Debug, Clone, Copy)]
pub struct AuditStats {
    pub enabled: bool,
    pub total_events: u64,
    pub total_denials: u64,
    pub total_grants: u64,
    pub total_revokes: u64,
    pub ring_entries: usize,
}

/// Get audit statistics.
#[must_use]
pub fn stats() -> AuditStats {
    let write_pos = WRITE_POS.load(Ordering::Relaxed) as usize;
    let entries = write_pos.min(RING_SIZE);

    AuditStats {
        enabled: ENABLED.load(Ordering::Relaxed),
        total_events: TOTAL_EVENTS.load(Ordering::Relaxed),
        total_denials: TOTAL_DENIALS.load(Ordering::Relaxed),
        total_grants: TOTAL_GRANTS.load(Ordering::Relaxed),
        total_revokes: TOTAL_REVOKES.load(Ordering::Relaxed),
        ring_entries: entries,
    }
}

/// Get the most recent N audit entries (newest first).
pub fn recent(buf: &mut [AuditEntry]) -> usize {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let available = write_pos.min(RING_SIZE);
    let to_copy = buf.len().min(available);

    for i in 0..to_copy {
        let idx = (write_pos.wrapping_sub(1).wrapping_sub(i)) & RING_MASK;
        unsafe {
            let ptr = RING.0.get() as *const AuditEntry;
            buf[i] = ptr.add(idx).read();
        }
    }

    to_copy
}

/// Count denied operations for a specific PID.
pub fn denials_for_pid(pid: u32) -> u32 {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let count = write_pos.min(RING_SIZE);
    let start = if write_pos <= RING_SIZE { 0 } else { write_pos & RING_MASK };

    let mut denials: u32 = 0;
    for i in 0..count {
        let idx = (start + i) & RING_MASK;
        let entry = unsafe {
            let ptr = RING.0.get() as *const AuditEntry;
            ptr.add(idx).read()
        };
        if entry.pid == pid && entry.op == AuditOp::Deny as u8 {
            denials = denials.saturating_add(1);
        }
    }
    denials
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for capability auditing.
pub fn self_test() {
    serial_println!("[cap_audit] Running self-test...");

    // Test 1: Reset state.
    reset();
    let s = stats();
    assert_eq!(s.total_events, 0);
    assert_eq!(s.total_denials, 0);
    serial_println!("[cap_audit]   Reset: OK");

    // Test 2: Record events.
    record_grant(1, 100, 0xFF);
    record_use(1, 100, 0x03);
    record_deny(2, 200, 0x01, 13);
    record_revoke(1, 100);

    let s = stats();
    assert_eq!(s.total_events, 4);
    assert_eq!(s.total_denials, 1);
    assert_eq!(s.total_grants, 1);
    assert_eq!(s.total_revokes, 1);
    serial_println!("[cap_audit]   Record events: OK (4 events)");

    // Test 3: Recent entries.
    let mut buf = [AuditEntry::empty(); 8];
    let n = recent(&mut buf);
    assert_eq!(n, 4);
    // Most recent first = revoke.
    assert_eq!(buf[0].operation(), AuditOp::Revoke);
    assert_eq!(buf[0].pid, 1);
    // Second = deny.
    assert_eq!(buf[1].operation(), AuditOp::Deny);
    assert_eq!(buf[1].pid, 2);
    assert_eq!(buf[1].result, 13);
    serial_println!("[cap_audit]   Recent (newest first): OK");

    // Test 4: Denial count per PID.
    let d = denials_for_pid(2);
    assert_eq!(d, 1);
    let d = denials_for_pid(1);
    assert_eq!(d, 0);
    serial_println!("[cap_audit]   Per-PID denial count: OK");

    // Test 5: Transfer recording.
    record_transfer(1, 3, 150, 0x07);
    let mut buf = [AuditEntry::empty(); 1];
    let n = recent(&mut buf);
    assert_eq!(n, 1);
    assert_eq!(buf[0].operation(), AuditOp::Transfer);
    assert_eq!(buf[0].pid, 1);
    assert_eq!(buf[0].target_pid, 3);
    serial_println!("[cap_audit]   Transfer recording: OK");

    // Test 6: Disable/enable.
    disable();
    let before = TOTAL_EVENTS.load(Ordering::Relaxed);
    record_grant(99, 999, 0xFF);
    assert_eq!(TOTAL_EVENTS.load(Ordering::Relaxed), before);
    enable();
    serial_println!("[cap_audit]   Disable/enable: OK");

    // Cleanup.
    reset();

    serial_println!("[cap_audit] Self-test PASSED");
}
