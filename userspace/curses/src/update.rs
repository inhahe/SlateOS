//! Bringing the terminal into line with the screen: `tty_update.c` --
//! `doupdate`'s work once the windows have been copied into `newscr`.
//!
//! `curscr` is what the terminal shows; `newscr` what it should. Each line
//! that differs is transformed (`TransformLine`): the stretch that changed
//! found, the cheapest way to rewrite it chosen -- overwriting, clearing to
//! the end of the line, inserting or deleting characters -- and `curscr`
//! updated to match. Before that, lines that moved are scrolled into place
//! ([`crate::hashmap`]) and blank lines at the bottom cleared at once.
//!
//! The terminal, `newscr` and `curscr` are passed separately: the update
//! reads `newscr` (the cursor optimiser looks at the characters it might
//! write over), writes `curscr`, and writes the terminal, all at once.

use crate::addch::Ctype;
use crate::caps::{boolean, string};
use crate::cell::{
    A_ALTCHARSET, A_COLOR, A_NORMAL, Attr, BLANK_TEXT, CCHARW_MAX, Cell, NONBLANK_ATTR,
};
use crate::hashmap::HashState;
pub use crate::term::ACS_LEN;
use crate::term::{INFINITY, Term};
use crate::window::{NOCHANGE, Window};

/// `blankchar` and `normal`: a plain blank.
const fn blank() -> Cell {
    Cell::new2(BLANK_TEXT, A_NORMAL)
}

/// `BCE_ATTRS`.
const BCE_ATTRS: Attr = A_NORMAL | A_COLOR;

/// `CHECK_INTERVAL`: lines between looks at pending input.
const CHECK_INTERVAL: i32 = 5;

/// `ClrBlank (win)`: a blank as clearing leaves it -- with the background's
/// colour on a terminal that erases in it (`bce`). The background is
/// `stdscr`'s for `curscr` and `stdscr` alike, the only windows asked.
#[must_use]
pub fn clr_blank(t: &Term, stdscr_bkgd: &Cell) -> Cell {
    let mut b = blank();
    if t.flag(boolean::BACK_COLOR_ERASE) {
        b.add_attr(stdscr_bkgd.attr & BCE_ATTRS);
    }
    b
}

/// `FILL_BCE (sp)`: colours are on, not the terminal's own, and the
/// terminal does not erase in the background colour -- so a scroll's new
/// lines are written over with blanks.
fn fill_bce(t: &Term) -> bool {
    t.coloron && !t.default_color && !t.flag(boolean::BACK_COLOR_ERASE)
}

/// `has_ic ()`.
#[must_use]
pub fn has_ic(t: &Term) -> bool {
    (t.has(string::INSERT_CHARACTER)
        || t.has(string::PARM_ICH)
        || (t.has(string::ENTER_INSERT_MODE) && t.has(string::EXIT_INSERT_MODE)))
        && (t.has(string::DELETE_CHARACTER) || t.has(string::PARM_DCH))
}

/// `DelCharCost (sp, count)`.
fn del_char_cost(t: &Term, count: i32) -> i32 {
    if t.has(string::PARM_DCH) {
        t.costs.dch_cost
    } else if t.has(string::DELETE_CHARACTER) {
        t.costs.dch1_cost.wrapping_mul(count)
    } else {
        INFINITY
    }
}

/// `InsCharCost (sp, count)`.
fn ins_char_cost(t: &Term, count: i32) -> i32 {
    if t.has(string::PARM_ICH) {
        t.costs.ich_cost
    } else if t.has(string::ENTER_INSERT_MODE) && t.has(string::EXIT_INSERT_MODE) {
        t.costs
            .smir_cost
            .wrapping_add(t.costs.rmir_cost)
            .wrapping_add(t.costs.ip_cost.wrapping_mul(count))
    } else if t.has(string::INSERT_CHARACTER) {
        t.costs
            .ich1_cost
            .wrapping_add(t.costs.ip_cost)
            .wrapping_mul(count)
    } else {
        INFINITY
    }
}

/// `GoTo (row, col)`: the cursor moved there.
pub fn go_to(t: &mut Term, newscr: &Window, ctype: &dyn Ctype, row: i32, col: i32) {
    let (r, c) = (t.cursrow, t.curscol);
    t.mvcur(newscr, ctype, r, c, row, col);
}

/// `Charable (ch)`.
fn charable(t: &Term, ctype: &dyn Ctype, ch: &Cell) -> bool {
    (t.legacy_coding != 0 || ch.attr & A_ALTCHARSET != 0 || !ch.is_widec_ext())
        && ch.chars[1] == 0
        && ctype.wctob(ch.ch()) == ch.ch()
}

/// `PUTC (ch)`: the cell's characters, as the bytes the locale writes them
/// in -- the one byte when it is one, else each character through
/// `wcrtomb`, stopping at the first it cannot write.
fn putc(t: &mut Term, ctype: &dyn Ctype, ch: &Cell) {
    if ch.is_widec_ext() {
        return;
    }
    if charable(t, ctype, ch) {
        t.outch(ch.ch().to_le_bytes()[0]);
        return;
    }
    for (i, &wc) in ch.chars.iter().enumerate().take(CCHARW_MAX) {
        if wc == 0 {
            break;
        }
        match ctype.wcrtomb(wc) {
            Some(bytes) if !bytes.is_empty() => {
                for b in bytes {
                    t.outch(b);
                }
            }
            _ => {
                if wc.cast_unsigned() <= 255 && i == 0 {
                    t.outch(ch.ch().to_le_bytes()[0]);
                }
                break;
            }
        }
    }
}

/// `PutAttrChar (ch)`: the character written with its attributes, the
/// alternate character set and `~` on a `hz` terminal seen to, and the
/// cursor's column moved on by its width.
fn put_attr_char(t: &mut Term, ctype: &dyn Ctype, ch: &Cell) {
    let mut ch = *ch;
    let mut attr = ch;
    // "If this is not a valid character, there is nothing more to do."
    if ch.is_widec_ext() {
        return;
    }
    // "Determine the number of character cells which the 'ch' value will
    // use on the screen. It should be at least one."
    let mut chlen = ctype.wcwidth(ch.ch());
    if chlen <= 0 {
        let c = ch.ch();
        let in_acs = attr.attr & A_ALTCHARSET != 0
            && ((usize::try_from(c).is_ok_and(|c| c < ACS_LEN) && acs_mapped(t, c)) || c >= 128);
        if !(c.cast_unsigned() <= 255
            && (ctype.isprint(c)
                || (t.legacy_coding > 0 && c >= 160)
                || (t.legacy_coding > 1 && c >= 128)
                || in_acs))
        {
            ch = blank();
        }
        chlen = 1;
    }
    if attr.attr & A_ALTCHARSET != 0
        && !t.acs_map.is_empty()
        && usize::try_from(ch.ch()).is_ok_and(|c| c < ACS_LEN)
    {
        let c8 = usize::try_from(ch.ch()).unwrap_or(0);
        let mut my_ch = ch;
        let wacs = t.wacs.get(c8).copied().unwrap_or_default();
        if t.screen_unicode && wacs.chars[0] != 0 {
            if t.screen_acs_map.get(c8).copied().unwrap_or(false) {
                if t.screen_acs_fix {
                    attr.rem_attr(A_ALTCHARSET);
                    my_ch = wacs;
                }
            } else {
                attr.rem_attr(A_ALTCHARSET);
                my_ch = wacs;
            }
        } else if !t.screen_acs_map.get(c8).copied().unwrap_or(false) {
            // "If we found no mapping for a given alternate-character set
            // item in the terminal description, attempt to use the ASCII
            // fallback code which is populated in the _acs_map[] array."
            let temp = t.acs_map.get(c8).copied().unwrap_or(0) & 0xff;
            if temp != 0 {
                attr.rem_attr(A_ALTCHARSET);
                my_ch.set_char(temp.cast_signed(), attr.attr);
            }
        }
        // "If we (still) have alternate character set, it is the normal 8bit
        // flavor."
        if attr.attr & A_ALTCHARSET != 0 {
            let j = usize::try_from(ch.ch()).unwrap_or(0);
            let temp = t.acs_map.get(j).copied().unwrap_or(0) & 0xff;
            if temp != 0 {
                my_ch.set_char(temp.cast_signed(), attr.attr);
            } else {
                my_ch = ch;
                attr.rem_attr(A_ALTCHARSET);
            }
        }
        ch = my_ch;
    }
    if t.flag(boolean::TILDE_GLITCH) && ch.ch() == 0x7e {
        ch = Cell::with_char(0x60, attr.attr);
    }
    t.update_attrs(&attr);
    putc(t, ctype, &ch);
    t.curscol = t.curscol.wrapping_add(chlen);
    if let Some(cp) = t.s(string::CHAR_PADDING) {
        t.putp(Some(&cp));
    }
}

/// Whether the terminal's `acsc` maps `c`: `_acs_map[c] != 0`.
fn acs_mapped(t: &Term, c: i32) -> bool {
    usize::try_from(c)
        .ok()
        .and_then(|c| t.acs_map.get(c))
        .is_some_and(|&m| m != 0)
}

/// `PutCharLR (ch)`: the lower-right corner, written without scrolling the
/// screen where the terminal allows.
fn put_char_lr(t: &mut Term, newscr: &Window, ctype: &dyn Ctype, ch: &Cell) {
    if !t.flag(boolean::AUTO_RIGHT_MARGIN) {
        // "we can put the char directly"
        put_attr_char(t, ctype, ch);
    } else if t.has(string::ENTER_AM_MODE) && t.has(string::EXIT_AM_MODE) {
        let oldcol = t.curscol;
        // "we can suppress automargin"
        t.putp_cap(string::EXIT_AM_MODE);
        put_attr_char(t, ctype, ch);
        t.curscol = oldcol;
        t.putp_cap(string::ENTER_AM_MODE);
    } else if (t.has(string::ENTER_INSERT_MODE) && t.has(string::EXIT_INSERT_MODE))
        || t.has(string::INSERT_CHARACTER)
        || t.has(string::PARM_ICH)
    {
        let (lines, cols) = (t.lines, t.columns);
        go_to(
            t,
            newscr,
            ctype,
            lines.wrapping_sub(1),
            cols.wrapping_sub(2),
        );
        put_attr_char(t, ctype, ch);
        go_to(
            t,
            newscr,
            ctype,
            lines.wrapping_sub(1),
            cols.wrapping_sub(2),
        );
        let start = usize::try_from(cols.wrapping_sub(2)).unwrap_or(0);
        let tail: Vec<Cell> = newscr
            .line(lines.wrapping_sub(1))
            .map(|l| l.text.get(start..).unwrap_or_default().to_vec())
            .unwrap_or_default();
        ins_str(t, ctype, &tail, 1);
    }
}

/// `wrap_cursor ()`: past the right edge.
fn wrap_cursor(t: &mut Term) {
    if t.flag(boolean::EAT_NEWLINE_GLITCH) {
        // "it is safe to just tell the code that the cursor is in
        // hyperspace and let the next mvcur() call straighten things out."
        t.curscol = -1;
        t.cursrow = -1;
    } else if t.flag(boolean::AUTO_RIGHT_MARGIN) {
        t.curscol = 0;
        t.cursrow = t.cursrow.wrapping_add(1);
        // "We've actually moved - but may have to work around problems with
        // video attributes not working."
        if !t.flag(boolean::MOVE_STANDOUT_MODE) && t.current_attr.attr != 0 {
            t.vid_puts(A_NORMAL, 0);
        }
    } else {
        t.curscol = t.curscol.wrapping_sub(1);
    }
}

/// `PutChar (ch)`: a character written, the margin seen to.
fn put_char(t: &mut Term, newscr: &Window, ctype: &dyn Ctype, ch: &Cell) {
    if t.cursrow == t.lines.wrapping_sub(1) && t.curscol == t.columns.wrapping_sub(1) {
        put_char_lr(t, newscr, ctype, ch);
    } else {
        put_attr_char(t, ctype, ch);
    }
    if t.curscol >= t.columns {
        wrap_cursor(t);
    }
}

/// `isDefaultColor (c)`.
const fn is_default_color(c: i32) -> bool {
    c < 0
}

/// `can_clear_with (ch)`: a clearing command can produce this cell.
fn can_clear_with(t: &mut Term, ch: &Cell) -> bool {
    if !t.flag(boolean::BACK_COLOR_ERASE) && t.coloron {
        if !t.default_color {
            return false;
        }
        if !(is_default_color(t.default_fg) && is_default_color(t.default_bg)) {
            return false;
        }
        let pair = ch.pair();
        if pair != 0 {
            match t.pair_content(pair) {
                Some((fg, bg)) if is_default_color(fg) && is_default_color(bg) => {}
                _ => return false,
            }
        }
    }
    ch.is_blank() && ch.attr & !(NONBLANK_ATTR | A_COLOR) == A_NORMAL
}

/// `EmitRange (ntext, num)`: `num` cells written, with `ech` or `rep` for a
/// run of one where they are cheaper. `true` when the cursor was left inside
/// the range.
fn emit_range(t: &mut Term, newscr: &Window, ctype: &dyn Ctype, ntext: &[Cell]) -> bool {
    let at = |i: usize| ntext.get(i).copied().unwrap_or_default();
    let mut num = ntext.len();
    if t.has(string::ERASE_CHARS) || t.has(string::REPEAT_CHAR) {
        let mut off = 0usize;
        while num > 0 {
            while num > 1 && at(off) != at(off.saturating_add(1)) {
                put_char(t, newscr, ctype, &at(off));
                off = off.saturating_add(1);
                num = num.saturating_sub(1);
            }
            let ntext0 = at(off);
            if num == 1 {
                put_char(t, newscr, ctype, &ntext0);
                return false;
            }
            let mut runcount = 2usize;
            while runcount < num && at(off.saturating_add(runcount)) == ntext0 {
                runcount = runcount.saturating_add(1);
            }
            let run = i32::try_from(runcount).unwrap_or(i32::MAX);
            // "The cost expression in the middle isn't exactly right."
            if t.has(string::ERASE_CHARS)
                && run > t.costs.ech_cost.wrapping_add(t.costs.cup_ch_cost)
                && can_clear_with(t, &ntext0)
            {
                t.update_attrs(&ntext0);
                let s = t.tiparm(string::ERASE_CHARS, &[i64::from(run)]);
                t.putp(s.as_deref());
                // "If this is the last part of the given interval, don't
                // bother moving cursor, since it can be the last update on
                // the line."
                if runcount < num {
                    let (r, c) = (t.cursrow, t.curscol.wrapping_add(run));
                    go_to(t, newscr, ctype, r, c);
                } else {
                    return true;
                }
            } else if t.has(string::REPEAT_CHAR)
                && !t.screen_unicode
                && usize::try_from(ntext0.ch()).is_ok_and(|c| {
                    c < if ntext0.attr & A_ALTCHARSET != 0 {
                        ACS_LEN
                    } else {
                        256
                    }
                })
                && run > t.costs.rep_cost
            {
                let wrap_possible = t.curscol.wrapping_add(run) >= t.columns;
                let mut rep_count = run;
                if wrap_possible {
                    rep_count = rep_count.wrapping_sub(1);
                }
                t.update_attrs(&ntext0);
                let mut temp = ntext0;
                if ntext0.attr & A_ALTCHARSET != 0 {
                    let mapped = usize::try_from(temp.ch())
                        .ok()
                        .and_then(|c| t.acs_map.get(c))
                        .copied()
                        .unwrap_or(0)
                        & 0xff;
                    if mapped != 0 {
                        temp.set_char(mapped.cast_signed(), ntext0.attr | A_ALTCHARSET);
                    }
                }
                let s = t.tiparm(
                    string::REPEAT_CHAR,
                    &[i64::from(temp.ch()), i64::from(rep_count)],
                );
                if let Some(s) = s {
                    t.tputs(&s, 1);
                }
                t.curscol = t.curscol.wrapping_add(rep_count);
                if wrap_possible {
                    put_char(t, newscr, ctype, &ntext0);
                }
            } else {
                for i in 0..runcount {
                    put_char(t, newscr, ctype, &at(off.saturating_add(i)));
                }
            }
            off = off.saturating_add(runcount);
            num = num.saturating_sub(runcount);
        }
        return false;
    }
    for c in ntext {
        put_char(t, newscr, ctype, c);
    }
    false
}

/// `PutRange (otext, ntext, row, first, last)`: columns `first` to `last`
/// of a line written, jumping over a long enough stretch that is already
/// right. `true` when the cursor was left inside the range.
#[allow(clippy::too_many_arguments)]
fn put_range(
    t: &mut Term,
    newscr: &Window,
    ctype: &dyn Ctype,
    otext: Option<&[Cell]>,
    ntext: &[Cell],
    row: i32,
    first: i32,
    last: i32,
) -> bool {
    let cell = |s: &[Cell], i: i32| {
        usize::try_from(i)
            .ok()
            .and_then(|i| s.get(i))
            .copied()
            .unwrap_or_default()
    };
    let slice = |from: i32, count: i32| -> Vec<Cell> {
        let from = usize::try_from(from).unwrap_or(0);
        let count = usize::try_from(count).unwrap_or(0);
        ntext.iter().skip(from).take(count).copied().collect()
    };
    if let Some(otext) = otext
        && last.wrapping_sub(first).wrapping_add(1) > t.costs.inline_cost
    {
        let mut first = first;
        let mut same = 0i32;
        let mut j = first;
        while j <= last {
            if same == 0 && cell(otext, j).is_widec_ext() {
                j = j.wrapping_add(1);
                continue;
            }
            if cell(otext, j) == cell(ntext, j) {
                same = same.wrapping_add(1);
            } else {
                if same > t.costs.inline_cost {
                    let run = slice(first, j.wrapping_sub(same).wrapping_sub(first));
                    emit_range(t, newscr, ctype, &run);
                    first = j;
                    go_to(t, newscr, ctype, row, first);
                }
                same = 0;
            }
            j = j.wrapping_add(1);
        }
        let run = slice(first, j.wrapping_sub(same).wrapping_sub(first));
        let i = emit_range(t, newscr, ctype, &run);
        // "Always return 1 for the next GoTo() after a PutRange() if we
        // found identical characters at end of interval"
        if same == 0 { i } else { true }
    } else {
        let run = slice(first, last.wrapping_sub(first).wrapping_add(1));
        emit_range(t, newscr, ctype, &run)
    }
}

/// `ClrToEOL (blank, needclear)`: the rest of the cursor's line cleared,
/// with `el` when that is cheaper than blanks.
fn clr_to_eol(
    t: &mut Term,
    newscr: &Window,
    curscr: &mut Window,
    ctype: &dyn Ctype,
    blank: Cell,
    needclear: bool,
) {
    let mut needclear = needclear;
    if t.cursrow >= 0 {
        let row = t.cursrow;
        let cols = t.columns;
        if let Some(line) = curscr.line_mut(row) {
            for j in t.curscol..cols {
                if j >= 0
                    && let Some(cp) = line.at_mut(j)
                    && *cp != blank
                {
                    *cp = blank;
                    needclear = true;
                }
            }
        }
    }
    if needclear {
        t.update_attrs(&blank);
        if t.has(string::CLR_EOL) && t.costs.el_cost <= t.columns.wrapping_sub(t.curscol) {
            t.putp_cap(string::CLR_EOL);
        } else {
            let count = t.columns.wrapping_sub(t.curscol);
            for _ in 0..count.max(0) {
                put_char(t, newscr, ctype, &blank);
            }
        }
    }
}

/// `ClrToEOS (blank)`: from the cursor to the end of the screen cleared.
fn clr_to_eos(t: &mut Term, curscr: &mut Window, blank: Cell) {
    let mut row = t.cursrow.max(0);
    let mut col = t.curscol.max(0);
    t.update_attrs(&blank);
    if let Some(ed) = t.s(string::CLR_EOS) {
        t.tputs(&ed, t.lines.wrapping_sub(row));
    }
    while col < t.columns {
        if let Some(c) = curscr.line_mut(row).and_then(|l| l.at_mut(col)) {
            *c = blank;
        }
        col = col.wrapping_add(1);
    }
    row = row.wrapping_add(1);
    while row < t.lines {
        if let Some(line) = curscr.line_mut(row) {
            line.text.fill(blank);
        }
        row = row.wrapping_add(1);
    }
}

/// `ClrBottom (total)`: when the last lines are to be blank, clear them
/// with `ed` at once. The first line not so cleared.
fn clr_bottom(
    t: &mut Term,
    newscr: &Window,
    curscr: &mut Window,
    hash: &mut HashState,
    ctype: &dyn Ctype,
    total: i32,
) -> i32 {
    let mut top = total;
    let last = t.columns.min(i32::from(newscr.maxx).wrapping_add(1));
    let blank = newscr.cell(total.wrapping_sub(1), last.wrapping_sub(1));
    if t.has(string::CLR_EOS) && can_clear_with(t, &blank) {
        let mut row = total.wrapping_sub(1);
        while row >= 0 {
            let all = |w: &Window| (0..last).all(|col| w.cell(row, col) == blank);
            if !all(newscr) {
                break;
            }
            if !all(curscr) {
                top = row;
            }
            row = row.wrapping_sub(1);
        }
        // "don't use clr_eos for just one line if clr_eol available"
        if top < total {
            go_to(t, newscr, ctype, top, 0);
            clr_to_eos(t, curscr, blank);
            if hash.oldhash.is_some() && hash.newhash.is_some() {
                hash.copy_new_to_old(top, t.lines);
            }
        }
    }
    top
}

/// `ClearScreen (blank)`: the whole screen cleared, the cursor home.
fn clear_screen(
    t: &mut Term,
    newscr: &Window,
    curscr: &mut Window,
    ctype: &dyn Ctype,
    blank: Cell,
) {
    let mut fast_clear =
        t.has(string::CLEAR_SCREEN) || t.has(string::CLR_EOS) || t.has(string::CLR_EOL);
    if t.coloron && !t.default_color {
        let pair = t.current_attr.pair();
        t.do_color(pair, 0, false);
        if !t.flag(boolean::BACK_COLOR_ERASE) {
            fast_clear = false;
        }
    }
    if fast_clear {
        if t.has(string::CLEAR_SCREEN) {
            t.update_attrs(&blank);
            t.putp_cap(string::CLEAR_SCREEN);
            t.cursrow = 0;
            t.curscol = 0;
        } else if t.has(string::CLR_EOS) {
            t.cursrow = -1;
            t.curscol = -1;
            go_to(t, newscr, ctype, 0, 0);
            t.update_attrs(&blank);
            if let Some(ed) = t.s(string::CLR_EOS) {
                t.tputs(&ed, t.lines);
            }
        } else {
            t.cursrow = -1;
            t.curscol = -1;
            t.update_attrs(&blank);
            for i in 0..t.lines {
                go_to(t, newscr, ctype, i, 0);
                t.putp_cap(string::CLR_EOL);
            }
            go_to(t, newscr, ctype, 0, 0);
        }
    } else {
        t.update_attrs(&blank);
        for i in 0..t.lines {
            go_to(t, newscr, ctype, i, 0);
            for _ in 0..t.columns {
                put_char(t, newscr, ctype, &blank);
            }
        }
        go_to(t, newscr, ctype, 0, 0);
    }
    for line in &mut curscr.lines {
        line.text.fill(blank);
    }
}

/// `InsStr (line, count)`: `count` characters inserted at the cursor.
fn ins_str(t: &mut Term, ctype: &dyn Ctype, line: &[Cell], count: i32) {
    let cells = line.iter().take(usize::try_from(count).unwrap_or(0));
    // "Prefer parm_ich as it has the smallest cost - no need to shift the
    // whole line on each character."
    if t.has(string::PARM_ICH) {
        if let Some(s) = t.tiparm(string::PARM_ICH, &[i64::from(count)]) {
            t.tputs(&s, 1);
        }
        for c in cells {
            put_attr_char(t, ctype, c);
        }
    } else if t.has(string::ENTER_INSERT_MODE) && t.has(string::EXIT_INSERT_MODE) {
        t.putp_cap(string::ENTER_INSERT_MODE);
        for c in cells {
            put_attr_char(t, ctype, c);
            t.putp_cap(string::INSERT_PADDING);
        }
        t.putp_cap(string::EXIT_INSERT_MODE);
    } else {
        for c in cells {
            t.putp_cap(string::INSERT_CHARACTER);
            put_attr_char(t, ctype, c);
            t.putp_cap(string::INSERT_PADDING);
        }
    }
}

/// `DelChar (count)`: `count` characters deleted at the cursor.
fn del_char(t: &mut Term, count: i32) {
    if t.has(string::PARM_DCH) {
        if let Some(s) = t.tiparm(string::PARM_DCH, &[i64::from(count)]) {
            t.tputs(&s, 1);
        }
    } else {
        for _ in 0..count.max(0) {
            t.putp_cap(string::DELETE_CHARACTER);
        }
    }
}

/// `TransformLine (lineno)`: line `lineno` of `curscr` made `newscr`'s.
#[allow(clippy::too_many_lines)]
pub fn transform_line(
    t: &mut Term,
    newscr: &Window,
    curscr: &mut Window,
    hash: &mut HashState,
    ctype: &dyn Ctype,
    stdscr_bkgd: &Cell,
    lineno: i32,
) {
    let cols = t.columns;
    // "copy new hash value to old one"
    hash.copy_one(lineno);
    let new_line: Vec<Cell> = newscr
        .line(lineno)
        .map(|l| l.text.clone())
        .unwrap_or_default();
    let nl = |n: i32| {
        usize::try_from(n)
            .ok()
            .and_then(|n| new_line.get(n))
            .copied()
            .unwrap_or_default()
    };
    let ol = |c: &Window, n: i32| c.cell(lineno, n);

    // "If we have colors, there is the possibility of having two color
    // pairs that display as the same colors."
    if t.coloron {
        for n in 0..cols {
            let (newc, oldc) = (nl(n), ol(curscr, n));
            if newc != oldc {
                let old_pair = oldc.pair();
                let new_pair = newc.pair();
                let alloc = i32::try_from(t.color_pairs.len()).unwrap_or(i32::MAX);
                if old_pair != new_pair
                    && oldc.uncolored() == newc.uncolored()
                    && old_pair < alloc
                    && new_pair < alloc
                {
                    let (a, b) = (t.color_pair(old_pair), t.color_pair(new_pair));
                    if a.fg == b.fg
                        && a.bg == b.bg
                        && let Some(c) = curscr.line_mut(lineno).and_then(|l| l.at_mut(n))
                    {
                        c.set_pair(new_pair);
                    }
                }
            }
        }
    }

    let mut attrchanged = false;
    if t.flag(boolean::CEOL_STANDOUT_GLITCH) && t.has(string::CLR_EOL) {
        for n in 0..cols {
            if !nl(n).same_attr(&ol(curscr, n)) {
                attrchanged = true;
                break;
            }
        }
    }

    let mut first_char = 0i32;
    if attrchanged {
        // "we may have to disregard the whole line"
        go_to(t, newscr, ctype, lineno, first_char);
        let b = clr_blank(t, stdscr_bkgd);
        clr_to_eol(t, newscr, curscr, ctype, b, false);
        let old: Vec<Cell> = curscr
            .line(lineno)
            .map(|l| l.text.clone())
            .unwrap_or_default();
        put_range(
            t,
            newscr,
            ctype,
            Some(&old),
            &new_line,
            lineno,
            0,
            cols.wrapping_sub(1),
        );
    } else {
        // "it may be cheap to clear leading whitespace with clr_bol"
        let blank0 = nl(0);
        if t.has(string::CLR_BOL) && can_clear_with(t, &blank0) {
            let mut o_first = 0;
            while o_first < cols && ol(curscr, o_first) == blank0 {
                o_first = o_first.wrapping_add(1);
            }
            let mut n_first = 0;
            while n_first < cols && nl(n_first) == blank0 {
                n_first = n_first.wrapping_add(1);
            }
            match n_first.cmp(&o_first) {
                std::cmp::Ordering::Equal => {
                    first_char = n_first;
                    // "find the first differing character"
                    while first_char < cols && nl(first_char) == ol(curscr, first_char) {
                        first_char = first_char.wrapping_add(1);
                    }
                }
                std::cmp::Ordering::Less => first_char = n_first,
                std::cmp::Ordering::Greater => {
                    first_char = o_first;
                    if t.costs.el1_cost < n_first.wrapping_sub(o_first) {
                        if n_first >= cols && t.costs.el_cost <= t.costs.el1_cost {
                            go_to(t, newscr, ctype, lineno, 0);
                            t.update_attrs(&blank0);
                            t.putp_cap(string::CLR_EOL);
                        } else {
                            go_to(t, newscr, ctype, lineno, n_first.wrapping_sub(1));
                            t.update_attrs(&blank0);
                            t.putp_cap(string::CLR_BOL);
                        }
                        while first_char < n_first {
                            if let Some(c) =
                                curscr.line_mut(lineno).and_then(|l| l.at_mut(first_char))
                            {
                                *c = blank0;
                            }
                            first_char = first_char.wrapping_add(1);
                        }
                    }
                }
            }
        } else {
            // "find the first differing character"
            while first_char < cols && nl(first_char) == ol(curscr, first_char) {
                first_char = first_char.wrapping_add(1);
            }
        }
        // "if there wasn't one, we're done"
        if first_char >= cols {
            return;
        }

        let blank = nl(cols.wrapping_sub(1));
        if !can_clear_with(t, &blank) {
            // "find the last differing character"
            let mut n_last = cols.wrapping_sub(1);
            while n_last > first_char && nl(n_last) == ol(curscr, n_last) {
                n_last = n_last.wrapping_sub(1);
            }
            if n_last >= first_char {
                go_to(t, newscr, ctype, lineno, first_char);
                let old: Vec<Cell> = curscr
                    .line(lineno)
                    .map(|l| l.text.clone())
                    .unwrap_or_default();
                put_range(
                    t,
                    newscr,
                    ctype,
                    Some(&old),
                    &new_line,
                    lineno,
                    first_char,
                    n_last,
                );
                copy_cells(
                    curscr,
                    lineno,
                    &new_line,
                    first_char,
                    n_last.wrapping_add(1),
                );
            }
            return;
        }

        // "find last non-blank character on old line"
        let mut o_last = cols.wrapping_sub(1);
        while o_last > first_char && ol(curscr, o_last) == blank {
            o_last = o_last.wrapping_sub(1);
        }
        // "find last non-blank character on new line"
        let mut n_last = cols.wrapping_sub(1);
        while n_last > first_char && nl(n_last) == blank {
            n_last = n_last.wrapping_sub(1);
        }

        if n_last == first_char && t.costs.el_cost < o_last.wrapping_sub(n_last) {
            go_to(t, newscr, ctype, lineno, first_char);
            if nl(first_char) != blank {
                put_char(t, newscr, ctype, &nl(first_char));
            }
            clr_to_eol(t, newscr, curscr, ctype, blank, false);
        } else if n_last != o_last && (nl(n_last) != ol(curscr, o_last) || !(t.idcok && has_ic(t)))
        {
            go_to(t, newscr, ctype, lineno, first_char);
            let old: Vec<Cell> = curscr
                .line(lineno)
                .map(|l| l.text.clone())
                .unwrap_or_default();
            if o_last.wrapping_sub(n_last) > t.costs.el_cost {
                if put_range(
                    t,
                    newscr,
                    ctype,
                    Some(&old),
                    &new_line,
                    lineno,
                    first_char,
                    n_last,
                ) {
                    go_to(t, newscr, ctype, lineno, n_last.wrapping_add(1));
                }
                clr_to_eol(t, newscr, curscr, ctype, blank, false);
            } else {
                let n = n_last.max(o_last);
                put_range(
                    t,
                    newscr,
                    ctype,
                    Some(&old),
                    &new_line,
                    lineno,
                    first_char,
                    n,
                );
            }
        } else {
            let n_last_nonblank = n_last;
            let o_last_nonblank = o_last;
            // "find the last characters that really differ"
            // "can be -1 if no characters differ"
            while nl(n_last) == ol(curscr, o_last) {
                // "don't split a wide char"
                if nl(n_last).is_widec_ext()
                    && nl(n_last.wrapping_sub(1)) != ol(curscr, o_last.wrapping_sub(1))
                {
                    break;
                }
                n_last = n_last.wrapping_sub(1);
                o_last = o_last.wrapping_sub(1);
                if n_last == -1 || o_last == -1 {
                    break;
                }
            }
            let mut n = o_last.min(n_last);
            if n >= first_char {
                go_to(t, newscr, ctype, lineno, first_char);
                let old: Vec<Cell> = curscr
                    .line(lineno)
                    .map(|l| l.text.clone())
                    .unwrap_or_default();
                put_range(
                    t,
                    newscr,
                    ctype,
                    Some(&old),
                    &new_line,
                    lineno,
                    first_char,
                    n,
                );
            }
            if o_last < n_last {
                let m = n_last_nonblank.max(o_last_nonblank);
                if n != 0 {
                    while nl(n.wrapping_add(1)).is_widec_ext() && n != 0 {
                        n = n.wrapping_sub(1);
                        o_last = o_last.wrapping_sub(1);
                    }
                } else if n >= first_char && nl(n).is_widec_base() {
                    while nl(n.wrapping_add(1)).is_widec_ext() {
                        n = n.wrapping_add(1);
                        o_last = o_last.wrapping_add(1);
                    }
                }
                go_to(t, newscr, ctype, lineno, n.wrapping_add(1));
                if n_last < n_last_nonblank
                    || ins_char_cost(t, n_last.wrapping_sub(o_last)) > m.wrapping_sub(n)
                {
                    let old: Vec<Cell> = curscr
                        .line(lineno)
                        .map(|l| l.text.clone())
                        .unwrap_or_default();
                    put_range(
                        t,
                        newscr,
                        ctype,
                        Some(&old),
                        &new_line,
                        lineno,
                        n.wrapping_add(1),
                        m,
                    );
                } else {
                    let from = usize::try_from(n.wrapping_add(1)).unwrap_or(0);
                    let tail = new_line.get(from..).unwrap_or_default().to_vec();
                    ins_str(t, ctype, &tail, n_last.wrapping_sub(o_last));
                }
            } else if o_last > n_last {
                go_to(t, newscr, ctype, lineno, n.wrapping_add(1));
                if del_char_cost(t, o_last.wrapping_sub(n_last))
                    > t.costs
                        .el_cost
                        .wrapping_add(n_last_nonblank)
                        .wrapping_sub(n.wrapping_add(1))
                {
                    let old: Vec<Cell> = curscr
                        .line(lineno)
                        .map(|l| l.text.clone())
                        .unwrap_or_default();
                    if put_range(
                        t,
                        newscr,
                        ctype,
                        Some(&old),
                        &new_line,
                        lineno,
                        n.wrapping_add(1),
                        n_last_nonblank,
                    ) {
                        go_to(t, newscr, ctype, lineno, n_last_nonblank.wrapping_add(1));
                    }
                    clr_to_eol(t, newscr, curscr, ctype, blank, false);
                } else {
                    // "The delete-char sequence will effectively shift in
                    // blanks from the right margin of the screen. Ensure that
                    // they are the right color by setting the video
                    // attributes from the last character on the row."
                    t.update_attrs(&blank);
                    del_char(t, o_last.wrapping_sub(n_last));
                }
            }
        }
    }
    // "update the code's internal representation"
    if cols > first_char {
        copy_cells(curscr, lineno, &new_line, first_char, cols);
    }
}

/// `memcpy (oldLine + from, newLine + from, …)`: columns `from` to `to`
/// (exclusive) of `curscr`'s line `lineno` made `new_line`'s.
fn copy_cells(curscr: &mut Window, lineno: i32, new_line: &[Cell], from: i32, to: i32) {
    if let Some(line) = curscr.line_mut(lineno) {
        for col in from.max(0)..to {
            if let (Some(dst), Some(src)) = (
                line.at_mut(col),
                usize::try_from(col).ok().and_then(|c| new_line.get(c)),
            ) {
                *dst = *src;
            }
        }
    }
}

/// `ClrUpdate ()`: the screen cleared and every line written afresh.
pub fn clr_update(
    t: &mut Term,
    newscr: &Window,
    curscr: &mut Window,
    hash: &mut HashState,
    ctype: &dyn Ctype,
    stdscr_bkgd: &Cell,
) {
    let blank = clr_blank(t, stdscr_bkgd);
    let nonempty = t.lines.min(i32::from(newscr.maxy).wrapping_add(1));
    clear_screen(t, newscr, curscr, ctype, blank);
    let nonempty = clr_bottom(t, newscr, curscr, hash, ctype, nonempty);
    for i in 0..nonempty {
        transform_line(t, newscr, curscr, hash, ctype, stdscr_bkgd, i);
    }
}

/// `MARK_NOCHANGE (win, row)`.
pub fn mark_nochange(win: &mut Window, row: i32) {
    if let Some(line) = win.line_mut(row) {
        line.firstchar = NOCHANGE;
        line.lastchar = NOCHANGE;
    }
}

/// `scroll_csr_forward (n, top, bot, miny, maxy, blank)`: rows `top` to
/// `bot` scrolled up `n`, given the scrolling region `miny` to `maxy`.
/// `false` (`ERR`) when the terminal has no way to.
#[allow(clippy::too_many_arguments)]
fn scroll_csr_forward(
    t: &mut Term,
    newscr: &Window,
    ctype: &dyn Ctype,
    n: i32,
    top: i32,
    bot: i32,
    miny: i32,
    maxy: i32,
    blank: Cell,
) -> bool {
    if n == 1 && t.has(string::SCROLL_FORWARD) && top == miny && bot == maxy {
        go_to(t, newscr, ctype, bot, 0);
        t.update_attrs(&blank);
        t.putp_cap(string::SCROLL_FORWARD);
    } else if n == 1 && t.has(string::DELETE_LINE) && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        t.putp_cap(string::DELETE_LINE);
    } else if t.has(string::PARM_INDEX) && top == miny && bot == maxy {
        go_to(t, newscr, ctype, bot, 0);
        t.update_attrs(&blank);
        if let Some(s) = t.tiparm(string::PARM_INDEX, &[i64::from(n)]) {
            t.tputs(&s, n);
        }
    } else if t.has(string::PARM_DELETE_LINE) && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        if let Some(s) = t.tiparm(string::PARM_DELETE_LINE, &[i64::from(n)]) {
            t.tputs(&s, n);
        }
    } else if t.has(string::SCROLL_FORWARD) && top == miny && bot == maxy {
        go_to(t, newscr, ctype, bot, 0);
        t.update_attrs(&blank);
        for _ in 0..n {
            t.putp_cap(string::SCROLL_FORWARD);
        }
    } else if t.has(string::DELETE_LINE) && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        for _ in 0..n {
            t.putp_cap(string::DELETE_LINE);
        }
    } else {
        return false;
    }
    if fill_bce(t) {
        for i in 0..n {
            go_to(t, newscr, ctype, bot.wrapping_sub(i), 0);
            for _ in 0..t.columns {
                put_char(t, newscr, ctype, &blank);
            }
        }
    }
    true
}

/// `scroll_csr_backward (n, top, bot, miny, maxy, blank)`: down `n`.
#[allow(clippy::too_many_arguments)]
fn scroll_csr_backward(
    t: &mut Term,
    newscr: &Window,
    ctype: &dyn Ctype,
    n: i32,
    top: i32,
    bot: i32,
    miny: i32,
    maxy: i32,
    blank: Cell,
) -> bool {
    if n == 1 && t.has(string::SCROLL_REVERSE) && top == miny && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        t.putp_cap(string::SCROLL_REVERSE);
    } else if n == 1 && t.has(string::INSERT_LINE) && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        t.putp_cap(string::INSERT_LINE);
    } else if t.has(string::PARM_RINDEX) && top == miny && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        if let Some(s) = t.tiparm(string::PARM_RINDEX, &[i64::from(n)]) {
            t.tputs(&s, n);
        }
    } else if t.has(string::PARM_INSERT_LINE) && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        if let Some(s) = t.tiparm(string::PARM_INSERT_LINE, &[i64::from(n)]) {
            t.tputs(&s, n);
        }
    } else if t.has(string::SCROLL_REVERSE) && top == miny && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        for _ in 0..n {
            t.putp_cap(string::SCROLL_REVERSE);
        }
    } else if t.has(string::INSERT_LINE) && bot == maxy {
        go_to(t, newscr, ctype, top, 0);
        t.update_attrs(&blank);
        for _ in 0..n {
            t.putp_cap(string::INSERT_LINE);
        }
    } else {
        return false;
    }
    if fill_bce(t) {
        for i in 0..n {
            go_to(t, newscr, ctype, top.wrapping_add(i), 0);
            for _ in 0..t.columns {
                put_char(t, newscr, ctype, &blank);
            }
        }
    }
    true
}

/// `scroll_idl (n, del, ins, blank)`: by deleting `n` lines at `del` and
/// inserting them at `ins`.
fn scroll_idl(
    t: &mut Term,
    newscr: &Window,
    ctype: &dyn Ctype,
    n: i32,
    del: i32,
    ins: i32,
    blank: Cell,
) -> bool {
    if !((t.has(string::PARM_DELETE_LINE) || t.has(string::DELETE_LINE))
        && (t.has(string::PARM_INSERT_LINE) || t.has(string::INSERT_LINE)))
    {
        return false;
    }
    go_to(t, newscr, ctype, del, 0);
    t.update_attrs(&blank);
    if n == 1 && t.has(string::DELETE_LINE) {
        t.putp_cap(string::DELETE_LINE);
    } else if t.has(string::PARM_DELETE_LINE) {
        if let Some(s) = t.tiparm(string::PARM_DELETE_LINE, &[i64::from(n)]) {
            t.tputs(&s, n);
        }
    } else {
        for _ in 0..n {
            t.putp_cap(string::DELETE_LINE);
        }
    }
    go_to(t, newscr, ctype, ins, 0);
    t.update_attrs(&blank);
    if n == 1 && t.has(string::INSERT_LINE) {
        t.putp_cap(string::INSERT_LINE);
    } else if t.has(string::PARM_INSERT_LINE) {
        if let Some(s) = t.tiparm(string::PARM_INSERT_LINE, &[i64::from(n)]) {
            t.tputs(&s, n);
        }
    } else {
        for _ in 0..n {
            t.putp_cap(string::INSERT_LINE);
        }
    }
    true
}

/// `_nc_scrolln (n, top, bot, maxy)`: the rows `top` to `bot` of the
/// terminal scrolled by `n` (up when positive), and `curscr` with them.
/// `false` (`ERR`) when the terminal cannot.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn scrolln(
    t: &mut Term,
    newscr: &Window,
    curscr: &mut Window,
    hash: &mut HashState,
    ctype: &dyn Ctype,
    stdscr_bkgd: &Cell,
    n: i32,
    top: i32,
    bot: i32,
    maxy: i32,
) -> bool {
    let blank = clr_blank(t, stdscr_bkgd);
    let mut cursor_saved = false;
    let res = if n > 0 {
        // "Explicitly clear if stuff pushed off top of region might be
        // saved by the terminal."
        let mut r = scroll_csr_forward(t, newscr, ctype, n, top, bot, 0, maxy, blank);
        if !r && t.has(string::CHANGE_SCROLL_REGION) {
            if ((n == 1 && t.has(string::SCROLL_FORWARD)) || t.has(string::PARM_INDEX))
                && (t.cursrow == bot || t.cursrow == bot.wrapping_sub(1))
                && t.has(string::SAVE_CURSOR)
                && t.has(string::RESTORE_CURSOR)
            {
                cursor_saved = true;
                t.putp_cap(string::SAVE_CURSOR);
            }
            let s = t.tiparm(
                string::CHANGE_SCROLL_REGION,
                &[i64::from(top), i64::from(bot)],
            );
            t.putp(s.as_deref());
            if cursor_saved {
                t.putp_cap(string::RESTORE_CURSOR);
            } else {
                t.cursrow = -1;
                t.curscol = -1;
            }
            r = scroll_csr_forward(t, newscr, ctype, n, top, bot, top, bot, blank);
            let s = t.tiparm(string::CHANGE_SCROLL_REGION, &[0, i64::from(maxy)]);
            t.putp(s.as_deref());
            t.cursrow = -1;
            t.curscol = -1;
        }
        if !r && t.idlok {
            r = scroll_idl(
                t,
                newscr,
                ctype,
                n,
                top,
                bot.wrapping_sub(n).wrapping_add(1),
                blank,
            );
        }
        // "Clear the newly shifted-in text."
        if r && (t.flag(boolean::NON_DEST_SCROLL_REGION)
            || (t.flag(boolean::MEMORY_BELOW) && bot == maxy))
        {
            let blank2 = Cell::blank();
            if bot == maxy && t.has(string::CLR_EOS) {
                go_to(t, newscr, ctype, bot.wrapping_sub(n).wrapping_add(1), 0);
                clr_to_eos(t, curscr, blank2);
            } else {
                for i in 0..n {
                    go_to(t, newscr, ctype, bot.wrapping_sub(i), 0);
                    clr_to_eol(t, newscr, curscr, ctype, blank2, false);
                }
            }
        }
        r
    } else {
        let mut r =
            scroll_csr_backward(t, newscr, ctype, n.wrapping_neg(), top, bot, 0, maxy, blank);
        if !r && t.has(string::CHANGE_SCROLL_REGION) {
            if top != 0
                && (t.cursrow == top || t.cursrow == top.wrapping_sub(1))
                && t.has(string::SAVE_CURSOR)
                && t.has(string::RESTORE_CURSOR)
            {
                cursor_saved = true;
                t.putp_cap(string::SAVE_CURSOR);
            }
            let s = t.tiparm(
                string::CHANGE_SCROLL_REGION,
                &[i64::from(top), i64::from(bot)],
            );
            t.putp(s.as_deref());
            if cursor_saved {
                t.putp_cap(string::RESTORE_CURSOR);
            } else {
                t.cursrow = -1;
                t.curscol = -1;
            }
            r = scroll_csr_backward(
                t,
                newscr,
                ctype,
                n.wrapping_neg(),
                top,
                bot,
                top,
                bot,
                blank,
            );
            let s = t.tiparm(string::CHANGE_SCROLL_REGION, &[0, i64::from(maxy)]);
            t.putp(s.as_deref());
            t.cursrow = -1;
            t.curscol = -1;
        }
        if !r && t.idlok {
            r = scroll_idl(
                t,
                newscr,
                ctype,
                n.wrapping_neg(),
                bot.wrapping_add(n).wrapping_add(1),
                top,
                blank,
            );
        }
        // "Clear the newly shifted-in text."
        if r && (t.flag(boolean::NON_DEST_SCROLL_REGION)
            || (t.flag(boolean::MEMORY_ABOVE) && top == 0))
        {
            let blank2 = Cell::blank();
            for i in 0..n.wrapping_neg() {
                go_to(t, newscr, ctype, i.wrapping_add(top), 0);
                clr_to_eol(t, newscr, curscr, ctype, blank2, false);
            }
        }
        r
    };
    if !res {
        return false;
    }
    curscr.scroll_window(n, top, bot, blank);
    // "shift hash values too - they can be reused"
    hash.scroll_oldhash(curscr, n, top, bot);
    true
}

/// `_nc_screen_resume ()`: the terminal in a known state -- attributes off,
/// a full repaint due, colours reset, `rmir`, the margin mode set.
pub fn screen_resume(t: &mut Term, newscr: &mut Window) {
    t.current_attr.set_attr(A_NORMAL);
    newscr.clear = true;
    if t.coloron || t.color_defs != 0 {
        t.reset_colors();
    }
    // "restore user-defined colors, if any"
    if t.color_defs < 0 && t.direct_color == (0, 0, 0) {
        t.color_defs = t.color_defs.wrapping_neg();
        let defs = usize::try_from(t.color_defs).unwrap_or(0);
        let inits: Vec<(i32, i32, i32, i32)> = t
            .color_table
            .iter()
            .take(defs)
            .enumerate()
            .filter(|(_, c)| c.init)
            .map(|(n, c)| (i32::try_from(n).unwrap_or(0), c.r, c.g, c.b))
            .collect();
        for (n, r, g, b) in inits {
            t.init_color(n, r, g, b);
        }
    }
    if t.has(string::EXIT_ATTRIBUTE_MODE) {
        t.putp_cap(string::EXIT_ATTRIBUTE_MODE);
    } else {
        // "turn off attributes"
        t.putp_cap(string::EXIT_ALT_CHARSET_MODE);
        t.putp_cap(string::EXIT_STANDOUT_MODE);
        t.putp_cap(string::EXIT_UNDERLINE_MODE);
    }
    t.putp_cap(string::EXIT_INSERT_MODE);
    if t.has(string::ENTER_AM_MODE) && t.has(string::EXIT_AM_MODE) {
        if t.flag(boolean::AUTO_RIGHT_MARGIN) {
            t.putp_cap(string::ENTER_AM_MODE);
        } else {
            t.putp_cap(string::EXIT_AM_MODE);
        }
    }
}

/// `_nc_screen_wrap ()`: attributes and colours put back for the shell.
pub fn screen_wrap(t: &mut Term, newscr: &Window, curscr: &mut Window, ctype: &dyn Ctype) {
    t.update_attrs(&blank());
    if t.coloron && !t.default_color {
        t.default_color = true;
        t.do_color(-1, 0, false);
        t.default_color = false;
        let (r, c, lines) = (t.cursrow, t.curscol, t.lines);
        t.mvcur(newscr, ctype, r, c, lines.wrapping_sub(1), 0);
        clr_to_eol(t, newscr, curscr, ctype, Cell::blank(), true);
    }
    if t.color_defs != 0 {
        t.reset_colors();
    }
}

/// `doupdate`'s work, once the screen is known to be in curses mode: a
/// repaint from scratch when either screen asks, else each changed line
/// transformed after the scroll optimiser has moved what moved; then the
/// cursor put where `newscr` has it and the attributes turned off.
#[allow(clippy::too_many_arguments)]
pub fn do_update(
    t: &mut Term,
    newscr: &mut Window,
    curscr: &mut Window,
    hash: &mut HashState,
    ctype: &dyn Ctype,
    stdscr_bkgd: &Cell,
) {
    let mut nonempty = 0;
    if curscr.clear || newscr.clear {
        // "force refresh ?"
        clr_update(t, newscr, curscr, hash, ctype, stdscr_bkgd);
        curscr.clear = false;
        newscr.clear = false;
    } else {
        let mut changedlines = CHECK_INTERVAL;
        // `check_pending ()` -- every `CHECK_INTERVAL` lines -- only flushes
        // what is buffered when input is waiting, and upstream goes on with
        // the update either way; the bytes are the same, so it is left out.
        nonempty = t.lines.min(i32::from(newscr.maxy).wrapping_add(1));
        if t.scrolling {
            crate::hashmap::scroll_optimize(t, newscr, curscr, hash, ctype, stdscr_bkgd);
        }
        nonempty = clr_bottom(t, newscr, curscr, hash, ctype, nonempty);
        for i in 0..nonempty {
            if changedlines == CHECK_INTERVAL {
                changedlines = 0;
            }
            // "newscr->line[i].firstchar is normally set by wnoutrefresh.
            // curscr->line[i].firstchar is normally set by _nc_scroll_window
            // in the vertical-movement optimization code"
            let n_changed = newscr.line(i).is_some_and(|l| l.firstchar != NOCHANGE);
            let c_changed = curscr.line(i).is_some_and(|l| l.firstchar != NOCHANGE);
            if n_changed || c_changed {
                transform_line(t, newscr, curscr, hash, ctype, stdscr_bkgd, i);
                changedlines = changedlines.wrapping_add(1);
            }
            // "mark line changed successfully"
            if i <= i32::from(newscr.maxy) {
                mark_nochange(newscr, i);
            }
            if i <= i32::from(curscr.maxy) {
                mark_nochange(curscr, i);
            }
        }
    }
    // "put everything back in sync"
    for i in nonempty..=i32::from(newscr.maxy) {
        mark_nochange(newscr, i);
    }
    for i in nonempty..=i32::from(curscr.maxy) {
        mark_nochange(curscr, i);
    }
    if !newscr.leaveok {
        curscr.curx = newscr.curx;
        curscr.cury = newscr.cury;
        go_to(
            t,
            newscr,
            ctype,
            i32::from(curscr.cury),
            i32::from(curscr.curx),
        );
    }
    // "We would like to keep the physical screen in normal mode in case we
    // get other processes writing to the screen."
    t.update_attrs(&blank());
    t.flush();
    curscr.attrs = newscr.attrs;
}
