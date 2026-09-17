//! Writing a filename into a file that has to stay human-readable.
//!
//! A path on this OS is bytes: `design.txt` allows every byte in a name except
//! `/` and NUL, so a name need not be text. A record whose purpose is to name a
//! file well enough to find it again therefore cannot store it with `Display`
//! or `to_string_lossy` -- those substitute `U+FFFD`, and when the record is
//! the only copy, "restore" recreates the file under a different name and
//! reports success.
//!
//! `design-decisions.md` §426 settled how to do it instead: percent-encode `%`
//! and every byte outside printable ASCII, behind a format version marker.
//! Ordinary paths are unchanged (`/home/u/notes.txt` encodes to itself), the
//! record stays printable, single-line and valid JSON, and the rare path pays
//! with a form that is still readable (`caf%E9.txt`).
//!
//! # Why this is a crate
//!
//! §426 chose one escape *specifically* so that two formats would not drift
//! apart, rejecting per-format escaping because "each format gets a different
//! escape with different edge cases, and 'what the container forbids' is
//! exactly the kind of thing that is revisited later and gets it wrong".
//!
//! It was then implemented twice -- the same four functions, byte for byte, in
//! `apps/explorer` and `apps/backup`, differing across forty-four lines only in
//! one doc comment. Two copies of one decision is the risk the decision was
//! taken to remove, so they now share this.
//!
//! It lives under `apps/` and depends on nothing, because `apps/backup` is a
//! command-line program that must not link a widget library -- which rules out
//! `guitk`, where the rest of this lane's shared code lives. `apps/safeio` is
//! the precedent for a dependency-free library crate in this position.

#![deny(clippy::all, clippy::pedantic)]

use std::path::{Path, PathBuf};

/// Escape a path into a single line of printable ASCII, losslessly.
///
/// Paths on this OS may contain any byte except `/` and NUL, so they are not
/// necessarily UTF-8 and cannot be written with `Display` -- that substitutes
/// U+FFFD and the original bytes are gone. `OsStr::as_encoded_bytes` gives the
/// exact bytes back; everything outside printable ASCII, plus `%` itself, is
/// percent-encoded so the record stays line-oriented text.
#[must_use]
pub fn encode_path(path: &Path) -> String {
    encode_bytes(path.as_os_str().as_encoded_bytes())
}

/// The lossless core of [`encode_path`], on bytes rather than a path.
///
/// Kept separate because this -- not the `OsStr` conversion around it -- is
/// where the round-trip property lives, and it can be tested on any host.
#[must_use]
pub fn encode_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b == b'%' || !(0x20..0x7f).contains(&b) {
            // Digits pushed directly rather than through `format!`: this runs
            // once per byte of every path in a manifest, and the lint that
            // objects to `format!` into a `String` is right that allocating a
            // three-character `String` per byte to throw it away is waste.
            out.push('%');
            out.push(hex_digit(b >> 4));
            out.push(hex_digit(b & 0x0F));
        } else {
            out.push(b as char); // guarded: printable ASCII only
        }
    }
    out
}

/// One upper-case hex digit for a nibble.
///
/// `saturating_add` rather than `+` only because the workspace denies
/// unchecked arithmetic; a nibble is 0..=15 by construction at both call
/// sites, so neither branch can saturate.
const fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => b'0'.saturating_add(nibble) as char,
        _ => b'A'.saturating_add(nibble.saturating_sub(10)) as char,
    }
}

/// Reverse of [`encode_path`].
#[must_use]
pub fn decode_path(encoded: &str) -> PathBuf {
    PathBuf::from(os_string_from_bytes(decode_bytes(encoded)))
}

/// Reverse of [`encode_bytes`].
///
/// A `%` not followed by two hex digits is passed through literally rather than
/// dropped: these files may have been hand-edited, and losing a byte silently
/// is worse than keeping one that was never an escape.
#[must_use]
pub fn decode_bytes(encoded: &str) -> Vec<u8> {
    let bytes = encoded.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%'
            && let Some(hex) = encoded.get(i.saturating_add(1)..i.saturating_add(3))
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i = i.saturating_add(3);
            continue;
        }
        out.push(b);
        i = i.saturating_add(1);
    }
    out
}

/// Build an `OsString` from the raw bytes of a path.
///
/// This is where the byte world meets the platform's path type, so it is split
/// per platform rather than papered over with
/// `OsStr::from_encoded_bytes_unchecked`: that function's contract is that the
/// bytes are valid for the platform's `OsStr` encoding, which is true for
/// arbitrary bytes on Unix but *not* on Windows, where `OsStr` is WTF-8. Since
/// our target is `target-family = ["unix"]`, the safe, total conversion below
/// is the one that actually runs; Windows appears only as a test host.
#[cfg(unix)]
#[must_use]
pub fn os_string_from_bytes(bytes: Vec<u8>) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(bytes)
}

/// The host-only half of [`os_string_from_bytes`].
///
/// Windows `OsString` is WTF-8 and cannot hold arbitrary bytes, so a name that
/// is not valid UTF-8 cannot round-trip *on the development host*. It round
/// trips on the target, and [`decode_bytes`] round-trips at the byte level on
/// every host -- which is the level these records are written at.
#[cfg(not(unix))]
#[must_use]
pub fn os_string_from_bytes(bytes: Vec<u8>) -> std::ffi::OsString {
    match String::from_utf8(bytes) {
        Ok(s) => std::ffi::OsString::from(s),
        // Reachable only on a non-Unix host reading a record written on the
        // target. Nothing better is representable; `decode_bytes` is the API
        // to use if the exact bytes are needed.
        Err(e) => std::ffi::OsString::from(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that indexes out of range should fail loudly at the line that did it"
)]
mod tests {
    use super::*;

    /// An ordinary path encodes to itself, which is the whole point of the
    /// choice §426 made over base64.
    #[test]
    fn an_ordinary_path_is_unchanged() {
        assert_eq!(encode_bytes(b"/home/u/notes.txt"), "/home/u/notes.txt");
    }

    /// A `%` in a real name is escaped, or decoding would invent a byte.
    #[test]
    fn a_literal_percent_is_escaped() {
        assert_eq!(encode_bytes(b"50%.txt"), "50%25.txt");
        assert_eq!(decode_bytes("50%25.txt"), b"50%.txt");
    }

    /// Bytes that are not text survive exactly.
    #[test]
    fn arbitrary_bytes_round_trip() {
        let raw: &[u8] = &[0x2F, 0x61, 0xFF, 0x00, 0x0A, 0x7F, 0x80, 0x62];
        let encoded = encode_bytes(raw);
        assert!(
            encoded.is_ascii(),
            "the encoded form must be printable ASCII: {encoded}"
        );
        assert!(!encoded.contains('\n'), "a record line must stay one line");
        assert_eq!(decode_bytes(&encoded), raw);
    }

    /// Every byte value round-trips, not just the interesting ones.
    #[test]
    fn every_byte_round_trips() {
        let all: Vec<u8> = (0u8..=255).collect();
        assert_eq!(decode_bytes(&encode_bytes(&all)), all);
    }

    /// A stray `%` is kept rather than swallowed.
    ///
    /// These files can be hand-edited, and losing a byte silently is worse
    /// than keeping one that was never an escape.
    #[test]
    fn a_stray_percent_is_passed_through() {
        assert_eq!(decode_bytes("100% done"), b"100% done");
        assert_eq!(decode_bytes("ends with %"), b"ends with %");
        assert_eq!(decode_bytes("%zz"), b"%zz");
    }

    /// Hex is accepted in either case, since a human may have typed it.
    #[test]
    fn lower_case_hex_decodes_too() {
        assert_eq!(decode_bytes("caf%e9.txt"), decode_bytes("caf%E9.txt"));
    }

    /// The empty string is the empty path, not an error.
    #[test]
    fn empty_round_trips() {
        assert_eq!(encode_bytes(b""), "");
        assert_eq!(decode_bytes(""), b"");
    }

    /// A path round-trips through the `Path` API on this host.
    #[test]
    fn a_path_round_trips() {
        let p = Path::new("/home/u/a b/c.txt");
        assert_eq!(decode_path(&encode_path(p)), p);
    }
}
