//! Adding characters to a window: `lib_addch.c` (`waddch`, and through it
//! `waddnstr` and `wprintw`), `lib_add_wch.c` (`wadd_wch`, and through it
//! `waddnwstr`), `charable.c`, `lib_wunctrl.c` and the `unctrl` that
//! `MKunctrl.awk` generates.
//!
//! The two families are separate in upstream and stay separate here,
//! because they differ: `waddch` takes bytes, building a multibyte character
//! from them one at a time with `mbrtowc`, and decides what is printable
//! with `<ctype.h>`; `wadd_wch` takes wide characters and asks `iswprint`.
//! `wadd_wch`'s newline is checked against the scrolling region without
//! `waddch`'s bound on the window's last row; and looking for the cell a
//! combining character joins, `waddch` looks as far as column 0 and
//! `wadd_wch` stops at column 1. (`waddch` also moves its cursor to that
//! cell on the way -- and then back, before it returns.)
//!
//! What a locale answers comes through [`Ctype`], which the screen answers
//! from the C library ([`crate::Libc`]) and the tests from a table of their
//! own.
//!
//! Upstream is built with `NDEBUG`, so a character added outside the window
//! is not refused; it writes past the line, which C leaves undefined. Here
//! such a write is dropped.

use crate::cell::{A_ALTCHARSET, A_COLOR, A_NORMAL, Attr, BLANK_TEXT, CCHARW_MAX, Cell, WChar};
use crate::window::{WRAPPED, Window, short};

/// What a locale answers, as the C library would answer it.
pub trait Ctype {
    /// `mbrtowc` from the initial state over `bytes`: the length of the
    /// character when they make one whole (`> 0`), 0 for a NUL, -1 when
    /// they make none (`EILSEQ`), -2 when they make the start of one; and
    /// the character.
    fn mbrtowc(&self, bytes: &[u8]) -> (i32, WChar);
    /// `wcwidth`.
    fn wcwidth(&self, wc: WChar) -> i32;
    /// `iswprint`.
    fn iswprint(&self, wc: WChar) -> bool;
    /// `isprint`, of an `unsigned char` value.
    fn isprint(&self, c: i32) -> bool;
    /// `iscntrl`, of an `unsigned char` value.
    fn iscntrl(&self, c: i32) -> bool;
    /// `wctob`.
    fn wctob(&self, wc: WChar) -> i32;
    /// `btowc`.
    fn btowc(&self, c: i32) -> u32;
    /// `wcrtomb` from the initial state: the character's bytes, or `None`
    /// when the locale cannot write it.
    fn wcrtomb(&self, wc: WChar) -> Option<Vec<u8>>;
    /// `_nc_unicode_locale ()`: the locale's codeset is `UTF-8`.
    fn unicode_locale(&self) -> bool;
    /// Whether the locale is `C` or `POSIX`, or has no name at all -- what
    /// `_nc_setupscreen` makes `_legacy_coding` of.
    fn legacy_locale(&self) -> bool;
}

/// What adding a character needs to know of the screen.
pub struct AddCtx<'a> {
    /// The locale.
    pub ctype: &'a dyn Ctype,
    /// `sp->_legacy_coding`: 1 in the `C` or `POSIX` locale, else 0 (2 only
    /// after `use_legacy_coding (2)`, which nothing here calls).
    pub legacy_coding: i32,
    /// `TABSIZE`.
    pub tabsize: i32,
}

/// `is8bits (c)`.
const fn is8bits(c: i32) -> bool {
    c.cast_unsigned() <= 255
}

/// `x + (tabsize - (x % tabsize))`: the column of the next tab stop.
fn next_tab(x: i32, tabsize: i32) -> i32 {
    let tabsize = tabsize.max(1);
    x.wrapping_add(tabsize.wrapping_sub(x.checked_rem(tabsize).unwrap_or(0)))
}

/// `COLOR_MASK (ch)`: everything, or everything but the colour bits when
/// `ch` has some.
const fn color_mask(ch: Attr) -> Attr {
    !(if ch & A_COLOR != 0 { A_COLOR } else { 0 })
}

/// `render_char (win, ch)`: the character as the window's attributes, pair
/// and background make it -- a plain blank becomes the background.
#[must_use]
pub fn render_char(win: &Window, ch: Cell) -> Cell {
    let a = win.attrs;
    let mut pair = ch.pair();
    if ch.is_blank() && ch.attr == A_NORMAL && pair == 0 {
        // "color/pair in attrs has precedence over bkgrnd"
        let mut out = win.bkgd;
        out.set_attr(a | win.bkgd.attr);
        pair = win.pair();
        if pair == 0 {
            pair = win.bkgd.pair();
        }
        out.set_pair(pair);
        out
    } else {
        // "color in attrs has precedence over bkgrnd"
        let a = a | (win.bkgd.attr & color_mask(a));
        // "color in ch has precedence"
        if pair == 0 {
            pair = win.pair();
            if pair == 0 {
                pair = win.bkgd.pair();
            }
        }
        let mut out = ch;
        out.add_attr(a & color_mask(ch.attr));
        out.set_pair(pair);
        out
    }
}

/// `_nc_to_char (ch)`: `wctob`.
fn to_char(ctx: &AddCtx<'_>, ch: WChar) -> i32 {
    ctx.ctype.wctob(ch)
}

/// `_nc_is_charable (ch)`: the character is one byte of the locale.
fn is_charable(ctx: &AddCtx<'_>, ch: WChar) -> bool {
    ctx.ctype.wctob(ch) == ch
}

/// `Charable (ch)`: a cell that goes out as the one byte it is.
#[must_use]
pub fn charable(ctx: &AddCtx<'_>, ch: &Cell) -> bool {
    (ctx.legacy_coding != 0 || ch.attr & A_ALTCHARSET != 0 || !ch.is_widec_ext())
        && ch.chars[1] == 0
        && is_charable(ctx, ch.ch())
}

/// `unctrl (ch)` (`safe_unctrl` with a screen): how a byte is shown -- `^X`
/// for a control character, `~X` and `M-X` for the bytes above 127 the
/// locale does not print, the byte itself otherwise. Only the character
/// bits of `ch` are looked at (`ChCharOf`), so there is always an answer.
#[must_use]
pub fn unctrl(ctx: &AddCtx<'_>, ch: Attr) -> Vec<u8> {
    let [byte, ..] = (ch & crate::cell::A_CHARTEXT).to_le_bytes();
    let check = i32::from(byte);
    // `unctrl_c1`: the byte itself.
    let raw = || vec![byte];
    if ctx.legacy_coding > 1 && (128..160).contains(&check) {
        return raw();
    }
    if (160..256).contains(&check)
        && (ctx.legacy_coding > 0 || (ctx.legacy_coding == 0 && ctx.ctype.isprint(check)))
    {
        return raw();
    }
    // `unctrl_table`.
    match byte {
        0..=31 => vec![b'^', byte.wrapping_add(0x40)],
        32..=126 => vec![byte],
        127 => b"^?".to_vec(),
        128..=159 => vec![b'~', byte.wrapping_sub(64)],
        160..=254 => vec![b'M', b'-', byte.wrapping_sub(128)],
        255 => b"~?".to_vec(),
    }
}

/// `wunctrl (wc)`: [`unctrl`] of a one-byte character, widened; the cell's
/// own characters otherwise.
#[must_use]
pub fn wunctrl(ctx: &AddCtx<'_>, wc: &Cell) -> Vec<WChar> {
    if charable(ctx, wc) {
        let shown = unctrl(ctx, to_char(ctx, wc.ch()).cast_unsigned());
        shown
            .iter()
            .map(|&b| ctx.ctype.btowc(i32::from(b)).cast_signed())
            .collect()
    } else {
        wc.chars.iter().copied().take_while(|&c| c != 0).collect()
    }
}

/// The two `newline_forces_scroll`s: `waddch`'s keeps the cursor on the
/// window's last row, `wadd_wch`'s does not.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    /// `lib_addch.c`.
    Narrow,
    /// `lib_add_wch.c`.
    Wide,
}

/// `newline_forces_scroll (win, &ypos)`: the row a newline at `ypos` moves
/// to, and whether it must scroll the region to get there.
fn newline_forces_scroll(win: &Window, ypos: &mut i16, family: Family) -> bool {
    match family {
        Family::Narrow => {
            if *ypos >= win.regtop && *ypos <= win.regbottom {
                if *ypos == win.regbottom {
                    *ypos = win.regbottom;
                    return true;
                } else if *ypos < win.maxy {
                    *ypos = short(i32::from(*ypos).wrapping_add(1));
                }
            } else if *ypos < win.maxy {
                *ypos = short(i32::from(*ypos).wrapping_add(1));
            }
            false
        }
        Family::Wide => {
            if *ypos >= win.regtop && *ypos == win.regbottom {
                *ypos = win.regbottom;
                true
            } else {
                *ypos = short(i32::from(*ypos).wrapping_add(1));
                false
            }
        }
    }
}

/// `wrap_to_next_line (win)`: the cursor to the start of the next row,
/// scrolling when it may; `false` (`ERR`) at the bottom of a window that may
/// not.
fn wrap_to_next_line(win: &mut Window, family: Family) -> bool {
    win.flags |= WRAPPED;
    let mut y = win.cury;
    if newline_forces_scroll(win, &mut y, family) {
        win.cury = y;
        win.curx = win.maxx;
        if !win.scroll {
            return false;
        }
        win.wscrl(1);
    } else {
        win.cury = y;
    }
    win.curx = 0;
    true
}

/// `_nc_build_wch (win, &ch)`: one more byte of a multibyte character. The
/// length when it completes one (the character is then in `ch`), -1 when
/// the bytes make none, -2 when more are needed.
fn build_wch(win: &mut Window, ctx: &AddCtx<'_>, ch: &mut Cell) -> i32 {
    let x = i32::from(win.curx);
    let y = i32::from(win.cury);
    if win.addch_used != 0 && (win.addch_x != x || win.addch_y != y) {
        // "discard the incomplete multibyte character"
        win.addch_used = 0;
    }
    win.addch_x = x;
    win.addch_y = y;
    // "If the background character is a wide-character, that may interfere
    // with processing multibyte characters in this function."
    if !is8bits(ch.ch()) {
        if win.addch_used != 0 {
            win.addch_used = 0;
        }
        return 1;
    }
    let used = win.addch_used;
    let Some(slot) = win.addch_work.get_mut(used) else {
        win.addch_used = 0;
        return -1;
    };
    // `(char) CharOf (ch)`: the low byte, which is all of it here.
    *slot = ch.ch().to_le_bytes()[0];
    win.addch_used = used.saturating_add(1);
    let (len, result) = ctx
        .ctype
        .mbrtowc(win.addch_work.get(..win.addch_used).unwrap_or_default());
    if len > 0 {
        let attrs = ch.attr;
        let pair = ch.pair();
        ch.set_char(result, attrs);
        ch.set_pair(pair);
        win.addch_used = 0;
    } else if len == -1 {
        // "assume that the error was in the previous input" -- handled by
        // the caller through unctrl.
        win.addch_used = 0;
    }
    len
}

/// `fill_cells (win, count)`: `count` blanks from the cursor, which is put
/// back where it was.
fn fill_cells(win: &mut Window, ctx: &AddCtx<'_>, count: i32, family: Family) {
    let save = (win.curx, win.cury);
    for _ in 0..count.max(0) {
        let ok = match family {
            Family::Narrow => waddch_literal(win, ctx, Cell::blank()),
            Family::Wide => wadd_wch_literal(win, ctx, Cell::blank()),
        };
        if !ok {
            break;
        }
    }
    (win.curx, win.cury) = save;
}

/// The cells a character wider than one column takes, put on the line from
/// the cursor -- after filling the rest of the line and wrapping when it
/// does not fit, and blanking the cells of any wide character it cuts.
/// `None` when it cannot be placed; else the column after it.
fn place_wide(
    win: &mut Window,
    ctx: &AddCtx<'_>,
    ch: Cell,
    len: i32,
    family: Family,
) -> Option<i32> {
    let mut x = i32::from(win.curx);
    let mut y = i32::from(win.cury);
    let maxx = i32::from(win.maxx);
    if len > maxx.wrapping_add(1) {
        // "character will not fit"
        return None;
    } else if x.wrapping_add(len) > maxx.wrapping_add(1) {
        let count = maxx.wrapping_add(1).wrapping_sub(x);
        fill_cells(win, ctx, count, family);
        if !wrap_to_next_line(win, family) {
            return None;
        }
        x = i32::from(win.curx);
        y = i32::from(win.cury);
    }
    // "Check for cells which are orphaned by adding this character, set
    // those to blanks."
    for i in 0..len {
        let here = win.cell(y, x.wrapping_add(i));
        if here.is_widec_base() {
            break;
        } else if here.is_widec_ext() {
            let mut j = i;
            while x.wrapping_add(j) <= maxx {
                if !win.cell(y, x.wrapping_add(j)).is_widec_ext() {
                    fill_cells(win, ctx, j, family);
                    break;
                }
                j = j.wrapping_add(1);
            }
            break;
        }
    }
    for i in 0..len {
        let mut value = ch;
        value.set_widec_ext(i);
        if let Some(line) = win.line_mut(y) {
            if let Some(c) = line.at_mut(x) {
                *c = value;
            }
            line.changed_cell(x);
        }
        x = x.wrapping_add(1);
    }
    Some(x)
}

/// After a character is placed, ending at column `x`: the cursor there, or
/// on to the next line past the window's edge.
fn after_placing(win: &mut Window, x: i32, family: Family) -> bool {
    if x > i32::from(win.maxx) {
        return wrap_to_next_line(win, family);
    }
    win.curx = short(x);
    true
}

/// `waddch_literal (win, ch)`: the character put at the cursor as it is.
#[allow(clippy::too_many_lines)]
fn waddch_literal(win: &mut Window, ctx: &AddCtx<'_>, ch: Cell) -> bool {
    let x = i32::from(win.curx);
    let y = i32::from(win.cury);
    let mut ch = render_char(win, ch);
    if let Some(line) = win.line_mut(y) {
        line.changed_cell(x);
    }
    // "Build up multibyte characters until we have a wide-character."
    if win.addch_used != 0 || !charable(ctx, &ch) {
        let len = build_wch(win, ctx, &mut ch);
        if len >= -1 {
            let attr = ch.attr;
            // "handle EILSEQ"
            if len == -1 && is8bits(ch.ch()) {
                let shown = unctrl(ctx, ch.ch().cast_unsigned());
                if shown.len() > 1 {
                    for b in shown {
                        if !waddch(win, ctx, u32::from(b) | attr) {
                            return false;
                        }
                    }
                    return true;
                }
            }
            if len == -1 {
                return waddch(win, ctx, u32::from(b' ') | attr);
            }
        } else {
            return true;
        }
    }
    // "Non-spacing characters are added to the current cell."
    let len = ctx.ctype.wcwidth(ch.ch());
    let x = i32::from(win.curx);
    let y = i32::from(win.cury);
    if len == 0 {
        if (x > 0 && y >= 0) || (win.maxx >= 0 && win.cury >= 1) {
            let (row, col) = if x > 0 && y >= 0 {
                let mut j = x.wrapping_sub(1);
                while j >= 0 {
                    if !win.cell(y, j).is_widec_ext() {
                        win.curx = short(j);
                        break;
                    }
                    j = j.wrapping_sub(1);
                }
                (y, j)
            } else {
                (y.wrapping_sub(1), i32::from(win.maxx))
            };
            if let Some(cell) = win.line_mut(row).and_then(|l| l.at_mut(col)) {
                if let Some(free) = cell.chars.iter_mut().take(CCHARW_MAX).find(|c| **c == 0) {
                    *free = ch.ch();
                }
            }
        }
        return after_placing(win, x, Family::Narrow);
    } else if len > 1 {
        return match place_wide(win, ctx, ch, len, Family::Narrow) {
            Some(x) => after_placing(win, x, Family::Narrow),
            None => false,
        };
    }
    // "Single-column characters."
    if let Some(cell) = win.line_mut(y).and_then(|l| l.at_mut(x)) {
        *cell = ch;
    }
    after_placing(win, x.wrapping_add(1), Family::Narrow)
}

/// `waddch_nosync (win, ch)`: a character, a control character acted on,
/// or one shown as `unctrl` shows it.
fn waddch_nosync(win: &mut Window, ctx: &AddCtx<'_>, ch: Cell) -> bool {
    let t = ch.ch();
    let shown = unctrl(ctx, t.cast_unsigned());
    let single = shown.len() == 1;
    if ch.attr & A_ALTCHARSET != 0
        || (ctx.legacy_coding != 0 && single)
        || (ctx.ctype.isprint(t) && !ctx.ctype.iscntrl(t))
        || (ctx.legacy_coding == 0 && (win.addch_used != 0 || !is_charable(ctx, ch.ch())))
    {
        return waddch_literal(win, ctx, ch);
    }
    // "Handle carriage control and other codes that are not printable, or
    // are known to expand to more than one character according to unctrl()."
    let mut x = win.curx;
    let mut y = win.cury;
    match t {
        0x09 => {
            let tabsize = ctx.tabsize;
            let target = next_tab(i32::from(x), tabsize);
            x = short(target);
            // "Space-fill the tab on the bottom line so that we'll get the
            // "correct" cursor position."
            if (!win.scroll && y == win.regbottom) || x <= win.maxx {
                let mut blank = Cell::blank();
                blank.add_attr(ch.attr);
                while win.curx < x {
                    if !waddch_literal(win, ctx, blank) {
                        return false;
                    }
                }
                return true;
            }
            win.wclrtoeol();
            win.flags |= WRAPPED;
            if newline_forces_scroll(win, &mut y, Family::Narrow) {
                x = win.maxx;
                if win.scroll {
                    win.wscrl(1);
                    x = 0;
                }
            } else {
                x = 0;
            }
        }
        0x0a => {
            win.wclrtoeol();
            if newline_forces_scroll(win, &mut y, Family::Narrow) {
                if win.scroll {
                    win.wscrl(1);
                } else {
                    return false;
                }
            }
            x = 0;
            win.flags &= !WRAPPED;
        }
        0x0d => {
            x = 0;
            win.flags &= !WRAPPED;
        }
        0x08 => {
            if x == 0 {
                return true;
            }
            x = x.wrapping_sub(1);
            win.flags &= !WRAPPED;
        }
        _ => {
            for b in shown {
                let mut sch = Cell::with_char(WChar::from(b), ch.attr);
                sch.set_pair(ch.pair());
                if !waddch_literal(win, ctx, sch) {
                    return false;
                }
            }
            return true;
        }
    }
    win.curx = x;
    win.cury = y;
    true
}

/// `waddch (win, ch)`: a `chtype` -- a byte and its attributes.
pub fn waddch(win: &mut Window, ctx: &AddCtx<'_>, ch: Attr) -> bool {
    waddch_nosync(win, ctx, Cell::from_chtype(ch))
}

/// `waddnstr (win, str, n)`: the bytes of `bytes` up to a NUL, at most `n`
/// of them (all, when `n` is negative), each through `waddch`; `false` at
/// the first that fails, or when there is nothing to add.
pub fn waddnstr(win: &mut Window, ctx: &AddCtx<'_>, bytes: &[u8], n: i32) -> bool {
    if n == 0 {
        return false;
    }
    let limit = if n > 0 {
        usize::try_from(n).unwrap_or(usize::MAX)
    } else {
        usize::MAX
    };
    for &b in bytes.iter().take(limit) {
        if b == 0 {
            break;
        }
        let ch = Cell::with_char(WChar::from(b), A_NORMAL);
        if !waddch_nosync(win, ctx, ch) {
            return false;
        }
    }
    true
}

/// `wadd_wch_literal (win, ch)`.
fn wadd_wch_literal(win: &mut Window, ctx: &AddCtx<'_>, ch: Cell) -> bool {
    let x = i32::from(win.curx);
    let y = i32::from(win.cury);
    let ch = render_char(win, ch);
    if let Some(line) = win.line_mut(y) {
        line.changed_cell(x);
    }
    let len = ctx.ctype.wcwidth(ch.ch());
    if len == 0 {
        // "non-spacing"
        if (x > 0 && y >= 0) || (win.maxx >= 0 && win.cury >= 1) {
            let (row, col) = if x > 0 && y >= 0 {
                let mut j = x.wrapping_sub(1);
                while j > 0 {
                    if !win.cell(y, j).is_widec_ext() {
                        break;
                    }
                    j = j.wrapping_sub(1);
                }
                (y, j)
            } else {
                (y.wrapping_sub(1), i32::from(win.maxx))
            };
            if let Some(cell) = win.line_mut(row).and_then(|l| l.at_mut(col)) {
                if let Some(free) = cell.chars.iter_mut().take(CCHARW_MAX).find(|c| **c == 0) {
                    *free = ch.ch();
                }
            }
        }
        return after_placing(win, x, Family::Wide);
    } else if len > 1 {
        return match place_wide(win, ctx, ch, len, Family::Wide) {
            Some(x) => after_placing(win, x, Family::Wide),
            None => false,
        };
    }
    if let Some(cell) = win.line_mut(y).and_then(|l| l.at_mut(x)) {
        *cell = ch;
    }
    after_placing(win, x.wrapping_add(1), Family::Wide)
}

/// `wadd_wch_nosync (win, ch)`.
fn wadd_wch_nosync(win: &mut Window, ctx: &AddCtx<'_>, ch: Cell) -> bool {
    // "If we are using the alternate character set, forget about locale.
    // Otherwise, if the locale claims the code is printable, treat it that
    // way."
    if ch.attr & A_ALTCHARSET != 0 || ctx.ctype.iswprint(ch.ch()) {
        return wadd_wch_literal(win, ctx, ch);
    }
    let mut x = win.curx;
    let mut y = win.cury;
    match ch.ch() {
        0x09 => {
            let tabsize = ctx.tabsize;
            x = short(next_tab(i32::from(x), tabsize));
            if (!win.scroll && y == win.regbottom) || x <= win.maxx {
                let mut blank = Cell::new2(BLANK_TEXT, A_NORMAL);
                blank.add_attr(ch.attr);
                while win.curx < x {
                    if !wadd_wch_literal(win, ctx, blank) {
                        return false;
                    }
                }
                return true;
            }
            win.wclrtoeol();
            win.flags |= WRAPPED;
            if newline_forces_scroll(win, &mut y, Family::Wide) {
                x = win.maxx;
                if win.scroll {
                    win.wscrl(1);
                    x = 0;
                }
            } else {
                x = 0;
            }
        }
        0x0a => {
            win.wclrtoeol();
            if newline_forces_scroll(win, &mut y, Family::Wide) {
                if win.scroll {
                    win.wscrl(1);
                } else {
                    return false;
                }
            }
            x = 0;
            win.flags &= !WRAPPED;
        }
        0x0d => {
            x = 0;
            win.flags &= !WRAPPED;
        }
        0x08 => {
            if x == 0 {
                return true;
            }
            x = x.wrapping_sub(1);
            win.flags &= !WRAPPED;
        }
        _ => {
            let shown = wunctrl(ctx, &ch);
            for c in shown {
                let mut sch = Cell::with_char(c, ch.attr);
                sch.set_pair(ch.pair());
                if !wadd_wch_literal(win, ctx, sch) {
                    return false;
                }
            }
            return true;
        }
    }
    win.curx = x;
    win.cury = y;
    true
}

/// `wadd_wch (win, wch)`.
pub fn wadd_wch(win: &mut Window, ctx: &AddCtx<'_>, wch: &Cell) -> bool {
    wadd_wch_nosync(win, ctx, *wch)
}

/// `waddnwstr (win, str, n)`: the wide characters of `wide` up to a NUL, at
/// most `n` of them (all, when `n` is negative), each through `wadd_wch`.
pub fn waddnwstr(win: &mut Window, ctx: &AddCtx<'_>, wide: &[WChar], n: i32) -> bool {
    if n == 0 {
        return false;
    }
    let limit = if n > 0 {
        usize::try_from(n).unwrap_or(usize::MAX)
    } else {
        usize::MAX
    };
    for &c in wide.iter().take(limit) {
        if c == 0 {
            break;
        }
        let ch = Cell::with_char(c, A_NORMAL);
        if !wadd_wch(win, ctx, &ch) {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::cell::{A_BOLD, A_REVERSE, color_pair};
    use crate::testing::Utf8;
    use crate::window::NOCHANGE;

    fn ctx(locale: &dyn Ctype, legacy: i32) -> AddCtx<'_> {
        AddCtx {
            ctype: locale,
            legacy_coding: legacy,
            tabsize: 8,
        }
    }

    fn win(lines: i32, cols: i32) -> Window {
        let mut w = Window::newwin(lines, cols, 0, 0, (lines, cols), 0).unwrap();
        for line in &mut w.lines {
            line.firstchar = NOCHANGE;
            line.lastchar = NOCHANGE;
        }
        w
    }

    fn text(w: &Window, y: usize) -> String {
        w.lines[y]
            .text
            .iter()
            .filter(|c| !c.is_widec_ext())
            .map(|c| char::from_u32(c.ch().cast_unsigned()).unwrap())
            .collect()
    }

    #[test]
    fn bytes_go_in_with_the_windows_attributes() {
        let u = Utf8;
        let c = ctx(&u, 0);
        let mut w = win(2, 10);
        w.wattrset(A_BOLD | color_pair(2));
        assert!(waddnstr(&mut w, &c, b"ab", -1));
        assert_eq!(text(&w, 0), "ab        ");
        assert_eq!(w.lines[0].text[0].attr, A_BOLD | color_pair(2));
        assert_eq!(w.lines[0].text[0].pair(), 2);
        assert_eq!((w.lines[0].firstchar, w.lines[0].lastchar), (0, 1));
        assert_eq!(w.curx, 2);
    }

    #[test]
    fn a_multibyte_character_is_built_a_byte_at_a_time() {
        let u = Utf8;
        let c = ctx(&u, 0);
        let mut w = win(1, 10);
        assert!(waddnstr(&mut w, &c, "é中x".as_bytes(), -1));
        assert_eq!(text(&w, 0), "é中x      ");
        assert!(w.lines[0].text[1].is_widec_base());
        assert!(w.lines[0].text[2].is_widec_ext());
        assert_eq!(w.curx, 4);
    }

    #[test]
    fn a_byte_that_is_no_character_is_shown_as_unctrl_shows_it() {
        let u = Utf8;
        let c = ctx(&u, 0);
        let mut w = win(1, 10);
        assert!(waddnstr(&mut w, &c, b"\xff\x01", -1));
        assert_eq!(text(&w, 0), "~?^A      ");
    }

    #[test]
    fn control_characters_move_the_cursor() {
        let u = Utf8;
        let c = ctx(&u, 1);
        let mut w = win(3, 10);
        assert!(waddnstr(&mut w, &c, b"ab\tc\r", -1));
        assert_eq!(text(&w, 0), "ab      c ");
        assert_eq!((w.cury, w.curx), (0, 0));
        assert!(waddnstr(&mut w, &c, b"xyz\n", -1));
        assert_eq!(text(&w, 0), "xyz       ", "the newline cleared the line");
        assert_eq!((w.cury, w.curx), (1, 0));
        assert!(waddnstr(&mut w, &c, b"q\x08r", -1));
        assert_eq!(text(&w, 1), "r         ");
        // At the bottom of a window that may not scroll, a newline fails.
        w.wmove(2, 0);
        assert!(!waddnstr(&mut w, &c, b"\n", -1));
    }

    #[test]
    fn writing_past_the_edge_wraps_and_the_last_cell_is_allowed() {
        let u = Utf8;
        let c = ctx(&u, 1);
        let mut w = win(2, 3);
        assert!(waddnstr(&mut w, &c, b"abcd", -1));
        assert_eq!(text(&w, 0), "abc");
        assert_eq!(text(&w, 1), "d  ");
        // The lower-right corner takes a character, then cannot wrap.
        w.wmove(1, 2);
        assert!(!waddnstr(&mut w, &c, b"z", -1));
        assert_eq!(text(&w, 1), "d z");
    }

    #[test]
    fn a_wide_character_that_does_not_fit_fills_the_line_and_wraps() {
        let u = Utf8;
        let c = ctx(&u, 0);
        let mut w = win(2, 3);
        w.wmove(0, 2);
        let wide = [0x4e2d];
        assert!(waddnwstr(&mut w, &c, &wide, -1));
        assert_eq!(text(&w, 0), "   ");
        assert_eq!(text(&w, 1), "中 ");
        assert_eq!((w.cury, w.curx), (1, 2));
    }

    #[test]
    fn a_combining_character_joins_the_cell_before() {
        let u = Utf8;
        let c = ctx(&u, 0);
        let mut w = win(1, 5);
        assert!(waddnwstr(&mut w, &c, &[0x61, 0x301, 0x62], -1));
        assert_eq!(w.lines[0].text[0].chars[..2], [0x61, 0x301]);
        assert_eq!(text(&w, 0), "ab   ");
        // The byte family joins it the same way, its cursor where it was.
        let mut v = win(1, 5);
        assert!(waddnstr(&mut v, &c, "a\u{301}b".as_bytes(), -1));
        assert_eq!(v.lines[0].text[0].chars[..2], [0x61, 0x301]);
        assert_eq!(text(&v, 0), "ab   ");
    }

    #[test]
    fn wide_control_characters_are_shown_by_wunctrl() {
        let u = Utf8;
        let c = ctx(&u, 0);
        let mut w = win(1, 6);
        assert!(waddnwstr(&mut w, &c, &[0x7, 0x7f], -1));
        assert_eq!(text(&w, 0), "^G^?  ");
    }

    #[test]
    fn unctrl_follows_the_locale_above_127() {
        let u = Utf8;
        let legacy = ctx(&u, 1);
        let modern = ctx(&u, 0);
        assert_eq!(unctrl(&legacy, 0x01), b"^A");
        assert_eq!(unctrl(&legacy, 0x41), b"A");
        assert_eq!(unctrl(&legacy, 0x7f), b"^?");
        assert_eq!(unctrl(&legacy, 0x81), b"~A");
        assert_eq!(
            unctrl(&legacy, 0xe9),
            b"\xe9",
            "a legacy locale shows it as it is"
        );
        assert_eq!(unctrl(&modern, 0xe9), b"M-i");
        assert_eq!(unctrl(&modern, 0xff), b"~?");
        assert_eq!(
            unctrl(&modern, 0x100 | A_REVERSE),
            b"^@".to_vec(),
            "the character bits only"
        );
    }

    #[test]
    fn a_blank_takes_the_background_and_the_windows_pair() {
        let mut w = win(1, 2);
        w.bkgd = Cell::new2(0x2e, A_REVERSE);
        w.color = 4;
        let blank = render_char(&w, Cell::blank());
        assert_eq!(blank.ch(), 0x2e);
        assert_eq!(blank.pair(), 4);
        assert_eq!(blank.attr & A_REVERSE, A_REVERSE);
        let other = render_char(&w, Cell::with_char(0x41, A_BOLD));
        assert_eq!(other.ch(), 0x41);
        assert_eq!(other.attr & (A_BOLD | A_REVERSE), A_BOLD | A_REVERSE);
        assert_eq!(other.pair(), 4);
    }
}
