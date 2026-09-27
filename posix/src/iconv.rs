// The arithmetic here is on byte counts bounded by the caller's buffers and
// by the 32 KiB conversion buffer, and on code points below 2^32; each
// subtraction is guarded by the comparison before it.
#![allow(clippy::arithmetic_side_effects)]

//! POSIX character set conversion (`<iconv.h>`): `iconv_open`, `iconv`,
//! `iconv_close`, with glibc 2.39's semantics for the character sets this
//! libc converts.
//!
//! # The character sets
//!
//! | name (glibc's; every alias of it is in [`NAMES`]) | what it is |
//! |---|---|
//! | `UTF-8` | glibc's older, wider UTF-8: up to six bytes and U+7FFFFFFF; overlong forms and surrogates refused |
//! | `ASCII` (`ANSI_X3.4-1968`) | seven bits; the C locale's character set, and so the empty name's |
//! | `ISO-8859-1` | Latin-1 |
//! | `CP1252` (`WINDOWS-1252`) | Latin-1 with printable characters in 0x80-0x9F -- the euro sign is 0x80 -- and 0x81, 0x8D, 0x8F, 0x90, 0x9D unassigned |
//! | `UTF-16`, `UTF-32` | a byte-order mark written first, in the machine's order (FF FE on x86-64), and read if there is one; the machine's order without one |
//! | `UTF-16LE`, `-16BE`, `-32LE`, `-32BE` | no mark written or read |
//! | `UCS-2` | the machine's order, no mark; `UCS-2BE` the other order |
//! | `UNICODE` | UCS-2 behind `UTF-16`'s mark |
//! | `UCS-4` | big-endian, no mark, up to U+7FFFFFFF; `UCS-4LE` |
//! | `WCHAR_T` | what a `wchar_t` holds: glibc's own form, UCS-4 in the machine's order, unchecked |
//!
//! A name is read as glibc reads one: options peeled off the end -- the last
//! `/`- or `,`-separated word, while there are two slashes -- then every
//! character but letters, digits and `_-.,:` dropped, the rest upper-cased,
//! and the result looked up exactly.  So `" utf-8 "` is UTF-8 and `UTF_8` is
//! no character set, as in glibc; `//TRANSLIT` and `//IGNORE` count on the
//! target's name only.
//!
//! # How a conversion runs, and where it stops
//!
//! As glibc's does: in steps.  Every character set but `WCHAR_T` is one step
//! to or from glibc's INTERNAL form (UCS-4 in the machine's order -- which is
//! `WCHAR_T`), so a conversion is two steps -- the source decoded into a buffer
//! of 8160 characters, that buffer encoded for the target, round after round
//! -- or one, when either side is `WCHAR_T`.  Where a conversion stops, and
//! with which error, is each step's loop as glibc writes it, and the rounds as
//! glibc's `iconv/skeleton.c` chains them:
//!
//! | the character | `errno` | unless the target named |
//! |---|---|---|
//! | is not valid in the source | `EILSEQ` | `//IGNORE`: it is skipped |
//! | is cut off by the end of the input | `EINVAL` | -- |
//! | cannot be written in the target | `EILSEQ` | `//TRANSLIT`: a substitute; `//IGNORE`: skipped |
//! | does not fit in the output | `E2BIG` | -- |
//!
//! What the steps make of that, pinned by the tests below:
//!
//! - In two steps the source is decoded ahead of the output: invalid or
//!   cut-off input right after the last character that fitted is `EILSEQ` or
//!   `EINVAL`, not `E2BIG`.  To or from `WCHAR_T`, one step asks for room
//!   first, and says `E2BIG`.
//! - On `E2BIG`, and on an unwritable character, the input is left just after
//!   the last character the target took -- ahead of any invalid bytes
//!   `//IGNORE` skipped on the way to the next one.
//! - `//IGNORE` converts the rest, then fails with `EILSEQ` -- for what the
//!   source skipped only if it was in the last 8160 characters, as glibc's
//!   buffer loses the earlier ones; for what the target skipped, straight
//!   after the round it was in.  glibc's `UCS-4`, `UCS-4LE` and `UCS-2BE`
//!   decoders, and its `UCS-2BE` encoder for a surrogate, skip without failing.
//! - A Unicode tag character, U+E0000-U+E007F, is dropped without a word by
//!   every target that cannot write it (ASCII, Latin-1, CP1252, UCS-2).
//! - `//TRANSLIT` writes glibc's C-locale substitute -- `"EUR"` for the euro
//!   sign ([`crate::iconv_translit`]) -- or `?`, in the target's own encoding;
//!   each counts as one irreversible conversion, which is what a successful
//!   `iconv` returns.
//! - A mark is read from the first two (four) bytes of the first call, and
//!   written when the first character reaches the target -- or at once, from
//!   `WCHAR_T`, input or none.  The reset, `iconv(cd, NULL, ...)`, writes
//!   nothing and makes the next call read (write) a mark again.
//!
//! Probed against Ubuntu 24.04's glibc 2.39 on 2026-09-26, and read from its
//! source.
//!
//! # Where this is not glibc
//!
//! Three glibc bugs are not reproduced:
//!
//! - glibc's reset forgets that a mark was read but not the byte order it
//!   gave, so a second stream after a big-endian first is read big-endian,
//!   mark or none.  The reset here forgets both.
//! - glibc's `//TRANSLIT` into `UTF-16`, `UTF-32` or `UNICODE` writes a second
//!   mark before each substitute in the first round (U+1F600 into
//!   `UNICODE//TRANSLIT` is FF FE FF FE 3F 00).  Here, one mark.
//! - When the first call reads a mark and then stops for want of room, glibc
//!   reads the next two (four) bytes as a mark again, and a leading U+FEFF is
//!   lost.  Here the mark is read once.
//!
//! And `WCHAR_T` to `WCHAR_T` is refused, as glibc has no step for it.
//!
//! # Until 2026-09-26
//!
//! Only UTF-8, ASCII and Latin-1 converted, and every other name was
//! refused.  Their names were matched with `-` and `_` removed, so `UTF_8`
//! opened and `8859_1` or `" UTF-8"` did not.  A full buffer said `E2BIG`
//! where glibc says `EILSEQ` or `EINVAL`.  A tag character was `EILSEQ`.  A
//! NULL `*outbuf` was accepted with no room to write
//! (`B-D-ICONV-HAD-THREE-CHARSETS`).  Before that, conversion to ASCII
//! replaced what it could not write with `?`, and the identity conversions
//! copied bytes unchecked (`B-D-ICONV-WAS-NOT-GLIBCS`).
//!
//! # Descriptors
//!
//! A conversion has state -- a mark read or still to write -- and a 32 KiB
//! buffer between its two steps, so an `iconv_t` is a handle to a descriptor
//! `iconv_open` allocates: a slot in this process's table, its generation,
//! and a tag, packed so that no handle is ever `(iconv_t)-1`.  Anything that
//! is not a live handle -- `(iconv_t)-1`, a closed one, one never returned --
//! is `EBADF`.  Using one descriptor from two threads at once is the caller's
//! race, as in glibc; different descriptors are independent.

use crate::errno;
use crate::perprocess::{PoolGuard, PoolLock, lock_pool, process_global};

/// Opaque conversion descriptor (see the module docs).
pub type IconvT = isize;

/// Error return from `iconv_open`.
pub const ICONV_OPEN_ERR: IconvT = -1;

// ---------------------------------------------------------------------------
// Character sets
// ---------------------------------------------------------------------------

/// Whether this machine is little-endian.  glibc writes a byte-order mark,
/// `UCS-2` and `WCHAR_T` in the machine's order, and reads unmarked `UTF-16`
/// and `UTF-32` in it.
const LITTLE_ENDIAN: bool = cfg!(target_endian = "little");

/// A byte-order mark, U+FEFF.
const MARK: u32 = 0xFEFF;

/// Which member of the UTF-16 or UTF-32 family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// The unsuffixed name: a byte-order mark written and read.
    Marked,
    /// `LE`: little-endian, no mark.
    Little,
    /// `BE`: big-endian, no mark.
    Big,
}

impl Form {
    /// Whether this form's order is not the machine's -- until a mark says
    /// otherwise, for `Marked`.
    fn swapped(self) -> bool {
        match self {
            Form::Marked => false,
            Form::Little => !LITTLE_ENDIAN,
            Form::Big => LITTLE_ENDIAN,
        }
    }
}

/// The character sets this libc converts: one per glibc conversion module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Charset {
    /// `ISO-10646/UTF8/`.
    Utf8,
    /// `ANSI_X3.4-1968//`.
    Ascii,
    /// `ISO-8859-1//`.
    Latin1,
    /// `CP1252//`.
    Cp1252,
    /// `ISO-10646/UCS4/`: big-endian.
    Ucs4,
    /// `UCS-4LE//`.
    Ucs4Le,
    /// `ISO-10646/UCS2/`, in the machine's order; `reversed`, the other --
    /// `UNICODEBIG//` on a little-endian machine.  glibc's two modules differ
    /// in more than the order: see [`decode_ucs2`] and [`encode_char`].
    Ucs2 { reversed: bool },
    /// `UNICODE//`: UCS-2 behind a byte-order mark.
    Unicode,
    /// `UTF-16//` and its `LE` and `BE`.
    Utf16(Form),
    /// `UTF-32//` and its `LE` and `BE`.
    Utf32(Form),
    /// `WCHAR_T//`: glibc's INTERNAL, the form every other set is converted
    /// through, so a conversion to or from it has one step, not two.
    Internal,
}

impl Charset {
    /// The byte-order mark's width, for a set that reads and writes one.
    fn mark_width(self) -> Option<usize> {
        match self {
            Charset::Utf16(Form::Marked) | Charset::Unicode => Some(2),
            Charset::Utf32(Form::Marked) => Some(4),
            _ => None,
        }
    }

    /// The width of one ASCII character in this set -- the smallest output a
    /// step writing it asks room for, and a substitute's unit.
    fn unit(self) -> usize {
        match self {
            Charset::Utf8 | Charset::Ascii | Charset::Latin1 | Charset::Cp1252 => 1,
            Charset::Ucs2 { .. } | Charset::Unicode | Charset::Utf16(_) => 2,
            Charset::Ucs4 | Charset::Ucs4Le | Charset::Utf32(_) | Charset::Internal => 4,
        }
    }

    /// Whether writing this set swaps the machine's byte order.
    fn target_swap(self) -> bool {
        match self {
            Charset::Utf16(form) | Charset::Utf32(form) => form.swapped(),
            Charset::Ucs2 { reversed } => reversed,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// Every name glibc 2.39 gives the sets above -- its gconv-modules aliases
/// and its built-in ones -- as [`parse_spec`] leaves a name: upper case, with
/// two slashes.  Checked name by name against Ubuntu 24.04's glibc on
/// 2026-09-26, as were the near misses glibc refuses (`UTF_8`, `LATIN-1`,
/// `UCS4LE`, `INTERNAL`).
const NAMES: &[(&[u8], Charset)] = &[
    (b"ISO-10646/UTF8/", Charset::Utf8),
    (b"ISO-10646/UTF-8/", Charset::Utf8),
    (b"UTF8//", Charset::Utf8),
    (b"UTF-8//", Charset::Utf8),
    (b"ISO-IR-193//", Charset::Utf8),
    (b"OSF05010001//", Charset::Utf8),
    (b"ANSI_X3.4-1968//", Charset::Ascii),
    (b"ANSI_X3.4//", Charset::Ascii),
    (b"ANSI_X3.4-1986//", Charset::Ascii),
    (b"ISO-IR-6//", Charset::Ascii),
    (b"ISO_646.IRV:1991//", Charset::Ascii),
    (b"ASCII//", Charset::Ascii),
    (b"ISO646-US//", Charset::Ascii),
    (b"US-ASCII//", Charset::Ascii),
    (b"US//", Charset::Ascii),
    (b"IBM367//", Charset::Ascii),
    (b"CP367//", Charset::Ascii),
    (b"CSASCII//", Charset::Ascii),
    (b"OSF00010020//", Charset::Ascii),
    (b"ISO-8859-1//", Charset::Latin1),
    (b"ISO-IR-100//", Charset::Latin1),
    (b"ISO_8859-1:1987//", Charset::Latin1),
    (b"ISO_8859-1//", Charset::Latin1),
    (b"ISO8859-1//", Charset::Latin1),
    (b"ISO88591//", Charset::Latin1),
    (b"LATIN1//", Charset::Latin1),
    (b"L1//", Charset::Latin1),
    (b"IBM819//", Charset::Latin1),
    (b"CP819//", Charset::Latin1),
    (b"CSISOLATIN1//", Charset::Latin1),
    (b"8859_1//", Charset::Latin1),
    (b"OSF00010001//", Charset::Latin1),
    (b"CP1252//", Charset::Cp1252),
    (b"MS-ANSI//", Charset::Cp1252),
    (b"WINDOWS-1252//", Charset::Cp1252),
    (b"ISO-10646/UCS4/", Charset::Ucs4),
    (b"UCS4//", Charset::Ucs4),
    (b"UCS-4//", Charset::Ucs4),
    (b"UCS-4BE//", Charset::Ucs4),
    (b"CSUCS4//", Charset::Ucs4),
    (b"ISO-10646//", Charset::Ucs4),
    (b"10646-1:1993//", Charset::Ucs4),
    (b"10646-1:1993/UCS4/", Charset::Ucs4),
    (b"OSF00010104//", Charset::Ucs4),
    (b"OSF00010105//", Charset::Ucs4),
    (b"OSF00010106//", Charset::Ucs4),
    (b"UCS-4LE//", Charset::Ucs4Le),
    (b"ISO-10646/UCS2/", Charset::Ucs2 { reversed: false }),
    (b"UCS2//", Charset::Ucs2 { reversed: false }),
    (b"UCS-2//", Charset::Ucs2 { reversed: false }),
    (b"OSF00010100//", Charset::Ucs2 { reversed: false }),
    (b"OSF00010101//", Charset::Ucs2 { reversed: false }),
    (b"OSF00010102//", Charset::Ucs2 { reversed: false }),
    (
        b"UNICODELITTLE//",
        Charset::Ucs2 {
            reversed: !LITTLE_ENDIAN,
        },
    ),
    (
        b"UCS-2LE//",
        Charset::Ucs2 {
            reversed: !LITTLE_ENDIAN,
        },
    ),
    (
        b"UNICODEBIG//",
        Charset::Ucs2 {
            reversed: LITTLE_ENDIAN,
        },
    ),
    (
        b"UCS-2BE//",
        Charset::Ucs2 {
            reversed: LITTLE_ENDIAN,
        },
    ),
    (b"UNICODE//", Charset::Unicode),
    (b"CSUNICODE//", Charset::Unicode),
    (b"UTF-16//", Charset::Utf16(Form::Marked)),
    (b"UTF16//", Charset::Utf16(Form::Marked)),
    (b"UTF-16LE//", Charset::Utf16(Form::Little)),
    (b"UTF16LE//", Charset::Utf16(Form::Little)),
    (b"UTF-16BE//", Charset::Utf16(Form::Big)),
    (b"UTF16BE//", Charset::Utf16(Form::Big)),
    (b"UTF-32//", Charset::Utf32(Form::Marked)),
    (b"UTF32//", Charset::Utf32(Form::Marked)),
    (b"UTF-32LE//", Charset::Utf32(Form::Little)),
    (b"UTF32LE//", Charset::Utf32(Form::Little)),
    (b"UTF-32BE//", Charset::Utf32(Form::Big)),
    (b"UTF32BE//", Charset::Utf32(Form::Big)),
    (b"WCHAR_T//", Charset::Internal),
];

/// Room for a stripped name: longer than any in [`NAMES`], so one that does
/// not fit is none of them.
const NAME_BUF: usize = 40;

/// C's `isspace` in the C locale.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// A name as glibc 2.39's `__gconv_create_spec` reads it: the character set
/// -- `None` for a name glibc has no module by -- and whether `TRANSLIT` and
/// `IGNORE` were among its options.
///
/// First `gconv_parse_code`: while the name, trailing blanks, `,` and `/`
/// dropped, still has two slashes, the text from its last `/` or `,` is an
/// option, compared without regard to case and cut off.  Then `strip`:
/// letters and digits, upper-cased, and `_-.,:` kept, every other character
/// dropped, the first two slashes kept (a third ends the name), and slashes
/// added to make two.  The name that leaves, `//`, is the locale's character
/// set -- in the C locale, this libc's only one, ASCII.
fn parse_spec(spec: &[u8]) -> (Option<Charset>, bool, bool) {
    let (mut translit, mut ignore) = (false, false);
    let mut end = spec.len();
    loop {
        while end > 0
            && spec
                .get(end - 1)
                .is_some_and(|&b| is_c_space(b) || b == b',' || b == b'/')
        {
            end -= 1;
        }
        let code = spec.get(..end).unwrap_or_default();
        let two_slashes = code.iter().filter(|&&b| b == b'/').nth(1).is_some();
        if !two_slashes {
            break;
        }
        let Some(at) = code.iter().rposition(|&b| b == b'/' || b == b',') else {
            break;
        };
        let word = code.get(at + 1..).unwrap_or_default();
        if word.eq_ignore_ascii_case(b"TRANSLIT") {
            translit = true;
        }
        if word.eq_ignore_ascii_case(b"IGNORE") {
            ignore = true;
        }
        end = at;
    }

    let mut buf = [0u8; NAME_BUF];
    let mut n = 0;
    let mut fits = true;
    let mut push = |b: u8, n: &mut usize| match buf.get_mut(*n) {
        Some(slot) => {
            *slot = b;
            *n += 1;
        }
        None => fits = false,
    };
    let mut slashes = 0;
    for &b in spec.get(..end).unwrap_or_default() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b',' | b':') {
            push(b.to_ascii_uppercase(), &mut n);
        } else if b == b'/' {
            slashes += 1;
            if slashes == 3 {
                break;
            }
            push(b'/', &mut n);
        }
    }
    while slashes < 2 {
        push(b'/', &mut n);
        slashes += 1;
    }
    let name = buf.get(..n).unwrap_or_default();
    let charset = if !fits {
        None
    } else if name == b"//" {
        Some(Charset::Ascii)
    } else {
        NAMES.iter().find(|(k, _)| *k == name).map(|&(_, c)| c)
    };
    (charset, translit, ignore)
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

// ---------------------------------------------------------------------------
// CP1252
// ---------------------------------------------------------------------------

/// CP1252's 0x80-0x9F, from glibc 2.39's `localedata/charmaps/CP1252`; 0 is
/// unassigned.  Every other byte is its own code point.
const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, // 0x80
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0, // 0x88
    0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, // 0x90
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178, // 0x98
];

/// The code point of CP1252 byte `b`, `None` if unassigned.
fn cp1252_decode(b: u8) -> Option<u32> {
    if b < 0x80 || b >= 0xA0 {
        return Some(u32::from(b));
    }
    let u = *CP1252_HIGH.get(usize::from(b - 0x80))?;
    (u != 0).then_some(u32::from(u))
}

/// The CP1252 byte for `c`, `None` if it has none.
fn cp1252_encode(c: u32) -> Option<u8> {
    if c < 0x80 || (0xA0..=0xFF).contains(&c) {
        return Some(c as u8);
    }
    let at = CP1252_HIGH
        .iter()
        .position(|&u| u != 0 && u32::from(u) == c)?;
    Some(0x80 + at as u8)
}

// ---------------------------------------------------------------------------
// The steps' loops
// ---------------------------------------------------------------------------

/// How a step's loop ended: glibc's `__GCONV_*` statuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    /// Carry on: a round that filled its buffer and was emptied, or a loop a
    /// substitution ended.
    Ok,
    /// Every byte of the input went through.
    EmptyInput,
    /// No room for the next character: `E2BIG`.
    FullOutput,
    /// The input ends inside a character: `EINVAL`.
    IncompleteInput,
    /// A character that cannot go through: `EILSEQ`.
    IllegalInput,
}

/// What one loop, or one call, did: bytes read and written, how it ended,
/// and the irreversible conversions it made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pass {
    read: usize,
    wrote: usize,
    status: Status,
    irreversible: usize,
}

impl Pass {
    /// A step that stopped before its loop.
    const fn stopped(status: Status) -> Self {
        Pass {
            read: 0,
            wrote: 0,
            status,
            irreversible: 0,
        }
    }
}

/// The 16-bit unit at `at`, in the machine's order unless `swap`.
fn get16(b: &[u8], at: usize, swap: bool) -> Option<u16> {
    let bytes: [u8; 2] = b.get(at..at + 2)?.try_into().ok()?;
    let v = u16::from_ne_bytes(bytes);
    Some(if swap { v.swap_bytes() } else { v })
}

/// The 32-bit unit at `at`, in the machine's order unless `swap`.
fn get32(b: &[u8], at: usize, swap: bool) -> Option<u32> {
    let bytes: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    let v = u32::from_ne_bytes(bytes);
    Some(if swap { v.swap_bytes() } else { v })
}

/// Write `v` at the start of `out`, in the machine's order unless `swap`;
/// `false` if it does not fit.
fn put16(out: &mut [u8], v: u16, swap: bool) -> bool {
    let v = if swap { v.swap_bytes() } else { v };
    match out.get_mut(..2) {
        Some(dst) => {
            dst.copy_from_slice(&v.to_ne_bytes());
            true
        }
        None => false,
    }
}

/// As [`put16`], for 32 bits.
fn put32(out: &mut [u8], v: u32, swap: bool) -> bool {
    let v = if swap { v.swap_bytes() } else { v };
    match out.get_mut(..4) {
        Some(dst) => {
            dst.copy_from_slice(&v.to_ne_bytes());
            true
        }
        None => false,
    }
}

/// Whether `c` is a UTF-16 surrogate.
fn is_surrogate(c: u32) -> bool {
    (0xD800..0xE000).contains(&c)
}

/// One character read by a decoder.
#[derive(Debug, PartialEq, Eq)]
enum Decoded {
    /// The character and its length in bytes.
    Char(u32, usize),
    /// Not valid: glibc's `STANDARD_FROM_LOOP_ERR_HANDLER` -- `EILSEQ`, or
    /// under `//IGNORE` this many bytes skipped and the conversion failed at
    /// its end.
    Illegal(usize),
    /// Not valid, and under `//IGNORE` skipped without failing the conversion:
    /// glibc's reversed UCS-2 decoder.
    IllegalQuiet(usize),
    /// Cut off by the end of the input.
    Incomplete,
}

/// glibc's UTF-8 decoder (`utf8_internal_loop`), one character.
///
/// A byte that begins no sequence is invalid together with the continuation
/// bytes after it, five bytes at most; a sequence the input cuts off is
/// incomplete if every byte of it there is a continuation, and invalid up to
/// the first that is not otherwise; a bad continuation byte ends the invalid
/// part before it; an overlong form or a surrogate is invalid whole.
fn decode_utf8(s: &[u8]) -> Decoded {
    let Some(&b0) = s.first() else {
        return Decoded::Incomplete;
    };
    if b0 < 0x80 {
        return Decoded::Char(u32::from(b0), 1);
    }
    let continuation = |i: usize| s.get(i).is_some_and(|&b| b & 0xC0 == 0x80);
    let (cnt, init): (usize, u8) = if (0xC2..0xE0).contains(&b0) {
        (2, b0 & 0x1F)
    } else if b0 & 0xF0 == 0xE0 {
        (3, b0 & 0x0F)
    } else if b0 & 0xF8 == 0xF0 {
        (4, b0 & 0x07)
    } else if b0 & 0xFC == 0xF8 {
        (5, b0 & 0x03)
    } else if b0 & 0xFE == 0xFC {
        (6, b0 & 0x01)
    } else {
        let mut n = 1;
        while n < 5 && continuation(n) {
            n += 1;
        }
        return Decoded::Illegal(n);
    };
    if cnt > s.len() {
        let mut n = 1;
        while n < s.len() && continuation(n) {
            n += 1;
        }
        return if n == s.len() {
            Decoded::Incomplete
        } else {
            Decoded::Illegal(n)
        };
    }
    let mut ch = u32::from(init);
    let mut n = 1;
    while n < cnt {
        let Some(&b) = s.get(n) else { break };
        if b & 0xC0 != 0x80 {
            break;
        }
        ch = (ch << 6) | u32::from(b & 0x3F);
        n += 1;
    }
    if n < cnt || (cnt > 2 && ch >> (5 * cnt - 4) == 0) || is_surrogate(ch) {
        return Decoded::Illegal(n);
    }
    Decoded::Char(ch, cnt)
}

/// glibc's UCS-2 decoders, one character: a surrogate is invalid -- and the
/// reversed module (`ucs2reverse_internal_loop`) skips it under `//IGNORE`
/// without failing the conversion, where the machine-order one fails it.
fn decode_ucs2(s: &[u8], reversed: bool) -> Decoded {
    let Some(u) = get16(s, 0, reversed) else {
        return Decoded::Incomplete;
    };
    if is_surrogate(u32::from(u)) {
        return if reversed {
            Decoded::IllegalQuiet(2)
        } else {
            Decoded::Illegal(2)
        };
    }
    Decoded::Char(u32::from(u), 2)
}

/// glibc's UTF-16 decoder (`from_utf16_loop`), one character: a low
/// surrogate first is invalid; a high one the input ends after is
/// incomplete; one followed by anything but a low one is invalid alone.
fn decode_utf16(s: &[u8], swap: bool) -> Decoded {
    let Some(u1) = get16(s, 0, swap) else {
        return Decoded::Incomplete;
    };
    if !is_surrogate(u32::from(u1)) {
        return Decoded::Char(u32::from(u1), 2);
    }
    if u1 >= 0xDC00 {
        return Decoded::Illegal(2);
    }
    let Some(u2) = get16(s, 2, swap) else {
        return Decoded::Incomplete;
    };
    if !(0xDC00..0xE000).contains(&u2) {
        return Decoded::Illegal(2);
    }
    let c = ((u32::from(u1) - 0xD7C0) << 10) + (u32::from(u2) - 0xDC00);
    Decoded::Char(c, 4)
}

/// The first step: `input` in `from`, into glibc's INTERNAL form in `out`.
fn decode_loop(from: Charset, swap: bool, ignore: bool, input: &[u8], out: &mut [u8]) -> Pass {
    match from {
        Charset::Utf8 => generic_decode(input, out, ignore, 1, decode_utf8),
        Charset::Ascii => generic_decode(input, out, ignore, 1, |s| match s.first() {
            Some(&b) if b < 0x80 => Decoded::Char(u32::from(b), 1),
            Some(_) => Decoded::Illegal(1),
            None => Decoded::Incomplete,
        }),
        Charset::Latin1 => generic_decode(input, out, ignore, 1, |s| match s.first() {
            Some(&b) => Decoded::Char(u32::from(b), 1),
            None => Decoded::Incomplete,
        }),
        Charset::Cp1252 => generic_decode(input, out, ignore, 1, |s| match s.first() {
            Some(&b) => cp1252_decode(b).map_or(Decoded::Illegal(1), |c| Decoded::Char(c, 1)),
            None => Decoded::Incomplete,
        }),
        Charset::Ucs2 { reversed } => {
            generic_decode(input, out, ignore, 2, |s| decode_ucs2(s, reversed))
        }
        Charset::Unicode => generic_decode(input, out, ignore, 2, |s| {
            let Some(u) = get16(s, 0, swap) else {
                return Decoded::Incomplete;
            };
            if is_surrogate(u32::from(u)) {
                Decoded::Illegal(2)
            } else {
                Decoded::Char(u32::from(u), 2)
            }
        }),
        Charset::Utf16(_) => generic_decode(input, out, ignore, 2, |s| decode_utf16(s, swap)),
        Charset::Utf32(_) => generic_decode(input, out, ignore, 4, |s| {
            let Some(u) = get32(s, 0, swap) else {
                return Decoded::Incomplete;
            };
            if u >= 0x11_0000 || is_surrogate(u) {
                Decoded::Illegal(4)
            } else {
                Decoded::Char(u, 4)
            }
        }),
        Charset::Ucs4 => decode_ucs4(input, out, ignore, true),
        Charset::Ucs4Le => decode_ucs4(input, out, ignore, false),
        // [`Descriptor::convert`] runs no step for WCHAR_T; this is never
        // reached, and would fail rather than pass bytes through unread.
        Charset::Internal => Pass::stopped(Status::IllegalInput),
    }
}

/// glibc's generic loop (`iconv/loop.c`) around a decoder: before each
/// character, enough input for the smallest (`EINVAL` if not) and room for
/// one INTERNAL character (`E2BIG`).
fn generic_decode(
    input: &[u8],
    out: &mut [u8],
    ignore: bool,
    min_in: usize,
    decode: impl Fn(&[u8]) -> Decoded,
) -> Pass {
    let (mut i, mut o, mut irreversible) = (0, 0, 0);
    let mut status = Status::EmptyInput;
    while let Some(rest) = input.get(i..).filter(|r| !r.is_empty()) {
        if rest.len() < min_in {
            status = Status::IncompleteInput;
            break;
        }
        if out.len() - o < 4 {
            status = Status::FullOutput;
            break;
        }
        match decode(rest) {
            Decoded::Char(c, n) => {
                if !put32(out.get_mut(o..).unwrap_or_default(), c, false) {
                    status = Status::FullOutput;
                    break;
                }
                o += 4;
                i += n;
            }
            Decoded::Illegal(n) => {
                status = Status::IllegalInput;
                if !ignore {
                    break;
                }
                i += n;
                irreversible += 1;
            }
            Decoded::IllegalQuiet(n) => {
                if !ignore {
                    status = Status::IllegalInput;
                    break;
                }
                i += n;
                irreversible += 1;
            }
            Decoded::Incomplete => {
                status = Status::IncompleteInput;
                break;
            }
        }
    }
    Pass {
        read: i,
        wrote: o,
        status,
        irreversible,
    }
}

/// glibc's UCS-4 decoders (`ucs4_internal_loop`, `ucs4le_internal_loop`):
/// four bytes a character while there are four and room for them, a value
/// above U+7FFFFFFF invalid -- skipped under `//IGNORE` without failing the
/// conversion.  Where both run out, the big-endian one reports the output
/// first (`E2BIG`), the little-endian one the input (`EINVAL`).
fn decode_ucs4(input: &[u8], out: &mut [u8], ignore: bool, big: bool) -> Pass {
    let (mut i, mut o, mut irreversible) = (0, 0, 0);
    while let Some(src) = input.get(i..i + 4) {
        if out.len() - o < 4 {
            break;
        }
        let bytes: [u8; 4] = src.try_into().unwrap_or_default();
        let v = if big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        };
        if v > 0x7FFF_FFFF {
            if ignore {
                irreversible += 1;
                i += 4;
                continue;
            }
            return Pass {
                read: i,
                wrote: o,
                status: Status::IllegalInput,
                irreversible,
            };
        }
        if !put32(out.get_mut(o..).unwrap_or_default(), v, false) {
            break;
        }
        o += 4;
        i += 4;
    }
    let out_full = out.len() - o < 4;
    let in_short = input.len() - i < 4;
    let status = if i == input.len() {
        Status::EmptyInput
    } else if big {
        if out_full {
            Status::FullOutput
        } else {
            Status::IncompleteInput
        }
    } else if in_short {
        Status::IncompleteInput
    } else {
        Status::FullOutput
    };
    Pass {
        read: i,
        wrote: o,
        status,
        irreversible,
    }
}

/// One character written by an encoder.
#[derive(Debug, PartialEq, Eq)]
enum Encoded {
    /// This many bytes written.
    Wrote(usize),
    /// It does not fit in what is left of the output.
    NoRoom,
    /// The target cannot write it: glibc's `STANDARD_TO_LOOP_ERR_HANDLER`,
    /// after the tag-character check.
    Unwritable,
    /// A surrogate, which glibc's UCS-2, UTF-16 and UTF-32 encoders refuse
    /// themselves: `EILSEQ`, or skipped under `//IGNORE` -- failing the
    /// conversion at its end unless `quiet` (the reversed UCS-2 encoder).
    /// Never substituted.
    Surrogate { quiet: bool },
}

/// `c` in `to`, at the start of `out`.
fn encode_char(to: Charset, swap: bool, c: u32, out: &mut [u8]) -> Encoded {
    let byte = |b: u8, out: &mut [u8]| match out.first_mut() {
        Some(slot) => {
            *slot = b;
            Encoded::Wrote(1)
        }
        None => Encoded::NoRoom,
    };
    let unit16 = |u: u16, out: &mut [u8]| {
        if put16(out, u, swap) {
            Encoded::Wrote(2)
        } else {
            Encoded::NoRoom
        }
    };
    match to {
        Charset::Utf8 => encode_utf8(c, out),
        Charset::Ascii if c <= 0x7F => byte(c as u8, out),
        Charset::Latin1 if c <= 0xFF => byte(c as u8, out),
        Charset::Cp1252 if c < 0xFFFF => match cp1252_encode(c) {
            Some(b) => byte(b, out),
            None => Encoded::Unwritable,
        },
        Charset::Ascii | Charset::Latin1 | Charset::Cp1252 => Encoded::Unwritable,
        Charset::Ucs2 { reversed } => {
            if c >= 0x1_0000 {
                Encoded::Unwritable
            } else if is_surrogate(c) {
                Encoded::Surrogate { quiet: reversed }
            } else {
                unit16(c as u16, out)
            }
        }
        Charset::Unicode => {
            if c >= 0x1_0000 {
                Encoded::Unwritable
            } else if is_surrogate(c) {
                Encoded::Surrogate { quiet: false }
            } else {
                unit16(c as u16, out)
            }
        }
        Charset::Utf16(_) => {
            if is_surrogate(c) {
                Encoded::Surrogate { quiet: false }
            } else if c >= 0x11_0000 {
                Encoded::Unwritable
            } else if c >= 0x1_0000 {
                if out.len() < 4 {
                    return Encoded::NoRoom;
                }
                let high = (0xD7C0 + (c >> 10)) as u16;
                let low = (0xDC00 + (c & 0x3FF)) as u16;
                if put16(out, high, swap) && put16(out.get_mut(2..).unwrap_or_default(), low, swap)
                {
                    Encoded::Wrote(4)
                } else {
                    Encoded::NoRoom
                }
            } else {
                unit16(c as u16, out)
            }
        }
        Charset::Utf32(_) => {
            if c >= 0x11_0000 {
                Encoded::Unwritable
            } else if is_surrogate(c) {
                Encoded::Surrogate { quiet: false }
            } else if put32(out, c, swap) {
                Encoded::Wrote(4)
            } else {
                Encoded::NoRoom
            }
        }
        // UCS-4 has a loop of its own ([`encode_ucs4`]), and WCHAR_T no step.
        Charset::Ucs4 | Charset::Ucs4Le | Charset::Internal => Encoded::Unwritable,
    }
}

/// glibc's UTF-8 encoder (`internal_utf8_loop`), one character: up to six
/// bytes; a surrogate or a value above U+7FFFFFFF cannot be written.
fn encode_utf8(c: u32, out: &mut [u8]) -> Encoded {
    if c < 0x80 {
        return match out.first_mut() {
            Some(slot) => {
                *slot = c as u8;
                Encoded::Wrote(1)
            }
            None => Encoded::NoRoom,
        };
    }
    if c > 0x7FFF_FFFF || is_surrogate(c) {
        return Encoded::Unwritable;
    }
    let len = match c {
        0x80..=0x7FF => 2,
        0x800..=0xFFFF => 3,
        0x1_0000..=0x1F_FFFF => 4,
        0x20_0000..=0x3FF_FFFF => 5,
        _ => 6,
    };
    let Some(dst) = out.get_mut(..len) else {
        return Encoded::NoRoom;
    };
    let mut v = c;
    for slot in dst.iter_mut().skip(1).rev() {
        *slot = 0x80 | (v & 0x3F) as u8;
        v >>= 6;
    }
    if let Some(first) = dst.first_mut() {
        *first = (0xFF00_u32 >> len) as u8 | v as u8;
    }
    Encoded::Wrote(len)
}

/// What `//TRANSLIT` writes for `c` into `out`, in `to`: glibc's C-locale
/// substitute, or `?` for a character its table does not list; `None` if it
/// does not all fit, and nothing is written then.
///
/// Every substitute is ASCII, one unit of `to` a character, which glibc
/// writes by running the step's own loop over it -- so any target can write
/// it, in its own encoding and byte order.
fn transliterate(to: Charset, swap: bool, c: u32, out: &mut [u8]) -> Option<usize> {
    let table = crate::iconv_translit::C_TRANSLIT;
    let substitute: &[u8] = match table.binary_search_by_key(&c, |&(k, _)| k) {
        Ok(at) => table.get(at).map_or(b"?", |&(_, r)| r.as_bytes()),
        Err(_) => b"?",
    };
    if substitute.len() * to.unit() > out.len() {
        return None;
    }
    let mut o = 0;
    for &b in substitute {
        // ASCII is every target's, so this writes; a failure would only
        // leave the substitute unwritten, reported as no room.
        match encode_char(to, swap, u32::from(b), out.get_mut(o..)?) {
            Encoded::Wrote(n) => o += n,
            _ => return None,
        }
    }
    Some(o)
}

/// The last step: glibc's INTERNAL form in `input`, into `to` in `out`.
fn encode_loop(to: Charset, translit: bool, ignore: bool, input: &[u8], out: &mut [u8]) -> Pass {
    match to {
        Charset::Ucs4 => encode_ucs4(input, out, true),
        Charset::Ucs4Le => encode_ucs4(input, out, false),
        _ => generic_encode(to, translit, ignore, input, out),
    }
}

/// glibc's generic loop around an encoder: before each character, a whole
/// INTERNAL character of input (`EINVAL` if not -- from `WCHAR_T`) and room
/// for the smallest output (`E2BIG`); then the character, or the errors'
/// handling: a tag character dropped, a substitute (`//TRANSLIT`), a skip
/// (`//IGNORE`).
fn generic_encode(to: Charset, translit: bool, ignore: bool, input: &[u8], out: &mut [u8]) -> Pass {
    let swap = to.target_swap();
    let min_out = to.unit();
    let (mut i, mut o, mut irreversible) = (0, 0, 0);
    let mut status = Status::EmptyInput;
    while i < input.len() {
        let Some(c) = get32(input, i, false) else {
            status = Status::IncompleteInput;
            break;
        };
        if out.len() - o < min_out {
            status = Status::FullOutput;
            break;
        }
        let rest = out.get_mut(o..).unwrap_or_default();
        match encode_char(to, swap, c, rest) {
            Encoded::Wrote(n) => {
                o += n;
                i += 4;
            }
            Encoded::NoRoom => {
                status = Status::FullOutput;
                break;
            }
            Encoded::Surrogate { quiet } => {
                if !quiet {
                    status = Status::IllegalInput;
                }
                if !ignore {
                    status = Status::IllegalInput;
                    break;
                }
                i += 4;
                irreversible += 1;
            }
            Encoded::Unwritable => {
                // UNICODE_TAG_HANDLER: a tag character is dropped, without a
                // word, by a target that cannot write it.  (Every target that
                // can write one writes it, so the check needs no list.)
                if c >> 7 == 0xE_0000 >> 7 {
                    i += 4;
                    continue;
                }
                status = Status::IllegalInput;
                if translit {
                    match transliterate(to, swap, c, rest) {
                        Some(n) => {
                            // glibc's loop carries on with the result OK, not
                            // "input used up": which is what ends a round.
                            o += n;
                            i += 4;
                            irreversible += 1;
                            status = Status::Ok;
                            continue;
                        }
                        None => {
                            status = Status::FullOutput;
                            break;
                        }
                    }
                }
                if !ignore {
                    break;
                }
                irreversible += 1;
                i += 4;
            }
        }
    }
    Pass {
        read: i,
        wrote: o,
        status,
        irreversible,
    }
}

/// glibc's UCS-4 encoders (`internal_ucs4_loop`, `internal_ucs4le_loop`):
/// whole characters copied in the target's order, unchecked, as far as the
/// shorter of input and output goes; out of room is reported before a
/// partial character of input.
fn encode_ucs4(input: &[u8], out: &mut [u8], big: bool) -> Pass {
    let (units, _) = input.as_chunks::<4>();
    let (slots, _) = out.as_chunks_mut::<4>();
    let mut n = 0;
    for (src, dst) in units.iter().zip(slots.iter_mut()) {
        let v = u32::from_ne_bytes(*src);
        *dst = if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        };
        n += 1;
    }
    let done = n * 4;
    let status = if done == input.len() {
        Status::EmptyInput
    } else if out.len() - done < 4 {
        Status::FullOutput
    } else {
        Status::IncompleteInput
    };
    Pass {
        read: done,
        wrote: done,
        status,
        irreversible: 0,
    }
}

// ---------------------------------------------------------------------------
// Descriptors
// ---------------------------------------------------------------------------

/// glibc's `GCONV_NCHAR_GOAL`: the characters the first step's buffer holds.
const CHUNK_CHARS: usize = 8160;

/// That buffer in bytes: INTERNAL is four bytes a character.
const CHUNK_BYTES: usize = CHUNK_CHARS * 4;

/// One open conversion.
struct Descriptor {
    from: Charset,
    to: Charset,
    /// `//TRANSLIT` on the target's name.
    translit: bool,
    /// `//IGNORE` on the target's name.
    ignore: bool,
    /// A marked source's byte order -- whether to swap the machine's --
    /// once its first two (four) bytes were seen; `None` until then, and
    /// after a reset.
    from_swap: Option<bool>,
    /// A marked target's mark is not written yet.
    mark_pending: bool,
    /// The first step's output in a two-step conversion: [`CHUNK_BYTES`]
    /// bytes from `malloc`, owned by this descriptor; null in a one-step one.
    chunk: *mut u8,
}

impl Descriptor {
    /// glibc's `PREPARE_LOOP` for the first step: the bytes a mark took, and
    /// whether to swap.  A marked source's first call with two (four) bytes
    /// settles its order; with fewer, the call ends there: done if there were
    /// none, `EINVAL` if some.
    fn source_order(&mut self, input: &[u8]) -> Result<(usize, bool), Status> {
        let fixed = match self.from {
            Charset::Utf16(form) | Charset::Utf32(form) if form != Form::Marked => {
                return Ok((0, form.swapped()));
            }
            _ => (0, false),
        };
        let Some(width) = self.from.mark_width() else {
            return Ok(fixed);
        };
        if let Some(swap) = self.from_swap {
            return Ok((0, swap));
        }
        if input.len() < width {
            return Err(if input.is_empty() {
                Status::EmptyInput
            } else {
                Status::IncompleteInput
            });
        }
        let head = if width == 2 {
            get16(input, 0, false).map(u32::from)
        } else {
            get32(input, 0, false)
        };
        let reversed = if width == 2 { 0xFFFE } else { 0xFFFE_0000 };
        let (skip, swap) = match head {
            Some(MARK) => (width, false),
            Some(v) if v == reversed => (width, true),
            _ => (0, false),
        };
        self.from_swap = Some(swap);
        Ok((skip, swap))
    }

    /// The last step as glibc's `skeleton.c` runs it: a marked target's mark
    /// first, while one is due (`E2BIG`, and nothing else, if it does not
    /// fit); then one pass of the loop, after which no mark is due.
    fn encode_step(&mut self, input: &[u8], out: &mut [u8]) -> Pass {
        let mut mark = 0;
        if let (Some(width), true) = (self.to.mark_width(), self.mark_pending) {
            let written = if width == 2 {
                put16(out, MARK as u16, false)
            } else {
                put32(out, MARK, false)
            };
            if !written {
                return Pass::stopped(Status::FullOutput);
            }
            mark = width;
        }
        let rest = out.get_mut(mark..).unwrap_or_default();
        let pass = encode_loop(self.to, self.translit, self.ignore, input, rest);
        self.mark_pending = false;
        Pass {
            wrote: mark + pass.wrote,
            ..pass
        }
    }

    /// A one-step conversion into `WCHAR_T`: the first step, into the
    /// caller's buffer.
    fn decode_step(&mut self, input: &[u8], out: &mut [u8]) -> Pass {
        let (skip, swap) = match self.source_order(input) {
            Ok(order) => order,
            Err(status) => return Pass::stopped(status),
        };
        let rest = input.get(skip..).unwrap_or_default();
        let pass = decode_loop(self.from, swap, self.ignore, rest, out);
        Pass {
            read: skip + pass.read,
            ..pass
        }
    }

    /// A two-step conversion, round by round, as glibc's `skeleton.c` chains
    /// its steps.  Each round decodes into the buffer and encodes what that
    /// made; where the target stops short, the round is decoded again into
    /// just the part it took, so the input stops where the output did.  A
    /// round that filled the buffer and was emptied goes on; any other end is
    /// the call's -- the target's if it stopped, the source's if not.
    fn convert_two(&mut self, input: &[u8], out: &mut [u8]) -> Pass {
        let (skip, swap) = match self.source_order(input) {
            Ok(order) => order,
            Err(status) => return Pass::stopped(status),
        };
        if self.chunk.is_null() {
            // iconv_open allocates the buffer for every two-step conversion.
            return Pass::stopped(Status::IllegalInput);
        }
        // SAFETY: `chunk` is this descriptor's own CHUNK_BYTES-byte block
        // (non-null, checked), and nothing else refers to it during the call.
        let chunk = unsafe { core::slice::from_raw_parts_mut(self.chunk, CHUNK_BYTES) };
        let (mut i, mut o, mut irreversible) = (skip, 0, 0);
        loop {
            let round = i;
            let first = decode_loop(
                self.from,
                swap,
                self.ignore,
                input.get(i..).unwrap_or_default(),
                chunk,
            );
            i += first.read;
            let mut status = first.status;
            if first.wrote > 0 {
                let made = chunk.get(..first.wrote).unwrap_or_default();
                let last = self.encode_step(made, out.get_mut(o..).unwrap_or_default());
                o += last.wrote;
                irreversible += last.irreversible;
                if last.status != Status::EmptyInput {
                    if last.read != first.wrote {
                        let again = decode_loop(
                            self.from,
                            swap,
                            self.ignore,
                            input.get(round..).unwrap_or_default(),
                            chunk.get_mut(..last.read).unwrap_or_default(),
                        );
                        i = round + again.read;
                    }
                    status = last.status;
                } else if status == Status::FullOutput {
                    status = Status::Ok;
                }
            }
            if status != Status::Ok {
                return Pass {
                    read: i,
                    wrote: o,
                    status,
                    irreversible,
                };
            }
        }
    }

    /// One `iconv` call's conversion.
    fn convert(&mut self, input: &[u8], out: &mut [u8]) -> Pass {
        match (self.from, self.to) {
            (Charset::Internal, _) => self.encode_step(input, out),
            (_, Charset::Internal) => self.decode_step(input, out),
            _ => self.convert_two(input, out),
        }
    }

    /// The reset: the next call reads (writes) a mark again.
    fn reset(&mut self) {
        self.from_swap = None;
        self.mark_pending = true;
    }
}

/// A new descriptor from `malloc`, with its buffer if it has two steps;
/// `None` if memory ran out.
fn allocate(from: Charset, to: Charset, translit: bool, ignore: bool) -> Option<*mut Descriptor> {
    let chunk = if from == Charset::Internal || to == Charset::Internal {
        core::ptr::null_mut()
    } else {
        let p = crate::malloc::malloc(CHUNK_BYTES);
        if p.is_null() {
            return None;
        }
        p
    };
    let d = crate::malloc::malloc(size_of::<Descriptor>()).cast::<Descriptor>();
    if d.is_null() {
        // SAFETY: `chunk` is null or the block just allocated.
        unsafe { crate::malloc::free(chunk) };
        return None;
    }
    // SAFETY: `d` is a fresh block of the descriptor's size, and malloc's
    // alignment suits any type.
    unsafe {
        d.write(Descriptor {
            from,
            to,
            translit,
            ignore,
            from_swap: None,
            mark_pending: true,
            chunk,
        });
    }
    Some(d)
}

/// Free a descriptor [`allocate`] returned, and its buffer.
///
/// # Safety
///
/// `d` came from [`allocate`], is no longer in the table, and is not freed
/// twice.
unsafe fn deallocate(d: *mut Descriptor) {
    // SAFETY: the caller's contract: `d` is a live descriptor.
    unsafe {
        crate::malloc::free((*d).chunk);
        crate::malloc::free(d.cast::<u8>());
    }
}

// ---------------------------------------------------------------------------
// The table of open descriptors
// ---------------------------------------------------------------------------

/// One slot: its descriptor (null when free), and how many times it has
/// been freed -- so a closed handle does not name its slot's next tenant.
#[derive(Clone, Copy)]
struct Slot {
    desc: *mut Descriptor,
    generation: u32,
}

/// This process's slots, grown as needed.  Indices are stable; the array
/// moves.
struct Table {
    slots: *mut Slot,
    cap: usize,
}

process_global! {
    /// This process's open descriptors.
    fn table() -> Table = Table { slots: core::ptr::null_mut(), cap: 0 };

    /// Serialises every use of the table, with its scope
    /// ([`crate::perprocess::PoolLock`]).  A descriptor itself is used
    /// outside it, by the one call it was handed to.
    fn table_lock() -> PoolLock = PoolLock::new();
}

/// The bits of a handle above the slot and generation: positive, and never
/// all ones, so no handle is `(iconv_t)-1`.
const HANDLE_TAG: u64 = 0x1C1 << 52;
/// Where the tag is.
const TAG_MASK: u64 = 0xFFF << 52;
/// The generation's width: 20 bits, above the slot index's 32.
const GENERATION_MASK: u32 = 0xF_FFFF;

/// The handle for slot `index` in `generation`.
fn pack(index: usize, generation: u32) -> Option<IconvT> {
    let index = u64::try_from(index).ok().filter(|&i| i <= 0xFFFF_FFFF)?;
    let raw = HANDLE_TAG | (u64::from(generation & GENERATION_MASK) << 32) | index;
    IconvT::try_from(raw).ok()
}

/// A handle's slot and generation, if it is shaped like one.
fn unpack(cd: IconvT) -> Option<(usize, u32)> {
    let raw = u64::try_from(cd).ok()?;
    if raw & TAG_MASK != HANDLE_TAG {
        return None;
    }
    let generation = ((raw >> 32) as u32) & GENERATION_MASK;
    let index = usize::try_from(raw & 0xFFFF_FFFF).ok()?;
    Some((index, generation))
}

/// Holds the table's lock; the table is reached through it.
struct Locked {
    _guard: PoolGuard<'static>,
    t: *mut Table,
}

fn lock() -> Locked {
    Locked {
        // SAFETY: `table_lock()` is this context's lock, valid as long as the
        // table it guards.
        _guard: unsafe { lock_pool(table_lock()) },
        t: table(),
    }
}

impl Locked {
    fn slot(&mut self, i: usize) -> Option<&mut Slot> {
        // SAFETY: the lock is held; `slots` holds `cap` initialised entries.
        unsafe {
            let t = &mut *self.t;
            if i < t.cap {
                Some(&mut *t.slots.add(i))
            } else {
                None
            }
        }
    }

    fn cap(&self) -> usize {
        // SAFETY: the lock is held.
        unsafe { (*self.t).cap }
    }

    /// Put `desc` in a free slot, growing the table if there is none: its
    /// handle, or `None` if memory ran out.
    fn insert(&mut self, desc: *mut Descriptor) -> Option<IconvT> {
        let free = (0..self.cap()).find(|&i| self.slot(i).is_some_and(|s| s.desc.is_null()));
        let index = match free {
            Some(i) => i,
            None => {
                let old = self.cap();
                let cap = old.checked_mul(2)?.max(8);
                let bytes = cap.checked_mul(size_of::<Slot>())?;
                // SAFETY: the lock is held; `slots` is null or the table's
                // own malloc block, and realloc keeps its `old` entries.
                let grown = unsafe {
                    let t = &mut *self.t;
                    crate::malloc::realloc(t.slots.cast::<u8>(), bytes).cast::<Slot>()
                };
                if grown.is_null() {
                    return None;
                }
                for k in old..cap {
                    // SAFETY: `grown` holds `cap` entries.
                    unsafe {
                        grown.add(k).write(Slot {
                            desc: core::ptr::null_mut(),
                            generation: 0,
                        });
                    }
                }
                // SAFETY: the lock is held.
                unsafe {
                    let t = &mut *self.t;
                    t.slots = grown;
                    t.cap = cap;
                }
                old
            }
        };
        let slot = self.slot(index)?;
        let handle = pack(index, slot.generation)?;
        slot.desc = desc;
        Some(handle)
    }

    /// The live descriptor `cd` names.
    fn get(&mut self, cd: IconvT) -> Option<*mut Descriptor> {
        let (index, generation) = unpack(cd)?;
        let slot = self.slot(index)?;
        (!slot.desc.is_null() && slot.generation == generation).then_some(slot.desc)
    }

    /// Take the live descriptor `cd` names out of the table.
    fn remove(&mut self, cd: IconvT) -> Option<*mut Descriptor> {
        let desc = self.get(cd)?;
        let (index, _) = unpack(cd)?;
        let slot = self.slot(index)?;
        slot.desc = core::ptr::null_mut();
        slot.generation = (slot.generation + 1) & GENERATION_MASK;
        Some(desc)
    }
}

// ---------------------------------------------------------------------------
// The C interface
// ---------------------------------------------------------------------------

/// Open a conversion from `fromcode` to `tocode`.
///
/// `(iconv_t)-1` with `EINVAL` when either names no character set this libc
/// converts, or both `WCHAR_T`; with `ENOMEM` when memory runs out; with
/// `EFAULT` for a NULL name, where glibc faults reading it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iconv_open(tocode: *const u8, fromcode: *const u8) -> IconvT {
    if tocode.is_null() || fromcode.is_null() {
        errno::set_errno(errno::EFAULT);
        return ICONV_OPEN_ERR;
    }
    // SAFETY: both non-NULL (checked) and, per the C contract, NUL-terminated.
    let (to_spec, from_spec) = unsafe { (cstr_slice(tocode), cstr_slice(fromcode)) };
    let (to, translit, ignore) = parse_spec(to_spec);
    // The source's options are parsed and dropped: glibc applies only the
    // target's.
    let (from, _, _) = parse_spec(from_spec);
    let (Some(from), Some(to)) = (from, to) else {
        errno::set_errno(errno::EINVAL);
        return ICONV_OPEN_ERR;
    };
    if from == Charset::Internal && to == Charset::Internal {
        errno::set_errno(errno::EINVAL);
        return ICONV_OPEN_ERR;
    }
    let Some(desc) = allocate(from, to, translit, ignore) else {
        errno::set_errno(errno::ENOMEM);
        return ICONV_OPEN_ERR;
    };
    if let Some(handle) = lock().insert(desc) {
        return handle;
    }
    // SAFETY: `desc` is the block just allocated, never put in the table.
    unsafe { deallocate(desc) };
    errno::set_errno(errno::ENOMEM);
    ICONV_OPEN_ERR
}

/// Perform character set conversion.
///
/// Converts from `*inbuf` into `*outbuf`, advancing both and counting down
/// `*inbytesleft` and `*outbytesleft` by what went through; see the module
/// docs for where it stops and why.  Returns the number of irreversible
/// conversions, or `(size_t)-1` with `errno`.
///
/// With `inbuf` or `*inbuf` NULL it resets the conversion -- the next call
/// reads (writes) a byte-order mark again -- writing nothing and returning 0,
/// after checking the descriptor (`EBADF`), as glibc does.
///
/// Where glibc reads through a NULL pointer -- `inbytesleft`, `outbuf` or
/// `outbytesleft` on a conversion, or `outbytesleft` beside a non-NULL
/// `*outbuf` on a reset -- this answers `EFAULT` instead; and where glibc's
/// assertion ends the program, on a conversion with `*outbuf` NULL, `EFAULT`
/// too.
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
        let Some(desc) = lock().get(cd) else {
            return fail(errno::EBADF);
        };
        // SAFETY: a live descriptor, handed to this call alone (the module
        // docs: one descriptor, one thread at a time).
        unsafe { (*desc).reset() };
        return 0;
    }

    if inbytesleft.is_null() || outbuf.is_null() || outbytesleft.is_null() {
        return fail(errno::EFAULT);
    }
    // SAFETY: both non-NULL (checked), per the C contract valid to read.
    let (in_left, out_left) = unsafe { (*inbytesleft, *outbytesleft) };
    let Some(desc) = lock().get(cd) else {
        return fail(errno::EBADF);
    };
    if out_start.is_null() {
        return fail(errno::EFAULT);
    }

    // SAFETY: the caller's buffers, for the lengths its counts give.
    let input = unsafe { core::slice::from_raw_parts(in_start, in_left) };
    // SAFETY: as above.
    let output = unsafe { core::slice::from_raw_parts_mut(out_start, out_left) };

    // SAFETY: a live descriptor, handed to this call alone.
    let pass = unsafe { (*desc).convert(input, output) };
    // SAFETY: the four pointers are the caller's, checked non-NULL above;
    // `read` and `wrote` are within the buffers they index.
    unsafe {
        *inbuf = in_start.add(pass.read);
        *inbytesleft = in_left - pass.read;
        *outbuf = out_start.add(pass.wrote);
        *outbytesleft = out_left - pass.wrote;
    }
    match pass.status {
        Status::Ok | Status::EmptyInput => pass.irreversible,
        Status::IllegalInput => fail(errno::EILSEQ),
        Status::FullOutput => fail(errno::E2BIG),
        Status::IncompleteInput => fail(errno::EINVAL),
    }
}

/// Close a conversion descriptor: 0, or -1 with `EBADF` for one that is not
/// open -- `(iconv_t)-1` above all, which glibc refuses the same way.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn iconv_close(cd: IconvT) -> i32 {
    let Some(desc) = lock().remove(cd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    // SAFETY: `desc` came from `allocate` and has just left the table.
    unsafe { deallocate(desc) };
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

    fn open_bytes(to: &[u8], from: &[u8]) -> IconvT {
        let to = [to, b"\0"].concat();
        let from = [from, b"\0"].concat();
        iconv_open(to.as_ptr(), from.as_ptr())
    }

    fn open(to: &str, from: &str) -> IconvT {
        open_bytes(to.as_bytes(), from.as_bytes())
    }

    /// Bytes from hex digits: `"fffe4100"`.
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// What one `iconv` call on `cd` answers: (return, errno, bytes left in
    /// the input, what was written).
    fn call(cd: IconvT, input: &[u8], cap: usize) -> (isize, i32, usize, Vec<u8>) {
        let mut out = vec![0u8; cap];
        let mut ip = input.as_ptr();
        let mut il = input.len();
        let mut op = out.as_mut_ptr();
        let mut ol = cap;
        errno::set_errno(0);
        let r = unsafe { iconv(cd, &raw mut ip, &raw mut il, &raw mut op, &raw mut ol) };
        let e = errno::get_errno();
        out.truncate(cap - ol);
        #[allow(clippy::cast_possible_wrap)]
        (r as isize, e, il, out)
    }

    /// `call` on a fresh descriptor, closed after.
    fn run(to: &str, from: &str, input: &[u8], cap: usize) -> (isize, i32, usize, Vec<u8>) {
        let cd = open(to, from);
        assert_ne!(cd, ICONV_OPEN_ERR, "{to} <- {from}");
        let answer = call(cd, input, cap);
        assert_eq!(iconv_close(cd), 0);
        answer
    }

    fn reset(cd: IconvT) {
        let r = unsafe {
            iconv(
                cd,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(r, 0);
    }

    const OK: i32 = 0;
    const EILSEQ: i32 = errno::EILSEQ;
    const EINVAL: i32 = errno::EINVAL;
    const E2BIG: i32 = errno::E2BIG;

    /// A table row: (target, the first call's return, `errno`, bytes left
    /// and output in hex; the second call's output; the output after a
    /// reset).
    type Encoding<'a> = (&'a str, isize, i32, usize, &'a str, &'a str, &'a str);
    /// A table row: (the other character set, input, return, `errno`, bytes
    /// left, output in hex).
    type OneWay<'a> = (&'a str, &'a [u8], isize, i32, usize, &'a str);
    /// A table row: (target, source, input, room, return, `errno`, bytes
    /// left, output in hex).
    type Case<'a> = (
        &'a str,
        &'a str,
        &'a [u8],
        usize,
        isize,
        i32,
        usize,
        &'a str,
    );
    /// As [`Case`], in 32 bytes of room.
    type Roomy<'a> = (&'a str, &'a str, &'a [u8], isize, i32, usize, &'a str);

    // -- what Ubuntu 24.04's glibc 2.39 answered, probed 2026-09-26 --

    #[test]
    fn unencodable_is_eilseq_not_a_question_mark() {
        assert_eq!(
            run("ASCII", "UTF-8", "a\u{e9}z".as_bytes(), 16),
            (-1, EILSEQ, 3, b"a".to_vec())
        );
        assert_eq!(
            run("ASCII", "ISO-8859-1", b"a\xe9z", 16),
            (-1, EILSEQ, 2, b"a".to_vec())
        );
        assert_eq!(
            run("ISO-8859-1", "UTF-8", "a\u{20ac}z".as_bytes(), 16),
            (-1, EILSEQ, 4, b"a".to_vec())
        );
    }

    #[test]
    fn translit_is_glibcs_c_locale_table() {
        let t = |s: &str| run("ASCII//TRANSLIT", "UTF-8", s.as_bytes(), 32);
        assert_eq!(
            t("a\u{e9}z"),
            (1, OK, 0, b"a?z".to_vec()),
            "not in the table: ?"
        );
        assert_eq!(t("\u{20ac}"), (1, OK, 0, b"EUR".to_vec()));
        assert_eq!(t("\u{a9}"), (1, OK, 0, b"(C)".to_vec()));
        assert_eq!(t("\u{ab}"), (1, OK, 0, b"<<".to_vec()));
        assert_eq!(t("\u{fb01}"), (1, OK, 0, b"fi".to_vec()));
        assert_eq!(t("\u{df}"), (1, OK, 0, b"ss".to_vec()));
        assert_eq!(t("\u{201c}x\u{201d}"), (2, OK, 0, b"\"x\"".to_vec()));
        assert_eq!(
            run("ISO-8859-1//TRANSLIT", "UTF-8", "\u{20ac}".as_bytes(), 32),
            (1, OK, 0, b"EUR".to_vec())
        );
        assert_eq!(
            run("ASCII//TRANSLIT", "ISO-8859-1", b"\xe9", 32),
            (1, OK, 0, b"?".to_vec())
        );
    }

    #[test]
    fn a_substitute_that_does_not_fit_is_e2big_and_not_written() {
        assert_eq!(
            run("ASCII//TRANSLIT", "UTF-8", "\u{20ac}".as_bytes(), 2),
            (-1, E2BIG, 3, Vec::new())
        );
    }

    #[test]
    fn ignore_skips_and_then_fails_with_eilseq() {
        assert_eq!(
            run("ASCII//IGNORE", "UTF-8", "a\u{4e00}b".as_bytes(), 32),
            (-1, EILSEQ, 0, b"ab".to_vec())
        );
        assert_eq!(
            run("ASCII//IGNORE", "UTF-8", b"a\xffb", 32),
            (-1, EILSEQ, 0, b"ab".to_vec()),
            "invalid input is skipped too"
        );
        assert_eq!(
            run("UTF-8//IGNORE", "UTF-8", b"a\xffb", 32),
            (-1, EILSEQ, 0, b"ab".to_vec())
        );
    }

    #[test]
    fn a_full_buffer_stops_before_a_character_ignore_would_skip() {
        assert_eq!(
            run("ASCII//IGNORE", "UTF-8", "a\u{4e00}b".as_bytes(), 1),
            (-1, E2BIG, 4, b"a".to_vec())
        );
    }

    #[test]
    fn translit_comes_before_ignore() {
        for to in ["ASCII//TRANSLIT//IGNORE", "ASCII//TRANSLIT,IGNORE"] {
            assert_eq!(
                run(to, "UTF-8", "a\u{4e00}b".as_bytes(), 32),
                (1, OK, 0, b"a?b".to_vec()),
                "{to}"
            );
        }
    }

    #[test]
    fn options_are_the_targets_and_any_case() {
        assert_eq!(
            run("ascii//translit", "utf-8", "\u{e9}".as_bytes(), 32),
            (1, OK, 0, b"?".to_vec())
        );
        assert_eq!(
            run("ASCII//FOO", "UTF-8", b"ab", 32),
            (0, OK, 0, b"ab".to_vec())
        );
        assert_eq!(
            run("UTF-8", "UTF-8//IGNORE", b"a\xffb", 32),
            (-1, EILSEQ, 2, b"a".to_vec()),
            "the source's //IGNORE is not applied"
        );
        assert_eq!(
            run("UTF-8", "UTF-16LE//IGNORE", b"\x00\xdcA\x00", 32),
            (-1, EILSEQ, 4, Vec::new())
        );
    }

    #[test]
    fn utf8_input_is_validated_even_to_utf8() {
        let t = |b: &[u8]| run("UTF-8", "UTF-8", b, 32);
        assert_eq!(t(b"a\xffz"), (-1, EILSEQ, 2, b"a".to_vec()));
        assert_eq!(t(b"a\xc3"), (-1, EINVAL, 1, b"a".to_vec()));
        assert_eq!(t(b"\xed\xa0\x80"), (-1, EILSEQ, 3, Vec::new()), "surrogate");
        assert_eq!(t(b"\xc1\xbf"), (-1, EILSEQ, 2, Vec::new()), "overlong");
        assert_eq!(t(b"\xe0\x9f\xbf"), (-1, EILSEQ, 3, Vec::new()), "overlong");
        assert_eq!(
            t(b"\xf0\x8f\xbf\xbf"),
            (-1, EILSEQ, 4, Vec::new()),
            "overlong"
        );
        assert_eq!(
            t(b"\x80"),
            (-1, EILSEQ, 1, Vec::new()),
            "stray continuation"
        );
        assert_eq!(t(b"\xfe"), (-1, EILSEQ, 1, Vec::new()));
        assert_eq!(
            t(b"\xc2\x41"),
            (-1, EILSEQ, 2, Vec::new()),
            "bad continuation"
        );
        assert_eq!(t(b"\xe1\x80"), (-1, EINVAL, 2, Vec::new()), "cut off");
        assert_eq!(t(b"\xe1\x80\x41"), (-1, EILSEQ, 3, Vec::new()));
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
            assert_eq!(t(s), (0, OK, 0, s.to_vec()), "{s:x?}");
        }
        assert_eq!(
            run("ISO-8859-1", "UTF-8", b"\xf4\x90\x80\x80", 32),
            (-1, EILSEQ, 4, Vec::new()),
            "decoded, then unencodable"
        );
    }

    #[test]
    fn ascii_input_above_0x7f_is_invalid() {
        assert_eq!(
            run("UTF-8", "ASCII", b"a\xe9z", 16),
            (-1, EILSEQ, 2, b"a".to_vec())
        );
        assert_eq!(
            run("UTF-8", "ASCII", b"\x7f", 16),
            (0, OK, 0, b"\x7f".to_vec())
        );
        assert_eq!(run("ASCII", "UTF-8", b"\0", 16), (0, OK, 0, b"\0".to_vec()));
    }

    #[test]
    fn e2big_stops_before_the_character_that_does_not_fit() {
        assert_eq!(
            run("UTF-8", "ISO-8859-1", b"a\xe9z", 2),
            (-1, E2BIG, 2, b"a".to_vec())
        );
        assert_eq!(
            run("UTF-8", "UTF-8", b"abc", 2),
            (-1, E2BIG, 1, b"ab".to_vec())
        );
    }

    #[test]
    fn latin1_round_trips_through_utf8() {
        let all: Vec<u8> = (0..=255).collect();
        let (r, e, left, utf8) = run("UTF-8", "ISO-8859-1", &all, 512);
        assert_eq!((r, e, left), (0, OK, 0));
        assert_eq!(
            utf8,
            all.iter()
                .map(|&b| char::from(b))
                .collect::<std::string::String>()
                .into_bytes()
        );
        let (r, e, left, back) = run("ISO-8859-1", "UTF-8", &utf8, 512);
        assert_eq!((r, e, left, back), (0, OK, 0, all));
    }

    #[test]
    fn the_empty_name_is_the_c_locales_ascii() {
        assert_eq!(
            run("", "UTF-8", "a\u{e9}".as_bytes(), 32),
            (-1, EILSEQ, 2, b"a".to_vec())
        );
    }

    // -- the character sets beside UTF-8, ASCII and Latin-1 --

    /// "A", e-acute, the euro sign and U+1F600, from UTF-8 into each target;
    /// then "B"; then, after a reset, "C" -- the mark written first, and
    /// again after the reset.
    #[test]
    fn the_encoders_are_glibcs() {
        let text = "A\u{e9}\u{20ac}\u{1f600}".as_bytes();
        #[rustfmt::skip]
        let cases: &[Encoding] = &[
            ("UTF-16", 0, OK, 0, "fffe4100e900ac203dd800de", "4200", "fffe4300"),
            ("UTF16", 0, OK, 0, "fffe4100e900ac203dd800de", "4200", "fffe4300"),
            ("UTF-16LE", 0, OK, 0, "4100e900ac203dd800de", "4200", "4300"),
            ("UTF-16BE", 0, OK, 0, "004100e920acd83dde00", "0042", "0043"),
            ("UTF-32", 0, OK, 0, "fffe000041000000e9000000ac20000000f60100", "42000000", "fffe000043000000"),
            ("UTF-32LE", 0, OK, 0, "41000000e9000000ac20000000f60100", "42000000", "43000000"),
            ("UTF-32BE", 0, OK, 0, "00000041000000e9000020ac0001f600", "00000042", "00000043"),
            ("UCS-2", -1, EILSEQ, 4, "4100e900ac20", "4200", "4300"),
            ("UCS2", -1, EILSEQ, 4, "4100e900ac20", "4200", "4300"),
            ("UCS-2LE", -1, EILSEQ, 4, "4100e900ac20", "4200", "4300"),
            ("UCS-2BE", -1, EILSEQ, 4, "004100e920ac", "0042", "0043"),
            ("UCS-4", 0, OK, 0, "00000041000000e9000020ac0001f600", "00000042", "00000043"),
            ("UCS4", 0, OK, 0, "00000041000000e9000020ac0001f600", "00000042", "00000043"),
            ("UCS-4BE", 0, OK, 0, "00000041000000e9000020ac0001f600", "00000042", "00000043"),
            ("UCS-4LE", 0, OK, 0, "41000000e9000000ac20000000f60100", "42000000", "43000000"),
            ("WCHAR_T", 0, OK, 0, "41000000e9000000ac20000000f60100", "42000000", "43000000"),
            ("CP1252", -1, EILSEQ, 4, "41e980", "42", "43"),
            ("WINDOWS-1252", -1, EILSEQ, 4, "41e980", "42", "43"),
            ("MS-ANSI", -1, EILSEQ, 4, "41e980", "42", "43"),
            ("UNICODE", -1, EILSEQ, 4, "fffe4100e900ac20", "4200", "fffe4300"),
        ];
        for &(to, r, e, left, first, second, after_reset) in cases {
            let cd = open(to, "UTF-8");
            assert_ne!(cd, ICONV_OPEN_ERR, "{to}");
            assert_eq!(call(cd, text, 64), (r, e, left, hex(first)), "{to}");
            assert_eq!(call(cd, b"B", 64), (0, OK, 0, hex(second)), "{to}: again");
            reset(cd);
            assert_eq!(
                call(cd, b"C", 64),
                (0, OK, 0, hex(after_reset)),
                "{to}: after the reset"
            );
            assert_eq!(iconv_close(cd), 0);
        }
    }

    #[test]
    fn the_decoders_are_glibcs() {
        #[rustfmt::skip]
        let cases: &[OneWay] = &[
            ("UTF-16", b"\xff\xfeA\0", 0, OK, 0, "41"),
            ("UTF-16", b"\xfe\xff\0A", 0, OK, 0, "41"),
            ("UTF-16", b"A\0B\0", 0, OK, 0, "4142"),
            ("UTF-16", b"\0A\0B", 0, OK, 0, "e48480e48880"),
            ("UTF-16LE", b"\xff\xfeA\0", 0, OK, 0, "efbbbf41"),
            ("UTF-16BE", b"\xfe\xff\0A", 0, OK, 0, "efbbbf41"),
            ("UTF-32", b"\xff\xfe\0\0A\0\0\0", 0, OK, 0, "41"),
            ("UTF-32", b"\0\0\xfe\xff\0\0\0A", 0, OK, 0, "41"),
            ("UTF-32", b"A\0\0\0", 0, OK, 0, "41"),
            ("UTF-32", b"\0\0\0A", -1, EILSEQ, 4, ""),
            ("UCS-2", b"A\0", 0, OK, 0, "41"),
            ("UCS-2", b"\xff\xfeA\0", 0, OK, 0, "efbbbf41"),
            ("UCS-4", b"\0\0\0A", 0, OK, 0, "41"),
            ("UCS-4", b"\xff\xfe\0\0A\0\0\0", -1, EILSEQ, 8, ""),
            ("WCHAR_T", b"A\0\0\0", 0, OK, 0, "41"),
            ("UNICODE", b"A\0", 0, OK, 0, "41"),
            ("UNICODE", b"\xfe\xff\0A", 0, OK, 0, "41"),
            ("UTF-16LE", b"\x00\xdc", -1, EILSEQ, 2, ""),
            ("UTF-16LE", b"\x3d\xd8", -1, EINVAL, 2, ""),
            ("UTF-16LE", b"\x3d\xd8A\0", -1, EILSEQ, 4, ""),
            ("UTF-16LE", b"\x3d\xd8\x00\xde", 0, OK, 0, "f09f9880"),
            ("UCS-2LE", b"\x3d\xd8\x00\xde", -1, EILSEQ, 4, ""),
            ("UTF-32LE", b"\x00\xd8\0\0", -1, EILSEQ, 4, ""),
            ("UTF-32LE", b"\0\0\x11\0", -1, EILSEQ, 4, ""),
            ("UCS-4LE", b"\0\0\x11\0", 0, OK, 0, "f4908080"),
            ("UCS-4LE", b"\0\0\0\x80", -1, EILSEQ, 4, ""),
            ("UTF-16LE", b"A", -1, EINVAL, 1, ""),
            ("UTF-32LE", b"A\0\0", -1, EINVAL, 3, ""),
            ("CP1252", b"\x80\x81\x8d\x8f\x90\x9d\x9f\xa0", -1, EILSEQ, 7, "e282ac"),
            ("CP1252", b"\x80", 0, OK, 0, "e282ac"),
            ("CP1252", b"\x81", -1, EILSEQ, 1, ""),
            ("CP1252", b"\x9f", 0, OK, 0, "c5b8"),
        ];
        for &(from, input, r, e, left, out) in cases {
            assert_eq!(
                run("UTF-8", from, input, 64),
                (r, e, left, hex(out)),
                "{from} {input:x?}"
            );
        }
    }

    #[test]
    fn unwritable_characters_are_glibcs() {
        #[rustfmt::skip]
        let cases: &[OneWay] = &[
            ("UCS-2", b"\xf0\x9f\x98\x80", -1, EILSEQ, 4, ""),
            ("UCS-2", b"\xef\xbf\xbe", 0, OK, 0, "feff"),
            ("CP1252", b"\xe2\x82\xac", 0, OK, 0, "80"),
            ("CP1252", b"\xc2\x81", -1, EILSEQ, 2, ""),
            ("CP1252", b"\xc4\x80", -1, EILSEQ, 2, ""),
            ("CP1252//TRANSLIT", b"\xc4\x80", 1, OK, 0, "3f"),
            ("CP1252", b"\xef\xbf\xbf", -1, EILSEQ, 3, ""),
            ("CP1252", b"\0", 0, OK, 0, "00"),
            ("CP1252//TRANSLIT", b"\xe2\x89\xa0", 1, OK, 0, "3f"),
            ("CP1252//TRANSLIT", b"\xc2\xa9", 0, OK, 0, "a9"),
            ("UTF-16LE", b"\xf4\x90\x80\x80", -1, EILSEQ, 4, ""),
            ("UCS-4", b"\xf4\x90\x80\x80", 0, OK, 0, "00110000"),
            ("UTF-32", b"\xf4\x90\x80\x80", -1, EILSEQ, 4, "fffe0000"),
        ];
        for &(to, input, r, e, left, out) in cases {
            assert_eq!(
                run(to, "UTF-8", input, 64),
                (r, e, left, hex(out)),
                "{to} {input:x?}"
            );
        }
        assert_eq!(run("UTF-8", "CP1252", b"\0", 8), (0, OK, 0, hex("00")));
    }

    #[test]
    fn cp1252_is_glibcs_table() {
        let all: Vec<u8> = (0..=255).collect();
        let holes = [0x81u8, 0x8d, 0x8f, 0x90, 0x9d];
        let (r, e, left, utf8) = run("UTF-8//IGNORE", "CP1252", &all, 1024);
        assert_eq!((r, e, left), (-1, EILSEQ, 0), "the five holes are skipped");
        let defined: Vec<u8> = all.iter().copied().filter(|b| !holes.contains(b)).collect();
        assert_eq!(defined.len(), 251);
        let (r, e, left, back) = run("CP1252", "UTF-8", &utf8, 1024);
        assert_eq!((r, e, left, back), (0, OK, 0, defined), "and round-trip");
        for b in holes {
            assert_eq!(run("UTF-8", "CP1252", &[b], 8), (-1, EILSEQ, 1, Vec::new()));
        }
    }

    #[test]
    fn a_marked_target_writes_its_mark_once_it_fits() {
        let cd = open("UTF-16", "UTF-8");
        assert_eq!(
            call(cd, b"A", 2),
            (-1, E2BIG, 1, hex("fffe")),
            "the mark fits"
        );
        assert_eq!(
            call(cd, b"A", 4),
            (0, OK, 0, hex("4100")),
            "and is not due again"
        );
        assert_eq!(iconv_close(cd), 0);

        let cd = open("UTF-16", "UTF-8");
        assert_eq!(call(cd, b"A", 3), (-1, E2BIG, 1, hex("fffe")));
        assert_eq!(iconv_close(cd), 0);

        let cd = open("UTF-16", "UTF-8");
        assert_eq!(
            call(cd, b"", 8),
            (0, OK, 0, Vec::new()),
            "no character, no mark"
        );
        assert_eq!(call(cd, b"A", 8), (0, OK, 0, hex("fffe4100")));
        assert_eq!(iconv_close(cd), 0);

        let cd = open("UTF-16", "UTF-8");
        assert_eq!(
            call(cd, b"\xff", 32),
            (-1, EILSEQ, 1, Vec::new()),
            "nothing decoded"
        );
        assert_eq!(call(cd, b"A", 32), (0, OK, 0, hex("fffe4100")));
        assert_eq!(iconv_close(cd), 0);

        let cd = open("UTF-16", "UTF-8");
        assert_eq!(
            call(cd, b"A", 1),
            (-1, E2BIG, 1, Vec::new()),
            "the mark does not fit"
        );
        assert_eq!(call(cd, b"A", 8), (0, OK, 0, hex("fffe4100")));
        assert_eq!(iconv_close(cd), 0);

        let cd = open("UTF-32", "UTF-8");
        assert_eq!(
            call(cd, b"\xf4\x90\x80\x80", 32),
            (-1, EILSEQ, 4, hex("fffe0000")),
            "the mark goes out before a character the target refuses"
        );
        assert_eq!(call(cd, b"A", 32), (0, OK, 0, hex("41000000")));
        assert_eq!(iconv_close(cd), 0);

        assert_eq!(
            run("UTF-16//IGNORE", "UTF-8", b"\xffA", 32),
            (-1, EILSEQ, 0, hex("fffe4100"))
        );
    }

    #[test]
    fn from_wchar_t_a_mark_is_written_at_once() {
        let t = |to: &str, input: &[u8], cap| run(to, "WCHAR_T", input, cap);
        assert_eq!(
            t("UTF-16", b"", 8),
            (0, OK, 0, hex("fffe")),
            "input or none"
        );
        assert_eq!(t("UTF-16", b"", 0), (-1, E2BIG, 0, Vec::new()));
        assert_eq!(t("UTF-16", b"A\0", 8), (-1, EINVAL, 2, hex("fffe")));
        assert_eq!(t("UTF-32", b"", 8), (0, OK, 0, hex("fffe0000")));
        assert_eq!(t("UNICODE", b"", 8), (0, OK, 0, hex("fffe")));
        let cd = open("UTF-16", "WCHAR_T");
        assert_eq!(call(cd, b"A\0\0\0", 1), (-1, E2BIG, 4, Vec::new()));
        assert_eq!(call(cd, b"A\0\0\0", 8), (0, OK, 0, hex("fffe4100")));
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn a_marked_source_reads_its_mark_on_the_first_call() {
        let t = |from: &str, input: &[u8], cap| run("UTF-8", from, input, cap);
        assert_eq!(
            t("UTF-16", b"\xff\xfeA\0", 0),
            (-1, E2BIG, 2, Vec::new()),
            "read, then no room"
        );
        assert_eq!(t("UTF-16", b"\xfe\xff\0A", 0), (-1, E2BIG, 2, Vec::new()));
        assert_eq!(t("UTF-16", b"\xff", 32), (-1, EINVAL, 1, Vec::new()));
        assert_eq!(t("UTF-16", b"", 32), (0, OK, 0, Vec::new()));
        assert_eq!(t("UTF-16", b"\xff\xfeA", 32), (-1, EINVAL, 1, Vec::new()));
        assert_eq!(t("UTF-32", b"\xff\xfe\0", 32), (-1, EINVAL, 3, Vec::new()));
        assert_eq!(t("UTF-32", b"\xff\xfe\0\0", 32), (0, OK, 0, Vec::new()));
        assert_eq!(
            run("UTF-16", "UTF-16", b"\xfe\xff\0A", 32),
            (0, OK, 0, hex("fffe4100"))
        );
        assert_eq!(
            run("UTF-32BE", "UTF-16", b"\xfe\xff\0A", 32),
            (0, OK, 0, hex("00000041"))
        );
        assert_eq!(
            run("WCHAR_T", "UTF-16", b"\xfe\xff\0A", 32),
            (0, OK, 0, hex("41000000"))
        );
        assert_eq!(
            run("WCHAR_T", "UTF-16", b"\xfe\xff\0A", 0),
            (-1, E2BIG, 2, Vec::new())
        );

        // Settled by the first call with two bytes, whatever it then does ...
        let cd = open("UTF-8", "UTF-16");
        assert_eq!(call(cd, b"\xff\xfe", 32), (0, OK, 0, Vec::new()));
        assert_eq!(
            call(cd, b"\xfe\xff\0A", 32),
            (0, OK, 0, hex("efbfbee48480")),
            "a later FE FF is U+FFFE, not a mark"
        );
        assert_eq!(iconv_close(cd), 0);
        let cd = open("UTF-8", "UTF-16");
        assert_eq!(call(cd, b"\x00\xdcA\0", 32), (-1, EILSEQ, 4, Vec::new()));
        assert_eq!(
            call(cd, b"\xfe\xff\0A", 32),
            (0, OK, 0, hex("efbfbee48480"))
        );
        assert_eq!(iconv_close(cd), 0);
        // ... and not by one with fewer.
        let cd = open("UTF-8", "UTF-16");
        assert_eq!(call(cd, b"\xfe", 32), (-1, EINVAL, 1, Vec::new()));
        assert_eq!(call(cd, b"\xfe\xff\0A", 32), (0, OK, 0, hex("41")));
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn the_order_a_mark_gives_lasts_until_the_reset() {
        let cd = open("UTF-8", "UTF-16");
        assert_eq!(call(cd, b"\xfe\xff\0A", 64), (0, OK, 0, hex("41")));
        assert_eq!(call(cd, b"\0B", 64), (0, OK, 0, hex("42")));
        assert_eq!(
            call(cd, b"\xff\xfeC\0", 64),
            (0, OK, 0, hex("efbfbee48c80")),
            "a mark in mid-stream is a character, in the order already read"
        );
        reset(cd);
        // glibc reads this as U+4400: its reset forgets that a mark was read,
        // not the order it gave (module docs, "Where this is not glibc").
        assert_eq!(call(cd, b"D\0", 64), (0, OK, 0, hex("44")));
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn a_mark_is_read_once_even_when_the_first_call_stops_short() {
        let cd = open("UTF-8", "UTF-16");
        assert_eq!(
            call(cd, b"\xff\xfe\xff\xfeA\0", 0),
            (-1, E2BIG, 4, Vec::new())
        );
        // glibc reads this FF FE as a mark again and loses the U+FEFF (module
        // docs, "Where this is not glibc").
        assert_eq!(call(cd, b"\xff\xfeA\0", 32), (0, OK, 0, hex("efbbbf41")));
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn tag_characters_are_dropped_by_the_targets_that_cannot_write_them() {
        let tagged = b"a\xf3\xa0\x81\x81b";
        #[rustfmt::skip]
        let cases: &[(&str, &str)] = &[
            ("ASCII", "6162"),
            ("ASCII//TRANSLIT", "6162"),
            ("ASCII//IGNORE", "6162"),
            ("ISO-8859-1", "6162"),
            ("UCS-2", "61006200"),
            ("UCS-2BE", "00610062"),
            ("UNICODE", "fffe61006200"),
            ("CP1252", "6162"),
            ("CP1252//TRANSLIT", "6162"),
            ("UTF-16LE", "610040db41dc6200"),
            ("UCS-4", "00000061000e004100000062"),
        ];
        for &(to, out) in cases {
            assert_eq!(run(to, "UTF-8", tagged, 32), (0, OK, 0, hex(out)), "{to}");
        }
        assert_eq!(
            run("ASCII", "UTF-8", b"\xf3\xa0\x81\xbf", 32),
            (0, OK, 0, Vec::new())
        );
        assert_eq!(
            run("ASCII", "UTF-8", b"\xf3\xa0\x80\x80", 32),
            (0, OK, 0, Vec::new())
        );
        assert_eq!(
            run("ASCII", "UTF-8", b"\xf3\xa0\x82\x80", 32),
            (-1, EILSEQ, 4, Vec::new()),
            "U+E0080 is not one"
        );
    }

    #[test]
    fn two_steps_decode_ahead_of_the_output() {
        #[rustfmt::skip]
        let cases: &[Case] = &[
            ("UTF-8", "UTF-8", b"ab\xc3", 2, -1, EINVAL, 1, "6162"),
            ("UTF-8", "UTF-8", b"ab\xff", 2, -1, EILSEQ, 1, "6162"),
            ("ASCII//IGNORE", "UTF-8", b"a\xff", 1, -1, EILSEQ, 0, "61"),
            ("ASCII//IGNORE", "UTF-8", b"a\xffb", 1, -1, E2BIG, 2, "61"),
            ("ASCII//IGNORE", "UTF-8", b"a\xff\xffb", 1, -1, E2BIG, 3, "61"),
            ("ASCII//IGNORE", "UTF-8", b"a\xe4\xb8\x80", 1, -1, E2BIG, 3, "61"),
            ("UTF-8", "UCS-4", b"\0\0\0AB\0", 1, -1, EINVAL, 2, "41"),
            ("UTF-16LE", "UTF-8", b"A", 1, -1, E2BIG, 1, ""),
            ("UTF-16LE", "UTF-8", b"A\xf0\x9f\x98\x80", 5, -1, E2BIG, 4, "4100"),
            ("UTF-8", "UTF-16LE", b"A\0B", 1, -1, EINVAL, 1, "41"),
            ("UTF-8", "UTF-16LE", b"\x3d\xd8\x00", 32, -1, EINVAL, 3, ""),
            ("UTF-8", "UTF-16LE", b"\x3d\xd8A\0", 32, -1, EILSEQ, 4, ""),
        ];
        for &(to, from, input, cap, r, e, left, out) in cases {
            assert_eq!(
                run(to, from, input, cap),
                (r, e, left, hex(out)),
                "{to} <- {from} {input:x?} in {cap}"
            );
        }
    }

    #[test]
    fn one_step_asks_for_room_first() {
        #[rustfmt::skip]
        let cases: &[Case] = &[
            ("WCHAR_T", "UTF-8", b"a\xff", 4, -1, E2BIG, 1, "61000000"),
            ("WCHAR_T", "UTF-8", b"a\xc3", 4, -1, E2BIG, 1, "61000000"),
            ("WCHAR_T//IGNORE", "UTF-8", b"a\xff", 4, -1, E2BIG, 1, "61000000"),
            ("UCS-4", "WCHAR_T", b"A\0\0\0B\0", 4, -1, E2BIG, 2, "00000041"),
            ("UCS-4LE", "WCHAR_T", b"A\0\0\0B\0", 4, -1, E2BIG, 2, "41000000"),
            ("UTF-8", "WCHAR_T", b"A\0\0\0B\0", 1, -1, EINVAL, 2, "41"),
            ("WCHAR_T", "UCS-4", b"\0\0\0AB\0", 4, -1, E2BIG, 2, "41000000"),
            ("WCHAR_T", "UCS-4LE", b"A\0\0\0B\0", 4, -1, EINVAL, 2, "41000000"),
        ];
        for &(to, from, input, cap, r, e, left, out) in cases {
            assert_eq!(
                run(to, from, input, cap),
                (r, e, left, hex(out)),
                "{to} <- {from} {input:x?} in {cap}"
            );
        }
    }

    #[test]
    fn ignore_as_each_of_glibcs_modules_does_it() {
        #[rustfmt::skip]
        let cases: &[Roomy] = &[
            // Skipped without failing: UCS-4's decoders, reversed UCS-2's --
            // counted only where the step is the last.
            ("UTF-8//IGNORE", "UCS-4", b"\x80\0\0\0\0\0\0A", 0, OK, 0, "41"),
            ("WCHAR_T//IGNORE", "UCS-4", b"\x80\0\0\0\0\0\0A", 1, OK, 0, "41000000"),
            ("UTF-8//IGNORE", "UCS-4LE", b"\0\0\0\x80A\0\0\0", 0, OK, 0, "41"),
            ("UTF-8//IGNORE", "UCS-2BE", b"\xd8\x00\x00A", 0, OK, 0, "41"),
            ("WCHAR_T//IGNORE", "UCS-2BE", b"\xd8\x00\x00A", 1, OK, 0, "41000000"),
            ("UCS-2BE//IGNORE", "WCHAR_T", b"\x00\xd8\0\0A\0\0\0", 1, OK, 0, "0041"),
            // Skipped, failing the conversion at its end.
            ("UTF-8//IGNORE", "UCS-2", b"\x00\xd8A\0", -1, EILSEQ, 0, "41"),
            ("UCS-2//IGNORE", "WCHAR_T", b"\x00\xd8\0\0A\0\0\0", -1, EILSEQ, 0, "4100"),
            ("UTF-16LE//IGNORE", "WCHAR_T", b"\x00\xd8\0\0A\0\0\0", -1, EILSEQ, 0, "4100"),
            ("UTF-32LE//IGNORE", "WCHAR_T", b"\x00\xd8\0\0A\0\0\0", -1, EILSEQ, 0, "41000000"),
            ("UTF-8//IGNORE", "WCHAR_T", b"\0\0\0\x80A\0\0\0", -1, EILSEQ, 0, "41"),
            ("UCS-2//IGNORE", "UTF-8", b"\xf0\x9f\x98\x80A", -1, EILSEQ, 0, "4100"),
            ("UTF-16LE//IGNORE", "UTF-8", b"\xf4\x90\x80\x80A", -1, EILSEQ, 0, "4100"),
            // UCS-4's encoders check nothing.
            ("UCS-4//IGNORE", "WCHAR_T", b"\x00\xd8\0\0\0\0\0\x80", 0, OK, 0, "0000d80080000000"),
            ("UCS-4", "WCHAR_T", b"\0\0\0\x80", 0, OK, 0, "80000000"),
            ("UCS-4", "UCS-4", b"\0\0\xd8\0", 0, OK, 0, "0000d800"),
            ("UTF-8", "UCS-4", b"\0\0\xd8\0", -1, EILSEQ, 4, ""),
        ];
        for &(to, from, input, r, e, left, out) in cases {
            assert_eq!(
                run(to, from, input, 32),
                (r, e, left, hex(out)),
                "{to} <- {from} {input:x?}"
            );
        }
    }

    #[test]
    fn translit_writes_in_the_targets_own_encoding() {
        #[rustfmt::skip]
        let cases: &[Case] = &[
            ("UTF-8//TRANSLIT", "UCS-4", b"\0\0\xd8\0", 32, 1, OK, 0, "3f"),
            ("UTF-8//TRANSLIT", "WCHAR_T", b"\0\0\0\x80", 32, 1, OK, 0, "3f"),
            ("UCS-2//TRANSLIT", "UTF-8", b"\xf0\x9d\x90\x80", 32, 1, OK, 0, "4100"),
            ("UCS-2BE//TRANSLIT", "UTF-8", b"\xf0\x9d\x90\x80", 32, 1, OK, 0, "0041"),
            ("UCS-2//TRANSLIT", "UTF-8", b"\xf0\x9f\x98\x80", 32, 1, OK, 0, "3f00"),
            ("UCS-2//TRANSLIT", "UTF-8", b"\xf0\x9f\x98\x80", 1, -1, E2BIG, 4, ""),
            ("UTF-16LE//TRANSLIT", "UTF-8", b"\xf4\x90\x80\x80", 32, 1, OK, 0, "3f00"),
            ("UTF-16LE//TRANSLIT", "UTF-8", b"\xf4\x90\x80\x80", 3, 1, OK, 0, "3f00"),
            ("UTF-32LE//TRANSLIT", "UTF-8", b"\xf4\x90\x80\x80", 32, 1, OK, 0, "3f000000"),
            // glibc writes a second mark before each of these substitutes
            // (module docs, "Where this is not glibc").
            ("UTF-16//TRANSLIT", "UTF-8", b"\xf4\x90\x80\x80", 32, 1, OK, 0, "fffe3f00"),
            ("UTF-32//TRANSLIT", "UTF-8", b"\xf4\x90\x80\x80", 32, 1, OK, 0, "fffe00003f000000"),
            ("UNICODE//TRANSLIT", "UTF-8", b"\xf0\x9d\x90\x80", 32, 1, OK, 0, "fffe4100"),
            ("UNICODE//TRANSLIT", "UTF-8", b"A\xf0\x9d\x90\x80\xf0\x9d\x90\x81", 32, 2, OK, 0, "fffe410041004200"),
        ];
        for &(to, from, input, cap, r, e, left, out) in cases {
            assert_eq!(
                run(to, from, input, cap),
                (r, e, left, hex(out)),
                "{to} <- {from} {input:x?} in {cap}"
            );
        }
    }

    #[test]
    fn a_substitute_ends_a_skip_by_the_source() {
        // glibc: the substitution leaves the target's loop with OK, which
        // replaces the source's "something was skipped" for the round.
        assert_eq!(
            run("ASCII//TRANSLIT//IGNORE", "UTF-8", b"\xff\xc2\xa9", 32),
            (1, OK, 0, b"(C)".to_vec())
        );
        assert_eq!(
            run("ASCII//TRANSLIT//IGNORE", "UTF-8", b"\xffa", 32),
            (-1, EILSEQ, 0, b"a".to_vec())
        );
        assert_eq!(
            run("ASCII//TRANSLIT//IGNORE", "UTF-8", b"\xff\xc2\xa9a", 32),
            (1, OK, 0, b"(C)a".to_vec())
        );
        assert_eq!(
            run("ASCII//TRANSLIT//IGNORE", "UTF-8", b"\xc2\xa9\xff", 32),
            (1, OK, 0, b"(C)".to_vec())
        );
    }

    #[test]
    fn the_first_step_decodes_8160_characters_a_round() {
        let ignore = |input: &[u8], cap: usize| run("ASCII//IGNORE", "UTF-8", input, cap);
        let skipped_then = |n: usize| [&b"\xff"[..], &vec![b'a'; n]].concat();
        // A skip in the last round fails the conversion; in an earlier one,
        // it is lost with that round's buffer.
        for (n, r, e) in [
            (8159, -1, EILSEQ),
            (8160, -1, EILSEQ),
            (8161, 0, OK),
            (9000, 0, OK),
        ] {
            let (got_r, got_e, left, out) = ignore(&skipped_then(n), 9100);
            assert_eq!((got_r, got_e, left, out.len()), (r, e, 0, n), "{n}");
        }
        let mut late = vec![b'a'; 9000];
        late.push(0xff);
        let (r, e, left, out) = ignore(&late, 9100);
        assert_eq!((r, e, left, out.len()), (-1, EILSEQ, 0, 9000));
        // Substitutes are counted round after round.
        let both = [&b"\xc2\xa9"[..], &vec![b'a'; 9000], b"\xc2\xa9"].concat();
        let (r, e, left, out) = run("ASCII//TRANSLIT", "UTF-8", &both, 9100);
        assert_eq!((r, e, left, out.len()), (2, OK, 0, 9006));
        // Out of room in the second round: the input stops with the output.
        let (r, e, left, out) = ignore(&skipped_then(9001), 8500);
        assert_eq!((r, e, left, out.len()), (-1, E2BIG, 501, 8500));
    }

    #[test]
    fn a_skip_by_the_target_ends_the_call_after_its_round() {
        // Unlike a skip by the source, the target's is the round's end: the
        // call returns EILSEQ there, with the rest still to convert.
        let input = [&b"\xe4\xb8\x80"[..], &vec![b'a'; 9000]].concat();
        let (r, e, left, out) = run("ASCII//IGNORE", "UTF-8", &input, 9100);
        assert_eq!((r, e), (-1, EILSEQ));
        assert_eq!(out.len(), CHUNK_CHARS - 1);
        assert_eq!(left, input.len() - 3 - (CHUNK_CHARS - 1));
    }

    // -- names --

    #[test]
    fn every_alias_names_its_character_set() {
        let text = "A\u{e9}".as_bytes();
        #[rustfmt::skip]
        let sets: &[(&[&str], isize, &str)] = &[
            (&["ISO-10646/UTF8/", "UTF8", "UTF-8", "ISO-IR-193", "OSF05010001", "ISO-10646/UTF-8/"], 0, "41c3a9"),
            (&["ANSI_X3.4-1968", "ANSI_X3.4", "ISO-IR-6", "ANSI_X3.4-1986", "ISO_646.IRV:1991", "ASCII",
               "ISO646-US", "US-ASCII", "US", "IBM367", "CP367", "CSASCII", "OSF00010020"], -1, "41"),
            (&["ISO-8859-1", "ISO-IR-100", "ISO_8859-1:1987", "ISO_8859-1", "ISO8859-1", "ISO88591",
               "LATIN1", "L1", "IBM819", "CP819", "CSISOLATIN1", "8859_1", "OSF00010001"], 0, "41e9"),
            (&["ISO-10646/UCS4/", "UCS4", "UCS-4", "UCS-4BE", "CSUCS4", "ISO-10646", "10646-1:1993",
               "10646-1:1993/UCS4/", "OSF00010104", "OSF00010105", "OSF00010106"], 0, "00000041000000e9"),
            (&["UCS-4LE", "WCHAR_T"], 0, "41000000e9000000"),
            (&["ISO-10646/UCS2/", "UCS2", "UCS-2", "OSF00010100", "OSF00010101", "OSF00010102",
               "UNICODELITTLE", "UCS-2LE"], 0, "4100e900"),
            (&["UNICODEBIG", "UCS-2BE"], 0, "004100e9"),
            (&["UTF-16", "UTF16", "UNICODE", "CSUNICODE"], 0, "fffe4100e900"),
            (&["UTF-16LE", "UTF16LE"], 0, "4100e900"),
            (&["UTF-16BE", "UTF16BE"], 0, "004100e9"),
            (&["UTF-32", "UTF32"], 0, "fffe000041000000e9000000"),
            (&["UTF-32LE", "UTF32LE"], 0, "41000000e9000000"),
            (&["UTF-32BE", "UTF32BE"], 0, "00000041000000e9"),
            (&["CP1252", "MS-ANSI", "WINDOWS-1252"], 0, "41e9"),
        ];
        let mut count = 0;
        for &(names, r, out) in sets {
            for &name in names {
                let (got_r, _, _, got) = run(name, "UTF-8", text, 32);
                assert_eq!((got_r, got), (r, hex(out)), "{name}");
                count += 1;
            }
        }
        assert_eq!(count, NAMES.len(), "every name in the table, and no other");
    }

    #[test]
    fn names_are_read_as_glibc_reads_them() {
        let text = "A\u{e9}".as_bytes();
        let t = |name: &[u8]| {
            let cd = open_bytes(name, b"UTF-8");
            assert_ne!(
                cd,
                ICONV_OPEN_ERR,
                "{:?}",
                std::string::String::from_utf8_lossy(name)
            );
            let got = call(cd, text, 32);
            assert_eq!(iconv_close(cd), 0);
            got
        };
        let utf8 = (0, OK, 0, hex("41c3a9"));
        for name in [
            &b" utf-8"[..],
            b"utf-8 ",
            b"u t f - 8",
            b"UTF\x01-8",
            b"UTF\xff-8",
            b"utf-8//",
            b"utf-8/",
            b"UTF-8///",
            b"ISO-10646/UTF8",
            b"ISO-10646/UTF8//",
            b"ISO-10646/UTF8/X/Y",
            b"ISO-10646/UTF-8",
            b"UTF-8//TRANSLIT//IGNORE",
            b"UTF-8 //TRANSLIT",
            b"UTF-8//TRANSLIT ",
            b"UTF-8//,",
            b"UTF-8,",
            b"UTF-8/,/",
        ] {
            assert_eq!(
                t(name),
                utf8,
                "{:?}",
                std::string::String::from_utf8_lossy(name)
            );
        }
        assert_eq!(
            t(b"ISO-10646//UTF8"),
            (0, OK, 0, hex("00000041000000e9")),
            "UCS-4"
        );
        assert_eq!(
            t(b"ISO-10646/UCS4/TRANSLIT"),
            (0, OK, 0, hex("00000041000000e9"))
        );
        assert_eq!(t(b"ucs-2le"), (0, OK, 0, hex("4100e900")));
        assert_eq!(t(b"Utf-16"), (0, OK, 0, hex("fffe4100e900")));
        for name in [&b"//"[..], b"/", b"", b",", b" "] {
            assert_eq!(
                t(name),
                (-1, EILSEQ, 2, hex("41")),
                "{name:?}: the locale's ASCII"
            );
        }
        assert_eq!(t(b"//TRANSLIT"), (1, OK, 0, hex("413f")));
        assert_eq!(
            run("UTF-8", " utf-16le ", b"A\0", 32),
            (0, OK, 0, hex("41")),
            "the source's name the same way"
        );
    }

    #[test]
    fn near_misses_are_no_character_set() {
        for name in [
            "INTERNAL",
            "UTF_8",
            "UTF-16-LE",
            "UCS4LE",
            "UCS-4-LE",
            "WCHAR-T",
            "LATIN-1",
            "ISO8859_1",
            "ISO_8859_1",
            "CP-1252",
            "WINDOWS1252",
            "MSANSI",
            "UCS2LE",
            "UCS2BE",
            "UNICODE-1-1",
            "UTF-8/TRANSLIT",
            "UTF-8,TRANSLIT",
            "EBCDIC",
            "NOSUCH",
            "UTF-8x",
        ] {
            errno::set_errno(0);
            assert_eq!(open(name, "UTF-8"), ICONV_OPEN_ERR, "{name}");
            assert_eq!(errno::get_errno(), EINVAL, "{name}");
            errno::set_errno(0);
            assert_eq!(open("UTF-8", name), ICONV_OPEN_ERR, "from {name}");
            assert_eq!(errno::get_errno(), EINVAL);
        }
        let long = "U".repeat(200);
        assert_eq!(open(&long, "UTF-8"), ICONV_OPEN_ERR);
        errno::set_errno(0);
        assert_eq!(
            open("WCHAR_T", "WCHAR_T"),
            ICONV_OPEN_ERR,
            "glibc has no such step"
        );
        assert_eq!(errno::get_errno(), EINVAL);
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
        reset(cd);
        assert_eq!(errno::get_errno(), 0);
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
        assert_eq!((r, ol), (0, 4), "the reset writes nothing");
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

        let cd = open("UTF-16", "UTF-8");
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
        assert_eq!((r, ol), (0, 4), "not even a mark");
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn a_descriptor_that_is_not_open_is_ebadf() {
        let input = b"test";
        let closed = open("UTF-8", "UTF-8");
        assert_eq!(iconv_close(closed), 0);
        let live = open("UTF-8", "ASCII");
        let (index, generation) = unpack(live).unwrap();
        for cd in [
            -1,
            0,
            1,
            6,
            99,
            closed,
            pack(index, generation + 1).unwrap(),
            pack(index + 1, generation).unwrap(),
            pack(0xFFFF_FFFF, 0).unwrap(),
            live ^ (1 << 60),
            IconvT::MIN,
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
            assert_eq!(il, 4);
            errno::set_errno(0);
            assert_eq!(
                (iconv_close(cd), errno::get_errno()),
                (-1, errno::EBADF),
                "{cd:#x}"
            );
        }
        assert_eq!(
            call(live, b"ok", 8),
            (0, OK, 0, b"ok".to_vec()),
            "untouched"
        );
        assert_eq!(iconv_close(live), 0);
    }

    #[test]
    fn a_slot_used_again_gets_a_new_handle() {
        let first = open("UTF-8", "UTF-8");
        assert_eq!(iconv_close(first), 0);
        let second = open("UTF-8", "UTF-8");
        assert_ne!(first, second);
        assert_eq!(
            unpack(first).unwrap().0,
            unpack(second).unwrap().0,
            "the same slot"
        );
        assert_eq!(iconv_close(first), -1, "the old handle names nothing");
        assert_eq!(iconv_close(second), 0);
    }

    #[test]
    fn descriptors_are_independent() {
        let many: Vec<IconvT> = (0..100).map(|_| open("UTF-16", "UTF-8")).collect();
        assert!(many.iter().all(|&cd| cd > 0));
        let mut sorted = many.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 100, "distinct");
        // Each writes its own mark, once.
        for &cd in &many {
            assert_eq!(call(cd, b"A", 8), (0, OK, 0, hex("fffe4100")));
        }
        for &cd in &many {
            assert_eq!(call(cd, b"B", 8), (0, OK, 0, hex("4200")));
            assert_eq!(iconv_close(cd), 0);
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
        // glibc asserts `*outbuf` is not NULL on every conversion, room or not.
        for (in_left, out_left) in [(1usize, 8usize), (1, 0), (0, 8), (0, 0)] {
            let mut null_out: *mut u8 = core::ptr::null_mut();
            let mut il = in_left;
            let mut ol = out_left;
            errno::set_errno(0);
            let r = unsafe { iconv(cd, &raw mut ip, &raw mut il, &raw mut null_out, &raw mut ol) };
            assert_eq!(
                (r, errno::get_errno(), il, ol),
                (usize::MAX, errno::EFAULT, in_left, out_left),
                "*outbuf NULL, {in_left} in, {out_left} out"
            );
        }
        assert_eq!(iconv_close(cd), 0);
    }

    #[test]
    fn a_descriptor_is_never_the_error_value() {
        for from in ["UTF-8", "ASCII", "LATIN1", "UTF-16", "WCHAR_T"] {
            for to in [
                "UTF-8",
                "ASCII//TRANSLIT//IGNORE",
                "LATIN1//IGNORE",
                "UCS-4LE",
            ] {
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
            (1, OK, 0, b"ab".to_vec())
        );
    }
}
