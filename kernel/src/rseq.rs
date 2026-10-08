//! Restartable sequences, the kernel half: keep a registered thread's
//! `cpu_id` current, and abort its critical section when it is switched out,
//! migrated or signalled inside one -- Linux's `kernel/rseq.c`
//! (`rseq_handle_notify_resume`, `rseq_ip_fixup`, `rseq_update_cpu_node_id`;
//! design-decisions 1546).
//!
//! ## What a thread registers
//!
//! `rseq(2)` (`syscall::linux::sys_rseq`) registers a 32-byte `struct rseq`
//! per thread, kept by `proc::thread_clone` with its abort signature:
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

use crate::mm::page_table::USER_SPACE_END;

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
