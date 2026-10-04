//! The decoder: a stream of frames in, the pictures it shows out.
//!
//! This is libvpx's frame loop -- `vp8_dx_iface.c`'s `vp8_decode`, and
//! `onyxd_if.c`'s `vp8dx_receive_compressed_data`, `swap_frame_buffers` and
//! `vp8dx_get_raw_frame` -- with the state they keep: four frame buffers
//! shared by reference count among the frame being decoded and the three
//! references (last, golden and altref), and the size the last key frame set.
//!
//! The buffers are libvpx's in number and in how they are handed out, the
//! lowest free one first, because two things a damaged stream can do depend
//! on which buffer is which: a reference copied from a buffer that does not
//! exist is buffer 0, and a version-3 macroblock whose chroma libvpx does not
//! predict keeps what its buffer held. A picture handed to the caller keeps
//! its buffer alive; the decoder then decodes into a fresh one, remembering
//! the old for that one case.
//!
//! Translated into Rust from libvpx v1.17.0's `vp8/vp8_dx_iface.c`,
//! `vp8/decoder/onyxd_if.c` and `vp8/common/alloccommon.c` (copyright the
//! WebM project authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "buffer numbers are below 4: the slots and reference counts are four long, and every number stored is one found among them"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "reference counts are at most 4, and only decremented when positive"
)]

use std::sync::Arc;

use crate::Error;
use crate::decodeframe::{self, Common, FrameError, Refs};
use crate::frame::Frame;
use crate::header;

/// The largest picture this decoder accepts by default, in luma samples:
/// 8192 x 4352, as gui/video/vp9's. libvpx accepts any size VP8 can code
/// (16383 x 16383); a limit is what keeps a hostile stream from asking for
/// gigabytes. [`Decoder::with_max_pixels`] changes it.
pub const DEFAULT_MAX_PIXELS: u64 = 8192 * 4352;

/// libvpx's `NUM_YV12_BUFFERS`.
const BUFFERS: usize = 4;

/// A picture the decoder showed: 8-bit Y, U and V, chroma halved both ways.
#[derive(Clone, Debug)]
pub struct Picture {
    frame: Arc<Frame>,
    corrupted: bool,
}

/// One plane of a [`Picture`]: `height` rows of `width` samples, `stride`
/// apart. Rows are longer than `width`; only the first `width` samples of
/// each are the picture.
#[derive(Clone, Copy, Debug)]
pub struct PlaneView<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

impl Picture {
    /// The picture's width in luma samples.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.frame.display_width
    }

    /// The picture's height in luma samples.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.frame.display_height
    }

    /// Plane `i`: 0 Y, 1 U, 2 V. A chroma plane is half the luma size,
    /// rounded up.
    #[must_use]
    pub fn plane(&self, i: usize) -> Option<PlaneView<'_>> {
        let p = self.frame.planes.get(i)?;
        let (w, h) = (self.width() as usize, self.height() as usize);
        let (width, height) = if i == 0 {
            (w, h)
        } else {
            (w.div_ceil(2), h.div_ceil(2))
        };
        Some(PlaneView {
            data: p.data.get(p.origin()..)?,
            stride: p.stride,
            width,
            height,
        })
    }

    /// Whether the picture decoded from damaged data, or predicts from a
    /// picture that did: libvpx's `VP8D_GET_FRAME_CORRUPTED`.
    #[must_use]
    pub fn corrupted(&self) -> bool {
        self.corrupted
    }
}

/// The decoder. Feed it frames in stream order with [`Decoder::decode`].
#[derive(Debug)]
pub struct Decoder {
    max_pixels: u64,
    /// Whether a key frame has created the decoder's state: libvpx's
    /// `decoder_init`.
    init: bool,
    /// The size the last key frame declared: libvpx's `ctx->si`. A new size
    /// reallocates everything.
    si: (u32, u32),
    common: Common,
    /// The frame buffers, none until a key frame sizes them.
    slots: Vec<Option<Arc<Frame>>>,
    /// Whether each buffer is corrupt: libvpx's `corrupted`, kept beside the
    /// buffers so that marking one does not copy a picture the caller holds.
    corrupted: [bool; BUFFERS],
    /// How many of new, last, golden and altref name each buffer: libvpx's
    /// `fb_idx_ref_cnt`.
    ref_cnt: [u8; BUFFERS],
    last: usize,
    golden: usize,
    altref: usize,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// A decoder for a new stream, which must begin with a key frame.
    #[must_use]
    pub fn new() -> Self {
        Self {
            max_pixels: DEFAULT_MAX_PIXELS,
            init: false,
            si: (0, 0),
            common: Common::new(),
            slots: Vec::new(),
            corrupted: [false; BUFFERS],
            ref_cnt: [0; BUFFERS],
            last: 1,
            golden: 2,
            altref: 3,
        }
    }

    /// A decoder that refuses pictures of more than `max_pixels` luma
    /// samples, rather than [`DEFAULT_MAX_PIXELS`].
    #[must_use]
    pub fn with_max_pixels(max_pixels: u64) -> Self {
        Self {
            max_pixels,
            ..Self::new()
        }
    }

    /// Decode one frame (one block of a container) and return the picture
    /// it shows, if it shows one: libvpx's `vpx_codec_decode` followed by
    /// `vpx_codec_get_frame`. An empty frame does nothing, as libvpx's flush
    /// call does.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for a stream that does not begin with a key
    /// frame, a key frame without VP8's start code, or a picture larger than
    /// the decoder accepts; [`Error::Corrupt`] for a damaged frame. A frame
    /// that fails shows nothing; the frames after it decode, predicting from
    /// what the references hold, as libvpx's do (inter frames after a failed
    /// key frame are refused until a key frame decodes).
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<Picture>, Error> {
        if data.is_empty() {
            return Ok(None);
        }
        let (w, h) = self.si;
        let peek = header::peek(data);
        if let Some(size) = peek.size {
            self.si = size;
        }
        let mut res = peek.result;
        // Peeking refuses an inter frame, which decoding does not.
        if matches!(res, Err(Error::Unsupported(_))) && !peek.is_kf {
            res = Ok(());
        }
        if !self.init && !peek.is_kf {
            res = Err(Error::Unsupported(
                "the stream does not start with a key frame",
            ));
        }
        if res.is_ok() && self.init && (w, h) == (0, 0) && self.si == (0, 0) {
            res = Err(Error::Corrupt("a key frame is needed to reset the decoder"));
        }
        let resolution_change = self.si != (w, h);
        res?;
        if !self.init {
            self.common = Common::new();
            self.init = true;
        }
        if resolution_change {
            let (nw, nh) = self.si;
            self.common.width = nw;
            self.common.height = nh;
            self.common.decoded_key_frame = false;
            if u64::from(nw) * u64::from(nh) > self.max_pixels {
                // As libvpx after a failed allocation: nothing allocated,
                // and the next key frame tries again.
                self.slots.clear();
                self.common.deallocate();
                self.si = (0, 0);
                return Err(Error::Unsupported(
                    "the frame is larger than this decoder accepts",
                ));
            }
            self.allocate(nw, nh);
        }
        self.receive(data)
    }

    /// Fresh buffers and per-macroblock state for a new size: libvpx's
    /// `vp8_alloc_frame_buffers`, and `vp8_decode`'s release of buffer 0 for
    /// the first frame.
    fn allocate(&mut self, width: u32, height: u32) {
        self.slots = (0..BUFFERS)
            .map(|_| Some(Arc::new(Frame::new(width, height))))
            .collect();
        self.corrupted = [false; BUFFERS];
        self.ref_cnt = [0, 1, 1, 1];
        (self.last, self.golden, self.altref) = (1, 2, 3);
        self.common.allocate(width, height);
    }

    /// Decode a frame into a free buffer and update the references: libvpx's
    /// `vp8dx_receive_compressed_data`, `swap_frame_buffers` and
    /// `vp8dx_get_raw_frame`.
    fn receive(&mut self, data: &[u8]) -> Result<Option<Picture>, Error> {
        let Some(new) = (0..BUFFERS).find(|&i| self.ref_cnt[i] == 0) else {
            return Err(Error::Corrupt("no frame buffer is free"));
        };
        if self.slots.len() != BUFFERS {
            return Err(Error::Corrupt("a key frame is needed to reset the decoder"));
        }
        self.ref_cnt[new] = 1;
        let (width, height) = (self.common.width, self.common.height);
        let (mut frame, stale) = match self.slots[new].take() {
            Some(arc) => match Arc::try_unwrap(arc) {
                Ok(frame) => (frame, None),
                // A picture the caller holds: decode into a new buffer.
                Err(shared) => (Frame::new(width, height), Some(shared)),
            },
            None => (Frame::new(width, height), None),
        };
        self.corrupted[new] = false;
        let result = match (
            &self.slots[self.last],
            &self.slots[self.golden],
            &self.slots[self.altref],
        ) {
            (Some(last), Some(golden), Some(altref)) => {
                let refs = Refs {
                    last,
                    golden,
                    altref,
                    corrupted: [
                        false,
                        self.corrupted[self.last],
                        self.corrupted[self.golden],
                        self.corrupted[self.altref],
                    ],
                };
                decodeframe::decode_frame(
                    &mut self.common,
                    data,
                    &mut frame,
                    stale.as_deref(),
                    &refs,
                )
            }
            _ => Err(FrameError::Error(Error::Corrupt(
                "a reference frame is missing",
            ))),
        };
        self.slots[new] = Some(Arc::new(frame));
        let corrupted = match result {
            Ok(corrupted) => corrupted,
            Err(e) => {
                self.ref_cnt[new] = self.ref_cnt[new].saturating_sub(1);
                return Err(match e {
                    FrameError::Error(e) => {
                        // libvpx's error path: the missing frame may have been
                        // meant to update any reference; mark the last.
                        self.corrupted[self.last] = true;
                        e
                    }
                    FrameError::NoKeyFrameYet => Error::Corrupt("no key frame has decoded yet"),
                });
            }
        };
        self.corrupted[new] = corrupted;

        // libvpx's `swap_frame_buffers`. A copy from buffer "3" names no
        // reference; libvpx copies from buffer 0, carries on, and shows
        // nothing.
        let c = &self.common;
        let mut bad_copy = false;
        if c.copy_to_arf != 0 {
            let src = match c.copy_to_arf {
                1 => self.last,
                2 => self.golden,
                _ => {
                    bad_copy = true;
                    0
                }
            };
            ref_cnt_fb(&mut self.ref_cnt, &mut self.altref, src);
        }
        if c.copy_to_gf != 0 {
            let src = match c.copy_to_gf {
                1 => self.last,
                2 => self.altref,
                _ => {
                    bad_copy = true;
                    0
                }
            };
            ref_cnt_fb(&mut self.ref_cnt, &mut self.golden, src);
        }
        if c.refresh_golden {
            ref_cnt_fb(&mut self.ref_cnt, &mut self.golden, new);
        }
        if c.refresh_altref {
            ref_cnt_fb(&mut self.ref_cnt, &mut self.altref, new);
        }
        let to_show = if c.refresh_last {
            ref_cnt_fb(&mut self.ref_cnt, &mut self.last, new);
            self.last
        } else {
            new
        };
        self.ref_cnt[new] = self.ref_cnt[new].saturating_sub(1);
        if bad_copy {
            return Err(Error::Corrupt(
                "a reference copied from a buffer that does not exist",
            ));
        }
        if !c.show_frame {
            return Ok(None);
        }
        Ok(self.slots[to_show].as_ref().map(|frame| Picture {
            frame: Arc::clone(frame),
            corrupted: self.corrupted[to_show],
        }))
    }
}

/// Point reference `idx` at buffer `new_idx`, moving a count from the old
/// buffer to the new: libvpx's `ref_cnt_fb`.
fn ref_cnt_fb(counts: &mut [u8; BUFFERS], idx: &mut usize, new_idx: usize) {
    counts[*idx] = counts[*idx].saturating_sub(1);
    *idx = new_idx;
    counts[new_idx] += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_frame_does_nothing() {
        let mut d = Decoder::new();
        assert!(matches!(d.decode(&[]), Ok(None)));
        assert!(!d.init);
    }

    #[test]
    fn a_stream_must_start_with_a_key_frame() {
        let mut d = Decoder::new();
        let inter = [0x31u8, 0x01, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(matches!(d.decode(&inter), Err(Error::Unsupported(_))));
    }

    #[test]
    fn a_frame_too_large_is_refused_before_anything_is_allocated() {
        let mut d = Decoder::with_max_pixels(64 * 64);
        let mut kf = vec![0x10u8, 0x02, 0x00, 0x9d, 0x01, 0x2a];
        kf.extend_from_slice(&100u16.to_le_bytes());
        kf.extend_from_slice(&100u16.to_le_bytes());
        kf.extend_from_slice(&[0; 32]);
        assert!(matches!(d.decode(&kf), Err(Error::Unsupported(_))));
        assert!(d.slots.is_empty());
        // And an inter frame after it asks for a key frame.
        let inter = [0x31u8, 0x01, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(matches!(d.decode(&inter), Err(Error::Corrupt(_))));
    }

    #[test]
    fn references_move_their_counts() {
        let mut counts = [1u8, 2, 0, 1];
        let mut idx = 1;
        ref_cnt_fb(&mut counts, &mut idx, 2);
        assert_eq!((counts, idx), ([1, 1, 1, 1], 2));
    }
}
