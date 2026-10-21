//! Numbers: `numeric.c`, the glue between dc and bc's `number.c`
//! ([`bcnumber`]) -- each operation and its complaint, reading a number,
//! and writing one with the line breaks `DC_LINE_LENGTH` asks for.

use bcnumber::{Diag, Num};

use crate::{Data, Dc};

/// `DEFAULT_LINE_MAX`.
const DEFAULT_LINE_MAX: i32 = 70;

/// `number.c`'s `rt_warn` and `rt_error`, as dc defines them: a line on
/// standard error, checked.
struct Runtime<'a>(&'a mut Dc);

impl Diag for Runtime<'_> {
    fn warn(&mut self, msg: &str) {
        let mut line = b"Runtime warning: ".to_vec();
        line.extend_from_slice(msg.as_bytes());
        line.push(b'\n');
        self.0.err.write(&line);
        self.0.check_stderr();
    }

    fn error(&mut self, msg: &str) {
        let mut line = b"Runtime error: ".to_vec();
        line.extend_from_slice(msg.as_bytes());
        line.push(b'\n');
        self.0.err.write(&line);
        self.0.check_stderr();
    }
}

/// dc's scale as `number.c` takes it: never negative, as `k` refuses one.
fn scale_of(kscale: i32) -> usize {
    usize::try_from(kscale).unwrap_or(0)
}

/// `dc_add`.
#[allow(clippy::unnecessary_wraps, reason = "an `Op2`, as every operation is")]
pub fn add(_: &mut Dc, a: &Num, b: &Num, _kscale: i32) -> Option<Num> {
    Some(bcnumber::add(a, b, 0))
}

/// `dc_sub`.
#[allow(clippy::unnecessary_wraps, reason = "an `Op2`, as every operation is")]
pub fn sub(_: &mut Dc, a: &Num, b: &Num, _kscale: i32) -> Option<Num> {
    Some(bcnumber::sub(a, b, 0))
}

/// `dc_mul`.
#[allow(clippy::unnecessary_wraps, reason = "an `Op2`, as every operation is")]
pub fn mul(_: &mut Dc, a: &Num, b: &Num, kscale: i32) -> Option<Num> {
    Some(bcnumber::multiply(a, b, i64::from(kscale)))
}

/// `dc_div`.
pub fn div(dc: &mut Dc, a: &Num, b: &Num, kscale: i32) -> Option<Num> {
    let r = bcnumber::divide(a, b, scale_of(kscale));
    if r.is_none() {
        dc.complain(b"divide by zero\n");
    }
    r
}

/// `dc_divrem`: the quotient and the remainder.
pub fn divrem(dc: &mut Dc, a: &Num, b: &Num, kscale: i32) -> Option<(Num, Num)> {
    let r = bcnumber::divmod(a, b, scale_of(kscale));
    if r.is_none() {
        dc.complain(b"divide by zero\n");
    }
    r
}

/// `dc_rem`.
pub fn rem(dc: &mut Dc, a: &Num, b: &Num, kscale: i32) -> Option<Num> {
    let r = bcnumber::modulo(a, b, scale_of(kscale));
    if r.is_none() {
        dc.complain(b"remainder by zero\n");
    }
    r
}

/// `dc_modexp`: `base` to the `expo` modulo `modulus`. Of its failures
/// only a zero modulus is said (and, as upstream, not checked); a negative
/// exponent fails without a word.
pub fn modexp(dc: &mut Dc, base: &Num, expo: &Num, modulus: &Num, kscale: i32) -> Option<Num> {
    let r = bcnumber::raisemod(base, expo, modulus, scale_of(kscale), &mut Runtime(dc));
    if r.is_none() && bcnumber::is_zero(modulus) {
        dc.say(b"remainder by zero\n");
    }
    r
}

/// `dc_exp`.
#[allow(clippy::unnecessary_wraps, reason = "an `Op2`, as every operation is")]
pub fn exp(dc: &mut Dc, a: &Num, b: &Num, kscale: i32) -> Option<Num> {
    Some(bcnumber::raise(
        a,
        b,
        Num::zero(),
        i64::from(kscale),
        &mut Runtime(dc),
    ))
}

/// `dc_sqrt`.
pub fn sqrt(dc: &mut Dc, value: &Num, kscale: i32) -> Option<Num> {
    let r = bcnumber::sqrt(value, scale_of(kscale), &mut Runtime(dc));
    if r.is_none() {
        dc.complain(b"square root of negative number\n");
    }
    r
}

/// `dc_int2data (value)`.
#[must_use]
pub fn int2data(value: i32) -> Data {
    Data::num(bcnumber::int2num(value))
}

/// `dc_numlen (value)`: its digits, every leading zero not counted -- but
/// never fewer than one.
#[must_use]
pub fn numlen(n: &Num) -> i32 {
    let mut i = n.len.saturating_add(n.scale);
    let mut p = n.v.iter();
    while i > 1 && p.next() == Some(&0) {
        i = i.saturating_sub(1);
    }
    i32::try_from(i).unwrap_or(i32::MAX)
}

/// `isspace` in the C locale.
const fn is_space(c: Option<u8>) -> bool {
    matches!(c, Some(b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
}

/// A digit as dc reads one in any base: `0`-`9` and `A`-`F`.
fn digit(c: Option<u8>) -> Option<i32> {
    match c? {
        d @ b'0'..=b'9' => Some(i32::from(d.wrapping_sub(b'0'))),
        d @ b'A'..=b'F' => Some(i32::from(d.wrapping_sub(b'A')).wrapping_add(10)),
        _ => None,
    }
}

/// `dc_getnum (input, ibase, &readahead)`: a number read from `input` in
/// base `ibase`, and the byte after it (`None` for the end).
pub fn getnum(input: &mut dyn FnMut() -> Option<u8>, ibase: i32) -> (Num, Option<u8>) {
    let base = bcnumber::int2num(ibase);
    let mut result = Num::zero();
    let mut c = input();
    while is_space(c) {
        c = input();
    }
    let mut negative = false;
    if c == Some(b'_') || c == Some(b'-') {
        negative = true;
        c = input();
    } else if c == Some(b'+') {
        c = input();
    }
    while is_space(c) {
        c = input();
    }
    while let Some(d) = digit(c) {
        c = input();
        result = bcnumber::multiply(&result, &base, 0);
        result = bcnumber::add(&result, &bcnumber::int2num(d), 0);
    }
    if c == Some(b'.') {
        let mut divisor = Num::one();
        let mut build = Num::zero();
        let mut decimal = 0usize;
        loop {
            c = input();
            let Some(d) = digit(c) else {
                break;
            };
            build = bcnumber::multiply(&build, &base, 0);
            build = bcnumber::add(&build, &bcnumber::int2num(d), 0);
            divisor = bcnumber::multiply(&divisor, &base, 0);
            decimal = decimal.saturating_add(1);
        }
        // The divisor is a power of the base, never zero.
        if let Some(fraction) = bcnumber::divide(&build, &divisor, decimal) {
            result = bcnumber::add(&result, &fraction, 0);
        }
    }
    if negative {
        result = bcnumber::sub(&Num::zero(), &result, 0);
    }
    (result, c)
}

impl Dc {
    /// `dc_num2int (value)`: a number as a C `int` -- the `long`
    /// `bc_num2long` gives, cut to 32 bits; -1 for one too large, said.
    pub fn num2int(&mut self, n: &Num) -> i32 {
        let mut result = bcnumber::num2long(n);
        if result == 0 && !bcnumber::is_zero(n) {
            self.say(b"value overflows simple integer; punting...\n");
            result = -1;
        }
        // `(int) result`: the low 32 bits.
        cstrtol::low_i32(result)
    }

    /// `set_line_max_from_environment ()`: `DC_LINE_LENGTH`, if it is a
    /// number `strtol` reads whole (spaces after it allowed) that fits an
    /// `int` and is not negative or 1; else 70. 0 means no breaks.
    fn set_line_max(&mut self) {
        self.line_max = DEFAULT_LINE_MAX;
        let Some(env) = self.line_length.as_deref() else {
            return;
        };
        let (proposed, end, overflow) = cstrtol::strtol_overflow(env, 0);
        let rest = env.get(end..).unwrap_or_default();
        let rest_blank = rest.iter().all(|&b| cstrtol::isspace(b));
        let line = cstrtol::low_i32(proposed);
        self.line_max = line;
        if !rest_blank || overflow || i64::from(line) != proposed || line < 0 || line == 1 {
            self.line_max = DEFAULT_LINE_MAX;
        }
    }

    /// `out_char (ch)`: one byte of a number to standard output, after a
    /// backslash and a newline when the line is full. NUL starts a line
    /// afresh and writes nothing.
    fn out_char(&mut self, ch: u8) {
        if ch == 0 {
            self.out_col = 0;
            return;
        }
        if self.line_max < 0 {
            self.set_line_max();
        }
        self.out_col = self.out_col.saturating_add(1);
        if self.out_col >= self.line_max && self.line_max != 0 {
            self.out.write(b"\\\n");
            self.out_col = 1;
        }
        self.out.write(&[ch]);
        // Both streams, as upstream's `out_char` checks them after each
        // digit.
        self.check_stdout();
        self.check_stderr();
    }

    /// `dc_out_num (value, obase)`: a number in base `obase`.
    pub fn out_num(&mut self, n: &Num, obase: i32) {
        self.out_char(0);
        let mut digits = Vec::new();
        bcnumber::out_num(n, obase, &mut |c| digits.push(c), false);
        for c in digits {
            self.out_char(c);
        }
    }

    /// `dc_dump_num (value)`: the integer part of the number's absolute
    /// value, as bytes, base 256, most significant first.
    pub fn dump_num(&mut self, n: &Num) {
        let Some(mut value) = bcnumber::divide(n, &Num::one(), 0) else {
            return;
        };
        value.neg = false;
        let base = bcnumber::int2num(256);
        let mut bytes = Vec::new();
        loop {
            let Some((q, r)) = bcnumber::divmod(&value, &base, 0) else {
                return;
            };
            // `(int) bc_num2long (digit)`, then `putchar`'s `unsigned char`.
            bytes.push(bcnumber::num2long(&r).to_le_bytes()[0]);
            value = q;
            if bcnumber::is_zero(&value) {
                break;
            }
        }
        for b in bytes.into_iter().rev() {
            self.out.write(&[b]);
            self.check_stdout();
        }
    }

    /// `dc_print (value, obase, newline)`: a value, then a newline if
    /// asked, then standard output flushed (but not checked: the next check
    /// sees a failure).
    pub fn print(&mut self, value: &Data, newline: bool) {
        match value {
            Data::Num(n) => {
                let obase = self.obase;
                self.out_num(n, obase);
            }
            Data::Str(s) => {
                self.out.write(s);
                self.check_stdout();
            }
        }
        if newline {
            self.out.write(b"\n");
        }
        self.out.flush();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn read(text: &[u8], ibase: i32) -> (String, Option<u8>) {
        let mut it = text.iter().copied();
        let (n, after) = getnum(&mut || it.next(), ibase);
        (bcnumber::num2str(&n), after)
    }

    #[test]
    fn numbers_are_read_as_upstream_reads_them() {
        assert_eq!(read(b"12.5x", 10), ("12.5".to_owned(), Some(b'x')));
        assert_eq!(
            read(b"_ 3 ", 10),
            ("-3".to_owned(), Some(b' ')),
            "spaces after the sign"
        );
        assert_eq!(read(b"FF", 16), ("255".to_owned(), None));
        assert_eq!(read(b"A", 10), ("10".to_owned(), None), "A-F in any base");
        assert_eq!(read(b".1", 2), ("0.5".to_owned(), None));
        assert_eq!(read(b"1.", 10), ("1".to_owned(), None));
    }

    #[test]
    fn a_length_counts_no_leading_zero_but_one() {
        assert_eq!(numlen(&bcnumber::str2num(b"0.005", 3)), 1);
        assert_eq!(numlen(&bcnumber::str2num(b"0", 0)), 1);
        assert_eq!(numlen(&bcnumber::str2num(b"120.50", 2)), 5);
    }
}
