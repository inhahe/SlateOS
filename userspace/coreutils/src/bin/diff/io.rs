//! diffutils' `io.c`: reading the two files, finding what they share at the
//! start and the end, and putting every line of the rest into an equivalence
//! class -- the number the comparison works on instead of the line.
//!
//! What this decides is observable even though it is "only" preprocessing:
//! the identical prefix and suffix are cut at line boundaries and kept for
//! `--horizon-lines` (at least the context) lines, and only the lines between
//! are compared, so `discard_confusing_lines`' thresholds and
//! `shift_boundaries`' freedom both depend on where these cuts fall.
//!
//! Lines are equal when the active `-i`/`-E`/`-Z`/`-b`/`-w` rules say so.
//! Upstream finds a line's class by hashing it under those rules and then
//! confirming with `lines_differ` against each class of the same hash, newest
//! first; that order is kept, because `-E`'s hash and `lines_differ` do not
//! quite agree about a backspace or a carriage return, and where they
//! disagree the first class to say "equal" wins.

use crate::util::lines_differ;
use crate::{Opts, OutputStyle, WhiteSpace};
use std::io::Read;

/// `lin`: a line number, upstream's `ptrdiff_t`.
pub type Lin = isize;

/// What `stat` told us about a file -- the fields diff reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub mode: u32,
    pub size: i64,
    pub mtime_sec: i64,
    pub mtime_nsec: u32,
    pub ctime_sec: i64,
    pub dev: u64,
    pub ino: u64,
    pub rdev: u64,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub blksize: u64,
}

pub const S_IFMT: u32 = 0o170_000;
pub const S_IFDIR: u32 = 0o040_000;
pub const S_IFREG: u32 = 0o100_000;
pub const S_IFLNK: u32 = 0o120_000;
pub const S_IFCHR: u32 = 0o020_000;
pub const S_IFBLK: u32 = 0o060_000;
pub const S_IFIFO: u32 = 0o010_000;
pub const S_IFSOCK: u32 = 0o140_000;

impl Stat {
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
    pub fn is_reg(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }
    pub fn is_lnk(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }

    /// From a `std::fs::Metadata`.
    #[cfg(unix)]
    pub fn of(m: &std::fs::Metadata) -> Stat {
        use std::os::unix::fs::MetadataExt;
        Stat {
            mode: m.mode(),
            size: m.size().try_into().unwrap_or(i64::MAX),
            mtime_sec: m.mtime(),
            mtime_nsec: u32::try_from(m.mtime_nsec()).unwrap_or(0),
            ctime_sec: m.ctime(),
            dev: m.dev(),
            ino: m.ino(),
            rdev: m.rdev(),
            nlink: m.nlink(),
            uid: m.uid(),
            gid: m.gid(),
            blksize: m.blksize(),
        }
    }

    /// From a `std::fs::Metadata`, as well as the host can say.
    #[cfg(not(unix))]
    pub fn of(m: &std::fs::Metadata) -> Stat {
        let (mtime_sec, mtime_nsec) = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or((0, 0), |d| {
                (i64::try_from(d.as_secs()).unwrap_or(0), d.subsec_nanos())
            });
        let kind = if m.is_dir() {
            S_IFDIR
        } else if m.file_type().is_symlink() {
            S_IFLNK
        } else {
            S_IFREG
        };
        Stat {
            mode: kind | 0o644,
            size: m.len().try_into().unwrap_or(i64::MAX),
            mtime_sec,
            mtime_nsec,
            nlink: 1,
            blksize: 4096,
            ..Stat::default()
        }
    }

    /// gnulib's `ST_BLKSIZE`: the block size, or 512 when the field is absurd.
    pub fn blocksize(&self) -> usize {
        match usize::try_from(self.blksize) {
            Ok(b) if b > 0 && b <= usize::MAX / 8 + 1 => b,
            _ => 512,
        }
    }
}

/// `same_file`: one inode, or one special file.
pub fn same_file(s: &Stat, t: &Stat) -> bool {
    (s.ino == t.ino && s.dev == t.dev)
        || (((s.mode & S_IFMT == S_IFBLK && t.mode & S_IFMT == S_IFBLK)
            || (s.mode & S_IFMT == S_IFCHR && t.mode & S_IFMT == S_IFCHR))
            && s.rdev == t.rdev)
}

/// `same_file_attributes`: the attributes two names for one file must share.
pub fn same_file_attributes(s: &Stat, t: &Stat) -> bool {
    s.mode == t.mode
        && s.nlink == t.nlink
        && s.uid == t.uid
        && s.gid == t.gid
        && s.size == t.size
        && s.mtime_sec == t.mtime_sec
        && s.ctime_sec == t.ctime_sec
}

/// gnulib's `file_type`, as the "is a ... while ..." message says it.
pub fn file_type(st: &Stat) -> &'static str {
    match st.mode & S_IFMT {
        S_IFREG => {
            if st.size == 0 {
                "regular empty file"
            } else {
                "regular file"
            }
        }
        S_IFDIR => "directory",
        S_IFBLK => "block special file",
        S_IFCHR => "character special file",
        S_IFIFO => "fifo",
        S_IFLNK => "symbolic link",
        S_IFSOCK => "socket",
        _ => "weird file",
    }
}

/// `desc`: where a file's bytes come from, or why they do not.
#[derive(Debug)]
pub enum Desc {
    /// `NONEXISTENT`: absent, and with `-N`/`-P` read as empty.
    Nonexistent,
    /// `UNOPENED`: stat'ed but not yet opened.
    Unopened,
    /// The error the stat or open failed with.
    Errno(i32),
    /// Descriptor 0.
    Stdin,
    /// An opened file.
    File(std::fs::File),
    /// The other file's descriptor: the two operands are one file.
    Shared,
}

/// Reading that failed: the name to report and the error.
pub struct ReadError(pub std::io::Error);

/// `changed`: a flag per line, with a zero at -1 and past the end that the
/// scans run into instead of a bounds check.
#[derive(Clone, Debug, Default)]
pub struct Flags {
    v: Vec<u8>,
}

impl Flags {
    pub fn new(lines: Lin) -> Flags {
        let n = usize::try_from(lines).unwrap_or(0).saturating_add(3);
        Flags { v: vec![0; n] }
    }
    #[inline]
    fn slot(i: Lin) -> Option<usize> {
        usize::try_from(i.checked_add(1)?).ok()
    }
    #[inline]
    pub fn get(&self, i: Lin) -> bool {
        Self::slot(i)
            .and_then(|s| self.v.get(s))
            .is_some_and(|&b| b != 0)
    }
    #[inline]
    pub fn set(&mut self, i: Lin, on: bool) {
        if let Some(b) = Self::slot(i).and_then(|s| self.v.get_mut(s)) {
            *b = u8::from(on);
        }
    }
}

/// `struct file_data`.
#[derive(Debug)]
pub struct FileData {
    pub desc: Desc,
    pub name: Vec<u8>,
    pub stat: Stat,
    /// The bytes, once read: possibly with CRs stripped and a newline
    /// appended. Shared with the other file when the two are one.
    pub buffer: std::rc::Rc<Vec<u8>>,
    pub missing_newline: bool,
    pub eof: bool,
    /// Line starts for lines `linbuf_base ..= valid_lines`, stored from 0.
    linbuf: Vec<usize>,
    pub linbuf_base: Lin,
    pub buffered_lines: Lin,
    pub valid_lines: Lin,
    pub prefix_end: usize,
    pub prefix_lines: Lin,
    pub suffix_begin: usize,
    pub equivs: Vec<Lin>,
    pub undiscarded: Vec<Lin>,
    pub realindexes: Vec<Lin>,
    pub nondiscarded_lines: Lin,
    pub changed: Flags,
    pub equiv_max: Lin,
}

impl FileData {
    pub fn new(desc: Desc, name: Vec<u8>) -> FileData {
        FileData {
            desc,
            name,
            stat: Stat::default(),
            buffer: std::rc::Rc::new(Vec::new()),
            missing_newline: false,
            eof: false,
            linbuf: Vec::new(),
            linbuf_base: 0,
            buffered_lines: 0,
            valid_lines: 0,
            prefix_end: 0,
            prefix_lines: 0,
            suffix_begin: 0,
            equivs: Vec::new(),
            undiscarded: Vec::new(),
            realindexes: Vec::new(),
            nondiscarded_lines: 0,
            changed: Flags::default(),
            equiv_max: 0,
        }
    }

    /// `linbuf[i]`: where line `i` starts in [`FileData::buffer`].
    pub fn line_start(&self, i: Lin) -> usize {
        i.checked_sub(self.linbuf_base)
            .and_then(|k| usize::try_from(k).ok())
            .and_then(|k| self.linbuf.get(k))
            .copied()
            .unwrap_or(self.buffer.len())
    }

    /// Line `i`, from its start to the next line's: its newline included,
    /// unless it is a last line whose newline was supplied.
    pub fn line(&self, i: Lin) -> &[u8] {
        let from = self.line_start(i);
        let to = self.line_start(i.saturating_add(1));
        self.buffer.get(from..to.max(from)).unwrap_or_default()
    }

    /// `equivs[i]`.
    pub fn equiv(&self, i: Lin) -> Lin {
        usize::try_from(i)
            .ok()
            .and_then(|k| self.equivs.get(k))
            .copied()
            .unwrap_or(0)
    }
}

/// Descriptor 0 as a reader.
struct Fd0;

impl Read for Fd0 {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        coreutils::stdfd::read(0, buf)
    }
}

/// `file_block_read`: up to `want` more bytes, noting the end.
///
/// gnulib's `block_read` reads until it has `want` bytes or the end, which
/// is `read_to_end` on a `take`: that also retries an interrupted read, keeps
/// what came before a failed one, and reads a file into the buffer without
/// zeroing it first.
fn block_read(file: &mut FileData, buf: &mut Vec<u8>, want: usize) -> Result<(), ReadError> {
    if want == 0 || file.eof {
        return Ok(());
    }
    let start = buf.len();
    let limit = u64::try_from(want).unwrap_or(u64::MAX);
    let r = match &mut file.desc {
        Desc::Stdin => Fd0.take(limit).read_to_end(buf),
        Desc::File(f) => f.take(limit).read_to_end(buf),
        _ => Ok(0),
    };
    r.map_err(ReadError)?;
    file.eof = buf.len().saturating_sub(start) < want;
    Ok(())
}

/// `sip`: get ready to read, and -- unless `skip_test` -- read the first
/// block and say whether it holds a NUL, which makes the file binary.
fn sip(file: &mut FileData, buf: &mut Vec<u8>, skip_test: bool) -> Result<bool, ReadError> {
    if matches!(file.desc, Desc::Nonexistent) {
        return Ok(false);
    }
    if !skip_test {
        let bufsize = lcm(std::mem::size_of::<usize>(), file.stat.blocksize());
        block_read(file, buf, bufsize)?;
        return Ok(buf.contains(&0));
    }
    file.eof = false;
    Ok(false)
}

fn gcd(a: usize, b: usize) -> usize {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = a.checked_rem(b).unwrap_or(0);
        a = b;
        b = t;
    }
    a
}

/// `buffer_lcm`, without its overflow fallback: block sizes are small.
fn lcm(a: usize, b: usize) -> usize {
    let g = gcd(a, b).max(1);
    a.checked_div(g).unwrap_or(1).saturating_mul(b)
}

/// `slurp`: the rest of the file -- everything there is, to the end: a
/// regular file's size is only a guess, since the file may be growing.
fn slurp(file: &mut FileData, buf: &mut Vec<u8>) -> Result<(), ReadError> {
    if matches!(file.desc, Desc::Nonexistent) || file.eof {
        return Ok(());
    }
    let r = match &mut file.desc {
        Desc::Stdin => Fd0.read_to_end(buf),
        Desc::File(f) => f.read_to_end(buf),
        _ => Ok(0),
    };
    r.map_err(ReadError)?;
    file.eof = true;
    Ok(())
}

/// `prepare_text`: strip CRs before newlines if asked, and end the text with a
/// newline, remembering one was added.
fn prepare_text(file: &mut FileData, buf: &mut Vec<u8>, strip_trailing_cr: bool) {
    if strip_trailing_cr {
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0usize;
        while let Some(&b) = buf.get(i) {
            if b == b'\r' && buf.get(i.saturating_add(1)) == Some(&b'\n') {
                i = i.saturating_add(1);
                continue;
            }
            out.push(b);
            i = i.saturating_add(1);
        }
        *buf = out;
    }
    if buf.last().is_some_and(|&b| b != b'\n') {
        buf.push(b'\n');
        file.missing_newline = true;
    }
}

/// `read_files`: read both files and class their lines. True if either looks
/// binary (or `pretend_binary`), in which case nothing is classed.
pub fn read_files(
    files: &mut [FileData; 2],
    pretend_binary: bool,
    o: &Opts,
) -> Result<bool, (usize, ReadError)> {
    let skip_test = o.text || pretend_binary;
    let shared = matches!(files[1].desc, Desc::Shared);
    let mut bufs: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    let mut appears_binary = pretend_binary;
    {
        let [f0, f1] = files;
        let [b0, b1] = &mut bufs;
        appears_binary |= sip(f0, b0, skip_test).map_err(|e| (0, e))?;
        if !shared {
            appears_binary |= sip(f1, b1, skip_test || appears_binary).map_err(|e| (1, e))?;
        }
    }
    if appears_binary {
        let [b0, b1] = bufs;
        let b0 = std::rc::Rc::new(b0);
        files[0].buffer = b0.clone();
        files[1].buffer = if shared { b0 } else { std::rc::Rc::new(b1) };
        return Ok(true);
    }
    find_identical_ends(files, bufs, shared, o)?;
    classify(files, o);
    Ok(false)
}

/// `read_files`' binary path needs the rest of each file in blocks; this is
/// `diff_2_files`' loop, comparing the two a block at a time.
pub fn binary_differ(files: &mut [FileData; 2]) -> Result<bool, (usize, ReadError)> {
    let shared = matches!(files[1].desc, Desc::Shared);
    if shared {
        return Ok(false);
    }
    let size = lcm(
        std::mem::size_of::<usize>(),
        lcm(files[0].stat.blocksize(), files[1].stat.blocksize()),
    );
    let mut bufs = [
        std::rc::Rc::try_unwrap(std::mem::take(&mut files[0].buffer)).unwrap_or_default(),
        std::rc::Rc::try_unwrap(std::mem::take(&mut files[1].buffer)).unwrap_or_default(),
    ];
    loop {
        for (f, (file, buf)) in files.iter_mut().zip(bufs.iter_mut()).enumerate() {
            let want = size.saturating_sub(buf.len());
            if !matches!(file.desc, Desc::Nonexistent) {
                block_read(file, buf, want).map_err(|e| (f, e))?;
            }
        }
        if bufs[0] != bufs[1] {
            return Ok(true);
        }
        if bufs[0].len() != size {
            return Ok(false);
        }
        bufs[0].clear();
        bufs[1].clear();
    }
}

/// `find_identical_ends`: slurp both files, then find the common prefix and
/// suffix, keep `horizon_lines` of each, and lay out the line table.
fn find_identical_ends(
    files: &mut [FileData; 2],
    bufs: [Vec<u8>; 2],
    shared: bool,
    o: &Opts,
) -> Result<(), (usize, ReadError)> {
    let [mut b0, mut b1] = bufs;
    slurp(&mut files[0], &mut b0).map_err(|e| (0, e))?;
    prepare_text(&mut files[0], &mut b0, o.strip_trailing_cr);
    if shared {
        files[1].missing_newline = files[0].missing_newline;
        b1 = b0.clone();
    } else {
        slurp(&mut files[1], &mut b1).map_err(|e| (1, e))?;
        prepare_text(&mut files[1], &mut b1, o.strip_trailing_cr);
    }
    let robust = robust_output_style(o.output_style);
    let n0 = b0.len();
    let n1 = b1.len();

    // The identical prefix: the first mismatch, or the end of the shorter.
    let (mut p0, mut p1);
    if shared {
        p0 = n1;
        p1 = n1;
    } else {
        let k = b0
            .iter()
            .zip(b1.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(n0.min(n1));
        p0 = k;
        p1 = k;
        // Don't count a missing newline as part of the prefix.
        if robust
            && ((n0.saturating_sub(usize::from(files[0].missing_newline)) < p0)
                != (n1.saturating_sub(usize::from(files[1].missing_newline)) < p1))
        {
            p0 = p0.saturating_sub(1);
            p1 = p1.saturating_sub(1);
        }
    }

    // Back to the start of a line, then back `horizon_lines` more lines.
    let mut i = o.horizon_lines;
    while p0 != 0 {
        if b0.get(p0.saturating_sub(1)) == Some(&b'\n') {
            if i == 0 {
                break;
            }
            i = i.saturating_sub(1);
        }
        p0 = p0.saturating_sub(1);
        p1 = p1.saturating_sub(1);
    }
    files[0].prefix_end = p0;
    files[1].prefix_end = p1;

    // The identical suffix.
    let mut s0 = n0;
    let mut s1 = n1;
    if !robust || files[0].missing_newline == files[1].missing_newline {
        let end0 = n0;
        // Stop where either file's scan would run into the prefix.
        let beg0 =
            files[0]
                .prefix_end
                .saturating_add(if n0 < n1 { 0 } else { n0.saturating_sub(n1) });
        while s0 != beg0 {
            s0 = s0.saturating_sub(1);
            s1 = s1.saturating_sub(1);
            if b0.get(s0) != b1.get(s1) {
                // Point at the first byte of the matching suffix.
                s0 = s0.saturating_add(1);
                s1 = s1.saturating_add(1);
                break;
            }
        }
        // Whichever way the scan ended, the suffix starts at `s0` now.
        let scanned0 = s0;
        // A suffix that starts mid-line gives the rest of that line to the
        // middle; then `horizon_lines` whole lines go too.
        let at_line_start =
            |b: &[u8], p: usize| p == 0 || b.get(p.saturating_sub(1)) == Some(&b'\n');
        let mut i = o.horizon_lines.saturating_add(isize::from(
            !(at_line_start(&b0, s0) && at_line_start(&b1, s1)),
        ));
        while i > 0 && s0 != end0 {
            i = i.saturating_sub(1);
            loop {
                let b = b0.get(s0).copied();
                s0 = s0.saturating_add(1);
                if b == Some(b'\n') || b.is_none() {
                    break;
                }
            }
        }
        s1 = s1.saturating_add(s0.saturating_sub(scanned0));
    }
    files[0].suffix_begin = s0;
    files[1].suffix_begin = s1;

    // The prefix lines worth keeping: all of them, or only the last `context`
    // when nothing before them will be printed.
    let prefix_needed = !(o.no_diff_means_no_output
        && files[0].prefix_end == files[0].suffix_begin
        && files[1].prefix_end == files[1].suffix_begin);
    let mut starts0: Vec<usize> = Vec::new();
    if prefix_needed {
        let mut p = 0usize;
        while p != files[0].prefix_end {
            starts0.push(p);
            while b0.get(p).is_some_and(|&b| b != b'\n') {
                p = p.saturating_add(1);
            }
            p = p.saturating_add(1);
        }
    }
    let lines = isize::try_from(starts0.len()).unwrap_or(isize::MAX);
    let save_all = !(o.no_diff_means_no_output
        && o.function_regexp.is_none()
        && o.context < isize::MAX / 4
        && usize::try_from(o.context).is_ok_and(|c| c < n0));
    let buffered_prefix = if !save_all && o.context < lines {
        o.context
    } else {
        lines
    };
    let keep = usize::try_from(buffered_prefix).unwrap_or(0);
    let first_kept = starts0.len().saturating_sub(keep);
    // The prefix is the same bytes at the same offsets in both files, so its
    // line starts serve both.
    let kept: Vec<usize> = starts0.get(first_kept..).unwrap_or_default().to_vec();
    let b0 = std::rc::Rc::new(b0);
    let b1 = if shared {
        b0.clone()
    } else {
        std::rc::Rc::new(b1)
    };
    for (file, buf) in files.iter_mut().zip([b0, b1]) {
        file.linbuf.clone_from(&kept);
        file.linbuf_base = buffered_prefix.saturating_neg();
        file.prefix_lines = lines;
        file.buffer = buf;
    }
    Ok(())
}

/// `robust_output_style`: everything but the two ed formats can say that a
/// file does not end in a newline.
pub fn robust_output_style(s: OutputStyle) -> bool {
    !matches!(s, OutputStyle::Ed | OutputStyle::ForwardEd)
}

/// The hash `find_and_hash_each_line` gives a line: `HASH (h, c)` is
/// `c + ROL (h, 7)` on a `size_t`.
fn hash_step(h: usize, c: u8) -> usize {
    usize::from(c).wrapping_add(h.rotate_left(7))
}

/// C's `isspace` in the C and C.UTF-8 locales.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `find_and_hash_each_line` for both files: one equivalence class per set of
/// lines the options call equal, numbered from 1 in order of appearance.
fn classify(files: &mut [FileData; 2], o: &Opts) {
    // Both buffers, held apart from `files`: a class made from one file's
    // line is read again while the other file is being classed.
    let bufs = [files[0].buffer.clone(), files[1].buffer.clone()];
    // Upstream sizes its tables from a guess of 32 bytes a line, as this
    // does; here the buckets double whenever the classes outnumber them.
    let middle = |file: &FileData| file.suffix_begin.saturating_sub(file.prefix_end);
    let guess = middle(&files[0])
        .saturating_add(middle(&files[1]))
        .checked_div(32)
        .unwrap_or(0)
        .saturating_add(5);
    // Class 0 is for the lines that are not hashed; real classes start at 1.
    let mut eqs: Vec<EquivClass> = Vec::with_capacity(guess.saturating_add(1));
    eqs.push(EquivClass::default());
    let mut buckets = Buckets::new(guess);
    // Upstream's `buckets[-1]`: the chain of incomplete last lines.
    let mut apart_head = 0usize;

    let robust = robust_output_style(o.output_style);
    let diff_length_compare_anyway = o.ignore_white_space != WhiteSpace::None;
    let same_length_anyway = diff_length_compare_anyway || o.ignore_case;
    for ((f, file), buf) in files.iter_mut().enumerate().zip(&bufs) {
        let buf: &[u8] = buf;
        let suffix_begin = file.suffix_begin;
        let bufend = buf.len();
        // The last line, when it is incomplete and not silently completed,
        // can equal only the other file's incomplete line.
        let incomplete_apart =
            file.missing_newline && robust && o.ignore_white_space < WhiteSpace::TrailingSpace;
        let mut p = file.prefix_end;
        let mut equivs: Vec<Lin> = Vec::new();
        let mut line = 0isize;
        while p < suffix_begin {
            let ip = p;
            let (h, after) = hash_line(buf, p, o);
            p = after;
            let length = p.saturating_sub(ip).saturating_sub(1);
            let apart = p == bufend && incomplete_apart;
            let head = if apart { apart_head } else { buckets.head(h) };
            // Walk the chain newest first, for a class that takes the line.
            let text = buf.get(ip..).unwrap_or_default();
            let mut i = head;
            let class = loop {
                let Some(e) = eqs.get(i).filter(|_| i != 0) else {
                    // None does: a new class, at the head of the chain.
                    let c = eqs.len();
                    eqs.push(EquivClass {
                        next: head,
                        hash: h,
                        start: ip,
                        length,
                        file: f,
                        apart,
                    });
                    if apart {
                        apart_head = c;
                    } else {
                        buckets.set_head(h, c);
                        if buckets.len() < eqs.len() {
                            buckets.grow(&mut eqs);
                        }
                    }
                    break c;
                };
                if e.hash == h {
                    let eqline = bufs
                        .get(e.file)
                        .and_then(|b| b.get(e.start..))
                        .unwrap_or_default();
                    let compare = if e.length == length {
                        // Identical is the common case, and cheaper to see
                        // than `lines_differ`'s rules.
                        if eqline.get(..length) == text.get(..length) {
                            break i;
                        }
                        same_length_anyway
                    } else {
                        diff_length_compare_anyway
                    };
                    // Both run on past their newlines, where `lines_differ`
                    // stops.
                    if compare && !lines_differ(eqline, text, o) {
                        break i;
                    }
                }
                i = e.next;
            };
            file.linbuf.push(ip);
            equivs.push(isize::try_from(class).unwrap_or(isize::MAX));
            line = line.saturating_add(1);
        }
        file.buffered_lines = line;
        // The suffix lines worth keeping, and one past the last, so that every
        // kept line's length is known.
        let mut i = 0isize;
        loop {
            file.linbuf.push(p);
            if p == bufend {
                if file.missing_newline && robust_output_style(o.output_style) {
                    if let Some(last) = file.linbuf.last_mut() {
                        *last = last.saturating_sub(1);
                    }
                }
                break;
            }
            if o.context <= i && o.no_diff_means_no_output {
                break;
            }
            line = line.saturating_add(1);
            while buf.get(p).is_some_and(|&b| b != b'\n') {
                p = p.saturating_add(1);
            }
            p = p.saturating_add(1);
            i = i.saturating_add(1);
        }
        file.valid_lines = line;
        file.equivs = equivs;
    }
    let max = isize::try_from(eqs.len()).unwrap_or(isize::MAX);
    files[0].equiv_max = max;
    files[1].equiv_max = max;
}

/// `struct equivclass`: one class of lines, chained through its bucket.
#[derive(Clone, Copy, Default)]
struct EquivClass {
    /// `next`: the class made before this one in the same chain; 0 ends it,
    /// so a chain is walked newest first, as upstream walks it.
    next: usize,
    /// `hash`: what every line of the class hashes to.
    hash: usize,
    /// Upstream's `line` and `length`: the line the class was made from, as
    /// where it starts in its file's buffer and its length without the
    /// newline. An offset, not a copy: both buffers outlive the classing,
    /// which upstream's pointer into them relies on too.
    start: usize,
    length: usize,
    /// Which file's buffer `start` is in.
    file: usize,
    /// Made from an incomplete last line, and so chained apart from the
    /// buckets, in upstream's `buckets[-1]`.
    apart: bool,
}

/// Upstream's `buckets`: where each chain of classes starts, by hash.
///
/// Upstream takes `h % nbuckets` over a prime number of buckets, sized once
/// from a guess at the line count. Which bucket a hash lands in decides
/// nothing but speed -- all the classes of one hash share a bucket whatever
/// the function, in the same newest-first order -- so this spreads the hash
/// with a multiply instead of dividing once per line, and doubles the table
/// when a wrong guess has left it too small.
struct Buckets {
    heads: Vec<usize>,
    /// 64 less the number of bits in a bucket index.
    shift: u32,
}

impl Buckets {
    /// A table with at least `classes` buckets.
    fn new(classes: usize) -> Buckets {
        let bits = usize::BITS
            .saturating_sub(classes.leading_zeros())
            .clamp(9, usize::BITS.saturating_sub(1));
        Buckets::with_bits(bits)
    }

    fn with_bits(bits: u32) -> Buckets {
        Buckets {
            heads: vec![0; 1usize.checked_shl(bits).unwrap_or(1)],
            shift: u64::BITS.saturating_sub(bits),
        }
    }

    fn len(&self) -> usize {
        self.heads.len()
    }

    fn slot(&self, h: usize) -> usize {
        let spread = (h as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        usize::try_from(spread.checked_shr(self.shift).unwrap_or(0)).unwrap_or(0)
    }

    /// The newest class whose hash shares `h`'s bucket, or 0.
    fn head(&self, h: usize) -> usize {
        self.heads.get(self.slot(h)).copied().unwrap_or(0)
    }

    fn set_head(&mut self, h: usize, class: usize) {
        let s = self.slot(h);
        if let Some(head) = self.heads.get_mut(s) {
            *head = class;
        }
    }

    /// Twice the buckets, every chain relinked in the order its classes were
    /// made -- so each is still newest first -- and the incomplete lines'
    /// chain left alone.
    fn grow(&mut self, eqs: &mut [EquivClass]) {
        let bits = u64::BITS
            .saturating_sub(self.shift)
            .saturating_add(1)
            .min(usize::BITS.saturating_sub(1));
        *self = Buckets::with_bits(bits);
        for (i, e) in eqs.iter_mut().enumerate().skip(1) {
            if !e.apart {
                e.next = self.head(e.hash);
                self.set_head(e.hash, i);
            }
        }
    }
}

/// Hash the line starting at `p` under the white-space rules; return the hash
/// and the position just past its newline -- or, for a line without one, one
/// past the end, where upstream's sentinel newline would be.
fn hash_line(buf: &[u8], p: usize, o: &Opts) -> (usize, usize) {
    let rest = buf.get(p..).unwrap_or_default();
    let mut h = 0usize;
    if o.ignore_white_space == WhiteSpace::None && !o.ignore_case {
        // The usual case, in the one pass upstream's loop makes.
        let mut bytes = rest.iter();
        let mut newline = false;
        for &c in bytes.by_ref() {
            if c == b'\n' {
                newline = true;
                break;
            }
            h = hash_step(h, c);
        }
        let used = rest.len().saturating_sub(bytes.as_slice().len());
        return (
            h,
            p.saturating_add(used).saturating_add(usize::from(!newline)),
        );
    }
    let length = rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
    let line = rest.get(..length).unwrap_or_default();
    let next = p.saturating_add(length).saturating_add(1);
    let ig_case = o.ignore_case;
    let low = |c: u8| if ig_case { c.to_ascii_lowercase() } else { c };
    match o.ignore_white_space {
        WhiteSpace::None => {
            for &c in line {
                h = hash_step(h, low(c));
            }
        }
        WhiteSpace::AllSpace => {
            for &c in line {
                if !is_space(c) {
                    h = hash_step(h, low(c));
                }
            }
        }
        WhiteSpace::SpaceChange => {
            // A run of white space hashes as one space, or as nothing at the
            // end of the line.
            let mut bytes = line.iter().copied();
            'line: while let Some(mut c) = bytes.next() {
                if is_space(c) {
                    loop {
                        match bytes.next() {
                            None => break 'line,
                            Some(c1) if is_space(c1) => {}
                            Some(c1) => {
                                c = c1;
                                break;
                            }
                        }
                    }
                    h = hash_step(h, b' ');
                }
                h = hash_step(h, low(c));
            }
        }
        WhiteSpace::TabExpansion
        | WhiteSpace::TabExpansionAndTrailingSpace
        | WhiteSpace::TrailingSpace => {
            let mut column = 0usize;
            let tabsize = o.tabsize.max(1);
            let mut bytes = line.iter();
            while let Some(&c0) = bytes.next() {
                let mut c = c0;
                // White space with nothing else after it ends the line.
                if o.ignore_white_space.trailing()
                    && is_space(c)
                    && bytes.as_slice().iter().all(|&c1| is_space(c1))
                {
                    break;
                }
                let mut repetitions = 1usize;
                if o.ignore_white_space.tab_expansion() {
                    match c {
                        0x08 => column = column.saturating_sub(usize::from(column > 0)),
                        b'\t' => {
                            c = b' ';
                            repetitions =
                                tabsize.saturating_sub(column.checked_rem(tabsize).unwrap_or(0));
                            column = column.checked_add(repetitions).unwrap_or(0);
                        }
                        b'\r' => column = 0,
                        _ => column = column.saturating_add(1),
                    }
                }
                let c = low(c);
                for _ in 0..repetitions {
                    h = hash_step(h, c);
                }
            }
        }
    }
    (h, next)
}

/// Load two in-memory texts as `read_files` would load two files, for the
/// unit tests.
#[cfg(test)]
pub fn test_load(files: &mut [FileData; 2], a: &[u8], b: &[u8], o: &Opts) {
    for f in files.iter_mut() {
        f.eof = true;
    }
    if find_identical_ends(files, [a.to_vec(), b.to_vec()], false, o).is_ok() {
        classify(files, o);
    }
}
