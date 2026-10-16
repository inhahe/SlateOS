//! A window: `WINDOW` and its lines, with the operations on one that need
//! nothing of the screen -- `lib_newwin.c`'s `_nc_makenew`, `lib_move.c`,
//! `lib_clreol.c`, `lib_clrbot.c`, `lib_erase.c`, `lib_clear.c`,
//! `lib_touch.c`, `lib_scroll.c`, `wresize.c`, `lib_winch.c`,
//! `lib_in_wch.c` and the attribute calls of `lib_wattron.c`,
//! `lib_wattroff.c` and the generated `lib_gen.c`.
//!
//! Positions are `NCURSES_SIZE_T`, a `short`, as the C library's are: a
//! window too large for one is refused ([`Window::makenew`]), and every
//! position C stores back through a cast to `short` is stored here through
//! [`short`].

use crate::cell::{A_COLOR, ALL_BUT_COLOR, Attr, Cell, pair_number};

/// `NCURSES_SIZE_T`.
pub type Size = i16;

/// `_NOCHANGE`: a line, or its first or last changed column, untouched.
pub const NOCHANGE: Size = -1;
/// `_NEWINDEX`: a line made by an insertion or a scroll.
pub const NEWINDEX: i32 = -1;

/// `_SUBWIN`.
pub const SUBWIN: i16 = 0x01;
/// `_ENDLINE`: the window is flush right.
pub const ENDLINE: i16 = 0x02;
/// `_FULLWIN`: the window is the whole screen.
pub const FULLWIN: i16 = 0x04;
/// `_SCROLLWIN`: the window's bottom is the screen's.
pub const SCROLLWIN: i16 = 0x08;
/// `_ISPAD`.
pub const ISPAD: i16 = 0x10;
/// `_HASMOVED`: the cursor moved since the last refresh.
pub const HASMOVED: i16 = 0x20;
/// `_WRAPPED`: the cursor just wrapped.
pub const WRAPPED: i16 = 0x40;

/// `(NCURSES_SIZE_T) v`: the low sixteen bits of an `int`, as C's cast
/// keeps them.
#[must_use]
pub fn short(v: i32) -> Size {
    let [a, b, _, _] = v.to_le_bytes();
    Size::from_le_bytes([a, b])
}

/// `MB_LEN_MAX` in glibc, which sizes a window's multibyte work area.
pub const MB_LEN_MAX: usize = 16;

/// `struct ldat`: one line of a window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// `text`: the cells.
    pub text: Vec<Cell>,
    /// `firstchar`: the first changed column, or [`NOCHANGE`].
    pub firstchar: Size,
    /// `lastchar`: the last changed column, or [`NOCHANGE`].
    pub lastchar: Size,
    /// `oldindex`: kept for the scroll hints this build has not; always the
    /// line's own number or [`NEWINDEX`] where upstream would set it.
    pub oldindex: Size,
}

impl Line {
    /// `CHANGED_CELL (line, col)`.
    pub fn changed_cell(&mut self, col: i32) {
        if self.firstchar == NOCHANGE {
            self.firstchar = short(col);
            self.lastchar = short(col);
        } else if col < i32::from(self.firstchar) {
            self.firstchar = short(col);
        } else if col > i32::from(self.lastchar) {
            self.lastchar = short(col);
        }
    }

    /// `CHANGED_RANGE (line, start, end)`.
    pub fn changed_range(&mut self, start: i32, end: i32) {
        if self.firstchar == NOCHANGE || i32::from(self.firstchar) > start {
            self.firstchar = short(start);
        }
        if self.lastchar == NOCHANGE || i32::from(self.lastchar) < end {
            self.lastchar = short(end);
        }
    }

    /// `CHANGED_TO_EOL (line, start, end)`.
    pub fn changed_to_eol(&mut self, start: i32, end: i32) {
        if self.firstchar == NOCHANGE || i32::from(self.firstchar) > start {
            self.firstchar = short(start);
        }
        self.lastchar = short(end);
    }

    /// The cell at `col`; a blank past the end, which upstream would read
    /// out of bounds.
    #[must_use]
    pub fn at(&self, col: i32) -> Cell {
        usize::try_from(col)
            .ok()
            .and_then(|c| self.text.get(c))
            .copied()
            .unwrap_or_default()
    }

    /// The cell at `col`, to change; `None` out of bounds.
    pub fn at_mut(&mut self, col: i32) -> Option<&mut Cell> {
        usize::try_from(col).ok().and_then(|c| self.text.get_mut(c))
    }
}

/// `WINDOW`, with what upstream keeps beside it in its `WINDOWLIST` entry:
/// the bytes of a multibyte character being added one byte at a time.
#[derive(Clone, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's WINDOW, field for field"
)]
pub struct Window {
    /// `_cury`, `_curx`: the cursor.
    pub cury: Size,
    pub curx: Size,
    /// `_maxy`, `_maxx`: the last row and column, not the size.
    pub maxy: Size,
    pub maxx: Size,
    /// `_begy`, `_begx`: where the window is on the screen.
    pub begy: Size,
    pub begx: Size,
    /// `_flags`.
    pub flags: i16,
    /// `_attrs`: the attributes added characters get.
    pub attrs: Attr,
    /// `_color`: the colour pair they get, whole (extended colours).
    pub color: i32,
    /// `_bkgrnd`: the background cell.
    pub bkgd: Cell,
    /// `_clear`: repaint from scratch at the next refresh.
    pub clear: bool,
    /// `_leaveok`.
    pub leaveok: bool,
    /// `_scroll`.
    pub scroll: bool,
    /// `_idlok`.
    pub idlok: bool,
    /// `_idcok`.
    pub idcok: bool,
    /// `_immed`.
    pub immed: bool,
    /// `_sync`.
    pub sync: bool,
    /// `_use_keypad`.
    pub use_keypad: bool,
    /// `_delay`.
    pub delay: i32,
    /// `_line`.
    pub lines: Vec<Line>,
    /// `_regtop`, `_regbottom`: the scrolling region.
    pub regtop: Size,
    pub regbottom: Size,
    /// `_yoffset`: rows taken off the top of the screen.
    pub yoffset: Size,
    /// `addch_work`, `addch_used`, `addch_x`, `addch_y`: a multibyte
    /// character's bytes so far, and where they are going.
    pub addch_work: [u8; MB_LEN_MAX * 9 + 1],
    pub addch_used: usize,
    pub addch_x: i32,
    pub addch_y: i32,
}

/// `dimension_limit (value)`: a size that fits `NCURSES_SIZE_T` and is
/// above 0.
fn dimension_limit(value: i32) -> bool {
    i32::from(short(value)) == value && value > 0
}

impl Window {
    /// `_nc_makenew (lines, columns, begy, begx, flags)` on a screen of
    /// `screen_lines` by `screen_columns` with `topstolen` rows taken off
    /// its top: a window whose every line is marked changed, its text not
    /// yet made. `None` for a size `NCURSES_SIZE_T` cannot hold.
    #[must_use]
    pub fn makenew(
        num_lines: i32,
        num_columns: i32,
        begy: i32,
        begx: i32,
        flags: i16,
        screen: (i32, i32),
        topstolen: Size,
    ) -> Option<Self> {
        let (screen_lines, screen_columns) = screen;
        if !dimension_limit(num_lines) || !dimension_limit(num_columns) {
            return None;
        }
        let is_padwin = flags & ISPAD != 0;
        let rows = usize::try_from(num_lines).ok()?;
        let mut win = Self {
            cury: 0,
            curx: 0,
            maxy: short(num_lines.wrapping_sub(1)),
            maxx: short(num_columns.wrapping_sub(1)),
            begy: short(begy),
            begx: short(begx),
            flags,
            attrs: 0,
            color: 0,
            bkgd: Cell::blank(),
            clear: !is_padwin && num_lines == screen_lines && num_columns == screen_columns,
            leaveok: false,
            scroll: false,
            idlok: false,
            idcok: true,
            immed: false,
            sync: false,
            use_keypad: false,
            delay: -1,
            lines: Vec::with_capacity(rows),
            regtop: 0,
            regbottom: short(num_lines.wrapping_sub(1)),
            yoffset: topstolen,
            addch_work: [0; MB_LEN_MAX * 9 + 1],
            addch_used: 0,
            addch_x: 0,
            addch_y: 0,
        };
        // "SVr4 curses marks the whole new window changed."
        for _ in 0..rows {
            win.lines.push(Line {
                text: Vec::new(),
                firstchar: 0,
                lastchar: short(num_columns.wrapping_sub(1)),
                oldindex: 0,
            });
        }
        if !is_padwin && begx.wrapping_add(num_columns) == screen_columns {
            win.flags |= ENDLINE;
            if begx == 0 && num_lines == screen_lines && begy == 0 {
                win.flags |= FULLWIN;
            }
            if begy.wrapping_add(num_lines) == screen_lines {
                win.flags |= SCROLLWIN;
            }
        }
        Some(win)
    }

    /// `newwin`'s part after `_nc_makenew`: every line filled with blanks.
    /// Sizes of 0 are the caller's to have resolved.
    #[must_use]
    pub fn newwin(
        num_lines: i32,
        num_columns: i32,
        begy: i32,
        begx: i32,
        screen: (i32, i32),
        topstolen: Size,
    ) -> Option<Self> {
        if begy < 0 || begx < 0 || num_lines < 0 || num_columns < 0 {
            return None;
        }
        let mut win = Self::makenew(num_lines, num_columns, begy, begx, 0, screen, topstolen)?;
        let cols = usize::try_from(num_columns).ok()?;
        for line in &mut win.lines {
            line.text = vec![Cell::blank(); cols];
        }
        Some(win)
    }

    /// The line `y`; `None` outside the window.
    #[must_use]
    pub fn line(&self, y: i32) -> Option<&Line> {
        usize::try_from(y).ok().and_then(|y| self.lines.get(y))
    }

    /// The line `y`, to change.
    pub fn line_mut(&mut self, y: i32) -> Option<&mut Line> {
        usize::try_from(y).ok().and_then(|y| self.lines.get_mut(y))
    }

    /// The cell at `(y, x)`; a blank outside the window.
    #[must_use]
    pub fn cell(&self, y: i32, x: i32) -> Cell {
        self.line(y).map(|l| l.at(x)).unwrap_or_default()
    }

    /// `IS_WRAPPED (w)`.
    #[must_use]
    pub const fn is_wrapped(&self) -> bool {
        self.flags & WRAPPED != 0
    }

    /// `GET_WINDOW_PAIR (w)`: the window's pair, from `_color` or else its
    /// attributes.
    #[must_use]
    pub const fn pair(&self) -> i32 {
        if self.color != 0 {
            self.color
        } else {
            pair_number(self.attrs)
        }
    }

    /// `LEGALYX (w, y, x)`.
    #[must_use]
    pub fn legal_yx(&self, y: i32, x: i32) -> bool {
        x >= 0 && x <= i32::from(self.maxx) && y >= 0 && y <= i32::from(self.maxy)
    }

    /// `wmove (win, y, x)`: `false` (`ERR`) outside the window.
    pub fn wmove(&mut self, y: i32, x: i32) -> bool {
        if !self.legal_yx(y, x) {
            return false;
        }
        self.curx = short(x);
        self.cury = short(y);
        self.flags &= !WRAPPED;
        self.flags |= HASMOVED;
        true
    }

    /// `wclrtoeol (win)`: the rest of the cursor's line made the background.
    pub fn wclrtoeol(&mut self) -> bool {
        let y = self.cury;
        let x = self.curx;
        // "If we have just wrapped the cursor, the clear applies to the new
        // line, unless we are at the lower right corner."
        if self.is_wrapped() && y < self.maxy {
            self.flags &= !WRAPPED;
        }
        if self.is_wrapped() || y > self.maxy || x > self.maxx {
            return false;
        }
        let blank = self.bkgd;
        let maxx = i32::from(self.maxx);
        let Some(line) = self.line_mut(i32::from(y)) else {
            return false;
        };
        line.changed_to_eol(i32::from(x), maxx);
        for col in i32::from(x)..=maxx {
            if let Some(c) = line.at_mut(col) {
                *c = blank;
            }
        }
        true
    }

    /// `wclrtobot (win)`: from the cursor to the window's end made the
    /// background.
    pub fn wclrtobot(&mut self) -> bool {
        let mut startx = i32::from(self.curx);
        let blank = self.bkgd;
        let maxx = i32::from(self.maxx);
        for y in i32::from(self.cury)..=i32::from(self.maxy) {
            if let Some(line) = self.line_mut(y) {
                line.changed_to_eol(startx, maxx);
                for col in startx..=maxx {
                    if let Some(c) = line.at_mut(col) {
                        *c = blank;
                    }
                }
            }
            startx = 0;
        }
        true
    }

    /// `werase (win)`: every cell the background, the cursor home. (A
    /// derived window's reach into its parent's wide characters does not
    /// arise: this crate makes no derived windows.)
    pub fn werase(&mut self) -> bool {
        let blank = self.bkgd;
        let maxx = self.maxx;
        for line in &mut self.lines {
            line.text.fill(blank);
            line.firstchar = 0;
            line.lastchar = maxx;
        }
        self.curx = 0;
        self.cury = 0;
        self.flags &= !WRAPPED;
        true
    }

    /// `wclear (win)`: [`Window::werase`], and a repaint from scratch at the
    /// next refresh.
    pub fn wclear(&mut self) -> bool {
        let ok = self.werase();
        if ok {
            self.clear = true;
        }
        ok
    }

    /// `wtouchln (win, y, n, changed)`.
    pub fn wtouchln(&mut self, y: i32, n: i32, changed: bool) -> bool {
        if n < 0 || y < 0 || y > i32::from(self.maxy) {
            return false;
        }
        let maxx = self.maxx;
        let maxy = i32::from(self.maxy);
        let mut i = y;
        while i < y.saturating_add(n) {
            if i > maxy {
                break;
            }
            if let Some(line) = self.line_mut(i) {
                line.firstchar = if changed { 0 } else { NOCHANGE };
                line.lastchar = if changed { maxx } else { NOCHANGE };
            }
            i = i.saturating_add(1);
        }
        true
    }

    /// `touchline (win, top, count)`.
    pub fn touchline(&mut self, top: i32, count: i32) -> bool {
        self.wtouchln(top, count, true)
    }

    /// `winch (win)`: the cell under the cursor as a `chtype`.
    #[must_use]
    pub fn winch(&self) -> Attr {
        let c = self.cell(i32::from(self.cury), i32::from(self.curx));
        c.ch().cast_unsigned() | c.attr
    }

    /// `win_wch (win, wcval)`: the cell under the cursor.
    #[must_use]
    pub fn win_wch(&self) -> Cell {
        self.cell(i32::from(self.cury), i32::from(self.curx))
    }

    /// `wattr_on (win, at, NULL)`.
    pub fn wattr_on(&mut self, at: Attr) {
        if at & A_COLOR != 0 {
            self.color = pair_number(at);
        }
        // `toggle_attr_on`.
        if pair_number(at) > 0 {
            self.attrs = (self.attrs & ALL_BUT_COLOR) | at;
        } else {
            self.attrs |= at;
        }
    }

    /// `wattr_off (win, at, NULL)`.
    pub fn wattr_off(&mut self, at: Attr) {
        if at & A_COLOR != 0 {
            self.color = 0;
        }
        // `toggle_attr_off`.
        if pair_number(at) > 0 {
            self.attrs &= !(at | A_COLOR);
        } else {
            self.attrs &= !at;
        }
    }

    /// `wattrset (win, at)`: the pair `at` carries, and `at` itself.
    pub fn wattrset(&mut self, at: Attr) {
        self.color = pair_number(at);
        self.attrs = at;
    }

    /// `wattr_set (win, a, pair, NULL)`.
    pub fn wattr_set(&mut self, a: Attr, pair: i16) {
        self.attrs = a & !A_COLOR;
        self.color = i32::from(pair);
    }

    /// `wstandout (win)`: `wattrset (win, A_STANDOUT)`, which clears every
    /// other attribute and the pair.
    pub fn wstandout(&mut self) {
        self.wattrset(crate::cell::A_STANDOUT);
    }

    /// `wstandend (win)`: `wattrset (win, A_NORMAL)`.
    pub fn wstandend(&mut self) {
        self.wattrset(crate::cell::A_NORMAL);
    }

    /// `_nc_scroll_window (win, n, top, bottom, blank)`: rows `top` to
    /// `bottom` moved `n` up (or `-n` down), the rows uncovered `blank`,
    /// and all of them marked changed.
    pub fn scroll_window(&mut self, n: i32, top: i32, bottom: i32, blank: Cell) {
        let maxy = i32::from(self.maxy);
        if top < 0 || bottom < top || bottom > maxy {
            return;
        }
        let bottom_limit = |line: i32| line >= 0 && line >= top;
        let top_limit = |line: i32| line <= maxy && line <= bottom;
        if n < 0 {
            let limit = top.saturating_sub(n);
            let mut line = bottom;
            while line >= limit && bottom_limit(line) {
                self.copy_line(line.saturating_add(n), line);
                line = line.saturating_sub(1);
            }
            let mut line = top;
            while line < limit && top_limit(line) {
                self.fill_line(line, blank);
                line = line.saturating_add(1);
            }
        }
        if n > 0 {
            let limit = bottom.saturating_sub(n);
            let mut line = top;
            while line <= limit && top_limit(line) {
                self.copy_line(line.saturating_add(n), line);
                line = line.saturating_add(1);
            }
            let mut line = bottom;
            while line > limit && bottom_limit(line) {
                self.fill_line(line, blank);
                line = line.saturating_sub(1);
            }
        }
        self.touchline(top, bottom.saturating_sub(top).saturating_add(1));
        if self.addch_used != 0 {
            let next = self.addch_y.saturating_add(n);
            if next < 0 || next > maxy {
                self.addch_y = 0;
            } else {
                self.addch_y = next;
            }
        }
    }

    /// `memcpy` of line `from`'s text over line `to`'s.
    fn copy_line(&mut self, from: i32, to: i32) {
        let Some(text) = self.line(from).map(|l| l.text.clone()) else {
            return;
        };
        if let Some(line) = self.line_mut(to) {
            for (dst, src) in line.text.iter_mut().zip(text) {
                *dst = src;
            }
        }
    }

    /// Line `y`'s every cell `blank`.
    fn fill_line(&mut self, y: i32, blank: Cell) {
        if let Some(line) = self.line_mut(y) {
            line.text.fill(blank);
        }
    }

    /// `wscrl (win, n)`: the scrolling region moved `n` lines, if the window
    /// may scroll.
    pub fn wscrl(&mut self, n: i32) -> bool {
        if !self.scroll {
            return false;
        }
        if n != 0 {
            let (top, bottom, blank) =
                (i32::from(self.regtop), i32::from(self.regbottom), self.bkgd);
            self.scroll_window(n, top, bottom, blank);
        }
        true
    }

    /// `wresize (win, to_lines, to_cols)` for a window that is no
    /// sub-window: rows and columns added as the background and marked
    /// changed, those taken away dropped, the cursor and the scrolling
    /// region kept inside.
    pub fn wresize(&mut self, to_lines: i32, to_cols: i32) -> bool {
        let to_lines = to_lines.wrapping_sub(1);
        let to_cols = to_cols.wrapping_sub(1);
        if to_lines < 0 || to_cols < 0 {
            return false;
        }
        let size_x = i32::from(self.maxx);
        let size_y = i32::from(self.maxy);
        if to_lines == size_y && to_cols == size_x {
            return true;
        }
        let Ok(rows) = usize::try_from(to_lines.saturating_add(1)) else {
            return false;
        };
        let Ok(cols) = usize::try_from(to_cols.saturating_add(1)) else {
            return false;
        };
        let mut new_lines: Vec<Line> = Vec::with_capacity(rows);
        for row in 0..=to_lines {
            let begin = if row > size_y {
                0
            } else {
                size_x.saturating_add(1)
            };
            let end = to_cols;
            let text = if row <= size_y {
                if to_cols == size_x {
                    self.line(row).map(|l| l.text.clone()).unwrap_or_default()
                } else {
                    let mut s = Vec::with_capacity(cols);
                    for col in 0..=to_cols {
                        let mut valid = col <= size_x;
                        if col == to_cols && col < size_x && self.cell(row, col).is_widec_base() {
                            valid = false;
                        }
                        s.push(if valid {
                            self.cell(row, col)
                        } else {
                            self.bkgd
                        });
                    }
                    s
                }
            } else {
                vec![self.bkgd; cols]
            };
            // `typeCalloc`: a new line's marks start at 0.
            let mut line = Line {
                text,
                firstchar: 0,
                lastchar: 0,
                oldindex: 0,
            };
            if row <= size_y
                && let Some(old) = self.line(row)
            {
                line.firstchar = old.firstchar;
                line.lastchar = old.lastchar;
            }
            if to_cols != size_x || row > size_y {
                if end >= begin {
                    // Growing.
                    if i32::from(line.firstchar) < begin {
                        line.firstchar = short(begin);
                    }
                } else {
                    // Shrinking.
                    line.firstchar = 0;
                }
                line.lastchar = short(to_cols);
            }
            new_lines.push(line);
        }
        self.lines = new_lines;
        self.maxx = short(to_cols);
        self.maxy = short(to_lines);
        if self.regtop > self.maxy {
            self.regtop = self.maxy;
        }
        if self.regbottom > self.maxy || i32::from(self.regbottom) == size_y {
            self.regbottom = self.maxy;
        }
        if self.curx > self.maxx {
            self.curx = self.maxx;
        }
        if self.cury > self.maxy {
            self.cury = self.maxy;
        }
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::cell::{A_BOLD, A_REVERSE, A_STANDOUT, color_pair};

    fn win(lines: i32, cols: i32) -> Window {
        Window::newwin(lines, cols, 0, 0, (lines, cols), 0).unwrap()
    }

    #[test]
    fn a_new_window_is_blank_and_wholly_changed() {
        let w = win(3, 5);
        assert_eq!((w.maxy, w.maxx), (2, 4));
        assert!(w.clear, "a full-screen window starts clear");
        assert_eq!(w.flags, ENDLINE | FULLWIN | SCROLLWIN);
        for line in &w.lines {
            assert_eq!((line.firstchar, line.lastchar), (0, 4));
            assert!(line.text.iter().all(Cell::is_blank));
        }
        // Smaller than the screen: no `_clear`, and only what it reaches.
        let small = Window::newwin(2, 5, 1, 0, (3, 5), 0).unwrap();
        assert!(!small.clear);
        assert_eq!(small.flags, ENDLINE | SCROLLWIN);
    }

    #[test]
    fn a_size_a_short_cannot_hold_is_refused() {
        assert!(Window::makenew(40000, 80, 0, 0, 0, (40000, 80), 0).is_none());
        assert!(Window::makenew(0, 80, 0, 0, 0, (24, 80), 0).is_none());
        assert!(Window::makenew(32767, 1, 0, 0, 0, (24, 80), 0).is_some());
        assert_eq!(short(40000), -25536);
    }

    #[test]
    fn moving_is_checked_and_clears_the_wrap() {
        let mut w = win(3, 5);
        w.flags |= WRAPPED;
        assert!(w.wmove(2, 4));
        assert_eq!((w.cury, w.curx), (2, 4));
        assert_eq!(w.flags & (WRAPPED | HASMOVED), HASMOVED);
        assert!(!w.wmove(3, 0));
        assert!(!w.wmove(0, -1));
    }

    #[test]
    fn clearing_to_the_end_marks_what_it_cleared() {
        let mut w = win(3, 5);
        for line in &mut w.lines {
            line.firstchar = NOCHANGE;
            line.lastchar = NOCHANGE;
            line.text.fill(Cell::new2(0x78, 0));
        }
        w.wmove(1, 2);
        assert!(w.wclrtoeol());
        assert_eq!((w.lines[1].firstchar, w.lines[1].lastchar), (2, 4));
        assert_eq!(w.lines[1].text[1].ch(), 0x78);
        assert!(w.lines[1].text[2..].iter().all(Cell::is_blank));
        assert_eq!(w.lines[0].firstchar, NOCHANGE);
        w.wmove(1, 3);
        assert!(w.wclrtobot());
        assert_eq!((w.lines[2].firstchar, w.lines[2].lastchar), (0, 4));
        assert!(w.lines[2].text.iter().all(Cell::is_blank));
    }

    #[test]
    fn attributes_follow_the_toggle_macros() {
        let mut w = win(1, 1);
        w.wattr_on(A_BOLD);
        w.wattr_on(color_pair(5));
        w.wattr_on(A_REVERSE | color_pair(3));
        assert_eq!(
            w.attrs,
            A_BOLD | A_REVERSE | color_pair(3),
            "a pair replaces the pair, and the rest stays"
        );
        assert_eq!(w.pair(), 3);
        w.wattr_off(A_REVERSE);
        assert_eq!(w.attrs, A_BOLD | color_pair(3));
        w.wattr_off(color_pair(3));
        assert_eq!((w.attrs, w.color), (A_BOLD, 0));
        w.wattrset(A_BOLD | color_pair(2));
        w.wstandout();
        assert_eq!((w.attrs, w.color), (A_STANDOUT, 0));
        w.wstandend();
        assert_eq!(w.attrs, 0);
    }

    #[test]
    fn scrolling_moves_rows_and_blanks_what_it_uncovers() {
        let mut w = win(4, 2);
        for (i, line) in w.lines.iter_mut().enumerate() {
            line.text
                .fill(Cell::new2(0x30 + i32::try_from(i).unwrap(), 0));
        }
        w.scroll_window(1, 0, 3, Cell::blank());
        let rows: Vec<i32> = w.lines.iter().map(|l| l.text[0].ch()).collect();
        assert_eq!(rows, [0x31, 0x32, 0x33, 0x20]);
        w.scroll_window(-2, 1, 3, Cell::blank());
        let rows: Vec<i32> = w.lines.iter().map(|l| l.text[0].ch()).collect();
        assert_eq!(rows, [0x31, 0x20, 0x20, 0x32]);
        assert!(!w.wscrl(1), "a window may not scroll unless told");
    }

    #[test]
    fn resizing_keeps_what_fits_and_fills_the_rest() {
        let mut w = win(2, 3);
        for line in &mut w.lines {
            line.firstchar = NOCHANGE;
            line.lastchar = NOCHANGE;
            line.text.fill(Cell::new2(0x78, 0));
        }
        w.wmove(1, 2);
        assert!(w.wresize(3, 5));
        assert_eq!((w.maxy, w.maxx), (2, 4));
        assert_eq!(w.lines[0].text[2].ch(), 0x78);
        assert!(w.lines[0].text[3].is_blank());
        // Grown columns are marked from the first new one; a new row whole.
        assert_eq!((w.lines[0].firstchar, w.lines[0].lastchar), (3, 4));
        assert_eq!((w.lines[2].firstchar, w.lines[2].lastchar), (0, 4));
        assert!(w.wresize(1, 2));
        assert_eq!((w.cury, w.curx), (0, 1), "the cursor is kept inside");
        assert_eq!((w.lines[0].firstchar, w.lines[0].lastchar), (0, 1));
        assert!(!w.wresize(0, 2));
    }
}
