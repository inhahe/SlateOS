//! The decoder against libvpx's VP8 test vectors: every picture each vector
//! shows must hash to the MD5 libvpx publishes for it.
//!
//! The hash is libvpx's `test/md5_helper.h`: each plane's visible rows in
//! order, Y then U then V, one byte per sample. A chroma plane's size is the
//! luma size halved and rounded up each way.
//!
//! The committed vectors run on every `cargo test`; the full suite of 62
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

use vp8::{Decoder, Picture};

/// libvpx's `MD5::Add(const vpx_image_t *img)`.
fn picture_md5(p: &Picture) -> String {
    let mut md5 = md5::Md5::new();
    for plane in 0..3 {
        let v = p.plane(plane).unwrap();
        for row in 0..v.height {
            md5.update(&v.data[row * v.stride..row * v.stride + v.width]);
        }
    }
    md5::hex(&md5.finalize()).to_string()
}

/// Decode a vector; compare each picture with libvpx's. Returns how many
/// pictures matched, or the first difference.
fn check(path: &Path) -> Result<usize, String> {
    let v = common::read_vector(path)?;
    let want = common::read_md5s(&common::md5_path(path))?;
    let mut d = Decoder::new();
    let mut n = 0usize;
    for (i, frame) in v.frames.iter().enumerate() {
        let picture = d.decode(frame).map_err(|e| format!("frame {i}: {e}"))?;
        if let Some(p) = picture {
            let got = picture_md5(&p);
            let expected = want
                .get(n)
                .ok_or_else(|| format!("frame {i}: picture {n} beyond libvpx's {}", want.len()))?;
            if &got != expected {
                return Err(format!(
                    "frame {i}: picture {n} ({}x{}) hashes {got}, libvpx {expected}",
                    p.width(),
                    p.height()
                ));
            }
            if p.corrupted() {
                return Err(format!("frame {i}: picture {n} is marked corrupt"));
            }
            n += 1;
        }
    }
    if n != want.len() {
        return Err(format!("{n} pictures, libvpx shows {}", want.len()));
    }
    Ok(n)
}

fn check_all(dir: &Path) -> (usize, Vec<String>) {
    let vectors = common::vectors_in(dir);
    assert!(!vectors.is_empty(), "no vectors in {}", dir.display());
    let mut failures = Vec::new();
    for p in &vectors {
        if let Err(e) = check(p) {
            failures.push(format!("{}: {e}", p.file_name().unwrap().to_string_lossy()));
        }
    }
    (vectors.len(), failures)
}

#[test]
fn every_committed_vector_decodes_to_libvpx_pictures() {
    let (n, failures) = check_all(&common::committed_dir());
    assert!(
        failures.is_empty(),
        "{} of {n} vectors differ from libvpx:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "needs the full vector suite: python gui/video/vp8/tools/fetch_vectors.py"]
fn every_vector_of_the_full_suite_decodes_to_libvpx_pictures() {
    let dir = common::full_suite_dir()
        .expect("the full suite is not fetched: python gui/video/vp8/tools/fetch_vectors.py");
    let (n, failures) = check_all(&dir);
    assert_eq!(n, 62, "libvpx's suite has 62 VP8 vectors");
    assert!(
        failures.is_empty(),
        "{} of {n} vectors differ from libvpx:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
