//! What a getty asks of a terminal line beyond its attributes: whose session
//! it belongs to, making it a controlling terminal and giving it up, hanging
//! the line up, its foreground group, and -- on a virtual console -- the
//! keyboard's mode and lights. And the group a line is handed to (`tty`).
//!
//! The request numbers are Linux's on x86-64, which SlateOS's C library
//! answers to as well (design-decisions §768).

/// `KDGKBMODE`'s answers: the keyboard sends scancodes as they come.
pub const K_RAW: i32 = 0;
/// Characters, through the keymap: an 8-bit console.
pub const K_XLATE: i32 = 1;
/// Keycodes.
pub const K_MEDIUMRAW: i32 = 2;
/// Characters, as UTF-8: a Unicode console.
pub const K_UNICODE: i32 = 3;

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn tcgetsid(fd: i32) -> i32;
        pub fn tcsetpgrp(fd: i32, pgrp: i32) -> i32;
        pub fn vhangup() -> i32;
        // Variadic, as C declares it; see `pty.rs` on why one declaration
        // serves both our library and glibc.
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
        pub fn getgrnam(name: *const u8) -> *const Group;
    }

    /// `struct group`, as far as `gr_gid`: two pointers before it.
    #[repr(C)]
    pub struct Group {
        pub gr_name: *const u8,
        pub gr_passwd: *const u8,
        pub gr_gid: u32,
    }

    /// Make the terminal the caller's controlling terminal.
    pub const TIOCSCTTY: u64 = 0x540E;
    /// Give up the controlling terminal.
    pub const TIOCNOTTY: u64 = 0x5422;
    /// A virtual console's keyboard mode.
    pub const KDGKBMODE: u64 = 0x4B44;
    /// A virtual console's keyboard flags: its lights' states and their
    /// defaults.
    pub const KDGKBLED: u64 = 0x4B64;
}

/// `tcgetsid (fd)`: the session whose controlling terminal is on `fd`.
///
/// # Errors
///
/// `EBADF`, or `ENOTTY` when it is no session's controlling terminal;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn session_of(fd: i32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: one number in, one out; no memory of ours.
        let sid = unsafe { sys::tcgetsid(fd) };
        if sid < 0 {
            Err(crate::last_errno())
        } else {
            Ok(sid)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(crate::ENOSYS)
    }
}

/// `ioctl (fd, TIOCSCTTY, steal)`: make the terminal on `fd` this session
/// leader's controlling terminal -- taking it from another session, if
/// `steal` and the caller may.
///
/// # Errors
///
/// `EPERM` when the caller leads no session, or the terminal is another
/// session's and may not be taken; `ENOTTY`, `EBADF`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn make_controlling(fd: i32, steal: bool) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: the request takes an integer argument, not a pointer.
        let rc = unsafe { sys::ioctl(fd, sys::TIOCSCTTY, i32::from(steal)) };
        if rc == -1 {
            Err(crate::last_errno())
        } else {
            Ok(())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, steal);
        Err(crate::ENOSYS)
    }
}

/// `ioctl (fd, TIOCNOTTY)`: give up the controlling terminal on `fd`.
///
/// # Errors
///
/// `ENOTTY` when it is not the caller's controlling terminal, `EBADF`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn give_up_controlling(fd: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: the request takes no argument.
        let rc = unsafe { sys::ioctl(fd, sys::TIOCNOTTY) };
        if rc == -1 {
            Err(crate::last_errno())
        } else {
            Ok(())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(crate::ENOSYS)
    }
}

/// `vhangup ()`: hang up the caller's controlling terminal, so that every
/// other process holding it loses it.
///
/// # Errors
///
/// `EPERM` without the privilege; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn hang_up() -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: no arguments.
        if unsafe { sys::vhangup() } == 0 {
            Ok(())
        } else {
            Err(crate::last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        Err(crate::ENOSYS)
    }
}

/// `tcsetpgrp (fd, pgrp)`: make `pgrp` the foreground group of the terminal
/// on `fd`.
///
/// # Errors
///
/// `ENOTTY` when it is not the caller's controlling terminal, `EPERM` for a
/// group in another session, `EINVAL`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn set_foreground_group(fd: i32, pgrp: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: two numbers in; no memory of ours.
        if unsafe { sys::tcsetpgrp(fd, pgrp) } == 0 {
            Ok(())
        } else {
            Err(crate::last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, pgrp);
        Err(crate::ENOSYS)
    }
}

/// `ioctl (fd, KDGKBMODE, &mode)`: the keyboard mode of the virtual console
/// on `fd` -- [`K_UNICODE`] and the rest. Only a virtual console answers,
/// which is how a getty tells one from a serial line.
///
/// # Errors
///
/// `ENOTTY` or `EINVAL` for a terminal that is not a virtual console,
/// `EBADF`; [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn keyboard_mode(fd: i32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        let mut mode: i32 = 0;
        // SAFETY: the kernel writes one `int` through the pointer, which
        // points at `mode`, alive across the call.
        let rc = unsafe { sys::ioctl(fd, sys::KDGKBMODE, &raw mut mode) };
        if rc == -1 {
            Err(crate::last_errno())
        } else {
            Ok(mode)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(crate::ENOSYS)
    }
}

/// `ioctl (fd, KDGKBLED, &flags)`: a virtual console's keyboard flags -- the
/// lights' states in the low three bits (Scroll, Num, Caps Lock) and their
/// defaults in the three above.
///
/// # Errors
///
/// As [`keyboard_mode`].
pub fn keyboard_leds(fd: i32) -> Result<u8, i32> {
    #[cfg(unix)]
    {
        let mut flags: u8 = 0;
        // SAFETY: the kernel writes one `char` through the pointer, which
        // points at `flags`, alive across the call.
        let rc = unsafe { sys::ioctl(fd, sys::KDGKBLED, &raw mut flags) };
        if rc == -1 {
            Err(crate::last_errno())
        } else {
            Ok(flags)
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(crate::ENOSYS)
    }
}

/// `getgrnam (name)->gr_gid`: the group called `name`, `None` when there is
/// none (or the group database could not be read).
#[must_use]
pub fn group_id(name: &core::ffi::CStr) -> Option<u32> {
    #[cfg(unix)]
    {
        // SAFETY: `name` is a NUL-terminated string the call only reads; the
        // result is the library's own record, read at once and not kept.
        let group = unsafe { sys::getgrnam(name.as_ptr().cast()) };
        if group.is_null() {
            return None;
        }
        // SAFETY: a non-null result points at a `struct group` that stays
        // valid until the next group lookup, and this reads one field of it.
        Some(unsafe { (*group).gr_gid })
    }
    #[cfg(not(unix))]
    {
        let _ = name;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The group record's layout is the C library's, as far as `gr_gid`.
    #[test]
    fn a_group_s_id_follows_two_pointers() {
        #[cfg(unix)]
        assert_eq!(core::mem::offset_of!(sys::Group, gr_gid), 16);
        assert_eq!(core::mem::offset_of!(posix::pwd::Group, gr_gid), 16);
    }

    /// Something that is no terminal answers as one that is not.
    #[cfg(unix)]
    #[test]
    fn a_pipe_is_no_terminal() {
        let pipe = crate::fd::pipe();
        assert!(pipe.is_ok(), "{pipe:?}");
        let Ok((r, w)) = pipe else { return };
        assert!(session_of(r).is_err());
        assert!(keyboard_mode(r).is_err());
        assert!(keyboard_leds(r).is_err());
        assert!(make_controlling(r, false).is_err());
        assert!(give_up_controlling(r).is_err());
        assert!(set_foreground_group(r, 1).is_err());
        assert!(crate::termios::flush(r, crate::termios::TCIOFLUSH).is_err());
        // `hang_up` is not run: on a test run from a terminal, with the
        // privilege, it would hang up the terminal running the tests.
        let _ = crate::fd::close(r);
        let _ = crate::fd::close(w);
    }

    /// Off Unix every call declines -- `hang_up` among them, which nothing
    /// can safely run on a real system's terminal.
    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        assert_eq!(session_of(0), Err(crate::ENOSYS));
        assert_eq!(make_controlling(0, true), Err(crate::ENOSYS));
        assert_eq!(give_up_controlling(0), Err(crate::ENOSYS));
        assert_eq!(hang_up(), Err(crate::ENOSYS));
        assert_eq!(set_foreground_group(0, 1), Err(crate::ENOSYS));
        assert_eq!(keyboard_mode(0), Err(crate::ENOSYS));
        assert_eq!(keyboard_leds(0), Err(crate::ENOSYS));
        assert_eq!(group_id(c"root"), None);
    }

    #[cfg(unix)]
    #[test]
    fn root_has_a_group() {
        assert_eq!(group_id(c"root"), Some(0));
        assert_eq!(group_id(c"no-such-group-at-all"), None);
    }
}
