//! Capability request broker — dynamic privilege elevation.
//!
//! Allows processes to request capabilities they don't currently hold,
//! with a reason string displayed to the user for approval/denial.
//!
//! ## Design (from design.txt)
//!
//! > "When the app asks a user to grant it some capability, it should
//! > pass a parameter to the request that's passed along to the user
//! > security dialog that tells the user what that particular capability
//! > is being asked for."
//!
//! ## Request Flow
//!
//! 1. Process calls `request_capability(resource_type, rights, reason)`.
//! 2. Kernel queues the request with process metadata (PID, name).
//! 3. The security policy handler (initially console-based, later GUI)
//!    is notified and presents the request to the user.
//! 4. The handler calls `approve(request_id)` or `deny(request_id)`.
//! 5. If approved, the capability is inserted into the requesting
//!    process's capability table.  If denied, the process gets an error.
//!
//! ## Policy
//!
//! - Auto-deny if no policy handler is registered (fail-safe).
//! - Requests have a timeout — if no response within `REQUEST_TIMEOUT_MS`,
//!   the request is auto-denied.
//! - A process can have at most `MAX_PENDING_PER_PROCESS` active requests.
//! - Completed requests are retained for audit logging.
//!
//! ## Current Scope
//!
//! This module provides the kernel-side infrastructure:
//! - Request queue and lifecycle management.
//! - Approval/denial API.
//! - Audit trail of all requests.
//! - Kshell integration for manual approve/deny.
//!
//! The GUI security dialog will use these primitives when the desktop
//! environment is available.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::cap::{ResourceType, Rights};
use crate::error::{KernelError, KernelResult};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum pending requests in the system.
const MAX_PENDING_REQUESTS: usize = 32;

/// Maximum pending requests per process.
const MAX_PENDING_PER_PROCESS: usize = 4;

/// Maximum length of the reason string.
const MAX_REASON_LEN: usize = 256;

/// Request timeout in milliseconds (30 seconds).
const REQUEST_TIMEOUT_MS: u64 = 30_000;

// ---------------------------------------------------------------------------
// Request data structures
// ---------------------------------------------------------------------------

/// Unique identifier for a capability request.
pub type RequestId = u64;

/// Status of a capability request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    /// Waiting for approval/denial.
    Pending,
    /// Approved by the policy handler.
    Approved,
    /// Denied by the policy handler.
    Denied,
    /// Timed out (no response within deadline).
    TimedOut,
    /// Cancelled by the requesting process.
    Cancelled,
}

/// A capability request from a process.
#[derive(Debug, Clone)]
pub struct CapRequest {
    /// Unique request identifier.
    pub id: RequestId,
    /// PID of the requesting process.
    pub pid: u64,
    /// Name of the requesting process (for display).
    pub process_name: String,
    /// Resource type being requested.
    pub resource_type: ResourceType,
    /// Rights being requested.
    pub rights: Rights,
    /// Human-readable reason for the request.
    pub reason: String,
    /// Current status.
    pub status: RequestStatus,
    /// Timestamp when the request was created (monotonic ms).
    pub created_at_ms: u64,
    /// Timestamp when the request was resolved (0 if pending).
    pub resolved_at_ms: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Next request ID counter.
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Whether a policy handler is registered.
static HANDLER_REGISTERED: AtomicBool = AtomicBool::new(false);

/// The request queue.
static REQUESTS: spin::Mutex<Vec<CapRequest>> = spin::Mutex::new(Vec::new());

/// Monotonic millisecond clock for timeouts.
fn now_ms() -> u64 {
    crate::hpet::elapsed_ns() / 1_000_000
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Submit a capability request.
///
/// The process identified by `pid` requests `rights` on `resource_type`
/// for the given `reason`.  Returns a `RequestId` that can be used to
/// check the status later.
///
/// Returns `Err(ResourceExhausted)` if the queue is full or the process
/// has too many pending requests.
pub fn request_capability(
    pid: u64,
    process_name: &str,
    resource_type: ResourceType,
    rights: Rights,
    reason: &str,
) -> KernelResult<RequestId> {
    let mut requests = REQUESTS.lock();

    // Check system-wide limit.
    let pending_count = requests.iter().filter(|r| r.status == RequestStatus::Pending).count();
    if pending_count >= MAX_PENDING_REQUESTS {
        serial_println!("[cap-request] Queue full ({} pending)", pending_count);
        return Err(KernelError::ResourceExhausted);
    }

    // Check per-process limit.
    let proc_pending = requests.iter()
        .filter(|r| r.pid == pid && r.status == RequestStatus::Pending)
        .count();
    if proc_pending >= MAX_PENDING_PER_PROCESS {
        serial_println!("[cap-request] Process {} has too many pending requests ({})",
            pid, proc_pending);
        return Err(KernelError::ResourceExhausted);
    }

    // Truncate reason to max length.
    let reason_str = if reason.len() > MAX_REASON_LEN {
        String::from(&reason[..MAX_REASON_LEN])
    } else {
        String::from(reason)
    };

    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let now = now_ms();

    let req = CapRequest {
        id,
        pid,
        process_name: String::from(process_name),
        resource_type,
        rights,
        reason: reason_str,
        status: RequestStatus::Pending,
        created_at_ms: now,
        resolved_at_ms: 0,
    };

    serial_println!("[cap-request] #{}: pid={} requests {:?}/{:?} -- {:?}",
        id, pid, resource_type, rights, req.reason);

    requests.push(req);

    // If no handler is registered, auto-deny immediately.
    if !HANDLER_REGISTERED.load(Ordering::Acquire) {
        serial_println!("[cap-request] #{}: auto-denied (no policy handler)", id);
        if let Some(r) = requests.iter_mut().find(|r| r.id == id) {
            r.status = RequestStatus::Denied;
            r.resolved_at_ms = now;
        }
    }

    Ok(id)
}

/// Approve a pending request.
///
/// The capability is NOT automatically inserted — the caller (policy
/// handler) is responsible for granting the capability via the cap table.
/// This just marks the request as approved for audit purposes.
pub fn approve(request_id: RequestId) -> KernelResult<CapRequest> {
    let mut requests = REQUESTS.lock();
    let req = requests.iter_mut()
        .find(|r| r.id == request_id)
        .ok_or(KernelError::NotFound)?;

    if req.status != RequestStatus::Pending {
        return Err(KernelError::InvalidArgument);
    }

    req.status = RequestStatus::Approved;
    req.resolved_at_ms = now_ms();

    serial_println!("[cap-request] #{}: APPROVED (pid={}, {:?}/{:?})",
        request_id, req.pid, req.resource_type, req.rights);

    Ok(req.clone())
}

/// Deny a pending request.
pub fn deny(request_id: RequestId) -> KernelResult<()> {
    let mut requests = REQUESTS.lock();
    let req = requests.iter_mut()
        .find(|r| r.id == request_id)
        .ok_or(KernelError::NotFound)?;

    if req.status != RequestStatus::Pending {
        return Err(KernelError::InvalidArgument);
    }

    req.status = RequestStatus::Denied;
    req.resolved_at_ms = now_ms();

    serial_println!("[cap-request] #{}: DENIED (pid={}, {:?}/{:?})",
        request_id, req.pid, req.resource_type, req.rights);

    Ok(())
}

/// Cancel a request (by the requesting process).
pub fn cancel(request_id: RequestId, pid: u64) -> KernelResult<()> {
    let mut requests = REQUESTS.lock();
    let req = requests.iter_mut()
        .find(|r| r.id == request_id && r.pid == pid)
        .ok_or(KernelError::NotFound)?;

    if req.status != RequestStatus::Pending {
        return Err(KernelError::InvalidArgument);
    }

    req.status = RequestStatus::Cancelled;
    req.resolved_at_ms = now_ms();
    Ok(())
}

/// Get the status of a request.
#[must_use]
pub fn get_status(request_id: RequestId) -> Option<RequestStatus> {
    let requests = REQUESTS.lock();
    requests.iter().find(|r| r.id == request_id).map(|r| r.status)
}

/// Get a request by ID.
#[must_use]
pub fn get_request(request_id: RequestId) -> Option<CapRequest> {
    let requests = REQUESTS.lock();
    requests.iter().find(|r| r.id == request_id).cloned()
}

/// List all pending requests (for the policy handler to display).
#[must_use]
pub fn list_pending() -> Vec<CapRequest> {
    let requests = REQUESTS.lock();
    requests.iter()
        .filter(|r| r.status == RequestStatus::Pending)
        .cloned()
        .collect()
}

/// List all requests (including resolved, for audit).
#[must_use]
pub fn list_all() -> Vec<CapRequest> {
    let requests = REQUESTS.lock();
    requests.clone()
}

/// Get the number of pending requests.
#[must_use]
pub fn pending_count() -> usize {
    let requests = REQUESTS.lock();
    requests.iter().filter(|r| r.status == RequestStatus::Pending).count()
}

/// Register a policy handler.
///
/// Once registered, requests will remain pending (instead of auto-denied)
/// until the handler approves or denies them.
pub fn register_handler() {
    HANDLER_REGISTERED.store(true, Ordering::Release);
    serial_println!("[cap-request] Policy handler registered");
}

/// Unregister the policy handler.
///
/// All pending requests are auto-denied when the handler goes away.
pub fn unregister_handler() {
    HANDLER_REGISTERED.store(false, Ordering::Release);

    let mut requests = REQUESTS.lock();
    let now = now_ms();
    for req in requests.iter_mut() {
        if req.status == RequestStatus::Pending {
            req.status = RequestStatus::Denied;
            req.resolved_at_ms = now;
            serial_println!("[cap-request] #{}: auto-denied (handler unregistered)", req.id);
        }
    }
}

/// Whether a policy handler is currently registered.
#[must_use]
pub fn handler_active() -> bool {
    HANDLER_REGISTERED.load(Ordering::Acquire)
}

/// Expire timed-out requests.
///
/// Called periodically (e.g., from the idle loop or a timer) to
/// mark requests that have been pending too long.
pub fn expire_timeouts() {
    let now = now_ms();
    let mut requests = REQUESTS.lock();
    for req in requests.iter_mut() {
        if req.status == RequestStatus::Pending {
            let elapsed = now.saturating_sub(req.created_at_ms);
            if elapsed >= REQUEST_TIMEOUT_MS {
                req.status = RequestStatus::TimedOut;
                req.resolved_at_ms = now;
                serial_println!("[cap-request] #{}: timed out after {}ms", req.id, elapsed);
            }
        }
    }
}

/// Clear resolved requests older than `max_age_ms`.
///
/// Keeps the audit trail bounded.
pub fn gc(max_age_ms: u64) {
    let now = now_ms();
    let mut requests = REQUESTS.lock();
    requests.retain(|r| {
        if r.status == RequestStatus::Pending {
            return true; // Never GC pending requests.
        }
        let age = now.saturating_sub(r.resolved_at_ms);
        age < max_age_ms
    });
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run self-tests for the capability request broker.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[cap-request] Running self-test...");

    // Test 1: Submit a request (auto-denied since no handler).
    let id = request_capability(
        42,
        "test-app",
        ResourceType::File,
        Rights::READ,
        "Need to read user config file",
    )?;
    assert!(id > 0);
    let status = get_status(id).unwrap();
    assert_eq!(status, RequestStatus::Denied, "Should be auto-denied without handler");
    serial_println!("[cap-request]   Auto-deny without handler: OK");

    // Test 2: Register handler, submit request (stays pending).
    register_handler();
    let id2 = request_capability(
        43,
        "another-app",
        ResourceType::Socket,
        Rights::WRITE,
        "Connect to update server",
    )?;
    let status2 = get_status(id2).unwrap();
    assert_eq!(status2, RequestStatus::Pending, "Should be pending with handler");
    serial_println!("[cap-request]   Pending with handler: OK");

    // Test 3: Approve.
    let approved_req = approve(id2)?;
    assert_eq!(approved_req.status, RequestStatus::Approved);
    assert_eq!(approved_req.pid, 43);
    serial_println!("[cap-request]   Approve: OK");

    // Test 4: Deny.
    let id3 = request_capability(
        44,
        "sketchy-app",
        ResourceType::Process,
        Rights::ALL,
        "I need all the powers",
    )?;
    deny(id3)?;
    let status3 = get_status(id3).unwrap();
    assert_eq!(status3, RequestStatus::Denied);
    serial_println!("[cap-request]   Deny: OK");

    // Test 5: Cancel.
    let id4 = request_capability(
        45,
        "impatient-app",
        ResourceType::File,
        Rights::WRITE,
        "Save preferences",
    )?;
    cancel(id4, 45)?;
    let status4 = get_status(id4).unwrap();
    assert_eq!(status4, RequestStatus::Cancelled);
    serial_println!("[cap-request]   Cancel: OK");

    // Test 6: Per-process limit.
    for i in 0..MAX_PENDING_PER_PROCESS {
        let _ = request_capability(
            99,
            "spammer",
            ResourceType::File,
            Rights::READ,
            &alloc::format!("request {}", i),
        )?;
    }
    let result = request_capability(
        99,
        "spammer",
        ResourceType::File,
        Rights::READ,
        "one too many",
    );
    assert!(result.is_err(), "Should hit per-process limit");
    serial_println!("[cap-request]   Per-process limit: OK");

    // Test 7: Unregister handler auto-denies pending.
    let pending_before = pending_count();
    unregister_handler();
    let pending_after = pending_count();
    assert_eq!(pending_after, 0, "All pending should be denied");
    assert!(pending_before > 0);
    serial_println!("[cap-request]   Unregister auto-deny: OK ({}->{})", pending_before, pending_after);

    // Cleanup: GC all entries.
    gc(0);

    serial_println!("[cap-request] Self-test PASSED");
    Ok(())
}
