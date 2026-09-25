//! The compositor asking a client to draw windows whole again (`RPNT`).
//!
//! Sent as part of the desktop's artifact recovery (the Ctrl+Super+R
//! "full redraw", or a shell's [`RecoverDisplay`] request). The compositor
//! redraws everything it holds from scratch; what it cannot redraw is a
//! client's own idea of what it has already drawn. A client that paints only
//! the regions it believes are dirty, and whose belief has gone wrong, keeps a
//! stale patch on screen through any amount of compositor recomposition. So
//! the compositor names the windows, and the client draws each one entire.
//!
//! A frame of its own rather than an input event, for two reasons. A repaint
//! is not something the user did to the window, so it has no place in the
//! event vocabulary every widget matches on; and it is the display asking, as
//! a window list is the display telling, which is the kind of traffic that
//! already has frames of its own ([`window_list`](crate::window_list),
//! [`tray`](crate::tray)).
//!
//! ## Wire format
//!
//! ```text
//! magic(4) = "RPNT" | version(1) | flags(1) = 0 | count(u32 LE)
//! count × window(u64 LE)
//! ```
//!
//! [`RecoverDisplay`]: crate::control::RequestBody::RecoverDisplay

use crate::{DecodeError, Reader, capacity_hint, write_u32, write_u64};

/// Repaint frame magic: `b"RPNT"`.
pub const REPAINT_MAGIC: [u8; 4] = *b"RPNT";

/// The version of the repaint frame this build writes and understands.
pub const REPAINT_VERSION: u8 = 1;

/// Magic, version, flags, and the window count.
pub const REPAINT_HEADER_LEN: usize = 4 + 1 + 1 + 4;

/// The most windows one frame may name.
///
/// A bound for the decoder to refuse before allocating, so a corrupt length
/// cannot ask for a gigabyte. Its own constant rather than the window list's,
/// though the two are the same size today: they bound different frames, and a
/// shared constant would couple limits that are free to diverge.
pub const MAX_REPAINT_WINDOWS: u32 = 1 << 16;

/// Which windows a client should draw whole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Repaint {
    /// The windows, as the compositor's ids for them — the ids the client was
    /// given when it created each one.
    pub windows: Vec<u64>,
}

/// Encode a repaint request as one self-contained `RPNT` frame.
#[must_use]
pub fn encode_repaint(repaint: &Repaint) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity_hint(REPAINT_HEADER_LEN, repaint.windows.len(), 8));
    encode_repaint_into(&mut out, repaint);
    out
}

/// Encode into a caller-provided buffer, appending to whatever it holds.
pub fn encode_repaint_into(out: &mut Vec<u8>, repaint: &Repaint) {
    out.extend_from_slice(&REPAINT_MAGIC);
    out.push(REPAINT_VERSION);
    out.push(0); // flags
    // Saturating rather than panicking, as everywhere else in this crate: the
    // decoder refuses anything past MAX_REPAINT_WINDOWS regardless.
    write_u32(
        out,
        u32::try_from(repaint.windows.len()).unwrap_or(u32::MAX),
    );
    for &window in &repaint.windows {
        write_u64(out, window);
    }
}

/// Decode an `RPNT` frame.
///
/// # Errors
///
/// [`DecodeError::BadMagic`] if the frame is not `RPNT`,
/// [`DecodeError::UnsupportedVersion`] for a version this build does not know,
/// [`DecodeError::ReservedFlags`] for reserved bits set,
/// [`DecodeError::TooManyRepaints`] for a count past [`MAX_REPAINT_WINDOWS`],
/// and [`DecodeError::UnexpectedEof`] for a frame that ends early.
///
/// On success, answers the request and how many bytes it occupied.
pub fn decode_repaint(input: &[u8]) -> Result<(Repaint, usize), DecodeError> {
    let mut r = Reader::new(input);
    if r.take_array::<4>()? != REPAINT_MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = r.read_u8()?;
    if version != REPAINT_VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let count = r.read_u32()?;
    if count > MAX_REPAINT_WINDOWS {
        return Err(DecodeError::TooManyRepaints(count));
    }
    // Grown as the ids are read rather than reserved for the claimed count, so
    // a frame that claims many and delivers few costs what it delivered.
    let mut windows = Vec::new();
    for _ in 0..count {
        windows.push(r.read_u64()?);
    }
    // The position, not the input length: a frame arrives in a stream with
    // whatever follows it.
    Ok((Repaint { windows }, r.position()))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    #[test]
    fn a_repaint_round_trips() {
        for windows in [vec![], vec![7], vec![1, u64::MAX, 42]] {
            let repaint = Repaint { windows };
            let bytes = encode_repaint(&repaint);
            let (back, used) = decode_repaint(&bytes).unwrap();
            assert_eq!(back, repaint);
            assert_eq!(used, bytes.len());
        }
    }

    #[test]
    fn a_frame_followed_by_another_reports_only_its_own_length() {
        let mut bytes = encode_repaint(&Repaint { windows: vec![3] });
        let first = bytes.len();
        bytes.extend_from_slice(b"next frame");
        let (_, used) = decode_repaint(&bytes).unwrap();
        assert_eq!(used, first, "swallowed the start of the next frame");
    }

    #[test]
    fn every_truncation_is_an_early_end_not_a_panic() {
        let bytes = encode_repaint(&Repaint {
            windows: vec![1, 2],
        });
        for cut in 0..bytes.len() {
            assert_eq!(
                decode_repaint(&bytes[..cut]),
                Err(DecodeError::UnexpectedEof),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn a_count_past_the_bound_is_refused_before_anything_is_allocated() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&REPAINT_MAGIC);
        bytes.push(REPAINT_VERSION);
        bytes.push(0);
        write_u32(&mut bytes, MAX_REPAINT_WINDOWS + 1);
        assert_eq!(
            decode_repaint(&bytes),
            Err(DecodeError::TooManyRepaints(MAX_REPAINT_WINDOWS + 1))
        );
    }

    #[test]
    fn a_wrong_magic_version_or_flag_is_refused() {
        let good = encode_repaint(&Repaint { windows: vec![1] });
        let mut magic = good.clone();
        magic[0] = b'X';
        assert_eq!(decode_repaint(&magic), Err(DecodeError::BadMagic));
        let mut version = good.clone();
        version[4] = REPAINT_VERSION + 1;
        assert_eq!(
            decode_repaint(&version),
            Err(DecodeError::UnsupportedVersion(REPAINT_VERSION + 1))
        );
        let mut flags = good;
        flags[5] = 1;
        assert_eq!(decode_repaint(&flags), Err(DecodeError::ReservedFlags(1)));
    }
}
