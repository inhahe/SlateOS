//! POSIX signal-delivery shim (kernel side).
//!
//! Our OS deliberately does **not** use Unix signals for process control
//! (see `design.txt`: "No Unix signals for process control. Use IPC
//! messages."). Hardware faults are delivered as SEH-style exceptions
//! (`proc/exception.rs`), and process lifecycle is driven by IPC.
//!
//! However, the POSIX compatibility layer must still support
//! `signal()`/`sigaction()`/`kill()` for ported software (bash,
//! coreutils, Python). This module provides the *minimum* kernel
//! machinery to make asynchronous signal delivery work, modelled closely
//! on the exception-delivery path:
//!
//! 1. The POSIX runtime registers a single process-wide **trampoline**
//!    (`register_trampoline`). The trampoline is the only thing the
//!    kernel knows how to jump to; the per-signal handler table lives
//!    entirely in userspace.
//! 2. `kill()`/`raise()` post a signal into a target process's **pending
//!    set** (`set_pending`, via the `SYS_SIGNAL_SEND` syscall).
//! 3. When the target process next returns to userspace from a syscall,
//!    the syscall-return path checks for a deliverable signal
//!    (`take_deliverable`) and, if the trampoline is registered, builds a
//!    [`SignalContext`] on the user stack and redirects execution to the
//!    trampoline (see `handlers::deliver_pending_signal`).
//! 4. The trampoline invokes the userspace handler then calls
//!    `SYS_SIGNAL_RETURN` to restore the interrupted context.
//!
//! ## What the kernel does and does not know
//!
//! The kernel tracks only three things per process: the pending set, the
//! blocked mask, and the trampoline address. It does **not** track
//! per-signal dispositions — userspace owns that table and decides
//! whether to terminate, ignore, or invoke a handler. The one exception
//! is the kernel-side *default-action* table, used solely to decide what
//! happens when a signal is posted to a process that has **no trampoline
//! registered** (a non-POSIX process, or one that has not yet run its
//! libc init): terminating signals kill it, everything else is dropped.
//! `SIGKILL` is always fatal and can never be delivered to a handler.
//!
//! ## Concurrency
//!
//! All per-process state lives behind a single `Mutex`. Posting a signal
//! (possibly from another process) and consuming one (always the running
//! process itself) both take this lock briefly. To keep the syscall hot
//! path cheap, a global pending counter lets the return path skip the
//! lock entirely when no signals are pending anywhere.

use crate::error::{KernelError, KernelResult};
use crate::proc::pcb::ProcessId;
use crate::serial_println;
use alloc::collections::BTreeMap;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Signal numbers
// ---------------------------------------------------------------------------

/// Number of supported signals. POSIX signals are numbered 1..=NSIG.
/// We support a 64-bit pending set, so signals 1..=64.
pub const NSIG: u32 = 64;

/// `SIGKILL` — always fatal, never catchable. Standard Linux number.
pub const SIGKILL: u32 = 9;

/// `SIGSTOP` — stop signal, never catchable. Standard Linux number.
pub const SIGSTOP: u32 = 19;

/// Default disposition of a signal for a process with no handler.
///
/// This mirrors the Linux default-action table closely enough for the
/// kernel's fallback decision (terminate vs. drop) when no userspace
/// trampoline is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAction {
    /// Terminate the process (optionally with a core dump — we don't
    /// distinguish; both terminate).
    Terminate,
    /// Ignore the signal (no effect).
    Ignore,
    /// Stop the process. We have no suspend mechanism, so the fallback
    /// treats this as a drop.
    Stop,
    /// Continue a stopped process. No-op in our model.
    Continue,
}

/// Look up the default action for a signal number.
///
/// Based on the Linux signal(7) default-action table. Real-time signals
/// (>= 32) default to Terminate, matching Linux.
#[must_use]
pub fn default_action(sig: u32) -> DefaultAction {
    match sig {
        // Ignored by default.
        17 /* SIGCHLD */ | 23 /* SIGURG */ | 28 /* SIGWINCH */ => {
            DefaultAction::Ignore
        }
        // Stop signals: SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU.
        19..=22 => DefaultAction::Stop,
        // Continue.
        18 /* SIGCONT */ => DefaultAction::Continue,
        // Everything else (including SIGKILL and RT signals) terminates.
        _ => DefaultAction::Terminate,
    }
}

/// Returns `true` if `sig` is a valid signal number (1..=NSIG).
#[must_use]
pub fn is_valid_signal(sig: u32) -> bool {
    sig >= 1 && sig <= NSIG
}

/// Convert a 1-based signal number to its bit in a 64-bit set.
///
/// Returns `None` if the signal number is out of range.
#[inline]
#[must_use]
fn signal_bit(sig: u32) -> Option<u64> {
    if is_valid_signal(sig) {
        // sig is 1..=64, so the shift amount is 0..=63 — always valid.
        // `checked_sub`/`checked_shl` keep this arithmetic-side-effect
        // free; both branches are statically guaranteed to be `Some`.
        let shift = sig.checked_sub(1)?;
        1u64.checked_shl(shift)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Signal context (userspace ABI)
// ---------------------------------------------------------------------------

/// Saved CPU context at the point a signal interrupted userspace.
///
/// The kernel writes this onto the user stack before jumping to the
/// trampoline, and restores from it on `SYS_SIGNAL_RETURN`. It captures
/// exactly the register state needed to reconstruct the interrupted
/// [`SyscallFrame`](crate::syscall::entry::SyscallFrame), plus `rax`
/// (the interrupted syscall's return value) and the delivered signal
/// number.
///
/// # ABI
///
/// This struct is part of the userspace ABI. Fields must not be
/// reordered or resized. The trampoline receives `signum` in `rdi` and a
/// pointer to this struct in `rsi`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SignalContext {
    /// The signal number being delivered (1..=NSIG).
    pub signum: u64,
    /// Saved RAX — the interrupted syscall's return value, restored on
    /// `SYS_SIGNAL_RETURN` so the interrupted code sees the correct
    /// result.
    pub rax: u64,
    /// Saved RDI (syscall arg0).
    pub rdi: u64,
    /// Saved RSI (syscall arg1).
    pub rsi: u64,
    /// Saved RDX (syscall arg2).
    pub rdx: u64,
    /// Saved R10 (syscall arg3).
    pub r10: u64,
    /// Saved R8 (syscall arg4).
    pub r8: u64,
    /// Saved R9 (syscall arg5).
    pub r9: u64,
    /// Saved RBX.
    pub rbx: u64,
    /// Saved RBP.
    pub rbp: u64,
    /// Saved R12.
    pub r12: u64,
    /// Saved R13.
    pub r13: u64,
    /// Saved R14.
    pub r14: u64,
    /// Saved R15.
    pub r15: u64,
    /// Interrupted instruction pointer.
    pub rip: u64,
    /// Interrupted stack pointer.
    pub rsp: u64,
    /// Interrupted RFLAGS.
    pub rflags: u64,
}

/// Size of the signal context in bytes (17 × 8 = 136).
pub const SIGNAL_CONTEXT_SIZE: usize = core::mem::size_of::<SignalContext>();

// ---------------------------------------------------------------------------
// Per-process signal state
// ---------------------------------------------------------------------------

/// Per-process signal bookkeeping.
#[derive(Debug, Clone, Copy, Default)]
struct SignalState {
    /// Pending set: bit `n-1` set means signal `n` is pending.
    pending: u64,
    /// Blocked mask: bit `n-1` set means signal `n` is blocked.
    blocked: u64,
    /// Userspace trampoline address (0 = not registered).
    trampoline: u64,
}

/// All per-process signal state, keyed by process ID.
static SIGNAL_STATES: Mutex<BTreeMap<ProcessId, SignalState>> =
    Mutex::new(BTreeMap::new());

/// Count of pending signals across all processes.
///
/// Used as a cheap fast-path gate in the syscall-return delivery check so
/// the common case (no signals pending anywhere) avoids taking the lock.
/// May over-count transiently (e.g. while a blocked signal sits pending),
/// which only costs an occasional needless lock acquisition — never a
/// missed delivery.
static PENDING_COUNT: AtomicUsize = AtomicUsize::new(0);

// ---------------------------------------------------------------------------
// Trampoline registration
// ---------------------------------------------------------------------------

/// Register (or replace) the process-wide signal trampoline.
///
/// `addr == 0` unregisters, reverting to "no asynchronous delivery"
/// (pending signals stay pending but are not delivered).
pub fn register_trampoline(pid: ProcessId, addr: u64) {
    let mut states = SIGNAL_STATES.lock();
    states.entry(pid).or_default().trampoline = addr;
}

/// Get the registered trampoline address for a process, if any.
#[must_use]
pub fn trampoline(pid: ProcessId) -> Option<u64> {
    let states = SIGNAL_STATES.lock();
    states.get(&pid).map(|s| s.trampoline).filter(|&a| a != 0)
}

/// Returns `true` if the process has a non-zero trampoline registered.
#[must_use]
pub fn has_trampoline(pid: ProcessId) -> bool {
    trampoline(pid).is_some()
}

/// Clear all signal state for a process. Called on process death.
///
/// Decrements the global pending counter by the number of pending
/// signals this process had, keeping the fast-path gate accurate.
pub fn remove(pid: ProcessId) {
    let mut states = SIGNAL_STATES.lock();
    if let Some(state) = states.remove(&pid) {
        let n = state.pending.count_ones() as usize;
        if n != 0 {
            PENDING_COUNT.fetch_sub(n, Ordering::Relaxed);
        }
    }
}

/// Reset signal state across `exec()`.
///
/// POSIX resets all caught signals to their default disposition on a
/// successful `exec` (the new image's handler table is empty until its
/// libc init re-registers). Since our per-signal dispositions live in
/// userspace and are discarded with the old address space, the kernel's
/// job is simply to drop the now-stale trampoline so we never jump to a
/// garbage address in the new image. Pending signals are preserved
/// (matching POSIX), and will be delivered once the new image registers
/// its trampoline.
pub fn on_exec(pid: ProcessId) {
    let mut states = SIGNAL_STATES.lock();
    if let Some(state) = states.get_mut(&pid) {
        state.trampoline = 0;
        // Blocked mask is also reset on exec per POSIX.
        state.blocked = 0;
    }
}

/// Inherit signal state from a parent across `fork()`.
///
/// POSIX semantics: the child inherits the parent's blocked-signal mask
/// and signal dispositions, but the set of pending signals is **empty**
/// in the child.  Our per-signal dispositions live in userspace (and are
/// carried over automatically by the copy-on-write address space), so the
/// kernel's job is to copy the blocked mask and the trampoline address
/// (the child's CoW-copied trampoline lives at the same user address) and
/// to start the child with no pending signals.
///
/// Overwrites any existing child state (the child is freshly created, so
/// there should be none, but this is idempotent).
pub fn inherit_for_fork(parent: ProcessId, child: ProcessId) {
    let mut states = SIGNAL_STATES.lock();
    let (blocked, trampoline) = states
        .get(&parent)
        .map_or((0, 0), |s| (s.blocked, s.trampoline));
    // If the child somehow already had pending signals recorded, drop
    // them from the global counter before overwriting.
    if let Some(existing) = states.get(&child) {
        let n = existing.pending.count_ones() as usize;
        if n != 0 {
            PENDING_COUNT.fetch_sub(n, Ordering::Relaxed);
        }
    }
    states.insert(
        child,
        SignalState {
            pending: 0,
            blocked,
            trampoline,
        },
    );
}

// ---------------------------------------------------------------------------
// Blocked mask
// ---------------------------------------------------------------------------

/// Set the blocked-signal mask for a process, returning the previous mask.
///
/// `SIGKILL` and `SIGSTOP` cannot be blocked (their bits are always
/// cleared from the stored mask), matching POSIX.
pub fn set_blocked(pid: ProcessId, mask: u64) -> u64 {
    // SIGKILL (bit 8) and SIGSTOP (bit 18) can never be blocked.
    let unblockable = (1u64 << (SIGKILL - 1)) | (1u64 << (SIGSTOP - 1));
    let mask = mask & !unblockable;
    let mut states = SIGNAL_STATES.lock();
    let state = states.entry(pid).or_default();
    let old = state.blocked;
    state.blocked = mask;
    old
}

/// Get the blocked-signal mask for a process.
#[must_use]
pub fn blocked(pid: ProcessId) -> u64 {
    let states = SIGNAL_STATES.lock();
    states.get(&pid).map(|s| s.blocked).unwrap_or(0)
}

/// Get the pending-signal set for a process (without clearing it).
#[must_use]
pub fn pending(pid: ProcessId) -> u64 {
    let states = SIGNAL_STATES.lock();
    states.get(&pid).map(|s| s.pending).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Posting and consuming signals
// ---------------------------------------------------------------------------

/// Set a signal pending on a process.
///
/// The signal number must be valid (caller checks via [`is_valid_signal`]).
/// Returns `true` if the bit transitioned from clear to set (so the
/// global counter was incremented). This is a pure state operation — it
/// does **not** make any delivery or termination decision; callers that
/// need the no-trampoline fallback should consult [`classify_post`].
pub fn set_pending(pid: ProcessId, sig: u32) -> bool {
    let Some(bit) = signal_bit(sig) else {
        return false;
    };
    let mut states = SIGNAL_STATES.lock();
    let state = states.entry(pid).or_default();
    if state.pending & bit == 0 {
        state.pending |= bit;
        PENDING_COUNT.fetch_add(1, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// The kernel's decision about what to do with a posted signal.
///
/// Returned by [`classify_post`] so the syscall handler can perform the
/// side effect (set-pending vs. terminate vs. drop) without this module
/// reaching into the process-termination machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostDecision {
    /// Mark the signal pending; it will be delivered to the trampoline on
    /// the target's next return to userspace.
    Deliver,
    /// The target has no trampoline and the signal's default action is
    /// fatal: the caller must terminate the process with this exit code.
    Terminate(i32),
    /// The signal was dropped (ignored / unsupported stop on a process
    /// with no handler).
    Drop,
}

/// Decide what to do with a signal posted to `pid`, and record pending
/// state if delivery is chosen.
///
/// * `SIGKILL` is always `Terminate` (never catchable).
/// * If the process has a trampoline registered, the signal is marked
///   pending (`Deliver`).
/// * Otherwise the default action decides: terminating signals →
///   `Terminate(128 + sig)`, everything else → `Drop`.
///
/// The caller is responsible for the actual termination (the kernel's
/// process-kill path) when `Terminate` is returned.
#[must_use]
pub fn classify_post(pid: ProcessId, sig: u32) -> PostDecision {
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let term_code = 128i32.wrapping_add(sig as i32);

    // SIGKILL is unconditionally fatal and never delivered to a handler.
    if sig == SIGKILL {
        return PostDecision::Terminate(term_code);
    }

    if has_trampoline(pid) {
        set_pending(pid, sig);
        PostDecision::Deliver
    } else {
        match default_action(sig) {
            DefaultAction::Terminate => PostDecision::Terminate(term_code),
            DefaultAction::Ignore
            | DefaultAction::Stop
            | DefaultAction::Continue => PostDecision::Drop,
        }
    }
}

/// Pick and consume the lowest-numbered deliverable signal for a process.
///
/// A signal is deliverable if it is pending and not blocked. The chosen
/// signal's pending bit is cleared. Returns the signal number, or `None`
/// if nothing is deliverable.
#[must_use]
pub fn take_deliverable(pid: ProcessId) -> Option<u32> {
    let mut states = SIGNAL_STATES.lock();
    let state = states.get_mut(&pid)?;
    let deliverable = state.pending & !state.blocked;
    if deliverable == 0 {
        return None;
    }
    // Lowest set bit = lowest-numbered signal (POSIX delivers low first).
    let bit_index = deliverable.trailing_zeros();
    let bit = 1u64 << bit_index;
    state.pending &= !bit;
    PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
    // bit_index is 0..=63, so +1 is 1..=64 — a valid signal number.
    // `saturating_add` keeps this arithmetic-side-effect free.
    Some(bit_index.saturating_add(1))
}

/// Pick and consume the lowest-numbered pending signal that is also in
/// `mask`, for a `signalfd` read.
///
/// Unlike [`take_deliverable`], this does **not** consult the blocked
/// mask: a `signalfd` consumes any pending signal that is in the fd's
/// acceptance mask (the process is expected to have blocked those signals
/// so they aren't first delivered to a handler, but the dequeue itself is
/// gated only by the fd mask, matching Linux's `signalfd_dequeue`). The
/// chosen signal's pending bit is cleared. Returns the signal number, or
/// `None` if no pending signal falls within `mask`.
#[must_use]
pub fn take_pending_in_mask(pid: ProcessId, mask: u64) -> Option<u32> {
    let mut states = SIGNAL_STATES.lock();
    let state = states.get_mut(&pid)?;
    let eligible = state.pending & mask;
    if eligible == 0 {
        return None;
    }
    let bit_index = eligible.trailing_zeros();
    let bit = 1u64 << bit_index;
    state.pending &= !bit;
    PENDING_COUNT.fetch_sub(1, Ordering::Relaxed);
    // bit_index is 0..=63, so +1 is 1..=64 — a valid signal number.
    Some(bit_index.saturating_add(1))
}

/// Returns `true` if any pending signal for `pid` falls within `mask`.
///
/// Used by the `poll`/`select`/`epoll` readiness check for a `signalfd`:
/// the fd is readable exactly when a masked signal is pending. Does not
/// consume anything.
#[must_use]
pub fn has_pending_in_mask(pid: ProcessId, mask: u64) -> bool {
    let states = SIGNAL_STATES.lock();
    states
        .get(&pid)
        .is_some_and(|s| s.pending & mask != 0)
}

/// Cheap fast-path gate: `true` if any signal might be pending anywhere.
///
/// The syscall-return delivery path calls this before doing any
/// per-process work, so the common (no-signals) case is a single relaxed
/// atomic load.
#[inline]
#[must_use]
pub fn any_pending() -> bool {
    PENDING_COUNT.load(Ordering::Relaxed) != 0
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Synthetic PID base for self-tests (well outside any real PID range).
const TEST_PID_BASE: ProcessId = 0xFFFF_5160_0000;

/// Signal-shim self-tests — pure state machinery (no userspace delivery).
///
/// Verifies the pending/blocked/trampoline bookkeeping, the default-action
/// table, the no-trampoline post classification, and ABI struct layout.
/// The actual asynchronous-delivery path (stack frame building + frame
/// rewrite) is exercised by the userspace POSIX test programs; it cannot
/// be unit-tested here without a ring-3 harness.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[signal] Running signal-shim self-test...");

    test_context_abi()?;
    test_signal_validity()?;
    test_default_actions()?;
    test_trampoline_registry()?;
    test_pending_and_take()?;
    test_blocked_masking()?;
    test_classify_post()?;
    test_on_exec()?;
    test_pending_count_accounting()?;
    test_signalfd_dequeue()?;

    serial_println!("[signal] Signal-shim self-test PASSED (10 tests)");
    Ok(())
}

/// Helper: assert a condition, logging and returning an error on failure.
fn check(cond: bool, what: &str) -> KernelResult<()> {
    if cond {
        Ok(())
    } else {
        serial_println!("[signal]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
}

fn test_context_abi() -> KernelResult<()> {
    check(SIGNAL_CONTEXT_SIZE == 17 * 8, "SignalContext size == 136")?;
    check(
        SIGNAL_CONTEXT_SIZE == core::mem::size_of::<SignalContext>(),
        "constant matches size_of",
    )?;
    check(
        core::mem::align_of::<SignalContext>() == 8,
        "SignalContext align == 8",
    )?;
    serial_println!("[signal]   context ABI ({SIGNAL_CONTEXT_SIZE}B): OK");
    Ok(())
}

fn test_signal_validity() -> KernelResult<()> {
    check(!is_valid_signal(0), "0 invalid")?;
    check(is_valid_signal(1), "1 valid")?;
    check(is_valid_signal(NSIG), "NSIG valid")?;
    check(!is_valid_signal(NSIG + 1), "NSIG+1 invalid")?;
    check(signal_bit(1) == Some(1), "bit(1)==1")?;
    check(signal_bit(64) == Some(1u64 << 63), "bit(64)==1<<63")?;
    check(signal_bit(0).is_none(), "bit(0)==None")?;
    serial_println!("[signal]   signal validity: OK");
    Ok(())
}

fn test_default_actions() -> KernelResult<()> {
    check(default_action(9) == DefaultAction::Terminate, "SIGKILL term")?;
    check(default_action(15) == DefaultAction::Terminate, "SIGTERM term")?;
    check(default_action(17) == DefaultAction::Ignore, "SIGCHLD ign")?;
    check(default_action(28) == DefaultAction::Ignore, "SIGWINCH ign")?;
    check(default_action(19) == DefaultAction::Stop, "SIGSTOP stop")?;
    check(default_action(18) == DefaultAction::Continue, "SIGCONT cont")?;
    check(default_action(34) == DefaultAction::Terminate, "RT term")?;
    serial_println!("[signal]   default-action table: OK");
    Ok(())
}

fn test_trampoline_registry() -> KernelResult<()> {
    let p = TEST_PID_BASE + 1;
    check(!has_trampoline(p), "initially no trampoline")?;
    register_trampoline(p, 0x4000);
    check(trampoline(p) == Some(0x4000), "trampoline stored")?;
    check(has_trampoline(p), "has_trampoline true")?;
    register_trampoline(p, 0);
    check(!has_trampoline(p), "unregister clears")?;
    remove(p);
    serial_println!("[signal]   trampoline registry: OK");
    Ok(())
}

fn test_pending_and_take() -> KernelResult<()> {
    let p = TEST_PID_BASE + 2;
    register_trampoline(p, 0x4000);
    check(pending(p) == 0, "no pending initially")?;
    check(set_pending(p, 10), "set 10 returns true")?;
    check(set_pending(p, 2), "set 2 returns true")?;
    check(!set_pending(p, 10), "re-set 10 returns false")?;
    check(pending(p) == ((1 << 9) | (1 << 1)), "pending mask")?;
    // Lowest-numbered first.
    check(take_deliverable(p) == Some(2), "take 2 first")?;
    check(take_deliverable(p) == Some(10), "take 10 next")?;
    check(take_deliverable(p).is_none(), "nothing left")?;
    remove(p);
    serial_println!("[signal]   pending set/take: OK");
    Ok(())
}

fn test_blocked_masking() -> KernelResult<()> {
    let p = TEST_PID_BASE + 3;
    register_trampoline(p, 0x4000);
    set_pending(p, 5);
    set_blocked(p, 1 << 4); // block signal 5
    check(take_deliverable(p).is_none(), "blocked not deliverable")?;
    set_blocked(p, 0);
    check(take_deliverable(p) == Some(5), "unblocked deliverable")?;
    // SIGKILL/SIGSTOP cannot be blocked.
    let p2 = TEST_PID_BASE + 30;
    let requested =
        (1u64 << (SIGKILL - 1)) | (1u64 << (SIGSTOP - 1)) | (1u64 << 0);
    set_blocked(p2, requested);
    check(blocked(p2) == (1u64 << 0), "KILL/STOP unblockable")?;
    remove(p);
    remove(p2);
    serial_println!("[signal]   blocked masking: OK");
    Ok(())
}

fn test_classify_post() -> KernelResult<()> {
    let p = TEST_PID_BASE + 4;
    check(
        classify_post(p, SIGKILL) == PostDecision::Terminate(128 + 9),
        "no-tramp SIGKILL terminate",
    )?;
    check(
        classify_post(p, 15) == PostDecision::Terminate(128 + 15),
        "no-tramp SIGTERM terminate",
    )?;
    check(classify_post(p, 17) == PostDecision::Drop, "no-tramp SIGCHLD drop")?;
    register_trampoline(p, 0x4000);
    check(classify_post(p, 15) == PostDecision::Deliver, "tramp SIGTERM deliver")?;
    check(pending(p) & (1 << 14) == (1 << 14), "SIGTERM pending after deliver")?;
    check(
        classify_post(p, SIGKILL) == PostDecision::Terminate(128 + 9),
        "SIGKILL terminate even with tramp",
    )?;
    remove(p);
    serial_println!("[signal]   classify_post: OK");
    Ok(())
}

fn test_on_exec() -> KernelResult<()> {
    let p = TEST_PID_BASE + 5;
    register_trampoline(p, 0x4000);
    set_pending(p, 12);
    set_blocked(p, 1 << 0);
    on_exec(p);
    check(!has_trampoline(p), "exec clears trampoline")?;
    check(blocked(p) == 0, "exec clears blocked mask")?;
    check(pending(p) & (1 << 11) == (1 << 11), "exec preserves pending")?;
    remove(p);
    serial_println!("[signal]   on_exec semantics: OK");
    Ok(())
}

fn test_pending_count_accounting() -> KernelResult<()> {
    let p = TEST_PID_BASE + 6;
    register_trampoline(p, 0x4000);
    let before = PENDING_COUNT.load(Ordering::Relaxed);
    set_pending(p, 3);
    set_pending(p, 4);
    check(
        PENDING_COUNT.load(Ordering::Relaxed) == before.saturating_add(2),
        "count incremented by 2",
    )?;
    remove(p); // removing a process with 2 pending should drop the count
    check(
        PENDING_COUNT.load(Ordering::Relaxed) == before,
        "count restored after remove",
    )?;
    serial_println!("[signal]   pending-count accounting: OK");
    Ok(())
}

fn test_signalfd_dequeue() -> KernelResult<()> {
    let p = TEST_PID_BASE + 7;
    // Signal 5 and signal 10 pending; a signalfd masks only {5, 7}.
    set_pending(p, 5);
    set_pending(p, 10);
    let fd_mask = (1u64 << 4) | (1u64 << 6); // signals 5 and 7
    check(has_pending_in_mask(p, fd_mask), "signal 5 visible to fd mask")?;
    check(
        !has_pending_in_mask(p, 1u64 << 6),
        "signal 7 not pending → mask {7} not readable",
    )?;
    // Dequeue: only signal 5 is in the mask (10 is not), so 5 comes out
    // and 10 stays pending.
    check(take_pending_in_mask(p, fd_mask) == Some(5), "dequeue 5 from fd mask")?;
    check(
        take_pending_in_mask(p, fd_mask).is_none(),
        "no further masked signal after 5 consumed",
    )?;
    check(pending(p) & (1u64 << 9) == (1u64 << 9), "signal 10 still pending")?;
    // Dequeue ignores the blocked mask (unlike take_deliverable).
    set_blocked(p, 1u64 << 9); // block signal 10
    check(
        take_pending_in_mask(p, 1u64 << 9) == Some(10),
        "blocked signal still dequeued by signalfd",
    )?;
    remove(p);
    serial_println!("[signal]   signalfd dequeue (mask/blocked-independent): OK");
    Ok(())
}
