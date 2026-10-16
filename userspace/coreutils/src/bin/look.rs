//! `look` -- display lines beginning with a given string: util-linux
//! 2.39.3's, ported.
//!
//! ```text
//! look [options] <string> [<file>]
//! ```
//!
//! A transcription of `misc-utils/look.c`, which is 4.4BSD's: a binary search
//! of a sorted file for a line at or before the first that begins with
//! `string`, a linear search forward from there, and every matching line
//! printed. What upstream does and this keeps:
//!
//! - **The file.** `$WORDLIST` if it is set and readable, else
//!   `/usr/share/dict/words`; `-a` the alternative `/usr/share/dict/web2`; and
//!   a second operand over all of them.
//! - **`-d` and `-f` by default only without a file.** With one operand,
//!   both are on; with two, only what was asked for.
//! - **`-d`** compares only blanks and alphanumerics -- in the string, once,
//!   and in each line as it is read -- and **`-f`** ignores case, in the C
//!   locale's sense (ASCII), as `strncasecmp` does there.
//! - **`-t c`** cuts the string *after* the first `c`, so `c` itself is
//!   still compared.
//! - **The search's rules, unsorted input and all.** A line matches when its
//!   first `strlen (string)` compared characters equal the string; a shorter
//!   line compares as lesser. The binary search lands where upstream's lands
//!   on a file that is not sorted, so what such a file yields is upstream's.
//! - **The file is mapped, not read.** An empty file is `Invalid argument`
//!   and a directory `No such device`, as `mmap` answers; status 1 for
//!   those, for a file that cannot be opened, and for no match.
//! - **Standard output is util-linux's** (`closestream.h`): a write that
//!   fails mid-way is `look: stdout: REASON`, then `write error`, status 1.
//!
//! # Deliberate differences
//!
//! - `--version` names SlateOS coreutils, as every program here does.

use std::ffi::OsString;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes};
use coreutils::stdfd;

coreutils::guard_std_fds!();

const LOOK: Program = Program::new("look", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "adft:Vh";

/// Upstream's `longopts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("alternative", Takes::Nothing),
    ("alphanum", Takes::Nothing),
    ("ignore-case", Takes::Nothing),
    ("terminate", Takes::Required),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
];

/// `_PATH_WORDS`.
const PATH_WORDS: &[u8] = b"/usr/share/dict/words";
/// `_PATH_WORDS_ALT`.
const PATH_WORDS_ALT: &[u8] = b"/usr/share/dict/web2";

/// `EINVAL`: what `mmap` says to a zero-length mapping.
const EINVAL: i32 = 22;
/// `ENODEV`: what `mmap` says to a file it cannot map, a directory among
/// them.
const ENODEV: i32 = 19;

const VERSION: &str = "look from SlateOS coreutils 0.1.0\n";

/// How a string and a line compare: `compare`'s `LESS`, `GREATER` and
/// `EQUAL`, from the string's side.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Order {
    /// The line sorts after the string: no match can follow.
    Less,
    /// The line sorts before the string: keep going.
    Greater,
    /// The line begins with the string.
    Equal,
}

/// The search: upstream's file-scope `dflag`, `fflag`, `string` and
/// `stringlen`.
struct Look {
    dflag: bool,
    fflag: bool,
    /// The string, reformatted for `-d` once, up front.
    string: Vec<u8>,
}

/// `isalnum` in the C locale.
fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

/// `isblank` in the C locale: space and tab.
fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// `strncmp` or `strncasecmp` of two C strings, `n` bytes at most: the
/// sign of the first difference, a string's end comparing below any byte.
fn strncmp(a: &[u8], b: &[u8], n: usize, fold: bool) -> std::cmp::Ordering {
    let byte = |s: &[u8], i: usize| {
        let c = s.get(i).copied().unwrap_or(0);
        if fold { c.to_ascii_lowercase() } else { c }
    };
    for i in 0..n {
        let (x, y) = (byte(a, i), byte(b, i));
        if x != y {
            return x.cmp(&y);
        }
        if x == 0 {
            break;
        }
    }
    std::cmp::Ordering::Equal
}

/// `SKIP_PAST_NEWLINE`: the index after the next newline, or `back`.
fn skip_past_newline(buf: &[u8], mut p: usize, back: usize) -> usize {
    while p < back {
        let c = buf.get(p).copied();
        p = p.saturating_add(1);
        if c == Some(b'\n') {
            break;
        }
    }
    p
}

impl Look {
    /// `look`'s reformatting: under `-d`, only the string's blanks and
    /// alphanumerics take part.
    fn new(string: &[u8], dflag: bool, fflag: bool) -> Self {
        let string = if dflag {
            string
                .iter()
                .copied()
                .filter(|&c| is_alnum(c) || is_blank(c))
                .collect()
        } else {
            string.to_vec()
        };
        Look {
            dflag,
            fflag,
            string,
        }
    }

    /// `compare (s2, s2end)`: the line at `s2` -- up to its newline or
    /// `s2end` -- against the string.
    fn compare(&self, buf: &[u8], s2: usize, s2end: usize) -> Order {
        let stringlen = self.string.len();
        let mut comparbuf = Vec::with_capacity(stringlen);
        let mut i = stringlen;
        let mut p = s2;
        while p < s2end && i > 0 {
            let Some(&c) = buf.get(p) else {
                break;
            };
            if c == b'\n' {
                break;
            }
            if !self.dflag || is_alnum(c) || is_blank(c) {
                comparbuf.push(c);
                i = i.saturating_sub(1);
            }
            p = p.saturating_add(1);
        }
        // The copy is a C string: a NUL byte in the line ends it there.
        if let Some(nul) = comparbuf.iter().position(|&c| c == 0) {
            comparbuf.truncate(nul);
        }
        match strncmp(&comparbuf, &self.string, stringlen, self.fflag) {
            std::cmp::Ordering::Greater => Order::Less,
            std::cmp::Ordering::Less => Order::Greater,
            std::cmp::Ordering::Equal => Order::Equal,
        }
    }

    /// `binary_search`: the start of a line at or before the first match.
    fn binary_search(&self, buf: &[u8], mut front: usize, mut back: usize) -> usize {
        let mid = |f: usize, b: usize| f.saturating_add(b.saturating_sub(f) / 2);
        let mut p = skip_past_newline(buf, mid(front, back), back);
        while p < back && back > front {
            if self.compare(buf, p, back) == Order::Greater {
                front = p;
            } else {
                back = p;
            }
            p = skip_past_newline(buf, mid(front, back), back);
        }
        front
    }

    /// `linear_search`: the first matching line from `front`, or `None`.
    fn linear_search(&self, buf: &[u8], mut front: usize, back: usize) -> Option<usize> {
        while front < back {
            match self.compare(buf, front, back) {
                Order::Equal => return Some(front),
                Order::Less => return None,
                Order::Greater => {}
            }
            front = skip_past_newline(buf, front, back);
        }
        None
    }
}

/// util-linux's `warn`/`err`: `look: MESSAGE: REASON`.
fn warn(message: &[u8], e: &std::io::Error) {
    let mut m = b"look: ".to_vec();
    m.extend_from_slice(message);
    m.extend_from_slice(b": ");
    m.extend_from_slice(coreutils::errmsg::strerror(e).as_bytes());
    m.push(b'\n');
    ulclosestream::stderr_write(&m);
}

/// `errtryhelp (EXIT_FAILURE)`'s referral.
fn try_help() {
    ulclosestream::stderr_write(b"Try 'look --help' for more information.\n");
}

/// `usage ()`: to standard output.
fn usage() -> String {
    let help = |opt: &str, what: &str| format!("{opt:<26}{what}\n");
    let mut s = String::new();
    s.push_str("\nUsage:\n");
    s.push_str(" look [options] <string> [<file>...]\n");
    s.push_str("\nDisplay lines beginning with a specified string.\n");
    s.push_str("\nOptions:\n");
    s.push_str(" -a, --alternative        use the alternative dictionary\n");
    s.push_str(" -d, --alphanum           compare only blanks and alphanumeric characters\n");
    s.push_str(" -f, --ignore-case        ignore case differences when comparing\n");
    s.push_str(" -t, --terminate <char>   define the string-termination character\n");
    s.push('\n');
    s.push_str(&help(" -h, --help", "display this help"));
    s.push_str(&help(" -V, --version", "display version"));
    s.push_str("\nFor more details see look(1).\n");
    s
}

/// The file, as `mmap` would give it to upstream: its bytes, or why not.
fn map_file(path: &[u8]) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(os_from_bytes(path))?;
    let meta = file.metadata()?;
    if meta.len() == 0 {
        return Err(std::io::Error::from_raw_os_error(EINVAL));
    }
    if meta.is_dir() {
        return Err(std::io::Error::from_raw_os_error(ENODEV));
    }
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut &file, &mut data)?;
    // A mapping is `st_size` bytes: a file that grew since is cut there, one
    // that shrank reads short, as the pages past its end would be absent.
    data.truncate(usize::try_from(meta.len()).unwrap_or(usize::MAX));
    Ok(data)
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut out = ulclosestream::Stdout::new(1);
    let status = run(&argv, &mut out);
    ExitCode::from(out.close(status, b"look"))
}

/// Upstream's `main`.
fn run(argv: &[OsString], out: &mut ulclosestream::Stdout) -> u8 {
    let mut file: Vec<u8> = match std::env::var_os("WORDLIST") {
        Some(w) if stdfd::readable(&os_bytes(&w)).is_ok() => os_bytes(&w).into_owned(),
        _ => PATH_WORDS.to_vec(),
    };
    let mut dflag = false;
    let mut fflag = false;
    let mut termchar: u8 = 0;
    let mut operands: Vec<Vec<u8>> = Vec::new();

    for item in LOOK.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let opt = match item {
            Ok(o) => o,
            Err(e) => {
                ulclosestream::stderr_write(format!("look: {}\n", e.sentence).as_bytes());
                try_help();
                return 1;
            }
        };
        match opt {
            Opt::Short(b'a', _) | Opt::Long("alternative", _) => file = PATH_WORDS_ALT.to_vec(),
            Opt::Short(b'd', _) | Opt::Long("alphanum", _) => dflag = true,
            Opt::Short(b'f', _) | Opt::Long("ignore-case", _) => fflag = true,
            Opt::Short(b't', v) | Opt::Long("terminate", v) => {
                termchar = v
                    .as_deref()
                    .map_or(0, |v| os_bytes(v).first().copied().unwrap_or(0));
            }
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                out.write(VERSION.as_bytes());
                return 0;
            }
            Opt::Short(b'h', _) | Opt::Long("help", _) => {
                out.write(usage().as_bytes());
                return 0;
            }
            Opt::Operand(v) => operands.push(os_bytes(v).into_owned()),
            Opt::Short(..) | Opt::Long(..) => {
                try_help();
                return 1;
            }
        }
    }

    let mut string = match operands.as_slice() {
        // Two operands: the user's -d and -f only.
        [s, f] => {
            file.clone_from(f);
            s.clone()
        }
        // One: -df by default.
        [s] => {
            dflag = true;
            fflag = true;
            s.clone()
        }
        _ => {
            ulclosestream::warnx(b"look", "bad usage");
            try_help();
            return 1;
        }
    };

    // `-t c`: the string ends just after its first `c`.
    if termchar != 0 {
        if let Some(at) = string.iter().position(|&c| c == termchar) {
            string.truncate(at.saturating_add(1));
        }
    }

    let buf = match map_file(&file) {
        Ok(b) => b,
        Err(e) => {
            warn(&file, &e);
            return 1;
        }
    };

    let look = Look::new(&string, dflag, fflag);
    let back = buf.len();
    let front = look.binary_search(&buf, 0, back);
    let Some(mut front) = look.linear_search(&buf, front, back) else {
        return 1;
    };

    // `print_from`: every matching line, each `putchar` checked.
    while front < back && look.compare(&buf, front, back) == Order::Equal {
        let end = skip_past_newline(&buf, front, back);
        for &c in buf.get(front..end).unwrap_or_default() {
            out.write(&[c]);
            if let Some(e) = out.error() {
                let e = std::io::Error::new(e.kind(), e.to_string());
                warn(b"stdout", &e);
                return 1;
            }
        }
        front = end;
    }
    0
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn found(look: &Look, text: &[u8]) -> Option<Vec<u8>> {
        let back = text.len();
        let front = look.binary_search(text, 0, back);
        let mut front = look.linear_search(text, front, back)?;
        let mut out = Vec::new();
        while front < back && look.compare(text, front, back) == Order::Equal {
            let end = skip_past_newline(text, front, back);
            out.extend_from_slice(&text[front..end]);
            front = end;
        }
        Some(out)
    }

    #[test]
    fn a_prefix_finds_every_line_it_begins() {
        let words = b"apple\napply\napt\nbanana\nband\n";
        let l = Look::new(b"app", false, false);
        assert_eq!(found(&l, words), Some(b"apple\napply\n".to_vec()));
        let l = Look::new(b"ban", false, false);
        assert_eq!(found(&l, words), Some(b"banana\nband\n".to_vec()));
        let l = Look::new(b"c", false, false);
        assert_eq!(found(&l, words), None);
        // An empty string begins every line.
        let l = Look::new(b"", false, false);
        assert_eq!(found(&l, words), Some(words.to_vec()));
    }

    #[test]
    fn case_and_punctuation_are_folded_only_when_asked() {
        // Sorted as bytes: what a case-sensitive search needs.
        let bytewise = b"Apple\nApt\napple's\n";
        let l = Look::new(b"apple", false, false);
        assert_eq!(found(&l, bytewise), Some(b"apple's\n".to_vec()));
        // Sorted as a dictionary: what -d and -f need.
        let dictionary = b"Apple\napple's\nApt\n";
        let l = Look::new(b"apples", true, true);
        assert_eq!(found(&l, dictionary), Some(b"apple's\n".to_vec()));
        let l = Look::new(b"APP", false, true);
        assert_eq!(found(&l, dictionary), Some(b"Apple\napple's\n".to_vec()));
        // The wrong order for the search finds nothing -- upstream's binary
        // search on that file stops past the line.
        let l = Look::new(b"apple", false, false);
        assert_eq!(found(&l, dictionary), None);
    }

    #[test]
    fn the_last_line_needs_no_newline() {
        let l = Look::new(b"ze", false, false);
        assert_eq!(found(&l, b"alpha\nzeta"), Some(b"zeta".to_vec()));
    }

    #[test]
    fn strncmp_is_cs_with_a_strings_end_below_every_byte() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        assert_eq!(strncmp(b"ab", b"abc", 3, false), Less);
        assert_eq!(strncmp(b"abd", b"abc", 3, false), Greater);
        assert_eq!(strncmp(b"ABC", b"abc", 3, true), Equal);
        assert_eq!(strncmp(b"ABC", b"abc", 3, false), Less);
        assert_eq!(strncmp(b"abc", b"abd", 2, false), Equal);
        assert_eq!(strncmp(b"\xe9", b"z", 1, false), Greater);
    }
}
