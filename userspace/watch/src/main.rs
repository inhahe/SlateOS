//! Slate OS Watch Utility -- Execute a Command Periodically
//!
//! Runs a command repeatedly at a configurable interval, displaying its output
//! in a full-screen terminal view. Modeled after the Linux `watch(1)` utility.
//!
//! # Usage
//!
//! ```text
//! watch <command>                Run command every 2 seconds
//! watch -n 5 <command>           Run command every 5 seconds
//! watch -d <command>             Highlight differences between runs
//! watch -g <command>             Exit when output changes
//! watch -e <command>             Exit on command error
//! watch -t <command>             Hide the header line
//! watch -c <command>             Preserve ANSI color codes
//! watch -x <cmd> [args...]       Execute directly (no shell)
//! watch -b <command>             Beep (BEL) on command error
//! watch -p <command>             Precise interval timing
//! watch --json <command>         Output each run as JSON
//! ```

use quoting::quoteaf_os;
use std::env;
use std::fmt::Write as FmtWrite;
use std::process;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Default interval between command executions (seconds).
const DEFAULT_INTERVAL: f64 = 2.0;

/// ANSI escape: clear screen and move cursor to top-left.
const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";

/// ANSI escape: reverse video on.
const REVERSE_ON: &str = "\x1b[7m";

/// ANSI escape: reverse video off.
const REVERSE_OFF: &str = "\x1b[27m";

/// ANSI escape: reset all attributes.
const RESET: &str = "\x1b[0m";

/// BEL character for terminal beep.
const BEL: &str = "\x07";

/// Shell used to run commands (matching Slate OS convention).
const SHELL: &str = "/bin/sh";

/// Hostname file path.
const ETC_HOSTNAME: &str = "/etc/hostname";

// ============================================================================
// Configuration
// ============================================================================

/// Parsed command-line configuration.
#[derive(Debug)]
struct Config {
    /// Interval between runs in seconds.
    interval: f64,
    /// Highlight differences between successive runs.
    differences: bool,
    /// Exit when output changes.
    chgexit: bool,
    /// Exit on command error (nonzero exit code).
    errexit: bool,
    /// Suppress the header line.
    no_title: bool,
    /// Preserve ANSI color codes in command output.
    color: bool,
    /// Use exec mode (no shell).
    exec: bool,
    /// Beep on command error.
    beep: bool,
    /// Precise interval timing (subtract execution time from sleep).
    precise: bool,
    /// Output each run as JSON instead of full-screen.
    json: bool,
    /// The command and its arguments.
    command: Vec<String>,
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Print usage information.
fn print_usage() {
    println!("Slate OS Watch Utility v{VERSION}");
    println!();
    println!("Execute a command periodically, displaying its output.");
    println!();
    println!("USAGE:");
    println!("  watch [OPTIONS] <command> [args...]");
    println!();
    println!("OPTIONS:");
    println!("  -n, --interval <secs>  Seconds between updates (default: {DEFAULT_INTERVAL})");
    println!("  -d, --differences      Highlight changes between updates");
    println!("  -g, --chgexit          Exit when output changes");
    println!("  -e, --errexit          Exit on command error");
    println!("  -t, --no-title         Turn off the header line");
    println!("  -c, --color            Interpret ANSI color sequences");
    println!("  -x, --exec             Pass command to exec instead of sh -c");
    println!("  -b, --beep             Beep on command error");
    println!("  -p, --precise          Attempt to run command at precise intervals");
    println!("      --json             Output each run as JSON");
    println!("  -h, --help             Display this help");
    println!("  -v, --version          Display version");
}

/// Parse command-line arguments into a `Config`.
///
/// Returns `None` if the program should exit (help/version were printed).
/// Parse the command line.
///
/// # Three outcomes, and they used to be two channels
///
/// * `Ok(Some(cfg))` -- run this.
/// * `Ok(None)` -- `--help` or `--version` was printed; exit 0.
/// * `Err(message)` -- a bad argument; the caller prints it and exits 1.
///
/// This returned `Option<Config>` and called `process::exit(1)` from **six**
/// places inside itself. So the signature described one failure mechanism
/// while the body used another, and every argument error was untestable: a
/// test that passed `-n` with nothing after it did not get a value back, it
/// took the whole test binary down with it. That is how these six paths came
/// to have no tests at all.
///
/// Behaviour is unchanged -- the same messages, the same exit codes, in the
/// same order. What moved is *where* the process ends, which is now `main`,
/// once.
fn parse_args(args: &[String]) -> Result<Option<Config>, String> {
    let mut cfg = Config {
        interval: DEFAULT_INTERVAL,
        differences: false,
        chgexit: false,
        errexit: false,
        no_title: false,
        color: false,
        exec: false,
        beep: false,
        precise: false,
        json: false,
        command: Vec::new(),
    };

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-h" | "--help" | "help" => {
                print_usage();
                return Ok(None);
            }
            "-v" | "--version" => {
                println!("watch (Slate OS) {VERSION}");
                return Ok(None);
            }
            "-n" | "--interval" => {
                if i + 1 >= args.len() {
                    return Err("watch: -n/--interval requires a numeric argument".to_string());
                }
                i += 1;
                match args[i].parse::<f64>() {
                    Ok(val) if val > 0.0 && val.is_finite() => {
                        cfg.interval = val;
                    }
                    _ => {
                        return Err(format!(
                            "watch: invalid interval {} (must be a positive number)",
                            quoteaf_os(&args[i])
                        ));
                    }
                }
            }
            "-d" | "--differences" => cfg.differences = true,
            "-g" | "--chgexit" => cfg.chgexit = true,
            "-e" | "--errexit" => cfg.errexit = true,
            "-t" | "--no-title" => cfg.no_title = true,
            "-c" | "--color" => cfg.color = true,
            "-x" | "--exec" => cfg.exec = true,
            "-b" | "--beep" => cfg.beep = true,
            "-p" | "--precise" => cfg.precise = true,
            "--json" => cfg.json = true,
            _ => {
                // First non-option argument starts the command.
                if let Some(flags) = arg.strip_prefix('-') {
                    // Might be combined short flags like -de.
                    let mut recognized = true;
                    for ch in flags.chars() {
                        match ch {
                            'd' => cfg.differences = true,
                            'g' => cfg.chgexit = true,
                            'e' => cfg.errexit = true,
                            't' => cfg.no_title = true,
                            'c' => cfg.color = true,
                            'x' => cfg.exec = true,
                            'b' => cfg.beep = true,
                            'p' => cfg.precise = true,
                            'n' => {
                                // -n<value> without space, or -n with next arg.
                                let rest = &flags[flags.find('n').map_or(0, |p| p + 1)..];
                                if !rest.is_empty() {
                                    // Inline value: -n5.
                                    match rest.parse::<f64>() {
                                        Ok(val) if val > 0.0 && val.is_finite() => {
                                            cfg.interval = val;
                                        }
                                        _ => {
                                            return Err(format!(
                                                "watch: invalid interval {} (must be a positive number)",
                                                quoteaf_os(rest)
                                            ));
                                        }
                                    }
                                } else if i + 1 < args.len() {
                                    i += 1;
                                    match args[i].parse::<f64>() {
                                        Ok(val) if val > 0.0 && val.is_finite() => {
                                            cfg.interval = val;
                                        }
                                        _ => {
                                            return Err(format!(
                                                "watch: invalid interval {} (must be a positive number)",
                                                quoteaf_os(&args[i])
                                            ));
                                        }
                                    }
                                } else {
                                    return Err("watch: -n requires a numeric argument".to_string());
                                }
                                // After processing 'n' (possibly with inline
                                // value), remaining chars were already consumed
                                // by the loop before 'n', so break out.
                                break;
                            }
                            _ => {
                                recognized = false;
                                break;
                            }
                        }
                    }
                    if !recognized {
                        // Not a recognized combined flag group -- treat as
                        // start of command.
                        cfg.command = args[i..].to_vec();
                        break;
                    }
                } else {
                    // Non-flag argument: everything from here is the command.
                    cfg.command = args[i..].to_vec();
                    break;
                }
            }
        }
        i += 1;
    }

    if cfg.command.is_empty() {
        return Err(
            "watch: no command specified\nTry 'watch --help' for more information.".to_string(),
        );
    }

    Ok(Some(cfg))
}

// ============================================================================
// Hostname helper
// ============================================================================

/// The system hostname for the header line, or a marker that is visibly not a
/// hostname.
///
/// # What this replaces
///
/// It read `/proc/sys/kernel/hostname`, then `/etc/hostname`, and if neither
/// gave anything it returned **`"localhost"`** -- a perfectly plausible
/// machine name, printed by a program that did not know the machine's name.
/// Nothing downstream could tell that from a machine actually called
/// `localhost`.
///
/// Nine programs in `userspace/` read that file directly and, when the read
/// failed, gave five different answers between them: an empty string,
/// `unknown`, `slateos`, `localhost` and `null`. Two of those five are
/// inventions. This is one of the two.
///
/// It now asks `gethostname(3)` through [`libcall`], which is the POSIX
/// accessor and reads the same file `uname`'s `nodename` does, so the two
/// cannot disagree. `/etc/hostname` remains as a fallback because it is the
/// *persistent* name and is genuinely a second source rather than a guess.
/// The last resort is `(unknown)`, which is not a hostname a machine could
/// have -- the parentheses are the point.
fn read_hostname() -> String {
    if let Some(name) = ask_libc_for_hostname() {
        return name;
    }
    if let Ok(content) = std::fs::read_to_string(ETC_HOSTNAME) {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    UNKNOWN_HOSTNAME.to_string()
}

/// What to print when no source knows the machine's name.
///
/// Parenthesised so that it cannot be mistaken for a name: a machine may be
/// called `localhost` or `unknown`, but none is called `(unknown)`.
const UNKNOWN_HOSTNAME: &str = "(unknown)";

/// `gethostname(3)`, or `None` if it declines.
///
/// Through `libcall` rather than `posix` as a Rust dependency: the rlib copy
/// of the libc has its syscalls stubbed out, so the Rust path would answer
/// from a library that never reached the kernel (`design-decisions.md` 768).
fn ask_libc_for_hostname() -> Option<String> {
    let mut buf = [0u8; libcall::HOST_NAME_MAX + 1];
    let n = libcall::hostname_into(&mut buf).ok()?;
    let name = core::str::from_utf8(buf.get(..n)?).ok()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

// ============================================================================
// Time formatting
// ============================================================================

/// Format the current time as a human-readable string.
///
/// Produces output like: `Sat May 17 15:30:45 2025`.
/// Falls back to a Unix timestamp if the system clock is unavailable.
fn format_current_time() -> String {
    let secs = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return "unknown time".to_string(),
    };

    // Break down the Unix timestamp into calendar components.
    // This is a simplified civil-time conversion (no timezone support --
    // displays UTC). Sufficient for a watch header.
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Day of week: Jan 1 1970 was a Thursday (day 4).
    let dow = ((days + 4) % 7) as usize;
    let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let day_name = day_names.get(dow).copied().unwrap_or("???");

    // Year/month/day from days since epoch.
    let (year, month, day) = days_to_ymd(days);
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month_idx = if (1..=12).contains(&month) {
        (month - 1) as usize
    } else {
        0
    };
    let month_name = month_names.get(month_idx).copied().unwrap_or("???");

    format!("{day_name} {month_name} {day:2} {hours:02}:{minutes:02}:{seconds:02} {year}")
}

/// Convert days since Unix epoch to (year, month, day).
///
/// Uses the standard algorithm for converting a day count to a Gregorian
/// calendar date. Handles leap years correctly.
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm adapted from Howard Hinnant's civil_from_days.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // month index [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}

// ============================================================================
// Command execution
// ============================================================================

/// Result of running the watched command.
struct RunResult {
    /// Combined stdout + stderr output.
    output: String,
    /// Exit code (None if the process was killed by a signal).
    exit_code: Option<i32>,
    /// Whether the command succeeded (exit code 0).
    success: bool,
}

/// Execute the command and capture its output.
fn run_command(cfg: &Config) -> RunResult {
    let result = if cfg.exec {
        // Exec mode: run the command directly without a shell.
        if cfg.command.is_empty() {
            return RunResult {
                output: String::new(),
                exit_code: None,
                success: false,
            };
        }
        let mut cmd = Command::new(&cfg.command[0]);
        if cfg.command.len() > 1 {
            cmd.args(&cfg.command[1..]);
        }
        cmd.output()
    } else {
        // Shell mode: join args and pass to sh -c.
        let shell_cmd = cfg.command.join(" ");
        Command::new(SHELL).arg("-c").arg(&shell_cmd).output()
    };

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut combined = stdout.into_owned();
            if !stderr.is_empty() {
                if !combined.is_empty() && !combined.ends_with('\n') {
                    combined.push('\n');
                }
                combined.push_str(&stderr);
            }
            RunResult {
                output: combined,
                exit_code: output.status.code(),
                success: output.status.success(),
            }
        }
        Err(e) => RunResult {
            output: format!("watch: cannot execute command: {e}"),
            exit_code: None,
            success: false,
        },
    }
}

// ============================================================================
// Difference highlighting
// ============================================================================

/// Strip ANSI escape sequences from a string for comparison purposes.
///
/// This prevents escape codes from creating false "differences" when comparing
/// successive outputs.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip the escape sequence: ESC followed by '[' then parameters
            // ending with an alphabetic character.
            if let Some(next) = chars.next()
                && next == '['
            {
                // CSI sequence: consume until we hit a letter.
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            // OSC or other sequences: skip one character and continue.
        } else {
            out.push(ch);
        }
    }
    out
}

/// Highlight differences between `prev` and `curr` using reverse video.
///
/// Compares character-by-character. Characters that differ are wrapped in
/// reverse video escape sequences. Lines present in `curr` but not in `prev`
/// are highlighted entirely.
fn highlight_differences(prev: &str, curr: &str) -> String {
    let prev_clean = strip_ansi(prev);
    let curr_clean = strip_ansi(curr);

    let prev_lines: Vec<&str> = prev_clean.lines().collect();
    let curr_lines: Vec<&str> = curr_clean.lines().collect();
    // We also need the raw current lines for output (to preserve colors if
    // the user passed -c).
    let curr_raw_lines: Vec<&str> = curr.lines().collect();

    let mut result = String::new();

    for (line_idx, raw_line) in curr_raw_lines.iter().enumerate() {
        let curr_stripped = curr_lines.get(line_idx).copied().unwrap_or("");

        if line_idx >= prev_lines.len() {
            // Entirely new line -- highlight all of it.
            let _ = write!(result, "{REVERSE_ON}{raw_line}{REVERSE_OFF}");
        } else {
            let prev_stripped = prev_lines[line_idx];
            if curr_stripped == prev_stripped {
                // Identical line -- output as-is.
                result.push_str(raw_line);
            } else {
                // Character-level diff. We operate on the stripped version for
                // comparison but emit the raw characters for output.
                let prev_chars: Vec<char> = prev_stripped.chars().collect();
                let curr_chars: Vec<char> = curr_stripped.chars().collect();
                let mut in_highlight = false;

                for (ci, &ch) in curr_chars.iter().enumerate() {
                    let differs = if ci < prev_chars.len() {
                        ch != prev_chars[ci]
                    } else {
                        true // new character beyond prev length
                    };

                    if differs && !in_highlight {
                        result.push_str(REVERSE_ON);
                        in_highlight = true;
                    } else if !differs && in_highlight {
                        result.push_str(REVERSE_OFF);
                        in_highlight = false;
                    }
                    result.push(ch);
                }
                if in_highlight {
                    result.push_str(REVERSE_OFF);
                }
            }
        }
        result.push('\n');
    }

    result
}

// ============================================================================
// JSON output
// ============================================================================

/// Escape a string for JSON output (handles quotes, backslashes, control chars).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Output a single run as a JSON object on one line.
fn print_json_run(run_number: u64, cfg: &Config, result: &RunResult) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let command_str = cfg.command.join(" ");
    let escaped_cmd = json_escape(&command_str);
    let escaped_output = json_escape(&result.output);
    let exit_code = result
        .exit_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "null".to_string());

    println!(
        "{{\"run\":{run_number},\"timestamp\":{timestamp},\"command\":\"{escaped_cmd}\",\
         \"exit_code\":{exit_code},\"success\":{},\"output\":\"{escaped_output}\"}}",
        result.success
    );
}

// ============================================================================
// Display
// ============================================================================

/// Render the header line showing interval, command, hostname, and time.
fn render_header(cfg: &Config) -> String {
    let command_str = cfg.command.join(" ");
    let hostname = read_hostname();
    let time_str = format_current_time();

    format!(
        "Every {:.1}s: {:<40} {}: {}",
        cfg.interval, command_str, hostname, time_str
    )
}

/// Display the full screen: clear, header (if enabled), and command output.
fn display_output(cfg: &Config, output: &str, prev_output: Option<&str>) {
    print!("{CLEAR_SCREEN}");

    if !cfg.no_title {
        let header = render_header(cfg);
        println!("{header}");
        println!();
    }

    if cfg.differences {
        if let Some(prev) = prev_output {
            let highlighted = highlight_differences(prev, output);
            print!("{highlighted}");
        } else {
            // First run -- no previous output to compare against.
            print!("{output}");
        }
    } else if !cfg.color {
        // Strip ANSI codes when color mode is off.
        let stripped = strip_ansi(output);
        print!("{stripped}");
    } else {
        print!("{output}");
    }

    // Ensure the terminal attribute state is clean after our output.
    print!("{RESET}");
}

// ============================================================================
// Main loop
// ============================================================================

/// Run the main watch loop. Returns the exit code for the process.
fn run_watch(cfg: &Config) -> i32 {
    let interval = Duration::from_secs_f64(cfg.interval);
    let mut prev_output: Option<String> = None;
    let mut run_number: u64 = 0;

    loop {
        let start = Instant::now();
        let result = run_command(cfg);
        run_number = run_number.saturating_add(1);

        // Handle error cases.
        if !result.success {
            if cfg.beep {
                print!("{BEL}");
            }
            if cfg.errexit {
                if !cfg.json {
                    display_output(cfg, &result.output, prev_output.as_deref());
                    let code = result.exit_code.unwrap_or(1);
                    eprintln!("\nwatch: command exited with status {code}");
                } else {
                    print_json_run(run_number, cfg, &result);
                }
                return result.exit_code.unwrap_or(1);
            }
        }

        // Check for output changes (for --chgexit).
        if cfg.chgexit
            && let Some(ref prev) = prev_output
        {
            // Compare stripped versions to ignore ANSI differences.
            let prev_stripped = strip_ansi(prev);
            let curr_stripped = strip_ansi(&result.output);
            if prev_stripped != curr_stripped {
                if !cfg.json {
                    display_output(cfg, &result.output, Some(prev));
                } else {
                    print_json_run(run_number, cfg, &result);
                }
                return 0;
            }
        }

        // Display or emit output.
        if cfg.json {
            print_json_run(run_number, cfg, &result);
        } else {
            display_output(cfg, &result.output, prev_output.as_deref());
        }

        prev_output = Some(result.output);

        // Sleep for the remaining interval.
        let elapsed = start.elapsed();
        if cfg.precise {
            // Precise mode: subtract execution time from the interval.
            if let Some(remaining) = interval.checked_sub(elapsed)
                && !remaining.is_zero()
            {
                std::thread::sleep(remaining);
            }
            // If execution took longer than the interval, run again immediately.
        } else {
            std::thread::sleep(interval);
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let cfg = match parse_args(&args) {
        Ok(Some(c)) => c,
        Ok(None) => {
            // Help or version was printed; exit cleanly.
            process::exit(0);
        }
        Err(message) => {
            // The one place the process ends over a bad argument. It used to
            // be six places inside `parse_args`.
            eprintln!("{message}");
            process::exit(1);
        }
    };

    let code = run_watch(&cfg);
    process::exit(code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(rest: &[&str]) -> Vec<String> {
        core::iter::once("watch")
            .chain(rest.iter().copied())
            .map(str::to_string)
            .collect()
    }

    /// The marker printed when no source knows the machine's name cannot be
    /// mistaken for a machine's name.
    ///
    /// This is the whole point of the change that added it. `read_hostname`
    /// used to fall back to `"localhost"`, which is a name a machine can
    /// actually have -- so a header reading `localhost` was indistinguishable
    /// from one on a machine really called that. A hostname may contain
    /// letters, digits, hyphens and dots and nothing else, so the parentheses
    /// are what make this unambiguous rather than the word inside them.
    #[test]
    fn the_unknown_marker_is_not_a_possible_hostname() {
        assert!(UNKNOWN_HOSTNAME.contains('('));
        assert!(
            !UNKNOWN_HOSTNAME
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'),
            "{UNKNOWN_HOSTNAME} is spelled like a hostname could be"
        );
    }

    /// On the host there is no Slate kernel, so libc declines and the caller
    /// falls through rather than being handed a guess.
    #[cfg(not(unix))]
    #[test]
    fn the_host_has_no_libc_hostname_to_give() {
        assert_eq!(ask_libc_for_hostname(), None);
    }

    #[test]
    fn the_default_interval_is_two_seconds_and_the_command_is_the_rest() {
        let cfg = parse_args(&argv(&["uptime"])).unwrap().unwrap();
        assert!((cfg.interval - DEFAULT_INTERVAL).abs() < f64::EPSILON);
        assert_eq!(cfg.command, vec!["uptime".to_string()]);
    }

    #[test]
    fn an_interval_is_taken_from_the_following_argument() {
        let cfg = parse_args(&argv(&["-n", "0.5", "date"])).unwrap().unwrap();
        assert!((cfg.interval - 0.5).abs() < f64::EPSILON);
        assert_eq!(cfg.command, vec!["date".to_string()]);
    }

    /// An interval with no argument after it is refused rather than defaulted.
    ///
    /// This test is the reason `parse_args` returns a `Result` at all. It used
    /// to call `process::exit(1)` here, so this assertion did not fail -- it
    /// terminated the test binary, and the six argument-error paths could not
    /// be tested at all.
    #[test]
    fn an_interval_flag_with_nothing_after_it_is_refused() {
        let err = parse_args(&argv(&["-n"])).unwrap_err();
        assert!(err.contains("numeric argument"), "{err}");
    }

    /// Every shape of a bad interval is refused, and says which value it
    /// refused.
    #[test]
    fn a_bad_interval_is_refused_in_each_of_its_spellings() {
        for args in [
            vec!["-n", "zero", "date"],
            vec!["-n", "-1", "date"],
            vec!["-n", "0", "date"],
            vec!["-nzero", "date"],
        ] {
            let err = parse_args(&argv(&args)).unwrap_err();
            assert!(
                err.contains("invalid interval") || err.contains("numeric argument"),
                "{args:?} gave {err}"
            );
        }
    }

    /// A command is required.
    #[test]
    fn no_command_is_refused_with_a_pointer_to_help() {
        let err = parse_args(&argv(&[])).unwrap_err();
        assert!(err.contains("no command specified"), "{err}");
        assert!(err.contains("--help"), "{err}");
    }

    /// `--help` and `--version` are not errors.
    #[test]
    fn help_and_version_report_success_with_nothing_to_run() {
        assert!(parse_args(&argv(&["--help"])).unwrap().is_none());
        assert!(parse_args(&argv(&["--version"])).unwrap().is_none());
    }

    #[test]
    fn flags_set_their_fields_and_do_not_join_the_command() {
        let cfg = parse_args(&argv(&["-d", "-t", "-b", "ls", "-l"]))
            .unwrap()
            .unwrap();
        assert!(cfg.differences);
        assert!(cfg.no_title);
        assert!(cfg.beep);
        assert_eq!(cfg.command, vec!["ls".to_string(), "-l".to_string()]);
    }

    /// Everything after the command is the command's, including things that
    /// look like watch's own flags.
    ///
    /// `watch -n 1 ls -d` runs `ls -d`; it does not turn on watch's
    /// difference highlighting.
    #[test]
    fn a_flag_after_the_command_belongs_to_the_command() {
        let cfg = parse_args(&argv(&["-n", "1", "ls", "-d"]))
            .unwrap()
            .unwrap();
        assert!(!cfg.differences, "-d after the command was taken by watch");
        assert_eq!(cfg.command, vec!["ls".to_string(), "-d".to_string()]);
    }
}
