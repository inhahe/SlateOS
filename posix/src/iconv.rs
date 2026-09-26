// The arithmetic here is on byte counts bounded by the caller's buffers and
// on code points below 2^31; each subtraction is guarded by the comparison
// before it.
#![allow(clippy::arithmetic_side_effects)]

//! POSIX character set conversion (`<iconv.h>`): `iconv_open`, `iconv`,
//! `iconv_close`, with glibc 2.39's semantics for the character sets this
//! libc converts -- UTF-8, ASCII and ISO-8859-1 (Latin-1).
//!
//! # What glibc does, and so what this does
//!
//! Every conversion decodes a character from the source and encodes it for the
//! target, and stops at the first one that cannot go through, leaving the
//! caller's pointers and counts at the start of it:
//!
//! | the character | `errno` | unless the target named |
//! |---|---|---|
//! | is not valid in the source (a stray byte, an overlong or surrogate UTF-8 form, a byte above 0x7F in ASCII) | `EILSEQ` | `//IGNORE`: it is skipped |
//! | is cut off by the end of the input | `EINVAL` | -- (never skipped) |
//! | cannot be written in the target | `EILSEQ` | `//TRANSLIT`: a substitute is written; `//IGNORE`: it is skipped |
//! | does not fit in the output | `E2BIG` | -- |
//!
//! `//TRANSLIT` substitutes what glibc's C-locale table does -- `"EUR"` for
//! the euro sign, `"(C)"` for the copyright sign, `"ss"` for sharp s
//! ([`crate::iconv_translit`]) -- and `?` for anything it does not list, and
//! each substitution counts as one irreversible conversion, which is what a
//! successful `iconv` returns.  A conversion that skipped anything under
//! `//IGNORE` still converts all the rest, then fails with `EILSEQ`, as
//! glibc's does.  Options follow the target's name, `//` or `,` apart and in
//! any case; unknown ones are ignored, and the source's are ignored
//! altogether.
//!
//! glibc's UTF-8 is the old, wider one: up to six bytes and U+7FFFFFFF, with
//! overlong forms and surrogates refused.  So is this.
//!
//! Probed against Ubuntu 24.04's glibc 2.39 on 2026-09-26; the tests below
//! pin what it answered.
//!
//! # Until 2026-09-26
//!
//! Converting to ASCII replaced what it could not write with `?` and counted
//! it, where glibc refuses with `EILSEQ` unless asked to transliterate; the
//! "identity" conversions copied bytes, where glibc validates them, so invalid
//! UTF-8 went through a UTF-8 to UTF-8 conversion unchanged; `//TRANSLIT` and
//! `//IGNORE` made the name unrecognised; a NULL name was `EINVAL` where glibc
//! faults; the reset call and `iconv_close` accepted any descriptor
//! (`B-D-ICONV-WAS-NOT-GLIBCS`).
//!
//! # Descriptors
//!
//! An `iconv_t` is a pointer-sized handle a program only compares with
//! `(iconv_t)-1` and hands back.  No state survives between calls for these
//! character sets, so the handle is the conversion itself: [`DESCRIPTOR_TAG`]
//! with the two character sets and the target's options packed below it.
//! Anything without the tag is refused with `EBADF`.

use crate::errno;

/// Opaque conversion descriptor (see the module docs).
pub type IconvT = isize;

/// Error return from `iconv_open`.
pub const ICONV_OPEN_ERR: IconvT = -1;

/// The character sets this libc converts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Charset {
    Utf8 = 0,
    Ascii = 1,
    Latin1 = 2,
}

impl Charset {
    fn from_index(i: isize) -> Option<Self> {
        match i {
            0 => Some(Self::Utf8),
            1 => Some(Self::Ascii),
            2 => Some(Self::Latin1),
            _ => None,
        }
    }
}

/// `//TRANSLIT` on the target.
const TRANSLIT: isize = 1;
/// `//IGNORE` on the target.
const IGNORE: isize = 2;

/// The high bits every descriptor carries.  Positive and far from -1, so a
/// descriptor can never be mistaken for the error return.
const DESCRIPTOR_TAG: isize = 0x1C0_0000;
/// The bits below the tag: source, target and options.
const DESCRIPTOR_FIELDS: isize = 0xFFF;

/// A conversion: what [`iconv`] needs to know about a descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Conversion {
    from: Charset,
    to: Charset,
    flags: isize,
}

impl Conversion {
    fn descriptor(self) -> IconvT {
        DESCRIPTOR_TAG | (self.from as isize) | ((self.to as isize) << 4) | (self.flags << 8)
    }

    fn from_descriptor(cd: IconvT) -> Option<Self> {
        if (cd & !DESCRIPTOR_FIELDS) != DESCRIPTOR_TAG {
            return None;
        }
        let flags = (cd >> 8) & 0xF;
        if flags & !(TRANSLIT | IGNORE) != 0 {
            return None;
        }
        Some(Self {
            from: Charset::from_index(cd & 0xF)?,
            to: Charset::from_index((cd >> 4) & 0xF)?,
            flags,
        })
    }
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// glibc's names for the three character sets (gconv-modules' aliases), in
/// the form [`normalise`] brings a name to: upper case, `-` and `_` removed.
/// Removing those is more lenient than glibc, which lists each spelling; it
/// accepts no name glibc would read as another character set.
const UTF8_NAMES: &[&[u8]] = &[b"UTF8", b"ISO10646/UTF8/", b"ISO10646/UTF8"];
const ASCII_NAMES: &[&[u8]] = &[
    b"ANSIX3.41968",
    b"ANSIX3.41986",
    b"ISOIR6",
    b"ISO646.IRV:1991",
    b"ASCII",
    b"ISO646US",
    b"USASCII",
    b"US",
    b"IBM367",
    b"CP367",
    b"CSASCII",
    b"OSF00010020",
];
const LATIN1_NAMES: &[&[u8]] = &[
    b"ISO88591",
    b"ISO88591:1987",
    b"ISOIR100",
    b"LATIN1",
    b"L1",
    b"IBM819",
    b"CP819",
    b"CSISOLATIN1",
    b"OSF00010001",
];

/// Upper case with `-` and `_` removed, into `buf`; `None` if too long to be
/// any name above.
fn normalise<'b>(name: &[u8], buf: &'b mut [u8; 32]) -> Option<&'b [u8]> {
    let mut n = 0;
    for &b in name {
        if b == b'-' || b == b'_' {
            continue;
        }
        *buf.get_mut(n)? = b.to_ascii_uppercase();
        n += 1;
    }
    buf.get(..n)
}

/// A name with its options: the character set, and `TRANSLIT`/`IGNORE`.
///
/// The name ends at the first `//`; the options after it are separated by
/// `//` or `,`, compared without regard to case, and unknown ones are
/// ignored, as glibc ignores them.  An empty name is the locale's character
/// set, which in the C locale -- this libc's only one -- is ASCII
/// (`nl_langinfo(CODESET)` says `ANSI_X3.4-1968`).
fn parse_name(spec: &[u8]) -> Option<(Charset, isize)> {
    let (name, options) = match spec.windows(2).position(|w| w == b"//") {
        Some(i) => (spec.get(..i)?, spec.get(i + 2..)?),
        None => (spec, &b""[..]),
    };
    let charset = if name.is_empty() {
        Charset::Ascii
    } else {
        let mut buf = [0u8; 32];
        let norm = normalise(name, &mut buf)?;
        if UTF8_NAMES.contains(&norm) {
            Charset::Utf8
        } else if ASCII_NAMES.contains(&norm) {
            Charset::Ascii
        } else if LATIN1_NAMES.contains(&norm) {
            Charset::Latin1
        } else {
            return None;
        }
    };
    let mut flags = 0;
    for option in options.split(|&b| b == b'/' || b == b',') {
        if option.eq_ignore_ascii_case(b"TRANSLIT") {
            flags |= TRANSLIT;
        } else if option.eq_ignore_ascii_case(b"IGNORE") {
            flags |= IGNORE;
        }
    }
    Some((charset, flags))
}

/// View a NUL-terminated C string as a byte slice (excluding the NUL).
///
/// # Safety
///
/// `p` must be non-null and point to a valid NUL-terminated string.
unsafe fn cstr_slice<'a>(p: *const u8) -> &'a [u8] {
    // SAFETY: the caller's contract.
    let len = unsafe { crate::string::strlen(p) };
    // SAFETY: `p` is valid for `len` bytes, per the strlen scan above.
    unsafe { core::slice::from_raw_parts(p, len) }
}

/// Open a conversion from `fromcode` to `tocode`.
///
/// `(iconv_t)-1` with `EINVAL` when either names no character set this libc
/// converts, and with `EFAULT` for a NULL name, where glibc faults reading it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iconv_open(tocode: *const u8, fromcode: *const u8) -> IconvT {
    if tocode.is_null() || fromcode.is_null() {
        errno::set_errno(errno::EFAULT);
        return ICONV_OPEN_ERR;
    }
    // SAFETY: both non-NULL (checked) and, per the C contract, NUL-terminated.
    let (to_spec, from_spec) = unsafe { (cstr_slice(tocode), cstr_slice(fromcode)) };
    // The source's options are parsed and dropped: glibc applies only the
    // target's.
    match (parse_name(from_spec), parse_name(to_spec)) {
        (Some((from, _)), Some((to, flags))) => Conversion { from, to, flags }.descriptor(),
        _ => {
            errno::set_errno(errno::EINVAL);
            ICONV_OPEN_ERR
        }
    }
}

// ---------------------------------------------------------------------------
// Decoding and encoding one character
// ---------------------------------------------------------------------------

/// Why the next input character cannot be read.
#[derive(Debug, PartialEq, Eq)]
enum DecodeError {
    /// Not valid in the source: `EILSEQ`, or skipped under `//IGNORE` (this
    /// many bytes).
    Invalid(usize),
    /// Cut off by the end of the input: `EINVAL`.
    Incomplete,
}

/// The first character of `input` (non-empty) and its length in bytes.
fn decode(from: Charset, input: &[u8]) -> Result<(u32, usize), DecodeError> {
    let Some(&b0) = input.first() else {
        return Err(DecodeError::Incomplete);
    };
    match from {
        Charset::Ascii if b0 < 0x80 => Ok((u32::from(b0), 1)),
        Charset::Ascii => Err(DecodeError::Invalid(1)),
        Charset::Latin1 => Ok((u32::from(b0), 1)),
        Charset::Utf8 => decode_utf8(input),
    }
}

/// glibc's UTF-8: up to six bytes, U+7FFFFFFF at most; a stray continuation
/// byte, `0xFE`/`0xFF`, an overlong form or a surrogate is invalid, and the
/// error is reported at the lead byte (one byte is skipped under `//IGNORE`).
fn decode_utf8(input: &[u8]) -> Result<(u32, usize), DecodeError> {
    let b0 = *input.first().ok_or(DecodeError::Incomplete)?;
    let (len, init, min) = match b0 {
        0x00..=0x7F => return Ok((u32::from(b0), 1)),
        0xC2..=0xDF => (2, u32::from(b0 & 0x1F), 0x80),
        0xE0..=0xEF => (3, u32::from(b0 & 0x0F), 0x800),
        0xF0..=0xF7 => (4, u32::from(b0 & 0x07), 0x1_0000),
        0xF8..=0xFB => (5, u32::from(b0 & 0x03), 0x20_0000),
        0xFC..=0xFD => (6, u32::from(b0 & 0x01), 0x400_0000),
        // 0x80..=0xC1 (a continuation byte, or an overlong two-byte lead)
        // and 0xFE, 0xFF.
        _ => return Err(DecodeError::Invalid(1)),
    };
    let mut cp = init;
    for i in 1..len {
        let Some(&b) = input.get(i) else {
            // Every byte so far was a valid continuation: the character is
            // cut off, not wrong.
            return Err(DecodeError::Incomplete);
        };
        if b & 0xC0 != 0x80 {
            return Err(DecodeError::Invalid(1));
        }
        cp = (cp << 6) | u32::from(b & 0x3F);
    }
    if cp < min || (0xD800..=0xDFFF).contains(&cp) {
        return Err(DecodeError::Invalid(1));
    }
    Ok((cp, len))
}

/// `cp` in `to`, into `out`; the number of bytes, or `None` if `to` cannot
/// represent it.
fn encode(to: Charset, cp: u32, out: &mut [u8; 6]) -> Option<usize> {
    let limit = match to {
        Charset::Utf8 => return Some(encode_utf8(cp, out)),
        Charset::Ascii => 0x80,
        Charset::Latin1 => 0x100,
    };
    if cp >= limit {
        return None;
    }
    *out.first_mut()? = cp as u8;
    Some(1)
}

/// glibc's UTF-8 encoder, six bytes at most (`cp` is below 2^31: it came
/// from [`decode`]).  Each byte is masked to its bits before the cast.
fn encode_utf8(cp: u32, out: &mut [u8; 6]) -> usize {
    let (len, lead): (usize, u8) = match cp {
        0..=0x7F => (1, 0),
        0x80..=0x7FF => (2, 0xC0),
        0x800..=0xFFFF => (3, 0xE0),
        0x1_0000..=0x1F_FFFF => (4, 0xF0),
        0x20_0000..=0x3FF_FFFF => (5, 0xF8),
        _ => (6, 0xFC),
    };
    let mut v = cp;
    for slot in out.iter_mut().take(len).skip(1).rev() {
        *slot = 0x80 | (v & 0x3F) as u8;
        v >>= 6;
    }
    if let Some(first) = out.first_mut() {
        *first = lead | v as u8;
    }
    len
}

/// What `//TRANSLIT` writes for `cp`: glibc's C-locale table, or `?`.
fn transliterate(cp: u32) -> &'static [u8] {
    let table = crate::iconv_translit::C_TRANSLIT;
    match table.binary_search_by_key(&cp, |&(k, _)| k) {
        Ok(i) => table.get(i).map_or(b"?", |&(_, r)| r.as_bytes()),
        Err(_) => b"?",
    }
}

/// How a conversion ended.
#[derive(Debug, PartialEq, Eq)]
enum Stop {
    /// All the input went through: the irreversible count, or `EILSEQ` if
    /// `//IGNORE` skipped anything.
    Done { irreversible: usize, skipped: bool },
    /// Stopped at a character: this `errno`.
    Error(i32),
}

/// Convert `input` into `output` with `conv`: how far each got, and why it
/// stopped.  The heart of [`iconv`], on slices.
fn convert(conv: Conversion, input: &[u8], output: &mut [u8]) -> (usize, usize, Stop) {
    let (mut i, mut o) = (0usize, 0usize);
    let mut irreversible = 0usize;
    let mut skipped = false;
    let mut buf = [0u8; 6];
    while let Some(rest) = input.get(i..).filter(|r| !r.is_empty()) {
        // glibc asks for room for the smallest character before it looks at
        // the next one, so a full buffer stops even ahead of one `//IGNORE`
        // would have skipped.
        if o >= output.len() {
            return (i, o, Stop::Error(errno::E2BIG));
        }
        let (cp, len) = match decode(conv.from, rest) {
            Ok(c) => c,
            Err(DecodeError::Invalid(n)) if conv.flags & IGNORE != 0 => {
                i += n;
                skipped = true;
                continue;
            }
            Err(DecodeError::Invalid(_)) => return (i, o, Stop::Error(errno::EILSEQ)),
            Err(DecodeError::Incomplete) => return (i, o, Stop::Error(errno::EINVAL)),
        };
        let (written, substituted): (&[u8], bool) = match encode(conv.to, cp, &mut buf) {
            Some(n) => (buf.get(..n).unwrap_or(&[]), false),
            None if conv.flags & TRANSLIT != 0 => (transliterate(cp), true),
            None if conv.flags & IGNORE != 0 => {
                i += len;
                skipped = true;
                continue;
            }
            None => return (i, o, Stop::Error(errno::EILSEQ)),
        };
        // All of it or none: a substitute that does not fit is not begun.
        let Some(dst) = output.get_mut(o..o + written.len()) else {
            return (i, o, Stop::Error(errno::E2BIG));
        };
        dst.copy_from_slice(written);
        if substituted {
            irreversible += 1;
        }
        i += len;
        o += written.len();
    }
    (
        i,
        o,
        Stop::Done {
            irreversible,
            skipped,
        },
    )
}

/// Perform character set conversion.
///
/// Converts from `*inbuf` into `*outbuf`, advancing both and counting down
/// `*inbytesleft` and `*outbytesleft` by what went through; see the module
/// docs for where it stops and why.  Returns the number of irreversible
/// conversions (`//TRANSLIT` substitutions), or `(size_t)-1` with `errno`.
///
/// With `inbuf` or `*inbuf` NULL it resets the conversion state -- these
/// character sets have none, so it writes nothing and returns 0 -- after
/// checking the descriptor (`EBADF`), as glibc does.
///
/// Where glibc reads through a NULL pointer -- `inbytesleft`, `outbuf` or
/// `outbytesleft` on a conversion, `*outbuf` with room to write, or
/// `outbytesleft` beside a non-NULL `*outbuf` on a reset -- this answers
/// `EFAULT` instead.
///
/// # Safety
///
/// Each non-NULL pointer must be valid as C's `iconv` requires: `*inbuf` for
/// `*inbytesleft` bytes of reading and `*outbuf` for `*outbytesleft` bytes of
/// writing.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn iconv(
    cd: IconvT,
    inbuf: *mut *const u8,
    inbytesleft: *mut usize,
    outbuf: *mut *mut u8,
    outbytesleft: *mut usize,
) -> usize {
    let fail = |e: i32| {
        errno::set_errno(e);
        usize::MAX
    };
    // SAFETY: each is NULL or, per the C contract, valid to read.
    let in_start = if inbuf.is_null() {
        core::ptr::null()
    } else {
        unsafe { *inbuf }
    };
    let out_start = if outbuf.is_null() {
        core::ptr::null_mut()
    } else {
        unsafe { *outbuf }
    };

    if in_start.is_null() {
        // The reset.  glibc computes the end of the output from
        // `*outbytesleft` when there is an output pointer, before it looks at
        // the descriptor.
        if !out_start.is_null() && outbytesleft.is_null() {
            return fail(errno::EFAULT);
        }
        return match Conversion::from_descriptor(cd) {
            Some(_) => 0,
            None => fail(errno::EBADF),
        };
    }

    if inbytesleft.is_null() || outbuf.is_null() || outbytesleft.is_null() {
        return fail(errno::EFAULT);
    }
    // SAFETY: both non-NULL (checked), per the C contract valid to read.
    let (in_left, out_left) = unsafe { (*inbytesleft, *outbytesleft) };
    let Some(conv) = Conversion::from_descriptor(cd) else {
        return fail(errno::EBADF);
    };
    if out_start.is_null() && in_left > 0 && out_left > 0 {
        return fail(errno::EFAULT);
    }

    // SAFETY: the caller's buffers, for the lengths its counts give (a NULL
    // `*outbuf` only with nothing to write into it, checked above).
    let input = unsafe { core::slice::from_raw_parts(in_start, in_left) };
    let output: &mut [u8] = if out_start.is_null() {
        &mut []
    } else {
        // SAFETY: as above.
        unsafe { core::slice::from_raw_parts_mut(out_start, out_left) }
    };

    let (read, written, stop) = convert(conv, input, output);
    // SAFETY: the four pointers are the caller's, checked non-NULL above;
    // `read` and `written` are within the buffers they index.
    unsafe {
        *inbuf = in_start.add(read);
        *inbytesleft = in_left - read;
        if !out_start.is_null() {
            *outbuf = out_start.add(written);
        }
        *outbytesleft = out_left - written;
    }
    match stop {
        Stop::Done { skipped: true, .. } => fail(errno::EILSEQ),
        Stop::Done { irreversible, .. } => irreversible,
        Stop::Error(e) => fail(e),
    }
}

/// Close a conversion descriptor: 0, or -1 with `EBADF` for one `iconv_open`
/// did not return -- `(iconv_t)-1` above all, which glibc refuses the same
/// way.  Nothing is held, so nothing is freed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iconv_close(cd: IconvT) -> i32 {
    if Conversion::from_descriptor(cd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::undocumented_unsafe_blocks,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn open(to: &str, from: &str) -> IconvT {
        let to = [to.as_bytes(), b"\0"].concat();
        let from = [from.as_bytes(), b"\0"].concat();
        iconv_open(to.as_ptr(), from.as_ptr())
    }

    /// What one `iconv` call answers: (return, errno, bytes left in the input,
    /// what was written).
    fn run(to: &str, from: &str, input: &[u8], cap: usize) -> (isize, i32, usize, Vec<u8>) {
        let cd = open(to, from);
        assert_ne!(cd, ICONV_OPEN_ERR, "{to} <- {from}");
        let mut out = vec![0u8; cap];
        let mut ip = input.as_ptr();
        let mut il = input.len();
        let mut op = out.as_mut_ptr();
        let mut ol = cap;
        errno::set_errno(0);
        let r = unsafe { iconv(cd, &raw mut ip, &raw mut il, &raw mut op, &raw mut ol) };
        let e = errno::get_errno();
        assert_eq!(iconv_close(cd), 0);
        out.truncate(cap - ol);
        #[allow(clippy::cast_possible_wrap)]
        (r as isize, e, il, out)
    }

    // -- what Ubuntu 24.04's glibc 2.39 answered, probed 2026-09-26 --

    #[test]
    fn unencodable_is_eilseq_not_a_question_mark() {
        // THE REGRESSION PIN: this wrote "a?z" and counted one irreversible
        // conversion; glibc stops at the e-acute.
        assert_eq!(
            run("ASCII", "UTF-8", "a\u{e9}z".as_bytes(), 16),
            (-1, errno::EILSEQ, 3, b"a".to_vec())
        );
        assert_eq!(
            run("ASCII", "ISO-8859-1", b"a\xe9z", 16),
            (-1, errno::EILSEQ, 2, b"a".to_vec())
        );
        assert_eq!(
            run("ISO-8859-1", "UTF-8", "a\u{20ac}z".as_bytes(), 16),
            (-1, errno::EILSEQ, 4, b"a".to_vec())
        );
    }

    #[test]
    fn translit_is_glibcs_c_locale_table() {
        let t = |s: &str| run("ASCII//TRANSLIT", "UTF-8", s.as_bytes(), 32);
        assert_eq!(
            t("a\u{e9}z"),
            (1, 0, 0, b"a?z".to_vec()),
            "not in the table: ?"
        );
        assert_eq!(t("\u{20ac}"), (1, 0, 0, b"EUR".to_vec()));
        assert_eq!(t("\u{a9}"), (1, 0, 0, b"(C)".to_vec()));
        assert_eq!(t("\u{ab}"), (1, 0, 0, b"<<".to_vec()));
        assert_eq!(t("\u{fb01}"), (1, 0, 0, b"fi".to_vec()));
        assert_eq!(t("\u{df}"), (1, 0, 0, b"ss".to_vec()));
        assert_eq!(t("\u{201c}x\u{201d}"), (2, 0, 0, b"\"x\"".to_vec()));
        assert_eq!(
            run("ISO-8859-1//TRANSLIT", "UTF-8", "\u{20ac}".as_bytes(), 32),
            (1, 0, 0, b"EUR".to_vec())
        );
        assert_eq!(
            run("ASCII//TRANSLIT", "ISO-8859-1", b"\xe9", 32),
            (1, 0, 0, b"?".to_vec())
        );
    }

    #[test]
    fn a_substitute_that_does_not_fit_is_e2big_and_not_written() {
        assert_eq!(
            run("ASCII//TRANSLIT", "UTF-8", "\u{20ac}".as_bytes(), 2),
            (-1, errno::E2BIG, 3, Vec::new())
        );
    }

    #[test]
    fn ignore_skips_and_then_fails_with_eilseq() {
        assert_eq!(
            run("ASCII//IGNORE", "UTF-8", "a\u{4e00}b".as_bytes(), 32),
            (-1, errno::EILSEQ, 0, b"ab".to_vec())
        );
        assert_eq!(
            run("ASCII//IGNORE", "UTF-8", b"a\xffb", 32),
            (-1, errno::EILSEQ, 0, b"ab".to_vec()),
            "invalid input is skipped too"
        );
        assert_eq!(
            run("UTF-8//IGNORE", "UTF-8", b"a\xffb", 32),
            (-1, errno::EILSEQ, 0, b"ab".to_vec())
        );
    }

    #[test]
    fn a_full_buffer_stops_before_a_character_ignore_would_skip() {
        assert_eq!(
            run("ASCII//IGNORE", "UTF-8", "a\u{4e00}b".as_bytes(), 1),
            (-1, errno::E2BIG, 4, b"a".to_vec())
        );
    }

    #[test]
    fn translit_comes_before_ignore() {
        for to in ["ASCII//TRANSLIT//IGNORE", "ASCII//TRANSLIT,IGNORE"] {
            assert_eq!(
                run(to, "UTF-8", "a\u{4e00}b".as_bytes(), 32),
                (1, 0, 0, b"a?b".to_vec()),
                "{to}"
            );
        }
    }

    #[test]
    fn options_are_the_targets_and_any_case() {
        assert_eq!(
            run("ascii//translit", "utf-8", "\u{e9}".as_bytes(), 32),
            (1, 0, 0, b"?".to_vec())
        );
        assert_eq!(
            run("ASCII//FOO", "UTF-8", b"ab", 32),
            (0, 0, 0, b"ab".to_vec())
        );
        assert_eq!(
            run("UTF-8", "UTF-8//IGNORE", b"a\xffb", 32),
            (-1, errno::EILSEQ, 2, b"a".to_vec()),
            "the source's //IGNORE is not applied"
        );
    }

    #[test]
    fn utf8_input_is_validated_even_to_utf8() {
        // The identity conversions copied bytes.
        let t = |b: &[u8]| run("UTF-8", "UTF-8", b, 32);
        assert_eq!(t(b"a\xffz"), (-1, errno::EILSEQ, 2, b"a".to_vec()));
        assert_eq!(t(b"a\xc3"), (-1, errno::EINVAL, 1, b"a".to_vec()));
        assert_eq!(
            t(b"\xed\xa0\x80"),
            (-1, errno::EILSEQ, 3, Vec::new()),
            "surrogate"
        );
        assert_eq!(
            t(b"\xc1\xbf"),
            (-1, errno::EILSEQ, 2, Vec::new()),
            "overlong"
        );
        assert_eq!(
            t(b"\xe0\x9f\xbf"),
            (-1, errno::EILSEQ, 3, Vec::new()),
            "overlong"
        );
        assert_eq!(
            t(b"\xf0\x8f\xbf\xbf"),
            (-1, errno::EILSEQ, 4, Vec::new()),
            "overlong"
        );
        assert_eq!(
            t(b"\x80"),
            (-1, errno::EILSEQ, 1, Vec::new()),
            "stray continuation"
        );
        assert_eq!(t(b"\xfe"), (-1, errno::EILSEQ, 1, Vec::new()));
        assert_eq!(
            t(b"\xc2\x41"),
            (-1, errno::EILSEQ, 2, Vec::new()),
            "bad continuation"
        );
        assert_eq!(
            t(b"\xe1\x80"),
            (-1, errno::EINVAL, 2, Vec::new()),
            "cut off"
        );
        assert_eq!(t(b"\xe1\x80\x41"), (-1, errno::EILSEQ, 3, Vec::new()));
    }

    #[test]
    fn utf8_is_glibcs_six_byte_form() {
        let t = |b: &[u8]| run("UTF-8", "UTF-8", b, 32);
        for s in [
            &b"\xef\xbf\xbf"[..],
            b"\xf4\x8f\xbf\xbf",
            b"\xf4\x90\x80\x80",
            b"\xf8\x88\x80\x80\x80",
            b"\xfc\x84\x80\x80\x80\x80",
            b"\xef\xbb\xbfa",
        ] {
            assert_eq!(t(s), (0, 0, 0, s.to_vec()), "{s:x?}");
        }
        assert_eq!(
            run("ISO-8859-1", "UTF-8", b"\xf4\x90\x80\x80", 32),
            (-1, errno::EILSEQ, 4, Vec::new()),
            "decoded, then unencodable"
        );
    }

    #[test]
    fn ascii_input_above_0x7f_is_invalid() {
        assert_eq!(
            run("UTF-8", "ASCII", b"a\xe9z", 16),
            (-1, errno::EILSEQ, 2, b"a".to_vec())
        );
        assert_eq!(
            run("UTF-8", "ASCII", b"\x7f", 16),
            (0, 0, 0, b"\x7f".to_vec())
        );
        assert_eq!(run("ASCII", "UTF-8", b"\0", 16), (0, 0, 0, b"\0".to_vec()));
    }

    #[test]
    fn e2big_stops_before_the_character_that_does_not_fit() {
        assert_eq!(
            run("UTF-8", "ISO-8859-1", b"a\xe9z", 2),
            (-1, errno::E2BIG, 2, b"a".to_vec())
        );
        assert_eq!(
            run("UTF-8", "UTF-8", b"abc", 2),
            (-1, errno::E2BIG, 1, b"ab".to_vec())
        );
    }

    #[test]
    fn latin1_round_trips_through_utf8() {
        let all: Vec<u8> = (0..=255).collect();
        let (r, e, left, utf8) = run("UTF-8", "ISO-8859-1", &all, 512);
        assert_eq!((r, e, left), (0, 0, 0));
        assert_eq!(
            utf8,
            all.iter()
                .map(|&b| char::from(b))
                .collect::<std::string::String>()
                .into_bytes()
        );
        let (r, e, left, back) = run("ISO-8859-1", "UTF-8", &utf8, 512);
        assert_eq!((r, e, left, back), (0, 0, 0, all));
    }

    #[test]
    fn the_empty_name_is_the_c_locales_ascii() {
        assert_eq!(
            run("", "UTF-8", "a\u{e9}".as_bytes(), 32),
            (-1, errno::EILSEQ, 2, b"a".to_vec())
        );
    }

    #[test]
    fn names_are_glibcs_aliases() {
        for n in [
            "UTF-8",
            "utf8",
            "UTF8",
            "ANSI_X3.4-1968",
            "US-ASCII",
            "us",
            "CP367",
            "ISO-8859-1",
            "ISO_8859-1:1987",
            "latin1",
            "L1",
            "CP819",
            "IBM819",
        ] {
            assert_ne!(open(n, "UTF-8"), ICONV_OPEN_ERR, "{n}");
        }
        for n in ["UTF-16", "EBCDIC", "NOSUCH", "UTF-8x"] {
            errno::set_errno(0);
            assert_eq!(open(n, "UTF-8"), ICONV_OPEN_ERR, "{n}");
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }
    }

    // -- the NULL pointers, the descriptor, the reset --

    #[test]
    fn a_null_name_is_efault() {
        for (to, from) in [
            (core::ptr::null(), b"UTF-8\0".as_ptr()),
            (b"UTF-8\0".as_ptr(), core::ptr::null()),
        ] {
            errno::set_errno(0);
            assert_eq!(iconv_open(to, from), ICONV_OPEN_ERR);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }
    }

    #[test]
    fn the_reset_is_zero_and_checks_the_descriptor() {
        let cd = open("UTF-8", "UTF-8");
        let mut nul: *const u8 = core::ptr::null();
        let mut five = 5usize;
        errno::set_errno(0);
        let r = unsafe {
            iconv(
                cd,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!((r, errno::get_errno()), (0, 0));
        let r = unsafe {
            iconv(
                cd,
                &raw mut nul,
                &raw mut five,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!((r, five), (0, 5), "the counts are left alone");
        let mut out = [0u8; 4];
        let mut op = out.as_mut_ptr();
        let mut ol = out.len();
        let r = unsafe {
            iconv(
                cd,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut op,
                &raw mut ol,
            )
        };
        assert_eq!((r, ol), (0, 4), "nothing to write: no state");
        errno::set_errno(0);
        let r = unsafe {
            iconv(
                -1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(
            (r, errno::get_errno()),
            (usize::MAX, errno::EBADF),
            "glibc: EBADF"
        );
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn a_descriptor_iconv_open_did_not_return_is_ebadf() {
        let input = b"test";
        for cd in [
            -1,
            0,
            1,
            6,
            99,
            DESCRIPTOR_TAG | 0xF,
            DESCRIPTOR_TAG | 0xF00,
        ] {
            let mut ip = input.as_ptr();
            let mut il = input.len();
            let mut out = [0u8; 8];
            let mut op = out.as_mut_ptr();
            let mut ol = out.len();
            errno::set_errno(0);
            let r = unsafe { iconv(cd, &raw mut ip, &raw mut il, &raw mut op, &raw mut ol) };
            assert_eq!(
                (r, errno::get_errno()),
                (usize::MAX, errno::EBADF),
                "{cd:#x}"
            );
            errno::set_errno(0);
            assert_eq!(
                (iconv_close(cd), errno::get_errno()),
                (-1, errno::EBADF),
                "{cd:#x}"
            );
        }
    }

    #[test]
    fn a_null_count_or_output_pointer_is_efault() {
        let cd = open("UTF-8", "UTF-8");
        let input = b"x";
        let mut out = [0u8; 8];
        let mut ip = input.as_ptr();
        let mut il = 1usize;
        let mut op = out.as_mut_ptr();
        let mut ol = out.len();
        for (a, b, c) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            errno::set_errno(0);
            let r = unsafe {
                iconv(
                    cd,
                    &raw mut ip,
                    if a {
                        core::ptr::null_mut()
                    } else {
                        &raw mut il
                    },
                    if b {
                        core::ptr::null_mut()
                    } else {
                        &raw mut op
                    },
                    if c {
                        core::ptr::null_mut()
                    } else {
                        &raw mut ol
                    },
                )
            };
            assert_eq!((r, errno::get_errno()), (usize::MAX, errno::EFAULT));
        }
        let mut null_out: *mut u8 = core::ptr::null_mut();
        let r = unsafe { iconv(cd, &raw mut ip, &raw mut il, &raw mut null_out, &raw mut ol) };
        assert_eq!(
            (r, errno::get_errno()),
            (usize::MAX, errno::EFAULT),
            "*outbuf NULL with room"
        );
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn a_descriptor_is_never_the_error_value() {
        for from in ["UTF-8", "ASCII", "LATIN1"] {
            for to in ["UTF-8", "ASCII//TRANSLIT//IGNORE", "LATIN1//IGNORE"] {
                let cd = open(to, from);
                assert!(cd > 0, "{to} <- {from}: {cd:#x}");
                assert_eq!(iconv_close(cd), 0);
            }
        }
    }

    #[test]
    fn the_table_is_sorted_and_ascii() {
        let t = crate::iconv_translit::C_TRANSLIT;
        assert_eq!(t.len(), 1659);
        assert!(t.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(t.iter().all(|(_, r)| r.is_ascii()));
    }

    #[test]
    fn a_zero_width_character_transliterates_to_nothing() {
        assert_eq!(
            run("ASCII//TRANSLIT", "UTF-8", "a\u{200b}b".as_bytes(), 32),
            (1, 0, 0, b"ab".to_vec())
        );
    }
}
