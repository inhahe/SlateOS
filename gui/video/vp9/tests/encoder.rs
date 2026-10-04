//! The encoder against the decoder, on real pictures: frames of libvpx's
//! conformance vectors, decoded, encoded again at a spread of quantisers,
//! and decoded once more. What the decoder shows must be exactly what the
//! encoder says it reconstructed; at quantiser 0 that is the source itself,
//! and above it the loss must stay where the quantiser puts it.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    reason = "a test: a mismatch should fail it loudly"
)]

mod common;

use std::path::Path;

use vp9::{Decoder, Encoder, EncoderConfig, Picture, PlaneView};

/// Mean squared error to PSNR in dB, for 8-bit samples.
fn psnr(a: &PlaneView<'_, u8>, b: &PlaneView<'_, u8>) -> f64 {
    let mut sse = 0u64;
    for y in 0..a.height {
        for x in 0..a.width {
            let d = i64::from(a.data[y * a.stride + x]) - i64::from(b.data[y * b.stride + x]);
            sse += (d * d) as u64;
        }
    }
    if sse == 0 {
        return f64::INFINITY;
    }
    let mse = sse as f64 / (a.width * a.height) as f64;
    10.0 * (255.0 * 255.0 / mse).log10()
}

fn same(a: &Picture, b: &Picture) -> bool {
    (0..3).all(|i| {
        let (pa, pb) = (a.plane8(i).unwrap(), b.plane8(i).unwrap());
        pa.width == pb.width
            && pa.height == pb.height
            && (0..pa.height).all(|y| {
                pa.data[y * pa.stride..][..pa.width] == pb.data[y * pb.stride..][..pb.width]
            })
    })
}

/// Every picture of a vector, as the decoder shows them.
fn pictures(name: &str) -> Vec<Picture> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    let v = common::read_vector(&path).unwrap();
    let mut d = Decoder::new();
    v.packets
        .iter()
        .filter_map(|p| d.decode(p).unwrap())
        .collect()
}

/// Encode `source` with the quantiser pinned at `quantizer` (libvpx's 0 to
/// 63 scale), decode the result, and check it against the encoder's
/// reconstruction. Returns the compressed size and the worst plane's PSNR.
fn round_trip(source: &Picture, quantizer: u8) -> (usize, f64) {
    let (w, h) = (source.width(), source.height());
    let mut encoder = Encoder::new(EncoderConfig {
        min_quantizer: quantizer,
        max_quantizer: quantizer,
        ..EncoderConfig::realtime(w, h, 1000)
    })
    .unwrap();
    let planes = [0, 1, 2].map(|i| source.plane8(i).unwrap());
    let frame = encoder.encode(planes).unwrap();
    let shown = Decoder::new().decode(&frame).unwrap().unwrap();
    let recon = encoder.reconstruction().unwrap();
    assert!(
        same(&shown, &recon),
        "{w}x{h} at {quantizer}: the decoder sees something else"
    );
    let worst = (0..3)
        .map(|i| psnr(&source.plane8(i).unwrap(), &recon.plane8(i).unwrap()))
        .fold(f64::INFINITY, f64::min);
    (frame.len(), worst)
}

/// Real pictures at a spread of quantisers: the decoder sees the encoder's
/// reconstruction, quantiser 0 is lossless, and a coarser quantiser gives a
/// smaller frame and more loss, but never more than its step allows.
#[test]
fn real_pictures_round_trip_at_every_quantiser() {
    // (quantiser, the least PSNR in dB any plane may give): about 1.5 dB
    // under the least the encoder's fixed decisions gave when this was
    // written. A transform or quantiser gone wrong costs far more. The
    // quantisers are libvpx's 0-to-63 scale: indices 32, 60, 120, 200, 255.
    let floors = [(8u8, 41.0), (15, 36.5), (30, 29.5), (50, 19.0), (63, 14.5)];
    for name in [
        "vp90-2-01-sharpness-1.webm",
        "vp90-2-02-size-66x66.webm",
        "vp90-2-02-size-08x10.webm",
    ] {
        for (i, source) in pictures(name).iter().enumerate().step_by(3) {
            let (lossless_size, lossless) = round_trip(source, 0);
            assert!(
                lossless.is_infinite(),
                "{name} picture {i}: quantiser 0 lost {lossless} dB"
            );
            let mut last_size = lossless_size;
            for &(q, floor) in &floors {
                let (size, p) = round_trip(source, q);
                eprintln!("{name} picture {i} q{q}: {size} bytes, {p:.2} dB");
                assert!(
                    p >= floor,
                    "{name} picture {i} at {q}: a plane at {p:.2} dB, below {floor}"
                );
                assert!(
                    size <= last_size,
                    "{name} picture {i}: {q} gave {size} bytes, more than {last_size}"
                );
                last_size = size;
            }
        }
    }
}

/// The pictures libvpx's reference encode was made from: the first 30 that
/// the conformance vector `vp90-2-22-svc_1280x720_1.webm` shows, which
/// `vpxdec --i420 --limit=30` wrote out for `vpxenc` (and which this
/// decoder shows bit for bit). `None` until the full vector suite is
/// fetched.
fn reference_input() -> Option<Vec<Picture>> {
    let dir = common::full_suite_dir()?;
    let v = common::read_vector(&dir.join("vp90-2-22-svc_1280x720_1.webm")).ok()?;
    let mut d = Decoder::new();
    let mut out = Vec::new();
    for p in &v.packets {
        if let Some(picture) = d.decode(p).ok()? {
            out.push(picture);
            if out.len() == 30 {
                break;
            }
        }
    }
    Some(out)
}

/// libvpx's reference encode of [`reference_input`]: `vpxenc --rt
/// --cpu-used=8` at the settings `EncoderConfig::realtime(1280, 720, 1000)`
/// gives (`tests/data/encoder/README.md` has the command). The encoder's
/// frames must be byte-identical to it.
#[test]
#[ignore = "needs the full vector suite: python gui/video/vp9/tools/fetch_vectors.py"]
fn frames_match_vpxenc_realtime() {
    let input = reference_input()
        .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/encoder/rt8.ivf");
    let reference = common::read_ivf(&std::fs::read(path).unwrap()).unwrap();
    let mut encoder = Encoder::new(EncoderConfig::realtime(1280, 720, 1000)).unwrap();
    assert_eq!((input.len(), reference.packets.len()), (30, 30));
    // Every frame: the key frame, then 29 inter frames of libvpx's own
    // decisions, made by the encoder.
    for (i, (picture, want)) in input.iter().zip(&reference.packets).enumerate() {
        let planes = [0, 1, 2].map(|p| picture.plane8(p).unwrap());
        let got = encoder.encode(planes).unwrap();
        let first = got.iter().zip(want.iter()).position(|(a, b)| a != b);
        assert!(
            &got == want,
            "frame {i}: {} bytes against libvpx's {}, first difference at byte {first:?}",
            got.len(),
            want.len()
        );
    }
}

/// The first `count` pictures the full suite's vector `name` shows, as I420:
/// what `common::cut_reference_input` builds the second reference from.
fn shown(name: &str, count: usize) -> Option<Vec<common::I420>> {
    let dir = common::full_suite_dir()?;
    let v = common::read_vector(&dir.join(name)).ok()?;
    let mut d = Decoder::new();
    let mut out = Vec::new();
    for p in &v.packets {
        let Some(picture) = d.decode(p).ok()? else {
            continue;
        };
        let planes = [0, 1, 2].map(|i| {
            let view = picture.plane8(i).unwrap();
            (0..view.height)
                .flat_map(|y| &view.data[y * view.stride..][..view.width])
                .copied()
                .collect::<Vec<u8>>()
        });
        out.push(common::I420 {
            width: usize::try_from(picture.width()).unwrap(),
            height: usize::try_from(picture.height()).unwrap(),
            planes,
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

/// The MD5 of the second reference's input, as `vpxenc` was given it: the
/// pictures one after another, raw.
const CUT_INPUT_MD5: &str = "58155d770c2098e80eee156307a3c300";

/// libvpx's second reference encode (`tests/data/encoder/README.md`): 150
/// pictures of 651x357 through two scene cuts, a rising noise level, golden
/// refreshes and a fade, with blocks hanging over the picture's edges. The
/// encoder's frames must be byte-identical to it.
#[test]
#[ignore = "needs the full vector suite: python gui/video/vp9/tools/fetch_vectors.py"]
fn frames_match_vpxenc_through_cuts_noise_and_edges() {
    let input = common::cut_reference_input(shown)
        .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
    let mut md5 = md5::Md5::new();
    for p in &input {
        md5.update(&p.raw());
    }
    assert_eq!(
        md5::hex(&md5.finalize()).to_string(),
        CUT_INPUT_MD5,
        "the input is not what vpxenc was given"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/encoder/rt8cut.ivf");
    let reference = common::read_ivf(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!((input.len(), reference.packets.len()), (150, 150));
    let (w, h) = (common::CUT_WIDTH, common::CUT_HEIGHT);
    let mut encoder = Encoder::new(EncoderConfig::realtime(w as u32, h as u32, 600)).unwrap();
    for (i, (picture, want)) in input.iter().zip(&reference.packets).enumerate() {
        let got = encoder.encode(views(picture)).unwrap();
        let first = got.iter().zip(want.iter()).position(|(a, b)| a != b);
        assert!(
            &got == want,
            "frame {i}: {} bytes against libvpx's {}, first difference at byte {first:?}",
            got.len(),
            want.len()
        );
    }
}

/// The MD5 of the third reference's input, as `vpxenc` was given it.
const SMALL_INPUT_MD5: &str = "e75fb192ba8ab28ad9fde1224898daed";

/// libvpx's third reference encode (`tests/data/encoder/README.md`): 90
/// pictures of 350x286, small enough that libvpx partitions its inter frames
/// by its learned search. The encoder's frames must be byte-identical to it.
#[test]
#[ignore = "needs the full vector suite: python gui/video/vp9/tools/fetch_vectors.py"]
fn frames_match_vpxenc_when_partitioned_by_search() {
    let input = common::small_reference_input(shown)
        .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
    let mut md5 = md5::Md5::new();
    for p in &input {
        md5.update(&p.raw());
    }
    assert_eq!(
        md5::hex(&md5.finalize()).to_string(),
        SMALL_INPUT_MD5,
        "the input is not what vpxenc was given"
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/encoder/rt8small.ivf");
    let reference = common::read_ivf(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!((input.len(), reference.packets.len()), (90, 90));
    let (w, h) = (common::SMALL_WIDTH, common::SMALL_HEIGHT);
    let mut encoder = Encoder::new(EncoderConfig::realtime(w as u32, h as u32, 200)).unwrap();
    for (i, (picture, want)) in input.iter().zip(&reference.packets).enumerate() {
        let got = encoder.encode(views(picture)).unwrap();
        let first = got.iter().zip(want.iter()).position(|(a, b)| a != b);
        assert!(
            &got == want,
            "frame {i}: {} bytes against libvpx's {}, first difference at byte {first:?}",
            got.len(),
            want.len()
        );
    }
}

/// Write a reference's input, raw I420, to the file `VP9_WRITE_INPUT` names:
/// what `vpxenc` is given to make `rt8cut.ivf`, or with `VP9_WRITE_WHICH=small`
/// `rt8small.ivf`. A tool rather than a test, so it does nothing unless
/// asked: a run of every ignored test passes through it.
#[test]
#[ignore = "a tool for remaking the references: VP9_WRITE_INPUT=path"]
fn write_cut_reference_input() {
    let Ok(out) = std::env::var("VP9_WRITE_INPUT") else {
        eprintln!("VP9_WRITE_INPUT is not set: nothing written");
        return;
    };
    let input = if std::env::var("VP9_WRITE_WHICH").as_deref() == Ok("small") {
        common::small_reference_input(shown)
    } else {
        common::cut_reference_input(shown)
    }
    .expect("the full suite is not fetched");
    let raw: Vec<u8> = input.iter().flat_map(common::I420::raw).collect();
    std::fs::write(out, raw).unwrap();
}
