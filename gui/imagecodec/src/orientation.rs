//! EXIF orientation: which way up a picture is meant to be shown.
//!
//! A camera writes its pixels as the sensor saw them and records, in EXIF, how
//! to turn them: a phone held upright takes its picture on its side and says
//! "rotate a quarter turn". Browsers have turned pictures by it since 2020
//! (`image-orientation: from-image` is the default), and so do the system's
//! photo viewers everywhere; a decoder that ignores it shows every portrait
//! photograph from a phone lying down.
//!
//! # Where Chrome reads it, and how
//!
//! - **JPEG**: the first `APP1` segment before the scan whose payload starts
//!   `Exif\0` (and has more than the six bytes of that and its pad), from the
//!   seventh byte on (Blink's `JPEGImageDecoder` through Skia's
//!   `SkJpegMetadataDecoder`).
//! - **PNG**: the first `eXIf` chunk before the image data (Skia's Rust PNG
//!   codec, over the `png` crate, which keeps the first and passes over a
//!   second).
//! - **Nowhere else.** Chrome does not turn a WebP, GIF, BMP or icon by its
//!   EXIF, and neither does this.
//!
//! Either way the bytes are a TIFF structure, and the orientation is read from
//! it as Skia's `SkExif::Parse` reads it ([`from_exif`]): the first orientation
//! entry that is one `SHORT` of 1 to 8, in the first directory or, through the
//! EXIF pointer, the directory it points to.
//!
//! The decoders apply it: [`crate::decode`], [`crate::decode_scaled`] and
//! [`crate::dimensions`] all describe the picture as it is shown, turned.

use alloc::vec::Vec;

use crate::Image;

/// How a picture's stored pixels are turned to show it: the EXIF orientation
/// values, named by where the stored first row and first column end up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// 1: as stored.
    #[default]
    TopLeft,
    /// 2: mirrored left to right.
    TopRight,
    /// 3: turned half round.
    BottomRight,
    /// 4: mirrored top to bottom.
    BottomLeft,
    /// 5: mirrored across the top-left to bottom-right diagonal.
    LeftTop,
    /// 6: turned a quarter turn clockwise.
    RightTop,
    /// 7: mirrored across the other diagonal.
    RightBottom,
    /// 8: turned a quarter turn anticlockwise.
    LeftBottom,
}

impl Orientation {
    /// The orientation an EXIF value names, if it names one.
    #[must_use]
    pub const fn from_value(value: u16) -> Option<Self> {
        Some(match value {
            1 => Self::TopLeft,
            2 => Self::TopRight,
            3 => Self::BottomRight,
            4 => Self::BottomLeft,
            5 => Self::LeftTop,
            6 => Self::RightTop,
            7 => Self::RightBottom,
            8 => Self::LeftBottom,
            _ => return None,
        })
    }

    /// Whether showing it swaps width and height.
    #[must_use]
    pub const fn swaps_dimensions(self) -> bool {
        matches!(
            self,
            Self::LeftTop | Self::RightTop | Self::RightBottom | Self::LeftBottom
        )
    }

    /// The turn that undoes this one: for a program that wants the pixels as
    /// stored, `orientation.inverse().apply(decoded)`.
    #[must_use]
    pub const fn inverse(self) -> Self {
        match self {
            Self::RightTop => Self::LeftBottom,
            Self::LeftBottom => Self::RightTop,
            other => other,
        }
    }

    /// A stored size as shown.
    #[must_use]
    pub const fn shown(self, (width, height): (u32, u32)) -> (u32, u32) {
        if self.swaps_dimensions() {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// The picture as shown.
    #[must_use]
    pub fn apply(self, image: Image) -> Image {
        let (w, h) = (image.width as usize, image.height as usize);
        let source = |x: usize, y: usize| -> u32 {
            y.checked_mul(w)
                .and_then(|row| row.checked_add(x))
                .and_then(|at| image.pixels.get(at))
                .copied()
                .unwrap_or(0)
        };
        let (out_w, out_h) = if self.swaps_dimensions() {
            (h, w)
        } else {
            (w, h)
        };
        // For each shown pixel, the stored pixel it is. Every subtraction is
        // from a bound the loop keeps its counter below.
        let from = |x: usize, y: usize| -> (usize, usize) {
            let (last_x, last_y) = (w.saturating_sub(1), h.saturating_sub(1));
            match self {
                Self::TopLeft => (x, y),
                Self::TopRight => (last_x.saturating_sub(x), y),
                Self::BottomRight => (last_x.saturating_sub(x), last_y.saturating_sub(y)),
                Self::BottomLeft => (x, last_y.saturating_sub(y)),
                Self::LeftTop => (y, x),
                Self::RightTop => (y, last_y.saturating_sub(x)),
                Self::RightBottom => (last_x.saturating_sub(y), last_y.saturating_sub(x)),
                Self::LeftBottom => (last_x.saturating_sub(y), x),
            }
        };
        if self == Self::TopLeft {
            return image;
        }
        let mut pixels = Vec::with_capacity(image.pixels.len());
        for y in 0..out_h {
            for x in 0..out_w {
                let (sx, sy) = from(x, y);
                pixels.push(source(sx, sy));
            }
        }
        let (width, height) = self.shown((image.width, image.height));
        Image {
            width,
            height,
            pixels,
        }
    }
}

// ---------------------------------------------------------------------------
// SkExif::Parse, for the orientation
// ---------------------------------------------------------------------------

const ORIENTATION_TAG: u16 = 0x0112;
const EXIF_POINTER_TAG: u16 = 0x8769;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const ENTRY_SIZE: usize = 12;

/// Byte order and position, over a TIFF structure.
#[derive(Clone, Copy)]
struct Tiff<'a> {
    data: &'a [u8],
    little: bool,
}

impl Tiff<'_> {
    fn u16(self, at: usize) -> Option<u16> {
        let bytes: [u8; 2] = self.data.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    }

    fn u32(self, at: usize) -> Option<u32> {
        let bytes: [u8; 4] = self.data.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }

    /// The entries of the directory at `offset` (`validate_ifd`, truncation
    /// allowed): as many whole ones as the data holds, up to its count.
    fn entries(self, offset: usize) -> Option<impl Iterator<Item = usize>> {
        let count = usize::from(self.u16(offset)?);
        let first = offset.checked_add(2)?;
        let room = self.data.len().saturating_sub(first) / ENTRY_SIZE;
        Some((0..count.min(room)).map(move |i| first.saturating_add(i.saturating_mul(ENTRY_SIZE))))
    }

    /// An entry's value if it is one `kind` (`getEntryValuesGeneric` with a
    /// count of 1): the tag's type must be `kind` exactly, its count exactly 1.
    fn single(self, entry: usize, kind: u16) -> Option<u32> {
        if self.u16(entry.checked_add(2)?)? != kind || self.u32(entry.checked_add(4)?)? != 1 {
            return None;
        }
        let value = entry.checked_add(8)?;
        match kind {
            TYPE_SHORT => self.u16(value).map(u32::from),
            _ => self.u32(value),
        }
    }
}

/// The orientation in a TIFF structure -- an EXIF block's contents, or a PNG
/// `eXIf` chunk's -- as Skia reads it, or `None` if it holds none that counts.
#[must_use]
pub fn from_exif(data: &[u8]) -> Option<Orientation> {
    if data.len() < 8 {
        return None;
    }
    let little = match data.get(..4)? {
        [b'I', b'I', 0x2A, 0x00] => true,
        [b'M', b'M', 0x00, 0x2A] => false,
        _ => return None,
    };
    let tiff = Tiff { data, little };
    let root = tiff.u32(4)? as usize;
    search(tiff, root, true)
}

/// `parse_ifd`, for the orientation: entries in order, the first valid
/// orientation winning, the EXIF directory searched where its pointer is
/// (from the first directory only).
fn search(tiff: Tiff<'_>, offset: usize, root: bool) -> Option<Orientation> {
    for entry in tiff.entries(offset)? {
        match tiff.u16(entry) {
            Some(ORIENTATION_TAG) => {
                let value = tiff
                    .single(entry, TYPE_SHORT)
                    .and_then(|v| u16::try_from(v).ok());
                if let Some(found) = value.and_then(Orientation::from_value) {
                    return Some(found);
                }
            }
            Some(EXIF_POINTER_TAG) if root => {
                if let Some(found) = tiff
                    .single(entry, TYPE_LONG)
                    .and_then(|sub| search(tiff, sub as usize, false))
                {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "tests"
    )]

    use super::*;
    use alloc::vec;

    /// A little- or big-endian TIFF with the given entries (tag, type, count,
    /// value) in its first directory, and optionally a second directory at
    /// `sub` with its own.
    fn tiff(
        little: bool,
        entries: &[(u16, u16, u32, u32)],
        sub: &[(u16, u16, u32, u32)],
    ) -> Vec<u8> {
        let u16b = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let u32b = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let mut out = if little {
            b"II*\0".to_vec()
        } else {
            b"MM\0*".to_vec()
        };
        out.extend_from_slice(&u32b(8));
        let dir = |out: &mut Vec<u8>, entries: &[(u16, u16, u32, u32)]| {
            out.extend_from_slice(&u16b(entries.len() as u16));
            for &(tag, kind, count, value) in entries {
                out.extend_from_slice(&u16b(tag));
                out.extend_from_slice(&u16b(kind));
                out.extend_from_slice(&u32b(count));
                if kind == TYPE_SHORT {
                    out.extend_from_slice(&u16b(value as u16));
                    out.extend_from_slice(&[0, 0]);
                } else {
                    out.extend_from_slice(&u32b(value));
                }
            }
            out.extend_from_slice(&u32b(0));
        };
        dir(&mut out, entries);
        if !sub.is_empty() {
            dir(&mut out, sub);
        }
        out
    }

    /// Where `tiff` puts the second directory, for an `entries`-long first.
    fn sub_offset(entries: usize) -> u32 {
        (8 + 2 + 12 * entries + 4) as u32
    }

    #[test]
    fn the_orientation_is_read_in_either_byte_order() {
        for little in [true, false] {
            for value in 1..=8u32 {
                let data = tiff(
                    little,
                    &[
                        (0x0100, TYPE_LONG, 1, 640),
                        (ORIENTATION_TAG, TYPE_SHORT, 1, value),
                    ],
                    &[],
                );
                assert_eq!(
                    from_exif(&data),
                    Orientation::from_value(value as u16),
                    "{value}"
                );
            }
        }
    }

    #[test]
    fn only_a_single_short_of_one_to_eight_counts() {
        for (kind, count, value) in [
            (TYPE_SHORT, 1, 0),
            (TYPE_SHORT, 1, 9),
            (TYPE_LONG, 1, 6),
            (TYPE_SHORT, 2, 6),
        ] {
            let data = tiff(true, &[(ORIENTATION_TAG, kind, count, value)], &[]);
            assert_eq!(from_exif(&data), None, "{kind} x{count} = {value}");
        }
        // A bad one does not hide a good one after it.
        let data = tiff(
            true,
            &[
                (ORIENTATION_TAG, TYPE_SHORT, 1, 0),
                (ORIENTATION_TAG, TYPE_SHORT, 1, 3),
            ],
            &[],
        );
        assert_eq!(from_exif(&data), Some(Orientation::BottomRight));
    }

    #[test]
    fn the_exif_directory_is_searched_where_its_pointer_is() {
        let data = tiff(
            true,
            &[(EXIF_POINTER_TAG, TYPE_LONG, 1, sub_offset(1))],
            &[(ORIENTATION_TAG, TYPE_SHORT, 1, 6)],
        );
        assert_eq!(from_exif(&data), Some(Orientation::RightTop));
        // The first directory's own, if it comes first, wins.
        let entries = [
            (ORIENTATION_TAG, TYPE_SHORT, 1, 8),
            (EXIF_POINTER_TAG, TYPE_LONG, 1, sub_offset(2)),
        ];
        let data = tiff(true, &entries, &[(ORIENTATION_TAG, TYPE_SHORT, 1, 6)]);
        assert_eq!(from_exif(&data), Some(Orientation::LeftBottom));
        // A pointer of the wrong type is not followed.
        let data = tiff(
            true,
            &[(EXIF_POINTER_TAG, TYPE_SHORT, 1, sub_offset(1))],
            &[(ORIENTATION_TAG, TYPE_SHORT, 1, 6)],
        );
        assert_eq!(from_exif(&data), None);
    }

    #[test]
    fn a_directory_cut_short_keeps_its_whole_entries() {
        let data = tiff(
            true,
            &[
                (ORIENTATION_TAG, TYPE_SHORT, 1, 3),
                (0x0100, TYPE_LONG, 1, 5),
            ],
            &[],
        );
        // Cut inside the second entry: the first still counts.
        assert_eq!(
            from_exif(&data[..8 + 2 + 12 + 5]),
            Some(Orientation::BottomRight)
        );
        // Cut inside the first: nothing.
        assert_eq!(from_exif(&data[..8 + 2 + 11]), None);
        assert_eq!(from_exif(b"II*\0\x08\0\0"), None, "a header cut short");
        assert_eq!(from_exif(b"II+\0\x08\0\0\0\0\0"), None, "not TIFF");
    }

    #[test]
    fn each_turn_is_undone_by_its_inverse() {
        let image = Image {
            width: 3,
            height: 2,
            pixels: vec![1, 2, 3, 4, 5, 6],
        };
        for value in 1..=8 {
            let o = Orientation::from_value(value).unwrap();
            assert_eq!(o.inverse().apply(o.apply(image.clone())), image, "{value}");
        }
    }

    #[test]
    fn each_orientation_turns_the_pixels_as_its_name_says() {
        // 3 wide, 2 high:  a b c
        //                  d e f
        let image = Image {
            width: 3,
            height: 2,
            pixels: vec![1, 2, 3, 4, 5, 6],
        };
        let shown = |o: Orientation| {
            let out = o.apply(image.clone());
            (out.width, out.height, out.pixels)
        };
        assert_eq!(shown(Orientation::TopLeft), (3, 2, vec![1, 2, 3, 4, 5, 6]));
        assert_eq!(shown(Orientation::TopRight), (3, 2, vec![3, 2, 1, 6, 5, 4]));
        assert_eq!(
            shown(Orientation::BottomRight),
            (3, 2, vec![6, 5, 4, 3, 2, 1])
        );
        assert_eq!(
            shown(Orientation::BottomLeft),
            (3, 2, vec![4, 5, 6, 1, 2, 3])
        );
        assert_eq!(shown(Orientation::LeftTop), (2, 3, vec![1, 4, 2, 5, 3, 6]));
        assert_eq!(shown(Orientation::RightTop), (2, 3, vec![4, 1, 5, 2, 6, 3]));
        assert_eq!(
            shown(Orientation::RightBottom),
            (2, 3, vec![6, 3, 5, 2, 4, 1])
        );
        assert_eq!(
            shown(Orientation::LeftBottom),
            (2, 3, vec![3, 6, 2, 5, 1, 4])
        );
    }
}
