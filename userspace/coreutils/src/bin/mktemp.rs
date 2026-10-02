//! `mktemp` — create a temporary file or directory, safely, and print its name.
//!
//! A port of GNU coreutils 9.4's `src/mktemp.c`, with gnulib's
//! `gen_tempname_len` and `file_name_concat` beside it: the same options, the
//! same order of checks and the same messages, and the same rules for where
//! the name goes -- `-p`, `--tmpdir`, the deprecated `-t` and `$TMPDIR` each
//! decide it differently.
//!
//! # What this replaced
//!
//! `userspace/mktemp`, a hand-written "multi-personality utility: mktemp / id /
//! groups / whoami". Its three other personalities duplicated coreutils' own
//! `id`, `groups` and `whoami`, which nothing linked to it; its `mktemp` read
//! argv as `String` (a template that was not UTF-8 killed it before its first
//! statement) and was its own reading of the options.
//!
//! # What no reading of `--help` suggests
//!
//! **The X's are the last run of X's before the suffix,** and the suffix is
//! everything after the last `X` when `--suffix` is not given -- so
//! `mktemp fooXXXbar` puts `bar` after three random characters, and
//! `mktemp fooXXbarX` is "too few X's". **`-t` and `-p` disagree about
//! `$TMPDIR`:** with `-t` it outranks `-p`'s directory; with `-p` it does not.
//! **`-u` creates nothing** but still checks that the name is free, and a
//! directory it cannot look in is an error. **A name that cannot be printed
//! is removed:** if writing it fails, the file just made is deleted, so a
//! failed `mktemp` leaves nothing behind.
//!
//! # Reference
//!
//! Measured against GNU `mktemp` (coreutils 9.4) through WSL; where a rule could
//! not be settled by measurement, `coreutils-9.4/src/mktemp.c` and gnulib's
//! `tempname.c` and `filenamecat-lgpl.c` settled it. `scripts/mktemp-diff.sh`
//! is the executable form of every claim here.

use coreutils::diag;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write as _;
use std::process::ExitCode;

/// `mktemp -Z; echo $?` is 1.
const MKTEMP: Program = Program::new("mktemp", 1);

/// Upstream's `getopt_long` string, verbatim: `-V` is an undocumented alias
/// for `--version`, kept for the original `mktemp`.
const SHORT_OPTIONS: &str = "dp:qtuV";

/// Upstream's `longopts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("directory", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("dry-run", Takes::Nothing),
    ("suffix", Takes::Required),
    ("tmpdir", Takes::Optional),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `default_template`.
const DEFAULT_TEMPLATE: &[u8] = b"tmp.XXXXXXXXXX";

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Settings {
    /// `-d`.
    directory: bool,
    /// `-p DIR` or `--tmpdir[=DIR]`: whether given, and its directory.
    dest_dir_arg: Option<Vec<u8>>,
    use_dest_dir: bool,
    /// `-q`.
    quiet: bool,
    /// `-t`.
    deprecated_t: bool,
    /// `-u`.
    dry_run: bool,
    /// `--suffix`.
    suffix: Option<Vec<u8>>,
    templates: Vec<OsString>,
}

/// What to make, and where: the name with its X's, and how many there are and
/// how much follows them.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    /// The full name, X's and all: what diagnostics quote.
    template: Vec<u8>,
    /// How many of the X's (the last run before the suffix) are replaced.
    x_count: usize,
    /// How many bytes follow them.
    suffix_len: usize,
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
            diag!("mktemp: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    let mut out = Stream::stdout();
    match request {
        Request::Help => {
            // A failed write is `close_stdout`'s to report, below.
            let _ = out.write_all(HELP.as_bytes());
            stdfd::close_stdout("mktemp", out, ExitCode::SUCCESS)
        }
        Request::Version => {
            let _ = out.write_all(b"mktemp (SlateOS coreutils) 0.1.0\n");
            stdfd::close_stdout("mktemp", out, ExitCode::SUCCESS)
        }
        Request::Run(settings) => {
            let env_tmpdir = std::env::var_os("TMPDIR").map(|v| os_bytes(&v).into_owned());
            match plan(&settings, env_tmpdir.as_deref()) {
                Ok(plan) => imp::make(&settings, &plan, out),
                Err(message) => {
                    diag!("mktemp: {message}");
                    stdfd::close_stdout("mktemp", out, ExitCode::FAILURE)
                }
            }
        }
    }
}

/// Read the command line: upstream's option loop and its one check of the
/// operands.
///
/// # Errors
/// Any getopt diagnostic, and "too many templates".
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut set = Settings::default();
    for item in MKTEMP.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'd', _) | Opt::Long("directory", _) => set.directory = true,
            Opt::Short(b'p', v) | Opt::Long("tmpdir", v) => {
                set.dest_dir_arg = v.map(|d| os_bytes(&d).into_owned());
                set.use_dest_dir = true;
            }
            Opt::Short(b'q', _) | Opt::Long("quiet", _) => set.quiet = true,
            Opt::Short(b't', _) => {
                set.use_dest_dir = true;
                set.deprecated_t = true;
            }
            Opt::Short(b'u', _) | Opt::Long("dry-run", _) => set.dry_run = true,
            Opt::Long("suffix", Some(v)) => set.suffix = Some(os_bytes(&v).into_owned()),
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Short(b'V', _) | Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => set.templates.push(word.clone()),
            // Every option in the tables is handled above; an unknown one
            // arrives as an `Err`, and a missing value cannot parse.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    if set.templates.len() >= 2 {
        return Err(MKTEMP.usage_referring("too many templates".to_string()));
    }
    Ok(Request::Run(set))
}

/// `main` from the operands to the name: the suffix, the X's, and the
/// directory, each checked in upstream's order. `env_tmpdir` is `$TMPDIR`.
///
/// # Errors
/// The message upstream exits with.
fn plan(set: &Settings, env_tmpdir: Option<&[u8]>) -> Result<Plan, String> {
    let (given, use_dest_dir) = match set.templates.first() {
        Some(t) => (os_bytes(t).into_owned(), set.use_dest_dir),
        None => (DEFAULT_TEMPLATE.to_vec(), true),
    };

    // The template and where its suffix begins.
    let (mut template, suffix_at) = match &set.suffix {
        Some(suffix) => {
            if given.last() != Some(&b'X') {
                return Err(format!(
                    "with --suffix, template {} must end in X",
                    quote(&given)
                ));
            }
            let at = given.len();
            let mut t = given;
            t.extend_from_slice(suffix);
            (t, at)
        }
        None => {
            let at = given
                .iter()
                .rposition(|&b| b == b'X')
                .map_or(given.len(), |i| i.saturating_add(1));
            (given, at)
        }
    };
    let suffix = template.get(suffix_at..).unwrap_or_default();
    let suffix_len = suffix.len();
    if suffix_len > 0 && last_component(suffix) != suffix {
        return Err(format!(
            "invalid suffix {}, contains directory separator",
            quote(suffix)
        ));
    }
    let x_count = template
        .get(..suffix_at)
        .unwrap_or_default()
        .iter()
        .rev()
        .take_while(|&&b| b == b'X')
        .count();
    if x_count < 3 {
        return Err(format!("too few X's in template {}", quote(&template)));
    }

    if use_dest_dir {
        let given_dir = set.dest_dir_arg.as_deref().filter(|d| !d.is_empty());
        let env = env_tmpdir.filter(|d| !d.is_empty());
        let dest_dir: &[u8] = if set.deprecated_t {
            let dir = env.or(given_dir).unwrap_or(b"/tmp");
            if last_component(&template) != template.as_slice() {
                return Err(format!(
                    "invalid template, {}, contains directory separator",
                    quote(&template)
                ));
            }
            dir
        } else {
            let dir = given_dir.or(env).unwrap_or(b"/tmp");
            if template.first() == Some(&b'/') {
                return Err(format!(
                    "invalid template, {}; with --tmpdir, it may not be absolute",
                    quote(&template)
                ));
            }
            dir
        };
        template = file_name_concat(dest_dir, &template);
    }
    Ok(Plan {
        template,
        x_count,
        suffix_len,
    })
}

/// gnulib's `last_component`: what follows the last slash that is followed
/// by something, leading slashes skipped -- the name's own last component,
/// trailing slashes included.
fn last_component(name: &[u8]) -> &[u8] {
    let mut base = name.iter().position(|&b| b != b'/').unwrap_or(name.len());
    let mut saw_slash = false;
    let mut i = base;
    while let Some(&b) = name.get(i) {
        if b == b'/' {
            saw_slash = true;
        } else if saw_slash {
            base = i;
            saw_slash = false;
        }
        i = i.saturating_add(1);
    }
    name.get(base..).unwrap_or_default()
}

/// gnulib's `base_len`: the length of a last component without its trailing
/// slashes (but a name of slashes only keeps one).
fn base_len(name: &[u8]) -> usize {
    let mut len = name.len();
    while len > 1 && name.get(len.saturating_sub(1)) == Some(&b'/') {
        len = len.saturating_sub(1);
    }
    len
}

/// gnulib's `file_name_concat (dir, base, NULL)`: `dir` without its trailing
/// slashes, a slash, and `base` -- no slash added when either side already
/// has one, and none taken from a root `dir`, whose `base` loses its own
/// leading slash instead.
fn file_name_concat(dir: &[u8], base: &[u8]) -> Vec<u8> {
    let dirbase = last_component(dir);
    let dirbase_start = dir.len().saturating_sub(dirbase.len());
    let dirbaselen = base_len(dirbase);
    let dirlen = dirbase_start.saturating_add(dirbaselen);
    let mut base = base;
    let mut sep = false;
    if dirbaselen > 0 {
        let dir_ends_slash = dirlen > 0 && dir.get(dirlen.saturating_sub(1)) == Some(&b'/');
        if !dir_ends_slash && base.first() != Some(&b'/') {
            sep = true;
        }
    } else if base.first() == Some(&b'/') {
        base = base.get(1..).unwrap_or_default();
    }
    let mut out = dir.get(..dirlen).unwrap_or(dir).to_vec();
    if sep {
        out.push(b'/');
    }
    out.extend_from_slice(base);
    out
}

/// GNU's `--help`, minus the ancillary block of URLs.
const HELP: &str = "\
Usage: mktemp [OPTION]... [TEMPLATE]
Create a temporary file or directory, safely, and print its name.
TEMPLATE must contain at least 3 consecutive 'X's in last component.
If TEMPLATE is not specified, use tmp.XXXXXXXXXX, and --tmpdir is implied.
Files are created u+rw, and directories u+rwx, minus umask restrictions.

  -d, --directory     create a directory, not a file
  -u, --dry-run       do not create anything; merely print a name (unsafe)
  -q, --quiet         suppress diagnostics about file/dir-creation failure
      --suffix=SUFF   append SUFF to TEMPLATE; SUFF must not contain a slash.
                        This option is implied if TEMPLATE does not end in X
  -p DIR, --tmpdir[=DIR]  interpret TEMPLATE relative to DIR; if DIR is not
                        specified, use $TMPDIR if set, else /tmp.  With
                        this option, TEMPLATE must not be an absolute name;
                        unlike with -t, TEMPLATE may contain slashes, but
                        mktemp creates only the final component
  -t                  interpret TEMPLATE as a single file name component,
                        relative to a directory: $TMPDIR, if set; else the
                        directory specified via -p; else /tmp [deprecated]
      --help        display this help and exit
      --version     output version information and exit
";

/// gnulib's `letters`: the 62 characters an `X` becomes.
// Used by `imp`, which is unix-only; the host build keeps it for its tests.
#[cfg_attr(not(unix), allow(dead_code))]
const LETTERS: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Random base-62 digits, unbiased: gnulib's `try_tempname_len` draws 64 bits
/// and takes ten digits from them, discarding a draw from the biased top of
/// the range.
#[cfg_attr(not(unix), allow(dead_code))] // As `LETTERS`.
struct Digits {
    value: u64,
    left: u32,
}

#[cfg_attr(not(unix), allow(dead_code))] // As `LETTERS`.
impl Digits {
    /// 62^10, the most digits one draw gives without bias.
    const POWER: u64 = 839_299_365_868_340_224;
    const PER_DRAW: u32 = 10;

    fn new() -> Digits {
        Digits { value: 0, left: 0 }
    }

    fn next(&mut self) -> Result<u8, randrange::EntropyError> {
        if self.left == 0 {
            let biased_min = u64::MAX - u64::MAX % Self::POWER;
            loop {
                let mut bytes = [0u8; 8];
                randrange::fill_secret(&mut bytes)?;
                let v = u64::from_ne_bytes(bytes);
                if v < biased_min {
                    self.value = v;
                    break;
                }
            }
            self.left = Self::PER_DRAW;
        }
        let digit = LETTERS
            .get(usize::try_from(self.value % 62).unwrap_or(0))
            .copied()
            .unwrap_or(b'a');
        self.value /= 62;
        self.left = self.left.saturating_sub(1);
        Ok(digit)
    }
}

#[cfg(unix)]
mod imp {
    use super::{Digits, Plan, Settings};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::{os_from_bytes, quote};
    use coreutils::stdfd::{self, Stream};
    use std::io::{self, Write as _};
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::process::ExitCode;

    /// What `gen_tempname_len` makes of a name.
    #[derive(Clone, Copy)]
    enum Kind {
        File,
        Dir,
        /// `-u`: nothing; the name must merely be free.
        NoCreate,
    }

    /// `main` from the plan: make the file or directory, print its name, and
    /// remove it again if the name cannot be printed.
    pub fn make(set: &Settings, plan: &Plan, mut out: Stream) -> ExitCode {
        let kind = if set.dry_run {
            Kind::NoCreate
        } else if set.directory {
            Kind::Dir
        } else {
            Kind::File
        };
        let mut name = plan.template.clone();
        if let Err(e) = gen_tempname(&mut name, plan.suffix_len, plan.x_count, kind) {
            if !set.quiet {
                let what = if set.directory { "directory" } else { "file" };
                diag!(
                    "mktemp: failed to create {what} via template {}: {}",
                    quote(&plan.template),
                    strerror(&e)
                );
            }
            return stdfd::close_stdout("mktemp", out, ExitCode::FAILURE);
        }
        // `puts`. A failed write is found by the close below.
        let _ = out.write_all(&name);
        let _ = out.write_all(b"\n");
        if set.dry_run {
            return stdfd::close_stdout("mktemp", out, ExitCode::SUCCESS);
        }
        // `close_stream (stdout)` now rather than at exit, so that a name
        // that could not be printed takes its file with it.
        match out.finish() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                let path = os_from_bytes(&name);
                // `remove`: a file or an empty directory.
                let removed = if set.directory {
                    std::fs::remove_dir(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                // Nothing more can be done about a leftover the run is
                // already failing over.
                let _ = removed;
                if !set.quiet {
                    stdfd::write_error("mktemp", &e);
                }
                ExitCode::FAILURE
            }
        }
    }

    /// gnulib's `gen_tempname_len`: replace the X's with random characters
    /// until a name is made (or found free), giving up after 62^3 names taken.
    fn gen_tempname(
        name: &mut [u8],
        suffix_len: usize,
        x_len: usize,
        kind: Kind,
    ) -> io::Result<()> {
        let end = name.len().saturating_sub(suffix_len);
        let start = end.saturating_sub(x_len);
        let mut digits = Digits::new();
        for _ in 0..62u32 * 62 * 62 {
            if let Some(xs) = name.get_mut(start..end) {
                for x in xs {
                    *x = digits.next().map_err(|_| {
                        io::Error::other("the system random number generator is unavailable")
                    })?;
                }
            }
            match try_name(name, kind) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::from(io::ErrorKind::AlreadyExists))
    }

    /// `try_file`, `try_dir` and `try_nocreate`.
    fn try_name(name: &[u8], kind: Kind) -> io::Result<()> {
        let path = os_from_bytes(name);
        match kind {
            Kind::File => std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map(drop),
            Kind::Dir => std::fs::DirBuilder::new().mode(0o700).create(&path),
            Kind::NoCreate => match std::fs::symlink_metadata(&path) {
                Ok(_) => Err(io::Error::from(io::ErrorKind::AlreadyExists)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            },
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::{Plan, Settings};
    use coreutils::diag;
    use coreutils::stdfd::Stream;
    use std::process::ExitCode;

    pub fn make(_set: &Settings, _plan: &Plan, _out: Stream) -> ExitCode {
        diag!("mktemp: unix-only utility; not supported on this platform");
        ExitCode::FAILURE
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

    fn settings(words: &[&str]) -> Settings {
        let args: Vec<OsString> = words.iter().map(OsString::from).collect();
        match parse_args(&args).unwrap() {
            Request::Run(s) => s,
            other => panic!("{other:?}"),
        }
    }

    fn planned(words: &[&str], tmpdir: Option<&str>) -> Result<Plan, String> {
        plan(&settings(words), tmpdir.map(str::as_bytes))
    }

    #[test]
    fn the_xs_are_the_last_run_before_the_suffix() {
        let p = planned(&["fooXXXbar"], None).unwrap();
        assert_eq!((p.x_count, p.suffix_len), (3, 3));
        let p = planned(&["aXXXXXX"], None).unwrap();
        assert_eq!((p.x_count, p.suffix_len), (6, 0));
        let e = planned(&["fooXXbarX"], None).unwrap_err();
        assert!(e.starts_with("too few X's in template "), "{e}");
        let p = planned(&["--suffix=.txt", "aXXX"], None).unwrap();
        assert_eq!(p.template, b"aXXX.txt");
        assert_eq!((p.x_count, p.suffix_len), (3, 4));
        let e = planned(&["--suffix=.t", "aXXXb"], None).unwrap_err();
        assert!(e.starts_with("with --suffix, template "), "{e}");
        let e = planned(&["--suffix=/x", "aXXX"], None).unwrap_err();
        assert!(e.starts_with("invalid suffix "), "{e}");
    }

    #[test]
    fn where_the_name_goes() {
        // No template: tmp.XXXXXXXXXX under $TMPDIR or /tmp.
        assert_eq!(planned(&[], None).unwrap().template, b"/tmp/tmp.XXXXXXXXXX");
        assert_eq!(
            planned(&[], Some("/var")).unwrap().template,
            b"/var/tmp.XXXXXXXXXX"
        );
        // -p's directory outranks $TMPDIR; -t's $TMPDIR outranks -p's.
        assert_eq!(
            planned(&["-p", "/d", "aXXX"], Some("/e")).unwrap().template,
            b"/d/aXXX"
        );
        assert_eq!(
            planned(&["-t", "-p", "/d", "aXXX"], Some("/e"))
                .unwrap()
                .template,
            b"/e/aXXX"
        );
        assert_eq!(
            planned(&["-t", "-p", "/d", "aXXX"], None).unwrap().template,
            b"/d/aXXX"
        );
        assert_eq!(
            planned(&["--tmpdir", "aXXX"], None).unwrap().template,
            b"/tmp/aXXX"
        );
        // An empty one is no directory at all.
        assert_eq!(
            planned(&["-p", "", "aXXX"], Some("")).unwrap().template,
            b"/tmp/aXXX"
        );
        // A template with a directory is refused by -t, and an absolute one by -p.
        assert!(planned(&["-t", "a/bXXX"], None).is_err());
        assert!(planned(&["-p", "/d", "/aXXX"], None).is_err());
        assert_eq!(
            planned(&["-p", "/d", "a/bXXX"], None).unwrap().template,
            b"/d/a/bXXX"
        );
        // Without -p or -t a template is used as it is.
        assert_eq!(planned(&["aXXX"], Some("/e")).unwrap().template, b"aXXX");
    }

    #[test]
    fn file_name_concat_is_gnulibs() {
        assert_eq!(file_name_concat(b"/tmp", b"x"), b"/tmp/x");
        assert_eq!(file_name_concat(b"/tmp/", b"x"), b"/tmp/x");
        assert_eq!(file_name_concat(b"/tmp//", b"x"), b"/tmp/x");
        assert_eq!(file_name_concat(b"/", b"x"), b"/x");
        assert_eq!(file_name_concat(b"/", b"/x"), b"/x");
        assert_eq!(file_name_concat(b".", b"x"), b"./x");
        assert_eq!(file_name_concat(b"a//b/", b"x"), b"a//b/x");
        assert_eq!(file_name_concat(b"a", b"/x"), b"a/x");
    }

    #[test]
    fn too_many_templates_refers_to_help() {
        let args: Vec<OsString> = ["a", "b"].iter().map(OsString::from).collect();
        let e = parse_args(&args).unwrap_err();
        assert_eq!(e.sentence, "too many templates");
        assert!(e.referral.is_some());
    }

    #[test]
    fn digits_are_from_the_62() {
        let mut d = Digits::new();
        for _ in 0..100 {
            assert!(LETTERS.contains(&d.next().unwrap()));
        }
    }
}
