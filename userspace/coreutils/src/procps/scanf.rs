//! glibc's numeric conversions, as procps' library applies them to `/proc`.
//!
//! `readproc.c` reads almost every number it reports with one of four C
//! functions -- `strtol`, `strtoul`, `atoi` and `sscanf` -- and what it
//! reports for a file that is not quite what the kernel writes is whatever
//! those functions make of it. A field too wide for `int` is not refused: `%d`
//! keeps its low 32 bits. A negative number read by `%lu` is not refused
//! either: it wraps. A conversion that fails stops the scan and leaves every
//! later field as it was. These are the rules transcribed here, so that a
//! `/proc/<pid>/stat` the port reads gives the numbers upstream's would.
//!
//! The input is always a C string: callers cut it at its first NUL, as
//! `file2str` and every function after it see it.

/// `isspace` in the C locale: the six ASCII spaces.
#[must_use]
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | 0x09..=0x0d)
}

/// The digits of a base-10 number after its sign: their magnitude, saturated
/// at `u64::MAX` and flagged when it overflowed, and how many there were.
fn digits(s: &[u8]) -> (u64, bool, usize) {
    let mut mag: u64 = 0;
    let mut overflow = false;
    let mut n = 0usize;
    for &d in s.iter().take_while(|b| b.is_ascii_digit()) {
        match mag
            .checked_mul(10)
            .and_then(|m| m.checked_add(u64::from(d.wrapping_sub(b'0'))))
        {
            Some(m) => mag = m,
            None => overflow = true,
        }
        n = n.saturating_add(1);
    }
    (mag, overflow, n)
}

/// Leading whitespace and an optional sign: how many bytes they take, and
/// whether the sign was a minus.
fn prefix(s: &[u8]) -> (usize, bool) {
    let ws = s.iter().take_while(|&&b| is_space(b)).count();
    match s.get(ws) {
        Some(b'-') => (ws.saturating_add(1), true),
        Some(b'+') => (ws.saturating_add(1), false),
        _ => (ws, false),
    }
}

/// `strtol(s, &end, 10)`: the value and how many bytes it took, `(0, 0)` when
/// there is no number -- `end` left at `s`. Out of range saturates at
/// `LONG_MIN`/`LONG_MAX`, as glibc's does (setting `ERANGE`, which no caller
/// here reads).
#[must_use]
pub fn strtol(s: &[u8]) -> (i64, usize) {
    let (at, neg) = prefix(s);
    let (mag, overflow, n) = digits(s.get(at..).unwrap_or_default());
    if n == 0 {
        return (0, 0);
    }
    let used = at.saturating_add(n);
    let value = if neg {
        if overflow || mag > i64::MIN.unsigned_abs() {
            i64::MIN
        } else {
            0i64.wrapping_sub_unsigned(mag)
        }
    } else if overflow || mag > i64::MAX.unsigned_abs() {
        i64::MAX
    } else {
        i64::try_from(mag).unwrap_or(i64::MAX)
    };
    (value, used)
}

/// `strtoul(s, &end, 10)`: as [`strtol`], but a minus sign negates in
/// `unsigned long` -- `-1` is `ULONG_MAX` -- and only a magnitude past
/// `ULONG_MAX` saturates, whatever its sign.
#[must_use]
pub fn strtoul(s: &[u8]) -> (u64, usize) {
    let (at, neg) = prefix(s);
    let (mag, overflow, n) = digits(s.get(at..).unwrap_or_default());
    if n == 0 {
        return (0, 0);
    }
    let value = if overflow {
        u64::MAX
    } else if neg {
        mag.wrapping_neg()
    } else {
        mag
    };
    (value, at.saturating_add(n))
}

/// [`strtoul`]'s value, and whether it overflowed -- the one case in which
/// glibc's sets `errno`, which `simple_nextpid` checks.
#[must_use]
pub fn strtoul_overflow(s: &[u8]) -> (u64, bool) {
    let (at, _) = prefix(s);
    let (_, overflow, _) = digits(s.get(at..).unwrap_or_default());
    (strtoul(s).0, overflow)
}

/// C's conversion of a `long` to `int`: the low 32 bits, two's complement.
#[must_use]
pub fn low_i32(v: i64) -> i32 {
    let b = v.to_le_bytes();
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// C's conversion of a `long` to a 32-bit unsigned type (`uid_t`, `gid_t`).
#[must_use]
pub fn low_u32(v: i64) -> u32 {
    let b = v.to_le_bytes();
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// C's conversion of an `int` to `unsigned short`: the low 16 bits.
#[must_use]
pub fn low_u16(v: i32) -> u16 {
    let b = v.to_le_bytes();
    u16::from_le_bytes([b[0], b[1]])
}

/// C's conversion of a `long` to `unsigned long`: the same 64 bits.
#[must_use]
pub fn as_ulong(v: i64) -> u64 {
    u64::from_le_bytes(v.to_le_bytes())
}

/// `atoi`: glibc's is `(int) strtol (s, NULL, 10)`.
#[must_use]
pub fn atoi(s: &[u8]) -> i32 {
    low_i32(strtol(s).0)
}

/// A `sscanf` in progress over a C string, one directive at a time.
///
/// Each conversion method is one `%` directive; each returns `None` once the
/// scan has failed -- this conversion or an earlier one -- which is the point
/// where `sscanf` returns and every later destination keeps its old value.
/// So a caller assigns only what comes back `Some`.
pub struct Scan<'a> {
    s: &'a [u8],
    at: usize,
    failed: bool,
}

impl<'a> Scan<'a> {
    /// Start scanning `s`, which ends at its first NUL if it has one.
    #[must_use]
    pub fn new(s: &'a [u8]) -> Self {
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        Self {
            s: s.get(..end).unwrap_or_default(),
            at: 0,
            failed: false,
        }
    }

    fn rest(&self) -> &'a [u8] {
        self.s.get(self.at..).unwrap_or_default()
    }

    fn fail<T>(&mut self) -> Option<T> {
        self.failed = true;
        None
    }

    /// A blank in the format: any amount of whitespace, none included.
    pub fn ws(&mut self) {
        if !self.failed {
            let n = self.rest().iter().take_while(|&&b| is_space(b)).count();
            self.at = self.at.saturating_add(n);
        }
    }

    /// `%c`: the next byte, whitespace included.
    pub fn ch(&mut self) -> Option<u8> {
        if self.failed {
            return None;
        }
        match self.rest().first() {
            Some(&c) => {
                self.at = self.at.saturating_add(1);
                Some(c)
            }
            None => self.fail(),
        }
    }

    /// The text of one integer conversion -- whitespace skipped, then a sign
    /// and digits -- or a failure if there are no digits. glibc collects
    /// exactly this and hands it to `strtol`/`strtoul`.
    fn number(&mut self) -> Option<&'a [u8]> {
        if self.failed {
            return None;
        }
        self.ws();
        let rest = self.rest();
        let sign = usize::from(matches!(rest.first(), Some(b'-' | b'+')));
        let n = rest
            .get(sign..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if n == 0 {
            return self.fail();
        }
        let len = sign.saturating_add(n);
        self.at = self.at.saturating_add(len);
        rest.get(..len)
    }

    /// `%d`: `strtol`'s value, cut to `int`.
    pub fn int(&mut self) -> Option<i32> {
        self.number().map(|t| low_i32(strtol(t).0))
    }

    /// `%lu` and `%llu` (one width on x86-64): `strtoul`'s value.
    pub fn ulong(&mut self) -> Option<u64> {
        self.number().map(|t| strtoul(t).0)
    }

    /// `%*s`: one whitespace-delimited word, discarded.
    pub fn skip_word(&mut self) -> Option<()> {
        if self.failed {
            return None;
        }
        self.ws();
        let n = self.rest().iter().take_while(|&&b| !is_space(b)).count();
        if n == 0 {
            return self.fail();
        }
        self.at = self.at.saturating_add(n);
        Some(())
    }

    /// `%*u`: one number, discarded.
    pub fn skip_uint(&mut self) -> Option<()> {
        self.number().map(|_| ())
    }

    /// A literal in the format, which must match the input byte for byte.
    pub fn lit(&mut self, text: &[u8]) -> Option<()> {
        if self.failed {
            return None;
        }
        for &want in text {
            if want == b' ' {
                self.ws();
                continue;
            }
            match self.rest().first() {
                Some(&c) if c == want => self.at = self.at.saturating_add(1),
                _ => return self.fail(),
            }
        }
        Some(())
    }
}

/// `strtoull(s, &end, 16)`: the value, how many bytes it took, and whether
/// it overflowed -- glibc's `ERANGE`, after which the value is `ULLONG_MAX`.
///
/// Leading spaces and a sign are allowed, and a `0x` or `0X` before the
/// digits. A `0x` with no hexadecimal digit after it is glibc's one special
/// case: the `0` is the number, and the end is left pointing at the `x` --
/// so `0x` and `0xg` take one byte, not zero and not two. With no digits at
/// all nothing is taken, the sign and spaces included. A minus sign negates
/// in `unsigned long long`, as for [`strtoul`].
#[must_use]
pub fn strtoull_hex(s: &[u8]) -> (u64, usize, bool) {
    let (mut at, neg) = prefix(s);
    let rest = s.get(at..).unwrap_or_default();
    let hex_prefix = matches!(rest, [b'0', b'x' | b'X', ..]);
    if hex_prefix {
        at = at.saturating_add(2);
    }
    let mut mag: u64 = 0;
    let mut overflow = false;
    let mut n = 0usize;
    for &d in s.get(at..).unwrap_or_default() {
        let v = match d {
            b'0'..=b'9' => d.wrapping_sub(b'0'),
            b'a'..=b'f' => d.wrapping_sub(b'a').wrapping_add(10),
            b'A'..=b'F' => d.wrapping_sub(b'A').wrapping_add(10),
            _ => break,
        };
        match mag
            .checked_mul(16)
            .and_then(|m| m.checked_add(u64::from(v)))
        {
            Some(m) => mag = m,
            None => overflow = true,
        }
        n = n.saturating_add(1);
    }
    if n == 0 {
        // No digits: `noconv`. After a `0x`, the `0` was the number.
        return if hex_prefix {
            (0, at.saturating_sub(1), false)
        } else {
            (0, 0, false)
        };
    }
    let value = if overflow {
        u64::MAX
    } else if neg {
        mag.wrapping_neg()
    } else {
        mag
    };
    (value, at.saturating_add(n), overflow)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn strtoull_hex_reads_what_glibc_reads() {
        assert_eq!(strtoull_hex(b"00000000000004a3"), (0x4a3, 16, false));
        assert_eq!(strtoull_hex(b"FFFFFFFFFFFFFFFF"), (u64::MAX, 16, false));
        assert_eq!(strtoull_hex(b"1FFFFFFFFFFFFFFFF"), (u64::MAX, 17, true));
        assert_eq!(strtoull_hex(b"0x1f"), (0x1f, 4, false));
        assert_eq!(strtoull_hex(b" -1"), (u64::MAX, 3, false));
        assert_eq!(strtoull_hex(b"12\nSigBlk"), (0x12, 2, false));
        // The `0x` special case, and no number at all.
        assert_eq!(strtoull_hex(b"0x"), (0, 1, false));
        assert_eq!(strtoull_hex(b"0xg"), (0, 1, false));
        assert_eq!(strtoull_hex(b"-"), (0, 0, false));
        assert_eq!(strtoull_hex(b""), (0, 0, false));
        assert_eq!(strtoull_hex(b"g"), (0, 0, false));
    }

    #[test]
    fn strtol_reads_what_glibc_reads() {
        assert_eq!(strtol(b"42"), (42, 2));
        assert_eq!(strtol(b"  -7x"), (-7, 4));
        assert_eq!(strtol(b"\t+3"), (3, 3));
        assert_eq!(strtol(b"x"), (0, 0));
        assert_eq!(strtol(b"-"), (0, 0));
        assert_eq!(strtol(b""), (0, 0));
        // Saturation, both ways.
        assert_eq!(strtol(b"99999999999999999999").0, i64::MAX);
        assert_eq!(strtol(b"-99999999999999999999").0, i64::MIN);
        assert_eq!(strtol(b"-9223372036854775808").0, i64::MIN);
        assert_eq!(strtol(b"9223372036854775808").0, i64::MAX);
    }

    #[test]
    fn strtoul_negates_in_unsigned_and_saturates_on_magnitude() {
        assert_eq!(strtoul(b"5"), (5, 1));
        assert_eq!(strtoul(b"-1"), (u64::MAX, 2));
        assert_eq!(strtoul(b"-5").0, u64::MAX - 4);
        assert_eq!(strtoul(b"18446744073709551615").0, u64::MAX);
        assert_eq!(strtoul(b"18446744073709551616").0, u64::MAX);
        assert_eq!(strtoul(b"-18446744073709551616").0, u64::MAX);
        assert_eq!(strtoul(b"abc"), (0, 0));
    }

    #[test]
    fn int_keeps_the_low_32_bits() {
        assert_eq!(low_i32(4_294_967_297), 1);
        assert_eq!(low_i32(2_147_483_648), i32::MIN);
        // `LONG_MAX`, which an overflowing `%d` produces, is -1 as an `int`.
        assert_eq!(low_i32(i64::MAX), -1);
        assert_eq!(atoi(b" 12abc"), 12);
        assert_eq!(atoi(b"4294967297"), 1);
        assert_eq!(low_u32(-1), u32::MAX);
        assert_eq!(as_ulong(-5), u64::MAX - 4);
    }

    #[test]
    fn a_failed_conversion_ends_the_scan() {
        let mut s = Scan::new(b"R 12 abc 7");
        assert_eq!(s.ch(), Some(b'R'));
        s.ws();
        assert_eq!(s.int(), Some(12));
        assert_eq!(s.int(), None);
        // Everything after a failure fails, even what would have parsed.
        assert_eq!(s.int(), None);
        assert_eq!(s.ch(), None);
    }

    #[test]
    fn the_scan_stops_at_the_first_nul() {
        let mut s = Scan::new(b"1 2\x003");
        assert_eq!(s.int(), Some(1));
        assert_eq!(s.int(), Some(2));
        assert_eq!(s.int(), None);
    }

    #[test]
    fn words_and_discarded_numbers() {
        let mut s = Scan::new(b"  abc 12 x");
        assert_eq!(s.skip_word(), Some(()));
        assert_eq!(s.skip_uint(), Some(()));
        assert_eq!(s.skip_uint(), None);
        let mut e = Scan::new(b"   ");
        assert_eq!(e.skip_word(), None);
    }

    #[test]
    fn literals_match_with_blanks_as_any_whitespace() {
        let mut s = Scan::new(b"rchar: 5\nwchar:  6");
        assert_eq!(s.lit(b"rchar: "), Some(()));
        assert_eq!(s.ulong(), Some(5));
        assert_eq!(s.lit(b" wchar: "), Some(()));
        assert_eq!(s.ulong(), Some(6));
        let mut t = Scan::new(b"rchar 5");
        assert_eq!(t.lit(b"rchar: "), None);
    }

    #[test]
    fn a_sign_alone_is_no_number() {
        let mut s = Scan::new(b"- 5");
        assert_eq!(s.int(), None);
        let mut u = Scan::new(b"+5 -3");
        assert_eq!(u.int(), Some(5));
        assert_eq!(u.ulong(), Some(u64::MAX - 2));
    }
}
