//! What a parsed AVIF file *is*: which item or track is the picture, which is
//! its alpha, how each splits into tiles to decode, and the size, depth and
//! colour libavif gives the result -- libavif 1.3.0's `avifDecoderParse`
//! after the boxes are read (the `ispe` harvest) and `avifDecoderReset`
//! (`src/read.c`), with the decoder settings Chrome and Pillow share.
//!
//! This is where most of libavif's refusals happen that are not about a box's
//! syntax: a primary item that is missing or unusable, a grid whose tiles do
//! not add up, an alpha plane split differently from its picture, a gain map
//! whose metadata does not validate (refused even though the gain map itself
//! is never decoded). They are made here in libavif's order.
//!
//! Where libavif scans every item for every item -- to find each grid tile's
//! alpha, or each tone-mapped item's inputs -- this builds an index first, so
//! a file of a million items costs a million steps, not a trillion; the
//! answers are the same, because the index keeps the items in libavif's
//! order.
//!
//! Portions of this file are copyright 2019 Joe Drago and 2023 Google LLC,
//! from libavif, and used under its BSD-2-Clause licence:
//! `licenses/libavif-LICENSE.txt`.

use alloc::borrow::Cow;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use super::container::{self, Av1Config, Clap, Colr, File, FourCc, Item, Meta, Property};
use super::movie::{Repetition, Sample, Track};
use super::stream::Stream;
use super::{Error, IMAGE_COUNT_LIMIT, obu, too_large};
use crate::{ImageError, ImageResult, Limits};

/// The URNs an `auxC` property names an alpha plane by (`AVIF_URN_ALPHA0`,
/// `AVIF_URN_ALPHA1`).
const ALPHA_URNS: [&[u8]; 2] = [
    b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha",
    b"urn:mpeg:hevc:2015:auxid:1",
];

fn is_alpha_urn(urn: &[u8]) -> bool {
    ALPHA_URNS.contains(&urn)
}

/// How a picture's chroma is sampled, as its `av1C` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YuvFormat {
    Yuv444,
    Yuv422,
    Yuv420,
    /// Monochrome: luma only.
    Yuv400,
}

/// A `grid` item's layout: `avifImageGrid`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Grid {
    pub(crate) rows: u32,
    pub(crate) columns: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
}

impl Grid {
    fn tiles(self) -> usize {
        usize::try_from(self.rows.saturating_mul(self.columns)).unwrap_or(usize::MAX)
    }
}

/// Where a tile's coded frames come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TileInput {
    /// One frame: the first `size` bytes of the item at `position`, and the
    /// AV1 layer to show if the item selects one (`lsel`).
    Item {
        position: usize,
        size: usize,
        spatial_id: Option<u8>,
    },
    /// Every sample of the track at `track`, one frame each.
    Track { track: usize },
}

/// One AV1 bitstream to decode: `avifTile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tile {
    /// The size the decoded frame is scaled to if it is not already: the
    /// item's `ispe`, or the track header's.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// `a1op`'s operating point.
    pub(crate) operating_point: u8,
    /// Whether every layer must be decoded to reach the one shown (`lsel`).
    pub(crate) all_layers: bool,
    pub(crate) input: TileInput,
}

/// The tiles of the picture, or of its alpha, and the grid they form if they
/// form one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Layer {
    pub(crate) grid: Option<Grid>,
    pub(crate) tiles: Vec<Tile>,
}

/// The picture's colour code points (ITU-T H.273) and range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Colour {
    pub(crate) primaries: u16,
    pub(crate) transfer: u16,
    pub(crate) matrix: u16,
    pub(crate) full_range: bool,
}

impl Colour {
    /// `avifImageSetDefaults`: all unspecified, full range.
    const DEFAULT: Self = Self {
        primaries: 2,
        transfer: 2,
        matrix: 2,
        full_range: true,
    };
}

/// A sequence's timing, from its colour track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Timing {
    pub(crate) timescale: u32,
    pub(crate) duration: u64,
    pub(crate) repetition: Repetition,
}

/// A file libavif has parsed and reset: what `avifDecoder` holds after
/// `avifDecoderParse` returns, before the first frame is decoded.
#[derive(Debug)]
pub(crate) struct Picture<'a> {
    pub(crate) file: File<'a>,
    pub(crate) color: Layer,
    pub(crate) alpha: Option<Layer>,
    /// The picture's size before any crop or turn: the colour item's `ispe`
    /// or the colour track's header.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// 8, 10 or 12, from `av1C`.
    pub(crate) depth: u8,
    pub(crate) format: YuvFormat,
    pub(crate) chroma_sample_position: u8,
    pub(crate) colour: Colour,
    /// Whether `colour`'s code points are settled -- from `colr`, or from the
    /// sequence header -- or are still to be taken from the first decoded
    /// frame.
    pub(crate) cicp_set: bool,
    /// The `colr` ICC profile, which is not applied.
    pub(crate) icc: Option<&'a [u8]>,
    pub(crate) clap: Option<Clap>,
    pub(crate) irot: Option<u8>,
    pub(crate) imir: Option<u8>,
    /// The colour was premultiplied by the alpha (`prem`).
    #[cfg_attr(
        not(feature = "avif"),
        expect(dead_code, reason = "only decoding reads it")
    )]
    pub(crate) alpha_premultiplied: bool,
    /// Frames: 1 for a still picture.
    #[cfg_attr(
        not(feature = "avif"),
        expect(dead_code, reason = "only decoding reads it")
    )]
    pub(crate) frame_count: u32,
    #[expect(
        dead_code,
        reason = "read by the animation that follows the first frame"
    )]
    pub(crate) timing: Option<Timing>,
}

/// `avifDecoderItemShouldBeSkipped`: an item that is empty, needs a property
/// libavif does not know, is neither a coded picture nor a grid, or is a
/// thumbnail.
fn skipped(item: &Item) -> bool {
    item.size == 0
        || item.unsupported_essential
        || (!container::is_av1(&item.kind) && &item.kind != b"grid")
        || item.thumbnail_for != 0
}

/// `avifDecoderItemIsAlphaAux`: `item` is an alpha plane of item `parent`.
fn is_alpha_aux(meta: &Meta<'_>, item: &Item, parent: u32) -> bool {
    item.aux_for == parent
        && matches!(
            meta.find_property(item, b"auxC"),
            Some(Property::Aux { aux_type, .. }) if is_alpha_urn(aux_type)
        )
}

/// An item's `av1C`.
fn av1_config(meta: &Meta<'_>, item: &Item) -> Option<Av1Config> {
    match meta.find_property(item, b"av1C") {
        Some(Property::Av1C(config)) => Some(*config),
        _ => None,
    }
}

impl<'a> Picture<'a> {
    /// `avifDecoderParse`: the boxes, the `ispe` harvest, and the reset.
    pub(crate) fn read(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut file = container::parse(bytes)?;
        harvest_ispe(&mut file.meta)?;
        reset(file)
    }

    /// The size shown, before any turn: cropped as Chrome crops it, when the
    /// `clap` box describes a whole crop starting at the top left.
    pub(crate) fn shown_size(&self) -> (u32, u32) {
        self.crop()
            .map_or((self.width, self.height), |rect| (rect.width, rect.height))
    }

    /// Chrome's crop: `avifCropRectConvertCleanApertureBox`, kept only when
    /// it is valid and starts at (0, 0).
    pub(crate) fn crop(&self) -> Option<CropRect> {
        let rect = crop_rect(&self.clap?, self.width, self.height, self.format)?;
        (rect.x == 0 && rect.y == 0).then_some(rect)
    }

    /// Refuse a picture larger than `limits`, before anything its size
    /// implies is allocated.
    pub(crate) fn check(&self, limits: Limits) -> ImageResult<()> {
        let pixels = u64::from(self.width).saturating_mul(u64::from(self.height));
        if pixels > limits.max_pixels {
            return Err(ImageError::TooLarge {
                pixels,
                limit: limits.max_pixels,
            });
        }
        Ok(())
    }

    /// The bytes of a tile's frame `frame`, or the first `partial` of them
    /// (all, for 0): `avifDecoderPrepareSample`.
    pub(crate) fn sample(
        &self,
        tile: &Tile,
        frame: u32,
        partial: usize,
    ) -> Result<Cow<'a, [u8]>, Error> {
        match tile.input {
            TileInput::Item { position, size, .. } => {
                let want = if partial > 0 { partial.min(size) } else { size };
                read_item(&self.file, position, 0, want)
            }
            TileInput::Track { track } => {
                let sample = track_sample(&self.file, track, frame)?;
                let size =
                    usize::try_from(sample.size).map_err(|_| Error::Parse("AVIF sample size"))?;
                let want = if partial > 0 { partial.min(size) } else { size };
                read_file(self.file.bytes, sample.offset, want)
            }
        }
    }
}

/// The sample `frame` of track `track`.
fn track_sample(file: &File<'_>, track: usize, frame: u32) -> Result<Sample, Error> {
    let table = file
        .tracks
        .get(track)
        .and_then(|t| t.sample_table.as_ref())
        .ok_or(Error::NoContent("AVIF track without samples"))?;
    let mut found = None;
    let mut index = 0u32;
    table.samples(IMAGE_COUNT_LIMIT, file_size(file.bytes), |sample| {
        if index == frame {
            found = Some(sample);
            return false;
        }
        index = index.saturating_add(1);
        true
    })?;
    found.ok_or(Error::NoContent("AVIF frame past the last"))
}

fn file_size(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// The memory reader's read of `size` bytes at `offset`, which the caller has
/// checked lie in the file: `avifIOMemoryReaderRead`.
fn read_file(bytes: &[u8], offset: u64, size: usize) -> Result<Cow<'_, [u8]>, Error> {
    let start = usize::try_from(offset).map_err(|_| Error::Parse("AVIF offset"))?;
    let end = start.checked_add(size).ok_or(Error::Truncated)?;
    bytes
        .get(start..end)
        .map(Cow::Borrowed)
        .ok_or(Error::Truncated)
}

/// `avifDecoderItemRead`: `partial` bytes (all of them, for 0) of the item at
/// `position`, from `offset` -- its extents joined, from the file or from
/// `idat`, with libavif's checks on each extent it reaches. An extent after
/// the bytes asked for is not reached, and not checked.
pub(crate) fn read_item<'a>(
    file: &File<'a>,
    position: usize,
    offset: usize,
    partial: usize,
) -> Result<Cow<'a, [u8]>, Error> {
    let meta = &file.meta;
    let item = meta.items.get(position).ok_or(Error::MissingImage)?;
    if !item.has_extents() {
        return Err(Error::Truncated);
    }
    let idat = if item.idat_stored {
        match meta.idat {
            Some(idat) if !idat.is_empty() => Some(idat),
            _ => {
                return Err(Error::NoContent(
                    "AVIF item in an idat box that is not there",
                ));
            }
        }
    } else {
        None
    };
    let size_hint = file.bytes.len();
    if size_hint > 0 && item.size > size_hint {
        return Err(Error::Truncated);
    }
    if offset >= item.size {
        return Err(Error::Truncated);
    }
    let most = item.size.saturating_sub(offset);
    let wanted = if partial > 0 && partial < most {
        partial
    } else {
        most
    };
    let total = offset.saturating_add(wanted);

    let mut joined: Vec<u8> = Vec::new();
    let mut single: Option<&'a [u8]> = None;
    let mut remaining = total;
    for extent in &item.extents {
        let take = extent.size.min(remaining);
        let chunk: &'a [u8] = if let Some(idat) = idat {
            let at = usize::try_from(extent.offset)
                .ok()
                .filter(|&at| at <= idat.len())
                .ok_or(Error::Parse("AVIF extent outside idat"))?;
            if extent.size > idat.len().saturating_sub(at) {
                return Err(Error::Parse("AVIF extent outside idat"));
            }
            idat.get(at..at.saturating_add(take))
                .ok_or(Error::Truncated)?
        } else {
            if size_hint > 0 && extent.offset > file_size(file.bytes) {
                return Err(Error::Parse("AVIF extent past the end of the file"));
            }
            let at = usize::try_from(extent.offset).map_err(|_| Error::Truncated)?;
            let rest = file.bytes.get(at..).unwrap_or_default();
            rest.get(..take).ok_or(Error::Truncated)?
        };
        if item.extents.len() == 1 {
            single = Some(chunk);
        } else {
            joined.extend_from_slice(chunk);
        }
        remaining = remaining.saturating_sub(take);
        if remaining == 0 {
            break;
        }
    }
    if remaining != 0 {
        return Err(Error::Truncated);
    }
    match single {
        Some(bytes) => bytes
            .get(offset..total)
            .map(Cow::Borrowed)
            .ok_or(Error::Truncated),
        None => {
            joined.truncate(total);
            if offset > 0 {
                joined.drain(..offset);
            }
            Ok(Cow::Owned(joined))
        }
    }
}

/// `avifDecoderParse`'s walk over the items: every item not skipped takes
/// its size from `ispe`, which it must have -- an alpha item too, under
/// `AVIF_STRICT_ALPHA_ISPE_REQUIRED`, which Chrome and Pillow leave on.
fn harvest_ispe(meta: &mut Meta<'_>) -> Result<(), Error> {
    for position in 0..meta.items.len() {
        let Some(item) = meta.items.get(position) else {
            break;
        };
        if skipped(item) {
            continue;
        }
        let (width, height) = match meta.find_property(item, b"ispe") {
            Some(&Property::Ispe { width, height }) => (width, height),
            _ => {
                return Err(
                    if matches!(
                        meta.find_property(item, b"auxC"),
                        Some(Property::Aux { aux_type, .. }) if is_alpha_urn(aux_type)
                    ) {
                        Error::Parse("AVIF alpha item without ispe")
                    } else {
                        Error::Parse("AVIF item without ispe")
                    },
                );
            }
        };
        if width == 0 || height == 0 {
            return Err(Error::Parse("AVIF item of no size"));
        }
        if too_large(width, height) {
            return Err(Error::Parse("AVIF item too large"));
        }
        if let Some(item) = meta.items.get_mut(position) {
            item.width = width;
            item.height = height;
        }
    }
    Ok(())
}

/// What the colour and alpha categories carry through the reset: the main
/// item's position, and its grid if it is one.
struct Category {
    position: usize,
    grid: Option<Grid>,
}

/// `avifDecoderReset`.
fn reset(mut file: File<'_>) -> Result<Picture<'_>, Error> {
    // AVIF_DECODER_SOURCE_AUTO: the major brand decides, else tracks if any.
    let tracks = match &file.major_brand {
        b"avis" => true,
        b"avif" => false,
        _ => !file.tracks.is_empty(),
    };
    let (color, alpha, props, size, alpha_premultiplied, frame_count, timing) = if tracks {
        reset_tracks(&file)?
    } else {
        reset_items(&mut file)?
    };

    // Every sample must have some data.
    for tile in color
        .tiles
        .iter()
        .chain(alpha.iter().flat_map(|a| a.tiles.iter()))
    {
        match tile.input {
            TileInput::Item { size, .. } => {
                if size == 0 {
                    return Err(Error::Parse("AVIF sample of no bytes"));
                }
            }
            TileInput::Track { track } => {
                let Some(table) = file.tracks.get(track).and_then(|t| t.sample_table.as_ref())
                else {
                    continue;
                };
                let mut empty = false;
                table.samples(IMAGE_COUNT_LIMIT, file_size(file.bytes), |sample| {
                    empty = sample.size == 0;
                    !empty
                })?;
                if empty {
                    return Err(Error::Parse("AVIF sample of no bytes"));
                }
            }
        }
    }

    let mut picture = Picture {
        file,
        color,
        alpha,
        width: size.0,
        height: size.1,
        depth: 8,
        format: YuvFormat::Yuv444,
        chroma_sample_position: 0,
        colour: Colour::DEFAULT,
        cicp_set: false,
        icc: None,
        clap: None,
        irot: None,
        imir: None,
        alpha_premultiplied,
        frame_count,
        timing,
    };

    // avifReadColorProperties: at most one ICC profile and one `nclx`.
    let (icc, nclx) = colour_properties(&props)?;
    picture.icc = icc;
    if let Some(cicp) = nclx {
        picture.colour = Colour {
            primaries: cicp.primaries,
            transfer: cicp.transfer,
            matrix: cicp.matrix,
            full_range: cicp.full_range,
        };
        picture.cicp_set = true;
    }
    for property in &props {
        match *property {
            Property::Clap(clap) if picture.clap.is_none() => picture.clap = Some(clap),
            Property::Irot(angle) if picture.irot.is_none() => picture.irot = Some(angle),
            Property::Imir(axis) if picture.imir.is_none() => picture.imir = Some(axis),
            _ => {}
        }
    }

    if !picture.cicp_set {
        harvest_cicp(&mut picture)?;
    }

    // avifReadCodecConfigProperty.
    let config = props
        .iter()
        .find_map(|p| match p {
            Property::Av1C(config) => Some(*config),
            _ => None,
        })
        .ok_or(Error::Parse("AVIF picture without av1C"))?;
    picture.depth = config.depth();
    picture.format = if config.monochrome {
        YuvFormat::Yuv400
    } else if config.chroma_subsampling_x && config.chroma_subsampling_y {
        YuvFormat::Yuv420
    } else if config.chroma_subsampling_x {
        YuvFormat::Yuv422
    } else {
        YuvFormat::Yuv444
    };
    picture.chroma_sample_position = config.chroma_sample_position;
    Ok(picture)
}

/// What the two halves of the reset hand the common tail.
type Reset<'a> = (
    Layer,
    Option<Layer>,
    Vec<Property<'a>>,
    (u32, u32),
    bool,
    u32,
    Option<Timing>,
);

/// The sequence half of `avifDecoderReset`.
fn reset_tracks<'a>(file: &File<'a>) -> Result<Reset<'a>, Error> {
    let usable = |track: &Track<'_>| {
        track.id != 0
            && track
                .sample_table
                .as_ref()
                .is_some_and(|t| t.has_chunks() && t.is_av1())
    };
    let color_index = file
        .tracks
        .iter()
        .position(|t| usable(t) && t.aux_for == 0)
        .ok_or(Error::NoContent(
            "AVIF sequence without an AV1 colour track",
        ))?;
    let color_track = file.tracks.get(color_index).ok_or(Error::MissingImage)?;
    let props = color_track
        .sample_table
        .as_ref()
        .and_then(|t| t.av1_properties())
        .ok_or(Error::Parse("AVIF colour track without properties"))?
        .to_vec();
    let alpha_index = file.tracks.iter().position(|t| {
        if !usable(t) {
            return false;
        }
        // An `auxi` that is present must name alpha; one that is absent is
        // taken to (libavif before 2022 wrote none).
        let alpha_aux = t
            .sample_table
            .as_ref()
            .and_then(|t| t.av1_properties())
            .is_none_or(|props| {
                props.iter().find(|p| p.kind() == *b"auxi").is_none_or(
                    |p| matches!(p, Property::Aux { aux_type, .. } if is_alpha_urn(aux_type)),
                )
            });
        alpha_aux && t.aux_for == color_track.id
    });

    let tile_of = |index: usize, track: &Track<'_>| -> Result<(Tile, u32), Error> {
        let table = track.sample_table.as_ref().ok_or(Error::MissingImage)?;
        let count = table.samples(IMAGE_COUNT_LIMIT, file_size(file.bytes), |_| true)?;
        Ok((
            Tile {
                width: track.width,
                height: track.height,
                operating_point: 0,
                all_layers: false,
                input: TileInput::Track { track: index },
            },
            count,
        ))
    };
    let (color_tile, frame_count) = tile_of(color_index, color_track)?;
    let color = Layer {
        grid: None,
        tiles: vec![color_tile],
    };
    let mut alpha = None;
    let mut premultiplied = false;
    if let Some(index) = alpha_index {
        let alpha_track = file.tracks.get(index).ok_or(Error::MissingImage)?;
        let (tile, _) = tile_of(index, alpha_track)?;
        alpha = Some(Layer {
            grid: None,
            tiles: vec![tile],
        });
        premultiplied = color_track.prem_by == alpha_track.id;
    }
    let timing = Timing {
        timescale: color_track.media_timescale,
        duration: color_track.media_duration,
        repetition: color_track.repetition,
    };
    Ok((
        color,
        alpha,
        props,
        (color_track.width, color_track.height),
        premultiplied,
        frame_count,
        Some(timing),
    ))
}

/// The still-picture half of `avifDecoderReset`.
fn reset_items<'a>(file: &mut File<'a>) -> Result<Reset<'a>, Error> {
    let primary = file.meta.primary;
    if primary == 0 {
        return Err(Error::MissingImage);
    }
    let color_position = file
        .meta
        .position(primary)
        .filter(|&at| file.meta.items.get(at).is_some_and(|item| !skipped(item)))
        .ok_or(Error::MissingImage)?;
    let color_grid = read_and_parse(file, color_position, true, None)?;
    let mut color = Category {
        position: color_position,
        grid: color_grid,
    };

    let mut alpha = match find_alpha(&mut file.meta, &color)? {
        Some((position, in_input, grid)) => Some(Category {
            position,
            grid: read_and_parse(file, position, in_input, grid)?,
        }),
        None => None,
    };

    // A gain map (HDR tone mapping) is never decoded here, but libavif reads
    // and checks its metadata whenever the file has the `tmap` brand, and a
    // broken one refuses the picture.
    if file.is_compatible_with(b"tmap") {
        if let Some(tmap) = find_gain_map(file, color.position)? {
            let data = read_item(file, tmap, 0, 0)?;
            match parse_tmap(&data) {
                Ok(()) | Err(Error::Unsupported(_)) => {}
                Err(error) => return Err(error),
            }
        }
    }

    let mut layers = Vec::new();
    for category in [Some(&mut color), alpha.as_mut()].into_iter().flatten() {
        if category.grid.is_some() {
            adopt_grid_codec(&mut file.meta, category)?;
        }
        let tiles = generate_tiles(file, category)?;
        validate_properties(&file.meta, category)?;
        layers.push(Layer {
            grid: category.grid,
            tiles,
        });
    }
    let mut layers = layers.into_iter();
    let color_layer = layers.next().ok_or(Error::MissingImage)?;
    let alpha_layer = layers.next();

    let meta = &file.meta;
    let color_item = meta.items.get(color.position).ok_or(Error::MissingImage)?;
    let premultiplied = alpha
        .as_ref()
        .and_then(|a| meta.items.get(a.position))
        .is_some_and(|alpha_item| color_item.prem_by == alpha_item.id);
    let props = color_item
        .properties
        .iter()
        .filter_map(|&at| meta.properties.get(at).cloned())
        .collect();
    Ok((
        color_layer,
        alpha_layer,
        props,
        (color_item.width, color_item.height),
        premultiplied,
        1,
        None,
    ))
}

/// `avifDecoderItemReadAndParse`: a grid's layout, read and checked against
/// its tiles -- or, for the alpha grid libavif made up, the one it copied.
fn read_and_parse(
    file: &File<'_>,
    position: usize,
    in_input: bool,
    made_up: Option<Grid>,
) -> Result<Option<Grid>, Error> {
    let meta = &file.meta;
    let item = meta.items.get(position).ok_or(Error::MissingImage)?;
    if &item.kind != b"grid" {
        return Ok(None);
    }
    let grid = if in_input {
        let data = read_item(file, position, 0, 0)?;
        let grid = parse_grid(&data).ok_or(Error::Grid("AVIF grid box"))?;
        let inputs = meta
            .items
            .iter()
            .filter(|other| other.dimg_for == item.id)
            .count();
        if inputs != grid.tiles() {
            return Err(Error::Grid(
                "AVIF grid whose tile count is not rows times columns",
            ));
        }
        grid
    } else {
        made_up.ok_or(Error::Grid("AVIF grid"))?
    };
    // avifDecoderItemGetGridCodecType: the first tile's.
    let coded = meta
        .items
        .iter()
        .any(|other| other.dimg_for == item.id && container::is_av1(&other.kind));
    if !coded {
        return Err(Error::Grid("AVIF grid of no AV1 tile"));
    }
    Ok(Some(grid))
}

/// `avifParseImageGridBox`, with libavif's default size limits.
fn parse_grid(data: &[u8]) -> Option<Grid> {
    let mut s = Stream::new(data);
    if s.u8().ok()? != 0 {
        return None; // version
    }
    let flags = s.u8().ok()?;
    let rows = u32::from(s.u8().ok()?).saturating_add(1);
    let columns = u32::from(s.u8().ok()?).saturating_add(1);
    let (output_width, output_height) = if flags & 1 == 0 {
        (u32::from(s.u16().ok()?), u32::from(s.u16().ok()?))
    } else {
        (s.u32().ok()?, s.u32().ok()?)
    };
    if output_width == 0 || output_height == 0 || too_large(output_width, output_height) {
        return None;
    }
    (s.remaining() == 0).then_some(Grid {
        rows,
        columns,
        output_width,
        output_height,
    })
}

/// `avifMetaFindAlphaItem`: the colour item's alpha item, or -- for a grid
/// whose every tile has its own alpha item -- an alpha grid made up of those,
/// laid out as the colour grid is. Returns the alpha item's position, whether
/// it is in the file (a made-up grid is not), and a made-up grid's layout.
fn find_alpha(
    meta: &mut Meta<'_>,
    color: &Category,
) -> Result<Option<(usize, bool, Option<Grid>)>, Error> {
    let color_item = meta.items.get(color.position).ok_or(Error::MissingImage)?;
    let color_id = color_item.id;
    let found = meta
        .items
        .iter()
        .position(|item| !skipped(item) && is_alpha_aux(meta, item, color_id));
    if let Some(position) = found {
        return Ok(Some((position, true, None)));
    }
    if &color_item.kind != b"grid" {
        return Ok(None);
    }
    let Some(grid) = color.grid else {
        return Ok(None);
    };
    let tile_count = grid.tiles();
    if tile_count == 0 {
        return Ok(None);
    }
    // Each item's alpha items, in item order: the index libavif's nested
    // loop searches.
    let mut alphas: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (position, item) in meta.items.iter().enumerate() {
        if item.aux_for != 0 && is_alpha_aux(meta, item, item.aux_for) {
            alphas.entry(item.aux_for).or_default().push(position);
        }
    }
    let mut by_tile: Vec<Option<usize>> = vec![None; tile_count];
    let mut count = 0usize;
    for tile in meta.items.iter().filter(|item| item.dimg_for == color_id) {
        let mut seen = false;
        for &alpha in alphas.get(&tile.id).map_or(&[][..], Vec::as_slice) {
            let index = usize::from(tile.dimg_idx);
            let taken = by_tile.get(index).is_none_or(Option::is_some);
            let reused = meta.items.get(alpha).is_none_or(|a| a.dimg_for != 0);
            if seen || reused || taken {
                return Err(Error::Grid("AVIF grid whose alpha tiles do not match"));
            }
            if let Some(slot) = by_tile.get_mut(index) {
                *slot = Some(alpha);
            }
            count = count.saturating_add(1);
            seen = true;
        }
        if !seen {
            // A tile with no alpha: the picture has none.
            return Ok(None);
        }
    }
    if count != tile_count {
        return Err(Error::Grid("AVIF grid whose alpha tiles do not match"));
    }
    if u32::try_from(meta.items.len()).map_or(true, |n| n >= u32::MAX - 1) {
        return Err(Error::Alpha(
            "AVIF file with no item ID left for its alpha grid",
        ));
    }
    // The first unused ID.
    let mut id = 1u32;
    while meta.position(id).is_some() {
        id = id.checked_add(1).ok_or(Error::Alpha("AVIF item IDs"))?;
    }
    let (width, height) = (color_item.width, color_item.height);
    let position = meta.find_or_create(id);
    let made = meta.item_mut(id);
    made.kind = *b"grid";
    made.width = width;
    made.height = height;
    for (index, alpha) in by_tile.iter().enumerate() {
        let alpha = alpha.ok_or(Error::Grid("AVIF grid alpha tile"))?;
        let item = meta
            .items
            .get_mut(alpha)
            .ok_or(Error::Grid("AVIF grid alpha tile"))?;
        item.dimg_for = id;
        item.dimg_idx = u16::try_from(index).map_err(|_| Error::Grid("AVIF grid alpha tile"))?;
    }
    Ok(Some((position, false, Some(grid))))
}

/// `avifFillDimgIdxToItemIdxArray`: the position of each of a grid's tiles,
/// in the grid's order.
fn tile_positions(meta: &Meta<'_>, grid_id: u32, tiles: usize) -> Result<Vec<usize>, Error> {
    let mut positions: Vec<Option<usize>> = vec![None; tiles];
    for (position, item) in meta.items.iter().enumerate() {
        if item.dimg_for != grid_id {
            continue;
        }
        let slot = positions
            .get_mut(usize::from(item.dimg_idx))
            .ok_or(Error::Grid("AVIF grid tile index"))?;
        if slot.is_some() {
            return Err(Error::Grid("AVIF grid tile named twice"));
        }
        *slot = Some(position);
    }
    positions
        .into_iter()
        .map(|p| p.ok_or(Error::Grid("AVIF grid tile count")))
        .collect()
}

/// `avifDecoderAdoptGridTileCodecType`: every tile AV1 and usable, and the
/// first tile's `av1C` added to the grid's properties, where the rest of the
/// reset looks for it.
fn adopt_grid_codec(meta: &mut Meta<'_>, category: &Category) -> Result<(), Error> {
    let Some(grid) = category.grid else {
        return Ok(());
    };
    let grid_item = meta
        .items
        .get(category.position)
        .ok_or(Error::MissingImage)?;
    let positions = tile_positions(meta, grid_item.id, grid.tiles())?;
    let mut adopted = None;
    for position in positions {
        let tile = meta
            .items
            .get(position)
            .ok_or(Error::Grid("AVIF grid tile"))?;
        if !container::is_av1(&tile.kind) {
            return Err(Error::Grid("AVIF grid tile that is not AV1"));
        }
        if tile.unsupported_essential {
            return Err(Error::Grid(
                "AVIF grid tile with an unknown essential property",
            ));
        }
        if adopted.is_none() {
            let index = tile
                .properties
                .iter()
                .copied()
                .find(|&at| matches!(meta.properties.get(at), Some(Property::Av1C(_))))
                .ok_or(Error::Grid("AVIF grid whose first tile has no av1C"))?;
            adopted = Some(index);
        }
    }
    if let (Some(index), Some(grid_item)) = (adopted, meta.items.get_mut(category.position)) {
        grid_item.properties.push(index);
    }
    Ok(())
}

/// `avifDecoderGenerateImageTiles`.
fn generate_tiles(file: &File<'_>, category: &Category) -> Result<Vec<Tile>, Error> {
    let meta = &file.meta;
    let item = meta
        .items
        .get(category.position)
        .ok_or(Error::MissingImage)?;
    match category.grid {
        Some(grid) => {
            let positions = tile_positions(meta, item.id, grid.tiles())?;
            positions
                .into_iter()
                .map(|position| {
                    let tile = meta
                        .items
                        .get(position)
                        .ok_or(Error::Grid("AVIF grid tile"))?;
                    if !container::is_av1(&tile.kind) {
                        return Err(Error::Grid("AVIF grid tile that is not AV1"));
                    }
                    tile_of_item(file, position)
                })
                .collect()
        }
        None => {
            if item.size == 0 {
                return Err(Error::MissingImage);
            }
            Ok(vec![tile_of_item(file, category.position)?])
        }
    }
}

/// `avifDecoderDataCreateTile` and `avifCodecDecodeInputFillFromDecoderItem`
/// for one coded item, as a still picture (progressive decoding off, as
/// Chrome and Pillow leave it for a whole file).
fn tile_of_item(file: &File<'_>, position: usize) -> Result<Tile, Error> {
    let meta = &file.meta;
    let item = meta.items.get(position).ok_or(Error::MissingImage)?;
    if item.size > file.bytes.len() {
        return Err(Error::Parse("AVIF item larger than the file"));
    }
    // a1lx: the layers' sizes, the last taking what is left.
    let mut layers: Vec<usize> = Vec::new();
    if let Some(Property::A1lx(sizes)) = meta.find_property(item, b"a1lx") {
        let mut remaining = item.size;
        let mut ended = false;
        for &size in sizes {
            let size = usize::try_from(size).unwrap_or(usize::MAX);
            if size == 0 {
                layers.push(remaining);
                remaining = 0;
                ended = true;
                break;
            }
            if size >= remaining {
                return Err(Error::Parse("AVIF a1lx layer larger than its item"));
            }
            layers.push(size);
            remaining = remaining.saturating_sub(size);
        }
        if !ended && remaining > 0 {
            layers.push(remaining);
        }
    }
    let operating_point = match meta.find_property(item, b"a1op") {
        Some(&Property::A1op(op)) => op,
        _ => 0,
    };
    let (all_layers, size, spatial_id) = match meta.find_property(item, b"lsel") {
        Some(&Property::Lsel(layer)) if layer != 0xFFFF => {
            let size = if layers.is_empty() {
                item.size
            } else {
                let wanted = usize::from(layer);
                if wanted >= layers.len() {
                    return Err(Error::Parse("AVIF lsel layer beyond a1lx"));
                }
                layers.iter().take(wanted.saturating_add(1)).sum()
            };
            let spatial = u8::try_from(layer).map_err(|_| Error::Parse("AVIF lsel layer"))?;
            (true, size, Some(spatial))
        }
        _ => (false, item.size, None),
    };
    Ok(Tile {
        width: item.width,
        height: item.height,
        operating_point,
        all_layers,
        input: TileInput::Item {
            position,
            size,
            spatial_id,
        },
    })
}

/// `avifDecoderItemValidateProperties` with `AVIF_STRICT_PIXI_REQUIRED` and
/// `AVIF_STRICT_CLAP_VALID` off, as Chrome and Pillow turn them off.
fn validate_properties(meta: &Meta<'_>, category: &Category) -> Result<(), Error> {
    let item = meta
        .items
        .get(category.position)
        .ok_or(Error::MissingImage)?;
    let config = av1_config(meta, item).ok_or(Error::Parse("AVIF item without av1C"))?;
    if &item.kind == b"grid" {
        for tile in meta.items.iter().filter(|tile| tile.dimg_for == item.id) {
            let tile_config =
                av1_config(meta, tile).ok_or(Error::Parse("AVIF grid tile without av1C"))?;
            if tile_config != config {
                return Err(Error::Parse("AVIF grid tiles of different av1C"));
            }
        }
    }
    if let Some(&Property::Pixi { depth, .. }) = meta.find_property(item, b"pixi") {
        if depth != config.depth() {
            return Err(Error::Parse("AVIF pixi depth unlike av1C's"));
        }
    }
    Ok(())
}

/// `avifReadColorProperties` without the reading: the ICC profile and the
/// `nclx` code points, each of which may appear at most once.
fn colour_properties<'a>(
    props: &[Property<'a>],
) -> Result<(Option<&'a [u8]>, Option<container::Cicp>), Error> {
    let mut icc = None;
    let mut nclx = None;
    for property in props {
        if let Property::Colr(Colr::Icc(profile)) = property {
            if icc.is_some() {
                return Err(Error::Parse("AVIF picture with two ICC profiles"));
            }
            icc = Some(*profile);
        }
    }
    for property in props {
        if let Property::Colr(Colr::Nclx(cicp)) = property {
            if nclx.is_some() {
                return Err(Error::Parse("AVIF picture with two nclx boxes"));
            }
            nclx = Some(*cicp);
        }
    }
    Ok((icc, nclx))
}

/// The most of a first frame libavif reads looking for its sequence header,
/// and the step it reads it in.
const HARVEST_MOST: usize = 4096;
const HARVEST_STEP: usize = 64;

/// libavif's harvest of the colour from the first frame's sequence header,
/// when the container has no `nclx`: read 64 bytes more at a time, up to
/// 4096 or the whole frame, until it parses. A read that fails fails the
/// file; a header never found leaves the colour to the decoder -- and the
/// range at full, which is where libavif leaves it.
fn harvest_cicp(picture: &mut Picture<'_>) -> Result<(), Error> {
    let Some(tile) = picture.color.tiles.first().copied() else {
        return Ok(());
    };
    let size = match tile.input {
        TileInput::Item { size, .. } => size,
        TileInput::Track { track } => {
            let sample = track_sample(&picture.file, track, 0)?;
            usize::try_from(sample.size).map_err(|_| Error::Parse("AVIF sample size"))?
        }
    };
    let mut searched = 0usize;
    loop {
        searched = searched.saturating_add(HARVEST_STEP).min(size);
        let data = picture.sample(&tile, 0, searched)?;
        if let Some(found) = obu::sequence_colour(&data) {
            picture.colour = Colour {
                primaries: found.primaries,
                transfer: found.transfer,
                matrix: found.matrix,
                full_range: found.full_range,
            };
            picture.cicp_set = true;
            return Ok(());
        }
        if searched == size || searched >= HARVEST_MOST {
            return Ok(());
        }
    }
}

/// `avifDecoderFindGainMapItem`, for its checks: the `tmap` item that tone
/// maps the colour item, if there is one and it is preferred -- or libavif's
/// reason to refuse the file.
fn find_gain_map(file: &File<'_>, color_position: usize) -> Result<Option<usize>, Error> {
    let meta = &file.meta;
    let color = meta.items.get(color_position).ok_or(Error::MissingImage)?;
    // avifDecoderDataFindToneMappedImageItem, with the inputs indexed.
    let mut inputs: BTreeMap<u32, Vec<(u16, u32)>> = BTreeMap::new();
    for item in &meta.items {
        if item.dimg_for != 0 {
            inputs
                .entry(item.dimg_for)
                .or_default()
                .push((item.dimg_idx, item.id));
        }
    }
    let mut found = None;
    for (position, item) in meta.items.iter().enumerate() {
        if item.size == 0
            || item.unsupported_essential
            || item.thumbnail_for != 0
            || &item.kind != b"tmap"
        {
            continue;
        }
        let mut ids = [0u32; 2];
        let mut count = 0usize;
        for &(index, id) in inputs.get(&item.id).map_or(&[][..], Vec::as_slice) {
            if let Some(slot) = ids.get_mut(usize::from(index)) {
                if *slot != 0 {
                    return Err(Error::ToneMap("AVIF tmap input named twice"));
                }
                *slot = id;
            }
            count = count.saturating_add(1);
        }
        if count != 2 || ids.contains(&0) {
            return Err(Error::ToneMap("AVIF tmap without exactly two inputs"));
        }
        if ids[0] != color.id {
            continue;
        }
        found = Some((position, ids[1]));
        break;
    }
    let Some((tmap_position, gain_id)) = found else {
        return Ok(None);
    };
    let tmap = meta.items.get(tmap_position).ok_or(Error::MissingImage)?;
    if !preferred_alternative(meta, tmap.id, color.id) {
        return Ok(None);
    }
    let gain_position = meta
        .position(gain_id)
        .ok_or(Error::ToneMap("AVIF gain map item"))?;
    let gain = meta
        .items
        .get(gain_position)
        .ok_or(Error::ToneMap("AVIF gain map item"))?;
    if skipped(gain) {
        return Err(Error::ToneMap("AVIF gain map that is not a picture"));
    }
    read_and_parse(file, gain_position, true, None)?;
    let tmap_props: Vec<Property<'_>> = tmap
        .properties
        .iter()
        .filter_map(|&at| meta.properties.get(at).cloned())
        .collect();
    colour_properties(&tmap_props)?;
    match meta.find_property(tmap, b"ispe") {
        None => return Err(Error::Parse("AVIF tmap without ispe")),
        Some(&Property::Ispe { width, height })
            if (width, height) != (color.width, color.height) =>
        {
            return Err(Error::Parse("AVIF tmap of another size than its picture"));
        }
        Some(_) => {}
    }
    const TRANSFORMS: [FourCc; 4] = [*b"pasp", *b"clap", *b"irot", *b"imir"];
    if TRANSFORMS
        .iter()
        .any(|kind| meta.find_property(tmap, kind).is_some())
    {
        return Err(Error::ToneMap("AVIF tmap with a transform of its own"));
    }
    Ok(Some(tmap_position))
}

/// `avifIsPreferredAlternativeTo`: `first` comes before `second` in an `altr`
/// group.
fn preferred_alternative(meta: &Meta<'_>, first: u32, second: u32) -> bool {
    for group in meta.groups.iter().filter(|g| &g.kind == b"altr") {
        let mut seen_first = false;
        for &entity in &group.entities {
            if entity == first {
                seen_first = true;
            } else if entity == second {
                return seen_first;
            }
        }
    }
    false
}

/// `avifParseToneMappedImageBox`: `Unsupported` for a version libavif does
/// not read (and ignores the gain map for), `ToneMap` for one it refuses.
fn parse_tmap(data: &[u8]) -> Result<(), Error> {
    const WHAT: &str = "AVIF tmap metadata";
    let bad = |_| Error::ToneMap(WHAT);
    let mut s = Stream::new(data);
    if s.u8().map_err(bad)? != 0 {
        return Err(Error::Unsupported("AVIF tmap version"));
    }
    let minimum_version = s.u16().map_err(bad)?;
    if minimum_version > 0 {
        return Err(Error::Unsupported("AVIF tmap version"));
    }
    let writer_version = s.u16().map_err(bad)?;
    if writer_version < minimum_version {
        return Err(Error::ToneMap(WHAT));
    }
    // avifParseGainMapMetadata.
    let multichannel = s.bits(1).map_err(bad)? == 1;
    s.bits(1).map_err(bad)?; // use_base_colour_space
    s.bits(6).map_err(bad)?; // reserved
    let base_headroom_d = {
        s.u32().map_err(bad)?;
        s.u32().map_err(bad)?
    };
    let alternate_headroom_d = {
        s.u32().map_err(bad)?;
        s.u32().map_err(bad)?
    };
    let channels = if multichannel { 3 } else { 1 };
    // Per channel: min, max, gamma, base offset, alternate offset, each a
    // numerator then a denominator; the minimum and maximum signed.
    let mut fields = [[0u32; 10]; 3];
    for channel in fields.iter_mut().take(channels) {
        for field in channel.iter_mut() {
            *field = s.u32().map_err(bad)?;
        }
    }
    if writer_version == 0 && s.remaining() != 0 {
        return Err(Error::ToneMap(WHAT));
    }
    // avifGainMapValidateMetadata, over three channels, the missing ones
    // copied from the first.
    for index in 0..3 {
        let [
            min_n,
            min_d,
            max_n,
            max_d,
            gamma_n,
            gamma_d,
            _,
            base_d,
            _,
            alternate_d,
        ] = *fields
            .get(if index < channels { index } else { 0 })
            .ok_or(Error::ToneMap(WHAT))?;
        if [min_d, max_d, gamma_d, base_d, alternate_d].contains(&0) {
            return Err(Error::ToneMap(WHAT));
        }
        let (min_n, max_n) = (
            i64::from(min_n.cast_signed()),
            i64::from(max_n.cast_signed()),
        );
        if max_n.saturating_mul(i64::from(min_d)) < min_n.saturating_mul(i64::from(max_d)) {
            return Err(Error::ToneMap(WHAT));
        }
        if gamma_n == 0 {
            return Err(Error::ToneMap(WHAT));
        }
    }
    if base_headroom_d == 0 || alternate_headroom_d == 0 {
        return Err(Error::ToneMap(WHAT));
    }
    Ok(())
}

/// A crop in pixels: `avifCropRect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CropRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// `avifFraction`: a ratio of two `int32_t`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fraction {
    n: i32,
    d: i32,
}

impl Fraction {
    /// `avifFractionSimplify`.
    fn simplify(&mut self) {
        let (mut a, mut b) = (i64::from(self.n).abs(), i64::from(self.d).abs());
        while b != 0 {
            let r = a.checked_rem(b).unwrap_or(0);
            a = b;
            b = r;
        }
        if a > 1 {
            self.n = narrow(i64::from(self.n).checked_div(a).unwrap_or(0));
            self.d = narrow(i64::from(self.d).checked_div(a).unwrap_or(0));
        }
    }

    /// `avifFractionCD`: both over one denominator, if it fits.
    fn common_denominator(a: &mut Self, b: &mut Self) -> bool {
        a.simplify();
        b.simplify();
        if a.d != b.d {
            let (ad, bd) = (i64::from(a.d), i64::from(b.d));
            let an = i64::from(a.n).saturating_mul(bd);
            let adn = ad.saturating_mul(bd);
            let bn = i64::from(b.n).saturating_mul(ad);
            let bdn = bd.saturating_mul(ad);
            let (Ok(an), Ok(adn), Ok(bn), Ok(bdn)) = (
                i32::try_from(an),
                i32::try_from(adn),
                i32::try_from(bn),
                i32::try_from(bdn),
            ) else {
                return false;
            };
            *a = Self { n: an, d: adn };
            *b = Self { n: bn, d: bdn };
        }
        true
    }

    /// `avifFractionAdd` (`sign` 1) and `avifFractionSub` (`sign` -1).
    fn combine(mut a: Self, mut b: Self, sign: i64) -> Option<Self> {
        if !Self::common_denominator(&mut a, &mut b) {
            return None;
        }
        let n = i64::from(a.n).saturating_add(sign.saturating_mul(i64::from(b.n)));
        let mut result = Self {
            n: i32::try_from(n).ok()?,
            d: a.d,
        };
        result.simplify();
        Some(result)
    }
}

/// A quotient of two `int32_t`s, which fits one but for `INT32_MIN / -1`.
fn narrow(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// `calcCenter`: half of `dim`, as a fraction.
fn center(dim: i32) -> Fraction {
    if dim % 2 != 0 {
        Fraction { n: dim, d: 2 }
    } else {
        Fraction { n: dim >> 1, d: 1 }
    }
}

/// `avifCropRectConvertCleanApertureBox`: the crop a `clap` box describes,
/// if it describes one -- whole pixels, inside the picture, and starting on a
/// chroma sample.
pub(crate) fn crop_rect(
    clap: &Clap,
    image_w: u32,
    image_h: u32,
    format: YuvFormat,
) -> Option<CropRect> {
    let signed = u32::cast_signed;
    let (width_n, width_d) = (signed(clap.width_n), signed(clap.width_d));
    let (height_n, height_d) = (signed(clap.height_n), signed(clap.height_d));
    let (horiz_n, horiz_d) = (signed(clap.horiz_off_n), signed(clap.horiz_off_d));
    let (vert_n, vert_d) = (signed(clap.vert_off_n), signed(clap.vert_off_d));
    if width_d <= 0 || height_d <= 0 || horiz_d <= 0 || vert_d <= 0 {
        return None;
    }
    if width_n < 0 || height_n < 0 {
        return None;
    }
    if width_n.checked_rem(width_d)? != 0 || height_n.checked_rem(height_d)? != 0 {
        return None;
    }
    let clap_w = width_n.checked_div(width_d)?;
    let clap_h = height_n.checked_div(height_d)?;
    let image_w = i32::try_from(image_w).ok()?;
    let image_h = i32::try_from(image_h).ok()?;
    let center_x = Fraction::combine(
        center(image_w),
        Fraction {
            n: horiz_n,
            d: horiz_d,
        },
        1,
    )?;
    let center_y = Fraction::combine(
        center(image_h),
        Fraction {
            n: vert_n,
            d: vert_d,
        },
        1,
    )?;
    let crop_x = Fraction::combine(center_x, Fraction { n: clap_w, d: 2 }, -1)?;
    if crop_x.n.checked_rem(crop_x.d)? != 0 {
        return None;
    }
    let crop_y = Fraction::combine(center_y, Fraction { n: clap_h, d: 2 }, -1)?;
    if crop_y.n.checked_rem(crop_y.d)? != 0 {
        return None;
    }
    if crop_x.n < 0 || crop_y.n < 0 {
        return None;
    }
    let rect = CropRect {
        x: u32::try_from(crop_x.n.checked_div(crop_x.d)?).ok()?,
        y: u32::try_from(crop_y.n.checked_div(crop_y.d)?).ok()?,
        width: u32::try_from(clap_w).ok()?,
        height: u32::try_from(clap_h).ok()?,
    };
    // avifCropRectIsValid.
    if rect.width == 0 || rect.height == 0 {
        return None;
    }
    let right = rect.x.checked_add(rect.width)?;
    let bottom = rect.y.checked_add(rect.height)?;
    if right > u32::try_from(image_w).ok()? || bottom > u32::try_from(image_h).ok()? {
        return None;
    }
    // avifCropRectRequiresUpsampling: a subsampled picture's crop starts on a
    // chroma sample.
    let odd_x =
        !rect.x.is_multiple_of(2) && matches!(format, YuvFormat::Yuv420 | YuvFormat::Yuv422);
    let odd_y = !rect.y.is_multiple_of(2) && format == YuvFormat::Yuv420;
    if odd_x || odd_y {
        return None;
    }
    Some(rect)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "tests fail loudly"
    )]

    use super::*;

    fn clap(w: (u32, u32), h: (u32, u32), x: (i32, u32), y: (i32, u32)) -> Clap {
        Clap {
            width_n: w.0,
            width_d: w.1,
            height_n: h.0,
            height_d: h.1,
            horiz_off_n: x.0.cast_unsigned(),
            horiz_off_d: x.1,
            vert_off_n: y.0.cast_unsigned(),
            vert_off_d: y.1,
        }
    }

    #[test]
    fn a_clean_aperture_is_a_crop_about_the_centre() {
        let rect = |c: Clap, w, h, f| crop_rect(&c, w, h, f);
        // The centre 60x40 of 100x80.
        assert_eq!(
            rect(
                clap((60, 1), (40, 1), (0, 1), (0, 1)),
                100,
                80,
                YuvFormat::Yuv444
            ),
            Some(CropRect {
                x: 20,
                y: 20,
                width: 60,
                height: 40
            })
        );
        // Offset to the top left, which Chrome keeps.
        assert_eq!(
            rect(
                clap((60, 1), (40, 1), (-20, 1), (-20, 1)),
                100,
                80,
                YuvFormat::Yuv420
            ),
            Some(CropRect {
                x: 0,
                y: 0,
                width: 60,
                height: 40
            })
        );
        // Fractions that are whole, and fractions that are not.
        assert!(
            rect(
                clap((120, 2), (80, 2), (0, 1), (0, 1)),
                100,
                80,
                YuvFormat::Yuv444
            )
            .is_some()
        );
        assert!(
            rect(
                clap((61, 2), (40, 1), (0, 1), (0, 1)),
                100,
                80,
                YuvFormat::Yuv444
            )
            .is_none()
        );
        // Outside the picture; a zero or negative denominator.
        assert!(
            rect(
                clap((60, 1), (40, 1), (40, 1), (0, 1)),
                100,
                80,
                YuvFormat::Yuv444
            )
            .is_none()
        );
        assert!(
            rect(
                clap((60, 0), (40, 1), (0, 1), (0, 1)),
                100,
                80,
                YuvFormat::Yuv444
            )
            .is_none()
        );
        // An odd origin in a subsampled picture.
        assert!(
            rect(
                clap((60, 1), (40, 1), (1, 1), (0, 1)),
                100,
                80,
                YuvFormat::Yuv422
            )
            .is_none()
        );
        assert!(
            rect(
                clap((60, 1), (40, 1), (1, 1), (0, 1)),
                100,
                80,
                YuvFormat::Yuv444
            )
            .is_some()
        );
    }

    #[test]
    fn a_grid_box_is_version_0_with_nothing_after_it() {
        assert_eq!(
            parse_grid(&[0, 0, 1, 2, 0, 100, 0, 50]),
            Some(Grid {
                rows: 2,
                columns: 3,
                output_width: 100,
                output_height: 50
            })
        );
        assert_eq!(
            parse_grid(&[0, 1, 0, 0, 0, 0, 0, 7, 0, 0, 0, 9])
                .map(|g| (g.output_width, g.output_height)),
            Some((7, 9))
        );
        assert_eq!(parse_grid(&[1, 0, 1, 2, 0, 100, 0, 50]), None);
        assert_eq!(parse_grid(&[0, 0, 1, 2, 0, 100, 0, 50, 0]), None);
        assert_eq!(parse_grid(&[0, 0, 1, 2, 0, 0, 0, 50]), None);
        assert_eq!(parse_grid(&[0, 1, 0, 0, 0, 0x80, 0, 1, 0, 0, 0, 1]), None);
    }

    #[test]
    fn a_tone_map_s_metadata_is_checked_as_libavif_checks_it() {
        let mut good = vec![0, 0, 0, 0, 0, 0];
        for value in [1u32, 1, 2, 1] {
            good.extend_from_slice(&value.to_be_bytes());
        }
        for value in [0u32, 1, 2, 1, 1, 1, 0, 1, 0, 1] {
            good.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(parse_tmap(&good), Ok(()));
        let mut version = good.clone();
        version[0] = 1;
        assert!(matches!(parse_tmap(&version), Err(Error::Unsupported(_))));
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(matches!(parse_tmap(&trailing), Err(Error::ToneMap(_))));
        // A maximum below the minimum.
        let mut inverted = good;
        let at = 6 + 16 + 8;
        inverted[at..at + 4].copy_from_slice(&(-1i32).to_be_bytes());
        assert!(matches!(parse_tmap(&inverted), Err(Error::ToneMap(_))));
    }
}
