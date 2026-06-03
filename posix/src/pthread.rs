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

/// Pthread mutex attribute type (glibc x86_64: 4 bytes).
pub type PthreadMutexattrT = [u8; 4];

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

/// Pthread condition variable attribute type (glibc x86_64: 4 bytes).
pub type PthreadCondattrT = [u8; 4];

/// Static initializer for `pthread_cond_t`.
#[allow(clippy::declare_interior_mutable_const)]
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
    kind: AtomicI32::new(0), // PTHREAD_MUTEX_NORMAL
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

/// Look up thread info by kernel task ID without removing it.
///
/// Used by `pthread_getattr_np` to report a live thread's stack bounds.
#[cfg(target_os = "none")]
fn find_thread_info(task_id: u64) -> Option<ThreadInfo> {
    // SAFETY: Same single-creator convention as store_thread_info.
    unsafe {
        let table = &*thread_table_ptr();
        for slot in table.iter() {
            if let Some(info) = slot.as_ref() {
                if info.task_id == task_id {
                    return Some(*info);
                }
            }
        }
    }
    None
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
#[cfg(target_os = "none")]
core::arch::global_asm!(
    ".global _pthread_trampoline",
    ".type _pthread_trampoline, @function",
    "_pthread_trampoline:",
    "    pop rdi",      // rdi = arg
    "    pop rsi",      // rsi = start_routine
    "    call rsi",     // rax = start_routine(arg)
    "    mov rdi, rax", // exit value = return value
    "    mov eax, 511", // SYS_THREAD_EXIT
    "    syscall",
    "    ud2", // unreachable
);

#[cfg(target_os = "none")]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
    #[cfg(target_os = "none")]
    let entry = _pthread_trampoline as *const () as u64;
    #[cfg(not(target_os = "none"))]
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
        unsafe {
            *thread = task_id;
        }
    }

    0
}

/// Wait for a thread to terminate.
///
/// Blocks until the specified thread exits, stores its return value
/// in `*retval` (if non-null), and frees the thread's stack.
///
/// Returns 0 on success, or a POSIX error number on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_join(thread_id: PthreadT, retval: *mut *mut u8) -> i32 {
    let ret = syscall::syscall1(syscall::SYS_THREAD_JOIN, thread_id);

    if ret < 0 {
        // The kernel returns negative error codes; map to POSIX.
        return errno::ESRCH;
    }

    // The kernel returns the exit value from SYS_THREAD_EXIT.
    if !retval.is_null() {
        // SAFETY: caller guarantees retval is a valid pointer.
        unsafe {
            *retval = ret as *mut u8;
        }
    }

    // Free the thread's stack.
    if let Some(info) = take_thread_info(thread_id) {
        let _ = crate::mman::munmap(info.stack_base as *mut core::ffi::c_void, info.stack_size);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_self() -> PthreadT {
    syscall::syscall0(syscall::SYS_TASK_ID) as PthreadT
}

/// Compare two thread IDs.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_equal(t1: PthreadT, t2: PthreadT) -> i32 {
    i32::from(t1 == t2)
}

/// Terminate the calling thread.
///
/// Issues `SYS_THREAD_EXIT` with the specified return value.
/// If this is the last thread in the process, the process exits.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_exit(retval: *mut u8) -> ! {
    let _ = syscall::syscall1(syscall::SYS_THREAD_EXIT, retval as u64);
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

/// Maximum spin iterations before yielding on a contended mutex.
const MUTEX_SPIN_LIMIT: u32 = 100;

/// Initialize a mutex.
///
/// Reads the mutex type from `attr` (if non-null) to determine whether
/// the mutex is normal, recursive, or error-checking.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_init(
    mutex: *mut PthreadMutexT,
    attr: *const PthreadMutexattrT,
) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_lock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_trylock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_mutex_unlock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: caller guarantees mutex is valid.
    let m = unsafe { &*mutex };
    let kind = m.kind.load(Ordering::Relaxed);

    if kind == PTHREAD_MUTEX_RECURSIVE || kind == PTHREAD_MUTEX_ERRORCHECK {
        let self_id = syscall::syscall0(syscall::SYS_TASK_ID) as i32;
        if m.owner.load(Ordering::Relaxed) != self_id {
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

    // Release the lock.
    unsafe {
        (*mutex).owner.store(0, Ordering::Relaxed);
        (*mutex).count.store(0, Ordering::Relaxed);
        (*mutex).locked.store(0, Ordering::Release);
    }
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_once(once: *mut PthreadOnceT, init: extern "C" fn()) -> i32 {
    if once.is_null() {
        return errno::EFAULT;
    }

    // SAFETY: caller guarantees once is valid.
    let done = unsafe { &(*once).done };

    // Fast path: already initialized.
    if done.load(Ordering::Acquire) == 1 {
        return 0;
    }

    // Try to claim the initialization.
    if done
        .compare_exchange(0, -1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_key_create(
    key: *mut PthreadKeyT,
    _destructor: Option<extern "C" fn(*mut u8)>,
) -> i32 {
    if key.is_null() {
        return errno::EFAULT;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_getspecific(key: PthreadKeyT) -> *mut u8 {
    let vals = unsafe { core::ptr::addr_of_mut!(TSD_VALUES).as_ref() };
    let Some(vals) = vals else {
        return core::ptr::null_mut();
    };
    vals.get(key as usize)
        .copied()
        .unwrap_or(core::ptr::null_mut())
}

/// Set thread-specific data.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_setspecific(key: PthreadKeyT, value: *mut u8) -> i32 {
    let vals = unsafe { core::ptr::addr_of_mut!(TSD_VALUES).as_mut() };
    let Some(vals) = vals else {
        return errno::EINVAL;
    };
    if let Some(slot) = vals.get_mut(key as usize) {
        *slot = value;
        0
    } else {
        errno::EINVAL
    }
}

/// Delete a thread-specific data key.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_key_delete(_key: PthreadKeyT) -> i32 {
    0 // No-op: we don't reclaim key indices.
}

// ---------------------------------------------------------------------------
// Condition variables
// ---------------------------------------------------------------------------

/// Initialize a condition variable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_init(
    cond: *mut PthreadCondT,
    _attr: *const PthreadCondattrT,
) -> i32 {
    if cond.is_null() {
        return errno::EFAULT;
    }
    // SAFETY: cond is non-null.
    unsafe {
        let c = &mut *cond;
        c.generation = AtomicI32::new(0);
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
/// Atomically releases `mutex`, waits for a signal/broadcast on `cond`,
/// then re-acquires `mutex`.  Uses a spin-yield loop watching the
/// generation counter — not ideal but correct.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_wait(cond: *mut PthreadCondT, mutex: *mut PthreadMutexT) -> i32 {
    if cond.is_null() || mutex.is_null() {
        return errno::EFAULT;
    }

    // SAFETY: Both pointers verified non-null.
    let c = unsafe { &*cond };
    let current_gen = c.generation.load(Ordering::Acquire);

    // Release the mutex while waiting.
    // SAFETY: mutex verified non-null above.
    unsafe {
        pthread_mutex_unlock(mutex);
    }

    // Spin-yield until the generation changes (signal/broadcast happened).
    while c.generation.load(Ordering::Acquire) == current_gen {
        core::hint::spin_loop();
        // Yield the CPU to avoid burning cycles.
        let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000); // 1ms yield.
    }

    // Re-acquire the mutex.
    // SAFETY: mutex verified non-null above.
    unsafe {
        pthread_mutex_lock(mutex);
    }
    0
}

/// Wait on a condition variable with a timeout.
///
/// Like `pthread_cond_wait` but returns `ETIMEDOUT` if the absolute
/// time `abstime` passes before a signal.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_timedwait(
    cond: *mut PthreadCondT,
    mutex: *mut PthreadMutexT,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    if cond.is_null() || mutex.is_null() || abstime.is_null() {
        return errno::EFAULT;
    }

    let c = unsafe { &*cond };
    let current_gen = c.generation.load(Ordering::Acquire);

    // SAFETY: mutex verified non-null above.
    unsafe {
        pthread_mutex_unlock(mutex);
    }

    // Get current time and compute deadline with full nanosecond precision.
    let abs = unsafe { &*abstime };
    let dl_secs = abs.tv_sec;
    let dl_nanos = abs.tv_nsec;
    let mut now_ts = crate::stat::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };

    let mut timed_out = false;
    while c.generation.load(Ordering::Acquire) == current_gen {
        let _ = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut now_ts);
        if now_ts.tv_sec > dl_secs || (now_ts.tv_sec == dl_secs && now_ts.tv_nsec >= dl_nanos) {
            timed_out = true;
            break;
        }
        core::hint::spin_loop();
        let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000); // 1ms yield.
    }

    // SAFETY: mutex verified non-null above.
    unsafe {
        pthread_mutex_lock(mutex);
    }
    if timed_out { errno::ETIMEDOUT } else { 0 }
}

/// Signal (wake one waiter on) a condition variable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_cond_signal(cond: *mut PthreadCondT) -> i32 {
    if cond.is_null() {
        return errno::EFAULT;
    }
    // Bump generation counter — any waiter spinning on it will notice.
    let c = unsafe { &*cond };
    c.generation.fetch_add(1, Ordering::Release);
    0
}

/// Broadcast (wake all waiters on) a condition variable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static PTHREAD_RWLOCK_INITIALIZER: PthreadRwlockT = PthreadRwlockT {
    state: AtomicI32::new(0),
    _pad: [0; 52],
};

/// Initialize a read-write lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_init(
    rwlock: *mut PthreadRwlockT,
    _attr: *const PthreadRwlockattrT,
) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
    }
    unsafe {
        (*rwlock).state = AtomicI32::new(0);
    }
    0
}

/// Destroy a read-write lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_destroy(_rwlock: *mut PthreadRwlockT) -> i32 {
    0
}

/// Acquire a read lock (shared).
///
/// Spins until no writer holds the lock, then increments the reader count.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_rdlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
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
        if rw
            .state
            .compare_exchange_weak(
                current,
                current.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            return 0;
        }
        core::hint::spin_loop();
    }
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
/// Spins until no readers or writers hold the lock.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_wrlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
    }
    let rw = unsafe { &*rwlock };
    loop {
        if rw
            .state
            .compare_exchange_weak(0, -1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return 0;
        }
        core::hint::spin_loop();
        let _ = syscall::syscall1(syscall::SYS_SLEEP, 1_000_000);
    }
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
        .compare_exchange(0, -1, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        0
    } else {
        errno::EBUSY
    }
}

/// Release a read-write lock.
///
/// If the calling thread holds a read lock, decrements the reader count.
/// If the calling thread holds a write lock, releases it (sets state to 0).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlock_unlock(rwlock: *mut PthreadRwlockT) -> i32 {
    if rwlock.is_null() {
        return errno::EFAULT;
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
//   [16..24)  stack address — lowest address of the stack region (usize)
//   [24..32)  guard size    (usize)
//
// Offsets 12..16 and 32..56 are reserved/unused.  These offsets are an
// internal contract only — C callers treat the type as opaque.
const ATTR_OFF_STACKSIZE: usize = 0;
// Only `encode_attr` (main/created-thread fill path) writes the detach
// field via this constant; the get/set detachstate accessors use a
// literal offset, so on host-without-test builds this would be unused.
#[cfg(any(target_os = "none", test))]
const ATTR_OFF_DETACH: usize = 8;
const ATTR_OFF_STACKADDR: usize = 16;
const ATTR_OFF_GUARDSIZE: usize = 24;

/// Default thread guard size: one 16 KiB page.
///
/// Must match the kernel page/guard granularity (`FRAME_SIZE` in
/// `kernel/src/mm`).  Only referenced when filling main-thread attributes.
#[cfg(any(target_os = "none", test))]
const DEFAULT_GUARD_SIZE: usize = 16 * 1024;

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
        core::ptr::write_unaligned(p.add(ATTR_OFF_STACKADDR).cast::<usize>(), attr.addr);
        core::ptr::write_unaligned(p.add(ATTR_OFF_GUARDSIZE).cast::<usize>(), attr.guard);
    }
}

/// Read the stored stack address from an attribute buffer.
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
/// thread table and its mmap'd stack bounds are returned (no guard page is
/// installed for created threads, so `guard` is 0).  Otherwise the thread
/// is assumed to be the main thread and the kernel main-stack geometry is
/// reported.
#[cfg(target_os = "none")]
fn resolve_thread_stack_attr(task_id: u64) -> StackAttr {
    if let Some(info) = find_thread_info(task_id) {
        StackAttr {
            addr: info.stack_base,
            size: info.stack_size,
            guard: 0,
            detached: info.detached,
        }
    } else {
        main_thread_stack_attr()
    }
}

/// Initialize a thread attribute object to default values.
///
/// Defaults: joinable (not detached), stack size = `DEFAULT_THREAD_STACK_SIZE`.
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
    }
    0
}

/// Destroy a thread attribute object.
///
/// No-op — no resources to release.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_destroy(_attr: *mut PthreadAttrT) -> i32 {
    0
}

/// Set the stack size in a thread attribute object.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setstacksize(attr: *mut PthreadAttrT, stacksize: usize) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    if stacksize < 4096 {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setdetachstate(attr: *mut PthreadAttrT, detachstate: i32) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    if detachstate != PTHREAD_CREATE_JOINABLE && detachstate != PTHREAD_CREATE_DETACHED {
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
    let addr = attr_read_stackaddr(buf);
    // Same semantics as pthread_attr_getstacksize: a stored 0 means the
    // size was never set, so report the default.
    // SAFETY: attr non-null; reading 8 bytes at offset 0 is in-bounds.
    let stored = unsafe { core::ptr::read_unaligned(attr.cast::<usize>()) };
    let size = if stored == 0 {
        DEFAULT_THREAD_STACK_SIZE
    } else {
        stored
    };
    // SAFETY: both out-pointers verified non-null above.
    unsafe {
        *stackaddr = addr as *mut core::ffi::c_void;
        *stacksize = size;
    }
    0
}

/// Set both the stack address and size in a thread attribute object.
///
/// `stackaddr` is the lowest address of the caller-provided stack region.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_attr_setstack(
    attr: *mut PthreadAttrT,
    stackaddr: *mut core::ffi::c_void,
    stacksize: usize,
) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    if stacksize < 4096 {
        return errno::EINVAL;
    }
    let p = attr.cast::<u8>();
    // SAFETY: attr is non-null; writing 8 bytes at offsets 0 and 16 ends at
    // index ≤ 23 < 56.  Unaligned because PthreadAttrT has align(1).
    unsafe {
        core::ptr::write_unaligned(p.add(ATTR_OFF_STACKSIZE).cast::<usize>(), stacksize);
        core::ptr::write_unaligned(
            p.add(ATTR_OFF_STACKADDR).cast::<usize>(),
            stackaddr as usize,
        );
    }
    0
}

/// Get the guard size from a thread attribute object.
///
/// Returns the recorded guard size (0 if none was set).
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
    /// Padding to reach glibc x86_64 size (32 bytes total).
    _pad: [u8; 20],
}

/// Pthread barrier attribute type (glibc x86_64: 4 bytes).
pub type PthreadBarrierattrT = [u8; 4];

/// Return value for the one thread designated as the "serial thread".
pub const PTHREAD_BARRIER_SERIAL_THREAD: i32 = -1;

/// Initialize a barrier.
///
/// `count` is the number of threads that must call `pthread_barrier_wait`
/// before any of them successfully return.  Must be > 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrier_init(
    barrier: *mut PthreadBarrierT,
    _attr: *const PthreadBarrierattrT,
    count: u32,
) -> i32 {
    if barrier.is_null() {
        return errno::EFAULT;
    }
    if count == 0 {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrier_destroy(_barrier: *mut PthreadBarrierT) -> i32 {
    0
}

/// Wait at a barrier.
///
/// Blocks until `count` threads have called this function on the same
/// barrier.  Exactly one thread returns `PTHREAD_BARRIER_SERIAL_THREAD`;
/// all others return 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_barrier_wait(barrier: *mut PthreadBarrierT) -> i32 {
    if barrier.is_null() {
        return errno::EFAULT;
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

// Cancellation state and type are per-thread under POSIX, but our
// libc runs in a context where threading is effectively
// single-tasked (or pthread is a thin wrapper).  We track the values
// in process-global atomics so that:
//
//   * the previous value is correctly reported via the `old*` out
//     pointer, even across non-default settings;
//   * a successful set() reliably persists until the next set();
//   * tests can observe and reset the state deterministically.
//
// This is a stub-quality model, but it gives callers the documented
// pthread contract instead of a pair of constants.
static CANCEL_STATE: AtomicI32 = AtomicI32::new(PTHREAD_CANCEL_ENABLE);
static CANCEL_TYPE: AtomicI32 = AtomicI32::new(PTHREAD_CANCEL_DEFERRED);

/// Inspect the current cancellation state (test/debug helper).
#[must_use]
pub fn current_cancel_state() -> i32 {
    CANCEL_STATE.load(Ordering::Relaxed)
}

/// Inspect the current cancellation type (test/debug helper).
#[must_use]
pub fn current_cancel_type() -> i32 {
    CANCEL_TYPE.load(Ordering::Relaxed)
}

/// Reset the cancellation state/type back to POSIX defaults.  Used by
/// tests to avoid cross-test contamination of the static atomics.
#[cfg(test)]
pub(crate) fn reset_cancel_state_and_type() {
    CANCEL_STATE.store(PTHREAD_CANCEL_ENABLE, Ordering::Relaxed);
    CANCEL_TYPE.store(PTHREAD_CANCEL_DEFERRED, Ordering::Relaxed);
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
/// On success we atomically swap the new value into `CANCEL_STATE`
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
    // Swap atomically so concurrent callers can never observe an
    // intermediate state.
    let prev = CANCEL_STATE.swap(state, Ordering::Relaxed);
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
    let prev = CANCEL_TYPE.swap(cancel_type, Ordering::Relaxed);
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutexattr_settype(attr: *mut PthreadMutexattrT, kind: i32) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    if !(0..=2).contains(&kind) {
        return errno::EINVAL;
    }
    // Store kind in first 4 bytes (all 4 bytes of the attr).
    // SAFETY: attr is non-null and 4 bytes.
    // Use write_unaligned because PthreadMutexattrT is [u8; 4] with align(1).
    unsafe {
        core::ptr::write_unaligned(attr.cast::<i32>(), kind);
    }
    0
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
        *kind = core::ptr::read_unaligned(attr.cast::<i32>());
    }
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_mutex_timedlock(
    mutex: *mut PthreadMutexT,
    abstime: *const crate::stat::Timespec,
) -> i32 {
    if mutex.is_null() || abstime.is_null() {
        return errno::EFAULT;
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
    if m.locked
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        m.owner.store(self_id, Ordering::Relaxed);
        m.count.store(1, Ordering::Relaxed);
        return 0;
    }

    // Spin with timeout check.
    let dl_secs = unsafe { (*abstime).tv_sec };
    let dl_nanos = unsafe { (*abstime).tv_nsec };

    loop {
        // Check timeout by reading current time.
        let mut now = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let _ = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut now);

        if now.tv_sec > dl_secs || (now.tv_sec == dl_secs && now.tv_nsec >= dl_nanos) {
            return errno::ETIMEDOUT;
        }

        // Try to acquire.
        if m.locked
            .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_condattr_setclock(attr: *mut PthreadCondattrT, clock_id: i32) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    // Validate clock_id.
    if clock_id != crate::time::CLOCK_REALTIME && clock_id != crate::time::CLOCK_MONOTONIC {
        return errno::EINVAL;
    }
    // Store in first 4 bytes.
    unsafe {
        core::ptr::write_unaligned(attr.cast::<i32>(), clock_id);
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
        *clock_id = core::ptr::read_unaligned(attr.cast::<i32>());
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
/// We only support `PTHREAD_PROCESS_PRIVATE` (0).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_rwlockattr_setpshared(
    attr: *mut PthreadRwlockattrT,
    pshared: i32,
) -> i32 {
    if attr.is_null() {
        return errno::EFAULT;
    }
    if pshared != PTHREAD_PROCESS_PRIVATE {
        // PTHREAD_PROCESS_SHARED not supported.
        return errno::ENOTSUP;
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_setname_np(thread: PthreadT, name: *const u8) -> i32 {
    if name.is_null() {
        return errno::EFAULT;
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
    let Some(slot) = slot else {
        return errno::EINVAL;
    };
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pthread_getname_np(thread: PthreadT, name: *mut u8, len: usize) -> i32 {
    if name.is_null() {
        return errno::EFAULT;
    }
    if len == 0 {
        return errno::EINVAL;
    }

    let idx = (thread as usize) % MAX_NAMED_THREADS;

    // SAFETY: Single-threaded access assumption.
    let slot = unsafe {
        core::ptr::addr_of!(THREAD_NAMES)
            .as_ref()
            .and_then(|names| names.get(idx))
    };
    let Some(slot) = slot else {
        return errno::EINVAL;
    };
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
        unsafe {
            *name.add(i) = slot.get(i).copied().unwrap_or(0);
        }
        i = i.wrapping_add(1);
    }
    unsafe {
        *name.add(i) = 0;
    }

    0
}

// ---------------------------------------------------------------------------
// pthread_atfork
// ---------------------------------------------------------------------------

/// Register handlers to be called before/after fork.
///
/// Since our OS doesn't have fork() yet, this is a stub that accepts
/// handlers but never calls them.  Returns 0 (success) always.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_atfork(
    _prepare: Option<extern "C" fn()>,
    _parent: Option<extern "C" fn()>,
    _child: Option<extern "C" fn()>,
) -> i32 {
    0
}

// ---------------------------------------------------------------------------
// pthread_setaffinity_np / pthread_getaffinity_np — CPU affinity
// ---------------------------------------------------------------------------

/// CPU set type — bitmask of CPUs.
///
/// Matches the Linux `cpu_set_t` layout (1024 bits = 128 bytes on
/// x86_64).  Each bit corresponds to a CPU number.
#[repr(C)]
pub struct CpuSetT {
    /// Bitmask of CPUs (1024 bits = 128 bytes).
    pub __bits: [u64; 16],
}

impl CpuSetT {
    /// Create an empty CPU set (no CPUs selected).
    pub fn new() -> Self {
        // SAFETY: zero-init is valid for CpuSetT.
        unsafe { core::mem::zeroed() }
    }
}

impl Default for CpuSetT {
    fn default() -> Self {
        Self::new()
    }
}

/// Set a CPU in the CPU set.
pub fn cpu_set(cpu: usize, set: &mut CpuSetT) {
    if cpu < 1024 {
        set.__bits[cpu / 64] |= 1u64 << (cpu % 64);
    }
}

/// Clear a CPU in the CPU set.
pub fn cpu_clr(cpu: usize, set: &mut CpuSetT) {
    if cpu < 1024 {
        set.__bits[cpu / 64] &= !(1u64 << (cpu % 64));
    }
}

/// Test whether a CPU is set in the CPU set.
pub fn cpu_isset(cpu: usize, set: &CpuSetT) -> bool {
    cpu < 1024 && (set.__bits[cpu / 64] & (1u64 << (cpu % 64))) != 0
}

/// Zero all CPUs in the set.
pub fn cpu_zero(set: &mut CpuSetT) {
    set.__bits = [0; 16];
}

/// Count the number of CPUs in the set.
pub fn cpu_count(set: &CpuSetT) -> i32 {
    let mut count: i32 = 0;
    for &word in &set.__bits {
        count = count.wrapping_add(word.count_ones() as i32);
    }
    count
}

/// Set the CPU affinity mask for a thread.
///
/// Stub: returns 0 (success) — our scheduler doesn't support per-thread
/// affinity yet.  The `cpuset` is accepted but not enforced.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_setaffinity_np(
    _thread: PthreadT,
    cpusetsize: usize,
    cpuset: *const CpuSetT,
) -> i32 {
    if cpuset.is_null() {
        return crate::errno::EFAULT;
    }
    if cpusetsize < core::mem::size_of::<CpuSetT>() {
        return crate::errno::EINVAL;
    }
    // Accept silently — no enforcement.
    0
}

/// Get the CPU affinity mask for a thread.
///
/// Stub: returns a mask with all CPUs set (no affinity restrictions).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pthread_getaffinity_np(
    _thread: PthreadT,
    cpusetsize: usize,
    cpuset: *mut CpuSetT,
) -> i32 {
    if cpuset.is_null() {
        return crate::errno::EFAULT;
    }
    if cpusetsize < core::mem::size_of::<CpuSetT>() {
        return crate::errno::EINVAL;
    }
    // SAFETY: caller guarantees cpuset is valid and big enough.
    let set = unsafe { &mut *cpuset };
    // Set all CPUs as available (single-node system).
    set.__bits = [u64::MAX; 16];
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicI32, Ordering};

    // =======================================================================
    // Constants
    // =======================================================================

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

        // Remaining bytes should be zero.
        for &b in &attr[8..] {
            assert_eq!(b, 0, "attr bytes after stack size should be zeroed");
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

    #[test]
    fn attr_setstacksize_minimum_4096() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        // Exactly 4096 should succeed.
        assert_eq!(pthread_attr_setstacksize(&mut attr, 4096), 0);
    }

    #[test]
    fn attr_setstacksize_rejects_too_small() {
        let mut attr: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut attr);
        assert_eq!(pthread_attr_setstacksize(&mut attr, 4095), errno::EINVAL);
        assert_eq!(pthread_attr_setstacksize(&mut attr, 0), errno::EINVAL);
        assert_eq!(pthread_attr_setstacksize(&mut attr, 1), errno::EINVAL);
    }

    #[test]
    fn attr_setstacksize_null_returns_efault() {
        assert_eq!(
            pthread_attr_setstacksize(core::ptr::null_mut(), 8192),
            errno::EFAULT
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

    #[test]
    fn setstack_null_attr_returns_efault() {
        assert_eq!(
            pthread_attr_setstack(core::ptr::null_mut(), core::ptr::null_mut(), 8192),
            errno::EFAULT
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
    fn getguardsize_default_attr_is_zero() {
        // A default-init attr records no guard; getguardsize returns 0.
        let mut buf: PthreadAttrT = [0; 56];
        pthread_attr_init(&mut buf);
        let mut guard: usize = 12345;
        assert_eq!(pthread_attr_getguardsize(&buf, &mut guard), 0);
        assert_eq!(guard, 0);
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
            _pad: [0xFF; 44],
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
            _pad: [0; 44],
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
            _pad: [0xFF; 52],
        };
        let ret = pthread_rwlock_init(&mut rwlock, core::ptr::null());
        assert_eq!(ret, 0);
        assert_eq!(rwlock.state.load(Ordering::Relaxed), 0);
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
            _pad: [0; 52],
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

    #[test]
    fn rwlockattr_setpshared_rejects_invalid() {
        let mut attr: PthreadRwlockattrT = [0; 8];
        pthread_rwlockattr_init(&mut attr);
        assert_eq!(pthread_rwlockattr_setpshared(&mut attr, 2), errno::ENOTSUP);
        assert_eq!(pthread_rwlockattr_setpshared(&mut attr, -1), errno::ENOTSUP);
    }

    #[test]
    fn rwlockattr_setpshared_null_returns_efault() {
        assert_eq!(
            pthread_rwlockattr_setpshared(core::ptr::null_mut(), 0),
            errno::EFAULT
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
            _pad: [0xFF; 20],
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
            _pad: [0; 20],
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
            _pad: [0; 20],
        };
        assert_eq!(pthread_barrier_destroy(&mut barrier), 0);
    }

    #[test]
    fn barrier_init_large_count() {
        let mut barrier = PthreadBarrierT {
            count: 0,
            current: AtomicI32::new(0),
            generation: AtomicI32::new(0),
            _pad: [0; 20],
        };
        let ret = pthread_barrier_init(&mut barrier, core::ptr::null(), u32::MAX);
        assert_eq!(ret, 0);
        assert_eq!(barrier.count, u32::MAX);
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

    #[test]
    fn atfork_returns_zero() {
        assert_eq!(pthread_atfork(None, None, None), 0);
    }

    extern "C" fn dummy_fork_handler() {}

    #[test]
    fn atfork_with_handlers_returns_zero() {
        assert_eq!(
            pthread_atfork(
                Some(dummy_fork_handler),
                Some(dummy_fork_handler),
                Some(dummy_fork_handler)
            ),
            0
        );
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
        assert_eq!(unsafe { pthread_once(&mut once, once_increment) }, 0);
        assert_eq!(unsafe { *core::ptr::addr_of!(ONCE_COUNTER) }, 1);
        // Second call should not invoke init again.
        assert_eq!(unsafe { pthread_once(&mut once, once_increment) }, 0);
        assert_eq!(unsafe { *core::ptr::addr_of!(ONCE_COUNTER) }, 1);
    }

    #[test]
    fn once_null_returns_efault() {
        assert_eq!(
            unsafe { pthread_once(core::ptr::null_mut(), once_increment) },
            errno::EFAULT
        );
    }

    // =======================================================================
    // Thread-specific data (TSD)
    // =======================================================================

    #[test]
    fn tsd_create_set_get() {
        let mut key: PthreadKeyT = 0;
        assert_eq!(unsafe { pthread_key_create(&mut key, None) }, 0);

        let val = 42u8;
        assert_eq!(
            unsafe { pthread_setspecific(key, core::ptr::addr_of!(val) as *mut u8) },
            0
        );

        let got = unsafe { pthread_getspecific(key) };
        assert_eq!(got, core::ptr::addr_of!(val) as *mut u8);
    }

    #[test]
    fn tsd_key_create_null_returns_efault() {
        assert_eq!(
            unsafe { pthread_key_create(core::ptr::null_mut(), None) },
            errno::EFAULT
        );
    }

    #[test]
    fn tsd_key_delete_returns_zero() {
        assert_eq!(pthread_key_delete(0), 0);
    }

    #[test]
    fn tsd_getspecific_invalid_key() {
        // Key beyond MAX_KEYS should return null.
        let got = unsafe { pthread_getspecific(9999) };
        assert!(got.is_null());
    }

    #[test]
    fn tsd_setspecific_invalid_key() {
        let ret = unsafe { pthread_setspecific(9999, core::ptr::null_mut()) };
        assert_eq!(ret, errno::EINVAL);
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
            _pad: [0; 52],
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
            _pad: [0; 52],
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
            _pad: [0; 44],
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
            _pad: [0; 44],
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
            _pad: [0; 44],
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
            _pad: [0; 44],
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
        let ret = unsafe { pthread_setname_np(0, core::ptr::null()) };
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_pthread_setname_np_too_long() {
        // PTHREAD_NAME_MAX is 16, so a 16-char name (excluding null) is too long.
        let name = b"0123456789abcdef\0";
        let ret = unsafe { pthread_setname_np(0, name.as_ptr()) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    #[test]
    fn test_pthread_setname_np_max_valid() {
        // 15 chars + null = exactly PTHREAD_NAME_MAX.
        let name = b"0123456789abcde\0";
        let ret = unsafe { pthread_setname_np(1, name.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_pthread_setname_np_short() {
        let name = b"main\0";
        let ret = unsafe { pthread_setname_np(2, name.as_ptr()) };
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_pthread_getname_np_null() {
        let ret = unsafe { pthread_getname_np(0, core::ptr::null_mut(), 16) };
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_pthread_getname_np_zero_len() {
        let mut buf = [0u8; 16];
        let ret = unsafe { pthread_getname_np(0, buf.as_mut_ptr(), 0) };
        assert_eq!(ret, crate::errno::EINVAL);
    }

    #[test]
    fn test_pthread_setname_getname_roundtrip() {
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
        let name = b"longthreadname\0"; // 14 chars
        let _ = unsafe { pthread_setname_np(4, name.as_ptr()) };

        let mut buf = [0u8; 5]; // Too small for "longthreadname\0" (15 bytes)
        let ret = unsafe { pthread_getname_np(4, buf.as_mut_ptr(), 5) };
        assert_eq!(ret, crate::errno::ERANGE);
    }

    #[test]
    fn test_pthread_setname_empty() {
        let name = b"\0";
        let ret = unsafe { pthread_setname_np(5, name.as_ptr()) };
        assert_eq!(ret, 0);
        let mut buf = [0xFFu8; 16];
        let ret = unsafe { pthread_getname_np(5, buf.as_mut_ptr(), 16) };
        assert_eq!(ret, 0);
        assert_eq!(buf[0], 0, "Empty name should give empty string");
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
            dummy,
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

    #[test]
    fn test_pthread_mutex_timedlock_null_abstime() {
        // SAFETY: zero-init is valid for PthreadMutexT (all-zeros = unlocked).
        let mut m: PthreadMutexT = unsafe { core::mem::zeroed() };
        unsafe {
            pthread_mutex_init(&raw mut m, core::ptr::null());
        }
        let ret = pthread_mutex_timedlock(&raw mut m, core::ptr::null());
        assert_eq!(ret, crate::errno::EFAULT);
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

    // -----------------------------------------------------------------------
    // CpuSetT — CPU set operations
    // -----------------------------------------------------------------------

    #[test]
    fn test_cpu_set_layout() {
        // 16 × u64 = 128 bytes = 1024 bits.
        assert_eq!(core::mem::size_of::<CpuSetT>(), 128);
    }

    #[test]
    fn test_cpu_set_and_isset() {
        let mut set = CpuSetT::new();
        assert!(!cpu_isset(0, &set));
        cpu_set(0, &mut set);
        assert!(cpu_isset(0, &set));
        assert!(!cpu_isset(1, &set));
    }

    #[test]
    fn test_cpu_clr() {
        let mut set = CpuSetT::new();
        cpu_set(5, &mut set);
        assert!(cpu_isset(5, &set));
        cpu_clr(5, &mut set);
        assert!(!cpu_isset(5, &set));
    }

    #[test]
    fn test_cpu_zero() {
        let mut set = CpuSetT::new();
        cpu_set(0, &mut set);
        cpu_set(63, &mut set);
        cpu_set(1023, &mut set);
        cpu_zero(&mut set);
        assert!(!cpu_isset(0, &set));
        assert!(!cpu_isset(63, &set));
        assert!(!cpu_isset(1023, &set));
    }

    #[test]
    fn test_cpu_count() {
        let mut set = CpuSetT::new();
        assert_eq!(cpu_count(&set), 0);
        cpu_set(0, &mut set);
        cpu_set(7, &mut set);
        cpu_set(100, &mut set);
        assert_eq!(cpu_count(&set), 3);
    }

    #[test]
    fn test_cpu_set_boundary() {
        // Test first and last CPU in each 64-bit word boundary.
        let mut set = CpuSetT::new();
        cpu_set(63, &mut set);
        assert!(cpu_isset(63, &set));
        cpu_set(64, &mut set);
        assert!(cpu_isset(64, &set));
        cpu_set(1023, &mut set);
        assert!(cpu_isset(1023, &set));
    }

    #[test]
    fn test_cpu_set_out_of_range() {
        // Out of range (≥ 1024) should be silently ignored.
        let mut set = CpuSetT::new();
        cpu_set(1024, &mut set);
        assert!(!cpu_isset(1024, &set));
        assert_eq!(cpu_count(&set), 0);
    }

    // -----------------------------------------------------------------------
    // pthread_setaffinity_np / pthread_getaffinity_np
    // -----------------------------------------------------------------------

    #[test]
    fn test_pthread_setaffinity_np_null_cpuset() {
        let ret = pthread_setaffinity_np(0, core::mem::size_of::<CpuSetT>(), core::ptr::null());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_pthread_setaffinity_np_small_size() {
        let set = CpuSetT::new();
        let ret = pthread_setaffinity_np(0, 1, &set);
        assert_eq!(ret, crate::errno::EINVAL);
    }

    #[test]
    fn test_pthread_setaffinity_np_success() {
        let mut set = CpuSetT::new();
        cpu_set(0, &mut set);
        let ret = pthread_setaffinity_np(0, core::mem::size_of::<CpuSetT>(), &set);
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_pthread_getaffinity_np_null_cpuset() {
        let ret = pthread_getaffinity_np(0, core::mem::size_of::<CpuSetT>(), core::ptr::null_mut());
        assert_eq!(ret, crate::errno::EFAULT);
    }

    #[test]
    fn test_pthread_getaffinity_np_small_size() {
        let mut set = CpuSetT::new();
        let ret = pthread_getaffinity_np(0, 1, &raw mut set);
        assert_eq!(ret, crate::errno::EINVAL);
    }

    #[test]
    fn test_pthread_getaffinity_np_returns_all_cpus() {
        let mut set = CpuSetT::new();
        let ret = pthread_getaffinity_np(0, core::mem::size_of::<CpuSetT>(), &raw mut set);
        assert_eq!(ret, 0);
        // All bits should be set.
        for word in &set.__bits {
            assert_eq!(*word, u64::MAX);
        }
    }

    // -----------------------------------------------------------------------
    // Phase 82 — pthread_setcancelstate / pthread_setcanceltype validation
    //
    // POSIX requires that invalid `state`/`type` arguments return EINVAL
    // without mutating the current cancellation state/type, and that
    // valid calls report the previous value via the out-pointer.
    //
    // These tests use an RAII guard that snapshots and restores the
    // global atomics around each case, so they remain deterministic
    // under `--test-threads=1` even when tests run in arbitrary order.
    // -----------------------------------------------------------------------

    struct CancelGuard {
        saved_state: i32,
        saved_type: i32,
    }
    impl CancelGuard {
        fn new() -> Self {
            Self {
                saved_state: current_cancel_state(),
                saved_type: current_cancel_type(),
            }
        }
    }
    impl Drop for CancelGuard {
        fn drop(&mut self) {
            CANCEL_STATE.store(self.saved_state, Ordering::Relaxed);
            CANCEL_TYPE.store(self.saved_type, Ordering::Relaxed);
        }
    }

    // ---- (a) Helper / constant invariants -------------------------------

    #[test]
    fn test_cancel_state_constants_distinct() {
        assert_ne!(PTHREAD_CANCEL_ENABLE, PTHREAD_CANCEL_DISABLE);
        assert_ne!(PTHREAD_CANCEL_DEFERRED, PTHREAD_CANCEL_ASYNCHRONOUS);
    }

    #[test]
    fn test_cancel_state_default_after_reset() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_DEFERRED);
    }

    // ---- (b) pthread_setcancelstate: EINVAL on bad input ----------------

    #[test]
    fn test_setcancelstate_rejects_negative() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let ret = pthread_setcancelstate(-1, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        // State must be unchanged.
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
    }

    #[test]
    fn test_setcancelstate_rejects_value_two() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let ret = pthread_setcancelstate(2, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
    }

    #[test]
    fn test_setcancelstate_rejects_large_value() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let ret = pthread_setcancelstate(i32::MAX, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_ENABLE);
    }

    #[test]
    fn test_setcancelstate_invalid_does_not_write_oldstate() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let mut old: i32 = 0x5A5A_5A5A;
        let ret = pthread_setcancelstate(42, &raw mut old);
        assert_eq!(ret, errno::EINVAL);
        // Sentinel must be untouched.
        assert_eq!(old, 0x5A5A_5A5A);
    }

    // ---- (c) pthread_setcancelstate: success cases ----------------------

    #[test]
    fn test_setcancelstate_enable_to_disable_reports_previous() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let mut old: i32 = -123;
        let ret = pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, &raw mut old);
        assert_eq!(ret, 0);
        assert_eq!(old, PTHREAD_CANCEL_ENABLE);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_DISABLE);
    }

    #[test]
    fn test_setcancelstate_save_restore_roundtrip() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
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
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let ret = pthread_setcancelstate(PTHREAD_CANCEL_DISABLE, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(current_cancel_state(), PTHREAD_CANCEL_DISABLE);
    }

    #[test]
    fn test_setcancelstate_idempotent() {
        // Setting the same value twice should be a no-op (and report
        // that value back).
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
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
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let ret = pthread_setcanceltype(-1, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_DEFERRED);
    }

    #[test]
    fn test_setcanceltype_rejects_value_two() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let ret = pthread_setcanceltype(2, core::ptr::null_mut());
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_DEFERRED);
    }

    #[test]
    fn test_setcanceltype_invalid_does_not_write_oldtype() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let mut old: i32 = 0x1234_5678;
        let ret = pthread_setcanceltype(99, &raw mut old);
        assert_eq!(ret, errno::EINVAL);
        assert_eq!(old, 0x1234_5678);
    }

    // ---- (e) pthread_setcanceltype: success cases -----------------------

    #[test]
    fn test_setcanceltype_deferred_to_async_reports_previous() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let mut old: i32 = 0xABCD;
        let ret = pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, &raw mut old);
        assert_eq!(ret, 0);
        assert_eq!(old, PTHREAD_CANCEL_DEFERRED);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_ASYNCHRONOUS);
    }

    #[test]
    fn test_setcanceltype_save_restore_roundtrip() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
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
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
        let ret = pthread_setcanceltype(PTHREAD_CANCEL_ASYNCHRONOUS, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(current_cancel_type(), PTHREAD_CANCEL_ASYNCHRONOUS);
    }

    // ---- (f) Non-interference between state and type --------------------

    #[test]
    fn test_setcancelstate_does_not_affect_type() {
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
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
        let _g = CancelGuard::new();
        reset_cancel_state_and_type();
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
}
