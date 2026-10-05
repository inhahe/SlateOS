//! argp's line filler: text written into a buffer and laid out in lines no
//! wider than a right margin, each line after a break begun at a wrap
//! margin, each line after a newline at a left margin -- glibc's
//! argp-fmtstream, which every word of argp's help and usage goes through.
//!
//! **Its buffering is part of its output, so it is glibc's too.** The text
//! waits in a buffer of 200 bytes (grown only when one write needs more),
//! and is laid out only when something asks where the line has got to, a
//! margin changes, or the buffer must be written out to make room -- which
//! a formatted write does whenever 150 bytes are not free. What has been
//! written out cannot be broken again: a line whose last blank went out
//! with the buffer is broken after its next word instead, and glibc's
//! output says so. So the sizes and the moments are glibc's, and so is the
//! scan for a break: back from the first column past the margin to a
//! blank, the blanks there swallowed and the newline put in place of the
//! first; a word longer than a line left on a line of its own; blanks to
//! the wrap margin after each break, to the left margin after each newline.
//!
//! Two departures, both where glibc's own scan leaves the line it is
//! breaking (posix/argp's tests list the oracle's cases):
//!
//! - **A line that begins past the right margin** -- a wrap margin at or
//!   beyond it, or a right margin of 0: glibc's scan starts before its
//!   buffer, reads outside it, and on every help tried loops for ever
//!   (`ARGP_HELP_FMT=rmargin=20`). Here such a line takes the next word, and
//!   the next line the one after: one word to a line, at the margin it was
//!   to begin at.
//! - **A word longer than a line that ends exactly at the margin**: glibc's
//!   scan for its end steps past it -- over the newline after it, whose
//!   next character it then overwrites, or past the buffered text, into
//!   whatever its buffer last held there, which its output then depends
//!   on. Here the word fits, as it does: it ends the line, or stays for
//!   the text after it to decide.
//!
//! Where glibc's lines come out ugly but come out the same each time -- a
//! wrap margin one column short of the right one fills each line with one
//! word, or with nothing -- they come out the same here. The byte past the
//! buffered text, which glibc's scan reads when a line reaches the margin
//! exactly, is a NUL here, as it is in glibc after a formatted write.

use crate::list::{List, NoMem};

/// The buffer's first size, and the room a formatted write asks for before
/// it formats: glibc's, which decide where text is written out part way.
const INIT_BUF_SIZE: usize = 200;
const PRINTF_SIZE_GUESS: usize = 150;

/// A blank, as C's `isblank` has one: a space or a tab.
const fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// The text of one help or usage message on its way to a stream.
pub(crate) struct FmtStream {
    /// The C stream (`FILE *`) it is written to.
    stream: *mut u8,
    /// Text not yet written out; its length is glibc's `p - buf`.
    buf: List<u8>,
    /// glibc's buffer size: how much text may wait before it is written out.
    cap: usize,
    /// How much of `buf` has been laid out.
    point_offs: usize,
    /// The column laid-out text ends in -- -1 just after a break with no
    /// wrap margin, so that the left margin is not added there (glibc's).
    point_col: isize,
    lmargin: usize,
    rmargin: usize,
    wmargin: usize,
}

impl FmtStream {
    /// A filler writing to `stream` (a `FILE *`), with these margins; `None`
    /// when the heap has no room for its buffer.
    pub(crate) fn new(
        stream: *mut u8,
        lmargin: usize,
        rmargin: usize,
        wmargin: usize,
    ) -> Option<Self> {
        let buf = List::with_capacity(INIT_BUF_SIZE).ok()?;
        Some(Self {
            stream,
            buf,
            cap: INIT_BUF_SIZE,
            point_offs: 0,
            point_col: 0,
            lmargin,
            rmargin,
            wmargin,
        })
    }

    /// The left margin.
    pub(crate) const fn lmargin(&self) -> usize {
        self.lmargin
    }

    /// The right margin.
    pub(crate) const fn rmargin(&self) -> usize {
        self.rmargin
    }

    /// The wrap margin.
    pub(crate) const fn wmargin(&self) -> usize {
        self.wmargin
    }

    /// Lay out what is waiting, then set the left margin: the old one.
    pub(crate) fn set_lmargin(&mut self, m: usize) -> usize {
        self.catch_up();
        core::mem::replace(&mut self.lmargin, m)
    }

    /// Lay out what is waiting, then set the wrap margin: the old one.
    pub(crate) fn set_wmargin(&mut self, m: usize) -> usize {
        self.catch_up();
        core::mem::replace(&mut self.wmargin, m)
    }

    /// The column the text so far ends in, laid out first.
    pub(crate) fn point(&mut self) -> usize {
        self.catch_up();
        usize::try_from(self.point_col).unwrap_or(0)
    }

    /// Lay out what has not been, if anything.
    fn catch_up(&mut self) {
        if self.buf.len() > self.point_offs {
            self.update();
        }
    }

    /// `bytes` added, as glibc's `__argp_fmtstream_write`.
    pub(crate) fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.buf.len().saturating_add(bytes.len()) <= self.cap || self.ensure(bytes.len()) {
            // Room was made, or the heap had none: glibc loses the text then.
            let _ = self.buf.extend_from_slice(bytes);
        }
    }

    /// One byte added, as glibc's `__argp_fmtstream_putc`.
    pub(crate) fn putc(&mut self, c: u8) {
        if self.buf.len() < self.cap || self.ensure(1) {
            let _ = self.buf.push(c);
        }
    }

    /// The pieces, concatenated, added as glibc's `__argp_fmtstream_printf`
    /// adds a formatted string: room for 150 bytes made first, which may
    /// write the buffer out, and room for all of it if that is not enough.
    pub(crate) fn print(&mut self, pieces: &[&[u8]]) {
        let n = pieces
            .iter()
            .map(|p| p.len())
            .fold(0usize, usize::saturating_add);
        if !self.ensure(PRINTF_SIZE_GUESS) {
            return;
        }
        if n >= self.cap.saturating_sub(self.buf.len()) && !self.ensure(n.saturating_add(1)) {
            return;
        }
        for p in pieces {
            let _ = self.buf.extend_from_slice(p);
        }
    }

    /// Room for `amount` more bytes: the buffer laid out and written out
    /// when there is less, and grown when even an empty one is too small.
    /// False when it could not be: glibc's then drops the write.
    fn ensure(&mut self, amount: usize) -> bool {
        if self.cap.saturating_sub(self.buf.len()) >= amount {
            return true;
        }
        self.update();
        self.emit_all();
        self.point_offs = 0;
        if self.cap < amount {
            let new = self.cap.saturating_add(amount);
            if self
                .buf
                .reserve(new.saturating_sub(self.buf.len()))
                .is_err()
            {
                crate::errno::set_errno(crate::errno::ENOMEM);
                return false;
            }
            self.cap = new;
        }
        true
    }

    /// The whole buffer written to the stream, and emptied.
    fn emit_all(&mut self) {
        let n = self.buf.len();
        self.emit(0, n);
        self.buf.clear();
    }

    /// `buf[from..to]` written to the stream -- a failed write is the
    /// stream's to report, as glibc's leaves it.
    fn emit(&self, from: usize, to: usize) {
        if let Some(bytes) = self.buf.get(from..to)
            && !bytes.is_empty()
        {
            // SAFETY: the caller's stream, live for this message; the bytes
            // are the buffer's.
            unsafe { crate::stdio::fwrite(bytes.as_ptr(), 1, bytes.len(), self.stream) };
        }
    }

    /// Blanks written straight to the stream, ahead of what is waiting in
    /// the buffer -- glibc's way when its buffer has no room for them.
    fn emit_blanks(&self, n: usize) {
        for _ in 0..n {
            crate::stdio::fputc(i32::from(b' '), self.stream);
        }
    }

    /// `n` copies of `c` inserted at `at`, the text after moved up.
    fn insert(&mut self, at: usize, c: u8, n: usize) -> Result<(), NoMem> {
        let old = self.buf.len();
        self.buf.resize(old.saturating_add(n), c)?;
        let s = self.buf.as_mut_slice();
        s.copy_within(at..old, at.saturating_add(n));
        for b in s.iter_mut().skip(at).take(n) {
            *b = c;
        }
        Ok(())
    }

    /// `buf[i]`, and a NUL past the text (see the module documentation).
    fn at(&self, i: usize) -> u8 {
        self.buf.get(i).copied().unwrap_or(0)
    }

    /// Lay out everything not yet laid out: glibc's `__argp_fmtstream_update`.
    #[allow(clippy::too_many_lines)] // One scan, as glibc's; split, it would not read as it.
    fn update(&mut self) {
        // Where the scan is: glibc's `buf`.
        let mut b = self.point_offs;
        while b < self.buf.len() {
            if self.point_col == 0 && self.lmargin != 0 {
                // A new line: blanks to the left margin.
                let pad = self.lmargin;
                if self.buf.len().saturating_add(pad) < self.cap {
                    if self.insert(b, b' ', pad).is_err() {
                        self.emit_blanks(pad);
                    } else {
                        b = b.saturating_add(pad);
                    }
                } else {
                    self.emit_blanks(pad);
                }
                self.point_col = isize::try_from(pad).unwrap_or(isize::MAX);
            }
            let len = self.buf.len().saturating_sub(b);
            let found = self
                .buf
                .get(b..)
                .and_then(|s| s.iter().position(|&c| c == b'\n'));
            if self.point_col < 0 {
                self.point_col = 0;
            }
            let col = usize::try_from(self.point_col).unwrap_or(0);
            let mut nl = match found {
                None => {
                    if col.saturating_add(len) < self.rmargin {
                        // A partial line that fits: nothing more to do.
                        self.point_col = self.point_col.saturating_add_unsigned(len);
                        break;
                    }
                    self.buf.len()
                }
                Some(i) => {
                    if col.saturating_add(i) < self.rmargin {
                        // A whole line that fits.
                        self.point_col = 0;
                        b = b.saturating_add(i).saturating_add(1);
                        continue;
                    }
                    b.saturating_add(i)
                }
            };

            // The line is too long: break it. `past` is the first column past
            // the margin, as an index -- `None` when the line begins beyond
            // it, where glibc's scan would start before its buffer.
            let past = self
                .rmargin
                .checked_sub(col)
                .map(|room| b.saturating_add(room));
            let mut nextline;
            let mut p;
            // The blank to break at, scanning back from the first column past.
            let back = past.and_then(|start| (b..=start).rev().find(|&i| is_blank(self.at(i))));
            if let Some(blank) = back.filter(|&blank| blank.saturating_add(1) > b) {
                nextline = blank.saturating_add(1);
                // Swallow the blanks before it: the newline replaces the first.
                p = blank;
                while p > b && is_blank(self.at(p.saturating_sub(1))) {
                    p = p.saturating_sub(1);
                }
                nl = p;
            } else {
                // A word longer than a line: on a line of its own, glibc's
                // scan going on from the first column past the margin to the
                // word's end. Two cases are not glibc's (the module
                // documentation): a word that ends exactly at the margin,
                // and a line that begins past it.
                p = match past {
                    Some(start) if start < nl => {
                        let mut q = start.saturating_add(1);
                        while q < nl && !is_blank(self.at(q)) {
                            q = q.saturating_add(1);
                        }
                        q
                    }
                    Some(_) => nl,
                    None => {
                        let mut q = b;
                        while q < nl && is_blank(self.at(q)) {
                            q = q.saturating_add(1);
                        }
                        while q < nl && !is_blank(self.at(q)) {
                            q = q.saturating_add(1);
                        }
                        q
                    }
                };
                if p >= nl {
                    if past.is_some_and(|start| start < nl) || nl < self.buf.len() {
                        // It already ends a line -- or reaches the end of the
                        // text so far, where glibc's starts the column again
                        // all the same.
                        self.point_col = 0;
                        b = nl.saturating_add(1);
                        continue;
                    }
                    // It ends exactly at the margin, or past it on a line
                    // begun past it, with nothing after it yet: it stays, and
                    // the text after decides.
                    self.point_col = self.point_col.saturating_add_unsigned(len);
                    break;
                }
                nl = p;
                p = p.saturating_add(1);
                while is_blank(self.at(p)) {
                    p = p.saturating_add(1);
                }
                nextline = p;
            }

            // The newline, and the blanks of the wrap margin after it: where
            // the blanks swallowed leave room, in place; where not, the text
            // after moved up -- or, with no room for that either, the line so
            // far written out first, as glibc does.
            let end = self.buf.len();
            let wm = self.wmargin;
            let at_end = nextline == end.saturating_add(1);
            let short = if at_end {
                self.cap.saturating_sub(nl) < wm.saturating_add(1)
            } else {
                nextline.saturating_sub(nl.saturating_add(1)) < wm
            };
            if short && end > nextline {
                if self.cap.saturating_sub(end) > wm.saturating_add(1) {
                    // Make room for them.
                    let mv = end.saturating_sub(nextline);
                    let to = nl.saturating_add(1).saturating_add(wm);
                    let need = to.saturating_add(mv);
                    if need > end && self.buf.resize(need, b' ').is_err() {
                        return;
                    }
                    self.buf
                        .as_mut_slice()
                        .copy_within(nextline..nextline.saturating_add(mv), to);
                    if let Some(c) = self.buf.as_mut_slice().get_mut(nl) {
                        *c = b'\n';
                    }
                    nl = nl.saturating_add(1);
                    nextline = to;
                    self.buf.truncate(to.saturating_add(mv));
                } else {
                    // Write out the buffer up to the break, and its newline,
                    // and go on from the buffer's start: the next line's
                    // blanks overwrite what went out, the text after is moved
                    // down to them -- glibc's way, `nextline` still counted
                    // from the start.
                    self.emit(0, nl);
                    crate::stdio::fputc(i32::from(b'\n'), self.stream);
                    nl = 0;
                }
            } else {
                // Room for the newline and the blanks before the next word.
                if let Some(c) = self.buf.as_mut_slice().get_mut(nl) {
                    *c = b'\n';
                } else if self.buf.push(b'\n').is_err() {
                    return;
                }
                nl = nl.saturating_add(1);
            }
            let end = self.buf.len();
            let fits_here = nextline.saturating_sub(nl) >= wm
                || (nextline == end.saturating_add(1) && self.cap.saturating_sub(nextline) >= wm);
            if fits_here {
                let needed = nl.saturating_add(wm);
                if needed > self.buf.len() && self.buf.resize(needed, b' ').is_err() {
                    return;
                }
                for c in self.buf.as_mut_slice().iter_mut().skip(nl).take(wm) {
                    *c = b' ';
                }
                nl = needed;
            } else {
                self.emit_blanks(wm);
            }
            // The rest of the text after the newline and its blanks.
            let end = self.buf.len();
            if nl < nextline {
                if nextline < end {
                    self.buf.as_mut_slice().copy_within(nextline..end, nl);
                    self.buf
                        .truncate(nl.saturating_add(end.saturating_sub(nextline)));
                } else {
                    self.buf.truncate(nl);
                }
            } else if nl > nextline {
                // Only when the blanks went past the old text's end.
                self.buf.truncate(nl);
            }
            b = nl;
            self.point_col = if wm == 0 {
                -1
            } else {
                isize::try_from(wm).unwrap_or(isize::MAX)
            };
        }
        self.point_offs = self.buf.len();
    }

    /// The rest laid out and written out, and the filler done with:
    /// glibc's `__argp_fmtstream_free`.
    pub(crate) fn finish(mut self) {
        self.update();
        self.emit_all();
    }
}
