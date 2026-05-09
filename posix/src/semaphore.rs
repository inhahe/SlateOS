//! POSIX semaphore implementation.
//!
//! Implements unnamed semaphores using atomic operations.
//! Named semaphores (`sem_open`/`sem_unlink`) are stubs returning ENOSYS.
//!
//! ## Implementation
//!
//! Unnamed semaphores use a simple atomic counter with spin-yield
//! waiting.  This is sufficient for single-process multi-threaded
//! programs but doesn't support cross-process semaphores.
//!
//! Functions: `sem_init`, `sem_destroy`, `sem_wait`, `sem_trywait`,
//! `sem_timedwait`, `sem_post`, `sem_getvalue`, `sem_open` (stub),
//! `sem_close` (stub), `sem_unlink` (stub).

use crate::errno;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// POSIX semaphore.
///
/// We use a simple i32 counter.  Positive means the resource is
/// available; zero or negative means waiters are blocked.
#[repr(C)]
pub struct SemT {
    /// Semaphore value (atomic).
    value: core::sync::atomic::AtomicI32,
}

/// Failed return value for sem_open.
pub const SEM_FAILED: *mut SemT = core::ptr::null_mut();

// ---------------------------------------------------------------------------
// Unnamed semaphores
// ---------------------------------------------------------------------------

/// Initialize an unnamed semaphore.
///
/// `pshared` is ignored (cross-process semaphores not supported).
/// `value` is the initial semaphore count.
///
/// Returns 0 on success, -1 on error.
#[unsafe(no_mangle)]
pub extern "C" fn sem_init(sem: *mut SemT, _pshared: i32, value: u32) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: Caller guarantees sem is valid and writable.
    unsafe {
        core::ptr::addr_of_mut!((*sem).value)
            .write(core::sync::atomic::AtomicI32::new(value as i32));
    }

    0
}

/// Destroy an unnamed semaphore.
///
/// No-op in our implementation (no resources to free).
#[unsafe(no_mangle)]
pub extern "C" fn sem_destroy(_sem: *mut SemT) -> i32 {
    0
}

/// Lock (decrement) a semaphore, blocking if the value is zero.
///
/// Uses spin-yield waiting (no kernel futex support yet).
#[unsafe(no_mangle)]
pub extern "C" fn sem_wait(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };

    loop {
        let current = atomic.load(core::sync::atomic::Ordering::Acquire);
        if current > 0
            && atomic
                .compare_exchange_weak(
                    current,
                    current.wrapping_sub(1),
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
        {
            return 0;
        }
        // Yield to other threads.
        core::hint::spin_loop();
    }
}

/// Try to lock a semaphore without blocking.
///
/// Returns 0 if the semaphore was decremented, -1 with EAGAIN if
/// the semaphore is already zero.
#[unsafe(no_mangle)]
pub extern "C" fn sem_trywait(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };
    let current = atomic.load(core::sync::atomic::Ordering::Acquire);

    if current > 0
        && atomic
            .compare_exchange(
                current,
                current.wrapping_sub(1),
                core::sync::atomic::Ordering::AcqRel,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
    {
        return 0;
    }

    errno::set_errno(errno::EAGAIN);
    -1
}

/// Unlock (increment) a semaphore.
///
/// If threads are waiting, one will be woken.
#[unsafe(no_mangle)]
pub extern "C" fn sem_post(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };
    atomic.fetch_add(1, core::sync::atomic::Ordering::Release);
    0
}

/// Lock a semaphore with a timeout.
///
/// Like `sem_wait` but returns `ETIMEDOUT` if the absolute time
/// `abstime` passes before the semaphore can be decremented.
#[unsafe(no_mangle)]
pub extern "C" fn sem_timedwait(sem: *mut SemT, abstime: *const crate::stat::Timespec) -> i32 {
    if sem.is_null() || abstime.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };

    loop {
        // Try to decrement.
        let current = atomic.load(core::sync::atomic::Ordering::Acquire);
        if current > 0
            && atomic
                .compare_exchange_weak(
                    current,
                    current.wrapping_sub(1),
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
        {
            return 0;
        }

        // Check timeout.
        let mut now = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        let _ = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut now);
        let deadline = unsafe { &*abstime };
        if now.tv_sec > deadline.tv_sec
            || (now.tv_sec == deadline.tv_sec && now.tv_nsec >= deadline.tv_nsec)
        {
            errno::set_errno(errno::ETIMEDOUT);
            return -1;
        }

        // Yield briefly.
        core::hint::spin_loop();
        let _ = crate::syscall::syscall1(crate::syscall::SYS_SLEEP, 1_000_000);
    }
}

/// Get the current value of a semaphore.
#[unsafe(no_mangle)]
pub extern "C" fn sem_getvalue(sem: *mut SemT, sval: *mut i32) -> i32 {
    if sem.is_null() || sval.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };
    let val = atomic.load(core::sync::atomic::Ordering::Relaxed);
    unsafe { *sval = val; }
    0
}

// ---------------------------------------------------------------------------
// Named semaphores — stubs
// ---------------------------------------------------------------------------

/// Open a named semaphore.
///
/// Stub: returns `SEM_FAILED` (not supported).
#[unsafe(no_mangle)]
pub extern "C" fn sem_open(
    _name: *const u8,
    _oflag: i32,
) -> *mut SemT {
    errno::set_errno(errno::ENOSYS);
    SEM_FAILED
}

/// Close a named semaphore.
///
/// Stub: returns -1.
#[unsafe(no_mangle)]
pub extern "C" fn sem_close(_sem: *mut SemT) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Remove a named semaphore.
///
/// Stub: returns -1.
#[unsafe(no_mangle)]
pub extern "C" fn sem_unlink(_name: *const u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}
