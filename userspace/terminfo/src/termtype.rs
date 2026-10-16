//! `TERMTYPE2`: a terminal's description as ncurses holds it before
//! `setupterm` makes it the program's -- every value raw, so an absent
//! capability and a cancelled one are told apart -- and `alloc_ttype.c`'s
//! keeping of the *extended* capabilities, the ones a description names
//! for itself.
//!
//! The standard capabilities are indexed as `term.h` indexes them; each
//! type's extended ones follow its standard ones, in the order of their
//! names, which `ext_names` lists -- the booleans', then the numbers', then
//! the strings', each run sorted. Two descriptions with different extended
//! capabilities are compared or merged only after [`TermType::align`] has
//! given both the union of the two runs (`_nc_align_termtype`).
//!
//! The index arithmetic here is upstream's, exactly, including where it is
//! off: `adjust_cancels` keeps using the position it computed before it
//! moved a name, and so reads the value of the string after the one it
//! means. The port keeps that; what `tic` and `infocmp` print depends on
//! it.

use crate::entry::{ABSENT_NUMERIC, BOOLCOUNT, CANCELLED_NUMERIC, NUMCOUNT, STRCOUNT};

/// `ABSENT_BOOLEAN`.
pub const ABSENT_BOOLEAN: i8 = -1;
/// `CANCELLED_BOOLEAN`.
pub const CANCELLED_BOOLEAN: i8 = -2;

/// A string capability's value: `ABSENT_STRING`, `CANCELLED_STRING`, or a
/// string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Str {
    /// `ABSENT_STRING`, a null pointer.
    #[default]
    Absent,
    /// `CANCELLED_STRING`, `(char *) -1`.
    Cancelled,
    /// A value.
    Value(Vec<u8>),
}

impl Str {
    /// `VALID_STRING`: the value, if there is one.
    #[must_use]
    pub fn valid(&self) -> Option<&[u8]> {
        match self {
            Self::Value(v) => Some(v),
            Self::Absent | Self::Cancelled => None,
        }
    }

    /// `PRESENT`: neither absent nor cancelled.
    #[must_use]
    pub fn present(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    /// `WANTED`: absent -- a cancelled capability is not wanted.
    #[must_use]
    pub fn wanted(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

/// `BOOLEAN`, `NUMBER`, `STRING` as `alloc_ttype.c` passes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtType {
    /// `BOOLEAN`.
    Boolean,
    /// `NUMBER`.
    Number,
    /// `STRING`.
    String,
}

/// `TERMTYPE2`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TermType {
    /// `term_names`: the name field.
    pub term_names: Vec<u8>,
    /// `Booleans`: 1, 0, or [`ABSENT_BOOLEAN`] or [`CANCELLED_BOOLEAN`] --
    /// or, from a compiled entry, any byte at all.
    pub booleans: Vec<i8>,
    /// `Numbers`.
    pub numbers: Vec<i32>,
    /// `Strings`.
    pub strings: Vec<Str>,
    /// `ext_Names`: the extended capabilities' names. `None` for one a
    /// compiled entry left unterminated, which upstream reads as a null
    /// pointer.
    pub ext_names: Vec<Option<Vec<u8>>>,
    /// `ext_Booleans`.
    pub ext_booleans: usize,
    /// `ext_Numbers`.
    pub ext_numbers: usize,
    /// `ext_Strings`.
    pub ext_strings: usize,
}

/// A C array index from upstream's signed arithmetic: `None` for one below
/// zero.
fn ix(i: i64) -> Option<usize> {
    usize::try_from(i).ok()
}

/// `i64` of a count.
fn n64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

impl TermType {
    /// `_nc_init_termtype`: every standard capability absent -- the
    /// booleans false -- and no extended ones.
    #[must_use]
    pub fn init() -> Self {
        Self {
            term_names: Vec::new(),
            booleans: vec![0; BOOLCOUNT],
            numbers: vec![ABSENT_NUMERIC; NUMCOUNT],
            strings: vec![Str::Absent; STRCOUNT],
            ext_names: Vec::new(),
            ext_booleans: 0,
            ext_numbers: 0,
            ext_strings: 0,
        }
    }

    /// `NUM_EXT_NAMES`.
    #[must_use]
    pub fn num_ext_names(&self) -> usize {
        self.ext_booleans
            .saturating_add(self.ext_numbers)
            .saturating_add(self.ext_strings)
    }

    /// `ext_Names[i]`, as a C string: empty for a null one, where upstream
    /// would follow the null pointer.
    #[must_use]
    pub fn ext_name(&self, i: usize) -> &[u8] {
        self.ext_names
            .get(i)
            .and_then(Option::as_deref)
            .unwrap_or_default()
    }

    /// `_nc_first_ext_name`.
    fn first_ext_name(&self, t: ExtType) -> usize {
        match t {
            ExtType::Boolean => 0,
            ExtType::Number => self.ext_booleans,
            ExtType::String => self.ext_booleans.saturating_add(self.ext_numbers),
        }
    }

    /// `_nc_last_ext_name`.
    fn last_ext_name(&self, t: ExtType) -> usize {
        match t {
            ExtType::Boolean => self.ext_booleans,
            ExtType::Number => self.ext_booleans.saturating_add(self.ext_numbers),
            ExtType::String => self.num_ext_names(),
        }
    }

    /// `_nc_find_ext_name`: where `name` is among the extended names of
    /// type `t`.
    #[must_use]
    pub fn find_ext_name(&self, name: &[u8], t: ExtType) -> Option<usize> {
        (self.first_ext_name(t)..self.last_ext_name(t)).find(|&j| self.ext_name(j) == name)
    }

    /// `_nc_ext_data_index`: the data index of extended name `n`.
    fn ext_data_index(&self, n: i64, t: ExtType) -> i64 {
        match t {
            ExtType::Boolean => n
                .wrapping_add(n64(self.booleans.len()))
                .wrapping_sub(n64(self.ext_booleans)),
            ExtType::Number => n
                .wrapping_add(n64(self.numbers.len()).wrapping_sub(n64(self.ext_numbers)))
                .wrapping_sub(n64(self.ext_booleans)),
            ExtType::String => n
                .wrapping_add(n64(self.strings.len()).wrapping_sub(n64(self.ext_strings)))
                .wrapping_sub(n64(self.ext_booleans).wrapping_add(n64(self.ext_numbers))),
        }
    }

    /// `_nc_del_ext_name`: the extended capability `name` of type `t`
    /// removed, name and value; whether it was there.
    fn del_ext_name(&mut self, name: &[u8], t: ExtType) -> bool {
        let Some(first) = self.find_ext_name(name, t) else {
            return false;
        };
        if first < self.ext_names.len() {
            self.ext_names.remove(first);
        }
        let at = ix(self.ext_data_index(n64(first), t));
        match t {
            ExtType::Boolean => {
                if let Some(at) = at.filter(|&a| a < self.booleans.len()) {
                    self.booleans.remove(at);
                } else {
                    self.booleans.pop();
                }
                self.ext_booleans = self.ext_booleans.saturating_sub(1);
            }
            ExtType::Number => {
                if let Some(at) = at.filter(|&a| a < self.numbers.len()) {
                    self.numbers.remove(at);
                } else {
                    self.numbers.pop();
                }
                self.ext_numbers = self.ext_numbers.saturating_sub(1);
            }
            ExtType::String => {
                if let Some(at) = at.filter(|&a| a < self.strings.len()) {
                    self.strings.remove(at);
                } else {
                    self.strings.pop();
                }
                self.ext_strings = self.ext_strings.saturating_sub(1);
            }
        }
        true
    }

    /// `_nc_ins_ext_name`: room made for the extended capability `name` of
    /// type `t`, in order -- its value whatever was at its place before, as
    /// upstream leaves it for the caller to set -- and its data index.
    fn ins_ext_name(&mut self, name: &[u8], t: ExtType) -> usize {
        let first = self.first_ext_name(t);
        let last = self.last_ext_name(t);
        let mut j = first;
        while j < last {
            match name.cmp(self.ext_name(j)) {
                std::cmp::Ordering::Equal => {
                    return ix(self.ext_data_index(n64(j), t)).unwrap_or(0);
                }
                std::cmp::Ordering::Less => break,
                std::cmp::Ordering::Greater => j = j.saturating_add(1),
            }
        }
        let at_name = j.min(self.ext_names.len());
        self.ext_names.insert(at_name, Some(name.to_vec()));
        let data = ix(self.ext_data_index(n64(j), t)).unwrap_or(0);
        match t {
            ExtType::Boolean => {
                self.ext_booleans = self.ext_booleans.saturating_add(1);
                open(&mut self.booleans, data);
            }
            ExtType::Number => {
                self.ext_numbers = self.ext_numbers.saturating_add(1);
                open(&mut self.numbers, data);
            }
            ExtType::String => {
                self.ext_strings = self.ext_strings.saturating_add(1);
                open(&mut self.strings, data);
            }
        }
        data
    }

    /// `adjust_cancels (to, from)`: a cancelled extended string of `self`
    /// whose name `from` has as a boolean or number made one of those --
    /// with upstream's stale positions.
    fn adjust_cancels(&mut self, from: &Self) {
        let first = n64(self.ext_booleans).wrapping_add(n64(self.ext_numbers));
        let last = first.wrapping_add(n64(self.ext_strings));
        let mut j = first;
        while j < last {
            let name = ix(j)
                .and_then(|i| self.ext_names.get(i))
                .cloned()
                .flatten()
                .unwrap_or_default();
            let j_str = n64(self.strings.len())
                .wrapping_sub(first)
                .wrapping_sub(n64(self.ext_strings));
            let cancelled = ix(j.wrapping_add(j_str))
                .and_then(|i| self.strings.get(i))
                .is_some_and(|s| *s == Str::Cancelled);
            if !cancelled {
                j = j.wrapping_add(1);
                continue;
            }
            if from.find_ext_name(&name, ExtType::Boolean).is_some() {
                if self.del_ext_name(&name, ExtType::String)
                    || self.del_ext_name(&name, ExtType::Number)
                {
                    let k = self.ins_ext_name(&name, ExtType::Boolean);
                    if let Some(b) = self.booleans.get_mut(k) {
                        *b = 0;
                    }
                } else {
                    j = j.wrapping_add(1);
                }
            } else if from.find_ext_name(&name, ExtType::Number).is_some() {
                if self.del_ext_name(&name, ExtType::String)
                    || self.del_ext_name(&name, ExtType::Boolean)
                {
                    let k = self.ins_ext_name(&name, ExtType::Number);
                    if let Some(n) = self.numbers.get_mut(k) {
                        *n = CANCELLED_NUMERIC;
                    }
                } else {
                    j = j.wrapping_add(1);
                }
            } else if from.find_ext_name(&name, ExtType::String).is_some() {
                if self.del_ext_name(&name, ExtType::Number)
                    || self.del_ext_name(&name, ExtType::Boolean)
                {
                    let k = self.ins_ext_name(&name, ExtType::String);
                    if let Some(s) = self.strings.get_mut(k) {
                        *s = Str::Cancelled;
                    }
                } else {
                    j = j.wrapping_add(1);
                }
            } else {
                j = j.wrapping_add(1);
            }
        }
    }

    /// `realign_data (to, ext_Names, ...)`: the values of `self` laid out
    /// for the merged extended names.
    fn realign_data(
        &mut self,
        ext_names: &[Option<Vec<u8>>],
        ext_booleans: usize,
        ext_numbers: usize,
        ext_strings: usize,
    ) {
        let old_names = self.ext_names.clone();
        let (to_booleans, to_numbers, to_strings) =
            (self.ext_booleans, self.ext_numbers, self.ext_strings);
        let find = |lo: usize, hi: usize, name: &Option<Vec<u8>>| -> bool {
            let name = name.as_deref().unwrap_or_default();
            (lo..hi).any(|n| {
                old_names
                    .get(n)
                    .and_then(Option::as_deref)
                    .unwrap_or_default()
                    == name
            })
        };

        if self.ext_booleans != ext_booleans {
            let (to1, from) = (0usize, 0usize);
            let to2 = to_booleans.saturating_add(to1);
            let new_len = n64(self.booleans.len())
                .wrapping_add(n64(ext_booleans))
                .wrapping_sub(n64(self.ext_booleans))
                .max(0);
            let old = std::mem::take(&mut self.booleans);
            let mut v = old.clone();
            v.resize(ix(new_len).unwrap_or(0), 0);
            let mut n = n64(self.ext_booleans).wrapping_sub(1);
            let base = n64(v.len()).wrapping_sub(n64(ext_booleans));
            for m in (0..n64(ext_booleans)).rev() {
                let name = ix(m).and_then(|m| ext_names.get(m.saturating_add(from)));
                let value = if name.is_some_and(|nm| find(to1, to2, nm)) {
                    let src = ix(base.wrapping_add(n))
                        .and_then(|i| old.get(i))
                        .copied()
                        .unwrap_or(0);
                    n = n.wrapping_sub(1);
                    src
                } else {
                    0
                };
                if let Some(slot) = ix(base.wrapping_add(m)).and_then(|i| v.get_mut(i)) {
                    *slot = value;
                }
            }
            self.booleans = v;
            self.ext_booleans = ext_booleans;
        }

        if self.ext_numbers != ext_numbers {
            let to1 = to_booleans;
            let to2 = to_numbers.saturating_add(to1);
            let from = ext_booleans;
            let new_len = n64(self.numbers.len())
                .wrapping_add(n64(ext_numbers))
                .wrapping_sub(n64(self.ext_numbers))
                .max(0);
            let old = std::mem::take(&mut self.numbers);
            let mut v = old.clone();
            v.resize(ix(new_len).unwrap_or(0), ABSENT_NUMERIC);
            let mut n = n64(self.ext_numbers).wrapping_sub(1);
            let base = n64(v.len()).wrapping_sub(n64(ext_numbers));
            for m in (0..n64(ext_numbers)).rev() {
                let name = ix(m).and_then(|m| ext_names.get(m.saturating_add(from)));
                let value = if name.is_some_and(|nm| find(to1, to2, nm)) {
                    let src = ix(base.wrapping_add(n))
                        .and_then(|i| old.get(i))
                        .copied()
                        .unwrap_or(ABSENT_NUMERIC);
                    n = n.wrapping_sub(1);
                    src
                } else {
                    ABSENT_NUMERIC
                };
                if let Some(slot) = ix(base.wrapping_add(m)).and_then(|i| v.get_mut(i)) {
                    *slot = value;
                }
            }
            self.numbers = v;
            self.ext_numbers = ext_numbers;
        }

        if self.ext_strings != ext_strings {
            let to1 = to_booleans.saturating_add(to_numbers);
            let to2 = to_strings.saturating_add(to1);
            let from = ext_booleans.saturating_add(ext_numbers);
            let new_len = n64(self.strings.len())
                .wrapping_add(n64(ext_strings))
                .wrapping_sub(n64(self.ext_strings))
                .max(0);
            let old = std::mem::take(&mut self.strings);
            let mut v = old.clone();
            v.resize(ix(new_len).unwrap_or(0), Str::Absent);
            let mut n = n64(self.ext_strings).wrapping_sub(1);
            let base = n64(v.len()).wrapping_sub(n64(ext_strings));
            for m in (0..n64(ext_strings)).rev() {
                let name = ix(m).and_then(|m| ext_names.get(m.saturating_add(from)));
                let value = if name.is_some_and(|nm| find(to1, to2, nm)) {
                    let src = ix(base.wrapping_add(n))
                        .and_then(|i| old.get(i))
                        .cloned()
                        .unwrap_or_default();
                    n = n.wrapping_sub(1);
                    src
                } else {
                    Str::Absent
                };
                if let Some(slot) = ix(base.wrapping_add(m)).and_then(|i| v.get_mut(i)) {
                    *slot = value;
                }
            }
            self.strings = v;
            self.ext_strings = ext_strings;
        }
    }

    /// `_nc_align_termtype (to, from)`: both descriptions given the union of
    /// their extended capabilities, in order, so that index `i` is the same
    /// capability in both.
    pub fn align(&mut self, from: &mut Self) {
        let na = self.num_ext_names();
        let nb = from.num_ext_names();
        if na == 0 && nb == 0 {
            return;
        }
        if na == nb
            && self.ext_booleans == from.ext_booleans
            && self.ext_numbers == from.ext_numbers
            && self.ext_strings == from.ext_strings
            && (0..na).all(|n| self.ext_name(n) == from.ext_name(n))
        {
            return;
        }
        if self.ext_strings != 0 && from.ext_booleans.saturating_add(from.ext_numbers) != 0 {
            let snapshot = from.clone();
            self.adjust_cancels(&snapshot);
        }
        if from.ext_strings != 0 && self.ext_booleans.saturating_add(self.ext_numbers) != 0 {
            let snapshot = self.clone();
            from.adjust_cancels(&snapshot);
        }
        let mut ext_names: Vec<Option<Vec<u8>>> = Vec::new();
        let run = |t: &Self, lo: usize, len: usize| -> Vec<Option<Vec<u8>>> {
            (lo..lo.saturating_add(len))
                .map(|i| t.ext_names.get(i).cloned().flatten())
                .collect()
        };
        let ext_booleans = merge_names(
            &mut ext_names,
            &run(self, 0, self.ext_booleans),
            &run(from, 0, from.ext_booleans),
        );
        let ext_numbers = merge_names(
            &mut ext_names,
            &run(self, self.ext_booleans, self.ext_numbers),
            &run(from, from.ext_booleans, from.ext_numbers),
        );
        let ext_strings = merge_names(
            &mut ext_names,
            &run(
                self,
                self.ext_booleans.saturating_add(self.ext_numbers),
                self.ext_strings,
            ),
            &run(
                from,
                from.ext_booleans.saturating_add(from.ext_numbers),
                from.ext_strings,
            ),
        );
        let total = ext_booleans
            .saturating_add(ext_numbers)
            .saturating_add(ext_strings);
        // `na` and `nb` are the counts from before `adjust_cancels`, which
        // can move a name between runs but never add or drop one.
        if na != total {
            self.realign_data(&ext_names, ext_booleans, ext_numbers, ext_strings);
            self.ext_names = ext_names.clone();
        }
        if nb != total {
            from.realign_data(&ext_names, ext_booleans, ext_numbers, ext_strings);
            from.ext_names = ext_names;
        }
    }
}

/// `_nc_ins_ext_name`'s opening of a slot at `at`: everything from there up
/// one, the slot keeping a copy of the value it pushed up -- what upstream's
/// shift leaves there for its caller to set.
pub(crate) fn open<T: Clone + Default>(v: &mut Vec<T>, at: usize) {
    let at = at.min(v.len());
    let copy = v.get(at).cloned().unwrap_or_default();
    v.insert(at, copy);
}

/// `merge_names (dst, a, na, b, nb)`: two sorted runs of names merged onto
/// `dst`, a name in both once; how many.
fn merge_names(
    dst: &mut Vec<Option<Vec<u8>>>,
    a: &[Option<Vec<u8>>],
    b: &[Option<Vec<u8>>],
) -> usize {
    let key = |x: &Option<Vec<u8>>| x.clone().unwrap_or_default();
    let (mut i, mut j, mut n) = (0usize, 0usize, 0usize);
    while let (Some(x), Some(y)) = (a.get(i), b.get(j)) {
        match key(x).cmp(&key(y)) {
            std::cmp::Ordering::Less => {
                dst.push(x.clone());
                i = i.saturating_add(1);
            }
            std::cmp::Ordering::Greater => {
                dst.push(y.clone());
                j = j.saturating_add(1);
            }
            std::cmp::Ordering::Equal => {
                dst.push(x.clone());
                i = i.saturating_add(1);
                j = j.saturating_add(1);
            }
        }
        n = n.saturating_add(1);
    }
    for x in a.get(i..).unwrap_or_default() {
        dst.push(x.clone());
        n = n.saturating_add(1);
    }
    for y in b.get(j..).unwrap_or_default() {
        dst.push(y.clone());
        n = n.saturating_add(1);
    }
    n
}

// ---- read_entry.c: a compiled entry --------------------------------------

/// `MAGIC`: the classic format, numbers in 16 bits.
pub(crate) const MAGIC: u16 = 0o432;
/// `MAGIC2`: the extended-numbers format, numbers in 32 bits.
pub(crate) const MAGIC2: u16 = 0o1036;
/// `MAX_ENTRY_SIZE1`: the size limit of a classic entry.
const MAX_ENTRY_SIZE1: usize = 4096;

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
) -> Option<Vec<Str>> {
    let mut out = Vec::with_capacity(count);
    let size_i = i32::try_from(size).unwrap_or(i32::MAX);
    for i in 0..count {
        let nn = my_number(offsets, i.saturating_mul(2));
        // First the offset: absent (-1), cancelled (-2), past the table
        // (absent), in it, or corrupt -- below -2, or at the table's very
        // end.
        let start = match nn {
            -1 => Err(Str::Absent),
            -2 => Err(Str::Cancelled),
            _ if nn > size_i => Err(Str::Absent),
            _ if nn >= 0 && nn < size_i => Ok(as_count(nn)),
            _ => return None,
        };
        // Then the string: one with no NUL before the end is ignored; with
        // `always`, an empty one or none at all is corrupt.
        let value = match start {
            Ok(start) => {
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
                    None => Str::Absent,
                    Some(0) if always => return None,
                    Some(end) => Str::Value(rest.get(..end).unwrap_or_default().to_vec()),
                }
            }
            Err(_) if always => return None,
            Err(s) => s,
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

/// `_nc_read_termtype`: the entry in `buffer`, every value as the file has
/// it, or `None` -- `TGETENT_NO` -- for anything upstream rejects. Its
/// extended capabilities are read only when `user_definable`
/// (`_nc_user_definable`, which `tic` and `infocmp` clear without `-x`).
#[allow(
    clippy::too_many_lines,
    reason = "upstream's _nc_read_termtype, in one piece so it reads against it"
)]
#[must_use]
pub fn read_termtype(buffer: &[u8], user_definable: bool) -> Option<TermType> {
    let mut r = Reader { buffer, offset: 0 };
    let header = r.read(12);
    if header.len() != 12 {
        return None;
    }
    let magic = low_msb(header, 0);
    let (number_width, max_entry_size) = match magic {
        MAGIC2 => (4usize, crate::entry::MAX_ENTRY_SIZE),
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

    // The names: as much of the field as fits, a short read zero-filled --
    // and when the field is longer than that, the rest of it is read as
    // what follows, as upstream reads it.
    let want = crate::entry::MAX_NAME_SIZE.min(name_size);
    let raw = r.read(want);
    let term_names = raw
        .get(..raw.iter().position(|&b| b == 0).unwrap_or(raw.len()))
        .unwrap_or_default()
        .to_vec();

    let raw_bools = r.read(bool_count);
    if raw_bools.len() < bool_count {
        return None;
    }
    let mut booleans: Vec<i8> = raw_bools.iter().map(|&b| i8::from_le_bytes([b])).collect();
    r.even_boundary(name_size.saturating_add(bool_count));

    let raw_numbers = r.read(num_count.saturating_mul(number_width));
    if raw_numbers.len() != num_count.saturating_mul(number_width) {
        return None;
    }
    let mut numbers = convert_numbers(raw_numbers, num_count, number_width);

    let mut strings: Vec<Str> = Vec::new();
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
    booleans.resize(BOOLCOUNT, 0);
    numbers.resize(NUMCOUNT, ABSENT_NUMERIC);
    strings.resize(STRCOUNT, Str::Absent);

    let mut t = TermType {
        term_names,
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
    if !user_definable {
        return Some(t);
    }
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
            t.booleans.extend(b.iter().map(|&x| i8::from_le_bytes([x])));
        }
        r.even_boundary(ext_bools);

        if ext_nums != 0 {
            let b = r.read(ext_nums.saturating_mul(number_width));
            if b.len() != ext_nums.saturating_mul(number_width) {
                return None;
            }
            t.numbers.extend(convert_numbers(b, ext_nums, number_width));
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
            let tb = r.read(ext_limit);
            if tb.len() != ext_limit {
                return None;
            }
            tb
        } else {
            &[][..]
        };

        let mut base = 0usize;
        if ext_strs != 0 {
            let values = convert_strings(offsets, ext_strs, ext_limit, ext_table, false)?;
            for v in values.iter().filter_map(Str::valid) {
                base = base.saturating_add(v.len()).saturating_add(1);
            }
            t.strings.extend(values);
        }

        if need != 0 {
            if ext_strs >= max_entry_size / 2 {
                return None;
            }
            let name_offsets = offsets
                .get(ext_strs.saturating_mul(2)..)
                .unwrap_or_default();
            let name_table = ext_table.get(base..).unwrap_or_default();
            t.ext_names = convert_strings(name_offsets, need, ext_limit, name_table, true)?
                .into_iter()
                .map(|s| match s {
                    Str::Value(v) => Some(v),
                    Str::Absent | Str::Cancelled => None,
                })
                .collect();
        }
        t.ext_booleans = ext_bools;
        t.ext_numbers = ext_nums;
        t.ext_strings = ext_strs;
    }
    Some(t)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A description with the extended capabilities given, each with a
    /// value: booleans true, numbers their position, strings their name.
    fn with_ext(bools: &[&str], nums: &[&str], strs: &[&str]) -> TermType {
        let mut t = TermType::init();
        for b in bools {
            t.ext_names.push(Some(b.as_bytes().to_vec()));
            t.booleans.push(1);
        }
        for (i, n) in nums.iter().enumerate() {
            t.ext_names.push(Some(n.as_bytes().to_vec()));
            t.numbers.push(i32::try_from(i).unwrap());
        }
        for s in strs {
            t.ext_names.push(Some(s.as_bytes().to_vec()));
            t.strings.push(Str::Value(s.as_bytes().to_vec()));
        }
        t.ext_booleans = bools.len();
        t.ext_numbers = nums.len();
        t.ext_strings = strs.len();
        t
    }

    fn names(t: &TermType) -> Vec<String> {
        (0..t.num_ext_names())
            .map(|i| String::from_utf8(t.ext_name(i).to_vec()).unwrap())
            .collect()
    }

    #[test]
    fn aligning_gives_both_the_union_in_order() {
        let mut a = with_ext(&["AX"], &[], &["Ss", "kxIN"]);
        let mut b = with_ext(&["XT"], &["U8"], &["Ms"]);
        a.align(&mut b);
        assert_eq!(names(&a), ["AX", "XT", "U8", "Ms", "Ss", "kxIN"]);
        assert_eq!(names(&b), names(&a));
        // a keeps its own values; what it lacked is absent (false).
        assert_eq!(&a.booleans[BOOLCOUNT..], &[1, 0]);
        assert_eq!(&a.numbers[NUMCOUNT..], &[ABSENT_NUMERIC]);
        assert_eq!(
            &a.strings[STRCOUNT..],
            &[
                Str::Absent,
                Str::Value(b"Ss".to_vec()),
                Str::Value(b"kxIN".to_vec())
            ]
        );
        assert_eq!(&b.booleans[BOOLCOUNT..], &[0, 1]);
        assert_eq!(&b.numbers[NUMCOUNT..], &[0]);
        assert_eq!(
            &b.strings[STRCOUNT..],
            &[Str::Value(b"Ms".to_vec()), Str::Absent, Str::Absent]
        );
    }

    #[test]
    fn aligning_the_same_names_changes_nothing() {
        let mut a = with_ext(&["AX"], &[], &["Ss"]);
        let mut b = a.clone();
        b.booleans[BOOLCOUNT] = 0;
        let (a0, b0) = (a.clone(), b.clone());
        a.align(&mut b);
        assert_eq!((a, b), (a0, b0));
    }

    #[test]
    fn a_cancelled_string_becomes_the_others_boolean() {
        let mut a = with_ext(&[], &[], &["XT"]);
        a.strings[STRCOUNT] = Str::Cancelled;
        let mut b = with_ext(&["XT"], &[], &[]);
        a.align(&mut b);
        assert_eq!(names(&a), ["XT"]);
        assert_eq!(a.ext_booleans, 1);
        assert_eq!(a.booleans[BOOLCOUNT], 0);
        assert_eq!(a.strings.len(), STRCOUNT);
    }
}
