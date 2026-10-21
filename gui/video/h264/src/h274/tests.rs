//! Film grain held to FFmpeg's own synthesis: `tools/h274_expect.c` runs
//! FFmpeg 8.1.3's `ff_h274_apply_film_grain` on the pictures and models of
//! [`CASES`] and prints each output plane's Adler-32, which the translation
//! must match. Between them the cases take every path: bands with gaps
//! between them (blank blocks), signs flipped, strengths that overflow a
//! sample of grain, sums clipped, chroma halving and doubling past what
//! 16 bits hold, cut-offs clipped, a negative seed, a component without
//! grain, 16x16 blocks cut short, and the two refusals.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "test code, on values the tests choose"
)]

use super::{GrainParams, Unsupported, apply_film_grain, pattern};
use crate::picture::{AnyFrame, FrameBuf, PixelFormat, Plane};
use crate::sei::FilmGrain;

/// A band of brightness, as `h274_expect.c`'s `struct band`: its lower and
/// upper bound, then the strength and the two cut-offs it is given.
type Band = (u8, u8, i16, i16, i16);

/// A case, as `h274_expect.c`'s `struct cas`, with what FFmpeg made of it:
/// each plane's Adler-32, or `None` where it refused.
struct Case {
    name: &'static str,
    size: (usize, usize),
    format: PixelFormat,
    model_id: u32,
    log2_scale_factor: u32,
    seed: i32,
    /// Each component's bands; `None` for a component without grain.
    comps: [Option<&'static [Band]>; 3],
    expect: Option<[u32; 3]>,
}

const CASES: [Case; 6] = [
    Case {
        name: "bands",
        size: (64, 48),
        format: PixelFormat::Yuv420(8),
        model_id: 0,
        log2_scale_factor: 3,
        seed: 0,
        comps: [
            Some(&[
                (0, 80, 40, 4, 6),
                (81, 160, -60, 8, 3),
                (161, 255, 90, 14, 14),
            ]),
            Some(&[(0, 127, 30, 2, 2), (128, 255, -25, 5, 7)]),
            Some(&[(0, 255, 50, 6, 4)]),
        ],
        expect: Some([0x4dbc_8a29, 0x81e0_d113, 0x2ab8_1752]),
    },
    Case {
        name: "gaps",
        size: (48, 48),
        format: PixelFormat::Yuv420(8),
        model_id: 0,
        log2_scale_factor: 2,
        seed: 7,
        comps: [
            Some(&[(20, 60, 70, 3, 9), (140, 200, -45, 12, 2)]),
            Some(&[(100, 180, -7, 4, 4)]),
            None,
        ],
        expect: Some([0xd861_090d, 0xecef_41d4, 0x7286_ac90]),
    },
    Case {
        name: "strong",
        size: (32, 32),
        format: PixelFormat::Yuv420(8),
        model_id: 0,
        log2_scale_factor: 0,
        seed: -5,
        comps: [
            Some(&[(0, 255, 2000, 9, 11)]),
            Some(&[(0, 255, -3001, 1, 99)]),
            Some(&[(0, 255, 32767, 20000, -20000)]),
        ],
        expect: Some([0x6395_bdd8, 0xeba8_7b87, 0xf353_b1ac]),
    },
    Case {
        name: "faint",
        size: (80, 16),
        format: PixelFormat::Yuv420(8),
        model_id: 0,
        log2_scale_factor: 15,
        seed: 123_456_789,
        comps: [
            Some(&[(0, 127, -32768, 2, 14), (128, 255, 32767, 14, 2)]),
            None,
            Some(&[(0, 255, 300, 7, 7)]),
        ],
        expect: Some([0x35e0_9a56, 0x48e5_b341, 0x8ad3_e24a]),
    },
    Case {
        name: "model1",
        size: (32, 32),
        format: PixelFormat::Yuv420(8),
        model_id: 1,
        log2_scale_factor: 3,
        seed: 0,
        comps: [Some(&[(0, 255, 50, 6, 6)]), None, None],
        expect: None,
    },
    Case {
        name: "yuv422",
        size: (32, 32),
        format: PixelFormat::Yuv422(8),
        model_id: 0,
        log2_scale_factor: 3,
        seed: 0,
        comps: [Some(&[(0, 255, 50, 6, 6)]), None, None],
        expect: None,
    },
];

/// Each input sample (`h274_expect.c`'s `sample`): a gradient, so that the
/// 8x8 blocks' means spread over 0 to 255, with some noise on it.
fn sample(c: usize, x: usize, y: usize) -> u8 {
    (((x + y) * 3 + c * 59 + (((x * 37) ^ (y * 101)) & 31)) & 0xFF) as u8
}

/// The case's input picture, and a frame of its size for the output.
fn frames(case: &Case) -> (FrameBuf<u8>, FrameBuf<u8>) {
    let shift_y = u32::from(case.format == PixelFormat::Yuv420(8));
    let make = || match AnyFrame::new(case.size.0, case.size.1, 1, shift_y, 8).unwrap() {
        AnyFrame::Eight(f) => f,
        AnyFrame::High(_) => unreachable!("8 bits"),
    };
    let mut input = make();
    for (c, plane) in input.planes.iter_mut().enumerate() {
        for y in 0..plane.height {
            for x in 0..plane.width {
                plane.data[y * plane.stride + x] = sample(c, x, y);
            }
        }
    }
    (input, make())
}

/// The case's model, as the SEI would carry it.
fn params(case: &Case) -> GrainParams {
    let mut model = FilmGrain {
        present: true,
        model_id: case.model_id,
        log2_scale_factor: case.log2_scale_factor,
        ..FilmGrain::default()
    };
    for (c, bands) in case.comps.iter().enumerate() {
        let Some(bands) = bands else { continue };
        model.comp_model_present_flag[c] = 1;
        model.num_intensity_intervals[c] = bands.len() as u16;
        model.num_model_values[c] = 3;
        for (i, &(lo, hi, v0, v1, v2)) in bands.iter().enumerate() {
            model.intensity_interval_lower_bound[c][i] = lo;
            model.intensity_interval_upper_bound[c][i] = hi;
            model.comp_model_value[c][i][..3].copy_from_slice(&[v0, v1, v2]);
        }
    }
    GrainParams {
        // FFmpeg's int seed widened into its uint64_t.
        seed: i64::from(case.seed) as u64,
        model,
    }
}

/// Plane `p`'s rows, unpadded, summed with Adler-32 (`av_adler32_update`).
fn adler(p: &Plane<u8>) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for y in 0..p.height {
        for &s in &p.data[y * p.stride..y * p.stride + p.width] {
            a = (a + u32::from(s)) % 65521;
            b = (b + a) % 65521;
        }
    }
    (b << 16) | a
}

#[test]
fn grain_is_ffmpegs() {
    for case in &CASES {
        let (input, mut out) = frames(case);
        let r = apply_film_grain(&mut out, &input, case.format, &params(case));
        match case.expect {
            Some(expect) => {
                assert_eq!(r, Ok(()), "{}", case.name);
                let got = [0, 1, 2].map(|c| adler(&out.planes[c]));
                assert_eq!(got, expect, "{}: each plane's Adler-32", case.name);
            }
            None => {
                assert_eq!(r, Err(Unsupported), "{}", case.name);
                assert!(
                    out.planes.iter().all(|p| p.data.iter().all(|&s| s == 0)),
                    "{}: a refused picture's frame is left as it was",
                    case.name
                );
            }
        }
    }
}

#[test]
fn a_component_without_grain_is_copied() {
    let case = &CASES[1];
    assert!(case.comps[2].is_none());
    let (input, mut out) = frames(case);
    apply_film_grain(&mut out, &input, case.format, &params(case)).unwrap();
    assert_eq!(out.planes[2], input.planes[2]);
}

#[test]
fn a_block_in_no_band_has_no_grain() {
    // "gaps" leaves brightness between its luma bands ungrained: some
    // blocks come out as they went in, and others do not.
    let case = &CASES[1];
    let (input, mut out) = frames(case);
    apply_film_grain(&mut out, &input, case.format, &params(case)).unwrap();
    let (i, o) = (&input.planes[0], &out.planes[0]);
    let unchanged = |bx: usize, by: usize| {
        (0..8).all(|y| {
            let at = (by * 8 + y) * i.stride + bx * 8;
            i.data[at..at + 8] == o.data[at..at + 8]
        })
    };
    let blocks: Vec<bool> = (0..i.height / 8)
        .flat_map(|by| (0..i.width / 8).map(move |bx| (bx, by)))
        .map(|(bx, by)| unchanged(bx, by))
        .collect();
    assert!(blocks.iter().any(|&u| u), "some block is in no band");
    assert!(blocks.iter().any(|&u| !u), "some block has grain");
}

#[test]
fn full_range_pictures_are_refused() {
    // FFmpeg's yuvj420p is not yuv420p: it refuses it as it refuses 4:2:2.
    let case = &CASES[0];
    let (input, mut out) = frames(case);
    let r = apply_film_grain(&mut out, &input, PixelFormat::Yuvj420, &params(case));
    assert_eq!(r, Err(Unsupported));
}

#[test]
fn a_pattern_is_made_once_and_kept() {
    let first: *const _ = pattern(3, 5);
    assert!(std::ptr::eq(first, pattern(3, 5)));
    assert!(!std::ptr::eq(first, pattern(5, 3)));
    // Every pattern's values are clipped to +-127.
    assert!(
        pattern(12, 12)
            .iter()
            .flatten()
            .all(|&v| (-127..=127).contains(&v))
    );
}
