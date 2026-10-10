//! A file's sound as the player opens it: Ogg, Matroska, MP4, FLAC and MP3
//! containers, and the Opus, Vorbis, FLAC and MP3 decoders behind them.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
use videocodec::Sound;

fuzz_target!(|data: &[u8]| {
    let Ok(mut sound) = Sound::open(Cursor::new(data)) else {
        return;
    };
    for _ in 0..48 {
        if !matches!(sound.next_block(), Ok(Some(_))) {
            break;
        }
    }
    let _ = sound.seek(0);
    let _ = sound.next_block();
    let _ = sound.seek(1_000_000_000);
    let _ = sound.next_block();
});
