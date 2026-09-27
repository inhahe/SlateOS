//! The file structure of an AVIF picture -- ISO BMFF boxes (ISO/IEC 14496-12)
//! carrying HEIF items (ISO/IEC 23008-12) -- read as libavif 1.3.0 reads it:
//! `avifParse`, `avifParseMetaBox` and the parsers under them (`src/read.c`).
//!
//! A picture file is a list of top-level boxes. `ftyp` says what the file is:
//! an `avif` brand means a still picture described by a `meta` box, an `avis`
//! brand a sequence described by a `moov` box (`movie.rs`). The `meta` box
//! names *items* -- a coded picture (`av01`), a grid of them (`grid`),
//! metadata -- and says where each item's bytes are (`iloc`: in the file, or
//! in the box's own `idat`), what properties each has (`iprp`: its size, bit
//! depth, colour, rotation...) and how items relate (`iref`: this item is the
//! alpha of that one, those items are the tiles of this grid). Which item is
//! the picture is `pitm`.
//!
//! Every rule libavif applies while reading these is applied here, in its
//! order and with its outcome: a file libavif refuses is refused, for
//! libavif's reason. That includes the places libavif is lenient -- an
//! `iref` child box's own size is only checked to fit, never used -- since
//! a reader stricter than the one the files were tested against refuses files
//! that open everywhere else. The experimental parts of libavif (the `mini`
//! box, sample transforms) are compiled out of the builds Pillow and Chrome
//! use, and are out of here.
//!
//! Portions of this file are copyright 2019 Joe Drago, from libavif, and
//! used under its BSD-2-Clause licence: `licenses/libavif-LICENSE.txt`.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::Error;
use super::movie::{self, Track};
use super::stream::{Stream, Truncated};

/// A four-character code: a box type, a brand, an item type.
pub(super) type FourCc = [u8; 4];

/// `AVIF_RESULT_BMFF_PARSE_FAILED` for a box named `what`.
const fn bad(what: &'static str) -> impl Fn(Truncated) -> Error + Copy {
    move |_| Error::Parse(what)
}

/// A file's top-level structure, as far as a picture needs it.
#[derive(Debug)]
pub(super) struct File<'a> {
    /// The whole file: item extents and samples are offsets into it.
    pub(super) bytes: &'a [u8],
    pub(super) major_brand: FourCc,
    pub(super) compatible_brands: Vec<FourCc>,
    /// The root `meta` box's contents; empty if the file has none, as
    /// libavif's is.
    pub(super) meta: Meta<'a>,
    /// The `moov` box's tracks, in order.
    pub(super) tracks: Vec<Track<'a>>,
}

impl File<'_> {
    /// `avifBrandArrayHasBrand` on the compatible brands alone -- which is
    /// what libavif asks of the `tmap` brand once the file is parsed, where
    /// parsing itself asked of the major brand too.
    pub(super) fn is_compatible_with(&self, brand: &FourCc) -> bool {
        self.compatible_brands.contains(brand)
    }
}

/// A `meta` box: its items, and what they share.
#[derive(Debug, Default)]
pub(super) struct Meta<'a> {
    /// `pitm`: the primary item, 0 if the box names none.
    pub(super) primary: u32,
    /// Every item any box mentioned, in the order they were first mentioned:
    /// libavif searches them in this order, and the first match wins.
    pub(super) items: Vec<Item>,
    /// Where each item ID is in `items`. libavif searches the list; an index
    /// gives the same answers without costing a hostile file's item count
    /// squared.
    index: BTreeMap<u32, usize>,
    /// `ipco`, in order: `ipma` refers to these by 1-based index, and items
    /// hold 0-based indices into it.
    pub(super) properties: Vec<Property<'a>>,
    /// `idat`: the bytes of items whose construction method is 1.
    pub(super) idat: Option<&'a [u8]>,
    /// `grpl`: entity groups, of which libavif reads `altr` ("alternatives").
    pub(super) groups: Vec<Group>,
}

impl<'a> Meta<'a> {
    /// The position of item `id` in [`Meta::items`].
    pub(super) fn position(&self, id: u32) -> Option<usize> {
        self.index.get(&id).copied()
    }

    #[cfg(test)]
    pub(super) fn item(&self, id: u32) -> Option<&Item> {
        self.items.get(self.position(id)?)
    }

    /// `avifMetaFindOrCreateItem`: the position of the item with `id`, created
    /// empty if no box has named it yet -- items come into being in whichever
    /// box mentions them first.
    pub(super) fn find_or_create(&mut self, id: u32) -> usize {
        if let Some(&at) = self.index.get(&id) {
            return at;
        }
        let at = self.items.len();
        self.items.push(Item::new(id));
        self.index.insert(id, at);
        at
    }

    pub(super) fn item_mut(&mut self, id: u32) -> &mut Item {
        let at = self.find_or_create(id);
        // `find_or_create` returns a position that exists.
        #[allow(clippy::indexing_slicing, reason = "a position just found or pushed")]
        &mut self.items[at]
    }

    /// `avifPropertyArrayFind` on an item's properties: the first of type
    /// `kind`.
    pub(super) fn find_property(&self, item: &Item, kind: &FourCc) -> Option<&Property<'a>> {
        item.properties
            .iter()
            .filter_map(|&at| self.properties.get(at))
            .find(|p| &p.kind() == kind)
    }
}

/// Where part of an item's bytes lie: in the file, or in `idat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Extent {
    pub(super) offset: u64,
    pub(super) size: usize,
}

/// One item of a `meta` box: `avifDecoderItem`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Item {
    pub(super) id: u32,
    /// `infe`'s item type -- `av01`, `grid`, `Exif`... -- or zeros for an item
    /// no `infe` describes.
    pub(super) kind: FourCc,
    /// Where the item's bytes are, in order. A run of identical empty extents
    /// is kept as one: reading the second changes nothing the first did not,
    /// and a file can declare 65535 of them per item for no bytes at all.
    pub(super) extents: Vec<Extent>,
    /// The sum of the extents' sizes.
    pub(super) size: usize,
    /// Construction method 1: the extents are offsets into `idat`.
    pub(super) idat_stored: bool,
    /// Indices into [`Meta::properties`], in `ipma`'s order.
    pub(super) properties: Vec<usize>,
    ipma_seen: bool,
    /// An essential property this reader does not know: the item must not be
    /// used (ISO/IEC 23008-12 section 10.2.1).
    pub(super) unsupported_essential: bool,
    /// `iref`: this item is a thumbnail of, the auxiliary image of, and
    /// premultiplied by, those items -- 0 for none. (A `cdsc` reference, "a
    /// description of", ties Exif and XMP to their picture, which is all
    /// libavif uses it for; neither is read here, so it is not kept.)
    pub(super) thumbnail_for: u32,
    pub(super) aux_for: u32,
    pub(super) prem_by: u32,
    /// `dimg`, read backwards: this item is input `dimg_idx` of item
    /// `dimg_for` -- a grid's tile, or a tone-mapped image's input.
    pub(super) dimg_for: u32,
    pub(super) dimg_idx: u16,
    has_dimg_from: bool,
    /// `ispe`'s size, once `avifDecoderParse` has harvested it.
    pub(super) width: u32,
    pub(super) height: u32,
}

impl Item {
    pub(super) const fn new(id: u32) -> Self {
        Self {
            id,
            kind: [0; 4],
            extents: Vec::new(),
            size: 0,
            idat_stored: false,
            properties: Vec::new(),
            ipma_seen: false,
            unsupported_essential: false,
            thumbnail_for: 0,
            aux_for: 0,
            prem_by: 0,
            dimg_for: 0,
            dimg_idx: 0,
            has_dimg_from: false,
            width: 0,
            height: 0,
        }
    }

    /// Whether any extent was declared for the item.
    pub(super) fn has_extents(&self) -> bool {
        !self.extents.is_empty()
    }
}

/// An entity group of `grpl`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Group {
    pub(super) kind: FourCc,
    pub(super) entities: Vec<u32>,
}

/// An item property box, as far as libavif reads it: `avifProperty`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Property<'a> {
    /// `ispe`: the picture's size.
    Ispe { width: u32, height: u32 },
    /// `auxC` of an item, or `auxi` of a track: what an auxiliary image is,
    /// by URN.
    Aux { kind: FourCc, aux_type: &'a [u8] },
    /// `colr`.
    Colr(Colr<'a>),
    /// `av1C`: the AV1 codec configuration.
    Av1C(Av1Config),
    /// `pasp`: pixel aspect ratio, reported and not applied.
    Pasp { h_spacing: u32, v_spacing: u32 },
    /// `clap`: the clean aperture, a crop.
    Clap(Clap),
    /// `irot`: anticlockwise quarter turns, 0 to 3.
    Irot(u8),
    /// `imir`: 0 exchanges top and bottom, 1 left and right.
    Imir(u8),
    /// `pixi`: bits per channel, the same for every channel.
    Pixi { planes: u8, depth: u8 },
    /// `a1op`: the AV1 operating point to decode.
    A1op(u8),
    /// `lsel`: the layer to show, 0xFFFF for all.
    Lsel(u16),
    /// `a1lx`: the sizes of an AV1 image's first three layers.
    A1lx([u32; 3]),
    /// `clli`: content light level, reported and not applied.
    Clli,
    /// Anything else, kept by type: an item's unknown *essential* property
    /// makes it unusable, and its type still answers `avifPropertyArrayFind`.
    Opaque(FourCc),
}

impl Property<'_> {
    /// The four-character type of the box the property came from.
    pub(super) const fn kind(&self) -> FourCc {
        match self {
            Self::Ispe { .. } => *b"ispe",
            Self::Aux { kind, .. } | Self::Opaque(kind) => *kind,
            Self::Colr(_) => *b"colr",
            Self::Av1C(_) => *b"av1C",
            Self::Pasp { .. } => *b"pasp",
            Self::Clap(_) => *b"clap",
            Self::Irot(_) => *b"irot",
            Self::Imir(_) => *b"imir",
            Self::Pixi { .. } => *b"pixi",
            Self::A1op(_) => *b"a1op",
            Self::Lsel(_) => *b"lsel",
            Self::A1lx(_) => *b"a1lx",
            Self::Clli => *b"clli",
        }
    }
}

/// A `colr` box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Colr<'a> {
    /// `rICC` or `prof`: an ICC profile, which is the rest of the box.
    Icc(&'a [u8]),
    /// `nclx`: code points.
    Nclx(Cicp),
    /// A colour type libavif does not read.
    Other,
}

/// Coding-independent code points (ITU-T H.273): the picture's colour
/// primaries, transfer function, YUV matrix and range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cicp {
    pub(super) primaries: u16,
    pub(super) transfer: u16,
    pub(super) matrix: u16,
    pub(super) full_range: bool,
}

/// `av1C`'s four bytes of fields: `avifCodecConfigurationBox`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Av1Config {
    pub(super) seq_profile: u8,
    pub(super) seq_level_idx0: u8,
    pub(super) seq_tier0: u8,
    pub(super) high_bitdepth: bool,
    pub(super) twelve_bit: bool,
    pub(super) monochrome: bool,
    pub(super) chroma_subsampling_x: bool,
    pub(super) chroma_subsampling_y: bool,
    pub(super) chroma_sample_position: u8,
}

impl Av1Config {
    /// `avifCodecConfigurationBoxGetDepth`.
    pub(super) const fn depth(&self) -> u8 {
        if self.twelve_bit {
            12
        } else if self.high_bitdepth {
            10
        } else {
            8
        }
    }
}

/// `clap`'s eight fields: the crop, as fractions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Clap {
    pub(super) width_n: u32,
    pub(super) width_d: u32,
    pub(super) height_n: u32,
    pub(super) height_d: u32,
    pub(super) horiz_off_n: u32,
    pub(super) horiz_off_d: u32,
    pub(super) vert_off_n: u32,
    pub(super) vert_off_d: u32,
}

/// `avifGetCodecType`: whether an item or sample entry type is a coded AV1
/// picture. (`av02`, AV2, exists only in builds with the experimental AVM
/// codec.)
pub(super) fn is_av1(kind: &FourCc) -> bool {
    kind == b"av01"
}

/// `avifPeekCompatibleFileType`: whether `bytes` begin as an AVIF file does
/// -- an `ftyp` box first, of a known size and whole, whose brands include
/// `avif` or `avis`.
///
/// Chrome asks this of a file's first 144 bytes with CrabbyAvif, which reads
/// at most 32 compatible brands; libavif, asked of the whole file, reads them
/// all. The two differ only for an `ftyp` box longer than 144 bytes, which no
/// encoder writes.
#[must_use]
pub(super) fn sniff(bytes: &[u8]) -> bool {
    let mut s = Stream::new(bytes);
    let Ok(header) = s.box_header_partial(true) else {
        return false;
    };
    let Some(size) = header.size else {
        return false;
    };
    if &header.kind != b"ftyp" {
        return false;
    }
    let Ok(payload) = s.bytes(size) else {
        return false;
    };
    parse_ftyp(payload).is_ok_and(|(major, compatible)| is_compatible(&major, &compatible))
}

/// `avifFileTypeHasBrand`.
fn has_brand(major: &FourCc, compatible: &[FourCc], brand: &FourCc) -> bool {
    major == brand || compatible.contains(brand)
}

/// `avifFileTypeIsCompatible`.
fn is_compatible(major: &FourCc, compatible: &[FourCc]) -> bool {
    has_brand(major, compatible, b"avif") || has_brand(major, compatible, b"avis")
}

/// `avifParseFileTypeBox`: the major brand and the compatible ones.
fn parse_ftyp(payload: &[u8]) -> Result<(FourCc, Vec<FourCc>), Truncated> {
    let mut s = Stream::new(payload);
    let major = s.array::<4>()?;
    s.skip(4)?; // minor_version
    let rest = s.rest();
    if !rest.len().is_multiple_of(4) {
        return Err(Truncated);
    }
    let compatible = rest
        .chunks_exact(4)
        .filter_map(|brand| brand.try_into().ok())
        .collect();
    Ok((major, compatible))
}

/// The most bytes libavif reads to find a top-level box's header.
const TOP_LEVEL_HEADER_READ: usize = 32;

/// `avifParse`: the top-level boxes, read until everything the brands require
/// has been seen.
pub(super) fn parse(bytes: &[u8]) -> Result<File<'_>, Error> {
    let size_hint = bytes.len();
    let mut offset = 0usize;
    let mut ftyp: Option<(FourCc, Vec<FourCc>)> = None;
    let mut meta: Option<Meta<'_>> = None;
    let mut tracks: Option<Vec<Track<'_>>> = None;
    let (mut needs_meta, mut needs_moov, mut needs_tmap) = (false, false, false);
    let mut tmap_seen = false;
    let mut meta_is_size_zero = false;

    loop {
        // The memory reader's size hint is the file's length; a hint of 0
        // means "unknown" to libavif, which an empty file then reads as.
        if size_hint > 0 && offset > size_hint {
            return Err(Error::Parse("AVIF box past the end of the file"));
        }
        let rest = bytes.get(offset..).unwrap_or_default();
        if rest.is_empty() {
            break;
        }
        let window = rest.get(..TOP_LEVEL_HEADER_READ).unwrap_or(rest);
        let mut hs = Stream::new(window);
        let header = hs
            .box_header_partial(true)
            .map_err(bad("AVIF top-level box header"))?;
        offset = offset.saturating_add(hs.offset());

        let kind = header.kind;
        let is_ftyp = &kind == b"ftyp";
        let is_meta = &kind == b"meta";
        let is_moov = &kind == b"moov";
        let non_skippable = is_ftyp || is_meta || is_moov;
        if is_meta {
            meta_is_size_zero = header.size.is_none();
        }
        // ISO/IEC 14496-12 section 6.3.4: `ftyp` comes before any
        // variable-length box.
        if !is_ftyp
            && (non_skippable || matches!(&kind, b"free" | b"skip" | b"mdat"))
            && ftyp.is_none()
        {
            return Err(Error::Parse("AVIF box before the ftyp box"));
        }

        let mut contents: &[u8] = &[];
        if non_skippable {
            let available = bytes.get(offset..).unwrap_or_default();
            contents = match header.size {
                None => available,
                Some(size) => available.get(..size).ok_or(Error::Truncated)?,
            };
            offset = offset.saturating_add(contents.len());
        } else {
            match header.size {
                // An unknown box running to the end of the file: nothing can
                // follow it, and what was needed has not been seen.
                None => return Err(Error::Parse("AVIF box of size 0")),
                Some(size) => {
                    offset = offset
                        .checked_add(size)
                        .ok_or(Error::Parse("AVIF box size"))?;
                }
            }
        }

        if is_ftyp {
            if ftyp.is_some() {
                return Err(Error::Parse("AVIF file with two ftyp boxes"));
            }
            let (major, compatible) = parse_ftyp(contents).map_err(bad("AVIF ftyp box"))?;
            if !is_compatible(&major, &compatible) {
                return Err(Error::NotAvif);
            }
            needs_meta = has_brand(&major, &compatible, b"avif");
            needs_moov = has_brand(&major, &compatible, b"avis");
            needs_tmap = has_brand(&major, &compatible, b"tmap");
            if needs_tmap {
                needs_meta = true;
            }
            ftyp = Some((major, compatible));
        } else if is_meta {
            if meta.is_some() {
                return Err(Error::Parse("AVIF file with two meta boxes"));
            }
            let mut parsed = Meta::default();
            parse_meta(&mut parsed, contents)?;
            tmap_seen = parsed.items.iter().any(|item| &item.kind == b"tmap");
            meta = Some(parsed);
        } else if is_moov {
            if tracks.is_some() {
                return Err(Error::Parse("AVIF file with two moov boxes"));
            }
            tracks = Some(movie::parse_moov(contents)?);
        }

        let seen_all = ftyp.is_some()
            && (!needs_meta || meta.is_some())
            && (!needs_moov || tracks.is_some())
            && (!needs_tmap || tmap_seen);
        if seen_all {
            break;
        }
    }

    let Some((major_brand, compatible_brands)) = ftyp else {
        return Err(Error::NotAvif);
    };
    let seen_all = (!needs_meta || meta.is_some()) && (!needs_moov || tracks.is_some());
    if !seen_all {
        return Err(Error::Truncated);
    }
    if needs_tmap && !tmap_seen {
        return Err(if meta_is_size_zero {
            Error::Truncated
        } else {
            Error::Parse("AVIF tmap brand with no tmap item")
        });
    }
    Ok(File {
        bytes,
        major_brand,
        compatible_brands,
        meta: meta.unwrap_or_default(),
        tracks: tracks.unwrap_or_default(),
    })
}

/// The boxes a `meta` box may hold at most one of (`avifUniqueBoxFlag`).
const UNIQUE: [FourCc; 7] = [
    *b"iloc", *b"pitm", *b"idat", *b"iprp", *b"iinf", *b"iref", *b"grpl",
];

/// `avifParseMetaBox`, into `meta`: a track may hold more than one `meta`
/// box, and libavif reads each into the same `avifMeta`, so the rules that
/// span boxes -- one `idat`, one `pitm`, one set of extents per item -- span
/// them too.
pub(super) fn parse_meta<'a>(meta: &mut Meta<'a>, payload: &'a [u8]) -> Result<(), Error> {
    let mut s = Stream::new(payload);
    s.enforce_version(0).map_err(bad("AVIF meta box"))?;
    let mut first = true;
    let mut seen = [false; UNIQUE.len()];
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header().map_err(bad("AVIF meta box child"))?;
        let body = s.bytes(size).map_err(bad("AVIF meta box child"))?;
        if first {
            // `hdlr` first, and its handler `pict`.
            if &kind != b"hdlr" {
                return Err(Error::Parse("AVIF meta box without hdlr first"));
            }
            let handler = parse_hdlr(body).map_err(bad("AVIF hdlr box"))?;
            if &handler != b"pict" {
                return Err(Error::Parse("AVIF hdlr handler other than pict"));
            }
            first = false;
            continue;
        }
        if &kind == b"hdlr" {
            return Err(Error::Parse("AVIF meta box with two hdlr boxes"));
        }
        let Some(which) = UNIQUE.iter().position(|unique| unique == &kind) else {
            continue; // skipped, as libavif skips it
        };
        let Some(flag) = seen.get_mut(which) else {
            continue;
        };
        if *flag {
            return Err(Error::Parse("AVIF meta box with a repeated unique box"));
        }
        *flag = true;
        match &kind {
            b"iloc" => parse_iloc(meta, body)?,
            b"pitm" => parse_pitm(meta, body).map_err(bad("AVIF pitm box"))?,
            b"idat" => {
                if meta.idat.is_some_and(|idat| !idat.is_empty()) || body.is_empty() {
                    return Err(Error::Parse("AVIF idat box"));
                }
                meta.idat = Some(body);
            }
            b"iprp" => parse_iprp(meta, body)?,
            b"iinf" => parse_iinf(meta, body)?,
            b"iref" => parse_iref(meta, body)?,
            _ => parse_grpl(meta, body)?,
        }
    }
    if first {
        return Err(Error::Parse("AVIF meta box with no boxes in it"));
    }
    Ok(())
}

/// `avifParseHandlerBox`: the handler type.
pub(super) fn parse_hdlr(payload: &[u8]) -> Result<FourCc, Truncated> {
    let mut s = Stream::new(payload);
    s.enforce_version(0)?;
    if s.u32()? != 0 {
        return Err(Truncated); // pre_defined
    }
    let handler = s.array::<4>()?;
    for _ in 0..3 {
        s.u32()?; // reserved
    }
    s.string()?; // name
    Ok(handler)
}

/// `avifCheckItemID`: item 0 names nothing.
fn check_item_id(id: u32, what: &'static str) -> Result<u32, Error> {
    if id == 0 {
        Err(Error::Parse(what))
    } else {
        Ok(id)
    }
}

/// `avifParseItemLocationBox`.
fn parse_iloc(meta: &mut Meta<'_>, payload: &[u8]) -> Result<(), Error> {
    const WHAT: &str = "AVIF iloc box";
    let bad = bad(WHAT);
    let mut s = Stream::new(payload);
    let (version, _) = s.version_and_flags().map_err(bad)?;
    if version > 2 {
        return Err(Error::Parse(WHAT));
    }
    let offset_size = s.bits(4).map_err(bad)?;
    let length_size = s.bits(4).map_err(bad)?;
    let base_offset_size = s.bits(4).map_err(bad)?;
    let index_size = if version == 1 || version == 2 {
        s.bits(4).map_err(bad)?
    } else {
        s.bits(4).map_err(bad)?; // reserved
        0
    };
    let valid = |v: u32| matches!(v, 0 | 4 | 8);
    if !(valid(offset_size) && valid(length_size) && valid(base_offset_size) && valid(index_size)) {
        return Err(Error::Parse(WHAT));
    }
    let item_count = if version < 2 {
        u32::from(s.u16().map_err(bad)?)
    } else {
        s.u32().map_err(bad)?
    };
    for _ in 0..item_count {
        let id = if version < 2 {
            u32::from(s.u16().map_err(bad)?)
        } else {
            s.u32().map_err(bad)?
        };
        check_item_id(id, WHAT)?;
        if meta.item_mut(id).has_extents() {
            return Err(Error::Parse("AVIF item with two sets of extents"));
        }
        if version == 1 || version == 2 {
            if s.bits(12).map_err(bad)? != 0 {
                return Err(Error::Parse(WHAT)); // reserved
            }
            match s.bits(4).map_err(bad)? {
                0 => {}
                1 => meta.item_mut(id).idat_stored = true,
                // Construction method 2, item offset, is unsupported.
                _ => return Err(Error::Parse("AVIF iloc construction method")),
            }
        }
        s.u16().map_err(bad)?; // data_reference_index
        let base_offset = s.ux8(base_offset_size).map_err(bad)?;
        let extent_count = s.u16().map_err(bad)?;
        for _ in 0..extent_count {
            if (version == 1 || version == 2) && index_size > 0 {
                s.ux8(index_size).map_err(bad)?; // item_reference_index, unused
            }
            let extent_offset = s.ux8(offset_size).map_err(bad)?;
            let extent_length = s.ux8(length_size).map_err(bad)?;
            let offset = base_offset
                .checked_add(extent_offset)
                .ok_or(Error::Parse("AVIF extent offset"))?;
            let size = usize::try_from(extent_length).map_err(|_| Error::Parse(WHAT))?;
            let item = meta.item_mut(id);
            item.size = item
                .size
                .checked_add(size)
                .ok_or(Error::Parse("AVIF item size"))?;
            let extent = Extent { offset, size };
            if !(size == 0 && item.extents.last() == Some(&extent)) {
                item.extents.push(extent);
            }
        }
    }
    Ok(())
}

/// `avifParsePrimaryItemBox`.
fn parse_pitm(meta: &mut Meta<'_>, payload: &[u8]) -> Result<(), Truncated> {
    if meta.primary > 0 {
        return Err(Truncated);
    }
    let mut s = Stream::new(payload);
    let (version, _) = s.version_and_flags()?;
    meta.primary = if version == 0 {
        u32::from(s.u16()?)
    } else {
        s.u32()?
    };
    Ok(())
}

/// The most `ipma` boxes of distinct version and flags libavif keeps track of
/// (`MAX_IPMA_VERSION_AND_FLAGS_SEEN`).
const MAX_IPMA_KINDS: usize = 4;

/// `avifParseItemPropertiesBox`: `ipco`, then `ipma` boxes.
fn parse_iprp<'a>(meta: &mut Meta<'a>, payload: &'a [u8]) -> Result<(), Error> {
    const WHAT: &str = "AVIF iprp box";
    let mut s = Stream::new(payload);
    let (kind, size) = s.box_header().map_err(bad(WHAT))?;
    if &kind != b"ipco" {
        return Err(Error::Parse("AVIF iprp box without ipco first"));
    }
    let ipco = s.bytes(size).map_err(bad(WHAT))?;
    parse_ipco(&mut meta.properties, ipco, false)?;
    let mut kinds_seen: Vec<u32> = Vec::new();
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header().map_err(bad(WHAT))?;
        if &kind != b"ipma" {
            return Err(Error::Parse("AVIF iprp box holding other than ipma"));
        }
        let body = s.bytes(size).map_err(bad(WHAT))?;
        let version_and_flags = parse_ipma(meta, body)?;
        if kinds_seen.contains(&version_and_flags) || kinds_seen.len() == MAX_IPMA_KINDS {
            return Err(Error::Parse("AVIF ipma boxes of one version and flags"));
        }
        kinds_seen.push(version_and_flags);
    }
    Ok(())
}

/// `avifParseItemPropertyContainerBox`: `is_track` for a sample entry's
/// properties, where an auxiliary image's type is `auxi` rather than `auxC`.
pub(super) fn parse_ipco<'a>(
    properties: &mut Vec<Property<'a>>,
    payload: &'a [u8],
    is_track: bool,
) -> Result<(), Error> {
    let mut s = Stream::new(payload);
    while s.has_bytes_left(1) {
        let (kind, size) = s.box_header().map_err(bad("AVIF ipco box"))?;
        let body = s.bytes(size).map_err(bad("AVIF ipco box"))?;
        properties.push(parse_property(kind, body, is_track)?);
    }
    Ok(())
}

/// The most channels `pixi` may describe (`MAX_PIXI_PLANE_DEPTHS`).
const MAX_PIXI_PLANES: u8 = 4;

/// The most AV1 spatial layers (`AVIF_MAX_AV1_LAYER_COUNT`).
pub(super) const MAX_AV1_LAYERS: u16 = 4;

/// One property box: the parsers `avifParseItemPropertyContainerBox` calls.
fn parse_property(kind: FourCc, body: &[u8], is_track: bool) -> Result<Property<'_>, Error> {
    let mut s = Stream::new(body);
    Ok(match &kind {
        b"ispe" => {
            let bad = bad("AVIF ispe box");
            s.enforce_version(0).map_err(bad)?;
            Property::Ispe {
                width: s.u32().map_err(bad)?,
                height: s.u32().map_err(bad)?,
            }
        }
        b"auxC" | b"auxi" if (&kind == b"auxi") == is_track => {
            let bad = bad("AVIF auxC box");
            s.enforce_version(0).map_err(bad)?;
            Property::Aux {
                kind,
                aux_type: s.string().map_err(bad)?,
            }
        }
        b"colr" => Property::Colr(parse_colr(&mut s).map_err(bad("AVIF colr box"))?),
        b"av1C" => Property::Av1C(parse_av1c(&mut s).map_err(bad("AVIF av1C box"))?),
        b"pasp" => {
            let bad = bad("AVIF pasp box");
            Property::Pasp {
                h_spacing: s.u32().map_err(bad)?,
                v_spacing: s.u32().map_err(bad)?,
            }
        }
        b"clap" => Property::Clap(parse_clap(&mut s).map_err(bad("AVIF clap box"))?),
        b"irot" => {
            let bad = bad("AVIF irot box");
            if s.bits(6).map_err(bad)? != 0 {
                return Err(Error::Parse("AVIF irot box"));
            }
            Property::Irot(narrow(s.bits(2).map_err(bad)?))
        }
        b"imir" => {
            let bad = bad("AVIF imir box");
            if s.bits(7).map_err(bad)? != 0 {
                return Err(Error::Parse("AVIF imir box"));
            }
            Property::Imir(narrow(s.bits(1).map_err(bad)?))
        }
        b"pixi" => {
            let bad = bad("AVIF pixi box");
            s.enforce_version(0).map_err(bad)?;
            let planes = s.u8().map_err(bad)?;
            if planes == 0 || planes > MAX_PIXI_PLANES {
                return Err(Error::Unsupported("AVIF pixi plane count"));
            }
            let depth = s.u8().map_err(bad)?;
            for _ in 1..planes {
                if s.u8().map_err(bad)? != depth {
                    return Err(Error::Unsupported("AVIF planes of different depths"));
                }
            }
            Property::Pixi { planes, depth }
        }
        b"a1op" => {
            let op = s.u8().map_err(bad("AVIF a1op box"))?;
            if op > 31 {
                return Err(Error::Parse("AVIF a1op operating point"));
            }
            Property::A1op(op)
        }
        b"lsel" => {
            let layer = s.u16().map_err(bad("AVIF lsel box"))?;
            if layer != 0xFFFF && layer >= MAX_AV1_LAYERS {
                return Err(Error::Parse("AVIF lsel layer"));
            }
            Property::Lsel(layer)
        }
        b"a1lx" => {
            let bad = bad("AVIF a1lx box");
            let large = s.u8().map_err(bad)?;
            if large & 0xFE != 0 {
                return Err(Error::Parse("AVIF a1lx box"));
            }
            let mut sizes = [0u32; 3];
            for size in &mut sizes {
                *size = if large == 0 {
                    u32::from(s.u16().map_err(bad)?)
                } else {
                    s.u32().map_err(bad)?
                };
            }
            Property::A1lx(sizes)
        }
        b"clli" => {
            // `avifParseContentLightLevelInformationBox`: two numbers.
            let bad = bad("AVIF clli box");
            s.bits(16).map_err(bad)?;
            s.bits(16).map_err(bad)?;
            Property::Clli
        }
        _ => Property::Opaque(kind),
    })
}

/// A field of at most eight bits, as the byte libavif stores it in.
fn narrow(bits: u32) -> u8 {
    u8::try_from(bits).unwrap_or(u8::MAX)
}

/// `avifParseColourInformationBox`.
fn parse_colr<'a>(s: &mut Stream<'a>) -> Result<Colr<'a>, Truncated> {
    let colour_type = s.array::<4>()?;
    Ok(match &colour_type {
        b"rICC" | b"prof" => Colr::Icc(s.rest()),
        b"nclx" => {
            let primaries = s.u16()?;
            let transfer = s.u16()?;
            let matrix = s.u16()?;
            let full_range = s.bits(1)? == 1;
            if s.bits(7)? != 0 {
                return Err(Truncated); // reserved
            }
            Colr::Nclx(Cicp {
                primaries,
                transfer,
                matrix,
                full_range,
            })
        }
        _ => Colr::Other,
    })
}

/// `avifParseCodecConfiguration`: `av1C`'s four bytes. The `configOBUs` after
/// them are not read, as libavif does not read them.
pub(super) fn parse_av1c(s: &mut Stream<'_>) -> Result<Av1Config, Truncated> {
    if s.bits(1)? != 1 {
        return Err(Truncated); // marker
    }
    if s.bits(7)? != 1 {
        return Err(Truncated); // version
    }
    let seq_profile = narrow(s.bits(3)?);
    let seq_level_idx0 = narrow(s.bits(5)?);
    let seq_tier0 = narrow(s.bits(1)?);
    let high_bitdepth = s.bits(1)? == 1;
    let twelve_bit = s.bits(1)? == 1;
    let monochrome = s.bits(1)? == 1;
    let chroma_subsampling_x = s.bits(1)? == 1;
    let chroma_subsampling_y = s.bits(1)? == 1;
    let chroma_sample_position = narrow(s.bits(2)?);
    s.skip(1)?; // reserved and initial_presentation_delay
    Ok(Av1Config {
        seq_profile,
        seq_level_idx0,
        seq_tier0,
        high_bitdepth,
        twelve_bit,
        monochrome,
        chroma_subsampling_x,
        chroma_subsampling_y,
        chroma_sample_position,
    })
}

/// `avifParseCleanApertureBoxProperty`.
fn parse_clap(s: &mut Stream<'_>) -> Result<Clap, Truncated> {
    Ok(Clap {
        width_n: s.u32()?,
        width_d: s.u32()?,
        height_n: s.u32()?,
        height_d: s.u32()?,
        horiz_off_n: s.u32()?,
        horiz_off_d: s.u32()?,
        vert_off_n: s.u32()?,
        vert_off_d: s.u32()?,
    })
}

/// `avifParseItemPropertyAssociation`: returns the box's version and flags.
fn parse_ipma(meta: &mut Meta<'_>, payload: &[u8]) -> Result<u32, Error> {
    const WHAT: &str = "AVIF ipma box";
    let bad = bad(WHAT);
    let mut s = Stream::new(payload);
    let (version, flags) = s.version_and_flags().map_err(bad)?;
    let index_bits: u8 = if flags & 1 != 0 { 15 } else { 7 };
    let entry_count = s.u32().map_err(bad)?;
    let mut previous = 0u32;
    for _ in 0..entry_count {
        let id = if version < 1 {
            u32::from(s.u16().map_err(bad)?)
        } else {
            s.u32().map_err(bad)?
        };
        check_item_id(id, WHAT)?;
        // Ordered by increasing item ID, each at most once.
        if id <= previous {
            return Err(Error::Parse("AVIF ipma items out of order"));
        }
        previous = id;
        let at = meta.find_or_create(id);
        let associations = {
            let item = meta.items.get_mut(at).ok_or(Error::Parse(WHAT))?;
            if item.ipma_seen {
                return Err(Error::Parse("AVIF item in two ipma boxes"));
            }
            item.ipma_seen = true;
            s.u8().map_err(bad)?
        };
        for _ in 0..associations {
            let essential = s.bits(1).map_err(bad)? == 1;
            let index = s.bits(index_bits).map_err(bad)?;
            if index == 0 {
                if essential {
                    return Err(Error::Parse("AVIF essential property 0"));
                }
                continue;
            }
            let index = usize::try_from(index.wrapping_sub(1)).map_err(|_| Error::Parse(WHAT))?;
            let property = meta
                .properties
                .get(index)
                .ok_or(Error::Parse("AVIF ipma property index"))?;
            let kind = property.kind();
            let known = !matches!(property, Property::Opaque(_));
            let item = meta.items.get_mut(at).ok_or(Error::Parse(WHAT))?;
            if known {
                // AVIF 2.3.2.3.2: `a1lx` shall not be essential. AVIF 2.3.2.1.1,
                // HEIF 6.5.11.1 and MIAF 7.3.9: `a1op`, `lsel` and the
                // transforms shall be.
                if essential && &kind == b"a1lx" {
                    return Err(Error::Parse("AVIF essential a1lx"));
                }
                if !essential && matches!(&kind, b"a1op" | b"lsel" | b"clap" | b"irot" | b"imir") {
                    return Err(Error::Parse("AVIF inessential transform"));
                }
            } else if essential {
                item.unsupported_essential = true;
            }
            item.properties.push(index);
        }
    }
    Ok(u32::from(version) << 24 | flags)
}

/// `avifParseItemInfoBox`.
fn parse_iinf(meta: &mut Meta<'_>, payload: &[u8]) -> Result<(), Error> {
    const WHAT: &str = "AVIF iinf box";
    let bad = bad(WHAT);
    let mut s = Stream::new(payload);
    let (version, _) = s.version_and_flags().map_err(bad)?;
    let entry_count = match version {
        0 => u32::from(s.u16().map_err(bad)?),
        1 => s.u32().map_err(bad)?,
        _ => return Err(Error::Parse(WHAT)),
    };
    for _ in 0..entry_count {
        let (kind, size) = s.box_header().map_err(bad)?;
        if &kind != b"infe" {
            return Err(Error::Parse("AVIF iinf box holding other than infe"));
        }
        parse_infe(meta, s.bytes(size).map_err(bad)?)?;
    }
    Ok(())
}

/// `avifParseItemInfoEntry`.
fn parse_infe(meta: &mut Meta<'_>, payload: &[u8]) -> Result<(), Error> {
    const WHAT: &str = "AVIF infe box";
    let bad = bad(WHAT);
    let mut s = Stream::new(payload);
    let (version, _) = s.version_and_flags().map_err(bad)?;
    let id = match version {
        2 => u32::from(s.u16().map_err(bad)?),
        3 => s.u32().map_err(bad)?,
        _ => return Err(Error::Parse(WHAT)),
    };
    check_item_id(id, WHAT)?;
    s.u16().map_err(bad)?; // item_protection_index
    let kind = s.array::<4>().map_err(bad)?;
    s.string().map_err(bad)?; // item_name
    if &kind == b"mime" {
        s.string().map_err(bad)?; // content_type
    }
    meta.item_mut(id).kind = kind;
    Ok(())
}

/// `avifParseItemReferenceBox`. Each child box's size is checked to fit and
/// then not used: the references are read straight on, as libavif reads
/// them.
fn parse_iref(meta: &mut Meta<'_>, payload: &[u8]) -> Result<(), Error> {
    const WHAT: &str = "AVIF iref box";
    let bad = bad(WHAT);
    let mut s = Stream::new(payload);
    let (version, _) = s.version_and_flags().map_err(bad)?;
    if version > 1 {
        return Ok(()); // skipped, as libavif skips it
    }
    let read_id = |s: &mut Stream<'_>| -> Result<u32, Error> {
        let id = if version == 0 {
            u32::from(s.u16().map_err(bad)?)
        } else {
            s.u32().map_err(bad)?
        };
        check_item_id(id, WHAT)
    };
    while s.has_bytes_left(1) {
        let (kind, _size) = s.box_header().map_err(bad)?;
        let from = read_id(&mut s)?;
        let item = meta.item_mut(from);
        if &kind == b"dimg" {
            // HEIF 6.6.1: at most one `dimg` from an item.
            if item.has_dimg_from {
                return Err(Error::Parse("AVIF item with two dimg boxes"));
            }
            item.has_dimg_from = true;
        }
        let count = s.u16().map_err(bad)?;
        for index in 0..count {
            let to = read_id(&mut s)?;
            match &kind {
                b"thmb" => meta.item_mut(from).thumbnail_for = to,
                b"auxl" => meta.item_mut(from).aux_for = to,
                b"prem" => meta.item_mut(from).prem_by = to,
                b"dimg" => {
                    // Derived images refer the other way: `to` is input
                    // `index` of `from`.
                    let input = meta.item_mut(to);
                    if input.dimg_for == from {
                        return Err(Error::Grid("an input named twice"));
                    }
                    if input.dimg_for != 0 {
                        return Err(Error::Unsupported("AVIF item input to two derived images"));
                    }
                    input.dimg_for = from;
                    input.dimg_idx = index;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// `avifParseGroupsListBox`. Like `iref`, each group box's size is checked to
/// fit and then not used.
fn parse_grpl(meta: &mut Meta<'_>, payload: &[u8]) -> Result<(), Error> {
    let bad = bad("AVIF grpl box");
    let mut s = Stream::new(payload);
    while s.has_bytes_left(1) {
        let (kind, _size) = s.box_header().map_err(bad)?;
        // The version and flags depend on the grouping type, and libavif
        // checks neither.
        s.version_and_flags().map_err(bad)?;
        s.u32().map_err(bad)?; // group_id
        let count = s.u32().map_err(bad)?;
        // Each entity is four bytes the box must hold; checking that first
        // keeps a hostile count from sizing the list.
        let bytes = usize::try_from(count)
            .ok()
            .and_then(|count| count.checked_mul(4))
            .ok_or(Error::Parse("AVIF grpl box"))?;
        let mut entities = Vec::new();
        if s.has_bytes_left(bytes) {
            entities.reserve_exact(bytes / 4);
        }
        for _ in 0..count {
            entities.push(s.u32().map_err(bad)?);
        }
        meta.groups.push(Group { kind, entities });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "tests build boxes by hand and fail loudly"
    )]

    use super::*;
    use alloc::vec;

    /// A box of type `kind` holding `payload`.
    fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(8 + payload.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    /// A full box: version and flags, then `payload`.
    fn full(kind: &[u8; 4], version: u8, flags: u32, payload: &[u8]) -> Vec<u8> {
        let mut body = vec![version];
        body.extend_from_slice(&flags.to_be_bytes()[1..]);
        body.extend_from_slice(payload);
        boxed(kind, &body)
    }

    fn hdlr() -> Vec<u8> {
        let mut body = vec![0; 4];
        body.extend_from_slice(b"pict");
        body.extend_from_slice(&[0; 12]);
        body.push(0);
        full(b"hdlr", 0, 0, &body)
    }

    fn meta_of(body: &[u8]) -> Result<Meta<'_>, Error> {
        let mut meta = Meta::default();
        parse_meta(&mut meta, body)?;
        Ok(meta)
    }

    fn ftyp(major: &[u8; 4], compatible: &[&[u8; 4]]) -> Vec<u8> {
        let mut body = major.to_vec();
        body.extend_from_slice(&[0; 4]);
        for brand in compatible {
            body.extend_from_slice(*brand);
        }
        boxed(b"ftyp", &body)
    }

    #[test]
    fn a_file_is_avif_by_its_first_box_brands() {
        assert!(sniff(&ftyp(b"avif", &[b"mif1"])));
        assert!(sniff(&ftyp(b"mif1", &[b"avis"])));
        assert!(!sniff(&ftyp(b"heic", &[b"mif1"])));
        // Not the first box; cut short; running to the end of the file.
        let mut late = boxed(b"free", &[]);
        late.extend(ftyp(b"avif", &[]));
        assert!(!sniff(&late));
        let whole = ftyp(b"avif", &[]);
        assert!(!sniff(&whole[..whole.len() - 1]));
        let mut open = whole.clone();
        open[..4].copy_from_slice(&[0; 4]);
        assert!(!sniff(&open));
        // Brands that do not come in fours.
        let mut ragged = whole;
        ragged[3] += 1;
        ragged.push(b'x');
        assert!(!sniff(&ragged));
    }

    #[test]
    fn a_meta_box_must_begin_with_a_pict_handler() {
        let mut body = vec![0; 4];
        body.extend(hdlr());
        let meta = meta_of(&body).unwrap();
        assert_eq!(meta.primary, 0);
        assert!(meta_of(&[0; 4]).is_err());
        let mut late = vec![0; 4];
        late.extend(full(b"pitm", 0, 0, &[0, 1]));
        late.extend(hdlr());
        assert!(meta_of(&late).is_err());
        let mut twice = body.clone();
        twice.extend(hdlr());
        assert!(meta_of(&twice).is_err());
        let mut pitm_twice = body;
        pitm_twice.extend(full(b"pitm", 0, 0, &[0, 1]));
        pitm_twice.extend(full(b"pitm", 0, 0, &[0, 2]));
        assert!(meta_of(&pitm_twice).is_err());
    }

    #[test]
    fn items_come_into_being_in_the_order_boxes_first_name_them() {
        let mut body = vec![0; 4];
        body.extend(hdlr());
        // iref first: item 5 refers to item 3 as its thumbnail.
        let mut iref = Vec::new();
        iref.extend(boxed(b"thmb", &[0, 5, 0, 1, 0, 3]));
        body.extend(full(b"iref", 0, 0, &iref));
        // iinf then names item 3 and item 9.
        let infe = |id: u8, kind: &[u8; 4]| {
            let mut e = vec![0, id, 0, 0];
            e.extend_from_slice(kind);
            e.push(0);
            full(b"infe", 2, 0, &e)
        };
        let mut iinf = vec![0, 2];
        iinf.extend(infe(3, b"av01"));
        iinf.extend(infe(9, b"Exif"));
        body.extend(full(b"iinf", 0, 0, &iinf));
        let meta = meta_of(&body).unwrap();
        let ids: Vec<u32> = meta.items.iter().map(|i| i.id).collect();
        // Item 5 was named by `iref`; item 3, the reference's target, only
        // came into being when `iinf` named it.
        assert_eq!(ids, [5, 3, 9]);
        assert_eq!(meta.item(5).unwrap().thumbnail_for, 3);
        assert_eq!(&meta.item(3).unwrap().kind, b"av01");
    }

    #[test]
    fn extents_add_up_and_empty_repeats_cost_nothing() {
        let mut body = vec![0; 4];
        body.extend(hdlr());
        // Version 1: sizes 4/4/4/0, one item of two extents, then an item of
        // 65535 empty extents that the file spends no bytes on.
        let mut iloc = vec![0x44, 0x40, 0, 1];
        iloc.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0, 0, 0x10, 0, 2]);
        iloc.extend_from_slice(&[0, 0, 0, 4, 0, 0, 0, 6, 0, 0, 0, 20, 0, 0, 0, 1]);
        let mut empty = vec![0x00, 0x00, 0, 1];
        empty.extend_from_slice(&[0, 2, 0, 0, 0xFF, 0xFF]);
        body.extend(full(b"iloc", 1, 0, &iloc));
        let meta = meta_of(&body).unwrap();
        let item = meta.item(1).unwrap();
        assert_eq!(item.size, 7);
        assert_eq!(
            item.extents,
            [
                Extent {
                    offset: 0x14,
                    size: 6
                },
                Extent {
                    offset: 0x24,
                    size: 1
                }
            ]
        );
        let mut body = vec![0; 4];
        body.extend(hdlr());
        body.extend(full(b"iloc", 0, 0, &empty));
        let meta = meta_of(&body).unwrap();
        let item = meta.item(2).unwrap();
        assert_eq!((item.size, item.extents.len()), (0, 1));
    }

    #[test]
    fn a_property_association_obeys_the_essential_rules() {
        let mut body = vec![0; 4];
        body.extend(hdlr());
        let mut ipco = Vec::new();
        ipco.extend(full(b"ispe", 0, 0, &[0, 0, 0, 64, 0, 0, 0, 32]));
        ipco.extend(boxed(b"irot", &[1]));
        ipco.extend(boxed(b"zzzz", &[]));
        let iprp_with = |assoc: &[u8]| {
            let mut ipma = vec![0, 0, 0, 1, 0, 1, assoc.len() as u8];
            ipma.extend_from_slice(assoc);
            let mut iprp = boxed(b"ipco", &ipco);
            iprp.extend(full(b"ipma", 0, 0, &ipma));
            let mut meta = body.clone();
            meta.extend(boxed(b"iprp", &iprp));
            meta
        };
        // ispe inessential, irot essential, the unknown box essential.
        let bytes = iprp_with(&[0x01, 0x82, 0x83]);
        let meta = meta_of(&bytes).unwrap();
        let item = meta.item(1).unwrap();
        assert_eq!(item.properties, [0, 1, 2]);
        assert!(item.unsupported_essential);
        assert_eq!(
            meta.find_property(item, b"ispe"),
            Some(&Property::Ispe {
                width: 64,
                height: 32
            })
        );
        // A transform must be essential; index 0 must not be; nor may an
        // index name a property that does not exist.
        assert!(meta_of(&iprp_with(&[0x02])).is_err());
        assert!(meta_of(&iprp_with(&[0x80])).is_err());
        assert!(meta_of(&iprp_with(&[0x04])).is_err());
    }

    #[test]
    fn a_dimg_reference_is_read_from_the_tile_s_side() {
        let mut body = vec![0; 4];
        body.extend(hdlr());
        let mut iref = Vec::new();
        iref.extend(boxed(b"dimg", &[0, 1, 0, 2, 0, 7, 0, 8]));
        body.extend(full(b"iref", 0, 0, &iref));
        let meta = meta_of(&body).unwrap();
        assert_eq!(meta.item(7).unwrap().dimg_for, 1);
        assert_eq!(meta.item(8).unwrap().dimg_idx, 1);
        // The same input twice is a broken grid; one input of two grids is
        // unsupported.
        let mut twice = vec![0; 4];
        twice.extend(hdlr());
        twice.extend(full(
            b"iref",
            0,
            0,
            &boxed(b"dimg", &[0, 1, 0, 2, 0, 7, 0, 7]),
        ));
        assert!(matches!(meta_of(&twice), Err(Error::Grid(_))));
        let mut shared = vec![0; 4];
        shared.extend(hdlr());
        let mut two = boxed(b"dimg", &[0, 1, 0, 1, 0, 7]);
        two.extend(boxed(b"dimg", &[0, 2, 0, 1, 0, 7]));
        shared.extend(full(b"iref", 0, 0, &two));
        assert!(matches!(meta_of(&shared), Err(Error::Unsupported(_))));
    }

    #[test]
    fn the_top_level_walk_stops_once_the_brands_are_satisfied() {
        let mut meta = vec![0; 4];
        meta.extend(hdlr());
        let mut file = ftyp(b"avif", &[b"mif1"]);
        file.extend(boxed(b"meta", &meta));
        // Anything after is never looked at.
        file.extend_from_slice(b"garbage");
        let parsed = parse(&file).unwrap();
        assert_eq!(&parsed.major_brand, b"avif");
        // A meta box cut short is truncated; a missing one is truncated too.
        let mut short = ftyp(b"avif", &[]);
        let whole = boxed(b"meta", &meta);
        short.extend_from_slice(&whole[..whole.len() - 1]);
        assert_eq!(parse(&short).unwrap_err(), Error::Truncated);
        assert_eq!(parse(&ftyp(b"avif", &[])).unwrap_err(), Error::Truncated);
        // A variable-length box before ftyp.
        let mut early = boxed(b"free", &[]);
        early.extend(ftyp(b"avif", &[]));
        assert!(matches!(parse(&early), Err(Error::Parse(_))));
        // An unknown box before ftyp is simply skipped.
        let mut skipped = boxed(b"wide", &[]);
        skipped.extend(file);
        assert!(parse(&skipped).is_ok());
    }
}
