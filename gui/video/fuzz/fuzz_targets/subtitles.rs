//! A film's subtitles as the player opens them: Matroska's and WebM's text
//! tracks, and the SubRip, ASS, SSA and WebVTT readers behind them -- markup,
//! override blocks, script headers and cue settings from a stranger.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;
use videocodec::Subtitles;

fuzz_target!(|data: &[u8]| {
    let Ok(mut subtitles) = Subtitles::open(Cursor::new(data)) else {
        return;
    };
    for _ in 0..256 {
        if !matches!(subtitles.next_cue(), Ok(Some(_))) {
            break;
        }
    }
    let _ = subtitles.seek(0);
    let _ = subtitles.next_cue();
    let _ = subtitles.seek(30_000_000_000);
    let _ = subtitles.next_cue();
});
