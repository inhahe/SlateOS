//! `mknod` — make block or character special files, or FIFOs.
//!
//! ```text
//! Usage: mknod [OPTION]... NAME TYPE [MAJOR MINOR]
//! ```
//!
//! A port of GNU coreutils 9.4's `src/mknod.c`. There was no `mknod`; scripts
//! that set up a chroot or a container's `/dev` call it by name.
//!
//! # TYPE, and how many operands follow it
//!
//! Only TYPE's first character counts -- `mknod /dev/rst0 character 18 0` is
//! allowed -- and it decides the operand count before anything else is read:
//! `p` takes none, anything else takes MAJOR and MINOR, and a missing or extra
//! pair gets a second line saying which kind of file needs what. MAJOR and
//! MINOR are read as C reads an integer literal (`0x` hex, leading `0` octal)
//! and must each fit in 32 bits; the pair is packed the way glibc's `makedev`
//! packs it, which is also this system's `gnu_dev_makedev`.
//!
//! # The mode
//!
//! `a=rw` less the umask, unless `-m` says otherwise. With `-m` the mode is
//! computed against the umask (read and put back, not cleared), the node is
//! created -- the kernel applies the umask again -- and then set to exactly
//! that mode with `lchmod`, which cannot be redirected through a symlink put
//! in the new node's place. A bad `-m` is reported before the operands are
//! even counted, which is upstream's order and not `mkfifo`'s.
//!
//! # Options this implementation does not have
//!
//! `-Z` and `--context`, as in `mkfifo`: refused by name rather than ignored,
//! because silently dropping a requested security context is the defect.
//!
//! # On this system
//!
//! FIFOs are made with `mkfifo(2)`. Device nodes reach `mknod(2)`, which the
//! libc still answers with `ENOSYS` once it has checked the request -- so
//! `mknod x b 1 1` fails with `Function not implemented` (or `Operation not
//! permitted` without `CAP_MKNOD`) rather than pretending.
//!
//! # Checked against GNU
//!
//! `scripts/mknod-diff.sh`.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::xnum::{Status, xstrtoumax_base};
use std::ffi::{OsStr, OsString};

coreutils::guard_std_fds!();

const MKNOD: Program = Program::new("mknod", 1);

/// Upstream's `longopts[]`, in declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("context", Takes::Optional),
    ("mode", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

const SHORT_OPTIONS: &str = "m:Z";

/// `MODE_RW_UGO`: what a node gets before the umask when `-m` says nothing.
const BASE_MODE: u32 = 0o666;

/// `S_IFBLK`, `S_IFCHR`, as `<sys/stat.h>` has them on Linux and here.
const S_IFBLK: u32 = 0o060_000;
const S_IFCHR: u32 = 0o020_000;

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Make {
        mode: Option<OsString>,
        operands: Vec<OsString>,
    },
}

/// What the operands asked for, once counted and read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Node {
    Fifo,
    /// `S_IFBLK` or `S_IFCHR`, and the packed device number.
    Device {
        kind: u32,
        dev: u64,
    },
}

fn help_text() -> String {
    "\
Usage: mknod [OPTION]... NAME TYPE [MAJOR MINOR]
Create the special file NAME of the given TYPE.

Mandatory arguments to long options are mandatory for short options too.
  -m, --mode=MODE    set file permission bits to MODE, not a=rw - umask
  -Z                   set the SELinux security context to default type
      --context[=CTX]  like -Z, or if CTX is specified then set the SELinux
                         or SMACK security context to CTX
      --help        display this help and exit
      --version     output version information and exit

Both MAJOR and MINOR must be specified when TYPE is b, c, or u, and they
must be omitted when TYPE is p.  If MAJOR or MINOR begins with 0x or 0X,
it is interpreted as hexadecimal; otherwise, if it begins with 0, as octal;
otherwise, as decimal.  TYPE may be:

  b      create a block (buffered) special file
  c, u   create a character (unbuffered) special file
  p      create a FIFO

NOTE: your shell may have its own version of mknod, which usually supersedes
the version described here.  Please refer to your shell's documentation
for details about the options it supports.
"
    .to_string()
}

/// The option loop. Operands are checked later, after `-m`.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut mode: Option<OsString> = None;
    let mut operands: Vec<OsString> = Vec::new();
    for item in MKNOD.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'm', value) | Opt::Long("mode", value) => mode = value,
            Opt::Short(b'Z', _) => {
                return Err(
                    MKNOD.usage_referring("option -Z is not implemented by this mknod".to_string())
                );
            }
            Opt::Long("context", _) => {
                return Err(MKNOD.usage_referring(
                    "option '--context' is not implemented by this mknod".to_string(),
                ));
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(x) => operands.push(x.clone()),
            // Unreachable: every table entry is handled above.
            Opt::Long(other, _) => {
                return Err(MKNOD.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(MKNOD.invalid_option(c)),
        }
    }
    Ok(Request::Make { mode, operands })
}

/// Upstream's mode computation: `a=rw` adjusted by MODE against the umask.
///
/// # Errors
///
/// `invalid mode`, or `mode must specify only file permission bits` for a
/// mode with setuid, setgid or the sticky bit in it. Neither names the spec.
fn mode_for(spec: &OsStr, umask_value: u32) -> Result<u32, &'static str> {
    let changes = modechange::compile(&os_bytes(spec)).ok_or("invalid mode")?;
    let mode = modechange::adjust(BASE_MODE, false, umask_value, &changes).mode;
    if mode & !0o777 != 0 {
        return Err("mode must specify only file permission bits");
    }
    Ok(mode)
}

/// A MAJOR or MINOR: `xstrtoumax` in base 0 with no suffix, and it must fit
/// the 32-bit `major_t`/`minor_t`.
fn device_number(text: &OsStr) -> Option<u32> {
    match xstrtoumax_base(&os_bytes(text), 0, Some(b"")) {
        (value, Status::Ok) => u32::try_from(value).ok(),
        _ => None,
    }
}

/// glibc's `makedev`: twelve bits of major and eight of minor in the low word,
/// the rest above them.
fn makedev(major: u32, minor: u32) -> u64 {
    let (major, minor) = (u64::from(major), u64::from(minor));
    ((major & 0xfff) << 8) | ((major & !0xfff) << 32) | (minor & 0xff) | ((minor & !0xff) << 12)
}

/// The operand count and TYPE, as upstream checks them: a message, a second
/// line of explanation or `None`, and whether to refer to `--help`.
#[derive(Debug, PartialEq, Eq)]
struct Refusal {
    message: String,
    explanation: Option<&'static str>,
    referral: bool,
}

/// Count the operands against TYPE, then read TYPE and the device numbers.
///
/// # Errors
///
/// As upstream, in upstream's order: too few operands, too many, an unknown
/// TYPE (all three referred to `--help`), then a bad MAJOR, a bad MINOR, or a
/// pair that packs to `NODEV` (not referred).
fn read_operands(operands: &[OsString]) -> Result<(OsString, Node), Refusal> {
    let fifo_typed = operands
        .get(1)
        .is_some_and(|t| os_bytes(t).first() == Some(&b'p'));
    let expected = if operands.is_empty() || fifo_typed {
        2
    } else {
        4
    };
    if operands.len() < expected {
        let message = match operands.last() {
            None => "missing operand".to_string(),
            Some(last) => format!("missing operand after {}", quote(&os_bytes(last))),
        };
        let explanation = (expected == 4 && operands.len() == 2)
            .then_some("Special files require major and minor device numbers.");
        return Err(Refusal {
            message,
            explanation,
            referral: true,
        });
    }
    if operands.len() > expected {
        let extra = operands
            .get(expected)
            .map(|x| os_bytes(x).into_owned())
            .unwrap_or_default();
        let explanation = (expected == 2 && operands.len() == 4)
            .then_some("Fifos do not have major and minor device numbers.");
        return Err(Refusal {
            message: format!("extra operand {}", quote(&extra)),
            explanation,
            referral: true,
        });
    }
    let (Some(name), Some(kind)) = (operands.first(), operands.get(1)) else {
        // Unreachable: at least two operands were counted above.
        return Err(Refusal {
            message: "missing operand".to_string(),
            explanation: None,
            referral: true,
        });
    };
    let kind = match os_bytes(kind).first() {
        Some(b'p') => return Ok((name.clone(), Node::Fifo)),
        Some(b'b') => S_IFBLK,
        Some(b'c' | b'u') => S_IFCHR,
        _ => {
            return Err(Refusal {
                message: format!("invalid device type {}", quote(&os_bytes(kind))),
                explanation: None,
                referral: true,
            });
        }
    };
    let (Some(major_text), Some(minor_text)) = (operands.get(2), operands.get(3)) else {
        return Err(Refusal {
            message: "missing operand".to_string(),
            explanation: None,
            referral: true,
        });
    };
    let Some(major) = device_number(major_text) else {
        return Err(Refusal {
            message: format!(
                "invalid major device number {}",
                quote(&os_bytes(major_text))
            ),
            explanation: None,
            referral: false,
        });
    };
    let Some(minor) = device_number(minor_text) else {
        return Err(Refusal {
            message: format!(
                "invalid minor device number {}",
                quote(&os_bytes(minor_text))
            ),
            explanation: None,
            referral: false,
        });
    };
    let dev = makedev(major, minor);
    if dev == u64::MAX {
        // `NODEV`. Upstream prints the two operands bare, unquoted. Both have
        // just parsed as numbers, so they are ASCII -- white space, a sign,
        // digits, `x` -- and byte-for-char is exact rather than a decode.
        let bare =
            |text: &OsStr| -> String { os_bytes(text).iter().map(|&b| char::from(b)).collect() };
        return Err(Refusal {
            message: format!("invalid device {} {}", bare(major_text), bare(minor_text)),
            explanation: None,
            referral: false,
        });
    }
    Ok((name.clone(), Node::Device { kind, dev }))
}

#[cfg(unix)]
mod imp {
    use super::{MKNOD, Node, Refusal, Request, help_text, mode_for, parse_args, read_operands};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::pathname::c_path;
    use coreutils::quote::{quoteaf_os, quotef_os};
    use coreutils::stdfd::{self, Stream};
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::path::Path;
    use std::process::ExitCode;

    unsafe extern "C" {
        fn umask(mask: u32) -> u32;
        fn mknod(path: *const u8, mode: u32, dev: u64) -> i32;
        fn mkfifo(path: *const u8, mode: u32) -> i32;
        fn lchmod(path: *const u8, mode: u32) -> i32;
    }

    /// The umask, read the only way POSIX allows -- by setting it -- and put
    /// straight back.
    fn current_umask() -> u32 {
        // SAFETY: `umask` cannot fail and touches only this process's
        // file-mode creation mask; the second call restores the first's answer.
        unsafe {
            let value = umask(0);
            umask(value);
            value
        }
    }

    /// Run one libc call on `name` as a C string.
    fn with_c_path(name: &OsString, call: impl FnOnce(*const u8) -> i32) -> io::Result<()> {
        let path = c_path(Path::new(name))?;
        if call(path.as_ptr()) == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn refuse(refusal: &Refusal) -> ExitCode {
        diag!("mknod: {}", refusal.message);
        if let Some(line) = refusal.explanation {
            diag!("{line}");
        }
        if refusal.referral {
            diag!("Try 'mknod --help' for more information.");
        }
        ExitCode::FAILURE
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let (mode_spec, operands) = match parse_args(&args) {
            Ok(Request::Make { mode, operands }) => (mode, operands),
            Ok(Request::Help) => {
                let mut out = Stream::stdout();
                // Deliberately unread: `close_stdout` reports a failed write.
                let _ = out.write_all(help_text().as_bytes());
                return stdfd::close_stdout("mknod", out, ExitCode::SUCCESS);
            }
            Ok(Request::Version) => {
                let mut out = Stream::stdout();
                let _ = out.write_all(b"mknod (SlateOS coreutils) 0.1.0\n");
                return stdfd::close_stdout("mknod", out, ExitCode::SUCCESS);
            }
            Err(e) => {
                MKNOD.report(&e);
                return ExitCode::FAILURE;
            }
        };

        // The mode first: upstream reports a bad `-m` before it counts.
        let mode = match &mode_spec {
            None => super::BASE_MODE,
            Some(spec) => match mode_for(spec, current_umask()) {
                Ok(mode) => mode,
                Err(message) => {
                    diag!("mknod: {message}");
                    return ExitCode::FAILURE;
                }
            },
        };

        let (name, node) = match read_operands(&operands) {
            Ok(read) => read,
            Err(refusal) => return refuse(&refusal),
        };

        let made = match node {
            // SAFETY (both): the pointer is a NUL-terminated path that outlives
            // the call, which reads it without keeping it.
            Node::Fifo => with_c_path(&name, |p| unsafe { mkfifo(p, mode) }),
            Node::Device { kind, dev } => {
                with_c_path(&name, |p| unsafe { mknod(p, mode | kind, dev) })
            }
        };
        if let Err(e) = made {
            diag!("mknod: {}: {}", quotef_os(&name), strerror(&e));
            return ExitCode::FAILURE;
        }
        // The kernel applied the umask to the mode just asked for; `-m` wants
        // exactly its own, and `lchmod` will not follow a symlink someone put
        // in the node's place meanwhile.
        if mode_spec.is_some() {
            // SAFETY: as above.
            if let Err(e) = with_c_path(&name, |p| unsafe { lchmod(p, mode) }) {
                diag!(
                    "mknod: cannot set permissions of {}: {}",
                    quoteaf_os(&name),
                    strerror(&e)
                );
                return ExitCode::FAILURE;
            }
        }
        let out = Stream::stdout();
        stdfd::close_stdout("mknod", out, ExitCode::SUCCESS)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has no `mknod(2)`.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("mknod: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_fifo_takes_two_operands() {
        assert_eq!(
            read_operands(&argv(&["q", "p"])),
            Ok(("q".into(), Node::Fifo))
        );
        // Only the first character counts.
        assert_eq!(
            read_operands(&argv(&["q", "pipe"])),
            Ok(("q".into(), Node::Fifo))
        );
        let e = read_operands(&argv(&["q", "p", "1", "2"])).unwrap_err();
        assert_eq!(e.message, "extra operand ‘1’");
        assert_eq!(
            e.explanation,
            Some("Fifos do not have major and minor device numbers.")
        );
        let e = read_operands(&argv(&["q", "p", "1"])).unwrap_err();
        assert_eq!(
            (e.message.as_str(), e.explanation),
            ("extra operand ‘1’", None)
        );
    }

    #[test]
    fn a_device_takes_four() {
        assert_eq!(
            read_operands(&argv(&["d", "b", "8", "1"])),
            Ok((
                "d".into(),
                Node::Device {
                    kind: S_IFBLK,
                    dev: makedev(8, 1)
                }
            ))
        );
        assert!(matches!(
            read_operands(&argv(&["d", "character", "0x10", "010"])),
            Ok((_, Node::Device { kind: S_IFCHR, dev })) if dev == makedev(16, 8)
        ));
        assert!(matches!(
            read_operands(&argv(&["d", "u", "1", "3"])),
            Ok((_, Node::Device { kind: S_IFCHR, .. }))
        ));
        let e = read_operands(&argv(&["d", "b"])).unwrap_err();
        assert_eq!(e.message, "missing operand after ‘b’");
        assert_eq!(
            e.explanation,
            Some("Special files require major and minor device numbers.")
        );
        let e = read_operands(&argv(&["d", "b", "1"])).unwrap_err();
        assert_eq!(
            (e.message.as_str(), e.explanation),
            ("missing operand after ‘1’", None)
        );
    }

    #[test]
    fn operand_counts_come_before_the_type() {
        let e = read_operands(&argv(&[])).unwrap_err();
        assert_eq!(e.message, "missing operand");
        let e = read_operands(&argv(&["d"])).unwrap_err();
        assert_eq!(e.message, "missing operand after ‘d’");
        let e = read_operands(&argv(&["d", "x", "1", "2"])).unwrap_err();
        assert_eq!(
            (e.message.as_str(), e.referral),
            ("invalid device type ‘x’", true)
        );
    }

    #[test]
    fn device_numbers_are_c_literals_that_fit_32_bits() {
        assert_eq!(device_number(OsStr::new("0x1f")), Some(31));
        assert_eq!(device_number(OsStr::new("017")), Some(15));
        assert_eq!(device_number(OsStr::new("4294967295")), Some(u32::MAX));
        assert_eq!(device_number(OsStr::new("4294967296")), None);
        assert_eq!(device_number(OsStr::new("1k")), None);
        assert_eq!(device_number(OsStr::new("-1")), None);
        let e = read_operands(&argv(&["d", "b", "x", "1"])).unwrap_err();
        assert_eq!(
            (e.message.as_str(), e.referral),
            ("invalid major device number ‘x’", false)
        );
        let e = read_operands(&argv(&["d", "b", "1", "x"])).unwrap_err();
        assert_eq!(e.message, "invalid minor device number ‘x’");
    }

    #[test]
    fn makedev_packs_as_glibc_does() {
        assert_eq!(makedev(8, 1), 0x801);
        assert_eq!(makedev(0x1000, 0x100), (0x1000_u64 << 32) | (0x100 << 12));
        assert_eq!(makedev(u32::MAX, u32::MAX), u64::MAX);
        let e = read_operands(&argv(&["d", "b", "4294967295", "4294967295"])).unwrap_err();
        assert_eq!(e.message, "invalid device 4294967295 4294967295");
    }

    #[test]
    fn modes() {
        assert_eq!(mode_for(OsStr::new("600"), 0o022), Ok(0o600));
        assert_eq!(mode_for(OsStr::new("u=rw,go="), 0o022), Ok(0o600));
        // No `who`: the umask masks what is added -- it removes group and other
        // write, not execute, so `+x` adds all three x bits...
        assert_eq!(mode_for(OsStr::new("+x"), 0o022), Ok(0o777));
        // ...and `+w` from nothing adds only the owner's.
        assert_eq!(mode_for(OsStr::new("a=,+w"), 0o022), Ok(0o200));
        assert_eq!(mode_for(OsStr::new("zzz"), 0o022), Err("invalid mode"));
        assert_eq!(
            mode_for(OsStr::new("4755"), 0o022),
            Err("mode must specify only file permission bits")
        );
    }

    #[test]
    fn options() {
        assert_eq!(
            parse_args(&argv(&["-m", "600", "q", "p"])).unwrap(),
            Request::Make {
                mode: Some("600".into()),
                operands: argv(&["q", "p"])
            }
        );
        assert!(parse_args(&argv(&["-Z", "q", "p"])).is_err());
        assert!(parse_args(&argv(&["--context=x", "q", "p"])).is_err());
        assert_eq!(parse_args(&argv(&["q", "--help"])).unwrap(), Request::Help);
    }
}
