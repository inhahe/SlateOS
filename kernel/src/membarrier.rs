//! `membarrier(2)`: make every thread of the caller's process -- or of every
//! process -- pass a full memory barrier, for both ABIs: the Linux
//! `membarrier` and the native `SYS_MEMBARRIER` (1147) are this one function
//! (design-decisions 1545). The barrier itself is an interrupt to the CPUs
//! running the address space ([`crate::cpusync`]), or a grace period
//! ([`crate::rcu::synchronize`]) for `MEMBARRIER_CMD_GLOBAL`; the RSEQ
//! commands also restart the interrupted threads' rseq critical sections
//! ([`crate::rseq`]).
//!
//! Moved out of `syscall/linux.rs` on 2026-10-08 so the native handler calls
//! it directly rather than through the Linux translation layer: the gate
//! `check-linux-only-capabilities` reads a capability reached only through
//! the Linux layer as one native code cannot ask for.

use crate::error::KernelError;
use crate::proc::pcb;
use crate::syscall::dispatch::SyscallResult;
use crate::syscall::linux::{errno, linux_err};

// ---------------------------------------------------------------------------
// membarrier(2) command numbers (from <linux/membarrier.h>) and the per-mm
// registration model.  Shared between [`membarrier`] and the pure decision
// helper `membarrier_decide` (which is unit-tested in isolation).
// ---------------------------------------------------------------------------
const MEMBARRIER_CMD_QUERY: u64 = 0;
const MEMBARRIER_CMD_GLOBAL: u64 = 1 << 0;
const MEMBARRIER_CMD_GLOBAL_EXPEDITED: u64 = 1 << 1;
const MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED: u64 = 1 << 2;
const MEMBARRIER_CMD_PRIVATE_EXPEDITED: u64 = 1 << 3;
const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED: u64 = 1 << 4;
const MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE: u64 = 1 << 5;
const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE: u64 = 1 << 6;
const MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ: u64 = 1 << 7;
const MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ: u64 = 1 << 8;
const MEMBARRIER_CMD_GET_REGISTRATIONS: u64 = 1 << 9;
const MEMBARRIER_CMD_FLAG_CPU: u64 = 1;

// Per-mm registration READY bits stored opaquely in `Process::membarrier_state`.
// The numeric layout is private to this module; only the GET_REGISTRATIONS
// return value (a bitmask of REGISTER command numbers) is ABI-visible, and it
// is reconstructed by `membarrier_registrations_mask`.
const MEMBARRIER_READY_GLOBAL_EXPEDITED: u32 = 1 << 0;
const MEMBARRIER_READY_PRIVATE_EXPEDITED: u32 = 1 << 1;
const MEMBARRIER_READY_PRIVATE_EXPEDITED_SYNC_CORE: u32 = 1 << 2;
const MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ: u32 = 1 << 3;

/// What a (non-QUERY) `membarrier` command resolves to given the issuing mm's
/// current registration `state`.  Kept separate from [`membarrier`] so the
/// gating logic can be unit-tested without a process context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MembarrierAction {
    /// Make the CPUs `scope` names pass a barrier, then return 0.
    Barrier(MembarrierScope),
    /// Set these READY bits in the issuing mm's state, then return 0.
    Register(u32),
    /// Return the registered-command bitmask (computed from the real state by
    /// `membarrier_registrations_mask`).
    GetRegistrations,
    /// Return `-EPERM`: an expedited barrier was issued without first
    /// registering the matching command (Linux gates expedited barriers on
    /// prior registration, and that check precedes the single-CPU shortcut).
    Eperm,
    /// Return `-EINVAL`: unrecognised command.
    Einval,
}

/// How far a `membarrier` barrier reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MembarrierScope {
    /// `GLOBAL`: every CPU passes a quiescent state -- Linux's
    /// `synchronize_rcu`, here `rcu::synchronize`. Slow, and disturbs no CPU.
    Global,
    /// `GLOBAL_EXPEDITED`: every CPU running user code is interrupted
    /// (`cpusync`) -- a superset of Linux's, which interrupts only CPUs
    /// running a process registered for it.
    GlobalExpedited,
    /// `PRIVATE_EXPEDITED` and its `SYNC_CORE` form: every CPU running the
    /// caller's address space is interrupted.
    Private,
    /// `PRIVATE_EXPEDITED_RSEQ`: as [`Private`](Self::Private) -- or one CPU
    /// of them, with `MEMBARRIER_CMD_FLAG_CPU` -- and each interrupted thread
    /// inside an rseq critical section restarts it (`crate::rseq`).
    PrivateRseq,
}

/// Decide how to handle a non-QUERY `membarrier` `cmd` given the issuing mm's
/// registration `state` (the READY bitmask).  Pure — no side effects — so the
/// EPERM gating can be exhaustively unit-tested.
///
/// Mirrors Linux v6.6 `kernel/sched/membarrier.c`:
///   * `GLOBAL` / `GLOBAL_EXPEDITED` issue need no registration.
///   * each `PRIVATE_EXPEDITED*` issue returns `-EPERM` unless its matching
///     READY bit is set (set by the corresponding `REGISTER_*`).
///   * `REGISTER_*` set their READY bit and succeed.
///   * `GET_REGISTRATIONS` reports the registered commands.
fn membarrier_decide(cmd: u64, state: u32) -> MembarrierAction {
    match cmd {
        MEMBARRIER_CMD_GLOBAL => MembarrierAction::Barrier(MembarrierScope::Global),
        MEMBARRIER_CMD_GLOBAL_EXPEDITED => {
            MembarrierAction::Barrier(MembarrierScope::GlobalExpedited)
        }
        MEMBARRIER_CMD_PRIVATE_EXPEDITED => {
            if state & MEMBARRIER_READY_PRIVATE_EXPEDITED != 0 {
                MembarrierAction::Barrier(MembarrierScope::Private)
            } else {
                MembarrierAction::Eperm
            }
        }
        MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE => {
            if state & MEMBARRIER_READY_PRIVATE_EXPEDITED_SYNC_CORE != 0 {
                MembarrierAction::Barrier(MembarrierScope::Private)
            } else {
                MembarrierAction::Eperm
            }
        }
        // The interrupted threads' rseq critical sections restart too.
        MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ => {
            if state & MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ != 0 {
                MembarrierAction::Barrier(MembarrierScope::PrivateRseq)
            } else {
                MembarrierAction::Eperm
            }
        }
        MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ => {
            MembarrierAction::Register(MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ)
        }
        MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED => {
            MembarrierAction::Register(MEMBARRIER_READY_GLOBAL_EXPEDITED)
        }
        MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED => {
            MembarrierAction::Register(MEMBARRIER_READY_PRIVATE_EXPEDITED)
        }
        MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE => {
            MembarrierAction::Register(MEMBARRIER_READY_PRIVATE_EXPEDITED_SYNC_CORE)
        }
        MEMBARRIER_CMD_GET_REGISTRATIONS => MembarrierAction::GetRegistrations,
        _ => MembarrierAction::Einval,
    }
}

/// Reconstruct the ABI-visible `GET_REGISTRATIONS` bitmask (a set of
/// `MEMBARRIER_CMD_REGISTER_*` command numbers) from an mm's opaque READY
/// `state`.  Mirrors Linux v6.6 `membarrier_get_registrations`.
fn membarrier_registrations_mask(state: u32) -> u32 {
    let mut mask = 0u32;
    if state & MEMBARRIER_READY_GLOBAL_EXPEDITED != 0 {
        #[allow(clippy::cast_possible_truncation)]
        {
            mask |= MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED as u32;
        }
    }
    if state & MEMBARRIER_READY_PRIVATE_EXPEDITED != 0 {
        #[allow(clippy::cast_possible_truncation)]
        {
            mask |= MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED as u32;
        }
    }
    if state & MEMBARRIER_READY_PRIVATE_EXPEDITED_SYNC_CORE != 0 {
        #[allow(clippy::cast_possible_truncation)]
        {
            mask |= MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE as u32;
        }
    }
    if state & MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ != 0 {
        #[allow(clippy::cast_possible_truncation)]
        {
            mask |= MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ as u32;
        }
    }
    mask
}

/// `membarrier(cmd, flags, cpu_id)` — make the threads sharing the
/// caller's address space (or every thread, for the `GLOBAL` commands) pass
/// a full memory barrier before this returns.
///
/// ## How
///
/// - `PRIVATE_EXPEDITED` and `PRIVATE_EXPEDITED_SYNC_CORE` interrupt every
///   CPU whose last dispatch was into the caller's address space
///   (`sched::cpus_running_aspace`, `cpusync::sync_cpus`): the interrupt is
///   the barrier, and its `iretq` the serializing instruction `SYNC_CORE`
///   asks for. A CPU not running the process passes both when it next
///   switches to it (`cpusync`'s module doc says why that is enough).
/// - `GLOBAL_EXPEDITED` interrupts every CPU running user code: more than
///   Linux, which asks only CPUs running a process registered for it.
/// - `GLOBAL` waits for every CPU to pass a quiescent state
///   (`rcu::synchronize`), as Linux's `synchronize_rcu` does: slower, and it
///   interrupts nobody.
/// - `PRIVATE_EXPEDITED_RSEQ` does as `PRIVATE_EXPEDITED`, to one CPU with
///   `MEMBARRIER_CMD_FLAG_CPU` (a negative `cpu_id` meaning all, as on Linux),
///   and each interrupted thread inside an rseq critical section restarts it
///   on its way back to user mode (`crate::rseq`, design-decisions 1546).
///
/// Until 2026-10-07 every barrier fenced the calling CPU alone, on the
/// reasoning that no other thread could share the caller's address space --
/// true until threads ran on other CPUs
/// (requests/d-a-membarrier-needs-the-kernel-to-interrupt-the-other-cpus.md;
/// design-decisions 1545).
///
/// ## Registration gating (TD8)
///
/// Linux still requires an expedited command to be **registered**
/// before it may be issued: `membarrier_private_expedited()` returns
/// `-EPERM` when the issuing mm's `membarrier_state` lacks the
/// matching `*_READY` bit, and that check runs *before* the
/// single-CPU shortcut — so even on our uniprocessor an unregistered
/// `PRIVATE_EXPEDITED*` issue must be `-EPERM`, not 0.  We honour
/// this with a per-mm READY bitmask (`Process::membarrier_state`,
/// shared across the process's threads so a thread may register and
/// a sibling issue).  `REGISTER_*` set their bit; the three
/// `PRIVATE_EXPEDITED*` issues are gated on it; `GET_REGISTRATIONS`
/// reports the registered set.  The gating decision lives in the
/// pure, unit-tested [`membarrier_decide`].  `GLOBAL` and
/// `GLOBAL_EXPEDITED` issue need no registration (Linux does not
/// EPERM them).  The boot self-test runs in kernel context with no
/// owner mm; there the registration model is moot (no sibling
/// userspace threads), so the fence is always permitted — see the
/// `u32::MAX` gating-state note at the issue site.
///
/// ## Linux ABI reference (cmd values from `<linux/membarrier.h>`)
///
///   0 = QUERY
///   1 = GLOBAL
///   2 = GLOBAL_EXPEDITED
///   4 = REGISTER_GLOBAL_EXPEDITED
///   8 = PRIVATE_EXPEDITED
///  16 = REGISTER_PRIVATE_EXPEDITED
///  32 = PRIVATE_EXPEDITED_SYNC_CORE
///  64 = REGISTER_PRIVATE_EXPEDITED_SYNC_CORE
/// 128 = PRIVATE_EXPEDITED_RSEQ
/// 256 = REGISTER_PRIVATE_EXPEDITED_RSEQ
/// 512 = GET_REGISTRATIONS (Linux 6.3+)
///
/// `flags` defines a single bit: `MEMBARRIER_CMD_FLAG_CPU = 1`,
/// which (combined with `cpu_id`) targets one CPU instead of all
/// CPUs in the MM.  Legal only on EXPEDITED commands.  In our
/// model targeting one CPU vs all is moot because no other thread
/// shares the MM; we accept the bit on the documented commands
/// and reject it on REGISTER_*/GLOBAL where Linux also rejects it.
pub fn membarrier(cmd_raw: u64, flags_raw: u64, cpu_raw: u64) -> SyscallResult {
    // Command numbers, READY bits, and the gating helper live at module scope
    // (just above) so the `membarrier_decide` logic can be unit-tested.

    // Linux signature: `SYSCALL_DEFINE3(membarrier, int, cmd, unsigned
    // int, flags, int, cpu_id)`.  All three args are declared as 32-bit
    // C types, so the x86_64 syscall ABI truncates each register to its
    // low 32 bits before the body runs.  Pre-batch (301) we held cmd
    // and flags as raw u64 and matched against u64-typed bit constants
    // (MEMBARRIER_CMD_GLOBAL = 1<<0, etc.) — discriminators pre-batch:
    //   * cmd = 0x1_0000_0001: Linux truncates to MEMBARRIER_CMD_GLOBAL
    //     and runs the fence (returns 0).  Pre-batch: 0x1_0000_0001
    //     didn't match any arm -> terminal EINVAL.
    //   * cmd = 1 (GLOBAL), flags = 0x1_0000_0000: Linux truncates flags
    //     to 0, passes the `flags & !FLAG_CPU` gate, runs the fence
    //     (returns 0).  Pre-batch: 0x1_0000_0000 & !1 = 0x1_0000_0000
    //     != 0 -> EINVAL.
    // Cast each arg at entry to restore the 32-bit view.  Sign-extension
    // would alias negative ints across the u64 constant range, but Linux
    // never assigns negative cmd values and FLAG_CPU is bit 0; so zero-
    // extending via `as u32` then `u64::from` preserves the same answer
    // shape as Linux's signed switch (negatives still hit `default ->
    // EINVAL` either way).
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let cmd_i32 = cmd_raw as i32;
    #[allow(clippy::cast_sign_loss)]
    let cmd: u64 = u64::from(cmd_i32 as u32);
    #[allow(clippy::cast_possible_truncation)]
    let flags_u32 = flags_raw as u32;
    let flags: u64 = u64::from(flags_u32);
    // cpu_id is C `int` in the Linux prototype, delivered in rdx.  The
    // AMD64 syscall ABI does not sign- or zero-extend `int` across the
    // syscall instruction, so Linux observes only the low 32 bits.
    // Pre-batch we tested the raw u64 against 0 in the QUERY arm, so a
    // caller passing 0x1_0000_0000 (high-only) was rejected with EINVAL
    // where Linux's truncated cpu_id=0 advances to the QUERY bitmask
    // return.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let cpu_id_i32 = cpu_raw as i32;

    // QUERY enumerates the supported commands as a bitmask.  flags
    // and cpu_id must be 0.
    if cmd == MEMBARRIER_CMD_QUERY {
        if flags != 0 || cpu_id_i32 != 0 {
            return linux_err(errno::EINVAL);
        }
        let supported = MEMBARRIER_CMD_GLOBAL
            | MEMBARRIER_CMD_GLOBAL_EXPEDITED
            | MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
            | MEMBARRIER_CMD_PRIVATE_EXPEDITED
            | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED
            | MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE
            | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE
            | MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ
            | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ
            | MEMBARRIER_CMD_GET_REGISTRATIONS;
        #[allow(clippy::cast_possible_wrap)]
        return SyscallResult::ok(supported as i64);
    }

    // Any unknown flag bit -> EINVAL.
    if flags & !MEMBARRIER_CMD_FLAG_CPU != 0 {
        return linux_err(errno::EINVAL);
    }

    // ## Batch 503 — narrow FLAG_CPU acceptance to PRIVATE_EXPEDITED_RSEQ only
    //
    // Linux v6.6 `kernel/sched/membarrier.c::SYSCALL_DEFINE3(membarrier)`
    // validates `flags` via a per-cmd switch (verbatim):
    //
    //   switch (cmd) {
    //   case MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ:
    //       if (unlikely(flags && flags != MEMBARRIER_CMD_FLAG_CPU))
    //           return -EINVAL;
    //       break;
    //   default:
    //       if (unlikely(flags))
    //           return -EINVAL;
    //   }
    //
    // So in v6.6 ONLY `MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ` accepts
    // `MEMBARRIER_CMD_FLAG_CPU`; every other cmd (including the two
    // earlier PRIVATE_EXPEDITED variants and the QUERY/GLOBAL family)
    // falls into the `default` arm which rejects any non-zero flags.
    //
    // Pre-batch we allowed FLAG_CPU on PRIVATE_EXPEDITED and
    // PRIVATE_EXPEDITED_SYNC_CORE as well, citing "per the Linux
    // man-page" — but the man-page documents the future ABI of those
    // commands, not v6.6's switch surface.  glibc 2.38+ probes the
    // flag-acceptance shape per-cmd to decide whether to use the
    // per-CPU expedited barrier (FLAG_CPU + RSEQ) or fall back to the
    // broadcast variant; pre-batch a `(PRIVATE_EXPEDITED, FLAG_CPU)`
    // probe succeeded where Linux v6.6 returns EINVAL, mis-training
    // the runtime into believing PRIVATE_EXPEDITED supports targeted
    // delivery.
    if flags & MEMBARRIER_CMD_FLAG_CPU != 0 && cmd != MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ {
        return linux_err(errno::EINVAL);
    }

    // Resolve the issuing mm's per-process registration state (TD8).  A
    // userspace caller always has an owner process; only the in-kernel boot
    // self-test reaches here with none.  In that kernel context there is no
    // mm registration model and no sibling userspace threads to synchronise,
    // so an expedited barrier is a valid local no-op regardless of
    // registration — we feed `u32::MAX` to the gating helper to bypass the
    // EPERM check there, while reporting the *real* (empty) state for
    // GET_REGISTRATIONS.
    let pid = crate::proc::thread::owner_process(crate::sched::current_task_id());
    let real_state = pid
        .and_then(crate::proc::pcb::membarrier_state)
        .unwrap_or(0);
    let gating_state = if pid.is_some() { real_state } else { u32::MAX };

    match membarrier_decide(cmd, gating_state) {
        MembarrierAction::Barrier(scope) => {
            match scope {
                MembarrierScope::Global => crate::rcu::synchronize(),
                MembarrierScope::GlobalExpedited => {
                    crate::cpusync::sync_cpus(crate::sched::cpus_running_user());
                }
                MembarrierScope::Private | MembarrierScope::PrivateRseq => {
                    // A kernel caller (the boot self-test) has no address
                    // space of its own for another CPU to be running.
                    let aspace = pid.and_then(crate::proc::pcb::get_pml4).unwrap_or(0);
                    if aspace == 0 {
                        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
                    } else if scope == MembarrierScope::Private {
                        crate::cpusync::sync_cpus(crate::sched::cpus_running_aspace(aspace));
                    } else {
                        // FLAG_CPU names one CPU -- none, if it is past the
                        // CPUs there are -- and a negative cpu_id all of
                        // them, as Linux's membarrier_private_expedited.
                        let running = crate::sched::cpus_running_aspace(aspace);
                        let targets = if flags & MEMBARRIER_CMD_FLAG_CPU == 0 || cpu_id_i32 < 0 {
                            running
                        } else {
                            u32::try_from(cpu_id_i32)
                                .ok()
                                .and_then(|c| 1u64.checked_shl(c))
                                .map_or(0, |bit| running & bit)
                        };
                        crate::cpusync::sync_cpus_restarting_rseq(targets);
                    }
                }
            }
            SyscallResult::ok(0)
        }
        MembarrierAction::Register(bits) => {
            // Record the registration in the per-mm state so a later
            // expedited issue of the matching command is permitted.  In the
            // no-owner-process kernel context there is nowhere to record it;
            // the no-op return matches Linux's "REGISTER always succeeds".
            if let Some(p) = pid {
                let _ = crate::proc::pcb::membarrier_register(p, bits);
            }
            SyscallResult::ok(0)
        }
        MembarrierAction::GetRegistrations => {
            // Linux 6.3+: bitmask of REGISTER_* commands currently registered
            // for this mm.  Computed from the real state (0 in kernel context).
            SyscallResult::ok(i64::from(membarrier_registrations_mask(real_state)))
        }
        MembarrierAction::Eperm => linux_err(errno::EPERM),
        MembarrierAction::Einval => linux_err(errno::EINVAL),
    }
}

#[inline(never)]
pub fn self_test_registration() -> crate::error::KernelResult<()> {
    use crate::serial_println;
    // TD8 — membarrier(2) per-mm registration gating.  Exercise the pure
    // decision helper exhaustively, then the per-mm READY-bit store via a
    // throwaway process (the EPERM path can't be driven through the syscall
    // layer at boot, where the caller has no owner mm).

    // 1. Pure `membarrier_decide` gating.
    let cases: &[(u64, u32, MembarrierAction)] = &[
        // GLOBAL family needs no registration.
        (
            MEMBARRIER_CMD_GLOBAL,
            0,
            MembarrierAction::Barrier(MembarrierScope::Global),
        ),
        (
            MEMBARRIER_CMD_GLOBAL_EXPEDITED,
            0,
            MembarrierAction::Barrier(MembarrierScope::GlobalExpedited),
        ),
        // PRIVATE_EXPEDITED is EPERM until registered.
        (MEMBARRIER_CMD_PRIVATE_EXPEDITED, 0, MembarrierAction::Eperm),
        (
            MEMBARRIER_CMD_PRIVATE_EXPEDITED,
            MEMBARRIER_READY_PRIVATE_EXPEDITED,
            MembarrierAction::Barrier(MembarrierScope::Private),
        ),
        (
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE,
            0,
            MembarrierAction::Eperm,
        ),
        (
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE,
            MEMBARRIER_READY_PRIVATE_EXPEDITED_SYNC_CORE,
            MembarrierAction::Barrier(MembarrierScope::Private),
        ),
        (
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ,
            0,
            MembarrierAction::Eperm,
        ),
        (
            MEMBARRIER_CMD_PRIVATE_EXPEDITED_RSEQ,
            MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ,
            MembarrierAction::Barrier(MembarrierScope::PrivateRseq),
        ),
        // A non-matching READY bit does NOT permit a different command.
        (
            MEMBARRIER_CMD_PRIVATE_EXPEDITED,
            MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ,
            MembarrierAction::Eperm,
        ),
        // REGISTER_* map to their READY bit.
        (
            MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED,
            0,
            MembarrierAction::Register(MEMBARRIER_READY_GLOBAL_EXPEDITED),
        ),
        (
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED,
            0,
            MembarrierAction::Register(MEMBARRIER_READY_PRIVATE_EXPEDITED),
        ),
        (
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE,
            0,
            MembarrierAction::Register(MEMBARRIER_READY_PRIVATE_EXPEDITED_SYNC_CORE),
        ),
        (
            MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ,
            0,
            MembarrierAction::Register(MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ),
        ),
        (
            MEMBARRIER_CMD_GET_REGISTRATIONS,
            0,
            MembarrierAction::GetRegistrations,
        ),
        // Unknown command.
        (1 << 10, 0, MembarrierAction::Einval),
    ];
    for &(cmd, state, expected) in cases {
        let got = membarrier_decide(cmd, state);
        if got != expected {
            serial_println!(
                "[syscall/linux]   FAIL: membarrier_decide(cmd={}, state={}) = {:?}, expected {:?}",
                cmd,
                state,
                got,
                expected
            );
            return Err(KernelError::InternalError);
        }
    }

    // 2. GET_REGISTRATIONS bitmask reconstruction.
    if membarrier_registrations_mask(0) != 0 {
        serial_println!("[syscall/linux]   FAIL: membarrier_registrations_mask(0) != 0");
        return Err(KernelError::InternalError);
    }
    #[allow(clippy::cast_possible_truncation)]
    let priv_cmd = MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED as u32;
    if membarrier_registrations_mask(MEMBARRIER_READY_PRIVATE_EXPEDITED) != priv_cmd {
        serial_println!("[syscall/linux]   FAIL: registrations_mask(PRIVATE) != REGISTER_PRIVATE");
        return Err(KernelError::InternalError);
    }
    let all_ready = MEMBARRIER_READY_GLOBAL_EXPEDITED
        | MEMBARRIER_READY_PRIVATE_EXPEDITED
        | MEMBARRIER_READY_PRIVATE_EXPEDITED_SYNC_CORE
        | MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ;
    #[allow(clippy::cast_possible_truncation)]
    let all_cmds = (MEMBARRIER_CMD_REGISTER_GLOBAL_EXPEDITED
        | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED
        | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_SYNC_CORE
        | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ) as u32;
    if membarrier_registrations_mask(all_ready) != all_cmds {
        serial_println!("[syscall/linux]   FAIL: registrations_mask(all) != all REGISTER cmds");
        return Err(KernelError::InternalError);
    }

    // 3. Per-mm READY-bit store via a throwaway process.
    {
        let pid = pcb::create("membarrier-reg-test", 0);
        // Fresh mm: no registrations -> a PRIVATE_EXPEDITED issue would EPERM.
        if pcb::membarrier_state(pid) != Some(0) {
            serial_println!("[syscall/linux]   FAIL: fresh membarrier_state != Some(0)");
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
        if membarrier_decide(MEMBARRIER_CMD_PRIVATE_EXPEDITED, 0) != MembarrierAction::Eperm {
            serial_println!("[syscall/linux]   FAIL: unregistered PRIVATE_EXPEDITED not Eperm");
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
        // Register PRIVATE_EXPEDITED -> bit set, idempotent.
        if pcb::membarrier_register(pid, MEMBARRIER_READY_PRIVATE_EXPEDITED)
            != Some(MEMBARRIER_READY_PRIVATE_EXPEDITED)
        {
            serial_println!("[syscall/linux]   FAIL: register PRIVATE returned wrong state");
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
        if pcb::membarrier_register(pid, MEMBARRIER_READY_PRIVATE_EXPEDITED)
            != Some(MEMBARRIER_READY_PRIVATE_EXPEDITED)
        {
            serial_println!("[syscall/linux]   FAIL: re-register PRIVATE not idempotent");
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
        // The matching issue is now permitted.
        let state = pcb::membarrier_state(pid).unwrap_or(0);
        if membarrier_decide(MEMBARRIER_CMD_PRIVATE_EXPEDITED, state)
            != MembarrierAction::Barrier(MembarrierScope::Private)
        {
            serial_println!("[syscall/linux]   FAIL: registered PRIVATE_EXPEDITED not a barrier");
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
        // A different expedited command is not permitted by it: SYNC_CORE
        // stays EPERM.
        if membarrier_decide(MEMBARRIER_CMD_PRIVATE_EXPEDITED_SYNC_CORE, state)
            != MembarrierAction::Eperm
        {
            serial_println!("[syscall/linux]   FAIL: SYNC_CORE permitted by PRIVATE registration");
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
        // Register a second command; both bits coexist.
        let both = MEMBARRIER_READY_PRIVATE_EXPEDITED | MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ;
        if pcb::membarrier_register(pid, MEMBARRIER_READY_PRIVATE_EXPEDITED_RSEQ) != Some(both) {
            serial_println!("[syscall/linux]   FAIL: register RSEQ did not OR into state");
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
        #[allow(clippy::cast_possible_truncation)]
        let both_cmds = (MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED
            | MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED_RSEQ) as u32;
        if membarrier_registrations_mask(both) != both_cmds {
            serial_println!("[syscall/linux]   FAIL: registrations_mask(both) wrong");
            pcb::destroy(pid);
            return Err(KernelError::InternalError);
        }
        pcb::destroy(pid);
        // After destroy the mm is gone -> None (no registration to consult).
        if pcb::membarrier_state(pid).is_some() {
            serial_println!("[syscall/linux]   FAIL: membarrier_state on destroyed pid not None");
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[syscall/linux]   membarrier per-mm registration gating (TD8): OK");
    Ok(())
}
