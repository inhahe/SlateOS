//! JPEG, decoded exactly as libjpeg-turbo decodes it.
//!
//! The format a photograph is almost always in, and the one every other
//! program on a desktop shows through the same library: web browsers, image
//! libraries, GNOME's image loader, Pillow -- all libjpeg-turbo. So "decoding
//! a JPEG correctly" means, in practice, producing libjpeg-turbo's pixels, and
//! this module is a port of its decompressor rather than a decoder written
//! from the standard: the same marker parsing and the same refusals, the same
//! Huffman, progressive and arithmetic entropy decoders with the same
//! behaviour on damaged data, the same integer inverse DCT, the same "fancy"
//! chroma upsampling and fixed-point colour conversion, block smoothing for
//! incomplete progressive data, and reduced-size decoding inside the
//! transform for thumbnails. Each part cites the libjpeg-turbo 3.1.1 file it
//! transcribes.
//!
//! # The parts
//!
//! - [`source`]: the bytes, and past their end the fake end-of-image markers
//!   every libjpeg data source supplies. How a cut-off file decodes -- the
//!   rows that arrived, then grey -- follows from that.
//! - [`marker`]: the datastream's markers (`jdmarker.c`).
//! - [`huffman`], [`arith`]: entropy decoding, sequential and progressive
//!   (`jdhuff.c`, `jdphuff.c`, `jdarith.c`).
//! - [`coef`]: a multi-scan image's coefficients, kept compactly for
//!   thumbnails, and block smoothing (`jdcoefct.c`).
//! - [`idct`]: the accurate integer transform and the reduced ones
//!   (`jidctint.c`, `jidctred.c`).
//! - [`lossless`]: lossless JPEG -- its Huffman decoder, its predictors and
//!   the buffers between them (`jdlhuff.c`, `jdlossls.c`, `jddiffct.c`).
//! - [`upsample`], [`color`]: bringing chroma to full size and converting it
//!   (`jdsample.c`, `jdcolor.c`).
//! - [`decompress`]: the decompression object and its control flow
//!   (`jdapimin.c`, `jdapistd.c`, `jdinput.c`, `jdmaster.c`), with libjpeg's
//!   interface -- header, start, rows, finish -- because TIFF's JPEG
//!   compression drives it step by step and reacts to where it fails.
//!
//! # What this file adds
//!
//! The crate's entry points, and the few decisions libjpeg leaves to its
//! caller, taken as Chrome takes them: which colour space to ask for (RGB for
//! RGB and YCbCr files; CMYK for CMYK and YCCK ones, converted to RGB by
//! Chrome's formula for the inverted CMYK Adobe writes; anything else
//! refused), a limit of 100 scans, and EXIF orientation read as Chrome reads
//! it. What follows the last row of the picture -- libjpeg's
//! `jpeg_finish_decompress` -- is not read: a picture whose every row decoded
//! is shown, whatever is after it.
//!
//! One choice is not Chrome's. A greyscale file is decoded as greyscale and
//! made RGB here, as GNOME's image loader and Pillow do it, where Chrome asks
//! libjpeg for RGB: the pixels are the same for every lossy file, but libjpeg
//! converts nothing in a lossless one, so Chrome cannot show a lossless
//! greyscale JPEG -- the commonest kind, from medical and scientific imaging
//! -- and they can.
//!
//! # Where this and libjpeg-turbo can differ
//!
//! libjpeg-turbo is itself not one decoder: its SIMD builds compute the
//! inverse DCT in 16-bit lanes and its C code in 64-bit integers, and on the
//! rare damaged file whose coefficients overflow 16 bits the two produce
//! different garbage. This follows the C code, which is the reference and is
//! the same on every machine. Lossless JPEG is decoded as libjpeg-turbo 3
//! decodes it through the same 8-bit interface, samples of 2 to 8 bits; wider
//! ones, and arithmetic-coded ones, it refuses, and so does this. And
//! [`Limits`] can refuse a file libjpeg would try to allocate for.

use crate::orientation::Orientation;
use crate::{Image, ImageError, ImageResult, Limits};

mod arith;
mod coef;
mod color;
mod decompress;
mod error;
mod huffman;
mod idct;
mod lossless;
mod marker;
mod source;
mod tables;
mod upsample;

pub(crate) use color::ColorSpace;
pub(crate) use decompress::{Decompress, Headed, RawPlane};
pub(crate) use tables::Tables;

/// Whether `bytes` begins with a JPEG signature.
///
/// `FF D8` is the start-of-image marker, and the third byte is the start of
/// the next marker, which is always `FF`. Checking three rather than two
/// avoids claiming every file that happens to open with `FF D8`.
#[must_use]
pub fn is_jpeg(bytes: &[u8]) -> bool {
    matches!(bytes, [0xFF, 0xD8, 0xFF, ..])
}

/// The most scans a datastream may have before the decode is abandoned, as
/// Chrome's progress monitor (and libtiff's) abandons it: a crafted
/// progressive file can otherwise make a decoder read thousands of scans
/// over one small picture.
const MAX_SCANS: u32 = 100;

/// Decode a JPEG.
///
/// # Errors
///
/// [`ImageError::Malformed`] or [`ImageError::Unsupported`] with libjpeg's
/// reason for a datastream it would not decode, and [`ImageError::TooLarge`]
/// past `limits`.
pub fn decode(bytes: &[u8], limits: Limits) -> ImageResult<Image> {
    Ok(orientation(bytes).apply(decode_at(bytes, limits, 8)?))
}

/// Which way up the picture is shown: its EXIF orientation, read as Chrome
/// reads it -- from the first `APP1` segment before the scan whose payload
/// starts `Exif\0` and is longer than that and its pad byte (see
/// [`crate::orientation`]). As stored if there is none, or none that counts.
#[must_use]
pub fn orientation(bytes: &[u8]) -> Orientation {
    exif(bytes)
        .and_then(crate::orientation::from_exif)
        .unwrap_or_default()
}

/// A big-endian `u16` at `at`.
fn read_u16(bytes: &[u8], at: usize) -> ImageResult<u16> {
    let hi = *bytes.get(at).ok_or(ImageError::Truncated)?;
    let lo = *bytes
        .get(at.saturating_add(1))
        .ok_or(ImageError::Truncated)?;
    Ok(u16::from_be_bytes([hi, lo]))
}

/// The EXIF block's contents (a TIFF structure), if the file has one before
/// its scan.
fn exif(bytes: &[u8]) -> Option<&[u8]> {
    if !is_jpeg(bytes) {
        return None;
    }
    let mut at = 2usize;
    loop {
        // To the next marker, over any fill bytes.
        while *bytes.get(at)? != 0xFF {
            at = at.checked_add(1)?;
        }
        while *bytes.get(at)? == 0xFF {
            at = at.checked_add(1)?;
        }
        let marker = *bytes.get(at)?;
        at = at.checked_add(1)?;
        match marker {
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            // The scan, or the end: no EXIF before it.
            0xDA | 0xD9 => return None,
            _ => {}
        }
        let length = usize::from(read_u16(bytes, at).ok()?);
        let payload = bytes.get(at.checked_add(2)?..at.checked_add(length)?)?;
        at = at.checked_add(length)?;
        if marker == 0xE1 && payload.len() > 6 && payload.starts_with(b"Exif\0") {
            return payload.get(6..);
        }
    }
}

/// [`decode`], with each 8x8 block reconstructed at `block` samples a side.
fn decode_at(bytes: &[u8], limits: Limits, block: usize) -> ImageResult<Image> {
    if !is_jpeg(bytes) {
        return Err(ImageError::UnknownFormat);
    }
    let mut tables = Tables::new();
    let mut jpeg = Decompress::new(bytes, &mut tables);
    jpeg.read_header(true)?;
    let claimed = (jpeg.image_width() as u64).saturating_mul(jpeg.image_height() as u64);
    if claimed > limits.max_pixels {
        return Err(ImageError::TooLarge {
            pixels: claimed,
            limit: limits.max_pixels,
        });
    }
    // Chrome's choice of output -- RGB where libjpeg can make it, CMYK (and
    // its own conversion) for the four-component spaces, and nothing else --
    // except that greyscale is asked for as greyscale and made RGB here, as
    // GNOME's loader and Pillow do: the same pixels, and the only way a
    // lossless greyscale file, which libjpeg will not convert, decodes.
    let space = jpeg.jpeg_color_space();
    let out = match space {
        ColorSpace::Grayscale => ColorSpace::Grayscale,
        ColorSpace::Rgb | ColorSpace::YCbCr => ColorSpace::Rgb,
        ColorSpace::Cmyk | ColorSpace::Ycck => ColorSpace::Cmyk,
        ColorSpace::Unknown => {
            return Err(ImageError::Unsupported(
                "a JPEG of two, or five or more, components",
            ));
        }
    };
    jpeg.set_color_spaces(space, out);
    jpeg.set_block_size(block);
    jpeg.set_max_scans(MAX_SCANS);
    jpeg.start(&limits, None)?;
    let (width, height) = (jpeg.output_width(), jpeg.output_height());
    let mut pixels = alloc::vec![0u32; width.saturating_mul(height)];
    for row in pixels.chunks_exact_mut(width.max(1)) {
        jpeg.read_row_argb(row)?;
    }
    Ok(Image {
        width: u32::try_from(width).map_err(|_| ImageError::Malformed("an impossible width"))?,
        height: u32::try_from(height).map_err(|_| ImageError::Malformed("an impossible height"))?,
        pixels,
    })
}

/// The picture's size as shown -- turned by its EXIF orientation -- without
/// decoding it.
///
/// Walks to the frame header and stops. A thumbnailer needs this to choose how
/// much of the picture to reconstruct, and reading it should not cost what
/// reading the picture costs.
///
/// # Errors
///
/// As [`decode`], for the header it does read.
pub fn dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    Ok(orientation(bytes).shown(stored_dimensions(bytes)?))
}

/// The frame's own width and height, before any turning: read as libjpeg
/// reads the header (`jpeg_read_header`), which is how Chrome sizes a JPEG
/// before decoding it -- so a file whose size this reports is a file whose
/// header [`decode`] gets past, and the other way round.
fn stored_dimensions(bytes: &[u8]) -> ImageResult<(u32, u32)> {
    if !is_jpeg(bytes) {
        return Err(ImageError::UnknownFormat);
    }
    let mut tables = Tables::new();
    let mut jpeg = Decompress::new(bytes, &mut tables);
    jpeg.read_header(true)?;
    let side = |n: usize| u32::try_from(n).map_err(|_| ImageError::Malformed("an impossible size"));
    Ok((side(jpeg.image_width())?, side(jpeg.image_height())?))
}

/// Decode at the smallest size that still covers `max_w` x `max_h`.
///
/// **Scaled during reconstruction, not decoded whole and shrunk**, as
/// libjpeg-turbo scales: each block is transformed at a half, a quarter or an
/// eighth of its size, reading only the coefficients that size needs, so a
/// preview of a 4000x5333 photograph is reconstructed at 500x667 and never
/// allocates the picture whole -- and a progressive one keeps only those
/// coefficients of each block. The caller's exact box is then fitted by
/// averaging what remains, which is at most a 2x reduction. A lossless JPEG
/// has no transform to scale in, and libjpeg decodes it whole whatever was
/// asked; its thumbnail is averaged down from that.
///
/// **Measured**, release build, a 4000x5333 photograph (6.8 MB, 4:2:0): a
/// 128-pixel thumbnail in about 0.18 s and the whole picture in about 0.76 s;
/// at 4:2:2 (8.6 MB), 0.22 s and 1.18 s. The decoder this port replaced took
/// 0.45 s and 1.42 s for the first, measured side by side on the same machine
/// the same day (`examples/time_decode.rs` times one file; the comparison was a
/// scratch build of both).
///
/// # Errors
///
/// As [`decode`].
pub fn decode_scaled(bytes: &[u8], limits: Limits, max_w: u32, max_h: u32) -> ImageResult<Image> {
    // The box is for the picture as shown, so for one shown on its side it
    // is turned before the stored picture is fitted into it.
    let turn = orientation(bytes);
    let (max_w, max_h) = turn.shown((max_w, max_h));
    let (width, height) = stored_dimensions(bytes)?;
    let mut block = 8usize;
    if max_w > 0 && max_h > 0 {
        // The smallest power of two whose reconstruction still covers the
        // request in both directions.
        for candidate in [1u32, 2, 4] {
            let at_w = width.saturating_mul(candidate).div_ceil(8);
            let at_h = height.saturating_mul(candidate).div_ceil(8);
            if at_w >= max_w && at_h >= max_h {
                block = candidate as usize;
                break;
            }
        }
    }
    let image = decode_at(bytes, limits, block)?;
    if max_w == 0 || max_h == 0 || (image.width <= max_w && image.height <= max_h) {
        return Ok(turn.apply(image));
    }
    let factor_w = image.width.div_ceil(max_w).max(1);
    let factor_h = image.height.div_ceil(max_h).max(1);
    Ok(turn.apply(box_filter(&image, factor_w.max(factor_h) as usize)))
}

/// Average each `factor` x `factor` square down to one pixel.
///
/// Averaging rather than picking one pixel per square: dropping pixels turns a
/// fine texture into moire, which in a thumbnail grid looks like a picture of
/// something else. The cost is one pass over the image.
// The sums are of four bytes at a time into a `u32` and the divisor is at least
// one, so neither can overflow; the index arithmetic is `saturating_` and every
// access through it is a `get`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "byte sums into u32, divisor >= 1"
)]
fn box_filter(image: &Image, factor: usize) -> Image {
    let factor = factor.max(1);
    let src_w = image.width as usize;
    let src_h = image.height as usize;
    let out_w = src_w.div_ceil(factor).max(1);
    let out_h = src_h.div_ceil(factor).max(1);
    let mut pixels = alloc::vec![0u32; out_w.saturating_mul(out_h)];

    for oy in 0..out_h {
        for ox in 0..out_w {
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for dy in 0..factor {
                for dx in 0..factor {
                    let sx = ox.saturating_mul(factor).saturating_add(dx);
                    let sy = oy.saturating_mul(factor).saturating_add(dy);
                    if sx >= src_w || sy >= src_h {
                        continue;
                    }
                    let Some(pixel) = image
                        .pixels
                        .get(sy.saturating_mul(src_w).saturating_add(sx))
                    else {
                        continue;
                    };
                    r = r.saturating_add((pixel >> 16) & 0xFF);
                    g = g.saturating_add((pixel >> 8) & 0xFF);
                    b = b.saturating_add(pixel & 0xFF);
                    n = n.saturating_add(1);
                }
            }
            let n = n.max(1);
            let pixel = 0xFF00_0000 | ((r / n) << 16) | ((g / n) << 8) | (b / n);
            if let Some(slot) = pixels.get_mut(oy.saturating_mul(out_w).saturating_add(ox)) {
                *slot = pixel;
            }
        }
    }
    Image {
        width: u32::try_from(out_w).unwrap_or(1),
        height: u32::try_from(out_h).unwrap_or(1),
        pixels,
    }
}

#[cfg(test)]
mod tests {
    // The same reasoning the crate's other test modules give: a test that
    // indexes out of range should fail loudly at the line that did it. The
    // defensive lints keep panics out of code that runs on a user's file.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    extern crate std;
    use super::*;

    /// A 24x16 baseline JPEG: two gradients crossed with a hard checker edge.
    ///
    /// Written by a reference encoder, not by this crate. A fixture this crate
    /// produced would only prove it agrees with itself -- the same reason
    /// `testing` exists for PNG, pointed the other way.
    ///
    /// Gradients and a hard edge because a flat field is the one image every
    /// decoder gets right: the DCT has nothing to do with it.
    const FIXTURE: &[u8] = crate::testing::SMALL_JPEG;

    /// What libjpeg-turbo makes of [`FIXTURE`], `RRGGBB` per pixel.
    const EXPECTED: &str = concat!(
        "0000FE0700FF1704FE1B06FF2F00003200004304004803005800FF5F00FF6E04FE7306FF",
        "8800008E00009D0400A10200B100FFB700FFC704FECB07FFDF0000E40000F30500FA0300",
        "0511FF0B11FB150EFA2013FF2E0C023A1208440F074A10005C12FF6112FB6B0DF97713FF",
        "880E039313089E0F07A40F00B410FFBC12FBC60EFAD014FFE00C01EA1207F50E06FC1000",
        "0020F90D26FD191EFF2624FF281F00352500401E024F23085621F86525FF701DFF7E24FF",
        "8120009026009A1E02A92306AF20F8BF25FFC91EFFD623FFD82000E52600F11E01FF2406",
        "0034FA0838FE1732FF242CFB293A04353300452F005331034F34FB6037FF6D32FE7B2CFA",
        "813A049035009F2F01AE3103A934FCB737FEC732FED52CFBD93A03E73300F52F00FF3102",
        "0A42010C40001645011E4B062E3DFA3843FD4248FE4445FF604202633F016D4603774C07",
        "883DFA9243FE9A49FE9E46FDBA4203BD3F02C64503D04B06E03DFAE943FDF249FEF546FD",
        "0458030752001359001D53002E57FF3852FF4659FD4853FA5957045D52026B5900765300",
        "8758FF9151FFA059FDA253FAB45805B75202C45800CD5300DF57FFEA52FFF859FCFA53F9",
        "006700086605176905236400286AFC3565F74268F74E68FF546600606604706A087C6400",
        "806AFB8C65F49C68F8A868FFAE6600BA6604C96807D46400D86BFCE566F5F368F7FF68FF",
        "0177000C7900157301287000287BFF3B79FE4472FF4F74FF5876006478006D7404817100",
        "807AFE9378FD9E73FFAA75FFB27800BD7900C67202D97100D87BFFEC78FDF473FFFF74FF",
        "0388FF0886FF178AFD1B8DFF318400358701448B004A89005A88FF6086FF708BFE748EFF",
        "8B84008F87009D8B00A48900B488FFB985FFC98AFDCD8DFFE28400E78700F68C00FA8900",
        "039AFF0999FA1495FA1D9BFF2F9503399B084497074A97005B99FF629AFB6C96FA769CFF",
        "889603939B089D9607A39600B599FFBB99FAC496F9CF9BFFE09603EB9B08F39706FA9700",
        "00AAF90BAEFD15A7FF22ADFF26AA0033AF003EA7024CAD0854A9F963AEFF6EA8FF7BADFF",
        "7FA9008DB00098A702A5AC06AEA9F9BBAEFEC7A7FFD4ACFFD7A900E5AF00EFA802FEAD06",
        "00BDFA06C0FD15BAFE21B4FA28C30535BD0144B70052BA034DBDFC5EC0FF6DBAFE7AB5FB",
        "80C3048EBD009DB700ABB902A8BDFCB7BFFEC5BAFED3B4FAD7C304E5BC00F4B600FFBA03",
        "0BCA020DC70116CD0120D3062FC5FA39CBFE42D0FE45CDFF62CA0365C7026FCD0378D407",
        "89C5FB92CAFD9CD0FF9ECCFDBCCA03BDC802C8CC03D1D306E1C4FAE9CBFDF4D0FEF5CEFD",
        "04DE0308D90116E0001EDA0030DEFF3AD8FF49E0FD4BDAFA5CDE0460D9026FE10079DA00",
        "8ADEFF94D8FFA2DFFCA5DAFAB6DE04BAD902C6E100D0DA00E2DEFFEAD8FFF8DFFCFCDBFA",
        "00EE000AEE0518F20623EC0028F2FC34ECF643F0F74EF0FF55EE0060EE0471F3097CEC01",
        "82F2FD8EECF49DF0F8A8F0FFB0EE00BAEE05CAF208D5EC00DAF2FCE4EDF4F3F0F7FFF0FF",
        "00FF000BFF0013FC0226FB0126FFFE38FFFD41FCFF4DFEFF55FF0061FF006CFD0481FB02",
        "7EFFFE92FFFD9AFBFFA6FDFFAFFF00BBFF00C5FC03D7FA00D6FFFEE8FFFDF3FCFFFFFEFF"
    );

    fn expected_at(index: usize) -> (u32, u32, u32) {
        let text = &EXPECTED[index * 6..index * 6 + 6];
        let value = u32::from_str_radix(text, 16).unwrap();
        ((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF)
    }

    /// The decoder is libjpeg-turbo, pixel for pixel.
    ///
    /// This used to allow two levels either way -- the difference between
    /// two decoders' inverse DCTs -- and needed that allowance; a port of
    /// libjpeg's own arithmetic needs none.
    #[test]
    fn it_agrees_with_libjpeg_turbo_exactly() {
        let image = decode(FIXTURE, Limits::default()).expect("the fixture decodes");
        assert_eq!((image.width, image.height), (24, 16));
        for (index, &got) in image.pixels.iter().enumerate() {
            let (r, g, b) = expected_at(index);
            assert_eq!(got, 0xFF00_0000 | (r << 16) | (g << 8) | b, "pixel {index}");
        }
    }

    /// A thumbnail request gets a smaller picture, not a refusal.
    #[test]
    fn a_scaled_decode_shrinks_rather_than_refusing() {
        let small = decode_scaled(FIXTURE, Limits::default(), 8, 8).expect("decodes");
        assert!(
            small.width <= 8 && small.height <= 8,
            "got {}x{}",
            small.width,
            small.height
        );
        assert_eq!(small.pixels.len(), (small.width * small.height) as usize);
        assert!(
            small.pixels.iter().all(|p| p >> 24 == 0xFF),
            "every pixel opaque"
        );
    }

    /// A picture already smaller than the bounds comes back at its own size.
    #[test]
    fn a_small_picture_is_not_enlarged() {
        let same = decode_scaled(FIXTURE, Limits::default(), 512, 512).expect("decodes");
        assert_eq!((same.width, same.height), (24, 16), "no pixels invented");
    }

    /// Shrinking averages rather than dropping pixels. The factor straddles
    /// the fixture's 4-pixel checker squares, so a true average produces
    /// mid-tones no dropped-pixel scaler can.
    #[test]
    fn shrinking_averages_rather_than_sampling() {
        let small = decode_scaled(FIXTURE, Limits::default(), 5, 4).expect("decodes");
        let mid_tones = small
            .pixels
            .iter()
            .filter(|p| (40..=215).contains(&(*p & 0xFF)))
            .count();
        assert!(
            mid_tones > 0,
            "every pixel is at one extreme, which is what sampling gives"
        );
    }

    /// Every entry point on the crate dispatches to JPEG, not just `decode`.
    #[test]
    fn every_entry_point_knows_about_jpeg() {
        let limits = Limits::default();
        assert_eq!(
            crate::dimensions(FIXTURE).expect("dimensions dispatches"),
            (24, 16)
        );
        let whole = crate::decode(FIXTURE, limits).expect("decode dispatches");
        assert_eq!((whole.width, whole.height), (24, 16));
        let small = crate::decode_scaled(FIXTURE, limits, 8, 8).expect("decode_scaled dispatches");
        assert!(small.width <= 8 && small.height <= 8);
    }

    /// The caller's byte budget is honoured, not only its pixel budget.
    #[test]
    fn a_tight_byte_budget_is_refused() {
        let limits = Limits {
            max_decompressed_bytes: 8,
            ..Limits::default()
        };
        match decode(FIXTURE, limits) {
            Err(ImageError::TooLarge { limit, .. }) => assert_eq!(limit, 8),
            other => panic!("the byte budget was ignored: {other:?}"),
        }
    }

    /// A progressive frame whose one scan sends DC and all 63 AC
    /// coefficients together is refused, as libjpeg refuses it: no
    /// progressive scan may.
    #[test]
    fn a_baseline_scan_in_a_progressive_frame_is_refused_as_malformed() {
        let mut progressive = FIXTURE.to_vec();
        let at = progressive
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("the fixture is baseline");
        progressive[at + 1] = 0xC2;
        match decode(&progressive, Limits::default()) {
            Err(ImageError::Malformed(why)) => {
                assert!(why.contains("progressive"), "it should say which: {why}");
            }
            other => panic!("a baseline scan cannot be a progressive pass, got {other:?}"),
        }
    }

    /// The signature check does not claim every file starting `FF D8`.
    #[test]
    fn the_signature_wants_three_bytes() {
        assert!(is_jpeg(FIXTURE));
        assert!(!is_jpeg(&[0xFF, 0xD8]));
        assert!(!is_jpeg(&[0xFF, 0xD8, 0x00]));
        assert!(!is_jpeg(&[0x89, b'P', b'N', b'G']));
    }

    /// A file cut off in its tables never reaches a scan, and is refused.
    #[test]
    fn a_file_cut_before_its_scan_is_refused() {
        let start = FIXTURE.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
        for cut in [FIXTURE.len() / 3, start] {
            assert!(
                decode(&FIXTURE[..cut], Limits::default()).is_err(),
                "cut at {cut}"
            );
        }
    }

    /// A file cut off in its scan shows the rows that arrived, then grey, as
    /// every program built on libjpeg shows it.
    #[test]
    fn a_file_cut_in_its_scan_shows_what_arrived_then_grey() {
        let start = FIXTURE.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
        let cut = start + (FIXTURE.len() - start) / 3;
        let image = decode(&FIXTURE[..cut], Limits::default()).expect("the start decodes");
        let whole = decode(FIXTURE, Limits::default()).unwrap();
        assert_eq!((image.width, image.height), (24, 16));
        assert_eq!(image.pixels[0], whole.pixels[0], "the first block arrived");
        let last = *image.pixels.last().unwrap();
        assert_eq!(last, 0xFF80_8080, "the last block did not, and is mid-grey");
    }

    /// A picture past the caller's limit is refused before it is allocated.
    #[test]
    fn a_picture_past_the_limit_is_refused() {
        let limits = Limits {
            max_pixels: 16,
            ..Limits::default()
        };
        match decode(FIXTURE, limits) {
            Err(ImageError::TooLarge { pixels, limit }) => {
                assert_eq!(limit, 16);
                assert_eq!(pixels, 24 * 16);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}
