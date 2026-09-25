//! `sum` — checksum and count the blocks in a file.
//!
//! ```text
//! Usage: sum [OPTION]... [FILE]...
//! ```
//!
//! A port of GNU coreutils 9.4's `sum`, which upstream builds from two files:
//! the two algorithms in `src/sum.c`, and the `HASH_ALGO_SUM` half of
//! `src/digest.c`, the driver it shares with `md5sum` and the rest. The shared
//! half here is [`coreutils::digest::feed_file`], which opens and reads an
//! operand exactly as the hash programs do.
//!
//! Neither algorithm is a cryptographic hash -- one is a 16-bit rotate-and-add,
//! the other a byte sum folded to 16 bits -- which is why `sum` is here while
//! `cksum`, whose `-a` offers SHA-2, BLAKE2 and SM3, waits on
//! `design-decisions.md` §539.
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

/// `bsd_sum_stream`'s checksum: rotate right one bit within 16, then add.
#[derive(Default)]
struct Bsd {
    checksum: u16,
}

impl Bsd {
    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            // `(checksum >> 1) + ((checksum & 1) << 15)`, then `+= byte` and
            // `&= 0xffff`: a 16-bit rotation and a 16-bit wrapping add.
            self.checksum = self.checksum.rotate_right(1).wrapping_add(u16::from(byte));
        }
    }
}

/// `sysv_sum_stream`'s checksum: every byte summed in an `unsigned int`.
#[derive(Default)]
struct Sysv {
    sum: u32,
}

impl Sysv {
    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.sum = self.sum.wrapping_add(u32::from(byte));
        }
    }

    /// `r = (s & 0xffff) + (s >> 16); checksum = (r & 0xffff) + (r >> 16)`.
    ///
    /// Neither sum can overflow: `r` is at most `0x1fffe`, and the checksum at
    /// most `0xffff`.
    fn finish(&self) -> u32 {
        let r = (self.sum & 0xffff).wrapping_add(self.sum >> 16);
        (r & 0xffff).wrapping_add(r >> 16)
    }
}

/// One output line, `output_bsd` or `output_sysv`.
///
/// `human_readable (length, …, human_ceiling, 1, BLOCK)` is the size in whole
/// blocks, rounded up, and prints as a plain integer.
fn render(algorithm: Algorithm, checksum: u32, length: u64, name: Option<&[u8]>) -> Vec<u8> {
    let mut line = match algorithm {
        Algorithm::Bsd => format!("{checksum:05} {:>5}", length.div_ceil(1024)),
        Algorithm::Sysv => format!("{checksum} {}", length.div_ceil(512)),
    }
    .into_bytes();
    if let Some(name) = name {
        line.push(b' ');
        line.extend_from_slice(name);
    }
    line.push(b'\n');
    line
}

/// One operand: its checksum and its length, or `None` once `feed_file` has
/// reported why not.
fn sum_file(algorithm: Algorithm, name: &[u8], read_stdin: &mut bool) -> Option<(u32, u64)> {
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
                Algorithm::Bsd => u32::from(bsd.checksum),
                Algorithm::Sysv => sysv.finish(),
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
                // Deliberately unread, as above.
                let _ = out.write_all(&render(algorithm, checksum, length, shown));
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

    fn bsd(data: &[u8]) -> u16 {
        let mut b = Bsd::default();
        b.update(data);
        b.checksum
    }

    fn sysv(data: &[u8]) -> u32 {
        let mut s = Sysv::default();
        s.update(data);
        s.finish()
    }

    /// Values measured from GNU 9.4's `sum` and `sum -s`.
    #[test]
    fn checksums_match_upstream() {
        assert_eq!(bsd(b"hello\n"), 36979);
        assert_eq!(sysv(b"hello\n"), 542);
        assert_eq!(bsd(b""), 0);
        assert_eq!(sysv(b""), 0);
    }

    /// Feeding in pieces is feeding in one: the rotation carries across.
    #[test]
    fn chunking_does_not_change_the_answer() {
        let data: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
        let mut b = Bsd::default();
        let mut s = Sysv::default();
        for piece in data.chunks(333) {
            b.update(piece);
            s.update(piece);
        }
        assert_eq!(b.checksum, bsd(&data));
        assert_eq!(s.finish(), sysv(&data));
    }

    /// The fold: a sum past 16 bits comes back into range twice.
    #[test]
    fn the_sysv_fold_handles_carries() {
        let s = Sysv { sum: 0xffff_ffff };
        assert_eq!(s.finish(), 0xffff);
        let s = Sysv { sum: 0x0001_ffff };
        assert_eq!(s.finish(), 1);
    }

    #[test]
    fn lines_are_upstreams_two_formats() {
        assert_eq!(
            render(Algorithm::Bsd, 36979, 6, Some(b"s1")),
            b"36979     1 s1\n"
        );
        assert_eq!(render(Algorithm::Bsd, 7, 70_000, None), b"00007    69\n");
        assert_eq!(render(Algorithm::Sysv, 542, 6, Some(b"-")), b"542 1 -\n");
        assert_eq!(render(Algorithm::Sysv, 0, 0, None), b"0 0\n");
        // A name is data here, byte for byte.
        assert_eq!(
            render(Algorithm::Sysv, 1, 1, Some(b"a b\xff")),
            b"1 1 a b\xff\n"
        );
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
