//! SlateOS `head` / `tail` Utility -- Output First or Last Part of Files
//!
//! A combined head+tail binary that detects its invocation name via `argv[0]`.
//! When invoked as `tail`, it outputs the last part of files; when invoked as
//! `head` (or any other name), it outputs the first part.
//!
//! # Usage (head mode)
//!
//! ```text
//! head [OPTION]... [FILE]...
//!
//! Print the first 10 lines of each FILE to standard output.
//! With more than one FILE, precede each with a header giving the file name.
//!
//!   -n, --lines=N           Print the first N lines (default 10)
//!   -n -N                   Print all but the last N lines
//!   -c, --bytes=N           Print the first N bytes
//!   -c -N                   Print all but the last N bytes
//!   -q, --quiet, --silent   Never print headers giving file names
//!   -v, --verbose           Always print headers giving file names
//!   -z, --zero-terminated   Line delimiter is NUL, not newline
//!       --help              Display this help and exit
//!       --version           Output version information and exit
//! ```
//!
//! # Usage (tail mode)
//!
//! ```text
//! tail [OPTION]... [FILE]...
//!
//! Print the last 10 lines of each FILE to standard output.
//!
//!   -n, --lines=N           Print the last N lines (default 10)
//!   -n +N                   Output starting with line N
//!   -c, --bytes=N           Print the last N bytes
//!   -c +N                   Output starting with byte N
//!   -f, --follow            Output appended data as the file grows
//!   -F                      Like -f, but retry if file is replaced
//!   --pid=PID               With -f, terminate after process PID dies
//!   -s, --sleep-interval=N  With -f, sleep N seconds between polls (default 1.0)
//!   -q, --quiet, --silent   Never print headers giving file names
//!   -v, --verbose           Always print headers giving file names
//!   -z, --zero-terminated   Line delimiter is NUL, not newline
//!       --help              Display this help and exit
//!       --version           Output version information and exit
//! ```

use std::collections::VecDeque;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process;
use std::thread;
use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Default number of lines to output when no `-n` is given.
const DEFAULT_LINES: u64 = 10;

/// Read buffer size for byte-mode operations.
const BUF_SIZE: usize = 8192;

/// Default sleep interval for `tail -f`, in seconds.
const DEFAULT_FOLLOW_SLEEP: f64 = 1.0;

// ============================================================================
// Mode and configuration types
// ============================================================================

/// Whether the binary was invoked as `head` or `tail`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Head,
    Tail,
}

/// Whether the user specified a count in lines or bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CountMode {
    Lines,
    Bytes,
}

/// The count value, which can have a sign prefix in some modes.
///
/// - For head: positive N means "first N"; negative N means "all but last N".
/// - For tail: positive N means "last N"; the `+` prefix means "starting from N"
///   (1-indexed), stored as `FromStart(N)`.
#[derive(Clone, Copy)]
enum CountValue {
    /// A plain count (head: first N; tail: last N).
    Plain(u64),
    /// head only: output all but the last N items.
    AllButLast(u64),
    /// tail only: output starting from the Nth item (1-indexed).
    FromStart(u64),
}

/// How to handle filename headers between files.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HeaderMode {
    /// Print headers when there are multiple files.
    Auto,
    /// Always print headers.
    Always,
    /// Never print headers.
    Never,
}

/// Follow mode for tail.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FollowMode {
    /// Do not follow.
    None,
    /// Follow by file descriptor (keep reading the same fd).
    Descriptor,
    /// Follow by name (reopen the file if it is replaced).
    Name,
}

/// Fully parsed command-line configuration.
struct Config {
    tool: Tool,
    count_mode: CountMode,
    count_value: CountValue,
    header_mode: HeaderMode,
    zero_terminated: bool,
    follow: FollowMode,
    follow_pid: Option<u32>,
    follow_sleep: f64,
    file_paths: Vec<String>,
}

/// Result of argument parsing.
enum ParseResult {
    Run(Config),
    Help,
    Version,
}

// ============================================================================
// Size suffix parsing
// ============================================================================

/// Parse a numeric string that may end with a size suffix (K, M, G).
/// K = 1024, M = 1024^2, G = 1024^3.
/// Also supports lowercase (k, m, g) and the `b` suffix (512-byte blocks).
/// Returns `None` if the string is not a valid number.
fn parse_size(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }

    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Determine suffix and multiplier.
    let (num_part, multiplier) = if let Some(prefix) = s.strip_suffix('K') {
        (prefix, 1024u64)
    } else if let Some(prefix) = s.strip_suffix('k') {
        (prefix, 1024u64)
    } else if let Some(prefix) = s.strip_suffix('M') {
        (prefix, 1024u64 * 1024)
    } else if let Some(prefix) = s.strip_suffix('m') {
        (prefix, 1024u64 * 1024)
    } else if let Some(prefix) = s.strip_suffix('G') {
        (prefix, 1024u64 * 1024 * 1024)
    } else if let Some(prefix) = s.strip_suffix('g') {
        (prefix, 1024u64 * 1024 * 1024)
    } else if let Some(prefix) = s.strip_suffix('b') {
        (prefix, 512u64)
    } else {
        (s, 1u64)
    };

    let n: u64 = num_part.parse().ok()?;
    n.checked_mul(multiplier)
}

/// Parse a count argument, which may have a leading `+` or `-` sign, and an
/// optional size suffix. Returns `(absolute_value, sign_char)` where sign_char
/// is `'+'`, `'-'`, or `' '` for no sign.
fn parse_count_arg(s: &str) -> Result<(u64, char), String> {
    if s.is_empty() {
        return Err("empty count".to_string());
    }

    let first = s.as_bytes()[0];
    let (sign, rest) = match first {
        b'+' => ('+', &s[1..]),
        b'-' => ('-', &s[1..]),
        _ => (' ', s),
    };

    match parse_size(rest) {
        Some(val) => Ok((val, sign)),
        None => Err(format!("invalid number: '{s}'")),
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

fn detect_tool(args: &[String]) -> Tool {
    if let Some(arg0) = args.first() {
        let basename = Path::new(arg0)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("head");
        if basename == "tail" {
            return Tool::Tail;
        }
    }
    Tool::Head
}

fn parse_args(args: &[String]) -> ParseResult {
    let tool = detect_tool(args);
    let tool_name = match tool {
        Tool::Head => "head",
        Tool::Tail => "tail",
    };

    let mut count_mode = CountMode::Lines;
    let mut count_value: Option<CountValue> = None;
    let mut header_mode = HeaderMode::Auto;
    let mut zero_terminated = false;
    let mut follow = FollowMode::None;
    let mut follow_pid: Option<u32> = None;
    let mut follow_sleep = DEFAULT_FOLLOW_SLEEP;
    let mut file_paths: Vec<String> = Vec::new();
    let mut end_of_opts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') {
            file_paths.push(arg.clone());
            i += 1;
            continue;
        }

        // `-` alone means stdin.
        if arg == "-" {
            file_paths.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // Long options.
        if arg.starts_with("--") {
            if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--quiet" || arg == "--silent" {
                header_mode = HeaderMode::Never;
            } else if arg == "--verbose" {
                header_mode = HeaderMode::Always;
            } else if arg == "--zero-terminated" {
                zero_terminated = true;
            } else if arg == "--lines" || arg.starts_with("--lines=") {
                let val_str = if let Some(eq_val) = arg.strip_prefix("--lines=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{tool_name}: option '--lines' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                count_mode = CountMode::Lines;
                count_value = Some(parse_count_value(tool, &val_str, tool_name));
            } else if arg == "--bytes" || arg.starts_with("--bytes=") {
                let val_str = if let Some(eq_val) = arg.strip_prefix("--bytes=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{tool_name}: option '--bytes' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                count_mode = CountMode::Bytes;
                count_value = Some(parse_count_value(tool, &val_str, tool_name));
            } else if arg == "--follow" || arg.starts_with("--follow=") {
                if tool != Tool::Tail {
                    eprintln!("{tool_name}: unrecognized option '--follow'");
                    process::exit(1);
                }
                if let Some(eq_val) = arg.strip_prefix("--follow=") {
                    match eq_val {
                        "name" => follow = FollowMode::Name,
                        "descriptor" => follow = FollowMode::Descriptor,
                        _ => {
                            eprintln!("{tool_name}: invalid argument '{eq_val}' for '--follow'");
                            process::exit(1);
                        }
                    }
                } else {
                    follow = FollowMode::Descriptor;
                }
            } else if arg == "--pid" || arg.starts_with("--pid=") {
                if tool != Tool::Tail {
                    eprintln!("{tool_name}: unrecognized option '--pid'");
                    process::exit(1);
                }
                let val_str = if let Some(eq_val) = arg.strip_prefix("--pid=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{tool_name}: option '--pid' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                match val_str.parse::<u32>() {
                    Ok(pid) => follow_pid = Some(pid),
                    Err(_) => {
                        eprintln!("{tool_name}: invalid PID: '{val_str}'");
                        process::exit(1);
                    }
                }
            } else if arg == "--sleep-interval" || arg.starts_with("--sleep-interval=") {
                if tool != Tool::Tail {
                    eprintln!("{tool_name}: unrecognized option '--sleep-interval'");
                    process::exit(1);
                }
                let val_str = if let Some(eq_val) = arg.strip_prefix("--sleep-interval=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{tool_name}: option '--sleep-interval' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                match val_str.parse::<f64>() {
                    Ok(secs) if secs >= 0.0 => follow_sleep = secs,
                    _ => {
                        eprintln!("{tool_name}: invalid sleep interval: '{val_str}'");
                        process::exit(1);
                    }
                }
            } else {
                eprintln!("{tool_name}: unrecognized option '{arg}'");
                eprintln!("Try '{tool_name} --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options. Some take a value argument (n, c, s), so we need
        // careful handling when they appear in a combined group.
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut ci = 0;
        while ci < chars.len() {
            let ch = chars[ci];
            match ch {
                'n' => {
                    // The rest of this arg (if any) is the value; otherwise
                    // consume the next argument.
                    let val_str = rest_or_next_arg(&chars, ci, args, &mut i, tool_name, 'n');
                    count_mode = CountMode::Lines;
                    count_value = Some(parse_count_value(tool, &val_str, tool_name));
                    ci = chars.len(); // consumed the rest
                }
                'c' => {
                    let val_str = rest_or_next_arg(&chars, ci, args, &mut i, tool_name, 'c');
                    count_mode = CountMode::Bytes;
                    count_value = Some(parse_count_value(tool, &val_str, tool_name));
                    ci = chars.len();
                }
                'q' => header_mode = HeaderMode::Never,
                'v' => header_mode = HeaderMode::Always,
                'z' => zero_terminated = true,
                'f' => {
                    if tool != Tool::Tail {
                        eprintln!("{tool_name}: invalid option -- 'f'");
                        process::exit(1);
                    }
                    follow = FollowMode::Descriptor;
                }
                'F' => {
                    if tool != Tool::Tail {
                        eprintln!("{tool_name}: invalid option -- 'F'");
                        process::exit(1);
                    }
                    follow = FollowMode::Name;
                }
                's' => {
                    if tool != Tool::Tail {
                        eprintln!("{tool_name}: invalid option -- 's'");
                        process::exit(1);
                    }
                    let val_str = rest_or_next_arg(&chars, ci, args, &mut i, tool_name, 's');
                    match val_str.parse::<f64>() {
                        Ok(secs) if secs >= 0.0 => follow_sleep = secs,
                        _ => {
                            eprintln!("{tool_name}: invalid sleep interval: '{val_str}'");
                            process::exit(1);
                        }
                    }
                    ci = chars.len();
                }
                _ => {
                    eprintln!("{tool_name}: invalid option -- '{ch}'");
                    eprintln!("Try '{tool_name} --help' for more information.");
                    process::exit(1);
                }
            }
            ci += 1;
        }

        i += 1;
    }

    // Apply default count if none was specified.
    let count_value = count_value.unwrap_or(CountValue::Plain(DEFAULT_LINES));

    ParseResult::Run(Config {
        tool,
        count_mode,
        count_value,
        header_mode,
        zero_terminated,
        follow,
        follow_pid,
        follow_sleep,
        file_paths,
    })
}

/// For short options that take a value: if there are more characters remaining
/// in the current option group, those characters are the value. Otherwise,
/// consume the next argument.
fn rest_or_next_arg(
    chars: &[char],
    current_idx: usize,
    args: &[String],
    arg_idx: &mut usize,
    tool_name: &str,
    opt_char: char,
) -> String {
    if current_idx + 1 < chars.len() {
        // Remaining chars in this group are the value.
        chars[current_idx + 1..].iter().collect()
    } else {
        // Consume the next argument.
        *arg_idx += 1;
        if *arg_idx >= args.len() {
            eprintln!("{tool_name}: option requires an argument -- '{opt_char}'");
            process::exit(1);
        }
        args[*arg_idx].clone()
    }
}

/// Parse a count value string, interpreting sign prefixes according to the
/// tool mode.
fn parse_count_value(tool: Tool, s: &str, tool_name: &str) -> CountValue {
    match parse_count_arg(s) {
        Ok((val, sign)) => match (tool, sign) {
            // head: `-N` means all but last N.
            (Tool::Head, '-') => CountValue::AllButLast(val),
            // head: `+N` or plain N means first N.
            (Tool::Head, _) => CountValue::Plain(val),
            // tail: `+N` means starting from line/byte N.
            (Tool::Tail, '+') => CountValue::FromStart(val),
            // tail: `-N` or plain N means last N.
            (Tool::Tail, _) => CountValue::Plain(val),
        },
        Err(e) => {
            eprintln!("{tool_name}: {e}");
            process::exit(1);
        }
    }
}

// ============================================================================
// Core head operations
// ============================================================================

/// Output the first `n` lines from a reader. Stops reading as soon as the
/// required lines have been emitted, so this is efficient for large files.
fn head_lines<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    n: u64,
    delimiter: u8,
) -> io::Result<()> {
    let mut remaining = n;
    if remaining == 0 {
        return Ok(());
    }

    let mut line_buf = Vec::with_capacity(256);
    loop {
        line_buf.clear();
        let bytes_read = read_until_byte(reader, delimiter, &mut line_buf)?;
        if bytes_read == 0 {
            break; // EOF
        }
        writer.write_all(&line_buf)?;
        remaining -= 1;
        if remaining == 0 {
            break;
        }
    }
    Ok(())
}

/// Output the first `n` bytes from a reader.
fn head_bytes<R: Read, W: Write>(reader: &mut R, writer: &mut W, n: u64) -> io::Result<()> {
    let mut remaining = n;
    let mut buf = [0u8; BUF_SIZE];

    while remaining > 0 {
        let to_read = BUF_SIZE.min(remaining as usize);
        let bytes_read = reader.read(&mut buf[..to_read])?;
        if bytes_read == 0 {
            break;
        }
        writer.write_all(&buf[..bytes_read])?;
        remaining -= bytes_read as u64;
    }
    Ok(())
}

/// Output all but the last `n` lines from a reader. Reads the entire input
/// into a ring buffer of lines, delaying output by `n` lines.
fn head_all_but_last_lines<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    n: u64,
    delimiter: u8,
) -> io::Result<()> {
    if n == 0 {
        // "All but last 0" = output everything.
        let mut buf = [0u8; BUF_SIZE];
        loop {
            let bytes_read = reader.read(&mut buf)?;
            if bytes_read == 0 {
                break;
            }
            writer.write_all(&buf[..bytes_read])?;
        }
        return Ok(());
    }

    let cap = n as usize;
    let mut ring: VecDeque<Vec<u8>> = VecDeque::with_capacity(cap);
    let mut line_buf = Vec::with_capacity(256);

    loop {
        line_buf.clear();
        let bytes_read = read_until_byte(reader, delimiter, &mut line_buf)?;
        if bytes_read == 0 {
            break;
        }

        if ring.len() == cap {
            // Ring is full: output the oldest line to make room.
            if let Some(old) = ring.pop_front() {
                writer.write_all(&old)?;
            }
        }
        ring.push_back(line_buf.clone());
    }

    // The lines remaining in the ring are the last N lines, which we discard.
    Ok(())
}

/// Output all but the last `n` bytes from a reader. Reads all data, then
/// outputs everything except the final `n` bytes.
fn head_all_but_last_bytes<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    n: u64,
) -> io::Result<()> {
    if n == 0 {
        let mut buf = [0u8; BUF_SIZE];
        loop {
            let bytes_read = reader.read(&mut buf)?;
            if bytes_read == 0 {
                break;
            }
            writer.write_all(&buf[..bytes_read])?;
        }
        return Ok(());
    }

    // Read the entire input, then output all but the last n bytes.
    // For very large files this is memory-intensive, but it is the simplest
    // correct approach. A streaming ring-buffer of byte chunks could reduce
    // memory usage but adds significant complexity.
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;

    let total = data.len() as u64;
    if total > n {
        writer.write_all(&data[..(total - n) as usize])?;
    }
    Ok(())
}

// ============================================================================
// Core tail operations
// ============================================================================

/// Output the last `n` lines from a reader. Uses a ring buffer so only the
/// last N lines are held in memory at any time.
fn tail_last_lines<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    n: u64,
    delimiter: u8,
) -> io::Result<()> {
    if n == 0 {
        // Consume input, output nothing.
        let mut discard = [0u8; BUF_SIZE];
        while reader.read(&mut discard)? > 0 {}
        return Ok(());
    }

    let cap = n as usize;
    let mut ring: VecDeque<Vec<u8>> = VecDeque::with_capacity(cap);
    let mut line_buf = Vec::with_capacity(256);

    loop {
        line_buf.clear();
        let bytes_read = read_until_byte(reader, delimiter, &mut line_buf)?;
        if bytes_read == 0 {
            break;
        }
        if ring.len() == cap {
            ring.pop_front();
        }
        ring.push_back(line_buf.clone());
    }

    for line in &ring {
        writer.write_all(line)?;
    }
    Ok(())
}

/// Output the last `n` bytes from a reader.
fn tail_last_bytes<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    n: u64,
) -> io::Result<()> {
    if n == 0 {
        let mut discard = [0u8; BUF_SIZE];
        while reader.read(&mut discard)? > 0 {}
        return Ok(());
    }

    // Ring buffer of byte chunks. We keep up to `n` bytes in the ring.
    let cap = n as usize;
    let mut ring: VecDeque<u8> = VecDeque::with_capacity(cap);
    let mut buf = [0u8; BUF_SIZE];

    loop {
        let bytes_read = reader.read(&mut buf)?;
        if bytes_read == 0 {
            break;
        }
        for &byte in &buf[..bytes_read] {
            if ring.len() == cap {
                ring.pop_front();
            }
            ring.push_back(byte);
        }
    }

    let data: Vec<u8> = ring.into_iter().collect();
    writer.write_all(&data)?;
    Ok(())
}

/// Output starting from line N (1-indexed). Skips the first N-1 lines, then
/// outputs everything remaining.
fn tail_from_line<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    start: u64,
    delimiter: u8,
) -> io::Result<()> {
    // Skip the first (start - 1) lines. If start is 0 or 1, output everything.
    let skip = start.saturating_sub(1);
    let mut line_buf = Vec::with_capacity(256);

    for _ in 0..skip {
        line_buf.clear();
        let bytes_read = read_until_byte(reader, delimiter, &mut line_buf)?;
        if bytes_read == 0 {
            return Ok(()); // File shorter than start lines
        }
    }

    // Output everything remaining.
    let mut buf = [0u8; BUF_SIZE];
    loop {
        let bytes_read = reader.read(&mut buf)?;
        if bytes_read == 0 {
            break;
        }
        writer.write_all(&buf[..bytes_read])?;
    }
    Ok(())
}

/// Output starting from byte N (1-indexed). Skips the first N-1 bytes.
fn tail_from_byte<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    start: u64,
) -> io::Result<()> {
    let skip = start.saturating_sub(1);
    let mut buf = [0u8; BUF_SIZE];
    let mut skipped: u64 = 0;

    // Skip bytes.
    while skipped < skip {
        let to_read = BUF_SIZE.min((skip - skipped) as usize);
        let bytes_read = reader.read(&mut buf[..to_read])?;
        if bytes_read == 0 {
            return Ok(()); // File shorter than start bytes
        }
        skipped += bytes_read as u64;
    }

    // Output everything remaining.
    loop {
        let bytes_read = reader.read(&mut buf)?;
        if bytes_read == 0 {
            break;
        }
        writer.write_all(&buf[..bytes_read])?;
    }
    Ok(())
}

// ============================================================================
// Follow mode (tail -f / -F)
// ============================================================================

/// Follow a file, outputting new data as it is appended. For `-f` (descriptor
/// mode), we keep the file handle open. For `-F` (name mode), we reopen the
/// file if it is replaced (detected by size shrinking or inode change).
fn follow_file<W: Write>(
    path: &str,
    writer: &mut W,
    sleep_secs: f64,
    mode: FollowMode,
    _pid: Option<u32>,
) -> io::Result<()> {
    let sleep_dur = Duration::from_secs_f64(sleep_secs);

    if path == "-" {
        // Follow stdin: just keep reading.
        let stdin = io::stdin();
        let mut locked = stdin.lock();
        let mut buf = [0u8; BUF_SIZE];
        loop {
            let n = locked.read(&mut buf)?;
            if n > 0 {
                writer.write_all(&buf[..n])?;
                writer.flush()?;
            } else {
                // TODO: check PID if specified (requires SlateOS process query API)
                thread::sleep(sleep_dur);
            }
        }
    }

    let mut file = File::open(path)?;
    // Seek to end so we only output new data.
    let mut pos = file.seek(SeekFrom::End(0))?;
    let mut buf = [0u8; BUF_SIZE];

    loop {
        let n = file.read(&mut buf)?;
        if n > 0 {
            writer.write_all(&buf[..n])?;
            writer.flush()?;
            pos += n as u64;
        } else {
            // No new data. Check if we should reopen (name mode).
            if mode == FollowMode::Name {
                // Check if the file was replaced by looking at its length.
                match File::open(path) {
                    Ok(mut new_file) => {
                        let new_len = new_file.seek(SeekFrom::End(0))?;
                        if new_len < pos {
                            // File was truncated or replaced. Start from the
                            // beginning of the new file.
                            let _ = new_file.seek(SeekFrom::Start(0))?;
                            file = new_file;
                            pos = 0;
                            continue;
                        }
                        // Otherwise the file might have grown via the new fd.
                        // Re-seek our existing fd to where we were.
                    }
                    Err(_) => {
                        // File disappeared. Keep trying.
                    }
                }
            }

            // TODO: check PID if specified (requires SlateOS process query API)
            thread::sleep(sleep_dur);
        }
    }
}

// ============================================================================
// Utility: read_until_byte (like BufRead::read_until but works on any delimiter)
// ============================================================================

/// Read from `reader` until `delimiter` byte is found (inclusive) or EOF.
/// Appends to `buf` and returns the number of bytes read (0 at EOF).
fn read_until_byte<R: BufRead>(reader: &mut R, delimiter: u8, buf: &mut Vec<u8>) -> io::Result<usize> {
    // BufRead::read_until does exactly this.
    reader.read_until(delimiter, buf)
}

// ============================================================================
// File processing
// ============================================================================

/// Process a single file (or stdin) according to the configuration.
fn process_source<W: Write>(
    config: &Config,
    path: &str,
    writer: &mut W,
) -> io::Result<()> {
    let delimiter = if config.zero_terminated { 0u8 } else { b'\n' };

    if path == "-" {
        let stdin = io::stdin();
        let mut locked = BufReader::new(stdin.lock());
        dispatch_operation(config, &mut locked, writer, delimiter)
    } else {
        let file = File::open(path).map_err(|e| {
            io::Error::new(e.kind(), format!("{path}: {e}"))
        })?;
        let mut reader = BufReader::with_capacity(BUF_SIZE, file);
        dispatch_operation(config, &mut reader, writer, delimiter)
    }
}

/// Dispatch to the correct head/tail operation based on config.
fn dispatch_operation<R: BufRead, W: Write>(
    config: &Config,
    reader: &mut R,
    writer: &mut W,
    delimiter: u8,
) -> io::Result<()> {
    match config.tool {
        Tool::Head => match (config.count_mode, config.count_value) {
            (CountMode::Lines, CountValue::Plain(n)) => {
                head_lines(reader, writer, n, delimiter)
            }
            (CountMode::Lines, CountValue::AllButLast(n)) => {
                head_all_but_last_lines(reader, writer, n, delimiter)
            }
            (CountMode::Bytes, CountValue::Plain(n)) => {
                head_bytes(reader, writer, n)
            }
            (CountMode::Bytes, CountValue::AllButLast(n)) => {
                head_all_but_last_bytes(reader, writer, n)
            }
            // FromStart is tail-only; for head, treat as plain.
            (CountMode::Lines, CountValue::FromStart(n)) => {
                head_lines(reader, writer, n, delimiter)
            }
            (CountMode::Bytes, CountValue::FromStart(n)) => {
                head_bytes(reader, writer, n)
            }
        },
        Tool::Tail => match (config.count_mode, config.count_value) {
            (CountMode::Lines, CountValue::Plain(n)) => {
                tail_last_lines(reader, writer, n, delimiter)
            }
            (CountMode::Lines, CountValue::FromStart(n)) => {
                tail_from_line(reader, writer, n, delimiter)
            }
            (CountMode::Bytes, CountValue::Plain(n)) => {
                tail_last_bytes(reader, writer, n)
            }
            (CountMode::Bytes, CountValue::FromStart(n)) => {
                tail_from_byte(reader, writer, n)
            }
            // AllButLast is head-only; for tail, treat as plain last N.
            (CountMode::Lines, CountValue::AllButLast(n)) => {
                tail_last_lines(reader, writer, n, delimiter)
            }
            (CountMode::Bytes, CountValue::AllButLast(n)) => {
                tail_last_bytes(reader, writer, n)
            }
        },
    }
}

// ============================================================================
// Header printing
// ============================================================================

/// Print a filename header in the standard format: `==> filename <==`.
fn print_header<W: Write>(writer: &mut W, name: &str, is_first: bool) -> io::Result<()> {
    if !is_first {
        writeln!(writer)?;
    }
    writeln!(writer, "==> {name} <==")?;
    Ok(())
}

/// Determine the display name for a source. `-` becomes `standard input`.
fn display_name(path: &str) -> &str {
    if path == "-" {
        "standard input"
    } else {
        path
    }
}

// ============================================================================
// Main driver
// ============================================================================

/// Process all files and produce output. Returns the exit code.
fn run(config: &Config) -> i32 {
    let tool_name = match config.tool {
        Tool::Head => "head",
        Tool::Tail => "tail",
    };

    let sources: Vec<String> = if config.file_paths.is_empty() {
        vec!["-".to_string()]
    } else {
        config.file_paths.clone()
    };

    let show_headers = match config.header_mode {
        HeaderMode::Always => true,
        HeaderMode::Never => false,
        HeaderMode::Auto => sources.len() > 1,
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut exit_code = 0;

    for (idx, path) in sources.iter().enumerate() {
        if show_headers
            && let Err(e) = print_header(&mut out, display_name(path), idx == 0) {
                if is_broken_pipe(&e) {
                    return 0;
                }
                eprintln!("{tool_name}: write error: {e}");
                return 1;
            }

        if let Err(e) = process_source(config, path, &mut out) {
            if is_broken_pipe(&e) {
                return 0;
            }
            eprintln!("{tool_name}: {e}");
            exit_code = 1;
        }
    }

    // Flush stdout before potentially entering follow mode.
    if let Err(e) = out.flush() {
        if !is_broken_pipe(&e) {
            eprintln!("{tool_name}: write error: {e}");
            return 1;
        }
        return 0;
    }

    // Follow mode (tail only). Only follows the last file.
    if config.follow != FollowMode::None && config.tool == Tool::Tail
        && let Some(last_path) = sources.last()
            && let Err(e) = follow_file(
                last_path,
                &mut out,
                config.follow_sleep,
                config.follow,
                config.follow_pid,
            ) {
                if is_broken_pipe(&e) {
                    return 0;
                }
                eprintln!("{tool_name}: {e}");
                return 1;
            }

    exit_code
}

/// Check if an I/O error is a broken pipe.
fn is_broken_pipe(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::BrokenPipe
}

// ============================================================================
// Help text
// ============================================================================

fn print_help_head() {
    println!("SlateOS head v{VERSION}");
    println!();
    println!("Print the first 10 lines of each FILE to standard output.");
    println!("With more than one FILE, precede each with a header giving the file name.");
    println!();
    println!("USAGE:");
    println!("  head [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -n, --lines=N           Print the first N lines (default 10)");
    println!("      -n -N               Print all but the last N lines");
    println!("  -c, --bytes=N           Print the first N bytes");
    println!("      -c -N               Print all but the last N bytes");
    println!("  -q, --quiet, --silent   Never print headers giving file names");
    println!("  -v, --verbose           Always print headers giving file names");
    println!("  -z, --zero-terminated   Line delimiter is NUL, not newline");
    println!("      --help              Display this help and exit");
    println!("      --version           Output version information and exit");
    println!();
    println!("N may have a suffix: K (x1024), M (x1048576), G (x1073741824).");
    println!();
    println!("With no FILE, or when FILE is -, read standard input.");
}

fn print_help_tail() {
    println!("SlateOS tail v{VERSION}");
    println!();
    println!("Print the last 10 lines of each FILE to standard output.");
    println!("With more than one FILE, precede each with a header giving the file name.");
    println!();
    println!("USAGE:");
    println!("  tail [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -n, --lines=N             Print the last N lines (default 10)");
    println!("      -n +N                 Output starting with line N");
    println!("  -c, --bytes=N             Print the last N bytes");
    println!("      -c +N                 Output starting with byte N");
    println!("  -f, --follow[=HOW]        Output appended data as the file grows;");
    println!("                            HOW is 'descriptor' (default) or 'name'");
    println!("  -F                        Same as --follow=name --retry");
    println!("  --pid=PID                 With -f, terminate after process PID dies");
    println!("  -s, --sleep-interval=N    With -f, sleep approximately N seconds");
    println!("                            between iterations (default 1.0)");
    println!("  -q, --quiet, --silent     Never print headers giving file names");
    println!("  -v, --verbose             Always print headers giving file names");
    println!("  -z, --zero-terminated     Line delimiter is NUL, not newline");
    println!("      --help                Display this help and exit");
    println!("      --version             Output version information and exit");
    println!();
    println!("N may have a suffix: K (x1024), M (x1048576), G (x1073741824).");
    println!();
    println!("With no FILE, or when FILE is -, read standard input.");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let tool = detect_tool(&args);

    match parse_args(&args) {
        ParseResult::Help => {
            match tool {
                Tool::Head => print_help_head(),
                Tool::Tail => print_help_tail(),
            }
            process::exit(0);
        }
        ParseResult::Version => {
            let name = match tool {
                Tool::Head => "head",
                Tool::Tail => "tail",
            };
            println!("{name} (SlateOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let code = run(&config);
            process::exit(code);
        }
    }
}
