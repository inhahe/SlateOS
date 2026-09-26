//! WebP against libwebp, through Pillow.
//!
//! Every comparison here is to the bit. Lossless WebP stores its encoder's
//! input exactly, alpha and the colour under transparent pixels included; lossy
//! WebP is a VP8 frame, whose decoding is specified to the bit, converted to RGB
//! by libwebp's own arithmetic. The fixtures (`tests/data/generate_webp.py`) are
//! chosen so that between them they use every tool each format has -- unit
//! tests in `webp/lossless.rs` and `webp/lossy.rs` check that they still do.
//!
//! Animations are held to libwebp's animation decoder the same way, frame by
//! frame: its compositing is integer arithmetic, so it too is exact, and a
//! unit test in `webp.rs` checks that the fixtures between them reach each of
//! its rules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use imagecodec::webp::{Animation, Repeat};
use imagecodec::{ImageError, Limits, decode, decode_scaled, dimensions};

fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}.webp", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Pillow's decode: width, height, pixels.
fn answer(name: &str) -> (u32, u32, Vec<u32>) {
    let path = format!("{}/tests/data/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut words = text.split_whitespace();
    let width: u32 = words.next().unwrap().parse().unwrap();
    let height: u32 = words.next().unwrap().parse().unwrap();
    let pixels: Vec<u32> = words.map(|w| u32::from_str_radix(w, 16).unwrap()).collect();
    assert_eq!(pixels.len(), (width * height) as usize, "{path}");
    (width, height, pixels)
}

const LOSSLESS: &[(&str, &str)] = &[
    ("webp_lossless_photo", "a photograph at the most effort"),
    ("webp_lossless_fast", "the same at the least"),
    ("webp_lossless_big", "larger, with meta prefix codes"),
    (
        "webp_lossless_alpha",
        "alpha, and colour kept under clear pixels",
    ),
    ("webp_lossless_2c", "two colours: eight indices a byte"),
    ("webp_lossless_4c", "four colours: four a byte"),
    ("webp_lossless_13c", "thirteen colours: two a byte"),
    (
        "webp_lossless_100c",
        "a hundred colours: a palette, unbundled",
    ),
    ("webp_lossless_1x1", "one pixel"),
    ("webp_lossless_column", "one pixel wide"),
    ("webp_lossless_row", "one pixel high"),
    (
        "webp_lossless_extended",
        "the extended container, behind a VP8X",
    ),
];

#[test]
fn every_lossless_fixture_decodes_to_exactly_what_libwebp_does() {
    for (name, why) in LOSSLESS {
        let bytes = read(name);
        let (width, height, want) = answer(name);
        let image = decode(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((image.width, image.height), (width, height), "{name}");
        assert_eq!(dimensions(&bytes).unwrap(), (width, height), "{name}");
        if let Some(at) = image.pixels.iter().zip(&want).position(|(a, b)| a != b) {
            panic!(
                "{name} ({why}): pixel ({}, {}) is {:08X} where libwebp gives {:08X}",
                at as u32 % width,
                at as u32 / width,
                image.pixels[at],
                want[at]
            );
        }
    }
}

/// A lossy fixture's answer: Pillow's decode, kept as a PNG.
fn png_answer(name: &str) -> imagecodec::Image {
    let path = format!("{}/tests/data/{name}.png", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    decode(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{path}: {e}"))
}

const LOSSY: &[(&str, &str)] = &[
    (
        "webp_lossy_photo",
        "libwebp's defaults: four segments, the normal filter",
    ),
    (
        "webp_lossy_q100",
        "the finest quantiser, and the largest coefficients",
    ),
    (
        "webp_lossy_q5",
        "the coarsest quantiser, and strong filtering",
    ),
    ("webp_lossy_noise", "noise: every subblock mode"),
    ("webp_lossy_big", "many rows and columns of macroblocks"),
    ("webp_lossy_odd", "odd sizes: chroma's last column and row"),
    (
        "webp_lossy_even",
        "even sizes that are not whole macroblocks",
    ),
    ("webp_lossy_1x1", "one pixel"),
    ("webp_lossy_column", "one pixel wide"),
    ("webp_lossy_row", "one pixel high"),
    ("webp_lossy_alpha", "an alpha plane, compressed"),
    (
        "webp_lossy_alpha_levels",
        "an alpha plane the encoder quantised",
    ),
    ("webp_lossy_alpha_raw", "an alpha plane stored raw"),
    ("webp_lossy_simple", "the simple loop filter"),
    ("webp_lossy_unfiltered", "no loop filter"),
    ("webp_lossy_sharp", "the loop filter at its sharpest"),
    ("webp_lossy_one_segment", "no segments"),
    ("webp_lossy_partitions", "eight coefficient partitions"),
    ("webp_lossy_skip", "macroblocks coded as empty"),
    (
        "webp_lossy_segment_deltas",
        "segments by delta, and filter levels clamped once as libwebp does",
    ),
    ("webp_lossy_segments_unmapped", "segments with no map"),
    (
        "webp_lossy_segments_unvalued",
        "segments with no values: libwebp's absolute zeros",
    ),
    ("webp_lossy_strong", "filter level 63, with deltas"),
    ("webp_lossy_quant_deltas", "every quantiser delta"),
    (
        "webp_lossy_colour_space",
        "the colour-space bits, which WebP ignores",
    ),
    (
        "webp_lossy_libvpx",
        "libvpx's frame: its filter deltas, four partitions",
    ),
    (
        "webp_lossy_corrupt_coder",
        "corrupt: a coder pushed past its range, read as libwebp reads it",
    ),
    (
        "webp_lossy_short_by_padding",
        "cut short, and finished by the chunk's padding byte as in libwebp",
    ),
    (
        "webp_lossless_alpha_unhinted",
        "alpha, but a header that says none: shown opaque",
    ),
    (
        "webp_lossy_alpha_unflagged",
        "an alpha chunk the VP8X flags disown: dropped",
    ),
    (
        "webp_lossy_alpha_corrupt_tail",
        "corrupt alpha whose last symbol reads libwebp's stale window",
    ),
    (
        "webp_lossless_stray_symbol",
        "corrupt: a prefix code naming a symbol past its alphabet",
    ),
    ("webp_alpha_raw_none", "a raw alpha plane"),
    ("webp_alpha_raw_horizontal", "raw, filtered from the left"),
    ("webp_alpha_raw_vertical", "raw, filtered from above"),
    ("webp_alpha_raw_gradient", "raw, filtered by gradient"),
    ("webp_alpha_lossless_none", "a compressed alpha plane"),
    (
        "webp_alpha_lossless_horizontal",
        "compressed, filtered from the left",
    ),
    (
        "webp_alpha_lossless_vertical",
        "compressed, filtered from above",
    ),
    (
        "webp_alpha_lossless_gradient",
        "compressed, filtered by gradient",
    ),
];

#[test]
fn every_lossy_fixture_decodes_to_exactly_what_libwebp_does() {
    for (name, why) in LOSSY {
        let bytes = read(name);
        let want = png_answer(name);
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
        if let Some(at) = image
            .pixels
            .iter()
            .zip(&want.pixels)
            .position(|(a, b)| a != b)
        {
            panic!(
                "{name} ({why}): pixel ({}, {}) is {:08X} where libwebp gives {:08X}",
                at as u32 % want.width,
                at as u32 / want.width,
                image.pixels[at],
                want.pixels[at]
            );
        }
    }
}

/// `bytes`'s first chunk replaced by the first `keep` bytes of its payload,
/// with the chunk and RIFF sizes made to match: a file that is complete as a
/// container and cut short inside its bitstream.
fn cut_inside_the_bitstream(bytes: &[u8], keep: usize) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    let body = 4 + 8 + keep + (keep & 1);
    out.extend_from_slice(&(body as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&bytes[12..16]);
    out.extend_from_slice(&(keep as u32).to_le_bytes());
    out.extend_from_slice(&bytes[20..20 + keep]);
    if keep & 1 == 1 {
        out.push(0);
    }
    out
}

#[test]
fn a_lossy_webp_whose_coefficients_run_out_is_truncated() {
    let full = read("webp_lossy_photo");
    let payload = u32::from_le_bytes(full[16..20].try_into().unwrap()) as usize;
    // Past the headers, the modes and the start of the coefficients: the
    // frame reads until its one partition runs dry, as libwebp does.
    for keep in [payload / 2, payload * 3 / 4, payload - 8] {
        let cut = cut_inside_the_bitstream(&full, keep);
        assert_eq!(
            decode(&cut, Limits::default()),
            Err(ImageError::Truncated),
            "cut to {keep} of {payload}"
        );
    }
    // Before its first partition ends it is refused as a short partition.
    let cut = cut_inside_the_bitstream(&full, 30);
    assert!(decode(&cut, Limits::default()).is_err());
}

#[test]
fn no_truncation_or_bit_flip_of_a_lossy_webp_panics() {
    for (name, cuts, flips) in [
        ("webp_lossy_odd", 1, 3),
        ("webp_lossy_skip", 1, 5),
        ("webp_alpha_lossless_gradient", 3, 11),
        ("webp_lossy_partitions", 17, 97),
        ("webp_lossy_segment_deltas", 7, 41),
    ] {
        let full = read(name);
        for cut in (0..full.len()).step_by(cuts) {
            let _ = decode(&full[..cut], Limits::default());
        }
        for at in (0..full.len() * 8).step_by(flips) {
            let mut bent = full.clone();
            bent[at / 8] ^= 1 << (at % 8);
            let _ = decode(&bent, Limits::default());
        }
    }
}

#[test]
fn a_lossy_picture_past_the_limits_is_refused_before_it_is_decoded() {
    let bytes = read("webp_lossy_big");
    let few = Limits {
        max_pixels: 10_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, few),
        Err(ImageError::TooLarge { .. })
    ));
    let tight = Limits {
        max_decompressed_bytes: 50_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, tight),
        Err(ImageError::TooLarge { .. })
    ));
}

#[test]
fn a_thumbnail_of_a_webp_fits_its_box() {
    let bytes = read("webp_lossless_big");
    let thumb = decode_scaled(&bytes, Limits::default(), 50, 50).unwrap();
    assert_eq!((thumb.width, thumb.height), (50, 36), "203x149 into 50x50");
    let same = decode_scaled(&read("webp_lossless_1x1"), Limits::default(), 50, 50).unwrap();
    assert_eq!((same.width, same.height), (1, 1), "no pixels invented");
    let lossy = decode_scaled(&read("webp_lossy_big"), Limits::default(), 40, 40).unwrap();
    assert_eq!((lossy.width, lossy.height), (40, 29), "131x97 into 40x40");
}

#[test]
fn a_picture_past_the_limits_is_refused_before_it_is_decoded() {
    let bytes = read("webp_lossless_big");
    let few = Limits {
        max_pixels: 10_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, few),
        Err(ImageError::TooLarge { .. })
    ));
    let tight = Limits {
        max_decompressed_bytes: 50_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode(&bytes, tight),
        Err(ImageError::TooLarge { .. })
    ));
}

#[test]
fn no_truncation_or_bit_flip_of_a_lossless_webp_panics() {
    // (file, stride through its prefixes, stride through its bits): every
    // prefix and every third bit of the small ones, a spread of the large one,
    // so the suite stays quick in a debug build.
    for (name, cuts, flips) in [
        ("webp_lossless_4c", 1, 3),
        ("webp_lossless_2c", 1, 3),
        ("webp_lossless_alpha", 5, 37),
        ("webp_lossless_photo", 11, 101),
    ] {
        let full = read(name);
        for cut in (0..full.len()).step_by(cuts) {
            let _ = decode(&full[..cut], Limits::default());
        }
        for at in (0..full.len() * 8).step_by(flips) {
            let mut bent = full.clone();
            bent[at / 8] ^= 1 << (at % 8);
            let _ = decode(&bent, Limits::default());
        }
    }
}

#[test]
fn a_cut_short_lossless_webp_is_truncated_not_a_different_picture() {
    let full = read("webp_lossless_photo");
    // Past the header, before the end: whatever the decoder makes of it, it
    // must not be a picture that passes for the whole one.
    let cut = &full[..full.len() / 2];
    match decode(cut, Limits::default()) {
        Err(ImageError::Truncated | ImageError::Malformed(_)) => {}
        other => panic!("half a file decoded as {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Animations
// ---------------------------------------------------------------------------

/// An animated fixture's answer: Pillow's frames, and what it says of them.
struct Played {
    width: u32,
    height: u32,
    loop_count: u16,
    /// Whether Pillow shows it with its alpha (mode `RGBA`, not `RGB`).
    alpha: bool,
    durations: Vec<u32>,
    /// Each frame's canvas, top to bottom.
    strip: imagecodec::Image,
}

fn played(name: &str) -> Played {
    let path = format!("{}/tests/data/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let words: Vec<&str> = text.split_whitespace().collect();
    let frames: usize = words[4].parse().unwrap();
    let answer = Played {
        width: words[0].parse().unwrap(),
        height: words[1].parse().unwrap(),
        loop_count: words[2].parse().unwrap(),
        alpha: match words[3] {
            "RGBA" => true,
            "RGB" => false,
            other => panic!("{path}: mode {other}"),
        },
        durations: words[5..].iter().map(|w| w.parse().unwrap()).collect(),
        strip: png_answer(name),
    };
    assert_eq!(answer.durations.len(), frames, "{path}");
    assert_eq!(answer.strip.height, answer.height * frames as u32, "{path}");
    answer
}

impl Played {
    fn frame(&self, i: usize) -> &[u32] {
        let size = (self.width * self.height) as usize;
        &self.strip.pixels[i * size..(i + 1) * size]
    }
}

const ANIMATED: &[(&str, &str)] = &[
    (
        "webp_anim_lossless",
        "libwebp's encoder, lossless: rectangles that changed, mostly not blended",
    ),
    ("webp_anim_lossy", "the same, lossy, alpha in ALPH chunks"),
    (
        "webp_anim_mixed",
        "lossy frames then lossless ones, blended and not",
    ),
    (
        "webp_anim_clear",
        "on a clear canvas, each frame cleared after it, key frames forced often",
    ),
    (
        "webp_anim_opaque",
        "opaque, so marked as having no alpha, and shown without it",
    ),
    (
        "webp_anim_rules",
        "by hand: each reason a frame is a key frame; blending beside a cleared rectangle; clear pixels not blended",
    ),
    (
        "webp_anim_key_chain",
        "by hand: a first frame short of the canvas, and the key frames that follow from it",
    ),
    (
        "webp_anim_unflagged",
        "by hand: frames with alpha under a VP8X chunk that says there is none",
    ),
    (
        "webp_anim_container",
        "by hand: an ANMF chunk's size overruled by its picture's, a chunk after the picture inside it, metadata between frames",
    ),
];

fn first_difference(ours: &[u32], want: &[u32], width: u32) -> Option<String> {
    let at = ours.iter().zip(want).position(|(a, b)| a != b)?;
    Some(format!(
        "pixel ({}, {}) is {:08X} where libwebp gives {:08X}",
        at as u32 % width,
        at as u32 / width,
        ours[at],
        want[at]
    ))
}

#[test]
fn every_animation_plays_exactly_as_libwebp_composites_it() {
    for (name, why) in ANIMATED {
        let bytes = read(name);
        let want = played(name);
        let mut animation = Animation::new(&bytes, Limits::default())
            .unwrap_or_else(|e| panic!("{name} ({why}): {e}"));
        assert_eq!(animation.size(), (want.width, want.height), "{name}");
        assert_eq!(animation.frame_count(), want.durations.len(), "{name}");
        assert_eq!(animation.has_alpha(), want.alpha, "{name}");
        let repeat = if want.loop_count == 0 {
            Repeat::Forever
        } else {
            Repeat::Times(want.loop_count)
        };
        assert_eq!(animation.repeat(), repeat, "{name}");
        for (i, &duration) in want.durations.iter().enumerate() {
            let frame = animation
                .next_frame()
                .unwrap_or_else(|e| panic!("{name} frame {i}: {e}"))
                .unwrap_or_else(|| panic!("{name}: no frame {i}"));
            assert_eq!(frame.duration_ms, duration, "{name} frame {i}");
            assert_eq!(
                (frame.image.width, frame.image.height),
                (want.width, want.height)
            );
            if let Some(what) = first_difference(&frame.image.pixels, want.frame(i), want.width) {
                panic!("{name} ({why}), frame {i}: {what}");
            }
        }
        assert!(
            animation.next_frame().unwrap().is_none(),
            "{name}: a frame past the last"
        );

        // A still decode is the first frame, which is what Pillow opens to.
        let first = decode(&bytes, Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((first.width, first.height), (want.width, want.height));
        if let Some(what) = first_difference(&first.pixels, want.frame(0), want.width) {
            panic!("{name} decoded still: {what}");
        }
        assert_eq!(dimensions(&bytes).unwrap(), (want.width, want.height));
    }
}

#[test]
fn a_rewound_animation_plays_the_same_frames_again() {
    for (name, _) in ANIMATED {
        let bytes = read(name);
        let mut animation = Animation::new(&bytes, Limits::default()).unwrap();
        let mut first = Vec::new();
        while let Some(frame) = animation.next_frame().unwrap() {
            first.push((frame.image.pixels.clone(), frame.duration_ms));
        }
        animation.rewind();
        let mut second = Vec::new();
        while let Some(frame) = animation.next_frame().unwrap() {
            second.push((frame.image.pixels.clone(), frame.duration_ms));
        }
        assert_eq!(first, second, "{name}");
        // And the canvas given up is the last frame shown.
        assert_eq!(animation.into_canvas().pixels, first.last().unwrap().0);
    }
}

#[test]
fn a_still_webp_is_an_animation_of_one_frame() {
    for name in [
        "webp_lossless_alpha",
        "webp_lossy_alpha",
        "webp_lossless_alpha_unhinted",
    ] {
        let bytes = read(name);
        let still = decode(&bytes, Limits::default()).unwrap();
        let mut animation = Animation::new(&bytes, Limits::default()).unwrap();
        assert_eq!(animation.frame_count(), 1, "{name}");
        assert_eq!(animation.repeat(), Repeat::Times(1), "{name}");
        let frame = animation.next_frame().unwrap().unwrap();
        assert_eq!(frame.image, &still, "{name}");
        assert_eq!(frame.duration_ms, 0, "{name}");
        assert!(animation.next_frame().unwrap().is_none(), "{name}");
    }
}

/// A WebP's chunks after the file header: fourcc and payload.
fn riff_chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut at = 12;
    let mut out = Vec::new();
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        out.push((
            bytes[at..at + 4].try_into().unwrap(),
            bytes[at + 8..at + 8 + size].to_vec(),
        ));
        at += 8 + size + (size & 1);
    }
    out
}

/// A WebP of `chunks`, sizes and padding made to match.
fn riff_file(chunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut body = b"WEBP".to_vec();
    for (tag, payload) in chunks {
        body.extend_from_slice(tag);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
        if payload.len() & 1 == 1 {
            body.push(0);
        }
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

#[test]
fn a_frame_that_will_not_decode_leaves_the_canvas_and_fails_again() {
    // `webp_anim_rules` with its third frame's lossless stream cut to its
    // header and a byte: the file is sound, the frame is not.
    let mut chunks = riff_chunks(&read("webp_anim_rules"));
    let third = chunks
        .iter_mut()
        .filter(|(tag, _)| tag == b"ANMF")
        .nth(2)
        .unwrap();
    let inner = riff_chunks(&[&[0u8; 12][..], &third.1[16..]].concat());
    assert_eq!(&inner[0].0, b"VP8L");
    let cut = [(inner[0].0, inner[0].1[..6].to_vec())];
    let mut payload = third.1[..16].to_vec();
    payload.extend_from_slice(&riff_file(&cut)[12..]);
    third.1 = payload;
    let bytes = riff_file(&chunks);

    let want = played("webp_anim_rules");
    let mut animation = Animation::new(&bytes, Limits::default()).unwrap();
    for i in 0..2 {
        let frame = animation.next_frame().unwrap().unwrap();
        assert_eq!(frame.image.pixels, want.frame(i), "frame {i}");
    }
    assert!(animation.next_frame().is_err(), "the broken frame");
    assert!(animation.next_frame().is_err(), "asked again");
    assert_eq!(
        animation.into_canvas().pixels,
        want.frame(1),
        "the canvas as the last good frame left it"
    );
}

#[test]
fn an_animation_past_the_limits_is_refused_before_anything_is_decoded() {
    let bytes = read("webp_anim_lossless");
    let canvas: u64 = 40 * 26;
    let few = Limits {
        max_pixels: canvas - 1,
        ..Limits::default()
    };
    assert!(matches!(
        Animation::new(&bytes, few),
        Err(ImageError::TooLarge { .. })
    ));
    assert!(matches!(
        decode(&bytes, few),
        Err(ImageError::TooLarge { .. })
    ));
    let exact = Limits {
        max_pixels: canvas,
        ..Limits::default()
    };
    assert!(Animation::new(&bytes, exact).is_ok());
    assert!(decode(&bytes, exact).is_ok());
    // Playing takes two canvases: the one shown, and the one the next frame
    // is drawn over.
    let two = Limits {
        max_decompressed_bytes: (2 * 4 * canvas) as usize,
        ..Limits::default()
    };
    assert!(Animation::new(&bytes, two).is_ok());
    let short = Limits {
        max_decompressed_bytes: (2 * 4 * canvas - 1) as usize,
        ..Limits::default()
    };
    assert!(matches!(
        Animation::new(&bytes, short),
        Err(ImageError::TooLarge { .. })
    ));
    // One shown without its alpha takes a third, to show it in.
    let opaque = read("webp_anim_opaque");
    let three = Limits {
        max_decompressed_bytes: 3 * 4 * 37 * 23,
        ..Limits::default()
    };
    assert!(Animation::new(&opaque, three).is_ok());
    let not_three = Limits {
        max_decompressed_bytes: 3 * 4 * 37 * 23 - 1,
        ..Limits::default()
    };
    assert!(matches!(
        Animation::new(&opaque, not_three),
        Err(ImageError::TooLarge { .. })
    ));
}

#[test]
fn a_thumbnail_of_an_animation_is_its_first_frame() {
    let bytes = read("webp_anim_lossy");
    let thumb = decode_scaled(&bytes, Limits::default(), 20, 20).unwrap();
    assert_eq!((thumb.width, thumb.height), (20, 13), "40x26 into 20x20");
    let first = decode(&bytes, Limits::default()).unwrap();
    let scaled = decode_scaled(&bytes, Limits::default(), 40, 40).unwrap();
    assert_eq!(scaled, first, "no pixels invented");
}

#[test]
fn no_truncation_or_bit_flip_of_an_animation_panics() {
    for (name, cuts, flips) in [
        ("webp_anim_rules", 3, 13),
        ("webp_anim_key_chain", 1, 5),
        ("webp_anim_container", 1, 3),
        ("webp_anim_lossy", 7, 29),
    ] {
        let full = read(name);
        let play = |bytes: &[u8]| {
            let _ = decode(bytes, Limits::default());
            if let Ok(mut animation) = Animation::new(bytes, Limits::default()) {
                while let Ok(Some(_)) = animation.next_frame() {}
            }
        };
        for cut in (0..full.len()).step_by(cuts) {
            play(&full[..cut]);
        }
        for at in (0..full.len() * 8).step_by(flips) {
            let mut bent = full.clone();
            bent[at / 8] ^= 1 << (at % 8);
            play(&bent);
        }
    }
}
