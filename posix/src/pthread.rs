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
//! ## Mutexes
//!
//! Mutexes use atomic compare-and-swap for thread safety.  Under
//! contention the lock spins briefly then yields via `SYS_SLEEP`.
//! A futex-based implementation would be more efficient but requires
//! wiring up the kernel's futex syscall to userspace.
//!
//! ## Limitations
//!
//! - Thread-specific data (TSD) uses a **global** array, not per-thread
//!   storage.  Proper TLS requires kernel support for the FS/GS segment.
//! - Detached thread stacks are leaked (no cleanup notification).
//! - `pthread_cancel` is not implemented.
//! - Mutex is a spinlock (no futex-based blocking).
//! - Thread attributes (`pthread_attr_t`) are ignored; stack size is
//!   fixed at 64 KiB.

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
#[repr(C)]
pub struct PthreadMutexT {
    locked: AtomicI32,
    // Padding to match typical libc struct size.
    _pad: [u8; 36],
}

/// Pthread mutex attribute type.
pub type PthreadMutexattrT = [u8; 8];

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
    _pad: [0; 36],
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_init(
    mutex: *mut PthreadMutexT,
    _attr: *const PthreadMutexattrT,
) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: caller guarantees mutex is valid.
    unsafe { (*mutex).locked.store(0, Ordering::Release); }
    0
}

/// Lock a mutex.
///
/// Uses atomic CAS for thread safety.  On contention, spins briefly
/// then yields via `SYS_SLEEP(1ms)` to avoid wasting CPU time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_lock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }

    // SAFETY: caller guarantees mutex is valid.
    let locked = unsafe { &(*mutex).locked };

    // Fast path: uncontended acquisition.
    if locked
        .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        return 0;
    }

    // Slow path: spin briefly, then yield.
    loop {
        for _ in 0..MUTEX_SPIN_LIMIT {
            if locked
                .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
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
/// Returns 0 on success, `EBUSY` if the mutex is already locked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_trylock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: caller guarantees mutex is valid.
    let locked = unsafe { &(*mutex).locked };
    if locked
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        0
    } else {
        errno::EBUSY
    }
}

/// Unlock a mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_unlock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    // SAFETY: caller guarantees mutex is valid.
    unsafe { (*mutex).locked.store(0, Ordering::Release); }
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
