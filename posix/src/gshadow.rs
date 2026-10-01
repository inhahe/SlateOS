//! `<gshadow.h>` — the shadow group database: `/etc/gshadow`, read as glibc
//! 2.39's `nss_files` reads it.
//!
//! `/etc/group` is world-readable, so it cannot hold a group's password
//! hash or say who may administer the group; those live in `/etc/gshadow`,
//! which only root can read.  This is the C library's view of it, beside
//! [`crate::pwd`]'s of `/etc/group` and [`crate::shadow`]'s of the users'
//! shadow file, and it works as they do:
//!
//! - **Lines** are glibc's `sgetsgent_r`: `name:password:admins:members`,
//!   each list split at `,` with the white space before a name dropped (not
//!   the white space after it) and empty names dropped.  The member list is
//!   the rest of the line, `:` and all, as glibc's.  A NIS compat name (`+`
//!   or `-` first) with nothing after it has no password and no admin list.
//! - **Privilege** is the file's: without the right to read `/etc/gshadow`
//!   the lookup fails with `open`'s `EACCES`.
//! - With **no `/etc/gshadow`** the database is empty -- no group has a
//!   shadow entry -- and a lookup finds nothing.  (SlateOS makes no
//!   `/etc/gshadow` of its own: `/etc/users.yaml`, design-decisions §353,
//!   gives groups no passwords.)
//! - Lookups, the `_r` forms, enumeration and `fgetsgent` behave as
//!   [`crate::shadow`]'s do.
//!
//! glibc 2.39's answers -- `sgetsgent` over 27 lines, its `_r` form at
//! buffer sizes around each one's need, `fgetsgent` and `fgetsgent_r` over a
//! file of entries, blank lines and comments, and `putsgent` over 15 entries
//! -- are `gshadow_oracle.txt` (`posix/tools/oracle/gshadow_harness.py`),
//! which the tests replay.  One difference, as in [`crate::shadow`]: an `_r`
//! form sets `*result` to NULL on every failure, where glibc's
//! `sgetsgent_r` leaves it as it was when the string does not fit the
//! buffer.

use crate::errno;
use crate::nss_files::{
    Cursor, EntryWriter, Fields, Held, Room, Which, c_bytes, deliver, enumerated, fget_held,
    fget_reentrant, hold, is_compat, is_space, lines, lookup_result, next_entry, reentrant,
    string_or_null, valid_field, valid_list_field, with_text,
};
use crate::perprocess::process_global;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Shadow group database entry (`struct sgrp`), glibc's layout.
#[repr(C)]
pub struct Sgrp {
    /// The group's name.
    pub sg_namp: *const u8,
    /// Its encrypted password, a lock marker, or NULL (a compat entry).
    pub sg_passwd: *const u8,
    /// Its administrators, a NULL-terminated list -- or NULL (a compat
    /// entry).
    pub sg_adm: *const *const u8,
    /// Its members, a NULL-terminated list.
    pub sg_mem: *const *const u8,
}

impl Sgrp {
    pub(crate) const EMPTY: Self = Self {
        sg_namp: core::ptr::null(),
        sg_passwd: core::ptr::null(),
        sg_adm: core::ptr::null(),
        sg_mem: core::ptr::null(),
    };
}

/// `/etc/gshadow` when there is none: no entries.
const NO_GSHADOW_TEXT: &[u8] = b"";

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// One `/etc/gshadow` entry, borrowed from its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SgEntry<'a> {
    namp: &'a [u8],
    passwd: Option<&'a [u8]>,
    /// The admin list as written, or `None` for a compat entry.
    adm: Option<&'a [u8]>,
    /// The member list as written: the rest of the line.
    mem: &'a [u8],
}

/// glibc's `sgetsgent_r` `LINE_PARSER`.  Every line is an entry: nothing
/// in it is a number that could fail to be one.
fn parse_sg(line: &[u8]) -> SgEntry<'_> {
    let mut f = Fields::new(line);
    let namp = f.string();
    if f.at_end() && is_compat(namp) {
        return SgEntry {
            namp,
            passwd: None,
            adm: None,
            mem: f.rest(),
        };
    }
    let passwd = f.string();
    let adm = f.string();
    SgEntry {
        namp,
        passwd: Some(passwd),
        adm: Some(adm),
        mem: f.rest(),
    }
}

/// `parse_list`'s names: split at `,`, each without the white space before
/// it, the empty ones dropped.
fn names(list: &[u8]) -> impl Iterator<Item = &[u8]> {
    list.split(|&b| b == b',').filter_map(|m| {
        let start = m.iter().position(|&b| !is_space(b))?;
        m.get(start..)
    })
}

/// Room for a NULL-terminated vector of `list`'s names, NULLs for now.
fn vector(room: &mut Room, list: &[u8]) -> Result<*mut *const u8, i32> {
    room.pointers(names(list).count().checked_add(1).ok_or(errno::ERANGE)?)
}

/// `list`'s names into `room`, their addresses into `v`, which
/// [`vector`] made for them.
fn fill_vector(room: &mut Room, v: *mut *const u8, list: &[u8]) -> Result<(), i32> {
    for (i, name) in names(list).enumerate() {
        let s = room.string(name)?;
        // SAFETY: `v` holds one pointer for each name, and a NULL after.
        unsafe { v.add(i).write(s.cast_const()) };
    }
    Ok(())
}

/// The entry in `room`: the two vectors first, while the buffer is aligned
/// for pointers, then the strings -- so that no entry needs more room than
/// glibc's, which lays out the line and then its vectors: one that fits a
/// buffer there fits it here.
fn fill_sg(e: &SgEntry<'_>, room: &mut Room) -> Result<Sgrp, i32> {
    let adm = match e.adm {
        Some(list) => Some((vector(room, list)?, list)),
        None => None,
    };
    let mem = vector(room, e.mem)?;
    let namp = room.string(e.namp)?.cast_const();
    let passwd = string_or_null(room, e.passwd)?;
    if let Some((v, list)) = adm {
        fill_vector(room, v, list)?;
    }
    fill_vector(room, mem, e.mem)?;
    Ok(Sgrp {
        sg_namp: namp,
        sg_passwd: passwd,
        sg_adm: adm.map_or(core::ptr::null(), |(v, _)| v.cast_const()),
        sg_mem: mem.cast_const(),
    })
}

/// A line's entry, filled into `room`, for the stream and database readers.
fn take(line: &[u8], room: &mut Room) -> Option<Result<Sgrp, i32>> {
    Some(fill_sg(&parse_sg(line), room))
}

// ---------------------------------------------------------------------------
// Lookup and enumeration
// ---------------------------------------------------------------------------

process_global! {
    fn held_sg() -> Held<Sgrp> = Held::new(Sgrp::EMPTY);
    fn held_sgent() -> Held<Sgrp> = Held::new(Sgrp::EMPTY);
    fn held_fsg() -> Held<Sgrp> = Held::new(Sgrp::EMPTY);
    fn held_ssg() -> Held<Sgrp> = Held::new(Sgrp::EMPTY);
    fn sg_cursor() -> Cursor = Cursor::CLOSED;
}

/// Look up a group's shadow entry (reentrant): 0 with `*result` the entry
/// or NULL, `ERANGE`, `EFAULT`, or the error reading `/etc/gshadow`
/// (`EACCES` without the privilege) -- as [`crate::shadow::getspnam_r`],
/// whose NULL name is judged where glibc's lookup first touches it.
///
/// # Safety
///
/// `name` is NULL or a C string; non-null `sgrp` and `result` are writable,
/// and `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getsgnam_r(
    name: *const u8,
    sgrp: *mut Sgrp,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Sgrp,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented; `name` is read
    // only once it is known not to be NULL.
    unsafe {
        reentrant(sgrp, buf, buflen, result, |room| {
            with_text(Which::Gshadow, NO_GSHADOW_TEXT, |text| {
                let mut entries = lines(text, 0).map(|(line, _)| parse_sg(line)).peekable();
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
                    .map(|e| fill_sg(&e, room))
                    .transpose()
            })?
        })
    }
}

/// Look up a group's shadow entry: the entry, or NULL -- `errno` 0 for no
/// such group, `EACCES` without the privilege.
///
/// # Safety
///
/// `name` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getsgnam(name: *const u8) -> *const Sgrp {
    // SAFETY: `name` is passed on as given; the rest is this process's.
    lookup_result(hold(held_sg(), |p, b, l, r| unsafe {
        getsgnam_r(name, p, b, l, r)
    }))
}

/// Rewind the shadow group database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setsgent() {
    // SAFETY: this process's cursor.
    unsafe { *sg_cursor() = Cursor::CLOSED };
}

/// The next shadow group entry into the caller's buffer: 0, `ENOENT` at the
/// end (and for a database that cannot be read), or `ERANGE` -- after which
/// the same entry comes again.
///
/// # Safety
///
/// Non-null `sgrp` and `result` are writable, and `buf` writable for
/// `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getsgent_r(
    sgrp: *mut Sgrp,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Sgrp,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented.
    unsafe {
        next_entry(
            sg_cursor(),
            Which::Gshadow,
            NO_GSHADOW_TEXT,
            take,
            sgrp,
            buf,
            buflen,
            result,
        )
    }
}

/// The next shadow group entry, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getsgent() -> *const Sgrp {
    // SAFETY: this process's storage.
    enumerated(hold(held_sgent(), |p, b, l, r| unsafe {
        getsgent_r(p, b, l, r)
    }))
}

/// Close the shadow group database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endsgent() {
    // SAFETY: this process's cursor.
    unsafe { *sg_cursor() = Cursor::CLOSED };
}

// ---------------------------------------------------------------------------
// A caller's stream or string: fgetsgent, sgetsgent, putsgent
// ---------------------------------------------------------------------------

/// The next `/etc/gshadow`-format entry of `stream` into the caller's
/// buffer (GNU), as glibc's `fgetsgent_r` reads it -- blank lines, lines of
/// white space and `#` comments skipped, white space before the name
/// dropped: 0 with `*result` set; `ENOENT` at the end; `ERANGE` when the
/// buffer cannot hold the line (the same entry comes next time); the
/// stream's error.
///
/// # Safety
///
/// `stream` is NULL or an open stream; non-null `sgrp` and `result` are
/// writable, and `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetsgent_r(
    stream: *mut u8,
    sgrp: *mut Sgrp,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Sgrp,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented.
    unsafe { fget_reentrant(stream, take, sgrp, buf, buflen, result) }
}

/// The next `/etc/gshadow`-format entry of `stream`, or NULL -- `errno`
/// `ENOENT` at the end.
///
/// # Safety
///
/// `stream` is NULL or an open stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fgetsgent(stream: *mut u8) -> *const Sgrp {
    // SAFETY: `stream` as given; the storage is this process's.
    unsafe { fget_held(stream, held_fsg(), take) }
}

/// `string` read as an `/etc/gshadow` line, into the caller's structure and
/// buffer (GNU): 0 with `*result` set.  The line ends at the string's first
/// newline, and white space before the name is part of it, as in glibc.
/// `ERANGE` when the buffer cannot hold the string -- glibc copies it there
/// first, and so asks that much -- or its entry; a NULL pointer is `EFAULT`
/// (§303).
///
/// # Safety
///
/// `string` is NULL or a C string; non-null `sgrp` and `result` are
/// writable, and `buf` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sgetsgent_r(
    string: *const u8,
    sgrp: *mut Sgrp,
    buf: *mut u8,
    buflen: usize,
    result: *mut *const Sgrp,
) -> i32 {
    if !result.is_null() {
        // SAFETY: the caller's, non-null.
        unsafe { result.write(core::ptr::null()) };
    }
    if string.is_null() || sgrp.is_null() || buf.is_null() || result.is_null() {
        errno::set_errno(errno::EFAULT);
        return errno::EFAULT;
    }
    // SAFETY: non-null, the caller's C string.
    let text = unsafe { c_bytes(string) };
    if text.len() >= buflen {
        errno::set_errno(errno::ERANGE);
        return errno::ERANGE;
    }
    let line = text.split(|&b| b == b'\n').next().unwrap_or(&[]);
    // SAFETY: the caller gives `buflen` writable bytes at `buf`.
    let mut room = unsafe { Room::new(buf, buflen) };
    match fill_sg(&parse_sg(line), &mut room) {
        Ok(entry) => {
            // SAFETY: both the caller's, checked non-null above.
            unsafe { deliver(entry, sgrp, result) };
            0
        }
        Err(e) => {
            errno::set_errno(e);
            e
        }
    }
}

/// `string` read as an `/etc/gshadow` line: the entry, this process's until
/// the next call, or NULL with `errno`.
///
/// # Safety
///
/// `string` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn sgetsgent(string: *const u8) -> *const Sgrp {
    // SAFETY: `string` as given; the rest is this process's.
    match hold(held_ssg(), |p, b, l, r| unsafe {
        sgetsgent_r(string, p, b, l, r)
    }) {
        Ok(p) => p,
        Err(e) => {
            errno::set_errno(e);
            core::ptr::null()
        }
    }
}

/// Write `g` to `stream` as an `/etc/gshadow` line, as glibc's `putsgent`
/// writes it: 0, or -1 with `errno`.  A NULL password or list is written
/// empty.  `EINVAL` for a NULL name, a name or password holding a `:` or a
/// newline, or a list name holding one of those or the `,` between names; a
/// NULL `g` is `EFAULT` (§303) and a NULL stream `EBADF` (§1120), where
/// glibc would fault.
///
/// # Safety
///
/// `g` is NULL or a `struct sgrp` whose non-null strings are C strings and
/// whose non-null lists are NULL-terminated arrays of them; `stream` is NULL
/// or an open stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn putsgent(g: *const Sgrp, stream: *mut u8) -> i32 {
    if g.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: non-null, the caller's.
    let g = unsafe { &*g };
    // SAFETY: the lists are the caller's, as documented.
    let lists_valid = unsafe { valid_list_field(g.sg_adm) && valid_list_field(g.sg_mem) };
    if g.sg_namp.is_null() || !valid_field(g.sg_namp) || !valid_field(g.sg_passwd) || !lists_valid {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if stream.is_null() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    let mut w = EntryWriter::new(stream);
    w.c_str_or_empty(g.sg_namp);
    w.bytes(b":");
    w.c_str_or_empty(g.sg_passwd);
    w.bytes(b":");
    // SAFETY: as above.
    unsafe { write_list(&mut w, g.sg_adm) };
    w.bytes(b":");
    // SAFETY: as above.
    unsafe { write_list(&mut w, g.sg_mem) };
    w.bytes(b"\n");
    w.finish()
}

/// A list's names, `,` between them; nothing for a NULL list.
///
/// # Safety
///
/// `list` is NULL or a NULL-terminated array of C strings.
unsafe fn write_list(w: &mut EntryWriter, list: *const *const u8) {
    if list.is_null() {
        return;
    }
    let mut at = list;
    let mut first = true;
    loop {
        // SAFETY: the array is NULL-terminated, and `at` has not passed it.
        let name = unsafe { at.read() };
        if name.is_null() {
            return;
        }
        if !first {
            w.bytes(b",");
        }
        w.c_str_or_empty(name);
        first = false;
        // SAFETY: not past the terminator, which is still ahead.
        at = unsafe { at.add(1) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nss_files::{set_test_error, set_test_text};
    use std::boxed::Box;
    use std::ffi::CString;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers (posix/tools/oracle/gshadow_harness.py).
    const ORACLE: &str = include_str!("gshadow_oracle.txt");

    /// The harness's `errno` before each call.
    const KEPT: i32 = 12345;

    /// The file the harness reads with `fgetsgent`.
    const FILE: &[u8] = b"\n# comment\nwheel:!:root:root,ann\n\n   \nbad\nstaff:x::bob\n  indented:x:a:b\n#last:x::\nlast:*::\n";

    fn errno_word(e: i32) -> String {
        match e {
            0 => "0".into(),
            KEPT => "kept".into(),
            errno::EINVAL => "EINVAL".into(),
            errno::ERANGE => "ERANGE".into(),
            errno::ENOENT => "ENOENT".into(),
            errno::EBADF => "EBADF".into(),
            e => format!("e{e}"),
        }
    }

    /// A string as the harness's `str` writes it.
    fn show(p: *const u8) -> String {
        if p.is_null() {
            return "(null)".into();
        }
        // SAFETY: the entry's C string.
        let bytes = unsafe { c_bytes(p) };
        let mut s = String::new();
        for &b in bytes {
            match b {
                b'\\' => s.push_str("\\\\"),
                b'\n' => s.push_str("\\n"),
                b'\t' => s.push_str("\\t"),
                0x20..=0x7e => s.push(b as char),
                _ => s.push_str(&format!("\\x{b:02x}")),
            }
        }
        s
    }

    fn show_list(l: *const *const u8) -> String {
        if l.is_null() {
            return "(null)".into();
        }
        let mut names = Vec::new();
        let mut at = l;
        loop {
            // SAFETY: the entry's NULL-terminated list.
            let p = unsafe { at.read() };
            if p.is_null() {
                break;
            }
            names.push(show(p));
            // SAFETY: as above.
            at = unsafe { at.add(1) };
        }
        format!("[{}]", names.join(","))
    }

    fn show_entry(g: &Sgrp) -> String {
        format!(
            "{}|{}|{}|{}",
            show(g.sg_namp),
            show(g.sg_passwd),
            show_list(g.sg_adm),
            show_list(g.sg_mem)
        )
    }

    /// A probe's line, the harness's escapes undone.
    fn unescape(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let b = s.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' {
                match b[i + 1] {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'\\' => out.push(b'\\'),
                    b'x' => {
                        let hex = &s[i + 2..i + 4];
                        out.push(u8::from_str_radix(hex, 16).unwrap());
                        i += 2;
                    }
                    other => panic!("escape {other}"),
                }
                i += 2;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    fn oracle() -> Vec<(&'static str, &'static str)> {
        ORACLE
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once(" = "))
            .collect()
    }

    /// A read-only stream over `text`.
    fn stream_of(text: &[u8]) -> *mut u8 {
        let bytes: &'static mut [u8] = Box::leak(text.to_vec().into_boxed_slice());
        // SAFETY: a leaked buffer of the length given, which outlives the
        // stream.
        let s = unsafe {
            crate::stdio_mem::fmemopen(bytes.as_mut_ptr().cast(), bytes.len(), c"r".as_ptr().cast())
        };
        assert!(!s.is_null(), "fmemopen");
        s
    }

    /// `sgetsgent` reads every line as glibc's does.
    #[test]
    fn sgetsgent_reads_lines_as_glibc_does() {
        let mut n = 0;
        for (probe, want) in oracle() {
            let Some(line) = probe
                .strip_prefix("sgetsgent(\"")
                .and_then(|p| p.strip_suffix("\")"))
            else {
                continue;
            };
            let c = CString::new(unescape(line)).unwrap();
            errno::set_errno(KEPT);
            // SAFETY: a C string.
            let g = unsafe { sgetsgent(c.as_ptr().cast()) };
            let e = errno::get_errno();
            // SAFETY: this process's entry, if any.
            let got = match unsafe { g.as_ref() } {
                Some(g) => show_entry(g),
                None => "NULL".into(),
            };
            assert_eq!(format!("{got} errno={}", errno_word(e)), want, "{probe}");
            n += 1;
        }
        assert_eq!(n, 27);
    }

    /// Its `_r` form: wherever glibc's fits the entry in the buffer, this
    /// one does too, and gives the same entry; where glibc's cannot, this
    /// one answers `ERANGE` or, needing less room, the entry.  (`*result`
    /// is NULL on every failure, where glibc's leaves it untouched when the
    /// string itself does not fit.)
    #[test]
    fn sgetsgent_r_fits_wherever_glibcs_does() {
        // Each line's entry, as sgetsgent gave it.
        let entries: std::collections::BTreeMap<&str, &str> = oracle()
            .into_iter()
            .filter_map(|(probe, want)| {
                let line = probe.strip_prefix("sgetsgent(\"")?.strip_suffix("\")")?;
                Some((line, want.strip_suffix(" errno=kept")?))
            })
            .collect();
        let mut n = 0;
        for (probe, want) in oracle() {
            let Some(rest) = probe.strip_prefix("sgetsgent_r(\"") else {
                continue;
            };
            let (line, size) = rest.rsplit_once("\", ").unwrap();
            let size: usize = size.trim_end_matches(')').parse().unwrap();
            let c = CString::new(unescape(line)).unwrap();
            let mut buf = [0u8; 64];
            let mut g = Sgrp::EMPTY;
            let mut res: *const Sgrp = core::ptr::dangling();
            errno::set_errno(KEPT);
            // SAFETY: a C string, and a buffer of at least `size` bytes.
            let rc = unsafe {
                sgetsgent_r(
                    c.as_ptr().cast(),
                    &raw mut g,
                    buf.as_mut_ptr(),
                    size,
                    &raw mut res,
                )
            };
            let e = errno::get_errno();
            if let Some(entry) = want.strip_prefix("0 ") {
                let entry = entry.strip_suffix(" errno=kept").unwrap();
                assert_eq!((rc, e), (0, KEPT), "{probe}");
                assert!(core::ptr::eq(res, &raw const g), "{probe}");
                assert_eq!(show_entry(&g), entry, "{probe}");
            } else {
                assert!(want.starts_with("ERANGE "), "{probe}: {want}");
                if rc == 0 {
                    assert_eq!(e, KEPT, "{probe}");
                    assert!(core::ptr::eq(res, &raw const g), "{probe}");
                    assert_eq!(show_entry(&g), entries[line], "{probe}: the whole entry");
                } else {
                    assert_eq!((rc, e), (errno::ERANGE, errno::ERANGE), "{probe}");
                    assert!(res.is_null(), "{probe}");
                }
            }
            n += 1;
        }
        assert_eq!(n, 27 * 9);
    }

    /// `fgetsgent` and `fgetsgent_r` over a file of entries, blank lines,
    /// lines of white space and comments, as glibc's.
    #[test]
    fn fgetsgent_reads_a_file_as_glibc_does() {
        let want: std::collections::BTreeMap<_, _> = oracle().into_iter().collect();
        let s = stream_of(FILE);
        for i in 0.. {
            let key = format!("fgetsgent #{i}");
            let Some(w) = want.get(key.as_str()) else {
                break;
            };
            errno::set_errno(KEPT);
            // SAFETY: an open stream.
            let g = unsafe { fgetsgent(s) };
            let e = errno::get_errno();
            // SAFETY: this process's entry, if any.
            let got = match unsafe { g.as_ref() } {
                Some(g) => show_entry(g),
                None => "NULL".into(),
            };
            assert_eq!(format!("{got} errno={}", errno_word(e)), *w, "{key}");
        }
        assert_eq!(crate::stdio::fclose(s), 0);
        let s = stream_of(FILE);
        let mut buf = [0u8; 4096];
        for i in 0.. {
            let key = format!("fgetsgent_r #{i}");
            let Some(w) = want.get(key.as_str()) else {
                break;
            };
            let mut g = Sgrp::EMPTY;
            let mut res: *const Sgrp = core::ptr::dangling();
            errno::set_errno(KEPT);
            // SAFETY: an open stream, and a buffer of the size passed.
            let rc =
                unsafe { fgetsgent_r(s, &raw mut g, buf.as_mut_ptr(), buf.len(), &raw mut res) };
            let e = errno::get_errno();
            let got = if rc == 0 {
                format!("0 {}", show_entry(&g))
            } else {
                format!(
                    "{} {}",
                    errno_word(rc),
                    if res.is_null() { "NULL" } else { "other" }
                )
            };
            assert_eq!(format!("{got} errno={}", errno_word(e)), *w, "{key}");
        }
        assert_eq!(crate::stdio::fclose(s), 0);
    }

    /// `putsgent` writes what glibc's writes, and refuses what it refuses.
    #[test]
    fn putsgent_writes_what_glibc_writes() {
        let want: std::collections::BTreeMap<_, _> = oracle().into_iter().collect();
        let c = |s: &str| CString::new(s).unwrap();
        let strings: Vec<CString> = [
            "wheel", "!", "root", "ann", "bob", "x", "a,b", "a:b", "a\nb", "", "wh:eel", "x\ny",
            "x:y", "x,y", "+wheel",
        ]
        .iter()
        .map(|s| c(s))
        .collect();
        let p = |s: &str| {
            strings
                .iter()
                .find(|x| x.as_bytes() == s.as_bytes())
                .unwrap()
                .as_ptr()
                .cast::<u8>()
        };
        let adm = [p("root"), p("ann"), core::ptr::null()];
        let mem = [p("bob"), core::ptr::null()];
        let none: [*const u8; 1] = [core::ptr::null()];
        let comma = [p("a,b"), core::ptr::null()];
        let colon = [p("a:b"), core::ptr::null()];
        let nl = [p("a\nb"), core::ptr::null()];
        let empty = [p(""), core::ptr::null()];
        let null = core::ptr::null::<u8>();
        let nl_list = core::ptr::null::<*const u8>();
        let sg = |n: *const u8, pw: *const u8, a: *const *const u8, m: *const *const u8| Sgrp {
            sg_namp: n,
            sg_passwd: pw,
            sg_adm: a,
            sg_mem: m,
        };
        let cases = [
            ("full", sg(p("wheel"), p("!"), adm.as_ptr(), mem.as_ptr())),
            (
                "null passwd",
                sg(p("wheel"), null, adm.as_ptr(), mem.as_ptr()),
            ),
            ("null lists", sg(p("wheel"), p("x"), nl_list, nl_list)),
            (
                "empty lists",
                sg(p("wheel"), p("x"), none.as_ptr(), none.as_ptr()),
            ),
            (
                "empty member",
                sg(p("wheel"), p("x"), empty.as_ptr(), empty.as_ptr()),
            ),
            ("null name", sg(null, p("x"), adm.as_ptr(), mem.as_ptr())),
            ("empty name", sg(p(""), p("x"), adm.as_ptr(), mem.as_ptr())),
            (
                "colon in name",
                sg(p("wh:eel"), p("x"), adm.as_ptr(), mem.as_ptr()),
            ),
            (
                "newline in passwd",
                sg(p("wheel"), p("x\ny"), adm.as_ptr(), mem.as_ptr()),
            ),
            (
                "colon in passwd",
                sg(p("wheel"), p("x:y"), adm.as_ptr(), mem.as_ptr()),
            ),
            (
                "comma in admin",
                sg(p("wheel"), p("x"), comma.as_ptr(), mem.as_ptr()),
            ),
            (
                "colon in member",
                sg(p("wheel"), p("x"), adm.as_ptr(), colon.as_ptr()),
            ),
            (
                "newline in member",
                sg(p("wheel"), p("x"), adm.as_ptr(), nl.as_ptr()),
            ),
            (
                "comma in passwd",
                sg(p("wheel"), p("x,y"), adm.as_ptr(), mem.as_ptr()),
            ),
            (
                "plus name",
                sg(p("+wheel"), p("x"), adm.as_ptr(), mem.as_ptr()),
            ),
        ];
        for (what, g) in &cases {
            let mut out: *mut u8 = core::ptr::null_mut();
            let mut len = 0usize;
            // SAFETY: a pair of out-pointers.
            let stream = unsafe { crate::stdio_mem::open_memstream(&raw mut out, &raw mut len) };
            errno::set_errno(KEPT);
            // SAFETY: an entry of this frame's strings; an open stream.
            let rc = unsafe { putsgent(g, stream) };
            let e = errno::get_errno();
            assert_eq!(crate::stdio::fclose(stream), 0);
            // SAFETY: open_memstream's block of `len` bytes.
            let bytes = unsafe { core::slice::from_raw_parts(out, len) }.to_vec();
            // SAFETY: the stream's block.
            unsafe { crate::malloc::free(out) };
            let text = CString::new(bytes).unwrap();
            let got = format!(
                "{rc} errno={} \"{}\"",
                errno_word(e),
                show(text.as_ptr().cast())
            );
            assert_eq!(got, want[format!("putsgent {what}").as_str()], "{what}");
        }
        // NULLs, where glibc's faults.
        errno::set_errno(0);
        // SAFETY: NULLs, refused.
        assert_eq!(
            unsafe { putsgent(core::ptr::null(), core::ptr::null_mut()) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // SAFETY: as above.
        assert_eq!(unsafe { putsgent(&cases[0].1, core::ptr::null_mut()) }, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    const GSHADOW: &[u8] = b"wheel:!:root:root,ann\n+nis\nstaff:x::bob\n\nlast:*::\n";

    /// The database: looked up by name (compat entries skipped), listed in
    /// order, rewound; empty with no file; the file's error without the
    /// right to read it.
    #[test]
    fn the_database_is_looked_up_and_listed() {
        set_test_text(Which::Gshadow, Some(GSHADOW));
        // SAFETY: C strings.
        let g = unsafe { getsgnam(c"staff".as_ptr().cast()) };
        // SAFETY: this process's entry.
        assert_eq!(show_entry(unsafe { &*g }), "staff|x|[]|[bob]");
        errno::set_errno(KEPT);
        // SAFETY: as above.
        assert!(
            unsafe { getsgnam(c"+nis".as_ptr().cast()) }.is_null(),
            "compat: skipped"
        );
        // SAFETY: as above.
        assert!(unsafe { getsgnam(c"nobody".as_ptr().cast()) }.is_null());
        let mut small = [0u8; 8];
        let mut e = Sgrp::EMPTY;
        let mut res: *const Sgrp = core::ptr::dangling();
        // SAFETY: a buffer of the size passed.
        let rc = unsafe {
            getsgnam_r(
                c"wheel".as_ptr().cast(),
                &raw mut e,
                small.as_mut_ptr(),
                8,
                &raw mut res,
            )
        };
        assert_eq!((rc, res.is_null()), (errno::ERANGE, true));
        setsgent();
        let mut seen = Vec::new();
        loop {
            let g = getsgent();
            // SAFETY: this process's entry, if any.
            match unsafe { g.as_ref() } {
                Some(g) => seen.push(show(g.sg_namp)),
                None => break,
            }
        }
        assert_eq!(seen, ["wheel", "+nis", "staff", "last"]);
        assert!(getsgent().is_null(), "still at the end");
        setsgent();
        // SAFETY: as above.
        assert_eq!(show(unsafe { &*getsgent() }.sg_namp), "wheel", "rewound");
        endsgent();
        set_test_text(Which::Gshadow, None);
        // SAFETY: C strings.
        assert!(
            unsafe { getsgnam(c"wheel".as_ptr().cast()) }.is_null(),
            "no file: no entries"
        );
        assert!(getsgent().is_null());
        set_test_error(Which::Gshadow, errno::EACCES);
        errno::set_errno(0);
        // SAFETY: as above.
        assert!(unsafe { getsgnam(c"wheel".as_ptr().cast()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EACCES, "the file's error");
        set_test_text(Which::Gshadow, None);
        endsgent();
    }

    #[test]
    fn sgrp_is_glibcs_layout() {
        assert_eq!(core::mem::size_of::<Sgrp>(), 32);
        assert_eq!(core::mem::offset_of!(Sgrp, sg_adm), 16);
        assert_eq!(core::mem::offset_of!(Sgrp, sg_mem), 24);
    }
}
