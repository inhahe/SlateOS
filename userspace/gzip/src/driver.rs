//! `gzip.c`: the command line, the files, and the signals -- everything
//! that touches the file system. Unix only.
//!
//! Upstream opens a file's directory (`atdir_set`) and then opens, creates
//! and removes names relative to it; so does this, through
//! `libcall::file`. Its `open` is gnulib's `open_safer`, which never
//! answers 0, 1 or 2; so is this one's.

// `ifname` and `ofname` are byte buffers upstream indexes freely; the
// indexes here are lengths and offsets already checked against them.

use crate::member::End;
use crate::{DEFLATED, ERROR, Gz, Io, MAX_PATH_LEN, OK, Res, Stop, WARNING, last_component, shown};
use getoptlong::{Opt, Program, Takes};
use libcall::file::{
    AT_FDCWD, Directory, O_CREAT, O_DIRECTORY, O_EXCL, O_NOCTTY, O_NOFOLLOW, O_NONBLOCK, O_RDONLY,
    O_WRONLY,
};
use libcall::signal::SigSet;
use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStringExt;
use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};

/// The version every banner names.
const VERSION: &str = "1.12";
/// The default suffix, and the longest a suffix may be.
const Z_SUFFIX: &[u8] = b".gz";
const MAX_SUFFIX: usize = 30;
/// The shortest part of a name `shorten_name` truncates.
const MIN_PART: usize = 3;

/// The signals gzip catches; `SIGINT` first, as `foreground` depends on it.
const SIGINT: i32 = 2;
const HANDLED: [i32; 6] = [SIGINT, 1, 13, 15, 24, 25];
/// `SIGPIPE`.
const SIGPIPE: i32 = 13;

/// `ENAMETOOLONG`, `EEXIST`, `ENOENT`, `EINVAL`, `EBADF`, `EAGAIN`.
const ENAMETOOLONG: i32 = 36;
const EEXIST: i32 = 17;
const ENOENT: i32 = 2;
const EINVAL: i32 = 22;
const EBADF: i32 = 9;
const EAGAIN: i32 = 11;

/// The output file the signal handler removes, and the signal that makes
/// it exit 2 rather than die: upstream's `remove_ofname_fd`,
/// `remove_ofname` and `exiting_signal`, which a handler reads.
static REMOVE_FD: AtomicI32 = AtomicI32::new(-1);
static REMOVE_NAME: [AtomicU8; MAX_PATH_LEN] = [const { AtomicU8::new(0) }; MAX_PATH_LEN];
static EXITING_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// `volatile_strcpy (remove_ofname, name)`.
fn set_remove_name(name: &[u8]) {
    for (i, cell) in REMOVE_NAME.iter().enumerate() {
        cell.store(name.get(i).copied().unwrap_or(0), Ordering::Relaxed);
        if i >= name.len() {
            break;
        }
    }
}

/// `remove_output_file`'s body, which a signal handler may run: close the
/// output and remove it. Only async-signal-safe calls.
fn remove_output_now() {
    let fd = REMOVE_FD.swap(-1, Ordering::SeqCst);
    if fd >= 0 {
        // A failure to close or remove is nobody's to report here: the
        // run is ending.
        let _ = libcall::fd::close(fd);
        let mut name = [0u8; MAX_PATH_LEN];
        for (slot, cell) in name.iter_mut().zip(REMOVE_NAME.iter()) {
            *slot = cell.load(Ordering::Relaxed);
        }
        name[MAX_PATH_LEN - 1] = 0;
        if let Ok(path) = std::ffi::CStr::from_bytes_until_nul(&name) {
            let _ = libcall::file::unlink(path);
        }
    }
}

/// `abort_gzip_signal`.
extern "C" fn abort_gzip_signal(sig: i32) {
    remove_output_now();
    if sig == EXITING_SIGNAL.load(Ordering::SeqCst) {
        libcall::process::exit_immediately(WARNING);
    }
    // Die of the signal, as the shell expects.
    let _ = libcall::signal::set_default(sig);
    let _ = libcall::signal::raise(sig);
}

/// A file's status, as `struct stat` gives what gzip reads of it.
#[derive(Debug, Clone, Copy, Default)]
struct Stat {
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    size: i64,
    atime: (i64, i64),
    mtime: (i64, i64),
}

const S_IFMT: u32 = 0o170_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFREG: u32 = 0o100_000;
const S_ISUID: u32 = 0o4000;
const S_ISGID: u32 = 0o2000;
const S_ISVTX: u32 = 0o1000;

impl Stat {
    fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
    fn is_reg(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }
}

/// `fstat (fd)`.
fn fstat(fd: i32) -> Result<Stat, i32> {
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::MetadataExt;
    // SAFETY: the descriptor is the caller's and stays open; the `File` is
    // never dropped, so it does not close it.
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    let md = file
        .metadata()
        .map_err(|e| e.raw_os_error().unwrap_or(EINVAL))?;
    Ok(Stat {
        mode: md.mode(),
        nlink: md.nlink(),
        uid: md.uid(),
        gid: md.gid(),
        size: md.size() as i64,
        atime: (md.atime(), md.atime_nsec()),
        mtime: (md.mtime(), md.mtime_nsec()),
    })
}

/// A descriptor's `File`, borrowed: the calls `std` already has.
fn with_file<T>(fd: i32, f: impl FnOnce(&std::fs::File) -> std::io::Result<T>) -> Result<T, i32> {
    use std::os::fd::FromRawFd;
    // SAFETY: as in `fstat`: borrowed, never closed here.
    let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    f(&file).map_err(|e| e.raw_os_error().unwrap_or(EINVAL))
}

/// `openat_safer (dir, name, flags, mode)`: as `openat`, but never one of
/// the standard descriptors -- a result below 3 is moved above them.
fn open_safer(dir: i32, name: &[u8], flags: i32, mode: u32) -> Result<i32, i32> {
    let Ok(cname) = CString::new(name) else {
        return Err(ENOENT);
    };
    let fd = libcall::file::openat(dir, &cname, flags, mode)?;
    if (0..=2).contains(&fd) {
        let moved = libcall::fd::dup_above(fd, 3);
        // The original is replaced either way; its close cannot matter.
        let _ = libcall::fd::close(fd);
        return moved;
    }
    Ok(fd)
}

/// The process's descriptors, as upstream reaches them.
#[derive(Default)]
pub struct RealIo {
    /// stdio's buffer for standard output.
    out: Vec<u8>,
    /// stdio's buffer for standard input, which `yesno` reads.
    stdin: Vec<u8>,
    stdin_at: usize,
    stdin_eof: bool,
}

impl RealIo {
    /// `fflush (stdout)`.
    fn flush_stdout(&mut self) -> Result<(), i32> {
        let out = std::mem::take(&mut self.out);
        if out.is_empty() {
            return Ok(());
        }
        libcall::fd::write_all(1, &out, false)
    }

    /// `getchar ()` on stdio's standard input: `None` at the end, which
    /// sticks, as glibc's has since 2.28.
    fn getchar(&mut self) -> Option<u8> {
        if self.stdin_at == self.stdin.len() {
            if self.stdin_eof {
                return None;
            }
            self.stdin.resize(4096, 0);
            match libcall::fd::read(0, &mut self.stdin) {
                Ok(n) if n > 0 => {
                    self.stdin.truncate(n);
                    self.stdin_at = 0;
                }
                _ => {
                    self.stdin.clear();
                    self.stdin_at = 0;
                    self.stdin_eof = true;
                    return None;
                }
            }
        }
        let c = self.stdin.get(self.stdin_at).copied();
        self.stdin_at += 1;
        c
    }
}

impl Io for RealIo {
    fn read(&mut self, fd: i32, buf: &mut [u8]) -> Result<usize, i32> {
        match libcall::fd::read(fd, buf) {
            Err(EAGAIN) => {
                // Input files are opened non-blocking; some file systems
                // then refuse a read. Clear the flag and read again.
                let flags = libcall::file::status_flags(fd)?;
                if flags & O_NONBLOCK == 0 {
                    return Err(EAGAIN);
                }
                libcall::file::set_status_flags(fd, flags & !O_NONBLOCK)?;
                libcall::fd::read(fd, buf)
            }
            r => r,
        }
    }

    fn write(&mut self, fd: i32, buf: &[u8]) -> Result<usize, i32> {
        libcall::fd::write(fd, buf)
    }

    fn diag(&mut self, text: &[u8]) {
        // stderr is unbuffered and its failures unreported, as `fprintf`'s.
        let _ = libcall::fd::write_all(2, text, false);
    }

    fn print(&mut self, text: &[u8]) {
        self.out.extend_from_slice(text);
    }

    fn strerror(&self, errno: i32) -> String {
        errmsg::strerror(&std::io::Error::from_raw_os_error(errno))
    }
}

/// The options `gzip` reads, as `getopt_long` sees them.
const SHORTOPTS: &str = "ab:cdfhH?klLmMnNqrS:tvVZ123456789";
const LONGOPTS: &[(&str, Takes)] = &[
    ("ascii", Takes::Nothing),
    ("to-stdout", Takes::Nothing),
    ("stdout", Takes::Nothing),
    ("decompress", Takes::Nothing),
    ("uncompress", Takes::Nothing),
    ("force", Takes::Nothing),
    ("help", Takes::Nothing),
    ("keep", Takes::Nothing),
    ("list", Takes::Nothing),
    ("license", Takes::Nothing),
    ("no-name", Takes::Nothing),
    ("name", Takes::Nothing),
    ("-presume-input-tty", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("synchronous", Takes::Nothing),
    ("recursive", Takes::Nothing),
    ("suffix", Takes::Required),
    ("test", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("version", Takes::Nothing),
    ("fast", Takes::Nothing),
    ("best", Takes::Nothing),
    ("lzw", Takes::Nothing),
    ("bits", Takes::Required),
    ("rsyncable", Takes::Nothing),
];

/// What an option is: its short letter, or one of the three long-only ones.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Code {
    Short(u8),
    PresumeInputTty,
    Rsyncable,
    Synchronous,
}

/// The long option `name`'s code.
fn long_code(name: &str) -> Code {
    Code::Short(match name {
        "ascii" => b'a',
        "to-stdout" | "stdout" => b'c',
        "decompress" | "uncompress" => b'd',
        "force" => b'f',
        "help" => b'h',
        "keep" => b'k',
        "list" => b'l',
        "license" => b'L',
        "no-name" => b'n',
        "name" => b'N',
        "-presume-input-tty" => return Code::PresumeInputTty,
        "quiet" | "silent" => b'q',
        "synchronous" => return Code::Synchronous,
        "recursive" => b'r',
        "suffix" => b'S',
        "test" => b't',
        "verbose" => b'v',
        "version" => b'V',
        "fast" => b'1',
        "best" => b'9',
        "lzw" => b'Z',
        "bits" => b'b',
        _ => return Code::Rsyncable,
    })
}

/// The run: upstream's globals in `gzip.c` that [`Gz`] does not hold.
struct Run {
    gz: Gz<RealIo>,
    /// `argv[0]` as given, which glibc's getopt diagnostics name.
    argv0: Vec<u8>,
    presume_input_tty: bool,
    synchronous: bool,
    ascii: bool,
    keep: bool,
    recursive: bool,
    foreground: bool,
    z_suffix: Vec<u8>,
    stdin_was_read: bool,
    total_in: i64,
    total_out: i64,
    istat: Stat,
    dfd: i32,
    dfname: Vec<u8>,
    first_time: bool,
    caught: SigSet,
}

/// Where the run goes after an option: on, or out with a status.
type Step = Result<(), Stop>;

impl Run {
    fn prog(&self) -> String {
        self.gz.prog()
    }

    fn diag(&mut self, text: &str) {
        self.gz.diag(text);
    }

    /// `try_help`.
    fn try_help(&mut self) -> Step {
        let text = format!("Try `{} --help' for more information.\n", self.prog());
        self.diag(&text);
        Err(Stop::Exit(ERROR))
    }

    /// `progerror (string)`.
    fn progerror(&mut self, s: &[u8], errno: i32) {
        let text = format!("{}: ", self.prog());
        self.diag(&text);
        self.gz.perror(s, errno);
        self.gz.exit_code = ERROR;
    }

    /// `help`.
    fn help(&mut self) {
        const HELP: &[&str] = &[
            "Compress or uncompress FILEs (by default, compress FILES in-place).",
            "",
            "Mandatory arguments to long options are mandatory for short options too.",
            "",
            "  -c, --stdout      write on standard output, keep original files unchanged",
            "  -d, --decompress  decompress",
            "  -f, --force       force overwrite of output file and compress links",
            "  -h, --help        give this help",
            "  -k, --keep        keep (don't delete) input files",
            "  -l, --list        list compressed file contents",
            "  -L, --license     display software license",
            "  -n, --no-name     do not save or restore the original name and timestamp",
            "  -N, --name        save or restore the original name and timestamp",
            "  -q, --quiet       suppress all warnings",
            "  -r, --recursive   operate recursively on directories",
            "      --rsyncable   make rsync-friendly archive",
            "  -S, --suffix=SUF  use suffix SUF on compressed files",
            "      --synchronous synchronous output (safer if system crashes, but slower)",
            "  -t, --test        test compressed file integrity",
            "  -v, --verbose     verbose mode",
            "  -V, --version     display version number",
            "  -1, --fast        compress faster",
            "  -9, --best        compress better",
            "",
            "With no FILE, or when FILE is -, read standard input.",
            "",
            "Report bugs to <bug-gzip@gnu.org>.",
        ];
        let mut text = b"Usage: ".to_vec();
        text.extend_from_slice(&self.gz.program_name);
        text.extend_from_slice(b" [OPTION]... [FILE]...\n");
        for line in HELP {
            text.extend_from_slice(line.as_bytes());
            text.push(b'\n');
        }
        self.gz.io.print(&text);
    }

    /// `license`.
    fn license(&mut self) {
        let mut text = self.gz.program_name.clone();
        text.extend_from_slice(format!(" {VERSION}\n").as_bytes());
        for line in [
            "Copyright (C) 2018 Free Software Foundation, Inc.",
            "Copyright (C) 1993 Jean-loup Gailly.",
            "This is free software.  You may redistribute copies of it under the terms of",
            "the GNU General Public License <https://www.gnu.org/licenses/gpl.html>.",
            "There is NO WARRANTY, to the extent permitted by law.",
        ] {
            text.extend_from_slice(line.as_bytes());
            text.push(b'\n');
        }
        self.gz.io.print(&text);
    }

    /// `version`.
    fn version(&mut self) {
        self.license();
        self.gz.io.print(b"\nWritten by Jean-loup Gailly.\n");
    }

    /// `finish_out`: close standard output and exit 0.
    fn finish_out(&mut self) -> Step {
        let flushed = self.gz.io.flush_stdout();
        let closed = libcall::fd::close(1);
        if let Err(e) = flushed.and(closed) {
            // `fclose (stdout)` failed: `write_error`, with `ofname` still
            // empty.
            return self.gz.write_error(e);
        }
        Err(Stop::Exit(OK))
    }

    /// One option: upstream's `switch (optc)`. `env` marks one from the
    /// `GZIP` variable, where only a few are allowed; `long` is the long
    /// name it was given by, if any.
    fn option(
        &mut self,
        code: Code,
        value: Option<OsString>,
        env: bool,
        long: Option<&str>,
    ) -> Step {
        if env {
            let allowed = match code {
                Code::Short(c) => matches!(c, b'n' | b'N' | b'q' | b'v' | b'1'..=b'9'),
                Code::Rsyncable => true,
                Code::PresumeInputTty | Code::Synchronous => false,
            };
            if !allowed {
                let what = match (long, code) {
                    (Some(name), _) => format!("--{name}: "),
                    (None, Code::Short(c)) => format!("-{}: ", char::from(c)),
                    (None, _) => String::new(),
                };
                let text = format!(
                    "{}: {what}option not valid in GZIP environment variable\n",
                    self.prog()
                );
                self.diag(&text);
                return self.try_help();
            }
        }
        match code {
            Code::PresumeInputTty => self.presume_input_tty = true,
            Code::Rsyncable => self.gz.rsync = true,
            Code::Synchronous => self.synchronous = true,
            Code::Short(c) => match c {
                b'a' => self.ascii = true,
                b'b' => {
                    let arg = value.map(OsString::into_vec).unwrap_or_default();
                    if arg.iter().any(|b| !b.is_ascii_digit()) {
                        let text = format!("{}: -b operand is not an integer\n", self.prog());
                        self.diag(&text);
                        return self.try_help();
                    }
                    // `atoi`: the digits' value, saturated where it would
                    // have overflowed.
                    let mut n: i32 = 0;
                    for &b in &arg {
                        n = n.saturating_mul(10).saturating_add(i32::from(b - b'0'));
                    }
                    self.gz.maxbits = n;
                }
                b'c' => self.gz.to_stdout = true,
                b'd' => self.gz.decompress = true,
                b'f' => self.gz.force += 1,
                b'h' | b'H' => {
                    self.help();
                    return self.finish_out();
                }
                b'k' => self.keep = true,
                b'l' => {
                    self.gz.list = true;
                    self.gz.decompress = true;
                    self.gz.test = true;
                    self.gz.to_stdout = true;
                }
                b'L' => {
                    self.license();
                    return self.finish_out();
                }
                b'm' => self.gz.no_time = 1,
                b'M' => self.gz.no_time = 0,
                b'n' => {
                    self.gz.no_name = 1;
                    self.gz.no_time = 1;
                }
                b'N' => {
                    self.gz.no_name = 0;
                    self.gz.no_time = 0;
                }
                b'q' => {
                    self.gz.quiet = true;
                    self.gz.verbose = 0;
                }
                b'r' => self.recursive = true,
                b'S' => self.z_suffix = value.map(OsString::into_vec).unwrap_or_default(),
                b't' => {
                    self.gz.test = true;
                    self.gz.decompress = true;
                    self.gz.to_stdout = true;
                }
                b'v' => {
                    self.gz.verbose += 1;
                    self.gz.quiet = false;
                }
                b'V' => {
                    self.version();
                    return self.finish_out();
                }
                b'Z' => {
                    let text = format!("{}: -Z not supported in this version\n", self.prog());
                    self.diag(&text);
                    return self.try_help();
                }
                b'1'..=b'9' => self.gz.level = i32::from(c - b'0'),
                _ => return self.try_help(),
            },
        }
        Ok(())
    }

    /// A getopt diagnostic: glibc's sentence, under `argv[0]` as given.
    fn getopt_error(&mut self, e: &getoptlong::Error) -> Step {
        let mut text = self.argv0.clone();
        text.extend_from_slice(b": ");
        text.extend_from_slice(e.sentence.as_bytes());
        text.push(b'\n');
        self.gz.io.diag(&text);
        self.try_help()
    }

    /// The `GZIP` variable's words, parsed first, as `add_envopt` and the
    /// option loop's first half do.
    fn parse_env(&mut self, words: &[OsString]) -> Step {
        const ENV: Program = Program::new("gzip", ERROR);
        let mut parser = ENV.parse(words, SHORTOPTS, LONGOPTS);
        let mut optind = 0usize;
        let mut first_operand: Option<OsString> = None;
        loop {
            // gzip's own test, before each call: a `--` where getopt would
            // look next is an option the variable may not hold.
            if words
                .get(optind)
                .is_some_and(|w| w.as_encoded_bytes() == b"--")
            {
                let text = format!(
                    "{}: --: option not valid in GZIP environment variable\n",
                    self.prog()
                );
                self.diag(&text);
                return self.try_help();
            }
            let item = loop {
                match parser.next() {
                    Some(Ok(Opt::Operand(w))) => {
                        if first_operand.is_none() {
                            first_operand = Some(w.clone());
                        }
                    }
                    other => break other,
                }
            };
            match item {
                None => break,
                Some(Err(e)) => return self.getopt_error(&e),
                Some(Ok(Opt::Short(c, value))) => {
                    optind = parser.optind();
                    self.option(Code::Short(c), value, true, None)?;
                }
                Some(Ok(Opt::Long(name, value))) => {
                    optind = parser.optind();
                    self.option(long_code(name), value, true, Some(name))?;
                }
                Some(Ok(Opt::Operand(_))) => {}
            }
        }
        if let Some(word) = first_operand {
            let mut text = format!("{}: ", self.prog()).into_bytes();
            text.extend_from_slice(shown(word.as_encoded_bytes()).as_bytes());
            text.extend_from_slice(b": non-option in GZIP environment variable\n");
            self.gz.io.diag(&text);
            return self.try_help();
        }
        if !self.gz.quiet {
            let text = format!(
                "{}: warning: GZIP environment variable is deprecated; use an alias or script\n",
                self.prog()
            );
            self.diag(&text);
        }
        Ok(())
    }

    /// The command line's words: the options, and the operands left.
    fn parse_args(&mut self, args: &[OsString]) -> Result<Vec<Vec<u8>>, Stop> {
        const GZIP: Program = Program::new("gzip", ERROR);
        let mut operands = Vec::new();
        for item in GZIP.parse(args, SHORTOPTS, LONGOPTS) {
            match item {
                Err(e) => {
                    self.getopt_error(&e)?;
                }
                Ok(Opt::Operand(w)) => operands.push(w.clone().into_vec()),
                Ok(Opt::Short(c, value)) => self.option(Code::Short(c), value, false, None)?,
                Ok(Opt::Long(name, value)) => {
                    self.option(long_code(name), value, false, Some(name))?;
                }
            }
        }
        Ok(operands)
    }

    /// `install_signal_handlers`.
    fn install_signal_handlers(&mut self) {
        let mut caught = SigSet::empty();
        for &sig in &HANDLED {
            if !libcall::signal::is_ignored(sig).unwrap_or(false) {
                // A signal number the library refuses is one it cannot
                // deliver either.
                let _ = caught.add(sig);
            }
        }
        for (i, &sig) in HANDLED.iter().enumerate() {
            if caught.contains(sig) {
                if i == 0 {
                    self.foreground = true;
                }
                // Installing fails only for a signal that cannot be caught,
                // which none of these is.
                let _ = libcall::signal::set_handler_masked(sig, abort_gzip_signal, &caught);
            }
        }
        self.caught = caught;
    }

    /// `sigprocmask (SIG_BLOCK, &caught_signals, &oldset)`.
    fn block(&self) -> Option<SigSet> {
        libcall::signal::block(&self.caught).ok()
    }

    /// `sigprocmask (SIG_SETMASK, &oldset, NULL)`.
    fn unblock(old: Option<SigSet>) {
        if let Some(old) = old {
            // Restoring a mask the library just gave us cannot fail.
            let _ = libcall::signal::set_mask(&old);
        }
    }

    /// `remove_output_file (false)`.
    fn remove_output_file(&self) {
        let old = self.block();
        remove_output_now();
        Self::unblock(old);
    }

    /// `get_input_size_and_time`.
    fn get_input_size_and_time(&mut self) {
        self.gz.ifile_size = -1;
        self.gz.time_stamp.1 = -1;
        if self.istat.is_reg() {
            self.gz.ifile_size = self.istat.size;
            if self.gz.no_time == 0 || self.gz.list {
                self.gz.time_stamp = self.istat.mtime;
            }
        }
    }

    /// `treat_stdin`.
    fn treat_stdin(&mut self) -> Res<()> {
        let tty_fd = if self.gz.decompress { 0 } else { 1 };
        if self.gz.force == 0
            && !self.gz.list
            && (self.presume_input_tty || libcall::fd::is_terminal(tty_fd))
        {
            if !self.gz.quiet {
                let p = self.prog();
                let (verb, de) = if self.gz.decompress {
                    ("read from", "de")
                } else {
                    ("written to", "")
                };
                let text = format!(
                    "{p}: compressed data not {verb} a terminal. Use -f to force {de}compression.\nFor help, type: {p} -h\n"
                );
                self.diag(&text);
            }
            return Err(Stop::Exit(ERROR));
        }

        self.gz.ifname = b"stdin".to_vec();
        self.gz.ofname = b"stdout".to_vec();

        match fstat(0) {
            Ok(st) => self.istat = st,
            Err(e) => {
                self.progerror(b"standard input", e);
                return Err(Stop::Exit(ERROR));
            }
        }
        self.get_input_size_and_time();

        self.gz.clear_bufs();
        self.gz.to_stdout = true;
        self.gz.part_nb = 0;
        self.gz.ifd = 0;
        self.stdin_was_read = true;

        if self.gz.decompress {
            let m = self.gz.get_method(0)?;
            self.gz.method = m;
            if m < 0 {
                return Err(Stop::Exit(self.gz.exit_code));
            }
        }

        if self.gz.members(0, 1)? != End::Eof {
            return Ok(());
        }

        if self.gz.list {
            self.do_list(self.gz.method);
            return Ok(());
        }

        if self.gz.verbose != 0 {
            if self.gz.test {
                self.diag(" OK\n");
            } else if !self.gz.decompress {
                let gz = &self.gz;
                let r = Gz::<RealIo>::ratio(
                    gz.bytes_in - (gz.bytes_out - gz.header_bytes),
                    gz.bytes_in,
                );
                self.diag(&format!("{r}\n"));
            }
        }
        Ok(())
    }

    /// `atdir_eq (dir, dirlen)`.
    fn atdir_eq(&self, dir: &[u8]) -> bool {
        let dir = if dir.is_empty() { &b"."[..] } else { dir };
        self.dfname == dir
    }

    /// `atdir_set (dir, dirlen)`: the cached descriptor for the directory
    /// `dir`, opening it if it is not the one cached; -1 if it cannot be.
    fn atdir_set(&mut self, dir: &[u8]) -> i32 {
        if !self.atdir_eq(dir) {
            if self.dfd >= 0 {
                // Replacing the cache; a failed close loses nothing.
                let _ = libcall::fd::close(self.dfd);
            }
            let dir = if dir.is_empty() { &b"."[..] } else { dir };
            self.dfname = dir.to_vec();
            self.dfd = open_safer(AT_FDCWD, dir, O_RDONLY | O_DIRECTORY, 0).unwrap_or(-1);
        }
        self.dfd
    }

    /// `open_and_stat (name, flags, &st)`.
    fn open_and_stat(&mut self, name: &[u8], mut flags: i32) -> Result<(i32, Stat), i32> {
        let mut atfd = AT_FDCWD;
        let mut base = name;
        if !self.gz.to_stdout && self.gz.force == 0 {
            flags |= O_NOFOLLOW;
        }
        if !self.keep {
            let b = last_component(name);
            let f = self.atdir_set(&name[..b]);
            if f >= 0 {
                base = &name[b..];
                atfd = f;
            }
        }
        let fd = open_safer(atfd, base, flags, 0)?;
        match fstat(fd) {
            Ok(st) => Ok((fd, st)),
            Err(e) => {
                let _ = libcall::fd::close(fd);
                Err(e)
            }
        }
    }

    /// `get_suffix (name)`: where a known compressed suffix starts in
    /// `name`, if one ends it.
    fn get_suffix(&self, name: &[u8]) -> Option<usize> {
        const BUILTIN: [&[u8]; 7] = [b".gz", b".z", b".taz", b".tgz", b"-gz", b"-z", b"_z"];
        let z = &self.z_suffix;
        let suffix_of_builtin = BUILTIN.iter().any(|s| z.len() < s.len() && s.ends_with(z));
        let z_lower = z.to_ascii_lowercase();
        let mut known: Vec<&[u8]> = Vec::with_capacity(8);
        if !suffix_of_builtin {
            known.push(&z_lower);
        }
        known.extend_from_slice(&BUILTIN);
        if suffix_of_builtin {
            known.push(&z_lower);
        }

        let nlen = name.len();
        let tail = if nlen <= MAX_SUFFIX + 2 {
            name
        } else {
            &name[nlen - MAX_SUFFIX - 2..]
        };
        let suffix = tail.to_ascii_lowercase();
        let slen = suffix.len();
        for s in known {
            let sl = s.len();
            if slen > sl && suffix[slen - sl - 1] != b'/' && &suffix[slen - sl..] == s {
                return Some(nlen - sl);
            }
        }
        None
    }

    /// `open_input_file (iname, &istat)`: open the input, trying the
    /// compressed suffixes when decompressing a name that does not exist.
    fn open_input_file(&mut self, iname: &[u8]) -> Option<i32> {
        let open_flags = O_RDONLY | O_NONBLOCK | O_NOCTTY;
        if MAX_PATH_LEN - 1 <= iname.len() {
            return self.name_too_long(iname);
        }
        self.gz.ifname = iname.to_vec();

        let first = self.open_and_stat(iname, open_flags);
        let err = match first {
            Ok((fd, st)) => {
                self.istat = st;
                return Some(fd);
            }
            Err(e) => e,
        };
        if !self.gz.decompress || err != ENOENT {
            let name = self.gz.ifname.clone();
            self.progerror(&name, err);
            return None;
        }
        if self.get_suffix(iname).is_some() {
            let name = self.gz.ifname.clone();
            self.progerror(&name, err);
            return None;
        }
        let ilen = iname.len();
        let mut suffixes: Vec<Vec<u8>> = vec![self.z_suffix.clone()];
        for s in [&b".gz"[..], b".z", b"-z", b".Z"] {
            suffixes.push(s.to_vec());
        }
        let start = usize::from(self.z_suffix == b".gz");
        let mut z_suffix_errno = 0;
        for s in suffixes.iter().skip(start) {
            if MAX_PATH_LEN <= ilen + s.len() {
                return self.name_too_long(iname);
            }
            let mut name = iname.to_vec();
            name.extend_from_slice(s);
            self.gz.ifname.clone_from(&name);
            match self.open_and_stat(&name, open_flags) {
                Ok((fd, st)) => {
                    self.istat = st;
                    return Some(fd);
                }
                Err(e) if e != ENOENT => {
                    self.progerror(&name, e);
                    return None;
                }
                Err(e) => {
                    if *s == self.z_suffix {
                        z_suffix_errno = e;
                    }
                }
            }
        }
        let mut name = iname.to_vec();
        name.extend_from_slice(&self.z_suffix);
        self.gz.ifname.clone_from(&name);
        self.progerror(&name, z_suffix_errno);
        None
    }

    /// `open_input_file`'s `name_too_long`.
    fn name_too_long(&mut self, iname: &[u8]) -> Option<i32> {
        let text = format!("{}: {}: file name too long\n", self.prog(), shown(iname));
        self.diag(&text);
        self.gz.exit_code = ERROR;
        None
    }

    /// `make_ofname`: the output's name from the input's. `OK`, or
    /// `WARNING` for a file to skip.
    fn make_ofname(&mut self) -> i32 {
        self.gz.ofname = self.gz.ifname.clone();
        let suff = self.get_suffix(&self.gz.ofname);

        if self.gz.decompress {
            let Some(at) = suff else {
                if !self.recursive && self.gz.test {
                    return OK;
                }
                if self.gz.verbose != 0 || (!self.recursive && !self.gz.quiet) {
                    let text = format!(
                        "{}: {}: unknown suffix -- ignored\n",
                        self.prog(),
                        shown(&self.gz.ifname)
                    );
                    self.gz.warn(&text);
                }
                return WARNING;
            };
            self.gz.ofname[at..].make_ascii_lowercase();
            let ext = self.gz.ofname[at..].to_vec();
            self.gz.ofname.truncate(at);
            if ext == b".tgz" || ext == b".taz" {
                self.gz.ofname.extend_from_slice(b".tar");
            }
        } else if let (Some(at), 0) = (suff, self.gz.force) {
            if self.gz.verbose != 0 || (!self.recursive && !self.gz.quiet) {
                let text = format!(
                    "{}: {} already has {} suffix -- unchanged\n",
                    self.prog(),
                    shown(&self.gz.ifname),
                    shown(&self.gz.ofname[at..])
                );
                self.diag(&text);
            }
            return WARNING;
        } else {
            self.gz.save_orig_name = false;
            if MAX_PATH_LEN <= self.gz.ofname.len() + self.z_suffix.len() {
                let text = format!(
                    "{}: {}: file name too long\n",
                    self.prog(),
                    shown(&self.gz.ifname)
                );
                self.gz.warn(&text);
                return WARNING;
            }
            let z = self.z_suffix.clone();
            self.gz.ofname.extend_from_slice(&z);
        }
        OK
    }

    /// `do_list (method)`: a line of `-l`, or the totals for a negative
    /// method.
    fn do_list(&mut self, method: i32) {
        const METHODS: [&str; 9] = ["store", "compr", "pack ", "lzh  ", "", "", "", "", "defla"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        const W: usize = 19;
        type G = Gz<RealIo>;
        let mut out = String::new();

        if self.first_time && method >= 0 {
            self.first_time = false;
            if self.gz.verbose != 0 {
                out.push_str("method  crc     date  time  ");
            }
            if !self.gz.quiet {
                out.push_str(&format!(
                    "{:>W$} {:>W$}  ratio uncompressed_name\n",
                    "compressed", "uncompressed"
                ));
            }
        } else if method < 0 {
            if self.total_in <= 0 || self.total_out <= 0 {
                return;
            }
            if self.gz.verbose != 0 {
                out.push_str("                            ");
            }
            if self.gz.verbose != 0 || !self.gz.quiet {
                out.push_str(&G::off(self.total_in, W));
                out.push(' ');
                out.push_str(&G::off(self.total_out, W));
                out.push(' ');
            }
            out.push_str(&G::ratio(
                self.total_out - (self.total_in - self.gz.header_bytes),
                self.total_out,
            ));
            out.push_str(" (totals)\n");
            self.gz.io.print(out.as_bytes());
            return;
        }

        let mut crc: u64 = !0;
        if method == DEFLATED && !self.gz.last_member {
            crc = u64::from(self.gz.unzip_crc);
        }

        if self.gz.verbose != 0 {
            let name = METHODS.get(method as usize).copied().unwrap_or("");
            out.push_str(&format!("{name:>5} {crc:08x} "));
            match libcall::clock::local_at(self.gz.time_stamp.0) {
                Ok(tm) => {
                    let mon = MONTHS.get(tm.mon as usize).copied().unwrap_or("???");
                    out.push_str(&format!("{mon}{:3} {:02}:{:02} ", tm.mday, tm.hour, tm.min));
                }
                Err(_) => out.push_str("??? ?? ??:?? "),
            }
        }
        let gz = &mut self.gz;
        out.push_str(&G::off(gz.bytes_in, W));
        out.push(' ');
        out.push_str(&G::off(gz.bytes_out, W));
        out.push(' ');
        if gz.bytes_in == -1 {
            self.total_in = -1;
            gz.bytes_in = 0;
            gz.bytes_out = 0;
            gz.header_bytes = 0;
        } else if self.total_in >= 0 {
            self.total_in += gz.bytes_in;
        }
        if gz.bytes_out == -1 {
            self.total_out = -1;
            gz.bytes_in = 0;
            gz.bytes_out = 0;
            gz.header_bytes = 0;
        } else if self.total_out >= 0 {
            self.total_out += gz.bytes_out;
        }
        out.push_str(&G::ratio(
            gz.bytes_out - (gz.bytes_in - gz.header_bytes),
            gz.bytes_out,
        ));
        out.push(' ');
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(&gz.ofname);
        bytes.push(b'\n');
        gz.io.print(&bytes);
    }

    /// `shorten_name (name)`: make the output's name one shorter, for a
    /// file system that refused it.
    fn shorten_name(&mut self) -> Res<()> {
        let len = self.gz.ofname.len();
        if self.gz.decompress {
            if len <= 1 {
                return self.gz.gzip_error("name too short");
            }
            self.gz.ofname.truncate(len - 1);
            return Ok(());
        }
        let Some(p) = self.get_suffix(&self.gz.ofname) else {
            return self.gz.gzip_error("can't recover suffix\n");
        };
        self.gz.ofname.truncate(p);
        self.gz.save_orig_name = true;

        if len > 4 && p >= 4 && &self.gz.ofname[p - 4..p] == b".tar" {
            self.gz.ofname.truncate(p - 4);
            self.gz.ofname.extend_from_slice(b".tgz");
            return Ok(());
        }

        let name = &self.gz.ofname;
        let base = last_component(name);
        let mut trunc: Option<usize> = None;
        let mut min_part = MIN_PART;
        loop {
            let mut at = base;
            while at < name.len() {
                let plen = name[at..]
                    .iter()
                    .position(|&b| b == b'.')
                    .unwrap_or(name.len() - at);
                at += plen;
                if plen > min_part {
                    trunc = Some(at - 1);
                }
                if at < name.len() {
                    at += 1;
                }
            }
            if trunc.is_some() {
                break;
            }
            min_part -= 1;
            if min_part == 0 {
                break;
            }
        }

        let cut = if let Some(t) = trunc {
            self.gz.ofname.remove(t);
            self.gz.ofname.len()
        } else {
            let Some(mut t) = self.gz.ofname.iter().rposition(|&b| b == b'.') else {
                return self.gz.gzip_error("internal error in shorten_name");
            };
            if t + 1 == self.gz.ofname.len() {
                t = t.saturating_sub(1);
            }
            t
        };
        self.gz.ofname.truncate(cut);
        let z = self.z_suffix.clone();
        self.gz.ofname.extend_from_slice(&z);
        Ok(())
    }

    /// `check_ofname`: the output exists; ask, or with `-f` remove it.
    /// `OK` to go on, `ERROR` to skip the file.
    fn check_ofname(&mut self) -> i32 {
        if self.gz.force == 0 {
            let mut ok = false;
            let mut text = format!("{}: ", self.prog()).into_bytes();
            text.extend_from_slice(shown(&self.gz.ofname).as_bytes());
            text.extend_from_slice(b" already exists;");
            self.gz.io.diag(&text);
            if self.foreground && (self.presume_input_tty || libcall::fd::is_terminal(0)) {
                self.diag(" do you wish to overwrite (y or n)? ");
                // `yesno`: the first character of a line.
                let c = self.gz.io.getchar();
                ok = matches!(c, Some(b'y' | b'Y'));
                let mut c = c;
                while !matches!(c, Some(b'\n') | None) {
                    c = self.gz.io.getchar();
                }
            }
            if !ok {
                self.diag("\tnot overwritten\n");
                if self.gz.exit_code == OK {
                    self.gz.exit_code = WARNING;
                }
                return ERROR;
            }
        }
        let name = self.gz.ofname.clone();
        if let Err(e) = unlink_bytes(&name) {
            self.progerror(&name, e);
            return ERROR;
        }
        OK
    }

    /// `create_outfile`: create the output, shortening its name or asking
    /// about an existing one as needed. `OK` or `ERROR` (input closed).
    fn create_outfile(&mut self) -> Res<i32> {
        let mut name_shortened = false;
        let flags = O_WRONLY | O_CREAT | O_EXCL;
        let mut base = 0usize;
        let mut atfd = AT_FDCWD;

        if !self.keep {
            let b = last_component(&self.gz.ofname);
            let dir = self.gz.ofname[..b].to_vec();
            let f = self.atdir_set(&dir);
            if f >= 0 {
                base = b;
                atfd = f;
            }
        }

        loop {
            set_remove_name(&self.gz.ofname);
            let old = self.block();
            let opened = open_safer(atfd, &self.gz.ofname[base..], flags, 0o600);
            let fd = opened.unwrap_or(-1);
            self.gz.ofd = fd;
            REMOVE_FD.store(fd, Ordering::SeqCst);
            Self::unblock(old);

            let e = match opened {
                Ok(_) => break,
                Err(e) => e,
            };
            match e {
                ENAMETOOLONG => {
                    self.shorten_name()?;
                    name_shortened = true;
                }
                EEXIST => {
                    if self.check_ofname() != OK {
                        let _ = libcall::fd::close(self.gz.ifd);
                        return Ok(ERROR);
                    }
                }
                _ => {
                    let name = self.gz.ofname.clone();
                    self.progerror(&name, e);
                    let _ = libcall::fd::close(self.gz.ifd);
                    return Ok(ERROR);
                }
            }
        }

        if name_shortened && self.gz.decompress {
            let text = format!(
                "{}: {}: warning, name truncated\n",
                self.prog(),
                shown(&self.gz.ofname)
            );
            self.gz.warn(&text);
        }
        Ok(OK)
    }

    /// `copy_stat (&istat)`: the input's times, group, mode and owner, onto
    /// the output.
    fn copy_stat(&mut self) {
        let st = self.istat;
        let mode = st.mode & 0o777;
        let ofd = self.gz.ofd;
        let mut mtime = st.mtime;
        let restoring =
            self.gz.decompress && 0 <= self.gz.time_stamp.1 && mtime != self.gz.time_stamp;
        if restoring {
            mtime = self.gz.time_stamp;
        }
        let set =
            if (0..1_000_000_000).contains(&st.atime.1) && (0..1_000_000_000).contains(&mtime.1) {
                libcall::file::set_times(ofd, st.atime, mtime)
            } else {
                Err(EINVAL)
            };
        match set {
            Ok(()) => {
                if restoring && 1 < self.gz.verbose {
                    let text = format!("{}: timestamp restored\n", shown(&self.gz.ofname));
                    self.diag(&text);
                }
            }
            Err(e) => self.warn_perror(e),
        }

        // Group, then mode, then owner: the mode is right on systems that
        // let a user give a file away, without a window for set-id bits.
        let _ = with_file(ofd, |f| std::os::unix::fs::fchown(f, None, Some(st.gid)));
        let chmod = with_file(ofd, |f| {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(mode))
        });
        if let Err(e) = chmod {
            self.warn_perror(e);
        }
        let _ = with_file(ofd, |f| std::os::unix::fs::fchown(f, Some(st.uid), None));
    }

    /// `WARN ((stderr, "%s: ", program_name)); if (!quiet) perror (ofname)`.
    fn warn_perror(&mut self, e: i32) {
        let text = format!("{}: ", self.prog());
        self.gz.warn(&text);
        if !self.gz.quiet {
            let name = self.gz.ofname.clone();
            self.gz.perror(&name, e);
        }
    }

    /// `treat_dir (fd, dir)`: every entry of the directory open on `fd`.
    fn treat_dir(&mut self, fd: i32, dir: &[u8]) -> Res<()> {
        let mut stream = match Directory::from_fd(fd) {
            Ok(s) => s,
            Err(e) => {
                self.progerror(dir, e);
                let _ = libcall::fd::close(fd);
                return Ok(());
            }
        };
        let mut entries: Option<Vec<Vec<u8>>> = Some(Vec::new());
        let mut buf = [0u8; 256];
        while let Some(next) = stream.next_name(&mut buf) {
            match next {
                Ok(name) => {
                    if name != b"." && name != b".." {
                        if let Some(list) = entries.as_mut() {
                            list.push(name.to_vec());
                        }
                    }
                }
                Err(e) => {
                    self.progerror(dir, e);
                    entries = None;
                    break;
                }
            }
        }
        if let Err(e) = stream.close() {
            self.progerror(dir, e);
        }
        let Some(entries) = entries else {
            return Ok(());
        };

        for entry in entries {
            let len = dir.len();
            if len + entry.len() < MAX_PATH_LEN - 2 {
                let mut nbuf = dir.to_vec();
                if last_component(&nbuf) < nbuf.len() && nbuf[len - 1] != b'/' {
                    nbuf.push(b'/');
                }
                nbuf.extend_from_slice(&entry);
                self.treat_file(&nbuf)?;
            } else {
                let text = format!(
                    "{}: {}/{}: pathname too long\n",
                    self.prog(),
                    shown(dir),
                    shown(&entry)
                );
                self.diag(&text);
                self.gz.exit_code = ERROR;
            }
        }
        Ok(())
    }

    /// `treat_file (iname)`.
    fn treat_file(&mut self, iname: &[u8]) -> Res<()> {
        if iname == b"-" {
            let cflag = self.gz.to_stdout;
            self.treat_stdin()?;
            self.gz.to_stdout = cflag;
            return Ok(());
        }

        let Some(ifd) = self.open_input_file(iname) else {
            return Ok(());
        };
        self.gz.ifd = ifd;

        if self.istat.is_dir() {
            if self.recursive {
                return self.treat_dir(ifd, iname);
            }
            let _ = libcall::fd::close(ifd);
            let text = format!(
                "{}: {} is a directory -- ignored\n",
                self.prog(),
                shown(&self.gz.ifname)
            );
            self.gz.warn(&text);
            return Ok(());
        }

        if !self.gz.to_stdout {
            let refuse = if !self.istat.is_reg() {
                Some(" is not a directory or a regular file - ignored".to_string())
            } else if self.istat.mode & S_ISUID != 0 {
                Some(" is set-user-ID on execution - ignored".to_string())
            } else if self.istat.mode & S_ISGID != 0 {
                Some(" is set-group-ID on execution - ignored".to_string())
            } else if self.gz.force == 0 && self.istat.mode & S_ISVTX != 0 {
                Some(" has the sticky bit set - file ignored".to_string())
            } else if self.gz.force == 0 && 2 <= self.istat.nlink {
                let others = self.istat.nlink - 1;
                Some(format!(
                    " has {others} other link{} -- file ignored",
                    if self.istat.nlink == 2 { "" } else { "s" }
                ))
            } else {
                None
            };
            if let Some(why) = refuse {
                let text = format!("{}: {}{why}\n", self.prog(), shown(&self.gz.ifname));
                self.gz.warn(&text);
                let _ = libcall::fd::close(ifd);
                return Ok(());
            }
        }

        self.get_input_size_and_time();

        if self.gz.to_stdout && !self.gz.test {
            self.gz.ofname = b"stdout".to_vec();
        } else if self.make_ofname() != OK {
            let _ = libcall::fd::close(ifd);
            return Ok(());
        }

        self.gz.clear_bufs();
        self.gz.part_nb = 0;

        if self.gz.decompress {
            let m = self.gz.get_method(ifd)?;
            self.gz.method = m;
            if m < 0 {
                let _ = libcall::fd::close(ifd);
                return Ok(());
            }
        }

        if self.gz.to_stdout {
            self.gz.ofd = 1;
        } else {
            if self.create_outfile()? != OK {
                return Ok(());
            }
            if !self.gz.decompress
                && self.gz.save_orig_name
                && self.gz.verbose == 0
                && !self.gz.quiet
            {
                let text = format!(
                    "{}: {} compressed to {}\n",
                    self.prog(),
                    shown(&self.gz.ifname),
                    shown(&self.gz.ofname)
                );
                self.diag(&text);
            }
        }
        if !self.gz.save_orig_name {
            self.gz.save_orig_name = self.gz.no_name == 0;
        }

        if self.gz.verbose != 0 && !self.gz.list {
            let text = format!("{}:\t", shown(&self.gz.ifname));
            self.diag(&text);
        }

        let ofd = self.gz.ofd;
        if self.gz.members(ifd, ofd)? == End::WorkFailed {
            // Force the clean-up below.
            self.gz.method = -1;
        }
        let method = self.gz.method;

        if let Err(e) = libcall::fd::close(ifd) {
            return self.gz.read_error(e);
        }

        if self.gz.list {
            self.do_list(method);
            return Ok(());
        }

        if !self.gz.to_stdout {
            self.copy_stat();

            let ofd = self.gz.ofd;
            let mut failed: Option<i32> = None;
            if self.synchronous {
                if self.dfd >= 0 {
                    if let Err(e) = with_file(self.dfd, std::fs::File::sync_data) {
                        if e != EINVAL {
                            failed = Some(e);
                        }
                    }
                }
                if failed.is_none() {
                    if let Err(e) = with_file(ofd, std::fs::File::sync_all) {
                        if e != EINVAL {
                            failed = Some(e);
                        }
                    }
                }
            }
            if failed.is_none() {
                if let Err(e) = libcall::fd::close(ofd) {
                    failed = Some(e);
                }
            }
            if let Some(e) = failed {
                return self.gz.write_error(e);
            }

            if !self.keep {
                let ifname = self.gz.ifname.clone();
                let base = last_component(&ifname);
                let ufd = if self.atdir_eq(&ifname[..base]) {
                    self.dfd
                } else {
                    -1
                };
                let old = self.block();
                REMOVE_FD.store(-1, Ordering::SeqCst);
                let res = if ufd < 0 {
                    unlink_bytes(&ifname)
                } else {
                    match CString::new(&ifname[base..]) {
                        Ok(c) => libcall::file::unlinkat(ufd, &c, 0),
                        Err(_) => Err(ENOENT),
                    }
                };
                Self::unblock(old);
                if let Err(e) = res {
                    let text = format!("{}: ", self.prog());
                    self.gz.warn(&text);
                    if !self.gz.quiet {
                        self.gz.perror(&ifname, e);
                    }
                }
            }
        }

        if method == -1 {
            if !self.gz.to_stdout {
                self.remove_output_file();
            }
            return Ok(());
        }

        if self.gz.verbose != 0 {
            let gz = &self.gz;
            let mut text = if gz.test {
                " OK".to_string()
            } else if gz.decompress {
                Gz::<RealIo>::ratio(gz.bytes_out - (gz.bytes_in - gz.header_bytes), gz.bytes_out)
            } else {
                Gz::<RealIo>::ratio(gz.bytes_in - (gz.bytes_out - gz.header_bytes), gz.bytes_in)
            };
            if !gz.test {
                text.push_str(&format!(
                    " -- {} {}",
                    if self.keep {
                        "created"
                    } else {
                        "replaced with"
                    },
                    shown(&gz.ofname)
                ));
            }
            text.push('\n');
            self.diag(&text);
        }
        Ok(())
    }

    /// `main`, after the arguments are words.
    fn main(&mut self, args: &[OsString]) -> Res<i32> {
        if let Some(env) = std::env::var_os("GZIP") {
            let bytes = env.into_vec();
            let words: Vec<OsString> = bytes
                .split(|&b| b == b' ' || b == b'\t')
                .filter(|w| !w.is_empty())
                .map(|w| OsString::from_vec(w.to_vec()))
                .collect();
            if !words.is_empty() {
                self.parse_env(&words)?;
            }
        }
        let operands = self.parse_args(args)?;

        let gz = &mut self.gz;
        if gz.no_time < 0 {
            gz.no_time = i32::from(gz.decompress);
        }
        if gz.no_name < 0 {
            gz.no_name = i32::from(gz.decompress);
        }

        if self.ascii && !self.gz.quiet {
            let text = format!("{}: option --ascii ignored on this system\n", self.prog());
            self.diag(&text);
        }
        if self.z_suffix.is_empty() || self.z_suffix.len() > MAX_SUFFIX {
            let mut text = format!("{}: invalid suffix '", self.prog()).into_bytes();
            text.extend_from_slice(shown(&self.z_suffix).as_bytes());
            text.extend_from_slice(b"'\n");
            self.gz.io.diag(&text);
            return Err(Stop::Exit(ERROR));
        }

        EXITING_SIGNAL.store(if self.gz.quiet { SIGPIPE } else { 0 }, Ordering::SeqCst);
        self.install_signal_handlers();

        if operands.is_empty() {
            self.treat_stdin()?;
        } else {
            for name in &operands {
                self.treat_file(name)?;
            }
        }
        if self.stdin_was_read {
            if let Err(e) = libcall::fd::close(0) {
                self.gz.ifname = b"stdin".to_vec();
                return self.gz.read_error(e);
            }
        }
        if self.gz.list {
            if !self.gz.quiet && 1 < operands.len() {
                self.do_list(-1);
            }
            if let Err(e) = self.gz.io.flush_stdout() {
                return self.gz.write_error(e);
            }
        }
        if self.gz.to_stdout {
            let mut failed = None;
            if self.synchronous {
                if let Err(e) = with_file(1, std::fs::File::sync_data) {
                    if e != EINVAL {
                        failed = Some(e);
                    }
                }
            }
            if failed.is_none() {
                if let Err(e) = libcall::fd::close(1) {
                    failed = Some(e);
                }
            }
            if let Some(e) = failed {
                if e != EBADF {
                    return self.gz.write_error(e);
                }
            }
        }
        Ok(self.gz.exit_code)
    }
}

/// `xunlink (name)`.
fn unlink_bytes(name: &[u8]) -> Result<(), i32> {
    match CString::new(name) {
        Ok(c) => libcall::file::unlink(&c),
        Err(_) => Err(ENOENT),
    }
}

/// The scripts Ubuntu installs as `gunzip` (and `uncompress`) and `zcat`:
/// their help and version texts, the line each `printf` is on, and the
/// option they put before the arguments.
struct Script {
    name: &'static str,
    usage: &'static str,
    help_line: u32,
    gzip_option: &'static str,
}

const GUNZIP: Script = Script {
    name: "gunzip",
    usage: "Uncompress FILEs (by default, in-place).

Mandatory arguments to long options are mandatory for short options too.

  -c, --stdout      write on standard output, keep original files unchanged
  -f, --force       force overwrite of output file and compress links
  -k, --keep        keep (don't delete) input files
  -l, --list        list compressed file contents
  -n, --no-name     do not save or restore the original name and timestamp
  -N, --name        save or restore the original name and timestamp
  -q, --quiet       suppress all warnings
  -r, --recursive   operate recursively on directories
  -S, --suffix=SUF  use suffix SUF on compressed files
      --synchronous synchronous output (safer if system crashes, but slower)
  -t, --test        test compressed file integrity
  -v, --verbose     verbose mode
      --help        display this help and exit
      --version     display version information and exit

With no FILE, or when FILE is -, read standard input.

Report bugs to <bug-gzip@gnu.org>.",
    help_line: 53,
    gzip_option: "-d",
};

const ZCAT: Script = Script {
    name: "zcat",
    usage: "Uncompress FILEs to standard output.

  -f, --force       force; read compressed data even from a terminal
  -l, --list        list compressed file contents
  -q, --quiet       suppress all warnings
  -r, --recursive   operate recursively on directories
  -S, --suffix=SUF  use suffix SUF on compressed files
      --synchronous synchronous output (safer if system crashes, but slower)
  -t, --test        test compressed file integrity
  -v, --verbose     verbose mode
      --help        display this help and exit
      --version     display version information and exit

With no FILE, or when FILE is -, read standard input.

Report bugs to <bug-gzip@gnu.org>.",
    help_line: 47,
    gzip_option: "-cd",
};

/// The script's `$0`: the path the program was run by -- `argv[0]` when
/// it names one, else where `PATH` finds it, as the shell that ran the
/// script found it.
fn script_path(argv0: &[u8]) -> Vec<u8> {
    if argv0.contains(&b'/') {
        return argv0.to_vec();
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in path.into_vec().split(|&b| b == b':') {
            let mut candidate = if dir.is_empty() {
                b".".to_vec()
            } else {
                dir.to_vec()
            };
            candidate.push(b'/');
            candidate.extend_from_slice(argv0);
            let p = std::path::PathBuf::from(OsString::from_vec(candidate.clone()));
            let runnable = std::fs::metadata(&p).is_ok_and(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.is_file() && m.permissions().mode() & 0o111 != 0
            });
            if runnable {
                return candidate;
            }
        }
    }
    argv0.to_vec()
}

/// A script's own `--help` or `--version`: `printf '%s\n' "$text" || exit
/// 1; exit`, and dash's message when the write fails.
fn script_text(script: &Script, dollar0: &[u8], version: bool) -> i32 {
    let text = if version {
        format!(
            "{} (gzip) {VERSION}\nCopyright (C) 2007, 2011-2018 Free Software Foundation, Inc.\n\
             This is free software.  You may redistribute copies of it under the terms of\n\
             the GNU General Public License <https://www.gnu.org/licenses/gpl.html>.\n\
             There is NO WARRANTY, to the extent permitted by law.\n\nWritten by Paul Eggert.\n",
            script.name
        )
        .into_bytes()
    } else {
        let mut t = b"Usage: ".to_vec();
        t.extend_from_slice(dollar0);
        t.extend_from_slice(b" [OPTION]... [FILE]...\n");
        t.extend_from_slice(script.usage.as_bytes());
        t.push(b'\n');
        t
    };
    if libcall::fd::write_all(1, &text, false).is_ok() {
        return OK;
    }
    let line = script.help_line + u32::from(version);
    let mut msg = dollar0.to_vec();
    msg.extend_from_slice(format!(": {line}: printf: printf: I/O error\n").as_bytes());
    let _ = libcall::fd::write_all(2, &msg, false);
    ERROR
}

/// Run `gzip` with `argv`, the program's own words. The exit status; a
/// caught signal ends the process before this returns.
#[must_use]
pub fn run(argv: Vec<OsString>) -> i32 {
    let argv0 = argv
        .first()
        .map(|a| a.as_encoded_bytes().to_vec())
        .unwrap_or_default();
    let mut name = argv0[last_component(&argv0)..].to_vec();
    if name.len() > 4 && name.ends_with(b".exe") {
        name.truncate(name.len() - 4);
    }
    let rest: Vec<OsString> = argv.into_iter().skip(1).collect();

    // `gunzip`, `uncompress` and `zcat` are Ubuntu's scripts, which run
    // `gzip` by that name: its messages say `gzip`.
    let script = match name.as_slice() {
        b"gunzip" | b"uncompress" => Some(&GUNZIP),
        b"zcat" => Some(&ZCAT),
        _ => None,
    };
    let (argv0, program_name, args) = match script {
        Some(script) => {
            match rest.first().map(|a| a.as_encoded_bytes()) {
                Some(b"--help") => return script_text(script, &script_path(&argv0), false),
                Some(b"--version") => return script_text(script, &script_path(&argv0), true),
                _ => {}
            }
            let mut args = vec![OsString::from(script.gzip_option)];
            args.extend(rest);
            (b"gzip".to_vec(), b"gzip".to_vec(), args)
        }
        None => (argv0, name, rest),
    };

    let mut run = Run {
        gz: Gz::new(RealIo::default(), program_name),
        argv0,
        presume_input_tty: false,
        synchronous: false,
        ascii: false,
        keep: false,
        recursive: false,
        foreground: false,
        z_suffix: Z_SUFFIX.to_vec(),
        stdin_was_read: false,
        total_in: 0,
        total_out: 0,
        istat: Stat::default(),
        dfd: -1,
        dfname: Vec::new(),
        first_time: true,
        caught: SigSet::empty(),
    };
    let status = match run.main(&args) {
        Ok(status) | Err(Stop::Exit(status)) => status,
        Err(Stop::Abort) => {
            run.remove_output_file();
            ERROR
        }
    };
    // `exit` flushes stdio, and reports nothing if it cannot.
    let _ = run.gz.io.flush_stdout();
    status
}
