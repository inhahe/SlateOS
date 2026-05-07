//! POSIX threads stubs.
//!
//! Our kernel uses its own threading model.  These stubs let C
//! programs that reference pthread symbols link successfully.
//! All functions that would create threads return ENOSYS.
//!
//! Mutex and once operations are implemented for single-threaded use.

use crate::errno;

/// Opaque pthread_t type (thread identifier).
pub type PthreadT = u64;

/// Opaque pthread_attr_t type.
pub type PthreadAttrT = [u8; 64];

/// Pthread mutex type (simple, single-threaded implementation).
#[repr(C)]
pub struct PthreadMutexT {
    locked: i32,
    // Padding to match typical libc layout.
    _pad: [u8; 36],
}

/// Pthread mutex attribute type.
pub type PthreadMutexattrT = [u8; 8];

/// Pthread once control type.
#[repr(C)]
pub struct PthreadOnceT {
    done: i32,
}

/// Static initializer for pthread_once_t.
pub const PTHREAD_ONCE_INIT: PthreadOnceT = PthreadOnceT { done: 0 };

/// Static initializer for pthread_mutex_t (unlocked).
pub const PTHREAD_MUTEX_INITIALIZER: PthreadMutexT = PthreadMutexT {
    locked: 0,
    _pad: [0; 36],
};

// ---------------------------------------------------------------------------
// Thread creation/management (stubs)
// ---------------------------------------------------------------------------

/// Create a new thread.
///
/// Stub: returns ENOSYS (threading not yet supported).
#[unsafe(no_mangle)]
pub extern "C" fn pthread_create(
    _thread: *mut PthreadT,
    _attr: *const PthreadAttrT,
    _start: extern "C" fn(*mut u8) -> *mut u8,
    _arg: *mut u8,
) -> i32 {
    errno::ENOSYS
}

/// Wait for a thread to terminate.
///
/// Stub: returns ESRCH (no threads exist).
#[unsafe(no_mangle)]
pub extern "C" fn pthread_join(_thread: PthreadT, _retval: *mut *mut u8) -> i32 {
    errno::ESRCH
}

/// Detach a thread.
///
/// Stub: returns ESRCH.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_detach(_thread: PthreadT) -> i32 {
    errno::ESRCH
}

/// Get the calling thread's ID.
///
/// Returns 1 (the "main thread").
#[unsafe(no_mangle)]
pub extern "C" fn pthread_self() -> PthreadT {
    1
}

/// Compare two thread IDs.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_equal(t1: PthreadT, t2: PthreadT) -> i32 {
    i32::from(t1 == t2)
}

/// Terminate the calling thread.
///
/// Stub: calls _exit(0) since there's only one thread.
#[unsafe(no_mangle)]
pub extern "C" fn pthread_exit(_retval: *mut u8) -> ! {
    #[allow(clippy::used_underscore_items)]
    crate::process::_exit(0);
}

// ---------------------------------------------------------------------------
// Mutex operations (single-threaded implementations)
// ---------------------------------------------------------------------------

/// Initialize a mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_init(
    mutex: *mut PthreadMutexT,
    _attr: *const PthreadMutexattrT,
) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    unsafe { (*mutex).locked = 0; }
    0
}

/// Lock a mutex.
///
/// In single-threaded mode, this just marks the mutex as locked.
/// If already locked, returns EDEADLK (would deadlock).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_lock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    if unsafe { (*mutex).locked } != 0 {
        return errno::EDEADLK;
    }
    unsafe { (*mutex).locked = 1; }
    0
}

/// Try to lock a mutex without blocking.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_trylock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    if unsafe { (*mutex).locked } != 0 {
        return errno::EBUSY;
    }
    unsafe { (*mutex).locked = 1; }
    0
}

/// Unlock a mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_unlock(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    unsafe { (*mutex).locked = 0; }
    0
}

/// Destroy a mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_mutex_destroy(mutex: *mut PthreadMutexT) -> i32 {
    if mutex.is_null() {
        return errno::EINVAL;
    }
    unsafe { (*mutex).locked = 0; }
    0
}

// ---------------------------------------------------------------------------
// Once control
// ---------------------------------------------------------------------------

/// Execute a function exactly once.
///
/// Single-threaded implementation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_once(
    once: *mut PthreadOnceT,
    init: extern "C" fn(),
) -> i32 {
    if once.is_null() {
        return errno::EINVAL;
    }
    if unsafe { (*once).done } == 0 {
        init();
        unsafe { (*once).done = 1; }
    }
    0
}

// ---------------------------------------------------------------------------
// Thread-specific data (minimal stubs)
// ---------------------------------------------------------------------------

/// Key type for thread-specific data.
pub type PthreadKeyT = u32;

/// Maximum number of thread-specific data keys.
const MAX_KEYS: usize = 64;

/// Thread-specific data values (single thread, so one set of values).
static mut TSD_VALUES: [*mut u8; MAX_KEYS] = [core::ptr::null_mut(); MAX_KEYS];
/// Next key to allocate.
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
    0 // No-op: we don't reclaim keys.
}
