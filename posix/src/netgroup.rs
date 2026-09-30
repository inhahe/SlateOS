// Offsets and counts here are within one database's text, which bounds
// every sum made of them.  Clippy cannot see the bound.
#![allow(clippy::arithmetic_side_effects)]
//! Netgroups (`<netdb.h>`): `setnetgrent`, `getnetgrent`, `getnetgrent_r`,
//! `endnetgrent` and `innetgr` over `/etc/netgroup`, read as glibc 2.39's
//! `nss_files` reads it and expanded as its `getnetgrent_r` expands it.
//!
//! A netgroup is a named set of `(host,user,domain)` triples -- NFS exports
//! and `hosts.equiv`-style access lists name one instead of a list of hosts.
//!
//! - **Entries.** A group's line begins with its name, at the start of the
//!   line, and a white-space character; the first such line is the group.
//!   A line that ends in a backslash carries on into the next, the two
//!   joined with a blank.  Nothing is a comment: `#` is a character like
//!   another (a line `# note` is a group named `#`).
//! - **Members**, after the name, are triples and the names of other
//!   groups, apart at white space.  A triple's fields run to a comma, a
//!   comma and a closing parenthesis; each is its first word, and an empty
//!   one is NULL -- any value.  One with no closing parenthesis ends the
//!   group there.
//! - **Expansion.** A group's triples come in order; the groups it names
//!   come after them, the last named first, each once however often or
//!   deeply it is named, the group itself never again -- so a cycle ends.
//!   A named group with no line has no triples.
//! - **`innetgr`** is whether some triple has the host (ignoring case), the
//!   user (exactly) and the domain (ignoring case) asked for; a NULL asked
//!   for, or a NULL field, matches anything.  `-` is only `-`.
//! - **No `/etc/netgroup`**: `setnetgrent` and `innetgr` answer 0 with
//!   `errno` `ENOENT`, as glibc's -- its open's error.
//!
//! **Two differences, both limits of glibc's that its other functions do not
//! have** (design-decisions §1154): `getnetgrent`'s block grows for a triple
//! that does not fit it, where glibc's is a fixed 1 KiB and a longer triple
//! ends the enumeration; and `innetgr` reads a triple of any length, where
//! glibc's 1 KiB buffer makes a longer one end the group it is in.
//!
//! The enumeration is this process's, as glibc's (which locks it).  glibc
//! 2.39's answers -- every group of a test file walked, 36 `innetgr`
//! questions, `getnetgrent_r` at buffer sizes around a triple's need, with
//! the file and without -- are `netgroup_oracle.txt`
//! (`posix/tools/oracle/netgroup_harness.py`), which the tests replay.

use core::ops::Range;

use crate::decfloat::MallocBuf;
use crate::errno;
use crate::nss_files::{self, Db, Held, Room, Text, Which, c_bytes, hold, is_space};
use crate::perprocess::process_global;

// ---------------------------------------------------------------------------
// Reading the file
// ---------------------------------------------------------------------------

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

/// Whether `line` carries on into the next: it ends in a backslash and a
/// newline.
fn carries_on(line: &[u8]) -> bool {
    line.ends_with(b"\\\n")
}

/// The entry of `text` at offset `at` -- a line, and the lines it carries on
/// into -- as the offset past it; `None` at the end.
fn entry_end(text: &[u8], at: usize) -> Option<usize> {
    let (mut line, mut end) = line_at(text, at)?;
    while carries_on(line) {
        let Some((next, after)) = line_at(text, end) else {
            break;
        };
        (line, end) = (next, after);
    }
    Some(end)
}

/// Whether the entry at offset `at` is group `name`'s: its line begins with
/// the name and a white-space character.
fn names(text: &[u8], at: usize, name: &[u8]) -> bool {
    let line = line_at(text, at).map_or(&[][..], |(l, _)| l);
    line.strip_prefix(name)
        .and_then(<[u8]>::first)
        .is_some_and(|&b| is_space(b))
}

/// Group `name`'s member list from its entry at `at`: after the name and
/// the character after it, each backslash and newline that carries a line
/// on replaced by a blank, as glibc's `setnetgrent` joins them.  `None`
/// when memory runs out.
fn members(text: &[u8], at: usize, name_len: usize) -> Option<MallocBuf<u8>> {
    // The entry's pieces in order: each line's text -- the first's after the
    // name and the character after it -- and, for a line that carries on,
    // that text less its backslash and newline, then a blank.
    let pieces = || {
        let mut next = Some(at);
        let mut first = true;
        core::iter::from_fn(move || {
            let (line, end) = line_at(text, next?)?;
            let body = if first {
                line.get(name_len + 1..).unwrap_or(&[])
            } else {
                line
            };
            first = false;
            Some(if carries_on(line) {
                next = Some(end);
                let kept = body.get(..body.len().saturating_sub(2)).unwrap_or(&[]);
                [kept, &b" "[..]]
            } else {
                next = None;
                [body, &[][..]]
            })
        })
        .flatten()
    };
    let len: usize = pieces().map(<[u8]>::len).sum();
    let mut block = MallocBuf::<u8>::zeroed(len)?;
    let mut n = 0usize;
    for p in pieces() {
        if let Some(dst) = block.as_mut().get_mut(n..n + p.len()) {
            dst.copy_from_slice(p);
        }
        n += p.len();
    }
    Some(block)
}

/// A triple's fields: each one's range in its member list, `None` for an
/// empty one.
type Fields = [Option<Range<usize>>; 3];

/// A group's next member in its member list `m` from offset `at`.
enum Member {
    /// A triple: each field's range in `m`, `None` for an empty one; and the
    /// offset past it.
    Triple(Fields, usize),
    /// The name of another group, and the offset past it.
    Group(Range<usize>, usize),
    /// No more: the list's end, or a triple with no end.
    End,
}

/// A field's text: its first word, `None` for none -- glibc's
/// `strip_whitespace`.
fn word(m: &[u8], field: Range<usize>) -> Option<Range<usize>> {
    let f = m.get(field.clone())?;
    let start = f.iter().position(|&b| !is_space(b))?;
    let len = f
        .get(start..)?
        .iter()
        .position(|&b| is_space(b))
        .unwrap_or(f.len() - start);
    Some(field.start + start..field.start + start + len)
}

/// glibc's `_nss_netgroup_parseline`, over the list up to its first NUL.
fn next_member(m: &[u8], at: usize) -> Member {
    let m = m
        .get(..m.iter().position(|&b| b == 0).unwrap_or(m.len()))
        .unwrap_or(&[]);
    let Some(p) = m
        .get(at..)
        .and_then(|r| r.iter().position(|&b| !is_space(b)))
        .map(|i| at + i)
    else {
        return Member::End;
    };
    if m.get(p) != Some(&b'(') {
        let end = m
            .get(p..)
            .and_then(|r| r.iter().position(|&b| is_space(b)))
            .map_or(m.len(), |i| p + i);
        let next = if end < m.len() { end + 1 } else { end };
        return Member::Group(p..end, next);
    }
    // Each field to its terminator; the list's end first is no triple.
    let find = |from: usize, b: u8| -> Option<usize> {
        m.get(from..)?
            .iter()
            .position(|&c| c == b)
            .map(|i| from + i)
    };
    let host = p + 1;
    let Some(c1) = find(host, b',') else {
        return Member::End;
    };
    let Some(c2) = find(c1 + 1, b',') else {
        return Member::End;
    };
    let Some(close) = find(c2 + 1, b')') else {
        return Member::End;
    };
    Member::Triple(
        [
            word(m, host..c1),
            word(m, c1 + 1..c2),
            word(m, c2 + 1..close),
        ],
        close + 1,
    )
}

// ---------------------------------------------------------------------------
// Expanding a group
// ---------------------------------------------------------------------------

/// A group being expanded: the database, where each of its entries begins,
/// which entries' groups have been read or wait to be, those waiting, and
/// the one being read.
struct Walk {
    text: Text,
    /// Each entry's offset in `text`, by entry number.
    starts: MallocBuf<usize>,
    count: usize,
    /// By entry number: 1 once its group is read or waiting (glibc's known
    /// and needed groups).
    seen: MallocBuf<u8>,
    /// The waiting groups' entry numbers: the last pushed is read next.
    waiting: MallocBuf<usize>,
    nwaiting: usize,
    /// The member list being read, and where its next member begins.
    list: MallocBuf<u8>,
    at: usize,
}

impl Walk {
    /// Start expanding `name` over `text`: `Ok(None)` for no such group,
    /// `Err(ENOMEM)` when memory runs out.
    fn start(text: Text, name: &[u8]) -> Result<Option<Self>, i32> {
        let t = text.bytes();
        let mut count = 0usize;
        let mut at = 0;
        while let Some(end) = entry_end(t, at) {
            count += 1;
            at = end;
        }
        let mut starts = MallocBuf::<usize>::zeroed(count).ok_or(errno::ENOMEM)?;
        let (mut at, mut i) = (0, 0);
        while let Some(end) = entry_end(t, at) {
            if let Some(s) = starts.as_mut().get_mut(i) {
                *s = at;
            }
            (at, i) = (end, i + 1);
        }
        let seen = MallocBuf::<u8>::zeroed(count).ok_or(errno::ENOMEM)?;
        let waiting = MallocBuf::<usize>::zeroed(count).ok_or(errno::ENOMEM)?;
        let mut walk = Self {
            text,
            starts,
            count,
            seen,
            waiting,
            nwaiting: 0,
            list: MallocBuf::<u8>::zeroed(0).ok_or(errno::ENOMEM)?,
            at: 0,
        };
        let Some(entry) = walk.find(name) else {
            return Ok(None);
        };
        walk.mark(entry);
        walk.open(entry, name.len())?;
        Ok(Some(walk))
    }

    /// The entry number of group `name`'s line.
    fn find(&self, name: &[u8]) -> Option<usize> {
        let t = self.text.bytes();
        (0..self.count).find(|&i| names(t, self.start_of(i), name))
    }

    fn start_of(&self, entry: usize) -> usize {
        self.starts.as_ref().get(entry).copied().unwrap_or(0)
    }

    fn mark(&mut self, entry: usize) {
        if let Some(s) = self.seen.as_mut().get_mut(entry) {
            *s = 1;
        }
    }

    fn marked(&self, entry: usize) -> bool {
        self.seen.as_ref().get(entry).is_some_and(|&s| s != 0)
    }

    /// Read entry `entry`'s member list next; its group's name is
    /// `name_len` bytes.
    fn open(&mut self, entry: usize, name_len: usize) -> Result<(), i32> {
        let list =
            members(self.text.bytes(), self.start_of(entry), name_len).ok_or(errno::ENOMEM)?;
        self.list = list;
        self.at = 0;
        Ok(())
    }

    /// The next triple, its fields' ranges in [`Walk::list`] and the offset
    /// past it -- not yet taken: [`Walk::take`] moves past it -- or `None`
    /// when the expansion is over.  The group names it passes are taken.
    fn peek(&mut self) -> Result<Option<(Fields, usize)>, i32> {
        loop {
            match next_member(self.list.as_ref(), self.at) {
                Member::Triple(fields, next) => return Ok(Some((fields, next))),
                Member::Group(name, next) => {
                    self.at = next;
                    let found = {
                        let n = self.list.as_ref().get(name).unwrap_or(&[]);
                        self.find(n)
                    };
                    if let Some(entry) = found.filter(|&e| !self.marked(e)) {
                        self.mark(entry);
                        if let Some(w) = self.waiting.as_mut().get_mut(self.nwaiting) {
                            *w = entry;
                            self.nwaiting += 1;
                        }
                    }
                }
                Member::End => {
                    let Some(top) = self.nwaiting.checked_sub(1) else {
                        return Ok(None);
                    };
                    self.nwaiting = top;
                    let entry = self.waiting.as_ref().get(top).copied().unwrap_or(0);
                    // Its name is the first word of its line.
                    let t = self.text.bytes();
                    let line = line_at(t, self.start_of(entry)).map_or(&[][..], |(l, _)| l);
                    let name_len = line.iter().position(|&b| is_space(b)).unwrap_or(line.len());
                    self.open(entry, name_len)?;
                }
            }
        }
    }

    /// Move past the triple [`Walk::peek`] gave.
    fn take(&mut self, next: usize) {
        self.at = next;
    }
}

// ---------------------------------------------------------------------------
// The enumeration
// ---------------------------------------------------------------------------

/// A triple as `getnetgrent` hands it back.
#[derive(Clone, Copy)]
struct Triple {
    host: *const u8,
    user: *const u8,
    domain: *const u8,
}

impl Triple {
    const EMPTY: Self = Self {
        host: core::ptr::null(),
        user: core::ptr::null(),
        domain: core::ptr::null(),
    };
}

process_global! {
    fn walk() -> Option<Walk> = None;
    fn held_triple() -> Held<Triple> = Held::new(Triple::EMPTY);
}

/// The enumeration's next triple, copied into `room`: `None` at the end;
/// `ERANGE`, after which the same triple comes again; `ENOMEM`.
fn next_triple(room: &mut Room) -> Result<Option<Triple>, i32> {
    // SAFETY: this process's walk, used by one call at a time.
    let Some(w) = (unsafe { &mut *walk() }) else {
        return Ok(None);
    };
    let Some((fields, next)) = w.peek()? else {
        return Ok(None);
    };
    let list = w.list.as_ref();
    let mut copy = |f: &Option<Range<usize>>| -> Result<*const u8, i32> {
        match f {
            Some(r) => room
                .string(list.get(r.clone()).unwrap_or(&[]))
                .map(<*mut u8>::cast_const),
            None => Ok(core::ptr::null()),
        }
    };
    let [h, u, d] = &fields;
    let triple = Triple {
        host: copy(h)?,
        user: copy(u)?,
        domain: copy(d)?,
    };
    w.take(next);
    Ok(Some(triple))
}

/// Start enumerating netgroup `netgroup`'s triples, ending any enumeration
/// before: 1 when there is such a group, else 0 -- with `errno` the error
/// reading `/etc/netgroup` (`ENOENT` with no such file), and as it was for
/// a group the file does not have.
///
/// # Safety
///
/// `netgroup` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn setnetgrent(netgroup: *const u8) -> i32 {
    // SAFETY: this process's walk, used by one call at a time.
    let slot = unsafe { &mut *walk() };
    *slot = None;
    if netgroup.is_null() {
        return 0;
    }
    // SAFETY: non-null, and the caller's C string.
    let name = unsafe { c_bytes(netgroup) };
    if name.is_empty() {
        // glibc's files service is unavailable for "".
        return 0;
    }
    let text = match nss_files::read(Which::Netgroup) {
        Ok(Db::Text(t)) => t,
        Ok(Db::Missing) => {
            errno::set_errno(errno::ENOENT);
            return 0;
        }
        Err(e) => {
            errno::set_errno(e);
            return 0;
        }
    };
    match Walk::start(text, name) {
        Ok(Some(w)) => {
            *slot = Some(w);
            1
        }
        Ok(None) => 0,
        Err(e) => {
            errno::set_errno(e);
            0
        }
    }
}

/// End the enumeration, freeing what it holds.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endnetgrent() {
    // SAFETY: this process's walk, used by one call at a time.
    unsafe { *walk() = None };
}

/// The enumeration's next triple into the caller's buffer: 1 with
/// `*hostp`, `*userp` and `*domainp` its fields (NULL for an empty one), or
/// 0 -- at the end, or with `errno` `ERANGE` when `buffer` is too small,
/// after which the same triple comes again.
///
/// # Safety
///
/// `hostp`, `userp` and `domainp` are writable; `buffer` is writable for
/// `buflen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getnetgrent_r(
    hostp: *mut *const u8,
    userp: *mut *const u8,
    domainp: *mut *const u8,
    buffer: *mut u8,
    buflen: usize,
) -> i32 {
    if hostp.is_null() || userp.is_null() || domainp.is_null() || buffer.is_null() {
        errno::set_errno(errno::EFAULT);
        return 0;
    }
    // SAFETY: the caller gives `buflen` writable bytes at `buffer`.
    let mut room = unsafe { Room::new(buffer, buflen) };
    match next_triple(&mut room) {
        Ok(Some(t)) => {
            // SAFETY: writable, by contract.
            unsafe {
                hostp.write(t.host);
                userp.write(t.user);
                domainp.write(t.domain);
            }
            1
        }
        Ok(None) => 0,
        Err(e) => {
            errno::set_errno(e);
            0
        }
    }
}

/// [`getnetgrent_r`] into this process's block, which grows for a triple
/// that does not fit it: the strings are good until the next call.
///
/// # Safety
///
/// As [`getnetgrent_r`] for the three pointers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getnetgrent(
    hostp: *mut *const u8,
    userp: *mut *const u8,
    domainp: *mut *const u8,
) -> i32 {
    if hostp.is_null() || userp.is_null() || domainp.is_null() {
        errno::set_errno(errno::EFAULT);
        return 0;
    }
    let r = hold(held_triple(), |out, buf, len, result| {
        // SAFETY: the block `hold` gives, `len` bytes.
        let mut room = unsafe { Room::new(buf, len) };
        match next_triple(&mut room) {
            Ok(t) => {
                // SAFETY: `hold`'s own entry and result.
                unsafe {
                    if let Some(t) = t {
                        out.write(t);
                        result.write(out.cast_const());
                    } else {
                        result.write(core::ptr::null());
                    }
                }
                0
            }
            Err(e) => e,
        }
    });
    match r {
        Ok(p) if !p.is_null() => {
            // SAFETY: `p` is the held entry, and the pointers are writable.
            unsafe {
                hostp.write((*p).host);
                userp.write((*p).user);
                domainp.write((*p).domain);
            }
            1
        }
        Ok(_) => 0,
        Err(e) => {
            errno::set_errno(e);
            0
        }
    }
}

/// Whether netgroup `netgroup` -- its triples and the groups it names --
/// has a triple for `host` (compared ignoring case), `user` (exactly) and
/// `domain` (ignoring case), a NULL asked for or a NULL field matching
/// anything: 1 or 0.  `errno` as [`setnetgrent`] leaves it.  The
/// enumeration [`setnetgrent`] started is not disturbed.
///
/// # Safety
///
/// Each argument is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn innetgr(
    netgroup: *const u8,
    host: *const u8,
    user: *const u8,
    domain: *const u8,
) -> i32 {
    if netgroup.is_null() {
        return 0;
    }
    // SAFETY: each non-null, and the caller's C string.
    let (name, want) = unsafe {
        let s = |p: *const u8| (!p.is_null()).then(|| c_bytes(p));
        (c_bytes(netgroup), [s(host), s(user), s(domain)])
    };
    if name.is_empty() {
        return 0;
    }
    let text = match nss_files::read(Which::Netgroup) {
        Ok(Db::Text(t)) => t,
        Ok(Db::Missing) => {
            errno::set_errno(errno::ENOENT);
            return 0;
        }
        Err(e) => {
            errno::set_errno(e);
            return 0;
        }
    };
    let mut w = match Walk::start(text, name) {
        Ok(Some(w)) => w,
        Ok(None) => return 0,
        Err(e) => {
            errno::set_errno(e);
            return 0;
        }
    };
    loop {
        let (fields, next) = match w.peek() {
            Ok(Some(t)) => t,
            Ok(None) => return 0,
            Err(e) => {
                errno::set_errno(e);
                return 0;
            }
        };
        let list = w.list.as_ref();
        let matches = fields.iter().zip(want).enumerate().all(|(i, (f, want))| {
            let (Some(f), Some(want)) = (f, want) else {
                return true;
            };
            let f = list.get(f.clone()).unwrap_or(&[]);
            if i == 1 {
                f == want
            } else {
                f.eq_ignore_ascii_case(want)
            }
        });
        if matches {
            return 1;
        }
        w.take(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nss_files::{set_test_error, set_test_text};
    use core::ffi::CStr;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers, and the file it read
    /// (`posix/tools/oracle/netgroup_harness.py`).
    const ORACLE: &str = include_str!("netgroup_oracle.txt");

    /// The oracle's input, its escapes undone.
    fn input() -> &'static [u8] {
        let line = ORACLE
            .lines()
            .find_map(|l| l.strip_prefix("input netgroup = "))
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
                other => panic!("escape {other:?}"),
            });
        }
        Vec::leak(out)
    }

    /// The harness's `en`.
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

    /// The harness's `field`.
    fn field(p: *const u8) -> String {
        if p.is_null() {
            "NULL".into()
        } else {
            // SAFETY: a C string from the functions under test.
            format!("\"{}\"", String::from_utf8_lossy(unsafe { c_bytes(p) }))
        }
    }

    /// The harness's `triple`.
    fn triple(h: *const u8, u: *const u8, d: *const u8) -> String {
        format!("({},{},{})", field(h), field(u), field(d))
    }

    const GROUPS: &[&CStr] = &[
        c"trusted",
        c"servers",
        c"all",
        c"loopA",
        c"self",
        c"spaced",
        c"empty",
        c"dupes",
        c"nested",
        c"missingref",
        c"Upper",
        c"upper",
        c"tabs",
        c"commented",
        c"indented",
        c"bad1",
        c"bad2",
        c"bad3",
        c"bad4",
        c"cont",
        c"after-empty-cont",
        c"last",
        c"nosuchgroup",
        c"",
        c"#",
    ];

    /// The harness's `innetgr` questions: the group, then the three fields.
    const QUESTIONS: &[(&CStr, [Option<&CStr>; 3])] = &[
        (
            c"trusted",
            [Some(c"alpha"), Some(c"root"), Some(c"example.com")],
        ),
        (c"trusted", [Some(c"alpha"), None, None]),
        (
            c"trusted",
            [Some(c"ALPHA"), Some(c"root"), Some(c"EXAMPLE.COM")],
        ),
        (c"trusted", [Some(c"alpha"), Some(c"ROOT"), None]),
        (c"trusted", [Some(c"alpha"), Some(c"other"), None]),
        (
            c"trusted",
            [Some(c"beta"), Some(c"anyone"), Some(c"anywhere")],
        ),
        (c"trusted", [Some(c"anyhost"), Some(c"guest"), None]),
        (c"trusted", [Some(c"gamma"), None, None]),
        (c"trusted", [None, None, None]),
        (c"trusted", [Some(c""), None, None]),
        (c"trusted", [Some(c"dup-line"), None, None]),
        (c"servers", [Some(c"web1"), None, Some(c"prod")]),
        (c"servers", [Some(c"web1"), Some(c"someone"), Some(c"prod")]),
        (c"servers", [Some(c"web1"), Some(c"-"), Some(c"prod")]),
        (c"servers", [Some(c"db1"), None, None]),
        (c"all", [Some(c"alpha"), Some(c"root"), None]),
        (c"all", [Some(c"web2"), None, Some(c"prod")]),
        (c"all", [Some(c"gamma"), Some(c"admin"), Some(c"anything")]),
        (c"nested", [Some(c"db1"), None, None]),
        (c"nested", [Some(c"delta"), None, None]),
        (c"loopA", [Some(c"b1"), None, None]),
        (c"loopA", [Some(c"zz"), None, None]),
        (c"self", [Some(c"s1"), None, None]),
        (c"missingref", [Some(c"m"), None, None]),
        (c"spaced", [Some(c"host"), Some(c"user"), Some(c"dom")]),
        (
            c"spaced",
            [Some(c" host "), Some(c" user "), Some(c" dom ")],
        ),
        (c"Upper", [Some(c"up"), Some(c"Us"), Some(c"dom")]),
        (c"Upper", [Some(c"UP"), Some(c"us"), Some(c"Dom")]),
        (c"upper", [Some(c"UP"), Some(c"Us"), Some(c"Dom")]),
        (c"empty", [None, None, None]),
        (c"nosuchgroup", [None, None, None]),
        (c"bad4", [Some(c"a"), Some(c"b"), Some(c"c")]),
        (c"bad4", [Some(c"d"), Some(c"e"), Some(c"f")]),
        (c"cont", [Some(c"k1"), None, None]),
        (c"after-empty-cont", [Some(c"k2"), None, None]),
        (c"last", [Some(c"z1"), None, None]),
    ];

    fn cstr(c: Option<&CStr>) -> *const u8 {
        c.map_or(core::ptr::null(), |c| c.as_ptr().cast())
    }

    /// The harness's program, call for call: the lines it prints as `run`.
    #[allow(clippy::too_many_lines)] // the harness's program, in its order
    fn probes(run: &str) -> Vec<String> {
        // Each run is a process of its own there.
        endnetgrent();
        let mut out = Vec::new();
        let (mut h, mut u, mut d) = (core::ptr::null(), core::ptr::null(), core::ptr::null());
        errno::set_errno(12345);
        // SAFETY: writable pointers.
        let rc = unsafe { getnetgrent(&mut h, &mut u, &mut d) };
        out.push(format!(
            "{run} getnetgrent before setnetgrent = {rc} errno={}",
            en(errno::get_errno())
        ));
        for g in GROUPS {
            let name = g.to_str().unwrap();
            errno::set_errno(12345);
            // SAFETY: a C string.
            let rc = unsafe { setnetgrent(g.as_ptr().cast()) };
            out.push(format!(
                "{run} setnetgrent({name}) = {rc} errno={}",
                en(errno::get_errno())
            ));
            for n in 0..64 {
                errno::set_errno(12345);
                // SAFETY: writable pointers.
                let rc = unsafe { getnetgrent(&mut h, &mut u, &mut d) };
                let t = if rc == 1 {
                    triple(h, u, d)
                } else {
                    String::new()
                };
                out.push(format!(
                    "{run} getnetgrent({name}) #{n} = {rc} {t} errno={}",
                    en(errno::get_errno())
                ));
                if rc != 1 {
                    break;
                }
            }
            endnetgrent();
        }
        for (g, [qh, qu, qd]) in QUESTIONS {
            errno::set_errno(12345);
            // SAFETY: C strings or NULL.
            let rc = unsafe { innetgr(g.as_ptr().cast(), cstr(*qh), cstr(*qu), cstr(*qd)) };
            out.push(format!(
                "{run} innetgr({},{},{},{}) = {rc} errno={}",
                g.to_str().unwrap(),
                field(cstr(*qh)),
                field(cstr(*qu)),
                field(cstr(*qd)),
                en(errno::get_errno())
            ));
        }
        let mut buf = [0u8; 64];
        for size in (0..=48).step_by(4) {
            // SAFETY: a C string.
            unsafe { setnetgrent(c"trusted".as_ptr().cast()) };
            errno::set_errno(12345);
            // SAFETY: writable pointers; `size` bytes of `buf`'s 64.
            let rc = unsafe { getnetgrent_r(&mut h, &mut u, &mut d, buf.as_mut_ptr(), size) };
            let t = if rc == 1 {
                triple(h, u, d)
            } else {
                String::new()
            };
            let first = format!("{rc} {t} errno={}", en(errno::get_errno()));
            errno::set_errno(12345);
            // SAFETY: as above, all 64.
            let rc = unsafe { getnetgrent_r(&mut h, &mut u, &mut d, buf.as_mut_ptr(), 64) };
            let t = if rc == 1 {
                triple(h, u, d)
            } else {
                String::new()
            };
            out.push(format!(
                "{run} getnetgrent_r(trusted, {size}) = {first} then {rc} {t} errno={}",
                en(errno::get_errno())
            ));
            endnetgrent();
        }
        // SAFETY: C strings; writable pointers.
        unsafe {
            setnetgrent(c"trusted".as_ptr().cast());
            setnetgrent(c"servers".as_ptr().cast());
            let rc = getnetgrent(&mut h, &mut u, &mut d);
            let t = if rc == 1 {
                triple(h, u, d)
            } else {
                String::new()
            };
            out.push(format!(
                "{run} after setnetgrent(trusted), setnetgrent(servers) = {rc} {t}"
            ));
            endnetgrent();
            errno::set_errno(12345);
            let rc = getnetgrent(&mut h, &mut u, &mut d);
            out.push(format!(
                "{run} getnetgrent after endnetgrent = {rc} errno={}",
                en(errno::get_errno())
            ));
        }
        out
    }

    fn glibcs(run: &str) -> Vec<String> {
        ORACLE
            .lines()
            .filter(|l| l.strip_prefix(run).is_some_and(|r| r.starts_with(' ')))
            .map(String::from)
            .collect()
    }

    /// Every probe of both runs is glibc's answer.
    #[test]
    fn every_answer_is_glibcs() {
        set_test_text(Which::Netgroup, Some(input()));
        let files = probes("files");
        set_test_text(Which::Netgroup, None);
        let none = probes("none");
        let mut wrong = Vec::new();
        for (run, ours) in [("files", files), ("none", none)] {
            let want = glibcs(run);
            for i in 0..want.len().max(ours.len()) {
                let (w, o) = (want.get(i), ours.get(i));
                if w != o {
                    wrong.push(format!(
                        "{run} line {}\n  glibc: {w:?}\n  ours:  {o:?}",
                        i + 1
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} differ:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// A triple bigger than glibc's 1 KiB buffers is given by `getnetgrent`
    /// and found by `innetgr`; glibc's enumeration would end there, and its
    /// `innetgr` pass over the rest of the group.
    #[test]
    fn a_long_triple_is_read() {
        let mut t = b"big (".to_vec();
        t.extend(core::iter::repeat_n(b'h', 2000));
        t.extend_from_slice(b",u,d) (next,,)\n");
        set_test_text(Which::Netgroup, Some(Vec::leak(t)));
        let (mut h, mut u, mut d) = (core::ptr::null(), core::ptr::null(), core::ptr::null());
        // SAFETY: C strings; writable pointers.
        unsafe {
            assert_eq!(setnetgrent(c"big".as_ptr().cast()), 1);
            assert_eq!(getnetgrent(&mut h, &mut u, &mut d), 1);
            assert_eq!(c_bytes(h).len(), 2000);
            assert_eq!(getnetgrent(&mut h, &mut u, &mut d), 1);
            assert_eq!(c_bytes(h), b"next");
            endnetgrent();
            assert_eq!(
                innetgr(
                    c"big".as_ptr().cast(),
                    c"next".as_ptr().cast(),
                    core::ptr::null(),
                    core::ptr::null()
                ),
                1
            );
        }
        set_test_text(Which::Netgroup, None);
    }

    /// `innetgr` walks the group on its own: the enumeration goes on where it
    /// was.
    #[test]
    fn innetgr_leaves_the_enumeration_alone() {
        set_test_text(Which::Netgroup, Some(b"g (a,,) (b,,)\nh (c,,)\n"));
        let (mut h, mut u, mut d) = (core::ptr::null(), core::ptr::null(), core::ptr::null());
        // SAFETY: C strings; writable pointers.
        unsafe {
            assert_eq!(setnetgrent(c"g".as_ptr().cast()), 1);
            assert_eq!(getnetgrent(&mut h, &mut u, &mut d), 1);
            assert_eq!(c_bytes(h), b"a");
            assert_eq!(
                innetgr(
                    c"h".as_ptr().cast(),
                    c"c".as_ptr().cast(),
                    core::ptr::null(),
                    core::ptr::null()
                ),
                1
            );
            assert_eq!(getnetgrent(&mut h, &mut u, &mut d), 1);
            assert_eq!(c_bytes(h), b"b");
        }
        endnetgrent();
        set_test_text(Which::Netgroup, None);
    }

    /// NULL where a string belongs: no group, no enumeration -- where glibc's
    /// would fault.
    #[test]
    fn null_arguments() {
        set_test_text(Which::Netgroup, Some(b"g (a,,)\n"));
        let mut buf = [0u8; 16];
        let (mut h, mut u, mut d) = (core::ptr::null(), core::ptr::null(), core::ptr::null());
        // SAFETY: NULL is the input under test.
        unsafe {
            assert_eq!(setnetgrent(core::ptr::null()), 0);
            assert_eq!(
                innetgr(
                    core::ptr::null(),
                    core::ptr::null(),
                    core::ptr::null(),
                    core::ptr::null()
                ),
                0
            );
            errno::set_errno(0);
            assert_eq!(
                getnetgrent_r(core::ptr::null_mut(), &mut u, &mut d, buf.as_mut_ptr(), 16),
                0
            );
            assert_eq!(errno::get_errno(), errno::EFAULT);
            assert_eq!(getnetgrent(&mut h, core::ptr::null_mut(), &mut d), 0);
        }
        set_test_text(Which::Netgroup, None);
    }

    /// A file that cannot be read is `setnetgrent`'s and `innetgr`'s 0 with
    /// its error.
    #[test]
    fn an_unreadable_file_is_its_error() {
        set_test_error(Which::Netgroup, errno::EACCES);
        errno::set_errno(0);
        // SAFETY: C strings.
        unsafe {
            assert_eq!(setnetgrent(c"g".as_ptr().cast()), 0);
            assert_eq!(errno::get_errno(), errno::EACCES);
            errno::set_errno(0);
            assert_eq!(
                innetgr(
                    c"g".as_ptr().cast(),
                    core::ptr::null(),
                    core::ptr::null(),
                    core::ptr::null()
                ),
                0
            );
            assert_eq!(errno::get_errno(), errno::EACCES);
        }
        set_test_text(Which::Netgroup, None);
    }
}
