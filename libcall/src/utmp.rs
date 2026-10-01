//! The user-accounting files -- `utmp`, `wtmp`, `btmp` -- through the C
//! library.
//!
//! `utmp` (`/var/run/utmp`) says who is logged in now; `wtmp`
//! (`/var/log/wtmp`) is the history `last` reads; `btmp` (`/var/log/btmp`)
//! the failed attempts `lastb` reads. `posix/src/utmpx.rs` implements glibc's
//! calls over them -- the file locking, the search rules of `getutxid` and
//! `getutxline`, the record conversion -- and a program that writes them
//! reaches those calls here, through the C ABI, as `design-decisions.md` §768
//! has every stateful call reached. Asked for by lane D in
//! `requests/d-b-utmp-is-a-real-file-now-create-it-and-write-logins.md`.
//!
//! # Only on SlateOS
//!
//! Unlike most of this crate, the calls are gated on
//! `target_vendor = "slateos"`, not `unix`. On a Linux development host
//! `cfg(unix)` would reach glibc's versions -- which write the *developer's
//! own* `/var/run/utmp` and `/var/log/wtmp`, from a test. Elsewhere [`Utmp`]
//! is a file with nothing in it, [`Utmp::put`] answers [`crate::ENOSYS`] and
//! [`append`] does nothing, which is what a host has: no accounting of its
//! own to join.
//!
//! # The record
//!
//! [`Utmpx`] restates the C library's `struct utmpx` (musl's layout, 400
//! bytes) rather than importing it: this crate must not depend on `posix`
//! (see the crate documentation). The tests hold the two to the same size and
//! the same field offsets, so they cannot drift.

use core::ffi::CStr;

/// `ut_type`: an unused slot.
pub const EMPTY: i16 = 0;
/// `ut_type`: a process `init` started.
pub const INIT_PROCESS: i16 = 5;
/// `ut_type`: a `getty` waiting for a name.
pub const LOGIN_PROCESS: i16 = 6;
/// `ut_type`: a logged-in session.
pub const USER_PROCESS: i16 = 7;
/// `ut_type`: a session that has ended.
pub const DEAD_PROCESS: i16 = 8;

/// Length of `ut_line`.
pub const UT_LINESIZE: usize = 32;
/// Length of `ut_user`.
pub const UT_NAMESIZE: usize = 32;
/// Length of `ut_host`.
pub const UT_HOSTSIZE: usize = 256;
/// Length of `ut_id`.
pub const UT_IDSIZE: usize = 4;

/// Who is logged in now (glibc's `_PATH_UTMP`).
pub const UTMP_FILE: &CStr = c"/var/run/utmp";
/// The login history `last` reads (`_PATH_WTMP`).
pub const WTMP_FILE: &CStr = c"/var/log/wtmp";
/// The failed attempts `lastb` reads (`_PATH_BTMP`).
pub const BTMP_FILE: &CStr = c"/var/log/btmp";

/// `struct timeval`: seconds and microseconds since the epoch.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timeval {
    /// Whole seconds.
    pub tv_sec: i64,
    /// Microseconds within the second.
    pub tv_usec: i64,
}

/// One accounting record: the C library's `struct utmpx`.
///
/// The text fields are fixed-size and need not be NUL-terminated: a name
/// that fills its field has no terminator. Fill them with [`fill_field`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utmpx {
    /// What the record is: [`USER_PROCESS`], [`DEAD_PROCESS`], ...
    pub ut_type: i16,
    /// The process the record is about.
    pub ut_pid: i32,
    /// The terminal, without `/dev/` (`tty1`, `pts/3`).
    pub ut_line: [u8; UT_LINESIZE],
    /// The record's identity in the file: usually the terminal's number.
    pub ut_id: [u8; UT_IDSIZE],
    /// The user's name.
    pub ut_user: [u8; UT_NAMESIZE],
    /// The remote host, for a login from the network.
    pub ut_host: [u8; UT_HOSTSIZE],
    /// `struct exit_status`: termination and exit status of a dead process.
    pub ut_exit: [i16; 2],
    /// Session ID.
    pub ut_session: i32,
    /// When the record was written.
    pub ut_tv: Timeval,
    /// The remote host's address.
    pub ut_addr_v6: [i32; 4],
    /// musl's `__unused[20]`, part of the record a C reader expects.
    reserved: [u8; 20],
}

impl Utmpx {
    /// An all-zero record: [`EMPTY`], every field empty.
    pub const ZERO: Self = Self {
        ut_type: EMPTY,
        ut_pid: 0,
        ut_line: [0; UT_LINESIZE],
        ut_id: [0; UT_IDSIZE],
        ut_user: [0; UT_NAMESIZE],
        ut_host: [0; UT_HOSTSIZE],
        ut_exit: [0; 2],
        ut_session: 0,
        ut_tv: Timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        ut_addr_v6: [0; 4],
        reserved: [0; 20],
    };
}

/// Copy `src` into a fixed-size record field as `strncpy` does -- util-linux
/// calls it `str2memcpy`: as much as fits, the rest zero, and no terminator
/// when `src` fills the field.
pub fn fill_field(field: &mut [u8], src: &[u8]) {
    let n = src.len().min(field.len());
    let (head, tail) = field.split_at_mut(n);
    head.copy_from_slice(src.get(..n).unwrap_or_default());
    tail.fill(0);
}

#[cfg(target_vendor = "slateos")]
mod sys {
    use super::Utmpx;

    unsafe extern "C" {
        pub fn setutxent();
        pub fn getutxent() -> *mut Utmpx;
        pub fn getutxid(id: *const Utmpx) -> *mut Utmpx;
        pub fn getutxline(line: *const Utmpx) -> *mut Utmpx;
        pub fn pututxline(ut: *const Utmpx) -> *mut Utmpx;
        pub fn endutxent();
        pub fn updwtmpx(file: *const core::ffi::c_char, ut: *const Utmpx);
    }
}

/// The `utmp` file, open: `setutxent` when made, `endutxent` when dropped.
///
/// The C library keeps one position in the file per process, so there must
/// not be two of these at once; `login` holds one for the length of one
/// lookup-and-write, as util-linux's does.
pub struct Utmp {
    _open: (),
}

impl Utmp {
    /// Open the file and stand at its first record.
    #[must_use]
    pub fn open() -> Self {
        #[cfg(target_vendor = "slateos")]
        // SAFETY: `setutxent` takes nothing and only (re)opens the library's
        // own file state.
        unsafe {
            sys::setutxent();
        }
        Self { _open: () }
    }

    /// Back to the first record (`setutxent` again).
    pub fn rewind(&mut self) {
        #[cfg(target_vendor = "slateos")]
        // SAFETY: as in `open`.
        unsafe {
            sys::setutxent();
        }
    }

    /// The next record (`getutxent`), or `None` at the end of the file or if
    /// it cannot be read.
    pub fn next_record(&mut self) -> Option<Utmpx> {
        #[cfg(target_vendor = "slateos")]
        {
            // SAFETY: `getutxent` returns null or a pointer to the library's
            // static record, valid until the next utmp call; it is copied out
            // before anything else runs.
            unsafe { sys::getutxent().as_ref().copied() }
        }
        #[cfg(not(target_vendor = "slateos"))]
        {
            None
        }
    }

    /// The next record whose `ut_line` is `line.ut_line`, among
    /// `LOGIN_PROCESS` and `USER_PROCESS` records (`getutxline`).
    pub fn find_line(&mut self, line: &Utmpx) -> Option<Utmpx> {
        #[cfg(target_vendor = "slateos")]
        {
            // SAFETY: `line` is a live record for the call; the result is
            // null or the library's static record, copied out at once.
            unsafe { sys::getutxline(line).as_ref().copied() }
        }
        #[cfg(not(target_vendor = "slateos"))]
        {
            let _ = line;
            None
        }
    }

    /// The next record matching `id` by `getutxid`'s rule: by type for the
    /// run-level and boot records, by `ut_id` among the process records.
    pub fn find_id(&mut self, id: &Utmpx) -> Option<Utmpx> {
        #[cfg(target_vendor = "slateos")]
        {
            // SAFETY: as in `find_line`.
            unsafe { sys::getutxid(id).as_ref().copied() }
        }
        #[cfg(not(target_vendor = "slateos"))]
        {
            let _ = id;
            None
        }
    }

    /// Write `ut` (`pututxline`): over the record `getutxid` would find for
    /// it, or at the end.
    ///
    /// # Errors
    ///
    /// The `errno` `pututxline` set -- the file cannot be opened for writing,
    /// cannot be locked, or a write failed -- and [`crate::ENOSYS`] anywhere
    /// that is not SlateOS.
    pub fn put(&mut self, ut: &Utmpx) -> Result<(), i32> {
        #[cfg(target_vendor = "slateos")]
        {
            // SAFETY: `ut` is a live record for the call, which only reads
            // it; the result is only compared with null.
            let written = unsafe { sys::pututxline(ut) };
            if written.is_null() {
                Err(crate::last_errno())
            } else {
                Ok(())
            }
        }
        #[cfg(not(target_vendor = "slateos"))]
        {
            let _ = ut;
            Err(crate::ENOSYS)
        }
    }
}

impl Drop for Utmp {
    fn drop(&mut self) {
        #[cfg(target_vendor = "slateos")]
        // SAFETY: `endutxent` takes nothing and only closes the library's
        // own file state.
        unsafe {
            sys::endutxent();
        }
    }
}

/// Append `ut` to the history file `file` (`updwtmpx`): `wtmp` for a login
/// or a logout, `btmp` for a failed attempt.
///
/// Reports nothing, because the C call reports nothing: a file that does not
/// exist is left alone, as glibc leaves it -- whether there is a history is
/// the system's choice, made by creating the file -- and a record that could
/// not be written whole is cut back off. Nothing at all happens anywhere that
/// is not SlateOS.
pub fn append(file: &CStr, ut: &Utmpx) {
    #[cfg(target_vendor = "slateos")]
    // SAFETY: `file` is a NUL-terminated path and `ut` a live record, both
    // only read for the duration of the call.
    unsafe {
        sys::updwtmpx(file.as_ptr(), ut);
    }
    #[cfg(not(target_vendor = "slateos"))]
    {
        let _ = (file, ut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn the_record_is_the_c_librarys_field_for_field() {
        use posix::utmpx::Utmpx as Theirs;
        assert_eq!(size_of::<Utmpx>(), size_of::<Theirs>());
        assert_eq!(size_of::<Utmpx>(), 400, "musl's struct utmpx");
        assert_eq!(offset_of!(Utmpx, ut_type), offset_of!(Theirs, ut_type));
        assert_eq!(offset_of!(Utmpx, ut_pid), offset_of!(Theirs, ut_pid));
        assert_eq!(offset_of!(Utmpx, ut_line), offset_of!(Theirs, ut_line));
        assert_eq!(offset_of!(Utmpx, ut_id), offset_of!(Theirs, ut_id));
        assert_eq!(offset_of!(Utmpx, ut_user), offset_of!(Theirs, ut_user));
        assert_eq!(offset_of!(Utmpx, ut_host), offset_of!(Theirs, ut_host));
        assert_eq!(offset_of!(Utmpx, ut_exit), offset_of!(Theirs, ut_exit));
        assert_eq!(
            offset_of!(Utmpx, ut_session),
            offset_of!(Theirs, ut_session)
        );
        assert_eq!(offset_of!(Utmpx, ut_tv), offset_of!(Theirs, ut_tv));
        assert_eq!(
            offset_of!(Utmpx, ut_addr_v6),
            offset_of!(Theirs, ut_addr_v6)
        );
    }

    #[test]
    fn the_constants_are_the_c_librarys() {
        use posix::utmpx as p;
        assert_eq!(USER_PROCESS, p::USER_PROCESS);
        assert_eq!(DEAD_PROCESS, p::DEAD_PROCESS);
        assert_eq!(LOGIN_PROCESS, p::LOGIN_PROCESS);
        assert_eq!(INIT_PROCESS, p::INIT_PROCESS);
        assert_eq!(EMPTY, p::EMPTY);
        assert_eq!(UT_LINESIZE, p::UT_LINESIZE);
        assert_eq!(UT_NAMESIZE, p::UT_NAMESIZE);
        assert_eq!(UT_HOSTSIZE, p::UT_HOSTSIZE);
        assert_eq!(UT_IDSIZE, p::UT_IDSIZE);
        assert_eq!(UTMP_FILE.to_bytes_with_nul(), p::UTMPX_FILE);
        assert_eq!(WTMP_FILE.to_bytes_with_nul(), p::WTMPX_FILE);
    }

    #[test]
    fn a_field_is_filled_as_strncpy_fills_it() {
        let mut f = [0xAAu8; 4];
        fill_field(&mut f, b"ab");
        assert_eq!(f, *b"ab\0\0");
        fill_field(&mut f, b"abcd");
        assert_eq!(f, *b"abcd", "no terminator when it fills the field");
        fill_field(&mut f, b"abcdef");
        assert_eq!(f, *b"abcd", "cut, not refused");
        fill_field(&mut f, b"");
        assert_eq!(f, [0; 4]);
    }

    #[cfg(not(target_vendor = "slateos"))]
    #[test]
    fn a_host_has_no_accounting_to_join() {
        let mut db = Utmp::open();
        assert_eq!(db.next_record(), None);
        assert_eq!(db.find_line(&Utmpx::ZERO), None);
        assert_eq!(db.find_id(&Utmpx::ZERO), None);
        assert_eq!(db.put(&Utmpx::ZERO), Err(crate::ENOSYS));
        append(WTMP_FILE, &Utmpx::ZERO);
    }
}
