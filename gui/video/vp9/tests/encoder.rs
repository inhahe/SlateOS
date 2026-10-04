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

/// Encode `source`, decode the result, and check it against the encoder's
/// reconstruction. Returns the compressed size and the worst plane's PSNR.
fn round_trip(source: &Picture, quantizer: u8) -> (usize, f64) {
    let (w, h) = (source.width(), source.height());
    let mut encoder = Encoder::new(EncoderConfig {
        width: w,
        height: h,
        quantizer,
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
    // written. A transform or quantiser gone wrong costs far more.
    let floors = [
        (30u8, 41.0),
        (60, 36.5),
        (120, 29.5),
        (200, 19.0),
        (255, 14.5),
    ];
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
