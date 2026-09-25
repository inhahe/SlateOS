//! Samples to pixels, as libtiff 4.7.1's RGBA interface converts them.
//!
//! A port of `tif_getimage.c` -- `TIFFRGBAImageOK`, `TIFFRGBAImageBegin`
//! and the strip and tile readers with their "put" routines -- which is how
//! image viewers built on libtiff (GNOME's, through gdk-pixbuf) show a TIFF.
//! Its conversions are kept exactly, including the crude ones: CMYK is
//! turned to RGB by `(255 - K) * (255 - C) / 255` with no colour profile,
//! 16-bit samples are rounded to 8 by `(n + 128) / 257` except grey, which
//! keeps the high byte, and a 16-bit palette is scaled down only when an
//! entry is 256 or more.
//!
//! Two things differ from libtiff's raster, both on purpose:
//!
//! - **No flips.** libtiff's reader turns the picture by `Orientation`, but
//!   only by flipping, treating 5-8 as 1-4; the viewers then turn 5-8 the
//!   rest of the way. Here the stored picture is read as it is and turned
//!   once, by [`crate::orientation`], which ends in the same place.
//! - **Straight alpha where the file has it.** libtiff premultiplies
//!   unassociated alpha into its raster; the compositor wants straight
//!   alpha, which is what the file holds, so it is kept (see [`Alpha`]).

use alloc::vec;
use alloc::vec::Vec;

use super::dir::{self, Directory, File, compression, extra, photometric};
use super::read::Reader;
use crate::{ImageError, ImageResult};

/// How a raster's alpha relates to its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Alpha {
    /// Every pixel opaque.
    Opaque,
    /// Colour premultiplied by alpha, as libtiff's raster holds it.
    Associated,
    /// Straight colour. `libtiff_premultiplies` says whether libtiff's own
    /// raster would hold it premultiplied (RGB), or straight as here (grey,
    /// which libtiff passes through).
    Unassociated { libtiff_premultiplies: bool },
}

/// The decoded picture in libtiff's packing -- `r | g << 8 | b << 16 |
/// a << 24` -- stored orientation, with how its alpha reads.
pub(crate) struct Raster {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u32>,
    pub(crate) alpha: Alpha,
}

const A1: u32 = 0xFF << 24;

fn pack(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16) | A1
}

fn pack4(r: u32, g: u32, b: u32, a: u32) -> u32 {
    r | (g << 8) | (b << 16) | (a << 24)
}

/// `(n + 128) / 257`: sixteen bits to eight (`BuildMapBitdepth16To8`).
fn to8(n: u16) -> u32 {
    u32::from(n).wrapping_add(128) / 257
}

/// `(v * a + 127) / 255`: straight to premultiplied (`BuildMapUaToAa`).
pub(crate) fn premultiply(v: u32, a: u32) -> u32 {
    // Both at most 255: no overflow.
    v.wrapping_mul(a).wrapping_add(127) / 255
}

/// The pixel routine chosen for the image (`PickContigCase`,
/// `PickSeparateCase`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Put {
    Palette(u16),
    Grey8,
    AGrey8,
    Grey16,
    Bw(u16),
    Rgb8,
    Rgbaa8,
    Rgbua8,
    Rgb16,
    Rgbaa16,
    Rgbua16,
    Cmyk8,
    SepRgb8,
    SepRgbaa8,
    SepRgbua8,
    SepRgb16,
    SepRgbaa16,
    SepRgbua16,
    SepCmyk8,
}

/// Lookup tables the routines use.
struct Maps {
    /// `BWmap` / `PALmap`: for each byte of packed samples, the pixels it
    /// holds.
    unpack: Vec<Vec<u32>>,
}

/// What `TIFFRGBAImageBegin` settles.
struct Image {
    width: u32,
    height: u32,
    bits: u16,
    samples: u16,
    photometric: u16,
    alpha: u16,
    contig: bool,
    put: Put,
    maps: Maps,
}

/// Read the whole picture, as `TIFFReadRGBAImageOriented` does with
/// `stop_on_error` set, but unflipped.
///
/// # Errors
///
/// Anything libtiff's RGBA reader refuses: a kind of sample it cannot
/// convert, or a strip that will not read.
pub(crate) fn read(file: File<'_>, dir: &Directory) -> ImageResult<Raster> {
    check(dir)?;
    let img = begin(dir)?;
    let mut raster = vec![0u32; (img.width as usize).saturating_mul(img.height as usize)];
    let mut reader = Reader::new(file, dir);
    match (img.contig, dir.tiled) {
        (true, false) => strips_contig(&img, dir, &mut reader, &mut raster)?,
        (true, true) => tiles_contig(&img, dir, &mut reader, &mut raster)?,
        (false, false) => strips_separate(&img, dir, &mut reader, &mut raster)?,
        (false, true) => tiles_separate(&img, dir, &mut reader, &mut raster)?,
    }
    let alpha = match img.put {
        Put::Rgbaa8 | Put::Rgbaa16 | Put::SepRgbaa8 | Put::SepRgbaa16 => Alpha::Associated,
        Put::Rgbua8 | Put::Rgbua16 | Put::SepRgbua8 | Put::SepRgbua16 => Alpha::Unassociated {
            libtiff_premultiplies: true,
        },
        Put::AGrey8 if img.alpha == extra::ASSOCIATED_ALPHA => Alpha::Associated,
        Put::AGrey8 => Alpha::Unassociated {
            libtiff_premultiplies: false,
        },
        _ => Alpha::Opaque,
    };
    let _ = img.bits;
    Ok(Raster {
        width: img.width,
        height: img.height,
        pixels: raster,
        alpha,
    })
}

/// `TIFFRGBAImageOK`.
fn check(dir: &Directory) -> ImageResult<()> {
    let refuse = ImageError::Unsupported;
    if compression::known(dir.compression) && !compression::configured(dir.compression) {
        return Err(refuse("TIFF compression scheme"));
    }
    if !matches!(dir.bits_per_sample, 1 | 2 | 4 | 8 | 16) {
        return Err(refuse("TIFF sample depth"));
    }
    if dir.sample_format == 3 {
        return Err(refuse("TIFF floating-point samples"));
    }
    let colours = i32::from(dir.samples_per_pixel).wrapping_sub(i32::from(dir.extra_samples()));
    let p = match dir.photometric {
        Some(p) => p,
        None => match colours {
            1 => photometric::MIN_IS_BLACK,
            3 => photometric::RGB,
            _ => return Err(refuse("TIFF without PhotometricInterpretation")),
        },
    };
    match p {
        photometric::MIN_IS_WHITE | photometric::MIN_IS_BLACK | photometric::PALETTE => {
            if dir.planar_config == 1 && dir.samples_per_pixel != 1 && dir.bits_per_sample < 8 {
                return Err(refuse("TIFF packed samples of several channels"));
            }
        }
        photometric::YCBCR => {}
        photometric::RGB => {
            if colours < 3 {
                return Err(refuse("TIFF RGB of fewer than three channels"));
            }
        }
        photometric::SEPARATED => {
            if dir.ink_set != 1 {
                return Err(refuse("TIFF inks other than CMYK"));
            }
            if dir.samples_per_pixel < 4 {
                return Err(refuse("TIFF CMYK of fewer than four channels"));
            }
        }
        photometric::LOGL => {
            if dir.compression != compression::SGILOG {
                return Err(refuse("TIFF LogL"));
            }
        }
        photometric::LOGLUV => {
            if dir.compression != compression::SGILOG && dir.compression != compression::SGILOG24 {
                return Err(refuse("TIFF LogLuv"));
            }
            if dir.planar_config != 1 || dir.samples_per_pixel != 3 || colours != 3 {
                return Err(refuse("TIFF LogLuv"));
            }
        }
        photometric::CIELAB => {
            if dir.samples_per_pixel != 3 || colours != 3 || !matches!(dir.bits_per_sample, 8 | 16)
            {
                return Err(refuse("TIFF CIELab of this shape"));
            }
        }
        _ => return Err(refuse("TIFF photometric interpretation")),
    }
    Ok(())
}

fn is_ccitt(scheme: u16) -> bool {
    matches!(
        scheme,
        compression::CCITT_FAX3
            | compression::CCITT_FAX4
            | compression::CCITT_RLE
            | compression::CCITT_RLEW
    )
}

/// `TIFFRGBAImageBegin`, and the choice of routine.
fn begin(dir: &Directory) -> ImageResult<Image> {
    let refuse = ImageError::Unsupported;
    let bits = dir.bits_per_sample;
    let samples = dir.samples_per_pixel;
    let mut alpha = 0u16;
    if let Some(&first) = dir.sample_info.first() {
        match first {
            extra::UNSPECIFIED => {
                if samples > 3 {
                    alpha = extra::ASSOCIATED_ALPHA;
                }
            }
            extra::ASSOCIATED_ALPHA | extra::UNASSOCIATED_ALPHA => alpha = first,
            _ => {}
        }
    }
    let colours = i32::from(samples).wrapping_sub(i32::from(dir.extra_samples()));
    let p = match dir.photometric {
        Some(p) => p,
        None => match colours {
            1 if is_ccitt(dir.compression) => photometric::MIN_IS_WHITE,
            1 => photometric::MIN_IS_BLACK,
            3 => photometric::RGB,
            _ => return Err(refuse("TIFF without PhotometricInterpretation")),
        },
    };
    let mut color_map = None;
    match p {
        photometric::PALETTE | photometric::MIN_IS_WHITE | photometric::MIN_IS_BLACK => {
            if p == photometric::PALETTE {
                color_map = Some(
                    dir.color_map
                        .clone()
                        .ok_or(refuse("TIFF palette image without a ColorMap"))?,
                );
            }
            if dir.planar_config == 1 && samples != 1 && bits < 8 {
                return Err(refuse("TIFF packed samples of several channels"));
            }
        }
        photometric::YCBCR | photometric::CIELAB => {}
        photometric::RGB => {
            if colours < 3 {
                return Err(refuse("TIFF RGB of fewer than three channels"));
            }
        }
        photometric::SEPARATED => {
            if dir.ink_set != 1 || samples < 4 {
                return Err(refuse("TIFF CMYK of this shape"));
            }
        }
        photometric::LOGL | photometric::LOGLUV => {
            return Err(refuse("TIFF LogLuv, not yet decoded here"));
        }
        _ => return Err(refuse("TIFF photometric interpretation")),
    }
    let contig = !(dir.planar_config == 2 && samples > 1);
    let mut img = Image {
        width: dir.width,
        height: dir.length,
        bits,
        samples,
        photometric: p,
        alpha,
        contig,
        put: Put::Grey8,
        maps: Maps { unpack: Vec::new() },
    };
    img.put = if contig {
        pick_contig(&mut img, color_map)?
    } else {
        pick_separate(&mut img)?
    };
    Ok(img)
}

/// `PickContigCase`.
fn pick_contig(img: &mut Image, color_map: Option<[Vec<u16>; 3]>) -> ImageResult<Put> {
    let refuse = ImageError::Unsupported("TIFF sample layout");
    let (bits, samples, alpha) = (img.bits, img.samples, img.alpha);
    let put = match img.photometric {
        photometric::RGB => match bits {
            8 if alpha == extra::ASSOCIATED_ALPHA && samples >= 4 => Put::Rgbaa8,
            8 if alpha == extra::UNASSOCIATED_ALPHA && samples >= 4 => Put::Rgbua8,
            8 if samples >= 3 => Put::Rgb8,
            16 if alpha == extra::ASSOCIATED_ALPHA && samples >= 4 => Put::Rgbaa16,
            16 if alpha == extra::UNASSOCIATED_ALPHA && samples >= 4 => Put::Rgbua16,
            16 if samples >= 3 => Put::Rgb16,
            _ => return Err(refuse),
        },
        photometric::SEPARATED => {
            if samples >= 4 && bits == 8 {
                Put::Cmyk8
            } else {
                return Err(refuse);
            }
        }
        photometric::PALETTE => {
            let map = color_map.ok_or(refuse.clone())?;
            if !matches!(bits, 1 | 2 | 4 | 8) {
                return Err(refuse);
            }
            img.maps.unpack = palette_map(&map, bits);
            Put::Palette(bits)
        }
        photometric::MIN_IS_WHITE | photometric::MIN_IS_BLACK => {
            img.maps.unpack = grey_map(img.photometric, bits);
            match bits {
                16 => Put::Grey16,
                8 if alpha != 0 && samples == 2 => Put::AGrey8,
                8 => Put::Grey8,
                1 | 2 | 4 => Put::Bw(bits),
                _ => return Err(refuse),
            }
        }
        photometric::YCBCR => {
            return Err(ImageError::Unsupported("TIFF YCbCr, not yet decoded here"));
        }
        photometric::CIELAB => {
            return Err(ImageError::Unsupported("TIFF CIELab, not yet decoded here"));
        }
        _ => return Err(refuse),
    };
    Ok(put)
}

/// `PickSeparateCase`.
fn pick_separate(img: &mut Image) -> ImageResult<Put> {
    let refuse = ImageError::Unsupported("TIFF sample layout");
    let (bits, samples, alpha) = (img.bits, img.samples, img.alpha);
    let put = match img.photometric {
        // Grey planes go through the RGB routines, each channel the grey.
        photometric::MIN_IS_WHITE | photometric::MIN_IS_BLACK | photometric::RGB => match bits {
            8 if alpha == extra::ASSOCIATED_ALPHA => Put::SepRgbaa8,
            8 if alpha == extra::UNASSOCIATED_ALPHA => Put::SepRgbua8,
            8 => Put::SepRgb8,
            16 if alpha == extra::ASSOCIATED_ALPHA => Put::SepRgbaa16,
            16 if alpha == extra::UNASSOCIATED_ALPHA => Put::SepRgbua16,
            16 => Put::SepRgb16,
            _ => return Err(refuse),
        },
        photometric::SEPARATED => {
            if bits == 8 && samples == 4 {
                // The fourth plane, K, is read where alpha would be.
                img.alpha = 1;
                Put::SepCmyk8
            } else {
                return Err(refuse);
            }
        }
        photometric::YCBCR => {
            return Err(ImageError::Unsupported("TIFF YCbCr, not yet decoded here"));
        }
        _ => return Err(refuse),
    };
    Ok(put)
}

/// `setupMap` then `makebwmap`: grey levels scaled to 0-255, inverted for
/// min-is-white, unpacked per byte.
fn grey_map(p: u16, bits: u16) -> Vec<Vec<u32>> {
    // 1, 2, 4 or 8 bits here; 16 is scaled as 8, by its high byte.
    let range: u32 = if bits == 16 {
        255
    } else {
        (1u32 << bits).wrapping_sub(1)
    };
    let map: Vec<u32> = (0..=range)
        .map(|x| {
            let v = if p == photometric::MIN_IS_WHITE {
                range.wrapping_sub(x)
            } else {
                x
            };
            v.wrapping_mul(255).checked_div(range).unwrap_or(0)
        })
        .collect();
    let grey = |x: u32| {
        let c = map.get(x as usize).copied().unwrap_or(0);
        pack(c, c, c)
    };
    unpack_table(bits, grey)
}

/// `checkcmap`, `cvtcmap` and `makecmap`: a palette whose entries are all
/// under 256 is taken as 8-bit, anything else scaled down by 8 bits.
fn palette_map(map: &[Vec<u16>; 3], bits: u16) -> Vec<Vec<u32>> {
    let n = 1usize << bits;
    let wide = map
        .iter()
        .any(|channel| channel.iter().take(n).any(|&v| v >= 256));
    let [red, green, blue] = map;
    let entry = |c: u32| {
        let get = |channel: &Vec<u16>| {
            let v = channel.get(c as usize).copied().unwrap_or(0);
            u32::from(if wide { v >> 8 } else { v }) & 0xFF
        };
        pack(get(red), get(green), get(blue))
    };
    unpack_table(bits, entry)
}

/// For each byte of packed samples, the pixels it holds, most significant
/// sample first.
fn unpack_table(bits: u16, pixel: impl Fn(u32) -> u32) -> Vec<Vec<u32>> {
    (0u32..256)
        .map(|i| match bits {
            1 => (0..8u32).rev().map(|s| pixel((i >> s) & 1)).collect(),
            2 => [6u32, 4, 2, 0]
                .iter()
                .map(|s| pixel((i >> s) & 3))
                .collect(),
            4 => vec![pixel(i >> 4), pixel(i & 0xF)],
            _ => vec![pixel(i)],
        })
        .collect()
}

/// Step a cursor as the C steps a pointer. Every access through a cursor is
/// bounds-checked, so a step past either end makes that access fail rather
/// than anything worse: wrapping is the whole of the arithmetic's risk.
fn step(at: isize, by: isize) -> isize {
    at.wrapping_add(by)
}

/// The sample byte at `at`.
fn byte(buf: &[u8], at: isize) -> ImageResult<u32> {
    usize::try_from(at)
        .ok()
        .and_then(|i| buf.get(i))
        .map(|&b| u32::from(b))
        .ok_or(ImageError::Malformed("TIFF samples short of their strip"))
}

/// The little-endian 16-bit sample at byte offset `at`.
fn word(buf: &[u8], at: isize) -> ImageResult<u16> {
    let lo = byte(buf, at)?;
    let hi = byte(buf, step(at, 1))?;
    Ok(u16::try_from(lo | (hi << 8)).unwrap_or(0))
}

fn store(raster: &mut [u32], at: isize, pixel: u32) -> ImageResult<()> {
    let slot = usize::try_from(at)
        .ok()
        .and_then(|i| raster.get_mut(i))
        .ok_or(ImageError::Malformed("TIFF pixels outside the picture"))?;
    *slot = pixel;
    Ok(())
}

/// The first pixel a packed-sample table gives for byte `v`.
fn unpacked(img: &Image, v: u32) -> u32 {
    img.maps
        .unpack
        .get(v as usize)
        .and_then(|e| e.first())
        .copied()
        .unwrap_or(0)
}

/// `(255 - k) * (255 - c) / 255`: libtiff's CMYK to RGB, no profile.
fn ink(k: u32, c: u32) -> u32 {
    255u32.wrapping_sub(k).wrapping_mul(255u32.wrapping_sub(c)) / 255
}

/// Where one call of a routine reads and writes, and how far each skips
/// between rows (in the C's units: pixels, or samples once scaled).
#[derive(Clone, Copy)]
struct Span {
    /// Raster index of the first pixel.
    cp: isize,
    w: u32,
    h: u32,
    fromskew: isize,
    toskew: isize,
}

/// A contiguous-sample routine (`DECLAREContigPutFunc`): `span.w` x
/// `span.h` pixels from `buf` at `pp` into `raster`, with the C's skews.
#[allow(clippy::too_many_lines)] // One arm per routine, as the C has one function each.
fn put_contig(
    img: &Image,
    raster: &mut [u32],
    span: Span,
    buf: &[u8],
    mut pp: isize,
) -> ImageResult<()> {
    let Span {
        mut cp,
        w,
        h,
        fromskew,
        toskew,
    } = span;
    let spp = isize::from(img.samples.cast_signed());
    match img.put {
        Put::Palette(8) | Put::Grey8 => {
            for _ in 0..h {
                for _ in 0..w {
                    store(raster, cp, unpacked(img, byte(buf, pp)?))?;
                    cp = step(cp, 1);
                    pp = step(pp, spp);
                }
                cp = step(cp, toskew);
                pp = step(pp, fromskew);
            }
        }
        Put::AGrey8 => {
            for _ in 0..h {
                for _ in 0..w {
                    let grey = unpacked(img, byte(buf, pp)?);
                    let a = byte(buf, step(pp, 1))?;
                    store(raster, cp, grey & ((a << 24) | !A1))?;
                    cp = step(cp, 1);
                    pp = step(pp, spp);
                }
                cp = step(cp, toskew);
                pp = step(pp, fromskew);
            }
        }
        Put::Grey16 => {
            for _ in 0..h {
                for _ in 0..w {
                    // The sample's high byte.
                    store(raster, cp, unpacked(img, u32::from(word(buf, pp)? >> 8)))?;
                    cp = step(cp, 1);
                    pp = step(pp, spp.wrapping_mul(2));
                }
                cp = step(cp, toskew);
                pp = step(pp, fromskew);
            }
        }
        Put::Palette(bits @ (1 | 2 | 4)) | Put::Bw(bits @ (1 | 2 | 4)) => {
            let per: u32 = 8u32.checked_div(u32::from(bits)).unwrap_or(1);
            let fromskew = fromskew
                .checked_div(isize::try_from(per).unwrap_or(1))
                .unwrap_or(0);
            for _ in 0..h {
                let mut x = w;
                while x > 0 {
                    let group = img
                        .maps
                        .unpack
                        .get(byte(buf, pp)? as usize)
                        .ok_or(ImageError::Malformed("TIFF samples"))?;
                    pp = step(pp, 1);
                    let n = x.min(per);
                    for &pixel in group.iter().take(n as usize) {
                        store(raster, cp, pixel)?;
                        cp = step(cp, 1);
                    }
                    x = x.saturating_sub(n);
                }
                cp = step(cp, toskew);
                pp = step(pp, fromskew);
            }
        }
        Put::Rgb8 | Put::Rgbaa8 | Put::Rgbua8 | Put::Cmyk8 => {
            let fromskew = fromskew.wrapping_mul(spp);
            for _ in 0..h {
                for _ in 0..w {
                    let (r, g, b) = (
                        byte(buf, pp)?,
                        byte(buf, step(pp, 1))?,
                        byte(buf, step(pp, 2))?,
                    );
                    let pixel = match img.put {
                        Put::Rgb8 => pack(r, g, b),
                        // Straight, where libtiff premultiplies: see `Alpha`.
                        Put::Rgbaa8 | Put::Rgbua8 => pack4(r, g, b, byte(buf, step(pp, 3))?),
                        _ => {
                            let k = byte(buf, step(pp, 3))?;
                            pack(ink(k, r), ink(k, g), ink(k, b))
                        }
                    };
                    store(raster, cp, pixel)?;
                    cp = step(cp, 1);
                    pp = step(pp, spp);
                }
                cp = step(cp, toskew);
                pp = step(pp, fromskew);
            }
        }
        Put::Rgb16 | Put::Rgbaa16 | Put::Rgbua16 => {
            // The C's `wp` is a 16-bit pointer skewed in samples; here it is
            // a byte offset, stepped twice as far.
            let sample = spp.wrapping_mul(2);
            let fromskew = fromskew.wrapping_mul(sample);
            for _ in 0..h {
                for _ in 0..w {
                    let r = to8(word(buf, pp)?);
                    let g = to8(word(buf, step(pp, 2))?);
                    let b = to8(word(buf, step(pp, 4))?);
                    let pixel = match img.put {
                        Put::Rgb16 => pack(r, g, b),
                        _ => pack4(r, g, b, to8(word(buf, step(pp, 6))?)),
                    };
                    store(raster, cp, pixel)?;
                    cp = step(cp, 1);
                    pp = step(pp, sample);
                }
                cp = step(cp, toskew);
                pp = step(pp, fromskew);
            }
        }
        _ => return Err(ImageError::Unsupported("TIFF sample layout")),
    }
    Ok(())
}

/// A separate-plane routine (`DECLARESepPutFunc`): red, green, blue and
/// alpha planes at `planes` in `buf`, each advanced as the C advances its
/// pointer.
fn put_separate(
    img: &Image,
    raster: &mut [u32],
    span: Span,
    buf: &[u8],
    planes: [isize; 4],
) -> ImageResult<()> {
    let Span {
        mut cp,
        w,
        h,
        fromskew,
        toskew,
    } = span;
    let [mut r, mut g, mut b, mut a] = planes;
    // A 16-bit plane's cursor steps two bytes a sample, and is skewed in
    // samples.
    let sample: isize = if matches!(img.put, Put::SepRgb16 | Put::SepRgbaa16 | Put::SepRgbua16) {
        2
    } else {
        1
    };
    let fromskew = fromskew.wrapping_mul(sample);
    let with_alpha = matches!(
        img.put,
        Put::SepRgbaa8 | Put::SepRgbua8 | Put::SepCmyk8 | Put::SepRgbaa16 | Put::SepRgbua16
    );
    for _ in 0..h {
        for _ in 0..w {
            let pixel = match img.put {
                Put::SepRgb8 => pack(byte(buf, r)?, byte(buf, g)?, byte(buf, b)?),
                Put::SepRgbaa8 | Put::SepRgbua8 => {
                    pack4(byte(buf, r)?, byte(buf, g)?, byte(buf, b)?, byte(buf, a)?)
                }
                Put::SepCmyk8 => {
                    let k = byte(buf, a)?;
                    pack4(
                        ink(k, byte(buf, r)?),
                        ink(k, byte(buf, g)?),
                        ink(k, byte(buf, b)?),
                        255,
                    )
                }
                Put::SepRgb16 => pack(to8(word(buf, r)?), to8(word(buf, g)?), to8(word(buf, b)?)),
                Put::SepRgbaa16 | Put::SepRgbua16 => pack4(
                    to8(word(buf, r)?),
                    to8(word(buf, g)?),
                    to8(word(buf, b)?),
                    to8(word(buf, a)?),
                ),
                _ => return Err(ImageError::Unsupported("TIFF sample layout")),
            };
            store(raster, cp, pixel)?;
            cp = step(cp, 1);
            r = step(r, sample);
            g = step(g, sample);
            b = step(b, sample);
            if with_alpha {
                a = step(a, sample);
            }
        }
        r = step(r, fromskew);
        g = step(g, fromskew);
        b = step(b, fromskew);
        a = step(a, fromskew);
        cp = step(cp, toskew);
    }
    Ok(())
}

fn size(v: Option<u64>) -> ImageResult<usize> {
    v.and_then(|n| usize::try_from(n).ok())
        .ok_or(ImageError::Malformed("TIFF strip size"))
}

fn cursor(v: usize) -> ImageResult<isize> {
    isize::try_from(v).map_err(|_| ImageError::Malformed("TIFF picture too large to index"))
}

/// The strip holding `row` of `plane` (`TIFFComputeStrip`).
fn strip_of(dir: &Directory, row: u32, plane: u32) -> u32 {
    let strip = row.checked_div(dir.rows_per_strip).unwrap_or(0);
    if dir.planar_config == 2 {
        strip.wrapping_add(plane.wrapping_mul(dir.strips_per_image))
    } else {
        strip
    }
}

/// How many bytes strip `index` decodes to (`TIFFReadEncodedStripGetStripSize`).
fn strip_bytes(dir: &Directory, index: u32) -> ImageResult<usize> {
    let rps = dir.rows_per_strip.min(dir.length);
    if rps == 0 {
        return Err(ImageError::Malformed("TIFF RowsPerStrip"));
    }
    let per_plane = dir.length.div_ceil(rps);
    let in_plane = index.checked_rem(per_plane).unwrap_or(0);
    let rows = dir
        .length
        .saturating_sub(in_plane.saturating_mul(rps))
        .min(rps);
    size(dir.strip_size_rows(rows))
}

/// The rows the next read covers: the rest of the strip or tile that
/// `row` is in, or of the picture, whichever ends first.
fn rows_from(row: u32, per: u32, h: u32) -> u32 {
    let into = row.checked_rem(per).unwrap_or(0);
    per.saturating_sub(into).min(h.saturating_sub(row))
}

/// Byte offset of `row` within its strip or tile.
fn row_offset(row: u32, per: u32, row_size: usize) -> ImageResult<usize> {
    (row.checked_rem(per).unwrap_or(0) as usize)
        .checked_mul(row_size)
        .ok_or(ImageError::Malformed("TIFF strip size"))
}

/// Raster index of pixel (`x`, `y`).
fn pixel_at(w: u32, x: u32, y: u32) -> ImageResult<isize> {
    let at = (y as usize)
        .checked_mul(w as usize)
        .and_then(|n| n.checked_add(x as usize));
    cursor(at.ok_or(ImageError::Malformed("TIFF picture too large to index"))?)
}

/// `gtStripContig`, unflipped.
fn strips_contig(
    img: &Image,
    dir: &Directory,
    reader: &mut Reader<'_>,
    raster: &mut [u32],
) -> ImageResult<()> {
    let sub_v = u32::from(dir.ycbcr_subsampling[1]);
    if sub_v == 0 {
        return Err(ImageError::Malformed("TIFF YCbCr vertical subsampling"));
    }
    let max_strip = size(dir.strip_size())?;
    let rps = dir.rows_per_strip;
    let scanline = size(dir.scanline_size())?;
    let (w, h) = (img.width, img.height);
    let mut buf: Vec<u8> = Vec::new();
    let mut row = 0u32;
    while row < h {
        let nrow = rows_from(row, rps, h);
        // Whole blocks of subsampled rows, even when the picture ends
        // partway through one.
        let nrowsub = nrow.div_ceil(sub_v).saturating_mul(sub_v);
        let temp = row.checked_rem(rps).unwrap_or(0).saturating_add(nrowsub);
        let want = (temp as usize)
            .checked_mul(scanline)
            .ok_or(ImageError::Malformed("TIFF strip size"))?;
        let strip = strip_of(dir, row, 0);
        let this = strip_bytes(dir, strip)?.min(want);
        if buf.is_empty() {
            buf = vec![0u8; max_strip];
        }
        reader.decode(strip, &mut buf, this, None)?;
        let span = Span {
            cp: pixel_at(w, 0, row)?,
            w,
            h: nrow,
            fromskew: 0,
            toskew: 0,
        };
        put_contig(
            img,
            raster,
            span,
            &buf,
            cursor(row_offset(row, rps, scanline)?)?,
        )?;
        row = row.saturating_add(nrow);
    }
    Ok(())
}

/// The planes a separate-plane read takes, in the C's order, and where each
/// goes in the buffer: red, green and blue (or grey once), then alpha --
/// or, for CMYK, K in alpha's place.
fn planes_read(img: &Image) -> Vec<(u32, usize)> {
    let colours = separate_colours(img);
    let mut planes = vec![(0u32, 0usize)];
    if colours > 1 {
        planes.extend([(1, 1), (2, 2)]);
    }
    if img.alpha != 0 {
        planes.push((colours, 3));
    }
    planes
}

/// How many colour planes the separate readers take: one for grey, else
/// three.
fn separate_colours(img: &Image) -> u32 {
    match img.photometric {
        photometric::MIN_IS_WHITE | photometric::MIN_IS_BLACK | photometric::PALETTE => 1,
        _ => 3,
    }
}

/// Where the four plane pointers start in a separate reader's buffer of
/// `chunk`-byte planes: grey reads its one plane as red, green and blue.
fn plane_bases(img: &Image, chunk: usize, pos: usize) -> ImageResult<[isize; 4]> {
    let at = |slot: usize| -> ImageResult<isize> {
        let slot = if separate_colours(img) == 1 && slot < 3 {
            0
        } else {
            slot
        };
        cursor(
            slot.checked_mul(chunk)
                .and_then(|n| n.checked_add(pos))
                .ok_or(ImageError::Malformed("TIFF strip size"))?,
        )
    };
    Ok([at(0)?, at(1)?, at(2)?, at(3)?])
}

/// `gtStripSeparate`, unflipped.
fn strips_separate(
    img: &Image,
    dir: &Directory,
    reader: &mut Reader<'_>,
    raster: &mut [u32],
) -> ImageResult<()> {
    let strip_size = size(dir.strip_size())?;
    let buf_size = strip_size
        .checked_mul(if img.alpha != 0 { 4 } else { 3 })
        .ok_or(ImageError::Malformed("TIFF strip size"))?;
    let rps = dir.rows_per_strip;
    let scanline = size(dir.scanline_size())?;
    let (w, h) = (img.width, img.height);
    let mut buf: Vec<u8> = Vec::new();
    let mut row = 0u32;
    while row < h {
        let nrow = rows_from(row, rps, h);
        let temp = row.checked_rem(rps).unwrap_or(0).saturating_add(nrow);
        let want = (temp as usize)
            .checked_mul(scanline)
            .ok_or(ImageError::Malformed("TIFF strip size"))?;
        if buf.is_empty() {
            buf = vec![0u8; buf_size];
        }
        for (plane, slot) in planes_read(img) {
            let strip = strip_of(dir, row, plane);
            let this = strip_bytes(dir, strip)?.min(want);
            let at = slot
                .checked_mul(strip_size)
                .ok_or(ImageError::Malformed("TIFF strip size"))?;
            let dest = buf
                .get_mut(at..)
                .ok_or(ImageError::Malformed("TIFF strip size"))?;
            reader.decode(strip, dest, this, None)?;
        }
        let pos = row_offset(row, rps, scanline)?;
        let span = Span {
            cp: pixel_at(w, 0, row)?,
            w,
            h: nrow,
            fromskew: 0,
            toskew: 0,
        };
        put_separate(img, raster, span, &buf, plane_bases(img, strip_size, pos)?)?;
        row = row.saturating_add(nrow);
    }
    Ok(())
}

/// `TIFFComputeTile`, in the C's 32-bit arithmetic.
fn tile_of(dir: &Directory, x: u32, y: u32, plane: u32) -> u32 {
    let dx = if dir.tile_width == u32::MAX {
        dir.width
    } else {
        dir.tile_width
    };
    let dy = if dir.tile_length == u32::MAX {
        dir.length
    } else {
        dir.tile_length
    };
    let dz = if dir.tile_depth == u32::MAX {
        dir.depth
    } else {
        dir.tile_depth
    };
    let (Some(col), Some(line)) = (x.checked_div(dx), y.checked_div(dy)) else {
        return 1;
    };
    if dz == 0 {
        return 1;
    }
    let xpt = dir::howmany32(dir.width, dx);
    let ypt = dir::howmany32(dir.length, dy);
    let zpt = dir::howmany32(dir.depth, dz);
    let across = xpt.wrapping_mul(line).wrapping_add(col);
    if dir.planar_config == 2 {
        xpt.wrapping_mul(ypt)
            .wrapping_mul(zpt)
            .wrapping_mul(plane)
            .wrapping_add(across)
    } else {
        across
    }
}

/// `TIFFCheckTile`.
fn check_tile(dir: &Directory, x: u32, y: u32, plane: u32) -> ImageResult<()> {
    if x >= dir.width
        || y >= dir.length
        || (dir.planar_config == 2 && plane >= u32::from(dir.samples_per_pixel))
    {
        return Err(ImageError::Malformed("TIFF tile out of range"));
    }
    Ok(())
}

/// A tile loop's geometry: the tile size, and `w - tw`, the skew back to
/// the next row of the raster after a whole tile's row.
struct Tiles {
    tw: u32,
    th: u32,
    toskew: isize,
    row_size: usize,
}

impl Tiles {
    fn new(dir: &Directory, w: u32) -> ImageResult<Self> {
        let (tw, th) = (dir.tile_width, dir.tile_length);
        if tw == 0 || th == 0 {
            return Err(ImageError::Malformed("TIFF tile of no size"));
        }
        let toskew = isize::try_from(i64::from(w).wrapping_sub(i64::from(tw)))
            .map_err(|_| ImageError::Malformed("TIFF tile width"))?;
        Ok(Self {
            tw,
            th,
            toskew,
            row_size: size(dir.tile_row_size())?,
        })
    }

    /// The span a tile at column `tocol` fills: whole, or cut at the
    /// picture's right edge, skipping the rest of each tile row.
    fn span(&self, w: u32, tocol: u32, row: u32, nrow: u32) -> ImageResult<(Span, u32)> {
        let cp = pixel_at(w, tocol, row)?;
        if tocol.saturating_add(self.tw) > w {
            let this_tw = w.saturating_sub(tocol);
            let fromskew = isize::try_from(self.tw.saturating_sub(this_tw))
                .map_err(|_| ImageError::Malformed("TIFF tile width"))?;
            Ok((
                Span {
                    cp,
                    w: this_tw,
                    h: nrow,
                    fromskew,
                    toskew: step(self.toskew, fromskew),
                },
                this_tw,
            ))
        } else {
            Ok((
                Span {
                    cp,
                    w: self.tw,
                    h: nrow,
                    fromskew: 0,
                    toskew: self.toskew,
                },
                self.tw,
            ))
        }
    }
}

/// `gtTileContig`, unflipped.
fn tiles_contig(
    img: &Image,
    dir: &Directory,
    reader: &mut Reader<'_>,
    raster: &mut [u32],
) -> ImageResult<()> {
    let tile_size = size(dir.tile_size())?;
    let (w, h) = (img.width, img.height);
    let tiles = Tiles::new(dir, w)?;
    let mut buf: Vec<u8> = Vec::new();
    let mut row = 0u32;
    while row < h {
        let nrow = rows_from(row, tiles.th, h);
        let mut tocol = 0u32;
        while tocol < w {
            check_tile(dir, tocol, row, 0)?;
            let tile = tile_of(dir, tocol, row, 0);
            let first = buf.is_empty();
            if first {
                buf = vec![0u8; tile_size];
            }
            reader.decode(tile, &mut buf, tile_size, first.then_some(tile_size as u64))?;
            let pos = cursor(row_offset(row, tiles.th, tiles.row_size)?)?;
            let (span, done) = tiles.span(w, tocol, row, nrow)?;
            put_contig(img, raster, span, &buf, pos)?;
            tocol = tocol.saturating_add(done);
        }
        row = row.saturating_add(nrow);
    }
    Ok(())
}

/// `gtTileSeparate`, unflipped.
fn tiles_separate(
    img: &Image,
    dir: &Directory,
    reader: &mut Reader<'_>,
    raster: &mut [u32],
) -> ImageResult<()> {
    let tile_size = size(dir.tile_size())?;
    let buf_size = tile_size
        .checked_mul(if img.alpha != 0 { 4 } else { 3 })
        .ok_or(ImageError::Malformed("TIFF tile size"))?;
    let (w, h) = (img.width, img.height);
    let tiles = Tiles::new(dir, w)?;
    let mut buf: Vec<u8> = Vec::new();
    let mut row = 0u32;
    while row < h {
        let nrow = rows_from(row, tiles.th, h);
        let mut tocol = 0u32;
        while tocol < w {
            for (plane, slot) in planes_read(img) {
                check_tile(dir, tocol, row, plane)?;
                let tile = tile_of(dir, tocol, row, plane);
                let first = buf.is_empty();
                if first {
                    buf = vec![0u8; buf_size];
                }
                let at = slot
                    .checked_mul(tile_size)
                    .ok_or(ImageError::Malformed("TIFF tile size"))?;
                let dest = buf
                    .get_mut(at..)
                    .ok_or(ImageError::Malformed("TIFF tile size"))?;
                reader.decode(tile, dest, tile_size, first.then_some(buf_size as u64))?;
            }
            let pos = row_offset(row, tiles.th, tiles.row_size)?;
            let (span, done) = tiles.span(w, tocol, row, nrow)?;
            put_separate(img, raster, span, &buf, plane_bases(img, tile_size, pos)?)?;
            tocol = tocol.saturating_add(done);
        }
        row = row.saturating_add(nrow);
    }
    Ok(())
}
