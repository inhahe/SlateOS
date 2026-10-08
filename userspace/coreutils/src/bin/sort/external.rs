//! Sorting more than fits in memory, as upstream does it: the input read a
//! buffer at a time, each buffer sorted and written to a temporary file, and
//! the temporary files merged `--batch-size` at a time into the output; and
//! `-m`'s merge of already-sorted inputs, by the same merge.
//!
//! # What is upstream's here, and why each part is
//!
//! The output never depends on where the input was cut: a merge of sorted
//! runs is the sorted whole, ties going to the earlier run, so `-s` and `-u`
//! come out as a sort of the whole would have them. What *is* observable is
//! **whether and when a temporary file is made** -- `-T` naming a directory
//! that does not exist is an error only then, and `--compress-program` runs
//! once for each -- so the buffer is upstream's to the byte:
//!
//! * its size is `sort_buffer_size`'s, from the inputs' sizes, `-S` and the
//!   machine's memory and limits (`default_sort_size`);
//! * it is filled as `fillbuf` fills it: `readsize` bytes at a time, each line
//!   charged `line_bytes` of the buffer for upstream's `struct line` and the
//!   merge-tree room `sort` keeps beside it -- 48 bytes with one thread, more
//!   with more, which is why the thread count matters here at all;
//! * a line longer than the buffer grows it, as `x2nrealloc` grows it;
//! * a buffer that ends with an input file and has room left takes in the
//!   next file rather than being written out.
//!
//! None of that allocates the size it accounts for. Upstream `malloc`s the
//! whole buffer, which on Linux costs nothing until it is touched; SlateOS
//! commits memory when it is allocated (design.txt: no silent overcommit),
//! where a buffer forty-nine times the input would be real. So the buffer
//! here holds only the bytes read and a 16-byte record per line, and keeps
//! upstream's arithmetic beside them.
//!
//! The temporary files are `mkostemp_safer`'s: `DIR/sortXXXXXX`, opened
//! exclusively, never on descriptor 0, 1 or 2, in the `-T` directories in
//! turn (else `$TMPDIR`, else `/tmp`). Each is registered as it is made and
//! removed as soon as a merge has read it; on any exit -- and on the signals
//! upstream catches, which are caught here for that alone -- whatever is
//! still registered is removed.
//!
//! The merge is `merge` and `mergefps`: passes of `--batch-size` files into
//! new temporary files until one pass can reach the output, ties to the
//! earlier file, `-u` keeping the first of each run of equal lines.

use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use coreutils::cleanup;
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::quote::{os_bytes, quoteaf_os, quotef_os};
use coreutils::stdfd;
use coreutils::stdio::StdioFile;

use crate::limits::{self, LINE};
use crate::{Config, die};

// ── the buffer ──────────────────────────────────────────────────────────────

/// `INPUT_FILE_SIZE_GUESS`: what an input of unknown size is taken to hold.
const INPUT_FILE_SIZE_GUESS: usize = 128 * 1024;

/// One buffer of input lines, accounted as upstream's `struct buffer` is.
///
/// `text` holds the bytes read, every line with its terminator; `lines` where
/// each complete line starts and how long it is, terminator included. The
/// last `left` bytes of `text` are a line not yet complete, carried into the
/// next fill. `alloc` and `line_bytes` are upstream's numbers, used only in
/// its arithmetic: see the module documentation for why they are not
/// allocated.
pub struct Buffer {
    text: Vec<u8>,
    lines: Vec<(usize, usize)>,
    alloc: usize,
    line_bytes: usize,
    left: usize,
    eof: bool,
}

impl Buffer {
    /// Upstream's `initbuf`: `alloc` rounded up to a whole `struct line` --
    /// a whole one more when it is one already, as upstream's arithmetic has
    /// it.
    fn new(line_bytes: usize, alloc: usize) -> Self {
        let alloc = alloc.saturating_add(LINE.saturating_sub(alloc % LINE));
        Buffer {
            text: Vec::new(),
            lines: Vec::new(),
            alloc,
            line_bytes,
            left: 0,
            eof: false,
        }
    }

    /// The lines filled, terminators cut off.
    fn line(&self, index: usize) -> &[u8] {
        self.lines.get(index).map_or(&[][..], |&(start, len)| {
            self.text
                .get(start..start.saturating_add(len).saturating_sub(1))
                .unwrap_or_default()
        })
    }

    /// What is left of `alloc` for text and lines: upstream's `avail`.
    fn avail(&self) -> usize {
        self.alloc
            .saturating_sub(self.text.len())
            .saturating_sub(self.lines.len().saturating_mul(self.line_bytes))
    }

    /// Upstream's `fillbuf`: read until the buffer holds as many lines as its
    /// accounting allows, or the input ends. `false` when there was nothing
    /// to read at all.
    ///
    /// # Errors
    ///
    /// A failed read, which upstream reports and dies of.
    fn fill(&mut self, input: &mut dyn Read, eol: u8) -> io::Result<bool> {
        if self.eof {
            return Ok(false);
        }
        if self.text.len() != self.left {
            // Keep only the unfinished line, moved to the front.
            let keep_from = self.text.len().saturating_sub(self.left);
            self.text.drain(..keep_from);
            self.lines.clear();
        }
        loop {
            let mut ptr = self.text.len();
            let mut line_start = self
                .lines
                .last()
                .map_or(0, |&(start, len)| start.saturating_add(len));
            let per_byte = self.line_bytes.saturating_add(1);
            while per_byte < self.avail() {
                // "Do not read so many bytes that there might not be enough
                // room for the corresponding line array."
                let readsize = self
                    .avail()
                    .saturating_sub(1)
                    .checked_div(per_byte)
                    .unwrap_or(0);
                let got = read_up_to(input, &mut self.text, readsize)?;
                if got != readsize {
                    self.eof = true;
                    if self.text.is_empty() {
                        return Ok(false);
                    }
                    if line_start != self.text.len() && self.text.last() != Some(&eol) {
                        self.text.push(eol);
                    }
                }
                let ptrlim = self.text.len();
                while let Some(offset) = self
                    .text
                    .get(ptr..ptrlim)
                    .and_then(|chunk| chunk.iter().position(|&b| b == eol))
                {
                    ptr = ptr.saturating_add(offset).saturating_add(1);
                    self.lines
                        .push((line_start, ptr.saturating_sub(line_start)));
                    line_start = ptr;
                }
                ptr = ptrlim;
                if self.eof {
                    break;
                }
            }
            if !self.lines.is_empty() {
                self.left = ptr.saturating_sub(line_start);
                return Ok(true);
            }
            // "The current input line is too long to fit in the buffer":
            // `x2nrealloc` in `struct line`s, half as many again and one.
            let lines = self.alloc / LINE;
            self.alloc = lines
                .saturating_add(lines / 2)
                .saturating_add(1)
                .saturating_mul(LINE);
        }
    }
}

/// `fread (ptr, 1, n, fp)`: append up to `n` bytes, fewer only at the end of
/// the input.
fn read_up_to(input: &mut dyn Read, into: &mut Vec<u8>, n: usize) -> io::Result<usize> {
    let start = into.len();
    into.resize(start.saturating_add(n), 0);
    let mut got = 0usize;
    while got < n {
        let slot = into
            .get_mut(start.saturating_add(got)..start.saturating_add(n))
            .unwrap_or_default();
        match input.read(slot) {
            Ok(0) => break,
            Ok(read) => got = got.saturating_add(read),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                into.truncate(start.saturating_add(got));
                return Err(e);
            }
        }
    }
    into.truncate(start.saturating_add(got));
    Ok(got)
}

/// `sizeof (struct line)` per line, as upstream charges a buffer: half as
/// much again with one thread, and with `n` threads one `struct line` for
/// each level of the merge tree `n` threads build, plus one.
fn bytes_per_line(nthreads: usize) -> usize {
    if nthreads <= 1 {
        return LINE * 3 / 2;
    }
    let mut levels = 1usize;
    let mut tmp = 1usize;
    while tmp < nthreads {
        tmp = tmp.saturating_mul(2);
        levels = levels.saturating_add(1);
    }
    levels.saturating_mul(LINE)
}

/// The thread count upstream sorts with: `--parallel`'s, else the processing
/// units this process may use, at most eight.
fn threads(cfg: &Config) -> usize {
    let n = if cfg.nthreads == 0 {
        let np = coreutils::nproc::now(coreutils::nproc::Query::CurrentOverridable);
        usize::try_from(np.min(limits::DEFAULT_MAX_THREADS)).unwrap_or(1)
    } else {
        cfg.nthreads
    };
    // "Avoid integer overflow later": SIZE_MAX / (2 * sizeof (struct
    // merge_node)), a merge node being eleven pointers' worth.
    n.min(usize::MAX / (2 * 88))
}

/// Upstream's `default_sort_size`: the most the buffer may take when `-S`
/// does not say -- half the data and address-space limits, fifteen
/// sixteenths of the resident limit, three quarters of physical memory, and
/// no more than what is free or an eighth of the total, whichever is more.
fn default_sort_size() -> usize {
    let mut size = usize::MAX;
    for resource in [limits::Resource::Data, limits::Resource::AddressSpace] {
        if let Some(limit) = limits::rlimit(resource).and_then(|l| usize::try_from(l).ok())
            && limit < size
        {
            size = limit;
        }
    }
    size /= 2;
    if let Some(rss) = limits::rlimit(limits::Resource::Rss).and_then(|l| usize::try_from(l).ok()) {
        let margin = (rss / 16).saturating_mul(15);
        if margin < size {
            size = margin;
        }
    }
    let avail = limits::physmem_available();
    let total = limits::physmem_total();
    let mem = avail.max(total / 8.0);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )] // upstream's doubles
    {
        if total * 0.75 < size as f64 {
            size = (total * 0.75) as usize;
        }
        if mem < size as f64 {
            size = mem as usize;
        }
    }
    size.max(limits::min_sort_size(limits::NMERGE_DEFAULT))
}

/// What `stat` says of an input: a regular file's size, or `None` for
/// anything else.
fn regular_size(meta: &std::fs::Metadata) -> Option<usize> {
    meta.file_type()
        .is_file()
        .then(|| usize::try_from(meta.len()).unwrap_or(usize::MAX))
}

/// Upstream's `sort_buffer_size`: enough for the worst case of every input
/// remaining -- each byte a line -- or `-S` / the default, whichever is less.
/// The first input is open (`first`); the rest are asked by name, `-` by
/// `fstat (0)`. Dies, as upstream does, of an input it cannot `stat`.
fn sort_buffer_size(
    cfg: &Config,
    first: &std::fs::Metadata,
    files: &[OsString],
    line_bytes: usize,
) -> usize {
    let per_byte = line_bytes.saturating_add(1);
    let mut size = per_byte.saturating_add(1);
    let mut size_bound = 0usize;
    for (n, name) in files.iter().enumerate() {
        let meta = if n == 0 {
            Ok(first.clone())
        } else if name == "-" {
            stdfd::metadata(0)
        } else {
            std::fs::metadata(name)
        };
        let meta = meta.unwrap_or_else(|e| {
            die(&format!(
                "stat failed: {}: {}",
                quotef_os(name),
                strerror(&e)
            ))
        });
        let file_size = match regular_size(&meta) {
            Some(bytes) => bytes,
            None => {
                if cfg.sort_size != 0 {
                    return cfg.sort_size;
                }
                INPUT_FILE_SIZE_GUESS
            }
        };
        if size_bound == 0 {
            size_bound = if cfg.sort_size == 0 {
                default_sort_size()
            } else {
                cfg.sort_size
            };
        }
        let worst = file_size
            .checked_mul(per_byte)
            .and_then(|w| w.checked_add(1));
        match worst {
            Some(worst) if size_bound.saturating_sub(size) > worst => {
                size = size.saturating_add(worst);
            }
            _ => return size_bound,
        }
    }
    size
}

// ── inputs ──────────────────────────────────────────────────────────────────

/// An input being read: descriptor 0 itself, as upstream's `stdin` reads it
/// (a closed one is an error, not an empty input); a file; or what a
/// decompressor makes of a temporary file.
pub enum Input {
    Stdin,
    File(File),
    Decompressed(ChildStdout),
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Input::Stdin => stdfd::RawStdin.read(buf),
            Input::File(file) => file.read(buf),
            Input::Decompressed(pipe) => pipe.read(buf),
        }
    }
}

/// Upstream's `stream_open (name, "r")`: `None` when it cannot be opened,
/// with why.
fn stream_open(name: &OsStr) -> Result<Input, io::Error> {
    if name == "-" {
        return Ok(Input::Stdin);
    }
    File::open(name).map(Input::File)
}

/// Upstream's `xfclose` for an input: standard input is left open (it may be
/// read again), as is a file that was opened onto descriptor 0, which
/// upstream's switch on the descriptor number cannot tell from it; anything
/// else is closed, and a failed close is the end of the run.
fn close_input(input: Input, name: &OsStr) {
    match input {
        Input::Stdin => {}
        Input::File(file) => {
            if on_descriptor_zero(&file) {
                // Kept open, deliberately, for the rest of the process.
                std::mem::forget(file);
            } else if let Err(e) = stdfd::close(file) {
                die(&format!(
                    "close failed: {}: {}",
                    quotef_os(name),
                    strerror(&e)
                ));
            }
        }
        Input::Decompressed(pipe) => drop(pipe),
    }
}

#[cfg(unix)]
fn on_descriptor_zero(file: &File) -> bool {
    use std::os::fd::AsRawFd;
    file.as_raw_fd() == 0
}

#[cfg(not(unix))]
fn on_descriptor_zero(_file: &File) -> bool {
    false
}

/// The `st_blksize` glibc sizes a stream's buffer by: how much of a shared
/// descriptor one `fread` takes, which is what two streams on descriptor 0
/// split between them.
#[cfg(unix)]
fn stdio_buffer_size(input: &Input) -> usize {
    use std::os::unix::fs::MetadataExt;
    let meta = match input {
        Input::Stdin => stdfd::metadata(0).ok(),
        Input::File(file) => file.metadata().ok(),
        Input::Decompressed(_) => None,
    };
    meta.map(|m| m.blksize())
        .and_then(|b| usize::try_from(b).ok())
        .filter(|&b| b > 0)
        .unwrap_or(4096)
}

#[cfg(not(unix))]
fn stdio_buffer_size(_input: &Input) -> usize {
    4096
}

/// Lines from one input, each without its terminator; a last line without
/// one is a line all the same, as `fillbuf` gives it its terminator.
struct Lines {
    reader: BufReader<Input>,
    eol: u8,
}

impl Lines {
    fn new(input: Input, eol: u8) -> Self {
        let size = stdio_buffer_size(&input);
        Lines {
            reader: BufReader::with_capacity(size, input),
            eol,
        }
    }

    fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        loop {
            match self.reader.read_until(self.eol, &mut line) {
                Ok(0) if line.is_empty() => return Ok(None),
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        if line.last() == Some(&self.eol) {
            line.pop();
        }
        Ok(Some(line))
    }

    fn into_input(self) -> Input {
        self.reader.into_inner()
    }
}

// ── the signals ─────────────────────────────────────────────────────────────

/// Upstream's `main`: catch the usual suspects, so that no temporary file is
/// left behind by a signal (`coreutils::cleanup`), and give `SIGCHLD` its
/// default action -- "Don't inherit CHLD handling from parent", which would
/// otherwise reap the compress program before it could be waited for.
pub fn install_signal_handlers() {
    cleanup::install();
    #[cfg(unix)]
    {
        // Unchecked, as upstream's `signal` call is.
        let _ = libcall::signal::set_default(libcall::signal::SIGCHLD);
    }
}

/// Remove whatever temporary files are left: upstream's `exit_cleanup`,
/// which every way out of `sort` passes through.
pub fn remove_all() {
    cleanup::remove_all();
}

// ── temporary files ──────────────────────────────────────────────────────────

/// One temporary file: where it is, and the compressor or decompressor
/// working on it that has not yet been waited for.
pub struct Temp {
    path: PathBuf,
    compressed: bool,
    child: Option<Child>,
}

/// Where the temporary files go, and the compress program.
pub struct Temps<'a> {
    dirs: Vec<OsString>,
    next: usize,
    compress: Option<&'a OsStr>,
}

impl<'a> Temps<'a> {
    pub fn new(cfg: &'a Config) -> Self {
        let dirs = if cfg.temp_dirs.is_empty() {
            // `TMPDIR`, even empty; else `/tmp`.
            vec![std::env::var_os("TMPDIR").unwrap_or_else(|| OsString::from("/tmp"))]
        } else {
            cfg.temp_dirs.clone()
        };
        Temps {
            dirs,
            next: 0,
            compress: cfg.compress_program.as_deref(),
        }
    }

    /// Upstream's `create_temp`: a new temporary file in the next directory,
    /// and where to write it -- through the compress program, if there is
    /// one. Dies, as upstream does, when no file can be made there.
    fn create(&mut self) -> (Temp, TempWriter) {
        let dir = self
            .dirs
            .get(self.next)
            .cloned()
            .unwrap_or_else(|| OsString::from("/tmp"));
        self.next = self.next.saturating_add(1);
        if self.next == self.dirs.len() {
            self.next = 0;
        }
        let (path, file) = make_temp_file(&dir).unwrap_or_else(|e| {
            die(&format!(
                "cannot create temporary file in {}: {}",
                quoteaf_os(&dir),
                strerror(&e)
            ))
        });
        let mut temp = Temp {
            path,
            compressed: false,
            child: None,
        };
        let Some(program) = self.compress else {
            return (temp, TempWriter::File(io::BufWriter::new(file)));
        };
        let spawned = cleanup::without(|| {
            Command::new(program)
                .stdin(Stdio::piped())
                .stdout(Stdio::from(file))
                .spawn()
        });
        match spawned {
            Ok(mut child) => match child.stdin.take() {
                Some(stdin) => {
                    temp.compressed = true;
                    temp.child = Some(child);
                    (temp, TempWriter::Compressor(io::BufWriter::new(stdin)))
                }
                None => abnormal(program),
            },
            // A fork that failed is upstream's cue to write the file as it
            // is; an exec that failed is its child's death.
            Err(e) if is_fork_failure(&e) => {
                let file = File::options()
                    .write(true)
                    .open(&temp.path)
                    .unwrap_or_else(|e| couldnt_create(&temp.path, &e));
                (temp, TempWriter::File(io::BufWriter::new(file)))
            }
            Err(e) => {
                child_died("couldn't execute compress program", &e);
                abnormal(program)
            }
        }
    }

    /// Upstream's `open_temp`: the temporary file to read, through the
    /// decompressor when it was compressed -- after its compressor has been
    /// waited for.
    ///
    /// # Errors
    ///
    /// The file cannot be opened.
    fn open(&self, temp: &mut Temp) -> io::Result<Input> {
        if !temp.compressed {
            return File::open(&temp.path).map(Input::File);
        }
        let program = self.compress.unwrap_or_default();
        if let Some(child) = temp.child.take() {
            wait(child, program);
        }
        let file = File::open(&temp.path)?;
        let spawned = cleanup::without(|| {
            Command::new(program)
                .arg("-d")
                .stdin(Stdio::from(file))
                .stdout(Stdio::piped())
                .spawn()
        });
        match spawned {
            Ok(mut child) => {
                let pipe = child.stdout.take();
                temp.child = Some(child);
                pipe.map(Input::Decompressed)
                    .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))
            }
            Err(e) if is_fork_failure(&e) => die(&format!(
                "couldn't create process for {} -d: {}",
                quoteaf_os(program),
                strerror(&e)
            )),
            Err(e) => {
                child_died("couldn't execute compress program (with -d)", &e);
                abnormal(program)
            }
        }
    }

    /// Upstream's `zaptemp`: wait for whoever is still working on the file,
    /// then remove it -- warning, not dying, when it cannot be.
    fn zap(&self, mut temp: Temp) {
        if let Some(child) = temp.child.take() {
            wait(child, self.compress.unwrap_or_default());
        }
        if let Err(e) = cleanup::remove(&temp.path) {
            diag!(
                "sort: warning: cannot remove: {}: {}",
                quotef_os(temp.path.as_os_str()),
                strerror(&e)
            );
        }
    }
}

/// `mkostemp_safer ("DIR/sortXXXXXX", O_CLOEXEC)`: a new file of six random
/// letters and digits, made exclusively, mode 0600, kept off descriptors
/// 0-2 -- and registered before anything can interrupt.
fn make_temp_file(dir: &OsStr) -> io::Result<(PathBuf, File)> {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut random = coreutils::randint::RandRead::open(None)?;
    let mut base = os_bytes(dir).into_owned();
    base.extend_from_slice(b"/sort");
    // glibc's `__gen_tempname` tries this many names before it gives up.
    for _ in 0..238_328u32 {
        let mut pick = [0u8; 6];
        if random.read(&mut pick).is_err() {
            return Err(io::Error::other(
                "the system random number generator is unavailable",
            ));
        }
        let mut name = base.clone();
        name.extend(pick.iter().map(|&b| {
            let at = usize::from(b).checked_rem(LETTERS.len()).unwrap_or(0);
            LETTERS.get(at).copied().unwrap_or(b'X')
        }));
        let path = PathBuf::from(crate::os_from_bytes(&name));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        // Registered in the same blocked section as it is made, so no signal
        // can arrive between the two and leave it behind.
        let made: io::Result<File> = cleanup::change(|list| {
            let file = options.open(&path)?;
            cleanup::add_to(list, &path);
            Ok(file)
        });
        match made {
            Ok(file) => return Ok((path, stdfd::fd_safer(file)?)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}

/// Whether a failed spawn failed to fork -- no process at all -- rather
/// than to run the program in the process it made.
fn is_fork_failure(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(11 | 12))
}

/// What upstream's child writes when its `exec` fails: the words, then
/// `: errno N` -- a number, `strerror` not being safe to call there -- and
/// no `sort: ` before them.
fn child_died(words: &str, e: &io::Error) {
    let number = e.raw_os_error().unwrap_or(0);
    diag!("{words}: errno {number}");
}

/// `'PROG' [-d] terminated abnormally`: a compress program that failed, as
/// upstream reports it when it waits for one.
fn abnormal(program: &OsStr) -> ! {
    die(&format!(
        "{} [-d] terminated abnormally",
        quoteaf_os(program)
    ))
}

/// Upstream's `wait_proc`: a compress program that did not exit 0 ends the
/// run.
fn wait(mut child: Child, program: &OsStr) {
    match child.wait() {
        Ok(status) if status.success() => {}
        Ok(_) => abnormal(program),
        Err(e) => die(&format!(
            "waiting for {} [-d]: {}",
            quoteaf_os(program),
            strerror(&e)
        )),
    }
}

fn couldnt_create(path: &Path, e: &io::Error) -> ! {
    die(&format!(
        "couldn't create temporary file: {}: {}",
        quotef_os(path.as_os_str()),
        strerror(e)
    ))
}

/// Where a temporary file's lines go: the file itself, or the compress
/// program's standard input.
enum TempWriter {
    File(io::BufWriter<File>),
    Compressor(io::BufWriter<ChildStdin>),
}

impl TempWriter {
    fn writer(&mut self) -> &mut dyn Write {
        match self {
            TempWriter::File(w) => w,
            TempWriter::Compressor(w) => w,
        }
    }

    /// `xfclose` of a temporary file: flushed and closed, a failure the end
    /// of the run.
    fn close(self, temp: &Temp) {
        let flushed = match self {
            TempWriter::File(mut w) => w.flush().and_then(|()| {
                w.into_inner()
                    .map_err(io::IntoInnerError::into_error)
                    .and_then(stdfd::close)
            }),
            TempWriter::Compressor(mut w) => w.flush(),
        };
        if let Err(e) = flushed {
            die(&format!(
                "close failed: {}: {}",
                quotef_os(temp.path.as_os_str()),
                strerror(&e)
            ));
        }
    }
}

// ── where lines go ───────────────────────────────────────────────────────────

/// The run's output: `-o`'s file, opened at the start as upstream's
/// `check_output` opens it -- created, not emptied -- or standard output.
/// It is emptied and written only when the last input has been read, which
/// is what lets `sort -o f f` read `f` first.
pub struct Output {
    file: Option<File>,
    name: String,
    out: Option<StdioFile>,
    debug: bool,
}

impl Output {
    /// Upstream's `check_output`.
    pub fn open(cfg: &Config) -> Self {
        let (file, name) = match &cfg.output {
            None => (None, coreutils::quote::quotef(b"standard output")),
            Some(path) => {
                let mut options = OpenOptions::new();
                options.write(true).create(true);
                let file = options.open(path).unwrap_or_else(|e| {
                    die(&format!(
                        "open failed: {}: {}",
                        quotef_os(path),
                        strerror(&e)
                    ))
                });
                (Some(file), quotef_os(path))
            }
        };
        Output {
            file,
            name,
            out: None,
            debug: cfg.debug,
        }
    }

    /// What `fstat` says of the output -- `get_outstatus`, for
    /// `avoid_trashing_input`.
    fn metadata(&self) -> Option<std::fs::Metadata> {
        match &self.file {
            Some(file) => file.metadata().ok(),
            None => stdfd::metadata(1).ok(),
        }
    }

    /// Upstream's `stream_open (output, "w")`: `-o`'s file emptied now, a
    /// failure fatal unless the file is not a regular one.
    fn start(&mut self) {
        if self.out.is_some() {
            return;
        }
        self.out = Some(match self.file.take() {
            None => StdioFile::stdout(),
            Some(file) => {
                if let Err(e) = file.set_len(0) {
                    // A device or a pipe cannot be emptied, and need not be:
                    // only a regular file -- or one `fstat` cannot see -- is
                    // an error.
                    let regular = match file.metadata() {
                        Ok(meta) => meta.file_type().is_file(),
                        Err(_) => true,
                    };
                    if regular {
                        die(&format!(
                            "{}: error truncating: {}",
                            self.name,
                            strerror(&e)
                        ));
                    }
                }
                StdioFile::from_file(file)
            }
        });
    }

    /// Write one line: as it is with its terminator, or under `--debug` as
    /// upstream's `write_line` draws it.
    fn line(&mut self, cfg: &Config, line: &[u8]) -> Result<(), io::Error> {
        self.start();
        let mut record = Vec::with_capacity(line.len().saturating_add(1));
        if self.debug {
            crate::debug::annotate(
                &mut record,
                line,
                &cfg.keys,
                cfg.tab,
                !(cfg.unique || cfg.stable),
            );
        } else {
            record.extend_from_slice(line);
            record.push(cfg.delim);
        }
        match self.out.as_mut() {
            Some(out) => out.write(&record),
            None => Ok(()),
        }
    }

    /// The end of the output: `xfclose`'s `fflush` for standard output,
    /// `close_stdout`'s close for `-o`'s file.
    pub fn finish(mut self, cfg: &Config) -> Result<(), std::process::ExitCode> {
        self.start();
        let Some(mut out) = self.out.take() else {
            return Ok(());
        };
        if let Err(e) = out.flush() {
            return Err(self.failed("fflush failed", &e));
        }
        if cfg.output.is_some()
            && let Err(e) = out.close()
        {
            diag!("sort: write error: {}", strerror(&e));
            return Err(std::process::ExitCode::from(2));
        }
        Ok(())
    }

    /// A write that failed: said as upstream says it, unless nobody is left
    /// to read (see `stdfd::reader_gone`).
    fn failed(&self, what: &str, e: &io::Error) -> std::process::ExitCode {
        if stdfd::reader_gone(e) {
            return std::process::ExitCode::SUCCESS;
        }
        diag!("sort: {what}: {}: {}", self.name, strerror(e));
        diag!("sort: write error");
        std::process::ExitCode::from(2)
    }
}

/// Where one pass writes: the output, or a temporary file.
enum Sink<'o> {
    Output(&'o mut Output),
    Temp(TempWriter, &'o Temp),
}

/// Write `line` to `sink`; a temporary file that cannot be written is the
/// end of the run, the output's failure the run's status.
fn put(cfg: &Config, sink: &mut Sink<'_>, line: &[u8]) -> Result<(), std::process::ExitCode> {
    match sink {
        Sink::Output(out) => out
            .line(cfg, line)
            .map_err(|e| out.failed("write failed", &e)),
        Sink::Temp(writer, temp) => {
            let w = writer.writer();
            let written = w.write_all(line).and_then(|()| w.write_all(&[cfg.delim]));
            if let Err(e) = written {
                die(&format!(
                    "write failed: {}: {}",
                    quotef_os(temp.path.as_os_str()),
                    strerror(&e)
                ));
            }
            Ok(())
        }
    }
}

// ── sorting ───────────────────────────────────────────────────────────────────

/// One file the merge reads: an input named on the command line, or a
/// temporary file this run made -- removed once the merge has read it
/// (`zap`), unless it is the copy `avoid_trashing_input` made, which upstream
/// leaves for the exit to remove.
pub struct MergeFile {
    name: OsString,
    temp: Option<Temp>,
    zap: bool,
}

impl MergeFile {
    pub fn input(name: OsString) -> Self {
        MergeFile {
            name,
            temp: None,
            zap: false,
        }
    }

    fn temp(temp: Temp) -> Self {
        MergeFile {
            name: temp.path.clone().into_os_string(),
            temp: Some(temp),
            zap: true,
        }
    }

    /// A reference to `avoid_trashing_input`'s copy: read through the
    /// decompressor as the copy is, never removed by the merge.
    fn copy(path: &Path, compressed: bool, child: Option<Child>) -> Self {
        MergeFile {
            name: path.as_os_str().to_os_string(),
            temp: Some(Temp {
                path: path.to_path_buf(),
                compressed,
                child,
            }),
            zap: false,
        }
    }
}

/// Upstream's `sort`: every input read a buffer at a time; a buffer that
/// holds the end of the last input, with nothing written before it, goes
/// straight to the output; any other is sorted into a temporary file, and
/// the temporary files are then merged.
pub fn sort(cfg: &Config, output: &mut Output) -> Result<(), std::process::ExitCode> {
    let line_bytes = bytes_per_line(threads(cfg));
    let mut temps = Temps::new(cfg);
    let mut written: Vec<MergeFile> = Vec::new();
    let mut buf: Option<Buffer> = None;
    let files = &cfg.files;
    for (n, name) in files.iter().enumerate() {
        let mut input = stream_open(name).unwrap_or_else(|e| {
            die(&format!(
                "open failed: {}: {}",
                quotef_os(name),
                strerror(&e)
            ))
        });
        let more = n.saturating_add(1) < files.len();
        if buf.is_none() {
            // Sized once, with the first input open and every other one
            // asked by name.
            let meta = match &input {
                Input::File(file) => file.metadata(),
                _ => stdfd::metadata(0),
            };
            let meta = meta.unwrap_or_else(|e| {
                die(&format!(
                    "stat failed: {}: {}",
                    quotef_os(name),
                    strerror(&e)
                ))
            });
            let rest = files.get(n..).unwrap_or_default();
            buf = Some(Buffer::new(
                line_bytes,
                sort_buffer_size(cfg, &meta, rest, line_bytes),
            ));
        }
        let Some(b) = buf.as_mut() else {
            break;
        };
        b.eof = false;
        loop {
            let filled = b.fill(&mut input, cfg.delim).unwrap_or_else(|e| {
                die(&format!(
                    "read failed: {}: {}",
                    quotef_os(name),
                    strerror(&e)
                ))
            });
            if !filled {
                break;
            }
            if b.eof && more && line_bytes.saturating_add(1) < b.avail() {
                // "End of file, but there is more input and buffer room.
                // Concatenate the next input file."
                b.left = b.text.len();
                break;
            }
            let mut order: Vec<usize> = (0..b.lines.len()).collect();
            order.sort_by(|&x, &y| crate::compare(cfg, b.line(x), b.line(y)));
            if b.eof && !more && written.is_empty() && b.left == 0 {
                close_input(input, name);
                let mut sink = Sink::Output(output);
                write_unique(cfg, b, &order, &mut sink)?;
                return Ok(());
            }
            let (temp, writer) = temps.create();
            let mut sink = Sink::Temp(writer, &temp);
            write_unique(cfg, b, &order, &mut sink)?;
            if let Sink::Temp(writer, _) = sink {
                writer.close(&temp);
            }
            written.push(MergeFile::temp(temp));
        }
        close_input(input, name);
    }
    merge(cfg, &mut temps, written, output)
}

/// Upstream's `write_unique` over one sorted buffer: with `-u`, a line equal
/// to the one written before it is not written.
fn write_unique(
    cfg: &Config,
    b: &Buffer,
    order: &[usize],
    sink: &mut Sink<'_>,
) -> Result<(), std::process::ExitCode> {
    let mut saved: Option<usize> = None;
    for &index in order {
        let line = b.line(index);
        if cfg.unique
            && let Some(prev) = saved
            && crate::compare(cfg, b.line(prev), line) == std::cmp::Ordering::Equal
        {
            continue;
        }
        saved = Some(index);
        put(cfg, sink, line)?;
    }
    Ok(())
}

// ── merging ───────────────────────────────────────────────────────────────────

/// Upstream's `merge`: passes of `--batch-size` files into temporary files
/// until what is left can be merged at once into the output.
pub fn merge(
    cfg: &Config,
    temps: &mut Temps<'_>,
    mut files: Vec<MergeFile>,
    output: &mut Output,
) -> Result<(), std::process::ExitCode> {
    let nmerge = usize::try_from(cfg.nmerge).unwrap_or(usize::MAX).max(2);
    while nmerge < files.len() {
        let mut out: Vec<MergeFile> = Vec::new();
        let mut rest: std::collections::VecDeque<MergeFile> = files.into_iter().collect();
        while nmerge <= rest.len() {
            let batch: Vec<MergeFile> = rest.drain(..nmerge).collect();
            out.push(merge_into_temp(cfg, temps, batch, &mut rest)?);
        }
        let cheap_slots = nmerge.saturating_sub(out.len().checked_rem(nmerge).unwrap_or(0));
        if cheap_slots < rest.len() {
            // "So many files remain that they can't all be put into the last
            // NMERGE-sized output window. Do one more merge. Merge as few
            // files as possible, to avoid needless I/O."
            let short = rest.len().saturating_sub(cheap_slots).saturating_add(1);
            let batch: Vec<MergeFile> = rest.drain(..short).collect();
            out.push(merge_into_temp(cfg, temps, batch, &mut rest)?);
        }
        out.extend(rest);
        files = out;
    }
    let mut files = avoid_trashing_input(cfg, temps, files, output)?;
    loop {
        let (opened, error) = open_input_files(temps, &mut files);
        if opened.len() == files.len() {
            let sources: Vec<(Lines, MergeFile)> = opened
                .into_iter()
                .zip(files)
                .map(|(input, file)| (Lines::new(input, cfg.delim), file))
                .collect();
            let mut sink = Sink::Output(output);
            return mergefps(cfg, temps, sources, &mut sink);
        }
        let failed_name = files
            .get(opened.len())
            .map(|f| f.name.clone())
            .unwrap_or_default();
        if opened.len() <= 2 || error.as_ref().and_then(io::Error::raw_os_error) != Some(EMFILE) {
            let e = error.unwrap_or_else(|| io::Error::from(io::ErrorKind::NotFound));
            die(&format!(
                "open failed: {}: {}",
                quotef_os(&failed_name),
                strerror(&e)
            ));
        }
        // "We ran out of file descriptors. Close one of the input files, to
        // gain a file descriptor. Then create a temporary file with our
        // spare file descriptor."
        let mut opened = opened;
        let mut nopened = opened.len();
        if let Some(last) = opened.pop() {
            nopened = nopened.saturating_sub(1);
            if let Some(file) = files.get(nopened) {
                close_input(last, &file.name);
            }
        }
        let (temp, writer) = temps.create();
        let batch: Vec<MergeFile> = files.drain(..nopened).collect();
        let sources: Vec<(Lines, MergeFile)> = opened
            .into_iter()
            .zip(batch)
            .map(|(input, file)| (Lines::new(input, cfg.delim), file))
            .collect();
        let mut sink = Sink::Temp(writer, &temp);
        mergefps(cfg, temps, sources, &mut sink)?;
        if let Sink::Temp(writer, _) = sink {
            writer.close(&temp);
        }
        files.insert(0, MergeFile::temp(temp));
    }
}

/// `EMFILE`: this process has every descriptor it may have.
const EMFILE: i32 = 24;

/// Upstream's `mergefiles` into a new temporary file: as many of `batch` as
/// can be opened (at least two, or the run ends), the rest put back at the
/// front of `rest` for the next pass.
fn merge_into_temp(
    cfg: &Config,
    temps: &mut Temps<'_>,
    mut batch: Vec<MergeFile>,
    rest: &mut std::collections::VecDeque<MergeFile>,
) -> Result<MergeFile, std::process::ExitCode> {
    let (temp, writer) = temps.create();
    let (opened, error) = open_input_files(temps, &mut batch);
    if opened.len() < batch.len() && opened.len() < 2 {
        let name = batch
            .get(opened.len())
            .map(|f| f.name.clone())
            .unwrap_or_default();
        let e = error.unwrap_or_else(|| io::Error::from(io::ErrorKind::NotFound));
        die(&format!(
            "open failed: {}: {}",
            quotef_os(&name),
            strerror(&e)
        ));
    }
    let unopened: Vec<MergeFile> = batch.split_off(opened.len());
    for file in unopened.into_iter().rev() {
        rest.push_front(file);
    }
    let sources: Vec<(Lines, MergeFile)> = opened
        .into_iter()
        .zip(batch)
        .map(|(input, file)| (Lines::new(input, cfg.delim), file))
        .collect();
    let mut sink = Sink::Temp(writer, &temp);
    mergefps(cfg, temps, sources, &mut sink)?;
    if let Sink::Temp(writer, _) = sink {
        writer.close(&temp);
    }
    Ok(MergeFile::temp(temp))
}

/// Upstream's `open_input_files`: open as many of `files` as can be, in
/// order, stopping at the first that cannot -- returning those opened and
/// why the next was not.
fn open_input_files(temps: &Temps<'_>, files: &mut [MergeFile]) -> (Vec<Input>, Option<io::Error>) {
    let mut opened = Vec::with_capacity(files.len());
    for file in files.iter_mut() {
        let input = match file.temp.as_mut() {
            Some(temp) if temp.compressed => temps.open(temp),
            _ => stream_open(&file.name),
        };
        match input {
            Ok(input) => opened.push(input),
            Err(e) => return (opened, Some(e)),
        }
    }
    (opened, None)
}

/// Upstream's `avoid_trashing_input`: an input that is the output -- by
/// name, or by device and inode -- is first copied to a temporary file, and
/// read from there; one copy serves every input that is. Upstream leaves the
/// copy for the exit to remove, as here.
fn avoid_trashing_input(
    cfg: &Config,
    temps: &mut Temps<'_>,
    mut files: Vec<MergeFile>,
    output: &Output,
) -> Result<Vec<MergeFile>, std::process::ExitCode> {
    let outstat = output.metadata();
    let out_path = cfg
        .output
        .as_deref()
        .map(Path::new)
        .unwrap_or(Path::new(""));
    let mut copy: Option<(PathBuf, bool)> = None;
    for index in 0..files.len() {
        let (name, is_temp) = match files.get(index) {
            Some(file) => (file.name.clone(), file.temp.is_some()),
            None => break,
        };
        if is_temp {
            continue;
        }
        let is_stdin = name == "-";
        let same = if cfg.output.as_deref() == Some(name.as_os_str()) && !is_stdin {
            true
        } else {
            let Some(out) = outstat.as_ref() else { break };
            let instat = if is_stdin {
                stdfd::metadata(0)
            } else {
                std::fs::metadata(&name)
            };
            instat.is_ok_and(|i| {
                coreutils::fileid::same_inode((Path::new(&name), &i), (out_path, out))
            })
        };
        if !same {
            continue;
        }
        let reference = match &copy {
            Some((path, compressed)) => MergeFile::copy(path, *compressed, None),
            None => {
                let (mut temp, writer) = temps.create();
                let mut one = vec![MergeFile::input(name.clone())];
                let (opened, error) = open_input_files(temps, &mut one);
                if opened.is_empty() {
                    let e = error.unwrap_or_else(|| io::Error::from(io::ErrorKind::NotFound));
                    die(&format!(
                        "open failed: {}: {}",
                        quotef_os(&name),
                        strerror(&e)
                    ));
                }
                let sources: Vec<(Lines, MergeFile)> = opened
                    .into_iter()
                    .zip(one)
                    .map(|(input, f)| (Lines::new(input, cfg.delim), f))
                    .collect();
                let mut sink = Sink::Temp(writer, &temp);
                mergefps(cfg, temps, sources, &mut sink)?;
                if let Sink::Temp(writer, _) = sink {
                    writer.close(&temp);
                }
                copy = Some((temp.path.clone(), temp.compressed));
                MergeFile::copy(&temp.path, temp.compressed, temp.child.take())
            }
        };
        if let Some(slot) = files.get_mut(index) {
            *slot = reference;
        }
    }
    Ok(files)
}

/// Upstream's `mergefps`: the smallest line of all the inputs, again and
/// again, ties to the earlier input; under `-u` only the first of each run
/// of equal lines. Each input is closed as it runs out -- and removed, if
/// this run made it.
fn mergefps(
    cfg: &Config,
    temps: &Temps<'_>,
    sources: Vec<(Lines, MergeFile)>,
    sink: &mut Sink<'_>,
) -> Result<(), std::process::ExitCode> {
    struct Live {
        lines: Lines,
        file: MergeFile,
        current: Vec<u8>,
    }
    let read = |live: &mut Lines, file: &MergeFile| -> Option<Vec<u8>> {
        live.next().unwrap_or_else(|e| {
            die(&format!(
                "read failed: {}: {}",
                quotef_os(&file.name),
                strerror(&e)
            ))
        })
    };
    let finish = |lines: Lines, file: MergeFile| {
        close_input(lines.into_input(), &file.name);
        match file.temp {
            Some(temp) if file.zap => temps.zap(temp),
            // Not ours to remove, but a decompressor reading it is still
            // ours to wait for.
            Some(mut temp) => {
                if let Some(child) = temp.child.take() {
                    wait(child, temps.compress.unwrap_or_default());
                }
            }
            None => {}
        }
    };
    let mut live: Vec<Live> = Vec::with_capacity(sources.len());
    for (mut lines, file) in sources {
        match read(&mut lines, &file) {
            Some(current) => live.push(Live {
                lines,
                file,
                current,
            }),
            None => finish(lines, file),
        }
    }
    // `ord`: the inputs by their current line, ties to the earlier input.
    let mut ord: Vec<usize> = (0..live.len()).collect();
    ord.sort_by(|&a, &b| {
        let (Some(x), Some(y)) = (live.get(a), live.get(b)) else {
            return std::cmp::Ordering::Equal;
        };
        crate::compare(cfg, &x.current, &y.current)
    });
    let mut saved: Option<Vec<u8>> = None;
    while let Some(&first) = ord.first() {
        let Some(smallest) = live.get(first).map(|l| l.current.clone()) else {
            break;
        };
        if cfg.unique {
            if let Some(kept) = &saved
                && crate::compare(cfg, kept, &smallest) != std::cmp::Ordering::Equal
            {
                put(cfg, sink, kept)?;
                saved = None;
            }
            if saved.is_none() {
                saved = Some(smallest);
            }
        } else {
            put(cfg, sink, &smallest)?;
        }
        let next = live
            .get_mut(first)
            .and_then(|l| read(&mut l.lines, &l.file));
        match next {
            Some(line) => {
                if let Some(l) = live.get_mut(first) {
                    l.current = line;
                }
                // Push it back past every input whose line is smaller, and
                // past the equal ones of earlier inputs.
                ord.remove(0);
                let mut lo = 0usize;
                let mut hi = ord.len();
                while lo < hi {
                    let probe = lo.saturating_add(hi) / 2;
                    let other = ord.get(probe).copied().unwrap_or(0);
                    let cmp = match (live.get(first), live.get(other)) {
                        (Some(a), Some(b)) => crate::compare(cfg, &a.current, &b.current),
                        _ => std::cmp::Ordering::Equal,
                    };
                    if cmp == std::cmp::Ordering::Less
                        || (cmp == std::cmp::Ordering::Equal && first < other)
                    {
                        hi = probe;
                    } else {
                        lo = probe.saturating_add(1);
                    }
                }
                ord.insert(lo, first);
            }
            None => {
                ord.remove(0);
                // Its slot stays, emptied, so the indices in `ord` hold.
                if let Some(l) = live.get_mut(first) {
                    let lines =
                        std::mem::replace(&mut l.lines, Lines::new(Input::Stdin, cfg.delim));
                    let file = std::mem::replace(&mut l.file, MergeFile::input(OsString::new()));
                    finish(lines, file);
                }
            }
        }
    }
    if let Some(kept) = saved {
        put(cfg, sink, &kept)?;
    }
    Ok(())
}

// ── checking ──────────────────────────────────────────────────────────────────

/// Upstream's `check` of `-c`/`-C`: read the one input until a line is out
/// of order -- or, under `-u`, equal to the one before -- and say which.
/// `true` when it is in order.
pub fn check(cfg: &Config, diagnose: bool) -> bool {
    let name = cfg
        .files
        .first()
        .cloned()
        .unwrap_or_else(|| OsString::from("-"));
    let input = stream_open(&name).unwrap_or_else(|e| {
        die(&format!(
            "open failed: {}: {}",
            quotef_os(&name),
            strerror(&e)
        ))
    });
    let mut lines = Lines::new(input, cfg.delim);
    let mut prev: Option<Vec<u8>> = None;
    let mut number = 0u64;
    loop {
        let line = match lines.next() {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => die(&format!(
                "read failed: {}: {}",
                quotef_os(&name),
                strerror(&e)
            )),
        };
        number = number.saturating_add(1);
        if let Some(p) = &prev {
            let diff = crate::compare(cfg, p, &line);
            let disordered = if cfg.unique {
                diff != std::cmp::Ordering::Less
            } else {
                diff == std::cmp::Ordering::Greater
            };
            if disordered {
                if diagnose {
                    let mut err = stdfd::Stream::stderr();
                    // Unread: a lost diagnostic is `close_stderr`'s to find.
                    let _ = err.write_all(b"sort: ");
                    let _ = err.write_all(&os_bytes(&name));
                    let _ = write!(err, ":{number}: disorder: ");
                    let _ = err.write_all(&line);
                    let _ = err.write_all(&[cfg.delim]);
                }
                close_input(lines.into_input(), &name);
                return false;
            }
        }
        prev = Some(line);
    }
    close_input(lines.into_input(), &name);
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Every line `fill` gives, fill after fill, until it gives no more.
    fn all_lines(buf: &mut Buffer, data: &[u8]) -> Vec<Vec<Vec<u8>>> {
        let mut input: &[u8] = data;
        let mut fills = Vec::new();
        while buf.fill(&mut input, b'\n').unwrap() {
            fills.push((0..buf.lines.len()).map(|i| buf.line(i).to_vec()).collect());
        }
        fills
    }

    #[test]
    fn a_thread_count_charges_a_line_for_each_merge_level() {
        assert_eq!(bytes_per_line(1), 48);
        assert_eq!(bytes_per_line(2), 64);
        assert_eq!(bytes_per_line(8), 128);
        assert_eq!(bytes_per_line(12), 160);
    }

    #[test]
    fn initbuf_rounds_up_past_a_whole_line() {
        assert_eq!(Buffer::new(48, 1000).alloc, 1024);
        // Already whole: a whole line more, as upstream's arithmetic has it.
        assert_eq!(Buffer::new(48, 1024).alloc, 1056);
    }

    #[test]
    fn every_line_comes_out_once_in_order_however_the_buffer_cuts() {
        let data: Vec<u8> = (0..500)
            .flat_map(|n| format!("line {n}\n").into_bytes())
            .collect();
        let mut buf = Buffer::new(48, 1024);
        let fills = all_lines(&mut buf, &data);
        assert!(fills.len() > 1, "a 1 KiB buffer cannot hold 500 lines");
        let lines: Vec<Vec<u8>> = fills.into_iter().flatten().collect();
        let want: Vec<Vec<u8>> = (0..500).map(|n| format!("line {n}").into_bytes()).collect();
        assert_eq!(lines, want);
    }

    #[test]
    fn a_last_line_without_a_terminator_is_given_one() {
        let mut buf = Buffer::new(48, 4096);
        assert_eq!(
            all_lines(&mut buf, b"b\na"),
            vec![vec![b"b".to_vec(), b"a".to_vec()]]
        );
        let mut buf = Buffer::new(48, 4096);
        assert!(all_lines(&mut buf, b"").is_empty());
    }

    #[test]
    fn a_line_longer_than_the_buffer_grows_it() {
        let mut data = vec![b'x'; 5000];
        data.extend_from_slice(b"\nshort\n");
        let mut buf = Buffer::new(48, 64);
        let lines: Vec<Vec<u8>> = all_lines(&mut buf, &data).into_iter().flatten().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 5000);
        assert_eq!(lines[1], b"short");
        assert!(buf.alloc > 5000);
    }

    // Unix only: the host build has no system random source to name the
    // file from, and sorts nothing that spills.
    #[cfg(unix)]
    #[test]
    fn a_temporary_file_is_registered_until_it_is_removed() {
        let dir = std::env::temp_dir();
        let (path, file) = make_temp_file(dir.as_os_str()).unwrap();
        drop(file);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("sort") && name.len() == 10, "{name}");
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        let registered = cleanup::change(|list| list.contains(&c));
        assert!(registered);
        assert!(path.exists());
        remove_all();
        assert!(!path.exists());
    }
}
