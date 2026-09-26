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
    Cursor, Fields, Held, Room, Which, c_bytes, enumerated, hold, is_compat, lines, lookup_result,
    next_entry, reentrant, string_or_null, with_text,
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
    const EMPTY: Self = Self {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getspnam_r(
    name: *const u8,
    spwd: *mut Spwd,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Spwd,
) -> i32 {
    if name.is_null() {
        errno::set_errno(errno::EFAULT);
        return errno::EFAULT;
    }
    // SAFETY: `name` is non-null and the caller's NUL-terminated string.
    let name = unsafe { c_bytes(name) };
    // SAFETY: the other pointers are the caller's, as documented.
    unsafe {
        reentrant(spwd, buf, buflen, result, |room| {
            with_text(Which::Shadow, ROOT_SHADOW_TEXT, |text| {
                lines(text, 0)
                    .filter_map(|(line, _)| parse_sp(line))
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
neg:*:-1:2147483648:-1::::
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
        // `(long int) (int)` of the 32-bit value: -1 stays -1, and 2^31
        // wraps as it does in glibc.
        assert_eq!((n.sp_lstchg, n.sp_min, n.sp_max), (-1, -2_147_483_648, -1));
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
}
