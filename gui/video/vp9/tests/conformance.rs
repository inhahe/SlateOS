//! The decoder against libvpx's conformance vectors: every picture each
//! vector shows must hash to the MD5 libvpx publishes for it.
//!
//! The hash is libvpx's `test/md5_helper.h`: each plane's visible rows in
//! order, Y then U then V, each sample one byte for 8-bit streams and two
//! (little-endian) for 10- and 12-bit ones. A chroma plane's size is the
//! luma size halved and rounded up along each subsampled axis.
//!
//! The committed vectors run on every `cargo test`; the full suite of 314
//! runs with `--ignored` once `tools/fetch_vectors.py` has fetched it.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test: a mismatch should fail it loudly"
)]

mod common;

use std::path::Path;

use vp9::{Decoder, Picture};

/// libvpx's `MD5::Add(const vpx_image_t *img)`.
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

/// Decode a vector on up to `threads` threads; compare each picture with
/// libvpx's. Returns how many pictures matched, or the first difference.
fn check(path: &Path, threads: usize) -> Result<usize, String> {
    let v = common::read_vector(path)?;
    let want = common::read_md5s(&common::md5_path(path))?;
    let mut d = Decoder::new();
    d.set_threads(threads);
    let mut n = 0usize;
    for (i, packet) in v.packets.iter().enumerate() {
        let picture = d.decode(packet).map_err(|e| format!("packet {i}: {e}"))?;
        if let Some(p) = picture {
            let got = picture_md5(&p);
            let expected = want
                .get(n)
                .ok_or_else(|| format!("packet {i}: picture {n} beyond libvpx's {}", want.len()))?;
            if &got != expected {
                return Err(format!(
                    "packet {i}: picture {n} ({}x{}) hashes {got}, libvpx {expected}",
                    p.width(),
                    p.height()
                ));
            }
            n += 1;
        }
    }
    if n != want.len() {
        return Err(format!("{n} pictures, libvpx shows {}", want.len()));
    }
    Ok(n)
}

fn check_all(dir: &Path, threads: usize) -> (usize, Vec<String>) {
    let vectors = common::vectors_in(dir);
    assert!(!vectors.is_empty(), "no vectors in {}", dir.display());
    let mut failures = Vec::new();
    for p in &vectors {
        if let Err(e) = check(p, threads) {
            failures.push(format!("{}: {e}", p.file_name().unwrap().to_string_lossy()));
        }
    }
    (vectors.len(), failures)
}

/// The thread counts every vector is decoded with: one, and four -- more
/// than one forces the threaded path wherever a frame has two tile columns
/// or more, whatever the machine's cores, and four against eight or sixteen
/// columns gives each thread several.
const THREADS: [usize; 2] = [1, 4];

#[test]
fn every_committed_vector_decodes_to_libvpx_pictures() {
    for threads in THREADS {
        let (n, failures) = check_all(&common::committed_dir(), threads);
        assert!(
            failures.is_empty(),
            "on {threads} thread(s), {} of {n} vectors differ from libvpx:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

/// Tile columns dealt unevenly to threads, and more threads than columns:
/// the pictures are the same however the work is shared.
#[test]
fn any_thread_count_gives_libvpx_pictures() {
    let path = common::committed_dir().join("vp90-2-14-resize-fp-tiles-1-2.webm");
    for threads in [1, 2, 3, 5, 64] {
        let result = check(&path, threads);
        assert!(result.is_ok(), "on {threads} thread(s): {result:?}");
    }
}

#[test]
#[ignore = "needs the full vector suite: python gui/video/vp9/tools/fetch_vectors.py"]
fn every_vector_of_the_full_suite_decodes_to_libvpx_pictures() {
    let dir = common::full_suite_dir()
        .expect("the full suite is not fetched: python gui/video/vp9/tools/fetch_vectors.py");
    for threads in THREADS {
        let (n, failures) = check_all(&dir, threads);
        assert_eq!(n, 314, "libvpx's suite has 314 VP9 vectors");
        assert!(
            failures.is_empty(),
            "on {threads} thread(s), {} of {n} vectors differ from libvpx:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
