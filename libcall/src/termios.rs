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
        pub fn ttyname_r(fd: i32, buf: *mut u8, buflen: usize) -> i32;
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

// The control characters' slots in `c_cc`.

/// `VINTR`: interrupt (`^C`).
pub const VINTR: usize = 0;
/// `VQUIT`: quit (`^\`).
pub const VQUIT: usize = 1;
/// `VERASE`: erase a character.
pub const VERASE: usize = 2;
/// `VKILL`: erase the line.
pub const VKILL: usize = 3;
/// `VEOF`: end of file (`^D`).
pub const VEOF: usize = 4;
/// `VTIME`: the non-canonical read timeout.
pub const VTIME: usize = 5;
/// `VMIN`: the non-canonical read minimum.
pub const VMIN: usize = 6;
/// `VSWTC`: switch (unused on Linux).
pub const VSWTC: usize = 7;
/// `VSTART`: resume output (`^Q`).
pub const VSTART: usize = 8;
/// `VSTOP`: stop output (`^S`).
pub const VSTOP: usize = 9;
/// `VSUSP`: suspend (`^Z`).
pub const VSUSP: usize = 10;
/// `VEOL`: an extra end of line.
pub const VEOL: usize = 11;
/// `VREPRINT`: reprint the line (`^R`).
pub const VREPRINT: usize = 12;
/// `VDISCARD`: discard output (`^O`).
pub const VDISCARD: usize = 13;
/// `VWERASE`: erase a word (`^W`).
pub const VWERASE: usize = 14;
/// `VLNEXT`: take the next character literally (`^V`).
pub const VLNEXT: usize = 15;
/// `VEOL2`: a second extra end of line.
pub const VEOL2: usize = 16;

// `c_iflag`.

/// `IGNBRK`.
pub const IGNBRK: u32 = 0o1;
/// `BRKINT`.
pub const BRKINT: u32 = 0o2;
/// `IGNPAR`.
pub const IGNPAR: u32 = 0o4;
/// `PARMRK`.
pub const PARMRK: u32 = 0o10;
/// `INPCK`.
pub const INPCK: u32 = 0o20;
/// `ISTRIP`.
pub const ISTRIP: u32 = 0o40;
/// `INLCR`.
pub const INLCR: u32 = 0o100;
/// `IGNCR`.
pub const IGNCR: u32 = 0o200;
/// `ICRNL`.
pub const ICRNL: u32 = 0o400;
/// `IUCLC`.
pub const IUCLC: u32 = 0o1000;
/// `IXON`.
pub const IXON: u32 = 0o2000;
/// `IXANY`.
pub const IXANY: u32 = 0o4000;
/// `IXOFF`.
pub const IXOFF: u32 = 0o10000;
/// `IMAXBEL`.
pub const IMAXBEL: u32 = 0o20000;
/// `IUTF8`.
pub const IUTF8: u32 = 0o40000;

// `c_oflag`.

/// `OPOST`.
pub const OPOST: u32 = 0o1;
/// `OLCUC`.
pub const OLCUC: u32 = 0o2;
/// `ONLCR`.
pub const ONLCR: u32 = 0o4;
/// `OCRNL`.
pub const OCRNL: u32 = 0o10;
/// `ONOCR`.
pub const ONOCR: u32 = 0o20;
/// `ONLRET`.
pub const ONLRET: u32 = 0o40;
/// `OFILL`.
pub const OFILL: u32 = 0o100;
/// `OFDEL`.
pub const OFDEL: u32 = 0o200;
/// `NLDLY`.
pub const NLDLY: u32 = 0o400;
/// `CRDLY`.
pub const CRDLY: u32 = 0o3000;
/// `TABDLY`.
pub const TABDLY: u32 = 0o14000;
/// `TAB3`: tabs expanded to spaces (`XTABS`).
pub const TAB3: u32 = 0o14000;
/// `BSDLY`.
pub const BSDLY: u32 = 0o20000;
/// `VTDLY`.
pub const VTDLY: u32 = 0o40000;
/// `FFDLY`.
pub const FFDLY: u32 = 0o100_000;

// `c_cflag`.

/// `CSIZE`: the character-size bits.
pub const CSIZE: u32 = 0o60;
/// `CS8`.
pub const CS8: u32 = 0o60;
/// `CSTOPB`.
pub const CSTOPB: u32 = 0o100;
/// `CREAD`.
pub const CREAD: u32 = 0o200;
/// `PARENB`.
pub const PARENB: u32 = 0o400;
/// `PARODD`.
pub const PARODD: u32 = 0o1000;
/// `HUPCL`.
pub const HUPCL: u32 = 0o2000;
/// `CLOCAL`.
pub const CLOCAL: u32 = 0o4000;

// `c_lflag`.

/// `ISIG`.
pub const ISIG: u32 = 0o1;
/// `ICANON`.
pub const ICANON: u32 = 0o2;
/// `XCASE`.
pub const XCASE: u32 = 0o4;
/// `ECHO`.
pub const ECHO: u32 = 0o10;
/// `ECHOE`.
pub const ECHOE: u32 = 0o20;
/// `ECHOK`.
pub const ECHOK: u32 = 0o40;
/// `ECHONL`.
pub const ECHONL: u32 = 0o100;
/// `NOFLSH`.
pub const NOFLSH: u32 = 0o200;
/// `TOSTOP`.
pub const TOSTOP: u32 = 0o400;
/// `ECHOCTL`.
pub const ECHOCTL: u32 = 0o1000;
/// `ECHOPRT`.
pub const ECHOPRT: u32 = 0o2000;
/// `ECHOKE`.
pub const ECHOKE: u32 = 0o4000;
/// `FLUSHO`.
pub const FLUSHO: u32 = 0o10000;
/// `PENDIN`.
pub const PENDIN: u32 = 0o40000;
/// `IEXTEN`.
pub const IEXTEN: u32 = 0o100_000;

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

/// `fcntl (fd, F_GETFL)`: the file status flags of `fd`, `O_NONBLOCK` among
/// them.
///
/// # Errors
///
/// `EBADF`; [`ENOSYS`](crate::ENOSYS) on a host with no C library of ours.
pub fn status_flags(fd: i32) -> Result<i32, i32> {
    #[cfg(unix)]
    {
        // SAFETY: `F_GETFL` takes no third argument and reads no memory.
        let flags = unsafe { sys::fcntl(fd, sys::F_GETFL) };
        if flags < 0 {
            return Err(crate::last_errno());
        }
        Ok(flags)
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(crate::ENOSYS)
    }
}

/// `fcntl (fd, F_SETFL, flags)`: set `fd`'s file status flags.
///
/// Separate from [`clear_nonblocking`] for the caller that must make the
/// call exactly as written somewhere else -- `wall`'s `ttymsg`, which asks
/// the flags of one descriptor and sets them on another.
///
/// # Errors
///
/// `EBADF`; [`ENOSYS`](crate::ENOSYS) on a host with no C library of ours.
pub fn set_status_flags(fd: i32, flags: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        // SAFETY: `F_SETFL` takes an `int` and reads no memory.
        if unsafe { sys::fcntl(fd, sys::F_SETFL, flags) } < 0 {
            return Err(crate::last_errno());
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, flags);
        Err(crate::ENOSYS)
    }
}

/// `ttyname_r (fd, buf, buf.len ())`: the path of the terminal on `fd`,
/// written into `buf` with its terminator, and its length without it.
///
/// # Errors
///
/// `ENOTTY` when `fd` is not a terminal, `EBADF`, and `ERANGE` when `buf`
/// is too short for the path; [`ENOSYS`](crate::ENOSYS) on a host with no C
/// library of ours.
pub fn ttyname_into(fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
    #[cfg(unix)]
    {
        // SAFETY: `buf` is ours, writable for its whole length, which is
        // what the call is told; it writes the path and a NUL within it or
        // fails with `ERANGE`.
        let rc = unsafe { sys::ttyname_r(fd, buf.as_mut_ptr(), buf.len()) };
        if rc != 0 {
            return Err(rc);
        }
        Ok(buf.iter().position(|&b| b == 0).unwrap_or(buf.len()))
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, buf);
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
        use posix::ioctl as p;
        assert_eq!(NCCS, p::NCCS);
        assert_eq!(TCSANOW, p::TCSANOW);
        assert_eq!(TCSADRAIN, p::TCSADRAIN);
        assert_eq!(TCSAFLUSH, p::TCSAFLUSH);
        // Every flag and slot the C library defines too.
        for (ours, theirs) in [
            (BRKINT, p::BRKINT),
            (INPCK, p::INPCK),
            (ISTRIP, p::ISTRIP),
            (INLCR, p::INLCR),
            (IGNCR, p::IGNCR),
            (ICRNL, p::ICRNL),
            (IXON, p::IXON),
            (OPOST, p::OPOST),
            (ONLCR, p::ONLCR),
            (CSIZE, p::CSIZE),
            (CS8, p::CS8),
            (CREAD, p::CREAD),
            (PARENB, p::PARENB),
            (HUPCL, p::HUPCL),
            (CLOCAL, p::CLOCAL),
            (ISIG, p::ISIG),
            (ICANON, p::ICANON),
            (ECHO, p::ECHO),
            (ECHONL, p::ECHONL),
            (IEXTEN, p::IEXTEN),
        ] {
            assert_eq!(ours, theirs);
        }
        for (ours, theirs) in [
            (VINTR, p::VINTR),
            (VQUIT, p::VQUIT),
            (VERASE, p::VERASE),
            (VKILL, p::VKILL),
            (VEOF, p::VEOF),
            (VTIME, p::VTIME),
            (VMIN, p::VMIN),
            (VSTART, p::VSTART),
            (VSTOP, p::VSTOP),
            (VSUSP, p::VSUSP),
            (VEOL, p::VEOL),
        ] {
            assert_eq!(ours, theirs);
        }
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
            // ...and has no name as one: `ENOTTY`.
            let mut buf = [0u8; 64];
            assert_eq!(ttyname_into(fds[0], &mut buf), Err(25));
            // SAFETY: descriptors this test owns, closed once.
            unsafe {
                close(fds[0]);
                close(fds[1]);
            }
        }

        /// A pseudo-terminal has no modem lines, and a pipe is no terminal:
        /// both are errors, which is all `reset` asks.
        #[test]
        fn a_pty_has_no_modem_lines() {
            let pty = Pty::new();
            assert!(crate::pty::modem_bits(pty.slave).is_err());
            let mut fds = [-1i32; 2];
            // SAFETY: `fds` holds the two descriptors `pipe` writes.
            assert_eq!(unsafe { pipe(fds.as_mut_ptr()) }, 0);
            assert_eq!(crate::pty::modem_bits(fds[0]), Err(25));
            // SAFETY: descriptors this test owns, closed once.
            unsafe {
                close(fds[0]);
                close(fds[1]);
            }
        }

        /// A terminal is named by its path under `/dev/pts`, and a buffer too
        /// short for it is `ERANGE`.
        #[test]
        fn a_terminal_has_a_name() {
            let pty = Pty::new();
            let mut buf = [0u8; 4096];
            let n = ttyname_into(pty.slave, &mut buf).expect("ttyname_r");
            assert!(buf[..n].starts_with(b"/dev/pts/"), "{:?}", &buf[..n]);
            assert_eq!(buf[n], 0, "the name is NUL-terminated");
            let mut short = [0u8; 4];
            assert_eq!(ttyname_into(pty.slave, &mut short), Err(34));
        }

        /// The status flags read back as set, `O_NONBLOCK` among them; a
        /// descriptor that is not open is `EBADF` both ways.
        #[test]
        fn status_flags_round_trip() {
            let mut fds = [-1i32; 2];
            // SAFETY: `fds` holds the two descriptors `pipe` writes.
            assert_eq!(unsafe { pipe(fds.as_mut_ptr()) }, 0);
            let flags = status_flags(fds[0]).expect("F_GETFL");
            assert_eq!(flags & O_NONBLOCK, 0);
            set_status_flags(fds[0], flags | O_NONBLOCK).expect("F_SETFL");
            assert_eq!(
                status_flags(fds[0]).expect("F_GETFL") & O_NONBLOCK,
                O_NONBLOCK
            );
            clear_nonblocking(fds[0]).expect("clear");
            assert_eq!(status_flags(fds[0]).expect("F_GETFL") & O_NONBLOCK, 0);
            // SAFETY: descriptors this test owns, closed once.
            unsafe {
                close(fds[0]);
                close(fds[1]);
            }
            assert_eq!(status_flags(fds[0]), Err(9));
            assert_eq!(set_status_flags(fds[0], 0), Err(9));
        }
    }

    #[cfg(not(unix))]
    #[test]
    fn the_host_has_no_flags_and_no_names() {
        assert_eq!(status_flags(0), Err(crate::ENOSYS));
        assert_eq!(set_status_flags(0, 0), Err(crate::ENOSYS));
        let mut buf = [0u8; 16];
        assert_eq!(ttyname_into(0, &mut buf), Err(crate::ENOSYS));
    }
}
