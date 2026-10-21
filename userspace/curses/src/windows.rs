//! Windows by handle: `stdscr` and the sub-windows made inside it --
//! `lib_newwin.c`'s `derwin` and `subwin`, `lib_delwin.c`, `lib_touch.c`,
//! `lib_erase.c` and `lib_clear.c` for a sub-window, `lib_refresh.c`'s
//! `wnoutrefresh` and `wrefresh` of any window, `lib_box.c`'s `wborder`,
//! `wresize.c`, and `resizeterm.c`'s walk over every window when the screen
//! changes size.
//!
//! # A sub-window shares its parent's cells
//!
//! Upstream's sub-window has no text of its own: each of its lines points
//! into its parent's, so what is drawn in one is drawn in the other, while
//! each keeps its own cursor, attributes and record of which columns
//! changed. Here a sub-window's lines hold a copy of the cells it covers,
//! made from `stdscr`, at the root of every family, just before anything is
//! done to the sub-window ([`Screen::with_win`]) and written back just after
//! -- within one call, under the screen's lock, so nothing can see the two
//! disagree. What each window keeps for itself is exactly upstream's: the
//! cursor, the attributes, the first and last changed column of each line.
//! Drawing in a sub-window marks nothing changed in its parent, as
//! upstream's does not without `syncok`; `touchwin` on the parent, or a
//! refresh of the sub-window, is what puts the drawing on the screen.
//!
//! # The window list
//!
//! Upstream keeps a screen's windows in one list, the newest first, and
//! resizing visits them in that order, a generation at a time. Here that
//! list is [`Screen::subwins`], the newest first, then `stdscr`, `curscr`
//! and `newscr`, the order `_nc_setupscreen` leaves them in.
//!
//! There are no windows but these: no `newwin` and no pads, which nothing
//! that uses this crate makes.

use core::sync::atomic::{AtomicBool, Ordering};

use crate::addch::{AddCtx, render_char};
use crate::cell::{A_ALTCHARSET, Attr, Cell};
use crate::screen::{Screen, adjust_window, wnoutrefresh_into};
use crate::term::ACS_LEN;
use crate::window::{ISPAD, NOCHANGE, SUBWIN, Size, Window, short};

/// A window: `stdscr`, or a sub-window by the number it was made with. A
/// handle to a sub-window since deleted names nothing, and every call on it
/// fails: numbers are never given out twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Win {
    /// `stdscr`.
    Stdscr,
    /// A sub-window.
    Sub(u64),
}

/// A sub-window, and where it is in its parent.
#[derive(Clone, Debug)]
pub struct SubWin {
    /// The number [`Win::Sub`] names it by.
    pub id: u64,
    /// The window. Its lines' cells are a copy of the ones it covers in
    /// `stdscr`, current only while a call works on it.
    pub win: Window,
    /// `_parent`.
    pub parent: Win,
    /// `_pary`, `_parx`: where it starts in its parent.
    pub pary: Size,
    pub parx: Size,
}

/// One entry of upstream's window list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    /// `stdscr` or a sub-window.
    Win(Win),
    /// `curscr`.
    Curscr,
    /// `newscr`.
    Newscr,
}

/// `sub`'s cells copied from `root`'s, `(y, x)` being where `sub` starts in
/// `root`: as many lines as `sub` has, as many columns as it is wide, fewer
/// where `root` has none.
fn copy_in(root: &Window, sub: &mut Window, y: i32, x: i32) {
    let width = usize::try_from(i32::from(sub.maxx).wrapping_add(1)).unwrap_or(0);
    let x0 = usize::try_from(x).unwrap_or(0);
    let mut row = y;
    for line in &mut sub.lines {
        line.text.clear();
        if let Some(src) = root.line(row) {
            let end = x0.saturating_add(width).min(src.text.len());
            if let Some(cells) = src.text.get(x0..end) {
                line.text.extend_from_slice(cells);
            }
        }
        row = row.wrapping_add(1);
    }
}

/// `sub`'s cells written back into `root`'s, `(y, x)` being where `sub`
/// starts in `root`.
fn copy_out(sub: &Window, root: &mut Window, y: i32, x: i32) {
    let x0 = usize::try_from(x).unwrap_or(0);
    let mut row = y;
    for line in &sub.lines {
        if let Some(dst) = root.line_mut(row) {
            for (cell, slot) in line.text.iter().zip(dst.text.iter_mut().skip(x0)) {
                *slot = *cell;
            }
        }
        row = row.wrapping_add(1);
    }
}

/// `wborder`'s `_my_render (win, ch)`: the character `ch` as the window would
/// draw it, back as a `chtype` -- the character and the attribute word, the
/// pair kept only as far as the attribute bits hold it.
fn my_render(win: &Window, ch: Attr) -> Attr {
    let wch = render_char(win, Cell::from_chtype(ch));
    wch.ch().cast_unsigned() | wch.attr
}

/// `wborder (win, ls, rs, ts, bs, tl, tr, bl, br)` on a window whose cells
/// are current: the edges drawn, each 0 taking the line-drawing default,
/// every line marked changed.
#[allow(
    clippy::similar_names,
    reason = "upstream's names: ls, rs, ts, bs and their rendered w* forms"
)]
fn wborder(win: &mut Window, edges: [Attr; 8]) -> bool {
    let [ls, rs, ts, bs, tl, tr, bl, br] = edges;
    let dflt = |ch: Attr, letter: u8| if ch == 0 { acs(letter) } else { ch };
    let wls = my_render(win, dflt(ls, b'x'));
    let wrs = my_render(win, dflt(rs, b'x'));
    let wts = my_render(win, dflt(ts, b'q'));
    let wbs = my_render(win, dflt(bs, b'q'));
    let wtl = my_render(win, dflt(tl, b'l'));
    let wtr = my_render(win, dflt(tr, b'k'));
    let wbl = my_render(win, dflt(bl, b'm'));
    let wbr = my_render(win, dflt(br, b'j'));
    let endx = i32::from(win.maxx);
    let endy = i32::from(win.maxy);
    let set = |win: &mut Window, y: i32, x: i32, ch: Attr| {
        if let Some(c) = win.line_mut(y).and_then(|l| l.at_mut(x)) {
            *c = Cell::from_chtype(ch);
        }
    };
    for i in 0..=endx {
        set(win, 0, i, wts);
        set(win, endy, i, wbs);
    }
    for y in [endy, 0] {
        if let Some(line) = win.line_mut(y) {
            line.firstchar = 0;
            line.lastchar = short(endx);
        }
    }
    for i in 0..=endy {
        if endx > 0 && win.cell(i, endx).is_widec_ext() {
            set(win, i, endx.wrapping_sub(1), Attr::from(b' '));
        }
        set(win, i, 0, wls);
        set(win, i, endx, wrs);
        if let Some(line) = win.line_mut(i) {
            line.firstchar = 0;
            line.lastchar = short(endx);
        }
        if win.cell(i, 1).is_widec_ext() {
            set(win, i, 1, Attr::from(b' '));
        }
    }
    set(win, 0, 0, wtl);
    set(win, 0, endx, wtr);
    set(win, endy, 0, wbl);
    set(win, endy, endx, wbr);
    true
}

/// The letters of the VT100 line-drawing set (`acsc`'s), for [`acs`]:
/// `ACS_ULCORNER` is `acs (ACS_ULCORNER)`, and so on.
pub const ACS_ULCORNER: u8 = b'l';
/// Lower left corner.
pub const ACS_LLCORNER: u8 = b'm';
/// Upper right corner.
pub const ACS_URCORNER: u8 = b'k';
/// Lower right corner.
pub const ACS_LRCORNER: u8 = b'j';
/// Tee pointing right.
pub const ACS_LTEE: u8 = b't';
/// Tee pointing left.
pub const ACS_RTEE: u8 = b'u';
/// Tee pointing up.
pub const ACS_BTEE: u8 = b'v';
/// Tee pointing down.
pub const ACS_TTEE: u8 = b'w';
/// Horizontal line.
pub const ACS_HLINE: u8 = b'q';
/// Vertical line.
pub const ACS_VLINE: u8 = b'x';
/// Large plus or crossover.
pub const ACS_PLUS: u8 = b'n';
/// Arrow pointing left.
pub const ACS_LARROW: u8 = b',';
/// Arrow pointing right.
pub const ACS_RARROW: u8 = b'+';
/// Arrow pointing down.
pub const ACS_DARROW: u8 = b'.';
/// Arrow pointing up.
pub const ACS_UARROW: u8 = b'-';
/// Checker board (stipple).
pub const ACS_CKBOARD: u8 = b'a';
/// Bullet.
pub const ACS_BULLET: u8 = b'~';
/// Diamond.
pub const ACS_DIAMOND: u8 = b'`';
/// Solid square block.
pub const ACS_BLOCK: u8 = b'0';

/// `acs_map` has been filled in: a screen was set up.
static ACS_MAP: AtomicBool = AtomicBool::new(false);

/// `_nc_init_acs`'s part for programs: `acs_map` filled in, for good.
pub(crate) fn fill_acs_map() {
    ACS_MAP.store(true, Ordering::Release);
}

/// `NCURSES_ACS (c)`: what a program reads from `acs_map` -- once a screen
/// has been set up, the letter of the VT100 line-drawing set with
/// `A_ALTCHARSET`, which the update draws as the terminal can; before that,
/// and for a byte outside the map, 0.
#[must_use]
pub fn acs(letter: u8) -> Attr {
    if ACS_MAP.load(Ordering::Acquire) && letter != 0 && usize::from(letter) < ACS_LEN {
        A_ALTCHARSET | Attr::from(letter)
    } else {
        0
    }
}

impl Screen {
    /// Where the sub-window numbered `id` is in the list.
    fn sub_index(&self, id: u64) -> Option<usize> {
        self.subwins.iter().position(|s| s.id == id)
    }

    /// The window `w`, to read its place, size, cursor and flags -- not its
    /// cells, which for a sub-window are current only inside
    /// [`Screen::with_win`]. `None` for a sub-window since deleted.
    #[must_use]
    pub fn window(&self, w: Win) -> Option<&Window> {
        match w {
            Win::Stdscr => Some(&self.stdscr),
            Win::Sub(id) => self.subwins.iter().find(|s| s.id == id).map(|s| &s.win),
        }
    }

    /// The window `w`, to change anything of it but its cells.
    pub(crate) fn window_mut(&mut self, w: Win) -> Option<&mut Window> {
        match w {
            Win::Stdscr => Some(&mut self.stdscr),
            Win::Sub(id) => self
                .subwins
                .iter_mut()
                .find(|s| s.id == id)
                .map(|s| &mut s.win),
        }
    }

    /// Where `w`'s cells are in `stdscr`'s: `(0, 0)` for `stdscr` itself,
    /// and for a sub-window the sum of every `_pary` and `_parx` up its
    /// family. `None` for a window since deleted.
    fn origin(&self, w: Win) -> Option<(i32, i32)> {
        let (mut y, mut x) = (0i32, 0i32);
        let mut at = w;
        // A parent is always older than its child and cannot be deleted
        // before it, so the walk ends within the list's length.
        for _ in 0..=self.subwins.len() {
            match at {
                Win::Stdscr => return Some((y, x)),
                Win::Sub(id) => {
                    let s = self.subwins.get(self.sub_index(id)?)?;
                    y = y.wrapping_add(i32::from(s.pary));
                    x = x.wrapping_add(i32::from(s.parx));
                    at = s.parent;
                }
            }
        }
        None
    }

    /// `f` on the window `w`, its cells current, with what adding characters
    /// to it needs; `None` for a window since deleted.
    pub fn with_win<R>(
        &mut self,
        w: Win,
        f: impl FnOnce(&mut Window, &AddCtx<'_>) -> R,
    ) -> Option<R> {
        match w {
            Win::Stdscr => {
                let (win, ctx) = self.stdscr_ctx();
                Some(f(win, &ctx))
            }
            Win::Sub(id) => {
                let (y, x) = self.origin(w)?;
                let i = self.sub_index(id)?;
                let ctx = AddCtx {
                    ctype: &*self.ctype,
                    legacy_coding: self.t.legacy_coding,
                    tabsize: self.tabsize,
                };
                let sub = &mut self.subwins.get_mut(i)?.win;
                copy_in(&self.stdscr, sub, y, x);
                let r = f(sub, &ctx);
                copy_out(sub, &mut self.stdscr, y, x);
                Some(r)
            }
        }
    }

    /// `derwin (orig, lines, cols, begy, begx)`: a sub-window of `orig`, at
    /// `(begy, begx)` in it; 0 lines or columns for the rest of `orig`.
    /// `None` when it would not fit inside `orig`, or `orig` is gone.
    pub fn derwin(
        &mut self,
        orig: Win,
        num_lines: i32,
        num_columns: i32,
        begy: i32,
        begx: i32,
    ) -> Option<Win> {
        let o = self.window(orig)?;
        if begy < 0 || begx < 0 || num_lines < 0 || num_columns < 0 {
            return None;
        }
        let rows = i32::from(o.maxy).wrapping_add(1);
        let cols = i32::from(o.maxx).wrapping_add(1);
        if begy.wrapping_add(num_lines) > rows || begx.wrapping_add(num_columns) > cols {
            return None;
        }
        let num_lines = if num_lines == 0 {
            rows.wrapping_sub(begy)
        } else {
            num_lines
        };
        let num_columns = if num_columns == 0 {
            cols.wrapping_sub(begx)
        } else {
            num_columns
        };
        let flags = SUBWIN | (o.flags & ISPAD);
        let mut win = Window::makenew(
            num_lines,
            num_columns,
            i32::from(o.begy).wrapping_add(begy),
            i32::from(o.begx).wrapping_add(begx),
            flags,
            (self.t.lines, self.t.columns),
            self.topstolen,
        )?;
        win.attrs = o.attrs;
        win.bkgd = o.bkgd;
        let id = self.next_win;
        self.next_win = self.next_win.wrapping_add(1);
        self.subwins.insert(
            0,
            SubWin {
                id,
                win,
                parent: orig,
                pary: short(begy),
                parx: short(begx),
            },
        );
        Some(Win::Sub(id))
    }

    /// `subwin (orig, lines, cols, begy, begx)`: [`Screen::derwin`] with the
    /// place given on the screen rather than in `orig`.
    pub fn subwin(
        &mut self,
        orig: Win,
        num_lines: i32,
        num_columns: i32,
        begy: i32,
        begx: i32,
    ) -> Option<Win> {
        let o = self.window(orig)?;
        let (y, x) = (
            begy.wrapping_sub(i32::from(o.begy)),
            begx.wrapping_sub(i32::from(o.begx)),
        );
        self.derwin(orig, num_lines, num_columns, y, x)
    }

    /// `delwin (win)`: the sub-window gone, and its parent marked changed
    /// throughout. `ERR` for one that still has sub-windows of its own, and
    /// for `stdscr`, which a program here cannot do without.
    pub fn delwin(&mut self, w: Win) -> bool {
        let Win::Sub(id) = w else {
            return false;
        };
        let Some(i) = self.sub_index(id) else {
            return false;
        };
        // `cannot_delete`.
        if self.subwins.iter().any(|s| s.parent == w) {
            return false;
        }
        let parent = self.subwins.get(i).map(|s| s.parent);
        if let Some(parent) = parent {
            self.touchwin(parent);
        }
        self.subwins.remove(i);
        true
    }

    /// `touchwin (win)`: every line of `w` marked changed.
    pub fn touchwin(&mut self, w: Win) -> bool {
        self.window_mut(w).is_some_and(|win| {
            let rows = i32::from(win.maxy).wrapping_add(1);
            win.wtouchln(0, rows, true)
        })
    }

    /// `is_wintouched (win)`: a line of `w` is marked changed.
    #[must_use]
    pub fn is_wintouched(&self, w: Win) -> bool {
        self.window(w).is_some_and(|win| {
            win.lines
                .iter()
                .take(usize::try_from(i32::from(win.maxy).wrapping_add(1)).unwrap_or(0))
                .any(|l| l.firstchar != NOCHANGE)
        })
    }

    /// `wnoutrefresh (win)`: `w`'s changes copied into `newscr`.
    pub fn wnoutrefresh_win(&mut self, w: Win) -> bool {
        if self.window(w).is_none_or(|win| win.flags & ISPAD != 0) {
            return false;
        }
        match w {
            Win::Stdscr => {
                wnoutrefresh_into(&mut self.stdscr, &mut self.newscr);
                true
            }
            Win::Sub(id) => {
                let (Some((y, x)), Some(i)) = (self.origin(w), self.sub_index(id)) else {
                    return false;
                };
                let Some(s) = self.subwins.get_mut(i) else {
                    return false;
                };
                copy_in(&self.stdscr, &mut s.win, y, x);
                wnoutrefresh_into(&mut s.win, &mut self.newscr);
                true
            }
        }
    }

    /// `wrefresh (win)`: [`Screen::wnoutrefresh_win`], then the update.
    pub fn wrefresh(&mut self, w: Win) -> bool {
        if !self.wnoutrefresh_win(w) {
            return false;
        }
        if self.window(w).is_some_and(|win| win.clear) {
            self.newscr.clear = true;
        }
        self.doupdate();
        // "Reset the clearok() flag in case it was set for the special case
        // in hardscroll.c".
        if let Some(win) = self.window_mut(w) {
            win.clear = false;
        }
        true
    }

    /// `werase (win)`: every cell of `w` the background, the cursor home --
    /// and for a sub-window starting inside a wide character of its
    /// parent's, that character blanked too, back to where it begins
    /// (searching at most `_begx` cells back).
    pub fn werase(&mut self, w: Win) -> bool {
        if let Win::Sub(id) = w {
            let (Some((y, x)), Some(i)) = (self.origin(w), self.sub_index(id)) else {
                return false;
            };
            let Some(s) = self.subwins.get(i) else {
                return false;
            };
            let (rows, begx, blank) = (i32::from(s.win.maxy), i32::from(s.win.begx), s.win.bkgd);
            for row in 0..=rows {
                let Some(line) = self.stdscr.line_mut(y.wrapping_add(row)) else {
                    continue;
                };
                if !line.at(x).is_widec_ext() {
                    continue;
                }
                // "If this is a derived window, we have to handle the case
                // where a multicolumn character extends into the window that
                // we are erasing."
                let mut col = x;
                let mut n = begx;
                while n > 0 {
                    n = n.wrapping_sub(1);
                    col = col.wrapping_sub(1);
                    let base = line.at(col).is_widec_base();
                    if let Some(c) = line.at_mut(col) {
                        *c = blank;
                    }
                    if base {
                        break;
                    }
                }
            }
        }
        self.with_win(w, |win, _| win.werase()).unwrap_or(false)
    }

    /// `wclear (win)`: [`Screen::werase`], and a repaint from scratch at the
    /// next refresh of `w`.
    pub fn wclear(&mut self, w: Win) -> bool {
        let ok = self.werase(w);
        if ok && let Some(win) = self.window_mut(w) {
            win.clear = true;
        }
        ok
    }

    /// `wborder (win, ls, rs, ts, bs, tl, tr, bl, br)`.
    pub fn wborder(&mut self, w: Win, edges: [Attr; 8]) -> bool {
        self.with_win(w, |win, _| wborder(win, edges))
            .unwrap_or(false)
    }

    /// `wresize (win, lines, cols)`: `w` made the new size -- a sub-window
    /// only as far as its parent holds it -- and the sub-windows of `w`
    /// brought back inside it.
    pub fn wresize(&mut self, w: Win, to_lines: i32, to_cols: i32) -> bool {
        let Some(win) = self.window(w) else {
            return false;
        };
        let (last_line, last_col) = (to_lines.wrapping_sub(1), to_cols.wrapping_sub(1));
        if last_line < 0 || last_col < 0 {
            return false;
        }
        if last_line == i32::from(win.maxy) && last_col == i32::from(win.maxx) {
            return true;
        }
        if let Win::Sub(id) = w {
            // "Check if the new limits will fit into the parent window's
            // size. If not, do not resize."
            let Some(s) = self.sub_index(id).and_then(|i| self.subwins.get(i)) else {
                return false;
            };
            let Some(p) = self.window(s.parent) else {
                return false;
            };
            if i32::from(s.pary).wrapping_add(last_line) > i32::from(p.maxy)
                || i32::from(s.parx).wrapping_add(last_col) > i32::from(p.maxx)
            {
                return false;
            }
        }
        // A sub-window's cells are only a copy, made again before use, so
        // what `Window::wresize` makes of them does not matter; its change
        // marks are the same as a window's of its own.
        if !self
            .window_mut(w)
            .is_some_and(|win| win.wresize(to_lines, to_cols))
        {
            return false;
        }
        self.repair_subwindows(w);
        true
    }

    /// `repair_subwindows (cmp)`: each sub-window of `cmp` moved and cut
    /// back to fit inside it, its cursor and scrolling region with it, and
    /// its own sub-windows after it.
    fn repair_subwindows(&mut self, cmp: Win) {
        let Some(c) = self.window(cmp) else {
            return;
        };
        let (cmaxy, cmaxx) = (c.maxy, c.maxx);
        let children: Vec<u64> = self
            .subwins
            .iter()
            .filter(|s| s.parent == cmp)
            .map(|s| s.id)
            .collect();
        for id in children {
            if let Some(s) = self.subwins.iter_mut().find(|s| s.id == id) {
                s.pary = s.pary.min(cmaxy);
                s.parx = s.parx.min(cmaxx);
                let t = &mut s.win;
                if i32::from(t.maxy).wrapping_add(i32::from(s.pary)) > i32::from(cmaxy) {
                    t.maxy = short(i32::from(cmaxy).wrapping_sub(i32::from(s.pary)));
                }
                if i32::from(t.maxx).wrapping_add(i32::from(s.parx)) > i32::from(cmaxx) {
                    t.maxx = short(i32::from(cmaxx).wrapping_sub(i32::from(s.parx)));
                }
                t.cury = t.cury.min(t.maxy);
                t.curx = t.curx.min(t.maxx);
                t.regtop = t.regtop.min(t.maxy);
                t.regbottom = t.regbottom.min(t.maxy);
                // Upstream keeps the lines past the new last one, unused;
                // here every line of `lines` is one the window has.
                t.lines
                    .truncate(usize::try_from(i32::from(t.maxy).wrapping_add(1)).unwrap_or(0));
            }
            self.repair_subwindows(Win::Sub(id));
        }
    }

    /// Upstream's window list, the newest first.
    fn window_list(&self) -> Vec<Slot> {
        let mut list: Vec<Slot> = self
            .subwins
            .iter()
            .map(|s| Slot::Win(Win::Sub(s.id)))
            .collect();
        list.extend([Slot::Win(Win::Stdscr), Slot::Curscr, Slot::Newscr]);
        list
    }

    /// The parent of the window in `slot`, if it has one.
    fn parent_of(&self, slot: Slot) -> Option<Win> {
        match slot {
            Slot::Win(Win::Sub(id)) => self.subwins.iter().find(|s| s.id == id).map(|s| s.parent),
            _ => None,
        }
    }

    /// `parent_depth (win)`: how many parents are above it.
    fn parent_depth(&self, slot: Slot) -> usize {
        let mut depth = 0usize;
        let mut at = slot;
        while let Some(p) = self.parent_of(at) {
            depth = depth.saturating_add(1);
            at = Slot::Win(p);
            if depth > self.subwins.len() {
                break;
            }
        }
        depth
    }

    /// `child_depth (win)`: how many generations are below it, following
    /// the first child the list holds at each one.
    fn child_depth(&self, slot: Slot) -> usize {
        let Slot::Win(w) = slot else {
            return 0;
        };
        let mut depth = 0usize;
        let mut at = w;
        while let Some(child) = self.subwins.iter().find(|s| s.parent == at) {
            depth = depth.saturating_add(1);
            at = Win::Sub(child.id);
            if depth > self.subwins.len() {
                break;
            }
        }
        depth
    }

    /// `adjust_window (win, to_lines, to_cols, stolen)` for the window in
    /// `slot`.
    fn adjust_slot(&mut self, slot: Slot, to: (i32, i32), stolen: i32) -> bool {
        let from = self.resize_from;
        let top = i32::from(self.topstolen);
        match slot {
            Slot::Curscr => {
                let (l, c) = adjust_window(&mut self.curscr, to, stolen, from, top);
                self.curscr.wresize(l, c)
            }
            Slot::Newscr => {
                let (l, c) = adjust_window(&mut self.newscr, to, stolen, from, top);
                self.newscr.wresize(l, c)
            }
            Slot::Win(w) => {
                let Some(win) = self.window_mut(w) else {
                    return false;
                };
                let (l, c) = adjust_window(win, to, stolen, from, top);
                self.wresize(w, l, c)
            }
        }
    }

    /// `increase_size (to_lines, to_cols, stolen)`: every window adjusted,
    /// those with no parent first, then their children, and so on.
    pub(crate) fn increase_size(&mut self, to: (i32, i32), stolen: i32) -> bool {
        self.adjust_by_depth(to, stolen, Self::parent_depth)
    }

    /// `decrease_size (to_lines, to_cols, stolen)`: every window adjusted,
    /// those with no children first, then their parents, and so on.
    pub(crate) fn decrease_size(&mut self, to: (i32, i32), stolen: i32) -> bool {
        self.adjust_by_depth(to, stolen, Self::child_depth)
    }

    /// The windows adjusted a depth at a time, each depth in the list's
    /// order, until no window is that deep; `ERR` at the first that fails.
    fn adjust_by_depth(
        &mut self,
        to: (i32, i32),
        stolen: i32,
        depth_of: fn(&Self, Slot) -> usize,
    ) -> bool {
        let list = self.window_list();
        let mut depth = 0usize;
        loop {
            let mut found = false;
            for &slot in &list {
                if depth_of(self, slot) == depth {
                    found = true;
                    if !self.adjust_slot(slot, to, stolen) {
                        return false;
                    }
                }
            }
            if !found {
                return true;
            }
            depth = depth.saturating_add(1);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::addch::{self, Ctype};
    use crate::screen::Options;
    use crate::term::Sink;
    use crate::testing::{CLocale, Utf8};

    /// A screen on xterm-256color kept in memory, 24 by 80.
    fn xterm(ctype: Box<dyn Ctype + Send>) -> Screen {
        let opts = Options {
            output: Sink::Memory(Vec::new()),
            input: -1,
            use_env: false,
        };
        Screen::newterm(b"xterm-256color", &opts, ctype).unwrap()
    }

    #[test]
    fn a_sub_window_must_fit_inside_its_parent() {
        let mut sp = xterm(Box::new(CLocale));
        assert!(sp.subwin(Win::Stdscr, 30, 10, 0, 0).is_none());
        assert!(sp.derwin(Win::Stdscr, 5, 5, 22, 78).is_none());
        assert!(sp.derwin(Win::Stdscr, -1, 5, 0, 0).is_none());
        // 0 lines or columns: the rest of the parent.
        let w = sp.derwin(Win::Stdscr, 0, 0, 20, 70).unwrap();
        let win = sp.window(w).unwrap();
        assert_eq!((win.maxy, win.maxx, win.begy, win.begx), (3, 9, 20, 70));
    }

    #[test]
    fn what_a_sub_window_draws_is_drawn_in_its_parent() {
        let mut sp = xterm(Box::new(CLocale));
        let w = sp.subwin(Win::Stdscr, 3, 10, 5, 5).unwrap();
        let n = sp.derwin(w, 1, 4, 1, 2).unwrap();
        assert_eq!(
            sp.with_win(n, |win, ctx| addch::waddnstr(win, ctx, b"hi", -1)),
            Some(true)
        );
        assert_eq!(sp.stdscr.cell(6, 7).ch(), i32::from(b'h'));
        assert_eq!(sp.stdscr.cell(6, 8).ch(), i32::from(b'i'));
        // ... and seen from the window between them.
        assert_eq!(
            sp.with_win(w, |win, _| win.cell(1, 3).ch()),
            Some(i32::from(b'i'))
        );
        // Its own changes are marked in it, not in its parent.
        sp.wnoutrefresh_win(Win::Stdscr);
        assert!(!sp.is_wintouched(Win::Stdscr));
        sp.with_win(n, |win, ctx| addch::waddnstr(win, ctx, b"x", -1));
        assert!(sp.is_wintouched(n));
        assert!(!sp.is_wintouched(Win::Stdscr));
    }

    #[test]
    fn a_window_with_sub_windows_cannot_be_deleted_and_deleting_one_touches_its_parent() {
        let mut sp = xterm(Box::new(CLocale));
        let w = sp.subwin(Win::Stdscr, 3, 10, 5, 5).unwrap();
        let n = sp.derwin(w, 1, 4, 1, 2).unwrap();
        assert!(!sp.delwin(w));
        assert!(!sp.delwin(Win::Stdscr));
        sp.with_win(w, |win, _| win.wtouchln(0, 3, false));
        assert!(sp.delwin(n));
        assert!(sp.is_wintouched(w));
        assert!(!sp.delwin(n), "a deleted window names nothing");
        assert!(sp.delwin(w));
        assert!(sp.window(w).is_none());
    }

    #[test]
    fn a_box_is_drawn_with_the_line_drawing_characters() {
        let mut sp = xterm(Box::new(CLocale));
        let w = sp.subwin(Win::Stdscr, 3, 4, 2, 3).unwrap();
        assert!(sp.wborder(w, [0; 8]));
        let at = |sp: &Screen, y: i32, x: i32| {
            let c = sp.stdscr.cell(y, x);
            (c.ch(), c.attr & A_ALTCHARSET != 0)
        };
        assert_eq!(at(&sp, 2, 3), (i32::from(b'l'), true));
        assert_eq!(at(&sp, 2, 4), (i32::from(b'q'), true));
        assert_eq!(at(&sp, 2, 6), (i32::from(b'k'), true));
        assert_eq!(at(&sp, 3, 3), (i32::from(b'x'), true));
        assert_eq!(at(&sp, 4, 6), (i32::from(b'j'), true));
        // A given edge is drawn as given.
        assert!(sp.wborder(
            w,
            [
                Attr::from(b'|'),
                Attr::from(b'|'),
                Attr::from(b'-'),
                Attr::from(b'-'),
                0,
                0,
                0,
                0
            ]
        ));
        assert_eq!(at(&sp, 2, 4), (i32::from(b'-'), false));
        assert_eq!(at(&sp, 2, 3), (i32::from(b'l'), true));
        assert_eq!(acs(b'q'), A_ALTCHARSET | Attr::from(b'q'));
        assert_eq!(acs(0), 0);
        assert_eq!(acs(200), 0);
    }

    #[test]
    fn shrinking_the_screen_cuts_a_sub_window_back_to_fit() {
        let mut sp = xterm(Box::new(CLocale));
        let w = sp.subwin(Win::Stdscr, 5, 70, 10, 5).unwrap();
        let n = sp.derwin(w, 3, 60, 1, 5).unwrap();
        assert!(sp.resize_term(20, 60));
        let size = |sp: &Screen, w: Win| {
            let win = sp.window(w).unwrap();
            (i32::from(win.maxy) + 1, i32::from(win.maxx) + 1)
        };
        // The sub-windows first, each made at most the new width; then
        // stdscr, which cuts them back inside itself.
        assert_eq!(size(&sp, Win::Stdscr), (20, 60));
        assert_eq!(size(&sp, w), (5, 55));
        assert_eq!(size(&sp, n), (3, 50));
        // Growing leaves them as they are.
        assert!(sp.resize_term(30, 100));
        assert_eq!(size(&sp, w), (5, 55));
    }

    #[test]
    fn erasing_a_sub_window_inside_a_wide_character_blanks_all_of_it() {
        let mut sp = xterm(Box::new(Utf8));
        let (win, ctx) = sp.stdscr_ctx();
        win.wmove(0, 4);
        // A double-width character in columns 4 and 5.
        addch::waddnwstr(win, &ctx, &[0x4e2d], -1);
        let _ = ctx;
        assert!(sp.stdscr.cell(0, 5).is_widec_ext());
        let w = sp.subwin(Win::Stdscr, 1, 3, 0, 5).unwrap();
        assert!(sp.werase(w));
        assert!(sp.stdscr.cell(0, 4).is_blank());
        assert!(sp.stdscr.cell(0, 5).is_blank());
    }
}
