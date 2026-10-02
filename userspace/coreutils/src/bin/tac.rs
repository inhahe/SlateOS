//! `tac` — write each file to standard output, last record first.
//!
//! A port of GNU coreutils 9.4's `src/tac.c`, function by function: the same
//! options, the same messages, and the same reading -- backwards, a buffer at
//! a time, with the buffer's size carried from one file to the next -- because
//! with `-r` the reading is observable.
//!
//! # What this replaced
//!
//! `userspace/tac`, a hand-written "reverse line printer and character
//! reverser for Slate OS". It read its arguments as `String` (so a file name
//! that was not UTF-8 killed it before its first statement), had no `-b`,
//! `-r` or `-s`, and added options of its own that no `tac` has.
//!
//! # Three things no reading of `--help` suggests
//!
//! **`-s ''` separates on NUL.** Without `-r`, upstream takes an empty
//! separator's length as 1 and compares its first byte -- the string's
//! terminator -- so the records end at NUL bytes. With `-r`, an empty
//! separator is an error.
//!
//! **`-r` patterns are Emacs syntax, matched against a window.**
//! `re_compile_pattern` reads glibc's default syntax, `RE_SYNTAX_EMACS`
//! (`\(…\|…\)` groups, `+` and `?` operators, no `{}` intervals, no `[:class:]`)
//! with `^` and `$` also anchoring at newlines. And the pattern is searched
//! backwards within the part of the read buffer not yet printed, so `^` and
//! `$` also hold where that window begins and ends: wherever an 8 KiB read
//! happened to start. `tac -r -s '^x'` on a file longer than one read finds an
//! `x` at a read boundary that is not at a line's start. That is upstream's
//! behaviour, and kept: it falls out of reading the file the same way.
//!
//! **Reading `-` twice reads it twice.** Standard input that is a file is
//! seeked to its end for each `-`, so `tac - - <f` prints `f` reversed twice;
//! a pipe has nothing left the second time.
//!
//! # Reference
//!
//! Measured against GNU `tac` (coreutils 9.4) through WSL; where a rule could
//! not be settled by measurement, `coreutils-9.4/src/tac.c` settled it.
//! `scripts/tac-diff.sh` is the executable form of every claim here.

use coreutils::diag;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write as _;
use std::process::ExitCode;

/// `tac -Z; echo $?` is 1.
const TAC: Program = Program::new("tac", 1);

/// Upstream's `getopt_long` string, verbatim.
const SHORT_OPTIONS: &str = "brs:";

/// Upstream's `longopts`, in its order -- observable through the ambiguity
/// message, which names the entries a prefix matched in table order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("before", Takes::Nothing),
    ("regex", Takes::Nothing),
    ("separator", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    /// `-b`: the separator goes before the record it precedes in the file,
    /// rather than after the one it follows (`separator_ends_record` false).
    before: bool,
    /// `-r`: the separator is a regular expression.
    regex: bool,
    /// `-s`, or a newline.
    separator: Vec<u8>,
    files: Vec<OsString>,
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            diag!("tac: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            // A failed write is `close_stdout`'s to report, below.
            let _ = out.write_all(HELP.as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"tac (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Request::Run(settings) => match Tac::new(&settings) {
            Ok(mut tac) => {
                let ok = imp::run(&mut tac, &settings.files, &mut out);
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(message) => {
                diag!("tac: {message}");
                return ExitCode::FAILURE;
            }
        },
    };
    stdfd::close_stdout("tac", out, earned)
}

/// Read the command line: upstream's option loop.
///
/// # Errors
/// Any getopt diagnostic.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut set = Settings {
        before: false,
        regex: false,
        separator: b"\n".to_vec(),
        files: Vec::new(),
    };
    for item in TAC.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'b', _) | Opt::Long("before", _) => set.before = true,
            Opt::Short(b'r', _) | Opt::Long("regex", _) => set.regex = true,
            Opt::Short(b's', Some(v)) | Opt::Long("separator", Some(v)) => {
                set.separator = os_bytes(&v).into_owned();
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => set.files.push(word.clone()),
            // Every option in the two tables is handled above; an unknown one
            // arrives as an `Err`, and `-s` without its value cannot parse.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    Ok(Request::Run(set))
}

/// GNU's `--help`, minus the ancillary block of URLs, as every utility here
/// omits it.
const HELP: &str = "\
Usage: tac [OPTION]... [FILE]...
Write each FILE to standard output, last line first.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -b, --before             attach the separator before instead of after
  -r, --regex              interpret the separator as a regular expression
  -s, --separator=STRING   use STRING as the separator instead of newline
      --help        display this help and exit
      --version     output version information and exit
";

/// `INITIAL_READSIZE`: the bytes of one read, to begin with.
const INITIAL_READSIZE: usize = 8192;

/// How the separator is found: as a string, or by a regular expression.
// Read by `imp`, which is unix-only: the host build is a stub that says so.
#[cfg_attr(not(unix), allow(dead_code))]
enum Separator {
    /// The separator's bytes. Upstream's `sentinel_length` is their length,
    /// and its `match_length` the same.
    Fixed(Vec<u8>),
    /// A compiled `-r` pattern. Upstream's `sentinel_length` is 0.
    Regex(ere::Regex),
}

/// Upstream's file-scope state, which outlives a file: the buffer and its
/// read size grow and shrink as files are read, and the next file is read
/// with whatever they were left at.
#[cfg_attr(not(unix), allow(dead_code))] // As `Separator`.
struct Tac {
    separator: Separator,
    /// `separator_ends_record`.
    ends_record: bool,
    /// `read_size`: the bytes of the next read.
    read_size: usize,
    /// The buffer and the bytes before `G_buffer`: upstream points `G_buffer`
    /// `offset` bytes into its allocation, where a copy of the separator sits
    /// in front of the data as a sentinel, so that a backward string search
    /// always stops -- or one spare byte, for `-r`.
    buf: Vec<u8>,
    offset: usize,
    /// `match_length`: for a string, its length; for a pattern, the length
    /// of the last match.
    match_length: usize,
    /// `output`'s buffer: what has been printed and not yet handed to
    /// standard output. Upstream's is static, so it too outlives a file.
    pending: Vec<u8>,
}

impl Tac {
    /// `main`'s setup: compile the separator and size the buffer.
    ///
    /// # Errors
    /// The message upstream exits with: an empty `-r` separator, or the
    /// regular expression's own error.
    fn new(settings: &Settings) -> Result<Tac, String> {
        let separator = if settings.regex {
            if settings.separator.is_empty() {
                return Err("separator cannot be empty".to_string());
            }
            Separator::Regex(compile(&settings.separator)?)
        } else if settings.separator.is_empty() {
            // `*separator ? strlen (separator) : 1` -- the one byte being the
            // empty string's terminator.
            Separator::Fixed(vec![0])
        } else {
            Separator::Fixed(settings.separator.clone())
        };
        let sentinel_length = match &separator {
            Separator::Fixed(s) => s.len(),
            Separator::Regex(_) => 0,
        };
        let mut read_size = INITIAL_READSIZE;
        while sentinel_length >= read_size / 2 {
            read_size = read_size
                .checked_mul(2)
                .ok_or_else(|| "memory exhausted".to_string())?;
        }
        let half = read_size
            .checked_add(sentinel_length)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| "memory exhausted".to_string())?;
        let size = half
            .checked_mul(2)
            .ok_or_else(|| "memory exhausted".to_string())?;
        let mut buf = vec![0u8; size];
        let (offset, match_length) = match &separator {
            Separator::Fixed(s) => {
                if let Some(front) = buf.get_mut(..s.len()) {
                    front.copy_from_slice(s);
                }
                (s.len(), s.len())
            }
            Separator::Regex(_) => (1, 0),
        };
        Ok(Tac {
            separator,
            ends_record: !settings.before,
            read_size,
            buf,
            offset,
            match_length,
            pending: Vec::new(),
        })
    }

    /// `sentinel_length`.
    #[cfg_attr(not(unix), allow(dead_code))] // As `Separator`.
    fn sentinel_length(&self) -> usize {
        match &self.separator {
            Separator::Fixed(s) => s.len(),
            Separator::Regex(_) => 0,
        }
    }
}

/// Compile a `-r` separator as `re_compile_pattern` would: Emacs syntax, the
/// newline anchor on, and the pattern and the input read as the locale reads
/// them -- characters in a UTF-8 `LC_CTYPE`, bytes in the C locale.
fn compile(pattern: &[u8]) -> Result<ere::Regex, String> {
    let compiled = if coreutils::locale::ctype_is_utf8() {
        ere::emacs::compile(pattern, false)
    } else {
        ere::emacs::compile_bytes(pattern, false)
    };
    compiled
        .map(|re| re.with_newline_anchor(true))
        .map_err(|e| e.message().to_string())
}

#[cfg(not(unix))]
mod imp {
    use super::Tac;
    use coreutils::diag;
    use coreutils::stdfd::Stream;
    use std::ffi::OsString;

    /// `tac` seeks its input, which the Windows build host's handles are not
    /// worth teaching to do; SlateOS is the target.
    pub fn run(_tac: &mut Tac, _files: &[OsString], _out: &mut Stream) -> bool {
        diag!("tac: unix-only utility; not supported on this platform");
        false
    }
}

#[cfg(unix)]
mod imp {
    use super::{Separator, Tac};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::{os_bytes, os_from_bytes, quoteaf_os, quotef};
    use coreutils::stdfd::{self, Stream};
    use std::ffi::OsString;
    use std::fs::File;
    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::mem::ManuallyDrop;
    use std::os::fd::FromRawFd;

    /// Every file, in order; `-` and no files at all are standard input.
    /// Upstream's loop in `main`, and its close of standard input after it.
    pub fn run(tac: &mut Tac, files: &[OsString], out: &mut Stream) -> bool {
        let stdin_only = [OsString::from("-")];
        let files = if files.is_empty() {
            &stdin_only[..]
        } else {
            files
        };
        let mut ok = true;
        let mut have_read_stdin = false;
        for name in files {
            ok &= tac_file(tac, name, out, &mut have_read_stdin);
        }
        // `output (nullptr, nullptr)`: what the buffer still holds. A failed
        // write is `close_stdout`'s to report.
        let _ = out.write_all(&tac.pending);
        tac.pending.clear();
        if have_read_stdin {
            // SAFETY: descriptor 0 is standard input; nothing else in this
            // process closes it, and nothing uses it after this point.
            if unsafe { libc_close(0) } != 0 {
                diag!("tac: -: {}", strerror(&io::Error::last_os_error()));
                ok = false;
            }
        }
        ok
    }

    unsafe extern "C" {
        /// `close(2)`, for its result: `File`'s drop discards a failed close,
        /// which upstream reports as a read error.
        #[link_name = "close"]
        fn libc_close(fd: i32) -> i32;
    }

    /// One input, open: a file this process opened, or standard input, whose
    /// descriptor is the process's and is not closed here.
    struct Input {
        file: ManuallyDrop<File>,
        owned: bool,
        fd: i32,
    }

    impl Drop for Input {
        fn drop(&mut self) {
            if self.owned {
                // SAFETY: `owned` is set only for a file opened by `tac_file`
                // and not closed since, so this is its one close.
                unsafe { ManuallyDrop::drop(&mut self.file) };
            }
        }
    }

    /// `tac_file`: print one file in reverse, copying it to a temporary file
    /// first if it cannot be seeked.
    fn tac_file(
        tac: &mut Tac,
        name: &OsString,
        out: &mut Stream,
        have_read_stdin: &mut bool,
    ) -> bool {
        let bytes = os_bytes(name);
        let is_stdin = bytes.as_ref() == b"-";
        let (input, label) = if is_stdin {
            *have_read_stdin = true;
            // SAFETY: descriptor 0 is standard input, open for the life of
            // the process; `owned` is false, so it is never closed here.
            let file = ManuallyDrop::new(unsafe { File::from_raw_fd(0) });
            (
                Input {
                    file,
                    owned: false,
                    fd: 0,
                },
                b"standard input".to_vec(),
            )
        } else {
            match File::open(os_from_bytes(&bytes)) {
                Ok(f) => {
                    use std::os::fd::AsRawFd;
                    let fd = f.as_raw_fd();
                    (
                        Input {
                            file: ManuallyDrop::new(f),
                            owned: true,
                            fd,
                        },
                        bytes.to_vec(),
                    )
                }
                Err(e) => {
                    diag!(
                        "tac: failed to open {} for reading: {}",
                        quoteaf_os(name),
                        strerror(&e)
                    );
                    return false;
                }
            }
        };
        let mut input = input;
        let file_size = (&*input.file).seek(SeekFrom::End(0));
        let tty = stdfd::is_tty(input.fd);
        let ok = match file_size {
            Ok(size) if !tty => tac_seekable(tac, &input.file, &label, size, out),
            _ => tac_nonseekable(tac, &input.file, &label, out),
        };
        if !is_stdin {
            let fd = input.fd;
            input.owned = false;
            // SAFETY: the descriptor was opened by this function and is closed
            // once, here, with `owned` cleared so `Drop` does not close it again.
            if unsafe { libc_close(fd) } != 0 {
                diag!(
                    "tac: {}: read error: {}",
                    quotef(&label),
                    strerror(&io::Error::last_os_error())
                );
                return false;
            }
        }
        ok
    }

    /// `safe_read`: a read that retries an interrupted call. `Err` is
    /// `SAFE_READ_ERROR`.
    fn safe_read(file: &File, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match (&*file).read(buf) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                other => return other,
            }
        }
    }

    /// `full_read`: read until `buf` is full or the file ends. The count, and
    /// the error that cut it short.
    fn full_read(file: &File, buf: &mut [u8]) -> (usize, Option<io::Error>) {
        let mut got: usize = 0;
        while let Some(rest) = buf.get_mut(got..) {
            if rest.is_empty() {
                break;
            }
            match safe_read(file, rest) {
                Ok(0) => break,
                Ok(n) => got = got.saturating_add(n),
                Err(e) => return (got, Some(e)),
            }
        }
        (got, None)
    }

    /// `lseek`, with upstream's complaint when it fails -- which it makes and
    /// then carries on, reading wherever the descriptor is.
    fn seek_or_warn(file: &File, to: SeekFrom, label: &[u8]) {
        if let Err(e) = (&*file).seek(to) {
            diag!("tac: {}: seek failed: {}", quotef(label), strerror(&e));
        }
    }

    /// `tac_seekable`: print in reverse the file open on `file`, which is
    /// positioned at `file_pos`, near its end.
    ///
    /// Positions in the buffer are relative to `G_buffer`, which is
    /// `tac.offset` bytes into `tac.buf`: a match start below 0 is upstream's
    /// "backed off the front without finding a match". Every index and sum
    /// below stays inside the buffer by upstream's own sizing; they are
    /// written checked so that a broken invariant ends this file's output
    /// rather than the process.
    #[allow(clippy::too_many_lines)]
    fn tac_seekable(
        tac: &mut Tac,
        file: &File,
        label: &[u8],
        file_pos: u64,
        out: &mut Stream,
    ) -> bool {
        let sentinel = tac.sentinel_length();
        let mut file_pos = file_pos;
        let mut first_time = true;

        // Lop off enough for the rest of the file to be a multiple of the
        // read size.
        let remainder = file_pos.checked_rem(as_u64(tac.read_size)).unwrap_or(0);
        if remainder != 0 {
            file_pos = file_pos.saturating_sub(remainder);
            seek_or_warn(file, SeekFrom::Start(file_pos), label);
        }

        // Scan backward, looking for end of file -- for proc-like files whose
        // size is an estimate.
        let mut saved;
        loop {
            let read = tac.read_size;
            saved = safe_read(file, g_mut(&mut tac.buf, tac.offset, 0, read));
            match saved {
                Ok(0) if file_pos != 0 => {
                    let back = i64::try_from(read)
                        .ok()
                        .and_then(i64::checked_neg)
                        .unwrap_or(i64::MIN);
                    seek_or_warn(file, SeekFrom::Current(back), label);
                    file_pos = file_pos.saturating_sub(as_u64(read));
                }
                _ => break,
            }
        }
        // Now scan forward, looking for end of file.
        while let Ok(n) = saved {
            if n != tac.read_size {
                break;
            }
            let read = tac.read_size;
            match safe_read(file, g_mut(&mut tac.buf, tac.offset, 0, read)) {
                Ok(0) => break,
                Ok(nread) => {
                    saved = Ok(nread);
                    file_pos = file_pos.saturating_add(as_u64(nread));
                }
                Err(e) => saved = Err(e),
            }
        }
        let saved = match saved {
            Ok(n) => n,
            Err(e) => {
                diag!("tac: {}: read error: {}", quotef(label), strerror(&e));
                return false;
            }
        };

        let mut past_end = saved;
        let mut match_start = as_isize(saved);
        if sentinel > 0 {
            match_start = match_start.saturating_sub(as_isize(tac.match_length.saturating_sub(1)));
        }
        // The `-r` search's decoded window, built when the buffer's bytes
        // change and reused by every search until they do again.
        let mut window: Option<ere::Search<'_>> = None;

        loop {
            // Search backward from `match_start - 1` for the separator.
            match &tac.separator {
                Separator::Regex(re) => {
                    let i = usize::try_from(match_start).unwrap_or(0);
                    // `regoff_t` is an `int`: a window past it is the
                    // "record too large" upstream exits with.
                    if i32::try_from(i).is_err() {
                        diag!("tac: record too large");
                        stdfd::exit_now(1, 1);
                    }
                    if i == 0 {
                        match_start = -1;
                    } else {
                        let search = window.get_or_insert_with(|| {
                            re.search(g_slice(&tac.buf, tac.offset, 0, past_end))
                        });
                        match search.rsearch(i) {
                            Ok(None) => match_start = -1,
                            Ok(Some((start, end))) => {
                                match_start = as_isize(start);
                                tac.match_length = end.saturating_sub(start);
                            }
                            Err(_) => {
                                diag!("tac: error in regular expression search");
                                stdfd::exit_now(1, 1);
                            }
                        }
                    }
                }
                Separator::Fixed(sep) => {
                    // The copy of the separator in front of `G_buffer` stops
                    // this at `-sentinel` at the latest.
                    let first = sep.first().copied().unwrap_or(0);
                    let rest = sep.get(1..).unwrap_or_default();
                    loop {
                        match_start = match_start.saturating_sub(1);
                        let Some(at) = tac.offset.checked_add_signed(match_start) else {
                            // Below the allocation: not reachable while the
                            // sentinel is in place, and "no match" if it is.
                            break;
                        };
                        if tac.buf.get(at) == Some(&first)
                            && (rest.is_empty()
                                || tac
                                    .buf
                                    .get(at.saturating_add(1)..at.saturating_add(sep.len()))
                                    == Some(rest))
                        {
                            break;
                        }
                    }
                }
            }

            if match_start < 0 {
                if file_pos == 0 {
                    // The beginning of the file: print the remaining record.
                    output(
                        &mut tac.pending,
                        out,
                        g_slice(&tac.buf, tac.offset, 0, past_end),
                    );
                    return true;
                }
                let saved_record = past_end;
                if saved_record > tac.read_size {
                    // Room for another read in front of what is pending:
                    // upstream's realloc, which keeps the allocation's start
                    // (and the sentinel there) and so `G_buffer`'s offset.
                    let grown = tac.read_size.checked_mul(2).and_then(|r| {
                        r.checked_mul(2)
                            .and_then(|n| n.checked_add(sentinel))
                            .and_then(|n| n.checked_add(2))
                            .map(|size| (r, size))
                    });
                    match grown {
                        Some((read, size)) if size >= tac.buf.len() => {
                            tac.read_size = read;
                            tac.buf.resize(size, 0);
                        }
                        _ => {
                            diag!("tac: memory exhausted");
                            stdfd::exit_now(1, 1);
                        }
                    }
                }
                // Back up to the start of the next bufferful.
                if file_pos >= as_u64(tac.read_size) {
                    file_pos = file_pos.saturating_sub(as_u64(tac.read_size));
                } else {
                    tac.read_size = usize::try_from(file_pos).unwrap_or(usize::MAX);
                    file_pos = 0;
                }
                seek_or_warn(file, SeekFrom::Start(file_pos), label);

                // Shift the pending record right to make room for the new.
                let g = tac.offset;
                let read = tac.read_size;
                tac.buf
                    .copy_within(g..g.saturating_add(saved_record), g.saturating_add(read));
                past_end = read.saturating_add(saved_record);
                match_start = as_isize(if sentinel > 0 { read } else { past_end });
                window = None;
                let (got, err) = full_read(file, g_mut(&mut tac.buf, tac.offset, 0, read));
                if got != read {
                    // A short read with no error is upstream's `errno` of
                    // whatever came before -- here, that the file ended.
                    let e = err.unwrap_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof));
                    diag!("tac: {}: read error: {}", quotef(label), strerror(&e));
                    return false;
                }
            } else {
                let start = usize::try_from(match_start).unwrap_or(0);
                if tac.ends_record {
                    let match_end = start.saturating_add(tac.match_length);
                    // Unless this match is the very end of the file, print the
                    // record after it.
                    if !first_time || match_end != past_end {
                        output(
                            &mut tac.pending,
                            out,
                            g_slice(&tac.buf, tac.offset, match_end, past_end),
                        );
                    }
                    past_end = match_end;
                    first_time = false;
                } else {
                    output(
                        &mut tac.pending,
                        out,
                        g_slice(&tac.buf, tac.offset, start, past_end),
                    );
                    past_end = start;
                }
                if sentinel > 0 {
                    match_start =
                        match_start.saturating_sub(as_isize(tac.match_length.saturating_sub(1)));
                }
            }
        }
    }

    /// `G_buffer[from..to]`.
    fn g_slice(buf: &[u8], offset: usize, from: usize, to: usize) -> &[u8] {
        buf.get(offset.saturating_add(from)..offset.saturating_add(to))
            .unwrap_or_default()
    }

    /// `G_buffer[from..from + len]`, to read into. Borrows the buffer alone,
    /// so the `-r` window can go on borrowing the pattern beside it.
    fn g_mut(buf: &mut [u8], offset: usize, from: usize, len: usize) -> &mut [u8] {
        let start = offset.saturating_add(from);
        buf.get_mut(start..start.saturating_add(len))
            .unwrap_or_default()
    }

    /// A buffer length as a file offset: `usize` is no wider than `u64` here.
    fn as_u64(n: usize) -> u64 {
        u64::try_from(n).unwrap_or(u64::MAX)
    }

    /// A buffer length as a signed position: a `Vec` never holds more than
    /// `isize::MAX` bytes.
    fn as_isize(n: usize) -> isize {
        isize::try_from(n).unwrap_or(isize::MAX)
    }

    /// `WRITESIZE`: the bytes `output` gathers before handing them on.
    const WRITESIZE: usize = 8192;

    /// `output`: append `bytes` to upstream's own output buffer, handing it to
    /// standard output each time it fills.
    ///
    /// The buffer is observable, which is why it is kept rather than left to
    /// the stream. A diagnostic flushes standard output first, as glibc's
    /// `error` does -- but not this buffer, so `tac f missing g` names
    /// `missing` before printing any of `f`. And a full device fails a write
    /// made when the buffer fills, an *earlier* write, which `close_stdout`
    /// reports as a bare "write error". A failed write is `close_stdout`'s to
    /// report either way.
    fn output(pending: &mut Vec<u8>, out: &mut Stream, bytes: &[u8]) {
        let mut rest = bytes;
        let mut available = WRITESIZE.saturating_sub(pending.len());
        while rest.len() >= available {
            let (head, tail) = rest.split_at(available);
            pending.extend_from_slice(head);
            let _ = out.write_all(pending);
            pending.clear();
            rest = tail;
            available = WRITESIZE;
        }
        pending.extend_from_slice(rest);
    }

    /// `tac_nonseekable`: copy the input to a temporary file, then print that
    /// in reverse.
    fn tac_nonseekable(tac: &mut Tac, input: &File, label: &[u8], out: &mut Stream) -> bool {
        let Some((tmp, name, copied)) = copy_to_temp(tac, input, label) else {
            return false;
        };
        tac_seekable(tac, &tmp, &name, copied, out)
    }

    /// `copy_to_temp`: copy `input` to a new temporary file and return it, the
    /// name it was made under, and the number of bytes copied.
    fn copy_to_temp(tac: &mut Tac, input: &File, label: &[u8]) -> Option<(File, Vec<u8>, u64)> {
        let (tmp, name) = temp_stream()?;
        let mut copied: u64 = 0;
        loop {
            let read = tac.read_size;
            match safe_read(input, g_mut(&mut tac.buf, tac.offset, 0, read)) {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = (&tmp).write_all(g_slice(&tac.buf, tac.offset, 0, n)) {
                        diag!("tac: {}: write error: {}", quotef(&name), strerror(&e));
                        return None;
                    }
                    copied = copied.saturating_add(as_u64(n));
                }
                Err(e) => {
                    diag!("tac: {}: read error: {}", quotef(label), strerror(&e));
                    return None;
                }
            }
        }
        Some((tmp, name, copied))
    }

    /// gnulib's `temp_stream`: a new file named `cutmpXXXXXX` in the temporary
    /// directory, unlinked as soon as it is open -- nothing else can reach it,
    /// and it is gone with the process -- and the name it was made under.
    ///
    /// The directory is `path_search`'s: `$TMPDIR` if that names an existing
    /// directory, otherwise `/tmp`. So `TMPDIR=/nonexistent` is not an error;
    /// only a machine with neither is.
    fn temp_stream() -> Option<(File, Vec<u8>)> {
        use std::os::unix::fs::OpenOptionsExt;
        use std::path::PathBuf;
        let dir = std::env::var_os("TMPDIR")
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .or_else(|| Some(PathBuf::from("/tmp")).filter(|d| d.is_dir()));
        let Some(dir) = dir else {
            diag!(
                "tac: failed to make temporary file name: {}",
                strerror(&io::Error::from(io::ErrorKind::NotFound))
            );
            return None;
        };
        let template = dir.join("cutmpXXXXXX");
        let pid = std::process::id();
        let mut last = io::Error::from(io::ErrorKind::AlreadyExists);
        for attempt in 0u32..100 {
            let path = dir.join(format!("cutmp{pid:x}{attempt:02}"));
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(f) => {
                    let name = os_bytes(path.as_os_str()).into_owned();
                    if let Err(e) = std::fs::remove_file(&path) {
                        diag!("tac: {}: {}", quotef(&name), strerror(&e));
                    }
                    return Some((f, name));
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = e,
                Err(e) => {
                    last = e;
                    break;
                }
            }
        }
        diag!(
            "tac: failed to create temporary file {}: {}",
            quoteaf_os(template.as_os_str()),
            strerror(&last)
        );
        None
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn settings(words: &[&str]) -> Settings {
        match parse_args(&argv(words)).unwrap() {
            Request::Run(s) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_options_are_upstreams() {
        let s = settings(&["-b", "-r", "-s", "x+", "f"]);
        assert!(s.before && s.regex);
        assert_eq!(s.separator, b"x+");
        assert_eq!(s.files, argv(&["f"]));
        let s = settings(&["--before", "--regex", "--separator=;", "a", "b"]);
        assert!(s.before && s.regex);
        assert_eq!(s.separator, b";");
        assert_eq!(s.files, argv(&["a", "b"]));
        assert_eq!(settings(&[]).separator, b"\n");
        assert_eq!(parse_args(&argv(&["--help", "-Z"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--ver"])).unwrap(), Request::Version);
        assert!(parse_args(&argv(&["-Z"])).is_err());
        assert!(parse_args(&argv(&["-s"])).is_err());
    }

    #[test]
    fn an_empty_separator_is_a_nul_unless_it_is_a_pattern() {
        let tac = Tac::new(&settings(&["-s", ""])).unwrap();
        assert!(matches!(&tac.separator, Separator::Fixed(s) if s == &[0]));
        let err = Tac::new(&settings(&["-r", "-s", ""])).err().unwrap();
        assert_eq!(err, "separator cannot be empty");
    }

    #[test]
    fn a_bad_pattern_says_what_glibc_says() {
        let err = Tac::new(&settings(&["-r", "-s", "\\("])).err().unwrap();
        assert_eq!(err, "Unmatched ( or \\(");
    }

    #[test]
    fn a_long_separator_grows_the_read() {
        let long = "x".repeat(5000);
        let tac = Tac::new(&settings(&["-s", &long])).unwrap();
        assert_eq!(tac.read_size, 16384);
        assert_eq!(tac.buf.len(), 2 * (16384 + 5000 + 1));
        assert_eq!(&tac.buf[..5000], long.as_bytes());
    }
}
