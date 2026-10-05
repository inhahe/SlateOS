//! awk's value model: a value is a number, a string, or — the thing that makes
//! awk awk — both at once.
//!
//! ## Why this is an enum and not an `f64` or a `Vec<u8>`
//!
//! `$1 == "10"` and `$1 == 10` are allowed to disagree, and in awk they do. A
//! field that looks like a number compares *numerically* against a number and
//! *textually* against a string, so `$1 == 10` is true of a line reading ` 10 `
//! and `$1 == "10"` is false of it. That is POSIX's **strnum** rule, and it is
//! the entire reason a value carries both readings rather than one.
//!
//! Getting it wrong is not a rounding error. `awk '$1 == "007"'` matching a
//! line whose first field is `7` is a wrong answer that looks like a right one.
//!
//! Only values that came from **input** are strnums: fields, `getline`'s
//! result, the elements `split` produces, `ARGV`, `ENVIRON`, and `-v`
//! assignments. A string *literal* in the program never is, because the program
//! said what it meant.
//!
//! ## Two kinds of value that change kind once — gawk's lazy typing
//!
//! gawk does not settle what a value is when it makes it. It keeps flags on
//! the value's node and settles them the first time something asks, *in
//! place*, on the one node every copy of the value shares (`x = $1` copies a
//! pointer). Two of those settlements are visible to a program, and this
//! models both, with the decided state in a cell that every copy shares:
//!
//! * **Input that looks numeric** ([`Input`]) starts out flagged a string;
//!   the first numeric use -- arithmetic, a comparison, a truth test, or use as
//!   an integer array subscript (`fixtype`, `force_number`, `is_integer`) --
//!   re-flags it a number. Nothing reads differently afterwards except where
//!   gawk looks at the flag itself: storing it as a string-array index keeps
//!   the node (a strnum) only while it is still flagged a string, and a
//!   comparison that finds no string flag on either side compares as C does
//!   (`NaN` unequal to everything) rather than as gawk sorts (`NaN` equal to
//!   `NaN`, above every number).
//!
//!   Which values are one node follows gawk too. A field is shared while an
//!   expression works on it -- `$1 + 0` settles the field itself, so a later
//!   `a[$1]` stores what gawk stores -- and with a function it is passed to;
//!   but storing it in a variable or an array element stores a copy (gawk's
//!   `UNFIELD`), as does passing `$0` to a function. Every other value is
//!   shared wherever it goes.
//! * **An index from `for (k in a)`** over an integer-indexed array
//!   ([`Index`], gawk's `INTIND`) starts out a number, and becomes a string --
//!   for good -- the first time anything asks for its text or its type: a
//!   comparison, a truth test, concatenation, `print`. So in
//!   `for (k in a) if (k > max) max = k` over `9`, `10` and `100`, the first
//!   comparison against an unset `max` makes `k` a string, `max` takes it, and
//!   every later comparison is between strings: gawk answers `9`, and so must
//!   this.
//!
//! ## Why strings are bytes
//!
//! awk is a filter. A line that is not UTF-8 has to come out the way it went
//! in, so a record is `Vec<u8>` and stays one; nothing here decodes. Where a
//! *character* count is required rather than a byte count — `length`, `substr`,
//! `index`, `RSTART`/`RLENGTH` — the character model in `ere::ch` is used, which
//! is the same one the regex engine indexes by, so the two cannot disagree
//! about where the third character starts.

use crate::ast::CmpOp;
use std::cell::{Cell, OnceCell};
use std::cmp::Ordering;
use std::rc::Rc;

/// A byte string. awk's only string type.
pub type Str = Vec<u8>;

/// One awk value.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// Never assigned: gawk's `Nnull_string`. Equal to both `""` and `0`, and
    /// false.
    #[default]
    Uninit,
    /// A number, and only a number.
    Num(f64),
    /// A string, and only a string — a program literal, or the result of a
    /// string operation.
    Str(Rc<Str>),
    /// A string *from input* that looks like a number, carrying both readings.
    StrNum(Rc<Input>),
    /// An index handed out by `for (k in a)` over an integer-indexed array.
    Index(Rc<Index>),
}

/// Input that looks like a number: gawk's `USER_INPUT` node.
#[derive(Debug)]
pub struct Input {
    text: Rc<Str>,
    num: f64,
    /// gawk has looked at it as a number, clearing the node's `STRING` flag.
    /// Shared by every copy, as gawk's node is; see the module docs.
    forced: Cell<bool>,
    /// Whether this is a field's own node, which a store copies.
    field: FieldNode,
}

/// Whether a value is a field's own node: gawk keeps those outside its
/// reference counting (no `MALLOC` flag), so storing one copies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldNode {
    /// Not a field: shared wherever it goes.
    No,
    /// `$1`, `$2`, ...: copied when stored.
    Field,
    /// `$0`: copied when stored, and when passed to a function.
    Record,
}

/// An integer index from `for (k in a)`: gawk's `INTIND` number.
#[derive(Debug)]
pub struct Index {
    num: i64,
    /// Its text, once anything has asked for it -- from which moment it is a
    /// string (gawk's `format_val` swaps `INTIND|NUMBER` for `STRING`).
    text: OnceCell<Rc<Str>>,
}

impl Value {
    /// A value built from input: a strnum if it looks like a number, else a
    /// plain string. This is the only place a `StrNum` is ever made.
    #[must_use]
    pub fn from_input(bytes: Str) -> Value {
        Value::input(bytes, FieldNode::No)
    }

    /// A field's value, as the record's split made it (`$0` when `node` is
    /// [`FieldNode::Record`]).
    #[must_use]
    pub fn from_field(bytes: Str, node: FieldNode) -> Value {
        Value::input(bytes, node)
    }

    fn input(bytes: Str, field: FieldNode) -> Value {
        match numeric_string(&bytes) {
            Some(n) => Value::StrNum(Rc::new(Input {
                text: Rc::new(bytes),
                num: n,
                forced: Cell::new(false),
                field,
            })),
            None => Value::Str(Rc::new(bytes)),
        }
    }

    /// The value as a variable or an array element stores it: a field's own
    /// node is copied -- settled as far as it is, and separate from then on
    /// (gawk's `UNFIELD`) -- and anything else is shared.
    #[must_use]
    pub fn unfield(self) -> Value {
        match &self {
            Value::StrNum(i) if i.field != FieldNode::No => Value::StrNum(Rc::new(Input {
                text: Rc::clone(&i.text),
                num: i.num,
                forced: Cell::new(i.forced.get()),
                field: FieldNode::No,
            })),
            _ => self,
        }
    }

    /// The value as a function's parameter receives it: shared, but `$0`
    /// copied (gawk's `setup_frame`: "$0 needs to be passed by value").
    #[must_use]
    pub fn for_call(self) -> Value {
        match &self {
            Value::StrNum(i) if i.field == FieldNode::Record => self.unfield(),
            _ => self,
        }
    }

    /// A command-line assignment's value -- `-v n=10`, or an `n=10` operand:
    /// input, already looked at as a number, because gawk's `arg_assign`
    /// calls `force_number` on it as it makes it.
    #[must_use]
    pub fn from_assignment(bytes: Str) -> Value {
        let v = Value::from_input(bytes);
        // Called for its effect on the value, which is the point.
        let _ = v.to_num();
        v
    }

    /// A program string literal, or any computed string. Never a strnum.
    #[must_use]
    pub fn str(bytes: Str) -> Value {
        Value::Str(Rc::new(bytes))
    }

    /// The index `n` as `for (k in a)` hands it out from an integer-indexed
    /// array: a number, until something asks for its text.
    #[must_use]
    pub fn index(n: i64) -> Value {
        Value::Index(Rc::new(Index {
            num: n,
            text: OnceCell::new(),
        }))
    }

    /// The value read as a number: gawk's `force_number`.
    ///
    /// A plain string converts by the `strtod` rule — as much of a number as
    /// there is at the front, zero if there is none — so `"3abc" + 0` is 3 and
    /// `"abc" + 0` is 0. That is a conversion, not an error; awk has no way to
    /// report one here.
    #[must_use]
    pub fn to_num(&self) -> f64 {
        match self {
            Value::Uninit => 0.0,
            Value::Num(n) => *n,
            Value::Str(s) => num_prefix(s).map_or(0.0, |(n, _)| n),
            Value::StrNum(i) => {
                i.forced.set(true);
                i.num
            }
            #[allow(clippy::cast_precision_loss)]
            Value::Index(i) => i.num as f64,
        }
    }

    /// The value read as a string, formatting a number with `convfmt`: gawk's
    /// `force_string`.
    #[must_use]
    pub fn to_str(&self, convfmt: &[u8]) -> Rc<Str> {
        match self {
            Value::Uninit => Rc::new(Str::new()),
            Value::Num(n) => Rc::new(num_to_str(*n, convfmt)),
            Value::Str(s) => Rc::clone(s),
            Value::StrNum(i) => Rc::clone(&i.text),
            Value::Index(i) => Rc::clone(i.text()),
        }
    }

    /// Whether the value is true in a condition: gawk's `boolval`.
    ///
    /// Note the asymmetry that trips people up: an *uninitialised* or *numeric*
    /// value is true when non-zero, but a plain string is true when non-empty —
    /// so the string `"0"` read from the program is **true** while the field
    /// `0` read from input is **false**. That is what strnum is for. An index
    /// from `for (k in a)` is asked its type here, and answers string: `0` is
    /// true.
    #[must_use]
    pub fn truthy(&self) -> bool {
        self.fixtype();
        match self {
            Value::Uninit => false,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::StrNum(i) => i.num != 0.0,
            // A string now, and the text of an integer is never empty.
            Value::Index(_) => true,
        }
    }

    /// gawk's `fixtype`: settle what the value is. Input that looks numeric
    /// becomes a number, and an index becomes a string.
    fn fixtype(&self) {
        match self {
            Value::StrNum(i) => i.forced.set(true),
            Value::Index(i) => {
                let _ = i.text();
            }
            Value::Uninit | Value::Num(_) | Value::Str(_) => {}
        }
    }

    /// Whether gawk's node carries the `STRING` flag, as it stands before
    /// [`Value::fixtype`]: what `cmp_scalars` tests to choose how to compare.
    fn flagged_string(&self) -> bool {
        match self {
            // `Nnull_string` is flagged both a string and a number.
            Value::Uninit | Value::Str(_) => true,
            Value::Num(_) => false,
            Value::StrNum(i) => !i.forced.get(),
            Value::Index(i) => i.text.get().is_some(),
        }
    }

    /// Whether the node carries the `NUMBER` flag, asked after
    /// [`Value::fixtype`] has settled it.
    fn flagged_number(&self) -> bool {
        match self {
            Value::Uninit | Value::Num(_) | Value::StrNum(_) => true,
            Value::Str(_) => false,
            Value::Index(i) => i.text.get().is_none(),
        }
    }

    /// Whether gawk's node has a string to show without making one: not a
    /// number (this keeps no number's formatted text; known-issues row 38),
    /// nor an index from `for (k in a)` nothing has asked the text of.
    #[must_use]
    pub fn has_text(&self) -> bool {
        match self {
            Value::Num(_) => false,
            Value::Index(i) => i.text.get().is_some(),
            Value::Uninit | Value::Str(_) | Value::StrNum(_) => true,
        }
    }

    /// Whether `printf "%c"` takes this as a character code rather than a
    /// string's first character: gawk calls `fixtype` and asks for `NUMBER`.
    #[must_use]
    pub fn is_char_code(&self) -> bool {
        self.fixtype();
        self.flagged_number()
    }

    /// Whether two values are one gawk node: `cmp_nodes` says equal at once,
    /// without settling either.
    fn same_node(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Uninit, Value::Uninit) => true,
            (Value::Str(a), Value::Str(b)) => Rc::ptr_eq(a, b),
            (Value::StrNum(a), Value::StrNum(b)) => Rc::ptr_eq(a, b),
            (Value::Index(a), Value::Index(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// gawk's `is_integer`: whether this subscript names an integer slot of an
    /// integer-indexed array, and which.
    ///
    /// A number must be integral and within C's 32-bit `int`; text must be
    /// written as gawk's `sprintf("%ld")` would write that integer -- `"3"` and
    /// `"-3"`, not `"+3"`, `"03"`, `"-0"`, `" 3"` or `"3.0"`. Input that passes
    /// the textual test is looked at as a number on the way (gawk sets its
    /// `NUMBER` flag), whether or not it then fits in 32 bits.
    #[must_use]
    pub fn is_integer(&self) -> Option<i64> {
        match self {
            Value::Uninit => None,
            Value::Num(d) => int32(*d),
            Value::Index(i) => Some(i.num),
            Value::Str(s) => integer_text(s).filter(|l| fits_int32(*l)),
            Value::StrNum(i) => {
                let l = integer_text(&i.text)?;
                i.forced.set(true);
                fits_int32(l).then_some(l)
            }
        }
    }

    /// What gawk's `str_lookup` keeps as the index of a new element, given the
    /// value's text: the node itself when it is a string that no format made
    /// -- so input not yet looked at as a number stays a strnum -- and a fresh
    /// string otherwise.
    #[must_use]
    pub fn index_name(&self, text: &Rc<Str>) -> Value {
        match self {
            Value::Str(s) => Value::Str(Rc::clone(s)),
            Value::StrNum(i) if !i.forced.get() => Value::StrNum(Rc::clone(i)),
            _ => Value::Str(Rc::clone(text)),
        }
    }
}

impl Index {
    /// The text, made on first asking -- which is what makes it a string.
    fn text(&self) -> &Rc<Str> {
        self.text
            .get_or_init(|| Rc::new(self.num.to_string().into_bytes()))
    }
}

/// `d` as a C `int`, if it is one exactly: `is_integer`'s numeric test.
fn int32(d: f64) -> Option<i64> {
    // `d <= INT32_MAX && d >= INT32_MIN && d == (int32_t) d`; NaN fails the
    // first test. In range, so the cast truncates and nothing else.
    if d <= f64::from(i32::MAX) && d >= f64::from(i32::MIN) {
        #[allow(clippy::cast_possible_truncation)]
        let i = d as i32;
        #[allow(clippy::float_cmp)]
        if f64::from(i) == d {
            return Some(i64::from(i));
        }
    }
    None
}

fn fits_int32(l: i64) -> bool {
    i32::try_from(l).is_ok()
}

/// The integer `s` spells, if it spells one as `is_integer` requires and C's
/// `strtol` takes it whole into a `long`: an optional `-`, then `0` alone or
/// digits not starting with `0`, and nothing else.
fn integer_text(s: &[u8]) -> Option<i64> {
    let (neg, digits) = match s {
        [b'-', rest @ ..] => (true, rest),
        _ => (false, s),
    };
    match digits {
        [] => return None,
        [b'0'] if !neg => return Some(0),
        [b'0', ..] => return None,
        _ => {}
    }
    let mut n: i64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        // Accumulated negative when negative, so `LONG_MIN` itself fits, as
        // it does for `strtol`; past either end is `ERANGE`, which refuses.
        let d = i64::from(c.wrapping_sub(b'0'));
        n = n.checked_mul(10)?;
        n = if neg {
            n.checked_sub(d)?
        } else {
            n.checked_add(d)?
        };
    }
    Some(n)
}

/// Compare two values as gawk's `cmp_scalars` does, for `op`.
///
/// When neither side is flagged a string -- numbers, input already looked at
/// as a number, an index not yet asked its text -- the comparison is C's: a
/// NaN is unequal to everything, itself included. Otherwise it is
/// `cmp_nodes`: two values that are numbers once settled compare as gawk sorts
/// numbers (NaN equal to NaN and above everything else), and anything else
/// compares as text, byte by byte -- which is the order `strcoll` gives in the
/// C.UTF-8 locale gawk is measured in, for text that is UTF-8.
#[must_use]
pub fn compare(a: &Value, b: &Value, op: CmpOp, convfmt: &[u8]) -> bool {
    let order = if a.flagged_string() || b.flagged_string() {
        if a.same_node(b) {
            Ordering::Equal
        } else {
            a.fixtype();
            b.fixtype();
            if a.flagged_number() && b.flagged_number() {
                awknum_order(a.to_num(), b.to_num())
            } else {
                a.to_str(convfmt)
                    .as_slice()
                    .cmp(b.to_str(convfmt).as_slice())
            }
        }
    } else {
        a.fixtype();
        b.fixtype();
        let (x, y) = (a.to_num(), b.to_num());
        // `cmp_doubles`: C's comparisons, so a NaN makes all but `!=` false.
        return match op {
            CmpOp::Lt => x < y,
            CmpOp::Le => x <= y,
            CmpOp::Gt => x > y,
            CmpOp::Ge => x >= y,
            #[allow(clippy::float_cmp)]
            CmpOp::Eq => x == y,
            #[allow(clippy::float_cmp)]
            CmpOp::Ne => x != y,
        };
    };
    match op {
        CmpOp::Lt => order.is_lt(),
        CmpOp::Le => order.is_le(),
        CmpOp::Gt => order.is_gt(),
        CmpOp::Ge => order.is_ge(),
        CmpOp::Eq => order.is_eq(),
        CmpOp::Ne => order.is_ne(),
    }
}

/// gawk's `cmp_awknums`: numbers as gawk sorts them, every NaN equal to every
/// other and greater than any number.
fn awknum_order(x: f64, y: f64) -> Ordering {
    match (x.is_nan(), y.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
    }
}

/// `x1 ^ x2` as gawk computes it (`calc_exp` in eval.c): an integral exponent
/// by repeated squaring, anything else by `pow`.
///
/// Not `f64::powf`, because the two round differently: `1.1 ^ 50` is
/// `117.39085287969571` by gawk's squaring and `...79` by `pow`, and
/// `2 ^ -1074` is 0 to gawk (it takes `1 / 2^1074`, and `2^1074` is already
/// infinite) where `pow` gives the smallest subnormal. awk's `^` is gawk's
/// here, digit for digit.
#[must_use]
pub fn calc_exp(x1: f64, x2: f64) -> f64 {
    let lx = c_long(x2);
    // `(lx = x2) == x2`: an exponent that survives the trip through `long`.
    // Exact on purpose -- the question is whether the value is an integer --
    // and the cast is exact for every `lx` that can answer yes.
    #[allow(clippy::cast_precision_loss, clippy::float_cmp)]
    if lx as f64 == x2 {
        if lx == 0 {
            return 1.0;
        }
        return if lx > 0 {
            calc_exp_posint(x1, lx)
        } else {
            // `-lx` in C, which wraps for LONG_MIN exactly as this does.
            1.0 / calc_exp_posint(x1, lx.wrapping_neg())
        };
    }
    x1.powf(x2)
}

/// `x ^ n` for a positive `n` by squaring, in gawk's order of operations so
/// that it rounds where gawk's does.
fn calc_exp_posint(mut x: f64, mut n: i64) -> f64 {
    let mut mult = 1.0;
    while n > 1 {
        if n % 2 == 1 {
            mult *= x;
        }
        x *= x;
        n /= 2;
    }
    mult * x
}

/// A C `(long) d` as x86-64 performs it: truncation toward zero, and the
/// "integer indefinite" value `LONG_MIN` for NaN and anything out of range,
/// which is what gawk's `NR`, `FNR`, field numbers and `calc_exp` see.
#[must_use]
pub fn c_long(d: f64) -> i64 {
    // 2^63 is exactly representable; anything at or above it, or below
    // -2^63, does not fit.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if d.is_nan() || d >= LIMIT || d < -LIMIT {
        return i64::MIN;
    }
    // In range by the guard above, so the cast truncates and nothing else.
    #[allow(clippy::cast_possible_truncation)]
    let l = d as i64;
    l
}

/// Render a number as gawk's `format_val` does, for `CONVFMT` (a string
/// conversion) or `OFMT` (`print`).
///
/// * An infinity or a NaN is `+inf`, `-inf`, `+nan` or `-nan`: gawk's own
///   spelling, sign always shown, not C's.
/// * An integral value within a C `long` is `%d` of it -- `print 1/1` says
///   `1`, not `1.000000` -- and one past that range is `%.0f`, every digit:
///   `print 2^63` is `9223372036854775808` and `print 1e30` is
///   `1000000000000000019884624838656`, where `%.6g` would have rounded them
///   away.
/// * Anything else goes through `fmt`.
#[must_use]
pub fn num_to_str(n: f64, fmt: &[u8]) -> Str {
    if !n.is_finite() {
        let word: &[u8] = match (n.is_nan(), n.is_sign_negative()) {
            (true, true) => b"-nan",
            (true, false) => b"+nan",
            (false, true) => b"-inf",
            (false, false) => b"+inf",
        };
        return word.to_vec();
    }
    let t = n.trunc();
    // `double_to_int(n) != n || val <= LONG_MIN || val >= LONG_MAX`, where
    // LONG_MAX as a double is 2^63 exactly.
    const LONG_LIMIT: f64 = 9_223_372_036_854_775_808.0;
    // Exact on purpose: whether `n` has a fractional part at all.
    #[allow(clippy::float_cmp)]
    let integral = t == n;
    if !integral || t <= -LONG_LIMIT || t >= LONG_LIMIT {
        if integral {
            return format!("{n:.0}").into_bytes();
        }
        return crate::fmt::sprintf_one_number(fmt, n);
    }
    // In range by the test above, so the cast is exact; `-0.0` is `0`.
    #[allow(clippy::cast_possible_truncation)]
    let i = t as i64;
    i.to_string().into_bytes()
}

/// Whether the whole string is a number, and if so which — the strnum test,
/// as gawk's `r_force_number` makes it under `--posix`.
///
/// The *whole* string, up to surrounding blanks: `" 12 "` is a number and
/// `"12x"` is not. `--posix` hands the rest to C's `strtod` unfiltered, so
/// everything `strtod` reads is a number: hexadecimal (`0x1A`, `0x1p-3`),
/// `inf` and `infinity`, `nan` and `nan(...)`, in any case. (This excluded
/// those until 2026-10-01, on the belief that gawk's POSIX mode did; it is
/// gawk's *non*-POSIX mode that does.) A single character must be a digit.
#[must_use]
pub fn numeric_string(s: &[u8]) -> Option<f64> {
    let t = trim_blanks(s);
    match t {
        [] => None,
        [c] => c.is_ascii_digit().then(|| f64::from(c.wrapping_sub(b'0'))),
        _ => {
            let (n, used) = strtod(t)?;
            if used != t.len() {
                return None;
            }
            // gawk gives a NaN written with a minus its sign itself, in case
            // `strtod` did not.
            if n.is_nan() && t.first() == Some(&b'-') && !n.is_sign_negative() {
                return Some(-n);
            }
            Some(n)
        }
    }
}

/// The longest numeric prefix of `s`, and how many bytes it used: C's
/// `strtod`, which is what gawk converts a string with -- so `"0x1A" + 0` is
/// 26, `"inf" + 0` is `+inf` and `"3abc" + 0` is 3. Leading blanks are skipped
/// and counted.
#[must_use]
pub fn num_prefix(s: &[u8]) -> Option<(f64, usize)> {
    strtod(s)
}

/// glibc's `strtod`: the longest prefix of `s` that is a number -- decimal,
/// hexadecimal, `inf`/`infinity` or `nan`/`nan(chars)`, any case, with a sign
/// -- correctly rounded, and the bytes it took (leading blanks included), or
/// `None` where there is no number at all. A value too large is an infinity
/// and one too small a zero or a subnormal, as `strtod` returns them with
/// `ERANGE` -- which gawk accepts.
#[must_use]
pub fn strtod(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0usize;
    while matches!(s.get(i), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)) {
        i = i.saturating_add(1);
    }
    let negative = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    let rest = s.get(i..).unwrap_or_default();
    let (magnitude, used) = if let Some(found) = hex_prefix(rest) {
        found
    } else if let Some(used) = word_prefix(rest, b"infinity").or_else(|| word_prefix(rest, b"inf"))
    {
        (f64::INFINITY, used)
    } else if let Some(used) = word_prefix(rest, b"nan") {
        // `nan(n-char-sequence)`: letters, digits and `_`, closed; an
        // unclosed one leaves just the `nan`.
        let mut j = used;
        if rest.get(j) == Some(&b'(') {
            let mut k = j.saturating_add(1);
            while matches!(rest.get(k), Some(c) if c.is_ascii_alphanumeric() || *c == b'_') {
                k = k.saturating_add(1);
            }
            if rest.get(k) == Some(&b')') {
                j = k.saturating_add(1);
            }
        }
        (f64::NAN, j)
    } else {
        decimal_prefix(rest)?
    };
    let n = if negative { -magnitude } else { magnitude };
    Some((n, i.saturating_add(used)))
}

/// `word`, in any case, at the start of `s`: how long it is, if it is there.
fn word_prefix(s: &[u8], word: &[u8]) -> Option<usize> {
    let head = s.get(..word.len())?;
    head.eq_ignore_ascii_case(word).then_some(word.len())
}

/// An unsigned decimal number at the start of `s`, as `strtod` and awk's
/// own numeric constants read one: digits with an optional point (at least
/// one digit, before or after it), then an exponent only if it is complete --
/// `1e` is the number 1 followed by the letter e, not a malformed float.
/// Correctly rounded, as `strtod` is.
#[must_use]
pub fn decimal_prefix(s: &[u8]) -> Option<(f64, usize)> {
    let mut i = 0usize;
    let mut digits = 0usize;
    while matches!(s.get(i), Some(c) if c.is_ascii_digit()) {
        i = i.saturating_add(1);
        digits = digits.saturating_add(1);
    }
    if s.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        while matches!(s.get(i), Some(c) if c.is_ascii_digit()) {
            i = i.saturating_add(1);
            digits = digits.saturating_add(1);
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(s.get(i), Some(b'e' | b'E')) {
        let mut j = i.saturating_add(1);
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        if matches!(s.get(j), Some(c) if c.is_ascii_digit()) {
            while matches!(s.get(j), Some(c) if c.is_ascii_digit()) {
                j = j.saturating_add(1);
            }
            i = j;
        }
    }
    let text = s.get(..i)?;
    // Every byte in `text` is ASCII by construction above.
    let n: f64 = std::str::from_utf8(text).ok()?.parse().ok()?;
    Some((n, i))
}

/// A hexadecimal number at the start of `s`, as `strtod` reads one: `0x`,
/// hex digits with an optional point (at least one digit), then a binary
/// exponent `p` only if it is complete; correctly rounded, to nearest with
/// ties to even. `None` without a digit after the `0x` -- `strtod` then reads
/// the `0` alone, which is the decimal reading's to give.
fn hex_prefix(s: &[u8]) -> Option<(f64, usize)> {
    if !matches!(s, [b'0', b'x' | b'X', ..]) {
        return None;
    }
    let mut i = 2usize;
    // The significant digits, gathered into 64 bits; what does not fit only
    // matters as "something non-zero was cut off".
    let mut mantissa: u64 = 0;
    let mut kept = 0i64; // digits gathered
    let mut sticky = false;
    let mut int_digits = 0i64; // digits before the point, leading zeros included
    let mut leading_zeros = 0i64;
    let mut any = false;
    let mut seen_point = false;
    loop {
        match s.get(i) {
            Some(b'.') if !seen_point => seen_point = true,
            Some(c) if c.is_ascii_hexdigit() => {
                any = true;
                let d = u64::from(char::from(*c).to_digit(16).unwrap_or(0));
                if !seen_point {
                    int_digits = int_digits.saturating_add(1);
                }
                if mantissa == 0 && d == 0 && kept == 0 {
                    leading_zeros = leading_zeros.saturating_add(1);
                } else if kept < 16 {
                    mantissa = (mantissa << 4) | d;
                    kept = kept.saturating_add(1);
                } else if d != 0 {
                    sticky = true;
                }
            }
            _ => break,
        }
        i = i.saturating_add(1);
    }
    if !any {
        return None;
    }
    let mut exp: i64 = 0;
    if matches!(s.get(i), Some(b'p' | b'P')) {
        let mut j = i.saturating_add(1);
        let neg = s.get(j) == Some(&b'-');
        if matches!(s.get(j), Some(b'+' | b'-')) {
            j = j.saturating_add(1);
        }
        if matches!(s.get(j), Some(c) if c.is_ascii_digit()) {
            while let Some(c) = s.get(j).filter(|c| c.is_ascii_digit()) {
                // Far past any exponent that could matter; held there.
                exp = exp
                    .saturating_mul(10)
                    .saturating_add(i64::from(c.wrapping_sub(b'0')))
                    .min(1_i64 << 40);
                j = j.saturating_add(1);
            }
            if neg {
                exp = exp.saturating_neg();
            }
            i = j;
        }
    }
    // value = mantissa * 2^e2 (+ something below it when `sticky`): every
    // digit not gathered scales the gathered ones by 16, and every digit
    // after the point divides by 16.
    let e2 = exp.saturating_add(
        int_digits
            .saturating_sub(leading_zeros)
            .saturating_sub(kept)
            .saturating_mul(4),
    );
    Some((round_binary(mantissa, e2, sticky), i))
}

/// `m * 2^e2`, plus a little more when `sticky`, as the nearest `f64`, ties to
/// even: the rounding `strtod` does, subnormals and overflow to infinity
/// included.
fn round_binary(m: u64, e2: i64, sticky: bool) -> f64 {
    if m == 0 {
        return 0.0;
    }
    let lz = i64::from(m.leading_zeros());
    let m = m << lz;
    // The exponent of `m`'s leading bit, now bit 63.
    let top = e2.saturating_sub(lz).saturating_add(63);
    // Bits kept: 53 for a normal result, fewer below the normal range.
    let keep = if top >= -1022 {
        53
    } else {
        top.saturating_add(1075)
    };
    if keep < 0 {
        return 0.0;
    }
    let dropped = 64i64.saturating_sub(keep); // 11 ..= 64
    let (mut q, round_up) = if dropped >= 64 {
        // Nothing kept: the value is at least half the smallest subnormal,
        // and exactly half only when nothing else was cut off.
        let half = 1u64 << 63;
        (0u64, m > half || (m == half && sticky))
    } else {
        let d = u32::try_from(dropped).unwrap_or(63);
        let q = m >> d;
        let rem = m & ((1u64 << d).wrapping_sub(1));
        let half = 1u64 << d.saturating_sub(1);
        let up = rem > half || (rem == half && (sticky || q & 1 == 1));
        (q, up)
    };
    if round_up {
        q = q.saturating_add(1);
    }
    if top >= -1022 {
        // Normal: q is 2^52 ..= 2^53; a carry out moves up a binade.
        let (q, top) = if q >> 53 != 0 {
            (q >> 1, top.saturating_add(1))
        } else {
            (q, top)
        };
        if top > 1023 {
            return f64::INFINITY;
        }
        let biased = u64::try_from(top.saturating_add(1023)).unwrap_or(0);
        f64::from_bits((biased << 52) | (q & ((1u64 << 52) - 1)))
    } else {
        // Subnormal: q is the stored mantissa; a carry into bit 52 is the
        // smallest normal number, which the same bits spell.
        f64::from_bits(q)
    }
}

/// `s` without leading and trailing blanks, for the strnum test.
fn trim_blanks(s: &[u8]) -> &[u8] {
    let mut a = 0usize;
    let mut b = s.len();
    while matches!(s.get(a), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)) {
        a = a.saturating_add(1);
    }
    while b > a
        && matches!(
            s.get(b.saturating_sub(1)),
            Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
        )
    {
        b = b.saturating_sub(1);
    }
    s.get(a..b).unwrap_or_default()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    // The values compared are exactly representable; an epsilon would only
    // hide a conversion that came out a bit off.
    clippy::float_cmp,
    // The expected values are written as gawk printed them with `%.17g`,
    // which is the point of them, not the shortest literal for the `f64`.
    clippy::excessive_precision
)]
mod tests {
    use super::*;

    fn sv(s: &str) -> Value {
        Value::str(s.as_bytes().to_vec())
    }
    fn iv(s: &str) -> Value {
        Value::from_input(s.as_bytes().to_vec())
    }

    fn eq(a: &Value, b: &Value) -> bool {
        compare(a, b, CmpOp::Eq, b"%.6g")
    }
    fn lt(a: &Value, b: &Value) -> bool {
        compare(a, b, CmpOp::Lt, b"%.6g")
    }
    fn gt(a: &Value, b: &Value) -> bool {
        compare(a, b, CmpOp::Gt, b"%.6g")
    }

    #[test]
    fn a_field_that_looks_numeric_compares_as_a_number() {
        // The rule the whole enum exists for.
        assert!(eq(&iv(" 10 "), &Value::Num(10.0)));
        // …but against a string literal it is text, so the blanks count.
        assert!(!eq(&iv(" 10 "), &sv("10")));
    }

    #[test]
    fn a_program_literal_is_never_a_strnum() {
        // `"007" == 7` is false: one side is a literal, so this is text.
        assert!(!eq(&sv("007"), &Value::Num(7.0)));
        // The same characters from input do compare equal.
        assert!(eq(&iv("007"), &Value::Num(7.0)));
    }

    /// gawk 5.2.1 `--posix`, measured: an index from `for (k in a)` over an
    /// integer-indexed array compares as a number the first time and as a
    /// string from then on -- `print (k < 10), (k < 10)` with `k` 9 prints
    /// `1 0` -- because the first comparison settles it into a string.
    #[test]
    fn an_index_is_a_number_until_something_asks_for_its_text() {
        let k = Value::index(9);
        assert!(lt(&k, &Value::Num(10.0)));
        // Now a string: "9" against "10", as text.
        assert!(!lt(&k, &Value::Num(10.0)));

        // Every copy is the one node: settling a copy settles the original.
        let k = Value::index(9);
        let copy = k.clone();
        assert!(copy.truthy());
        assert!(!lt(&k, &Value::Num(10.0)));

        // Arithmetic does not ask its type; `%d`-style use does not either.
        let k = Value::index(9);
        assert_eq!(k.to_num(), 9.0);
        assert!(lt(&k, &Value::Num(10.0)));

        // A truth test does: the index 0 is the string "0", which is true.
        assert!(Value::index(0).truthy());
        // Against a string or an unset value it is text at once.
        let k = Value::index(9);
        assert!(!lt(&k, &sv("10")));
        assert!(gt(&Value::index(9), &Value::Uninit));
    }

    /// The `max` idiom over 9, 10 and 100, as gawk runs it: the first
    /// comparison is against an unset `max`, which makes `k` a string, and
    /// `max = k` passes the string on. gawk prints 9.
    #[test]
    fn the_max_idiom_over_indices_compares_text() {
        let mut max = Value::Uninit;
        for n in [9, 10, 100] {
            let k = Value::index(n);
            if gt(&k, &max) {
                max = k;
            }
        }
        assert_eq!(max.to_str(b"%.6g").as_slice(), b"9");
    }

    /// Input that looks numeric is flagged a string until something treats it
    /// as a number, and the flag is on the node every copy shares.
    #[test]
    fn input_is_settled_by_numeric_use_and_the_settling_is_shared() {
        let f = iv("10");
        let copy = f.clone();
        assert!(f.flagged_string());
        let _ = copy.to_num();
        assert!(!f.flagged_string());
        // A comparison settles it; a string use does not.
        let f = iv("10");
        let _ = f.to_str(b"%.6g");
        assert!(f.flagged_string());
        assert!(eq(&f, &Value::Num(10.0)));
        assert!(!f.flagged_string());
        // Text that is not a number is a plain string from the start.
        assert!(matches!(iv("x"), Value::Str(_)));
    }

    /// A NaN compares as C compares it when neither side is flagged a string,
    /// and as gawk sorts numbers when one is (`cmp_awknums`: equal to another
    /// NaN, above every number).
    #[test]
    fn nan_compares_by_whichever_rule_gawk_reaches() {
        let nan = Value::Num(f64::NAN);
        assert!(!eq(&nan, &nan.clone()));
        assert!(!gt(&nan, &Value::Num(1.0)));
        assert!(compare(&nan, &Value::Num(1.0), CmpOp::Ne, b"%.6g"));
        // Against the unset value, which is flagged a string as well.
        assert!(gt(&nan, &Value::Uninit));
        assert!(!lt(&nan, &Value::Uninit));
    }

    /// One node compared with itself is equal at once, and settles nothing.
    #[test]
    fn a_node_compared_with_itself_is_not_settled() {
        let f = iv("10");
        assert!(eq(&f, &f.clone()));
        assert!(f.flagged_string());
    }

    /// `is_integer`, case by case from gawk's int_array.c.
    #[test]
    fn integer_subscripts_are_spelled_as_sprintf_would_spell_them() {
        assert_eq!(sv("3").is_integer(), Some(3));
        assert_eq!(sv("-3").is_integer(), Some(-3));
        assert_eq!(sv("0").is_integer(), Some(0));
        for s in ["+3", "03", "-0", " 3", "3 ", "3.0", "", "-", "1e3", "0x10"] {
            assert_eq!(sv(s).is_integer(), None, "{s:?}");
        }
        assert_eq!(sv("2147483647").is_integer(), Some(2_147_483_647));
        assert_eq!(sv("-2147483648").is_integer(), Some(-2_147_483_648));
        assert_eq!(sv("2147483648").is_integer(), None);
        assert_eq!(Value::Num(3.0).is_integer(), Some(3));
        assert_eq!(Value::Num(-0.0).is_integer(), Some(0));
        assert_eq!(Value::Num(3.5).is_integer(), None);
        assert_eq!(Value::Num(1e10).is_integer(), None);
        assert_eq!(Value::Num(f64::NAN).is_integer(), None);
        assert_eq!(Value::Uninit.is_integer(), None);
        assert_eq!(Value::index(7).is_integer(), Some(7));
        // Input that spells an integer is settled a number on the way, even
        // one too big for 32 bits; input that does not is left alone.
        let big = iv("99999999999");
        assert_eq!(big.is_integer(), None);
        assert!(!big.flagged_string());
        let frac = iv("1.5");
        assert_eq!(frac.is_integer(), None);
        assert!(frac.flagged_string());
    }

    /// What a string array keeps as an index: the node when it is a string no
    /// format made, input included while still unsettled; a fresh string
    /// otherwise.
    #[test]
    fn a_string_array_keeps_unsettled_input_as_input() {
        let text = Rc::new(b"10".to_vec());
        assert!(matches!(iv("10").index_name(&text), Value::StrNum(_)));
        let settled = iv("10");
        let _ = settled.to_num();
        assert!(matches!(settled.index_name(&text), Value::Str(_)));
        assert!(matches!(Value::Num(10.0).index_name(&text), Value::Str(_)));
        assert!(matches!(
            Value::from_assignment(b"10".to_vec()).index_name(&text),
            Value::Str(_)
        ));
    }

    #[test]
    fn the_string_zero_is_true_but_the_field_zero_is_false() {
        assert!(sv("0").truthy());
        assert!(!iv("0").truthy());
        assert!(!Value::Uninit.truthy());
        assert!(!sv("").truthy());
        assert!(sv("x").truthy());
    }

    /// gawk's `^`, digit for digit. The expected values are what gawk 5.2.1
    /// printed with `%.17g`, which round-trips an `f64` exactly.
    #[test]
    fn power_rounds_where_gawks_squaring_does() {
        assert_eq!(calc_exp(3.0, 33.0), 5_559_060_566_555_523.0);
        // `pow` gives ...79; squaring gives ...71.
        assert_eq!(calc_exp(1.1, 50.0), 117.390_852_879_695_71);
        assert_eq!(calc_exp(7.0, -21.0), 1.790_363_270_599_549_8e-18);
        // 2^1074 overflows before the reciprocal is taken.
        assert_eq!(calc_exp(2.0, -1074.0), 0.0);
        assert_eq!(calc_exp(2.0, 1024.0), f64::INFINITY);
        assert_eq!(calc_exp(0.0, 0.0), 1.0);
        assert_eq!(calc_exp(-0.0, -1.0), f64::NEG_INFINITY);
        assert_eq!(calc_exp(-2.0, 3.0), -8.0);
        // A fractional exponent is `pow`'s.
        assert_eq!(calc_exp(2.0, 0.5), 2f64.sqrt());
        assert!(calc_exp(-8.0, 1.0 / 3.0).is_nan());
    }

    /// A C `(long)` on x86-64: truncation, and `LONG_MIN` for anything that
    /// does not fit -- which is what gawk prints for `FNR = 1e30; print FNR`.
    #[test]
    fn a_c_long_truncates_and_saturates_to_long_min() {
        assert_eq!(c_long(2.5), 2);
        assert_eq!(c_long(-2.5), -2);
        assert_eq!(c_long(-0.5), 0);
        assert_eq!(c_long(1e30), i64::MIN);
        assert_eq!(c_long(-1e30), i64::MIN);
        assert_eq!(c_long(f64::NAN), i64::MIN);
        assert_eq!(c_long(f64::INFINITY), i64::MIN);
        assert_eq!(c_long(9_223_372_036_854_775_808.0), i64::MIN);
        assert_eq!(c_long(-9_223_372_036_854_775_808.0), i64::MIN);
        assert_eq!(
            c_long(9_223_372_036_854_774_784.0),
            9_223_372_036_854_774_784
        );
    }

    #[test]
    fn a_number_prints_as_an_integer_when_it_is_one() {
        assert_eq!(num_to_str(1.0, b"%.6g"), b"1");
        assert_eq!(num_to_str(-0.0, b"%.6g"), b"0");
        assert_eq!(num_to_str(1e17, b"%.6g"), b"100000000000000000");
        assert_eq!(num_to_str(0.5, b"%.6g"), b"0.5");
        assert_eq!(num_to_str(1.0 / 3.0, b"%.6g"), b"0.333333");
        // Past a `long`, every digit, as gawk's `%.0f` writes them.
        assert_eq!(num_to_str(1e18, b"%.6g"), b"1000000000000000000");
        assert_eq!(
            num_to_str(9_223_372_036_854_775_808.0, b"%.6g"),
            b"9223372036854775808"
        );
        assert_eq!(
            num_to_str(-9_223_372_036_854_775_808.0, b"%.6g"),
            b"-9223372036854775808"
        );
        assert_eq!(
            num_to_str(1e30, b"%.6g"),
            b"1000000000000000019884624838656"
        );
        // gawk's own spelling of the values that are not numbers.
        assert_eq!(num_to_str(f64::INFINITY, b"%.6g"), b"+inf");
        assert_eq!(num_to_str(f64::NEG_INFINITY, b"%.6g"), b"-inf");
        assert_eq!(num_to_str(f64::NAN, b"%.6g"), b"+nan");
        assert_eq!(num_to_str(-f64::NAN, b"%.6g"), b"-nan");
        // A subnormal, through CONVFMT, digit for digit.
        assert_eq!(num_to_str(1e-320, b"%.6g"), b"9.99989e-321");
    }

    #[test]
    fn conversion_takes_the_numeric_prefix_and_no_more() {
        assert_eq!(sv("3abc").to_num(), 3.0);
        assert_eq!(sv("abc").to_num(), 0.0);
        assert_eq!(sv("  -2.5e2xyz").to_num(), -250.0);
        // `1e` is 1 followed by a letter, not a broken float.
        assert_eq!(sv("1e").to_num(), 1.0);
        assert_eq!(num_prefix(b"1e"), Some((1.0, 1)));
    }

    #[test]
    fn the_strnum_test_wants_the_whole_string() {
        assert_eq!(numeric_string(b" 12 "), Some(12.0));
        assert_eq!(numeric_string(b"12x"), None);
        assert_eq!(numeric_string(b""), None);
        assert_eq!(numeric_string(b"+.5"), Some(0.5));
        // One character must be a digit; `strtod` would not take the others
        // anyway.
        assert_eq!(numeric_string(b"7"), Some(7.0));
        assert_eq!(numeric_string(b"."), None);
        assert_eq!(numeric_string(b"+"), None);
        // `--posix` hands the rest to `strtod`: hexadecimal, infinities and
        // NaNs are numbers (gawk 5.2.1, measured).
        assert_eq!(numeric_string(b"0x1A"), Some(26.0));
        assert_eq!(numeric_string(b" 0x1p3 "), Some(8.0));
        assert_eq!(numeric_string(b"inf"), Some(f64::INFINITY));
        assert_eq!(numeric_string(b"-Infinity"), Some(f64::NEG_INFINITY));
        assert!(numeric_string(b"nan").is_some_and(|n| n.is_nan() && !n.is_sign_negative()));
        assert!(numeric_string(b"-nan").is_some_and(|n| n.is_nan() && n.is_sign_negative()));
        assert!(numeric_string(b"NaN(12_ab)").is_some_and(f64::is_nan));
        // Only whole: `0x` alone is the 0 and a letter; `nan(` is unclosed.
        assert_eq!(numeric_string(b"0x"), None);
        assert_eq!(numeric_string(b"nan("), None);
        assert_eq!(numeric_string(b"infinit"), None);
    }

    /// glibc's `strtod`, case by case as gawk 5.2.1 `--posix` printed them
    /// (`target/drafts/strtod-probe.sh`).
    #[test]
    fn strtod_reads_what_glibc_reads() {
        let n = |s: &str| strtod(s.as_bytes()).map(|(n, _)| n);
        let used = |s: &str| strtod(s.as_bytes()).map(|(_, u)| u);
        assert_eq!(n("0x"), Some(0.0));
        assert_eq!(used("0x"), Some(1));
        assert_eq!(n("0x1g"), Some(1.0));
        assert_eq!(n("0x.8"), Some(0.5));
        assert_eq!(n("0x1p-2"), Some(0.25));
        assert_eq!(n("0X1P+3"), Some(8.0));
        assert_eq!(n("-0x10"), Some(-16.0));
        assert_eq!(n("0x1.8p1"), Some(3.0));
        assert_eq!(n("0x0.001"), Some(1.0 / 4096.0));
        assert_eq!(used("0x1.p"), Some(4));
        // Ties to even at 53 bits, and a long mantissa's tail as a sticky bit.
        assert_eq!(n("0x1.fffffffffffff8p0"), Some(2.0));
        assert_eq!(n("0x1.fffffffffffff7p0"), Some(1.999_999_999_999_999_8));
        assert_eq!(
            n("0x123456789abcdef123p0"),
            Some(3.358_127_276_707_303_3e20)
        );
        // Subnormals, the halfway point below the smallest, and overflow.
        assert_eq!(n("0x1p-1074"), Some(4.940_656_458_412_465_4e-324));
        assert_eq!(n("0x1p-1075"), Some(0.0));
        assert_eq!(n("0x1.8p-1075"), Some(4.940_656_458_412_465_4e-324));
        assert_eq!(n("0x1p-1022"), Some(f64::MIN_POSITIVE));
        assert_eq!(n("0x1p1024"), Some(f64::INFINITY));
        assert_eq!(n("0x1p1023"), Some(2f64.powi(1023)));
        assert_eq!(n("0x1.fffffffffffffp1023"), Some(f64::MAX));
        assert_eq!(n("0x1p99999999999999999999"), Some(f64::INFINITY));
        assert_eq!(n("0x1p-99999999999999999999"), Some(0.0));
        // The words, any case, and how much each takes.
        assert_eq!(n("infinity"), Some(f64::INFINITY));
        assert_eq!(used("infinit"), Some(3));
        assert_eq!(used("  -INF"), Some(6));
        assert_eq!(used("nan()"), Some(5));
        assert_eq!(used("nan(12ab_)x"), Some(10));
        assert_eq!(used("nan("), Some(3));
        // Decimal as before, and nothing where there is no number.
        assert_eq!(n("  -2.5e2xyz"), Some(-250.0));
        assert_eq!(n("5."), Some(5.0));
        assert_eq!(n("1e400"), Some(f64::INFINITY));
        assert_eq!(n("1e-400"), Some(0.0));
        assert_eq!(n("in"), None);
        assert_eq!(n("-"), None);
        assert_eq!(n(" "), None);
        // The decimal reader the lexer uses takes no hexadecimal.
        assert_eq!(decimal_prefix(b"0x1A"), Some((0.0, 1)));
    }
}
