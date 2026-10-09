//! Linux `clone(CLONE_VM | CLONE_THREAD)` translation — spawning a new
//! thread inside an existing process (the `pthread_create` path).
//!
//! ## Model
//!
//! Linux `clone()` does two very different things depending on flags:
//!
//! 1. *Fork-equivalent* (no `CLONE_VM`): make a copy-on-write
//!    duplicate of the address space — handled by [`super::fork`].
//! 2. *Thread creation* (`CLONE_VM | CLONE_THREAD`): create a new
//!    schedulable thread that shares the parent's address space, fd
//!    table, signal handlers, and credentials.  This module handles
//!    that path.
//!
//! ## Trampoline
//!
//! The new thread starts its life in ring 0 just like any other
//! kernel-spawned task.  Its entry point is [`clone_thread_trampoline`],
//! which:
//!
//!   1. Reclaims the heap-allocated register image (a boxed
//!      `[u64; REG_IMAGE_LEN]`; see [`REG_IMAGE_LEN`]).
//!   2. If `CLONE_SETTLS` was requested, writes the new FS base into
//!      `IA32_FS_BASE` (MSR 0xC000_0100) so the child sees its own TLS
//!      block immediately on the first ring-3 instruction.
//!   3. Builds an IRETQ frame with the child's RIP (= parent's
//!      post-syscall RIP), the caller-supplied `child_stack` as RSP,
//!      and `RAX = 0` so userspace observes a clone return value of 0.
//!   4. IRETQs to ring 3.
//!
//! ## Futex on exit (`CLONE_CHILD_CLEARTID`)
//!
//! glibc's `pthread_join` blocks on a futex at the address it passed
//! as `ctid` to `clone()`.  When the thread exits, the kernel must
//! atomically write 0 to that address and wake one waiter.  We track
//! the (task → ctid) mapping in [`CLEAR_CHILD_TID`] and clear it from
//! the [`on_thread_exit_hook`] which is called at the top of
//! [`super::thread::on_thread_exit`] while CR3 still points at the
//! dying thread's address space.

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;

use crate::error::{KernelError, KernelResult};
use crate::sched::task::TaskId;
use crate::syscall::entry::SyscallFrame;

use super::pcb::ProcessId;
use super::thread;

/// Number of `u64` slots in the cloned thread's saved register image.
///
/// Layout (each slot is 8 bytes, byte offsets are slot * 8):
///
/// | slot | field   | source                                        |
/// |------|---------|-----------------------------------------------|
/// | 0    | RIP     | `frame.user_rip` (instruction after SYSCALL)  |
/// | 1    | CS      | `USER_CS` (ring-3 code selector)              |
/// | 2    | RFLAGS  | `frame.user_rflags` or `0x202` if cleared     |
/// | 3    | RSP     | `args.child_stack` (caller-supplied)          |
/// | 4    | SS      | `USER_DS` (ring-3 data selector)              |
/// | 5    | RDI     | `frame.arg0` — preserved for the caller       |
/// | 6    | RSI     | `frame.arg1`                                  |
/// | 7    | RDX     | `frame.arg2`                                  |
/// | 8    | R10     | `frame.arg3`                                  |
/// | 9    | R8      | `frame.arg4`                                  |
/// | 10   | R9      | `frame.arg5`                                  |
/// | 11   | RBX     | `frame.rbx`                                   |
/// | 12   | RBP     | `frame.rbp`                                   |
/// | 13   | R12     | `frame.r12`                                   |
/// | 14   | R13     | `frame.r13`                                   |
/// | 15   | R14     | `frame.r14`                                   |
/// | 16   | R15     | `frame.r15`                                   |
/// | 17   | FS_BASE | `args.new_tls` (0 = leave MSR untouched)      |
///
/// Keep in sync with [`clone_thread_trampoline`] — its inline asm
/// reads from these byte offsets directly.
const REG_IMAGE_LEN: usize = 18;

/// IA32_FS_BASE model-specific register.  Written via `WRMSR` to
/// install a new TLS base for the cloned thread before IRETQ.
const IA32_FS_BASE: u32 = 0xC000_0100;

/// Caller-supplied arguments to [`clone_thread`].  These mirror the
/// Linux `clone(flags, child_stack, ptid, ctid, tls)` parameters in
/// the x86_64 syscall ABI register order, except `child_tid` and
/// `tls` which the Linux x86_64 ABI passes as args 3 and 4 (we
/// already extracted those from the syscall frame at the dispatch
/// site).
#[derive(Clone, Copy, Debug)]
pub struct CloneThreadArgs {
    /// Raw `CLONE_*` flag bitmask from the caller.
    pub flags: u64,
    /// User-virtual address of the top of the child's stack.  Must
    /// be non-zero and already mapped in the (shared) address space.
    pub child_stack: u64,
    /// User-virtual address where `CLONE_PARENT_SETTID` should
    /// deposit the new thread's TID, in the **parent's** view of the
    /// address space.  Since `CLONE_VM` shares the AS, this is the
    /// same address space the child sees.  0 means no copy.
    pub parent_tid_ptr: u64,
    /// User-virtual address used by `CLONE_CHILD_SETTID` /
    /// `CLONE_CHILD_CLEARTID`.  Written *and* registered for
    /// futex-wake-on-exit.  0 means no copy / no registration.
    pub child_tid_ptr: u64,
    /// New thread-local-storage base.  Written to `IA32_FS_BASE`
    /// when `CLONE_SETTLS` is set.  0 means leave the MSR alone.
    pub new_tls: u64,
}

// ---------------------------------------------------------------------------
// CLONE_CHILD_CLEARTID registration
// ---------------------------------------------------------------------------

/// Map from cloned task -> userspace `ctid` address that should be
/// zeroed (and woken via futex) when the task exits.  Populated by
/// [`clone_thread`] when `CLONE_CHILD_CLEARTID` was set, drained by
/// [`on_thread_exit_hook`].
static CLEAR_CHILD_TID: Mutex<BTreeMap<TaskId, u64>> = Mutex::new(BTreeMap::new());

/// Register `ctid_ptr` so `*ctid_ptr = 0; futex_wake(ctid_ptr, 1)` is
/// performed when `task_id` exits.  Replaces any prior registration
/// (matches Linux's `set_tid_address(2)` semantics).
pub fn register_clear_child_tid(task_id: TaskId, ctid_ptr: u64) {
    if ctid_ptr == 0 {
        CLEAR_CHILD_TID.lock().remove(&task_id);
    } else {
        CLEAR_CHILD_TID.lock().insert(task_id, ctid_ptr);
    }
}

/// Drop `task_id`'s `ctid` registration without performing the exit-time
/// zero-write or futex wake.
///
/// Used only to unwind a failed two-phase spawn: the ctid is registered while
/// the child is still suspended, so if `thread::admit` then fails the child
/// will never run and never reach `on_thread_exit_hook`.  Without this the
/// entry would sit in [`CLEAR_CHILD_TID`] forever, keyed on a task id that is
/// gone — a slow leak, and a latent mis-fire if task ids are ever recycled.
pub fn forget_clear_child_tid(task_id: TaskId) {
    CLEAR_CHILD_TID.lock().remove(&task_id);
}

/// Look up (without removing) a task's registered `ctid` address.
///
/// Used by `prctl(PR_GET_TID_ADDRESS)` to report the address of the
/// clear-child-tid futex back to userspace (gdb and a couple of
/// thread-debugging libraries call this).  Returns `None` if the
/// task has not called `set_tid_address` (or registered via
/// `CLONE_CHILD_CLEARTID`); callers should report 0 in that case to
/// match Linux, which returns the zero-initialised slot.
pub fn lookup_clear_child_tid(task_id: TaskId) -> Option<u64> {
    CLEAR_CHILD_TID.lock().get(&task_id).copied()
}

// ---------------------------------------------------------------------------
// Robust futex list registration (set_robust_list / get_robust_list)
// ---------------------------------------------------------------------------

/// Map from task → (`head` userspace pointer, `len` in bytes) recorded
/// by `set_robust_list(2)`.  `head` is a `struct robust_list_head*` in
/// the calling task's address space; `len` is required by Linux to equal
/// `sizeof(struct robust_list_head)` (24 bytes on x86_64) — we store
/// whatever the caller supplied after validating the length so
/// `get_robust_list(2)` can return it verbatim.
///
/// Entries are removed in [`on_thread_exit_hook`] so the table does not
/// grow without bound across the lifetime of the system.
///
/// What we deliberately do NOT do here: walk the robust list on thread
/// exit and wake the recorded futexes.  glibc's robust-mutex protocol
/// expects the kernel to run that walk when the owner dies abruptly,
/// and we do not yet implement it.  This is recorded in todo.txt; the
/// limitation only matters once we ship pthread mutex types that opt
/// into `PTHREAD_MUTEX_ROBUST_NP`, which we do not.
static ROBUST_LIST: Mutex<BTreeMap<TaskId, (u64, u64)>> = Mutex::new(BTreeMap::new());

/// Register `head` and `len` as the task's robust-list head pointer.
/// `head == 0` is a legal request — it unregisters any prior entry
/// (matches Linux, where storing NULL means "no robust list").
pub fn register_robust_list(task_id: TaskId, head: u64, len: u64) {
    if head == 0 {
        ROBUST_LIST.lock().remove(&task_id);
    } else {
        ROBUST_LIST.lock().insert(task_id, (head, len));
    }
}

/// Look up the robust-list registration for `task_id`.  Returns
/// `(head, len)` if one is registered, `None` otherwise.
///
/// The caller of `get_robust_list(2)` always wants *something* back
/// (Linux returns NULL/`sizeof(robust_list_head)` for an unregistered
/// task), so the syscall layer fills in the default — this getter
/// reports the raw state.
pub fn lookup_robust_list(task_id: TaskId) -> Option<(u64, u64)> {
    ROBUST_LIST.lock().get(&task_id).copied()
}

// ---------------------------------------------------------------------------
// rseq registration (rseq(2))
// ---------------------------------------------------------------------------

/// Per-task `rseq(2)` registration.
///
/// `ptr` is the userspace pointer to the `struct rseq` (32 bytes,
/// 32-byte-aligned) the task registered with; `sig` is the 32-bit
/// signature value that must precede the `abort_ip` of any rseq
/// critical section the task installs (Linux checks this at abort
/// time as a defence against attacker-supplied abort handlers).
///
/// `rseq(2)`'s register/unregister/duplicate checks are answered from
/// this map, and `crate::rseq` reads it on a registered thread's way back
/// to user mode -- to keep the area's `cpu_id` current and abort a critical
/// section the thread was switched out, moved or signalled in
/// (design-decisions 1546). Whether a thread has an entry is mirrored on its
/// scheduler record (`sched::set_rseq_registered`), which is how a dispatch
/// knows to mark its CPU without taking this lock.
///
/// A forked child gets its forking thread's entry (Linux's `rseq_fork`); a
/// new thread starts with none; an exec drops it ([`release_for_exec`]).
/// Entries are removed in [`on_thread_exit_hook`] so the table does
/// not grow without bound across the lifetime of the system.  No
/// userspace write happens at exit (the thread is dying; Linux also
/// does not zero the struct on exit).
static RSEQ: Mutex<BTreeMap<TaskId, (u64, u32, u32)>> = Mutex::new(BTreeMap::new());

/// Release what thread `task_id` registered in the image an exec is about to
/// replace -- Linux's `exec_mm_release`, from `spawn::exec_process` just
/// before the old address space is torn down, while it is still mapped:
///
/// - its PI futexes are handed to their waiters and its robust list walked,
///   as at exit ([`on_thread_exit_hook`]'s order): the old image's mutexes die with
///   it (Linux's `futex_exec_release`);
/// - the robust-list head, the rseq area and the clear-child-tid word are
///   forgotten.
///
/// Each names an address in the old image. Until 2026-10-08 all three outlived
/// an exec, and the thread's exit then walked, or wrote into, whatever the new
/// image had put at those addresses -- unless its C library happened to
/// register over them first.
pub fn release_for_exec(task_id: TaskId) {
    let robust_head = ROBUST_LIST.lock().remove(&task_id).map(|(head, _len)| head);
    crate::ipc::futex::exit_pi_owned_futexes(task_id);
    if let Some(head) = robust_head {
        crate::ipc::futex::exit_robust_list(head, task_id);
    }
    let _ = unregister_rseq(task_id);
    CLEAR_CHILD_TID.lock().remove(&task_id);
}

/// Register `ptr` (`rseq` userspace pointer), `len` (struct length —
/// Linux requires 32), and `sig` (abort-signature) for `task_id`.
///
/// Caller is expected to have validated `ptr` (non-NULL, aligned, in
/// user space, readable+writable for `len` bytes) and `len == 32`.
pub fn register_rseq(task_id: TaskId, ptr: u64, len: u32, sig: u32) {
    RSEQ.lock().insert(task_id, (ptr, len, sig));
    // A task that has gone has nothing to be dispatched with.
    let _ = crate::sched::set_rseq_registered(task_id, true);
}

/// Look up the rseq registration for `task_id`.  Returns
/// `(ptr, len, sig)` if registered, `None` otherwise.
pub fn lookup_rseq(task_id: TaskId) -> Option<(u64, u32, u32)> {
    RSEQ.lock().get(&task_id).copied()
}

/// Remove the rseq registration for `task_id`, if any.  Returns
/// the previous value for the caller's sanity-checks.
pub fn unregister_rseq(task_id: TaskId) -> Option<(u64, u32, u32)> {
    let removed = RSEQ.lock().remove(&task_id);
    // As above: a task that has gone has no flag to clear.
    let _ = crate::sched::set_rseq_registered(task_id, false);
    removed
}

/// Called from [`super::thread::on_thread_exit`] BEFORE the thread is
/// detached from its process.
///
/// In the normal self-exit path CR3 still points at the dying thread's
/// address space, so `copy_to_user` against the registered `ctid`
/// pointer (and the robust-list / PI-futex walks) resolve.  When the
/// hook is instead invoked from a *different* address space (a reaper
/// or boot self-test calling `on_thread_exit` for another task), the
/// user-memory passes are skipped — see the AS-active guard in the
/// body — while the in-kernel registration drops still run so the
/// tables never leak.
///
/// If the task had `CLONE_CHILD_CLEARTID` set and its AS is active:
///   1. Writes a 32-bit zero to `*ctid` in user space (best-effort —
///      a destroyed mapping just produces an EFAULT we ignore).
///   2. Wakes one waiter on the futex at `ctid` so a `pthread_join`
///      caller spinning on it observes the zero and proceeds.
pub fn on_thread_exit_hook(task_id: TaskId) {
    // The futex/robust-list/ctid recovery passes all dereference USER
    // pointers (PI mutex words, the robust-list chain, the ctid word).
    // Those reads/writes are only valid when the active page tables
    // belong to the dying thread's process.  In the normal teardown
    // path (a thread exiting itself) CR3 still points at its own
    // address space, so they resolve.  But this hook also runs from
    // cross-address-space cleanup paths — e.g. a boot self-test or a
    // reaper that calls `thread::on_thread_exit(other_task)` while its
    // OWN address space is active.  In that case the dying thread's
    // user pointers (lazily-mapped mmap regions, etc.) are not present
    // in the active AS, and a blind `read_user` would fault fatally in
    // ring 0.  Guard the user-memory work behind an AS-active check;
    // the in-kernel registration drops always run so the tables never
    // leak regardless of which AS is current.
    let as_active = match crate::proc::thread::owner_process(task_id) {
        Some(pid) => {
            crate::proc::pcb::get_pml4(pid) == Some(crate::mm::page_table::active_pml4_phys())
        }
        None => false,
    };

    // Robust-mutex + PI-owner cleanup for the dying thread.  Two
    // independent recovery passes run (see `ipc::futex` for the full
    // rationale):
    //
    //   1. Hand off every PI mutex the thread still owns to its
    //      highest-priority kernel-blocked waiter, setting FUTEX_OWNER_DIED.
    //      Without this a `FUTEX_LOCK_PI` waiter on a mutex whose owner
    //      died would hang forever.
    //   2. Walk the thread's registered userspace robust list (the
    //      PTHREAD_MUTEX_ROBUST protocol) and set FUTEX_OWNER_DIED on every
    //      lock it still holds, waking one waiter per non-PI mutex.
    //
    // Order matters: the PI handoff runs first so its authoritative
    // ownership transfer is not clobbered by the robust walk (which, seeing
    // a freshly-transferred live owner, leaves that word untouched).
    //
    // Both passes touch user memory, so they only run when the dying
    // thread's address space is the active one.  When it is not, the
    // process is being torn down from another context and there are no
    // live waiters in *this* AS to hand off to, so skipping is correct.
    let robust_head = ROBUST_LIST.lock().get(&task_id).map(|&(head, _len)| head);
    if as_active {
        crate::ipc::futex::exit_pi_owned_futexes(task_id);
        if let Some(head) = robust_head {
            crate::ipc::futex::exit_robust_list(head, task_id);
        }
    }

    // Drop the robust-list registration so the table does not grow across
    // thread lifetimes.  Removal happens before the CLEAR_CHILD_TID
    // handling, because robust-list cleanup is independent of
    // CLONE_CHILD_CLEARTID.  This runs unconditionally to avoid leaks.
    ROBUST_LIST.lock().remove(&task_id);

    // Drop any rseq registration.  Linux does not zero the userspace
    // struct on exit (the thread is dying — there is no observer left
    // in this address space's task list); we match that.
    RSEQ.lock().remove(&task_id);

    // A traced thread's trace ends with it.
    crate::proc::ptrace::on_thread_exit(task_id);

    let ctid_ptr = match CLEAR_CHILD_TID.lock().remove(&task_id) {
        Some(p) => p,
        None => return,
    };

    // The ctid zero-write and wake also touch user memory / are only
    // meaningful in the dying thread's AS.  Skip them cross-AS.
    if !as_active {
        return;
    }

    // 1. Zero the user-visible ctid.
    let zero: i32 = 0;
    // SAFETY: copy_to_user validates the user range and uses STAC/
    // CLAC for SMAP.  We verified above that the active address space
    // belongs to the dying thread's process; if the page has already
    // been unmapped the copy returns an error which we deliberately
    // ignore — the thread is exiting anyway.
    let _ = unsafe {
        crate::mm::user::copy_to_user(
            (&raw const zero).cast::<u8>(),
            ctid_ptr,
            core::mem::size_of::<i32>(),
        )
    };

    // 2. Wake one waiter on the futex at ctid_ptr.  This is the
    //    sentinel pthread_join blocks on.
    let _ = crate::ipc::futex::futex_wake(ctid_ptr, 1);
}

// ---------------------------------------------------------------------------
// Trampoline
// ---------------------------------------------------------------------------

/// Ring-0 entry point for a freshly-cloned thread.
///
/// `image_raw` is `Box::into_raw(Box::new([u64; REG_IMAGE_LEN]))`,
/// constructed by [`clone_thread`].  The trampoline reclaims and
/// frees the box, installs the new FS base if requested, gives a traced
/// thread its first stop, and enters ring 3 with `RAX = 0`
/// (`crate::proc::user_entry`).
///
/// # Safety
///
/// `image_raw` must point to a valid `Box<[u64; REG_IMAGE_LEN]>`
/// created by [`clone_thread`].  The IRETQ frame slots must describe
/// a valid ring-3 context (RIP/RSP in user space, CS/SS being the
/// ring-3 selectors, RFLAGS with the reserved bits well-formed).
extern "C" fn clone_thread_trampoline(image_raw: u64) {
    // Reclaim the heap-allocated register image and copy it onto
    // this thread's kernel stack so we can free the heap allocation
    // before the (never-returning) IRETQ.
    //
    // SAFETY: `image_raw` was produced by `Box::into_raw` in
    // `clone_thread` for this thread alone.  No other code observes
    // it.
    let boxed = unsafe { Box::from_raw(image_raw as *mut [u64; REG_IMAGE_LEN]) };
    let regs: [u64; REG_IMAGE_LEN] = *boxed;
    drop(boxed);

    // Install new FS base (CLONE_SETTLS).  A value of 0 means
    // "caller didn't ask" — leave the inherited MSR alone.
    let new_fs = regs[17];
    if new_fs != 0 {
        // SAFETY: IA32_FS_BASE is a documented architectural MSR;
        // we are in ring 0 with interrupts disabled by the syscall
        // entry path.  Setting FS_BASE to a value the caller has
        // arranged is exactly what `arch_prctl(ARCH_SET_FS, ...)`
        // does and is the standard pthread TLS setup.
        unsafe { crate::cpu::wrmsr(IA32_FS_BASE, new_fs) };
    }

    // The thread's whole register set: its creator's at the call, with the
    // call's 0 in RAX and RCX/R11 as a SYSRET from it leaves them -- never the
    // kernel's own values, which this trampoline used to hand ring 3 in them
    // (`crate::proc::user_entry`).
    let [
        rip,
        _cs,
        rflags,
        rsp,
        _ss,
        rdi,
        rsi,
        rdx,
        r10,
        r8,
        r9,
        rbx,
        rbp,
        r12,
        r13,
        r14,
        r15,
        _fs,
    ] = regs;
    let mut entry = crate::proc::user_entry::UserEntry {
        rip,
        rsp,
        rflags,
        rax: 0,
        rbx,
        rcx: rip,
        rdx,
        rsi,
        rdi,
        rbp,
        r8,
        r9,
        r10,
        r11: rflags,
        r12,
        r13,
        r14,
        r15,
    };

    // A freeze that began while the thread was being made: its first entry to
    // ring 3 passes no signal checkpoint, so it is looked for here, before
    // its first instruction (`proc::freezer`).
    crate::proc::freezer::park_if_frozen();

    // A traced thread's first stop, before its first instruction
    // (`ptrace::first_entry`): its tracer may change the registers.
    crate::proc::ptrace::first_entry(&mut entry);

    // SAFETY: the cloned thread shares its creator's address space, which the
    // scheduler made the active one before dispatching it; RIP is the
    // creator's own return address and RSP the caller-supplied stack (or a
    // tracer's, which `ptrace`'s register checks keep to user addresses), and
    // RFLAGS the creator's own, made safe when the image was built.
    unsafe { crate::proc::user_entry::enter(&entry) }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// `CLONE_PARENT_SETTID` flag bit — kept here as a `const` so this
/// module doesn't need to depend on the Linux ABI's `clone_flags`
/// module ordering at compile time.
const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
/// `CLONE_CHILD_CLEARTID` flag bit.
const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
/// `CLONE_CHILD_SETTID` flag bit.
const CLONE_CHILD_SETTID: u64 = 0x0100_0000;

/// Spawn a new ring-3 thread in the address space of `parent_pid`.
///
/// The new thread resumes execution at the instruction immediately
/// after the parent's `SYSCALL` (i.e. at `frame.user_rip`) with
/// `RAX = 0` and `RSP = args.child_stack`, optionally with a new
/// `FS_BASE` (TLS) and TID notifications recorded.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `args.child_stack` is 0 (a
///   thread-creation clone is meaningless without its own stack).
/// - Propagates allocation / spawn failures from [`thread::spawn`].
pub fn clone_thread(
    parent_pid: ProcessId,
    frame: &SyscallFrame,
    args: &CloneThreadArgs,
) -> KernelResult<TaskId> {
    if args.child_stack == 0 {
        return Err(KernelError::InvalidArgument);
    }

    let regs = build_register_image(frame, args);
    let image_raw = Box::into_raw(Box::new(regs)) as u64;

    // The calling thread's scheduling, as Linux's `sched_fork` gives a new
    // thread: policy, real-time priority and ordinary level, reset to the
    // ordinary default under SCHED_RESET_ON_FORK (`sched::inheritance_from`).
    // The thread is spawned at that level and given the rest while still
    // suspended. Until 2026-10-07 it was spawned at the caller's *effective*
    // level, an interactive boost or a lent level included.
    let inheritance = crate::sched::inheritance_from(crate::sched::current_task_id());
    let priority = inheritance.map_or(crate::sched::task::DEFAULT_PRIORITY, |i| i.level());

    // Compute the child's persistent FS (TLS) base *before* spawning so it can
    // be seeded onto the Task while it is still suspended.  IA32_FS_BASE is a
    // global CPU register not saved in the GP Context; the trampoline's
    // one-shot WRMSR only sets it for the first run, and the switch-in path
    // restores `Task::fs_base` unconditionally — so if the child were admitted
    // with fs_base still 0, being preempted before we seeded it would clobber
    // its TLS base and send glibc's `start_thread` to a garbage `start_routine`
    // (B-PTHREAD-CHILD-JUMPS-TO-GARBAGE defect 1).
    // CLONE_SETTLS (new_tls != 0) gives the thread its own TLS block;
    // otherwise it inherits the calling thread's current FS base.
    let child_fs = if args.new_tls != 0 {
        args.new_tls
    } else {
        // SAFETY: reading IA32_FS_BASE is side-effect-free.
        unsafe { crate::cpu::rdmsr(crate::cpu::IA32_FS_BASE) }
    };
    // clone() has no "set GS" flag, so the new thread inherits the calling
    // thread's userspace %gs base (the authoritative Task field, 0 if unset).
    let child_gs = crate::sched::current_task_gs_base();

    // Phase 1 of the two-phase spawn: the child is registered with the process
    // but is NOT yet runnable, so it cannot run — or exit — while we install
    // the ctid/settid state below.  Registering any of that *after* admission
    // races the child to completion; see `thread::spawn_suspended_with_tls`
    // (B-PTHREAD-JOIN-LOST-CTID).
    let task_id = match thread::spawn_suspended_with_tls(
        parent_pid,
        b"cloned-thread",
        priority,
        clone_thread_trampoline,
        image_raw,
        child_fs,
        child_gs,
    ) {
        Ok(id) => id,
        Err(e) => {
            // The trampoline never ran, so it never freed the image —
            // reclaim and drop it here to avoid a leak.
            //
            // SAFETY: `image_raw` came from `Box::into_raw` just above
            // and was not consumed (spawn failed before the task ran).
            drop(unsafe { Box::from_raw(image_raw as *mut [u64; REG_IMAGE_LEN]) });
            return Err(e);
        }
    };

    if let Some(inheritance) = inheritance {
        crate::sched::inherit_scheduling(task_id, inheritance);
    }

    // NOTE: the child's FS/GS bases were seeded by `spawn_with_tls` *before*
    // the task was admitted — deliberately not done here, where the child may
    // already be running (see the computation above and thread.rs).

    // The child is still suspended here, so all three registrations below are
    // race-free.  For CLONE_PARENT_SETTID Linux promises that the parent's TID
    // store is observable BEFORE the parent's own return from clone() — we are
    // still in the parent's syscall frame, so doing the copy now (before we
    // return our value to userspace) satisfies that ordering.

    let tid_for_user: i32 = i32::try_from(task_id).unwrap_or(i32::MAX);

    // CLONE_PARENT_SETTID: store the new TID at *ptid in the
    // parent's view of the address space (= the shared AS, since
    // CLONE_VM).
    if (args.flags & CLONE_PARENT_SETTID) != 0 && args.parent_tid_ptr != 0 {
        // SAFETY: copy_to_user validates the user range under SMAP.
        // We ignore the result: if the user gave us a bad pointer
        // Linux returns -EFAULT but the new thread is already
        // running, which is an unrecoverable corner case.  Track
        // the leak in todo.txt rather than racing the child to undo.
        let _ = unsafe {
            crate::mm::user::copy_to_user(
                (&raw const tid_for_user).cast::<u8>(),
                args.parent_tid_ptr,
                core::mem::size_of::<i32>(),
            )
        };
    }

    // CLONE_CHILD_SETTID: store the new TID at *ctid in the shared
    // AS.  The child would do this itself in normal Linux, but
    // doing it from the parent is equivalent and simpler (no
    // trampoline argument plumbing).  Doing it while the child is still
    // suspended is also what makes it safe: were the child allowed to run and
    // exit first, its exit hook would zero *ctid and this store would then
    // resurrect a non-zero tid into a dead thread's descriptor, hanging any
    // `pthread_join` on it just as surely as a missed wake.
    if (args.flags & CLONE_CHILD_SETTID) != 0 && args.child_tid_ptr != 0 {
        let _ = unsafe {
            crate::mm::user::copy_to_user(
                (&raw const tid_for_user).cast::<u8>(),
                args.child_tid_ptr,
                core::mem::size_of::<i32>(),
            )
        };
    }

    // CLONE_CHILD_CLEARTID: register the ctid for futex-on-exit.  This MUST be
    // in place before the child is admitted: `on_thread_exit_hook` looks the
    // entry up and returns early when it is absent, skipping both the *ctid
    // zero-write and the futex wake that `pthread_join` is parked on.
    if (args.flags & CLONE_CHILD_CLEARTID) != 0 && args.child_tid_ptr != 0 {
        register_clear_child_tid(task_id, args.child_tid_ptr);
    }

    // A traced creator's thread is traced too, when its tracer asked
    // (`PTRACE_O_TRACECLONE`, or CLONE_PTRACE): before it can run, so its
    // first instruction is its first stop.
    crate::proc::ptrace::attach_new(
        crate::sched::current_task_id(),
        parent_pid,
        task_id,
        crate::proc::ptrace::Creation::from_clone(args.flags, args.flags & 0xff),
        frame.syscall_nr,
    );

    // Phase 2: everything the exit path needs is registered — let the child run.
    // On failure `admit` has already unwound the thread registration and
    // destroyed the task, but the ctid entry we just installed is keyed on a
    // task id that will never exit, so drop it here rather than leaking it.
    if let Err(e) = thread::admit(parent_pid, task_id) {
        crate::proc::ptrace::forget_new(task_id);
        forget_clear_child_tid(task_id);
        // SAFETY: `image_raw` came from `Box::into_raw` above and was not
        // consumed — the task never ran, so the trampoline never freed it.
        drop(unsafe { Box::from_raw(image_raw as *mut [u64; REG_IMAGE_LEN]) });
        return Err(e);
    }

    Ok(task_id)
}

/// Build the register image consumed by [`clone_thread_trampoline`].
fn build_register_image(frame: &SyscallFrame, args: &CloneThreadArgs) -> [u64; REG_IMAGE_LEN] {
    // RFLAGS must have the reserved bit 1 set and IF (interrupts
    // enabled).  If the parent's saved RFLAGS is somehow zero (a
    // synthetic frame in a test, for example) substitute the
    // canonical user-mode value.
    let rflags = if frame.user_rflags == 0 {
        0x202
    } else {
        frame.user_rflags
    };

    [
        frame.user_rip,                 // 0: RIP
        u64::from(crate::gdt::USER_CS), // 1: CS
        rflags,                         // 2: RFLAGS
        args.child_stack,               // 3: RSP (caller-supplied)
        u64::from(crate::gdt::USER_DS), // 4: SS
        frame.arg0,                     // 5: RDI
        frame.arg1,                     // 6: RSI
        frame.arg2,                     // 7: RDX
        frame.arg3,                     // 8: R10
        frame.arg4,                     // 9: R8
        frame.arg5,                     // 10: R9
        frame.rbx,                      // 11: RBX
        frame.rbp,                      // 12: RBP
        frame.r12,                      // 13: R12
        frame.r13,                      // 14: R13
        frame.r14,                      // 15: R14
        frame.r15,                      // 16: R15
        args.new_tls,                   // 17: FS_BASE (0 = leave MSR alone)
    ]
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the thread-clone self-test.
///
/// We can exercise:
///   - `build_register_image` field layout (deterministic given
///     stable selectors and the input frame);
///   - `register_clear_child_tid` / `on_thread_exit_hook` for the
///     bookkeeping state (no actual ring-3 transition);
///   - `clone_thread` argument-validation rejecting `child_stack=0`.
///
/// We can NOT exercise the IRETQ trampoline from a self-test because
/// that would require a live userspace context — the integration
/// path is covered by booting a real Linux binary that calls
/// `pthread_create`.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[thread_clone] Running self-test...");

    // (1) build_register_image: verify slot mapping.
    let frame = SyscallFrame {
        exit_full: 0,
        rcx: 0,
        r11: 0,
        syscall_nr: 0,
        arg0: 0x1111,
        arg1: 0x2222,
        arg2: 0x3333,
        arg3: 0x4444,
        arg4: 0x5555,
        arg5: 0x6666,
        rbx: 0xBBBB,
        rbp: 0xCCCC,
        r12: 0xDDDD,
        r13: 0xEEEE,
        r14: 0xFFFF,
        r15: 0xAAAA,
        user_rip: 0x0000_7FFF_FFFF_0010,
        user_rsp: 0x0000_7FFF_FFFE_0000,
        user_rflags: 0x246,
    };
    let args = CloneThreadArgs {
        flags: 0,
        child_stack: 0x0000_7FFF_FFFD_F000,
        parent_tid_ptr: 0,
        child_tid_ptr: 0,
        new_tls: 0xABCD_1234_5678_DEAD,
    };
    let regs = build_register_image(&frame, &args);
    let checks: &[(usize, u64, &str)] = &[
        (0, frame.user_rip, "RIP"),
        (2, frame.user_rflags, "RFLAGS"),
        (3, args.child_stack, "RSP"),
        (5, frame.arg0, "RDI"),
        (10, frame.arg5, "R9"),
        (16, frame.r15, "R15"),
        (17, args.new_tls, "FS_BASE"),
    ];
    for &(idx, expected, name) in checks {
        if regs[idx] != expected {
            crate::serial_println!(
                "[thread_clone]   FAIL: regs[{}] ({}) = {:#x}, expected {:#x}",
                idx,
                name,
                regs[idx],
                expected,
            );
            return Err(KernelError::InternalError);
        }
    }
    // RFLAGS substitution: zero -> 0x202.
    let frame_zero_rflags = SyscallFrame {
        exit_full: 0,
        rcx: 0,
        r11: 0,
        user_rflags: 0,
        ..frame
    };
    let regs2 = build_register_image(&frame_zero_rflags, &args);
    if regs2[2] != 0x202 {
        crate::serial_println!(
            "[thread_clone]   FAIL: zero RFLAGS should be substituted with 0x202, got {:#x}",
            regs2[2],
        );
        return Err(KernelError::InternalError);
    }

    // (2) CLEAR_CHILD_TID bookkeeping.
    //
    // Use a TID well above any real task ID the boot self-test has
    // assigned to avoid colliding with a live task.
    let test_tid: TaskId = 0xFFFF_FF00_0000_0001;
    let test_ctid: u64 = 0xDEAD_BEEF_CAFE_0000;
    register_clear_child_tid(test_tid, test_ctid);
    {
        let map = CLEAR_CHILD_TID.lock();
        if map.get(&test_tid) != Some(&test_ctid) {
            crate::serial_println!("[thread_clone]   FAIL: ctid registration did not stick");
            return Err(KernelError::InternalError);
        }
    }
    // register(_, 0) should remove the entry (Linux set_tid_address
    // semantics for clear_child_tid with a NULL pointer).
    register_clear_child_tid(test_tid, 0);
    {
        let map = CLEAR_CHILD_TID.lock();
        if map.contains_key(&test_tid) {
            crate::serial_println!("[thread_clone]   FAIL: ctid registration to 0 did not clear");
            return Err(KernelError::InternalError);
        }
    }

    // (3) on_thread_exit_hook is a no-op when the task has no
    // registration — must not panic, must not crash.
    on_thread_exit_hook(test_tid);

    // (4) clone_thread rejects child_stack == 0.
    let bad_args = CloneThreadArgs {
        flags: 0,
        child_stack: 0,
        parent_tid_ptr: 0,
        child_tid_ptr: 0,
        new_tls: 0,
    };
    // parent_pid = 0 is intentionally a non-existent pid, but the
    // child_stack check comes first.
    match clone_thread(0, &frame, &bad_args) {
        Err(KernelError::InvalidArgument) => {}
        other => {
            crate::serial_println!(
                "[thread_clone]   FAIL: clone_thread(child_stack=0) -> {:?}",
                other.map(|_| "Ok"),
            );
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[thread_clone] Self-test PASSED");
    Ok(())
}
