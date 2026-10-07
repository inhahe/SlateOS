//! What the VP8 tests share: reading libvpx's test vectors out of their
//! container, and finding them.
//!
//! libvpx's VP8 vectors are all IVF, libvpx's own minimal format. The reader
//! here does only what the vectors need -- the frames, in order -- and
//! refuses a file it cannot read loudly, so such a vector fails its test
//! rather than passing with no frames.
//!
//! Two sets of vectors are read. `tests/data` holds a selection of them,
//! chosen to cover the decoder's features cheaply, and is committed, so
//! those tests always run. The full set of 62 (4 MB) is fetched by
//! `tools/fetch_vectors.py` into `target/vp8vectors`; when it is there, the
//! full-suite test runs over it too.
//!
//! The vectors and their MD5 files are the WebM project's, published with
//! libvpx (<https://storage.googleapis.com/downloads.webmproject.org/test_data/libvpx/>)
//! under libvpx's licence (`licenses/libvpx-LICENSE`), and are unchanged.

#![allow(
    dead_code,
    reason = "each test binary uses a different part of this shared module"
)]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test support: a malformed vector should fail its test loudly"
)]

use std::path::{Path, PathBuf};

/// A vector's frames, as its container stores them.
pub struct Vector {
    /// The frame size the IVF header declares.
    pub width: u32,
    pub height: u32,
    /// One entry per IVF frame.
    pub frames: Vec<Vec<u8>>,
}

fn le16(b: &[u8]) -> u32 {
    u32::from(u16::from_le_bytes([b[0], b[1]]))
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// IVF: a 32-byte header (`DKIF`, version, header size, fourcc, width,
/// height, rate, scale, frame count), then each frame as a 4-byte size, an
/// 8-byte timestamp and the data.
pub fn read_ivf(data: &[u8]) -> Result<Vector, String> {
    if data.len() < 32 || &data[..4] != b"DKIF" {
        return Err("not an IVF file".into());
    }
    let header_len = le16(&data[6..]) as usize;
    // The fourcc is not checked: four of libvpx's VP8 vectors say `I420`,
    // and libvpx's own test reader takes the codec from the test, not the
    // file.
    let width = le16(&data[12..]);
    let height = le16(&data[14..]);
    let mut pos = header_len.max(32);
    let mut frames = Vec::new();
    while pos < data.len() {
        if data.len() - pos < 12 {
            return Err("IVF frame header truncated".into());
        }
        let size = le32(&data[pos..]) as usize;
        pos += 12;
        let frame = data.get(pos..pos + size).ok_or("IVF frame truncated")?;
        frames.push(frame.to_vec());
        pos += size;
    }
    Ok(Vector {
        width,
        height,
        frames,
    })
}

/// Read a vector.
pub fn read_vector(path: &Path) -> Result<Vector, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    read_ivf(&data)
}

/// The per-frame MD5s libvpx's tests compare against, one per shown frame.
pub fn read_md5s(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_owned)
        .collect())
}

/// The committed vectors.
pub fn committed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

/// The full suite, if `tools/fetch_vectors.py` has fetched it:
/// `target/vp8vectors` at the workspace root.
pub fn full_suite_dir() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3)?;
    let dir = root.join("target").join("vp8vectors");
    dir.join("list.txt").is_file().then_some(dir)
}

/// The vectors in `dir` that have an MD5 file beside them, sorted.
pub fn vectors_in(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().and_then(|e| e.to_str()) == Some("ivf") && md5_path(p).is_file()
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// A vector's MD5 file: its name with `.md5` appended.
pub fn md5_path(vector: &Path) -> PathBuf {
    let mut name = vector.as_os_str().to_owned();
    name.push(".md5");
    PathBuf::from(name)
}
