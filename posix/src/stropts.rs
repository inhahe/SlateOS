//! `<stropts.h>` — STREAMS interface constants.
//!
//! The STREAMS API is a legacy System V mechanism for building
//! protocol stacks.  It was included in POSIX.1-2001 but marked
//! obsolescent in POSIX.1-2008.  Most modern systems do not
//! implement STREAMS, but the header constants are still needed
//! for source compatibility.
//!
//! # Status of these calls
//!
//! Linux has never supported STREAMS — glibc's `stropts.h` stubs all
//! return ENOSYS. Solaris up to Solaris 11 supported them fully; AIX
//! supports a subset. Modern Linux software almost never uses
//! `putmsg`/`getmsg`/`putpmsg`/`getpmsg` — the few legacy programs that
//! still do are typically Solaris ports being rebuilt for Linux, where
//! the build system gates the STREAMS code path on a runtime probe
//! that catches ENOSYS and disables STREAMS support.
//!
//! Our position matches Linux glibc: STREAMS will never be implemented
//! in this OS (the design uses Channel IPC + io_uring + sockets, not
//! the STREAMS module-push model). The validators here exist to give
//! probing callers meaningful EBADF/EFAULT/EINVAL feedback on malformed
//! inputs, and a clean ENOSYS otherwise, so their existing fallback
//! paths fire correctly.

use crate::errno;

// ---------------------------------------------------------------------------
// ioctl commands for STREAMS
// ---------------------------------------------------------------------------

/// Push a module onto the stream.
pub const I_PUSH: i32 = 0x5302;

/// Pop the topmost module from the stream.
pub const I_POP: i32 = 0x5303;

/// Look at the topmost module.
pub const I_LOOK: i32 = 0x5304;

/// Flush read/write queues.
pub const I_FLUSH: i32 = 0x5305;

/// Send an ioctl downstream.
pub const I_STR: i32 = 0x5308;

/// Set read options.
pub const I_SRDOPT: i32 = 0x5301;

/// Get read options.
pub const I_GRDOPT: i32 = 0x5309;

/// Send a priority-band message.
pub const I_SENDFD: i32 = 0x5311;

/// Receive a file descriptor.
pub const I_RECVFD: i32 = 0x5312;

/// Find a module on the stream.
pub const I_FIND: i32 = 0x530B;

/// Link a stream underneath a multiplexor.
pub const I_LINK: i32 = 0x530C;

/// Unlink a stream from a multiplexor.
pub const I_UNLINK: i32 = 0x530D;

/// Check for pending input on the stream head.
pub const I_NREAD: i32 = 0x5318;

/// Peek at a message on the stream head.
pub const I_PEEK: i32 = 0x530F;

/// Create a file descriptor for a STREAMS-based pipe.
pub const I_FDINSERT: i32 = 0x5310;

/// Set event notifications.
pub const I_SETSIG: i32 = 0x5306;

/// Get current event notifications.
pub const I_GETSIG: i32 = 0x5307;

/// Check if a stream is associated with a terminal.
pub const I_CANPUT: i32 = 0x5313;

/// Persistent link.
pub const I_PLINK: i32 = 0x5316;

/// Persistent unlink.
pub const I_PUNLINK: i32 = 0x5317;

// ---------------------------------------------------------------------------
// Flush flags (for I_FLUSH)
// ---------------------------------------------------------------------------

/// Flush read queue.
pub const FLUSHR: i32 = 0x01;

/// Flush write queue.
pub const FLUSHW: i32 = 0x02;

/// Flush read and write queues.
pub const FLUSHRW: i32 = 0x03;

// ---------------------------------------------------------------------------
// Read options (for I_SRDOPT / I_GRDOPT)
// ---------------------------------------------------------------------------

/// Normal read mode (byte-stream).
pub const RNORM: i32 = 0x0000;

/// Message non-discard mode.
pub const RMSGN: i32 = 0x0001;

/// Message discard mode.
pub const RMSGD: i32 = 0x0002;

// ---------------------------------------------------------------------------
// Priority band flags
// ---------------------------------------------------------------------------

/// Normal (non-priority) message.
pub const RS_HIPRI: i32 = 0x01;

/// Any message (normal or priority).
pub const MSG_HIPRI: i32 = 0x01;

/// Any-band message.
pub const MSG_ANY: i32 = 0x02;

/// Band message.
pub const MSG_BAND: i32 = 0x04;

// ---------------------------------------------------------------------------
// Error codes specific to STREAMS
// ---------------------------------------------------------------------------

/// No message at stream head.
pub const MORECTL: i32 = 1;

/// More data expected.
pub const MOREDATA: i32 = 2;

/// More control and data expected.
pub const MORECTL_MOREDATA: i32 = 3;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum priority band number (POSIX `_POSIX_STREAM_MAX` and Solaris
/// `STRMSGSZ` agree on 255 as the upper bound).
pub const STREAM_BAND_MAX: i32 = 255;

/// Mask of valid putmsg flag bits. POSIX defines only `RS_HIPRI`.
pub const PUTMSG_FLAGS_VALID: i32 = RS_HIPRI;

/// Mask of valid putpmsg flag bits. POSIX defines `MSG_HIPRI` and
/// `MSG_BAND`. (Note `RS_HIPRI` and `MSG_HIPRI` share value `0x01` —
/// they're aliases for the same priority bit in different contexts.)
pub const PUTPMSG_FLAGS_VALID: i32 = MSG_HIPRI | MSG_BAND;

/// Mask of valid getpmsg flag bits.
pub const GETPMSG_FLAGS_VALID: i32 = MSG_HIPRI | MSG_ANY | MSG_BAND;

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Put a message onto a stream.
///
/// POSIX `putmsg(fd, ctlptr, dataptr, flags)` sends a message
/// (control and/or data parts) down the stream attached to `fd`.
/// `ctlptr` and `dataptr` each point to a `struct strbuf` (or NULL
/// to indicate "no control/data part").
///
/// Linux semantics: STREAMS is not implemented, so a real Linux
/// glibc returns ENOSYS for every call. We validate the obvious bad
/// cases first:
/// - `fd < 0` → EBADF.
/// - Both `ctlptr == NULL && dataptr == NULL` → EINVAL (POSIX requires
///   at least one part to be present).
/// - `flags & ~PUTMSG_FLAGS_VALID != 0` → EINVAL.
/// - All other inputs → ENOSYS (STREAMS not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putmsg(
    fd: i32,
    ctlptr: *const u8,
    dataptr: *const u8,
    flags: i32,
) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if ctlptr.is_null() && dataptr.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if flags & !PUTMSG_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Put a priority-band message onto a stream.
///
/// Like `putmsg`, plus an explicit `band` (0–255, normal=0). Linux
/// semantics + our additions:
/// - `fd < 0` → EBADF.
/// - Both ctlptr and dataptr NULL → EINVAL.
/// - `band < 0` or `band > STREAM_BAND_MAX (255)` → EINVAL.
/// - `flags & ~PUTPMSG_FLAGS_VALID != 0` → EINVAL.
/// - `MSG_HIPRI` set with nonzero band → EINVAL (high-priority messages
///   live in their own out-of-band path, not a priority band).
/// - All other inputs → ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putpmsg(
    fd: i32,
    ctlptr: *const u8,
    dataptr: *const u8,
    band: i32,
    flags: i32,
) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if ctlptr.is_null() && dataptr.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if !(0..=STREAM_BAND_MAX).contains(&band) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if flags & !PUTPMSG_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if (flags & MSG_HIPRI) != 0 && band != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get a message from a stream.
///
/// - `fd < 0` → EBADF.
/// - `flagsp == NULL` → EFAULT (the caller passes flags in/out by
///   pointer — without a real address we can't read or write the
///   request).
/// - All other inputs → ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getmsg(
    fd: i32,
    _ctlptr: *mut u8,
    _dataptr: *mut u8,
    flagsp: *mut i32,
) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if flagsp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get a priority-band message from a stream.
///
/// - `fd < 0` → EBADF.
/// - `bandp == NULL` or `flagsp == NULL` → EFAULT.
/// - `*flagsp & ~GETPMSG_FLAGS_VALID != 0` → EINVAL (caller is asking
///   for an unknown flag combination).
/// - All other inputs → ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpmsg(
    fd: i32,
    _ctlptr: *mut u8,
    _dataptr: *mut u8,
    bandp: *mut i32,
    flagsp: *mut i32,
) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if bandp.is_null() || flagsp.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: We've verified flagsp is non-null. Use read_unaligned to
    // tolerate any alignment; the value is just an i32 the caller put
    // in.
    let flag_in = unsafe { core::ptr::read_unaligned(flagsp) };
    if flag_in & !GETPMSG_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Test if a file descriptor is associated with a STREAMS device.
///
/// Returns 0 (not a STREAMS device — STREAMS not supported) for any
/// non-negative fd. For `fd < 0`, returns -1 with EBADF (matches
/// POSIX's "the function shall fail" clause).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isastream(fd: i32) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    0
}

/// Attach a STREAMS-based file descriptor to an object in the
/// filesystem name space.
///
/// - `fd < 0` → EBADF.
/// - `path == NULL` → EFAULT.
/// - `*path == 0` (empty string) → ENOENT.
/// - All other inputs → ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fattach(fd: i32, path: *const u8) -> i32 {
    if fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: path is non-null; reading the first byte is safe for any
    // caller-supplied C string (even a one-byte NUL).
    let first = unsafe { core::ptr::read(path) };
    if first == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Detach a STREAMS-based file descriptor from the filesystem.
///
/// - `path == NULL` → EFAULT.
/// - `*path == 0` (empty string) → ENOENT.
/// - All other inputs → ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdetach(path: *const u8) -> i32 {
    if path.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: see fattach.
    let first = unsafe { core::ptr::read(path) };
    if first == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ioctl command constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            I_PUSH, I_POP, I_LOOK, I_FLUSH, I_STR, I_SRDOPT,
            I_GRDOPT, I_SENDFD, I_RECVFD, I_FIND, I_LINK, I_UNLINK,
            I_NREAD, I_PEEK, I_FDINSERT, I_SETSIG, I_GETSIG,
            I_CANPUT, I_PLINK, I_PUNLINK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(
                    cmds[i], cmds[j],
                    "STREAMS ioctl commands must be distinct"
                );
            }
        }
    }

    #[test]
    fn test_push_pop() {
        assert_ne!(I_PUSH, I_POP);
    }

    #[test]
    fn test_link_unlink() {
        assert_ne!(I_LINK, I_UNLINK);
        assert_ne!(I_PLINK, I_PUNLINK);
    }

    #[test]
    fn test_flush_flags() {
        assert_eq!(FLUSHR, 0x01);
        assert_eq!(FLUSHW, 0x02);
        assert_eq!(FLUSHRW, FLUSHR | FLUSHW);
    }

    #[test]
    fn test_read_options() {
        assert_eq!(RNORM, 0);
        assert_ne!(RMSGN, RMSGD);
    }

    #[test]
    fn test_stream_band_max() {
        assert_eq!(STREAM_BAND_MAX, 255);
    }

    #[test]
    fn test_valid_flag_masks() {
        assert_eq!(PUTMSG_FLAGS_VALID, RS_HIPRI);
        assert_eq!(PUTPMSG_FLAGS_VALID, MSG_HIPRI | MSG_BAND);
        assert_eq!(GETPMSG_FLAGS_VALID, MSG_HIPRI | MSG_ANY | MSG_BAND);
    }

    // -----------------------------------------------------------------------
    // putmsg
    // -----------------------------------------------------------------------

    #[test]
    fn test_putmsg_negative_fd_ebadf() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putmsg(-1, dummy.as_ptr(), core::ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_putmsg_int_min_fd_ebadf() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putmsg(i32::MIN, dummy.as_ptr(), core::ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_putmsg_both_null_einval() {
        errno::set_errno(errno::EBADF);
        let r = putmsg(0, core::ptr::null(), core::ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_putmsg_unknown_flag_einval() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putmsg(0, dummy.as_ptr(), core::ptr::null(), 0x2);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_putmsg_valid_reaches_enosys() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putmsg(3, dummy.as_ptr(), core::ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_putmsg_hipri_reaches_enosys() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putmsg(3, dummy.as_ptr(), core::ptr::null(), RS_HIPRI);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_putmsg_data_only_reaches_enosys() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putmsg(3, core::ptr::null(), dummy.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // putpmsg
    // -----------------------------------------------------------------------

    #[test]
    fn test_putpmsg_negative_fd_ebadf() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putpmsg(-1, dummy.as_ptr(), core::ptr::null(), 0, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_putpmsg_both_null_einval() {
        errno::set_errno(errno::EBADF);
        let r = putpmsg(0, core::ptr::null(), core::ptr::null(), 0, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_putpmsg_negative_band_einval() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putpmsg(0, dummy.as_ptr(), core::ptr::null(), -1, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_putpmsg_huge_band_einval() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putpmsg(0, dummy.as_ptr(), core::ptr::null(), 256, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_putpmsg_band_at_max_ok() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putpmsg(0, dummy.as_ptr(), core::ptr::null(), 255, 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_putpmsg_unknown_flag_einval() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putpmsg(0, dummy.as_ptr(), core::ptr::null(), 0, 0x100);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_putpmsg_hipri_with_band_einval() {
        // HIPRI is out-of-band — it can't be set on a priority band.
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putpmsg(0, dummy.as_ptr(), core::ptr::null(), 1, MSG_HIPRI);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_putpmsg_hipri_with_zero_band_ok() {
        // HIPRI on band 0 is allowed; the validator falls through to ENOSYS.
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putpmsg(0, dummy.as_ptr(), core::ptr::null(), 0, MSG_HIPRI);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_putpmsg_msg_band_reaches_enosys() {
        let dummy = [0u8; 8];
        errno::set_errno(errno::EBADF);
        let r = putpmsg(0, dummy.as_ptr(), core::ptr::null(), 1, MSG_BAND);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // getmsg
    // -----------------------------------------------------------------------

    #[test]
    fn test_getmsg_negative_fd_ebadf() {
        let mut flags: i32 = 0;
        errno::set_errno(errno::EBADF);
        let r = getmsg(
            -1,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut flags,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_getmsg_null_flagsp_efault() {
        errno::set_errno(errno::EBADF);
        let r = getmsg(
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getmsg_valid_reaches_enosys() {
        let mut flags: i32 = 0;
        errno::set_errno(errno::EBADF);
        let r = getmsg(
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut flags,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // getpmsg
    // -----------------------------------------------------------------------

    #[test]
    fn test_getpmsg_negative_fd_ebadf() {
        let mut band: i32 = 0;
        let mut flags: i32 = 0;
        errno::set_errno(errno::EBADF);
        let r = getpmsg(
            -1,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut band,
            &raw mut flags,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_getpmsg_null_bandp_efault() {
        let mut flags: i32 = 0;
        errno::set_errno(errno::EBADF);
        let r = getpmsg(
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut flags,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getpmsg_null_flagsp_efault() {
        let mut band: i32 = 0;
        errno::set_errno(errno::EBADF);
        let r = getpmsg(
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut band,
            core::ptr::null_mut(),
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_getpmsg_unknown_flag_einval() {
        let mut band: i32 = 0;
        let mut flags: i32 = 0x80; // outside GETPMSG_FLAGS_VALID
        errno::set_errno(errno::EBADF);
        let r = getpmsg(
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut band,
            &raw mut flags,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_getpmsg_msg_any_reaches_enosys() {
        let mut band: i32 = 0;
        let mut flags: i32 = MSG_ANY;
        errno::set_errno(errno::EBADF);
        let r = getpmsg(
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut band,
            &raw mut flags,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // isastream
    // -----------------------------------------------------------------------

    #[test]
    fn test_isastream_negative_fd_ebadf() {
        errno::set_errno(errno::EBADF);
        let r = isastream(-1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_isastream_int_min_ebadf() {
        errno::set_errno(errno::EBADF);
        let r = isastream(i32::MIN);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_isastream_valid_fd_returns_zero() {
        // Any non-negative fd: not a STREAMS device, return 0 cleanly.
        assert_eq!(isastream(0), 0);
        assert_eq!(isastream(999), 0);
    }

    // -----------------------------------------------------------------------
    // fattach
    // -----------------------------------------------------------------------

    #[test]
    fn test_fattach_negative_fd_ebadf() {
        let path = b"/tmp/foo\0";
        errno::set_errno(errno::EBADF);
        let r = fattach(-1, path.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fattach_null_path_efault() {
        errno::set_errno(errno::EBADF);
        let r = fattach(0, core::ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_fattach_empty_path_enoent() {
        let empty = b"\0";
        errno::set_errno(errno::EBADF);
        let r = fattach(0, empty.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn test_fattach_valid_reaches_enosys() {
        let path = b"/var/run/streams/svc\0";
        errno::set_errno(errno::EBADF);
        let r = fattach(0, path.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // fdetach
    // -----------------------------------------------------------------------

    #[test]
    fn test_fdetach_null_path_efault() {
        errno::set_errno(errno::EBADF);
        let r = fdetach(core::ptr::null());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_fdetach_empty_path_enoent() {
        let empty = b"\0";
        errno::set_errno(errno::EBADF);
        let r = fdetach(empty.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);
    }

    #[test]
    fn test_fdetach_valid_reaches_enosys() {
        let path = b"/var/run/streams/svc\0";
        errno::set_errno(errno::EBADF);
        let r = fdetach(path.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // Real-world workflow tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_solaris_port_streams_probe_workflow() {
        // A Solaris-to-Linux port of a network daemon (TLI/XTI users,
        // Sybase netlibrary, NetManage's PC-NFS server) calls
        // putmsg(fd, &ctl, &data, 0) at startup to send a T_OPT_DATA_REQ
        // message. On ENOSYS, the port's autoconf-generated fallback
        // disables the STREAMS code path and reverts to plain sockets.
        let ctl_buf = [0u8; 32];
        let data_buf = [0u8; 256];
        errno::set_errno(errno::EBADF);
        let r = putmsg(5, ctl_buf.as_ptr(), data_buf.as_ptr(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_aix_tli_compat_workflow() {
        // AIX's TLI library calls getpmsg(fd, &ctl, &data, &band, &flag)
        // to read T_DATA_IND messages. Probe with flag = MSG_ANY to ask
        // "give me whatever is queued." On ENOSYS, the AIX-compat layer
        // disables TLI and falls back to BSD sockets.
        let mut band: i32 = 0;
        let mut flag: i32 = MSG_ANY;
        errno::set_errno(errno::EBADF);
        let r = getpmsg(
            7,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut band,
            &raw mut flag,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_isastream_probe_in_libc_init_workflow() {
        // glibc's `ttyname(3)` historically called `isastream(fd)` as a
        // pre-flight on the controlling terminal fd. Linux glibc just
        // returns 0 here; we match. Negative fd from a not-yet-opened
        // tty -> EBADF surfaces a real error to the caller.
        assert_eq!(isastream(2), 0); // stderr in the test process
        errno::set_errno(errno::EBADF);
        let r = isastream(-1);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fattach_streams_pipe_mount_workflow() {
        // Solaris-style "mount a pipe at a path" via fattach. A
        // /var/run/streams/foo path used by autofs's old streams-mount
        // helper. On ENOSYS the helper falls back to UNIX-domain socket
        // bind at the same path.
        let path = b"/var/run/streams/autofs\0";
        errno::set_errno(errno::EBADF);
        let r = fattach(5, path.as_ptr());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // errno-preserved-on-validation-success regression tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_putmsg_errno_set_to_enosys_on_validation_success() {
        let dummy = [0u8; 1];
        errno::set_errno(errno::EBADF);
        let r = putmsg(0, dummy.as_ptr(), core::ptr::null(), 0);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_getpmsg_errno_set_to_enosys_on_validation_success() {
        let mut band: i32 = 0;
        let mut flags: i32 = 0;
        errno::set_errno(errno::EBADF);
        let r = getpmsg(
            0,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            &raw mut band,
            &raw mut flags,
        );
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }
}
