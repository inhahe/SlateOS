//! The frame header reader against libvpx's conformance vectors: every frame
//! of every vector starts as a VP9 frame should, every stream starts where a
//! decoder can, and every key frame declares the size its container does.
//!
//! This checks the readers that find frames (superframe indexes, the frame
//! header's first fields) on real streams before anything decodes them; the
//! conformance test proper, which compares decoded pictures, builds on it.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test: a malformed vector should fail it loudly"
)]

mod common;

use std::path::Path;

use vp9::header::{StreamInfo, parse_superframe_index, peek_stream_info};

/// The frames a packet carries: those its superframe index lists, or the
/// packet itself when it has none.
fn frames_of(packet: &[u8]) -> Result<Vec<&[u8]>, String> {
    let sizes = parse_superframe_index(packet).map_err(|e| e.to_string())?;
    if sizes.is_empty() {
        return Ok(vec![packet]);
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    for size in sizes {
        let end = pos
            .checked_add(size as usize)
            .filter(|&e| e <= packet.len())
            .ok_or("a superframe frame runs past its packet")?;
        out.push(&packet[pos..end]);
        pos = end;
    }
    Ok(out)
}

/// Vectors whose frames change size, or whose container states a size other
/// than the coded one: their key frames are not checked against it.
fn sized_by_container(name: &str) -> bool {
    !["resize", "scaling", "svc"]
        .iter()
        .any(|w| name.contains(w))
}

fn check(path: &Path) -> Result<usize, String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let v = common::read_vector(path)?;
    let md5s = common::read_md5s(&common::md5_path(path))?;
    let first = v.packets.first().ok_or("no frames")?;
    let first_frame = *frames_of(first)?.first().ok_or("an empty first packet")?;
    let si = peek_stream_info(first_frame).map_err(|e| format!("frame 0: {e}"))?;
    if !si.is_kf && !si.intra_only {
        return Err("the stream does not start with a key or intra-only frame".into());
    }
    let mut frames = 0;
    for (i, packet) in v.packets.iter().enumerate() {
        for frame in frames_of(packet)? {
            let si: StreamInfo = peek_stream_info(frame).map_err(|e| format!("packet {i}: {e}"))?;
            if si.is_kf && sized_by_container(name) && (si.width, si.height) != (v.width, v.height)
            {
                return Err(format!(
                    "packet {i}: a key frame of {}x{} in a {}x{} container",
                    si.width, si.height, v.width, v.height
                ));
            }
            frames += 1;
        }
    }
    // libvpx shows at most one frame per packet, so there are never more
    // pictures to check than packets.
    if md5s.len() > v.packets.len() {
        return Err(format!(
            "{} pictures from {} packets",
            md5s.len(),
            v.packets.len()
        ));
    }
    Ok(frames)
}

fn check_all(dir: &Path) -> usize {
    let vectors = common::vectors_in(dir);
    assert!(!vectors.is_empty(), "no vectors in {}", dir.display());
    let failures: Vec<String> = vectors
        .iter()
        .filter_map(|p| check(p).err().map(|e| format!("{}: {e}", p.display())))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    vectors.len()
}

#[test]
fn every_committed_vector_starts_and_frames_as_vp9() {
    let n = check_all(&common::committed_dir());
    assert!(n >= 40, "only {n} committed vectors found");
}

#[test]
#[ignore = "needs the full vector suite: python gui/video/vp9/tools/fetch_vectors.py"]
fn every_vector_of_the_full_suite_starts_and_frames_as_vp9() {
    let dir = common::full_suite_dir()
        .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
    let n = check_all(&dir);
    assert_eq!(n, 314, "libvpx's suite has 314 VP9 vectors");
}

#[test]
fn the_readers_find_every_frame_of_a_known_vector() {
    // vp90-2-02-size-08x08.webm: an 8x8 stream of ten frames, libvpx's MD5
    // file listing ten pictures.
    let path = common::committed_dir().join("vp90-2-02-size-08x08.webm");
    let v = common::read_vector(&path).unwrap();
    assert_eq!((v.width, v.height), (8, 8));
    let md5s = common::read_md5s(&common::md5_path(&path)).unwrap();
    assert_eq!(md5s.len(), 10);
    let frames: usize = v.packets.iter().map(|p| frames_of(p).unwrap().len()).sum();
    assert!(frames >= md5s.len());
}
