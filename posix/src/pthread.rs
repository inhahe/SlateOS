//! POSIX threads — working implementation backed by kernel threads.
//!
//! ## Thread Creation
//!
//! `pthread_create` allocates a user-mode stack via `mmap`, pushes the
//! start routine and argument onto it, then calls `SYS_THREAD_CREATE`
//! with an assembly trampoline as the entry point.  The trampoline pops
//! the arguments, calls the start routine, and issues `SYS_THREAD_EXIT`
//! with the return value.
//!
//! ## Thread Lifecycle
//!
//! - **Joinable** (default): Another thread calls `pthread_join` which
//!   blocks on `SYS_THREAD_JOIN`, then frees the stack.
//! - **Detached**: `pthread_detach` marks the thread; its stack cannot
//!   currently be reclaimed (known limitation — needs kernel-level
//!   cleanup notification or a reaper thread).
//!
//! ## Synchronization Primitives
//!
//! - **Mutexes**: atomic CAS with spin-yield.  Supports normal,
//!   recursive (reentrant), and error-checking mutex types.
//! - **Condition variables**: generation counter with spin-yield wait.
//! - **Read-write locks**: atomic state (0=unlocked, N=readers, -1=writer).
//! - **Barriers**: arrival counter with generation-based release.
//! - **Spinlocks**: pure atomic CAS busy-wait.
//!
//! ## Features
//!
//! - Thread: `pthread_create`, `pthread_join`, `pthread_detach`,
//!   `pthread_self`, `pthread_equal`, `pthread_exit`
//! - Attributes: `pthread_attr_init`/`destroy`/`setstacksize`/
//!   `getstacksize`/`setdetachstate`/`getdetachstate`
//! - Mutex: `pthread_mutex_init`/`destroy`/`lock`/`trylock`/`unlock`
//! - Mutex attributes: `pthread_mutexattr_init`/`destroy`/`settype`/
//!   `gettype`
//! - Condition: `pthread_cond_init`/`destroy`/`wait`/`timedwait`/
//!   `signal`/`broadcast`
//! - RW lock: `pthread_rwlock_init`/`destroy`/`rdlock`/`tryrdlock`/
//!   `wrlock`/`trywrlock`/`unlock`
//! - Barrier: `pthread_barrier_init`/`destroy`/`wait`
//! - Spinlock: `pthread_spin_init`/`destroy`/`lock`/`trylock`/`unlock`
//! - Cancel stubs: `pthread_setcancelstate`/`setcanceltype`/
//!   `testcancel`/`cancel`
//! - Once: `pthread_once`
//! - TSD: `pthread_key_create`/`delete`/`getspecific`/`setspecific`
//! - Yield: `sched_yield`
//!
//! ## Limitations
//!
//! - Thread-specific data (TSD) uses a **global** array, not per-thread
//!   storage.  Proper TLS requires kernel support for the FS/GS segment.
//! - Detached thread stacks are leaked (no cleanup notification).
//! - `pthread_cancel` accepted but never actually cancels a thread.
//! - Mutex is a spinlock (no futex-based blocking).
//! - Condition variables use spin-yield (1ms intervals) watching a
//!   generation counter.  Correct but not efficient.
//! - Recursive/error-checking mutexes track owner via syscall per
//!   lock/unlock (no futex-based blocking yet).

use crate::errno;
use crate::syscall;
use core::sync::atomic::{AtomicI32, Ordering};

/// Opaque pthread_t type — holds the kernel task ID.
pub type PthreadT = u64;

/// Opaque pthread_attr_t type.
pub type PthreadAttrT = [u8; 64];

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

/// Pthread mutex attribute type.
pub type PthreadMutexattrT = [u8; 8];

/// Pthread condition variable type — basic implementation.
///
/// Uses a generation counter so `pthread_cond_signal` can wake
/// threads spinning on `pthread_cond_wait`.
#[repr(C)]
pub struct PthreadCondT {
    /// Generation counter — incremented on each signal/broadcast.
    generation: AtomicI32,
    // Padding to match typical libc struct size.
    _pad: [u8; 44],
}

/// Pthread condition variable attribute type.
pub type PthreadCondattrT = [u8; 8];

/// Static initializer for `pthread_cond_t`.
#[allow(clippy::declare_interior_mutable_const)]
#[unsafe(no_mangle)]
pub static PTHREAD_COND_INITIALIZER: PthreadCondT = PthreadCondT {
    generation: AtomicI32::new(0),
    _pad: [0; 44],
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
    kind: AtomicI32::new(0),    // PTHREAD_MUTEX_NORMAL
    owner: AtomicI32::new(0),
    count: AtomicI32::new(0),
    _pad: [0; 24],
};

// ---------------------------------------------------------------------------
// Thread table — tracks created threads for stack cleanup
// ---------------------------------------------------------------------------

/// Per-thread metadata stored at creation time.
#[derive(Clone, Copy)]
struct ThreadInfo {
    task_id: u64,
    stack_base: usize,
    stack_size: usize,
    detached: bool,
}

/// Maximum number of concurrently tracked threads.
const MAX_THREADS: usize = 64;

/// Default user-mode stack size for new threads (64 KiB = 4 pages).
const DEFAULT_THREAD_STACK_SIZE: usize = 64 * 1024;

/// Thread info table.
static mut THREAD_TABLE: [Option<ThreadInfo>; MAX_THREADS] = [None; MAX_THREADS];

/// Raw pointer to the thread table.
#[inline]
fn thread_table_ptr() -> *mut [Option<ThreadInfo>; MAX_THREADS] {
    core::ptr::addr_of_mut!(THREAD_TABLE)
}

/// Store thread info in the first available slot.
///
/// Returns `true` on success, `false` if the table is full.
fn store_thread_info(info: ThreadInfo) -> bool {
    // SAFETY: Single-process access; thread creation is serialized
    // by convention (only one thread creates others at a time).
    unsafe {
        let table = &mut *thread_table_ptr();
        for slot in table.iter_mut() {
            if slot.is_none() {
                *slot = Some(info);
                return true;
            }
        }
    }
    false
}

/// Find and remove thread info by kernel task ID.
fn take_thread_info(task_id: u64) -> Option<ThreadInfo> {
    // SAFETY: Same single-creator convention as store_thread_info.
    unsafe {
        let table = &mut *thread_table_ptr();
        for slot in table.iter_mut() {
            let matches = slot.as_ref().is_some_and(|i| i.task_id == task_id);
            if matches {
                return slot.take();
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Assembly trampoline — entry point for new threads
// ---------------------------------------------------------------------------

// The trampoline runs in ring 3 on the new thread's user stack.
// Stack layout at entry:
//   [RSP]     = arg pointer         (for start_routine)
//   [RSP + 8] = start_routine ptr   (function to call)
//
// The trampoline:
//   1. Pops arg into RDI (first C argument register)
//   2. Pops start_routine into RSI
//   3. Calls start_routine(arg)
//   4. Issues SYS_THREAD_EXIT with the return value
//
// Stack alignment: after two pops RSP is 16-byte aligned (the mmap'd
// stack top is page-aligned), so the CALL satisfies the SysV ABI
// requirement.
#[cfg(not(test))]
core::arch::global_asm!(
    ".global _pthread_trampoline",
    ".type _pthread_trampoline, @function",
    "_pthread_trampoline:",
    "    pop rdi",           // rdi = arg
    "    pop rsi",           // rsi = start_routine
    "    call rsi",          // rax = start_routine(arg)
    "    mov rdi, rax",      // exit value = return value
    "    mov eax, 511",      // SYS_THREAD_EXIT
    "    syscall",
    "    ud2",               // unreachable
);

#[cfg(not(test))]
unsafe extern "C" {
    fn _pthread_trampoline();
}

// ---------------------------------------------------------------------------
// Thread creation / management
// ---------------------------------------------------------------------------

/// Create a new thread.
///
/// Allocates a user-mode stack, sets up the trampoline arguments, and
/// issues `SYS_THREAD_CREATE`.  On success, stores the new thread's
/// kernel task ID in `*thread`.
///
/// Returns 0 on success, or a POSIX error number on failure.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_create(
    thread: *mut PthreadT,
    _attr: *const PthreadAttrT,
    start: extern "C" fn(*mut u8) -> *mut u8,
    arg: *mut u8,
) -> i32 {
    // Allocate a user-mode stack.
    let stack = crate::mman::mmap(
        core::ptr::null_mut(),
        DEFAULT_THREAD_STACK_SIZE,
        crate::mman::PROT_READ | crate::mman::PROT_WRITE,
        crate::mman::MAP_PRIVATE | crate::mman::MAP_ANONYMOUS,
        -1,
        0,
    );

    if stack == crate::mman::MAP_FAILED {
        return errno::EAGAIN;
    }

    let stack_base = stack as usize;
    let stack_top = stack_base.wrapping_add(DEFAULT_THREAD_STACK_SIZE);

    // Push start_routine and arg onto the new stack for the trampoline.
    // SAFETY: mmap succeeded → [stack_base, stack_top) is valid memory.
    unsafe {
        let fn_slot = stack_top.wrapping_sub(8) as *mut u64;
        let arg_slot = stack_top.wrapping_sub(16) as *mut u64;
        core::ptr::write(fn_slot, start as usize as u64);
        core::ptr::write(arg_slot, arg as u64);
    }

    let user_rsp = stack_top.wrapping_sub(16) as u64;

    // Get the trampoline's address.
    #[cfg(not(test))]
    let entry = _pthread_trampoline as *const () as u64;
    #[cfg(test)]
    let entry: u64 = 0;

    // Create the kernel thread.
    let ret = syscall::syscall3(
        syscall::SYS_THREAD_CREATE,
        entry,
        user_rsp,
        u64::MAX, // default priority
    );

    if ret < 0 {
        let _ = crate::mman::munmap(stack, DEFAULT_THREAD_STACK_SIZE);
        return errno::EAGAIN;
    }

    let task_id = ret as u64;

    // Track the thread for later cleanup (best effort — if the table
    // is full the thread runs but its stack leaks on join).
    let _ = store_thread_info(ThreadInfo {
        task_id,
        stack_base,
        stack_size: DEFAULT_THREAD_STACK_SIZE,
        detached: false,
    });

    if !thread.is_null() {
        // SAFETY: caller guarantees thread points to valid PthreadT.
        unsafe { *thread = task_id; }
    }

    0
}

/// Wait for a thread to terminate.
///
/// Blocks until the specified thread exits, stores its return value
/// in `*retval` (if non-null), and frees the thread's stack.
///
/// Returns 0 on success, or a POSIX error number on failure.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_join(thread_id: PthreadT, retval: *mut *mut u8) -> i32 {
    let ret = syscall::syscall1(syscall::SYS_THREAD_JOIN, thread_id);

    if ret < 0 {
        // The kernel returns negative error codes; map to POSIX.
        return errno::ESRCH;
    }

    // The kernel returns the exit value from SYS_THREAD_EXIT.
    if !retval.is_null() {
        // SAFETY: caller guarantees retval is a valid pointer.
        unsafe { *retval = ret as *mut u8; }
    }

    // Free the thread's stack.
    if let Some(info) = take_thread_info(thread_id) {
        let _ = crate::mman::munmap(
            info.stack_base as *mut core::ffi::c_void,
            info.stack_size,
        );
    }

    0
}

/// Detach a thread.
///
/// Marks the thread so that its resources are released when it exits
/// without requiring a `pthread_join`.
///
/// **Known limitation**: detached thread stacks are currently leaked
/// because there is no kernel notification when a thread exits.  A
/// reaper thread or kernel callback would be needed to fix this.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_detach(thread_id: PthreadT) -> i32 {
    // SAFETY: Thread table access is serialized by convention.
    unsafe {
        let table = &mut *thread_table_ptr();
        for slot in table.iter_mut() {
            if slot.as_ref().is_some_and(|i| i.task_id == thread_id) {
                if let Some(info) = slot {
                    info.detached = true;
                }
                return 0;
            }
        }
    }
    errno::ESRCH
}

/// Get the calling thread's ID.
///
/// Returns the kernel task ID of the calling thread.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_self() -> PthreadT {
    syscall::syscall0(syscall::SYS_TASK_ID) as PthreadT
}

/// Compare two thread IDs.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_equal(t1: PthreadT, t2: PthreadT) -> i32 {
    i32::from(t1 == t2)
}

/// Terminate the calling thread.
///
/// Issues `SYS_THREAD_EXIT` with the specified return value.
/// If this is the last thread in the process, the process exits.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_exit(retval: *mut u8) -> ! {
    let _ = syscall::syscall1(syscall::SYS_THREAD_EXIT, retval as u64);
    // SAFETY: SYS_THREAD_EXIT never returns; this is a safety net.
    loop {
        unsafe { core::arch::asm!("hlt", options(nostack, nomem)); }
    }
}

// ---------------------------------------------------------------------------
// Mutex operations — thread-safe via atomics
// ---------------------------------------------------------------------------

/// Maximum spin iterations before yielding on a contended mutex.
const MUTEX_SPIN_LIMIT: u32 = 100;

/// Initialize a mutex.
///
/// Reads the mutex type from `attr` (if non-null) to determine whether
/// the mutex is normal, recursive, or error-checking.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_init(
    mutex: *mut PthreadMutexT,
    attr: *const PthreadMutexattrT,
) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    // Read kind from attr (default: PTHREAD_MUTEX_NORMAL = 0).
    let kind: i32 = if attr.is_null() {
        PTHREAD_MUTEX_NORMAL
    } else {
        // SAFETY: attr verified non-null.  PthreadMutexattrT is [u8; 8]
        // with first 4 bytes holding the kind (set by mutexattr_settype).
        unsafe { core::ptr::read_unaligned(attr.cast::<i32>()) }
    };
    // SAFETY: caller guarantees mutex is valid.
    unsafe {
        (*mutex).locked.store(0, Ordering::Release);
        (*mutex).kind.store(kind, Ordering::Release);
        (*mutex).owner.store(0, Ordering::Release);
        (*mutex).count.store(0, Ordering::Release);
    }
    0
}

/// Lock a mutex.
///
/// Uses atomic CAS for thread safety.  On contention, spins briefly
/// then yields via `SYS_SLEEP(1ms)` to avoid wasting CPU time.
///
/// Behavior depends on mutex type:
/// - **Normal**: blocks until lock is acquired (deadlock if already
///   held by calling thread).
/// - **Recursive**: if already held by calling thread, increments
///   recursion count and returns 0.
/// - **Error-checking**: if already held by calling thread, returns
///   EDEADLK without blocking.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_lock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }

    // SAFETY: caller guarantees mutex is valid.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);
    let self_id = syscall::syscall0(syscall::SYS_TASK_ID) as i32;

    // Recursive / error-checking: check if we already own the lock.
    if (kind == PTHREAD_MUTEX_RECURSIVE || kind == PTHREAD_MUTEX_ERRORCHECK)
        && m.locked.load(Ordering::Acquire) != 0
        && m.owner.load(Ordering::Relaxed) == self_id
    {
        if kind == PTHREAD_MUTEX_RECURSIVE {
            // Increment recursion count.
            let c = m.count.load(Ordering::Relaxed);
            m.count.store(c.wrapping_add(1), Ordering::Relaxed);
            return 0;
        }
        // Error-checking: double-lock by same thread.
        return errno::EDEADLK;
    }

    // Fast path: uncontended acquisition.
    if m.locked
        .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        m.owner.store(self_id, Ordering::Relaxed);
        m.count.store(1, Ordering::Relaxed);
        return 0;
    }

    // Slow path: spin briefly, then yield.
    loop {
        for _ in 0..MUTEX_SPIN_LIMIT {
            if m.locked
                .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                m.owner.store(self_id, Ordering::Relaxed);
                m.count.store(1, Ordering::Relaxed);
                return 0;
            }
            core::hint::spin_loop();
        }
        // Yield to other threads for ~1 ms.
        let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000);
    }
}

/// Try to lock a mutex without blocking.
///
/// Returns 0 on success, `EBUSY` if the mutex is already locked
/// (by another thread).  For recursive mutexes, succeeds if the
/// calling thread already holds the lock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_trylock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: caller guarantees mutex is valid.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);
    let self_id = syscall::syscall0(syscall::SYS_TASK_ID) as i32;

    // Recursive: if we already own it, increment count.
    if kind == PTHREAD_MUTEX_RECURSIVE
        && m.locked.load(Ordering::Acquire) != 0
        && m.owner.load(Ordering::Relaxed) == self_id
    {
        let c = m.count.load(Ordering::Relaxed);
        m.count.store(c.wrapping_add(1), Ordering::Relaxed);
        return 0;
    }

    if m.locked
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_unlock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: caller guarantees mutex is valid.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);

    if kind == PTHREAD_MUTEX_RECURSIVE || kind == PTHREAD_MUTEX_ERRORCHECK {
        let self_id = syscall::syscall0(syscall::SYS_TASK_ID) as i32;
        if m.owner.load(Ordering::Relaxed) != self_id
            && kind == PTHREAD_MUTEX_ERRORCHECK
        {
            return errno::EPERM;
        }
        // Normal/recursive: POSIX says UB for non-owner unlock on
        // normal, but we silently proceed to avoid crashes.
        if kind == PTHREAD_MUTEX_RECURSIVE {
            let c = m.count.load(Ordering::Relaxed);
            if c > 1 {
                // Still recursed — decrement count, keep lock held.
                m.count.store(c.wrapping_sub(1), Ordering::Relaxed);
                return 0;
            }
        }
    }

    // Release the lock.
    unsafe {
        (*mutex).owner.store(0, Ordering::Relaxed);
        (*mutex).count.store(0, Ordering::Relaxed);
        (*mutex).locked.store(0, Ordering::Release);
    }
    0
}

/// Destroy a mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_destroy(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: caller guarantees mutex is valid.
    unsafe { (*mutex).locked.store(0, Ordering::Release); }
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_once(
    once: *mut PthreadOnceT,
    init: extern "C" fn(),
) -> i32 {
    if once.is_null() {
        return errno::EINVAL;
    }

    // SAFETY: caller guarantees once is valid.
    let done = unsafe { &(*once).done };

    // Fast path: already initialized.
    if done.load(Ordering::Acquire) == 1 {
        return 0;
    }

    // Try to claim the initialization.
    if done.compare_exchange(0, -1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        init();
        done.store(1, Ordering::Release);
    } else {
        // Another thread is initializing — spin until done.
        while done.load(Ordering::Acquire) != 1 {
            core::hint::spin_loop();
        }
    }

    0
}

// ---------------------------------------------------------------------------
// Thread-specific data
// ---------------------------------------------------------------------------

/// Key type for thread-specific data.
pub type PthreadKeyT = u32;

/// Maximum number of TSD keys.
const MAX_KEYS: usize = 64;

/// Thread-specific data values.
///
/// **Limitation**: This is a global array shared by all threads.
/// Proper per-thread TSD requires kernel TLS support (FS/GS segment
/// setup per thread).
static mut TSD_VALUES: [*mut u8; MAX_KEYS] = [core::ptr::null_mut(); MAX_KEYS];
/// Next key index to allocate.
static mut TSD_NEXT_KEY: u32 = 0;

/// Create a thread-specific data key.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_key_create(
    key: *mut PthreadKeyT,
    _destructor: Option<extern "C" fn(*mut u8)>,
) -> i32 {
    if key.is_null() {
        return errno::EINVAL;
    }
    let next = unsafe { core::ptr::addr_of_mut!(TSD_NEXT_KEY).read() };
    if next as usize >= MAX_KEYS {
        return errno::EAGAIN;
    }
    unsafe {
        *key = next;
        core::ptr::addr_of_mut!(TSD_NEXT_KEY).write(next.wrapping_add(1));
    }
    0
}

/// Get thread-specific data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_getspecific(key: PthreadKeyT) -> *mut u8 {
    let vals = unsafe { core::ptr::addr_of_mut!(TSD_VALUES).as_ref() };
    let Some(vals) = vals else { return core::ptr::null_mut() };
    vals.get(key as usize)
        .copied()
        .unwrap_or(core::ptr::null_mut())
}

/// Set thread-specific data.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setspecific(key: PthreadKeyT, value: *mut u8) -> i32 {
    let vals = unsafe { core::ptr::addr_of_mut!(TSD_VALUES).as_mut() };
    let Some(vals) = vals else { return errno::EINVAL };
    if let Some(slot) = vals.get_mut(key as usize) {
        *slot = value;
        0
    } else {
        errno::EINVAL
    }
}

/// Delete a thread-specific data key.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_key_delete(_key: PthreadKeyT) -> i32 {
    0 // No-op: we don't reclaim key indices.
}

// ---------------------------------------------------------------------------
// Condition variables
// ---------------------------------------------------------------------------

/// Initialize a condition variable.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_cond_init(
    cond: *mut PthreadCondT,
    _attr: *const PthreadCondattrT,
) -> i32 {
    if cond.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: cond is non-null.
    unsafe {
        let c = &mut *cond;
        c.generation = AtomicI32::new(0);
    }
    0
}

/// Destroy a condition variable.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_cond_destroy(_cond: *mut PthreadCondT) -> i32 {
    0 // No resources to free.
}

/// Wait on a condition variable.
///
/// Atomically releases `mutex`, waits for a signal/broadcast on `cond`,
/// then re-acquires `mutex`.  Uses a spin-yield loop watching the
/// generation counter — not ideal but correct.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_cond_wait(
    cond: *mut PthreadCondT,
    mutex: *mut PthreadMutexT,
) -> i32 {
    if cond.is_null() || mutex.is_null() {
        return errno::EINVAL;
    }

    // SAFETY: Both pointers verified non-null.
    let c = unsafe { &*cond };
    let current_gen = c.generation.load(Ordering::Acquire);

    // Release the mutex while waiting.
    // SAFETY: mutex verified non-null above.
    unsafe { pthread_mutex_unlock(mutex); }

    // Spin-yield until the generation changes (signal/broadcast happened).
    while c.generation.load(Ordering::Acquire) == current_gen {
        core::hint::spin_loop();
        // Yield the CPU to avoid burning cycles.
        let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000); // 1ms yield.
    }

    // Re-acquire the mutex.
    // SAFETY: mutex verified non-null above.
    unsafe { pthread_mutex_lock(mutex); }
    0
}

/// Wait on a condition variable with a timeout.
///
/// Like `pthread_cond_wait` but returns `ETIMEDOUT` if the absolute
/// time `abstime` passes before a signal.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_cond_timedwait(
    cond: *mut PthreadCondT,
    mutex: *mut PthreadMutexT,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    if cond.is_null() || mutex.is_null() || abstime.is_null() {
        return errno::EINVAL;
    }

    let c = unsafe { &*cond };
    let current_gen = c.generation.load(Ordering::Acquire);

    // SAFETY: mutex verified non-null above.
    unsafe { pthread_mutex_unlock(mutex); }

    // Get current time and compute deadline with full nanosecond precision.
    let abs = unsafe { &*abstime };
    let dl_secs = abs.tv_sec;
    let dl_nanos = abs.tv_nsec;
    let mut now_ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };

    let mut timed_out = false;
    while c.generation.load(Ordering::Acquire) == current_gen {
        let _ = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut now_ts);
        if now_ts.tv_sec > dl_secs
            || (now_ts.tv_sec == dl_secs && now_ts.tv_nsec >= dl_nanos)
        {
            timed_out = true;
            break;
        }
        core::hint::spin_loop();
        let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000); // 1ms yield.
    }

    // SAFETY: mutex verified non-null above.
    unsafe { pthread_mutex_lock(mutex); }
    if timed_out { errno::ETIMEDOUT } else { 0 }
}

/// Signal (wake one waiter on) a condition variable.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_cond_signal(cond: *mut PthreadCondT) -> i32 {
    if cond.is_null() {
        return errno::EINVAL;
    }
    // Bump generation counter — any waiter spinning on it will notice.
    let c = unsafe { &*cond };
    c.generation.fetch_add(1, Ordering::Release);
    0
}

/// Broadcast (wake all waiters on) a condition variable.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_cond_broadcast(cond: *mut PthreadCondT) -> i32 {
    // Same as signal — our spin-based implementation wakes all waiters
    // since they all see the generation change.
    pthread_cond_signal(cond)
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
#[repr(C)]
pub struct PthreadRwlockT {
    state: AtomicI32,
    _pad: [u8; 52],
}

/// Pthread read-write lock attribute type.
pub type PthreadRwlockattrT = [u8; 8];

/// Static initializer for `pthread_rwlock_t`.
#[allow(clippy::declare_interior_mutable_const)]
#[unsafe(no_mangle)]
pub static PTHREAD_RWLOCK_INITIALIZER: PthreadRwlockT = PthreadRwlockT {
    state: AtomicI32::new(0),
    _pad: [0; 52],
};

/// Initialize a read-write lock.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlock_init(
    rwlock: *mut PthreadRwlockT,
    _attr: *const PthreadRwlockattrT,
) -> i32 {
    if rwlock.is_null() {
        return errno::EINVAL;
    }
    unsafe { (*rwlock).state = AtomicI32::new(0); }
    0
}

/// Destroy a read-write lock.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlock_destroy(_rwlock: *mut PthreadRwlockT) -> i32 {
    0
}

/// Acquire a read lock (shared).
///
/// Spins until no writer holds the lock, then increments the reader count.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlock_rdlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EINVAL;
    }
    let rw = unsafe { &*rwlock };
    loop {
        let current = rw.state.load(Ordering::Acquire);
        // If a writer holds the lock (state == -1), spin.
        if current < 0 {
            core::hint::spin_loop();
            let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000);
            continue;
        }
        // Try to add a reader.
        if rw.state.compare_exchange_weak(
            current,
            current.wrapping_add(1),
            Ordering::AcqRel,
            Ordering::Relaxed,
        ).is_ok() {
            return 0;
        }
        core::hint::spin_loop();
    }
}

/// Try to acquire a read lock without blocking.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlock_tryrdlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EINVAL;
    }
    let rw = unsafe { &*rwlock };
    let current = rw.state.load(Ordering::Acquire);
    if current < 0 {
        return errno::EBUSY;
    }
    if rw.state.compare_exchange(
        current,
        current.wrapping_add(1),
        Ordering::AcqRel,
        Ordering::Relaxed,
    ).is_ok() {
        0
    } else {
        errno::EBUSY
    }
}

/// Acquire a write lock (exclusive).
///
/// Spins until no readers or writers hold the lock.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlock_wrlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EINVAL;
    }
    let rw = unsafe { &*rwlock };
    loop {
        if rw.state.compare_exchange_weak(
            0,
            -1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ).is_ok() {
            return 0;
        }
        core::hint::spin_loop();
        let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000);
    }
}

/// Try to acquire a write lock without blocking.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlock_trywrlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EINVAL;
    }
    let rw = unsafe { &*rwlock };
    if rw.state.compare_exchange(
        0,
        -1,
        Ordering::AcqRel,
        Ordering::Relaxed,
    ).is_ok() {
        0
    } else {
        errno::EBUSY
    }
}

/// Release a read-write lock.
///
/// If the calling thread holds a read lock, decrements the reader count.
/// If the calling thread holds a write lock, releases it (sets state to 0).
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlock_unlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EINVAL;
    }
    let rw = unsafe { &*rwlock };
    let current = rw.state.load(Ordering::Acquire);
    if current == -1 {
        // Writer releasing — set to unlocked.
        rw.state.store(0, Ordering::Release);
    } else if current > 0 {
        // Reader releasing — decrement count.
        rw.state.fetch_sub(1, Ordering::AcqRel);
    }
    // If current == 0, the lock wasn't held — no-op (undefined behavior in POSIX).
    0
}

// ---------------------------------------------------------------------------
// sched_yield — voluntarily yield the CPU
// ---------------------------------------------------------------------------

/// Yield the processor to another thread/process.
#[unsafe(no_mangle)]
pub extern "C" fn sched_yield() -> i32 {
    let _ = syscall::syscall1(syscall::SYS_SLEEP, 0);
    0
}

// ---------------------------------------------------------------------------
// Thread attributes
// ---------------------------------------------------------------------------

/// Initialize a thread attribute object to default values.
///
/// Defaults: joinable (not detached), stack size = `DEFAULT_THREAD_STACK_SIZE`.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_attr_init(attr: *mut PthreadAttrT) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
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
    }
    0
}

/// Destroy a thread attribute object.
///
/// No-op — no resources to release.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_attr_destroy(_attr: *mut PthreadAttrT) -> i32 {
    0
}

/// Set the stack size in a thread attribute object.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_attr_setstacksize(attr: *mut PthreadAttrT, stacksize: usize) -> i32 {
    if attr.is_null() || stacksize < 4096 {
        return errno::EINVAL;
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
#[unsafe(no_mangle)]
pub extern "C" fn pthread_attr_getstacksize(attr: *const PthreadAttrT, stacksize: *mut usize) -> i32 {
    if attr.is_null() || stacksize.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: both pointers verified non-null.
    // Use read_unaligned because PthreadAttrT has align(1).
    unsafe {
        let stored = core::ptr::read_unaligned(attr.cast::<usize>());
        let sz = if stored == 0 { DEFAULT_THREAD_STACK_SIZE } else { stored };
        *stacksize = sz;
    }
    0
}

/// Detach-state constants for `pthread_attr_setdetachstate`.
pub const PTHREAD_CREATE_JOINABLE: i32 = 0;
pub const PTHREAD_CREATE_DETACHED: i32 = 1;

/// Set the detach state in a thread attribute object.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_attr_setdetachstate(attr: *mut PthreadAttrT, detachstate: i32) -> i32 {
    if attr.is_null() || (detachstate != PTHREAD_CREATE_JOINABLE && detachstate != PTHREAD_CREATE_DETACHED) {
        return errno::EINVAL;
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
#[unsafe(no_mangle)]
pub extern "C" fn pthread_attr_getdetachstate(attr: *const PthreadAttrT, detachstate: *mut i32) -> i32 {
    if attr.is_null() || detachstate.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: both pointers verified non-null.
    // Use read_unaligned because attr+8 may not be i32-aligned.
    unsafe {
        *detachstate = core::ptr::read_unaligned(attr.cast::<u8>().add(8).cast::<i32>());
    }
    0
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
    /// Generation counter — incremented when the barrier trips.
    generation: AtomicI32,
    /// Padding for alignment.
    _pad: [u8; 44],
}

/// Pthread barrier attribute type.
pub type PthreadBarrierattrT = [u8; 8];

/// Return value for the one thread designated as the "serial thread".
pub const PTHREAD_BARRIER_SERIAL_THREAD: i32 = -1;

/// Initialize a barrier.
///
/// `count` is the number of threads that must call `pthread_barrier_wait`
/// before any of them successfully return.  Must be > 0.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_barrier_init(
    barrier: *mut PthreadBarrierT,
    _attr: *const PthreadBarrierattrT,
    count: u32,
) -> i32 {
    if barrier.is_null() || count == 0 {
        return errno::EINVAL;
    }
    // SAFETY: barrier is non-null.
    unsafe {
        (*barrier).count = count;
        (*barrier).current = AtomicI32::new(0);
        (*barrier).generation = AtomicI32::new(0);
    }
    0
}

/// Destroy a barrier.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_barrier_destroy(_barrier: *mut PthreadBarrierT) -> i32 {
    0
}

/// Wait at a barrier.
///
/// Blocks until `count` threads have called this function on the same
/// barrier.  Exactly one thread returns `PTHREAD_BARRIER_SERIAL_THREAD`;
/// all others return 0.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_barrier_wait(barrier: *mut PthreadBarrierT) -> i32 {
    if barrier.is_null() {
        return errno::EINVAL;
    }

    let b = unsafe { &*barrier };
    let my_gen = b.generation.load(Ordering::Acquire);

    // Increment arrival count.
    let arrived = b.current.fetch_add(1, Ordering::AcqRel).wrapping_add(1);

    if arrived as u32 == b.count {
        // Last thread to arrive — reset counter and bump generation.
        b.current.store(0, Ordering::Release);
        b.generation.fetch_add(1, Ordering::Release);
        return PTHREAD_BARRIER_SERIAL_THREAD;
    }

    // Not the last — spin-yield until the generation changes.
    while b.generation.load(Ordering::Acquire) == my_gen {
        core::hint::spin_loop();
        let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000);
    }
    0
}

// ---------------------------------------------------------------------------
// Pthread spinlocks
// ---------------------------------------------------------------------------

/// Pthread spinlock type.
pub type PthreadSpinlockT = AtomicI32;

/// Initialize a spinlock.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_spin_init(lock: *mut PthreadSpinlockT, _pshared: i32) -> i32 {
    if lock.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: lock is non-null.
    unsafe {
        core::ptr::addr_of_mut!(*lock).write(AtomicI32::new(0));
    }
    0
}

/// Destroy a spinlock.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_spin_destroy(_lock: *mut PthreadSpinlockT) -> i32 {
    0
}

/// Acquire a spinlock.
///
/// Busy-waits until the lock is acquired.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_spin_lock(lock: *mut PthreadSpinlockT) -> i32 {
    if lock.is_null() {
        return errno::EINVAL;
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
#[unsafe(no_mangle)]
pub extern "C" fn pthread_spin_trylock(lock: *mut PthreadSpinlockT) -> i32 {
    if lock.is_null() {
        return errno::EINVAL;
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
#[unsafe(no_mangle)]
pub extern "C" fn pthread_spin_unlock(lock: *mut PthreadSpinlockT) -> i32 {
    if lock.is_null() {
        return errno::EINVAL;
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

/// Set the calling thread's cancellation state.
///
/// Stub: succeeds silently, stores the old state.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_setcancelstate(state: i32, oldstate: *mut i32) -> i32 {
    if !oldstate.is_null() {
        // Report that cancellation was enabled (harmless default).
        // SAFETY: caller guarantees oldstate is valid if non-null.
        unsafe { *oldstate = PTHREAD_CANCEL_ENABLE; }
    }
    let _ = state;
    0
}

/// Set the calling thread's cancellation type.
///
/// Stub: succeeds silently.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_setcanceltype(cancel_type: i32, oldtype: *mut i32) -> i32 {
    if !oldtype.is_null() {
        // SAFETY: caller guarantees oldtype is valid if non-null.
        unsafe { *oldtype = PTHREAD_CANCEL_DEFERRED; }
    }
    let _ = cancel_type;
    0
}

/// Create a cancellation point.
///
/// Stub: no-op (cancellation is not supported).
#[unsafe(no_mangle)]
pub extern "C" fn pthread_testcancel() {}

/// Cancel a thread.
///
/// Stub: returns ENOSYS (cancellation is not supported).
#[unsafe(no_mangle)]
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

/// Initialize a mutex attribute object.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_mutexattr_init(attr: *mut PthreadMutexattrT) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: attr is non-null.
    unsafe { core::ptr::write_bytes(attr.cast::<u8>(), 0, core::mem::size_of::<PthreadMutexattrT>()); }
    0
}

/// Destroy a mutex attribute object.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_mutexattr_destroy(_attr: *mut PthreadMutexattrT) -> i32 {
    0
}

/// Set the mutex type attribute.
///
/// Supported types: `PTHREAD_MUTEX_NORMAL` (default),
/// `PTHREAD_MUTEX_RECURSIVE`, `PTHREAD_MUTEX_ERRORCHECK`.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_mutexattr_settype(attr: *mut PthreadMutexattrT, kind: i32) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    if !(0..=2).contains(&kind) {
        return errno::EINVAL;
    }
    // Store kind in first 4 bytes.
    // SAFETY: attr is non-null and 8 bytes.
    // Use write_unaligned because PthreadMutexattrT is [u8; 8] with align(1).
    unsafe { core::ptr::write_unaligned(attr.cast::<i32>(), kind); }
    0
}

/// Get the mutex type attribute.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_mutexattr_gettype(attr: *const PthreadMutexattrT, kind: *mut i32) -> i32 {
    if attr.is_null() || kind.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: both pointers verified non-null.
    // Use read_unaligned because PthreadMutexattrT is [u8; 8] with align(1).
    unsafe { *kind = core::ptr::read_unaligned(attr.cast::<i32>()); }
    0
}

// ---------------------------------------------------------------------------
// pthread_mutex_timedlock
// ---------------------------------------------------------------------------

/// Lock a mutex with a timeout.
///
/// Attempts to lock the mutex.  If the mutex is already locked, blocks
/// until the mutex becomes available or the absolute timeout `abstime`
/// expires.
///
/// Returns 0 on success, ETIMEDOUT on timeout, EINVAL on error.
/// For recursive mutexes, succeeds immediately if already held by
/// calling thread.  For error-checking, returns EDEADLK.
///
/// # Safety
///
/// `mutex` must point to a valid initialized mutex.
/// `abstime` must point to a valid `timespec`.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_mutex_timedlock(
    mutex: *mut PthreadMutexT,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    if mutex.is_null() || abstime.is_null() {
        return errno::EINVAL;
    }

    // SAFETY: mutex verified non-null.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);
    let self_id = syscall::syscall0(syscall::SYS_TASK_ID) as i32;

    // Recursive / error-checking: check if we already own the lock.
    if (kind == PTHREAD_MUTEX_RECURSIVE || kind == PTHREAD_MUTEX_ERRORCHECK)
        && m.locked.load(Ordering::Acquire) != 0
        && m.owner.load(Ordering::Relaxed) == self_id
    {
        if kind == PTHREAD_MUTEX_RECURSIVE {
            let c = m.count.load(Ordering::Relaxed);
            m.count.store(c.wrapping_add(1), Ordering::Relaxed);
            return 0;
        }
        return errno::EDEADLK;
    }

    // Fast path: try to acquire immediately.
    if m.locked.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        m.owner.store(self_id, Ordering::Relaxed);
        m.count.store(1, Ordering::Relaxed);
        return 0;
    }

    // Spin with timeout check.
    let dl_secs = unsafe { (*abstime).tv_sec };
    let dl_nanos = unsafe { (*abstime).tv_nsec };

    loop {
        // Check timeout by reading current time.
        let mut now = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        let _ = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut now);

        if now.tv_sec > dl_secs
            || (now.tv_sec == dl_secs && now.tv_nsec >= dl_nanos)
        {
            return errno::ETIMEDOUT;
        }

        // Try to acquire.
        if m.locked.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            m.owner.store(self_id, Ordering::Relaxed);
            m.count.store(1, Ordering::Relaxed);
            return 0;
        }

        // Yield to avoid burning CPU.
        sched_yield();
    }
}

// ---------------------------------------------------------------------------
// Condition variable attributes
// ---------------------------------------------------------------------------

/// Initialize a condition variable attribute object.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_condattr_init(attr: *mut PthreadCondattrT) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: attr verified non-null; zeroing a [u8; 8] is safe.
    unsafe { core::ptr::write_bytes(attr.cast::<u8>(), 0, core::mem::size_of::<PthreadCondattrT>()); }
    0
}

/// Destroy a condition variable attribute object.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_condattr_destroy(_attr: *mut PthreadCondattrT) -> i32 {
    0
}

/// Set the clock for a condition variable attribute.
///
/// Stores the clock ID for use by `pthread_cond_timedwait`.
/// We accept any valid clock but our timedwait currently only uses
/// the real-time clock.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_condattr_setclock(attr: *mut PthreadCondattrT, clock_id: i32) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    // Validate clock_id.
    if clock_id != crate::time::CLOCK_REALTIME && clock_id != crate::time::CLOCK_MONOTONIC {
        return errno::EINVAL;
    }
    // Store in first 4 bytes.
    unsafe { core::ptr::write_unaligned(attr.cast::<i32>(), clock_id); }
    0
}

/// Get the clock for a condition variable attribute.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_condattr_getclock(attr: *const PthreadCondattrT, clock_id: *mut i32) -> i32 {
    if attr.is_null() || clock_id.is_null() {
        return errno::EINVAL;
    }
    unsafe { *clock_id = core::ptr::read_unaligned(attr.cast::<i32>()); }
    0
}

// ---------------------------------------------------------------------------
// Read-write lock attributes
// ---------------------------------------------------------------------------

/// Initialize a rwlock attribute object.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlockattr_init(attr: *mut PthreadRwlockattrT) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    unsafe { core::ptr::write_bytes(attr.cast::<u8>(), 0, core::mem::size_of::<PthreadRwlockattrT>()); }
    0
}

/// Destroy a rwlock attribute object.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlockattr_destroy(_attr: *mut PthreadRwlockattrT) -> i32 {
    0
}

/// Set the process-shared attribute for a rwlock.
///
/// We only support `PTHREAD_PROCESS_PRIVATE` (0).
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlockattr_setpshared(attr: *mut PthreadRwlockattrT, pshared: i32) -> i32 {
    if attr.is_null() {
        return errno::EINVAL;
    }
    if pshared != 0 {
        // PTHREAD_PROCESS_SHARED not supported.
        return errno::ENOTSUP;
    }
    unsafe { core::ptr::write_unaligned(attr.cast::<i32>(), pshared); }
    0
}

/// Get the process-shared attribute for a rwlock.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_rwlockattr_getpshared(attr: *const PthreadRwlockattrT, pshared: *mut i32) -> i32 {
    if attr.is_null() || pshared.is_null() {
        return errno::EINVAL;
    }
    unsafe { *pshared = core::ptr::read_unaligned(attr.cast::<i32>()); }
    0
}

// ---------------------------------------------------------------------------
// pthread_setname_np / pthread_getname_np (GNU extensions)
// ---------------------------------------------------------------------------

/// Maximum thread name length (including null terminator).
/// Linux limit is 16 bytes.
const PTHREAD_NAME_MAX: usize = 16;

/// Thread name storage.
///
/// Simple global array indexed by task ID modulo array size.
/// Not ideal (collisions possible) but sufficient for basic use.
/// A real implementation would store names per-thread in TLS.
const MAX_NAMED_THREADS: usize = 64;
static mut THREAD_NAMES: [[u8; PTHREAD_NAME_MAX]; MAX_NAMED_THREADS] =
    [[0u8; PTHREAD_NAME_MAX]; MAX_NAMED_THREADS];

/// Set the name of a thread (GNU extension).
///
/// `name` must be a null-terminated string of at most 15 characters
/// (plus null).  Returns 0 on success, ERANGE if too long.
///
/// # Safety
///
/// `name` must be a valid null-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_setname_np(thread: PthreadT, name: *const u8) -> i32 {
    if name.is_null() {
        return errno::EINVAL;
    }

    let name_len = unsafe { crate::string::strlen(name) };
    if name_len >= PTHREAD_NAME_MAX {
        return errno::ERANGE;
    }

    let idx = (thread as usize) % MAX_NAMED_THREADS;

    // SAFETY: Single-threaded access assumption (same as rest of posix crate).
    let slot = unsafe {
        core::ptr::addr_of_mut!(THREAD_NAMES)
            .as_mut()
            .and_then(|names| names.get_mut(idx))
    };
    let Some(slot) = slot else { return errno::EINVAL; };
    let mut i: usize = 0;
    while i < name_len {
        if let Some(s) = slot.get_mut(i) {
            *s = unsafe { *name.add(i) };
        }
        i = i.wrapping_add(1);
    }
    if let Some(s) = slot.get_mut(i) {
        *s = 0;
    }

    0
}

/// Get the name of a thread (GNU extension).
///
/// Copies the thread name into `name` (at most `len` bytes including null).
/// Returns 0 on success, ERANGE if buffer too small.
///
/// # Safety
///
/// `name` must be valid for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_getname_np(thread: PthreadT, name: *mut u8, len: usize) -> i32 {
    if name.is_null() || len == 0 {
        return errno::EINVAL;
    }

    let idx = (thread as usize) % MAX_NAMED_THREADS;

    // SAFETY: Single-threaded access assumption.
    let slot = unsafe {
        core::ptr::addr_of!(THREAD_NAMES)
            .as_ref()
            .and_then(|names| names.get(idx))
    };
    let Some(slot) = slot else { return errno::EINVAL; };
    let name_len = {
        let mut l: usize = 0;
        while l < PTHREAD_NAME_MAX {
            if slot.get(l).copied().unwrap_or(0) == 0 {
                break;
            }
            l = l.wrapping_add(1);
        }
        l
    };

    if name_len.wrapping_add(1) > len {
        return errno::ERANGE;
    }

    let mut i: usize = 0;
    while i < name_len {
        unsafe { *name.add(i) = slot.get(i).copied().unwrap_or(0); }
        i = i.wrapping_add(1);
    }
    unsafe { *name.add(i) = 0; }

    0
}

// ---------------------------------------------------------------------------
// pthread_atfork
// ---------------------------------------------------------------------------

/// Register handlers to be called before/after fork.
///
/// Since our OS doesn't have fork() yet, this is a stub that accepts
/// handlers but never calls them.  Returns 0 (success) always.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_atfork(
    _prepare: Option<extern "C" fn()>,
    _parent: Option<extern "C" fn()>,
    _child: Option<extern "C" fn()>,
) -> i32 {
    0
}
