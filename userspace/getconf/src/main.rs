//! `getconf` -- query system configuration variables: glibc 2.39's
//! `posix/getconf.c`, ported.
//!
//! ```text
//! getconf [-v SPEC] VAR
//! getconf [-v SPEC] PATH_VAR PATH
//! getconf -a [PATH]
//! ```
//!
//! Prints the value of a configuration variable: a system limit or option
//! (`ARG_MAX`, `PAGESIZE`, `_NPROCESSORS_ONLN`), a limit of the file system
//! holding PATH (`NAME_MAX /`), or a string (`PATH`); with `-a`, every one.
//! Each name is answered by the C library -- `sysconf`, `pathconf` or
//! `confstr`, through `libcall::conf` -- so a script is told exactly what a C
//! program on the same system is told. That is the whole job: the `getconf`
//! this replaces printed a table of numbers typed into its own source, and was
//! deleted for it (known-issues `B-NO-GETCONF-OR-LOCALE-UNTIL-PORTED`).
//!
//! The names, their order and which call answers each are upstream's own
//! table, generated into [`vars`] by `scripts/getconf-gen.py` from glibc
//! 2.39's `getconf.c`, with the constants' values measured from glibc's
//! headers -- the Linux x86_64 numbering SlateOS's C library shares. A name
//! the library has no answer for prints `undefined`, as upstream's does for a
//! name its kernel does not support.
//!
//! What is upstream's, kept: `-v SPEC` is accepted and ignored (glibc built
//! for x86_64 defines every programming environment, so its getconf takes
//! that branch); `_POSIX_` may be left off a name that has it; `-a` pads each
//! name to 35 columns and leaves a value it could not get blank; `sysconf`'s
//! `-1` is `undefined` except for `UINT_MAX` and `ULONG_MAX`, whose values are
//! all ones and are printed unsigned; diagnostics name the program as it was
//! invoked (`error`'s `program_invocation_name`) while the usage line names
//! its last component (`__progname`); and a write that fails is ignored, as
//! upstream's `printf`s are, so `getconf -a > /dev/full` exits 0 as it does
//! there. `scripts/getconf-diff.sh` holds all of it to Ubuntu 24.04's getconf.
//!
//! Upstream's notice is the glibc entry of
//! `userspace/localtime/licenses/notices.yaml` (getconf.c is GPL-2.0-or-later).

use std::ffi::{CString, OsString};
use std::io::{self, Write};
use std::process::ExitCode;

#[cfg(test)]
mod tests;
mod vars;

/// Which call answers a name, and with which constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    /// `sysconf(constant)`.
    Sysconf(i32),
    /// `confstr(constant, ...)`.
    Confstr(i32),
    /// `pathconf(path, constant)`.
    Pathconf(i32),
}

/// One row of upstream's `vars[]`.
#[derive(Debug)]
pub(crate) struct Var {
    /// The name `getconf` answers to.
    pub(crate) name: &'static str,
    /// The C constant upstream passes, as it is spelt in the source.
    pub(crate) constant: &'static str,
    /// The call, and the constant's value.
    pub(crate) call: Call,
}

/// The two `sysconf` names whose value is all ones -- `UINT_MAX` is
/// representable, `ULONG_MAX` comes back as `-1` -- and which upstream prints
/// unsigned rather than as `undefined`.
const ALL_ONES: [&str; 2] = ["_SC_UINT_MAX", "_SC_ULONG_MAX"];

/// The C library, as `getconf` needs it. A trait so the tests can stand in a
/// library whose answers they choose.
pub(crate) trait Library {
    fn sysconf(&self, name: i32) -> i64;
    fn pathconf(&self, path: &CString, name: i32) -> Result<Option<i64>, i32>;
    fn confstr(&self, name: i32, buf: Option<&mut [u8]>) -> usize;
}

/// The library this program links.
struct Libc;

impl Library for Libc {
    fn sysconf(&self, name: i32) -> i64 {
        libcall::conf::sysconf(name)
    }
    fn pathconf(&self, path: &CString, name: i32) -> Result<Option<i64>, i32> {
        libcall::conf::pathconf(path, name)
    }
    fn confstr(&self, name: i32, buf: Option<&mut [u8]>) -> usize {
        libcall::conf::confstr(name, buf)
    }
}

/// How `run` ends: the exit status upstream's `exit` or `error` would give.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Exit(pub(crate) u8);

/// The two streams, and the names the diagnostics use.
pub(crate) struct Io<'a> {
    pub(crate) out: &'a mut dyn Write,
    pub(crate) err: &'a mut dyn Write,
    /// `argv[0]` as given: `error`'s `program_invocation_name`.
    pub(crate) invocation: Vec<u8>,
}

impl Io<'_> {
    /// The last component of `argv[0]`: `__progname`.
    fn short_name(&self) -> &[u8] {
        let s = &self.invocation;
        s.iter()
            .rposition(|&b| b == b'/')
            .map_or(s.as_slice(), |i| {
                s.get(i.saturating_add(1)..).unwrap_or_default()
            })
    }

    /// `usage`: two lines to standard error, exit 2.
    fn usage(&mut self) -> Exit {
        let name = self.short_name().to_vec();
        let mut text = b"Usage: ".to_vec();
        text.extend_from_slice(&name);
        text.extend_from_slice(b" [-v specification] variable_name [pathname]\n       ");
        text.extend_from_slice(&name);
        text.extend_from_slice(b" -a [pathname]\n");
        // Ignored, as upstream ignores `fprintf`'s result: there is nowhere
        // left to report a failure to write the usage.
        let _ = self.err.write_all(&text);
        Exit(2)
    }

    /// glibc's `error (status, errnum, ...)`: standard output flushed first,
    /// then `NAME: message[: strerror]` on standard error.
    fn error(&mut self, status: u8, errnum: i32, message: &[u8]) -> Exit {
        // Ignored, as `error`'s own `fflush (stdout)` result is.
        let _ = self.out.flush();
        let mut text = self.invocation.clone();
        text.extend_from_slice(b": ");
        text.extend_from_slice(message);
        if errnum != 0 {
            text.extend_from_slice(b": ");
            text.extend_from_slice(
                errmsg::strerror(&io::Error::from_raw_os_error(errnum)).as_bytes(),
            );
        }
        text.push(b'\n');
        let _ = self.err.write_all(&text);
        Exit(status)
    }

    /// A line of standard output. Upstream's `printf`s are unchecked, so a
    /// failed write is not this program's failure either.
    fn put(&mut self, bytes: &[u8]) {
        let _ = self.out.write_all(bytes);
    }
}

/// `confstr`'s value for `name`, as upstream fetches it: the size first, then
/// a buffer of that size. `Err` is `error`'s exit, already reported.
fn confstr_value(lib: &dyn Library, io: &mut Io<'_>, name: i32) -> Result<Vec<u8>, Exit> {
    let clen = lib.confstr(name, None);
    let mut cvalue = vec![0u8; clen];
    if lib.confstr(name, Some(&mut cvalue)) != clen {
        // `error (3, errno, "confstr")`: the second call disagreeing with the
        // first. The errno is the library's; there is no other to report.
        return Err(io.error(3, 0, b"confstr"));
    }
    // `printf ("%.*s\n", clen, cvalue)`: at most `clen` bytes, stopping at the
    // NUL -- which is the last of them when the value fitted.
    let end = cvalue.iter().position(|&b| b == 0).unwrap_or(cvalue.len());
    cvalue.truncate(end);
    Ok(cvalue)
}

/// `print_all`: every name, padded to 35 columns, then its value or nothing.
fn print_all(lib: &dyn Library, io: &mut Io<'_>, path: &[u8]) -> Exit {
    let path = c_path(path);
    for var in &vars::VARS {
        let mut line = format!("{:<35}", var.name).into_bytes();
        match var.call {
            Call::Pathconf(name) => {
                if let Ok(Some(value)) = lib.pathconf(&path, name) {
                    line.extend_from_slice(value.to_string().as_bytes());
                }
            }
            Call::Sysconf(name) => {
                let value = lib.sysconf(name);
                if value == -1 {
                    if ALL_ONES.contains(&var.constant) {
                        line.extend_from_slice(u64::MAX.to_string().as_bytes());
                    }
                } else {
                    line.extend_from_slice(value.to_string().as_bytes());
                }
            }
            Call::Confstr(name) => match confstr_value(lib, io, name) {
                Ok(value) => line.extend_from_slice(&value),
                Err(exit) => return exit,
            },
        }
        line.push(b'\n');
        io.put(&line);
    }
    Exit(0)
}

/// A path operand as the C string `pathconf` takes. An argument cannot hold
/// a NUL, so nothing is ever cut here; were one present, the path would end
/// there, as a C string handed the same bytes would.
fn c_path(path: &[u8]) -> CString {
    let end = path.iter().position(|&b| b == 0).unwrap_or(path.len());
    CString::new(path.get(..end).unwrap_or_default()).unwrap_or_default()
}

/// `main`, after `argv[0]`. `args` are the operands as bytes.
pub(crate) fn run(args: &[Vec<u8>], lib: &dyn Library, io: &mut Io<'_>) -> Exit {
    // `argv[i]` in upstream's numbering, where argv[0] is the program.
    let mut argv: Vec<&[u8]> = Vec::with_capacity(args.len().saturating_add(1));
    argv.push(b"getconf");
    argv.extend(args.iter().map(Vec::as_slice));

    if argv.get(1) == Some(&&b"--version"[..]) {
        io.put(
            b"getconf (GNU libc) 2.39\n\
              Copyright (C) 2024 Free Software Foundation, Inc.\n\
              This is free software; see the source for copying conditions.  There is NO\n\
              warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n\
              Written by Roland McGrath.\n",
        );
        return Exit(0);
    }
    if argv.get(1) == Some(&&b"--help"[..]) {
        io.put(
            b"Usage: getconf [-v SPEC] VAR\n  \
              or:  getconf [-v SPEC] PATH_VAR PATH\n\
              \n\
              Get the configuration value for variable VAR, or for variable PATH_VAR\n\
              for path PATH.  If SPEC is given, give values for compilation\n\
              environment SPEC.\n\n\
              For bug reporting instructions, please see:\n\
              <https://www.gnu.org/software/libc/bugs.html>.\n",
        );
        return Exit(0);
    }

    // ALL_ENVIRONMENTS_DEFINED: `-v SPEC` (or `-vSPEC`) is stepped over.
    let mut first = 1;
    if argv.get(1).is_some_and(|a| a.starts_with(b"-v")) {
        if argv.get(1) == Some(&&b"-v"[..]) {
            if argv.len() < 3 {
                return io.usage();
            }
            first = 3;
        } else {
            first = 2;
        }
    }
    // From here on upstream works on the shifted argv, whose argv[1] is
    // `rest[0]`: `-a` with at most one path, or after an optional `--` the
    // variable and at most one path (`argc - ai` of one or two).
    let rest = argv.get(first..).unwrap_or_default();

    if rest.first() == Some(&&b"-a"[..]) {
        return match rest.len() {
            1 => print_all(lib, io, b"/"),
            2 => print_all(lib, io, rest.get(1).copied().unwrap_or_default()),
            _ => io.usage(),
        };
    }

    let skip = usize::from(rest.first() == Some(&&b"--"[..]));
    let ops = rest.get(skip..).unwrap_or_default();
    let (Some(&wanted), 1..=2) = (ops.first(), ops.len()) else {
        return io.usage();
    };
    let operands = ops.len();

    for var in &vars::VARS {
        let name = var.name.as_bytes();
        let matches = name == wanted
            || name
                .strip_prefix(b"_POSIX_")
                .is_some_and(|tail| tail == wanted);
        if !matches {
            continue;
        }
        return match var.call {
            Call::Pathconf(constant) => {
                if operands < 2 {
                    return io.usage();
                }
                let path = ops.get(1).copied().unwrap_or_default();
                match lib.pathconf(&c_path(path), constant) {
                    Ok(Some(value)) => io.put(format!("{value}\n").as_bytes()),
                    Ok(None) => io.put(b"undefined\n"),
                    Err(errno) => {
                        let mut message = b"pathconf: ".to_vec();
                        message.extend_from_slice(path);
                        return io.error(3, errno, &message);
                    }
                }
                Exit(0)
            }
            Call::Sysconf(constant) => {
                if operands > 1 {
                    return io.usage();
                }
                let value = lib.sysconf(constant);
                if value == -1 {
                    if ALL_ONES.contains(&var.constant) {
                        io.put(format!("{}\n", u64::MAX).as_bytes());
                    } else {
                        io.put(b"undefined\n");
                    }
                } else {
                    io.put(format!("{value}\n").as_bytes());
                }
                Exit(0)
            }
            Call::Confstr(constant) => {
                if operands > 1 {
                    return io.usage();
                }
                match confstr_value(lib, io, constant) {
                    Ok(mut value) => {
                        value.push(b'\n');
                        io.put(&value);
                        Exit(0)
                    }
                    Err(exit) => exit,
                }
            }
        };
    }

    // `error (2, 0, _("Unrecognized variable `%s'"), argv[ai])`.
    let mut message = b"Unrecognized variable `".to_vec();
    message.extend_from_slice(wanted);
    message.push(b'\'');
    io.error(2, 0, &message)
}

/// An argument's bytes. `as_encoded_bytes` is the bytes themselves on Unix,
/// where this program runs; elsewhere it is the platform's own encoding,
/// which only the host's tests ever see.
fn bytes(arg: OsString) -> Vec<u8> {
    arg.as_encoded_bytes().to_vec()
}

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let invocation = args
        .next()
        .map(bytes)
        .unwrap_or_else(|| b"getconf".to_vec());
    let operands: Vec<Vec<u8>> = args.map(bytes).collect();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let stderr = io::stderr();
    let mut err = stderr.lock();
    let mut io = Io {
        out: &mut out,
        err: &mut err,
        invocation,
    };
    let Exit(status) = run(&operands, &Libc, &mut io);
    // `exit`'s flush of stdout, whose failure upstream never sees.
    let _ = io.out.flush();
    ExitCode::from(status)
}
