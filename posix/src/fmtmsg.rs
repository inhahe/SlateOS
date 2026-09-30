//! `<fmtmsg.h>`: `fmtmsg` (XSH `fmtmsg`) and glibc's `addseverity`.
//!
//! ## The message
//!
//! ```text
//! label: severity: text
//! TO FIX: action  tag
//! ```
//!
//! A part appears when it is given -- not its null value, `MM_NULLLBL` and
//! the rest, `MM_NOSEV` for the severity -- and, on standard error, when
//! `MSGVERB` selects it. `": "` follows the label when anything else does,
//! and the severity when a text, an action or a tag does; a newline follows
//! the text when an action or a tag does; `"TO FIX: "` goes before the
//! action, two spaces between it and the tag; a newline ends the message, an
//! empty one too. As glibc 2.39 prints it:
//! `posix/tools/oracle/fmtmsg_harness.py` records 804 cases
//! (`fmtmsg_oracle.txt`), each in a process of its own, and the tests replay
//! them.
//!
//! - **`classification`**: `MM_PRINT` writes to standard error, through its
//!   stream; `MM_CONSOLE` to `/dev/console`, every part whatever `MSGVERB`
//!   says. The source and recovery bits describe the condition and change
//!   nothing here.
//! - **`label`**: two fields around a colon, at most 10 bytes before it and
//!   14 after; anything else is `MM_NOTOK`, and nothing is written.
//! - **`severity`**: `MM_HALT`, `MM_ERROR`, `MM_WARNING`, `MM_INFO` print
//!   their names, `MM_NOSEV` nothing; a level `SEV_LEVEL` or `addseverity`
//!   defined prints its string; any other is `MM_NOTOK`.
//! - **`MSGVERB`**: colon-separated keywords, `label`, `severity`, `text`,
//!   `action`, `tag`, the parts standard error gets; unset, empty, or with
//!   anything else in it, all of them.
//! - **`SEV_LEVEL`**: colon-separated `keyword,level,string` entries, each
//!   defining a level above `MM_INFO` (read by `strtol`, so `05` is 5); an
//!   entry that is not one is passed over.
//! - **The return**: `MM_OK`; `MM_NOMSG` when standard error could not be
//!   written, `MM_NOCON` when the console could not be, `MM_NOTOK` when
//!   neither could (POSIX: "the function failed completely").
//!
//! **`addseverity(level, string)`** defines a level above `MM_INFO` as
//! `string`, redefines one, or with `string` NULL removes one: `MM_OK`, or
//! `MM_NOTOK` for a level at or below `MM_INFO` -- the standard five cannot
//! be changed -- or the removal of one that is not defined. The string is
//! the caller's, kept as its pointer, as glibc keeps it.
//!
//! One answer is not glibc's (design-decisions §1150): glibc reads
//! `MSGVERB` and `SEV_LEVEL` at a process's first `fmtmsg`, so an
//! `addseverity` before it is undone by `SEV_LEVEL` and one after it is not;
//! here they are read at the first call of either function, and a
//! program's `addseverity` always has the last word.
//!
//! Until 2026-09-30 this knew no `MSGVERB`, `SEV_LEVEL`, `addseverity` or
//! console, printed a severity it did not know instead of refusing it,
//! accepted any label, ended the first line after the text whatever
//! followed, and wrote around `stderr` to descriptor 2 (known-issues.md ->
//! D-POSIX-FMTMSG-KNEW-NONE-OF-ITS-VARIABLES).

use crate::decfloat::MallocBuf;
use crate::perprocess::{PoolLock, lock_pool, process_global};

// ---------------------------------------------------------------------------
// Classification flags (long bitmask)
// ---------------------------------------------------------------------------

// musl's and glibc's values, which a program compiled against <fmtmsg.h>
// passes. (MM_RECOVER and MM_NRECOV were 0x10000 and 0x20000 until
// 2026-09-30, and MM_APPL, MM_UTIL and MM_OPSYS absent: nothing read
// them, and scripts/check-libc-abi.py did not read this header.)

/// The condition's source is hardware.
pub const MM_HARD: i64 = 0x001;
/// ... software.
pub const MM_SOFT: i64 = 0x002;
/// ... firmware.
pub const MM_FIRM: i64 = 0x004;

/// Detected by an application.
pub const MM_APPL: i64 = 0x008;
/// ... by a utility.
pub const MM_UTIL: i64 = 0x010;
/// ... by the operating system.
pub const MM_OPSYS: i64 = 0x020;

/// The error is recoverable.
pub const MM_RECOVER: i64 = 0x040;
/// ... not recoverable.
pub const MM_NRECOV: i64 = 0x080;

/// Display the message on standard error.
pub const MM_PRINT: i64 = 0x100;
/// ... on the system console.
pub const MM_CONSOLE: i64 = 0x200;

/// No classification.
pub const MM_NULLMC: i64 = 0;

// ---------------------------------------------------------------------------
// Severity levels
// ---------------------------------------------------------------------------

/// No severity: none is printed.
pub const MM_NOSEV: i32 = 0;
/// No severity (`MM_NOSEV`, POSIX's null value for the argument).
pub const MM_NULLSEV: i32 = 0;
/// The condition requires the program to halt.
pub const MM_HALT: i32 = 1;
/// A fault the program has detected.
pub const MM_ERROR: i32 = 2;
/// An unusual condition, not an error.
pub const MM_WARNING: i32 = 3;
/// Information.
pub const MM_INFO: i32 = 4;

// ---------------------------------------------------------------------------
// Null values
// ---------------------------------------------------------------------------

/// No label.
pub const MM_NULLLBL: *const u8 = core::ptr::null();
/// No text.
pub const MM_NULLTXT: *const u8 = core::ptr::null();
/// No action.
pub const MM_NULLACT: *const u8 = core::ptr::null();
/// No tag.
pub const MM_NULLTAG: *const u8 = core::ptr::null();

// ---------------------------------------------------------------------------
// Return values
// ---------------------------------------------------------------------------

/// Everything asked for was written.
pub const MM_OK: i32 = 0;
/// Nothing was: the arguments were refused, or neither channel worked.
pub const MM_NOTOK: i32 = -1;
/// Standard error could not be written; the rest succeeded.
pub const MM_NOMSG: i32 = 1;
/// The console could not be written; the rest succeeded.
pub const MM_NOCON: i32 = 4;

// ---------------------------------------------------------------------------
// MSGVERB
// ---------------------------------------------------------------------------

/// The parts, as `MSGVERB` names them, and each one's bit.
const KEYWORDS: [(&[u8], u8); 5] = [
    (b"label", 1),
    (b"severity", 2),
    (b"text", 4),
    (b"action", 8),
    (b"tag", 16),
];
/// Every part.
const ALL: u8 = 31;

/// The parts `MSGVERB`'s value selects: all of them unless each of its
/// colon-separated words is a keyword, as glibc reads it -- a word must end
/// the value or be followed by a colon, and one colon is skipped after it,
/// so `text:` is `text` and `:text` and `text::tag` are not keywords at all.
fn verb_mask(value: Option<&[u8]>) -> u8 {
    let Some(v) = value.filter(|v| !v.is_empty()) else {
        return ALL;
    };
    let (mut mask, mut i) = (0u8, 0usize);
    while i < v.len() {
        let rest = v.get(i..).unwrap_or(&[]);
        let hit = KEYWORDS.iter().find(|(word, _)| {
            rest.starts_with(word) && matches!(rest.get(word.len()), None | Some(b':'))
        });
        let Some(&(word, bit)) = hit else {
            return ALL;
        };
        mask |= bit;
        i = i.saturating_add(word.len());
        if v.get(i) == Some(&b':') {
            i = i.saturating_add(1);
        }
    }
    mask
}

// ---------------------------------------------------------------------------
// The levels
// ---------------------------------------------------------------------------

/// What the standard levels print, `MM_NOSEV` through `MM_INFO`.
const STANDARD: [&[u8]; 5] = [b"", b"HALT", b"ERROR", b"WARNING", b"INFO"];

/// A level `SEV_LEVEL` or `addseverity` defined.
struct Node {
    level: i32,
    /// What it prints: a C string.
    string: *const u8,
    /// `SEV_LEVEL`'s own copy of the string, freed with the node; NULL for
    /// `addseverity`'s, which is the caller's.
    owned: *mut u8,
    next: *mut Node,
}

/// No memory for a level.
struct NoMemory;

/// What `fmtmsg` and `addseverity` share: the parts `MSGVERB` selects and
/// the levels defined, read from the environment at the first call of
/// either.
pub(crate) struct Table {
    ready: bool,
    mask: u8,
    head: *mut Node,
}

impl Table {
    pub(crate) const fn new() -> Self {
        Self {
            ready: false,
            mask: ALL,
            head: core::ptr::null_mut(),
        }
    }

    /// Read `MSGVERB` and `SEV_LEVEL`, once.
    fn ensure_ready(&mut self, env: &Env<'_>) {
        if self.ready {
            return;
        }
        self.ready = true;
        self.mask = verb_mask(env.msgverb);
        if let Some(v) = env.sev_level {
            self.sev_levels(v);
        }
    }

    /// `SEV_LEVEL`'s entries, as glibc reads them: the first field up to its
    /// comma is skipped, the second is `strtol`'s, and a level above
    /// `MM_INFO` followed by a comma is defined as everything after that
    /// comma up to the entry's end.
    fn sev_levels(&mut self, v: &[u8]) {
        let mut i = 0usize;
        while i < v.len() {
            let end = v
                .get(i..)
                .and_then(|r| r.iter().position(|&b| b == b':'))
                .map_or(v.len(), |p| i.saturating_add(p));
            let mut j = i;
            while j < end {
                j = j.saturating_add(1);
                if v.get(j.saturating_sub(1)) == Some(&b',') {
                    break;
                }
            }
            if j < end {
                let (level, k) = strtol(v, j);
                if level > i64::from(MM_INFO)
                    && v.get(k) == Some(&b',')
                    && let Ok(level) = i32::try_from(level)
                {
                    let string = v.get(k.saturating_add(1)..end).unwrap_or(&[]);
                    // A level that finds no memory is not defined: fmtmsg
                    // will call it unknown, which is what it is here.
                    let _ = self.define_copy(level, string);
                }
            }
            i = if end < v.len() {
                end.saturating_add(1)
            } else {
                end
            };
        }
    }

    /// Define `level` as a copy of `string` (`SEV_LEVEL`'s).
    fn define_copy(&mut self, level: i32, string: &[u8]) -> Result<(), NoMemory> {
        let copy = crate::malloc::malloc(string.len().saturating_add(1));
        if copy.is_null() {
            return Err(NoMemory);
        }
        // SAFETY: `copy` holds `string.len() + 1` bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(string.as_ptr(), copy, string.len());
            copy.add(string.len()).write(0);
        }
        self.define(level, copy, copy)
    }

    /// Define or redefine `level` as the C string `string`; `owned`, if not
    /// NULL, is freed when the definition goes.
    fn define(&mut self, level: i32, string: *const u8, owned: *mut u8) -> Result<(), NoMemory> {
        let mut at = self.head;
        while !at.is_null() {
            // SAFETY: a node of this table's list.
            let node = unsafe { &mut *at };
            if node.level == level {
                // SAFETY: NULL or the node's own copy.
                unsafe { crate::malloc::free(node.owned) };
                node.string = string;
                node.owned = owned;
                return Ok(());
            }
            at = node.next;
        }
        let node = crate::malloc::malloc(size_of::<Node>()).cast::<Node>();
        if node.is_null() {
            // SAFETY: NULL or the copy this call was given to keep.
            unsafe { crate::malloc::free(owned) };
            return Err(NoMemory);
        }
        // SAFETY: a fresh block the size of a `Node`, from `malloc`, which
        // aligns for any object.
        unsafe {
            node.write(Node {
                level,
                string,
                owned,
                next: self.head,
            });
        }
        self.head = node;
        Ok(())
    }

    /// Remove `level`: was it defined?
    fn remove(&mut self, level: i32) -> bool {
        let mut link: *mut *mut Node = &raw mut self.head;
        // SAFETY: `link` is the head or a node's `next`, and each non-NULL
        // pointer a node of this table's list, unlinked before it is freed.
        unsafe {
            while !(*link).is_null() {
                let node = *link;
                if (*node).level == level {
                    *link = (*node).next;
                    crate::malloc::free((*node).owned);
                    crate::malloc::free(node.cast());
                    return true;
                }
                link = &raw mut (*node).next;
            }
        }
        false
    }

    /// What `level` prints, if it is one: a standard level's name, or a
    /// defined level's string.
    fn string(&self, level: i32) -> Option<&[u8]> {
        if let Some(s) = usize::try_from(level).ok().and_then(|i| STANDARD.get(i)) {
            return Some(s);
        }
        let mut at = self.head;
        while !at.is_null() {
            // SAFETY: a node of this table's list.
            let node = unsafe { &*at };
            if node.level == level {
                // SAFETY: a C string -- SEV_LEVEL's copy, or the one the
                // caller of addseverity gave and keeps.
                return Some(unsafe {
                    core::slice::from_raw_parts(node.string, crate::string::strlen(node.string))
                });
            }
            at = node.next;
        }
        None
    }
}

impl Drop for Table {
    fn drop(&mut self) {
        let mut at = self.head;
        while !at.is_null() {
            // SAFETY: each node of this table's list, freed once.
            unsafe {
                let next = (*at).next;
                crate::malloc::free((*at).owned);
                crate::malloc::free(at.cast());
                at = next;
            }
        }
    }
}

/// C's `strtol(s + i, &end, 0)`: the value and where it ended (`i` when
/// there is no number).
fn strtol(s: &[u8], i: usize) -> (i64, usize) {
    let mut j = i;
    while s.get(j).is_some_and(|b| b" \t\n\x0b\x0c\r".contains(b)) {
        j = j.saturating_add(1);
    }
    let negative = s.get(j) == Some(&b'-');
    if matches!(s.get(j), Some(b'+' | b'-')) {
        j = j.saturating_add(1);
    }
    let mut base = 10u32;
    if s.get(j) == Some(&b'0')
        && matches!(s.get(j.saturating_add(1)), Some(b'x' | b'X'))
        && s.get(j.saturating_add(2))
            .is_some_and(u8::is_ascii_hexdigit)
    {
        base = 16;
        j = j.saturating_add(2);
    } else if s.get(j) == Some(&b'0') {
        base = 8;
    }
    let start = j;
    let mut v: i64 = 0;
    while let Some(d) = s.get(j).and_then(|&b| char::from(b).to_digit(base)) {
        v = v
            .saturating_mul(i64::from(base))
            .saturating_add(i64::from(d));
        j = j.saturating_add(1);
    }
    if j == start {
        return (0, i);
    }
    (if negative { v.saturating_neg() } else { v }, j)
}

// ---------------------------------------------------------------------------
// The message
// ---------------------------------------------------------------------------

/// Where `MSGVERB` and `SEV_LEVEL` come from: the environment, or a test's
/// own.
pub(crate) struct Env<'a> {
    pub(crate) msgverb: Option<&'a [u8]>,
    pub(crate) sev_level: Option<&'a [u8]>,
}

/// Where the two messages go: standard error and the console, or a test's
/// own. Each answers whether the message was written.
pub(crate) trait Out {
    fn stderr(&mut self, message: &[u8]) -> bool;
    fn console(&mut self, message: &[u8]) -> bool;
}

/// The message's parts: its label, severity, text, action and tag, each
/// `None` when not given.
struct Parts<'a> {
    label: Option<&'a [u8]>,
    /// The severity's string, `None` for `MM_NOSEV`.
    severity: Option<&'a [u8]>,
    text: Option<&'a [u8]>,
    action: Option<&'a [u8]>,
    tag: Option<&'a [u8]>,
}

/// The message, with the parts `mask` selects: its bytes and length.
fn compose(mask: u8, p: &Parts<'_>) -> Option<(MallocBuf<u8>, usize)> {
    let label = p.label.filter(|_| mask & 1 != 0);
    let severity = p.severity.filter(|_| mask & 2 != 0);
    let text = p.text.filter(|_| mask & 4 != 0);
    let action = p.action.filter(|_| mask & 8 != 0);
    let tag = p.tag.filter(|_| mask & 16 != 0);
    let after_severity = text.is_some() || action.is_some() || tag.is_some();
    let empty: &[u8] = b"";
    let pieces: [&[u8]; 11] = [
        label.unwrap_or(empty),
        if label.is_some() && (severity.is_some() || after_severity) {
            b": "
        } else {
            empty
        },
        severity.unwrap_or(empty),
        if severity.is_some() && after_severity {
            b": "
        } else {
            empty
        },
        text.unwrap_or(empty),
        if text.is_some() && (action.is_some() || tag.is_some()) {
            b"\n"
        } else {
            empty
        },
        if action.is_some() { b"TO FIX: " } else { empty },
        action.unwrap_or(empty),
        if action.is_some() && tag.is_some() {
            b"  "
        } else {
            empty
        },
        tag.unwrap_or(empty),
        b"\n",
    ];
    let len = pieces
        .iter()
        .try_fold(0usize, |n, p| n.checked_add(p.len()))?;
    let mut buf = MallocBuf::<u8>::zeroed(len)?;
    let mut at = 0usize;
    for piece in pieces {
        buf.as_mut()
            .get_mut(at..at.saturating_add(piece.len()))?
            .copy_from_slice(piece);
        at = at.saturating_add(piece.len());
    }
    Some((buf, len))
}

/// Is `label` two fields around a colon, at most 10 bytes and 14?
fn label_ok(label: &[u8]) -> bool {
    label
        .iter()
        .position(|&b| b == b':')
        .is_some_and(|c| c <= 10 && label.len().saturating_sub(c).saturating_sub(1) <= 14)
}

/// `fmtmsg` against `table`, `env` and `out`.
#[allow(clippy::too_many_arguments)] // fmtmsg's six, and where they go
fn fmtmsg_in(
    table: &mut Table,
    env: &Env<'_>,
    out: &mut dyn Out,
    classification: i64,
    label: Option<&[u8]>,
    severity: i32,
    text: Option<&[u8]>,
    action: Option<&[u8]>,
    tag: Option<&[u8]>,
) -> i32 {
    table.ensure_ready(env);
    if label.is_some_and(|l| !label_ok(l)) {
        return MM_NOTOK;
    }
    let Some(string) = table.string(severity) else {
        return MM_NOTOK;
    };
    let parts = Parts {
        label,
        severity: (severity != MM_NULLSEV).then_some(string),
        text,
        action,
        tag,
    };
    let mut result = MM_OK;
    if classification & MM_PRINT != 0 {
        let written = compose(table.mask, &parts)
            .is_some_and(|(m, n)| out.stderr(m.as_ref().get(..n).unwrap_or(&[])));
        if !written {
            result = MM_NOMSG;
        }
    }
    if classification & MM_CONSOLE != 0 {
        let written = compose(ALL, &parts)
            .is_some_and(|(m, n)| out.console(m.as_ref().get(..n).unwrap_or(&[])));
        if !written {
            result = if result == MM_NOMSG {
                MM_NOTOK
            } else {
                MM_NOCON
            };
        }
    }
    result
}

/// `addseverity` against `table` and `env`.
fn addseverity_in(table: &mut Table, env: &Env<'_>, severity: i32, string: *const u8) -> i32 {
    table.ensure_ready(env);
    if severity <= MM_INFO {
        return MM_NOTOK;
    }
    if string.is_null() {
        return if table.remove(severity) {
            MM_OK
        } else {
            MM_NOTOK
        };
    }
    match table.define(severity, string, core::ptr::null_mut()) {
        Ok(()) => MM_OK,
        Err(NoMemory) => MM_NOTOK,
    }
}

// ---------------------------------------------------------------------------
// The C functions
// ---------------------------------------------------------------------------

process_global! {
    /// The levels and `MSGVERB`'s parts.
    fn table() -> Table = Table::new();
    /// Serialises every use of the table.
    fn table_lock() -> PoolLock = PoolLock::new();
}

/// A C string argument: `None` for NULL.
///
/// # Safety
///
/// `p` must be NULL or a C string.
unsafe fn arg<'a>(p: *const u8) -> Option<&'a [u8]> {
    // SAFETY: the caller's C string.
    (!p.is_null()).then(|| unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) })
}

/// The process's `MSGVERB` and `SEV_LEVEL`.
fn process_env() -> Env<'static> {
    // SAFETY: C string literals; getenv answers NULL or a C string, which
    // stays valid while the environment is not changed -- as long as the
    // table's reading of it.
    unsafe {
        Env {
            msgverb: arg(crate::environ::getenv(c"MSGVERB".as_ptr().cast())),
            sev_level: arg(crate::environ::getenv(c"SEV_LEVEL".as_ptr().cast())),
        }
    }
}

/// Standard error's stream, and `/dev/console`.
struct System {
    stream: *mut u8,
}

impl Out for System {
    fn stderr(&mut self, message: &[u8]) -> bool {
        // SAFETY: a live stream and `message.len()` readable bytes.
        let n = unsafe { crate::stdio::fwrite(message.as_ptr(), 1, message.len(), self.stream) };
        n == message.len()
    }

    fn console(&mut self, message: &[u8]) -> bool {
        // O_NOCTTY where glibc has none: writing a message is no reason
        // for a session leader to take the console as its terminal.
        let fd = crate::file::open(
            crate::paths::_PATH_CONSOLE.as_ptr(),
            crate::fcntl::O_WRONLY | crate::fcntl::O_NOCTTY | crate::fcntl::O_CLOEXEC,
            0,
        );
        if fd < 0 {
            return false;
        }
        let mut done = 0usize;
        let mut ok = true;
        while let Some(rest) = message.get(done..).filter(|r| !r.is_empty()) {
            let n = crate::file::write(fd, rest.as_ptr(), rest.len());
            match usize::try_from(n) {
                Ok(n) if n > 0 => done = done.saturating_add(n),
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        // A descriptor this opened and wrote; a failure to close it loses
        // nothing the message has not already been given.
        let _ = crate::file::close(fd);
        ok
    }
}

/// `fmtmsg(classification, label, severity, text, action, tag)`: display a
/// message on standard error and/or the console -- see the module's
/// documentation.
///
/// # Safety
///
/// Each string must be NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn fmtmsg(
    classification: i64,
    label: *const u8,
    severity: i32,
    text: *const u8,
    action: *const u8,
    tag: *const u8,
) -> i32 {
    let mut out = System {
        stream: crate::stdio::stderr_stream(),
    };
    // SAFETY: the caller's strings.
    let (label, text, action, tag) = unsafe { (arg(label), arg(text), arg(action), arg(tag)) };
    // SAFETY: `table_lock()` is this context's lock, valid as long as the
    // table.
    let _guard = unsafe { lock_pool(table_lock()) };
    // SAFETY: the lock is held, so this is the only reference to the table.
    let table = unsafe { &mut *table() };
    fmtmsg_in(
        table,
        &process_env(),
        &mut out,
        classification,
        label,
        severity,
        text,
        action,
        tag,
    )
}

/// `addseverity(severity, string)` (glibc): define, redefine or remove a
/// level above `MM_INFO` -- see the module's documentation.
///
/// # Safety
///
/// `string` must be NULL or a C string that stays valid while the level is
/// defined.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn addseverity(severity: i32, string: *const u8) -> i32 {
    // SAFETY: as in fmtmsg.
    let _guard = unsafe { lock_pool(table_lock()) };
    // SAFETY: as in fmtmsg.
    let table = unsafe { &mut *table() };
    addseverity_in(table, &process_env(), severity, string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::ToOwned;
    use std::collections::HashMap;
    use std::format;
    use std::string::{String, ToString};
    use std::vec;
    use std::vec::Vec;

    const ORACLE: &str = include_str!("fmtmsg_oracle.txt");
    const DEVIATIONS: &str = include_str!("fmtmsg_deviations.txt");

    /// The oracle's escapes back: `\x` NULL, `""` empty, `\xNN` a byte.
    fn from_oracle(t: &str) -> Option<Vec<u8>> {
        if t == "\\x" {
            return None;
        }
        if t == "\"\"" {
            return Some(Vec::new());
        }
        let b = t.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') && i + 4 <= b.len() {
                out.push(u8::from_str_radix(&t[i + 2..i + 4], 16).unwrap());
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        Some(out)
    }

    /// Standard error as the harness writes it: `\x` for nothing.
    fn to_oracle(b: &[u8]) -> String {
        if b.is_empty() {
            return "\\x".to_owned();
        }
        let mut s = String::new();
        for &c in b {
            if c == b'\\' || c <= b' ' || c > b'~' {
                s.push_str(&format!("\\x{c:02x}"));
            } else {
                s.push(char::from(c));
            }
        }
        s
    }

    /// Standard error and the console, captured; either failing on request.
    #[derive(Default)]
    struct Capture {
        err: Vec<u8>,
        con: Vec<u8>,
        err_fails: bool,
        con_fails: bool,
    }

    impl Out for Capture {
        fn stderr(&mut self, message: &[u8]) -> bool {
            if !self.err_fails {
                self.err.extend_from_slice(message);
            }
            !self.err_fails
        }

        fn console(&mut self, message: &[u8]) -> bool {
            if !self.con_fails {
                self.con.extend_from_slice(message);
            }
            !self.con_fails
        }
    }

    /// A case of the oracle: its fields, and glibc's answer.
    fn parse(line: &str) -> (HashMap<&str, &str>, &str) {
        let (left, right) = line.split_once(" = ").unwrap();
        (
            left.split(' ')
                .map(|f| f.split_once('=').unwrap())
                .collect(),
            right,
        )
    }

    /// A case answered here, as the harness writes glibc's: `<fmtmsg's
    /// return> <addseverity's, or -> <standard error>`; and the console.
    fn answer(fields: &HashMap<&str, &str>) -> (String, Vec<u8>) {
        let (verb, sev) = (from_oracle(fields["verb"]), from_oracle(fields["sev"]));
        let env = Env {
            msgverb: verb.as_deref(),
            sev_level: sev.as_deref(),
        };
        let mut table = Table::new();
        // addseverity keeps its caller's strings: these outlive the table.
        let mut strings: Vec<Vec<u8>> = Vec::new();
        let mut adds = Vec::new();
        if fields["add"] != "\\x" {
            for op in fields["add"].split(',') {
                let (n, s) = op.split_once('/').unwrap();
                let string = match from_oracle(s) {
                    None => core::ptr::null(),
                    Some(mut v) => {
                        v.push(0);
                        strings.push(v);
                        strings.last().unwrap().as_ptr()
                    }
                };
                adds.push(addseverity_in(&mut table, &env, n.parse().unwrap(), string).to_string());
            }
        }
        let mut out = Capture {
            err_fails: fields.get("stderr") == Some(&"closed"),
            ..Capture::default()
        };
        let part = |k: &str| from_oracle(fields[k]);
        let (label, text, action, tag) = (part("label"), part("text"), part("action"), part("tag"));
        let r = fmtmsg_in(
            &mut table,
            &env,
            &mut out,
            fields["class"].parse().unwrap(),
            label.as_deref(),
            fields["severity"].parse().unwrap(),
            text.as_deref(),
            action.as_deref(),
            tag.as_deref(),
        );
        drop(table);
        drop(strings);
        let adds = if adds.is_empty() {
            "-".to_owned()
        } else {
            adds.join(",")
        };
        (format!("{r} {adds} {}", to_oracle(&out.err)), out.con)
    }

    /// Every case of `fmtmsg_oracle.txt` answered as glibc 2.39 answers it
    /// -- the return, addseverity's, standard error byte for byte -- but for
    /// those `fmtmsg_deviations.txt` lists (design-decisions §1150).
    #[test]
    fn fmtmsg_is_glibcs_but_where_its_order_is_not() {
        let mut deviations = HashMap::new();
        let mut lines = DEVIATIONS.lines().filter(|l| !l.starts_with('#'));
        while let Some(glibc) = lines.next() {
            let here = lines.next().unwrap();
            deviations.insert(
                glibc.strip_prefix("glibc ").unwrap(),
                here.strip_prefix("here  ")
                    .unwrap()
                    .split_once(" = ")
                    .unwrap()
                    .1,
            );
        }
        let (mut n, mut used) = (0, 0);
        let mut bad = Vec::new();
        for line in ORACLE.lines().filter(|l| !l.starts_with('#')) {
            let (fields, glibc) = parse(line);
            let want = match deviations.get(line) {
                Some(here) => {
                    used += 1;
                    *here
                }
                None => glibc,
            };
            let (got, console) = answer(&fields);
            n += 1;
            if got != want {
                bad.push(format!("{line}\n   got {got}"));
            }
            // The console's message is every part, whatever MSGVERB says.
            let class: i64 = fields["class"].parse().unwrap();
            if class & MM_CONSOLE != 0 && !got.starts_with("-1") {
                let mut all = fields.clone();
                all.insert("verb", "\\x");
                all.insert("class", "256");
                all.insert("stderr", "open");
                let (plain, _) = answer(&all);
                let text = plain.splitn(3, ' ').nth(2).unwrap();
                assert_eq!(to_oracle(&console), text, "the console's, {line}");
            }
        }
        assert_eq!(used, deviations.len(), "the deviation list is stale");
        assert!(n >= 804, "{n} cases");
        assert!(
            bad.is_empty(),
            "{} of {n} cases wrong, the first:\n{}",
            bad.len(),
            bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    fn run(env: &Env<'_>, table: &mut Table, out: &mut Capture, class: i64, severity: i32) -> i32 {
        fmtmsg_in(
            table,
            env,
            out,
            class,
            Some(b"UX:cat"),
            severity,
            Some(b"t"),
            None,
            None,
        )
    }

    /// The departure, reasoned: SEV_LEVEL is read at the first call of
    /// either function, so a program's addseverity comes after it and has
    /// the last word, whether or not fmtmsg was called first.
    #[test]
    fn a_programs_addseverity_has_the_last_word() {
        let env = Env {
            msgverb: None,
            sev_level: Some(b"five,5,FIVE"),
        };
        for fmtmsg_first in [false, true] {
            let mut table = Table::new();
            let mut out = Capture::default();
            if fmtmsg_first {
                assert_eq!(run(&env, &mut table, &mut out, MM_PRINT, 5), MM_OK);
            }
            assert_eq!(
                addseverity_in(&mut table, &env, 5, c"OVER".as_ptr().cast()),
                MM_OK
            );
            out.err.clear();
            assert_eq!(run(&env, &mut table, &mut out, MM_PRINT, 5), MM_OK);
            assert_eq!(
                out.err, b"UX:cat: OVER: t\n",
                "fmtmsg first: {fmtmsg_first}"
            );
            // ... and can remove what SEV_LEVEL defined.
            assert_eq!(
                addseverity_in(&mut table, &env, 5, core::ptr::null()),
                MM_OK
            );
            assert_eq!(run(&env, &mut table, &mut out, MM_PRINT, 5), MM_NOTOK);
        }
    }

    /// POSIX's returns when a channel fails: each alone, and both.
    #[test]
    fn a_failing_channel_is_named_and_both_are_notok() {
        let env = Env {
            msgverb: None,
            sev_level: None,
        };
        let both = MM_PRINT | MM_CONSOLE;
        for (err_fails, con_fails, want) in [
            (false, false, MM_OK),
            (true, false, MM_NOMSG),
            (false, true, MM_NOCON),
            (true, true, MM_NOTOK),
        ] {
            let mut out = Capture {
                err_fails,
                con_fails,
                ..Capture::default()
            };
            assert_eq!(run(&env, &mut Table::new(), &mut out, both, 2), want);
        }
    }

    /// No length is too long: the message is built whole, however big.
    #[test]
    fn a_long_message_is_written_whole() {
        let env = Env {
            msgverb: None,
            sev_level: None,
        };
        let text = vec![b'x'; 100_000];
        let mut out = Capture::default();
        let r = fmtmsg_in(
            &mut Table::new(),
            &env,
            &mut out,
            MM_PRINT,
            None,
            MM_NOSEV,
            Some(&text),
            None,
            None,
        );
        assert_eq!(r, MM_OK);
        assert_eq!(out.err.len(), 100_001);
    }

    /// SEV_LEVEL's level is strtol's, base 0; a level of MM_INFO or below,
    /// or with no comma after it, defines nothing.
    #[test]
    fn sev_level_reads_its_levels_as_strtol() {
        let env = Env {
            msgverb: None,
            sev_level: Some(b"h,0x6,HEX:o,010,OCT:low,4,LOW:n,7:e,9,"),
        };
        let mut table = Table::new();
        table.ensure_ready(&env);
        assert_eq!(table.string(6), Some(&b"HEX"[..]));
        assert_eq!(table.string(8), Some(&b"OCT"[..]));
        assert_eq!(
            table.string(4),
            Some(&b"INFO"[..]),
            "the standard level stands"
        );
        assert_eq!(table.string(7), None, "no comma after the level");
        assert_eq!(
            table.string(9),
            Some(&b""[..]),
            "an empty string is a string"
        );
        assert_eq!(strtol(b"  -12x", 0), (-12, 5));
        assert_eq!(strtol(b"abc", 0), (0, 0));
        assert_eq!(
            strtol(b"0x", 0),
            (0, 1),
            "a 0 whose x has no digit after it"
        );
    }

    /// addseverity keeps the caller's string, as glibc does: what it prints
    /// is what the string says at the time.
    #[test]
    fn addseverity_keeps_the_callers_string() {
        let env = Env {
            msgverb: None,
            sev_level: None,
        };
        let mut table = Table::new();
        let mut s = *b"ONE\0";
        assert_eq!(addseverity_in(&mut table, &env, 6, s.as_ptr()), MM_OK);
        s[..3].copy_from_slice(b"TWO");
        let mut out = Capture::default();
        assert_eq!(run(&env, &mut table, &mut out, MM_PRINT, 6), MM_OK);
        assert_eq!(out.err, b"UX:cat: TWO: t\n");
        drop(table);
    }

    /// The C functions: the process's table and environment, standard
    /// error's stream, and /dev/console -- which a host test cannot open.
    #[test]
    fn the_c_functions_reach_the_table_and_the_console() {
        // SAFETY: C strings, kept for the rest of the process.
        unsafe {
            assert_eq!(addseverity(9, c"NINE".as_ptr().cast()), MM_OK);
            assert_eq!(addseverity(MM_INFO, c"X".as_ptr().cast()), MM_NOTOK);
            let label = c"UX:cat".as_ptr().cast();
            assert_eq!(
                fmtmsg(MM_NULLMC, label, 9, MM_NULLTXT, MM_NULLACT, MM_NULLTAG),
                MM_OK
            );
            assert_eq!(
                fmtmsg(MM_NULLMC, label, 10, MM_NULLTXT, MM_NULLACT, MM_NULLTAG),
                MM_NOTOK
            );
            assert_eq!(
                fmtmsg(MM_CONSOLE, label, 9, MM_NULLTXT, MM_NULLACT, MM_NULLTAG),
                MM_NOCON
            );
            assert_eq!(addseverity(9, core::ptr::null()), MM_OK);
            assert_eq!(
                fmtmsg(MM_NULLMC, label, 9, MM_NULLTXT, MM_NULLACT, MM_NULLTAG),
                MM_NOTOK
            );
        }
        // The stream's side of System, over a captured stream.
        let pair = crate::error::capture::Pair::new();
        let mut system = System {
            stream: pair.stderr,
        };
        assert!(system.stderr(b"UX:cat: ERROR: t\n"));
        assert_eq!(pair.text(), "UX:cat: ERROR: t\n");
    }

    // -- the constants ---------------------------------------------------

    #[test]
    fn classification_is_musls() {
        let bits = [
            MM_HARD, MM_SOFT, MM_FIRM, MM_APPL, MM_UTIL, MM_OPSYS, MM_RECOVER, MM_NRECOV, MM_PRINT,
            MM_CONSOLE,
        ];
        for (i, b) in bits.iter().enumerate() {
            assert_eq!(*b, 1 << i);
        }
        assert_eq!(MM_NULLMC, 0);
        assert_eq!(
            (MM_NOSEV, MM_HALT, MM_ERROR, MM_WARNING, MM_INFO),
            (0, 1, 2, 3, 4)
        );
        assert_eq!((MM_OK, MM_NOTOK, MM_NOMSG, MM_NOCON), (0, -1, 1, 4));
    }
}
