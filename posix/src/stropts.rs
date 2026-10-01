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
// ioctl commands for STREAMS: musl's numbers, `('S' << 8) | n`, which are
// glibc's -- what a program compiled against <stropts.h> passes. (Until
// 2026-09-30 seven of these and RMSGD/RMSGN were other numbers, so that
// I_NREAD here was I_SRDOPT there; nothing in the library used them, and
// scripts/check-libc-abi.py did not read this header.)
// ---------------------------------------------------------------------------

const SID: i32 = (b'S' as i32) << 8;

/// Size the top message.
pub const I_NREAD: i32 = SID | 1;
/// Push a STREAMS module.
pub const I_PUSH: i32 = SID | 2;
/// Pop a STREAMS module.
pub const I_POP: i32 = SID | 3;
/// Get the top module's name.
pub const I_LOOK: i32 = SID | 4;
/// Flush a STREAM.
pub const I_FLUSH: i32 = SID | 5;
/// Set the read mode.
pub const I_SRDOPT: i32 = SID | 6;
/// Get the read mode.
pub const I_GRDOPT: i32 = SID | 7;
/// Send a STREAMS `ioctl`.
pub const I_STR: i32 = SID | 8;
/// Ask for notification signals.
pub const I_SETSIG: i32 = SID | 9;
/// Retrieve the current notification signals.
pub const I_GETSIG: i32 = SID | 10;
/// Look for a STREAMS module.
pub const I_FIND: i32 = SID | 11;
/// Connect two STREAMs.
pub const I_LINK: i32 = SID | 12;
/// Disconnect two STREAMs.
pub const I_UNLINK: i32 = SID | 13;
/// Get a file descriptor sent with `I_SENDFD`.
pub const I_RECVFD: i32 = SID | 14;
/// Peek at the top message on a STREAM.
pub const I_PEEK: i32 = SID | 15;
/// Send implementation-defined information about another STREAM.
pub const I_FDINSERT: i32 = SID | 16;
/// Pass a file descriptor through a STREAMS pipe.
pub const I_SENDFD: i32 = SID | 17;
/// Set the write mode.
pub const I_SWROPT: i32 = SID | 19;
/// Get the write mode.
pub const I_GWROPT: i32 = SID | 20;
/// Get all the module names on a STREAM.
pub const I_LIST: i32 = SID | 21;
/// Persistently connect two STREAMs.
pub const I_PLINK: i32 = SID | 22;
/// Dismantle a persistent STREAMS link.
pub const I_PUNLINK: i32 = SID | 23;
/// Flush one band of a STREAM.
pub const I_FLUSHBAND: i32 = SID | 28;
/// See whether any message exists in a band.
pub const I_CKBAND: i32 = SID | 29;
/// Get the band of the top message on a STREAM.
pub const I_GETBAND: i32 = SID | 30;
/// Is the top message "marked"?
pub const I_ATMARK: i32 = SID | 31;
/// Set the close time delay.
pub const I_SETCLTIME: i32 = SID | 32;
/// Get the close time delay.
pub const I_GETCLTIME: i32 = SID | 33;
/// Is a band writable?
pub const I_CANPUT: i32 = SID | 34;

/// The size of the buffer `I_LOOK`'s argument points to, at least.
pub const FMNAMESZ: i32 = 8;

// ---------------------------------------------------------------------------
// I_FLUSH's argument
// ---------------------------------------------------------------------------

/// Flush the read queues.
pub const FLUSHR: i32 = 0x01;
/// Flush the write queues.
pub const FLUSHW: i32 = 0x02;
/// Flush the read and write queues.
pub const FLUSHRW: i32 = 0x03;
/// Flush one band only (`I_FLUSHBAND`).
pub const FLUSHBAND: i32 = 0x04;

// ---------------------------------------------------------------------------
// I_SETSIG's events
// ---------------------------------------------------------------------------

/// A message other than a high-priority one has arrived at the read queue.
pub const S_INPUT: i32 = 0x0001;
/// A high-priority message is on the read queue.
pub const S_HIPRI: i32 = 0x0002;
/// The write queue for normal data is no longer full.
pub const S_OUTPUT: i32 = 0x0004;
/// A STREAMS signal message holding SIGPOLL has reached the read queue.
pub const S_MSG: i32 = 0x0008;
/// An error has reached the STREAM head.
pub const S_ERROR: i32 = 0x0010;
/// A hangup has reached the STREAM head.
pub const S_HANGUP: i32 = 0x0020;
/// A normal (band 0) message has arrived at the read queue.
pub const S_RDNORM: i32 = 0x0040;
/// `S_OUTPUT`.
pub const S_WRNORM: i32 = S_OUTPUT;
/// A message of a band other than 0 has arrived at the read queue.
pub const S_RDBAND: i32 = 0x0080;
/// The write queue for a band other than 0 is no longer full.
pub const S_WRBAND: i32 = 0x0100;
/// With `S_RDBAND`: SIGURG rather than SIGPOLL.
pub const S_BANDURG: i32 = 0x0200;

// ---------------------------------------------------------------------------
// putmsg's flags, the read and write modes, I_ATMARK's, getmsg's
// ---------------------------------------------------------------------------

/// Send a high-priority message.
pub const RS_HIPRI: i32 = 0x01;

/// Byte-STREAM mode, the default.
pub const RNORM: i32 = 0x0000;
/// Message-discard mode.
pub const RMSGD: i32 = 0x0001;
/// Message-non-discard mode.
pub const RMSGN: i32 = 0x0002;
/// Deliver a message's control part as data.
pub const RPROTDAT: i32 = 0x0004;
/// Discard a message's control part, delivering its data.
pub const RPROTDIS: i32 = 0x0008;
/// Fail `read` with EBADMSG at a message with a control part.
pub const RPROTNORM: i32 = 0x0010;
/// The three `RPROT` modes' bits.
pub const RPROTMASK: i32 = 0x001C;

/// Send a zero-length message downstream for a `write` of 0 bytes.
pub const SNDZERO: i32 = 0x001;
/// SIGPIPE for a `write` to a STREAM with no reader.
pub const SNDPIPE: i32 = 0x002;

/// Is the message marked?
pub const ANYMARK: i32 = 0x01;
/// Is it the last one marked on the queue?
pub const LASTMARK: i32 = 0x02;

/// Unlink every STREAM linked to this one.
pub const MUXID_ALL: i32 = -1;

/// Receive a high-priority message.
pub const MSG_HIPRI: i32 = 0x01;
/// Receive any message.
pub const MSG_ANY: i32 = 0x02;
/// Receive a message from the given band.
pub const MSG_BAND: i32 = 0x04;

/// `getmsg`: more control information is left in the message.
pub const MORECTL: i32 = 1;
/// `getmsg`: more data is left in the message.
pub const MOREDATA: i32 = 2;

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

    /// musl's numbers, `('S' << 8) | n` -- glibc's too -- for all 29, and
    /// so distinct. (scripts/check-libc-abi.py holds each to the header as
    /// well.)
    #[test]
    fn test_ioctl_commands_are_musls() {
        let table = [
            (I_NREAD, 1),
            (I_PUSH, 2),
            (I_POP, 3),
            (I_LOOK, 4),
            (I_FLUSH, 5),
            (I_SRDOPT, 6),
            (I_GRDOPT, 7),
            (I_STR, 8),
            (I_SETSIG, 9),
            (I_GETSIG, 10),
            (I_FIND, 11),
            (I_LINK, 12),
            (I_UNLINK, 13),
            (I_RECVFD, 14),
            (I_PEEK, 15),
            (I_FDINSERT, 16),
            (I_SENDFD, 17),
            (I_SWROPT, 19),
            (I_GWROPT, 20),
            (I_LIST, 21),
            (I_PLINK, 22),
            (I_PUNLINK, 23),
            (I_FLUSHBAND, 28),
            (I_CKBAND, 29),
            (I_GETBAND, 30),
            (I_ATMARK, 31),
            (I_SETCLTIME, 32),
            (I_GETCLTIME, 33),
            (I_CANPUT, 34),
        ];
        for (cmd, n) in table {
            assert_eq!(cmd, 0x5300 | n);
        }
        // The one a program most often asks, which was this library's
        // I_SRDOPT until 2026-09-30.
        assert_eq!(I_NREAD, 21249);
    }

    #[test]
    fn test_flush_flags() {
        assert_eq!(FLUSHR, 0x01);
        assert_eq!(FLUSHW, 0x02);
        assert_eq!(FLUSHRW, FLUSHR | FLUSHW);
        assert_eq!(FLUSHBAND, 0x04);
    }

    #[test]
    fn test_read_options() {
        assert_eq!((RNORM, RMSGD, RMSGN), (0, 1, 2));
        assert_eq!(RPROTMASK, RPROTDAT | RPROTDIS | RPROTNORM);
    }

    #[test]
    fn test_events() {
        assert_eq!(S_WRNORM, S_OUTPUT);
        let all = [
            S_INPUT, S_HIPRI, S_OUTPUT, S_MSG, S_ERROR, S_HANGUP, S_RDNORM, S_RDBAND, S_WRBAND,
            S_BANDURG,
        ];
        assert_eq!(all.iter().fold(0, |a, b| a | b), 0x3FF, "one bit each");
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
