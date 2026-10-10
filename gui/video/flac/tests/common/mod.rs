//! What the reference tests share: the crate's reading of a file, in the
//! lines `tools/reference.c` prints of libFLAC's, and the comparison.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_panics_doc,
    reason = "test support: a panic is a failed test"
)]

use std::fs::File;

use flac::{Block, Event, Reader, Status};

/// The reference's name for an error.
fn status(s: Status) -> &'static str {
    match s {
        Status::LostSync => "lost-sync",
        Status::BadHeader => "bad-header",
        Status::CrcMismatch => "crc-mismatch",
        Status::Unparseable => "unparseable",
        Status::BadMetadata => "bad-metadata",
        Status::OutOfBounds => "out-of-bounds",
        Status::MissingFrame => "missing-frame",
    }
}

/// The log's lines from `from` on: blocks (with STREAMINFO's facts) and
/// errors.
fn log_lines(r: &Reader<File>, from: &mut usize, blocks: &mut usize, out: &mut Vec<String>) {
    for e in &r.log()[*from..] {
        match *e {
            Event::Metadata { kind, length } => {
                out.push(format!("meta {kind} {length}"));
                if let Some(Block::StreamInfo(s)) = r.metadata().blocks.get(*blocks) {
                    out.push(format!(
                        "info {} {} {} {} {} {} {} {}",
                        s.min_block_size,
                        s.max_block_size,
                        s.min_frame_size,
                        s.max_frame_size,
                        s.sample_rate,
                        s.channels,
                        s.bits_per_sample,
                        s.total_samples
                    ));
                }
                *blocks += 1;
            }
            Event::Error(s) => out.push(format!("error {}", status(s))),
        }
    }
    *from = r.log().len();
}

/// Frames to the end, each after the log's lines before it.
fn frames(r: &mut Reader<File>, from: &mut usize, blocks: &mut usize, out: &mut Vec<String>) {
    loop {
        let mut pending = Vec::new();
        let Some(frame) = r.next_frame().unwrap() else {
            break;
        };
        let h = frame.header;
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for channel in &frame.channels {
            for &s in *channel {
                for b in s.to_le_bytes() {
                    hash ^= u64::from(b);
                    hash = hash.wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        let line = format!(
            "frame {} {} {} {} {} {hash:016x}",
            h.sample_number, h.block_size, h.channels, h.bits_per_sample, h.sample_rate
        );
        log_lines(r, from, blocks, &mut pending);
        out.extend(pending);
        out.push(line);
    }
    log_lines(r, from, blocks, out);
}

/// What the crate makes of the file at `path`, in the reference's lines;
/// with the seeks the reference made.
pub fn ours(path: &str, seeks: &[u64]) -> Vec<String> {
    let mut r = Reader::open(File::open(path).unwrap()).unwrap();
    let (mut from, mut blocks, mut out) = (0, 0, Vec::new());
    log_lines(&r, &mut from, &mut blocks, &mut out);
    frames(&mut r, &mut from, &mut blocks, &mut out);
    let md5 = match r.md5_matches() {
        Some(true) => "ok",
        Some(false) => "bad",
        None => "none",
    };
    out.push(format!("end FLAC__STREAM_DECODER_END_OF_STREAM {md5}"));
    for &s in seeks {
        out.push(format!("seek {s}"));
        if !r.seek(s).unwrap() {
            out.push("seek-failed".to_owned());
            continue;
        }
        frames(&mut r, &mut from, &mut blocks, &mut out);
    }
    out
}

/// Holds the file at `path` (`NAME.flac`) to its answer (`NAME.txt`).
pub fn decodes_as_libflac_does(path: &str) {
    let base = path.strip_suffix(".flac").unwrap();
    let text = std::fs::read_to_string(format!("{base}.txt")).unwrap();
    let want: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let seeks: Vec<u64> = want
        .iter()
        .filter_map(|l| l.strip_prefix("seek ").map(|s| s.parse().unwrap()))
        .collect();
    let got = ours(path, &seeks);
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        assert_eq!(g, w, "{path}: line {}", i + 1);
    }
    assert_eq!(got.len(), want.len(), "{path}: the lines");
}
