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
use crate::threading::{MIN_MACROBLOCKS_PER_THREAD, Threading};

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
    /// The two colour bits of the key frame it follows.
    color_space: u8,
    clamping_type: u8,
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

    /// The colour space bit of the key frame the picture follows: 0 for
    /// VP8's YUV, which its specification (RFC 6386 §9.2) likens to BT.601;
    /// 1 is reserved. libvpx reads it and goes by nothing; FFmpeg takes 0 for
    /// BT.601 (its `BT470BG` matrix) and 1 for unspecified.
    #[must_use]
    pub fn color_space(&self) -> u8 {
        self.color_space
    }

    /// The clamping type bit of the key frame the picture follows: 0 if the
    /// decoder must clamp reconstructed pixels, 1 if no clamping is needed.
    /// libvpx clamps whatever it says; FFmpeg takes 1 for full range samples
    /// and 0 for the studio range.
    #[must_use]
    pub fn clamping_type(&self) -> u8 {
        self.clamping_type
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
    /// How many threads a frame's macroblock rows may decode on.
    threading: Threading,
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
            threading: Threading {
                threads: std::thread::available_parallelism()
                    .map_or(1, std::num::NonZeroUsize::get),
                min_macroblocks: MIN_MACROBLOCKS_PER_THREAD,
            },
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

    /// Decode on at most `threads` threads (at least one) from the next
    /// frame on. A new decoder uses as many as the machine has cores.
    ///
    /// A frame's token partitions are what decode in parallel, a row of
    /// macroblocks to each, so a frame of one partition -- what encoders
    /// make unless asked for more (`vpxenc --token-parts`) -- decodes on
    /// one thread whatever this says, as libvpx's do. A small frame decodes
    /// on fewer than this too: a thread for each 150 macroblocks at most
    /// (16x16 pixels each; a 640x360 picture has 920), below which starting
    /// a thread costs more than it saves. The pictures are the same however
    /// many threads make them.
    pub fn set_threads(&mut self, threads: usize) {
        self.threading.threads = threads.max(1);
    }

    /// How many threads the decoder may use.
    #[must_use]
    pub fn threads(&self) -> usize {
        self.threading.threads
    }

    /// Give a thread as few as `macroblocks` macroblocks a frame (0: any
    /// number), rather than the 150 below which threads cost more than they
    /// save: for tests and measurements, which decode small frames on
    /// threads to see that they make the same pictures, or what they cost.
    #[doc(hidden)]
    pub fn set_min_macroblocks_per_thread(&mut self, macroblocks: usize) {
        self.threading.min_macroblocks = macroblocks;
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
                    self.threading,
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
            color_space: c.color_space,
            clamping_type: c.clamping_type,
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
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test: a failure should be loud"
    )]

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

    /// The frames of an IVF file.
    fn ivf_frames(data: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let mut at = 32;
        while let Some(head) = data.get(at..at + 12) {
            let size = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
            at += 12;
            let Some(frame) = data.get(at..at + size) else {
                break;
            };
            frames.push(frame.to_vec());
            at += size;
        }
        frames
    }

    /// The committed test vectors: each one's name and frames.
    fn committed_vectors() -> Vec<(String, Vec<Vec<u8>>)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("data");
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "ivf"))
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "no vectors in {}", dir.display());
        paths
            .iter()
            .map(|p| {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                (name, ivf_frames(&std::fs::read(p).unwrap()))
            })
            .collect()
    }

    /// What a decode call returned, with the picture's every byte.
    type Shown = Result<Option<(bool, Vec<u8>)>, Error>;

    fn shown(result: &Result<Option<Picture>, Error>) -> Shown {
        result.clone().map(|p| {
            p.map(|p| {
                let bytes = p
                    .frame
                    .planes
                    .iter()
                    .flat_map(|plane| plane.data.iter().copied());
                (p.corrupted(), bytes.collect())
            })
        })
    }

    /// Assert that two decoders hold the same state: every buffer, borders
    /// and all, every reference, every count, and what the frame loop keeps
    /// for the next frame.
    fn assert_same_state(one: &Decoder, other: &Decoder, what: &str) {
        assert_eq!(one.slots.len(), other.slots.len(), "{what}");
        for (i, (a, b)) in one.slots.iter().zip(&other.slots).enumerate() {
            match (a, b) {
                (Some(a), Some(b)) => {
                    for p in 0..3 {
                        assert!(
                            a.planes[p].data == b.planes[p].data,
                            "{what}: buffer {i}, plane {p} differs"
                        );
                    }
                }
                (None, None) => {}
                _ => panic!("{what}: buffer {i} is in one decoder only"),
            }
        }
        assert_eq!(one.corrupted, other.corrupted, "{what}");
        assert_eq!(one.ref_cnt, other.ref_cnt, "{what}");
        let refs = |d: &Decoder| (d.last, d.golden, d.altref);
        assert_eq!(refs(one), refs(other), "{what}");
        let skips = |d: &Decoder| {
            let cells = d.common.grid.cells.iter();
            cells.map(|c| c.mb_skip_coeff).collect::<Vec<_>>()
        };
        assert!(skips(one) == skips(other), "{what}: skip flags differ");
        assert_eq!(one.common.above(), other.common.above(), "{what}");
    }

    /// Decode `frames` on one thread and on several in step, holding each
    /// picture until the next if `hold` (so frames decode into fresh
    /// buffers), and assert that every frame leaves the same state.
    fn decode_alike(name: &str, frames: &[Vec<u8>], hold: bool) {
        let mut decoders: Vec<Decoder> = [1, 2, 3, 8]
            .into_iter()
            .map(|threads| {
                let mut d = Decoder::new();
                d.set_threads(threads);
                // The committed vectors are far too small to be given
                // threads otherwise.
                d.set_min_macroblocks_per_thread(0);
                d
            })
            .collect();
        let mut held: Vec<Option<Picture>> = vec![None; decoders.len()];
        for (i, frame) in frames.iter().enumerate() {
            let results: Vec<_> = decoders.iter_mut().map(|d| d.decode(frame)).collect();
            let what = format!("{name}, frame {i}");
            for (d, r) in decoders.iter().zip(&results).skip(1) {
                let threads = d.threads();
                assert!(
                    shown(&results[0]) == shown(r),
                    "{what} on {threads} threads"
                );
                assert_same_state(&decoders[0], d, &format!("{what} on {threads} threads"));
            }
            if hold {
                for (h, r) in held.iter_mut().zip(results) {
                    *h = r.ok().flatten();
                }
            }
        }
    }

    #[test]
    fn every_vector_leaves_the_same_buffers_on_any_number_of_threads() {
        let (threaded, _) = crate::threading::TALLY.get();
        for (name, frames) in committed_vectors() {
            decode_alike(&name, &frames, false);
            decode_alike(&name, &frames, true);
        }
        let (now, _) = crate::threading::TALLY.get();
        // The partitions vectors have two, four and eight partitions.
        assert!(
            now - threaded >= 100,
            "only {} frames decoded on threads",
            now - threaded
        );
    }

    #[test]
    fn a_small_frame_decodes_on_one_thread_unless_told_otherwise() {
        let vectors = committed_vectors();
        let (_, frames) = vectors
            .iter()
            .find(|(name, _)| name == "vp80-04-partitions-1406.ivf")
            .unwrap();
        let decode = |min_macroblocks| {
            let (threaded, _) = crate::threading::TALLY.get();
            let mut d = Decoder::new();
            d.set_threads(8);
            d.set_min_macroblocks_per_thread(min_macroblocks);
            for frame in frames {
                d.decode(frame).unwrap();
            }
            crate::threading::TALLY.get().0 - threaded
        };
        // 176x144: 99 macroblocks, too few for a second thread.
        assert_eq!(decode(MIN_MACROBLOCKS_PER_THREAD), 0);
        // Eight partitions, nine rows: threads, when allowed any frame.
        assert_eq!(decode(0), frames.len());
    }

    #[test]
    fn damaged_streams_leave_the_same_buffers_on_any_number_of_threads() {
        // Numerical Recipes' generator.
        let mut seed = 0x5eed_0f08_u32;
        let mut next = |n: usize| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as usize % n.max(1)
        };
        for (name, frames) in committed_vectors() {
            for round in 0..6 {
                let damaged: Vec<Vec<u8>> = frames
                    .iter()
                    .take(12)
                    .map(|f| {
                        let mut f = f.clone();
                        for _ in 0..next(6) {
                            let at = next(f.len());
                            if let Some(b) = f.get_mut(at) {
                                *b ^= (next(255) + 1) as u8;
                            }
                        }
                        if next(5) == 0 {
                            let len = next(f.len() + 1);
                            f.truncate(len);
                        }
                        f
                    })
                    .collect();
                decode_alike(&format!("{name}, damage {round}"), &damaged, round % 2 == 1);
            }
        }
    }
}
