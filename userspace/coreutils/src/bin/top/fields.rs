//! "Fields Management support": the screen's geometry, each window's
//! column header and the library items it needs, the Fields screen (`f`),
//! and the upkeep of the fields table -- the widths fixed at startup, the
//! ones that widen themselves, and the memory columns' scale.

use coreutils::procps::pids::Item;

use crate::cfmt::{Arg, snprintf};
use crate::defs::{
    BREAK_AUTOX, BREAK_OFF, BREAK_SCREEN, COLPADSIZ, EU_CMD, EU_CMDLINE, EU_COD, EU_CPN, EU_CPU,
    EU_DAT, EU_GID, EU_GRP, EU_ID_FUID, EU_LXC, EU_MAXPFLGS, EU_NMA, EU_NS1, EU_NS6, EU_NS7,
    EU_NS8, EU_PGD, EU_PID, EU_PPD, EU_PSS, EU_PZA, EU_PZF, EU_PZS, EU_RES, EU_RESET, EU_RSS,
    EU_RZA, EU_RZF, EU_RZL, EU_RZS, EU_SHR, EU_SID, EU_STA, EU_SWP, EU_TGD, EU_THD, EU_TICS_ALL_C,
    EU_TM2, EU_TM3, EU_TME, EU_TPG, EU_TREE_ADD, EU_TREE_HID, EU_TREE_LVL, EU_TTY, EU_UED, EU_UEN,
    EU_URD, EU_URN, EU_USD, EU_USE, EU_USN, EU_USS, EU_VRT, EU_WCH, EU_XOF, EU_XON, FLD_OFFSET,
    FLD_ON, KBD_DOWN, KBD_END, KBD_ENTER, KBD_ESC, KBD_HOME, KBD_LEFT, KBD_PGDN, KBD_PGUP,
    KBD_RIGHT, KBD_SPACE, KBD_UP, ROWMAXSIZ, SCREENMAX, SHOW_CMDLIN, SHOW_CTIMES, SHOW_FOREST,
    SHOW_HICOLS, SHOW_IDLEPS, W_MIN_COL, W_MIN_ROW,
};
use crate::term::IOKEY_ONCE;
use crate::{BOT_DELIMIT, Top, nls, utf8};

/// `GRAPH_length_max`, `GRAPH_length_min`, `GRAPH_prefix_std`,
/// `GRAPH_prefix_abv`, `GRAPH_suffix`.
const GRAPH_LENGTH_MAX: i32 = 100;
const GRAPH_LENGTH_MIN: i32 = 10;
const GRAPH_PREFIX_STD: i32 = 25;
const GRAPH_PREFIX_ABV: i32 = 12;
const GRAPH_SUFFIX: i32 = 2;
/// `ADJOIN_space`: the printable width of `Adjoin_sp` (`" ~6 ~1"`).
pub const ADJOIN_SPACE: i32 = 2;
/// `ADJOIN_limit`.
pub const ADJOIN_LIMIT: i32 = 8;

/// `FLDget (q, i)`: the field a `fieldscur` entry names.
#[must_use]
pub fn fld_get(v: i32) -> i32 {
    (v >> 1).wrapping_sub(FLD_OFFSET)
}

/// `FLDviz (q, i)`.
#[must_use]
pub fn fld_viz(v: i32) -> bool {
    v & FLD_ON != 0
}

/// `ENUcvt (e, x)`.
#[must_use]
pub fn enu_cvt(e: i32, x: i32) -> i32 {
    (e.wrapping_add(FLD_OFFSET) << 1) | x
}

/// `N_col (f)`: a field's heading.
#[must_use]
pub fn n_col(f: i32) -> &'static [u8] {
    usize::try_from(f)
        .ok()
        .and_then(|i| nls::HEAD.get(i))
        .map_or(&b""[..], |s| s.as_bytes())
}

/// `N_fld (f)`: a field's description.
#[must_use]
pub fn n_fld(f: i32) -> &'static [u8] {
    usize::try_from(f)
        .ok()
        .and_then(|i| nls::DESC.get(i))
        .map_or(&b""[..], |s| s.as_bytes())
}

/// `utf8_justify (str, width, justr)`: [`crate::fmt::justify_pad`] in
/// columns.
#[must_use]
pub fn utf8_justify(s: &[u8], width: i32, justr: bool) -> Vec<u8> {
    let tmp = snprintf(
        SCREENMAX,
        b"%.*s",
        &[Arg::I(i64::from(utf8::embody(s, width))), Arg::S(s)],
    )
    .0;
    let width = width.wrapping_add(utf8::delta(&tmp));
    let fmt: &[u8] = if justr { b"%*.*s%s" } else { b"%-*.*s%s" };
    snprintf(
        SCREENMAX,
        fmt,
        &[
            Arg::I(i64::from(width)),
            Arg::I(i64::from(width)),
            Arg::S(&tmp),
            Arg::S(crate::defs::COLPADSTR),
        ],
    )
    .0
}

/// `strtol (s, &ep, 0)` of an environment variable, as `adj_geometry`
/// takes it: a positive number that is all of the value, or 0.
fn env_num(name: &str) -> i32 {
    let Some(v) = std::env::var_os(name).map(std::ffi::OsString::into_encoded_bytes) else {
        return 0;
    };
    if v.is_empty() {
        return 0;
    }
    let (t, used) = cstrtol::strtol(&v, 0);
    if used == v.len() && t > 0 && t <= 0x7fff_ffff {
        i32::try_from(t).unwrap_or(0)
    } else {
        0
    }
}

impl Top {
    /// `VARcol (f)`: a column whose width follows the screen's.
    #[must_use]
    pub fn varcol(&self, f: i32) -> bool {
        usize::try_from(f)
            .ok()
            .and_then(|i| self.fieldstab.get(i))
            .is_some_and(|fl| fl.width == -1)
    }

    /// `Fieldstab[f].width`.
    #[must_use]
    pub fn fwidth(&self, f: i32) -> i32 {
        usize::try_from(f)
            .ok()
            .and_then(|i| self.fieldstab.get(i))
            .map_or(0, |fl| fl.width)
    }

    /// `adj_geometry`: the screen's size, the `-w` override, the row
    /// optimisation's buffer, and the graphs' lengths.
    pub fn adj_geometry(&mut self) {
        self.screen_cols = self.ti.columns;
        self.screen_rows = self.ti.lines;
        if let Ok(wz) = libcall::pty::window_size(1)
            && wz.cols > 0
            && wz.rows > 0
        {
            self.screen_cols = i32::from(wz.cols);
            self.screen_rows = i32::from(wz.rows);
        }
        if self.caps.avoid_eol {
            self.screen_cols = self.screen_cols.wrapping_sub(1);
        }
        if self.screen_cols > SCREENMAX as i32 {
            self.screen_cols = SCREENMAX as i32;
        }
        if self.screen_cols < W_MIN_COL {
            self.screen_cols = W_MIN_COL;
        }

        if !self.st.w_set {
            if self.width_mode > 0 {
                self.st.w_cols = self.width_mode;
            } else if self.width_mode < 0 {
                let tc = env_num("COLUMNS");
                if 0 < tc {
                    self.st.w_cols = tc;
                }
                let tr = env_num("LINES");
                if 0 < tr {
                    self.st.w_rows = tr;
                }
                if self.st.w_cols == 0 {
                    self.st.w_cols = SCREENMAX as i32;
                }
                if self.st.w_cols != 0 && self.st.w_cols < W_MIN_COL {
                    self.st.w_cols = W_MIN_COL;
                }
                if self.st.w_rows != 0 && self.st.w_rows < W_MIN_ROW {
                    self.st.w_rows = W_MIN_ROW;
                }
            }
            if self.st.w_cols > SCREENMAX as i32 {
                self.st.w_cols = SCREENMAX as i32;
            }
            self.st.w_set = true;
        }

        let rows_needed: usize = if self.batch {
            if self.st.w_cols != 0 {
                self.screen_cols = self.st.w_cols;
            }
            self.screen_rows = if self.st.w_rows != 0 {
                self.st.w_rows
            } else {
                i32::MAX
            };
            1
        } else {
            let max_rows = i32::MAX
                .checked_div(i32::try_from(ROWMAXSIZ).unwrap_or(1))
                .unwrap_or(0);
            if self.st.w_cols != 0 && self.st.w_cols < self.screen_cols {
                self.screen_cols = self.st.w_cols;
            }
            if self.st.w_rows != 0 && self.st.w_rows < self.screen_rows {
                self.screen_rows = self.st.w_rows;
            }
            if self.screen_rows < 0 || self.screen_rows > max_rows {
                self.screen_rows = max_rows;
            }
            usize::try_from(self.screen_rows).unwrap_or(0)
        };
        self.out.pseudo_rows = rows_needed;
        if self.out.pseudo.len() < rows_needed {
            self.out.pseudo.resize(rows_needed, Vec::new());
        }
        self.out.psu_clreos(0);

        // The graphs.
        let du = self.cw().rc.double_up;
        let cols = self.screen_cols;
        if du != 0 {
            let num = du.wrapping_add(1);
            let pfx = if du < 2 {
                GRAPH_PREFIX_STD
            } else {
                GRAPH_PREFIX_ABV
            };
            let mut len = cols
                .wrapping_sub(ADJOIN_SPACE.wrapping_mul(du))
                .wrapping_sub(num.wrapping_mul(pfx.wrapping_add(GRAPH_SUFFIX)))
                .checked_div(num)
                .unwrap_or(0);
            len = len.clamp(GRAPH_LENGTH_MIN, GRAPH_LENGTH_MAX);
            self.graph_cpus.length = len;
            let mut ml = cols
                .wrapping_sub(ADJOIN_SPACE)
                .wrapping_sub(pfx.wrapping_add(GRAPH_SUFFIX).wrapping_mul(2))
                / 2;
            ml = ml.clamp(GRAPH_LENGTH_MIN, GRAPH_LENGTH_MAX);
            self.graph_mems.length = ml;
            if num > 2 {
                let cpu_graph =
                    (GRAPH_PREFIX_ABV + GRAPH_SUFFIX).wrapping_add(self.graph_cpus.length);
                let nxt_graph = cpu_graph.wrapping_add(ADJOIN_SPACE);
                let mut len = cpu_graph;
                while len.wrapping_add(nxt_graph) <= GRAPH_LENGTH_MAX {
                    len = len.wrapping_add(nxt_graph);
                }
                len = len.wrapping_sub(GRAPH_PREFIX_ABV + ADJOIN_SPACE);
                self.graph_mems.length = len;
            }
        } else {
            let mut len = cols.wrapping_sub(GRAPH_PREFIX_STD + GRAPH_LENGTH_MAX + GRAPH_SUFFIX);
            if len >= 0 {
                len = GRAPH_LENGTH_MAX;
            } else {
                len = cols.wrapping_sub(GRAPH_PREFIX_STD + GRAPH_SUFFIX);
            }
            if len < GRAPH_LENGTH_MIN {
                len = GRAPH_LENGTH_MIN;
            }
            self.graph_cpus.length = len;
            self.graph_mems.length = len;
        }
        // `(float)length / 100.0`: in `double`, stored as `float`.
        self.graph_cpus.adjust = crate::c_float(f64::from(self.graph_cpus.length) / 100.0);
        self.graph_cpus.style = self.cw().rc.graph_cpus;
        self.graph_mems.adjust = crate::c_float(f64::from(self.graph_mems.length) / 100.0);
        self.graph_mems.style = self.cw().rc.graph_mems;

        self.out.flush();
        crate::set_frames(BREAK_OFF);
    }

    /// `ckITEM (f)`: the library item behind field `f`, asked for.
    fn ck_item(&mut self, f: usize) {
        if let (Some(slot), Some(fl)) = (self.pids_itms.get_mut(f), self.fieldstab.get(f)) {
            *slot = fl.item;
        }
    }

    /// `build_headers`: each visible window's column header, and every
    /// library item the frame will read.
    pub fn build_headers(&mut self) {
        for (i, slot) in self.pids_itms.iter_mut().enumerate() {
            *slot = if i < EU_RESET {
                Item::Noop
            } else {
                Item::Extra
            };
        }
        self.ck_item(EU_PID);
        self.ck_item(EU_STA);
        self.ck_item(EU_CMD);

        let first = self.curwin;
        let mut w = first;
        loop {
            if self.viz_is(w) {
                let mut hdr: Vec<u8> = Vec::new();
                if self.rc.mode_altscr != 0 {
                    hdr.extend_from_slice(format!("{}", self.winstk[w].winnum).as_bytes());
                }
                let n = usize::try_from(self.winstk[w].maxpflgs).unwrap_or(0);
                for i in 0..n {
                    let f = self.winstk[w].procflgs.get(i).copied().unwrap_or(0);
                    let Ok(fu) = usize::try_from(f) else { continue };
                    if EU_MAXPFLGS <= fu {
                        continue;
                    }
                    self.ck_item(fu);
                    match fu {
                        EU_CMD => {
                            if self.winstk[w].chk(SHOW_CMDLIN) {
                                self.ck_item(EU_CMDLINE);
                            }
                        }
                        EU_CPU => self.ck_item(EU_THD),
                        EU_TME | EU_TM2 => {
                            if self.winstk[w].chk(SHOW_CTIMES) {
                                self.ck_item(EU_TICS_ALL_C);
                            }
                        }
                        _ => {}
                    }
                    let width = if self.varcol(f) {
                        self.winstk[w].varcolsz
                    } else {
                        self.fwidth(f)
                    };
                    let align = self.fieldstab.get(fu).map_or(0, |fl| fl.align);
                    let piece = utf8_justify(n_col(f), width, self.winstk[w].chk(align));
                    hdr.extend_from_slice(&piece);
                }
                self.winstk[w].columnhdr = hdr;
                if !self.winstk[w].chk(SHOW_IDLEPS) {
                    self.ck_item(EU_CPU);
                }
                if self.winstk[w].chk(SHOW_FOREST) {
                    for f in [
                        EU_PPD,
                        EU_TGD,
                        EU_TM3,
                        EU_TREE_HID,
                        EU_TREE_LVL,
                        EU_TREE_ADD,
                    ] {
                        self.ck_item(f);
                    }
                }
                if self.winstk[w].usrseltyp != 0 {
                    for f in [EU_UED, EU_URD, EU_USD, EU_ID_FUID] {
                        self.ck_item(f);
                    }
                }
                let f = self.winstk[w].rc.sortindx;
                if usize::try_from(f) == Ok(EU_CMD) {
                    if self.winstk[w].chk(SHOW_CMDLIN) {
                        self.ck_item(EU_CMDLINE);
                    }
                } else if let Ok(fu) = usize::try_from(f) {
                    self.ck_item(fu);
                }
                let items = self.bot.item;
                for &it in items.iter().take_while(|&&it| it > BOT_DELIMIT) {
                    if let Ok(fu) = usize::try_from(it) {
                        self.ck_item(fu);
                    }
                }
            }
            if self.rc.mode_altscr != 0 {
                w = Top::next_win(w);
            }
            if w == first {
                break;
            }
        }
    }

    /// `calibrate_fields`: the geometry, each visible window's columns --
    /// as many as fit from its scroll position, and from where all the
    /// rest would fit -- then the headers and the library's items.
    pub fn calibrate_fields(&mut self) {
        self.adj_geometry();
        let first = self.curwin;
        let mut w = first;
        loop {
            if self.viz_is(w) {
                let cols = self.screen_cols;
                let altscr = self.rc.mode_altscr != 0;
                let fieldstab = self.fieldstab;
                let varcol = |f: i32| {
                    usize::try_from(f)
                        .ok()
                        .and_then(|i| fieldstab.get(i))
                        .is_some_and(|fl| fl.width == -1)
                };
                let width = |f: i32| {
                    usize::try_from(f)
                        .ok()
                        .and_then(|i| fieldstab.get(i))
                        .map_or(0, |fl| fl.width)
                };
                let win = &mut self.winstk[w];
                win.hdrcaplen = 0;
                win.totpflgs = 0;
                let sortindx = win.rc.sortindx;
                let hicols = win.chk(SHOW_HICOLS);
                let mut all: Vec<i32> = Vec::new();
                for i in 0..EU_MAXPFLGS {
                    let v = win.rc.fieldscur.get(i).copied().unwrap_or(0);
                    if fld_viz(v) {
                        let f = fld_get(v);
                        if hicols && f == sortindx {
                            all.extend_from_slice(&[EU_XON as i32, f, EU_XOF as i32]);
                        } else {
                            all.push(f);
                        }
                    }
                }
                if all.is_empty() {
                    all.push(EU_PID as i32);
                }
                for (slot, f) in win.pflgsall.iter_mut().zip(&all) {
                    *slot = *f;
                }
                win.totpflgs = i32::try_from(all.len().min(win.pflgsall.len())).unwrap_or(0);

                // "build a preliminary columns header not to exceed screen
                // width while accounting for a possible leading window number"
                win.varcolsz = 0;
                let mut varcolcnt = 0i32;
                let mut hdr_len = usize::from(altscr);
                let mut i = 0i32;
                while i.wrapping_add(win.begpflg) < win.totpflgs {
                    let at = usize::try_from(i.wrapping_add(win.begpflg)).unwrap_or(0);
                    let f = win.pflgsall.get(at).copied().unwrap_or(0);
                    if let Some(slot) = win.procflgs.get_mut(usize::try_from(i).unwrap_or(0)) {
                        *slot = f;
                    }
                    if f >= EU_MAXPFLGS as i32 {
                        i = i.wrapping_add(1);
                        continue;
                    }
                    let h = n_col(f);
                    let len = COLPADSIZ.wrapping_add(if varcol(f) {
                        i32::try_from(h.len()).unwrap_or(0)
                    } else {
                        width(f)
                    });
                    if cols < i32::try_from(hdr_len).unwrap_or(0).wrapping_add(len) {
                        break;
                    }
                    if varcol(f) {
                        varcolcnt = varcolcnt.wrapping_add(1);
                        win.varcolsz = win
                            .varcolsz
                            .wrapping_add(i32::try_from(h.len()).unwrap_or(0));
                    }
                    // `fmtmk ("%*.*s", len, len, h)`.
                    hdr_len = hdr_len.saturating_add(usize::try_from(len).unwrap_or(0));
                    i = i.wrapping_add(1);
                }
                let last = usize::try_from(i.wrapping_sub(1)).unwrap_or(0);
                if i >= 1 && win.procflgs.get(last) == Some(&(EU_XON as i32)) {
                    i = i.wrapping_sub(1);
                }
                win.maxpflgs = i;
                win.varcolsz = win
                    .varcolsz
                    .wrapping_add(cols.wrapping_sub(i32::try_from(hdr_len).unwrap_or(0)));
                if varcolcnt != 0 {
                    win.varcolsz = win.varcolsz.checked_div(varcolcnt).unwrap_or(0);
                }

                // "establish the field where all remaining fields would still
                // fit within screen width, including a leading window number"
                let mut hdr_len = usize::from(altscr);
                win.endpflg = 0;
                let mut i = win.totpflgs.wrapping_sub(1);
                while -1 < i {
                    let f = win
                        .pflgsall
                        .get(usize::try_from(i).unwrap_or(0))
                        .copied()
                        .unwrap_or(0);
                    if f >= EU_MAXPFLGS as i32 {
                        win.endpflg = i;
                        i = i.wrapping_sub(1);
                        continue;
                    }
                    let h = n_col(f);
                    let len = COLPADSIZ.wrapping_add(if varcol(f) {
                        i32::try_from(h.len()).unwrap_or(0)
                    } else {
                        width(f)
                    });
                    if cols < i32::try_from(hdr_len).unwrap_or(0).wrapping_add(len) {
                        break;
                    }
                    hdr_len = hdr_len.saturating_add(usize::try_from(len).unwrap_or(0));
                    win.endpflg = i;
                    i = i.wrapping_sub(1);
                }
                let end = usize::try_from(win.endpflg).unwrap_or(0);
                if win.pflgsall.get(end) == Some(&(EU_XOF as i32)) {
                    win.endpflg = win.endpflg.wrapping_add(1);
                }
            }
            if self.rc.mode_altscr != 0 {
                w = Top::next_win(w);
            }
            if w == first {
                break;
            }
        }
        self.build_headers();
        self.pids.reset(&self.pids_itms);
    }

    /// `display_fields (focus, extend)`: the Fields screen's table, in as
    /// many columns as the screen needs, the shown fields starred and bold,
    /// the focus field highlighted (with its description while it is being
    /// moved).
    fn display_fields(&mut self, focus: i32, extend: bool) {
        const YRSVD: i32 = 4;
        const XEQUS: i32 = 2;
        const XSUFX: i32 = 22;
        let maxp = EU_MAXPFLGS as i32;
        let w = self.curwin;
        let mxcol = if self.screen_cols / 11 > 0 {
            self.screen_cols / 11
        } else {
            1
        };
        let mut xadd = 0i32;
        let mut cmax = self.screen_cols;
        let rmax = self.screen_rows.wrapping_sub(YRSVD);

        let mk_err = |top: &mut Self| {
            top.out.putp(b"\n");
            top.out.putp(nls::XTRA_WINSIZE_TXT.as_bytes());
        };
        let i = i32::from(maxp.checked_rem(mxcol).unwrap_or(0) != 0);
        if rmax < i.wrapping_add(maxp.checked_div(mxcol).unwrap_or(0)) {
            mk_err(self);
            return;
        }
        let mut i = maxp.checked_div(rmax).unwrap_or(0);
        if maxp.checked_rem(rmax).unwrap_or(0) != 0 {
            i = i.wrapping_add(1);
        }
        if i > 1 {
            cmax = cmax.checked_div(i).unwrap_or(0);
            xadd = 1;
        }
        let xprfx = xadd.wrapping_add(10);
        if cmax > xprfx.wrapping_add(XSUFX) {
            cmax = xprfx.wrapping_add(XSUFX);
        }
        let smax = cmax.wrapping_sub(xprfx);
        if smax < 0 {
            mk_err(self);
            return;
        }

        if self.st.col_sav != self.screen_cols || self.st.row_sav != self.screen_rows {
            self.st.col_sav = self.screen_cols;
            self.st.row_sav = self.screen_rows;
            let c = self.caps.clr_eos.clone();
            self.out.putp(&c);
        }
        self.out.flush();

        for i in 0..maxp {
            let idx = usize::try_from(i).unwrap_or(0);
            let v = self.winstk[w].rc.fieldscur.get(idx).copied().unwrap_or(0);
            let b = fld_viz(v);
            let x = i.checked_div(rmax).unwrap_or(0).wrapping_mul(cmax);
            let y = i.checked_rem(rmax).unwrap_or(0).wrapping_add(YRSVD);
            let hdr = self.winstk[w].capclr_hdr.clone();
            let e: Vec<u8> = if i == focus && extend {
                hdr.clone()
            } else {
                Vec::new()
            };
            let f = fld_get(v);
            let fld = n_fld(f);
            let sbuf = snprintf(
                (XSUFX * 4) as usize,
                b"= %.*s",
                &[
                    Arg::I(i64::from(utf8::embody(fld, smax.wrapping_sub(XEQUS)))),
                    Arg::S(fld),
                ],
            )
            .0;
            let col = n_col(f);
            let cut = Self::fmtmk(
                "%.*s",
                &[Arg::I(i64::from(utf8::embody(col, 8))), Arg::S(col)],
            );
            let xcol = utf8::delta(&cut);
            let xfld = utf8::delta(sbuf.get(2..).unwrap_or_default());
            let g = self.tg2(x, y);
            let bold = self.winstk[w].cap_bold.clone();
            let norm = self.caps.norm.clone();
            let focus_cap: &[u8] = if i == focus { &hdr } else { b"" };
            self.out.putt(
                b"%s%c%s%s %s%-*.*s%s%s%s %-*.*s%s",
                &[
                    Arg::S(&g),
                    Arg::C(if b { b'*' } else { b' ' }),
                    Arg::S(if b { &bold } else { &norm }),
                    Arg::S(&e),
                    Arg::S(focus_cap),
                    Arg::I(i64::from(xcol).wrapping_add(8)),
                    Arg::I(i64::from(xcol).wrapping_add(8)),
                    Arg::S(col),
                    Arg::S(&norm),
                    Arg::S(if b { &bold } else { b"" }),
                    Arg::S(&e),
                    Arg::I(i64::from(smax).wrapping_add(i64::from(xfld))),
                    Arg::I(i64::from(smax).wrapping_add(i64::from(xfld))),
                    Arg::S(&sbuf),
                    Arg::S(&norm),
                ],
            );
        }
        let c = self.caps.caps_off.clone();
        self.out.putp(&c);
    }

    /// `fields_utility`: the Fields screen -- every field shown, hidden,
    /// moved or made the sort field, for any window.
    pub fn fields_utility(&mut self) {
        let mut w = self.curwin;
        let mut h: Option<&'static [u8]> = None;
        // `p`: the entry being moved, by its index.
        let mut p: Option<usize> = None;
        let mut f;

        let spew_fi = |top: &Self, w: usize| -> (i32, i32) {
            let f = top.winstk[w].rc.sortindx;
            let cur = &top.winstk[w].rc.fieldscur;
            let at = cur
                .iter()
                .take(EU_MAXPFLGS)
                .position(|&v| v == enu_cvt(f, FLD_ON))
                .or_else(|| {
                    cur.iter()
                        .take(EU_MAXPFLGS)
                        .position(|&v| v == enu_cvt(f, 0))
                });
            (f, at.map_or(0, |a| i32::try_from(a).unwrap_or(0)))
        };
        let (f0, mut i) = spew_fi(self, w);
        f = f0;

        'signify: loop {
            let c = self.caps.clr_scr.clone();
            self.out.putp(&c);
            self.adj_geometry();

            loop {
                let hh = *h.get_or_insert_with(|| n_col(f));
                let home = self.caps.home.clone();
                self.out.putp(&home);
                let grp = self.winstk[w].grpname.clone();
                let what: &[u8] = if self.winstk[w].chk(SHOW_FOREST) {
                    nls::FOREST_VIEWS_TXT.as_bytes()
                } else {
                    hh
                };
                let line = Self::fmtmk(nls::FIELD_HEADER_FMT, &[Arg::S(&grp), Arg::S(what)]);
                self.show_special(true, &line);
                self.display_fields(i, p.is_some());
                self.out.flush();

                if crate::frames() != BREAK_OFF {
                    continue 'signify;
                }
                let key = self.iokey(IOKEY_ONCE);
                if key < 1 {
                    continue 'signify;
                }

                let maxp = EU_MAXPFLGS as i32;
                match key {
                    KBD_UP => {
                        if i > 0 {
                            i = i.wrapping_sub(1);
                            if let Some(pp) = p {
                                p = Some(self.swap_em(w, i, pp));
                            }
                        }
                    }
                    KBD_DOWN => {
                        if i.wrapping_add(1) < maxp {
                            i = i.wrapping_add(1);
                            if let Some(pp) = p {
                                p = Some(self.swap_em(w, i, pp));
                            }
                        }
                    }
                    KBD_LEFT | KBD_ENTER => p = None,
                    KBD_RIGHT => p = usize::try_from(i).ok(),
                    KBD_HOME | KBD_PGUP => {
                        if p.is_none() {
                            i = 0;
                        }
                    }
                    KBD_END | KBD_PGDN => {
                        if p.is_none() {
                            i = maxp.wrapping_sub(1);
                        }
                    }
                    k if k == KBD_SPACE || k == i32::from(b'd') => {
                        if p.is_none() {
                            let at = usize::try_from(i).unwrap_or(0);
                            if let Some(v) = self.winstk[w].rc.fieldscur.get_mut(at) {
                                *v ^= FLD_ON;
                            }
                            self.un_scrl(w);
                        }
                    }
                    k if k == i32::from(b's') => {
                        if p.is_none() {
                            let at = usize::try_from(i).unwrap_or(0);
                            f = fld_get(self.winstk[w].rc.fieldscur.get(at).copied().unwrap_or(0));
                            self.winstk[w].rc.sortindx = f;
                            h = None;
                            self.un_scrl(w);
                            self.winstk[w].off(SHOW_FOREST);
                        }
                    }
                    k if k == i32::from(b'a') || k == i32::from(b'w') => {
                        w = if k == i32::from(b'a') {
                            Top::next_win(w)
                        } else {
                            Top::prev_win(w)
                        };
                        self.curwin = w;
                        (f, i) = spew_fi(self, w);
                        h = None;
                        p = None;
                    }
                    _ => {}
                }
                if key == i32::from(b'q') || key == KBD_ESC {
                    break 'signify;
                }
            }
        }
        // "signal that we just corrupted entire screen"
        crate::set_frames(BREAK_SCREEN);
    }

    /// `unSCRL`: back to the first column, the sort column's highlight off.
    fn un_scrl(&mut self, w: usize) {
        let win = &mut self.winstk[w];
        win.begpflg = 0;
        win.varcolbeg = 0;
        win.off(SHOW_HICOLS);
    }

    /// `swapEM`: the entry being moved (at `p`) and the one at `i`
    /// exchanged; the moved one is now at `i`.
    fn swap_em(&mut self, w: usize, i: i32, p: usize) -> usize {
        self.un_scrl(w);
        let at = usize::try_from(i).unwrap_or(0);
        self.winstk[w].rc.fieldscur.swap(at, p);
        at
    }

    /// `Fieldstab[c].width = w`.
    fn set_width(&mut self, c: usize, w: i32) {
        if let Some(fl) = self.fieldstab.get_mut(c) {
            fl.width = w;
        }
    }

    /// `widths_resize`: each column a value was cut in since the last frame
    /// one wider -- with `Rc.fixed_widest` at -1.
    pub fn widths_resize(&mut self) {
        self.autox.found = false;
        for i in 0..EU_MAXPFLGS {
            if self.autox.array.get(i).copied().unwrap_or(false) {
                if let Some(fl) = self.fieldstab.get_mut(i) {
                    fl.width = fl.width.wrapping_add(1);
                }
                if let Some(a) = self.autox.array.get_mut(i) {
                    *a = false;
                }
                self.autox.found = true;
            }
        }
        if self.autox.found {
            crate::set_frames(BREAK_AUTOX);
        }
    }

    /// `zap_fieldstab`: the widths the system decides (PIDs, CPUs, nodes),
    /// the user's extra width, the memory columns' scale, and the headers.
    pub fn zap_fieldstab(&mut self) {
        if !self.st.zap_once {
            self.fieldstab[EU_CPN].width = 1;
            self.fieldstab[EU_NMA].width = 2;
            let pid_cols = [EU_PID, EU_PPD, EU_PGD, EU_SID, EU_TGD, EU_TPG];
            for c in pid_cols {
                self.set_width(c, 5);
            }
            let digits = i32::try_from(coreutils::procps::sysinfo::pid_length(
                std::path::Path::new(crate::PROC),
            ))
            .unwrap_or(i32::MAX);
            if 5 < digits {
                if 10 < digits {
                    self.error_exit(nls::FAIL_WIDEPID_TXT.as_bytes());
                }
                for c in pid_cols {
                    self.set_width(c, digits);
                }
            }
            self.st.zap_once = true;
        }

        self.cpu_pmax = 99.9;
        if self.rc.mode_irixps != 0 && self.cpu_cnt > 1 && !self.thread_mode {
            self.cpu_pmax = 100.0 * crate::c_float(f64::from(self.cpu_cnt));
            if self.cpu_cnt > 1000 {
                if self.cpu_pmax > 9_999_999.0 {
                    self.cpu_pmax = 9_999_999.0;
                }
            } else if self.cpu_cnt > 100 {
                // Upstream compares the count, not the maximum, here.
                if self.cpu_cnt > 999_999 {
                    self.cpu_pmax = 999_999.0;
                }
            } else if self.cpu_cnt > 10 {
                if self.cpu_pmax > 99_999.0 {
                    self.cpu_pmax = 99_999.0;
                }
            } else if self.cpu_pmax > 999.9 {
                self.cpu_pmax = 999.9;
            }
        }

        let digits = i32::try_from(format!("{}", self.cpu_cnt).len()).unwrap_or(0);
        if 1 < digits {
            if 5 < digits {
                self.error_exit(nls::FAIL_WIDECPU_TXT.as_bytes());
            }
            self.fieldstab[EU_CPN].width = digits;
        }
        let digits = i32::try_from(format!("{}", self.numa_node_tot).len()).unwrap_or(0);
        if 2 < digits {
            self.fieldstab[EU_NMA].width = digits;
        }

        if self.rc.fixed_widest >= 0 {
            let fw = self.rc.fixed_widest;
            let wide = |base: i32| if fw != 0 { base.wrapping_add(fw) } else { base };
            for c in [EU_UED, EU_URD, EU_USD, EU_GID] {
                self.set_width(c, wide(5));
            }
            for c in [EU_UEN, EU_URN, EU_USN, EU_GRP, EU_TTY, EU_LXC] {
                self.set_width(c, wide(8));
            }
            self.fieldstab[EU_WCH].width = wide(10);
            for c in (EU_NS1..=EU_NS6).chain(EU_NS7..=EU_NS8) {
                self.set_width(c, wide(10));
            }
        }

        let scale = self.rc.task_mscale;
        for c in [
            EU_VRT, EU_SWP, EU_RES, EU_COD, EU_DAT, EU_SHR, EU_USE, EU_RZA, EU_RZF, EU_RZL, EU_RZS,
            EU_RSS, EU_PSS, EU_PZA, EU_PZF, EU_PZS, EU_USS,
        ] {
            if let Some(fl) = self.fieldstab.get_mut(c) {
                fl.scale = scale;
            }
        }

        self.calibrate_fields();
    }
}

/// The fields a window's `fieldscur` holds, for a check that it names each
/// once: `msch (fieldscur, fieldscur[n], EU_MAXPFLGS)`'s first position.
#[must_use]
pub fn first_pos(cur: &[i32], v: i32) -> Option<usize> {
    cur.iter().take(EU_MAXPFLGS).position(|&x| x == v)
}

/// `mlen (fieldscur)`: the entries before the first 0.
#[must_use]
pub fn mlen(cur: &[i32]) -> usize {
    cur.iter().take_while(|&&v| v != 0).count()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn fieldscur_entries_encode_field_and_visibility() {
        let v = enu_cvt(18, FLD_ON);
        assert_eq!(v, ((18 + 37) << 1) | 1);
        assert_eq!(fld_get(v), 18);
        assert!(fld_viz(v));
        assert!(!fld_viz(enu_cvt(18, 0)));
        // ORIG_TOPDEFS' first entry: PID, shown.
        assert_eq!(fld_get(75), 0);
        assert!(fld_viz(75));
    }

    #[test]
    fn headers_justify_in_columns() {
        assert_eq!(utf8_justify(b"PID", 7, true), b"    PID ");
        assert_eq!(utf8_justify(b"COMMAND", 3, false), b"COM ");
    }

    #[test]
    fn mlen_stops_at_the_first_zero() {
        assert_eq!(mlen(&[3, 4, 0, 5]), 2);
        assert_eq!(first_pos(&[3, 4, 3], 3), Some(0));
    }
}
