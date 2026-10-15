//! "Inspect Other Output": the `Y` screens -- a file or a command's output
//! for one task, chosen from the rcfile's inspect entries (or three
//! demonstrations), scrolled and searched like a pager, its unprintable
//! bytes shown `^C`- and `<AB>`-style.

use crate::cfmt::{Arg, snprintf};
use crate::defs::{
    BIGBUFSIZ, BREAK_SCREEN, EU_CMD, EU_PID, FNDBUFSIZ, KBD_DOWN, KBD_END, KBD_ENTER, KBD_ESC,
    KBD_HOME, KBD_LEFT, KBD_PGDN, KBD_PGUP, KBD_RIGHT, KBD_SPACE, KBD_UP, LRGBUFSIZ, READMINSZ,
    SCREENMAX, SMLBUFSIZ,
};
use crate::term::{IOKEY_ONCE, c_str};
use crate::{InspView, Top, nls, utf8};

/// What an inspect entry does: `insp_do_file`, `insp_do_pipe`, or
/// `insp_do_demo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Func {
    Demo,
    File,
    Pipe,
}

/// `INT_MAX`: "no match in this row".
const NO_MATCH: i32 = i32::MAX;

/// What `readfile` leaves for a stream that never opened: an empty buffer
/// of `READMINSZ` bytes, and why.
fn read_nothing(why: std::io::Error) -> (Vec<u8>, usize, Option<std::io::Error>) {
    (vec![0; READMINSZ], 0, Some(why))
}

/// A child's output as a descriptor.
#[cfg(unix)]
fn raw_fd(out: &std::process::ChildStdout) -> i32 {
    std::os::fd::AsRawFd::as_raw_fd(out)
}

/// A child's output as a descriptor: none, where there are no descriptors
/// to read (and so nothing [`Top::read_all`] can read).
#[cfg(not(unix))]
fn raw_fd(_out: &std::process::ChildStdout) -> i32 {
    -1
}

impl InspView {
    /// `INSP_RLEN (idx)`: a row's length.
    fn rlen(&self, idx: i32) -> i32 {
        let i = usize::try_from(idx).unwrap_or(0);
        let a = self.p.get(i).copied().unwrap_or(0);
        let b = self.p.get(i.saturating_add(1)).copied().unwrap_or(0);
        i32::try_from(b)
            .unwrap_or(0)
            .wrapping_sub(i32::try_from(a).unwrap_or(0))
            .wrapping_sub(1)
    }

    /// `Insp_p[row]`: where a row begins.
    fn row_at(&self, row: i32) -> usize {
        usize::try_from(row)
            .ok()
            .and_then(|r| self.p.get(r))
            .copied()
            .unwrap_or(0)
    }

    /// The byte at `i` of the buffer, NUL past it.
    fn at(&self, i: usize) -> u8 {
        self.buf.get(i).copied().unwrap_or(0)
    }

    /// `insp_cnt_nl`: where each line begins.
    fn cnt_nl(&mut self) {
        let end = self.bufrd.saturating_add(1);
        let mut cur = 0usize;
        let mut p = vec![0usize];
        let mut nl = 0usize;
        for beg in 0..end {
            if self.at(beg) == b'\n' {
                if let Some(slot) = p.get_mut(nl) {
                    *slot = cur;
                } else {
                    p.push(cur);
                }
                nl = nl.saturating_add(1);
                cur = beg.saturating_add(1);
            }
        }
        p.resize(nl.saturating_add(2), 0);
        if let Some(first) = p.first_mut() {
            *first = 0;
        }
        if let Some(slot) = p.get_mut(nl) {
            *slot = cur;
        }
        nl = nl.saturating_add(1);
        if let Some(slot) = p.get_mut(nl) {
            *slot = end;
        }
        if end.saturating_sub(cur) == 1 {
            nl = nl.saturating_sub(1);
        }
        self.p = p;
        self.nl = i32::try_from(nl).unwrap_or(i32::MAX);
    }
}

impl Top {
    /// `INSP_MKSL (big, txt)`: the status line, on row 3.
    fn insp_mksl(&mut self, big: bool, txt: &[u8]) {
        let sz = (if big { self.screen_cols } else { 80 }).wrapping_add(utf8::delta(txt));
        let p = Self::fmtmk(
            "%-*.*s",
            &[Arg::I(i64::from(sz)), Arg::I(i64::from(sz)), Arg::S(txt)],
        );
        self.msg_row = 3;
        let g = self.tg2(0, 3);
        let n = utf8::embody(&p, self.screen_cols);
        let hdr = self.cw().capclr_hdr.clone();
        self.out.putt(
            b"%s%s%.*s%s",
            &[
                Arg::S(&g),
                Arg::S(&hdr),
                Arg::I(i64::from(n)),
                Arg::S(&p),
                Arg::S(&self.caps.clr_eol.clone()),
            ],
        );
        let c = self.caps.caps_off.clone();
        self.out.putp(&c);
        self.out.flush();
    }

    /// `insp_do_demo`, `insp_do_file`, `insp_do_pipe`: the chosen entry's
    /// output read.
    fn insp_run(&mut self, func: Func, fmts: &[u8], pid: i32) {
        let failed = |e: &std::io::Error| {
            Self::fmtmk(nls::YINSP_FAILED_FMT, &[Arg::S(&crate::strerror_of(e))])
        };
        match func {
            Func::Demo => {
                let story = nls::YINSP_DSTORY_TXT.as_bytes();
                let mut buf = story.to_vec();
                buf.resize(READMINSZ.saturating_add(story.len()), 0);
                self.insp.buf = buf;
                self.insp.bufrd = story.len();
            }
            Func::File => {
                let path = snprintf(LRGBUFSIZ, fmts, &[Arg::I(i64::from(pid))]).0;
                // `fopen (buf, "r")`, which may wait -- on a FIFO with no
                // writer -- and a signal while it waits is acted on there.
                let opened = match std::ffi::CString::new(path) {
                    Err(_) => Err(libcall::ENOENT),
                    Ok(c) => loop {
                        match libcall::fd::open_read(&c) {
                            Err(libcall::EINTR) => self.check_signals(),
                            other => break other,
                        }
                    },
                };
                let (buf, n, err) = match opened {
                    Ok(fd) => {
                        let got = self.read_all(fd);
                        // `fclose` of a stream only read: nothing it could
                        // report changes what was read.
                        let _ = libcall::fd::close(fd);
                        got
                    }
                    Err(e) => read_nothing(std::io::Error::from_raw_os_error(e)),
                };
                self.insp.buf = buf;
                self.insp.bufrd = n;
                if let Some(e) = err {
                    let msg = failed(&e);
                    self.insp_set_text(&msg);
                }
            }
            Func::Pipe => {
                // `sigaction (SIGINT, SIG_IGN)`. What it was is not wanted
                // back -- `sig_endpgm` goes there afterwards, as upstream
                // puts it, whatever it had been -- and a failure leaves an
                // interrupt ending the program, which is what it did anyway.
                let _ = libcall::signal::ignore(libcall::signal::SIGINT);
                let cmd = snprintf(LRGBUFSIZ, fmts, &[Arg::I(i64::from(pid))]).0;
                let child = shellcmd::shell_bytes(&cmd)
                    .stdout(std::process::Stdio::piped())
                    .spawn();
                let (buf, n, err) = match child {
                    Ok(mut c) => {
                        let got = match c.stdout.take() {
                            Some(out) => self.read_all(raw_fd(&out)),
                            // Never: the output was asked for as a pipe.
                            None => read_nothing(std::io::Error::from_raw_os_error(
                                libcall::EINVAL,
                            )),
                        };
                        // `pclose`: the command waited for, its status unread
                        // -- upstream looks at nothing but the output. A signal
                        // while it is waited for is acted on there.
                        let pid = i32::try_from(c.id()).unwrap_or(-1);
                        while libcall::process::wait(pid) == Err(libcall::EINTR) {
                            self.check_signals();
                        }
                        got
                    }
                    Err(e) => read_nothing(e),
                };
                self.insp.buf = buf;
                self.insp.bufrd = n;
                if let Some(e) = err {
                    let msg = failed(&e);
                    self.insp_set_text(&msg);
                }
                // `sa.sa_handler = sig_endpgm`. Should it fail, an interrupt
                // is ignored until the program ends -- nothing else could be
                // done about it here either.
                let _ = libcall::signal::set_handler(
                    libcall::signal::SIGINT,
                    crate::sig_endpgm_handler(),
                    false,
                );
            }
        }
        self.insp.cnt_nl();
    }

    /// `readfile (fp, &buf, &bufsz, &bufrd)`: everything on `fd`, in a
    /// buffer of `READMINSZ` more bytes than it holds, NUL after it -- and
    /// the error, if the read failed.
    ///
    /// A command's output can keep the program waiting here for as long as
    /// the command likes, and upstream's handlers act inside that `fread`: a
    /// signal that interrupts the read is acted on before it goes on
    /// (deliberate difference 1 keeps the handlers to flags, and this is one
    /// of the places the program waits).
    fn read_all(&mut self, fd: i32) -> (Vec<u8>, usize, Option<std::io::Error>) {
        let mut data = Vec::new();
        let mut chunk = vec![0u8; READMINSZ];
        let err = loop {
            match libcall::fd::read(fd, &mut chunk) {
                Ok(0) => break None,
                Ok(n) => data.extend_from_slice(chunk.get(..n).unwrap_or_default()),
                Err(libcall::EINTR) => self.check_signals(),
                Err(e) => break Some(std::io::Error::from_raw_os_error(e)),
            }
        };
        let n = data.len();
        data.resize(n.saturating_add(READMINSZ), 0);
        (data, n, err)
    }

    /// `Insp_bufrd = snprintf (Insp_buf, Insp_bufsz, "%s", msg)`.
    fn insp_set_text(&mut self, msg: &[u8]) {
        let size = self.insp.buf.len();
        let (text, full) = snprintf(size, b"%s", &[Arg::S(msg)]);
        for (k, b) in self.insp.buf.iter_mut().enumerate() {
            *b = text.get(k).copied().unwrap_or(0);
        }
        self.insp.bufrd = full;
    }

    /// The selected entry's search string.
    fn insp_fstr(&self) -> Vec<u8> {
        self.inspect
            .tab
            .get(self.insp.sel)
            .map(|e| e.fstr.clone())
            .unwrap_or_default()
    }

    /// `insp_find_ofs (col, row)`: where the search string is in `row`, from
    /// column `col` -- each NUL-separated piece of the row searched on its
    /// own -- or `INT_MAX`.
    fn insp_find_ofs(&self, col: i32, row: i32) -> i32 {
        let fstr = self.insp_fstr();
        let Some(&first) = fstr.first() else {
            return NO_MATCH;
        };
        let v = &self.insp;
        let start = v.row_at(row);
        let rlen = v.rlen(row);
        let row_bytes = v
            .buf
            .get(start..start.saturating_add(usize::try_from(rlen.max(0)).unwrap_or(0)))
            .unwrap_or_default();
        if !row_bytes.contains(&first) {
            return NO_MATCH;
        }
        let next = v.row_at(row.wrapping_add(1));
        let mut col = col;
        while col < rlen {
            let at = start.saturating_add(usize::try_from(col).unwrap_or(0));
            if v.at(at) == 0 {
                col = col.wrapping_add(1);
                continue;
            }
            let piece = c_str(v.buf.get(at..).unwrap_or_default());
            if let Some(off) = crate::wins::strstr_at(piece, &fstr) {
                let fnd = at.saturating_add(off);
                if fnd < next {
                    return i32::try_from(fnd.saturating_sub(start)).unwrap_or(NO_MATCH);
                }
                return NO_MATCH;
            }
            col = col
                .wrapping_add(i32::try_from(piece.len()).unwrap_or(0))
                .wrapping_add(1);
        }
        NO_MATCH
    }

    /// `insp_find_str (ch, col, row)`: `L` (or `/`) asks for a string and
    /// finds it; `&` (or `n`) finds the next.
    fn insp_find_str(&mut self, ch: i32, col: &mut i32, row: &mut i32) {
        let sel = self.insp.sel;
        if (ch == i32::from(b'&') || ch == i32::from(b'n')) && self.insp_fstr().is_empty() {
            self.show_msg(nls::FIND_NO_NEXT_TXT.as_bytes());
            return;
        }
        if ch == i32::from(b'L') || ch == i32::from(b'/') {
            let s = self.ioline(nls::GET_FIND_STR_TXT.as_bytes());
            if s.first() == Some(&0x1b) {
                return;
            }
            let f = snprintf(FNDBUFSIZ, b"%s", &[Arg::S(&s)]).0;
            if let Some(e) = self.inspect.tab.get_mut(sel) {
                e.flen = i32::try_from(f.len()).unwrap_or(0);
                e.fstr = f;
            }
            self.st.insp_found = false;
        }
        if !self.insp_fstr().is_empty() {
            self.insp_mksl(false, nls::YINSP_WAITIN_TXT.as_bytes());
            let mut xx = *col;
            let mut yy = *row;
            while yy < self.insp.nl {
                xx = self.insp_find_ofs(xx, yy);
                if xx < self.insp.rlen(yy) {
                    self.st.insp_found = true;
                    if xx == *col && yy == *row {
                        xx = xx.wrapping_add(1);
                        continue;
                    }
                    *col = xx;
                    *row = yy;
                    return;
                }
                xx = 0;
                yy = yy.wrapping_add(1);
            }
            let redux = if self.st.insp_found {
                nls::WORD_ANOTHER_TXT.as_bytes()
            } else {
                b""
            };
            let fstr = self.insp_fstr();
            let msg = Self::fmtmk(nls::FIND_NO_FIND_FMT, &[Arg::S(redux), Arg::S(&fstr)]);
            self.show_msg(&msg);
        }
    }

    /// `insp_mkrow_raw (col, row)`: one row from byte `col`, controls as
    /// `^X`, other unprintables as `<AB>`, matches highlighted.
    fn insp_mkrow_raw(&mut self, col: i32, row: i32) {
        let start = self
            .insp
            .row_at(row)
            .saturating_add(usize::try_from(col).unwrap_or(0));
        let tline: Vec<u8> = if col < self.insp.rlen(row) {
            (0..SCREENMAX)
                .map(|k| self.insp.at(start.saturating_add(k)))
                .collect()
        } else {
            vec![b'\n']
        };
        let cols = self.screen_cols;
        let fstr = self.insp_fstr();
        let flen = i32::try_from(fstr.len()).unwrap_or(0);
        let msgcap = self.cw().capclr_msg.clone();
        let hdr = self.cw().capclr_hdr.clone();
        let off = self.caps.caps_off.clone();
        let mut hicap = false;
        let (mut fr, mut to, mut ofs) = (0i32, 0i32, 0i32);
        while to < cols {
            if ofs == 0 {
                ofs = self.insp_find_ofs(col.wrapping_add(fr), row);
            }
            if col.wrapping_add(fr) < ofs {
                let uch = tline
                    .get(usize::try_from(fr).unwrap_or(0))
                    .copied()
                    .unwrap_or(0);
                if uch == b'\n' {
                    break;
                }
                if uch > 126 || uch < 32 {
                    let p = if uch > 126 {
                        Self::fmtmk("<%02X>", &[Arg::U(u64::from(uch))])
                    } else {
                        Self::fmtmk("^%c", &[Arg::C(uch.wrapping_add(b'@'))])
                    };
                    let lead: &[u8] = if hicap { b"" } else { &msgcap };
                    self.out.putt(
                        b"%s%.*s",
                        &[
                            Arg::S(lead),
                            Arg::I(i64::from(cols.wrapping_sub(to))),
                            Arg::S(&p),
                        ],
                    );
                    to = to.wrapping_add(if uch > 126 { 4 } else { 2 });
                    hicap = true;
                } else {
                    if hicap {
                        self.out.putp(&off);
                        hicap = false;
                    }
                    to = to.wrapping_add(1);
                    if to <= cols {
                        self.out.putp(&[uch]);
                    }
                }
            } else {
                self.out.putt(
                    b"%s%.*s%s",
                    &[
                        Arg::S(&hdr),
                        Arg::I(i64::from(cols.wrapping_sub(to))),
                        Arg::S(&fstr),
                        Arg::S(&off),
                    ],
                );
                fr = fr.wrapping_add(flen).wrapping_sub(1);
                to = to.wrapping_add(flen);
                hicap = false;
                ofs = 0;
            }
            if col.wrapping_add(fr) >= self.insp.rlen(row) {
                break;
            }
            fr = fr.wrapping_add(1);
        }
        if hicap {
            self.out.putp(&off);
        }
        let c = self.caps.clr_eol.clone();
        self.out.putp(&c);
    }

    /// `insp_mkrow_utf8 (col, row)`: one row from column `col` of text the
    /// locale can show, controls as `^X`, matches highlighted.
    fn insp_mkrow_utf8(&mut self, col: i32, row: i32) {
        let rbeg = self.insp.row_at(row);
        let rowtxt = c_str(self.insp.buf.get(rbeg..).unwrap_or_default()).to_vec();
        let col = utf8::proper_col(&rowtxt, col, true);
        let start = rbeg.saturating_add(usize::try_from(col).unwrap_or(0));
        let tline: Vec<u8> = if col < self.insp.rlen(row) {
            (0..BIGBUFSIZ)
                .map(|k| self.insp.at(start.saturating_add(k)))
                .collect()
        } else {
            vec![b'\n']
        };
        let cols = self.screen_cols;
        let fstr = self.insp_fstr();
        let flen = i32::try_from(fstr.len()).unwrap_or(0);
        let msgcap = self.cw().capclr_msg.clone();
        let hdr = self.cw().capclr_hdr.clone();
        let off = self.caps.caps_off.clone();
        let t = |fr: i32| {
            tline
                .get(usize::try_from(fr).unwrap_or(0))
                .copied()
                .unwrap_or(0)
        };
        let (mut fr, mut to, mut ofs) = (0i32, 0i32, 0i32);
        while to < cols {
            if ofs == 0 {
                ofs = self.insp_find_ofs(col.wrapping_add(fr), row);
            }
            if col.wrapping_add(fr) < ofs {
                let uch = t(fr);
                let bno = utf8::tab(uch);
                let rest = tline
                    .get(usize::try_from(fr).unwrap_or(0)..)
                    .unwrap_or_default();
                let cno = utf8::cols(rest, bno);
                fr = fr.wrapping_add(1);
                let put = |top: &mut Self, bytes: &[u8], to: &mut i32| {
                    *to = to.wrapping_add(cno);
                    if *to <= cols {
                        top.out.putp(bytes);
                    }
                };
                match bno {
                    1 => {
                        if uch == b'\n' {
                        } else if uch < 32 {
                            let p = Self::fmtmk("^%c", &[Arg::C(uch.wrapping_add(b'@'))]);
                            self.out.putt(
                                b"%s%.*s%s",
                                &[
                                    Arg::S(&msgcap),
                                    Arg::I(i64::from(cols.wrapping_sub(to))),
                                    Arg::S(&p),
                                    Arg::S(&off),
                                ],
                            );
                            to = to.wrapping_add(2);
                        } else if uch == 127 {
                            put(self, b" ", &mut to);
                        } else {
                            put(self, &[uch], &mut to);
                        }
                    }
                    2..=4 => {
                        let mut bytes = vec![uch];
                        for _ in 1..bno {
                            bytes.push(t(fr));
                            fr = fr.wrapping_add(1);
                        }
                        put(self, &bytes, &mut to);
                    }
                    _ => put(self, b" ", &mut to),
                }
            } else {
                self.out.putt(
                    b"%s%.*s%s",
                    &[
                        Arg::S(&hdr),
                        Arg::I(i64::from(cols.wrapping_sub(to))),
                        Arg::S(&fstr),
                        Arg::S(&off),
                    ],
                );
                fr = fr.wrapping_add(flen);
                to = to.wrapping_add(flen);
                ofs = 0;
            }
            if col.wrapping_add(fr) >= self.insp.rlen(row) {
                break;
            }
        }
        let c = self.caps.clr_eol.clone();
        self.out.putp(&c);
    }

    /// `insp_show_pgs (col, row, max)`: the status line and a page of rows.
    fn insp_show_pgs(&mut self, col: i32, row: i32, max: i32) {
        let nl = self.insp.nl;
        let r = i32::try_from(format!("{nl}").len()).unwrap_or(0);
        let c = i32::try_from(format!("{}", col.wrapping_add(self.screen_cols)).len()).unwrap_or(0);
        let (mut l, mut ls) = (row.wrapping_add(1), nl);
        if self.insp.bufrd == 0 {
            l = 0;
            ls = 0;
        }
        let name = self
            .inspect
            .tab
            .get(self.insp.sel)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let buf = snprintf(
            SMLBUFSIZ,
            nls::YINSP_STATUS_FMT.as_bytes(),
            &[
                Arg::S(&name),
                Arg::I(i64::from(r)),
                Arg::I(i64::from(l)),
                Arg::I(i64::from(r)),
                Arg::I(i64::from(ls)),
                Arg::I(i64::from(c)),
                Arg::I(i64::from(col).wrapping_add(1)),
                Arg::I(i64::from(c)),
                Arg::I(i64::from(col.wrapping_add(self.screen_cols))),
                Arg::U(u64::try_from(self.insp.bufrd).unwrap_or(0)),
            ],
        )
        .0;
        self.insp_mksl(false, &buf);
        let mut max = max;
        let mut row = row;
        while max != 0 && row < nl {
            self.out.putp(b"\n");
            if self.insp.utf8 {
                self.insp_mkrow_utf8(col, row);
            } else {
                self.insp_mkrow_raw(col, row);
            }
            max = max.wrapping_sub(1);
            row = row.wrapping_add(1);
        }
        if max != 0 {
            let c = self.caps.nl_clreos.clone();
            self.out.putp(&c);
        }
    }

    /// `insp_view_choice (p)`: the output paged, scrolled and searched until
    /// the user leaves -- with `q`/Esc (the key comes back) or Enter
    /// (`INT_MAX`, to choose again).
    fn insp_view_choice(&mut self, pid: i32, cmd: &[u8]) -> i32 {
        let hz_amt = 8;
        let (mut curlin, mut curcol) = (0i32, 0i32);
        'signify: loop {
            let c = self.caps.clr_scr.clone();
            self.out.putp(&c);
            self.adj_geometry();
            loop {
                let max_ln = self.screen_rows.wrapping_sub(self.msg_row.wrapping_add(1));
                if curcol < 0 {
                    curcol = 0;
                }
                if curlin >= self.insp.nl {
                    curlin = self.insp.nl.wrapping_sub(1);
                }
                if curlin < 0 {
                    curlin = 0;
                }
                let sel = self.inspect.tab.get(self.insp.sel).cloned();
                let fstr = sel.as_ref().map(|e| e.fstr.clone()).unwrap_or_default();
                let flen = sel.as_ref().map_or(0, |e| e.flen);
                let buf = if flen < 22 {
                    snprintf(LRGBUFSIZ, b"%s", &[Arg::S(&fstr)]).0
                } else {
                    snprintf(LRGBUFSIZ, b"%.19s...", &[Arg::S(&fstr)]).0
                };
                let pidtxt = snprintf(6, b"%d", &[Arg::I(i64::from(pid))]).0;
                let cmdtxt = snprintf(64, b"%s", &[Arg::S(cmd)]).0;
                let home = self.caps.home.clone();
                self.out.putp(&home);
                let locating: &[u8] = if fstr.is_empty() { b" N/A " } else { &buf };
                let hd = Self::fmtmk(
                    nls::YINSP_HDVIEW_FMT,
                    &[Arg::S(&pidtxt), Arg::S(&cmdtxt), Arg::S(locating)],
                );
                self.show_special(true, &hd);
                self.insp_show_pgs(curcol, curlin, max_ln);
                self.out.flush();
                // `tcflush`: keys a terminal cannot discard are only read a moment later.
                let _ = libcall::termios::flush_input(0);

                if crate::frames() != 0 {
                    continue 'signify;
                }
                let mut key = self.iokey(IOKEY_ONCE);
                if key < 1 {
                    continue 'signify;
                }
                let max_ln = self.screen_rows.wrapping_sub(self.msg_row.wrapping_add(1));
                match key {
                    KBD_ENTER | KBD_ESC => {
                        if key == KBD_ENTER {
                            key = i32::MAX;
                        }
                        let c = self.caps.clr_scr.clone();
                        self.out.putp(&c);
                        return key;
                    }
                    k if k == i32::from(b'q') => {
                        let c = self.caps.clr_scr.clone();
                        self.out.putp(&c);
                        return key;
                    }
                    KBD_LEFT => curcol = curcol.wrapping_sub(hz_amt),
                    KBD_RIGHT => curcol = curcol.wrapping_add(hz_amt),
                    KBD_UP => curlin = curlin.wrapping_sub(1),
                    KBD_DOWN => curlin = curlin.wrapping_add(1),
                    k if k == KBD_PGUP || k == i32::from(b'b') => {
                        curlin = curlin.wrapping_sub(max_ln.wrapping_sub(1));
                    }
                    k if k == KBD_PGDN || k == KBD_SPACE => {
                        curlin = curlin.wrapping_add(max_ln.wrapping_sub(1));
                    }
                    k if k == KBD_HOME || k == i32::from(b'g') => {
                        curcol = 0;
                        curlin = 0;
                    }
                    k if k == KBD_END || k == i32::from(b'G') => {
                        curcol = 0;
                        curlin = self.insp.nl.wrapping_sub(max_ln);
                    }
                    k if b"L&/n".iter().any(|&c| i32::from(c) == k) => {
                        if self.insp.utf8 {
                            let rbeg = self.insp.row_at(curlin);
                            let rowtxt =
                                c_str(self.insp.buf.get(rbeg..).unwrap_or_default()).to_vec();
                            let mut tmpcol = utf8::proper_col(&rowtxt, curcol, true);
                            self.insp_find_str(k, &mut tmpcol, &mut curlin);
                            let rbeg = self.insp.row_at(curlin);
                            let rowtxt =
                                c_str(self.insp.buf.get(rbeg..).unwrap_or_default()).to_vec();
                            curcol = utf8::proper_col(&rowtxt, tmpcol, false);
                        } else {
                            self.insp_find_str(k, &mut curcol, &mut curlin);
                        }
                        // "must re-hide cursor in case a prompt for a string
                        // makes it huge"
                        self.cursor_state = self.caps.curs_hide.clone();
                        let c = self.cursor_state.clone();
                        self.out.putp(&c);
                    }
                    k if k == i32::from(b'=') => {
                        let (typ, fmts) = sel
                            .as_ref()
                            .map(|e| (e.typ.clone(), e.fmts.clone()))
                            .unwrap_or_default();
                        let b = snprintf(LRGBUFSIZ, b"%s: %s", &[Arg::S(&typ), Arg::S(&fmts)]).0;
                        self.insp_mksl(true, &b);
                        if self.iokey(IOKEY_ONCE) < 1 {
                            continue 'signify;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// `inspection_utility (pid)`: the inspect screens for `pid` -- choose an
    /// entry, see its output, choose again or leave.
    pub fn inspection_utility(&mut self, pid: i32) {
        let w = self.curwin;
        let n = self.pidsmaxt();
        let Some(found_idx) = (0..n).find(|&i| self.pv_int(w, i, EU_PID) == pid) else {
            let msg = Self::fmtmk(nls::YINSP_PIDBAD_FMT, &[Arg::I(i64::from(pid))]);
            self.show_msg(&msg);
            return;
        };
        // "must re-hide cursor since the prompt for a pid made it huge"
        self.cursor_state = self.caps.curs_hide.clone();
        let c = self.cursor_state.clone();
        self.out.putp(&c);
        let total = self.inspect.total;
        'signify: loop {
            let c = self.caps.clr_scr.clone();
            self.out.putp(&c);
            self.adj_geometry();
            // `signify_that:` forgets the key, so every way back here reads
            // a new one.
            let mut key = i32::MAX;
            loop {
                // `mkSEL (sels)`.
                let sel = self.st.insp_sel;
                let mut sels: Vec<u8> = Vec::new();
                for (k, e) in self.inspect.tab.iter_mut().enumerate() {
                    e.caps = if i32::try_from(k) == Ok(sel) {
                        b"~4"
                    } else {
                        b"~1"
                    };
                }
                for e in &self.inspect.tab {
                    let s = snprintf(SMLBUFSIZ, b" %s %s", &[Arg::S(&e.name), Arg::S(e.caps)]).0;
                    let room = (SCREENMAX - 1).saturating_sub(sels.len());
                    sels.extend_from_slice(s.get(..s.len().min(room)).unwrap_or_default());
                }
                let home = self.caps.home.clone();
                self.out.putp(&home);
                // Upstream names the task at row `Inspect.total` of the
                // window here -- its loop counter, left over from `mkSEL` --
                // not the one being inspected.
                let shown_cmd = self.pv_str(w, total, EU_CMD);
                let hd = Self::fmtmk(
                    nls::YINSP_HDSELS_FMT,
                    &[Arg::I(i64::from(pid)), Arg::S(&shown_cmd), Arg::S(&sels)],
                );
                self.show_special(true, &hd);
                self.insp_mksl(false, b" ");

                if crate::frames() != 0 {
                    continue 'signify;
                }
                if key == i32::MAX {
                    key = self.iokey(IOKEY_ONCE);
                }
                if key < 1 {
                    continue 'signify;
                }
                match key {
                    k if k == i32::from(b'q') || k == KBD_ESC => {}
                    KBD_END | KBD_LEFT => {
                        if key == KBD_END {
                            self.st.insp_sel = 0;
                        }
                        self.st.insp_sel = self.st.insp_sel.wrapping_sub(1);
                        if self.st.insp_sel < 0 {
                            self.st.insp_sel = total.wrapping_sub(1);
                        }
                        key = i32::MAX;
                    }
                    KBD_HOME | KBD_RIGHT => {
                        if key == KBD_HOME {
                            self.st.insp_sel = total;
                        }
                        self.st.insp_sel = self.st.insp_sel.wrapping_add(1);
                        if self.st.insp_sel >= total {
                            self.st.insp_sel = 0;
                        }
                        key = i32::MAX;
                    }
                    KBD_ENTER => {
                        let sel = usize::try_from(self.st.insp_sel).unwrap_or(0);
                        let Some(entry) = self.inspect.tab.get(sel).cloned() else {
                            continue 'signify;
                        };
                        let busy = if entry.typ.as_slice() == b"file" {
                            nls::YINSP_WAITIN_TXT
                        } else {
                            nls::YINSP_WORKIN_TXT
                        };
                        self.insp_mksl(false, busy.as_bytes());
                        self.insp.sel = sel;
                        self.insp_run(entry.func, &entry.fmts, pid);
                        let body = c_str(&self.insp.buf).to_vec();
                        self.insp.utf8 = utf8::delta(&body) != 0;
                        let cmd = self.pv_str(w, found_idx, EU_CMD);
                        key = self.insp_view_choice(pid, &cmd);
                        self.insp.buf.clear();
                        self.insp.p.clear();
                    }
                    _ => continue 'signify,
                }
                if key == i32::from(b'q') || key == KBD_ESC {
                    break 'signify;
                }
            }
        }
        crate::set_frames(BREAK_SCREEN);
    }
}
