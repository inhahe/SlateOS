//! `pathchk` — check whether file names are valid or portable.
//!
//! ```text
//! Usage: pathchk [OPTION]... NAME...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/pathchk.c`. POSIX requires the utility.
//!
//! # Replaces `nproc`'s `pathchk` personality
//!
//! `userspace/nproc` answered to `pathchk` by reading its own `argv[0]`, and
//! nothing ever started it under that name, so the command did not exist. What
//! it would have done was also not `pathchk`: it never looked at the
//! filesystem, so a name under an unreadable directory passed, and it checked
//! every name against fixed limits of 4096 and 255 rather than asking the
//! directory, so `-p` -- which is *defined* by POSIX's minimums of 256 and 14 --
//! accepted names that the portability check exists to reject. The branch is
//! gone; this is the one program with the name.
//!
//! # What is checked, and in what order
//!
//! Upstream's `validate_file_name`, which stops at the first problem with each
//! name:
//!
//! 1. with `-P`, a component starting with `-`;
//! 2. with `-p` or `-P`, an empty name;
//! 3. with `-p`, a byte outside `A-Z a-z 0-9 . _ - /`; *without* `-p`, whether
//!    the name can be looked up at all (`lstat` -- `ENOENT` is fine, anything
//!    else, like an unsearchable directory, is reported);
//! 4. the whole length, against 256 with `-p` or else `pathconf(_PC_PATH_MAX)`
//!    -- the latter asked only for a name that does not exist and is at least
//!    256 bytes, since anything shorter fits on every POSIX system;
//! 5. each component's length, against 14 with `-p` or else
//!    `pathconf(_PC_NAME_MAX)` of the directory it would be in -- again asked
//!    only when some component of a name that does not exist is over 14. A
//!    directory that does not exist lends its parent's limit to everything
//!    below it.
//!
//! # One deliberate difference
//!
//! When `pathconf(_PC_PATH_MAX)` answers "no limit" (`-1` with `errno`
//! untouched), upstream takes `MIN (-1, ...)` as the limit and so reports every
//! name as `limit -2 exceeded`. Here "no limit" means no limit. Neither Linux
//! nor this system's libc ever gives that answer for `/` or `.`, so the two
//! cannot be told apart on either; it is recorded so that a port to a system
//! that does is not surprised.
//!
//! # Checked against GNU
//!
//! `scripts/pathchk-diff.sh`.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::pathname::NameLimit;
use coreutils::quote::{Mb, next_mb, quote, quoteaf, quotef};
use std::ffi::OsString;
use std::io;

coreutils::guard_std_fds!();

const PATHCHK: Program = Program::new("pathchk", 1);

/// Upstream's `"+pP"`: the `+` stops at the first operand, so in
/// `pathchk a -p` the `-p` is a second name to check.
const SHORT_OPTIONS: &str = "+pP";

/// Upstream's `longopts[]`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("portability", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `_POSIX_PATH_MAX`: the longest name every POSIX system must accept.
const POSIX_PATH_MAX: u64 = 256;

/// `_POSIX_NAME_MAX`: the longest component every POSIX system must accept.
const POSIX_NAME_MAX: u64 = 14;

/// Upstream's `NAME_MAX_MINIMUM` and `PATH_MAX_MINIMUM`: `_XOPEN_NAME_MAX`
/// and `_XOPEN_PATH_MAX` where the C library defines them, else the POSIX
/// minimums. glibc defines neither (measured), so GNU on Linux is built with
/// 14 and 256, and so is this.
const NAME_MAX_MINIMUM: u64 = POSIX_NAME_MAX;
const PATH_MAX_MINIMUM: u64 = POSIX_PATH_MAX;

/// Which checks were asked for.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Checks {
    /// `-p`: POSIX's minimum limits and portable character set.
    basic: bool,
    /// `-P`: no empty names and no component starting with `-`.
    extra: bool,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Check(Checks, Vec<OsString>),
}

fn help_text() -> String {
    "\
Usage: pathchk [OPTION]... NAME...
Diagnose invalid or non-portable file names.

  -p                  check for most POSIX systems
  -P                  check for empty names and leading \"-\"
      --portability   check for all POSIX systems (equivalent to -p -P)
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// Upstream's `getopt_long` loop, then its operand check.
///
/// # Errors
///
/// An unknown option, or no NAME at all (`missing operand`).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut checks = Checks::default();
    let mut names = Vec::new();
    for item in PATHCHK.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'p', _) => checks.basic = true,
            Opt::Short(b'P', _) => checks.extra = true,
            Opt::Long("portability", _) => {
                checks.basic = true;
                checks.extra = true;
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(name) => names.push(name.clone()),
            // Unreachable: every name in the table is handled above.
            Opt::Long(other, _) => {
                return Err(PATHCHK.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(PATHCHK.invalid_option(c)),
        }
    }
    if names.is_empty() {
        return Err(PATHCHK.usage_referring("missing operand".to_string()));
    }
    Ok(Request::Check(checks, names))
}

/// What a check needs to ask the system. A trait so that the tests can
/// script both answers, including the ones no real directory gives on demand.
trait System {
    /// `lstat`, reduced to whether the name resolves.
    fn lstat(&self, name: &[u8]) -> io::Result<()>;
    /// [`coreutils::pathname::pathconf`].
    fn pathconf(&self, dir: &[u8], limit: NameLimit) -> io::Result<Option<u64>>;
}

/// Each component of `file` with the offset it starts at: upstream's
/// `component_start`/`component_len` walk. Runs of `/` separate and are
/// skipped; they never make an empty component.
fn components(file: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let rest = file.get(at..)?;
        let skip = rest.iter().take_while(|&&b| b == b'/').count();
        let start = at.checked_add(skip)?;
        let tail = file.get(start..)?;
        if tail.is_empty() {
            return None;
        }
        let len = tail.iter().position(|&b| b == b'/').unwrap_or(tail.len());
        at = start.checked_add(len)?;
        Some((start, tail.get(..len)?))
    })
}

/// `no_leading_hyphen`: is there a `-` at the start of the name or right
/// after a `/`?
fn has_leading_hyphen(file: &[u8]) -> bool {
    file.first() == Some(&b'-') || file.windows(2).any(|w| w == b"/-")
}

/// Whether `b` is in POSIX's portable filename character set, plus `/`.
fn is_portable(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/')
}

/// `portable_chars_only`'s report for the first byte outside the set, if any.
///
/// The character is quoted whole when the bytes there make one (what
/// upstream's `mbrlen` measures), and as its single byte otherwise.
fn nonportable_character(file: &[u8]) -> Option<String> {
    let at = file.iter().position(|&b| !is_portable(b))?;
    let rest = file.get(at..)?;
    let len = match next_mb(rest) {
        Some(Mb::Char(_, n)) => n,
        _ => 1,
    };
    Some(format!(
        "non-portable character {} in file name {}",
        quote(rest.get(..len).unwrap_or(rest)),
        quoteaf(file)
    ))
}

/// `limit N exceeded by length M ...`: upstream prints `maxsize - 1` for the
/// whole name, which is signed, so a limit of 0 reads `-1`.
fn one_less(n: u64) -> i128 {
    i128::from(n).saturating_sub(1)
}

/// The length of a name as the unsigned count the limits are compared with.
fn length_of(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// Upstream's `validate_file_name`: `true` if `file` passes every check
/// asked for; otherwise the reason is pushed onto `diags` and the rest of the
/// checks are skipped.
fn validate(file: &[u8], checks: Checks, sys: &impl System, diags: &mut Vec<String>) -> bool {
    let filelen = length_of(file);

    if checks.extra && has_leading_hyphen(file) {
        diags.push(format!(
            "leading '-' in a component of file name {}",
            quoteaf(file)
        ));
        return false;
    }

    // Empty names are not portable. As of 2005 POSIX did not say whether
    // `pathchk -p ''` should fail, so upstream's failing it is a choice.
    if (checks.basic || checks.extra) && file.is_empty() {
        diags.push("empty file name".to_string());
        return false;
    }

    let mut file_exists = false;
    if checks.basic {
        if let Some(message) = nonportable_character(file) {
            diags.push(message);
            return false;
        }
    } else {
        // Can the name be looked up at all? Not existing is fine; an
        // unsearchable directory, a non-directory used as one, or a name the
        // system cannot even represent is not. `""` is allowed through only if
        // `lstat` accepts it, which on Linux it does not.
        match sys.lstat(file) {
            Ok(()) => file_exists = true,
            Err(e) if e.kind() == io::ErrorKind::NotFound && !file.is_empty() => {}
            Err(e) => {
                diags.push(format!("{}: {}", quotef(file), strerror(&e)));
                return false;
            }
        }
    }

    if checks.basic || (!file_exists && PATH_MAX_MINIMUM <= filelen) {
        let maxsize = if checks.basic {
            Some(POSIX_PATH_MAX)
        } else {
            let dir = if file.first() == Some(&b'/') {
                "/"
            } else {
                "."
            };
            match sys.pathconf(dir.as_bytes(), NameLimit::Path) {
                Ok(limit) => limit,
                Err(e) => {
                    // Upstream prints the directory bare, not quoted: it is
                    // always one of these two.
                    diags.push(format!(
                        "{dir}: unable to determine maximum file name length: {}",
                        strerror(&e)
                    ));
                    return false;
                }
            }
        };
        if let Some(maxsize) = maxsize {
            if maxsize <= filelen {
                diags.push(format!(
                    "limit {} exceeded by length {filelen} of file name {}",
                    one_less(maxsize),
                    quoteaf(file)
                ));
                return false;
            }
        }
    }

    // `pathconf(_PC_NAME_MAX)` is avoided when every component is short
    // enough for any filesystem; `-p` checks them all below regardless.
    let check_component_lengths = checks.basic
        || (!file_exists && components(file).any(|(_, c)| NAME_MAX_MINIMUM < length_of(c)));

    if check_component_lengths {
        // The limit for the current component. It starts at the minimum for
        // the sake of systems where `pathconf` fails on "." with ENOENT.
        let mut name_max = NAME_MAX_MINIMUM;
        // Nonzero once the limit is known for everything that follows: always
        // with `-p`, and from the first directory that does not exist.
        let mut known_name_max = if checks.basic { POSIX_NAME_MAX } else { 0 };
        for (start, component) in components(file) {
            if known_name_max != 0 {
                name_max = known_name_max;
            } else {
                // The directory this component would be in: the name up to
                // the component, trailing slashes and all.
                let dir: &[u8] = if start == 0 {
                    b"."
                } else {
                    file.get(..start).unwrap_or(b".")
                };
                match sys.pathconf(dir, NameLimit::Component) {
                    Ok(Some(limit)) => name_max = limit,
                    Ok(None) => name_max = u64::MAX,
                    // The directory does not exist: its parent's limit holds
                    // for it and everything under it.
                    Err(e) if e.kind() == io::ErrorKind::NotFound => known_name_max = name_max,
                    Err(e) => {
                        diags.push(format!("{}: {}", quotef(dir), strerror(&e)));
                        return false;
                    }
                }
            }
            let length = length_of(component);
            if name_max < length {
                diags.push(format!(
                    "limit {name_max} exceeded by length {length} of file name component {}",
                    quote(component)
                ));
                return false;
            }
        }
    }

    true
}

#[cfg(unix)]
mod imp {
    use super::{PATHCHK, Request, System, help_text, parse_args, validate};
    use coreutils::diag;
    use coreutils::pathname::{NameLimit, pathconf};
    use coreutils::quote::{os_bytes, os_from_bytes};
    use coreutils::stdfd::{self, Stream};
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::process::ExitCode;

    /// The real filesystem.
    struct Live;

    impl System for Live {
        fn lstat(&self, name: &[u8]) -> io::Result<()> {
            std::fs::symlink_metadata(os_from_bytes(name)).map(|_| ())
        }

        fn pathconf(&self, dir: &[u8], limit: NameLimit) -> io::Result<Option<u64>> {
            pathconf(dir, limit)
        }
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(e) => {
                PATHCHK.report(&e);
                return ExitCode::FAILURE;
            }
        };
        let mut out = Stream::stdout();
        let earned = match request {
            Request::Help => {
                // Deliberately unread: a failed write is `Stream`'s to
                // remember and `close_stdout`'s to report, once.
                let _ = out.write_all(help_text().as_bytes());
                ExitCode::SUCCESS
            }
            Request::Version => {
                let _ = out.write_all(b"pathchk (SlateOS coreutils) 0.1.0\n");
                ExitCode::SUCCESS
            }
            Request::Check(checks, names) => {
                let mut ok = true;
                for name in &names {
                    let mut diags = Vec::new();
                    ok &= validate(&os_bytes(name), checks, &Live, &mut diags);
                    for message in diags {
                        diag!("pathchk: {message}");
                    }
                }
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
        };
        stdfd::close_stdout("pathchk", out, earned)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has no `pathconf`.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("pathchk: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Built from kinds rather than raw numbers: the tests also run on the
    /// Windows host, where a raw OS error is not an errno.
    fn enoent() -> io::Error {
        io::Error::from(io::ErrorKind::NotFound)
    }
    fn eacces() -> io::Error {
        io::Error::from(io::ErrorKind::PermissionDenied)
    }

    /// A scripted system: `lstat` and `pathconf` answer from closures.
    struct Fake {
        lstat: fn(&[u8]) -> io::Result<()>,
        pathconf: fn(&[u8], NameLimit) -> io::Result<Option<u64>>,
    }

    impl System for Fake {
        fn lstat(&self, name: &[u8]) -> io::Result<()> {
            (self.lstat)(name)
        }
        fn pathconf(&self, dir: &[u8], limit: NameLimit) -> io::Result<Option<u64>> {
            (self.pathconf)(dir, limit)
        }
    }

    fn missing(_: &[u8]) -> io::Result<()> {
        Err(enoent())
    }

    /// Linux's answers for an ordinary filesystem.
    // The `Result` is the shape `Fake::pathconf` holds, not a choice.
    #[allow(clippy::unnecessary_wraps)]
    fn linux_limits(_: &[u8], limit: NameLimit) -> io::Result<Option<u64>> {
        Ok(Some(match limit {
            NameLimit::Component => 255,
            NameLimit::Path => 4096,
        }))
    }

    fn nothing_exists() -> Fake {
        Fake {
            lstat: missing,
            pathconf: linux_limits,
        }
    }

    fn run(file: &[u8], checks: Checks, sys: &Fake) -> (bool, Vec<String>) {
        let mut diags = Vec::new();
        let ok = validate(file, checks, sys, &mut diags);
        (ok, diags)
    }

    const NONE: Checks = Checks {
        basic: false,
        extra: false,
    };
    const P: Checks = Checks {
        basic: true,
        extra: false,
    };
    const BIG_P: Checks = Checks {
        basic: false,
        extra: true,
    };

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn components_skip_every_run_of_slashes() {
        let got: Vec<(usize, &[u8])> = components(b"//a//bc/").collect();
        assert_eq!(got, vec![(2, &b"a"[..]), (5, &b"bc"[..])]);
        assert_eq!(components(b"").count(), 0);
        assert_eq!(components(b"///").count(), 0);
        assert_eq!(components(b"x").collect::<Vec<_>>(), vec![(0, &b"x"[..])]);
    }

    #[test]
    fn a_short_name_that_does_not_exist_is_fine() {
        assert_eq!(run(b"a/b", NONE, &nothing_exists()), (true, vec![]));
    }

    #[test]
    fn a_name_that_cannot_be_looked_up_is_reported_with_its_errno() {
        let sys = Fake {
            lstat: |_| Err(eacces()),
            pathconf: linux_limits,
        };
        let (ok, diags) = run(b"d/x", NONE, &sys);
        assert!(!ok);
        assert_eq!(diags, vec!["d/x: Permission denied"]);
    }

    #[test]
    fn the_empty_name_fails_lstat_even_though_the_errno_is_enoent() {
        let (ok, diags) = run(b"", NONE, &nothing_exists());
        assert!(!ok);
        assert_eq!(diags, vec!["'': No such file or directory"]);
    }

    #[test]
    fn dash_p_uses_posix_minimums_not_the_directory() {
        // 14 is the component limit, whatever the directory says.
        let (ok, diags) = run(b"abcdefghijklmno", P, &nothing_exists());
        assert!(!ok);
        assert_eq!(
            diags,
            vec!["limit 14 exceeded by length 15 of file name component ‘abcdefghijklmno’"]
        );
        assert!(run(b"abcdefghijklmn", P, &nothing_exists()).0);
        // 256 bytes is already one too many for the whole name, so the
        // message names 255 as the limit.
        let mut long = b"aaaaaaaaaaaaa".to_vec();
        for _ in 0..19 {
            long.extend_from_slice(b"/abcdefghijkl");
        }
        assert_eq!(long.len(), 260);
        let (ok, diags) = run(&long, P, &nothing_exists());
        assert!(!ok);
        assert!(
            diags[0].starts_with("limit 255 exceeded by length 260 of file name "),
            "{diags:?}"
        );
    }

    #[test]
    fn dash_p_reports_the_first_nonportable_character_whole() {
        let (ok, diags) = run(b"a b", P, &nothing_exists());
        assert!(!ok);
        assert_eq!(diags, vec!["non-portable character ‘ ’ in file name 'a b'"]);
        // A two-byte character is quoted as the character...
        let (_, diags) = run("xé".as_bytes(), P, &nothing_exists());
        assert_eq!(diags, vec!["non-portable character ‘é’ in file name 'xé'"]);
        // ...and a byte that begins none as that byte.
        let (_, diags) = run(b"x\xffy", P, &nothing_exists());
        assert_eq!(
            diags,
            vec![r"non-portable character ‘\377’ in file name 'x'$'\377''y'"]
        );
    }

    #[test]
    fn dash_capital_p_refuses_empty_names_and_leading_dashes() {
        assert_eq!(
            run(b"", BIG_P, &nothing_exists()),
            (false, vec!["empty file name".to_string()])
        );
        assert_eq!(
            run(b"a/-b", BIG_P, &nothing_exists()),
            (
                false,
                vec!["leading '-' in a component of file name 'a/-b'".to_string()]
            )
        );
        assert!(!run(b"-a", BIG_P, &nothing_exists()).0);
        // A dash inside a component is fine.
        assert!(run(b"a-b/c-", BIG_P, &nothing_exists()).0);
        // Without -P, a leading dash is just a name.
        assert!(run(b"-a", NONE, &nothing_exists()).0);
    }

    #[test]
    fn a_long_component_asks_the_directory_it_would_be_in() {
        let sys = Fake {
            lstat: missing,
            pathconf: |dir, limit| {
                assert_eq!(limit, NameLimit::Component);
                // "." for the first component, then the name so far.
                match dir {
                    b"." => Ok(Some(255)),
                    b"a//" => Ok(Some(20)),
                    other => panic!("asked about {other:?}"),
                }
            },
        };
        let (ok, diags) = run(b"a//bbbbbbbbbbbbbbbbbbbbb", NONE, &sys);
        assert!(!ok);
        assert_eq!(
            diags,
            vec!["limit 20 exceeded by length 21 of file name component ‘bbbbbbbbbbbbbbbbbbbbb’"]
        );
    }

    #[test]
    fn a_directory_that_does_not_exist_lends_its_parents_limit_to_everything_below() {
        let sys = Fake {
            lstat: missing,
            pathconf: |dir, _| match dir {
                b"." => Ok(Some(16)),
                b"nodir/" => Err(enoent()),
                other => panic!("asked about {other:?} after the limit was known"),
            },
        };
        // 16 holds for `nodir/`'s children and theirs.
        assert!(run(b"nodir/aaaaaaaaaaaaaaa/bbbbbbbbbbbbbbbb", NONE, &sys).0);
        let (ok, diags) = run(b"nodir/aaaaaaaaaaaaaaaaa", NONE, &sys);
        assert!(!ok);
        assert!(
            diags[0].starts_with("limit 16 exceeded by length 17"),
            "{diags:?}"
        );
    }

    #[test]
    fn a_directory_pathconf_cannot_ask_is_reported() {
        let sys = Fake {
            lstat: missing,
            pathconf: |dir, _| match dir {
                b"." => Ok(Some(255)),
                _ => Err(eacces()),
            },
        };
        let (ok, diags) = run(b"my dir/aaaaaaaaaaaaaaaaaaaa", NONE, &sys);
        assert!(!ok);
        assert_eq!(diags, vec!["'my dir/': Permission denied"]);
    }

    #[test]
    fn an_existing_name_is_never_measured() {
        let sys = Fake {
            lstat: |_| Ok(()),
            pathconf: |_, _| panic!("an existing name needs no limit"),
        };
        assert!(run(&[b'a'; 300], NONE, &sys).0);
    }

    #[test]
    fn a_long_missing_name_is_measured_against_the_path_limit() {
        let sys = Fake {
            lstat: missing,
            pathconf: |dir, limit| match limit {
                NameLimit::Path => {
                    assert_eq!(dir, b"/");
                    Ok(Some(300))
                }
                NameLimit::Component => Ok(Some(255)),
            },
        };
        let mut name = b"/".to_vec();
        name.extend(std::iter::repeat_n(b"abc/".iter().copied(), 75).flatten());
        assert_eq!(name.len(), 301);
        let (ok, diags) = run(&name, NONE, &sys);
        assert!(!ok);
        assert!(diags[0].starts_with("limit 299 exceeded by length 301 of file name "));
        // And an unlimited answer is no limit.
        let unlimited = Fake {
            lstat: missing,
            pathconf: |_, _| Ok(None),
        };
        assert!(run(&name, NONE, &unlimited).0);
    }

    #[test]
    fn options() {
        assert_eq!(
            parse_args(&argv(&["-p", "a"])).unwrap(),
            Request::Check(P, argv(&["a"]))
        );
        assert_eq!(
            parse_args(&argv(&["--portability", "a"])).unwrap(),
            Request::Check(
                Checks {
                    basic: true,
                    extra: true
                },
                argv(&["a"])
            )
        );
        // `+`: the first operand ends the options.
        assert_eq!(
            parse_args(&argv(&["a", "-p"])).unwrap(),
            Request::Check(NONE, argv(&["a", "-p"]))
        );
        let e = parse_args(&argv(&["-p"])).unwrap_err();
        assert_eq!(
            e.message(),
            "missing operand\nTry 'pathchk --help' for more information."
        );
        let e = parse_args(&argv(&["-x", "a"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'pathchk --help' for more information."
        );
    }
}
