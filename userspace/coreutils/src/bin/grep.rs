//! grep — select the lines of its input that match a pattern.
//!
//! ```text
//! grep [OPTION]... PATTERN [FILE]...
//! grep [OPTION]... -e PATTERN... [FILE]...
//! grep [OPTION]... -f PATTERNFILE... [FILE]...
//! ```
//!
//! | | |
//! |---|---|
//! | `-E` | PATTERN is an extended regular expression |
//! | `-F` | PATTERN is a fixed string, not a pattern |
//! | `-e P` | use P as a pattern; may be repeated |
//! | `-f F` | read patterns from file F, one per line |
//! | `-i` | match without regard to letter case |
//! | `-v` | select the lines that do *not* match |
//! | `-w` | the match must be a whole word |
//! | `-x` | the match must be the whole line |
//! | `-c` | print a count of selected lines instead of the lines |
//! | `-l` / `-L` | print the names of files that do / do not match |
//! | `-o` | print only the matching part of each line |
//! | `-n` | prefix each line with its line number |
//! | `-H` / `-h` | always / never prefix a line with its file name |
//! | `-q` | print nothing; the exit status is the answer |
//! | `-s` | do not report unreadable files |
//! | `-m N` | stop after N selected lines per file; `-m 0` prints nothing |
//! | `-r` | search directories recursively |
//! | `-Z` | write a NUL after a file name instead of the `:` or newline |
//! | `-z` | the input is NUL-separated too, and so is the output |
//! | `-a` | accepted and ignored: this grep never suppresses binary output |
//! | `--` | end of options; what follows is a pattern or a file |
//!
//! Exit status: 0 if a line was selected, 1 if none was, 2 on an error.
//!
//! ## Patterns are patterns
//!
//! Until `userspace/ere` existed this program matched with `str::contains`, so
//! `grep '^posix'` found nothing, `grep ' [ab]='` matched only a line that
//! literally contained `[ab]=`, and `-E` was an unknown option. It was a
//! quiet failure — the exit status said "no match", which is what a real grep
//! says about a file that does not match — and it survived its own test suite
//! because every test asserted the substring behaviour it had. See
//! `design-decisions.md` §322.
//!
//! Patterns are POSIX **Basic** regular expressions by default, *egrep*
//! syntax under `-E` — which is not quite POSIX-extended; see `ere::Syntax` —
//! and literal text under `-F`. Lines are bytes: a path on this system may
//! hold any byte but `/` and NUL, so a grep that insisted on UTF-8 could not
//! search a file listing.
//!
//! ## `-Z` and `-z` are not decoration here
//!
//! On a system whose paths may contain a newline, `grep -rl … | xargs` is
//! ambiguous by construction: the delimiter is a byte a name is allowed to
//! hold. `-Z` delimits names with the one byte a name cannot hold, and `-z`
//! makes grep read such a stream, so `find -print0 | xargs -0 grep -z` and
//! `grep -rlZ … | xargs -0` are the spellings that are actually correct.

use coreutils::diag;
use coreutils::quote::{self, quotef_os};
use coreutils::stdfd;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use coreutils::errmsg::strerror;
// Aliased: this file already has a `Syntax` — the `-G`/`-E`/`-F` selector —
// and `ere::Syntax` is the dialect that selector maps *onto*.
use ere::{MatchLimit, Regex, Syntax as EreSyntax, bre};

// Before `main`, so that `stdfd::restore` still sees a caller's `grep … >&-` as
// the closed descriptor it is rather than the `/dev/null` the Rust runtime
// substitutes for it before `main` runs. Without it `grep a abc >&-` prints
// nothing and exits 0, where GNU says `grep: write error: Bad file descriptor`
// and exits 2.
coreutils::guard_std_fds!();

/// Which language the patterns are written in.
#[derive(Clone, Copy, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Syntax {
    /// POSIX Basic regular expressions — grep's default, where `+` and `?` are
    /// literal characters and groups are spelled `\(…\)`.
    #[default]
    Basic,
    /// POSIX Extended regular expressions (`-E`).
    Extended,
    /// Literal text (`-F`). Not a language at all, but it reaches the same
    /// engine by being quoted into one, so `-i`, `-w` and `-x` need no second
    /// implementation.
    Fixed,
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    syntax: Syntax,
    ignore_case: bool,
    invert: bool,
    count_only: bool,
    line_numbers: bool,
    recursive: bool,
    /// `-R`: also follow a symbolic link found *during* the walk.
    ///
    /// `-r` and `-R` differ over exactly this. Both follow a link named on the
    /// command line — `grep -r foo link-to-dir` descends it — and only `-R`
    /// follows one it discovers. Ours followed them always until 2026-08-25,
    /// which reported files GNU does not and, on a tree containing a link back
    /// to one of its own ancestors, did not terminate.
    deref_links: bool,
    quiet: bool,
    /// `-w`: the match must be bounded by non-word characters.
    word: bool,
    /// `-x`: the match must be the entire line.
    whole_line: bool,
    /// `-o`: print the matching part rather than the line.
    only_matching: bool,
    files_with_matches: bool,
    files_without_match: bool,
    no_messages: bool,
    /// `-H`/`-h`, when given: overrides the usual "prefix when searching more
    /// than one file" rule.
    filename: Option<bool>,
    max_count: Option<usize>,
    /// `-Z`: write a NUL after a file name instead of the `:` or newline that
    /// normally follows it.
    ///
    /// Not a convenience. A path on this system may hold any byte but `/` and
    /// NUL — newline included — so `grep -rl … | xargs` is ambiguous by
    /// construction and `grep -rlZ … | xargs -0` is the only spelling that is
    /// not. The one byte a name cannot contain is the one that delimits it.
    null_name: bool,
    /// `-z`: the *input* is NUL-separated too, and so is the output.
    ///
    /// The other half of the same pipeline: `find -print0 | xargs -0 grep -z`
    /// only works if grep agrees about what a line is. Kept separate from
    /// `null_name` because GNU keeps them separate, and because `-z` alone
    /// changes what a line *is* while `-Z` alone only changes how a name is
    /// punctuated.
    null_data: bool,
}

impl Options {
    /// The byte that ends a line of input and of output: `\n`, or NUL under
    /// `-z`.
    fn line_sep(&self) -> u8 {
        if self.null_data { 0 } else { b'\n' }
    }

    /// What follows a file name that is being used as a prefix — `:` normally,
    /// NUL under `-Z`.
    fn name_sep(&self) -> u8 {
        if self.null_name { 0 } else { b':' }
    }
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct GrepArgs {
    opts: Options,
    /// Patterns given directly, by `-e` or as the first operand.
    patterns: Vec<Vec<u8>>,
    /// Files named by `-f`, whose lines are patterns. Read by `main`, so that
    /// argument parsing stays a pure function of argv.
    pattern_files: Vec<String>,
    files: Vec<String>,
}

/// A compiled pattern.
///
/// The engine rejects an empty pattern, as glibc does — but `grep ''` is not a
/// malformed pattern, it is the pattern that matches every line, and scripts
/// use it. So the empty case is carried here rather than pushed into the
/// engine, where it would have to be spelled as a regex that matches empty at
/// every position and would then be indistinguishable from one.
enum Pat {
    Empty,
    Re(Regex),
}

/// Parse grep's argv.
///
/// Clusters of single-letter options are supported, and an option that takes an
/// argument takes the rest of its cluster or, failing that, the next argument —
/// so `-m5`, `-m 5`, `-im5` and `-im 5` all mean the same. A bare `-` is the
/// name of standard input, not a cluster. `--` ends option parsing, which is
/// how a pattern beginning with `-` is given.
fn parse_args(args: &[String]) -> Result<GrepArgs, String> {
    let mut opts = Options::default();
    let mut patterns: Vec<Vec<u8>> = Vec::new();
    let mut pattern_files: Vec<String> = Vec::new();
    let mut positional: Vec<String> = Vec::new();
    let mut no_more_options = false;

    let mut i = 0;
    while let Some(arg) = args.get(i) {
        i = i.saturating_add(1);
        if no_more_options || arg == "-" || !arg.starts_with('-') {
            positional.push(arg.clone());
            continue;
        }
        if arg == "--" {
            no_more_options = true;
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            parse_long(long, &mut opts, &mut patterns, &mut pattern_files)?;
            continue;
        }

        // A cluster of short options. `rest` is consumed as we go so that an
        // option taking an argument can swallow what is left of it.
        let mut rest = arg.get(1..).unwrap_or("").chars();
        while let Some(c) = rest.next() {
            // The argument of an option that takes one: the remainder of this
            // cluster if there is any, otherwise the next argv entry.
            let mut take_arg = |what: char| -> Result<String, String> {
                let tail: String = rest.by_ref().collect();
                if !tail.is_empty() {
                    return Ok(tail);
                }
                let next = args.get(i).cloned();
                i = i.saturating_add(1);
                next.ok_or_else(|| format!("option requires an argument -- '{what}'"))
            };
            match c {
                'E' => opts.syntax = Syntax::Extended,
                'F' => opts.syntax = Syntax::Fixed,
                'G' => opts.syntax = Syntax::Basic,
                'i' | 'y' => opts.ignore_case = true,
                'v' => opts.invert = true,
                'c' => opts.count_only = true,
                'n' => opts.line_numbers = true,
                'r' => opts.recursive = true,
                'R' => {
                    opts.recursive = true;
                    opts.deref_links = true;
                }
                'w' => opts.word = true,
                'x' => opts.whole_line = true,
                'o' => opts.only_matching = true,
                'l' => opts.files_with_matches = true,
                'L' => opts.files_without_match = true,
                'H' => opts.filename = Some(true),
                'h' => opts.filename = Some(false),
                'q' => opts.quiet = true,
                's' => opts.no_messages = true,
                'Z' => opts.null_name = true,
                'z' => opts.null_data = true,
                // Accepted and ignored: this grep does not suppress output for
                // input it thinks is binary, so there is nothing for `-a` to
                // turn off. Refusing it would break callers that pass it
                // defensively, and they are asking for what we already do.
                'a' => {}
                'e' => patterns.push(take_arg('e')?.into_bytes()),
                'f' => pattern_files.push(take_arg('f')?),
                'm' => {
                    let v = take_arg('m')?;
                    let n = v
                        .parse::<usize>()
                        .map_err(|_| format!("invalid max count: {v}"))?;
                    opts.max_count = Some(n);
                }
                other => return Err(format!("unknown option: -{other}")),
            }
        }
    }

    // The first operand is the pattern only when no `-e`/`-f` supplied one;
    // with them, every operand is a file. This is what makes `grep -e -v file`
    // search for the text `-v`.
    let mut files = positional;
    if patterns.is_empty() && pattern_files.is_empty() {
        if files.is_empty() {
            return Err("missing PATTERN".to_string());
        }
        patterns.push(files.remove(0).into_bytes());
    }

    if files.is_empty() {
        // Recursion with no operand walks the working directory, as GNU does;
        // without it there is nothing to walk and the input is stdin.
        files.push(if opts.recursive {
            ".".to_string()
        } else {
            "-".to_string()
        });
    }

    Ok(GrepArgs {
        opts,
        patterns,
        pattern_files,
        files,
    })
}

/// One `--long-option`, with or without an `=value`.
fn parse_long(
    long: &str,
    opts: &mut Options,
    patterns: &mut Vec<Vec<u8>>,
    pattern_files: &mut Vec<String>,
) -> Result<(), String> {
    let (name, value) = match long.split_once('=') {
        Some((n, v)) => (n, Some(v)),
        None => (long, None),
    };
    // An option that needs a value and was not given one, reported by name
    // rather than silently treated as absent.
    let need = |v: Option<&str>| -> Result<String, String> {
        v.map(str::to_string)
            .ok_or_else(|| format!("option '--{name}' requires an argument"))
    };
    match name {
        "extended-regexp" => opts.syntax = Syntax::Extended,
        "fixed-strings" => opts.syntax = Syntax::Fixed,
        "basic-regexp" => opts.syntax = Syntax::Basic,
        "ignore-case" => opts.ignore_case = true,
        "invert-match" => opts.invert = true,
        "count" => opts.count_only = true,
        "line-number" => opts.line_numbers = true,
        "recursive" => opts.recursive = true,
        "dereference-recursive" => {
            opts.recursive = true;
            opts.deref_links = true;
        }
        "word-regexp" => opts.word = true,
        "line-regexp" => opts.whole_line = true,
        "only-matching" => opts.only_matching = true,
        "files-with-matches" => opts.files_with_matches = true,
        "files-without-match" => opts.files_without_match = true,
        "with-filename" => opts.filename = Some(true),
        "no-filename" => opts.filename = Some(false),
        "quiet" | "silent" => opts.quiet = true,
        "no-messages" => opts.no_messages = true,
        "null" => opts.null_name = true,
        "null-data" => opts.null_data = true,
        "text" | "binary-files" => {}
        "regexp" => patterns.push(need(value)?.into_bytes()),
        "file" => pattern_files.push(need(value)?),
        "max-count" => {
            let v = need(value)?;
            let n = v
                .parse::<usize>()
                .map_err(|_| format!("invalid max count: {v}"))?;
            opts.max_count = Some(n);
        }
        other => return Err(format!("unknown option: --{other}")),
    }
    Ok(())
}

/// The patterns held in the text of a `-f` file, or of a `-e` argument.
///
/// Each line is its own pattern. A trailing newline ends the last pattern
/// rather than starting an empty one — the distinction matters, because an
/// empty pattern matches every line, so getting it wrong turns
/// `grep -f list.txt` into `cat`.
fn split_patterns(raw: &[u8]) -> Vec<Vec<u8>> {
    let body = raw.strip_suffix(b"\n").unwrap_or(raw);
    if body.is_empty() {
        return vec![Vec::new()];
    }
    body.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect()
}

/// Quote a literal so the regex engine matches it as text (`-F`).
///
/// Escaping is done against the *Extended* metacharacters, which is the
/// superset: a byte that is special in BRE but not ERE is escaped anyway, and
/// `\+` is `+` in both.
fn quote_ere(literal: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(literal.len());
    for &b in literal {
        if b"\\.[]{}()*+?^$|".contains(&b) {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

/// Compile every pattern, or name the first one that will not compile.
fn compile_patterns(patterns: &[Vec<u8>], opts: &Options) -> Result<Vec<Pat>, String> {
    let mut out = Vec::with_capacity(patterns.len());
    for p in patterns {
        if p.is_empty() {
            out.push(Pat::Empty);
            continue;
        }
        let compiled = match opts.syntax {
            Syntax::Basic => bre::compile(p, opts.ignore_case),
            // `-E` is *egrep* syntax, which is not the POSIX-extended syntax
            // the same engine gives `osh`, `find -regextype posix-extended`
            // and `awk`. The two differ on what happens to nonsense: GNU
            // `grep -E '*a'` warns and matches "a", and `grep -E 'a{b}'`
            // matches the text `a{b}`, where POSIX-extended refuses both.
            // Compiling grep's patterns in the stricter dialect would refuse
            // patterns GNU grep runs. See `ere::Syntax` for the measured table.
            Syntax::Extended => Regex::new_syntax(p, opts.ignore_case, EreSyntax::EGREP),
            // `-F` escapes every metacharacter before it gets here, so no
            // pattern reaching this arm can contain the constructs the two
            // dialects disagree about.
            Syntax::Fixed => Regex::new_flags(&quote_ere(p), opts.ignore_case),
        };
        match compiled {
            Ok(re) => out.push(Pat::Re(re)),
            Err(e) => {
                // Escaped, not lossy: a pattern is an argv token, so it is a
                // byte string and need not decode. `from_utf8_lossy` would
                // substitute U+FFFD and hand the user a message naming a
                // *different* pattern from the one they typed, which is the one
                // thing this diagnostic exists to get right. See
                // design-decisions.md §369.
                return Err(format!(
                    "{}: {}",
                    quote::escape_unprintable(p),
                    quote::escape_unprintable(&e.detail)
                ));
            }
        }
    }
    Ok(out)
}

/// Whether a byte can be part of a word, for `-w`.
///
/// ASCII letters, digits and `_`, as POSIX says — plus every byte above ASCII,
/// because in any encoding this system will see one of those is part of a
/// letter. Calling them non-word would make `grep -w foo` match the `foo` in
/// `naïvefoo`.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Whether a candidate match satisfies `-w`/`-x`. Without either it always
/// does; they are the only constraints that look outside the match.
fn accepted(span: (usize, usize), line: &[u8], opts: &Options) -> bool {
    let (start, end) = span;
    if opts.whole_line && !(start == 0 && end == line.len()) {
        return false;
    }
    if opts.word {
        let before = start
            .checked_sub(1)
            .and_then(|i| line.get(i).copied())
            .is_some_and(is_word_byte);
        let after = line.get(end).copied().is_some_and(is_word_byte);
        if before || after {
            return false;
        }
    }
    true
}

/// Where a match sits, or that the search gave up.
///
/// The third outcome is the point of the `Result`. `-v` prints every line that
/// did *not* match, and `grep -v` feeding an `xargs rm` is a real shape of
/// script; reading "I abandoned the search" as "did not match" would delete
/// files on the strength of a question grep declined to answer. Only a pattern
/// with a backreference can produce one.
type Match = Result<Option<(usize, usize)>, MatchLimit>;

/// Report an abandoned search as an I/O error, which is the channel this
/// program already uses to fail a file with a diagnostic and a non-zero status.
fn limit_err(e: MatchLimit) -> io::Error {
    io::Error::other(e.to_string())
}

/// The leftmost-longest match of any pattern at or after byte offset `from`.
///
/// Several patterns are several searches; the winner is the one that starts
/// earliest, and among those the longest — the same rule the engine applies
/// within one pattern, extended across the set so that `-e ab -e abc` behaves
/// like `abc\|ab`.
fn leftmost(pats: &[Pat], line: &[u8], from: usize) -> Match {
    let mut best: Option<(usize, usize)> = None;
    for p in pats {
        let found = match p {
            Pat::Empty => (from <= line.len()).then_some((from, from)),
            Pat::Re(re) => re.find_at(line, from)?,
        };
        if let Some((s, e)) = found {
            best = Some(match best {
                Some(b) if b.0 < s || (b.0 == s && b.1 >= e) => b,
                _ => (s, e),
            });
        }
    }
    Ok(best)
}

/// The leftmost match at or after `from` that also satisfies `-w`/`-x`.
fn next_match(pats: &[Pat], line: &[u8], from: usize, opts: &Options) -> Match {
    let mut pos = from;
    loop {
        let Some(cand) = leftmost(pats, line, pos)? else {
            return Ok(None);
        };
        if accepted(cand, line, opts) {
            return Ok(Some(cand));
        }
        // Retry one byte on rather than past the candidate: a match that *is* a
        // word can begin inside one that is not, as `-w` on `xfoo foo` shows.
        // `find_at` rounds a mid-character offset forward, so stepping by a
        // byte cannot land inside a character.
        pos = cand.0.saturating_add(1);
        if pos > line.len() {
            return Ok(None);
        }
    }
}

/// Every non-overlapping accepted match of the line, left to right — what `-o`
/// prints.
fn matches_in(
    pats: &[Pat],
    line: &[u8],
    opts: &Options,
) -> Result<Vec<(usize, usize)>, MatchLimit> {
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some((s, e)) = next_match(pats, line, pos, opts)? {
        out.push((s, e));
        // An empty match is at a position rather than over one, so the scan has
        // to step past it or it would be found here for ever.
        pos = if e > s { e } else { e.saturating_add(1) };
        if pos > line.len() {
            break;
        }
    }
    Ok(out)
}

/// Whether the line is selected, `-v` included.
fn line_selected(line: &[u8], pats: &[Pat], opts: &Options) -> Result<bool, MatchLimit> {
    let matched = next_match(pats, line, 0, opts)?.is_some();
    Ok(matched != opts.invert)
}

/// What a file is called in output. Standard input is spelled `-` on the
/// command line and *named* `(standard input)` in a diagnostic or a prefix, as
/// every other grep does — a `-:` prefix reads as part of the line.
fn display_name(path: &str) -> &str {
    if path == "-" {
        "(standard input)"
    } else {
        path
    }
}

/// Whether every printed line is prefixed with the name of the file it came
/// from.
///
/// `explicit` is `-H`/`-h`, which settle it outright. Otherwise the rule is not
/// "more than one file was searched" but **"more than one file was *named*, or
/// `-r` was pointed at a directory"** — which is a question about the operands,
/// not about what they expand to. Measured, GNU grep 3.11:
///
/// | command | prefix? | why |
/// |---|---|---|
/// | `grep -r foo dir` where `dir` holds one file | yes | an operand was a directory |
/// | `grep -r foo link-to-file` | no | one operand, and not a directory |
/// | `grep foo a b` | yes | two operands |
///
/// Counting the expansion instead got the first row wrong. It is also the only
/// formulation that can be answered *before* the walk begins, which the walk
/// now requires: it streams, so by the time the size of the expansion is known,
/// lines have already been printed with or without a prefix.
fn wants_filename(explicit: Option<bool>, operands: usize, named_a_directory: bool) -> bool {
    explicit.unwrap_or(operands > 1 || named_a_directory)
}

/// The prefix shown before a printed line: file name, line number, both or
/// neither.
///
/// Bytes rather than `String` because `-Z` puts a NUL after the name, and
/// because the name itself is a path — which on this system may hold any byte
/// but `/` and NUL. Only the *name's* separator changes under `-Z`; the one
/// after a line number stays `:`, which is what GNU does and is what keeps
/// `-nZ` output parseable at all.
fn line_prefix(
    filename: &str,
    line_idx_zero_based: usize,
    show_filename: bool,
    opts: &Options,
) -> Vec<u8> {
    let mut prefix = Vec::new();
    if show_filename {
        prefix.extend_from_slice(filename.as_bytes());
        prefix.push(opts.name_sep());
    }
    if opts.line_numbers {
        // Zero-based internally, one-based on the way out.
        prefix.extend_from_slice(line_idx_zero_based.saturating_add(1).to_string().as_bytes());
        prefix.push(b':');
    }
    prefix
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 2)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            diag!("grep: {e}");
            return ExitCode::from(2);
        }
    };

    let mut patterns = parsed.patterns;
    for pf in &parsed.pattern_files {
        let raw = if pf == "-" {
            let mut buf = Vec::new();
            io::stdin().read_to_end(&mut buf).map(|_| buf)
        } else {
            fs::read(pf)
        };
        match raw {
            Ok(raw) => patterns.extend(split_patterns(&raw)),
            Err(e) => {
                diag!("grep: {}: {}", quotef_os(pf), strerror(&e));
                return ExitCode::from(2);
            }
        }
    }

    let pats = match compile_patterns(&patterns, &parsed.opts) {
        Ok(p) => p,
        Err(e) => {
            diag!("grep: {e}");
            return ExitCode::from(2);
        }
    };

    let named_a_directory = parsed.opts.recursive
        && parsed.files.iter().any(|f| {
            // `is_dir` follows a symlink, matching what `Run::operand` does
            // with the same operand a moment later.
            Path::new(f).is_dir()
        });
    let show_filename = wants_filename(parsed.opts.filename, parsed.files.len(), named_a_directory);

    let mut run = Run {
        // `Stream`, not `io::stdout().lock()`, because the verdict on the
        // output is part of the answer: `grep a file >&-` wrote into a buffer
        // that was discarded by a final `let _ = out.flush()` and exited 0
        // having printed nothing. `Stream` records the failure instead of
        // returning it — so no write below can fail — and `close_stdout_with`
        // reports it as `grep: write error: …` and exits 2, as GNU does.
        out: stdfd::Stream::stdout(),
        pats: &pats,
        opts: &parsed.opts,
        show_filename,
        any_match: false,
        had_error: false,
        done: false,
    };

    for f in &parsed.files {
        run.operand(f);
        if run.done {
            break;
        }
    }

    let earned = if run.any_match && parsed.opts.quiet {
        // `-q` asks one question — "is there a match?" — and an error reading
        // some *other* file does not unanswer it. POSIX says so outright
        // ("exit with zero status if an input line is selected, even if an
        // error was detected"), and it is measurable: `grep -q foo nonexistent
        // words` prints the diagnostic and still exits 0, where the same
        // command without `-q` exits 2.
        ExitCode::SUCCESS
    } else if run.had_error {
        // Otherwise an error outranks both answers: a script that
        // distinguishes 0 from 1 is asking about the content of files it
        // believes were all read.
        ExitCode::from(2)
    } else if run.any_match {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    };
    // grep's `exit_failure` is 2, not the family's 1.
    stdfd::close_stdout_with("grep", run.out, earned, 2)
}

/// Everything one run carries from file to file.
///
/// It is a struct rather than a handful of locals because the recursive walk
/// has to **stream**: GNU searches each file at the moment it reaches it, so
/// the walk and the searching are one traversal and not two phases. Collecting
/// the tree into a `Vec` first — which is what this did until 2026-08-25 — got
/// two observable things wrong:
///
/// * `grep -rq foo dir` stops at the first match and never looks at what comes
///   after it, so it never emits the diagnostics those files would have
///   produced. Measured: on a tree holding a symlink loop, `grep -Rq foo L`
///   prints nothing, while `grep -Rq zzz L` prints the loop warning — the
///   difference being only how far the walk got.
/// * A diagnostic about one file belongs *between* the matches of its
///   neighbours. Expanding first put every diagnostic before every match.
struct Run<'a> {
    out: stdfd::Stream,
    pats: &'a [Pat],
    opts: &'a Options,
    show_filename: bool,
    any_match: bool,
    had_error: bool,
    /// Set once `-q` has its answer. Every remaining file and directory is
    /// then skipped — including the diagnostics they would have produced,
    /// which is the point.
    done: bool,
}

impl Run<'_> {
    /// Handle one command-line operand.
    fn operand(&mut self, f: &str) {
        // `is_dir`, which follows a symlink — deliberately, and unlike the
        // walk. A link named on the command line is followed by `-r` as well
        // as `-R`; it is only a link *discovered* by the walk that `-r` skips.
        if self.opts.recursive && Path::new(f).is_dir() {
            let mut ancestors: Vec<PathBuf> = Vec::new();
            self.walk(Path::new(f), &mut ancestors);
        } else {
            self.search(f);
        }
    }

    /// Search one named file — or standard input, spelled `-`.
    fn search(&mut self, path: &str) {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            if Path::new(path).is_dir() {
                // Only reachable without `-r`; with it, `operand` walked the
                // directory instead of arriving here.
                if !self.opts.no_messages {
                    diag!("grep: {}: Is a directory", quotef_os(path));
                }
                // Named but not searched, so the run's answer is about less
                // than it was asked about — status 2, as for a file that could
                // not be opened.
                self.had_error = true;
                return;
            }
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    if !self.opts.no_messages {
                        diag!("grep: {}: {}", quotef_os(path), strerror(&e));
                    }
                    // A file that could not be read is an error, not an absence
                    // of matches: exiting 1 would tell a script the file has
                    // been searched and found wanting.
                    self.had_error = true;
                    return;
                }
            }
        };

        let shown = display_name(path);
        match search_stream(
            &mut self.out,
            reader,
            self.pats,
            shown,
            self.show_filename,
            self.opts,
        ) {
            Ok(matched) => {
                if matched {
                    self.any_match = true;
                    if self.opts.quiet {
                        // `-q` is a question, and it has been answered.
                        self.done = true;
                        return;
                    }
                }
                // `-l` and `-L` name the file rather than the lines; which of
                // the two asked decides which answer is worth naming.
                let name_it = (self.opts.files_with_matches && matched)
                    || (self.opts.files_without_match && !matched);
                if name_it {
                    // NUL after the name under `-Z`, newline otherwise — and
                    // *not* `-z`'s separator, which describes the input. This
                    // is the half of `-Z` that matters: `grep -rlZ | xargs -0`
                    // is the only listing of paths that survives a path
                    // containing a newline, which this system permits.
                    let _ = self.out.write_all(shown.as_bytes());
                    let _ = self.out.write_all(if self.opts.null_name {
                        &b"\0"[..]
                    } else {
                        &b"\n"[..]
                    });
                }
            }
            Err(e) => {
                if !self.opts.no_messages {
                    diag!("grep: {}: {}", quotef_os(path), strerror(&e));
                }
                self.had_error = true;
            }
        }
    }

    /// Search a file the walk found, rather than one the command line named.
    ///
    /// `to_string_lossy` is a known defect and not a choice: every path in this
    /// program is a `String`, so a path holding a byte that is not valid UTF-8
    /// — which this system permits in a filename — is corrupted on the way in
    /// and printed back wrong. It is one entry in the argv-UTF-8 backlog
    /// (`known-issues.md`); fixing it means moving the whole program onto
    /// `OsString`, which is a change to `search_stream`'s signature and to
    /// every prefix it builds. Isolated into a named method so that the fix is
    /// a change to one line, and so that the defect is not silently spelled
    /// inline in the middle of the walk.
    fn search_found(&mut self, path: &Path) {
        let shown = path.to_string_lossy().into_owned();
        self.search(&shown);
    }

    /// Walk one directory, searching what it holds, deepest-last and in sorted
    /// order.
    ///
    /// # Which symlinks are followed
    ///
    /// A symlink met *during* the walk is skipped by `-r` and followed by `-R`
    /// — that single difference is the whole of what separates the two flags.
    /// A symlink *named on the command line* is followed by both, which is why
    /// [`Run::operand`] asks `is_dir` (following) where this asks
    /// `symlink_metadata` (not following).
    ///
    /// # The loop
    ///
    /// `-R` on a tree containing a link back to one of its own ancestors
    /// describes an infinite tree. GNU prints
    /// `grep: PATH: warning: recursive directory loop` and carries on.
    /// Measured, and all three halves of that matter: the message is
    /// suppressed by `-s`, it does **not** change the exit status (`grep -R zzz
    /// loop-tree` exits 1, not 2), and it names the link rather than what the
    /// link resolves to.
    ///
    /// # Ordering
    ///
    /// Sorted, where GNU emits readdir order. See `design-decisions.md` §380:
    /// the order a directory hands back its entries is a property of the
    /// filesystem, so GNU's output is not reproducible between two machines
    /// holding the same files, whereas a sorted listing is stable and
    /// diffable.
    fn walk(&mut self, dir: &Path, ancestors: &mut Vec<PathBuf>) {
        if self.done {
            return;
        }
        if self.opts.deref_links {
            // Identity by resolved path: two names for one directory are one
            // directory, and it is the resolved form that says so. A failure
            // to resolve is not fatal here — the `read_dir` below will report
            // it properly — so the unresolvable path stands in for itself.
            let real = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
            if ancestors.contains(&real) {
                if !self.opts.no_messages {
                    diag!(
                        "grep: {}: warning: recursive directory loop",
                        quotef_os(dir)
                    );
                }
                return;
            }
            ancestors.push(real);
        }

        self.walk_entries(dir, ancestors);

        if self.opts.deref_links {
            ancestors.pop();
        }
    }

    /// The body of [`Run::walk`], split out so that the ancestor pushed by the
    /// caller is popped on every path out of it.
    fn walk_entries(&mut self, dir: &Path, ancestors: &mut Vec<PathBuf>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                if !self.opts.no_messages {
                    diag!("grep: {}: {}", quotef_os(dir), strerror(&e));
                }
                self.had_error = true;
                return;
            }
        };

        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) => paths.push(e.path()),
                Err(e) => {
                    if !self.opts.no_messages {
                        diag!("grep: {}: {}", quotef_os(dir), strerror(&e));
                    }
                    self.had_error = true;
                }
            }
        }
        paths.sort();

        for path in paths {
            if self.done {
                return;
            }
            // `symlink_metadata`, not `metadata`: the question here is what the
            // entry *is*, not what it points at.
            let md = match fs::symlink_metadata(&path) {
                Ok(md) => md,
                Err(e) => {
                    if !self.opts.no_messages {
                        diag!("grep: {}: {}", quotef_os(&path), strerror(&e));
                    }
                    self.had_error = true;
                    continue;
                }
            };

            if md.file_type().is_symlink() {
                if !self.opts.deref_links {
                    // `-r` passes over it in silence — including a dangling
                    // one, which it never asks about and so never reports.
                    continue;
                }
                match fs::metadata(&path) {
                    Ok(target) if target.is_dir() => self.walk(&path, ancestors),
                    Ok(_) => self.search_found(&path),
                    Err(e) => {
                        if !self.opts.no_messages {
                            diag!("grep: {}: {}", quotef_os(&path), strerror(&e));
                        }
                        self.had_error = true;
                    }
                }
                continue;
            }

            if md.is_dir() {
                self.walk(&path, ancestors);
            } else {
                self.search_found(&path);
            }
        }
    }
}

/// Search one stream, printing what the options ask for. Returns whether any
/// line was selected.
fn search_stream(
    out: &mut impl Write,
    reader: impl Read,
    pats: &[Pat],
    filename: &str,
    show_filename: bool,
    opts: &Options,
) -> io::Result<bool> {
    // `-m 0` is not "no limit", and it is not "stop after the first" either:
    // GNU prints nothing at all — not even the `-c` count line, which is the
    // surprising half — and reports the file as not matching. Answering it
    // before opening a line is also the only way to get that, since the count
    // line below is printed unconditionally.
    if opts.max_count == Some(0) {
        return Ok(false);
    }

    let mut buf = BufReader::new(reader);
    // Printing nothing means the first selected line settles it, and reading
    // the rest of a file is work whose result is discarded — which for `-q` on
    // a pipe is also the difference between returning and waiting.
    let stop_at_first = opts.quiet || opts.files_with_matches || opts.files_without_match;
    let sep = opts.line_sep();
    let mut match_count: usize = 0;
    let mut line_idx: usize = 0;
    let mut line: Vec<u8> = Vec::new();

    loop {
        line.clear();
        // Lines are read as bytes: a file this system can name may hold any
        // byte but `/` and NUL, and `String`-typed input could not carry one.
        if buf.read_until(sep, &mut line)? == 0 {
            break;
        }
        // The separator is not part of the line, and a final line without one
        // is still a line.
        let body = line.strip_suffix(&[sep][..]).unwrap_or(&line);

        if line_selected(body, pats, opts).map_err(limit_err)? {
            match_count = match_count.saturating_add(1);
            if stop_at_first {
                return Ok(true);
            }
            if !opts.count_only {
                let prefix = line_prefix(filename, line_idx, show_filename, opts);
                if opts.only_matching {
                    // `-o` with `-v` prints nothing: the part of the line that
                    // did not match is the whole line, and GNU declines to call
                    // that a match.
                    if !opts.invert {
                        for (s, e) in matches_in(pats, body, opts).map_err(limit_err)? {
                            // An *empty* match is skipped rather than printed.
                            // `grep -o 'o*'` on "foo bar" prints one `oo`, not
                            // an `oo` surrounded by six blank lines: a pattern
                            // that can match nothing matches at every position,
                            // so printing those would make `-o` unusable for
                            // exactly the patterns people write it for. The
                            // *line* still counts as selected, which is why
                            // this loop can legitimately print nothing.
                            if e > s {
                                out.write_all(&prefix)?;
                                out.write_all(body.get(s..e).unwrap_or_default())?;
                                out.write_all(&[sep])?;
                            }
                        }
                    }
                } else {
                    out.write_all(&prefix)?;
                    out.write_all(body)?;
                    out.write_all(&[sep])?;
                }
            }
            if opts.max_count.is_some_and(|m| match_count >= m) {
                break;
            }
        }
        line_idx = line_idx.saturating_add(1);
    }

    if opts.count_only {
        if show_filename {
            out.write_all(filename.as_bytes())?;
            out.write_all(&[opts.name_sep()])?;
        }
        out.write_all(match_count.to_string().as_bytes())?;
        // A newline, not `sep`: `-z` says what a *line of input* is, and a
        // count is not one. Measured — `grep -zHc` ends its count with `\n`
        // even though every matched line it would otherwise print ends with
        // NUL.
        out.write_all(b"\n")?;
    }

    Ok(match_count > 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    /// Compile one pattern under `opts` — most cases have exactly one.
    fn pats(pattern: &str, opts: &Options) -> Vec<Pat> {
        compile_patterns(&[pattern.as_bytes().to_vec()], opts).unwrap()
    }

    /// The `unwrap` is the assertion: a test pattern that exhausted the
    /// backtracking budget would be a bug in the budget, not in the test.
    fn selects(line: &str, pattern: &str, opts: &Options) -> bool {
        line_selected(line.as_bytes(), &pats(pattern, opts), opts).unwrap()
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_empty_errors() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("missing PATTERN"));
    }

    #[test]
    fn parse_pattern_only_reads_stdin() {
        let a = parse_args(&s(&["foo"])).unwrap();
        assert_eq!(a.patterns, vec![b"foo".to_vec()]);
        assert_eq!(a.files, vec!["-"]);
        assert_eq!(a.opts, Options::default());
    }

    #[test]
    fn parse_pattern_and_files() {
        let a = parse_args(&s(&["foo", "a.txt", "b.txt"])).unwrap();
        assert_eq!(a.patterns, vec![b"foo".to_vec()]);
        assert_eq!(a.files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn parse_clustered_flags() {
        let a = parse_args(&s(&["-ivcnr", "foo"])).unwrap();
        assert!(a.opts.ignore_case);
        assert!(a.opts.invert);
        assert!(a.opts.count_only);
        assert!(a.opts.line_numbers);
        assert!(a.opts.recursive);
    }

    #[test]
    fn parse_unknown_flag_errors() {
        // `-Q` rather than `-Z`, which this test used until `-Z` was
        // implemented: an "unknown option" test has to name a letter grep does
        // not have, and every such test is one feature away from asserting the
        // wrong thing.
        let err = parse_args(&s(&["-Q", "foo"])).unwrap_err();
        assert!(err.contains("unknown option"), "{err}");
        assert!(err.contains('Q'), "{err}");
        let err = parse_args(&s(&["--zzz", "foo"])).unwrap_err();
        assert!(err.contains("unknown option"), "{err}");
    }

    #[test]
    fn parse_the_two_null_flags_are_different_flags() {
        let a = parse_args(&s(&["-Z", "foo"])).unwrap();
        assert!(a.opts.null_name && !a.opts.null_data);
        let a = parse_args(&s(&["-z", "foo"])).unwrap();
        assert!(a.opts.null_data && !a.opts.null_name);
        let a = parse_args(&s(&["--null", "--null-data", "foo"])).unwrap();
        assert!(a.opts.null_name && a.opts.null_data);
    }

    #[test]
    fn parse_bare_dash_is_a_filename() {
        let a = parse_args(&s(&["foo", "-"])).unwrap();
        assert_eq!(a.files, vec!["-"]);
    }

    #[test]
    fn parse_flag_after_pattern_still_a_flag() {
        // Order-insensitive, as GNU is by default: options may follow operands.
        let a = parse_args(&s(&["foo", "-v", "x.txt"])).unwrap();
        assert!(a.opts.invert);
        assert_eq!(a.patterns, vec![b"foo".to_vec()]);
        assert_eq!(a.files, vec!["x.txt"]);
    }

    #[test]
    fn parse_the_options_that_used_to_be_unknown() {
        // Every one of these was `unknown option` until this rewrite, and each
        // is in the failure lane C reported: `grep -E`, `grep -q`, `grep -c --`.
        assert_eq!(
            parse_args(&s(&["-E", "a+"])).unwrap().opts.syntax,
            Syntax::Extended
        );
        assert!(parse_args(&s(&["-q", "a"])).unwrap().opts.quiet);
        assert_eq!(
            parse_args(&s(&["-F", "a"])).unwrap().opts.syntax,
            Syntax::Fixed
        );
        for flag in ["-w", "-x", "-o", "-l", "-L", "-H", "-h", "-s", "-a"] {
            assert!(parse_args(&s(&[flag, "a"])).is_ok(), "{flag} rejected");
        }
    }

    #[test]
    fn parse_double_dash_ends_the_options() {
        let a = parse_args(&s(&["-c", "--", "-v", "x.txt"])).unwrap();
        assert!(a.opts.count_only);
        assert!(!a.opts.invert, "-v after -- is the pattern, not an option");
        assert_eq!(a.patterns, vec![b"-v".to_vec()]);
        assert_eq!(a.files, vec!["x.txt"]);
    }

    #[test]
    fn parse_e_makes_every_operand_a_file() {
        let a = parse_args(&s(&["-e", "-v", "x.txt"])).unwrap();
        assert!(!a.opts.invert);
        assert_eq!(a.patterns, vec![b"-v".to_vec()]);
        assert_eq!(a.files, vec!["x.txt"]);
    }

    #[test]
    fn parse_repeated_e_collects_patterns() {
        let a = parse_args(&s(&["-e", "foo", "-e", "bar"])).unwrap();
        assert_eq!(a.patterns, vec![b"foo".to_vec(), b"bar".to_vec()]);
    }

    #[test]
    fn parse_an_option_argument_may_be_glued_to_its_cluster() {
        assert_eq!(
            parse_args(&s(&["-m5", "a"])).unwrap().opts.max_count,
            Some(5)
        );
        assert_eq!(
            parse_args(&s(&["-m", "5", "a"])).unwrap().opts.max_count,
            Some(5)
        );
        assert_eq!(
            parse_args(&s(&["-im5", "a"])).unwrap().opts.max_count,
            Some(5)
        );
        assert_eq!(
            parse_args(&s(&["-efoo"])).unwrap().patterns,
            vec![b"foo".to_vec()]
        );
    }

    #[test]
    fn parse_a_missing_option_argument_is_an_error() {
        assert!(
            parse_args(&s(&["-e"]))
                .unwrap_err()
                .contains("requires an argument")
        );
        assert!(
            parse_args(&s(&["-m"]))
                .unwrap_err()
                .contains("requires an argument")
        );
        assert!(
            parse_args(&s(&["-m", "x", "a"]))
                .unwrap_err()
                .contains("invalid max count")
        );
    }

    #[test]
    fn parse_long_options() {
        let a = parse_args(&s(&["--ignore-case", "--max-count=2", "--regexp=foo"])).unwrap();
        assert!(a.opts.ignore_case);
        assert_eq!(a.opts.max_count, Some(2));
        assert_eq!(a.patterns, vec![b"foo".to_vec()]);
        assert!(
            parse_args(&s(&["--nope", "a"]))
                .unwrap_err()
                .contains("unknown option")
        );
    }

    #[test]
    fn parse_recursive_with_no_operand_walks_here() {
        assert_eq!(parse_args(&s(&["-r", "foo"])).unwrap().files, vec!["."]);
    }

    /// `-r` and `-R` are not synonyms, and the whole of the difference is one
    /// flag: whether a symbolic link *found during the walk* is followed.
    /// Treating them as synonyms made `-r` report files GNU does not, and made
    /// it run forever on a tree containing a link back to one of its own
    /// ancestors.
    #[test]
    fn parse_capital_r_is_the_dereferencing_one() {
        let lower = parse_args(&s(&["-r", "foo"])).unwrap().opts;
        assert!(lower.recursive);
        assert!(!lower.deref_links);

        let upper = parse_args(&s(&["-R", "foo"])).unwrap().opts;
        assert!(upper.recursive);
        assert!(upper.deref_links);

        // The long spellings say the same thing at more length, and
        // `--dereference-recursive` implies `--recursive` rather than needing
        // it alongside.
        let long = parse_args(&s(&["--recursive", "foo"])).unwrap().opts;
        assert!(long.recursive);
        assert!(!long.deref_links);

        let long_deref = parse_args(&s(&["--dereference-recursive", "foo"]))
            .unwrap()
            .opts;
        assert!(long_deref.recursive);
        assert!(long_deref.deref_links);
    }

    // ---------------- patterns are patterns ----------------
    //
    // The eight oils tests lane C reported were failing all landed here: what
    // they asked of `grep` was a regular expression, and what they got was a
    // substring search that answered "no match".

    #[test]
    fn a_bracket_expression_is_a_set_not_three_characters() {
        let o = Options::default();
        assert!(selects("declare -r a=\"1\"", " [ab]=", &o));
        assert!(selects("declare -r b=\"2\"", " [ab]=", &o));
        assert!(!selects("declare -r c=\"3\"", " [ab]=", &o));
        // …and the old behaviour is now the one that fails.
        assert!(!selects("literal [ab]= here", " [ab]=", &o));
    }

    #[test]
    fn an_anchor_anchors() {
        let o = Options::default();
        assert!(selects("posix on", "^posix", &o));
        assert!(!selects("set -o posix", "^posix", &o));
        assert!(selects("declare -a FUNCNAME", "^declare -a FUNCNAME$", &o));
        assert!(!selects(
            "declare -a FUNCNAMEX",
            "^declare -a FUNCNAME$",
            &o
        ));
    }

    #[test]
    fn basic_and_extended_disagree_about_plus_and_that_is_the_point() {
        // BRE: `a+b` is three literal characters. ERE: one or more `a`, then
        // `b`. Not a subset relation, which is why `-E` cannot be a no-op.
        let bre = Options::default();
        let ere = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        assert!(selects("a+b", "a+b", &bre));
        assert!(!selects("aab", "a+b", &bre));
        assert!(!selects("a+b", "a+b", &ere));
        assert!(selects("aab", "a+b", &ere));
        // Groups swap in the same way.
        assert!(selects("xayb", "\\(a\\|b\\)", &bre));
        assert!(selects("xayb", "(a|b)", &ere));
    }

    #[test]
    fn fixed_strings_have_no_metacharacters() {
        let f = Options {
            syntax: Syntax::Fixed,
            ..Options::default()
        };
        assert!(selects("a.c", "a.c", &f));
        assert!(!selects("abc", "a.c", &f));
        assert!(selects("cost: $5*", "$5*", &f));
    }

    #[test]
    fn case_folding_reaches_the_pattern_not_only_the_line() {
        let o = Options {
            ignore_case: true,
            ..Options::default()
        };
        assert!(selects("Hello World", "hello", &o));
        assert!(selects("HELLO", "^hel", &o));
        assert!(selects("ABC", "[a-c]*$", &o));
    }

    #[test]
    fn a_malformed_pattern_is_reported_rather_than_matched_literally() {
        let o = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        let err = compile_patterns(&[b"a[".to_vec()], &o).err().unwrap();
        assert!(err.contains("a["), "{err}");
        // A reference to a group the pattern does not have is a compile error,
        // not a literal digit.
        let err = compile_patterns(&[b"\\(a\\)\\2".to_vec()], &Options::default())
            .err()
            .unwrap();
        assert!(err.contains("backreference"), "{err}");
    }

    #[test]
    fn a_backreference_selects_a_line_that_repeats_itself() {
        let o = Options::default();
        assert!(selects("abcabc", "\\(abc\\)\\1", &o));
        assert!(!selects("abcdef", "\\(abc\\)\\1", &o));
        // The same pattern in ERE syntax, where the parentheses are bare.
        let e = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        assert!(selects("xyxy", "(xy)\\1", &e));
        assert!(!selects("xyzy", "(xy)\\1", &e));
    }

    #[test]
    fn a_pathological_backreference_gives_up_rather_than_hanging() {
        // Backreference matching is NP-hard, so the engine spends a budget and
        // then declines to answer. `line_selected` must report that as an error
        // and not as "did not match" — `-v` would otherwise print the line.
        let o = Options::default();
        let pats = compile_patterns(
            &[b"\\(a*\\)\\(a*\\)\\(a*\\)\\(a*\\)\\(a*\\)\\1\\2\\3\\4\\5b".to_vec()],
            &o,
        )
        .unwrap();
        let line = vec![b'a'; 300];
        assert!(line_selected(&line, &pats, &o).is_err());
    }

    #[test]
    fn the_empty_pattern_matches_every_line() {
        // The engine rejects an empty pattern; `grep ''` is not an error.
        let o = Options::default();
        assert!(selects("anything", "", &o));
        assert!(selects("", "", &o));
    }

    #[test]
    fn several_patterns_are_matched_as_one_set() {
        let o = Options::default();
        let p = compile_patterns(&[b"foo".to_vec(), b"^bar".to_vec()], &o).unwrap();
        assert!(line_selected(b"a foo b", &p, &o).unwrap());
        assert!(line_selected(b"bar b", &p, &o).unwrap());
        assert!(!line_selected(b"a bar", &p, &o).unwrap());
    }

    // ---------------- -w / -x / -o ----------------

    #[test]
    fn word_regexp_needs_non_word_neighbours() {
        let o = Options {
            word: true,
            ..Options::default()
        };
        assert!(selects("foo", "foo", &o));
        assert!(selects("a foo b", "foo", &o));
        assert!(selects("(foo)", "foo", &o));
        assert!(!selects("foobar", "foo", &o));
        assert!(!selects("barfoo", "foo", &o));
        assert!(!selects("foo_bar", "foo", &o));
        // A match that is a word may begin inside one that is not, so a
        // rejected candidate must not end the search for the line.
        assert!(selects("xfoo foo", "foo", &o));
    }

    #[test]
    fn line_regexp_needs_the_whole_line() {
        let o = Options {
            whole_line: true,
            ..Options::default()
        };
        assert!(selects("foo", "foo", &o));
        assert!(!selects("foo ", "foo", &o));
        assert!(selects("foo bar", "foo.*", &o));
    }

    #[test]
    fn only_matching_reports_the_parts_not_the_line() {
        let o = Options {
            syntax: Syntax::Extended,
            only_matching: true,
            ..Options::default()
        };
        let p = pats("[0-9]+", &o);
        let spans = matches_in(&p, b"ab12cd345", &o).unwrap();
        assert_eq!(spans, vec![(2, 4), (6, 9)]);
    }

    #[test]
    fn the_longest_alternative_wins_as_posix_requires() {
        // Leftmost-*longest*, not leftmost-first: `-o` is where the difference
        // becomes visible output rather than an internal detail.
        let o = Options {
            syntax: Syntax::Extended,
            ..Options::default()
        };
        let p = pats("a|ab", &o);
        assert_eq!(matches_in(&p, b"ab", &o).unwrap(), vec![(0, 2)]);
        // …and across `-e` patterns, which are a set and not an order.
        let p = compile_patterns(&[b"a".to_vec(), b"ab".to_vec()], &o).unwrap();
        assert_eq!(matches_in(&p, b"ab", &o).unwrap(), vec![(0, 2)]);
    }

    // ---------------- lines are bytes ----------------

    #[test]
    fn a_line_that_is_not_utf8_is_still_a_line() {
        let o = Options::default();
        let p = pats("b", &o);
        // 0xFF begins no valid UTF-8 sequence. A `String`-typed pipeline would
        // have replaced it or refused the file.
        let (out, matched) = run_search(&[b'a', 0xFF, b'b', b'\n'], &p, &o, "f", false);
        assert!(matched);
        assert_eq!(out, vec![b'a', 0xFF, b'b', b'\n']);
    }

    #[test]
    fn a_nul_in_the_input_is_data_like_any_other_byte() {
        let o = Options::default();
        let p = pats("x", &o);
        let (out, _) = run_search(b"a\0x\n", &p, &o, "f", false);
        assert_eq!(out, b"a\0x\n");
    }

    #[test]
    fn a_final_line_without_a_newline_is_still_searched() {
        let o = Options::default();
        let p = pats("b", &o);
        let (out, matched) = run_search(b"a\nb", &p, &o, "f", false);
        assert!(matched);
        assert_eq!(out, b"b\n", "and it is printed with one");
    }

    // ---------------- line_prefix ----------------

    /// `line_prefix` reads two fields off `Options`; these name them without
    /// making every assertion below construct a whole struct.
    fn pfx_opts(line_numbers: bool, null_name: bool) -> Options {
        Options {
            line_numbers,
            null_name,
            ..Options::default()
        }
    }

    #[test]
    fn standard_input_has_a_name_of_its_own() {
        assert_eq!(display_name("-"), "(standard input)");
        assert_eq!(display_name("a.txt"), "a.txt");
        // `grep -H pattern -` printing `-:line` reads as part of the line.
        assert_eq!(
            line_prefix(display_name("-"), 0, true, &pfx_opts(false, false)),
            b"(standard input):"
        );
    }

    /// The prefix rule is about the operands, not the expansion: one directory
    /// operand prefixes even when it holds a single file, and one symlink-to-a-
    /// file operand does not prefix at all. Ours counted the expansion, so
    /// `grep -r foo dir-with-one-file` printed a bare line where GNU prints
    /// `dir/file:line`.
    #[test]
    fn a_directory_operand_earns_the_prefix_however_little_is_in_it() {
        assert!(wants_filename(None, 1, true));
        assert!(!wants_filename(None, 1, false));
        assert!(wants_filename(None, 2, false));
        assert!(!wants_filename(None, 0, false));
        // -H and -h outrank the count either way.
        assert!(wants_filename(Some(true), 1, false));
        assert!(!wants_filename(Some(false), 9, true));
    }

    #[test]
    fn prefix_none() {
        assert_eq!(line_prefix("f", 0, false, &pfx_opts(false, false)), b"");
    }

    #[test]
    fn prefix_filename_only() {
        assert_eq!(
            line_prefix("a.txt", 0, true, &pfx_opts(false, false)),
            b"a.txt:"
        );
    }

    #[test]
    fn prefix_line_number_only() {
        assert_eq!(
            line_prefix("ignored", 0, false, &pfx_opts(true, false)),
            b"1:"
        );
        assert_eq!(
            line_prefix("ignored", 41, false, &pfx_opts(true, false)),
            b"42:"
        );
    }

    #[test]
    fn prefix_filename_and_line_number() {
        assert_eq!(
            line_prefix("a.txt", 9, true, &pfx_opts(true, false)),
            b"a.txt:10:"
        );
    }

    #[test]
    fn null_ends_the_name_but_never_the_line_number() {
        // `-Z` is about the byte that follows a *file name*. Applying it to the
        // line number too would make `-nZ` output unparseable, and it is not
        // what GNU does.
        assert_eq!(
            line_prefix("a.txt", 0, true, &pfx_opts(false, true)),
            b"a.txt\0"
        );
        assert_eq!(
            line_prefix("a.txt", 9, true, &pfx_opts(true, true)),
            b"a.txt\x0010:"
        );
    }

    // ---------------- search_stream ----------------

    fn run_search(
        input: &[u8],
        pats: &[Pat],
        opts: &Options,
        filename: &str,
        show_filename: bool,
    ) -> (Vec<u8>, bool) {
        let mut out: Vec<u8> = Vec::new();
        let matched = search_stream(&mut out, input, pats, filename, show_filename, opts).unwrap();
        (out, matched)
    }

    fn run(
        input: &[u8],
        pattern: &str,
        opts: Options,
        filename: &str,
        show_filename: bool,
    ) -> (String, bool) {
        let p = pats(pattern, &opts);
        let (out, matched) = run_search(input, &p, &opts, filename, show_filename);
        (String::from_utf8(out).unwrap(), matched)
    }

    #[test]
    fn search_basic_match() {
        let (out, matched) = run(b"foo\nbar\nfoobar\n", "foo", Options::default(), "f", false);
        assert!(matched);
        assert_eq!(out, "foo\nfoobar\n");
    }

    #[test]
    fn search_no_match_returns_false() {
        let (out, matched) = run(b"abc\ndef\n", "xyz", Options::default(), "f", false);
        assert!(!matched);
        assert_eq!(out, "");
    }

    #[test]
    fn search_count_only() {
        let opts = Options {
            count_only: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\nab\nabc\n", "a", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "3\n");
    }

    #[test]
    fn search_count_only_with_filename() {
        let opts = Options {
            count_only: true,
            ..Options::default()
        };
        let (out, _) = run(b"a\nab\nabc\n", "a", opts, "x.txt", true);
        assert_eq!(out, "x.txt:3\n");
    }

    #[test]
    fn search_count_of_no_matches_is_zero_not_silence() {
        let opts = Options {
            count_only: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\nb\n", "z", opts, "f", false);
        assert!(!matched);
        assert_eq!(out, "0\n");
    }

    #[test]
    fn search_line_numbers() {
        let opts = Options {
            line_numbers: true,
            ..Options::default()
        };
        let (out, _) = run(b"x\nfoo\nbar\nfoo\n", "foo", opts, "f", false);
        assert_eq!(out, "2:foo\n4:foo\n");
    }

    #[test]
    fn search_invert() {
        let opts = Options {
            invert: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\nb\nc\n", "b", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "a\nc\n");
    }

    #[test]
    fn search_ignore_case() {
        let opts = Options {
            ignore_case: true,
            ..Options::default()
        };
        let (out, _) = run(b"FOO\nbar\nFooBar\n", "foo", opts, "f", false);
        assert_eq!(out, "FOO\nFooBar\n");
    }

    #[test]
    fn search_show_filename_prefix() {
        let (out, _) = run(b"foo\n", "foo", Options::default(), "x.txt", true);
        assert_eq!(out, "x.txt:foo\n");
    }

    #[test]
    fn search_max_count_stops_early() {
        let opts = Options {
            max_count: Some(2),
            ..Options::default()
        };
        let (out, _) = run(b"a\na\na\na\n", "a", opts, "f", false);
        assert_eq!(out, "a\na\n");
    }

    #[test]
    fn max_count_zero_is_not_no_limit_and_not_one() {
        // Measured against GNU grep 3.0: `-m 0` prints nothing and reports no
        // match, and — the part that is easy to get wrong — suppresses the
        // `-c` count line entirely rather than printing `0`.
        let opts = Options {
            max_count: Some(0),
            ..Options::default()
        };
        let (out, matched) = run(b"a\na\n", "a", opts, "f", false);
        assert_eq!(out, "");
        assert!(!matched);

        let opts = Options {
            max_count: Some(0),
            count_only: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\na\n", "a", opts, "f", true);
        assert_eq!(out, "");
        assert!(!matched);
    }

    #[test]
    fn only_matching_skips_the_empty_matches() {
        // `o*` matches the empty string at every position that is not an `o`,
        // and GNU prints none of them: `-o` on "foo bar" is one line, `oo`.
        // Printing the empty ones would surround it with six blank lines and
        // make `-o` useless for exactly the patterns it exists for.
        let opts = Options {
            only_matching: true,
            ..Options::default()
        };
        let (out, matched) = run(b"foo bar\n", "o*", opts, "f", false);
        assert_eq!(out, "oo\n");
        assert!(matched);

        // A line whose *only* matches are empty prints nothing at all, yet is
        // still a selected line — which is why the exit status is 0.
        let opts = Options {
            only_matching: true,
            ..Options::default()
        };
        let (out, matched) = run(b"xyz\n", "o*", opts, "f", false);
        assert_eq!(out, "");
        assert!(matched);
    }

    #[test]
    fn null_data_changes_what_a_line_is_on_both_sides() {
        let opts = Options {
            null_data: true,
            ..Options::default()
        };
        let (out, matched) = run(b"foo\0bar\0", "o", opts, "f", false);
        assert_eq!(out, "foo\0");
        assert!(matched);

        // A NUL-separated stream has no newlines in it, so a `-z` grep that
        // still split on `\n` would return the whole file as one line.
        let opts = Options {
            null_data: true,
            line_numbers: true,
            ..Options::default()
        };
        let (out, _) = run(b"foo\0bar\0", "a", opts, "f", false);
        assert_eq!(out, "2:bar\0");
    }

    #[test]
    fn a_count_is_not_a_line_so_z_does_not_reach_it() {
        // Measured: `grep -zHc` ends its count with a newline even though the
        // matched lines it would otherwise print end with NUL.
        let opts = Options {
            null_data: true,
            count_only: true,
            ..Options::default()
        };
        let (out, _) = run(b"foo\0bar\0", "o", opts, "f.txt", true);
        assert_eq!(out, "f.txt:1\n");

        // `-Z` *does* reach it: it is the byte after a file name.
        let opts = Options {
            null_name: true,
            count_only: true,
            ..Options::default()
        };
        let (out, _) = run(b"foo\nbar\n", "o", opts, "f.txt", true);
        assert_eq!(out, "f.txt\x001\n");
    }

    #[test]
    fn search_quiet_prints_nothing_but_answers() {
        let opts = Options {
            quiet: true,
            ..Options::default()
        };
        let (out, matched) = run(b"a\nfoo\n", "foo", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "");
    }

    #[test]
    fn search_only_matching_prints_each_match_on_its_own_line() {
        let opts = Options {
            syntax: Syntax::Extended,
            only_matching: true,
            line_numbers: true,
            ..Options::default()
        };
        let (out, _) = run(b"ab12cd345\nnone\n", "[0-9]+", opts, "f", false);
        assert_eq!(out, "1:12\n1:345\n");
    }

    #[test]
    fn search_only_matching_with_invert_prints_nothing() {
        let opts = Options {
            only_matching: true,
            invert: true,
            ..Options::default()
        };
        let (out, matched) = run(b"abc\n", "z", opts, "f", false);
        assert!(matched, "the line is still selected");
        assert_eq!(out, "");
    }

    // ---------------- split_patterns ----------------

    #[test]
    fn a_pattern_file_holds_one_pattern_per_line() {
        assert_eq!(
            split_patterns(b"foo\nbar\n"),
            vec![b"foo".to_vec(), b"bar".to_vec()]
        );
        assert_eq!(
            split_patterns(b"foo\nbar"),
            vec![b"foo".to_vec(), b"bar".to_vec()]
        );
        // A trailing newline ends the last pattern; it does not begin an empty
        // one, which would match every line and turn grep into cat.
        assert_eq!(split_patterns(b"foo\n"), vec![b"foo".to_vec()]);
        // An empty file, though, *is* the empty pattern.
        assert_eq!(split_patterns(b""), vec![Vec::<u8>::new()]);
        assert_eq!(
            split_patterns(b"a\n\nb\n").len(),
            3,
            "a blank line is the empty pattern"
        );
    }

    #[test]
    fn quoting_a_literal_defuses_every_metacharacter() {
        assert_eq!(quote_ere(b"a.c"), b"a\\.c".to_vec());
        assert_eq!(quote_ere(b"a+b|c"), b"a\\+b\\|c".to_vec());
        assert_eq!(quote_ere(b"plain"), b"plain".to_vec());
    }
}
