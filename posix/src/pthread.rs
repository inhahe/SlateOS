//! POSIX threads — working implementation backed by kernel threads.
//!
//! ## Thread Creation
//!
//! `pthread_create` maps the new thread's memory -- from the bottom, an
//! inaccessible guard, the stack its attribute asked for, then the TLS
//! block, TCB and per-thread block; or, on a stack the caller supplied, the
//! TLS part alone.  It claims and fills the thread's slot in the thread
//! table, pushes the start routine and argument, then calls
//! `SYS_THREAD_CREATE` with an assembly trampoline as the entry point.  The
//! trampoline pops the arguments, calls the start routine, and exits
//! through `pthread_exit`.
//!
//! ## Thread Lifecycle
//!
//! - **Joinable** (default): Another thread calls `pthread_join` which
//!   blocks on `SYS_THREAD_JOIN`, then frees the mapping.
//! - **Detached** (created so, or marked by `pthread_detach`): when it exits
//!   it frees its *own* mapping (glibc `__unmapself` style) via the
//!   `__pthread_exit_unmap` primitive, so detached threads are reclaimed
//!   without a joiner.  The per-slot atomic `state` arbitrates the
//!   detach-vs-exit race so exactly one party frees the mapping.
//!
//! ## Synchronization Primitives
//!
//! Every blocking call sleeps on a futex ([`crate::lowlevellock`]) and is
//! woken by the thread that releases it, and every uncontended path is one
//! atomic operation with no syscall.  Until 2026-09-26 they slept in 1 ms
//! steps and polled (`known-issues.md` →
//! `B-D-PTHREAD-SYNC-POLLED-IN-1MS-STEPS`).
//!
//! - **Mutexes**: glibc's three-state low-level lock.  Normal, recursive
//!   and error-checking types; the owner is the calling thread's task id,
//!   cached in its per-thread block so locking makes no syscall.
//! - **Condition variables**: a sequence counter the waiters sleep on, with
//!   a count of waiters so that a signal nobody is waiting for costs no
//!   syscall.  The clock the attribute named is kept and used
//!   (`design-decisions.md` §1110).
//! - **Read-write locks**: a reader-preferring futex lock (0 = unlocked,
//!   N = readers, -1 = writer); the writer's own `rdlock`/`wrlock` is
//!   `EDEADLK`.
//! - **Barriers**: the arrival count and round number under a low-level
//!   lock; waiters sleep on the round.
//! - **Spinlocks**: pure atomic CAS busy-wait.
//!
//! ## Why each opaque type carries a `const` size assertion
//!
//! `pthread_mutex_t` and friends are *opaque but complete*: C code never
//! reads their fields, but it does put them on the stack and pass their
//! address in, so the header's size is a hard contract.  If our definition
//! is **larger** than the header's, `pthread_mutex_init` writes past the
//! end of the caller's object and corrupts its frame — and nothing on the
//! Rust side can notice, because every Rust caller shares our definition
//! and so agrees with itself.  The two halves are compiled from different
//! headers in different languages; no single diff contains both.
//!
//! That is not hypothetical.  `posix_spawn_file_actions_t` was 4624 bytes
//! against an 80-byte header, and it took a ring-3 crash in GNU make to
//! find it (see `spawn.rs` and known-issues.md).  Its sibling
//! `posix_spawnattr_t` had a size test and was correct.
//!
//! So the bound below is asserted at **compile** time, not in a `#[test]`
//! that has to be run to help.
//!
//! ### `<=` here, `==` in semaphore.rs / regex.rs / glob.rs
//!
//! `<=` is the exact *safety* property: oversized always smashes the
//! caller's frame, undersized never can — we simply use less of the slot
//! than the caller reserved.  The four pthread types below are genuinely
//! smaller than their headers (a futex word or two against musl's 40–56)
//! and no `_reserved` tail is added, because unlike `sem_t` these are never
//! copied: POSIX leaves it undefined to move an initialised
//! `pthread_mutex_t`/`_cond_t`/`_rwlock_t`/`_barrier_t` at all, so a
//! by-value copy is already wrong whatever the size.  `<=` states what
//! matters for them and keeps firing if a field is ever added.
//!
//! Where an explicit `_reserved` tail *is* carried — `SemT`, `RegexT` and
//! `PosixSpawnFileActionsT` — or every field of the header's is there by
//! name, as in `GlobT` since 2026-09-29, the assertion is tightened to
//! `==`, which says strictly more: it catches a field *removal* as well as
//! an addition, and a removal is what would silently shorten a by-value
//! copy of one of those.  (See `TD-B-THREE-C-VISIBLE-TYPES-ARE-SMALLER-
//! THAN-THEIR-HEADERS` in known-issues.md, now closed.)
//!
//! ## Features
//!
//! - Thread: `pthread_create`, `pthread_join`, `pthread_detach`,
//!   `pthread_self`, `pthread_equal`, `pthread_exit`
//! - Attributes: `pthread_attr_init`/`destroy`/`setstacksize`/
//!   `getstacksize`/`setdetachstate`/`getdetachstate`
//! - Mutex: `pthread_mutex_init`/`destroy`/`lock`/`trylock`/`timedlock`/
//!   `clocklock`/`unlock`/`consistent`/`getprioceiling`/`setprioceiling`
//! - Mutex attributes: `pthread_mutexattr_init`/`destroy`/`settype`/
//!   `gettype` and the `pshared`, `protocol`, `prioceiling` and `robust`
//!   getter/setter pairs
//! - Condition: `pthread_cond_init`/`destroy`/`wait`/`timedwait`/
//!   `clockwait`/`signal`/`broadcast`; `pthread_condattr_*` including
//!   `setclock` and `setpshared`
//! - RW lock: `pthread_rwlock_init`/`destroy`/`rdlock`/`tryrdlock`/
//!   `timedrdlock`/`clockrdlock`/`wrlock`/`trywrlock`/`timedwrlock`/
//!   `clockwrlock`/`unlock`
//! - Barrier: `pthread_barrier_init`/`destroy`/`wait`, and
//!   `pthread_barrierattr_*`
//! - Spinlock: `pthread_spin_init`/`destroy`/`lock`/`trylock`/`unlock`
//! - Cancel stubs: `pthread_setcancelstate`/`setcanceltype`/
//!   `testcancel`/`cancel`
//! - Once: `pthread_once`
//! - TSD: `pthread_key_create`/`delete`/`getspecific`/`setspecific`
//! - Yield: `sched_yield`
//!
//! ## Limitations
//!
//! - Thread-specific data (TSD) is **per-thread**: keyed on the kernel
//!   task ID (`SYS_TASK_ID`), each thread gets its own value array, and
//!   key destructors run at thread exit (`pthread_exit` / return from the
//!   start routine) for up to `PTHREAD_DESTRUCTOR_ITERATIONS` rounds.
//!   This is a table-keyed-by-tid implementation rather than FS/GS-based
//!   TLS, which is correct (proper per-thread semantics) but does an
//!   O(active-threads) slot lookup per access.
//! - Detached thread stacks are reclaimed by self-unmap on exit, and the
//!   kernel drops the per-thread exit-value entry eagerly (the self-unmap
//!   path passes the `SYS_THREAD_EXIT` detached flag = 1), so a detached
//!   thread leaks neither its stack nor a kernel map entry.
//! - `pthread_cancel` returns `ENOSYS`. It does not pretend to cancel: a
//!   caller is told the operation is unavailable rather than left believing
//!   a thread is stopping. (This line read "accepted but never actually
//!   cancels a thread" until 2026-09-13, which described the function before
//!   it started refusing.)
//! - Process-shared objects (`PTHREAD_PROCESS_SHARED`) are refused with
//!   `ENOTSUP`, glibc's answer where shared futexes are unsupported: the
//!   kernel keys a futex by address space, so a waiter in one process could
//!   never be woken from another (`known-issues.md` →
//!   `B-D-PROCESS-SHARED-SYNC-IS-SILENTLY-PRIVATE`).
//! - Priority-inheritance, priority-protect and robust mutexes are not
//!   supported: their attributes can be set and read back, but
//!   `pthread_mutex_init` refuses them with `ENOTSUP`.

use crate::errno;
use crate::sched::CpuSetT;
// The stack floor lives with the other pthread limits; `pthread_attr_setstack`
// and `pthread_attr_setstacksize` are the only users, and they must agree with
// it rather than carry a second, hardcoded value of their own.
use crate::linux_pthread_key_types::PTHREAD_STACK_MIN;
use crate::syscall;
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU8, AtomicU64, AtomicUsize, Ordering,
};

/// Opaque pthread_t type — holds the kernel task ID.
pub type PthreadT = u64;

/// `KernelError::Cancelled` as it appears in a syscall return register.
///
/// `SYS_THREAD_JOIN` reports it when the target thread was *killed* —
/// an unhandled ring-3 fault, an explicit kill, or process teardown —
/// rather than exiting on its own.  Kept in sync with
/// `kernel/src/error.rs`.
const KERNEL_ERR_CANCELLED: i64 = -5;

/// `PTHREAD_CANCELED` — `(void *)-1`, the value POSIX reserves for the
/// return of a thread that did not finish normally.
///
/// Mirrors [`crate::linux_pthread_key_types::PTHREAD_CANCELED`] as the
/// signed value stored in the kernel's `i64` exit-value slot.
const PTHREAD_CANCELED_VALUE: i64 = -1;

/// Opaque pthread_attr_t type (glibc x86_64: 56 bytes).
pub type PthreadAttrT = [u8; 56];

/// Pthread mutex type — thread-safe via atomic operations.
///
/// Binary-compatible with C: `AtomicI32` has the same size and
/// alignment as `i32`.
///
/// Supports normal, recursive, and error-checking mutex types:
/// - **Normal** (default): deadlock on double-lock, UB on unlock by
///   non-owner (matches POSIX default).
/// - **Recursive**: same thread can lock multiple times; each lock
///   increments a recursion count that must be matched by unlocks.
/// - **Error-checking**: returns EDEADLK on double-lock, EPERM on
///   unlock by non-owner.
#[repr(C)]
pub struct PthreadMutexT {
    /// 0 = unlocked, 1 = locked.
    locked: AtomicI32,
    /// Mutex type (PTHREAD_MUTEX_NORMAL / RECURSIVE / ERRORCHECK).
    kind: AtomicI32,
    /// Task ID of the owning thread (valid when locked).
    owner: AtomicI32,
    /// Recursion count (for PTHREAD_MUTEX_RECURSIVE; 0 when unlocked).
    count: AtomicI32,
    // Padding to match typical libc struct size (40 - 16 = 24 bytes).
    _pad: [u8; 24],
}

/// See the module note on why these are `const` and not `#[test]`.
const _: () = {
    assert!(
        size_of::<PthreadMutexT>() <= 40,
        "musl/glibc pthread_mutex_t"
    );
    assert!(align_of::<PthreadMutexT>() <= 8);
};

/// Pthread mutex attribute type (glibc x86_64: 4 bytes).
pub type PthreadMutexattrT = [u8; 4];

/// Pthread condition variable type — basic implementation.
///
/// Uses a generation counter so `pthread_cond_signal` can wake
/// threads spinning on `pthread_cond_wait`.
#[repr(C)]
pub struct PthreadCondT {
    /// Sequence number, and the futex waiters sleep on: every signal and
    /// broadcast advances it, so a waiter that saw the old value wakes.
    generation: AtomicI32,
    /// The clock `pthread_cond_timedwait` measures its deadline against,
    /// from the attribute's `pthread_condattr_setclock`: `CLOCK_REALTIME`
    /// (0, also `PTHREAD_COND_INITIALIZER`'s) or `CLOCK_MONOTONIC`.  Until
    /// 2026-09-26 the attribute was ignored and every deadline was read as
    /// real time -- so a monotonic deadline, a few seconds past boot, had
    /// always passed.
    clock: i32,
    /// Threads inside a wait, so a signal with no one waiting costs no
    /// syscall.
    waiters: AtomicI32,
    // Padding to match typical libc struct size.
    _pad: [u8; 36],
}

/// See the module note on why these are `const` and not `#[test]`.
const _: () = {
    assert!(size_of::<PthreadCondT>() <= 48, "musl/glibc pthread_cond_t");
    assert!(align_of::<PthreadCondT>() <= 8);
};

/// Pthread condition variable attribute type (glibc x86_64: 4 bytes).
pub type PthreadCondattrT = [u8; 4];

/// Static initializer for `pthread_cond_t`.
#[allow(clippy::declare_interior_mutable_const)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static PTHREAD_COND_INITIALIZER: PthreadCondT = PthreadCondT {
    generation: AtomicI32::new(0),
    clock: 0,
    waiters: AtomicI32::new(0),
    _pad: [0; 36],
};

/// Pthread once control type — thread-safe via atomic flag.
#[repr(C)]
pub struct PthreadOnceT {
    /// 0 = not started, -1 = in progress, 1 = done.
    done: AtomicI32,
}

/// Static initializer for `pthread_once_t`.
///
/// Interior mutability is expected here — C code uses this as a
/// compile-time initializer: `pthread_once_t once = PTHREAD_ONCE_INIT;`
#[allow(clippy::declare_interior_mutable_const)]
pub const PTHREAD_ONCE_INIT: PthreadOnceT = PthreadOnceT {
    done: AtomicI32::new(0),
};

/// Static initializer for `pthread_mutex_t` (unlocked).
///
/// Interior mutability is expected — C code uses this as:
/// `pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;`
#[allow(clippy::declare_interior_mutable_const)]
pub const PTHREAD_MUTEX_INITIALIZER: PthreadMutexT = PthreadMutexT {
    locked: AtomicI32::new(0),
    kind: AtomicI32::new(0), // PTHREAD_MUTEX_NORMAL
    owner: AtomicI32::new(0),
    count: AtomicI32::new(0),
    _pad: [0; 24],
};

// ---------------------------------------------------------------------------
// Thread table — tracks created threads for stack cleanup
// ---------------------------------------------------------------------------
//
// The table is lock-free.  Each slot's `task_id` doubles as an occupancy
// flag (`SLOT_EMPTY` / `SLOT_RESERVED` / real id) and each slot carries an
// atomic `state` that arbitrates — race-free — which single party frees the
// thread's mapping.  This replaces the former `static mut` +
// "single-creator convention" (which was a data race for the concurrent
// detach-vs-exit window) with real atomics.
//
// A slot is claimed and filled *before* its thread exists, and the thread is
// handed the slot's address in its per-thread block, so it never has to find
// itself by task id -- which it could not do before its creator had
// published that id.  Until 2026-09-26 the slot was filled only after
// `SYS_THREAD_CREATE` returned, and a thread that exited first found no slot
// at all (`known-issues.md` → `D-PTHREAD-SLOT-PUBLISH-RACE`).
//
// The table grows a chunk at a time and never shrinks, so a slot's address
// is stable for the life of the process.  It was a fixed 64 slots, and a
// 65th thread ran untracked: its mapping leaked, and `pthread_detach` told
// the caller it did not exist.

/// Slots per chunk of the thread table.
const CHUNK_SLOTS: usize = 64;

/// Usable stack for a new thread whose attribute names no size (64 KiB =
/// 4 pages).
const DEFAULT_THREAD_STACK_SIZE: usize = 64 * 1024;

/// `task_id` sentinel for an unused slot.
const SLOT_EMPTY: u64 = 0;
/// `task_id` sentinel for a slot claimed but not yet published.
const SLOT_RESERVED: u64 = u64::MAX;

/// Thread is joinable and still tracked (initial state).
const STATE_JOINABLE: u8 = 0;
/// Thread was detached; it will free its *own* mapping when it exits.
const STATE_DETACHED: u8 = 1;
/// Thread exited while joinable and left its mapping for a joiner (or for a
/// `pthread_detach` that lost the race) to reclaim.
const STATE_EXITED: u8 = 2;

/// Per-thread metadata slot.  All fields are atomic, so the table needs no
/// external lock -- and all-zero is an empty slot, which is what lets fresh
/// anonymous memory serve as a new chunk.
///
/// Ownership protocol (all `state` transitions are `compare_exchange`):
///
/// ```text
///   JOINABLE --pthread_detach--> DETACHED   (thread self-unmaps on exit)
///   JOINABLE --pthread_exit----> EXITED      (joiner/late-detach frees it)
/// ```
///
/// A thread created detached starts in `DETACHED`.
///
/// Exactly one party ever unmaps a given mapping:
/// - `DETACHED` → the exiting thread frees its own mapping (self-unmap).
/// - `EXITED`   → whichever of `pthread_join` / a late `pthread_detach`
///   observes it frees the mapping, but only *after* `SYS_THREAD_JOIN`
///   confirms the thread is off it (so there is no use-after-free).
struct ThreadSlot {
    /// Kernel task id, or `SLOT_EMPTY` / `SLOT_RESERVED`.
    task_id: AtomicU64,
    /// Base of the mapping this library made for the thread: guard, stack,
    /// TLS block, TCB and per-thread block -- or, for a thread on a stack the
    /// caller supplied (`pthread_attr_setstack`), the TLS part alone.  This
    /// is what gets unmapped.
    map_base: AtomicUsize,
    /// Size of that mapping in bytes.
    map_size: AtomicUsize,
    /// Lowest address of the thread's usable stack, as `pthread_getattr_np`
    /// reports it.
    stack_base: AtomicUsize,
    /// Size of the usable stack in bytes.  Smaller than the mapping: the
    /// guard is below it and the TLS block and TCB above.
    stack_size: AtomicUsize,
    /// Size of the inaccessible guard below the stack (0 for none).
    guard_size: AtomicUsize,
    /// Lifecycle state (`STATE_*`).
    state: AtomicU8,
}

impl ThreadSlot {
    const fn new() -> Self {
        Self {
            task_id: AtomicU64::new(SLOT_EMPTY),
            map_base: AtomicUsize::new(0),
            map_size: AtomicUsize::new(0),
            stack_base: AtomicUsize::new(0),
            stack_size: AtomicUsize::new(0),
            guard_size: AtomicUsize::new(0),
            state: AtomicU8::new(STATE_JOINABLE),
        }
    }
}

/// One chunk of the thread table.
#[repr(C)]
struct ThreadChunk {
    slots: [ThreadSlot; CHUNK_SLOTS],
    /// The next chunk, or null.  Set once, by [`link_chunk`].
    next: AtomicPtr<ThreadChunk>,
}

impl ThreadChunk {
    const fn new() -> Self {
        Self {
            slots: [const { ThreadSlot::new() }; CHUNK_SLOTS],
            next: AtomicPtr::new(core::ptr::null_mut()),
        }
    }
}

/// The table's first chunk; later ones are mapped as threads outnumber the
/// slots.
static THREAD_TABLE: ThreadChunk = ThreadChunk::new();

/// A new thread's record, written into its reserved slot before the thread
/// can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThreadRecord {
    map_base: usize,
    map_size: usize,
    stack_base: usize,
    stack_size: usize,
    guard_size: usize,
    detached: bool,
}

/// The chunks from `first` on.
fn chunks_from(first: &'static ThreadChunk) -> impl Iterator<Item = &'static ThreadChunk> {
    core::iter::successors(Some(first), |chunk| {
        // SAFETY: a non-null `next` was linked by `link_chunk` (Release,
        // paired with this Acquire) after its chunk was fully in place, and
        // chunks are never freed.
        unsafe { chunk.next.load(Ordering::Acquire).as_ref() }
    })
}

/// Link `fresh` after `last`: `Ok(fresh)`, or `Err` with the chunk another
/// thread linked there first (and `fresh` is the caller's to free).
fn link_chunk(
    last: &'static ThreadChunk,
    fresh: &'static ThreadChunk,
) -> Result<&'static ThreadChunk, &'static ThreadChunk> {
    match last.next.compare_exchange(
        core::ptr::null_mut(),
        core::ptr::from_ref(fresh).cast_mut(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Ok(fresh),
        // SAFETY: as in `chunks_from`.
        Err(theirs) => Err(unsafe { &*theirs }),
    }
}

/// Map a new chunk and link it after `last`, or return the chunk another
/// thread linked first.  `None` if the memory cannot be had.
fn grow_table(last: &'static ThreadChunk) -> Option<&'static ThreadChunk> {
    let size = size_of::<ThreadChunk>();
    let mem = crate::mman::mmap(
        core::ptr::null_mut(),
        size,
        crate::mman::PROT_READ | crate::mman::PROT_WRITE,
        crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
        -1,
        0,
    );
    if mem == crate::mman::MAP_FAILED {
        return None;
    }
    // SAFETY: the mapping is page-aligned, large enough, never freed once
    // linked, and zero-filled -- and zero is an empty chunk (every slot
    // `SLOT_EMPTY` and `STATE_JOINABLE`, `next` null).
    let fresh = unsafe { &*mem.cast::<ThreadChunk>() };
    match link_chunk(last, fresh) {
        Ok(chunk) => Some(chunk),
        Err(theirs) => {
            // Never shared, so nothing can be using it; a failed unmap would
            // only leave one page mapped.
            let _ = crate::mman::munmap(mem, size);
            Some(theirs)
        }
    }
}

/// Claim a free slot (`SLOT_EMPTY` → `SLOT_RESERVED`) in the table starting
/// at `first`, calling `grow` for a new chunk when every slot is taken.
/// `None` only when the table cannot grow.
///
/// A reserved slot is its claimer's alone -- [`find_slot`] never returns
/// one -- so the claimer may fill it before publishing a task id.
fn claim_slot_in(
    first: &'static ThreadChunk,
    mut grow: impl FnMut(&'static ThreadChunk) -> Option<&'static ThreadChunk>,
) -> Option<&'static ThreadSlot> {
    let mut chunk = first;
    loop {
        for slot in &chunk.slots {
            if slot
                .task_id
                .compare_exchange(
                    SLOT_EMPTY,
                    SLOT_RESERVED,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return Some(slot);
            }
        }
        // SAFETY: as in `chunks_from`.
        chunk = match unsafe { chunk.next.load(Ordering::Acquire).as_ref() } {
            Some(next) => next,
            None => grow(chunk)?,
        };
    }
}

/// [`claim_slot_in`] the process's table.
fn claim_slot() -> Option<&'static ThreadSlot> {
    claim_slot_in(&THREAD_TABLE, grow_table)
}

/// Record a new thread in its reserved slot.  Nobody else reads the slot
/// until its id is published.
fn fill_slot(slot: &ThreadSlot, record: &ThreadRecord) {
    slot.map_base.store(record.map_base, Ordering::Relaxed);
    slot.map_size.store(record.map_size, Ordering::Relaxed);
    slot.stack_base.store(record.stack_base, Ordering::Relaxed);
    slot.stack_size.store(record.stack_size, Ordering::Relaxed);
    slot.guard_size.store(record.guard_size, Ordering::Relaxed);
    slot.state.store(
        if record.detached {
            STATE_DETACHED
        } else {
            STATE_JOINABLE
        },
        Ordering::Relaxed,
    );
}

/// Locate the slot tracking `task_id` in the table starting at `first`.
///
/// The two sentinels are never a thread's id and must not match: an id of 0
/// found an empty slot, which `pthread_detach(0)` then marked detached and
/// reported as a success.
fn find_slot_in(first: &'static ThreadChunk, task_id: u64) -> Option<&'static ThreadSlot> {
    if task_id == SLOT_EMPTY || task_id == SLOT_RESERVED {
        return None;
    }
    chunks_from(first)
        .flat_map(|chunk| chunk.slots.iter())
        .find(|slot| slot.task_id.load(Ordering::Acquire) == task_id)
}

/// [`find_slot_in`] the process's table.
fn find_slot(task_id: u64) -> Option<&'static ThreadSlot> {
    find_slot_in(&THREAD_TABLE, task_id)
}

/// Release a slot back to the pool.  Must be called only by the single
/// party that owns the mapping's free (see [`ThreadSlot`] protocol).
fn release_slot(slot: &ThreadSlot) {
    slot.task_id.store(SLOT_EMPTY, Ordering::Release);
}

/// The calling thread's own slot, whose address its creator left in its
/// per-thread block before it started; `None` for the initial thread.
fn own_slot() -> Option<&'static ThreadSlot> {
    // SAFETY: `current()` is the calling thread's block, written by another
    // thread only before this one started.
    let addr = unsafe { (*crate::perthread::current()).thread_slot };
    // SAFETY: a non-zero value is the address of a slot in the table, put
    // there by `pthread_create`, and chunks are never freed.
    unsafe { (addr as *const ThreadSlot).as_ref() }
}

/// Read-only snapshot of a tracked thread's metadata.
// Only consumed by `find_thread_info` (the `pthread_getattr_np` helper),
// which is itself `target_os="none"`-gated; carry the same gate so the
// host/slateos builds don't see it as dead code.
#[cfg(target_os = "none")]
#[derive(Clone, Copy)]
struct ThreadInfo {
    stack_base: usize,
    stack_size: usize,
    guard_size: usize,
    detached: bool,
}

#[cfg(target_os = "none")]
impl ThreadInfo {
    fn of(slot: &ThreadSlot) -> Self {
        Self {
            stack_base: slot.stack_base.load(Ordering::Relaxed),
            stack_size: slot.stack_size.load(Ordering::Relaxed),
            guard_size: slot.guard_size.load(Ordering::Relaxed),
            detached: slot.state.load(Ordering::Acquire) == STATE_DETACHED,
        }
    }
}

/// Look up a thread's metadata without removing it, for
/// `pthread_getattr_np`.
///
/// The calling thread is answered from its own slot, which exists from
/// before it ran: Rust's std asks for its thread's stack bounds as the
/// thread starts, possibly before the creator has published the thread's
/// id -- and a lookup by id then found nothing and reported the *main*
/// thread's stack.
#[cfg(target_os = "none")]
fn find_thread_info(task_id: u64) -> Option<ThreadInfo> {
    if task_id == pthread_self() {
        if let Some(slot) = own_slot() {
            return Some(ThreadInfo::of(slot));
        }
    }
    find_slot(task_id).map(ThreadInfo::of)
}

// ---------------------------------------------------------------------------
// Assembly trampoline — entry point for new threads
// ---------------------------------------------------------------------------

// The trampoline runs in ring 3 on the new thread's user stack.
// Stack layout at entry (all three words pushed by `pthread_create`):
//   [RSP]      = arg pointer         (for start_routine)
//   [RSP + 8]  = start_routine ptr   (function to call)
//   [RSP + 16] = thread pointer      (this thread's %fs base, pre-built)
//
// The trampoline pops the three words straight into the SysV argument
// registers and calls `__pthread_thread_start(start_routine, arg, tp)`,
// which installs the thread pointer, runs the routine, executes TSD
// destructors, and issues SYS_THREAD_EXIT.
//
// Routing the return path through a Rust entry (rather than the old
// inline `call start_routine; SYS_THREAD_EXIT`) is what lets us run
// pthread-key destructors when a thread returns normally — POSIX
// requires that returning from the start routine behave exactly like
// `pthread_exit(return_value)`.
//
// Stack alignment: after the three pops RSP is back at the thread's stack
// top, which `pthread_create` places at a 16-byte boundary, so the CALL
// satisfies the SysV ABI requirement.
#[cfg(target_os = "none")]
core::arch::global_asm!(
    ".global _pthread_trampoline",
    ".type _pthread_trampoline, @function",
    "_pthread_trampoline:",
    "    pop rsi",                     // rsi = arg            (2nd C argument)
    "    pop rdi",                     // rdi = start_routine  (1st C argument)
    "    pop rdx",                     // rdx = thread pointer (3rd C argument)
    "    call __pthread_thread_start", // never returns
    "    ud2",                         // unreachable
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    fn _pthread_trampoline();
}

// ---------------------------------------------------------------------------
// Detached-thread self-unmap primitive
// ---------------------------------------------------------------------------
//
// A detached thread must free its *own* mapping as its final act (no joiner
// will do it).  This is the glibc `__unmapself` pattern: unmap the region
// and exit without ever touching the (now-freed) memory between the two
// syscalls.  The region covers the thread's stack *and* its TLS block/TCB
// (one mapping — see `crate::tls`), so past the munmap neither the stack nor
// `%fs`-relative memory may be touched; the code below touches only
// registers, and the kernel never dereferences a task's saved `fs_base` (it
// only writes the MSR).  We carry `retval` across the SYS_MUNMAP call in R12 — a
// callee-saved register the kernel's SYSCALL entry stub preserves (it
// pushes/pops rbx/rbp/r12-r15 around the handler, see kernel
// syscall/entry.rs) — so nothing is stashed on the doomed stack.
//
// Register in (SysV): rdi=map_base, rsi=map_size, rdx=retval.
//
// Why this is safe even though we unmap the running stack:
// - The SYS_MUNMAP syscall runs on the *kernel* stack; on SYSRET the CPU
//   merely reloads user RSP into the register (never dereferences it).
// - Every instruction after the munmap syscall touches only registers, so
//   the freed stack region is never read or written.
// - Asynchronous interrupts from ring-3 push onto the kernel stack (via
//   TSS.RSP0), not the user stack, so they don't fault on the freed page.
#[cfg(target_os = "none")]
core::arch::global_asm!(
    ".global __pthread_exit_unmap",
    ".type __pthread_exit_unmap, @function",
    "__pthread_exit_unmap:",
    "    mov r12, rdx",                 // stash retval in callee-saved r12
    "    mov rax, {sys_munmap}",        // SYS_MUNMAP(stack_base, stack_size)
    "    syscall",                      // rdi/rsi already hold base/size
    // --- stack is unmapped past this point; registers only ---
    "    mov rdi, r12",                 // arg0 = retval
    "    mov esi, 1",                   // arg1 = detached flag (don't retain exit value)
    "    mov rax, {sys_thread_exit}",   // SYS_THREAD_EXIT(retval, detached=1)
    "    syscall",
    "    ud2",                          // unreachable
    sys_munmap = const crate::syscall::SYS_MUNMAP,
    sys_thread_exit = const crate::syscall::SYS_THREAD_EXIT,
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    /// Free `[map_base, map_base+map_size)` then terminate the calling
    /// thread with `retval`, never touching that memory between the two
    /// syscalls.  Never returns.  Caller must guarantee the region is this
    /// thread's own stack+TLS mapping and that no other party will also free
    /// it.
    fn __pthread_exit_unmap(map_base: usize, map_size: usize, retval: u64);
}

/// Rust entry point for a newly created thread (called by the assembly
/// trampoline).  Installs the thread pointer, runs the user start routine,
/// then exits via [`pthread_exit`] so that thread-specific-data destructors
/// run before the kernel tears the thread down.  Returning from `start` is,
/// per POSIX, equivalent to `pthread_exit(start(arg))`.
///
/// `tp` is the thread pointer `pthread_create` built for us: the block and
/// TCB are already initialised (in the creating thread), so all that is
/// left is to point `%fs` at them.  This **must** be the first thing the
/// new thread does — every C function compiled with a stack protector reads
/// `%fs:0x28` in its prologue, and any `__thread` access is a `%fs`-relative
/// load, so until `fs_base` is installed the thread can only safely execute
/// code that touches neither.  This function qualifies: Rust emits no stack
/// canary (the target spec enables no stack protector) and posix's Rust code
/// uses no `#[thread_local]`.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn __pthread_thread_start(
    start: extern "C" fn(*mut u8) -> *mut u8,
    arg: *mut u8,
    tp: u64,
) -> ! {
    // A zero `tp` means the creator could not allocate TLS; it never starts
    // a thread in that case (`pthread_create` fails with EAGAIN), so this is
    // only a defensive guard against a bad `fs_base` install.
    if tp != 0 {
        let _ = crate::tls::install(tp);
    }
    let ret = start(arg);
    pthread_exit(ret);
}

// ---------------------------------------------------------------------------
// Thread creation / management
// ---------------------------------------------------------------------------

/// Create a new thread.
///
/// Honours the attribute: its stack size, its guard size, a stack of the
/// caller's own (`pthread_attr_setstack`) and its detach state.  Until
/// 2026-09-26 the attribute was ignored -- every thread got a 64 KiB stack
/// with no guard, created joinable, whatever it asked for -- so a thread that
/// asked for a megabyte and used it ran off the end of its stack into
/// whatever was mapped below (`known-issues.md` →
/// `B-D-PTHREAD-CREATE-IGNORED-ITS-ATTRIBUTE`).  Rust's std asks for 2 MiB.
///
/// The TLS block is built here, in the creating thread, rather than by the
/// child: it is the only place a failure can be reported (`EAGAIN`), and
/// keeping it in a mapping this library made means the join/detach reclaim
/// protocol frees it too -- one mapping, one owner (see [`crate::tls`] for
/// the layout).
///
/// The thread's slot is claimed and filled before the thread exists, and its
/// address is left in the thread's per-thread block, so the thread can find
/// its slot even if it exits before this function has published its id.
///
/// Returns 0 on success, or a POSIX error number on failure.  A NULL `start`
/// is `EFAULT`, after the thread's memory could be had: glibc creates the
/// thread, which faults calling it (design-decisions.md §1115).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_create(
    thread: *mut PthreadT,
    attr: *const PthreadAttrT,
    start: Option<extern "C" fn(*mut u8) -> *mut u8>,
    arg: *mut u8,
) -> i32 {
    let want = CreateAttr::read(attr);
    // Every thread here runs as `SCHED_OTHER` at priority 0 (see
    // `pthread_getschedparam`): an explicit request for anything else is one
    // this system cannot carry out, refused as Linux refuses an unprivileged
    // real-time request, before anything is allocated.
    if let Some((policy, priority)) = want.explicit_sched
        && (policy != crate::sched::SCHED_OTHER || priority != 0)
    {
        return errno::EPERM;
    }
    if let Some(e) = want.affinity {
        return e;
    }
    let Some(slot) = claim_slot() else {
        return errno::EAGAIN;
    };
    match launch(slot, &want, start, arg) {
        Ok(task_id) => {
            // Publish.  From here `pthread_join`/`pthread_detach` can find
            // the thread, and the thread -- which waits for this store before
            // it lets go of its slot -- may exit.
            slot.task_id.store(task_id, Ordering::Release);
            if !thread.is_null() {
                // SAFETY: caller guarantees thread points to valid PthreadT.
                unsafe {
                    *thread = task_id;
                }
            }
            0
        }
        Err(e) => {
            release_slot(slot);
            e
        }
    }
}

/// What `pthread_create` was asked for, read out of its attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CreateAttr {
    /// Usable stack in bytes, before rounding to pages.
    stack_size: usize,
    /// Guard below the stack in bytes, before rounding.  Ignored for a stack
    /// the caller supplied, as POSIX requires.
    guard_size: usize,
    /// Lowest address of a stack the caller supplied, if any.
    stack_addr: Option<usize>,
    /// Start detached.
    detached: bool,
    /// The policy and priority asked for with `PTHREAD_EXPLICIT_SCHED`;
    /// `None` to inherit the creator's.
    explicit_sched: Option<(i32, i32)>,
    /// What making the attribute's CPU set the new thread's affinity would
    /// answer, if it has one and that is not 0: glibc's `pthread_create`
    /// sets it on the new thread and fails with its error, the thread never
    /// running.  Here every thread runs on every CPU, so a set of them all
    /// is kept and any other refused -- `ENOSYS`, or `EINVAL` for one with no
    /// CPU there is (`crate::sched::affinity_change`).
    affinity: Option<i32>,
}

impl CreateAttr {
    /// What `pthread_attr_init`'s values read as -- and so what a NULL
    /// attribute stands for until `pthread_setattr_default_np` changes the
    /// defaults. Only the tests name it: `read` takes a NULL attribute from
    /// the current defaults.
    #[cfg(test)]
    const DEFAULT: Self = Self {
        stack_size: DEFAULT_THREAD_STACK_SIZE,
        guard_size: DEFAULT_GUARD_SIZE,
        stack_addr: None,
        detached: false,
        explicit_sched: None,
        affinity: None,
    };

    /// What `attr` asks for; NULL stands for the process's default
    /// attributes (`pthread_setattr_default_np`), read while they are
    /// locked -- their CPU set is theirs, and another thread may replace
    /// them.
    fn read(attr: *const PthreadAttrT) -> Self {
        if attr.is_null() {
            return with_default_attr(|d| Self::read_buf(d));
        }
        // SAFETY: non-null, and by the caller's contract an initialised
        // attribute object.
        Self::read_buf(unsafe { &*attr })
    }

    /// What the attribute object `buf` asks for.
    fn read_buf(buf: &PthreadAttrT) -> Self {
        let size = attr_read_stacksize(buf);
        let stack_size = if size == 0 {
            DEFAULT_THREAD_STACK_SIZE
        } else {
            size
        };
        let top = attr_read_stackaddr(buf);
        // SAFETY: an initialised attribute object's extension is live.
        let affinity = unsafe { attr_cpuset(buf) }.and_then(|set| {
            crate::sched::read_affinity_mask(set.len(), set.as_ptr().cast())
                .and_then(|mask| crate::sched::affinity_change(&mask, crate::sched::online_cpus()))
                .err()
        });
        Self {
            stack_size,
            guard_size: attr_read_guardsize(buf),
            // The top less the size: glibc's `stackaddr - stacksize`, which
            // a caller with a top below its size has wrap as glibc's does.
            stack_addr: (top != 0).then(|| top.wrapping_sub(stack_size)),
            detached: attr_read_detachstate(buf) == PTHREAD_CREATE_DETACHED,
            explicit_sched: (attr_read_i32(buf, ATTR_OFF_INHERIT) == PTHREAD_EXPLICIT_SCHED).then(
                || {
                    (
                        attr_read_i32(buf, ATTR_OFF_POLICY),
                        attr_read_i32(buf, ATTR_OFF_PRIORITY),
                    )
                },
            ),
            affinity,
        }
    }
}

/// The memory `pthread_create` maps for a thread, before it has an address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThreadPlan {
    /// Bytes to map, a whole number of pages.
    map_size: usize,
    /// Guard at the bottom of the mapping, a whole number of pages (0 for
    /// none).
    guard: usize,
    /// Usable stack, a whole number of pages -- 0 when the caller supplied
    /// the stack and the mapping holds only the TLS part.
    stack: usize,
    /// The caller's stack: its lowest address and its size.
    user_stack: Option<(usize, usize)>,
}

/// Where a thread's pieces land once its mapping has an address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThreadLayout {
    /// Lowest address of the usable stack.
    stack_base: usize,
    /// The stack's top, 16-aligned; the trampoline's words go just below.
    stack_top: usize,
    /// The thread pointer.
    tp: u64,
}

/// Size a thread's mapping; `None` when the sizes asked for overflow.
fn plan_thread(want: &CreateAttr, img: &crate::tls::TlsImage) -> Option<ThreadPlan> {
    let page = crate::unistd::PAGE_SIZE;
    let tls = usize::try_from(img.reserve()).ok()?;
    if let Some(addr) = want.stack_addr {
        return Some(ThreadPlan {
            map_size: tls.checked_next_multiple_of(page)?,
            guard: 0,
            stack: 0,
            user_stack: Some((addr, want.stack_size)),
        });
    }
    let guard = want.guard_size.checked_next_multiple_of(page)?;
    let stack = want.stack_size.checked_next_multiple_of(page)?;
    let map_size = guard
        .checked_add(stack)?
        .checked_add(tls)?
        .checked_next_multiple_of(page)?;
    Some(ThreadPlan {
        map_size,
        guard,
        stack,
        user_stack: None,
    })
}

impl ThreadPlan {
    /// Lay the thread out in the mapping at `map_base`.
    fn place(&self, map_base: usize, img: &crate::tls::TlsImage) -> ThreadLayout {
        if let Some((addr, size)) = self.user_stack {
            // The caller's stack as given; the TLS part fills our mapping.
            return ThreadLayout {
                stack_base: addr,
                stack_top: addr.wrapping_add(size) & !0xf,
                tp: img.thread_pointer(map_base as u64, 0),
            };
        }
        let stack_base = map_base.wrapping_add(self.guard);
        // Variant-II layout: TLS block immediately below the thread pointer,
        // TCB at and above it, both above the stack.  The stack therefore
        // ends where the TLS block begins — rounded *down* to 16, because the
        // TLS block's start only inherits the segment's `p_align`, which the
        // psABI permits to be as weak as 1.  SysV requires RSP+8 to be
        // 16-byte aligned at a function's entry, and the trampoline pops
        // exactly three words before its `call`, so RSP at
        // `__pthread_thread_start` is `stack_top - 8`: an unaligned
        // `stack_top` would misalign every SSE spill in the child.  Rounding
        // down costs at most 15 bytes of stack and never encroaches on the
        // TLS block above.
        let tp = img.thread_pointer(stack_base as u64, self.stack as u64);
        ThreadLayout {
            stack_base,
            stack_top: (tp.wrapping_sub(img.block_size()) as usize) & !0xf,
            tp,
        }
    }
}

/// Map, prepare and start the thread whose slot is `slot`: its task id, or
/// the error number `pthread_create` returns.  On failure nothing is left
/// mapped; the caller releases the slot.
fn launch(
    slot: &'static ThreadSlot,
    want: &CreateAttr,
    start: Option<extern "C" fn(*mut u8) -> *mut u8>,
    arg: *mut u8,
) -> Result<u64, i32> {
    let tls_img = crate::tls::image();
    let plan = plan_thread(want, &tls_img).ok_or(errno::EAGAIN)?;
    let mem = crate::mman::mmap(
        core::ptr::null_mut(),
        plan.map_size,
        crate::mman::PROT_READ | crate::mman::PROT_WRITE,
        crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
        -1,
        0,
    );
    if mem == crate::mman::MAP_FAILED {
        return Err(errno::EAGAIN);
    }
    // Every failure below unmaps `mem` again.  The mapping was never shared,
    // so an unmap that failed would only leave it mapped.
    if plan.guard > 0 && crate::mman::mprotect(mem, plan.guard, crate::mman::PROT_NONE) != 0 {
        let _ = crate::mman::munmap(mem, plan.map_size);
        return Err(errno::EAGAIN);
    }
    // A NULL start routine: glibc creates the thread, which faults calling it.
    // `pthread_create` can fail, so it does here, as late as it can without a
    // thread -- after the memory the thread needed was had.
    let Some(start) = start else {
        let _ = crate::mman::munmap(mem, plan.map_size);
        return Err(errno::EFAULT);
    };
    let map_base = mem as usize;
    let layout = plan.place(map_base, &tls_img);

    // Initialise the child's TLS block, TCB and per-thread block before it
    // can run.
    // SAFETY: mmap succeeded, so [map_base, map_base + map_size) is valid;
    // `plan_thread` sized it for `thread_pointer`'s contract, which puts
    // [tp - block_size, tp + TCB_SIZE + perthread::BLOCK_SIZE) inside it,
    // and no thread uses it yet.
    unsafe {
        crate::tls::init_block(layout.tp, &tls_img);
        // The SAME value as every other thread, deliberately: glibc copies
        // the parent's guard into the child TCB, and a thread that used a
        // different one would abort a process that was never smashed the
        // moment a frame outlived the change. The parent's TLS is live here,
        // so the lookup is safe.
        crate::tls::set_stack_guard(layout.tp, crate::crt::process_stack_guard());
        (*crate::perthread::block_at(layout.tp)).thread_slot = core::ptr::from_ref(slot) as usize;
    }
    fill_slot(
        slot,
        &ThreadRecord {
            map_base,
            map_size: plan.map_size,
            stack_base: layout.stack_base,
            stack_size: layout.stack_top.wrapping_sub(layout.stack_base),
            guard_size: plan.guard,
            detached: want.detached,
        },
    );

    // Push arg, start_routine and the thread pointer onto the new stack for
    // the trampoline (see its stack-layout comment).
    // SAFETY: the three words lie just below `stack_top`, inside the stack:
    // our own mapping, or the caller's stack, which POSIX makes the caller
    // vouch for.
    unsafe {
        let tp_slot = layout.stack_top.wrapping_sub(8) as *mut u64;
        let fn_slot = layout.stack_top.wrapping_sub(16) as *mut u64;
        let arg_slot = layout.stack_top.wrapping_sub(24) as *mut u64;
        core::ptr::write(tp_slot, layout.tp);
        core::ptr::write(fn_slot, start as usize as u64);
        core::ptr::write(arg_slot, arg as u64);
    }
    let user_rsp = layout.stack_top.wrapping_sub(24) as u64;

    // Get the trampoline's address.
    #[cfg(target_os = "none")]
    let entry = _pthread_trampoline as *const () as u64;
    #[cfg(not(target_os = "none"))]
    let entry: u64 = 0;

    // Create the kernel thread, counted first: it may end before the
    // syscall returns (`LIVE_THREADS`).
    live_threads_add(&LIVE_THREADS);
    let ret = syscall::syscall3(
        syscall::SYS_THREAD_CREATE,
        entry,
        user_rsp,
        u64::MAX, // default priority
    );
    if ret < 0 {
        // It never ran, so it can never be the last; the result is moot.
        let _ = live_threads_remove(&LIVE_THREADS);
        let _ = crate::mman::munmap(mem, plan.map_size);
        return Err(errno::EAGAIN);
    }
    Ok(ret as u64)
}

/// Wait for a thread to terminate.
///
/// Blocks until the specified thread exits, stores its return value
/// in `*retval` (if non-null), and frees the thread's stack.
///
/// Returns 0 on success, or a POSIX error number on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_join(thread_id: PthreadT, retval: *mut *mut u8) -> i32 {
    // A thread waiting for itself would wait for ever: glibc's EDEADLK.
    if is_calling_thread(thread_id) {
        return errno::EDEADLK;
    }
    // A detached thread must not be joined (POSIX: EINVAL).  Reject early
    // so we never race the detached thread's self-unmap.
    if let Some(slot) = find_slot(thread_id) {
        if slot.state.load(Ordering::Acquire) == STATE_DETACHED {
            return errno::EINVAL;
        }
    }

    // The exit value comes back through an out-pointer, not the return
    // register: a thread may exit with a legitimately negative value
    // (`pthread_exit(PTHREAD_CANCELED)` is `(void *)-1`), which a
    // value-in-rax ABI could not tell apart from an error code.
    let mut exit_value: i64 = 0;
    let ret = syscall::syscall2(
        syscall::SYS_THREAD_JOIN,
        thread_id,
        (&raw mut exit_value) as u64,
    );

    if ret < 0 {
        // `Cancelled` (-5) means the target was killed — an unhandled
        // fault, an explicit kill, or process teardown — so it never
        // produced a value.  POSIX reserves exactly one representation
        // for that: `PTHREAD_CANCELED`.  Report it as a *successful*
        // join with that sentinel rather than as ESRCH, so the caller
        // can tell "the thread died" from "no such thread" — and, above
        // all, never sees it as a normal return of 0.  The stack is
        // still ours to reclaim below either way.
        if ret != KERNEL_ERR_CANCELLED {
            return errno::ESRCH;
        }
        exit_value = PTHREAD_CANCELED_VALUE;
    }

    if !retval.is_null() {
        // SAFETY: caller guarantees retval is a valid pointer.
        unsafe {
            *retval = exit_value as *mut u8;
        }
    }

    // Free the thread's mapping (stack + TLS block + TCB).  SYS_THREAD_JOIN
    // has confirmed the thread has exited — it is off its stack and no
    // longer reads its TLS — so the unmap is safe.  Release the slot before
    // unmapping so it can be reused promptly.
    if let Some(slot) = find_slot(thread_id) {
        let base = slot.map_base.load(Ordering::Relaxed);
        let size = slot.map_size.load(Ordering::Relaxed);
        release_slot(slot);
        if base != 0 {
            let _ = crate::mman::munmap(base as *mut core::ffi::c_void, size);
        }
    }

    0
}

/// Whether `thread_id` is the calling thread's own id.
fn is_calling_thread(thread_id: PthreadT) -> bool {
    u64::try_from(current_tid()).is_ok_and(|me| me == thread_id)
}

/// How far a thread is on its way to being joined: whether it has exited,
/// or why it cannot be joined at all -- itself (`EDEADLK`), detached
/// (`EINVAL`), or not a thread this process tracks (`ESRCH`).
///
/// "Exited" is the thread's own word on its way out, its slot's
/// `STATE_EXITED`. A thread killed before it could say so -- by a fault it
/// did not handle -- is never seen to have exited here, though
/// `pthread_join` would return `PTHREAD_CANCELED` for it: the kernel's join
/// only waits (known-issues.md, D-POSIX-TRYJOIN-CANNOT-SEE-A-KILLED-THREAD).
fn join_readiness(thread_id: PthreadT) -> Result<bool, i32> {
    if is_calling_thread(thread_id) {
        return Err(errno::EDEADLK);
    }
    let slot = find_slot(thread_id).ok_or(errno::ESRCH)?;
    match slot.state.load(Ordering::Acquire) {
        STATE_DETACHED => Err(errno::EINVAL),
        STATE_EXITED => Ok(true),
        _ => Ok(false),
    }
}

/// Join a thread only if it has already exited (`pthread_tryjoin_np`, GNU):
/// as [`pthread_join`] then, and `EBUSY` at once while it runs. `EDEADLK`
/// for the calling thread, `EINVAL` for a detached one, `ESRCH` for one
/// this process does not know.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_tryjoin_np(thread_id: PthreadT, retval: *mut *mut u8) -> i32 {
    match join_readiness(thread_id) {
        Ok(true) => pthread_join(thread_id, retval),
        Ok(false) => errno::EBUSY,
        Err(e) => e,
    }
}

/// How long [`pthread_timedjoin_np`] sleeps between looks: the kernel has
/// no timed join, so it polls.
const TIMEDJOIN_POLL_NS: i64 = 1_000_000;

/// Join a thread, waiting for it until `abstime` on `CLOCK_REALTIME`
/// (`pthread_timedjoin_np`, GNU): as [`pthread_join`] once it exits, and
/// `ETIMEDOUT` if the time comes first. A NULL `abstime` waits without end,
/// as glibc's does; one whose nanoseconds are outside 0..1e9 is `EINVAL`,
/// as it is to every other timed wait -- when there is a wait, that is: a
/// thread already gone is joined whatever `abstime` says. Otherwise as
/// [`pthread_tryjoin_np`].
///
/// # Safety
///
/// `abstime` is NULL or a readable `struct timespec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_timedjoin_np(
    thread_id: PthreadT,
    retval: *mut *mut u8,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    // SAFETY: the caller's contract.
    unsafe { timed_join(thread_id, retval, crate::time::CLOCK_REALTIME, abstime) }
}

/// Join a thread, waiting for it until `abstime` on `clockid`
/// (`pthread_clockjoin_np`, GNU): [`pthread_timedjoin_np`] against the clock
/// named, which must be `CLOCK_REALTIME` or `CLOCK_MONOTONIC` -- `EINVAL`
/// for any other, before the thread is looked at, as glibc checks it.
///
/// # Safety
///
/// `abstime` is NULL or a readable `struct timespec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_clockjoin_np(
    thread_id: PthreadT,
    retval: *mut *mut u8,
    clockid: i32,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    if clockid != crate::time::CLOCK_REALTIME && clockid != crate::time::CLOCK_MONOTONIC {
        return errno::EINVAL;
    }
    // SAFETY: the caller's contract.
    unsafe { timed_join(thread_id, retval, clockid, abstime) }
}

/// [`pthread_clockjoin_np`] with the clock checked.
///
/// # Safety
///
/// As [`pthread_clockjoin_np`].
unsafe fn timed_join(
    thread_id: PthreadT,
    retval: *mut *mut u8,
    clockid: i32,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    if abstime.is_null() {
        return pthread_join(thread_id, retval);
    }
    // SAFETY: non-null, the caller's.
    let deadline = unsafe { *abstime };
    loop {
        match join_readiness(thread_id) {
            Ok(true) => return pthread_join(thread_id, retval),
            Ok(false) => {}
            Err(e) => return e,
        }
        if !(0..1_000_000_000).contains(&deadline.tv_nsec) {
            return errno::EINVAL;
        }
        let mut now = crate::stat::Timespec::default();
        if crate::time::clock_gettime(clockid, &raw mut now) != 0 {
            return errno::get_errno();
        }
        let left_ns = i128::from(deadline.tv_sec)
            .saturating_sub(i128::from(now.tv_sec))
            .saturating_mul(1_000_000_000)
            .saturating_add(i128::from(deadline.tv_nsec))
            .saturating_sub(i128::from(now.tv_nsec));
        if left_ns <= 0 {
            return errno::ETIMEDOUT;
        }
        let pause = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: i64::try_from(left_ns.min(i128::from(TIMEDJOIN_POLL_NS)))
                .unwrap_or(TIMEDJOIN_POLL_NS),
        };
        // A short or failed sleep only means an earlier look.
        let _ = crate::time::nanosleep(&raw const pause, core::ptr::null_mut());
    }
}

/// Detach a thread.
///
/// Marks the thread so its stack is reclaimed when it exits, without a
/// `pthread_join`.  A detached thread frees its own stack on exit (see
/// [`pthread_exit`]).  If the thread has *already* exited (as joinable)
/// by the time we detach it, we reap it here instead.
///
/// Returns 0 on success, `ESRCH` if no such thread is tracked, or
/// `EINVAL` if the thread is already detached.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_detach(thread_id: PthreadT) -> i32 {
    let Some(slot) = find_slot(thread_id) else {
        return errno::ESRCH;
    };

    match slot.state.compare_exchange(
        STATE_JOINABLE,
        STATE_DETACHED,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        // Thread still running (or not yet at exit): it will self-unmap
        // its stack when it exits.  Nothing more to do here.
        Ok(_) => 0,
        // Thread already exited while joinable and left its stack behind.
        // We now own the reclaim: join to guarantee it is off its stack,
        // then free it.
        Err(STATE_EXITED) => {
            // Reaping only — the exit value is discarded, so pass a null
            // out-pointer rather than a scratch slot.
            let _ = syscall::syscall2(syscall::SYS_THREAD_JOIN, thread_id, 0);
            let base = slot.map_base.load(Ordering::Relaxed);
            let size = slot.map_size.load(Ordering::Relaxed);
            release_slot(slot);
            if base != 0 {
                let _ = crate::mman::munmap(base as *mut core::ffi::c_void, size);
            }
            0
        }
        // Already detached (double detach) or an unexpected state.
        Err(_) => errno::EINVAL,
    }
}

/// Get the calling thread's ID.
///
/// Returns the kernel task ID of the calling thread.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_self() -> PthreadT {
    syscall::syscall0(syscall::SYS_TASK_ID) as PthreadT
}

/// Compare two thread IDs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_equal(t1: PthreadT, t2: PthreadT) -> i32 {
    i32::from(t1 == t2)
}

// ---------------------------------------------------------------------------
// Identifying the initial thread
//
// `THREAD_TABLE` only holds threads this library created; the initial thread
// arrives from the kernel and has no slot.  That is fine for stack
// bookkeeping — nobody unmaps the initial stack — but it is not fine for
// *existence* questions: without this, a worker thread asking about the
// thread that started the process would be told ESRCH, which is the one
// answer that is definitely wrong.  `pthread_kill(main_thread, SIGTERM)` is
// a standard shutdown idiom, so this is not a corner case.
//
// Recording the id in a static rather than giving the initial thread a table
// slot keeps the table's ownership protocol intact: every slot in it names a
// mapping that exactly one party must free, and the initial thread owns no
// such mapping.
// ---------------------------------------------------------------------------

/// Kernel task id of the initial (crt0) thread, or `SLOT_EMPTY` before
/// [`record_initial_thread`] has run.
static INITIAL_TASK_ID: AtomicU64 = AtomicU64::new(SLOT_EMPTY);

/// Record the calling thread as the process's initial thread.
///
/// Called once from `crate::crt::__libc_start_main`, on the initial thread,
/// before `main`.  Calling it from anywhere else would make the functions
/// below misidentify which thread is the initial one, so it is
/// crate-private.
pub(crate) fn record_initial_thread() {
    INITIAL_TASK_ID.store(pthread_self(), Ordering::Release);
}

/// Does `thread` name a thread of this process that is, as far as we can
/// tell, still alive?
///
/// Three sources, in increasing cost:
/// 1. it is us — always live, and the answer needs no table;
/// 2. it is the initial thread — see [`INITIAL_TASK_ID`];
/// 3. it holds a slot in `THREAD_TABLE`.
///
/// Case 3 has a benign race: a thread that has exited but not yet been
/// joined still holds its slot, so this reports it live.  That matches
/// POSIX, which makes `pthread_kill` on an unjoined-but-exited thread
/// undefined rather than requiring `ESRCH`, and matches Linux, where the
/// task stays a zombie until reaped.  What it must never do is the reverse —
/// report a live thread as absent — and it cannot, because a slot is only
/// released after the thread is confirmed off its stack.
pub(crate) fn thread_is_live(thread: PthreadT) -> bool {
    if thread == SLOT_EMPTY {
        // No kernel task has id 0 — that is precisely why `SLOT_EMPTY` is
        // 0.  Rejecting it here also stops an unrecorded `INITIAL_TASK_ID`
        // from matching a caller's zeroed `pthread_t`.
        return false;
    }
    thread == pthread_self()
        || thread == INITIAL_TASK_ID.load(Ordering::Acquire)
        || find_slot(thread).is_some()
}

/// Send a signal to a specific thread.
///
/// Returns 0 on success or a positive errno.  Like the rest of the
/// `pthread_*` family, and unlike `kill(2)`, this does **not** set `errno`.
///
/// * `EINVAL` — `sig` is outside `[0, NSIG)`.
/// * `ESRCH`  — no such thread in this process.
///
/// `sig == 0` performs the existence check without delivering anything,
/// exactly as `kill(pid, 0)` does.
///
/// # What "to a specific thread" means here
///
/// Signal delivery in this system is **process-directed**: the kernel sets a
/// signal pending on a process, not on a task (see this crate's `signal`
/// module header on `SYS_SIGNAL_SEND`).  So:
///
/// * `thread == pthread_self()` is exact.  The signal is dispatched
///   synchronously on this thread through the same path `raise(3)` uses,
///   which is precisely what `pthread_kill(pthread_self(), sig)` means.
///   This is also the overwhelmingly common call: CPython's
///   `signal.pthread_kill` in a single-threaded interpreter, every
///   `raise`-alike, and the whole "deliver to myself" idiom land here.
/// * For any **other** thread of this process the signal is delivered
///   process-directed, so the disposition runs on whichever thread next
///   dispatches rather than necessarily on the named one.  The signal is
///   not lost and the process-visible effect — handler runs, or the default
///   action is taken — is right; what is approximate is *which* thread runs
///   the handler.
///
/// That approximation is deliberate.  Refusing a peer thread with `ENOSYS`
/// would be more precise about our limitation and less useful about the
/// caller's intent: `pthread_kill(t, SIGTERM)` as a shutdown request works
/// correctly under process-directed delivery, while a caller told `ENOSYS`
/// learns only that it must find another way to do something we could in
/// fact do.  The use the approximation genuinely fails — "interrupt the
/// blocking call *in thread t*" — needs per-thread pending sets in the
/// kernel; that is tracked in `todo.txt`, and when it lands this function
/// targets the task directly with no change to any caller.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_kill(thread: PthreadT, sig: i32) -> i32 {
    // Signal number first: an out-of-range signal is a programming error
    // worth reporting even when the thread id is also stale, and Linux's
    // `tgkill` likewise rejects the signal independently of the lookup.
    if !(0..crate::signal::NSIG).contains(&sig) {
        return errno::EINVAL;
    }
    if !thread_is_live(thread) {
        return errno::ESRCH;
    }
    if sig == 0 {
        // Existence probe only — the check above is the entire answer.
        return 0;
    }
    if thread == pthread_self() {
        // Exact: dispatch on this thread, synchronously.
        if crate::signal::raise(sig) == 0 {
            return 0;
        }
        // `raise` reports through errno; convert to this family's
        // return-the-errno convention.  It can only fail on a signal
        // number `raise` rejects but we accepted, i.e. `sig == 0`, which
        // returned above — so this is unreachable in practice and the
        // fallback exists to avoid inventing a success.
        let e = errno::get_errno();
        return if e == 0 { errno::EINVAL } else { e };
    }
    // Peer thread: process-directed delivery (see the doc comment above).
    let self_pid = syscall::syscall0(syscall::SYS_PROCESS_ID);
    #[allow(clippy::cast_sign_loss)]
    let ret = syscall::syscall2(syscall::SYS_SIGNAL_SEND, self_pid as u64, sig as u64);
    if ret < 0 {
        // The target is our own process and the thread was just confirmed
        // to exist, so a failure here is not "no such process"; report what
        // the kernel actually said rather than guessing.
        #[allow(clippy::cast_possible_truncation)]
        return errno::translate(ret) as i32;
    }
    0
}

/// Obtain a clock id that measures a thread's CPU time.
///
/// Returns 0 on success or a positive errno; like the rest of this family it
/// does not set `errno`.
///
/// * `ESRCH`  — no such thread in this process.
/// * `EFAULT` — `clock_id` is NULL.  glibc has no such check and faults, and
///   POSIX leaves it undefined; reporting it is strictly more useful than a
///   crash and cannot be confused with a real clock id.
/// * `ENOENT` — the thread exists but has no CPU-time clock.  POSIX
///   documents exactly this errno for `pthread_getcpuclockid`: "the system
///   does not support CPU-time clocks for the specified thread."
///
/// # Which threads have a clock
///
/// Only the calling thread.  `clock_gettime(CLOCK_THREAD_CPUTIME_ID, …)`
/// reads *the caller's* clock, so handing that same id back for a peer would
/// produce a number that looks like the peer's CPU time and is in fact the
/// caller's — a silent wrong answer, which is worse than a refusal.  Linux
/// avoids this by encoding the tid into the clock id
/// (`CPUCLOCK_PERTHREAD_MASK`) and decoding it in the kernel; until
/// `clock_gettime` can do that decoding, `ENOENT` is the truthful answer for
/// a peer.
///
/// Note that even for the calling thread our `CLOCK_THREAD_CPUTIME_ID` is
/// currently the monotonic clock rather than accumulated CPU time — a
/// pre-existing approximation documented on `crate::time::clock_gettime`,
/// not one this function introduces.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_getcpuclockid(
    thread: PthreadT,
    clock_id: *mut crate::types::ClockidT,
) -> i32 {
    if !thread_is_live(thread) {
        return errno::ESRCH;
    }
    if clock_id.is_null() {
        return errno::EFAULT;
    }
    if thread != pthread_self() {
        return errno::ENOENT;
    }
    // SAFETY: clock_id is non-null (checked above).  Written unaligned
    // because the caller's storage need only meet the C ABI's alignment for
    // `clockid_t`, which Rust's `write` would additionally assume.
    unsafe {
        core::ptr::write_unaligned(clock_id, crate::time::CLOCK_THREAD_CPUTIME_ID);
    }
    0
}

/// Threads of this process that have not ended: the initial thread, plus
/// each one `pthread_create` started, less each one that reached
/// `pthread_exit` -- glibc's `__nptl_nthreads`.
///
/// The thread that takes it to zero ends the process as `exit(0)` would, as
/// POSIX requires of `pthread_exit` ("as if the implementation called
/// exit() with a zero argument at thread termination time"): `atexit`
/// handlers and static destructors run, and streams are flushed.  Until
/// 2026-09-26 the kernel ended the process when its last thread ended, and
/// none of that happened -- a program whose `main` called `pthread_exit` lost
/// its threads' buffered output.
///
/// Every thread this libc starts goes through [`launch`] (`pthread_create`'s),
/// which counts it *before* `SYS_THREAD_CREATE`, so a new thread that ends at
/// once can never take the count below the threads still running.  `fork`'s
/// child has one thread and says so ([`reset_live_threads_after_fork`]).
static LIVE_THREADS: AtomicUsize = AtomicUsize::new(1);

/// Count one more thread, about to be started.
fn live_threads_add(count: &AtomicUsize) {
    count.fetch_add(1, Ordering::AcqRel);
}

/// Count one thread fewer: `true` when it was the last.
///
/// Saturating, so a count that is somehow already zero stays there rather
/// than wrapping to "many threads left"; the caller then treats the thread as
/// the last, which is what a zero count means.
fn live_threads_remove(count: &AtomicUsize) -> bool {
    let before = count
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            Some(n.saturating_sub(1))
        })
        .unwrap_or_else(|n| n);
    before <= 1
}

/// `fork`'s child: the one thread that called `fork`.
pub(crate) fn reset_live_threads_after_fork() {
    LIVE_THREADS.store(1, Ordering::Release);
}

/// Terminate the calling thread.
///
/// Runs the calling thread's `thread_local` destructors, then its
/// thread-specific-data destructors -- glibc's order -- then issues
/// `SYS_THREAD_EXIT` with the specified return value.  If this is the last
/// thread in the process, the process ends as `exit(0)` would
/// ([`LIVE_THREADS`]).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_exit(retval: *mut u8) -> ! {
    // The cleanup handlers still pushed, innermost first, before anything
    // else: POSIX runs them, then the thread-specific-data destructors.
    run_cleanup_handlers();

    // C++ `thread_local` destructors, as glibc's `start_thread` calls
    // `__call_tls_dtors` before it deallocates the TSD: a destructor may
    // still use a key.
    crate::exit_list::run_thread_dtors();

    // POSIX: run key destructors and release this thread's TSD storage
    // before the kernel reclaims the thread; also free its name slot.
    let self_tid = pthread_self();
    tsd_thread_cleanup();
    crate::netdb::thread_cleanup();
    crate::dlfcn::thread_cleanup();
    thread_name_release(self_tid);

    // The last thread ends the process, and as `exit(0)`: see
    // `LIVE_THREADS`.  Before anything below gives this thread's stack away.
    if live_threads_remove(&LIVE_THREADS) {
        crate::crt::exit(0);
    }

    // Decide how this thread's stack is reclaimed.  The `compare_exchange`
    // arbitrates against a concurrent `pthread_detach`: exactly one party
    // ends up owning the free.
    let mut self_unmap: Option<(usize, usize)> = None;
    if let Some(slot) = own_slot() {
        // Our creator publishes our id just after `SYS_THREAD_CREATE`
        // returns to it.  A thread that gets here first waits for that: it
        // must not release a slot its creator is about to write.  The wait
        // is a few instructions of the creator's, so it is almost never
        // entered at all.
        while slot.task_id.load(Ordering::Acquire) == SLOT_RESERVED {
            sched_yield();
        }
        match slot.state.compare_exchange(
            STATE_JOINABLE,
            STATE_EXITED,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            // We were joinable at exit: leave the stack and slot for a
            // future `pthread_join` (or a `pthread_detach` that observes
            // STATE_EXITED) to reclaim.
            Ok(_) => {}
            // We were detached: free our own stack as the final act.
            // Release the slot *before* unmapping (after the unmap we can
            // no longer safely touch memory).
            Err(STATE_DETACHED) => {
                let base = slot.map_base.load(Ordering::Relaxed);
                let size = slot.map_size.load(Ordering::Relaxed);
                release_slot(slot);
                if base != 0 {
                    self_unmap = Some((base, size));
                }
            }
            // Already EXITED (double pthread_exit — shouldn't happen) or an
            // unexpected state: fall through to a plain exit.
            Err(_) => {}
        }
    }

    #[cfg(target_os = "none")]
    if let Some((base, size)) = self_unmap {
        // SAFETY: (base, size) describes this thread's own mmap'd
        // stack+TLS region, and the STATE_DETACHED arbitration above
        // guarantees no other party will free it.  `__pthread_exit_unmap`
        // issues SYS_MUNMAP then SYS_THREAD_EXIT without touching the
        // (freed) stack or TLS between the two, carrying `retval` in a
        // callee-saved register.  Never returns.  All code that may touch
        // TLS — including the user's TSD destructors — has already run
        // above.
        unsafe {
            __pthread_exit_unmap(base, size, retval as u64);
        }
    }
    // Consumed on the bare-metal detached path above; unused on host.
    let _ = self_unmap;

    // Joinable exit: pass detached=0 explicitly so the kernel retains the
    // exit value for a future join (arg1 must be a defined 0, not stale).
    let _ = syscall::syscall2(syscall::SYS_THREAD_EXIT, retval as u64, 0);
    // SAFETY: SYS_THREAD_EXIT never returns; this is a safety net.
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nostack, nomem));
        }
    }
}

// ---------------------------------------------------------------------------
// Mutex operations — thread-safe via atomics
// ---------------------------------------------------------------------------

/// Initialize a mutex.
///
/// Reads the mutex type from `attr` (if non-null) to determine whether
/// the mutex is normal, recursive, or error-checking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_init(
    mutex: *mut PthreadMutexT,
    attr: *const PthreadMutexattrT,
) -> i32 {
    // glibc's order (nptl/pthread_mutex_init.c): the attribute's sanity
    // checks, then the mutex.  A protocol other than none, and robustness,
    // are ENOTSUP here -- no priority-inheriting futexes, no priority
    // ceilings, no robust list -- as glibc answers where it lacks them.
    let word: u32 = if attr.is_null() {
        0
    } else {
        // SAFETY: attr verified non-null; `[u8; 4]`, so read unaligned.
        unsafe { core::ptr::read_unaligned(attr.cast::<u32>()) }
    };
    if word & (MUTEXATTR_PROTOCOL_MASK | MUTEXATTR_FLAG_ROBUST) != 0 {
        return errno::ENOTSUP;
    }
    if mutex.is_null() {
        return errno::EFAULT;
    }
    let kind = (word & !MUTEXATTR_FLAG_BITS) as i32;
    // SAFETY: caller guarantees mutex is valid.
    unsafe {
        (*mutex).locked.store(0, Ordering::Release);
        (*mutex).kind.store(kind, Ordering::Release);
        (*mutex).owner.store(0, Ordering::Release);
        (*mutex).count.store(0, Ordering::Release);
    }
    0
}

/// The calling thread's kernel task id, from its per-thread block: fetched
/// by syscall the first time, a load after.  glibc keeps it in `struct
/// pthread` for the same reason -- every lock records its owner, and until
/// 2026-09-26 a `SYS_TASK_ID` syscall was the uncontended lock's whole cost.
/// `fork`'s child resets it (see `process::fork`), its id being new.
pub(crate) fn current_tid() -> i32 {
    let pt = crate::perthread::current();
    // SAFETY: `current()` is the calling thread's block (or, in a program
    // with no thread pointer, the single-threaded fallback); only this thread
    // touches it.
    let cached = unsafe { (*pt).tid };
    if cached != 0 {
        return cached;
    }
    let tid = raw_task_id();
    // SAFETY: as above.
    unsafe { (*pt).tid = tid };
    tid
}

/// The task id from the kernel (on the host, the test thread's stand-in).
fn raw_task_id() -> i32 {
    #[cfg(target_os = "none")]
    {
        syscall::syscall0(syscall::SYS_TASK_ID) as i32
    }
    #[cfg(not(target_os = "none"))]
    {
        crate::process::gettid()
    }
}

/// A recursive mutex's owner locking it again: one more level, or `EAGAIN`
/// once the count would overflow, as glibc answers.
fn recursive_relock(m: &PthreadMutexT) -> i32 {
    let c = m.count.load(Ordering::Relaxed);
    if c == i32::MAX {
        return errno::EAGAIN;
    }
    m.count.store(c.wrapping_add(1), Ordering::Relaxed);
    0
}

/// Whether the calling thread (`self_id`) holds `m`.
fn held_by(m: &PthreadMutexT, self_id: i32) -> bool {
    m.locked.load(Ordering::Relaxed) != 0 && m.owner.load(Ordering::Relaxed) == self_id
}

/// Lock a mutex.
///
/// The lock word is a futex ([`crate::lowlevellock`]): uncontended, this is
/// one compare-and-swap and no syscall; contended, the thread sleeps in the
/// kernel until the holder's unlock wakes it.  Until 2026-09-26 a contended
/// lock spun, then slept in 1 ms steps and polled.
///
/// - **Normal**: relocking by the owner deadlocks, as POSIX specifies.
/// - **Recursive**: if already held by calling thread, increments
///   recursion count and returns 0 (`EAGAIN` at the count's limit).
/// - **Error-checking**: if already held by calling thread, returns
///   EDEADLK without blocking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_lock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: caller guarantees mutex is valid.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);
    let self_id = current_tid();
    if kind != PTHREAD_MUTEX_NORMAL && held_by(m, self_id) {
        if kind == PTHREAD_MUTEX_RECURSIVE {
            return recursive_relock(m);
        }
        return errno::EDEADLK;
    }
    crate::lowlevellock::lll_lock(&m.locked);
    m.owner.store(self_id, Ordering::Relaxed);
    m.count.store(1, Ordering::Relaxed);
    0
}

/// Try to lock a mutex without blocking.
///
/// Returns 0 on success, `EBUSY` if the mutex is already locked (by
/// another thread, or -- for an error-checking mutex -- by this one).  A
/// recursive mutex the calling thread holds gains a level.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_trylock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: caller guarantees mutex is valid.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);
    let self_id = current_tid();
    if kind == PTHREAD_MUTEX_RECURSIVE && held_by(m, self_id) {
        return recursive_relock(m);
    }
    if crate::lowlevellock::lll_trylock(&m.locked) {
        m.owner.store(self_id, Ordering::Relaxed);
        m.count.store(1, Ordering::Relaxed);
        0
    } else {
        errno::EBUSY
    }
}

/// Unlock a mutex.
///
/// For recursive mutexes, decrements the recursion count; the mutex
/// is only released when the count reaches zero.  For error-checking
/// mutexes, returns EPERM if the calling thread does not own the lock.
/// Releasing a contended lock wakes one waiter.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_unlock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: caller guarantees mutex is valid.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);

    if kind == PTHREAD_MUTEX_RECURSIVE || kind == PTHREAD_MUTEX_ERRORCHECK {
        if !held_by(m, current_tid()) {
            // POSIX: EPERM for error-checking; UB for recursive.
            // We return EPERM for both to prevent silent corruption.
            return errno::EPERM;
        }
        if kind == PTHREAD_MUTEX_RECURSIVE {
            let c = m.count.load(Ordering::Relaxed);
            if c > 1 {
                // Still recursed — decrement count, keep lock held.
                m.count.store(c.wrapping_sub(1), Ordering::Relaxed);
                return 0;
            }
        }
    }

    m.owner.store(0, Ordering::Relaxed);
    m.count.store(0, Ordering::Relaxed);
    crate::lowlevellock::lll_unlock(&m.locked);
    0
}

/// Destroy a mutex.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_destroy(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: caller guarantees mutex is valid.
    unsafe {
        (*mutex).locked.store(0, Ordering::Release);
    }
    0
}

// ---------------------------------------------------------------------------
// Once control — thread-safe via atomics
// ---------------------------------------------------------------------------

/// Execute a function exactly once, even across multiple threads.
///
/// Uses a three-state atomic flag:
/// - 0: not started
/// - -1: initialization in progress (another thread is running `init`)
/// - 1: initialization complete
///
/// Threads that arrive while init is running spin-wait until complete.
///
/// A NULL `init` is called by glibc only when it is the one to run it, and
/// faults there; while another thread runs its own it waits, and once that is
/// done there is nothing to call.  So a NULL `init` returns 0 in those two
/// cases and is `EFAULT` in the first, leaving the once as it was
/// (design-decisions.md §1115).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_once(
    once: *mut PthreadOnceT,
    init: Option<extern "C" fn()>,
) -> i32 {
    if once.is_null() {
        return errno::EFAULT;
    }

    // SAFETY: caller guarantees once is valid.
    let done = unsafe { &(*once).done };

    // Fast path: already initialized.
    if done.load(Ordering::Acquire) == 1 {
        return 0;
    }

    let Some(init) = init else {
        loop {
            match done.load(Ordering::Acquire) {
                1 => return 0,
                0 => return errno::EFAULT,
                d => crate::lowlevellock::futex_wait(done, d),
            }
        }
    };

    // Try to claim the initialization.
    if done
        .compare_exchange(0, -1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        init();
        done.store(1, Ordering::Release);
        crate::lowlevellock::futex_wake_all(done);
    } else {
        // Another thread is initializing: sleep until it is done.  Until
        // 2026-09-26 this spun, for however long `init` took.
        loop {
            let d = done.load(Ordering::Acquire);
            if d == 1 {
                break;
            }
            crate::lowlevellock::futex_wait(done, d);
        }
    }

    0
}

// ---------------------------------------------------------------------------
// Thread-specific data
// ---------------------------------------------------------------------------
//
// glibc's design (nptl/pthread_key_create.c and its siblings): a process-wide
// table of keys, each with a sequence number that is odd while the key is in
// use, and a destructor; and each thread's values in its own storage, every
// value stamped with its key's number when it was set.  A key deleted and
// created again has a new number, so a value a thread set under the old key
// reads as NULL -- nobody has to visit every thread's storage.
// `pthread_getspecific` is a few loads and a compare, with no lock and no
// syscall.
//
// Until 2026-09-26 the values lived in one table of 64 rows keyed by task
// id, under a spin lock, with a `SYS_TASK_ID` syscall on every access; a 65th
// thread holding values got ENOMEM, and a deleted key's index was never
// reused (known-issues.md → TD-D-TSD-IS-A-GLOBAL-TABLE-KEYED-BY-TASK-ID).

/// Key type for thread-specific data.
pub type PthreadKeyT = u32;

/// Values per block of a thread's storage.  A thread's blocks are allocated
/// as it first sets a key in each.
const TSD_BLOCK_KEYS: usize = 32;

/// Keys a process may hold at once: musl's `PTHREAD_KEYS_MAX`, which is what
/// the C headers our programs compile against advertise, and what
/// `sysconf(_SC_THREAD_KEYS_MAX)` reports.
const KEYS_MAX: usize = crate::perthread::TSD_BLOCKS * TSD_BLOCK_KEYS;

const _: () = assert!(KEYS_MAX == 128);

/// POSIX `_POSIX_THREAD_DESTRUCTOR_ITERATIONS`: the number of times the
/// destructor sweep is repeated at thread exit so that destructors which
/// re-set a key (re-arming TSD) eventually drain.
const PTHREAD_DESTRUCTOR_ITERATIONS: usize = 4;

/// A key.  `seq` is odd while the key is in use and even while it is free;
/// every create and every delete advances it.
struct TsdKey {
    seq: AtomicU64,
    /// The destructor as an address, 0 for none.
    destructor: AtomicUsize,
}

impl TsdKey {
    /// A free key.  A `const fn`, not an associated `const`: every use of a
    /// `const` holding atomics is a fresh copy, which clippy rightly refuses.
    const fn free() -> Self {
        Self {
            seq: AtomicU64::new(0),
            destructor: AtomicUsize::new(0),
        }
    }
}

/// One thread's value for one key, stamped with the key's `seq` at the time
/// it was set.  All-zero is "no value".
#[repr(C)]
#[derive(Clone, Copy)]
struct TsdEntry {
    seq: u64,
    value: *mut u8,
}

/// The process's keys.
static TSD_KEYS: [TsdKey; KEYS_MAX] = [const { TsdKey::free() }; KEYS_MAX];

const fn key_in_use(seq: u64) -> bool {
    seq & 1 == 1
}

/// The calling thread's entry for `key` (which is `< KEYS_MAX`), allocating
/// its block if `create`; `None` if there is no block and `create` is false,
/// or the block cannot be allocated.
fn tsd_entry(key: usize, create: bool) -> Option<*mut TsdEntry> {
    let pt = crate::perthread::current();
    // SAFETY: `current()` is the calling thread's block, which only this
    // thread touches.
    let slot = unsafe { (*pt).tsd.get_mut(key / TSD_BLOCK_KEYS)? };
    if slot.is_null() {
        if !create {
            return None;
        }
        // Zeroed: every entry "no value".
        let block = crate::malloc::calloc(TSD_BLOCK_KEYS, size_of::<TsdEntry>());
        if block.is_null() {
            return None;
        }
        *slot = block;
    }
    // SAFETY: a block holds `TSD_BLOCK_KEYS` entries.
    Some(unsafe { slot.cast::<TsdEntry>().add(key % TSD_BLOCK_KEYS) })
}

/// Run the calling thread's key destructors and free its storage: glibc's
/// `__nptl_deallocate_tsd`.
///
/// Called from [`pthread_exit`] (and so from a start routine's return).  A
/// value is cleared before its destructor runs, and a sweep is repeated --
/// up to [`PTHREAD_DESTRUCTOR_ITERATIONS`] times -- only while destructors
/// set values again.  A value set under a key deleted since is dropped
/// without its destructor, as in glibc.
fn tsd_thread_cleanup() {
    let pt = crate::perthread::current();
    for _ in 0..PTHREAD_DESTRUCTOR_ITERATIONS {
        // SAFETY: the calling thread's block.
        let used = unsafe { core::mem::replace(&mut (*pt).tsd_used, false) };
        if !used {
            break;
        }
        for (key, k) in TSD_KEYS.iter().enumerate() {
            let Some(e) = tsd_entry(key, false) else {
                continue;
            };
            // SAFETY: `e` is this thread's entry.
            let (value, seq) = unsafe { ((*e).value, (*e).seq) };
            if value.is_null() {
                continue;
            }
            // SAFETY: as above.
            unsafe { (*e).value = core::ptr::null_mut() };
            let d = k.destructor.load(Ordering::Acquire);
            if seq == k.seq.load(Ordering::Acquire) && d != 0 {
                // SAFETY: a non-zero `destructor` was stored from a function
                // pointer of exactly this type by `pthread_key_create`.
                let f = unsafe { core::mem::transmute::<usize, extern "C" fn(*mut u8)>(d) };
                f(value);
            }
        }
    }
    // SAFETY: the calling thread's block; each slot is null or its `calloc`.
    unsafe {
        for slot in &mut (*pt).tsd {
            crate::malloc::free(*slot);
            *slot = core::ptr::null_mut();
        }
        (*pt).tsd_used = false;
    }
}

/// Create a thread-specific data key: the first free key, glibc's search.
/// `EAGAIN` when all [`KEYS_MAX`] are in use.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_key_create(
    key: *mut PthreadKeyT,
    destructor: Option<extern "C" fn(*mut u8)>,
) -> i32 {
    if key.is_null() {
        return errno::EFAULT;
    }
    for (i, k) in TSD_KEYS.iter().enumerate() {
        let seq = k.seq.load(Ordering::Relaxed);
        // Free, and not about to wrap (glibc's `KEY_USABLE`).
        if key_in_use(seq) || seq.checked_add(2).is_none() {
            continue;
        }
        if k.seq
            .compare_exchange(seq, seq | 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            k.destructor
                .store(destructor.map_or(0, |f| f as usize), Ordering::Release);
            // SAFETY: non-null; the caller's contract.  `i < KEYS_MAX`, so it
            // fits.
            unsafe { *key = i as PthreadKeyT };
            return 0;
        }
    }
    errno::EAGAIN
}

/// Get the calling thread's value for `key`: NULL for a key out of range, a
/// key it never set, or a value set under a key since deleted.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_getspecific(key: PthreadKeyT) -> *mut u8 {
    let Some(k) = TSD_KEYS.get(key as usize) else {
        return core::ptr::null_mut();
    };
    let Some(e) = tsd_entry(key as usize, false) else {
        return core::ptr::null_mut();
    };
    // SAFETY: `e` is the calling thread's entry.
    unsafe {
        let value = (*e).value;
        if !value.is_null() && (*e).seq != k.seq.load(Ordering::Acquire) {
            (*e).value = core::ptr::null_mut();
            return core::ptr::null_mut();
        }
        value
    }
}

/// Set the calling thread's value for `key`.  `EINVAL` for a key out of range
/// or not in use; `ENOMEM` if the thread's storage cannot grow.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_setspecific(key: PthreadKeyT, value: *mut u8) -> i32 {
    let Some(k) = TSD_KEYS.get(key as usize) else {
        return errno::EINVAL;
    };
    let seq = k.seq.load(Ordering::Acquire);
    if !key_in_use(seq) {
        return errno::EINVAL;
    }
    let Some(e) = tsd_entry(key as usize, true) else {
        return errno::ENOMEM;
    };
    // SAFETY: `e` and the per-thread block are the calling thread's.
    unsafe {
        *e = TsdEntry { seq, value };
        (*crate::perthread::current()).tsd_used = true;
    }
    0
}

/// Delete a thread-specific data key.
///
/// The key's number advances, so every thread's value for it reads as NULL
/// from now on and its destructor never runs for them -- POSIX leaves both to
/// the application.  A key not in use is `EINVAL`, as in glibc; until
/// 2026-09-26 any in-range key answered 0, and the index was never reused.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_key_delete(key: PthreadKeyT) -> i32 {
    let Some(k) = TSD_KEYS.get(key as usize) else {
        return errno::EINVAL;
    };
    let seq = k.seq.load(Ordering::Relaxed);
    if key_in_use(seq)
        && k.seq
            .compare_exchange(
                seq,
                seq.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_ok()
    {
        0
    } else {
        errno::EINVAL
    }
}

// ---------------------------------------------------------------------------
// Condition variables
// ---------------------------------------------------------------------------

/// Initialize a condition variable, with the clock its attribute names
/// (`CLOCK_REALTIME` without one).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_init(cond: *mut PthreadCondT, attr: *const PthreadCondattrT) -> i32 {
    if cond.is_null() {
        return errno::EFAULT;
    }
    let clock = if attr.is_null() {
        crate::time::CLOCK_REALTIME
    } else {
        // SAFETY: non-null; `[u8; 4]`, so read unaligned.  The clock sits
        // above the pshared bit (see `pthread_condattr_setclock`).
        (unsafe { core::ptr::read_unaligned(attr.cast::<i32>()) } >> 1) & 1
    };
    // SAFETY: cond is non-null.
    unsafe {
        let c = &mut *cond;
        c.generation = AtomicI32::new(0);
        c.clock = clock;
        c.waiters = AtomicI32::new(0);
    }
    0
}

/// Destroy a condition variable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_destroy(_cond: *mut PthreadCondT) -> i32 {
    0 // No resources to free.
}

/// Wait on a condition variable.
///
/// Atomically releases `mutex`, sleeps on the condition variable's futex
/// until a signal or broadcast advances it, then re-acquires `mutex`.
/// Until 2026-09-26 it slept in 1 ms steps and polled.  A `mutex` the caller
/// cannot unlock is its unlock's error, returned before any wait, as glibc
/// returns it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_wait(cond: *mut PthreadCondT, mutex: *mut PthreadMutexT) -> i32 {
    if cond.is_null() || mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers verified non-null.
    cond_wait_until(unsafe { &*cond }, mutex, None)
}

/// Wait on a condition variable with a timeout.
///
/// Like `pthread_cond_wait` but returns `ETIMEDOUT` if the absolute time
/// `abstime` -- on the clock the condition variable was made with -- passes
/// first.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_timedwait(
    cond: *mut PthreadCondT,
    mutex: *mut PthreadMutexT,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    // glibc's order (nptl/pthread_cond_wait.c:635): the deadline is read
    // first -- a NULL one is the first fault, a malformed `tv_nsec` EINVAL
    // with the mutex still held -- and only then the condition variable.
    // Until 2026-09-26 all three pointers were tested together, ahead of the
    // deadline.
    let Some(deadline) = read_deadline(abstime) else {
        return errno::EFAULT;
    };
    if !crate::time::valid_nanoseconds(deadline.tv_nsec) {
        return errno::EINVAL;
    }
    if cond.is_null() || mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: verified non-null.
    let c = unsafe { &*cond };
    cond_wait_until(c, mutex, Some((c.clock, deadline)))
}

/// `pthread_cond_clockwait` (glibc 2.30): [`pthread_cond_timedwait`] with the
/// deadline on `clockid` rather than the condition variable's own clock.
/// glibc reads the deadline, then judges the clock (`CLOCK_REALTIME` or
/// `CLOCK_MONOTONIC`), then the condition variable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_clockwait(
    cond: *mut PthreadCondT,
    mutex: *mut PthreadMutexT,
    clockid: i32,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    let Some(deadline) = read_deadline(abstime) else {
        return errno::EFAULT;
    };
    if !crate::time::valid_nanoseconds(deadline.tv_nsec) {
        return errno::EINVAL;
    }
    if !crate::lowlevellock::supported_clock(clockid) {
        return errno::EINVAL;
    }
    if cond.is_null() || mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: verified non-null.
    cond_wait_until(unsafe { &*cond }, mutex, Some((clockid, deadline)))
}

/// A deadline read from `abstime`, or `None` for NULL.
fn read_deadline(abstime: *const crate::stat::Timespec) -> Option<crate::stat::Timespec> {
    if abstime.is_null() {
        return None;
    }
    // SAFETY: non-null, and the caller's contract makes it a readable
    // `timespec`; read unaligned, as a C caller's may not be.
    Some(unsafe { core::ptr::read_unaligned(abstime) })
}

/// The wait: note the sequence number, release the mutex, sleep until the
/// number moves (or the deadline passes), take the mutex back.
///
/// The waiter count is advanced before the sequence number is read and the
/// signaller advances the number before it reads the count, all
/// sequentially consistent: so a signaller that sees no waiters signalled
/// before any waiter read the number, and that waiter does not sleep on the
/// old value.  Spurious wakes stay inside the loop.
fn cond_wait_until(
    c: &PthreadCondT,
    mutex: *mut PthreadMutexT,
    deadline: Option<(i32, crate::stat::Timespec)>,
) -> i32 {
    c.waiters.fetch_add(1, Ordering::SeqCst);
    let seq = c.generation.load(Ordering::SeqCst);
    // SAFETY: the caller checked `mutex` non-null.
    let err = unsafe { pthread_mutex_unlock(mutex) };
    if err != 0 {
        c.waiters.fetch_sub(1, Ordering::SeqCst);
        return err;
    }
    let mut timed_out = false;
    while c.generation.load(Ordering::Acquire) == seq {
        match deadline {
            None => crate::lowlevellock::futex_wait(&c.generation, seq),
            Some((clock, ref at)) => {
                match crate::lowlevellock::ns_until(&crate::lowlevellock::now_on(clock), at) {
                    None => {
                        timed_out = true;
                        break;
                    }
                    Some(ns) => crate::lowlevellock::futex_wait_timeout(&c.generation, seq, ns),
                }
            }
        }
    }
    c.waiters.fetch_sub(1, Ordering::SeqCst);
    // The caller held the mutex, so taking it back cannot fail but by
    // misuse the wait cannot report beyond what it returns.
    // SAFETY: as above.
    let _ = unsafe { pthread_mutex_lock(mutex) };
    if timed_out { errno::ETIMEDOUT } else { 0 }
}

/// Signal (wake one waiter on) a condition variable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_signal(cond: *mut PthreadCondT) -> i32 {
    if cond.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null.
    let c = unsafe { &*cond };
    c.generation.fetch_add(1, Ordering::SeqCst);
    if c.waiters.load(Ordering::SeqCst) > 0 {
        crate::lowlevellock::futex_wake(&c.generation, 1);
    }
    0
}

/// Broadcast (wake all waiters on) a condition variable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_broadcast(cond: *mut PthreadCondT) -> i32 {
    if cond.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null.
    let c = unsafe { &*cond };
    c.generation.fetch_add(1, Ordering::SeqCst);
    if c.waiters.load(Ordering::SeqCst) > 0 {
        crate::lowlevellock::futex_wake_all(&c.generation);
    }
    0
}

// ---------------------------------------------------------------------------
// Read-write locks
// ---------------------------------------------------------------------------

/// Pthread read-write lock type.
///
/// Uses an `AtomicI32` as a combined state:
/// - 0: unlocked
/// - positive N: N readers holding the lock
/// - -1: one writer holding the lock
///
/// Readers are preferred -- glibc's default -- unless the lock was made
/// with [`PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP`]: then, while a
/// writer waits, a new reader does not join the readers holding the lock
/// but waits with the writer, so that the read phase ends and the writer
/// gets its turn.  A reader asking again for a lock it holds would then wait
/// for itself, which is why glibc calls it non-recursive.
#[repr(C)]
pub struct PthreadRwlockT {
    /// 0 unlocked, N > 0 held by N readers, [`RWLOCK_WRITER`] held by a
    /// writer; also the futex waiters sleep on.
    state: AtomicI32,
    /// Threads asleep (or about to sleep) on `state`, so an unlock with no
    /// one waiting costs no syscall.
    waiters: AtomicI32,
    /// The writer's task id while `state` is [`RWLOCK_WRITER`], for
    /// `EDEADLK`.
    writer: AtomicI32,
    /// 1 if writers are preferred ([`PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP`]),
    /// else 0.  At byte 12 -- the fourth `int` of musl's `pthread_rwlock_t`
    /// -- where posix/include's `PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP`
    /// puts it.
    prefer_writer: AtomicI32,
    /// Writers waiting for a writer-preferring lock; the futex readers held
    /// back by them sleep on.  Kept only on such a lock.
    writers_waiting: AtomicI32,
    _pad: [u8; 36],
}

/// [`PthreadRwlockT::state`] while a writer holds the lock.
const RWLOCK_WRITER: i32 = -1;

/// See the module note on why these are `const` and not `#[test]`.
const _: () = {
    assert!(
        size_of::<PthreadRwlockT>() <= 56,
        "musl/glibc pthread_rwlock_t"
    );
    assert!(align_of::<PthreadRwlockT>() <= 8);
};

/// Pthread read-write lock attribute type.
pub type PthreadRwlockattrT = [u8; 8];

/// Static initializer for `pthread_rwlock_t`.
#[allow(clippy::declare_interior_mutable_const)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static PTHREAD_RWLOCK_INITIALIZER: PthreadRwlockT = PthreadRwlockT {
    state: AtomicI32::new(0),
    waiters: AtomicI32::new(0),
    writer: AtomicI32::new(0),
    prefer_writer: AtomicI32::new(0),
    writers_waiting: AtomicI32::new(0),
    _pad: [0; 36],
};

/// Readers preferred, glibc's default (`pthread_rwlockattr_setkind_np`): a
/// reader is not held back by a waiting writer, so a reader may lock again.
pub const PTHREAD_RWLOCK_PREFER_READER_NP: i32 = 0;
/// Writers preferred -- in name: glibc takes it as
/// [`PTHREAD_RWLOCK_PREFER_READER_NP`], since a reader locking again while a
/// writer waits would wait for itself, and so does this library.
pub const PTHREAD_RWLOCK_PREFER_WRITER_NP: i32 = 1;
/// Writers preferred, readers never locking again (see [`PthreadRwlockT`]).
pub const PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP: i32 = 2;
/// The default kind: [`PTHREAD_RWLOCK_PREFER_READER_NP`].
pub const PTHREAD_RWLOCK_DEFAULT_NP: i32 = PTHREAD_RWLOCK_PREFER_READER_NP;

/// Where a `pthread_rwlockattr_t` keeps its kind: the second `unsigned` of
/// musl's two (the first is the process-shared flag).
const RWLOCKATTR_OFF_KIND: usize = 4;

/// Initialize a read-write lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_init(
    rwlock: *mut PthreadRwlockT,
    attr: *const PthreadRwlockattrT,
) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
    }
    // Writers are preferred only for the non-recursive kind: glibc's
    // `__flags` is set for it alone (nptl/pthread_rwlock_init.c).
    // SAFETY: NULL or the caller's attribute object.
    let kind = unsafe { attr.as_ref() }.map_or(PTHREAD_RWLOCK_DEFAULT_NP, rwlockattr_kind);
    // SAFETY: non-null, the caller's lock, not in use (POSIX's contract).
    unsafe {
        (*rwlock).state = AtomicI32::new(0);
        (*rwlock).waiters = AtomicI32::new(0);
        (*rwlock).writer = AtomicI32::new(0);
        (*rwlock).prefer_writer = AtomicI32::new(i32::from(
            kind == PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP,
        ));
        (*rwlock).writers_waiting = AtomicI32::new(0);
    }
    0
}

/// The kind an attribute object holds.
fn rwlockattr_kind(attr: &PthreadRwlockattrT) -> i32 {
    attr.get(RWLOCKATTR_OFF_KIND..RWLOCKATTR_OFF_KIND + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(PTHREAD_RWLOCK_DEFAULT_NP, i32::from_ne_bytes)
}

/// Destroy a read-write lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_destroy(_rwlock: *mut PthreadRwlockT) -> i32 {
    0
}

/// Acquire a read lock (shared).
///
/// Taken at once unless a writer holds it; otherwise the thread sleeps on
/// the lock's futex until the writer leaves.  Readers are preferred, as by
/// glibc's default: a reader is not held back by a waiting writer.  A thread
/// holding the lock for writing is `EDEADLK`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_rdlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null.
    rdlock_until(unsafe { &*rwlock }, None)
}

/// Try to acquire a read lock without blocking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_tryrdlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
    }
    let rw = unsafe { &*rwlock };
    let current = rw.state.load(Ordering::Acquire);
    if current < 0 {
        return errno::EBUSY;
    }
    // A writer-preferring lock with a writer waiting is busy to a reader
    // (glibc's `pthread_rwlock_tryrdlock`).
    if rw.prefer_writer.load(Ordering::Relaxed) != 0
        && rw.writers_waiting.load(Ordering::Acquire) > 0
    {
        return errno::EBUSY;
    }
    if current == i32::MAX {
        return errno::EAGAIN;
    }
    if rw
        .state
        .compare_exchange(
            current,
            current.wrapping_add(1),
            Ordering::AcqRel,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        0
    } else {
        errno::EBUSY
    }
}

/// Acquire a write lock (exclusive).
///
/// Sleeps on the lock's futex until no reader or writer holds it.  A thread
/// already holding it for writing is `EDEADLK`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_wrlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null.
    wrlock_until(unsafe { &*rwlock }, None)
}

/// Try to acquire a write lock without blocking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_trywrlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
    }
    let rw = unsafe { &*rwlock };
    if rw
        .state
        .compare_exchange(0, RWLOCK_WRITER, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        rw.writer.store(current_tid(), Ordering::Relaxed);
        0
    } else {
        errno::EBUSY
    }
}

/// Release a read-write lock.
///
/// A writer's release, or the last reader's, wakes the threads asleep on
/// the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_unlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
    }
    let rw = unsafe { &*rwlock };
    let current = rw.state.load(Ordering::Acquire);
    let released = if current == RWLOCK_WRITER {
        rw.writer.store(0, Ordering::Relaxed);
        rw.state.store(0, Ordering::SeqCst);
        true
    } else if current > 0 {
        rw.state.fetch_sub(1, Ordering::SeqCst) == 1
    } else {
        // Not held: undefined behaviour in POSIX; glibc does not check.
        false
    };
    if released && rw.waiters.load(Ordering::SeqCst) > 0 {
        crate::lowlevellock::futex_wake_all(&rw.state);
    }
    0
}

/// `pthread_rwlock_timedrdlock`: [`pthread_rwlock_rdlock`] with a deadline
/// on `CLOCK_REALTIME`.  Missing until 2026-09-26, so a program using it did
/// not link.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_timedrdlock(
    rwlock: *mut PthreadRwlockT,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    pthread_rwlock_clockrdlock(rwlock, crate::time::CLOCK_REALTIME, abstime)
}

/// `pthread_rwlock_timedwrlock`: [`pthread_rwlock_wrlock`] with a deadline
/// on `CLOCK_REALTIME`.  Missing until 2026-09-26.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_timedwrlock(
    rwlock: *mut PthreadRwlockT,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    pthread_rwlock_clockwrlock(rwlock, crate::time::CLOCK_REALTIME, abstime)
}

/// `pthread_rwlock_clockrdlock` (glibc 2.30).  The deadline and clock are
/// judged first, eagerly -- glibc switched rwlocks from lazy to eager checks
/// (nptl/pthread_rwlock_common.c:286) -- then the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_clockrdlock(
    rwlock: *mut PthreadRwlockT,
    clockid: i32,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    match rwlock_deadline(clockid, abstime) {
        Ok(deadline) => {
            if rwlock.is_null() {
                return errno::EFAULT;
            }
            // SAFETY: non-null.
            rdlock_until(unsafe { &*rwlock }, Some(deadline))
        }
        Err(e) => e,
    }
}

/// `pthread_rwlock_clockwrlock` (glibc 2.30): as
/// [`pthread_rwlock_clockrdlock`], for writing.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_clockwrlock(
    rwlock: *mut PthreadRwlockT,
    clockid: i32,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    match rwlock_deadline(clockid, abstime) {
        Ok(deadline) => {
            if rwlock.is_null() {
                return errno::EFAULT;
            }
            // SAFETY: non-null.
            wrlock_until(unsafe { &*rwlock }, Some(deadline))
        }
        Err(e) => e,
    }
}

/// A timed rwlock's deadline, judged as glibc judges it: `EFAULT` for a NULL
/// one (the first fault), `EINVAL` for an unsupported clock or a malformed
/// `tv_nsec`.
fn rwlock_deadline(
    clockid: i32,
    abstime: *const crate::stat::Timespec,
) -> Result<(i32, crate::stat::Timespec), i32> {
    let Some(deadline) = read_deadline(abstime) else {
        return Err(errno::EFAULT);
    };
    if !crate::lowlevellock::supported_clock(clockid)
        || !crate::time::valid_nanoseconds(deadline.tv_nsec)
    {
        return Err(errno::EINVAL);
    }
    Ok((clockid, deadline))
}

/// Sleep on `rw.state` while it holds `seen`, or until the deadline; `false`
/// once the deadline has passed.
fn rwlock_sleep(
    rw: &PthreadRwlockT,
    seen: i32,
    deadline: Option<&(i32, crate::stat::Timespec)>,
) -> bool {
    rw.waiters.fetch_add(1, Ordering::SeqCst);
    let in_time = match deadline {
        None => {
            crate::lowlevellock::futex_wait(&rw.state, seen);
            true
        }
        Some((clock, at)) => {
            match crate::lowlevellock::ns_until(&crate::lowlevellock::now_on(*clock), at) {
                None => false,
                Some(ns) => {
                    crate::lowlevellock::futex_wait_timeout(&rw.state, seen, ns);
                    true
                }
            }
        }
    };
    rw.waiters.fetch_sub(1, Ordering::SeqCst);
    in_time
}

/// Sleep while `writers_waiting` is still `seen` -- a reader held back by a
/// writer on a writer-preferring lock -- or until `deadline`: `false` once it
/// has passed.  The writer wakes it when it stops waiting, with the lock or
/// without.
fn held_back_sleep(
    rw: &PthreadRwlockT,
    seen: i32,
    deadline: Option<&(i32, crate::stat::Timespec)>,
) -> bool {
    match deadline {
        None => {
            crate::lowlevellock::futex_wait(&rw.writers_waiting, seen);
            true
        }
        Some((clock, at)) => {
            match crate::lowlevellock::ns_until(&crate::lowlevellock::now_on(*clock), at) {
                None => false,
                Some(ns) => {
                    crate::lowlevellock::futex_wait_timeout(&rw.writers_waiting, seen, ns);
                    true
                }
            }
        }
    }
}

/// The read lock, with an optional deadline.
fn rdlock_until(rw: &PthreadRwlockT, deadline: Option<(i32, crate::stat::Timespec)>) -> i32 {
    if rw.state.load(Ordering::Relaxed) == RWLOCK_WRITER
        && rw.writer.load(Ordering::Relaxed) == current_tid()
    {
        return errno::EDEADLK;
    }
    let prefer_writer = rw.prefer_writer.load(Ordering::Relaxed) != 0;
    loop {
        let s = rw.state.load(Ordering::Acquire);
        if s >= 0 {
            // Readers hold it and a writer waits: on a writer-preferring
            // lock, wait with the writer rather than extend the read phase.
            // With no reader holding it, race the writer for it, as glibc's
            // does -- one of the two was going to win anyway.
            if prefer_writer && s > 0 {
                let w = rw.writers_waiting.load(Ordering::Acquire);
                if w > 0 {
                    if !held_back_sleep(rw, w, deadline.as_ref()) {
                        return errno::ETIMEDOUT;
                    }
                    continue;
                }
            }
            if s == i32::MAX {
                return errno::EAGAIN;
            }
            if rw
                .state
                .compare_exchange_weak(s, s.wrapping_add(1), Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return 0;
            }
            continue;
        }
        if !rwlock_sleep(rw, s, deadline.as_ref()) {
            return errno::ETIMEDOUT;
        }
    }
}

/// The write lock, with an optional deadline.
///
/// On a writer-preferring lock a writer that has to wait says so in
/// `writers_waiting`, which holds new readers back, and on leaving it -- with
/// the lock or without -- wakes the readers it held back to look again.
fn wrlock_until(rw: &PthreadRwlockT, deadline: Option<(i32, crate::stat::Timespec)>) -> i32 {
    let self_id = current_tid();
    if rw.state.load(Ordering::Relaxed) == RWLOCK_WRITER
        && rw.writer.load(Ordering::Relaxed) == self_id
    {
        return errno::EDEADLK;
    }
    let prefer_writer = rw.prefer_writer.load(Ordering::Relaxed) != 0;
    let mut waiting = false;
    let answer = loop {
        if rw
            .state
            .compare_exchange_weak(0, RWLOCK_WRITER, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            rw.writer.store(self_id, Ordering::Relaxed);
            break 0;
        }
        let s = rw.state.load(Ordering::Acquire);
        if s == 0 {
            continue;
        }
        if prefer_writer && !waiting {
            // Counted before the sleep, and the lock looked at again after,
            // so that no reader can join in between unseen.
            rw.writers_waiting.fetch_add(1, Ordering::SeqCst);
            waiting = true;
            continue;
        }
        if !rwlock_sleep(rw, s, deadline.as_ref()) {
            break errno::ETIMEDOUT;
        }
    };
    if waiting {
        rw.writers_waiting.fetch_sub(1, Ordering::SeqCst);
        crate::lowlevellock::futex_wake_all(&rw.writers_waiting);
    }
    answer
}

// ---------------------------------------------------------------------------
// sched_yield — voluntarily yield the CPU
// ---------------------------------------------------------------------------

/// Yield the processor to another thread/process.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sched_yield() -> i32 {
    let _ = syscall::syscall1(syscall::SYS_SLEEP, 0);
    0
}

// ---------------------------------------------------------------------------
// Thread attributes
// ---------------------------------------------------------------------------

// `PthreadAttrT` is an opaque `[u8; 56]` byte buffer.  We carve it into
// fixed fields written/read with unaligned accessors (the buffer has
// align(1)):
//
//   [ 0.. 8)  stack size   (usize)
//   [ 8..12)  detach state (i32: 0 = joinable, 1 = detached)
//   [16..24)  stack address — the stack's top, one past its highest byte
//             (usize), as glibc's `stackaddr` is: `pthread_attr_setstack`
//             stores its address plus its size, `pthread_attr_setstackaddr`
//             its argument, and the lowest address is the top less the
//             stack size wherever it is needed -- so a size set afterwards
//             moves the bottom, not the top, as in glibc.  0 for none.
//   [24..32)  guard size    (usize)
//   [32..36)  inherit-scheduler (i32: PTHREAD_INHERIT_SCHED or _EXPLICIT_)
//   [36..40)  scheduling policy (i32: SCHED_*)
//   [40..44)  scheduling priority (i32)
//   [44..48)  contention scope (i32: PTHREAD_SCOPE_SYSTEM)
//   [48..56)  extension — a `malloc`ed [`AttrExt`], or NULL: the CPU set
//             `pthread_attr_setaffinity_np` gives the new thread, which
//             `pthread_attr_destroy` frees (glibc's `extension`)
//
// Offsets 12..16 are reserved/unused.  These offsets are an
// internal contract only — C callers treat the type as opaque.  All-zero
// fields 32..48 are the defaults: inherit, `SCHED_OTHER`, priority 0, system
// scope -- so `pthread_attr_init`'s zeroing, and `encode_attr`'s, set them.
const ATTR_OFF_STACKSIZE: usize = 0;
const ATTR_OFF_DETACH: usize = 8;
const ATTR_OFF_STACKADDR: usize = 16;
const ATTR_OFF_GUARDSIZE: usize = 24;
const ATTR_OFF_INHERIT: usize = 32;
const ATTR_OFF_POLICY: usize = 36;
const ATTR_OFF_PRIORITY: usize = 40;
const ATTR_OFF_SCOPE: usize = 44;
const ATTR_OFF_EXT: usize = 48;

/// What an attribute object holds out of line (glibc's
/// `struct pthread_attr_extension`, less the signal mask this library cannot
/// give one thread alone -- signal masks are the process's here, see
/// `signal.rs`'s `pthread_sigmask`).
#[repr(C)]
struct AttrExt {
    /// The CPU set's bytes, `malloc`ed, or NULL for none.
    cpuset: *mut u8,
    /// How many.
    cpusetsize: usize,
}

/// The attribute object's extension, or NULL.
fn attr_read_ext(buf: &PthreadAttrT) -> *mut AttrExt {
    // SAFETY: reading 8 bytes at offset 48 ends at index 55 < 56.
    let v = unsafe { core::ptr::read_unaligned(buf.as_ptr().add(ATTR_OFF_EXT).cast::<usize>()) };
    core::ptr::with_exposed_provenance_mut(v)
}

/// Make `ext` the attribute object's extension.
fn attr_write_ext(buf: &mut PthreadAttrT, ext: *mut AttrExt) {
    // SAFETY: writing 8 bytes at offset 48 ends at index 55 < 56.
    unsafe {
        core::ptr::write_unaligned(
            buf.as_mut_ptr().add(ATTR_OFF_EXT).cast::<usize>(),
            ext.expose_provenance(),
        );
    }
}

/// The attribute object's CPU set, if it has one.
///
/// # Safety
///
/// `buf`'s extension, if any, is one this file made and has not freed.
unsafe fn attr_cpuset(buf: &PthreadAttrT) -> Option<&[u8]> {
    let ext = attr_read_ext(buf);
    if ext.is_null() {
        return None;
    }
    // SAFETY: the caller's contract; the set is `cpusetsize` bytes.
    unsafe {
        let e = &*ext;
        if e.cpuset.is_null() {
            None
        } else {
            Some(core::slice::from_raw_parts(e.cpuset, e.cpusetsize))
        }
    }
}

/// Give the attribute object `set` as its CPU set -- `ENOMEM` if there is
/// no memory for it, the object as it was -- or none, for an empty `set`.
///
/// # Safety
///
/// As [`attr_cpuset`]; `set` is not the object's own.
unsafe fn attr_set_cpuset(buf: &mut PthreadAttrT, set: &[u8]) -> Result<(), i32> {
    let mut ext = attr_read_ext(buf);
    if set.is_empty() {
        if !ext.is_null() {
            // SAFETY: the caller's contract.
            unsafe {
                crate::malloc::free((*ext).cpuset);
                (*ext).cpuset = core::ptr::null_mut();
                (*ext).cpusetsize = 0;
            }
        }
        return Ok(());
    }
    if ext.is_null() {
        ext = crate::malloc::calloc(1, core::mem::size_of::<AttrExt>()).cast::<AttrExt>();
        if ext.is_null() {
            return Err(errno::ENOMEM);
        }
        attr_write_ext(buf, ext);
    }
    // SAFETY: the caller's contract, or the zeroed extension just made
    // (a NULL set of 0 bytes).
    unsafe {
        if (*ext).cpusetsize != set.len() {
            let grown = crate::malloc::realloc((*ext).cpuset, set.len());
            if grown.is_null() {
                return Err(errno::ENOMEM);
            }
            (*ext).cpuset = grown;
            (*ext).cpusetsize = set.len();
        }
        core::ptr::copy_nonoverlapping(set.as_ptr(), (*ext).cpuset, set.len());
    }
    Ok(())
}

/// Free the attribute object's extension, and forget it.
///
/// # Safety
///
/// As [`attr_cpuset`].
unsafe fn attr_free_ext(buf: &mut PthreadAttrT) {
    let ext = attr_read_ext(buf);
    if !ext.is_null() {
        // SAFETY: the caller's contract.
        unsafe {
            crate::malloc::free((*ext).cpuset);
            crate::malloc::free(ext.cast());
        }
        attr_write_ext(buf, core::ptr::null_mut());
    }
}

/// A copy of the attribute object whose extension is its own: `ENOMEM` if
/// there is no memory for it (glibc's `__pthread_attr_copy`).
///
/// # Safety
///
/// As [`attr_cpuset`].
unsafe fn attr_deep_copy(buf: &PthreadAttrT) -> Result<PthreadAttrT, i32> {
    let mut copy = *buf;
    attr_write_ext(&mut copy, core::ptr::null_mut());
    // SAFETY: the caller's contract.
    if let Some(set) = unsafe { attr_cpuset(buf) } {
        // SAFETY: `copy` has no extension yet; `set` is `buf`'s.
        unsafe { attr_set_cpuset(&mut copy, set) }?;
    }
    Ok(copy)
}

/// Take the scheduling attributes from the creating thread (the default).
pub const PTHREAD_INHERIT_SCHED: i32 = 0;
/// Take them from the attribute object.
pub const PTHREAD_EXPLICIT_SCHED: i32 = 1;
/// Compete for the processor with every thread on the system (the only
/// scope Linux, and this system, has).
pub const PTHREAD_SCOPE_SYSTEM: i32 = 0;
/// Compete only within the process: not supported, as on Linux.
pub const PTHREAD_SCOPE_PROCESS: i32 = 1;

/// The `i32` field at `off` of an attribute object.
fn attr_read_i32(buf: &PthreadAttrT, off: usize) -> i32 {
    let bytes = buf
        .get(off..off.wrapping_add(4))
        .and_then(|b| <[u8; 4]>::try_from(b).ok());
    bytes.map_or(0, i32::from_ne_bytes)
}

/// Store `v` in the `i32` field at `off` of an attribute object.
fn attr_write_i32(buf: &mut PthreadAttrT, off: usize, v: i32) {
    if let Some(slot) = buf.get_mut(off..off.wrapping_add(4)) {
        slot.copy_from_slice(&v.to_ne_bytes());
    }
}

/// Default thread guard size: one page, as in glibc and musl.
///
/// `pthread_attr_init` records it and `pthread_create` maps it, inaccessible,
/// below every stack it makes; the main thread's kernel guard is the same
/// size.
const DEFAULT_GUARD_SIZE: usize = crate::unistd::PAGE_SIZE;

// Main-thread stack geometry.  These MUST stay in sync with the kernel's
// user-stack layout in `kernel/src/proc/spawn.rs`:
//   USER_STACK_TOP   = 0x0000_7FFF_FFFF_0000  (exclusive top)
//   MAX_STACK_FRAMES = 256 × 16 KiB = 4 MiB   (max on-demand growth)
//   USER_STACK_GUARD = USER_STACK_TOP - MAX_STACK_SIZE  (lowest usable)
//
// The main thread's stack grows on demand from 64 KiB up to 4 MiB; the
// kernel installs a hardware guard just below `MAIN_STACK_LOW`.  We report
// the full growable region so std places its overflow guard correctly.
#[cfg(any(target_os = "none", test))]
const MAIN_STACK_TOP: usize = 0x0000_7FFF_FFFF_0000;
#[cfg(any(target_os = "none", test))]
const MAIN_STACK_SIZE: usize = 256 * 16 * 1024; // 4 MiB
// Compile-time constant subtraction; cannot overflow (TOP > SIZE).
#[cfg(any(target_os = "none", test))]
#[allow(clippy::arithmetic_side_effects)]
const MAIN_STACK_LOW: usize = MAIN_STACK_TOP - MAIN_STACK_SIZE;

/// Resolved stack attributes for a thread.
#[cfg(any(target_os = "none", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct StackAttr {
    /// Lowest address of the stack region.
    addr: usize,
    /// Size of the stack region in bytes.
    size: usize,
    /// Guard size in bytes (0 if no guard page).
    guard: usize,
    /// Whether the thread is detached.
    detached: bool,
}

/// Compute the stack attributes for the main thread.
///
/// Pure function — depends only on the compile-time kernel layout
/// constants, so it is deterministic and host-testable.
#[cfg(any(target_os = "none", test))]
fn main_thread_stack_attr() -> StackAttr {
    StackAttr {
        addr: MAIN_STACK_LOW,
        size: MAIN_STACK_SIZE,
        guard: DEFAULT_GUARD_SIZE,
        detached: false,
    }
}

/// Encode resolved stack attributes into an opaque attribute buffer.
///
/// Pure with respect to its `&mut [u8; 56]` argument — no globals touched,
/// so it is fully host-testable.  Zeroes the buffer first so all reserved
/// fields are well-defined.  Uses unaligned writes because `PthreadAttrT`
/// is `[u8; 56]` (align(1)).
#[cfg(any(target_os = "none", test))]
fn encode_attr(buf: &mut PthreadAttrT, attr: StackAttr) {
    *buf = [0u8; 56];
    let p = buf.as_mut_ptr();
    // SAFETY: every field write of 8 bytes lands at an offset ≤ 24, so the
    // last byte touched is at index ≤ 31 — well within the 56-byte buffer.
    unsafe {
        core::ptr::write_unaligned(p.add(ATTR_OFF_STACKSIZE).cast::<usize>(), attr.size);
        core::ptr::write_unaligned(
            p.add(ATTR_OFF_DETACH).cast::<i32>(),
            i32::from(attr.detached),
        );
        core::ptr::write_unaligned(
            p.add(ATTR_OFF_STACKADDR).cast::<usize>(),
            attr.addr.wrapping_add(attr.size),
        );
        core::ptr::write_unaligned(p.add(ATTR_OFF_GUARDSIZE).cast::<usize>(), attr.guard);
    }
}

/// Read the stored stack size from an attribute buffer (0 = never set).
fn attr_read_stacksize(buf: &PthreadAttrT) -> usize {
    // SAFETY: reading 8 bytes at offset 0 ends at index 7 < 56.
    unsafe { core::ptr::read_unaligned(buf.as_ptr().add(ATTR_OFF_STACKSIZE).cast::<usize>()) }
}

/// Read the stored detach state from an attribute buffer.
fn attr_read_detachstate(buf: &PthreadAttrT) -> i32 {
    // SAFETY: reading 4 bytes at offset 8 ends at index 11 < 56.
    unsafe { core::ptr::read_unaligned(buf.as_ptr().add(ATTR_OFF_DETACH).cast::<i32>()) }
}

/// Read the stored stack top from an attribute buffer (0 for none).
fn attr_read_stackaddr(buf: &PthreadAttrT) -> usize {
    // SAFETY: reading 8 bytes at offset 16 ends at index 23 < 56.
    unsafe { core::ptr::read_unaligned(buf.as_ptr().add(ATTR_OFF_STACKADDR).cast::<usize>()) }
}

/// Read the stored guard size from an attribute buffer.
fn attr_read_guardsize(buf: &PthreadAttrT) -> usize {
    // SAFETY: reading 8 bytes at offset 24 ends at index 31 < 56.
    unsafe { core::ptr::read_unaligned(buf.as_ptr().add(ATTR_OFF_GUARDSIZE).cast::<usize>()) }
}

/// Resolve a thread's stack attributes by kernel task ID.
///
/// If the thread was created via `pthread_create` it is found in the
/// thread table and its stack bounds and guard are returned.  Otherwise the
/// thread is assumed to be the main thread and the kernel main-stack
/// geometry is reported.
#[cfg(target_os = "none")]
fn resolve_thread_stack_attr(task_id: u64) -> StackAttr {
    if let Some(info) = find_thread_info(task_id) {
        StackAttr {
            addr: info.stack_base,
            size: info.stack_size,
            guard: info.guard_size,
            detached: info.detached,
        }
    } else {
        main_thread_stack_attr()
    }
}

/// Initialize a thread attribute object to default values.
///
/// Defaults: joinable (not detached), stack size =
/// `DEFAULT_THREAD_STACK_SIZE`, guard = one page (`DEFAULT_GUARD_SIZE`, as
/// glibc's `__pthread_attr_init` records `__getpagesize ()`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_init(attr: *mut PthreadAttrT) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // Zero the entire attribute structure.
    // SAFETY: attr is non-null and is `[u8; 64]` — all-zero is a valid state.
    unsafe {
        core::ptr::write_bytes(attr.cast::<u8>(), 0, core::mem::size_of::<PthreadAttrT>());
    }
    // Store default stack size in bytes [0..8).
    // SAFETY: attr is non-null and 64 bytes; writing 8 bytes at offset 0 is safe.
    // Use write_unaligned because PthreadAttrT is a [u8; 64] with align(1).
    unsafe {
        core::ptr::write_unaligned(attr.cast::<usize>(), DEFAULT_THREAD_STACK_SIZE);
        core::ptr::write_unaligned(
            attr.cast::<u8>().add(ATTR_OFF_GUARDSIZE).cast::<usize>(),
            DEFAULT_GUARD_SIZE,
        );
    }
    0
}

/// Destroy a thread attribute object: its CPU set, if it has one, is
/// freed.  (Until 2026-09-30 an attribute object held nothing to free.)
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_destroy(attr: *mut PthreadAttrT) -> i32 {
    // SAFETY: NULL or the caller's attribute object, whose extension, if
    // any, `pthread_attr_setaffinity_np` or `pthread_getattr_np` made.
    if let Some(buf) = unsafe { attr.as_mut() } {
        // SAFETY: as above.
        unsafe { attr_free_ext(buf) };
    }
    0
}

/// Set the stack size in a thread attribute object.
///
/// A size below [`PTHREAD_STACK_MIN`] is `EINVAL`, and that verdict is
/// reached **before** `attr` is examined — glibc's
/// `__pthread_attr_setstacksize` (nptl/pthread_attr_setstacksize.c) opens with
/// `int ret = check_stacksize_attr (stacksize); if (ret) return ret;` and only
/// then dereferences the attribute, so on Linux a too-small size wins over a
/// bad pointer.  See `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setstacksize(attr: *mut PthreadAttrT, stacksize: usize) -> i32 {
    if stacksize < PTHREAD_STACK_MIN as usize {
        return errno::EINVAL;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    // Store stack size at bytes [0..8).
    // SAFETY: attr is non-null, 64 bytes — we only write 8 bytes.
    // Use write_unaligned because PthreadAttrT has align(1).
    unsafe {
        core::ptr::write_unaligned(attr.cast::<usize>(), stacksize);
    }
    0
}

/// Get the stack size from a thread attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getstacksize(
    attr: *const PthreadAttrT,
    stacksize: *mut usize,
) -> i32 {
    if attr.is_null() || stacksize.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers verified non-null.
    // Use read_unaligned because PthreadAttrT has align(1).
    unsafe {
        let stored = core::ptr::read_unaligned(attr.cast::<usize>());
        let sz = if stored == 0 {
            DEFAULT_THREAD_STACK_SIZE
        } else {
            stored
        };
        *stacksize = sz;
    }
    0
}

/// Detach-state constants for `pthread_attr_setdetachstate`.
pub const PTHREAD_CREATE_JOINABLE: i32 = 0;
pub const PTHREAD_CREATE_DETACHED: i32 = 1;

/// Set the detach state in a thread attribute object.
///
/// A state that is neither `PTHREAD_CREATE_JOINABLE` nor
/// `PTHREAD_CREATE_DETACHED` is `EINVAL`, decided before `attr` is examined —
/// glibc's `__pthread_attr_setdetachstate`
/// (nptl/pthread_attr_setdetachstate.c) opens with the `/* Catch invalid
/// values.  */` test and dereferences the attribute only afterwards.  See
/// `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setdetachstate(attr: *mut PthreadAttrT, detachstate: i32) -> i32 {
    if detachstate != PTHREAD_CREATE_JOINABLE && detachstate != PTHREAD_CREATE_DETACHED {
        return errno::EINVAL;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    // Store detach state at byte offset 8.
    // SAFETY: attr is non-null and 64 bytes.
    // Use write_unaligned because attr+8 may not be i32-aligned.
    unsafe {
        core::ptr::write_unaligned(attr.cast::<u8>().add(8).cast::<i32>(), detachstate);
    }
    0
}

/// Get the detach state from a thread attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getdetachstate(
    attr: *const PthreadAttrT,
    detachstate: *mut i32,
) -> i32 {
    if attr.is_null() || detachstate.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers verified non-null.
    // Use read_unaligned because attr+8 may not be i32-aligned.
    unsafe {
        *detachstate = core::ptr::read_unaligned(attr.cast::<u8>().add(8).cast::<i32>());
    }
    0
}

/// Get the stack address and size from a thread attribute object.
///
/// `*stackaddr` receives the lowest address of the stack region and
/// `*stacksize` its size in bytes.  Rust's std and glibc use this (after
/// `pthread_getattr_np`) to locate the stack for overflow-guard setup.
///
/// If the attribute has no recorded stack address (e.g. a default-init
/// attr), `*stackaddr` is set to null and `*stacksize` to the stored
/// (or default) stack size.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getstack(
    attr: *const PthreadAttrT,
    stackaddr: *mut *mut core::ffi::c_void,
    stacksize: *mut usize,
) -> i32 {
    if attr.is_null() || stackaddr.is_null() || stacksize.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: attr verified non-null; PthreadAttrT is [u8; 56].
    let buf = unsafe { &*attr };
    let top = attr_read_stackaddr(buf);
    // Same semantics as pthread_attr_getstacksize: a stored 0 means the
    // size was never set, so report the default.
    // SAFETY: attr non-null; reading 8 bytes at offset 0 is in-bounds.
    let stored = unsafe { core::ptr::read_unaligned(attr.cast::<usize>()) };
    let size = if stored == 0 {
        DEFAULT_THREAD_STACK_SIZE
    } else {
        stored
    };
    // The top less the size, as glibc's (0, none, stays NULL).
    let addr = if top == 0 { 0 } else { top.wrapping_sub(size) };
    // SAFETY: both out-pointers verified non-null above.
    unsafe {
        *stackaddr = addr as *mut core::ffi::c_void;
        *stacksize = size;
    }
    0
}

/// Set both the stack address and size in a thread attribute object.
///
/// `stackaddr` is the lowest address of the caller-provided stack region;
/// the object keeps its top, `stackaddr + stacksize`, as glibc's does -- a
/// region ending past the top of the address space is `EINVAL`.
///
/// As with [`pthread_attr_setstacksize`], the size is checked against
/// [`PTHREAD_STACK_MIN`] before `attr` is examined: glibc's
/// `__pthread_attr_setstack` (nptl/pthread_attr_setstack.c) shares the same
/// `check_stacksize_attr` prologue.  See `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setstack(
    attr: *mut PthreadAttrT,
    stackaddr: *mut core::ffi::c_void,
    stacksize: usize,
) -> i32 {
    if stacksize < PTHREAD_STACK_MIN as usize {
        return errno::EINVAL;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    let Some(top) = (stackaddr as usize).checked_add(stacksize) else {
        return errno::EINVAL;
    };
    let p = attr.cast::<u8>();
    // SAFETY: attr is non-null; writing 8 bytes at offsets 0 and 16 ends at
    // index ≤ 23 < 56.  Unaligned because PthreadAttrT has align(1).
    unsafe {
        core::ptr::write_unaligned(p.add(ATTR_OFF_STACKSIZE).cast::<usize>(), stacksize);
        core::ptr::write_unaligned(p.add(ATTR_OFF_STACKADDR).cast::<usize>(), top);
    }
    0
}

/// `pthread_attr_getstackaddr(attr, &addr)` (obsolete; POSIX dropped it in
/// 2008 for `pthread_attr_getstack`): the stack address the object holds --
/// its top, as `pthread_attr_setstackaddr` takes it and as glibc keeps it
/// (after `pthread_attr_setstack`, the address plus the size) -- or NULL for
/// none.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getstackaddr(
    attr: *const PthreadAttrT,
    stackaddr: *mut *mut core::ffi::c_void,
) -> i32 {
    if attr.is_null() || stackaddr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both non-null: the caller's attribute object and pointer.
    unsafe {
        *stackaddr = core::ptr::with_exposed_provenance_mut(attr_read_stackaddr(&*attr));
    }
    0
}

/// `pthread_attr_setstackaddr(attr, addr)` (obsolete): the new thread's
/// stack ends at `addr` -- its top, the stack growing down from it, as
/// glibc's header says of a stack that grows down -- and is the object's
/// stack size long.  The memory is the caller's; nothing is checked until
/// `pthread_create`, as in glibc.  NULL takes a stack address back.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setstackaddr(
    attr: *mut PthreadAttrT,
    stackaddr: *mut core::ffi::c_void,
) -> i32 {
    // SAFETY: NULL or the caller's attribute object.
    let Some(buf) = (unsafe { attr.as_mut() }) else {
        return errno::EFAULT;
    };
    if let Some(slot) = buf.get_mut(ATTR_OFF_STACKADDR..ATTR_OFF_STACKADDR + 8) {
        slot.copy_from_slice(&stackaddr.expose_provenance().to_ne_bytes());
    }
    0
}

/// `pthread_attr_setaffinity_np(attr, size, set)` (GNU): threads created
/// with the object run only on the CPUs in `set`'s first `size` bytes --
/// kept in the object (`ENOMEM` if there is no memory for them) until it is
/// destroyed.  A NULL `set` or a `size` of 0 takes a set back.
///
/// Every thread here runs on every CPU (see `pthread_setaffinity_np`), so a
/// set that leaves one out makes `pthread_create` fail (`ENOSYS`), as a
/// failed affinity makes glibc's; one with no CPU there is, `EINVAL`.
///
/// # Safety
///
/// `cpuset` is NULL or readable for `cpusetsize` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_attr_setaffinity_np(
    attr: *mut PthreadAttrT,
    cpusetsize: usize,
    cpuset: *const CpuSetT,
) -> i32 {
    // SAFETY: NULL or the caller's attribute object.
    let Some(buf) = (unsafe { attr.as_mut() }) else {
        return errno::EFAULT;
    };
    let set: &[u8] = if cpuset.is_null() || cpusetsize == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract.
        unsafe { core::slice::from_raw_parts(cpuset.cast::<u8>(), cpusetsize) }
    };
    // SAFETY: an initialised attribute object; `set` is the caller's.
    match unsafe { attr_set_cpuset(buf, set) } {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// `pthread_attr_getaffinity_np(attr, size, set)` (GNU): the object's CPU
/// set into `size` bytes at `set`, the bytes past its own zeroed -- `EINVAL`
/// if a CPU it holds does not fit -- or, for an object with none, every bit
/// of the `size` bytes set, as glibc answers "no information".
///
/// # Safety
///
/// `cpuset` is writable for `cpusetsize` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_attr_getaffinity_np(
    attr: *const PthreadAttrT,
    cpusetsize: usize,
    cpuset: *mut CpuSetT,
) -> i32 {
    // SAFETY: NULL or the caller's attribute object.
    let Some(buf) = (unsafe { attr.as_ref() }) else {
        return errno::EFAULT;
    };
    if cpuset.is_null() && cpusetsize > 0 {
        return errno::EFAULT;
    }
    let out = cpuset.cast::<u8>();
    // SAFETY: an initialised attribute object.
    match unsafe { attr_cpuset(buf) } {
        Some(set) => {
            if set.iter().skip(cpusetsize).any(|&b| b != 0) {
                return errno::EINVAL;
            }
            let n = set.len().min(cpusetsize);
            // SAFETY: `out` is writable for `cpusetsize >= n` bytes (the
            // caller's contract), and is not the object's own set.
            unsafe {
                core::ptr::copy_nonoverlapping(set.as_ptr(), out, n);
                core::ptr::write_bytes(out.add(n), 0, cpusetsize.wrapping_sub(n));
            }
        }
        // SAFETY: as above.
        None => unsafe { core::ptr::write_bytes(out, 0xff, cpusetsize) },
    }
    0
}

/// Get the guard size from a thread attribute object.
///
/// Returns the recorded guard size: one page from `pthread_attr_init`, or
/// whatever `pthread_attr_setguardsize` stored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getguardsize(
    attr: *const PthreadAttrT,
    guardsize: *mut usize,
) -> i32 {
    if attr.is_null() || guardsize.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers verified non-null; PthreadAttrT is [u8; 56].
    let buf = unsafe { &*attr };
    let g = attr_read_guardsize(buf);
    // SAFETY: guardsize verified non-null.
    unsafe {
        *guardsize = g;
    }
    0
}

/// Set the guard size in a thread attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setguardsize(attr: *mut PthreadAttrT, guardsize: usize) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: attr is non-null; writing 8 bytes at offset 24 ends at index
    // 31 < 56.  Unaligned because PthreadAttrT has align(1).
    unsafe {
        core::ptr::write_unaligned(
            attr.cast::<u8>().add(ATTR_OFF_GUARDSIZE).cast::<usize>(),
            guardsize,
        );
    }
    0
}

/// Fill a thread attribute object with the actual attributes of a running
/// thread (Linux-specific `_np` extension).
///
/// Rust's std and glibc call this to discover a thread's real stack bounds
/// for stack-overflow guard installation.  For threads created via
/// `pthread_create` the recorded stack region is reported; otherwise the
/// thread is treated as the main thread and the kernel main-stack geometry
/// (matching `kernel/src/proc/spawn.rs`) is reported.
///
/// Returns 0 on success, `EFAULT` if `attr` is null.
#[cfg(target_os = "none")]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_getattr_np(thread: PthreadT, attr: *mut PthreadAttrT) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    let resolved = resolve_thread_stack_attr(thread);
    // SAFETY: attr verified non-null; PthreadAttrT is [u8; 56].
    let buf = unsafe { &mut *attr };
    encode_attr(buf, resolved);
    attr_add_thread_affinity(buf, thread)
}

/// glibc's `pthread_getattr_np` tail: the thread's CPU set into the object,
/// asked for in 32 bytes and twice as many until it fits (`EINVAL` from
/// `pthread_getaffinity_np` for too few), up to 10 KiB.  0, or the error
/// that stopped it -- `ENOMEM`, or `ENOSYS` taken as "none to report".
#[cfg(any(target_os = "none", test))]
fn attr_add_thread_affinity(buf: &mut PthreadAttrT, thread: PthreadT) -> i32 {
    let mut size = 32usize;
    loop {
        let mut set = [0u8; 10 * 1024];
        let Some(room) = set.get_mut(..size) else {
            return errno::EINVAL;
        };
        match pthread_getaffinity_np(thread, size, room.as_mut_ptr().cast()) {
            0 => {
                // SAFETY: an object `encode_attr` just made (no extension).
                return match unsafe { attr_set_cpuset(buf, room) } {
                    Ok(()) => 0,
                    Err(e) => e,
                };
            }
            e if e == errno::EINVAL && size < 10 * 1024 => size = size.wrapping_mul(2),
            e if e == errno::ENOSYS => return 0,
            e => return e,
        }
    }
}

// ---------------------------------------------------------------------------
// Pthread barriers
// ---------------------------------------------------------------------------

/// Pthread barrier type.
///
/// Uses an atomic counter to track how many threads have arrived.
/// When the count reaches the threshold, all threads are released.
#[repr(C)]
pub struct PthreadBarrierT {
    /// Number of threads that must call `pthread_barrier_wait`.
    count: u32,
    /// Current number of waiting threads.
    current: AtomicI32,
    /// Generation counter — incremented when the barrier trips; also the
    /// futex waiters sleep on.
    generation: AtomicI32,
    /// A low-level lock over `current` and `generation`, so that an arrival
    /// is counted in exactly one round (see [`pthread_barrier_wait`]).
    lock: AtomicI32,
    /// Padding to reach glibc x86_64 size (32 bytes total).
    _pad: [u8; 16],
}

/// See the module note on why these are `const` and not `#[test]`.
const _: () = {
    assert!(
        size_of::<PthreadBarrierT>() <= 32,
        "musl/glibc pthread_barrier_t"
    );
    assert!(align_of::<PthreadBarrierT>() <= 8);
};

/// Pthread barrier attribute type (glibc x86_64: 4 bytes).
pub type PthreadBarrierattrT = [u8; 4];

/// Return value for the one thread designated as the "serial thread".
pub const PTHREAD_BARRIER_SERIAL_THREAD: i32 = -1;

/// `pthread_barrierattr_init`: private (nptl/pthread_barrierattr_init.c).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrierattr_init(attr: *mut PthreadBarrierattrT) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null and writable, per the contract.
    unsafe { core::ptr::write_unaligned(attr.cast::<i32>(), PTHREAD_PROCESS_PRIVATE) };
    0
}

/// `pthread_barrierattr_destroy`: nothing to do, as in glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrierattr_destroy(_attr: *mut PthreadBarrierattrT) -> i32 {
    0
}

/// `pthread_barrierattr_getpshared` (nptl/pthread_barrierattr_getpshared.c).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrierattr_getpshared(
    attr: *const PthreadBarrierattrT,
    pshared: *mut i32,
) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null; `[u8; 4]`, so read unaligned.
    let word = unsafe { core::ptr::read_unaligned(attr.cast::<i32>()) };
    put_i32(pshared, word)
}

/// `pthread_barrierattr_setpshared`: judged as the other `setpshared`s, so
/// `PTHREAD_PROCESS_SHARED` is `ENOTSUP`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrierattr_setpshared(
    attr: *mut PthreadBarrierattrT,
    pshared: i32,
) -> i32 {
    let err = futex_supports_pshared(pshared);
    if err != 0 {
        return err;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null and writable, per the contract.
    unsafe { core::ptr::write_unaligned(attr.cast::<i32>(), pshared) };
    0
}

/// Upper bound on a barrier's `count`, matching glibc's `BARRIER_IN_THRESHOLD`
/// (`UINT_MAX / 2`, sysdeps/nptl/internaltypes.h:119).  glibc reserves the top
/// half of the arrival counter's range for its reset protocol; we have the same
/// need for a different reason — our arrival counter is an `AtomicI32`, so a
/// `count` above `i32::MAX` could never be reached and the barrier would hang.
const BARRIER_IN_THRESHOLD: u32 = u32::MAX / 2;

/// Initialize a barrier.
///
/// `count` is the number of threads that must call `pthread_barrier_wait`
/// before any of them successfully return.  It must be greater than 0 and
/// below [`BARRIER_IN_THRESHOLD`]; both bounds are checked before `barrier` is
/// examined, because glibc's `___pthread_barrier_init`
/// (nptl/pthread_barrier_init.c) opens with `if (__glibc_unlikely (count == 0
/// || count >= BARRIER_IN_THRESHOLD)) return EINVAL;` and casts the barrier
/// only afterwards.  See `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrier_init(
    barrier: *mut PthreadBarrierT,
    attr: *const PthreadBarrierattrT,
    count: u32,
) -> i32 {
    if count == 0 || count >= BARRIER_IN_THRESHOLD {
        return errno::EINVAL;
    }
    // glibc's next check: an attribute holding neither sharing value is
    // EINVAL (nptl/pthread_barrier_init.c).  Shared cannot be stored here.
    if !attr.is_null() {
        // SAFETY: non-null; `[u8; 4]`, so read unaligned.
        let pshared = unsafe { core::ptr::read_unaligned(attr.cast::<i32>()) };
        if pshared != PTHREAD_PROCESS_PRIVATE && pshared != PTHREAD_PROCESS_SHARED {
            return errno::EINVAL;
        }
    }
    if barrier.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: barrier is non-null.
    unsafe {
        (*barrier).count = count;
        (*barrier).current = AtomicI32::new(0);
        (*barrier).generation = AtomicI32::new(0);
        (*barrier).lock = AtomicI32::new(0);
    }
    0
}

/// Destroy a barrier.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrier_destroy(_barrier: *mut PthreadBarrierT) -> i32 {
    0
}

/// Wait at a barrier.
///
/// Blocks until `count` threads have called this function on the same
/// barrier.  Exactly one thread returns `PTHREAD_BARRIER_SERIAL_THREAD`;
/// all others return 0.
///
/// Arrivals are counted under the barrier's low-level lock, so each is
/// counted in exactly one round.  Until 2026-09-26 they were not: between
/// the last arrival's reset of the count and its advance of the generation,
/// a thread arriving for the next round read the old generation and was
/// released with this one.  Waiters sleep on the generation's futex, where
/// they polled in 1 ms steps.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrier_wait(barrier: *mut PthreadBarrierT) -> i32 {
    if barrier.is_null() {
        return errno::EFAULT;
    }

    // SAFETY: non-null, and the caller's contract makes it an initialised
    // barrier.
    let b = unsafe { &*barrier };
    crate::lowlevellock::lll_lock(&b.lock);
    let round = b.generation.load(Ordering::Relaxed);
    let arrived = b.current.load(Ordering::Relaxed).wrapping_add(1);
    if arrived as u32 == b.count {
        // The last arrival: reset for the next round, advance, release.
        b.current.store(0, Ordering::Relaxed);
        b.generation.store(round.wrapping_add(1), Ordering::Release);
        crate::lowlevellock::lll_unlock(&b.lock);
        crate::lowlevellock::futex_wake_all(&b.generation);
        return PTHREAD_BARRIER_SERIAL_THREAD;
    }
    b.current.store(arrived, Ordering::Relaxed);
    crate::lowlevellock::lll_unlock(&b.lock);

    while b.generation.load(Ordering::Acquire) == round {
        crate::lowlevellock::futex_wait(&b.generation, round);
    }
    0
}

// ---------------------------------------------------------------------------
// Pthread spinlocks
// ---------------------------------------------------------------------------

/// Pthread spinlock type.
pub type PthreadSpinlockT = AtomicI32;

/// Initialize a spinlock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_spin_init(lock: *mut PthreadSpinlockT, _pshared: i32) -> i32 {
    if lock.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: lock is non-null.
    unsafe {
        core::ptr::addr_of_mut!(*lock).write(AtomicI32::new(0));
    }
    0
}

/// Destroy a spinlock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_spin_destroy(_lock: *mut PthreadSpinlockT) -> i32 {
    0
}

/// Acquire a spinlock.
///
/// Busy-waits until the lock is acquired.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_spin_lock(lock: *mut PthreadSpinlockT) -> i32 {
    if lock.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: lock is non-null.
    let atomic = unsafe { &*lock };
    while atomic
        .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    0
}

/// Try to acquire a spinlock without blocking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_spin_trylock(lock: *mut PthreadSpinlockT) -> i32 {
    if lock.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: lock is non-null.
    let atomic = unsafe { &*lock };
    if atomic
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        0
    } else {
        errno::EBUSY
    }
}

/// Release a spinlock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_spin_unlock(lock: *mut PthreadSpinlockT) -> i32 {
    if lock.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: lock is non-null.
    let atomic = unsafe { &*lock };
    atomic.store(0, Ordering::Release);
    0
}

// ---------------------------------------------------------------------------
// Pthread cancel stubs
// ---------------------------------------------------------------------------
//
// Our OS doesn't support thread cancellation.  These stubs allow programs
// that call `pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &old)` at
// startup to link and run.

/// Cancel type: deferred (default).
pub const PTHREAD_CANCEL_DEFERRED: i32 = 0;
/// Cancel type: asynchronous.
pub const PTHREAD_CANCEL_ASYNCHRONOUS: i32 = 1;
/// Cancel state: enabled (default).
pub const PTHREAD_CANCEL_ENABLE: i32 = 0;
/// Cancel state: disabled.
pub const PTHREAD_CANCEL_DISABLE: i32 = 1;

// Cancellation state and type are attributes of a *thread* under POSIX:
// every one of these functions is specified in terms of "the calling
// thread", and a new thread starts enabled and deferred.  They live in
// the per-thread block (`crate::perthread`) accordingly.
//
// They were process-global atomics until a flake hunt caught
// `test_setcanceltype_null_oldtype_succeeds` reading a value another
// test thread had just set.  The test-only `Mutex` and reset helper that
// had been added to contain that were treating the symptom: the same
// sharing would have been a plain conformance bug on the target the
// moment a program called `pthread_create`, with no test in sight.
//
// POSIX's initial values are both zero, which is what lets these ride in
// a block whose whole contract is that all-zero is the valid initial
// state.  Assert it rather than trusting the constants to stay put.
const _: () = assert!(PTHREAD_CANCEL_ENABLE == 0 && PTHREAD_CANCEL_DEFERRED == 0);

/// Inspect the calling thread's cancellation state (test/debug helper).
#[must_use]
pub fn current_cancel_state() -> i32 {
    // SAFETY: `current()` is this thread's block, and the reference is
    // not held across a call that could hand out another.
    unsafe { (*crate::perthread::current()).cancel_state }
}

/// Inspect the calling thread's cancellation type (test/debug helper).
#[must_use]
pub fn current_cancel_type() -> i32 {
    // SAFETY: as in `current_cancel_state`.
    unsafe { (*crate::perthread::current()).cancel_type }
}

/// Set the calling thread's cancellation state.
///
/// POSIX:
///
/// > Legal values for state are PTHREAD_CANCEL_ENABLE and
/// > PTHREAD_CANCEL_DISABLE.  ...  If pthread_setcancelstate() is
/// > given an invalid third [sic — `state`] argument, it shall
/// > return [EINVAL] without changing the state of the cancelability
/// > state.
///
/// On success we swap the new value into the calling thread's block
/// and write the previous value to `*oldstate` (if non-null), so
/// that a save-restore idiom like
///
/// ```c
/// int old;
/// pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &old);
/// /* critical region */
/// pthread_setcancelstate(old, NULL);
/// ```
///
/// works correctly across nested calls.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_setcancelstate(state: i32, oldstate: *mut i32) -> i32 {
    // Validate first — POSIX requires that an invalid value leaves
    // the cancellation state unchanged.
    if state != PTHREAD_CANCEL_ENABLE && state != PTHREAD_CANCEL_DISABLE {
        return errno::EINVAL;
    }
    // SAFETY: `current()` is this thread's block; no other thread can
    // observe the swap, since the value is the calling thread's own.
    let prev = unsafe {
        let slot = &raw mut (*crate::perthread::current()).cancel_state;
        core::mem::replace(&mut *slot, state)
    };
    if !oldstate.is_null() {
        // SAFETY: caller guarantees oldstate is valid if non-null.
        unsafe {
            *oldstate = prev;
        }
    }
    0
}

/// Set the calling thread's cancellation type.
///
/// POSIX:
///
/// > Legal values for type are PTHREAD_CANCEL_DEFERRED and
/// > PTHREAD_CANCEL_ASYNCHRONOUS.  ...  If pthread_setcanceltype()
/// > is given an invalid first argument, it shall return [EINVAL]
/// > without changing the state of the cancelability type.
///
/// Same swap-and-report semantics as `pthread_setcancelstate`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_setcanceltype(cancel_type: i32, oldtype: *mut i32) -> i32 {
    if cancel_type != PTHREAD_CANCEL_DEFERRED && cancel_type != PTHREAD_CANCEL_ASYNCHRONOUS {
        return errno::EINVAL;
    }
    // SAFETY: as in `pthread_setcancelstate`.
    let prev = unsafe {
        let slot = &raw mut (*crate::perthread::current()).cancel_type;
        core::mem::replace(&mut *slot, cancel_type)
    };
    if !oldtype.is_null() {
        // SAFETY: caller guarantees oldtype is valid if non-null.
        unsafe {
            *oldtype = prev;
        }
    }
    0
}

/// Create a cancellation point.
///
/// Stub: no-op (cancellation is not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_testcancel() {}

/// Cancel a thread.
///
/// Stub: returns ENOSYS (cancellation is not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cancel(_thread: PthreadT) -> i32 {
    errno::ENOSYS
}

// ---------------------------------------------------------------------------
// Mutex attributes
// ---------------------------------------------------------------------------

/// Mutex type: normal (default).
pub const PTHREAD_MUTEX_NORMAL: i32 = 0;
/// Mutex type: recursive.
pub const PTHREAD_MUTEX_RECURSIVE: i32 = 1;
/// Mutex type: error-checking.
pub const PTHREAD_MUTEX_ERRORCHECK: i32 = 2;
/// Mutex type: default (alias for normal).
pub const PTHREAD_MUTEX_DEFAULT: i32 = 0;

/// Process-shared attribute: private to the creating process.
pub const PTHREAD_PROCESS_PRIVATE: i32 = 0;
/// Process-shared attribute: shared between processes.
pub const PTHREAD_PROCESS_SHARED: i32 = 1;

/// Initialize a mutex attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_init(attr: *mut PthreadMutexattrT) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: attr is non-null.
    unsafe {
        core::ptr::write_bytes(
            attr.cast::<u8>(),
            0,
            core::mem::size_of::<PthreadMutexattrT>(),
        );
    }
    0
}

/// Destroy a mutex attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_destroy(_attr: *mut PthreadMutexattrT) -> i32 {
    0
}

/// Set the mutex type attribute.
///
/// Supported types: `PTHREAD_MUTEX_NORMAL` (default),
/// `PTHREAD_MUTEX_RECURSIVE`, `PTHREAD_MUTEX_ERRORCHECK`.
///
/// An out-of-range `kind` is `EINVAL` and outranks a bad `attr` — glibc's
/// `___pthread_mutexattr_settype` (nptl/pthread_mutexattr_settype.c) opens
/// with `if (kind < PTHREAD_MUTEX_NORMAL || kind > PTHREAD_MUTEX_ADAPTIVE_NP)
/// return EINVAL;` and casts `attr` only afterwards.  See
/// `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_settype(attr: *mut PthreadMutexattrT, kind: i32) -> i32 {
    if !(0..=2).contains(&kind) {
        return errno::EINVAL;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    // The type is the word's low bits; the flag bits are kept, as glibc's
    // `(iattr->mutexkind & PTHREAD_MUTEXATTR_FLAG_BITS) | kind` keeps them.
    update_mutexattr(attr, |w| (w & MUTEXATTR_FLAG_BITS) | kind as u32)
}

/// Get the mutex type attribute.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_gettype(attr: *const PthreadMutexattrT, kind: *mut i32) -> i32 {
    if attr.is_null() || kind.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers verified non-null.
    // Use read_unaligned because PthreadMutexattrT is [u8; 4] with align(1).
    unsafe {
        *kind = (core::ptr::read_unaligned(attr.cast::<u32>()) & !MUTEXATTR_FLAG_BITS) as i32;
    }
    0
}

// ---------------------------------------------------------------------------
// Mutex attribute bits -- glibc's `struct pthread_mutexattr` layout
// ---------------------------------------------------------------------------
//
// The attribute is one 32-bit word, laid out as glibc lays out `mutexkind`
// (nptl/pthreadP.h): the type in the low bits, the priority ceiling in bits
// 12..23, the protocol in bits 28..29, and a flag each for robustness (bit
// 30) and process sharing (bit 31).  Until 2026-09-26 the type was the whole
// word, and none of the other attributes existed.

/// Protocol: no priority inheritance or protection.
pub const PTHREAD_PRIO_NONE: i32 = 0;
/// Protocol: priority inheritance.
pub const PTHREAD_PRIO_INHERIT: i32 = 1;
/// Protocol: priority protection (the ceiling).
pub const PTHREAD_PRIO_PROTECT: i32 = 2;
/// Robustness: a dead owner leaves the mutex locked.
pub const PTHREAD_MUTEX_STALLED: i32 = 0;
/// Robustness: a dead owner's mutex is handed on, `EOWNERDEAD`.
pub const PTHREAD_MUTEX_ROBUST: i32 = 1;

const MUTEXATTR_PROTOCOL_SHIFT: u32 = 28;
const MUTEXATTR_PROTOCOL_MASK: u32 = 0x3000_0000;
const MUTEXATTR_PRIO_CEILING_SHIFT: u32 = 12;
const MUTEXATTR_PRIO_CEILING_MASK: u32 = 0x00ff_f000;
const MUTEXATTR_FLAG_ROBUST: u32 = 0x4000_0000;
const MUTEXATTR_FLAG_PSHARED: u32 = 0x8000_0000;
/// Every bit that is not the type.
const MUTEXATTR_FLAG_BITS: u32 = 0xf000_0000 | MUTEXATTR_PRIO_CEILING_MASK;

/// glibc's `futex_supports_pshared` where shared futexes are unsupported
/// (sysdeps/nptl/futex-internal.h): 0 for private, `ENOTSUP` for shared,
/// `EINVAL` for anything else.  Ours are unsupported because the kernel keys
/// a futex by address space and virtual address, so a waiter in one process
/// is never woken from another (known-issues
/// `B-D-PROCESS-SHARED-SYNC-IS-SILENTLY-PRIVATE`).
fn futex_supports_pshared(pshared: i32) -> i32 {
    match pshared {
        PTHREAD_PROCESS_PRIVATE => 0,
        PTHREAD_PROCESS_SHARED => errno::ENOTSUP,
        _ => errno::EINVAL,
    }
}

/// The attribute word at `attr`, or `EFAULT` for NULL -- this libc's
/// substitute for glibc's fault on the dereference (design-decisions.md
/// §303).
fn mutexattr_word(attr: *const PthreadMutexattrT) -> Result<u32, i32> {
    if attr.is_null() {
        return Err(errno::EFAULT);
    }
    // SAFETY: non-null, and the caller's contract makes it a readable
    // attribute; `PthreadMutexattrT` is `[u8; 4]`, so read unaligned.
    Ok(unsafe { core::ptr::read_unaligned(attr.cast::<u32>()) })
}

/// Rewrite the attribute word at `attr` as `f` says; `EFAULT` for NULL.
fn update_mutexattr(attr: *mut PthreadMutexattrT, f: impl FnOnce(u32) -> u32) -> i32 {
    match mutexattr_word(attr) {
        Ok(word) => {
            // SAFETY: as `mutexattr_word`; the attribute is also writable.
            unsafe { core::ptr::write_unaligned(attr.cast::<u32>(), f(word)) };
            0
        }
        Err(e) => e,
    }
}

/// Write `value` through `out`; `EFAULT` for NULL.
fn put_i32(out: *mut i32, value: i32) -> i32 {
    if out.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null, and the caller's contract makes it writable.
    unsafe { core::ptr::write_unaligned(out, value) };
    0
}

/// `pthread_mutexattr_getpshared` (nptl/pthread_mutexattr_getpshared.c).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_getpshared(
    attr: *const PthreadMutexattrT,
    pshared: *mut i32,
) -> i32 {
    match mutexattr_word(attr) {
        Ok(word) => put_i32(
            pshared,
            if word & MUTEXATTR_FLAG_PSHARED != 0 {
                PTHREAD_PROCESS_SHARED
            } else {
                PTHREAD_PROCESS_PRIVATE
            },
        ),
        Err(e) => e,
    }
}

/// `pthread_mutexattr_setpshared`: the value is judged first, as glibc
/// judges it (`futex_supports_pshared`), and `PTHREAD_PROCESS_SHARED` is
/// `ENOTSUP` -- see [`futex_supports_pshared`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_setpshared(attr: *mut PthreadMutexattrT, pshared: i32) -> i32 {
    let err = futex_supports_pshared(pshared);
    if err != 0 {
        return err;
    }
    update_mutexattr(attr, |w| {
        if pshared == PTHREAD_PROCESS_PRIVATE {
            w & !MUTEXATTR_FLAG_PSHARED
        } else {
            w | MUTEXATTR_FLAG_PSHARED
        }
    })
}

/// `pthread_mutexattr_getprotocol` (nptl/pthread_mutexattr_getprotocol.c).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_getprotocol(
    attr: *const PthreadMutexattrT,
    protocol: *mut i32,
) -> i32 {
    match mutexattr_word(attr) {
        Ok(word) => put_i32(
            protocol,
            ((word & MUTEXATTR_PROTOCOL_MASK) >> MUTEXATTR_PROTOCOL_SHIFT) as i32,
        ),
        Err(e) => e,
    }
}

/// `pthread_mutexattr_setprotocol`: any of the three protocols is stored,
/// as glibc stores it; the two this libc cannot provide are refused by
/// [`pthread_mutex_init`], as glibc refuses what it cannot provide.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_setprotocol(
    attr: *mut PthreadMutexattrT,
    protocol: i32,
) -> i32 {
    if !(PTHREAD_PRIO_NONE..=PTHREAD_PRIO_PROTECT).contains(&protocol) {
        return errno::EINVAL;
    }
    update_mutexattr(attr, |w| {
        (w & !MUTEXATTR_PROTOCOL_MASK) | ((protocol as u32) << MUTEXATTR_PROTOCOL_SHIFT)
    })
}

/// `pthread_mutexattr_getprioceiling`: a ceiling never set reads as the
/// lowest `SCHED_FIFO` priority, as glibc's does.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_getprioceiling(
    attr: *const PthreadMutexattrT,
    prioceiling: *mut i32,
) -> i32 {
    match mutexattr_word(attr) {
        Ok(word) => {
            let mut ceiling =
                ((word & MUTEXATTR_PRIO_CEILING_MASK) >> MUTEXATTR_PRIO_CEILING_SHIFT) as i32;
            if ceiling == 0 {
                ceiling = ceiling.max(crate::sched::sched_get_priority_min(
                    crate::sched::SCHED_FIFO,
                ));
            }
            put_i32(prioceiling, ceiling)
        }
        Err(e) => e,
    }
}

/// `pthread_mutexattr_setprioceiling`: a ceiling outside the `SCHED_FIFO`
/// priorities is `EINVAL` (nptl/pthread_mutexattr_setprioceiling.c).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_setprioceiling(
    attr: *mut PthreadMutexattrT,
    prioceiling: i32,
) -> i32 {
    let min = crate::sched::sched_get_priority_min(crate::sched::SCHED_FIFO);
    let max = crate::sched::sched_get_priority_max(crate::sched::SCHED_FIFO);
    let field = (MUTEXATTR_PRIO_CEILING_MASK >> MUTEXATTR_PRIO_CEILING_SHIFT) as i32;
    if prioceiling < min || prioceiling > max || prioceiling & field != prioceiling {
        return errno::EINVAL;
    }
    update_mutexattr(attr, |w| {
        (w & !MUTEXATTR_PRIO_CEILING_MASK) | ((prioceiling as u32) << MUTEXATTR_PRIO_CEILING_SHIFT)
    })
}

/// `pthread_mutexattr_getrobust` (nptl/pthread_mutexattr_getrobust.c).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_getrobust(
    attr: *const PthreadMutexattrT,
    robustness: *mut i32,
) -> i32 {
    match mutexattr_word(attr) {
        Ok(word) => put_i32(
            robustness,
            if word & MUTEXATTR_FLAG_ROBUST != 0 {
                PTHREAD_MUTEX_ROBUST
            } else {
                PTHREAD_MUTEX_STALLED
            },
        ),
        Err(e) => e,
    }
}

/// `pthread_mutexattr_setrobust`: stored as glibc stores it; a robust mutex
/// is refused by [`pthread_mutex_init`], this libc having no robust list.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_setrobust(
    attr: *mut PthreadMutexattrT,
    robustness: i32,
) -> i32 {
    if robustness != PTHREAD_MUTEX_STALLED && robustness != PTHREAD_MUTEX_ROBUST {
        return errno::EINVAL;
    }
    update_mutexattr(attr, |w| {
        if robustness == PTHREAD_MUTEX_STALLED {
            w & !MUTEXATTR_FLAG_ROBUST
        } else {
            w | MUTEXATTR_FLAG_ROBUST
        }
    })
}

/// `pthread_mutexattr_getrobust_np`, glibc's name before POSIX adopted it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_getrobust_np(
    attr: *const PthreadMutexattrT,
    robustness: *mut i32,
) -> i32 {
    pthread_mutexattr_getrobust(attr, robustness)
}

/// `pthread_mutexattr_setrobust_np`, glibc's name before POSIX adopted it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_setrobust_np(
    attr: *mut PthreadMutexattrT,
    robustness: i32,
) -> i32 {
    pthread_mutexattr_setrobust(attr, robustness)
}

/// `pthread_mutex_consistent`: `EINVAL` unless the mutex is robust and its
/// owner died -- which no mutex here can be, [`pthread_mutex_init`] refusing
/// robust ones (nptl/pthread_mutex_consistent.c).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutex_consistent(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    errno::EINVAL
}

/// `pthread_mutex_consistent_np`, glibc's older name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutex_consistent_np(mutex: *mut PthreadMutexT) -> i32 {
    pthread_mutex_consistent(mutex)
}

/// `pthread_mutex_getprioceiling`: `EINVAL` for a mutex without priority
/// protection (nptl/pthread_mutex_getprioceiling.c), which is every mutex
/// here.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutex_getprioceiling(
    mutex: *const PthreadMutexT,
    _prioceiling: *mut i32,
) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    errno::EINVAL
}

/// `pthread_mutex_setprioceiling`: as [`pthread_mutex_getprioceiling`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutex_setprioceiling(
    mutex: *mut PthreadMutexT,
    _prioceiling: i32,
    _old_ceiling: *mut i32,
) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    errno::EINVAL
}

// ---------------------------------------------------------------------------
// pthread_mutex_timedlock
// ---------------------------------------------------------------------------

/// Lock a mutex with a timeout.
///
/// Attempts to lock the mutex.  If the mutex is already locked, sleeps
/// until the mutex becomes available or the absolute timeout `abstime`
/// on `CLOCK_REALTIME` expires.
///
/// Returns 0 on success, ETIMEDOUT on timeout, EINVAL on error.
/// For recursive mutexes, succeeds immediately if already held by
/// calling thread.  For error-checking, returns EDEADLK.
///
/// # Safety
///
/// `mutex` must point to a valid initialized mutex.
/// `abstime` must point to a valid `timespec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutex_timedlock(
    mutex: *mut PthreadMutexT,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    mutex_lock_until(mutex, crate::time::CLOCK_REALTIME, abstime)
}

/// `pthread_mutex_clocklock` (glibc 2.30): [`pthread_mutex_timedlock`] with
/// the deadline on `clockid`, which must be `CLOCK_REALTIME` or
/// `CLOCK_MONOTONIC` -- judged first, before the mutex is looked at.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutex_clocklock(
    mutex: *mut PthreadMutexT,
    clockid: i32,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    if !crate::lowlevellock::supported_clock(clockid) {
        return errno::EINVAL;
    }
    mutex_lock_until(mutex, clockid, abstime)
}

/// The timed lock, with the deadline on `clock`.
fn mutex_lock_until(
    mutex: *mut PthreadMutexT,
    clock: i32,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: mutex verified non-null.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);
    let self_id = current_tid();

    // Recursive / error-checking: check if we already own the lock.
    if kind != PTHREAD_MUTEX_NORMAL && held_by(m, self_id) {
        if kind == PTHREAD_MUTEX_RECURSIVE {
            return recursive_relock(m);
        }
        return errno::EDEADLK;
    }

    // Fast path: try to acquire immediately.
    if crate::lowlevellock::lll_trylock(&m.locked) {
        m.owner.store(self_id, Ordering::Relaxed);
        m.count.store(1, Ordering::Relaxed);
        return 0;
    }

    // Only now — after the fast path has failed and we are committed to
    // blocking.  `pthread_mutex_timedlock.c:221` reads "We are about to
    // block; check whether the timeout is invalid", and the check sits
    // inside the contended branch, so an *uncontended* `timedlock` with a
    // malformed deadline succeeds and never looks at the timespec.  POSIX
    // permits exactly this: "the validity of the abstime parameter need not
    // be checked if the lock can be immediately acquired."  (So is a NULL
    // one: until 2026-09-26 it was EFAULT before the fast path.)
    //
    // The placement is deliberately different from `pthread_cond_timedwait`
    // and from `sem_timedwait`, both of which check eagerly — glibc
    // took the lazy option here and the eager one there, and
    // `pthread_rwlock_common.c:286-291` documents having *switched* from
    // lazy to eager for rwlocks.  Do not unify them.
    if abstime.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: abstime verified non-null.
    let deadline = unsafe { core::ptr::read_unaligned(abstime) };
    if !crate::time::valid_nanoseconds(deadline.tv_nsec) {
        return errno::EINVAL;
    }
    let taken = crate::lowlevellock::lll_timedlock(&m.locked, || {
        crate::lowlevellock::ns_until(&crate::lowlevellock::now_on(clock), &deadline)
    });
    if !taken {
        return errno::ETIMEDOUT;
    }
    m.owner.store(self_id, Ordering::Relaxed);
    m.count.store(1, Ordering::Relaxed);
    0
}

// ---------------------------------------------------------------------------
// Condition variable attributes
// ---------------------------------------------------------------------------

/// Initialize a condition variable attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_condattr_init(attr: *mut PthreadCondattrT) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: attr verified non-null; zeroing a [u8; 8] is safe.
    unsafe {
        core::ptr::write_bytes(
            attr.cast::<u8>(),
            0,
            core::mem::size_of::<PthreadCondattrT>(),
        );
    }
    0
}

/// Destroy a condition variable attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_condattr_destroy(_attr: *mut PthreadCondattrT) -> i32 {
    0
}

/// Set the clock for a condition variable attribute.
///
/// Stores the clock ID for use by `pthread_cond_timedwait`.
/// We accept any valid clock but our timedwait currently only uses
/// the real-time clock.
///
/// A clock other than `CLOCK_REALTIME`/`CLOCK_MONOTONIC` is `EINVAL`, and that
/// verdict precedes any look at `attr` — glibc's
/// `__pthread_condattr_setclock` (nptl/pthread_condattr_setclock.c) opens with
/// `/* Only a few clocks are allowed.  */` and casts `attr` only afterwards.
/// See `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_condattr_setclock(attr: *mut PthreadCondattrT, clock_id: i32) -> i32 {
    // Validate clock_id.
    if clock_id != crate::time::CLOCK_REALTIME && clock_id != crate::time::CLOCK_MONOTONIC {
        return errno::EINVAL;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    // glibc's layout (nptl/pthread_condattr_setclock.c): bit 0 is the
    // process-shared flag, the clock is above it.  Until 2026-09-26 the clock
    // was the whole word, which left no room for the flag.
    // SAFETY: non-null and writable, per the contract.
    unsafe {
        let word = core::ptr::read_unaligned(attr.cast::<i32>());
        core::ptr::write_unaligned(attr.cast::<i32>(), (word & 1) | (clock_id << 1));
    }
    0
}

/// Get the clock for a condition variable attribute.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_condattr_getclock(
    attr: *const PthreadCondattrT,
    clock_id: *mut i32,
) -> i32 {
    if attr.is_null() || clock_id.is_null() {
        return errno::EFAULT;
    }
    unsafe {
        *clock_id = (core::ptr::read_unaligned(attr.cast::<i32>()) >> 1) & 1;
    }
    0
}

/// `pthread_condattr_getpshared`: bit 0 of the attribute, as glibc keeps it
/// (nptl/pthread_condattr_getpshared.c).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_condattr_getpshared(
    attr: *const PthreadCondattrT,
    pshared: *mut i32,
) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null; `[u8; 4]`, so read unaligned.
    let word = unsafe { core::ptr::read_unaligned(attr.cast::<i32>()) };
    put_i32(pshared, word & 1)
}

/// `pthread_condattr_setpshared`: judged as [`pthread_mutexattr_setpshared`]
/// judges it, so `PTHREAD_PROCESS_SHARED` is `ENOTSUP`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_condattr_setpshared(attr: *mut PthreadCondattrT, pshared: i32) -> i32 {
    let err = futex_supports_pshared(pshared);
    if err != 0 {
        return err;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: non-null and writable, per the contract.
    unsafe {
        let word = core::ptr::read_unaligned(attr.cast::<i32>());
        core::ptr::write_unaligned(attr.cast::<i32>(), (word & !1) | pshared);
    }
    0
}

// ---------------------------------------------------------------------------
// Read-write lock attributes
// ---------------------------------------------------------------------------

/// Initialize a rwlock attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlockattr_init(attr: *mut PthreadRwlockattrT) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    unsafe {
        core::ptr::write_bytes(
            attr.cast::<u8>(),
            0,
            core::mem::size_of::<PthreadRwlockattrT>(),
        );
    }
    0
}

/// Destroy a rwlock attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlockattr_destroy(_attr: *mut PthreadRwlockattrT) -> i32 {
    0
}

/// Set the process-shared attribute for a rwlock.
///
/// We only support `PTHREAD_PROCESS_PRIVATE` (0); `PTHREAD_PROCESS_SHARED` is
/// `ENOTSUP` because we have no cross-process futex.  A value that is neither
/// is `EINVAL`, not `ENOTSUP` — glibc's `__pthread_rwlockattr_setpshared`
/// (nptl/pthread_rwlockattr_setpshared.c) delegates to
/// `futex_supports_pshared` (sysdeps/nptl/futex-internal.h:102), which accepts
/// both POSIX values and returns `EINVAL` for anything else.  Both verdicts are
/// reached before `attr` is cast, so an out-of-domain value outranks a bad
/// pointer.  See `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlockattr_setpshared(
    attr: *mut PthreadRwlockattrT,
    pshared: i32,
) -> i32 {
    if pshared != PTHREAD_PROCESS_PRIVATE && pshared != PTHREAD_PROCESS_SHARED {
        return errno::EINVAL;
    }
    if pshared == PTHREAD_PROCESS_SHARED {
        return errno::ENOTSUP;
    }
    if attr.is_null() {
        return errno::EFAULT;
    }
    unsafe {
        core::ptr::write_unaligned(attr.cast::<i32>(), pshared);
    }
    0
}

/// Get the process-shared attribute for a rwlock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlockattr_getpshared(
    attr: *const PthreadRwlockattrT,
    pshared: *mut i32,
) -> i32 {
    if attr.is_null() || pshared.is_null() {
        return errno::EFAULT;
    }
    unsafe {
        *pshared = core::ptr::read_unaligned(attr.cast::<i32>());
    }
    0
}

/// `pthread_rwlockattr_setkind_np(attr, pref)` (GNU): whom locks made with
/// the object prefer -- [`PTHREAD_RWLOCK_PREFER_READER_NP`] (the default),
/// [`PTHREAD_RWLOCK_PREFER_WRITER_NP`] (which glibc takes as the first), or
/// [`PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP`].  Any other is
/// `EINVAL`, judged before the object, as glibc judges it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlockattr_setkind_np(attr: *mut PthreadRwlockattrT, pref: i32) -> i32 {
    if !matches!(
        pref,
        PTHREAD_RWLOCK_PREFER_READER_NP
            | PTHREAD_RWLOCK_PREFER_WRITER_NP
            | PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP
    ) {
        return errno::EINVAL;
    }
    // SAFETY: NULL or the caller's attribute object.
    let Some(a) = (unsafe { attr.as_mut() }) else {
        return errno::EFAULT;
    };
    if let Some(slot) = a.get_mut(RWLOCKATTR_OFF_KIND..RWLOCKATTR_OFF_KIND + 4) {
        slot.copy_from_slice(&pref.to_ne_bytes());
    }
    0
}

/// `pthread_rwlockattr_getkind_np(attr, &pref)` (GNU): the kind the object
/// holds, [`PTHREAD_RWLOCK_PREFER_READER_NP`] until it is set.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlockattr_getkind_np(
    attr: *const PthreadRwlockattrT,
    pref: *mut i32,
) -> i32 {
    // SAFETY: NULL or the caller's attribute object and pointer.
    match unsafe { (attr.as_ref(), pref.as_mut()) } {
        (Some(a), Some(out)) => {
            *out = rwlockattr_kind(a);
            0
        }
        _ => errno::EFAULT,
    }
}

// ---------------------------------------------------------------------------
// pthread_setname_np / pthread_getname_np (GNU extensions)
// ---------------------------------------------------------------------------

/// Maximum thread name length (including null terminator).
/// Linux limit is 16 bytes.
const PTHREAD_NAME_MAX: usize = 16;

/// Maximum number of threads that can have a stored name simultaneously.
const MAX_NAMED_THREADS: usize = 64;

/// One thread's name, keyed by kernel task ID (`task_id == 0` ⇒ free).
///
/// Keying on the real task ID (rather than the old `tid % N` hash) means
/// two threads whose IDs collide modulo the table size no longer clobber
/// each other's names.  Slots are released at thread exit
/// (`thread_name_release`, called from `pthread_exit`).
#[derive(Clone, Copy)]
struct ThreadNameSlot {
    task_id: u64,
    name: [u8; PTHREAD_NAME_MAX],
}

impl ThreadNameSlot {
    const EMPTY: Self = Self {
        task_id: 0,
        name: [0u8; PTHREAD_NAME_MAX],
    };
}

static mut THREAD_NAMES: [ThreadNameSlot; MAX_NAMED_THREADS] =
    [ThreadNameSlot::EMPTY; MAX_NAMED_THREADS];

/// Guards [`THREAD_NAMES`] (names are set/read from arbitrary threads).
static THREAD_NAME_LOCK: AtomicBool = AtomicBool::new(false);

#[inline]
fn thread_name_lock() {
    while THREAD_NAME_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

#[inline]
fn thread_name_unlock() {
    THREAD_NAME_LOCK.store(false, Ordering::Release);
}

/// Find the table index for `task_id`, optionally allocating a free slot.
/// Caller must hold [`THREAD_NAME_LOCK`].
fn thread_name_index(task_id: u64, create: bool) -> Option<usize> {
    if task_id == 0 {
        return None;
    }
    // SAFETY: caller holds THREAD_NAME_LOCK.
    let table = unsafe { &mut *core::ptr::addr_of_mut!(THREAD_NAMES) };
    for (i, slot) in table.iter().enumerate() {
        if slot.task_id == task_id {
            return Some(i);
        }
    }
    if !create {
        return None;
    }
    for (i, slot) in table.iter_mut().enumerate() {
        if slot.task_id == 0 {
            *slot = ThreadNameSlot::EMPTY;
            slot.task_id = task_id;
            return Some(i);
        }
    }
    None
}

/// Release the calling thread's name slot at exit.
fn thread_name_release(task_id: u64) {
    if task_id == 0 {
        return;
    }
    thread_name_lock();
    if let Some(idx) = thread_name_index(task_id, false) {
        // SAFETY: lock held; idx in range.
        let table = unsafe { &mut *core::ptr::addr_of_mut!(THREAD_NAMES) };
        if let Some(slot) = table.get_mut(idx) {
            *slot = ThreadNameSlot::EMPTY;
        }
    }
    thread_name_unlock();
}

/// Set the name of a thread (GNU extension).
///
/// `name` must be a null-terminated string of at most 15 characters
/// (plus null).  Returns 0 on success, ERANGE if too long, ENOMEM if the
/// name table is full.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_setname_np(thread: PthreadT, name: *const u8) -> i32 {
    if name.is_null() {
        return errno::EFAULT;
    }

    let name_len = unsafe { crate::string::strlen(name) };
    if name_len >= PTHREAD_NAME_MAX {
        return errno::ERANGE;
    }

    thread_name_lock();
    let rc = match thread_name_index(thread, true) {
        Some(idx) => {
            // SAFETY: lock held; idx in range.
            let table = unsafe { &mut *core::ptr::addr_of_mut!(THREAD_NAMES) };
            if let Some(slot) = table.get_mut(idx) {
                slot.name = [0u8; PTHREAD_NAME_MAX];
                let mut i: usize = 0;
                while i < name_len {
                    if let Some(s) = slot.name.get_mut(i) {
                        // SAFETY: i < name_len ≤ strlen(name).
                        *s = unsafe { *name.add(i) };
                    }
                    i = i.wrapping_add(1);
                }
                0
            } else {
                errno::EINVAL
            }
        }
        // Table full.
        None => errno::ENOMEM,
    };
    thread_name_unlock();
    rc
}

/// Get the name of a thread (GNU extension).
///
/// Copies the thread name into `name` (at most `len` bytes including null).
/// A thread with no name set yields the empty string.
///
/// `len` must be at least [`PTHREAD_NAME_MAX`] (Linux's `TASK_COMM_LEN`, 16)
/// *whatever the name's actual length*, and that is the first thing checked —
/// glibc's `__pthread_getname_np` (nptl/pthread_getname.c) opens with `if (len
/// < TASK_COMM_LEN) return ERANGE;` and touches `buf` only afterwards, via
/// `prctl (PR_GET_NAME, buf)` or a read from `/proc/self/task/<tid>/comm`.  So
/// a short buffer outranks a null one, and a 4-byte buffer is `ERANGE` even
/// for a 2-character name.  See `design-decisions.md` §303.
///
/// # Safety
///
/// `name` must be valid for `len` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_getname_np(thread: PthreadT, name: *mut u8, len: usize) -> i32 {
    if len < PTHREAD_NAME_MAX {
        return errno::ERANGE;
    }
    if name.is_null() {
        return errno::EFAULT;
    }

    // Snapshot the name under the lock into a local buffer, then copy out.
    let mut local = [0u8; PTHREAD_NAME_MAX];
    let name_len = {
        thread_name_lock();
        let nl = match thread_name_index(thread, false) {
            Some(idx) => {
                // SAFETY: lock held; idx in range.
                let table = unsafe { &*core::ptr::addr_of!(THREAD_NAMES) };
                let mut l = 0usize;
                if let Some(slot) = table.get(idx) {
                    while l < PTHREAD_NAME_MAX {
                        let b = slot.name.get(l).copied().unwrap_or(0);
                        if b == 0 {
                            break;
                        }
                        if let Some(d) = local.get_mut(l) {
                            *d = b;
                        }
                        l = l.wrapping_add(1);
                    }
                }
                l
            }
            None => 0,
        };
        thread_name_unlock();
        nl
    };

    // No second `ERANGE` test is needed: a stored name is at most
    // `PTHREAD_NAME_MAX - 1` bytes, and `len >= PTHREAD_NAME_MAX` was checked
    // on entry, so the name plus its terminator always fits.  glibc has no
    // second test either — its only length check is the entry one.

    let mut i: usize = 0;
    while i < name_len {
        // SAFETY: i < name_len < PTHREAD_NAME_MAX <= len; name valid for len.
        unsafe {
            *name.add(i) = local.get(i).copied().unwrap_or(0);
        }
        i = i.wrapping_add(1);
    }
    // SAFETY: i == name_len < PTHREAD_NAME_MAX <= len.
    unsafe {
        *name.add(i) = 0;
    }

    0
}

// ---------------------------------------------------------------------------
// pthread_atfork
// ---------------------------------------------------------------------------

/// Maximum number of `pthread_atfork` handler triplets we can register.
///
/// glibc uses an unbounded linked list, but in a `no_std` staticlib we
/// avoid the allocator on this path and use a fixed table.  Real programs
/// register a handful of these (one per library that holds locks across
/// fork), so 32 is comfortably more than enough.
const MAX_ATFORK_HANDLERS: usize = 32;

/// One `pthread_atfork` registration: a `(prepare, parent, child)` triplet.
#[derive(Clone, Copy)]
struct AtforkEntry {
    prepare: Option<extern "C" fn()>,
    parent: Option<extern "C" fn()>,
    child: Option<extern "C" fn()>,
}

impl AtforkEntry {
    const EMPTY: Self = Self {
        prepare: None,
        parent: None,
        child: None,
    };
}

/// Registered atfork handlers, in registration order (index 0 first).
static mut ATFORK_HANDLERS: [AtforkEntry; MAX_ATFORK_HANDLERS] =
    [AtforkEntry::EMPTY; MAX_ATFORK_HANDLERS];
/// Number of valid entries in [`ATFORK_HANDLERS`].
static mut ATFORK_COUNT: usize = 0;
/// Spinlock guarding [`ATFORK_HANDLERS`] / [`ATFORK_COUNT`].
static ATFORK_LOCK: AtomicBool = AtomicBool::new(false);

/// Acquire [`ATFORK_LOCK`].
#[inline]
fn atfork_lock() {
    while ATFORK_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

/// Release [`ATFORK_LOCK`].
#[inline]
fn atfork_unlock() {
    ATFORK_LOCK.store(false, Ordering::Release);
}

/// Register handlers to be called around `fork()`.
///
/// Per POSIX, `prepare` is called in the parent before the fork (in LIFO
/// order of registration), `parent` is called in the parent after the
/// fork (FIFO order), and `child` is called in the child after the fork
/// (FIFO order).  Any handler may be `NULL`/`None` to skip that phase.
///
/// Returns 0 on success, or `ENOMEM` if the handler table is full.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_atfork(
    prepare: Option<extern "C" fn()>,
    parent: Option<extern "C" fn()>,
    child: Option<extern "C" fn()>,
) -> i32 {
    atfork_lock();
    let count = unsafe { ATFORK_COUNT };
    if count >= MAX_ATFORK_HANDLERS {
        atfork_unlock();
        return errno::ENOMEM;
    }
    // SAFETY: lock held, so no concurrent mutation; `count` is in range
    // (checked above).
    let table = unsafe { &mut *core::ptr::addr_of_mut!(ATFORK_HANDLERS) };
    if let Some(slot) = table.get_mut(count) {
        *slot = AtforkEntry {
            prepare,
            parent,
            child,
        };
        // SAFETY: lock held.
        unsafe {
            ATFORK_COUNT = count.wrapping_add(1);
        }
        atfork_unlock();
        0
    } else {
        atfork_unlock();
        errno::ENOMEM
    }
}

/// Snapshot the registered handlers into a caller-provided buffer.
///
/// Copies the function pointers out under the lock so the actual handler
/// calls happen with the lock released (a handler must not deadlock if it
/// touches pthread state, and could in principle call `pthread_atfork`).
/// Returns the number of entries copied.
fn atfork_snapshot(out: &mut [AtforkEntry; MAX_ATFORK_HANDLERS]) -> usize {
    atfork_lock();
    let count = unsafe { ATFORK_COUNT };
    // SAFETY: lock held; no concurrent mutation.
    let table = unsafe { &*core::ptr::addr_of!(ATFORK_HANDLERS) };
    for (dst, src) in out.iter_mut().zip(table.iter()).take(count) {
        *dst = *src;
    }
    atfork_unlock();
    count.min(MAX_ATFORK_HANDLERS)
}

/// Run the `prepare` handlers in LIFO order, in the parent before a fork.
///
/// Called by `fork()`/`vfork()` immediately before issuing the fork
/// syscall.
pub(crate) fn atfork_run_prepare() {
    let mut snap = [AtforkEntry::EMPTY; MAX_ATFORK_HANDLERS];
    let count = atfork_snapshot(&mut snap);
    // LIFO: last registered runs first.
    let mut i = count;
    while i > 0 {
        i = i.wrapping_sub(1);
        if let Some(f) = snap.get(i).and_then(|e| e.prepare) {
            f();
        }
    }
}

/// Run the `parent` handlers in FIFO order, in the parent after a fork.
///
/// Called by `fork()` in the parent (and, per glibc behaviour, also after
/// a failed fork so locks acquired by `prepare` handlers are released).
pub(crate) fn atfork_run_parent() {
    let mut snap = [AtforkEntry::EMPTY; MAX_ATFORK_HANDLERS];
    let count = atfork_snapshot(&mut snap);
    for e in snap.iter().take(count) {
        if let Some(f) = e.parent {
            f();
        }
    }
}

/// Run the `child` handlers in FIFO order, in the child after a fork.
///
/// Called by `fork()` in the child process.
pub(crate) fn atfork_run_child() {
    let mut snap = [AtforkEntry::EMPTY; MAX_ATFORK_HANDLERS];
    let count = atfork_snapshot(&mut snap);
    for e in snap.iter().take(count) {
        if let Some(f) = e.child {
            f();
        }
    }
}

// ---------------------------------------------------------------------------
// pthread_setaffinity_np / pthread_getaffinity_np — CPU affinity
// ---------------------------------------------------------------------------

/// Set the CPU affinity mask for a thread -- which, as for a process
/// ([`crate::sched::sched_setaffinity`]), can only be to the mask it has:
/// every online CPU succeeds, a narrower mask is `ENOSYS`, and one with no
/// online CPU is `EINVAL`.  Until 2026-09-27 any mask of a whole `cpu_set_t`
/// was accepted and ignored, and a shorter one refused.
///
/// Unlike the rest of this file, `EFAULT` here is the *kernel's* verdict rather
/// than a substitute for a glibc segfault: glibc's
/// `__pthread_setaffinity_new` (nptl/pthread_setaffinity.c) is nothing but
/// `INTERNAL_SYSCALL_CALL (sched_setaffinity, …)`, and Linux's
/// `get_user_cpu_mask` (kernel/sched/core.c:8429) does no size rejection at all
/// — a short `len` merely clears the mask — so `copy_from_user` reaches the
/// null pointer first and `EFAULT` outranks the length.  That is the opposite
/// order from [`pthread_getaffinity_np`].  See `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_setaffinity_np(
    _thread: PthreadT,
    cpusetsize: usize,
    cpuset: *const CpuSetT,
) -> i32 {
    let result = crate::sched::read_affinity_mask(cpusetsize, cpuset)
        .and_then(|mask| crate::sched::affinity_change(&mask, crate::sched::online_cpus()));
    match result {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// Get the CPU affinity mask for a thread: every online CPU, the mask every
/// thread has, written as [`crate::sched::sched_getaffinity`] writes it --
/// the CPUs there are, and zeroes to the end of `cpusetsize` (glibc's
/// `memset` after the kernel's copy).  Until 2026-09-27 it set all 1024 bits,
/// CPUs that do not exist included, and refused any mask shorter than a whole
/// `cpu_set_t`, though Linux takes `CPU_ALLOC_SIZE`'s 8 bytes.
///
/// Both length rejections precede the null check, because glibc's
/// `__pthread_getaffinity_np` (nptl/pthread_getaffinity.c) forwards straight to
/// `sched_getaffinity`, whose two `EINVAL` tests sit above the `copy_to_user`
/// that would fault (kernel/sched/core.c:8506-8509).  The second test — a
/// length that is not a whole number of `unsigned long`s — has no counterpart
/// in `sched_setaffinity`.  See `design-decisions.md` §303.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_getaffinity_np(
    _thread: PthreadT,
    cpusetsize: usize,
    cpuset: *mut CpuSetT,
) -> i32 {
    let ncpus = crate::sched::online_cpus();
    let result = crate::sched::affinity_len_ok(cpusetsize, ncpus)
        .and_then(|()| crate::sched::fill_affinity(cpusetsize, cpuset, ncpus));
    match result {
        Ok(()) => 0,
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// Per-thread scheduling parameters
// ---------------------------------------------------------------------------

/// Report a thread's scheduling policy and parameters.
///
/// **This reports the scheduler we have, which is not the one POSIX
/// describes.** Every thread runs under `SCHED_OTHER` at priority 0, because
/// that is what `sched_getscheduler` already reports for every process
/// (`sched.rs`) — our scheduler has no priority classes to distinguish. A
/// caller asking this question gets a true answer about this system rather
/// than a plausible answer about a system with priorities.
///
/// Returns 0, or `EFAULT` if either output pointer is null. POSIX's pthread
/// functions return the error number rather than setting `errno`, which is the
/// convention the rest of this file follows.
///
/// Measured need: one of the twenty undefined symbols in upstream CMake's link
/// against our libc — see `scripts/cmake-spike/README.md`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_getschedparam(
    _thread: PthreadT,
    policy: *mut i32,
    param: *mut crate::sched::SchedParam,
) -> i32 {
    if policy.is_null() || param.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: both pointers verified non-null above; `SchedParam` is repr(C)
    // and the caller owns storage for one.
    unsafe {
        *policy = crate::sched::SCHED_OTHER;
        *param = crate::sched::SchedParam::default();
    }
    0
}

/// Set a thread's scheduling policy and parameters.
///
/// Accepts `SCHED_OTHER` at priority 0 and **refuses everything else with
/// `EINVAL`**, rather than accepting a request it cannot carry out.
///
/// That choice is the point of this function. A silent success would tell a
/// caller its real-time thread is running at the priority it asked for, when
/// nothing in the scheduler distinguishes that thread from any other — and a
/// program that believes it has a priority it does not have makes worse
/// decisions than one that knows it cannot have one. `EINVAL` is a documented
/// answer to `pthread_setschedparam`; a false yes is not.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_setschedparam(
    _thread: PthreadT,
    policy: i32,
    param: *const crate::sched::SchedParam,
) -> i32 {
    if param.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: verified non-null above; `SchedParam` is repr(C).
    let requested = unsafe { (*param).sched_priority };
    if policy != crate::sched::SCHED_OTHER || requested != 0 {
        return errno::EINVAL;
    }
    0
}

/// Set a thread's priority within its policy: 0, the one priority
/// `SCHED_OTHER` has, and `EINVAL` for any other -- what glibc answers for a
/// `SCHED_OTHER` thread, which every thread here is.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_setschedprio(_thread: PthreadT, prio: i32) -> i32 {
    if prio == 0 { 0 } else { errno::EINVAL }
}

// ---------------------------------------------------------------------------
// Scheduling attributes
// ---------------------------------------------------------------------------
//
// Stored in the attribute object and validated as glibc validates them;
// honoured by `pthread_create` as far as the scheduler can -- which is
// `SCHED_OTHER` at priority 0, so an explicit request for anything else makes
// `pthread_create` fail with `EPERM` rather than start a thread without it.

/// The priority range glibc's `check_sched_priority_attr` allows `policy`:
/// `sched_get_priority_min`'s to `_max`'s.
fn priority_in_range(policy: i32, priority: i32) -> bool {
    let (lo, hi) = (
        crate::sched::sched_get_priority_min(policy),
        crate::sched::sched_get_priority_max(policy),
    );
    lo >= 0 && hi >= 0 && (lo..=hi).contains(&priority)
}

/// The policies an attribute may name: glibc's `check_sched_policy_attr`.
fn policy_allowed(policy: i32) -> bool {
    matches!(
        policy,
        crate::sched::SCHED_OTHER | crate::sched::SCHED_FIFO | crate::sched::SCHED_RR
    )
}

/// Whether `attr` takes its scheduling from itself or from the creator.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setinheritsched(attr: *mut PthreadAttrT, inherit: i32) -> i32 {
    // SAFETY: NULL or the caller's attribute object.
    let Some(buf) = (unsafe { attr.as_mut() }) else {
        return errno::EFAULT;
    };
    if inherit != PTHREAD_INHERIT_SCHED && inherit != PTHREAD_EXPLICIT_SCHED {
        return errno::EINVAL;
    }
    attr_write_i32(buf, ATTR_OFF_INHERIT, inherit);
    0
}

/// See [`pthread_attr_setinheritsched`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getinheritsched(
    attr: *const PthreadAttrT,
    inherit: *mut i32,
) -> i32 {
    // SAFETY: NULL or the caller's objects.
    let (Some(buf), Some(out)) = (unsafe { attr.as_ref() }, unsafe { inherit.as_mut() }) else {
        return errno::EFAULT;
    };
    *out = attr_read_i32(buf, ATTR_OFF_INHERIT);
    0
}

/// The scheduling policy an explicit-scheduling attribute asks for:
/// `SCHED_OTHER`, `SCHED_FIFO` or `SCHED_RR`, else `EINVAL`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setschedpolicy(attr: *mut PthreadAttrT, policy: i32) -> i32 {
    // SAFETY: NULL or the caller's attribute object.
    let Some(buf) = (unsafe { attr.as_mut() }) else {
        return errno::EFAULT;
    };
    if !policy_allowed(policy) {
        return errno::EINVAL;
    }
    attr_write_i32(buf, ATTR_OFF_POLICY, policy);
    0
}

/// See [`pthread_attr_setschedpolicy`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getschedpolicy(attr: *const PthreadAttrT, policy: *mut i32) -> i32 {
    // SAFETY: NULL or the caller's objects.
    let (Some(buf), Some(out)) = (unsafe { attr.as_ref() }, unsafe { policy.as_mut() }) else {
        return errno::EFAULT;
    };
    *out = attr_read_i32(buf, ATTR_OFF_POLICY);
    0
}

/// The priority an explicit-scheduling attribute asks for, which must lie in
/// the range of the attribute's policy (`EINVAL` otherwise, as glibc).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setschedparam(
    attr: *mut PthreadAttrT,
    param: *const crate::sched::SchedParam,
) -> i32 {
    // SAFETY: NULL or the caller's objects.
    let (Some(buf), Some(p)) = (unsafe { attr.as_mut() }, unsafe { param.as_ref() }) else {
        return errno::EFAULT;
    };
    if !priority_in_range(attr_read_i32(buf, ATTR_OFF_POLICY), p.sched_priority) {
        return errno::EINVAL;
    }
    attr_write_i32(buf, ATTR_OFF_PRIORITY, p.sched_priority);
    0
}

/// See [`pthread_attr_setschedparam`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getschedparam(
    attr: *const PthreadAttrT,
    param: *mut crate::sched::SchedParam,
) -> i32 {
    // SAFETY: NULL or the caller's objects.
    let (Some(buf), Some(out)) = (unsafe { attr.as_ref() }, unsafe { param.as_mut() }) else {
        return errno::EFAULT;
    };
    *out = crate::sched::SchedParam::default();
    out.sched_priority = attr_read_i32(buf, ATTR_OFF_PRIORITY);
    0
}

/// The contention scope: `PTHREAD_SCOPE_SYSTEM`; `PTHREAD_SCOPE_PROCESS` is
/// `ENOTSUP`, and anything else `EINVAL`, as on Linux.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setscope(attr: *mut PthreadAttrT, scope: i32) -> i32 {
    // SAFETY: NULL or the caller's attribute object.
    let Some(buf) = (unsafe { attr.as_mut() }) else {
        return errno::EFAULT;
    };
    match scope {
        PTHREAD_SCOPE_SYSTEM => {
            attr_write_i32(buf, ATTR_OFF_SCOPE, scope);
            0
        }
        PTHREAD_SCOPE_PROCESS => errno::ENOTSUP,
        _ => errno::EINVAL,
    }
}

/// See [`pthread_attr_setscope`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_getscope(attr: *const PthreadAttrT, scope: *mut i32) -> i32 {
    // SAFETY: NULL or the caller's objects.
    let (Some(buf), Some(out)) = (unsafe { attr.as_ref() }, unsafe { scope.as_mut() }) else {
        return errno::EFAULT;
    };
    *out = attr_read_i32(buf, ATTR_OFF_SCOPE);
    0
}

// ---------------------------------------------------------------------------
// The concurrency hint and the default attributes
// ---------------------------------------------------------------------------

crate::perprocess::process_global! {
    /// `pthread_setconcurrency`'s level: a hint no implementation with one
    /// kernel thread per pthread uses, kept only to be read back, as glibc
    /// and musl keep it.
    fn concurrency_level() -> core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

    /// The attributes `pthread_create` uses for a NULL attribute --
    /// `pthread_attr_init`'s until `pthread_setattr_default_np` changes
    /// them. Guarded by [`DEFAULT_ATTR_LOCK`].
    fn default_attr() -> PthreadAttrT = initial_default_attr();
}

/// Guards [`default_attr`], which any thread may set while another creates.
static DEFAULT_ATTR_LOCK: AtomicBool = AtomicBool::new(false);

/// What `pthread_attr_init` writes: the default stack and guard sizes, and
/// zero -- the default -- everywhere else.
// A `const fn`, for `process_global!`'s constant initialiser, where `get_mut`
// and checked arithmetic are not available: the indices are the two field
// offsets plus 0..8, all below 32 in a 56-byte array.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn initial_default_attr() -> PthreadAttrT {
    let mut a = [0u8; 56];
    let stack = DEFAULT_THREAD_STACK_SIZE.to_ne_bytes();
    let guard = DEFAULT_GUARD_SIZE.to_ne_bytes();
    let mut i = 0;
    while i < 8 {
        a[ATTR_OFF_STACKSIZE + i] = stack[i];
        a[ATTR_OFF_GUARDSIZE + i] = guard[i];
        i += 1;
    }
    a
}

/// Run `f` on the default attributes, holding [`DEFAULT_ATTR_LOCK`].
fn with_default_attr<R>(f: impl FnOnce(&mut PthreadAttrT) -> R) -> R {
    while DEFAULT_ATTR_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    // SAFETY: `default_attr()` is this process's storage, and the lock gives
    // this thread the only reference for the duration of `f`.
    let r = f(unsafe { &mut *default_attr() });
    DEFAULT_ATTR_LOCK.store(false, Ordering::Release);
    r
}

/// The concurrency level last set, 0 until then.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_getconcurrency() -> i32 {
    // SAFETY: this process's storage; an atomic.
    unsafe { (*concurrency_level()).load(Ordering::Relaxed) }
}

/// Record a concurrency level (a hint, unused); `EINVAL` if negative.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_setconcurrency(level: i32) -> i32 {
    if level < 0 {
        return errno::EINVAL;
    }
    // SAFETY: as in `pthread_getconcurrency`.
    unsafe { (*concurrency_level()).store(level, Ordering::Relaxed) };
    0
}

/// The attributes a NULL-attribute `pthread_create` uses, into `attr` (GNU).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_getattr_default_np(attr: *mut PthreadAttrT) -> i32 {
    // SAFETY: NULL or the caller's attribute object.
    let Some(out) = (unsafe { attr.as_mut() }) else {
        return errno::EFAULT;
    };
    // A copy with a CPU set of its own, as glibc's `__pthread_attr_copy`
    // makes it: `ENOMEM` if there is no memory for one.
    // SAFETY: the defaults' extension is live while they are locked.
    match with_default_attr(|d| unsafe { attr_deep_copy(d) }) {
        Ok(copy) => {
            *out = copy;
            0
        }
        Err(e) => e,
    }
}

/// Make `attr` the attributes a NULL-attribute `pthread_create` uses (GNU),
/// checked as glibc checks them: a policy `pthread_attr_setschedpolicy`
/// would take, a positive priority in its range, a stack size of 0 (keep
/// the current one) or at least `PTHREAD_STACK_MIN`, and no stack address --
/// a default stack would be every thread's stack. `EINVAL` otherwise.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_setattr_default_np(attr: *const PthreadAttrT) -> i32 {
    // SAFETY: NULL or the caller's attribute object.
    let Some(new) = (unsafe { attr.as_ref() }) else {
        return errno::EFAULT;
    };
    let policy = attr_read_i32(new, ATTR_OFF_POLICY);
    let priority = attr_read_i32(new, ATTR_OFF_PRIORITY);
    let size = attr_read_stacksize(new);
    if !policy_allowed(policy)
        || (priority > 0 && !priority_in_range(policy, priority))
        || (size != 0 && size < PTHREAD_STACK_MIN as usize)
        || attr_read_stackaddr(new) != 0
    {
        return errno::EINVAL;
    }
    // The defaults get a CPU set of their own (`ENOMEM` if there is no
    // memory for it), and the one they had is freed once they are replaced.
    // SAFETY: the caller's attribute object.
    let copy = match unsafe { attr_deep_copy(new) } {
        Ok(copy) => copy,
        Err(e) => return e,
    };
    let mut old = with_default_attr(|d| {
        let keep = attr_read_stacksize(d);
        let old = *d;
        *d = copy;
        if size == 0 {
            if let Some(slot) = d.get_mut(ATTR_OFF_STACKSIZE..ATTR_OFF_STACKSIZE + 8) {
                slot.copy_from_slice(&keep.to_ne_bytes());
            }
        }
        old
    });
    // SAFETY: the old defaults' extension, now no one's but this copy's.
    unsafe { attr_free_ext(&mut old) };
    0
}

// ---------------------------------------------------------------------------
// Cleanup handlers
// ---------------------------------------------------------------------------

/// musl's `struct __ptcb`: one cleanup handler. `pthread_cleanup_push(f, x)`
/// is a macro that declares one on the pushing function's stack and calls
/// [`_pthread_cleanup_push`]; `pthread_cleanup_pop(run)` calls
/// [`_pthread_cleanup_pop`] with the same record. Until 2026-09-28 neither
/// function existed, so every C program using cleanup handlers failed to
/// link.
#[repr(C)]
pub struct Ptcb {
    /// The handler.
    pub f: Option<unsafe extern "C" fn(*mut u8)>,
    /// Its argument.
    pub x: *mut u8,
    /// The next handler out, pushed before this one.
    pub next: *mut Ptcb,
}

/// Push `cb` -- `f(x)` -- onto the calling thread's cleanup handlers.
///
/// # Safety
///
/// `cb` is the caller's record, live until the matching
/// [`_pthread_cleanup_pop`], which the macro pair guarantees by scope.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _pthread_cleanup_push(
    cb: *mut Ptcb,
    f: Option<unsafe extern "C" fn(*mut u8)>,
    x: *mut u8,
) {
    // SAFETY: `cb` is the caller's record (this function's contract);
    // `current()` is this thread's block.
    unsafe {
        let head = &raw mut (*crate::perthread::current()).cleanup;
        cb.write(Ptcb { f, x, next: *head });
        *head = cb;
    }
}

/// Pop `cb`, the innermost handler, and run it if `run` is nonzero.
///
/// # Safety
///
/// `cb` is the record the matching [`_pthread_cleanup_push`] pushed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn _pthread_cleanup_pop(cb: *mut Ptcb, run: i32) {
    // SAFETY: as in `_pthread_cleanup_push`; the handler is the caller's.
    unsafe {
        (*crate::perthread::current()).cleanup = (*cb).next;
        if run != 0
            && let Some(f) = (*cb).f
        {
            f((*cb).x);
        }
    }
}

/// Run the calling thread's remaining cleanup handlers, innermost first,
/// each popped before it runs so a handler that exits the thread cannot run
/// again (`pthread_exit`).
fn run_cleanup_handlers() {
    loop {
        // SAFETY: this thread's block; each record is live, on the stack of
        // a frame that has not returned (its pop has not run).
        unsafe {
            let head = (*crate::perthread::current()).cleanup;
            if head.is_null() {
                return;
            }
            (*crate::perthread::current()).cleanup = (*head).next;
            if let Some(f) = (*head).f {
                f((*head).x);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {

    /// Serialises the fourteen tests that drive `THREAD_NAMES`.
    ///
    /// The production `THREAD_NAME_LOCK` spinlock beside that table keeps the
    /// data consistent, so this is not about corruption -- it is about
    /// interference. These tests address threads by number, and the numbers
    /// overlap: six of them write thread 0, and ids 1, 2, 3, 4 and 9 are each
    /// driven by two. `test_pthread_setname_getname_roundtrip` names threads 3
    /// and 4 and reads them back, while `..._short_len_rejected` writes 3 and
    /// `..._buffer_too_small` writes 4. Cargo runs these on several threads, so
    /// the roundtrip can read a name another test set and fail in a way that
    /// reproduces rarely and looks like a bug in `pthread_getname_np`.
    ///
    /// Named `..._TEST_LOCK` because `THREAD_NAME_LOCK` is already taken by the
    /// real spinlock this file ships; the two guard different things.
    static THREAD_NAME_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Take the lock, ignoring poisoning: a panicking test would otherwise make
    /// the other thirteen panic on `.unwrap()`, reporting one real failure as
    /// fourteen and burying the one that matters.
    fn thread_name_test_guard() -> std::sync::MutexGuard<'static, ()> {
        THREAD_NAME_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    use super::*;

    // -- the functions musl's <pthread.h> declares (2026-09-28) --

    /// What the cleanup handlers below record: the order they ran in.
    static CLEANUP_RAN: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
    /// Serialises the tests that use [`CLEANUP_RAN`].
    static CLEANUP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    unsafe extern "C" fn record(x: *mut u8) {
        CLEANUP_RAN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(x as usize);
    }

    fn ran() -> Vec<usize> {
        core::mem::take(
            &mut *CLEANUP_RAN
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    fn blank() -> Ptcb {
        Ptcb {
            f: None,
            x: core::ptr::null_mut(),
            next: core::ptr::null_mut(),
        }
    }

    #[test]
    #[allow(clippy::used_underscore_items)] // the C names are the API
    fn cleanup_pop_runs_the_handler_only_when_asked_and_in_lifo_order() {
        let _g = CLEANUP_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = ran();
        let (mut a, mut b, mut c) = (blank(), blank(), blank());
        // SAFETY: each record outlives its pop, as the C macros guarantee.
        unsafe {
            _pthread_cleanup_push(&raw mut a, Some(record), 1 as *mut u8);
            _pthread_cleanup_push(&raw mut b, Some(record), 2 as *mut u8);
            _pthread_cleanup_push(&raw mut c, Some(record), 3 as *mut u8);
            _pthread_cleanup_pop(&raw mut c, 1);
            _pthread_cleanup_pop(&raw mut b, 0);
            _pthread_cleanup_pop(&raw mut a, 7);
            assert!((*crate::perthread::current()).cleanup.is_null());
        }
        assert_eq!(ran(), [3, 1]);
    }

    #[test]
    #[allow(clippy::used_underscore_items)]
    fn pthread_exit_runs_what_is_still_pushed_innermost_first() {
        let _g = CLEANUP_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = ran();
        let (mut a, mut b) = (blank(), blank());
        // SAFETY: as above; `run_cleanup_handlers` is `pthread_exit`'s first
        // step, which a test thread cannot take whole.
        unsafe {
            _pthread_cleanup_push(&raw mut a, Some(record), 10 as *mut u8);
            _pthread_cleanup_push(&raw mut b, Some(record), 20 as *mut u8);
        }
        run_cleanup_handlers();
        // SAFETY: this thread's block.
        assert!(unsafe { (*crate::perthread::current()).cleanup.is_null() });
        assert_eq!(ran(), [20, 10]);
    }

    fn fresh_attr() -> PthreadAttrT {
        let mut a: PthreadAttrT = [0xAA; 56];
        assert_eq!(pthread_attr_init(&raw mut a), 0);
        a
    }

    #[test]
    fn scheduling_attributes_default_to_inherit_other_zero_system() {
        let a = fresh_attr();
        let (mut v, mut p) = (-1, crate::sched::SchedParam::default());
        assert_eq!(pthread_attr_getinheritsched(&raw const a, &raw mut v), 0);
        assert_eq!(v, PTHREAD_INHERIT_SCHED);
        assert_eq!(pthread_attr_getschedpolicy(&raw const a, &raw mut v), 0);
        assert_eq!(v, crate::sched::SCHED_OTHER);
        p.sched_priority = 9;
        assert_eq!(pthread_attr_getschedparam(&raw const a, &raw mut p), 0);
        assert_eq!(p.sched_priority, 0);
        assert_eq!(pthread_attr_getscope(&raw const a, &raw mut v), 0);
        assert_eq!(v, PTHREAD_SCOPE_SYSTEM);
    }

    #[test]
    fn scheduling_attributes_are_checked_as_glibc_checks_them() {
        let mut a = fresh_attr();
        let mut v = -1;
        assert_eq!(
            pthread_attr_setinheritsched(&raw mut a, PTHREAD_EXPLICIT_SCHED),
            0
        );
        assert_eq!(pthread_attr_getinheritsched(&raw const a, &raw mut v), 0);
        assert_eq!(v, PTHREAD_EXPLICIT_SCHED);
        assert_eq!(pthread_attr_setinheritsched(&raw mut a, 2), errno::EINVAL);

        assert_eq!(
            pthread_attr_setschedpolicy(&raw mut a, crate::sched::SCHED_FIFO),
            0
        );
        assert_eq!(
            pthread_attr_setschedpolicy(&raw mut a, crate::sched::SCHED_BATCH),
            errno::EINVAL
        );
        let prio = |sched_priority| crate::sched::SchedParam {
            sched_priority,
            ..Default::default()
        };
        assert_eq!(pthread_attr_setschedparam(&raw mut a, &prio(50)), 0);
        assert_eq!(
            pthread_attr_setschedparam(&raw mut a, &prio(100)),
            errno::EINVAL
        );
        // SCHED_OTHER's range is 0..=0.
        assert_eq!(
            pthread_attr_setschedpolicy(&raw mut a, crate::sched::SCHED_OTHER),
            0
        );
        assert_eq!(
            pthread_attr_setschedparam(&raw mut a, &prio(1)),
            errno::EINVAL
        );

        assert_eq!(pthread_attr_setscope(&raw mut a, PTHREAD_SCOPE_SYSTEM), 0);
        assert_eq!(
            pthread_attr_setscope(&raw mut a, PTHREAD_SCOPE_PROCESS),
            errno::ENOTSUP
        );
        assert_eq!(pthread_attr_setscope(&raw mut a, 7), errno::EINVAL);
        assert_eq!(
            pthread_attr_setscope(core::ptr::null_mut(), 0),
            errno::EFAULT
        );
        assert_eq!(
            pthread_attr_getscope(&raw const a, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn an_explicit_schedule_the_scheduler_cannot_give_is_eperm() {
        let mut a = fresh_attr();
        assert_eq!(
            pthread_attr_setinheritsched(&raw mut a, PTHREAD_EXPLICIT_SCHED),
            0
        );
        assert_eq!(
            pthread_attr_setschedpolicy(&raw mut a, crate::sched::SCHED_RR),
            0
        );
        let p = crate::sched::SchedParam {
            sched_priority: 10,
            ..Default::default()
        };
        assert_eq!(pthread_attr_setschedparam(&raw mut a, &raw const p), 0);
        assert_eq!(
            CreateAttr::read(&raw const a).explicit_sched,
            Some((crate::sched::SCHED_RR, 10))
        );
        let mut t: PthreadT = 0;
        // Refused before anything is allocated or any thread started.
        assert_eq!(
            pthread_create(&raw mut t, &raw const a, None, core::ptr::null_mut()),
            errno::EPERM
        );
        // Inheriting again, the same numbers are only stored.
        assert_eq!(
            pthread_attr_setinheritsched(&raw mut a, PTHREAD_INHERIT_SCHED),
            0
        );
        assert_eq!(CreateAttr::read(&raw const a).explicit_sched, None);
    }

    #[test]
    fn pthread_setschedprio_takes_other_s_one_priority() {
        assert_eq!(pthread_setschedprio(pthread_self(), 0), 0);
        assert_eq!(pthread_setschedprio(pthread_self(), 1), errno::EINVAL);
    }

    #[test]
    fn the_concurrency_level_is_kept_and_read_back() {
        assert_eq!(pthread_getconcurrency(), 0);
        assert_eq!(pthread_setconcurrency(4), 0);
        assert_eq!(pthread_getconcurrency(), 4);
        assert_eq!(pthread_setconcurrency(-1), errno::EINVAL);
        assert_eq!(pthread_getconcurrency(), 4);
    }

    #[test]
    fn default_attributes_are_what_null_attr_threads_get() {
        let mut d: PthreadAttrT = [0; 56];
        assert_eq!(pthread_getattr_default_np(&raw mut d), 0);
        assert_eq!(d, fresh_attr());
        assert_eq!(CreateAttr::read(core::ptr::null()), CreateAttr::DEFAULT);

        let mut want = fresh_attr();
        assert_eq!(pthread_attr_setstacksize(&raw mut want, 256 * 1024), 0);
        assert_eq!(pthread_setattr_default_np(&raw const want), 0);
        assert_eq!(CreateAttr::read(core::ptr::null()).stack_size, 256 * 1024);

        // Stack size 0 keeps the current one.
        let mut keep = fresh_attr();
        if let Some(slot) = keep.get_mut(ATTR_OFF_STACKSIZE..ATTR_OFF_STACKSIZE + 8) {
            slot.copy_from_slice(&0usize.to_ne_bytes());
        }
        assert_eq!(pthread_setattr_default_np(&raw const keep), 0);
        assert_eq!(CreateAttr::read(core::ptr::null()).stack_size, 256 * 1024);

        // A default stack address is refused, and so is a small stack.
        let mut bad = fresh_attr();
        let mut stack = vec![0u8; 256 * 1024];
        assert_eq!(
            pthread_attr_setstack(&raw mut bad, stack.as_mut_ptr().cast(), stack.len()),
            0
        );
        assert_eq!(pthread_setattr_default_np(&raw const bad), errno::EINVAL);
        let mut small = fresh_attr();
        if let Some(slot) = small.get_mut(ATTR_OFF_STACKSIZE..ATTR_OFF_STACKSIZE + 8) {
            slot.copy_from_slice(&16usize.to_ne_bytes());
        }
        assert_eq!(pthread_setattr_default_np(&raw const small), errno::EINVAL);
        assert_eq!(pthread_setattr_default_np(core::ptr::null()), errno::EFAULT);
    }

    #[test]
    fn getschedparam_reports_the_scheduler_we_have() {
        let mut policy = -1i32;
        let mut param = crate::sched::SchedParam::default();
        assert_eq!(pthread_getschedparam(0, &raw mut policy, &raw mut param), 0);
        assert_eq!(policy, crate::sched::SCHED_OTHER);
        assert_eq!(param.sched_priority, 0);
    }

    #[test]
    fn getschedparam_refuses_a_null_output() {
        let mut param = crate::sched::SchedParam::default();
        let mut policy = 0i32;
        assert_eq!(
            pthread_getschedparam(0, core::ptr::null_mut(), &raw mut param),
            errno::EFAULT
        );
        assert_eq!(
            pthread_getschedparam(0, &raw mut policy, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn setschedparam_refuses_a_priority_it_cannot_deliver() {
        // The important one. Accepting this would tell a caller its real-time
        // thread runs at priority 50 when nothing in the scheduler
        // distinguishes it from any other thread -- and a program that
        // believes it has a priority it does not have makes worse decisions
        // than one that knows it cannot have one.
        let rt = crate::sched::SchedParam {
            sched_priority: 50,
            ..crate::sched::SchedParam::default()
        };
        assert_eq!(
            pthread_setschedparam(0, crate::sched::SCHED_FIFO, &raw const rt),
            errno::EINVAL
        );
        // Same policy, priority it cannot give either.
        assert_eq!(
            pthread_setschedparam(0, crate::sched::SCHED_OTHER, &raw const rt),
            errno::EINVAL
        );
    }

    #[test]
    fn setschedparam_accepts_the_one_request_it_can_satisfy() {
        let plain = crate::sched::SchedParam::default();
        assert_eq!(
            pthread_setschedparam(0, crate::sched::SCHED_OTHER, &raw const plain),
            0
        );
        assert_eq!(
            pthread_setschedparam(0, crate::sched::SCHED_OTHER, core::ptr::null()),
            errno::EFAULT
        );
    }
    use core::sync::atomic::{AtomicI32, Ordering};

    // =======================================================================
    // Constants
    // =======================================================================

    // =======================================================================
    // pthread_kill / pthread_getcpuclockid
    // =======================================================================

    /// An out-of-range signal must be rejected *before* the thread is
    /// looked up: Linux's `tgkill` validates the signal independently,
    /// and a caller that passed both a stale id and a bad signal is
    /// better served by the argument complaint.
    #[test]
    fn pthread_kill_rejects_bad_signal_before_bad_thread() {
        let stale: PthreadT = u64::MAX - 1;
        assert_eq!(pthread_kill(stale, -1), errno::EINVAL);
        assert_eq!(pthread_kill(stale, crate::signal::NSIG), errno::EINVAL);
        // Same bad thread, valid signal -> the thread verdict surfaces.
        assert_eq!(pthread_kill(stale, crate::signal::SIGTERM), errno::ESRCH);
    }

    /// Signal 0 is the existence probe: no signal is sent, and the
    /// answer is purely "does this thread exist".  The calling thread
    /// always does.
    #[test]
    fn pthread_kill_signal_zero_probes_existence() {
        assert_eq!(pthread_kill(pthread_self(), 0), 0);
        assert_eq!(pthread_kill(u64::MAX - 1, 0), errno::ESRCH);
    }

    /// `SLOT_EMPTY` is the table's "no thread here" sentinel, not a
    /// thread id.  If it were ever accepted as live, an uninitialised
    /// `pthread_t` would silently signal something.
    #[test]
    fn pthread_kill_rejects_the_empty_slot_sentinel() {
        assert_eq!(pthread_kill(SLOT_EMPTY, 0), errno::ESRCH);
        assert_eq!(
            pthread_kill(SLOT_EMPTY, crate::signal::SIGTERM),
            errno::ESRCH
        );
    }

    /// The initial thread has no `THREAD_TABLE` slot — only
    /// `pthread_create`d threads get one — so it is recognised through
    /// `INITIAL_TASK_ID`, recorded by `__libc_start_main`.  Without that
    /// registration, `pthread_kill(main_thread, SIGTERM)` from a worker
    /// would answer ESRCH, which breaks the standard shutdown idiom.
    #[test]
    fn initial_thread_is_recognised_as_live() {
        // Host tests do not run through the crt, so register explicitly;
        // this is exactly what `__libc_start_main` does.
        record_initial_thread();
        let initial = INITIAL_TASK_ID.load(Ordering::Acquire);
        assert_ne!(initial, SLOT_EMPTY);
        assert!(thread_is_live(initial));
        assert_eq!(pthread_kill(initial, 0), 0);
    }

    /// `pthread_getcpuclockid` must validate the thread before the
    /// output pointer (a stale id is the caller's real bug), and must
    /// hand back the *thread* CPU clock for the caller itself.
    #[test]
    fn pthread_getcpuclockid_self_yields_thread_cputime_clock() {
        let mut clk: crate::types::ClockidT = -1;
        assert_eq!(pthread_getcpuclockid(pthread_self(), &raw mut clk), 0);
        assert_eq!(clk, crate::time::CLOCK_THREAD_CPUTIME_ID);
    }

    #[test]
    fn pthread_getcpuclockid_validates_thread_before_pointer() {
        // Stale thread + NULL out: the thread verdict wins.
        assert_eq!(
            pthread_getcpuclockid(u64::MAX - 1, core::ptr::null_mut()),
            errno::ESRCH
        );
        // Live thread + NULL out: now the pointer is the complaint.
        assert_eq!(
            pthread_getcpuclockid(pthread_self(), core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn mutex_type_constants() {
        assert_eq!(PTHREAD_MUTEX_NORMAL, 0);
        assert_eq!(PTHREAD_MUTEX_RECURSIVE, 1);
        assert_eq!(PTHREAD_MUTEX_ERRORCHECK, 2);
        assert_eq!(PTHREAD_MUTEX_DEFAULT, 0);
        assert_eq!(PTHREAD_MUTEX_DEFAULT, PTHREAD_MUTEX_NORMAL);
    }

    #[test]
    fn detach_state_constants() {
        assert_eq!(PTHREAD_CREATE_JOINABLE, 0);
        assert_eq!(PTHREAD_CREATE_DETACHED, 1);
    }

    #[test]
    fn barrier_serial_thread_constant() {
        assert_eq!(PTHREAD_BARRIER_SERIAL_THREAD, -1);
    }

    #[test]
    fn process_shared_constants() {
        assert_eq!(PTHREAD_PROCESS_PRIVATE, 0);
        assert_eq!(PTHREAD_PROCESS_SHARED, 1);
    }

    #[test]
    fn cancel_constants() {
        assert_eq!(PTHREAD_CANCEL_DEFERRED, 0);
        assert_eq!(PTHREAD_CANCEL_ASYNCHRONOUS, 1);
        assert_eq!(PTHREAD_CANCEL_ENABLE, 0);
        assert_eq!(PTHREAD_CANCEL_DISABLE, 1);
    }

    // =======================================================================
    // Struct sizes
    // =======================================================================

    #[test]
    fn struct_size_pthread_mutex_t() {
        assert_eq!(core::mem::size_of::<PthreadMutexT>(), 40);
    }

    #[test]
    fn struct_size_pthread_cond_t() {
        assert_eq!(core::mem::size_of::<PthreadCondT>(), 48);
    }

    #[test]
    fn struct_size_pthread_spinlock_t() {
        assert_eq!(core::mem::size_of::<PthreadSpinlockT>(), 4);
    }

    #[test]
    fn struct_size_pthread_attr_t() {
        // glibc x86_64 pthread_attr_t = 56 bytes.
        assert_eq!(core::mem::size_of::<PthreadAttrT>(), 56);
    }

    #[test]
    fn struct_size_pthread_barrier_t() {
        // glibc x86_64 pthread_barrier_t = 32 bytes.
        assert_eq!(core::mem::size_of::<PthreadBarrierT>(), 32);
    }

    #[test]
    fn struct_size_pthread_mutexattr_t() {
        // glibc x86_64 pthread_mutexattr_t = 4 bytes.
        assert_eq!(core::mem::size_of::<PthreadMutexattrT>(), 4);
    }

    #[test]
    fn struct_size_pthread_condattr_t() {
        // glibc x86_64 pthread_condattr_t = 4 bytes.
        assert_eq!(core::mem::size_of::<PthreadCondattrT>(), 4);
    }

    #[test]
    fn struct_size_pthread_barrierattr_t() {
        // glibc x86_64 pthread_barrierattr_t = 4 bytes.
        assert_eq!(core::mem::size_of::<PthreadBarrierattrT>(), 4);
    }

    #[test]
    fn struct_size_pthread_rwlock_t() {
        // glibc x86_64 pthread_rwlock_t = 56 bytes.
        assert_eq!(core::mem::size_of::<PthreadRwlockT>(), 56);
    }

    #[test]
    fn struct_size_pthread_rwlockattr_t() {
        // glibc x86_64 pthread_rwlockattr_t = 8 bytes.
        assert_eq!(core::mem::size_of::<PthreadRwlockattrT>(), 8);
    }

    #[test]
    fn struct_size_pthread_once_t() {
        // glibc x86_64 pthread_once_t = 4 bytes.
        assert_eq!(core::mem::size_of::<PthreadOnceT>(), 4);
    }

    // =======================================================================
    // pthread_equal
    // =======================================================================

    #[test]
    fn pthread_equal_same() {
        assert_eq!(pthread_equal(42, 42), 1);
    }

    #[test]
    fn pthread_equal_different() {
        assert_eq!(pthread_equal(1, 2), 0);
    }

    #[test]
    fn pthread_equal_zero() {
        assert_eq!(pthread_equal(0, 0), 1);
    }

    #[test]
    fn pthread_equal_max() {
        assert_eq!(pthread_equal(u64::MAX, u64::MAX), 1);
        assert_eq!(pthread_equal(u64::MAX, 0), 0);
    }

    // =======================================================================
    // Mutex attributes
    // =======================================================================

    #[test]
    fn mutexattr_init_zeroes() {
        let mut attr: PthreadMutexattrT = [0xFF; 4];
        let ret = pthread_mutexattr_init(&mut attr);
        assert_eq!(ret, 0);
        assert_eq!(attr, [0u8; 4]);
    }

    #[test]
    fn mutexattr_init_null_returns_efault() {
        let ret = pthread_mutexattr_init(core::ptr::null_mut());
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn mutexattr_destroy_returns_zero() {
        let mut attr: PthreadMutexattrT = [0; 4];
        let ret = pthread_mutexattr_destroy(&mut attr);
        assert_eq!(ret, 0);
    }

    #[test]
    fn mutexattr_settype_normal() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        let ret = pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_NORMAL);
        assert_eq!(ret, 0);
    }

    #[test]
    fn mutexattr_settype_recursive() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        let ret = pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_RECURSIVE);
        assert_eq!(ret, 0);
    }

    #[test]
    fn mutexattr_settype_errorcheck() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        let ret = pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_ERRORCHECK);
        assert_eq!(ret, 0);
    }

    #[test]
    fn mutexattr_settype_invalid_rejected() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        assert_eq!(pthread_mutexattr_settype(&mut attr, 3), errno::EINVAL);
        assert_eq!(pthread_mutexattr_settype(&mut attr, -1), errno::EINVAL);
        assert_eq!(pthread_mutexattr_settype(&mut attr, 100), errno::EINVAL);
    }

    #[test]
    fn mutexattr_settype_null_returns_efault() {
        assert_eq!(
            pthread_mutexattr_settype(core::ptr::null_mut(), 0),
            errno::EFAULT
        );
    }

    /// glibc's `___pthread_mutexattr_settype` (nptl/pthread_mutexattr_settype.c)
    /// opens with `if (kind < PTHREAD_MUTEX_NORMAL || kind > PTHREAD_MUTEX_ERRORCHECK)
    /// return EINVAL;` and only afterwards writes through `attr`, so an
    /// out-of-domain kind is decided before the pointer is ever touched.
    /// See `design-decisions.md` §303.
    #[test]
    fn mutexattr_settype_bad_kind_outranks_a_null_attr() {
        assert_eq!(
            pthread_mutexattr_settype(core::ptr::null_mut(), 3),
            errno::EINVAL
        );
    }

    #[test]
    fn mutexattr_gettype_reads_back() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        let mut kind: i32 = -1;
        let ret = pthread_mutexattr_gettype(&attr, &mut kind);
        assert_eq!(ret, 0);
        assert_eq!(kind, 0); // Default is NORMAL after init.
    }

    #[test]
    fn mutexattr_gettype_null_attr_returns_efault() {
        let mut kind: i32 = 0;
        assert_eq!(
            pthread_mutexattr_gettype(core::ptr::null(), &mut kind),
            errno::EFAULT
        );
    }

    #[test]
    fn mutexattr_gettype_null_kind_returns_efault() {
        let attr: PthreadMutexattrT = [0; 4];
        assert_eq!(
            pthread_mutexattr_gettype(&attr, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn mutexattr_roundtrip_normal() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_NORMAL);
        let mut kind: i32 = -1;
        pthread_mutexattr_gettype(&attr, &mut kind);
        assert_eq!(kind, PTHREAD_MUTEX_NORMAL);
    }

    #[test]
    fn mutexattr_roundtrip_recursive() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_RECURSIVE);
        let mut kind: i32 = -1;
        pthread_mutexattr_gettype(&attr, &mut kind);
        assert_eq!(kind, PTHREAD_MUTEX_RECURSIVE);
    }

    #[test]
    fn mutexattr_roundtrip_errorcheck() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_ERRORCHECK);
        let mut kind: i32 = -1;
        pthread_mutexattr_gettype(&attr, &mut kind);
        assert_eq!(kind, PTHREAD_MUTEX_ERRORCHECK);
    }

    // =======================================================================
    // Mutex init (no lock/unlock -- those need kernel SYS_TASK_ID)
    // =======================================================================

    #[test]
    fn mutex_init_default_attr() {
        let mut mutex = PTHREAD_MUTEX_INITIALIZER;
        // Set non-zero values to confirm init overwrites them.
        mutex.locked.store(1, Ordering::Relaxed);
        mutex.kind.store(99, Ordering::Relaxed);
        mutex.owner.store(42, Ordering::Relaxed);
        mutex.count.store(7, Ordering::Relaxed);

        let ret = unsafe { pthread_mutex_init(&mut mutex, core::ptr::null()) };
        assert_eq!(ret, 0);
        assert_eq!(mutex.locked.load(Ordering::Relaxed), 0);
        assert_eq!(mutex.kind.load(Ordering::Relaxed), PTHREAD_MUTEX_NORMAL);
        assert_eq!(mutex.owner.load(Ordering::Relaxed), 0);
        assert_eq!(mutex.count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn mutex_init_with_recursive_attr() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_RECURSIVE);

        let mut mutex = PTHREAD_MUTEX_INITIALIZER;
        let ret = unsafe { pthread_mutex_init(&mut mutex, &attr) };
        assert_eq!(ret, 0);
        assert_eq!(mutex.locked.load(Ordering::Relaxed), 0);
        assert_eq!(mutex.kind.load(Ordering::Relaxed), PTHREAD_MUTEX_RECURSIVE);
        assert_eq!(mutex.owner.load(Ordering::Relaxed), 0);
        assert_eq!(mutex.count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn mutex_init_with_errorcheck_attr() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_ERRORCHECK);

        let mut mutex = PTHREAD_MUTEX_INITIALIZER;
        let ret = unsafe { pthread_mutex_init(&mut mutex, &attr) };
        assert_eq!(ret, 0);
        assert_eq!(mutex.kind.load(Ordering::Relaxed), PTHREAD_MUTEX_ERRORCHECK);
    }

    #[test]
    fn mutex_init_null_mutex_returns_efault() {
        let ret = unsafe { pthread_mutex_init(core::ptr::null_mut(), core::ptr::null()) };
        assert_eq!(ret, errno::EFAULT);
    }

    // =======================================================================
    // Thread attributes
    // =======================================================================

    #[test]
    fn attr_init_stores_default_stack_size() {
        let mut attr: PthreadAttrT = [0xFF; 56];
        let ret = pthread_attr_init(&mut attr);
        assert_eq!(ret, 0);

        // First 8 bytes hold the default stack size.
        let stored = unsafe { core::ptr::read_unaligned(attr.as_ptr().cast::<usize>()) };
        assert_eq!(stored, DEFAULT_THREAD_STACK_SIZE);
        assert_eq!(stored, 64 * 1024);

        // The guard -- one page, as glibc records it -- and every other byte
        // zero.
        let guard = unsafe {
            core::ptr::read_unaligned(attr.as_ptr().add(ATTR_OFF_GUARDSIZE).cast::<usize>())
        };
        assert_eq!(guard, DEFAULT_GUARD_SIZE);
        for (i, &b) in attr.iter().enumerate().skip(8) {
            if !(ATTR_OFF_GUARDSIZE..ATTR_OFF_GUARDSIZE + 8).contains(&i) {
                assert_eq!(b, 0, "attr byte {i} should be zeroed");
            }
        }
    }

    #[test]
    fn attr_init_null_returns_efault() {
        assert_eq!(pthread_attr_init(core::ptr::null_mut()), errno::EFAULT);
    }

    #[test]
    fn attr_destroy_returns_zero() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        assert_eq!(pthread_attr_destroy(&mut attr), 0);
    }

    #[test]
    fn attr_setstacksize_stores_value() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        let ret = pthread_attr_setstacksize(&mut attr, 128 * 1024);
        assert_eq!(ret, 0);
        let stored = unsafe { core::ptr::read_unaligned(attr.as_ptr().cast::<usize>()) };
        assert_eq!(stored, 128 * 1024);
    }

    /// The floor is `PTHREAD_STACK_MIN`, which glibc's `check_stacksize_attr`
    /// (sysdeps/nptl/pthreadP.h:704) compares against -- with the number from
    /// the header a C program compiles against, musl's 2048.  (glibc's own is
    /// 16 KiB, and was this floor until 2026-09-27, refusing a port's
    /// `PTHREAD_STACK_MIN + margin` below it.)
    #[test]
    fn attr_setstacksize_minimum_is_pthread_stack_min() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        assert_eq!(
            pthread_attr_setstacksize(&mut attr, PTHREAD_STACK_MIN as usize),
            0
        );
        assert_eq!(
            pthread_attr_setstacksize(&mut attr, PTHREAD_STACK_MIN as usize - 1),
            errno::EINVAL
        );
        // A 4 KiB page clears musl's floor; the stack is still rounded up to
        // one of this kernel's 16 KiB pages when the thread is made.
        assert_eq!(pthread_attr_setstacksize(&mut attr, 4096), 0);
    }

    #[test]
    fn attr_setstacksize_rejects_too_small() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        assert_eq!(pthread_attr_setstacksize(&mut attr, 2047), errno::EINVAL);
        assert_eq!(pthread_attr_setstacksize(&mut attr, 0), errno::EINVAL);
        assert_eq!(pthread_attr_setstacksize(&mut attr, 1), errno::EINVAL);
    }

    /// `EFAULT` is reserved for the case where the *size* is acceptable and only
    /// the pointer is bad, so the size here is one no floor refuses.
    #[test]
    fn attr_setstacksize_null_returns_efault() {
        assert_eq!(
            pthread_attr_setstacksize(core::ptr::null_mut(), 64 * 1024),
            errno::EFAULT
        );
    }

    /// glibc's `__pthread_attr_setstacksize` (nptl/pthread_attr_setstacksize.c)
    /// opens with `int ret = check_stacksize_attr (stacksize); if (ret) return ret;`
    /// and dereferences the attribute only afterwards, so a too-small size is
    /// decided before the pointer is examined.  See `design-decisions.md` §303.
    #[test]
    fn attr_setstacksize_too_small_outranks_a_null_attr() {
        assert_eq!(
            pthread_attr_setstacksize(core::ptr::null_mut(), 1024),
            errno::EINVAL
        );
    }

    #[test]
    fn attr_getstacksize_reads_default() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        let mut size: usize = 0;
        let ret = pthread_attr_getstacksize(&attr, &mut size);
        assert_eq!(ret, 0);
        assert_eq!(size, DEFAULT_THREAD_STACK_SIZE);
    }

    #[test]
    fn attr_getstacksize_roundtrip() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        pthread_attr_setstacksize(&mut attr, 256 * 1024);
        let mut size: usize = 0;
        pthread_attr_getstacksize(&attr, &mut size);
        assert_eq!(size, 256 * 1024);
    }

    #[test]
    fn attr_getstacksize_null_attr_returns_efault() {
        let mut size: usize = 0;
        assert_eq!(
            pthread_attr_getstacksize(core::ptr::null(), &mut size),
            errno::EFAULT
        );
    }

    #[test]
    fn attr_getstacksize_null_size_returns_efault() {
        let attr: PthreadAttrT = [0; 56];
        assert_eq!(
            pthread_attr_getstacksize(&attr, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn attr_getstacksize_returns_default_when_zero() {
        // If the stored stack size is 0 (e.g. from a raw-zeroed attr),
        // getstacksize should return DEFAULT_THREAD_STACK_SIZE.
        let attr: PthreadAttrT = [0; 56]; // All zeros -- stack size field is 0.
        let mut size: usize = 0;
        let ret = pthread_attr_getstacksize(&attr, &mut size);
        assert_eq!(ret, 0);
        assert_eq!(size, DEFAULT_THREAD_STACK_SIZE);
    }

    #[test]
    fn attr_setdetachstate_joinable() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        let ret = pthread_attr_setdetachstate(&mut attr, PTHREAD_CREATE_JOINABLE);
        assert_eq!(ret, 0);
    }

    #[test]
    fn attr_setdetachstate_detached() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        let ret = pthread_attr_setdetachstate(&mut attr, PTHREAD_CREATE_DETACHED);
        assert_eq!(ret, 0);
    }

    #[test]
    fn attr_setdetachstate_rejects_invalid() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        assert_eq!(pthread_attr_setdetachstate(&mut attr, 2), errno::EINVAL);
        assert_eq!(pthread_attr_setdetachstate(&mut attr, -1), errno::EINVAL);
        assert_eq!(pthread_attr_setdetachstate(&mut attr, 99), errno::EINVAL);
    }

    #[test]
    fn attr_setdetachstate_null_returns_efault() {
        assert_eq!(
            pthread_attr_setdetachstate(core::ptr::null_mut(), 0),
            errno::EFAULT
        );
    }

    /// glibc's `__pthread_attr_setdetachstate`
    /// (nptl/pthread_attr_setdetachstate.c) opens with a `/* Catch invalid
    /// values.  */` test on `detachstate` and writes through `attr` only
    /// afterwards.  See `design-decisions.md` §303.
    #[test]
    fn attr_setdetachstate_bad_state_outranks_a_null_attr() {
        assert_eq!(
            pthread_attr_setdetachstate(core::ptr::null_mut(), 2),
            errno::EINVAL
        );
    }

    #[test]
    fn attr_getdetachstate_reads_joinable() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        pthread_attr_setdetachstate(&mut attr, PTHREAD_CREATE_JOINABLE);
        let mut state: i32 = -1;
        let ret = pthread_attr_getdetachstate(&attr, &mut state);
        assert_eq!(ret, 0);
        assert_eq!(state, PTHREAD_CREATE_JOINABLE);
    }

    #[test]
    fn attr_getdetachstate_reads_detached() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        pthread_attr_setdetachstate(&mut attr, PTHREAD_CREATE_DETACHED);
        let mut state: i32 = -1;
        let ret = pthread_attr_getdetachstate(&attr, &mut state);
        assert_eq!(ret, 0);
        assert_eq!(state, PTHREAD_CREATE_DETACHED);
    }

    #[test]
    fn attr_getdetachstate_null_attr_returns_efault() {
        let mut state: i32 = 0;
        assert_eq!(
            pthread_attr_getdetachstate(core::ptr::null(), &mut state),
            errno::EFAULT
        );
    }

    #[test]
    fn attr_getdetachstate_null_state_returns_efault() {
        let attr: PthreadAttrT = [0; 56];
        assert_eq!(
            pthread_attr_getdetachstate(&attr, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    // =======================================================================
    // Stack/guard attributes (getattr_np support) — pure, race-free tests
    // =======================================================================

    #[test]
    fn main_thread_stack_attr_matches_kernel_layout() {
        let a = main_thread_stack_attr();
        // Low address + size must reach exactly the kernel stack top.
        assert_eq!(a.addr, MAIN_STACK_LOW);
        assert_eq!(a.addr.wrapping_add(a.size), MAIN_STACK_TOP);
        assert_eq!(a.size, 4 * 1024 * 1024);
        assert_eq!(a.guard, DEFAULT_GUARD_SIZE);
        assert!(!a.detached);
    }

    #[test]
    fn encode_attr_roundtrips_through_getstack_getguardsize() {
        let resolved = StackAttr {
            addr: 0x1234_5000,
            size: 128 * 1024,
            guard: 16 * 1024,
            detached: true,
        };
        let mut buf: PthreadAttrT = [0xAB; 56];
        encode_attr(&mut buf, resolved);

        // getstack reports the encoded address and size.
        let mut addr: *mut core::ffi::c_void = core::ptr::null_mut();
        let mut size: usize = 0;
        assert_eq!(pthread_attr_getstack(&buf, &mut addr, &mut size), 0);
        assert_eq!(addr as usize, 0x1234_5000);
        assert_eq!(size, 128 * 1024);

        // getguardsize reports the encoded guard.
        let mut guard: usize = 0;
        assert_eq!(pthread_attr_getguardsize(&buf, &mut guard), 0);
        assert_eq!(guard, 16 * 1024);

        // detach state round-trips too.
        let mut detach: i32 = -1;
        assert_eq!(pthread_attr_getdetachstate(&buf, &mut detach), 0);
        assert_eq!(detach, PTHREAD_CREATE_DETACHED);
    }

    #[test]
    fn encode_attr_zeroes_reserved_bytes() {
        let mut buf: PthreadAttrT = [0xFF; 56];
        encode_attr(
            &mut buf,
            StackAttr {
                addr: 0,
                size: 0,
                guard: 0,
                detached: false,
            },
        );
        // All bytes must be cleared when every field is zero.
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn getstack_main_attr_reports_growable_region() {
        // Emulate pthread_getattr_np filling the buffer for the main thread.
        let mut buf: PthreadAttrT = [0; 56];
        encode_attr(&mut buf, main_thread_stack_attr());

        let mut addr: *mut core::ffi::c_void = core::ptr::null_mut();
        let mut size: usize = 0;
        assert_eq!(pthread_attr_getstack(&buf, &mut addr, &mut size), 0);
        assert_eq!(addr as usize, MAIN_STACK_LOW);
        assert_eq!((addr as usize).wrapping_add(size), MAIN_STACK_TOP);
    }

    #[test]
    fn getstack_default_attr_reports_null_addr_default_size() {
        // A default-initialized attr has no stack address; getstack should
        // report null and the default stack size.
        let mut buf: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut buf);
        let mut addr: *mut core::ffi::c_void = 0xDEAD_0000usize as *mut core::ffi::c_void;
        let mut size: usize = 0;
        assert_eq!(pthread_attr_getstack(&buf, &mut addr, &mut size), 0);
        assert!(addr.is_null());
        assert_eq!(size, DEFAULT_THREAD_STACK_SIZE);
    }

    #[test]
    fn setstack_then_getstack_roundtrips() {
        let mut buf: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut buf);
        let provided = 0x4000_0000usize as *mut core::ffi::c_void;
        assert_eq!(pthread_attr_setstack(&mut buf, provided, 256 * 1024), 0);

        let mut addr: *mut core::ffi::c_void = core::ptr::null_mut();
        let mut size: usize = 0;
        assert_eq!(pthread_attr_getstack(&buf, &mut addr, &mut size), 0);
        assert_eq!(addr as usize, 0x4000_0000);
        assert_eq!(size, 256 * 1024);
    }

    #[test]
    fn setstack_rejects_too_small() {
        let mut buf: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut buf);
        assert_eq!(
            pthread_attr_setstack(&mut buf, core::ptr::null_mut(), 100),
            errno::EINVAL
        );
    }

    /// As with `pthread_attr_setstacksize`, `EFAULT` only applies once the size
    /// clears `PTHREAD_STACK_MIN`, so the size here is one no floor refuses.
    #[test]
    fn setstack_null_attr_returns_efault() {
        assert_eq!(
            pthread_attr_setstack(core::ptr::null_mut(), core::ptr::null_mut(), 64 * 1024),
            errno::EFAULT
        );
    }

    /// glibc's `__pthread_attr_setstack` (nptl/pthread_attr_setstack.c) shares
    /// `check_stacksize_attr`'s prologue with `setstacksize`, so a too-small size
    /// is decided before the attribute pointer is touched.
    /// See `design-decisions.md` §303.
    #[test]
    fn setstack_too_small_outranks_a_null_attr() {
        assert_eq!(
            pthread_attr_setstack(core::ptr::null_mut(), core::ptr::null_mut(), 1024),
            errno::EINVAL
        );
    }

    #[test]
    fn setguardsize_then_getguardsize_roundtrips() {
        let mut buf: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut buf);
        assert_eq!(pthread_attr_setguardsize(&mut buf, 32 * 1024), 0);
        let mut guard: usize = 0;
        assert_eq!(pthread_attr_getguardsize(&buf, &mut guard), 0);
        assert_eq!(guard, 32 * 1024);
    }

    #[test]
    fn getguardsize_default_attr_is_one_page() {
        // glibc's `__pthread_attr_init` records `__getpagesize ()`; this
        // reported 0 until 2026-09-26, when no created thread had a guard.
        let mut buf: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut buf);
        let mut guard: usize = 12345;
        assert_eq!(pthread_attr_getguardsize(&buf, &mut guard), 0);
        assert_eq!(guard, crate::unistd::PAGE_SIZE);
    }

    #[test]
    fn getstack_null_args_return_efault() {
        let buf: PthreadAttrT = [0; 56];
        let mut addr: *mut core::ffi::c_void = core::ptr::null_mut();
        let mut size: usize = 0;
        assert_eq!(
            pthread_attr_getstack(core::ptr::null(), &mut addr, &mut size),
            errno::EFAULT
        );
        assert_eq!(
            pthread_attr_getstack(&buf, core::ptr::null_mut(), &mut size),
            errno::EFAULT
        );
        assert_eq!(
            pthread_attr_getstack(&buf, &mut addr, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn getguardsize_null_args_return_efault() {
        let buf: PthreadAttrT = [0; 56];
        let mut guard: usize = 0;
        assert_eq!(
            pthread_attr_getguardsize(core::ptr::null(), &mut guard),
            errno::EFAULT
        );
        assert_eq!(
            pthread_attr_getguardsize(&buf, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn setguardsize_null_attr_returns_efault() {
        assert_eq!(
            pthread_attr_setguardsize(core::ptr::null_mut(), 4096),
            errno::EFAULT
        );
    }

    // =======================================================================
    // Condition variable init / destroy
    // =======================================================================

    #[test]
    fn cond_init_zeroes_generation() {
        let mut cond = PthreadCondT {
            generation: AtomicI32::new(42),
            clock: 0,
            waiters: AtomicI32::new(0),
            _pad: [0xFF; 36],
        };
        let ret = pthread_cond_init(&mut cond, core::ptr::null());
        assert_eq!(ret, 0);
        assert_eq!(cond.generation.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cond_init_null_returns_efault() {
        assert_eq!(
            pthread_cond_init(core::ptr::null_mut(), core::ptr::null()),
            errno::EFAULT
        );
    }

    #[test]
    fn cond_destroy_returns_zero() {
        let mut cond = PthreadCondT {
            generation: AtomicI32::new(0),
            clock: 0,
            waiters: AtomicI32::new(0),
            _pad: [0; 36],
        };
        assert_eq!(pthread_cond_destroy(&mut cond), 0);
    }

    // =======================================================================
    // Condition variable attributes
    // =======================================================================

    #[test]
    fn condattr_init_zeroes() {
        let mut attr: PthreadCondattrT = [0xFF; 4];
        let ret = pthread_condattr_init(&mut attr);
        assert_eq!(ret, 0);
        assert_eq!(attr, [0u8; 4]);
    }

    #[test]
    fn condattr_init_null_returns_efault() {
        assert_eq!(pthread_condattr_init(core::ptr::null_mut()), errno::EFAULT);
    }

    #[test]
    fn condattr_destroy_returns_zero() {
        let mut attr: PthreadCondattrT = [0; 4];
        assert_eq!(pthread_condattr_destroy(&mut attr), 0);
    }

    #[test]
    fn condattr_setclock_realtime() {
        let mut attr: PthreadCondattrT = [0; 4];
        pthread_condattr_init(&mut attr);
        // CLOCK_REALTIME = 0
        let ret = pthread_condattr_setclock(&mut attr, 0);
        assert_eq!(ret, 0);
    }

    #[test]
    fn condattr_setclock_monotonic() {
        let mut attr: PthreadCondattrT = [0; 4];
        pthread_condattr_init(&mut attr);
        // CLOCK_MONOTONIC = 1
        let ret = pthread_condattr_setclock(&mut attr, 1);
        assert_eq!(ret, 0);
    }

    #[test]
    fn condattr_setclock_invalid_rejected() {
        let mut attr: PthreadCondattrT = [0; 4];
        pthread_condattr_init(&mut attr);
        assert_eq!(pthread_condattr_setclock(&mut attr, 2), errno::EINVAL);
        assert_eq!(pthread_condattr_setclock(&mut attr, -1), errno::EINVAL);
        assert_eq!(pthread_condattr_setclock(&mut attr, 99), errno::EINVAL);
    }

    #[test]
    fn condattr_setclock_null_returns_efault() {
        assert_eq!(
            pthread_condattr_setclock(core::ptr::null_mut(), 0),
            errno::EFAULT
        );
    }

    /// glibc's `__pthread_condattr_setclock` (nptl/pthread_condattr_setclock.c)
    /// validates `clock_id` in full — including the `futex_supports_exact_relative_timeouts`
    /// check — before it writes through `attr`, so a bad clock outranks a null
    /// pointer.  See `design-decisions.md` §303.
    #[test]
    fn condattr_setclock_bad_clock_outranks_a_null_attr() {
        assert_eq!(
            pthread_condattr_setclock(core::ptr::null_mut(), 99),
            errno::EINVAL
        );
    }

    #[test]
    fn condattr_getclock_reads_back() {
        let mut attr: PthreadCondattrT = [0; 4];
        pthread_condattr_init(&mut attr);
        let mut clock_id: i32 = -1;
        let ret = pthread_condattr_getclock(&attr, &mut clock_id);
        assert_eq!(ret, 0);
        assert_eq!(clock_id, 0); // Default after init is 0 (CLOCK_REALTIME).
    }

    #[test]
    fn condattr_getclock_roundtrip_monotonic() {
        let mut attr: PthreadCondattrT = [0; 4];
        pthread_condattr_init(&mut attr);
        pthread_condattr_setclock(&mut attr, 1); // CLOCK_MONOTONIC
        let mut clock_id: i32 = -1;
        pthread_condattr_getclock(&attr, &mut clock_id);
        assert_eq!(clock_id, 1);
    }

    #[test]
    fn condattr_getclock_null_attr_returns_efault() {
        let mut clock_id: i32 = 0;
        assert_eq!(
            pthread_condattr_getclock(core::ptr::null(), &mut clock_id),
            errno::EFAULT
        );
    }

    #[test]
    fn condattr_getclock_null_clockid_returns_efault() {
        let attr: PthreadCondattrT = [0; 4];
        assert_eq!(
            pthread_condattr_getclock(&attr, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    // =======================================================================
    // Rwlock init / destroy
    // =======================================================================

    #[test]
    fn rwlock_init_zeroes_state() {
        let mut rwlock = PthreadRwlockT {
            state: AtomicI32::new(42),
            waiters: AtomicI32::new(0),
            writer: AtomicI32::new(0),
            prefer_writer: AtomicI32::new(7),
            writers_waiting: AtomicI32::new(7),
            _pad: [0xFF; 36],
        };
        let ret = pthread_rwlock_init(&mut rwlock, core::ptr::null());
        assert_eq!(ret, 0);
        assert_eq!(rwlock.state.load(Ordering::Relaxed), 0);
        assert_eq!(
            rwlock.prefer_writer.load(Ordering::Relaxed),
            0,
            "readers, by default"
        );
        assert_eq!(rwlock.writers_waiting.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn rwlock_init_null_returns_efault() {
        assert_eq!(
            pthread_rwlock_init(core::ptr::null_mut(), core::ptr::null()),
            errno::EFAULT
        );
    }

    #[test]
    fn rwlock_destroy_returns_zero() {
        let mut rwlock = PthreadRwlockT {
            state: AtomicI32::new(0),
            waiters: AtomicI32::new(0),
            writer: AtomicI32::new(0),
            prefer_writer: AtomicI32::new(0),
            writers_waiting: AtomicI32::new(0),
            _pad: [0; 36],
        };
        assert_eq!(pthread_rwlock_destroy(&mut rwlock), 0);
    }

    // =======================================================================
    // Rwlock attributes
    // =======================================================================

    #[test]
    fn rwlockattr_init_zeroes() {
        let mut attr: PthreadRwlockattrT = [0xFF; 8];
        let ret = pthread_rwlockattr_init(&mut attr);
        assert_eq!(ret, 0);
        assert_eq!(attr, [0u8; 8]);
    }

    #[test]
    fn rwlockattr_init_null_returns_efault() {
        assert_eq!(
            pthread_rwlockattr_init(core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn rwlockattr_destroy_returns_zero() {
        let mut attr: PthreadRwlockattrT = [0; 8];
        assert_eq!(pthread_rwlockattr_destroy(&mut attr), 0);
    }

    #[test]
    fn rwlockattr_setpshared_private() {
        let mut attr: PthreadRwlockattrT = [0; 8];
        pthread_rwlockattr_init(&mut attr);
        let ret = pthread_rwlockattr_setpshared(&mut attr, PTHREAD_PROCESS_PRIVATE);
        assert_eq!(ret, 0);
    }

    #[test]
    fn rwlockattr_setpshared_rejects_shared() {
        let mut attr: PthreadRwlockattrT = [0; 8];
        pthread_rwlockattr_init(&mut attr);
        // Not supported -- returns ENOTSUP.
        assert_eq!(
            pthread_rwlockattr_setpshared(&mut attr, PTHREAD_PROCESS_SHARED),
            errno::ENOTSUP
        );
    }

    /// A value that is neither `PTHREAD_PROCESS_PRIVATE` nor
    /// `PTHREAD_PROCESS_SHARED` is `EINVAL`, not `ENOTSUP`: glibc's
    /// `___pthread_rwlockattr_setpshared` gates on
    /// `futex_supports_pshared (pshared)` (sysdeps/nptl/futex-internal.h:102),
    /// which accepts *both* POSIX values and returns `EINVAL` for anything else.
    /// `ENOTSUP` is our own verdict on `PTHREAD_PROCESS_SHARED`, which glibc
    /// accepts and we do not — it must not swallow out-of-domain values too.
    /// See `design-decisions.md` §303.
    #[test]
    fn rwlockattr_setpshared_rejects_invalid() {
        let mut attr: PthreadRwlockattrT = [0; 8];
        pthread_rwlockattr_init(&mut attr);
        assert_eq!(pthread_rwlockattr_setpshared(&mut attr, 2), errno::EINVAL);
        assert_eq!(pthread_rwlockattr_setpshared(&mut attr, -1), errno::EINVAL);
    }

    #[test]
    fn rwlockattr_setpshared_null_returns_efault() {
        assert_eq!(
            pthread_rwlockattr_setpshared(core::ptr::null_mut(), 0),
            errno::EFAULT
        );
    }

    /// The domain check precedes the pointer, and so does our `ENOTSUP` verdict:
    /// both are decided from the scalar alone.  See `design-decisions.md` §303.
    #[test]
    fn rwlockattr_setpshared_scalar_verdicts_outrank_a_null_attr() {
        assert_eq!(
            pthread_rwlockattr_setpshared(core::ptr::null_mut(), 2),
            errno::EINVAL
        );
        assert_eq!(
            pthread_rwlockattr_setpshared(core::ptr::null_mut(), PTHREAD_PROCESS_SHARED),
            errno::ENOTSUP
        );
    }

    #[test]
    fn rwlockattr_getpshared_reads_private() {
        let mut attr: PthreadRwlockattrT = [0; 8];
        pthread_rwlockattr_init(&mut attr);
        pthread_rwlockattr_setpshared(&mut attr, 0);
        let mut val: i32 = -1;
        let ret = pthread_rwlockattr_getpshared(&attr, &mut val);
        assert_eq!(ret, 0);
        assert_eq!(val, 0);
    }

    #[test]
    fn rwlockattr_getpshared_null_attr_returns_efault() {
        let mut val: i32 = 0;
        assert_eq!(
            pthread_rwlockattr_getpshared(core::ptr::null(), &mut val),
            errno::EFAULT
        );
    }

    #[test]
    fn rwlockattr_getpshared_null_val_returns_efault() {
        let attr: PthreadRwlockattrT = [0; 8];
        assert_eq!(
            pthread_rwlockattr_getpshared(&attr, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    // =======================================================================
    // Barrier init / destroy
    // =======================================================================

    #[test]
    fn barrier_init_stores_count() {
        let mut barrier = PthreadBarrierT {
            count: 0,
            current: AtomicI32::new(99),
            generation: AtomicI32::new(99),
            lock: AtomicI32::new(0),
            _pad: [0xFF; 16],
        };
        let ret = pthread_barrier_init(&mut barrier, core::ptr::null(), 5);
        assert_eq!(ret, 0);
        assert_eq!(barrier.count, 5);
        assert_eq!(barrier.current.load(Ordering::Relaxed), 0);
        assert_eq!(barrier.generation.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn barrier_init_count_zero_returns_einval() {
        let mut barrier = PthreadBarrierT {
            count: 0,
            current: AtomicI32::new(0),
            generation: AtomicI32::new(0),
            lock: AtomicI32::new(0),
            _pad: [0; 16],
        };
        let ret = pthread_barrier_init(&mut barrier, core::ptr::null(), 0);
        assert_eq!(ret, errno::EINVAL);
    }

    #[test]
    fn barrier_init_null_returns_efault() {
        let ret = pthread_barrier_init(core::ptr::null_mut(), core::ptr::null(), 3);
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn barrier_destroy_returns_zero() {
        let mut barrier = PthreadBarrierT {
            count: 3,
            current: AtomicI32::new(0),
            generation: AtomicI32::new(0),
            lock: AtomicI32::new(0),
            _pad: [0; 16],
        };
        assert_eq!(pthread_barrier_destroy(&mut barrier), 0);
    }

    /// A large-but-legal count is accepted.  The ceiling is
    /// `BARRIER_IN_THRESHOLD` (`UINT_MAX / 2`), matching glibc's
    /// `___pthread_barrier_init` (nptl/pthread_barrier_init.c), which rejects
    /// `count == 0 || count >= BARRIER_IN_THRESHOLD`.
    #[test]
    fn barrier_init_large_count() {
        let mut barrier = PthreadBarrierT {
            count: 0,
            current: AtomicI32::new(0),
            generation: AtomicI32::new(0),
            lock: AtomicI32::new(0),
            _pad: [0; 16],
        };
        let large = BARRIER_IN_THRESHOLD - 1;
        let ret = pthread_barrier_init(&mut barrier, core::ptr::null(), large);
        assert_eq!(ret, 0);
        assert_eq!(barrier.count, large);
    }

    /// `u32::MAX` used to be accepted, which was a latent hang: the arrival
    /// counter is an `AtomicI32`, so a count above `i32::MAX` can never be
    /// reached and every waiter would block forever.  glibc caps `count` at
    /// `BARRIER_IN_THRESHOLD` (`UINT_MAX / 2`,
    /// sysdeps/nptl/internaltypes.h:119) for its own reset protocol; we adopt
    /// the same bound.  See `design-decisions.md` §303.
    #[test]
    fn barrier_init_count_above_the_threshold_returns_einval() {
        let mut barrier = PthreadBarrierT {
            count: 0,
            current: AtomicI32::new(0),
            generation: AtomicI32::new(0),
            lock: AtomicI32::new(0),
            _pad: [0; 16],
        };
        assert_eq!(
            pthread_barrier_init(&mut barrier, core::ptr::null(), u32::MAX),
            errno::EINVAL
        );
        assert_eq!(
            pthread_barrier_init(&mut barrier, core::ptr::null(), BARRIER_IN_THRESHOLD),
            errno::EINVAL
        );
        assert_eq!(barrier.count, 0, "a rejected init must not store the count");
    }

    /// glibc validates `count` before it writes through `barrier`, so a bad
    /// count outranks a null pointer.  See `design-decisions.md` §303.
    #[test]
    fn barrier_init_bad_count_outranks_a_null_barrier() {
        assert_eq!(
            pthread_barrier_init(core::ptr::null_mut(), core::ptr::null(), 0),
            errno::EINVAL
        );
        assert_eq!(
            pthread_barrier_init(core::ptr::null_mut(), core::ptr::null(), u32::MAX),
            errno::EINVAL
        );
    }

    // =======================================================================
    // Spinlock init / destroy / trylock / unlock
    // =======================================================================

    #[test]
    fn spin_init_stores_zero() {
        let mut lock = AtomicI32::new(99);
        let ret = pthread_spin_init(&mut lock, 0);
        assert_eq!(ret, 0);
        assert_eq!(lock.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn spin_init_null_returns_efault() {
        assert_eq!(pthread_spin_init(core::ptr::null_mut(), 0), errno::EFAULT);
    }

    #[test]
    fn spin_destroy_returns_zero() {
        let mut lock = AtomicI32::new(0);
        assert_eq!(pthread_spin_destroy(&mut lock), 0);
    }

    #[test]
    fn spin_trylock_succeeds_when_unlocked() {
        let mut lock = AtomicI32::new(0);
        pthread_spin_init(&mut lock, 0);
        let ret = pthread_spin_trylock(&mut lock);
        assert_eq!(ret, 0);
        // Lock should now be held (value = 1).
        assert_eq!(lock.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn spin_trylock_fails_when_locked() {
        let mut lock = AtomicI32::new(0);
        pthread_spin_init(&mut lock, 0);
        // Acquire the lock.
        pthread_spin_trylock(&mut lock);
        // Second trylock should fail with EBUSY.
        let ret = pthread_spin_trylock(&mut lock);
        assert_eq!(ret, errno::EBUSY);
    }

    #[test]
    fn spin_trylock_null_returns_efault() {
        assert_eq!(pthread_spin_trylock(core::ptr::null_mut()), errno::EFAULT);
    }

    #[test]
    fn spin_unlock_releases_lock() {
        let mut lock = AtomicI32::new(0);
        pthread_spin_init(&mut lock, 0);
        pthread_spin_trylock(&mut lock);
        assert_eq!(lock.load(Ordering::Relaxed), 1);
        let ret = pthread_spin_unlock(&mut lock);
        assert_eq!(ret, 0);
        assert_eq!(lock.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn spin_unlock_null_returns_efault() {
        assert_eq!(pthread_spin_unlock(core::ptr::null_mut()), errno::EFAULT);
    }

    #[test]
    fn spin_lock_unlock_cycle() {
        let mut lock = AtomicI32::new(0);
        pthread_spin_init(&mut lock, 0);

        // Lock, unlock, lock again -- should succeed each time.
        assert_eq!(pthread_spin_trylock(&mut lock), 0);
        assert_eq!(pthread_spin_unlock(&mut lock), 0);
        assert_eq!(pthread_spin_trylock(&mut lock), 0);
        assert_eq!(lock.load(Ordering::Relaxed), 1);
        assert_eq!(pthread_spin_unlock(&mut lock), 0);
        assert_eq!(lock.load(Ordering::Relaxed), 0);
    }

    // =======================================================================
    // Cancel stubs
    // =======================================================================

    #[test]
    fn setcancelstate_returns_zero() {
        let mut old: i32 = -1;
        let ret = pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &mut old);
        assert_eq!(ret, 0);
        assert_eq!(old, PTHREAD_CANCEL_ENABLE); // Stub always reports ENABLE.
    }

    #[test]
    fn setcancelstate_null_oldstate_ok() {
        let ret = pthread_setcancelstate(PTHREAD_CANCEL_ENABLE, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn setcanceltype_returns_zero() {
        let mut old: i32 = -1;
        let ret = pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &mut old);
        assert_eq!(ret, 0);
        assert_eq!(old, PTHREAD_CANCEL_DEFERRED); // Stub always reports DEFERRED.
    }

    #[test]
    fn setcanceltype_null_oldtype_ok() {
        let ret = pthread_setcanceltype(PTHREAD_CANCEL_DEFERRED, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn testcancel_is_noop() {
        // Should simply not panic or crash.
        pthread_testcancel();
    }

    #[test]
    fn cancel_returns_enosys() {
        let ret = pthread_cancel(42);
        assert_eq!(ret, errno::ENOSYS);
    }

    // =======================================================================
    // Static initializers
    // =======================================================================

    #[test]
    fn mutex_initializer_is_unlocked() {
        let m = PTHREAD_MUTEX_INITIALIZER;
        assert_eq!(m.locked.load(Ordering::Relaxed), 0);
        assert_eq!(m.kind.load(Ordering::Relaxed), PTHREAD_MUTEX_NORMAL);
        assert_eq!(m.owner.load(Ordering::Relaxed), 0);
        assert_eq!(m.count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cond_initializer_is_zeroed() {
        // Cannot move out of a static with non-Copy fields; read via reference.
        assert_eq!(
            PTHREAD_COND_INITIALIZER.generation.load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn once_init_is_zeroed() {
        let o = PTHREAD_ONCE_INIT;
        assert_eq!(o.done.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn rwlock_initializer_is_unlocked() {
        // Cannot move out of a static with non-Copy fields; read via reference.
        assert_eq!(PTHREAD_RWLOCK_INITIALIZER.state.load(Ordering::Relaxed), 0);
    }

    // =======================================================================
    // Mutex destroy
    // =======================================================================

    #[test]
    fn mutex_destroy_null_returns_efault() {
        let ret = unsafe { pthread_mutex_destroy(core::ptr::null_mut()) };
        assert_eq!(ret, errno::EFAULT);
    }

    #[test]
    fn mutex_destroy_clears_locked() {
        let mut mutex = PTHREAD_MUTEX_INITIALIZER;
        mutex.locked.store(1, Ordering::Relaxed);
        let ret = unsafe { pthread_mutex_destroy(&mut mutex) };
        assert_eq!(ret, 0);
        assert_eq!(mutex.locked.load(Ordering::Relaxed), 0);
    }

    // =======================================================================
    // pthread_atfork stub
    // =======================================================================

    // These two register into the *process-global* atfork table, so they must
    // hold `ATFORK_TEST_LOCK` and reset afterwards, like the tests further down.
    // Without that they leaked handlers into a table that
    // `atfork_table_full_returns_enomem` fills exactly, so with more than one
    // test thread that test intermittently saw ENOMEM on a registration it
    // expected to succeed.
    #[test]
    fn atfork_returns_zero() {
        let _g = ATFORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        atfork_reset();
        assert_eq!(pthread_atfork(None, None, None), 0);
        atfork_reset();
    }

    extern "C" fn dummy_fork_handler() {}

    #[test]
    fn atfork_with_handlers_returns_zero() {
        let _g = ATFORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        atfork_reset();
        assert_eq!(
            pthread_atfork(
                Some(dummy_fork_handler),
                Some(dummy_fork_handler),
                Some(dummy_fork_handler)
            ),
            0
        );
        atfork_reset();
    }

    // =======================================================================
    // pthread_self
    // =======================================================================

    #[test]
    fn self_returns_thread_id() {
        // In test mode SYS_TASK_ID returns 0, which is a valid thread ID.
        let id = pthread_self();
        // The call should not crash and should return a consistent value.
        assert_eq!(id, pthread_self());
    }

    // =======================================================================
    // Mutex lock / trylock / unlock
    // =======================================================================

    #[test]
    fn mutex_lock_unlock_normal() {
        #[allow(clippy::declare_interior_mutable_const)]
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        assert_eq!(unsafe { pthread_mutex_lock(&mut m) }, 0);
        assert_eq!(unsafe { pthread_mutex_unlock(&mut m) }, 0);
    }

    #[test]
    fn mutex_trylock_uncontended() {
        #[allow(clippy::declare_interior_mutable_const)]
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        assert_eq!(unsafe { pthread_mutex_trylock(&mut m) }, 0);
        assert_eq!(unsafe { pthread_mutex_unlock(&mut m) }, 0);
    }

    #[test]
    fn mutex_trylock_locked_returns_ebusy() {
        #[allow(clippy::declare_interior_mutable_const)]
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        // Lock it first.
        assert_eq!(unsafe { pthread_mutex_lock(&mut m) }, 0);
        // Trylock on already-locked mutex from "different thread" perspective:
        // Since SYS_TASK_ID returns 0 and the owner is also 0, the normal
        // mutex type doesn't check ownership on trylock — it just sees
        // locked.state != 0. The CAS from 0→1 fails because state is already 1.
        // For a non-recursive, non-errorchecking mutex: EBUSY.
        // But owner == self_id for normal mutex doesn't matter, so the CAS
        // just fails with EBUSY.
        assert_eq!(unsafe { pthread_mutex_trylock(&mut m) }, errno::EBUSY);
        assert_eq!(unsafe { pthread_mutex_unlock(&mut m) }, 0);
    }

    #[test]
    fn mutex_lock_null_returns_efault() {
        assert_eq!(
            unsafe { pthread_mutex_lock(core::ptr::null_mut()) },
            errno::EFAULT
        );
    }

    #[test]
    fn mutex_trylock_null_returns_efault() {
        assert_eq!(
            unsafe { pthread_mutex_trylock(core::ptr::null_mut()) },
            errno::EFAULT
        );
    }

    #[test]
    fn mutex_unlock_null_returns_efault() {
        assert_eq!(
            unsafe { pthread_mutex_unlock(core::ptr::null_mut()) },
            errno::EFAULT
        );
    }

    #[test]
    fn mutex_recursive_lock_twice() {
        let mut m = PthreadMutexT {
            locked: AtomicI32::new(0),
            kind: AtomicI32::new(PTHREAD_MUTEX_RECURSIVE),
            owner: AtomicI32::new(0),
            count: AtomicI32::new(0),
            _pad: [0; 24],
        };
        // First lock.
        assert_eq!(unsafe { pthread_mutex_lock(&mut m) }, 0);
        // Second lock (recursive) — should succeed.
        assert_eq!(unsafe { pthread_mutex_lock(&mut m) }, 0);
        // First unlock decrements count.
        assert_eq!(unsafe { pthread_mutex_unlock(&mut m) }, 0);
        // Lock is still held (count was 2, now 1).
        assert_eq!(m.locked.load(core::sync::atomic::Ordering::Relaxed), 1);
        // Second unlock releases.
        assert_eq!(unsafe { pthread_mutex_unlock(&mut m) }, 0);
        assert_eq!(m.locked.load(core::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn mutex_errorcheck_double_lock_returns_edeadlk() {
        let mut m = PthreadMutexT {
            locked: AtomicI32::new(0),
            kind: AtomicI32::new(PTHREAD_MUTEX_ERRORCHECK),
            owner: AtomicI32::new(0),
            count: AtomicI32::new(0),
            _pad: [0; 24],
        };
        // First lock succeeds.
        assert_eq!(unsafe { pthread_mutex_lock(&mut m) }, 0);
        // Second lock from same thread: EDEADLK.
        assert_eq!(unsafe { pthread_mutex_lock(&mut m) }, errno::EDEADLK);
        // Unlock.
        assert_eq!(unsafe { pthread_mutex_unlock(&mut m) }, 0);
    }

    // =======================================================================
    // pthread_once
    // =======================================================================

    static mut ONCE_COUNTER: i32 = 0;

    extern "C" fn once_increment() {
        // SAFETY: single-threaded tests.
        unsafe {
            *core::ptr::addr_of_mut!(ONCE_COUNTER) += 1;
        }
    }

    #[test]
    fn once_calls_init_exactly_once() {
        let mut once = PTHREAD_ONCE_INIT;
        unsafe {
            *core::ptr::addr_of_mut!(ONCE_COUNTER) = 0;
        }
        assert_eq!(unsafe { pthread_once(&mut once, Some(once_increment)) }, 0);
        assert_eq!(unsafe { *core::ptr::addr_of!(ONCE_COUNTER) }, 1);
        // Second call should not invoke init again.
        assert_eq!(unsafe { pthread_once(&mut once, Some(once_increment)) }, 0);
        assert_eq!(unsafe { *core::ptr::addr_of!(ONCE_COUNTER) }, 1);
    }

    #[test]
    fn once_null_returns_efault() {
        assert_eq!(
            unsafe { pthread_once(core::ptr::null_mut(), Some(once_increment)) },
            errno::EFAULT
        );
    }

    // =======================================================================
    // Thread-specific data (TSD)
    // =======================================================================

    // The values are per thread, but the key table is the process's, and
    // `tsd_keys_run_out_and_come_back` takes every free key: the TSD tests
    // take this lock so none of them is starved by it.
    static TSD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn tsd_guard() -> std::sync::MutexGuard<'static, ()> {
        TSD_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn new_key(d: Option<extern "C" fn(*mut u8)>) -> PthreadKeyT {
        let mut key: PthreadKeyT = PthreadKeyT::MAX;
        assert_eq!(unsafe { pthread_key_create(&mut key, d) }, 0);
        key
    }

    #[test]
    fn tsd_create_set_get() {
        let _g = tsd_guard();
        let key = new_key(None);
        let val = 42u8;
        let p = core::ptr::addr_of!(val) as *mut u8;
        assert!(unsafe { pthread_getspecific(key) }.is_null());
        assert_eq!(unsafe { pthread_setspecific(key, p) }, 0);
        assert_eq!(unsafe { pthread_getspecific(key) }, p);
        assert_eq!(pthread_key_delete(key), 0);
    }

    /// Each thread has its own value -- and on the host too now: the old
    /// table keyed values by a task id every test thread shared.
    #[test]
    fn tsd_values_are_per_thread() {
        let _g = tsd_guard();
        let key = new_key(None);
        assert_eq!(unsafe { pthread_setspecific(key, 0x10 as *mut u8) }, 0);
        let other = std::thread::spawn(move || {
            let before = unsafe { pthread_getspecific(key) } as usize;
            assert_eq!(unsafe { pthread_setspecific(key, 0x20 as *mut u8) }, 0);
            (before, unsafe { pthread_getspecific(key) } as usize)
        })
        .join()
        .expect("thread");
        assert_eq!(other, (0, 0x20));
        assert_eq!(unsafe { pthread_getspecific(key) } as usize, 0x10);
        assert_eq!(pthread_key_delete(key), 0);
    }

    #[test]
    fn tsd_key_create_null_returns_efault() {
        let _g = tsd_guard();
        assert_eq!(
            unsafe { pthread_key_create(core::ptr::null_mut(), None) },
            errno::EFAULT
        );
    }

    /// Deleting a key not in use is EINVAL, as in glibc (it answered 0).
    #[test]
    fn tsd_key_delete_needs_a_key_in_use() {
        let _g = tsd_guard();
        let key = new_key(None);
        assert_eq!(pthread_key_delete(key), 0);
        assert_eq!(pthread_key_delete(key), errno::EINVAL);
        assert_eq!(pthread_key_delete(KEYS_MAX as PthreadKeyT), errno::EINVAL);
    }

    #[test]
    fn tsd_out_of_range_and_unused_keys() {
        let _g = tsd_guard();
        assert!(unsafe { pthread_getspecific(9999) }.is_null());
        assert_eq!(
            unsafe { pthread_setspecific(9999, core::ptr::null_mut()) },
            errno::EINVAL
        );
        let key = new_key(None);
        assert_eq!(pthread_key_delete(key), 0);
        assert_eq!(
            unsafe { pthread_setspecific(key, 0x1 as *mut u8) },
            errno::EINVAL,
            "a deleted key"
        );
    }

    /// A value set under a key that was deleted and created again is gone:
    /// the key's number moved on.
    #[test]
    fn tsd_recreated_key_hides_old_values() {
        let _g = tsd_guard();
        let key = new_key(None);
        assert_eq!(unsafe { pthread_setspecific(key, 0x30 as *mut u8) }, 0);
        assert_eq!(pthread_key_delete(key), 0);
        let again = new_key(None);
        assert_eq!(again, key, "the first free key is reused");
        assert!(unsafe { pthread_getspecific(again) }.is_null());
        assert_eq!(pthread_key_delete(again), 0);
    }

    /// Every key can be had; the next is EAGAIN; one deleted can be had
    /// again.  The old table never reused an index, so a program creating
    /// and deleting keys ran out after 64 creations.
    #[test]
    fn tsd_keys_run_out_and_come_back() {
        let _g = tsd_guard();
        let mut taken = Vec::new();
        loop {
            let mut key: PthreadKeyT = 0;
            match unsafe { pthread_key_create(&mut key, None) } {
                0 => taken.push(key),
                e => {
                    assert_eq!(e, errno::EAGAIN);
                    break;
                }
            }
            assert!(taken.len() <= KEYS_MAX);
        }
        let last = taken.pop().expect("at least one key");
        assert_eq!(pthread_key_delete(last), 0);
        assert_eq!(new_key(None), last);
        taken.push(last);
        for k in taken {
            assert_eq!(pthread_key_delete(k), 0);
        }
    }

    static TSD_DTOR_CALLS: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0);
    static TSD_DTOR_LAST: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

    extern "C" fn tsd_record_dtor(val: *mut u8) {
        TSD_DTOR_CALLS.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        TSD_DTOR_LAST.store(val as usize, core::sync::atomic::Ordering::SeqCst);
    }

    #[test]
    fn tsd_cleanup_runs_destructor_and_clears_value() {
        let _g = tsd_guard();
        TSD_DTOR_CALLS.store(0, core::sync::atomic::Ordering::SeqCst);
        TSD_DTOR_LAST.store(0, core::sync::atomic::Ordering::SeqCst);
        let key = new_key(Some(tsd_record_dtor));
        let val = 0xABu8;
        let val_ptr = core::ptr::addr_of!(val) as *mut u8;
        assert_eq!(unsafe { pthread_setspecific(key, val_ptr) }, 0);

        // Simulate thread exit for the calling (host) thread.
        tsd_thread_cleanup();
        assert_eq!(TSD_DTOR_CALLS.load(core::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            TSD_DTOR_LAST.load(core::sync::atomic::Ordering::SeqCst),
            val_ptr as usize
        );
        assert!(unsafe { pthread_getspecific(key) }.is_null());
        let pt = crate::perthread::current();
        assert!(
            unsafe { (*pt).tsd.iter().all(|b| b.is_null()) },
            "blocks freed"
        );

        // A value set under a key deleted since is dropped without its
        // destructor.
        assert_eq!(unsafe { pthread_setspecific(key, val_ptr) }, 0);
        assert_eq!(pthread_key_delete(key), 0);
        tsd_thread_cleanup();
        assert_eq!(TSD_DTOR_CALLS.load(core::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn tsd_cleanup_null_value_skips_destructor() {
        let _g = tsd_guard();
        TSD_DTOR_CALLS.store(0, core::sync::atomic::Ordering::SeqCst);
        let key = new_key(Some(tsd_record_dtor));
        // Never set a value (stays null) → destructor must not run.
        tsd_thread_cleanup();
        assert_eq!(TSD_DTOR_CALLS.load(core::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(pthread_key_delete(key), 0);
    }

    static TSD_REARM_KEY: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static TSD_REARM_CALLS: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0);

    extern "C" fn tsd_rearming_dtor(_: *mut u8) {
        TSD_REARM_CALLS.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        let key = TSD_REARM_KEY.load(core::sync::atomic::Ordering::SeqCst);
        // Set the key again, every time: the sweep repeats, but only
        // PTHREAD_DESTRUCTOR_ITERATIONS times.
        let _ = unsafe { pthread_setspecific(key, 0x40 as *mut u8) };
    }

    #[test]
    fn tsd_cleanup_repeats_for_rearmed_keys_but_not_forever() {
        let _g = tsd_guard();
        TSD_REARM_CALLS.store(0, core::sync::atomic::Ordering::SeqCst);
        let key = new_key(Some(tsd_rearming_dtor));
        TSD_REARM_KEY.store(key, core::sync::atomic::Ordering::SeqCst);
        assert_eq!(unsafe { pthread_setspecific(key, 0x40 as *mut u8) }, 0);
        tsd_thread_cleanup();
        assert_eq!(
            TSD_REARM_CALLS.load(core::sync::atomic::Ordering::SeqCst),
            PTHREAD_DESTRUCTOR_ITERATIONS
        );
        assert_eq!(pthread_key_delete(key), 0);
    }

    // =======================================================================
    // pthread_atfork
    // =======================================================================

    // The atfork table is process-global; serialize these tests so the
    // shared static isn't mutated concurrently by parallel test threads.
    static ATFORK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // Records the order in which handlers fire.
    static ATFORK_SEQ: std::sync::Mutex<Vec<i32>> = std::sync::Mutex::new(Vec::new());

    fn atfork_seq_push(v: i32) {
        ATFORK_SEQ.lock().unwrap_or_else(|e| e.into_inner()).push(v);
    }
    fn atfork_seq_take() -> Vec<i32> {
        let mut g = ATFORK_SEQ.lock().unwrap_or_else(|e| e.into_inner());
        core::mem::take(&mut *g)
    }

    fn atfork_reset() {
        atfork_seq_take();
        super::atfork_lock();
        // SAFETY: lock held; clearing the table for a clean test slate.
        unsafe {
            super::ATFORK_COUNT = 0;
            let t = &mut *core::ptr::addr_of_mut!(super::ATFORK_HANDLERS);
            for e in t.iter_mut() {
                *e = super::AtforkEntry::EMPTY;
            }
        }
        super::atfork_unlock();
    }

    extern "C" fn prep1() {
        atfork_seq_push(11);
    }
    extern "C" fn prep2() {
        atfork_seq_push(12);
    }
    extern "C" fn parent1() {
        atfork_seq_push(21);
    }
    extern "C" fn parent2() {
        atfork_seq_push(22);
    }
    extern "C" fn child1() {
        atfork_seq_push(31);
    }
    extern "C" fn child2() {
        atfork_seq_push(32);
    }

    #[test]
    fn atfork_prepare_lifo_parent_and_child_fifo() {
        let _g = ATFORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        atfork_reset();

        assert_eq!(pthread_atfork(Some(prep1), Some(parent1), Some(child1)), 0);
        assert_eq!(pthread_atfork(Some(prep2), Some(parent2), Some(child2)), 0);

        // prepare runs LIFO: second-registered first.
        atfork_run_prepare();
        assert_eq!(atfork_seq_take(), vec![12, 11]);

        // parent runs FIFO: registration order.
        atfork_run_parent();
        assert_eq!(atfork_seq_take(), vec![21, 22]);

        // child runs FIFO: registration order.
        atfork_run_child();
        assert_eq!(atfork_seq_take(), vec![31, 32]);

        atfork_reset();
    }

    #[test]
    fn atfork_null_handlers_are_skipped() {
        let _g = ATFORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        atfork_reset();

        // Only a prepare handler; parent/child are None.
        assert_eq!(pthread_atfork(Some(prep1), None, None), 0);
        atfork_run_prepare();
        assert_eq!(atfork_seq_take(), vec![11]);
        // Running parent/child with no registered handlers does nothing.
        atfork_run_parent();
        atfork_run_child();
        assert!(
            atfork_seq_take().is_empty(),
            "parent/child atfork handlers must run nothing when none are registered"
        );

        atfork_reset();
    }

    #[test]
    fn atfork_table_full_returns_enomem() {
        let _g = ATFORK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        atfork_reset();

        // Fill the table exactly.
        for _ in 0..super::MAX_ATFORK_HANDLERS {
            assert_eq!(pthread_atfork(Some(prep1), None, None), 0);
        }
        // One more must fail with ENOMEM.
        assert_eq!(pthread_atfork(Some(prep1), None, None), errno::ENOMEM);

        atfork_reset();
    }

    // =======================================================================
    // Spinlock operations
    // =======================================================================

    #[test]
    fn spin_lock_unlock() {
        let mut lock = AtomicI32::new(0);
        assert_eq!(pthread_spin_lock(&mut lock), 0);
        assert_eq!(lock.load(core::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(pthread_spin_unlock(&mut lock), 0);
        assert_eq!(lock.load(core::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn spin_trylock_uncontended() {
        let mut lock = AtomicI32::new(0);
        assert_eq!(pthread_spin_trylock(&mut lock), 0);
        assert_eq!(lock.load(core::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(pthread_spin_unlock(&mut lock), 0);
    }

    #[test]
    fn spin_trylock_locked_returns_ebusy() {
        let mut lock = AtomicI32::new(0);
        assert_eq!(pthread_spin_lock(&mut lock), 0);
        assert_eq!(pthread_spin_trylock(&mut lock), errno::EBUSY);
        assert_eq!(pthread_spin_unlock(&mut lock), 0);
    }

    #[test]
    fn spin_lock_null_returns_efault() {
        assert_eq!(pthread_spin_lock(core::ptr::null_mut()), errno::EFAULT);
    }

    // =======================================================================
    // RW lock operations
    // =======================================================================

    #[test]
    fn rwlock_rdlock_tryrdlock() {
        let mut rw = PthreadRwlockT {
            state: AtomicI32::new(0),
            waiters: AtomicI32::new(0),
            writer: AtomicI32::new(0),
            prefer_writer: AtomicI32::new(0),
            writers_waiting: AtomicI32::new(0),
            _pad: [0; 36],
        };
        // Read-lock.
        assert_eq!(pthread_rwlock_rdlock(&mut rw), 0);
        // Another read-lock should succeed (multiple readers).
        assert_eq!(pthread_rwlock_tryrdlock(&mut rw), 0);
        // State should be 2 (two readers).
        assert_eq!(rw.state.load(core::sync::atomic::Ordering::Relaxed), 2);
        // Unlock twice.
        assert_eq!(pthread_rwlock_unlock(&mut rw), 0);
        assert_eq!(pthread_rwlock_unlock(&mut rw), 0);
        assert_eq!(rw.state.load(core::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn rwlock_wrlock_trywrlock() {
        let mut rw = PthreadRwlockT {
            state: AtomicI32::new(0),
            waiters: AtomicI32::new(0),
            writer: AtomicI32::new(0),
            prefer_writer: AtomicI32::new(0),
            writers_waiting: AtomicI32::new(0),
            _pad: [0; 36],
        };
        // Write-lock.
        assert_eq!(pthread_rwlock_wrlock(&mut rw), 0);
        assert_eq!(rw.state.load(core::sync::atomic::Ordering::Relaxed), -1);
        // Try write-lock again — should fail.
        assert_eq!(pthread_rwlock_trywrlock(&mut rw), errno::EBUSY);
        // Try read-lock — should fail (writer holds lock).
        assert_eq!(pthread_rwlock_tryrdlock(&mut rw), errno::EBUSY);
        // Unlock.
        assert_eq!(pthread_rwlock_unlock(&mut rw), 0);
        assert_eq!(rw.state.load(core::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn rwlock_null_returns_efault() {
        assert_eq!(pthread_rwlock_rdlock(core::ptr::null_mut()), errno::EFAULT);
        assert_eq!(pthread_rwlock_wrlock(core::ptr::null_mut()), errno::EFAULT);
        assert_eq!(
            pthread_rwlock_tryrdlock(core::ptr::null_mut()),
            errno::EFAULT
        );
        assert_eq!(
            pthread_rwlock_trywrlock(core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    // =======================================================================
    // Condition variable signal/broadcast
    // =======================================================================

    #[test]
    fn cond_signal_increments_generation() {
        let mut cond = PthreadCondT {
            generation: AtomicI32::new(0),
            clock: 0,
            waiters: AtomicI32::new(0),
            _pad: [0; 36],
        };
        let gen_before = cond.generation.load(core::sync::atomic::Ordering::Relaxed);
        assert_eq!(pthread_cond_signal(&mut cond), 0);
        let gen_after = cond.generation.load(core::sync::atomic::Ordering::Relaxed);
        assert_eq!(gen_after, gen_before + 1);
    }

    #[test]
    fn cond_broadcast_increments_generation() {
        let mut cond = PthreadCondT {
            generation: AtomicI32::new(0),
            clock: 0,
            waiters: AtomicI32::new(0),
            _pad: [0; 36],
        };
        assert_eq!(pthread_cond_broadcast(&mut cond), 0);
        assert_eq!(
            cond.generation.load(core::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn cond_signal_null_returns_efault() {
        assert_eq!(pthread_cond_signal(core::ptr::null_mut()), errno::EFAULT);
    }

    #[test]
    fn cond_broadcast_null_returns_efault() {
        assert_eq!(pthread_cond_broadcast(core::ptr::null_mut()), errno::EFAULT);
    }

    #[test]
    fn cond_wait_null_returns_efault() {
        #[allow(clippy::declare_interior_mutable_const)]
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        assert_eq!(
            pthread_cond_wait(core::ptr::null_mut(), &mut m),
            errno::EFAULT
        );
        let mut c = PthreadCondT {
            generation: AtomicI32::new(0),
            clock: 0,
            waiters: AtomicI32::new(0),
            _pad: [0; 36],
        };
        assert_eq!(
            pthread_cond_wait(&mut c, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    #[test]
    fn cond_timedwait_null_returns_efault() {
        #[allow(clippy::declare_interior_mutable_const)]
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        let mut c = PthreadCondT {
            generation: AtomicI32::new(0),
            clock: 0,
            waiters: AtomicI32::new(0),
            _pad: [0; 36],
        };
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // Null cond.
        assert_eq!(
            pthread_cond_timedwait(core::ptr::null_mut(), &mut m, &ts),
            errno::EFAULT
        );
        // Null mutex.
        assert_eq!(
            pthread_cond_timedwait(&mut c, core::ptr::null_mut(), &ts),
            errno::EFAULT
        );
        // Null abstime.
        assert_eq!(
            pthread_cond_timedwait(&mut c, &mut m, core::ptr::null()),
            errno::EFAULT
        );
    }

    // =======================================================================
    // sched_yield
    // =======================================================================

    #[test]
    fn sched_yield_returns_zero() {
        assert_eq!(sched_yield(), 0);
    }

    // =======================================================================
    // pthread_setname_np / pthread_getname_np
    // =======================================================================

    #[test]
    fn test_pthread_setname_np_null() {
        let _g = thread_name_test_guard();
        let ret = unsafe { pthread_setname_np(0, core::ptr::null()) };
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_pthread_setname_np_too_long() {
        let _g = thread_name_test_guard();
        // PTHREAD_NAME_MAX is 16, so a 16-char name (excluding null) is too long.
        let name = b"0123456789abcdef\0";
        let ret = unsafe { pthread_setname_np(0, name.as_ptr()) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    #[test]
    fn test_pthread_setname_np_max_valid() {
        let _g = thread_name_test_guard();
        // 15 chars + null = exactly PTHREAD_NAME_MAX.
        let name = b"0123456789abcde\0";
        let ret = unsafe { pthread_setname_np(1, name.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_pthread_setname_np_short() {
        let _g = thread_name_test_guard();
        let name = b"main\0";
        let ret = unsafe { pthread_setname_np(2, name.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_pthread_getname_np_null() {
        let _g = thread_name_test_guard();
        let ret = unsafe { pthread_getname_np(0, core::ptr::null_mut(), 16) };
        assert_eq!(ret, crate::errno::EFAULT);
    }

    /// A zero length is `ERANGE`, not `EINVAL`: glibc's `__pthread_getname_np`
    /// (nptl/pthread_getname.c) has exactly one length test,
    /// `if (len < TASK_COMM_LEN) return ERANGE;`, and zero is simply the smallest
    /// value that fails it.  See `design-decisions.md` §303.
    #[test]
    fn test_pthread_getname_np_zero_len() {
        let _g = thread_name_test_guard();
        let mut buf = [0u8; 16];
        let ret = unsafe { pthread_getname_np(0, buf.as_mut_ptr(), 0) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    /// The length test precedes the buffer dereference, so a short buffer
    /// outranks a null one.  See `design-decisions.md` §303.
    #[test]
    fn test_pthread_getname_np_short_len_outranks_a_null_buffer() {
        let _g = thread_name_test_guard();
        let ret = unsafe { pthread_getname_np(0, core::ptr::null_mut(), 4) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    /// glibc requires `len >= TASK_COMM_LEN` *unconditionally* — the comparison
    /// is against the constant, never against the name's actual length.  So a
    /// 4-byte buffer is `ERANGE` even for a 2-character name, which is a case we
    /// used to accept.  See `design-decisions.md` §303.
    #[test]
    fn test_pthread_getname_np_short_len_rejected_even_when_the_name_would_fit() {
        let _g = thread_name_test_guard();
        let name = b"ab\0";
        assert_eq!(unsafe { pthread_setname_np(9, name.as_ptr()) }, 0);

        let mut buf = [0u8; 4];
        let ret = unsafe { pthread_getname_np(9, buf.as_mut_ptr(), 4) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    #[test]
    fn test_pthread_setname_getname_roundtrip() {
        let _g = thread_name_test_guard();
        let name = b"worker\0";
        let ret = unsafe { pthread_setname_np(3, name.as_ptr()) };
        assert_eq!(ret, 0);

        let mut buf = [0u8; 16];
        let ret = unsafe { pthread_getname_np(3, buf.as_mut_ptr(), 16) };
        assert_eq!(ret, 0);
        assert_eq!(&buf[..7], b"worker\0");
    }

    #[test]
    fn test_pthread_getname_np_buffer_too_small() {
        let _g = thread_name_test_guard();
        let name = b"longthreadname\0"; // 14 chars
        let _ = unsafe { pthread_setname_np(4, name.as_ptr()) };

        // 5 is below TASK_COMM_LEN, so it is rejected on entry — it would also
        // be too small for "longthreadname\0" (15 bytes), but glibc never gets
        // as far as comparing against the stored name.
        let mut buf = [0u8; 5];
        let ret = unsafe { pthread_getname_np(4, buf.as_mut_ptr(), 5) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    #[test]
    fn test_pthread_setname_empty() {
        let _g = thread_name_test_guard();
        let name = b"\0";
        let ret = unsafe { pthread_setname_np(5, name.as_ptr()) };
        assert_eq!(ret, 0);
        let mut buf = [0xFFu8; 16];
        let ret = unsafe { pthread_getname_np(5, buf.as_mut_ptr(), 16) };
        assert_eq!(ret, 0);
        assert_eq!(buf[0], 0, "Empty name should give empty string");
    }

    #[test]
    fn test_pthread_name_no_modulo_collision() {
        let _g = thread_name_test_guard();
        // Two task IDs that alias under the old `tid % MAX_NAMED_THREADS`
        // hash (100 and 100 + MAX_NAMED_THREADS ≡ same slot) must now keep
        // independent names.
        let a: PthreadT = 100;
        let b: PthreadT = 100 + MAX_NAMED_THREADS as PthreadT;
        let na = b"alpha\0";
        let nb = b"bravo\0";
        assert_eq!(unsafe { pthread_setname_np(a, na.as_ptr()) }, 0);
        assert_eq!(unsafe { pthread_setname_np(b, nb.as_ptr()) }, 0);

        let mut buf = [0u8; 16];
        assert_eq!(unsafe { pthread_getname_np(a, buf.as_mut_ptr(), 16) }, 0);
        assert_eq!(&buf[..5], b"alpha");
        let mut buf2 = [0u8; 16];
        assert_eq!(unsafe { pthread_getname_np(b, buf2.as_mut_ptr(), 16) }, 0);
        assert_eq!(&buf2[..5], b"bravo");

        // Cleanup so the slots don't accumulate across the suite.
        thread_name_release(a);
        thread_name_release(b);
    }

    #[test]
    fn test_pthread_name_release_clears_slot() {
        let _g = thread_name_test_guard();
        let t: PthreadT = 200;
        let n = b"worker\0";
        assert_eq!(unsafe { pthread_setname_np(t, n.as_ptr()) }, 0);
        let mut buf = [0u8; 16];
        assert_eq!(unsafe { pthread_getname_np(t, buf.as_mut_ptr(), 16) }, 0);
        assert_eq!(&buf[..6], b"worker");

        // After release, the (unnamed) thread reports the empty string.
        thread_name_release(t);
        let mut buf2 = [0xFFu8; 16];
        assert_eq!(unsafe { pthread_getname_np(t, buf2.as_mut_ptr(), 16) }, 0);
        assert_eq!(buf2[0], 0, "released slot should yield empty name");
    }

    #[test]
    fn test_pthread_getname_unset_thread_is_empty() {
        let _g = thread_name_test_guard();
        // A thread ID that was never named yields the empty string (not an
        // error), regardless of any stale data at the old modulo slot.
        let mut buf = [0xFFu8; 16];
        assert_eq!(
            unsafe { pthread_getname_np(54321, buf.as_mut_ptr(), 16) },
            0
        );
        assert_eq!(buf[0], 0);
    }

    // -----------------------------------------------------------------------
    // pthread_create — error paths
    // -----------------------------------------------------------------------

    // Note: We can't fully test pthread_create because the kernel syscall
    // goes to the Windows kernel in test mode.  We can test that it returns
    // a non-crashing value when called (the mmap + syscall may fail).

    #[test]
    fn test_pthread_create_no_crash() {
        extern "C" fn dummy(_arg: *mut u8) -> *mut u8 {
            core::ptr::null_mut()
        }
        let mut tid: PthreadT = 0;
        // This will likely fail (EAGAIN) because the kernel syscall
        // is meaningless on Windows, but must not crash.
        let _ret = pthread_create(
            &raw mut tid,
            core::ptr::null(),
            Some(dummy),
            core::ptr::null_mut(),
        );
    }

    // -----------------------------------------------------------------------
    // pthread_join — error paths
    // -----------------------------------------------------------------------

    #[test]
    fn test_pthread_join_invalid_thread() {
        // Joining a nonexistent thread — syscall returns unpredictable
        // values on test host, so accept either 0 or ESRCH.
        let mut retval: *mut u8 = core::ptr::null_mut();
        let ret = pthread_join(0xDEAD_BEEF, &raw mut retval);
        assert!(
            ret == 0 || ret == crate::errno::ESRCH,
            "expected 0 or ESRCH, got {ret}"
        );
    }

    #[test]
    fn test_pthread_join_null_retval() {
        // Null retval pointer should be fine (just don't store the value).
        // Syscall result is unpredictable on test host.
        let ret = pthread_join(0xDEAD_BEEF, core::ptr::null_mut());
        assert!(
            ret == 0 || ret == crate::errno::ESRCH,
            "expected 0 or ESRCH, got {ret}"
        );
    }

    // -----------------------------------------------------------------------
    // pthread_detach — error paths
    // -----------------------------------------------------------------------

    #[test]
    fn test_pthread_detach_nonexistent() {
        let ret = pthread_detach(0xDEAD_BEEF);
        assert_eq!(ret, crate::errno::ESRCH);
    }

    // -----------------------------------------------------------------------
    // Thread-table state machine (detach / exit / join arbitration)
    //
    // These exercise the lock-free ownership protocol directly with unique
    // synthetic task ids so they don't collide across the shared static
    // THREAD_TABLE (tests may run in parallel).  Each test releases the
    // slots it claims.
    // -----------------------------------------------------------------------

    /// Claim, fill and publish a slot for a synthetic thread, as
    /// `pthread_create` does.  `map_base` 0 keeps the reclaim paths from
    /// calling the host's `munmap`.
    fn track(tid: u64, map_base: usize, map_size: usize, detached: bool) -> &'static ThreadSlot {
        let slot = claim_slot().expect("a free slot");
        fill_slot(
            slot,
            &ThreadRecord {
                map_base,
                map_size,
                stack_base: map_base,
                stack_size: DEFAULT_THREAD_STACK_SIZE,
                guard_size: 0,
                detached,
            },
        );
        slot.task_id.store(tid, Ordering::Release);
        slot
    }

    /// A private table for the growth tests, so they cannot starve the
    /// process table other tests are using.
    fn private_table() -> &'static ThreadChunk {
        Box::leak(Box::new(ThreadChunk::new()))
    }

    /// Grow a private table from the heap, as `grow_table` does from mmap.
    fn grow_on_heap(last: &'static ThreadChunk) -> Option<&'static ThreadChunk> {
        Some(link_chunk(last, private_table()).unwrap_or_else(|theirs| theirs))
    }

    #[test]
    fn test_thread_slot_store_find_release() {
        let tid: u64 = 0x5100_0001;
        let slot = track(tid, 0x1_0000, DEFAULT_THREAD_STACK_SIZE + 0x80, false);
        let found = find_slot(tid).expect("slot should be found after store");
        assert!(core::ptr::eq(found, slot));
        assert_eq!(slot.map_base.load(Ordering::Relaxed), 0x1_0000);
        assert_eq!(
            slot.stack_size.load(Ordering::Relaxed),
            DEFAULT_THREAD_STACK_SIZE
        );
        // The mapping is larger than the usable stack: the TLS block and TCB
        // live above it in the same region, and the whole region is what gets
        // unmapped.
        assert_eq!(
            slot.map_size.load(Ordering::Relaxed),
            DEFAULT_THREAD_STACK_SIZE + 0x80
        );
        assert_eq!(slot.state.load(Ordering::Relaxed), STATE_JOINABLE);
        release_slot(slot);
        assert!(
            find_slot(tid).is_none(),
            "slot should be free after release"
        );
    }

    #[test]
    fn test_detach_marks_state_detached() {
        let tid: u64 = 0x5100_0002;
        let slot = track(tid, 0, DEFAULT_THREAD_STACK_SIZE, false);
        assert_eq!(pthread_detach(tid), 0);
        assert_eq!(slot.state.load(Ordering::Acquire), STATE_DETACHED);
        // The exit path's CAS(JOINABLE -> EXITED) must now lose to DETACHED,
        // steering the exiting thread onto the self-unmap branch.
        let cas = slot.state.compare_exchange(
            STATE_JOINABLE,
            STATE_EXITED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        assert_eq!(cas, Err(STATE_DETACHED));
        release_slot(slot);
    }

    #[test]
    fn test_double_detach_is_einval() {
        let tid: u64 = 0x5100_0003;
        let slot = track(tid, 0, DEFAULT_THREAD_STACK_SIZE, false);
        assert_eq!(pthread_detach(tid), 0);
        assert_eq!(pthread_detach(tid), crate::errno::EINVAL);
        release_slot(slot);
    }

    /// A thread created detached is detached from its first instant: a
    /// `pthread_detach` of it is a double detach, and it cannot be joined.
    #[test]
    fn test_created_detached_thread_refuses_detach_and_join() {
        let tid: u64 = 0x5100_0006;
        let slot = track(tid, 0, DEFAULT_THREAD_STACK_SIZE, true);
        assert_eq!(slot.state.load(Ordering::Acquire), STATE_DETACHED);
        assert_eq!(pthread_detach(tid), crate::errno::EINVAL);
        let mut rv: *mut u8 = core::ptr::null_mut();
        assert_eq!(pthread_join(tid, &raw mut rv), crate::errno::EINVAL);
        release_slot(slot);
    }

    #[test]
    fn test_join_rejects_detached_thread() {
        let tid: u64 = 0x5100_0004;
        let slot = track(tid, 0, DEFAULT_THREAD_STACK_SIZE, false);
        assert_eq!(pthread_detach(tid), 0);
        // Joining a detached thread must be rejected before any syscall.
        let mut rv: *mut u8 = core::ptr::null_mut();
        assert_eq!(pthread_join(tid, &raw mut rv), crate::errno::EINVAL);
        release_slot(slot);
    }

    #[test]
    fn tryjoin_answers_by_the_threads_state() {
        let tid: u64 = 0x5100_0020;
        let slot = track(tid, 0, DEFAULT_THREAD_STACK_SIZE, false);
        let mut rv: *mut u8 = core::ptr::null_mut();
        assert_eq!(
            pthread_tryjoin_np(tid, &raw mut rv),
            crate::errno::EBUSY,
            "still running"
        );
        // Exited: the join goes ahead, and its answer is pthread_join's --
        // on the host, whose kernel call fails, ESRCH.
        slot.state.store(STATE_EXITED, Ordering::Release);
        assert_eq!(
            pthread_tryjoin_np(tid, &raw mut rv),
            pthread_join(tid, &raw mut rv)
        );
        if let Some(s) = find_slot(tid) {
            release_slot(s);
        }
        let detached: u64 = 0x5100_0021;
        let slot = track(detached, 0, DEFAULT_THREAD_STACK_SIZE, true);
        assert_eq!(
            pthread_tryjoin_np(detached, &raw mut rv),
            crate::errno::EINVAL
        );
        release_slot(slot);
        assert_eq!(
            pthread_tryjoin_np(0x5100_00ff, &raw mut rv),
            crate::errno::ESRCH
        );
    }

    #[test]
    fn timedjoin_waits_until_its_time_and_no_longer() {
        use crate::stat::Timespec;
        let tid: u64 = 0x5100_0022;
        let slot = track(tid, 0, DEFAULT_THREAD_STACK_SIZE, false);
        let mut rv: *mut u8 = core::ptr::null_mut();
        let at = |tv_sec, tv_nsec| Timespec { tv_sec, tv_nsec };
        // SAFETY: each `abstime` is this frame's.
        unsafe {
            assert_eq!(
                pthread_timedjoin_np(tid, &raw mut rv, &at(0, 0)),
                crate::errno::ETIMEDOUT,
                "1970 has passed"
            );
            assert_eq!(
                pthread_timedjoin_np(tid, &raw mut rv, &at(0, 1_000_000_000)),
                crate::errno::EINVAL
            );
        }
        let mut now = Timespec::default();
        assert_eq!(
            crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut now),
            0
        );
        let soon = if now.tv_nsec < 970_000_000 {
            at(now.tv_sec, now.tv_nsec + 30_000_000)
        } else {
            at(now.tv_sec + 1, now.tv_nsec - 970_000_000)
        };
        let started = std::time::Instant::now();
        // SAFETY: as above.
        assert_eq!(
            unsafe { pthread_timedjoin_np(tid, &raw mut rv, &soon) },
            crate::errno::ETIMEDOUT
        );
        assert!(started.elapsed() >= std::time::Duration::from_millis(25));
        // Gone already: joined whatever `abstime` says.
        slot.state.store(STATE_EXITED, Ordering::Release);
        // SAFETY: as above.
        let r = unsafe { pthread_timedjoin_np(tid, &raw mut rv, &at(0, -1)) };
        assert_eq!(r, pthread_join(tid, &raw mut rv));
        if let Some(s) = find_slot(tid) {
            release_slot(s);
        }
    }

    #[test]
    fn a_thread_joining_itself_is_edeadlk() {
        // Give this host thread a task id, as the target's first lookup would.
        let pt = crate::perthread::current();
        // SAFETY: this thread's own block.
        let saved = unsafe { (*pt).tid };
        // SAFETY: as above.
        unsafe { (*pt).tid = 0x5100_0030 };
        let mut rv: *mut u8 = core::ptr::null_mut();
        assert_eq!(
            pthread_join(0x5100_0030, &raw mut rv),
            crate::errno::EDEADLK
        );
        assert_eq!(
            pthread_tryjoin_np(0x5100_0030, &raw mut rv),
            crate::errno::EDEADLK
        );
        let later = crate::stat::Timespec {
            tv_sec: i64::MAX,
            tv_nsec: 0,
        };
        // SAFETY: `later` is this frame's.
        assert_eq!(
            unsafe { pthread_timedjoin_np(0x5100_0030, &raw mut rv, &later) },
            crate::errno::EDEADLK
        );
        // SAFETY: as above.
        unsafe { (*pt).tid = saved };
    }

    #[test]
    fn test_detach_after_joinable_exit_reaps() {
        let tid: u64 = 0x5100_0005;
        let slot = track(tid, 0, DEFAULT_THREAD_STACK_SIZE, false);
        // Simulate the exit path winning the race: it marks the slot EXITED
        // and leaves the mapping for a reaper.
        let cas = slot.state.compare_exchange(
            STATE_JOINABLE,
            STATE_EXITED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        assert!(cas.is_ok());
        // A late pthread_detach must observe EXITED, reap, and free the slot.
        assert_eq!(pthread_detach(tid), 0);
        assert!(
            find_slot(tid).is_none(),
            "detach-after-exit must release the slot"
        );
    }

    /// The sentinels are never a thread's id.  Until 2026-09-26 an id of 0
    /// matched an empty slot, and `pthread_detach(0)` marked it detached and
    /// answered 0.
    #[test]
    fn test_find_slot_never_matches_a_sentinel() {
        let table = private_table();
        let slot = claim_slot_in(table, |_| None).expect("an empty private table");
        assert_eq!(slot.task_id.load(Ordering::Relaxed), SLOT_RESERVED);
        assert!(find_slot_in(table, SLOT_RESERVED).is_none());
        assert!(find_slot_in(table, SLOT_EMPTY).is_none());
        assert_eq!(pthread_detach(0), crate::errno::ESRCH);
        assert_eq!(pthread_detach(SLOT_RESERVED), crate::errno::ESRCH);
    }

    /// A zero-filled chunk -- what `grow_table` gets from mmap -- is an empty
    /// chunk.
    #[test]
    fn test_a_zeroed_chunk_is_empty() {
        // SAFETY: every field is an atomic integer or pointer, for which
        // all-zero is a valid value.
        let chunk: ThreadChunk = unsafe { core::mem::zeroed() };
        for slot in &chunk.slots {
            assert_eq!(slot.task_id.load(Ordering::Relaxed), SLOT_EMPTY);
            assert_eq!(slot.state.load(Ordering::Relaxed), STATE_JOINABLE);
        }
        assert!(chunk.next.load(Ordering::Relaxed).is_null());
    }

    /// The table grows past a chunk instead of refusing the 65th thread, and
    /// every slot keeps its address.
    #[test]
    fn test_the_table_grows_a_chunk_at_a_time() {
        let table = private_table();
        let mut claimed = Vec::new();
        for _ in 0..CHUNK_SLOTS * 2 + 1 {
            claimed.push(claim_slot_in(table, grow_on_heap).expect("growth"));
        }
        assert_eq!(chunks_from(table).count(), 3);
        for (i, slot) in claimed.iter().enumerate() {
            slot.task_id
                .store(0x5200_0000 + i as u64, Ordering::Release);
        }
        for (i, slot) in claimed.iter().enumerate() {
            let found = find_slot_in(table, 0x5200_0000 + i as u64).expect("tracked");
            assert!(core::ptr::eq(found, *slot));
        }
        // A released slot in the first chunk is reused before the table grows
        // again.
        release_slot(claimed[5]);
        let again = claim_slot_in(table, |_| None).expect("the freed slot");
        assert!(core::ptr::eq(again, claimed[5]));
    }

    /// Two threads that both find the table full link one chunk between
    /// them; the loser uses the winner's.
    #[test]
    fn test_a_lost_growth_race_uses_the_winners_chunk() {
        let table = private_table();
        let first = private_table();
        assert!(link_chunk(table, first).is_ok());
        let second = private_table();
        let Err(lost) = link_chunk(table, second) else {
            panic!("a second chunk linked where one already was");
        };
        assert!(core::ptr::eq(lost, first));
    }

    /// A NULL attribute and a freshly initialised one ask for the same
    /// thread -- which is why `pthread_attr_init` must record the guard.
    #[test]
    fn test_create_attr_defaults() {
        assert_eq!(CreateAttr::read(core::ptr::null()), CreateAttr::DEFAULT);
        let mut buf: PthreadAttrT = [0; 56];
        assert_eq!(pthread_attr_init(&mut buf), 0);
        assert_eq!(CreateAttr::read(&buf), CreateAttr::DEFAULT);
        assert_eq!(CreateAttr::DEFAULT.guard_size, crate::unistd::PAGE_SIZE);
    }

    #[test]
    fn test_create_attr_reads_what_the_setters_stored() {
        let mut buf: PthreadAttrT = [0; 56];
        assert_eq!(pthread_attr_init(&mut buf), 0);
        assert_eq!(pthread_attr_setstacksize(&mut buf, 2 << 20), 0);
        assert_eq!(pthread_attr_setguardsize(&mut buf, 0), 0);
        assert_eq!(
            pthread_attr_setdetachstate(&mut buf, PTHREAD_CREATE_DETACHED),
            0
        );
        assert_eq!(
            CreateAttr::read(&buf),
            CreateAttr {
                stack_size: 2 << 20,
                guard_size: 0,
                stack_addr: None,
                detached: true,
                explicit_sched: None,
                affinity: None,
            }
        );
        assert_eq!(
            pthread_attr_setstack(&mut buf, 0x4000_0000 as *mut core::ffi::c_void, 1 << 20),
            0
        );
        let got = CreateAttr::read(&buf);
        assert_eq!(got.stack_addr, Some(0x4000_0000));
        assert_eq!(got.stack_size, 1 << 20);
    }

    /// A TLS image with a block, so the layout arithmetic has something to
    /// place.
    const TEST_TLS: crate::tls::TlsImage = crate::tls::TlsImage {
        init_vaddr: 0,
        init_size: 8,
        mem_size: 24,
        align: 8,
    };

    /// Our own stack: guard at the bottom, then at least the stack asked for,
    /// then the TLS part -- all inside the mapping.
    #[test]
    fn test_plan_places_guard_stack_and_tls_in_one_mapping() {
        let page = crate::unistd::PAGE_SIZE;
        let want = CreateAttr {
            stack_size: 100_000,
            guard_size: 1,
            stack_addr: None,
            detached: false,
            explicit_sched: None,
            affinity: None,
        };
        let plan = plan_thread(&want, &TEST_TLS).expect("fits");
        assert_eq!(plan.guard, page, "the guard rounds up to a page");
        assert_eq!(plan.stack, 100_000usize.next_multiple_of(page));
        assert_eq!(plan.map_size % page, 0);
        let base = 0x10_0000_0000;
        let l = plan.place(base, &TEST_TLS);
        assert_eq!(l.stack_base, base + page);
        assert_eq!(l.stack_top % 16, 0);
        assert!(l.stack_top - l.stack_base >= 100_000);
        let block = TEST_TLS.block_size() as usize;
        assert!(l.stack_top <= l.tp as usize - block);
        let end =
            l.tp as usize + crate::tls::TCB_SIZE as usize + crate::perthread::BLOCK_SIZE as usize;
        assert!(end <= base + plan.map_size);
    }

    #[test]
    fn test_plan_without_a_guard() {
        let want = CreateAttr {
            guard_size: 0,
            ..CreateAttr::DEFAULT
        };
        let plan = plan_thread(&want, &TEST_TLS).expect("fits");
        assert_eq!(plan.guard, 0);
        assert_eq!(plan.place(0x20_0000, &TEST_TLS).stack_base, 0x20_0000);
    }

    /// A caller's stack is used as given; only the TLS part is mapped, and
    /// the guard is ignored, as POSIX requires.
    #[test]
    fn test_plan_on_the_callers_stack() {
        let want = CreateAttr {
            stack_size: 0x1_0000,
            guard_size: 0x4000,
            stack_addr: Some(0x7000_0008),
            detached: false,
            explicit_sched: None,
            affinity: None,
        };
        let plan = plan_thread(&want, &TEST_TLS).expect("fits");
        assert_eq!(plan.guard, 0);
        assert_eq!(plan.stack, 0);
        let tls = TEST_TLS.reserve() as usize;
        assert_eq!(
            plan.map_size,
            tls.next_multiple_of(crate::unistd::PAGE_SIZE)
        );
        let base = 0x30_0000_0000;
        let l = plan.place(base, &TEST_TLS);
        assert_eq!(l.stack_base, 0x7000_0008);
        assert_eq!(l.stack_top, (0x7000_0008 + 0x1_0000) & !0xf);
        assert!(l.tp as usize >= base && (l.tp as usize) < base + plan.map_size);
    }

    #[test]
    fn test_plan_refuses_sizes_that_overflow() {
        let huge = CreateAttr {
            stack_size: usize::MAX - 10,
            ..CreateAttr::DEFAULT
        };
        assert!(plan_thread(&huge, &TEST_TLS).is_none());
        let huge_guard = CreateAttr {
            guard_size: usize::MAX,
            ..CreateAttr::DEFAULT
        };
        assert!(plan_thread(&huge_guard, &TEST_TLS).is_none());
    }

    /// On the host there is no thread to create: `pthread_create` fails
    /// cleanly and gives its slot back.
    #[test]
    fn test_failed_create_leaves_no_slot_behind() {
        extern "C" fn never(_: *mut u8) -> *mut u8 {
            core::ptr::null_mut()
        }
        let mut t: PthreadT = 0;
        let reserved_before = chunks_from(&THREAD_TABLE)
            .flat_map(|c| c.slots.iter())
            .filter(|s| s.task_id.load(Ordering::Relaxed) == SLOT_RESERVED)
            .count();
        assert_eq!(
            pthread_create(
                &mut t,
                core::ptr::null(),
                Some(never),
                core::ptr::null_mut()
            ),
            crate::errno::EAGAIN
        );
        assert_eq!(t, 0);
        let reserved_after = chunks_from(&THREAD_TABLE)
            .flat_map(|c| c.slots.iter())
            .filter(|s| s.task_id.load(Ordering::Relaxed) == SLOT_RESERVED)
            .count();
        // Other tests may hold reservations of their own at this moment, but
        // never more than before plus theirs; this call's is gone.
        assert!(reserved_after <= reserved_before + 1);
    }

    // -----------------------------------------------------------------------
    // pthread_barrier_wait — null pointer
    // -----------------------------------------------------------------------

    #[test]
    fn test_pthread_barrier_wait_null() {
        let ret = pthread_barrier_wait(core::ptr::null_mut());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    // -----------------------------------------------------------------------
    // pthread_mutex_timedlock — null pointer, error paths
    // -----------------------------------------------------------------------

    #[test]
    fn test_pthread_mutex_timedlock_null_mutex() {
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ret = pthread_mutex_timedlock(core::ptr::null_mut(), &ts);
        assert_eq!(ret, crate::errno::EFAULT);
    }

    /// A NULL deadline is looked at only when the lock must be waited for:
    /// an uncontended lock succeeds, as glibc's does, and a contended one is
    /// EFAULT where glibc's `__lll_clocklock_wait` would fault reading it.
    #[test]
    fn test_pthread_mutex_timedlock_null_abstime() {
        // SAFETY: zero-init is valid for PthreadMutexT (all-zeros = unlocked).
        let mut m: PthreadMutexT = unsafe { core::mem::zeroed() };
        unsafe {
            pthread_mutex_init(&raw mut m, core::ptr::null());
        }
        assert_eq!(pthread_mutex_timedlock(&raw mut m, core::ptr::null()), 0);
        // Held now, and a normal mutex does not look for its owner: this
        // call would have to wait.
        assert_eq!(
            pthread_mutex_timedlock(&raw mut m, core::ptr::null()),
            crate::errno::EFAULT
        );
        assert_eq!(unsafe { pthread_mutex_unlock(&raw mut m) }, 0);
    }

    #[test]
    fn test_pthread_mutex_timedlock_unlocked() {
        // timedlock on an unlocked mutex should succeed immediately.
        // SAFETY: zero-init is valid for PthreadMutexT (all-zeros = unlocked).
        let mut m: PthreadMutexT = unsafe { core::mem::zeroed() };
        unsafe {
            pthread_mutex_init(&raw mut m, core::ptr::null());
        }
        let ts = crate::stat::Timespec {
            tv_sec: 999_999,
            tv_nsec: 0,
        };
        let ret = pthread_mutex_timedlock(&raw mut m, &ts);
        assert_eq!(ret, 0, "timedlock on unlocked mutex should succeed");
        // Unlock.
        unsafe {
            pthread_mutex_unlock(&raw mut m);
        }
    }

    /// glibc checks the deadline only once it is about to block
    /// (`nptl/pthread_mutex_timedlock.c:221`, "We are about to block; check
    /// whether the timeout is invalid"), so an uncontended acquisition never
    /// looks at `tv_nsec` and a malformed one is not an error.  POSIX allows
    /// this: "the validity of the abstime parameter need not be checked if
    /// the lock can be immediately acquired."
    #[test]
    fn test_pthread_mutex_timedlock_uncontended_ignores_a_bad_deadline() {
        // SAFETY: zero-init is valid for PthreadMutexT (all-zeros = unlocked).
        let mut m: PthreadMutexT = unsafe { core::mem::zeroed() };
        unsafe {
            pthread_mutex_init(&raw mut m, core::ptr::null());
        }
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000, // exactly out of range
        };
        assert_eq!(
            pthread_mutex_timedlock(&raw mut m, &ts),
            0,
            "the fast path must not consult the timespec"
        );
        unsafe {
            pthread_mutex_unlock(&raw mut m);
        }
    }

    /// The other half of the same rule: once the fast path fails, the very
    /// next thing glibc does is reject the deadline, so a contended
    /// `timedlock` with a malformed `tv_nsec` is `EINVAL` and never spins.
    #[test]
    fn test_pthread_mutex_timedlock_contended_rejects_a_bad_deadline() {
        // SAFETY: zero-init is valid for PthreadMutexT (all-zeros = unlocked).
        let mut m: PthreadMutexT = unsafe { core::mem::zeroed() };
        unsafe {
            pthread_mutex_init(&raw mut m, core::ptr::null());
            // A NORMAL mutex, so re-locking from this thread contends rather
            // than recursing or reporting EDEADLK — the fast path fails and
            // we reach the deadline check without ever blocking.
            assert_eq!(pthread_mutex_lock(&raw mut m), 0);
        }
        for bad in [-1i64, 1_000_000_000, i64::MAX] {
            let ts = crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: bad,
            };
            assert_eq!(
                pthread_mutex_timedlock(&raw mut m, &ts),
                crate::errno::EINVAL,
                "tv_nsec={bad} must be EINVAL once the lock is contended"
            );
        }
        unsafe {
            pthread_mutex_unlock(&raw mut m);
        }
    }

    /// `___pthread_cond_timedwait64` (nptl/pthread_cond_wait.c:635) checks
    /// the deadline as its first statement, so — unlike `timedlock` — there
    /// is no fast path that skips it, and the mutex is still held on return.
    #[test]
    fn test_pthread_cond_timedwait_rejects_a_bad_deadline_before_unlocking() {
        // SAFETY: zero-init is valid for both types (all-zeros = unlocked /
        // generation 0).
        let mut m: PthreadMutexT = unsafe { core::mem::zeroed() };
        let mut c: PthreadCondT = unsafe { core::mem::zeroed() };
        unsafe {
            pthread_mutex_init(&raw mut m, core::ptr::null());
        }
        pthread_cond_init(&raw mut c, core::ptr::null());
        unsafe {
            assert_eq!(pthread_mutex_lock(&raw mut m), 0);
        }
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        };
        assert_eq!(
            pthread_cond_timedwait(&raw mut c, &raw mut m, &ts),
            crate::errno::EINVAL
        );
        // The rejection must be side-effect-free: glibc returns before the
        // mutex is released, so the caller still owns it.
        assert_eq!(
            unsafe { pthread_mutex_trylock(&raw mut m) },
            crate::errno::EBUSY,
            "a rejected timedwait must not have dropped the mutex"
        );
        unsafe {
            pthread_mutex_unlock(&raw mut m);
        }
    }

    /// `valid_nanoseconds` looks only at `tv_nsec`, so a deadline already in
    /// the past is a *timeout*, not a malformed argument.  This pins the
    /// boundary that separates the two, and is the reason we cannot reuse
    /// the kernel's stricter `timespec64_valid` here.
    #[test]
    fn test_pthread_cond_timedwait_negative_tv_sec_is_etimedout_not_einval() {
        // SAFETY: zero-init is valid for both types.
        let mut m: PthreadMutexT = unsafe { core::mem::zeroed() };
        let mut c: PthreadCondT = unsafe { core::mem::zeroed() };
        unsafe {
            pthread_mutex_init(&raw mut m, core::ptr::null());
        }
        pthread_cond_init(&raw mut c, core::ptr::null());
        unsafe {
            assert_eq!(pthread_mutex_lock(&raw mut m), 0);
        }
        let ts = crate::stat::Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        assert_eq!(
            pthread_cond_timedwait(&raw mut c, &raw mut m, &ts),
            crate::errno::ETIMEDOUT
        );
        unsafe {
            pthread_mutex_unlock(&raw mut m);
        }
    }

    // -----------------------------------------------------------------------
    // pthread_setaffinity_np / pthread_getaffinity_np
    // -----------------------------------------------------------------------

    #[test]
    fn test_pthread_setaffinity_np_null_cpuset() {
        let ret = pthread_setaffinity_np(0, core::mem::size_of::<CpuSetT>(), core::ptr::null());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    /// A short mask is zero-extended, as the kernel reads one: one byte
    /// holding the host's one CPU is every CPU; one holding none is `EINVAL`.
    #[test]
    fn test_pthread_setaffinity_np_small_size() {
        let mut set = empty_set();
        assert_eq!(pthread_setaffinity_np(0, 1, &set), crate::errno::EINVAL);
        set.bits[0] = 1;
        assert_eq!(pthread_setaffinity_np(0, 1, &set), 0);
        assert_eq!(pthread_setaffinity_np(0, 8, &set), 0);
    }

    #[test]
    fn test_pthread_setaffinity_np_success() {
        let mut set = empty_set();
        set.bits[0] = 1;
        let ret = pthread_setaffinity_np(0, core::mem::size_of::<CpuSetT>(), &set);
        assert_eq!(ret, 0);
    }

    fn empty_set() -> CpuSetT {
        CpuSetT { bits: [0; 16] }
    }

    #[test]
    fn test_pthread_getaffinity_np_null_cpuset() {
        let ret = pthread_getaffinity_np(0, core::mem::size_of::<CpuSetT>(), core::ptr::null_mut());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    /// One byte is not a whole `unsigned long`: `EINVAL`.  Eight are, and
    /// hold the host's one CPU, as `CPU_ALLOC_SIZE(1)` asks.
    #[test]
    fn test_pthread_getaffinity_np_small_size() {
        let mut set = empty_set();
        let ret = pthread_getaffinity_np(0, 1, &raw mut set);
        assert_eq!(ret, crate::errno::EINVAL);
        set.bits = [u64::MAX; 16];
        assert_eq!(pthread_getaffinity_np(0, 8, &raw mut set), 0);
        assert_eq!(set.bits[0], 1, "the host's one CPU");
        assert_eq!(set.bits[1], u64::MAX, "past the 8 bytes, left alone");
    }

    /// A length that is not a whole number of `unsigned long`s is `EINVAL` —
    /// `sched_getaffinity`'s second test, `len & (sizeof (unsigned long) - 1)`
    /// (kernel/sched/core.c:8506-8509).  It has no counterpart in
    /// `sched_setaffinity`.  See `design-decisions.md` §303.
    #[test]
    fn test_pthread_getaffinity_np_unaligned_size() {
        let mut set = empty_set();
        let unaligned = core::mem::size_of::<CpuSetT>() + 1;
        let ret = pthread_getaffinity_np(0, unaligned, &raw mut set);
        assert_eq!(ret, crate::errno::EINVAL);
    }

    /// The two calls order their checks *oppositely*, and that asymmetry is
    /// Linux's, not ours: `sched_getaffinity` rejects the length above its
    /// `copy_to_user`, while `sched_setaffinity`'s `get_user_cpu_mask`
    /// (kernel/sched/core.c:8429) copies first and never rejects a size.  So a
    /// null mask with a short length is `EINVAL` on the get side and `EFAULT` on
    /// the set side.  See `design-decisions.md` §303.
    #[test]
    fn test_affinity_np_orders_its_checks_oppositely() {
        assert_eq!(
            pthread_getaffinity_np(0, 1, core::ptr::null_mut()),
            crate::errno::EINVAL
        );
        assert_eq!(
            pthread_setaffinity_np(0, 1, core::ptr::null()),
            crate::errno::EFAULT
        );
    }

    /// The CPUs there are, not all 1024 bits: on the host, CPU 0 alone, and
    /// every other bit cleared.
    #[test]
    fn test_pthread_getaffinity_np_returns_the_online_cpus() {
        let mut set = CpuSetT {
            bits: [u64::MAX; 16],
        };
        let ret = pthread_getaffinity_np(0, core::mem::size_of::<CpuSetT>(), &raw mut set);
        assert_eq!(ret, 0);
        assert_eq!(set.bits[0], 1);
        assert!(set.bits[1..].iter().all(|&w| w == 0));
    }

    // -----------------------------------------------------------------------
    // Phase 82 — pthread_setcancelstate / pthread_setcanceltype validation
    //
    // POSIX requires that invalid `state`/`type` arguments return EINVAL
    // without mutating the current cancellation state/type, and that
    // valid calls report the previous value via the out-pointer.
    //
    // These tests need no snapshot/restore guard and no reset helper:
    // the state is per-thread and libtest gives every test its own
    // thread, so each one starts from the POSIX defaults by construction.
    // The guard they used to carry could not have worked anyway — it
    // restored the values it *observed* on entry, which another test
    // running concurrently may already have changed.
    // -----------------------------------------------------------------------

    // ---- (a) Helper / constant invariants -------------------------------

    #[test]
    fn test_cancel_state_constants_distinct() {
        assert_ne!(PTHREAD_CANCEL_ENABLE, PTHREAD_CANCEL_DISABLE);
        assert_ne!(PTHREAD_CANCEL_DEFERRED, PTHREAD_CANCEL_ASYNCHRONOUS);
    }

    #[test]
    fn test_cancel_state_default_after_reset() {
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_DEFERRED);
    }

    // ---- (b) pthread_setcancelstate: EINVAL on bad input ----------------

    #[test]
    fn test_setcancelstate_rejects_negative() {
        let ret = pthread_setcancelstate(-1, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        // State must be unchanged.
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
    }

    #[test]
    fn test_setcancelstate_rejects_value_two() {
        let ret = pthread_setcancelstate(2, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
    }

    #[test]
    fn test_setcancelstate_rejects_large_value() {
        let ret = pthread_setcancelstate(i32::MAX, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
    }

    #[test]
    fn test_setcancelstate_invalid_does_not_write_oldstate() {
        let mut old: i32 = 0x5A5A_5A5A;
        let ret = pthread_setcancelstate(42, &raw mut old);
        assert_eq!(ret, errno::EINVAL);
        // Sentinel must be untouched.
        assert_eq!(old, 0x5A5A_5A5A);
    }

    // ---- (c) pthread_setcancelstate: success cases ----------------------

    #[test]
    fn test_setcancelstate_enable_to_disable_reports_previous() {
        let mut old: i32 = -123;
        let ret = pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &raw mut old);
        assert_eq!(ret, 0);
        assert_eq!(old, PTHREAD_CANCEL_ENABLE);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_DISABLE);
    }

    #[test]
    fn test_setcancelstate_save_restore_roundtrip() {
        let mut saved: i32 = 0;
        assert_eq!(
            pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &raw mut saved),
            0,
        );
        assert_eq!(saved, PTHREAD_CANCEL_ENABLE);
        // Now restore.
        assert_eq!(pthread_setcancelstate(saved, core::ptr::null_mut()), 0);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
    }

    #[test]
    fn test_setcancelstate_null_oldstate_succeeds() {
        let ret = pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_DISABLE);
    }

    #[test]
    fn test_setcancelstate_idempotent() {
        // Setting the same value twice should be a no-op (and report
        // that value back).
        let mut old: i32 = 99;
        assert_eq!(
            pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &raw mut old),
            0
        );
        assert_eq!(
            pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &raw mut old),
            0
        );
        assert_eq!(old, PTHREAD_CANCEL_DISABLE);
    }

    // ---- (d) pthread_setcanceltype: EINVAL on bad input -----------------

    #[test]
    fn test_setcanceltype_rejects_negative() {
        let ret = pthread_setcanceltype(-1, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_DEFERRED);
    }

    #[test]
    fn test_setcanceltype_rejects_value_two() {
        let ret = pthread_setcanceltype(2, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_DEFERRED);
    }

    #[test]
    fn test_setcanceltype_invalid_does_not_write_oldtype() {
        let mut old: i32 = 0x1234_5678;
        let ret = pthread_setcanceltype(99, &raw mut old);
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(old, 0x1234_5678);
    }

    // ---- (e) pthread_setcanceltype: success cases -----------------------

    #[test]
    fn test_setcanceltype_deferred_to_async_reports_previous() {
        let mut old: i32 = 0xABCD;
        let ret = pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &raw mut old);
        assert_eq!(ret, 0);
        assert_eq!(old, PTHREAD_CANCEL_DEFERRED);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_ASYNCHRONOUS);
    }

    #[test]
    fn test_setcanceltype_save_restore_roundtrip() {
        let mut saved: i32 = 0;
        assert_eq!(
            pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &raw mut saved),
            0,
        );
        assert_eq!(saved, PTHREAD_CANCEL_DEFERRED);
        assert_eq!(pthread_setcanceltype(saved, core::ptr::null_mut()), 0);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_DEFERRED);
    }

    #[test]
    fn test_setcanceltype_null_oldtype_succeeds() {
        let ret = pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_ASYNCHRONOUS);
    }

    // ---- (f) Non-interference between state and type --------------------

    #[test]
    fn test_setcancelstate_does_not_affect_type() {
        assert_eq!(
            pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, core::ptr::null_mut()),
            0,
        );
        assert_eq!(
            pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, core::ptr::null_mut()),
            0,
        );
        // Type must still be ASYNCHRONOUS.
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_ASYNCHRONOUS);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_DISABLE);
    }

    #[test]
    fn test_setcanceltype_does_not_affect_state() {
        assert_eq!(
            pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, core::ptr::null_mut()),
            0,
        );
        assert_eq!(
            pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, core::ptr::null_mut()),
            0,
        );
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_DISABLE);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_ASYNCHRONOUS);
    }

    // -- futex-based synchronisation, under real threads --

    /// A shareable raw pointer for handing pthread objects to std threads.
    struct Shared<T>(*mut T);
    // By hand: `derive` would demand `T: Copy`, and the pthread objects are
    // not.
    impl<T> Clone for Shared<T> {
        fn clone(&self) -> Self {
            *self
        }
    }
    impl<T> Copy for Shared<T> {}
    // SAFETY: the pthread objects behind these pointers are designed for
    // concurrent use, and every test joins its threads before the object
    // goes out of scope.
    unsafe impl<T> Send for Shared<T> {}
    impl<T> Shared<T> {
        /// By value, so a closure calling it captures the whole wrapper,
        /// not the (non-`Send`) pointer field.
        fn get(self) -> *mut T {
            self.0
        }
    }

    fn ts(sec: i64, nsec: i64) -> crate::stat::Timespec {
        crate::stat::Timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        }
    }

    /// `clock`'s now, plus `ms` milliseconds.
    fn in_ms(clock: i32, ms: i64) -> crate::stat::Timespec {
        let now = crate::lowlevellock::now_on(clock);
        let total = now.tv_nsec + ms * 1_000_000;
        ts(now.tv_sec + total / 1_000_000_000, total % 1_000_000_000)
    }

    #[test]
    fn futex_mutex_excludes_under_contention() {
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        let mp = Shared(&raw mut m);
        let mut counter = 0u64;
        let cp = Shared(&raw mut counter);
        let threads: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(move || {
                    let (mp, cp) = (mp.get(), cp.get());
                    for _ in 0..500 {
                        assert_eq!(unsafe { pthread_mutex_lock(mp) }, 0);
                        unsafe { *cp += 1 };
                        assert_eq!(unsafe { pthread_mutex_unlock(mp) }, 0);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(counter, 4000);
        assert_eq!(m.locked.load(Ordering::Relaxed), 0, "left unlocked");
    }

    /// The owner is the calling thread's cached id: an error-checking mutex
    /// held by one thread is EPERM to unlock from another and EDEADLK to
    /// relock from its owner.
    #[test]
    fn futex_mutex_ownership_is_per_thread() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_ERRORCHECK);
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        assert_eq!(unsafe { pthread_mutex_init(&raw mut m, &attr) }, 0);
        assert_eq!(unsafe { pthread_mutex_lock(&raw mut m) }, 0);
        assert_eq!(unsafe { pthread_mutex_lock(&raw mut m) }, errno::EDEADLK);
        let mp = Shared(&raw mut m);
        let other = std::thread::spawn(move || {
            let mp = mp.get();
            unsafe { pthread_mutex_unlock(mp) }
        })
        .join()
        .unwrap();
        assert_eq!(other, errno::EPERM);
        assert_eq!(unsafe { pthread_mutex_unlock(&raw mut m) }, 0);
    }

    /// A lazily-checked deadline: an uncontended timed lock never reads it,
    /// NULL included; a held lock times out.
    #[test]
    fn futex_mutex_timedlock() {
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        assert_eq!(pthread_mutex_timedlock(&raw mut m, core::ptr::null()), 0);
        let mp = Shared(&raw mut m);
        let r = std::thread::spawn(move || {
            let mp = mp.get();
            let soon = in_ms(crate::time::CLOCK_REALTIME, 20);
            pthread_mutex_timedlock(mp, &raw const soon)
        })
        .join()
        .unwrap();
        assert_eq!(r, errno::ETIMEDOUT);
        assert_eq!(unsafe { pthread_mutex_unlock(&raw mut m) }, 0);
    }

    // -- the rwlock's kinds: glibc's pthread_rwlockattr_setkind_np --

    /// A lock as `pthread_rwlock_init` would find one: every byte 0xAA.
    fn scribbled_rwlock() -> PthreadRwlockT {
        PthreadRwlockT {
            state: AtomicI32::new(0),
            waiters: AtomicI32::new(0),
            writer: AtomicI32::new(0),
            prefer_writer: AtomicI32::new(0x5555),
            writers_waiting: AtomicI32::new(0x5555),
            _pad: [0xAA; 36],
        }
    }

    fn rwlock_of_kind(kind: i32) -> PthreadRwlockT {
        let mut a: PthreadRwlockattrT = [0; 8];
        assert_eq!(pthread_rwlockattr_init(&mut a), 0);
        assert_eq!(pthread_rwlockattr_setkind_np(&mut a, kind), 0);
        let mut rw = scribbled_rwlock();
        assert_eq!(pthread_rwlock_init(&mut rw, &a), 0);
        rw
    }

    #[test]
    fn rwlockattr_kinds_are_glibcs() {
        assert_eq!(
            (
                PTHREAD_RWLOCK_PREFER_READER_NP,
                PTHREAD_RWLOCK_PREFER_WRITER_NP,
                PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP,
                PTHREAD_RWLOCK_DEFAULT_NP
            ),
            (0, 1, 2, 0)
        );
        let mut a: PthreadRwlockattrT = [0xAA; 8];
        assert_eq!(pthread_rwlockattr_init(&mut a), 0);
        let mut k = -1;
        assert_eq!(pthread_rwlockattr_getkind_np(&a, &mut k), 0);
        assert_eq!(k, PTHREAD_RWLOCK_PREFER_READER_NP, "the default");
        for kind in [2, 1, 0, 2] {
            assert_eq!(pthread_rwlockattr_setkind_np(&mut a, kind), 0);
            assert_eq!(pthread_rwlockattr_getkind_np(&a, &mut k), 0);
            assert_eq!(k, kind);
        }
        for bad in [3, -1, i32::MAX, i32::MIN] {
            assert_eq!(pthread_rwlockattr_setkind_np(&mut a, bad), errno::EINVAL);
            assert_eq!(pthread_rwlockattr_getkind_np(&a, &mut k), 0);
            assert_eq!(k, 2, "a refusal changes nothing");
        }
        // The kind and the process-shared flag are apart.
        assert_eq!(
            pthread_rwlockattr_setpshared(&mut a, PTHREAD_PROCESS_PRIVATE),
            0
        );
        assert_eq!(pthread_rwlockattr_getkind_np(&a, &mut k), 0);
        assert_eq!(k, 2);
        let mut shared = -1;
        assert_eq!(pthread_rwlockattr_getpshared(&a, &mut shared), 0);
        assert_eq!(shared, PTHREAD_PROCESS_PRIVATE);
        // The kind is judged before the object, as glibc's; NULLs, where
        // glibc's fault, are EFAULT.
        assert_eq!(
            pthread_rwlockattr_setkind_np(core::ptr::null_mut(), 9),
            errno::EINVAL
        );
        assert_eq!(
            pthread_rwlockattr_setkind_np(core::ptr::null_mut(), 0),
            errno::EFAULT
        );
        assert_eq!(
            pthread_rwlockattr_getkind_np(core::ptr::null(), &mut k),
            errno::EFAULT
        );
        assert_eq!(
            pthread_rwlockattr_getkind_np(&a, core::ptr::null_mut()),
            errno::EFAULT
        );
    }

    /// Writers are preferred for the non-recursive kind alone -- glibc takes
    /// PREFER_WRITER_NP as PREFER_READER_NP -- and a NULL attribute is the
    /// default; C's `PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP`,
    /// `{{{0, 0, 0, 1}}}`, is a writer-preferring lock.
    #[test]
    fn only_the_nonrecursive_kind_prefers_writers() {
        for (kind, prefers) in [(0, 0), (1, 0), (2, 1)] {
            let rw = rwlock_of_kind(kind);
            assert_eq!(
                rw.prefer_writer.load(Ordering::Relaxed),
                prefers,
                "kind {kind}"
            );
            assert_eq!(rw.writers_waiting.load(Ordering::Relaxed), 0);
        }
        let mut rw = scribbled_rwlock();
        assert_eq!(pthread_rwlock_init(&mut rw, core::ptr::null()), 0);
        assert_eq!(rw.prefer_writer.load(Ordering::Relaxed), 0);
        assert_eq!(core::mem::offset_of!(PthreadRwlockT, prefer_writer), 12);
        let mut ints = [0i32; 14];
        ints[3] = 1;
        // SAFETY: 56 bytes of integers, which every field of the lock is.
        let c_init: PthreadRwlockT = unsafe { core::mem::transmute(ints) };
        assert_eq!(c_init.prefer_writer.load(Ordering::Relaxed), 1);
        assert_eq!(c_init.state.load(Ordering::Relaxed), 0, "and unlocked");
    }

    /// While a writer waits, a writer-preferring lock takes no new reader --
    /// not even one that already holds it, which is why glibc calls the kind
    /// non-recursive -- and the other kinds do.
    #[test]
    fn a_waiting_writer_holds_readers_back_only_where_writers_are_preferred() {
        for (kind, held_back) in [(0, false), (1, false), (2, true)] {
            let mut rw = rwlock_of_kind(kind);
            assert_eq!(pthread_rwlock_rdlock(&raw mut rw), 0);
            let rp = Shared(&raw mut rw);
            let writer = std::thread::spawn(move || {
                let rw = rp.get();
                let got = pthread_rwlock_wrlock(rw);
                assert_eq!(pthread_rwlock_unlock(rw), 0);
                got
            });
            if held_back {
                // SAFETY: the lock outlives both threads; an atomic read.
                while unsafe { (*rp.get()).writers_waiting.load(Ordering::Acquire) } == 0 {
                    std::thread::yield_now();
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let again = pthread_rwlock_tryrdlock(&raw mut rw);
            let soon = in_ms(crate::time::CLOCK_REALTIME, 20);
            let timed = pthread_rwlock_timedrdlock(&raw mut rw, &raw const soon);
            if held_back {
                assert_eq!(
                    again,
                    errno::EBUSY,
                    "kind {kind}: busy while the writer waits"
                );
                assert_eq!(
                    timed,
                    errno::ETIMEDOUT,
                    "kind {kind}: a reader waits with it"
                );
            } else {
                assert_eq!((again, timed), (0, 0), "kind {kind}: readers go first");
                assert_eq!(pthread_rwlock_unlock(&raw mut rw), 0);
                assert_eq!(pthread_rwlock_unlock(&raw mut rw), 0);
            }
            assert_eq!(pthread_rwlock_unlock(&raw mut rw), 0);
            assert_eq!(writer.join().unwrap(), 0, "kind {kind}: the writer's turn");
        }
    }

    /// A writer that gives up waiting lets in the readers it held back.
    #[test]
    fn a_writer_that_gives_up_lets_the_readers_it_held_back_in() {
        let mut rw = rwlock_of_kind(PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP);
        assert_eq!(pthread_rwlock_rdlock(&raw mut rw), 0);
        let rp = Shared(&raw mut rw);
        let writer = std::thread::spawn(move || {
            let later = in_ms(crate::time::CLOCK_MONOTONIC, 400);
            pthread_rwlock_clockwrlock(rp.get(), crate::time::CLOCK_MONOTONIC, &raw const later)
        });
        // SAFETY: the lock outlives both threads; an atomic read.
        while unsafe { (*rp.get()).writers_waiting.load(Ordering::Acquire) } == 0 {
            std::thread::yield_now();
        }
        let reader = std::thread::spawn(move || {
            let rw = rp.get();
            let got = pthread_rwlock_rdlock(rw);
            assert_eq!(pthread_rwlock_unlock(rw), 0);
            got
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!reader.is_finished(), "held back while the writer waits");
        assert_eq!(writer.join().unwrap(), errno::ETIMEDOUT);
        assert_eq!(reader.join().unwrap(), 0, "in once the writer gave up");
        // SAFETY: as above.
        assert_eq!(
            unsafe { (*rp.get()).writers_waiting.load(Ordering::Acquire) },
            0
        );
        assert_eq!(pthread_rwlock_unlock(&raw mut rw), 0);
    }

    /// `pthread_mutex_clocklock` judges its clock before the mutex.
    #[test]
    fn futex_mutex_clocklock_clock_first() {
        assert_eq!(
            pthread_mutex_clocklock(core::ptr::null_mut(), 42, core::ptr::null()),
            errno::EINVAL
        );
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        let at = in_ms(crate::time::CLOCK_MONOTONIC, 10);
        assert_eq!(
            pthread_mutex_clocklock(&raw mut m, crate::time::CLOCK_MONOTONIC, &raw const at),
            0
        );
        assert_eq!(unsafe { pthread_mutex_unlock(&raw mut m) }, 0);
    }

    /// A producer and a consumer hand values over a condition variable; the
    /// consumer sees every one in order.
    #[test]
    fn futex_cond_hands_over_values() {
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        let mut c: PthreadCondT = unsafe { core::mem::zeroed() };
        assert_eq!(pthread_cond_init(&raw mut c, core::ptr::null()), 0);
        let mut slot: Option<u32> = None;
        let (mp, cp, sp) = (
            Shared(&raw mut m),
            Shared(&raw mut c),
            Shared(&raw mut slot),
        );
        let consumer = std::thread::spawn(move || {
            let (mp, cp, sp) = (mp.get(), cp.get(), sp.get());
            let mut got = Vec::new();
            for _ in 0..100 {
                unsafe { pthread_mutex_lock(mp) };
                while unsafe { (*sp).is_none() } {
                    assert_eq!(pthread_cond_wait(cp, mp), 0);
                }
                got.push(unsafe { (*sp).take() }.unwrap());
                pthread_cond_broadcast(cp);
                unsafe { pthread_mutex_unlock(mp) };
            }
            got
        });
        for v in 0..100u32 {
            unsafe { pthread_mutex_lock(&raw mut m) };
            while slot.is_some() {
                assert_eq!(pthread_cond_wait(&raw mut c, &raw mut m), 0);
            }
            slot = Some(v);
            pthread_cond_signal(&raw mut c);
            unsafe { pthread_mutex_unlock(&raw mut m) };
        }
        assert_eq!(consumer.join().unwrap(), (0..100).collect::<Vec<_>>());
    }

    /// The attribute's clock is the one the deadline is measured on.  A
    /// monotonic deadline 30 ms ahead waits about 30 ms; until 2026-09-26 it
    /// was read as real time -- decades past -- and returned at once.
    #[test]
    fn futex_cond_timedwait_honours_the_attribute_clock() {
        let mut attr: PthreadCondattrT = [0; 4];
        pthread_condattr_init(&mut attr);
        assert_eq!(
            pthread_condattr_setclock(&mut attr, crate::time::CLOCK_MONOTONIC),
            0
        );
        let mut c: PthreadCondT = unsafe { core::mem::zeroed() };
        assert_eq!(pthread_cond_init(&raw mut c, &attr), 0);
        assert_eq!(c.clock, crate::time::CLOCK_MONOTONIC);
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        unsafe { pthread_mutex_lock(&raw mut m) };
        let start = std::time::Instant::now();
        let at = in_ms(crate::time::CLOCK_MONOTONIC, 30);
        assert_eq!(
            pthread_cond_timedwait(&raw mut c, &raw mut m, &raw const at),
            errno::ETIMEDOUT
        );
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(25),
            "{:?}",
            start.elapsed()
        );
        // The mutex is held again on return.
        assert!(m.locked.load(Ordering::Relaxed) != 0);
        unsafe { pthread_mutex_unlock(&raw mut m) };
    }

    /// `pthread_cond_clockwait` reads the deadline, then judges the clock,
    /// then the condition variable; and it measures on the clock it is
    /// given.
    #[test]
    fn futex_cond_clockwait() {
        let bad = ts(0, 1_000_000_000);
        assert_eq!(
            pthread_cond_clockwait(
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                crate::time::CLOCK_MONOTONIC,
                &raw const bad
            ),
            errno::EINVAL,
            "the deadline first"
        );
        let ok = ts(0, 0);
        assert_eq!(
            pthread_cond_clockwait(
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                9,
                &raw const ok
            ),
            errno::EINVAL,
            "then the clock"
        );
        let mut c: PthreadCondT = unsafe { core::mem::zeroed() };
        pthread_cond_init(&raw mut c, core::ptr::null());
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        unsafe { pthread_mutex_lock(&raw mut m) };
        let at = in_ms(crate::time::CLOCK_MONOTONIC, 20);
        let start = std::time::Instant::now();
        assert_eq!(
            pthread_cond_clockwait(
                &raw mut c,
                &raw mut m,
                crate::time::CLOCK_MONOTONIC,
                &raw const at
            ),
            errno::ETIMEDOUT
        );
        assert!(start.elapsed() >= std::time::Duration::from_millis(15));
        unsafe { pthread_mutex_unlock(&raw mut m) };
    }

    /// `pthread_cond_timedwait` reads the deadline before the condition
    /// variable: NULL pointers with a malformed deadline are EINVAL.
    #[test]
    fn futex_cond_timedwait_deadline_first() {
        let bad = ts(0, -1);
        assert_eq!(
            pthread_cond_timedwait(core::ptr::null_mut(), core::ptr::null_mut(), &raw const bad),
            errno::EINVAL
        );
        assert_eq!(
            pthread_cond_timedwait(
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null()
            ),
            errno::EFAULT
        );
    }

    /// A condition variable's wait returns its mutex's unlock error, having
    /// not waited: an error-checking mutex the caller does not hold.
    #[test]
    fn futex_cond_wait_on_a_mutex_not_held() {
        let mut attr: PthreadMutexattrT = [0; 4];
        pthread_mutexattr_init(&mut attr);
        pthread_mutexattr_settype(&mut attr, PTHREAD_MUTEX_ERRORCHECK);
        let mut m = PTHREAD_MUTEX_INITIALIZER;
        unsafe { pthread_mutex_init(&raw mut m, &attr) };
        let mut c: PthreadCondT = unsafe { core::mem::zeroed() };
        pthread_cond_init(&raw mut c, core::ptr::null());
        assert_eq!(pthread_cond_wait(&raw mut c, &raw mut m), errno::EPERM);
        assert_eq!(c.waiters.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn futex_rwlock_readers_share_writers_exclude() {
        // SAFETY: all-zero is `PTHREAD_RWLOCK_INITIALIZER`'s value.
        let mut rw: PthreadRwlockT = unsafe { core::mem::zeroed() };
        let p = Shared(&raw mut rw);
        let mut value = 0u64;
        let vp = Shared(&raw mut value);
        let writers: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(move || {
                    let (p, vp) = (p.get(), vp.get());
                    for _ in 0..200 {
                        assert_eq!(pthread_rwlock_wrlock(p), 0);
                        unsafe { *vp += 1 };
                        assert_eq!(pthread_rwlock_unlock(p), 0);
                        assert_eq!(pthread_rwlock_rdlock(p), 0);
                        let _ = unsafe { *vp };
                        assert_eq!(pthread_rwlock_unlock(p), 0);
                    }
                })
            })
            .collect();
        for t in writers {
            t.join().unwrap();
        }
        assert_eq!(value, 800);
        assert_eq!(rw.state.load(Ordering::Relaxed), 0);
    }

    /// The writer relocking is EDEADLK (glibc checks `__cur_writer`); a
    /// timed write lock times out while a reader holds the lock.
    #[test]
    fn futex_rwlock_edeadlk_and_timeouts() {
        // SAFETY: all-zero is `PTHREAD_RWLOCK_INITIALIZER`'s value.
        let mut rw: PthreadRwlockT = unsafe { core::mem::zeroed() };
        assert_eq!(pthread_rwlock_wrlock(&raw mut rw), 0);
        assert_eq!(pthread_rwlock_wrlock(&raw mut rw), errno::EDEADLK);
        assert_eq!(pthread_rwlock_rdlock(&raw mut rw), errno::EDEADLK);
        assert_eq!(pthread_rwlock_unlock(&raw mut rw), 0);
        assert_eq!(pthread_rwlock_rdlock(&raw mut rw), 0);
        let p = Shared(&raw mut rw);
        let r = std::thread::spawn(move || {
            let p = p.get();
            let soon = in_ms(crate::time::CLOCK_REALTIME, 20);
            pthread_rwlock_timedwrlock(p, &raw const soon)
        })
        .join()
        .unwrap();
        assert_eq!(r, errno::ETIMEDOUT);
        assert_eq!(pthread_rwlock_unlock(&raw mut rw), 0);
        // The timed forms judge their deadline eagerly, before the lock.
        let bad = ts(0, 1_000_000_000);
        assert_eq!(
            pthread_rwlock_timedrdlock(core::ptr::null_mut(), &raw const bad),
            errno::EINVAL
        );
        assert_eq!(
            pthread_rwlock_clockwrlock(core::ptr::null_mut(), 7, &raw const bad),
            errno::EINVAL
        );
        assert_eq!(
            pthread_rwlock_timedwrlock(&raw mut rw, core::ptr::null()),
            errno::EFAULT
        );
    }

    /// Many rounds of a reused barrier: in every round exactly one thread is
    /// the serial one, and no thread leaves a round before all have arrived.
    #[test]
    fn futex_barrier_rounds() {
        const N: usize = 4;
        const ROUNDS: usize = 50;
        let mut b: PthreadBarrierT = unsafe { core::mem::zeroed() };
        assert_eq!(
            pthread_barrier_init(&raw mut b, core::ptr::null(), N as u32),
            0
        );
        let bp = Shared(&raw mut b);
        let arrivals: std::sync::Arc<Vec<std::sync::atomic::AtomicUsize>> = std::sync::Arc::new(
            (0..ROUNDS)
                .map(|_| std::sync::atomic::AtomicUsize::new(0))
                .collect(),
        );
        let serials: std::sync::Arc<Vec<std::sync::atomic::AtomicUsize>> = std::sync::Arc::new(
            (0..ROUNDS)
                .map(|_| std::sync::atomic::AtomicUsize::new(0))
                .collect(),
        );
        let threads: Vec<_> = (0..N)
            .map(|_| {
                let (arrivals, serials) = (arrivals.clone(), serials.clone());
                std::thread::spawn(move || {
                    let bp = bp.get();
                    for round in 0..ROUNDS {
                        arrivals[round].fetch_add(1, Ordering::SeqCst);
                        let r = pthread_barrier_wait(bp);
                        assert_eq!(arrivals[round].load(Ordering::SeqCst), N, "left early");
                        if r == PTHREAD_BARRIER_SERIAL_THREAD {
                            serials[round].fetch_add(1, Ordering::SeqCst);
                        } else {
                            assert_eq!(r, 0);
                        }
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        for round in 0..ROUNDS {
            assert_eq!(serials[round].load(Ordering::SeqCst), 1, "round {round}");
        }
    }

    static ONCE_RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    extern "C" fn slow_once_init() {
        std::thread::sleep(std::time::Duration::from_millis(20));
        ONCE_RUNS.fetch_add(1, Ordering::SeqCst);
    }

    /// Callers racing `pthread_once` wait for the one running `init` -- and
    /// find it done when they return.
    #[test]
    fn futex_once_waiters_see_init_done() {
        let mut once = PTHREAD_ONCE_INIT;
        let op = Shared(&raw mut once);
        let threads: Vec<_> = (0..6)
            .map(|_| {
                std::thread::spawn(move || {
                    let op = op.get();
                    assert_eq!(unsafe { pthread_once(op, Some(slow_once_init)) }, 0);
                    assert!(
                        ONCE_RUNS.load(Ordering::SeqCst) >= 1,
                        "returned before init finished"
                    );
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(ONCE_RUNS.load(Ordering::SeqCst), 1);
    }

    /// The thread id is cached in the per-thread block: the same across
    /// calls, and different between threads.
    #[test]
    fn current_tid_is_cached_and_per_thread() {
        let a = current_tid();
        assert_ne!(a, 0);
        assert_eq!(current_tid(), a);
        assert_eq!(unsafe { (*crate::perthread::current()).tid }, a);
        let b = std::thread::spawn(current_tid).join().unwrap();
        assert_ne!(a, b);
    }

    // -- the last thread's pthread_exit is exit(0) --

    #[test]
    fn only_the_last_thread_is_the_last() {
        let n = AtomicUsize::new(1);
        live_threads_add(&n);
        live_threads_add(&n);
        assert!(!live_threads_remove(&n), "three running, one ends");
        assert!(!live_threads_remove(&n), "two running, one ends");
        assert!(live_threads_remove(&n), "the initial thread's count, last");
    }

    #[test]
    fn a_thread_that_ends_before_its_creator_resumes_is_not_the_last() {
        // The creator counts it before the system call, so the order in
        // which the two run cannot make the new thread look like the last.
        let n = AtomicUsize::new(1);
        live_threads_add(&n);
        assert!(!live_threads_remove(&n), "the creator is still running");
        assert_eq!(n.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_zero_count_does_not_wrap() {
        let n = AtomicUsize::new(0);
        assert!(live_threads_remove(&n));
        assert_eq!(n.load(Ordering::Acquire), 0, "saturates at zero");
    }

    // -- A NULL start routine or once routine (design-decisions.md §1115) --

    /// A done once needs no routine: 0, as in glibc.  One not yet run would
    /// have glibc call the NULL; here it is EFAULT, and the once can still be
    /// run by a real routine afterwards.
    #[test]
    fn a_null_once_routine_is_efault_only_when_it_would_run() {
        static RAN: AtomicI32 = AtomicI32::new(0);
        extern "C" fn mark() {
            RAN.fetch_add(1, Ordering::Relaxed);
        }
        let mut once = PTHREAD_ONCE_INIT;
        // SAFETY: a live once control.
        unsafe {
            assert_eq!(pthread_once(&mut once, None), errno::EFAULT);
            assert_eq!(RAN.load(Ordering::Relaxed), 0);
            assert_eq!(pthread_once(&mut once, Some(mark)), 0, "still runnable");
            assert_eq!(RAN.load(Ordering::Relaxed), 1);
            assert_eq!(pthread_once(&mut once, None), 0, "done: nothing to call");
        }
    }

    /// A NULL start routine is judged only after the thread's memory has been
    /// had: glibc creates the thread, which faults calling it.  The host has
    /// no memory to give a thread, so there its EAGAIN is the answer -- the
    /// NULL is not judged first.  (With the memory, it is EFAULT and no
    /// thread.)
    #[test]
    fn a_null_start_routine_is_judged_after_the_threads_memory() {
        let mut t: PthreadT = 0;
        assert_eq!(
            pthread_create(&mut t, core::ptr::null(), None, core::ptr::null_mut()),
            errno::EAGAIN
        );
        assert_eq!(t, 0);
    }

    // -- glibc's affinity and stack-address attributes, and clockjoin --

    /// A CPU set comes back as it was given -- zero-extended into a larger
    /// buffer, refused (`EINVAL`) into one too small for a CPU it holds --
    /// and an object with none answers every bit set, glibc's "no
    /// information".
    #[test]
    fn attr_affinity_is_kept_and_given_back_as_glibcs() {
        let mut a = fresh_attr();
        let mut big = [0x5au8; 128];
        let mut small = [0x5au8; 8];
        let mut set = [0u8; 16];
        set[0] = 0b101;
        set[9] = 0x80;
        let low: [u8; 4] = [1, 0, 0, 0];
        // SAFETY: every buffer is at least the size passed with it.
        unsafe {
            assert_eq!(
                pthread_attr_getaffinity_np(&a, 128, big.as_mut_ptr().cast()),
                0
            );
            assert!(big.iter().all(|&b| b == 0xff), "no set: every bit");
            assert_eq!(
                pthread_attr_setaffinity_np(&mut a, 16, set.as_ptr().cast()),
                0
            );
            assert_eq!(
                pthread_attr_getaffinity_np(&a, 128, big.as_mut_ptr().cast()),
                0
            );
            assert_eq!(&big[..16], &set);
            assert!(big[16..].iter().all(|&b| b == 0), "zero-extended");
            assert_eq!(
                pthread_attr_getaffinity_np(&a, 8, small.as_mut_ptr().cast()),
                errno::EINVAL,
                "CPU 79 does not fit in 8 bytes"
            );
            // A set of another size replaces it.
            assert_eq!(
                pthread_attr_setaffinity_np(&mut a, 4, low.as_ptr().cast()),
                0
            );
            assert_eq!(
                pthread_attr_getaffinity_np(&a, 8, small.as_mut_ptr().cast()),
                0
            );
            assert_eq!(small, [1, 0, 0, 0, 0, 0, 0, 0]);
            // A size of 0, or NULL, takes it back.
            assert_eq!(
                pthread_attr_setaffinity_np(&mut a, 0, low.as_ptr().cast()),
                0
            );
            assert_eq!(
                pthread_attr_getaffinity_np(&a, 8, small.as_mut_ptr().cast()),
                0
            );
            assert_eq!(small, [0xff; 8]);
            assert_eq!(
                pthread_attr_setaffinity_np(&mut a, 4, low.as_ptr().cast()),
                0
            );
            assert_eq!(pthread_attr_setaffinity_np(&mut a, 4, core::ptr::null()), 0);
            assert_eq!(
                pthread_attr_getaffinity_np(&a, 8, small.as_mut_ptr().cast()),
                0
            );
            assert_eq!(small, [0xff; 8]);
            // NULLs, where glibc's fault.
            assert_eq!(
                pthread_attr_setaffinity_np(core::ptr::null_mut(), 4, low.as_ptr().cast()),
                errno::EFAULT
            );
            assert_eq!(
                pthread_attr_getaffinity_np(core::ptr::null(), 8, small.as_mut_ptr().cast()),
                errno::EFAULT
            );
            assert_eq!(
                pthread_attr_getaffinity_np(&a, 8, core::ptr::null_mut()),
                errno::EFAULT
            );
            assert_eq!(pthread_attr_getaffinity_np(&a, 0, core::ptr::null_mut()), 0);
        }
        assert_eq!(pthread_attr_destroy(&mut a), 0);
        assert!(attr_read_ext(&a).is_null(), "destroyed: nothing held");
        assert_eq!(pthread_attr_destroy(&mut a), 0, "twice: nothing to free");
    }

    /// `pthread_create` keeps a set of every CPU and refuses any other --
    /// `ENOSYS`, or `EINVAL` for one of no CPU there is -- before a thread
    /// is made, as a thread's affinity can be nothing else here.
    #[test]
    fn a_created_threads_affinity_is_every_cpu_or_refused() {
        extern "C" fn nothing(_: *mut u8) -> *mut u8 {
            core::ptr::null_mut()
        }
        let ncpus = crate::sched::online_cpus();
        let mut all = [0u8; 128];
        for cpu in 0..ncpus.min(1024) {
            all[cpu / 8] |= 1 << (cpu % 8);
        }
        let mut a = fresh_attr();
        // SAFETY: a buffer of the size passed.
        unsafe {
            assert_eq!(
                pthread_attr_setaffinity_np(&mut a, 128, all.as_ptr().cast()),
                0
            )
        };
        assert_eq!(CreateAttr::read(&a).affinity, None);
        let mut beyond = [0u8; 128];
        beyond[127] = 0x80;
        // SAFETY: as above.
        unsafe {
            assert_eq!(
                pthread_attr_setaffinity_np(&mut a, 128, beyond.as_ptr().cast()),
                0
            )
        };
        assert_eq!(
            CreateAttr::read(&a).affinity,
            Some(errno::EINVAL),
            "CPU 1023"
        );
        let mut t: PthreadT = 0;
        assert_eq!(
            pthread_create(&mut t, &a, Some(nothing), core::ptr::null_mut()),
            errno::EINVAL
        );
        assert_eq!(t, 0, "no thread");
        if ncpus > 1 {
            let mut one = [0u8; 128];
            one[0] = 1;
            // SAFETY: as above.
            unsafe {
                assert_eq!(
                    pthread_attr_setaffinity_np(&mut a, 128, one.as_ptr().cast()),
                    0
                )
            };
            assert_eq!(CreateAttr::read(&a).affinity, Some(errno::ENOSYS));
        }
        assert_eq!(pthread_attr_destroy(&mut a), 0);
    }

    /// The default attributes carry a CPU set of their own: copied in by
    /// `pthread_setattr_default_np`, out by `pthread_getattr_default_np`,
    /// read by a NULL-attribute `pthread_create`, and freed when replaced.
    #[test]
    fn the_default_attributes_carry_a_cpu_set_of_their_own() {
        let mut a = fresh_attr();
        let set: [u8; 8] = [1, 0, 0, 0, 0, 0, 0, 0];
        // SAFETY: a buffer of the size passed.
        unsafe {
            assert_eq!(
                pthread_attr_setaffinity_np(&mut a, 8, set.as_ptr().cast()),
                0
            )
        };
        assert_eq!(pthread_setattr_default_np(&a), 0);
        assert_eq!(
            pthread_attr_destroy(&mut a),
            0,
            "the defaults' set is a copy"
        );
        let mut d: PthreadAttrT = [0; 56];
        assert_eq!(pthread_getattr_default_np(&mut d), 0);
        assert!(!attr_read_ext(&d).is_null());
        assert_ne!(
            attr_read_ext(&d),
            with_default_attr(|x| attr_read_ext(x)),
            "and so is this"
        );
        let mut got = [0u8; 8];
        // SAFETY: as above.
        unsafe {
            assert_eq!(
                pthread_attr_getaffinity_np(&d, 8, got.as_mut_ptr().cast()),
                0
            )
        };
        assert_eq!(got, set);
        assert_eq!(pthread_attr_destroy(&mut d), 0);
        let want = if crate::sched::online_cpus() == 1 {
            None
        } else {
            Some(errno::ENOSYS)
        };
        assert_eq!(CreateAttr::read(core::ptr::null()).affinity, want);
        let fresh = fresh_attr();
        assert_eq!(pthread_setattr_default_np(&fresh), 0);
        assert!(with_default_attr(|x| attr_read_ext(x).is_null()));
        assert_eq!(CreateAttr::read(core::ptr::null()).affinity, None);
    }

    /// `pthread_getattr_np` reports the thread's CPUs, as glibc's does: 32
    /// bytes of them, every online CPU here.
    #[test]
    fn getattr_reports_the_threads_cpus_as_glibcs_does() {
        let mut a: PthreadAttrT = [0; 56];
        encode_attr(&mut a, main_thread_stack_attr());
        assert_eq!(attr_add_thread_affinity(&mut a, 0), 0);
        let ncpus = crate::sched::online_cpus();
        let mut got = [0u8; 128];
        // SAFETY: a buffer of the size passed.
        unsafe {
            assert_eq!(
                pthread_attr_getaffinity_np(&a, 128, got.as_mut_ptr().cast()),
                0
            )
        };
        for cpu in 0..1024 {
            assert_eq!(
                (got[cpu / 8] >> (cpu % 8)) & 1 == 1,
                cpu < ncpus,
                "CPU {cpu}"
            );
        }
        // SAFETY: the extension `attr_add_thread_affinity` made.
        assert_eq!(unsafe { attr_cpuset(&a) }.map(<[u8]>::len), Some(32));
        assert_eq!(pthread_attr_destroy(&mut a), 0);
    }

    /// glibc keeps a stack by its top: `pthread_attr_setstackaddr` takes the
    /// top and `pthread_attr_getstackaddr` gives it back -- after
    /// `pthread_attr_setstack`, its address plus its size -- and a size set
    /// afterwards moves the bottom, not the top.
    #[test]
    fn a_stack_is_kept_by_its_top_as_glibcs() {
        use core::ffi::c_void;
        let mut a = fresh_attr();
        let mut top: *mut c_void = core::ptr::dangling_mut();
        assert_eq!(pthread_attr_getstackaddr(&a, &mut top), 0);
        assert!(top.is_null(), "none");
        assert_eq!(
            pthread_attr_setstack(&mut a, 0x4000_0000usize as *mut c_void, 0x10_0000),
            0
        );
        assert_eq!(pthread_attr_getstackaddr(&a, &mut top), 0);
        assert_eq!(top as usize, 0x4010_0000);
        assert_eq!(pthread_attr_setstacksize(&mut a, 0x8_0000), 0);
        let (mut addr, mut size): (*mut c_void, usize) = (core::ptr::null_mut(), 0);
        assert_eq!(pthread_attr_getstack(&a, &mut addr, &mut size), 0);
        assert_eq!(
            (addr as usize, size),
            (0x4008_0000, 0x8_0000),
            "the top stays"
        );
        assert_eq!(CreateAttr::read(&a).stack_addr, Some(0x4008_0000));
        assert_eq!(
            pthread_attr_setstackaddr(&mut a, 0x5000_0000usize as *mut c_void),
            0
        );
        assert_eq!(pthread_attr_getstack(&a, &mut addr, &mut size), 0);
        assert_eq!(addr as usize, 0x5000_0000 - 0x8_0000);
        assert_eq!(pthread_attr_setstackaddr(&mut a, core::ptr::null_mut()), 0);
        assert_eq!(CreateAttr::read(&a).stack_addr, None, "NULL takes it back");
        assert_eq!(
            pthread_attr_setstack(&mut a, (usize::MAX - 0x100) as *mut c_void, 0x10_0000),
            errno::EINVAL,
            "past the end of memory"
        );
        assert_eq!(
            pthread_attr_setstackaddr(core::ptr::null_mut(), core::ptr::null_mut()),
            errno::EFAULT
        );
        assert_eq!(
            pthread_attr_getstackaddr(core::ptr::null(), &mut top),
            errno::EFAULT
        );
        assert_eq!(
            pthread_attr_getstackaddr(&a, core::ptr::null_mut()),
            errno::EFAULT
        );
        assert_eq!(pthread_attr_destroy(&mut a), 0);
    }

    /// `pthread_clockjoin_np` takes `CLOCK_REALTIME` and `CLOCK_MONOTONIC`,
    /// judging the clock before the thread, and otherwise answers as
    /// `pthread_timedjoin_np` does, against the clock named.
    #[test]
    fn clockjoin_takes_the_two_clocks_and_judges_the_clock_first() {
        use crate::stat::Timespec;
        use crate::time::{CLOCK_MONOTONIC, CLOCK_REALTIME};
        let mut rv: *mut u8 = core::ptr::null_mut();
        let at = |tv_sec, tv_nsec| Timespec { tv_sec, tv_nsec };
        let later = at(i64::MAX, 0);
        // SAFETY: each `abstime` is this frame's.
        unsafe {
            for clock in [2, 3, 7, -1, 99] {
                assert_eq!(
                    pthread_clockjoin_np(0x5100_00fe, &raw mut rv, clock, &later),
                    errno::EINVAL,
                    "clock {clock}, before the thread"
                );
            }
            assert_eq!(
                pthread_clockjoin_np(0x5100_00fe, &raw mut rv, CLOCK_MONOTONIC, &later),
                errno::ESRCH
            );
        }
        let tid: u64 = 0x5100_0040;
        let slot = track(tid, 0, DEFAULT_THREAD_STACK_SIZE, false);
        // SAFETY: as above.
        unsafe {
            assert_eq!(
                pthread_clockjoin_np(tid, &raw mut rv, CLOCK_MONOTONIC, &at(0, 0)),
                errno::ETIMEDOUT,
                "boot time has passed"
            );
            assert_eq!(
                pthread_clockjoin_np(tid, &raw mut rv, CLOCK_MONOTONIC, &at(0, 1_000_000_000)),
                errno::EINVAL
            );
        }
        let mut now = Timespec::default();
        assert_eq!(crate::time::clock_gettime(CLOCK_MONOTONIC, &raw mut now), 0);
        let soon = if now.tv_nsec < 970_000_000 {
            at(now.tv_sec, now.tv_nsec + 30_000_000)
        } else {
            at(now.tv_sec + 1, now.tv_nsec - 970_000_000)
        };
        let started = std::time::Instant::now();
        // SAFETY: as above.
        assert_eq!(
            unsafe { pthread_clockjoin_np(tid, &raw mut rv, CLOCK_MONOTONIC, &soon) },
            errno::ETIMEDOUT
        );
        assert!(started.elapsed() >= std::time::Duration::from_millis(25));
        slot.state.store(STATE_EXITED, Ordering::Release);
        // SAFETY: as above.
        let r = unsafe { pthread_clockjoin_np(tid, &raw mut rv, CLOCK_REALTIME, &at(0, -1)) };
        assert_eq!(r, pthread_join(tid, &raw mut rv), "gone already: joined");
        if let Some(s) = find_slot(tid) {
            release_slot(s);
        }
    }
}
