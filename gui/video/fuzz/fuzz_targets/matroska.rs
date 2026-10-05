//! The Matroska demuxer on its own: the description, the metadata a probe
//! shows, attachments read when asked for, packets, and seeks.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let Ok(mut demuxer) = matroska::Demuxer::open(Cursor::new(data)) else {
        return;
    };
    let _ = demuxer.metadata().len();
    let _ = demuxer.chapters().len();
    let _ = demuxer.chapter_ends(Some(0));
    let _ = demuxer.chapter_ends(None);
    let attachments = demuxer.attachments().len();
    for i in 0..attachments.min(4) {
        let _ = demuxer.attachment_data(i);
    }
    let track = demuxer.tracks().first().map(|t| t.number);
    for _ in 0..64 {
        if !matches!(demuxer.next_packet(), Ok(Some(_))) {
            break;
        }
    }
    if let Some(track) = track {
        let _ = demuxer.seek(track, 0);
        let _ = demuxer.next_packet();
        let _ = demuxer.seek(track, 5_000_000_000);
        let _ = demuxer.next_packet();
        let _ = demuxer.seek(track, i64::MAX / 4);
        let _ = demuxer.next_packet();
    }
});
