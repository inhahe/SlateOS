//! A terminal as `setupterm` leaves it: a compiled entry read by
//! `_nc_read_termtype` ([`crate::termtype::read_termtype`]) and then made
//! the program's by `_nc_setup_tinfo` -- and the quick-dump spellings
//! `_nc_read_tic_entry` accepts in place of a directory.
//!
//! The file is little-endian 16-bit words: a header of six (the magic, then
//! the sizes of the names, booleans, numbers, string offsets and string
//! table), the names, a byte per boolean -- padded to an even offset -- a
//! number each (16 bits, or 32 with the magic 01036), an offset per string
//! into the table that follows, and then, optionally, the same again for
//! the *extended* capabilities, which carry their names with them. Every
//! check upstream makes is made, in its order, and a failure is
//! `TGETENT_NO` -- the entry is not there -- exactly as upstream's is, so
//! the search goes on to the next directory.

use crate::termtype::{Str, TermType};
use crate::{Kind, names};

/// `BOOLCOUNT`: the standard booleans.
pub const BOOLCOUNT: usize = 44;
/// `NUMCOUNT`: the standard numbers.
pub const NUMCOUNT: usize = 39;
/// `STRCOUNT`: the standard strings.
pub const STRCOUNT: usize = 414;

/// `ABSENT_NUMERIC`: a number the entry does not give.
pub const ABSENT_NUMERIC: i32 = -1;
/// `CANCELLED_NUMERIC`: a number the entry cancels.
pub const CANCELLED_NUMERIC: i32 = -2;

/// `MAX_NAME_SIZE`: the longest name field read, and the longest terminal
/// name `setupterm` accepts.
pub(crate) const MAX_NAME_SIZE: usize = 512;
/// `MAX_ENTRY_SIZE`: the size limit of an entry, and how much of a file is
/// read (one byte more).
pub(crate) const MAX_ENTRY_SIZE: usize = 32768;

/// A terminal's description, as `setupterm` leaves it: the standard
/// capabilities by their index in `term.h` (see [`crate::boolean`],
/// [`crate::number`], [`crate::string`]), the extended ones by name.
///
/// As `_nc_setup_tinfo` leaves it, too: a boolean that is neither 0 nor 1
/// is false, and a cancelled string is an absent one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    names: Vec<u8>,
    booleans: Vec<bool>,
    numbers: Vec<i32>,
    strings: Vec<Option<Vec<u8>>>,
    /// The extended capabilities' names: the booleans', the numbers', the
    /// strings', in that order. `None` for a name the table does not end.
    ext_names: Vec<Option<Vec<u8>>>,
    ext_booleans: usize,
    ext_numbers: usize,
    ext_strings: usize,
}

impl Entry {
    /// The name field: every name of the terminal, `|` between them, the
    /// last a description.
    #[must_use]
    pub fn names(&self) -> &[u8] {
        &self.names
    }

    /// The boolean at `index`; false for one the entry does not have.
    #[must_use]
    pub fn flag(&self, index: usize) -> bool {
        self.booleans.get(index).copied().unwrap_or(false)
    }

    /// The number at `index`: [`ABSENT_NUMERIC`] or [`CANCELLED_NUMERIC`]
    /// for one the entry does not give.
    #[must_use]
    pub fn number(&self, index: usize) -> i32 {
        self.numbers.get(index).copied().unwrap_or(ABSENT_NUMERIC)
    }

    /// The string at `index`, or `None`.
    #[must_use]
    pub fn string(&self, index: usize) -> Option<&[u8]> {
        self.strings.get(index)?.as_deref()
    }

    /// Where the capability `name` of `kind` is: its standard index, else
    /// the index past the standard ones of the entry's own extended one of
    /// that name -- `_nc_find_type_entry`, then the extended names in order.
    fn index_of(&self, kind: Kind, name: &[u8]) -> Option<usize> {
        let (standard, count, first_name, ext_count): (&[&str], usize, usize, usize) = match kind {
            Kind::Boolean => (&names::BOOLNAMES, BOOLCOUNT, 0, self.ext_booleans),
            Kind::Number => (
                &names::NUMNAMES,
                NUMCOUNT,
                self.ext_booleans,
                self.ext_numbers,
            ),
            Kind::String => (
                &names::STRNAMES,
                STRCOUNT,
                self.ext_booleans.saturating_add(self.ext_numbers),
                self.ext_strings,
            ),
        };
        if let Some(i) = standard.iter().position(|n| n.as_bytes() == name) {
            return Some(i);
        }
        (0..ext_count)
            .find(|&j| {
                self.ext_names
                    .get(first_name.saturating_add(j))
                    .and_then(Option::as_deref)
                    == Some(name)
            })
            .map(|j| count.saturating_add(j))
    }

    /// The extended string capability called `name` -- `tigetstr` for a name
    /// that is not a standard one -- or `None`.
    #[must_use]
    pub fn ext_string(&self, name: &[u8]) -> Option<&[u8]> {
        match self.tigetstr(name) {
            TiString::Value(s) => Some(s),
            TiString::Absent | TiString::NotAString => None,
        }
    }

    /// `tigetflag (name)`: 1 or 0 for a boolean capability, standard or the
    /// entry's own, and -1 (`ABSENT_BOOLEAN`) for a name that is neither.
    #[must_use]
    pub fn tigetflag(&self, name: &[u8]) -> i32 {
        self.index_of(Kind::Boolean, name)
            .map_or(-1, |i| i32::from(self.flag(i)))
    }

    /// `tigetnum (name)`: the number, [`ABSENT_NUMERIC`] for one the entry
    /// does not give, and [`CANCELLED_NUMERIC`] for a name that is no
    /// numeric capability at all.
    #[must_use]
    pub fn tigetnum(&self, name: &[u8]) -> i32 {
        self.index_of(Kind::Number, name)
            .map_or(CANCELLED_NUMERIC, |i| {
                let n = self.number(i);
                if n >= 0 { n } else { ABSENT_NUMERIC }
            })
    }

    /// `tigetstr (name)`.
    #[must_use]
    pub fn tigetstr(&self, name: &[u8]) -> TiString<'_> {
        match self.index_of(Kind::String, name) {
            None => TiString::NotAString,
            Some(i) => self.string(i).map_or(TiString::Absent, TiString::Value),
        }
    }

    /// Set the number at `index` -- as `setupterm` puts the screen's size
    /// into `lines` and `cols`.
    pub fn set_number(&mut self, index: usize, value: i32) {
        if let Some(n) = self.numbers.get_mut(index) {
            *n = value;
        }
    }

    /// `_nc_tinfo_cmdch`'s rewrite: every `proto` byte of every string,
    /// standard and extended, made `cc`.
    pub(crate) fn replace_in_strings(&mut self, proto: u8, cc: u8) {
        for s in self.strings.iter_mut().flatten() {
            for b in s.iter_mut().filter(|b| **b == proto) {
                *b = cc;
            }
        }
    }

    /// `longname ()`: what follows the last `|` of the first 255 bytes of
    /// the name field (`ttytype`), or all of them when there is none.
    #[must_use]
    pub fn longname(&self) -> &[u8] {
        // `NAMESIZE - 1`.
        let ttytype = self
            .names
            .get(..self.names.len().min(255))
            .unwrap_or_default();
        match ttytype.iter().rposition(|&b| b == b'|') {
            Some(0) | None => ttytype,
            Some(bar) => ttytype.get(bar.saturating_add(1)..).unwrap_or_default(),
        }
    }
}

/// What `tigetstr` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TiString<'a> {
    /// `CANCELLED_STRING`: the name is no string capability, standard or the
    /// entry's own.
    NotAString,
    /// A null pointer: a string capability the terminal does not have.
    Absent,
    /// Its value.
    Value(&'a [u8]),
}

impl Entry {
    /// What `_nc_setup_tinfo` makes of a description it has read: a boolean
    /// that is neither 0 nor 1 false, a cancelled string absent.
    ///
    /// It is also how `tic`'s checks see an entry they are given as the
    /// current terminal: `tparm`, `tigetflag` and `tigetstr` answer alike
    /// for a cancelled capability and an absent one, so the difference this
    /// loses is one none of them could see.
    #[must_use]
    pub fn from_termtype(t: TermType) -> Self {
        Self {
            names: t.term_names,
            booleans: t.booleans.iter().map(|&b| b == 1).collect(),
            numbers: t.numbers,
            strings: t
                .strings
                .into_iter()
                .map(|s| match s {
                    Str::Value(v) => Some(v),
                    Str::Absent | Str::Cancelled => None,
                })
                .collect(),
            ext_names: t.ext_names,
            ext_booleans: t.ext_booleans,
            ext_numbers: t.ext_numbers,
            ext_strings: t.ext_strings,
        }
    }
}

/// `_nc_read_termtype`, then `_nc_setup_tinfo`: the terminal in `buffer`,
/// or `None` -- `TGETENT_NO` -- for anything upstream rejects.
pub(crate) fn read_termtype(buffer: &[u8]) -> Option<Entry> {
    crate::termtype::read_termtype(buffer, true).map(Entry::from_termtype)
}

/// `_nc_name_match (names, name, "|")`: whether `name` is one of the
/// names, whole.
#[must_use]
pub(crate) fn name_match(names: &[u8], name: &[u8]) -> bool {
    names.split(|&c| c == b'|').any(|n| n == name)
}

/// `decode_quickdump`: the bytes a `hex:` or `b64:` search-list element
/// spells, or nothing -- zero bytes, upstream's "not a quick-dump".
pub(crate) fn decode_quickdump(source: &[u8]) -> Vec<u8> {
    if let Some(rest) = source.strip_prefix(b"b64:") {
        decode_b64(rest)
    } else if let Some(rest) = source.strip_prefix(b"hex:") {
        decode_hex(rest)
    } else {
        Vec::new()
    }
}

/// The `b64:` form: four characters to three bytes, `=` padding.
fn decode_b64(mut s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while !s.is_empty() {
        let mut bits = [0u32; 4];
        let mut result: i32 = 3;
        for slot in &mut bits {
            let Some((&ch, rest)) = s.split_first() else {
                // The string ended inside a group: upstream reads its NUL,
                // which is no base-64 character.
                return Vec::new();
            };
            s = rest;
            *slot = match ch {
                b'A'..=b'Z' => u32::from(ch.wrapping_sub(b'A')),
                b'a'..=b'z' => u32::from(ch.wrapping_sub(b'a')).wrapping_add(26),
                b'0'..=b'9' => u32::from(ch.wrapping_sub(b'0')).wrapping_add(52),
                b'-' | b'+' => 62,
                b'_' | b'/' => 63,
                b'=' => {
                    result = result.saturating_sub(1);
                    64
                }
                _ => return Vec::new(),
            };
        }
        if out
            .len()
            .saturating_add(usize::try_from(result).unwrap_or(0))
            >= MAX_ENTRY_SIZE
        {
            return Vec::new();
        }
        let [b0, b1, b2, b3] = bits;
        out.push(low_byte(b0.wrapping_shl(2) | b1.wrapping_shr(4)));
        if b2 < 64 {
            out.push(low_byte(b1.wrapping_shl(4) | b2.wrapping_shr(2)));
            if b3 < 64 {
                out.push(low_byte(b2.wrapping_shl(6) | b3));
            }
        }
    }
    out
}

/// `(char) value`: the low byte.
fn low_byte(v: u32) -> u8 {
    v.to_le_bytes().first().copied().unwrap_or(0)
}

/// The `hex:` form: two digits a byte.
fn decode_hex(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut it = s.iter();
    while let Some(&hi) = it.next() {
        let Some(&lo) = it.next() else {
            return Vec::new();
        };
        let (Some(h), Some(l)) = (hex_digit(hi), hex_digit(lo)) else {
            return Vec::new();
        };
        if out.len() >= MAX_ENTRY_SIZE {
            return Vec::new();
        }
        out.push(h.wrapping_shl(4) | l);
    }
    out
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c.wrapping_sub(b'0')),
        b'A'..=b'F' => Some(c.wrapping_sub(b'A').wrapping_add(10)),
        b'a'..=b'f' => Some(c.wrapping_sub(b'a').wrapping_add(10)),
        _ => None,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
pub(crate) mod tests {
    use super::*;
    use crate::termtype::{MAGIC, MAGIC2};

    /// A compiled entry: `names`, booleans, numbers (16-bit unless
    /// `wide`), and strings by offset into the table that follows.
    pub(crate) fn compile(
        names: &[u8],
        bools: &[u8],
        nums: &[i32],
        strs: &[Option<&[u8]>],
        wide: bool,
    ) -> Vec<u8> {
        let mut table = Vec::new();
        let mut offsets: Vec<i16> = Vec::new();
        for s in strs {
            match s {
                Some(v) => {
                    offsets.push(i16::try_from(table.len()).unwrap());
                    table.extend_from_slice(v);
                    table.push(0);
                }
                None => offsets.push(-1),
            }
        }
        let mut names = names.to_vec();
        names.push(0);
        let mut t = Vec::new();
        let magic = if wide { MAGIC2 } else { MAGIC };
        for w in [
            magic,
            u16::try_from(names.len()).unwrap(),
            u16::try_from(bools.len()).unwrap(),
            u16::try_from(nums.len()).unwrap(),
            u16::try_from(strs.len()).unwrap(),
            u16::try_from(table.len()).unwrap(),
        ] {
            t.extend_from_slice(&w.to_le_bytes());
        }
        t.extend_from_slice(&names);
        t.extend_from_slice(bools);
        if t.len() % 2 == 1 {
            t.push(0);
        }
        for &n in nums {
            if wide {
                t.extend_from_slice(&n.to_le_bytes());
            } else {
                t.extend_from_slice(&i16::try_from(n).unwrap().to_le_bytes());
            }
        }
        for o in offsets {
            t.extend_from_slice(&o.to_le_bytes());
        }
        t.extend_from_slice(&table);
        t
    }

    #[test]
    fn standard_capabilities_read_through_their_offsets() {
        let file = compile(
            b"t|test",
            &[0, 1, 2],
            &[80, -1, -2],
            &[Some(b"\x1b[1m"), None, Some(b"")],
            false,
        );
        let e = read_termtype(&file).unwrap();
        assert_eq!(e.names(), b"t|test");
        assert_eq!(e.longname(), b"test");
        let solo = read_termtype(&compile(b"solo", &[], &[], &[], false)).unwrap();
        assert_eq!(solo.longname(), b"solo");
        let bar = read_termtype(&compile(b"|x", &[], &[], &[], false)).unwrap();
        assert_eq!(bar.longname(), b"|x");
        assert!(!e.flag(0));
        assert!(e.flag(1));
        // Neither 0 nor 1: false, as `_nc_setup_tinfo` makes it.
        assert!(!e.flag(2));
        assert_eq!(e.number(0), 80);
        assert_eq!(e.number(1), ABSENT_NUMERIC);
        assert_eq!(e.number(2), CANCELLED_NUMERIC);
        assert_eq!(e.number(NUMCOUNT - 1), ABSENT_NUMERIC);
        assert_eq!(e.string(0), Some(&b"\x1b[1m"[..]));
        assert_eq!(e.string(1), None);
        assert_eq!(e.string(2), Some(&b""[..]));
        assert_eq!(e.string(STRCOUNT - 1), None);
    }

    #[test]
    fn wide_numbers_are_32_bits() {
        let file = compile(b"w", &[], &[0x0100_0000, 8], &[], true);
        let e = read_termtype(&file).unwrap();
        assert_eq!(e.number(0), 0x0100_0000);
        assert_eq!(e.number(1), 8);
    }

    #[test]
    fn what_upstream_rejects_is_rejected() {
        assert!(read_termtype(b"short").is_none());
        let good = compile(b"x", &[1], &[1], &[Some(b"a")], false);
        // Bad magic.
        let mut bad = good.clone();
        bad[0] = 0;
        assert!(read_termtype(&bad).is_none());
        // More booleans than there are.
        let mut bad = good.clone();
        bad[4..6].copy_from_slice(&45u16.to_le_bytes());
        assert!(read_termtype(&bad).is_none());
        // A string offset at the very end of the table.
        let mut bad = good.clone();
        let at = bad.len() - 4;
        bad[at..at + 2].copy_from_slice(&2i16.to_le_bytes());
        assert!(read_termtype(&bad).is_none());
        // Truncated string table.
        let bad = &good[..good.len() - 1];
        assert!(read_termtype(bad).is_none());
    }

    #[test]
    fn extended_capabilities_are_found_by_name() {
        let mut file = compile(b"x", &[], &[], &[Some(b"std")], false);
        if file.len() % 2 == 1 {
            file.push(0);
        }
        // One extended boolean (XT), no numbers, one string (xm = "X").
        // Table: "X\0" then the names "XT\0xm\0".
        let table = b"X\0XT\0xm\0";
        for w in [1i16, 0, 1, 3, i16::try_from(table.len()).unwrap()] {
            file.extend_from_slice(&w.to_le_bytes());
        }
        file.push(1); // XT
        file.push(0); // even boundary
        // String offsets (xm at 0), then name offsets relative to the names.
        for o in [0i16, 0, 3] {
            file.extend_from_slice(&o.to_le_bytes());
        }
        file.extend_from_slice(table);
        let e = read_termtype(&file).unwrap();
        assert_eq!(e.string(0), Some(&b"std"[..]));
        assert_eq!(e.ext_string(b"xm"), Some(&b"X"[..]));
        assert_eq!(e.ext_string(b"XT"), None);
        assert!(e.flag(BOOLCOUNT));
        // tiget*: the extended names beside the standard ones.
        assert_eq!(e.tigetflag(b"XT"), 1);
        assert_eq!(e.tigetflag(b"bw"), 0);
        assert_eq!(e.tigetflag(b"xm"), -1);
        assert_eq!(e.tigetnum(b"U8"), CANCELLED_NUMERIC);
        assert_eq!(e.tigetnum(b"cols"), ABSENT_NUMERIC);
        assert_eq!(e.tigetstr(b"xm"), TiString::Value(b"X"));
        assert_eq!(e.tigetstr(b"cbt"), TiString::Value(b"std"));
        assert_eq!(e.tigetstr(b"bel"), TiString::Absent);
        assert_eq!(e.tigetstr(b"cols"), TiString::NotAString);
    }

    #[test]
    fn quick_dumps_decode_as_upstream_decodes_them() {
        assert_eq!(decode_quickdump(b"hex:1a02ff"), vec![0x1a, 0x02, 0xff]);
        assert_eq!(decode_quickdump(b"hex:1a0"), Vec::<u8>::new());
        assert_eq!(decode_quickdump(b"hex:zz"), Vec::<u8>::new());
        assert_eq!(decode_quickdump(b"b64:TWFu"), b"Man".to_vec());
        assert_eq!(decode_quickdump(b"b64:TWE="), b"Ma".to_vec());
        assert_eq!(decode_quickdump(b"b64:TW"), Vec::<u8>::new());
        assert_eq!(decode_quickdump(b"/usr/share/terminfo"), Vec::<u8>::new());
    }

    #[test]
    fn names_match_whole() {
        assert!(name_match(b"xterm|xterm terminal emulator", b"xterm"));
        assert!(!name_match(b"xterm-256color|xterm", b"xterm-256"));
        assert!(name_match(b"a|b|c", b"c"));
        assert!(!name_match(b"a|b|c", b""));
    }
}
