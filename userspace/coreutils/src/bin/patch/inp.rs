//! `inp.c`: the file being patched, as numbered lines.
//!
//! Plan A reads it into memory and indexes its lines. Plan B -- what `-x 16`
//! asks for, and upstream's fallback when memory runs out -- copies it into
//! a temporary file in fixed-size records, one line per record and blocks of
//! records, and reads back a block at a time into one of two buffers. Both
//! are upstream's to the byte, including where they disagree: plan A looks
//! for a `Prereq:` revision only where one ends before the last byte of the
//! file, and plan B only where whitespace follows it.
//!
//! Plan B's two buffers are one allocation, as upstream's are, so that a
//! final unterminated line longer than its record runs on into the second
//! buffer as it does there. Upstream would write on past the end of the
//! allocation when it is longer still; such a line is cut at the end here.

use crate::sys::{self, oflag};
use crate::util::{self, say};
use crate::{Ctx, Lin, TMP_IN, Verbosity};

/// `TIBUFSIZE_MINIMUM`.
const TIBUFSIZE_MINIMUM: usize = 8 * 1024;

/// `NULL_DEVICE`.
const NULL_DEVICE: &[u8] = b"/dev/null";

/// The input file's lines, by one plan or the other.
#[derive(Debug)]
pub struct Inp {
    /// `i_buffer`: plan A's copy of the file.
    i_buffer: Vec<u8>,
    /// `i_ptr`: where each line starts in it, from `[1]`; the entry after
    /// the last line is where that line ends.
    i_ptr: Vec<usize>,
    /// `tifd`: plan B's temporary file, -1 with none.
    tifd: i32,
    /// `tibufsize`: the size of a block, and of each buffer.
    tibufsize: usize,
    /// `tibuf[0]` and `tibuf[1]`, one after the other.
    tibuf: Vec<u8>,
    /// `tiline`: the first line in each buffer, -1 for none.
    tiline: [Lin; 2],
    /// `lines_per_buf`.
    lines_per_buf: Lin,
    /// `tireclen`: the length of a record.
    tireclen: usize,
    /// `last_line_size`.
    last_line_size: usize,
}

impl Default for Inp {
    fn default() -> Self {
        Self {
            i_buffer: Vec::new(),
            i_ptr: Vec::new(),
            tifd: -1,
            tibufsize: 0,
            tibuf: Vec::new(),
            tiline: [-1, -1],
            lines_per_buf: 0,
            tireclen: 0,
            last_line_size: 0,
        }
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

// Line numbers and record offsets are upstream's arithmetic, bounded as
// upstream bounds them (`too_many_lines`, `lines_too_long`).
#[allow(clippy::arithmetic_side_effects)]
impl Ctx {
    /// `re_input`: ready for another file.
    pub fn re_input(&mut self) {
        if self.using_plan_a {
            self.inp.i_buffer = Vec::new();
            self.inp.i_ptr = Vec::new();
        } else {
            if self.inp.tifd >= 0 {
                // The plan B file is only read; a failure to close it loses
                // nothing.
                let _ = sys::close_fd(self.inp.tifd);
            }
            self.inp.tifd = -1;
            self.inp.tibuf = Vec::new();
            self.inp.tiline = [-1, -1];
            self.inp.tireclen = 0;
        }
    }

    /// `scan_input`: the line index built, by plan A if it can be.
    pub fn scan_input(&mut self, filename: &[u8], file_type: u32) {
        self.using_plan_a = self.debug & 16 == 0 && self.plan_a(filename);
        if !self.using_plan_a {
            if file_type & sys::S_IFMT != sys::S_IFREG {
                let mut m = b"Can't handle symbolic link ".to_vec();
                m.extend_from_slice(&self.q(filename));
                self.fatal(&m);
            }
            self.plan_b(filename);
        }
    }

    /// `report_revision`: whether the `Prereq:` revision was found, and
    /// what follows from it.
    fn report_revision(&mut self, found_revision: bool) {
        let rev = self.q(self.revision.as_deref().unwrap_or_default());
        if found_revision {
            if self.verbosity == Verbosity::Verbose {
                let mut m = b"Good.  This file appears to be the ".to_vec();
                m.extend_from_slice(&rev);
                m.extend_from_slice(b" version.\n");
                say(&m);
            }
        } else if self.force {
            if self.verbosity != Verbosity::Silent {
                let mut m = b"Warning: this file doesn't appear to be the ".to_vec();
                m.extend_from_slice(&rev);
                m.extend_from_slice(b" version -- patching anyway.\n");
                say(&m);
            }
        } else if self.batch {
            let mut m = b"This file doesn't appear to be the ".to_vec();
            m.extend_from_slice(&rev);
            m.extend_from_slice(b" version -- aborting.");
            self.fatal(&m);
        } else {
            let mut m = b"This file doesn't appear to be the ".to_vec();
            m.extend_from_slice(&rev);
            m.extend_from_slice(b" version -- patch anyway? [n] ");
            self.ask(&m);
            if self.answer() != b'y' {
                self.fatal(b"aborted");
            }
        }
    }

    fn too_many_lines(&mut self, filename: &[u8]) -> ! {
        let mut m = b"File ".to_vec();
        m.extend_from_slice(&self.q(filename));
        m.extend_from_slice(b" has too many lines");
        self.fatal(&m);
    }

    fn lines_too_long(&mut self, filename: &[u8]) -> ! {
        let mut m = b"Lines in file ".to_vec();
        m.extend_from_slice(&self.q(filename));
        m.extend_from_slice(b" are too long");
        self.fatal(&m);
    }

    /// `get_input_file`: the input file's status, fetched from RCS, SCCS,
    /// ClearCase or Perforce first if it is under one and `-g` allows; false
    /// when it is not a file of the kind the patch is for.
    pub fn get_input_file(&mut self, filename: &[u8], outname: &[u8], file_type: u32) -> bool {
        let elsewhere = filename != outname;

        if self.inerrno == -1 {
            match self.stat_file(filename) {
                Ok(st) => {
                    self.instat = st;
                    self.inerrno = 0;
                }
                Err(e) => self.inerrno = e,
            }
        }

        // Perhaps look for RCS or SCCS versions.
        let mode = self.instat.mode;
        let looks = file_type & sys::S_IFMT == sys::S_IFREG
            && self.patch_get != 0
            && self.invc != 0
            && (self.inerrno != 0
                || (!elsewhere
                    && (
                        // No one can write to it.
                        mode & 0o222 == 0
                        // Only the owner (who's not me) can write to it.
                        || (mode & 0o022 == 0 && self.instat.uid != sys::effective_ids().0)
                    )));
        if looks {
            let filestat = (self.inerrno == 0).then_some(self.instat);
            let found = self.version_controller(filename, elsewhere, filestat.as_ref());
            self.invc = i32::from(found.is_some());
            if let Some((name, getbuf, diffbuf)) = found {
                let mut cs = Some(name);
                if self.inerrno == 0 {
                    if !elsewhere && mode & 0o222 != 0 {
                        // Somebody can write to it.
                        let mut m = b"File ".to_vec();
                        m.extend_from_slice(&self.q(filename));
                        m.extend_from_slice(
                            format!(" seems to be locked by somebody else under {name}").as_bytes(),
                        );
                        self.fatal(&m);
                    }
                    if let Some(diffbuf) = diffbuf {
                        // It might be checked out unlocked. See if it's safe
                        // to check out the default version locked.
                        if self.verbosity == Verbosity::Verbose {
                            let mut m = b"Comparing file ".to_vec();
                            m.extend_from_slice(&self.q(filename));
                            m.extend_from_slice(
                                format!(" to default {name} version...\n").as_bytes(),
                            );
                            say(&m);
                        }
                        if self.systemic(&diffbuf) != 0 {
                            let mut m = b"warning: Patching file ".to_vec();
                            m.extend_from_slice(&self.q(filename));
                            m.extend_from_slice(
                                format!(", which does not match default {name} version\n")
                                    .as_bytes(),
                            );
                            say(&m);
                            cs = None;
                        }
                    }
                    if self.dry_run {
                        cs = None;
                    }
                }
                if let Some(cs) = cs {
                    let exists = self.inerrno == 0;
                    let mut st = self.instat;
                    if self.version_get(filename, cs, exists, elsewhere, &getbuf, &mut st) {
                        self.instat = st;
                        self.inerrno = 0;
                    }
                }
            }
        }

        if self.inerrno != 0 {
            self.instat.mode = 0o666;
            self.instat.size = 0;
        } else {
            let ft = file_type & sys::S_IFMT;
            if !((ft == sys::S_IFREG || ft == sys::S_IFLNK) && ft == self.instat.mode & sys::S_IFMT)
            {
                let mut m = b"File ".to_vec();
                m.extend_from_slice(&self.q(filename));
                m.extend_from_slice(b" is not a ");
                m.extend_from_slice(if ft == sys::S_IFLNK {
                    b"symbolic link".as_slice()
                } else {
                    b"regular file"
                });
                m.extend_from_slice(b" -- refusing to patch\n");
                say(&m);
                return false;
            }
        }
        true
    }

    /// `plan_a`: the file in memory, its lines indexed; false when it cannot
    /// be read that way.
    fn plan_a(&mut self, filename: &[u8]) -> bool {
        let Ok(mut size) = usize::try_from(self.instat.size) else {
            return false;
        };
        let mut buffer: Vec<u8> = Vec::new();
        if buffer.try_reserve_exact(size.max(1)).is_err() {
            return false;
        }

        // Read the input file, but don't bother reading it if it's empty.
        // When creating files, the files do not actually exist.
        if size != 0 {
            if self.instat.is_reg() {
                let flags = oflag::RDONLY | self.nofollow();
                let ifd = match self.safe.open(filename, flags, 0) {
                    Ok(fd) => fd,
                    Err(e) => {
                        let mut m = b"can't open file ".to_vec();
                        m.extend_from_slice(&self.q(filename));
                        self.pfatal(&m, e);
                    }
                };
                buffer.resize(size, 0);
                let mut buffered = 0usize;
                while size - buffered != 0 {
                    let Some(dest) = buffer.get_mut(buffered..size) else {
                        break;
                    };
                    match coreutils::stdfd::read(ifd, dest) {
                        Ok(0) => {
                            // The file may have shrunk!
                            size = buffered;
                            break;
                        }
                        Ok(n) => buffered += n,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(_) => {
                            // Perhaps size is too large for this host.
                            let _ = sys::close_fd(ifd);
                            return false;
                        }
                    }
                }
                buffer.truncate(size);
                if sys::close_fd(ifd).is_err() {
                    self.read_fatal();
                }
            } else if self.instat.is_lnk() {
                match self.safe.readlink(filename, size) {
                    Ok(target) => {
                        size = target.len();
                        buffer = target;
                    }
                    Err(e) => {
                        let mut m = b"can't read symbolic link ".to_vec();
                        m.extend_from_slice(&self.q(filename));
                        self.pfatal(&m, e);
                    }
                }
            } else {
                return false;
            }
        }

        // Scan the buffer and build array of pointers to lines.
        let lim = size;
        let mut iline: Lin = 3; // 1 unused, 1 for SOF, 1 for EOF if last line is incomplete
        for _ in buffer.iter().filter(|&&c| c == b'\n') {
            iline += 1;
            if iline < 0 {
                self.too_many_lines(filename);
            }
        }
        let mut ptr: Vec<usize> = Vec::new();
        if ptr
            .try_reserve_exact(usize::try_from(iline).unwrap_or(0))
            .is_err()
        {
            return false;
        }
        ptr.push(0);
        let mut s = 0usize;
        loop {
            ptr.push(s);
            match buffer
                .get(s..lim)
                .and_then(|rest| rest.iter().position(|&c| c == b'\n'))
            {
                Some(k) => s = s + k + 1,
                None => break,
            }
        }
        if size != 0 && buffer.get(lim - 1) != Some(&b'\n') {
            ptr.push(lim);
        }
        self.input_lines = Lin::try_from(ptr.len()).unwrap_or(Lin::MAX) - 2;

        if let Some(rev) = self.revision.clone() {
            let revlen = rev.len();
            let mut found_revision = false;
            if revlen <= size
                && let Some(&rev0) = rev.first()
            {
                // Only a revision that starts before `lim - revlen` -- not
                // one that ends at the very end -- and that is followed by
                // white space unless it ends one byte short of that:
                // upstream's bounds, kept.
                let limrev = lim - revlen;
                let mut s = 0usize;
                while s < limrev {
                    match buffer
                        .get(s..limrev)
                        .and_then(|r| r.iter().position(|&c| c == rev0))
                    {
                        Some(k) => s += k,
                        None => break,
                    }
                    if buffer.get(s..s + revlen) == Some(rev.as_slice())
                        && (s == 0 || buffer.get(s - 1).is_some_and(|&c| is_space(c)))
                        && (s + 1 == limrev || buffer.get(s + revlen).is_some_and(|&c| is_space(c)))
                    {
                        found_revision = true;
                        break;
                    }
                    s += 1;
                }
            }
            self.report_revision(found_revision);
        }

        // Plan A will work.
        self.inp.i_buffer = buffer;
        self.inp.i_ptr = ptr;
        true
    }

    /// `plan_b`: the file copied into the temporary file's records.
    fn plan_b(&mut self, filename: &[u8]) {
        let filename = if self.instat.size == 0 {
            NULL_DEVICE
        } else {
            filename
        };
        let flags = oflag::RDONLY | self.nofollow();
        let ifd = match self.safe.open(filename, flags, 0) {
            Ok(fd) => fd,
            Err(e) => {
                let mut m = b"Can't open file ".to_vec();
                m.extend_from_slice(&self.q(filename));
                self.pfatal(&m, e);
            }
        };
        if self.tmpin.needs_removal {
            // Reopen the existing temporary file.
            let name = self.tmpin.name.clone().unwrap_or_default();
            self.inp.tifd = self.create_file(&name, oflag::RDWR, 0, true);
        } else {
            let (name, made) = self.make_tempfile(b'i', None, oflag::RDWR, 0o600);
            self.tmpin.name = Some(name.clone());
            match made {
                Ok(fd) => self.inp.tifd = fd,
                Err(e) => {
                    // Unquoted, as upstream has it here.
                    let mut m = b"Can't create temporary file ".to_vec();
                    m.extend_from_slice(&name);
                    self.pfatal(&m, e);
                }
            }
            self.tmpin.needs_removal = true;
            util::register_temp(TMP_IN, &name);
        }

        let mut data = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            match coreutils::stdfd::read(ifd, &mut chunk) {
                Ok(0) => break,
                Ok(n) => data.extend_from_slice(chunk.get(..n).unwrap_or_default()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => self.read_fatal(),
            }
        }

        // The longest line, and the revision, in one pass.
        let rev = self.revision.clone();
        let revlen = rev.as_ref().map_or(0, Vec::len);
        let mut found_revision = rev.is_none();
        // `i`, with upstream's `(size_t) -1` as `None`.
        let mut i: Option<usize> = Some(0);
        let mut len = 0usize;
        let mut maxlen = 1usize;
        let mut line: Lin = 1;
        for &c in &data {
            len += 1;
            if len > usize::MAX / 2 {
                self.lines_too_long(filename);
            }
            if c == b'\n' {
                line += 1;
                if line < 0 {
                    self.too_many_lines(filename);
                }
                if maxlen < len {
                    maxlen = len;
                }
                len = 0;
            }
            if !found_revision {
                if i == Some(revlen) {
                    found_revision = is_space(c);
                    i = None;
                } else if let Some(k) = i {
                    i = (rev.as_ref().and_then(|r| r.get(k)) == Some(&c)).then_some(k + 1);
                }
                if i.is_none() && is_space(c) {
                    i = Some(0);
                }
            }
        }
        if rev.is_some() {
            self.report_revision(found_revision);
        }

        let mut tibufsize = TIBUFSIZE_MINIMUM;
        while tibufsize < maxlen {
            tibufsize <<= 1;
        }
        let lines_per_buf = Lin::try_from(tibufsize / maxlen).unwrap_or(1);
        self.inp.tibufsize = tibufsize;
        self.inp.lines_per_buf = lines_per_buf;
        self.inp.tireclen = maxlen;
        self.inp.tibuf = vec![0u8; 2 * tibufsize];

        let tifd = self.inp.tifd;
        let mut pos = 0usize;
        let mut line: Lin = 1;
        'eof: loop {
            let slot = usize::try_from(line % lines_per_buf).unwrap_or(0);
            let mut p = maxlen * slot;
            let p0 = p;
            if line % lines_per_buf == 0 {
                // New block.
                self.write_block(tifd);
            }
            let Some(&first) = data.get(pos) else {
                break 'eof;
            };
            pos += 1;
            let mut c = first;
            loop {
                if let Some(b) = self.inp.tibuf.get_mut(p) {
                    *b = c;
                }
                p += 1;
                if c == b'\n' {
                    self.inp.last_line_size = p - p0;
                    break;
                }
                let Some(&next) = data.get(pos) else {
                    self.inp.last_line_size = p - p0;
                    line += 1;
                    break 'eof;
                };
                pos += 1;
                c = next;
            }
            line += 1;
        }
        // `fclose (ifp)`.
        if sys::close_fd(ifd).is_err() {
            self.read_fatal();
        }

        if line % lines_per_buf != 0 {
            self.write_block(tifd);
        }
        self.input_lines = line - 1;
    }

    /// `write (tifd, tibuf[0], tibufsize)`, all of it or `write_fatal`.
    fn write_block(&mut self, tifd: i32) {
        let size = self.inp.tibufsize;
        let block = self.inp.tibuf.get(..size).unwrap_or_default().to_vec();
        if coreutils::stdfd::write_all(tifd, &block).is_err() {
            self.write_fatal();
        }
    }

    /// `ifetch`: line `line` of the input file -- empty past either end.
    /// `whichbuf` says which of plan B's buffers to read a block into when
    /// neither holds it.
    pub fn ifetch(&mut self, line: Lin, whichbuf: bool) -> &[u8] {
        if line < 1 || line > self.input_lines {
            return &[];
        }
        if self.using_plan_a {
            let at = |i: Lin| {
                usize::try_from(i)
                    .ok()
                    .and_then(|i| self.inp.i_ptr.get(i))
                    .copied()
            };
            let (Some(p), Some(q)) = (at(line), at(line + 1)) else {
                return &[];
            };
            return self.inp.i_buffer.get(p..q).unwrap_or_default();
        }
        let lpb = self.inp.lines_per_buf.max(1);
        let offline = line % lpb;
        let baseline = line - offline;
        let which = if self.inp.tiline[0] == baseline {
            false
        } else if self.inp.tiline[1] == baseline {
            true
        } else {
            if let Some(t) = self.inp.tiline.get_mut(usize::from(whichbuf)) {
                *t = baseline;
            }
            let size = self.inp.tibufsize;
            let block = usize::try_from(baseline / lpb).unwrap_or(0);
            let offset = i64::try_from(block * size).unwrap_or(i64::MAX);
            if sys::lseek_fd(self.inp.tifd, offset, sys::SEEK_SET).is_err() {
                self.read_fatal();
            }
            let start = usize::from(whichbuf) * size;
            let tifd = self.inp.tifd;
            if let Some(dest) = self.inp.tibuf.get_mut(start..start + size) {
                // One `read`, as upstream: a short one leaves the rest of the
                // buffer as it was.
                if coreutils::stdfd::read(tifd, dest).is_err() {
                    self.read_fatal();
                }
            }
            whichbuf
        };
        let p = usize::from(which) * self.inp.tibufsize
            + self.inp.tireclen * usize::try_from(offline).unwrap_or(0);
        let size = if line == self.input_lines {
            self.inp.last_line_size
        } else {
            self.inp
                .tibuf
                .get(p..)
                .and_then(|r| r.iter().position(|&c| c == b'\n'))
                .map_or(0, |k| k + 1)
        };
        let end = (p + size).min(self.inp.tibuf.len());
        self.inp.tibuf.get(p..end).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::Inp;

    #[test]
    fn a_new_input_has_no_plan_b_file() {
        let inp = Inp::default();
        assert_eq!(inp.tifd, -1);
        assert_eq!(inp.tiline, [-1, -1]);
    }
}
