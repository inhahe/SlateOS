//! A compiled terminfo entry: `_nc_read_termtype` (`read_entry.c`), and the
//! quick-dump spellings `_nc_read_tic_entry` accepts in place of a
//! directory.
//!
//! The file is little-endian 16-bit words: a header of six (the magic, then
//! the sizes of the names, booleans, numbers, string offsets and string
//! table), the names, a byte per boolean -- padded to an even offset -- a
//! number each (16 bits, or 32 with the magic 01036), an offset per string
//! into the table that follows, and then, optionally, the same again for
//! the *extended* capabilities, which carry their names with them. Every
//! check upstream makes is made here, in its order, and a failure is
//! `TGETENT_NO` -- the entry is not there -- exactly as upstream's is, so
//! the search goes on to the next directory.

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

/// `MAGIC`: the classic format, numbers in 16 bits.
const MAGIC: u16 = 0o432;
/// `MAGIC2`: the extended-numbers format, numbers in 32 bits.
const MAGIC2: u16 = 0o1036;
/// `MAX_NAME_SIZE`: the longest name field read, and the longest terminal
/// name `setupterm` accepts.
pub(crate) const MAX_NAME_SIZE: usize = 512;
/// `MAX_ENTRY_SIZE1`: the size limit of a classic entry.
const MAX_ENTRY_SIZE1: usize = 4096;
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

    /// The extended string capability called `name` -- `tigetstr` for a name
    /// that is not a standard one -- or `None`.
    #[must_use]
    pub fn ext_string(&self, name: &[u8]) -> Option<&[u8]> {
        let first = self.ext_booleans.saturating_add(self.ext_numbers);
        (0..self.ext_strings)
            .find(|&j| {
                self.ext_names
                    .get(first.saturating_add(j))
                    .and_then(Option::as_deref)
                    == Some(name)
            })
            .and_then(|j| self.string(STRCOUNT.saturating_add(j)))
    }

    /// The extended number called `name`: [`ABSENT_NUMERIC`] when there is
    /// none, as `tigetnum` answers for a valid name the entry lacks.
    #[must_use]
    pub fn ext_number(&self, name: &[u8]) -> i32 {
        (0..self.ext_numbers)
            .find(|&j| {
                self.ext_names
                    .get(self.ext_booleans.saturating_add(j))
                    .and_then(Option::as_deref)
                    == Some(name)
            })
            .map_or(ABSENT_NUMERIC, |j| {
                let n = self.number(NUMCOUNT.saturating_add(j));
                if n >= 0 { n } else { ABSENT_NUMERIC }
            })
    }
}

/// `LOW_MSB`.
fn low_msb(b: &[u8], at: usize) -> u16 {
    let lo = b.get(at).copied().unwrap_or(0);
    let hi = b.get(at.saturating_add(1)).copied().unwrap_or(0);
    u16::from_le_bytes([lo, hi])
}

/// `MyNumber`: `(short) LOW_MSB`.
fn my_number(b: &[u8], at: usize) -> i32 {
    i32::from(i16::from_le_bytes(low_msb(b, at).to_le_bytes()))
}

/// The usize of a count already checked to be non-negative.
fn as_count(n: i32) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// `fake_read`: up to `want` bytes from where the last read stopped.
struct Reader<'a> {
    buffer: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn read(&mut self, want: usize) -> &'a [u8] {
        let have = self.buffer.len().saturating_sub(self.offset);
        let n = want.min(have);
        let start = self.offset;
        self.offset = self.offset.saturating_add(n);
        self.buffer
            .get(start..start.saturating_add(n))
            .unwrap_or_default()
    }

    /// `even_boundary (value)`: a byte skipped after an odd count.
    fn even_boundary(&mut self, value: usize) {
        if !value.is_multiple_of(2) {
            self.read(1);
        }
    }
}

/// `convert_strings`: a string per offset, or `None` for corrupt data.
/// `always` is for the extended names, which must all be there.
fn convert_strings(
    offsets: &[u8],
    count: usize,
    size: usize,
    table: &[u8],
    always: bool,
) -> Option<Vec<Option<Vec<u8>>>> {
    let mut out = Vec::with_capacity(count);
    let size_i = i32::try_from(size).unwrap_or(i32::MAX);
    for i in 0..count {
        let at = i.saturating_mul(2);
        let pair = (
            offsets.get(at).copied().unwrap_or(0),
            offsets.get(at.saturating_add(1)).copied().unwrap_or(0),
        );
        let nn = my_number(offsets, at);
        // First the offset: absent (-1), cancelled (-2, which
        // `_nc_setup_tinfo` makes absent), past the table (absent), in it,
        // or corrupt -- below -2, or at the table's very end.
        let start = if pair == (0o377, 0o377) || pair == (0o376, 0o377) || nn > size_i {
            None
        } else if nn >= 0 && nn < size_i {
            Some(as_count(nn))
        } else {
            return None;
        };
        // Then the string: one with no NUL before the end is ignored; with
        // `always`, an empty one or none at all is corrupt.
        let value = match start {
            Some(start) => {
                // Upstream scans to `table + size`, which for the extended
                // names runs past the table they are in; stopping at the end
                // of the bytes there are is upstream's verdict without its
                // over-read: no NUL, so no string.
                let rest = table.get(start..).unwrap_or_default();
                let limit = size.saturating_sub(start).min(rest.len());
                match rest
                    .get(..limit)
                    .and_then(|r| r.iter().position(|&b| b == 0))
                {
                    None => None,
                    Some(0) if always => return None,
                    Some(end) => Some(rest.get(..end).unwrap_or_default().to_vec()),
                }
            }
            None if always => return None,
            None => None,
        };
        out.push(value);
    }
    Some(out)
}

/// `valid_shorts`: whether any of `n` words is positive.
fn valid_shorts(b: &[u8], n: usize) -> bool {
    (0..n).any(|k| my_number(b, k.saturating_mul(2)) > 0)
}

/// Numbers of `width` bytes each: sign-extended from 16 bits, or 32 as
/// they are.
fn convert_numbers(b: &[u8], n: usize, width: usize) -> Vec<i32> {
    (0..n)
        .map(|k| {
            let at = k.saturating_mul(width);
            if width == 2 {
                my_number(b, at)
            } else {
                let q = |o: usize| b.get(at.saturating_add(o)).copied().unwrap_or(0);
                i32::from_le_bytes([q(0), q(1), q(2), q(3)])
            }
        })
        .collect()
}

/// `_nc_read_termtype`: the entry in `buffer`, or `None` -- `TGETENT_NO` --
/// for anything upstream rejects.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's _nc_read_termtype, in one piece so it reads against it"
)]
pub(crate) fn read_termtype(buffer: &[u8]) -> Option<Entry> {
    let mut r = Reader { buffer, offset: 0 };
    let header = r.read(12);
    if header.len() != 12 {
        return None;
    }
    let magic = low_msb(header, 0);
    let (number_width, max_entry_size) = match magic {
        MAGIC2 => (4usize, MAX_ENTRY_SIZE),
        MAGIC => (2usize, MAX_ENTRY_SIZE1),
        _ => return None,
    };
    let name_size = my_number(header, 2);
    let bool_count = my_number(header, 4);
    let num_count = my_number(header, 6);
    let str_count = my_number(header, 8);
    let str_size = my_number(header, 10);
    if name_size < 0
        || bool_count < 0
        || num_count < 0
        || str_count < 0
        || as_count(bool_count) > BOOLCOUNT
        || as_count(num_count) > NUMCOUNT
        || as_count(str_count) > STRCOUNT
        || str_size < 0
    {
        return None;
    }
    let (name_size, bool_count, num_count, str_count, str_size) = (
        as_count(name_size),
        as_count(bool_count),
        as_count(num_count),
        as_count(str_count),
        as_count(str_size),
    );
    if str_count.saturating_mul(2) >= max_entry_size {
        return None;
    }

    // The names: as much of the field as fits, a short read zero-filled.
    let want = MAX_NAME_SIZE.min(name_size);
    let raw = r.read(want);
    let names = raw
        .get(..raw.iter().position(|&b| b == 0).unwrap_or(raw.len()))
        .unwrap_or_default()
        .to_vec();

    let raw_bools = r.read(bool_count);
    if raw_bools.len() < bool_count {
        return None;
    }
    let mut booleans: Vec<bool> = raw_bools.iter().map(|&b| b == 1).collect();
    r.even_boundary(name_size.saturating_add(bool_count));

    let raw_numbers = r.read(num_count.saturating_mul(number_width));
    if raw_numbers.len() != num_count.saturating_mul(number_width) {
        return None;
    }
    let mut numbers = convert_numbers(raw_numbers, num_count, number_width);

    let mut strings: Vec<Option<Vec<u8>>> = Vec::new();
    if str_count > 0 {
        let offsets = r.read(str_count.saturating_mul(2));
        if offsets.len() != str_count.saturating_mul(2) {
            return None;
        }
        let table = r.read(str_size);
        if table.len() != str_size {
            return None;
        }
        strings = convert_strings(offsets, str_count, str_size, table, false)?;
    }
    booleans.resize(BOOLCOUNT, false);
    numbers.resize(NUMCOUNT, ABSENT_NUMERIC);
    strings.resize(STRCOUNT, None);

    let mut entry = Entry {
        names,
        booleans,
        numbers,
        strings,
        ext_names: Vec::new(),
        ext_booleans: 0,
        ext_numbers: 0,
        ext_strings: 0,
    };

    // The extended capabilities, if the file goes on.
    r.even_boundary(str_size);
    let ext_header = r.read(10);
    if ext_header.len() == 10 && valid_shorts(ext_header, 5) {
        let ext_bool_count = my_number(ext_header, 0);
        let ext_num_count = my_number(ext_header, 2);
        let ext_str_count = my_number(ext_header, 4);
        let ext_str_usage = my_number(ext_header, 6);
        let ext_str_limit = my_number(ext_header, 8);
        let need_i = ext_bool_count
            .saturating_add(ext_num_count)
            .saturating_add(ext_str_count);
        let half = i32::try_from(max_entry_size / 2).unwrap_or(i32::MAX);
        let max_i = i32::try_from(max_entry_size).unwrap_or(i32::MAX);
        if need_i >= half
            || ext_str_usage >= max_i
            || ext_str_limit >= max_i
            || ext_bool_count < 0
            || ext_num_count < 0
            || ext_str_count < 0
            || ext_str_usage < 0
            || ext_str_limit < 0
        {
            return None;
        }
        let (ext_bools, ext_nums, ext_strs, ext_limit) = (
            as_count(ext_bool_count),
            as_count(ext_num_count),
            as_count(ext_str_count),
            as_count(ext_str_limit),
        );
        let need = ext_bools.saturating_add(ext_nums).saturating_add(ext_strs);

        if ext_bools != 0 {
            let b = r.read(ext_bools);
            if b.len() != ext_bools {
                return None;
            }
            entry.booleans.extend(b.iter().map(|&x| x == 1));
        }
        r.even_boundary(ext_bools);

        if ext_nums != 0 {
            let b = r.read(ext_nums.saturating_mul(number_width));
            if b.len() != ext_nums.saturating_mul(number_width) {
                return None;
            }
            entry
                .numbers
                .extend(convert_numbers(b, ext_nums, number_width));
        }

        if ext_strs.saturating_add(need) >= max_entry_size / 2 {
            return None;
        }
        let offsets = if ext_strs != 0 || need != 0 {
            let b = r.read(ext_strs.saturating_add(need).saturating_mul(2));
            if b.len() != ext_strs.saturating_add(need).saturating_mul(2) {
                return None;
            }
            b
        } else {
            &[][..]
        };

        let ext_table = if ext_limit != 0 {
            let t = r.read(ext_limit);
            if t.len() != ext_limit {
                return None;
            }
            t
        } else {
            &[][..]
        };

        let mut base = 0usize;
        if ext_strs != 0 {
            let values = convert_strings(offsets, ext_strs, ext_limit, ext_table, false)?;
            for v in values.iter().flatten() {
                base = base.saturating_add(v.len()).saturating_add(1);
            }
            entry.strings.extend(values);
        }

        if need != 0 {
            if ext_strs >= max_entry_size / 2 {
                return None;
            }
            let name_offsets = offsets
                .get(ext_strs.saturating_mul(2)..)
                .unwrap_or_default();
            let name_table = ext_table.get(base..).unwrap_or_default();
            entry.ext_names = convert_strings(name_offsets, need, ext_limit, name_table, true)?;
        }
        entry.ext_booleans = ext_bools;
        entry.ext_numbers = ext_nums;
        entry.ext_strings = ext_strs;
    }
    Some(entry)
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
        assert_eq!(e.ext_number(b"U8"), ABSENT_NUMERIC);
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
