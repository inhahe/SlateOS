//! CEA-608 closed captions -- television's captions, as QuickTime and
//! broadcast recorders keep them in MP4 (`c608`) -- decoded into what the
//! screen shows: a cue for each stretch it shows the same.
//!
//! **What a sample holds.** Atoms: `cdat`, field 1's byte pairs (data
//! channels CC1 and CC2), and `cdt2`, field 2's (CC3, CC4 and XDS). This
//! reads field 1's first channel, CC1 -- the captions a television shows
//! unless told otherwise -- and passes the rest over. (FFmpeg reads every
//! byte of a sample after the first atom's header as field 1's pairs, a
//! `cdt2` atom's header and pairs among them.)
//!
//! **Whose rules.** Neither FFmpeg's decoder nor CCExtractor is right
//! throughout -- FFmpeg ignores Backspace and Erase Non-Displayed Memory and
//! mixes CC2 into CC1, CCExtractor ignores parity -- so this follows the
//! rules a decoder is held to: 47 CFR
//! 79.101, the FCC's, for every command -- a character failing parity a
//! solid block, a control code repeated in the very next pair ignored as
//! its redundant copy (and not when pairs apart), Backspace erasing one
//! cell, Delete to End of Row from the cursor, a PAC moving the cursor and
//! erasing nothing (moving roll-up's window, intact, when it names another
//! base row), mid-row codes and Flash On a space each, a colour turning
//! italics off, Roll-Up erasing a pop-on or paint-on caption, the
//! thirty-second column overwritten when a row is full, another channel's
//! commands and everything after them not shown -- and CTA-608-E's
//! extended characters as McPoodle's SCC tools document them, by Unicode
//! name. Each fixture is checked against the external reader that is right
//! about what it holds (`tests/data/generate_subtitle_fixtures.py`).
//!
//! **What a cue is.** The screen as one stretch shows it: from a command
//! that changes it -- End of Caption, Erase Displayed Memory, a roll-up
//! Carriage Return, a Roll-Up command erasing a caption, a PAC moving
//! roll-up's window -- or from the first character on a blank screen, to
//! the next such command, or to typing that leaves the screen blank; its
//! text the screen's at the stretch's end. A command that leaves the screen
//! as it was ends nothing (captioners send Roll-Up before every line). So a
//! roll-up line is shown whole from the Carriage Return before it, as
//! FFmpeg shows it -- though not before its first character, where FFmpeg
//! shows it from the Carriage Return with the screen still blank.
//!
//! **How it is said.** SRT markup, as every format here: each row showing
//! anything, top to bottom, a line; an empty cell before or between
//! characters a no-break space, so that a row keeps its column on the
//! caption grid; the whole in `<font face="Monospace">`, the grid's face,
//! as FFmpeg writes it; colour, italics and underline as tags; and `{\anN}`
//! at the left of the third of the picture the rows' middle falls in.
//!
//! **What it costs.** A pair is one step, and the screen is said again only
//! when a pair wrote to it: a sample's cost is its length.

use super::srt::{Op, Toggle, Writer, escape};

const ROWS: usize = 15;
const COLS: usize = 32;

/// A character's look: its colour (an index into [`COLOURS`]), italics and
/// underline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Style {
    colour: u8,
    italic: bool,
    underline: bool,
}

/// White, upright, not underlined: a row's look until a code says
/// otherwise.
const WHITE: Style = Style {
    colour: 0,
    italic: false,
    underline: false,
};

/// The seven colours a PAC or mid-row code gives, `0xRRGGBB`: white, green,
/// blue, cyan, red, yellow, magenta.
const COLOURS: [u32; 7] = [
    0xFF_FF_FF, 0x00_FF_00, 0x00_00_FF, 0x00_FF_FF, 0xFF_00_00, 0xFF_FF_00, 0xFF_00_FF,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cell {
    ch: char,
    style: Style,
}

type Row = [Option<Cell>; COLS];
type Memory = [Row; ROWS];

const BLANK_ROW: Row = [None; COLS];
const BLANK: Memory = [BLANK_ROW; ROWS];

/// The standard characters that are not ASCII's: 0x2A, 0x5C, 0x5E, 0x5F,
/// 0x60, 0x7B to 0x7F.
fn standard(c: u8) -> char {
    match c {
        0x2A => '\u{e1}',
        0x5C => '\u{e9}',
        0x5E => '\u{ed}',
        0x5F => '\u{f3}',
        0x60 => '\u{fa}',
        0x7B => '\u{e7}',
        0x7C => '\u{f7}',
        0x7D => '\u{d1}',
        0x7E => '\u{f1}',
        0x7F => '\u{2588}',
        c => char::from(c),
    }
}

/// 0x11 0x30 to 0x3F. 0x39 is the transparent space: a cell showing no
/// character and no background, said as a no-break space.
const SPECIAL: [char; 16] = [
    '\u{ae}', '\u{b0}', '\u{bd}', '\u{bf}', '\u{2122}', '\u{a2}', '\u{a3}', '\u{266a}', '\u{e0}',
    '\u{a0}', '\u{e8}', '\u{e2}', '\u{ea}', '\u{ee}', '\u{f4}', '\u{fb}',
];

/// 0x12 and 0x13, 0x20 to 0x3F: Spanish, miscellaneous and French;
/// Portuguese, German and Danish. 0x12 0x2D is drawn in McPoodle's table as
/// a bullet and named "middle dot": the bullet, as CCExtractor reads it.
const EXTENDED: [[char; 32]; 2] = [
    [
        '\u{c1}', '\u{c9}', '\u{d3}', '\u{da}', '\u{dc}', '\u{fc}', '\u{2018}', '\u{a1}', '*',
        '\u{2019}', '\u{2014}', '\u{a9}', '\u{2120}', '\u{2022}', '\u{201c}', '\u{201d}', '\u{c0}',
        '\u{c2}', '\u{c7}', '\u{c8}', '\u{ca}', '\u{cb}', '\u{eb}', '\u{ce}', '\u{cf}', '\u{ef}',
        '\u{d4}', '\u{d9}', '\u{f9}', '\u{db}', '\u{ab}', '\u{bb}',
    ],
    [
        '\u{c3}', '\u{e3}', '\u{cd}', '\u{cc}', '\u{ec}', '\u{d2}', '\u{f2}', '\u{d5}', '\u{f5}',
        '{', '}', '\\', '^', '_', '\u{a6}', '~', '\u{c4}', '\u{e4}', '\u{d6}', '\u{f6}', '\u{df}',
        '\u{a5}', '\u{a4}', '|', '\u{c5}', '\u{e5}', '\u{d8}', '\u{f8}', '\u{250c}', '\u{2510}',
        '\u{2514}', '\u{2518}',
    ],
];

/// A PAC's row (0 the top), by its first byte on channel 1 and its second
/// byte's 0x20 bit.
fn pac_row(c1: u8, c2: u8) -> Option<usize> {
    let row: usize = match (c1, c2 & 0x20 != 0) {
        (0x11, false) => 1,
        (0x11, true) => 2,
        (0x12, false) => 3,
        (0x12, true) => 4,
        (0x15, false) => 5,
        (0x15, true) => 6,
        (0x16, false) => 7,
        (0x16, true) => 8,
        (0x17, false) => 9,
        (0x17, true) => 10,
        (0x10, false) => 11,
        (0x13, false) => 12,
        (0x13, true) => 13,
        (0x14, false) => 14,
        (0x14, true) => 15,
        _ => return None,
    };
    row.checked_sub(1)
}

/// Whether a byte has odd parity, as every byte sent has.
fn odd(b: u8) -> bool {
    b.count_ones() % 2 == 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// No captioning command yet: characters have nowhere to go.
    Unset,
    PopOn,
    PaintOn,
    RollUp,
    /// Text mode: its characters are not captions.
    Text,
}

/// What a stretch of the screen showed: from `start` to `end`, nanoseconds,
/// as SRT markup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    pub start: i64,
    pub end: i64,
    pub text: String,
}

/// The screen as it shows: each row showing anything, top to bottom, as
/// runs of one look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Screen(Vec<(usize, Vec<(String, Style)>)>);

impl Screen {
    fn of(memory: &Memory) -> Self {
        let mut rows = Vec::new();
        for (r, row) in memory.iter().enumerate() {
            let Some(last) = row.iter().rposition(Option::is_some) else {
                continue;
            };
            let mut runs: Vec<(String, Style)> = Vec::new();
            for cell in row.iter().take(last.saturating_add(1)) {
                let (ch, style) = cell.map_or(('\u{a0}', WHITE), |c| (c.ch, c.style));
                match runs.last_mut() {
                    Some((text, s)) if *s == style => text.push(ch),
                    _ => runs.push((ch.to_string(), style)),
                }
            }
            rows.push((r, runs));
        }
        Self(rows)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// As SRT markup: at the left of the third of the picture the rows'
    /// middle falls in, in the grid's face.
    fn markup(&self) -> String {
        let mut w = Writer::new();
        let first = self.0.first().map_or(0, |(r, _)| *r);
        let last = self.0.last().map_or(0, |(r, _)| *r);
        // The middle row, 0 the top, in the first five rows, the next five,
        // or the last: twice it against 10 and 20.
        let twice = first.saturating_add(last);
        w.op(Op::Align(if twice < 10 {
            7
        } else if twice < 20 {
            4
        } else {
            1
        }));
        w.op(Op::Face(Some("Monospace".to_owned())));
        let mut now = WHITE;
        for (i, (_, runs)) in self.0.iter().enumerate() {
            if i > 0 {
                w.op(Op::Break);
            }
            for (text, style) in runs {
                // What turns off first, then the colour, then what turns on:
                // a colour opened inside a tag about to close would close
                // with it and open again.
                let toggles = [
                    (Toggle::Italic, now.italic, style.italic),
                    (Toggle::Underline, now.underline, style.underline),
                ];
                for (toggle, _, _) in toggles.iter().filter(|(_, was, is)| *was && !*is) {
                    w.op(Op::Toggle(*toggle, false));
                }
                if style.colour != now.colour {
                    let rgb = COLOURS.get(usize::from(style.colour)).copied();
                    w.op(Op::Colour(rgb.filter(|_| style.colour != 0)));
                }
                for (toggle, _, _) in toggles.iter().filter(|(_, was, is)| !*was && *is) {
                    w.op(Op::Toggle(*toggle, true));
                }
                now = *style;
                w.op(Op::Text(escape(text, true)));
            }
        }
        w.finish()
    }
}

/// CC1 of field 1, decoded pair by pair.
#[derive(Debug)]
pub(crate) struct Decoder {
    displayed: Box<Memory>,
    hidden: Box<Memory>,
    mode: Mode,
    /// Roll-up's window, 2 to 4 rows.
    roll: usize,
    /// The cursor: its row (0 the top; roll-up's base row) and column.
    row: usize,
    col: usize,
    /// The look the next character takes.
    style: Style,
    /// Whether the last control code was channel 1's: what follows another
    /// channel's is that channel's.
    ours: bool,
    /// The pair before, if it was a control code: the same again is its
    /// redundant copy.
    last: Option<[u8; 2]>,
    /// When the stretch the screen shows began.
    since: Option<i64>,
    /// The screen as the stretch shows it.
    screen: Screen,
    /// Whether the pair being read wrote to the screen.
    touched: bool,
}

impl Default for Decoder {
    fn default() -> Self {
        Self {
            displayed: Box::new(BLANK),
            hidden: Box::new(BLANK),
            mode: Mode::Unset,
            roll: 2,
            row: ROWS.saturating_sub(1),
            col: 0,
            style: WHITE,
            ours: true,
            last: None,
            since: None,
            screen: Screen::default(),
            touched: false,
        }
    }
}

impl Decoder {
    /// A sample's pairs at `t`: its `cdat` atoms' (field 1), in order; its
    /// other atoms passed over. `None` for a sample whose atoms do not fit
    /// it.
    pub(crate) fn pairs(sample: &[u8]) -> Option<Vec<[u8; 2]>> {
        let mut out = Vec::new();
        let mut rest = sample;
        while !rest.is_empty() {
            let size = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?);
            let kind = rest.get(4..8)?;
            let size = usize::try_from(size).ok()?;
            if size < 8 || size > rest.len() {
                return None;
            }
            if kind == b"cdat" {
                // A byte past the last pair is no pair, as FFmpeg counts
                // them.
                out.extend(
                    rest.get(8..size)?
                        .chunks_exact(2)
                        .filter_map(|p| <[u8; 2]>::try_from(p).ok()),
                );
            }
            rest = rest.get(size..)?;
        }
        Some(out)
    }

    /// A pair at `t` (nanoseconds); the stretch it ended, if it ended one.
    pub(crate) fn pair(&mut self, [b1, b2]: [u8; 2], t: i64) -> Option<Shown> {
        self.touched = false;
        let flushed = self.apply(b1, b2);
        if !self.touched {
            return None;
        }
        let after = Screen::of(&self.displayed);
        if after == self.screen {
            return None;
        }
        let before = core::mem::replace(&mut self.screen, after);
        if flushed || self.screen.is_empty() {
            // A command changing the screen, or typing leaving it blank: the
            // stretch so far ends.
            let ended = self.since.take().filter(|_| !before.is_empty());
            if !self.screen.is_empty() {
                self.since = Some(t);
            }
            return ended.map(|start| Shown {
                start,
                end: t,
                text: before.markup(),
            });
        }
        if self.since.is_none() {
            self.since = Some(t);
        }
        None
    }

    /// The track's end at `t`: what the screen still shows, until then.
    pub(crate) fn finish(&mut self, t: i64) -> Option<Shown> {
        let start = self.since.take()?;
        (!self.screen.is_empty()).then(|| Shown {
            start,
            end: t,
            text: self.screen.markup(),
        })
    }

    /// The pair's effect; whether it was a command that ends a stretch.
    fn apply(&mut self, b1: u8, b2: u8) -> bool {
        if [b1, b2] == [0x80, 0x80] {
            self.last = None;
            return false;
        }
        let (c1, c2) = (b1 & 0x7F, b2 & 0x7F);
        if (0x10..=0x1F).contains(&c1) {
            // A control code, or a character of two bytes: each byte must
            // pass parity, and the same pair in the very next frame is the
            // redundant copy.
            if !odd(b1) || !odd(b2) || self.last == Some([b1, b2]) {
                self.last = None;
                return false;
            }
            self.last = Some([b1, b2]);
            self.ours = c1 < 0x18;
            if !self.ours
                || (self.mode == Mode::Text && !(c1 == 0x14 && (0x20..=0x2F).contains(&c2)))
            {
                return false;
            }
            return self.control(c1, c2);
        }
        self.last = None;
        if !self.ours || matches!(self.mode, Mode::Unset | Mode::Text) {
            return false;
        }
        for b in [b1, b2] {
            let c = b & 0x7F;
            if c == 0 {
                continue;
            }
            if !odd(b) {
                self.put('\u{2588}');
            } else if c >= 0x20 {
                self.put(standard(c));
            }
        }
        false
    }

    /// The memory characters go to: the one not shown while loading a
    /// pop-on caption, else the screen.
    fn target(&mut self) -> &mut Memory {
        if self.mode == Mode::PopOn {
            &mut self.hidden
        } else {
            self.touched = true;
            &mut self.displayed
        }
    }

    fn put(&mut self, ch: char) {
        let (row, col, style) = (self.row, self.col, self.style);
        if let Some(cell) = self.target().get_mut(row).and_then(|r| r.get_mut(col)) {
            *cell = Some(Cell { ch, style });
        }
        if self.col < COLS.saturating_sub(1) {
            self.col = self.col.saturating_add(1);
        }
    }

    fn control(&mut self, c1: u8, c2: u8) -> bool {
        match (c1, c2) {
            (0x11, 0x20..=0x2F) => {
                // Mid-row: a space, then the new look. A colour turns italics
                // off; italics keeps the colour.
                let code = (c2 >> 1) & 7;
                let underline = c2 & 1 != 0;
                self.style = if code == 7 {
                    Style {
                        italic: true,
                        underline,
                        ..self.style
                    }
                } else {
                    Style {
                        colour: code,
                        italic: false,
                        underline,
                    }
                };
                self.put(' ');
                false
            }
            (0x11, 0x30..=0x3F) => {
                if let Some(&ch) = SPECIAL.get(usize::from(c2 & 0x0F)) {
                    self.put(ch);
                }
                false
            }
            (0x12 | 0x13, 0x20..=0x3F) => {
                // An extended character takes the place of the one before
                // it, sent for a decoder without extended characters.
                self.col = self.col.saturating_sub(1);
                let table = EXTENDED.get(usize::from(c1 & 1));
                if let Some(&ch) = table.and_then(|t| t.get(usize::from(c2 & 0x1F))) {
                    self.put(ch);
                }
                false
            }
            (_, 0x40..=0x7F) => self.pac(c1, c2),
            (0x17, 0x21..=0x23) => {
                // A tab offset: the cells passed over as they are.
                self.col = self
                    .col
                    .saturating_add(usize::from(c2 & 0x03))
                    .min(COLS.saturating_sub(1));
                false
            }
            (0x14, 0x20..=0x2F) => self.command(c2),
            _ => false,
        }
    }

    /// A PAC; whether it moved roll-up's window, which ends a stretch.
    fn pac(&mut self, c1: u8, c2: u8) -> bool {
        let Some(row) = pac_row(c1, c2) else {
            return false;
        };
        let moved = self.mode == Mode::RollUp && row != self.row;
        if moved {
            // The window moves, intact, to its new base row: its rows, top
            // to bottom, to the rows ending at the new one -- any with no
            // room above the screen's top gone.
            let window = self.window();
            let rows: Vec<Row> = window
                .clone()
                .map(|r| self.displayed.get(r).copied().unwrap_or(BLANK_ROW))
                .collect();
            for r in window {
                if let Some(slot) = self.displayed.get_mut(r) {
                    *slot = BLANK_ROW;
                }
            }
            let count = rows.len();
            for (i, content) in rows.into_iter().enumerate() {
                let up = count.saturating_sub(1).saturating_sub(i);
                if let Some(slot) = row.checked_sub(up).and_then(|r| self.displayed.get_mut(r)) {
                    *slot = content;
                }
            }
            self.touched = true;
        }
        self.row = row;
        let code = (c2 >> 1) & 0x0F;
        let underline = c2 & 1 != 0;
        if code & 0x08 != 0 {
            self.col = usize::from(code & 7).saturating_mul(4);
            self.style = Style { underline, ..WHITE };
        } else {
            self.col = 0;
            self.style = if code == 7 {
                Style {
                    colour: 0,
                    italic: true,
                    underline,
                }
            } else {
                Style {
                    colour: code,
                    italic: false,
                    underline,
                }
            };
        }
        moved
    }

    /// Roll-up's window: the base row and the rows above it, as many as it
    /// has, the screen's top its limit.
    fn window(&self) -> core::ops::RangeInclusive<usize> {
        self.row.saturating_add(1).saturating_sub(self.roll)..=self.row
    }

    fn command(&mut self, c2: u8) -> bool {
        match c2 {
            0x20 => self.mode = Mode::PopOn,
            0x21 => {
                // Backspace: one cell back, erased; nothing in the first
                // column.
                if self.col > 0 {
                    self.col = self.col.saturating_sub(1);
                    let (row, col) = (self.row, self.col);
                    if let Some(cell) = self.target().get_mut(row).and_then(|r| r.get_mut(col)) {
                        *cell = None;
                    }
                }
            }
            0x24 => {
                // Delete to End of Row: from the cursor on.
                let (row, col) = (self.row, self.col);
                if let Some(cells) = self.target().get_mut(row).and_then(|r| r.get_mut(col..)) {
                    cells.fill(None);
                }
            }
            0x25..=0x27 => {
                // Roll-Up, 2 to 4 rows: a pop-on or paint-on caption erased;
                // already rolling, the window's height changed, the rows
                // above it gone.
                let rows = usize::from(c2.saturating_sub(0x23));
                if self.mode != Mode::RollUp {
                    *self.displayed = BLANK;
                    *self.hidden = BLANK;
                    self.row = ROWS.saturating_sub(1);
                    self.col = 0;
                    self.style = WHITE;
                }
                self.mode = Mode::RollUp;
                self.roll = rows;
                let above = self.row.saturating_add(1).saturating_sub(rows);
                if let Some(gone) = self.displayed.get_mut(..above) {
                    gone.fill(BLANK_ROW);
                }
                self.touched = true;
                return true;
            }
            // Flash On: a space, flashing -- said as a space.
            0x28 => self.put(' '),
            0x29 => self.mode = Mode::PaintOn,
            0x2A | 0x2B => self.mode = Mode::Text,
            0x2C => {
                *self.displayed = BLANK;
                self.touched = true;
                return true;
            }
            0x2D if self.mode == Mode::RollUp => {
                // Carriage Return: the window's rows up one, the top one
                // off it, the base row blank.
                let window = self.window();
                let (top, base) = (*window.start(), *window.end());
                if let Some(rows) = self.displayed.get_mut(top..=base) {
                    rows.rotate_left(1);
                    if let Some(last) = rows.last_mut() {
                        *last = BLANK_ROW;
                    }
                }
                self.col = 0;
                self.style = WHITE;
                self.touched = true;
                return true;
            }
            0x2E => *self.hidden = BLANK,
            0x2F => {
                // End of Caption: the memory loaded shown, the one shown
                // kept out of sight.
                core::mem::swap(&mut self.displayed, &mut self.hidden);
                self.mode = Mode::PopOn;
                self.touched = true;
                return true;
            }
            _ => {}
        }
        false
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_wrap,
        reason = "a test: a failure should be loud, and its sizes are small"
    )]

    use super::*;

    /// A byte with odd parity, as every byte is sent.
    fn p(b: u8) -> u8 {
        if b.count_ones().is_multiple_of(2) {
            b | 0x80
        } else {
            b
        }
    }

    /// Pairs as a captioner's encoder sends them: a control code twice.
    #[derive(Default)]
    struct Script(Vec<[u8; 2]>);

    impl Script {
        fn code(mut self, c1: u8, c2: u8) -> Self {
            self.0.push([p(c1), p(c2)]);
            self.0.push([p(c1), p(c2)]);
            self
        }
        fn once(mut self, c1: u8, c2: u8) -> Self {
            self.0.push([p(c1), p(c2)]);
            self
        }
        fn text(mut self, s: &str) -> Self {
            for chunk in s.as_bytes().chunks(2) {
                self.0
                    .push([p(chunk[0]), p(chunk.get(1).copied().unwrap_or(0))]);
            }
            self
        }
        fn wait(mut self, n: usize) -> Self {
            self.0.extend(std::iter::repeat_n([0x80, 0x80], n));
            self
        }
        fn rcl(self) -> Self {
            self.code(0x14, 0x20)
        }
        fn eoc(self) -> Self {
            self.code(0x14, 0x2F)
        }
        fn edm(self) -> Self {
            self.code(0x14, 0x2C)
        }
        fn cr(self) -> Self {
            self.code(0x14, 0x2D)
        }
        fn ru(self, n: u8) -> Self {
            self.code(0x14, 0x23 + n)
        }
        fn rdc(self) -> Self {
            self.code(0x14, 0x29)
        }
        /// A PAC: row 15, column 0, white.
        fn pac15(self) -> Self {
            self.code(0x14, 0x60)
        }
        /// A PAC: row 14, column 0, white.
        fn pac14(self) -> Self {
            self.code(0x14, 0x40)
        }
    }

    /// Every stretch `script` shows, its pairs a frame apart (1000 ns).
    fn shown(script: Script) -> Vec<Shown> {
        let mut d = Decoder::default();
        let mut out = Vec::new();
        let n = script.0.len();
        for (i, pair) in script.0.into_iter().enumerate() {
            out.extend(d.pair(pair, i as i64 * 1000));
        }
        out.extend(d.finish(n as i64 * 1000));
        out
    }

    /// The text alone: tags, the placement and the word joiners escaping
    /// text from markup gone.
    fn plain(markup: &str) -> String {
        let mut out = String::new();
        let mut tag = false;
        let mut chars = markup.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '<' if chars.peek() != Some(&'\u{2060}') => tag = true,
                '>' if tag => tag = false,
                '{' if chars.peek() == Some(&'\\') => {
                    for d in chars.by_ref() {
                        if d == '}' {
                            break;
                        }
                    }
                }
                '\u{2060}' => {}
                c if !tag => out.push(c),
                _ => {}
            }
        }
        out
    }

    fn texts(script: Script) -> Vec<(i64, i64, String)> {
        shown(script)
            .into_iter()
            .map(|s| (s.start, s.end, plain(&s.text)))
            .collect()
    }

    #[test]
    fn a_pop_on_caption_shows_from_end_of_caption_to_the_erase() {
        // RCL at 0, PAC at 2, text at 4 to 6, End of Caption at 7, the
        // erase at 19.
        let s = Script::default()
            .rcl()
            .pac15()
            .text("pop on")
            .eoc()
            .wait(10)
            .edm();
        assert_eq!(texts(s), [(7000, 19000, "pop on".to_owned())]);
    }

    #[test]
    fn a_caption_replaced_ends_where_the_next_begins() {
        let s = Script::default()
            .rcl()
            .pac15()
            .text("one")
            .eoc()
            .wait(5)
            .rcl()
            .code(0x14, 0x2E)
            .pac15()
            .text("two")
            .eoc()
            .wait(5)
            .edm();
        let t = texts(s);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].1, t[1].0);
        assert_eq!((t[0].2.as_str(), t[1].2.as_str()), ("one", "two"));
    }

    #[test]
    fn the_same_caption_shown_again_ends_nothing() {
        let s = Script::default()
            .rcl()
            .pac15()
            .text("same")
            .eoc()
            .wait(5)
            .rcl()
            .code(0x14, 0x2E)
            .pac15()
            .text("same")
            .eoc()
            .wait(5)
            .edm();
        assert_eq!(texts(s).len(), 1);
    }

    #[test]
    fn an_extended_character_takes_the_place_of_the_one_before() {
        let s = Script::default()
            .rcl()
            .pac15()
            .text("Ae")
            .code(0x12, 0x20)
            .text("x")
            .eoc()
            .wait(2)
            .edm();
        assert_eq!(texts(s)[0].2, "A\u{c1}x");
    }

    #[test]
    fn a_character_failing_parity_is_a_solid_block() {
        let mut s = Script::default().rcl().pac15().text("a");
        s.0.push([0x41, p(0x42)]);
        let s = s.eoc().wait(2).edm();
        assert_eq!(texts(s)[0].2, "a\u{2588}B");
    }

    #[test]
    fn a_control_code_failing_parity_is_ignored() {
        let mut s = Script::default().rcl().pac15().text("a");
        s.0.push([0x14, p(0x2F)]);
        let s = s.wait(2).edm();
        assert!(
            texts(s).is_empty(),
            "the End of Caption failing parity shows nothing"
        );
    }

    #[test]
    fn a_control_code_is_redundant_only_in_the_very_next_pair() {
        // Two Backspaces, a pair apart: both erase.
        let s = Script::default()
            .rcl()
            .pac15()
            .text("abcd")
            .once(0x14, 0x21)
            .wait(1)
            .once(0x14, 0x21)
            .eoc()
            .wait(2)
            .edm();
        assert_eq!(texts(s)[0].2, "ab");
        // Two in a row: the second is the first's copy.
        let s = Script::default()
            .rcl()
            .pac15()
            .text("abcd")
            .once(0x14, 0x21)
            .once(0x14, 0x21)
            .eoc()
            .wait(2)
            .edm();
        assert_eq!(texts(s)[0].2, "abc");
    }

    #[test]
    fn another_channel_and_text_mode_show_nothing() {
        // In the middle of loading channel 1's caption: channel 2's commands
        // and characters, then text mode's characters, none of them into it.
        let s = Script::default()
            .rcl()
            .pac15()
            .text("ours")
            .code(0x1C, 0x20)
            .code(0x1C, 0x60)
            .text("channel two")
            .code(0x1C, 0x2F)
            .code(0x14, 0x2A)
            .text("text mode")
            .rcl()
            .eoc()
            .wait(2)
            .edm();
        let t = texts(s);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].2, "ours");
    }

    #[test]
    fn backspace_in_the_first_column_is_nothing_and_a_full_row_overwrites_its_last() {
        // Back at the first column of a row with text in it: a Backspace
        // there erases nothing.
        let s = Script::default()
            .rcl()
            .pac15()
            .text("x")
            .pac15()
            .code(0x14, 0x21)
            .pac14()
            .text("0123456789012345678901234567890123")
            .eoc()
            .wait(2)
            .edm();
        assert_eq!(texts(s)[0].2, "01234567890123456789012345678903\nx");
    }

    #[test]
    fn mid_row_codes_are_spaces_and_a_colour_turns_italics_off() {
        let s = Script::default()
            .rcl()
            .pac15()
            .text("a")
            .code(0x11, 0x2E)
            .text("b")
            .code(0x11, 0x28)
            .text("c")
            .eoc()
            .wait(2)
            .edm();
        let markup = shown(s).remove(0).text;
        assert_eq!(plain(&markup), "a b c");
        assert!(markup.contains("<i> b</i>"), "{markup}");
        assert!(
            markup.contains("<font color=\"#ff0000\"> c</font>"),
            "{markup}"
        );
    }

    #[test]
    fn roll_up_rolls_its_window_and_a_line_shows_from_its_first_character() {
        // The line rolled off the top is longer than the one typed after
        // it: nothing of it comes round to the base row.
        let s = Script::default()
            .ru(2)
            .cr()
            .pac15()
            .text("first line")
            .wait(4)
            .cr()
            .text("two")
            .wait(4)
            .cr()
            .text("3")
            .wait(4)
            .edm();
        let t = texts(s);
        let lines: Vec<&str> = t.iter().map(|c| c.2.as_str()).collect();
        assert_eq!(lines, ["first line", "first line\ntwo", "two\n3"]);
        // The first line from its first character (pair 6), not the
        // Carriage Return before it (pair 2).
        assert_eq!(t[0].0, 6000);
    }

    #[test]
    fn roll_up_sent_again_before_each_line_ends_nothing() {
        let s = Script::default()
            .ru(2)
            .cr()
            .pac15()
            .text("one")
            .wait(4)
            .ru(2)
            .cr()
            .pac15()
            .text("two")
            .wait(4)
            .edm();
        assert_eq!(texts(s).len(), 2);
    }

    #[test]
    fn roll_up_erases_a_pop_on_caption() {
        let s = Script::default()
            .rcl()
            .pac15()
            .text("pop")
            .eoc()
            .wait(4)
            .ru(2)
            .wait(4);
        let t = texts(s);
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].0, t[0].1), (6000, 12000));
    }

    #[test]
    fn a_pac_naming_another_base_row_moves_the_window_intact() {
        let s = Script::default()
            .ru(2)
            .cr()
            .pac15()
            .text("low")
            .wait(4)
            .code(0x17, 0x40)
            .wait(4)
            .edm();
        let shown = shown(s);
        assert_eq!(shown.len(), 2);
        assert!(shown[0].text.starts_with("{\\an1}"), "{}", shown[0].text);
        assert!(shown[1].text.starts_with("{\\an4}"), "{}", shown[1].text);
        assert_eq!(plain(&shown[1].text), "low");
    }

    /// Typing that leaves the screen blank ends the stretch: what is typed
    /// after begins one of its own.
    #[test]
    fn typing_that_leaves_the_screen_blank_ends_a_stretch() {
        let s = Script::default()
            .rdc()
            .pac15()
            .text("ab")
            .wait(2)
            .once(0x14, 0x21)
            .wait(1)
            .once(0x14, 0x21)
            .wait(4)
            .text("cd")
            .wait(2)
            .edm();
        // RDC 0, PAC 2, "ab" 4, the Backspaces 7 and 9, "cd" 14, the erase
        // 17.
        assert_eq!(
            texts(s),
            [
                (4000, 9000, "a".to_owned()),
                (14000, 17000, "cd".to_owned())
            ]
        );
    }

    #[test]
    fn paint_on_shows_from_its_first_character_to_the_erase() {
        let s = Script::default()
            .wait(3)
            .rdc()
            .pac15()
            .text("pai")
            .wait(3)
            .text("nt")
            .wait(3)
            .edm();
        // RDC at 3, the PAC at 5, "pai" from 7, "nt" at 12, the erase at 16.
        assert_eq!(texts(s), [(7000, 16000, "paint".to_owned())]);
    }

    #[test]
    fn a_row_keeps_its_column_and_the_caption_its_third() {
        // Row 1 at column 8, then row 3 at column 0: the top third.
        let s = Script::default()
            .rcl()
            .code(0x11, 0x54)
            .text("top")
            .code(0x12, 0x40)
            .text("x")
            .eoc()
            .wait(2)
            .edm();
        let markup = shown(s).remove(0).text;
        let indent = "\u{a0}".repeat(8);
        assert_eq!(
            markup,
            format!("{{\\an7}}<font face=\"Monospace\">{indent}top\nx</font>")
        );
    }

    #[test]
    fn a_samples_pairs_are_its_field_1_atoms() {
        let atom = |kind: &[u8; 4], body: &[u8]| {
            let mut out = u32::try_from(body.len() + 8)
                .unwrap()
                .to_be_bytes()
                .to_vec();
            out.extend_from_slice(kind);
            out.extend_from_slice(body);
            out
        };
        let sample = [
            atom(b"cdat", &[1, 2, 3, 4, 5]),
            atom(b"cdt2", &[9, 9]),
            atom(b"cdat", &[6, 7]),
        ]
        .concat();
        assert_eq!(Decoder::pairs(&sample), Some(vec![[1, 2], [3, 4], [6, 7]]));
        assert_eq!(Decoder::pairs(&[]), Some(Vec::new()));
        assert_eq!(Decoder::pairs(&sample[..sample.len() - 1]), None);
        assert_eq!(Decoder::pairs(&[0, 0, 0, 4, b'c', b'd', b'a', b't']), None);
    }
}
