//! `sum` — checksum and count the blocks in a file.
//!
//! ```text
//! Usage: sum [OPTION]... [FILE]...
//! ```
//!
//! A port of GNU coreutils 9.4's `sum`, which upstream builds from two files:
//! the two algorithms in `src/sum.c`, and the `HASH_ALGO_SUM` half of
//! `src/digest.c`, the driver it shares with `md5sum` and the rest. Here the
//! algorithms and their output lines are [`coreutils::sum`] — which `cksum -a
//! bsd` and `-a sysv` print through too, as upstream links `sum.c` into both —
//! and the reading is [`coreutils::digest::feed_file`], which opens and reads
//! an operand exactly as the hash programs do.
//!
//! # The two algorithms
//!
//! * **BSD** (`-r`, the default): for each byte, rotate the 16-bit checksum
//!   right by one bit and add the byte. Printed `%05d %5s`, the size in
//!   1024-byte blocks rounded up.
//! * **System V** (`-s`, `--sysv`): the sum of every byte modulo 2^32, folded
//!   twice to 16 bits. Printed `%d %s`, in 512-byte blocks rounded up.
//!
//! Whichever of `-r` and `-s` comes last wins.
//!
//! # When the name is printed
//!
//! Whenever the command line named a file at all -- upstream's `optind !=
//! argc` -- so `sum f` prints `f`, `sum -` prints `-`, and only a bare `sum`
//! reading standard input prints no name. The name goes to standard output as
//! its bytes, as upstream's `printf (" %s", file)` does: this is data, not a
//! diagnostic, and quoting it would be a different file name.
//!
//! # Checked against GNU
//!
//! `scripts/sum-diff.sh`.

use coreutils::digest::{Fed, feed_file};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use coreutils::sum::{Bsd, Sysv, output_bsd, output_sysv};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

coreutils::guard_std_fds!();

const SUM: Program = Program::new("sum", 1);

/// Upstream's `long_options[]` for `HASH_ALGO_SUM`: `--sysv` and the two
/// standard ones. (`-r` has no long spelling.)
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("sysv", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Algorithm {
    /// `-r`, the default.
    Bsd,
    /// `-s`, `--sysv`.
    Sysv,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run {
        algorithm: Algorithm,
        files: Vec<OsString>,
    },
}

fn help_text() -> String {
    "\
Usage: sum [OPTION]... [FILE]...
Print or check BSD (16-bit) checksums.

With no FILE, or when FILE is -, read standard input.

  -r              use BSD sum algorithm (the default), use 1K blocks
  -s, --sysv      use System V sum algorithm, use 512 bytes blocks
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

/// Upstream's `getopt_long` loop with `short_opts = "rs"`.
///
/// # Errors
///
/// An unknown option.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut algorithm = Algorithm::Bsd;
    let mut files: Vec<OsString> = Vec::new();
    for item in SUM.parse(args, "rs", LONG_OPTIONS) {
        match item? {
            Opt::Short(b'r', _) => algorithm = Algorithm::Bsd,
            Opt::Short(b's', _) | Opt::Long("sysv", _) => algorithm = Algorithm::Sysv,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(x) => files.push(x.clone()),
            // Unreachable: every name in the table is handled above.
            Opt::Long(other, _) => {
                return Err(SUM.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(SUM.invalid_option(c)),
        }
    }
    Ok(Request::Run { algorithm, files })
}

/// One operand: its checksum and its length, or `None` once `feed_file` has
/// reported why not.
fn sum_file(algorithm: Algorithm, name: &[u8], read_stdin: &mut bool) -> Option<(u16, u64)> {
    let mut bsd = Bsd::default();
    let mut sysv = Sysv::default();
    // `uintmax_t total_bytes`, with upstream's `EOVERFLOW` check. A 64-bit
    // count cannot overflow on any input that exists (it would take 16 EiB),
    // so the check is spent on nothing and the addition saturates instead.
    let mut length: u64 = 0;
    let fed = feed_file("sum", name, false, read_stdin, &mut |data| {
        match algorithm {
            Algorithm::Bsd => bsd.update(data),
            Algorithm::Sysv => sysv.update(data),
        }
        length = length.saturating_add(u64::try_from(data.len()).unwrap_or(u64::MAX));
    });
    match fed {
        Fed::Ok => Some((
            match algorithm {
                Algorithm::Bsd => bsd.checksum(),
                Algorithm::Sysv => sysv.checksum(),
            },
            length,
        )),
        // `Missing` needs `ignore_missing`, which `sum` never passes.
        Fed::Missing | Fed::Failed => None,
    }
}

fn run() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            SUM.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    let mut out = Stream::stdout_line_buffered();
    let (algorithm, files) = match request {
        Request::Help => {
            // Deliberately unread: `Stream` records a failed write, and
            // `close_stdout` reports it.
            let _ = out.write_all(help_text().as_bytes());
            return stdfd::close_stdout("sum", out, ExitCode::SUCCESS);
        }
        Request::Version => {
            // Deliberately unread, as above.
            let _ = out.write_all(b"sum (SlateOS coreutils) 0.1.0\n");
            return stdfd::close_stdout("sum", out, ExitCode::SUCCESS);
        }
        Request::Run { algorithm, files } => (algorithm, files),
    };

    // `DIGEST_OUT (…, optind != argc, …)`: a name is printed whenever any
    // operand was given, `-` included.
    let named = !files.is_empty();
    let operands: Vec<OsString> = if named {
        files
    } else {
        vec![OsString::from("-")]
    };
    let mut ok = true;
    let mut read_stdin = false;
    for operand in &operands {
        let name = os_bytes(operand);
        match sum_file(algorithm, &name, &mut read_stdin) {
            Some((checksum, length)) => {
                let shown = named.then_some(&*name);
                let line = match algorithm {
                    Algorithm::Bsd => output_bsd(checksum, length, shown, false, b'\n'),
                    Algorithm::Sysv => output_sysv(checksum, length, shown, false, b'\n'),
                };
                // Deliberately unread, as above.
                let _ = out.write_all(&line);
            }
            None => ok = false,
        }
    }
    // `if (have_read_stdin && fclose (stdin) == EOF)`: a read error on stdin
    // has already been reported by `feed_file`, and a locked Rust stdin has no
    // separate close to fail.
    let _ = read_stdin;
    stdfd::close_stdout(
        "sum",
        out,
        if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        },
    )
}

fn main() -> ExitCode {
    stdfd::close_stderr(run(), 1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_last_of_r_and_s_wins() {
        let run = |list: &[&str]| match parse_args(&args(list)).unwrap() {
            Request::Run { algorithm, .. } => algorithm,
            other => panic!("{other:?}"),
        };
        assert_eq!(run(&[]), Algorithm::Bsd);
        assert_eq!(run(&["-s"]), Algorithm::Sysv);
        assert_eq!(run(&["-r", "-s"]), Algorithm::Sysv);
        assert_eq!(run(&["-s", "-r"]), Algorithm::Bsd);
        assert_eq!(run(&["--sysv", "f"]), Algorithm::Sysv);
    }

    #[test]
    fn help_version_and_refusals() {
        assert_eq!(parse_args(&args(&["--help", "-x"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
        let e = parse_args(&args(&["-x"])).unwrap_err();
        assert_eq!(e.status, 1);
    }
}
