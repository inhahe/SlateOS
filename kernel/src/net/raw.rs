//! Raw Ethernet frame boundary for the userspace network stack.
//!
//! This is the foundation of the "move networking to userspace" migration
//! (design-decisions.md §63, Path B).  It exposes the physical NIC's layer-2
//! send/receive to a **single** privileged userspace process — the `netstack`
//! daemon — *without* moving the driver out of the kernel.  The kernel keeps
//! the thin virtio-net / e1000 driver; the daemon owns the protocol stack.
//!
//! ## Exclusive claim
//!
//! Raw access bypasses the entire in-kernel protocol stack and firewall, so it
//! is exclusive: while a raw handle is claimed, [`is_claimed`] returns `true`
//! and [`crate::net::poll`] stops draining physical-NIC frames itself (they
//! belong to the daemon, delivered via [`recv`]).  Exactly one owner processes
//! uplink frames at a time.
//!
//! The claim self-heals: if the owning process dies without releasing (crash,
//! kill), the next liveness check reclaims the NIC for the in-kernel stack so
//! connectivity is never permanently wedged by a dead daemon.
//!
//! ## Capability
//!
//! Opening a raw handle requires a [`crate::cap::ResourceType::NetRaw`]
//! capability with `WRITE` rights — strictly more privileged than an ordinary
//! `Socket`, since it grants unfiltered link-layer access.  Gating lives in the
//! syscall handler ([`crate::syscall`]); this module is the mechanism.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::{ProcessId, ProcessState};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Sentinel PID meaning "no owner".
const NO_OWNER: u64 = 0;

/// Whether a raw handle is currently claimed.
static CLAIMED: AtomicBool = AtomicBool::new(false);

/// PID of the current raw owner (meaningful only while `CLAIMED` is `true`).
static OWNER: AtomicU64 = AtomicU64::new(NO_OWNER);

/// Is a process considered dead (gone or zombied) for claim-ownership purposes?
fn owner_is_dead(pid: u64) -> bool {
    match crate::proc::pcb::state(pid) {
        None => true,                        // no such process
        Some(ProcessState::Zombie) => true,  // exited, not yet reaped
        Some(_) => false,
    }
}

/// Force-release the claim (used when a stale/dead owner is detected).
fn release_stale() {
    OWNER.store(NO_OWNER, Ordering::Release);
    CLAIMED.store(false, Ordering::Release);
}

/// True while a **live** process owns the raw NIC handle.
///
/// [`crate::net::poll`] consults this each poll to decide whether to drain the
/// physical NIC itself.  Cheap in the common (unclaimed) case — a single
/// relaxed load — and only touches the process table when a claim is actually
/// held (rare: only while the daemon owns the NIC, during which poll skips the
/// per-packet drain entirely, so the cost is once-per-poll, not per-packet).
pub fn is_claimed() -> bool {
    if !CLAIMED.load(Ordering::Acquire) {
        return false;
    }
    let cur = OWNER.load(Ordering::Acquire);
    if cur == NO_OWNER || owner_is_dead(cur) {
        // Owner died without releasing — hand the NIC back to the in-kernel
        // stack so connectivity resumes.
        release_stale();
        return false;
    }
    true
}

/// PID of the current (live) raw owner, or `None` if unclaimed.
#[must_use]
pub fn owner() -> Option<ProcessId> {
    if is_claimed() {
        let cur = OWNER.load(Ordering::Acquire);
        if cur != NO_OWNER {
            return Some(cur);
        }
    }
    None
}

/// Claim exclusive raw access to the physical NIC for `pid`.
///
/// Succeeds if the NIC is unclaimed, already owned by `pid` (idempotent), or
/// the current owner has died without releasing (the stale claim is reclaimed).
///
/// # Errors
///
/// [`KernelError::DeviceBusy`] if a *different, live* process holds the claim.
pub fn claim(pid: ProcessId) -> KernelResult<()> {
    // Fast path: take an unclaimed NIC.
    if CLAIMED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        OWNER.store(pid, Ordering::Release);
        return Ok(());
    }

    // Already claimed — inspect the owner.
    let cur = OWNER.load(Ordering::Acquire);
    if cur == pid {
        return Ok(()); // idempotent re-claim by the same process
    }
    if cur == NO_OWNER || owner_is_dead(cur) {
        // Reclaim from a dead/absent owner without dropping `CLAIMED`.
        if OWNER
            .compare_exchange(cur, pid, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(());
        }
        // Lost the race to another claimant; fall through to the live check.
        let now = OWNER.load(Ordering::Acquire);
        if now == pid {
            return Ok(());
        }
    }
    Err(KernelError::DeviceBusy)
}

/// Release a claim held by `pid`.
///
/// No-op (returns `Ok`) if `pid` is not the current owner — release is
/// idempotent and a non-owner releasing must not disturb the real owner.
///
/// # Errors
///
/// Never returns `Err` currently; the signature is `KernelResult` for
/// forward-compatibility with per-interface claims.
pub fn release(pid: ProcessId) -> KernelResult<()> {
    // Only the owner may clear the claim.
    if OWNER
        .compare_exchange(pid, NO_OWNER, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        CLAIMED.store(false, Ordering::Release);
    }
    Ok(())
}

/// Transmit a raw Ethernet frame out the physical NIC.
///
/// The frame is sent verbatim — no protocol processing, no firewall.  Only the
/// raw owner should call this (enforced by the syscall capability gate).
///
/// # Errors
///
/// Propagates [`crate::net::send_frame`] errors (e.g.
/// [`KernelError::NoSuchDevice`] if no NIC is present).
pub fn transmit(frame: &[u8]) -> KernelResult<()> {
    super::send_frame(frame)
}

/// Receive one raw Ethernet frame from the physical NIC (non-blocking).
///
/// Returns `Some(frame)` if a frame was pending, or `None` if the NIC queue is
/// empty.  Pulls directly from the driver; because [`is_claimed`] gates the
/// in-kernel drain in [`crate::net::poll`], the owner does not race the kernel
/// stack for frames.
#[must_use]
pub fn receive() -> Option<alloc::vec::Vec<u8>> {
    super::recv_frame()
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: claim/liveness tests that need a live process table run as ring-3
    // boot self-tests, not unit tests — the process table is a kernel global.
    // These unit tests cover the pure claim-state transitions with synthetic
    // PIDs, relying on `owner_is_dead` treating unknown PIDs as dead.

    fn reset() {
        OWNER.store(NO_OWNER, Ordering::SeqCst);
        CLAIMED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn unclaimed_is_not_claimed() {
        reset();
        assert!(!is_claimed());
        assert_eq!(owner(), None);
    }

    #[test]
    fn release_by_non_owner_is_noop() {
        reset();
        // PID 4242 claims; unknown PIDs are "dead", so use the raw state.
        OWNER.store(4242, Ordering::SeqCst);
        CLAIMED.store(true, Ordering::SeqCst);
        // A different PID releasing must not clear a claim it does not own.
        let _ = release(9999);
        assert_eq!(OWNER.load(Ordering::SeqCst), 4242);
    }
}
