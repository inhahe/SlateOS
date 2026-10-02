//! libmagic's `encoding.c`: which character code, if any, a buffer's bytes
//! read as -- ASCII, UTF-8 with or without its BOM, UTF-7's BOM, UTF-16 and
//! UTF-32 with theirs, ISO-8859, "extended ASCII", or one of those through
//! EBCDIC -- and the buffer decoded into one code point per character.
//!
//! The order of the tries is upstream's and decides the answer: a buffer that
//! is both valid UTF-8 and ISO-8859 is UTF-8. So are the quirks: a UTF-16
//! surrogate pair decodes to its high half *and* the pair, and a truncated
//! UTF-8 sequence at the end of the buffer is valid.

/// `file_unichar_t`: an `unsigned long`.
pub type Unichar = u64;

/// `F`: never in text.
const F: u8 = 0;
/// `T`: in plain ASCII text.
const T: u8 = 1;
/// `I`: in ISO-8859 text.
const I: u8 = 2;
/// `X`: in non-ISO extended ASCII (Mac, IBM PC) text.
const X: u8 = 3;

/// `text_chars`: what each byte says about the text it is in.
#[rustfmt::skip]
static TEXT_CHARS: [u8; 256] = [
    //                  BEL BS HT LF VT FF CR
    F, F, F, F, F, F, F, T, T, T, T, T, T, T, F, F,  // 0x0X
    //                              ESC
    F, F, F, F, F, F, F, F, F, F, F, T, F, F, F, F,  // 0x1X
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,  // 0x2X
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,  // 0x3X
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,  // 0x4X
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,  // 0x5X
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, T,  // 0x6X
    T, T, T, T, T, T, T, T, T, T, T, T, T, T, T, F,  // 0x7X
    //            NEL
    X, X, X, X, X, T, X, X, X, X, X, X, X, X, X, X,  // 0x8X
    X, X, X, X, X, X, X, X, X, X, X, X, X, X, X, X,  // 0x9X
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,  // 0xaX
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,  // 0xbX
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,  // 0xcX
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,  // 0xdX
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,  // 0xeX
    I, I, I, I, I, I, I, I, I, I, I, I, I, I, I, I,  // 0xfX
];

fn text_char(c: u8) -> u8 {
    TEXT_CHARS[usize::from(c)]
}

/// What [`file_encoding`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoding {
    /// Whether the buffer looks like text at all (the C function's result).
    pub looks_text: bool,
    /// `code`: "ASCII", "Unicode text, UTF-8" and so on, or "unknown".
    pub code: &'static str,
    /// `code_mime`: the charset for `--mime-encoding`.
    pub code_mime: &'static str,
    /// `type`: "text" or "binary".
    pub typ: &'static str,
    /// The decoded characters, one per element (`ubuf`, `ulen`).
    pub ubuf: Vec<Unichar>,
}

/// `file_encoding`: try each character code in upstream's order.
///
/// Only the first `encoding_max` bytes are looked at.
#[must_use]
pub fn file_encoding(buf: &[u8], encoding_max: usize) -> Encoding {
    let nbytes = buf.len().min(encoding_max);
    let buf = buf.get(..nbytes).unwrap_or(buf);
    let mut ubuf: Vec<Unichar> = Vec::with_capacity(nbytes.saturating_add(1));
    let mut enc = Encoding {
        looks_text: true,
        code: "unknown",
        code_mime: "binary",
        typ: "text",
        ubuf: Vec::new(),
    };
    if looks_ascii(buf.iter().copied(), &mut ubuf) {
        if looks_utf7(buf, &mut ubuf) > 0 {
            enc.code = "Unicode text, UTF-7";
            enc.code_mime = "utf-7";
        } else {
            enc.code = "ASCII";
            enc.code_mime = "us-ascii";
        }
    } else if looks_utf8_with_bom(buf, &mut ubuf) > 0 {
        enc.code = "Unicode text, UTF-8 (with BOM)";
        enc.code_mime = "utf-8";
    } else if file_looks_utf8(buf, Some(&mut ubuf)) > 1 {
        enc.code = "Unicode text, UTF-8";
        enc.code_mime = "utf-8";
    } else {
        let ucs32 = looks_ucs32(buf, &mut ubuf);
        if ucs32 != 0 {
            if ucs32 == 1 {
                enc.code = "Unicode text, UTF-32, little-endian";
                enc.code_mime = "utf-32le";
            } else {
                enc.code = "Unicode text, UTF-32, big-endian";
                enc.code_mime = "utf-32be";
            }
        } else {
            let ucs16 = looks_ucs16(buf, &mut ubuf);
            if ucs16 != 0 {
                if ucs16 == 1 {
                    enc.code = "Unicode text, UTF-16, little-endian";
                    enc.code_mime = "utf-16le";
                } else {
                    enc.code = "Unicode text, UTF-16, big-endian";
                    enc.code_mime = "utf-16be";
                }
            } else if looks_latin1(buf.iter().copied(), &mut ubuf) {
                enc.code = "ISO-8859";
                enc.code_mime = "iso-8859-1";
            } else if looks_extended(buf.iter().copied(), &mut ubuf) {
                enc.code = "Non-ISO extended-ASCII";
                enc.code_mime = "unknown-8bit";
            } else {
                // `from_ebcdic` into a second buffer, then the tests -- which
                // stop at the first byte that fails, so the bytes are
                // translated as they are read rather than all of them first.
                let nbuf = || buf.iter().map(|&b| from_ebcdic(b));
                if looks_ascii(nbuf(), &mut ubuf) {
                    enc.code = "EBCDIC";
                    enc.code_mime = "ebcdic";
                } else if looks_latin1(nbuf(), &mut ubuf) {
                    enc.code = "International EBCDIC";
                    enc.code_mime = "ebcdic";
                } else {
                    enc.looks_text = false;
                    enc.typ = "binary";
                }
            }
        }
    }
    enc.ubuf = ubuf;
    enc
}

/// The `LOOKS` macro: every byte of the buffer is in an allowed class. The
/// decoded characters are what was read before a failure, as in C.
fn looks(bytes: impl IntoIterator<Item = u8>, ubuf: &mut Vec<Unichar>, ok: impl Fn(u8) -> bool) -> bool {
    ubuf.clear();
    for b in bytes {
        if !ok(text_char(b)) {
            return false;
        }
        ubuf.push(Unichar::from(b));
    }
    true
}

fn looks_ascii(bytes: impl IntoIterator<Item = u8>, ubuf: &mut Vec<Unichar>) -> bool {
    looks(bytes, ubuf, |t| t == T)
}

fn looks_latin1(bytes: impl IntoIterator<Item = u8>, ubuf: &mut Vec<Unichar>) -> bool {
    looks(bytes, ubuf, |t| t == T || t == I)
}

fn looks_extended(bytes: impl IntoIterator<Item = u8>, ubuf: &mut Vec<Unichar>) -> bool {
    looks(bytes, ubuf, |t| t == T || t == I || t == X)
}

/// `XX`: an invalid first byte (size 1).
const XX: u8 = 0xF1;
/// `AS`: ASCII (size 1).
const AS: u8 = 0xF0;
const S1: u8 = 0x02;
const S2: u8 = 0x13;
const S3: u8 = 0x03;
const S4: u8 = 0x23;
const S5: u8 = 0x34;
const S6: u8 = 0x04;
const S7: u8 = 0x44;

/// `first`: about the first byte of a UTF-8 sequence (from Go's `utf8.go`).
#[rustfmt::skip]
static FIRST: [u8; 256] = [
    //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, // 0x00-0x0F
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, // 0x10-0x1F
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, // 0x20-0x2F
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, // 0x30-0x3F
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, // 0x40-0x4F
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, // 0x50-0x5F
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, // 0x60-0x6F
    AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, AS, // 0x70-0x7F
    XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, // 0x80-0x8F
    XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, // 0x90-0x9F
    XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, // 0xA0-0xAF
    XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, // 0xB0-0xBF
    XX, XX, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, // 0xC0-0xCF
    S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, S1, // 0xD0-0xDF
    S2, S3, S3, S3, S3, S3, S3, S3, S3, S3, S3, S3, S3, S4, S3, S3, // 0xE0-0xEF
    S5, S6, S6, S6, S7, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, XX, // 0xF0-0xFF
];

/// `accept_ranges`: the valid second bytes, by `first`'s high nibble. The C
/// array has sixteen entries "to avoid bounds checks"; the eleven it does not
/// spell out are zero, which accepts no byte.
static ACCEPT_RANGES: [(u8, u8); 16] = [
    (0x80, 0xBF),
    (0xA0, 0xBF),
    (0x80, 0x9F),
    (0x90, 0xBF),
    (0x80, 0x8F),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
];

/// `file_looks_utf8`: -1 invalid UTF-8, 0 valid but with odd control
/// characters, 1 seven-bit text, 2 UTF-8 with at least one high character.
/// Decodes into `ubuf` when one is given.
pub fn file_looks_utf8(buf: &[u8], mut ubuf: Option<&mut Vec<Unichar>>) -> i32 {
    if let Some(u) = ubuf.as_deref_mut() {
        u.clear();
    }
    let mut gotone = false;
    let mut ctrl = false;
    let n = buf.len();
    let mut i = 0usize;
    'outer: while i < n {
        let b = buf[i];
        if b & 0x80 == 0 {
            if text_char(b) != T {
                ctrl = true;
            }
            if let Some(u) = ubuf.as_deref_mut() {
                u.push(Unichar::from(b));
            }
        } else if b & 0x40 == 0 {
            return -1;
        } else {
            let x = FIRST[usize::from(b)];
            let ar = ACCEPT_RANGES[usize::from(x >> 4)];
            if x == XX {
                return -1;
            }
            let (mut c, following): (Unichar, usize) = if b & 0x20 == 0 {
                (Unichar::from(b & 0x1f), 1)
            } else if b & 0x10 == 0 {
                (Unichar::from(b & 0x0f), 2)
            } else if b & 0x08 == 0 {
                (Unichar::from(b & 0x07), 3)
            } else if b & 0x04 == 0 {
                (Unichar::from(b & 0x03), 4)
            } else if b & 0x02 == 0 {
                (Unichar::from(b & 0x01), 5)
            } else {
                return -1;
            };
            for k in 0..following {
                i += 1;
                if i >= n {
                    break 'outer;
                }
                let cb = buf[i];
                if k == 0 && (cb < ar.0 || cb > ar.1) {
                    return -1;
                }
                if cb & 0x80 == 0 || cb & 0x40 != 0 {
                    return -1;
                }
                c = (c << 6).wrapping_add(Unichar::from(cb & 0x3f));
            }
            if let Some(u) = ubuf.as_deref_mut() {
                u.push(c);
            }
            gotone = true;
        }
        i += 1;
    }
    if ctrl {
        0
    } else if gotone {
        2
    } else {
        1
    }
}

/// `looks_utf8_with_BOM`: -1 without the BOM, else [`file_looks_utf8`] of the
/// rest.
fn looks_utf8_with_bom(buf: &[u8], ubuf: &mut Vec<Unichar>) -> i32 {
    if buf.len() > 3 && buf[0] == 0xef && buf[1] == 0xbb && buf[2] == 0xbf {
        file_looks_utf8(&buf[3..], Some(ubuf))
    } else {
        -1
    }
}

/// `looks_utf7`: UTF-7's BOM, `+/v` and one of `89+/`.
fn looks_utf7(buf: &[u8], ubuf: &mut Vec<Unichar>) -> i32 {
    if buf.len() > 4 && buf[0] == b'+' && buf[1] == b'/' && buf[2] == b'v' {
        match buf[3] {
            b'8' | b'9' | b'+' | b'/' => {
                ubuf.clear();
                1
            }
            _ => -1,
        }
    } else {
        -1
    }
}

fn ucs16_nochar(c: u32) -> bool {
    (0xfdd0..=0xfdef).contains(&c)
}

fn ucs16_hisurr(c: u32) -> bool {
    (0xd800..=0xdbff).contains(&c)
}

fn ucs16_losurr(c: u32) -> bool {
    (0xdc00..=0xdfff).contains(&c)
}

/// `looks_ucs16`: 0 no, 1 little-endian, 2 big-endian -- by the BOM.
fn looks_ucs16(bf: &[u8], ubf: &mut Vec<Unichar>) -> i32 {
    let n = bf.len();
    if n < 2 {
        return 0;
    }
    let bigend = if bf[0] == 0xff && bf[1] == 0xfe {
        false
    } else if bf[0] == 0xfe && bf[1] == 0xff {
        true
    } else {
        return 0;
    };
    ubf.clear();
    let mut hi: u32 = 0;
    let mut i = 2usize;
    while i + 1 < n {
        let (a, b) = (u32::from(bf[i]), u32::from(bf[i + 1]));
        let mut uc = if bigend { b | (a << 8) } else { a | (b << 8) };
        uc &= 0xffff;
        if uc == 0xfffe || uc == 0xffff || ucs16_nochar(uc) {
            return 0;
        }
        if hi != 0 {
            if !ucs16_losurr(uc) {
                return 0;
            }
            uc = 0x10000 + 0x400 * (hi - 1) + (uc - 0xdc00);
            hi = 0;
        }
        if uc < 128 && text_char(u8::try_from(uc).unwrap_or(0)) != T {
            return 0;
        }
        ubf.push(Unichar::from(uc));
        if ucs16_hisurr(uc) {
            hi = uc - 0xd800 + 1;
        }
        if ucs16_losurr(uc) {
            return 0;
        }
        i += 2;
    }
    1 + i32::from(bigend)
}

/// `looks_ucs32`: 0 no, 1 little-endian, 2 big-endian -- by the BOM.
fn looks_ucs32(bf: &[u8], ubf: &mut Vec<Unichar>) -> i32 {
    let n = bf.len();
    if n < 4 {
        return 0;
    }
    let bigend = if bf[0] == 0xff && bf[1] == 0xfe && bf[2] == 0 && bf[3] == 0 {
        false
    } else if bf[0] == 0 && bf[1] == 0 && bf[2] == 0xfe && bf[3] == 0xff {
        true
    } else {
        return 0;
    };
    ubf.clear();
    let mut i = 4usize;
    while i + 3 < n {
        let w = [bf[i], bf[i + 1], bf[i + 2], bf[i + 3]];
        let c = Unichar::from(if bigend {
            u32::from_be_bytes(w)
        } else {
            u32::from_le_bytes(w)
        });
        ubf.push(c);
        if c == 0xfffe {
            return 0;
        }
        if c < 128 && text_char(u8::try_from(c).unwrap_or(0)) != T {
            return 0;
        }
        i += 4;
    }
    1 + i32::from(bigend)
}

/// `ebcdic_to_ascii`: the table from the rationale of POSIX.2 draft 11.2's
/// `dd`.
#[rustfmt::skip]
static EBCDIC_TO_ASCII: [u8; 256] = [
  0,   1,   2,   3, 156,   9, 134, 127, 151, 141, 142,  11,  12,  13,  14,  15,
 16,  17,  18,  19, 157, 133,   8, 135,  24,  25, 146, 143,  28,  29,  30,  31,
128, 129, 130, 131, 132,  10,  23,  27, 136, 137, 138, 139, 140,   5,   6,   7,
144, 145,  22, 147, 148, 149, 150,   4, 152, 153, 154, 155,  20,  21, 158,  26,
b' ', 160, 161, 162, 163, 164, 165, 166, 167, 168, 213, b'.', b'<', b'(', b'+', b'|',
b'&', 169, 170, 171, 172, 173, 174, 175, 176, 177, b'!', b'$', b'*', b')', b';', b'~',
b'-', b'/', 178, 179, 180, 181, 182, 183, 184, 185, 203, b',', b'%', b'_', b'>', b'?',
186, 187, 188, 189, 190, 191, 192, 193, 194, b'`', b':', b'#', b'@', b'\'', b'=', b'"',
195, b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', 196, 197, 198, 199, 200, 201,
202, b'j', b'k', b'l', b'm', b'n', b'o', b'p', b'q', b'r', b'^', 204, 205, 206, 207, 208,
209, 229, b's', b't', b'u', b'v', b'w', b'x', b'y', b'z', 210, 211, 212, b'[', 214, 215,
216, 217, 218, 219, 220, 221, 222, 223, 224, 225, 226, 227, 228, b']', 230, 231,
b'{', b'A', b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'I', 232, 233, 234, 235, 236, 237,
b'}', b'J', b'K', b'L', b'M', b'N', b'O', b'P', b'Q', b'R', 238, 239, 240, 241, 242, 243,
b'\\', 159, b'S', b'T', b'U', b'V', b'W', b'X', b'Y', b'Z', 244, 245, 246, 247, 248, 249,
b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', 250, 251, 252, 253, 254, 255,
];

/// `from_ebcdic`, a byte at a time.
fn from_ebcdic(b: u8) -> u8 {
    EBCDIC_TO_ASCII[usize::from(b)]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn code(buf: &[u8]) -> &'static str {
        file_encoding(buf, 65536).code
    }

    #[test]
    fn the_codes_are_tried_in_upstreams_order() {
        assert_eq!(code(b"hello\n"), "ASCII");
        assert_eq!(code(b"+/v8 x"), "Unicode text, UTF-7");
        assert_eq!(code("caf\u{e9}\n".as_bytes()), "Unicode text, UTF-8");
        assert_eq!(code(b"\xef\xbb\xbfhi\n"), "Unicode text, UTF-8 (with BOM)");
        assert_eq!(code(b"\xff\xfeh\0i\0"), "Unicode text, UTF-16, little-endian");
        assert_eq!(code(b"\xfe\xff\0h\0i"), "Unicode text, UTF-16, big-endian");
        assert_eq!(code(b"\xff\xfe\0\0h\0\0\0"), "Unicode text, UTF-32, little-endian");
        assert_eq!(code(b"caf\xe9\n"), "ISO-8859");
        assert_eq!(code(b"caf\x85\x90\n"), "Non-ISO extended-ASCII");
        // "HELLO" in EBCDIC with its line feed (0x25, '%') is Latin-1 text;
        // with its NEL (0x15), which no ASCII text holds, it is EBCDIC.
        assert_eq!(code(b"\xc8\xc5\xd3\xd3\xd6\x25"), "ISO-8859");
        assert_eq!(code(b"\xc8\xc5\xd3\xd3\xd6\x15"), "EBCDIC");
        let bin = file_encoding(b"\x00\x01\x02", 65536);
        assert!(!bin.looks_text);
        assert_eq!(bin.typ, "binary");
        assert_eq!(bin.code, "unknown");
    }

    #[test]
    fn utf8_says_how_it_looks() {
        assert_eq!(file_looks_utf8(b"abc", None), 1);
        assert_eq!(file_looks_utf8("\u{e9}".as_bytes(), None), 2);
        assert_eq!(file_looks_utf8(b"a\x01", None), 0);
        assert_eq!(file_looks_utf8(b"\x80", None), -1);
        assert_eq!(file_looks_utf8(b"\xc0\x80", None), -1);
        // A sequence cut off by the end of the buffer is not an error.
        assert_eq!(file_looks_utf8(b"ab\xe2\x82", None), 1);
        let mut u = Vec::new();
        assert_eq!(file_looks_utf8("x\u{20ac}".as_bytes(), Some(&mut u)), 2);
        assert_eq!(u, vec![0x78, 0x20ac]);
    }

    #[test]
    fn a_surrogate_pair_decodes_to_both_halves() {
        let mut u = Vec::new();
        // U+1F600 as UTF-16LE after the BOM.
        assert_eq!(looks_ucs16(b"\xff\xfe\x3d\xd8\x00\xde", &mut u), 1);
        assert_eq!(u, vec![0xd83d, 0x1f600]);
    }
}
