//! The checks Linux 6.6 makes of a caller's buffers before a read or a write
//! moves a byte: lib/iov_iter.c's `import_ubuf` for one buffer,
//! `iovec_from_user` and `import_iovec` for a vector of them.
//!
//! One copy, used by every call that takes a caller's I/O vector -- kernel
//! AIO's `IOCB_CMD_PREADV`/`PWRITEV`, `process_vm_readv`/`writev` and
//! `process_madvise` so far.
//! They had grown their own, and the copies differed: one judged the vector's
//! count at 64 bits where upstream's `import_iovec` takes an `unsigned`, one
//! never asked `access_ok` of the array it was about to read.
//!
//! ```text
//!   import_ubuf(buf, len)        len capped at MAX_RW_COUNT, then
//!                                !access_ok(buf, len)        -> EFAULT
//!
//!   iovec_from_user(iov, n)      n == 0                      -> no segments
//!                                n > UIO_MAXIOV              -> EINVAL
//!                                the array unreadable        -> EFAULT
//!                                any iov_len > SSIZE_MAX     -> EINVAL
//!
//!   import_iovec(iov, n)         n == 1: that one segment read as above,
//!                                        then import_ubuf judges it
//!                                else:   iovec_from_user, then each
//!                                        segment's full length through
//!                                        access_ok (EFAULT), the total
//!                                        capped at MAX_RW_COUNT
//! ```
//!
//! What cannot be reproduced is the fault itself: upstream's copy fails
//! cleanly on any unmapped address, and a library can only refuse the
//! addresses no mapping can have -- NULL, and anything `access_ok` rejects.
//! Any other bad pointer faults here as it would in any C library.

use crate::errno;
use crate::file::Iovec;

/// Linux's `UIO_MAXIOV`: the most segments one vector may have.
pub(crate) const UIO_MAXIOV: u64 = 1024;

/// Linux's `MAX_RW_COUNT` (include/linux/fs.h), `INT_MAX & PAGE_MASK` with
/// Linux's 4 KiB page: the most one read or write transfers.
pub(crate) const MAX_RW_COUNT: usize = 0x7FFF_F000;

/// `access_ok` on x86-64 (arch/x86/include/asm/uaccess_64.h) for a run-time
/// size: the range must not wrap and must end in the user half, which
/// `valid_user_address` tests as `(long)(x) >= 0`.
///
/// It admits NULL.  A NULL buffer is caught where it is dereferenced, not
/// here.
pub(crate) fn access_ok(base: usize, len: usize) -> bool {
    let (end, wrapped) = base.overflowing_add(len);
    !wrapped && end.cast_signed() >= 0
}

/// `import_ubuf` (and `import_single_range`, its twin): one buffer of the
/// caller's, its length capped at `MAX_RW_COUNT` *before* `access_ok`
/// judges it.  Returns the capped length.
pub(crate) fn import_ubuf(base: usize, len: usize) -> Result<usize, i32> {
    let len = len.min(MAX_RW_COUNT);
    if access_ok(base, len) {
        Ok(len)
    } else {
        Err(errno::EFAULT)
    }
}

/// `iovec_from_user`: a vector's segments, each length judged.
///
/// `nr_segs` is upstream's `unsigned long`.  `access_ok` of the array is
/// `copy_iovec_from_user`'s `user_access_begin`; NULL is the fault a NULL
/// array would take.
///
/// # Safety
///
/// When `nr_segs` is at most `UIO_MAXIOV` and `iov` is non-NULL and passes
/// `access_ok`, `iov` must point to `nr_segs` readable `Iovec`s that outlive
/// `'a`.
pub(crate) unsafe fn iovec_from_user<'a>(
    iov: *const Iovec,
    nr_segs: u64,
) -> Result<&'a [Iovec], i32> {
    if nr_segs == 0 {
        return Ok(&[]);
    }
    if nr_segs > UIO_MAXIOV {
        return Err(errno::EINVAL);
    }
    // `nr_segs <= UIO_MAXIOV`, so the count and the byte size fit.
    let n = usize::try_from(nr_segs).map_err(|_| errno::EINVAL)?;
    let bytes = n.saturating_mul(size_of::<Iovec>());
    if iov.is_null() || !access_ok(iov as usize, bytes) {
        return Err(errno::EFAULT);
    }
    // SAFETY: the caller's contract, for a non-NULL array in range.
    let segs = unsafe { core::slice::from_raw_parts(iov, n) };
    // "check for size_t not fitting in ssize_t": any one segment, before any
    // is judged for its range.
    if segs.iter().any(|seg| seg.iov_len.cast_signed() < 0) {
        return Err(errno::EINVAL);
    }
    Ok(segs)
}

/// `import_iovec`: a vector of the caller's own memory.  Returns the bytes it
/// covers, capped at `MAX_RW_COUNT` as upstream caps the segments.
///
/// `nr_segs` is upstream's `unsigned`: a caller holding a wider count
/// truncates it, as the C call's conversion does.  One segment takes
/// `__import_iovec_ubuf`'s path, which caps its length *before* `access_ok`
/// -- so one segment of `SSIZE_MAX` bytes is accepted, and two are not.
///
/// # Safety
///
/// As [`iovec_from_user`].
pub(crate) unsafe fn import_iovec(iov: *const Iovec, nr_segs: u32) -> Result<usize, i32> {
    // SAFETY: the caller's contract.
    let segs = unsafe { iovec_from_user(iov, u64::from(nr_segs)) }?;
    if let [one] = segs {
        return import_ubuf(one.iov_base as usize, one.iov_len);
    }
    let mut total = 0usize;
    for seg in segs {
        if !access_ok(seg.iov_base as usize, seg.iov_len) {
            return Err(errno::EFAULT);
        }
        // `total <= MAX_RW_COUNT` throughout, so the subtraction holds.
        total = total.saturating_add(seg.iov_len.min(MAX_RW_COUNT.saturating_sub(total)));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(base: usize, len: usize) -> Iovec {
        Iovec {
            iov_base: base as *mut u8,
            iov_len: len,
        }
    }

    /// `access_ok` admits NULL: it is a range test, and `valid_user_address`
    /// is a sign test on the end.
    #[test]
    fn access_ok_matches_upstream() {
        assert!(access_ok(0, 4096), "NULL is in range; it faults on use");
        assert!(access_ok(0x1000, i64::MAX as usize - 0x1000));
        assert!(!access_ok(0x1000, i64::MAX as usize - 0xFFF));
        assert!(!access_ok(usize::MAX, 2), "a wrapping range");
        assert!(!access_ok(1 << 63, 0), "a kernel-half start");
    }

    #[test]
    fn import_ubuf_caps_before_it_judges() {
        assert_eq!(import_ubuf(0x1000, 10), Ok(10));
        assert_eq!(import_ubuf(0x1000, usize::MAX), Ok(MAX_RW_COUNT));
        assert_eq!(import_ubuf(1 << 63, 1), Err(errno::EFAULT));
        // A range that ends in the kernel half only before the cap.
        let near_top = (i64::MAX as usize) - MAX_RW_COUNT;
        assert_eq!(import_ubuf(near_top, usize::MAX), Ok(MAX_RW_COUNT));
    }

    #[test]
    fn iovec_from_user_verdicts_in_order() {
        // SAFETY (every call): the vectors are local arrays of the stated
        // length, or NULL.
        unsafe {
            assert_eq!(iovec_from_user(core::ptr::null(), 0).map(<[_]>::len), Ok(0));
            assert_eq!(
                iovec_from_user(core::ptr::null(), UIO_MAXIOV + 1).map(<[_]>::len),
                Err(errno::EINVAL),
                "the count before the pointer"
            );
            assert_eq!(
                iovec_from_user(core::ptr::null(), 1).map(<[_]>::len),
                Err(errno::EFAULT)
            );
            assert_eq!(
                iovec_from_user((1usize << 63) as *const Iovec, 1).map(<[_]>::len),
                Err(errno::EFAULT),
                "an array in the kernel half is not read"
            );
            let v = [seg(0x1000, 1), seg(1 << 63, usize::MAX)];
            assert_eq!(
                iovec_from_user(v.as_ptr(), 2).map(<[_]>::len),
                Err(errno::EINVAL),
                "a length past SSIZE_MAX, whatever the ranges"
            );
            let v = [seg(0x1000, 1), seg(0x2000, 2)];
            assert_eq!(iovec_from_user(v.as_ptr(), 2).map(<[_]>::len), Ok(2));
        }
    }

    #[test]
    fn import_iovec_one_segment_is_capped_first() {
        let big = [seg(0x1000, i64::MAX as usize - 0x1000)];
        // SAFETY: a local array of one.
        assert_eq!(unsafe { import_iovec(big.as_ptr(), 1) }, Ok(MAX_RW_COUNT));
        let two = [seg(0x1000, i64::MAX as usize - 0x1000), seg(0x1000, 1)];
        // SAFETY: a local array of two.
        assert_eq!(unsafe { import_iovec(two.as_ptr(), 2) }, Ok(MAX_RW_COUNT));
        let past = [seg(0x1000, i64::MAX as usize - 0xFFF), seg(0x1000, 1)];
        // SAFETY: as above.
        assert_eq!(
            unsafe { import_iovec(past.as_ptr(), 2) },
            Err(errno::EFAULT),
            "two segments are judged at full length"
        );
    }

    #[test]
    fn import_iovec_sums_and_caps() {
        let v = [seg(0x1000, 3), seg(0x2000, 0), seg(0x3000, 4)];
        // SAFETY: a local array of three.
        assert_eq!(unsafe { import_iovec(v.as_ptr(), 3) }, Ok(7));
        let v = [seg(0x1000, MAX_RW_COUNT - 1), seg(0x2000_0000, 5)];
        // SAFETY: a local array of two.
        assert_eq!(unsafe { import_iovec(v.as_ptr(), 2) }, Ok(MAX_RW_COUNT));
        // SAFETY: nothing is read for no segments.
        assert_eq!(unsafe { import_iovec(core::ptr::null(), 0) }, Ok(0));
    }
}
