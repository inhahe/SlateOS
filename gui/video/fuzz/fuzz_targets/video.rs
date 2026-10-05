//! A video file as the player opens it: the container read, a few frames
//! decoded through whichever codec the file names, and seeks either way.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
use videocodec::{Limits, SeekMode, Video};

fuzz_target!(|data: &[u8]| {
    let limits = Limits {
        max_pixels: 1 << 20,
    };
    let Ok(mut video) = Video::open_with(Cursor::new(data), None, limits) else {
        return;
    };
    for _ in 0..6 {
        if !matches!(video.next_frame(), Ok(Some(_))) {
            break;
        }
    }
    let _ = video.seek(0, SeekMode::KeyFrame);
    let _ = video.next_frame();
    let _ = video.seek(1_000_000, SeekMode::Exact);
    let _ = video.next_frame();
    let _ = video.seek(i64::MAX / 4, SeekMode::KeyFrame);
    let _ = video.next_picture();
});
