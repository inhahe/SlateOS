//! Fixtures for the programs that show EXIF: a camera's EXIF as a TIFF
//! structure, and a JPEG carrying it.
//!
//! Behind the `testing` feature, which a dependent turns on in its own
//! `[dev-dependencies]`: `#[cfg(test)]` is set only when *this* crate is under
//! test, and the image viewer's and the file manager's tests need the same
//! photograph's EXIF that this crate's own tests read.
//!
//! A fixture that cannot be built is a broken test, so these panic on one
//! rather than return a result for a test to unwrap.
#![allow(
    clippy::panic,
    reason = "test fixtures: a fixture that cannot be built is a broken test"
)]

/// A camera's EXIF as a little-endian TIFF structure: made by a `Canon`,
/// the `Canon EOS R5`, at ISO 400 and f/2.8, taken `2025:06:15 14:30:22`,
/// orientation 6 (turned right).
#[must_use]
pub fn camera_tiff() -> Vec<u8> {
    let mut t: Vec<u8> = b"II".to_vec();
    t.extend_from_slice(&42u16.to_le_bytes());
    t.extend_from_slice(&8u32.to_le_bytes());
    // IFD0 at 8: four entries (54 bytes), so the Exif directory is at 62; it
    // has three (42 bytes), so the values that do not fit in an entry start
    // at 104.
    let (make_at, model_at, fnum_at, date_at) = (104u32, 110u32, 124u32, 132u32);
    let entry = |t: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32| {
        t.extend_from_slice(&tag.to_le_bytes());
        t.extend_from_slice(&kind.to_le_bytes());
        t.extend_from_slice(&count.to_le_bytes());
        t.extend_from_slice(&value.to_le_bytes());
    };
    t.extend_from_slice(&4u16.to_le_bytes());
    entry(&mut t, 0x010F, 2, 6, make_at);
    entry(&mut t, 0x0110, 2, 13, model_at);
    entry(&mut t, 0x0112, 3, 1, 6);
    entry(&mut t, 0x8769, 4, 1, 62);
    t.extend_from_slice(&0u32.to_le_bytes());
    t.extend_from_slice(&3u16.to_le_bytes());
    entry(&mut t, 0x8827, 3, 1, 400);
    entry(&mut t, 0x829D, 5, 1, fnum_at);
    entry(&mut t, 0x9003, 2, 20, date_at);
    t.extend_from_slice(&0u32.to_le_bytes());
    t.extend_from_slice(b"Canon\0");
    t.extend_from_slice(b"Canon EOS R5\0\0");
    t.extend_from_slice(&28u32.to_le_bytes());
    t.extend_from_slice(&10u32.to_le_bytes());
    t.extend_from_slice(b"2025:06:15 14:30:22\0");
    t
}

/// `jpeg` with an `APP1` segment holding `tiff` as its EXIF, straight after
/// its start-of-image marker -- where cameras put it.
///
/// # Panics
///
/// When `jpeg` does not begin with a start-of-image marker, or `tiff` is too
/// large for one segment: a fixture that cannot be built is a broken test.
#[must_use]
pub fn with_exif(jpeg: &[u8], tiff: &[u8]) -> Vec<u8> {
    assert!(jpeg.starts_with(&[0xFF, 0xD8]), "not a JPEG");
    let length = tiff
        .len()
        .checked_add(2 + 6)
        .and_then(|n| u16::try_from(n).ok());
    let Some(length) = length else {
        panic!("EXIF too large for one segment");
    };
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(tiff);
    out.extend_from_slice(jpeg.get(2..).unwrap_or_default());
    out
}
