//! Completion notification as a `struct sigevent` asks -- the part of
//! glibc's `__aio_notify_only` (rt/aio_notify.c) that asynchronous I/O and
//! message queues share.
//!
//! - `SIGEV_THREAD` runs the caller's function with its value on a new
//!   thread, detached unless the caller's own attributes say otherwise.
//! - `SIGEV_SIGNAL` raises the signal.  glibc queues it with its value
//!   (`rt_sigqueueinfo`); `sigqueue` cannot deliver a value here yet, and
//!   plain `raise` is glibc's own choice on a system without queued signals.
//! - `SIGEV_NONE`, `SIGEV_THREAD_ID` and anything unknown notify nothing, as
//!   in glibc.
//!
//! This crate's `time::Sigevent` does not name the `SIGEV_THREAD` fields, so
//! [`SigeventView`] reads a `struct sigevent` in musl's layout directly.

use crate::errno;

/// The fields of a `struct sigevent` (musl's x86_64 layout, 64 bytes) that
/// notification reads.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SigeventView {
    /// `union sigval`: an `int` or a pointer, passed to the `SIGEV_THREAD`
    /// function in one register.
    pub(crate) sigev_value: usize,
    pub(crate) sigev_signo: i32,
    pub(crate) sigev_notify: i32,
    /// `SIGEV_THREAD`'s function (musl's `__sev_fields.__sev_thread`).
    pub(crate) sigev_notify_function: Option<extern "C" fn(usize)>,
    /// And its thread's attributes; NULL for a detached default thread.
    pub(crate) sigev_notify_attributes: *const crate::pthread::PthreadAttrT,
    pub(crate) __pad: [u8; 32],
}

const _: () = assert!(size_of::<SigeventView>() == 64);

impl SigeventView {
    /// `SIGEV_NONE`: what a NULL `struct sigevent` stands for.
    pub(crate) const NONE: Self = Self {
        sigev_value: 0,
        sigev_signo: 0,
        sigev_notify: crate::time::SIGEV_NONE,
        sigev_notify_function: None,
        sigev_notify_attributes: core::ptr::null(),
        __pad: [0; 32],
    };

    /// Read a `struct sigevent` at `p`.
    pub(crate) fn read(p: *const u8) -> Self {
        // SAFETY: callers pass 64 readable bytes -- an `aiocb`'s
        // `aio_sigevent` or a caller's `struct sigevent`; every bit pattern
        // is a valid
        // `SigeventView` (the function pointer is an `Option`, and a non-null
        // fn pointer is only invalid to *call*).
        unsafe { core::ptr::read_unaligned(p.cast::<Self>()) }
    }
}

/// What a notification thread calls.  glibc copies it to the heap because
/// the `sigevent` may be gone before the thread runs.
struct NotifyCall {
    function: extern "C" fn(usize),
    value: usize,
}

extern "C" fn notify_thread(arg: *mut u8) -> *mut u8 {
    // SAFETY: `arg` is the `NotifyCall` `notify` allocated for this thread
    // alone.
    let call = unsafe { core::ptr::read(arg.cast::<NotifyCall>()) };
    // SAFETY: allocated by `malloc` in `notify`, and read above.
    unsafe { crate::malloc::free(arg) };
    (call.function)(call.value);
    core::ptr::null_mut()
}

/// Notify as `sev` asks (glibc's `__aio_notify_only`); `Err` with `errno`
/// set if it could not be done.  `SIGEV_NONE`, `SIGEV_THREAD_ID` and
/// anything unknown notify nothing, as in glibc.
pub(crate) fn notify(sev: &SigeventView) -> Result<(), ()> {
    match sev.sigev_notify {
        crate::time::SIGEV_SIGNAL => {
            // glibc sends it with `rt_sigqueueinfo`, for which signal 0 only
            // probes.
            if sev.sigev_signo == 0 || crate::signal::raise(sev.sigev_signo) == 0 {
                Ok(())
            } else {
                Err(())
            }
        }
        crate::time::SIGEV_THREAD => {
            let Some(function) = sev.sigev_notify_function else {
                // glibc would start a thread that calls NULL, and fault.
                errno::set_errno(errno::EFAULT);
                return Err(());
            };
            let call = crate::malloc::malloc(size_of::<NotifyCall>());
            if call.is_null() {
                errno::set_errno(errno::ENOMEM);
                return Err(());
            }
            // SAFETY: `malloc` returned a block big and aligned enough.
            unsafe {
                core::ptr::write(
                    call.cast::<NotifyCall>(),
                    NotifyCall {
                        function,
                        value: sev.sigev_value,
                    },
                );
            }
            let mut detached: crate::pthread::PthreadAttrT = [0; 56];
            let attr = if sev.sigev_notify_attributes.is_null() {
                // Both succeed on a valid attribute object and a valid state.
                let _ = crate::pthread::pthread_attr_init(&raw mut detached);
                let _ = crate::pthread::pthread_attr_setdetachstate(
                    &raw mut detached,
                    crate::pthread::PTHREAD_CREATE_DETACHED,
                );
                (&raw const detached).cast()
            } else {
                sev.sigev_notify_attributes
            };
            let mut tid: crate::pthread::PthreadT = 0;
            let rc = crate::pthread::pthread_create(&raw mut tid, attr, notify_thread, call);
            if rc != 0 {
                // glibc tests this with `< 0`, which a positive error number
                // never is, and so never notices; the caller is told.
                // SAFETY: the thread never started, so the block is ours.
                unsafe { crate::malloc::free(call) };
                errno::set_errno(rc);
                return Err(());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_is_musls() {
        assert_eq!(size_of::<SigeventView>(), 64);
        assert_eq!(core::mem::offset_of!(SigeventView, sigev_signo), 8);
        assert_eq!(core::mem::offset_of!(SigeventView, sigev_notify), 12);
        assert_eq!(
            core::mem::offset_of!(SigeventView, sigev_notify_function),
            16
        );
        assert_eq!(
            core::mem::offset_of!(SigeventView, sigev_notify_attributes),
            24
        );
    }

    #[test]
    fn test_none_notifies_nothing() {
        assert!(notify(&SigeventView::NONE).is_ok());
        let unknown = SigeventView {
            sigev_notify: 77,
            ..SigeventView::NONE
        };
        assert!(notify(&unknown).is_ok());
    }

    #[test]
    fn test_signal_zero_probes_and_a_bad_signal_fails() {
        let probe = SigeventView {
            sigev_notify: crate::time::SIGEV_SIGNAL,
            ..SigeventView::NONE
        };
        assert!(notify(&probe).is_ok());
        let bad = SigeventView {
            sigev_signo: 100_000,
            ..probe
        };
        errno::set_errno(0);
        assert!(notify(&bad).is_err());
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// No function is glibc calling NULL; on the host, a function cannot get
    /// its thread.
    #[test]
    fn test_thread_needs_a_function_and_a_thread() {
        extern "C" fn never(_: usize) {}
        let none = SigeventView {
            sigev_notify: crate::time::SIGEV_THREAD,
            ..SigeventView::NONE
        };
        errno::set_errno(0);
        assert!(notify(&none).is_err());
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let some = SigeventView {
            sigev_notify_function: Some(never),
            ..none
        };
        errno::set_errno(0);
        assert!(notify(&some).is_err());
        assert_eq!(errno::get_errno(), errno::EAGAIN);
    }

    #[test]
    fn test_read_takes_unaligned_bytes() {
        let mut bytes = [0u8; 72];
        let sev = SigeventView {
            sigev_value: 0x1234,
            sigev_signo: 10,
            sigev_notify: crate::time::SIGEV_SIGNAL,
            ..SigeventView::NONE
        };
        // SAFETY: 64 bytes at offset 3 of a 72-byte buffer.
        unsafe {
            core::ptr::write_unaligned(bytes.as_mut_ptr().add(3).cast::<SigeventView>(), sev);
        }
        let back = SigeventView::read(bytes[3..].as_ptr());
        assert_eq!(
            (back.sigev_value, back.sigev_signo, back.sigev_notify),
            (0x1234, 10, crate::time::SIGEV_SIGNAL)
        );
    }
}
