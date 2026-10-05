//! The demuxer against libvpx's VP9 test vectors: every frame of the video
//! track, demuxed here, decodes -- by `vp9`, lane F's port of libvpx -- to
//! the very pictures libvpx publishes MD5s for. A frame cut short, a frame
//! out of order or a frame missed would make a picture differ, or the count.
//!
//! The vectors and their MD5 files are the WebM project's, published with
//! libvpx, under libvpx's licence (`tests/data/libvpx-LICENSE`); copied from
//! `gui/video/vp9/tests/data`, unchanged. Eight of its ninety-one, chosen
//! for what a demuxer meets: tiny frames, frame-parallel coding, frames
//! that show nothing and frames shown again, two resizes, 4:4:4 and 10-bit.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test: a mismatch should fail it loudly"
)]

use std::fs::File;
use std::path::{Path, PathBuf};

use mediaprobe::Kind;
use mediaprobe::mkv::Demuxer;
use vp9::{Decoder, Picture};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

/// libvpx's `test/md5_helper.h`, as `gui/video/vp9/tests/conformance.rs`
/// computes it: each plane's visible rows, Y then U then V, a sample one
/// byte at 8 bits and two, little-endian, above.
fn picture_md5(p: &Picture) -> String {
    let mut md5 = md5::Md5::new();
    for plane in 0..3 {
        if let Some(v) = p.plane8(plane) {
            for row in 0..v.height {
                md5.update(&v.data[row * v.stride..row * v.stride + v.width]);
            }
        } else if let Some(v) = p.plane16(plane) {
            for row in 0..v.height {
                let line = &v.data[row * v.stride..row * v.stride + v.width];
                let bytes: Vec<u8> = line.iter().flat_map(|s| s.to_le_bytes()).collect();
                md5.update(&bytes);
            }
        }
    }
    md5::hex(&md5.finalize()).to_string()
}

/// The MD5s a `.md5` file lists, a picture a line.
fn md5s(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
        .collect()
}

/// Demuxes `path`'s video track and decodes it; returns how many pictures
/// matched libvpx's, or the first difference.
fn check(path: &Path) -> Result<usize, String> {
    let len = std::fs::metadata(path).unwrap().len();
    let mut demuxer = Demuxer::open(File::open(path).unwrap(), len)
        .unwrap()
        .ok_or("not read as Matroska")?;
    let video = demuxer
        .streams()
        .iter()
        .find(|s| s.info.kind == Kind::Video)
        .ok_or("no video track")?
        .number;
    let want = md5s(&path.with_extension("webm.md5"));
    let mut decoder = Decoder::new();
    let mut shown = 0usize;
    let mut packets = 0usize;
    let mut last_time = i64::MIN;
    while let Some(packet) = demuxer.next_packet().unwrap() {
        if packet.track != video {
            continue;
        }
        // libvpx's vectors are muxed in presentation order, a frame a
        // block.
        if packet.timestamp_ns < last_time {
            return Err(format!("packet {packets}: time goes backwards"));
        }
        last_time = packet.timestamp_ns;
        let picture = decoder
            .decode(&packet.data)
            .map_err(|e| format!("packet {packets}: {e}"))?;
        if let Some(p) = picture {
            let got = picture_md5(&p);
            let expected = want
                .get(shown)
                .ok_or_else(|| format!("picture {shown} beyond libvpx's {}", want.len()))?;
            if &got != expected {
                return Err(format!(
                    "packet {packets}: picture {shown} hashes {got}, libvpx {expected}"
                ));
            }
            shown += 1;
        }
        packets += 1;
    }
    if shown != want.len() {
        return Err(format!("{shown} pictures, libvpx shows {}", want.len()));
    }
    Ok(shown)
}

#[test]
fn every_frame_demuxed_decodes_to_libvpx_pictures() {
    let mut vectors: Vec<PathBuf> = std::fs::read_dir(data_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "webm"))
        .collect();
    vectors.sort();
    assert_eq!(vectors.len(), 8, "the committed vectors");
    let mut failures = Vec::new();
    let mut pictures = 0;
    for v in &vectors {
        match check(v) {
            Ok(n) => pictures += n,
            Err(e) => failures.push(format!("{}: {e}", v.file_name().unwrap().to_string_lossy())),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(pictures > 100, "only {pictures} pictures");
}

/// A seek -- to the start, the middle and the last frame's time -- lands on a
/// frame of the file at or before the time sought where decoding can start:
/// a key frame, which a fresh decoder decodes to a picture.
#[test]
fn a_seek_lands_on_a_frame_decoding_can_start_at() {
    let path = data_dir().join("vp90-2-03-deltaq.webm");
    let len = std::fs::metadata(&path).unwrap().len();
    let mut demuxer = Demuxer::open(File::open(&path).unwrap(), len)
        .unwrap()
        .unwrap();
    let video = demuxer.streams()[0].number;
    // Every frame, with its time, to compare against.
    let mut all = Vec::new();
    while let Some(p) = demuxer.next_packet().unwrap() {
        if p.track == video {
            all.push(p);
        }
    }
    assert!(all.len() > 1);
    let last = all.last().unwrap().timestamp_ns;
    for target in [0, last / 2, last] {
        demuxer.seek(u64::try_from(target).unwrap(), video).unwrap();
        let first = loop {
            let p = demuxer
                .next_packet()
                .unwrap()
                .expect("a frame after the seek");
            if p.track == video {
                break p;
            }
        };
        // A frame of the file, at or before the target, where decoding can
        // start.
        assert!(first.keyframe, "seek to {target}: not a key frame");
        assert!(first.timestamp_ns <= target, "seek to {target}: past it");
        assert!(all.contains(&first));
        let mut decoder = Decoder::new();
        assert!(decoder.decode(&first.data).unwrap().is_some());
    }
}
