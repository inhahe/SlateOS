//! `sysctl` -- read and write kernel parameters at run time: procps-ng
//! 4.0.4's, ported.
//!
//! ```text
//! sysctl [options] [variable[=value] ...]
//! ```
//!
//! A transcription of `src/sysctl.c` and of `local/procio.c`, the stdio
//! replacement it reads and writes `/proc/sys` through. It replaces
//! `userspace/sysctl`, which was written from the manual: it searched
//! `/sys/kernel` as well as `/proc/sys`, read `-q` as "print no names" and
//! `-w name value` as a write, and had a `--search` procps does not.
//!
//! # What upstream does and this keeps
//!
//! - A key's separators are swapped as `slashdot` swaps them: the first of
//!   `.` and `/` in it decides which way, so `net.ipv4.conf.eth0/100.rp_filter`
//!   names the directory `eth0.100`, and a doubled separator is warned of.
//! - A file is read whole and printed a line at a time, each line of a
//!   multi-line value with its own `name = `; an empty file prints nothing at
//!   all. A file whose mode lacks the owner's read bit is skipped silently,
//!   and one whose mode lacks the write bit is refused a write with `EPERM`
//!   -- by the bits, not by trying.
//! - A write is one `write(2)` of the value and a newline, and a value the
//!   kernel answers `EINVAL` for is written again in pieces at its commas, as
//!   `procio.c` does for a long `cpumask`.
//! - Settings loaded with `-p` (and `--system`) are expanded as `glob(3)`
//!   expands them -- `GLOB_BRACE` and `GLOB_TILDE` for the file names, and a
//!   key with a wildcard in it matched against `/proc/sys` -- and a `-key`
//!   line with no value excludes what its pattern names from a wildcard's
//!   expansion.
//! - The exit status sums some failures and ORs others, as `main` does, so a
//!   key that is not under `/proc/sys` (-1) exits 255.
//! - A bad option prints getopt's complaint and then the usage on **standard
//!   output**, and exits 0: `case '?': Usage(stdout)`.
//! - `--system` runs the moment it is parsed, so options after it are not
//!   read.
//!
//! # On SlateOS
//!
//! The kernel's `/proc/sys` is read-only: procfs gives every file there mode
//! 0444 and honours no write under it. So a write is refused by the mode
//! before anything is tried -- `setting key "kernel.hostname": Operation not
//! permitted`, as procps' own says -- and `--system` on a booted system
//! reports each setting its files name. Reading, listing and `-p`'s parsing
//! work as on Linux.
//!
//! # Deliberate differences
//!
//! - `--version` names SlateOS coreutils, as every program here does.
//! - A directory is listed by `read_dir`, which leaves out `.` and `..`,
//!   where upstream reads two entries and discards them unread. The two agree
//!   on every filesystem whose first two entries are `.` and `..`, which is
//!   every one `/proc/sys` has been.

use std::ffi::{CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes};
use coreutils::stdfd::{self, Stream};
use ere::{Regex, Syntax};

coreutils::guard_std_fds!();

const SYSCTL: Program = Program::new("sysctl", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "bneNwfp::qoxaAXr:Vdh";

/// Upstream's `longopts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("deprecated", Takes::Nothing),
    ("dry-run", Takes::Nothing),
    ("binary", Takes::Nothing),
    ("ignore", Takes::Nothing),
    ("names", Takes::Nothing),
    ("values", Takes::Nothing),
    ("load", Takes::Optional),
    ("quiet", Takes::Nothing),
    ("write", Takes::Nothing),
    ("system", Takes::Nothing),
    ("pattern", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Upstream's `Usage()`.
const HELP: &str = concat!(
    "\n",
    "Usage:\n",
    " sysctl [options] [variable[=value] ...]\n",
    "\n",
    "Options:\n",
    "  -a, --all            display all variables\n",
    "  -A                   alias of -a\n",
    "  -X                   alias of -a\n",
    "      --deprecated     include deprecated parameters to listing\n",
    "      --dry-run        Print the key and values but do not write\n",
    "  -b, --binary         print value without new line\n",
    "  -e, --ignore         ignore unknown variables errors\n",
    "  -N, --names          print variable names without values\n",
    "  -n, --values         print only values of the given variable(s)\n",
    "  -p, --load[=<file>]  read values from file\n",
    "  -f                   alias of -p\n",
    "      --system         read values from all system directories\n",
    "  -r, --pattern <expression>\n",
    "                       select setting that match expression\n",
    "  -q, --quiet          do not echo variable set\n",
    "  -w, --write          enable writing a value to variable\n",
    "  -o                   does nothing\n",
    "  -x                   does nothing\n",
    "  -d                   alias of -h\n",
    "\n",
    " -h, --help     display this help and exit\n",
    " -V, --version  output version information and exit\n",
    "\n",
    "For more details see sysctl(8).\n",
);

/// Upstream prints `PROCPS_NG_VERSION`, `sysctl from procps-ng 4.0.4`.
const VERSION: &str = "sysctl from SlateOS coreutils 0.1.0\n";

/// `PROC_PATH`.
const PROC_PATH: &[u8] = b"/proc/sys/";
/// `DEFAULT_PRELOAD`.
const DEFAULT_PRELOAD: &[u8] = b"/etc/sysctl.conf";
/// `DEPRECATED`: left out of a listing unless `--deprecated`.
const DEPRECATED: &[&[u8]] = &[b"base_reachable_time", b"retrans_time"];
/// `PreloadSystem`'s directories, in its order.
const SYSTEM_DIRS: &[&[u8]] = &[
    b"/etc/sysctl.d",
    b"/run/sysctl.d",
    b"/usr/local/lib/sysctl.d",
    b"/usr/lib/sysctl.d",
    b"/lib/sysctl.d",
];
/// `GLOB_CHARS`.
const GLOB_CHARS: &[u8] = b"*?[";

/// `EXIT_SUCCESS` and `EXIT_FAILURE`, as the `int`s upstream adds and ORs.
const OK: i32 = 0;
const FAIL: i32 = 1;

/// The `errno` values the program tells apart.
const EPERM: i32 = 1;
const ENOENT: i32 = 2;
const EIO: i32 = 5;
const ENOMEM: i32 = 12;
const EACCES: i32 = 13;
const EISDIR: i32 = 21;
const EINVAL: i32 = 22;
const EROFS: i32 = 30;

/// `LINELEN` in `procio.c`: the longest piece a refused write is cut to.
const LINELEN: usize = 4096;

/// `-r`'s pattern, as `regcomp` left it.
enum Pattern {
    /// Compiled, `REG_EXTENDED`.
    Re(Regex),
    /// `regcomp` refused it: `pattern_match` then never matches.
    Never,
}

/// `SysctlSetting`.
#[derive(Debug, Clone)]
struct Setting {
    path: Vec<u8>,
    value: Option<Vec<u8>>,
    ignore_failure: bool,
    glob_exclude: bool,
}

/// Upstream's globals, and the stream they print to.
struct Sysctl<'o> {
    ignore_deprecated: bool,
    name_only: bool,
    print_name: bool,
    print_newline: bool,
    ignore_error: bool,
    quiet: bool,
    dry_run: bool,
    pattern: Option<Pattern>,
    /// Standard input has been read to its end and `fclose`d by a `-p -`.
    stdin_closed: bool,
    out: &'o mut Stream,
}

/// Write `bytes` to standard output: a `Stream` records a failed write for
/// `close_stdout` rather than returning it, as stdio's error flag does.
fn emit(out: &mut Stream, bytes: &[u8]) {
    // Never an error; see above.
    let _ = out.write_all(bytes);
}

/// `xwarnx`: `sysctl: MESSAGE`.
fn warnx(message: &[u8]) {
    let mut line = b"sysctl: ".to_vec();
    line.extend_from_slice(message);
    line.push(b'\n');
    stdfd::diag_bytes(&line);
}

/// `xwarn`: `sysctl: MESSAGE: REASON`, or no reason when `errno` is 0.
fn warn(message: &[u8], errno: i32) {
    let mut line = message.to_vec();
    if errno != 0 {
        line.extend_from_slice(b": ");
        line.extend_from_slice(
            coreutils::errmsg::strerror(&io::Error::from_raw_os_error(errno)).as_bytes(),
        );
    }
    warnx(&line);
}

/// The `errno` behind an I/O failure; `EIO` for one that has none.
fn errno_of(e: &io::Error) -> i32 {
    e.raw_os_error().unwrap_or(EIO)
}

/// `[a, b, ...]` joined.
fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// A path as the system takes it.
fn path(bytes: &[u8]) -> OsString {
    os_from_bytes(bytes)
}

/// The text a C string function sees: everything before the first NUL.
fn c_str(s: &[u8]) -> &[u8] {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    s.get(..end).unwrap_or_default()
}

/// `isspace` in the C locale.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | 0x09..=0x0d)
}

/// The position of the first byte of `set` in `s` at or after `from` --
/// `strpbrk`.
fn strpbrk(s: &[u8], from: usize, set: &[u8]) -> Option<usize> {
    s.get(from..)?
        .iter()
        .position(|b| set.contains(b))
        .and_then(|at| from.checked_add(at))
}

/// `slashdot (p, old, new)`: from the first `.` or `/` on, every `old` made
/// `new` and every `new` made `old` -- unless that first separator is `new`
/// already, which leaves the whole thing alone. A separator followed by
/// another is warned of once, quoting what is left from it.
fn slashdot(p: &mut [u8], old: u8, new: u8) {
    let mut warned = true;
    let Some(mut at) = strpbrk(p, 0, b"/.") else {
        return;
    };
    if p.get(at) == Some(&new) {
        return;
    }
    loop {
        let c = p.get(at).copied().unwrap_or(0);
        let next = p.get(at.saturating_add(1)).copied();
        if matches!(next, Some(b'/' | b'.')) && warned {
            let mut message = b"separators should not be repeated: ".to_vec();
            message.extend_from_slice(c_str(p.get(at..).unwrap_or_default()));
            warnx(&message);
            warned = false;
        }
        if let Some(slot) = p.get_mut(at) {
            if c == old {
                *slot = new;
            }
            if c == new {
                *slot = old;
            }
        }
        match strpbrk(p, at.saturating_add(1), b"/.") {
            Some(n) => at = n,
            None => break,
        }
    }
}

/// `getline`'s lines of `text`: each with its newline, the last without one
/// if the text does not end in a newline.
fn lines(text: &[u8]) -> Vec<&[u8]> {
    text.split_inclusive(|&c| c == b'\n').collect()
}

/// `lstrip`: from the first byte that is not a space.
fn lstrip(line: &[u8]) -> &[u8] {
    let n = line.iter().take_while(|&&b| is_space(b)).count();
    line.get(n..).unwrap_or_default()
}

/// `rstrip`: trailing spaces cut, though never the first byte.
fn rstrip(line: &[u8]) -> &[u8] {
    let line = c_str(line);
    let mut end = line.len();
    while end > 1
        && line
            .get(end.saturating_sub(1))
            .copied()
            .is_some_and(is_space)
    {
        end = end.saturating_sub(1);
    }
    line.get(..end).unwrap_or_default()
}

/// The mode bits of a file.
#[cfg(unix)]
fn mode_of(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode()
}

/// The mode bits of a file, as near as a host without them comes.
#[cfg(not(unix))]
fn mode_of(meta: &fs::Metadata) -> u32 {
    let base = if meta.is_dir() { 0o040_000 } else { 0o100_000 };
    base | if meta.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

/// `S_IRUSR` and `S_IWUSR`.
const S_IRUSR: u32 = 0o400;
const S_IWUSR: u32 = 0o200;

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut out = Stream::stdout();
    let rc = run(&argv, &mut out);
    // `return rc` from `main`: the status is its low eight bits.
    let status = ExitCode::from(rc.to_le_bytes().first().copied().unwrap_or(0));
    stdfd::close_stdout("sysctl", out, status)
}

/// `main`, to the value it returns.
fn run(argv: &[OsString], out: &mut Stream) -> i32 {
    let mut s = Sysctl {
        ignore_deprecated: true,
        name_only: false,
        print_name: true,
        print_newline: true,
        ignore_error: false,
        quiet: false,
        dry_run: false,
        pattern: None,
        stdin_closed: false,
        out,
    };
    // `if (argc < 2) Usage (stderr)`.
    if argv.is_empty() {
        stdfd::diag_bytes(HELP.as_bytes());
        return FAIL;
    }
    let mut write_mode = false;
    let mut display_all = false;
    let mut preload_opt = false;
    let mut preload_file: Option<Vec<u8>> = None;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in SYSCTL.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let opt = match item {
            Ok(opt) => opt,
            // getopt has said what was wrong; `case '?': Usage (stdout)`.
            Err(e) => {
                coreutils::diag!("sysctl: {}", e.sentence);
                emit(s.out, HELP.as_bytes());
                return OK;
            }
        };
        match opt {
            Opt::Short(b'b', _) | Opt::Long("binary", _) => {
                // "This is "binary" format": no newline, and so no name.
                s.print_newline = false;
                s.print_name = false;
            }
            Opt::Short(b'n', _) | Opt::Long("values", _) => s.print_name = false,
            Opt::Short(b'e', _) | Opt::Long("ignore", _) => s.ignore_error = true,
            Opt::Short(b'N', _) | Opt::Long("names", _) => s.name_only = true,
            Opt::Short(b'w', _) | Opt::Long("write", _) => write_mode = true,
            Opt::Short(b'f' | b'p', value) | Opt::Long("load", value) => {
                preload_opt = true;
                if let Some(v) = value {
                    preload_file = Some(os_bytes(&v).into_owned());
                }
            }
            Opt::Short(b'q', _) | Opt::Long("quiet", _) => s.quiet = true,
            // BSD's hexadecimal dumps: "does nothing".
            Opt::Short(b'o' | b'x', _) => {}
            Opt::Short(b'a' | b'A' | b'X', _) | Opt::Long("all", _) => display_all = true,
            Opt::Long("deprecated", _) => s.ignore_deprecated = false,
            Opt::Long("system", _) => {
                s.ignore_error = true;
                let mut list = Vec::new();
                let mut rc = s.preload_system(&mut list);
                rc |= s.write_setting_list(&list);
                return rc;
            }
            Opt::Long("dry-run", _) => s.dry_run = true,
            Opt::Short(b'r', value) | Opt::Long("pattern", value) => {
                let text = os_bytes(&value.unwrap_or_default()).into_owned();
                s.pattern = Some(compile(&text));
            }
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                emit(s.out, VERSION.as_bytes());
                return OK;
            }
            Opt::Short(b'd' | b'h', _) | Opt::Long("help", _) => {
                emit(s.out, HELP.as_bytes());
                return OK;
            }
            Opt::Operand(value) => operands.push(os_bytes(value).into_owned()),
            // Unreachable: every letter and every long option is above.
            Opt::Short(..) | Opt::Long(..) => {
                stdfd::diag_bytes(HELP.as_bytes());
                return FAIL;
            }
        }
    }

    if display_all {
        return s.display_all(PROC_PATH);
    }

    if preload_opt {
        let mut list = Vec::new();
        let mut ret = OK;
        match &preload_file {
            None => {
                if operands.is_empty() {
                    ret |= s.preload(&mut list, DEFAULT_PRELOAD);
                }
            }
            // "This happens when -pfile option is used without space."
            Some(file) => ret |= s.preload(&mut list, file),
        }
        for operand in &operands {
            ret |= s.preload(&mut list, operand);
        }
        ret |= s.write_setting_list(&list);
        return ret;
    }

    if operands.is_empty() {
        warnx(b"no variables specified\nTry `sysctl --help' for more information.");
        return FAIL;
    }
    if s.name_only && s.quiet {
        warnx(b"options -N and -q cannot coexist\nTry `sysctl --help' for more information.");
        return FAIL;
    }

    let mut return_code = OK;
    for operand in &operands {
        if write_mode || operand.contains(&b'=') {
            match s.parse_setting_line(b"command line", 0, operand) {
                Some(setting) => {
                    return_code |= s.write_setting(
                        &setting.path,
                        setting.value.as_deref(),
                        setting.ignore_failure,
                    );
                }
                None => return_code |= FAIL,
            }
        } else {
            return_code = return_code.wrapping_add(s.read_setting(operand));
        }
    }
    return_code
}

/// `regcomp (&re, pat, REG_EXTENDED | REG_NOSUB)`, under the locale.
fn compile(text: &[u8]) -> Pattern {
    let text = c_str(text);
    let compiled = if coreutils::locale::ctype_is_utf8() {
        Regex::new_syntax(text, false, Syntax::POSIX_EXTENDED)
    } else {
        Regex::new_syntax_bytes(text, false, Syntax::POSIX_EXTENDED)
    };
    compiled.map_or(Pattern::Never, Pattern::Re)
}

impl Sysctl<'_> {
    /// `pattern_match`: the string, as far as a NUL, against `-r`'s pattern.
    /// A search that ran out of budget is `regexec`'s failure: no match.
    fn pattern_match(&self, string: &[u8]) -> bool {
        match &self.pattern {
            None => true,
            Some(Pattern::Never) => false,
            Some(Pattern::Re(re)) => re.is_match(c_str(string)).unwrap_or(false),
        }
    }

    /// `is_proc_path`: whether `path`, resolved, is under `/proc/sys/`. One
    /// that does not resolve is not, silently; one that resolves elsewhere is
    /// not, and is warned of.
    fn is_proc_path(path_bytes: &[u8]) -> bool {
        let Ok(resolved) = fs::canonicalize(path(path_bytes)) else {
            return false;
        };
        if os_bytes(resolved.as_os_str()).starts_with(PROC_PATH) {
            return true;
        }
        warnx(&cat(&[
            b"Path is not under ",
            PROC_PATH,
            b": ",
            c_str(path_bytes),
        ]));
        false
    }

    /// `ReadSetting`.
    fn read_setting(&mut self, name: &[u8]) -> i32 {
        let name = c_str(name);
        if name.is_empty() {
            warnx(&cat(&[b"\"", name, b"\" is an unknown key"]));
            return -1;
        }
        let mut tmpname = cat(&[PROC_PATH, name]);
        if let Some(tail) = tmpname.get_mut(PROC_PATH.len()..) {
            slashdot(tail, b'.', b'/');
        }
        let mut outname = name.to_vec();
        slashdot(&mut outname, b'/', b'.');

        let meta = match fs::metadata(path(&tmpname)) {
            Ok(m) => m,
            Err(e) => {
                if self.ignore_error {
                    return OK;
                }
                warn(&cat(&[b"cannot stat ", &tmpname]), errno_of(&e));
                return FAIL;
            }
        };
        if mode_of(&meta) & S_IRUSR == 0 {
            return OK;
        }
        if !Self::is_proc_path(&tmpname) {
            return -1;
        }
        if meta.is_dir() {
            tmpname.push(b'/');
            return self.display_all(&tmpname);
        }
        if self.pattern.is_some() && !self.pattern_match(&outname) {
            return OK;
        }
        if self.name_only {
            emit(self.out, &cat(&[&outname, b"\n"]));
            return OK;
        }

        let mut file = match File::open(path(&tmpname)) {
            Ok(f) => f,
            Err(e) => {
                return match errno_of(&e) {
                    ENOENT => {
                        if self.ignore_error {
                            OK
                        } else {
                            warnx(&cat(&[b"\"", &outname, b"\" is an unknown key"]));
                            FAIL
                        }
                    }
                    EACCES => {
                        warnx(&cat(&[b"permission denied on key '", &outname, b"'"]));
                        FAIL
                    }
                    // "Ignore stable_secret below /proc/sys/net/ipv6/conf".
                    EIO => FAIL,
                    errno => {
                        warn(&cat(&[b"reading key \"", &outname, b"\""]), errno);
                        FAIL
                    }
                };
            }
        };
        // `fprocopen`'s read: the whole file, read again into a larger buffer
        // until one read leaves room.
        let mut text = Vec::new();
        if let Err(e) = file.read_to_end(&mut text) {
            return match errno_of(&e) {
                EACCES => {
                    warnx(&cat(&[b"permission denied on key '", &outname, b"'"]));
                    FAIL
                }
                EISDIR => {
                    tmpname.push(b'/');
                    self.display_all(&tmpname)
                }
                EIO => FAIL,
                _ => {
                    warnx(&cat(&[b"reading key \"", &outname, b"\""]));
                    FAIL
                }
            };
        }
        self.print_value(&outname, &text);
        OK
    }

    /// The `getline` loop of `ReadSetting`: every line of the value, each with
    /// its own `name = ` and the line's newline supplied if it had none, or
    /// the bare values -- with `-b`, each line's first newline taken away. An
    /// empty file prints nothing.
    fn print_value(&mut self, outname: &[u8], text: &[u8]) {
        let mut it = lines(text).into_iter();
        let Some(mut line) = it.next() else {
            return;
        };
        loop {
            if self.print_name {
                emit(self.out, &cat(&[outname, b" = "]));
                // `nlptr = &iobuf[strlen (iobuf) - 1]`, of the last line
                // printed: whether it ended in a newline.
                let ended = loop {
                    let shown = c_str(line);
                    emit(self.out, shown);
                    if shown.last() == Some(&b'\n') {
                        break true;
                    }
                    match it.next() {
                        Some(next) => line = next,
                        None => break false,
                    }
                };
                if !ended {
                    emit(self.out, b"\n");
                }
            } else {
                let mut shown = c_str(line);
                if !self.print_newline {
                    if let Some(nl) = shown.iter().position(|&c| c == b'\n') {
                        shown = shown.get(..nl).unwrap_or_default();
                    }
                }
                emit(self.out, shown);
            }
            match it.next() {
                Some(next) => line = next,
                None => break,
            }
        }
    }

    /// `DisplayAll`: every setting under `dir` (which ends in `/`), its
    /// directories walked in turn.
    fn display_all(&mut self, dir: &[u8]) -> i32 {
        let Ok(entries) = fs::read_dir(path(dir)) else {
            warnx(&cat(&[b"unable to open directory \"", c_str(dir), b"\""]));
            return FAIL;
        };
        let mut rc = OK;
        // A failed `readdir` ends the walk, as its `NULL` does.
        for entry in entries.map_while(Result::ok) {
            let name = os_bytes(&entry.file_name()).into_owned();
            if self.ignore_deprecated && DEPRECATED.contains(&name.as_slice()) {
                continue;
            }
            let mut tmpdir = cat(&[dir, &name]);
            match fs::metadata(path(&tmpdir)) {
                Err(e) => warn(&cat(&[b"cannot stat ", &tmpdir]), errno_of(&e)),
                Ok(meta) if meta.is_dir() => {
                    tmpdir.push(b'/');
                    // Its status is not upstream's either.
                    let _ = self.display_all(&tmpdir);
                }
                Ok(_) => {
                    let name = tmpdir.get(PROC_PATH.len()..).unwrap_or_default();
                    rc |= self.read_setting(name);
                }
            }
        }
        rc
    }

    /// `WriteSetting`.
    fn write_setting(
        &mut self,
        path_bytes: &[u8],
        value: Option<&[u8]>,
        ignore_failure: bool,
    ) -> i32 {
        let mut rc = OK;
        let meta = match fs::metadata(path(path_bytes)) {
            Ok(m) => m,
            Err(e) => {
                if !self.ignore_error {
                    warn(&cat(&[b"cannot stat ", path_bytes]), errno_of(&e));
                    rc = FAIL;
                }
                return rc;
            }
        };
        if !Self::is_proc_path(path_bytes) {
            return FAIL;
        }
        // "Convert the globbed path into a dotted key".
        let mut dotted = path_bytes
            .get(PROC_PATH.len()..)
            .unwrap_or_default()
            .to_vec();
        slashdot(&mut dotted, b'/', b'.');
        let quoted = cat(&[b"setting key \"", &dotted, b"\""]);

        if mode_of(&meta) & S_IWUSR == 0 {
            warn(&quoted, EPERM);
            return rc;
        }
        if meta.is_dir() {
            warn(&quoted, EISDIR);
            return rc;
        }
        let value_bytes = value.unwrap_or_default();
        if !self.dry_run {
            let ignoring: &[u8] = if ignore_failure { b", ignoring" } else { b"" };
            match OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(path(path_bytes))
            {
                Err(e) => {
                    let errno = errno_of(&e);
                    match errno {
                        ENOENT => {
                            if !self.ignore_error {
                                warnx(&cat(&[b"\"", &dotted, b"\" is an unknown key", ignoring]));
                                if !ignore_failure {
                                    rc = FAIL;
                                }
                            }
                        }
                        EPERM | EROFS | EACCES => warnx(&cat(&[
                            b"permission denied on key \"",
                            &dotted,
                            b"\"",
                            ignoring,
                        ])),
                        _ => warn(&cat(&[&quoted, ignoring]), errno),
                    }
                    if !ignore_failure && errno != ENOENT {
                        rc = FAIL;
                    }
                }
                Ok(mut file) => {
                    let mut text = c_str(value_bytes).to_vec();
                    text.push(b'\n');
                    if let Err(errno) = proc_write(&mut file, &mut text) {
                        warn(&quoted, errno);
                        return FAIL;
                    }
                }
            }
        }
        if (rc == OK && !self.quiet) || self.dry_run {
            let shown = c_str(value_bytes);
            let line = if self.name_only {
                cat(&[&dotted, b"\n"])
            } else if self.print_name {
                cat(&[&dotted, b" = ", shown, b"\n"])
            } else if self.print_newline {
                cat(&[shown, b"\n"])
            } else {
                shown.to_vec()
            };
            emit(self.out, &line);
        }
        rc
    }

    /// `parse_setting_line`: one line of a settings file, or a command-line
    /// `key=value`.
    fn parse_setting_line(&self, file: &[u8], linenum: i32, line: &[u8]) -> Option<Setting> {
        let line = c_str(line);
        let mut key = lstrip(line);
        if key.len() < 2 {
            return None;
        }
        // "skip over comments"
        if matches!(key.first(), Some(b'#' | b';')) {
            return None;
        }
        if self.pattern.is_some() && !self.pattern_match(key) {
            return None;
        }
        let (key, value, ignore_failure, glob_exclude) = match key.iter().position(|&c| c == b'=') {
            None => {
                if key.first() == Some(&b'-') {
                    key = key.get(1..).unwrap_or_default();
                    (rstrip(key), None, false, true)
                } else {
                    warnx(&cat(&[
                        c_str(file),
                        format!("({linenum}): invalid syntax, continuing...").as_bytes(),
                    ]));
                    return None;
                }
            }
            Some(eq) => {
                let mut k = key.get(..eq).unwrap_or_default();
                let mut ignore = false;
                if k.first() == Some(&b'-') {
                    ignore = true;
                    k = k.get(1..).unwrap_or_default();
                }
                let v = rstrip(lstrip(key.get(eq.saturating_add(1)..).unwrap_or_default()));
                (rstrip(k), Some(v.to_vec()), ignore, false)
            }
        };
        Some(setting_new(key, value, ignore_failure, glob_exclude))
    }

    /// `Preload`: the settings in each file `filename` expands to.
    fn preload(&mut self, list: &mut Vec<Setting>, filename: &[u8]) -> i32 {
        let pattern = CString::new(c_str(filename).to_vec()).unwrap_or_default();
        let mut files: Vec<Vec<u8>> = Vec::new();
        let flags =
            libcall::glob::GLOB_NOCHECK | libcall::glob::GLOB_BRACE | libcall::glob::GLOB_TILDE;
        match libcall::glob::glob(&pattern, flags, &mut |p| files.push(p.to_vec())) {
            Ok(()) | Err(libcall::glob::GLOB_NOMATCH) => {}
            Err(code) => {
                // `xerr (EXIT_FAILURE, _("glob failed"))`: out of memory is
                // the one failure left without an error function.
                let errno = if code == libcall::glob::GLOB_NOSPACE {
                    ENOMEM
                } else {
                    0
                };
                warn(b"glob failed", errno);
                // `exit` runs `close_stdout`, which delivers what is held.
                let _ = self.out.flush();
                std::process::exit(1);
            }
        }
        let mut n: i32 = 0;
        for name in &files {
            let mut text = Vec::new();
            if name.as_slice() == b"-" {
                // `fclose (stdin)` after the first `-`: a second one reads a
                // closed stream, which yields nothing.
                if !self.stdin_closed {
                    // A read error ends the input, as `getline`'s -1 does,
                    // keeping what came before it.
                    let _ = io::stdin().lock().read_to_end(&mut text);
                    self.stdin_closed = true;
                }
            } else {
                // `fopen` opens a directory too; reading one then fails, and
                // `getline`'s -1 ends the file's lines without a word.
                let mut file = match File::open(path(name)) {
                    Ok(f) => f,
                    Err(e) => {
                        warn(&cat(&[b"cannot open \"", name, b"\""]), errno_of(&e));
                        return FAIL;
                    }
                };
                // As above: what was read before a failure is kept.
                let _ = file.read_to_end(&mut text);
            }
            for line in lines(&text) {
                n = n.wrapping_add(1);
                if line.len() < 2 {
                    continue;
                }
                if let Some(setting) = self.parse_setting_line(name, n, line) {
                    list.push(setting);
                }
            }
        }
        OK
    }

    /// `PreloadSystem`: every `*.conf` in the system's directories, by name,
    /// the first directory to have a name keeping it -- then
    /// `/etc/sysctl.conf`.
    fn preload_system(&mut self, list: &mut Vec<Setting>) -> i32 {
        let mut cfgs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for dir in SYSTEM_DIRS {
            let Ok(entries) = fs::read_dir(path(dir)) else {
                continue;
            };
            for entry in entries.map_while(Result::ok) {
                let name = os_bytes(&entry.file_name()).into_owned();
                if name.len() < 5 || !name.ends_with(b".conf") {
                    continue;
                }
                if cfgs.iter().any(|(known, _)| *known == name) {
                    continue;
                }
                let full = cat(&[dir, b"/", &name]);
                cfgs.push((name, full));
            }
        }
        cfgs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut rc = OK;
        for (_, full) in &cfgs {
            if !self.quiet {
                emit(self.out, &cat(&[b"* Applying ", full, b" ...\n"]));
            }
            rc |= self.preload(list, full);
        }
        if fs::metadata(path(DEFAULT_PRELOAD)).is_ok_and(|m| m.is_file()) {
            if !self.quiet {
                emit(
                    self.out,
                    &cat(&[b"* Applying ", DEFAULT_PRELOAD, b" ...\n"]),
                );
            }
            rc |= self.preload(list, DEFAULT_PRELOAD);
        }
        rc
    }

    /// `write_setting_list`: each setting written, a key with a wildcard in it
    /// for every path it matches -- unless another setting names that path
    /// exactly, which either overrides it or, as `-key`, excludes it.
    fn write_setting_list(&mut self, list: &[Setting]) -> i32 {
        let mut rc = OK;
        for node in list {
            if node.glob_exclude {
                continue;
            }
            if node.path.iter().any(|b| GLOB_CHARS.contains(b)) {
                let pattern = CString::new(c_str(&node.path).to_vec()).unwrap_or_default();
                let mut paths: Vec<Vec<u8>> = Vec::new();
                if libcall::glob::glob(&pattern, 0, &mut |p| paths.push(p.to_vec())).is_err() {
                    continue;
                }
                for p in &paths {
                    if list.iter().any(|other| other.path == *p) {
                        continue;
                    }
                    rc |= self.write_setting(p, node.value.as_deref(), node.ignore_failure);
                }
            } else {
                rc |= self.write_setting(&node.path, node.value.as_deref(), node.ignore_failure);
            }
        }
        rc
    }
}

/// `setting_new`: the setting, its path `/proc/sys/` and the key -- less one
/// leading `-` -- with the key's separators made slashes.
fn setting_new(
    key: &[u8],
    value: Option<Vec<u8>>,
    ignore_failure: bool,
    glob_exclude: bool,
) -> Setting {
    let bare = key.strip_prefix(b"-").unwrap_or(key);
    let mut full = cat(&[PROC_PATH, bare]);
    if let Some(tail) = full.get_mut(PROC_PATH.len()..) {
        slashdot(tail, b'.', b'/');
    }
    Setting {
        path: full,
        value,
        ignore_failure,
        glob_exclude,
    }
}

/// `proc_write` at the close of a `fprocopen (path, "w")`: `text`, which
/// ends in its newline, in one `write`. A kernel that answers `EINVAL` -- a
/// value too long for one write -- is given it again in pieces, each ending
/// at the last comma in its first `LINELEN` bytes (made a newline), the file
/// position moved on by one between pieces. A write that took anything at
/// all counts as having taken everything.
fn proc_write(file: &mut File, text: &mut [u8]) -> Result<(), i32> {
    match file.write(text) {
        Ok(_) => Ok(()),
        Err(e) if errno_of(&e) == EINVAL => split_write(file, text),
        Err(e) => Err(errno_of(&e)),
    }
}

/// The piecewise retry of [`proc_write`].
fn split_write(file: &mut File, text: &mut [u8]) -> Result<(), i32> {
    let mut offset = 0usize;
    let mut remaining = text.len();
    loop {
        let window = if remaining > LINELEN {
            text.get(offset..offset.saturating_add(LINELEN))
                .and_then(|w| w.iter().rposition(|&c| c == b','))
        } else {
            text.get(offset..offset.saturating_add(remaining))
                .and_then(|w| w.iter().rposition(|&c| c == b'\n'))
        };
        let Some(at) = window else {
            return Err(EINVAL);
        };
        let token = offset.saturating_add(at);
        if let Some(slot) = text.get_mut(token) {
            *slot = b'\n';
        }
        if offset > 0 {
            // `lseek (cookie->fd, 1, SEEK_CUR)`, whose result upstream does
            // not look at: a file that cannot seek still gets the piece.
            let _ = file.seek(SeekFrom::Current(1));
        }
        let piece = text.get(offset..=token).unwrap_or_default();
        let len = match file.write(piece) {
            Ok(n) => n,
            Err(e) => return Err(errno_of(&e)),
        };
        if len < 1 || len >= remaining {
            return Ok(());
        }
        offset = offset.saturating_add(len);
        remaining = remaining.saturating_sub(len);
        if remaining == 0 {
            return Ok(());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn dots(s: &str, old: u8, new: u8) -> String {
        let mut b = s.as_bytes().to_vec();
        slashdot(&mut b, old, new);
        String::from_utf8(b).unwrap_or_default()
    }

    #[test]
    fn slashdot_swaps_from_the_first_separator() {
        assert_eq!(dots("kernel.hostname", b'.', b'/'), "kernel/hostname");
        assert_eq!(
            dots("net/ipv4/conf/eth0.100/rp_filter", b'/', b'.'),
            "net.ipv4.conf.eth0/100.rp_filter"
        );
        assert_eq!(
            dots("net.ipv4.conf.eth0/100.rp_filter", b'.', b'/'),
            "net/ipv4/conf/eth0.100/rp_filter"
        );
        // Already in the wanted form from the first separator: left alone.
        assert_eq!(dots("kernel/hostname.x", b'.', b'/'), "kernel/hostname.x");
        assert_eq!(dots("nosep", b'.', b'/'), "nosep");
    }

    #[test]
    fn a_setting_line_as_upstream_takes_it_apart() {
        let mut out = Stream::on(-1);
        let s = Sysctl {
            ignore_deprecated: true,
            name_only: false,
            print_name: true,
            print_newline: true,
            ignore_error: false,
            quiet: false,
            dry_run: false,
            pattern: None,
            stdin_closed: false,
            out: &mut out,
        };
        let set = s
            .parse_setting_line(b"f", 1, b"  kernel.x = 5  \n")
            .unwrap();
        assert_eq!(set.value.as_deref(), Some(&b"5"[..]));
        assert_eq!(set.path, b"/proc/sys/kernel/x");
        let ex = s.parse_setting_line(b"f", 2, b"-net.*.foo\n").unwrap();
        assert!(ex.glob_exclude);
        assert_eq!(ex.path, b"/proc/sys/net/*/foo");
        let ign = s.parse_setting_line(b"f", 3, b"-vm.a=1\n").unwrap();
        assert!(ign.ignore_failure);
        assert_eq!(ign.path, b"/proc/sys/vm/a");
        assert!(s.parse_setting_line(b"f", 4, b"# comment\n").is_none());
        assert!(
            s.parse_setting_line(b"f", 5, b"x\n").is_none(),
            "no '=': invalid syntax"
        );
        assert!(s.parse_setting_line(b"f", 6, b"\n").is_none());
    }

    #[test]
    fn strip_as_upstream_strips() {
        assert_eq!(lstrip(b"  a b "), b"a b ");
        assert_eq!(rstrip(b"a b \n"), b"a b");
        assert_eq!(rstrip(b" "), b" ", "never the first byte");
        assert_eq!(rstrip(b""), b"");
    }
}
