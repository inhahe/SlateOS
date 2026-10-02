//! libmagic's `print.c`: how dates, times and numbers a rule read are written
//! -- and `file_mdump`, the one-line form of a rule that `-c` and `-d` print.
//! (`file_magwarn`, also from here, is [`crate::funcs::Ms::magwarn`].)

use std::io::Write;

use crate::apprentice::file_name;
use crate::cdf_time::{MAX_CTIME, asctime, cdf_timestamp_to_timespec, with_local};
use crate::cstd::strtoull;
use crate::funcs::file_print_guid;
use crate::magic::*;
use crate::printf::{self, Arg};

/// `FILE_T_LOCAL`: the time is in the local zone.
pub const FILE_T_LOCAL: u32 = 1;
/// `FILE_T_WINDOWS`: the time is a Windows `FILETIME`.
pub const FILE_T_WINDOWS: u32 = 2;

/// `file_fmtdatetime`: a time as `asctime` writes it, without the newline;
/// `*Invalid datetime*` past the year 9999 or where `asctime` refuses.
#[must_use]
pub fn file_fmtdatetime(v: u64, flags: u32) -> Vec<u8> {
    #[allow(clippy::cast_possible_wrap)]
    let t: i64 = if flags & FILE_T_WINDOWS != 0 {
        // The conversion's failure is not checked upstream: its -1 is a time.
        cdf_timestamp_to_timespec(v as i64).unwrap_or(-1)
    } else {
        v as i64
    };
    let invalid = || b"*Invalid datetime*".to_vec();
    if t > MAX_CTIME {
        return invalid();
    }
    let tm = if flags & FILE_T_LOCAL != 0 {
        with_local(|z| z.localtime_r(t))
    } else {
        localtime::Zone::utc().localtime_r(t)
    };
    let Some(tm) = tm else {
        return invalid();
    };
    let Some(mut s) = asctime(&tm) else {
        return invalid();
    };
    if let Some(nl) = s.iter().position(|&c| c == b'\n') {
        s.truncate(nl);
    }
    s
}

/// `file_fmtdate`: an MS-DOS date as `strftime("%a, %b %d %Y")` writes it --
/// from a `struct tm` with no weekday, so always `Sun`, and with `?` for a
/// month out of range, as glibc writes one.
#[must_use]
pub fn file_fmtdate(v: u16) -> Vec<u8> {
    let mday = v & 0x1f;
    let mon = i32::from((v >> 5) & 0xf) - 1;
    let year = i32::from(v >> 9) + 80 + 1900;
    let mon: &[u8] = usize::try_from(mon)
        .ok()
        .and_then(|m| localtime::MON_ABBR.get(m).copied())
        .unwrap_or(b"?");
    let mut s = b"Sun, ".to_vec();
    s.extend_from_slice(mon);
    s.extend_from_slice(format!(" {mday:02} {year}").as_bytes());
    s
}

/// `file_fmttime`: an MS-DOS time as `strftime("%T")` writes it, fields out
/// of range and all.
#[must_use]
pub fn file_fmttime(v: u16) -> Vec<u8> {
    let sec = (v & 0x1f) * 2;
    let min = (v >> 5) & 0x3f;
    let hour = v >> 11;
    format!("{hour:02}:{min:02}:{sec:02}").into_bytes()
}

/// `file_fmtnum`: a numeral in `base` as a decimal, or `*Invalid number*` if
/// anything follows it or it overflows.
#[must_use]
pub fn file_fmtnum(us: &[u8], base: u32) -> Vec<u8> {
    let s = crate::cstd::cstr(us);
    let (val, used, overflow) = strtoull(s, base);
    if used != s.len() || overflow {
        return b"*Invalid number*".to_vec();
    }
    val.to_string().into_bytes()
}

/// `file_varint2uintmax_t`: a variable-length integer, seven bits a byte.
#[must_use]
pub fn file_varint2uintmax_t(us: &[u8], t: u8) -> u64 {
    let at = |i: usize| us.get(i).copied().unwrap_or(0);
    let mut x: u64 = 0;
    if t == FILE_LEVARINT {
        let mut c = 0usize;
        while at(c) != 0 {
            if at(c) & 0x80 == 0 {
                break;
            }
            c += 1;
        }
        loop {
            x |= u64::from(at(c) & 0x7f);
            x = x.wrapping_shl(7);
            if c == 0 {
                break;
            }
            c -= 1;
        }
    } else {
        let mut c = 0usize;
        while at(c) != 0 {
            x |= u64::from(at(c) & 0x7f);
            if at(c) & 0x80 == 0 {
                break;
            }
            x = x.wrapping_shl(7);
            c += 1;
        }
    }
    x
}

/// `file_fmtvarint`.
#[must_use]
pub fn file_fmtvarint(us: &[u8], t: u8) -> Vec<u8> {
    #[allow(clippy::cast_possible_wrap)]
    let v = file_varint2uintmax_t(us, t) as i64;
    v.to_string().into_bytes()
}

/// `file_showstr`: bytes as C escapes -- `\n`, `\t` and the rest by name,
/// anything else outside `040`-`0176` in octal. `len` of `None` is
/// `FILE_BADSIZE`: up to the NUL.
pub fn file_showstr(out: &mut Vec<u8>, s: &[u8], len: Option<usize>) {
    let n = match len {
        Some(n) => n,
        None => crate::cstd::cstrlen(s),
    };
    for i in 0..n {
        let c = s.get(i).copied().unwrap_or(0);
        if (0o40..=0o176).contains(&c) {
            out.push(c);
            continue;
        }
        out.push(b'\\');
        match c {
            0x07 => out.push(b'a'),
            0x08 => out.push(b'b'),
            0x0c => out.push(b'f'),
            b'\n' => out.push(b'n'),
            b'\r' => out.push(b'r'),
            b'\t' => out.push(b't'),
            0x0b => out.push(b'v'),
            _ => out.extend_from_slice(format!("{c:03o}").as_bytes()),
        }
    }
}

/// `optyp`: the operators' characters, by `FILE_OP*`.
const OPTYP: &[u8; 8] = b"&|^+-*/%";

/// `file_mdump`: a rule on one line of stderr, for `-c` and `-d`.
pub fn file_mdump(m: &Magic) {
    let mut w = format!("{}: ", m.lineno).into_bytes();
    w.extend_from_slice(&b">>>>>>>>"[..=usize::from(m.cont_level & 7)]);
    w.extend_from_slice(format!(" {}", m.offset).as_bytes());
    if m.flag & INDIR != 0 {
        w.push(b'(');
        if usize::from(m.in_type) < FILE_NAMES_SIZE {
            w.extend_from_slice(file_name(m.in_type));
        } else {
            w.extend_from_slice(b"*bad in_type*");
        }
        w.push(b',');
        if m.in_op & FILE_OPINVERSE != 0 {
            w.push(b'~');
        }
        w.push(OPTYP[usize::from(m.in_op & FILE_OPS_MASK)]);
        w.extend_from_slice(format!("{}),", m.in_offset).as_bytes());
    }
    w.push(b' ');
    if m.flag & UNSIGNED != 0 {
        w.push(b'u');
    }
    if usize::from(m.typ) < FILE_NAMES_SIZE {
        w.extend_from_slice(file_name(m.typ));
    } else {
        w.extend_from_slice(b"*bad type");
    }
    if m.mask_op & FILE_OPINVERSE != 0 {
        w.push(b'~');
    }
    if is_string(m.typ) {
        let f = m.str_flags();
        if f != 0 {
            w.push(b'/');
            for (bit, ch) in [
                (STRING_COMPACT_WHITESPACE, b'W'),
                (STRING_COMPACT_OPTIONAL_WHITESPACE, b'w'),
                (STRING_IGNORE_LOWERCASE, b'c'),
                (STRING_IGNORE_UPPERCASE, b'C'),
                (REGEX_OFFSET_START, b's'),
                (STRING_TEXTTEST, b't'),
                (STRING_BINTEST, b'b'),
                (PSTRING_1_LE, b'B'),
                (PSTRING_2_BE, b'H'),
                (PSTRING_2_LE, b'h'),
                (PSTRING_4_BE, b'L'),
                (PSTRING_4_LE, b'l'),
                (PSTRING_LENGTH_INCLUDES_ITSELF, b'J'),
            ] {
                if f & bit != 0 {
                    w.push(ch);
                }
            }
        }
        if m.str_range() != 0 {
            w.extend_from_slice(format!("/{}", m.str_range()).as_bytes());
        }
    } else {
        w.push(OPTYP[usize::from(m.mask_op & FILE_OPS_MASK)]);
        if m.num_mask() != 0 {
            w.extend_from_slice(format!("{:08x}", m.num_mask()).as_bytes());
        }
    }
    w.push(b',');
    w.push(m.reln);
    if m.reln != b'x' {
        #[allow(clippy::cast_possible_wrap)]
        match m.typ {
            FILE_BYTE | FILE_SHORT | FILE_LONG | FILE_LESHORT | FILE_LELONG | FILE_MELONG
            | FILE_BESHORT | FILE_BELONG | FILE_INDIRECT => {
                w.extend_from_slice(format!("{}", m.value.l() as i32).as_bytes());
            }
            FILE_BEQUAD | FILE_LEQUAD | FILE_QUAD | FILE_OFFSET => {
                w.extend_from_slice(format!("{}", m.value.q() as i64).as_bytes());
            }
            FILE_PSTRING | FILE_STRING | FILE_REGEX | FILE_BESTRING16 | FILE_LESTRING16
            | FILE_SEARCH => {
                file_showstr(&mut w, &m.value.0, Some(usize::from(m.vallen)));
            }
            FILE_DATE | FILE_LEDATE | FILE_BEDATE | FILE_MEDATE => {
                w.extend_from_slice(&file_fmtdatetime(u64::from(m.value.l()), 0));
                w.push(b',');
            }
            FILE_LDATE | FILE_LELDATE | FILE_BELDATE | FILE_MELDATE => {
                w.extend_from_slice(&file_fmtdatetime(u64::from(m.value.l()), FILE_T_LOCAL));
                w.push(b',');
            }
            FILE_QDATE | FILE_LEQDATE | FILE_BEQDATE => {
                w.extend_from_slice(&file_fmtdatetime(m.value.q(), 0));
                w.push(b',');
            }
            FILE_QLDATE | FILE_LEQLDATE | FILE_BEQLDATE => {
                w.extend_from_slice(&file_fmtdatetime(m.value.q(), FILE_T_LOCAL));
                w.push(b',');
            }
            FILE_QWDATE | FILE_LEQWDATE | FILE_BEQWDATE => {
                w.extend_from_slice(&file_fmtdatetime(m.value.q(), FILE_T_WINDOWS));
                w.push(b',');
            }
            FILE_FLOAT | FILE_BEFLOAT | FILE_LEFLOAT => {
                let s = printf::format(b"%G", &[Arg::F64(f64::from(m.value.f()))]).unwrap_or_default();
                w.extend_from_slice(&s);
            }
            FILE_DOUBLE | FILE_BEDOUBLE | FILE_LEDOUBLE => {
                let s = printf::format(b"%G", &[Arg::F64(m.value.d())]).unwrap_or_default();
                w.extend_from_slice(&s);
            }
            FILE_LEVARINT | FILE_BEVARINT => {
                w.extend_from_slice(&file_fmtvarint(&m.value.0, m.typ));
            }
            FILE_MSDOSDATE | FILE_BEMSDOSDATE | FILE_LEMSDOSDATE => {
                w.extend_from_slice(&file_fmtdate(m.value.h()));
                w.push(b',');
            }
            FILE_MSDOSTIME | FILE_BEMSDOSTIME | FILE_LEMSDOSTIME => {
                w.extend_from_slice(&file_fmttime(m.value.h()));
                w.push(b',');
            }
            FILE_OCTAL => w.extend_from_slice(&file_fmtnum(&m.value.0, 8)),
            FILE_DEFAULT => {}
            FILE_USE | FILE_NAME | FILE_DER => {
                w.push(b'\'');
                w.extend_from_slice(m.value.s());
                w.push(b'\'');
            }
            FILE_GUID => {
                let mut g = [0u8; 16];
                g.copy_from_slice(&m.value.0[..16]);
                w.extend_from_slice(&file_print_guid(&g));
            }
            t => w.extend_from_slice(format!("*bad type {t}*").as_bytes()),
        }
    }
    w.extend_from_slice(b",\"");
    w.extend_from_slice(m.desc());
    w.extend_from_slice(b"\"]\n");
    let _written = std::io::stderr().write_all(&w);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_asctimes() {
        assert_eq!(file_fmtdatetime(0, 0), b"Thu Jan  1 00:00:00 1970");
        assert_eq!(file_fmtdatetime(1_000_000_000, 0), b"Sun Sep  9 01:46:40 2001");
        assert_eq!(file_fmtdatetime(253_402_300_800, 0), b"*Invalid datetime*");
        // A negative quad date is a date before 1970.
        assert_eq!(file_fmtdatetime((-86_400i64) as u64, 0), b"Wed Dec 31 00:00:00 1969");
    }

    #[test]
    fn dos_dates_and_times_keep_their_odd_fields() {
        // 2023-03-15: year 43, month 3, day 15.
        assert_eq!(file_fmtdate((43 << 9) | (3 << 5) | 15), b"Sun, Mar 15 2023");
        assert_eq!(file_fmtdate(0), b"Sun, ? 00 1980");
        assert_eq!(file_fmttime((23 << 11) | (59 << 5) | 29), b"23:59:58");
        assert_eq!(file_fmttime(0xffff), b"31:63:62");
    }

    #[test]
    fn numbers_and_escapes() {
        assert_eq!(file_fmtnum(b"0777", 8), b"511");
        assert_eq!(file_fmtnum(b"", 8), b"0");
        assert_eq!(file_fmtnum(b"789", 8), b"*Invalid number*");
        let mut w = Vec::new();
        file_showstr(&mut w, b"a\n\x01\xff", Some(4));
        assert_eq!(w, b"a\\n\\001\\377");
        assert_eq!(file_varint2uintmax_t(&[0x81, 0x01], FILE_BEVARINT), 129);
    }
}
