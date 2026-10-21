//! "Interactive Input" support: every command top takes, grouped as
//! upstream's `key_tab` groups them -- global, summary, task area, window,
//! and the old sort keys -- with the screens a few of them open (help,
//! filters, find).

use crate::cfmt::{Arg, snprintf};
use crate::defs::{
    BREAK_KBD, BREAK_OFF, BREAK_SCREEN, EQUWINS_XXX, EU_CGR, EU_CMD, EU_CMDLINE_V, EU_CPU,
    EU_ENVIRON_V, EU_MAXPFLGS, EU_MEM, EU_PID, EU_SGN, EU_TM2, FLAGS_OFF, FLAGS_TOG, FNDBUFSIZ,
    KBD_BTAB, KBD_CTRL_E, KBD_CTRL_G, KBD_CTRL_I, KBD_CTRL_K, KBD_CTRL_L, KBD_CTRL_N, KBD_CTRL_O,
    KBD_CTRL_P, KBD_CTRL_R, KBD_CTRL_U, KBD_DOWN, KBD_END, KBD_ENTER, KBD_ESC, KBD_HOME, KBD_LEFT,
    KBD_PGDN, KBD_PGUP, KBD_RIGHT, KBD_SPACE, KBD_UP, NOPRINT_XXX, QSRT_NORMAL, SCREENMAX,
    SHOW_CMDLIN, SHOW_COLORS, SHOW_CTIMES, SHOW_FOREST, SHOW_HIBOLD, SHOW_HICOLS, SHOW_HIROWS,
    SHOW_IDLEPS, SHOW_JRNUMS, SHOW_JRSTRS, SHOW_TASKON, SK_EB, SK_KB, SK_PB, SMLBUFSIZ,
    VIEW_CPUNOD, VIEW_CPUSUM, VIEW_LOADAV, VIEW_MEMORY, VIEW_NOBOLD, VIEW_SCROLL, VIEW_STATES,
};
use crate::fields::ADJOIN_LIMIT;
use crate::fmt::TICS_AS_LAST;
use crate::term::{GET_NUM_ESC, GET_NUM_NOT, IOKEY_ONCE, strlcpy};
use crate::{BOT_ITEM_NS, BOT_MSG_LOG, BOT_UNFOCUS, Top, nls};

/// `BOT_SEP_CMA`, `BOT_SEP_SLS`, `BOT_SEP_SMI`, `BOT_SEP_SPC`.
const BOT_SEP_CMA: u8 = b',';
const BOT_SEP_SLS: u8 = b'/';
const BOT_SEP_SMI: u8 = b';';
const BOT_SEP_SPC: u8 = b' ';
/// `E_CORE`, `E_CORES_ONLY`.
const E_CORE: i32 = 1;
const E_CORES_ONLY: i32 = 3;
/// `SCROLLAMT`: how far a variable-width column scrolls.
const SCROLLAMT: i32 = 8;
/// `SIGTERM`.
const SIGTERM: i32 = 15;

/// `key_tab`: which group handles which key.
#[derive(Clone, Copy)]
enum Group {
    Global,
    Summary,
    Task,
    Window,
    Xtra,
}

/// The keys of each group, as upstream's `char` tables hold them.
const KEY_TAB: [(Group, &[i32]); 5] = [
    (
        Group::Global,
        &[
            b'?' as i32,
            b'B' as i32,
            b'd' as i32,
            b'E' as i32,
            b'e' as i32,
            b'f' as i32,
            b'g' as i32,
            b'H' as i32,
            b'h' as i32,
            b'I' as i32,
            b'k' as i32,
            b'r' as i32,
            b's' as i32,
            b'X' as i32,
            b'Y' as i32,
            b'Z' as i32,
            b'0' as i32,
            KBD_CTRL_E,
            KBD_CTRL_G,
            KBD_CTRL_I,
            KBD_CTRL_K,
            KBD_CTRL_L,
            KBD_CTRL_N,
            KBD_CTRL_P,
            KBD_CTRL_R,
            KBD_CTRL_U,
            KBD_ENTER,
            KBD_SPACE,
            KBD_BTAB,
        ],
    ),
    (
        Group::Summary,
        &[
            b'!' as i32,
            b'1' as i32,
            b'2' as i32,
            b'3' as i32,
            b'4' as i32,
            b'5' as i32,
            b'C' as i32,
            b'l' as i32,
            b'm' as i32,
            b't' as i32,
        ],
    ),
    (
        Group::Task,
        &[
            b'#' as i32,
            b'<' as i32,
            b'>' as i32,
            b'b' as i32,
            b'c' as i32,
            b'F' as i32,
            b'i' as i32,
            b'J' as i32,
            b'j' as i32,
            b'n' as i32,
            b'O' as i32,
            b'o' as i32,
            b'R' as i32,
            b'S' as i32,
            b'U' as i32,
            b'u' as i32,
            b'V' as i32,
            b'v' as i32,
            b'x' as i32,
            b'y' as i32,
            b'z' as i32,
            KBD_CTRL_O,
        ],
    ),
    (
        Group::Window,
        &[
            b'+' as i32,
            b'-' as i32,
            b'=' as i32,
            b'_' as i32,
            b'&' as i32,
            b'A' as i32,
            b'a' as i32,
            b'G' as i32,
            b'L' as i32,
            b'w' as i32,
            KBD_UP,
            KBD_DOWN,
            KBD_LEFT,
            KBD_RIGHT,
            KBD_PGUP,
            KBD_PGDN,
            KBD_HOME,
            KBD_END,
        ],
    ),
    (
        Group::Xtra,
        &[b'M' as i32, b'N' as i32, b'P' as i32, b'T' as i32],
    ),
];

/// `k == c` for a key and a character.
fn is(k: i32, c: u8) -> bool {
    k == i32::from(c)
}

impl Top {
    /// `find_string (ch)`: `L` asks for a string, `&` finds the next row
    /// with it.
    fn find_string(&mut self, ch: i32) {
        let w = self.curwin;
        if is(ch, b'&') && self.winstk[w].findstr.is_empty() {
            self.show_msg(nls::FIND_NO_NEXT_TXT.as_bytes());
            return;
        }
        if is(ch, b'L') {
            let s = self.ioline(nls::GET_FIND_STR_TXT.as_bytes());
            if s.first() == Some(&0x1b) {
                return;
            }
            let f = snprintf(FNDBUFSIZ, b"%s", &[Arg::S(&s)]).0;
            self.winstk[w].findlen = i32::try_from(f.len()).unwrap_or(0);
            self.winstk[w].findstr = f;
            self.st.find_found = false;
        }
        if !self.winstk[w].findstr.is_empty() {
            self.winstk[w].set(NOPRINT_XXX);
            let mut i = self.winstk[w].begtask;
            while i < self.pidsmaxt() {
                let row = self.task_show(w, i);
                if !row.is_empty() && self.find_ofs(w, &row).is_some() {
                    self.st.find_found = true;
                    if i == self.winstk[w].begtask {
                        i = i.wrapping_add(1);
                        continue;
                    }
                    self.winstk[w].begtask = i;
                    return;
                }
                i = i.wrapping_add(1);
            }
            let redux = if self.st.find_found {
                nls::WORD_ANOTHER_TXT.as_bytes()
            } else {
                b""
            };
            let f = self.winstk[w].findstr.clone();
            let msg = Self::fmtmk(nls::FIND_NO_FIND_FMT, &[Arg::S(redux), Arg::S(&f)]);
            self.show_msg(&msg);
        }
    }

    /// `help_view`: the help screen, and from it the windows' help screen.
    fn help_view(&mut self) {
        let mut w = self.curwin;
        self.cursor_state = self.caps.curs_huge.clone();
        let c = self.cursor_state.clone();
        self.out.putp(&c);
        let on = nls::ON_WORD_ONLY_TXT.as_bytes();
        let off = nls::OFF_ONE_WORD_TXT.as_bytes();
        'signify_that: loop {
            let c = self.caps.clr_scr.clone();
            self.out.putp(&c);
            self.adj_geometry();
            let ext: &[u8] = if self.secure_mode {
                b""
            } else {
                nls::KEYS_HELPEXT_FMT.as_bytes()
            };
            let text = Self::fmtmk(
                nls::KEYS_HELPBAS_FMT,
                &[
                    Arg::S(crate::PACKAGE_STRING.as_bytes()),
                    Arg::S(&self.winstk[w].grpname.clone()),
                    Arg::S(if self.winstk[w].chk(SHOW_CTIMES) {
                        on
                    } else {
                        off
                    }),
                    Arg::F(f64::from(self.rc.delay_time)),
                    Arg::S(if self.secure_mode { on } else { off }),
                    Arg::S(ext),
                ],
            );
            self.show_special(true, &text);
            let c = self.caps.clr_eos.clone();
            self.out.putp(&c);
            self.out.flush();

            if crate::frames() != 0 {
                continue 'signify_that;
            }
            // `char key`: the key as upstream's `char` keeps it.
            let key = crate::term::schar(self.iokey(IOKEY_ONCE).to_le_bytes()[0]);
            if key < 1 {
                continue 'signify_that;
            }
            match key {
                k if k == KBD_ESC || is(k, b'q') => {}
                k if is(k, b'?') || is(k, b'h') || is(k, b'H') => loop {
                    let c = self.caps.clr_scr.clone();
                    self.out.putp(&c);
                    self.adj_geometry();
                    let names: Vec<Vec<u8>> =
                        self.winstk.iter().map(|x| x.rc.winname.clone()).collect();
                    let text = Self::fmtmk(
                        nls::WINDOWS_HELP_FMT,
                        &[
                            Arg::S(&self.winstk[w].grpname.clone()),
                            Arg::S(names.first().map_or(&b""[..], Vec::as_slice)),
                            Arg::S(names.get(1).map_or(&b""[..], Vec::as_slice)),
                            Arg::S(names.get(2).map_or(&b""[..], Vec::as_slice)),
                            Arg::S(names.get(3).map_or(&b""[..], Vec::as_slice)),
                        ],
                    );
                    self.show_special(true, &text);
                    let c = self.caps.clr_eos.clone();
                    self.out.putp(&c);
                    self.out.flush();
                    if crate::frames() != 0 {
                        continue;
                    }
                    let key = crate::term::schar(self.iokey(IOKEY_ONCE).to_le_bytes()[0]);
                    if key < 1 {
                        continue;
                    }
                    w = self.win_select(key);
                    if key == KBD_ENTER || key == KBD_ESC || is(key, b'q') {
                        break;
                    }
                },
                _ => continue 'signify_that,
            }
            break;
        }
        crate::set_frames(BREAK_SCREEN);
    }

    /// `other_filters (ch)`: `o`/`O` add a filter; `^O` shows them.
    fn other_filters(&mut self, ch: i32) {
        let w = self.curwin;
        if is(ch, b'o') || is(ch, b'O') {
            let txt = if is(ch, b'o') {
                nls::OSEL_CASENOT_TXT
            } else {
                nls::OSEL_CASEYES_TXT
            };
            let prompt = Self::fmtmk(
                nls::OSEL_PROMPTS_FMT,
                &[
                    Arg::I(i64::from(self.winstk[w].osel_tot()).wrapping_add(1)),
                    Arg::S(txt.as_bytes()),
                ],
            );
            let glob = self.ioline(&prompt);
            if glob.first() == Some(&0x1b) || glob.is_empty() {
                return;
            }
            if let Some(p) = self.osel_add(w, ch, &glob, true) {
                self.show_msg(&p);
            }
        } else if ch == KBD_CTRL_O && self.vizchk(w) {
            let mut buf: Vec<u8> = Vec::new();
            for o in self.winstk[w].osel.iter().rev() {
                let piece = Self::fmtmk("%s'%s'", &[Arg::S(b" + "), Arg::S(&o.raw)]);
                let room = SCREENMAX.saturating_sub(buf.len().saturating_add(1));
                buf.extend_from_slice(piece.get(..piece.len().min(room)).unwrap_or_default());
            }
            let p: Vec<u8> = if buf.is_empty() {
                nls::WORD_NONEONE_TXT.as_bytes().to_vec()
            } else {
                let skip = buf.iter().take_while(|&&b| b == b' ' || b == b'+').count();
                buf.get(skip..).unwrap_or_default().to_vec()
            };
            let line = Self::fmtmk(nls::OSEL_STATLIN_FMT, &[Arg::S(&p)]);
            self.ioline(&line);
        }
    }

    /// The task at the top of the current window: its PID.
    fn top_pid(&self) -> i32 {
        let w = self.curwin;
        self.pv_int(w, self.winstk[w].begtask, EU_PID)
    }

    /// `keys_global (ch)`.
    #[allow(clippy::too_many_lines)]
    fn keys_global(&mut self, ch: i32) {
        let w = self.curwin;
        match ch {
            k if is(k, b'?') || is(k, b'h') => self.help_view(),
            k if is(k, b'B') => {
                self.winstk[w].tog(VIEW_NOBOLD);
                self.capsmk(w);
            }
            k if is(k, b'd') || is(k, b's') => {
                if self.secure_mode {
                    self.show_msg(nls::NOT_ONSECURE_TXT.as_bytes());
                } else {
                    let prompt = Self::fmtmk(
                        nls::DELAY_CHANGE_FMT,
                        &[Arg::F(f64::from(self.rc.delay_time))],
                    );
                    let tmp = self.get_float(&prompt);
                    if tmp > -1.0 {
                        self.rc.delay_time = tmp;
                    }
                }
            }
            k if is(k, b'E') => {
                self.rc.summ_mscale = self.rc.summ_mscale.wrapping_add(1);
                if self.rc.summ_mscale > SK_EB {
                    self.rc.summ_mscale = SK_KB;
                }
            }
            k if is(k, b'e') => {
                self.rc.task_mscale = self.rc.task_mscale.wrapping_add(1);
                if self.rc.task_mscale > SK_PB {
                    self.rc.task_mscale = SK_KB;
                }
            }
            k if is(k, b'f') => self.fields_utility(),
            k if is(k, b'g') => {
                self.win_select(0);
            }
            k if is(k, b'H') => {
                self.thread_mode = !self.thread_mode;
                if !self.winstk[w].chk(VIEW_STATES) {
                    let msg =
                        Self::fmtmk(nls::THREADS_SHOW_FMT, &[Arg::S(on_off(self.thread_mode))]);
                    self.show_msg(&msg);
                }
                for x in &mut self.winstk {
                    x.begtask = 0;
                    x.focus_pid = 0;
                }
                // "force an extra procs refresh to avoid %cpu distortions..."
                self.out.pseudo_row = crate::defs::PROC_XTRA;
                crate::set_frames(BREAK_SCREEN);
            }
            k if is(k, b'I') => {
                if self.cpu_cnt > 1 {
                    self.rc.mode_irixps = i32::from(self.rc.mode_irixps == 0);
                    let msg = Self::fmtmk(
                        nls::IRIX_CURMODE_FMT,
                        &[Arg::S(on_off(self.rc.mode_irixps != 0))],
                    );
                    self.show_msg(&msg);
                } else {
                    self.show_msg(nls::NOT_SMP_CPUS_TXT.as_bytes());
                }
            }
            k if is(k, b'k') => {
                if self.secure_mode {
                    self.show_msg(nls::NOT_ONSECURE_TXT.as_bytes());
                    return;
                }
                let mut num = SIGTERM;
                let def = self.top_pid();
                let prompt = Self::fmtmk(nls::GET_PID2KILL_FMT, &[Arg::I(i64::from(def))]);
                let mut pid = self.get_int(&prompt);
                if pid > GET_NUM_ESC {
                    if pid == GET_NUM_NOT {
                        pid = def;
                    }
                    let prompt = Self::fmtmk(
                        nls::GET_SIGS_NUM_FMT,
                        &[Arg::I(i64::from(pid)), Arg::I(i64::from(SIGTERM))],
                    );
                    let s = self.ioline(&prompt);
                    if s.first() != Some(&0x1b) {
                        if !s.is_empty() {
                            num = coreutils::procps::signals::signal_name_to_number(
                                &s,
                                libcall::sigrtmin(),
                            );
                        }
                        if crate::frames() != 0 {
                            return;
                        }
                        if 0 < num {
                            if let Err(e) = libcall::kill_any(pid, num) {
                                let msg = Self::fmtmk(
                                    nls::FAIL_SIGNALS_FMT,
                                    &[
                                        Arg::I(i64::from(pid)),
                                        Arg::I(i64::from(num)),
                                        Arg::S(&crate::strerror_no(e)),
                                    ],
                                );
                                self.show_msg(&msg);
                            }
                        } else if 0 > num {
                            self.show_msg(nls::BAD_SIGNALID_TXT.as_bytes());
                        }
                    }
                }
            }
            k if is(k, b'r') => {
                if self.secure_mode {
                    self.show_msg(nls::NOT_ONSECURE_TXT.as_bytes());
                    return;
                }
                let def = self.top_pid();
                let prompt = Self::fmtmk(nls::GET_PID2NICE_FMT, &[Arg::I(i64::from(def))]);
                let mut pid = self.get_int(&prompt);
                if pid > GET_NUM_ESC {
                    if pid == GET_NUM_NOT {
                        pid = def;
                    }
                    let prompt = Self::fmtmk(nls::GET_NICE_NUM_FMT, &[Arg::I(i64::from(pid))]);
                    let num = self.get_int(&prompt);
                    if num > GET_NUM_NOT
                        && let Err(e) = libcall::priority::setpriority(0, pid.cast_unsigned(), num)
                    {
                        let msg = Self::fmtmk(
                            nls::FAIL_RE_NICE_FMT,
                            &[
                                Arg::I(i64::from(pid)),
                                Arg::I(i64::from(num)),
                                Arg::S(&crate::strerror_no(e)),
                            ],
                        );
                        self.show_msg(&msg);
                    }
                }
            }
            k if is(k, b'X') => {
                let prompt = Self::fmtmk(
                    nls::XTRA_FIXWIDE_FMT,
                    &[Arg::I(i64::from(self.rc.fixed_widest))],
                );
                let num = self.get_int(&prompt);
                if num > GET_NUM_NOT {
                    self.rc.fixed_widest = if (0..=SCREENMAX as i32).contains(&num) {
                        num
                    } else {
                        -1
                    };
                }
            }
            k if is(k, b'Y') => {
                if self.inspect.total == 0 {
                    self.ioline(nls::YINSP_NOENT1_TXT.as_bytes());
                } else {
                    let def = self.top_pid();
                    let prompt = Self::fmtmk(nls::YINSP_PIDSEE_FMT, &[Arg::I(i64::from(def))]);
                    let mut pid = self.get_int(&prompt);
                    if pid > GET_NUM_ESC {
                        if pid == GET_NUM_NOT {
                            pid = def;
                        }
                        if pid != 0 {
                            self.inspection_utility(pid);
                        }
                    }
                }
            }
            k if is(k, b'Z') => self.wins_colors(),
            k if is(k, b'0') => self.rc.zero_suppress = i32::from(self.rc.zero_suppress == 0),
            KBD_CTRL_E => {
                self.rc.tics_scaled = self.rc.tics_scaled.wrapping_add(1);
                if self.rc.tics_scaled > TICS_AS_LAST {
                    self.rc.tics_scaled = 0;
                }
            }
            KBD_CTRL_G => self.bot_item_toggle(EU_CGR as i32, nls::X_BOT_CTLGRP_FMT, BOT_SEP_SLS),
            KBD_CTRL_I => {
                if self.bot.present() {
                    self.bot.indx = self.bot.indx.wrapping_add(1);
                    if self.bot.indx > self.bot_focus_redraw() {
                        self.bot.indx = BOT_UNFOCUS;
                    }
                }
            }
            KBD_CTRL_K => {
                self.bot_item_toggle(EU_CMDLINE_V as i32, nls::X_BOT_CMDLIN_FMT, BOT_SEP_SPC);
            }
            KBD_CTRL_L => self.bot_item_toggle(BOT_MSG_LOG, nls::X_BOT_MSGLOG_TXT, BOT_SEP_SMI),
            KBD_CTRL_N => {
                self.bot_item_toggle(EU_ENVIRON_V as i32, nls::X_BOT_ENVIRN_FMT, BOT_SEP_SPC);
            }
            KBD_CTRL_P => self.bot_item_toggle(BOT_ITEM_NS, nls::X_BOT_NAMESP_FMT, BOT_SEP_CMA),
            KBD_CTRL_R => {
                if self.secure_mode {
                    self.show_msg(nls::NOT_ONSECURE_TXT.as_bytes());
                    return;
                }
                let def = self.top_pid();
                let prompt = Self::fmtmk(nls::GET_PID2NICE_FMT, &[Arg::I(i64::from(def))]);
                let mut pid = self.get_int(&prompt);
                if pid > GET_NUM_ESC {
                    if pid == GET_NUM_NOT {
                        pid = def;
                    }
                    let prompt = Self::fmtmk(nls::AGNI_VALUEOF_FMT, &[Arg::I(i64::from(pid))]);
                    let num = self.get_int(&prompt);
                    if num > GET_NUM_NOT {
                        if !(-20..=19).contains(&num) {
                            self.show_msg(nls::AGNI_INVALID_TXT.as_bytes());
                        } else {
                            let path = format!("/proc/{pid}/autogroup");
                            match std::fs::OpenOptions::new().write(true).open(&path) {
                                Err(e) => {
                                    let msg = Self::fmtmk(
                                        nls::AGNI_NOTOPEN_FMT,
                                        &[Arg::S(&crate::strerror_of(&e))],
                                    );
                                    self.show_msg(&msg);
                                }
                                Ok(mut f) => {
                                    let text = format!("{num}");
                                    let wrote = std::io::Write::write(&mut f, text.as_bytes());
                                    match wrote {
                                        Ok(n) if n > 0 => {}
                                        Ok(_) => {
                                            let msg = Self::fmtmk(
                                                nls::AGNI_NOWRITE_FMT,
                                                &[Arg::S(&crate::strerror_no(5))],
                                            );
                                            self.show_msg(&msg);
                                        }
                                        Err(e) => {
                                            let msg = Self::fmtmk(
                                                nls::AGNI_NOWRITE_FMT,
                                                &[Arg::S(&crate::strerror_of(&e))],
                                            );
                                            self.show_msg(&msg);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            KBD_CTRL_U => self.bot_item_toggle(EU_SGN as i32, nls::X_BOT_SUPGRP_FMT, BOT_SEP_CMA),
            KBD_BTAB => {
                if self.bot.present() {
                    self.bot.indx = self.bot.indx.wrapping_sub(1);
                    let num = self.bot_focus_redraw();
                    if self.bot.indx <= BOT_UNFOCUS {
                        self.bot.indx = num.wrapping_add(1);
                    }
                }
            }
            _ => {}
        }
    }

    /// `keys_summary (ch)`.
    fn keys_summary(&mut self, ch: i32) {
        let w = self.curwin;
        if self.restrict_some && !is(ch, b'C') {
            self.show_msg(nls::X_RESTRICTED_TXT.as_bytes());
            return;
        }
        match ch {
            k if is(k, b'!') => {
                if self.winstk[w].chk(VIEW_CPUSUM) || self.winstk[w].chk(VIEW_CPUNOD) {
                    self.show_msg(nls::XTRA_MODEBAD_TXT.as_bytes());
                } else {
                    let rc = &mut self.winstk[w].rc;
                    rc.combine_cpus = if rc.combine_cpus == 0 {
                        2
                    } else {
                        rc.combine_cpus.wrapping_mul(2)
                    };
                    if rc.combine_cpus >= self.cpu_cnt {
                        rc.combine_cpus = 0;
                    }
                    rc.core_types = 0;
                }
            }
            k if is(k, b'1') => {
                let win = &mut self.winstk[w];
                if win.chk(VIEW_CPUNOD) {
                    win.off(VIEW_CPUSUM);
                } else {
                    win.tog(VIEW_CPUSUM);
                }
                win.off(VIEW_CPUNOD);
                win.set(VIEW_STATES);
                win.rc.double_up = 0;
                win.rc.core_types = 0;
            }
            k if is(k, b'2') => {
                if self.numa_node_tot == 0 {
                    self.show_msg(nls::NUMA_NODENOT_TXT.as_bytes());
                } else {
                    let sel = self.numa_node_sel;
                    let win = &mut self.winstk[w];
                    if sel < 0 {
                        win.tog(VIEW_CPUNOD);
                    }
                    if !win.chk(VIEW_CPUNOD) {
                        win.set(VIEW_CPUSUM);
                    }
                    win.set(VIEW_STATES);
                    win.rc.double_up = 0;
                    win.rc.core_types = 0;
                    self.numa_node_sel = -1;
                }
            }
            k if is(k, b'3') => {
                if self.numa_node_tot == 0 {
                    self.show_msg(nls::NUMA_NODENOT_TXT.as_bytes());
                } else {
                    let prompt = Self::fmtmk(
                        nls::NUMA_NODEGET_FMT,
                        &[Arg::I(i64::from(self.numa_node_tot).wrapping_sub(1))],
                    );
                    let num = self.get_int(&prompt);
                    if num > GET_NUM_NOT {
                        if num >= 0 && num < self.numa_node_tot {
                            self.numa_node_sel = num;
                            let win = &mut self.winstk[w];
                            win.set(VIEW_CPUNOD | VIEW_STATES);
                            win.off(VIEW_CPUSUM);
                            win.rc.double_up = 0;
                            win.rc.core_types = 0;
                        } else {
                            self.show_msg(nls::NUMA_NODEBAD_TXT.as_bytes());
                        }
                    }
                }
            }
            k if is(k, b'4') => {
                let cpus = self.cpu_cnt;
                let win = &mut self.winstk[w];
                win.rc.double_up = win.rc.double_up.wrapping_add(1);
                if win.rc.double_up >= ADJOIN_LIMIT || win.rc.double_up >= cpus {
                    win.rc.double_up = 0;
                }
                if win.rc.double_up > 1 && win.rc.graph_cpus == 0 {
                    win.rc.double_up = 0;
                }
                win.off(VIEW_CPUSUM | VIEW_CPUNOD);
            }
            k if is(k, b'5') => {
                let win = &self.winstk[w];
                if !win.chk(VIEW_STATES)
                    || win.chk(VIEW_CPUSUM | VIEW_CPUNOD)
                    || win.rc.combine_cpus != 0
                {
                    self.show_msg(nls::XTRA_MODEBAD_TXT.as_bytes());
                } else {
                    let cnt = usize::try_from(self.cpu_cnt).unwrap_or(0);
                    let any_e = self
                        .stat_reap
                        .cpus
                        .iter()
                        .take(cnt)
                        .any(|c| c.type_core == E_CORE);
                    let win = &mut self.winstk[w];
                    if any_e {
                        win.rc.core_types = win.rc.core_types.wrapping_add(1);
                        if win.rc.core_types > E_CORES_ONLY {
                            win.rc.core_types = 0;
                        }
                    } else {
                        win.rc.core_types = 0;
                    }
                }
            }
            k if is(k, b'C') => self.viztog(w, VIEW_SCROLL),
            k if is(k, b'l') => self.winstk[w].tog(VIEW_LOADAV),
            k if is(k, b'm') => {
                let win = &mut self.winstk[w];
                if !win.chk(VIEW_MEMORY) {
                    win.set(VIEW_MEMORY);
                } else {
                    win.rc.graph_mems = win.rc.graph_mems.wrapping_add(1);
                    if win.rc.graph_mems > 2 {
                        win.rc.graph_mems = 0;
                        win.off(VIEW_MEMORY);
                    }
                }
            }
            k if is(k, b't') => {
                let win = &mut self.winstk[w];
                if !win.chk(VIEW_STATES) {
                    win.set(VIEW_STATES);
                } else {
                    win.rc.graph_cpus = win.rc.graph_cpus.wrapping_add(1);
                    if win.rc.graph_cpus > 2 {
                        win.rc.graph_cpus = 0;
                        win.off(VIEW_STATES);
                    }
                }
                if win.rc.double_up > 1 && win.rc.graph_cpus == 0 {
                    win.rc.double_up = 0;
                }
            }
            _ => {}
        }
    }

    /// `keys_task (ch)`.
    #[allow(clippy::too_many_lines)]
    fn keys_task(&mut self, ch: i32) {
        let w = self.curwin;
        match ch {
            k if is(k, b'#') || is(k, b'n') => {
                if self.vizchk(w) {
                    let prompt = Self::fmtmk(
                        nls::GET_MAX_TASK_FMT,
                        &[Arg::I(i64::from(self.winstk[w].rc.maxtasks))],
                    );
                    let num = self.get_int(&prompt);
                    if num > GET_NUM_NOT {
                        if -1 < num {
                            self.winstk[w].rc.maxtasks = num;
                        } else {
                            self.show_msg(nls::BAD_MAX_TASK_TXT.as_bytes());
                        }
                    }
                }
            }
            k if is(k, b'<') || is(k, b'>') => {
                if self.vizchk(w) {
                    let win = &self.winstk[w];
                    let flg = |i: i32| {
                        usize::try_from(i)
                            .ok()
                            .and_then(|u| win.procflgs.get(u))
                            .copied()
                            .unwrap_or(0)
                    };
                    let maxp = win.maxpflgs;
                    let mut p = maxp.wrapping_sub(1);
                    while p > 0 && flg(p) != win.rc.sortindx {
                        p = p.wrapping_sub(1);
                    }
                    if p >= 0 && flg(p) == win.rc.sortindx {
                        let newp = if is(k, b'<') {
                            p = p.wrapping_sub(1);
                            if p >= 0 && (EU_MAXPFLGS as i32) < flg(p) {
                                p = p.wrapping_sub(1);
                            }
                            (p >= 0).then_some(p)
                        } else {
                            p = p.wrapping_add(1);
                            if (EU_MAXPFLGS as i32) < flg(p) {
                                p = p.wrapping_add(1);
                            }
                            (p < maxp).then_some(p)
                        };
                        if let Some(p) = newp {
                            let f = flg(p);
                            let win = &mut self.winstk[w];
                            win.rc.sortindx = f;
                            win.off(SHOW_FOREST);
                        }
                    }
                }
            }
            k if is(k, b'b') => {
                self.winstk[w].tog(SHOW_HIBOLD);
                self.capsmk(w);
            }
            k if is(k, b'c') => self.viztog(w, SHOW_CMDLIN),
            k if is(k, b'F') => {
                if self.vizchk(w) && self.winstk[w].chk(SHOW_FOREST) {
                    let n = self.top_pid();
                    let win = &mut self.winstk[w];
                    win.focus_pid = if win.focus_pid == n { 0 } else { n };
                }
            }
            k if is(k, b'i') => {
                if self.st.i_w_sav != Some(w) {
                    self.st.i_beg_sav = 0;
                    self.st.i_w_sav = Some(w);
                }
                let win = &mut self.winstk[w];
                if win.chk(SHOW_IDLEPS) {
                    self.st.i_beg_sav = win.begtask;
                    win.begtask = 0;
                } else {
                    win.begtask = self.st.i_beg_sav;
                    self.st.i_beg_sav = 0;
                }
                self.viztog(w, SHOW_IDLEPS);
            }
            k if is(k, b'J') => self.viztog(w, SHOW_JRNUMS),
            k if is(k, b'j') => self.viztog(w, SHOW_JRSTRS),
            k if is(k, b'R') => {
                if self.vizchk(w) {
                    self.winstk[w].tog(QSRT_NORMAL);
                    self.winstk[w].off(SHOW_FOREST);
                }
            }
            k if is(k, b'S') => {
                if self.vizchk(w) {
                    self.winstk[w].tog(SHOW_CTIMES);
                    let msg = Self::fmtmk(
                        nls::TIME_ACCUMED_FMT,
                        &[Arg::S(on_off(self.winstk[w].chk(SHOW_CTIMES)))],
                    );
                    self.show_msg(&msg);
                }
            }
            k if is(k, b'O') || is(k, b'o') || k == KBD_CTRL_O => {
                if self.vizchk(w) {
                    self.other_filters(ch);
                }
            }
            k if is(k, b'U') || is(k, b'u') => {
                if self.vizchk(w) {
                    let s = self.ioline(nls::GET_USER_IDS_TXT.as_bytes());
                    if s.first() != Some(&0x1b)
                        && let Some(err) =
                            self.user_certify(w, &s, u8::try_from(ch).unwrap_or(b'u'))
                    {
                        self.show_msg(err.as_bytes());
                    }
                }
            }
            k if is(k, b'V') => {
                if self.vizchk(w) {
                    self.winstk[w].tog(SHOW_FOREST);
                    if !self.enu_viz(w, EU_CMD as i32) {
                        let msg = Self::fmtmk(
                            nls::FOREST_MODES_FMT,
                            &[Arg::S(on_off(self.winstk[w].chk(SHOW_FOREST)))],
                        );
                        self.show_msg(&msg);
                    }
                    if !self.winstk[w].chk(SHOW_FOREST) {
                        self.winstk[w].focus_pid = 0;
                    }
                }
            }
            k if is(k, b'v') => {
                if self.vizchk(w) && self.winstk[w].chk(SHOW_FOREST) {
                    let pid = self.top_pid();
                    let tot = usize::try_from(self.st.hide_tot).unwrap_or(0);
                    let at = self
                        .st
                        .hide_pid
                        .iter()
                        .take(tot)
                        .position(|&h| h == pid || h == pid.wrapping_neg());
                    match at {
                        Some(i) => {
                            if let Some(h) = self.st.hide_pid.get_mut(i) {
                                *h = h.wrapping_neg();
                            }
                            // "if everything's expanded, let's empty the array"
                            if !self.st.hide_pid.iter().take(tot).any(|&h| h > 0) {
                                self.st.hide_tot = 0;
                            }
                        }
                        None => {
                            if self.st.hide_pid.len() <= tot {
                                self.st.hide_pid.resize(tot.saturating_add(128), 0);
                            }
                            if let Some(slot) = self.st.hide_pid.get_mut(tot) {
                                *slot = pid;
                            }
                            self.st.hide_tot = self.st.hide_tot.wrapping_add(1);
                        }
                    }
                }
            }
            k if is(k, b'x') => {
                if self.vizchk(w) {
                    let sortindx = self.winstk[w].rc.sortindx;
                    if self.enu_viz(w, sortindx) {
                        self.winstk[w].tog(SHOW_HICOLS);
                        let pos = self.enu_pos(w, sortindx).unwrap_or(-1);
                        let win = &mut self.winstk[w];
                        if pos < win.begpflg {
                            if win.chk(SHOW_HICOLS) {
                                win.begpflg = win.begpflg.wrapping_add(2);
                            } else {
                                win.begpflg = win.begpflg.wrapping_sub(2);
                            }
                            if 0 > win.begpflg {
                                win.begpflg = 0;
                            }
                        }
                        self.capsmk(w);
                    }
                }
            }
            k if is(k, b'y') => {
                if self.vizchk(w) {
                    self.winstk[w].tog(SHOW_HIROWS);
                    self.capsmk(w);
                }
            }
            k if is(k, b'z') => {
                if self.vizchk(w) {
                    self.winstk[w].tog(SHOW_COLORS);
                    self.capsmk(w);
                }
            }
            _ => {}
        }
    }

    /// `VARright (w)`: the only column shown is a variable-width one, which
    /// left and right then scroll.
    fn var_right(&self, w: usize) -> bool {
        let win = &self.winstk[w];
        (win.maxpflgs == 1 && self.varcol(win.procflgs[0]))
            || (win.maxpflgs == 3
                && win.procflgs[0] == crate::defs::EU_XON as i32
                && self.varcol(win.procflgs[1]))
    }

    /// `keys_window (ch)`.
    #[allow(clippy::too_many_lines)]
    fn keys_window(&mut self, ch: i32) {
        let w = self.curwin;
        let altscr = self.rc.mode_altscr != 0;
        match ch {
            k if is(k, b'+') => {
                if self.altchk() {
                    self.wins_reflag(FLAGS_OFF, EQUWINS_XXX);
                }
                self.st.hide_tot = 0;
            }
            k if is(k, b'-') => {
                if self.altchk() {
                    self.winstk[w].tog(SHOW_TASKON);
                }
            }
            k if is(k, b'=') => {
                self.win_reset(w);
                self.st.hide_tot = 0;
            }
            k if is(k, b'_') => {
                if self.altchk() {
                    self.wins_reflag(FLAGS_TOG, SHOW_TASKON);
                }
            }
            k if is(k, b'&') || is(k, b'L') => {
                if self.vizchk(w) {
                    self.find_string(ch);
                }
            }
            k if is(k, b'A') => self.rc.mode_altscr = i32::from(self.rc.mode_altscr == 0),
            k if is(k, b'a') || is(k, b'w') => {
                if self.altchk() {
                    self.win_select(ch);
                }
            }
            k if is(k, b'G') => {
                if self.altchk() {
                    let prompt = Self::fmtmk(
                        nls::NAME_WINDOWS_FMT,
                        &[Arg::S(&self.winstk[w].rc.winname.clone())],
                    );
                    let line = self.ioline(&prompt);
                    let tmp = strlcpy(&line, SMLBUFSIZ);
                    if !tmp.is_empty() && tmp.first() != Some(&0x1b) {
                        self.win_names(w, &tmp);
                    }
                }
            }
            KBD_UP | KBD_DOWN => {
                if self.vizchk(w) && self.winstk[w].chk(SHOW_IDLEPS) {
                    self.winstk[w].begnext = if ch == KBD_UP { -1 } else { 1 };
                }
            }
            KBD_LEFT => {
                if self.vizchk(w) {
                    let var_left = self.winstk[w].varcolbeg != 0 && self.var_right(w);
                    let win = &mut self.winstk[w];
                    if var_left {
                        win.varcolbeg = win.varcolbeg.wrapping_sub(SCROLLAMT);
                    } else if 0 < win.begpflg {
                        win.begpflg = win.begpflg.wrapping_sub(1);
                        let at = usize::try_from(win.begpflg).unwrap_or(0);
                        if (EU_MAXPFLGS as i32) < win.pflgsall.get(at).copied().unwrap_or(0) {
                            win.begpflg = win.begpflg.wrapping_sub(2);
                        }
                    }
                }
            }
            KBD_RIGHT => {
                if self.vizchk(w) {
                    let var_right = self.var_right(w);
                    let win = &mut self.winstk[w];
                    if var_right {
                        win.varcolbeg = win.varcolbeg.wrapping_add(SCROLLAMT);
                        if 0 > win.varcolbeg {
                            win.varcolbeg = 0;
                        }
                    } else if win.begpflg.wrapping_add(1) < win.totpflgs {
                        let at = usize::try_from(win.begpflg).unwrap_or(0);
                        if (EU_MAXPFLGS as i32) < win.pflgsall.get(at).copied().unwrap_or(0) {
                            let step = if win.begpflg.wrapping_add(3) < win.totpflgs {
                                3
                            } else {
                                0
                            };
                            win.begpflg = win.begpflg.wrapping_add(step);
                        } else {
                            win.begpflg = win.begpflg.wrapping_add(1);
                        }
                    }
                }
            }
            KBD_PGUP => {
                if self.vizchk(w) {
                    let win = &mut self.winstk[w];
                    if win.chk(SHOW_IDLEPS) && 0 < win.begtask {
                        win.begnext = win
                            .winlines
                            .wrapping_sub(if altscr { 1 } else { 2 })
                            .wrapping_neg();
                    }
                }
            }
            KBD_PGDN => {
                if self.vizchk(w) {
                    let n = self.pidsmaxt();
                    let win = &mut self.winstk[w];
                    if win.chk(SHOW_IDLEPS) && win.begtask < n.wrapping_sub(1) {
                        win.begnext = win.winlines.wrapping_sub(if altscr { 1 } else { 2 });
                    }
                }
            }
            KBD_HOME => {
                if self.vizchk(w) && self.winstk[w].chk(SHOW_IDLEPS) {
                    let win = &mut self.winstk[w];
                    win.begtask = 0;
                    win.begpflg = 0;
                    win.varcolbeg = 0;
                }
            }
            KBD_END => {
                if self.vizchk(w) && self.winstk[w].chk(SHOW_IDLEPS) {
                    let n = self.pidsmaxt();
                    let win = &mut self.winstk[w];
                    win.begnext = n.wrapping_sub(win.winlines).wrapping_add(1);
                    win.begpflg = win.endpflg;
                    win.varcolbeg = 0;
                }
            }
            _ => {}
        }
    }

    /// `keys_xtra (ch)`: the old sort keys.
    fn keys_xtra(&mut self, ch: i32) {
        let win = self.cwm();
        win.off(SHOW_FOREST);
        win.rc.sortindx = match u8::try_from(ch) {
            Ok(b'M') => EU_MEM as i32,
            Ok(b'N') => EU_PID as i32,
            Ok(b'P') => EU_CPU as i32,
            Ok(b'T') => EU_TM2 as i32,
            _ => win.rc.sortindx,
        };
    }

    /// `do_key (ch)`: one key from the main loop.
    pub fn do_key(&mut self, ch: i32) {
        crate::set_frames(BREAK_OFF);
        'all_done: {
            if ch == 0 || ch == KBD_ESC {
                break 'all_done;
            }
            if is(ch, b'q') {
                self.bye_bye(None);
            }
            if is(ch, b'W') {
                self.write_rcfile();
                break 'all_done;
            }
            // "and just in case 'Monpids' is active but matched no processes"
            if self.pidsmaxt() == 0 && !is(ch, b'=') {
                break 'all_done;
            }
            // `strchr (keys, ch)`: the key as a `char`.
            let byte = ch.to_le_bytes()[0];
            for (group, keys) in KEY_TAB {
                if keys.iter().any(|&k| k.to_le_bytes()[0] == byte) {
                    match group {
                        Group::Global => self.keys_global(ch),
                        Group::Summary => self.keys_summary(ch),
                        Group::Task => self.keys_task(ch),
                        Group::Window => self.keys_window(ch),
                        Group::Xtra => self.keys_xtra(ch),
                    }
                    if crate::frames() == BREAK_OFF {
                        crate::set_frames(BREAK_KBD);
                    }
                    let cw = self.cwm();
                    if cw.chk(SHOW_TASKON) && cw.begnext == 0 {
                        cw.begnext = 1;
                        cw.begtask = cw.begtask.wrapping_sub(1);
                    }
                    break 'all_done;
                }
            }
            self.show_msg(nls::UNKNOWN_CMDS_TXT.as_bytes());
        }
        self.cursor_state = self.caps.curs_hide.clone();
        let c = self.cursor_state.clone();
        self.out.putp(&c);
    }
}

/// `ON_word_only_txt` or `OFF_one_word_txt`.
fn on_off(on: bool) -> &'static [u8] {
    if on {
        nls::ON_WORD_ONLY_TXT.as_bytes()
    } else {
        nls::OFF_ONE_WORD_TXT.as_bytes()
    }
}
