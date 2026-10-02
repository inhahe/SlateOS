//! libmagic's `apprentice.c`: reading magic databases -- the text form, line
//! by line, and the compiled `.mgc` form -- into rules sorted as libmagic
//! sorts them, and writing the compiled form back out (`-C`).
//!
//! Function for function, with upstream's names. What libmagic does on its way
//! through a line is what this does, quirks included where they change a
//! result: a 63-character description counts as truncated; an offset is
//! `strtol` cut to 32 bits; a numeric value that overflows `strtoull` leaves
//! the cursor where it was; only an entry's first line decides whether it
//! tests binary data or text (`set_text_binary` means to walk the rest and
//! does not); the strength sort is glibc's `qsort`, a stable merge sort.
//!
//! # The built-in database
//!
//! Upstream reads its default database from a compiled `magic.mgc` installed
//! next to it. This program carries that database's source instead
//! ([`crate::magdir`], vendored from the release), and reads it where upstream
//! would read the file: when the default path is asked for and nothing is
//! installed there. It is parsed as upstream's build compiles it -- the
//! checks on, the warnings printed then and not now.

use std::rc::Rc;

use crate::cstd::{cstr, cstrlen, hexval, isalpha, isdigit, isspace, strtol_i32, strtoul, strtoull};
use crate::encoding::file_looks_utf8;
use crate::funcs::{MAGIC_SETS, MList, Ms, RegFlags, file_printable, file_regcomp};
use crate::magic::*;

/// `FILE_LOAD`, `FILE_CHECK`, `FILE_COMPILE` and `FILE_LIST`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Load,
    Check,
    Compile,
    List,
}

/// The default database's path, `MAGIC`: where an installed `magic.mgc` (or
/// a directory of magic) is looked for, and what the built-in database stands
/// in for when there is none.
pub const MAGIC: &str = "/usr/share/misc/magic";

/// `PATHSEP`: between the files of a magic path.
const PATHSEP: u8 = b':';

/// `usg_hdr`: what `-c` prints first.
const USG_HDR: &[u8] = b"cont\toffset\ttype\topcode\tmask\tvalue\tdesc";

// ---- the type table -------------------------------------------------------------

struct TypeTbl {
    name: &'static [u8],
    typ: u8,
    format: u8,
}

const fn t(name: &'static [u8], typ: u8, format: u8) -> TypeTbl {
    TypeTbl { name, typ, format }
}

/// `type_tbl`, in upstream's order: a type is found by the first entry that is
/// a prefix of the text, so the order is part of the grammar. It is *not* in
/// type-number order everywhere -- `leid3` (40) comes before `beid3` (39) --
/// which upstream's messages that index it by number repeat.
static TYPE_TBL: &[TypeTbl] = &[
    t(b"invalid", FILE_INVALID, FILE_FMT_NONE),
    t(b"byte", FILE_BYTE, FILE_FMT_NUM),
    t(b"short", FILE_SHORT, FILE_FMT_NUM),
    t(b"default", FILE_DEFAULT, FILE_FMT_NONE),
    t(b"long", FILE_LONG, FILE_FMT_NUM),
    t(b"string", FILE_STRING, FILE_FMT_STR),
    t(b"date", FILE_DATE, FILE_FMT_STR),
    t(b"beshort", FILE_BESHORT, FILE_FMT_NUM),
    t(b"belong", FILE_BELONG, FILE_FMT_NUM),
    t(b"bedate", FILE_BEDATE, FILE_FMT_STR),
    t(b"leshort", FILE_LESHORT, FILE_FMT_NUM),
    t(b"lelong", FILE_LELONG, FILE_FMT_NUM),
    t(b"ledate", FILE_LEDATE, FILE_FMT_STR),
    t(b"pstring", FILE_PSTRING, FILE_FMT_STR),
    t(b"ldate", FILE_LDATE, FILE_FMT_STR),
    t(b"beldate", FILE_BELDATE, FILE_FMT_STR),
    t(b"leldate", FILE_LELDATE, FILE_FMT_STR),
    t(b"regex", FILE_REGEX, FILE_FMT_STR),
    t(b"bestring16", FILE_BESTRING16, FILE_FMT_STR),
    t(b"lestring16", FILE_LESTRING16, FILE_FMT_STR),
    t(b"search", FILE_SEARCH, FILE_FMT_STR),
    t(b"medate", FILE_MEDATE, FILE_FMT_STR),
    t(b"meldate", FILE_MELDATE, FILE_FMT_STR),
    t(b"melong", FILE_MELONG, FILE_FMT_NUM),
    t(b"quad", FILE_QUAD, FILE_FMT_QUAD),
    t(b"lequad", FILE_LEQUAD, FILE_FMT_QUAD),
    t(b"bequad", FILE_BEQUAD, FILE_FMT_QUAD),
    t(b"qdate", FILE_QDATE, FILE_FMT_STR),
    t(b"leqdate", FILE_LEQDATE, FILE_FMT_STR),
    t(b"beqdate", FILE_BEQDATE, FILE_FMT_STR),
    t(b"qldate", FILE_QLDATE, FILE_FMT_STR),
    t(b"leqldate", FILE_LEQLDATE, FILE_FMT_STR),
    t(b"beqldate", FILE_BEQLDATE, FILE_FMT_STR),
    t(b"float", FILE_FLOAT, FILE_FMT_FLOAT),
    t(b"befloat", FILE_BEFLOAT, FILE_FMT_FLOAT),
    t(b"lefloat", FILE_LEFLOAT, FILE_FMT_FLOAT),
    t(b"double", FILE_DOUBLE, FILE_FMT_DOUBLE),
    t(b"bedouble", FILE_BEDOUBLE, FILE_FMT_DOUBLE),
    t(b"ledouble", FILE_LEDOUBLE, FILE_FMT_DOUBLE),
    t(b"leid3", FILE_LEID3, FILE_FMT_NUM),
    t(b"beid3", FILE_BEID3, FILE_FMT_NUM),
    t(b"indirect", FILE_INDIRECT, FILE_FMT_NUM),
    t(b"qwdate", FILE_QWDATE, FILE_FMT_STR),
    t(b"leqwdate", FILE_LEQWDATE, FILE_FMT_STR),
    t(b"beqwdate", FILE_BEQWDATE, FILE_FMT_STR),
    t(b"name", FILE_NAME, FILE_FMT_NONE),
    t(b"use", FILE_USE, FILE_FMT_NONE),
    t(b"clear", FILE_CLEAR, FILE_FMT_NONE),
    t(b"der", FILE_DER, FILE_FMT_STR),
    t(b"guid", FILE_GUID, FILE_FMT_STR),
    t(b"offset", FILE_OFFSET, FILE_FMT_QUAD),
    t(b"bevarint", FILE_BEVARINT, FILE_FMT_STR),
    t(b"levarint", FILE_LEVARINT, FILE_FMT_STR),
    t(b"msdosdate", FILE_MSDOSDATE, FILE_FMT_STR),
    t(b"lemsdosdate", FILE_LEMSDOSDATE, FILE_FMT_STR),
    t(b"bemsdosdate", FILE_BEMSDOSDATE, FILE_FMT_STR),
    t(b"msdostime", FILE_MSDOSTIME, FILE_FMT_STR),
    t(b"lemsdostime", FILE_LEMSDOSTIME, FILE_FMT_STR),
    t(b"bemsdostime", FILE_BEMSDOSTIME, FILE_FMT_STR),
    t(b"octal", FILE_OCTAL, FILE_FMT_STR),
];

/// `special_tbl`: not types, and not to be prefixed with `u`.
static SPECIAL_TBL: &[TypeTbl] = &[
    t(b"der", FILE_DER, FILE_FMT_STR),
    t(b"name", FILE_NAME, FILE_FMT_STR),
    t(b"use", FILE_USE, FILE_FMT_STR),
    t(b"octal", FILE_OCTAL, FILE_FMT_STR),
];

/// `file_names[type]`: a type's name, by type number.
#[must_use]
pub fn file_name(typ: u8) -> &'static [u8] {
    TYPE_TBL.iter().find(|e| e.typ == typ).map_or(b"", |e| e.name)
}

/// `file_formats[type]`, by type number.
#[must_use]
pub fn file_format(typ: u8) -> u8 {
    TYPE_TBL
        .iter()
        .find(|e| e.typ == typ)
        .map_or(FILE_FMT_NONE, |e| e.format)
}

/// `type_tbl[type].name`: the table *position*, as two messages index it --
/// so type 39, `beid3`, is named `leid3` there.
fn type_tbl_name(typ: u8) -> &'static [u8] {
    TYPE_TBL.get(usize::from(typ)).map_or(b"", |e| e.name)
}

/// `get_type`: the first entry `l` starts with -- `"invalid"` included -- and
/// the text after it; `None` when none does, which leaves the cursor alone.
fn get_type<'a>(tbl: &[TypeTbl], l: &'a [u8]) -> Option<(u8, &'a [u8])> {
    tbl.iter()
        .find(|p| l.starts_with(p.name))
        .map(|p| (p.typ, l.get(p.name.len()..).unwrap_or_default()))
}

/// `get_standard_integer_type`: the SUS spellings -- `d`/`u` then `C S I L Q`
/// or `1 2 4 8`, or alone. `l` is at the `d` or `u`. `None` is
/// `FILE_INVALID`, which leaves the cursor alone.
fn get_standard_integer_type(l: &[u8]) -> Option<(u8, &[u8])> {
    let at = |i: usize| l.get(i).copied().unwrap_or(0);
    if isalpha(at(1)) {
        let typ = match at(1) {
            b'C' => FILE_BYTE,
            b'S' => FILE_SHORT,
            b'I' | b'L' => FILE_LONG,
            b'Q' => FILE_QUAD,
            _ => return None,
        };
        Some((typ, l.get(2..).unwrap_or_default()))
    } else if isdigit(at(1)) {
        if isdigit(at(2)) {
            return None;
        }
        let typ = match at(1) {
            b'1' => FILE_BYTE,
            b'2' => FILE_SHORT,
            b'4' => FILE_LONG,
            b'8' => FILE_QUAD,
            _ => return None,
        };
        Some((typ, l.get(2..).unwrap_or_default()))
    } else {
        Some((FILE_LONG, l.get(1..).unwrap_or_default()))
    }
}

// ---- strength ------------------------------------------------------------------------

/// `nonmagic`: how many characters of a regex count towards its strength.
fn nonmagic(s: &[u8]) -> usize {
    let at = |j: usize| s.get(j).copied().unwrap_or(0);
    let mut rv = 0usize;
    let mut i = 0usize;
    while at(i) != 0 {
        match at(i) {
            b'\\' => {
                i += 1;
                if at(i) == 0 {
                    i -= 1;
                }
                rv += 1;
            }
            b'?' | b'*' | b'.' | b'+' | b'^' | b'$' => {}
            b'[' => {
                while at(i) != 0 && at(i) != b']' {
                    i += 1;
                }
                i = i.wrapping_sub(1);
            }
            b'{' => {
                while at(i) != 0 && at(i) != b'}' {
                    i += 1;
                }
                if at(i) == 0 {
                    i -= 1;
                }
            }
            _ => rv += 1,
        }
        i = i.wrapping_add(1);
    }
    if rv == 0 { 1 } else { rv }
}

/// `typesize`: how many bytes a numeric type reads, or `None`
/// (`FILE_BADSIZE`).
#[must_use]
pub fn typesize(typ: u8) -> Option<usize> {
    Some(match typ {
        FILE_BYTE => 1,
        FILE_SHORT | FILE_LESHORT | FILE_BESHORT | FILE_MSDOSDATE | FILE_BEMSDOSDATE
        | FILE_LEMSDOSDATE | FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => 2,
        FILE_LONG | FILE_LELONG | FILE_BELONG | FILE_MELONG | FILE_DATE | FILE_LEDATE
        | FILE_BEDATE | FILE_MEDATE | FILE_LDATE | FILE_LELDATE | FILE_BELDATE | FILE_MELDATE
        | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT | FILE_BEID3 | FILE_LEID3 => 4,
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_QDATE | FILE_LEQDATE | FILE_BEQDATE
        | FILE_QLDATE | FILE_LEQLDATE | FILE_BEQLDATE | FILE_QWDATE | FILE_LEQWDATE
        | FILE_BEQWDATE | FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE | FILE_OFFSET
        | FILE_BEVARINT | FILE_LEVARINT => 8,
        FILE_GUID => 16,
        _ => return None,
    })
}

const MULT: i64 = 10;

/// `apprentice_magic_strength_1`.
fn magic_strength_1(m: &Magic) -> i64 {
    let mut val: i64 = 2 * MULT;
    let len = i64::from(m.vallen);
    match m.typ {
        FILE_DEFAULT => return 0,
        FILE_PSTRING | FILE_STRING | FILE_OCTAL => val += len * MULT,
        FILE_BESTRING16 | FILE_LESTRING16 => val += len * MULT / 2,
        FILE_SEARCH => {
            if len != 0 {
                val += len * (MULT / len).max(1);
            }
        }
        FILE_REGEX => {
            #[allow(clippy::cast_possible_wrap)]
            let v = nonmagic(m.value.s()) as i64;
            val += v * (MULT / v).max(1);
        }
        FILE_INDIRECT | FILE_NAME | FILE_USE | FILE_CLEAR => {}
        FILE_DER => val += MULT,
        t => {
            if let Some(ts) = typesize(t) {
                #[allow(clippy::cast_possible_wrap)]
                {
                    val += ts as i64 * MULT;
                }
            }
        }
    }
    match m.reln {
        b'x' | b'!' => val = 0,
        b'=' => val += MULT,
        b'>' | b'<' => val -= 2 * MULT,
        b'^' | b'&' => val -= MULT,
        _ => {}
    }
    val
}

/// `file_magic_strength`: the weight entries are sorted by.
#[must_use]
pub fn file_magic_strength(m: &Magic) -> usize {
    let mut val = magic_strength_1(m);
    let f = i64::from(m.factor);
    match m.factor_op {
        FILE_FACTOR_OP_PLUS => val += f,
        FILE_FACTOR_OP_MINUS => val -= f,
        FILE_FACTOR_OP_TIMES => val *= f,
        FILE_FACTOR_OP_DIV => val = val.checked_div(f).unwrap_or(val),
        _ => {}
    }
    if val <= 0 {
        val = 1;
    }
    if m.desc[0] == 0 {
        val += 1;
    }
    usize::try_from(val).unwrap_or(1)
}

// ---- reading one line -----------------------------------------------------------------

/// One top-level rule and its continuations (`struct magic_entry`).
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub mp: Vec<Magic>,
}

/// `EATAB`: skip ASCII white space.
fn eatab(l: &[u8]) -> &[u8] {
    let n = l.iter().take_while(|&&c| c.is_ascii() && isspace(c)).count();
    l.get(n..).unwrap_or_default()
}

/// `get_op`.
fn get_op(c: u8) -> Option<u8> {
    Some(match c {
        b'&' => FILE_OPAND,
        b'|' => FILE_OPOR,
        b'^' => FILE_OPXOR,
        b'+' => FILE_OPADD,
        b'-' => FILE_OPMINUS,
        b'*' => FILE_OPMULTIPLY,
        b'/' => FILE_OPDIVIDE,
        b'%' => FILE_OPMODULO,
        _ => return None,
    })
}

/// `file_signextend`: a value read as this rule's type, sign-extended unless
/// the rule is unsigned. A type the C switch does not list -- the ID3 types,
/// among the parseable ones -- is `FILE_BADSIZE`, with a warning when
/// checking.
#[must_use]
pub fn file_signextend(ms: &Ms, m: &Magic, v: u64) -> u64 {
    if m.flag & UNSIGNED != 0 {
        return v;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss)]
    match m.typ {
        FILE_BYTE => i64::from(v as u8 as i8) as u64,
        FILE_SHORT | FILE_BESHORT | FILE_LESHORT => i64::from(v as u16 as i16) as u64,
        FILE_DATE | FILE_BEDATE | FILE_LEDATE | FILE_MEDATE | FILE_LDATE | FILE_BELDATE
        | FILE_LELDATE | FILE_MELDATE | FILE_LONG | FILE_BELONG | FILE_LELONG | FILE_MELONG
        | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT | FILE_MSDOSDATE | FILE_BEMSDOSDATE
        | FILE_LEMSDOSDATE | FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => {
            i64::from(v as u32 as i32) as u64
        }
        FILE_QUAD | FILE_BEQUAD | FILE_LEQUAD | FILE_QDATE | FILE_QLDATE | FILE_QWDATE
        | FILE_BEQDATE | FILE_BEQLDATE | FILE_BEQWDATE | FILE_LEQDATE | FILE_LEQLDATE
        | FILE_LEQWDATE | FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE | FILE_OFFSET
        | FILE_BEVARINT | FILE_LEVARINT | FILE_STRING | FILE_PSTRING | FILE_BESTRING16
        | FILE_LESTRING16 | FILE_REGEX | FILE_SEARCH | FILE_DEFAULT | FILE_INDIRECT
        | FILE_NAME | FILE_USE | FILE_CLEAR | FILE_DER | FILE_GUID | FILE_OCTAL => v,
        t => {
            if ms.flags & MAGIC_CHECK != 0 {
                ms.magwarn(format!("cannot happen: m->type={t}\n").as_bytes());
            }
            u64::MAX
        }
    }
}

/// `eatsize`: skip a C integer suffix (`10UL`).
fn eatsize(l: &[u8]) -> &[u8] {
    let low = |j: usize| l.get(j).copied().unwrap_or(0).to_ascii_lowercase();
    let mut i = 0usize;
    if low(i) == b'u' {
        i += 1;
    }
    if matches!(low(i), b'l' | b's' | b'h' | b'b' | b'c') {
        i += 1;
    }
    l.get(i..).unwrap_or_default()
}

/// The text of a line left to parse.
struct Line<'a> {
    l: &'a [u8],
}

impl Line<'_> {
    fn peek(&self) -> u8 {
        self.l.first().copied().unwrap_or(0)
    }
    fn at(&self, i: usize) -> u8 {
        self.l.get(i).copied().unwrap_or(0)
    }
    fn bump(&mut self, n: usize) {
        self.l = self.l.get(n..).unwrap_or_default();
    }
}

/// `parse_op_modifier`: `&0xff`, `+5` and the rest after a numeric type.
fn parse_op_modifier(ms: &Ms, m: &mut Magic, line: &mut Line<'_>, op: u8) {
    line.bump(1);
    m.mask_op |= op;
    let (val, used, _) = strtoull(line.l, 0);
    line.bump(used);
    let v = file_signextend(ms, m, val);
    m.set_num_mask(v);
    line.l = eatsize(line.l);
}

/// `string_modifier_check`.
fn string_modifier_check(ms: &Ms, m: &mut Magic) -> bool {
    if ms.flags & MAGIC_CHECK == 0 {
        return true;
    }
    if (m.typ != FILE_REGEX || m.str_flags() & REGEX_LINE_COUNT == 0)
        && (m.typ != FILE_PSTRING && m.str_flags() & PSTRING_LEN != 0)
    {
        ms.magwarn(b"'/BHhLl' modifiers are only allowed for pascal strings\n");
        return false;
    }
    match m.typ {
        FILE_BESTRING16 | FILE_LESTRING16 => {
            if m.str_flags() != 0 {
                ms.magwarn(b"no modifiers allowed for 16-bit strings\n");
                return false;
            }
        }
        FILE_STRING | FILE_PSTRING => {
            if m.str_flags() & REGEX_OFFSET_START != 0 {
                ms.magwarn(b"'/s' only allowed on regex and search\n");
                return false;
            }
        }
        FILE_SEARCH => {
            if m.str_range() == 0 {
                ms.magwarn(format!("missing range; defaulting to {STRING_DEFAULT_RANGE}\n").as_bytes());
                m.set_str_range(STRING_DEFAULT_RANGE);
                return false;
            }
        }
        FILE_REGEX => {
            if m.str_flags() & STRING_COMPACT_WHITESPACE != 0 {
                ms.magwarn(b"'/W' not allowed on regex\n");
                return false;
            }
            if m.str_flags() & STRING_COMPACT_OPTIONAL_WHITESPACE != 0 {
                ms.magwarn(b"'/w' not allowed on regex\n");
                return false;
            }
        }
        _ => {
            ms.magwarn(format!("coding error: m->type={}\n", m.typ).as_bytes());
            return false;
        }
    }
    true
}

fn set_flag(m: &mut Magic, f: u32) -> bool {
    m.set_str_flags(m.str_flags() | f);
    true
}

fn set_length(m: &mut Magic, f: u32) -> bool {
    m.set_str_flags((m.str_flags() & !PSTRING_LEN) | f);
    true
}

/// A `%c` of a byte, as C writes one: the byte itself, NUL included.
fn ch(c: u8) -> Vec<u8> {
    vec![c]
}

/// `parse_string_modifier`: the letters after a string type's `/`.
fn parse_string_modifier(ms: &Ms, m: &mut Magic, line: &mut Line<'_>) -> bool {
    let mut have_range = false;
    loop {
        line.bump(1);
        let c = line.peek();
        if isspace(c) {
            break;
        }
        let ok = match c {
            b'0'..=b'9' => {
                if have_range && ms.flags & MAGIC_CHECK != 0 {
                    ms.magwarn(b"multiple ranges");
                }
                have_range = true;
                let (v, used) = strtoul(line.l, 0);
                #[allow(clippy::cast_possible_truncation)]
                m.set_str_range(v as u32);
                if m.str_range() == 0 {
                    ms.magwarn(b"zero range");
                }
                // `l = t - 1`: stand on the last digit.
                line.bump(used.saturating_sub(1));
                true
            }
            b'W' => set_flag(m, STRING_COMPACT_WHITESPACE),
            b'w' => set_flag(m, STRING_COMPACT_OPTIONAL_WHITESPACE),
            b'c' => set_flag(m, STRING_IGNORE_LOWERCASE),
            b'C' => set_flag(m, STRING_IGNORE_UPPERCASE),
            b's' => set_flag(m, REGEX_OFFSET_START),
            b'b' => set_flag(m, STRING_BINTEST),
            b't' => set_flag(m, STRING_TEXTTEST),
            b'T' => set_flag(m, STRING_TRIM),
            b'f' => set_flag(m, STRING_FULL_WORD),
            b'B' if m.typ == FILE_PSTRING => set_length(m, PSTRING_1_LE),
            b'H' if m.typ == FILE_PSTRING => set_length(m, PSTRING_2_BE),
            b'h' if m.typ == FILE_PSTRING => set_length(m, PSTRING_2_LE),
            b'L' if m.typ == FILE_PSTRING => set_length(m, PSTRING_4_BE),
            b'l' if matches!(m.typ, FILE_PSTRING | FILE_REGEX) => set_length(m, PSTRING_4_LE),
            b'J' if m.typ == FILE_PSTRING => set_flag(m, PSTRING_LENGTH_INCLUDES_ITSELF),
            _ => false,
        };
        if !ok {
            if ms.flags & MAGIC_CHECK != 0 {
                let mut w = b"string modifier `".to_vec();
                w.extend_from_slice(&ch(c));
                w.extend_from_slice(b"' invalid");
                ms.magwarn(&w);
            }
            return false;
        }
        // Allow multiple `/` for readability.
        if line.at(1) == b'/' && !isspace(line.at(2)) {
            line.bump(1);
        }
    }
    string_modifier_check(ms, m)
}

/// `parse_indirect_modifier`: `indirect/r`.
fn parse_indirect_modifier(ms: &Ms, m: &mut Magic, line: &mut Line<'_>) -> bool {
    loop {
        line.bump(1);
        let c = line.peek();
        if isspace(c) {
            return true;
        }
        if c == b'r' {
            m.set_str_flags(m.str_flags() | INDIRECT_RELATIVE);
        } else {
            if ms.flags & MAGIC_CHECK != 0 {
                let mut w = b"indirect modifier `".to_vec();
                w.extend_from_slice(&ch(c));
                w.extend_from_slice(b"' invalid");
                ms.magwarn(&w);
            }
            return false;
        }
    }
}

/// `file_pstring_length_size`: the bytes a pascal string's length takes;
/// `None` (with an error) for a corrupt rule.
pub fn file_pstring_length_size(ms: &mut Ms, m: &Magic) -> Option<usize> {
    match m.str_flags() & PSTRING_LEN {
        PSTRING_1_LE => Some(1),
        PSTRING_2_LE | PSTRING_2_BE => Some(2),
        PSTRING_4_LE | PSTRING_4_BE => Some(4),
        f => {
            ms.error(None, format!("corrupt magic file (bad pascal string length {f})").as_bytes());
            None
        }
    }
}

/// `file_pstring_get_length`: the length a pascal string's prefix says.
pub fn file_pstring_get_length(ms: &mut Ms, m: &Magic, s: &[u8]) -> Option<usize> {
    let b = |i: usize| usize::from(s.get(i).copied().unwrap_or(0));
    let mut len = match m.str_flags() & PSTRING_LEN {
        PSTRING_1_LE => b(0),
        PSTRING_2_LE => (b(1) << 8) | b(0),
        PSTRING_2_BE => (b(0) << 8) | b(1),
        PSTRING_4_LE => (b(3) << 24) | (b(2) << 16) | (b(1) << 8) | b(0),
        PSTRING_4_BE => (b(0) << 24) | (b(1) << 16) | (b(2) << 8) | b(3),
        f => {
            ms.error(None, format!("corrupt magic file (bad pascal string length {f})").as_bytes());
            return None;
        }
    };
    if m.str_flags() & PSTRING_LENGTH_INCLUDES_ITSELF != 0 {
        let l = file_pstring_length_size(ms, m)?;
        len = len.wrapping_sub(l);
    }
    Some(len)
}

/// `hextoint`.
fn hextoint(c: u8) -> Option<u8> {
    if !c.is_ascii() {
        return None;
    }
    hexval(c)
}

/// `getstr`: a string value, C's escapes resolved, up to an unescaped white
/// space. `warn` -- only when compiling -- warns about needless escapes.
/// `None` (with an error recorded) when the value does not fit.
fn getstr<'a>(ms: &mut Ms, m: &mut Magic, s: &'a [u8], warn: bool) -> Option<&'a [u8]> {
    let mut warn = warn;
    let at = |j: usize| s.get(j).copied().unwrap_or(0);
    let pmax = MAXSTRING - 1;
    let mut p = 0usize;
    let mut i = 0usize;
    let mut bracket_nesting = 0usize;
    // `m` was zeroed when its line began, so the value's bytes past what is
    // written stay zero.
    loop {
        let c = at(i);
        if c == 0 {
            break;
        }
        i += 1;
        if isspace(c) {
            // `--s` after the loop: the space stays in the rest.
            i -= 1;
            break;
        }
        if p >= pmax {
            let mut msg = b"string too long: `".to_vec();
            msg.extend_from_slice(cstr(s));
            msg.push(b'\'');
            ms.error(None, &msg);
            return None;
        }
        if c != b'\\' {
            if c == b'[' {
                bracket_nesting += 1;
            }
            if c == b']' && bracket_nesting > 0 {
                bracket_nesting -= 1;
            }
            m.value.0[p] = c;
            p += 1;
            continue;
        }
        let c = at(i);
        i += 1;
        let out = match c {
            0 => {
                if warn {
                    ms.magwarn(b"incomplete escape");
                }
                i -= 1;
                break;
            }
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            b'0'..=b'7' => {
                let mut val = u32::from(c - b'0');
                let c2 = at(i);
                if (b'0'..=b'7').contains(&c2) {
                    i += 1;
                    val = (val << 3) | u32::from(c2 - b'0');
                    let c3 = at(i);
                    if (b'0'..=b'7').contains(&c3) {
                        i += 1;
                        val = (val << 3) | u32::from(c3 - b'0');
                    }
                }
                #[allow(clippy::cast_possible_truncation)]
                let b = val as u8;
                b
            }
            b'x' => {
                let mut val = b'x';
                if let Some(h1) = hextoint(at(i)) {
                    i += 1;
                    val = h1;
                    if let Some(h2) = hextoint(at(i)) {
                        i += 1;
                        val = (val << 4).wrapping_add(h2);
                    }
                }
                val
            }
            b' ' | b'>' | b'<' | b'&' | b'^' | b'=' | b'!' | b'\\' => c,
            other => {
                // `.` and a tab warn once and then stop all warnings for the
                // string; anything else warns if it needed no escape.
                if other == b'.' {
                    if m.typ == FILE_REGEX && bracket_nesting == 0 && warn {
                        ms.magwarn(b"escaped dot ('.') found, use \\\\. instead");
                    }
                    warn = false;
                }
                if other == b'\t' && warn {
                    ms.magwarn(b"escaped tab found, use \\\\t instead");
                    warn = false;
                }
                if warn {
                    if crate::cstd::isprint(other) {
                        if !b"<>&^=!".contains(&other)
                            && (m.typ != FILE_REGEX || !b"[]().*?^$|{}".contains(&other))
                        {
                            let mut w = b"no need to escape `".to_vec();
                            w.push(other);
                            w.push(b'\'');
                            ms.magwarn(&w);
                        }
                    } else {
                        ms.magwarn(format!("unknown escape sequence: \\{other:03o}").as_bytes());
                    }
                }
                other
            }
        };
        m.value.0[p] = out;
        p += 1;
    }
    m.value.0[p] = 0;
    #[allow(clippy::cast_possible_truncation)]
    {
        m.vallen = p as u8;
    }
    if m.typ == FILE_PSTRING {
        let l = file_pstring_length_size(ms, m)?;
        #[allow(clippy::cast_possible_truncation)]
        {
            m.vallen = m.vallen.wrapping_add(l as u8);
        }
    }
    Some(s.get(i..).unwrap_or_default())
}

/// `sscanf`'s `%Nx`: white space, an optional sign, an optional `0x`, then
/// hex digits -- `N` characters at most in all, at least one digit.
fn scan_hex(s: &[u8], i: &mut usize, width: usize) -> Option<u64> {
    while s.get(*i).copied().is_some_and(isspace) {
        *i += 1;
    }
    let mut left = width;
    let mut neg = false;
    if let Some(&c @ (b'+' | b'-')) = s.get(*i) {
        neg = c == b'-';
        *i += 1;
        left -= 1;
    }
    let mut v: u64 = 0;
    let mut digits = 0usize;
    // glibc takes a `0` as a digit, and then skips an `x` after it.
    if left > 0 && s.get(*i) == Some(&b'0') {
        *i += 1;
        left -= 1;
        digits += 1;
        if left > 0 && matches!(s.get(*i), Some(b'x' | b'X')) {
            *i += 1;
            left -= 1;
        }
    }
    while left > 0 {
        let Some(h) = s.get(*i).and_then(|&c| hexval(c)) else {
            break;
        };
        v = v.wrapping_mul(16).wrapping_add(u64::from(h));
        *i += 1;
        left -= 1;
        digits += 1;
    }
    if digits == 0 {
        return None;
    }
    Some(if neg { v.wrapping_neg() } else { v })
}

/// `file_parse_guid`: `sscanf("%8x-%4hx-%4hx-%2hhx%2hhx-%2hhx...")` into the
/// 16 bytes of `struct guid`.
fn parse_guid(s: &[u8]) -> Option<[u8; 16]> {
    let mut out = [0u8; 16];
    let mut i = 0usize;
    let lit = |c: u8, i: &mut usize| -> Option<()> { (s.get(*i) == Some(&c)).then(|| *i += 1) };
    #[allow(clippy::cast_possible_truncation)]
    {
        let d1 = scan_hex(s, &mut i, 8)? as u32;
        lit(b'-', &mut i)?;
        let d2 = scan_hex(s, &mut i, 4)? as u16;
        lit(b'-', &mut i)?;
        let d3 = scan_hex(s, &mut i, 4)? as u16;
        lit(b'-', &mut i)?;
        out[0..4].copy_from_slice(&d1.to_le_bytes());
        out[4..6].copy_from_slice(&d2.to_le_bytes());
        out[6..8].copy_from_slice(&d3.to_le_bytes());
        out[8] = scan_hex(s, &mut i, 2)? as u8;
        out[9] = scan_hex(s, &mut i, 2)? as u8;
        lit(b'-', &mut i)?;
        for slot in &mut out[10..16] {
            *slot = scan_hex(s, &mut i, 2)? as u8;
        }
    }
    Some(out)
}

/// `getvalue`: the value after a rule's relation.
fn getvalue<'a>(ms: &mut Ms, m: &mut Magic, p: &'a [u8], action: Action) -> Option<&'a [u8]> {
    match m.typ {
        FILE_BESTRING16 | FILE_LESTRING16 | FILE_STRING | FILE_PSTRING | FILE_REGEX
        | FILE_SEARCH | FILE_NAME | FILE_USE | FILE_DER | FILE_OCTAL => {
            let Some(rest) = getstr(ms, m, p, action == Action::Compile) else {
                if ms.flags & MAGIC_CHECK != 0 {
                    let mut w = b"cannot get string from `".to_vec();
                    w.extend_from_slice(m.value.s());
                    w.push(b'\'');
                    ms.magwarn(&w);
                }
                return None;
            };
            if m.typ == FILE_REGEX {
                let pat = m.value.s().to_vec();
                file_regcomp(ms, &pat, RegFlags::default()).ok()?;
            }
            return Some(rest);
        }
        _ => {
            if m.reln == b'x' {
                return Some(p);
            }
        }
    }
    match m.typ {
        FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
            // `errno = 0`, then whatever `strtof` leaves -- which is what the
            // final "could not find any valid magic files" names.
            let r = cprintf::extfloat::strtof(p);
            ms.errno = r.range_error.then_some(crate::funcs::Errno::Range);
            m.value.set_f(r.value);
            Some(if r.range_error { p } else { p.get(r.consumed..).unwrap_or_default() })
        }
        FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
            let r = cprintf::extfloat::strtod(p);
            ms.errno = r.range_error.then_some(crate::funcs::Errno::Range);
            m.value.set_d(r.value);
            Some(if r.range_error { p } else { p.get(r.consumed..).unwrap_or_default() })
        }
        FILE_GUID => {
            let g = parse_guid(p)?;
            m.value.0[..16].copy_from_slice(&g);
            // `*p += FILE_GUID_SIZE - 1`.
            Some(p.get(36..).unwrap_or_default())
        }
        _ => {
            let (ull0, used, erange) = strtoull(p, 0);
            ms.errno = erange.then_some(crate::funcs::Errno::Range);
            m.value.set_q(file_signextend(ms, m, ull0));
            if used == 0 {
                let mut w = b"Unparsable number `".to_vec();
                w.extend_from_slice(cstr(p));
                w.push(b'\'');
                ms.magwarn(&w);
                return None;
            }
            let Some(ts) = typesize(m.typ) else {
                let mut w = b"Expected numeric type got `".to_vec();
                w.extend_from_slice(type_tbl_name(m.typ));
                w.push(b'\'');
                ms.magwarn(&w);
                return None;
            };
            let mut ull = ull0;
            let q = p.iter().copied().find(|&c| !isspace(c)).unwrap_or(0);
            if q == b'-' && ull != u64::MAX {
                ull = ull.wrapping_neg();
            }
            let over = match ts {
                1 => {
                    let x = ull & !0xff;
                    x != 0 && x & !0xff != !0xff
                }
                2 => {
                    let x = ull & !0xffff;
                    x != 0 && x & !0xffff != !0xffff
                }
                4 => {
                    let x = ull & !0xffff_ffff;
                    x != 0 && x & !0xffff_ffff != !0xffff_ffff
                }
                _ => false,
            };
            if over {
                let mut w = b"Overflow for numeric type `".to_vec();
                w.extend_from_slice(type_tbl_name(m.typ));
                w.extend_from_slice(format!("' value {ull:#x}").as_bytes());
                ms.magwarn(&w);
                return None;
            }
            if erange {
                // C advances only when `errno` was not set.
                return Some(p);
            }
            Some(eatsize(p.get(used..).unwrap_or_default()))
        }
    }
}

/// `check_format_type`: whether the format in a description fits the type.
fn check_format_type(ptr: &[u8], typ: u8) -> Result<(), &'static str> {
    let at = |j: usize| ptr.get(j).copied().unwrap_or(0);
    if at(0) == 0 {
        return Err("missing format spec");
    }
    let mut i = 0usize;
    // `CHECKLEN`: no more than five digits, and no more than 1024.
    let checklen = |i: &mut usize| -> Result<(), &'static str> {
        let mut len: u64 = 0;
        let mut cnt = 0usize;
        while isdigit(at(*i)) {
            len = len.saturating_mul(10).saturating_add(u64::from(at(*i) - b'0'));
            *i += 1;
            cnt += 1;
        }
        if cnt > 5 || len > 1024 {
            return Err("too long");
        }
        Ok(())
    };
    match file_format(typ) {
        FILE_FMT_QUAD | FILE_FMT_NUM => {
            let quad = file_format(typ) == FILE_FMT_QUAD;
            let h = if quad {
                0
            } else {
                match typ {
                    FILE_BYTE => 2,
                    FILE_SHORT | FILE_BESHORT | FILE_LESHORT => 1,
                    _ => 0,
                }
            };
            while at(i) != 0 && b"-.#".contains(&at(i)) {
                i += 1;
            }
            checklen(&mut i)?;
            if at(i) == b'.' {
                i += 1;
            }
            checklen(&mut i)?;
            if quad {
                for _ in 0..2 {
                    if at(i) != b'l' {
                        return Err("not valid");
                    }
                    i += 1;
                }
            }
            match at(i) {
                b'c' if h == 2 => Ok(()),
                b'i' | b'd' | b'u' | b'o' | b'x' | b'X' => Ok(()),
                _ => Err("not valid"),
            }
        }
        FILE_FMT_FLOAT | FILE_FMT_DOUBLE => {
            if at(i) == b'-' {
                i += 1;
            }
            if at(i) == b'.' {
                i += 1;
            }
            checklen(&mut i)?;
            if at(i) == b'.' {
                i += 1;
            }
            checklen(&mut i)?;
            match at(i) {
                b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => Ok(()),
                _ => Err("not valid"),
            }
        }
        FILE_FMT_STR => {
            if at(i) == b'-' {
                i += 1;
            }
            while isdigit(at(i)) {
                i += 1;
            }
            if at(i) == b'.' {
                i += 1;
                while isdigit(at(i)) {
                    i += 1;
                }
            }
            if at(i) == b's' { Ok(()) } else { Err("not valid") }
        }
        _ => Err("not valid"),
    }
}

/// `check_format`: a description's one `%` fits its rule's type.
fn check_format(ms: &Ms, m: &Magic) -> bool {
    let desc = m.desc();
    let Some(pct) = desc.iter().position(|&c| c == b'%') else {
        return true;
    };
    if file_format(m.typ) == FILE_FMT_NONE {
        let mut w = b"No format string for `".to_vec();
        w.extend_from_slice(desc);
        w.extend_from_slice(b"' with description `");
        w.extend_from_slice(file_name(m.typ));
        w.push(b'\'');
        ms.magwarn(&w);
        return false;
    }
    let rest = desc.get(pct + 1..).unwrap_or_default();
    if let Err(estr) = check_format_type(rest, m.typ) {
        let mut w = format!("Printf format is {estr} for type `").into_bytes();
        w.extend_from_slice(file_name(m.typ));
        w.extend_from_slice(b"' in description `");
        w.extend_from_slice(desc);
        w.push(b'\'');
        ms.magwarn(&w);
        return false;
    }
    if rest.contains(&b'%') {
        let mut w = b"Too many format strings (should have at most one) for `".to_vec();
        w.extend_from_slice(file_name(m.typ));
        w.extend_from_slice(b"' with description `");
        w.extend_from_slice(desc);
        w.push(b'\'');
        ms.magwarn(&w);
        return false;
    }
    true
}

/// What `parse` made of a line.
enum Parsed {
    /// It added to the current entry, or was not a rule (0).
    Done,
    /// A new top-level rule starts: the current entry is complete and the line
    /// is read again into a fresh one (1).
    NewEntry,
    /// The line is bad (-1).
    Bad,
}

/// `parse`: one line of a magic file into `me`.
#[allow(clippy::too_many_lines)]
fn parse(ms: &mut Ms, me: &mut Entry, raw: &[u8], lineno: usize, action: Action) -> Parsed {
    let mut line = Line { l: cstr(raw) };
    let mut cont_level: u16 = 0;
    while line.peek() == b'>' {
        line.bump(1);
        cont_level = cont_level.wrapping_add(1);
    }
    if cont_level != 0 {
        let Some(last) = me.mp.last() else {
            ms.magerror(b"No current entry for continuation");
            return Parsed::Bad;
        };
        let diff = i32::from(cont_level) - i32::from(last.cont_level);
        if diff > 1 {
            ms.magwarn(
                format!(
                    "New continuation level {cont_level} is more than one larger than current level {}",
                    last.cont_level
                )
                .as_bytes(),
            );
        }
        let mut fresh = Magic::default();
        fresh.cont_level = cont_level;
        fresh.factor_op = 0;
        me.mp.push(fresh);
    } else {
        if !me.mp.is_empty() {
            return Parsed::NewEntry;
        }
        me.mp.push(Magic::default());
    }
    let Some(m) = me.mp.last_mut() else {
        return Parsed::Bad;
    };
    #[allow(clippy::cast_possible_truncation)]
    {
        m.lineno = lineno as u32;
    }

    if line.peek() == b'&' {
        line.bump(1);
        m.flag |= OFFADD;
    }
    if line.peek() == b'(' {
        line.bump(1);
        m.flag |= INDIR;
        if m.flag & OFFADD != 0 {
            m.flag = (m.flag & !OFFADD) | INDIROFFADD;
        }
        if line.peek() == b'&' {
            line.bump(1);
            m.flag |= OFFADD;
        }
    }
    // Indirect offsets are not valid at level 0.
    if m.cont_level == 0 && m.flag & (OFFADD | INDIROFFADD) != 0 {
        if ms.flags & MAGIC_CHECK != 0 {
            ms.magwarn(b"relative offset at level 0");
        }
        return Parsed::Bad;
    }

    if line.peek() == b'-' {
        line.bump(1);
        m.flag |= OFFNEGATIVE;
    }
    let (off, used) = strtol_i32(line.l, 0);
    if used == 0 {
        if ms.flags & MAGIC_CHECK != 0 {
            let mut w = b"offset `".to_vec();
            w.extend_from_slice(line.l);
            w.extend_from_slice(b"' invalid");
            ms.magwarn(&w);
        }
        return Parsed::Bad;
    }
    m.offset = off;
    line.bump(used);
    // `t`, where the last `strtol` stopped: one message prints the character
    // there.
    let mut t_char = line.peek();

    if m.flag & INDIR != 0 {
        m.in_type = FILE_LONG;
        m.in_offset = 0;
        m.in_op = 0;
        if matches!(line.peek(), b'.' | b',') {
            if line.peek() == b',' {
                m.in_op |= FILE_OPSIGNED;
            }
            line.bump(1);
            m.in_type = match line.peek() {
                b'l' => FILE_LELONG,
                b'L' => FILE_BELONG,
                b'm' => FILE_MELONG,
                b'h' | b's' => FILE_LESHORT,
                b'H' | b'S' => FILE_BESHORT,
                b'c' | b'b' | b'C' | b'B' => FILE_BYTE,
                b'e' | b'f' | b'g' => FILE_LEDOUBLE,
                b'E' | b'F' | b'G' => FILE_BEDOUBLE,
                b'i' => FILE_LEID3,
                b'I' => FILE_BEID3,
                b'o' => FILE_OCTAL,
                b'q' => FILE_LEQUAD,
                b'Q' => FILE_BEQUAD,
                other => {
                    if ms.flags & MAGIC_CHECK != 0 {
                        let mut w = b"indirect offset type `".to_vec();
                        w.extend_from_slice(&ch(other));
                        w.extend_from_slice(b"' invalid");
                        ms.magwarn(&w);
                    }
                    return Parsed::Bad;
                }
            };
            line.bump(1);
        }
        if line.peek() == b'~' {
            m.in_op |= FILE_OPINVERSE;
            line.bump(1);
        }
        if let Some(op) = get_op(line.peek()) {
            m.in_op |= op;
            line.bump(1);
        }
        if line.peek() == b'(' {
            m.in_op |= FILE_OPINDIRECT;
            line.bump(1);
        }
        if isdigit(line.peek()) || line.peek() == b'-' {
            let (v, used) = strtol_i32(line.l, 0);
            if used == 0 {
                if ms.flags & MAGIC_CHECK != 0 {
                    let mut w = b"in_offset `".to_vec();
                    w.extend_from_slice(line.l);
                    w.extend_from_slice(b"' invalid");
                    ms.magwarn(&w);
                }
                return Parsed::Bad;
            }
            m.in_offset = v;
            line.bump(used);
            t_char = line.peek();
        }
        let close1 = line.peek() == b')';
        line.bump(1);
        let close2 = !close1 || m.in_op & FILE_OPINDIRECT == 0 || {
            let c = line.peek() == b')';
            line.bump(1);
            c
        };
        if !close1 || !close2 {
            if ms.flags & MAGIC_CHECK != 0 {
                ms.magwarn(b"missing ')' in indirect offset");
            }
            return Parsed::Bad;
        }
    }
    line.l = eatab(line.l);

    // The type.
    if line.peek() == b'u' {
        // Try it as a keyword type prefixed by "u", then as a SUS integer type.
        let mut typ = FILE_INVALID;
        if let Some((tt, rest)) = get_type(TYPE_TBL, line.l.get(1..).unwrap_or_default()) {
            typ = tt;
            line.l = rest;
        }
        if typ == FILE_INVALID {
            if let Some((tt, rest)) = get_standard_integer_type(line.l) {
                typ = tt;
                line.l = rest;
            }
        }
        m.typ = typ;
        if m.typ != FILE_INVALID {
            m.flag |= UNSIGNED;
        }
    } else {
        let mut typ = FILE_INVALID;
        if let Some((tt, rest)) = get_type(TYPE_TBL, line.l) {
            typ = tt;
            line.l = rest;
        }
        if typ == FILE_INVALID {
            if line.peek() == b'd' {
                if let Some((tt, rest)) = get_standard_integer_type(line.l) {
                    typ = tt;
                    line.l = rest;
                }
            } else if line.peek() == b's' && !isalpha(line.at(1)) {
                typ = FILE_STRING;
                line.bump(1);
            }
        }
        m.typ = typ;
    }
    if m.typ == FILE_INVALID {
        // Not found -- try it as a special keyword.
        if let Some((tt, rest)) = get_type(SPECIAL_TBL, line.l) {
            m.typ = tt;
            line.l = rest;
        }
    }
    if m.typ == FILE_INVALID {
        if ms.flags & MAGIC_CHECK != 0 {
            let mut w = b"type `".to_vec();
            w.extend_from_slice(line.l);
            w.extend_from_slice(b"' invalid");
            ms.magwarn(&w);
        }
        return Parsed::Bad;
    }
    if m.typ == FILE_NAME && cont_level != 0 {
        if ms.flags & MAGIC_CHECK != 0 {
            let mut w = b"`name".to_vec();
            w.extend_from_slice(line.l);
            w.extend_from_slice(b"' entries can only be declared at top level");
            ms.magwarn(&w);
        }
        return Parsed::Bad;
    }

    m.mask_op = 0;
    if line.peek() == b'~' {
        if !is_string(m.typ) {
            m.mask_op |= FILE_OPINVERSE;
        } else if ms.flags & MAGIC_CHECK != 0 {
            ms.magwarn(b"'~' invalid for string types");
        }
        line.bump(1);
    }
    m.set_str_range(0);
    m.set_str_flags(if m.typ == FILE_PSTRING { PSTRING_1_LE } else { 0 });
    if let Some(op) = get_op(line.peek()) {
        if is_string(m.typ) {
            if op != FILE_OPDIVIDE {
                if ms.flags & MAGIC_CHECK != 0 {
                    let mut w = b"invalid string/indirect op: `".to_vec();
                    w.extend_from_slice(&ch(t_char));
                    w.push(b'\'');
                    ms.magwarn(&w);
                }
                return Parsed::Bad;
            }
            let ok = if m.typ == FILE_INDIRECT {
                parse_indirect_modifier(ms, m, &mut line)
            } else {
                parse_string_modifier(ms, m, &mut line)
            };
            if !ok {
                return Parsed::Bad;
            }
        } else {
            parse_op_modifier(ms, m, &mut line, op);
        }
    }
    line.l = eatab(line.l);

    // The relation.
    match line.peek() {
        b'>' | b'<' => {
            m.reln = line.peek();
            line.bump(1);
            if line.peek() == b'=' {
                if ms.flags & MAGIC_CHECK != 0 {
                    let mut w = ch(m.reln);
                    w.extend_from_slice(b"= not supported");
                    ms.magwarn(&w);
                    return Parsed::Bad;
                }
                line.bump(1);
            }
        }
        b'&' | b'^' | b'=' => {
            m.reln = line.peek();
            line.bump(1);
            if line.peek() == b'=' {
                // HP compat: ignore `&=` and the rest.
                line.bump(1);
            }
        }
        b'!' => {
            m.reln = b'!';
            line.bump(1);
        }
        _ => {
            m.reln = b'=';
            if line.peek() == b'x'
                && ((line.at(1).is_ascii() && isspace(line.at(1))) || line.at(1) == 0)
            {
                m.reln = b'x';
                line.bump(1);
            }
        }
    }
    // The value, except for an `x` relation.
    if m.reln != b'x' {
        let Some(rest) = getvalue(ms, m, line.l, action) else {
            return Parsed::Bad;
        };
        line.l = rest;
    }

    // The description.
    line.l = eatab(line.l);
    if line.peek() == 0x08 {
        line.bump(1);
        m.flag |= NOSPACE;
    } else if line.peek() == b'\\' && line.at(1) == b'b' {
        line.bump(2);
        m.flag |= NOSPACE;
    }
    // `for (i = 0; (desc[i++] = *l++) != '\0' && i < sizeof(desc); )`: up
    // to 64 bytes copied, the NUL included; a stop at 64 is "truncated" --
    // even when the 64th byte copied was the NUL.
    let text = line.l;
    let mut i = 0usize;
    loop {
        let c = text.get(i).copied().unwrap_or(0);
        m.desc[i] = c;
        i += 1;
        if c == 0 || i >= MAXDESC {
            break;
        }
    }
    if i == MAXDESC {
        m.desc[MAXDESC - 1] = 0;
        if ms.flags & MAGIC_CHECK != 0 {
            let mut w = b"description `".to_vec();
            w.extend_from_slice(m.desc());
            w.extend_from_slice(b"' truncated");
            ms.magwarn(&w);
        }
    }
    // The format is only checked while compiling, or when any of the magic
    // was not compiled.
    if ms.flags & MAGIC_CHECK != 0 && !check_format(ms, m) {
        return Parsed::Bad;
    }
    if action == Action::Check {
        crate::print::file_mdump(m);
    }
    m.mimetype[0] = 0;
    Parsed::Done
}

/// `parse_strength`: `!:strength +10`, on the entry's first line.
fn parse_strength(ms: &Ms, me: &mut Entry, line: &[u8]) -> bool {
    let Some(m) = me.mp.first_mut() else {
        return false;
    };
    if m.factor_op != FILE_FACTOR_OP_NONE {
        let mut w = b"Current entry already has a strength type: ".to_vec();
        w.push(m.factor_op);
        w.extend_from_slice(format!(" {}", m.factor).as_bytes());
        ms.magwarn(&w);
        return false;
    }
    if m.typ == FILE_NAME {
        let mut w = file_printable(ms.flags & MAGIC_RAW != 0, 512, &m.value.0, MAXSTRING);
        w.extend_from_slice(b": Strength setting is not supported in \"name\" magic entries");
        ms.magwarn(&w);
        return false;
    }
    let mut l = eatab(line);
    match l.first().copied().unwrap_or(0) {
        FILE_FACTOR_OP_NONE => {}
        c @ (FILE_FACTOR_OP_PLUS | FILE_FACTOR_OP_MINUS | FILE_FACTOR_OP_TIMES | FILE_FACTOR_OP_DIV) => {
            m.factor_op = c;
            l = l.get(1..).unwrap_or_default();
        }
        c => {
            let mut w = b"Unknown factor op `".to_vec();
            w.push(c);
            w.push(b'\'');
            ms.magwarn(&w);
            return false;
        }
    }
    l = eatab(l);
    let (factor, used) = strtoul(l, 0);
    let bad = if factor > 255 {
        ms.magwarn(format!("Too large factor `{factor}'").as_bytes());
        true
    } else if l.get(used).is_some_and(|&c| c != 0 && !isspace(c)) {
        let mut w = b"Bad factor `".to_vec();
        w.extend_from_slice(cstr(l));
        w.push(b'\'');
        ms.magwarn(&w);
        true
    } else {
        #[allow(clippy::cast_possible_truncation)]
        {
            m.factor = factor as u8;
        }
        if m.factor == 0 && m.factor_op == FILE_FACTOR_OP_DIV {
            let mut w = b"Cannot have factor op `".to_vec();
            w.push(m.factor_op);
            w.extend_from_slice(format!("' and factor {}", m.factor).as_bytes());
            ms.magwarn(&w);
            true
        } else {
            false
        }
    };
    if !bad {
        return true;
    }
    m.factor_op = FILE_FACTOR_OP_NONE;
    m.factor = 0;
    false
}

/// `goodchar`: alphanumerics and `extra` -- and the NUL, which `strchr`
/// finds in every string.
fn goodchar(x: u8, extra: &[u8]) -> bool {
    (x.is_ascii() && x.is_ascii_alphanumeric()) || extra.contains(&x) || x == 0
}

/// Which annotation `parse_extra` fills.
#[derive(Clone, Copy)]
enum Extra {
    Mime,
    Apple,
    Ext,
}

/// `parse_extra`: `!:mime`, `!:apple` and `!:ext`, onto the entry's last line.
fn parse_extra(ms: &mut Ms, me: &mut Entry, line: &[u8], llen: usize, which: Extra) -> bool {
    let (name, extra, nt): (&str, &[u8], bool) = match which {
        Extra::Mime => ("MIME", b"+-/.$?:{}", true),
        Extra::Apple => ("APPLE", b"!+-./?", false),
        // `&` for b&w, `~` for journal~.
        Extra::Ext => ("EXTENSION", b",!+-/@?_$&~", false),
    };
    let Some(m) = me.mp.last_mut() else {
        return false;
    };
    let len = match which {
        Extra::Mime => MAXMIME,
        Extra::Apple => 8,
        Extra::Ext => 64,
    };
    let buf: &[u8] = match which {
        Extra::Mime => &m.mimetype,
        Extra::Apple => &m.apple,
        Extra::Ext => &m.ext,
    };
    if buf[0] != 0 {
        let shown_len = if nt { cstrlen(buf) } else { len };
        let mut w = format!("Current entry already has a {name} type `").into_bytes();
        w.extend_from_slice(cstr(buf.get(..shown_len).unwrap_or(buf)));
        w.extend_from_slice(b"', new type `");
        w.extend_from_slice(cstr(line));
        w.push(b'\'');
        ms.magwarn(&w);
        return false;
    }
    if m.desc[0] == 0 {
        ms.magwarn(format!("Current entry does not yet have a description for adding a {name} type").as_bytes());
        return false;
    }
    let l = eatab(line);
    let at = |j: usize| l.get(j).copied().unwrap_or(0);
    let buf: &mut [u8] = match which {
        Extra::Mime => &mut m.mimetype,
        Extra::Apple => &mut m.apple,
        Extra::Ext => &mut m.ext,
    };
    let mut i = 0usize;
    while at(i) != 0 && i < llen && i < len && goodchar(at(i), extra) {
        buf[i] = at(i);
        i += 1;
    }
    if i == len && at(i) != 0 {
        if nt {
            buf[len - 1] = 0;
        }
        if ms.flags & MAGIC_CHECK != 0 {
            let mut w = format!("{name} type `").into_bytes();
            w.extend_from_slice(cstr(line));
            w.extend_from_slice(format!("' truncated {i}").as_bytes());
            ms.magwarn(&w);
        }
    } else {
        let c = at(i);
        if !isspace(c) && !goodchar(c, extra) {
            let mut w = format!("{name} type `").into_bytes();
            w.extend_from_slice(cstr(line));
            w.extend_from_slice(b"' has bad char '");
            w.push(c);
            w.push(b'\'');
            ms.magwarn(&w);
        }
        if nt {
            // A type that fills the field exactly ends here at `i == len`:
            // upstream writes its NUL one past `mimetype[]`, which in
            // `struct magic` is `apple[0]`.
            match buf.get_mut(i) {
                Some(b) => *b = 0,
                None => m.apple[0] = 0,
            }
        }
    }
    if i > 0 {
        return true;
    }
    let mut msg = b"Bad magic entry '".to_vec();
    msg.extend_from_slice(cstr(line));
    msg.push(b'\'');
    ms.magerror(&msg);
    false
}

// ---- reading a database -------------------------------------------------------------

/// The entries of a load, one list per set: ordinary entries, and the `name`
/// entries a `use` refers to (`struct magic_entry_set`).
#[derive(Default, Debug)]
pub struct EntrySets {
    pub sets: [Vec<Entry>; MAGIC_SETS],
}

/// `addentry`: a `name` entry goes to set 1, every other to set 0.
fn addentry(me: &mut Entry, mset: &mut EntrySets) {
    let i = usize::from(me.mp.first().is_some_and(|m| m.typ == FILE_NAME));
    mset.sets[i].push(std::mem::take(me));
}

/// `load_1`: one magic file's text into `mset`, counting its errors.
pub fn load_1(ms: &mut Ms, action: Action, name: &[u8], text: &[u8], errs: &mut usize, mset: &mut EntrySets) {
    ms.file = Some(name.to_vec());
    let mut me = Entry::default();
    let mut lineno = 0usize;
    ms.line = 1;
    let mut rest = text;
    while !rest.is_empty() {
        let (raw, has_nl) = match rest.iter().position(|&c| c == b'\n') {
            Some(n) => (&rest[..n], true),
            None => (rest, false),
        };
        // `len` is getline's: the line with its newline.
        let len = raw.len() + usize::from(has_nl);
        rest = rest.get(len..).unwrap_or_default();
        if has_nl {
            lineno += 1;
        }
        match raw.first().copied().unwrap_or(0) {
            // Empty, or a comment.
            0 | b'#' => {}
            b'!' if raw.get(1) == Some(&b':') => {
                let bangs: [(&[u8], Option<Extra>); 4] = [
                    (b"mime", Some(Extra::Mime)),
                    (b"apple", Some(Extra::Apple)),
                    (b"ext", Some(Extra::Ext)),
                    (b"strength", None),
                ];
                let found = bangs.iter().find(|(bname, _)| {
                    len.saturating_sub(2) > bname.len() && raw.get(2..2 + bname.len()) == Some(*bname)
                });
                match found {
                    None => {
                        let mut msg = b"Unknown !: entry `".to_vec();
                        msg.extend_from_slice(cstr(raw));
                        msg.push(b'\'');
                        ms.error(None, &msg);
                        *errs += 1;
                    }
                    Some((bname, which)) => {
                        if me.mp.is_empty() {
                            let mut msg = b"No current entry for :!".to_vec();
                            msg.extend_from_slice(bname);
                            msg.extend_from_slice(b" type");
                            ms.error(None, &msg);
                            *errs += 1;
                        } else {
                            let arg = raw.get(2 + bname.len()..).unwrap_or_default();
                            let alen = len - bname.len() - 2;
                            let ok = match which {
                                Some(w) => parse_extra(ms, &mut me, arg, alen, *w),
                                None => parse_strength(ms, &mut me, arg),
                            };
                            if !ok {
                                *errs += 1;
                            }
                        }
                    }
                }
            }
            _ => loop {
                match parse(ms, &mut me, raw, lineno, action) {
                    Parsed::Done => break,
                    Parsed::NewEntry => addentry(&mut me, mset),
                    Parsed::Bad => {
                        *errs += 1;
                        break;
                    }
                }
            },
        }
        ms.line += 1;
    }
    if !me.mp.is_empty() {
        addentry(&mut me, mset);
    }
}

/// `set_test_type`: whether a top-level rule tests binary data or text.
fn set_test_type(mstart: &mut Magic, m: &Magic) {
    match m.typ {
        FILE_BYTE | FILE_SHORT | FILE_LONG | FILE_DATE | FILE_BESHORT | FILE_BELONG
        | FILE_BEDATE | FILE_LESHORT | FILE_LELONG | FILE_LEDATE | FILE_LDATE | FILE_BELDATE
        | FILE_LELDATE | FILE_MEDATE | FILE_MELDATE | FILE_MELONG | FILE_QUAD | FILE_LEQUAD
        | FILE_BEQUAD | FILE_QDATE | FILE_LEQDATE | FILE_BEQDATE | FILE_QLDATE
        | FILE_LEQLDATE | FILE_BEQLDATE | FILE_QWDATE | FILE_LEQWDATE | FILE_BEQWDATE
        | FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT | FILE_DOUBLE | FILE_BEDOUBLE
        | FILE_LEDOUBLE | FILE_BEVARINT | FILE_LEVARINT | FILE_DER | FILE_GUID | FILE_OFFSET
        | FILE_MSDOSDATE | FILE_BEMSDOSDATE | FILE_LEMSDOSDATE | FILE_MSDOSTIME
        | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME | FILE_OCTAL => mstart.flag |= BINTEST,
        FILE_STRING | FILE_PSTRING | FILE_BESTRING16 | FILE_LESTRING16 => {
            // Allow text overrides.
            if mstart.str_flags() & STRING_TEXTTEST != 0 {
                mstart.flag |= TEXTTEST;
            } else {
                mstart.flag |= BINTEST;
            }
        }
        FILE_REGEX | FILE_SEARCH => {
            // Check for override.
            if mstart.str_flags() & STRING_BINTEST != 0 {
                mstart.flag |= BINTEST;
            }
            if mstart.str_flags() & STRING_TEXTTEST != 0 {
                mstart.flag |= TEXTTEST;
            }
            if mstart.flag & (TEXTTEST | BINTEST) != 0 {
                return;
            }
            // A binary test if the pattern is not text.
            let pat = m.value.0.get(..usize::from(m.vallen)).unwrap_or_default();
            if file_looks_utf8(pat, None) <= 0 {
                mstart.flag |= BINTEST;
            } else {
                mstart.flag |= TEXTTEST;
            }
        }
        _ => {}
    }
}

/// `set_text_binary`, as it runs: each entry's *first* line types it.
/// Upstream's loop means to visit the continuations too, but steps through the
/// array of entries rather than of lines, and every entry's first line is
/// level 0 -- so it stops after one.
fn set_text_binary(ms: &Ms, entries: &mut [Entry]) {
    for e in entries.iter_mut() {
        let Some(first) = e.mp.first_mut() else {
            continue;
        };
        let start = first.clone();
        set_test_type(first, &start);
        if ms.flags & MAGIC_DEBUG == 0 {
            continue;
        }
        let mut w = first.mimetype().to_vec();
        if first.mimetype[0] != 0 {
            w.extend_from_slice(b"; ");
        }
        w.extend_from_slice(if first.desc[0] != 0 { first.desc() } else { b"(no description)" });
        w.extend_from_slice(b": ");
        w.extend_from_slice(if first.flag & BINTEST != 0 { b"binary" } else { b"text" });
        w.push(b'\n');
        if first.flag & BINTEST != 0 {
            let d = first.desc();
            if let Some(p) = d.windows(4).position(|x| x == b"text") {
                let before_ok = p == 0 || isspace(d[p - 1]);
                let after = d.get(p + 4).copied().unwrap_or(0);
                if before_ok && (p + 5 == MAXSTRING || after == 0 || isspace(after)) {
                    w.extend_from_slice(b"*** Possible binary test for text type\n");
                }
            }
        }
        use std::io::Write;
        let _written = std::io::stderr().write_all(&w);
    }
}

/// `set_last_default`: a level-0 `default` that did not sort last is worth a
/// warning.
fn set_last_default(ms: &mut Ms, entries: &[Entry]) {
    let n = entries.len();
    let mut i = 0usize;
    while i < n {
        let first = entries[i].mp.first();
        if first.is_some_and(|m| m.cont_level == 0 && m.typ == FILE_DEFAULT) {
            i += 1;
            while i < n {
                if entries[i].mp.first().is_some_and(|m| m.cont_level == 0) {
                    break;
                }
                i += 1;
            }
            if i != n {
                ms.line = entries[i].mp.first().map_or(0, |m| m.lineno as usize);
                ms.magwarn(b"level 0 \"default\" did not sort last");
            }
            return;
        }
        i += 1;
    }
}

/// One loaded database: its rules, per set, in order (`struct magic_map`).
#[derive(Default, Debug)]
pub struct MagicMap {
    pub magic: [Vec<Magic>; MAGIC_SETS],
}

/// The loaded entries sorted and flattened, as `apprentice_load` finishes.
fn finish_load(ms: &mut Ms, mut mset: EntrySets) -> MagicMap {
    let mut map = MagicMap::default();
    for (j, entries) in mset.sets.iter_mut().enumerate() {
        set_text_binary(ms, entries);
        // glibc's `qsort` is a merge sort here, and so stable: entries of
        // equal strength keep their load order. (The strength is the same on
        // every call, so each entry's is worked out once.)
        entries.sort_by_cached_key(|e| std::cmp::Reverse(e.mp.first().map_or(0, file_magic_strength)));
        set_last_default(ms, entries);
        map.magic[j] = std::mem::take(entries).into_iter().flat_map(|e| e.mp).collect();
    }
    map
}

/// A path from bytes.
#[must_use]
pub fn os_path(p: &[u8]) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(p))
    }
    #[cfg(not(unix))]
    {
        // A host build's paths are UTF-16: bytes that are not UTF-8 name no
        // file there, and the empty path that stands for them opens none.
        std::str::from_utf8(p).map(std::path::PathBuf::from).unwrap_or_default()
    }
}

/// An `OsStr` as bytes.
#[must_use]
pub fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        s.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        // WTF-8 on Windows: lossless, and what `os_path` turns back.
        s.as_encoded_bytes().to_vec()
    }
}

/// Whether `fn_` is the default path standing for the built-in database:
/// it is, and there is nothing at it on disk.
fn is_builtin(fn_: &[u8]) -> bool {
    fn_ == MAGIC.as_bytes() && std::fs::symlink_metadata(os_path(fn_)).is_err()
}

/// `apprentice_load`: a magic file -- or a directory of them, read in `strcmp`
/// order -- parsed, typed and sorted. `None` if any file had an error.
pub fn apprentice_load(ms: &mut Ms, fn_: &[u8], action: Action) -> Option<MagicMap> {
    let mut errs = 0usize;
    let mut mset = EntrySets::default();
    // Enable checks for parsed files.
    ms.flags |= MAGIC_CHECK;
    if action == Action::Check {
        use std::io::Write;
        let mut w = USG_HDR.to_vec();
        w.push(b'\n');
        let _written = std::io::stderr().write_all(&w);
    }
    if is_builtin(fn_) {
        // The database upstream compiles from the directory these came from.
        for (name, text) in crate::magdir::FRAGMENTS {
            let mut path = fn_.to_vec();
            path.push(b'/');
            path.extend_from_slice(name.as_bytes());
            load_1(ms, action, &path, text, &mut errs, &mut mset);
        }
    } else {
        match std::fs::metadata(os_path(fn_)) {
            Ok(md) if md.is_dir() => {
                let Ok(rd) = std::fs::read_dir(os_path(fn_)) else {
                    return None;
                };
                let mut files: Vec<Vec<u8>> = Vec::new();
                for d in rd.flatten() {
                    let name = os_bytes(&d.file_name());
                    if name.first() == Some(&b'.') {
                        continue;
                    }
                    let mut mfn = fn_.to_vec();
                    mfn.push(b'/');
                    mfn.extend_from_slice(&name);
                    match std::fs::metadata(os_path(&mfn)) {
                        Ok(st) if st.is_file() => files.push(mfn),
                        _ => {}
                    }
                }
                files.sort();
                for f in &files {
                    load_one_file(ms, action, f, &mut errs, &mut mset);
                }
            }
            _ => load_one_file(ms, action, fn_, &mut errs, &mut mset),
        }
    }
    if errs != 0 {
        return None;
    }
    Some(finish_load(ms, mset))
}

/// `load_1`'s `fopen`: a file that cannot be read is an error, and one that
/// does not exist is only an error count.
fn load_one_file(ms: &mut Ms, action: Action, f: &[u8], errs: &mut usize, mset: &mut EntrySets) {
    ms.file = Some(f.to_vec());
    match std::fs::read(os_path(f)) {
        Ok(text) => load_1(ms, action, f, &text, errs, mset),
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                let mut msg = b"cannot read magic file `".to_vec();
                msg.extend_from_slice(f);
                msg.push(b'\'');
                ms.error(Some(&e), &msg);
            }
            ms.errno = Some(crate::funcs::Errno::of(&e));
            *errs += 1;
        }
    }
}

/// `mkdbname`: the compiled file for a magic path -- `fn.mgc`, unless it
/// already ends `.mgc`; when compiling, in the current directory.
fn mkdbname(ms: &mut Ms, fn_: &[u8], strip: bool) -> Vec<u8> {
    let mut f = fn_;
    if strip {
        if let Some(p) = f.iter().rposition(|&c| c == b'/') {
            f = &f[p + 1..];
        }
    }
    let base: &[u8] = f.strip_suffix(b".mgc").unwrap_or(f);
    // Compatibility with old code that looked in `.mime`.
    if ms.flags & MAGIC_MIME != 0 {
        let mut m = base.to_vec();
        m.extend_from_slice(b".mime.mgc");
        if std::fs::File::open(os_path(&m)).is_ok() {
            ms.flags &= MAGIC_MIME_TYPE;
            return m;
        }
    }
    let mut out = base.to_vec();
    out.extend_from_slice(b".mgc");
    if f.windows(5).any(|w| w == b".mime") {
        ms.flags &= MAGIC_MIME_TYPE;
    }
    out
}

/// `apprentice_map`: a compiled database, or `None`.
fn apprentice_map(ms: &mut Ms, fn_: &[u8]) -> Option<MagicMap> {
    let dbname = mkdbname(ms, fn_, false);
    let data = match std::fs::read(os_path(&dbname)) {
        Ok(d) => d,
        Err(e) => {
            ms.errno = Some(crate::funcs::Errno::of(&e));
            return None;
        }
    };
    if data.len() < 8 {
        let mut msg = b"file `".to_vec();
        msg.extend_from_slice(&dbname);
        msg.extend_from_slice(b"' is too small");
        ms.error(None, &msg);
        return None;
    }
    check_buffer(ms, &data, &dbname)
}

/// `check_buffer`: the header, the version and the counts of a compiled
/// database, and its rules.
fn check_buffer(ms: &mut Ms, data: &[u8], dbname: &[u8]) -> Option<MagicMap> {
    let word = |i: usize| {
        data.get(i..i + 4)
            .map_or(0, |w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
    };
    let swap = if word(0) == MAGICNO {
        false
    } else if word(0).swap_bytes() == MAGICNO {
        true
    } else {
        let mut msg = b"bad magic in `".to_vec();
        msg.extend_from_slice(dbname);
        msg.push(b'\'');
        ms.error(None, &msg);
        return None;
    };
    let fix = |v: u32| if swap { v.swap_bytes() } else { v };
    let version = fix(word(4));
    if version != VERSIONNO {
        let mut msg = format!("File 5.45 supports only version {VERSIONNO} magic files. `").into_bytes();
        msg.extend_from_slice(dbname);
        msg.extend_from_slice(format!("' is version {version}").as_bytes());
        ms.error(None, &msg);
        return None;
    }
    let entries = data.len() / FILE_MAGICSIZE;
    if entries * FILE_MAGICSIZE != data.len() {
        let mut msg = b"Size of `".to_vec();
        msg.extend_from_slice(dbname);
        msg.extend_from_slice(format!("' {} is not a multiple of {FILE_MAGICSIZE}", data.len()).as_bytes());
        ms.error(None, &msg);
        return None;
    }
    let n0 = fix(word(8)) as usize;
    let n1 = fix(word(12)) as usize;
    if entries != n0.wrapping_add(n1).wrapping_add(1) {
        let mut msg = b"Inconsistent entries in `".to_vec();
        msg.extend_from_slice(dbname);
        msg.extend_from_slice(format!("' {entries} != {}", n0.wrapping_add(n1).wrapping_add(1)).as_bytes());
        ms.error(None, &msg);
        return None;
    }
    let rule = |k: usize| {
        let at = (k + 1) * FILE_MAGICSIZE;
        let mut b = [0u8; FILE_MAGICSIZE];
        if let Some(src) = data.get(at..at + FILE_MAGICSIZE) {
            b.copy_from_slice(src);
        }
        Magic::from_bytes(&b, swap)
    };
    let mut map = MagicMap::default();
    map.magic[0] = (0..n0).map(rule).collect();
    map.magic[1] = (n0..n0 + n1).map(rule).collect();
    Some(map)
}

/// `apprentice_compile`: write `map` as `fn`'s compiled database, in the
/// current directory.
fn apprentice_compile(ms: &mut Ms, map: &MagicMap, fn_: &[u8]) -> i32 {
    let dbname = mkdbname(ms, fn_, true);
    let mut out = vec![0u8; FILE_MAGICSIZE];
    out[0..4].copy_from_slice(&MAGICNO.to_le_bytes());
    out[4..8].copy_from_slice(&VERSIONNO.to_le_bytes());
    #[allow(clippy::cast_possible_truncation)]
    {
        out[8..12].copy_from_slice(&(map.magic[0].len() as u32).to_le_bytes());
        out[12..16].copy_from_slice(&(map.magic[1].len() as u32).to_le_bytes());
    }
    for set in &map.magic {
        for m in set {
            out.extend_from_slice(&m.to_bytes());
        }
    }
    if let Err(e) = std::fs::write(os_path(&dbname), &out) {
        let mut msg = b"cannot open `".to_vec();
        msg.extend_from_slice(&dbname);
        msg.push(b'\'');
        ms.error(Some(&e), &msg);
        return -1;
    }
    0
}

/// `apprentice_1`: one file or directory of the magic path.
fn apprentice_1(ms: &mut Ms, fn_: &[u8], action: Action) -> i32 {
    if action == Action::Compile {
        let quiet = ms.quiet;
        let Some(map) = apprentice_load(ms, fn_, action) else {
            return -1;
        };
        ms.quiet = quiet;
        return apprentice_compile(ms, &map, fn_);
    }
    let errno = ms.errno;
    let map = match apprentice_map(ms, fn_) {
        Some(m) => m,
        None => {
            let builtin = is_builtin(fn_);
            // The built-in database stands for a compiled one, which upstream
            // maps without this warning.
            if ms.flags & MAGIC_CHECK != 0 && !builtin {
                let mut w = b"using regular magic file `".to_vec();
                w.extend_from_slice(fn_);
                w.push(b'\'');
                ms.magwarn_bare(&w);
            }
            // The built-in database is parsed as upstream's build compiled it:
            // checked, quietly, and leaving no trace in the flags, the
            // warnings' file name or `errno` -- mapping a compiled one, which
            // is what it stands for, touches none of them.
            let (flags, quiet, file) = (ms.flags, ms.quiet, ms.file.clone());
            if builtin {
                ms.quiet = true;
            }
            let loaded = apprentice_load(ms, fn_, action);
            if builtin {
                ms.flags = flags;
                ms.quiet = quiet;
                ms.file = file;
                ms.errno = errno;
            }
            match loaded {
                Some(m) => m,
                None => return -1,
            }
        }
    };
    for (i, set) in map.magic.into_iter().enumerate() {
        if let Some(list) = ms.mlist[i].as_mut() {
            list.push(Rc::new(MList::new(set)));
        }
    }
    if action == Action::List {
        let mut out = Vec::new();
        for i in 0..MAGIC_SETS {
            out.extend_from_slice(format!("Set {i}:\nBinary patterns:\n").as_bytes());
            apprentice_list(ms, i, BINTEST, &mut out);
            out.extend_from_slice(b"Text patterns:\n");
            apprentice_list(ms, i, TEXTTEST, &mut out);
        }
        crate::out::write(&out);
    }
    0
}

/// `magic_getpath`: the magic path -- the one given, else `$MAGIC`, else the
/// default.
#[must_use]
pub fn magic_getpath(magicfile: Option<&[u8]>, action: Action) -> Vec<u8> {
    if let Some(m) = magicfile {
        return m.to_vec();
    }
    if let Some(m) = std::env::var_os("MAGIC") {
        return os_bytes(&m);
    }
    if action == Action::Load {
        get_default_magic()
    } else {
        MAGIC.as_bytes().to_vec()
    }
}

/// `get_default_magic`: `~/.magic.mgc`, or `~/.magic` (a file, or a directory
/// holding `magic.mgc`), before the default -- when there is one.
fn get_default_magic() -> Vec<u8> {
    let Some(home) = std::env::var_os("HOME") else {
        return MAGIC.as_bytes().to_vec();
    };
    let home = os_bytes(&home);
    let mut hm = home.clone();
    hm.extend_from_slice(b"/.magic.mgc");
    if std::fs::metadata(os_path(&hm)).is_err() {
        hm = home.clone();
        hm.extend_from_slice(b"/.magic");
        match std::fs::metadata(os_path(&hm)) {
            Err(_) => return MAGIC.as_bytes().to_vec(),
            Ok(st) if st.is_dir() => {
                hm = home;
                hm.extend_from_slice(b"//.magic/magic.mgc");
                if std::fs::File::open(os_path(&hm)).is_err() {
                    return MAGIC.as_bytes().to_vec();
                }
            }
            Ok(_) => {}
        }
    }
    let mut out = hm;
    out.push(PATHSEP);
    out.extend_from_slice(MAGIC.as_bytes());
    out
}

/// `file_apprentice`: load every file of the magic path.
pub fn file_apprentice(ms: &mut Ms, magicfile: Option<&[u8]>, action: Action) -> i32 {
    ms.reset(false);
    let path = magic_getpath(magicfile, action);
    for slot in &mut ms.mlist {
        *slot = Some(Vec::new());
    }
    let mut errs: i32 = -1;
    for part in path.split(|&c| c == PATHSEP) {
        if part.is_empty() {
            break;
        }
        let fileerr = apprentice_1(ms, part, action);
        errs = errs.max(fileerr);
    }
    if errs == -1 {
        ms.mlist.fill(None);
        ms.error(None, b"could not find any valid magic files!");
        return -1;
    }
    0
}

/// `apprentice_list`: `file -l`'s listing of one set, for one kind of test --
/// every database loaded so far, in load order.
fn apprentice_list(ms: &Ms, set: usize, mode: u8, out: &mut Vec<u8>) {
    let Some(lists) = ms.mlist.get(set).and_then(Option::as_ref) else {
        return;
    };
    for ml in lists {
        let magic = &ml.magic;
        let n = magic.len();
        let mut magindex = 0usize;
        while magindex < n {
            let m = &magic[magindex];
            if m.flag & mode != mode {
                // Skip the sub-tests.
                while magindex + 1 < n && magic[magindex + 1].cont_level != 0 {
                    magindex += 1;
                }
                magindex += 1;
                continue;
            }
            // Walk the tree until an item with a description and a MIME type.
            let lineindex = magindex;
            let mut descindex = magindex;
            let mut mimeindex = magindex;
            while magindex + 1 < n && magic[magindex + 1].cont_level != 0 {
                let mi = magindex + 1;
                if magic[descindex].desc[0] == 0 && magic[mi].desc[0] != 0 {
                    descindex = mi;
                }
                if magic[mimeindex].mimetype[0] == 0 && magic[mi].mimetype[0] != 0 {
                    mimeindex = mi;
                }
                magindex += 1;
            }
            out.extend_from_slice(
                format!("Strength = {:3}@{}: ", file_magic_strength(m), magic[lineindex].lineno).as_bytes(),
            );
            out.extend_from_slice(magic[descindex].desc());
            out.extend_from_slice(b" [");
            out.extend_from_slice(magic[mimeindex].mimetype());
            out.extend_from_slice(b"]\n");
            magindex += 1;
        }
    }
}

/// `file_magicfind`: the `name` entry a `use` names -- its list, and where in
/// it the entry starts and how many rules it has.
#[must_use]
pub fn file_magicfind(ms: &Ms, name: &[u8]) -> Option<(Rc<MList>, usize, usize)> {
    let lists = ms.mlist[1].as_ref()?;
    for ml in lists {
        let ma = &ml.magic;
        for (i, m) in ma.iter().enumerate() {
            if m.typ != FILE_NAME || m.value.s() != name {
                continue;
            }
            let mut j = i + 1;
            while j < ma.len() && ma[j].cont_level != 0 {
                j += 1;
            }
            return Some((Rc::clone(ml), i, j - i));
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn load(text: &str) -> (Vec<Magic>, usize) {
        let mut ms = Ms::new(MAGIC_CHECK);
        ms.quiet = true;
        let mut sets = EntrySets::default();
        let mut errs = 0;
        load_1(&mut ms, Action::Load, b"t", text.as_bytes(), &mut errs, &mut sets);
        let map = finish_load(&mut ms, sets);
        (map.magic[0].clone(), errs)
    }

    #[test]
    fn a_rule_reads_as_libmagic_reads_it() {
        let (m, errs) = load("0\tstring\t\\x7fELF\tELF\n>4\tbyte\t2\t64-bit\n");
        assert_eq!(errs, 0);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].typ, FILE_STRING);
        assert_eq!(m[0].value.s(), b"\x7fELF");
        assert_eq!(m[0].vallen, 4);
        assert_eq!(m[0].desc(), b"ELF");
        assert_eq!(m[1].cont_level, 1);
        assert_eq!(m[1].offset, 4);
        assert_eq!(m[1].value.q(), 2);
        assert_eq!(m[0].flag & BINTEST, BINTEST);
    }

    #[test]
    fn offsets_masks_and_indirection() {
        let (m, errs) = load("0\tbelong&0xfffffff0\t0x10\tx\n>(4.l+8)\tubyte\t>3\ty\n>>&2\tshort\t-1\tz\n");
        assert_eq!(errs, 0);
        // `file_signextend`: a signed long's mask is sign-extended.
        assert_eq!(m[0].num_mask(), 0xffff_ffff_ffff_fff0);
        assert_eq!(m[1].flag & INDIR, INDIR);
        assert_eq!(m[1].in_type, FILE_LELONG);
        assert_eq!(m[1].in_offset, 8);
        assert_eq!(m[1].flag & UNSIGNED, UNSIGNED);
        assert_eq!(m[1].reln, b'>');
        assert_eq!(m[2].flag & OFFADD, OFFADD);
        assert_eq!(m[2].value.q(), u64::MAX);
    }

    #[test]
    fn a_description_of_63_bytes_is_called_truncated_and_kept() {
        let desc = "d".repeat(63);
        let (m, _) = load(&format!("0\tbyte\t1\t{desc}\n"));
        assert_eq!(m[0].desc().len(), 63);
        let (m, _) = load(&format!("0\tbyte\t1\t{desc}xx\n"));
        assert_eq!(m[0].desc().len(), 63);
    }

    #[test]
    fn strength_sorts_stably() {
        let (m, _) = load("0\tbyte\t1\tone\n0\tbyte\t2\ttwo\n0\tstring\tabc\tthree\n");
        let descs: Vec<&[u8]> = m.iter().map(Magic::desc).collect();
        assert_eq!(descs, vec![&b"three"[..], b"one", b"two"]);
    }

    #[test]
    fn only_the_first_line_types_an_entry() {
        // A text `search` under a binary first line leaves the entry binary
        // only: the continuation is never looked at.
        let (m, _) = load("0\tbyte\t1\tone\n>0\tsearch/10\tabc\tx\n");
        assert_eq!(m[0].flag & (BINTEST | TEXTTEST), BINTEST);
        let (m, _) = load("0\tsearch/10\tabc\ttext\n");
        assert_eq!(m[0].flag & (BINTEST | TEXTTEST), TEXTTEST);
    }

    #[test]
    fn annotations_attach_to_the_last_line() {
        let (m, _) = load("0\tstring\tPK\tzip\n!:mime\tapplication/zip\n!:ext\tzip/jar\n");
        assert_eq!(m[0].mimetype(), b"application/zip");
        assert_eq!(m[0].ext(), b"zip/jar");
    }

    #[test]
    fn type_names_are_by_number_and_messages_by_position() {
        assert_eq!(file_name(FILE_BEID3), b"beid3");
        assert_eq!(file_name(FILE_LEID3), b"leid3");
        assert_eq!(type_tbl_name(FILE_BEID3), b"leid3");
    }

    #[test]
    fn guids_scan_as_sscanf_does() {
        let g = parse_guid(b"00112233-4455-6677-8899-aabbccddeeff").unwrap();
        assert_eq!(&g[..4], &[0x33, 0x22, 0x11, 0x00]);
        assert_eq!(&g[8..], &[0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        assert_eq!(parse_guid(b"0011-"), None);
    }

    #[test]
    fn a_compiled_rule_round_trips() {
        let (m, _) = load("0\tstring\tPK\tzip\n!:mime\tapplication/zip\n>4\tleshort\t>20\tv%d\n");
        for r in &m {
            assert_eq!(Magic::from_bytes(&r.to_bytes(), false).to_bytes(), r.to_bytes());
        }
    }
}
