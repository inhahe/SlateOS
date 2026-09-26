//! GIF, frame by frame, against answers this crate did not compute.
//!
//! Two kinds of fixture (`tests/data/generate_gif.py` says which is which):
//! files Pillow wrote, answered by Pillow's decode where Pillow and the
//! browsers agree; and files written block by block to exercise what ordinary
//! encoders never produce -- a dictionary that fills without a clear, clears
//! mid-string, images larger than their screen or partly off it, no colour
//! table, indices past it -- answered by the indices they were made from,
//! drawn by the generator's reference compositor. GIF is lossless, so every
//! comparison is exact.
//!
//! Four of the hand-written files are where Pillow and browsers disagree
//! (`gif_offscreen`, `gif_partial_first`, `gif_clear_opaque` and
//! `gif_restore_first`): the generator checks that Pillow really does decode
//! each of them differently, so each is a test of the browsers' answer rather
//! than a test that happens to pass either way.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

use imagecodec::gif::{Animation, Repeat};
use imagecodec::{ImageError, Limits, decode, decode_scaled, dimensions};

fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}.gif", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A fixture's frames as the answer file gives them.
struct Answer {
    width: u32,
    height: u32,
    /// Each frame's delay in hundredths of a second, and its pixels.
    frames: Vec<(u16, Vec<u32>)>,
}

fn answer(name: &str) -> Answer {
    let path = format!("{}/tests/data/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut words = text.split_whitespace();
    let mut number = || words.next().unwrap();
    let width: u32 = number().parse().unwrap();
    let height: u32 = number().parse().unwrap();
    let count: usize = number().parse().unwrap();
    let mut frames = Vec::new();
    for _ in 0..count {
        let delay: u16 = number().parse().unwrap();
        let pixels: Vec<u32> = (0..width * height)
            .map(|_| u32::from_str_radix(number(), 16).unwrap())
            .collect();
        frames.push((delay, pixels));
    }
    Answer {
        width,
        height,
        frames,
    }
}

/// Every fixture with an answer, and what it is for.
const FIXTURES: &[(&str, &str)] = &[
    ("gif_photo", "a photograph in 256 colours, interlaced"),
    ("gif_photo_flat", "the same, not interlaced"),
    ("gif_four", "four colours: the smallest code size"),
    (
        "gif_anim_clear",
        "an animation clearing each frame (disposal 2)",
    ),
    (
        "gif_anim_keep",
        "an animation keeping each frame (disposal 1)",
    ),
    (
        "gif_anim_restore",
        "an animation restoring what was under (disposal 3)",
    ),
    (
        "gif_deferred_clear",
        "a dictionary that fills and is not cleared",
    ),
    ("gif_clears", "a clear every 97 codes"),
    (
        "gif_interlaced_odd",
        "interlaced at a height no pass divides",
    ),
    ("gif_87a", "GIF87a"),
    ("gif_oversize", "a first image larger than its screen"),
    ("gif_no_palette", "no colour table at all"),
    ("gif_past_palette", "indices past their colour table"),
    ("gif_offscreen", "frames partly and wholly off the canvas"),
    ("gif_partial_first", "a first image smaller than its screen"),
    ("gif_clear_opaque", "disposal 2 with no transparent index"),
    (
        "gif_restore_first",
        "disposal 3 on the first frame, and after",
    ),
    (
        "gif_local_palettes",
        "local colour tables, transparency over a kept frame",
    ),
    (
        "gif_truncated_whole",
        "the untruncated twin of gif_truncated",
    ),
];

/// The first pixel two frames differ at, if any, for a message worth reading.
fn first_difference(got: &[u32], want: &[u32], width: u32) -> Option<String> {
    got.iter().zip(want).position(|(a, b)| a != b).map(|at| {
        format!(
            "({}, {}): {:08X} where the answer has {:08X}",
            at as u32 % width,
            at as u32 / width,
            got[at],
            want[at]
        )
    })
}

#[test]
fn every_frame_of_every_fixture_is_the_answer() {
    for (name, why) in FIXTURES {
        let bytes = read(name);
        let want = answer(name);
        let mut animation = Animation::new(&bytes, Limits::default())
            .unwrap_or_else(|e| panic!("{name} ({why}): {e}"));
        assert_eq!(
            animation.size(),
            (want.width, want.height),
            "{name} ({why})"
        );
        assert_eq!(animation.frame_count(), want.frames.len(), "{name} ({why})");
        for (n, (delay, pixels)) in want.frames.iter().enumerate() {
            let frame = animation
                .next_frame()
                .unwrap_or_else(|e| panic!("{name} frame {n}: {e}"))
                .unwrap_or_else(|| panic!("{name} ({why}): frame {n} is missing"));
            assert_eq!(frame.delay_cs, *delay, "{name} frame {n}: delay");
            assert_eq!(frame.image.pixels.len(), pixels.len());
            if let Some(at) = first_difference(&frame.image.pixels, pixels, want.width) {
                panic!("{name} ({why}), frame {n}: {at}");
            }
        }
        assert!(
            animation.next_frame().unwrap().is_none(),
            "{name}: a frame past the last"
        );
    }
}

#[test]
fn a_decode_is_the_first_frame_at_the_size_dimensions_reports() {
    for (name, why) in FIXTURES {
        let bytes = read(name);
        let want = answer(name);
        let image = decode(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            (image.width, image.height),
            (want.width, want.height),
            "{name}"
        );
        assert_eq!(
            dimensions(&bytes).unwrap(),
            (want.width, want.height),
            "{name}"
        );
        if let Some(at) = first_difference(&image.pixels, &want.frames[0].1, want.width) {
            panic!("{name} ({why}): {at}");
        }
    }
}

#[test]
fn a_rewound_animation_plays_the_same_frames_again() {
    for name in ["gif_anim_restore", "gif_restore_first", "gif_offscreen"] {
        let bytes = read(name);
        let mut animation = Animation::new(&bytes, Limits::default()).unwrap();
        let mut first = Vec::new();
        while let Some(frame) = animation.next_frame().unwrap() {
            first.push(frame.image.pixels.clone());
        }
        animation.rewind();
        let mut second = Vec::new();
        while let Some(frame) = animation.next_frame().unwrap() {
            second.push(frame.image.pixels.clone());
        }
        assert!(first == second, "{name}: the second play differs");
    }
}

#[test]
fn how_often_an_animation_plays_is_read_as_written() {
    let repeat = |name: &str| {
        Animation::new(&read(name), Limits::default())
            .unwrap()
            .repeat()
    };
    assert_eq!(repeat("gif_anim_clear"), Repeat::Forever, "loop=0");
    assert_eq!(repeat("gif_local_palettes"), Repeat::Count(3));
    assert_eq!(repeat("gif_photo"), Repeat::Once, "no NETSCAPE block");
    assert_eq!(repeat("gif_87a"), Repeat::Once);
}

#[test]
fn a_delay_too_short_to_see_is_shown_for_a_tenth_of_a_second() {
    // gif_anim_clear's frames ask for 10, 5, 0, 20, 1 and 7 hundredths.
    let bytes = read("gif_anim_clear");
    let mut animation = Animation::new(&bytes, Limits::default()).unwrap();
    let mut shown = Vec::new();
    while let Some(frame) = animation.next_frame().unwrap() {
        shown.push((frame.delay_cs, frame.display_delay_ms()));
    }
    assert_eq!(
        shown,
        [(10, 100), (5, 50), (0, 100), (20, 200), (1, 100), (7, 70)]
    );
}

#[test]
fn a_file_cut_off_mid_image_draws_what_arrived() {
    // What a browser shows of a GIF still downloading: the rows that came,
    // and nothing -- not garbage, not an error -- where the rest will be.
    let image = decode(&read("gif_truncated"), Limits::default()).unwrap();
    let whole = answer("gif_truncated_whole");
    assert_eq!((image.width, image.height), (whole.width, whole.height));
    let drawn = image.pixels.iter().take_while(|&&p| p != 0).count();
    assert!(drawn > 0, "nothing was drawn");
    assert!(
        drawn < image.pixels.len(),
        "the whole image was drawn from two thirds of it"
    );
    assert!(
        image.pixels[drawn..].iter().all(|&p| p == 0),
        "what was drawn is not a prefix"
    );
    assert_eq!(&image.pixels[..drawn], &whole.frames[0].1[..drawn]);
}

#[test]
fn a_thumbnail_is_the_first_frame_averaged_to_fit() {
    let bytes = read("gif_photo");
    let full = decode(&bytes, Limits::default()).unwrap();
    let thumb = decode_scaled(&bytes, Limits::default(), 30, 30).unwrap();
    // 97x61 into 30x30, the aspect ratio kept.
    assert_eq!((thumb.width, thumb.height), (30, 18));
    // An average keeps the picture's mean colour, near enough.
    let mean = |pixels: &[u32], shift: u32| {
        pixels
            .iter()
            .map(|p| f64::from((p >> shift) & 0xFF))
            .sum::<f64>()
            / pixels.len() as f64
    };
    for shift in [16, 8, 0] {
        let (a, b) = (mean(&full.pixels, shift), mean(&thumb.pixels, shift));
        assert!(
            (a - b).abs() < 3.0,
            "channel {shift}: {a:.1} against {b:.1}"
        );
    }
    // And a picture already small enough is its own thumbnail.
    let same = decode_scaled(&read("gif_87a"), Limits::default(), 64, 64).unwrap();
    assert_eq!((same.width, same.height), (5, 3));
}

#[test]
fn a_canvas_past_the_byte_budget_is_refused_before_it_exists() {
    let bytes = read("gif_deferred_clear");
    // 200x150: 120 000 bytes of canvas.
    let tight = Limits {
        max_decompressed_bytes: 100_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, tight),
        Err(ImageError::TooLarge { .. })
    ));
    let few_pixels = Limits {
        max_pixels: 10_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, few_pixels),
        Err(ImageError::TooLarge { .. })
    ));
}

#[test]
fn no_truncation_or_bit_flip_of_a_gif_panics() {
    for name in [
        "gif_anim_restore",
        "gif_restore_first",
        "gif_interlaced_odd",
        "gif_local_palettes",
    ] {
        let full = read(name);
        for cut in 0..full.len() {
            let _ = decode(&full[..cut], Limits::default());
            if let Ok(mut animation) = Animation::new(&full[..cut], Limits::default()) {
                while let Ok(Some(_)) = animation.next_frame() {}
            }
        }
        for at in (0..full.len() * 8).step_by(3) {
            let mut bent = full.clone();
            bent[at / 8] ^= 1 << (at % 8);
            let _ = decode(&bent, Limits::default());
            let _ = decode_scaled(&bent, Limits::default(), 7, 7);
            if let Ok(mut animation) = Animation::new(&bent, Limits::default()) {
                while let Ok(Some(_)) = animation.next_frame() {}
            }
        }
    }
}

#[test]
fn a_file_with_no_image_in_it_is_refused_by_name() {
    // A screen, a colour table and the trailer: nothing to show.
    let mut bytes = b"GIF89a".to_vec();
    bytes.extend_from_slice(&[2, 0, 2, 0, 0x80, 0, 0]);
    bytes.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
    bytes.push(0x3B);
    assert!(matches!(
        decode(&bytes, Limits::default()),
        Err(ImageError::Malformed(_))
    ));
    assert_eq!(dimensions(&bytes).unwrap(), (2, 2));
}
