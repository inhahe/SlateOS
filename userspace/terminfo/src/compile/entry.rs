//! `ENTRY` (`term_entry.h`) and `alloc_entry.c`: a terminal the compiler
//! has read -- its description, the entries its `use=` names, and where
//! in the source it was -- and the string table upstream saves every
//! string into as it reads.
//!
//! That table is one fixed buffer of `MAX_ENTRY_SIZE` bytes, emptied for
//! each entry, so an entry whose strings do not fit loses those that come
//! last, with a warning (`Too much data, some is lost`). The strings
//! themselves live here as owned values; what is kept of the table is its
//! fill, [`StrBuf`], so that the same strings are lost.

use super::scan::{Scanner, cstr};
use super::{Abort, MAX_ENTRY_SIZE};
use crate::entry::{ABSENT_NUMERIC, CANCELLED_NUMERIC};
use crate::termtype::{CANCELLED_BOOLEAN, Str, TermType};

/// `MAX_USES`: the most `use=` an entry may have.
pub const MAX_USES: usize = 32;

/// What a `use=` was resolved to: an entry of the source, by its place in
/// the list, or a compiled entry read from the database, by its place
/// among those.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// An entry of the in-core list.
    Core(usize),
    /// An entry read from the database.
    Disk(usize),
}

/// `ENTRY_USES`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Use {
    /// `name`: the name the `use=` gives.
    pub name: Option<Vec<u8>>,
    /// `link`: what it was resolved to.
    pub link: Option<Link>,
    /// `line`: the line it is on.
    pub line: i64,
}

/// `ENTRY`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// `tterm`.
    pub tterm: TermType,
    /// `nuses`: how many of `uses` are live -- counted down as they are
    /// merged.
    pub nuses: usize,
    /// `uses`.
    pub uses: Vec<Use>,
    /// `cstart`: where the comments before the entry start.
    pub cstart: i64,
    /// `cend`: where they end.
    pub cend: i64,
    /// `startline`: the line the entry starts on.
    pub startline: i64,
}

impl Entry {
    /// `uses[i]`, made to exist.
    pub fn use_mut(&mut self, i: usize) -> &mut Use {
        while self.uses.len() <= i {
            self.uses.push(Use::default());
        }
        let last = self.uses.len().saturating_sub(1);
        let i = i.min(last);
        // The vector was just made long enough.
        #[allow(clippy::indexing_slicing, reason = "i < len by the loop above")]
        &mut self.uses[i]
    }
}

/// `stringbuf` and `next_free`: whether the table exists, and how much of
/// it is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StrBuf {
    allocated: bool,
    next_free: usize,
}

impl StrBuf {
    /// `_nc_save_str (string)`: the string, if the table has room for it --
    /// an empty one is the end of the one before and takes none -- or
    /// `None`, with a warning, if it has not; `None` and nothing said
    /// before the first entry has made the table.
    pub fn save(&mut self, scan: &mut Scanner<'_>, string: &[u8]) -> Option<Vec<u8>> {
        if !self.allocated {
            return None;
        }
        let string = cstr(string);
        let len = string.len().saturating_add(1);
        if len == 1 && self.next_free != 0 {
            (self.next_free < MAX_ENTRY_SIZE).then(Vec::new)
        } else if self.next_free.saturating_add(len) < MAX_ENTRY_SIZE {
            self.next_free = self.next_free.saturating_add(len);
            Some(string.to_vec())
        } else {
            let mut m = b"Too much data, some is lost: ".to_vec();
            m.extend_from_slice(string);
            scan.warning(&m);
            None
        }
    }

    /// `_nc_save_str` into a string capability: absent where it did not
    /// fit.
    pub fn save_value(&mut self, scan: &mut Scanner<'_>, string: &[u8]) -> Str {
        self.save(scan, string).map_or(Str::Absent, Str::Value)
    }

    /// `_nc_init_entry`'s part: the table made, and emptied.
    pub fn init(&mut self) {
        self.allocated = true;
        self.next_free = 0;
    }
}

/// `_nc_init_entry (tp)`: an empty table, and a description with nothing
/// in it.
pub fn init_entry(strbuf: &mut StrBuf, ep: &mut Entry) {
    strbuf.init();
    ep.tterm = TermType::init();
}

/// `_nc_wrap_entry (ep, copy_strings)`: with `copy_strings`, every string
/// saved again into an emptied table -- which is where one that no longer
/// fits is lost. (The rest of what upstream does here is moving the strings
/// into memory of the entry's own, which owned strings already are.)
///
/// # Errors
///
/// Upstream's abort when no entry has made the table yet.
pub fn wrap_entry(
    strbuf: &mut StrBuf,
    scan: &mut Scanner<'_>,
    ep: &mut Entry,
    copy_strings: bool,
) -> Result<(), Abort> {
    if !strbuf.allocated {
        return Err(scan.err_abort(b"_nc_wrap_entry called without initialization"));
    }
    if copy_strings {
        strbuf.next_free = 0;
        let names = std::mem::take(&mut ep.tterm.term_names);
        ep.tterm.term_names = strbuf.save(scan, &names).unwrap_or_default();
        for i in 0..ep.tterm.strings.len() {
            let value = ep
                .tterm
                .strings
                .get(i)
                .and_then(Str::valid)
                .map(<[u8]>::to_vec);
            if let Some(v) = value {
                let saved = strbuf.save_value(scan, &v);
                if let Some(slot) = ep.tterm.strings.get_mut(i) {
                    *slot = saved;
                }
            }
        }
        for i in 0..ep.nuses {
            if ep.uses.get(i).is_some_and(|u| u.name.is_none()) {
                let saved = strbuf.save(scan, b"");
                ep.use_mut(i).name = saved;
            }
        }
    }
    Ok(())
}

/// `_nc_merge_entry (target, source)`: what `source` has merged into
/// `target`, as a `use=` brings it in: a value cancelled in the target
/// stays cancelled; one cancelled in the source comes in absent (a boolean
/// false); any other the source has comes in.
pub fn merge_entry(to: &mut TermType, from: &TermType) {
    let mut from = from.clone();
    to.align(&mut from);
    for (t, &f) in to.booleans.iter_mut().zip(&from.booleans) {
        if *t != CANCELLED_BOOLEAN {
            if f == CANCELLED_BOOLEAN {
                *t = 0;
            } else if f == 1 {
                *t = f;
            }
        }
    }
    for (t, &f) in to.numbers.iter_mut().zip(&from.numbers) {
        if *t != CANCELLED_NUMERIC {
            if f == CANCELLED_NUMERIC {
                *t = ABSENT_NUMERIC;
            } else if f != ABSENT_NUMERIC {
                *t = f;
            }
        }
    }
    for (t, f) in to.strings.iter_mut().zip(from.strings) {
        if *t != Str::Cancelled {
            match f {
                Str::Cancelled => *t = Str::Absent,
                Str::Absent => {}
                v @ Str::Value(_) => *t = v,
            }
        }
    }
}

/// `string_desc` and `_nc_safe_strcat`: a fixed buffer appended to while
/// what is appended fits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StrDesc {
    /// What has been appended.
    pub buf: Vec<u8>,
    size: usize,
}

impl StrDesc {
    /// `_nc_str_init (dst, buf, len)`: room for `len - 1` bytes.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            buf: Vec::new(),
            size: len.saturating_sub(1),
        }
    }

    /// `_nc_safe_strcat (dst, src)`: `src` appended if it is a string and
    /// fits; whether it was.
    pub fn cat(&mut self, src: Option<&[u8]>) -> bool {
        match src {
            Some(s) => {
                let s = cstr(s);
                if s.len() < self.size {
                    self.buf.extend_from_slice(s);
                    self.size = self.size.saturating_sub(s.len());
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }
}

/// `_nc_visbuf (buf)`: a string shown as the warnings show one -- quoted,
/// its unprintable bytes escaped.
#[must_use]
pub fn visbuf(s: &Str) -> Vec<u8> {
    let s = match s {
        Str::Absent => return b"(null)".to_vec(),
        Str::Cancelled => return b"(cancelled)".to_vec(),
        Str::Value(v) => cstr(v),
    };
    let mut out = vec![b'"'];
    for &c in s {
        match c {
            b'"' | b'\\' => {
                out.push(b'\\');
                out.push(c);
            }
            0x20..=0x7e => out.push(c),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            0x08 => out.extend_from_slice(b"\\b"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x1b => out.extend_from_slice(b"\\e"),
            0x7f => out.extend_from_slice(b"\\^?"),
            0..=0x1f => {
                out.extend_from_slice(b"\\^");
                out.push(b'@'.wrapping_add(c));
            }
            _ => out.extend_from_slice(format!("\\{c:03o}").as_bytes()),
        }
    }
    out.push(b'"');
    out
}

/// The byte at `i`, NUL past the end.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `_nc_capcmp (s, t)`: 0 when two strings are the same but for their
/// padding (`$<...>`), else the difference of the first bytes that differ
/// (as `char`s); a string against none is 1.
#[must_use]
pub fn capcmp(s: Option<&[u8]>, t: Option<&[u8]>) -> i32 {
    let (s, t) = match (s, t) {
        (Some(s), Some(t)) => (cstr(s), cstr(t)),
        (None, None) => return 0,
        _ => return 1,
    };
    let skip = |x: &[u8], mut i: usize| -> usize {
        if at(x, i) == b'$' && at(x, i.saturating_add(1)) == b'<' {
            i = i.saturating_add(2);
            while matches!(at(x, i), b'0'..=b'9' | b'.' | b'*' | b'/' | b'>') {
                i = i.saturating_add(1);
            }
        }
        i
    };
    let (mut i, mut j) = (0usize, 0usize);
    loop {
        i = skip(s, i);
        j = skip(t, j);
        let (a, b) = (at(s, i), at(t, j));
        if a == 0 && b == 0 {
            return 0;
        }
        if a != b {
            // Two `char`s apart: the difference cannot overflow.
            return i32::from(i8::from_le_bytes([b]))
                .wrapping_sub(i32::from(i8::from_le_bytes([a])));
        }
        i = i.saturating_add(1);
        j = j.saturating_add(1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::entry::{BOOLCOUNT, STRCOUNT};

    #[test]
    fn the_table_loses_what_does_not_fit() {
        let mut diag = Vec::new();
        {
            let mut scan = Scanner::new(&mut diag);
            let mut sb = StrBuf::default();
            assert_eq!(sb.save(&mut scan, b"x"), None);
            sb.init();
            // 32748 bytes and a NUL, then 9 more: 32758 of 32768.
            let big = vec![b'a'; MAX_ENTRY_SIZE - 20];
            assert_eq!(sb.save(&mut scan, &big).map(|v| v.len()), Some(big.len()));
            assert_eq!(sb.save(&mut scan, b"12345678"), Some(b"12345678".to_vec()));
            // An empty string takes no room.
            assert_eq!(sb.save(&mut scan, b""), Some(Vec::new()));
            // What would reach the size is refused: the fill must stay
            // below it.
            assert_eq!(sb.save(&mut scan, b"0123456789"), None);
            assert_eq!(sb.save(&mut scan, b"12345678"), Some(b"12345678".to_vec()));
            assert_eq!(sb.save(&mut scan, b""), Some(Vec::new()));
        }
        assert_eq!(
            String::from_utf8(diag).unwrap(),
            "\"?\": Too much data, some is lost: 0123456789\n"
        );
    }

    #[test]
    fn merging_keeps_the_targets_cancellations() {
        let mut to = TermType::init();
        let mut from = TermType::init();
        to.booleans[0] = CANCELLED_BOOLEAN;
        from.booleans[0] = 1;
        from.booleans[1] = 1;
        from.booleans[2] = CANCELLED_BOOLEAN;
        to.booleans[2] = 1;
        from.numbers[0] = 80;
        from.numbers[1] = CANCELLED_NUMERIC;
        to.numbers[1] = 24;
        to.strings[0] = Str::Cancelled;
        from.strings[0] = Str::Value(b"a".to_vec());
        from.strings[1] = Str::Value(b"b".to_vec());
        from.strings[2] = Str::Cancelled;
        to.strings[2] = Str::Value(b"c".to_vec());
        merge_entry(&mut to, &from);
        assert_eq!(&to.booleans[..3], &[CANCELLED_BOOLEAN, 1, 0]);
        assert_eq!(&to.numbers[..2], &[80, ABSENT_NUMERIC]);
        assert_eq!(
            &to.strings[..3],
            &[Str::Cancelled, Str::Value(b"b".to_vec()), Str::Absent]
        );
        assert_eq!(to.booleans.len(), BOOLCOUNT);
        assert_eq!(to.strings.len(), STRCOUNT);
    }

    #[test]
    fn padding_is_ignored_when_comparing() {
        assert_eq!(capcmp(Some(b"\t$<8>"), Some(b"\t")), 0);
        assert_eq!(capcmp(Some(b"a$<1*/>b"), Some(b"ab")), 0);
        assert_ne!(capcmp(Some(b"a"), Some(b"b")), 0);
        assert_eq!(capcmp(None, Some(b"b")), 1);
        assert_eq!(capcmp(None, None), 0);
    }

    #[test]
    fn strings_show_as_the_warnings_show_them() {
        assert_eq!(
            visbuf(&Str::Value(b"\x1b[%d\"\x01\x7f\xe9".to_vec())),
            b"\"\\e[%d\\\"\\^A\\^?\\351\""
        );
        assert_eq!(visbuf(&Str::Absent), b"(null)");
    }
}
