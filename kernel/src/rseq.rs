//! Restartable sequences, the kernel half: keep a registered thread's
//! `cpu_id` current, and abort its critical section when it is switched out,
//! migrated or signalled inside one -- Linux's `kernel/rseq.c`
//! (`rseq_handle_notify_resume`, `rseq_ip_fixup`, `rseq_update_cpu_node_id`;
//! design-decisions 1546).
//!
//! ## What a thread registers
//!
//! `rseq(2)` ([`rseq`]: the Linux call and the native `SYS_RSEQ` are this
//! one function) registers a 32-byte `struct rseq` per thread, kept by
//! `proc::thread_clone` with its abort signature:
//!
//! | offset | field | written by |
//! |---|---|---|
//! | 0 | `u32 cpu_id_start` | the kernel: the CPU the thread runs on |
//! | 4 | `u32 cpu_id` | the kernel: the same, or `u32::MAX` once unregistered |
//! | 8 | `u64 rseq_cs` | the thread: its current `struct rseq_cs`, or 0 |
//! | 16 | `u32 flags` | the thread: deprecated, must be 0 |
//! | 20 | `u32 node_id` | the kernel: 0, one NUMA node |
//! | 24 | `u32 mm_cid` | the kernel: the CPU number, unique among the process's running threads |
//!
//! A critical section (`struct rseq_cs`: version 0, flags 0, `start_ip`,
//! `post_commit_offset`, `abort_ip`) is a stretch of user code that commits
//! with its last instruction; the thread publishes it in `rseq_cs` before
//! entering. If the thread is switched out, migrated or signalled while its
//! instruction pointer is inside, the kernel sends it to `abort_ip` instead
//! of back -- the section then starts over, on whatever CPU it is now on.
//! The four bytes before `abort_ip` must be the registered signature.
//!
//! ## When
//!
//! Every switch of a registered thread onto a CPU marks the CPU
//! ([`note_dispatch`]), and the thread's next return to user mode does the
//! work ([`exit_to_user`]) -- from a system call (`syscall::entry`), an
//! interrupt or an exception (`idt`'s exit), or a forked child's first entry
//! (`proc::fork`). That is Linux's: `rseq_preempt` marks a task at every
//! switch, voluntary or not, and the notify-resume work runs on the way out.
//! A thread picked again with nothing else having run is not marked: it
//! neither moved nor had a section raced, and Linux marks nothing then.
//! The work runs with interrupts on -- it reads and writes the thread's
//! memory, which can fault a page in -- so the thread can be switched out or
//! moved while doing it, and the dispatch that resumes it marks its new CPU;
//! the exit therefore checks again with interrupts off, until a check finds
//! nothing owed, as Linux's `exit_to_user_mode_loop` re-reads
//! `TIF_NOTIFY_RESUME`. Without that, a thread moved during the work would
//! return with the `cpu_id` of the CPU it left, and its next critical section
//! would race another thread's on that CPU's data.
//!
//! Delivering a signal does the work too ([`on_signal_delivery`]), before the
//! frame is built and whether or not a dispatch marked the CPU, so a handler
//! returns to the abort address. A thread that blocked in a system call is
//! never inside a section (a section makes no calls), so for it the work is
//! only the `cpu_id` update.
//!
//! A malformed `rseq_cs` -- a version or flags not 0, an address past user
//! space, an abort address inside its own section, or a wrong signature -- or
//! an area that cannot be read or written is answered with `SIGSEGV`, as
//! Linux answers it: posted by the exit work, which then has the exit deliver
//! it before the return; or, found while delivering a signal, in that
//! signal's place.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::error::KernelError;
use crate::mm::page_table::USER_SPACE_END;
use crate::syscall::dispatch::SyscallResult;
use crate::syscall::linux::{errno, linux_err, linux_errno_for};

/// Per CPU: a thread with an rseq area was switched in here and has not yet
/// returned to user mode since -- or a registration or a `membarrier` RSEQ
/// request asked for the work ([`notify_this_cpu`]).
static NOTIFY: [AtomicBool; crate::smp::MAX_CPUS] =
    [const { AtomicBool::new(false) }; crate::smp::MAX_CPUS];

/// Byte offsets in `struct rseq`.
const CPU_ID_START: u64 = 0;
const CPU_ID: u64 = 4;
const RSEQ_CS: u64 = 8;
const FLAGS: u64 = 16;
const NODE_ID: u64 = 20;
const MM_CID: u64 = 24;

/// `cpu_id` of a thread that is not registered (Linux's
/// `RSEQ_CPU_ID_UNINITIALIZED`).
pub const CPU_ID_UNINITIALIZED: u32 = u32::MAX;

/// Mark `cpu` as owing its current thread the rseq work: the thread was just
/// switched in there and `registered` (it has an rseq area). From
/// `sched::note_dispatch`, which knows the task.
pub fn note_dispatch(cpu: usize, registered: bool) {
    if registered && let Some(flag) = NOTIFY.get(cpu) {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Mark the calling CPU as owing the current thread the rseq work -- after
/// a registration, so the `cpu_id` it reads is current on its return; and on
/// a `membarrier` RSEQ request, so an interrupted section restarts.
pub fn notify_this_cpu() {
    if let Some(flag) = NOTIFY.get(crate::smp::fast_cpu_index()) {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Whether the calling CPU owes its current thread the rseq work. Meaningful
/// with interrupts off, when the thread cannot move.
#[must_use]
pub fn pending_here() -> bool {
    NOTIFY
        .get(crate::smp::fast_cpu_index())
        .is_some_and(|f| f.load(Ordering::Relaxed))
}

/// The last step of a return to user mode at `rip`: the rseq work this CPU
/// owes the current thread -- abort a critical section `rip` is in (rewriting
/// it to the abort address) and update the thread's `cpu_id` -- repeated
/// until a check made with interrupts off finds none owed, so the `cpu_id`
/// the thread returns with is that of the CPU it returns on (see the module
/// doc). Returns with interrupts off, which every exit's `sysretq` or
/// `iretq` wants anyway.
///
/// Returns `true` when the work posted `SIGSEGV` for a malformed area, for
/// the caller to deliver before the return, as Linux's exit loop does.
///
/// From the thread's own kernel stack on its way to ring 3, holding no lock:
/// the work runs with interrupts on, may fault a page in, and may be
/// switched out.
#[must_use]
pub fn exit_to_user(rip: &mut u64) -> bool {
    let mut posted = false;
    // SAFETY: disabling interrupts is always sound; every caller's `sysretq`
    // or `iretq` loads the user's own flags.
    unsafe {
        crate::cpu::cli();
    }
    while pending_here() {
        // Consume the mark while the thread cannot move: a dispatch during
        // the work below marks the CPU it lands on, for the next check.
        if let Some(flag) = NOTIFY.get(crate::smp::fast_cpu_index()) {
            flag.store(false, Ordering::Relaxed);
        }
        crate::cpu::irqoff_tracker::record_enable();
        // SAFETY: the caller is on the thread's own kernel stack on its way
        // to ring 3 and holds no lock (this function's contract), so it may
        // be interrupted -- and switched out, which the loop is for.
        unsafe {
            crate::cpu::sti();
        }
        if !handle(rip) {
            posted |= post_sigsegv();
        }
        // SAFETY: as above.
        unsafe {
            crate::cpu::cli();
        }
        crate::cpu::irqoff_tracker::record_disable();
    }
    posted
}

/// Before a signal frame is built for the current thread at `rip`: abort a
/// critical section it is in, so the frame records -- and the handler
/// returns to -- the abort address, and update its `cpu_id`. Linux's
/// `rseq_signal_deliver`, which runs whether or not a dispatch marked the
/// CPU.
///
/// `false` for a malformed area: the frame is then not built, and the caller
/// sends `SIGSEGV` in the signal's place -- what Linux's
/// `force_sigsegv(ksig->sig)` there comes to: the signal's handler never
/// runs, and a failure delivering `SIGSEGV` itself ends the program.
#[must_use]
pub fn on_signal_delivery(rip: &mut u64) -> bool {
    if let Some(flag) = NOTIFY.get(crate::smp::fast_cpu_index()) {
        flag.store(false, Ordering::Relaxed);
    }
    handle(rip)
}

/// The work, on the current thread's area if it has one. `false` when the
/// area or its critical section is malformed, or cannot be read or written.
fn handle(rip: &mut u64) -> bool {
    let task = crate::sched::current_task_id();
    let Some((area, _len, sig)) = crate::proc::thread_clone::lookup_rseq(task) else {
        return true;
    };
    fixup_ip(area, sig, rip).is_some() && update_cpu(area).is_some()
}

/// Linux's `force_sigsegv(0)`, for a malformed area found on the way out: a
/// `SIGSEGV` for the current thread, unblocked and not ignored. `true` when
/// one was posted.
fn post_sigsegv() -> bool {
    let task = crate::sched::current_task_id();
    crate::proc::thread::owner_process(task)
        .filter(|&p| p != 0)
        .is_some_and(|pid| crate::syscall::linux::force_sigsegv(pid, 0))
}

/// Read a `u32` of the current thread's memory.
fn read_u32(addr: u64) -> Option<u32> {
    let mut buf = [0u8; 4];
    crate::mm::user::validate_user_read(addr, 4).ok()?;
    // SAFETY: four bytes, validated readable; `buf` holds four.
    unsafe { crate::mm::user::copy_from_user(addr, buf.as_mut_ptr(), 4) }.ok()?;
    Some(u32::from_ne_bytes(buf))
}

/// Read a `u64` of the current thread's memory.
fn read_u64(addr: u64) -> Option<u64> {
    let mut buf = [0u8; 8];
    crate::mm::user::validate_user_read(addr, 8).ok()?;
    // SAFETY: eight bytes, validated readable; `buf` holds eight.
    unsafe { crate::mm::user::copy_from_user(addr, buf.as_mut_ptr(), 8) }.ok()?;
    Some(u64::from_ne_bytes(buf))
}

/// Write `bytes` to the current thread's memory at `addr`.
fn write(addr: u64, bytes: &[u8]) -> Option<()> {
    crate::mm::user::validate_user_write(addr, bytes.len()).ok()?;
    // SAFETY: `bytes.len()` bytes, validated writable.
    unsafe { crate::mm::user::copy_to_user(bytes.as_ptr(), addr, bytes.len()) }.ok()?;
    Some(())
}

/// Linux's `rseq_ip_fixup`: with a section published in the area at `area`
/// and `rip` inside it, clear the publication and send `rip` to its abort
/// address; outside one, clear it lazily. `None` for a malformed section.
fn fixup_ip(area: u64, sig: u32, rip: &mut u64) -> Option<()> {
    let cs = read_u64(area.checked_add(RSEQ_CS)?)?;
    if cs == 0 {
        return Some(());
    }
    if cs >= USER_SPACE_END {
        return None;
    }
    let version = read_u32(cs)?;
    let cs_flags = read_u32(cs.checked_add(4)?)?;
    let start_ip = read_u64(cs.checked_add(8)?)?;
    let post_commit_offset = read_u64(cs.checked_add(16)?)?;
    let abort_ip = read_u64(cs.checked_add(24)?)?;
    // rseq_get_rseq_cs's checks, in its order.
    let end = start_ip.checked_add(post_commit_offset)?;
    if start_ip >= USER_SPACE_END
        || end >= USER_SPACE_END
        || abort_ip >= USER_SPACE_END
        || version > 0
    {
        return None;
    }
    // The abort address may not lie inside its own section.
    if abort_ip.wrapping_sub(start_ip) < post_commit_offset {
        return None;
    }
    let sig_at = abort_ip.checked_sub(4)?;
    if read_u32(sig_at)? != sig {
        return None;
    }
    let inside = rip.wrapping_sub(start_ip) < post_commit_offset;
    if inside {
        // rseq_need_restart: the deprecated no-restart flags are refused, on
        // the section and on the area.
        let area_flags = read_u32(area.checked_add(FLAGS)?)?;
        if cs_flags != 0 || area_flags != 0 {
            return None;
        }
    }
    write(area.checked_add(RSEQ_CS)?, &0u64.to_ne_bytes())?;
    if inside {
        *rip = abort_ip;
    }
    Some(())
}

/// Write the calling CPU into the area at `area`, as [`update_cpu`] does on
/// the way out -- for `rseq(2)`'s registration, which Linux follows with the
/// same update before the call returns. `None` if it cannot be written.
pub fn write_current_cpu(area: u64) -> Option<()> {
    update_cpu(area)
}

/// Linux's `rseq_update_cpu_node_id`: the CPU the thread runs on, in
/// `cpu_id_start`, `cpu_id` and (as its concurrency id) `mm_cid`, and node 0.
fn update_cpu(area: u64) -> Option<()> {
    let cpu = u32::try_from(crate::smp::fast_cpu_index()).ok()?;
    write(area.checked_add(CPU_ID_START)?, &cpu.to_ne_bytes())?;
    write(area.checked_add(CPU_ID)?, &cpu.to_ne_bytes())?;
    write(area.checked_add(NODE_ID)?, &0u32.to_ne_bytes())?;
    write(area.checked_add(MM_CID)?, &cpu.to_ne_bytes())?;
    Some(())
}

/// Linux's `rseq_reset_rseq_cpu_node_id`, on unregistration: `cpu_id_start`
/// 0, `cpu_id` [`CPU_ID_UNINITIALIZED`], `node_id` and `mm_cid` 0. `None` if
/// the area cannot be written.
pub fn reset_area(area: u64) -> Option<()> {
    write(area.checked_add(CPU_ID_START)?, &0u32.to_ne_bytes())?;
    write(
        area.checked_add(CPU_ID)?,
        &CPU_ID_UNINITIALIZED.to_ne_bytes(),
    )?;
    write(area.checked_add(NODE_ID)?, &0u32.to_ne_bytes())?;
    write(area.checked_add(MM_CID)?, &0u32.to_ne_bytes())?;
    Some(())
}

/// `rseq(rseq*, len, flags, sig)` — restartable-sequence registration.
///
/// glibc 2.35+ calls this from every thread's startup; before batch
/// 121 we returned -ENOSYS, forcing glibc into a degraded mode where
/// its per-CPU malloc cache slot is computed via a `sched_getcpu`
/// fallback on every alloc, and per-thread tcache stays disabled.
///
/// Register and unregister here; the rest of rseq -- `cpu_id` kept current,
/// a critical section aborted when its thread is switched out, migrated or
/// signalled inside it -- is `crate::rseq`, on every return to user mode:
///   * Register: validate ptr/len/flags/sig, refuse double-register with
///     -EBUSY, store (ptr, len, sig) per task, write the CPU into the kernel's
///     fields of `struct rseq` (cpu_id_start, cpu_id, node_id, mm_cid), and
///     return 0 -- with the thread marked, so the fields are current when the
///     call returns.
///   * Unregister: validate the (ptr, len, sig) triple matches the stored
///     registration (-EINVAL, or -EPERM for the signature), write cpu_id back
///     to RSEQ_CPU_ID_UNINITIALIZED (-EFAULT, the registration kept, if it
///     cannot be), drop the entry, return 0.
///
/// Until 2026-10-08 the fields were written as 0 at registration and never
/// again, and no critical section was ever aborted (design-decisions 1546).
///
/// Linux ABI:
///   arg0 = rseq*    (32-byte aligned, 32 bytes valid in user space)
///   arg1 = len      (must equal sizeof(struct rseq) = 32)
///   arg2 = flags    (0 = register, RSEQ_FLAG_UNREGISTER = 1)
///   arg3 = sig      (32-bit signature, must match abort handler's
///                    preceding word; we store but never check
///                    because we never invoke an abort handler)
///
/// Gate order (matches Linux's `kernel/rseq.c::SYSCALL_DEFINE4(rseq)`):
///   1. If `flags & RSEQ_FLAG_UNREGISTER`:
///      a. Any other flag bit -> EINVAL.
///      b. No prior registration -> EINVAL.
///      c. Stored ptr != rseq -> EINVAL.
///      d. Stored len != rseq_len -> EINVAL.
///      e. Stored sig != sig -> **EPERM** (not EINVAL — discriminates
///      "tried to unregister someone else's rseq" from "wrong
///      syscall args").
///      f. Reset, return 0.
///   2. Register path (UNREGISTER bit clear):
///      a. Any flag bit -> EINVAL.
///      b. If already registered:
///         - ptr/len mismatch -> EINVAL.
///         - sig mismatch    -> EPERM.
///         - else            -> EBUSY.
///           c. Validate len/ptr/alignment/access_ok.
///           d. Store registration, initialise kernel-owned runtime fields.
///
/// Pre-batch we both returned EINVAL on sig mismatch for unregister
/// (Linux: EPERM) and returned EBUSY unconditionally on any second
/// register (Linux: EINVAL for ptr/len mismatch, EPERM for sig
/// mismatch, EBUSY only when everything matches).  We also did the
/// alignment/access_ok validation BEFORE branching on UNREGISTER,
/// so an unregister with an unmapped (but previously-valid) pointer
/// would return EFAULT where Linux returns EINVAL.
pub fn rseq(rseq_ptr: u64, len_raw: u64, flags_raw: u64, sig_raw: u64) -> SyscallResult {
    const RSEQ_STRUCT_SIZE: u64 = 32;
    const RSEQ_FLAG_UNREGISTER: u64 = 1;
    const RSEQ_STRUCT_ALIGN: u64 = 32;

    // Linux signature: `SYSCALL_DEFINE4(rseq, struct rseq __user *, rseq,
    //   u32, rseq_len, int, flags, u32, sig)`.  `rseq_len` is C `u32` in
    // rsi and `flags` is C `int` in rdx; the AMD64 syscall ABI delivers
    // both in 64-bit registers and does NOT zero/sign-extend them, so
    // only the low 32 bits are defined.  Pre-batch we held `len` and
    // `flags` as raw u64, so a probe like rseq_len=0x1_0000_0020
    // (high|32) fired the `len != 32` EINVAL gate where Linux's
    // truncated len=32 advances to the register path, and flags
    // =0x1_0000_0000 fired the `flags != 0` EINVAL gate where Linux's
    // truncated flags=0 advances likewise.  Truncate both to their
    // declared C-type width before any compare.
    #[allow(clippy::cast_possible_truncation)]
    let len_u32 = len_raw as u32;
    let len: u64 = u64::from(len_u32);
    #[allow(clippy::cast_possible_truncation)]
    let flags_u32 = flags_raw as u32;
    let flags: u64 = u64::from(flags_u32);
    #[allow(clippy::cast_possible_truncation)]
    let sig = sig_raw as u32;

    let task_id = crate::sched::current_task_id();

    if flags & RSEQ_FLAG_UNREGISTER != 0 {
        // Reject any other flag bit alongside UNREGISTER.
        if flags & !RSEQ_FLAG_UNREGISTER != 0 {
            return linux_err(errno::EINVAL);
        }
        // Linux does NOT validate alignment / access_ok on the
        // unregister path — the pointer is implicitly known to be
        // valid because it had to be validated at register time.
        // We match that order so a probe unregistering with the same
        // args it registered with sees the same errno Linux does.
        match crate::proc::thread_clone::lookup_rseq(task_id) {
            Some((stored_ptr, stored_len, stored_sig)) => {
                if stored_ptr != rseq_ptr {
                    return linux_err(errno::EINVAL);
                }
                if u64::from(stored_len) != len {
                    return linux_err(errno::EINVAL);
                }
                if stored_sig != sig {
                    // Sig mismatch on unregister: Linux returns EPERM.
                    return linux_err(errno::EPERM);
                }
                // Linux's rseq_reset_rseq_cpu_node_id: cpu_id back to
                // RSEQ_CPU_ID_UNINITIALIZED (-1), the rest 0 -- EFAULT, with
                // the registration kept, if the area cannot be written.
                if crate::rseq::reset_area(stored_ptr).is_none() {
                    return linux_err(errno::EFAULT);
                }
                // The record was found just above; a second unregistration
                // cannot have run between (the thread is this one).
                let _ = crate::proc::thread_clone::unregister_rseq(task_id);
                return SyscallResult::ok(0);
            }
            None => return linux_err(errno::EINVAL),
        }
    }

    // Register path: no flag bits should be set (UNREGISTER already
    // handled above; any remaining bit is rejected).
    if flags != 0 {
        return linux_err(errno::EINVAL);
    }

    // Already registered: discriminate ptr/len (EINVAL) from sig
    // (EPERM) from "everything matches" (EBUSY).
    if let Some((stored_ptr, stored_len, stored_sig)) =
        crate::proc::thread_clone::lookup_rseq(task_id)
    {
        if stored_ptr != rseq_ptr || u64::from(stored_len) != len {
            return linux_err(errno::EINVAL);
        }
        if stored_sig != sig {
            return linux_err(errno::EPERM);
        }
        return linux_err(errno::EBUSY);
    }

    // First-time registration: validate len/ptr/alignment/access_ok.
    if len != RSEQ_STRUCT_SIZE {
        return linux_err(errno::EINVAL);
    }
    if rseq_ptr == 0 {
        return linux_err(errno::EFAULT);
    }
    if rseq_ptr & (RSEQ_STRUCT_ALIGN - 1) != 0 {
        return linux_err(errno::EINVAL);
    }
    if let Err(e) = crate::mm::user::validate_user_read(rseq_ptr, RSEQ_STRUCT_SIZE as usize) {
        return linux_err(linux_errno_for(e));
    }
    if let Err(e) = crate::mm::user::validate_user_write(rseq_ptr, RSEQ_STRUCT_SIZE as usize) {
        return linux_err(linux_errno_for(e));
    }

    // Store before writing user space so a copy_to_user fault leaves
    // the task in a clean unregistered state on rollback.
    #[allow(clippy::cast_possible_truncation)]
    crate::proc::thread_clone::register_rseq(task_id, rseq_ptr, len as u32, sig);

    // The fields the kernel owns -- cpu_id_start, cpu_id, node_id, mm_cid --
    // get the CPU the thread runs on, now and again on its way out (Linux
    // marks the thread with `rseq_set_notify_resume`, so the call returns with
    // them current). rseq_cs (8), flags (16) and the padding (28) are the
    // thread's and stay as they are.
    crate::rseq::notify_this_cpu();
    let write_init = || -> Result<(), KernelError> {
        crate::rseq::write_current_cpu(rseq_ptr).ok_or(KernelError::PageFault)
    };
    if let Err(e) = write_init() {
        // Roll back the registration so the task is in the state it
        // was before this syscall.
        crate::proc::thread_clone::unregister_rseq(task_id);
        return linux_err(linux_errno_for(e));
    }
    SyscallResult::ok(0)
}
