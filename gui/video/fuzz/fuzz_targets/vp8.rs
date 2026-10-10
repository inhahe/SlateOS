//! The VP8 decoder on raw packets, several in a row so that frames refer to
//! the ones before them. The input is a run of packets, each a 32-bit
//! little-endian length and then that many bytes (the last cut short by the
//! end of the input).
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut decoder = vp8::Decoder::with_max_pixels(1 << 20);
    let mut rest = data;
    for _ in 0..8 {
        let Some((len, tail)) = rest.split_first_chunk::<4>() else {
            break;
        };
        let take = usize::try_from(u32::from_le_bytes(*len))
            .unwrap_or(usize::MAX)
            .min(tail.len());
        let (packet, after) = tail.split_at(take);
        let _ = decoder.decode(packet);
        rest = after;
    }
});
