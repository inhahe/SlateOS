//! Frame demultiplexing — reading a stream that carries all of this crate's
//! frame types at once.
//!
//! Each module here defines a frame with its own magic: `ORDR` for drawing,
//! `SCEN` for a stacked desktop, `INPT` for input, `CREQ`/`CRSP` for window
//! control. Each also has its own decoder, and each of those rejects everything
//! that is not its own kind. That is exactly right for a single-purpose stream
//! and useless for the one a real client has, which carries input *and* control
//! replies interleaved on one connection.
//!
//! One connection is the deliberate choice — the same one Wayland makes. The
//! alternative, a separate transport per window plus one for control, needs the
//! compositor to hand a fresh channel back inside a reply, which is
//! capability-passing this protocol does not have and should not grow just to
//! avoid a four-byte tag it already sends.
//!
//! So this module reads the magic first and dispatches on it. The magics were
//! already distinct — that was argued for on the grounds that a wrong-way frame
//! should fail immediately rather than decode into plausible nonsense — and
//! demultiplexing is that same property used constructively.

use guitk::render::RenderTree;

use crate::control::{
    REQUEST_MAGIC, RESPONSE_MAGIC, Request, Response, decode_requests, decode_responses,
};
use crate::input::{INPUT_MAGIC, InputEvent, decode_input_frame};
use crate::scene::{SCENE_MAGIC, SceneFrame, decode_scene_frame};
use crate::submit::{SUBMIT_MAGIC, Submission, decode_submit};
use crate::{DecodeError, MAGIC, decode_frame};

/// One decoded frame of any kind this crate defines.
///
/// Not `PartialEq`: [`RenderTree`] is not, because its commands hold floats.
#[derive(Clone, Debug)]
pub enum Frame {
    /// Draw commands for one window's client area (`ORDR`).
    Render(RenderTree),
    /// Draw commands with the window they are for (`SURF`).
    Submit(Submission),
    /// A stacked-desktop snapshot or delta (`SCEN`).
    Scene(SceneFrame),
    /// Input events addressed to windows (`INPT`).
    Input(Vec<InputEvent>),
    /// Window-control requests (`CREQ`).
    Requests(Vec<Request>),
    /// Window-control responses (`CRSP`).
    Responses(Vec<Response>),
}

impl Frame {
    /// A short name for the frame kind, for error messages and logs.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Render(_) => "render",
            Self::Submit(_) => "submit",
            Self::Scene(_) => "scene",
            Self::Input(_) => "input",
            Self::Requests(_) => "control request",
            Self::Responses(_) => "control response",
        }
    }
}

/// Decode whichever frame is at the front of `input`, returning it and the
/// bytes it consumed.
///
/// # Errors
///
/// [`DecodeError::BadMagic`] if the leading four bytes are not a magic this
/// crate knows, and whatever the selected decoder reports otherwise.
pub fn decode_any(input: &[u8]) -> Result<(Frame, usize), DecodeError> {
    // The magic is read before anything is decoded, so a stream that has lost
    // sync fails here rather than inside a decoder that would report a
    // confident but wrong error about a field it should never have reached.
    let magic: [u8; 4] = match input.get(..4) {
        Some(bytes) => bytes.try_into().unwrap_or(MAGIC),
        None => return Err(DecodeError::UnexpectedEof),
    };

    match magic {
        MAGIC => {
            let (tree, used) = decode_frame(input)?;
            Ok((Frame::Render(tree), used))
        }
        SUBMIT_MAGIC => {
            let (sub, used) = decode_submit(input)?;
            Ok((Frame::Submit(sub), used))
        }
        SCENE_MAGIC => {
            let (scene, used) = decode_scene_frame(input)?;
            Ok((Frame::Scene(scene), used))
        }
        INPUT_MAGIC => {
            let (events, used) = decode_input_frame(input)?;
            Ok((Frame::Input(events), used))
        }
        REQUEST_MAGIC => {
            let (reqs, used) = decode_requests(input)?;
            Ok((Frame::Requests(reqs), used))
        }
        RESPONSE_MAGIC => {
            let (resps, used) = decode_responses(input)?;
            Ok((Frame::Responses(resps), used))
        }
        _ => Err(DecodeError::BadMagic),
    }
}

/// Streaming form of [`decode_any`]: `Ok(None)` when the buffer holds only part
/// of a frame, so a caller reading from a transport can simply read more.
///
/// # Errors
///
/// As [`decode_any`], except that a short buffer is `Ok(None)` rather than
/// [`DecodeError::UnexpectedEof`].
pub fn try_decode_any(input: &[u8]) -> Result<Option<(Frame, usize)>, DecodeError> {
    match decode_any(input) {
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
    use guitk::event::Event;

    use super::*;
    use crate::control::{RequestBody, ResponseBody};
    use crate::input::encode_input_frame;
    use crate::scene::{SceneFrame, encode_scene_frame};
    use crate::{control, encode_frame_to_vec};

    fn render_bytes() -> Vec<u8> {
        let mut tree = RenderTree::new();
        tree.fill_rect(0.0, 0.0, 10.0, 10.0, Color::from_hex(0x11_22_33));
        encode_frame_to_vec(&tree)
    }

    #[test]
    fn each_frame_kind_is_recognised_by_its_magic() {
        let cases: Vec<(Vec<u8>, &str)> = vec![
            (render_bytes(), "render"),
            (
                crate::submit::encode_submit(3, &RenderTree::new()),
                "submit",
            ),
            (
                encode_scene_frame(&SceneFrame {
                    sequence: 3,
                    display_width: 1920,
                    display_height: 1080,
                    windows: Vec::new(),
                    removed: Vec::new(),
                }),
                "scene",
            ),
            (
                encode_input_frame(&[crate::InputEvent::new(1, Event::FocusIn)]),
                "input",
            ),
            (
                control::encode_requests(&[control::Request::new(1, RequestBody::GetDisplayInfo)]),
                "control request",
            ),
            (
                control::encode_responses(&[control::Response::new(1, ResponseBody::Ok)]),
                "control response",
            ),
        ];
        for (bytes, expected) in cases {
            let (frame, used) = decode_any(&bytes).expect("decodes");
            assert_eq!(frame.kind(), expected);
            assert_eq!(used, bytes.len(), "{expected} must consume its whole frame");
        }
    }

    #[test]
    fn a_mixed_stream_is_read_in_order() {
        // The reason this module exists: one connection carries input and
        // control replies interleaved, and the reader cannot know which is next.
        let mut buf = encode_input_frame(&[crate::InputEvent::new(1, Event::FocusIn)]);
        buf.extend_from_slice(&control::encode_responses(&[control::Response::new(
            7,
            ResponseBody::WindowCreated { window: 42 },
        )]));
        buf.extend_from_slice(&encode_input_frame(&[crate::InputEvent::new(
            1,
            Event::CloseRequested,
        )]));

        let mut kinds = Vec::new();
        let mut at = 0usize;
        while at < buf.len() {
            let (frame, used) = decode_any(&buf[at..]).expect("decodes");
            kinds.push(frame.kind());
            at += used;
        }
        assert_eq!(kinds, vec!["input", "control response", "input"]);
        assert_eq!(at, buf.len(), "no bytes left over");
    }

    #[test]
    fn an_unknown_magic_is_rejected_rather_than_guessed() {
        assert!(matches!(
            decode_any(b"XXXX\x01\x00\x00\x00\x00\x00"),
            Err(DecodeError::BadMagic)
        ));
    }

    #[test]
    fn a_partial_frame_of_every_kind_reads_as_incomplete() {
        // Including a buffer too short to even hold a magic, which is what a
        // transport delivers on its first read of a large frame.
        for bytes in [
            render_bytes(),
            crate::submit::encode_submit(1, &RenderTree::new()),
            encode_input_frame(&[crate::InputEvent::new(1, Event::FocusIn)]),
            control::encode_responses(&[control::Response::new(1, ResponseBody::Ok)]),
        ] {
            for n in 0..bytes.len() {
                assert!(
                    matches!(try_decode_any(&bytes[..n]), Ok(None)),
                    "a {n}-byte prefix must read as incomplete, not as an error"
                );
            }
            assert!(try_decode_any(&bytes).unwrap().is_some());
        }
    }

    #[test]
    fn corruption_inside_a_frame_is_still_an_error() {
        // Dispatching on the magic must not become a way to smuggle a corrupt
        // body past the decoder that owns it.
        let mut bytes = control::encode_responses(&[control::Response::new(1, ResponseBody::Ok)]);
        let last = bytes.len() - 1;
        bytes[last] = 0xEE;
        assert!(matches!(decode_any(&bytes), Err(DecodeError::BadTag(0xEE))));
    }
}
