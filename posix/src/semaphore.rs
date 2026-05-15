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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_init(sem: *mut SemT, _pshared: i32, value: u32) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Guard against u32 values that would wrap to negative when cast
    // to i32.  Our SEM_VALUE_MAX is i32::MAX.
    if value > i32::MAX as u32 {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_destroy(_sem: *mut SemT) -> i32 {
    0
}

/// Lock (decrement) a semaphore, blocking if the value is zero.
///
/// Uses spin-yield waiting (no kernel futex support yet).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_trywait(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };

    // Retry loop: a failed CAS doesn't mean value is zero — another
    // thread may have concurrently modified it.  Only give up when the
    // value is genuinely non-positive.
    loop {
        let current = atomic.load(core::sync::atomic::Ordering::Acquire);
        if current <= 0 {
            errno::set_errno(errno::EAGAIN);
            return -1;
        }
        if atomic
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
        // CAS failed — value changed. Retry with fresh load.
    }
}

/// Unlock (increment) a semaphore.
///
/// If threads are waiting, one will be woken.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_post(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };

    // POSIX: return EOVERFLOW if incrementing would exceed SEM_VALUE_MAX.
    // Without this check, wrapping past i32::MAX produces a negative value,
    // which sem_wait interprets as "no resources", causing deadlock.
    loop {
        let current = atomic.load(core::sync::atomic::Ordering::Relaxed);
        if current == i32::MAX {
            errno::set_errno(errno::EOVERFLOW);
            return -1;
        }
        if atomic
            .compare_exchange_weak(
                current,
                current.wrapping_add(1),
                core::sync::atomic::Ordering::Release,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            return 0;
        }
    }
}

/// Lock a semaphore with a timeout.
///
/// Like `sem_wait` but returns `ETIMEDOUT` if the absolute time
/// `abstime` passes before the semaphore can be decremented.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_close(_sem: *mut SemT) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Remove a named semaphore.
///
/// Stub: returns -1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_unlink(_name: *const u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- sem_init --

    #[test]
    fn test_sem_init_zero() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(-1),
        };
        let ret = sem_init(&raw mut sem, 0, 0);
        assert_eq!(ret, 0);
        let mut val: i32 = -1;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 0);
    }

    #[test]
    fn test_sem_init_positive() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        let ret = sem_init(&raw mut sem, 0, 5);
        assert_eq!(ret, 0);
        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 5);
    }

    #[test]
    fn test_sem_init_max_valid() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        let ret = sem_init(&raw mut sem, 0, i32::MAX as u32);
        assert_eq!(ret, 0);
        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, i32::MAX);
    }

    #[test]
    fn test_sem_init_overflow_rejected() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        // i32::MAX + 1 = 2147483648 — should be rejected
        let ret = sem_init(&raw mut sem, 0, (i32::MAX as u32).wrapping_add(1));
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sem_init_null() {
        let ret = sem_init(core::ptr::null_mut(), 0, 1);
        assert_eq!(ret, -1);
    }

    // -- sem_destroy --

    #[test]
    fn test_sem_destroy_succeeds() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(5),
        };
        assert_eq!(sem_destroy(&raw mut sem), 0);
    }

    // -- sem_post --

    #[test]
    fn test_sem_post_increments() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        let ret = sem_post(&raw mut sem);
        assert_eq!(ret, 0);

        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 1);
    }

    #[test]
    fn test_sem_post_multiple() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        for _ in 0..10 {
            sem_post(&raw mut sem);
        }

        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 10);
    }

    #[test]
    fn test_sem_post_overflow_rejected() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(i32::MAX),
        };
        let ret = sem_post(&raw mut sem);
        assert_eq!(ret, -1);
        // Value should not have changed
        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, i32::MAX);
    }

    #[test]
    fn test_sem_post_null() {
        let ret = sem_post(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -- sem_trywait --

    #[test]
    fn test_sem_trywait_decrements() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 3);

        let ret = sem_trywait(&raw mut sem);
        assert_eq!(ret, 0);

        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 2);
    }

    #[test]
    fn test_sem_trywait_fails_at_zero() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        let ret = sem_trywait(&raw mut sem);
        assert_eq!(ret, -1); // EAGAIN
    }

    #[test]
    fn test_sem_trywait_null() {
        let ret = sem_trywait(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sem_trywait_drain() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 3);

        assert_eq!(sem_trywait(&raw mut sem), 0);
        assert_eq!(sem_trywait(&raw mut sem), 0);
        assert_eq!(sem_trywait(&raw mut sem), 0);
        // Now zero — should fail
        assert_eq!(sem_trywait(&raw mut sem), -1);
    }

    // -- sem_getvalue --

    #[test]
    fn test_sem_getvalue_basic() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 42);

        let mut val: i32 = 0;
        let ret = sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(ret, 0);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_sem_getvalue_null_sem() {
        let mut val: i32 = 0;
        let ret = sem_getvalue(core::ptr::null_mut(), &raw mut val);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sem_getvalue_null_sval() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(5),
        };
        let ret = sem_getvalue(&raw mut sem, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -- post/trywait round trip --

    #[test]
    fn test_sem_post_trywait_round_trip() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        // Can't trywait on empty
        assert_eq!(sem_trywait(&raw mut sem), -1);

        // Post once
        assert_eq!(sem_post(&raw mut sem), 0);

        // Now can trywait
        assert_eq!(sem_trywait(&raw mut sem), 0);

        // Empty again
        assert_eq!(sem_trywait(&raw mut sem), -1);
    }

    // -- Named semaphore stubs --

    #[test]
    fn test_sem_open_returns_failed() {
        let ret = sem_open(b"/mysem\0".as_ptr(), 0);
        assert_eq!(ret, SEM_FAILED);
        assert!(SEM_FAILED.is_null());
    }

    #[test]
    fn test_sem_close_returns_error() {
        assert_eq!(sem_close(core::ptr::null_mut()), -1);
    }

    #[test]
    fn test_sem_unlink_returns_error() {
        assert_eq!(sem_unlink(b"/mysem\0".as_ptr()), -1);
    }

    // -- SemT layout --

    #[test]
    fn test_sem_size() {
        // AtomicI32 = 4 bytes
        assert_eq!(core::mem::size_of::<SemT>(), 4);
    }

    #[test]
    fn test_sem_alignment() {
        assert_eq!(core::mem::align_of::<SemT>(), 4);
    }

    // -- sem_init sets errno for null --

    #[test]
    fn test_sem_init_null_sets_einval() {
        crate::errno::set_errno(0);
        let ret = sem_init(core::ptr::null_mut(), 0, 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sem_init_overflow_sets_einval() {
        crate::errno::set_errno(0);
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        let ret = sem_init(&raw mut sem, 0, u32::MAX);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- sem_wait null sets errno --

    #[test]
    fn test_sem_wait_null_einval() {
        crate::errno::set_errno(0);
        let ret = sem_wait(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- sem_trywait sets EAGAIN --

    #[test]
    fn test_sem_trywait_zero_sets_eagain() {
        crate::errno::set_errno(0);
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);
        let ret = sem_trywait(&raw mut sem);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
    }

    #[test]
    fn test_sem_trywait_null_sets_einval() {
        crate::errno::set_errno(0);
        let ret = sem_trywait(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- sem_post overflow sets EOVERFLOW --

    #[test]
    fn test_sem_post_overflow_sets_eoverflow() {
        crate::errno::set_errno(0);
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(i32::MAX),
        };
        let ret = sem_post(&raw mut sem);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOVERFLOW);
    }

    #[test]
    fn test_sem_post_null_sets_einval() {
        crate::errno::set_errno(0);
        let ret = sem_post(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Named semaphore stubs set ENOSYS --

    #[test]
    fn test_sem_open_sets_enosys() {
        crate::errno::set_errno(0);
        let _ = sem_open(b"/test\0".as_ptr(), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_sem_close_sets_enosys() {
        crate::errno::set_errno(0);
        let _ = sem_close(core::ptr::null_mut());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_sem_unlink_sets_enosys() {
        crate::errno::set_errno(0);
        let _ = sem_unlink(b"/test\0".as_ptr());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- sem_timedwait null checks --

    #[test]
    fn test_sem_timedwait_null_sem() {
        crate::errno::set_errno(0);
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        let ret = sem_timedwait(core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    #[test]
    fn test_sem_timedwait_null_abstime() {
        crate::errno::set_errno(0);
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);
        let ret = sem_timedwait(&raw mut sem, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- sem_init with pshared (ignored but accepted) --

    #[test]
    fn test_sem_init_pshared_nonzero() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        // pshared=1 should still succeed (we ignore it)
        let ret = sem_init(&raw mut sem, 1, 10);
        assert_eq!(ret, 0);
        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 10);
    }

    // -- SEM_FAILED constant --

    #[test]
    fn test_sem_failed_is_null() {
        assert!(SEM_FAILED.is_null());
    }

    // -- Multiple post/trywait cycles --

    #[test]
    fn test_sem_multiple_cycles() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        for _ in 0..5 {
            assert_eq!(sem_post(&raw mut sem), 0);
            assert_eq!(sem_post(&raw mut sem), 0);
            assert_eq!(sem_trywait(&raw mut sem), 0);
            assert_eq!(sem_trywait(&raw mut sem), 0);
            assert_eq!(sem_trywait(&raw mut sem), -1); // empty
        }
    }

    // -- sem_getvalue after operations --

    #[test]
    fn test_sem_getvalue_tracks_operations() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 5);
        let mut val: i32 = 0;

        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 5);

        sem_trywait(&raw mut sem);
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 4);

        sem_post(&raw mut sem);
        sem_post(&raw mut sem);
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 6);
    }
}
