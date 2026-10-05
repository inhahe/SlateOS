//! The VP9 decoder on raw packets, several in a row so that frames refer to
//! the ones before them, on two threads so the tile and loop-filter workers
//! run too. Packets are framed as in `vp8.rs`.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut decoder = vp9::Decoder::with_max_pixels(1 << 20);
    decoder.set_threads(2);
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
