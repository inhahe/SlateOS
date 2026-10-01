//! C11's `<threads.h>`: threads, mutexes, condition variables, thread-specific
//! storage and `call_once`, over this library's pthreads.
//!
//! musl's header declares all twenty-five functions and, until 2026-09-28,
//! none existed here, so no C program written to C11 threads linked
//! (known-issues.md -> `D-POSIX-LIBC-LACKS-FUNCTIONS-ITS-HEADERS-DECLARE`).
//! Each is the pthread function it names, with two translations:
//!
//! - **Types.** `mtx_t` and `cnd_t` are the same 40 and 48 bytes as
//!   `pthread_mutex_t` and `pthread_cond_t` in musl's headers, and are those
//!   types here (`abi_layout.rs` checks both sizes); `tss_t` is a
//!   `pthread_key_t`, `once_flag` a `pthread_once_t`, `thrd_t` a
//!   `pthread_t`.
//! - **Results.** C11 answers with `thrd_success`, `thrd_busy`,
//!   `thrd_error`, `thrd_nomem` or `thrd_timedout` where pthreads answer
//!   with an error number: `EBUSY`, `ENOMEM` and `ETIMEDOUT` become the three
//!   named ones and every other error `thrd_error`, as glibc's
//!   `__thrd_err_map` makes them.
//!
//! A C11 thread's start function returns `int` where a pthread's returns
//! `void *`, so [`thrd_create`] starts a small trampoline, not the caller's
//! function cast to the other type -- calling through a pointer of the wrong
//! type is undefined, however the registers happen to line up.

use crate::errno;
use crate::pthread::{PthreadCondT, PthreadKeyT, PthreadMutexT, PthreadOnceT, PthreadT};
use crate::stat::Timespec;

/// C11's `thrd_success`: the call did what was asked.
pub const THRD_SUCCESS: i32 = 0;
/// C11's `thrd_busy`: the mutex is held by another thread.
pub const THRD_BUSY: i32 = 1;
/// C11's `thrd_error`: anything else went wrong.
pub const THRD_ERROR: i32 = 2;
/// C11's `thrd_nomem`: memory ran out.
pub const THRD_NOMEM: i32 = 3;
/// C11's `thrd_timedout`: the deadline passed.
pub const THRD_TIMEDOUT: i32 = 4;

/// C11's `mtx_plain`: a mutex that neither times out nor recurses.
pub const MTX_PLAIN: i32 = 0;
/// C11's `mtx_recursive`: the owner may lock it again.
pub const MTX_RECURSIVE: i32 = 1;
/// C11's `mtx_timed`: `mtx_timedlock` may be used on it.
pub const MTX_TIMED: i32 = 2;

/// A C11 thread's start function.
pub type ThrdStartT = Option<unsafe extern "C" fn(*mut u8) -> i32>;
/// A thread-specific-storage destructor.
pub type TssDtorT = Option<extern "C" fn(*mut u8)>;

/// A pthread error number as a C11 result: glibc's `__thrd_err_map`.
const fn thrd_result(err: i32) -> i32 {
    match err {
        0 => THRD_SUCCESS,
        errno::ENOMEM => THRD_NOMEM,
        errno::ETIMEDOUT => THRD_TIMEDOUT,
        errno::EBUSY => THRD_BUSY,
        _ => THRD_ERROR,
    }
}

// ---------------------------------------------------------------------------
// Threads
// ---------------------------------------------------------------------------

/// What [`thrd_create`] hands its trampoline: the caller's function and its
/// argument, in a `malloc`ed block the trampoline frees.
#[repr(C)]
struct Start {
    func: unsafe extern "C" fn(*mut u8) -> i32,
    arg: *mut u8,
}

/// The pthread start routine of every C11 thread: run the caller's function
/// and return its `int` as the thread's exit value, which [`thrd_join`]
/// narrows back.
extern "C" fn trampoline(start: *mut u8) -> *mut u8 {
    // SAFETY: `start` is the block `thrd_create` allocated and filled, and
    // handed to this thread alone; it is read, then freed, before the call.
    let Start { func, arg } = unsafe { start.cast::<Start>().read() };
    // SAFETY: `start` came from `malloc` and is not used again.
    unsafe { crate::malloc::free(start) };
    // SAFETY: the caller's function, called with the caller's argument, as
    // `thrd_create`'s contract with it says.
    let res = unsafe { func(arg) };
    // Sign-extended, so that `thrd_join`'s narrowing returns it exactly.
    res as isize as *mut u8
}

/// Start a thread running `func(arg)`; its id in `*thr`.
///
/// # Safety
///
/// `thr` is a valid `thrd_t *`; `func` is a function that may be called with
/// `arg` on another thread.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn thrd_create(thr: *mut PthreadT, func: ThrdStartT, arg: *mut u8) -> i32 {
    let Some(func) = func else {
        return THRD_ERROR;
    };
    let block = crate::malloc::malloc(size_of::<Start>()).cast::<Start>();
    if block.is_null() {
        return THRD_NOMEM;
    }
    // SAFETY: a fresh block of the right size (malloc aligns for any type).
    unsafe { block.write(Start { func, arg }) };
    let err =
        crate::pthread::pthread_create(thr, core::ptr::null(), Some(trampoline), block.cast());
    if err != 0 {
        // No thread ran, so the block is still ours.
        // SAFETY: from `malloc` above, not handed to anyone.
        unsafe { crate::malloc::free(block.cast()) };
    }
    thrd_result(err)
}

/// End the calling thread with `res` as its result.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn thrd_exit(res: i32) -> ! {
    crate::pthread::pthread_exit(res as isize as *mut u8)
}

/// Wait for `thr` to end; its result in `*res` unless `res` is NULL.
///
/// # Safety
///
/// `res` is NULL or a valid `int *`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn thrd_join(thr: PthreadT, res: *mut i32) -> i32 {
    let mut value: *mut u8 = core::ptr::null_mut();
    if crate::pthread::pthread_join(thr, &raw mut value) != 0 {
        return THRD_ERROR;
    }
    if !res.is_null() {
        // The trampoline widened an `int`; the low 32 bits are it.
        #[allow(clippy::cast_possible_truncation)]
        let r = value as isize as i32;
        // SAFETY: non-null, the caller's.
        unsafe { res.write(r) };
    }
    THRD_SUCCESS
}

/// Let `thr` free itself when it ends.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn thrd_detach(thr: PthreadT) -> i32 {
    thrd_result(crate::pthread::pthread_detach(thr))
}

/// The calling thread.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn thrd_current() -> PthreadT {
    crate::pthread::pthread_self()
}

/// Nonzero if `a` and `b` are the same thread. (C's header makes it a macro;
/// C++ calls this.)
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn thrd_equal(a: PthreadT, b: PthreadT) -> i32 {
    crate::pthread::pthread_equal(a, b)
}

/// Sleep for `*duration`: 0 when it has passed, -1 if a signal cut it short
/// (the rest in `*remaining`), -2 on any other failure -- glibc's three.
///
/// # Safety
///
/// `duration` is a valid `struct timespec *`; `remaining` NULL or one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn thrd_sleep(duration: *const Timespec, remaining: *mut Timespec) -> i32 {
    if crate::time::nanosleep(duration, remaining) == 0 {
        0
    } else if errno::get_errno() == errno::EINTR {
        -1
    } else {
        -2
    }
}

/// Give up the processor.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn thrd_yield() {
    // Nothing to report: `sched_yield` cannot fail.
    let _ = crate::pthread::sched_yield();
}

// ---------------------------------------------------------------------------
// Mutexes
// ---------------------------------------------------------------------------

/// Make `*mtx` a mutex of `kind`: `mtx_plain` or `mtx_timed`, either possibly
/// `| mtx_recursive`; any other kind is `thrd_error`, as glibc checks it.
/// (Every mutex here can be waited on with a deadline, so `mtx_timed` needs
/// nothing of its own.)
///
/// # Safety
///
/// `mtx` is a valid `mtx_t *`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mtx_init(mtx: *mut PthreadMutexT, kind: i32) -> i32 {
    let recursive = match kind {
        k if k == MTX_PLAIN || k == MTX_TIMED => false,
        k if k == MTX_PLAIN | MTX_RECURSIVE || k == MTX_TIMED | MTX_RECURSIVE => true,
        _ => return THRD_ERROR,
    };
    let mut attr: crate::pthread::PthreadMutexattrT = [0; 4];
    if crate::pthread::pthread_mutexattr_init(&raw mut attr) != 0 {
        return THRD_ERROR;
    }
    let pkind = if recursive {
        crate::pthread::PTHREAD_MUTEX_RECURSIVE
    } else {
        crate::pthread::PTHREAD_MUTEX_NORMAL
    };
    if crate::pthread::pthread_mutexattr_settype(&raw mut attr, pkind) != 0 {
        return THRD_ERROR;
    }
    // SAFETY: the caller's mutex; a local, initialised attribute.
    thrd_result(unsafe { crate::pthread::pthread_mutex_init(mtx, &raw const attr) })
}

/// Lock `*mtx`, waiting as long as it takes.
///
/// # Safety
///
/// `mtx` is a mutex `mtx_init` made.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mtx_lock(mtx: *mut PthreadMutexT) -> i32 {
    // SAFETY: this function's contract.
    thrd_result(unsafe { crate::pthread::pthread_mutex_lock(mtx) })
}

/// Lock `*mtx`, waiting until `*deadline` (`CLOCK_REALTIME`) at most.
///
/// # Safety
///
/// As for [`mtx_lock`]; `deadline` is a valid `struct timespec *`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mtx_timedlock(mtx: *mut PthreadMutexT, deadline: *const Timespec) -> i32 {
    thrd_result(crate::pthread::pthread_mutex_timedlock(mtx, deadline))
}

/// Lock `*mtx` if no one holds it: `thrd_busy` if someone does.
///
/// # Safety
///
/// As for [`mtx_lock`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mtx_trylock(mtx: *mut PthreadMutexT) -> i32 {
    // SAFETY: this function's contract.
    thrd_result(unsafe { crate::pthread::pthread_mutex_trylock(mtx) })
}

/// Unlock `*mtx`.
///
/// # Safety
///
/// As for [`mtx_lock`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mtx_unlock(mtx: *mut PthreadMutexT) -> i32 {
    // SAFETY: this function's contract.
    thrd_result(unsafe { crate::pthread::pthread_mutex_unlock(mtx) })
}

/// Release whatever `*mtx` holds; it is not used again.
///
/// # Safety
///
/// As for [`mtx_lock`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mtx_destroy(mtx: *mut PthreadMutexT) {
    // C11 gives `mtx_destroy` no result to report a failure in.
    // SAFETY: this function's contract.
    let _ = unsafe { crate::pthread::pthread_mutex_destroy(mtx) };
}

// ---------------------------------------------------------------------------
// Condition variables
// ---------------------------------------------------------------------------

/// Make `*cnd` a condition variable.
///
/// # Safety
///
/// `cnd` is a valid `cnd_t *`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cnd_init(cnd: *mut PthreadCondT) -> i32 {
    thrd_result(crate::pthread::pthread_cond_init(cnd, core::ptr::null()))
}

/// Wake one waiter.
///
/// # Safety
///
/// `cnd` is a condition variable `cnd_init` made.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cnd_signal(cnd: *mut PthreadCondT) -> i32 {
    thrd_result(crate::pthread::pthread_cond_signal(cnd))
}

/// Wake every waiter.
///
/// # Safety
///
/// As for [`cnd_signal`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cnd_broadcast(cnd: *mut PthreadCondT) -> i32 {
    thrd_result(crate::pthread::pthread_cond_broadcast(cnd))
}

/// Release `*mtx`, wait to be woken, and lock it again.
///
/// # Safety
///
/// As for [`cnd_signal`]; `mtx` is held by the caller.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cnd_wait(cnd: *mut PthreadCondT, mtx: *mut PthreadMutexT) -> i32 {
    thrd_result(crate::pthread::pthread_cond_wait(cnd, mtx))
}

/// [`cnd_wait`] until `*deadline` (`CLOCK_REALTIME`) at most:
/// `thrd_timedout` if it passes first.
///
/// # Safety
///
/// As for [`cnd_wait`]; `deadline` is a valid `struct timespec *`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cnd_timedwait(
    cnd: *mut PthreadCondT,
    mtx: *mut PthreadMutexT,
    deadline: *const Timespec,
) -> i32 {
    thrd_result(crate::pthread::pthread_cond_timedwait(cnd, mtx, deadline))
}

/// Release whatever `*cnd` holds.
///
/// # Safety
///
/// As for [`cnd_signal`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn cnd_destroy(cnd: *mut PthreadCondT) {
    // C11 gives `cnd_destroy` no result to report a failure in.
    let _ = crate::pthread::pthread_cond_destroy(cnd);
}

// ---------------------------------------------------------------------------
// Thread-specific storage, and call_once
// ---------------------------------------------------------------------------

/// A new key, into `*key`, whose values `dtor` is called on as threads end.
///
/// # Safety
///
/// `key` is a valid `tss_t *`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn tss_create(key: *mut PthreadKeyT, dtor: TssDtorT) -> i32 {
    // SAFETY: this function's contract; the destructor type is the same.
    thrd_result(unsafe { crate::pthread::pthread_key_create(key, dtor) })
}

/// Retire `key`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tss_delete(key: PthreadKeyT) {
    // C11 gives `tss_delete` no result to report a failure in.
    let _ = crate::pthread::pthread_key_delete(key);
}

/// The calling thread's value for `key`, NULL if none.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tss_get(key: PthreadKeyT) -> *mut u8 {
    // SAFETY: a key is only an index; an unknown one reads as NULL.
    unsafe { crate::pthread::pthread_getspecific(key) }
}

/// Set the calling thread's value for `key`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tss_set(key: PthreadKeyT, value: *mut u8) -> i32 {
    // SAFETY: as in `tss_get`.
    thrd_result(unsafe { crate::pthread::pthread_setspecific(key, value) })
}

/// Call `func` once in the life of the process, however many threads come
/// here with the same `flag`.
///
/// # Safety
///
/// `flag` is a valid `once_flag *`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn call_once(flag: *mut PthreadOnceT, func: Option<extern "C" fn()>) {
    // C11 gives `call_once` no result to report a failure in.
    // SAFETY: this function's contract.
    let _ = unsafe { crate::pthread::pthread_once(flag, func) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pthread_errors_map_as_glibc_maps_them() {
        assert_eq!(thrd_result(0), THRD_SUCCESS);
        assert_eq!(thrd_result(errno::EBUSY), THRD_BUSY);
        assert_eq!(thrd_result(errno::ENOMEM), THRD_NOMEM);
        assert_eq!(thrd_result(errno::ETIMEDOUT), THRD_TIMEDOUT);
        assert_eq!(thrd_result(errno::EINVAL), THRD_ERROR);
        assert_eq!(thrd_result(errno::EAGAIN), THRD_ERROR);
    }

    fn mutex() -> PthreadMutexT {
        // SAFETY: an all-zero mutex is a valid bit pattern; `mtx_init`
        // initialises it properly below.
        unsafe { core::mem::zeroed() }
    }

    #[test]
    fn mtx_init_takes_the_four_kinds_c11_defines() {
        for kind in [
            MTX_PLAIN,
            MTX_TIMED,
            MTX_PLAIN | MTX_RECURSIVE,
            MTX_TIMED | MTX_RECURSIVE,
        ] {
            let mut m = mutex();
            // SAFETY: a local mutex.
            unsafe {
                assert_eq!(mtx_init(&raw mut m, kind), THRD_SUCCESS, "kind {kind}");
                mtx_destroy(&raw mut m);
            }
        }
        let mut m = mutex();
        // SAFETY: a local mutex.
        assert_eq!(unsafe { mtx_init(&raw mut m, 4) }, THRD_ERROR);
    }

    #[test]
    fn a_plain_mutex_is_busy_while_held_and_a_recursive_one_is_not() {
        let (mut plain, mut rec) = (mutex(), mutex());
        // SAFETY: local mutexes, locked and unlocked by this thread.
        unsafe {
            assert_eq!(mtx_init(&raw mut plain, MTX_PLAIN), THRD_SUCCESS);
            assert_eq!(mtx_lock(&raw mut plain), THRD_SUCCESS);
            assert_eq!(mtx_trylock(&raw mut plain), THRD_BUSY);
            assert_eq!(mtx_unlock(&raw mut plain), THRD_SUCCESS);
            assert_eq!(mtx_trylock(&raw mut plain), THRD_SUCCESS);
            assert_eq!(mtx_unlock(&raw mut plain), THRD_SUCCESS);
            mtx_destroy(&raw mut plain);

            assert_eq!(
                mtx_init(&raw mut rec, MTX_PLAIN | MTX_RECURSIVE),
                THRD_SUCCESS
            );
            assert_eq!(mtx_lock(&raw mut rec), THRD_SUCCESS);
            assert_eq!(mtx_trylock(&raw mut rec), THRD_SUCCESS);
            assert_eq!(mtx_unlock(&raw mut rec), THRD_SUCCESS);
            assert_eq!(mtx_unlock(&raw mut rec), THRD_SUCCESS);
            mtx_destroy(&raw mut rec);
        }
    }

    #[test]
    fn tss_values_are_per_key() {
        let mut k: PthreadKeyT = 0;
        // SAFETY: a local key.
        assert_eq!(unsafe { tss_create(&raw mut k, None) }, THRD_SUCCESS);
        assert!(tss_get(k).is_null());
        let mut v = 7u8;
        assert_eq!(tss_set(k, &raw mut v), THRD_SUCCESS);
        assert_eq!(tss_get(k), &raw mut v);
        tss_delete(k);
    }

    #[test]
    fn thrd_current_equals_itself() {
        assert_ne!(thrd_equal(thrd_current(), thrd_current()), 0);
    }

    /// A duration `nanosleep` refuses is "any other failure": -2, not the
    /// -1 that means a signal cut the sleep short. (A real sleep cannot be
    /// tested on the host, where the sleep system call is a stub.)
    #[test]
    fn thrd_sleep_reports_a_bad_duration_as_minus_two() {
        let d = Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        };
        // SAFETY: a local timespec.
        assert_eq!(
            unsafe { thrd_sleep(&raw const d, core::ptr::null_mut()) },
            -2
        );
    }

    #[test]
    fn thrd_create_with_no_function_is_an_error() {
        let mut t: PthreadT = 0;
        // SAFETY: a local id; no function, so nothing runs.
        assert_eq!(
            unsafe { thrd_create(&raw mut t, None, core::ptr::null_mut()) },
            THRD_ERROR
        );
    }
}
