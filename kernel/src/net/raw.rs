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
        None => true,                       // no such process
        Some(ProcessState::Zombie) => true, // exited, not yet reaped
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
// `unnecessary_wraps`: deliberate, per the `# Errors` note above.  Releasing a
// *named interface* — rather than the single global claim this module has
// today — can fail (no such interface; not owned by `pid`), and every caller
// is already written against `KernelResult`.  Narrowing the return type to
// `()` now would mean re-widening it later and touching every call site, to no
// benefit in the interim.
#[allow(clippy::unnecessary_wraps)]
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

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------
//
// These checks were a `#[cfg(test)] mod tests`, which the kernel crate never
// compiles (`test = false`, no lib target — see `known-issues.md`
// → `A-KERNEL-UNIT-TESTS-NEVER-RUN`), so they had never run.  Lane B's
// `raced-globals.py` reported the two of them as racing on `CLAIMED`/`OWNER`;
// the interleaving it traced was real, but could not occur, because neither
// test executed.  As boot self-tests they are serialised by construction —
// boot self-tests run sequentially on one CPU — so the race is answered by
// moving them somewhere they run rather than by a mutex.
//
// Two things the unit-test versions got away with and this cannot:
//
//   * **The process table is real here.**  They hardcoded PID 4242 and relied
//     on `owner_is_dead` treating an unknown PID as dead.  That is an
//     assumption about live kernel state, so it is now checked.
//   * **These statics are live.**  Writing them is writing the actual NIC
//     claim, so the test refuses to run if one is held and restores the
//     prior state on every exit path.

/// Restore the module to the unclaimed state.
fn reset_claim_state() {
    OWNER.store(NO_OWNER, Ordering::SeqCst);
    CLAIMED.store(false, Ordering::SeqCst);
}

/// Find a PID with no process behind it, so `owner_is_dead` reports it dead.
///
/// Returns `None` if 64 consecutive candidates are all live, which would mean
/// the test cannot construct the state it needs.
fn find_absent_pid(start: u64) -> Option<ProcessId> {
    let mut pid = start;
    for _ in 0..64u32 {
        if crate::proc::pcb::state(pid).is_none() {
            return Some(pid);
        }
        pid = pid.saturating_add(1);
    }
    None
}

/// An unclaimed NIC reports itself unclaimed and ownerless.
fn check_unclaimed() -> KernelResult<()> {
    reset_claim_state();
    if is_claimed() {
        crate::serial_println!("[net-raw]   FAIL: unclaimed NIC reports claimed");
        return Err(KernelError::InternalError);
    }
    if let Some(pid) = owner() {
        crate::serial_println!("[net-raw]   FAIL: unclaimed NIC reports owner {pid}");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   unclaimed reports unclaimed: OK");
    Ok(())
}

/// A non-owner releasing must not disturb the real owner's claim.
fn check_release_by_non_owner(owner_pid: ProcessId, other_pid: ProcessId) -> KernelResult<()> {
    reset_claim_state();
    // Install the claim directly rather than via `claim()`: `owner_pid` names
    // no live process, so `claim()` is not the path under test here.
    OWNER.store(owner_pid, Ordering::SeqCst);
    CLAIMED.store(true, Ordering::SeqCst);

    // `release` cannot currently fail, and its result is irrelevant to the
    // property under test — that a non-owner's release is a no-op for the
    // owner's claim, which is asserted on `OWNER` below.
    let _ = release(other_pid);

    let after = OWNER.load(Ordering::SeqCst);
    if after != owner_pid {
        crate::serial_println!(
            "[net-raw]   FAIL: release by non-owner {other_pid} changed owner {owner_pid} -> {after}"
        );
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   release by a non-owner is a no-op: OK");
    Ok(())
}

/// A claim held by a dead process is reclaimed for the in-kernel stack.
///
/// The module's headline promise — "the claim self-heals" — had no test at
/// all.  It is also the behaviour that makes `is_claimed()` a *writer* despite
/// its name, which is what made lane B's report subtle: the query is what
/// performs the repair.
fn check_dead_owner_self_heals(dead_pid: ProcessId) -> KernelResult<()> {
    reset_claim_state();
    OWNER.store(dead_pid, Ordering::SeqCst);
    CLAIMED.store(true, Ordering::SeqCst);

    if is_claimed() {
        crate::serial_println!(
            "[net-raw]   FAIL: claim by dead PID {dead_pid} still reported live"
        );
        return Err(KernelError::InternalError);
    }
    // The repair must be durable, not just a `false` return.
    if CLAIMED.load(Ordering::SeqCst) || OWNER.load(Ordering::SeqCst) != NO_OWNER {
        crate::serial_println!("[net-raw]   FAIL: stale claim reported dead but not cleared");
        return Err(KernelError::InternalError);
    }
    // And a fresh claim must now succeed.
    if claim(dead_pid).is_err() {
        crate::serial_println!("[net-raw]   FAIL: NIC not reclaimable after self-heal");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[net-raw]   dead owner's claim self-heals: OK");
    Ok(())
}

/// Boot self-test for the exclusive-claim state machine.
///
/// Skips itself if a raw claim is live: these checks write `CLAIMED`/`OWNER`
/// directly, and clobbering the `netstack` daemon's claim would hand the NIC
/// back to the in-kernel stack behind its back.  In boot order nothing has
/// claimed yet, so the guard is against a future reordering rather than an
/// expected case.
///
/// # Errors
/// [`KernelError::InternalError`] if any claim transition misbehaves; the
/// failing case is logged first.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[net-raw] Running self-test...");

    if CLAIMED.load(Ordering::Acquire) {
        crate::serial_println!("[net-raw]   SKIPPED: a raw claim is already held");
        return Ok(());
    }

    // Two PIDs that name no process, so `owner_is_dead` reports both dead.
    let Some(absent) = find_absent_pid(4242) else {
        crate::serial_println!("[net-raw]   SKIPPED: no absent PID available");
        return Ok(());
    };
    let Some(other) = find_absent_pid(absent.saturating_add(1)) else {
        crate::serial_println!("[net-raw]   SKIPPED: no second absent PID available");
        return Ok(());
    };

    let result = check_unclaimed()
        .and_then(|()| check_release_by_non_owner(absent, other))
        .and_then(|()| check_dead_owner_self_heals(absent));

    // Leave the NIC unclaimed however we exited: a self-test that fails
    // half-way must not leave the in-kernel stack locked out of the NIC.
    reset_claim_state();

    result?;
    crate::serial_println!("[net-raw] Self-test PASSED");
    Ok(())
}
