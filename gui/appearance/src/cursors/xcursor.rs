//! XCursor files: the format the pictures of a Linux cursor theme are in.
//!
//! Every value is a little-endian `u32`:
//!
//! ```text
//! file:    "Xcur"  header-length  version  table-length
//!          table-length × (type  subtype  position)
//! image:   header-length  type  subtype  version
//!          width  height  x-hot  y-hot  delay
//!          width × height pixels: premultiplied ARGB, row by row
//! ```
//!
//! An image's table type is `0xfffd0002` and its subtype is its *nominal*
//! size -- the size the theme drew it for, which is usually its width -- and
//! a file holds each picture at several (a theme commonly ships 24, 32, 48, 64
//! and 96). Anything else in the table, such as a comment (`0xfffe0001`), is
//! skipped.
//!
//! [`read`] takes the pictures of the nominal size nearest the size asked
//! for, as libXcursor's `XcursorXcFileLoadImages` does: the first of the
//! nearest where two are as near, and *every* image of that size, in the
//! table's order, as the frames of an animation -- the busy pointer of a
//! common theme is sixty of them, sixteen milliseconds each. Like libXcursor
//! it starts an image's pixels 36 bytes into its chunk whatever the chunk's
//! own header-length says, and refuses the whole cursor if any one of its
//! frames is bad: half an animation is not the animation.
//!
//! # Trust
//!
//! A theme is whatever its author put in it, so nothing in the file is
//! believed before it is checked: every position and length against the
//! file's end, a picture's sides against [`MAX_SIDE`], its hot spot against
//! its sides, and the pixels decoded, over every frame, against
//! [`MAX_DECODED_PIXELS`] -- two table entries may name one image, and
//! without that bound a file of a few megabytes could ask for gigabytes. A
//! colour channel brighter than its pixel's alpha, which premultiplied colour
//! cannot be, is held to the alpha, so a careless theme cannot make a blend
//! overflow.

/// The table type of an image.
const IMAGE_TYPE: u32 = 0xfffd_0002;

/// The first four bytes of every XCursor file.
const MAGIC: &[u8; 4] = b"Xcur";

/// The file header's length, at least: the magic and three words.
const FILE_HEADER_LEN: usize = 16;

/// Bytes from the start of an image chunk to its first pixel.
const IMAGE_HEADER_LEN: usize = 36;

/// The longest table read. A real file's is a few hundred entries -- one per
/// frame per size -- so this is room to spare, and a bound on the scan.
const MAX_TABLE: usize = 4096;

/// The longest side of a picture, in pixels. libXcursor allows 32767; a
/// pointer past this is not a pointer.
pub const MAX_SIDE: u32 = 1024;

/// The most pixels decoded from one file, over every frame of the size
/// chosen: 16 Mi, which is 64 MiB of pixels. A common theme's busy pointer at
/// 96 pixels is under 0.6 Mi.
pub const MAX_DECODED_PIXELS: usize = 16 * 1024 * 1024;

/// One picture of a cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The hot spot -- the pixel that *is* the pointer's position -- from the
    /// picture's top-left corner.
    pub hot_x: u32,
    /// See [`hot_x`](Self::hot_x).
    pub hot_y: u32,
    /// How long this frame shows before the next, in milliseconds, as the file
    /// says it; meaningless for a cursor of one frame.
    pub delay_ms: u32,
    /// Row-major **premultiplied** `0xAARRGGBB`, `width * height` of them:
    /// the form a compositor lays over a frame, and the file's own.
    pub pixels: Vec<u32>,
}

/// A cursor's pictures at one size: one frame for a cursor that does not
/// move, several for one that does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorImages {
    /// The nominal size the theme drew these for.
    pub nominal: u32,
    /// The frames, in the order they show.
    pub frames: Vec<CursorFrame>,
}

/// The pictures in the XCursor file `file` whose nominal size is nearest
/// `size`, or `None` for a file that is not one, holds no picture, or holds
/// a bad one of that size (see the [module documentation](self)).
#[must_use]
pub fn read(file: &[u8], size: u32) -> Option<CursorImages> {
    if file.get(..MAGIC.len())? != MAGIC {
        return None;
    }
    let header = usize::try_from(word(file, 4)?).ok()?;
    let length = usize::try_from(word(file, 12)?).ok()?;
    if header < FILE_HEADER_LEN || length > MAX_TABLE {
        return None;
    }
    let table = (0..length)
        .map(|i| {
            let at = header.checked_add(i.checked_mul(12)?)?;
            Some(Entry {
                kind: word(file, at)?,
                nominal: word(file, at.checked_add(4)?)?,
                position: usize::try_from(word(file, at.checked_add(8)?)?).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let images = || table.iter().filter(|e| e.kind == IMAGE_TYPE);

    // The first of the nearest, as libXcursor picks: a later size only
    // replaces an earlier one that is strictly further away.
    let mut nearest: Option<u32> = None;
    for entry in images() {
        if nearest.is_none_or(|n| entry.nominal.abs_diff(size) < n.abs_diff(size)) {
            nearest = Some(entry.nominal);
        }
    }
    let nominal = nearest?;
    let mut budget = MAX_DECODED_PIXELS;
    let frames = images()
        .filter(|e| e.nominal == nominal)
        .map(|e| image(file, e, &mut budget))
        .collect::<Option<Vec<_>>>()?;
    Some(CursorImages { nominal, frames })
}

/// One entry of the table.
struct Entry {
    kind: u32,
    nominal: u32,
    position: usize,
}

/// The image `entry` names, its pixels taken from `budget`.
fn image(file: &[u8], entry: &Entry, budget: &mut usize) -> Option<CursorFrame> {
    // header-length (0), type (1), subtype (2), version (3), width (4),
    // height (5), x-hot (6), y-hot (7), delay (8).
    let field = |i: usize| word(file, entry.position.checked_add(i.checked_mul(4)?)?);
    // The chunk must be what the table said it was, as libXcursor requires.
    if field(1)? != IMAGE_TYPE || field(2)? != entry.nominal {
        return None;
    }
    let (width, height) = (field(4)?, field(5)?);
    let (hot_x, hot_y) = (field(6)?, field(7)?);
    let sides = 1..=MAX_SIDE;
    if !sides.contains(&width) || !sides.contains(&height) || hot_x > width || hot_y > height {
        return None;
    }
    let count = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    *budget = budget.checked_sub(count)?;
    let start = entry.position.checked_add(IMAGE_HEADER_LEN)?;
    let bytes = file.get(start..start.checked_add(count.checked_mul(4)?)?)?;
    let pixels = bytes
        .chunks_exact(4)
        .map(|p| premultiplied(p.try_into().unwrap_or([0; 4])))
        .collect();
    Some(CursorFrame {
        width,
        height,
        hot_x,
        hot_y,
        delay_ms: field(8)?,
        pixels,
    })
}

/// The little-endian word at `at`.
fn word(file: &[u8], at: usize) -> Option<u32> {
    let bytes = file.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// The pixel whose little-endian bytes are `bytes` -- blue, green, red,
/// alpha -- as `0xAARRGGBB`, each colour held to the alpha.
fn premultiplied(bytes: [u8; 4]) -> u32 {
    let [b, g, r, a] = bytes;
    u32::from_le_bytes([b.min(a), g.min(a), r.min(a), a])
}
