//! What the JPEG suites share: reading a fixture, reading the reference
//! decoder's answer for it, and measuring how far a decode is from that answer.

use imagecodec::Image;

/// A fixture's bytes, from `tests/data/<name>.jpg`.
pub fn read(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/data/{name}.jpg", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Pillow's decode of a fixture, from `tests/data/<name>.txt`: width, height,
/// then `AARRGGBB` per pixel.
pub struct Answer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

pub fn answer(name: &str) -> Answer {
    let path = format!("{}/tests/data/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut words = text.split_whitespace();
    let width: u32 = words.next().unwrap().parse().unwrap();
    let height: u32 = words.next().unwrap().parse().unwrap();
    let pixels: Vec<u32> = words.map(|w| u32::from_str_radix(w, 16).unwrap()).collect();
    assert_eq!(
        pixels.len(),
        (width * height) as usize,
        "{path}: wrong count"
    );
    Answer {
        width,
        height,
        pixels,
    }
}

/// The most a channel of a correct decode may differ from the reference's.
///
/// Two correct decoders differ, and the specification allows it: each rounds
/// its inverse DCT its own way, so a luma sample and a chroma sample may each
/// come out a level apart, and colour conversion multiplies the chroma by up to
/// 1.772 before both round once more. One plus 1.772 rounds to 3, and among
/// the fixtures a 3 turns up in a few channels in ten thousand. A decoder that
/// decodes differently -- bringing colour back up by repeating samples where
/// libjpeg filters them, say -- is off by tens.
pub const WORST: i32 = 3;

/// The most the mean difference may be: rounding goes both ways and averages
/// out, where a defect does not. Every fixture big enough to be judged by it
/// is under half of this.
pub const MEAN: f64 = 0.10;

/// Fewer pixels than this are too few for a mean to test for bias: over the
/// 36 pixels of the narrow fixtures, four of them two levels apart are already
/// a mean of 0.15. Those are there for the upsampling filter's edge, which
/// [`WORST`] holds them to with room to spare.
pub const FEW: usize = 1000;

/// Assert `image`, decoded from fixture `name`, is the reference's `answer` to
/// within rounding.
pub fn assert_agrees(name: &str, image: &Image, answer: &Answer) {
    let (worst, mean) = distance(name, image, answer);
    assert!(
        worst <= WORST,
        "{name}: a channel differs by {worst}, which is a decode and not a rounding"
    );
    if image.pixels.len() >= FEW {
        assert!(
            mean < MEAN,
            "{name}: mean channel error {mean:.3} is a bias, not rounding"
        );
    }
}

/// How far a decode is from the reference's: the worst difference in any one
/// channel of any one pixel, and the mean over every channel of every pixel.
pub fn distance(name: &str, image: &Image, answer: &Answer) -> (i32, f64) {
    assert_eq!(
        (image.width, image.height),
        (answer.width, answer.height),
        "{name}: a different size"
    );
    let mut worst = 0;
    let mut total = 0i64;
    for (index, (got, want)) in image.pixels.iter().zip(&answer.pixels).enumerate() {
        assert_eq!(got >> 24, 0xFF, "{name}: pixel {index} is not opaque");
        for shift in [16u32, 8, 0] {
            let mine = ((got >> shift) & 0xFF) as i32;
            let theirs = ((want >> shift) & 0xFF) as i32;
            let difference = (mine - theirs).abs();
            worst = worst.max(difference);
            total += i64::from(difference);
        }
    }
    (worst, total as f64 / (image.pixels.len() * 3) as f64)
}
