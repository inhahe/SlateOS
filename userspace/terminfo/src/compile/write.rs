//! `write_entry.c`'s `_nc_write_object`: a description as a compiled entry
//! -- the file `tic` writes and [`crate::termtype::read_termtype`] reads.
//!
//! What is written stops at the last capability that is there: the
//! booleans up to the last one true, the numbers and strings up to the last
//! one not absent -- and, unless extended capabilities are being kept
//! (`-x`), short of the obsolete ones at the end of each list. Numbers are
//! 16 bits unless one of them does not fit, when the whole entry takes the
//! 32-bit format (magic `01036`). The extended capabilities follow, with
//! their names, when there are any that are set.

use super::MAX_ENTRY_SIZE;
use super::scan::cstr;
use crate::entry::{ABSENT_NUMERIC, BOOLCOUNT, NUMCOUNT, STRCOUNT};
use crate::termtype::{Str, TermType};

/// `BOOLWRITE`: the booleans written when obsolete ones are not.
const BOOLWRITE: usize = 37;
/// `NUMWRITE`.
const NUMWRITE: usize = 33;
/// `STRWRITE`.
const STRWRITE: usize = 394;
/// `MAGIC`.
const MAGIC: i32 = 0o432;
/// `MAGIC2`.
const MAGIC2: i32 = 0o1036;
/// `MAX_OF_TYPE (NCURSES_COLOR_T)`: the largest number of the 16-bit
/// format.
const MAX_SHORT: i32 = 32767;
/// `MAX_NAME_SIZE`.
const MAX_NAME_SIZE: usize = 512;

/// `LITTLE_ENDIAN (p, x)`: `LO` and `HI` as C computes them -- remainder
/// and quotient truncated toward zero -- each kept to its low byte.
fn little_endian(x: i64) -> [u8; 2] {
    let lo = (x % 256).to_le_bytes().first().copied().unwrap_or(0);
    let hi = (x / 256).to_le_bytes().first().copied().unwrap_or(0);
    [lo, hi]
}

/// `fake_write`: what fits of `src` appended, the buffer being `limit`
/// bytes; how many whole items of `size` bytes went.
struct Out {
    buf: Vec<u8>,
    limit: usize,
}

impl Out {
    fn write(&mut self, src: &[u8], size: usize, count: usize) -> usize {
        let have = self.limit.saturating_sub(self.buf.len());
        let mut want = count.saturating_mul(size);
        if have > 0 {
            want = want.min(have);
            self.buf.extend_from_slice(src.get(..want).unwrap_or(src));
        } else {
            want = 0;
        }
        want.checked_div(size).unwrap_or(0)
    }

    /// `even_boundary (value)`: a zero byte after an odd count; false if it
    /// would not fit.
    fn even_boundary(&mut self, value: usize) -> bool {
        value.is_multiple_of(2) || self.write(&[0], 1, 1) == 1
    }

    /// `WRITE_STRING`: a string and its NUL.
    fn write_string(&mut self, s: &[u8]) -> bool {
        let mut v = cstr(s).to_vec();
        v.push(0);
        let n = v.len();
        self.write(&v, 1, n) == n
    }
}

/// `compute_offsets`: each string's offset in the table, -1 absent and -2
/// cancelled; and the table's size.
fn compute_offsets<'a>(
    strings: impl Iterator<Item = Option<&'a [u8]>> + 'a,
    cancelled: &[bool],
) -> (Vec<i64>, i64) {
    let mut nextfree: i64 = 0;
    let mut offsets = Vec::new();
    for (i, s) in strings.enumerate() {
        match s {
            None if cancelled.get(i).copied().unwrap_or(false) => offsets.push(-2),
            None => offsets.push(-1),
            Some(v) => {
                // `(short) nextfree`.
                let short = i64::from(i16::from_le_bytes(
                    nextfree
                        .to_le_bytes()
                        .get(..2)
                        .and_then(|b| b.try_into().ok())
                        .unwrap_or([0, 0]),
                ));
                offsets.push(short);
                nextfree = nextfree
                    .saturating_add(i64::try_from(cstr(v).len()).unwrap_or(i64::MAX))
                    .saturating_add(1);
            }
        }
    }
    (offsets, nextfree)
}

/// `convert_shorts`: offsets (or numbers) as 16-bit words, -1 and -2 as
/// `377 377` and `376 377`.
fn convert_shorts(values: &[i64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len().saturating_mul(2));
    for &v in values {
        match v {
            -1 => out.extend_from_slice(&[0o377, 0o377]),
            -2 => out.extend_from_slice(&[0o376, 0o377]),
            _ => out.extend_from_slice(&little_endian(v)),
        }
    }
    out
}

/// `convert_16bit` / `convert_32bit`: numbers as unsigned little-endian
/// words of `width` bytes.
fn convert_numbers(values: &[i32], width: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len().saturating_mul(width));
    for &v in values {
        let bytes = v.to_le_bytes();
        out.extend_from_slice(bytes.get(..width).unwrap_or(&bytes));
    }
    out
}

/// `extended_object`: whether, keeping extended capabilities, any is set.
fn extended_object(tp: &TermType, user_definable: bool) -> bool {
    if !user_definable {
        return false;
    }
    let b = tp
        .booleans
        .iter()
        .skip(BOOLCOUNT)
        .take(tp.ext_booleans)
        .any(|&v| v == 1);
    let n = tp
        .numbers
        .iter()
        .skip(NUMCOUNT)
        .take(tp.ext_numbers)
        .any(|&v| v != ABSENT_NUMERIC);
    let s = tp
        .strings
        .iter()
        .skip(STRCOUNT)
        .take(tp.ext_strings)
        .any(|v| *v != Str::Absent);
    b || n || s
}

/// `_nc_write_object (tp, buffer, &offset, limit)`: the compiled entry, or
/// `None` -- `ERR` -- if it does not fit in `limit` bytes.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's _nc_write_object, in one piece so it reads against it"
)]
#[must_use]
pub fn write_object(tp: &TermType, user_definable: bool, limit: usize) -> Option<Vec<u8>> {
    let (last_bool, last_num, last_str) = if user_definable {
        (BOOLCOUNT, NUMCOUNT, STRCOUNT)
    } else {
        (BOOLWRITE, NUMWRITE, STRWRITE)
    };
    let names = cstr(&tp.term_names);
    let mut namelen = names.len().saturating_add(1);

    let boolmax = (0..last_bool)
        .rfind(|&i| tp.booleans.get(i) == Some(&1))
        .map_or(0, |i| i.saturating_add(1));
    let mut nummax = 0usize;
    let mut need_ints = false;
    for i in 0..last_num {
        let v = tp.numbers.get(i).copied().unwrap_or(ABSENT_NUMERIC);
        if v != ABSENT_NUMERIC {
            nummax = i.saturating_add(1);
            if v > MAX_SHORT {
                need_ints = true;
            }
        }
    }
    let strmax = (0..last_str)
        .rfind(|&i| tp.strings.get(i).is_some_and(|s| *s != Str::Absent))
        .map_or(0, |i| i.saturating_add(1));
    let std_strings: Vec<&Str> = (0..strmax)
        .map(|i| tp.strings.get(i).unwrap_or(&Str::Absent))
        .collect();
    let cancelled: Vec<bool> = std_strings.iter().map(|s| **s == Str::Cancelled).collect();
    let (offsets, nextfree) = compute_offsets(std_strings.iter().map(|s| s.valid()), &cancelled);

    let (magic, width) = if need_ints {
        (MAGIC2, 4usize)
    } else {
        (MAGIC, 2usize)
    };
    namelen = namelen.min(MAX_NAME_SIZE.saturating_add(1));
    let mut header = Vec::with_capacity(12);
    for v in [
        i64::from(magic),
        i64::try_from(namelen).unwrap_or(0),
        i64::try_from(boolmax).unwrap_or(0),
        i64::try_from(nummax).unwrap_or(0),
        i64::try_from(strmax).unwrap_or(0),
        nextfree,
    ] {
        header.extend_from_slice(&little_endian(v));
    }

    let mut out = Out {
        buf: Vec::new(),
        limit,
    };
    // The names: `namelen` bytes, which may cut a long field short of its
    // NUL.
    let mut name_bytes = names.to_vec();
    name_bytes.push(0);
    name_bytes.truncate(namelen);
    if out.write(&header, 12, 1) != 1 || out.write(&name_bytes, 1, namelen) != namelen {
        return None;
    }

    let bools: Vec<u8> = (0..boolmax)
        .map(|i| u8::from(tp.booleans.get(i) == Some(&1)))
        .collect();
    if out.write(&bools, 1, boolmax) != boolmax {
        return None;
    }
    if !out.even_boundary(namelen.saturating_add(boolmax)) {
        return None;
    }

    let numbers: Vec<i32> = (0..nummax)
        .map(|i| tp.numbers.get(i).copied().unwrap_or(ABSENT_NUMERIC))
        .collect();
    if out.write(&convert_numbers(&numbers, width), width, nummax) != nummax {
        return None;
    }

    if out.write(&convert_shorts(&offsets), 2, strmax) != strmax {
        return None;
    }
    for s in std_strings.iter().filter_map(|s| s.valid()) {
        if !out.write_string(s) {
            return None;
        }
    }

    if extended_object(tp, user_definable) {
        let ext_total = tp.num_ext_names();
        if !out.even_boundary(usize::try_from(nextfree).unwrap_or(0)) {
            return None;
        }
        let ext_strings: Vec<&Str> = (0..tp.ext_strings)
            .map(|i| {
                tp.strings
                    .get(STRCOUNT.saturating_add(i))
                    .unwrap_or(&Str::Absent)
            })
            .collect();
        let ext_cancelled: Vec<bool> = ext_strings.iter().map(|s| **s == Str::Cancelled).collect();
        let (mut offsets, mut nextfree) =
            compute_offsets(ext_strings.iter().map(|s| s.valid()), &ext_cancelled);
        if tp.ext_strings >= MAX_ENTRY_SIZE / 2 {
            return None;
        }
        let name_list: Vec<Option<&[u8]>> = (0..ext_total)
            .map(|i| tp.ext_names.get(i).and_then(Option::as_deref))
            .collect();
        let (name_offsets, names_size) = compute_offsets(name_list.iter().copied(), &[]);
        offsets.extend(name_offsets);
        nextfree = nextfree.saturating_add(names_size);
        let strmax = tp.ext_strings.saturating_add(ext_total);
        let ext_usage =
            ext_total.saturating_add(ext_strings.iter().filter(|s| s.present()).count());

        let mut header = Vec::with_capacity(10);
        for v in [tp.ext_booleans, tp.ext_numbers, tp.ext_strings, ext_usage] {
            header.extend_from_slice(&little_endian(i64::try_from(v).unwrap_or(0)));
        }
        header.extend_from_slice(&little_endian(nextfree));
        if out.write(&header, 10, 1) != 1 {
            return None;
        }

        // The extended booleans go out as they are -- a cancelled one as
        // its 0376.
        let ext_bools: Vec<u8> = (0..tp.ext_booleans)
            .map(|i| {
                tp.booleans
                    .get(BOOLCOUNT.saturating_add(i))
                    .map_or(0, |&b| b.to_le_bytes().first().copied().unwrap_or(0))
            })
            .collect();
        if tp.ext_booleans != 0 && out.write(&ext_bools, 1, tp.ext_booleans) != tp.ext_booleans {
            return None;
        }
        if !out.even_boundary(tp.ext_booleans) {
            return None;
        }
        if tp.ext_numbers != 0 {
            let ext_numbers: Vec<i32> = (0..tp.ext_numbers)
                .map(|i| {
                    tp.numbers
                        .get(NUMCOUNT.saturating_add(i))
                        .copied()
                        .unwrap_or(ABSENT_NUMERIC)
                })
                .collect();
            if out.write(&convert_numbers(&ext_numbers, width), width, tp.ext_numbers)
                != tp.ext_numbers
            {
                return None;
            }
        }
        if out.write(&convert_shorts(&offsets), 2, strmax) != strmax {
            return None;
        }
        for s in ext_strings.iter().filter_map(|s| s.valid()) {
            if !out.write_string(s) {
                return None;
            }
        }
        for n in &name_list {
            if !out.write_string(n.unwrap_or_default()) {
                return None;
            }
        }
    }
    Some(out.buf)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::entry::CANCELLED_NUMERIC;
    use crate::termtype::{CANCELLED_BOOLEAN, read_termtype};

    #[test]
    fn what_is_written_reads_back() {
        let mut t = TermType::init();
        t.term_names = b"x|xx|test".to_vec();
        t.booleans[1] = 1;
        t.booleans[3] = CANCELLED_BOOLEAN;
        t.numbers[0] = 80;
        t.numbers[2] = CANCELLED_NUMERIC;
        t.strings[1] = Str::Value(b"\x07".to_vec());
        t.strings[5] = Str::Cancelled;
        let file = write_object(&t, false, MAX_ENTRY_SIZE).unwrap();
        let back = read_termtype(&file, false).unwrap();
        assert_eq!(back.term_names, t.term_names);
        assert_eq!(&back.booleans[..4], &[0, 1, 0, 0]);
        assert_eq!(&back.numbers[..3], &[80, ABSENT_NUMERIC, CANCELLED_NUMERIC]);
        assert_eq!(back.strings[1], Str::Value(b"\x07".to_vec()));
        assert_eq!(back.strings[5], Str::Cancelled);
        // Magic, 16-bit: 0432.
        assert_eq!(&file[..2], &[0o32, 0o1]);
    }

    #[test]
    fn a_big_number_takes_the_32_bit_format() {
        let mut t = TermType::init();
        t.term_names = b"w".to_vec();
        t.numbers[0] = 100_000;
        let file = write_object(&t, false, MAX_ENTRY_SIZE).unwrap();
        assert_eq!(&file[..2], &[0o36, 0o2]);
        assert_eq!(read_termtype(&file, false).unwrap().numbers[0], 100_000);
    }

    #[test]
    fn extended_capabilities_are_written_with_their_names() {
        let mut t = TermType::init();
        t.term_names = b"e".to_vec();
        t.ext_names = vec![Some(b"XT".to_vec()), Some(b"Ms".to_vec())];
        t.booleans.push(1);
        t.strings.push(Str::Value(b"\x1b]52".to_vec()));
        t.ext_booleans = 1;
        t.ext_strings = 1;
        let file = write_object(&t, true, MAX_ENTRY_SIZE).unwrap();
        let back = read_termtype(&file, true).unwrap();
        assert_eq!(back.ext_names, t.ext_names);
        assert_eq!(back.booleans[BOOLCOUNT], 1);
        assert_eq!(back.strings[STRCOUNT], Str::Value(b"\x1b]52".to_vec()));
        // Without -x, none of it.
        let plain = write_object(&t, false, MAX_ENTRY_SIZE).unwrap();
        assert!(plain.len() < file.len());
    }

    #[test]
    fn an_entry_too_big_for_the_buffer_is_refused() {
        let mut t = TermType::init();
        t.term_names = b"big".to_vec();
        t.strings[1] = Str::Value(vec![b'a'; 100]);
        assert!(write_object(&t, false, 64).is_none());
    }
}
