//! POSIX asynchronous I/O stubs (`<aio.h>`).
//!
//! Our OS does not implement POSIX AIO.  These stubs allow programs
//! that probe for AIO support at runtime to get a clean "not supported"
//! response.  Programs needing async I/O should use our `io_uring`-style
//! interface instead.
//!
//! ## Stubbed Functions
//!
//! - `aio_read` — initiate async read
//! - `aio_write` — initiate async write
//! - `aio_error` — check status of async operation
//! - `aio_return` — get return value of completed async operation
//! - `aio_cancel` — cancel pending async operations
//! - `aio_suspend` — wait for async operations
//! - `aio_fsync` — sync file for async operation
//! - `lio_listio` — initiate a list of async operations

use crate::errno;

// ---------------------------------------------------------------------------
// aiocb — async I/O control block
// ---------------------------------------------------------------------------

/// Async I/O control block.
///
/// Matches the POSIX `struct aiocb` layout.  All fields are present
/// for source compatibility even though the operations are stubbed.
#[repr(C)]
pub struct Aiocb {
    /// File descriptor.
    pub aio_fildes: i32,
    /// Offset within file.
    pub aio_offset: i64,
    /// Buffer for I/O.
    pub aio_buf: *mut u8,
    /// Number of bytes to read/write.
    pub aio_nbytes: usize,
    /// Request priority offset.
    pub aio_reqprio: i32,
    /// Signal notification.
    pub aio_sigevent: [u8; 64], // Opaque sigevent-sized placeholder.
    /// Operation (LIO_READ, LIO_WRITE, LIO_NOP).
    pub aio_lio_opcode: i32,
    /// Internal padding/reserved.
    _reserved: [u8; 32],
}

// ---------------------------------------------------------------------------
// lio_listio mode constants
// ---------------------------------------------------------------------------

/// Wait for all operations to complete.
pub const LIO_WAIT: i32 = 0;
/// Do not wait (notify via sigevent).
pub const LIO_NOWAIT: i32 = 1;

/// Read operation for lio_listio.
pub const LIO_READ: i32 = 0;
/// Write operation for lio_listio.
pub const LIO_WRITE: i32 = 1;
/// No-op for lio_listio.
pub const LIO_NOP: i32 = 2;

/// Cancel all operations on a file descriptor.
pub const AIO_CANCELED: i32 = 0;
/// Some operations could not be canceled.
pub const AIO_NOTCANCELED: i32 = 1;
/// All requested operations completed before cancel.
pub const AIO_ALLDONE: i32 = 2;

// ---------------------------------------------------------------------------
// Stub functions
// ---------------------------------------------------------------------------

/// Initiate an asynchronous read.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_read(_aiocbp: *mut Aiocb) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Initiate an asynchronous write.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_write(_aiocbp: *mut Aiocb) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get the error status of an async I/O operation.
///
/// Stub: returns ENOSYS (operation not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_error(_aiocbp: *const Aiocb) -> i32 {
    errno::ENOSYS
}

/// Get the return value of a completed async I/O operation.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_return(_aiocbp: *mut Aiocb) -> isize {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Cancel one or all pending async I/O operations on a file descriptor.
///
/// Stub: returns AIO_ALLDONE (nothing to cancel).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_cancel(_fd: i32, _aiocbp: *mut Aiocb) -> i32 {
    AIO_ALLDONE
}

/// Wait for one or more async I/O operations to complete.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_suspend(
    _list: *const *const Aiocb,
    _nent: i32,
    _timeout: *const crate::stat::Timespec,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Sync a file for an async I/O operation.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_fsync(_op: i32, _aiocbp: *mut Aiocb) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Initiate a list of async I/O operations.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lio_listio(
    _mode: i32,
    _list: *const *mut Aiocb,
    _nent: i32,
    _sig: *mut u8, // sigevent pointer
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Struct layout --

    #[test]
    fn test_aiocb_size() {
        let size = core::mem::size_of::<Aiocb>();
        // Should be large enough to hold all fields.
        assert!(size >= 120, "Aiocb should be at least 120 bytes, got {size}");
    }

    #[test]
    fn test_aiocb_alignment() {
        assert!(core::mem::align_of::<Aiocb>() >= 8,
            "Aiocb should be aligned to at least 8 bytes");
    }

    // -- Constants --

    #[test]
    fn test_lio_constants() {
        assert_eq!(LIO_WAIT, 0);
        assert_eq!(LIO_NOWAIT, 1);
        assert_eq!(LIO_READ, 0);
        assert_eq!(LIO_WRITE, 1);
        assert_eq!(LIO_NOP, 2);
    }

    #[test]
    fn test_aio_cancel_constants() {
        assert_eq!(AIO_CANCELED, 0);
        assert_eq!(AIO_NOTCANCELED, 1);
        assert_eq!(AIO_ALLDONE, 2);
    }

    #[test]
    fn test_lio_opcodes_distinct() {
        assert_ne!(LIO_READ, LIO_WRITE);
        assert_ne!(LIO_READ, LIO_NOP);
        assert_ne!(LIO_WRITE, LIO_NOP);
    }

    // -- aio_read --

    #[test]
    fn test_aio_read_enosys() {
        errno::set_errno(0);
        assert_eq!(aio_read(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- aio_write --

    #[test]
    fn test_aio_write_enosys() {
        errno::set_errno(0);
        assert_eq!(aio_write(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- aio_error --

    #[test]
    fn test_aio_error_returns_enosys() {
        let ret = aio_error(core::ptr::null());
        assert_eq!(ret, errno::ENOSYS);
    }

    // -- aio_return --

    #[test]
    fn test_aio_return_enosys() {
        errno::set_errno(0);
        assert_eq!(aio_return(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- aio_cancel --

    #[test]
    fn test_aio_cancel_alldone() {
        let ret = aio_cancel(0, core::ptr::null_mut());
        assert_eq!(ret, AIO_ALLDONE);
    }

    #[test]
    fn test_aio_cancel_with_fd() {
        let ret = aio_cancel(5, core::ptr::null_mut());
        assert_eq!(ret, AIO_ALLDONE);
    }

    // -- aio_suspend --

    #[test]
    fn test_aio_suspend_enosys() {
        errno::set_errno(0);
        assert_eq!(aio_suspend(core::ptr::null(), 0, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- aio_fsync --

    #[test]
    fn test_aio_fsync_enosys() {
        errno::set_errno(0);
        assert_eq!(aio_fsync(0, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- lio_listio --

    #[test]
    fn test_lio_listio_enosys() {
        errno::set_errno(0);
        assert_eq!(lio_listio(LIO_WAIT, core::ptr::null(), 0, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_lio_listio_nowait_enosys() {
        errno::set_errno(0);
        assert_eq!(lio_listio(LIO_NOWAIT, core::ptr::null(), 0, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- aio_read doesn't crash with zero struct --

    #[test]
    fn test_aio_read_with_zeroed_aiocb() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        errno::set_errno(0);
        assert_eq!(aio_read(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_aio_write_with_zeroed_aiocb() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        errno::set_errno(0);
        assert_eq!(aio_write(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_aio_error_with_zeroed_aiocb() {
        let cb: Aiocb = unsafe { core::mem::zeroed() };
        assert_eq!(aio_error(&cb), errno::ENOSYS);
    }

    #[test]
    fn test_aio_return_with_zeroed_aiocb() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        errno::set_errno(0);
        assert_eq!(aio_return(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_aio_cancel_with_aiocb() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        assert_eq!(aio_cancel(0, &raw mut cb), AIO_ALLDONE);
    }

    #[test]
    fn test_aio_fsync_with_aiocb() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        errno::set_errno(0);
        assert_eq!(aio_fsync(1, &raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- aio_suspend with count > 0 --

    #[test]
    fn test_aio_suspend_nent_positive() {
        errno::set_errno(0);
        assert_eq!(aio_suspend(core::ptr::null(), 5, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- aio_cancel with negative fd --

    #[test]
    fn test_aio_cancel_negative_fd() {
        assert_eq!(aio_cancel(-1, core::ptr::null_mut()), AIO_ALLDONE);
    }

    // -- AIO_* constants are distinct --

    #[test]
    fn test_aio_result_constants_distinct() {
        assert_ne!(AIO_CANCELED, AIO_NOTCANCELED);
        assert_ne!(AIO_CANCELED, AIO_ALLDONE);
        assert_ne!(AIO_NOTCANCELED, AIO_ALLDONE);
    }
}
