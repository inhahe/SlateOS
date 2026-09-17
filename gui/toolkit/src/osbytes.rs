//! Cutting `OsStr` paths on ASCII bytes without going through `str`.
//!
//! A SlateOS path is bytes: `design.txt` allows every byte in a name except
//! `/` and NUL, so a path need not be text. Anything that renders one through
//! `str` to split it -- `to_string_lossy`, `to_str().unwrap_or_default()` --
//! replaces whatever is not UTF-8 with `U+FFFD` and then operates on the
//! replacement, which is a *different path that looks right on screen*.
//!
//! Splitting on `/` by hand, rather than through [`std::path::Path`], is also
//! deliberate. `Path` is host-dependent: on the Windows machine these tests
//! run on it treats `\` as a separator and `C:\` as a prefix, so a widget
//! built on it would behave one way under test and another on the target, for
//! a filename the filesystem is specified to accept. `dialog::parent_path`
//! made this argument first; this module is where its proof now lives.
//!
//! # Why the `unsafe` is here and nowhere else
//!
//! `std` offers no *safe* way back from bytes to `&OsStr` on every platform
//! ([`std::os::unix::ffi::OsStrExt::from_bytes`] is Unix-only), so the cut has
//! to go through [`OsStr::from_encoded_bytes_unchecked`]. That is one
//! obligation, and it should be discharged once rather than re-argued at each
//! call site -- which is what two copies of it in this crate had become.

use std::ffi::OsStr;

/// Reinterpret bytes taken from an [`OsStr`] as an [`OsStr`] again.
///
/// # Safety
///
/// `bytes` must be a sub-slice of [`OsStr::as_encoded_bytes`] output whose
/// ends fall on the boundaries of ASCII bytes it was cut at. That function
/// documents exactly this contract: the encoding is a self-synchronising
/// superset of UTF-8, so a 7-bit ASCII byte can never occur *inside* a
/// multi-byte sequence, and a cut at one therefore cannot land in the middle
/// of a character on any platform.
///
/// Transforming bytes in place is permitted only when it preserves that
/// property -- ASCII-lowercasing does, because it maps ASCII to ASCII and
/// leaves every other byte alone.
///
/// Note this only ever *narrows*. Joining encoded runs must go through
/// [`std::ffi::OsString::push`], never a `Vec<u8>` concatenation: splicing two
/// runs together is the one case the contract does not cover, because on
/// Windows a trailing unpaired high surrogate meeting a leading low surrogate
/// has to be recomposed into a single four-byte sequence, and a raw join would
/// silently produce ill-formed WTF-8.
pub(crate) unsafe fn from_bytes(bytes: &[u8]) -> &OsStr {
    // SAFETY: the caller guarantees `bytes` came from `as_encoded_bytes` and
    // was cut only at ASCII boundaries, per the contract above.
    unsafe { OsStr::from_encoded_bytes_unchecked(bytes) }
}

/// The `/`-separated runs of `path`, empty ones included.
///
/// Safe, because it is the one shape of cut whose obligation is discharged by
/// construction: `/` is ASCII, so every piece [`slice::split`] yields already
/// ends on a valid boundary. Callers that want only the non-empty runs should
/// filter -- the empties are what distinguish `/a//b` from `/a/b`, and which
/// of those matters is the caller's business.
pub(crate) fn split_on_slash(path: &OsStr) -> impl Iterator<Item = &OsStr> {
    path.as_encoded_bytes().split(|&b| b == b'/').map(|part| {
        // SAFETY: `part` is a run of `as_encoded_bytes` output delimited by
        // ASCII `/`, which is exactly the contract `from_bytes` states -- and
        // the example `from_encoded_bytes_unchecked`'s own documentation
        // gives, with `/` in place of its ASCII space.
        unsafe { from_bytes(part) }
    })
}
