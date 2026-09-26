//! The lexer: upstream's `yylex`, `lookup_word` and `lookup_zone`, and the
//! word tables they search.
//!
//! Everything here is ASCII-only on purpose, as upstream's `c_isalpha` and
//! `c_toupper` are: a date string is not localised, and a byte outside ASCII
//! is a token the grammar has no use for, which makes it a syntax error rather
//! than a letter of some word.

use super::grammar::{Value, sym};
use super::{BILLION, LOG10_BILLION, ParserControl, TextInt, Timespec};

/// An entry in a lexical lookup table: upstream's `table`, with the token
/// already translated to its grammar symbol.
#[derive(Clone, Copy)]
pub(super) struct Entry {
    name: &'static [u8],
    sym: u8,
    value: i64,
}

const fn e(name: &'static [u8], sym: u8, value: i64) -> Entry {
    Entry { name, sym, value }
}

/// `HOUR (x)`.
const fn hour(x: i64) -> i64 {
    x.saturating_mul(60 * 60)
}

/// `MERam`, `MERpm`, `MER24`: upstream's meridian styles.
pub(super) const MER_AM: i64 = 0;
pub(super) const MER_PM: i64 = 1;
pub(super) const MER_24: i64 = 2;

const MERIDIAN_TABLE: &[Entry] = &[
    e(b"AM", sym::T_MERIDIAN, MER_AM),
    e(b"A.M.", sym::T_MERIDIAN, MER_AM),
    e(b"PM", sym::T_MERIDIAN, MER_PM),
    e(b"P.M.", sym::T_MERIDIAN, MER_PM),
];

const DST_ENTRY: Entry = e(b"DST", sym::T_DST, 0);

const MONTH_AND_DAY_TABLE: &[Entry] = &[
    e(b"JANUARY", sym::T_MONTH, 1),
    e(b"FEBRUARY", sym::T_MONTH, 2),
    e(b"MARCH", sym::T_MONTH, 3),
    e(b"APRIL", sym::T_MONTH, 4),
    e(b"MAY", sym::T_MONTH, 5),
    e(b"JUNE", sym::T_MONTH, 6),
    e(b"JULY", sym::T_MONTH, 7),
    e(b"AUGUST", sym::T_MONTH, 8),
    e(b"SEPTEMBER", sym::T_MONTH, 9),
    e(b"SEPT", sym::T_MONTH, 9),
    e(b"OCTOBER", sym::T_MONTH, 10),
    e(b"NOVEMBER", sym::T_MONTH, 11),
    e(b"DECEMBER", sym::T_MONTH, 12),
    e(b"SUNDAY", sym::T_DAY, 0),
    e(b"MONDAY", sym::T_DAY, 1),
    e(b"TUESDAY", sym::T_DAY, 2),
    e(b"TUES", sym::T_DAY, 2),
    e(b"WEDNESDAY", sym::T_DAY, 3),
    e(b"WEDNES", sym::T_DAY, 3),
    e(b"THURSDAY", sym::T_DAY, 4),
    e(b"THUR", sym::T_DAY, 4),
    e(b"THURS", sym::T_DAY, 4),
    e(b"FRIDAY", sym::T_DAY, 5),
    e(b"SATURDAY", sym::T_DAY, 6),
];

const TIME_UNITS_TABLE: &[Entry] = &[
    e(b"YEAR", sym::T_YEAR_UNIT, 1),
    e(b"MONTH", sym::T_MONTH_UNIT, 1),
    e(b"FORTNIGHT", sym::T_DAY_UNIT, 14),
    e(b"WEEK", sym::T_DAY_UNIT, 7),
    e(b"DAY", sym::T_DAY_UNIT, 1),
    e(b"HOUR", sym::T_HOUR_UNIT, 1),
    e(b"MINUTE", sym::T_MINUTE_UNIT, 1),
    e(b"MIN", sym::T_MINUTE_UNIT, 1),
    e(b"SECOND", sym::T_SEC_UNIT, 1),
    e(b"SEC", sym::T_SEC_UNIT, 1),
];

/// Assorted relative-time words. Upstream comments `SECOND` out as an ordinal
/// (it is the unit), so there is no way to say "second Tuesday".
const RELATIVE_TIME_TABLE: &[Entry] = &[
    e(b"TOMORROW", sym::T_DAY_SHIFT, 1),
    e(b"YESTERDAY", sym::T_DAY_SHIFT, -1),
    e(b"TODAY", sym::T_DAY_SHIFT, 0),
    e(b"NOW", sym::T_DAY_SHIFT, 0),
    e(b"LAST", sym::T_ORDINAL, -1),
    e(b"THIS", sym::T_ORDINAL, 0),
    e(b"NEXT", sym::T_ORDINAL, 1),
    e(b"FIRST", sym::T_ORDINAL, 1),
    e(b"THIRD", sym::T_ORDINAL, 3),
    e(b"FOURTH", sym::T_ORDINAL, 4),
    e(b"FIFTH", sym::T_ORDINAL, 5),
    e(b"SIXTH", sym::T_ORDINAL, 6),
    e(b"SEVENTH", sym::T_ORDINAL, 7),
    e(b"EIGHTH", sym::T_ORDINAL, 8),
    e(b"NINTH", sym::T_ORDINAL, 9),
    e(b"TENTH", sym::T_ORDINAL, 10),
    e(b"ELEVENTH", sym::T_ORDINAL, 11),
    e(b"TWELFTH", sym::T_ORDINAL, 12),
    e(b"AGO", sym::T_AGO, -1),
    e(b"HENCE", sym::T_AGO, 1),
];

/// Zones valid even for times they would not otherwise name — `GMT` in a
/// London summer.
const UNIVERSAL_TIME_ZONE_TABLE: &[Entry] = &[
    e(b"GMT", sym::T_ZONE, hour(0)),
    e(b"UT", sym::T_ZONE, hour(0)),
    e(b"UTC", sym::T_ZONE, hour(0)),
];

/// Upstream's zone abbreviations. Necessarily incomplete and ambiguous (an
/// Australian's `EST` is not this one), which is why the local zone's own
/// abbreviations are searched before these.
const TIME_ZONE_TABLE: &[Entry] = &[
    e(b"WET", sym::T_ZONE, hour(0)),
    e(b"WEST", sym::T_DAYZONE, hour(0)),
    e(b"BST", sym::T_DAYZONE, hour(0)),
    e(b"ART", sym::T_ZONE, -hour(3)),
    e(b"BRT", sym::T_ZONE, -hour(3)),
    e(b"BRST", sym::T_DAYZONE, -hour(3)),
    e(b"NST", sym::T_ZONE, -(hour(3) + 30 * 60)),
    e(b"NDT", sym::T_DAYZONE, -(hour(3) + 30 * 60)),
    e(b"AST", sym::T_ZONE, -hour(4)),
    e(b"ADT", sym::T_DAYZONE, -hour(4)),
    e(b"CLT", sym::T_ZONE, -hour(4)),
    e(b"CLST", sym::T_DAYZONE, -hour(4)),
    e(b"EST", sym::T_ZONE, -hour(5)),
    e(b"EDT", sym::T_DAYZONE, -hour(5)),
    e(b"CST", sym::T_ZONE, -hour(6)),
    e(b"CDT", sym::T_DAYZONE, -hour(6)),
    e(b"MST", sym::T_ZONE, -hour(7)),
    e(b"MDT", sym::T_DAYZONE, -hour(7)),
    e(b"PST", sym::T_ZONE, -hour(8)),
    e(b"PDT", sym::T_DAYZONE, -hour(8)),
    e(b"AKST", sym::T_ZONE, -hour(9)),
    e(b"AKDT", sym::T_DAYZONE, -hour(9)),
    e(b"HST", sym::T_ZONE, -hour(10)),
    e(b"HAST", sym::T_ZONE, -hour(10)),
    e(b"HADT", sym::T_DAYZONE, -hour(10)),
    e(b"SST", sym::T_ZONE, -hour(12)),
    e(b"WAT", sym::T_ZONE, hour(1)),
    e(b"CET", sym::T_ZONE, hour(1)),
    e(b"CEST", sym::T_DAYZONE, hour(1)),
    e(b"MET", sym::T_ZONE, hour(1)),
    e(b"MEZ", sym::T_ZONE, hour(1)),
    e(b"MEST", sym::T_DAYZONE, hour(1)),
    e(b"MESZ", sym::T_DAYZONE, hour(1)),
    e(b"EET", sym::T_ZONE, hour(2)),
    e(b"EEST", sym::T_DAYZONE, hour(2)),
    e(b"CAT", sym::T_ZONE, hour(2)),
    e(b"SAST", sym::T_ZONE, hour(2)),
    e(b"EAT", sym::T_ZONE, hour(3)),
    e(b"MSK", sym::T_ZONE, hour(3)),
    e(b"MSD", sym::T_DAYZONE, hour(3)),
    e(b"IST", sym::T_ZONE, hour(5) + 30 * 60),
    e(b"SGT", sym::T_ZONE, hour(8)),
    e(b"KST", sym::T_ZONE, hour(9)),
    e(b"JST", sym::T_ZONE, hour(9)),
    e(b"GST", sym::T_ZONE, hour(10)),
    e(b"NZST", sym::T_ZONE, hour(12)),
    e(b"NZDT", sym::T_DAYZONE, hour(12)),
];

/// The military zones, the right way round (RFC 822 had them backwards).
/// `J` is local time and `T` is also ISO 8601's date/time separator, so both
/// are tokens of their own rather than zones.
const MILITARY_TABLE: &[Entry] = &[
    e(b"A", sym::T_ZONE, hour(1)),
    e(b"B", sym::T_ZONE, hour(2)),
    e(b"C", sym::T_ZONE, hour(3)),
    e(b"D", sym::T_ZONE, hour(4)),
    e(b"E", sym::T_ZONE, hour(5)),
    e(b"F", sym::T_ZONE, hour(6)),
    e(b"G", sym::T_ZONE, hour(7)),
    e(b"H", sym::T_ZONE, hour(8)),
    e(b"I", sym::T_ZONE, hour(9)),
    e(b"J", sym::J, 0),
    e(b"K", sym::T_ZONE, hour(10)),
    e(b"L", sym::T_ZONE, hour(11)),
    e(b"M", sym::T_ZONE, hour(12)),
    e(b"N", sym::T_ZONE, -hour(1)),
    e(b"O", sym::T_ZONE, -hour(2)),
    e(b"P", sym::T_ZONE, -hour(3)),
    e(b"Q", sym::T_ZONE, -hour(4)),
    e(b"R", sym::T_ZONE, -hour(5)),
    e(b"S", sym::T_ZONE, -hour(6)),
    e(b"T", sym::T, 0),
    e(b"U", sym::T_ZONE, -hour(8)),
    e(b"V", sym::T_ZONE, -hour(9)),
    e(b"W", sym::T_ZONE, -hour(10)),
    e(b"X", sym::T_ZONE, -hour(11)),
    e(b"Y", sym::T_ZONE, -hour(12)),
    e(b"Z", sym::T_ZONE, hour(0)),
];

/// Upstream's `char buff[20]`: a word is looked up by at most its first
/// nineteen bytes, although all of it is consumed.
const WORD_MAX: usize = 19;

/// `c_isspace`: the C locale's six.
pub(super) fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// What the lexer returns for a table entry: its symbol and its value.
fn found(entry: &Entry) -> (u8, i64) {
    (entry.sym, entry.value)
}

/// `lookup_zone`: the universal zones, then the local zone's own
/// abbreviations, then upstream's table.
fn lookup_zone(pc: &ParserControl<'_, '_>, name: &[u8]) -> Option<(u8, i64)> {
    if let Some(entry) = UNIVERSAL_TIME_ZONE_TABLE.iter().find(|t| t.name == name) {
        return Some(found(entry));
    }
    // The local abbreviations first, as they are more likely to be right.
    if let Some(local) = pc.local_time_zone_table.iter().find(|z| z.name == name) {
        return Some((sym::T_LOCAL_ZONE, local.isdst));
    }
    TIME_ZONE_TABLE.iter().find(|t| t.name == name).map(found)
}

/// `lookup_word`: uppercase `word` in place and find what it names.
///
/// `word` is left as upstream leaves `buff` — uppercased, and with its periods
/// squeezed out if the search got that far — because the "unknown word"
/// diagnostic prints it in that state.
fn lookup_word(pc: &ParserControl<'_, '_>, word: &mut Vec<u8>) -> Option<(u8, i64)> {
    word.make_ascii_uppercase();

    if let Some(entry) = MERIDIAN_TABLE.iter().find(|t| t.name == word.as_slice()) {
        return Some(found(entry));
    }

    // A three-letter word, or a four-letter one ending in a period, is an
    // abbreviation of a month or day name.
    let wordlen = word.len();
    let abbrev = wordlen == 3 || (wordlen == 4 && word.get(3) == Some(&b'.'));
    let hit = MONTH_AND_DAY_TABLE.iter().find(|t| {
        if abbrev {
            word.get(..3) == t.name.get(..3)
        } else {
            t.name == word.as_slice()
        }
    });
    if let Some(entry) = hit {
        return Some(found(entry));
    }

    if let Some(hit) = lookup_zone(pc, word) {
        return Some(hit);
    }

    if word.as_slice() == DST_ENTRY.name {
        return Some(found(&DST_ENTRY));
    }

    if let Some(entry) = TIME_UNITS_TABLE.iter().find(|t| t.name == word.as_slice()) {
        return Some(found(entry));
    }

    // Strip a plural and try the units again.
    if let Some((&b'S', singular)) = word.split_last()
        && let Some(entry) = TIME_UNITS_TABLE.iter().find(|t| t.name == singular)
    {
        return Some(found(entry));
    }

    if let Some(entry) = RELATIVE_TIME_TABLE
        .iter()
        .find(|t| t.name == word.as_slice())
    {
        return Some(found(entry));
    }

    // The military zones are single letters.
    if let [letter] = word.as_slice()
        && let Some(entry) = MILITARY_TABLE
            .iter()
            .find(|t| t.name.first() == Some(letter))
    {
        return Some(found(entry));
    }

    // Drop any periods and try the zones again.
    let period_found = word.contains(&b'.');
    word.retain(|&c| c != b'.');
    if period_found {
        return lookup_zone(pc, word);
    }
    None
}

/// The byte at `i`, or NUL past the end — the C string's terminator, which is
/// what every read past the end of upstream's input sees.
fn at(input: &[u8], i: usize) -> u8 {
    input.get(i).copied().unwrap_or(0)
}

/// The value of an ASCII digit.
fn digit(c: u8) -> i64 {
    i64::from(c.wrapping_sub(b'0'))
}

/// `yylex`: the next token, as its grammar symbol and semantic value.
///
/// `pc.pos` is upstream's `pc->input`, and it moves exactly as that pointer
/// does — including where it is left on an error, since `--debug` reports
/// "stopped at" whatever follows it.
#[allow(clippy::too_many_lines, reason = "one upstream function, ported whole")]
pub(super) fn yylex(pc: &mut ParserControl<'_, '_>) -> (u8, Value) {
    let input = pc.input;
    loop {
        while c_isspace(at(input, pc.pos)) {
            pc.pos = pc.pos.saturating_add(1);
        }
        let mut c = at(input, pc.pos);

        if c.is_ascii_digit() || c == b'-' || c == b'+' {
            let mut p = pc.pos;
            let sign: i64;
            if c == b'-' || c == b'+' {
                sign = if c == b'-' { -1 } else { 1 };
                // Skip the sign and any blanks after it; `pc->input` follows.
                loop {
                    p = p.saturating_add(1);
                    pc.pos = p;
                    c = at(input, p);
                    if !c_isspace(c) {
                        break;
                    }
                }
                if !c.is_ascii_digit() {
                    // A sign with no number after it is skipped entirely.
                    continue;
                }
            } else {
                sign = 0;
            }

            // Accumulate toward the sign, so the most negative value fits.
            let mut value: i64 = 0;
            loop {
                let d = digit(c);
                let next = value.checked_mul(10).and_then(|v| {
                    if sign < 0 {
                        v.checked_sub(d)
                    } else {
                        v.checked_add(d)
                    }
                });
                let Some(next) = next else {
                    return (sym::YYUNDEF, Value::None);
                };
                value = next;
                p = p.saturating_add(1);
                c = at(input, p);
                if !c.is_ascii_digit() {
                    break;
                }
            }

            if (c == b'.' || c == b',') && at(input, p.saturating_add(1)).is_ascii_digit() {
                let mut s = value;

                // Accumulate the fraction, to nanosecond precision.
                p = p.saturating_add(1);
                let mut ns = i32::from(at(input, p).wrapping_sub(b'0'));
                p = p.saturating_add(1);
                for _ in 2..=LOG10_BILLION {
                    ns = ns.saturating_mul(10);
                    let d = at(input, p);
                    if d.is_ascii_digit() {
                        ns = ns.saturating_add(i32::from(d.wrapping_sub(b'0')));
                        p = p.saturating_add(1);
                    }
                }

                // Skip excess digits, truncating toward -Infinity.
                if sign < 0 {
                    while at(input, p).is_ascii_digit() {
                        if at(input, p) != b'0' {
                            ns = ns.saturating_add(1);
                            break;
                        }
                        p = p.saturating_add(1);
                    }
                }
                while at(input, p).is_ascii_digit() {
                    p = p.saturating_add(1);
                }

                // The timespec convention: tv_nsec is a positive offset even
                // when tv_sec is negative.
                if sign < 0 && ns != 0 {
                    let Some(prev) = s.checked_sub(1) else {
                        return (sym::YYUNDEF, Value::None);
                    };
                    s = prev;
                    ns = BILLION.saturating_sub(ns);
                }

                pc.pos = p;
                let token = if sign != 0 {
                    sym::T_SDECIMAL_NUMBER
                } else {
                    sym::T_UDECIMAL_NUMBER
                };
                return (
                    token,
                    Value::Timespec(Timespec {
                        tv_sec: s,
                        tv_nsec: ns,
                    }),
                );
            }

            let digits = i64::try_from(p.saturating_sub(pc.pos)).unwrap_or(i64::MAX);
            pc.pos = p;
            let token = if sign != 0 {
                sym::T_SNUMBER
            } else {
                sym::T_UNUMBER
            };
            return (
                token,
                Value::TextInt(TextInt {
                    negative: sign < 0,
                    value,
                    digits,
                }),
            );
        }

        if c.is_ascii_alphabetic() {
            let mut buff = Vec::with_capacity(WORD_MAX);
            loop {
                if buff.len() < WORD_MAX {
                    buff.push(c);
                }
                pc.pos = pc.pos.saturating_add(1);
                c = at(input, pc.pos);
                if !(c.is_ascii_alphabetic() || c == b'.') {
                    break;
                }
            }
            return match lookup_word(pc, &mut buff) {
                Some((token, value)) => (token, Value::Int(value)),
                None => {
                    if pc.debugging() {
                        // The word as `lookup_word` left it: bytes, not text.
                        let mut msg = b"error: unknown word '".to_vec();
                        msg.extend_from_slice(&buff);
                        msg.extend_from_slice(b"'\n");
                        pc.dbg_printf(&msg);
                    }
                    (sym::YYUNDEF, Value::None)
                }
            };
        }

        if c != b'(' {
            pc.pos = pc.pos.saturating_add(1);
            return (sym::translate_char(c), Value::None);
        }

        // A parenthesised comment, which may nest.
        let mut count: i64 = 0;
        loop {
            c = at(input, pc.pos);
            pc.pos = pc.pos.saturating_add(1);
            if c == 0 {
                return (sym::YYEOF, Value::None);
            }
            if c == b'(' {
                count = count.saturating_add(1);
            } else if c == b')' {
                count = count.saturating_sub(1);
            }
            if count == 0 {
                break;
            }
        }
    }
}
