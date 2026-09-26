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
//! Linux has never supported STREAMS, and glibc's calls are stubs --
//! compat symbols in glibc 2.39's posix/streams-compat.c, since 2.30
//! removed `<stropts.h>` from the API.  `putmsg`, `putpmsg`, `getmsg`,
//! `getpmsg`, `fattach` and `fdetach` set ENOSYS and return -1 whatever
//! their arguments; `isastream` is 0 for an open descriptor and -1 with
//! EBADF (from `fcntl (fd, F_GETFD)`) for one that is not.  This OS will not
//! implement STREAMS either -- it has Channel IPC, io_uring and sockets --
//! so these calls are glibc's, exactly.
//!
//! Until 2026-09-26 the six validated their arguments first -- EBADF for a
//! negative descriptor, EFAULT for a NULL pointer, ENOENT for an empty path,
//! EINVAL for flags and bands -- "so probing callers' fallback paths fire".
//! They did the opposite.  A program that probes for STREAMS with
//! placeholder arguments, such as `putmsg (-1, NULL, NULL, 0)`, is looking
//! for ENOSYS, and was told EBADF instead: that the call exists and only
//! its descriptor was wrong.  `isastream` had the reverse fault, calling a
//! closed descriptor "not a stream" where glibc says EBADF.

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
// Functions
// ---------------------------------------------------------------------------

/// Put a message onto a stream: ENOSYS, whatever the arguments.
///
/// glibc's is `__set_errno (ENOSYS); return -1;` (posix/streams-compat.c);
/// see the module docs for why nothing is validated first.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn putmsg(_fd: i32, _ctlptr: *const u8, _dataptr: *const u8, _flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Put a priority-band message onto a stream: ENOSYS, whatever the
/// arguments, as glibc's (posix/streams-compat.c).
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

/// Get a message from a stream: ENOSYS, whatever the arguments, as
/// glibc's (posix/streams-compat.c).  Nothing is read or written through
/// the pointers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getmsg(_fd: i32, _ctlptr: *mut u8, _dataptr: *mut u8, _flagsp: *mut i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get a priority-band message from a stream: ENOSYS, whatever the
/// arguments, as glibc's (posix/streams-compat.c).  Nothing is read or
/// written through the pointers.
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

/// Is `fd` a STREAMS device?  0 -- no -- for an open descriptor, and -1
/// with EBADF for one that is not open.
///
/// glibc's is `if (__fcntl (fildes, F_GETFD) < 0) return -1; return 0;`
/// (posix/streams-compat.c), so *any* descriptor that is not open is EBADF,
/// and an `O_PATH` one, which `F_GETFD` accepts, is 0.  Until 2026-09-26
/// only a negative descriptor was EBADF.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isastream(fd: i32) -> i32 {
    if crate::fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    0
}

/// Attach a STREAMS-based file descriptor to an object in the filesystem
/// name space: ENOSYS, whatever the arguments, as glibc's
/// (posix/streams-compat.c).  Until 2026-09-26 a probe such as
/// `fattach (-1, "")` was told EBADF.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fattach(_fd: i32, _path: *const u8) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Detach a STREAMS-based file descriptor from a filesystem name: ENOSYS,
/// whatever the argument, as glibc's (posix/streams-compat.c).
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
            I_PUSH, I_POP, I_LOOK, I_FLUSH, I_STR, I_SRDOPT, I_GRDOPT, I_SENDFD, I_RECVFD, I_FIND,
            I_LINK, I_UNLINK, I_NREAD, I_PEEK, I_FDINSERT, I_SETSIG, I_GETSIG, I_CANPUT, I_PLINK,
            I_PUNLINK,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j], "STREAMS ioctl commands must be distinct");
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

    // -----------------------------------------------------------------------
    // The six stubs: ENOSYS, whatever the arguments
    // -----------------------------------------------------------------------

    /// Run `call` with errno preset to something else and check that it
    /// failed with ENOSYS -- set, not merely left over.
    fn expect_enosys(what: &str, call: impl FnOnce() -> i32) {
        errno::set_errno(errno::EBADF);
        assert_eq!(call(), -1, "{what}");
        assert_eq!(errno::get_errno(), errno::ENOSYS, "{what}");
    }

    /// Every shape that had its own EBADF or EINVAL until 2026-09-26 is
    /// ENOSYS now, as in glibc -- above all the placeholder probe.
    #[test]
    fn test_putmsg_is_enosys_whatever_the_arguments() {
        let part = [0u8; 8];
        let (p, null) = (part.as_ptr(), core::ptr::null());
        expect_enosys("well formed", || putmsg(5, p, p, 0));
        expect_enosys("a probe's placeholders", || putmsg(-1, null, null, 0));
        expect_enosys("negative fd", || putmsg(-1, p, null, 0));
        expect_enosys("INT_MIN fd", || putmsg(i32::MIN, p, null, 0));
        expect_enosys("no part at all", || putmsg(5, null, null, 0));
        expect_enosys("unknown flag", || putmsg(5, p, null, 0x40));
        expect_enosys("high priority", || putmsg(5, null, p, RS_HIPRI));
    }

    #[test]
    fn test_putpmsg_is_enosys_whatever_the_arguments() {
        let part = [0u8; 8];
        let (p, null) = (part.as_ptr(), core::ptr::null());
        expect_enosys("well formed", || putpmsg(5, p, null, 3, MSG_BAND));
        expect_enosys("negative fd", || putpmsg(-1, p, null, 0, MSG_BAND));
        expect_enosys("no part at all", || putpmsg(5, null, null, 0, MSG_BAND));
        expect_enosys("negative band", || putpmsg(5, p, null, -1, MSG_BAND));
        expect_enosys("band 256", || putpmsg(5, p, null, 256, MSG_BAND));
        expect_enosys("unknown flag", || putpmsg(5, p, null, 0, 0x40));
        expect_enosys("high priority with a band", || {
            putpmsg(5, p, null, 1, MSG_HIPRI)
        });
    }

    #[test]
    fn test_getmsg_is_enosys_whatever_the_arguments() {
        let null = core::ptr::null_mut();
        let mut flags: i32 = 0;
        expect_enosys("well formed", || getmsg(5, null, null, &raw mut flags));
        expect_enosys("negative fd", || getmsg(-1, null, null, &raw mut flags));
        expect_enosys("NULL flagsp", || {
            getmsg(5, null, null, core::ptr::null_mut())
        });
        assert_eq!(flags, 0, "nothing is written through flagsp");
    }

    #[test]
    fn test_getpmsg_is_enosys_whatever_the_arguments() {
        let null = core::ptr::null_mut();
        let mut band: i32 = 7;
        let mut flags: i32 = MSG_ANY;
        expect_enosys("well formed", || {
            getpmsg(5, null, null, &raw mut band, &raw mut flags)
        });
        expect_enosys("negative fd", || {
            getpmsg(-1, null, null, &raw mut band, &raw mut flags)
        });
        expect_enosys("NULL bandp", || {
            getpmsg(5, null, null, core::ptr::null_mut(), &raw mut flags)
        });
        expect_enosys("NULL flagsp", || {
            getpmsg(5, null, null, &raw mut band, core::ptr::null_mut())
        });
        let mut unknown: i32 = 0x80;
        expect_enosys("unknown flag", || {
            getpmsg(5, null, null, &raw mut band, &raw mut unknown)
        });
        assert_eq!(
            (band, flags, unknown),
            (7, MSG_ANY, 0x80),
            "nothing is written"
        );
    }

    #[test]
    fn test_fattach_and_fdetach_are_enosys_whatever_the_arguments() {
        let path = b"/var/run/streams/svc\0".as_ptr();
        let (empty, null) = (b"\0".as_ptr(), core::ptr::null());
        expect_enosys("fattach, well formed", || fattach(5, path));
        expect_enosys("fattach, negative fd", || fattach(-1, path));
        expect_enosys("fattach, NULL path", || fattach(5, null));
        expect_enosys("fattach, empty path", || fattach(5, empty));
        expect_enosys("fattach, a probe's placeholders", || fattach(-1, empty));
        expect_enosys("fdetach, well formed", || fdetach(path));
        expect_enosys("fdetach, NULL path", || fdetach(null));
        expect_enosys("fdetach, empty path", || fdetach(empty));
    }

    // -----------------------------------------------------------------------
    // isastream
    // -----------------------------------------------------------------------

    /// An open descriptor is not a STREAMS device: 0, and errno untouched.
    #[test]
    fn test_isastream_open_descriptor_is_zero() {
        let fd = crate::fdtable::alloc_fd(crate::fdtable::HandleKind::File, 0x57AE)
            .expect("a free descriptor");
        errno::set_errno(0);
        assert_eq!(isastream(fd), 0);
        assert_eq!(errno::get_errno(), 0);
        let _ = crate::fdtable::close_fd(fd);
        assert_eq!(isastream(fd), -1, "closed again");
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    /// Any descriptor that is not open is EBADF -- not only a negative one.
    /// 999 was "not a stream" until 2026-09-26.
    #[test]
    fn test_isastream_descriptor_not_open_ebadf() {
        for fd in [-1, i32::MIN, 999] {
            errno::set_errno(0);
            assert_eq!(isastream(fd), -1, "fd {fd}");
            assert_eq!(errno::get_errno(), errno::EBADF, "fd {fd}");
        }
    }

    // -----------------------------------------------------------------------
    // Callers
    // -----------------------------------------------------------------------

    #[test]
    fn test_solaris_port_streams_probe_workflow() {
        // A Solaris-to-Linux port of a network daemon (TLI/XTI users)
        // calls putmsg(fd, &ctl, &data, 0) at startup; on ENOSYS its
        // fallback disables the STREAMS code path and uses sockets.
        let ctl_buf = [0u8; 32];
        let data_buf = [0u8; 256];
        expect_enosys("putmsg", || {
            putmsg(5, ctl_buf.as_ptr(), data_buf.as_ptr(), 0)
        });
    }

    #[test]
    fn test_tli_compat_getpmsg_workflow() {
        // A TLI compatibility layer reads T_DATA_IND messages with
        // getpmsg(fd, &ctl, &data, &band, &flag), flag = MSG_ANY; on ENOSYS
        // it falls back to BSD sockets.
        let mut band: i32 = 0;
        let mut flag: i32 = MSG_ANY;
        let null = core::ptr::null_mut();
        expect_enosys("getpmsg", || {
            getpmsg(7, null, null, &raw mut band, &raw mut flag)
        });
    }

    #[test]
    fn test_fattach_streams_pipe_mount_workflow() {
        // Solaris-style "mount a pipe at a path" via fattach; on ENOSYS the
        // caller binds a UNIX-domain socket at the path instead.
        let path = b"/var/run/streams/autofs\0";
        expect_enosys("fattach", || fattach(5, path.as_ptr()));
    }
}
