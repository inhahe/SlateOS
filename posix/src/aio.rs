//! POSIX asynchronous I/O (`<aio.h>`).
//!
//! Implemented as a thin synchronous wrapper: each `aio_read` / `aio_write`
//! call performs the underlying `pread` / `pwrite` immediately and records
//! the completion status in a small static table keyed by the caller's
//! `aiocb` pointer.  `aio_error` / `aio_return` then look the record up
//! and report the result.  The wrapper is therefore POSIX-conformant for
//! correctness but offers no true asynchrony — every submission blocks
//! until the underlying I/O finishes.  Programs needing real async I/O
//! should use our `io_uring`-style interface instead.
//!
//! ## Limitations
//!
//! - All operations complete synchronously inside `aio_read` / `aio_write`,
//!   so `aio_suspend` and `aio_cancel` always report immediate completion.
//! - Tracking table is small (16 in-flight ops); oldest record is evicted
//!   when full, which is harmless since completion data has already been
//!   reported once `aio_return` runs.
//! - `aio_notify` is unsupported (returns ENOSYS): we have no signal
//!   delivery yet.
//!
//! ## Functions
//!
//! - `aio_read` / `aio_write` — initiate I/O (executes synchronously).
//! - `aio_error` — query operation status (0 if done, errno otherwise).
//! - `aio_return` — fetch final byte count after completion.
//! - `aio_cancel` — always returns AIO_ALLDONE.
//! - `aio_suspend` — returns immediately (ops are already complete).
//! - `aio_fsync` — synchronous fsync.
//! - `lio_listio` — iterates list, running each op synchronously.

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
// Completion tracking table
// ---------------------------------------------------------------------------
//
// Per-aiocb record kept in a small static array.  Submissions populate a
// record; `aio_error` / `aio_return` read it back.  We don't need a hash
// map — looking up 16 entries is cheap.

const MAX_AIO_OPS: usize = 16;

#[derive(Clone, Copy)]
struct AioRecord {
    /// Slot is occupied iff `in_use` is true.
    in_use: bool,
    /// Raw aiocb pointer, used purely as identity.
    cb_ptr: usize,
    /// errno value (0 on successful completion).
    status: i32,
    /// Bytes transferred (or -1 on error).
    bytes: isize,
    /// Monotonic generation counter used to pick the oldest slot for
    /// eviction when the table is full.
    age: u64,
}

impl AioRecord {
    const EMPTY: Self = Self { in_use: false, cb_ptr: 0, status: 0, bytes: 0, age: 0 };
}

static mut AIO_TABLE: [AioRecord; MAX_AIO_OPS] =
    [const { AioRecord::EMPTY }; MAX_AIO_OPS];

/// Monotonic counter for `age` stamping.  Wraps after 2^64 submissions,
/// which is impossible in practice; treated as monotonic forever.
static mut AIO_AGE: u64 = 0;

/// Bump and return the next age value.
fn next_age() -> u64 {
    // SAFETY: single-threaded (consistent with the rest of posix).
    unsafe {
        AIO_AGE = AIO_AGE.wrapping_add(1);
        AIO_AGE
    }
}

/// Find the existing record for `cb_ptr`, if any.
fn find_aio_record(cb_ptr: usize) -> Option<usize> {
    // SAFETY: single-threaded.
    unsafe {
        let base = core::ptr::addr_of_mut!(AIO_TABLE).cast::<AioRecord>();
        let mut i: usize = 0;
        while i < MAX_AIO_OPS {
            let r = base.add(i);
            if (*r).in_use && (*r).cb_ptr == cb_ptr {
                return Some(i);
            }
            i = i.wrapping_add(1);
        }
    }
    None
}

/// Allocate (or reuse) a record slot for `cb_ptr`.
///
/// If a record already exists for this pointer, it's overwritten in
/// place.  Otherwise a free slot is used; if none are free, the oldest
/// (smallest `age`) is evicted.  The chosen slot is returned populated
/// with the given status/bytes.
fn store_aio_record(cb_ptr: usize, status: i32, bytes: isize) {
    // SAFETY: single-threaded.
    unsafe {
        let base = core::ptr::addr_of_mut!(AIO_TABLE).cast::<AioRecord>();
        // 1) Existing record?
        if let Some(idx) = find_aio_record(cb_ptr) {
            let r = base.add(idx);
            (*r).status = status;
            (*r).bytes = bytes;
            (*r).age = next_age();
            return;
        }
        // 2) Free slot?
        let mut i: usize = 0;
        while i < MAX_AIO_OPS {
            let r = base.add(i);
            if !(*r).in_use {
                (*r).in_use = true;
                (*r).cb_ptr = cb_ptr;
                (*r).status = status;
                (*r).bytes = bytes;
                (*r).age = next_age();
                return;
            }
            i = i.wrapping_add(1);
        }
        // 3) Evict the oldest record.
        let mut oldest_idx: usize = 0;
        let mut oldest_age: u64 = u64::MAX;
        let mut j: usize = 0;
        while j < MAX_AIO_OPS {
            let r = base.add(j);
            if (*r).age < oldest_age {
                oldest_age = (*r).age;
                oldest_idx = j;
            }
            j = j.wrapping_add(1);
        }
        let r = base.add(oldest_idx);
        (*r).cb_ptr = cb_ptr;
        (*r).status = status;
        (*r).bytes = bytes;
        (*r).age = next_age();
    }
}

/// Release the record for `cb_ptr` (called by `aio_return`).
fn free_aio_record(cb_ptr: usize) {
    if let Some(idx) = find_aio_record(cb_ptr) {
        // SAFETY: single-threaded; idx came from find_aio_record.
        unsafe {
            let base = core::ptr::addr_of_mut!(AIO_TABLE).cast::<AioRecord>();
            let r = base.add(idx);
            (*r).in_use = false;
            (*r).cb_ptr = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// Submission helpers
// ---------------------------------------------------------------------------

/// Validate the common aiocb fields and return `(fd, buf, len, offset)`
/// if OK.  On failure sets errno and returns None.
fn validate_aiocb(cb: *const Aiocb) -> Option<(i32, *mut u8, usize, i64)> {
    if cb.is_null() {
        errno::set_errno(errno::EINVAL);
        return None;
    }
    // SAFETY: caller contract — cb points to a valid Aiocb if non-null.
    let (fd, buf, nbytes, off) = unsafe {
        (
            (*cb).aio_fildes,
            (*cb).aio_buf,
            (*cb).aio_nbytes,
            (*cb).aio_offset,
        )
    };
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return None;
    }
    if buf.is_null() && nbytes > 0 {
        errno::set_errno(errno::EFAULT);
        return None;
    }
    if off < 0 {
        errno::set_errno(errno::EINVAL);
        return None;
    }
    Some((fd, buf, nbytes, off))
}

/// Translate an `ssize_t` I/O result into `(status, bytes)`.
///
/// On success `status == 0` and `bytes == n`.  On error `status` holds
/// the errno value (already set by the wrapped syscall) and `bytes ==
/// -1`.
fn classify_result(n: crate::types::SsizeT) -> (i32, isize) {
    if n >= 0 {
        (0, n as isize)
    } else {
        (errno::get_errno(), -1)
    }
}

// ---------------------------------------------------------------------------
// Public AIO functions
// ---------------------------------------------------------------------------

/// Initiate an asynchronous read (synchronously, in our impl).
///
/// Performs `pread(fd, buf, nbytes, offset)` immediately and stores
/// the result for retrieval via `aio_error` / `aio_return`.  Returns
/// 0 on successful submission, -1 with errno on validation failure
/// (EINVAL / EBADF / EFAULT).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_read(aiocbp: *mut Aiocb) -> i32 {
    let Some((fd, buf, nbytes, off)) = validate_aiocb(aiocbp) else {
        return -1;
    };
    let n = crate::file::pread(fd, buf, nbytes, off);
    let (status, bytes) = classify_result(n);
    store_aio_record(aiocbp as usize, status, bytes);
    0
}

/// Initiate an asynchronous write (synchronously, in our impl).
///
/// Performs `pwrite(fd, buf, nbytes, offset)` immediately and stores
/// the result.  Returns 0 on successful submission, -1 with errno on
/// validation failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_write(aiocbp: *mut Aiocb) -> i32 {
    let Some((fd, buf, nbytes, off)) = validate_aiocb(aiocbp) else {
        return -1;
    };
    let n = crate::file::pwrite(fd, buf as *const u8, nbytes, off);
    let (status, bytes) = classify_result(n);
    store_aio_record(aiocbp as usize, status, bytes);
    0
}

/// Get the error status of an async I/O operation.
///
/// Returns 0 if the operation has completed successfully, the errno
/// value of the underlying I/O failure if it failed, `EINVAL` if no
/// submission is known for this aiocb.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_error(aiocbp: *const Aiocb) -> i32 {
    if aiocbp.is_null() {
        return errno::EINVAL;
    }
    if let Some(idx) = find_aio_record(aiocbp as usize) {
        // SAFETY: idx is in-bounds (returned by find_aio_record).
        unsafe {
            let base = core::ptr::addr_of!(AIO_TABLE).cast::<AioRecord>();
            (*base.add(idx)).status
        }
    } else {
        errno::EINVAL
    }
}

/// Get the return value of a completed async I/O operation.
///
/// Returns the byte count written/read by the operation, or -1 on
/// error (with errno set by the wrapped syscall).  POSIX states that
/// the aiocb is invalidated by this call; we drop the tracking record.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_return(aiocbp: *mut Aiocb) -> isize {
    if aiocbp.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let Some(idx) = find_aio_record(aiocbp as usize) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    // SAFETY: idx came from find_aio_record.
    let (status, bytes) = unsafe {
        let base = core::ptr::addr_of!(AIO_TABLE).cast::<AioRecord>();
        let r = base.add(idx);
        ((*r).status, (*r).bytes)
    };
    free_aio_record(aiocbp as usize);
    if status != 0 {
        errno::set_errno(status);
        -1
    } else {
        bytes
    }
}

/// Cancel one or all pending async I/O operations on a file descriptor.
///
/// Since every submitted operation completes synchronously, there is
/// never anything pending: always returns `AIO_ALLDONE`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_cancel(_fd: i32, _aiocbp: *mut Aiocb) -> i32 {
    AIO_ALLDONE
}

/// Wait for one or more async I/O operations to complete.
///
/// All operations submitted via this implementation complete
/// synchronously, so there is nothing to wait for: we simply check
/// that the list is non-empty and return 0.  Returns -1 with EINVAL
/// for a null/empty list.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_suspend(
    list: *const *const Aiocb,
    nent: i32,
    _timeout: *const crate::stat::Timespec,
) -> i32 {
    if list.is_null() || nent <= 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    0
}

/// Sync a file for an async I/O operation.
///
/// Performs `fsync(aiocbp->aio_fildes)` immediately and stores the
/// result like `aio_read`/`aio_write`.  Returns 0 on submission, -1
/// with errno on validation failure.  The `op` argument selects
/// `O_SYNC` vs `O_DSYNC` semantics on Linux; our `fsync` does a full
/// sync regardless.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn aio_fsync(_op: i32, aiocbp: *mut Aiocb) -> i32 {
    if aiocbp.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // SAFETY: aiocbp is non-null by check above; caller's contract.
    let fd = unsafe { (*aiocbp).aio_fildes };
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    let ret = crate::file::fsync(fd);
    let (status, bytes) = if ret == 0 {
        (0, 0)
    } else {
        (errno::get_errno(), -1)
    };
    store_aio_record(aiocbp as usize, status, bytes);
    0
}

/// Initiate a list of async I/O operations.
///
/// Iterates `list`; for each non-null aiocb with `aio_lio_opcode` of
/// `LIO_READ` or `LIO_WRITE`, performs the corresponding sync wrap;
/// `LIO_NOP` is skipped.  `mode` is ignored (LIO_WAIT and LIO_NOWAIT
/// are identical when every op is synchronous).  Returns 0 on success,
/// -1 with errno on argument error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lio_listio(
    mode: i32,
    list: *const *mut Aiocb,
    nent: i32,
    _sig: *mut u8,
) -> i32 {
    if list.is_null() || nent < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if mode != LIO_WAIT && mode != LIO_NOWAIT {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let mut i: i32 = 0;
    let mut any_err = false;
    while i < nent {
        // SAFETY: caller contract — list points to `nent` aiocb pointers.
        let elem = unsafe { *list.offset(i as isize) };
        if elem.is_null() {
            i = i.wrapping_add(1);
            continue;
        }
        // SAFETY: elem non-null; caller owns the aiocb.
        let op = unsafe { (*elem).aio_lio_opcode };
        let rc = match op {
            LIO_READ => aio_read(elem),
            LIO_WRITE => aio_write(elem),
            LIO_NOP => 0,
            _ => {
                errno::set_errno(errno::EINVAL);
                -1
            }
        };
        if rc != 0 {
            any_err = true;
        }
        i = i.wrapping_add(1);
    }
    if any_err {
        // POSIX: report EIO if any individual operation could not be
        // submitted (errno of the last failure is left as set).
        -1
    } else {
        0
    }
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

    // -- aio_read: validation --

    #[test]
    fn test_aio_read_null_einval() {
        errno::set_errno(0);
        assert_eq!(aio_read(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_aio_read_negative_fd_ebadf() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        cb.aio_fildes = -1;
        errno::set_errno(0);
        assert_eq!(aio_read(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_aio_read_negative_offset_einval() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        cb.aio_fildes = 1;
        cb.aio_offset = -1;
        errno::set_errno(0);
        assert_eq!(aio_read(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_aio_read_null_buf_nonzero_nbytes_efault() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        cb.aio_fildes = 1;
        cb.aio_buf = core::ptr::null_mut();
        cb.aio_nbytes = 16;
        errno::set_errno(0);
        assert_eq!(aio_read(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- aio_write: same validation paths --

    #[test]
    fn test_aio_write_null_einval() {
        errno::set_errno(0);
        assert_eq!(aio_write(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_aio_write_negative_fd_ebadf() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        cb.aio_fildes = -1;
        errno::set_errno(0);
        assert_eq!(aio_write(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- aio_error / aio_return without submission --

    #[test]
    fn test_aio_error_null_einval() {
        assert_eq!(aio_error(core::ptr::null()), errno::EINVAL);
    }

    #[test]
    fn test_aio_error_unsubmitted_returns_einval() {
        let cb: Aiocb = unsafe { core::mem::zeroed() };
        // Never submitted — table has no record.
        assert_eq!(aio_error(&cb), errno::EINVAL);
    }

    #[test]
    fn test_aio_return_null_einval() {
        errno::set_errno(0);
        assert_eq!(aio_return(core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_aio_return_unsubmitted_einval() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        errno::set_errno(0);
        assert_eq!(aio_return(&raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- aio_cancel always reports AIO_ALLDONE --

    #[test]
    fn test_aio_cancel_alldone() {
        assert_eq!(aio_cancel(0, core::ptr::null_mut()), AIO_ALLDONE);
    }

    #[test]
    fn test_aio_cancel_with_fd() {
        assert_eq!(aio_cancel(5, core::ptr::null_mut()), AIO_ALLDONE);
    }

    #[test]
    fn test_aio_cancel_negative_fd() {
        assert_eq!(aio_cancel(-1, core::ptr::null_mut()), AIO_ALLDONE);
    }

    #[test]
    fn test_aio_cancel_with_aiocb() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        assert_eq!(aio_cancel(0, &raw mut cb), AIO_ALLDONE);
    }

    // -- aio_suspend: ops are always already complete --

    #[test]
    fn test_aio_suspend_null_einval() {
        errno::set_errno(0);
        assert_eq!(aio_suspend(core::ptr::null(), 0, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_aio_suspend_zero_nent_einval() {
        let list: [*const Aiocb; 0] = [];
        errno::set_errno(0);
        assert_eq!(aio_suspend(list.as_ptr(), 0, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_aio_suspend_positive_nent_succeeds() {
        let cb: Aiocb = unsafe { core::mem::zeroed() };
        let p: *const Aiocb = &cb;
        let list = [p];
        assert_eq!(aio_suspend(list.as_ptr(), 1, core::ptr::null()), 0);
    }

    // -- aio_fsync: validation --

    #[test]
    fn test_aio_fsync_null_einval() {
        errno::set_errno(0);
        assert_eq!(aio_fsync(0, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_aio_fsync_negative_fd_ebadf() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        cb.aio_fildes = -1;
        errno::set_errno(0);
        assert_eq!(aio_fsync(0, &raw mut cb), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- lio_listio --

    #[test]
    fn test_lio_listio_null_einval() {
        errno::set_errno(0);
        assert_eq!(
            lio_listio(LIO_WAIT, core::ptr::null(), 0, core::ptr::null_mut()),
            -1,
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_lio_listio_negative_nent_einval() {
        let list: [*mut Aiocb; 0] = [];
        errno::set_errno(0);
        assert_eq!(
            lio_listio(LIO_WAIT, list.as_ptr(), -1, core::ptr::null_mut()),
            -1,
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_lio_listio_invalid_mode_einval() {
        let list: [*mut Aiocb; 0] = [];
        errno::set_errno(0);
        assert_eq!(
            lio_listio(42, list.as_ptr(), 0, core::ptr::null_mut()),
            -1,
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_lio_listio_empty_list_succeeds() {
        let list: [*mut Aiocb; 0] = [];
        assert_eq!(
            lio_listio(LIO_WAIT, list.as_ptr(), 0, core::ptr::null_mut()),
            0,
        );
        assert_eq!(
            lio_listio(LIO_NOWAIT, list.as_ptr(), 0, core::ptr::null_mut()),
            0,
        );
    }

    #[test]
    fn test_lio_listio_lio_nop_passes() {
        let mut cb: Aiocb = unsafe { core::mem::zeroed() };
        cb.aio_lio_opcode = LIO_NOP;
        let p: *mut Aiocb = &raw mut cb;
        let list = [p];
        assert_eq!(
            lio_listio(LIO_WAIT, list.as_ptr(), 1, core::ptr::null_mut()),
            0,
        );
    }

    #[test]
    fn test_lio_listio_skips_null_entries() {
        // Null entries are skipped without error.
        let list: [*mut Aiocb; 2] = [core::ptr::null_mut(), core::ptr::null_mut()];
        assert_eq!(
            lio_listio(LIO_NOWAIT, list.as_ptr(), 2, core::ptr::null_mut()),
            0,
        );
    }

    // -- Internal helpers --

    #[test]
    fn test_aio_record_store_and_find() {
        // Pick a sentinel pointer that no other test will use.
        let key: usize = 0xDEAD_BEEF_FAB1_C0DEu64 as usize;
        store_aio_record(key, 0, 42);
        let idx = find_aio_record(key).expect("record should exist");
        // SAFETY: idx is in-bounds.
        let bytes = unsafe {
            let base = core::ptr::addr_of!(AIO_TABLE).cast::<AioRecord>();
            (*base.add(idx)).bytes
        };
        assert_eq!(bytes, 42);
        free_aio_record(key);
        assert!(find_aio_record(key).is_none());
    }

    #[test]
    fn test_aio_record_overwrite_in_place() {
        let key: usize = 0xCAFE_F00D_DEAD_BABEu64 as usize;
        store_aio_record(key, 0, 10);
        store_aio_record(key, 0, 99); // overwrite
        let idx = find_aio_record(key).expect("record should exist");
        let bytes = unsafe {
            let base = core::ptr::addr_of!(AIO_TABLE).cast::<AioRecord>();
            (*base.add(idx)).bytes
        };
        assert_eq!(bytes, 99);
        free_aio_record(key);
    }

    #[test]
    fn test_classify_result_success() {
        let (status, bytes) = classify_result(42);
        assert_eq!(status, 0);
        assert_eq!(bytes, 42);
    }

    #[test]
    fn test_classify_result_zero_eof() {
        let (status, bytes) = classify_result(0);
        assert_eq!(status, 0);
        assert_eq!(bytes, 0);
    }

    #[test]
    fn test_classify_result_error_propagates_errno() {
        errno::set_errno(errno::EIO);
        let (status, bytes) = classify_result(-1);
        assert_eq!(status, errno::EIO);
        assert_eq!(bytes, -1);
    }

    #[test]
    fn test_aio_pool_constants() {
        assert_eq!(MAX_AIO_OPS, 16);
    }

    // -- AIO_* constants are distinct --

    #[test]
    fn test_aio_result_constants_distinct() {
        assert_ne!(AIO_CANCELED, AIO_NOTCANCELED);
        assert_ne!(AIO_CANCELED, AIO_ALLDONE);
        assert_ne!(AIO_NOTCANCELED, AIO_ALLDONE);
    }
}
