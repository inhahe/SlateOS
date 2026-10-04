//! Decoding a picture's AV1 frames, and putting its tiles together: libavif
//! 1.3.0's `avifDecoderNextImage` and `avifDecoderDecodeTiles` (`src/read.c`)
//! over its dav1d glue (`src/codec_dav1d.c`), with rav1d -- dav1d in Rust --
//! as the decoder.
//!
//! What comes out is libavif's `avifImage` after decoding: planes of samples,
//! the picture's size, depth, chroma layout, range and colour, and alpha --
//! everything the conversion to RGB (`convert.rs`) reads.
//!
//! The decoder is opened as libavif opens dav1d: one frame of delay, film
//! grain applied, libavif's size limit, the item's operating point and layer
//! selection; libavif's choice of threads is its caller's, and rav1d chooses
//! from the number of CPUs here. A frame is sent, taken, and the decoder
//! drained, exactly as libavif does, so a sample holding more than one frame
//! behaves as it does there. Where libavif would reuse one decoder for all
//! of a grid's tiles it does here too; the pictures are the same either way,
//! since every tile is a key frame. A sequence's decoders are kept from one
//! frame to the next in a [`Codecs`], which the animation (`animation.rs`)
//! holds for as long as it plays.
//!
//! A tile decoded at another size than its `ispe` (or its track's) is
//! brought to that size as libavif brings it ([`scale_tile`], over libyuv's
//! scaling in `scale.rs`). After decoding, Chrome's checks apply: a frame
//! whose size, depth or chroma layout is not the container's is refused, as
//! Chrome refuses it (libavif would take the frame's word).
//!
//! Portions of this file are copyright 2019 Joe Drago, from libavif, and
//! used under its BSD-2-Clause licence: `licenses/libavif-LICENSE.txt`.

use alloc::vec;
use alloc::vec::Vec;

use rav1d::safe::{self, Data, Decoder, Layout, Settings};

use super::scale::{Scaled, scale_plane};
use super::setup::{Grid, Layer, Picture, Tile, TileInput, YuvFormat};
use super::{Error, IMAGE_SIZE_LIMIT};

/// A sample of 8 or 16 bits.
pub(crate) trait Sample: Copy + Default + Into<u32> + 'static {
    /// The plane of this sample size, from a decoded picture.
    fn plane(picture: &safe::Picture, index: usize) -> Option<safe::Plane<Self>>;
    /// `v` as this sample, which it fits.
    fn from_u32(v: u32) -> Self;
}

impl Sample for u8 {
    fn plane(picture: &safe::Picture, index: usize) -> Option<safe::Plane<Self>> {
        picture.plane_u8(index)
    }

    fn from_u32(v: u32) -> Self {
        Self::try_from(v).unwrap_or(Self::MAX)
    }
}

impl Sample for u16 {
    fn plane(picture: &safe::Picture, index: usize) -> Option<safe::Plane<Self>> {
        picture.plane_u16(index)
    }

    fn from_u32(v: u32) -> Self {
        Self::try_from(v).unwrap_or(Self::MAX)
    }
}

/// One plane of samples, packed: `height` rows of `width`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plane<T> {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) samples: Vec<T>,
}

impl<T> Plane<T> {
    /// Row `y`: empty past the last.
    pub(crate) fn row(&self, y: usize) -> &[T] {
        let start = y.saturating_mul(self.width);
        self.samples
            .get(start..start.saturating_add(self.width))
            .unwrap_or_default()
    }

    fn row_mut(&mut self, y: usize) -> &mut [T] {
        let start = y.saturating_mul(self.width);
        let end = start.saturating_add(self.width);
        self.samples.get_mut(start..end).unwrap_or_default()
    }
}

impl<T: Copy + Default> Plane<T> {
    /// A `width` x `height` plane of zeros.
    pub(crate) fn new(width: usize, height: usize) -> Result<Self, Error> {
        let len = width
            .checked_mul(height)
            .ok_or(Error::Parse("AVIF plane size"))?;
        Ok(Self {
            width,
            height,
            samples: vec![T::default(); len],
        })
    }

    /// The `width` x `height` rectangle at (`x`, `y`) -- `avifImageSetViewRect`,
    /// as a copy.
    pub(crate) fn view(&self, x: usize, y: usize, width: usize, height: usize) -> Self {
        let mut samples = Vec::with_capacity(width.saturating_mul(height));
        for row in y..y.saturating_add(height) {
            let source = self.row(row);
            samples.extend(source.iter().skip(x).take(width));
        }
        Self {
            width,
            height,
            samples,
        }
    }
}

impl<T> From<safe::Plane<T>> for Plane<T> {
    fn from(plane: safe::Plane<T>) -> Self {
        Self {
            width: plane.width,
            height: plane.height,
            samples: plane.samples,
        }
    }
}

/// A luma size in chroma samples, rounded up: `(size + shift) >> shift`, as
/// `avifImagePlaneWidth` computes it, for a shift of 0 or 1.
pub(crate) const fn chroma_size(size: u32, shift: u32) -> u32 {
    if shift == 0 { size } else { size.div_ceil(2) }
}

/// A decoded picture: libavif's `avifImage` once its frame is decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Yuv<T> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) depth: u8,
    pub(crate) format: YuvFormat,
    pub(crate) full_range: bool,
    pub(crate) primaries: u16,
    pub(crate) transfer: u16,
    pub(crate) matrix: u16,
    /// Luma, then the two chroma planes (none for 4:0:0).
    pub(crate) planes: [Option<Plane<T>>; 3],
    pub(crate) alpha: Option<Plane<T>>,
    /// The colour was premultiplied by the alpha.
    pub(crate) alpha_premultiplied: bool,
}

impl<T: Sample> Yuv<T> {
    /// The planes' shift right of the luma size: libavif's chroma shift.
    pub(crate) const fn chroma_shift(&self) -> (u32, u32) {
        match self.format {
            YuvFormat::Yuv420 => (1, 1),
            YuvFormat::Yuv422 => (1, 0),
            YuvFormat::Yuv444 | YuvFormat::Yuv400 => (0, 0),
        }
    }

    /// `avifImageSetViewRect` with its checks, then copied: the crop Chrome
    /// applies before converting.
    pub(crate) fn view(&self, x: u32, y: u32, width: u32, height: u32) -> Option<Self> {
        let (sx, sy) = self.chroma_shift();
        if width > self.width
            || height > self.height
            || x > self.width.checked_sub(width)?
            || y > self.height.checked_sub(height)?
        {
            return None;
        }
        if self.format != YuvFormat::Yuv400 && ((x & sx) != 0 || (y & sy) != 0) {
            return None;
        }
        let at = |v: u32| usize::try_from(v).ok();
        let (xu, yu, wu, hu) = (at(x)?, at(y)?, at(width)?, at(height)?);
        let chroma = |plane: &Plane<T>| {
            let (cw, ch) = (at(chroma_size(width, sx))?, at(chroma_size(height, sy))?);
            Some(plane.view(at(x >> sx)?, at(y >> sy)?, cw, ch))
        };
        let [luma, u, v] = &self.planes;
        Some(Self {
            width,
            height,
            depth: self.depth,
            format: self.format,
            full_range: self.full_range,
            primaries: self.primaries,
            transfer: self.transfer,
            matrix: self.matrix,
            planes: [
                luma.as_ref().map(|p| p.view(xu, yu, wu, hu)),
                match u {
                    Some(p) => Some(chroma(p)?),
                    None => None,
                },
                match v {
                    Some(p) => Some(chroma(p)?),
                    None => None,
                },
            ],
            alpha: self.alpha.as_ref().map(|p| p.view(xu, yu, wu, hu)),
            alpha_premultiplied: self.alpha_premultiplied,
        })
    }
}

/// A decoded picture, 8-bit or deeper.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decoded {
    Eight(Yuv<u8>),
    Deep(Yuv<u16>),
}

impl Decoded {
    /// Width and height.
    pub(crate) const fn size(&self) -> (u32, u32) {
        match self {
            Self::Eight(image) => (image.width, image.height),
            Self::Deep(image) => (image.width, image.height),
        }
    }

    /// [`Yuv::view`].
    pub(crate) fn view(&self, x: u32, y: u32, width: u32, height: u32) -> Option<Self> {
        match self {
            Self::Eight(image) => image.view(x, y, width, height).map(Self::Eight),
            Self::Deep(image) => image.view(x, y, width, height).map(Self::Deep),
        }
    }
}

/// Tiles of fewer pixels than this decode on the calling thread alone.
///
/// Starting a decoder's worker threads costs milliseconds -- measured here,
/// 0.4 ms to decode a 1x1 picture on one thread against 6 ms with a worker per
/// CPU -- and a still picture gives them little to share: dav1d parallelises
/// a frame by its AV1 tiles and by rows of its loop filters, and an encoder
/// splits a picture into AV1 tiles only when it is large. Below about two
/// megapixels the threads cost more than they save (Pillow's dav1d, with its
/// assembly, is slower with them too on the same files).
const THREADED_PIXELS: u32 = 1 << 21;

/// libavif's decoder settings for a tile: `dav1dCodecGetNextImage`'s.
fn settings(tile: &Tile) -> Settings {
    let pixels = tile.width.saturating_mul(tile.height);
    Settings {
        // libavif takes its caller's choice (Pillow passes its CPU count);
        // the pictures are the same whatever it is. 0 lets rav1d use every
        // CPU.
        threads: if pixels < THREADED_PIXELS { 1 } else { 0 },
        max_frame_delay: 1,
        apply_grain: true,
        operating_point: tile.operating_point,
        all_layers: tile.all_layers,
        frame_size_limit: IMAGE_SIZE_LIMIT,
    }
}

/// Why rav1d failed, as libavif reports it: `AVIF_RESULT_DECODE_COLOR_FAILED`
/// or `..._ALPHA_FAILED`.
fn decode_failed(alpha: bool) -> Error {
    if alpha {
        Error::Decode("AVIF alpha plane")
    } else {
        Error::Decode("AVIF picture")
    }
}

/// `dav1dCodecGetNextImage`'s loop: send the sample, take pictures until one
/// of the wanted layer, and drain the rest.
fn next_picture(
    decoder: &mut Decoder,
    sample: &[u8],
    spatial_id: Option<u8>,
    alpha: bool,
) -> Result<safe::Picture, Error> {
    let failed = decode_failed(alpha);
    let mut data = Data::new(sample).map_err(|_| failed)?;
    let picture = loop {
        if !data.is_consumed() {
            match decoder.send(&mut data) {
                Ok(()) | Err(safe::Error::Again) => {}
                Err(_) => return Err(failed),
            }
        }
        match decoder.picture() {
            Ok(picture) => {
                if spatial_id.is_some_and(|id| picture.spatial_id() != Some(id)) {
                    continue; // another layer: dropped
                }
                break picture;
            }
            Err(safe::Error::Again) if !data.is_consumed() => {}
            Err(_) => return Err(failed),
        }
    };
    // Drain: a frame after the one wanted must not come out of the next
    // sample's decode.
    loop {
        match decoder.picture() {
            Ok(_) => {}
            Err(safe::Error::Again) => break,
            Err(_) => return Err(failed),
        }
    }
    Ok(picture)
}

const fn format_of(layout: Layout) -> YuvFormat {
    match layout {
        Layout::I400 => YuvFormat::Yuv400,
        Layout::I420 => YuvFormat::Yuv420,
        Layout::I422 => YuvFormat::Yuv422,
        Layout::I444 => YuvFormat::Yuv444,
    }
}

/// One decoded tile: its `avifImage`.
struct DecodedTile<T> {
    width: u32,
    height: u32,
    depth: u8,
    format: YuvFormat,
    full_range: bool,
    colour: Option<safe::Colour>,
    planes: [Option<Plane<T>>; 3],
}

/// The picture's layers in libavif's order: colour, then alpha if there is
/// one; `true` marks alpha.
pub(crate) fn layers<'p>(picture: &'p Picture<'_>) -> Vec<(&'p Layer, bool)> {
    core::iter::once((&picture.color, false))
        .chain(picture.alpha.iter().map(|a| (a, true)))
        .collect()
}

/// The AV1 decoders of a picture, as libavif makes its codecs
/// (`avifDecoderCreateCodecs`): one for every tile, or one for them all when
/// libavif shares one (`avifTilesCanBeDecodedWithSameCodecInstance`). Each is
/// opened with its tile's settings when its first frame is decoded.
///
/// Those that must outlive a frame are kept here: the shared one, and each of
/// a sequence's, whose frames after a key frame are predicted from the frames
/// before -- so a sequence is decoded frame after frame through one `Codecs`,
/// and dropping it (libavif's `avifDecoderFlush`) is how decoding restarts at a
/// key frame. A still picture's unshared tiles are each decoded once, so each
/// of their decoders is dropped when its tile is done, where libavif keeps
/// them all until the picture is: a grid may have 65,536 tiles, and a decoder
/// apiece at once would be a great deal of memory to spend for no difference
/// in the pixels.
pub(crate) struct Codecs {
    shared: bool,
    /// The shared decoder at 0, or each sequence tile's at its position
    /// (colour tiles first, then alpha); empty for a still picture's
    /// unshared tiles.
    kept: Vec<Option<Decoder>>,
}

impl Codecs {
    pub(crate) fn new(picture: &Picture<'_>) -> Self {
        let layers = layers(picture);
        let shared = one_decoder(picture, &layers);
        let tiles = || layers.iter().flat_map(|(l, _)| l.tiles.iter());
        let kept = if shared {
            1
        } else if tiles().any(|t| matches!(t.input, TileInput::Track { .. })) {
            tiles().count()
        } else {
            0
        };
        Self {
            shared,
            kept: (0..kept).map(|_| None).collect(),
        }
    }

    /// Where the decoder of tile `index` (counting colour tiles, then alpha)
    /// is kept, if it is kept.
    fn slot(&mut self, index: usize) -> Option<&mut Option<Decoder>> {
        let index = if self.shared { 0 } else { index };
        self.kept.get_mut(index)
    }
}

/// Decode frame `frame` of `picture` with decoders of its own:
/// `avifDecoderNextImage` for a picture's only frame, or a sequence's first.
pub(crate) fn decode(picture: &Picture<'_>, frame: u32) -> Result<Decoded, Error> {
    // avifDecoderPrepareTiles: every tile's bytes, colour then alpha, before
    // anything is decoded.
    let mut samples = Vec::new();
    for (layer, _) in layers(picture) {
        for tile in &layer.tiles {
            samples.push(picture.sample(tile, frame, 0)?);
        }
    }
    decode_samples(picture, &samples, &mut Codecs::new(picture))
}

/// Decode one frame from its tiles' `samples` (colour tiles, then alpha, as
/// [`layers`] orders them) with `codecs`: the part of `avifDecoderNextImage`
/// after the samples are prepared.
pub(crate) fn decode_samples(
    picture: &Picture<'_>,
    samples: &[alloc::borrow::Cow<'_, [u8]>],
    codecs: &mut Codecs,
) -> Result<Decoded, Error> {
    let layers = layers(picture);
    if picture.depth > 8 {
        decode_as::<u16>(picture, &layers, samples, codecs).map(Decoded::Deep)
    } else {
        decode_as::<u8>(picture, &layers, samples, codecs).map(Decoded::Eight)
    }
}

/// Whether libavif decodes every tile with one decoder
/// (`avifDecoderCreateCodecs`, `avifTilesCanBeDecodedWithSameCodecInstance`).
fn one_decoder(picture: &Picture<'_>, layers: &[(&Layer, bool)]) -> bool {
    let tiles: Vec<&Tile> = layers.iter().flat_map(|(l, _)| l.tiles.iter()).collect();
    if tiles.len() == 1 {
        return true;
    }
    if matches!(
        tiles.first().map(|t| t.input),
        Some(TileInput::Track { .. })
    ) {
        return false; // one for colour, one for alpha
    }
    if picture.frame_count != 1 {
        return false;
    }
    let buffers = layers.iter().filter(|(l, _)| !l.tiles.is_empty()).count();
    let stolen = layers.iter().filter(|(l, _)| l.tiles.len() == 1).count();
    if stolen > 0 && buffers > 1 {
        return false;
    }
    let first = tiles.first().copied();
    tiles.iter().all(|t| {
        first
            .is_some_and(|f| t.operating_point == f.operating_point && t.all_layers == f.all_layers)
    })
}

fn decode_as<T: Scaled>(
    picture: &Picture<'_>,
    layers: &[(&Layer, bool)],
    samples: &[alloc::borrow::Cow<'_, [u8]>],
    codecs: &mut Codecs,
) -> Result<Yuv<T>, Error> {
    if layers.first().and_then(|(l, _)| l.tiles.first()).is_none() {
        return Err(Error::MissingImage);
    }

    let mut image = Yuv::<T> {
        width: picture.width,
        height: picture.height,
        depth: picture.depth,
        format: picture.format,
        full_range: picture.colour.full_range,
        primaries: picture.colour.primaries,
        transfer: picture.colour.transfer,
        matrix: picture.colour.matrix,
        planes: [None, None, None],
        alpha: None,
        alpha_premultiplied: picture.alpha_premultiplied,
    };
    let mut cicp_set = picture.cicp_set;
    let mut sample_index = 0usize;
    for &(layer, alpha) in layers {
        let mut first: Option<TileProps> = None;
        for (tile_index, tile) in layer.tiles.iter().enumerate() {
            let sample = samples.get(sample_index).ok_or(Error::MissingImage)?;
            // The shared decoder, or this tile's -- kept for the next frame
            // if it is a sequence's, and otherwise dropped with the tile.
            let mut own_decoder = None;
            let decoder = match codecs.slot(sample_index) {
                Some(slot) => {
                    if slot.is_none() {
                        *slot =
                            Some(Decoder::new(&settings(tile)).map_err(|_| decode_failed(alpha))?);
                    }
                    slot.as_mut().ok_or_else(|| decode_failed(alpha))?
                }
                None => own_decoder
                    .insert(Decoder::new(&settings(tile)).map_err(|_| decode_failed(alpha))?),
            };
            sample_index = sample_index.saturating_add(1);
            let spatial_id = match tile.input {
                TileInput::Item { spatial_id, .. } => spatial_id,
                TileInput::Track { .. } => None,
            };
            let decoded = next_picture(decoder, sample, spatial_id, alpha)?;
            let mut tile_image = tile_image::<T>(&decoded, alpha)?;
            if alpha && !tile_image.full_range {
                // avifImageLimitedToFullAlpha: version 1.0.0 of the AVIF
                // specification allowed limited-range alpha.
                if let Some(plane) = tile_image.planes[0].as_mut() {
                    for s in &mut plane.samples {
                        let v = limited_to_full_y(
                            tile_image.depth,
                            i32::try_from((*s).into()).unwrap_or(0),
                        );
                        *s = T::from_u32(u32::try_from(v).unwrap_or(0));
                    }
                }
            }
            scale_tile(&mut tile_image, tile.width, tile.height, alpha)?;
            match layer.grid {
                Some(grid) => {
                    if tile_index == 0 {
                        allocate(&mut image, &mut cicp_set, grid, &tile_image, alpha)?;
                        first = Some(TileProps::of(&tile_image, alpha));
                    }
                    let first = first.ok_or(Error::Grid("AVIF grid"))?;
                    copy_tile(&mut image, grid, first, &tile_image, tile_index, alpha)?;
                }
                None => steal(&mut image, tile_image, alpha)?,
            }
        }
    }

    // Chrome's checks: the frame is the container's size, depth and layout.
    if (image.width, image.height) != (picture.width, picture.height) {
        return Err(Error::Decode(
            "AVIF frame of another size than its container",
        ));
    }
    if image.depth != picture.depth {
        return Err(Error::Decode(
            "AVIF frame of another depth than its container",
        ));
    }
    if image.format != picture.format {
        return Err(Error::Decode(
            "AVIF frame of another chroma layout than its container",
        ));
    }
    Ok(image)
}

/// `LIMITED_TO_FULL` for luma (and alpha): `avifLimitedToFullY`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "v is a sample of at most 16 bits and the range constants are fixed, so every sum and product is far inside i32 (saturating besides), and the divisor is a non-zero range"
)]
pub(crate) fn limited_to_full_y(depth: u8, v: i32) -> i32 {
    let (min, max, full) = match depth {
        8 => (16, 235, 255),
        10 => (64, 940, 1023),
        12 => (256, 3760, 4095),
        _ => return v,
    };
    let range = max - min;
    let v = (v - min).saturating_mul(full).saturating_add(range / 2) / range;
    v.clamp(0, full)
}

/// A decoded frame as libavif's codec glue fills a tile's `avifImage`.
fn tile_image<T: Sample>(picture: &safe::Picture, alpha: bool) -> Result<DecodedTile<T>, Error> {
    let failed = decode_failed(alpha);
    let (width, height) = picture.size();
    let depth = picture.bit_depth();
    let colour = picture.colour();
    let full_range = colour.is_some_and(|c| c.full_range);
    let format = format_of(picture.layout());
    let plane = |i| T::plane(picture, i).map(Plane::from);
    // An alpha tile gives its luma only, as does a grey one.
    let planes = if alpha || format == YuvFormat::Yuv400 {
        [Some(plane(0).ok_or(failed)?), None, None]
    } else {
        [
            Some(plane(0).ok_or(failed)?),
            Some(plane(1).ok_or(failed)?),
            Some(plane(2).ok_or(failed)?),
        ]
    };
    Ok(DecodedTile {
        width,
        height,
        depth,
        format,
        full_range,
        colour,
        planes,
    })
}

/// `avifImageScaleWithLimit`, as `avifDecoderDecodeTiles` calls it for a
/// tile decoded at another size than its `ispe` (or its track's): every
/// plane brought to `width` x `height` -- chroma to that size's chroma size
/// -- by libyuv's scaling (`scale.rs`).
///
/// Refused as libavif refuses it, which turns its refusal into a failure to
/// decode the tile: a size of 0, a size past libavif's limits, or a decoded
/// frame over 16384 on a side, which libyuv's fixed point could overflow.
fn scale_tile<T: Scaled>(
    tile: &mut DecodedTile<T>,
    width: u32,
    height: u32,
    alpha: bool,
) -> Result<(), Error> {
    if (tile.width, tile.height) == (width, height) {
        return Ok(());
    }
    let failed = decode_failed(alpha);
    if width == 0 || height == 0 || super::too_large(width, height) {
        return Err(failed);
    }
    if tile.width > 16384 || tile.height > 16384 {
        return Err(failed);
    }
    let (sx, sy) = match tile.format {
        YuvFormat::Yuv420 => (1, 1),
        YuvFormat::Yuv422 => (1, 0),
        YuvFormat::Yuv444 | YuvFormat::Yuv400 => (0, 0),
    };
    let at = |v: u32| usize::try_from(v).map_err(|_| failed);
    // Luma or alpha at the new size; chroma at its own.
    let sizes = [
        (width, height),
        (chroma_size(width, sx), chroma_size(height, sy)),
        (chroma_size(width, sx), chroma_size(height, sy)),
    ];
    for (plane, (w, h)) in tile.planes.iter_mut().zip(sizes) {
        if let Some(p) = plane {
            *p = scale_plane(p, at(w)?, at(h)?).map_err(|_| failed)?;
        }
    }
    tile.width = width;
    tile.height = height;
    Ok(())
}

/// The single-tile case of `avifDecoderDecodeTiles`: the image takes the
/// tile's planes.
fn steal<T: Sample>(image: &mut Yuv<T>, tile: DecodedTile<T>, alpha: bool) -> Result<(), Error> {
    if (image.width, image.height, image.depth) != (tile.width, tile.height, tile.depth) {
        if alpha {
            return Err(Error::Decode("AVIF alpha plane unlike its picture"));
        }
        image.width = tile.width;
        image.height = tile.height;
        image.depth = tile.depth;
    }
    let [luma, u, v] = tile.planes;
    if alpha {
        image.alpha = luma;
    } else {
        image.planes = [luma, u, v];
        image.format = tile.format;
    }
    Ok(())
}

/// `avifDecoderDataAllocateImagePlanes`: a grid's first tile sets the image
/// up, after the grid rules are checked against it.
fn allocate<T: Sample>(
    image: &mut Yuv<T>,
    cicp_set: &mut bool,
    grid: Grid,
    tile: &DecodedTile<T>,
    alpha: bool,
) -> Result<(), Error> {
    let grid_error = Error::Grid("AVIF grid tiles");
    // Products of two u32s, which a u64 holds: libavif's are 32-bit and could
    // wrap, but the sizes reaching here are limited to 16384 x 16384 first.
    let span = |tile: u32, count: u32| u64::from(tile).saturating_mul(u64::from(count));
    let covers = span(tile.width, grid.columns) >= u64::from(grid.output_width)
        && span(tile.height, grid.rows) >= u64::from(grid.output_height);
    if !covers {
        return Err(Error::Grid("AVIF grid tiles that do not cover the picture"));
    }
    let overlaps = span(tile.width, grid.columns.saturating_sub(1)) < u64::from(grid.output_width)
        && span(tile.height, grid.rows.saturating_sub(1)) < u64::from(grid.output_height);
    if !overlaps {
        return Err(Error::Grid("AVIF grid tiles past the picture"));
    }
    // An alpha tile has no layout here (`AVIF_PIXEL_FORMAT_NONE`), so only its
    // size is checked.
    let format = if alpha { None } else { Some(tile.format) };
    if !grid_dimensions_valid(
        format,
        grid.output_width,
        grid.output_height,
        tile.width,
        tile.height,
    ) {
        return Err(grid_error);
    }
    let differs = (image.width, image.height, image.depth)
        != (grid.output_width, grid.output_height, tile.depth);
    let format_differs = !alpha && image.format != tile.format;
    if differs || format_differs {
        if alpha {
            return Err(Error::Grid("AVIF alpha grid unlike its picture"));
        }
        if differs {
            image.width = grid.output_width;
            image.height = grid.output_height;
            image.depth = tile.depth;
        }
        if format_differs {
            image.format = tile.format;
        }
        if !*cicp_set {
            *cicp_set = true;
            if let Some(colour) = tile.colour {
                image.primaries = u16::from(colour.primaries);
                image.transfer = u16::from(colour.transfer);
                image.matrix = u16::from(colour.matrix);
            }
        }
    }
    // avifImageAllocatePlanes.
    let width = usize::try_from(image.width).map_err(|_| grid_error)?;
    let height = usize::try_from(image.height).map_err(|_| grid_error)?;
    if alpha {
        image.alpha = Some(Plane::new(width, height)?);
    } else {
        let (sx, sy) = image.chroma_shift();
        let cw = usize::try_from(chroma_size(image.width, sx)).map_err(|_| grid_error)?;
        let ch = usize::try_from(chroma_size(image.height, sy)).map_err(|_| grid_error)?;
        image.planes = if image.format == YuvFormat::Yuv400 {
            [Some(Plane::new(width, height)?), None, None]
        } else {
            [
                Some(Plane::new(width, height)?),
                Some(Plane::new(cw, ch)?),
                Some(Plane::new(cw, ch)?),
            ]
        };
    }
    Ok(())
}

/// `avifAreGridDimensionsValid`: tiles at least 64 square, and even where
/// the chroma is subsampled.
fn grid_dimensions_valid(
    format: Option<YuvFormat>,
    image_w: u32,
    image_h: u32,
    tile_w: u32,
    tile_h: u32,
) -> bool {
    if tile_w < 64 || tile_h < 64 {
        return false;
    }
    let horizontal = matches!(format, Some(YuvFormat::Yuv420 | YuvFormat::Yuv422));
    let vertical = format == Some(YuvFormat::Yuv420);
    let even = |v: u32| v.is_multiple_of(2);
    !((horizontal && !(even(image_w) && even(tile_w)))
        || (vertical && !(even(image_h) && even(tile_h))))
}

/// What a grid's first tile fixes for the rest: the fields
/// `avifDecoderDataCopyTileToImage` compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TileProps {
    width: u32,
    height: u32,
    depth: u8,
    /// `None` for an alpha tile: libavif's `AVIF_PIXEL_FORMAT_NONE`.
    format: Option<YuvFormat>,
    full_range: bool,
    cicp: Option<(u8, u8, u8)>,
}

impl TileProps {
    /// The fields as libavif's tile image holds them. dav1d's glue fills in
    /// the chroma layout, range and colour of a colour tile only; an alpha
    /// tile's stay at `avifImageSetDefaults`' (no layout, full range, all
    /// unspecified), so alpha tiles are compared on size and depth alone.
    fn of<T>(tile: &DecodedTile<T>, alpha: bool) -> Self {
        if alpha {
            return Self {
                width: tile.width,
                height: tile.height,
                depth: tile.depth,
                format: None,
                full_range: true,
                cicp: None,
            };
        }
        Self {
            width: tile.width,
            height: tile.height,
            depth: tile.depth,
            format: Some(tile.format),
            full_range: tile.full_range,
            cicp: tile.colour.map(|c| (c.primaries, c.transfer, c.matrix)),
        }
    }
}

/// `avifDecoderDataCopyTileToImage`: a tile's samples into its place in the
/// grid, after checking it matches the first tile.
fn copy_tile<T: Sample>(
    image: &mut Yuv<T>,
    grid: Grid,
    first: TileProps,
    tile: &DecodedTile<T>,
    index: usize,
    alpha: bool,
) -> Result<(), Error> {
    if TileProps::of(tile, alpha) != first {
        return Err(Error::Grid("AVIF grid of mismatched tiles"));
    }
    let bad = Error::Grid("AVIF grid");
    let at = |v: u32| usize::try_from(v).map_err(|_| bad);
    let columns = at(grid.columns)?.max(1);
    let row = index.checked_div(columns).ok_or(bad)?;
    let column = index.checked_rem(columns).ok_or(bad)?;
    let (tile_w, tile_h) = (at(first.width)?, at(first.height)?);
    let (out_w, out_h) = (at(grid.output_width)?, at(grid.output_height)?);
    let x = tile_w.checked_mul(column).ok_or(bad)?;
    let y = tile_h.checked_mul(row).ok_or(bad)?;
    let width = tile_w.min(out_w.saturating_sub(x));
    let height = tile_h.min(out_h.saturating_sub(y));
    let copy = |dst: &mut Plane<T>,
                src: &Plane<T>,
                x: usize,
                y: usize,
                w: usize,
                h: usize|
     -> Result<(), Error> {
        for r in 0..h {
            let from = src.row(r).get(..w).ok_or(bad)?;
            let to = dst
                .row_mut(y.checked_add(r).ok_or(bad)?)
                .get_mut(x..x.checked_add(w).ok_or(bad)?)
                .ok_or(bad)?;
            to.copy_from_slice(from);
        }
        Ok(())
    };
    if alpha {
        let (Some(dst), Some(src)) = (image.alpha.as_mut(), tile.planes[0].as_ref()) else {
            return Err(bad);
        };
        return copy(dst, src, x, y, width, height);
    }
    let (sx, sy) = image.chroma_shift();
    let (sxu, syu) = (at(sx)?, at(sy)?);
    for (index, (plane, source)) in image.planes.iter_mut().zip(&tile.planes).enumerate() {
        let (Some(dst), Some(src)) = (plane.as_mut(), source.as_ref()) else {
            continue;
        };
        if index == 0 {
            copy(dst, src, x, y, width, height)?;
        } else {
            // The view's chroma: its origin shifted, its size rounded up.
            let cw = if sxu == 0 { width } else { width.div_ceil(2) };
            let ch = if syu == 0 { height } else { height.div_ceil(2) };
            copy(dst, src, x >> sx, y >> sy, cw, ch)?;
        }
    }
    Ok(())
}

/// How fast AVIF decodes: the measurements `THREADED_PIXELS` and the `-O3`
/// profile for rav1d and this crate (the workspace `Cargo.toml`) rest on, and
/// the yardstick for making rav1d faster.
///
/// Ignored in an ordinary run: timing is only meaningful in a release build,
/// and it is a measurement, not a check. Run it as
/// `cargo test -p imagecodec --release --lib -- --ignored --nocapture bench_avif`.
#[cfg(test)]
mod bench {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "a benchmark over committed inputs, which fails loudly"
    )]

    extern crate std;

    use std::time::{Duration, Instant};

    use super::{Decoder, next_picture, settings};
    use crate::Limits;
    use crate::avif::setup::Picture;

    /// Each figure is the best of this many runs: the least disturbed by
    /// whatever else the machine was doing, which on a machine shared with
    /// other builds is most of the noise.
    const RUNS: usize = 5;

    fn best(mut run: impl FnMut() -> Duration) -> Duration {
        (0..RUNS).map(|_| run()).min().unwrap_or_default()
    }

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    /// The inputs are `tests/data/generate_avif_bench.py`'s: photograph-like
    /// pictures at sizes either side of `THREADED_PIXELS`, and one deep.
    #[test]
    #[ignore = "measurement benchmark; run explicitly with --release --ignored --nocapture"]
    fn bench_avif_decode() {
        std::println!(
            "{:<28} {:>12} {:>12} {:>12}",
            "picture",
            "av1 1 thread",
            "av1 all",
            "as shipped"
        );
        for name in [
            "avifbench_8_420_640x480",
            "avifbench_8_420_1920x1080",
            "avifbench_8_420_2560x1440",
            "avifbench_10_444_1920x1080",
        ] {
            let path = std::format!("{}/tests/data/{name}.avif", env!("CARGO_MANIFEST_DIR"));
            let bytes = std::fs::read(&path).unwrap();
            let picture = Picture::read(&bytes).unwrap();
            let tile = picture.color.tiles[0];
            let sample = picture.sample(&tile, 0, 0).unwrap();
            // The AV1 frame alone, through a decoder made for it as `decode`
            // makes one -- its start-up included, since a still picture pays
            // it every time -- at one thread and at every CPU.
            let av1 = |threads: u32| {
                best(|| {
                    let mut chosen = settings(&tile);
                    chosen.threads = threads;
                    let start = Instant::now();
                    let mut decoder = Decoder::new(&chosen).unwrap();
                    next_picture(&mut decoder, &sample, None, false).unwrap();
                    start.elapsed()
                })
            };
            let (one, all) = (av1(1), av1(0));
            // And the whole of it as shipped: the container, the frame at the
            // threads THREADED_PIXELS picks, and the conversion to pixels.
            let shipped = best(|| {
                let start = Instant::now();
                crate::avif::decode(&bytes, Limits::default()).unwrap();
                start.elapsed()
            });
            std::println!(
                "{name:<28} {:>9.1} ms {:>9.1} ms {:>9.1} ms",
                ms(one),
                ms(all),
                ms(shipped)
            );
        }
    }
}
