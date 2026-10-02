//! libmagic's `der.c`: the `der` rule type, which reads one ASN.1 DER element
//! -- its tag, its length, and for a few types its value -- and compares them
//! with the rule's `TAG[LEN][=VALUE]`.
//!
//! Upstream's reading is kept as it is, including where it is not DER's: a
//! tag in the high-number form loses its last byte, and a length must leave
//! a byte after it.

use crate::funcs::Ms;
use crate::magic::{MAGIC_DEBUG, Magic};

/// `DER_BAD`.
const DER_BAD: u32 = u32::MAX;

/// `der__tag`: the names a rule spells a universal tag with.
static DER_TAG: [&str; 37] = [
    "eoc", "bool", "int", "bit_str", "octet_str", "null", "obj_id", "obj_desc", "ext", "real",
    "enum", "embed", "utf8_str", "rel_oid", "time", "res2", "seq", "set", "num_str", "prt_str",
    "t61_str", "vid_str", "ia5_str", "utc_time", "gen_time", "gr_str", "vis_str", "gen_str",
    "univ_str", "char_str", "bmp_str", "date", "tod", "datetime", "duration", "oid-iri",
    "rel-oid-iri",
];

const DER_TAG_UTF8_STRING: u32 = 0x0c;
const DER_TAG_PRINTABLE_STRING: u32 = 0x13;
const DER_TAG_IA5_STRING: u32 = 0x16;
const DER_TAG_UTCTIME: u32 = 0x17;

/// `gettag`. In the high-number form the loop stops at the first byte under
/// 0x80 without adding it in, as upstream's does.
fn gettag(c: &[u8], p: &mut usize, l: usize) -> u32 {
    let at = |i: usize| c.get(i).copied().unwrap_or(0);
    if *p >= l {
        return DER_BAD;
    }
    let mut tag = u32::from(at(*p) & 0x1f);
    *p += 1;
    if tag != 0x1f {
        return tag;
    }
    if *p >= l {
        return DER_BAD;
    }
    while at(*p) >= 0x80 {
        tag = tag
            .wrapping_mul(128)
            .wrapping_add(u32::from(at(*p)))
            .wrapping_sub(0x80);
        *p += 1;
        if *p >= l {
            return DER_BAD;
        }
    }
    tag
}

/// `getlength`: the short form, or the long form's big-endian digits; the
/// length bytes must leave at least one byte after them, and the length must
/// fit what is left.
fn getlength(c: &[u8], p: &mut usize, l: usize) -> u32 {
    let at = |i: usize| c.get(i).copied().unwrap_or(0);
    if *p >= l {
        return DER_BAD;
    }
    let onebyte = at(*p) & 0x80 == 0;
    let digits = at(*p) & 0x7f;
    *p += 1;
    if *p + usize::from(digits) >= l {
        return DER_BAD;
    }
    if onebyte {
        return u32::from(digits);
    }
    let mut len: u64 = 0;
    for _ in 0..digits {
        len = (len << 8) | u64::from(at(*p));
        *p += 1;
    }
    let pp = *p as u64;
    if len > u64::from(u32::MAX) - pp.min(u64::from(u32::MAX)) || pp + len > l as u64 {
        return DER_BAD;
    }
    u32::try_from(len).unwrap_or(DER_BAD)
}

/// `der_tag`: a tag's name, or its number as `%#x`.
fn der_tag(tag: u32) -> Vec<u8> {
    match DER_TAG.get(tag as usize) {
        Some(name) if tag < 0x25 => name.as_bytes().to_vec(),
        // `%#x`; every tag here is past the table, so never zero.
        _ => format!("{tag:#x}").into_bytes(),
    }
}

/// `der_data`: an element's value as a rule compares it, written over `buf`
/// (128 bytes) -- the strings as text, a UTC time as a date, anything else
/// in hex. A value of no bytes in hex writes nothing, leaving what `buf` held.
fn der_data(buf: &mut Vec<u8>, tag: u32, q: &[u8], len: u32) {
    let len = len as usize;
    let d = |i: usize| q.get(i).copied().unwrap_or(0);
    match tag {
        DER_TAG_PRINTABLE_STRING | DER_TAG_UTF8_STRING | DER_TAG_IA5_STRING => {
            // `snprintf(buf, 128, "%.*s", len, q)`: up to a NUL, and 127 bytes.
            let s: Vec<u8> = q.iter().take(len).take_while(|&&c| c != 0).take(127).copied().collect();
            *buf = s;
            return;
        }
        DER_TAG_UTCTIME if len >= 12 => {
            let mut s = b"20".to_vec();
            s.extend_from_slice(&[d(0), d(1), b'-', d(2), d(3), b'-', d(4), d(5), b' ']);
            s.extend_from_slice(&[d(6), d(7), b':', d(8), d(9), b':', d(10), d(11)]);
            s.extend_from_slice(b" GMT");
            // A NUL among the digits ends the string `%c` wrote it into.
            let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            s.truncate(end.min(127));
            *buf = s;
            return;
        }
        _ => {}
    }
    // `snprintf(buf + z, blen - z, "%.2x", d[i])` while `z < blen - 2`: up to
    // 63 bytes, each overwriting from its own position, and the NUL after.
    for i in 0..len {
        let z = i << 1;
        if z < 126 {
            buf.truncate(z.min(buf.len()));
            while buf.len() < z {
                buf.push(0);
            }
            buf.extend_from_slice(format!("{:02x}", d(i)).as_bytes());
        }
    }
}

/// `der_offs`: the offset after this element's header -- and, for a
/// continuation, after the whole element at the level above, so the next rule
/// there reads the next element. -1 when it does not parse or does not fit.
pub fn der_offs(ms: &mut Ms, m: &Magic, s: &[u8], nbytes: usize) -> i32 {
    let start = ms.search.s.unwrap_or(0);
    let b = s.get(start..).unwrap_or_default();
    let len = if ms.search.s_len != 0 { ms.search.s_len } else { nbytes };
    let mut offs = 0usize;
    if gettag(b, &mut offs, len) == DER_BAD {
        return -1;
    }
    let tlen = getlength(b, &mut offs, len);
    if tlen == DER_BAD {
        return -1;
    }
    // `ms->offset + m->offset` is unsigned 32-bit arithmetic in C: the sum
    // wraps there before it is added to `offs`.
    #[allow(clippy::cast_sign_loss)]
    let base = ms.offset.wrapping_add(m.offset as u32);
    offs = offs.wrapping_add(base as usize);
    if m.cont_level != 0 {
        if offs.wrapping_add(tlen as usize) > nbytes {
            return -1;
        }
        let parent = usize::from(m.cont_level) - 1;
        if let Some(li) = ms.c.get_mut(parent) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            {
                li.off = offs.wrapping_add(tlen as usize) as i32;
            }
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let o = offs as i32;
    o
}

/// `der_cmp`: whether the element is the rule's `TAG[LEN][=VALUE]` -- 1, 0,
/// or -1 when it does not parse. A value of `x` matches any. A match on a
/// value leaves the value in `ms_value` for the description.
pub fn der_cmp(ms: &mut Ms, m: &Magic, s: &[u8]) -> i32 {
    let start = ms.search.s.unwrap_or(0);
    let b = s.get(start..).unwrap_or_default();
    let want = m.value.s();
    let len = ms.search.s_len;
    let mut offs = 0usize;
    let tag = gettag(b, &mut offs, len);
    if tag == DER_BAD {
        return -1;
    }
    let tlen = getlength(b, &mut offs, len);
    if tlen == DER_BAD {
        return -1;
    }
    let mut buf = der_tag(tag);
    if ms.flags & MAGIC_DEBUG != 0 {
        let at = format!("der_cmp: tag {:p} got=", b.as_ptr());
        debug_bytes(&[at.as_bytes(), &buf, b" exp=", crate::cstd::cstr(want), b"\n"]);
    }
    if !want.starts_with(&buf) {
        return 0;
    }
    let mut i = buf.len();
    loop {
        match want.get(i).copied().unwrap_or(0) {
            0 => return 1,
            b'=' => {
                i += 1;
                break;
            }
            c if c.is_ascii_digit() => {
                let mut slen: u64 = 0;
                while let Some(&d) = want.get(i).filter(|d| d.is_ascii_digit()) {
                    slen = slen.wrapping_mul(10).wrapping_add(u64::from(d - b'0'));
                    i += 1;
                }
                if ms.flags & MAGIC_DEBUG != 0 {
                    eprintln!("der_cmp: len {slen} {tlen}");
                }
                if u64::from(tlen) != slen {
                    return 0;
                }
            }
            _ => return 0,
        }
    }
    let rest = want.get(i..).unwrap_or_default();
    der_data(&mut buf, tag, b.get(offs..).unwrap_or_default(), tlen);
    if ms.flags & MAGIC_DEBUG != 0 {
        debug_bytes(&[b"der_cmp: data ", &buf, b" ", crate::cstd::cstr(rest), b"\n"]);
    }
    if buf != rest && rest != b"x" {
        return 0;
    }
    // `strlcpy(ms->ms_value.s, buf, sizeof(ms->ms_value.s))`.
    let n = buf.len().min(127);
    ms.ms_value.0[..n].copy_from_slice(&buf[..n]);
    ms.ms_value.0[n] = 0;
    1
}

/// Debugging output, as bytes: the strings are the file's and the rule's,
/// which need not be text.
fn debug_bytes(parts: &[&[u8]]) {
    use std::io::Write;
    let mut w = Vec::new();
    for p in parts {
        w.extend_from_slice(p);
    }
    // `-d` output that cannot be written has nowhere else to go.
    let _written = std::io::stderr().write_all(&w);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_lengths_read_as_upstream_reads_them() {
        let mut p = 0;
        assert_eq!(gettag(&[0x30, 0x03], &mut p, 2), 0x10);
        // High-number form: the final byte is not added.
        let mut p = 0;
        assert_eq!(gettag(&[0x1f, 0x81, 0x05, 0], &mut p, 4), 0x1f * 128 + 1);
        assert_eq!(p, 2);
        // The contents must leave a byte after them.
        let mut p = 0;
        assert_eq!(getlength(&[0x03, 1, 2, 3, 4], &mut p, 5), 3);
        let mut p = 0;
        assert_eq!(getlength(&[0x03, 1, 2, 3], &mut p, 4), DER_BAD);
        let mut p = 0;
        assert_eq!(getlength(&[0x03], &mut p, 1), DER_BAD);
        let mut p = 0;
        assert_eq!(getlength(&[0x82, 0x01, 0x00, 0], &mut p, 300), 256);
        assert_eq!(der_tag(0x10), b"seq");
        assert_eq!(der_tag(0x40), b"0x40");
    }

    #[test]
    fn values_are_written_as_rules_compare_them() {
        let mut buf = b"seq".to_vec();
        der_data(&mut buf, DER_TAG_UTCTIME, b"230102030405Z", 13);
        assert_eq!(buf, b"2023-01-02 03:04:05 GMT");
        let mut buf = b"int".to_vec();
        der_data(&mut buf, 2, &[0xde, 0xad], 2);
        assert_eq!(buf, b"dead");
        // No bytes in hex leaves the buffer as it was.
        let mut buf = b"null".to_vec();
        der_data(&mut buf, 5, &[], 0);
        assert_eq!(buf, b"null");
    }
}
