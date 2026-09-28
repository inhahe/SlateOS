//! `<shadow.h>` — the shadow password database: `/etc/shadow`, read as glibc
//! 2.39's `nss_files` reads it.
//!
//! `/etc/passwd` is world-readable, so it cannot hold password hashes; they
//! live in `/etc/shadow`, which only root can read. On SlateOS both are
//! generated from `/etc/users.yaml` (design-decisions §353), and this is the
//! C library's view of the shadow half, beside [`crate::pwd`]'s:
//!
//! - **Lines** are glibc's `sgetspent_r`: `name:hash:lstchg:min:max` and,
//!   in the current form, `:warn:inact:expire:flag`. An empty number is -1
//!   (`flag`, all ones) -- "not set" -- and a line that ends early or holds a
//!   number that is not one is no entry.
//! - **Privilege** is the file's: without the right to read `/etc/shadow`,
//!   `open` fails with `EACCES`, and so does the lookup -- `getspnam` returns
//!   NULL with that `errno`, `getspnam_r` returns it.
//! - Lookups, the `_r` forms and enumeration behave as [`crate::pwd`]'s do.
//!
//! ## With no `/etc/shadow`
//!
//! The database is its built-in entry: `root`, with **`sp_pwdp = "!"`** and
//! every aging field unset (design-decisions §1113). `"!"` is the standard
//! *locked account* marker. A password check compares `crypt(typed, hash)`
//! with the stored field, and no output of `crypt` ever begins with `!`, so a
//! locked entry is one **no input can satisfy**: the built-in entry cannot
//! grant access. An empty `sp_pwdp` would mean *no password required* and
//! authenticate anybody; reporting "no such user" would make a caller that
//! tells "unknown account" from "locked account" apart reach the wrong
//! conclusion about a user `getpwnam` says exists. `sp_lstchg` is -1 rather
//! than 0, because 0 means *the password must be changed at next login*.
//!
//! ## Why this exists
//!
//! Measured, not speculative: the CPython 3.12 cross-build
//! (`scripts/cpython-spike/`) links its `spwd` extension module into the
//! interpreter, and `getspnam`/`getspent`/`setspent`/`endspent` were four of
//! the six symbols standing between CPython and a successful link against our
//! `libc.a`. `login`, `su` and `passwd` want the same header.
//!
//! ## What changed on 2026-09-26
//!
//! The database was the built-in entry and nothing else, whatever
//! `/etc/shadow` held (`known-issues.md` -> `B-D-PWD-KNEW-ONLY-ROOT`).

use crate::errno;
use crate::nss_files::{
    Cursor, EntryWriter, Fields, Held, Room, Which, c_bytes, deliver, enumerated, fget_held,
    fget_reentrant, hold, is_compat, lines, lookup_result, next_entry, reentrant, string_or_null,
    valid_field, with_text,
};
use crate::perprocess::process_global;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Shadow password database entry (`struct spwd`).
///
/// Field order and types match glibc and musl exactly; a mismatch here is a
/// silent memory-corruption bug in every C caller, not a compile error.
#[repr(C)]
pub struct Spwd {
    /// Login name.
    pub sp_namp: *const u8,
    /// Encrypted password (`crypt(3)` output), or a lock marker.
    pub sp_pwdp: *const u8,
    /// Date of the last change, in days since the epoch; -1 unset.
    pub sp_lstchg: i64,
    /// Minimum days between changes; -1 unset.
    pub sp_min: i64,
    /// Maximum days between changes; -1 unset.
    pub sp_max: i64,
    /// Days of warning before the password expires; -1 unset.
    pub sp_warn: i64,
    /// Days after expiry until the account is disabled; -1 unset.
    pub sp_inact: i64,
    /// Date the account expires, in days since the epoch; -1 unset.
    pub sp_expire: i64,
    /// Reserved; all ones when unset.
    pub sp_flag: u64,
}

impl Spwd {
    pub(crate) const EMPTY: Self = Self {
        sp_namp: core::ptr::null(),
        sp_pwdp: core::ptr::null(),
        sp_lstchg: -1,
        sp_min: -1,
        sp_max: -1,
        sp_warn: -1,
        sp_inact: -1,
        sp_expire: -1,
        sp_flag: u64::MAX,
    };
}

/// `/etc/shadow` when there is none: root, locked, nothing set.
const ROOT_SHADOW_TEXT: &[u8] = b"root:!::::\n";

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// One `/etc/shadow` entry, borrowed from its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SpEntry<'a> {
    namp: &'a [u8],
    pwdp: Option<&'a [u8]>,
    lstchg: i64,
    min: i64,
    max: i64,
    warn: i64,
    inact: i64,
    expire: i64,
    flag: u64,
}

/// A number field as glibc converts it: `(long int) (int)` of the 32-bit
/// value, -1 for an empty field.
fn long(v: Option<u32>) -> i64 {
    v.map_or(-1, |v| i64::from(v as i32))
}

/// glibc's `sgetspent_r` `LINE_PARSER`: `None` for a line that is no entry.
fn parse_sp(line: &[u8]) -> Option<SpEntry<'_>> {
    let mut f = Fields::new(line);
    let namp = f.string();
    if is_compat(namp) && f.at_end() {
        return Some(SpEntry {
            namp,
            pwdp: None,
            lstchg: 0,
            min: 0,
            max: 0,
            warn: -1,
            inact: -1,
            expire: -1,
            flag: u64::MAX,
        });
    }
    let pwdp = f.string();
    let lstchg = long(f.int_or_empty().ok()?);
    let min = long(f.int_or_empty().ok()?);
    let max = long(f.int_or_empty().ok()?);
    f.skip_space();
    let (warn, inact, expire, flag) = if f.at_end() {
        // The old form, which ends after `max`.
        (-1, -1, -1, u64::MAX)
    } else {
        let warn = long(f.int_or_empty().ok()?);
        let inact = long(f.int_or_empty().ok()?);
        let expire = long(f.int_or_empty().ok()?);
        let flag = if f.at_end() {
            u64::MAX
        } else {
            f.last_int_or_empty().ok()?.map_or(u64::MAX, u64::from)
        };
        (warn, inact, expire, flag)
    };
    Some(SpEntry {
        namp,
        pwdp: Some(pwdp),
        lstchg,
        min,
        max,
        warn,
        inact,
        expire,
        flag,
    })
}

fn fill_sp(e: &SpEntry<'_>, room: &mut Room) -> Result<Spwd, i32> {
    Ok(Spwd {
        sp_namp: room.string(e.namp)?.cast_const(),
        sp_pwdp: string_or_null(room, e.pwdp)?,
        sp_lstchg: e.lstchg,
        sp_min: e.min,
        sp_max: e.max,
        sp_warn: e.warn,
        sp_inact: e.inact,
        sp_expire: e.expire,
        sp_flag: e.flag,
    })
}

// ---------------------------------------------------------------------------
// Lookup
// ---------------------------------------------------------------------------

process_global! {
    fn held_sp() -> Held<Spwd> = Held::new(Spwd::EMPTY);
    fn held_spent() -> Held<Spwd> = Held::new(Spwd::EMPTY);
    fn sp_cursor() -> Cursor = Cursor::CLOSED;
}

/// Look up a user's shadow entry (reentrant): as `getpwnam_r` -- 0 with
/// `*result` set or NULL, `ERANGE`, `EFAULT`, or the error reading
/// `/etc/shadow` (`EACCES` without the privilege).
///
/// A NULL `name` is `EFAULT` only where glibc's lookup first touches it:
/// comparing it with the file's first entry.  glibc opens `/etc/shadow` and
/// reads that entry before it looks at the name (nss_files' `DB_LOOKUP`;
/// nscd, whose client reads the name first for `getpwnam_r`, does not serve
/// shadow), so a file it cannot read answers with its own error, and one
/// with no entries is "not found".  Until 2026-09-26 the NULL came first.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getspnam_r(
    name: *const u8,
    spwd: *mut Spwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Spwd,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented; `name` is read
    // only once it is known not to be NULL.
    unsafe {
        reentrant(spwd, buf, buflen, result, |room| {
            with_text(Which::Shadow, ROOT_SHADOW_TEXT, |text| {
                let mut entries = lines(text, 0)
                    .filter_map(|(line, _)| parse_sp(line))
                    .peekable();
                if name.is_null() {
                    return if entries.peek().is_some() {
                        Err(errno::EFAULT)
                    } else {
                        Ok(None)
                    };
                }
                // SAFETY: non-null, and the caller's NUL-terminated string.
                let name = c_bytes(name);
                entries
                    .find(|e| !is_compat(e.namp) && e.namp == name)
                    .map(|e| fill_sp(&e, room))
                    .transpose()
            })?
        })
    }
}

/// Look up a user's shadow entry: the entry, or NULL -- `errno` 0 for no
/// such user, `EACCES` without the privilege.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getspnam(name: *const u8) -> *const Spwd {
    // SAFETY: `name` is passed on as given; the rest is this process's.
    lookup_result(hold(held_sp(), |p, b, l, r| unsafe {
        getspnam_r(name, p, b, l, r)
    }))
}

/// Rewind the shadow database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setspent() {
    // SAFETY: this process's cursor.
    unsafe { *sp_cursor() = Cursor::CLOSED };
}

/// The next shadow entry into the caller's buffer: 0, `ENOENT` at the end
/// (and for a database that cannot be read), or `ERANGE` -- after which the
/// same entry comes again.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getspent_r(
    spwd: *mut Spwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Spwd,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented.
    unsafe {
        next_entry(
            sp_cursor(),
            Which::Shadow,
            ROOT_SHADOW_TEXT,
            |line, room| parse_sp(line).map(|e| fill_sp(&e, room)),
            spwd,
            buf,
            buflen,
            result,
        )
    }
}

/// The next shadow entry, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getspent() -> *const Spwd {
    // SAFETY: this process's storage.
    enumerated(hold(held_spent(), |p, b, l, r| unsafe {
        getspent_r(p, b, l, r)
    }))
}

/// Close the shadow database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endspent() {
    // SAFETY: this process's cursor.
    unsafe { *sp_cursor() = Cursor::CLOSED };
}

// ---------------------------------------------------------------------------
// A caller's stream or string: fgetspent, sgetspent, putspent
// ---------------------------------------------------------------------------

process_global! {
    fn held_fsp() -> Held<Spwd> = Held::new(Spwd::EMPTY);
    fn held_ssp() -> Held<Spwd> = Held::new(Spwd::EMPTY);
}

/// The next `/etc/shadow`-format entry of `stream` into the caller's buffer
/// (GNU), as glibc's `fgetspent_r` reads it: 0 with `*result` set; `ENOENT`
/// at the end; `ERANGE` when the buffer cannot hold the line (the same entry
/// comes next time); the stream's error. `errno` is set to what is
/// returned, when that is not 0.
///
/// # Safety
///
/// `stream` is NULL or an open stream; non-null `spwd` and `result` are
/// writable, and `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetspent_r(
    stream: *mut u8,
    spwd: *mut Spwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Spwd,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented.
    unsafe {
        fget_reentrant(
            stream,
            |line, room| parse_sp(line).map(|e| fill_sp(&e, room)),
            spwd,
            buf,
            buflen,
            result,
        )
    }
}

/// The next `/etc/shadow`-format entry of `stream`, or NULL -- `errno`
/// `ENOENT` at the end; as `crate::pwd::fgetpwent`.
///
/// # Safety
///
/// `stream` is NULL or an open stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetspent(stream: *mut u8) -> *const Spwd {
    // SAFETY: `stream` as given; the storage is this process's.
    unsafe {
        fget_held(stream, held_fsp(), |line, room| {
            parse_sp(line).map(|e| fill_sp(&e, room))
        })
    }
}

/// `string` read as an `/etc/shadow` line, into the caller's structure and
/// buffer (GNU): 0 with `*result` set. The line ends at the string's first
/// newline, and white space before the name is part of it, as in glibc.
///
/// `ERANGE` when the buffer cannot hold the whole string -- glibc copies it
/// there first, and so asks that much -- with `errno` untouched, as
/// glibc's. A string that is no entry is `EINVAL`, and `errno` too, where
/// glibc returns whatever `errno` already held, 0 included, which reads as
/// success to a caller that checks the return (design-decisions §1137). A
/// NULL pointer is `EFAULT` (§303).
///
/// # Safety
///
/// `string` is NULL or a C string; non-null `spwd` and `result` are
/// writable, and `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sgetspent_r(
    string: *const u8,
    spwd: *mut Spwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Spwd,
) -> i32 {
    if !result.is_null() {
        // SAFETY: the caller's, non-null.
        unsafe { result.write(core::ptr::null()) };
    }
    if string.is_null() || spwd.is_null() || buf.is_null() || result.is_null() {
        errno::set_errno(errno::EFAULT);
        return errno::EFAULT;
    }
    // SAFETY: non-null, the caller's C string.
    let text = unsafe { c_bytes(string) };
    if text.len() >= buflen {
        return errno::ERANGE;
    }
    let line = text.split(|&b| b == b'\n').next().unwrap_or(&[]);
    // SAFETY: the caller gives `buflen` writable bytes at `buf`.
    let mut room = unsafe { Room::new(buf, buflen) };
    let rc = match parse_sp(line).map(|e| fill_sp(&e, &mut room)) {
        Some(Ok(entry)) => {
            // SAFETY: both the caller's, checked non-null above.
            unsafe { deliver(entry, spwd, result) };
            return 0;
        }
        // Cannot happen -- the fields are a part of the string the buffer
        // holds -- but passed on if it did.
        Some(Err(e)) => e,
        None => errno::EINVAL,
    };
    errno::set_errno(rc);
    rc
}

/// `string` read as an `/etc/shadow` line: the entry, this process's until
/// the next call, or NULL -- `errno` `EINVAL` for a string that is no entry
/// (glibc leaves `errno` as it was; see [`sgetspent_r`]).
///
/// # Safety
///
/// `string` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sgetspent(string: *const u8) -> *const Spwd {
    // SAFETY: `string` as given; the rest is this process's.
    match hold(held_ssp(), |p, b, l, r| unsafe {
        sgetspent_r(string, p, b, l, r)
    }) {
        Ok(p) => p,
        Err(e) => {
            errno::set_errno(e);
            core::ptr::null()
        }
    }
}

/// Write `p` to `stream` as an `/etc/shadow` line, as glibc's `putspent`
/// writes it: 0, or -1 with `errno`. A number of -1 -- a flag of all ones --
/// is written empty, "not set"; the flag is written signed, as glibc's
/// `%ld` writes it. `EINVAL` for a NULL name, or a name or password holding
/// a `:` or newline; a NULL `p` is `EFAULT` (§303) and a NULL stream
/// `EBADF` (§1120), where glibc would fault.
///
/// # Safety
///
/// `p` is NULL or a `struct spwd` whose non-null strings are C strings;
/// `stream` is NULL or an open stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn putspent(p: *const Spwd, stream: *mut u8) -> i32 {
    if p.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null, the caller's.
    let p = unsafe { &*p };
    if p.sp_namp.is_null() || !valid_field(p.sp_namp) || !valid_field(p.sp_pwdp) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if stream.is_null() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    let mut w = EntryWriter::new(stream);
    w.c_str_or_empty(p.sp_namp);
    w.bytes(b":");
    w.c_str_or_empty(p.sp_pwdp);
    w.bytes(b":");
    for v in [
        p.sp_lstchg,
        p.sp_min,
        p.sp_max,
        p.sp_warn,
        p.sp_inact,
        p.sp_expire,
    ] {
        if v != -1 {
            w.int(i128::from(v));
        }
        w.bytes(b":");
    }
    if p.sp_flag != u64::MAX {
        // `%ld`: the flag's bits as a signed number.
        w.int(i128::from(p.sp_flag.cast_signed()));
    }
    w.bytes(b"\n");
    w.finish()
}

// ---------------------------------------------------------------------------
// lckpwdf / ulckpwdf
// ---------------------------------------------------------------------------

/// The file `lckpwdf` locks.
const PWD_LOCKFILE: &[u8] = b"/etc/.pwd.lock\0";

/// How long `lckpwdf` waits for another process's lock: glibc's 15 seconds.
const LCKPWDF_TIMEOUT_NS: u64 = 15_000_000_000;

/// How long it sleeps between tries.
const LCKPWDF_POLL_NS: i64 = 10_000_000;

/// [`lock_state`] while `lckpwdf` is opening the file or waiting for the
/// lock: neither free (-1) nor held (a descriptor).
const LOCKING: i32 = -2;

process_global! {
    /// The descriptor the lock is held through, -1 when there is none, or
    /// [`LOCKING`] while `lckpwdf` is taking it.
    fn lock_state() -> core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(-1);
}

/// Take the lock the account files' writers share (`lckpwdf`): an exclusive
/// `fcntl` lock on the whole of `/etc/.pwd.lock`, which is created, mode
/// 0600, if it is not there. 0, or -1: when this process holds it already or
/// another thread is taking it (`errno` untouched, as glibc's), when the file
/// cannot be opened, or when another process holds the lock for 15 seconds
/// -- `EINTR` then, what glibc's alarm leaves.
///
/// glibc waits in `F_SETLKW` under `alarm(15)`, which cancels an alarm the
/// caller had set; this tries `F_SETLK` against the clock instead.
///
/// The lock excludes only as well as `fcntl`'s record locks do, and on
/// SlateOS those do not yet reach the kernel, so for now two processes can
/// both hold it (known-issues.md, the `fcntl_ops.rs` row;
/// requests/d-a-native-programs-cannot-reach-the-record-lock-table.md).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lckpwdf() -> i32 {
    use core::sync::atomic::Ordering::{AcqRel, Acquire, Release};
    // SAFETY: this process's state, an atomic.
    let state = unsafe { &*lock_state() };
    if state
        .compare_exchange(-1, LOCKING, AcqRel, Acquire)
        .is_err()
    {
        return -1;
    }
    let fd = crate::file::open(
        PWD_LOCKFILE.as_ptr(),
        crate::fcntl::O_WRONLY | crate::fcntl::O_CREAT | crate::fcntl::O_CLOEXEC,
        0o600,
    );
    if fd < 0 {
        state.store(-1, Release);
        return -1;
    }
    let whole_file = crate::fcntl_ops::Flock {
        l_type: crate::fcntl_ops::F_WRLCK,
        l_whence: 0, // SEEK_SET
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    let got = poll_lock(
        || {
            let arg = (&raw const whole_file) as i64;
            if crate::fcntl_ops::fcntl(fd, crate::fcntl_ops::F_SETLK, arg) == 0 {
                Ok(())
            } else {
                Err(errno::get_errno())
            }
        },
        monotonic_ns,
        || {
            let pause = crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: LCKPWDF_POLL_NS,
            };
            // A short or failed sleep only means an earlier retry.
            let _ = crate::time::nanosleep(&raw const pause, core::ptr::null_mut());
        },
        LCKPWDF_TIMEOUT_NS,
    );
    match got {
        Ok(()) => {
            state.store(fd, Release);
            0
        }
        Err(e) => {
            // The lock was not taken, so there is nothing for the close to
            // lose; its own error would only hide the one that matters.
            let _ = crate::file::close(fd);
            errno::set_errno(e);
            state.store(-1, Release);
            -1
        }
    }
}

/// Release [`lckpwdf`]'s lock by closing its descriptor: `close`'s answer,
/// or -1 (`errno` untouched) when this process does not hold it -- or
/// another thread is still taking it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ulckpwdf() -> i32 {
    use core::sync::atomic::Ordering::{AcqRel, Acquire};
    // SAFETY: this process's state, an atomic.
    let state = unsafe { &*lock_state() };
    let fd = state.load(Acquire);
    if fd < 0 || state.compare_exchange(fd, -1, AcqRel, Acquire).is_err() {
        return -1;
    }
    crate::file::close(fd)
}

/// Try `try_lock` until it succeeds; fails with anything but contention
/// (`EAGAIN`, `EACCES`); or `timeout_ns` have passed by `now` -- `EINTR`
/// then -- with `sleep` between tries. Apart so the tests can drive the
/// clock.
fn poll_lock(
    mut try_lock: impl FnMut() -> Result<(), i32>,
    mut now: impl FnMut() -> u64,
    mut sleep: impl FnMut(),
    timeout_ns: u64,
) -> Result<(), i32> {
    let deadline = now().saturating_add(timeout_ns);
    loop {
        match try_lock() {
            Ok(()) => return Ok(()),
            Err(e) if e == errno::EAGAIN || e == errno::EACCES => {}
            Err(e) => return Err(e),
        }
        if now() >= deadline {
            return Err(errno::EINTR);
        }
        sleep();
    }
}

/// `CLOCK_MONOTONIC` in nanoseconds; `u64::MAX` if the clock cannot be
/// read, which ends the wait at its first check rather than never.
fn monotonic_ns() -> u64 {
    let mut ts = crate::stat::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if crate::time::clock_gettime(crate::time::CLOCK_MONOTONIC, &raw mut ts) != 0 {
        return u64::MAX;
    }
    u64::try_from(ts.tv_sec)
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::try_from(ts.tv_nsec).unwrap_or(0))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::errno;

    fn cstr(p: *const u8) -> &'static [u8] {
        // SAFETY: every pointer this is applied to is a null-terminated string
        // owned either statically or by the test's own buffer.
        unsafe { core::ffi::CStr::from_ptr(p.cast()).to_bytes() }
    }

    #[test]
    fn getspnam_root_is_locked_not_empty() {
        let sp = unsafe { getspnam(b"root\0".as_ptr()) };
        assert!(!sp.is_null());
        let sp = unsafe { &*sp };
        assert_eq!(cstr(sp.sp_namp), b"root");
        // The whole security argument of this module: an empty hash would
        // authenticate anyone.
        assert_eq!(cstr(sp.sp_pwdp), b"!");
        assert_ne!(cstr(sp.sp_pwdp), b"");
    }

    #[test]
    fn getspnam_aging_fields_are_unset() {
        let sp = unsafe { &*getspnam(b"root\0".as_ptr()) };
        assert_eq!(sp.sp_lstchg, -1);
        assert_eq!(sp.sp_min, -1);
        assert_eq!(sp.sp_max, -1);
        assert_eq!(sp.sp_warn, -1);
        assert_eq!(sp.sp_inact, -1);
        assert_eq!(sp.sp_expire, -1);
        assert_eq!(sp.sp_flag, u64::MAX);
    }

    #[test]
    fn getspnam_unknown_user_is_null() {
        assert!(unsafe { getspnam(b"nobody\0".as_ptr()) }.is_null());
        assert!(unsafe { getspnam(b"\0".as_ptr()) }.is_null());
        assert!(unsafe { getspnam(core::ptr::null()) }.is_null());
    }

    #[test]
    fn getspnam_prefix_of_root_does_not_match() {
        // "rootkit" starts with "root"; a length-blind comparison would say yes.
        assert!(unsafe { getspnam(b"rootkit\0".as_ptr()) }.is_null());
        assert!(unsafe { getspnam(b"roo\0".as_ptr()) }.is_null());
    }

    #[test]
    fn spent_enumeration_yields_one_entry() {
        setspent();
        let first = getspent();
        assert!(!first.is_null());
        assert_eq!(cstr(unsafe { (*first).sp_namp }), b"root");
        assert!(getspent().is_null());
        // Still exhausted on a repeat call.
        assert!(getspent().is_null());
    }

    #[test]
    fn setspent_and_endspent_both_rewind() {
        setspent();
        assert!(!getspent().is_null());
        assert!(getspent().is_null());

        setspent();
        assert!(!getspent().is_null());

        endspent();
        assert!(!getspent().is_null());
        endspent();
    }

    #[test]
    fn getspnam_r_copies_into_caller_buffer() {
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        let rc = unsafe {
            getspnam_r(
                b"root\0".as_ptr(),
                &mut sp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(rc, 0);
        assert!(!result.is_null());
        assert_eq!(cstr(sp.sp_namp), b"root");
        assert_eq!(cstr(sp.sp_pwdp), b"!");
        // The strings must live in the caller's buffer, not in our statics —
        // that is the entire point of the _r form.
        assert!(core::ptr::eq(sp.sp_namp, buf.as_ptr()));
    }

    #[test]
    fn getspnam_r_unknown_user_is_not_an_error() {
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        let rc = unsafe {
            getspnam_r(
                b"nobody\0".as_ptr(),
                &mut sp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(rc, 0);
        assert!(result.is_null());
    }

    #[test]
    fn getspnam_r_short_buffer_is_erange() {
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 3];
        let mut result: *const Spwd = core::ptr::null();
        let rc = unsafe {
            getspnam_r(
                b"root\0".as_ptr(),
                &mut sp,
                buf.as_mut_ptr(),
                buf.len(),
                &mut result,
            )
        };
        assert_eq!(rc, errno::ERANGE);
        assert!(result.is_null());
    }

    #[test]
    fn getspnam_r_null_outputs_are_efault() {
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        let name = b"root\0".as_ptr();

        assert_eq!(
            unsafe {
                getspnam_r(
                    name,
                    &mut sp,
                    buf.as_mut_ptr(),
                    buf.len(),
                    core::ptr::null_mut(),
                )
            },
            errno::EFAULT
        );
        assert_eq!(
            unsafe {
                getspnam_r(
                    name,
                    core::ptr::null_mut(),
                    buf.as_mut_ptr(),
                    buf.len(),
                    &mut result,
                )
            },
            errno::EFAULT
        );
        assert_eq!(
            unsafe { getspnam_r(name, &mut sp, core::ptr::null_mut(), buf.len(), &mut result) },
            errno::EFAULT
        );
    }

    #[test]
    fn getspent_r_walks_then_reports_enoent() {
        setspent();
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();

        let rc = unsafe { getspent_r(&mut sp, buf.as_mut_ptr(), buf.len(), &mut result) };
        assert_eq!(rc, 0);
        assert!(!result.is_null());

        let rc = unsafe { getspent_r(&mut sp, buf.as_mut_ptr(), buf.len(), &mut result) };
        assert_eq!(rc, errno::ENOENT);
        assert!(result.is_null());
        endspent();
    }

    #[test]
    fn getspent_r_erange_does_not_consume_the_entry() {
        setspent();
        let mut sp: Spwd = unsafe { core::mem::zeroed() };
        let mut small = [0u8; 2];
        let mut big = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();

        let rc = unsafe { getspent_r(&mut sp, small.as_mut_ptr(), small.len(), &mut result) };
        assert_eq!(rc, errno::ERANGE);

        // Retrying with a large enough buffer must still yield root: a failed
        // read that swallowed the entry would silently drop database rows.
        let rc = unsafe { getspent_r(&mut sp, big.as_mut_ptr(), big.len(), &mut result) };
        assert_eq!(rc, 0);
        assert!(!result.is_null());
        assert_eq!(cstr(sp.sp_namp), b"root");
        endspent();
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn spwd_matches_the_c_abi_layout() {
        // 2 pointers + 6 signed longs + 1 unsigned long = 9 * 8. A mismatch
        // here corrupts memory in every C caller rather than failing to build.
        assert_eq!(core::mem::size_of::<Spwd>(), 72);
        assert_eq!(core::mem::align_of::<Spwd>(), 8);
    }

    /// The entry behind `p`, which must not be NULL.
    fn found(p: *const Spwd) -> &'static Spwd {
        assert!(
            !p.is_null(),
            "expected an entry, got NULL (errno {})",
            errno::get_errno()
        );
        // SAFETY: non-null, and the library's entry.
        unsafe { &*p }
    }

    // -- /etc/shadow itself --

    const SHADOW: &[u8] = b"\
root:$6$salt$hash:19000:0:99999:7:::
alice:!:19500::::::
old:x:1:2:3
neg:*:4294967295:2147483648:0::::
minus:*:-1:1:1::::
short:x:1:2
+nis
";

    #[test]
    fn getspnam_reads_etc_shadow() {
        crate::nss_files::set_test_text(Which::Shadow, Some(SHADOW));
        // SAFETY: a NUL-terminated name; the library's entry.
        let sp = found(unsafe { getspnam(b"root\0".as_ptr()) });
        assert_eq!(cstr(sp.sp_pwdp), b"$6$salt$hash");
        assert_eq!(
            (
                sp.sp_lstchg,
                sp.sp_min,
                sp.sp_max,
                sp.sp_warn,
                sp.sp_inact,
                sp.sp_expire,
                sp.sp_flag
            ),
            (19000, 0, 99999, 7, -1, -1, u64::MAX)
        );
        // SAFETY: as above.
        let a = found(unsafe { getspnam(b"alice\0".as_ptr()) });
        assert_eq!(
            (a.sp_lstchg, a.sp_min, a.sp_max, a.sp_warn),
            (19500, -1, -1, -1)
        );
    }

    #[test]
    fn a_line_that_ends_before_expire_is_no_entry() {
        // Eight fields: `warn` and `inact` are there, `expire` is not, and
        // glibc's INT_FIELD_MAYBE_NULL refuses a field the line ended before.
        crate::nss_files::set_test_text(Which::Shadow, Some(b"cut:x:1:2:3::\n"));
        // SAFETY: a NUL-terminated name.
        assert!(unsafe { getspnam(b"cut\0".as_ptr()) }.is_null());
    }

    #[test]
    fn the_old_form_ends_after_max() {
        crate::nss_files::set_test_text(Which::Shadow, Some(SHADOW));
        // SAFETY: a NUL-terminated name; the library's entry.
        let o = found(unsafe { getspnam(b"old\0".as_ptr()) });
        assert_eq!((o.sp_lstchg, o.sp_min, o.sp_max), (1, 2, 3));
        assert_eq!(
            (o.sp_warn, o.sp_inact, o.sp_expire, o.sp_flag),
            (-1, -1, -1, u64::MAX)
        );
        // A line that ends before `max` is no entry.
        // SAFETY: as above.
        assert!(unsafe { getspnam(b"short\0".as_ptr()) }.is_null());
    }

    #[test]
    fn numbers_are_converted_as_glibc_converts_them() {
        crate::nss_files::set_test_text(Which::Shadow, Some(SHADOW));
        // SAFETY: a NUL-terminated name; the library's entry.
        let n = found(unsafe { getspnam(b"neg\0".as_ptr()) });
        // `(long int) (int)` of the 32-bit value: 2^32 - 1 is -1, and 2^31
        // wraps as it does in glibc.
        assert_eq!((n.sp_lstchg, n.sp_min, n.sp_max), (-1, -2_147_483_648, 0));
        // A number past 32 bits -- `-1` written as such is one -- makes the
        // line no entry, as Debian's glibc has it (design-decisions §1136).
        // SAFETY: as above.
        assert!(unsafe { getspnam(b"minus\0".as_ptr()) }.is_null());
    }

    #[test]
    fn an_unreadable_shadow_file_is_its_error() {
        crate::nss_files::set_test_error(Which::Shadow, errno::EACCES);
        // SAFETY: a NUL-terminated name.
        assert!(unsafe { getspnam(b"root\0".as_ptr()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EACCES);
        let mut sp = Spwd::EMPTY;
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        // SAFETY: the caller's objects.
        let rc = unsafe {
            getspnam_r(
                b"root\0".as_ptr(),
                &mut sp,
                buf.as_mut_ptr(),
                64,
                &mut result,
            )
        };
        assert_eq!((rc, result), (errno::EACCES, core::ptr::null()));
        // Enumeration of an unavailable database is simply over.
        setspent();
        // SAFETY: as above.
        assert_eq!(
            unsafe { getspent_r(&mut sp, buf.as_mut_ptr(), 64, &mut result) },
            errno::ENOENT
        );
        endspent();
        crate::nss_files::set_test_text(Which::Shadow, None);
    }

    #[test]
    fn getspent_walks_the_file_nis_lines_included() {
        crate::nss_files::set_test_text(Which::Shadow, Some(SHADOW));
        setspent();
        let mut names = std::vec::Vec::new();
        loop {
            let p = getspent();
            if p.is_null() {
                break;
            }
            // SAFETY: the library's entry.
            names.push(cstr(unsafe { (*p).sp_namp }).to_vec());
        }
        endspent();
        let want: [&[u8]; 5] = [b"root", b"alice", b"old", b"neg", b"+nis"];
        assert_eq!(names, want.map(<[u8]>::to_vec));
        crate::nss_files::set_test_text(Which::Shadow, None);
    }

    #[test]
    fn a_null_name_is_efault() {
        // SAFETY: NULL is what is being tested.
        assert!(unsafe { getspnam(core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// glibc reads the file before the name: an unreadable file is its own
    /// error and an empty one is "not found", a NULL name notwithstanding.
    /// Both were EFAULT until 2026-09-26.
    #[test]
    fn a_null_name_comes_after_the_file() {
        let mut sp = Spwd::EMPTY;
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        let mut ask = || {
            // SAFETY: the caller's objects; NULL is what is being tested.
            unsafe {
                getspnam_r(
                    core::ptr::null(),
                    &mut sp,
                    buf.as_mut_ptr(),
                    64,
                    &mut result,
                )
            }
        };
        crate::nss_files::set_test_error(Which::Shadow, errno::EACCES);
        assert_eq!(ask(), errno::EACCES);
        crate::nss_files::set_test_text(Which::Shadow, Some(b""));
        assert_eq!(ask(), 0, "no entry to compare the name with");
        crate::nss_files::set_test_text(Which::Shadow, Some(b"alice:!:19500::::::\n"));
        assert_eq!(ask(), errno::EFAULT, "the first comparison");
        crate::nss_files::set_test_text(Which::Shadow, None);
        assert!(result.is_null());
    }

    // -- sgetspent_r's buffer, putspent's NULLs --

    #[test]
    fn sgetspent_r_wants_room_for_the_whole_string_as_glibc_does() {
        let s = b"root:!:19000:0:99999:7:::\0";
        let len = s.len() - 1;
        let mut sp = Spwd::EMPTY;
        let mut buf = [0u8; 64];
        let mut result: *const Spwd = core::ptr::null();
        for (buflen, want) in [(len, errno::ERANGE), (len + 1, 0), (0, errno::ERANGE)] {
            errno::set_errno(1234);
            // SAFETY: a C string; `buf` holds 64 bytes.
            let rc = unsafe {
                sgetspent_r(
                    s.as_ptr(),
                    &raw mut sp,
                    buf.as_mut_ptr(),
                    buflen,
                    &raw mut result,
                )
            };
            assert_eq!(rc, want, "buflen {buflen}");
            if want == errno::ERANGE {
                assert_eq!(errno::get_errno(), 1234, "glibc leaves errno as it was");
                assert!(result.is_null());
            }
        }
        // SAFETY: non-null pointers; a NULL string is what is tested.
        let rc = unsafe {
            sgetspent_r(
                core::ptr::null(),
                &raw mut sp,
                buf.as_mut_ptr(),
                64,
                &raw mut result,
            )
        };
        assert_eq!(rc, errno::EFAULT);
    }

    #[test]
    fn putspent_refuses_what_glibc_would_fault_on() {
        // SAFETY: NULL is what is tested; the stream is never reached.
        assert_eq!(
            unsafe { putspent(core::ptr::null(), core::ptr::null_mut()) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let sp = Spwd {
            sp_namp: c"u".as_ptr().cast(),
            ..Spwd::EMPTY
        };
        // SAFETY: a record of C strings; a NULL stream is what is tested.
        assert_eq!(
            unsafe { putspent(&raw const sp, core::ptr::null_mut()) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EBADF);
        let bad = Spwd {
            sp_namp: c"a:b".as_ptr().cast(),
            ..Spwd::EMPTY
        };
        // SAFETY: as above: the fields are judged before the stream.
        assert_eq!(
            unsafe { putspent(&raw const bad, core::ptr::null_mut()) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- lckpwdf --

    #[test]
    fn the_lock_wait_ends_at_the_lock_or_the_deadline() {
        use core::cell::Cell;
        let clock = Cell::new(0u64);
        let tries = Cell::new(0u32);
        // Contended throughout: EINTR once the 15 seconds are up, as
        // glibc's alarm leaves it -- after trying all the while.
        let got = poll_lock(
            || {
                tries.set(tries.get() + 1);
                Err(errno::EAGAIN)
            },
            || clock.get(),
            || clock.set(clock.get() + 10_000_000),
            LCKPWDF_TIMEOUT_NS,
        );
        assert_eq!(got, Err(errno::EINTR));
        assert_eq!(
            tries.get(),
            1501,
            "every 10 ms for 15 s, and once at the start"
        );
        // Granted on the third try.
        tries.set(0);
        let got = poll_lock(
            || {
                tries.set(tries.get() + 1);
                if tries.get() == 3 {
                    Ok(())
                } else {
                    Err(errno::EACCES)
                }
            },
            || 0,
            || {},
            LCKPWDF_TIMEOUT_NS,
        );
        assert_eq!((got, tries.get()), (Ok(()), 3));
        // Any other failure is the answer at once.
        let got = poll_lock(|| Err(errno::EBADF), || 0, || {}, LCKPWDF_TIMEOUT_NS);
        assert_eq!(got, Err(errno::EBADF));
    }

    #[test]
    fn a_lock_held_or_being_taken_is_not_taken_again() {
        use core::sync::atomic::Ordering::SeqCst;
        // SAFETY: this thread's state.
        let state = unsafe { &*lock_state() };
        for held in [LOCKING, 99] {
            state.store(held, SeqCst);
            errno::set_errno(1234);
            assert_eq!(lckpwdf(), -1);
            assert_eq!(errno::get_errno(), 1234, "glibc leaves errno as it was");
            assert_eq!(state.load(SeqCst), held, "and the lock where it was");
        }
        // Released while another thread is still taking it: not ours to drop.
        state.store(LOCKING, SeqCst);
        assert_eq!(ulckpwdf(), -1);
        assert_eq!(state.load(SeqCst), LOCKING);
        state.store(-1, SeqCst);
        errno::set_errno(1234);
        assert_eq!(ulckpwdf(), -1, "nothing held");
        assert_eq!(errno::get_errno(), 1234);
    }

    #[test]
    fn a_lock_file_that_cannot_be_opened_leaves_the_lock_free() {
        use core::sync::atomic::Ordering::SeqCst;
        // The host has no /etc to create the file in: the open fails, and
        // the next call tries again rather than finding the lock "taken".
        // SAFETY: this thread's state.
        let state = unsafe { &*lock_state() };
        state.store(-1, SeqCst);
        errno::set_errno(0);
        assert_eq!(lckpwdf(), -1);
        assert_ne!(errno::get_errno(), 0, "the open's own errno");
        assert_eq!(state.load(SeqCst), -1);
    }
}
