//! DVD's subtitles -- VobSub (`S_VOBSUB`) -- read as the pictures they are.
//!
//! The track's setup (Matroska's CodecPrivate) is the `.idx` file's header:
//! `size: 720x480`, the canvas the pictures are placed on, and `palette:`,
//! the sixteen colours (`rrggbb`) a picture chooses its four among. Each
//! block is a *subpicture unit* (SPU):
//!
//! - its size, and where its *control sequences* begin;
//! - the picture's pixels, two bits each, run-length coded a line at a time
//!   in two *fields* -- the even lines, then the odd -- each line ending on a
//!   byte;
//! - control sequences, each a *date* (in 1024ths of 90 kHz ticks after the
//!   block's time), where the next begins, and commands: start (or *forced*
//!   start), stop, the four colours' palette indexes, their four alphas
//!   (0-15), the area on the canvas, and where the fields begin.
//!
//! What is shown is what FFmpeg's `dvdsub` decoder shows, read off its output
//! one probe a rule (design-decisions §1363): colours straight from the
//! palette, alpha times 17; the transparent rows and columns at a picture's
//! edges cut away, but for a forced picture's, which is kept whole (a
//! transparent pixel inside keeps its colour either way); a picture
//! whose codes do not fill its area, or whose fields lie past the data,
//! changes nothing; no palette, and greys are made up as FFmpeg makes them;
//! no start, and the picture shows from the block's time; no stop, and the
//! block's duration ends it; times in FFmpeg's whole milliseconds. But for
//! where a DVD player shows otherwise: each control sequence takes effect at
//! its own date -- a later colour or fade, or a second start, changes the
//! screen then (FFmpeg draws the picture once, as it is first placed, and
//! keeps the last start and stop) -- and an SPU whose every pixel is
//! transparent clears the screen (FFmpeg leaves the old picture up).
//!
//! `custom colors` in the setup -- VSFilter's own recolouring -- is ignored,
//! as FFmpeg ignores it.

use super::CueImage;

/// Commands.
const FORCED_START: u8 = 0x00;
const START: u8 = 0x01;
const STOP: u8 = 0x02;
const COLOURS: u8 = 0x03;
const ALPHAS: u8 = 0x04;
const AREA: u8 = 0x05;
const FIELDS: u8 = 0x06;

/// The control sequences of an SPU read: a bound on a damaged one's loop.
const MAX_SEQUENCES: usize = 64;

/// The canvas where the setup gives none: FFmpeg's.
const DEFAULT_CANVAS: (u32, u32) = (720, 576);

/// A VobSub track's setup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Setup {
    width: u32,
    height: u32,
    /// Red, green and blue of each of the sixteen colours; `None` where the
    /// setup gives no palette.
    palette: Option<[[u8; 3]; 16]>,
}

impl Setup {
    /// The `.idx` header's `size:` and `palette:` lines; anything else, and
    /// a line that does not read, ignored.
    pub(crate) fn parse(config: &[u8]) -> Self {
        let mut setup = Self {
            width: DEFAULT_CANVAS.0,
            height: DEFAULT_CANVAS.1,
            palette: None,
        };
        for line in config.split(|&b| b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if let Some(size) = line.strip_prefix(b"size:") {
                if let Some((w, h)) = size_of(size) {
                    (setup.width, setup.height) = (w, h);
                }
            } else if let Some(colours) = line.strip_prefix(b"palette:") {
                setup.palette = palette_of(colours);
            }
        }
        setup
    }
}

/// `WxH`, as `size:` gives it.
fn size_of(text: &[u8]) -> Option<(u32, u32)> {
    let text = core::str::from_utf8(text).ok()?.trim();
    let (w, h) = text.split_once('x')?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// Sixteen `rrggbb`s, separated by commas.
fn palette_of(text: &[u8]) -> Option<[[u8; 3]; 16]> {
    let text = core::str::from_utf8(text).ok()?;
    let mut palette = [[0u8; 3]; 16];
    let mut colours = text.split(',');
    for slot in &mut palette {
        let rgb = u32::from_str_radix(colours.next()?.trim(), 16).ok()?;
        let [_, r, g, b] = rgb.to_be_bytes();
        *slot = [r, g, b];
    }
    Some(palette)
}

/// A change an SPU makes to the screen: from `at` nanoseconds after its
/// block's time, these images -- none, a clear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Change {
    pub(crate) at: i64,
    pub(crate) images: Vec<CueImage>,
}

/// What an SPU's commands have set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Picture {
    /// The palette index of each of the four colours a pixel can be.
    colours: [u8; 4],
    /// Each colour's alpha, 0 to 15.
    alphas: [u8; 4],
    /// Left, right, top, bottom, inclusive.
    area: Option<[u16; 4]>,
    /// Where the even lines' codes begin, and the odd lines'.
    fields: Option<[u16; 2]>,
}

/// One control sequence, read: when, what it did, and the picture as its
/// commands left it.
struct Sequence {
    /// Milliseconds after the block's time.
    ms: i64,
    start: bool,
    forced: bool,
    stop: bool,
    picture: Picture,
}

/// The changes an SPU makes, in order -- or `None` for one that cannot be
/// read, which changes nothing. `duration`, the block's, in nanoseconds,
/// ends a picture its SPU never stops.
pub(crate) fn changes(setup: &Setup, spu: &[u8], duration: Option<i64>) -> Option<Vec<Change>> {
    let [s0, s1, ..] = *spu else {
        return None;
    };
    let size = usize::from(u16::from_be_bytes([s0, s1]));
    let spu = spu.get(..size.min(spu.len()))?;
    let mut sequences = sequences(spu)?;
    // No start anywhere: FFmpeg shows the picture from the block's time, as
    // the first sequence that places it leaves it.
    if !sequences.iter().any(|s| s.start) {
        if let Some(first) = sequences
            .iter_mut()
            .find(|s| s.picture.area.is_some() && s.picture.fields.is_some())
        {
            (first.start, first.ms) = (true, 0);
        }
    }
    let mut shown: Option<(Picture, bool)> = None;
    let mut out: Vec<Change> = Vec::new();
    for s in &sequences {
        // Everything a sequence sets takes effect at its date.
        let now = if s.stop {
            None
        } else if s.start {
            Some((s.picture, s.forced))
        } else {
            shown.map(|(_, forced)| (s.picture, forced))
        };
        if now == shown && !s.start {
            continue;
        }
        let images = match now {
            Some((p, forced)) => image(setup, spu, &p, forced)?.into_iter().collect(),
            None => Vec::new(),
        };
        // A date earlier than the last is taken as the last.
        let last = out.last().map_or(0, |c| c.at);
        let at = s.ms.saturating_mul(1_000_000).max(last);
        out.push(Change { at, images });
        shown = now;
    }
    // A picture never stopped ends with its block, where the block says.
    if shown.is_some() {
        let last = out.last().map_or(0, |c| c.at);
        if let Some(end) = duration.filter(|&d| d > last) {
            out.push(Change {
                at: end,
                images: Vec::new(),
            });
        }
    }
    Some(out)
}

/// An SPU's control sequences, in order: each read until the end of its
/// commands, or one not known here; the last the one that names itself as
/// the next (or an earlier one, a damaged SPU's loop).
fn sequences(spu: &[u8]) -> Option<Vec<Sequence>> {
    let [_, _, c0, c1, ..] = *spu else {
        return None;
    };
    let mut at = usize::from(u16::from_be_bytes([c0, c1]));
    let mut picture = Picture::default();
    let mut out = Vec::new();
    for _ in 0..MAX_SEQUENCES {
        let [d0, d1, n0, n1, commands @ ..] = spu.get(at..)? else {
            return None;
        };
        let next = usize::from(u16::from_be_bytes([*n0, *n1]));
        let mut s = Sequence {
            ms: date_ms(u16::from_be_bytes([*d0, *d1])),
            start: false,
            forced: false,
            stop: false,
            picture,
        };
        let mut rest = commands;
        while let [command, tail @ ..] = rest {
            rest = tail;
            match *command {
                FORCED_START => (s.start, s.forced) = (true, true),
                START => s.start = true,
                STOP => s.stop = true,
                COLOURS | ALPHAS => {
                    let [a, b, tail @ ..] = rest else {
                        return None;
                    };
                    let four = [b & 0x0F, b >> 4, a & 0x0F, a >> 4];
                    if *command == COLOURS {
                        picture.colours = four;
                    } else {
                        picture.alphas = four;
                    }
                    rest = tail;
                }
                AREA => {
                    let [a, b, c, d, e, f, tail @ ..] = rest else {
                        return None;
                    };
                    picture.area = Some([
                        u16::from_be_bytes([*a, *b]) >> 4,
                        u16::from_be_bytes([*b & 0x0F, *c]),
                        u16::from_be_bytes([*d, *e]) >> 4,
                        u16::from_be_bytes([*e & 0x0F, *f]),
                    ]);
                    rest = tail;
                }
                FIELDS => {
                    let [a, b, c, d, tail @ ..] = rest else {
                        return None;
                    };
                    picture.fields =
                        Some([u16::from_be_bytes([*a, *b]), u16::from_be_bytes([*c, *d])]);
                    rest = tail;
                }
                // The end of the sequence (0xff), or a command not read
                // here -- FFmpeg reads no further either.
                _ => break,
            }
        }
        s.picture = picture;
        out.push(s);
        if next <= at {
            break;
        }
        at = next;
    }
    Some(out)
}

/// A date's milliseconds as FFmpeg counts them: `(date << 10) / 90`, whole.
fn date_ms(date: u16) -> i64 {
    i64::from(u32::from(date) * 1024 / 90)
}

/// The picture `p` sets, as FFmpeg draws it: its pixels in their colours,
/// the transparent rows and columns at its edges cut away unless it is
/// `forced`; `Some(None)` for one with nothing left, `None` for one whose
/// codes cannot fill it.
#[allow(
    clippy::option_option,
    reason = "a picture of nothing, and a picture that cannot be read, are two answers"
)]
fn image(setup: &Setup, spu: &[u8], p: &Picture, forced: bool) -> Option<Option<CueImage>> {
    let [left, right, top, bottom] = p.area?;
    let [even, odd] = p.fields?;
    let w = usize::from(right.checked_sub(left)?).checked_add(1)?;
    let h = usize::from(bottom.checked_sub(top)?).checked_add(1)?;
    let mut pixels = vec![0u8; w.checked_mul(h)?];
    // The even lines, then the odd.
    for (field, from) in [(0, even), (1, odd)] {
        let mut codes = Nibbles::at(spu, usize::from(from));
        for line in pixels.chunks_exact_mut(w).skip(field).step_by(2) {
            codes.line(line)?;
        }
    }
    let rgba = colours(setup, p);
    // A pixel shown -- every pixel of a forced picture, which FFmpeg keeps
    // whole, and of any other one that is not transparent.
    let kept = |&index: &u8| forced || rgba.get(usize::from(index)).is_some_and(|c| c[3] != 0);
    let rows: Vec<&[u8]> = pixels.chunks_exact(w).collect();
    // The rows and columns holding a pixel shown.
    let top_row = rows.iter().position(|r| r.iter().any(kept));
    let bottom_row = rows.iter().rposition(|r| r.iter().any(kept));
    let left_col = (0..w).find(|&c| rows.iter().any(|r| r.get(c).is_some_and(kept)));
    let right_col = (0..w)
        .rev()
        .find(|&c| rows.iter().any(|r| r.get(c).is_some_and(kept)));
    let (Some(t), Some(b), Some(l), Some(r)) = (top_row, bottom_row, left_col, right_col) else {
        return Some(None);
    };
    let mut out = Vec::new();
    for row in rows.get(t..=b)? {
        for &index in row.get(l..=r)? {
            out.extend_from_slice(rgba.get(usize::from(index))?);
        }
    }
    Some(Some(CueImage {
        canvas_width: setup.width,
        canvas_height: setup.height,
        x: u32::from(left).checked_add(u32::try_from(l).ok()?)?,
        y: u32::from(top).checked_add(u32::try_from(t).ok()?)?,
        width: u32::try_from(r.checked_sub(l)?.checked_add(1)?).ok()?,
        height: u32::try_from(b.checked_sub(t)?.checked_add(1)?).ok()?,
        rgba: out,
        forced,
    }))
}

/// The four colours as RGBA: from the palette, or, with none, FFmpeg's
/// greys -- each distinct palette index among the colours not transparent,
/// in the colours' order, the next level of a ramp as many steps long.
fn colours(setup: &Setup, p: &Picture) -> [[u8; 4]; 4] {
    let alpha = |i: usize| p.alphas.get(i).map_or(0, |&a| a.saturating_mul(17));
    let mut rgba = [[0u8; 4]; 4];
    if let Some(palette) = &setup.palette {
        for (i, slot) in rgba.iter_mut().enumerate() {
            let index = p.colours.get(i).map_or(0, |&c| usize::from(c & 0x0F));
            let [r, g, b] = palette.get(index).copied().unwrap_or_default();
            *slot = [r, g, b, alpha(i)];
        }
        return rgba;
    }
    let mut given: Vec<u8> = Vec::new();
    for (i, colour) in p.colours.iter().enumerate() {
        if alpha(i) != 0 && !given.contains(colour) {
            given.push(*colour);
        }
    }
    let ramp: &[u32] = match given.len() {
        1 => &[0xFF],
        2 => &[0x00, 0xFF],
        3 => &[0x00, 0x80, 0xFF],
        _ => &[0x00, 0x55, 0xAA, 0xFF],
    };
    for (i, slot) in rgba.iter_mut().enumerate() {
        if alpha(i) == 0 {
            continue;
        }
        let k = p
            .colours
            .get(i)
            .and_then(|c| given.iter().position(|g| g == c))
            .unwrap_or(0);
        let level = ramp.get(k).copied().unwrap_or(0xFF);
        let grey = u8::try_from(level.saturating_mul(255) / 256).unwrap_or(u8::MAX);
        *slot = [grey, grey, grey, alpha(i)];
    }
    rgba
}

/// An SPU's codes, a nibble at a time.
struct Nibbles<'a> {
    data: &'a [u8],
    /// In nibbles.
    at: usize,
}

impl<'a> Nibbles<'a> {
    fn at(data: &'a [u8], byte: usize) -> Self {
        Self {
            data,
            at: byte.saturating_mul(2),
        }
    }

    fn nibble(&mut self) -> Option<u8> {
        let byte = self.data.get(self.at / 2)?;
        let n = if self.at.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0x0F
        };
        self.at = self.at.saturating_add(1);
        Some(n)
    }

    /// One line of colours: runs of one to four nibbles, the count in the
    /// bits above the colour's two, a count of 0 filling the line; a run
    /// past the line's end is cut to it; the line ends on a byte.
    fn line(&mut self, line: &mut [u8]) -> Option<()> {
        let mut x = 0;
        while let Some(left) = line.len().checked_sub(x).filter(|&l| l > 0) {
            let mut v = u16::from(self.nibble()?);
            for floor in [0x4, 0x10, 0x40] {
                if v >= floor {
                    break;
                }
                v = (v << 4) | u16::from(self.nibble()?);
            }
            let colour = u8::try_from(v & 3).ok()?;
            let run = match usize::from(v >> 2) {
                0 => left,
                n => n.min(left),
            };
            line.get_mut(x..x.checked_add(run)?)?.fill(colour);
            x = x.checked_add(run)?;
        }
        self.at = self.at.saturating_add(self.at % 2);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "a test: a failure should be loud, and its numbers are small"
    )]

    use super::*;

    const IDX: &[u8] = b"# VobSub index file, v7 (do not modify this line!)\nsize: 720x480\npalette: 808080, ffffff, ff0000, 00ff00, 0000ff, ffff00, 808080, 00ffff, ff00ff, 800000, 008000, 000080, 808000, 800080, 008080, c0c0c0\n";

    fn setup() -> Setup {
        Setup::parse(IDX)
    }

    /// A field's codes from rows of colours, each run in the fewest nibbles.
    fn field(rows: &[Vec<u8>]) -> Vec<u8> {
        let mut nib: Vec<u8> = Vec::new();
        for row in rows {
            let mut i = 0;
            while i < row.len() {
                let c = row[i];
                let mut n = 1;
                while i + n < row.len() && row[i + n] == c && n < 255 {
                    n += 1;
                }
                let v = (n << 2) | usize::from(c);
                let count = match n {
                    1..=3 => 1,
                    4..=15 => 2,
                    16..=63 => 3,
                    _ => 4,
                };
                for k in (0..count).rev() {
                    nib.push(((v >> (4 * k)) & 0xF) as u8);
                }
                i += n;
            }
            if nib.len() % 2 == 1 {
                nib.push(0);
            }
        }
        nib.chunks(2).map(|p| (p[0] << 4) | p[1]).collect()
    }

    /// A command of a control sequence.
    enum Cmd {
        /// The colours, alphas, area and fields the SPU was given.
        Place,
        Start,
        Forced,
        Stop,
        Colours([u8; 4]),
        Alphas([u8; 4]),
    }

    /// Four nibbles as the colour and alpha commands give them: the fourth
    /// colour first.
    fn four(v: [u8; 4]) -> [u8; 2] {
        [(v[3] << 4) | v[2], (v[1] << 4) | v[0]]
    }

    /// An SPU of `rows` placed at `at`, its control `sequences` (date,
    /// commands), each ended.
    fn spu(
        rows: &[Vec<u8>],
        at: (u16, u16),
        colours: [u8; 4],
        alphas: [u8; 4],
        sequences: &[(u16, Vec<Cmd>)],
    ) -> Vec<u8> {
        let even: Vec<Vec<u8>> = rows.iter().step_by(2).cloned().collect();
        let odd: Vec<Vec<u8>> = rows.iter().skip(1).step_by(2).cloned().collect();
        let (e, o) = (field(&even), field(&odd));
        let data = [e.clone(), o].concat();
        let (w, h) = (rows[0].len() as u16, rows.len() as u16);
        let (x1, x2, y1, y2) = (at.0, at.0 + w - 1, at.1, at.1 + h - 1);
        let bytes = |c: &Cmd| -> Vec<u8> {
            match c {
                Cmd::Place => {
                    let mut b = vec![COLOURS];
                    b.extend(four(colours));
                    b.push(ALPHAS);
                    b.extend(four(alphas));
                    b.push(AREA);
                    b.extend([
                        (x1 >> 4) as u8,
                        (((x1 & 0xF) << 4) | (x2 >> 8)) as u8,
                        x2 as u8,
                        (y1 >> 4) as u8,
                        (((y1 & 0xF) << 4) | (y2 >> 8)) as u8,
                        y2 as u8,
                    ]);
                    b.push(FIELDS);
                    b.extend(4u16.to_be_bytes());
                    b.extend((4 + e.len() as u16).to_be_bytes());
                    b
                }
                Cmd::Start => vec![START],
                Cmd::Forced => vec![FORCED_START],
                Cmd::Stop => vec![STOP],
                Cmd::Colours(c) => [&[COLOURS][..], &four(*c)].concat(),
                Cmd::Alphas(a) => [&[ALPHAS][..], &four(*a)].concat(),
            }
        };
        let bodies: Vec<Vec<u8>> = sequences
            .iter()
            .map(|(_, cmds)| {
                let mut b: Vec<u8> = cmds.iter().flat_map(bytes).collect();
                b.push(0xFF);
                b
            })
            .collect();
        let first = 4 + data.len();
        let mut offsets = Vec::new();
        let mut end = first;
        for b in &bodies {
            offsets.push(end);
            end += 4 + b.len();
        }
        let mut out = Vec::new();
        out.extend((end as u16).to_be_bytes());
        out.extend((first as u16).to_be_bytes());
        out.extend(&data);
        for (k, ((date, _), body)) in sequences.iter().zip(&bodies).enumerate() {
            let next = *offsets.get(k + 1).unwrap_or(&offsets[k]);
            out.extend(date.to_be_bytes());
            out.extend((next as u16).to_be_bytes());
            out.extend(body);
        }
        out
    }

    /// A `w` by `h` box: an outline of colour 2 round a fill of 1.
    fn boxed(w: usize, h: usize) -> Vec<Vec<u8>> {
        (0..h)
            .map(|r| {
                (0..w)
                    .map(|c| u8::from(r == 0 || r == h - 1 || c == 0 || c == w - 1) + 1)
                    .collect()
            })
            .collect()
    }

    const SHOWN: [u8; 4] = [0, 15, 15, 15];

    fn shown_until(stop: u16) -> Vec<(u16, Vec<Cmd>)> {
        vec![(0, vec![Cmd::Place, Cmd::Start]), (stop, vec![Cmd::Stop])]
    }

    #[test]
    fn a_setup_gives_the_canvas_and_the_palette() {
        let s = setup();
        assert_eq!((s.width, s.height), (720, 480));
        assert_eq!(s.palette.unwrap()[2], [255, 0, 0]);
        // Neither line: FFmpeg's canvas, and no palette; custom colours
        // ignored.
        let s = Setup::parse(
            b"custom colors: ON, tridx: 1000, colors: 112233, 445566, 778899, aabbcc\n",
        );
        assert_eq!((s.width, s.height, s.palette), (720, 576, None));
        // A palette short of sixteen is none; a line ended CRLF reads.
        assert_eq!(Setup::parse(b"palette: 000000, ffffff\n").palette, None);
        assert_eq!(Setup::parse(b"size: 720x576\r\n").height, 576);
    }

    #[test]
    fn a_picture_shows_from_its_start_until_its_stop() {
        let s = spu(
            &boxed(20, 10),
            (100, 100),
            [0, 1, 2, 3],
            SHOWN,
            &shown_until(88),
        );
        let c = changes(&setup(), &s, None).unwrap();
        assert_eq!(c.len(), 2);
        // 88 * 1024 / 90 is 1001 milliseconds, whole.
        assert_eq!(
            c[1],
            Change {
                at: 1_001_000_000,
                images: Vec::new()
            }
        );
        let i = &c[0].images[0];
        assert_eq!(c[0].at, 0);
        assert_eq!(
            (i.x, i.y, i.width, i.height, i.forced),
            (100, 100, 20, 10, false)
        );
        assert_eq!((i.canvas_width, i.canvas_height), (720, 480));
        // The outline red (palette 2), the fill white (palette 1).
        assert_eq!(&i.rgba[..4], [255, 0, 0, 255]);
        assert_eq!(&i.rgba[21 * 4..22 * 4], [255, 255, 255, 255]);
    }

    #[test]
    fn the_run_length_codes_read_as_a_dvd_codes_them() {
        let line = |codes: &[u8], w: usize| {
            let mut out = vec![9u8; w];
            Nibbles::at(codes, 0).line(&mut out).map(|()| out)
        };
        // 0x5: one pixel of 1; 0x13: four of 3; 0x042: sixteen of 2;
        // 0x0101: 64 of 1.
        assert_eq!(line(&[0x51, 0x30], 5), Some(vec![1, 3, 3, 3, 3]));
        assert_eq!(line(&[0x04, 0x20], 16), Some(vec![2; 16]));
        assert_eq!(line(&[0x01, 0x01], 64), Some(vec![1; 64]));
        // 0x0003: the rest of the line, of 3.
        assert_eq!(
            line(&[0x50, 0x00, 0x30], 7),
            Some(vec![1, 3, 3, 3, 3, 3, 3])
        );
        // A run past the line's end is cut to it.
        assert_eq!(line(&[0x13], 2), Some(vec![3, 3]));
        // Codes cut off: no line.
        assert_eq!(line(&[0x13], 9), None);
        // Two lines: the first ends on a byte, so its odd nibble is skipped
        // (0xF); the second, a pixel of 2 (0x6) and one of 1 (0x5).
        let mut codes = Nibbles::at(&[0x5F, 0x65], 0);
        let mut a = [0u8; 1];
        codes.line(&mut a).unwrap();
        let mut b = [0u8; 2];
        codes.line(&mut b).unwrap();
        assert_eq!((a, b), ([1], [2, 1]));
    }

    #[test]
    fn transparent_edges_are_cut_and_a_transparent_inside_is_kept() {
        // A box of 1 in a two-pixel border of colour 0 -- grey, transparent
        // -- with a hole of 0 in it.
        let mut rows = vec![vec![0u8; 24]; 14];
        for (r, row) in rows.iter_mut().enumerate().take(12).skip(2) {
            for (c, px) in row.iter_mut().enumerate().take(22).skip(2) {
                *px = u8::from(!((6..8).contains(&r) && (10..14).contains(&c)));
            }
        }
        let s = spu(&rows, (100, 100), [0, 1, 2, 3], SHOWN, &shown_until(88));
        let i = &changes(&setup(), &s, None).unwrap()[0].images[0];
        assert_eq!((i.x, i.y, i.width, i.height), (102, 102, 20, 10));
        // The hole, (12, 6) in the picture, (10, 4) in the image: grey, alpha 0.
        let at = (4 * 20 + 10) * 4;
        assert_eq!(&i.rgba[at..at + 4], [128, 128, 128, 0]);
        // Forced, the picture is kept whole, its transparent edges grey.
        let s = spu(
            &rows,
            (100, 100),
            [0, 1, 2, 3],
            SHOWN,
            &[(0, vec![Cmd::Place, Cmd::Forced])],
        );
        let i = &changes(&setup(), &s, None).unwrap()[0].images[0];
        assert_eq!((i.x, i.y, i.width, i.height), (100, 100, 24, 14));
        assert_eq!(&i.rgba[..4], [128, 128, 128, 0]);
    }

    #[test]
    fn each_sequence_takes_effect_at_its_date() {
        // Colours changed at 44, a fade at 66, stopped at 88, started again
        // at 132, stopped at 176.
        let s = spu(
            &boxed(8, 4),
            (10, 10),
            [0, 1, 2, 3],
            SHOWN,
            &[
                (0, vec![Cmd::Place, Cmd::Start]),
                (44, vec![Cmd::Colours([0, 5, 6, 7])]),
                (66, vec![Cmd::Alphas([0, 8, 8, 8])]),
                (88, vec![Cmd::Stop]),
                (132, vec![Cmd::Start]),
                (176, vec![Cmd::Stop]),
            ],
        );
        let c = changes(&setup(), &s, None).unwrap();
        let at: Vec<i64> = c.iter().map(|c| c.at / 1_000_000).collect();
        assert_eq!(at, [0, 500, 750, 1001, 1501, 2002]);
        // The fill: white, yellow (palette 5), yellow at alpha 136; then the
        // faded picture again.
        let fill = |k: usize| c[k].images[0].rgba[9 * 4..10 * 4].to_vec();
        assert_eq!(fill(0), [255, 255, 255, 255]);
        assert_eq!(fill(1), [255, 255, 0, 255]);
        assert_eq!(fill(2), [255, 255, 0, 136]);
        assert!(c[3].images.is_empty() && c[5].images.is_empty());
        assert_eq!(fill(4), [255, 255, 0, 136]);
    }

    #[test]
    fn forced_no_start_and_no_stop_are_ffmpegs() {
        let rows = boxed(8, 4);
        // A forced start.
        let s = spu(
            &rows,
            (0, 0),
            [0, 1, 2, 3],
            SHOWN,
            &[(0, vec![Cmd::Place, Cmd::Forced])],
        );
        assert!(changes(&setup(), &s, None).unwrap()[0].images[0].forced);
        // No start anywhere: shown from the block's time, though placed at
        // 22; no stop: the block's duration ends it.
        let s = spu(
            &rows,
            (0, 0),
            [0, 1, 2, 3],
            SHOWN,
            &[(22, vec![Cmd::Place])],
        );
        let c = changes(&setup(), &s, Some(2_000_000_000)).unwrap();
        assert_eq!(
            c.iter().map(|c| c.at).collect::<Vec<_>>(),
            [0, 2_000_000_000]
        );
        // No duration either: shown until something replaces it.
        assert_eq!(changes(&setup(), &s, None).unwrap().len(), 1);
    }

    /// The four colours `colours` and `alphas` make in `setup`.
    fn colours_of(setup: &Setup, colours: [u8; 4], alphas: [u8; 4]) -> [[u8; 4]; 4] {
        super::colours(
            setup,
            &Picture {
                colours,
                alphas,
                area: None,
                fields: None,
            },
        )
    }

    #[test]
    fn without_a_palette_the_greys_are_ffmpegs() {
        let s = Setup::parse(b"size: 720x480\n");
        let greys =
            |colours: [u8; 4], alphas: [u8; 4]| colours_of(&s, colours, alphas).map(|c| c[0]);
        assert_eq!(greys([0, 1, 2, 3], [15, 0, 0, 0]), [254, 0, 0, 0]);
        assert_eq!(greys([0, 1, 2, 3], [0, 15, 0, 15]), [0, 0, 0, 254]);
        assert_eq!(greys([0, 1, 2, 3], [0, 15, 12, 8]), [0, 0, 127, 254]);
        assert_eq!(greys([0, 1, 2, 3], [15; 4]), [0, 84, 169, 254]);
        // One palette index in three colours: one grey.
        assert_eq!(greys([5, 5, 7, 5], [15; 4]), [0, 0, 254, 0]);
        // Alpha as with a palette.
        assert_eq!(colours_of(&s, [0, 1, 2, 3], [0, 15, 12, 8])[3][3], 136);
    }

    #[test]
    fn damage_changes_nothing_and_a_transparent_picture_clears() {
        let rows = boxed(8, 4);
        // The odd lines' codes past the data.
        let mut s = spu(&rows, (0, 0), [0, 1, 2, 3], SHOWN, &shown_until(88));
        let f = s.iter().rposition(|&b| b == FIELDS).unwrap();
        s[f + 3..f + 5].copy_from_slice(&0x7FF0u16.to_be_bytes());
        assert_eq!(changes(&setup(), &s, None), None);
        // An area taller than the codes.
        let mut s = spu(&rows, (0, 0), [0, 1, 2, 3], SHOWN, &shown_until(88));
        let a = s.iter().rposition(|&b| b == AREA).unwrap();
        s[a + 6] = 30;
        assert_eq!(changes(&setup(), &s, None), None);
        // Every colour transparent: a clear, then the stop's.
        let s = spu(&rows, (0, 0), [0, 1, 2, 3], [0; 4], &shown_until(88));
        let c = changes(&setup(), &s, None).unwrap();
        assert!(c.iter().all(|c| c.images.is_empty()));
        assert_eq!(c[0].at, 0);
        // A sequence naming an earlier one as the next ends the reading.
        let mut s = spu(&rows, (0, 0), [0, 1, 2, 3], SHOWN, &shown_until(88));
        let first = usize::from(u16::from_be_bytes([s[2], s[3]]));
        s[first + 2..first + 4].copy_from_slice(&4u16.to_be_bytes());
        assert_eq!(changes(&setup(), &s, None).unwrap().len(), 1);
        // Too short to be an SPU.
        assert_eq!(changes(&setup(), &[0, 4], None), None);
        // Dates going back are taken as the last: a stop dated before the
        // colour change it follows comes with it.
        let s = spu(
            &rows,
            (0, 0),
            [0, 1, 2, 3],
            SHOWN,
            &[
                (0, vec![Cmd::Place, Cmd::Start]),
                (88, vec![Cmd::Colours([0, 5, 6, 7])]),
                (44, vec![Cmd::Stop]),
            ],
        );
        let at: Vec<i64> = changes(&setup(), &s, None)
            .unwrap()
            .iter()
            .map(|c| c.at / 1_000_000)
            .collect();
        assert_eq!(at, [0, 1001, 1001]);
    }
}
