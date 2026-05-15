//! `<stropts.h>` — STREAMS interface constants.
//!
//! The STREAMS API is a legacy System V mechanism for building
//! protocol stacks.  It was included in POSIX.1-2001 but marked
//! obsolescent in POSIX.1-2008.  Most modern systems do not
//! implement STREAMS, but the header constants are still needed
//! for source compatibility.
//!
//! All function stubs return `ENOSYS`.

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
// Functions (stubs — STREAMS not implemented)
// ---------------------------------------------------------------------------

/// Put a message onto a stream.
///
/// Returns -1 with `errno = ENOSYS` (STREAMS not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putmsg(
    _fd: i32,
    _ctlptr: *const u8,
    _dataptr: *const u8,
    _flags: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Put a priority-band message onto a stream.
///
/// Returns -1 with `errno = ENOSYS` (STREAMS not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putpmsg(
    _fd: i32,
    _ctlptr: *const u8,
    _dataptr: *const u8,
    _band: i32,
    _flags: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get a message from a stream.
///
/// Returns -1 with `errno = ENOSYS` (STREAMS not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getmsg(
    _fd: i32,
    _ctlptr: *mut u8,
    _dataptr: *mut u8,
    _flagsp: *mut i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get a priority-band message from a stream.
///
/// Returns -1 with `errno = ENOSYS` (STREAMS not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getpmsg(
    _fd: i32,
    _ctlptr: *mut u8,
    _dataptr: *mut u8,
    _bandp: *mut i32,
    _flagsp: *mut i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Test if a file descriptor is associated with a STREAMS device.
///
/// Returns 0 (not a STREAMS device — STREAMS not supported).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isastream(_fd: i32) -> i32 {
    0
}

/// Attach a STREAMS-based file descriptor to an object in the
/// filesystem name space.
///
/// Returns -1 with `errno = ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fattach(_fd: i32, _path: *const u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Detach a STREAMS-based file descriptor from the filesystem.
///
/// Returns -1 with `errno = ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdetach(_path: *const u8) -> i32 {
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

    // -----------------------------------------------------------------------
    // Flush flags
    // -----------------------------------------------------------------------

    #[test]
    fn test_flush_flags() {
        assert_eq!(FLUSHR, 0x01);
        assert_eq!(FLUSHW, 0x02);
        assert_eq!(FLUSHRW, FLUSHR | FLUSHW);
    }

    // -----------------------------------------------------------------------
    // Read options
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_options() {
        assert_eq!(RNORM, 0);
        assert_ne!(RMSGN, RMSGD);
    }

    // -----------------------------------------------------------------------
    // Function stubs
    // -----------------------------------------------------------------------

    #[test]
    fn test_putmsg_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = putmsg(-1, core::ptr::null(), core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_putpmsg_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = putpmsg(-1, core::ptr::null(), core::ptr::null(), 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_getmsg_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = getmsg(
            -1,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_getpmsg_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = getpmsg(
            -1,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_isastream_returns_zero() {
        // Nothing is a STREAMS device since we don't support STREAMS.
        assert_eq!(isastream(0), 0);
        assert_eq!(isastream(-1), 0);
        assert_eq!(isastream(999), 0);
    }

    #[test]
    fn test_fattach_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = fattach(0, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_fdetach_returns_enosys() {
        crate::errno::set_errno(0);
        let ret = fdetach(core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }
}
