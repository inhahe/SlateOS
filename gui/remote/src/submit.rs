//! Addressed draw submission — `SURF`, the frame a client sends to say *which*
//! window a picture is for.
//!
//! An `ORDR` frame is a list of draw commands and nothing else. That was
//! sufficient while a client meant one window on one connection, where the
//! addressee was implied by which socket the bytes arrived on. It stops being
//! sufficient the moment a client owns two windows — an app with a dialog open,
//! which is most apps — because both windows' frames now travel the same
//! connection and the compositor has no way to tell them apart.
//!
//! ## Why a wrapper rather than a window id inside `ORDR`
//!
//! Adding the id to the `ORDR` header would be fewer bytes and is the wrong
//! shape. `ORDR` is also nested *inside* [`SceneFrame`](crate::scene::SceneFrame)
//! — once per window, under a `SceneWindow` that already names the window. An id
//! in the header would be duplicated there, and two fields that must agree
//! eventually disagree; a decoder would then have to pick a winner, and whichever
//! it picked would be wrong for someone.
//!
//! So the layering is: `ORDR` says *what to draw*, and whoever carries it says
//! *who it is for* — `SceneWindow` in the compositor's direction, `SURF` in the
//! client's. Neither repeats the other.
//!
//! ## Wire format
//!
//! ```text
//! magic   : [u8;4] = b"SURF"
//! version : u8     = SUBMIT_VERSION
//! flags   : u8     = 0 (reserved)
//! window  : u64                        addressee window id
//! commands: <a complete ORDR frame>
//! ```
//!
//! The nested frame is the byte-for-byte output of
//! [`encode_frame`](crate::encode_frame), decoded by the same
//! [`decode_frame`](crate::decode_frame) — the command codec is not
//! reimplemented here, and a change to it needs no change to this module.

use guitk::render::RenderTree;

use crate::{DecodeError, Reader, write_u64};

/// Submit-frame magic: `b"SURF"`.
pub const SUBMIT_MAGIC: [u8; 4] = *b"SURF";

/// Submit protocol version. Bump on any incompatible layout change; never reuse
/// a number.
pub const SUBMIT_VERSION: u8 = 1;

/// Submit-frame header: magic + version + flags + window id.
const SUBMIT_HEADER_LEN: usize = 4 + 1 + 1 + 8;

/// One window's picture, addressed.
///
/// Not `PartialEq`: [`RenderTree`] is not, because its commands hold floats.
#[derive(Clone, Debug)]
pub struct Submission {
    /// The window this picture belongs to.
    pub window: u64,
    /// What to draw in that window's client area.
    pub commands: RenderTree,
}

/// Append an addressed draw submission to `out`.
pub fn encode_submit_into(window: u64, tree: &RenderTree, out: &mut Vec<u8>) {
    out.extend_from_slice(&SUBMIT_MAGIC);
    out.push(SUBMIT_VERSION);
    out.push(0); // flags
    write_u64(out, window);
    crate::encode_frame(tree, out);
}

/// Encode an addressed draw submission into a fresh buffer.
#[must_use]
pub fn encode_submit(window: u64, tree: &RenderTree) -> Vec<u8> {
    let mut out = Vec::new();
    encode_submit_into(window, tree, &mut out);
    out
}

/// Decode one submission, returning it and the bytes it consumed.
///
/// # Errors
///
/// [`DecodeError::BadMagic`] if the frame is not `SURF`,
/// [`DecodeError::UnsupportedVersion`] for a version this build does not know,
/// [`DecodeError::ReservedFlags`] for flag bits that are not yet defined, and
/// whatever [`decode_frame`](crate::decode_frame) reports for the nested
/// commands.
pub fn decode_submit(input: &[u8]) -> Result<(Submission, usize), DecodeError> {
    let mut r = Reader::new(input);
    r.need(SUBMIT_HEADER_LEN)?;
    r.expect_magic(SUBMIT_MAGIC)?;
    let ver = r.read_u8()?;
    if ver != SUBMIT_VERSION {
        return Err(DecodeError::UnsupportedVersion(ver));
    }
    let flags = r.read_u8()?;
    if flags != 0 {
        return Err(DecodeError::ReservedFlags(flags));
    }
    let window = r.read_u64()?;

    let (commands, used) = crate::decode_frame(r.rest())?;
    r.advance(used)?;

    Ok((Submission { window, commands }, r.position()))
}

/// Streaming form of [`decode_submit`]: `Ok(None)` when the buffer holds only
/// part of a frame.
///
/// # Errors
///
/// As [`decode_submit`], except that a short buffer is `Ok(None)` rather than
/// [`DecodeError::UnexpectedEof`].
pub fn try_decode_submit(input: &[u8]) -> Result<Option<(Submission, usize)>, DecodeError> {
    match decode_submit(input) {
        Ok(v) => Ok(Some(v)),
        Err(DecodeError::UnexpectedEof) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use guitk::color::Color;

    use super::*;

    fn tree(w: f32) -> RenderTree {
        let mut t = RenderTree::new();
        t.fill_rect(0.0, 0.0, w, 10.0, Color::from_hex(0x44_55_66));
        t
    }

    #[test]
    fn a_submission_survives_the_wire() {
        let bytes = encode_submit(0xDEAD_BEEF_0000_0001, &tree(64.0));
        let (back, used) = decode_submit(&bytes).expect("decodes");
        assert_eq!(used, bytes.len());
        assert_eq!(back.window, 0xDEAD_BEEF_0000_0001);
        assert_eq!(back.commands.commands.len(), 1);
    }

    #[test]
    fn two_windows_frames_do_not_get_confused_on_one_connection() {
        // The whole reason this frame exists: an app with a dialog open sends
        // both windows' pictures down one connection.
        let mut buf = encode_submit(1, &tree(10.0));
        buf.extend_from_slice(&encode_submit(2, &tree(20.0)));
        buf.extend_from_slice(&encode_submit(1, &tree(30.0)));

        let mut at = 0usize;
        let mut seen = Vec::new();
        while at < buf.len() {
            let (s, used) = decode_submit(&buf[at..]).expect("decodes");
            seen.push(s.window);
            at += used;
        }
        assert_eq!(seen, vec![1, 2, 1]);
        assert_eq!(at, buf.len(), "no bytes left over");
    }

    #[test]
    fn an_empty_picture_is_legal() {
        // A window that has gone blank still has to say so; the alternative is
        // that its last non-empty frame stays on screen forever.
        let bytes = encode_submit(9, &RenderTree::new());
        let (back, used) = decode_submit(&bytes).expect("decodes");
        assert_eq!(used, bytes.len());
        assert!(back.commands.commands.is_empty());
    }

    #[test]
    fn a_bare_render_frame_is_not_a_submission() {
        // The two are distinguishable on their first four bytes, so a client
        // that forgets the wrapper fails immediately rather than having its
        // first draw command read as a window id.
        let bare = crate::encode_frame_to_vec(&tree(8.0));
        assert!(matches!(decode_submit(&bare), Err(DecodeError::BadMagic)));
    }

    #[test]
    fn every_truncation_reads_as_incomplete_not_corrupt() {
        let bytes = encode_submit(7, &tree(12.0));
        for n in 0..bytes.len() {
            assert!(
                matches!(try_decode_submit(&bytes[..n]), Ok(None)),
                "a {n}-byte prefix must read as incomplete, not as an error"
            );
        }
        assert!(try_decode_submit(&bytes).expect("decodes").is_some());
    }

    #[test]
    fn a_bad_version_is_rejected_rather_than_guessed() {
        let mut bytes = encode_submit(1, &tree(4.0));
        bytes[4] = 0xFE;
        assert!(matches!(
            decode_submit(&bytes),
            Err(DecodeError::UnsupportedVersion(0xFE))
        ));
    }

    #[test]
    fn reserved_flag_bits_are_rejected() {
        // A future flag will change the layout; a decoder that ignored it would
        // read the new layout as the old one and report plausible nonsense.
        let mut bytes = encode_submit(1, &tree(4.0));
        bytes[5] = 0x01;
        assert!(matches!(
            decode_submit(&bytes),
            Err(DecodeError::ReservedFlags(0x01))
        ));
    }

    #[test]
    fn no_damaged_byte_of_a_submission_ever_panics() {
        let bytes = encode_submit(0x0102_0304_0506_0708, &tree(16.0));
        for i in 0..bytes.len() {
            for mask in [0x01u8, 0x80, 0xFF] {
                let mut bad = bytes.clone();
                bad[i] ^= mask;
                // The result is uninteresting; not unwinding is the assertion.
                let _ = decode_submit(&bad);
            }
        }
    }
}
