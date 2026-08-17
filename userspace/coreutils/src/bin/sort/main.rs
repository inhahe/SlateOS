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
//! Exit status: 0, 1 if `-c`/`-C` found the input out of order, 2 for any
//! other failure.
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

use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;

use coreutils::errmsg::strerror;
use keydef::{Blanks, KeySpec, Kind, parse_key, parse_obsolete_end, parse_obsolete_start};
use order::Ignore;

const USAGE: &str = "usage: sort [-bcCdfghimMnrsuVz] [-k KEYDEF]... [-t SEP] [-o FILE] [FILE...]";

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
        }
    }
}

fn main() {
    let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let cfg = match parse_args(&raw) {
        Ok(Some(c)) => c,
        Ok(None) => return,
        Err(e) => die(&e),
    };

    // Every input is read before anything is written, which is what lets
    // `sort -o f f` work: by the time the output file is truncated its old
    // contents are already in memory.
    let mut contents: Vec<Vec<u8>> = Vec::with_capacity(cfg.files.len());
    for path in &cfg.files {
        match read_file(path) {
            Ok(bytes) => contents.push(bytes),
            Err(e) => die(&format!(
                "cannot read: {}: {}",
                path.to_string_lossy(),
                strerror(&e)
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

fn read_file(path: &OsString) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if path == "-" {
        io::stdin().lock().read_to_end(&mut bytes)?;
    } else {
        File::open(path)?.read_to_end(&mut bytes)?;
    }
    Ok(bytes)
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
                format!("open failed: {}: {}", path.to_string_lossy(), strerror(&e)),
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

/// Parse argv. `Ok(None)` means `--help` or `--version` has already answered.
#[allow(clippy::too_many_lines)]
fn parse_args(raw: &[OsString]) -> Result<Option<Config>, String> {
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
            match long_option(&bytes, &mut cfg, &mut global)? {
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
                    parse_obsolete_end(&next_bytes, &mut key)?;
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
            let mut take_value = |rest: &mut Vec<u8>| -> Result<Vec<u8>, String> {
                if !rest.is_empty() {
                    return Ok(std::mem::take(rest));
                }
                let value = raw
                    .get(i)
                    .ok_or_else(|| format!("option requires an argument -- '{}'", flag as char))?;
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
                b'k' => cfg.keys.push(parse_key(&take_value(&mut rest)?)?),
                b't' => cfg.tab = Some(parse_tab(&take_value(&mut rest)?, cfg.tab)?),
                b'o' => cfg.output = Some(os_from_bytes(&take_value(&mut rest)?)),
                // Resource hints. This sort holds the whole input in memory, so
                // there are no temporary files to place, no external program to
                // compress them with, and nothing to parallelise; accepting and
                // ignoring them keeps existing command lines working.
                b'S' | b'T' | b'y' => {
                    let _ = take_value(&mut rest)?;
                }
                b'R' => {
                    return Err(
                        "-R (random sort) is not implemented: it needs a keyed hash this system \
                         does not have yet"
                            .to_string(),
                    );
                }
                other => {
                    return Err(format!(
                        "unknown option -- {}\nTry 'sort --help' for more information.",
                        other as char
                    ));
                }
            }
        }
    }

    if cfg.check.is_some() && cfg.files.len() > 1 {
        let extra = cfg
            .files
            .get(1)
            .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
        return Err(format!("extra operand '{extra}' not allowed with -c"));
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

fn long_option(bytes: &[u8], cfg: &mut Config, global: &mut KeySpec) -> Result<Answered, String> {
    let (name, value) = match bytes.iter().position(|&c| c == b'=') {
        Some(at) => (
            bytes.get(..at).unwrap_or_default(),
            bytes.get(at.saturating_add(1)..),
        ),
        None => (bytes, None),
    };
    let need = |what: &str| -> Result<Vec<u8>, String> {
        value
            .map(<[u8]>::to_vec)
            .ok_or_else(|| format!("option '{what}' requires an argument"))
    };
    match name {
        b"--help" => {
            println!("{USAGE}");
            return Ok(Answered::Yes);
        }
        b"--version" => {
            println!("sort (SlateOS coreutils)");
            return Ok(Answered::Yes);
        }
        b"--ignore-leading-blanks" => {
            keydef::set_ordering(b"b", global, Blanks::Both);
        }
        b"--dictionary-order" => global.ignore = Some(Ignore::NonDictionary),
        b"--ignore-nonprinting" => global.ignore = Some(Ignore::NonPrinting),
        b"--ignore-case" => global.fold = true,
        b"--general-numeric-sort" => global.kind = Kind::General,
        b"--human-numeric-sort" => global.kind = Kind::Human,
        b"--month-sort" => global.kind = Kind::Month,
        b"--numeric-sort" => global.kind = Kind::Numeric,
        b"--version-sort" => global.kind = Kind::Version,
        b"--reverse" => global.reverse = true,
        b"--unique" => cfg.unique = true,
        b"--stable" => cfg.stable = true,
        b"--merge" => cfg.merge = true,
        b"--zero-terminated" => cfg.delim = 0,
        b"--check" => {
            cfg.check = Some(match value {
                None | Some(b"diagnose-first") => Check::Diagnose,
                Some(b"quiet" | b"silent") => Check::Quiet,
                Some(other) => {
                    return Err(format!(
                        "invalid argument '{}' for '--check'",
                        String::from_utf8_lossy(other)
                    ));
                }
            });
        }
        b"--key" => cfg.keys.push(parse_key(&need("--key")?)?),
        b"--field-separator" => cfg.tab = Some(parse_tab(&need("--field-separator")?, cfg.tab)?),
        b"--output" => cfg.output = Some(os_from_bytes(&need("--output")?)),
        // Accepted and ignored, as for their short forms.
        b"--buffer-size"
        | b"--temporary-directory"
        | b"--parallel"
        | b"--compress-program"
        | b"--batch-size"
        | b"--random-source" => {}
        other => {
            return Err(format!(
                "unrecognized option '{}'\nTry 'sort --help' for more information.",
                String::from_utf8_lossy(other)
            ));
        }
    }
    Ok(Answered::No)
}

/// `-t`'s argument: one byte, or the two characters `\0` for NUL.
fn parse_tab(value: &[u8], existing: Option<u8>) -> Result<u8, String> {
    let tab = match value {
        [] => return Err("empty tab".to_string()),
        [one] => *one,
        b"\\0" => 0,
        other => {
            return Err(format!(
                "multi-character tab '{}'",
                String::from_utf8_lossy(other)
            ));
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
    eprintln!("sort: {msg}");
    process::exit(2)
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
        let err = |args: &[&str]| {
            let raw: Vec<OsString> = args.iter().map(OsString::from).collect();
            parse_args(&raw).unwrap_err()
        };
        assert!(err(&["-q"]).starts_with("unknown option -- q"));
        assert_eq!(err(&["-t", ""]), "empty tab");
        assert_eq!(err(&["-t", "ab"]), "multi-character tab 'ab'");
        assert_eq!(err(&["-t:", "-t;"]), "incompatible tabs");
        assert_eq!(
            err(&["-c", "a", "b"]),
            "extra operand 'b' not allowed with -c"
        );
        assert!(err(&["-k"]).contains("requires an argument"));
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
