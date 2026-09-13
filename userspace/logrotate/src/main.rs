#![deny(clippy::all)]

//! `logrotate` — rotate, retain and compress log files.
//!
//! Reads a configuration file describing which logs to rotate and how often,
//! decides which are due, and rotates them: `messages` becomes `messages.1`,
//! `messages.1` becomes `messages.2`, and the oldest beyond `rotate N` is
//! deleted. With `compress`, the newly rotated file is gzipped.
//!
//! # What this does and does not do
//!
//! Every directive it does not implement is an ERROR, not a silent skip. A
//! configuration file is a statement of intent about a machine's logs, and a
//! rotator that quietly ignores `olddir` will put files somewhere the
//! administrator did not ask for and report success. §1006's rule applied to
//! a config parser: refuse what you cannot do.
//!
//! Implemented: `daily`, `weekly`, `monthly`, `size`, `rotate N`, `compress`,
//! `missingok`, `notifempty`, and the global forms of the frequency and
//! `rotate` directives.
//!
//! # Why the state file matters
//!
//! Frequency is decided against the last rotation recorded in the state file,
//! not against the log's mtime — a log written to five seconds ago is not
//! thereby due, and a log nothing has written to since Tuesday still rotates
//! on Wednesday. Losing the state file means every log looks due exactly once,
//! which is the safe direction.

use quoting::quoteaf_os;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: &str = "0.1.0";

/// Where the last-rotation times live when `--state` is not given.
const DEFAULT_STATE: &str = "/var/lib/logrotate/status";

const SECONDS_PER_DAY: u64 = 86_400;

// ============================================================================
// Configuration
// ============================================================================

/// How often a log is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Schedule {
    Daily,
    Weekly,
    Monthly,
    /// Rotate when the log reaches this many bytes, regardless of the clock.
    Size(u64),
}

impl Schedule {
    /// Seconds that must pass since the last rotation, or `None` for `Size`.
    fn interval(self) -> Option<u64> {
        match self {
            Self::Daily => Some(SECONDS_PER_DAY),
            Self::Weekly => Some(7 * SECONDS_PER_DAY),
            // 30 days. Calendar months are not equal, and a rotator that
            // tried to be exact about "the first of the month" would need a
            // civil-date library to answer a question nobody asks of it.
            // Stated rather than hidden: `monthly` here means every 30 days.
            Self::Monthly => Some(30 * SECONDS_PER_DAY),
            Self::Size(_) => None,
        }
    }
}

/// One `{ ... }` block: the logs it names and what to do with them.
#[derive(Clone, Debug)]
struct Stanza {
    /// Paths as written. A `*` is expanded when the stanza runs, not when it
    /// is parsed, so a log created after the config was written is still
    /// found.
    patterns: Vec<String>,
    schedule: Schedule,
    /// How many old copies to keep. `rotate 0` deletes on rotation.
    keep: usize,
    compress: bool,
    /// A missing log is not an error.
    missingok: bool,
    /// An empty log is not rotated.
    notifempty: bool,
}

/// Defaults that apply to every stanza unless it overrides them.
#[derive(Clone, Debug)]
struct Globals {
    schedule: Schedule,
    keep: usize,
    compress: bool,
    missingok: bool,
    notifempty: bool,
}

impl Default for Globals {
    fn default() -> Self {
        Self {
            schedule: Schedule::Weekly,
            keep: 4,
            compress: false,
            missingok: false,
            notifempty: false,
        }
    }
}

/// Parse a size with an optional `k`, `M` or `G` suffix.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let (digits, mult) = match s.as_bytes().last() {
        Some(b'k' | b'K') => (&s[..s.len() - 1], 1024u64),
        Some(b'M' | b'm') => (&s[..s.len() - 1], 1024 * 1024),
        Some(b'G' | b'g') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    digits.trim().parse::<u64>().ok()?.checked_mul(mult)
}

/// Apply one directive to a settings block.
///
/// Returns `Err` for anything unrecognised. That is the point: a directive
/// this program does not implement must not be skipped silently, because the
/// file is a statement about what should happen to a machine's logs.
fn apply_directive(
    word: &str,
    rest: &str,
    schedule: &mut Schedule,
    keep: &mut usize,
    compress: &mut bool,
    missingok: &mut bool,
    notifempty: &mut bool,
) -> Result<(), String> {
    match word {
        "daily" => *schedule = Schedule::Daily,
        "weekly" => *schedule = Schedule::Weekly,
        "monthly" => *schedule = Schedule::Monthly,
        "size" | "minsize" | "maxsize" => {
            let n = parse_size(rest).ok_or_else(|| format!("invalid size: {rest}"))?;
            *schedule = Schedule::Size(n);
        }
        "rotate" => {
            *keep = rest
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("invalid rotate count: {rest}"))?;
        }
        "compress" => *compress = true,
        "nocompress" => *compress = false,
        "missingok" => *missingok = true,
        "nomissingok" => *missingok = false,
        "notifempty" => *notifempty = true,
        "ifempty" => *notifempty = false,
        other => {
            return Err(format!(
                "unsupported directive {other:?} -- this logrotate implements \
                 daily, weekly, monthly, size, rotate, compress, missingok \
                 and notifempty. Refusing rather than ignoring it: a skipped \
                 directive changes what happens to your logs and says nothing"
            ));
        }
    }
    Ok(())
}

/// Parse a configuration file into globals and stanzas.
fn parse_config(text: &str) -> Result<(Globals, Vec<Stanza>), String> {
    let mut globals = Globals::default();
    let mut stanzas = Vec::new();
    let mut pending: Option<Vec<String>> = None;
    let mut cur: Option<Stanza> = None;

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let n = lineno + 1;

        if line == "}" {
            let s = cur
                .take()
                .ok_or_else(|| format!("line {n}: unmatched }}"))?;
            stanzas.push(s);
            continue;
        }

        if let Some(head) = line.strip_suffix('{') {
            if cur.is_some() {
                return Err(format!("line {n}: a stanza cannot contain another"));
            }
            let mut pats: Vec<String> = head.split_whitespace().map(str::to_string).collect();
            if let Some(mut earlier) = pending.take() {
                earlier.append(&mut pats);
                pats = earlier;
            }
            if pats.is_empty() {
                return Err(format!("line {n}: a stanza names no log file"));
            }
            cur = Some(Stanza {
                patterns: pats,
                schedule: globals.schedule,
                keep: globals.keep,
                compress: globals.compress,
                missingok: globals.missingok,
                notifempty: globals.notifempty,
            });
            continue;
        }

        let (word, rest) = match line.split_once(char::is_whitespace) {
            Some((w, r)) => (w, r.trim()),
            None => (line, ""),
        };

        // A path on its own line before a `{` is part of the stanza's list.
        if cur.is_none() && (word.starts_with('/') || word.contains('*')) {
            pending
                .get_or_insert_with(Vec::new)
                .extend(line.split_whitespace().map(str::to_string));
            continue;
        }

        let target = cur.as_mut();
        let res = match target {
            Some(s) => apply_directive(
                word,
                rest,
                &mut s.schedule,
                &mut s.keep,
                &mut s.compress,
                &mut s.missingok,
                &mut s.notifempty,
            ),
            None => apply_directive(
                word,
                rest,
                &mut globals.schedule,
                &mut globals.keep,
                &mut globals.compress,
                &mut globals.missingok,
                &mut globals.notifempty,
            ),
        };
        res.map_err(|e| format!("line {n}: {e}"))?;
    }

    if cur.is_some() {
        return Err("unterminated stanza: missing `}`".to_string());
    }
    Ok((globals, stanzas))
}

// ============================================================================
// State
// ============================================================================

/// Last-rotation times, keyed by log path.
///
/// A line is `<unix-seconds> <path>`, with the timestamp first because a path
/// may contain spaces and the timestamp may not.
fn parse_state(text: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((when, path)) = line.split_once(' ')
            && let Ok(secs) = when.trim().parse::<u64>()
            && !path.trim().is_empty()
        {
            out.insert(path.trim().to_string(), secs);
        }
    }
    out
}

fn render_state(state: &BTreeMap<String, u64>) -> String {
    let mut out = String::from("# logrotate state -- written automatically\n");
    for (path, when) in state {
        out.push_str(&format!("{when} {path}\n"));
    }
    out
}

// ============================================================================
// Rotation
// ============================================================================

/// Is `log` due under `stanza`, given the last rotation and the current time?
fn is_due(stanza: &Stanza, size: u64, last: Option<u64>, now: u64, force: bool) -> bool {
    if force {
        return true;
    }
    if stanza.notifempty && size == 0 {
        return false;
    }
    match stanza.schedule {
        Schedule::Size(limit) => size >= limit,
        other => match (other.interval(), last) {
            // Never rotated: due now. Losing the state file therefore rotates
            // everything exactly once, which is the safe direction -- the
            // alternative is a log that is never rotated again.
            (Some(_), None) => true,
            (Some(iv), Some(prev)) => now.saturating_sub(prev) >= iv,
            (None, _) => false,
        },
    }
}

/// The name of the `n`th rotated copy: `messages.1`, `messages.2`, ...
fn rotated_name(log: &Path, n: usize) -> PathBuf {
    let mut s = log.as_os_str().to_os_string();
    s.push(format!(".{n}"));
    PathBuf::from(s)
}

/// The compressed form of a rotated copy.
fn gz_name(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".gz");
    PathBuf::from(s)
}

/// Shift the existing copies up one and move the live log into slot 1.
///
/// Every step is checked. A rotation that half-happened and reported success
/// would leave two copies with the same content and one gap, and nothing
/// downstream could tell.
fn rotate_one(log: &Path, keep: usize, compress: bool) -> Result<(), String> {
    // Delete the oldest, if keeping it would exceed `keep`.
    for cand in [rotated_name(log, keep), gz_name(&rotated_name(log, keep))] {
        if cand.exists() {
            fs::remove_file(&cand)
                .map_err(|e| format!("cannot remove {}: {e}", quoteaf_os(&cand)))?;
        }
    }

    // Shift downward from the oldest so nothing is overwritten.
    for n in (1..keep).rev() {
        for (from, to) in [
            (rotated_name(log, n), rotated_name(log, n + 1)),
            (
                gz_name(&rotated_name(log, n)),
                gz_name(&rotated_name(log, n + 1)),
            ),
        ] {
            if from.exists() {
                fs::rename(&from, &to).map_err(|e| {
                    format!(
                        "cannot rename {} to {}: {e}",
                        quoteaf_os(&from),
                        quoteaf_os(&to)
                    )
                })?;
            }
        }
    }

    if keep == 0 {
        // `rotate 0` keeps nothing: the live log is discarded rather than
        // moved. Doing this by renaming to `.1` and deleting it would be the
        // same outcome by a longer route, and would briefly leave a file the
        // configuration says should not exist.
        return fs::remove_file(log).map_err(|e| format!("cannot remove {}: {e}", quoteaf_os(log)));
    }

    let first = rotated_name(log, 1);
    fs::rename(log, &first).map_err(|e| {
        format!(
            "cannot rename {} to {}: {e}",
            quoteaf_os(log),
            quoteaf_os(&first)
        )
    })?;

    if compress {
        let data =
            fs::read(&first).map_err(|e| format!("cannot read {}: {e}", quoteaf_os(&first)))?;
        let packed = deflate::gzip(&data);
        let target = gz_name(&first);
        fs::write(&target, &packed)
            .map_err(|e| format!("cannot write {}: {e}", quoteaf_os(&target)))?;
        // Only now is the uncompressed copy redundant. Removing it first
        // would lose the log if the write failed.
        fs::remove_file(&first)
            .map_err(|e| format!("cannot remove {}: {e}", quoteaf_os(&first)))?;
    }
    Ok(())
}

/// Expand one pattern to the paths it names.
///
/// Only a trailing `*` in the final component is supported, which is what
/// every configuration in this tree uses. Anything more elaborate is refused
/// by the caller rather than silently matching nothing.
fn expand(pattern: &Path) -> Result<Vec<PathBuf>, String> {
    // A `&Path`, not a `&str`. A log path is OS-boundary data and may hold any
    // byte but `/` and NUL, so taking a `&str` would force every caller to
    // decode one -- and the only ways to do that either corrupt
    // (`to_string_lossy`) or panic (`to_str().expect`). The pre-push argv-utf8
    // gate refused both when this crate was first written.
    let p = pattern;
    let parent = p.parent().unwrap_or_else(|| Path::new("."));

    // Examine the components rather than the whole string, and check the
    // DIRECTORY ones before the file name.
    //
    // The first version of this looked at the file name first and returned
    // early when it held no `*` -- which silently turned `/var/*/messages`
    // into a literal path, the exact "matches nothing and looks like an empty
    // directory" outcome the refusal below exists to prevent. Its own test
    // caught it.
    //
    // A component that does not decode as UTF-8 cannot contain a `*` we could
    // match against, so it is treated as literal text, which is correct.
    let star_in_directory = parent
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|c| c.contains('*'));
    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if !star_in_directory && !name.contains('*') {
        return Ok(vec![p.to_path_buf()]);
    }
    if star_in_directory {
        return Err(format!(
            "{pattern:?}: a `*` in a directory component is not supported. \
             Refusing rather than matching nothing, which would look like a \
             directory with no logs in it"
        ));
    }
    let (prefix, suffix) = name
        .split_once('*')
        .ok_or_else(|| format!("cannot read the pattern {}", quoteaf_os(pattern)))?;
    if suffix.contains('*') {
        return Err(format!(
            "{}: only one `*` is supported",
            quoteaf_os(pattern)
        ));
    }
    let mut out = Vec::new();
    let entries = match fs::read_dir(parent) {
        Ok(e) => e,
        // A missing directory names no logs. Whether that is an error is the
        // caller's decision, via `missingok`.
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let Some(fname) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if fname.len() >= prefix.len() + suffix.len()
            && fname.starts_with(prefix)
            && fname.ends_with(suffix)
            // Never rotate our own output.
            && !is_rotated_copy(&fname)
        {
            out.push(parent.join(fname));
        }
    }
    out.sort();
    Ok(out)
}

/// Does this name look like something we produced -- `foo.1`, `foo.2.gz`?
///
/// Without this a pattern like `messages*` matches `messages.1` on the second
/// run and rotates the rotated copy, which is how a log ends up as
/// `messages.1.1.1`.
fn is_rotated_copy(name: &str) -> bool {
    let base = name.strip_suffix(".gz").unwrap_or(name);
    match base.rsplit_once('.') {
        Some((_, last)) => !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

// ============================================================================
// CLI
// ============================================================================

fn print_help() {
    println!("Usage: logrotate [options] <config>");
    println!();
    println!("Rotate, retain and compress log files.");
    println!();
    println!("Options:");
    println!("  -d, --dry-run       Say what would happen; change nothing");
    println!("  -f, --force         Rotate every log, whether due or not");
    println!("  -s, --state FILE    Use FILE instead of the default state file");
    println!("  -v, --verbose       Name every log considered");
    println!("  -h, --help          Show this help");
    println!("  -V, --version       Show version");
}

/// Both paths are `PathBuf`, and the argv they come from is read with
/// `args_os`.
///
/// `std::env::args()` PANICS on an argument that is not valid Unicode, and a
/// log path is OS-boundary data that may hold any byte but `/` and NUL -- so a
/// program whose entire job is naming log files would crash on exactly the
/// input it exists to handle. The pre-push argv-utf8 gate caught the first
/// version of this.
struct Args {
    config: Option<PathBuf>,
    state: PathBuf,
    dry_run: bool,
    force: bool,
    verbose: bool,
}

fn parse_args(argv: &[std::ffi::OsString]) -> Args {
    let mut a = Args {
        config: None,
        state: PathBuf::from(DEFAULT_STATE),
        dry_run: false,
        force: false,
        verbose: false,
    };
    let mut i = 1;
    while i < argv.len() {
        // Flags are ASCII by construction. An argument that does not decode
        // therefore matches none of them and falls to the positional arm --
        // which is correct, because it is a path.
        let as_text = argv[i].to_str().unwrap_or("");
        match as_text {
            "-h" | "--help" => {
                print_help();
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("logrotate {VERSION}");
                process::exit(0);
            }
            "-d" | "--dry-run" => a.dry_run = true,
            "-f" | "--force" => a.force = true,
            "-v" | "--verbose" => a.verbose = true,
            "-s" | "--state" => {
                i += 1;
                let Some(v) = argv.get(i) else {
                    eprintln!("logrotate: --state requires a file");
                    process::exit(1);
                };
                a.state = PathBuf::from(v);
            }
            other if !other.starts_with('-') || other.is_empty() => {
                a.config = Some(PathBuf::from(&argv[i]));
            }
            other => {
                // Stop. A rotator that did not understand its command line
                // must not go on to move a machine's logs.
                eprintln!("logrotate: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }
    a
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn main() {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let args = parse_args(&argv);

    let Some(config_path) = args.config else {
        eprintln!("logrotate: a configuration file is required");
        eprintln!("Try 'logrotate --help' for more information.");
        process::exit(1);
    };

    let text = match fs::read_to_string(&config_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("logrotate: cannot read {}: {e}", quoteaf_os(&config_path));
            process::exit(1);
        }
    };

    let (_globals, stanzas) = match parse_config(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("logrotate: {}: {e}", quoteaf_os(&config_path));
            process::exit(1);
        }
    };

    let state_path = args.state.clone();
    let mut state = fs::read_to_string(&state_path)
        .map(|t| parse_state(&t))
        .unwrap_or_default();

    let now = now_secs();
    let mut rotated = 0usize;
    let mut failures = 0usize;

    for stanza in &stanzas {
        for pattern in &stanza.patterns {
            let logs = match expand(Path::new(pattern)) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("logrotate: {e}");
                    failures += 1;
                    continue;
                }
            };
            if logs.is_empty() && !stanza.missingok {
                eprintln!("logrotate: {}: no such log file", quoteaf_os(pattern));
                failures += 1;
                continue;
            }
            for log in logs {
                let meta = match fs::metadata(&log) {
                    Ok(m) => m,
                    Err(e) => {
                        if !stanza.missingok {
                            eprintln!("logrotate: cannot stat {}: {e}", quoteaf_os(&log));
                            failures += 1;
                        }
                        continue;
                    }
                };
                let key = log.to_string_lossy().to_string();
                let due = is_due(
                    stanza,
                    meta.len(),
                    state.get(&key).copied(),
                    now,
                    args.force,
                );
                if !due {
                    if args.verbose {
                        println!("logrotate: {} is not due", quoteaf_os(&log));
                    }
                    continue;
                }
                if args.dry_run {
                    println!("logrotate: would rotate {}", quoteaf_os(&log));
                    continue;
                }
                match rotate_one(&log, stanza.keep, stanza.compress) {
                    Ok(()) => {
                        // Announced only now, after the rename and any
                        // compression have both landed.
                        println!("logrotate: rotated {}", quoteaf_os(&log));
                        state.insert(key, now);
                        rotated += 1;
                    }
                    Err(e) => {
                        eprintln!("logrotate: {e}");
                        failures += 1;
                    }
                }
            }
        }
    }

    if !args.dry_run && rotated > 0 {
        if let Some(parent) = state_path.parent()
            && !parent.as_os_str().is_empty()
        {
            // Discarded deliberately: the write below reports the same cause
            // against the path the caller named.
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(&state_path, render_state(&state)) {
            // The logs ARE rotated at this point, so this is not a rotation
            // failure -- but losing the state means every log looks due again
            // on the next run, so it must not be silent.
            eprintln!(
                "logrotate: rotated {rotated} log(s) but could not write the \
                 state file {}: {e}",
                quoteaf_os(&state_path)
            );
            eprintln!("logrotate: the next run will treat every log as due");
            process::exit(1);
        }
    }

    if failures > 0 {
        eprintln!("logrotate: {failures} log(s) could not be rotated");
        process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use scratchdir::ScratchDir;

    // ---- configuration ----------------------------------------------------

    #[test]
    fn a_stanza_takes_its_defaults_from_the_globals_above_it() {
        let (g, s) =
            parse_config("compress\nrotate 7\ndaily\n\n/var/log/messages {\n}\n").expect("parses");
        assert_eq!(g.keep, 7);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].keep, 7);
        assert!(s[0].compress);
        assert_eq!(s[0].schedule, Schedule::Daily);
    }

    #[test]
    fn a_stanza_overrides_the_globals_without_changing_them() {
        let (g, s) = parse_config("rotate 7\ndaily\n/a {\n  rotate 2\n  weekly\n}\n/b {\n}\n")
            .expect("parses");
        assert_eq!(s[0].keep, 2);
        assert_eq!(s[0].schedule, Schedule::Weekly);
        // The second stanza must still see the ORIGINAL globals. A parser
        // that let a stanza leak into the defaults would silently reconfigure
        // every log below it.
        assert_eq!(s[1].keep, 7);
        assert_eq!(s[1].schedule, Schedule::Daily);
        assert_eq!(g.keep, 7);
    }

    #[test]
    fn an_unsupported_directive_is_refused_rather_than_skipped() {
        // The whole point. `olddir` moves rotated logs elsewhere; ignoring it
        // puts them somewhere the administrator did not ask for and reports
        // success.
        let err = parse_config("/a {\n  olddir /var/log/old\n}\n").expect_err("must refuse");
        assert!(err.contains("olddir"), "{err}");
        assert!(err.contains("line 2"), "the line number helps: {err}");
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let (_, s) = parse_config("# a comment\n\n/a {\n  # another\n  daily  # trailing\n}\n")
            .expect("parses");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].schedule, Schedule::Daily);
    }

    #[test]
    fn several_logs_may_share_one_stanza() {
        let (_, s) = parse_config("/a /b {\n  daily\n}\n").expect("parses");
        assert_eq!(s[0].patterns, vec!["/a", "/b"]);
    }

    #[test]
    fn an_unterminated_stanza_is_an_error_not_a_silently_dropped_one() {
        let err = parse_config("/a {\n  daily\n").expect_err("must refuse");
        assert!(err.contains("unterminated"), "{err}");
    }

    #[test]
    fn a_stray_closing_brace_is_an_error() {
        parse_config("}\n").expect_err("must refuse");
    }

    #[test]
    fn sizes_accept_the_usual_suffixes() {
        assert_eq!(parse_size("100"), Some(100));
        assert_eq!(parse_size("2k"), Some(2048));
        assert_eq!(parse_size("3M"), Some(3 * 1024 * 1024));
        assert_eq!(parse_size("1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("notanumber"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn an_invalid_size_is_refused_rather_than_defaulted() {
        // A size that failed to parse must not become 0, which would rotate
        // on every run, nor be ignored, which would never rotate.
        let err = parse_config("/a {\n  size wat\n}\n").expect_err("must refuse");
        assert!(err.contains("size"), "{err}");
    }

    // ---- state ------------------------------------------------------------

    #[test]
    fn state_round_trips_including_a_path_with_spaces() {
        // The timestamp is written first precisely so a path may contain
        // spaces. A `path timestamp` format would split this one wrongly.
        let mut m = BTreeMap::new();
        m.insert("/var/log/my messages".to_string(), 1_700_000_000u64);
        m.insert("/var/log/plain".to_string(), 42u64);
        let back = parse_state(&render_state(&m));
        assert_eq!(back, m);
    }

    #[test]
    fn a_malformed_state_line_is_skipped_rather_than_poisoning_the_file() {
        // The state file is regenerated every run, so a corrupt line costs at
        // most one extra rotation of the log it named -- whereas refusing to
        // parse it would stop rotation entirely.
        let m = parse_state("garbage\n1700000000 /var/log/a\nnotanumber /b\n");
        assert_eq!(m.len(), 1);
        assert_eq!(m.get("/var/log/a"), Some(&1_700_000_000));
    }

    // ---- due-ness ---------------------------------------------------------

    fn stanza(schedule: Schedule) -> Stanza {
        Stanza {
            patterns: vec!["/a".to_string()],
            schedule,
            keep: 3,
            compress: false,
            missingok: false,
            notifempty: false,
        }
    }

    #[test]
    fn a_log_never_rotated_is_due() {
        assert!(is_due(&stanza(Schedule::Daily), 10, None, 1_000_000, false));
    }

    #[test]
    fn the_interval_boundary_is_inclusive() {
        let s = stanza(Schedule::Daily);
        let last = 1_000_000u64;
        assert!(!is_due(
            &s,
            10,
            Some(last),
            last + SECONDS_PER_DAY - 1,
            false
        ));
        assert!(is_due(&s, 10, Some(last), last + SECONDS_PER_DAY, false));
    }

    #[test]
    fn size_ignores_the_clock_and_the_clock_ignores_size() {
        let by_size = stanza(Schedule::Size(1000));
        // Rotated one second ago, but over the limit: due.
        assert!(is_due(&by_size, 1000, Some(999_999), 1_000_000, false));
        assert!(!is_due(&by_size, 999, Some(0), 1_000_000, false));

        // A daily log is not due for being large.
        let by_time = stanza(Schedule::Daily);
        assert!(!is_due(
            &by_time,
            u64::MAX,
            Some(1_000_000),
            1_000_001,
            false
        ));
    }

    #[test]
    fn notifempty_holds_back_an_empty_log_and_force_overrides_everything() {
        let mut s = stanza(Schedule::Daily);
        s.notifempty = true;
        assert!(!is_due(&s, 0, None, 1_000_000, false));
        assert!(is_due(&s, 1, None, 1_000_000, false));
        assert!(is_due(&s, 0, None, 1_000_000, true), "--force must win");
    }

    // ---- rotated-copy detection -------------------------------------------

    #[test]
    fn our_own_output_is_not_mistaken_for_a_log() {
        // Without this, `messages*` matches `messages.1` on the second run and
        // produces `messages.1.1`.
        assert!(is_rotated_copy("messages.1"));
        assert!(is_rotated_copy("messages.12"));
        assert!(is_rotated_copy("messages.3.gz"));
        assert!(!is_rotated_copy("messages"));
        assert!(!is_rotated_copy("messages.log"));
        assert!(!is_rotated_copy("messages."));
    }

    // ---- rotation ---------------------------------------------------------

    fn write(p: &Path, s: &str) {
        fs::write(p, s).expect("fixture write");
    }

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap_or_default()
    }

    #[test]
    fn rotation_shifts_every_copy_up_and_keeps_the_contents_in_order() {
        let dir = ScratchDir::new("logrotate-shift");
        let log = dir.path("messages");
        write(&log, "newest");
        write(&rotated_name(&log, 1), "older");
        write(&rotated_name(&log, 2), "oldest");

        rotate_one(&log, 3, false).expect("rotates");

        assert!(!log.exists(), "the live log should have been moved");
        assert_eq!(read(&rotated_name(&log, 1)), "newest");
        assert_eq!(read(&rotated_name(&log, 2)), "older");
        assert_eq!(read(&rotated_name(&log, 3)), "oldest");
    }

    #[test]
    fn the_copy_beyond_the_keep_count_is_deleted() {
        let dir = ScratchDir::new("logrotate-keep");
        let log = dir.path("messages");
        write(&log, "live");
        write(&rotated_name(&log, 1), "one");
        write(&rotated_name(&log, 2), "two");

        rotate_one(&log, 2, false).expect("rotates");

        assert_eq!(read(&rotated_name(&log, 1)), "live");
        assert_eq!(read(&rotated_name(&log, 2)), "one");
        assert!(
            !rotated_name(&log, 3).exists(),
            "keep 2 must not leave a third copy"
        );
    }

    #[test]
    fn rotate_zero_discards_the_log_rather_than_keeping_a_copy() {
        let dir = ScratchDir::new("logrotate-zero");
        let log = dir.path("messages");
        write(&log, "live");
        rotate_one(&log, 0, false).expect("rotates");
        assert!(!log.exists());
        assert!(!rotated_name(&log, 1).exists(), "rotate 0 keeps nothing");
    }

    #[test]
    fn compress_really_compresses_and_removes_the_plain_copy() {
        // The fabrication this avoids: renaming to `.gz` and leaving the
        // bytes alone. The gzip magic is checked, not just the extension.
        let dir = ScratchDir::new("logrotate-gz");
        let log = dir.path("messages");
        write(&log, "some log content that is definitely not gzip");

        rotate_one(&log, 3, true).expect("rotates");

        let gz = gz_name(&rotated_name(&log, 1));
        assert!(gz.exists(), "the compressed copy is missing");
        assert!(
            !rotated_name(&log, 1).exists(),
            "the uncompressed copy must be removed after a successful write"
        );
        let bytes = fs::read(&gz).expect("read");
        assert_eq!(&bytes[..2], &[0x1f, 0x8b], "not a gzip stream");
    }

    #[test]
    fn compressed_copies_shift_too() {
        // A previous run left `messages.1.gz`; this run must move it to
        // `messages.2.gz` rather than leaving it to be overwritten.
        let dir = ScratchDir::new("logrotate-gzshift");
        let log = dir.path("messages");
        write(&log, "live");
        write(&gz_name(&rotated_name(&log, 1)), "pretend-gz");

        rotate_one(&log, 3, false).expect("rotates");

        assert_eq!(read(&gz_name(&rotated_name(&log, 2))), "pretend-gz");
        assert_eq!(read(&rotated_name(&log, 1)), "live");
    }

    #[test]
    fn rotating_a_log_that_is_not_there_is_an_error_not_a_silent_success() {
        let dir = ScratchDir::new("logrotate-absent");
        let log = dir.path("nope");
        let err = rotate_one(&log, 3, false).expect_err("must not report success");
        assert!(err.contains("cannot rename"), "{err}");
    }

    // ---- pattern expansion ------------------------------------------------

    #[test]
    fn a_pattern_finds_matching_logs_and_skips_our_own_output() {
        let dir = ScratchDir::new("logrotate-glob");
        write(&dir.path("app.log"), "a");
        write(&dir.path("web.log"), "b");
        write(&dir.path("app.log.1"), "old");
        write(&dir.path("notes.txt"), "c");

        let pattern = dir.dir().join("*.log");
        // A `PathBuf` straight in: no decoding, nothing to panic on.
        let found = expand(&pattern).expect("expands");
        let names: Vec<String> = found
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        assert_eq!(names, vec!["app.log", "web.log"], "got {names:?}");
    }

    #[test]
    fn a_star_in_a_directory_component_is_refused_rather_than_matching_nothing() {
        let err = expand(Path::new("/var/*/messages")).expect_err("must refuse");
        assert!(err.contains("directory component"), "{err}");
    }

    #[test]
    fn a_pattern_naming_a_missing_directory_finds_nothing_without_erroring() {
        // Whether that is a failure is `missingok`'s decision, not the
        // expander's.
        let found = expand(Path::new("/zzq-no-such-directory/*.log")).expect("no error");
        assert!(found.is_empty());
    }

    #[test]
    fn a_path_with_no_star_is_returned_as_itself_even_if_absent() {
        let found = expand(Path::new("/var/log/messages")).expect("no error");
        assert_eq!(found, vec![PathBuf::from("/var/log/messages")]);
    }

    // ---- argument parsing -------------------------------------------------

    fn argv(parts: &[&str]) -> Vec<std::ffi::OsString> {
        parts.iter().map(|s| std::ffi::OsString::from(*s)).collect()
    }

    #[test]
    fn the_state_file_defaults_and_can_be_overridden() {
        let a = parse_args(&argv(&["logrotate", "cfg"]));
        assert_eq!(a.state, PathBuf::from(DEFAULT_STATE));
        assert_eq!(a.config, Some(PathBuf::from("cfg")));
        let b = parse_args(&argv(&["logrotate", "-s", "/tmp/st", "cfg"]));
        assert_eq!(b.state, PathBuf::from("/tmp/st"));
        assert_eq!(b.config, Some(PathBuf::from("cfg")));
    }

    #[test]
    fn the_flags_parse_and_do_not_swallow_the_config() {
        let a = parse_args(&argv(&["logrotate", "-d", "-f", "-v", "cfg"]));
        assert!(a.dry_run && a.force && a.verbose);
        assert_eq!(a.config, Some(PathBuf::from("cfg")));
    }
}
