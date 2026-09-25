//! `sync` — synchronize cached writes to persistent storage.
//!
//! ```text
//! Usage: sync [OPTION] [FILE]...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/sync.c`. It replaces a `sync`
//! personality of `userspace/getopt` that no link ever reached, and that
//! differed from GNU where scripts would notice:
//!
//! * **`sync -f` with no operand is a plain `sync`, not an error.** Upstream
//!   rejects only `--data` without a file; `-f` falls through to the
//!   whole-system `sync()`. The old code refused both.
//! * **Every file is attempted.** Upstream reports a file it cannot open or
//!   sync and carries on to the next (`ok &= sync_arg (…)`); the old code
//!   stopped at the first.
//! * **A file that will not open for reading is tried for writing** -- a
//!   write-only file can still be synced -- and the error reported when both
//!   fail is the *read* one, which is the one that says something about a
//!   directory.
//!
//! # What each mode calls
//!
//! | options | call |
//! |---|---|
//! | no operand (with or without `-f`) | `sync()` |
//! | operands | `fsync` on each |
//! | `-d` operands | `fdatasync` on each |
//! | `-f` operands | `syncfs` on each (the file system holding it) |
//!
//! Files are opened `O_NONBLOCK` so that a FIFO operand cannot hang the
//! command, and the flag is cleared again before the sync -- upstream's
//! `couldn't reset non-blocking mode` if that fails. The close is explicit and
//! checked, because a failed close is upstream's `failed to close` and std's
//! `File` drop would swallow it.
//!
//! # Checked against GNU
//!
//! `scripts/sync-diff.sh`.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const SYNC: Program = Program::new("sync", 1);

/// Upstream's `getopt_long` string, exactly.
const SHORT_OPTIONS: &str = "df";

/// Upstream's `long_options[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("data", Takes::Nothing),
    ("file-system", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// upstream's `enum sync_mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    File,
    Data,
    FileSystem,
    Sync,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run { mode: Mode, files: Vec<OsString> },
}

/// A refusal that upstream makes with `error (EXIT_FAILURE, …)`: one line, no
/// referral to `--help`.
#[derive(Debug, PartialEq, Eq)]
struct Refusal(&'static str);

fn help_text() -> String {
    "\
Usage: sync [OPTION] [FILE]...
Synchronize cached writes to persistent storage

If one or more files are specified, sync only them,
or their containing file systems.

  -d, --data             sync only file data, no unneeded metadata
  -f, --file-system      sync the file systems that contain the files
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// Parse the command line and decide the mode, as upstream's `main` does.
///
/// # Errors
///
/// getopt's own diagnostics as `Err(Ok(e))`; the two refusals upstream makes
/// after the scan -- both `--data` and `--file-system`, or `--data` with
/// nothing to sync -- as `Err(Err(refusal))`.
fn parse_args(args: &[OsString]) -> Result<Request, Result<getopt::Error, Refusal>> {
    let mut data = false;
    let mut file_system = false;
    let mut files = Vec::new();
    for item in SYNC.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item.map_err(Ok)? {
            Opt::Short(b'd', _) | Opt::Long("data", _) => data = true,
            Opt::Short(b'f', _) | Opt::Long("file-system", _) => file_system = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(f) => files.push(f.clone()),
            // Unreachable: every table entry is handled above.
            Opt::Long(other, _) => {
                return Err(Ok(
                    SYNC.usage_referring(format!("option '--{other}' is unhandled"))
                ));
            }
            Opt::Short(c, _) => return Err(Ok(SYNC.invalid_option(c))),
        }
    }
    if data && file_system {
        return Err(Err(Refusal("cannot specify both --data and --file-system")));
    }
    if files.is_empty() && data {
        return Err(Err(Refusal("--data needs at least one argument")));
    }
    // `HAVE_SYNCFS` is true on every target this runs on, so the middle arm
    // of upstream's condition (`arg_file_system && ! HAVE_SYNCFS`) is gone.
    let mode = if files.is_empty() {
        Mode::Sync
    } else if file_system {
        Mode::FileSystem
    } else if data {
        Mode::Data
    } else {
        Mode::File
    };
    Ok(Request::Run { mode, files })
}

#[cfg(unix)]
mod imp {
    use super::{Mode, Request, help_text, parse_args};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::quoteaf_os;
    use coreutils::stdfd::{self, Stream};
    use std::ffi::{OsStr, OsString};
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::os::fd::{AsRawFd, IntoRawFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::process::ExitCode;

    /// `O_NONBLOCK`, which is the same number on Linux and here.
    const O_NONBLOCK: i32 = 0o4000;
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;

    unsafe extern "C" {
        fn sync();
        fn syncfs(fd: i32) -> i32;
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn close(fd: i32) -> i32;
    }

    /// The error that last failed, as `errno`.
    fn last() -> io::Error {
        io::Error::last_os_error()
    }

    /// `open (file, O_RDONLY | O_NONBLOCK)`, falling back to write-only, as
    /// upstream does -- and on double failure, the READ attempt's error.
    fn open_for_sync(file: &OsStr) -> Result<File, io::Error> {
        let read = OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(file);
        match read {
            Ok(f) => Ok(f),
            Err(read_err) => OpenOptions::new()
                .write(true)
                .custom_flags(O_NONBLOCK)
                .open(file)
                .map_err(|_| read_err),
        }
    }

    /// Upstream's `sync_arg`: every step reported, and the close checked.
    fn sync_arg(mode: Mode, file: &OsStr) -> bool {
        let f = match open_for_sync(file) {
            Ok(f) => f,
            Err(e) => {
                diag!("sync: error opening {}: {}", quoteaf_os(file), strerror(&e));
                return false;
            }
        };
        let fd = f.as_raw_fd();
        let mut ok = true;

        // SAFETY: `fd` is open for the life of `f`; both calls take and return
        // scalars. The variable argument is an `i64` so a callee reading 64
        // bits reads a defined value.
        let flags = unsafe { fcntl(fd, F_GETFL, 0i64) };
        let reset = flags != -1
            // SAFETY: as above.
            && unsafe { fcntl(fd, F_SETFL, i64::from(flags & !O_NONBLOCK)) } >= 0;
        if !reset {
            diag!(
                "sync: couldn't reset non-blocking mode {}: {}",
                quoteaf_os(file),
                strerror(&last())
            );
            ok = false;
        }

        if ok {
            let synced = match mode {
                Mode::Data => f.sync_data(),
                Mode::File => f.sync_all(),
                Mode::FileSystem => {
                    // SAFETY: `fd` is open for the life of `f`.
                    if unsafe { syncfs(fd) } == 0 {
                        Ok(())
                    } else {
                        Err(last())
                    }
                }
                // Unreachable: `main` handles the whole-system mode itself.
                Mode::Sync => Ok(()),
            };
            if let Err(e) = synced {
                diag!("sync: error syncing {}: {}", quoteaf_os(file), strerror(&e));
                ok = false;
            }
        }

        // SAFETY: `into_raw_fd` gives up `f`'s ownership, so this is the one
        // close of the descriptor.
        if unsafe { close(f.into_raw_fd()) } < 0 {
            diag!(
                "sync: failed to close {}: {}",
                quoteaf_os(file),
                strerror(&last())
            );
            ok = false;
        }
        ok
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(Ok(e)) => {
                super::SYNC.report(&e);
                return ExitCode::FAILURE;
            }
            Err(Err(refusal)) => {
                diag!("sync: {}", refusal.0);
                return ExitCode::FAILURE;
            }
        };
        let mut out = Stream::stdout();
        let earned = match request {
            Request::Help => {
                let _ = out.write_all(help_text().as_bytes());
                ExitCode::SUCCESS
            }
            Request::Version => {
                let _ = out.write_all(b"sync (SlateOS coreutils) 0.1.0\n");
                ExitCode::SUCCESS
            }
            Request::Run {
                mode: Mode::Sync, ..
            } => {
                // SAFETY: no arguments, no result, and it cannot fail.
                unsafe { sync() };
                ExitCode::SUCCESS
            }
            Request::Run { mode, files } => {
                let mut ok = true;
                for f in &files {
                    ok &= sync_arg(mode, f);
                }
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
        };
        stdfd::close_stdout("sync", out, earned)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has neither `syncfs` nor a meaning for `O_NONBLOCK` on a file.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("sync: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn mode_of(args: &[&str]) -> Mode {
        match parse_args(&argv(args)).unwrap() {
            Request::Run { mode, .. } => mode,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_mode_table() {
        assert_eq!(mode_of(&[]), Mode::Sync);
        assert_eq!(mode_of(&["-f"]), Mode::Sync, "-f alone is a plain sync");
        assert_eq!(mode_of(&["a"]), Mode::File);
        assert_eq!(mode_of(&["-d", "a"]), Mode::Data);
        assert_eq!(mode_of(&["--file-system", "a", "b"]), Mode::FileSystem);
    }

    #[test]
    fn the_two_refusals_carry_no_referral() {
        assert_eq!(
            parse_args(&argv(&["-d", "-f", "a"]))
                .unwrap_err()
                .unwrap_err(),
            Refusal("cannot specify both --data and --file-system")
        );
        assert_eq!(
            parse_args(&argv(&["-d"])).unwrap_err().unwrap_err(),
            Refusal("--data needs at least one argument")
        );
        // Both checks look at the whole line, so the order of the flags is
        // not the order of the checks: both-flags wins even without a file.
        assert_eq!(
            parse_args(&argv(&["-df"])).unwrap_err().unwrap_err(),
            Refusal("cannot specify both --data and --file-system")
        );
    }

    #[test]
    fn options_permute_past_operands() {
        assert_eq!(mode_of(&["a", "-d"]), Mode::Data);
    }

    #[test]
    fn getopt_errors_refer_to_help() {
        let e = parse_args(&argv(&["-x"])).unwrap_err().unwrap();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'sync --help' for more information."
        );
    }
}
