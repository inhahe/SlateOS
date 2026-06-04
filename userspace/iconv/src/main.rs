//! OurOS iconv -- character encoding conversion utility.
//!
//! Converts text from one character encoding to another.  Reads input from
//! files (or stdin), decodes bytes into Unicode codepoints, then re-encodes
//! into the target encoding and writes to stdout (or a file).
//!
//! Supported encodings: UTF-8, UTF-16LE, UTF-16BE, UTF-32LE, UTF-32BE,
//! ASCII, ISO-8859-1 (Latin-1), ISO-8859-15 (Latin-9), Windows-1252, KOI8-R.

use std::env;
use std::io::{self, Read, Write};
use std::process;

// ---------------------------------------------------------------------------
// Error helpers
// ---------------------------------------------------------------------------

fn die(msg: &str) -> ! {
    let _ = writeln!(io::stderr(), "iconv: {msg}");
    process::exit(1);
}

// ---------------------------------------------------------------------------
// Encoding identifiers
// ---------------------------------------------------------------------------

/// Every encoding we support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
    Ascii,
    Iso8859_1,
    Iso8859_15,
    Windows1252,
    Koi8R,
}

/// All known encoding names for `--list`.
const ENCODING_NAMES: &[&str] = &[
    "ASCII",
    "ISO-8859-1",
    "ISO-8859-15",
    "KOI8-R",
    "US-ASCII",
    "UTF-8",
    "UTF-16BE",
    "UTF-16LE",
    "UTF-32BE",
    "UTF-32LE",
    "WINDOWS-1252",
];

/// Normalize an encoding name: uppercase, strip dashes and underscores.
fn normalize_encoding_name(name: &str) -> String {
    name.chars()
        .filter(|&c| c != '-' && c != '_')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Parse a user-supplied encoding name into an `Encoding`.
fn parse_encoding(name: &str) -> Result<Encoding, String> {
    let norm = normalize_encoding_name(name);
    match norm.as_str() {
        "UTF8" => Ok(Encoding::Utf8),
        "UTF16LE" => Ok(Encoding::Utf16Le),
        "UTF16BE" => Ok(Encoding::Utf16Be),
        "UTF32LE" => Ok(Encoding::Utf32Le),
        "UTF32BE" => Ok(Encoding::Utf32Be),
        "ASCII" | "USASCII" => Ok(Encoding::Ascii),
        "ISO88591" | "LATIN1" => Ok(Encoding::Iso8859_1),
        "ISO885915" | "LATIN9" => Ok(Encoding::Iso8859_15),
        "WINDOWS1252" | "CP1252" => Ok(Encoding::Windows1252),
        "KOI8R" => Ok(Encoding::Koi8R),
        _ => Err(format!("unknown encoding: {name}")),
    }
}

// ---------------------------------------------------------------------------
// Conversion tables -- ISO-8859-15 differences from ISO-8859-1
// ---------------------------------------------------------------------------

/// ISO-8859-15 maps these byte positions differently from ISO-8859-1.
/// All other 0x00..=0xFF positions are identity (same as Latin-1).
fn iso8859_15_to_unicode(byte: u8) -> u32 {
    match byte {
        0xA4 => 0x20AC, // EURO SIGN
        0xA6 => 0x0160, // LATIN CAPITAL LETTER S WITH CARON
        0xA8 => 0x0161, // LATIN SMALL LETTER S WITH CARON
        0xB4 => 0x017D, // LATIN CAPITAL LETTER Z WITH CARON
        0xB8 => 0x017E, // LATIN SMALL LETTER Z WITH CARON
        0xBC => 0x0152, // LATIN CAPITAL LIGATURE OE
        0xBD => 0x0153, // LATIN SMALL LIGATURE OE
        0xBE => 0x0178, // LATIN CAPITAL LETTER Y WITH DIAERESIS
        _ => u32::from(byte),
    }
}

/// Reverse map: Unicode codepoint -> ISO-8859-15 byte, or None.
fn unicode_to_iso8859_15(cp: u32) -> Option<u8> {
    match cp {
        0x20AC => Some(0xA4),
        0x0160 => Some(0xA6),
        0x0161 => Some(0xA8),
        0x017D => Some(0xB4),
        0x017E => Some(0xB8),
        0x0152 => Some(0xBC),
        0x0153 => Some(0xBD),
        0x0178 => Some(0xBE),
        // The six Latin-1 codepoints that ISO-8859-15 replaced are NOT
        // representable in ISO-8859-15 (currency sign 0xA4, broken bar 0xA6,
        // diaeresis 0xA8, acute accent 0xB4, cedilla 0xB8, vulgar fractions
        // 0xBC-0xBE).  We check for these to avoid mapping them via the
        // identity fallback.
        0x00A4 | 0x00A6 | 0x00A8 | 0x00B4 | 0x00B8 | 0x00BC | 0x00BD | 0x00BE => None,
        0..=0xFF => Some(cp as u8),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Windows-1252 table (0x80..0x9F)
// ---------------------------------------------------------------------------

/// Windows-1252 maps 0x80..0x9F to printable characters.  All other positions
/// 0x00..0x7F and 0xA0..0xFF are identity with Unicode.
const WIN1252_MAP: [u32; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, // 80-87
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D, 0x017D, 0x008F, // 88-8F
    0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, // 90-97
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178, // 98-9F
];

fn windows1252_to_unicode(byte: u8) -> u32 {
    if (0x80..=0x9F).contains(&byte) {
        WIN1252_MAP[(byte - 0x80) as usize]
    } else {
        u32::from(byte)
    }
}

fn unicode_to_windows1252(cp: u32) -> Option<u8> {
    // Fast path: identity range.
    if cp < 0x80 {
        return Some(cp as u8);
    }
    if (0xA0..=0xFF).contains(&cp) {
        return Some(cp as u8);
    }
    // Search the 0x80..0x9F mapping.
    for (i, &mapped) in WIN1252_MAP.iter().enumerate() {
        if mapped == cp {
            return Some((0x80 + i) as u8);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// KOI8-R table
// ---------------------------------------------------------------------------

/// KOI8-R 0x80..0xFF -> Unicode.  0x00..0x7F is ASCII identity.
#[rustfmt::skip]
const KOI8R_MAP: [u32; 128] = [
    // 0x80
    0x2500, 0x2502, 0x250C, 0x2510, 0x2514, 0x2518, 0x251C, 0x2524,
    0x252C, 0x2534, 0x253C, 0x2580, 0x2584, 0x2588, 0x258C, 0x2590,
    // 0x90
    0x2591, 0x2592, 0x2593, 0x2320, 0x25A0, 0x2219, 0x221A, 0x2248,
    0x2264, 0x2265, 0x00A0, 0x2321, 0x00B0, 0x00B2, 0x00B7, 0x00F7,
    // 0xA0
    0x2550, 0x2551, 0x2552, 0x0451, 0x2553, 0x2554, 0x2555, 0x2556,
    0x2557, 0x2558, 0x2559, 0x255A, 0x255B, 0x255C, 0x255D, 0x255E,
    // 0xB0
    0x255F, 0x2560, 0x2561, 0x0401, 0x2562, 0x2563, 0x2564, 0x2565,
    0x2566, 0x2567, 0x2568, 0x2569, 0x256A, 0x256B, 0x256C, 0x00A9,
    // 0xC0
    0x044E, 0x0430, 0x0431, 0x0446, 0x0434, 0x0435, 0x0444, 0x0433,
    0x0445, 0x0438, 0x0439, 0x043A, 0x043B, 0x043C, 0x043D, 0x043E,
    // 0xD0
    0x043F, 0x044F, 0x0440, 0x0441, 0x0442, 0x0443, 0x0436, 0x0432,
    0x044C, 0x044B, 0x0437, 0x0448, 0x044D, 0x0449, 0x0447, 0x044A,
    // 0xE0
    0x042E, 0x0410, 0x0411, 0x0426, 0x0414, 0x0415, 0x0424, 0x0413,
    0x0425, 0x0418, 0x0419, 0x041A, 0x041B, 0x041C, 0x041D, 0x041E,
    // 0xF0
    0x041F, 0x042F, 0x0420, 0x0421, 0x0422, 0x0423, 0x0416, 0x0412,
    0x042C, 0x042B, 0x0417, 0x0428, 0x042D, 0x0429, 0x0427, 0x042A,
];

fn koi8r_to_unicode(byte: u8) -> u32 {
    if byte < 0x80 {
        u32::from(byte)
    } else {
        KOI8R_MAP[(byte - 0x80) as usize]
    }
}

fn unicode_to_koi8r(cp: u32) -> Option<u8> {
    if cp < 0x80 {
        return Some(cp as u8);
    }
    for (i, &mapped) in KOI8R_MAP.iter().enumerate() {
        if mapped == cp {
            return Some((0x80 + i) as u8);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Decoder: bytes -> codepoints
// ---------------------------------------------------------------------------

/// Result of attempting to decode bytes.
#[derive(Debug)]
enum DecodeResult {
    /// Successfully decoded a codepoint consuming `consumed` bytes.
    Codepoint { cp: u32, consumed: usize },
    /// Invalid byte at position 0; `bad_byte` is the offending value.
    Invalid { bad_byte: u8, consumed: usize },
    /// Need more data; the buffer ends in the middle of a multi-byte sequence.
    /// `needed` is the minimum additional bytes required.
    Incomplete { needed: usize },
}

/// Decode one codepoint from `buf` using the given encoding.
fn decode_one(enc: Encoding, buf: &[u8]) -> DecodeResult {
    if buf.is_empty() {
        return DecodeResult::Incomplete { needed: 1 };
    }

    match enc {
        Encoding::Ascii => {
            let b = buf[0];
            if b > 127 {
                DecodeResult::Invalid { bad_byte: b, consumed: 1 }
            } else {
                DecodeResult::Codepoint { cp: u32::from(b), consumed: 1 }
            }
        }

        Encoding::Iso8859_1 => {
            DecodeResult::Codepoint { cp: u32::from(buf[0]), consumed: 1 }
        }

        Encoding::Iso8859_15 => {
            DecodeResult::Codepoint { cp: iso8859_15_to_unicode(buf[0]), consumed: 1 }
        }

        Encoding::Windows1252 => {
            DecodeResult::Codepoint { cp: windows1252_to_unicode(buf[0]), consumed: 1 }
        }

        Encoding::Koi8R => {
            DecodeResult::Codepoint { cp: koi8r_to_unicode(buf[0]), consumed: 1 }
        }

        Encoding::Utf8 => decode_utf8(buf),
        Encoding::Utf16Le => decode_utf16(buf, true),
        Encoding::Utf16Be => decode_utf16(buf, false),
        Encoding::Utf32Le => decode_utf32(buf, true),
        Encoding::Utf32Be => decode_utf32(buf, false),
    }
}

fn decode_utf8(buf: &[u8]) -> DecodeResult {
    let b0 = buf[0];

    if b0 < 0x80 {
        return DecodeResult::Codepoint { cp: u32::from(b0), consumed: 1 };
    }

    let (expected_len, mut cp) = if b0 & 0xE0 == 0xC0 {
        (2, u32::from(b0 & 0x1F))
    } else if b0 & 0xF0 == 0xE0 {
        (3, u32::from(b0 & 0x0F))
    } else if b0 & 0xF8 == 0xF0 {
        (4, u32::from(b0 & 0x07))
    } else {
        // Invalid leading byte (10xxxxxx or 11111xxx).
        return DecodeResult::Invalid { bad_byte: b0, consumed: 1 };
    };

    if buf.len() < expected_len {
        return DecodeResult::Incomplete { needed: expected_len - buf.len() };
    }

    for &b in &buf[1..expected_len] {
        if b & 0xC0 != 0x80 {
            return DecodeResult::Invalid { bad_byte: b0, consumed: 1 };
        }
        cp = (cp << 6) | u32::from(b & 0x3F);
    }

    // Reject overlong encodings.
    let valid = match expected_len {
        2 => cp >= 0x80,
        3 => cp >= 0x800,
        4 => cp >= 0x10000,
        _ => false,
    };
    if !valid || cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return DecodeResult::Invalid { bad_byte: b0, consumed: 1 };
    }

    DecodeResult::Codepoint { cp, consumed: expected_len }
}

fn decode_utf16(buf: &[u8], little_endian: bool) -> DecodeResult {
    if buf.len() < 2 {
        return DecodeResult::Incomplete { needed: 2 - buf.len() };
    }

    let unit = if little_endian {
        u16::from_le_bytes([buf[0], buf[1]])
    } else {
        u16::from_be_bytes([buf[0], buf[1]])
    };

    // High surrogate: need a second code unit.
    if (0xD800..=0xDBFF).contains(&unit) {
        if buf.len() < 4 {
            return DecodeResult::Incomplete { needed: 4 - buf.len() };
        }
        let unit2 = if little_endian {
            u16::from_le_bytes([buf[2], buf[3]])
        } else {
            u16::from_be_bytes([buf[2], buf[3]])
        };
        if !(0xDC00..=0xDFFF).contains(&unit2) {
            return DecodeResult::Invalid { bad_byte: buf[0], consumed: 2 };
        }
        let cp = 0x10000 + (u32::from(unit - 0xD800) << 10) + u32::from(unit2 - 0xDC00);
        DecodeResult::Codepoint { cp, consumed: 4 }
    } else if (0xDC00..=0xDFFF).contains(&unit) {
        // Lone low surrogate: invalid.
        DecodeResult::Invalid { bad_byte: buf[0], consumed: 2 }
    } else {
        DecodeResult::Codepoint { cp: u32::from(unit), consumed: 2 }
    }
}

fn decode_utf32(buf: &[u8], little_endian: bool) -> DecodeResult {
    if buf.len() < 4 {
        return DecodeResult::Incomplete { needed: 4 - buf.len() };
    }

    let cp = if little_endian {
        u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
    } else {
        u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
    };

    if cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
        DecodeResult::Invalid { bad_byte: buf[0], consumed: 4 }
    } else {
        DecodeResult::Codepoint { cp, consumed: 4 }
    }
}

// ---------------------------------------------------------------------------
// Encoder: codepoints -> bytes
// ---------------------------------------------------------------------------

/// Encode a codepoint into `out` using the given encoding.
/// Returns `Ok(bytes_written)` or `Err(())` if the codepoint is unmappable.
fn encode_one(enc: Encoding, cp: u32, out: &mut Vec<u8>) -> Result<usize, ()> {
    match enc {
        Encoding::Ascii => {
            if cp > 0x7F {
                Err(())
            } else {
                out.push(cp as u8);
                Ok(1)
            }
        }

        Encoding::Iso8859_1 => {
            if cp > 0xFF {
                Err(())
            } else {
                out.push(cp as u8);
                Ok(1)
            }
        }

        Encoding::Iso8859_15 => {
            match unicode_to_iso8859_15(cp) {
                Some(b) => { out.push(b); Ok(1) }
                None => Err(()),
            }
        }

        Encoding::Windows1252 => {
            match unicode_to_windows1252(cp) {
                Some(b) => { out.push(b); Ok(1) }
                None => Err(()),
            }
        }

        Encoding::Koi8R => {
            match unicode_to_koi8r(cp) {
                Some(b) => { out.push(b); Ok(1) }
                None => Err(()),
            }
        }

        Encoding::Utf8 => Ok(encode_utf8(cp, out)),
        Encoding::Utf16Le => Ok(encode_utf16(cp, out, true)),
        Encoding::Utf16Be => Ok(encode_utf16(cp, out, false)),
        Encoding::Utf32Le => { out.extend_from_slice(&cp.to_le_bytes()); Ok(4) }
        Encoding::Utf32Be => { out.extend_from_slice(&cp.to_be_bytes()); Ok(4) }
    }
}

fn encode_utf8(cp: u32, out: &mut Vec<u8>) -> usize {
    if cp < 0x80 {
        out.push(cp as u8);
        1
    } else if cp < 0x800 {
        out.push((0xC0 | (cp >> 6)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
        2
    } else if cp < 0x10000 {
        out.push((0xE0 | (cp >> 12)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
        3
    } else {
        out.push((0xF0 | (cp >> 18)) as u8);
        out.push((0x80 | ((cp >> 12) & 0x3F)) as u8);
        out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
        out.push((0x80 | (cp & 0x3F)) as u8);
        4
    }
}

fn encode_utf16(cp: u32, out: &mut Vec<u8>, little_endian: bool) -> usize {
    if cp < 0x10000 {
        let unit = cp as u16;
        let bytes = if little_endian { unit.to_le_bytes() } else { unit.to_be_bytes() };
        out.extend_from_slice(&bytes);
        2
    } else {
        let adjusted = cp - 0x10000;
        let high = (0xD800 + (adjusted >> 10)) as u16;
        let low = (0xDC00 + (adjusted & 0x3FF)) as u16;
        let hb = if little_endian { high.to_le_bytes() } else { high.to_be_bytes() };
        let lb = if little_endian { low.to_le_bytes() } else { low.to_be_bytes() };
        out.extend_from_slice(&hb);
        out.extend_from_slice(&lb);
        4
    }
}

// ---------------------------------------------------------------------------
// Substitution formatting
// ---------------------------------------------------------------------------

/// Format a substitution string, replacing `%02x`-style specifiers with the
/// given value.  Only supports `%02x` and `%x` for simplicity.  The POSIX
/// iconv `--byte-subst` / `--unicode-subst` use this.
fn format_subst(fmt: &str, value: u32) -> Vec<u8> {
    let mut result = Vec::new();
    let bytes = fmt.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            // Try to parse a simple format specifier.
            let rest = &bytes[i..];
            if rest.starts_with(b"%02x") {
                let formatted = format!("{value:02x}");
                result.extend_from_slice(formatted.as_bytes());
                i += 4;
                continue;
            } else if rest.starts_with(b"%04x") {
                let formatted = format!("{value:04x}");
                result.extend_from_slice(formatted.as_bytes());
                i += 4;
                continue;
            } else if rest.starts_with(b"%02X") {
                let formatted = format!("{value:02X}");
                result.extend_from_slice(formatted.as_bytes());
                i += 4;
                continue;
            } else if rest.starts_with(b"%04X") {
                let formatted = format!("{value:04X}");
                result.extend_from_slice(formatted.as_bytes());
                i += 4;
                continue;
            } else if rest.starts_with(b"%x") {
                let formatted = format!("{value:x}");
                result.extend_from_slice(formatted.as_bytes());
                i += 2;
                continue;
            } else if rest.starts_with(b"%X") {
                let formatted = format!("{value:X}");
                result.extend_from_slice(formatted.as_bytes());
                i += 2;
                continue;
            } else if rest.starts_with(b"%%") {
                result.push(b'%');
                i += 2;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// BOM detection for UTF-16
// ---------------------------------------------------------------------------

/// Detect a Byte Order Mark at the start of the buffer and return the
/// effective encoding if one is found, along with the number of BOM bytes
/// to skip.
fn detect_bom(enc: Encoding, buf: &[u8]) -> (Encoding, usize) {
    match enc {
        Encoding::Utf16Le | Encoding::Utf16Be => {
            if buf.len() >= 2 {
                if buf[0] == 0xFF && buf[1] == 0xFE {
                    return (Encoding::Utf16Le, 2);
                }
                if buf[0] == 0xFE && buf[1] == 0xFF {
                    return (Encoding::Utf16Be, 2);
                }
            }
            (enc, 0)
        }
        Encoding::Utf8 => {
            if buf.len() >= 3 && buf[0] == 0xEF && buf[1] == 0xBB && buf[2] == 0xBF {
                return (Encoding::Utf8, 3);
            }
            (enc, 0)
        }
        Encoding::Utf32Le | Encoding::Utf32Be => {
            if buf.len() >= 4 {
                if buf[0] == 0xFF && buf[1] == 0xFE && buf[2] == 0x00 && buf[3] == 0x00 {
                    return (Encoding::Utf32Le, 4);
                }
                if buf[0] == 0x00 && buf[1] == 0x00 && buf[2] == 0xFE && buf[3] == 0xFF {
                    return (Encoding::Utf32Be, 4);
                }
            }
            (enc, 0)
        }
        _ => (enc, 0),
    }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

struct Opts {
    from: Encoding,
    to: Encoding,
    output_file: Option<String>,
    discard_unmappable: bool,
    byte_subst: Option<String>,
    unicode_subst: Option<String>,
    verbose: bool,
    input_files: Vec<String>,
}

fn print_usage() {
    let _ = writeln!(
        io::stderr(),
        "\
Usage: iconv [OPTION...] [-f FROM] [-t TO] [FILE...]
Convert encoding of given files from one encoding to another.

Options:
  -f, --from-code=NAME     encoding of the input
  -t, --to-code=NAME       encoding of the output
  -o, --output=FILE        output file (default: stdout)
  -c                       discard unconvertible characters
  --byte-subst=FORMAT      substitution for unconvertible bytes
  --unicode-subst=FORMAT   substitution for unconvertible codepoints
  -l, --list               list all supported encodings
  --verbose                show conversion statistics
  -h, --help               display this help"
    );
}

fn parse_args() -> Opts {
    let args: Vec<String> = env::args().collect();
    let mut from: Option<Encoding> = None;
    let mut to: Option<Encoding> = None;
    let mut output_file: Option<String> = None;
    let mut discard = false;
    let mut byte_subst: Option<String> = None;
    let mut unicode_subst: Option<String> = None;
    let mut verbose = false;
    let mut input_files: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        // --list / -l
        if arg == "-l" || arg == "--list" {
            for name in ENCODING_NAMES {
                println!("{name}");
            }
            process::exit(0);
        }

        // --help / -h
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        }

        // --verbose
        if arg == "--verbose" {
            verbose = true;
            i += 1;
            continue;
        }

        // -c
        if arg == "-c" {
            discard = true;
            i += 1;
            continue;
        }

        // --byte-subst=FORMAT
        if let Some(val) = arg.strip_prefix("--byte-subst=") {
            byte_subst = Some(val.to_string());
            i += 1;
            continue;
        }

        // --unicode-subst=FORMAT
        if let Some(val) = arg.strip_prefix("--unicode-subst=") {
            unicode_subst = Some(val.to_string());
            i += 1;
            continue;
        }

        // --from-code=NAME
        if let Some(val) = arg.strip_prefix("--from-code=") {
            from = Some(parse_encoding(val).unwrap_or_else(|e| die(&e)));
            i += 1;
            continue;
        }

        // --to-code=NAME
        if let Some(val) = arg.strip_prefix("--to-code=") {
            to = Some(parse_encoding(val).unwrap_or_else(|e| die(&e)));
            i += 1;
            continue;
        }

        // --output=FILE
        if let Some(val) = arg.strip_prefix("--output=") {
            output_file = Some(val.to_string());
            i += 1;
            continue;
        }

        // -f / -t / -o with separate value
        if (arg == "-f" || arg == "-t" || arg == "-o") && i + 1 < args.len() {
            let val = &args[i + 1];
            match arg.as_str() {
                "-f" => from = Some(parse_encoding(val).unwrap_or_else(|e| die(&e))),
                "-t" => to = Some(parse_encoding(val).unwrap_or_else(|e| die(&e))),
                "-o" => output_file = Some(val.clone()),
                _ => {}
            }
            i += 2;
            continue;
        }

        // -- separator
        if arg == "--" {
            i += 1;
            while i < args.len() {
                input_files.push(args[i].clone());
                i += 1;
            }
            break;
        }

        // Unrecognized option
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            // Could be combined short flags like -cf
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            let mut all_known = true;
            while j < chars.len() {
                match chars[j] {
                    'c' => discard = true,
                    'f' => {
                        // Remainder of this arg or next arg is the value.
                        if j + 1 < chars.len() {
                            let rest: String = chars[j + 1..].iter().collect();
                            from = Some(parse_encoding(&rest).unwrap_or_else(|e| die(&e)));
                            j = chars.len(); // consume all
                            continue;
                        } else if i + 1 < args.len() {
                            from = Some(
                                parse_encoding(&args[i + 1]).unwrap_or_else(|e| die(&e)),
                            );
                            i += 1;
                        } else {
                            die("-f requires an argument");
                        }
                    }
                    't' => {
                        if j + 1 < chars.len() {
                            let rest: String = chars[j + 1..].iter().collect();
                            to = Some(parse_encoding(&rest).unwrap_or_else(|e| die(&e)));
                            j = chars.len();
                            continue;
                        } else if i + 1 < args.len() {
                            to = Some(
                                parse_encoding(&args[i + 1]).unwrap_or_else(|e| die(&e)),
                            );
                            i += 1;
                        } else {
                            die("-t requires an argument");
                        }
                    }
                    'o' => {
                        if j + 1 < chars.len() {
                            let rest: String = chars[j + 1..].iter().collect();
                            output_file = Some(rest);
                            j = chars.len();
                            continue;
                        } else if i + 1 < args.len() {
                            output_file = Some(args[i + 1].clone());
                            i += 1;
                        } else {
                            die("-o requires an argument");
                        }
                    }
                    _ => {
                        all_known = false;
                        break;
                    }
                }
                j += 1;
            }
            if !all_known {
                die(&format!("unrecognized option: {arg}"));
            }
            i += 1;
            continue;
        }

        if arg.starts_with("--") {
            die(&format!("unrecognized option: {arg}"));
        }

        // Positional argument: input file.
        input_files.push(arg.clone());
        i += 1;
    }

    let from = match from {
        Some(e) => e,
        None => die("-f/--from-code is required"),
    };
    let to = match to {
        Some(e) => e,
        None => die("-t/--to-code is required"),
    };

    Opts {
        from,
        to,
        output_file,
        discard_unmappable: discard,
        byte_subst,
        unicode_subst,
        verbose,
        input_files,
    }
}

// ---------------------------------------------------------------------------
// Conversion engine
// ---------------------------------------------------------------------------

struct ConvStats {
    bytes_in: u64,
    bytes_out: u64,
    codepoints: u64,
    errors: u64,
}

/// Convert a complete input byte stream from `from_enc` to `to_enc`, writing
/// to `writer`.  Returns conversion statistics and whether any errors
/// occurred.
fn convert_stream<R: Read, W: Write + ?Sized>(
    reader: &mut R,
    writer: &mut W,
    from_enc: Encoding,
    to_enc: Encoding,
    opts: &Opts,
    first_chunk: bool,
) -> io::Result<ConvStats> {
    let mut stats = ConvStats { bytes_in: 0, bytes_out: 0, codepoints: 0, errors: 0 };

    // We keep a buffer of un-consumed input (a remainder from the previous
    // read that ended in the middle of a multi-byte sequence, plus new data).
    let mut remainder: Vec<u8> = Vec::new();
    let mut read_buf = [0u8; 8192];
    let mut out_buf: Vec<u8> = Vec::with_capacity(8192);
    let mut bom_checked = !first_chunk;
    let mut effective_from = from_enc;

    loop {
        let n = reader.read(&mut read_buf)?;
        let eof = n == 0;

        remainder.extend_from_slice(&read_buf[..n]);
        stats.bytes_in += n as u64;

        // BOM detection on the very first chunk of the very first file.
        if !bom_checked && !remainder.is_empty() {
            let (detected, skip) = detect_bom(effective_from, &remainder);
            effective_from = detected;
            if skip > 0 && skip <= remainder.len() {
                remainder.drain(..skip);
            }
            bom_checked = true;
        }

        let mut pos: usize = 0;

        while pos < remainder.len() {
            let slice = &remainder[pos..];
            match decode_one(effective_from, slice) {
                DecodeResult::Codepoint { cp, consumed } => {
                    // Try to encode.
                    match encode_one(to_enc, cp, &mut out_buf) {
                        Ok(_) => {
                            stats.codepoints += 1;
                        }
                        Err(()) => {
                            // Unmappable codepoint.
                            if opts.discard_unmappable {
                                // Silently skip.
                            } else if let Some(ref fmt) = opts.unicode_subst {
                                let sub = format_subst(fmt, cp);
                                out_buf.extend_from_slice(&sub);
                            } else {
                                let _ = writeln!(
                                    io::stderr(),
                                    "iconv: cannot convert U+{cp:04X} to target encoding at byte offset {}",
                                    stats.bytes_in - (remainder.len() - pos) as u64,
                                );
                                stats.errors += 1;
                            }
                            stats.codepoints += 1;
                        }
                    }
                    pos += consumed;
                }
                DecodeResult::Invalid { bad_byte, consumed } => {
                    if opts.discard_unmappable {
                        // Silently skip.
                    } else if let Some(ref fmt) = opts.byte_subst {
                        let sub = format_subst(fmt, u32::from(bad_byte));
                        out_buf.extend_from_slice(&sub);
                    } else {
                        let offset = stats.bytes_in - (remainder.len() - pos) as u64;
                        let _ = writeln!(
                            io::stderr(),
                            "iconv: invalid byte 0x{bad_byte:02x} at offset {offset} in source encoding",
                        );
                        stats.errors += 1;
                    }
                    pos += consumed;
                }
                DecodeResult::Incomplete { needed: _needed } => {
                    if eof {
                        // Truncated sequence at end of input.
                        let bad_byte = slice[0];
                        if opts.discard_unmappable {
                            // skip
                        } else if let Some(ref fmt) = opts.byte_subst {
                            let sub = format_subst(fmt, u32::from(bad_byte));
                            out_buf.extend_from_slice(&sub);
                        } else {
                            let offset = stats.bytes_in - (remainder.len() - pos) as u64;
                            let _ = writeln!(
                                io::stderr(),
                                "iconv: incomplete byte sequence at offset {offset}",
                            );
                            stats.errors += 1;
                        }
                        pos += 1;
                    } else {
                        // Need more data; break out to read more.
                        break;
                    }
                }
            }
        }

        // Flush the output buffer.
        if !out_buf.is_empty() {
            writer.write_all(&out_buf)?;
            stats.bytes_out += out_buf.len() as u64;
            out_buf.clear();
        }

        // Remove consumed bytes from the remainder.
        if pos > 0 {
            remainder.drain(..pos);
        }

        if eof {
            break;
        }
    }

    Ok(stats)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let opts = parse_args();

    // Open output.
    let stdout_handle;
    let mut out_file;
    let writer: &mut dyn Write = if let Some(ref path) = opts.output_file {
        out_file = std::fs::File::create(path).unwrap_or_else(|e| {
            die(&format!("cannot open output file '{path}': {e}"));
        });
        &mut out_file
    } else {
        stdout_handle = io::stdout();
        // Use a raw handle to avoid double-lock overhead.  We never
        // interleave writes, so this is safe.
        // (We store the lock in a variable to keep the borrow alive.)
        &mut stdout_handle.lock() as &mut dyn Write
    };

    let mut total_stats = ConvStats { bytes_in: 0, bytes_out: 0, codepoints: 0, errors: 0 };
    let mut had_error = false;

    if opts.input_files.is_empty() {
        // Read from stdin.
        let mut stdin = io::stdin().lock();
        match convert_stream(&mut stdin, writer, opts.from, opts.to, &opts, true) {
            Ok(s) => {
                if s.errors > 0 {
                    had_error = true;
                }
                total_stats.bytes_in += s.bytes_in;
                total_stats.bytes_out += s.bytes_out;
                total_stats.codepoints += s.codepoints;
                total_stats.errors += s.errors;
            }
            Err(e) => die(&format!("I/O error: {e}")),
        }
    } else {
        for (idx, path) in opts.input_files.iter().enumerate() {
            let mut file = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(e) => {
                    let _ = writeln!(io::stderr(), "iconv: {path}: {e}");
                    had_error = true;
                    continue;
                }
            };
            match convert_stream(&mut file, writer, opts.from, opts.to, &opts, idx == 0) {
                Ok(s) => {
                    if s.errors > 0 {
                        had_error = true;
                    }
                    total_stats.bytes_in += s.bytes_in;
                    total_stats.bytes_out += s.bytes_out;
                    total_stats.codepoints += s.codepoints;
                    total_stats.errors += s.errors;
                }
                Err(e) => {
                    let _ = writeln!(io::stderr(), "iconv: {path}: {e}");
                    had_error = true;
                }
            }
        }
    }

    if opts.verbose {
        let _ = writeln!(
            io::stderr(),
            "iconv: {} bytes in, {} bytes out, {} codepoints, {} errors",
            total_stats.bytes_in,
            total_stats.bytes_out,
            total_stats.codepoints,
            total_stats.errors,
        );
    }

    process::exit(if had_error { 1 } else { 0 });
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Encoding name normalization
    // -----------------------------------------------------------------------

    #[test]
    fn test_normalize_encoding_name() {
        assert_eq!(normalize_encoding_name("UTF-8"), "UTF8");
        assert_eq!(normalize_encoding_name("utf_8"), "UTF8");
        assert_eq!(normalize_encoding_name("Utf-8"), "UTF8");
        assert_eq!(normalize_encoding_name("iso-8859-1"), "ISO88591");
        assert_eq!(normalize_encoding_name("WINDOWS_1252"), "WINDOWS1252");
    }

    #[test]
    fn test_parse_encoding_case_insensitive() {
        assert_eq!(parse_encoding("utf-8").ok(), Some(Encoding::Utf8));
        assert_eq!(parse_encoding("UTF8").ok(), Some(Encoding::Utf8));
        assert_eq!(parse_encoding("Utf_8").ok(), Some(Encoding::Utf8));
    }

    #[test]
    fn test_parse_encoding_us_ascii() {
        assert_eq!(parse_encoding("US-ASCII").ok(), Some(Encoding::Ascii));
        assert_eq!(parse_encoding("us_ascii").ok(), Some(Encoding::Ascii));
        assert_eq!(parse_encoding("ASCII").ok(), Some(Encoding::Ascii));
    }

    #[test]
    fn test_parse_encoding_all_variants() {
        assert!(parse_encoding("UTF-16LE").is_ok());
        assert!(parse_encoding("UTF-16BE").is_ok());
        assert!(parse_encoding("UTF-32LE").is_ok());
        assert!(parse_encoding("UTF-32BE").is_ok());
        assert!(parse_encoding("ISO-8859-1").is_ok());
        assert!(parse_encoding("ISO-8859-15").is_ok());
        assert!(parse_encoding("WINDOWS-1252").is_ok());
        assert!(parse_encoding("KOI8-R").is_ok());
    }

    #[test]
    fn test_parse_encoding_unknown() {
        assert!(parse_encoding("EBCDIC").is_err());
        assert!(parse_encoding("").is_err());
    }

    #[test]
    fn test_parse_encoding_aliases() {
        assert_eq!(parse_encoding("latin1").ok(), Some(Encoding::Iso8859_1));
        assert_eq!(parse_encoding("LATIN-9").ok(), Some(Encoding::Iso8859_15));
        assert_eq!(parse_encoding("CP1252").ok(), Some(Encoding::Windows1252));
    }

    // -----------------------------------------------------------------------
    // UTF-8 decode
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf8_decode_ascii() {
        let buf = b"A";
        match decode_one(Encoding::Utf8, buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x41);
                assert_eq!(consumed, 1);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf8_decode_2byte() {
        // U+00E9 (e with acute) = C3 A9
        let buf = [0xC3, 0xA9];
        match decode_one(Encoding::Utf8, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x00E9);
                assert_eq!(consumed, 2);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf8_decode_3byte() {
        // U+20AC (Euro sign) = E2 82 AC
        let buf = [0xE2, 0x82, 0xAC];
        match decode_one(Encoding::Utf8, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x20AC);
                assert_eq!(consumed, 3);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf8_decode_4byte() {
        // U+1F600 (grinning face) = F0 9F 98 80
        let buf = [0xF0, 0x9F, 0x98, 0x80];
        match decode_one(Encoding::Utf8, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x1F600);
                assert_eq!(consumed, 4);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf8_decode_overlong() {
        // Overlong encoding of U+0000: C0 80
        let buf = [0xC0, 0x80];
        match decode_one(Encoding::Utf8, &buf) {
            DecodeResult::Invalid { .. } => {}
            _ => panic!("expected invalid for overlong"),
        }
    }

    #[test]
    fn test_utf8_decode_surrogate() {
        // U+D800 encoded as UTF-8 would be ED A0 80 - this is invalid.
        let buf = [0xED, 0xA0, 0x80];
        match decode_one(Encoding::Utf8, &buf) {
            DecodeResult::Invalid { .. } => {}
            _ => panic!("expected invalid for surrogate"),
        }
    }

    #[test]
    fn test_utf8_decode_invalid_continuation() {
        // C3 followed by non-continuation byte
        let buf = [0xC3, 0x00];
        match decode_one(Encoding::Utf8, &buf) {
            DecodeResult::Invalid { .. } => {}
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_utf8_decode_incomplete() {
        // Only first byte of a 2-byte sequence.
        let buf = [0xC3];
        match decode_one(Encoding::Utf8, &buf) {
            DecodeResult::Incomplete { needed } => {
                assert_eq!(needed, 1);
            }
            _ => panic!("expected incomplete"),
        }
    }

    // -----------------------------------------------------------------------
    // UTF-8 encode
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf8_encode_ascii() {
        let mut out = Vec::new();
        let n = encode_utf8(0x41, &mut out);
        assert_eq!(n, 1);
        assert_eq!(out, vec![0x41]);
    }

    #[test]
    fn test_utf8_encode_2byte() {
        let mut out = Vec::new();
        let n = encode_utf8(0x00E9, &mut out);
        assert_eq!(n, 2);
        assert_eq!(out, vec![0xC3, 0xA9]);
    }

    #[test]
    fn test_utf8_encode_3byte() {
        let mut out = Vec::new();
        let n = encode_utf8(0x20AC, &mut out);
        assert_eq!(n, 3);
        assert_eq!(out, vec![0xE2, 0x82, 0xAC]);
    }

    #[test]
    fn test_utf8_encode_4byte() {
        let mut out = Vec::new();
        let n = encode_utf8(0x1F600, &mut out);
        assert_eq!(n, 4);
        assert_eq!(out, vec![0xF0, 0x9F, 0x98, 0x80]);
    }

    // -----------------------------------------------------------------------
    // UTF-16 decode
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf16le_decode_bmp() {
        // U+0041 'A' in LE: 41 00
        let buf = [0x41, 0x00];
        match decode_one(Encoding::Utf16Le, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x41);
                assert_eq!(consumed, 2);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf16be_decode_bmp() {
        // U+0041 'A' in BE: 00 41
        let buf = [0x00, 0x41];
        match decode_one(Encoding::Utf16Be, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x41);
                assert_eq!(consumed, 2);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf16le_decode_surrogate_pair() {
        // U+1F600: D83D DE00 in LE: 3D D8 00 DE
        let buf = [0x3D, 0xD8, 0x00, 0xDE];
        match decode_one(Encoding::Utf16Le, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x1F600);
                assert_eq!(consumed, 4);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf16be_decode_surrogate_pair() {
        // U+1F600: D83D DE00 in BE: D8 3D DE 00
        let buf = [0xD8, 0x3D, 0xDE, 0x00];
        match decode_one(Encoding::Utf16Be, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x1F600);
                assert_eq!(consumed, 4);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf16_decode_lone_low_surrogate() {
        // Lone low surrogate DC00 in BE
        let buf = [0xDC, 0x00];
        match decode_one(Encoding::Utf16Be, &buf) {
            DecodeResult::Invalid { .. } => {}
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_utf16_decode_high_surrogate_bad_follow() {
        // High surrogate D800 followed by non-surrogate 0041 in BE
        let buf = [0xD8, 0x00, 0x00, 0x41];
        match decode_one(Encoding::Utf16Be, &buf) {
            DecodeResult::Invalid { .. } => {}
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_utf16_decode_incomplete() {
        let buf = [0x41];
        match decode_one(Encoding::Utf16Le, &buf) {
            DecodeResult::Incomplete { needed } => {
                assert_eq!(needed, 1);
            }
            _ => panic!("expected incomplete"),
        }
    }

    #[test]
    fn test_utf16_decode_incomplete_surrogate() {
        // High surrogate but only 3 bytes available
        let buf = [0xD8, 0x3D, 0xDE];
        match decode_one(Encoding::Utf16Be, &buf) {
            DecodeResult::Incomplete { needed } => {
                assert_eq!(needed, 1);
            }
            _ => panic!("expected incomplete"),
        }
    }

    // -----------------------------------------------------------------------
    // UTF-16 encode
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf16le_encode_bmp() {
        let mut out = Vec::new();
        let n = encode_utf16(0x41, &mut out, true);
        assert_eq!(n, 2);
        assert_eq!(out, vec![0x41, 0x00]);
    }

    #[test]
    fn test_utf16be_encode_bmp() {
        let mut out = Vec::new();
        let n = encode_utf16(0x41, &mut out, false);
        assert_eq!(n, 2);
        assert_eq!(out, vec![0x00, 0x41]);
    }

    #[test]
    fn test_utf16le_encode_supplementary() {
        let mut out = Vec::new();
        let n = encode_utf16(0x1F600, &mut out, true);
        assert_eq!(n, 4);
        assert_eq!(out, vec![0x3D, 0xD8, 0x00, 0xDE]);
    }

    #[test]
    fn test_utf16be_encode_supplementary() {
        let mut out = Vec::new();
        let n = encode_utf16(0x1F600, &mut out, false);
        assert_eq!(n, 4);
        assert_eq!(out, vec![0xD8, 0x3D, 0xDE, 0x00]);
    }

    // -----------------------------------------------------------------------
    // UTF-32 decode/encode
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf32le_decode() {
        let buf = [0x41, 0x00, 0x00, 0x00];
        match decode_one(Encoding::Utf32Le, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x41);
                assert_eq!(consumed, 4);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf32be_decode() {
        let buf = [0x00, 0x00, 0x00, 0x41];
        match decode_one(Encoding::Utf32Be, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x41);
                assert_eq!(consumed, 4);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf32_decode_supplementary() {
        // U+1F600 in LE
        let buf = [0x00, 0xF6, 0x01, 0x00];
        match decode_one(Encoding::Utf32Le, &buf) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x1F600);
                assert_eq!(consumed, 4);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_utf32_decode_invalid_surrogate() {
        // U+D800 in LE: invalid
        let buf = [0x00, 0xD8, 0x00, 0x00];
        match decode_one(Encoding::Utf32Le, &buf) {
            DecodeResult::Invalid { .. } => {}
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_utf32_decode_out_of_range() {
        // 0x110000 in LE: out of Unicode range
        let buf = [0x00, 0x00, 0x11, 0x00];
        match decode_one(Encoding::Utf32Le, &buf) {
            DecodeResult::Invalid { .. } => {}
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_utf32le_encode() {
        let mut out = Vec::new();
        assert!(encode_one(Encoding::Utf32Le, 0x41, &mut out).is_ok());
        assert_eq!(out, vec![0x41, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_utf32be_encode() {
        let mut out = Vec::new();
        assert!(encode_one(Encoding::Utf32Be, 0x41, &mut out).is_ok());
        assert_eq!(out, vec![0x00, 0x00, 0x00, 0x41]);
    }

    #[test]
    fn test_utf32_decode_incomplete() {
        let buf = [0x41, 0x00];
        match decode_one(Encoding::Utf32Le, &buf) {
            DecodeResult::Incomplete { needed } => {
                assert_eq!(needed, 2);
            }
            _ => panic!("expected incomplete"),
        }
    }

    // -----------------------------------------------------------------------
    // ASCII
    // -----------------------------------------------------------------------

    #[test]
    fn test_ascii_decode_valid() {
        match decode_one(Encoding::Ascii, &[0x41]) {
            DecodeResult::Codepoint { cp, consumed } => {
                assert_eq!(cp, 0x41);
                assert_eq!(consumed, 1);
            }
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_ascii_decode_invalid_high_bit() {
        match decode_one(Encoding::Ascii, &[0x80]) {
            DecodeResult::Invalid { bad_byte, .. } => {
                assert_eq!(bad_byte, 0x80);
            }
            _ => panic!("expected invalid"),
        }
    }

    #[test]
    fn test_ascii_encode_valid() {
        let mut out = Vec::new();
        assert!(encode_one(Encoding::Ascii, 0x41, &mut out).is_ok());
        assert_eq!(out, vec![0x41]);
    }

    #[test]
    fn test_ascii_encode_reject_high() {
        let mut out = Vec::new();
        assert!(encode_one(Encoding::Ascii, 0x80, &mut out).is_err());
    }

    // -----------------------------------------------------------------------
    // ISO-8859-1
    // -----------------------------------------------------------------------

    #[test]
    fn test_iso8859_1_decode() {
        // 0xE9 = U+00E9 (e with acute) in Latin-1 (identity map)
        match decode_one(Encoding::Iso8859_1, &[0xE9]) {
            DecodeResult::Codepoint { cp, .. } => assert_eq!(cp, 0xE9),
            _ => panic!("expected codepoint"),
        }
    }

    #[test]
    fn test_iso8859_1_encode_in_range() {
        let mut out = Vec::new();
        assert!(encode_one(Encoding::Iso8859_1, 0xFF, &mut out).is_ok());
        assert_eq!(out, vec![0xFF]);
    }

    #[test]
    fn test_iso8859_1_encode_out_of_range() {
        let mut out = Vec::new();
        assert!(encode_one(Encoding::Iso8859_1, 0x100, &mut out).is_err());
    }

    #[test]
    fn test_iso8859_1_roundtrip() {
        for byte in 0..=255u8 {
            match decode_one(Encoding::Iso8859_1, &[byte]) {
                DecodeResult::Codepoint { cp, .. } => {
                    let mut out = Vec::new();
                    assert!(encode_one(Encoding::Iso8859_1, cp, &mut out).is_ok());
                    assert_eq!(out, vec![byte]);
                }
                _ => panic!("ISO-8859-1 should decode every byte"),
            }
        }
    }

    // -----------------------------------------------------------------------
    // ISO-8859-15
    // -----------------------------------------------------------------------

    #[test]
    fn test_iso8859_15_euro() {
        // 0xA4 in ISO-8859-15 = Euro sign U+20AC
        assert_eq!(iso8859_15_to_unicode(0xA4), 0x20AC);
        assert_eq!(unicode_to_iso8859_15(0x20AC), Some(0xA4));
    }

    #[test]
    fn test_iso8859_15_scaron() {
        assert_eq!(iso8859_15_to_unicode(0xA6), 0x0160); // S-caron
        assert_eq!(iso8859_15_to_unicode(0xA8), 0x0161); // s-caron
        assert_eq!(unicode_to_iso8859_15(0x0160), Some(0xA6));
        assert_eq!(unicode_to_iso8859_15(0x0161), Some(0xA8));
    }

    #[test]
    fn test_iso8859_15_zcaron() {
        assert_eq!(iso8859_15_to_unicode(0xB4), 0x017D);
        assert_eq!(iso8859_15_to_unicode(0xB8), 0x017E);
        assert_eq!(unicode_to_iso8859_15(0x017D), Some(0xB4));
        assert_eq!(unicode_to_iso8859_15(0x017E), Some(0xB8));
    }

    #[test]
    fn test_iso8859_15_oe() {
        assert_eq!(iso8859_15_to_unicode(0xBC), 0x0152);
        assert_eq!(iso8859_15_to_unicode(0xBD), 0x0153);
        assert_eq!(unicode_to_iso8859_15(0x0152), Some(0xBC));
        assert_eq!(unicode_to_iso8859_15(0x0153), Some(0xBD));
    }

    #[test]
    fn test_iso8859_15_y_diaeresis() {
        assert_eq!(iso8859_15_to_unicode(0xBE), 0x0178);
        assert_eq!(unicode_to_iso8859_15(0x0178), Some(0xBE));
    }

    #[test]
    fn test_iso8859_15_unchanged_positions() {
        // A position that is the same as Latin-1 (e.g. 0x41 = 'A')
        assert_eq!(iso8859_15_to_unicode(0x41), 0x41);
        assert_eq!(unicode_to_iso8859_15(0x41), Some(0x41));
    }

    #[test]
    fn test_iso8859_15_replaced_latin1_codepoints_unmappable() {
        // U+00A4 (currency sign) is NOT representable in ISO-8859-15 because
        // byte 0xA4 was reassigned to Euro sign.
        assert_eq!(unicode_to_iso8859_15(0x00A4), None);
    }

    // -----------------------------------------------------------------------
    // Windows-1252
    // -----------------------------------------------------------------------

    #[test]
    fn test_win1252_euro() {
        assert_eq!(windows1252_to_unicode(0x80), 0x20AC);
        assert_eq!(unicode_to_windows1252(0x20AC), Some(0x80));
    }

    #[test]
    fn test_win1252_smart_quotes() {
        assert_eq!(windows1252_to_unicode(0x93), 0x201C); // left double quote
        assert_eq!(windows1252_to_unicode(0x94), 0x201D); // right double quote
        assert_eq!(unicode_to_windows1252(0x201C), Some(0x93));
        assert_eq!(unicode_to_windows1252(0x201D), Some(0x94));
    }

    #[test]
    fn test_win1252_em_dash() {
        assert_eq!(windows1252_to_unicode(0x97), 0x2014);
        assert_eq!(unicode_to_windows1252(0x2014), Some(0x97));
    }

    #[test]
    fn test_win1252_trademark() {
        assert_eq!(windows1252_to_unicode(0x99), 0x2122);
        assert_eq!(unicode_to_windows1252(0x2122), Some(0x99));
    }

    #[test]
    fn test_win1252_identity_ranges() {
        // 0x00-0x7F and 0xA0-0xFF are identity.
        for b in 0..0x80u8 {
            assert_eq!(windows1252_to_unicode(b), u32::from(b));
        }
        for b in 0xA0..=0xFFu8 {
            assert_eq!(windows1252_to_unicode(b), u32::from(b));
        }
    }

    #[test]
    fn test_win1252_roundtrip_mapped_range() {
        for b in 0x80..0xA0u8 {
            let cp = windows1252_to_unicode(b);
            assert_eq!(unicode_to_windows1252(cp), Some(b));
        }
    }

    // -----------------------------------------------------------------------
    // KOI8-R
    // -----------------------------------------------------------------------

    #[test]
    fn test_koi8r_ascii_range() {
        for b in 0..0x80u8 {
            assert_eq!(koi8r_to_unicode(b), u32::from(b));
        }
    }

    #[test]
    fn test_koi8r_cyrillic_a() {
        // KOI8-R 0xC1 = U+0430 (Cyrillic small a)
        assert_eq!(koi8r_to_unicode(0xC1), 0x0430);
        assert_eq!(unicode_to_koi8r(0x0430), Some(0xC1));
    }

    #[test]
    fn test_koi8r_cyrillic_capital_a() {
        // KOI8-R 0xE1 = U+0410 (Cyrillic capital A)
        assert_eq!(koi8r_to_unicode(0xE1), 0x0410);
        assert_eq!(unicode_to_koi8r(0x0410), Some(0xE1));
    }

    #[test]
    fn test_koi8r_yo() {
        // KOI8-R 0xA3 = U+0451 (Cyrillic small yo)
        assert_eq!(koi8r_to_unicode(0xA3), 0x0451);
        assert_eq!(unicode_to_koi8r(0x0451), Some(0xA3));
        // KOI8-R 0xB3 = U+0401 (Cyrillic capital Yo)
        assert_eq!(koi8r_to_unicode(0xB3), 0x0401);
        assert_eq!(unicode_to_koi8r(0x0401), Some(0xB3));
    }

    #[test]
    fn test_koi8r_unmappable() {
        // Japanese hiragana should not map to KOI8-R.
        assert_eq!(unicode_to_koi8r(0x3042), None);
    }

    #[test]
    fn test_koi8r_roundtrip() {
        for b in 0x80..=0xFFu8 {
            let cp = koi8r_to_unicode(b);
            assert_eq!(unicode_to_koi8r(cp), Some(b));
        }
    }

    // -----------------------------------------------------------------------
    // BOM detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_bom_utf16le() {
        let data = [0xFF, 0xFE, 0x41, 0x00];
        let (enc, skip) = detect_bom(Encoding::Utf16Le, &data);
        assert_eq!(enc, Encoding::Utf16Le);
        assert_eq!(skip, 2);
    }

    #[test]
    fn test_bom_utf16be() {
        let data = [0xFE, 0xFF, 0x00, 0x41];
        let (enc, skip) = detect_bom(Encoding::Utf16Be, &data);
        assert_eq!(enc, Encoding::Utf16Be);
        assert_eq!(skip, 2);
    }

    #[test]
    fn test_bom_utf16le_detects_be() {
        // User says UTF-16LE but data has BE BOM -> switch to BE
        let data = [0xFE, 0xFF, 0x00, 0x41];
        let (enc, skip) = detect_bom(Encoding::Utf16Le, &data);
        assert_eq!(enc, Encoding::Utf16Be);
        assert_eq!(skip, 2);
    }

    #[test]
    fn test_bom_utf8() {
        let data = [0xEF, 0xBB, 0xBF, 0x41];
        let (enc, skip) = detect_bom(Encoding::Utf8, &data);
        assert_eq!(enc, Encoding::Utf8);
        assert_eq!(skip, 3);
    }

    #[test]
    fn test_bom_none() {
        let data = [0x41, 0x42, 0x43];
        let (enc, skip) = detect_bom(Encoding::Utf8, &data);
        assert_eq!(enc, Encoding::Utf8);
        assert_eq!(skip, 0);
    }

    #[test]
    fn test_bom_utf32le() {
        let data = [0xFF, 0xFE, 0x00, 0x00, 0x41, 0x00, 0x00, 0x00];
        let (enc, skip) = detect_bom(Encoding::Utf32Le, &data);
        assert_eq!(enc, Encoding::Utf32Le);
        assert_eq!(skip, 4);
    }

    #[test]
    fn test_bom_utf32be() {
        let data = [0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, 0x41];
        let (enc, skip) = detect_bom(Encoding::Utf32Be, &data);
        assert_eq!(enc, Encoding::Utf32Be);
        assert_eq!(skip, 4);
    }

    // -----------------------------------------------------------------------
    // Substitution formatting
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_subst_hex() {
        assert_eq!(format_subst("\\x%02x", 0xFF), b"\\xff");
        assert_eq!(format_subst("\\x%02x", 0x0A), b"\\x0a");
    }

    #[test]
    fn test_format_subst_upper_hex() {
        assert_eq!(format_subst("U+%04X", 0x20AC), b"U+20AC");
    }

    #[test]
    fn test_format_subst_plain_x() {
        assert_eq!(format_subst("[%x]", 255), b"[ff]");
    }

    #[test]
    fn test_format_subst_escaped_percent() {
        assert_eq!(format_subst("%%", 0), b"%");
    }

    #[test]
    fn test_format_subst_no_specifier() {
        assert_eq!(format_subst("?", 0), b"?");
    }

    // -----------------------------------------------------------------------
    // Cross-encoding conversion (integration via convert_stream)
    // -----------------------------------------------------------------------

    fn convert_bytes(input: &[u8], from: Encoding, to: Encoding) -> (Vec<u8>, ConvStats) {
        let opts = Opts {
            from,
            to,
            output_file: None,
            discard_unmappable: false,
            byte_subst: None,
            unicode_subst: None,
            verbose: false,
            input_files: Vec::new(),
        };
        let mut reader = io::Cursor::new(input);
        let mut output = Vec::new();
        let stats = convert_stream(&mut reader, &mut output, from, to, &opts, true)
            .expect("convert_stream should not fail on in-memory I/O");
        (output, stats)
    }

    fn convert_bytes_with_discard(input: &[u8], from: Encoding, to: Encoding) -> (Vec<u8>, ConvStats) {
        let opts = Opts {
            from,
            to,
            output_file: None,
            discard_unmappable: true,
            byte_subst: None,
            unicode_subst: None,
            verbose: false,
            input_files: Vec::new(),
        };
        let mut reader = io::Cursor::new(input);
        let mut output = Vec::new();
        let stats = convert_stream(&mut reader, &mut output, from, to, &opts, true)
            .expect("convert_stream should not fail on in-memory I/O");
        (output, stats)
    }

    fn convert_bytes_with_subst(
        input: &[u8],
        from: Encoding,
        to: Encoding,
        byte_subst: Option<&str>,
        unicode_subst: Option<&str>,
    ) -> (Vec<u8>, ConvStats) {
        let opts = Opts {
            from,
            to,
            output_file: None,
            discard_unmappable: false,
            byte_subst: byte_subst.map(String::from),
            unicode_subst: unicode_subst.map(String::from),
            verbose: false,
            input_files: Vec::new(),
        };
        let mut reader = io::Cursor::new(input);
        let mut output = Vec::new();
        let stats = convert_stream(&mut reader, &mut output, from, to, &opts, true)
            .expect("convert_stream should not fail on in-memory I/O");
        (output, stats)
    }

    #[test]
    fn test_latin1_to_utf8() {
        // "cafe\xE9" -> "cafe" + U+00E9 in UTF-8
        let input = b"caf\xE9";
        let (output, stats) = convert_bytes(input, Encoding::Iso8859_1, Encoding::Utf8);
        assert_eq!(output, "caf\u{00E9}".as_bytes());
        assert_eq!(stats.codepoints, 4);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_utf8_to_latin1() {
        let input = "caf\u{00E9}".as_bytes();
        let (output, stats) = convert_bytes(input, Encoding::Utf8, Encoding::Iso8859_1);
        assert_eq!(output, b"caf\xE9");
        assert_eq!(stats.codepoints, 4);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_utf8_to_utf16le() {
        let input = "A".as_bytes();
        let (output, _) = convert_bytes(input, Encoding::Utf8, Encoding::Utf16Le);
        assert_eq!(output, vec![0x41, 0x00]);
    }

    #[test]
    fn test_utf16le_to_utf8() {
        let input = [0x41, 0x00, 0xE9, 0x00]; // "A" + U+00E9
        let (output, _) = convert_bytes(&input, Encoding::Utf16Le, Encoding::Utf8);
        assert_eq!(output, "A\u{00E9}".as_bytes());
    }

    #[test]
    fn test_latin1_to_utf8_to_utf16_chain() {
        // Latin-1 -> UTF-8 -> UTF-16LE round-trip
        let latin1_input = b"\xE9"; // e-acute
        let (utf8, _) = convert_bytes(latin1_input, Encoding::Iso8859_1, Encoding::Utf8);
        let (utf16, _) = convert_bytes(&utf8, Encoding::Utf8, Encoding::Utf16Le);
        assert_eq!(utf16, vec![0xE9, 0x00]);
    }

    #[test]
    fn test_invalid_utf8_reports_error() {
        let input = [0xFF, 0xFE]; // invalid UTF-8 lead bytes
        let (_, stats) = convert_bytes(&input, Encoding::Utf8, Encoding::Utf8);
        assert!(stats.errors > 0);
    }

    #[test]
    fn test_invalid_utf8_discard() {
        // Mixed valid/invalid UTF-8: "A" + invalid + "B"
        let input = [0x41, 0xFF, 0x42];
        let (output, stats) = convert_bytes_with_discard(&input, Encoding::Utf8, Encoding::Utf8);
        assert_eq!(output, b"AB");
        assert_eq!(stats.errors, 0); // no errors when discarding
    }

    #[test]
    fn test_unmappable_unicode_to_ascii() {
        // U+00E9 (e-acute) cannot be encoded in ASCII.
        let input = "caf\u{00E9}".as_bytes();
        let (_, stats) = convert_bytes(input, Encoding::Utf8, Encoding::Ascii);
        assert!(stats.errors > 0);
    }

    #[test]
    fn test_unmappable_discard() {
        let input = "caf\u{00E9}".as_bytes();
        let (output, _) = convert_bytes_with_discard(input, Encoding::Utf8, Encoding::Ascii);
        assert_eq!(output, b"caf");
    }

    #[test]
    fn test_byte_subst() {
        let input = [0xFF]; // invalid UTF-8
        let (output, _) = convert_bytes_with_subst(
            &input,
            Encoding::Utf8,
            Encoding::Utf8,
            Some("\\x%02x"),
            None,
        );
        assert_eq!(output, b"\\xff");
    }

    #[test]
    fn test_unicode_subst() {
        // U+20AC (Euro) is unmappable to ASCII.  Use unicode-subst.
        let input = "\u{20AC}".as_bytes(); // Euro sign in UTF-8
        let (output, _) = convert_bytes_with_subst(
            input,
            Encoding::Utf8,
            Encoding::Ascii,
            None,
            Some("U+%04X"),
        );
        assert_eq!(output, b"U+20AC");
    }

    #[test]
    fn test_utf8_passthrough() {
        let input = "Hello, world!".as_bytes();
        let (output, stats) = convert_bytes(input, Encoding::Utf8, Encoding::Utf8);
        assert_eq!(output, input);
        assert_eq!(stats.codepoints, 13);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_utf8_multibyte_passthrough() {
        let input = "\u{1F600}\u{20AC}\u{00E9}".as_bytes();
        let (output, stats) = convert_bytes(input, Encoding::Utf8, Encoding::Utf8);
        assert_eq!(output, input);
        assert_eq!(stats.codepoints, 3);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_bom_utf16le_conversion() {
        // BOM + "A" in UTF-16LE
        let input = [0xFF, 0xFE, 0x41, 0x00];
        let (output, stats) = convert_bytes(&input, Encoding::Utf16Le, Encoding::Utf8);
        assert_eq!(output, b"A");
        assert_eq!(stats.codepoints, 1);
        assert_eq!(stats.errors, 0);
    }

    // -----------------------------------------------------------------------
    // Streaming: split multi-byte sequences across chunks
    // -----------------------------------------------------------------------

    /// A reader that yields data in tiny chunks to test boundary handling.
    struct ChunkedReader<'a> {
        data: &'a [u8],
        chunk_size: usize,
        pos: usize,
    }

    impl<'a> ChunkedReader<'a> {
        fn new(data: &'a [u8], chunk_size: usize) -> Self {
            Self { data, chunk_size, pos: 0 }
        }
    }

    impl<'a> Read for ChunkedReader<'a> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.data.len() {
                return Ok(0);
            }
            let end = (self.pos + self.chunk_size).min(self.data.len()).min(self.pos + buf.len());
            let n = end - self.pos;
            buf[..n].copy_from_slice(&self.data[self.pos..end]);
            self.pos += n;
            Ok(n)
        }
    }

    #[test]
    fn test_streaming_utf8_split_2byte() {
        // "caf\xC3\xA9" with chunk_size=4 splits the 2-byte e-acute.
        let input = b"caf\xC3\xA9";
        let opts = Opts {
            from: Encoding::Utf8,
            to: Encoding::Utf8,
            output_file: None,
            discard_unmappable: false,
            byte_subst: None,
            unicode_subst: None,
            verbose: false,
            input_files: Vec::new(),
        };
        let mut reader = ChunkedReader::new(input, 4);
        let mut output = Vec::new();
        let stats = convert_stream(&mut reader, &mut output, Encoding::Utf8, Encoding::Utf8, &opts, true)
            .expect("should succeed");
        assert_eq!(output, input.to_vec());
        assert_eq!(stats.codepoints, 4);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_streaming_utf16_split_pair() {
        // U+1F600 as UTF-16BE: D8 3D DE 00 -- split at 2 bytes.
        let input = [0xD8, 0x3D, 0xDE, 0x00];
        let opts = Opts {
            from: Encoding::Utf16Be,
            to: Encoding::Utf32Be,
            output_file: None,
            discard_unmappable: false,
            byte_subst: None,
            unicode_subst: None,
            verbose: false,
            input_files: Vec::new(),
        };
        let mut reader = ChunkedReader::new(&input, 2);
        let mut output = Vec::new();
        let stats = convert_stream(&mut reader, &mut output, Encoding::Utf16Be, Encoding::Utf32Be, &opts, true)
            .expect("should succeed");
        assert_eq!(output, vec![0x00, 0x01, 0xF6, 0x00]);
        assert_eq!(stats.codepoints, 1);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_streaming_utf8_1byte_chunks() {
        // Force every byte to arrive separately.
        let input = "\u{20AC}\u{00E9}A".as_bytes(); // 3-byte, 2-byte, 1-byte
        let opts = Opts {
            from: Encoding::Utf8,
            to: Encoding::Utf8,
            output_file: None,
            discard_unmappable: false,
            byte_subst: None,
            unicode_subst: None,
            verbose: false,
            input_files: Vec::new(),
        };
        let mut reader = ChunkedReader::new(input, 1);
        let mut output = Vec::new();
        let stats = convert_stream(&mut reader, &mut output, Encoding::Utf8, Encoding::Utf8, &opts, true)
            .expect("should succeed");
        assert_eq!(output, input.to_vec());
        assert_eq!(stats.codepoints, 3);
    }

    #[test]
    fn test_empty_input() {
        let (output, stats) = convert_bytes(b"", Encoding::Utf8, Encoding::Utf8);
        assert!(output.is_empty());
        assert_eq!(stats.codepoints, 0);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_win1252_to_utf8_smart_quotes() {
        // Windows-1252 0x93 0x94 -> left/right double quotes
        let input = [0x93, 0x94];
        let (output, stats) = convert_bytes(&input, Encoding::Windows1252, Encoding::Utf8);
        let expected = "\u{201C}\u{201D}";
        assert_eq!(output, expected.as_bytes());
        assert_eq!(stats.codepoints, 2);
    }

    #[test]
    fn test_koi8r_to_utf8() {
        // KOI8-R 0xC1 = U+0430 (Cyrillic small a)
        let input = [0xC1];
        let (output, _) = convert_bytes(&input, Encoding::Koi8R, Encoding::Utf8);
        assert_eq!(output, "\u{0430}".as_bytes());
    }
}
