//! `<ttyent.h>` — the terminal table, `/etc/ttys`, as glibc 2.39's
//! misc/getttyent.c reads it (4.4BSD's code).
//!
//! A line is `name getty type flags... # comment`: fields apart at blanks
//! and tabs, a field in double quotes keeping its blanks (`\"` a quote in
//! it), then any of `on`, `off` and `secure` (`TTY_ON`, `TTY_SECURE`) and
//! `window=command`, then the rest of the line after a `#`, or after the
//! first word none of those, as the comment.  Blank lines and `#` lines are
//! skipped, and so is a line of 100 bytes or more, or one that has no
//! newline (the file's last, unfinished) -- glibc's 100-byte line buffer,
//! kept here as it is there.
//!
//! `getttyent` reads the table's next entry, opening it first; `getttynam`
//! reads it from the start for a terminal's entry and closes it again;
//! `setttyent` opens or rewinds it (1, or 0 when it cannot be opened);
//! `endttyent` closes it (1, or 0 when the close fails).  One table and one
//! entry per process, as in glibc: the entry is overwritten by the next
//! call.  glibc 2.39's answers are `fsttys_oracle.txt`
//! (`posix/tools/oracle/fsttys_harness.py`), which the tests replay.

use crate::perprocess::process_global;

/// The table (glibc's `_PATH_TTYS`).
const PATH_TTYS: &core::ffi::CStr = c"/etc/ttys";

/// glibc's `MAXLINELENGTH`: `fgets`'s buffer, a byte of it the NUL.
const MAXLINELENGTH: usize = 100;

/// `ty_status`: logins enabled (`on`).
pub const TTY_ON: i32 = 0x01;
/// `ty_status`: root may log in (`secure`).
pub const TTY_SECURE: i32 = 0x02;

/// A terminal's entry (`struct ttyent`), glibc's layout.
#[repr(C)]
pub struct Ttyent {
    /// The terminal's device name.
    pub ty_name: *mut u8,
    /// The command started on it, usually a `getty`, or NULL.
    pub ty_getty: *mut u8,
    /// Its terminal type, or NULL.
    pub ty_type: *mut u8,
    /// [`TTY_ON`], [`TTY_SECURE`].
    pub ty_status: i32,
    /// The command that starts its window system, or NULL.
    pub ty_window: *mut u8,
    /// The line's comment, or NULL.
    pub ty_comment: *mut u8,
}

struct State {
    /// The table's stream, or NULL.
    tf: *mut u8,
    /// The line, which the entry's strings point into.
    line: [u8; MAXLINELENGTH],
    tty: Ttyent,
    /// The byte that ended the last field [`skip`] read: glibc's `zapchar`.
    zapchar: u8,
}

process_global! {
    fn state() -> State = State {
        tf: core::ptr::null_mut(),
        line: [0; MAXLINELENGTH],
        tty: Ttyent {
            ty_name: core::ptr::null_mut(),
            ty_getty: core::ptr::null_mut(),
            ty_type: core::ptr::null_mut(),
            ty_status: 0,
            ty_window: core::ptr::null_mut(),
            ty_comment: core::ptr::null_mut(),
        },
        zapchar: 0,
    };
}

/// The table, opened: `fopen(_PATH_TTYS, "rce")`.
fn open_table() -> *mut u8 {
    #[cfg(test)]
    return test_table::open();
    #[cfg(not(test))]
    // SAFETY: two C strings.
    unsafe {
        crate::stdio::fopen(PATH_TTYS.as_ptr().cast(), c"rce".as_ptr().cast())
    }
}

/// The byte at `i` of the line, NUL past its end.
fn at(line: &[u8; MAXLINELENGTH], i: usize) -> u8 {
    line.get(i).copied().unwrap_or(0)
}

fn put(line: &mut [u8; MAXLINELENGTH], i: usize, b: u8) {
    if let Some(slot) = line.get_mut(i) {
        *slot = b;
    }
}

/// C's `isspace` in the C locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// glibc's `skip`: take the field at `p` apart in place -- its quotes
/// removed, a `\"` in them made a `"`, the field ended at a blank, tab or
/// newline (and the ones after it passed over) or at a `#` -- and return
/// where the next field starts.
fn skip(line: &mut [u8; MAXLINELENGTH], zapchar: &mut u8, mut p: usize) -> usize {
    let mut quoted = false;
    let mut t = p;
    loop {
        let c = at(line, p);
        if c == 0 {
            break;
        }
        if c == b'"' {
            quoted = !quoted;
            p = p.wrapping_add(1);
            continue;
        }
        if quoted && c == b'\\' && at(line, p.wrapping_add(1)) == b'"' {
            p = p.wrapping_add(1);
        }
        let copied = at(line, p);
        put(line, t, copied);
        t = t.wrapping_add(1);
        if quoted {
            p = p.wrapping_add(1);
            continue;
        }
        if c == b'#' {
            *zapchar = c;
            put(line, p, 0);
            break;
        }
        if c == b'\t' || c == b' ' || c == b'\n' {
            *zapchar = c;
            put(line, p, 0);
            p = p.wrapping_add(1);
            while matches!(at(line, p), b'\t' | b' ' | b'\n') {
                p = p.wrapping_add(1);
            }
            break;
        }
        p = p.wrapping_add(1);
    }
    // glibc's `*--t = '\0'`: the byte before `t` -- the field's delimiter
    // copied in, or at the end of the line its last byte (a newline).
    put(line, t.wrapping_sub(1), 0);
    p
}

/// Whether the line at `p` starts with `word` and then white space (glibc's
/// `scmp`), or with `word` and then `=` (`vcmp`).
fn keyword(line: &[u8; MAXLINELENGTH], p: usize, word: &[u8], then: fn(u8) -> bool) -> bool {
    word.iter()
        .enumerate()
        .all(|(i, &b)| at(line, p.wrapping_add(i)) == b)
        && then(at(line, p.wrapping_add(word.len())))
}

/// A pointer to byte `i` of the line.
fn ptr_at(line: &mut [u8; MAXLINELENGTH], i: usize) -> *mut u8 {
    line.as_mut_ptr().wrapping_add(i)
}

/// The next entry of the open table into the state, or `false` at its end.
fn read_entry(st: &mut State) -> bool {
    let len = i32::try_from(MAXLINELENGTH).unwrap_or(i32::MAX);
    let mut p;
    loop {
        if crate::stdio::fgets(st.line.as_mut_ptr(), len, st.tf).is_null() {
            return false;
        }
        let end = st
            .line
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(MAXLINELENGTH);
        // A line too long for the buffer, or the file's unfinished last:
        // passed over to its end.
        if !st.line.get(..end).is_some_and(|l| l.contains(&b'\n')) {
            loop {
                let c = crate::stdio::getc_unlocked(st.tf);
                if c == i32::from(b'\n') || c == crate::stdio::EOF {
                    break;
                }
            }
            continue;
        }
        p = 0;
        while is_space(at(&st.line, p)) {
            p = p.wrapping_add(1);
        }
        let c = at(&st.line, p);
        if c != 0 && c != b'#' {
            break;
        }
    }
    let line = &mut st.line;
    let zap = &mut st.zapchar;
    *zap = 0;
    let name = p;
    p = skip(line, zap, p);
    let (getty, kind);
    if at(line, p) == 0 {
        getty = None;
        kind = None;
    } else {
        getty = Some(p);
        p = skip(line, zap, p);
        if at(line, p) == 0 {
            kind = None;
        } else {
            kind = Some(p);
            p = skip(line, zap, p);
        }
    }
    let mut status = 0;
    let mut window = None;
    while at(line, p) != 0 {
        if keyword(line, p, b"off", is_space) {
            status &= !TTY_ON;
        } else if keyword(line, p, b"on", is_space) {
            status |= TTY_ON;
        } else if keyword(line, p, b"secure", is_space) {
            status |= TTY_SECURE;
        } else if keyword(line, p, b"window", |b| b == b'=') {
            // glibc's `value`: after the `=`, which is there.
            window = line
                .get(p..)
                .and_then(|rest| {
                    rest.iter()
                        .take_while(|&&b| b != 0)
                        .position(|&b| b == b'=')
                })
                .map(|i| p.wrapping_add(i).wrapping_add(1));
        } else {
            break;
        }
        p = skip(line, zap, p);
    }
    if *zap == b'#' || at(line, p) == b'#' {
        loop {
            p = p.wrapping_add(1);
            let c = at(line, p);
            if c != b' ' && c != b'\t' {
                break;
            }
        }
    }
    let comment = (at(line, p) != 0).then_some(p);
    // `strchr`: the string's newline, not one past its NUL from an older line.
    if let Some(nl) = line.get(p..).and_then(|rest| {
        rest.iter()
            .take_while(|&&b| b != 0)
            .position(|&b| b == b'\n')
    }) {
        put(line, p.wrapping_add(nl), 0);
    }
    let mut ptr = |i: Option<usize>| i.map_or(core::ptr::null_mut(), |i| ptr_at(line, i));
    st.tty = Ttyent {
        ty_name: ptr(Some(name)),
        ty_getty: ptr(getty),
        ty_type: ptr(kind),
        ty_status: status,
        ty_window: ptr(window),
        ty_comment: ptr(comment),
    };
    true
}

/// The table's next entry, or NULL at its end or when it cannot be opened.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn getttyent() -> *mut Ttyent {
    // SAFETY: this process's state; these functions are not reentrant, as
    // glibc's are not.
    let st = unsafe { &mut *state() };
    if st.tf.is_null() && setttyent() == 0 {
        return core::ptr::null_mut();
    }
    if read_entry(st) {
        &raw mut st.tty
    } else {
        core::ptr::null_mut()
    }
}

/// Terminal `tty`'s entry, the table read from its start and closed after;
/// NULL if there is none.
///
/// # Safety
///
/// `tty` is NULL or a C string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getttynam(tty: *const u8) -> *mut Ttyent {
    setttyent();
    let mut found = core::ptr::null_mut();
    // Where glibc's `strcmp` faults, a NULL name: no terminal has none.
    if !tty.is_null() {
        loop {
            let t = getttyent();
            if t.is_null() {
                break;
            }
            // SAFETY: the entry's name and the caller's, both C strings.
            if unsafe { crate::string::strcmp((*t).ty_name, tty) } == 0 {
                found = t;
                break;
            }
        }
    }
    endttyent();
    found
}

/// Open the table, or rewind it: 1, or 0 when it cannot be opened.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn setttyent() -> i32 {
    // SAFETY: this process's state.
    let st = unsafe { &mut *state() };
    if st.tf.is_null() {
        st.tf = open_table();
        i32::from(!st.tf.is_null())
    } else {
        crate::stdio::rewind(st.tf);
        1
    }
}

/// Close the table: 1, or 0 when the close fails (1 when it is not open).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endttyent() -> i32 {
    // SAFETY: this process's state.
    let st = unsafe { &mut *state() };
    if st.tf.is_null() {
        return 1;
    }
    let closed = crate::stdio::fclose(st.tf) != crate::stdio::EOF;
    st.tf = core::ptr::null_mut();
    i32::from(closed)
}

/// The table the tests give: text in memory, or none.
#[cfg(test)]
pub(crate) mod test_table {
    use std::cell::Cell;

    std::thread_local! {
        static TEXT: Cell<Option<&'static [u8]>> = const { Cell::new(None) };
    }

    /// Make `text` this thread's `/etc/ttys`, or have none.
    pub(crate) fn set(text: Option<&'static [u8]>) {
        TEXT.with(|t| t.set(text));
    }

    /// A read-only stream over it, or NULL.
    pub(super) fn open() -> *mut u8 {
        let Some(text) = TEXT.with(Cell::get) else {
            return core::ptr::null_mut();
        };
        let bytes: &'static mut [u8] = std::boxed::Box::leak(text.to_vec().into_boxed_slice());
        // SAFETY: a leaked buffer of the length given, which outlives the
        // stream.
        unsafe {
            crate::stdio_mem::fmemopen(bytes.as_mut_ptr().cast(), bytes.len(), c"r".as_ptr().cast())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers (posix/tools/oracle/fsttys_harness.py).
    const ORACLE: &str = include_str!("fsttys_oracle.txt");

    /// A string as the harness's `str` writes it.
    fn show(p: *const u8) -> String {
        if p.is_null() {
            return "(null)".into();
        }
        // SAFETY: a C string of the entry's.
        let bytes = unsafe { core::slice::from_raw_parts(p, crate::string::strlen(p)) };
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

    /// The harness's escapes undone.
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
                        out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).unwrap());
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

    /// The oracle's `/etc/ttys`, leaked for the table to read.
    fn ttys() -> &'static [u8] {
        let line = ORACLE
            .lines()
            .find_map(|l| l.strip_prefix("input ttys = "))
            .unwrap();
        std::boxed::Box::leak(unescape(line).into_boxed_slice())
    }

    fn tty(what: &str, t: *mut Ttyent) -> String {
        // SAFETY: the table's entry, if any.
        match unsafe { t.as_ref() } {
            None => format!("{what} = NULL"),
            Some(t) => format!(
                "{what} = {}|{}|{}|{}|{}|{}",
                show(t.ty_name),
                show(t.ty_getty),
                show(t.ty_type),
                t.ty_status,
                show(t.ty_window),
                show(t.ty_comment)
            ),
        }
    }

    /// The harness's ttys half, call for call.
    fn ttys_run(run: &str) -> Vec<String> {
        let mut out = Vec::new();
        for n in 0..40 {
            let t = getttyent();
            out.push(tty(&format!("{run} getttyent #{n}"), t));
            if t.is_null() {
                break;
            }
        }
        out.push(tty(
            &format!("{run} getttyent at the end again"),
            getttyent(),
        ));
        out.push(format!("{run} setttyent = {}", setttyent()));
        out.push(tty(
            &format!("{run} getttyent after setttyent"),
            getttyent(),
        ));
        for name in [
            "ttyv1", "ttyd0", "ttyq0", "ttylong", "ttyx0", "missing", "ttyp3", "",
        ] {
            let c = std::ffi::CString::new(name).unwrap();
            // SAFETY: a C string.
            let t = unsafe { getttynam(c.as_ptr().cast()) };
            out.push(tty(&format!("{run} getttynam({name})"), t));
            out.push(tty(
                &format!("{run} getttyent after getttynam({name})"),
                getttyent(),
            ));
        }
        out.push(format!("{run} endttyent = {}", endttyent()));
        out.push(format!("{run} endttyent again = {}", endttyent()));
        out.push(format!("{run} setttyent after endttyent = {}", setttyent()));
        out.push(format!("{run} endttyent = {}", endttyent()));
        out
    }

    /// Every probe of the harness's ttys half, with /etc/ttys and without:
    /// quoted fields, `\"`, `window=`, comments after `#` and after an
    /// unknown word, a line too long for glibc's buffer.
    #[test]
    fn the_table_is_read_as_glibc_reads_it() {
        for (run, text) in [("files", Some(ttys())), ("none", None)] {
            test_table::set(text);
            let got = ttys_run(run);
            let want: Vec<_> = ORACLE
                .lines()
                .filter(|l| l.starts_with(&format!("{run} ")) && l.contains("tty"))
                .collect();
            assert_eq!(got.len(), want.len(), "{run}");
            for (g, w) in got.iter().zip(&want) {
                assert_eq!(g, w);
            }
            let _ = endttyent();
        }
        test_table::set(None);
        assert_eq!(PATH_TTYS, c"/etc/ttys");
    }

    /// The file's last line without its newline is passed over, as glibc's
    /// is (it cannot tell it from a line too long for its buffer); a NULL
    /// name is no entry.
    #[test]
    fn an_unfinished_last_line_and_a_null_name() {
        test_table::set(Some(b"tty1 getty vt100 on\ntty2 getty vt100 on"));
        let first = getttyent();
        // SAFETY: the entry.
        assert_eq!(show(unsafe { (*first).ty_name }), "tty1");
        assert!(getttyent().is_null(), "tty2 has no newline");
        // SAFETY: NULL is refused.
        assert!(unsafe { getttynam(core::ptr::null()) }.is_null());
        let _ = endttyent();
        test_table::set(None);
    }

    #[test]
    fn ttyent_is_glibcs_layout() {
        assert_eq!(core::mem::size_of::<Ttyent>(), 48);
        assert_eq!(core::mem::offset_of!(Ttyent, ty_status), 24);
        assert_eq!(core::mem::offset_of!(Ttyent, ty_window), 32);
        assert_eq!((TTY_ON, TTY_SECURE), (1, 2));
    }
}
