//! POSIX user accounting database (`<utmpx.h>`), with glibc's `<utmp.h>`
//! names beside it.
//!
//! ## The file is glibc's
//!
//! The database is `/var/run/utmp` ([`UTMPX_FILE`]), a file of fixed-size
//! records in glibc's x86-64 `struct utmp` layout -- 384 bytes, its time in
//! two 32-bit fields -- which is the file every Linux tool reads, and what
//! `utmpfile` reads for this tree's `who`, `last`, `w` and `finger`.  The
//! structure a C program holds is musl's `struct utmpx` ([`Utmpx`], 400
//! bytes, a full `struct timeval`), the one this system's headers declare;
//! each record is converted on its way in and out (see [`FileRecord`]).
//!
//! ## The calls are glibc's
//!
//! glibc 2.39's `login/utmp_file.c`, call for call: one open file and a
//! position in it, the last record read kept for [`pututxline`] to try
//! first; [`getutxid`] matching `RUN_LVL`, `BOOT_TIME`, `NEW_TIME` and
//! `OLD_TIME` by type alone and the process records by `ut_id` (by
//! `ut_line` when either `ut_id` is empty); [`getutxline`] finding
//! `USER_PROCESS` and `LOGIN_PROCESS` records by `ut_line`; and
//! [`pututxline`] overwriting the record it matches -- searching forward
//! from where the reading stopped -- or appending, and returning the
//! caller's own pointer.  A file that does not exist is not created: every
//! call answers with the failed `open`'s error, as glibc's do, and whoever
//! starts the system creates it.
//!
//! **Locking.**  glibc takes `fcntl(F_SETLKW)` locks around every read and
//! write.  So does this, through this library's `fcntl` -- which does not
//! enforce record locks yet
//! (`requests/b-a-advisory-record-locking-is-a-stub-that-always-succeeds.md`),
//! so until it does, two writers at once are not excluded.  glibc also
//! bounds the wait with a ten-second `alarm`; this waits as `fcntl` does.
//!
//! **A NULL record** is `EFAULT` where glibc's code first touches it:
//! comparing it with a record read, or writing it -- after the file's own
//! errors, and not at all when nothing would be compared or written.
//!
//! ## libutil's three, and glibc's two copies
//!
//! [`login`], [`logout`] and [`logwtmp`] are glibc's `login/login.c`,
//! `logout.c` and `logwtmp.c` over the calls above -- `login` names the line
//! after the terminal of standard input, output or error (`???` with none,
//! and then writes the history alone), and both it and `logout` leave the
//! database's name `_PATH_UTMP`, as glibc's do. Each is an archive member of
//! its own, as in glibc: `login` and `logout` are names a program may have
//! for functions of its own. [`getutmp`] and [`getutmpx`] copy an entry
//! field by field, `struct utmp` being `struct utmpx` in musl's headers.
//! Where glibc's dereferences a NULL argument, these do nothing.
//!
//! Until 2026-09-26 every call here was a stub: nothing was ever read,
//! [`pututxline`] reported success writing nothing, and [`utmpxname`]
//! accepted any name and used none.  So `who` saw nobody, and a login
//! recorded through this library was lost (`TD-POSIX-GETLOGIN-IS-A-CONSTANT`
//! in known-issues.md names what that left `getlogin` with).

use crate::errno;
use crate::perprocess::process_global;

// ---------------------------------------------------------------------------
// utmpx entry types (Linux values)
// ---------------------------------------------------------------------------

/// Empty entry.
pub const EMPTY: i16 = 0;
/// Entry for a process that started a run level change.
pub const RUN_LVL: i16 = 1;
/// Entry for the system boot time.
pub const BOOT_TIME: i16 = 2;
/// Time after system clock changed.
pub const NEW_TIME: i16 = 3;
/// Time when system clock changed.
pub const OLD_TIME: i16 = 4;
/// Entry for a process started by `init`.
pub const INIT_PROCESS: i16 = 5;
/// Entry for a session leader (login process).
pub const LOGIN_PROCESS: i16 = 6;
/// Normal user process.
pub const USER_PROCESS: i16 = 7;
/// Terminated process.
pub const DEAD_PROCESS: i16 = 8;
/// Not defined on Linux, reserved for future use.
pub const ACCOUNTING: i16 = 9;

// ---------------------------------------------------------------------------
// struct utmpx
// ---------------------------------------------------------------------------

/// Size of the `ut_user` field.
pub const UT_NAMESIZE: usize = 32;
/// Size of the `ut_line` field.
pub const UT_LINESIZE: usize = 32;
/// Size of the `ut_host` field.
pub const UT_HOSTSIZE: usize = 256;
/// Size of the `ut_id` field.
pub const UT_IDSIZE: usize = 4;

/// The `ut_tv` of a `struct utmpx`: a **full** `struct timeval`, 16 bytes.
///
/// # Not 32-bit fields, whatever the comment used to say
///
/// This was `i32`/`i32` under "32-bit fields for compatibility", which is
/// glibc's `struct { __int32_t tv_sec; __int32_t tv_usec; }`. musl uses a real
/// `struct timeval` — `time_t` and `suseconds_t`, both 8 bytes — and musl is
/// the C library every port in this tree links against. Measured:
///
/// ```text
/// musl: sizeof(struct utmpx)=400  ut_tv@344 (16 bytes)  ut_addr_v6@360
/// ours: 384                       ut_tv@340 ( 8 bytes)  ut_addr_v6@348
/// ```
///
/// Eight bytes short *and* 4-aligned rather than 8, which is where both the
/// missing 16 bytes and the shifted offsets came from. Found by
/// `scripts/check-libc-abi.py`; `design-decisions.md` §1011.  (The *file*
/// keeps glibc's 32-bit pair -- see [`FileRecord`].)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UtmpxTimeval {
    /// Seconds since epoch.
    pub tv_sec: i64,
    /// Microseconds.
    pub tv_usec: i64,
}

/// User accounting database entry.
///
/// Matches musl's `struct utmpx`, 400 bytes, asserted against musl's own
/// header by `scripts/check-libc-abi.py`. It used to say "glibc-compatible",
/// which was true and was the wrong library — see [`UtmpxTimeval`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Utmpx {
    /// Type of entry (EMPTY, USER_PROCESS, etc.).
    pub ut_type: i16,
    /// PID of login process.
    pub ut_pid: i32,
    /// Terminal device name (`/dev/ttyN`).
    pub ut_line: [u8; UT_LINESIZE],
    /// Identifier for the entry (typically terminal name suffix).
    pub ut_id: [u8; UT_IDSIZE],
    /// Username.
    pub ut_user: [u8; UT_NAMESIZE],
    /// Hostname for remote login.
    pub ut_host: [u8; UT_HOSTSIZE],
    /// Exit status for DEAD_PROCESS entries.
    pub ut_exit: [u8; 4], // struct exit_status { e_termination, e_exit }
    /// Session ID.
    pub ut_session: i32,
    /// Time entry was made.
    pub ut_tv: UtmpxTimeval,
    /// Internet address of remote host (IPv6).
    pub ut_addr_v6: [i32; 4],
    /// musl's trailing `char __unused[20]`. Was documented as "reserved for
    /// future use", which is what it looks like from this side and not what it
    /// is: it is part of the record a C `who` reads, and removing it would make
    /// the structure 380 bytes.
    _reserved: [u8; 20],
}

impl Utmpx {
    /// An all-zero entry: `EMPTY`, every string empty.
    pub const ZERO: Self = Self {
        ut_type: EMPTY,
        ut_pid: 0,
        ut_line: [0; UT_LINESIZE],
        ut_id: [0; UT_IDSIZE],
        ut_user: [0; UT_NAMESIZE],
        ut_host: [0; UT_HOSTSIZE],
        ut_exit: [0; 4],
        ut_session: 0,
        ut_tv: UtmpxTimeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        ut_addr_v6: [0; 4],
        _reserved: [0; 20],
    };
}

// ---------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------

/// The database, glibc's `_PATH_UTMP`.
pub const UTMPX_FILE: &[u8] = b"/var/run/utmp\0";
/// The login history, glibc's `_PATH_WTMP`, which [`updwtmpx`] appends to.
pub const WTMPX_FILE: &[u8] = b"/var/log/wtmp\0";

/// One record of the file: glibc's x86-64 `struct utmp`, 384 bytes, whose
/// time is a pair of 32-bit fields (`__WORDSIZE_TIME64_COMPAT32`).  The
/// offsets are the ones `utmpfile` reads.
#[repr(C)]
#[derive(Clone, Copy)]
struct FileRecord {
    ut_type: i16,
    ut_pid: i32,
    ut_line: [u8; UT_LINESIZE],
    ut_id: [u8; UT_IDSIZE],
    ut_user: [u8; UT_NAMESIZE],
    ut_host: [u8; UT_HOSTSIZE],
    ut_exit: [u8; 4],
    ut_session: i32,
    ut_tv_sec: i32,
    ut_tv_usec: i32,
    ut_addr_v6: [i32; 4],
    reserved: [u8; 20],
}

/// A record's size in the file.
const RECORD: usize = size_of::<FileRecord>();
const _: () = assert!(RECORD == 384);
/// [`RECORD`] as a file offset.
const RECORD_OFF: i64 = 384;

impl FileRecord {
    const ZERO: Self = Self {
        ut_type: EMPTY,
        ut_pid: 0,
        ut_line: [0; UT_LINESIZE],
        ut_id: [0; UT_IDSIZE],
        ut_user: [0; UT_NAMESIZE],
        ut_host: [0; UT_HOSTSIZE],
        ut_exit: [0; 4],
        ut_session: 0,
        ut_tv_sec: 0,
        ut_tv_usec: 0,
        ut_addr_v6: [0; 4],
        reserved: [0; 20],
    };

    /// The record a C program's entry is written as.  The time is cut to 32
    /// bits, as glibc's structure holds it: a program assigning `time(NULL)`
    /// to glibc's `ut_tv.tv_sec` truncates the same way.
    #[allow(clippy::cast_possible_truncation)]
    const fn of(u: &Utmpx) -> Self {
        Self {
            ut_type: u.ut_type,
            ut_pid: u.ut_pid,
            ut_line: u.ut_line,
            ut_id: u.ut_id,
            ut_user: u.ut_user,
            ut_host: u.ut_host,
            ut_exit: u.ut_exit,
            ut_session: u.ut_session,
            ut_tv_sec: u.ut_tv.tv_sec as i32,
            ut_tv_usec: u.ut_tv.tv_usec as i32,
            ut_addr_v6: u.ut_addr_v6,
            reserved: [0; 20],
        }
    }

    /// The entry a C program is handed for this record.  (`as`, because
    /// `i64::from` is not callable in a `const fn`; the casts widen.)
    #[allow(clippy::cast_lossless)]
    const fn entry(&self) -> Utmpx {
        Utmpx {
            ut_type: self.ut_type,
            ut_pid: self.ut_pid,
            ut_line: self.ut_line,
            ut_id: self.ut_id,
            ut_user: self.ut_user,
            ut_host: self.ut_host,
            ut_exit: self.ut_exit,
            ut_session: self.ut_session,
            ut_tv: UtmpxTimeval {
                tv_sec: self.ut_tv_sec as i64,
                tv_usec: self.ut_tv_usec as i64,
            },
            ut_addr_v6: self.ut_addr_v6,
            _reserved: [0; 20],
        }
    }
}

/// `strncmp(a, b, N) == 0` on two fixed-size fields: equal up to their first
/// NUL, or all the way.
fn field_eq<const N: usize>(a: &[u8; N], b: &[u8; N]) -> bool {
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return false;
        }
        if *x == 0 {
            return true;
        }
    }
    true
}

/// A process record: the types whose records are one process's, matched by
/// `ut_id` rather than by type.
const fn is_process_type(t: i16) -> bool {
    matches!(
        t,
        INIT_PROCESS | LOGIN_PROCESS | USER_PROCESS | DEAD_PROCESS
    )
}

/// glibc's `__utmp_equal` (sysdeps/generic/utmp-equal.h): two process
/// records of the same process -- by `ut_id`, or by `ut_line` when either
/// `ut_id` is empty.
fn same_process(entry: &FileRecord, m: &Utmpx) -> bool {
    is_process_type(entry.ut_type)
        && is_process_type(m.ut_type)
        && if entry.ut_id[0] != 0 && m.ut_id[0] != 0 {
            field_eq(&entry.ut_id, &m.ut_id)
        } else {
            field_eq(&entry.ut_line, &m.ut_line)
        }
}

/// glibc's `matches_last_entry`'s comparison: the four time records by type
/// alone, the rest by [`same_process`].
fn matches(entry: &FileRecord, m: &Utmpx) -> bool {
    match m.ut_type {
        RUN_LVL | BOOT_TIME | OLD_TIME | NEW_TIME => m.ut_type == entry.ut_type,
        _ => same_process(entry, m),
    }
}

// ---------------------------------------------------------------------------
// The I/O the database does -- on the file, or in the host tests a buffer
// ---------------------------------------------------------------------------

/// What the database asks of the file: open, read and write a record at an
/// offset, find the end, cut, lock.  One module for the target and one for
/// the host tests, where the file calls have nothing to reach.
#[cfg(not(test))]
mod io {
    use super::{FileRecord, RECORD};
    use crate::errno;
    use crate::types::Fd;

    /// The caller's errno of a call that answered -1.
    fn err() -> i32 {
        errno::get_errno()
    }

    pub(super) fn open(path: *const u8, flags: i32) -> Result<Fd, i32> {
        let fd = crate::file::open(path, flags, 0);
        if fd < 0 { Err(err()) } else { Ok(fd) }
    }

    /// Put a read-write descriptor for `path` where `fd` is, as glibc's
    /// `pututline` does with `dup2`, so the position and locks stay one
    /// file's.
    pub(super) fn reopen_rw(path: *const u8, fd: Fd) -> Result<(), i32> {
        let rw = open(path, crate::fcntl::O_RDWR | crate::fcntl::O_CLOEXEC)?;
        let moved = crate::file::dup2(rw, fd);
        let e = err();
        // The descriptor was this function's own and is now either `fd` or
        // useless; a failed close changes neither.
        let _ = crate::file::close(rw);
        if moved < 0 { Err(e) } else { Ok(()) }
    }

    pub(super) fn close(fd: Fd) {
        // glibc's `__close_nocancel_nostatus`: nothing to report to.
        let _ = crate::file::close(fd);
    }

    /// Read the record at `off`: the bytes read, fewer than a record at the
    /// end of the file.
    pub(super) fn read_at(fd: Fd, rec: &mut FileRecord, off: i64) -> Result<usize, i32> {
        let n = crate::file::pread(fd, (&raw mut *rec).cast::<u8>(), RECORD, off);
        usize::try_from(n).map_err(|_| err())
    }

    pub(super) fn write_at(fd: Fd, rec: &FileRecord, off: i64) -> Result<usize, i32> {
        let n = crate::file::pwrite(fd, (&raw const *rec).cast::<u8>(), RECORD, off);
        usize::try_from(n).map_err(|_| err())
    }

    pub(super) fn end(fd: Fd) -> Result<i64, i32> {
        let n = crate::file::lseek(fd, 0, crate::fcntl::SEEK_END);
        if n < 0 { Err(err()) } else { Ok(n) }
    }

    pub(super) fn truncate(fd: Fd, len: i64) {
        // glibc ignores it too: the record being cut is the failed one.
        let _ = crate::file::ftruncate(fd, len);
    }

    /// `fcntl(F_SETLKW)` on the whole file, glibc's `try_file_lock`.
    pub(super) fn lock(fd: Fd, kind: i16) -> Result<(), i32> {
        let mut fl = crate::fcntl_ops::Flock {
            l_type: kind,
            l_whence: 0,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        };
        // `fcntl`'s third argument is the pointer, as a `long`.
        let arg = i64::try_from((&raw mut fl).expose_provenance()).map_err(|_| errno::EFAULT)?;
        let r = crate::fcntl_ops::fcntl(fd, crate::fcntl_ops::F_SETLKW, arg);
        if r < 0 { Err(err()) } else { Ok(()) }
    }

    pub(super) fn unlock(fd: Fd) {
        // glibc's `file_unlock` ignores the result; an unlock of a lock this
        // process holds has nothing to fail on.
        let _ = lock(fd, crate::fcntl_ops::F_UNLCK);
    }
}

/// The host tests' stand-in for the file calls: files are buffers in this
/// thread, descriptors index them.
#[cfg(test)]
mod io {
    extern crate std;
    use super::{FileRecord, RECORD};
    use crate::errno;
    use crate::types::Fd;
    use core::cell::RefCell;
    use std::collections::HashMap;
    use std::vec::Vec;

    struct Open {
        path: Vec<u8>,
        writable: bool,
    }

    std::thread_local! {
        static FILES: RefCell<HashMap<Vec<u8>, Vec<u8>>> = RefCell::new(HashMap::new());
        static FDS: RefCell<Vec<Option<Open>>> = const { RefCell::new(Vec::new()) };
        /// A write this many bytes short, to test a full disk.
        static SHORT_WRITE: RefCell<Option<usize>> = const { RefCell::new(None) };
    }

    fn path_of(p: *const u8) -> Vec<u8> {
        // SAFETY: the tests pass NUL-terminated names.
        unsafe { core::ffi::CStr::from_ptr(p.cast()) }
            .to_bytes()
            .to_vec()
    }

    /// Create (or replace) a file with these bytes.
    pub(super) fn put_file(path: &[u8], bytes: &[u8]) {
        FILES.with(|f| f.borrow_mut().insert(path.to_vec(), bytes.to_vec()));
    }

    /// A file's bytes, if it exists.
    pub(super) fn file(path: &[u8]) -> Option<Vec<u8>> {
        FILES.with(|f| f.borrow().get(path).cloned())
    }

    pub(super) fn remove_file(path: &[u8]) {
        FILES.with(|f| f.borrow_mut().remove(path));
    }

    pub(super) fn short_write(by: Option<usize>) {
        SHORT_WRITE.with(|s| *s.borrow_mut() = by);
    }

    /// Descriptors this thread holds open.
    pub(super) fn open_count() -> usize {
        FDS.with(|f| f.borrow().iter().filter(|o| o.is_some()).count())
    }

    fn with_open<R>(
        fd: Fd,
        f: impl FnOnce(&Open, &mut Vec<u8>) -> Result<R, i32>,
    ) -> Result<R, i32> {
        FDS.with(|fds| {
            let fds = fds.borrow();
            let o = usize::try_from(fd)
                .ok()
                .and_then(|i| fds.get(i))
                .and_then(Option::as_ref)
                .ok_or(errno::EBADF)?;
            FILES.with(|files| {
                let mut files = files.borrow_mut();
                let bytes = files.get_mut(&o.path).ok_or(errno::ENOENT)?;
                f(o, bytes)
            })
        })
    }

    pub(super) fn open(path: *const u8, flags: i32) -> Result<Fd, i32> {
        let path = path_of(path);
        if file(&path).is_none() {
            return Err(errno::ENOENT);
        }
        let writable = flags & 3 != crate::fcntl::O_RDONLY;
        FDS.with(|fds| {
            let mut fds = fds.borrow_mut();
            fds.push(Some(Open { path, writable }));
            Fd::try_from(fds.len() - 1).map_err(|_| errno::EMFILE)
        })
    }

    pub(super) fn reopen_rw(path: *const u8, fd: Fd) -> Result<(), i32> {
        let path = path_of(path);
        if file(&path).is_none() {
            return Err(errno::ENOENT);
        }
        FDS.with(|fds| {
            let mut fds = fds.borrow_mut();
            let slot = usize::try_from(fd)
                .ok()
                .and_then(|i| fds.get_mut(i))
                .ok_or(errno::EBADF)?;
            *slot = Some(Open {
                path,
                writable: true,
            });
            Ok(())
        })
    }

    pub(super) fn close(fd: Fd) {
        FDS.with(|fds| {
            if let Some(slot) = usize::try_from(fd)
                .ok()
                .and_then(|i| fds.borrow_mut().get_mut(i).map(core::mem::take))
            {
                drop(slot);
            }
        });
    }

    pub(super) fn read_at(fd: Fd, rec: &mut FileRecord, off: i64) -> Result<usize, i32> {
        with_open(fd, |_, bytes| {
            let off = usize::try_from(off).map_err(|_| errno::EINVAL)?;
            let avail = bytes.get(off..).unwrap_or(&[]);
            let n = avail.len().min(RECORD);
            // SAFETY: `rec` is RECORD bytes of plain integers; any bytes are one.
            let dst =
                unsafe { core::slice::from_raw_parts_mut((&raw mut *rec).cast::<u8>(), RECORD) };
            dst[..n].copy_from_slice(&avail[..n]);
            Ok(n)
        })
    }

    pub(super) fn write_at(fd: Fd, rec: &FileRecord, off: i64) -> Result<usize, i32> {
        with_open(fd, |o, bytes| {
            if !o.writable {
                return Err(errno::EBADF);
            }
            let off = usize::try_from(off).map_err(|_| errno::EINVAL)?;
            let n = RECORD - SHORT_WRITE.with(|s| s.borrow().unwrap_or(0));
            // SAFETY: as in `read_at`.
            let src =
                unsafe { core::slice::from_raw_parts((&raw const *rec).cast::<u8>(), RECORD) };
            if bytes.len() < off + n {
                bytes.resize(off + n, 0);
            }
            bytes[off..off + n].copy_from_slice(&src[..n]);
            Ok(n)
        })
    }

    pub(super) fn end(fd: Fd) -> Result<i64, i32> {
        with_open(fd, |_, bytes| {
            i64::try_from(bytes.len()).map_err(|_| errno::EOVERFLOW)
        })
    }

    pub(super) fn truncate(fd: Fd, len: i64) {
        let _ = with_open(fd, |_, bytes| {
            bytes.truncate(usize::try_from(len).unwrap_or(0));
            Ok(())
        });
    }

    pub(super) fn lock(fd: Fd, _kind: i16) -> Result<(), i32> {
        with_open(fd, |_, _| Ok(()))
    }

    pub(super) fn unlock(_fd: Fd) {}
}

// ---------------------------------------------------------------------------
// The database's state: glibc's `file_fd`, `file_writable`, `file_offset`,
// `last_entry` and `__libc_utmp_file_name`
// ---------------------------------------------------------------------------

struct Db {
    /// The open file, or -1.
    fd: crate::types::Fd,
    /// Whether `fd` was opened read-write.
    writable: bool,
    /// Where the next record is read: glibc's `file_offset`.
    offset: i64,
    /// The last record read, for [`pututxline`] to try first.
    last: FileRecord,
    /// The file's name as [`utmpxname`] set it, a copy this owns; NULL is
    /// [`UTMPX_FILE`].
    name: *mut u8,
}

impl Db {
    const CLOSED: Self = Self {
        fd: -1,
        writable: false,
        offset: 0,
        last: FileRecord::ZERO,
        name: core::ptr::null_mut(),
    };

    fn file_name(&self) -> *const u8 {
        if self.name.is_null() {
            UTMPX_FILE.as_ptr()
        } else {
            self.name
        }
    }

    /// glibc's `__libc_setutent`: open the file read-only if it is not open,
    /// and go back to its start.  `false` with `errno` set when it cannot be
    /// opened.
    fn rewind(&mut self) -> bool {
        if self.fd < 0 {
            self.writable = false;
            match io::open(
                self.file_name(),
                crate::fcntl::O_RDONLY | crate::fcntl::O_CLOEXEC,
            ) {
                Ok(fd) => self.fd = fd,
                Err(e) => {
                    errno::set_errno(e);
                    return false;
                }
            }
        }
        self.offset = 0;
        true
    }

    /// glibc's `maybe_setutent`.
    fn ready(&mut self) -> bool {
        self.fd >= 0 || self.rewind()
    }

    /// glibc's `read_last_entry`: `Ok(true)` when a whole record was read
    /// into `last` and `offset` moved past it; `Ok(false)` at the end, a
    /// partial record counting as the end.
    fn read_next(&mut self) -> Result<bool, i32> {
        let mut rec = FileRecord::ZERO;
        if io::read_at(self.fd, &mut rec, self.offset)? != RECORD {
            return Ok(false);
        }
        self.last = rec;
        self.offset = self.offset.saturating_add(RECORD_OFF);
        Ok(true)
    }

    /// glibc's `internal_getut_nolock`: read forward to the first record
    /// `id` matches.  `Ok(false)` at the end (glibc's `ESRCH`).  A NULL `id`
    /// is `EFAULT` at the first record it would be compared with.
    fn find(&mut self, id: *const Utmpx) -> Result<bool, i32> {
        while self.read_next()? {
            // SAFETY: non-NULL is the caller's entry, per the C contract.
            let Some(id) = (unsafe { id.as_ref() }) else {
                return Err(errno::EFAULT);
            };
            if matches(&self.last, id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `getutline_r`'s search: read forward to the next `USER_PROCESS` or
    /// `LOGIN_PROCESS` record on `line`'s `ut_line`.  A NULL `line` is
    /// `EFAULT` at the first such record, where glibc compares it.
    fn find_line(&mut self, line: *const Utmpx) -> Result<bool, i32> {
        while self.read_next()? {
            if matches!(self.last.ut_type, USER_PROCESS | LOGIN_PROCESS) {
                // SAFETY: non-NULL is the caller's entry, per the C contract.
                let Some(line) = (unsafe { line.as_ref() }) else {
                    return Err(errno::EFAULT);
                };
                if field_eq(&line.ut_line, &self.last.ut_line) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// glibc's `__libc_endutent`.
    fn close(&mut self) {
        if self.fd >= 0 {
            io::close(self.fd);
            self.fd = -1;
        }
    }
}

process_global! {
    fn db() -> Db = Db::CLOSED;
    /// The entry the non-reentrant calls hand back, glibc's static `buffer`.
    fn held() -> Utmpx = Utmpx::ZERO;
}

/// Hand the last record read to the caller: `*buffer` and `*result`, as
/// glibc's `_r` functions do.  NULL outputs are `EFAULT`, where glibc's
/// `memcpy` or store would fault.
///
/// # Safety
///
/// Non-NULL `buffer` and `result` are the caller's to write.
unsafe fn deliver(rec: &FileRecord, buffer: *mut Utmpx, result: *mut *mut Utmpx) -> i32 {
    if buffer.is_null() || result.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: both checked non-NULL; the caller's to write.
    unsafe {
        buffer.write(rec.entry());
        result.write(buffer);
    }
    0
}

/// `*result = NULL`, where glibc sets it on a failure; nothing for a NULL
/// `result`, which glibc would fault writing -- the -1 already says it.
///
/// # Safety
///
/// A non-NULL `result` is the caller's to write.
unsafe fn no_result(result: *mut *mut Utmpx) -> i32 {
    if !result.is_null() {
        // SAFETY: non-NULL, the caller's to write.
        unsafe { result.write(core::ptr::null_mut()) };
    }
    -1
}

// ---------------------------------------------------------------------------
// The calls
// ---------------------------------------------------------------------------

/// Go back to the start of the database, opening it if it is not open.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setutxent() {
    // SAFETY: this process's state, one call at a time.
    let d = unsafe { &mut *db() };
    // glibc's `setutent` returns nothing either; a file that cannot be
    // opened is what the next read reports.
    let _ = d.rewind();
}

/// glibc's `getutent_r`: the next entry into `*buffer`, and `*result` set to
/// it.  0, or -1 at the end (`errno` as it was) or on an error, with
/// `*result` NULL.
///
/// # Safety
///
/// `buffer` and `result` are the caller's to write.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getutent_r(buffer: *mut Utmpx, result: *mut *mut Utmpx) -> i32 {
    let saved = errno::get_errno();
    // SAFETY: this process's state, one call at a time.
    let d = unsafe { &mut *db() };
    if !d.ready() {
        // SAFETY: the caller's `result`.
        return unsafe { no_result(result) };
    }
    if let Err(e) = io::lock(d.fd, crate::fcntl_ops::F_RDLCK) {
        errno::set_errno(e);
        return -1;
    }
    let r = d.read_next();
    io::unlock(d.fd);
    match r {
        // SAFETY: the caller's outputs.
        Ok(true) => unsafe { deliver(&d.last, buffer, result) },
        Ok(false) => {
            // glibc: `errno` unchanged at the end, a partial record included.
            errno::set_errno(saved);
            // SAFETY: the caller's `result`.
            unsafe { no_result(result) }
        }
        Err(e) => {
            errno::set_errno(e);
            // SAFETY: the caller's `result`.
            unsafe { no_result(result) }
        }
    }
}

/// Read the next entry: this process's copy of it, or NULL at the end or
/// on an error (`errno` set; unchanged at the end).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutxent() -> *mut Utmpx {
    let mut result = core::ptr::null_mut();
    // SAFETY: this process's buffer, and a local for the result.
    unsafe { getutent_r(held(), &raw mut result) };
    result
}

/// glibc's `getutid_r`: the next entry `id` matches -- the four time types
/// by type, the process types by `ut_id` (or `ut_line`) -- read forward
/// from where the reading stopped.  -1 with `ESRCH` at the end.  A NULL `id`
/// is `EFAULT` at the first entry it would be compared with.
///
/// # Safety
///
/// `id` is NULL or the caller's entry; `buffer` and `result` are the
/// caller's to write.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getutid_r(
    id: *const Utmpx,
    buffer: *mut Utmpx,
    result: *mut *mut Utmpx,
) -> i32 {
    // SAFETY: this process's state, one call at a time.
    let d = unsafe { &mut *db() };
    if !d.ready() {
        // SAFETY: the caller's `result`.
        return unsafe { no_result(result) };
    }
    if let Err(e) = io::lock(d.fd, crate::fcntl_ops::F_RDLCK) {
        errno::set_errno(e);
        // SAFETY: the caller's `result`.
        return unsafe { no_result(result) };
    }
    let r = d.find(id);
    io::unlock(d.fd);
    match r {
        // SAFETY: the caller's outputs.
        Ok(true) => unsafe { deliver(&d.last, buffer, result) },
        Ok(false) => {
            errno::set_errno(errno::ESRCH);
            // SAFETY: the caller's `result`.
            unsafe { no_result(result) }
        }
        Err(e) => {
            errno::set_errno(e);
            // SAFETY: the caller's `result`.
            unsafe { no_result(result) }
        }
    }
}

/// Find the next entry `id` matches: this process's copy, or NULL (`ESRCH`
/// at the end).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutxid(id: *const Utmpx) -> *mut Utmpx {
    let mut result = core::ptr::null_mut();
    // SAFETY: `id` as given; this process's buffer and a local result.
    unsafe { getutid_r(id, held(), &raw mut result) };
    result
}

/// glibc's `getutline_r`: the next `USER_PROCESS` or `LOGIN_PROCESS` entry
/// on `line->ut_line`.  -1 with `ESRCH` at the end.  A NULL `line` is
/// `EFAULT` at the first such entry.
///
/// # Safety
///
/// As [`getutid_r`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getutline_r(
    line: *const Utmpx,
    buffer: *mut Utmpx,
    result: *mut *mut Utmpx,
) -> i32 {
    // SAFETY: this process's state, one call at a time.
    let d = unsafe { &mut *db() };
    if !d.ready() {
        // SAFETY: the caller's `result`.
        return unsafe { no_result(result) };
    }
    if let Err(e) = io::lock(d.fd, crate::fcntl_ops::F_RDLCK) {
        errno::set_errno(e);
        // SAFETY: the caller's `result`.
        return unsafe { no_result(result) };
    }
    let r = d.find_line(line);
    io::unlock(d.fd);
    match r {
        // SAFETY: the caller's outputs.
        Ok(true) => unsafe { deliver(&d.last, buffer, result) },
        Ok(false) => {
            errno::set_errno(errno::ESRCH);
            // SAFETY: the caller's `result`.
            unsafe { no_result(result) }
        }
        Err(e) => {
            errno::set_errno(e);
            // SAFETY: the caller's `result`.
            unsafe { no_result(result) }
        }
    }
}

/// Find the next login entry on `line->ut_line`: this process's copy, or
/// NULL (`ESRCH` at the end).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutxline(line: *const Utmpx) -> *mut Utmpx {
    let mut result = core::ptr::null_mut();
    // SAFETY: `line` as given; this process's buffer and a local result.
    unsafe { getutline_r(line, held(), &raw mut result) };
    result
}

/// Write an entry: over the one it matches (the last entry read, if it
/// does, else the next match from there on), or at the end.
///
/// Returns `utmpx` itself on success, as glibc does, or NULL with `errno`
/// set: the failed `open` of the file (which is never created), `ENOSPC` for
/// a record written in part (an appended part is cut off again), or
/// `EFAULT` for a NULL entry -- where glibc first reads it: comparing it
/// with a record, or writing it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pututxline(utmpx: *const Utmpx) -> *mut Utmpx {
    // SAFETY: this process's state, one call at a time.
    let d = unsafe { &mut *db() };
    if !d.ready() {
        return core::ptr::null_mut();
    }
    if !d.writable {
        // glibc: the read-only descriptor becomes a read-write one, dup2'd
        // over it so the position is kept.
        if let Err(e) = io::reopen_rw(d.file_name(), d.fd) {
            errno::set_errno(e);
            return core::ptr::null_mut();
        }
        d.writable = true;
    }
    if let Err(e) = io::lock(d.fd, crate::fcntl_ops::F_WRLCK) {
        errno::set_errno(e);
        return core::ptr::null_mut();
    }
    let r = put(d, utmpx);
    io::unlock(d.fd);
    match r {
        Ok(()) => utmpx.cast_mut(),
        Err(e) => {
            errno::set_errno(e);
            core::ptr::null_mut()
        }
    }
}

/// The body of [`pututxline`], under its write lock: glibc's
/// `__libc_pututline` from "Find the correct place to insert the data".
fn put(d: &mut Db, utmpx: *const Utmpx) -> Result<(), i32> {
    // SAFETY: non-NULL is the caller's entry, per the C contract.
    let data = unsafe { utmpx.as_ref() };
    let mut found = false;
    // `matches_last_entry`: nothing read yet cannot match, and is not
    // compared.
    if d.offset > 0 {
        let data = data.ok_or(errno::EFAULT)?;
        if matches(&d.last, data) {
            // Read it back under the write lock, as glibc does.
            d.offset = d.offset.saturating_sub(RECORD_OFF);
            found = d.read_next()? && matches(&d.last, data);
        }
    }
    if !found {
        found = d.find(utmpx)?;
    }
    let at = if found {
        d.offset.saturating_sub(RECORD_OFF)
    } else {
        // Appended at the end, rounded down to a whole record so a partial
        // one is written over.
        let end = io::end(d.fd)?;
        end.checked_div(RECORD_OFF)
            .and_then(|n| n.checked_mul(RECORD_OFF))
            .unwrap_or(0)
    };
    // Writing a NULL entry is `write(fd, NULL, …)`: the kernel's EFAULT.
    let rec = FileRecord::of(data.ok_or(errno::EFAULT)?);
    let written = io::write_at(d.fd, &rec, at)?;
    if written != RECORD {
        if !found {
            io::truncate(d.fd, at);
        }
        return Err(errno::ENOSPC);
    }
    d.offset = at.saturating_add(RECORD_OFF);
    Ok(())
}

/// Close the database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endutxent() {
    // SAFETY: this process's state, one call at a time.
    unsafe { &mut *db() }.close();
}

/// Use `file` as the database from now on (glibc's `__utmpname`): the open
/// one is closed, and the name is kept.  0, or -1: `ENOMEM` when it cannot be
/// copied, `EFAULT` for a NULL name, which glibc faults comparing -- after it
/// has closed the file.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utmpxname(file: *const u8) -> i32 {
    // SAFETY: this process's state, one call at a time.
    let d = unsafe { &mut *db() };
    d.close();
    if file.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-NULL; both are NUL-terminated.
    let same = |a: *const u8, b: *const u8| unsafe { crate::string::strcmp(a, b) } == 0;
    if same(file, d.file_name()) {
        return 0;
    }
    let copy = if same(file, UTMPX_FILE.as_ptr()) {
        core::ptr::null_mut()
    } else {
        // SAFETY: a NUL-terminated name.
        let copy = unsafe { crate::string::strdup(file) };
        if copy.is_null() {
            // strdup's malloc set ENOMEM.
            return -1;
        }
        copy
    };
    // SAFETY: the old name is this module's own copy, or NULL.
    unsafe { crate::malloc::free(d.name) };
    d.name = copy;
    0
}

/// Append `utmpx` to the history file `file` (glibc's `updwtmp`): opened
/// write-only and locked; a partial record at its end cut off first, and the
/// record cut off again if it could not be written whole.  Nothing is
/// reported -- the call has no result -- and a file that cannot be opened is
/// left alone.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn updwtmpx(file: *const u8, utmpx: *const Utmpx) {
    let Ok(fd) = io::open(file, crate::fcntl::O_WRONLY | crate::fcntl::O_CLOEXEC) else {
        return;
    };
    if io::lock(fd, crate::fcntl_ops::F_WRLCK).is_ok() {
        append(fd, utmpx);
        io::unlock(fd);
    }
    io::close(fd);
}

/// The body of [`updwtmpx`]: glibc's `__libc_updwtmp` after the lock.
fn append(fd: crate::types::Fd, utmpx: *const Utmpx) {
    let Ok(end) = io::end(fd) else {
        return;
    };
    let whole = end
        .checked_div(RECORD_OFF)
        .and_then(|n| n.checked_mul(RECORD_OFF))
        .unwrap_or(0);
    if whole != end {
        io::truncate(fd, whole);
    }
    // SAFETY: non-NULL is the caller's entry; NULL is glibc's `write` of
    // NULL, which the kernel refuses -- so nothing is written.
    let Some(u) = (unsafe { utmpx.as_ref() }) else {
        return;
    };
    if io::write_at(fd, &FileRecord::of(u), whole) != Ok(RECORD) {
        io::truncate(fd, whole);
    }
}

// ---------------------------------------------------------------------------
// glibc's <utmp.h> names: the same calls, on the same structure
// ---------------------------------------------------------------------------

/// `setutent` — glibc's name for [`setutxent`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setutent() {
    setutxent();
}

/// `getutent` — glibc's name for [`getutxent`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutent() -> *mut Utmpx {
    getutxent()
}

/// `getutid` — glibc's name for [`getutxid`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutid(id: *const Utmpx) -> *mut Utmpx {
    getutxid(id)
}

/// `getutline` — glibc's name for [`getutxline`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getutline(line: *const Utmpx) -> *mut Utmpx {
    getutxline(line)
}

/// `pututline` — glibc's name for [`pututxline`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pututline(utmpx: *const Utmpx) -> *mut Utmpx {
    pututxline(utmpx)
}

/// `endutent` — glibc's name for [`endutxent`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endutent() {
    endutxent();
}

/// `utmpname` — glibc's name for [`utmpxname`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn utmpname(file: *const u8) -> i32 {
    utmpxname(file)
}

/// `updwtmp` — glibc's name for [`updwtmpx`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn updwtmp(file: *const u8, utmpx: *const Utmpx) {
    updwtmpx(file, utmpx);
}

// ---------------------------------------------------------------------------
// glibc's getutmp and getutmpx, and libutil's login, logout and logwtmp
// ---------------------------------------------------------------------------

/// The field a C `strncpy(field, s, sizeof field)` makes: `s`'s bytes up to
/// its NUL or the field's end, and NULs after -- no terminator when `s` fills
/// it, as in the file's records.
///
/// # Safety
///
/// `s` must be NULL (taken as "") or a C string.
unsafe fn strncpy_field(field: &mut [u8], s: *const u8) {
    field.fill(0);
    if s.is_null() {
        return;
    }
    for (i, slot) in field.iter_mut().enumerate() {
        // SAFETY: a C string, read up to its NUL.
        let b = unsafe { *s.add(i) };
        if b == 0 {
            break;
        }
        *slot = b;
    }
}

/// Now, as a record's time: glibc's `TIMESPEC_TO_TIMEVAL` of the real-time
/// clock.
fn now() -> UtmpxTimeval {
    let mut ts = crate::stat::Timespec::default();
    // A clock that cannot be read leaves the zero time, which glibc would
    // have written from its uninitialised stack instead.
    let _ = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut ts);
    UtmpxTimeval {
        tv_sec: ts.tv_sec,
        tv_usec: ts.tv_nsec / 1000,
    }
}

/// Copy the fields of one entry into another, field by field, as glibc's
/// `getutmp` and `getutmpx` do: the destination's reserved bytes are left
/// as they were.
fn copy_fields(from: &Utmpx, to: &mut Utmpx) {
    to.ut_type = from.ut_type;
    to.ut_pid = from.ut_pid;
    to.ut_line = from.ut_line;
    to.ut_id = from.ut_id;
    to.ut_user = from.ut_user;
    to.ut_host = from.ut_host;
    to.ut_exit = from.ut_exit;
    to.ut_session = from.ut_session;
    to.ut_tv = from.ut_tv;
    to.ut_addr_v6 = from.ut_addr_v6;
}

/// `getutmp(ux, u)` (glibc): copy a `struct utmpx` into a `struct utmp`.
/// In musl's headers they are one structure (`<utmp.h>` defines `utmp` as
/// `utmpx`), so every field comes across. A NULL pointer, which glibc's
/// dereferences, copies nothing.
///
/// # Safety
///
/// Each pointer must be NULL or a valid entry.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getutmp(ux: *const Utmpx, u: *mut Utmpx) {
    // SAFETY: the caller's entries.
    if let (Some(from), Some(to)) = unsafe { (ux.as_ref(), u.as_mut()) } {
        copy_fields(from, to);
    }
}

/// `getutmpx(u, ux)` (glibc): copy a `struct utmp` into a `struct utmpx` --
/// see [`getutmp`].
///
/// # Safety
///
/// As [`getutmp`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getutmpx(u: *const Utmpx, ux: *mut Utmpx) {
    // SAFETY: the caller's entries.
    if let (Some(from), Some(to)) = unsafe { (u.as_ref(), ux.as_mut()) } {
        copy_fields(from, to);
    }
}

// libutil's three are each an archive member of their own, as in glibc
// (login.o, logout.o, logwtmp.o): `login` and `logout` are names a program
// may well have for functions of its own, which must not meet these because
// it also reads the database. See string.rs's module header.

/// Own archive member -- see above.
mod gnu_login {
    use super::{
        USER_PROCESS, UTMPX_FILE, Utmpx, WTMPX_FILE, endutxent, pututxline, setutxent,
        strncpy_field, updwtmpx, utmpxname,
    };

    /// `login(ut)` (libutil): record a login -- `ut`, as a `USER_PROCESS`
    /// entry of this process, on the terminal of standard input (else
    /// output, else error) -- in the database, and append it to the
    /// history. With no terminal, the line is `???` and only the history is
    /// written. As glibc's, this leaves the database's name set to
    /// `_PATH_UTMP`. A NULL entry, which glibc's dereferences, records
    /// nothing.
    ///
    /// # Safety
    ///
    /// `ut` must be NULL or a valid entry.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn login(ut: *const Utmpx) {
        // SAFETY: the caller's entry.
        let Some(given) = (unsafe { ut.as_ref() }) else {
            return;
        };
        let mut copy = *given;
        copy.ut_type = USER_PROCESS;
        copy.ut_pid = crate::process::getpid();
        let mut tty = [0u8; crate::linux_limits::PATH_MAX];
        let named = [0, 1, 2]
            .into_iter()
            .any(|fd| crate::ioctl::ttyname_r(fd, tty.as_mut_ptr(), tty.len()) == 0);
        if named {
            let len = tty.iter().position(|&b| b == 0).unwrap_or(tty.len());
            let path = tty.get(..len).unwrap_or(&[]);
            // /dev/pts/3 is `pts/3`; elsewhere, the name after the last slash.
            let line = path.strip_prefix(b"/dev/").unwrap_or_else(|| {
                path.iter()
                    .rposition(|&b| b == b'/')
                    .and_then(|i| path.get(i.saturating_add(1)..))
                    .unwrap_or(path)
            });
            let mut name = [0u8; crate::linux_limits::PATH_MAX];
            if let Some(dst) = name.get_mut(..line.len()) {
                dst.copy_from_slice(line);
            }
            // SAFETY: `name` holds `line` and a NUL.
            unsafe { strncpy_field(&mut copy.ut_line, name.as_ptr()) };
            if utmpxname(UTMPX_FILE.as_ptr()) == 0 {
                setutxent();
                // What pututline does with the entry is the database's; glibc
                // does not look at its result either.
                let _ = pututxline(&raw const copy);
                endutxent();
            }
        } else {
            // SAFETY: a C string literal.
            unsafe { strncpy_field(&mut copy.ut_line, c"???".as_ptr().cast()) };
        }
        updwtmpx(WTMPX_FILE.as_ptr(), &raw const copy);
    }
}
pub use gnu_login::login;

/// Own archive member -- see above.
mod gnu_logout {
    use super::{
        DEAD_PROCESS, USER_PROCESS, UTMPX_FILE, Utmpx, endutxent, getutline_r, now, pututxline,
        setutxent, strncpy_field, utmpxname,
    };

    /// `logout(line)` (libutil): mark the database's login on `line` ended
    /// -- a `DEAD_PROCESS` entry, its user and host cleared, stamped now.
    /// 1 if an entry was found and rewritten, else 0. As glibc's, this
    /// leaves the database's name set to `_PATH_UTMP`. A NULL line, which
    /// glibc's reads, is 0.
    ///
    /// # Safety
    ///
    /// `line` must be NULL or a C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn logout(line: *const u8) -> i32 {
        if line.is_null() || utmpxname(UTMPX_FILE.as_ptr()) == -1 {
            return 0;
        }
        setutxent();
        let mut wanted = Utmpx::ZERO;
        wanted.ut_type = USER_PROCESS;
        // SAFETY: the caller's C string.
        unsafe { strncpy_field(&mut wanted.ut_line, line) };
        let mut buffer = Utmpx::ZERO;
        let mut found: *mut Utmpx = core::ptr::null_mut();
        let mut result = 0;
        // SAFETY: an entry, a buffer and a result pointer of this frame's.
        if unsafe { getutline_r(&raw const wanted, &raw mut buffer, &raw mut found) } >= 0 {
            // SAFETY: getutline_r answered with `buffer`.
            if let Some(ut) = unsafe { found.as_mut() } {
                ut.ut_user = [0; super::UT_NAMESIZE];
                ut.ut_host = [0; super::UT_HOSTSIZE];
                ut.ut_tv = now();
                ut.ut_type = DEAD_PROCESS;
                if !pututxline(ut).is_null() {
                    result = 1;
                }
            }
        }
        endutxent();
        result
    }
}
pub use gnu_logout::logout;

/// Own archive member -- see above.
mod gnu_logwtmp {
    use super::{DEAD_PROCESS, USER_PROCESS, Utmpx, WTMPX_FILE, now, strncpy_field, updwtmpx};

    /// `logwtmp(line, name, host)` (libutil): append to the history an
    /// entry for this process, now: a `USER_PROCESS` login on `line` by
    /// `name` from `host`, or with `name` empty a `DEAD_PROCESS` logout. A
    /// NULL argument, which glibc's reads, appends nothing.
    ///
    /// # Safety
    ///
    /// Each argument must be NULL or a C string.
    #[cfg_attr(target_os = "none", unsafe(no_mangle))]
    pub unsafe extern "C" fn logwtmp(line: *const u8, name: *const u8, host: *const u8) {
        if line.is_null() || name.is_null() || host.is_null() {
            return;
        }
        let mut ut = Utmpx::ZERO;
        ut.ut_pid = crate::process::getpid();
        // SAFETY: the caller's C strings.
        unsafe {
            ut.ut_type = if *name != 0 {
                USER_PROCESS
            } else {
                DEAD_PROCESS
            };
            strncpy_field(&mut ut.ut_line, line);
            strncpy_field(&mut ut.ut_user, name);
            strncpy_field(&mut ut.ut_host, host);
        }
        ut.ut_tv = now();
        updwtmpx(WTMPX_FILE.as_ptr(), &raw const ut);
    }
}
pub use gnu_logwtmp::logwtmp;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const DB: &[u8] = b"/var/run/utmp";

    fn entry(ut_type: i16, id: &[u8], line: &[u8], user: &[u8]) -> Utmpx {
        let mut u = Utmpx::ZERO;
        u.ut_type = ut_type;
        u.ut_id[..id.len()].copy_from_slice(id);
        u.ut_line[..line.len()].copy_from_slice(line);
        u.ut_user[..user.len()].copy_from_slice(user);
        u
    }

    fn record_bytes(u: &Utmpx) -> Vec<u8> {
        let rec = FileRecord::of(u);
        // SAFETY: a plain-integer struct viewed as its bytes.
        unsafe { core::slice::from_raw_parts((&raw const rec).cast::<u8>(), RECORD) }.to_vec()
    }

    fn file_of(entries: &[Utmpx]) -> Vec<u8> {
        entries.iter().flat_map(record_bytes).collect()
    }

    /// A fresh database with these entries, closed and at its start.
    fn fresh(entries: &[Utmpx]) {
        endutxent();
        assert_eq!(utmpxname(UTMPX_FILE.as_ptr()), 0);
        io::put_file(DB, &file_of(entries));
    }

    fn user_of(p: *const Utmpx) -> Vec<u8> {
        assert!(!p.is_null(), "an entry");
        // SAFETY: an entry this module handed back.
        let u = unsafe { &*p };
        u.ut_user.iter().copied().take_while(|&b| b != 0).collect()
    }

    fn records_in(path: &[u8]) -> Vec<(i16, Vec<u8>)> {
        let bytes = io::file(path).unwrap();
        assert_eq!(bytes.len() % RECORD, 0, "whole records");
        bytes
            .chunks(RECORD)
            .map(|c| {
                let mut rec = FileRecord::ZERO;
                // SAFETY: RECORD bytes into a plain-integer struct.
                unsafe {
                    core::ptr::copy_nonoverlapping(c.as_ptr(), (&raw mut rec).cast::<u8>(), RECORD);
                }
                let u = rec.entry();
                (
                    u.ut_type,
                    u.ut_user.iter().copied().take_while(|&b| b != 0).collect(),
                )
            })
            .collect()
    }

    // -- The structures --

    #[test]
    fn test_entry_type_values() {
        assert_eq!(
            [EMPTY, RUN_LVL, BOOT_TIME, NEW_TIME, OLD_TIME, INIT_PROCESS],
            [0, 1, 2, 3, 4, 5]
        );
        assert_eq!(
            [LOGIN_PROCESS, USER_PROCESS, DEAD_PROCESS, ACCOUNTING],
            [6, 7, 8, 9]
        );
        assert_eq!(
            (UT_NAMESIZE, UT_LINESIZE, UT_HOSTSIZE, UT_IDSIZE),
            (32, 32, 256, 4)
        );
    }

    #[test]
    fn test_utmpx_is_musls_400_bytes() {
        assert_eq!(size_of::<Utmpx>(), 400);
        assert_eq!(size_of::<UtmpxTimeval>(), 16);
    }

    /// The file's records are glibc's, at the offsets `utmpfile` reads.
    #[test]
    fn file_records_are_glibcs_384_bytes() {
        assert_eq!(RECORD, 384);
        let mut u = entry(USER_PROCESS, b"tty1", b"tty1", b"alice");
        u.ut_pid = 0x1234;
        u.ut_session = 7;
        u.ut_tv = UtmpxTimeval {
            tv_sec: 1_700_000_000,
            tv_usec: 250,
        };
        let b = record_bytes(&u);
        assert_eq!(&b[0..2], &USER_PROCESS.to_ne_bytes());
        assert_eq!(&b[4..8], &0x1234i32.to_ne_bytes());
        assert_eq!(&b[8..12], b"tty1");
        assert_eq!(&b[40..44], b"tty1");
        assert_eq!(&b[44..49], b"alice");
        assert_eq!(&b[336..340], &7i32.to_ne_bytes());
        assert_eq!(&b[340..344], &1_700_000_000i32.to_ne_bytes());
        assert_eq!(&b[344..348], &250i32.to_ne_bytes());
    }

    /// The file's time is 32-bit, as glibc's structure is: 2038 wraps.
    #[test]
    fn a_time_past_2038_wraps_in_the_file_as_it_does_in_glibc() {
        let mut u = Utmpx::ZERO;
        u.ut_tv.tv_sec = (1i64 << 31) + 5;
        let back = FileRecord::of(&u).entry();
        assert_eq!(back.ut_tv.tv_sec, -(1i64 << 31) + 5);
    }

    // -- Reading --

    #[test]
    fn a_missing_file_is_every_calls_enoent_and_is_not_created() {
        endutxent();
        assert_eq!(utmpxname(UTMPX_FILE.as_ptr()), 0);
        io::remove_file(DB);
        errno::set_errno(0);
        assert!(getutxent().is_null());
        assert_eq!(errno::get_errno(), errno::ENOENT);
        let u = entry(USER_PROCESS, b"1", b"tty1", b"alice");
        errno::set_errno(0);
        assert!(pututxline(&u).is_null());
        assert_eq!(errno::get_errno(), errno::ENOENT);
        assert!(io::file(DB).is_none(), "not created");
    }

    #[test]
    fn getutxent_walks_the_file_and_leaves_errno_at_the_end() {
        fresh(&[
            entry(BOOT_TIME, b"", b"~", b"reboot"),
            entry(USER_PROCESS, b"1", b"tty1", b"alice"),
        ]);
        assert_eq!(user_of(getutxent()), b"reboot");
        assert_eq!(user_of(getutxent()), b"alice");
        errno::set_errno(1234);
        assert!(getutxent().is_null());
        assert_eq!(errno::get_errno(), 1234, "unchanged at the end");
        setutxent();
        assert_eq!(user_of(getutxent()), b"reboot", "rewound");
        endutxent();
    }

    /// A partial record at the end is the end, as glibc reads it.
    #[test]
    fn a_partial_record_is_the_end() {
        let mut bytes = file_of(&[entry(USER_PROCESS, b"1", b"tty1", b"alice")]);
        bytes.extend_from_slice(&[7; 100]);
        endutxent();
        assert_eq!(utmpxname(UTMPX_FILE.as_ptr()), 0);
        io::put_file(DB, &bytes);
        assert_eq!(user_of(getutxent()), b"alice");
        assert!(getutxent().is_null());
        endutxent();
    }

    #[test]
    fn getutxid_matches_time_records_by_type_and_processes_by_id() {
        fresh(&[
            entry(BOOT_TIME, b"", b"~", b"reboot"),
            entry(LOGIN_PROCESS, b"2", b"tty2", b"LOGIN"),
            entry(USER_PROCESS, b"1", b"tty1", b"alice"),
        ]);
        let boot = entry(BOOT_TIME, b"", b"", b"");
        assert_eq!(user_of(getutxid(&boot)), b"reboot");
        // A process type matches any process type with the same id.
        setutxent();
        let dead = entry(DEAD_PROCESS, b"1", b"", b"");
        assert_eq!(user_of(getutxid(&dead)), b"alice");
        // No id: by line.
        setutxent();
        let by_line = entry(USER_PROCESS, b"", b"tty2", b"");
        assert_eq!(user_of(getutxid(&by_line)), b"LOGIN");
        // Read forward only: nothing after the last record.
        errno::set_errno(0);
        assert!(getutxid(&boot).is_null());
        assert_eq!(errno::get_errno(), errno::ESRCH);
        endutxent();
    }

    #[test]
    fn getutxline_finds_logins_on_the_line_only() {
        fresh(&[
            entry(DEAD_PROCESS, b"1", b"tty1", b"gone"),
            entry(USER_PROCESS, b"1", b"tty1", b"alice"),
        ]);
        let line = entry(EMPTY, b"", b"tty1", b"");
        assert_eq!(
            user_of(getutxline(&line)),
            b"alice",
            "the dead one is skipped"
        );
        errno::set_errno(0);
        assert!(getutxline(&line).is_null());
        assert_eq!(errno::get_errno(), errno::ESRCH);
        endutxent();
    }

    // -- Writing --

    #[test]
    fn pututxline_appends_then_overwrites_its_match() {
        fresh(&[entry(BOOT_TIME, b"", b"~", b"reboot")]);
        let login = entry(LOGIN_PROCESS, b"1", b"tty1", b"LOGIN");
        assert_eq!(
            pututxline(&login),
            (&raw const login).cast_mut(),
            "the caller's pointer"
        );
        assert_eq!(
            records_in(DB),
            [
                (BOOT_TIME, b"reboot".to_vec()),
                (LOGIN_PROCESS, b"LOGIN".to_vec())
            ]
        );
        // The same process logs in: its record is overwritten, not added.
        setutxent();
        let user = entry(USER_PROCESS, b"1", b"tty1", b"alice");
        assert!(!pututxline(&user).is_null());
        assert_eq!(
            records_in(DB),
            [
                (BOOT_TIME, b"reboot".to_vec()),
                (USER_PROCESS, b"alice".to_vec())
            ]
        );
        // And logs out.
        setutxent();
        let dead = entry(DEAD_PROCESS, b"1", b"tty1", b"");
        assert!(!pututxline(&dead).is_null());
        assert_eq!(records_in(DB)[1].0, DEAD_PROCESS);
        endutxent();
    }

    /// glibc tries the last record read first.
    #[test]
    fn pututxline_writes_over_the_record_just_read() {
        fresh(&[
            entry(USER_PROCESS, b"1", b"tty1", b"alice"),
            entry(USER_PROCESS, b"2", b"tty2", b"bob"),
        ]);
        let line = entry(EMPTY, b"", b"tty2", b"");
        assert_eq!(user_of(getutxline(&line)), b"bob");
        let dead = entry(DEAD_PROCESS, b"2", b"tty2", b"");
        assert!(!pututxline(&dead).is_null());
        let recs = records_in(DB);
        assert_eq!((recs.len(), recs[1].0), (2, DEAD_PROCESS));
        endutxent();
    }

    #[test]
    fn a_short_write_is_enospc_and_an_append_is_cut_back() {
        fresh(&[entry(BOOT_TIME, b"", b"~", b"reboot")]);
        io::short_write(Some(10));
        let u = entry(USER_PROCESS, b"1", b"tty1", b"alice");
        errno::set_errno(0);
        assert!(pututxline(&u).is_null());
        assert_eq!(errno::get_errno(), errno::ENOSPC);
        io::short_write(None);
        assert_eq!(
            io::file(DB).unwrap().len(),
            RECORD,
            "the partial record went"
        );
        endutxent();
    }

    /// A NULL entry is EFAULT where glibc first reads it.
    #[test]
    fn a_null_entry_is_efault_where_it_is_first_read() {
        // Nothing to compare it with: the write is where it is read.
        fresh(&[]);
        errno::set_errno(0);
        assert!(pututxline(core::ptr::null()).is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
        assert_eq!(io::file(DB).unwrap().len(), 0, "nothing written");
        // With a record, the first comparison.
        fresh(&[entry(USER_PROCESS, b"1", b"tty1", b"alice")]);
        errno::set_errno(0);
        assert!(getutxid(core::ptr::null()).is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // An empty file compares nothing: the end, not a fault.
        fresh(&[]);
        errno::set_errno(0);
        assert!(getutxid(core::ptr::null()).is_null());
        assert_eq!(errno::get_errno(), errno::ESRCH);
        // getutxline reads it only at a login record.
        fresh(&[entry(DEAD_PROCESS, b"1", b"tty1", b"gone")]);
        errno::set_errno(0);
        assert!(getutxline(core::ptr::null()).is_null());
        assert_eq!(errno::get_errno(), errno::ESRCH);
        endutxent();
    }

    // -- The name --

    #[test]
    fn utmpxname_switches_files_and_null_is_efault() {
        fresh(&[entry(USER_PROCESS, b"1", b"tty1", b"alice")]);
        io::put_file(
            b"/tmp/other",
            &file_of(&[entry(USER_PROCESS, b"1", b"tty1", b"bob")]),
        );
        assert_eq!(utmpxname(b"/tmp/other\0".as_ptr()), 0);
        assert_eq!(user_of(getutxent()), b"bob");
        assert_eq!(utmpxname(UTMPX_FILE.as_ptr()), 0);
        assert_eq!(user_of(getutxent()), b"alice", "back, and reopened");
        errno::set_errno(0);
        assert_eq!(utmpxname(core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        endutxent();
        assert_eq!(io::open_count(), 0, "nothing left open");
    }

    // -- The history --

    #[test]
    fn updwtmpx_appends_whole_records() {
        let wtmp = b"/var/log/wtmp";
        let mut bytes = file_of(&[entry(BOOT_TIME, b"", b"~", b"reboot")]);
        bytes.extend_from_slice(&[1; 50]); // a partial record, cut first
        io::put_file(wtmp, &bytes);
        let u = entry(USER_PROCESS, b"1", b"tty1", b"alice");
        updwtmpx(WTMPX_FILE.as_ptr(), &u);
        updwtmp(WTMPX_FILE.as_ptr(), core::ptr::null());
        assert_eq!(
            records_in(wtmp),
            [
                (BOOT_TIME, b"reboot".to_vec()),
                (USER_PROCESS, b"alice".to_vec())
            ]
        );
        io::remove_file(wtmp);
        updwtmpx(WTMPX_FILE.as_ptr(), &u);
        assert!(io::file(wtmp).is_none(), "not created");
        assert_eq!(io::open_count(), 0);
    }

    // -- getutmp, getutmpx, and libutil's login, logout, logwtmp --

    const WTMP: &[u8] = b"/var/log/wtmp";

    fn cstr_field(field: &[u8]) -> Vec<u8> {
        field.iter().copied().take_while(|&b| b != 0).collect()
    }

    /// The history's entries, whole.
    fn history() -> Vec<Utmpx> {
        io::file(WTMP)
            .unwrap()
            .chunks(RECORD)
            .map(|c| {
                let mut rec = FileRecord::ZERO;
                // SAFETY: RECORD bytes into a plain-integer struct.
                unsafe {
                    core::ptr::copy_nonoverlapping(c.as_ptr(), (&raw mut rec).cast::<u8>(), RECORD);
                }
                rec.entry()
            })
            .collect()
    }

    #[test]
    fn getutmp_and_getutmpx_copy_every_field_and_no_reserved_byte() {
        let mut from = entry(USER_PROCESS, b"p1", b"pts/1", b"alice");
        from.ut_pid = 42;
        from.ut_host[..4].copy_from_slice(b"host");
        from.ut_exit = [1, 2, 3, 4];
        from.ut_session = 7;
        from.ut_tv = UtmpxTimeval {
            tv_sec: 5,
            tv_usec: 6,
        };
        from.ut_addr_v6 = [1, 2, 3, 4];
        let mut to = Utmpx::ZERO;
        to._reserved = [9; 20];
        // SAFETY: entries of this frame's.
        unsafe { getutmp(&raw const from, &raw mut to) };
        assert_eq!(record_bytes(&to), record_bytes(&from));
        assert_eq!((to.ut_tv.tv_sec, to.ut_tv.tv_usec), (5, 6));
        assert_eq!(to._reserved, [9; 20], "glibc copies field by field");
        let mut back = Utmpx::ZERO;
        // SAFETY: as above; NULL copies nothing.
        unsafe {
            getutmpx(&raw const to, &raw mut back);
            getutmp(core::ptr::null(), &raw mut to);
            getutmpx(&raw const from, core::ptr::null_mut());
        }
        assert_eq!(record_bytes(&back), record_bytes(&from));
    }

    #[test]
    fn logwtmp_appends_a_login_and_a_logout() {
        io::put_file(WTMP, &[]);
        // SAFETY: C string literals; NULL appends nothing.
        unsafe {
            logwtmp(
                c"pts/2".as_ptr().cast(),
                c"bob".as_ptr().cast(),
                c"example.org".as_ptr().cast(),
            );
            logwtmp(
                c"pts/2".as_ptr().cast(),
                c"".as_ptr().cast(),
                c"".as_ptr().cast(),
            );
            logwtmp(core::ptr::null(), c"x".as_ptr().cast(), c"".as_ptr().cast());
        }
        let h = history();
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].ut_type, h[1].ut_type), (USER_PROCESS, DEAD_PROCESS));
        assert_eq!(cstr_field(&h[0].ut_user), b"bob");
        assert_eq!(cstr_field(&h[0].ut_line), b"pts/2");
        assert_eq!(cstr_field(&h[0].ut_host), b"example.org");
        assert_eq!(h[0].ut_pid, crate::process::getpid());
        assert!(h[0].ut_tv.tv_sec > 0, "stamped with the time");
        assert_eq!(cstr_field(&h[1].ut_user), b"");
        io::remove_file(WTMP);
    }

    /// On a terminal -- standard input here is the console, as a program's
    /// is on this system -- `login` records the entry, a `USER_PROCESS` of
    /// this process on the terminal's name less `/dev/`, in the database
    /// and the history.
    #[test]
    fn login_on_a_terminal_records_it_in_both() {
        fresh(&[]);
        io::put_file(WTMP, &[]);
        let ut = entry(LOGIN_PROCESS, b"c1", b"was", b"erin");
        // SAFETY: an entry of this frame's.
        unsafe { login(&raw const ut) };
        assert_eq!(records_in(DB), [(USER_PROCESS, b"erin".to_vec())]);
        let h = history();
        assert_eq!(h.len(), 1);
        assert_eq!(cstr_field(&h[0].ut_line), b"console");
        assert_eq!(h[0].ut_pid, crate::process::getpid());
        io::remove_file(WTMP);
    }

    /// With no terminal on standard input, output or error, `login` names
    /// the line `???` and writes the history alone, as glibc's does.
    #[test]
    fn login_with_no_terminal_writes_the_history_alone() {
        fresh(&[]);
        io::put_file(WTMP, &[]);
        // This thread's descriptors 0 to 2, closed for the call.
        let saved: Vec<_> = (0..3)
            .map(|fd| (fd, crate::fdtable::close_fd(fd)))
            .collect();
        let ut = entry(LOGIN_PROCESS, b"c1", b"was", b"carol");
        // SAFETY: an entry of this frame's; NULL records nothing.
        unsafe {
            login(&raw const ut);
            login(core::ptr::null());
        }
        for (fd, e) in saved {
            if let Some(e) = e {
                let _ = crate::fdtable::install_fd(fd, e.kind, e.handle);
            }
        }
        assert_eq!(records_in(DB), [], "the database is not written");
        let h = history();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].ut_type, USER_PROCESS);
        assert_eq!(h[0].ut_pid, crate::process::getpid());
        assert_eq!(cstr_field(&h[0].ut_line), b"???");
        assert_eq!(cstr_field(&h[0].ut_user), b"carol");
        io::remove_file(WTMP);
    }

    #[test]
    fn logout_marks_the_line_dead() {
        let mut on = entry(USER_PROCESS, b"p3", b"pts/3", b"dave");
        on.ut_host[..6].copy_from_slice(b"remote");
        fresh(&[entry(LOGIN_PROCESS, b"t1", b"tty1", b"LOGIN"), on]);
        // SAFETY: C string literals.
        assert_eq!(unsafe { logout(c"pts/3".as_ptr().cast()) }, 1);
        let bytes = io::file(DB).unwrap();
        let mut rec = FileRecord::ZERO;
        // SAFETY: RECORD bytes into a plain-integer struct.
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes[RECORD..].as_ptr(),
                (&raw mut rec).cast::<u8>(),
                RECORD,
            );
        }
        let dead = rec.entry();
        assert_eq!(dead.ut_type, DEAD_PROCESS);
        assert_eq!(cstr_field(&dead.ut_line), b"pts/3", "the line is kept");
        assert_eq!(
            (cstr_field(&dead.ut_user), cstr_field(&dead.ut_host)),
            (Vec::new(), Vec::new())
        );
        assert!(dead.ut_tv.tv_sec > 0, "stamped with the time");
        assert_eq!(
            records_in(DB)[0],
            (LOGIN_PROCESS, b"LOGIN".to_vec()),
            "the other untouched"
        );
        // SAFETY: as above.
        unsafe {
            assert_eq!(logout(c"pts/9".as_ptr().cast()), 0, "no login there");
            assert_eq!(logout(core::ptr::null()), 0);
            io::remove_file(DB);
            assert_eq!(logout(c"pts/3".as_ptr().cast()), 0, "no database");
        }
    }
}
