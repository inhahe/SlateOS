//! Replaying libvpx's own decisions through the encoder.
//!
//! The reference encode (`tests/data/encoder/rt8.ivf`, `vpxenc --rt
//! --cpu-used=8` on the first 30 pictures of a conformance vector) is
//! decoded frame by frame, and every decision libvpx made -- each block's
//! partition, modes, reference, vectors, filter, transform size, segment,
//! skip, and whether it coded any luma -- is read back from what the decoder
//! made of it, with the frame's quantiser, loop filter level and
//! segmentation. The encoder is then asked to code the same picture with the
//! same decisions, and must give libvpx's bytes: the doing half of the
//! encoder (prediction, transforms, quantisers, tokens, every probability
//! update, the headers, the segment map's coding) checked against libvpx's
//! frame by frame, whatever the deciding half does.
//!
//! The first frame is a key frame of the encoder's own decisions, which are
//! already libvpx's; each inter frame's references are then the encoder's
//! own reconstructions, which are libvpx's while the frames before matched.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]

// rustfmt formats the shared readers where they live, beside the integration
// tests, and must not follow this path: a mirror of only the changed files
// (the push gate's) does not hold them.
#[rustfmt::skip]
#[path = "../../tests/common/mod.rs"]
mod common;

use crate::block::{MiGrid, ModeInfo};
use crate::common::{BlockSize, PARTITION_SPLIT, Partition};
use crate::decoder::{Decoder, PlaneView};
use crate::enc::encodeframe::{BlockModes, Decide, FrameEncoder, InterModes};
use crate::enc::encoder::{Encoder, EncoderConfig, FixedFrame, FrameOptions};
use crate::tables;

/// libvpx's decisions for one frame, as the decoder read them.
struct Replay<'a> {
    mi: &'a MiGrid,
}

impl Decide for Replay<'_> {
    fn partition(
        &mut self,
        _f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> Partition {
        let Some(m) = self.mi.at(mi_row, mi_col) else {
            return PARTITION_SPLIT;
        };
        let bsl = usize::from(tables::B_WIDTH_LOG2[usize::from(bsize)]);
        tables::PARTITION_LOOKUP[bsl][usize::from(m.sb_type)]
    }

    fn modes(
        &mut self,
        _f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        _bsize: BlockSize,
    ) -> BlockModes {
        let m: ModeInfo = self.mi.at(mi_row, mi_col).copied().unwrap_or_default();
        let inter = m.is_inter();
        BlockModes {
            mode: m.mode,
            sub_modes: core::array::from_fn(|i| m.bmi[i].mode),
            uv_mode: m.uv_mode,
            tx_size: m.tx_size,
            inter: inter.then(|| InterModes {
                ref_frame: m.ref_frame[0],
                mode: m.mode,
                mv: m.mv[0],
                interp_filter: m.interp_filter,
                sub: core::array::from_fn(|i| (m.bmi[i].mode, m.bmi[i].mv[0])),
            }),
            segment_id: m.segment_id,
            // A libvpx block it coded nothing for is skipped; one it coded
            // no luma for may have had its luma transform skipped by the
            // mode search's estimate, which only skipping it again repeats.
            skip: inter && m.skip,
            skip_y: inter && !m.skip && !m.y_coded,
        }
    }
}

/// The first `count` pictures the full suite's vector `name` shows, as I420
/// (`common::I420`); `None` until the suite is fetched.
fn shown(name: &str, count: usize) -> Option<Vec<common::I420>> {
    let dir = common::full_suite_dir()?;
    let v = common::read_vector(&dir.join(name)).ok()?;
    let mut d = Decoder::new();
    let mut out = Vec::new();
    for p in &v.packets {
        let Some(picture) = d.decode(p).ok()? else {
            continue;
        };
        out.push(common::I420 {
            width: usize::try_from(picture.width()).unwrap(),
            height: usize::try_from(picture.height()).unwrap(),
            planes: [0, 1, 2].map(|i| {
                let view = picture.plane8(i).unwrap();
                (0..view.height)
                    .flat_map(|y| &view.data[y * view.stride..][..view.width])
                    .copied()
                    .collect()
            }),
        });
        if out.len() == count {
            return Some(out);
        }
    }
    None
}

/// `picture`'s planes as the encoder takes them.
fn views(picture: &common::I420) -> [PlaneView<'_, u8>; 3] {
    [0, 1, 2].map(|i| {
        let (width, height) = picture.plane_size(i);
        PlaneView {
            data: &picture.planes[i],
            stride: width,
            width,
            height,
        }
    })
}

/// A reference encode (`tests/data/encoder/README.md`): the pictures it was
/// made from, libvpx's frames, and the settings that make them.
struct Reference {
    input: Vec<common::I420>,
    frames: Vec<Vec<u8>>,
    config: EncoderConfig,
}

impl Reference {
    fn load(input: Vec<common::I420>, ivf: &str, config: EncoderConfig) -> Self {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/encoder")
            .join(ivf);
        let frames = common::read_ivf(&std::fs::read(path).unwrap())
            .unwrap()
            .packets;
        assert_eq!(input.len(), frames.len(), "{ivf}: pictures and frames");
        Self {
            input,
            frames,
            config,
        }
    }

    /// The first, `rt8.ivf`: the first 30 pictures the conformance vector
    /// `vp90-2-22-svc_1280x720_1.webm` shows.
    fn first() -> Self {
        let input = shown("vp90-2-22-svc_1280x720_1.webm", 30)
            .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
        Self::load(input, "rt8.ivf", EncoderConfig::realtime(1280, 720, 1000))
    }

    /// The second, `rt8cut.ivf`: 651x357 through two scene cuts, a rising
    /// noise level and a fade (`common::cut_reference_input`).
    fn second() -> Self {
        let input = common::cut_reference_input(shown)
            .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
        let (w, h) = (common::CUT_WIDTH, common::CUT_HEIGHT);
        let config =
            EncoderConfig::realtime(u32::try_from(w).unwrap(), u32::try_from(h).unwrap(), 600);
        Self::load(input, "rt8cut.ivf", config)
    }

    /// The one `VP9_TRACE_INPUT` names: `rt8` (the default) or `rt8cut`.
    fn from_env() -> Self {
        match std::env::var("VP9_TRACE_INPUT").as_deref() {
            Ok("rt8cut") => Self::second(),
            Ok("rt8") | Err(_) => Self::first(),
            Ok(other) => panic!("VP9_TRACE_INPUT={other}: rt8 or rt8cut"),
        }
    }
}

/// The encoder's own decisions on a reference's input, traced: every frame
/// is coded with [`Encoder::encode`], its decisions logged in the format of
/// the instrumented libvpx (`crate::enc::trace`) to the file `VP9_TRACE`
/// names (or `vp9-rust.trace` in the temporary directory), and each frame
/// compared with libvpx's. `VP9_TRACE_INPUT` picks the reference (`rt8`, the
/// default, or `rt8cut`) and `VP9_TRACE_FRAMES` how many of its frames. The
/// trace is for finding the first decision that differs, against libvpx's
/// own: `tools/trace/README.md`.
#[test]
#[ignore = "needs the full vector suite: python gui/video/vp9/tools/fetch_vectors.py"]
fn own_decisions_traced() {
    let reference = Reference::from_env();
    let frames: usize = std::env::var("VP9_TRACE_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(reference.frames.len());
    let mut enc = Encoder::new(reference.config).unwrap();
    crate::enc::trace::start();
    let mut first_bad = None;
    for (i, (picture, want)) in reference
        .input
        .iter()
        .zip(&reference.frames)
        .take(frames)
        .enumerate()
    {
        let got = enc.encode(views(picture)).unwrap();
        if &got != want && first_bad.is_none() {
            first_bad = Some((i, got.len(), want.len()));
        }
    }
    let lines = crate::enc::trace::take();
    let out = std::env::var("VP9_TRACE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("vp9-rust.trace"));
    std::fs::write(&out, lines.join("\n") + "\n").unwrap();
    assert!(
        first_bad.is_none(),
        "frame {} differs from libvpx's ({} bytes against {}); the trace is in {}",
        first_bad.map_or(0, |b| b.0),
        first_bad.map_or(0, |b| b.1),
        first_bad.map_or(0, |b| b.2),
        out.display()
    );
}

/// Every frame of `reference`, coded again from libvpx's own decisions, is
/// libvpx's frame byte for byte.
fn replay(reference: &Reference) {
    let mut enc = Encoder::new(reference.config).unwrap();
    let mut dec = Decoder::new();
    for (i, (picture, want)) in reference.input.iter().zip(&reference.frames).enumerate() {
        dec.decode(want).unwrap().unwrap();
        let (base_qindex, filter_level, seg) = dec.last_frame_settings();
        let mi = dec.last_mi().unwrap().clone();
        let got = if i == 0 {
            enc.encode(views(picture)).unwrap()
        } else {
            let mut d = Replay { mi: &mi };
            let opts = FrameOptions {
                tx_mode: None,
                inter: true,
                fixed: Some(FixedFrame {
                    base_qindex,
                    filter_level,
                    seg,
                }),
            };
            enc.encode_frame(views(picture), Some(&mut d), opts)
                .unwrap()
        };
        let first = got.iter().zip(want.iter()).position(|(a, b)| a != b);
        assert!(
            &got == want,
            "frame {i} (q {base_qindex}, filter {filter_level}): {} bytes against libvpx's {}, first difference at byte {first:?}",
            got.len(),
            want.len()
        );
    }
}

/// Every frame of the first reference encode, coded again from libvpx's own
/// decisions, is libvpx's frame byte for byte.
#[test]
#[ignore = "needs the full vector suite: python gui/video/vp9/tools/fetch_vectors.py"]
fn libvpxs_decisions_give_libvpxs_frames() {
    replay(&Reference::first());
}

/// The same for the second: blocks over the picture's edges predict from
/// the reconstruction's extended edges as libvpx's do, and the frames after
/// a scene cut, a golden refresh and a fade are libvpx's.
#[test]
#[ignore = "needs the full vector suite: python gui/video/vp9/tools/fetch_vectors.py"]
fn libvpxs_decisions_give_libvpxs_frames_through_cuts_noise_and_edges() {
    replay(&Reference::second());
}
