//! The terminal: upstream's "Misc Color/Display support" and "Low Level
//! Memory/Keyboard/File I/O support" -- the capabilities top builds once,
//! the three ways it writes (`PUTT`, `PUFF`, `POOF`), the messages and
//! prompts on the message row, `show_special`'s attribute markup, and the
//! keyboard: single keys, escape sequences, and the edited input line with
//! its recall.
//!
//! Everything top writes goes through `putp` -- ncurses' `tputs` with
//! `putchar` -- into standard output's buffer, which is flushed at the end
//! of each frame and before each pause, as upstream flushes stdio.

use coreutils::stdfd::Stream;
use terminfo::{Entry, Outc, Padding, Tparm};

use crate::cfmt::{Arg, snprintf};
use crate::defs::{
    CAPBUFSIZ, CLRBUFSIZ, KBD_BKSP, KBD_BTAB, KBD_DEL, KBD_DOWN, KBD_END, KBD_ENTER, KBD_ESC,
    KBD_HOME, KBD_INS, KBD_LEFT, KBD_PGDN, KBD_PGUP, KBD_RIGHT, KBD_UP, LRGBUFSIZ, MEDBUFSIZ,
    MSG_USLEEP, ROWMAXSIZ, SHOW_COLORS, SHOW_HIBOLD, SHOW_HICOLS, SMLBUFSIZ, VIEW_NOBOLD,
};
use crate::{Top, nls, utf8};

/// `IOKEY_INIT`, `IOKEY_ONCE`, `IOKEY_NEXT`.
pub const IOKEY_INIT: i32 = 0;
pub const IOKEY_ONCE: i32 = 1;
pub const IOKEY_NEXT: i32 = 2;

/// `GET_NUM_BAD`, `GET_NUM_ESC`, `GET_NUM_NOT`: what `get_int` and
/// `get_float` answer for bad input, an escape, and nothing.
pub const GET_NUM_BAD: i32 = i32::MIN;
pub const GET_NUM_ESC: i32 = i32::MIN + 1;
pub const GET_NUM_NOT: i32 = i32::MIN + 2;

/// `STRLCPY (dst, src)` into a buffer of `size`: at most `size - 1` bytes.
#[must_use]
pub fn strlcpy(src: &[u8], size: usize) -> Vec<u8> {
    src.get(..src.len().min(size.saturating_sub(1)))
        .unwrap_or_default()
        .to_vec()
}

/// A C string's bytes: everything before its first NUL.
#[must_use]
pub fn c_str(s: &[u8]) -> &[u8] {
    match s.iter().position(|&b| b == 0) {
        Some(n) => s.get(..n).unwrap_or_default(),
        None => s,
    }
}

/// `(int)(char) b`: a byte as x86's signed `char` widens.
#[must_use]
pub fn schar(b: u8) -> i32 {
    i32::from(b.cast_signed())
}

/// The terminal `setupterm` found, as `term.h`'s macros read it.
#[derive(Default)]
pub struct Tinfo {
    /// `cur_term->type`: the legacy copy the macros read.
    pub entry: Option<Entry>,
    pub tparm: Tparm,
    /// `Cap_can_goto`.
    pub can_goto: bool,
    pub keypad_local: Option<Vec<u8>>,
    pub keypad_xmit: Option<Vec<u8>>,
    /// `max_colors`, `columns`, `lines`: the copy's `short`s.
    pub max_colors: i32,
    pub columns: i32,
    pub lines: i32,
}

/// A number as the legacy copy keeps it: in a `short`, larger values
/// limited to its maximum.
fn short_num(n: i32) -> i32 {
    n.min(i32::from(i16::MAX))
}

impl Tinfo {
    /// A string capability by name, `None` for one the terminal lacks.
    #[must_use]
    pub fn str(&self, name: &str) -> Option<Vec<u8>> {
        self.entry
            .as_ref()
            .and_then(|e| e.ext_string(name.as_bytes()))
            .map(<[u8]>::to_vec)
    }

    /// `tIF (s)`: the capability, or the empty string.
    #[must_use]
    pub fn tif(&self, name: &str) -> Vec<u8> {
        self.str(name).unwrap_or_default()
    }

    /// A boolean capability by name.
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        self.entry
            .as_ref()
            .is_some_and(|e| e.tigetflag(name.as_bytes()) == 1)
    }

    /// `tparm (s, a, 0, 0, 0, 0, 0, 0, 0, 0)`; an absent capability (where
    /// upstream would copy from a null pointer) is the empty string.
    pub fn tpm(&mut self, name: &str, a: i64) -> Vec<u8> {
        let Some(s) = self.str(name) else {
            return Vec::new();
        };
        let Some(entry) = self.entry.as_ref() else {
            return Vec::new();
        };
        self.tparm
            .tiparm(entry, &s, &[a, 0, 0, 0, 0, 0, 0, 0, 0])
            .unwrap_or_default()
    }

    /// `tgoto (cursor_address, x, y)`: `tparm (cup, y, x)`, or `None` with no
    /// `cup` (or one that does not take two numbers).
    ///
    /// Upstream's `tgoto` reads a capability with no `%p` in it as termcap
    /// would; compiled terminfo has none such, so that branch is not here.
    pub fn tgoto(&mut self, x: i32, y: i32) -> Option<Vec<u8>> {
        let cup = self.str("cup")?;
        let entry = self.entry.as_ref()?;
        self.tparm
            .tiparm(entry, &cup, &[i64::from(y), i64::from(x)])
    }

    /// Fill in from what `setupterm` found.
    pub fn set(&mut self, entry: Entry) {
        self.max_colors = short_num(entry.number(terminfo::number::MAX_COLORS));
        self.columns = short_num(entry.number(0));
        self.lines = short_num(entry.number(2));
        self.entry = Some(entry);
        self.keypad_local = self.str("rmkx");
        self.keypad_xmit = self.str("smkx");
    }
}

/// The capabilities built once (`capsmk`'s `capsdone`), empty in batch mode.
#[derive(Clone, Debug, Default)]
pub struct Caps {
    pub done: bool,
    pub clr_eol: Vec<u8>,
    pub nl_clreos: Vec<u8>,
    pub clr_scr: Vec<u8>,
    pub curs_norm: Vec<u8>,
    pub curs_huge: Vec<u8>,
    pub curs_hide: Vec<u8>,
    pub clr_eos: Vec<u8>,
    pub home: Vec<u8>,
    pub norm: Vec<u8>,
    pub reverse: Vec<u8>,
    pub caps_off: Vec<u8>,
    pub caps_endline: Vec<u8>,
    pub rmam: Vec<u8>,
    pub smam: Vec<u8>,
    /// `Cap_avoid_eol`: a terminal that wraps at the last column, which top
    /// then keeps out of.
    pub avoid_eol: bool,
}

/// `tputs`'s `outc` into standard output.
struct ToStream<'a>(&'a mut Stream);

impl Outc for ToStream<'_> {
    fn put(&mut self, bytes: &[u8]) {
        // A failure is the stream's to remember, as stdio's error flag.
        let _ = std::io::Write::write_all(self.0, bytes);
    }

    fn flush(&mut self) {
        // A failure is the stream's to remember, as stdio's error flag; the
        // close at exit reports it.
        let _ = self.0.flush_now();
    }
}

/// The size of the buffer stdio gives `stderr` reopened on `/dev/null`:
/// that device's block size.
const ERR_BUFSIZ: usize = 4096;

/// Standard output, and the `Pseudo_screen` that spares the terminal a row
/// it already shows.
pub struct Out {
    pub stream: Option<Stream>,
    pub padding: Padding,
    pub batch: bool,
    /// `Pseudo_screen`: every row as last written; rows past `pseudo_rows`
    /// are allocated but not in use.
    pub pseudo: Vec<Vec<u8>>,
    pub pseudo_rows: usize,
    /// `Pseudo_row`.
    pub pseudo_row: i32,
    /// `Stderr_save`: standard error as it was before the run.
    pub stderr_save: i32,
    /// What upstream's `stderr` stream holds. Once `before` has reopened it
    /// on `/dev/null` it is fully buffered, and stays so: what is written to
    /// it waits -- past the moment `at_eoj` puts the descriptor back -- until
    /// `exit` flushes it to the real standard error. So getopt's complaint
    /// reaches the terminal, and `sig_abexit`'s note, which ends in `raise`
    /// or `_exit`, never does.
    pub err_held: Vec<u8>,
    /// Whether `stderr` is that buffered stream: the redirect took place.
    pub err_buffered: bool,
}

impl Out {
    /// Standard output, block-buffered or not as stdio would have it.
    #[must_use]
    pub fn new(stderr_save: i32) -> Self {
        Self {
            stream: Some(Stream::stdout()),
            padding: Padding::default(),
            batch: false,
            pseudo: Vec::new(),
            pseudo_rows: 0,
            pseudo_row: crate::defs::PROC_XTRA,
            stderr_save,
            err_held: Vec::new(),
            err_buffered: stderr_save >= 0,
        }
    }

    /// `fputs (s, stderr)`: held while the stream is buffered -- a buffer's
    /// worth, `/dev/null`'s block size, going out at once to wherever
    /// descriptor 2 then leads -- else written.
    pub fn err(&mut self, s: &[u8]) {
        if !self.err_buffered {
            // A standard error that cannot be written has no one to tell.
            let _ = libcall::fd::write_all(2, s, false);
            return;
        }
        if self.err_held.len().saturating_add(s.len()) > ERR_BUFSIZ {
            self.err_flush();
        }
        self.err_held.extend_from_slice(s);
    }

    /// `exit`'s flush of `stderr`.
    pub fn err_flush(&mut self) {
        if !self.err_held.is_empty() {
            // Nothing is left to tell of a standard error that fails.
            let _ = libcall::fd::write_all(2, &self.err_held, false);
            self.err_held.clear();
        }
    }

    /// `putp (s)`.
    pub fn putp(&mut self, s: &[u8]) {
        if let Some(st) = self.stream.as_mut() {
            terminfo::putp_to(s, &self.padding, &mut ToStream(st));
        }
    }

    /// `fputs (s, stdout)`, `putchar`: no padding read into it.
    pub fn raw(&mut self, s: &[u8]) {
        if let Some(st) = self.stream.as_mut() {
            // A failure is the stream's to remember; the close at exit reports it.
            let _ = std::io::Write::write_all(st, s);
        }
    }

    /// `fflush (stdout)`.
    pub fn flush(&mut self) {
        if let Some(st) = self.stream.as_mut() {
            // A failure is the stream's to remember; the close at exit reports it.
            let _ = st.flush_now();
        }
    }

    /// `PUTT (fmt, ...)`.
    pub fn putt(&mut self, fmt: &[u8], args: &[Arg<'_>]) {
        let s = snprintf(ROWMAXSIZ, fmt, args).0;
        self.putp(&s);
    }

    /// `PUFF (fmt, ...)`: a whole row, its trailing blanks dropped in batch
    /// mode, and not sent at all (a newline instead) when the row on the
    /// screen is already this.
    pub fn puff(&mut self, screen_rows: i32, fmt: &[u8], args: &[Arg<'_>]) {
        let s = snprintf(ROWMAXSIZ, fmt, args).0;
        if self.batch {
            let end = s
                .iter()
                .rposition(|&b| b != b' ')
                .map_or(0, |i| i.saturating_add(1));
            let trimmed = s.get(..end).unwrap_or_default().to_vec();
            self.putp(&trimmed);
        } else if self.pseudo_row >= 0 && self.pseudo_row < screen_rows {
            let at = usize::try_from(self.pseudo_row).unwrap_or(0);
            self.pseudo_row = self.pseudo_row.saturating_add(1);
            let same = self.pseudo.get(at).is_some_and(|row| *row == s);
            if same {
                self.putp(b"\n");
            } else {
                if let Some(row) = self.pseudo.get_mut(at) {
                    row.clone_from(&s);
                }
                self.putp(&s);
            }
        }
    }

    /// `POOF (str, cap)`: written straight out, its row forgotten.
    pub fn poof(&mut self, screen_rows: i32, s: &[u8], cap: &[u8]) {
        self.putp(s);
        self.putp(cap);
        if let Some(row) = usize::try_from(self.pseudo_row)
            .ok()
            .and_then(|at| self.pseudo.get_mut(at))
        {
            row.clear();
        }
        if self.pseudo_row.saturating_add(1) < screen_rows {
            self.pseudo_row = self.pseudo_row.saturating_add(1);
        }
    }

    /// `PSU_CLREOS (y)`: every row from `y` on forgotten.
    pub fn psu_clreos(&mut self, y: i32) {
        let from = usize::try_from(y).unwrap_or(0);
        for row in self.pseudo.iter_mut().take(self.pseudo_rows).skip(from) {
            row.clear();
        }
    }
}

/// Standard error made `/dev/null`, its descriptor kept to put back: the
/// returned number, or -1.
pub fn redirect_stderr() -> i32 {
    let Ok(saved) = libcall::fd::dup_above(2, 3) else {
        return -1;
    };
    #[cfg(unix)]
    if let Ok(null) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
        let fd = std::os::fd::AsRawFd::as_raw_fd(&null);
        // `freopen`'s result is not looked at upstream either: standard
        // error stays where it was if `/dev/null` cannot take its place.
        let _ = libcall::fd::dup2(fd, 2);
    }
    saved
}

/// Standard error put back as [`redirect_stderr`] found it.
pub fn restore_stderr(saved: i32) {
    if saved >= 0 {
        // The program is ending: a standard error that cannot be put back,
        // or a copy that will not close, has nothing left to spoil.
        let _ = libcall::fd::dup2(saved, 2);
        let _ = libcall::fd::close(saved);
    }
}

/// `iokey`'s statics: its table of key sequences, and what it has read but
/// not handed out.
#[derive(Clone, Debug)]
pub struct IoKey {
    pub tab: Vec<(Option<Vec<u8>>, i32)>,
    pub buf: [u8; MEDBUFSIZ],
    pub pos: usize,
    pub len: usize,
}

impl Default for IoKey {
    fn default() -> Self {
        let fixed: [(&[u8], i32); 19] = [
            (b"\x08", KBD_BKSP),
            (b"\x7f", KBD_BKSP),
            (b"\x1bh", KBD_LEFT),
            (b"\x1bj", KBD_DOWN),
            (b"\x1bk", KBD_UP),
            (b"\x1bl", KBD_RIGHT),
            (b"\x1b\x08", KBD_HOME),
            (b"\x1b\x0a", KBD_PGDN),
            (b"\x1b\x0b", KBD_PGUP),
            (b"\x1b\x0c", KBD_END),
            (b"\xc3\xa8", KBD_LEFT),
            (b"\xc3\xaa", KBD_DOWN),
            (b"\xc3\xab", KBD_UP),
            (b"\xc3\xac", KBD_RIGHT),
            (b"\xc2\x88", KBD_HOME),
            (b"\xc2\x8a", KBD_PGDN),
            (b"\xc2\x8b", KBD_PGUP),
            (b"\xc2\x8c", KBD_END),
            (b"\x1b\x09", KBD_BTAB),
        ];
        let mut tab: Vec<(Option<Vec<u8>>, i32)> = [
            KBD_BKSP, KBD_INS, KBD_DEL, KBD_LEFT, KBD_DOWN, KBD_UP, KBD_RIGHT, KBD_HOME, KBD_PGDN,
            KBD_PGUP, KBD_END, KBD_BTAB,
        ]
        .iter()
        .map(|&k| (None, k))
        .collect();
        tab.extend(fixed.iter().map(|&(s, k)| (Some(s.to_vec()), k)));
        Self {
            tab,
            buf: [0; MEDBUFSIZ],
            pos: 0,
            len: 0,
        }
    }
}

/// `ioline`'s statics: its buffer, overtype mode, and the strings it
/// recalls -- the empty top of the stack first, then newest to oldest.
#[derive(Clone, Debug)]
pub struct IoLine {
    pub buf: [u8; MEDBUFSIZ + 1],
    pub ovt: bool,
    pub saved: Vec<Vec<u8>>,
}

impl Default for IoLine {
    fn default() -> Self {
        Self {
            buf: [0; MEDBUFSIZ + 1],
            ovt: false,
            saved: vec![Vec::new()],
        }
    }
}

/// `strlen (buf)` of a fixed buffer.
fn strlen(buf: &[u8]) -> usize {
    c_str(buf).len()
}

/// `iscntrl (c)` for a C `char`: the C0 controls and DEL (the high half is
/// not, in the locales this tree has).
fn is_cntrl(c: i32) -> bool {
    (0..0x20).contains(&c) || c == 0x7f
}

impl Top {
    /// `tg2 (x, y)`: a cursor move, or nothing on a terminal without one
    /// (and in batch mode).
    pub fn tg2(&mut self, x: i32, y: i32) -> Vec<u8> {
        if self.ti.can_goto {
            self.ti.tgoto(x, y).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    /// `capsmk (q)`: the terminal strings of window `q`'s colours and
    /// emphasis -- and, the first time, the ones every window shares.
    pub fn capsmk(&mut self, q: usize) {
        if self.batch {
            return;
        }
        if !self.caps.done {
            let ti = &self.ti;
            let c = &mut self.caps;
            c.clr_eol = strlcpy(&ti.tif("el"), CAPBUFSIZ);
            c.clr_eos = strlcpy(&ti.tif("ed"), CAPBUFSIZ);
            c.clr_scr = strlcpy(&ti.tif("clear"), CAPBUFSIZ);
            c.nl_clreos = strlcpy(&[b"\n", ti.tif("ed").as_slice()].concat(), CAPBUFSIZ);
            c.curs_huge = strlcpy(&ti.tif("cvvis"), CAPBUFSIZ);
            c.curs_norm = strlcpy(&ti.tif("cnorm"), CAPBUFSIZ);
            c.curs_hide = strlcpy(&ti.tif("civis"), CAPBUFSIZ);
            c.home = strlcpy(&ti.tif("home"), CAPBUFSIZ);
            c.norm = strlcpy(&ti.tif("sgr0"), CAPBUFSIZ);
            c.reverse = strlcpy(&ti.tif("rev"), CAPBUFSIZ);
            if !ti.flag("xenl") {
                c.rmam = strlcpy(&ti.tif("rmam"), CAPBUFSIZ);
                c.smam = strlcpy(&ti.tif("smam"), CAPBUFSIZ);
                if c.rmam.is_empty() || c.smam.is_empty() {
                    c.rmam.clear();
                    c.smam.clear();
                    if ti.flag("am") {
                        c.avoid_eol = true;
                    }
                }
                let rmam = c.rmam.clone();
                self.out.putp(&rmam);
            }
            let ti = &self.ti;
            let c = &mut self.caps;
            c.caps_off = strlcpy(
                &[c.norm.as_slice(), ti.tif("op").as_slice()].concat(),
                CAPBUFSIZ,
            );
            c.caps_endline = strlcpy(
                &[c.caps_off.as_slice(), c.clr_eol.as_slice()].concat(),
                SMLBUFSIZ,
            );
            if self.ti.tgoto(1, 1).is_some() {
                self.ti.can_goto = true;
            }
            self.caps.done = true;
        }

        let bold = if self.winstk[q].chk(VIEW_NOBOLD) {
            self.caps.norm.clone()
        } else {
            self.ti.tif("bold")
        };
        self.winstk[q].cap_bold = strlcpy(&bold, CAPBUFSIZ);
        if self.winstk[q].chk(SHOW_COLORS) && self.ti.max_colors > 0 {
            let rc = self.winstk[q].rc.clone();
            let sum = self.ti.tpm("setaf", i64::from(rc.summclr));
            let msg = self.ti.tpm("setaf", i64::from(rc.msgsclr));
            let hdr = self.ti.tpm("setaf", i64::from(rc.headclr));
            let task = self.ti.tpm("setaf", i64::from(rc.taskclr));
            let w = &mut self.winstk[q];
            let c = &self.caps;
            w.capclr_sum = strlcpy(&sum, CLRBUFSIZ);
            w.capclr_msg = strlcpy(&[msg.as_slice(), c.reverse.as_slice()].concat(), CLRBUFSIZ);
            w.capclr_pmt = strlcpy(&[msg.as_slice(), w.cap_bold.as_slice()].concat(), CLRBUFSIZ);
            w.capclr_hdr = strlcpy(&[hdr.as_slice(), c.reverse.as_slice()].concat(), CLRBUFSIZ);
            w.capclr_rownorm = strlcpy(
                &[c.caps_off.as_slice(), task.as_slice()].concat(),
                CLRBUFSIZ,
            );
        } else {
            let w = &mut self.winstk[q];
            let c = &self.caps;
            w.capclr_sum.clear();
            w.capclr_msg = strlcpy(&c.reverse, CLRBUFSIZ);
            w.capclr_pmt = strlcpy(&w.cap_bold, CLRBUFSIZ);
            w.capclr_hdr = strlcpy(&c.reverse, CLRBUFSIZ);
            w.capclr_rownorm = strlcpy(&c.norm, CLRBUFSIZ);
        }
        let w = &mut self.winstk[q];
        let hi = if w.chk(SHOW_HIBOLD) {
            w.cap_bold.clone()
        } else {
            self.caps.reverse.clone()
        };
        w.capclr_rowhigh = strlcpy(
            &[w.capclr_rownorm.as_slice(), hi.as_slice()].concat(),
            SMLBUFSIZ,
        );
    }

    /// `show_msg (str)`: a message on the message row, kept in the log, for
    /// a moment.
    pub fn show_msg(&mut self, s: &[u8]) {
        let i = self.msg_this;
        if let Some(slot) = self.msg_tab.get_mut(i) {
            *slot = strlcpy(s, SMLBUFSIZ);
        }
        self.msg_this = i
            .saturating_add(1)
            .checked_rem(self.msg_tab.len())
            .unwrap_or(0);

        let g = self.tg2(0, self.msg_row);
        let w = self.curwin;
        let n = utf8::embody(s, self.screen_cols.wrapping_sub(2));
        self.out.putt(
            b"%s%s %.*s %s%s%s",
            &[
                Arg::S(&g),
                Arg::S(&self.winstk[w].capclr_msg),
                Arg::I(i64::from(n)),
                Arg::S(s),
                Arg::S(&self.caps.curs_hide),
                Arg::S(&self.caps.caps_off),
                Arg::S(&self.caps.clr_eol),
            ],
        );
        self.out.flush();
        crate::nap(MSG_USLEEP);
        self.check_signals();
    }

    /// `show_pmt (str)`: a prompt on the message row with the big cursor;
    /// the column the input begins at.
    pub fn show_pmt(&mut self, s: &[u8]) -> i32 {
        let buf = snprintf(
            MEDBUFSIZ,
            b"%.*s",
            &[
                Arg::I(i64::from(utf8::embody(s, self.screen_cols.wrapping_sub(2)))),
                Arg::S(s),
            ],
        )
        .0;
        let delta = utf8::delta(&buf);
        let g = self.tg2(0, self.msg_row);
        let w = self.curwin;
        self.out.putt(
            b"%s%s%.*s %s%s%s",
            &[
                Arg::S(&g),
                Arg::S(&self.winstk[w].capclr_pmt),
                Arg::I(i64::from(
                    self.screen_cols.wrapping_sub(2).wrapping_add(delta),
                )),
                Arg::S(&buf),
                Arg::S(&self.caps.curs_huge),
                Arg::S(&self.caps.caps_off),
                Arg::S(&self.caps.clr_eol),
            ],
        );
        self.out.flush();
        let len = i32::try_from(buf.len()).unwrap_or(0).wrapping_sub(delta);
        if len.wrapping_add(1) < self.screen_cols {
            len.wrapping_add(1)
        } else {
            self.screen_cols.wrapping_sub(1)
        }
    }

    /// `show_scroll`: the scroll coordinates on the message row.
    pub fn show_scroll(&mut self) {
        let w = self.curwin;
        let mut totpflgs = self.winstk[w].totpflgs;
        let mut begpflgs = self.winstk[w].begpflg.wrapping_add(1);
        if self.winstk[w].chk(SHOW_HICOLS) {
            totpflgs = totpflgs.wrapping_sub(2);
            // `ENUpos`: a sort field not among the columns is a negative
            // position here; upstream's pointer arithmetic on its null
            // pointer has no meaning.
            let pos = self.enu_pos(w, self.winstk[w].rc.sortindx).unwrap_or(-1);
            if pos < self.winstk[w].begpflg {
                begpflgs = begpflgs.wrapping_sub(2);
            }
        }
        if 1 > totpflgs {
            totpflgs = 1;
        }
        if 1 > begpflgs {
            begpflgs = 1;
        }
        let mut tmp1 = snprintf(
            SMLBUFSIZ,
            nls::SCROLL_COORD_FMT.as_bytes(),
            &[
                Arg::I(i64::from(self.winstk[w].begtask.wrapping_add(1))),
                Arg::I(i64::from(self.pidsmaxt())),
                Arg::I(i64::from(begpflgs)),
                Arg::I(i64::from(totpflgs)),
            ],
        )
        .0;
        if self.winstk[w].varcolbeg != 0 {
            let tmp2 = snprintf(
                SMLBUFSIZ,
                b" + %d",
                &[Arg::I(i64::from(self.winstk[w].varcolbeg))],
            )
            .0;
            tmp1.extend_from_slice(&tmp2);
        }
        let g = self.tg2(0, self.msg_row);
        self.out.putt(
            b"%s%s  %.*s%s",
            &[
                Arg::S(&g),
                Arg::S(&self.caps.caps_off),
                Arg::I(i64::from(self.screen_cols.wrapping_sub(3))),
                Arg::S(&tmp1),
                Arg::S(&self.caps.clr_eol),
            ],
        );
    }

    /// `show_special (interact, glob)`: lines with `~1`..`~8` markers, each
    /// marker giving the text before it the attribute of that number, cut
    /// to the screen's width; interactive lines go straight out, others
    /// through the frame's row optimisation.
    pub fn show_special(&mut self, interact: bool, glob: &[u8]) {
        let mut glob = glob;
        let w = self.curwin;
        while let Some(nl) = glob.iter().position(|&b| b == b'\n') {
            let lessor = nl.min(LRGBUFSIZ - 3);
            let mut lin: Vec<u8> = glob.get(..lessor).unwrap_or_default().to_vec();
            lin.extend_from_slice(&[0, 0, 0]);
            let at = |lin: &[u8], i: usize| lin.get(i).copied().unwrap_or(0);
            let mut room = self.screen_cols;
            let mut sub_beg = 0usize;
            let mut sub_end = 0usize;
            let mut row: Vec<u8> = Vec::new();

            while at(&lin, sub_beg) != 0 {
                let mut ch = i32::from(at(&lin, sub_end));
                if ch == i32::from(b'~') {
                    ch = i32::from(at(&lin, sub_end.saturating_add(1)))
                        .wrapping_sub(i32::from(b'0'));
                }
                match ch {
                    0..=8 => {
                        if ch == 0 {
                            for k in [sub_end.saturating_add(1), sub_end.saturating_add(2)] {
                                if let Some(b) = lin.get_mut(k) {
                                    *b = 0;
                                }
                            }
                        }
                        if let Some(b) = lin.get_mut(sub_end) {
                            *b = 0;
                        }
                        let sub = c_str(lin.get(sub_beg..).unwrap_or_default()).to_vec();
                        let cap = self.winstk[w]
                            .captab(usize::try_from(ch).unwrap_or(0), &self.caps.norm)
                            .to_vec();
                        let tmp = snprintf(
                            LRGBUFSIZ,
                            b"%s%.*s%s",
                            &[
                                Arg::S(&cap),
                                Arg::I(i64::from(utf8::embody(&sub, room))),
                                Arg::S(&sub),
                                Arg::S(&self.caps.caps_off),
                            ],
                        )
                        .0;
                        row.extend_from_slice(&tmp);
                        room = room.wrapping_sub(
                            i32::try_from(sub_end.saturating_sub(sub_beg)).unwrap_or(0),
                        );
                        room = room.wrapping_add(utf8::delta(&sub));
                        sub_end = sub_end.saturating_add(2);
                        sub_beg = sub_end;
                    }
                    _ => sub_end = sub_end.saturating_add(1),
                }
                if 0 >= room {
                    break;
                }
            }

            if interact {
                self.out
                    .putt(b"%s%s\n", &[Arg::S(&row), Arg::S(&self.caps.clr_eol)]);
            } else {
                let rows = self.screen_rows;
                self.out.puff(
                    rows,
                    b"%s%s\n",
                    &[Arg::S(&row), Arg::S(&self.caps.caps_endline)],
                );
            }
            glob = glob.get(nl.saturating_add(1)..).unwrap_or_default();
        }
        if !glob.is_empty() {
            let n = utf8::embody(glob, self.screen_cols.wrapping_sub(1));
            self.out
                .putt(b"%.*s", &[Arg::I(i64::from(n)), Arg::S(glob)]);
        }
    }

    /// `ioa (ts)`: wait for a key, for `ms` milliseconds or (with `None`)
    /// for ever; whether one came. A signal ends the wait without one.
    pub fn ioa(&mut self, ms: Option<i32>) -> bool {
        let timeout = ms.unwrap_or(-1);
        let both = [0, self.wake_read];
        let fds = if self.wake_read >= 0 {
            &both[..]
        } else {
            &both[..1]
        };
        match libcall::fd::wait_readable_among(fds, timeout) {
            // The wake-up pipe: `pselect` interrupted.
            Ok(mask) if mask & 0b10 != 0 => {
                self.check_signals();
                false
            }
            Ok(mask) => mask & 1 != 0,
            Err(_) => {
                self.check_signals();
                false
            }
        }
    }

    /// `ioch (ech, buf, cnt)`: a read of whatever keys are waiting, once
    /// one is; end of file ends the program. Any further bytes of an escape
    /// sequence are flushed.
    fn ioch(&mut self, cnt: usize) -> i32 {
        let mut rc = -1i32;
        if self.ioa(None) {
            let mut tmp = vec![0u8; cnt];
            rc = match libcall::fd::read(0, &mut tmp) {
                Ok(n) => {
                    if let Some(dst) = self.iokey.buf.get_mut(..n) {
                        dst.copy_from_slice(tmp.get(..n).unwrap_or_default());
                    }
                    i32::try_from(n).unwrap_or(0)
                }
                Err(_) => -1,
            };
        }
        if rc == 0 {
            self.bye_bye(None);
        }
        // `tcflush`: keys a terminal cannot discard are only read a moment later.
        let _ = libcall::termios::flush_input(0);
        rc
    }

    /// `iokey (action)`: one key -- a character, one of the `KBD_*` codes for
    /// a sequence the terminal sends, 0 for none (a signal), or -1 for an
    /// escape sequence it does not know.
    pub fn iokey(&mut self, action: i32) -> i32 {
        if action == IOKEY_INIT {
            let verase = self.tty_original.c_cc[libcall::termios::VERASE];
            let erase = if verase == 0 {
                self.ti.str("kbs")
            } else {
                Some(vec![verase])
            };
            if let Some(slot) = self.iokey.tab.first_mut() {
                slot.0 = erase;
            }
            let names = [
                "kich1", "kdch1", "kcub1", "kcud1", "kcuu1", "kcuf1", "khome", "knp", "kpp",
                "kend", "cbt",
            ];
            for (k, name) in names.iter().enumerate() {
                let s = self.ti.str(name);
                if let Some(slot) = self.iokey.tab.get_mut(k.saturating_add(1)) {
                    slot.0 = s;
                }
            }
            if let Some(kx) = self.ti.keypad_xmit.clone() {
                self.out.putp(&kx);
            }
            return 0;
        }

        if action == IOKEY_ONCE {
            self.iokey.buf = [0; MEDBUFSIZ];
            if 1 > self.ioch(MEDBUFSIZ - 1) {
                return 0;
            }
        }

        if action == IOKEY_NEXT {
            if self.iokey.pos < self.iokey.len {
                let b = self.iokey.buf.get(self.iokey.pos).copied().unwrap_or(0);
                self.iokey.pos = self.iokey.pos.saturating_add(1);
                return schar(b);
            }
            self.iokey.pos = 0;
            self.iokey.len = 0;
            self.iokey.buf = [0; MEDBUFSIZ];
            if 1 > self.ioch(MEDBUFSIZ - 1) {
                return 0;
            }
            if !is_cntrl(schar(self.iokey.buf[0])) {
                self.iokey.len = strlen(&self.iokey.buf);
                self.iokey.pos = 1;
                return schar(self.iokey.buf[0]);
            }
        }

        // "some emulators implement 'key repeat' too well ... we'll focus on
        // the last escaped sequence, while also allowing use of the meta key"
        let buf = c_str(&self.iokey.buf).to_vec();
        let pb = match buf.iter().rposition(|&b| b == 0x1b) {
            None => 0,
            Some(p) if p > 0 && buf.get(p.saturating_sub(1)) == Some(&0x1b) => p.saturating_sub(1),
            Some(p) => p,
        };
        let tail = buf.get(pb..).unwrap_or_default();
        for (s, key) in &self.iokey.tab {
            if s.as_deref() == Some(tail) {
                return *key;
            }
        }
        if self.iokey.buf[0] == 0x1b && self.iokey.buf[1] != 0 {
            return -1;
        }
        schar(self.iokey.buf[0])
    }

    /// `ioline (prompt)`: a line typed on the message row, with editing,
    /// overtype (Insert), and the last fifty lines recalled by Up and Down.
    /// What comes back starts with `ESC` when the user escaped, and is empty
    /// when a signal came first.
    pub fn ioline(&mut self, prompt: &[u8]) -> Vec<u8> {
        const SAVMAX: usize = 50;
        // `bufMAX`: the buffer less the terminator and one more.
        const BUFMAX: i32 = (MEDBUFSIZ + 1) as i32 - 2;
        let mut plin = 0usize;
        let mut cur: i32 = 0;
        let mut len: i32;
        let mut pos: i32 = 0;
        let beg = self.show_pmt(prompt);
        self.ioline.buf = [0; MEDBUFSIZ + 1];
        let c = if self.ioline.ovt {
            self.caps.curs_huge.clone()
        } else {
            self.caps.curs_norm.clone()
        };
        self.out.putp(&c);

        loop {
            self.out.flush();
            let key = self.iokey(IOKEY_NEXT);
            let mut redraw = true;
            match key {
                0 => {
                    self.ioline.buf[0] = 0;
                    return c_str(&self.ioline.buf).to_vec();
                }
                KBD_ESC => {
                    self.ioline.buf[0] = 0x1b;
                    return c_str(&self.ioline.buf).to_vec();
                }
                KBD_ENTER | KBD_BTAB | KBD_PGUP | KBD_PGDN => redraw = false,
                KBD_INS => {
                    self.ioline.ovt = !self.ioline.ovt;
                    let c = if self.ioline.ovt {
                        self.caps.curs_huge.clone()
                    } else {
                        self.caps.curs_norm.clone()
                    };
                    self.out.putp(&c);
                }
                KBD_DEL => self.sqz_str(&mut pos),
                KBD_BKSP => {
                    if 0 < cur {
                        self.mov_bkw(cur, &mut pos);
                        cur = cur.wrapping_sub(self.utf_col(pos));
                        pos = utf8::embody(&self.ioline.buf_str(), cur);
                        self.sqz_str(&mut pos);
                    }
                }
                KBD_LEFT => {
                    if 0 < cur {
                        self.mov_bkw(cur, &mut pos);
                        cur = cur.wrapping_sub(self.utf_col(pos));
                    }
                }
                KBD_RIGHT => {
                    let l = self.ioline.logical_len();
                    if cur < l {
                        cur = cur.wrapping_add(self.utf_col(pos));
                    }
                }
                KBD_HOME => {
                    cur = 0;
                    pos = 0;
                }
                KBD_END => {
                    cur = self.ioline.logical_len();
                    pos = i32::try_from(strlen(&self.ioline.buf)).unwrap_or(0);
                }
                KBD_UP => {
                    if plin.saturating_add(1) < self.ioline.saved.len() {
                        plin = plin.saturating_add(1);
                        self.ioline.load(plin, BUFMAX);
                        cur = self.ioline.logical_len();
                        pos = i32::try_from(strlen(&self.ioline.buf)).unwrap_or(0);
                    }
                }
                KBD_DOWN => {
                    plin = plin.saturating_sub(1);

                    self.ioline.load(plin, BUFMAX);
                    cur = self.ioline.logical_len();
                    pos = i32::try_from(strlen(&self.ioline.buf)).unwrap_or(0);
                }
                _ => {
                    len = self.ioline.logical_len();
                    let blen = i32::try_from(strlen(&self.ioline.buf)).unwrap_or(0);
                    if pos.wrapping_add(4) < BUFMAX
                        && beg.wrapping_add(len).wrapping_add(2) < self.screen_cols
                        && blen < BUFMAX
                    {
                        // `UTF8_tab[(unsigned char)key]`.
                        let mut tot = utf8::tab(key.to_le_bytes()[0]);
                        let sav = pos;
                        if tot < 1 {
                            tot = 1;
                        }
                        if self.ioline.ovt {
                            pos = utf8::embody(&self.ioline.buf_str(), cur);
                            self.sqz_str(&mut pos);
                        }
                        self.exp_str(pos, tot, beg);
                        self.ioline.put(pos, key);
                        pos = pos.wrapping_add(1);
                        while tot > 1 {
                            let k = self.iokey(IOKEY_NEXT);
                            self.ioline.put(pos, k);
                            pos = pos.wrapping_add(1);
                            tot = tot.wrapping_sub(1);
                        }
                        cur = cur.wrapping_add(self.utf_col(sav));
                    }
                }
            }
            if redraw {
                len = self.ioline.logical_len();
                if cur < 0 {
                    cur = 0;
                }
                if cur > len {
                    cur = len;
                }
                pos = utf8::embody(&self.ioline.buf_str(), cur);
                let g = self.tg2(beg, self.msg_row);
                let line = self.ioline.buf_str();
                let s = Self::fmtmk(
                    "%s%s%s",
                    &[Arg::S(&g), Arg::S(&self.caps.clr_eol), Arg::S(&line)],
                );
                self.out.putp(&s);
                let g = self.tg2(beg.wrapping_add(cur), self.msg_row);
                self.out.putp(&g);
            }
            if key == KBD_ENTER {
                break;
            }
        }

        // "weed out duplicates, including empty strings (top-of-stack)..."
        let buf = self.ioline.buf_str();
        if let Some(i) = self.ioline.saved.iter().position(|s| *s == buf) {
            if i > 1 {
                let s = self.ioline.saved.remove(i);
                self.ioline.saved.insert(1, s);
            }
            return buf;
        }
        if self.ioline.saved.len() > SAVMAX {
            self.ioline.saved.pop();
        }
        self.ioline.saved.insert(1, buf.clone());
        buf
    }

    /// `utfCOL (x)`: the columns of the character at byte `x` of the line.
    fn utf_col(&self, x: i32) -> i32 {
        let at = usize::try_from(x).unwrap_or(0);
        let rest = self.ioline.buf.get(at..).unwrap_or_default();
        let tot = utf8::tab(rest.first().copied().unwrap_or(0));
        utf8::cols(rest, tot)
    }

    /// `movBKW`: back to the start of the character before column `cur`.
    fn mov_bkw(&self, cur: i32, pos: &mut i32) {
        *pos = utf8::embody(&self.ioline.buf_str(), cur.wrapping_sub(1));
        while *pos > 0 && self.ioline.tot(*pos) < 0 {
            *pos = pos.wrapping_sub(1);
        }
    }

    /// `sqzSTR`: the character at `pos` taken out (with a combining one
    /// after it).
    fn sqz_str(&mut self, pos: &mut i32) {
        const BUFMAX: usize = MEDBUFSIZ + 1 - 2;
        let mut i = self.ioline.tot(*pos);
        while i < 0 && *pos > 0 {
            *pos = pos.wrapping_sub(1);
            i = self.ioline.tot(*pos);
        }
        let i = i.max(0);
        let mut i = usize::try_from(i).unwrap_or(0);
        let p = usize::try_from(*pos).unwrap_or(0);
        let next = i32::try_from(p.saturating_add(i)).unwrap_or(0);
        if self.utf_col(next) == 0 {
            let more = usize::try_from(self.ioline.tot(next).max(0)).unwrap_or(0);
            i = i.saturating_add(more);
        }
        let buf = &mut self.ioline.buf;
        let from = p.saturating_add(i).min(BUFMAX);
        let n = BUFMAX.saturating_sub(p.saturating_add(i));
        if p.saturating_add(n) <= buf.len() && from.saturating_add(n) <= buf.len() {
            buf.copy_within(from..from.saturating_add(n), p);
        }
        let zero_from = BUFMAX.saturating_sub(i);
        for b in buf.iter_mut().take(BUFMAX).skip(zero_from) {
            *b = 0;
        }
    }

    /// `expSTR (x)`: room for `x` bytes made at `pos`, if the line and the
    /// screen have it.
    fn exp_str(&mut self, pos: i32, x: i32, beg: i32) {
        const BUFMAX: i32 = (MEDBUFSIZ + 1) as i32 - 2;
        let len = self.ioline.logical_len();
        if pos.wrapping_add(4) < BUFMAX && beg.wrapping_add(len).wrapping_add(2) < self.screen_cols
        {
            let p = usize::try_from(pos).unwrap_or(0);
            let x = usize::try_from(x).unwrap_or(0);
            let n = usize::try_from(BUFMAX.wrapping_sub(pos)).unwrap_or(0);
            let buf = &mut self.ioline.buf;
            // Upstream's move can reach two bytes past its buffer; this one
            // stops at the end.
            let n = n.min(buf.len().saturating_sub(p.saturating_add(x)));
            buf.copy_within(p..p.saturating_add(n), p.saturating_add(x));
        }
    }

    /// `get_float (prompt)`: a non-negative number typed, or one of the
    /// `GET_NUM_*` codes as a `float`.
    // The codes are `int`s converted to `float`, as upstream returns them.
    #[allow(clippy::cast_precision_loss)]
    pub fn get_float(&mut self, prompt: &[u8]) -> f32 {
        let line = self.ioline(prompt);
        if line.first() == Some(&0x1b) || crate::frames() != 0 {
            return GET_NUM_ESC as f32;
        }
        if line.is_empty() {
            return GET_NUM_NOT as f32;
        }
        match crate::fmt::mkfloat(&line, false, self.radix) {
            Some(f) if f >= 0.0 => f,
            _ => {
                self.show_msg(nls::BAD_NUMFLOAT_TXT.as_bytes());
                GET_NUM_BAD as f32
            }
        }
    }

    /// `get_int (prompt)`: a whole number typed, or a `GET_NUM_*` code.
    // `(int) f` of a float `mkfloat` kept below `INT_MAX`.
    #[allow(clippy::cast_possible_truncation)]
    pub fn get_int(&mut self, prompt: &[u8]) -> i32 {
        let line = self.ioline(prompt);
        if line.first() == Some(&0x1b) || crate::frames() != 0 {
            return GET_NUM_ESC;
        }
        if line.is_empty() {
            return GET_NUM_NOT;
        }
        match crate::fmt::mkfloat(&line, true, self.radix) {
            Some(f) => f as i32,
            None => {
                self.show_msg(nls::BAD_INTEGERS_TXT.as_bytes());
                GET_NUM_BAD
            }
        }
    }

    /// `whack_terminal`: the terminal looked up, and -- unless in batch
    /// mode -- put in the nearly raw mode top reads single keys in.
    pub fn whack_terminal(&mut self) {
        let tenv = terminfo::Env::from_process();
        let term = std::env::var_os("TERM").map(std::ffi::OsString::into_encoded_bytes);
        let setup = if self.batch {
            terminfo::setupterm(Some(b"dumb"), term.as_deref(), &tenv)
        } else {
            terminfo::setupterm(None, term.as_deref(), &tenv)
        };
        if let Some(complaint) = &setup.complaint {
            // `setupterm` with no `errret`: say why and exit -- onto a
            // standard error that is `/dev/null` by now.
            let _ = libcall::fd::write(2, complaint);
            self.exit_failure_quietly();
        }
        self.out.padding = setup.padding();
        self.out.batch = self.batch;
        if let Some(entry) = setup.entry.as_ref() {
            self.ti.set(terminfo::legacy_copy(entry, &tenv));
        }
        if self.batch {
            return;
        }
        let Ok(orig) = libcall::termios::get_attr(0) else {
            self.error_exit(nls::FAIL_TTY_GET_TXT.as_bytes());
        };
        self.tty_original = orig;
        self.ttychanged = true;

        use libcall::termios as t;
        let mut tmptty = orig;
        tmptty.c_lflag |= t::ECHO | t::ECHOCTL | t::ECHOE | t::ICANON | t::ISIG;
        tmptty.c_lflag &= !t::NOFLSH;
        tmptty.c_oflag &= !t::TAB3;
        tmptty.c_iflag |= t::BRKINT;
        tmptty.c_iflag &= !t::IGNBRK;
        tmptty.c_lflag &= !(t::ECHO | t::ECHOCTL | t::ECHOE | t::ICANON);
        tmptty.c_cc[t::VMIN] = 1;
        tmptty.c_cc[t::VTIME] = 0;
        if let Err(e) = t::set_attr(0, t::TCSAFLUSH, &tmptty) {
            let msg = Self::fmtmk(nls::FAIL_TTY_SET_FMT, &[Arg::S(&crate::strerror_no(e))]);
            self.error_exit(&msg);
        }
        self.tty_raw = t::get_attr(0).unwrap_or(tmptty);
        // `setbuffer (stdout, ...)`: whole frames, not lines.
        if let Some(st) = self.out.stream.as_mut() {
            let _ = st.flush_now();
        }
        self.out.stream = Some(Stream::on(1));
        self.iokey(IOKEY_INIT);
    }

    /// `exit (EXIT_FAILURE)` with nothing more said.
    fn exit_failure_quietly(&mut self) -> ! {
        let status = match self.out.stream.take() {
            Some(stream) => coreutils::stdfd::close_stdout_procps(
                &self.myname,
                stream,
                std::process::ExitCode::FAILURE,
            ),
            None => std::process::ExitCode::FAILURE,
        };
        std::process::exit(i32::from(status != std::process::ExitCode::SUCCESS));
    }

    /// `at_eoj`: the terminal as top found it, the cursor below everything,
    /// standard output flushed and standard error back.
    pub fn at_eoj(&mut self) {
        if self.ttychanged {
            let _ = libcall::termios::set_attr(0, libcall::termios::TCSAFLUSH, &self.tty_original);
            if let Some(kl) = self.ti.keypad_local.clone() {
                self.out.putp(&kl);
            }
            let g = self.tg2(0, self.screen_rows);
            self.out.putp(&g);
            self.out.putp(b"\n");
            let c = self.caps.curs_norm.clone();
            self.out.putp(&c);
            let c = self.caps.clr_eol.clone();
            self.out.putp(&c);
            let c = self.caps.smam.clone();
            self.out.putp(&c);
        }
        self.out.flush();
        restore_stderr(self.out.stderr_save);
        self.out.stderr_save = -1;
    }

    /// `ENUpos (w, e)`: where field `e` is among window `w`'s active
    /// fields, if it is.
    #[must_use]
    pub fn enu_pos(&self, w: usize, e: i32) -> Option<i32> {
        let win = &self.winstk[w];
        let tot = usize::try_from(win.totpflgs).unwrap_or(0);
        win.pflgsall
            .iter()
            .take(tot)
            .position(|&f| f == e)
            .and_then(|p| i32::try_from(p).ok())
    }

    /// `ENUviz (w, e)`: whether field `e` is among the columns shown.
    #[must_use]
    pub fn enu_viz(&self, w: usize, e: i32) -> bool {
        let win = &self.winstk[w];
        let n = usize::try_from(win.maxpflgs).unwrap_or(0);
        win.procflgs.iter().take(n).any(|&f| f == e)
    }
}

impl IoLine {
    /// The line typed so far.
    #[must_use]
    pub fn buf_str(&self) -> Vec<u8> {
        c_str(&self.buf).to_vec()
    }

    /// `setLEN`: the line's length in columns.
    #[must_use]
    pub fn logical_len(&self) -> i32 {
        let s = c_str(&self.buf);
        i32::try_from(s.len())
            .unwrap_or(0)
            .wrapping_sub(utf8::delta(s))
    }

    /// `utfTOT (x)`.
    #[must_use]
    pub fn tot(&self, x: i32) -> i32 {
        let at = usize::try_from(x).unwrap_or(0);
        utf8::tab(self.buf.get(at).copied().unwrap_or(0))
    }

    /// `buf[pos] = key`, as C stores an `int` in a `char`.
    pub fn put(&mut self, pos: i32, key: i32) {
        if let Some(b) = usize::try_from(pos).ok().and_then(|p| self.buf.get_mut(p)) {
            *b = key.to_le_bytes()[0];
        }
    }

    /// A recalled line into the buffer: `memccpy (buf, plin->str, '\0',
    /// bufMAX)`.
    pub fn load(&mut self, at: usize, bufmax: i32) {
        self.buf = [0; MEDBUFSIZ + 1];
        let s = self.saved.get(at).cloned().unwrap_or_default();
        let n = s.len().min(usize::try_from(bufmax).unwrap_or(0));
        if let (Some(dst), Some(src)) = (self.buf.get_mut(..n), s.get(..n)) {
            dst.copy_from_slice(src);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn strlcpy_keeps_one_byte_for_the_terminator() {
        assert_eq!(strlcpy(b"abcdef", 4), b"abc");
        assert_eq!(strlcpy(b"ab", 4), b"ab");
    }

    #[test]
    fn a_byte_widens_as_a_signed_char() {
        assert_eq!(schar(b'a'), 97);
        assert_eq!(schar(0xff), -1);
        assert_eq!(schar(0x80), -128);
    }

    #[test]
    fn puff_skips_a_row_the_screen_already_has() {
        let mut out = Out::new(-1);
        out.stream = None;
        out.pseudo = vec![Vec::new(); 3];
        out.pseudo_rows = 3;
        out.pseudo_row = 0;
        out.puff(3, b"%s", &[Arg::S(b"row")]);
        assert_eq!(out.pseudo[0], b"row");
        out.pseudo_row = 0;
        out.puff(3, b"%s", &[Arg::S(b"row")]);
        assert_eq!(out.pseudo_row, 1);
        out.psu_clreos(0);
        assert!(out.pseudo[0].is_empty());
    }
}
