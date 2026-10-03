//! awk's arrays, built as gawk 5.2.1 builds them -- so that `for (k in a)`
//! visits the elements in gawk's order and hands out gawk's kind of index.
//!
//! ## Why a port, and not a hash map
//!
//! POSIX leaves the order of `for (k in a)` unspecified, and programs depend
//! on it anyway: a report that prints its totals with a `for`-in loop prints
//! them in whatever order the implementation keeps them. A Rust `HashMap` is
//! seeded at random, so the same program printed its lines in a different
//! order on every run -- the one answer no implementation gives. gawk's order
//! is deterministic, and it is the reference here, so the order is gawk's: its
//! data structures, ported, with the same hash functions, the same table
//! sizes and the same growth rules, because the order *is* those.
//!
//! The kind of index a loop hands out is the structure's too. gawk keeps
//! integer subscripts as C `long`s and gives them back as numbers that turn
//! into strings when asked (`Value::index`); it keeps string subscripts as the
//! value they arrived as (`Value::index_name`). See `value.rs` for what that
//! does to `for (k in a) if (k > max) max = k`.
//!
//! ## The three layouts (array.c, str_array.c, int_array.c, cint_array.c)
//!
//! An array has no layout until its first element arrives, and the first
//! subscript chooses one -- and an array emptied by `delete` forgets it:
//!
//! * a **cint** array, for a non-negative integer: power-of-two groups of
//!   integers (`[0, 2^NHAT)`, then `[2^n, 2^(n+1))` for each larger `n`), each a
//!   hashed array tree of fixed-size leaves, listed in ascending order;
//! * an **int** array, for a negative integer: a chained hash table on the
//!   integer, two to a bucket;
//! * a **str** array, for anything else: a chained hash table on the text.
//!
//! An int or cint array keeps the subscripts it cannot take -- text in either,
//! and in a cint array also integers too sparse to be worth a leaf -- in a
//! second array of its own (gawk's `xarray`), which a loop lists first. When
//! the integers are all deleted the second array takes the first one's place.
//!
//! gawk reads four tunables from the environment at start-up, and so does
//! this: `STR_CHAIN_MAX`, `INT_CHAIN_MAX`, `NHAT` and `AWK_HASH` (`gst` or
//! `fnv1a` for the other two string hashes).

use crate::value::{Str, Value};
use std::rc::Rc;
use std::sync::OnceLock;

/// The hash table sizes gawk grows through: primes, an order of magnitude a
/// step at first, then about double.
const SIZES: [usize; 21] = [
    13,
    127,
    1021,
    8191,
    16381,
    32749,
    65497,
    131_101,
    262_147,
    524_309,
    1_048_583,
    2_097_169,
    4_194_319,
    8_388_617,
    16_777_259,
    33_554_467,
    67_108_879,
    134_217_757,
    268_435_459,
    536_870_923,
    1_073_741_827,
];

/// gawk's `INT32_BIT`: a cint array has a slot for each power of two a 32-bit
/// integer can reach.
const INT32_BIT: usize = 32;

/// Which function hashes a string subscript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HashFn {
    /// gawk's default, `awk_hash`: sdbm, kept to 32 bits.
    Awk,
    /// `AWK_HASH=gst`: GNU Smalltalk's, scrambled.
    Gst,
    /// `AWK_HASH=fnv1a`.
    Fnv1a,
}

/// The tunables, as gawk's `array_init` reads them.
struct Tuning {
    str_chain_max: usize,
    int_chain_max: usize,
    nhat: usize,
    /// The waste a cint array tolerates before it sends integers to its second
    /// array: `2^(NHAT + 1)`.
    threshold: i64,
    hash: HashFn,
}

impl Tuning {
    fn from_env() -> Tuning {
        let mut t = Tuning {
            str_chain_max: 2,
            int_chain_max: 2,
            nhat: 10,
            threshold: 0,
            hash: HashFn::Awk,
        };
        if let Ok(n) = usize::try_from(getenv_long("STR_CHAIN_MAX"))
            && n > 0
        {
            t.str_chain_max = n;
        }
        if let Ok(n) = usize::try_from(getenv_long("INT_CHAIN_MAX"))
            && n > 0
        {
            t.int_chain_max = n;
        }
        let nhat = getenv_long("NHAT");
        if nhat > 1
            && nhat < 32
            && let Ok(n) = usize::try_from(nhat)
        {
            t.nhat = n;
        }
        // Not off the end of gawk's table of powers of two, which stops at 2^30.
        t.nhat = t.nhat.min(29);
        t.threshold = 1i64 << t.nhat.saturating_add(1);
        match std::env::var_os("AWK_HASH")
            .as_ref()
            .map(|v| v.as_encoded_bytes())
        {
            Some(b"gst") => t.hash = HashFn::Gst,
            Some(b"fnv1a") => t.hash = HashFn::Fnv1a,
            _ => {}
        }
        t
    }
}

fn tuning() -> &'static Tuning {
    static T: OnceLock<Tuning> = OnceLock::new();
    T.get_or_init(Tuning::from_env)
}

/// gawk's `getenv_long`: the leading digits of a variable as a number, or -1
/// if it is unset or does not start with one.
fn getenv_long(name: &str) -> i64 {
    let Some(v) = std::env::var_os(name) else {
        return -1;
    };
    let b = v.as_encoded_bytes();
    if !b.first().is_some_and(u8::is_ascii_digit) {
        return -1;
    }
    let mut n: i64 = 0;
    for &c in b.iter().take_while(|c| c.is_ascii_digit()) {
        // C's `long` would overflow here; wrapping is what it would do.
        n = n
            .wrapping_mul(10)
            .wrapping_add(i64::from(c.wrapping_sub(b'0')));
    }
    n
}

/// A string's hash code, before it is reduced to a bucket, by `f`.
fn hash_code(f: HashFn, s: &[u8]) -> u64 {
    match f {
        HashFn::Awk => u64::from(awk_hash(s)),
        HashFn::Gst => gst_hash(s),
        HashFn::Fnv1a => u64::from(fnv1a_hash(s)),
    }
}

/// gawk's `awk_hash`: Ozan Yigit's sdbm, `h = c + 65599 * h`, forced to 32
/// bits. Each byte is a C `char` -- signed on x86-64 -- so a byte above 0x7f
/// adds a negative number, as it does in gawk.
fn awk_hash(s: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in s {
        // `*s` as a signed char, widened: what the C adds.
        #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
        let c = i32::from(c as i8) as u32;
        let htmp = h << 6;
        h = c
            .wrapping_add(htmp)
            .wrapping_add(htmp << 10)
            .wrapping_sub(h);
    }
    h
}

/// gawk's `gst_hash_string` (GNU Smalltalk's), with its `scramble`, on a
/// 64-bit `unsigned long`.
fn gst_hash(s: &[u8]) -> u64 {
    let mut h: u64 = 1_497_032_417;
    for &c in s {
        // A signed char again, sign-extended to 64 bits.
        #[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
        let c = i64::from(c as i8) as u64;
        h = h.wrapping_add(c);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    scramble(h)
}

/// gawk's `scramble`, 64-bit branch. The shifts do not add up to 64: these
/// are not rotations, and are not meant to be.
fn scramble(mut x: u64) -> u64 {
    x ^= (!x) >> 31;
    x = x.wrapping_add((x << 21) | (x >> 11));
    x = x.wrapping_add((x << 5) | (x >> 27));
    x = x.wrapping_add((x << 27) | (x >> 5));
    x.wrapping_add(x << 31)
}

/// FNV-1a over 32 bits, the bytes unsigned.
fn fnv1a_hash(s: &[u8]) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for &c in s {
        h ^= u32::from(c);
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// gawk's `int_hash`: the final mix of Paul Hsieh's SuperFastHash, on the
/// integer cut to 32 bits.
fn int_hash(k: i64, hsize: usize) -> usize {
    // `(uint32_t) k`, as the C's parameter conversion does.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mut k = k as u32;
    k ^= k << 3;
    k = k.wrapping_add(k >> 5);
    k ^= k << 4;
    k = k.wrapping_add(k >> 17);
    k ^= k << 25;
    k = k.wrapping_add(k >> 6);
    let k = usize::try_from(k).unwrap_or(usize::MAX);
    k.checked_rem(hsize).unwrap_or(0)
}

/// The next table size after `old`, or `None` at the largest.
fn grown(old: usize) -> Option<usize> {
    SIZES.iter().copied().find(|&s| old < s)
}

// ---- the array --------------------------------------------------------------

/// An awk array.
#[derive(Debug, Default)]
pub struct Array {
    kind: Kind,
}

#[derive(Debug, Default)]
enum Kind {
    /// No element yet, so no layout yet.
    #[default]
    Null,
    Cint(Cint),
    Int(Ints),
    Str(Strs),
}

/// What a removal left behind, for the array holding the layout to act on.
enum Removed {
    No,
    Yes,
    /// The layout is empty: the array forgets it.
    Emptied,
    /// Only the second array is left: it takes the first one's place.
    Promote,
}

impl Array {
    #[must_use]
    pub fn new() -> Array {
        Array::default()
    }

    /// An array laid out for strings from the start, whatever its first
    /// subscript: how gawk makes `ENVIRON`, so that a variable named `0`
    /// does not make it an integer array.
    #[must_use]
    pub fn new_str() -> Array {
        Array {
            kind: Kind::Str(Strs::default()),
        }
    }

    /// How many elements: gawk's `assoc_length`.
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.kind {
            Kind::Null => 0,
            Kind::Cint(c) => c.table_size,
            Kind::Int(i) => i.table_size,
            Kind::Str(s) => s.table_size,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The element `subs` names, if there is one: gawk's `in_array`, which
    /// does not look at the subscript at all when the array is empty.
    #[must_use]
    pub fn get(&self, subs: &Value, convfmt: &[u8]) -> Option<&Value> {
        if self.is_empty() {
            return None;
        }
        self.exists(subs, convfmt)
    }

    /// The layout's own existence test (`aexists`).
    fn exists(&self, subs: &Value, convfmt: &[u8]) -> Option<&Value> {
        match &self.kind {
            Kind::Null => None,
            Kind::Cint(c) => c.exists(subs, convfmt),
            Kind::Int(i) => i.exists(subs, convfmt),
            Kind::Str(s) => s.exists(subs, convfmt),
        }
    }

    /// The element `subs` names, made (unset) if it is not there: gawk's
    /// `assoc_lookup`. The first element of an empty array chooses its layout.
    pub fn lookup(&mut self, subs: &Value, convfmt: &[u8]) -> &mut Value {
        if matches!(self.kind, Kind::Null) {
            // `null_lookup`: cint if it will take the subscript, then int,
            // then str.
            self.kind = match subs.is_integer() {
                Some(k) if k >= 0 => Kind::Cint(Cint::default()),
                Some(_) => Kind::Int(Ints::default()),
                None => Kind::Str(Strs::default()),
            };
        }
        match &mut self.kind {
            Kind::Cint(c) => c.lookup(subs, convfmt),
            Kind::Int(i) => i.lookup(subs, convfmt),
            Kind::Str(s) => s.lookup(subs, convfmt),
            // Unreachable: a layout was chosen just above.
            Kind::Null => unreachable_slot(),
        }
    }

    /// Remove the element `subs` names, reporting whether there was one:
    /// gawk's `assoc_remove`, and then -- as `do_delete` does -- an array left
    /// empty forgets its layout.
    pub fn remove(&mut self, subs: &Value, convfmt: &[u8]) -> bool {
        let removed = self.remove_layout(subs, convfmt);
        if removed && self.is_empty() {
            self.kind = Kind::Null;
        }
        removed
    }

    fn remove_layout(&mut self, subs: &Value, convfmt: &[u8]) -> bool {
        let outcome = match &mut self.kind {
            Kind::Null => Removed::No,
            Kind::Cint(c) => c.remove(subs, convfmt),
            Kind::Int(i) => i.remove(subs, convfmt),
            Kind::Str(s) => s.remove(subs, convfmt),
        };
        match outcome {
            Removed::No => false,
            Removed::Yes => true,
            Removed::Emptied => {
                self.kind = Kind::Null;
                true
            }
            Removed::Promote => {
                let x = match &mut self.kind {
                    Kind::Cint(c) => c.xarray.take(),
                    Kind::Int(i) => i.xarray.take(),
                    Kind::Null | Kind::Str(_) => None,
                };
                self.kind = x.map_or(Kind::Null, |x| x.kind);
                true
            }
        }
    }

    /// Remove every element: `delete a`, gawk's `assoc_clear`. The array
    /// forgets its layout.
    pub fn clear(&mut self) {
        self.kind = Kind::Null;
    }

    /// The indices, in the order `for (k in a)` visits them: gawk's
    /// `assoc_list` with `@unsorted`.
    #[must_use]
    pub fn indices(&self) -> Vec<Value> {
        let mut out = Vec::with_capacity(self.len());
        self.list(&mut out, false);
        out
    }

    /// The index `for (k in a)` would visit first: what gawk's
    /// `for (k in a) delete a[k]` leaves in `k`.
    #[must_use]
    pub fn first_index(&self) -> Option<Value> {
        let mut out = Vec::with_capacity(1);
        self.list(&mut out, true);
        out.into_iter().next()
    }

    fn list(&self, out: &mut Vec<Value>, first_only: bool) {
        match &self.kind {
            Kind::Null => {}
            Kind::Cint(c) => c.list(out, first_only),
            Kind::Int(i) => i.list(out, first_only),
            Kind::Str(s) => s.list(out, first_only),
        }
    }
}

// ---- str_array.c --------------------------------------------------------------

/// A string-indexed hash table. Each chain is kept with its head *last*: gawk
/// inserts at a chain's head and walks from it.
#[derive(Debug, Default)]
struct Strs {
    buckets: Vec<Vec<StrBucket>>,
    table_size: usize,
    /// At the largest size; it does not grow again.
    maxed: bool,
}

#[derive(Debug)]
struct StrBucket {
    /// The index as a loop hands it out.
    name: Value,
    text: Rc<Str>,
    code: u64,
    value: Value,
}

impl Strs {
    /// The bucket for `code`, in a table of the current size.
    fn bucket(&self, code: u64) -> usize {
        let size = u64::try_from(self.buckets.len()).unwrap_or(u64::MAX);
        usize::try_from(code.checked_rem(size).unwrap_or(0)).unwrap_or(0)
    }

    /// Where `text` is, if it is here: its bucket and its place in the chain.
    fn find(&self, text: &[u8], code: u64) -> Option<(usize, usize)> {
        let h = self.bucket(code);
        let chain = self.buckets.get(h)?;
        chain
            .iter()
            .rposition(|b| b.code == code && b.text.as_slice() == text)
            .map(|at| (h, at))
    }

    fn exists(&self, subs: &Value, convfmt: &[u8]) -> Option<&Value> {
        if self.table_size == 0 {
            return None;
        }
        let text = subs.to_str(convfmt);
        let code = hash_code(tuning().hash, &text);
        let (h, at) = self.find(&text, code)?;
        self.buckets.get(h)?.get(at).map(|b| &b.value)
    }

    fn lookup(&mut self, subs: &Value, convfmt: &[u8]) -> &mut Value {
        let text = subs.to_str(convfmt);
        if self.buckets.is_empty() {
            self.grow();
        }
        let t = tuning();
        let code = hash_code(t.hash, &text);
        let (h, at) = if let Some(found) = self.find(&text, code) {
            found
        } else {
            self.table_size = self.table_size.saturating_add(1);
            // Grow before installing, if the chains have got too long.
            let per_bucket = self.table_size.checked_div(self.buckets.len()).unwrap_or(0);
            if !self.maxed && per_bucket > t.str_chain_max {
                self.grow();
            }
            let h = self.bucket(code);
            let name = subs.index_name(&text);
            if self.buckets.get(h).is_none() {
                // Unreachable: `bucket` is always in the table.
                self.buckets.resize_with(h.saturating_add(1), Vec::new);
            }
            let chain = self.buckets.get_mut(h).map_or(0, |chain| {
                chain.push(StrBucket {
                    name,
                    text,
                    code,
                    value: Value::Uninit,
                });
                chain.len().saturating_sub(1)
            });
            (h, chain)
        };
        self.slot(h, at)
    }

    fn slot(&mut self, h: usize, at: usize) -> &mut Value {
        // `h` and `at` come from `find` or the push just made.
        let chain = self.buckets.get_mut(h);
        match chain.and_then(|c| c.get_mut(at)) {
            Some(b) => &mut b.value,
            None => unreachable_slot(),
        }
    }

    fn remove(&mut self, subs: &Value, convfmt: &[u8]) -> Removed {
        if self.table_size == 0 {
            return Removed::No;
        }
        let text = subs.to_str(convfmt);
        let code = hash_code(tuning().hash, &text);
        let Some((h, at)) = self.find(&text, code) else {
            return Removed::No;
        };
        if let Some(chain) = self.buckets.get_mut(h) {
            chain.remove(at);
        }
        self.table_size = self.table_size.saturating_sub(1);
        if self.table_size == 0 {
            Removed::Emptied
        } else {
            Removed::Yes
        }
    }

    /// gawk's `grow_table`: on to the next size, rehashing every element by
    /// walking the old chains from their heads and pushing each onto the head
    /// of its new one.
    fn grow(&mut self) {
        let old_size = self.buckets.len();
        let Some(new_size) = grown(old_size) else {
            self.maxed = true;
            return;
        };
        let old = std::mem::take(&mut self.buckets);
        self.buckets.resize_with(new_size, Vec::new);
        for chain in old {
            for b in chain.into_iter().rev() {
                let h = self.bucket(b.code);
                if let Some(c) = self.buckets.get_mut(h) {
                    c.push(b);
                }
            }
        }
    }

    fn list(&self, out: &mut Vec<Value>, first_only: bool) {
        for chain in &self.buckets {
            for b in chain.iter().rev() {
                out.push(b.name.clone());
                if first_only {
                    return;
                }
            }
        }
    }
}

// ---- int_array.c ----------------------------------------------------------------

/// An integer-indexed hash table, two elements to a bucket, with the
/// subscripts that are not integers in a string array of its own.
#[derive(Debug, Default)]
struct Ints {
    /// Chains with their heads last, as in [`Strs`]. Only a chain's head
    /// bucket can be part-full.
    buckets: Vec<Vec<IntBucket>>,
    /// Every element, the second array's included.
    table_size: usize,
    maxed: bool,
    xarray: Option<Box<Array>>,
}

#[derive(Debug, Default)]
struct IntBucket {
    nums: [i64; 2],
    vals: [Value; 2],
    count: usize,
}

impl Ints {
    fn xlen(&self) -> usize {
        self.xarray.as_ref().map_or(0, |x| x.len())
    }

    fn find(&self, k: i64) -> Option<(usize, usize, usize)> {
        let h = int_hash(k, self.buckets.len());
        let chain = self.buckets.get(h)?;
        for (at, b) in chain.iter().enumerate().rev() {
            for i in 0..b.count.min(2) {
                if b.nums.get(i) == Some(&k) {
                    return Some((h, at, i));
                }
            }
        }
        None
    }

    fn exists(&self, subs: &Value, convfmt: &[u8]) -> Option<&Value> {
        let Some(k) = subs.is_integer() else {
            return self.xarray.as_ref()?.exists(subs, convfmt);
        };
        if self.buckets.is_empty() {
            return None;
        }
        let (h, at, i) = self.find(k)?;
        self.buckets.get(h)?.get(at)?.vals.get(i)
    }

    fn lookup(&mut self, subs: &Value, convfmt: &[u8]) -> &mut Value {
        let Some(k) = subs.is_integer() else {
            // Not an integer: the second array, made on first need.
            let xn = self.xarray.get_or_insert_with(Box::default);
            if xn.exists(subs, convfmt).is_none() {
                self.table_size = self.table_size.saturating_add(1);
            }
            return xn.lookup(subs, convfmt);
        };
        if self.buckets.is_empty() {
            self.grow();
        }
        let (h, at, i) = if let Some(found) = self.find(k) {
            found
        } else {
            self.table_size = self.table_size.saturating_add(1);
            let ints = self.table_size.saturating_sub(self.xlen());
            let per_bucket = ints.checked_div(self.buckets.len()).unwrap_or(0);
            if !self.maxed && per_bucket > tuning().int_chain_max {
                self.grow();
            }
            let h = int_hash(k, self.buckets.len());
            let (at, i) = self.insert(k, h, Value::Uninit);
            (h, at, i)
        };
        match self
            .buckets
            .get_mut(h)
            .and_then(|c| c.get_mut(at))
            .and_then(|b| b.vals.get_mut(i))
        {
            Some(v) => v,
            None => unreachable_slot(),
        }
    }

    /// gawk's `int_insert`: into the head bucket if it has room, else into a
    /// new head.
    fn insert(&mut self, k: i64, h: usize, v: Value) -> (usize, usize) {
        let Some(chain) = self.buckets.get_mut(h) else {
            return (0, 0);
        };
        if chain.last().is_none_or(|b| b.count >= 2) {
            chain.push(IntBucket::default());
        }
        let at = chain.len().saturating_sub(1);
        let Some(b) = chain.last_mut() else {
            return (at, 0);
        };
        let i = b.count;
        if let (Some(n), Some(slot)) = (b.nums.get_mut(i), b.vals.get_mut(i)) {
            *n = k;
            *slot = v;
        }
        b.count = b.count.saturating_add(1);
        (at, i)
    }

    /// gawk's `int_remove`, including how it keeps every bucket but a
    /// chain's head full: a hole left in an inner bucket is filled from the
    /// head.
    fn remove(&mut self, subs: &Value, convfmt: &[u8]) -> Removed {
        if self.table_size == 0 || self.buckets.is_empty() {
            return Removed::No;
        }
        let Some(k) = subs.is_integer() else {
            let Some(xn) = self.xarray.as_mut() else {
                return Removed::No;
            };
            if !xn.remove_layout(subs, convfmt) {
                return Removed::No;
            }
            if xn.is_empty() {
                self.xarray = None;
            }
            self.table_size = self.table_size.saturating_sub(1);
            return Removed::Yes;
        };
        let Some((h, at, i)) = self.find(k) else {
            return Removed::No;
        };
        let Some(chain) = self.buckets.get_mut(h) else {
            return Removed::No;
        };
        let head = chain.len().saturating_sub(1);
        let Some(b) = chain.get_mut(at) else {
            return Removed::No;
        };
        if i == 0 && b.count == 2 {
            // Removing the first of two: the second moves down.
            b.nums[0] = b.nums[1];
            b.vals.swap(0, 1);
        }
        b.count = b.count.saturating_sub(1);
        if let Some(gone) = b.vals.get_mut(b.count) {
            *gone = Value::Uninit;
        }
        if b.count == 0 {
            chain.remove(at);
        } else if at != head {
            // An inner bucket is never part-full: take the head's last.
            let (num, val) = match chain.last_mut() {
                Some(hb) => {
                    hb.count = hb.count.saturating_sub(1);
                    let j = hb.count.min(1);
                    let num = hb.nums.get(j).copied().unwrap_or(0);
                    let val = hb.vals.get_mut(j).map(std::mem::take).unwrap_or_default();
                    (num, val)
                }
                None => (0, Value::Uninit),
            };
            if let Some(b) = chain.get_mut(at) {
                b.nums[1] = num;
                b.vals[1] = val;
                b.count = 2;
            }
            if chain.last().is_some_and(|hb| hb.count == 0) {
                chain.pop();
            }
        }
        self.table_size = self.table_size.saturating_sub(1);
        match &self.xarray {
            None if self.table_size == 0 => Removed::Emptied,
            Some(xn) if self.table_size == xn.len() => Removed::Promote,
            _ => Removed::Yes,
        }
    }

    /// gawk's `grow_int_table`: every element re-inserted, the old chains
    /// walked from their heads.
    fn grow(&mut self) {
        let old_size = self.buckets.len();
        let Some(new_size) = grown(old_size) else {
            self.maxed = true;
            return;
        };
        let old = std::mem::take(&mut self.buckets);
        self.buckets.resize_with(new_size, Vec::new);
        for chain in old {
            for b in chain.into_iter().rev() {
                let IntBucket { nums, vals, count } = b;
                for (num, val) in nums.into_iter().zip(vals).take(count) {
                    let h = int_hash(num, new_size);
                    self.insert(num, h, val);
                }
            }
        }
    }

    fn list(&self, out: &mut Vec<Value>, first_only: bool) {
        if let Some(xn) = &self.xarray {
            xn.list(out, first_only);
            if first_only || out.len() >= self.table_size {
                return;
            }
        }
        for chain in &self.buckets {
            for b in chain.iter().rev() {
                for &num in b.nums.iter().take(b.count) {
                    out.push(Value::index(num));
                    if first_only {
                        return;
                    }
                }
            }
        }
    }
}

// ---- cint_array.c ----------------------------------------------------------------

/// A non-negative-integer array: one hashed array tree per power of two.
#[derive(Debug, Default)]
struct Cint {
    /// `INT32_BIT` slots once the first element arrives; the ones below
    /// `NHAT` are never used. Empty until then.
    nodes: Vec<Option<Hat>>,
    /// Every element, the second array's included.
    table_size: usize,
    /// The slots the leaves hold, used or not: gawk's `array_capacity`.
    capacity: i64,
    xarray: Option<Box<Array>>,
}

/// A hashed array tree: a top array of `kids.len()` slots each covering
/// `size` integers from `base` on (gawk's `Node_array_tree`; half the slots,
/// when the range is an odd power of two, is its `HALFHAT`).
#[derive(Debug, Default)]
struct Hat {
    base: i64,
    size: i64,
    count: usize,
    kids: Vec<Option<Kid>>,
}

#[derive(Debug)]
enum Kid {
    Tree(Box<Hat>),
    Leaf(Leaf),
}

/// A leaf: `size` consecutive integers from `base` (`Node_array_leaf`).
#[derive(Debug, Default)]
struct Leaf {
    base: i64,
    size: i64,
    count: usize,
    vals: Vec<Option<Value>>,
}

/// Which power-of-two slot `k` belongs to: `1 + floor(log2 k)`, with every
/// `k` below `2^NHAT` in slot `NHAT`.
fn cint_hash(k: i64, nhat: usize) -> usize {
    // `(uint32_t) k`; a cint subscript is below 2^31, so nothing is lost.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let num = k as u32;
    if num == 0 {
        return nhat;
    }
    let r = usize::try_from(31u32.saturating_sub(num.leading_zeros())).unwrap_or(0);
    if r < nhat { nhat } else { r.saturating_add(1) }
}

fn pow2(n: usize) -> i64 {
    1i64.checked_shl(u32::try_from(n).unwrap_or(63))
        .unwrap_or(i64::MAX)
}

/// The slot of `kids` that covers `k`.
fn kid_index(k: i64, base: i64, size: i64) -> usize {
    usize::try_from(k.saturating_sub(base).checked_div(size).unwrap_or(0)).unwrap_or(0)
}

impl Cint {
    fn xlen(&self) -> usize {
        self.xarray.as_ref().map_or(0, |x| x.len())
    }

    /// gawk's `ISUINT`: a non-negative integer subscript.
    fn uint(subs: &Value) -> Option<i64> {
        subs.is_integer().filter(|k| *k >= 0)
    }

    fn find(&self, k: i64) -> Option<&Value> {
        let h1 = cint_hash(k, tuning().nhat);
        tree_exists(self.nodes.get(h1)?.as_ref()?, k)
    }

    fn exists(&self, subs: &Value, convfmt: &[u8]) -> Option<&Value> {
        if let Some(k) = Cint::uint(subs)
            && let Some(v) = self.find(k)
        {
            return Some(v);
        }
        self.xarray.as_ref()?.exists(subs, convfmt)
    }

    fn lookup(&mut self, subs: &Value, convfmt: &[u8]) -> &mut Value {
        let t = tuning();
        let k = Cint::uint(subs);
        if let Some(k) = k
            && self.find(k).is_some()
        {
            return self.hat_lookup(k);
        }
        let in_second = self
            .xarray
            .as_ref()
            .is_some_and(|xn| xn.exists(subs, convfmt).is_some());
        if in_second {
            return match self.xarray.as_mut() {
                Some(xn) => xn.lookup(subs, convfmt),
                None => unreachable_slot(),
            };
        }
        if let Some(k) = k {
            let h1 = cint_hash(k, t.nhat);
            // The capacity this would take, at a guess: what is held now plus
            // a leaf of the size this power of two's leaves usually are. Too
            // much waste, and the integer goes to the second array instead.
            let m = h1.saturating_sub(1);
            let mut li = m.max(t.nhat);
            while li >= t.nhat {
                li = li.saturating_add(1) / 2;
            }
            let capacity = self.capacity.saturating_add(pow2(li));
            let held =
                i64::try_from(self.table_size.saturating_sub(self.xlen())).unwrap_or(i64::MAX);
            if capacity.saturating_sub(held) <= t.threshold {
                if self.nodes.is_empty() {
                    self.capacity = 0;
                    self.nodes.resize_with(INT32_BIT, || None);
                }
                self.table_size = self.table_size.saturating_add(1);
                return self.hat_lookup(k);
            }
        }
        // `xinstall`: the second array, laid out by the subscript that
        // made it.
        self.table_size = self.table_size.saturating_add(1);
        let xn = self.xarray.get_or_insert_with(|| {
            Box::new(Array {
                kind: if subs.is_integer().is_some() {
                    Kind::Int(Ints::default())
                } else {
                    Kind::Str(Strs::default())
                },
            })
        });
        xn.lookup(subs, convfmt)
    }

    /// The element `k` in its tree, made if absent (the count of the array
    /// is the caller's to keep).
    fn hat_lookup(&mut self, k: i64) -> &mut Value {
        let nhat = tuning().nhat;
        let h1 = cint_hash(k, nhat);
        let Some(slot) = self.nodes.get_mut(h1) else {
            return unreachable_slot();
        };
        let tree = slot.get_or_insert_with(Hat::default);
        let m = h1.saturating_sub(1);
        if m < nhat {
            tree_lookup(&mut self.capacity, tree, k, nhat, 0, nhat)
        } else {
            tree_lookup(&mut self.capacity, tree, k, m, pow2(m), nhat)
        }
    }

    fn remove(&mut self, subs: &Value, convfmt: &[u8]) -> Removed {
        if self.table_size == 0 {
            return Removed::No;
        }
        if let Some(k) = Cint::uint(subs) {
            let h1 = cint_hash(k, tuning().nhat);
            let mut gone = false;
            if let Some(slot) = self.nodes.get_mut(h1)
                && let Some(tree) = slot.as_mut()
                && tree_remove(&mut self.capacity, tree, k)
            {
                gone = true;
                if tree.count == 0 {
                    *slot = None;
                }
            }
            if gone {
                self.table_size = self.table_size.saturating_sub(1);
                return match &self.xarray {
                    None if self.table_size == 0 => Removed::Emptied,
                    Some(xn) if self.table_size == xn.len() => Removed::Promote,
                    _ => Removed::Yes,
                };
            }
        }
        // `xremove`.
        let Some(xn) = self.xarray.as_mut() else {
            return Removed::No;
        };
        if !xn.remove_layout(subs, convfmt) {
            return Removed::No;
        }
        if xn.is_empty() {
            self.xarray = None;
        }
        self.table_size = self.table_size.saturating_sub(1);
        if self.table_size == 0 {
            Removed::Emptied
        } else {
            Removed::Yes
        }
    }

    fn list(&self, out: &mut Vec<Value>, first_only: bool) {
        if let Some(xn) = &self.xarray {
            xn.list(out, first_only);
            if first_only || out.len() >= self.table_size {
                return;
            }
        }
        for tree in self.nodes.iter().skip(tuning().nhat).flatten() {
            tree_list(tree, out, first_only);
            if first_only && !out.is_empty() {
                return;
            }
        }
    }
}

/// gawk's `tree_lookup`: find or make `k` in `tree`, which covers `2^m`
/// integers from `base`.
fn tree_lookup<'a>(
    capacity: &mut i64,
    tree: &'a mut Hat,
    k: i64,
    m: usize,
    base: i64,
    nhat: usize,
) -> &'a mut Value {
    // The top array and each slot are 2^n wide, n = ceil(m / 2); for an odd
    // m only the first half of the top array is needed.
    let n = m.saturating_add(1) / 2;
    if tree.count == 0 {
        let size = pow2(n);
        let mut actual = size;
        tree.base = base;
        tree.size = size;
        if n > m / 2 {
            actual /= 2;
        }
        tree.kids.clear();
        tree.kids
            .resize_with(usize::try_from(actual).unwrap_or(0), || None);
    }
    let size = tree.size;
    let i = kid_index(k, tree.base, size);
    let present = match tree.kids.get(i) {
        Some(Some(Kid::Tree(t))) => tree_exists(t, k).is_some(),
        Some(Some(Kid::Leaf(l))) => leaf_exists(l, k).is_some(),
        _ => false,
    };
    if !present {
        tree.count = tree.count.saturating_add(1);
    }
    let base = tree
        .base
        .saturating_add(size.saturating_mul(i64::try_from(i).unwrap_or(0)));
    let Some(slot) = tree.kids.get_mut(i) else {
        return unreachable_slot();
    };
    if n > nhat {
        let kid = slot.get_or_insert_with(|| Kid::Tree(Box::default()));
        match kid {
            Kid::Tree(t) => tree_lookup(capacity, t, k, n, base, nhat),
            Kid::Leaf(l) => leaf_lookup(capacity, l, k, size, base),
        }
    } else {
        let kid = slot.get_or_insert_with(|| Kid::Leaf(Leaf::default()));
        match kid {
            Kid::Leaf(l) => leaf_lookup(capacity, l, k, size, base),
            Kid::Tree(t) => tree_lookup(capacity, t, k, n, base, nhat),
        }
    }
}

fn tree_exists(tree: &Hat, k: i64) -> Option<&Value> {
    match tree
        .kids
        .get(kid_index(k, tree.base, tree.size))?
        .as_ref()?
    {
        Kid::Tree(t) => tree_exists(t, k),
        Kid::Leaf(l) => leaf_exists(l, k),
    }
}

/// gawk's `tree_remove`; a tree left empty is reset, for its parent to drop.
fn tree_remove(capacity: &mut i64, tree: &mut Hat, k: i64) -> bool {
    let i = kid_index(k, tree.base, tree.size);
    let Some(slot) = tree.kids.get_mut(i) else {
        return false;
    };
    let (removed, empty) = match slot.as_mut() {
        None => return false,
        Some(Kid::Tree(t)) => (tree_remove(capacity, t, k), t.count == 0),
        Some(Kid::Leaf(l)) => (leaf_remove(capacity, l, k), l.count == 0),
    };
    if !removed {
        return false;
    }
    if empty {
        *slot = None;
    }
    tree.count = tree.count.saturating_sub(1);
    if tree.count == 0 {
        *tree = Hat::default();
    }
    true
}

fn tree_list(tree: &Hat, out: &mut Vec<Value>, first_only: bool) {
    for kid in tree.kids.iter().flatten() {
        match kid {
            Kid::Tree(t) => tree_list(t, out, first_only),
            Kid::Leaf(l) => leaf_list(l, out, first_only),
        }
        if first_only && !out.is_empty() {
            return;
        }
    }
}

/// gawk's `leaf_lookup`: the leaf's array is made on first use, and what it
/// holds is counted in the array's capacity.
fn leaf_lookup<'a>(
    capacity: &mut i64,
    leaf: &'a mut Leaf,
    k: i64,
    size: i64,
    base: i64,
) -> &'a mut Value {
    if leaf.vals.is_empty() {
        leaf.count = 0;
        leaf.size = size;
        leaf.base = base;
        leaf.vals
            .resize_with(usize::try_from(size).unwrap_or(0), || None);
        *capacity = capacity.saturating_add(size);
    }
    let i = usize::try_from(k.saturating_sub(leaf.base)).unwrap_or(usize::MAX);
    let Some(slot) = leaf.vals.get_mut(i) else {
        return unreachable_slot();
    };
    if slot.is_none() {
        leaf.count = leaf.count.saturating_add(1);
    }
    slot.get_or_insert(Value::Uninit)
}

fn leaf_exists(leaf: &Leaf, k: i64) -> Option<&Value> {
    let i = usize::try_from(k.saturating_sub(leaf.base)).ok()?;
    leaf.vals.get(i)?.as_ref()
}

/// gawk's `leaf_remove`: a leaf left empty gives its slots back.
fn leaf_remove(capacity: &mut i64, leaf: &mut Leaf, k: i64) -> bool {
    let Ok(i) = usize::try_from(k.saturating_sub(leaf.base)) else {
        return false;
    };
    let Some(slot) = leaf.vals.get_mut(i) else {
        return false;
    };
    if slot.take().is_none() {
        return false;
    }
    leaf.count = leaf.count.saturating_sub(1);
    if leaf.count == 0 {
        leaf.vals = Vec::new();
        *capacity = capacity.saturating_sub(leaf.size);
        leaf.size = 0;
    }
    true
}

fn leaf_list(leaf: &Leaf, out: &mut Vec<Value>, first_only: bool) {
    for (i, v) in leaf.vals.iter().enumerate() {
        if v.is_some() {
            let n = leaf.base.saturating_add(i64::try_from(i).unwrap_or(0));
            out.push(Value::index(n));
            if first_only {
                return;
            }
        }
    }
}

/// A slot the code above has just found or made and then could not reach.
/// Every path here is guarded; were one ever missed, the element read or
/// written would be a scratch one rather than the process dying in the middle
/// of a user's run -- a wrong answer that a test would show, where a panic
/// would lose everything.
fn unreachable_slot<'a>() -> &'a mut Value {
    Box::leak(Box::new(Value::Uninit))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    const FMT: &[u8] = b"%.6g";

    fn s(x: &str) -> Value {
        Value::str(x.as_bytes().to_vec())
    }

    fn n(x: f64) -> Value {
        Value::Num(x)
    }

    fn order(a: &Array) -> String {
        a.indices()
            .iter()
            .map(|v| String::from_utf8(v.to_str(FMT).as_ref().clone()).unwrap())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn with(subs: &[Value]) -> Array {
        let mut a = Array::new();
        for k in subs {
            let _ = a.lookup(k, FMT);
        }
        a
    }

    /// The orders gawk 5.2.1 `--posix` printed for these, `for (k in a)
    /// printf "%s ", k` (`target/drafts/forin-probe5.sh`).
    #[test]
    fn orders_are_gawks() {
        let a = with(&[n(5.0), n(1_000_000.0), n(3.0), n(70000.0)]);
        assert_eq!(order(&a), "3 5 70000 1000000");
        let a = with(&[n(-1.0), n(-5.0), n(-3.0), n(2.0)]);
        assert_eq!(order(&a), "-1 -3 -5 2");
        let a = with(&[s("b"), s("a"), n(1.0), s("c"), n(0.0)]);
        assert_eq!(order(&a), "a b c 0 1");
        let a = with(&[n(1.5), n(0.5), n(2.0)]);
        assert_eq!(order(&a), "0.5 1.5 2");
        let a = with(&[s("the"), s("cat"), s("dog"), s("a")]);
        assert_eq!(order(&a), "the a cat dog");

        let keys: Vec<Value> = (0..50).map(|i| s(&format!("k{i}"))).collect();
        assert_eq!(
            order(&with(&keys)),
            "k20 k21 k22 k23 k24 k25 k26 k40 k27 k41 k28 k42 k29 k43 k44 k45 k46 k47 k48 \
             k49 k10 k0 k11 k12 k1 k13 k2 k14 k3 k4 k15 k5 k16 k30 k6 k17 k31 k7 k18 k32 k8 \
             k19 k9 k33 k34 k35 k36 k37 k38 k39"
        );
        let keys: Vec<Value> = (0..50).map(|i| n(f64::from(i * 7919 % 1000))).collect();
        assert_eq!(
            order(&with(&keys)),
            "0 3 28 31 56 84 109 112 137 165 190 193 218 246 271 274 299 327 352 355 380 \
             408 433 436 461 489 514 517 542 570 595 598 623 651 676 679 704 732 757 760 785 \
             813 838 841 866 894 919 922 947 975"
        );
        let keys: Vec<Value> = (0..50).map(|i| n(f64::from(-i * 37))).collect();
        assert_eq!(
            order(&with(&keys)),
            "-111 -148 -518 -666 -851 -1628 -592 -629 -74 -1184 -407 -1776 -1406 -444 \
             -1739 -1480 -185 -740 -1110 -555 -259 -1036 -962 -814 -1702 -1295 -222 -37 \
             -1554 -777 -1221 -888 -296 -333 -1332 -481 -1591 -1369 -1517 -1665 -999 -1258 \
             -1073 -1443 -370 -925 -1813 -703 -1147 0"
        );
    }

    #[test]
    fn the_first_subscript_chooses_the_layout_and_an_emptied_array_forgets_it() {
        let mut a = with(&[n(1.0)]);
        assert!(matches!(a.kind, Kind::Cint(_)));
        assert!(a.remove(&n(1.0), FMT));
        assert!(matches!(a.kind, Kind::Null));
        let _ = a.lookup(&s("x"), FMT);
        assert!(matches!(a.kind, Kind::Str(_)));
        a.clear();
        let _ = a.lookup(&n(-1.0), FMT);
        assert!(matches!(a.kind, Kind::Int(_)));
    }

    /// An integer array keeps text in a second array, which a loop lists
    /// first, and which takes over once the integers are gone.
    #[test]
    fn the_second_array_is_listed_first_and_takes_over() {
        let mut a = with(&[n(3.0), s("x"), n(1.0)]);
        assert_eq!(order(&a), "x 1 3");
        assert_eq!(a.first_index().unwrap().to_str(FMT).as_slice(), b"x");
        assert!(a.remove(&n(1.0), FMT));
        assert!(a.remove(&n(3.0), FMT));
        assert!(matches!(a.kind, Kind::Str(_)));
        assert_eq!(order(&a), "x");
        // Now a string array: a later integer is just text in it.
        let _ = a.lookup(&n(7.0), FMT);
        assert_eq!(a.len(), 2);
        assert!(a.get(&s("7"), FMT).is_some());
    }

    /// Elements are found by text in a string array and by value in an
    /// integer one, whatever form the subscript takes.
    #[test]
    fn subscripts_find_their_elements_in_every_layout() {
        let mut a = Array::new();
        *a.lookup(&n(10.0), FMT) = s("ten");
        assert_eq!(a.get(&s("10"), FMT).unwrap().to_str(FMT).as_slice(), b"ten");
        assert!(a.get(&s("010"), FMT).is_none());
        let mut b = Array::new();
        *b.lookup(&s("x"), FMT) = n(1.0);
        *b.lookup(&n(10.0), FMT) = s("ten");
        assert_eq!(b.get(&s("10"), FMT).unwrap().to_str(FMT).as_slice(), b"ten");
        // A non-integral number is its CONVFMT text.
        *b.lookup(&n(0.1), FMT) = s("tenth");
        assert!(b.get(&s("0.1"), FMT).is_some());
        assert_eq!(b.len(), 3);
    }

    /// Many elements, many removals: every layout keeps its count, finds
    /// what it holds and lists each once, through growth and refilling.
    #[test]
    fn churn_keeps_every_layout_consistent() {
        for (lo, step) in [(0i64, 1i64), (0, 997), (-5000, 3), (-1, -7)] {
            let mut a = Array::new();
            let keys: Vec<i64> = (0..3000).map(|i| lo + i * step).collect();
            for k in &keys {
                #[allow(clippy::cast_precision_loss)]
                let _ = a.lookup(&n(*k as f64), FMT);
            }
            let _ = a.lookup(&s("text"), FMT);
            assert_eq!(a.len(), keys.len() + 1);
            for k in keys.iter().step_by(2) {
                #[allow(clippy::cast_precision_loss)]
                let gone = a.remove(&n(*k as f64), FMT);
                assert!(gone, "{k}");
            }
            assert_eq!(a.len(), keys.len() / 2 + 1);
            let listed = a.indices();
            assert_eq!(listed.len(), a.len());
            for k in keys.iter().skip(1).step_by(2) {
                #[allow(clippy::cast_precision_loss)]
                let found = a.get(&n(*k as f64), FMT).is_some();
                assert!(found, "{k}");
            }
            let mut seen: Vec<Str> = listed
                .iter()
                .map(|v| v.to_str(FMT).as_ref().clone())
                .collect();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), a.len());
        }
        let mut a = Array::new();
        for i in 0..5000 {
            let _ = a.lookup(&s(&format!("key{i}")), FMT);
        }
        for i in (0..5000).step_by(3) {
            assert!(a.remove(&s(&format!("key{i}")), FMT));
        }
        assert_eq!(a.len(), 5000 - 1667);
        assert_eq!(a.indices().len(), a.len());
    }

    /// Integer subscripts too sparse for a leaf go to the second array.
    #[test]
    fn sparse_integers_overflow_into_the_second_array() {
        let mut a = Array::new();
        for i in 0..40 {
            let _ = a.lookup(&n(f64::from(i) * 100_000.0), FMT);
        }
        assert_eq!(a.len(), 40);
        let Kind::Cint(c) = &a.kind else {
            panic!("a cint array");
        };
        assert!(c.xarray.is_some());
        assert_eq!(a.indices().len(), 40);
    }

    #[test]
    fn hashes_are_gawks() {
        assert_eq!(awk_hash(b""), 0);
        assert_eq!(awk_hash(b"a"), 97);
        assert_eq!(awk_hash(b"ab"), 97u32.wrapping_mul(65599).wrapping_add(98));
        // A byte above 0x7f is a negative `char`.
        assert_eq!(awk_hash(&[0xff]), u32::MAX);
        assert_eq!(fnv1a_hash(b""), 2_166_136_261);
        assert_eq!(int_hash(0, 13), 0);
    }
}
