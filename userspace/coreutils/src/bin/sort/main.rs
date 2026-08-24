//! sort — sort, merge or check lines of text.
//!
//! ```text
//! sort [-bcCdfghimMnrsuVz] [-k KEYDEF]... [-t SEP] [-o FILE] [FILE...]
//! ```
//!
//! | | |
//! |---|---|
//! | `-b` | leading blanks are not part of a key |
//! | `-c` / `-C` | check that the input is sorted; report / stay quiet |
//! | `-d` `-f` `-i` | compare only alphanumerics+blanks / fold case / drop unprintables |
//! | `-g` `-h` `-M` `-n` `-V` | numeric through a double / SI suffixes / month names / exact decimal / version numbers |
//! | `-k SPEC` | sort on this part of the line; may be repeated |
//! | `-m` | merge already-sorted inputs |
//! | `-o FILE` | write here, which may be an input file |
//! | `-r` | reverse |
//! | `-s` | stable: do not break ties with the whole line |
//! | `-t SEP` | field separator |
//! | `-u` | output only the first of each run of equal keys |
//! | `-z` | lines end with NUL, not newline |
//!
//! Every option also has a long form, which may be abbreviated to any
//! unambiguous prefix (`--rev`) and takes its value either way round
//! (`--key=2` or `--key 2`), exactly as `getopt_long` does. `--sort=WORD` is
//! the long spelling of the ordering options, and `--files0-from=F` takes the
//! input names from `F` as a NUL-separated list — NUL being the one byte a path
//! cannot contain, which is what makes `find -print0 | sort --files0-from=-`
//! safe for names holding spaces or newlines.
//!
//! Exit status: 0, 1 if `-c`/`-C` found the input out of order, 2 for any
//! other failure — except a bad argument *to* an option, which is 1. See
//! `Fatal`.
//!
//! ## What this used to be
//!
//! Until this rewrite `sort` accepted `-r`, `-n` and `-u` and nothing else —
//! no `-k`, so no way to sort on a column at all, which is most of what `sort`
//! is used for. The three it did have were each wrong in a way that produced a
//! plausible answer rather than an error:
//!
//! - It read lines with `BufRead::lines()`, so a file with a non-UTF-8 byte in
//!   it stopped mid-way with a diagnostic. Paths on this system may hold any
//!   byte but `/` and NUL, so `find | sort` was one odd filename away from
//!   failing.
//! - `-n` went through an `f64`, which ties every pair of 20-digit
//!   identifiers that agree in their first 17 digits.
//! - `-u` deduplicated whole lines rather than keys, so `sort -nu` on `1` and
//!   `1.0` printed both. GNU prints one: `-u` means unique *by the key*, and
//!   the two are the same number.
//!
//! ## The two rules that are easy to get wrong
//!
//! **The last-resort comparison.** When every key ties, `sort` falls back to
//! comparing the whole line bytewise — which is what makes the order total and
//! the output reproducible. That fallback is *skipped* when `-u` or `-s` is
//! given, and skipping it is the entire meaning of both flags: `-s` keeps the
//! input order for tied lines, and `-u` treats them as duplicates.
//!
//! **The global ordering options are defaults, not overrides.** `-n` with no
//! `-k` makes one whole-line numeric key; `-n` with a `-k2,2` that names no
//! ordering of its own makes *that* key numeric. But a `-k2,2n` names its own,
//! so it inherits nothing else — including `-r`, which is why `sort -r -k2,2n`
//! does not reverse. That surprises people, and it is GNU's behaviour.
//!
//! ## C locale only
//!
//! Bytes compare as bytes and the month names are English. SlateOS has no
//! collation tables yet; see `known-issues.md`. `scripts/sort-diff.sh` pins
//! `LC_ALL=C` so the comparison against GNU is against the same ordering.

mod keydef;
mod order;

use coreutils::diag;
use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{quote, quoteaf_os, quotef_os};
use keydef::{Blanks, KeySpec, Kind, parse_key, parse_obsolete_end, parse_obsolete_start};
use order::Ignore;

const USAGE: &str = "\
usage: sort [OPTION]... [FILE]...
Write the sorted concatenation of each FILE to standard output.
With no FILE, or when FILE is -, read standard input.

Ordering:
  -b, --ignore-leading-blanks   a key does not start with its leading blanks
  -d, --dictionary-order        compare only blanks and alphanumerics
  -f, --ignore-case             fold lower case to upper
  -g, --general-numeric-sort    compare as a general number (accepts 1e3, 0x10)
  -h, --human-numeric-sort      compare by SI suffix, then by number (2K < 1M)
  -i, --ignore-nonprinting      compare only printable characters
  -M, --month-sort              compare as a month name (unknown < JAN < DEC)
  -n, --numeric-sort            compare as a decimal number, exactly
  -V, --version-sort            compare as a file name holding version numbers
  -r, --reverse                 reverse the result
      --sort=WORD               one of general-numeric, human-numeric, month,
                                numeric, version

Which part of the line:
  -k, --key=KEYDEF              sort on this key; KEYDEF is F[.C][OPTS][,F[.C][OPTS]]
  -t, --field-separator=SEP     use SEP rather than a blank-to-nonblank transition

Other:
  -c, --check[=diagnose-first]  check that the input is sorted; report the first
                                line that is not
  -C, --check=quiet             as -c, but say nothing; the status is the answer
  -m, --merge                   merge already-sorted files; do not sort
  -o, --output=FILE             write here, which may also be an input
  -s, --stable                  do not break ties with the whole line
  -u, --unique                  output only the first of each run of equal keys
  -z, --zero-terminated         lines end with NUL, not newline
      --files0-from=F           take the input names from F, NUL-separated
      --help                    print this and exit
      --version                 print the version and exit

Accepted and ignored, because this sort holds the whole input in memory:
  -S, --buffer-size=SIZE        -T, --temporary-directory=DIR
      --parallel=N                  --compress-program=PROG
      --batch-size=N                --random-source=FILE

A long option may be abbreviated to any unambiguous prefix, and takes its value
either as --key=2 or as --key 2.

KEYDEF's F is a field number and C a character within it, both starting at 1;
an omitted .C is the start of the field for a start position and the end of it
for an end position. OPTS is any of bdfgiMnRrV, applying to that key alone --
a key that names any ordering of its own inherits none of the global ones.

Exit status: 0 if all went well, 1 if -c or -C found the input out of order,
2 if something went wrong.";

/// What `-c` or `-C` asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Check {
    /// `-c`: report the first line that is out of order.
    Diagnose,
    /// `-C`: say nothing; the exit status is the answer.
    Quiet,
}

#[cfg_attr(test, derive(Debug))]
struct Config {
    keys: Vec<KeySpec>,
    tab: Option<u8>,
    reverse: bool,
    unique: bool,
    stable: bool,
    merge: bool,
    check: Option<Check>,
    delim: u8,
    output: Option<OsString>,
    files: Vec<OsString>,
    /// `--files0-from=F`: the operands come from `F` as NUL-separated names.
    /// Kept separate from `files` because it is read after the whole command
    /// line is parsed, and because combining it with an operand is an error.
    files0_from: Option<OsString>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            keys: Vec::new(),
            tab: None,
            reverse: false,
            unique: false,
            stable: false,
            merge: false,
            check: None,
            delim: b'\n',
            output: None,
            files: Vec::new(),
            files0_from: None,
        }
    }
}

fn main() {
    let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let cfg = match parse_args(&raw) {
        Ok(Some(c)) => c,
        Ok(None) => return,
        Err(e) => die_with(&e.message(), e.status),
    };

    // Every input is read before anything is written, which is what lets
    // `sort -o f f` work: by the time the output file is truncated its old
    // contents are already in memory.
    let mut contents: Vec<Vec<u8>> = Vec::with_capacity(cfg.files.len());
    for path in &cfg.files {
        match read_file(path) {
            Ok(bytes) => contents.push(bytes),
            // GNU words a failed open one way when checking and another when
            // sorting, and a failed *read* a third way in both. The wording is
            // the only thing that tells a user whether the name was wrong or
            // the thing behind it was, so it is worth reproducing.
            Err(failure) => die(&format!(
                "{}: {}: {}",
                match failure {
                    ReadFailure::Open(_) if cfg.check.is_some() => "open failed",
                    ReadFailure::Open(_) => "cannot read",
                    ReadFailure::Read(_) => "read failed",
                },
                quotef_os(path),
                strerror(failure.cause())
            )),
        }
    }
    let per_file: Vec<Vec<&[u8]>> = contents.iter().map(|c| split_lines(c, cfg.delim)).collect();

    if let Some(mode) = cfg.check {
        process::exit(check(
            &cfg,
            per_file.first().map_or(&[], Vec::as_slice),
            mode,
        ));
    }

    let ordered = if cfg.merge {
        merge(&cfg, &per_file)
    } else {
        let mut lines: Vec<&[u8]> = per_file.iter().flatten().copied().collect();
        // Stable, because `-s` and `-u` both stop the comparison short of the
        // whole-line fallback and then rely on the input order to decide.
        lines.sort_by(|a, b| compare(&cfg, a, b));
        lines
    };

    if let Err(e) = write_out(&cfg, &ordered) {
        // A closed pipe is how `sort | head` ends. It is not a failure worth a
        // diagnostic, but it is not success either.
        if e.kind() == io::ErrorKind::BrokenPipe {
            process::exit(2);
        }
        die(&format!("write failed: {}", strerror(&e)));
    }
}

// ── comparison ──────────────────────────────────────────────────────────────

/// The full comparison: the keys, then the whole line if they all tie.
fn compare(cfg: &Config, a: &[u8], b: &[u8]) -> Ordering {
    if !cfg.keys.is_empty() {
        let mut diff = Ordering::Equal;
        for key in &cfg.keys {
            diff = key.compare(a, b, cfg.tab);
            if diff != Ordering::Equal {
                break;
            }
        }
        // The last resort is what makes the order total. `-u` needs tied keys
        // to stay tied so it can drop them, and `-s` needs them tied so the
        // input order survives, so both skip it.
        if diff != Ordering::Equal || cfg.unique || cfg.stable {
            return diff;
        }
    }
    let diff = a.cmp(b);
    if cfg.reverse { diff.reverse() } else { diff }
}

/// Merge inputs that are each already sorted.
///
/// This is a real k-way merge and not a re-sort, because the two differ on
/// input that is not in fact sorted and GNU's answer there is the merge's.
/// Ties go to the earliest file, which is what makes `-m` stable.
fn merge<'a>(cfg: &Config, per_file: &[Vec<&'a [u8]>]) -> Vec<&'a [u8]> {
    let total: usize = per_file.iter().map(Vec::len).sum();
    let mut out: Vec<&[u8]> = Vec::with_capacity(total);
    let mut at: Vec<usize> = vec![0; per_file.len()];
    loop {
        let mut best: Option<usize> = None;
        for (index, lines) in per_file.iter().enumerate() {
            let Some(line) = lines.get(at.get(index).copied().unwrap_or(0)) else {
                continue;
            };
            let better = match best.and_then(|b| {
                per_file
                    .get(b)
                    .and_then(|f| f.get(at.get(b).copied().unwrap_or(0)))
            }) {
                None => true,
                Some(current) => compare(cfg, line, current) == Ordering::Less,
            };
            if better {
                best = Some(index);
            }
        }
        let Some(index) = best else { break };
        if let Some(line) = per_file
            .get(index)
            .and_then(|f| f.get(at.get(index).copied().unwrap_or(0)))
        {
            out.push(line);
        }
        if let Some(slot) = at.get_mut(index) {
            *slot = slot.saturating_add(1);
        }
    }
    out
}

/// `-c` / `-C`: is the input already in order?
///
/// Returns the exit status. With `-u` an equal pair is also a failure, since
/// the file would not be the output of `sort -u`.
fn check(cfg: &Config, lines: &[&[u8]], mode: Check) -> i32 {
    let name = cfg
        .files
        .first()
        .map_or_else(|| "-".to_string(), |f| f.to_string_lossy().into_owned());
    for index in 1..lines.len() {
        let (Some(prev), Some(cur)) = (lines.get(index.saturating_sub(1)), lines.get(index)) else {
            continue;
        };
        let diff = compare(cfg, prev, cur);
        let disordered = if cfg.unique {
            diff != Ordering::Less
        } else {
            diff == Ordering::Greater
        };
        if disordered {
            if mode == Check::Diagnose {
                let mut err = io::stderr().lock();
                // The offending line goes out as bytes: it is not necessarily
                // text, and a diagnostic is not a reason to mangle it.
                let _ = write!(err, "sort: {name}:{}: disorder: ", index.saturating_add(1));
                let _ = err.write_all(cur);
                let _ = err.write_all(&[cfg.delim]);
            }
            return 1;
        }
    }
    0
}

// ── input and output ────────────────────────────────────────────────────────

/// Which stage of reading a file failed.
///
/// The distinction is not bookkeeping: "no such file" and "is a directory" are
/// both `cannot read` if you collapse them, and the user then cannot tell a
/// misspelled name from a name that is right but not a file.
enum ReadFailure {
    Open(io::Error),
    Read(io::Error),
}

impl ReadFailure {
    fn cause(&self) -> &io::Error {
        match self {
            ReadFailure::Open(e) | ReadFailure::Read(e) => e,
        }
    }
}

fn read_file(path: &OsString) -> Result<Vec<u8>, ReadFailure> {
    let mut bytes = Vec::new();
    if path == "-" {
        io::stdin()
            .lock()
            .read_to_end(&mut bytes)
            .map_err(ReadFailure::Read)?;
    } else {
        File::open(path)
            .map_err(ReadFailure::Open)?
            .read_to_end(&mut bytes)
            .map_err(ReadFailure::Read)?;
    }
    Ok(bytes)
}

/// `--files0-from=F`: the input names, NUL-separated, read from `F`.
///
/// NUL is the separator precisely because it is the one byte a path cannot
/// contain, which is what makes `find -print0 | sort --files0-from=-` safe for
/// names holding spaces or newlines. A zero-length name is therefore not an
/// empty path but a malformed list, and is refused with the entry number so the
/// producer can be found.
fn read_files0(list: &OsString) -> Result<Vec<OsString>, String> {
    let bytes = read_file(list).map_err(|failure| match failure {
        ReadFailure::Open(e) => {
            format!("open failed: {}: {}", quotef_os(list), strerror(&e))
        }
        ReadFailure::Read(_) => {
            format!("cannot read file names from {}", quoteaf_os(list))
        }
    })?;
    // A list may or may not end with a separator; both are the same list, so
    // drop the trailing one rather than let it produce an empty final name.
    let body = bytes.strip_suffix(b"\0").unwrap_or(&bytes);
    if body.is_empty() {
        return Err(format!("no input from '{}'", list.to_string_lossy()));
    }
    let mut names = Vec::new();
    for (index, name) in body.split(|&c| c == 0).enumerate() {
        if name.is_empty() {
            return Err(format!(
                "{}:{}: invalid zero-length file name",
                list.to_string_lossy(),
                index.saturating_add(1)
            ));
        }
        names.push(os_from_bytes(name));
    }
    Ok(names)
}

/// Split input into lines on `delim`.
///
/// A final line with no terminator is still a line — `printf 'b\na'` has two —
/// and the terminator is supplied on output, so the result is terminated even
/// when the input was not.
fn split_lines(data: &[u8], delim: u8) -> Vec<&[u8]> {
    let mut lines: Vec<&[u8]> = Vec::new();
    let mut start = 0usize;
    for (index, &byte) in data.iter().enumerate() {
        if byte == delim {
            lines.push(data.get(start..index).unwrap_or_default());
            start = index.saturating_add(1);
        }
    }
    if start < data.len() {
        lines.push(data.get(start..).unwrap_or_default());
    }
    lines
}

fn write_out(cfg: &Config, lines: &[&[u8]]) -> io::Result<()> {
    let mut sink: Box<dyn Write> = match &cfg.output {
        Some(path) => Box::new(io::BufWriter::new(File::create(path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("open failed: {}: {}", quotef_os(path), strerror(&e)),
            )
        })?)),
        None => Box::new(io::BufWriter::new(io::stdout().lock())),
    };
    let mut previous: Option<&[u8]> = None;
    for line in lines {
        if cfg.unique
            && let Some(prev) = previous
            && compare(cfg, prev, line) == Ordering::Equal
        {
            continue;
        }
        sink.write_all(line)?;
        sink.write_all(&[cfg.delim])?;
        previous = Some(line);
    }
    sink.flush()
}

// ── command line ────────────────────────────────────────────────────────────

/// A command line that cannot be run, and the status to exit with.
///
/// This is [`coreutils::getopt::Error`], under the name the rest of this file
/// already used for it. The status split it carries — 2 for a bad option, 1 for
/// a bad argument *to* an option — is documented there, and is why the type is
/// shared rather than reimplemented per utility.
type Fatal = getopt::Error;

/// Every hand-written message in this file becomes a `Fatal` through here, so
/// `sort`'s status lives only in [`SORT`] and cannot be typed out again with
/// the wrong number.
///
/// It covers I/O failures as well as usage errors because `sort` makes no
/// distinction: measured, both `sort -k0` and `sort --files0-from=/nonexistent`
/// exit 2, which is upstream's single `SORT_FAILURE`.
///
/// There is deliberately no `From<String>` for [`getopt::Error`] doing this
/// implicitly: the status is per-utility, so a blanket conversion would have to
/// pick one number and be silently wrong for every utility of the other kind.
fn fatal(message: String) -> Fatal {
    SORT.usage(message)
}

/// Parse argv. `Ok(None)` means `--help` or `--version` has already answered.
#[allow(clippy::too_many_lines)]
fn parse_args(raw: &[OsString]) -> Result<Option<Config>, Fatal> {
    let mut cfg = Config::default();
    // The options that are not attached to a `-k` collect here, and are then
    // either handed to the keys that named no ordering of their own or, if
    // there are no keys at all, turned into one whole-line key.
    let mut global = KeySpec::whole_line();
    let mut only_operands = false;
    let mut i = 0usize;

    while let Some(arg) = raw.get(i) {
        let bytes = arg_bytes(arg);
        i = i.saturating_add(1);

        if only_operands {
            cfg.files.push(arg.clone());
            continue;
        }
        if bytes == b"--" {
            only_operands = true;
            continue;
        }
        if bytes.starts_with(b"--") {
            match long_option(&bytes, raw, &mut i, &mut cfg, &mut global)? {
                Answered::Yes => return Ok(None),
                Answered::No => {}
            }
            continue;
        }
        // `+POS [-POS]`: the obsolete key syntax. An argument that starts with
        // `+` but is not a position is a file, which is the only reason a file
        // called `+x` still works.
        if bytes.first() == Some(&b'+')
            && let Some(mut key) = parse_obsolete_start(&bytes)
        {
            if let Some(next) = raw.get(i) {
                let next_bytes = arg_bytes(next);
                if next_bytes.first() == Some(&b'-')
                    && next_bytes
                        .get(1)
                        .copied()
                        .is_some_and(|c| c.is_ascii_digit())
                {
                    parse_obsolete_end(&next_bytes, &mut key).map_err(fatal)?;
                    i = i.saturating_add(1);
                }
            }
            cfg.keys.push(key);
            continue;
        }
        if bytes.len() < 2 || bytes.first() != Some(&b'-') {
            cfg.files.push(arg.clone());
            continue;
        }

        let mut rest = bytes.get(1..).unwrap_or_default().to_vec();
        while let Some(&flag) = rest.first() {
            rest.remove(0);
            // An option that takes a value takes the rest of this bundle, or
            // the next argument if the bundle is exhausted.
            let mut take_value = |rest: &mut Vec<u8>| -> Result<Vec<u8>, Fatal> {
                if !rest.is_empty() {
                    return Ok(std::mem::take(rest));
                }
                let value = raw
                    .get(i)
                    .ok_or_else(|| SORT.short_missing_argument(flag))?;
                i = i.saturating_add(1);
                Ok(arg_bytes(value))
            };
            match flag {
                b'b' | b'd' | b'f' | b'g' | b'h' | b'i' | b'M' | b'n' | b'V' => {
                    keydef::set_ordering(&[flag], &mut global, Blanks::Both);
                }
                // `-r` lands on the global key like every other ordering
                // option, so a key that names none inherits it; the last
                // resort then takes it from there.
                b'r' => global.reverse = true,
                b'u' => cfg.unique = true,
                b's' => cfg.stable = true,
                b'm' => cfg.merge = true,
                b'c' => cfg.check = Some(Check::Diagnose),
                b'C' => cfg.check = Some(Check::Quiet),
                b'z' => cfg.delim = 0,
                b'k' => cfg
                    .keys
                    .push(parse_key(&take_value(&mut rest)?).map_err(fatal)?),
                b't' => cfg.tab = Some(parse_tab(&take_value(&mut rest)?, cfg.tab).map_err(fatal)?),
                b'o' => cfg.output = Some(os_from_bytes(&take_value(&mut rest)?)),
                // Resource hints. This sort holds the whole input in memory, so
                // there are no temporary files to place, no external program to
                // compress them with, and nothing to parallelise; accepting and
                // ignoring them keeps existing command lines working.
                b'S' | b'T' | b'y' => {
                    let _ = take_value(&mut rest)?;
                }
                b'R' => return Err(fatal(RANDOM_UNIMPLEMENTED.to_string())),
                // `other` is a byte, not a `char`: `other as char` would map
                // 0xC3 to `Ã` and re-encode it as two bytes, so a bundle like
                // `-é` would be reported as an option nobody typed.
                other => return Err(SORT.invalid_option(other)),
            }
        }
    }

    if let Some(list) = cfg.files0_from.clone() {
        // The two ways of naming inputs are exclusive, and GNU says so rather
        // than picking one: a command line that used both meant something the
        // reader cannot guess.
        if let Some(extra) = cfg.files.first() {
            return Err(fatal(format!(
                "extra operand {}\nfile operands cannot be combined with --files0-from\n\
                 Try 'sort --help' for more information.",
                quoteaf_os(extra)
            )));
        }
        cfg.files = read_files0(&list).map_err(fatal)?;
    }
    if cfg.check.is_some() && cfg.files.len() > 1 {
        let extra = cfg.files.get(1).map_or_else(String::new, quoteaf_os);
        return Err(fatal(format!("extra operand {extra} not allowed with -c")));
    }
    if cfg.files.is_empty() {
        cfg.files.push(OsString::from("-"));
    }

    // Inheritance, in GNU's order: keys that named no ordering take the global
    // one; if there are no keys at all and the global names an ordering, it
    // becomes a whole-line key. `-r` alone does not make a key — it is applied
    // to the last-resort comparison instead.
    for key in &mut cfg.keys {
        if !key.has_ordering() {
            key.inherit(&global);
        }
    }
    if cfg.keys.is_empty() && global.makes_a_key() {
        cfg.keys.push(global.clone());
    }
    cfg.reverse = global.reverse;
    Ok(Some(cfg))
}

/// Whether a long option has already printed the answer.
enum Answered {
    Yes,
    No,
}

/// The name every diagnostic below is stamped with, and the status a bad
/// command line exits with, both bound once.
///
/// The 2 is measured and is the minority: almost every GNU utility exits 1 for
/// a usage error, but `sort` has already given 1 a meaning — `sort -c` exits 1
/// when it finds the input unsorted — so it uses 2. `ls` and `grep` do the same
/// for the same reason. Note this does *not* extend to a bad argument to an
/// option: `sort --sort=bogus` is 1, which `getopt::argmatch` handles.
const SORT: Program = Program::new("sort", 2);

/// Every long option `sort` knows, with what it takes.
///
/// The table must list options we do *not* implement (`--debug`, `--random-sort`)
/// as well, because it is also what decides whether an abbreviation is
/// ambiguous: without `--debug` in it, `--d` would resolve to
/// `--dictionary-order` instead of being refused, and a user who typed `--d`
/// meaning `--debug` would silently get a dictionary sort.
///
/// **The order is load-bearing and is GNU's, not alphabetical**, because
/// `getopt_long` lists an ambiguous prefix's candidates in table order:
///
/// ```text
/// $ sort --r
/// sort: option '--r' is ambiguous; possibilities: '--random-sort' '--random-source' '--reverse'
/// ```
///
/// It was measured rather than recalled, because recall got it wrong —
/// `--random-sort` precedes `--random-source`, which is not the order anyone
/// would guess. The instrument is one command: an empty prefix matches every
/// option, so `sort --=x` prints the whole table in declaration order. See
/// [`Program::resolve_long`], which is what makes the order observable.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("ignore-leading-blanks", Takes::Nothing),
    ("check", Takes::Optional),
    ("compress-program", Takes::Required),
    ("debug", Takes::Nothing),
    ("dictionary-order", Takes::Nothing),
    ("ignore-case", Takes::Nothing),
    ("files0-from", Takes::Required),
    ("general-numeric-sort", Takes::Nothing),
    ("ignore-nonprinting", Takes::Nothing),
    ("key", Takes::Required),
    ("merge", Takes::Nothing),
    ("month-sort", Takes::Nothing),
    ("numeric-sort", Takes::Nothing),
    ("human-numeric-sort", Takes::Nothing),
    ("version-sort", Takes::Nothing),
    ("random-sort", Takes::Nothing),
    ("random-source", Takes::Required),
    ("sort", Takes::Required),
    ("output", Takes::Required),
    ("reverse", Takes::Nothing),
    ("stable", Takes::Nothing),
    ("batch-size", Takes::Required),
    ("buffer-size", Takes::Required),
    ("field-separator", Takes::Required),
    ("temporary-directory", Takes::Required),
    ("unique", Takes::Nothing),
    ("zero-terminated", Takes::Nothing),
    ("parallel", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

fn long_option(
    bytes: &[u8],
    raw: &[OsString],
    i: &mut usize,
    cfg: &mut Config,
    global: &mut KeySpec,
) -> Result<Answered, Fatal> {
    let body = bytes.get(2..).unwrap_or_default();
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            body.get(at.saturating_add(1)..),
        ),
        None => (body, None),
    };
    // A long option's name is ASCII; anything else cannot match one, and
    // reporting it as unrecognized is what GNU's byte comparison amounts to.
    let typed = std::str::from_utf8(typed).map_err(|_| SORT.unrecognized_option(bytes))?;
    let (name, takes) = SORT.resolve_long(typed, bytes, LONG_OPTIONS)?;

    if takes == Takes::Nothing && inline.is_some() {
        return Err(SORT.long_unwanted_argument(name));
    }
    // A required value may be written `--key=2` or `--key 2`; an optional one
    // only ever comes from the `=` form.
    let value: Option<Vec<u8>> = match (takes, inline) {
        (_, Some(v)) => Some(v.to_vec()),
        (Takes::Required, None) => {
            let next = raw
                .get(*i)
                .ok_or_else(|| SORT.long_missing_argument(name))?;
            *i = i.saturating_add(1);
            Some(arg_bytes(next))
        }
        _ => None,
    };
    // Every `Required` option reached here has a value, so this cannot fail;
    // it is written as a fallback rather than an unwrap all the same.
    let need = || value.clone().unwrap_or_default();

    match name {
        "help" => {
            println!("{USAGE}");
            return Ok(Answered::Yes);
        }
        "version" => {
            println!("sort (SlateOS coreutils)");
            return Ok(Answered::Yes);
        }
        "ignore-leading-blanks" => {
            keydef::set_ordering(b"b", global, Blanks::Both);
        }
        "dictionary-order" => global.ignore = Some(Ignore::NonDictionary),
        "ignore-nonprinting" => global.ignore = Some(Ignore::NonPrinting),
        "ignore-case" => global.fold = true,
        "general-numeric-sort" => global.kind = Kind::General,
        "human-numeric-sort" => global.kind = Kind::Human,
        "month-sort" => global.kind = Kind::Month,
        "numeric-sort" => global.kind = Kind::Numeric,
        "version-sort" => global.kind = Kind::Version,
        "reverse" => global.reverse = true,
        "unique" => cfg.unique = true,
        "stable" => cfg.stable = true,
        "merge" => cfg.merge = true,
        "zero-terminated" => cfg.delim = 0,
        "sort" => global.kind = parse_sort_word(&need())?,
        "check" => {
            cfg.check = Some(match value.as_deref() {
                None => Check::Diagnose,
                Some(word) => SORT.argmatch(word, "--check", CHECK_WORDS)?,
            });
        }
        "key" => cfg.keys.push(parse_key(&need()).map_err(fatal)?),
        "field-separator" => cfg.tab = Some(parse_tab(&need(), cfg.tab).map_err(fatal)?),
        "output" => cfg.output = Some(os_from_bytes(&need())),
        "files0-from" => cfg.files0_from = Some(os_from_bytes(&need())),
        "random-sort" => return Err(fatal(RANDOM_UNIMPLEMENTED.to_string())),
        "debug" => return Err(fatal(DEBUG_UNIMPLEMENTED.to_string())),
        // Accepted and ignored, as for their short forms.
        _ => {}
    }
    Ok(Answered::No)
}

/// `--check`'s words. `quiet` and `silent` are two spellings of one answer,
/// which `argmatch` can see because they carry the same value — that is what
/// stops a prefix matching only those two from being called ambiguous, and what
/// makes them share a line in the "Valid arguments are" list.
const CHECK_WORDS: &[(&str, Check)] = &[
    ("quiet", Check::Quiet),
    ("silent", Check::Quiet),
    ("diagnose-first", Check::Diagnose),
];

/// `--sort`'s words. `random` is spelled `None` because we recognise it — it is
/// a real ordering and must not be reported as an invalid argument — but cannot
/// perform it yet.
const SORT_WORDS: &[(&str, Option<Kind>)] = &[
    ("general-numeric", Some(Kind::General)),
    ("human-numeric", Some(Kind::Human)),
    ("month", Some(Kind::Month)),
    ("numeric", Some(Kind::Numeric)),
    ("random", None),
    ("version", Some(Kind::Version)),
];

/// `--sort=WORD`, the long spelling of the ordering options.
fn parse_sort_word(word: &[u8]) -> Result<Kind, Fatal> {
    SORT.argmatch(word, "--sort", SORT_WORDS)?
        .ok_or_else(|| fatal(RANDOM_UNIMPLEMENTED.to_string()))
}

/// The two options we accept into the parser and then refuse, each with the
/// reason rather than a bare "unknown option" — a user who typed one asked for
/// something real and deserves to be told it is missing, not that it does not
/// exist.
const RANDOM_UNIMPLEMENTED: &str =
    "random sort is not implemented: it needs a keyed hash this system does not have yet";
const DEBUG_UNIMPLEMENTED: &str = "--debug is not implemented";

/// `-t`'s argument: one byte, or the two characters `\0` for NUL.
fn parse_tab(value: &[u8], existing: Option<u8>) -> Result<u8, String> {
    let tab = match value {
        [] => return Err("empty tab".to_string()),
        [one] => *one,
        b"\\0" => 0,
        other => {
            return Err(format!("multi-character tab {}", quote(other)));
        }
    };
    // Two different `-t`s are a contradiction, not a last-one-wins.
    if existing.is_some_and(|e| e != tab) {
        return Err("incompatible tabs".to_string());
    }
    Ok(tab)
}

#[cfg(unix)]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    a.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    a.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn os_from_bytes(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b.to_vec())
}

#[cfg(not(unix))]
fn os_from_bytes(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

fn die(msg: &str) -> ! {
    die_with(msg, 2)
}

fn die_with(msg: &str, status: i32) -> ! {
    diag!("sort: {msg}");
    process::exit(status)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn cfg_of(args: &[&str]) -> Config {
        let raw: Vec<OsString> = args.iter().map(OsString::from).collect();
        parse_args(&raw).unwrap().unwrap()
    }

    fn run(args: &[&str], input: &str) -> String {
        let cfg = cfg_of(args);
        let data = input.as_bytes().to_vec();
        let mut lines = split_lines(&data, cfg.delim);
        lines.sort_by(|a, b| compare(&cfg, a, b));
        let mut out = String::new();
        let mut previous: Option<&[u8]> = None;
        for line in &lines {
            if cfg.unique
                && let Some(prev) = previous
                && compare(&cfg, prev, line) == Ordering::Equal
            {
                continue;
            }
            out.push_str(&String::from_utf8_lossy(line));
            out.push('\n');
            previous = Some(line);
        }
        out
    }

    #[test]
    fn unique_deduplicates_by_key_not_by_line() {
        // The bug this rewrite exists to fix: `1`, `1.0` and `01` are one
        // number, so `-nu` keeps one of them.
        assert_eq!(run(&["-nu"], "1\n1.0\n01\n"), "1\n");
        // Without `-n` they are three different lines.
        assert_eq!(run(&["-u"], "1\n1.0\n01\n"), "01\n1\n1.0\n");
    }

    #[test]
    fn tied_keys_are_broken_by_the_whole_line() {
        // Field 2 ties, so the whole line decides and the order is total.
        assert_eq!(run(&["-k2,2"], "b x\na x\n"), "a x\nb x\n");
        // ...unless `-s` says to keep the input order instead.
        assert_eq!(run(&["-s", "-k2,2"], "b x\na x\n"), "b x\na x\n");
    }

    #[test]
    fn the_last_resort_is_reversed_by_r() {
        assert_eq!(run(&["-r", "-k2,2"], "a x\nb x\n"), "b x\na x\n");
    }

    #[test]
    fn a_key_that_names_its_own_ordering_inherits_nothing_else() {
        // `-r` is not inherited by `-k2,2n`, so this sorts ascending by field
        // 2 — GNU's behaviour, and the one that surprises people.
        assert_eq!(run(&["-r", "-k2,2n"], "a 2\nb 1\nc 3\n"), "b 1\na 2\nc 3\n");
        // A key naming no ordering does inherit, reverse included.
        assert_eq!(
            run(&["-r", "-n", "-k2,2"], "a 2\nb 1\nc 3\n"),
            "c 3\na 2\nb 1\n"
        );
    }

    #[test]
    fn a_global_ordering_with_no_key_becomes_one_whole_line_key() {
        assert_eq!(run(&["-n"], "10\n9\n"), "9\n10\n");
        // `-r` alone makes no key, so the whole-line fallback does the work.
        let cfg = cfg_of(&["-r"]);
        assert!(cfg.keys.is_empty());
        let cfg = cfg_of(&["-n"]);
        assert_eq!(cfg.keys.len(), 1);
    }

    #[test]
    fn lines_are_bytes() {
        // Not valid UTF-8. The old sort stopped here with a diagnostic.
        let data = b"a\n\xff\nb\n".to_vec();
        let cfg = Config::default();
        let mut lines = split_lines(&data, b'\n');
        lines.sort_by(|a, b| compare(&cfg, a, b));
        assert_eq!(
            lines,
            vec![b"a".as_slice(), b"b".as_slice(), b"\xff".as_slice()]
        );
    }

    #[test]
    fn a_last_line_without_a_terminator_is_still_a_line() {
        assert_eq!(split_lines(b"b\na", b'\n').len(), 2);
        assert_eq!(split_lines(b"b\na\n", b'\n').len(), 2);
        assert_eq!(split_lines(b"", b'\n').len(), 0);
        assert_eq!(split_lines(b"\n", b'\n'), vec![b"".as_slice()]);
    }

    #[test]
    fn separators_and_fields() {
        assert_eq!(run(&["-t:", "-k2,2"], "a:2\nb:1\n"), "b:1\na:2\n");
        assert_eq!(run(&["-t", ":", "-k2,2n"], "a:10\nb:9\n"), "b:9\na:10\n");
        assert_eq!(cfg_of(&["-t", "\\0"]).tab, Some(0));
    }

    #[test]
    fn bad_command_lines_are_refused() {
        let fail = |args: &[&str]| {
            let raw: Vec<OsString> = args.iter().map(OsString::from).collect();
            parse_args(&raw).unwrap_err()
        };
        let err = |args: &[&str]| fail(args).sentence;
        assert!(err(&["-q"]).starts_with("invalid option -- 'q'"));
        assert_eq!(err(&["-t", ""]), "empty tab");
        assert_eq!(err(&["-t", "ab"]), "multi-character tab ‘ab’");
        assert_eq!(err(&["-t:", "-t;"]), "incompatible tabs");
        assert_eq!(
            err(&["-c", "a", "b"]),
            // Straight: both `extra operand` messages route through
            // `quoteaf`, which §351 keeps straight in every locale, unlike
            // the `multi-character tab ‘ab’` above which uses `quote`.
            "extra operand 'b' not allowed with -c"
        );
        assert!(err(&["-k"]).contains("requires an argument"));
        // Every failure exits 2 except a bad *argument to* an option, which
        // gnulib's argmatch exits 1 for. Both are measured from GNU.
        assert_eq!(fail(&["-q"]).status, 2);
        assert_eq!(fail(&["--sort=bogus"]).status, 1);
        assert_eq!(fail(&["--check=bogus"]).status, 1);
    }

    fn fail_msg(args: &[&str]) -> String {
        let raw: Vec<OsString> = args.iter().map(OsString::from).collect();
        // Every getopt and argmatch diagnostic ends with the same referral;
        // taking the sentence alone keeps the assertions to what is under test.
        #[allow(clippy::unwrap_used)]
        parse_args(&raw).unwrap_err().sentence
    }

    /// `getopt_long` resolves an abbreviation only when it is unambiguous, and
    /// an exact match always wins even when it is a prefix of something longer.
    /// Both were read off GNU: `sort --rev` reverses, `sort --r` is refused as
    /// ambiguous, and `sort --version` prints a version rather than complaining
    /// about `--version-sort`.
    #[test]
    fn long_options_abbreviate_the_way_getopt_long_does() {
        assert!(cfg_of(&["--rev"]).reverse);
        assert_eq!(
            fail_msg(&["--r"]),
            "option '--r' is ambiguous; possibilities: '--random-sort' '--random-source' '--reverse'"
        );
        assert_eq!(
            fail_msg(&["--d"]),
            "option '--d' is ambiguous; possibilities: '--debug' '--dictionary-order'"
        );
        // An option that takes a value takes it from the next argument too.
        assert_eq!(cfg_of(&["--key", "2"]).keys.len(), 1);
        assert_eq!(cfg_of(&["--sort", "numeric"]).keys.len(), 1);
        // `--check` takes an *optional* value, so it never reaches for the next
        // argument: `--check quiet` checks and leaves `quiet` an operand.
        let cfg = cfg_of(&["--check", "quiet"]);
        assert_eq!(cfg.check, Some(Check::Diagnose));
        assert_eq!(cfg.files, vec![OsString::from("quiet")]);
    }

    /// Every getopt sentence, against what glibc actually prints.
    ///
    /// These were wrong for a long time and the differential harness passed
    /// anyway, because the `sort` it compared against was MSYS2's — a Cygwin
    /// derivative whose getopt is not glibc's and which says `unknown option --
    /// bogus` where glibc says `unrecognized option '--bogus'`. The literals
    /// below are from glibc, and are here rather than only in the harness so
    /// that they are checked on a host with no reference sort at all.
    #[test]
    fn every_getopt_sentence_matches_glibc() {
        // A short option and a long one get different sentences.
        assert_eq!(fail_msg(&["-x"]), "invalid option -- 'x'");
        assert_eq!(fail_msg(&["-k"]), "option requires an argument -- 'k'");
        assert_eq!(fail_msg(&["--fo"]), "unrecognized option '--fo'");
        // The two that failed to resolve echo what was typed, `=VALUE` and all.
        assert_eq!(fail_msg(&["--fo=bar"]), "unrecognized option '--fo=bar'");
        // The two that resolved name the *resolution*: `--k` is reported as
        // `--key`, and `--stab=x` as `--stable`.
        assert_eq!(fail_msg(&["--k"]), "option '--key' requires an argument");
        assert_eq!(
            fail_msg(&["--stab=x"]),
            "option '--stable' doesn't allow an argument"
        );
        assert_eq!(
            fail_msg(&["--sort"]),
            "option '--sort' requires an argument"
        );
        // The ambiguous list is in the order the options are declared, which is
        // GNU's array order and not alphabetical: `--random-sort` precedes
        // `--random-source`, and an empty prefix matches every option.
        let all = fail_msg(&["--=x"]);
        assert!(all.starts_with(
            "option '--=x' is ambiguous; possibilities: \
             '--ignore-leading-blanks' '--check' '--compress-program' '--debug'"
        ));
        assert!(all.ends_with("'--parallel' '--help' '--version'"));
        assert_eq!(all.matches("' '").count(), LONG_OPTIONS.len() - 1);
    }

    /// The names in these diagnostics are quoted, which is where this parts
    /// company with GNU on purpose.
    ///
    /// glibc puts the option between two literal `'` and escapes nothing, so a
    /// file called `--fo\nsort: /etc/shadow: Permission denied`, picked up by
    /// `sort *`, makes GNU print a second line that `sort` never wrote. Ours
    /// renders it on one line. For every option a person would type the two are
    /// byte-identical, which the test above is what checks.
    #[test]
    fn an_option_name_cannot_forge_a_second_diagnostic_line() {
        let forged = fail_msg(&["--fo\nsort: /etc/shadow: Permission denied"]);
        assert_eq!(
            forged,
            r#"unrecognized option '--fo\nsort: /etc/shadow: Permission denied'"#
        );
        assert!(!forged.contains('\n'));
        // A short option too, and a byte that is not ASCII at all: reported as
        // the byte it was, not re-encoded through `char`.
        assert_eq!(fail_msg(&["-\n"]), r"invalid option -- '\n'");
        assert_eq!(fail_msg(&["-\u{e9}"]), r"invalid option -- '\303'");
    }

    /// `argmatch` resolves an option's *argument* by prefix, exactly as getopt
    /// resolves the option's name — `--sort=hum` and `--check=q` both work.
    /// This implementation did not do it at all until the getopt sweep; both
    /// commands were refused as invalid arguments.
    #[test]
    fn an_option_argument_abbreviates_too() {
        assert_eq!(
            cfg_of(&["--sort=hum"]).keys.first().map(|k| k.kind),
            Some(Kind::Human)
        );
        assert_eq!(
            cfg_of(&["--sort=n"]).keys.first().map(|k| k.kind),
            Some(Kind::Numeric)
        );
        assert_eq!(cfg_of(&["--check=q"]).check, Some(Check::Quiet));
        assert_eq!(cfg_of(&["--check=d"]).check, Some(Check::Diagnose));
        // An exact match wins over any longer word it is a prefix of.
        assert_eq!(cfg_of(&["--check=quiet"]).check, Some(Check::Quiet));
    }

    /// argmatch's two sentences differ in one word, and which one you get turns
    /// on whether the candidates *mean* different things rather than on how
    /// many there are.
    #[test]
    fn an_ambiguous_option_argument_is_a_different_sentence_from_an_invalid_one() {
        let valid = "\nValid arguments are:\n  - ‘quiet’, ‘silent’\n  - ‘diagnose-first’";
        assert_eq!(
            fail_msg(&["--check=bogus"]),
            format!("invalid argument ‘bogus’ for ‘--check’{valid}")
        );
        // The empty string is a prefix of all three words, which disagree.
        assert_eq!(
            fail_msg(&["--check="]),
            format!("ambiguous argument ‘’ for ‘--check’{valid}")
        );
        // `quiet` and `silent` share a value, so they share a line in the list
        // above — and a prefix matching only those two would *not* be
        // ambiguous, because there would be nothing to disambiguate. `--sort`
        // has no two words with one value, so every word gets its own line.
        assert!(fail_msg(&["--sort="]).contains("  - ‘month’\n  - ‘numeric’\n"));
        // A multi-byte word cannot prefix any of these ASCII words, so it takes
        // the invalid-argument path — and reaches the message as itself, since
        // `quote()` escapes what does not decode rather than what is not ASCII.
        assert!(fail_msg(&["--sort=\u{e9}"]).starts_with("invalid argument ‘é’ for ‘--sort’"));
    }

    #[test]
    fn obsolete_keys_are_accepted_and_files_named_plus_are_not_mistaken_for_them() {
        let cfg = cfg_of(&["+1", "-2"]);
        assert_eq!(cfg.keys.len(), 1);
        assert!(cfg.files.iter().all(|f| f == "-"));
        // `+notanumber` is a file.
        let cfg = cfg_of(&["+file"]);
        assert!(cfg.keys.is_empty());
        assert_eq!(cfg.files, vec![OsString::from("+file")]);
    }

    #[test]
    fn check_reports_the_first_line_out_of_order() {
        let cfg = cfg_of(&["-c"]);
        let data = b"b\na\n".to_vec();
        let lines = split_lines(&data, b'\n');
        assert_eq!(check(&cfg, &lines, Check::Quiet), 1);
        let cfg = cfg_of(&["-c"]);
        let data = b"a\nb\n".to_vec();
        let lines = split_lines(&data, b'\n');
        assert_eq!(check(&cfg, &lines, Check::Quiet), 0);
        // With `-u`, an equal pair is also out of order.
        let cfg = cfg_of(&["-cu"]);
        let data = b"a\na\n".to_vec();
        let lines = split_lines(&data, b'\n');
        assert_eq!(check(&cfg, &lines, Check::Quiet), 1);
    }

    #[test]
    fn merge_is_a_merge_and_not_a_re_sort() {
        // GNU's `-m` on input that is not in fact sorted answers with the
        // merge, which is not what sorting the concatenation would give.
        let cfg = cfg_of(&["-m"]);
        let a = b"3\n1\n".to_vec();
        let b = b"2\n".to_vec();
        let per_file = vec![split_lines(&a, b'\n'), split_lines(&b, b'\n')];
        let out = merge(&cfg, &per_file);
        assert_eq!(out, vec![b"2".as_slice(), b"3".as_slice(), b"1".as_slice()]);
    }
}
