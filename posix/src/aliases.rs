// Offsets and counts here are within one database's text or one caller's
// buffer, which bounds every sum made of them.  Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects)]
//! `<aliases.h>` — the mail aliases database: `/etc/aliases`, where a mail
//! system looks up what an address such as `postmaster` stands for, read as
//! glibc 2.39's `nss_files` reads it.
//!
//! - **Entries.** A line `name: member, member, ...` begins one, after any
//!   white space it begins with; a `#` ends a line's text, and so does a NUL.
//!   The lines after it that begin with white space (an empty line does not)
//!   carry its member list on.  A line with no `:`, or nothing before it, is
//!   no entry.  The name is all of what comes before the colon, a blank
//!   before it included; each member is what lies between two commas, its
//!   leading white space dropped and its trailing white space kept.  A member
//!   `:include:path` stands for the members of the file at `path` -- each of
//!   its lines read the same way, with no `:include:` there -- and for none
//!   when it cannot be read.  An entry with no members is no entry.
//! - **Lookups** find the first entry whose name is the one asked for,
//!   ignoring case (`strcasecmp`).
//! - **No `/etc/aliases`**: a lookup fails with `open`'s `ENOENT`, as
//!   glibc's does, and there is nothing to enumerate.  Every entry is this
//!   host's (`alias_local` 1), and `alias_members` holds `alias_members_len`
//!   strings with no NULL after them, as glibc's.
//!
//! **Where glibc's reading is at odds with itself, this follows the rest of
//! glibc** (design-decisions §1153):
//!
//! - An **empty member** -- two commas together, or a carried-on line that
//!   starts with one -- glibc's parser loops on for good: it steps past a
//!   comma only after a member.  Here it is passed over, as glibc's own
//!   `:include:` reading passes it over.
//! - An entry too big for **`getaliasent_r`'s buffer** is `ERANGE`, and here
//!   it comes again on the next call, as every other database's does.
//!   glibc's has read into the entry by then, so its next call starts in
//!   the middle of it and the entry is lost -- and `getaliasent`, which
//!   retries with a bigger buffer, passes it over.
//! - An entry that begins with white space **after an empty line** is found
//!   by a lookup, as the enumeration finds it.  glibc's lookup passes over
//!   every line that begins with white space after an entry it does not
//!   want, empty lines between or not.
//! - Enumerating past an **`:include:` file that cannot be opened** leaves
//!   `errno` as it was, as enumerating does everywhere else; glibc's leaves
//!   `open`'s error there.
//!
//! glibc 2.39's answers -- enumeration, lookups, the `_r` forms at buffer
//! sizes around an entry's need, with the file and without, and the four
//! empty-member files it loops on -- are `aliases_oracle.txt`
//! (`posix/tools/oracle/aliases_harness.py`), which the tests replay.

use crate::errno;
use crate::nss_files::{
    self, Cursor, Db, Held, Room, Which, c_bytes, enumerated, hold, is_space, lookup_result,
    next_record, reentrant,
};
use crate::perprocess::process_global;

// ---------------------------------------------------------------------------
// The entry
// ---------------------------------------------------------------------------

/// `struct aliasent`, glibc's layout.
#[repr(C)]
pub struct Aliasent {
    /// The alias's name.
    pub alias_name: *const u8,
    /// How many members it has.
    pub alias_members_len: usize,
    /// Its members: `alias_members_len` strings, with no NULL after them.
    pub alias_members: *const *const u8,
    /// Whether the alias is this host's: 1 for every entry of the file.
    pub alias_local: i32,
}

impl Aliasent {
    pub(crate) const EMPTY: Self = Self {
        alias_name: core::ptr::null(),
        alias_members_len: 0,
        alias_members: core::ptr::null(),
        alias_local: 0,
    };
}

/// With no `/etc/aliases` there is nothing to enumerate.
const NO_ALIASES_TEXT: &[u8] = b"";

/// What makes a member a file of members.
const INCLUDE: &[u8] = b":include:";

/// The longest path an `:include:` names, NUL and all (`PATH_MAX`).
const PATH_MAX: usize = 4096;

// ---------------------------------------------------------------------------
// Reading the file
// ---------------------------------------------------------------------------

/// A line's text as glibc's parser sees it: up to its first NUL, `#` or
/// newline.
fn text_of(line: &[u8]) -> &[u8] {
    let end = line
        .iter()
        .position(|&b| matches!(b, 0 | b'#' | b'\n'))
        .unwrap_or(line.len());
    line.get(..end).unwrap_or(&[])
}

/// `s` without the white space it begins with.
fn trim_start(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|&&b| is_space(b)).count();
    s.get(n..).unwrap_or(&[])
}

/// The line of `text` at offset `at`, its newline included, and the offset
/// past it; `None` at the end.
fn line_at(text: &[u8], at: usize) -> Option<(&[u8], usize)> {
    let rest = text.get(at..).filter(|r| !r.is_empty())?;
    let len = rest
        .iter()
        .position(|&b| b == b'\n')
        .map_or(rest.len(), |i| i + 1);
    Some((rest.get(..len)?, at + len))
}

/// Whether `line` carries the entry before it on: it begins with white
/// space, and is not empty.
fn carries_on(line: &[u8]) -> bool {
    line.first().is_some_and(|&b| b != b'\n' && is_space(b))
}

/// An entry's first line taken apart: its name, and its text after the
/// colon.  `None`: the line is no entry.
fn head(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let t = trim_start(text_of(line));
    let colon = t.iter().position(|&b| b == b':')?;
    if colon == 0 {
        return None;
    }
    Some((t.get(..colon)?, t.get(colon + 1..)?))
}

/// The next entry of `text` at or after offset `at` -- its first line and
/// the lines that carry it on -- and the offset past it; `None` at the end.
fn entry_at(text: &[u8], mut at: usize) -> Option<(&[u8], usize)> {
    loop {
        let (line, mut end) = line_at(text, at)?;
        if head(line).is_none() {
            at = end;
            continue;
        }
        while let Some((next, after)) = line_at(text, end) {
            if !carries_on(next) {
                break;
            }
            end = after;
        }
        return Some((text.get(at..end)?, end));
    }
}

/// An entry, as [`entry_at`] found it.
struct Entry<'a> {
    name: &'a [u8],
    /// The first line's text after the colon.
    first: &'a [u8],
    /// The lines that carry it on, newlines and all.
    more: &'a [u8],
}

impl<'a> Entry<'a> {
    /// The entry [`entry_at`] gave as `record`.
    fn parse(record: &'a [u8]) -> Option<Self> {
        let (line, end) = line_at(record, 0)?;
        let (name, first) = head(line)?;
        Some(Self {
            name,
            first,
            more: record.get(end..)?,
        })
    }

    /// Its members as written, an `:include:` among them still unread.
    fn written(&self) -> impl Iterator<Item = &'a [u8]> {
        let more = self.more;
        members_of(self.first).chain(
            core::iter::successors(line_at(more, 0), move |&(_, at)| line_at(more, at))
                .flat_map(|(line, _)| members_of(text_of(line))),
        )
    }
}

/// The members in one line's text: what lies between commas, leading white
/// space dropped; an empty one is passed over.
fn members_of(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|&b| b == b',')
        .map(trim_start)
        .filter(|m| !m.is_empty())
}

/// The `:include:` file at `path` -- as written, trailing white space and
/// all, as glibc's `fopen` is given it -- or `None` when it cannot be read:
/// missing, unreadable, or named by more than a path can hold.
fn read_include(path: &[u8]) -> Option<nss_files::Text> {
    let mut name = [0u8; PATH_MAX];
    name.get_mut(..path.len())?.copy_from_slice(path);
    let saved = errno::get_errno();
    let read = nss_files::read_path(name.get(..=path.len())?);
    // Missing, or unreadable: glibc's reading ignores the member, and the
    // `errno` its `fopen` left goes unseen -- glibc's answers keep the
    // caller's.
    errno::set_errno(saved);
    match read {
        Ok(Db::Text(t)) => Some(t),
        _ => None,
    }
}

/// Copy `e` into `room`: `None` for an entry with no members, which is no
/// entry; `ERANGE` when it does not fit.
///
/// The strings go in first, one after another -- the name, then each
/// member -- and the member pointers after them, as glibc lays the entry
/// out.
fn fill(e: &Entry<'_>, room: &mut Room) -> Result<Option<Aliasent>, i32> {
    let mark = room.used();
    let name = room.string(e.name)?;
    let mut n = 0usize;
    for m in e.written() {
        let Some(path) = m.strip_prefix(INCLUDE) else {
            room.string(m)?;
            n += 1;
            continue;
        };
        let Some(list) = read_include(path) else {
            continue;
        };
        let mut at = 0;
        while let Some((line, next)) = line_at(list.bytes(), at) {
            for m in members_of(text_of(line)) {
                room.string(m)?;
                n += 1;
            }
            at = next;
        }
    }
    if n == 0 {
        room.rewind(mark);
        return Ok(None);
    }
    let members = room.pointers(n)?;
    let mut p = name.cast_const();
    for i in 0..n {
        // SAFETY: `p` is one of the strings just copied, each ended by its
        // NUL, and the next begins just past it.
        p = unsafe { p.add(c_bytes(p).len() + 1) };
        // SAFETY: `members` holds `n` pointers, and `i < n`.
        unsafe { members.add(i).write(p) };
    }
    Ok(Some(Aliasent {
        alias_name: name,
        alias_members_len: n,
        alias_members: members.cast_const(),
        alias_local: 1,
    }))
}

// ---------------------------------------------------------------------------
// Lookup and enumeration
// ---------------------------------------------------------------------------

process_global! {
    fn held_alias() -> Held<Aliasent> = Held::new(Aliasent::EMPTY);
    fn held_aliasent() -> Held<Aliasent> = Held::new(Aliasent::EMPTY);
    fn alias_cursor() -> Cursor = Cursor::CLOSED;
}

/// Look up an alias by name, ignoring case (reentrant): 0 with `*result`
/// the entry or NULL; `ERANGE` when `buffer` is too small for it; the error
/// reading `/etc/aliases` -- `ENOENT` with no such file, as glibc's; or
/// `EFAULT` for a NULL `name`, where glibc's lookup, reaching the first
/// entry, would fault.
///
/// # Safety
///
/// `name` is NULL or a C string; non-null `result_buf` and `result` are
/// writable, and `buffer` writable for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getaliasbyname_r(
    name: *const u8,
    result_buf: *mut Aliasent,
    buffer: *mut u8,
    buflen: usize,
    result: *mut *const Aliasent,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented; `name` is read
    // only once it is known not to be NULL.
    unsafe {
        reentrant(result_buf, buffer, buflen, result, |room| {
            let file = match nss_files::read(Which::Aliases)? {
                Db::Text(t) => t,
                Db::Missing => return Err(errno::ENOENT),
            };
            let text = file.bytes();
            // SAFETY: non-null, and the caller's C string.
            let wanted = (!name.is_null()).then(|| c_bytes(name));
            let mut at = 0;
            while let Some((record, next)) = entry_at(text, at) {
                at = next;
                let Some(e) = Entry::parse(record) else {
                    continue;
                };
                let Some(wanted) = wanted else {
                    return Err(errno::EFAULT);
                };
                if !e.name.eq_ignore_ascii_case(wanted) {
                    continue;
                }
                // An entry of that name with no members is none: glibc's
                // lookup goes on to the next.
                if let Some(a) = fill(&e, room)? {
                    return Ok(Some(a));
                }
            }
            Ok(None)
        })
    }
}

/// Look up an alias by name, ignoring case: the entry, or NULL -- `errno` 0
/// for no such alias, the error reading `/etc/aliases` otherwise (`ENOENT`
/// with no such file).
///
/// # Safety
///
/// `name` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getaliasbyname(name: *const u8) -> *const Aliasent {
    // SAFETY: `name` is passed on as given; the rest is this process's.
    lookup_result(hold(held_alias(), |p, b, l, r| unsafe {
        getaliasbyname_r(name, p, b, l, r)
    }))
}

/// Rewind the aliases database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setaliasent() {
    // SAFETY: this process's cursor.
    unsafe { *alias_cursor() = Cursor::CLOSED };
}

/// The next alias into the caller's buffer: 0, `ENOENT` at the end (and for
/// a database that cannot be read), or `ERANGE` -- after which the same
/// entry comes again.
///
/// # Safety
///
/// Non-null `result_buf` and `result` are writable, and `buffer` writable
/// for `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getaliasent_r(
    result_buf: *mut Aliasent,
    buffer: *mut u8,
    buflen: usize,
    result: *mut *const Aliasent,
) -> i32 {
    // SAFETY: the pointers are the caller's, as documented.
    unsafe {
        next_record(
            alias_cursor(),
            Which::Aliases,
            NO_ALIASES_TEXT,
            entry_at,
            |record, room| fill(&Entry::parse(record)?, room).transpose(),
            result_buf,
            buffer,
            buflen,
            result,
        )
    }
}

/// The next alias, or NULL at the end.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getaliasent() -> *const Aliasent {
    // SAFETY: this process's storage.
    enumerated(hold(held_aliasent(), |p, b, l, r| unsafe {
        getaliasent_r(p, b, l, r)
    }))
}

/// Close the aliases database.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endaliasent() {
    // SAFETY: this process's cursor.
    unsafe { *alias_cursor() = Cursor::CLOSED };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nss_files::{set_test_error, set_test_file, set_test_text};
    use core::ffi::CStr;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers, and the files it read
    /// (`posix/tools/oracle/aliases_harness.py`).
    const ORACLE: &str = include_str!("aliases_oracle.txt");

    /// The oracle's input `name`, its escapes undone.
    fn input(name: &str) -> &'static [u8] {
        let prefix = format!("input {name} = ");
        let line = ORACLE
            .lines()
            .find_map(|l| l.strip_prefix(prefix.as_str()))
            .unwrap();
        let mut out = Vec::new();
        let mut bytes = line.bytes();
        while let Some(b) = bytes.next() {
            if b != b'\\' {
                out.push(b);
                continue;
            }
            out.push(match bytes.next() {
                Some(b'n') => b'\n',
                Some(b't') => b'\t',
                Some(b'r') => b'\r',
                Some(b'\\') => b'\\',
                other => panic!("{name}: escape {other:?}"),
            });
        }
        Vec::leak(out)
    }

    fn text(p: *const u8) -> String {
        // SAFETY: the functions under test hand back C strings.
        String::from_utf8_lossy(unsafe { c_bytes(p) }).into_owned()
    }

    /// The harness's `en`: errno's name, `kept` for its marker.
    fn en(e: i32) -> String {
        match e {
            0 => "0".into(),
            12345 => "kept".into(),
            errno::ENOENT => "ENOENT".into(),
            errno::ERANGE => "ERANGE".into(),
            errno::EINVAL => "EINVAL".into(),
            e => format!("e{e}"),
        }
    }

    /// The harness's `entry`.
    fn entry(a: &Aliasent) -> String {
        let mut s = format!("{}|{}|[", text(a.alias_name), a.alias_local);
        for i in 0..a.alias_members_len {
            // SAFETY: an answer's `alias_members_len` strings.
            let m = unsafe { *a.alias_members.add(i) };
            if i > 0 {
                s.push(',');
            }
            s += &format!("\"{}\"", text(m));
        }
        s.push(']');
        s
    }

    /// The harness's `show`, `errno` read now.
    fn show(run: &str, what: &str, a: *const Aliasent) -> String {
        let answer = if a.is_null() {
            "NULL".into()
        } else {
            // SAFETY: a non-null answer from the functions under test.
            entry(unsafe { &*a })
        };
        format!("{run} {what} = {answer} errno={}", en(errno::get_errno()))
    }

    /// The harness's names, in its order.
    const NAMES: &[&CStr] = &[
        c"postmaster",
        c"POSTMASTER",
        c"root",
        c"mailer-daemon",
        c"nobody",
        c"empty",
        c"spaced",
        c"spaced ",
        c"upper",
        c"Upper",
        c"dup",
        c"inc",
        c"incmissing",
        c"noname",
        c"",
        c"commented",
        c"hash",
        c"hash#name",
        c"trailing",
        c"tabbed",
        c"indented",
        c"cont",
        c"after-blank",
        c"before-blank",
        c"indented-after-blank",
        c"carriage",
        c"last",
        c"missing",
        c"second-line-member",
        c"admin",
    ];

    /// A `_r` form's answer as the harness prints it.
    fn delivered(rc: i32, a: &Aliasent, res: *const Aliasent, unset: *const Aliasent) -> String {
        if rc == 0 && core::ptr::eq(res, a) {
            entry(a)
        } else if res.is_null() {
            "NULL".into()
        } else if core::ptr::eq(res, unset) {
            "untouched".into()
        } else {
            "other".into()
        }
    }

    /// The harness's program, call for call: the lines it prints as `run`.
    /// Each run is a process of its own there, so it starts with the
    /// database closed here.
    fn probes(run: &str) -> Vec<String> {
        endaliasent();
        let mut out = Vec::new();
        for n in 0..64 {
            errno::set_errno(12345);
            let a = getaliasent();
            out.push(show(run, &format!("getaliasent #{n}"), a));
            if a.is_null() {
                break;
            }
        }
        if run.starts_with("case ") {
            return out;
        }
        setaliasent();
        errno::set_errno(12345);
        out.push(show(run, "getaliasent after setaliasent", getaliasent()));
        errno::set_errno(12345);
        out.push(show(run, "getaliasent after that", getaliasent()));
        endaliasent();
        errno::set_errno(12345);
        out.push(show(run, "getaliasent after endaliasent", getaliasent()));
        endaliasent();
        for name in NAMES {
            errno::set_errno(12345);
            // SAFETY: a C string.
            let a = unsafe { getaliasbyname(name.as_ptr().cast()) };
            let what = format!("getaliasbyname({})", name.to_str().unwrap());
            out.push(show(run, &what, a));
        }
        let unset = core::ptr::dangling::<Aliasent>();
        let mut buf = std::vec![0u8; 4096];
        for size in (0..=200).step_by(8) {
            let mut a = Aliasent::EMPTY;
            let mut res = unset;
            errno::set_errno(12345);
            // SAFETY: a C string; `size` bytes of `buf`, which has 4096;
            // outputs this test owns.
            let rc = unsafe {
                getaliasbyname_r(
                    c"root".as_ptr().cast(),
                    &mut a,
                    buf.as_mut_ptr(),
                    size,
                    &mut res,
                )
            };
            out.push(format!(
                "{run} getaliasbyname_r(root, {size}) = {} {} errno={}",
                en(rc),
                delivered(rc, &a, res, unset),
                en(errno::get_errno())
            ));
        }
        let mut a = Aliasent::EMPTY;
        let mut res = unset;
        errno::set_errno(12345);
        // SAFETY: as above.
        let rc = unsafe {
            getaliasbyname_r(
                c"missing".as_ptr().cast(),
                &mut a,
                buf.as_mut_ptr(),
                4096,
                &mut res,
            )
        };
        out.push(format!(
            "{run} getaliasbyname_r(missing) = {} {} errno={}",
            en(rc),
            if res.is_null() { "NULL" } else { "other" },
            en(errno::get_errno())
        ));
        setaliasent();
        for n in 0..64 {
            let mut a = Aliasent::EMPTY;
            let mut res = unset;
            errno::set_errno(12345);
            let len = if n == 1 { 16 } else { 4096 };
            // SAFETY: `len` bytes of `buf`; outputs this test owns.
            let rc = unsafe { getaliasent_r(&mut a, buf.as_mut_ptr(), len, &mut res) };
            let answer = match delivered(rc, &a, res, unset) {
                d if d == "untouched" => "other".into(),
                d => d,
            };
            out.push(format!(
                "{run} getaliasent_r #{n} = {} {answer} errno={}",
                en(rc),
                en(errno::get_errno())
            ));
            if rc != 0 && rc != errno::ERANGE {
                break;
            }
        }
        endaliasent();
        out
    }

    /// glibc's lines for `run`.
    fn glibcs(run: &str) -> Vec<&'static str> {
        ORACLE
            .lines()
            .filter(|l| l.strip_prefix(run).is_some_and(|r| r.starts_with(' ')))
            .collect()
    }

    /// What `line`, one of glibc's, says: the part after ` = `.
    fn answer(line: &str) -> &str {
        line.split_once(" = ").unwrap().1
    }

    /// glibc's `files` lines with this library's answers where it differs
    /// (the module's list, design-decisions §1153): `errno` kept past the
    /// missing `:include:`; the entry after an empty line found by name;
    /// and `getaliasent_r`'s entry that did not fit given again, where
    /// glibc's is lost -- that sequence is glibc's `getaliasent`'s, which
    /// had room.
    fn files_expected() -> Vec<String> {
        let glibc = glibcs("files");
        let entries: Vec<&str> = glibc
            .iter()
            .filter(|l| l.starts_with("files getaliasent #"))
            .map(|l| answer(l).strip_suffix(" errno=kept").unwrap_or(answer(l)))
            .map(|a| a.strip_suffix(" errno=ENOENT").unwrap_or(a))
            .collect();
        let found_after_blank = entries
            .iter()
            .find(|e| e.starts_with("indented-after-blank|"))
            .unwrap();
        let mut want = Vec::new();
        for line in &glibc {
            let line = if line.starts_with("files getaliasent #") && line.contains("|[") {
                line.replace(" errno=ENOENT", " errno=kept")
            } else if *line == "files getaliasbyname(indented-after-blank) = NULL errno=0" {
                format!("files getaliasbyname(indented-after-blank) = {found_after_blank} errno=0")
            } else if line.starts_with("files getaliasent_r #") {
                continue;
            } else {
                (*line).into()
            };
            want.push(line);
        }
        let mut n = 0;
        for (i, e) in entries.iter().enumerate() {
            if i == 1 {
                want.push(format!(
                    "files getaliasent_r #{n} = ERANGE NULL errno=ERANGE"
                ));
                n += 1;
            }
            let line = if *e == "NULL" {
                format!("files getaliasent_r #{n} = ENOENT NULL errno=kept")
            } else {
                format!("files getaliasent_r #{n} = 0 {e} errno=kept")
            };
            want.push(line);
            n += 1;
        }
        want
    }

    /// What this library reads in each file glibc loops on: the members
    /// either side of the empty one.
    const CASE_ENTRIES: [&[&str]; 4] = [
        &[r#"a|1|["x","y"]"#, r#"b|1|["z"]"#],
        &[r#"a|1|["x"]"#, r#"b|1|["z"]"#],
        &[r#"a|1|["x","y"]"#, r#"b|1|["z"]"#],
        &[r#"a|1|["x","y"]"#, r#"b|1|["z"]"#],
    ];

    /// Our lines against the expected ones -- where glibc's `_r` lookup
    /// found a buffer too small, ours may have fitted, and must then give
    /// the entry glibc gives with room (its layout's threshold, not the
    /// interface, as in `netdb.rs`); the differences, described.
    fn compare(run: &str, want: &[String], ours: &[String], wrong: &mut Vec<String>) {
        let roomy = want
            .iter()
            .find_map(|w| w.strip_prefix(&format!("{run} getaliasbyname_r(root, 200) = ")));
        for i in 0..want.len().max(ours.len()) {
            let (w, o) = (want.get(i), ours.get(i));
            let layout = match (w, o, roomy) {
                (Some(w), Some(o), Some(r)) if w.ends_with("= ERANGE NULL errno=ERANGE") => {
                    let (probe, _) = w.split_once(" = ").unwrap();
                    probe.contains("getaliasbyname_r(root, ") && *o == format!("{probe} = {r}")
                }
                _ => false,
            };
            if w != o && !layout {
                wrong.push(format!(
                    "{run} line {}\n  want: {w:?}\n  ours: {o:?}",
                    i + 1
                ));
            }
        }
    }

    /// Every probe of the harness's runs: glibc's answers, but for the
    /// four places the module lists, where glibc's reading is at odds with
    /// itself.
    #[test]
    fn every_answer_is_glibcs_where_glibc_agrees_with_itself() {
        let mut wrong = Vec::new();
        set_test_text(Which::Aliases, Some(input("aliases")));
        set_test_file(b"/etc/alias.list", Some(input("alias.list")));
        compare("files", &files_expected(), &probes("files"), &mut wrong);
        set_test_text(Which::Aliases, None);
        let none: Vec<String> = glibcs("none").into_iter().map(String::from).collect();
        compare("none", &none, &probes("none"), &mut wrong);
        for (i, entries) in CASE_ENTRIES.iter().enumerate() {
            let run = format!("case {i}");
            assert_eq!(
                glibcs(&run),
                [format!("{run} = loops")],
                "glibc loops on it"
            );
            set_test_text(Which::Aliases, Some(input(&run)));
            let mut want: Vec<String> = entries
                .iter()
                .enumerate()
                .map(|(n, e)| format!("{run} getaliasent #{n} = {e} errno=kept"))
                .collect();
            want.push(format!(
                "{run} getaliasent #{} = NULL errno=kept",
                entries.len()
            ));
            compare(&run, &want, &probes(&run), &mut wrong);
        }
        set_test_text(Which::Aliases, None);
        set_test_file(b"/etc/alias.list", None);
        assert!(
            wrong.is_empty(),
            "{} differ:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// An `:include:` in an included file is a member like any other; a
    /// file with nothing in it adds nothing; an entry whose only member is
    /// an unreadable file is no entry.
    #[test]
    fn an_include_is_one_level_deep() {
        set_test_text(
            Which::Aliases,
            Some(b"a: :include:/l1, x\nb: :include:/empty\nc: :include:/l1\n"),
        );
        set_test_file(b"/l1", Some(b":include:/l2, y\n"));
        set_test_file(b"/l2", Some(b"never\n"));
        set_test_file(b"/empty", Some(b""));
        let mut got = Vec::new();
        setaliasent();
        loop {
            let a = getaliasent();
            if a.is_null() {
                break;
            }
            // SAFETY: a non-null answer.
            got.push(entry(unsafe { &*a }));
        }
        endaliasent();
        assert_eq!(
            got,
            [
                r#"a|1|[":include:/l2","y","x"]"#,
                r#"c|1|[":include:/l2","y"]"#
            ]
        );
        for p in [&b"/l1"[..], b"/l2", b"/empty"] {
            set_test_file(p, None);
        }
        set_test_text(Which::Aliases, None);
    }

    /// A file that cannot be read is its error to a lookup, and the end to
    /// an enumeration, `errno` as it was.
    #[test]
    fn an_unreadable_file_is_its_error() {
        set_test_error(Which::Aliases, errno::EACCES);
        let mut a = Aliasent::EMPTY;
        let mut res: *const Aliasent = core::ptr::null();
        let mut buf = [0u8; 64];
        // SAFETY: a C string; outputs this test owns.
        let rc = unsafe {
            getaliasbyname_r(c"x".as_ptr().cast(), &mut a, buf.as_mut_ptr(), 64, &mut res)
        };
        assert_eq!(
            (rc, res.is_null(), errno::get_errno()),
            (errno::EACCES, true, errno::EACCES)
        );
        endaliasent();
        errno::set_errno(12345);
        assert!(getaliasent().is_null());
        assert_eq!(errno::get_errno(), 12345);
        set_test_text(Which::Aliases, None);
    }

    /// NULL is no name: `EFAULT` once there is an entry to compare it with
    /// (where glibc's `strcasecmp` would fault), and not found before.
    #[test]
    fn a_null_name() {
        let mut a = Aliasent::EMPTY;
        let mut res: *const Aliasent = core::ptr::null();
        let mut buf = [0u8; 64];
        set_test_text(Which::Aliases, Some(b"# nothing\n"));
        // SAFETY: NULL is the input under test; outputs this test owns.
        let rc =
            unsafe { getaliasbyname_r(core::ptr::null(), &mut a, buf.as_mut_ptr(), 64, &mut res) };
        assert_eq!((rc, res.is_null()), (0, true));
        set_test_text(Which::Aliases, Some(b"a: b\n"));
        // SAFETY: as above.
        let rc =
            unsafe { getaliasbyname_r(core::ptr::null(), &mut a, buf.as_mut_ptr(), 64, &mut res) };
        assert_eq!((rc, res.is_null()), (errno::EFAULT, true));
        set_test_text(Which::Aliases, None);
    }

    /// A lookup passes over an entry of that name with no members, as
    /// glibc's does, and gives the next.
    #[test]
    fn a_memberless_entry_is_passed_over() {
        set_test_text(Which::Aliases, Some(b"x:\nx: , ,\nx: y\n"));
        // SAFETY: a C string.
        let a = unsafe { getaliasbyname(c"X".as_ptr().cast()) };
        assert!(!a.is_null());
        // SAFETY: non-null.
        assert_eq!(entry(unsafe { &*a }), r#"x|1|["y"]"#);
        set_test_text(Which::Aliases, None);
    }

    /// An entry bigger than the non-reentrant block's first size is given,
    /// the block grown -- where glibc's `getaliasent`, its entry lost to
    /// the first `ERANGE`, gives the next.
    #[test]
    fn a_long_entry_is_not_lost() {
        let mut t = b"big: ".to_vec();
        for i in 0..400 {
            t.extend_from_slice(format!("member{i}, ").as_bytes());
        }
        t.extend_from_slice(b"\nnext: n\n");
        set_test_text(Which::Aliases, Some(Vec::leak(t)));
        setaliasent();
        let a = getaliasent();
        assert!(!a.is_null());
        // SAFETY: non-null.
        let a = unsafe { &*a };
        assert_eq!(
            (text(a.alias_name).as_str(), a.alias_members_len),
            ("big", 400)
        );
        // SAFETY: non-null, and the next entry.
        assert_eq!(text(unsafe { (*getaliasent()).alias_name }), "next");
        endaliasent();
        set_test_text(Which::Aliases, None);
    }
}
