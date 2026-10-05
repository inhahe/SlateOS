//! Terminal attributes as the C library reads and writes them: `tcgetattr`,
//! `tcsetattr`, and the four calls that read and set a `struct termios`'s
//! speeds.
//!
//! Through the C library rather than `ioctl (TCGETS)` by hand, because the
//! library is where the two ABIs meet: glibc's `tcsetattr` maps its three
//! `optional_actions` onto three different requests and its speed calls keep
//! `CBAUD` and `c_ispeed`/`c_ospeed` in step, and SlateOS's C library does the
//! same over its own syscalls (design-decisions §768). `stty` is the caller
//! this was written for: it reads the terminal, edits the record, writes it,
//! and reads it back to see whether every change took.
//!
//! The record is glibc's x86-64 `struct termios` -- 60 bytes, `NCCS` 32 --
//! which SlateOS's C library shares; the tests hold its size and every offset
//! to `posix`'s declaration.

/// The number of control characters, `NCCS`.
pub const NCCS: usize = 32;

/// `tcsetattr`: change the attributes at once.
pub const TCSANOW: i32 = 0;
/// `tcsetattr`: change them once the output written so far has been sent.
pub const TCSADRAIN: i32 = 1;
/// `tcsetattr`: as `TCSADRAIN`, and discard input not yet read.
pub const TCSAFLUSH: i32 = 2;

/// `struct termios`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Termios {
    /// Input modes.
    pub c_iflag: u32,
    /// Output modes.
    pub c_oflag: u32,
    /// Control modes, the speed bits (`CBAUD`) among them.
    pub c_cflag: u32,
    /// Local modes.
    pub c_lflag: u32,
    /// The line discipline.
    pub c_line: u8,
    /// The control characters, indexed by `VINTR` and the rest.
    pub c_cc: [u8; NCCS],
    /// The input speed, as `cfgetispeed` reads it on Linux.
    pub c_ispeed: u32,
    /// The output speed.
    pub c_ospeed: u32,
}

#[cfg(unix)]
mod sys {
    use super::Termios;

    unsafe extern "C" {
        pub fn tcgetattr(fd: i32, termios_p: *mut Termios) -> i32;
        pub fn tcsetattr(fd: i32, optional_actions: i32, termios_p: *const Termios) -> i32;
        pub fn cfgetispeed(termios_p: *const Termios) -> u32;
        pub fn cfgetospeed(termios_p: *const Termios) -> u32;
        pub fn cfsetispeed(termios_p: *mut Termios, speed: u32) -> i32;
        pub fn cfsetospeed(termios_p: *mut Termios, speed: u32) -> i32;
        // Variadic, as it is in C; see `pty.rs` on why one declaration serves
        // both our library and glibc.
        pub fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }

    /// `fcntl`: read the file status flags.
    pub const F_GETFL: i32 = 3;
    /// `fcntl`: set them.
    pub const F_SETFL: i32 = 4;
}

/// `tcgetattr(fd, &t)`: the attributes of the terminal on `fd`.
///
/// # Errors
///
/// `EBADF`, or `ENOTTY` when `fd` is not a terminal; [`ENOSYS`](crate::ENOSYS)
/// on a host with no C library of ours.
pub fn get_attr(fd: i32) -> Result<Termios, i32> {
    #[cfg(unix)]
    {
        let mut t = Termios::default();
        // SAFETY: `t` is a live, writable `struct termios` of the library's
        // layout (checked by the tests), written whole by the call.
        if unsafe { sys::tcgetattr(fd, &raw mut t) } == 0 {
            Ok(t)
        } else {
            Err(crate::last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(crate::ENOSYS)
    }
}

/// `tcsetattr(fd, when, t)`: give the terminal on `fd` the attributes `t`,
/// `when` being [`TCSANOW`], [`TCSADRAIN`] or [`TCSAFLUSH`].
///
/// Success means the library made *some* of the changes, which is POSIX's
/// contract: a caller that must know they all took reads the attributes back,
/// as `stty` does.
///
/// # Errors
///
/// `EBADF`, `ENOTTY`, `EINVAL` for an unknown `when`, `EINTR`;
/// [`ENOSYS`](crate::ENOSYS) on a host with no C library of ours.
pub fn set_attr(fd: i32, when: i32, t: &Termios) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `t` is a live `struct termios` of the library's layout, only
        // read by the call.
        if unsafe { sys::tcsetattr(fd, when, t) } == 0 {
            Ok(())
        } else {
            Err(crate::last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, when, t);
        Err(crate::ENOSYS)
    }
}

/// `cfgetispeed(t)`: the input speed, as a `B*` constant (`B38400`, …).
#[must_use]
pub fn input_speed(t: &Termios) -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: `t` is a live `struct termios`, only read.
        unsafe { sys::cfgetispeed(t) }
    }
    #[cfg(not(unix))]
    {
        // glibc 2.39's Linux arithmetic, for a host with no library to ask:
        // the speed bits of `c_cflag`, which the input speed shares.
        t.c_cflag & CBAUD
    }
}

/// `cfgetospeed(t)`: the output speed, as a `B*` constant.
#[must_use]
pub fn output_speed(t: &Termios) -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: `t` is a live `struct termios`, only read.
        unsafe { sys::cfgetospeed(t) }
    }
    #[cfg(not(unix))]
    {
        // glibc 2.39's: the speed bits of `c_cflag`.
        t.c_cflag & CBAUD
    }
}

/// `cfsetispeed(t, speed)`: set the input speed to the `B*` constant `speed`.
///
/// # Errors
///
/// `EINVAL` for a value that is not a speed; [`ENOSYS`](crate::ENOSYS) on a
/// host with no C library of ours.
pub fn set_input_speed(t: &mut Termios, speed: u32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `t` is a live, writable `struct termios`.
        if unsafe { sys::cfsetispeed(t, speed) } == 0 {
            Ok(())
        } else {
            Err(crate::last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (t, speed);
        Err(crate::ENOSYS)
    }
}

/// `cfsetospeed(t, speed)`: set the output speed to the `B*` constant `speed`.
///
/// # Errors
///
/// As [`set_input_speed`].
pub fn set_output_speed(t: &mut Termios, speed: u32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `t` is a live, writable `struct termios`.
        if unsafe { sys::cfsetospeed(t, speed) } == 0 {
            Ok(())
        } else {
            Err(crate::last_errno())
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (t, speed);
        Err(crate::ENOSYS)
    }
}

/// `CBAUD`: the speed bits of `c_cflag`, `CBAUDEX` -- the bit that selects
/// the speeds above 38400 -- among them.
pub const CBAUD: u32 = 0o010_017;

/// `O_NONBLOCK`, on Linux and in SlateOS's C library.
pub const O_NONBLOCK: i32 = 0o4000;

/// Make `fd` blocking again: `fcntl (fd, F_SETFL, fcntl (fd, F_GETFL) &
/// ~O_NONBLOCK)`.
///
/// The second half of opening a terminal the way `stty -F` does: with
/// `O_NONBLOCK`, so that a serial line with no carrier does not hold the open
/// forever, and then without it, so the line behaves as a terminal again.
///
/// # Errors
///
/// `EBADF`; [`ENOSYS`](crate::ENOSYS) on a host with no C library of ours.
pub fn clear_nonblocking(fd: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `F_GETFL` takes no third argument and reads no memory.
        let flags = unsafe { sys::fcntl(fd, sys::F_GETFL) };
        if flags < 0 {
            return Err(crate::last_errno());
        }
        // SAFETY: `F_SETFL` takes an `int` and reads no memory.
        if unsafe { sys::fcntl(fd, sys::F_SETFL, flags & !O_NONBLOCK) } < 0 {
            return Err(crate::last_errno());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(crate::ENOSYS)
    }
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it; the defensive lints exist for code that runs on a
    // user's data, which this is not.
    #![allow(clippy::indexing_slicing, clippy::expect_used)]

    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn the_record_is_the_c_librarys_field_for_field() {
        use posix::ioctl as p;
        assert_eq!(size_of::<Termios>(), size_of::<p::Termios>());
        assert_eq!(size_of::<Termios>(), 60);
        assert_eq!(offset_of!(Termios, c_line), offset_of!(p::Termios, c_line));
        assert_eq!(offset_of!(Termios, c_cc), offset_of!(p::Termios, c_cc));
        assert_eq!(
            offset_of!(Termios, c_ispeed),
            offset_of!(p::Termios, c_ispeed)
        );
        assert_eq!(
            offset_of!(Termios, c_ospeed),
            offset_of!(p::Termios, c_ospeed)
        );
    }

    #[test]
    fn the_constants_are_the_c_librarys() {
        assert_eq!(NCCS, posix::ioctl::NCCS);
        assert_eq!(TCSANOW, posix::ioctl::TCSANOW);
        assert_eq!(TCSADRAIN, posix::ioctl::TCSADRAIN);
        assert_eq!(TCSAFLUSH, posix::ioctl::TCSAFLUSH);
    }

    #[cfg(not(unix))]
    #[test]
    fn the_host_has_no_terminal() {
        assert_eq!(get_attr(0), Err(crate::ENOSYS));
        assert_eq!(
            set_attr(0, TCSANOW, &Termios::default()),
            Err(crate::ENOSYS)
        );
    }

    /// Real terminals, on a Linux host against glibc.
    #[cfg(unix)]
    mod on_a_real_terminal {
        use super::super::*;

        unsafe extern "C" {
            fn openpty(
                amaster: *mut i32,
                aslave: *mut i32,
                name: *mut u8,
                termp: *const u8,
                winp: *const u8,
            ) -> i32;
            fn close(fd: i32) -> i32;
            fn pipe(fds: *mut i32) -> i32;
        }

        /// A pty pair, closed on drop.
        struct Pty {
            master: i32,
            slave: i32,
        }

        impl Pty {
            fn new() -> Self {
                let (mut master, mut slave) = (-1, -1);
                // SAFETY: both out-pointers are live `i32`s; the name, the
                // attributes and the size are left to the library.
                let rc = unsafe {
                    openpty(
                        &raw mut master,
                        &raw mut slave,
                        core::ptr::null_mut(),
                        core::ptr::null(),
                        core::ptr::null(),
                    )
                };
                assert_eq!(rc, 0, "openpty");
                Pty { master, slave }
            }
        }

        impl Drop for Pty {
            fn drop(&mut self) {
                // SAFETY: descriptors this test owns, closed once.
                unsafe {
                    close(self.slave);
                    close(self.master);
                }
            }
        }

        /// What a change writes is what a read gives back.
        #[test]
        fn attributes_round_trip() {
            let pty = Pty::new();
            let mut t = get_attr(pty.slave).expect("tcgetattr");
            let echo = 0o000_010;
            t.c_lflag ^= echo;
            t.c_cc[0] = 0x07;
            set_attr(pty.slave, TCSANOW, &t).expect("tcsetattr");
            let back = get_attr(pty.slave).expect("tcgetattr again");
            assert_eq!(back.c_lflag & echo, t.c_lflag & echo);
            assert_eq!(back.c_cc[0], 0x07);
        }

        /// Each speed reads back as set -- and on glibc the two share
        /// `c_cflag`'s one speed field, so setting the output speed sets the
        /// input speed too. That sharing is what `stty`'s `check_speed`
        /// refuses to hide (`ispeed 9600 ospeed 38400` is "asymmetric").
        #[test]
        fn speeds_round_trip() {
            let pty = Pty::new();
            let mut t = get_attr(pty.slave).expect("tcgetattr");
            let b9600 = 0o000_015;
            let b115200 = 0o010_002;
            set_input_speed(&mut t, b9600).expect("cfsetispeed");
            assert_eq!(input_speed(&t), b9600);
            set_output_speed(&mut t, b115200).expect("cfsetospeed");
            assert_eq!(output_speed(&t), b115200);
            assert_eq!(input_speed(&t), b115200, "glibc's speeds share a field");
            assert_eq!(
                set_output_speed(&mut t, 0o7_777_777),
                Err(22),
                "not a speed"
            );
        }

        /// A pipe is not a terminal.
        #[test]
        fn a_pipe_has_no_attributes() {
            let mut fds = [-1i32; 2];
            // SAFETY: `fds` holds the two descriptors `pipe` writes.
            assert_eq!(unsafe { pipe(fds.as_mut_ptr()) }, 0);
            assert_eq!(get_attr(fds[0]), Err(25));
            assert_eq!(set_attr(fds[1], TCSANOW, &Termios::default()), Err(25));
            // SAFETY: descriptors this test owns, closed once.
            unsafe {
                close(fds[0]);
                close(fds[1]);
            }
        }
    }
}
