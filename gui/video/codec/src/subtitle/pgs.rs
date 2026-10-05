//! Blu-ray's subtitles -- HDMV presentation graphics, "PGS" -- read as the
//! pictures they are.
//!
//! A track is a series of *display sets*, a Matroska block each
//! (`S_HDMV/PGS`): segments, each a type byte, a 16-bit length and that many
//! bytes.
//!
//! - A *presentation composition* (PCS) gives the canvas -- the size of the
//!   film the subtitles were made for -- the objects to show and where, the
//!   palette to show them in, and whether the set begins an *epoch*: a run
//!   of sets sharing their objects and palettes.
//! - A *window definition* (WDS) says which regions a player's buffers
//!   redraw; nothing shown depends on it.
//! - A *palette definition* (PDS) sets entries of a palette: Y, Cr, Cb and
//!   alpha each.
//! - An *object definition* (ODS) gives an object's picture -- palette
//!   indexes, run-length coded -- in one segment or several.
//! - The *end* segment shows the composition.
//!
//! What is shown is what FFmpeg's `pgssub` decoder shows, read off its
//! output one probe a rule (design-decisions §1362), except that an object
//! the composition crops is cropped, as a Blu-ray player crops it (FFmpeg
//! reads the cropping and ignores it):
//!
//! - An epoch start or an acquisition point (either of the top two bits of
//!   the composition state) forgets the epoch's objects and palettes.
//! - A composition of no objects clears the screen; one naming a palette
//!   the epoch does not hold changes nothing (FFmpeg gives no subtitle);
//!   otherwise its first two objects are shown, in its order, one the epoch
//!   does not hold passed over. A composition that cannot be read clears.
//! - An epoch holds 8 palettes and 64 objects, counted by distinct id: a
//!   new id past that is ignored; a definition of a held id replaces it. A
//!   palette entry never defined is transparent.
//! - An object's run-length codes fill its pixels in order, a line's end
//!   moving nothing; a run that would pass the last pixel is skipped whole;
//!   an object whose codes do not fill it, or larger than the canvas, is
//!   not held (and one of its id held before goes with it).
//! - Colours are converted from BT.709's studio range, BT.601's on a canvas
//!   576 lines tall or less, in 10-bit fixed point as FFmpeg converts them;
//!   alpha is kept, and so is the colour of a transparent entry.
//!
//! Two bounds are this reader's own, for files FFmpeg would read into
//! gigabytes: a canvas is at most 8192 pixels each way, and an epoch's
//! objects hold at most 32 MiB of pixels in all.

use super::CueImage;

/// Segment types.
const PDS: u8 = 0x14;
const ODS: u8 = 0x15;
const PCS: u8 = 0x16;
const END: u8 = 0x80;

/// The palettes an epoch holds, as FFmpeg holds them.
const MAX_PALETTES: usize = 8;
/// The objects an epoch holds, as FFmpeg holds them.
const MAX_OBJECTS: usize = 64;
/// The objects a composition shows: a Blu-ray player's limit, and FFmpeg's.
const MAX_SHOWN: usize = 2;
/// The largest canvas read, each way: 8K is 7680 by 4320.
const MAX_CANVAS: u16 = 8192;
/// The pixels an epoch's objects may hold in all: 32 MiB, sixteen pictures
/// the size of a 1080-line film.
const MAX_HELD: usize = 32 << 20;

/// What a display set does to the screen.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Shown {
    /// These images, in place of whatever was shown; none clears it.
    Images(Vec<CueImage>),
    /// Nothing changes: no end segment, or a composition naming a palette
    /// the epoch does not hold.
    Unchanged,
}

/// One object a composition shows.
#[derive(Clone, Copy, Debug)]
struct Placed {
    id: u16,
    x: u16,
    y: u16,
    /// The part shown, relative to the object: left, top, width, height.
    crop: Option<[u16; 4]>,
    forced: bool,
}

/// What the last composition said.
#[derive(Clone, Debug, Default)]
struct Composition {
    palette: u8,
    /// At most [`MAX_SHOWN`].
    objects: Vec<Placed>,
}

/// An object's picture: palette indexes, row by row.
struct Object {
    width: u16,
    height: u16,
    pixels: Vec<u8>,
}

/// An object whose segments are still coming.
struct Partial {
    id: u16,
    width: u16,
    height: u16,
    /// The run-length codes so far.
    codes: Vec<u8>,
}

/// A palette: 256 entries, red, green, blue and alpha each.
type Palette = [[u8; 4]; 256];

/// A PGS track's state from one display set to the next.
#[derive(Default)]
pub(crate) struct Decoder {
    /// The canvas, as the last composition gave it.
    width: u16,
    height: u16,
    composition: Composition,
    palettes: Vec<(u8, Box<Palette>)>,
    objects: Vec<(u16, Object)>,
    partial: Option<Partial>,
    /// Whether the display set being read has met damage.
    hurt: bool,
    /// The display sets that met damage, as long as the track is read.
    damaged: u64,
}

impl Decoder {
    /// Forget the epoch, as a decoder joining the track does: after a
    /// seek, an epoch's objects come again only at its next start or
    /// acquisition point. The count of damage is kept.
    pub(crate) fn reset(&mut self) {
        let damaged = self.damaged;
        *self = Self {
            damaged,
            ..Self::default()
        };
    }

    /// The display sets that met damage: a composition that cannot be
    /// read, an object whose codes do not fill it or that does not fit, a
    /// segment cut off, a palette or object past an epoch's bounds.
    pub(crate) fn damaged(&self) -> u64 {
        self.damaged
    }

    /// One display set -- a block's segments -- and what it shows: the
    /// last end segment's composition, or [`Shown::Unchanged`] with none.
    pub(crate) fn display_set(&mut self, block: &[u8]) -> Shown {
        self.hurt = false;
        let mut shown = Shown::Unchanged;
        let mut rest = block;
        while let [kind, h, l, tail @ ..] = rest {
            let n = usize::from(u16::from_be_bytes([*h, *l]));
            // A segment cut off by the block's end is read as far as it
            // goes: the rest of its fields are missing.
            if n > tail.len() {
                self.hurt = true;
            }
            let (body, after) = tail.split_at(n.min(tail.len()));
            match *kind {
                PCS => self.presentation(body),
                PDS => self.palette(body),
                ODS => self.object(body),
                END => shown = self.show(),
                // A window's region (WDS, 0x17), and anything else, show
                // nothing.
                _ => {}
            }
            rest = after;
        }
        if !rest.is_empty() {
            // A segment's header cut off.
            self.hurt = true;
        }
        if self.hurt {
            self.damaged = self.damaged.saturating_add(1);
        }
        shown
    }

    fn presentation(&mut self, body: &[u8]) {
        // A composition that cannot be read shows nothing: FFmpeg's clears.
        self.composition = Composition::default();
        let [
            w0,
            w1,
            h0,
            h1,
            _rate,
            _n0,
            _n1,
            state,
            _update,
            palette,
            count,
            objects @ ..,
        ] = body
        else {
            self.hurt = true;
            return;
        };
        let (width, height) = (
            u16::from_be_bytes([*w0, *w1]),
            u16::from_be_bytes([*h0, *h1]),
        );
        if width > MAX_CANVAS || height > MAX_CANVAS {
            self.hurt = true;
            return;
        }
        (self.width, self.height) = (width, height);
        if state & 0xC0 != 0 {
            self.palettes.clear();
            self.objects.clear();
            self.partial = None;
        }
        let mut placed = Vec::new();
        let mut rest = objects;
        for _ in 0..usize::from(*count).min(MAX_SHOWN) {
            let [i0, i1, _window, flags, x0, x1, y0, y1, tail @ ..] = rest else {
                self.hurt = true;
                return;
            };
            let mut p = Placed {
                id: u16::from_be_bytes([*i0, *i1]),
                x: u16::from_be_bytes([*x0, *x1]),
                y: u16::from_be_bytes([*y0, *y1]),
                crop: None,
                forced: flags & 0x40 != 0,
            };
            rest = tail;
            if flags & 0x80 != 0 {
                let [a0, a1, b0, b1, c0, c1, d0, d1, tail @ ..] = rest else {
                    self.hurt = true;
                    return;
                };
                p.crop = Some([
                    u16::from_be_bytes([*a0, *a1]),
                    u16::from_be_bytes([*b0, *b1]),
                    u16::from_be_bytes([*c0, *c1]),
                    u16::from_be_bytes([*d0, *d1]),
                ]);
                rest = tail;
            }
            placed.push(p);
        }
        self.composition = Composition {
            palette: *palette,
            objects: placed,
        };
    }

    fn palette(&mut self, body: &[u8]) {
        let [id, _version, entries @ ..] = body else {
            self.hurt = true;
            return;
        };
        if !self.palettes.iter().any(|(i, _)| i == id) {
            if self.palettes.len() >= MAX_PALETTES {
                self.hurt = true;
                return;
            }
            self.palettes.push((*id, Box::new([[0; 4]; 256])));
        }
        let sd = self.height > 0 && self.height <= 576;
        let Some((_, palette)) = self.palettes.iter_mut().find(|(i, _)| i == id) else {
            return;
        };
        for e in entries.chunks_exact(5) {
            if let [index, y, cr, cb, alpha] = *e {
                let [r, g, b] = rgb(y, cr, cb, sd);
                if let Some(slot) = palette.get_mut(usize::from(index)) {
                    *slot = [r, g, b, alpha];
                }
            }
        }
    }

    fn object(&mut self, body: &[u8]) {
        let [i0, i1, _version, sequence, rest @ ..] = body else {
            self.hurt = true;
            return;
        };
        let id = u16::from_be_bytes([*i0, *i1]);
        if sequence & 0x80 != 0 {
            // The first segment: the codes' length (with the size's four
            // bytes), the size, then codes.
            let [_l0, _l1, _l2, w0, w1, h0, h1, codes @ ..] = rest else {
                self.hurt = true;
                self.partial = None;
                return;
            };
            self.partial = Some(Partial {
                id,
                width: u16::from_be_bytes([*w0, *w1]),
                height: u16::from_be_bytes([*h0, *h1]),
                codes: codes.to_vec(),
            });
        } else if let Some(p) = self.partial.as_mut().filter(|p| p.id == id) {
            p.codes.extend_from_slice(rest);
        } else {
            // The rest of an object whose start was never seen.
            self.hurt = true;
            return;
        }
        if sequence & 0x40 == 0 {
            return;
        }
        let Some(p) = self.partial.take() else {
            return;
        };
        self.hold(p);
    }

    /// An object whose last segment has come: decoded and held, or -- not
    /// fitting the canvas or the bounds, or its codes not filling it --
    /// not held, taking any object of its id with it.
    fn hold(&mut self, p: Partial) {
        let held = self.objects.iter().position(|(i, _)| *i == p.id);
        if held.is_none() && self.objects.len() >= MAX_OBJECTS {
            self.hurt = true;
            return;
        }
        let size = usize::from(p.width).saturating_mul(usize::from(p.height));
        let others: usize = self
            .objects
            .iter()
            .enumerate()
            .filter(|(at, _)| Some(*at) != held)
            .map(|(_, (_, o))| o.pixels.len())
            .fold(0, usize::saturating_add);
        let fits = p.width <= self.width
            && p.height <= self.height
            && others.saturating_add(size) <= MAX_HELD;
        let pixels = if fits {
            decode(&p.codes, p.width, p.height)
        } else {
            None
        };
        if pixels.is_none() {
            self.hurt = true;
        }
        match (pixels, held) {
            (Some(pixels), Some(at)) => {
                if let Some(slot) = self.objects.get_mut(at) {
                    slot.1 = Object {
                        width: p.width,
                        height: p.height,
                        pixels,
                    };
                }
            }
            (Some(pixels), None) => self.objects.push((
                p.id,
                Object {
                    width: p.width,
                    height: p.height,
                    pixels,
                },
            )),
            (None, Some(at)) => {
                self.objects.remove(at);
            }
            (None, None) => {}
        }
    }

    /// The end segment: the composition shown.
    fn show(&self) -> Shown {
        let c = &self.composition;
        if c.objects.is_empty() {
            return Shown::Images(Vec::new());
        }
        let Some((_, palette)) = self.palettes.iter().find(|(i, _)| *i == c.palette) else {
            return Shown::Unchanged;
        };
        let images = c
            .objects
            .iter()
            .filter_map(|p| {
                let (_, o) = self.objects.iter().find(|(i, _)| *i == p.id)?;
                image(o, p, palette, self.width, self.height)
            })
            .collect();
        Shown::Images(images)
    }
}

/// Whether a display set begins what it shows afresh -- its composition an
/// epoch start or an acquisition point -- so that decoding can start at it:
/// what a seek goes back to.
pub(crate) fn begins_epoch(block: &[u8]) -> bool {
    let mut rest = block;
    while let [kind, h, l, tail @ ..] = rest {
        let n = usize::from(u16::from_be_bytes([*h, *l]));
        let (body, after) = tail.split_at(n.min(tail.len()));
        if *kind == PCS {
            return body.get(7).is_some_and(|state| state & 0xC0 != 0);
        }
        rest = after;
    }
    false
}

/// `o` as placed by `p`, its pixels in `palette`'s colours; `None` where its
/// crop leaves nothing.
fn image(o: &Object, p: &Placed, palette: &Palette, width: u16, height: u16) -> Option<CueImage> {
    let [left, top, w, h] = p.crop.unwrap_or([0, 0, o.width, o.height]);
    // The crop within the object.
    let right = left.saturating_add(w).min(o.width);
    let bottom = top.saturating_add(h).min(o.height);
    let (w, h) = (right.checked_sub(left)?, bottom.checked_sub(top)?);
    if w == 0 || h == 0 {
        return None;
    }
    let mut rgba = Vec::with_capacity(
        usize::from(w)
            .saturating_mul(usize::from(h))
            .saturating_mul(4),
    );
    for row in top..bottom {
        let start = usize::from(row).saturating_mul(usize::from(o.width));
        let line = o.pixels.get(
            start.saturating_add(usize::from(left))..start.saturating_add(usize::from(right)),
        )?;
        for &index in line {
            rgba.extend_from_slice(palette.get(usize::from(index)).unwrap_or(&[0; 4]));
        }
    }
    Some(CueImage {
        canvas_width: u32::from(width),
        canvas_height: u32::from(height),
        x: u32::from(p.x),
        y: u32::from(p.y),
        width: u32::from(w),
        height: u32::from(h),
        rgba,
        forced: p.forced,
    })
}

/// An object's run-length codes, decoded into its `width * height` palette
/// indexes as FFmpeg decodes them; `None` when they do not fill it.
///
/// A byte not 0 is a pixel of that index. A 0 begins a code: a flags byte,
/// whose low six bits are a run's length -- with the next byte below them
/// when bit 6 is set -- of index 0, or of the index in the byte after when
/// bit 7 is set; a run of no length ends a line. A code cut off by the end
/// of the data reads 0s past it, as FFmpeg's zero padding does.
fn decode(codes: &[u8], width: u16, height: u16) -> Option<Vec<u8>> {
    let total = usize::from(width).checked_mul(usize::from(height))?;
    let mut pixels = vec![0u8; total];
    let (mut filled, mut lines, mut at) = (0usize, 0u16, 0usize);
    while at < codes.len() && lines < height {
        let first = byte(codes, &mut at);
        let (run, index) = if first != 0 {
            (1, first)
        } else {
            let flags = byte(codes, &mut at);
            let run = if flags & 0x40 == 0 {
                usize::from(flags & 0x3F)
            } else {
                usize::from(u16::from_be_bytes([flags & 0x3F, byte(codes, &mut at)]))
            };
            let index = if flags & 0x80 == 0 {
                0
            } else {
                byte(codes, &mut at)
            };
            (run, index)
        };
        if run == 0 {
            lines = lines.saturating_add(1);
        } else if let Some(end) = filled.checked_add(run).filter(|&e| e <= total) {
            if let Some(span) = pixels.get_mut(filled..end) {
                span.fill(index);
            }
            filled = end;
        }
    }
    (filled >= total).then_some(pixels)
}

/// The byte at `*at`, 0 past the end; `*at` moves on either way.
fn byte(codes: &[u8], at: &mut usize) -> u8 {
    let b = codes.get(*at).copied().unwrap_or(0);
    *at = at.saturating_add(1);
    b
}

/// A palette entry's colour: Y, Cr and Cb in the studio range, by BT.709's
/// matrix or (`sd`) BT.601's, in FFmpeg's 10-bit fixed point -- each
/// coefficient times 255/224 (Y's 255/219) rounded to a 1024th, the sum
/// rounded half up and clamped.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "every term is under 300 000 in size: no sum nears i32's bounds"
)]
pub(crate) fn rgb(y: u8, cr: u8, cb: u8, sd: bool) -> [u8; 3] {
    // In 1024ths: 1.5747, 0.1873, 0.4682 and 1.8556 (BT.709), or 1.402,
    // 0.34414, 0.71414 and 1.772 (BT.601), each times 255/224.
    let (r_cr, g_cb, g_cr, b_cb) = if sd {
        (1634, 401, 832, 2066)
    } else {
        (1836, 218, 546, 2163)
    };
    let (cb, cr) = (i32::from(cb) - 128, i32::from(cr) - 128);
    let y = (i32::from(y) - 16) * 1192;
    let channel = |v: i32| u8::try_from(((y + v + 512) >> 10).clamp(0, 255)).unwrap_or(u8::MAX);
    [
        channel(r_cr * cr),
        channel(-g_cb * cb - g_cr * cr),
        channel(b_cb * cb),
    ]
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "a test: a failure should be loud, and its numbers are small"
    )]

    use super::*;

    /// A segment: its type, its length, its body.
    fn seg(kind: u8, body: &[u8]) -> Vec<u8> {
        let n = u16::try_from(body.len()).unwrap();
        [&[kind][..], &n.to_be_bytes(), body].concat()
    }

    /// One object of a composition: id, x, y, forced, crop.
    type Obj = (u16, u16, u16, bool, Option<[u16; 4]>);

    /// A composition on a 1280 x 720 canvas: its state, its palette, its
    /// objects.
    fn pcs(state: u8, palette: u8, objects: &[Obj]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(1280u16.to_be_bytes());
        b.extend(720u16.to_be_bytes());
        let count = u8::try_from(objects.len()).unwrap();
        b.extend([0x10, 0, 0, state, 0, palette, count]);
        for &(id, x, y, forced, crop) in objects {
            b.extend(id.to_be_bytes());
            b.push(0);
            b.push((u8::from(crop.is_some()) * 0x80) | (u8::from(forced) * 0x40));
            b.extend(x.to_be_bytes());
            b.extend(y.to_be_bytes());
            if let Some(crop) = crop {
                for v in crop {
                    b.extend(v.to_be_bytes());
                }
            }
        }
        seg(PCS, &b)
    }

    /// A palette: (entry, Y, Cr, Cb, A) each.
    fn pds(id: u8, entries: &[[u8; 5]]) -> Vec<u8> {
        seg(PDS, &[&[id, 0][..], &entries.concat()].concat())
    }

    /// An object in one segment, from its rows of indexes: a pixel a byte,
    /// each row ended.
    fn ods(id: u16, rows: &[&[u8]]) -> Vec<u8> {
        let codes: Vec<u8> = rows
            .iter()
            .flat_map(|r| [r.to_vec(), vec![0, 0]].concat())
            .collect();
        let w = u16::try_from(rows[0].len()).unwrap();
        let h = u16::try_from(rows.len()).unwrap();
        let len = u32::try_from(codes.len() + 4).unwrap().to_be_bytes();
        let mut b = id.to_be_bytes().to_vec();
        b.extend([0, 0xC0, len[1], len[2], len[3]]);
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend(codes);
        seg(ODS, &b)
    }

    fn end() -> Vec<u8> {
        seg(END, &[])
    }

    const WHITE: [u8; 5] = [1, 235, 128, 128, 255];
    const RED: [u8; 5] = [2, 81, 240, 90, 128];

    /// The images of a display set that shows some.
    fn images(d: &mut Decoder, block: &[u8]) -> Vec<CueImage> {
        match d.display_set(block) {
            Shown::Images(images) => images,
            Shown::Unchanged => panic!("the display set changed nothing"),
        }
    }

    /// Epoch start, palette 0, the object `rows` as object 0 at (0, 0).
    fn first(rows: &[&[u8]]) -> Vec<u8> {
        [
            pcs(0x80, 0, &[(0, 0, 0, false, None)]),
            pds(0, &[WHITE, RED]),
            ods(0, rows),
            end(),
        ]
        .concat()
    }

    #[test]
    fn a_display_set_shows_its_composition() {
        let mut d = Decoder::default();
        let block = [
            pcs(0x80, 0, &[(0, 10, 20, false, None)]),
            pds(0, &[WHITE, RED]),
            ods(0, &[&[1, 2]]),
            end(),
        ]
        .concat();
        assert_eq!(
            images(&mut d, &block),
            [CueImage {
                canvas_width: 1280,
                canvas_height: 720,
                x: 10,
                y: 20,
                width: 2,
                height: 1,
                rgba: vec![255, 255, 255, 255, 255, 24, 0, 128],
                forced: false,
            }]
        );
    }

    #[test]
    fn the_run_length_codes_read_as_ffmpeg_reads_them() {
        // A pixel; runs of index 0, short and long; runs of an index, short
        // and long; a line's end.
        let codes = [
            9, 0, 0x03, 0, 0x40, 0x05, 0, 0x82, 7, 0, 0xC0, 0x03, 4, 0, 0,
        ];
        let mut want = vec![9];
        want.extend([0; 8]);
        want.extend([7; 2]);
        want.extend([4; 3]);
        assert_eq!(decode(&codes, 14, 1), Some(want));
        // Codes that do not fill the object: not an object.
        assert_eq!(decode(&[1, 1, 0, 0], 2, 2), None);
        // A run that would pass the last pixel is skipped whole, and a
        // line's end moves nothing: the 2s go, the 5s fill the end.
        let codes = [0, 0x84, 1, 0, 0, 0, 0x83, 2, 5, 5, 0, 0];
        assert_eq!(decode(&codes, 3, 2), Some(vec![1, 1, 1, 1, 5, 5]));
        // A code cut off reads 0s past the data: a run of 2 of index 0.
        assert_eq!(decode(&[0, 0x82], 2, 1), Some(vec![0, 0]));
        // Lines past the height are not read.
        assert_eq!(decode(&[3, 0, 0, 4, 0, 0], 1, 1), Some(vec![3]));
    }

    #[test]
    fn colours_are_ffmpegs() {
        // BT.709: where exact arithmetic rounds as FFmpeg does...
        assert_eq!(rgb(235, 128, 128, false), [255, 255, 255]);
        assert_eq!(rgb(81, 240, 90, false), [255, 24, 0]);
        assert_eq!(rgb(145, 34, 54, false), [0, 216, 0]);
        assert_eq!(rgb(41, 110, 240, false), [0, 15, 255]);
        // ... and where FFmpeg's fixed point does not.
        assert_eq!(rgb(255, 255, 255, false), [255, 183, 255]);
        assert_eq!(rgb(138, 242, 33, false), [255, 101, 0]);
        // BT.601, for a canvas 576 lines tall or less.
        assert_eq!(rgb(81, 240, 90, true), [254, 0, 0]);
        assert_eq!(rgb(145, 34, 54, true), [0, 255, 1]);
        assert_eq!(rgb(1, 1, 1, true), [0, 135, 0]);
        assert_eq!(rgb(215, 161, 61, true), [255, 231, 96]);
    }

    #[test]
    fn a_short_canvas_converts_by_bt601() {
        let mut d = Decoder::default();
        let mut block = pcs(0x80, 0, &[(0, 0, 0, false, None)]);
        // 720 x 480.
        block[3..7].copy_from_slice(&[0x02, 0xD0, 0x01, 0xE0]);
        block.extend([pds(0, &[[1, 81, 240, 90, 255]]), ods(0, &[&[1]]), end()].concat());
        assert_eq!(images(&mut d, &block)[0].rgba, [254, 0, 0, 255]);
    }

    #[test]
    fn nothing_composed_clears_and_a_missing_palette_changes_nothing() {
        let mut d = Decoder::default();
        assert_eq!(images(&mut d, &[pcs(0x80, 0, &[]), end()].concat()), []);
        let block = [
            pcs(0x80, 3, &[(0, 0, 0, false, None)]),
            pds(0, &[WHITE]),
            ods(0, &[&[1]]),
            end(),
        ]
        .concat();
        assert_eq!(d.display_set(&block), Shown::Unchanged);
        // No end segment: nothing yet; an end alone shows what was composed.
        let block = [pcs(0x00, 0, &[(0, 5, 5, false, None)]), pds(0, &[WHITE])].concat();
        assert_eq!(d.display_set(&block), Shown::Unchanged);
        assert_eq!(images(&mut d, &end()).len(), 1);
        // A composition cut short clears.
        let short = seg(PCS, &[5, 0, 2, 0xD0, 0x10]);
        assert_eq!(images(&mut d, &[short, end()].concat()), []);
    }

    #[test]
    fn an_epoch_start_or_acquisition_point_forgets_objects_and_palettes() {
        let mut d = Decoder::default();
        assert_eq!(images(&mut d, &first(&[&[1]])).len(), 1);
        // A normal set uses the epoch's object and palette.
        let moved = [pcs(0x00, 0, &[(0, 9, 9, false, None)]), end()].concat();
        assert_eq!(images(&mut d, &moved)[0].x, 9);
        for state in [0x40, 0x80, 0xC0] {
            images(&mut d, &first(&[&[1]]));
            // The palette again, the object not: nothing to show.
            let bare = [
                pcs(state, 0, &[(0, 0, 0, false, None)]),
                pds(0, &[WHITE]),
                end(),
            ]
            .concat();
            assert_eq!(images(&mut d, &bare), [], "state {state:#x}");
        }
        // Any other state keeps them.
        images(&mut d, &first(&[&[1]]));
        let kept = [pcs(0x20, 0, &[(0, 3, 3, false, None)]), end()].concat();
        assert_eq!(images(&mut d, &kept)[0].x, 3);
    }

    #[test]
    fn two_objects_are_shown_and_an_epoch_holds_eight_palettes_and_64_objects() {
        let mut d = Decoder::default();
        let objects: Vec<Obj> = (0..3).map(|i| (i, i * 10, 0, false, None)).collect();
        let mut block = [pcs(0x80, 0, &objects), pds(0, &[WHITE])].concat();
        for i in 0..3 {
            block.extend(ods(i, &[&[1]]));
        }
        block.extend(end());
        let xs: Vec<u32> = images(&mut d, &block).iter().map(|i| i.x).collect();
        assert_eq!(xs, [0, 10]);
        // Palettes 0..9: the ninth and tenth are not held.
        let mut block = pcs(0x80, 9, &[(0, 0, 0, false, None)]);
        for p in 0..10 {
            block.extend(pds(p, &[WHITE]));
        }
        block.extend([ods(0, &[&[1]]), end()].concat());
        assert_eq!(d.display_set(&block), Shown::Unchanged);
        // Counted, not numbered: palette 200 is held.
        let block = [
            pcs(0x80, 200, &[(0, 0, 0, false, None)]),
            pds(200, &[WHITE]),
            ods(0, &[&[1]]),
            end(),
        ]
        .concat();
        assert_eq!(images(&mut d, &block).len(), 1);
        // Objects 0..=64: the 65th is not held; a held id is replaced.
        let shown = [(64, 0, 0, false, None), (3, 5, 0, false, None)];
        let mut block = [pcs(0x80, 0, &shown), pds(0, &[WHITE])].concat();
        for i in 0..65 {
            block.extend(ods(i, &[&[1]]));
        }
        block.extend(ods(3, &[&[1, 1]]));
        block.extend(end());
        let shown = images(&mut d, &block);
        assert_eq!(shown.len(), 1);
        assert_eq!((shown[0].x, shown[0].width), (5, 2));
    }

    #[test]
    fn a_crop_shows_its_part_and_forced_is_kept() {
        let mut d = Decoder::default();
        let block = [
            pcs(0x80, 0, &[(0, 7, 8, true, Some([1, 0, 1, 1]))]),
            pds(0, &[WHITE, RED]),
            ods(0, &[&[1, 2], &[1, 1]]),
            end(),
        ]
        .concat();
        let shown = images(&mut d, &block);
        let s = &shown[0];
        assert_eq!((s.x, s.y, s.width, s.height, s.forced), (7, 8, 1, 1, true));
        assert_eq!(s.rgba, [255, 24, 0, 128]);
        // A crop past the object is cut to it; one outside it shows nothing.
        let block = [pcs(0x00, 0, &[(0, 0, 0, false, Some([1, 1, 9, 9]))]), end()].concat();
        assert_eq!(images(&mut d, &block)[0].width, 1);
        let block = [pcs(0x00, 0, &[(0, 0, 0, false, Some([5, 0, 1, 1]))]), end()].concat();
        assert_eq!(images(&mut d, &block), []);
    }

    #[test]
    fn an_epoch_s_beginning_is_known_and_damage_is_counted() {
        assert!(begins_epoch(&first(&[&[1]])));
        assert!(begins_epoch(&[pcs(0x40, 0, &[]), end()].concat()));
        assert!(!begins_epoch(&[pcs(0x00, 0, &[]), end()].concat()));
        assert!(!begins_epoch(&end()));
        let mut d = Decoder::default();
        images(&mut d, &first(&[&[1]]));
        assert_eq!(d.damaged(), 0);
        // A composition cut short; a palette claiming more than the block
        // holds; the rest of an object whose start never came.
        images(&mut d, &[seg(PCS, &[5, 0]), end()].concat());
        let mut cut = end();
        cut.extend([PDS, 0, 9, 0]);
        d.display_set(&cut);
        assert_eq!(
            d.display_set(&seg(ODS, &[0, 0, 0, 0x40, 1])),
            Shown::Unchanged
        );
        assert_eq!(d.damaged(), 3);
        d.reset();
        assert_eq!(d.damaged(), 3, "kept across a seek");
    }

    #[test]
    fn a_canvas_past_8192_is_damage() {
        let mut d = Decoder::default();
        let mut block = first(&[&[1]]);
        block[3..5].copy_from_slice(&8193u16.to_be_bytes());
        assert_eq!(images(&mut d, &block), []);
        assert_eq!(d.damaged(), 1);
        // 8192 itself is read.
        block[3..5].copy_from_slice(&8192u16.to_be_bytes());
        assert_eq!(images(&mut d, &block).len(), 1);
    }

    #[test]
    fn an_epoch_holds_32_mib_of_pixels() {
        let mut d = Decoder::default();
        // An 8192-pixel square canvas, and three objects 8192 by 1400 --
        // 11.5 million pixels each -- the third past the bound.
        let mut block = pcs(0x80, 0, &[(2, 0, 0, false, None)]);
        block[3..7].copy_from_slice(&[0x20, 0, 0x20, 0]);
        block.extend(pds(0, &[WHITE]));
        // A run of 8192 of colour 1 and a line's end, 1400 times.
        let line = [0, 0xC0 | 0x20, 0, 1, 0, 0];
        let codes: Vec<u8> = line
            .iter()
            .copied()
            .cycle()
            .take(line.len() * 1400)
            .collect();
        for id in 0..3u16 {
            let mut body = id.to_be_bytes().to_vec();
            body.extend([0, 0xC0, 0, 0, 0]);
            body.extend(8192u16.to_be_bytes());
            body.extend(1400u16.to_be_bytes());
            body.extend(&codes);
            block.extend(seg(ODS, &body));
        }
        block.extend(end());
        assert_eq!(images(&mut d, &block), []);
        assert_eq!(d.damaged(), 1);
    }

    #[test]
    fn an_object_in_two_segments_is_one_and_damage_takes_the_old_one_with_it() {
        let mut d = Decoder::default();
        // Object 0's one segment, split after its header and a code: its
        // body's first part marked first (0x80), the rest last (0x40).
        let whole = ods(0, &[&[1, 1, 1]]);
        let body = &whole[3..];
        let (a, b) = body.split_at(12);
        let mut head = a.to_vec();
        head[3] = 0x80;
        let mut tail = body[..4].to_vec();
        tail[3] = 0x40;
        tail.extend_from_slice(b);
        // Between them, the rest of an object 1 never begun -- a line's end
        // -- which is not object 0's.
        let stray = seg(ODS, &[0, 1, 0, 0x40, 0, 0]);
        let block = [
            pcs(0x80, 0, &[(0, 0, 0, false, None)]),
            pds(0, &[WHITE]),
            seg(ODS, &head),
            stray,
            seg(ODS, &tail),
            end(),
        ]
        .concat();
        assert_eq!(images(&mut d, &block)[0].width, 3);
        // Codes a line short: not held, and the old object 0 goes too.
        let mut short = ods(0, &[&[1, 1], &[1, 1]]);
        let n = short.len();
        short.truncate(n - 4);
        short[1..3].copy_from_slice(&u16::try_from(n - 7).unwrap().to_be_bytes());
        let block = [pcs(0x00, 0, &[(0, 0, 0, false, None)]), short, end()].concat();
        assert_eq!(images(&mut d, &block), []);
        // Larger than the canvas: not held either.
        let wide = vec![1u8; 1281];
        let block = [
            pcs(0x80, 0, &[(0, 0, 0, false, None)]),
            pds(0, &[WHITE]),
            ods(0, &[&wide]),
            end(),
        ]
        .concat();
        assert_eq!(images(&mut d, &block), []);
    }
}
