//! Moving the cursor the cheapest way: `lib_mvcur.c`, and the bounded
//! string buffers of `strings.c` it builds its answers in.
//!
//! Every way the terminal offers -- absolute addressing (`cup`), moves
//! relative to where the cursor is (`cuu`, `cud1`, `hpa`, …), and those
//! after a carriage return, after `home`, after `ll`, or wrapping back from
//! the left margin -- is costed, and the cheapest sent. A cost is
//! milliseconds at the terminal's speed in tenths, from each string's length
//! and padding (`_nc_msec_cost`), in C's `float`.
//!
//! The strings are built in buffers of 512 bytes (`OPT_SIZE`) through
//! `string_desc`s, whose quirks are kept: resetting a descriptor rewinds
//! where the next append goes without clearing what was written past it,
//! and an append that would not fit is refused, which can make a move
//! impossible -- or lose the part already chosen -- exactly where
//! upstream's does.

use crate::addch::Ctype;
use crate::caps::{boolean, string};
use crate::cell::A_ALTCHARSET;
use crate::term::{INFINITY, Term};
use crate::window::Window;

/// `OPT_SIZE`.
const OPT_SIZE: usize = 512;
/// `BAUDBYTE`: 9 bits to a byte.
const BAUDBYTE: i32 = 9;
/// `MAX_DELAY_MSECS`.
const MAX_DELAY_MSECS: f32 = 30000.0;
/// `COMPUTE_OVERHEAD`, `LONG_DIST`.
const LONG_DIST: i32 = 8 - 1;

/// A string buffer and its `string_desc`.
#[derive(Clone, Copy)]
struct Desc {
    /// `s_tail`, as an index into the buffer.
    tail: usize,
    /// `s_size`: room left.
    size: usize,
    /// `s_init`: room at the start.
    init: usize,
    /// `s_head != 0`: whether anything is written, or only counted.
    head: bool,
}

/// `OPT_SIZE` bytes of buffer.
type Buf = [u8; OPT_SIZE];

/// `strlen (s)` of a byte string that may hold a NUL.
fn c_len(s: &[u8]) -> usize {
    s.iter().position(|&b| b == 0).unwrap_or(s.len())
}

/// `_nc_str_init (dst, src, len)`, `src` a buffer (`head`) or none.
fn str_init(buf: &mut Buf, len: usize, head: bool) -> Desc {
    if head {
        buf[0] = 0;
    }
    let size = len.saturating_sub(1);
    Desc {
        tail: 0,
        size,
        init: size,
        head,
    }
}

/// `_nc_safe_strcat (dst, src)`.
fn safe_strcat(d: &mut Desc, buf: &mut Buf, src: Option<&[u8]>) -> bool {
    let Some(src) = src else { return false };
    let len = c_len(src);
    if len < d.size {
        if d.head {
            copy_at(buf, d.tail, src, len);
            d.tail = d.tail.saturating_add(len);
        }
        d.size = d.size.saturating_sub(len);
        true
    } else {
        false
    }
}

/// `_nc_safe_strcpy (dst, src)`.
fn safe_strcpy(d: &mut Desc, buf: &mut Buf, src: Option<&[u8]>) -> bool {
    let Some(src) = src else { return false };
    let len = c_len(src);
    if len < d.size {
        if d.head {
            copy_at(buf, 0, src, len);
            d.tail = len;
        }
        d.size = d.init.saturating_sub(len);
        true
    } else {
        false
    }
}

/// `strcpy (buf + at, src)`: `len` bytes and a NUL, as far as the buffer
/// goes.
fn copy_at(buf: &mut Buf, at: usize, src: &[u8], len: usize) {
    for (i, &b) in src.iter().take(len).enumerate() {
        if let Some(slot) = buf.get_mut(at.saturating_add(i)) {
            *slot = b;
        }
    }
    if let Some(slot) = buf.get_mut(at.saturating_add(len)) {
        *slot = 0;
    }
}

/// The buffer's string: what comes before its first NUL.
fn c_string(buf: &Buf) -> &[u8] {
    buf.get(..c_len(buf)).unwrap_or_default()
}

/// `repeated_append (target, total, num, repeat, src)`.
fn repeated_append(
    d: &mut Desc,
    buf: &mut Buf,
    total: i32,
    num: i32,
    repeat: i32,
    src: &[u8],
) -> i32 {
    let need = usize::try_from(repeat)
        .unwrap_or(0)
        .saturating_mul(c_len(src));
    if need < d.size {
        let mut total = total;
        for _ in 0..repeat.max(0) {
            if safe_strcat(d, buf, Some(src)) {
                total = total.wrapping_add(num);
            } else {
                return INFINITY;
            }
        }
        total
    } else {
        INFINITY
    }
}

impl Term {
    /// `_nc_msec_cost (cap, affcnt)`: what sending `cap` costs, in tenths of
    /// a millisecond -- its padding, and `_char_padding` for each byte.
    #[must_use]
    pub fn msec_cost(&self, cap: Option<&[u8]>, affcnt: i32) -> i32 {
        let Some(cap) = cap else { return INFINITY };
        let cap = cap.get(..c_len(cap)).unwrap_or_default();
        let mut cum_cost: f32 = 0.0;
        let mut cp = 0usize;
        while cp < cap.len() {
            let at = |i: usize| cap.get(i).copied().unwrap_or(0);
            if at(cp) == b'$'
                && at(cp.saturating_add(1)) == b'<'
                && cap.get(cp..).is_some_and(|r| r.contains(&b'>'))
            {
                let mut number: f32 = 0.0;
                let mut state = 0;
                cp = cp.saturating_add(2);
                while at(cp) != b'>' {
                    let c = at(cp);
                    if c.is_ascii_digit() {
                        match state {
                            0 => number = number * 10.0 + f32::from(c.wrapping_sub(b'0')),
                            2 => {
                                // `(float) ((*cp - '0') / 10.0)`: a double,
                                // then cut to float.
                                #[allow(
                                    clippy::cast_possible_truncation,
                                    reason = "C's float cast"
                                )]
                                let tenth = (f64::from(c.wrapping_sub(b'0')) / 10.0) as f32;
                                number += tenth;
                                state = 3;
                            }
                            _ => {}
                        }
                    } else if c == b'*' {
                        // "padding is always a suffix"
                        if state < 4 {
                            #[allow(clippy::cast_precision_loss, reason = "C's float conversion")]
                            let a = affcnt as f32;
                            number *= a;
                            state = 4;
                        }
                    } else if c == b'.' {
                        // "a single decimal point is allowed"
                        state = if state == 0 { 2 } else { 3 };
                    }
                    if number > MAX_DELAY_MSECS {
                        number = MAX_DELAY_MSECS;
                        break;
                    }
                    cp = cp.saturating_add(1);
                }
                if !self.no_padding {
                    cum_cost += number * 10.0;
                }
            } else {
                #[allow(clippy::cast_precision_loss, reason = "C's float conversion")]
                let pad = self.costs.char_padding as f32;
                cum_cost += pad;
            }
            cp = cp.saturating_add(1);
        }
        #[allow(clippy::cast_possible_truncation, reason = "C's (int) cast")]
        let cost = cum_cost as i32;
        cost
    }

    /// `normalized_cost (cap, affcnt)`: the cost in characters, rounded up.
    fn normalized_cost(&self, cap: Option<&[u8]>, affcnt: i32) -> i32 {
        let cost = self.msec_cost(cap, affcnt);
        if cost == INFINITY {
            return cost;
        }
        let cp = self.costs.char_padding.max(1);
        cost.wrapping_add(cp)
            .wrapping_sub(1)
            .checked_div(cp)
            .unwrap_or(cost)
    }

    /// `_nc_safe_strcat (target, TIPARM_n (cap, …))` of the string
    /// capability at `index`: the expansion appended straight from the
    /// buffer `tparm` keeps, so that a move costs no allocation -- `false`,
    /// as upstream's for a null string, when the terminal has no such
    /// capability or it cannot be expanded.
    fn strcat_tiparm(&mut self, target: &mut Desc, buf: &mut Buf, index: usize, n: i32) -> bool {
        let expansion = match self.entry.string(index) {
            Some(cap) => self.tparm.expand(&self.entry, cap, &[i64::from(n)]),
            None => None,
        };
        safe_strcat(target, buf, expansion)
    }

    /// The cost of a parameterised capability given these parameters.
    fn param_cost(&mut self, index: usize, params: &[i64], normalized: bool) -> i32 {
        let s = self.tiparm(index, params);
        if normalized {
            self.normalized_cost(s.as_deref(), 1)
        } else {
            self.msec_cost(s.as_deref(), 1)
        }
    }

    /// `_nc_mvcur_init ()`: every cost worked out, then
    /// [`Term::mvcur_resume`]. `out_is_tty` is whether the screen's output
    /// is a terminal.
    pub fn mvcur_init(&mut self, out_is_tty: bool) {
        self.costs.char_padding = if out_is_tty {
            let baud = if self.baudrate() > 0 {
                self.baudrate()
            } else {
                9600
            };
            (BAUDBYTE * 1000 * 10).checked_div(baud).unwrap_or(1)
        } else {
            1
        };
        if self.costs.char_padding <= 0 {
            self.costs.char_padding = 1;
        }
        let cost = |t: &Self, i: usize| t.msec_cost(t.entry.string(i), 0);
        self.costs.cr_cost = cost(self, string::CARRIAGE_RETURN);
        self.costs.home_cost = cost(self, string::CURSOR_HOME);
        self.costs.ll_cost = cost(self, string::CURSOR_TO_LL);
        self.costs.cub1_cost = cost(self, string::CURSOR_LEFT);
        self.costs.cuf1_cost = cost(self, string::CURSOR_RIGHT);
        self.costs.cud1_cost = cost(self, string::CURSOR_DOWN);
        self.costs.cuu1_cost = cost(self, string::CURSOR_UP);
        self.costs.smir_cost = cost(self, string::ENTER_INSERT_MODE);
        self.costs.rmir_cost = cost(self, string::EXIT_INSERT_MODE);
        self.costs.ip_cost = 0;
        if self.has(string::INSERT_PADDING) {
            self.costs.ip_cost = cost(self, string::INSERT_PADDING);
        }
        self.address_cursor = [string::CURSOR_ADDRESS, string::CURSOR_MEM_ADDRESS]
            .into_iter()
            .find(|&i| self.has(i));

        let cup = self.address_cursor;
        let cup_str = cup.and_then(|i| self.tiparm(i, &[23, 23]));
        self.costs.cup_cost = self.msec_cost(cup_str.as_deref(), 1);
        self.costs.cub_cost = self.param_cost(string::PARM_LEFT_CURSOR, &[23], false);
        self.costs.cuf_cost = self.param_cost(string::PARM_RIGHT_CURSOR, &[23], false);
        self.costs.cud_cost = self.param_cost(string::PARM_DOWN_CURSOR, &[23], false);
        self.costs.cuu_cost = self.param_cost(string::PARM_UP_CURSOR, &[23], false);
        self.costs.hpa_cost = self.param_cost(string::COLUMN_ADDRESS, &[23], false);
        self.costs.vpa_cost = self.param_cost(string::ROW_ADDRESS, &[23], false);

        let ncost = |t: &Self, i: usize| t.normalized_cost(t.entry.string(i), 1);
        self.costs.ed_cost = ncost(self, string::CLR_EOS);
        self.costs.el_cost = ncost(self, string::CLR_EOL);
        self.costs.el1_cost = ncost(self, string::CLR_BOL);
        self.costs.dch1_cost = ncost(self, string::DELETE_CHARACTER);
        self.costs.ich1_cost = ncost(self, string::INSERT_CHARACTER);
        // "If this is a bce-terminal, we want to bias the choice so we use
        // clr_eol rather than spaces at the end of a line."
        if self.flag(boolean::BACK_COLOR_ERASE) {
            self.costs.el_cost = 0;
        }
        self.costs.dch_cost = self.param_cost(string::PARM_DCH, &[23], true);
        self.costs.ich_cost = self.param_cost(string::PARM_ICH, &[23], true);
        self.costs.ech_cost = self.param_cost(string::ERASE_CHARS, &[23], true);
        self.costs.rep_cost = self.param_cost(string::REPEAT_CHAR, &[i64::from(b' '), 23], true);
        let cup_str = cup.and_then(|i| self.tiparm(i, &[23, 23]));
        self.costs.cup_ch_cost = self.normalized_cost(cup_str.as_deref(), 1);
        self.costs.hpa_ch_cost = self.param_cost(string::COLUMN_ADDRESS, &[23], true);
        self.costs.cuf_ch_cost = self.param_cost(string::PARM_RIGHT_CURSOR, &[23], true);
        self.costs.inline_cost = self
            .costs
            .cup_ch_cost
            .min(self.costs.hpa_ch_cost.min(self.costs.cuf_ch_cost));

        // "If save_cursor is used within enter_ca_mode, we should not use it
        // for scrolling optimization, since the corresponding restore_cursor
        // is not nested on the various terminals (vt100, xterm, etc.) which
        // use this feature."
        let nested = match (
            self.entry.string(string::SAVE_CURSOR),
            self.entry.string(string::ENTER_CA_MODE),
        ) {
            (Some(sc), Some(smcup)) => {
                // `strstr (enter_ca_mode, save_cursor)`, which an empty
                // `sc` is found in at once.
                let sc = sc.get(..c_len(sc)).unwrap_or_default();
                sc.is_empty() || smcup.windows(sc.len()).any(|w| w == sc)
            }
            _ => false,
        };
        if nested {
            self.entry.set_string(string::SAVE_CURSOR, None);
            self.entry.set_string(string::RESTORE_CURSOR, None);
        }
        self.mvcur_resume();
    }

    /// `_nc_mvcur_resume ()`: what to send at start and after a shell
    /// escape -- `smcup`, the scrolling region reset -- and the cursor's
    /// shape put back.
    pub fn mvcur_resume(&mut self) {
        self.putp_cap(string::ENTER_CA_MODE);
        // `reset_scroll_region ()`.
        if self.has(string::CHANGE_SCROLL_REGION) {
            let last = i64::from(self.lines.wrapping_sub(1));
            self.put_tiparm(string::CHANGE_SCROLL_REGION, &[0, last], 1);
        }
        self.cursrow = -1;
        self.curscol = -1;
        if self.cursor != -1 {
            let cursor = self.cursor;
            self.cursor = -1;
            self.curs_set(cursor);
        }
    }

    /// `curs_set (vis)`: the cursor made invisible (0), normal (1) or very
    /// visible (2) where the terminal can; the old visibility -- 1 when it
    /// was unknown -- or `ERR` (-1) when the terminal cannot. Either way the
    /// screen takes `vis` to be the visibility now, as upstream's does.
    pub fn curs_set(&mut self, vis: i32) -> i32 {
        if !(0..=2).contains(&vis) {
            return -1;
        }
        let cursor = self.cursor;
        if vis == cursor {
            return cursor;
        }
        let cap = match vis {
            2 => string::CURSOR_VISIBLE,
            1 => string::CURSOR_NORMAL,
            _ => string::CURSOR_INVISIBLE,
        };
        // `NCURSES_PUTP2_FLUSH`: `OK` where the terminal has the string.
        let code = if self.putp_cap(cap) {
            self.flush();
            if cursor == -1 { 1 } else { cursor }
        } else {
            -1
        };
        self.cursor = vis;
        code
    }

    /// `_nc_mvcur_wrap ()`: the cursor to the bottom line, normal again,
    /// `rmcup`, and a carriage return "to reset the terminal's tab counter".
    pub fn mvcur_wrap(&mut self, newscr: &Window, ctype: &dyn Ctype) {
        let lines = self.lines;
        self.mvcur(newscr, ctype, -1, -1, lines.wrapping_sub(1), 0);
        if self.cursor != -1 {
            let cursor = self.cursor;
            self.curs_set(1);
            self.cursor = cursor;
        }
        self.putp_cap(string::EXIT_CA_MODE);
        self.outch(b'\r');
    }

    /// `relative_move (target, from_y, from_x, to_y, to_x, ovw)`: local
    /// motions from one place to another into `target`, and their cost.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn relative_move(
        &mut self,
        newscr: &Window,
        ctype: &dyn Ctype,
        target: &mut Desc,
        buf: &mut Buf,
        from: (i32, i32),
        to: (i32, i32),
        ovw: bool,
    ) -> i32 {
        let (from_y, from_x) = from;
        let (to_y, to_x) = to;
        let mut ovw = ovw;
        let save = *target;
        let mut vcost = 0;
        let mut hcost = 0;

        if to_y != from_y {
            vcost = INFINITY;
            if self.strcat_tiparm(target, buf, string::ROW_ADDRESS, to_y) {
                vcost = self.costs.vpa_cost;
            }
            if to_y > from_y {
                let n = to_y.wrapping_sub(from_y);
                if self.has(string::PARM_DOWN_CURSOR) && self.costs.cud_cost < vcost {
                    *target = save;
                    if self.strcat_tiparm(target, buf, string::PARM_DOWN_CURSOR, n) {
                        vcost = self.costs.cud_cost;
                    }
                }
                if let Some(cud1) = self.entry.string(string::CURSOR_DOWN)
                    && cud1.first() != Some(&b'\n')
                    && n.wrapping_mul(self.costs.cud1_cost) < vcost
                {
                    *target = save;
                    vcost = repeated_append(target, buf, 0, self.costs.cud1_cost, n, cud1);
                }
            } else {
                let n = from_y.wrapping_sub(to_y);
                if self.has(string::PARM_UP_CURSOR) && self.costs.cuu_cost < vcost {
                    *target = save;
                    if self.strcat_tiparm(target, buf, string::PARM_UP_CURSOR, n) {
                        vcost = self.costs.cuu_cost;
                    }
                }
                if let Some(cuu1) = self.entry.string(string::CURSOR_UP)
                    && n.wrapping_mul(self.costs.cuu1_cost) < vcost
                {
                    *target = save;
                    vcost = repeated_append(target, buf, 0, self.costs.cuu1_cost, n, cuu1);
                }
            }
            if vcost == INFINITY {
                return INFINITY;
            }
        }

        let save = *target;

        if to_x != from_x {
            hcost = INFINITY;
            if self.has(string::COLUMN_ADDRESS) {
                *target = save;
                if self.strcat_tiparm(target, buf, string::COLUMN_ADDRESS, to_x) {
                    hcost = self.costs.hpa_cost;
                }
            }
            if to_x > from_x {
                let n = to_x.wrapping_sub(from_x);
                if self.has(string::PARM_RIGHT_CURSOR) && self.costs.cuf_cost < hcost {
                    *target = save;
                    if self.strcat_tiparm(target, buf, string::PARM_RIGHT_CURSOR, n) {
                        hcost = self.costs.cuf_cost;
                    }
                }
                if let Some(cuf1) = self.entry.string(string::CURSOR_RIGHT) {
                    let mut lhcost: i32 = 0;
                    let mut str_buf: Buf = [0; OPT_SIZE];
                    let mut check = str_init(&mut str_buf, OPT_SIZE, true);
                    if n <= 0 || usize::try_from(n).is_ok_and(|n| n >= check.size) {
                        ovw = false;
                    }
                    // "If we have no attribute changes, overwrite is cheaper."
                    if ovw && to_y >= 0 {
                        for i in 0..n {
                            let ch = newscr.cell(to_y, from_x.wrapping_add(i));
                            if !ch.same_attr(&self.current_attr) || !self.charable(ctype, &ch) {
                                ovw = false;
                                break;
                            }
                        }
                    }
                    if ovw && to_y >= 0 {
                        for i in 0..n {
                            let ch = newscr.cell(to_y, from_x.wrapping_add(i));
                            if let Some(slot) = str_buf.get_mut(check.tail) {
                                // `(char) CharOf (…)`.
                                *slot = ch.ch().to_le_bytes()[0];
                            }
                            check.tail = check.tail.saturating_add(1);
                        }
                        if let Some(slot) = str_buf.get_mut(check.tail) {
                            *slot = 0;
                        }
                        lhcost = lhcost.wrapping_add(n.wrapping_mul(self.costs.char_padding));
                    } else {
                        lhcost = repeated_append(
                            &mut check,
                            &mut str_buf,
                            lhcost,
                            self.costs.cuf1_cost,
                            n,
                            cuf1,
                        );
                    }
                    if lhcost < hcost {
                        *target = save;
                        if safe_strcat(target, buf, Some(c_string(&str_buf))) {
                            hcost = lhcost;
                        }
                    }
                }
            } else {
                let n = from_x.wrapping_sub(to_x);
                if self.has(string::PARM_LEFT_CURSOR) && self.costs.cub_cost < hcost {
                    *target = save;
                    if self.strcat_tiparm(target, buf, string::PARM_LEFT_CURSOR, n) {
                        hcost = self.costs.cub_cost;
                    }
                }
                if let Some(cub1) = self.entry.string(string::CURSOR_LEFT) {
                    let mut str_buf: Buf = [0; OPT_SIZE];
                    let mut check = str_init(&mut str_buf, OPT_SIZE, true);
                    let lhcost =
                        repeated_append(&mut check, &mut str_buf, 0, self.costs.cub1_cost, n, cub1);
                    if lhcost < hcost {
                        *target = save;
                        if safe_strcat(target, buf, Some(c_string(&str_buf))) {
                            hcost = lhcost;
                        }
                    }
                }
            }
            if hcost == INFINITY {
                return INFINITY;
            }
        }
        vcost.wrapping_add(hcost)
    }

    /// `Charable (ch)` for the screen's locale.
    fn charable(&self, ctype: &dyn Ctype, ch: &crate::cell::Cell) -> bool {
        (self.legacy_coding != 0 || ch.attr & A_ALTCHARSET != 0 || !ch.is_widec_ext())
            && ch.chars[1] == 0
            && ctype.wctob(ch.ch()) == ch.ch()
    }

    /// `NOT_LOCAL (sp, fy, fx, ty, tx)`.
    fn not_local(&self, fy: i32, fx: i32, ty: i32, tx: i32) -> bool {
        tx > LONG_DIST
            && tx < self.columns.wrapping_sub(1).wrapping_sub(LONG_DIST)
            && ty
                .wrapping_sub(fy)
                .wrapping_abs()
                .wrapping_add(tx.wrapping_sub(fx).wrapping_abs())
                > LONG_DIST
    }

    /// `onscreen_mvcur (yold, xold, ynew, xnew, ovw, _nc_outch)`: the
    /// cheapest of the tactics sent; `false` (`ERR`) when none can get
    /// there.
    #[allow(clippy::too_many_lines)]
    fn onscreen_mvcur(
        &mut self,
        newscr: &Window,
        ctype: &dyn Ctype,
        old: (i32, i32),
        new: (i32, i32),
        ovw: bool,
    ) -> bool {
        let (yold, xold) = old;
        let (ynew, xnew) = new;
        let mut buffer: Buf = [0; OPT_SIZE];
        let mut result = str_init(&mut buffer, OPT_SIZE, true);
        let mut tactic = 0;
        let mut usecost = INFINITY;

        // "tactic #0: use direct cursor addressing"
        let cup_str = match self.address_cursor.and_then(|i| self.entry.string(i)) {
            Some(cup) => self
                .tparm
                .expand(&self.entry, cup, &[i64::from(ynew), i64::from(xnew)]),
            None => None,
        };
        let mut nonlocal = false;
        if safe_strcpy(&mut result, &mut buffer, cup_str) {
            tactic = 0;
            usecost = self.costs.cup_cost;
            if yold == -1 || xold == -1 || self.not_local(yold, xold, ynew, xnew) {
                nonlocal = true;
            }
        }
        if !nonlocal {
            let mut null_buf: Buf = [0; OPT_SIZE];
            let mut try_from = |t: &mut Self, from: (i32, i32)| {
                let mut null = str_init(&mut null_buf, OPT_SIZE, false);
                t.relative_move(newscr, ctype, &mut null, &mut null_buf, from, new, ovw)
            };
            // "tactic #1: use local movement"
            if yold != -1 && xold != -1 {
                let c = try_from(self, (yold, xold));
                if c != INFINITY && c < usecost {
                    tactic = 1;
                    usecost = c;
                }
            }
            // "tactic #2: use carriage-return + local movement"
            if yold != -1 && self.has(string::CARRIAGE_RETURN) {
                let c = try_from(self, (yold, 0));
                if c != INFINITY && self.costs.cr_cost.wrapping_add(c) < usecost {
                    tactic = 2;
                    usecost = self.costs.cr_cost.wrapping_add(c);
                }
            }
            // "tactic #3: use home-cursor + local movement"
            if self.has(string::CURSOR_HOME) {
                let c = try_from(self, (0, 0));
                if c != INFINITY && self.costs.home_cost.wrapping_add(c) < usecost {
                    tactic = 3;
                    usecost = self.costs.home_cost.wrapping_add(c);
                }
            }
            // "tactic #4: use home-down + local movement"
            if self.has(string::CURSOR_TO_LL) {
                let c = try_from(self, (self.lines.wrapping_sub(1), 0));
                if c != INFINITY && self.costs.ll_cost.wrapping_add(c) < usecost {
                    tactic = 4;
                    usecost = self.costs.ll_cost.wrapping_add(c);
                }
            }
            // "tactic #5: use left margin for wrap to right-hand side, unless
            // strange wrap behavior indicated by xenl might hose us."
            let t5_cr_cost = if xold > 0 { self.costs.cr_cost } else { 0 };
            if self.flag(boolean::AUTO_LEFT_MARGIN)
                && !self.flag(boolean::EAT_NEWLINE_GLITCH)
                && yold > 0
                && self.has(string::CURSOR_LEFT)
            {
                let c = try_from(self, (yold.wrapping_sub(1), self.columns.wrapping_sub(1)));
                if c != INFINITY
                    && t5_cr_cost
                        .wrapping_add(self.costs.cub1_cost)
                        .wrapping_add(c)
                        < usecost
                {
                    tactic = 5;
                    usecost = t5_cr_cost
                        .wrapping_add(self.costs.cub1_cost)
                        .wrapping_add(c);
                }
            }
            if tactic != 0 {
                result = str_init(&mut buffer, OPT_SIZE, true);
            }
            match tactic {
                1 => {
                    self.relative_move(
                        newscr,
                        ctype,
                        &mut result,
                        &mut buffer,
                        (yold, xold),
                        new,
                        ovw,
                    );
                }
                2 => {
                    let cr = self.entry.string(string::CARRIAGE_RETURN);
                    safe_strcpy(&mut result, &mut buffer, cr);
                    self.relative_move(
                        newscr,
                        ctype,
                        &mut result,
                        &mut buffer,
                        (yold, 0),
                        new,
                        ovw,
                    );
                }
                3 => {
                    let home = self.entry.string(string::CURSOR_HOME);
                    safe_strcpy(&mut result, &mut buffer, home);
                    self.relative_move(newscr, ctype, &mut result, &mut buffer, (0, 0), new, ovw);
                }
                4 => {
                    let ll = self.entry.string(string::CURSOR_TO_LL);
                    safe_strcpy(&mut result, &mut buffer, ll);
                    let from = (self.lines.wrapping_sub(1), 0);
                    self.relative_move(newscr, ctype, &mut result, &mut buffer, from, new, ovw);
                }
                5 => {
                    if xold > 0 {
                        let cr = self.entry.string(string::CARRIAGE_RETURN);
                        safe_strcat(&mut result, &mut buffer, cr);
                    }
                    let cub1 = self.entry.string(string::CURSOR_LEFT);
                    safe_strcat(&mut result, &mut buffer, cub1);
                    let from = (yold.wrapping_sub(1), self.columns.wrapping_sub(1));
                    self.relative_move(newscr, ctype, &mut result, &mut buffer, from, new, ovw);
                }
                _ => {}
            }
        }
        if usecost != INFINITY {
            self.tputs(c_string(&buffer), 1);
            self.cursrow = ynew;
            self.curscol = xnew;
            true
        } else {
            false
        }
    }

    /// `_nc_real_mvcur (yold, xold, ynew, xnew, _nc_outch, TRUE)` --
    /// `TINFO_MVCUR`, the library's own: wrap-around rounded out,
    /// attributes that do not survive a move turned off around it.
    pub fn mvcur(
        &mut self,
        newscr: &Window,
        ctype: &dyn Ctype,
        yold: i32,
        xold: i32,
        ynew: i32,
        xnew: i32,
    ) -> bool {
        if yold == ynew && xold == xnew {
            return true;
        }
        let (mut yold, mut xold, mut ynew, mut xnew) = (yold, xold, ynew, xnew);
        let columns = self.columns.max(1);
        if xnew >= self.columns {
            ynew = ynew.wrapping_add(xnew.checked_div(columns).unwrap_or(0));
            xnew = xnew.checked_rem(columns).unwrap_or(xnew);
        }
        // "Force restore even if msgr is on when we're in an alternate
        // character set -- these have a strong tendency to screw up the CR &
        // LF used for local character motions!"
        let oldattr = self.current_attr;
        if oldattr.attr & A_ALTCHARSET != 0
            || (oldattr.attr != 0 && !self.flag(boolean::MOVE_STANDOUT_MODE))
        {
            self.vid_puts(0, 0);
        }
        if xold >= self.columns {
            let mut l = xold.wrapping_add(1).checked_div(columns).unwrap_or(0);
            yold = yold.wrapping_add(l);
            if yold >= self.lines {
                l = l.wrapping_sub(yold.wrapping_sub(self.lines).wrapping_sub(1));
            }
            if l > 0 {
                if self.has(string::CARRIAGE_RETURN) {
                    self.putp_cap(string::CARRIAGE_RETURN);
                } else {
                    self.outch(b'\r');
                }
                xold = 0;
                while l > 0 {
                    if self.has(string::NEWLINE) {
                        self.putp_cap(string::NEWLINE);
                    } else {
                        self.outch(b'\n');
                    }
                    l = l.wrapping_sub(1);
                }
            }
        }
        if yold > self.lines.wrapping_sub(1) {
            yold = self.lines.wrapping_sub(1);
        }
        if ynew > self.lines.wrapping_sub(1) {
            ynew = self.lines.wrapping_sub(1);
        }
        let code = self.onscreen_mvcur(newscr, ctype, (yold, xold), (ynew, xnew), true);
        // "Restore attributes if we disabled them before moving."
        if !oldattr.same_attr(&self.current_attr) {
            self.vid_puts(oldattr.attr, oldattr.pair());
        }
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_descriptor_counts_without_a_buffer_and_refuses_what_does_not_fit() {
        let mut buf: Buf = [0; OPT_SIZE];
        let mut d = str_init(&mut buf, 8, true);
        assert!(safe_strcat(&mut d, &mut buf, Some(b"abc")));
        assert!(safe_strcat(&mut d, &mut buf, Some(b"de")));
        assert_eq!(c_string(&buf), b"abcde");
        assert!(
            !safe_strcat(&mut d, &mut buf, Some(b"fg")),
            "7 bytes of room took 5"
        );
        // A copy, too, must fit in the room *left* (6.4's `len <
        // dst->s_size`), though it starts again at the front.
        assert!(!safe_strcpy(&mut d, &mut buf, Some(b"xy")));
        assert!(safe_strcpy(&mut d, &mut buf, Some(b"x")));
        assert_eq!(c_string(&buf), b"x");
        assert_eq!(d.size, 6);
        assert!(safe_strcat(&mut d, &mut buf, Some(b"y")));
        assert_eq!(c_string(&buf), b"xy");
        // A rewound descriptor appends over what was there.
        let save = d;
        assert!(safe_strcat(&mut d, &mut buf, Some(b"123")));
        d = save;
        assert!(safe_strcat(&mut d, &mut buf, Some(b"Z")));
        assert_eq!(c_string(&buf), b"xyZ");
        // A counting descriptor writes nothing.
        let mut other: Buf = [7; OPT_SIZE];
        let mut n = str_init(&mut other, 8, false);
        assert!(safe_strcat(&mut n, &mut other, Some(b"abc")));
        assert_eq!(n.size, 4);
        assert_eq!(other[0], 7);
    }

    #[test]
    fn repeating_refuses_a_total_that_would_not_fit() {
        let mut buf: Buf = [0; OPT_SIZE];
        let mut d = str_init(&mut buf, 8, true);
        assert_eq!(repeated_append(&mut d, &mut buf, 0, 3, 2, b"ab"), 6);
        assert_eq!(c_string(&buf), b"abab");
        assert_eq!(repeated_append(&mut d, &mut buf, 0, 3, 2, b"ab"), INFINITY);
    }
}
