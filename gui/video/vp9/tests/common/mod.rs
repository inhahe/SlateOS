//! What the VP9 tests share: reading libvpx's test vectors out of their
//! containers, and finding them.
//!
//! The vectors come in two containers. Most are WebM (Matroska), the rest
//! IVF, libvpx's own minimal format. Both readers here do only what the
//! vectors need -- one video track's frames, in order -- and refuse anything
//! else loudly, so a vector they cannot read fails its test rather than
//! passing with no frames.
//!
//! Two sets of vectors are read. `tests/data` holds a few hundred kilobytes of
//! them, chosen to cover the decoder's features cheaply, and is committed, so
//! those tests always run. The full set of 314 (27 MB) is fetched by
//! `tools/fetch_vectors.py` into `target/vp9vectors`; when it is there, the
//! full-suite tests run over it too, and say how many they found.
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
    /// The frame size the container declares: Matroska's `PixelWidth` and
    /// `PixelHeight`, IVF's header fields.
    pub width: u32,
    pub height: u32,
    /// One entry per container frame: a VP9 frame, or a superframe of several.
    pub packets: Vec<Vec<u8>>,
}

/// Read a vector, by its extension.
pub fn read_vector(path: &Path) -> Result<Vector, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match path.extension().and_then(|e| e.to_str()) {
        Some("ivf") => read_ivf(&data),
        Some("webm") => read_webm(&data),
        _ => Err(format!("{}: not .ivf or .webm", path.display())),
    }
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

// --- IVF -----------------------------------------------------------------------

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
    if &data[8..12] != b"VP90" {
        return Err(format!("IVF fourcc {:?} is not VP90", &data[8..12]));
    }
    let width = le16(&data[12..]);
    let height = le16(&data[14..]);
    let mut pos = header_len.max(32);
    let mut packets = Vec::new();
    while pos < data.len() {
        if data.len() - pos < 12 {
            return Err("IVF frame header truncated".into());
        }
        let size = le32(&data[pos..]) as usize;
        pos += 12;
        let frame = data.get(pos..pos + size).ok_or("IVF frame truncated")?;
        packets.push(frame.to_vec());
        pos += size;
    }
    Ok(Vector {
        width,
        height,
        packets,
    })
}

// --- WebM ------------------------------------------------------------------------

const ID_SEGMENT: u32 = 0x1853_8067;
const ID_TRACKS: u32 = 0x1654_AE6B;
const ID_TRACK_ENTRY: u32 = 0xAE;
const ID_TRACK_NUMBER: u32 = 0xD7;
const ID_TRACK_TYPE: u32 = 0x83;
const ID_CODEC_ID: u32 = 0x86;
const ID_VIDEO: u32 = 0xE0;
const ID_PIXEL_WIDTH: u32 = 0xB0;
const ID_PIXEL_HEIGHT: u32 = 0xBA;
const ID_CLUSTER: u32 = 0x1F43_B675;
const ID_SIMPLE_BLOCK: u32 = 0xA3;
const ID_BLOCK_GROUP: u32 = 0xA0;
const ID_BLOCK: u32 = 0xA1;
/// Top-level elements: where an unknown-size cluster ends.
const TOP_LEVEL: [u32; 7] = [
    ID_CLUSTER,
    ID_TRACKS,
    0x114D_9B74, // SeekHead
    0x1549_A966, // Info
    0x1C53_BB6B, // Cues
    0x1043_A770, // Chapters
    0x1254_C367, // Tags
];

/// An EBML variable-length integer at `pos`: its value with the length
/// marker removed (or kept, for an element ID), its length, and whether it is
/// the all-ones "unknown size".
fn vint(data: &[u8], pos: usize, keep_marker: bool) -> Result<(u64, usize, bool), String> {
    let first = *data.get(pos).ok_or("EBML integer past the end")?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 {
        return Err("EBML integer longer than 8 bytes".into());
    }
    let bytes = data.get(pos..pos + len).ok_or("EBML integer truncated")?;
    let mut value = if keep_marker {
        u64::from(first)
    } else {
        u64::from(first) & ((1u64 << (8 - len)) - 1)
    };
    for &b in &bytes[1..] {
        value = (value << 8) | u64::from(b);
    }
    let all_ones = !keep_marker && value == (1u64 << (7 * len)) - 1;
    Ok((value, len, all_ones))
}

/// One element: its ID, where its content starts and ends.
struct Element {
    id: u32,
    start: usize,
    end: usize,
}

/// The element at `pos` within a parent ending at `parent_end`. An unknown
/// size runs to the end of the parent.
fn element(data: &[u8], pos: usize, parent_end: usize) -> Result<Element, String> {
    let (id, id_len, _) = vint(data, pos, true)?;
    let (size, size_len, unknown) = vint(data, pos + id_len, false)?;
    let start = pos + id_len + size_len;
    let end = if unknown {
        parent_end
    } else {
        start
            .checked_add(usize::try_from(size).map_err(|_| "EBML size too large")?)
            .filter(|&e| e <= parent_end)
            .ok_or_else(|| format!("EBML element {id:#x} runs past its parent"))?
    };
    Ok(Element {
        id: u32::try_from(id).map_err(|_| "EBML ID too long")?,
        start,
        end,
    })
}

fn uint(data: &[u8], e: &Element) -> u64 {
    data[e.start..e.end]
        .iter()
        .fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
}

/// The children of `[start, end)`. An unknown-size child that is a cluster
/// ends at the next top-level element.
fn children(data: &[u8], start: usize, end: usize) -> Result<Vec<Element>, String> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos < end {
        let mut e = element(data, pos, end)?;
        if e.id == ID_CLUSTER && e.end == end {
            // Possibly unknown-size: stop it at the next top-level element.
            let mut p = e.start;
            while p < end {
                let c = element(data, p, end)?;
                if TOP_LEVEL.contains(&c.id) {
                    break;
                }
                p = c.end;
            }
            e.end = p;
        }
        pos = e.end;
        out.push(e);
    }
    Ok(out)
}

/// WebM: the frames of the first VP9 video track, in file order.
pub fn read_webm(data: &[u8]) -> Result<Vector, String> {
    let top = children(data, 0, data.len())?;
    let segment = top
        .iter()
        .find(|e| e.id == ID_SEGMENT)
        .ok_or("no Matroska segment")?;
    let mut track = None;
    let mut width = 0;
    let mut height = 0;
    let mut packets = Vec::new();
    for e in children(data, segment.start, segment.end)? {
        match e.id {
            ID_TRACKS => {
                for entry in children(data, e.start, e.end)? {
                    if entry.id != ID_TRACK_ENTRY || track.is_some() {
                        continue;
                    }
                    let mut number = None;
                    let mut video = false;
                    let mut vp9 = false;
                    let (mut w, mut h) = (0, 0);
                    for f in children(data, entry.start, entry.end)? {
                        match f.id {
                            ID_TRACK_NUMBER => number = Some(uint(data, &f)),
                            ID_TRACK_TYPE => video = uint(data, &f) == 1,
                            ID_CODEC_ID => vp9 = &data[f.start..f.end] == b"V_VP9",
                            ID_VIDEO => {
                                for v in children(data, f.start, f.end)? {
                                    match v.id {
                                        ID_PIXEL_WIDTH => w = uint(data, &v) as u32,
                                        ID_PIXEL_HEIGHT => h = uint(data, &v) as u32,
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if video && vp9 {
                        track = number;
                        width = w;
                        height = h;
                    }
                }
            }
            ID_CLUSTER => {
                let Some(t) = track else {
                    return Err("a cluster before the tracks".into());
                };
                for c in children(data, e.start, e.end)? {
                    match c.id {
                        ID_SIMPLE_BLOCK => block(data, &c, t, &mut packets)?,
                        ID_BLOCK_GROUP => {
                            for b in children(data, c.start, c.end)? {
                                if b.id == ID_BLOCK {
                                    block(data, &b, t, &mut packets)?;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if track.is_none() {
        return Err("no VP9 video track".into());
    }
    Ok(Vector {
        width,
        height,
        packets,
    })
}

/// A (Simple)Block: track number, 16-bit timecode, flags, then the frame.
fn block(data: &[u8], e: &Element, track: u64, out: &mut Vec<Vec<u8>>) -> Result<(), String> {
    let (number, len, _) = vint(data, e.start, false)?;
    if number != track {
        return Ok(());
    }
    let flags_at = e.start + len + 2;
    let flags = *data.get(flags_at).ok_or("block truncated")?;
    if flags & 0x06 != 0 {
        return Err("a laced block: no VP9 vector uses lacing".into());
    }
    out.push(data[flags_at + 1..e.end].to_vec());
    Ok(())
}

// --- Finding vectors -----------------------------------------------------------------

/// The committed vectors: `tests/data`.
pub fn committed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
}

/// The full suite, if `tools/fetch_vectors.py` has fetched it:
/// `target/vp9vectors` at the workspace root.
pub fn full_suite_dir() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3)?;
    let dir = root.join("target").join("vp9vectors");
    dir.join("list.txt").is_file().then_some(dir)
}

/// The vectors in `dir` that have an MD5 file beside them, sorted.
pub fn vectors_in(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| {
                    matches!(p.extension().and_then(|e| e.to_str()), Some("webm" | "ivf"))
                        && md5_path(p).is_file()
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

/// One picture of a reference encode's input: I420, each plane tightly
/// packed, its luma `width` x `height` and its chroma half that each way,
/// rounded up -- the layout `vpxenc --i420` reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct I420 {
    pub width: usize,
    pub height: usize,
    pub planes: [Vec<u8>; 3],
}

impl I420 {
    /// Plane `i`'s width and height.
    pub fn plane_size(&self, i: usize) -> (usize, usize) {
        if i == 0 {
            (self.width, self.height)
        } else {
            (self.width.div_ceil(2), self.height.div_ceil(2))
        }
    }

    /// The `width` x `height` window at (`x0`, `y0`), both even so that the
    /// chroma lines up.
    pub fn window(&self, x0: usize, y0: usize, width: usize, height: usize) -> Self {
        assert!(
            x0.is_multiple_of(2) && y0.is_multiple_of(2),
            "a window at an odd offset"
        );
        let mut planes: [Vec<u8>; 3] = Default::default();
        for (i, out) in planes.iter_mut().enumerate() {
            let s = usize::from(i > 0);
            let stride = self.plane_size(i).0;
            let (w, h) = ((width + s) >> s, (height + s) >> s);
            let (x, y) = (x0 >> s, y0 >> s);
            for row in y..y + h {
                out.extend_from_slice(&self.planes[i][row * stride + x..][..w]);
            }
        }
        Self {
            width,
            height,
            planes,
        }
    }

    /// The picture as `vpxenc --i420` reads it: the planes, one after
    /// another.
    pub fn raw(&self) -> Vec<u8> {
        self.planes.concat()
    }
}

/// The second reference encode's picture size (`tests/data/encoder/README.md`):
/// not a whole number of 8x8 cells either way, so blocks hang over the
/// picture's right and bottom edges and the last cells are partly outside
/// it; no more than 360 rows, which takes libvpx's low-resolution
/// partitioning branch; and no fewer than 640x360 pixels in all, so that
/// libvpx estimates the noise.
pub const CUT_WIDTH: usize = 651;
pub const CUT_HEIGHT: usize = 357;

/// Camera-like noise on a picture's luma, the same on every run: each sample
/// moves by the sum of three draws from {-1, 0, 1}, drawn from Numerical
/// Recipes' linear congruential generator seeded with `seed`.
fn add_noise(picture: &mut I420, seed: u32) {
    let mut state = seed;
    let mut draw = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        i32::try_from((state >> 16) % 3).unwrap() - 1
    };
    for v in &mut picture.planes[0] {
        let n = draw() + draw() + draw();
        *v = u8::try_from((i32::from(*v) + n).clamp(0, 255)).unwrap();
    }
}

/// One step of a fade to black: luma scaled by `k / n`, chroma drawn towards
/// grey by the same factor, each rounded towards zero.
fn fade(picture: &mut I420, k: i32, n: i32) {
    for v in &mut picture.planes[0] {
        *v = u8::try_from(i32::from(*v) * k / n).unwrap();
    }
    for plane in &mut picture.planes[1..] {
        for v in plane.iter_mut() {
            *v = u8::try_from(128 + (i32::from(*v) - 128) * k / n).unwrap();
        }
    }
}

/// The second reference encode's input (`tests/data/encoder/README.md`): 150
/// pictures of [`CUT_WIDTH`] x [`CUT_HEIGHT`], cut from two conformance
/// vectors to reach what the first reference's thirty pictures of steady
/// motion never do:
///
/// - 0-49: a window on `vp90-2-22-svc_1280x720_1.webm` (its pictures 30 to
///   79): two people talking, across the golden refresh at picture 40;
/// - 50-79: a window on `vp90-2-02-size-lf-1920x1080.webm` (pictures 0 to
///   29), leaves against a sky: a scene cut;
/// - 80-114: that vector's picture 29, still, with fresh noise each picture:
///   the noise estimate rises;
/// - 115-149: the first vector again (pictures 80 to 114), fading to black:
///   a second cut, then a change of light that is not motion.
///
/// `shown(name, count)` gives the first `count` pictures the full suite's
/// vector `name` shows, decoded -- the caller's decoder, so that this module
/// needs no crate path -- or `None` when they cannot be had.
pub fn cut_reference_input(shown: impl Fn(&str, usize) -> Option<Vec<I420>>) -> Option<Vec<I420>> {
    let talk = shown("vp90-2-22-svc_1280x720_1.webm", 115)?;
    let trees = shown("vp90-2-02-size-lf-1920x1080.webm", 30)?;
    let (w, h) = (CUT_WIDTH, CUT_HEIGHT);
    let mut out: Vec<I420> = talk[30..80]
        .iter()
        .map(|p| p.window(300, 180, w, h))
        .collect();
    out.extend(trees.iter().map(|p| p.window(600, 360, w, h)));
    for i in 0..35u32 {
        let mut p = trees[29].window(600, 360, w, h);
        add_noise(&mut p, 0x9e37_79b9 ^ i);
        out.push(p);
    }
    for (k, p) in (0i32..).zip(&talk[80..115]) {
        let mut p = p.window(300, 180, w, h);
        fade(&mut p, 35 - k, 35);
        out.push(p);
    }
    assert_eq!(out.len(), 150);
    Some(out)
}

/// The third reference encode's picture size (`tests/data/encoder/README.md`):
/// 352x288 or fewer pixels, where libvpx's speed 8 partitions inter frames
/// by its learned search, and not a whole number of 8x8 cells either way.
pub const SMALL_WIDTH: usize = 350;
pub const SMALL_HEIGHT: usize = 286;

/// The third reference encode's input (`tests/data/encoder/README.md`): 90
/// pictures of [`SMALL_WIDTH`] x [`SMALL_HEIGHT`] --
///
/// - 0-49: a window on `vp90-2-22-svc_1280x720_1.webm` (its pictures 0 to
///   49): two people talking, across the golden refresh at picture 40;
/// - 50-74: a window on `vp90-2-02-size-lf-1920x1080.webm` (pictures 0 to
///   24), leaves against a sky: a scene cut;
/// - 75-89: the first vector again (pictures 50 to 64), fading to black.
///
/// `shown` as for [`cut_reference_input`].
pub fn small_reference_input(
    shown: impl Fn(&str, usize) -> Option<Vec<I420>>,
) -> Option<Vec<I420>> {
    let talk = shown("vp90-2-22-svc_1280x720_1.webm", 65)?;
    let trees = shown("vp90-2-02-size-lf-1920x1080.webm", 25)?;
    let (w, h) = (SMALL_WIDTH, SMALL_HEIGHT);
    let mut out: Vec<I420> = talk[..50]
        .iter()
        .map(|p| p.window(464, 216, w, h))
        .collect();
    out.extend(trees.iter().map(|p| p.window(784, 396, w, h)));
    for (k, p) in (0i32..).zip(&talk[50..65]) {
        let mut p = p.window(464, 216, w, h);
        fade(&mut p, 15 - k, 15);
        out.push(p);
    }
    assert_eq!(out.len(), 90);
    Some(out)
}
