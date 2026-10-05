//! Every way a picture file is read: the header questions, a whole decode, a
//! thumbnail, and each animated format's frames.
#![no_main]
use imagecodec::Limits;
use libfuzzer_sys::fuzz_target;

/// Small enough that a hostile header cannot make the fuzzer spend its time
/// filling a huge canvas, large enough for every format's real paths.
const LIMITS: Limits = Limits {
    max_pixels: 1 << 22,
    max_decompressed_bytes: 1 << 26,
};

fuzz_target!(|data: &[u8]| {
    let _ = imagecodec::dimensions(data);
    let _ = imagecodec::pixel_format(data);
    let _ = imagecodec::decode(data, LIMITS);
    let _ = imagecodec::decode_scaled(data, LIMITS, 48, 48);
    if let Ok(mut a) = imagecodec::gif::Animation::new(data, LIMITS) {
        for _ in 0..8 {
            if !matches!(a.next_frame(), Ok(Some(_))) {
                break;
            }
        }
    }
    if let Ok(mut a) = imagecodec::webp::Animation::new(data, LIMITS) {
        for _ in 0..8 {
            if !matches!(a.next_frame(), Ok(Some(_))) {
                break;
            }
        }
    }
    if let Ok(mut a) = imagecodec::avif::Animation::new(data, LIMITS) {
        for _ in 0..8 {
            if !matches!(a.next_frame(), Ok(Some(_))) {
                break;
            }
        }
    }
});
