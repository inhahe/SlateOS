//! The MP4 demuxer on its own: the boxes and sample tables, the edit lists
//! and fragments that rewrite them, the index they make, and packets read
//! from it -- all tracks, then one alone, through a small read-ahead -- and
//! seeks back and forth. No decoder runs, so the fuzzer's time goes to the
//! tables, where a stranger's file says how many of everything there are.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let Ok(mut demuxer) = mp4::Demuxer::open(Cursor::new(data)) else {
        return;
    };
    let tracks = demuxer.tracks().len();
    for _ in 0..256 {
        if !matches!(demuxer.next_packet(), Ok(Some(_))) {
            break;
        }
    }
    for track in 0..tracks.min(4) {
        let _ = demuxer.seek(track, 0);
        let _ = demuxer.next_packet();
        let _ = demuxer.seek(track, 1 << 20);
        let _ = demuxer.next_packet();
        let _ = demuxer.seek_forward(track, i64::MAX / 4);
        let _ = demuxer.next_packet();
        let _ = demuxer.seek(track, -1);
        let _ = demuxer.next_packet();
    }
    if tracks > 0 {
        demuxer.select_tracks(Some(&[tracks - 1]));
        let _ = demuxer.set_read_ahead(64);
        let _ = demuxer.seek(tracks - 1, 0);
        for _ in 0..64 {
            if !matches!(demuxer.next_packet(), Ok(Some(_))) {
                break;
            }
        }
    }
});
