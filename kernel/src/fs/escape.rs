//! Octal escaping for byte strings embedded in line-oriented text formats.
//!
//! A path is an uninterpreted byte string — every byte except `/` and NUL is
//! legal (see [`super::path`]).  Several of the kernel's on-disk and
//! `/proc`-visible files are plain text with structural delimiters, so a path
//! written into one verbatim can break the format outright: a newline in a
//! filename splits a record in two, a space corrupts `/proc/mounts`, an `=`
//! shifts a key/value boundary.  Escaping is not cosmetic here; without it a
//! legal filename silently corrupts the file that names it.
//!
//! The encoding is Linux's, so the output is already understood by the tools
//! that read these files.  `fs/proc_namespace.c`'s `mangle()` is
//! `seq_escape(m, s, " \t\n\\")`, which is `string_escape_mem` with
//! `ESCAPE_OCTAL`: a byte is emitted raw only if it is printable ASCII
//! (`0x21..=0x7E`) and is not a backslash; every other byte — space, control
//! characters, DEL, and *all* bytes `>= 0x80` — becomes a three-digit octal
//! escape such as `\040`.
//!
//! Two consequences matter:
//!
//! - **It is total.** Any byte sequence at all can be escaped, including one
//!   that is not valid UTF-8, so callers never have to reject a path.
//! - **It is lossless and unambiguous.** A literal backslash is itself
//!   escaped, so [`unescape_octal`] is a straight inverse and the output is
//!   pure ASCII (which is what lets these files be declared `text`).

use alloc::string::String;
use alloc::vec::Vec;

/// Escape `bytes` so the result is pure printable ASCII with no structural
/// bytes in it.
///
/// `extra` names additional bytes to escape beyond the default set — for a
/// format whose delimiter is otherwise a printable character, e.g. `b"="` for
/// a `key=value` file.  Pass `&[]` for Linux `mangle()` semantics exactly.
#[must_use]
pub fn escape_octal(bytes: &[u8], extra: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b.is_ascii_graphic() && b != b'\\' && !extra.contains(&b) {
            out.push(char::from(b));
        } else {
            out.push('\\');
            // Three octal digits, most significant first. `b` is a `u8` and
            // every operand below is masked to 0..=7, so the adds are in range.
            out.push(char::from(b'0'.wrapping_add((b >> 6) & 0o7)));
            out.push(char::from(b'0'.wrapping_add((b >> 3) & 0o7)));
            out.push(char::from(b'0'.wrapping_add(b & 0o7)));
        }
    }
    out
}

/// Invert [`escape_octal`], recovering the original bytes.
///
/// Returns `None` for input that no call to `escape_octal` could have
/// produced — a trailing backslash, or a backslash not followed by exactly
/// three octal digits.  A malformed record is a corrupt record, and silently
/// salvaging part of it would hand the caller a path that names the wrong
/// file; rejecting is the only safe answer.
///
/// The `extra` set is deliberately not a parameter: escaped output records
/// which bytes were escaped, so decoding needs no knowledge of the format's
/// delimiters.
#[must_use]
pub fn unescape_octal(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0usize;
    while let Some(&b) = s.get(i) {
        if b != b'\\' {
            out.push(b);
            i = i.checked_add(1)?;
            continue;
        }
        // `\NNN`: exactly three octal digits, no more and no fewer.
        let mut value = 0u8;
        for k in 1..=3usize {
            let d = *s.get(i.checked_add(k)?)?;
            if !(b'0'..=b'7').contains(&d) {
                return None;
            }
            // Max is 0o777 = 511, which overflows a u8, so a leading digit
            // above 3 is not representable and must be rejected rather than
            // wrapped into a different byte.
            value = value.checked_mul(8)?.checked_add(d.wrapping_sub(b'0'))?;
        }
        out.push(value);
        i = i.checked_add(4)?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Exercise the escaper. Wired into the boot battery in `main.rs`, because the
/// kernel binary sets `test = false` and so has no host `cargo test` to run
/// these under.
#[allow(clippy::unwrap_used, clippy::panic)]
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[escape] Running self-test...");

    serial_println!("  escape::self_test 1: printable ASCII passes through");
    assert_eq!(
        escape_octal(b"/usr/local-bin_1.2", &[]),
        "/usr/local-bin_1.2"
    );

    serial_println!("  escape::self_test 2: Linux mangle() escape set");
    // The exact four bytes `mangle()` escapes: space, tab, newline, backslash.
    assert_eq!(escape_octal(b" \t\n\\", &[]), "\\040\\011\\012\\134");
    assert_eq!(escape_octal(b"\x00\x7f", &[]), "\\000\\177");

    serial_println!("  escape::self_test 3: total over non-UTF-8 bytes");
    let wild = escape_octal(b"/mnt/re\xffport\xfe", &[]);
    assert_eq!(wild, "/mnt/re\\377port\\376");
    // The whole point of the encoding: the result is safe to put in a file
    // that is declared to be text.
    assert!(wild.is_ascii());

    serial_println!("  escape::self_test 4: extra delimiters");
    assert_eq!(escape_octal(b"a=b", b"="), "a\\075b");
    // Without the extra set, `=` is an ordinary printable byte.
    assert_eq!(escape_octal(b"a=b", &[]), "a=b");

    serial_println!("  escape::self_test 5: round-trip");
    for case in [
        b"".as_slice(),
        b"plain",
        b" \t\n\\",
        b"/mnt/re\xffport\xfe",
        b"\x00\x7f\x80\xff",
        b"a=b=c",
    ] {
        let round = unescape_octal(escape_octal(case, b"=").as_bytes());
        assert_eq!(round.as_deref(), Some(case), "round-trip failed");
    }

    serial_println!("  escape::self_test 6: malformed input is rejected");
    // Trailing backslash, short escape, non-octal digit, and a three-digit
    // value that does not fit in a byte.
    assert_eq!(unescape_octal(b"abc\\"), None);
    assert_eq!(unescape_octal(b"\\04"), None);
    assert_eq!(unescape_octal(b"\\09"), None);
    assert_eq!(unescape_octal(b"\\400"), None);
    // 0o377 is the largest representable escape and must still decode.
    assert_eq!(unescape_octal(b"\\377"), Some(alloc::vec![0xff]));

    serial_println!("[escape] Self-test PASSED");
}
