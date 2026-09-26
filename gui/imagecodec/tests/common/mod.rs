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

/// Assert `image`, decoded from fixture `name`, is the reference's `answer`
/// exactly.
///
/// This allowed three levels a channel once -- the rounding two decoders'
/// inverse DCTs and colour conversions differ by -- and needed it. The decoder
/// is now a port of libjpeg-turbo's own arithmetic, the decoder every answer
/// here comes from, so any difference at all is a defect.
pub fn assert_exact(name: &str, image: &Image, answer: &Answer) {
    assert_eq!(
        (image.width, image.height),
        (answer.width, answer.height),
        "{name}: a different size"
    );
    for (index, (got, want)) in image.pixels.iter().zip(&answer.pixels).enumerate() {
        assert_eq!(
            got,
            want,
            "{name}: pixel {index} ({}, {})",
            index % image.width as usize,
            index / image.width as usize
        );
    }
}
