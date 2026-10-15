//! "Windows/Field Groups support", "Other Filtering", "Forest View
//! support" and "Special Separate Bottom Window support": the four
//! windows and their colours, the `u`/`U` and `o`/`O` filters, the process
//! tree the `V` view draws (with `v`'s collapsed branches), and the window
//! at the bottom of the screen that `^G`, `^K`, `^L`, `^N`, `^P` and `^U`
//! open on one task.

use coreutils::procps::pids::Val;

use crate::cfmt::{Arg, snprintf};
use crate::defs::{
    BIGBUFSIZ, BOTBUFSIZ, BREAK_SCREEN, EQUWINS_XXX, EU_CMD, EU_CMDLINE, EU_CMDLINE_V, EU_CPU,
    EU_ENVIRON_V, EU_ID_FUID, EU_NS1, EU_NS2, EU_NS3, EU_NS4, EU_NS5, EU_NS6, EU_NS7, EU_NS8,
    EU_PID, EU_PPD, EU_TGD, EU_TREE_ADD, EU_TREE_HID, EU_TREE_LVL, EU_UED, EU_URD, EU_USD,
    FLAGS_OFF, FLAGS_SET, FLAGS_TOG, KBD_DOWN, KBD_ENTER, KBD_ESC, KBD_UP, MAXBUFSIZ, MEDBUFSIZ,
    SHOW_CMDLIN, SHOW_COLORS, SHOW_FOREST, SHOW_HIBOLD, SHOW_IDLEPS, SHOW_TASKON, SMLBUFSIZ,
    VIEW_NOBOLD, WARN_ALT, WARN_VIZ,
};
use crate::term::{IOKEY_ONCE, c_str};
use crate::{BOT_DELIMIT, BOT_ITEM_NS, BOT_MSG_LOG, BOT_UNFOCUS, BotFocus, Osel, Top, nls, utf8};

/// `BOT_RSVD`: the bottom window's separator row.
const BOT_RSVD: i32 = 1;

/// `ns_tab`: the namespace fields, in the bottom window's order, with the
/// names `procps_ns_get_name` gives them.
const NS_TAB: [(usize, &str); 8] = [
    (EU_NS7, "cgroup"),
    (EU_NS1, "ipc"),
    (EU_NS2, "mnt"),
    (EU_NS3, "net"),
    (EU_NS4, "pid"),
    (EU_NS8, "time"),
    (EU_NS5, "user"),
    (EU_NS6, "uts"),
];

/// `strcasecmp`, folding ASCII as glibc does in the locales this tree has.
fn strcasecmp(a: &[u8], b: &[u8]) -> i32 {
    let la = a.iter().map(u8::to_ascii_lowercase);
    let lb = b.iter().map(u8::to_ascii_lowercase);
    for (x, y) in la.zip(lb) {
        if x != y {
            return i32::from(x).wrapping_sub(i32::from(y));
        }
    }
    i32::try_from(a.len())
        .unwrap_or(0)
        .wrapping_sub(i32::try_from(b.len()).unwrap_or(0))
}

/// `strcmp`: the sign of the first difference.
fn strcmp(a: &[u8], b: &[u8]) -> i32 {
    for (&x, &y) in a.iter().zip(b) {
        if x != y {
            return i32::from(x).wrapping_sub(i32::from(y));
        }
    }
    i32::try_from(a.len())
        .unwrap_or(0)
        .wrapping_sub(i32::try_from(b.len()).unwrap_or(0))
}

/// `strstr` (or, with `fold`, `strcasestr`): whether `needle` is in `hay`.
fn find(hay: &[u8], needle: &[u8], fold: bool) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.windows(needle.len()).any(|w| {
        if fold {
            w.eq_ignore_ascii_case(needle)
        } else {
            w == needle
        }
    })
}

/// `find_ofs (q, buf)`'s search: where `needle` first is in `hay`.
#[must_use]
pub fn strstr_at(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Which colour `wins_colors`' `pclr` points at.
#[derive(Clone, Copy)]
enum Target {
    Summ,
    Msgs,
    Head,
    Task,
}

impl Target {
    /// `*pclr = v`.
    fn set(self, rc: &mut crate::defs::Rcw, v: i32) {
        match self {
            Self::Summ => rc.summclr = v,
            Self::Msgs => rc.msgsclr = v,
            Self::Head => rc.headclr = v,
            Self::Task => rc.taskclr = v,
        }
    }
}

impl Top {
    /// `win_names (q, name)`: the window's name, and its `N:name` label.
    pub fn win_names(&mut self, q: usize, name: &[u8]) {
        let w = &mut self.winstk[q];
        if w.rc.winname != name {
            w.rc.winname = snprintf(crate::defs::WINNAMSIZ, b"%s", &[Arg::S(name)]).0;
        }
        w.grpname = snprintf(
            crate::defs::GRPNAMSIZ,
            b"%d:%s",
            &[Arg::I(i64::from(w.winnum)), Arg::S(name)],
        )
        .0;
    }

    /// `win_reset (q)`: a window back to all its tasks, unfiltered and
    /// unscrolled -- and process monitoring, the cumulative time scale and
    /// the bottom window off.
    pub fn win_reset(&mut self, q: usize) {
        let w = &mut self.winstk[q];
        w.set(SHOW_IDLEPS | SHOW_TASKON);
        w.rc.maxtasks = 0;
        w.usrseltyp = 0;
        w.begpflg = 0;
        w.begtask = 0;
        w.varcolbeg = 0;
        w.focus_pid = 0;
        w.begnext = 0;
        w.osel.clear();
        w.findstr.clear();
        w.rc.combine_cpus = 0;
        w.rc.core_types = 0;
        self.monpidsidx = 0;
        self.rc.tics_scaled = 0;
        self.bot.toss();
    }

    /// `win_select (ch)`: the window `ch` names -- the next, the previous, or
    /// one of the four -- made current; without `ch`, one asked for.
    pub fn win_select(&mut self, ch: i32) -> usize {
        let mut ch = ch;
        if ch == 0 {
            self.show_pmt(nls::CHOOSE_GROUP_TXT.as_bytes());
            ch = self.iokey(IOKEY_ONCE);
            if 1 > ch {
                return self.curwin;
            }
        }
        let w = match u8::try_from(ch) {
            Ok(b'a') => Top::next_win(self.curwin),
            Ok(b'w') => Top::prev_win(self.curwin),
            Ok(c @ b'1'..=b'4') => usize::from(c.wrapping_sub(b'1')),
            _ => self.curwin,
        };
        self.curwin = w;
        w
    }

    /// `win_warn (what)`: why a command was refused; always false.
    pub fn win_warn(&mut self, what: i32) -> bool {
        if what == WARN_ALT {
            self.show_msg(nls::DISABLED_CMD_TXT.as_bytes());
        } else if what == WARN_VIZ {
            let g = self.cw().grpname.clone();
            let msg = Self::fmtmk(nls::DISABLED_WIN_FMT, &[Arg::S(&g)]);
            self.show_msg(&msg);
        }
        false
    }

    /// `ALTCHKw`: the alternate display is on, or say it is not.
    pub fn altchk(&mut self) -> bool {
        self.rc.mode_altscr != 0 || self.win_warn(WARN_ALT)
    }

    /// `VIZCHKw (w)`: window `w` shows tasks, or say it does not.
    pub fn vizchk(&mut self, w: usize) -> bool {
        self.viz_is(w) || self.win_warn(WARN_VIZ)
    }

    /// `VIZTOGw (w, f)`: flag `f` toggled where window `w` shows tasks.
    pub fn viztog(&mut self, w: usize, f: i32) {
        if self.viz_is(w) {
            self.winstk[w].tog(f);
        } else {
            self.win_warn(WARN_VIZ);
        }
    }

    /// `wins_clrhlp (q, save)`: a window's colours saved (and colour
    /// forced on) or put back, then its strings remade.
    fn wins_clrhlp(&mut self, q: usize, save: bool) {
        let w = &mut self.winstk[q];
        if save {
            self.st.clrsav = [
                w.rc.winflags,
                w.rc.summclr,
                w.rc.msgsclr,
                w.rc.headclr,
                w.rc.taskclr,
            ];
            w.set(SHOW_COLORS);
        } else {
            let [f, s, m, h, t] = self.st.clrsav;
            w.rc.winflags = f;
            w.rc.summclr = s;
            w.rc.msgsclr = m;
            w.rc.headclr = h;
            w.rc.taskclr = t;
        }
        self.capsmk(q);
    }

    /// `wins_colors` (`Z`): the colour screen.
    pub fn wins_colors(&mut self) {
        let mut w = self.curwin;
        let mut clr = self.winstk[w].rc.taskclr;
        let mut target = Target::Task;
        let mut tgt = b'T';
        if 0 >= self.ti.max_colors {
            self.show_msg(nls::COLORS_NOMAP_TXT.as_bytes());
            return;
        }
        self.wins_clrhlp(w, true);
        self.cursor_state = self.caps.curs_huge.clone();
        let c = self.cursor_state.clone();
        self.out.putp(&c);

        let on = nls::ON_WORD_ONLY_TXT.as_bytes();
        let off = nls::OFF_ONE_WORD_TXT.as_bytes();
        let mut key;
        'signify: loop {
            let c = self.caps.clr_scr.clone();
            self.out.putp(&c);
            self.adj_geometry();
            loop {
                let c = self.caps.home.clone();
                self.out.putp(&c);
                let win = &self.winstk[w];
                let text = Self::fmtmk(
                    nls::COLOR_CUSTOM_FMT,
                    &[
                        Arg::S(&win.grpname),
                        Arg::S(if win.chk(VIEW_NOBOLD) { on } else { off }),
                        Arg::S(if win.chk(SHOW_COLORS) { on } else { off }),
                        Arg::S(if win.chk(SHOW_HIBOLD) { on } else { off }),
                        Arg::C(tgt),
                        Arg::I(i64::from(self.ti.max_colors)),
                        Arg::I(i64::from(clr)),
                        Arg::S(&win.grpname),
                    ],
                );
                self.show_special(true, &text);
                let c = self.caps.clr_eos.clone();
                self.out.putp(&c);
                self.out.flush();

                if crate::frames() != 0 {
                    continue 'signify;
                }
                key = self.iokey(IOKEY_ONCE);
                if key < 1 {
                    continue 'signify;
                }
                if key == KBD_ESC {
                    break 'signify;
                }
                match key {
                    k if k == i32::from(b'S') => {
                        target = Target::Summ;
                        clr = self.winstk[w].rc.summclr;
                        tgt = b'S';
                    }
                    k if k == i32::from(b'M') => {
                        target = Target::Msgs;
                        clr = self.winstk[w].rc.msgsclr;
                        tgt = b'M';
                    }
                    k if k == i32::from(b'H') => {
                        target = Target::Head;
                        clr = self.winstk[w].rc.headclr;
                        tgt = b'H';
                    }
                    k if k == i32::from(b'T') => {
                        target = Target::Task;
                        clr = self.winstk[w].rc.taskclr;
                        tgt = b'T';
                    }
                    k if (i32::from(b'0')..=i32::from(b'7')).contains(&k) => {
                        clr = k.wrapping_sub(i32::from(b'0'));
                        target.set(&mut self.winstk[w].rc, clr);
                    }
                    KBD_UP => {
                        clr = clr.wrapping_add(1);
                        if clr >= self.ti.max_colors {
                            clr = 0;
                        }
                        target.set(&mut self.winstk[w].rc, clr);
                    }
                    KBD_DOWN => {
                        clr = clr.wrapping_sub(1);
                        if clr < 0 {
                            clr = self.ti.max_colors.wrapping_sub(1);
                        }
                        target.set(&mut self.winstk[w].rc, clr);
                    }
                    k if k == i32::from(b'B') => self.winstk[w].tog(VIEW_NOBOLD),
                    k if k == i32::from(b'b') => self.winstk[w].tog(SHOW_HIBOLD),
                    k if k == i32::from(b'z') => self.winstk[w].tog(SHOW_COLORS),
                    k if k == i32::from(b'a') || k == i32::from(b'w') => {
                        w = self.win_select(k);
                        self.wins_clrhlp(w, true);
                        clr = self.winstk[w].rc.taskclr;
                        target = Target::Task;
                        tgt = b'T';
                    }
                    _ => {}
                }
                self.capsmk(w);
                if key == KBD_ENTER || key == i32::from(b'q') {
                    break 'signify;
                }
            }
        }
        if key == i32::from(b'q') || key == KBD_ESC {
            self.wins_clrhlp(w, false);
        }
        crate::set_frames(BREAK_SCREEN);
    }

    /// `wins_reflag (what, flg)`: a flag toggled, set or cleared in every
    /// window; `EQUWINS_xxx` resets each.
    pub fn wins_reflag(&mut self, what: i32, flg: i32) {
        let first = self.curwin;
        let mut w = first;
        loop {
            match what {
                FLAGS_TOG => self.winstk[w].tog(flg),
                FLAGS_SET => self.winstk[w].set(flg),
                FLAGS_OFF => self.winstk[w].off(flg),
                _ => {}
            }
            if flg == EQUWINS_XXX {
                self.win_reset(w);
            }
            w = Top::next_win(w);
            if w == first {
                break;
            }
        }
    }

    /// `wins_usrselect (q, idx)`: whether row `idx` of window `q` passes its
    /// `u`/`U` filter.
    #[must_use]
    pub fn wins_usrselect(&self, q: usize, idx: i32) -> bool {
        let win = &self.winstk[q];
        let uid = win.usrseluid.cast_unsigned();
        let rsv = |e: usize| {
            self.stack_of(q, idx)
                .and_then(|s| s.get(e))
                .map_or(0, Val::u_int)
        };
        let flg = win.usrselflg != 0;
        match win.usrseltyp {
            0 => return true,
            t if t == i32::from(b'U') => {
                if rsv(EU_URD) == uid || rsv(EU_USD) == uid || rsv(EU_ID_FUID) == uid {
                    return flg;
                }
                if rsv(EU_UED) == uid {
                    return flg;
                }
            }
            t if t == i32::from(b'u') => {
                if rsv(EU_UED) == uid {
                    return flg;
                }
            }
            _ => {}
        }
        !flg
    }

    /// `user_certify (q, str, typ)`: a user name or number for window `q`'s
    /// `u` (effective) or `U` (any) filter, `!` before it to exclude; the
    /// complaint, if it names no one.
    pub fn user_certify(&mut self, q: usize, s: &[u8], typ: u8) -> Option<&'static str> {
        self.monpidsidx = 0;
        let w = &mut self.winstk[q];
        w.usrseltyp = 0;
        w.usrselflg = 1;
        let mut s = c_str(s);
        if s.is_empty() {
            return None;
        }
        if s.first() == Some(&b'!') {
            s = s.get(1..).unwrap_or_default();
            w.usrselflg = 0;
        }
        let db = pwdb::Db::load();
        let (num, used) = cstrtol::strtoul(s, 0);
        // `(uid_t)`: the low 32 bits.
        let num = u32::try_from(num & 0xffff_ffff).unwrap_or(0);
        let uid = if used == s.len() {
            match db.user_by_uid(num) {
                Some(u) => Some(u.uid),
                None => {
                    // "allow foreign users, from e.g within chroot"
                    w.usrseluid = num.cast_signed();
                    w.usrseltyp = i32::from(typ);
                    return None;
                }
            }
        } else {
            db.user_by_name(s).map(|u| u.uid)
        };
        let Some(uid) = uid else {
            return Some(nls::BAD_USERNAME_TXT);
        };
        w.usrseluid = uid.cast_signed();
        w.usrseltyp = i32::from(typ);
        None
    }

    // -----------------------------------------------------------------------
    // Other filtering
    // -----------------------------------------------------------------------

    /// `osel_add (q, ch, glob, push)`: one `FIELD<value`, `FIELD>value` or
    /// `FIELD=value` filter (`!` before it to exclude) for window `q`; the
    /// complaint, if it is not one. `o` ignores case, `O` does not.
    pub fn osel_add(&mut self, q: usize, ch: i32, glob: &[u8], push: bool) -> Option<Vec<u8>> {
        let raw = snprintf(MEDBUFSIZ, b"%s", &[Arg::S(glob)]).0;
        if raw.is_empty() {
            return None;
        }
        if self.winstk[q].osel.iter().any(|o| o.raw == raw) {
            return Some(nls::OSEL_ERRDUPS_TXT.as_bytes().to_vec());
        }
        let mut glob = glob;
        let inc = if glob.first() == Some(&b'!') {
            glob = glob.get(1..).unwrap_or_default();
            false
        } else {
            true
        };
        let word = if inc {
            nls::WORD_INCLUDE_TXT
        } else {
            nls::WORD_EXCLUDE_TXT
        };
        let Some(at) = glob.iter().position(|b| b"<=>".contains(b)) else {
            return Some(Self::fmtmk(
                nls::OSEL_ERRDELM_FMT,
                &[Arg::S(word.as_bytes())],
            ));
        };
        let ops = glob.get(at).copied().unwrap_or(b'=');
        let name = glob.get(..at).unwrap_or_default();
        let pval = glob.get(at.saturating_add(1)..).unwrap_or_default();
        let Some(enu) = (0..crate::defs::EU_MAXPFLGS)
            .find(|&e| crate::fields::n_col(i32::try_from(e).unwrap_or(0)) == name)
        else {
            return Some(Self::fmtmk(nls::XTRA_BADFLDS_FMT, &[Arg::S(name)]));
        };
        if pval.is_empty() {
            return Some(Self::fmtmk(
                nls::OSEL_ERRVALU_FMT,
                &[Arg::S(word.as_bytes())],
            ));
        }
        // The field's alignment flag is never zero: every value but `=`'s is
        // right-justified to the column's width.
        let val = if ops == b'=' {
            pval.to_vec()
        } else {
            crate::fmt::justify_pad(pval, self.fieldstab.get(enu).map_or(0, |f| f.width), true)
        };
        let osel = Osel {
            typ: u8::try_from(ch).unwrap_or(b'O'),
            inc,
            enu,
            ops,
            val,
            raw,
        };
        if push {
            self.winstk[q].osel.insert(0, osel);
        } else {
            self.winstk[q].osel.push(osel);
        }
        None
    }

    /// `osel_matched (q, enu, str)`: whether column `enu`'s text `s` passes
    /// every filter window `q` has on it.
    #[must_use]
    pub fn osel_matched(&self, q: usize, enu: usize, s: &[u8]) -> bool {
        for o in &self.winstk[q].osel {
            if o.enu != enu {
                continue;
            }
            let fold = o.typ == b'o';
            match o.ops {
                b'<' => {
                    let r = if fold {
                        strcasecmp(s, &o.val)
                    } else {
                        strcmp(s, &o.val)
                    };
                    if (r >= 0 && o.inc) || (r < 0 && !o.inc) {
                        return false;
                    }
                }
                b'>' => {
                    let r = if fold {
                        strcasecmp(s, &o.val)
                    } else {
                        strcmp(s, &o.val)
                    };
                    if (r <= 0 && o.inc) || (r > 0 && !o.inc) {
                        return false;
                    }
                }
                _ => {
                    let p = find(s, &o.val, fold);
                    if (!p && o.inc) || (p && !o.inc) {
                        return false;
                    }
                }
            }
        }
        true
    }

    // -----------------------------------------------------------------------
    // Forest view
    // -----------------------------------------------------------------------

    /// A stack's `PIDS_extra` or other slot, to write.
    fn put_slot(&mut self, stack: usize, slot: usize, v: Val) {
        if let Some(s) = self.pids.stack_any_mut(stack) {
            if s.len() <= slot {
                s.resize(slot.saturating_add(1), Val::Zero);
            }
            if let Some(x) = s.get_mut(slot) {
                *x = v;
            }
        }
    }

    /// A stack's slot, read.
    fn slot_of(&self, stack: usize, slot: usize) -> Option<&Val> {
        self.pids.stack_any(stack).and_then(|s| s.get(slot))
    }

    /// `forest_adds (self, level)`: task `me` of the seed order and its
    /// descendants, depth first in the seed order, into the tree -- each
    /// with its level (levels past 100 are 101).
    fn forest_adds(&mut self, seed: &[usize], me: usize, level: i32) {
        let n = usize::try_from(self.pidsmaxt()).unwrap_or(0);
        let s_int =
            |top: &Self, stack: usize, slot: usize| top.slot_of(stack, slot).map_or(0, Val::s_int);
        // The recursion, kept on a stack of our own: a frame is a task and
        // where its scan for children has got to.
        let mut frames: Vec<(usize, i32, usize)> = Vec::new();
        let push =
            |top: &mut Self, frames: &mut Vec<(usize, i32, usize)>, me: usize, level: i32| {
                let ti = usize::try_from(top.st.tree_idx).unwrap_or(0);
                if ti < n {
                    let level = if level > 100 { 101 } else { level };
                    let stack = seed.get(me).copied().unwrap_or(0);
                    if let Some(slot) = top.st.tree_ppt.get_mut(ti) {
                        *slot = stack;
                    }
                    top.put_slot(stack, EU_TREE_LVL, Val::SInt(level));
                    top.st.tree_idx = top.st.tree_idx.wrapping_add(1);
                    frames.push((me, level, me.saturating_add(1)));
                }
            };
        push(self, &mut frames, me, level);
        while let Some(&(me, level, from)) = frames.last() {
            let me_stack = seed.get(me).copied().unwrap_or(0);
            let me_pid = s_int(self, me_stack, EU_PID);
            let mut found = None;
            for i in from..n {
                let st = seed.get(i).copied().unwrap_or(0);
                let tgid = s_int(self, st, EU_TGD);
                if me_pid == tgid
                    || (me_pid == s_int(self, st, EU_PPD) && s_int(self, st, EU_PID) == tgid)
                {
                    found = Some(i);
                    break;
                }
            }
            match found {
                Some(i) => {
                    if let Some(top) = frames.last_mut() {
                        top.2 = i.saturating_add(1);
                    }
                    push(self, &mut frames, i, level.wrapping_add(1));
                }
                None => {
                    frames.pop();
                }
            }
        }
    }

    /// `forest_begin (q)`: window `q`'s tasks in tree order, built once a
    /// frame -- sorted by start time, each parent before its children --
    /// with the collapsed branches (`v`) hidden and their CPU added to the
    /// parent.
    pub fn forest_begin(&mut self, q: usize) {
        let n = usize::try_from(self.pidsmaxt()).unwrap_or(0);
        if self.st.tree_idx == 0 {
            if self.st.tree_ppt.len() < n {
                self.st.tree_ppt.resize(n, 0);
            }
            {
                let pids = &self.pids;
                let ppt = &mut self.winstk[q].ppt;
                let end = n.min(ppt.len());
                if let Some(slice) = ppt.get_mut(..end) {
                    let _ = pids.sort(
                        slice,
                        coreutils::procps::pids::Item::TicsBegan,
                        coreutils::procps::pids::ASCEND,
                    );
                }
            }
            let seed: Vec<usize> = self.winstk[q].ppt.iter().take(n).copied().collect();
            for i in 0..n {
                let st = seed.get(i).copied().unwrap_or(0);
                if self.slot_of(st, EU_TREE_LVL).map_or(0, Val::s_int) == 0 {
                    self.forest_adds(&seed, i, 0);
                }
            }

            let tree = self.st.tree_ppt.clone();
            let lvl = |top: &Self, j: usize| {
                top.slot_of(tree.get(j).copied().unwrap_or(0), EU_TREE_LVL)
                    .map_or(0, Val::s_int)
            };
            for hi in 0..usize::try_from(self.st.hide_tot).unwrap_or(0) {
                let hide = self.st.hide_pid.get(hi).copied().unwrap_or(0);
                if hide <= 0 {
                    continue;
                }
                let mut j = 0usize;
                let mut found = false;
                while j < n {
                    let st = tree.get(j).copied().unwrap_or(0);
                    if self.slot_of(st, EU_PID).map_or(0, Val::s_int) == hide {
                        let parent = j;
                        let pst = st;
                        let mut children = false;
                        let level = lvl(self, parent);
                        while j.saturating_add(1) < n && lvl(self, j.saturating_add(1)) > level {
                            j = j.saturating_add(1);
                            let cst = tree.get(j).copied().unwrap_or(0);
                            self.put_slot(cst, EU_TREE_HID, Val::SCh(b'z'));
                            let add = self.slot_of(pst, EU_TREE_ADD).map_or(0, Val::u_int);
                            let cpu = self.slot_of(cst, EU_CPU).map_or(0, Val::u_int);
                            self.put_slot(pst, EU_TREE_ADD, Val::UInt(add.wrapping_add(cpu)));
                            children = true;
                        }
                        if children {
                            self.put_slot(pst, EU_TREE_HID, Val::SCh(b'x'));
                        }
                        found = true;
                        break;
                    }
                    j = j.saturating_add(1);
                }
                // "if a target task disappeared prevent any further scanning"
                if !found && let Some(h) = self.st.hide_pid.get_mut(hi) {
                    *h = h.wrapping_neg();
                }
            }
        }
        let tree: Vec<usize> = self.st.tree_ppt.iter().take(n).copied().collect();
        let ppt = &mut self.winstk[q].ppt;
        for (dst, src) in ppt.iter_mut().zip(tree) {
            *dst = src;
        }
    }

    /// `forest_config (q)`: where window `q`'s focus task (`F`) and its
    /// descendants are; focus off if it has gone.
    pub fn forest_config(&mut self, q: usize) {
        let n = self.pidsmaxt();
        let lvl = |top: &Self, i: i32| top.pv_int(q, i, EU_TREE_LVL);
        let mut level = 0;
        let mut i = 0;
        while i < n {
            if self.winstk[q].focus_pid == self.pv_int(q, i, EU_PID) {
                level = lvl(self, i);
                self.winstk[q].focus_beg = i;
                break;
            }
            i = i.wrapping_add(1);
        }
        if i == n {
            self.winstk[q].focus_pid = 0;
            self.winstk[q].begtask = 0;
        } else {
            while i.wrapping_add(1) < n && lvl(self, i.wrapping_add(1)) > level {
                i = i.wrapping_add(1);
            }
            let w = &mut self.winstk[q];
            w.focus_end = i.wrapping_add(1);
            // "watch out for newly forked/cloned tasks 'above' us ..."
            if w.begtask < w.focus_beg {
                w.begtask = w.focus_beg;
                w.begnext = 0;
            }
        }
    }

    /// `forest_display (q, idx)`: row `idx`'s command (or command line) with
    /// the tree's art before it.
    #[must_use]
    pub fn forest_display(&self, q: usize, idx: i32) -> Vec<u8> {
        let win = &self.winstk[q];
        let which = if win.chk(SHOW_CMDLIN) {
            self.pv_str(q, idx, EU_CMDLINE)
        } else {
            self.pv_str(q, idx, EU_CMD)
        };
        let level = self.pv_int(q, idx, EU_TREE_LVL);
        if !win.chk(SHOW_FOREST) || level == 0 {
            return which;
        }
        let hid = self
            .stack_of(q, idx)
            .and_then(|s| s.get(EU_TREE_HID))
            .map_or(0, Val::s_ch);
        if q == self.curwin && hid == b'x' {
            return snprintf(
                MAXBUFSIZ,
                b"+%*s%s",
                &[
                    Arg::I(i64::from(level).wrapping_mul(4).wrapping_sub(1)),
                    Arg::S(b"`- "),
                    Arg::S(&which),
                ],
            )
            .0;
        }
        if level > 100 {
            return snprintf(MAXBUFSIZ, b"%400s%s", &[Arg::S(b" +  "), Arg::S(&which)]).0;
        }
        if win.focus_pid != 0 {
            snprintf(
                MAXBUFSIZ,
                b"|%*s%s",
                &[
                    Arg::I(i64::from(level).wrapping_mul(4).wrapping_sub(1)),
                    Arg::S(b"`- "),
                    Arg::S(&which),
                ],
            )
            .0
        } else {
            snprintf(
                MAXBUFSIZ,
                b"%*s%s",
                &[
                    Arg::I(i64::from(level).wrapping_mul(4)),
                    Arg::S(b" `- "),
                    Arg::S(&which),
                ],
            )
            .0
        }
    }

    // -----------------------------------------------------------------------
    // The bottom window
    // -----------------------------------------------------------------------

    /// `bot_do (str, focus)`: one element, its leading blanks as they are,
    /// highlighted when it has the focus.
    fn bot_do(&mut self, s: &[u8], focus: bool) {
        let lead = s.iter().take_while(|&&b| b == b' ').count();
        self.out.raw(s.get(..lead).unwrap_or_default());
        let s = s.get(lead..).unwrap_or_default();
        let mut cap = self.caps.norm.clone();
        if focus {
            cap = if s.contains(&self.bot.sep) {
                self.cw().capclr_msg.clone()
            } else {
                self.caps.reverse.clone()
            };
        }
        let line = Self::fmtmk(
            "%s%s%s",
            &[Arg::S(&cap), Arg::S(s), Arg::S(&self.caps.norm)],
        );
        self.out.putp(&line);
    }

    /// The bottom window's header row, and the area below it cleared.
    fn bot_header(&mut self, hdr: &[u8]) {
        let g = self.tg2(0, self.screen_rows_bot());
        let tmp = snprintf(
            BIGBUFSIZ,
            b"%s%s%-*s",
            &[
                Arg::S(&g),
                Arg::S(&self.cw().capclr_hdr.clone()),
                Arg::I(i64::from(self.screen_cols.wrapping_add(utf8::delta(hdr)))),
                Arg::S(hdr),
            ],
        )
        .0;
        self.out.putp(&tmp);
    }

    /// `bot_focus_str (hdr, str)`: the bottom window from one string cut at
    /// the separator -- with `str`, a new one under a new header; without,
    /// the last one redrawn. How many elements, less one.
    pub fn bot_focus_str(&mut self, hdr: Option<&[u8]>, s: Option<&[u8]>) -> i32 {
        if let Some(s) = s {
            let n = s.len().min(BOTBUFSIZ - 1);
            self.bot.buf = if s.is_empty() || s == b"-" {
                nls::X_BOT_NODATA_TXT.as_bytes().to_vec()
            } else {
                c_str(s.get(..n).unwrap_or_default()).to_vec()
            };
            let cols = i32::try_from(self.bot.buf.len())
                .unwrap_or(0)
                .wrapping_sub(utf8::delta(&self.bot.buf))
                .checked_div(self.screen_cols.max(1))
                .unwrap_or(0);
            self.bot.rsvd = (1 + BOT_RSVD)
                .wrapping_add(cols)
                .min(self.screen_rows.wrapping_sub(1));
            self.bot_header(hdr.unwrap_or_default());
        }
        let g = self.tg2(0, self.screen_rows_bot().wrapping_add(1));
        let line = Self::fmtmk(
            "%s%s%s",
            &[
                Arg::S(&g),
                Arg::S(&self.caps.clr_eos),
                Arg::S(&self.caps.norm),
            ],
        );
        self.out.putp(&line);

        let buf = self.bot.buf.clone();
        let sep = self.bot.sep;
        let mut beg = 0usize;
        let mut x = BOT_UNFOCUS;
        while beg < buf.len() {
            let rest = buf.get(beg..).unwrap_or_default();
            let mut n = rest.iter().position(|&b| b == sep).unwrap_or(rest.len());
            if n >= BIGBUFSIZ {
                n = BIGBUFSIZ - 1;
            }
            x = x.wrapping_add(1);
            let piece = rest.get(..n).unwrap_or_default().to_vec();
            self.bot_do(&piece, x == self.bot.indx);
            beg = beg.saturating_add(n);
            if let Some(&b) = buf.get(beg) {
                self.out.raw(&[b]);
                beg = beg.saturating_add(1);
            }
            while buf.get(beg) == Some(&b' ') {
                self.out.raw(b" ");
                beg = beg.saturating_add(1);
            }
        }
        x
    }

    /// `bot_focus_strv (hdr, strv)`: the bottom window from a vector, one
    /// element a string. How many elements, less one.
    pub fn bot_focus_strv(&mut self, hdr: Option<&[u8]>, strv: Option<&[Vec<u8>]>) -> i32 {
        if let Some(strv) = strv {
            // `(char *)&strv[0] - strv[0]`: the strings and the alignment
            // the library put after them.
            let tot: usize = strv
                .iter()
                .fold(0usize, |t, s| t.saturating_add(s.len()).saturating_add(1));
            let align = 7usize.wrapping_sub(tot.wrapping_add(7) & 7);
            let n = tot.saturating_add(align).min(BOTBUFSIZ - 1);
            let mut joined: Vec<u8> = Vec::with_capacity(tot);
            for s in strv {
                joined.extend_from_slice(s);
                joined.push(0);
            }
            joined.resize(n, 0);
            let first = c_str(&joined);
            if (first.is_empty() || first == b"-") && n <= 8 {
                // `strcpy (Bot_buf, ...)`: over the front of what was copied.
                let nodata = [nls::X_BOT_NODATA_TXT.as_bytes(), b"\0"].concat();
                for (k, &b) in nodata.iter().enumerate() {
                    match joined.get_mut(k) {
                        Some(slot) => *slot = b,
                        None => joined.push(b),
                    }
                }
            }
            self.bot.buf = joined;
            let mut p = 0usize;
            let mut x = 0i32;
            let mut nsav = 0i32;
            for s in strv {
                p = p.saturating_add(s.len()).saturating_add(1);
                if p >= BOTBUFSIZ {
                    break;
                }
                x = x.wrapping_add(utf8::delta(s));
                nsav = nsav.wrapping_add(1);
            }
            self.bot.nsav = nsav;
            let n = i32::try_from(p)
                .unwrap_or(0)
                .wrapping_sub(x.wrapping_add(1));
            self.bot.rsvd = (1 + BOT_RSVD)
                .wrapping_add(n.checked_div(self.screen_cols.max(1)).unwrap_or(0))
                .min(self.screen_rows.wrapping_sub(1));
            self.bot_header(hdr.unwrap_or_default());
        }
        let g = self.tg2(0, self.screen_rows_bot().wrapping_add(1));
        let line = Self::fmtmk(
            "%s%s%s",
            &[
                Arg::S(&g),
                Arg::S(&self.caps.clr_eos),
                Arg::S(&self.caps.norm),
            ],
        );
        self.out.putp(&line);

        let buf = self.bot.buf.clone();
        let mut p = 0usize;
        let mut x = BOT_UNFOCUS;
        for _ in 0..self.bot.nsav {
            x = x.wrapping_add(1);
            let s = c_str(buf.get(p..).unwrap_or_default()).to_vec();
            self.bot_do(&s, x == self.bot.indx);
            p = p.saturating_add(s.len()).saturating_add(1);
            self.out.raw(b" ");
        }
        x
    }

    /// `Bot_focus_func (NULL, NULL)`: the bottom window redrawn; how many
    /// elements, less one.
    pub fn bot_focus_redraw(&mut self) -> i32 {
        match self.bot.focus {
            BotFocus::Str => self.bot_focus_str(None, None),
            BotFocus::Strv => self.bot_focus_strv(None, None),
        }
    }

    /// `bot_item_show`: the bottom window for its task, if the task is still
    /// in the current window.
    pub fn bot_item_show(&mut self) {
        let w = self.curwin;
        let n = self.pidsmaxt();
        let mut found = None;
        for i in 0..n {
            if self.bot.task == self.pv_int(w, i, EU_PID) {
                found = Some(i);
                break;
            }
        }
        if let Some(i) = found {
            let cmd = self.pv_str(w, i, EU_CMD);
            let hdr = Self::fmtmk(
                self.bot.head,
                &[Arg::I(i64::from(self.bot.task)), Arg::S(&cmd)],
            );
            match self.bot.what {
                BOT_MSG_LOG => {
                    let s = self.bot_msg_log();
                    self.bot_focus_str(Some(&hdr), Some(&s));
                }
                BOT_ITEM_NS => {
                    let s = self.bot_ns_line(w, i);
                    self.bot_focus_str(Some(&hdr), Some(&s));
                }
                what if what == EU_CMDLINE_V as i32 || what == EU_ENVIRON_V as i32 => {
                    let item = usize::try_from(self.bot.item[0]).unwrap_or(0);
                    let v = self
                        .stack_of(w, i)
                        .and_then(|s| s.get(item))
                        .and_then(Val::strv)
                        .map(<[Vec<u8>]>::to_vec);
                    self.bot_focus_strv(Some(&hdr), v.as_deref());
                }
                _ => {
                    let item = usize::try_from(self.bot.item[0]).unwrap_or(0);
                    let v = self
                        .stack_of(w, i)
                        .and_then(|s| s.get(item))
                        .and_then(Val::str)
                        .map(<[u8]>::to_vec);
                    self.bot_focus_str(Some(&hdr), v.as_deref());
                }
            }
        }
        // `BOT_KEEP`: drawn once; the frames after leave it be.
        self.bot.show = false;
    }

    /// `bot_item_hlp`'s message log: the newest message first, `; ` between.
    fn bot_msg_log(&self) -> Vec<u8> {
        let len = self.msg_tab.len().max(1);
        let this = self.msg_this.checked_rem(len).unwrap_or(0);
        let prev = |i: usize| {
            i.saturating_add(len.saturating_sub(1))
                .checked_rem(len)
                .unwrap_or(0)
        };
        let mut out = Vec::new();
        let start = prev(this);
        let mut m = start;
        loop {
            let msg = self.msg_tab.get(m).cloned().unwrap_or_default();
            if !msg.is_empty() {
                out.extend_from_slice(&msg);
                let older = self.msg_tab.get(prev(m)).is_some_and(|s| !s.is_empty());
                if m != this && older {
                    out.extend_from_slice(b"; ");
                }
            }
            m = prev(m);
            if m == start {
                break;
            }
        }
        out
    }

    /// `bot_item_hlp`'s namespaces: `name: inode` for each, in `ns_tab`'s
    /// order.
    fn bot_ns_line(&self, w: usize, idx: i32) -> Vec<u8> {
        let mut out = Vec::new();
        for (k, (eu, name)) in NS_TAB.iter().enumerate() {
            let v = self
                .stack_of(w, idx)
                .and_then(|s| s.get(*eu))
                .map_or(0, Val::ul);
            let tmp = snprintf(
                SMLBUFSIZ,
                b"%s: %-10lu",
                &[Arg::S(name.as_bytes()), Arg::U(v)],
            )
            .0;
            out.extend_from_slice(&tmp);
            if k.saturating_add(1) < NS_TAB.len() {
                out.extend_from_slice(b", ");
            }
        }
        out
    }

    /// `bot_item_toggle (what, head, sep)`: the bottom window for `what`, or
    /// off if it already shows it.
    pub fn bot_item_toggle(&mut self, what: i32, head: &'static str, sep: u8) {
        if self.bot.what == what {
            self.bot.toss();
            return;
        }
        self.bot.rsvd = 0;
        if what == BOT_ITEM_NS {
            for (slot, (eu, _)) in self.bot.item.iter_mut().zip(NS_TAB.iter()) {
                *slot = i32::try_from(*eu).unwrap_or(0);
            }
            if let Some(slot) = self.bot.item.get_mut(NS_TAB.len()) {
                *slot = BOT_DELIMIT;
            }
            self.bot.focus = BotFocus::Str;
        } else if what == EU_CMDLINE_V as i32 || what == EU_ENVIRON_V as i32 {
            self.bot.item[0] = what;
            self.bot.item[1] = BOT_DELIMIT;
            self.bot.focus = BotFocus::Strv;
        } else {
            self.bot.item[0] = what;
            self.bot.item[1] = BOT_DELIMIT;
            self.bot.focus = BotFocus::Str;
        }
        self.bot.sep = sep;
        self.bot.what = what;
        self.bot.indx = BOT_UNFOCUS;
        self.bot.head = head;
        self.bot.show = true;
        let w = self.curwin;
        let beg = self.winstk[w].begtask;
        self.bot.task = self.pv_int(w, beg, EU_PID);
    }

    /// Whether window `w` shows task rows at all (`Show_TASKON`).
    #[must_use]
    pub fn taskon(&self, w: usize) -> bool {
        self.winstk[w].chk(SHOW_TASKON)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn comparisons_are_c_strings() {
        assert!(strcmp(b"abc", b"abd") < 0);
        assert!(strcmp(b"ab", b"abc") < 0);
        assert_eq!(strcasecmp(b"ABC", b"abc"), 0);
        assert!(find(b"Hello", b"ELL", true));
        assert!(!find(b"Hello", b"ELL", false));
        assert_eq!(strstr_at(b"abcabc", b"ca"), Some(2));
    }
}
