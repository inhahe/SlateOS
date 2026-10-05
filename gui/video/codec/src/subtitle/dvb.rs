//! DVB subtitles (ETSI EN 300 743) -- the pictures digital television
//! carries -- read as the pictures they are.
//!
//! A track's block (`S_DVBSUB`) is a run of *segments*, each a sync byte
//! (0x0F), a type, the *page* it belongs to, a 16-bit length and the body:
//!
//! - A *page composition* lists the regions shown, where, and for how many
//!   seconds if nothing replaces them; its *state* says whether it begins
//!   afresh (an acquisition point or a mode change) or changes what is held.
//! - A *region composition* gives a region's size, depth (2, 4 or 8 bits a
//!   pixel), CLUT and background, and the objects placed in it. A region
//!   keeps its pixels from one display set to the next.
//! - A *CLUT definition* sets colours -- Y, Cr, Cb and transparency -- in
//!   the 2-, 4- and 8-bit tables of one CLUT.
//! - *Object data* is an object's pixels, run-length coded a field at a
//!   time, drawn when it comes into every region that places the object.
//! - A *display definition* gives the picture's size, and a window that
//!   moves the regions.
//! - The *end of display set* shows the page.
//!
//! The track's setup (its CodecPrivate) names the subtitle service, its
//! composition page and its ancillary page: only their segments are read.
//!
//! What is shown is a receiver's: EN 300 743's, as FFmpeg's `dvbsub`
//! decoder draws it -- read off its output one probe a rule, with its
//! `dvb_substream 0` and `compute_clut 0`, which make it read the service's
//! pages alone and give a region of no CLUT the standard's default one
//! (design-decisions §1365):
//!
//! - A page whose version is the one held changes nothing. One that begins
//!   afresh forgets the regions, the objects placed in them and the CLUTs
//!   (not the display definition). A page lists regions until one it has
//!   listed already; the first it lists is shown on top.
//! - A region's size changes its pixels' buffer, filled with its
//!   background, when the size makes a different count of pixels; a region
//!   asking for a fill is filled, its objects gone until their data comes
//!   again. Its objects must lie inside it, or the block is damaged.
//! - A CLUT starts as the standard's default and takes entries only in a
//!   new version. An entry's Y of 0 makes it transparent.
//! - Object data is drawn into every region placing the object, each field
//!   by its own lines -- a bottom field of no bytes the top's again --
//!   through the standard's map tables, or those the field gives, from a
//!   lower depth to the region's; data of a higher depth than its region,
//!   or placed past it, draws no further.
//! - A display set is shown at its end segment -- or at the end of a block
//!   that holds a page, a region and an object but no end -- for the page's
//!   timeout, or until the next. A block that cannot be read shows nothing
//!   new -- one of six bytes or fewer, or not starting with the sync byte,
//!   is such a block -- and what it changed before the damage stays
//!   changed.
//!
//! Where FFmpeg shows otherwise, the receiver wins (as for DVD and Blu-ray,
//! §1362, §1363): a CLUT entry marked for several tables goes into each
//! (FFmpeg: the first); a region is shown from its first composition, its
//! background before any object (FFmpeg: once an object is drawn); a
//! pixel of the non-modifying colour leaves its place as it was and the
//! next in theirs (FFmpeg draws the next one place to the left); and of
//! several display sets in one block the last shows (FFmpeg: the first that
//! shows anything).
//!
//! Two bounds are this reader's own, for files FFmpeg would read without
//! end: a page's regions hold four times FFmpeg's largest region in all,
//! and its regions place 1024 objects in all.

use super::CueImage;
use super::pgs::rgb;

/// Segment types.
const PAGE: u8 = 0x10;
const REGION: u8 = 0x11;
const CLUT: u8 = 0x12;
const OBJECT: u8 = 0x13;
const DISPLAY: u8 = 0x14;
const END: u8 = 0x80;

/// The display of a stream that defines none: standard definition.
const DEFAULT_DISPLAY: (u16, u16) = (720, 576);
/// A region's pixels at most: FFmpeg's (its "pixel buffer memory
/// constraint", a region of `w * h * 2 > 320 * 1024 * 8` refused).
const MAX_REGION: usize = 320 * 1024 * 8 / 2;
/// The pixels a page's regions hold in all: four of FFmpeg's largest.
const MAX_HELD: usize = 4 * MAX_REGION;
/// The objects a page's regions place in all.
const MAX_PLACED: usize = 1024;

/// The standard's map tables, from an object's depth to its region's.
const MAP_2_TO_4: [u8; 4] = [0x0, 0x7, 0x8, 0xF];
const MAP_2_TO_8: [u8; 4] = [0x00, 0x77, 0x88, 0xFF];
const MAP_4_TO_8: [u8; 16] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
];

/// What a block does to the screen.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Shown {
    /// These images from the block's time, in place of whatever was shown
    /// (none clears it), for `timeout` seconds unless replaced.
    Images { images: Vec<CueImage>, timeout: u8 },
    /// Nothing changes: no display set ends in the block, or it is damaged.
    Unchanged,
}

/// One CLUT: its version and its three tables, as RGBA.
#[derive(Clone, Debug)]
struct Clut {
    id: u8,
    version: Option<u8>,
    two: [[u8; 4]; 4],
    four: [[u8; 4]; 16],
    eight: Box<[[u8; 4]; 256]>,
}

impl Clut {
    /// The standard's default CLUT (EN 300 743 section 10).
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "sums of the standard's levels, each at most 255"
    )]
    fn standard(id: u8) -> Self {
        let level = |on: bool, full: u8| if on { full } else { 0 };
        let mut four = [[0u8; 4]; 16];
        for (i, entry) in (0u8..).zip(four.iter_mut()).skip(1) {
            let full = if i < 8 { 255 } else { 127 };
            *entry = [
                level(i & 1 != 0, full),
                level(i & 2 != 0, full),
                level(i & 4 != 0, full),
                255,
            ];
        }
        let mut eight = Box::new([[0u8; 4]; 256]);
        for (i, entry) in (0u8..=255).zip(eight.iter_mut()).skip(1) {
            let bit = |n: u8| i & (1 << n) != 0;
            *entry = if i < 8 {
                [
                    level(bit(0), 255),
                    level(bit(1), 255),
                    level(bit(2), 255),
                    63,
                ]
            } else {
                // Bits 0-2 and 4-6 give each channel two steps; bits 3 and 7
                // choose the cluster.
                let channel = |low: u8, high: u8, base: u8| {
                    let (a, b) = match i & 0x88 {
                        0x00 | 0x08 => (85, 170),
                        _ => (43, 85),
                    };
                    base + level(bit(low), a) + level(bit(high), b)
                };
                let base = if i & 0x88 == 0x80 { 127 } else { 0 };
                let alpha = if i & 0x88 == 0x08 { 127 } else { 255 };
                [
                    channel(0, 4, base),
                    channel(1, 5, base),
                    channel(2, 6, base),
                    alpha,
                ]
            };
        }
        Self {
            id,
            version: None,
            two: [
                [0, 0, 0, 0],
                [255, 255, 255, 255],
                [0, 0, 0, 255],
                [127, 127, 127, 255],
            ],
            four,
            eight,
        }
    }

    /// The table a region of `depth` bits a pixel shows its pixels through.
    fn table(&self, depth: u8) -> &[[u8; 4]] {
        match depth {
            2 => &self.two,
            4 => &self.four,
            _ => &self.eight[..],
        }
    }
}

/// One region: its size, depth and CLUT, and its pixels -- each an entry of
/// the CLUT's table for its depth.
#[derive(Debug)]
struct Region {
    id: u8,
    width: u16,
    height: u16,
    depth: u8,
    clut: u8,
    pixels: Vec<u8>,
}

/// An object placed in a region.
#[derive(Clone, Copy, Debug)]
struct Placement {
    object: u16,
    region: u8,
    x: u16,
    y: u16,
}

/// The display: its size, and where its window puts the regions.
#[derive(Clone, Copy, Debug)]
struct Display {
    version: Option<u8>,
    width: u32,
    height: u32,
    x: u16,
    y: u16,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            version: None,
            width: u32::from(DEFAULT_DISPLAY.0),
            height: u32::from(DEFAULT_DISPLAY.1),
            x: 0,
            y: 0,
        }
    }
}

/// A block's bytes, read past their end as FFmpeg reads its padded
/// packets: zeros.
#[derive(Clone, Copy)]
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn at(self, i: usize) -> u8 {
        self.0.get(i).copied().unwrap_or(0)
    }

    fn be16(self, i: usize) -> u16 {
        u16::from_be_bytes([self.at(i), self.at(i.saturating_add(1))])
    }
}

/// A segment the block could not be read past, or a display set FFmpeg
/// refuses: the block shows nothing new.
struct Damage;

/// A track's DVB subtitles being read.
#[derive(Debug, Default)]
pub(crate) struct Decoder {
    /// The service's composition and ancillary pages; `None`, every page.
    pages: Option<(u16, u16)>,
    display: Display,
    /// The page held: its version, timeout and regions (id, x, y) in its
    /// order.
    page_version: Option<u8>,
    timeout: u8,
    shown: Vec<(u8, u16, u16)>,
    regions: Vec<Region>,
    /// The objects known: those a region composition has named, kept until
    /// the last region placing one stops placing it.
    objects: Vec<u16>,
    placements: Vec<Placement>,
    cluts: Vec<Clut>,
    /// Whether the picture's size is settled, as FFmpeg's decoder's is: by
    /// the first display definition, or by the first display set of a page,
    /// a region and an object without one (standard definition). A first
    /// display definition too large for FFmpeg's pictures is damage.
    sized: bool,
    damaged: u64,
}

impl Decoder {
    /// A decoder for a track whose setup is `config`: its first five bytes
    /// name the service's pages -- a setup of four bytes, or of whole fives,
    /// as FFmpeg reads one; another reads every page.
    pub(crate) fn new(config: &[u8]) -> Self {
        let b = Bytes(config);
        let named = config.len() >= 4 && (config.len().is_multiple_of(5) || config.len() == 4);
        Self {
            pages: named.then(|| (b.be16(0), b.be16(2))),
            ..Self::default()
        }
    }

    /// Forget everything read, as at the track's start: a seek. (The
    /// picture's size stays settled, as FFmpeg's decoder's does.)
    pub(crate) fn reset(&mut self) {
        *self = Self {
            pages: self.pages,
            sized: self.sized,
            damaged: self.damaged,
            ..Self::default()
        };
    }

    /// The blocks met damaged.
    pub(crate) fn damaged(&self) -> u64 {
        self.damaged
    }

    /// Read a block's segments: what it shows.
    pub(crate) fn block(&mut self, data: &[u8]) -> Shown {
        match self.segments(Bytes(data), data.len()) {
            Ok(Some(shown)) => shown,
            Ok(None) => Shown::Unchanged,
            Err(Damage) => {
                self.damaged = self.damaged.saturating_add(1);
                Shown::Unchanged
            }
        }
    }

    /// The segments, read in turn as FFmpeg reads them: a block of six bytes
    /// or fewer, or not starting with the sync byte, is damage; reading stops
    /// where a segment does not start with it or fewer than six bytes are
    /// left. A display set shown at each end segment, the last kept.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within the block, each checked against its length"
    )]
    fn segments(&mut self, data: Bytes, len: usize) -> Result<Option<Shown>, Damage> {
        if len <= 6 || data.at(0) != 0x0F {
            return Err(Damage);
        }
        let (mut at, mut shown) = (0usize, None);
        let (mut page, mut region, mut object, mut display, mut end) =
            (false, false, false, false, false);
        while len - at >= 6 && data.at(at) == 0x0F {
            let kind = data.at(at + 1);
            let id = data.be16(at + 2);
            let size = usize::from(data.be16(at + 4));
            at += 6;
            if len - at < size {
                return Err(Damage);
            }
            if self.pages.is_none_or(|(c, a)| id == c || id == a) {
                match kind {
                    PAGE => {
                        self.page(data, at, size)?;
                        page = true;
                    }
                    REGION => {
                        self.region(data, at, size)?;
                        region = true;
                    }
                    CLUT => self.clut(data, at, size),
                    OBJECT => {
                        self.object(data, at, size)?;
                        object = true;
                    }
                    DISPLAY => {
                        self.display(data, at, size)?;
                        display = true;
                    }
                    END => {
                        shown = Some(self.show());
                        end = true;
                    }
                    _ => {}
                }
            }
            at += size;
        }
        // A block of a page, a region and an object settles the picture's
        // size where no display definition has, and is shown though it has
        // no end segment.
        if page && region && object {
            if !display {
                self.sized = true;
            }
            if !end {
                shown = Some(self.show());
            }
        }
        Ok(shown)
    }

    /// A page composition.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within the segment"
    )]
    fn page(&mut self, data: Bytes, at: usize, size: usize) -> Result<(), Damage> {
        if size < 1 {
            return Err(Damage);
        }
        let timeout = data.at(at);
        let version = data.at(at + 1) >> 4;
        let state = (data.at(at + 1) >> 2) & 3;
        if self.page_version == Some(version) {
            return Ok(());
        }
        self.page_version = Some(version);
        self.timeout = timeout;
        if state == 1 || state == 2 {
            self.regions.clear();
            self.objects.clear();
            self.placements.clear();
            self.cluts.clear();
        }
        self.shown.clear();
        let mut p = at + 2;
        while p + 5 < at + size {
            let id = data.at(p);
            if self.shown.iter().any(|&(r, _, _)| r == id) {
                break;
            }
            self.shown.push((id, data.be16(p + 2), data.be16(p + 4)));
            p += 6;
        }
        Ok(())
    }

    /// A region composition.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within the segment; pixel counts bounded by MAX_REGION"
    )]
    fn region(&mut self, data: Bytes, at: usize, size: usize) -> Result<(), Damage> {
        if size < 10 {
            return Err(Damage);
        }
        let id = data.at(at);
        let mut fill = (data.at(at + 1) >> 3) & 1 == 1;
        let width = data.be16(at + 2);
        let height = data.be16(at + 4);
        let index = match self.regions.iter().position(|r| r.id == id) {
            Some(i) => i,
            None => {
                self.regions.push(Region {
                    id,
                    width: 0,
                    height: 0,
                    depth: 4,
                    clut: 0,
                    pixels: Vec::new(),
                });
                self.regions.len() - 1
            }
        };
        let count = usize::from(width) * usize::from(height);
        let held: usize = self.regions.iter().map(|r| r.pixels.len()).sum();
        let Some(region) = self.regions.get_mut(index) else {
            return Err(Damage);
        };
        if count == 0 || count > MAX_REGION || held - region.pixels.len() + count > MAX_HELD {
            region.width = 0;
            region.height = 0;
            return Err(Damage);
        }
        region.width = width;
        region.height = height;
        if count != region.pixels.len() {
            region.pixels = vec![0; count];
            fill = true;
        }
        // 2, 4 or 8 bits; another depth is read as 4.
        region.depth = match (data.at(at + 6) >> 2) & 7 {
            1 => 2,
            3 => 8,
            _ => 4,
        };
        region.clut = data.at(at + 7);
        let background = match region.depth {
            8 => data.at(at + 8),
            4 => data.at(at + 9) >> 4,
            _ => (data.at(at + 9) >> 2) & 3,
        };
        if fill {
            region.pixels.fill(background);
        }
        // The objects it places, in place of those it placed: an object no
        // region places any longer is no longer known.
        let unplaced: Vec<u16> = self
            .placements
            .iter()
            .filter(|p| p.region == id)
            .map(|p| p.object)
            .collect();
        self.placements.retain(|p| p.region != id);
        for object in unplaced {
            if !self.placements.iter().any(|p| p.object == object) {
                self.objects.retain(|&o| o != object);
            }
        }
        let mut p = at + 10;
        while p + 5 < at + size {
            let object = data.be16(p);
            let kind = data.at(p + 2) >> 6;
            let x = data.be16(p + 2) & 0xFFF;
            let y = data.be16(p + 4) & 0xFFF;
            p += 6;
            // Known from here on, even placed outside the region.
            if !self.objects.contains(&object) {
                self.objects.push(object);
            }
            if x >= width || y >= height || self.placements.len() >= MAX_PLACED {
                return Err(Damage);
            }
            if (kind == 1 || kind == 2) && p + 1 < at + size {
                // A character's foreground and background codes.
                p += 2;
            }
            self.placements.push(Placement {
                object,
                region: id,
                x,
                y,
            });
        }
        Ok(())
    }

    /// A CLUT definition: entries taken only in a new version.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within the segment"
    )]
    fn clut(&mut self, data: Bytes, at: usize, size: usize) {
        let id = data.at(at);
        let version = data.at(at + 1) >> 4;
        let index = match self.cluts.iter().position(|c| c.id == id) {
            Some(i) => i,
            None => {
                self.cluts.push(Clut::standard(id));
                self.cluts.len() - 1
            }
        };
        let Some(clut) = self.cluts.get_mut(index) else {
            return;
        };
        if clut.version == Some(version) {
            return;
        }
        clut.version = Some(version);
        let end = at + size;
        let mut p = at + 2;
        while p + 4 < end {
            let entry = data.at(p);
            let flags = data.at(p + 1);
            let (y, cr, cb, t) = if flags & 1 == 1 {
                p += 6;
                (
                    data.at(p - 4),
                    data.at(p - 3),
                    data.at(p - 2),
                    data.at(p - 1),
                )
            } else {
                // Y in six bits, Cr and Cb in four, T in two.
                let (a, b) = (data.at(p + 2), data.at(p + 3));
                p += 4;
                (
                    a & 0xFC,
                    (((a & 3) << 2) | (b >> 6)) << 4,
                    (b << 2) & 0xF0,
                    (b << 6) & 0xC0,
                )
            };
            let alpha = if y == 0 { 0 } else { 255 - t };
            let [r, g, bl] = rgb(y, cr, cb, true);
            let colour = [r, g, bl, alpha];
            let e = usize::from(entry);
            if flags & 0x80 != 0
                && let Some(slot) = clut.two.get_mut(e)
            {
                *slot = colour;
            }
            if flags & 0x40 != 0
                && let Some(slot) = clut.four.get_mut(e)
            {
                *slot = colour;
            }
            if flags & 0x20 != 0
                && let Some(slot) = clut.eight.get_mut(e)
            {
                *slot = colour;
            }
        }
    }

    /// Object data: drawn into every region placing the object, the latest
    /// placement first.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within the segment"
    )]
    fn object(&mut self, data: Bytes, at: usize, size: usize) -> Result<(), Damage> {
        let id = data.be16(at);
        if !self.objects.contains(&id) {
            return Err(Damage);
        }
        let flags = data.at(at + 2);
        let coding = (flags >> 2) & 3;
        let non_mod = (flags >> 1) & 1 == 1;
        if coding != 0 {
            // Characters (coding 1) are not drawn; codings 2 and 3 do not
            // exist.
            return Err(Damage);
        }
        let top = usize::from(data.be16(at + 3));
        let bottom = usize::from(data.be16(at + 5));
        let first = at + 7;
        if first + top + bottom > at + size {
            return Err(Damage);
        }
        let placed: Vec<Placement> = self
            .placements
            .iter()
            .rev()
            .filter(|p| p.object == id)
            .copied()
            .collect();
        for p in placed {
            self.field(data, first, top, p, 0, non_mod);
            let (start, len) = if bottom > 0 {
                (first + top, bottom)
            } else {
                (first, top)
            };
            self.field(data, start, len, p, 1, non_mod);
        }
        Ok(())
    }

    /// One field of an object, drawn at a placement: its lines are every
    /// other row from `field` (0 the top, 1 the bottom).
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within the block and the region, each checked"
    )]
    fn field(
        &mut self,
        data: Bytes,
        start: usize,
        len: usize,
        p: Placement,
        field: usize,
        non_mod: bool,
    ) {
        let Some(region) = self.regions.iter_mut().find(|r| r.id == p.region) else {
            return;
        };
        let (width, height) = (usize::from(region.width), usize::from(region.height));
        let (mut x, mut y) = (usize::from(p.x), usize::from(p.y) + field);
        let (mut map24, mut map28, mut map48) = (MAP_2_TO_4, MAP_2_TO_8, MAP_4_TO_8);
        let mut at = start;
        let end = start + len;
        while at < end {
            let code = data.at(at);
            if (code != 0xF0 && x >= width) || y >= height {
                return;
            }
            at += 1;
            let row_start = y * width;
            let Some(row) = region.pixels.get_mut(row_start..row_start + width) else {
                return;
            };
            match code {
                0x10 => {
                    let map = match region.depth {
                        8 => Some(&map28[..]),
                        4 => Some(&map24[..]),
                        _ => None,
                    };
                    x = two_bit(data, &mut at, end, row, x, non_mod, map);
                }
                0x11 => {
                    if region.depth < 4 {
                        return;
                    }
                    let map = (region.depth == 8).then_some(&map48[..]);
                    x = four_bit(data, &mut at, end, row, x, non_mod, map);
                }
                0x12 => {
                    if region.depth < 8 {
                        return;
                    }
                    x = eight_bit(data, &mut at, end, row, x, non_mod);
                }
                0x20 => {
                    let (a, b) = (data.at(at), data.at(at + 1));
                    map24 = [a >> 4, a & 0xF, b >> 4, b & 0xF];
                    at += 2;
                }
                0x21 => {
                    for (i, m) in map28.iter_mut().enumerate() {
                        *m = data.at(at + i);
                    }
                    at += 4;
                }
                0x22 => {
                    for (i, m) in map48.iter_mut().enumerate() {
                        *m = data.at(at + i);
                    }
                    at += 16;
                }
                0xF0 => {
                    x = usize::from(p.x);
                    y += 2;
                }
                _ => {}
            }
        }
    }

    /// A display definition: taken only in a new version.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "positions within the segment"
    )]
    fn display(&mut self, data: Bytes, at: usize, size: usize) -> Result<(), Damage> {
        if size < 5 {
            return Err(Damage);
        }
        let info = data.at(at);
        let version = info >> 4;
        if self.display.version == Some(version) {
            return Ok(());
        }
        let (width, height) = (
            u32::from(data.be16(at + 1)) + 1,
            u32::from(data.be16(at + 3)) + 1,
        );
        self.display = Display {
            version: Some(version),
            width,
            height,
            x: 0,
            y: 0,
        };
        if !self.sized {
            // The first definition settles the picture's size -- unless it
            // is larger than FFmpeg takes a picture to be.
            // FFmpeg's image check: a line of 8 bytes a pixel and 128 more,
            // for the height and 128 more lines, under INT_MAX bytes.
            if u64::from(width + 128) * 8 * u64::from(height + 128)
                >= u64::from(i32::MAX.unsigned_abs())
            {
                return Err(Damage);
            }
            self.sized = true;
        }
        if info & 0x08 != 0 {
            if size < 13 {
                return Err(Damage);
            }
            self.display.x = data.be16(at + 5);
            self.display.y = data.be16(at + 9);
        }
        Ok(())
    }

    /// The page as it shows now: its regions, the first it lists on top
    /// (last in the list), each through its CLUT -- the standard's default
    /// where it names one not defined.
    fn show(&self) -> Shown {
        let standard = Clut::standard(0);
        let mut images = Vec::new();
        for &(id, x, y) in self.shown.iter().rev() {
            let Some(region) = self.regions.iter().find(|r| r.id == id) else {
                continue;
            };
            if region.width == 0 || region.height == 0 {
                continue;
            }
            let clut = self
                .cluts
                .iter()
                .find(|c| c.id == region.clut)
                .unwrap_or(&standard);
            let table = clut.table(region.depth);
            let mut rgba = Vec::with_capacity(region.pixels.len().saturating_mul(4));
            for &v in &region.pixels {
                rgba.extend_from_slice(table.get(usize::from(v)).unwrap_or(&[0; 4]));
            }
            if rgba.chunks_exact(4).all(|p| p.get(3) == Some(&0)) {
                continue;
            }
            images.push(CueImage {
                canvas_width: self.display.width,
                canvas_height: self.display.height,
                x: u32::from(x).saturating_add(u32::from(self.display.x)),
                y: u32::from(y).saturating_add(u32::from(self.display.y)),
                width: u32::from(region.width),
                height: u32::from(region.height),
                rgba,
                forced: false,
            });
        }
        Shown::Images {
            images,
            timeout: self.timeout,
        }
    }
}

/// Whether `block` begins afresh for the service `pages`: a page
/// composition of an acquisition point or a mode change, from which what
/// is shown can be known with nothing before it.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "positions within the block, each checked against its length"
)]
pub(crate) fn begins_epoch(block: &[u8], pages: Option<(u16, u16)>) -> bool {
    let data = Bytes(block);
    let mut at = 0usize;
    while block.len() - at >= 6 && data.at(at) == 0x0F {
        let kind = data.at(at + 1);
        let page = data.be16(at + 2);
        let size = usize::from(data.be16(at + 4));
        at += 6;
        if block.len() - at < size {
            return false;
        }
        if kind == PAGE && size >= 1 && pages.is_none_or(|(c, a)| page == c || page == a) {
            return matches!((data.at(at + 1) >> 2) & 3, 1 | 2);
        }
        at += size;
    }
    false
}

impl Decoder {
    /// The service's pages, for [`begins_epoch`].
    pub(crate) fn pages(&self) -> Option<(u16, u16)> {
        self.pages
    }
}

/// FFmpeg's bit reader over a field from `start`, `size` bytes: it reads
/// on past the end into what follows, its count stopping 8 bits past it.
struct Bits<'a> {
    data: Bytes<'a>,
    start: usize,
    index: usize,
    size: usize,
}

impl Bits<'_> {
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "bit positions within the block"
    )]
    fn read(&mut self, n: usize) -> u8 {
        let mut v = 0u8;
        for k in 0..n {
            let bit = self.index + k;
            let byte = self.data.at(self.start + bit / 8);
            v = (v << 1) | ((byte >> (7 - bit % 8)) & 1);
        }
        self.index = (self.index + n).min(self.size * 8 + 8);
        v
    }

    fn left(&self) -> bool {
        self.index < self.size.saturating_mul(8)
    }

    /// The bytes read, a part-read byte counted whole.
    fn bytes(&self) -> usize {
        self.index.div_ceil(8)
    }
}

/// Where a string of pixel codes draws: a region's row from `x`.
struct Draw<'r> {
    row: &'r mut [u8],
    x: usize,
    non_mod: bool,
}

impl Draw<'_> {
    /// `n` pixels of `code`, each `value` -- or, `code` being the
    /// non-modifying colour 1, left as they are -- no further than the row.
    #[allow(clippy::arithmetic_side_effects, reason = "a count of pixels")]
    fn run(&mut self, code: u8, value: u8, n: usize) {
        if self.non_mod && code == 1 {
            self.x += n;
            return;
        }
        for _ in 0..n {
            let Some(p) = self.row.get_mut(self.x) else {
                break;
            };
            *p = value;
            self.x += 1;
        }
    }
}

/// A 2-bit/pixel code string at `*at` (EN 300 743 section 7.2.5.2), drawn
/// from `x`: where the row has got to. As FFmpeg reads it: to the row's end
/// or the field's, then the six bits an end code would take, `*at` moving
/// on by whole bytes.
fn two_bit(
    data: Bytes,
    at: &mut usize,
    end: usize,
    row: &mut [u8],
    x: usize,
    non_mod: bool,
    map: Option<&[u8]>,
) -> usize {
    let width = row.len();
    let mut d = Draw { row, x, non_mod };
    let mut bits = Bits {
        data,
        start: *at,
        index: 0,
        size: end.saturating_sub(*at),
    };
    let value = |c: u8| map.map_or(c, |m| m.get(usize::from(c)).copied().unwrap_or(0));
    while bits.left() && d.x < width {
        let c = bits.read(2);
        if c != 0 {
            d.run(c, value(c), 1);
        } else if bits.read(1) == 1 {
            let n = usize::from(bits.read(3)).saturating_add(3);
            let c = bits.read(2);
            d.run(c, value(c), n);
        } else if bits.read(1) == 0 {
            match bits.read(2) {
                2 => {
                    let n = usize::from(bits.read(4)).saturating_add(12);
                    let c = bits.read(2);
                    d.run(c, value(c), n);
                }
                3 => {
                    let n = usize::from(bits.read(8)).saturating_add(29);
                    let c = bits.read(2);
                    d.run(c, value(c), n);
                }
                1 => d.run(0, value(0), 2),
                _ => {
                    *at = at.saturating_add(bits.bytes());
                    return d.x;
                }
            }
        } else {
            d.run(0, value(0), 1);
        }
    }
    bits.read(6);
    *at = at.saturating_add(bits.bytes());
    d.x
}

/// A 4-bit/pixel code string, as [`two_bit`] reads one: the eight bits of
/// an end code read after it.
fn four_bit(
    data: Bytes,
    at: &mut usize,
    end: usize,
    row: &mut [u8],
    x: usize,
    non_mod: bool,
    map: Option<&[u8]>,
) -> usize {
    let width = row.len();
    let mut d = Draw { row, x, non_mod };
    let mut bits = Bits {
        data,
        start: *at,
        index: 0,
        size: end.saturating_sub(*at),
    };
    let value = |c: u8| map.map_or(c, |m| m.get(usize::from(c)).copied().unwrap_or(0));
    while bits.left() && d.x < width {
        let c = bits.read(4);
        if c != 0 {
            d.run(c, value(c), 1);
        } else if bits.read(1) == 0 {
            let n = bits.read(3);
            if n == 0 {
                *at = at.saturating_add(bits.bytes());
                return d.x;
            }
            d.run(0, value(0), usize::from(n).saturating_add(2));
        } else if let 0 = bits.read(1) {
            let n = usize::from(bits.read(2)).saturating_add(4);
            let c = bits.read(4);
            d.run(c, value(c), n);
        } else {
            match bits.read(2) {
                2 => {
                    let n = usize::from(bits.read(4)).saturating_add(9);
                    let c = bits.read(4);
                    d.run(c, value(c), n);
                }
                3 => {
                    let n = usize::from(bits.read(8)).saturating_add(25);
                    let c = bits.read(4);
                    d.run(c, value(c), n);
                }
                1 => d.run(0, value(0), 2),
                _ => d.run(0, value(0), 1),
            }
        }
    }
    bits.read(8);
    *at = at.saturating_add(bits.bytes());
    d.x
}

/// An 8-bit/pixel code string, a byte at a time, as FFmpeg reads one:
/// within the field, to the row's end or the field's, then the end code
/// that should follow.
fn eight_bit(
    data: Bytes,
    at: &mut usize,
    end: usize,
    row: &mut [u8],
    x: usize,
    non_mod: bool,
) -> usize {
    let width = row.len();
    let mut d = Draw { row, x, non_mod };
    // A byte of the field; past its end, 0 and no further.
    let next = |p: &mut usize| {
        if *p < end {
            let b = data.at(*p);
            *p = p.saturating_add(1);
            b
        } else {
            0
        }
    };
    let mut p = *at;
    while d.x < width && p < end {
        let c = next(&mut p);
        if c != 0 {
            d.run(c, c, 1);
            continue;
        }
        let b = next(&mut p);
        let n = usize::from(b & 0x7F);
        let c = if b & 0x80 == 0 {
            if n == 0 {
                *at = p;
                return d.x;
            }
            0
        } else {
            next(&mut p)
        };
        d.run(c, c, n);
    }
    // The first byte of the end code that should follow, and its second
    // where it is there (FFmpeg's allowance for its own encoder, which
    // once wrote one).
    next(&mut p);
    if p < end && data.at(p) == 0 {
        p = p.saturating_add(1);
    }
    *at = p;
    d.x
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::panic,
        reason = "a test: a failure should be loud, and its numbers are small"
    )]

    use super::*;

    fn seg(kind: u8, page: u16, body: &[u8]) -> Vec<u8> {
        let mut v = vec![0x0F, kind];
        v.extend_from_slice(&page.to_be_bytes());
        v.extend_from_slice(&u16::try_from(body.len()).unwrap().to_be_bytes());
        v.extend_from_slice(body);
        v
    }

    /// A page of `regions` (id, x, y).
    fn pcs(version: u8, state: u8, timeout: u8, regions: &[(u8, u16, u16)]) -> Vec<u8> {
        let mut b = vec![timeout, (version << 4) | (state << 2) | 3];
        for &(id, x, y) in regions {
            b.extend_from_slice(&[id, 0xFF]);
            b.extend_from_slice(&x.to_be_bytes());
            b.extend_from_slice(&y.to_be_bytes());
        }
        seg(PAGE, 1, &b)
    }

    /// A region of `depth` (1 2-bit, 2 4-bit, 3 8-bit), filled with `fill`
    /// when given, placing `objects` (id, x, y).
    fn rcs(
        id: u8,
        w: u16,
        h: u16,
        depth: u8,
        fill: Option<u8>,
        objects: &[(u16, u16, u16)],
    ) -> Vec<u8> {
        let mut b = vec![id, (u8::from(fill.is_some()) << 3) | 7];
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        let f = fill.unwrap_or(0);
        b.extend_from_slice(&[
            (depth << 5) | (depth << 2) | 3,
            1,
            f,
            (f << 4) | ((f & 3) << 2) | 3,
        ]);
        for &(o, x, y) in objects {
            b.extend_from_slice(&o.to_be_bytes());
            b.extend_from_slice(&x.to_be_bytes());
            b.extend_from_slice(&(0xF000 | y).to_be_bytes());
        }
        seg(REGION, 1, &b)
    }

    /// A CLUT's entries: (entry, flags, Y, Cr, Cb, T), full range.
    fn cds(version: u8, entries: &[(u8, u8, u8, u8, u8, u8)]) -> Vec<u8> {
        let mut b = vec![1, (version << 4) | 0xF];
        for &(e, f, y, cr, cb, t) in entries {
            b.extend_from_slice(&[e, f | 1, y, cr, cb, t]);
        }
        seg(CLUT, 1, &b)
    }

    /// An object of 4-bit lines, each a run per value in `rows`, the top
    /// field the even rows and the bottom the odd.
    fn ods(id: u16, flags: u8, rows: &[&[u8]]) -> Vec<u8> {
        let field = |lines: Vec<&&[u8]>| {
            let mut out = Vec::new();
            for line in lines {
                out.push(0x11);
                let mut nibbles: Vec<u8> = Vec::new();
                for &v in *line {
                    assert!(v != 0, "a test's pixels are not 0");
                    nibbles.push(v);
                }
                nibbles.extend_from_slice(&[0, 0]);
                if nibbles.len() % 2 == 1 {
                    nibbles.push(0);
                }
                out.extend(nibbles.chunks(2).map(|c| (c[0] << 4) | c[1]));
                out.push(0xF0);
            }
            out
        };
        let top = field(rows.iter().step_by(2).collect());
        let bottom = field(rows.iter().skip(1).step_by(2).collect());
        let mut b = id.to_be_bytes().to_vec();
        b.push(flags | 1);
        b.extend_from_slice(&u16::try_from(top.len()).unwrap().to_be_bytes());
        b.extend_from_slice(&u16::try_from(bottom.len()).unwrap().to_be_bytes());
        b.extend(top);
        b.extend(bottom);
        seg(OBJECT, 1, &b)
    }

    fn end() -> Vec<u8> {
        seg(END, 1, &[])
    }

    const WHITE: (u8, u8, u8, u8, u8, u8) = (1, 0x40, 235, 128, 128, 0);
    const RED: (u8, u8, u8, u8, u8, u8) = (2, 0x40, 81, 240, 90, 0);

    fn images(shown: Shown) -> Vec<CueImage> {
        match shown {
            Shown::Images { images, .. } => images,
            Shown::Unchanged => panic!("nothing shown"),
        }
    }

    #[test]
    fn a_display_set_shows_its_regions_through_their_clut() {
        let mut d = Decoder::new(&[0, 1, 0, 1, 0x10]);
        let block = [
            pcs(0, 2, 5, &[(1, 100, 50)]),
            rcs(1, 3, 2, 2, None, &[(1, 0, 0)]),
            cds(0, &[WHITE, RED]),
            ods(1, 0, &[&[1, 2, 1], &[2, 2, 2]]),
            end(),
        ]
        .concat();
        let shown = d.block(&block);
        let Shown::Images { images, timeout } = shown else {
            panic!("nothing shown");
        };
        assert_eq!(timeout, 5);
        assert_eq!(images.len(), 1);
        let i = &images[0];
        assert_eq!(
            (i.canvas_width, i.canvas_height, i.x, i.y, i.width, i.height),
            (720, 576, 100, 50, 3, 2)
        );
        let w = [255, 255, 255, 255];
        let r = [254, 0, 0, 255];
        assert_eq!(i.rgba, [w, r, w, r, r, r].concat());
    }

    #[test]
    fn the_standard_default_cluts() {
        let c = Clut::standard(0);
        assert_eq!(
            c.two,
            [
                [0, 0, 0, 0],
                [255, 255, 255, 255],
                [0, 0, 0, 255],
                [127, 127, 127, 255]
            ]
        );
        assert_eq!(c.four[3], [255, 255, 0, 255]);
        assert_eq!(c.four[8], [0, 0, 0, 255]);
        assert_eq!(c.four[13], [127, 0, 127, 255]);
        // As FFmpeg's, under compute_clut 0, over all 256.
        assert_eq!(c.eight[1], [255, 0, 0, 63]);
        assert_eq!(c.eight[8], [0, 0, 0, 127]);
        assert_eq!(c.eight[0x11], [255, 0, 0, 255]);
        assert_eq!(c.eight[0x19], [255, 0, 0, 127]);
        assert_eq!(c.eight[0x81], [170, 127, 127, 255]);
        assert_eq!(c.eight[0x90], [212, 127, 127, 255]);
        assert_eq!(c.eight[0x89], [43, 0, 0, 255]);
        assert_eq!(c.eight[0xFF], [128, 128, 128, 255]);
    }

    #[test]
    fn a_page_of_its_own_version_changes_nothing_and_afresh_forgets() {
        let mut d = Decoder::new(&[]);
        d.block(
            &[
                pcs(3, 2, 9, &[(1, 0, 0)]),
                rcs(1, 2, 2, 2, Some(1), &[]),
                cds(0, &[WHITE]),
                end(),
            ]
            .concat(),
        );
        assert_eq!(d.shown, [(1, 0, 0)]);
        // The same version, listing nothing: ignored.
        let again = images(d.block(&[pcs(3, 0, 9, &[]), end()].concat()));
        assert_eq!(again.len(), 1, "the region still listed");
        // An acquisition point forgets the regions and CLUTs.
        d.block(&[pcs(4, 1, 9, &[(1, 0, 0)]), end()].concat());
        assert!(d.regions.is_empty() && d.cluts.is_empty());
    }

    #[test]
    fn the_first_region_listed_is_on_top_and_a_repeat_ends_the_list() {
        let mut d = Decoder::new(&[]);
        let set = [
            pcs(0, 2, 9, &[(1, 0, 0), (2, 5, 5), (1, 9, 9), (3, 0, 0)]),
            rcs(1, 2, 2, 2, Some(1), &[]),
            rcs(2, 2, 2, 2, Some(2), &[]),
            rcs(3, 2, 2, 2, Some(2), &[]),
            cds(0, &[WHITE, RED]),
            end(),
        ]
        .concat();
        let shown = images(d.block(&set));
        // Region 2 drawn first, region 1 last; region 3 never listed.
        let places: Vec<(u32, u32)> = shown.iter().map(|i| (i.x, i.y)).collect();
        assert_eq!(places, [(5, 5), (0, 0)]);
    }

    #[test]
    fn an_object_is_drawn_where_its_region_places_it_when_its_data_comes() {
        let mut d = Decoder::new(&[]);
        d.block(
            &[
                pcs(0, 2, 9, &[(1, 0, 0)]),
                rcs(1, 4, 2, 2, Some(1), &[(7, 1, 0)]),
                cds(0, &[WHITE, RED]),
                end(),
            ]
            .concat(),
        );
        let set = [
            pcs(1, 0, 9, &[(1, 0, 0)]),
            ods(7, 0, &[&[2, 2], &[2, 2]]),
            end(),
        ]
        .concat();
        let shown = images(d.block(&set));
        let (w, r) = ([255, 255, 255, 255], [254, 0, 0, 255]);
        assert_eq!(shown[0].rgba, [w, r, r, w, w, r, r, w].concat());
        // Data for an object no region places: damage.
        let before = d.damaged();
        assert_eq!(
            d.block(&[pcs(2, 0, 9, &[(1, 0, 0)]), ods(9, 0, &[&[1]]), end()].concat()),
            Shown::Unchanged
        );
        assert_eq!(d.damaged(), before + 1);
    }

    #[test]
    fn an_object_is_known_from_its_naming_until_no_region_places_it() {
        // Named by a placement outside its region -- damage -- the object
        // is known all the same: its data later is no damage.
        let mut d = Decoder::new(&[]);
        let outside = [
            pcs(0, 2, 9, &[(1, 0, 0)]),
            rcs(1, 2, 2, 2, Some(1), &[(5, 9, 0)]),
            end(),
        ]
        .concat();
        assert_eq!(d.block(&outside), Shown::Unchanged);
        assert_eq!(d.damaged(), 1);
        let data = [pcs(1, 0, 9, &[(1, 0, 0)]), ods(5, 0, &[&[1]]), end()].concat();
        assert!(matches!(d.block(&data), Shown::Images { .. }));
        assert_eq!(d.damaged(), 1);
        // Placed, then the region placing nothing: forgotten, its data
        // damage.
        let placed = [
            pcs(2, 0, 9, &[(1, 0, 0)]),
            rcs(1, 2, 2, 2, None, &[(7, 0, 0)]),
            end(),
        ]
        .concat();
        d.block(&placed);
        let none = [
            pcs(3, 0, 9, &[(1, 0, 0)]),
            rcs(1, 2, 2, 2, None, &[]),
            end(),
        ]
        .concat();
        d.block(&none);
        let data = [pcs(4, 0, 9, &[(1, 0, 0)]), ods(7, 0, &[&[1]]), end()].concat();
        assert_eq!(d.block(&data), Shown::Unchanged);
        assert_eq!(d.damaged(), 2);
    }

    #[test]
    fn the_non_modifying_colour_leaves_its_pixels_and_the_next_in_place() {
        let mut d = Decoder::new(&[]);
        d.block(
            &[
                pcs(0, 2, 9, &[(1, 0, 0)]),
                rcs(1, 4, 1, 2, Some(3), &[(1, 0, 0)]),
                end(),
            ]
            .concat(),
        );
        let set = [
            pcs(1, 0, 9, &[(1, 0, 0)]),
            ods(1, 0x02, &[&[2, 1, 1, 2]]),
            end(),
        ]
        .concat();
        let shown = images(d.block(&set));
        let fill = Clut::standard(0).four[3];
        let two = Clut::standard(0).four[2];
        assert_eq!(shown[0].rgba, [two, fill, fill, two].concat());
    }

    #[test]
    fn an_entry_for_several_tables_goes_into_each() {
        let mut d = Decoder::new(&[]);
        d.block(
            &[
                pcs(0, 2, 9, &[]),
                cds(0, &[(1, 0xE0, 235, 128, 128, 0)]),
                end(),
            ]
            .concat(),
        );
        let c = &d.cluts[0];
        assert_eq!(
            (c.two[1], c.four[1], c.eight[1]),
            ([255; 4], [255; 4], [255; 4])
        );
    }

    #[test]
    fn only_the_services_pages_are_read() {
        let mut d = Decoder::new(&[0, 1, 0, 2, 0x10]);
        assert_eq!(d.pages(), Some((1, 2)));
        let mut other = pcs(0, 2, 9, &[(1, 0, 0)]);
        other[3] = 3;
        d.block(&[other, end()].concat());
        assert!(d.shown.is_empty(), "page 3 is another service's");
        assert_eq!(
            Decoder::new(&[0, 1, 0, 2, 0x10, 0]).pages(),
            None,
            "six bytes: every page"
        );
        assert_eq!(Decoder::new(&[0, 1, 0, 2]).pages(), Some((1, 2)));
    }

    #[test]
    fn damage_shows_nothing_new_and_is_counted() {
        let mut d = Decoder::new(&[]);
        // A segment running past the block.
        let mut cut = pcs(0, 2, 9, &[(1, 0, 0)]);
        cut.extend_from_slice(&[0x0F, END, 0, 1, 0, 9]);
        assert_eq!(d.block(&cut), Shown::Unchanged);
        assert_eq!(d.damaged(), 1);
        // A region of no size, and one placing an object outside it.
        assert_eq!(
            d.block(
                &[
                    pcs(1, 2, 9, &[(1, 0, 0)]),
                    rcs(1, 0, 4, 2, None, &[]),
                    end()
                ]
                .concat()
            ),
            Shown::Unchanged
        );
        assert_eq!(
            d.block(
                &[
                    pcs(2, 2, 9, &[(1, 0, 0)]),
                    rcs(1, 4, 4, 2, None, &[(1, 4, 0)]),
                    end()
                ]
                .concat()
            ),
            Shown::Unchanged
        );
        assert_eq!(d.damaged(), 3);
        // Every kind of segment but the end: shown all the same.
        let set = [
            pcs(3, 2, 9, &[(1, 0, 0)]),
            rcs(1, 1, 1, 2, Some(1), &[(1, 0, 0)]),
            cds(0, &[WHITE]),
            ods(1, 0, &[&[1]]),
        ]
        .concat();
        assert_eq!(images(d.block(&set)).len(), 1);
        // A page alone and no end: nothing.
        assert_eq!(d.block(&pcs(4, 0, 9, &[(1, 0, 0)])), Shown::Unchanged);
    }

    #[test]
    fn of_several_display_sets_in_a_block_the_last_shows() {
        let mut d = Decoder::new(&[]);
        let show = [
            pcs(0, 2, 9, &[(1, 0, 0)]),
            rcs(1, 1, 1, 2, Some(1), &[]),
            cds(0, &[WHITE]),
            end(),
        ]
        .concat();
        let clear = [pcs(1, 0, 9, &[]), end()].concat();
        assert_eq!(images(d.block(&[show.clone(), clear].concat())), []);
    }

    #[test]
    fn an_epoch_begins_at_an_acquisition_point_or_a_mode_change() {
        assert!(begins_epoch(&pcs(0, 1, 9, &[]), None));
        assert!(begins_epoch(&pcs(0, 2, 9, &[]), Some((1, 1))));
        assert!(!begins_epoch(&pcs(0, 0, 9, &[]), None));
        assert!(
            !begins_epoch(&pcs(0, 2, 9, &[]), Some((2, 2))),
            "another service's page"
        );
    }

    #[test]
    fn a_block_of_six_bytes_or_not_of_segments_is_damage() {
        let mut d = Decoder::new(&[]);
        assert_eq!(d.block(&end()), Shown::Unchanged, "an end segment alone");
        assert_eq!(d.block(&[0x0E; 12]), Shown::Unchanged, "no sync byte");
        assert_eq!(d.damaged(), 2);
    }

    #[test]
    fn a_new_region_is_filled_with_its_background_and_shown() {
        // No fill asked for, but a new region's pixels are its background
        // -- shown, though no object is drawn in it (FFmpeg: not shown).
        let mut d = Decoder::new(&[]);
        let mut region = rcs(1, 2, 1, 2, Some(9), &[]);
        region[7] &= !0x08;
        let shown = images(d.block(&[pcs(0, 2, 9, &[(1, 0, 0)]), region, end()].concat()));
        assert_eq!(shown[0].rgba, [Clut::standard(0).four[9]; 2].concat());
    }

    #[test]
    fn a_line_as_wide_as_its_region_ends_on_its_end_code() {
        // 8-bit lines of two pixels in a region two wide: the end code after
        // each read whole, so the next line is drawn too.
        let mut d = Decoder::new(&[]);
        d.block(
            &[
                pcs(0, 2, 9, &[(1, 0, 0)]),
                rcs(1, 2, 4, 3, None, &[(1, 0, 0)]),
                end(),
            ]
            .concat(),
        );
        let line = [0x12, 7, 9, 0, 0, 0xF0];
        let top = [line, line].concat();
        let mut b = 1u16.to_be_bytes().to_vec();
        b.push(1);
        b.extend_from_slice(&u16::try_from(top.len()).unwrap().to_be_bytes());
        b.extend_from_slice(&0u16.to_be_bytes());
        b.extend(top);
        d.block(&[pcs(1, 0, 9, &[(1, 0, 0)]), seg(OBJECT, 1, &b), end()].concat());
        let rows: Vec<&[u8]> = d.regions[0].pixels.chunks(2).collect();
        assert_eq!(rows, [[7, 9], [7, 9], [7, 9], [7, 9]]);
    }

    #[test]
    fn regions_hold_ffmpegs_largest_four_times_and_place_1024_objects() {
        // A region of one pixel more than FFmpeg's largest: damage.
        let mut d = Decoder::new(&[]);
        let big = |id: u8, h: u16| rcs(id, 1280, h, 2, None, &[]);
        assert_eq!(
            d.block(&[pcs(0, 2, 9, &[]), big(1, 1025), end()].concat()),
            Shown::Unchanged
        );
        assert_eq!(d.damaged(), 1);
        // Four of the largest, then a fifth: damage.
        let mut d = Decoder::new(&[]);
        let four = [
            pcs(0, 2, 9, &[]),
            big(1, 1024),
            big(2, 1024),
            big(3, 1024),
            big(4, 1024),
            end(),
        ]
        .concat();
        assert!(matches!(d.block(&four), Shown::Images { .. }));
        assert_eq!(
            d.block(&[pcs(1, 0, 9, &[]), big(5, 1), end()].concat()),
            Shown::Unchanged
        );
        // 1024 objects placed, then one more: damage.
        let mut d = Decoder::new(&[]);
        let places: Vec<(u16, u16, u16)> = (0..1024).map(|i| (i, 0, 0)).collect();
        let full = [pcs(0, 2, 9, &[]), rcs(1, 1, 1, 2, None, &places), end()].concat();
        assert!(matches!(d.block(&full), Shown::Images { .. }));
        assert_eq!(
            d.block(
                &[
                    pcs(1, 0, 9, &[]),
                    rcs(2, 1, 1, 2, None, &[(9, 0, 0)]),
                    end()
                ]
                .concat()
            ),
            Shown::Unchanged
        );
    }

    #[test]
    fn a_reset_forgets_the_page_held_but_not_the_settled_size() {
        // A page, a region and an object, and no display definition: the
        // size settled, standard definition.
        let mut d = Decoder::new(&[]);
        d.block(
            &[
                pcs(5, 2, 9, &[(1, 0, 0)]),
                rcs(1, 1, 1, 2, Some(1), &[(1, 0, 0)]),
                ods(1, 0, &[&[1]]),
                end(),
            ]
            .concat(),
        );
        d.reset();
        // The same version again, read afresh.
        let shown = images(
            d.block(
                &[
                    pcs(5, 2, 9, &[(1, 3, 3)]),
                    rcs(1, 1, 1, 2, Some(1), &[]),
                    end(),
                ]
                .concat(),
            ),
        );
        assert_eq!((shown[0].x, shown[0].y), (3, 3));
        // A display definition after the size settled: taken, however large.
        let mut b = vec![0x10];
        b.extend_from_slice(&0xFFFFu16.to_be_bytes());
        b.extend_from_slice(&0xFFFFu16.to_be_bytes());
        assert!(matches!(
            d.block(&[seg(DISPLAY, 1, &b), end()].concat()),
            Shown::Images { .. }
        ));
        // As a first: too large a picture for FFmpeg, damage.
        let mut fresh = Decoder::new(&[]);
        assert_eq!(
            fresh.block(&[seg(DISPLAY, 1, &b), end()].concat()),
            Shown::Unchanged
        );
        assert_eq!(fresh.damaged(), 1);
    }
}
