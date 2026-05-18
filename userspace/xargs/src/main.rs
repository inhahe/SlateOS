//! OurOS `xargs` Utility -- Build and Execute Command Lines from Standard Input
//!
//! Reads items from standard input (delimited by whitespace, newlines, NUL
//! bytes, or a custom delimiter) and appends them as arguments to a specified
//! command, executing it one or more times.
//!
//! # Usage
//!
//! ```text
//! xargs [OPTIONS] [COMMAND [INITIAL-ARGS...]]
//!
//! find /tmp -name "*.log" | xargs rm
//! echo "a b c" | xargs -n 1 echo item:
//! find . -name "*.rs" -print0 | xargs -0 grep "TODO"
//! echo file1 file2 | xargs -I {} cp {} /backup/{}
//! ```

use std::env;
use std::io::{self, Read as _, Write as _};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Default maximum command-line length (128 KiB). Real systems derive this
/// from `ARG_MAX`; we use a reasonable default.
const DEFAULT_MAX_CHARS: usize = 128 * 1024;

// ============================================================================
// Exit codes
// ============================================================================

/// All commands succeeded.
const EXIT_SUCCESS: i32 = 0;
/// At least one command returned a non-zero status (1-125).
const EXIT_COMMAND_FAILED: i32 = 123;
/// Command line exceeded the `-s` limit with `-x` active.
const EXIT_TOO_LONG: i32 = 124;
/// Internal error (argument parsing, I/O failure, etc.).
const EXIT_INTERNAL: i32 = 125;
/// Command found but could not be executed.
const EXIT_NOT_EXECUTABLE: i32 = 126;
/// Command not found.
const EXIT_NOT_FOUND: i32 = 127;

// ============================================================================
// Configuration
// ============================================================================

/// Parsed command-line configuration.
struct Config {
    /// The command and any initial (fixed) arguments.
    command: Vec<String>,
    /// Maximum number of arguments per invocation (`-n`).
    max_args: Option<usize>,
    /// Maximum number of input lines per invocation (`-L`).
    max_lines: Option<usize>,
    /// Maximum total command-line character length (`-s`).
    max_chars: usize,
    /// Custom delimiter (`-d`).
    delimiter: Option<u8>,
    /// NUL-delimited input (`-0`).
    null_delimited: bool,
    /// Replace string for `-I` mode.
    replace_str: Option<String>,
    /// Prompt before each execution (`-p`).
    interactive: bool,
    /// Print each command to stderr before executing (`-t`).
    verbose: bool,
    /// Do not run the command if stdin is empty (`-r`).
    no_run_if_empty: bool,
    /// Maximum parallel processes (`-P`).
    max_procs: usize,
    /// Output commands as JSON instead of executing (`--json`).
    json: bool,
    /// Exit immediately if a command line would exceed `-s` (`-x`).
    exit_on_too_long: bool,
}

impl Config {
    fn new() -> Self {
        Self {
            command: Vec::new(),
            max_args: None,
            max_lines: None,
            max_chars: DEFAULT_MAX_CHARS,
            delimiter: None,
            null_delimited: false,
            replace_str: None,
            interactive: false,
            verbose: false,
            no_run_if_empty: false,
            max_procs: 1,
            json: false,
            exit_on_too_long: false,
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Parse a numeric option value from a string, returning `EXIT_INTERNAL` on
/// failure.
fn parse_usize(flag: &str, value: &str) -> Result<usize, i32> {
    value.parse::<usize>().map_err(|_| {
        eprintln!("xargs: invalid number for {flag}: '{value}'");
        EXIT_INTERNAL
    })
}

/// Consume the next argument from the iterator or report an error.
fn require_arg<'a>(
    flag: &str,
    iter: &mut impl Iterator<Item = &'a String>,
) -> Result<&'a String, i32> {
    iter.next().ok_or_else(|| {
        eprintln!("xargs: option '{flag}' requires an argument");
        EXIT_INTERNAL
    })
}

/// Parse command-line arguments into a `Config`.
///
/// Returns `Err(exit_code)` if the arguments are invalid or the user
/// requested `--help` / `--version` (in which case we already printed the
/// appropriate output).
fn parse_args(args: &[String]) -> Result<Config, i32> {
    let mut config = Config::new();
    let mut iter = args[1..].iter();
    let mut options_done = false;

    while let Some(arg) = iter.next() {
        // Once `--` is seen, everything remaining is the command.
        if options_done {
            config.command.push(arg.clone());
            continue;
        }

        if arg == "--" {
            options_done = true;
            continue;
        }

        // Long options.
        if arg.starts_with("--") {
            match arg.as_str() {
                "--help" => {
                    print_help();
                    return Err(EXIT_SUCCESS);
                }
                "--version" => {
                    println!("xargs (OurOS) {VERSION}");
                    return Err(EXIT_SUCCESS);
                }
                "--null" => config.null_delimited = true,
                "--no-run-if-empty" => config.no_run_if_empty = true,
                "--interactive" => config.interactive = true,
                "--verbose" => config.verbose = true,
                "--json" => config.json = true,
                "--exit" => config.exit_on_too_long = true,
                "--max-args" => {
                    let v = require_arg("--max-args", &mut iter)?;
                    let n = parse_usize("--max-args", v)?;
                    if n == 0 {
                        eprintln!("xargs: --max-args must be > 0");
                        return Err(EXIT_INTERNAL);
                    }
                    config.max_args = Some(n);
                }
                "--max-lines" => {
                    let v = require_arg("--max-lines", &mut iter)?;
                    let n = parse_usize("--max-lines", v)?;
                    if n == 0 {
                        eprintln!("xargs: --max-lines must be > 0");
                        return Err(EXIT_INTERNAL);
                    }
                    config.max_lines = Some(n);
                }
                "--max-chars" => {
                    let v = require_arg("--max-chars", &mut iter)?;
                    config.max_chars = parse_usize("--max-chars", v)?;
                }
                "--max-procs" => {
                    let v = require_arg("--max-procs", &mut iter)?;
                    config.max_procs = parse_usize("--max-procs", v)?;
                    if config.max_procs == 0 {
                        // 0 means "as many as possible" -- treat as a large
                        // number (we cap to thread count at runtime).
                        config.max_procs = usize::MAX;
                    }
                }
                "--delimiter" => {
                    let v = require_arg("--delimiter", &mut iter)?;
                    config.delimiter = Some(parse_delimiter(v)?);
                }
                "--replace" => {
                    let v = require_arg("--replace", &mut iter)?;
                    if v.is_empty() {
                        eprintln!("xargs: --replace requires a non-empty string");
                        return Err(EXIT_INTERNAL);
                    }
                    config.replace_str = Some(v.clone());
                }
                _ if arg.starts_with("--max-args=") => {
                    let v = &arg["--max-args=".len()..];
                    let n = parse_usize("--max-args", v)?;
                    if n == 0 {
                        eprintln!("xargs: --max-args must be > 0");
                        return Err(EXIT_INTERNAL);
                    }
                    config.max_args = Some(n);
                }
                _ if arg.starts_with("--max-lines=") => {
                    let v = &arg["--max-lines=".len()..];
                    let n = parse_usize("--max-lines", v)?;
                    if n == 0 {
                        eprintln!("xargs: --max-lines must be > 0");
                        return Err(EXIT_INTERNAL);
                    }
                    config.max_lines = Some(n);
                }
                _ if arg.starts_with("--max-chars=") => {
                    let v = &arg["--max-chars=".len()..];
                    config.max_chars = parse_usize("--max-chars", v)?;
                }
                _ if arg.starts_with("--max-procs=") => {
                    let v = &arg["--max-procs=".len()..];
                    config.max_procs = parse_usize("--max-procs", v)?;
                    if config.max_procs == 0 {
                        config.max_procs = usize::MAX;
                    }
                }
                _ if arg.starts_with("--delimiter=") => {
                    let v = &arg["--delimiter=".len()..];
                    config.delimiter = Some(parse_delimiter(v)?);
                }
                _ if arg.starts_with("--replace=") => {
                    let v = &arg["--replace=".len()..];
                    if v.is_empty() {
                        eprintln!("xargs: --replace requires a non-empty string");
                        return Err(EXIT_INTERNAL);
                    }
                    config.replace_str = Some(v.to_string());
                }
                _ => {
                    eprintln!("xargs: unrecognized option '{arg}'");
                    eprintln!("Try 'xargs --help' for more information.");
                    return Err(EXIT_INTERNAL);
                }
            }
            continue;
        }

        // Short options.
        if arg.starts_with('-') && arg.len() > 1 {
            let bytes = arg.as_bytes();
            let mut j = 1;
            while j < bytes.len() {
                match bytes[j] {
                    b'0' => config.null_delimited = true,
                    b'r' => config.no_run_if_empty = true,
                    b'p' => config.interactive = true,
                    b't' => config.verbose = true,
                    b'x' => config.exit_on_too_long = true,
                    b'n' => {
                        let v = consume_short_value(bytes, j, "-n", &mut iter)?;
                        let n = parse_usize("-n", &v)?;
                        if n == 0 {
                            eprintln!("xargs: -n must be > 0");
                            return Err(EXIT_INTERNAL);
                        }
                        config.max_args = Some(n);
                        // consume_short_value consumed the rest of this cluster.
                        j = bytes.len();
                        continue;
                    }
                    b'L' => {
                        let v = consume_short_value(bytes, j, "-L", &mut iter)?;
                        let n = parse_usize("-L", &v)?;
                        if n == 0 {
                            eprintln!("xargs: -L must be > 0");
                            return Err(EXIT_INTERNAL);
                        }
                        config.max_lines = Some(n);
                        j = bytes.len();
                        continue;
                    }
                    b's' => {
                        let v = consume_short_value(bytes, j, "-s", &mut iter)?;
                        config.max_chars = parse_usize("-s", &v)?;
                        j = bytes.len();
                        continue;
                    }
                    b'P' => {
                        let v = consume_short_value(bytes, j, "-P", &mut iter)?;
                        config.max_procs = parse_usize("-P", &v)?;
                        if config.max_procs == 0 {
                            config.max_procs = usize::MAX;
                        }
                        j = bytes.len();
                        continue;
                    }
                    b'd' => {
                        let v = consume_short_value(bytes, j, "-d", &mut iter)?;
                        config.delimiter = Some(parse_delimiter(&v)?);
                        j = bytes.len();
                        continue;
                    }
                    b'I' => {
                        let v = consume_short_value(bytes, j, "-I", &mut iter)?;
                        if v.is_empty() {
                            eprintln!("xargs: -I requires a non-empty string");
                            return Err(EXIT_INTERNAL);
                        }
                        config.replace_str = Some(v);
                        j = bytes.len();
                        continue;
                    }
                    _ => {
                        eprintln!(
                            "xargs: invalid option -- '{}'",
                            char::from(bytes[j])
                        );
                        eprintln!("Try 'xargs --help' for more information.");
                        return Err(EXIT_INTERNAL);
                    }
                }
                j += 1;
            }
            continue;
        }

        // Not an option -- start of the command.
        options_done = true;
        config.command.push(arg.clone());
    }

    // Default command is "echo".
    if config.command.is_empty() {
        config.command.push("echo".to_string());
    }

    // -I implies -n 1 -L 1 unless the user overrode them.
    if config.replace_str.is_some() {
        if config.max_args.is_none() {
            config.max_args = Some(1);
        }
        if config.max_lines.is_none() {
            config.max_lines = Some(1);
        }
    }

    Ok(config)
}

/// Consume the value part of a short option that takes an argument.
///
/// If there are remaining characters in the current cluster after position `j`,
/// they form the value. Otherwise the next argument from `iter` is used.
fn consume_short_value<'a>(
    bytes: &[u8],
    j: usize,
    flag: &str,
    iter: &mut impl Iterator<Item = &'a String>,
) -> Result<String, i32> {
    if j + 1 < bytes.len() {
        // The rest of this cluster is the value.
        Ok(String::from_utf8_lossy(&bytes[j + 1..]).into_owned())
    } else {
        // Next argument is the value.
        let v = iter.next().ok_or_else(|| {
            eprintln!("xargs: option '{flag}' requires an argument");
            EXIT_INTERNAL
        })?;
        Ok(v.clone())
    }
}

/// Parse a delimiter specification.
///
/// Accepts a single character, or the escape sequences `\\n`, `\\t`, `\\0`.
fn parse_delimiter(s: &str) -> Result<u8, i32> {
    let bytes = s.as_bytes();
    match bytes.len() {
        1 => Ok(bytes[0]),
        2 if bytes[0] == b'\\' => match bytes[1] {
            b'n' => Ok(b'\n'),
            b't' => Ok(b'\t'),
            b'0' => Ok(0),
            b'\\' => Ok(b'\\'),
            _ => {
                eprintln!("xargs: invalid delimiter escape: '{s}'");
                Err(EXIT_INTERNAL)
            }
        },
        _ => {
            eprintln!("xargs: delimiter must be a single character: '{s}'");
            Err(EXIT_INTERNAL)
        }
    }
}

// ============================================================================
// Input parsing
// ============================================================================

/// Read all of stdin into a byte buffer.
fn read_stdin() -> Result<Vec<u8>, i32> {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf).map_err(|e| {
        eprintln!("xargs: failed to read stdin: {e}");
        EXIT_INTERNAL
    })?;
    Ok(buf)
}

/// Split input on NUL bytes.
fn split_null(data: &[u8]) -> Vec<String> {
    data.split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// Split input on a specific single-byte delimiter.
fn split_delimiter(data: &[u8], delim: u8) -> Vec<String> {
    data.split(|&b| b == delim)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

/// Split input using default whitespace/newline rules with quote handling.
///
/// Handles:
/// - Single-quoted strings: content is literal (no escaping).
/// - Double-quoted strings: backslash escapes `"` and `\`.
/// - Backslash outside quotes: escapes the next character.
/// - Unquoted whitespace (space, tab, newline) separates arguments.
fn split_whitespace_quoted(data: &[u8]) -> Result<Vec<String>, i32> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut i = 0;

    while i < data.len() {
        let b = data[i];

        if in_single_quote {
            if b == b'\'' {
                in_single_quote = false;
            } else {
                current.push(char::from(b));
            }
            i += 1;
            continue;
        }

        if in_double_quote {
            if b == b'"' {
                in_double_quote = false;
            } else if b == b'\\' && i + 1 < data.len() {
                let next = data[i + 1];
                match next {
                    b'"' | b'\\' => {
                        current.push(char::from(next));
                        i += 1;
                    }
                    _ => {
                        // Backslash is literal if not before " or \.
                        current.push('\\');
                    }
                }
            } else {
                current.push(char::from(b));
            }
            i += 1;
            continue;
        }

        // Outside quotes.
        match b {
            b'\'' => {
                in_single_quote = true;
            }
            b'"' => {
                in_double_quote = true;
            }
            b'\\' => {
                if i + 1 < data.len() {
                    i += 1;
                    current.push(char::from(data[i]));
                }
                // Trailing backslash at EOF is silently ignored.
            }
            b' ' | b'\t' | b'\n' | b'\r' => {
                if !current.is_empty() {
                    items.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(char::from(b));
            }
        }
        i += 1;
    }

    if in_single_quote {
        eprintln!("xargs: unterminated single quote");
        return Err(EXIT_INTERNAL);
    }
    if in_double_quote {
        eprintln!("xargs: unterminated double quote");
        return Err(EXIT_INTERNAL);
    }

    if !current.is_empty() {
        items.push(current);
    }

    Ok(items)
}

/// Split the raw input into lines (preserving content within each line).
///
/// Used for `-L` mode when no `-d` or `-0` is given. Lines are split on
/// `\n`; each line's trailing whitespace is stripped.
fn split_lines(data: &[u8]) -> Vec<String> {
    data.split(|&b| b == b'\n')
        .map(|line| {
            let s = String::from_utf8_lossy(line).into_owned();
            s.trim_end().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

// ============================================================================
// Batching
// ============================================================================

/// A batch of arguments to pass in a single command invocation.
type Batch = Vec<String>;

/// Group parsed items into batches according to `-n` / `-s` limits.
///
/// Each batch contains at most `max_args` items and the total character
/// length (command + initial args + batch args, separated by spaces) does
/// not exceed `max_chars`.
fn batch_by_args(
    items: &[String],
    base_cmd: &[String],
    max_args: Option<usize>,
    max_chars: usize,
    exit_on_too_long: bool,
) -> Result<Vec<Batch>, i32> {
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let limit = max_args.unwrap_or(usize::MAX);
    let base_len: usize = base_cmd.iter().map(|s| s.len() + 1).sum();

    let mut batches: Vec<Batch> = Vec::new();
    let mut current_batch: Batch = Vec::new();
    let mut current_len = base_len;

    for item in items {
        let item_len = item.len() + 1; // +1 for the separating space

        // Would adding this item exceed the character limit?
        if !current_batch.is_empty() && current_len + item_len > max_chars {
            if exit_on_too_long {
                eprintln!(
                    "xargs: argument list too long ({} > {max_chars})",
                    current_len + item_len,
                );
                return Err(EXIT_TOO_LONG);
            }
            batches.push(std::mem::take(&mut current_batch));
            current_len = base_len;
        }

        // A single item that exceeds the limit on its own.
        if current_batch.is_empty()
            && base_len + item_len > max_chars
            && exit_on_too_long
        {
            eprintln!(
                "xargs: single argument too long ({} > {max_chars})",
                base_len + item_len,
            );
            return Err(EXIT_TOO_LONG);
        }
        // If the single item exceeds the limit but -x is not set, we still
        // include it -- the OS may accept a longer line.

        current_batch.push(item.clone());
        current_len += item_len;

        // Hit the per-batch argument count limit?
        if current_batch.len() >= limit {
            batches.push(std::mem::take(&mut current_batch));
            current_len = base_len;
        }
    }

    if !current_batch.is_empty() {
        batches.push(current_batch);
    }

    Ok(batches)
}

/// Group input lines into batches of at most `max_lines` lines each.
///
/// Each line may contain multiple whitespace-separated words; they are all
/// individual arguments in the resulting batch. The `-s` limit is also
/// respected.
fn batch_by_lines(
    lines: &[String],
    base_cmd: &[String],
    max_lines: usize,
    max_chars: usize,
    exit_on_too_long: bool,
) -> Result<Vec<Batch>, i32> {
    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let base_len: usize = base_cmd.iter().map(|s| s.len() + 1).sum();

    let mut batches: Vec<Batch> = Vec::new();
    let mut current_batch: Batch = Vec::new();
    let mut current_lines = 0usize;
    let mut current_len = base_len;

    for line in lines {
        // Split each line into words.
        let words: Vec<&str> = line.split_whitespace().collect();
        let words_len: usize = words.iter().map(|w| w.len() + 1).sum();

        // Would adding this line exceed limits?
        if current_lines >= max_lines
            || (!current_batch.is_empty() && current_len + words_len > max_chars)
        {
            if !current_batch.is_empty() {
                if current_len + words_len > max_chars && exit_on_too_long {
                    eprintln!("xargs: argument list too long");
                    return Err(EXIT_TOO_LONG);
                }
                batches.push(std::mem::take(&mut current_batch));
            }
            current_lines = 0;
            current_len = base_len;
        }

        for w in &words {
            current_batch.push((*w).to_string());
        }
        current_len += words_len;
        current_lines += 1;
    }

    if !current_batch.is_empty() {
        batches.push(current_batch);
    }

    Ok(batches)
}

// ============================================================================
// Command execution
// ============================================================================

/// Build the full command line from the base command and a batch of extra
/// arguments.
fn build_command_line(base_cmd: &[String], batch: &[String]) -> Vec<String> {
    let mut cmd = base_cmd.to_vec();
    cmd.extend_from_slice(batch);
    cmd
}

/// Build a command line in `-I` (replace) mode: for each input item, replace
/// every occurrence of `replace_str` in each element of `base_cmd`.
fn build_replace_command(base_cmd: &[String], replace_str: &str, item: &str) -> Vec<String> {
    base_cmd
        .iter()
        .map(|part| part.replace(replace_str, item))
        .collect()
}

/// Execute a single command line. Returns the exit code.
fn exec_command(cmd_line: &[String], verbose: bool) -> i32 {
    let Some(program) = cmd_line.first() else {
        return EXIT_SUCCESS;
    };

    if verbose {
        let line = cmd_line.join(" ");
        let _ = writeln!(io::stderr(), "{line}");
    }

    let mut cmd = process::Command::new(program);
    if cmd_line.len() > 1 {
        cmd.args(&cmd_line[1..]);
    }

    match cmd.status() {
        Ok(status) => {
            // Killed by signal returns None -- use 128 (no platform
            // signal info available).
            status.code().unwrap_or(128)
        }
        Err(e) => {
            let _ = writeln!(io::stderr(), "xargs: {program}: {e}");
            match e.kind() {
                io::ErrorKind::NotFound => EXIT_NOT_FOUND,
                io::ErrorKind::PermissionDenied => EXIT_NOT_EXECUTABLE,
                _ => EXIT_INTERNAL,
            }
        }
    }
}

/// Prompt the user interactively. Returns `true` if they respond `y` or `Y`.
fn prompt_user(cmd_line: &[String]) -> bool {
    let line = cmd_line.join(" ");
    let _ = write!(io::stderr(), "{line} ?...");
    let _ = io::stderr().flush();

    let mut response = String::new();
    if io::stdin().read_line(&mut response).is_err() {
        return false;
    }
    let trimmed = response.trim();
    trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes")
}

/// Merge an individual command exit code into the worst-so-far tracker.
fn merge_exit_code(worst: i32, code: i32) -> i32 {
    match code {
        0 => worst,
        // Fatal codes (126, 127) take precedence.
        EXIT_NOT_EXECUTABLE | EXIT_NOT_FOUND => code,
        // Non-zero but non-fatal: map to 123 if we don't already have worse.
        1..=125 => {
            if worst == EXIT_NOT_EXECUTABLE || worst == EXIT_NOT_FOUND {
                worst
            } else {
                EXIT_COMMAND_FAILED
            }
        }
        _ => {
            if worst == EXIT_SUCCESS {
                EXIT_COMMAND_FAILED
            } else {
                worst
            }
        }
    }
}

/// Execute a list of command-line batches, running up to `max_procs` in
/// parallel.
fn execute_batches(
    batches: &[Vec<String>],
    verbose: bool,
    interactive: bool,
    max_procs: usize,
) -> i32 {
    if max_procs <= 1 {
        // Sequential execution.
        let mut worst = EXIT_SUCCESS;
        for cmd_line in batches {
            if interactive && !prompt_user(cmd_line) {
                continue;
            }
            let code = exec_command(cmd_line, verbose);
            worst = merge_exit_code(worst, code);
            // Exit immediately on fatal error codes.
            if code == EXIT_NOT_FOUND || code == EXIT_NOT_EXECUTABLE {
                return worst;
            }
        }
        return worst;
    }

    // Parallel execution.
    let worst = Arc::new(Mutex::new(EXIT_SUCCESS));
    let mut remaining: Vec<Vec<String>> = batches.to_vec();

    // Process batches in chunks of max_procs.
    while !remaining.is_empty() {
        let chunk_size = remaining.len().min(max_procs);
        let chunk: Vec<Vec<String>> = remaining.drain(..chunk_size).collect();

        let handles: Vec<_> = chunk
            .into_iter()
            .map(|cmd_line| {
                let worst = Arc::clone(&worst);
                thread::spawn(move || {
                    let code = exec_command(&cmd_line, verbose);
                    let mut w = worst.lock().unwrap_or_else(|e| e.into_inner());
                    *w = merge_exit_code(*w, code);
                })
            })
            .collect();

        for h in handles {
            // If a thread panicked, treat it as an internal error.
            if h.join().is_err() {
                let mut w = worst.lock().unwrap_or_else(|e| e.into_inner());
                *w = merge_exit_code(*w, EXIT_INTERNAL);
            }
        }
    }

    let w = worst.lock().unwrap_or_else(|e| e.into_inner());
    *w
}

// ============================================================================
// JSON output
// ============================================================================

/// Print the command batches as a JSON array to stdout.
fn print_json(batches: &[Vec<String>]) {
    println!("[");
    for (i, cmd_line) in batches.iter().enumerate() {
        let comma = if i + 1 < batches.len() { "," } else { "" };
        let parts: Vec<String> = cmd_line.iter().map(|s| json_escape(s)).collect();
        println!("  [{} ]{comma}", parts.join(", "));
    }
    println!("]");
}

/// Escape a string for JSON output. Wraps the result in double quotes.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                for unit in c.encode_utf16(&mut [0u16; 2]) {
                    // Format as 4-digit hex.
                    let hex = format!("\\u{unit:04x}");
                    out.push_str(&hex);
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ============================================================================
// Help
// ============================================================================

fn print_help() {
    println!("OurOS xargs v{VERSION}");
    println!();
    println!("Build and execute command lines from standard input.");
    println!();
    println!("USAGE:");
    println!("  xargs [OPTIONS] [COMMAND [INITIAL-ARGS...]]");
    println!();
    println!("Reads items from stdin and appends them as arguments to COMMAND.");
    println!("If no COMMAND is given, `echo` is used.");
    println!();
    println!("OPTIONS:");
    println!("  -0, --null              Input items are NUL-delimited");
    println!("  -d CHAR, --delimiter=C  Use CHAR as input delimiter");
    println!("  -n N, --max-args=N      Use at most N arguments per invocation");
    println!("  -L N, --max-lines=N     Use at most N lines per invocation");
    println!("  -s N, --max-chars=N     Max command line length (default: 131072)");
    println!("  -I STR, --replace=STR   Replace STR in COMMAND with each input item");
    println!("  -p, --interactive       Prompt before each execution");
    println!("  -t, --verbose           Print each command before executing");
    println!("  -r, --no-run-if-empty   Don't run if stdin is empty");
    println!("  -P N, --max-procs=N     Run up to N commands in parallel");
    println!("  -x, --exit              Exit if command line exceeds -s limit");
    println!("      --json              Print commands as JSON (don't execute)");
    println!("      --help              Display this help and exit");
    println!("      --version           Display version and exit");
    println!();
    println!("EXAMPLES:");
    println!("  find /tmp -name '*.log' | xargs rm");
    println!("  echo 'a b c' | xargs -n 1 echo item:");
    println!("  find . -print0 | xargs -0 grep TODO");
    println!("  echo f1 f2 | xargs -I {{}} cp {{}} /backup/{{}}");
    println!();
    println!("EXIT STATUS:");
    println!("  0     All commands succeeded");
    println!("  123   One or more commands returned non-zero (1-125)");
    println!("  124   Command line too long (-x)");
    println!("  125   Internal error");
    println!("  126   Command found but not executable");
    println!("  127   Command not found");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = match parse_args(&args) {
        Ok(c) => c,
        Err(code) => process::exit(code),
    };

    let exit_code = run(config);
    process::exit(exit_code);
}

/// Top-level logic: read stdin, parse, batch, execute.
fn run(config: Config) -> i32 {
    // Read all of stdin.
    let data = match read_stdin() {
        Ok(d) => d,
        Err(code) => return code,
    };

    // -I (replace) mode: each input item triggers a separate command
    // invocation with replacement.
    if let Some(ref replace_str) = config.replace_str {
        return run_replace_mode(&config, &data, replace_str);
    }

    // -L (max-lines) mode: batch by lines, not by individual words.
    if let Some(max_lines) = config.max_lines {
        return run_line_mode(&config, &data, max_lines);
    }

    // Default / -n mode: split into items, batch by count and size.
    run_arg_mode(&config, &data)
}

/// Execute in `-I` (replace) mode.
fn run_replace_mode(config: &Config, data: &[u8], replace_str: &str) -> i32 {
    let items = parse_items(config, data);
    let items = match items {
        Ok(v) => v,
        Err(code) => return code,
    };

    if items.is_empty() && config.no_run_if_empty {
        return EXIT_SUCCESS;
    }
    if items.is_empty() {
        // With -I and no input, run the command once with the replace
        // string unmodified (matching GNU xargs behavior of not running
        // at all).
        return EXIT_SUCCESS;
    }

    // Build one command per input item.
    let cmd_lines: Vec<Vec<String>> = items
        .iter()
        .map(|item| build_replace_command(&config.command, replace_str, item))
        .collect();

    if config.json {
        print_json(&cmd_lines);
        return EXIT_SUCCESS;
    }

    execute_batches(&cmd_lines, config.verbose, config.interactive, config.max_procs)
}

/// Execute in `-L` (max-lines) mode.
fn run_line_mode(config: &Config, data: &[u8], max_lines: usize) -> i32 {
    let lines = if config.null_delimited {
        split_null(data)
    } else if let Some(delim) = config.delimiter {
        split_delimiter(data, delim)
    } else {
        split_lines(data)
    };

    if lines.is_empty() && config.no_run_if_empty {
        return EXIT_SUCCESS;
    }
    if lines.is_empty() {
        // Run command once with no extra args (default behavior).
        let cmd_line = config.command.clone();
        if config.json {
            print_json(&[cmd_line]);
            return EXIT_SUCCESS;
        }
        if config.interactive && !prompt_user(&cmd_line) {
            return EXIT_SUCCESS;
        }
        return exec_command(&cmd_line, config.verbose);
    }

    let batches = match batch_by_lines(
        &lines,
        &config.command,
        max_lines,
        config.max_chars,
        config.exit_on_too_long,
    ) {
        Ok(b) => b,
        Err(code) => return code,
    };

    // Build full command lines.
    let cmd_lines: Vec<Vec<String>> = batches
        .iter()
        .map(|batch| build_command_line(&config.command, batch))
        .collect();

    if config.json {
        print_json(&cmd_lines);
        return EXIT_SUCCESS;
    }

    execute_batches(&cmd_lines, config.verbose, config.interactive, config.max_procs)
}

/// Execute in default / `-n` (max-args) mode.
fn run_arg_mode(config: &Config, data: &[u8]) -> i32 {
    let items = parse_items(config, data);
    let items = match items {
        Ok(v) => v,
        Err(code) => return code,
    };

    if items.is_empty() && config.no_run_if_empty {
        return EXIT_SUCCESS;
    }
    if items.is_empty() {
        // Run command once with no extra args.
        let cmd_line = config.command.clone();
        if config.json {
            print_json(&[cmd_line]);
            return EXIT_SUCCESS;
        }
        if config.interactive && !prompt_user(&cmd_line) {
            return EXIT_SUCCESS;
        }
        return exec_command(&cmd_line, config.verbose);
    }

    let batches = match batch_by_args(
        &items,
        &config.command,
        config.max_args,
        config.max_chars,
        config.exit_on_too_long,
    ) {
        Ok(b) => b,
        Err(code) => return code,
    };

    let cmd_lines: Vec<Vec<String>> = batches
        .iter()
        .map(|batch| build_command_line(&config.command, batch))
        .collect();

    if config.json {
        print_json(&cmd_lines);
        return EXIT_SUCCESS;
    }

    execute_batches(&cmd_lines, config.verbose, config.interactive, config.max_procs)
}

/// Parse input items from raw data according to the delimiter configuration.
fn parse_items(config: &Config, data: &[u8]) -> Result<Vec<String>, i32> {
    if config.null_delimited {
        Ok(split_null(data))
    } else if let Some(delim) = config.delimiter {
        Ok(split_delimiter(data, delim))
    } else {
        split_whitespace_quoted(data)
    }
}
