//! SlateOS `tee` Utility -- Read Stdin, Write to Stdout and Files
//!
//! Reads standard input and writes it to both standard output and zero or more
//! files simultaneously. Modeled after GNU coreutils `tee` with the same flag
//! set.
//!
//! # Usage
//!
//! ```text
//! tee [OPTION]... [FILE]...
//!
//! Copy standard input to each FILE, and also to standard output.
//!
//!   -a, --append              Append to the given FILEs, do not overwrite
//!   -i, --ignore-interrupts   Ignore the SIGINT signal
//!   -p                        Diagnose errors writing to non-pipes
//!       --output-error=MODE   Set error behavior:
//!                                warn           warn on error writing to any output
//!                                warn-nopipe    warn on error writing to non-pipe outputs
//!                                exit           exit on error writing to any output
//!                                exit-nopipe    exit on error writing to non-pipe outputs
//!       --help                Display this help and exit
//!       --version             Output version information and exit
//! ```

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Read buffer size. 8 KiB balances syscall overhead against memory usage for
/// a simple pipe-forwarding utility.
const BUF_SIZE: usize = 8192;

// ============================================================================
// Output error mode
// ============================================================================

/// Controls how write errors are handled.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputErrorMode {
    /// Default: silently ignore pipe errors, warn on others (GNU default).
    WarnNopipe,
    /// Warn on any write error, including broken pipes.
    Warn,
    /// Exit immediately on any write error.
    Exit,
    /// Exit on non-pipe write errors; silently ignore pipe errors.
    ExitNopipe,
}

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Files to write to (in addition to stdout).
    file_paths: Vec<String>,
    /// Whether to append to files instead of truncating.
    append: bool,
    /// Whether to ignore SIGINT.
    ignore_interrupts: bool,
    /// Output error handling mode. The `-p` flag is folded into this during
    /// argument parsing (it upgrades the default to `WarnNopipe`).
    output_error: OutputErrorMode,
}

/// Result of argument parsing -- either a runnable config or an early-exit
/// action.
enum ParseResult {
    Run(Config),
    Help,
    Version,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args(args: &[String]) -> ParseResult {
    let mut file_paths: Vec<String> = Vec::new();
    let mut append = false;
    let mut ignore_interrupts = false;
    let mut diagnose_nonpipe = false;
    let mut output_error = OutputErrorMode::WarnNopipe;
    let mut output_error_set = false;

    // Whether we have seen `--` signaling end of options.
    let mut end_of_opts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            // Positional argument (file path). `-` is treated as a filename
            // (stdin-as-file is not meaningful for tee, but we accept it for
            // compatibility and it will just fail to open, which is fine).
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
            if arg == "--append" {
                append = true;
            } else if arg == "--ignore-interrupts" {
                ignore_interrupts = true;
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--output-error" || arg.starts_with("--output-error=") {
                let mode_str = if let Some(eq_val) = arg.strip_prefix("--output-error=") {
                    eq_val.to_string()
                } else {
                    // Next argument is the mode value.
                    i += 1;
                    if i >= args.len() {
                        eprintln!("tee: option '--output-error' requires an argument");
                        eprintln!("Try 'tee --help' for more information.");
                        process::exit(1);
                    }
                    args[i].clone()
                };

                output_error = match mode_str.as_str() {
                    "warn" => OutputErrorMode::Warn,
                    "warn-nopipe" => OutputErrorMode::WarnNopipe,
                    "exit" => OutputErrorMode::Exit,
                    "exit-nopipe" => OutputErrorMode::ExitNopipe,
                    _ => {
                        eprintln!(
                            "tee: invalid output error mode '{mode_str}'"
                        );
                        eprintln!("Valid modes: warn, warn-nopipe, exit, exit-nopipe");
                        eprintln!("Try 'tee --help' for more information.");
                        process::exit(1);
                    }
                };
                output_error_set = true;
            } else {
                eprintln!("tee: unrecognized option '{arg}'");
                eprintln!("Try 'tee --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options: may be combined (e.g., `-ai` = `-a -i`).
        for ch in arg[1..].chars() {
            match ch {
                'a' => append = true,
                'i' => ignore_interrupts = true,
                'p' => diagnose_nonpipe = true,
                _ => {
                    eprintln!("tee: invalid option -- '{ch}'");
                    eprintln!("Try 'tee --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    // `-p` upgrades the default to WarnNopipe if no explicit --output-error
    // was given. If --output-error was set, `-p` has no additional effect
    // (GNU behavior).
    if diagnose_nonpipe && !output_error_set {
        output_error = OutputErrorMode::WarnNopipe;
    }

    ParseResult::Run(Config {
        file_paths,
        append,
        ignore_interrupts,
        output_error,
    })
}

// ============================================================================
// Error classification
// ============================================================================

/// Returns `true` if the given I/O error represents a broken-pipe condition.
fn is_pipe_error(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::BrokenPipe
}

/// Decide whether to report a write error, based on the current error mode.
/// Returns `true` if the error should cause a diagnostic message.
fn should_warn(mode: OutputErrorMode, err: &io::Error) -> bool {
    match mode {
        OutputErrorMode::Warn | OutputErrorMode::Exit => true,
        OutputErrorMode::WarnNopipe | OutputErrorMode::ExitNopipe => !is_pipe_error(err),
    }
}

/// Decide whether a write error should cause immediate process exit.
fn should_exit(mode: OutputErrorMode, err: &io::Error) -> bool {
    match mode {
        OutputErrorMode::Exit => true,
        OutputErrorMode::ExitNopipe => !is_pipe_error(err),
        OutputErrorMode::Warn | OutputErrorMode::WarnNopipe => false,
    }
}

// ============================================================================
// Core tee loop
// ============================================================================

/// Open all output files and return them alongside their paths.
///
/// Returns the successfully opened files and prints a warning for each file
/// that could not be opened. The overall success flag is returned so the
/// caller can set the exit code.
fn open_files(config: &Config) -> (Vec<(String, File)>, bool) {
    let mut files: Vec<(String, File)> = Vec::new();
    let mut all_ok = true;

    for path in &config.file_paths {
        let result = if config.append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };

        match result {
            Ok(f) => files.push((path.clone(), f)),
            Err(e) => {
                eprintln!("tee: {path}: {e}");
                all_ok = false;
            }
        }
    }

    (files, all_ok)
}

/// Run the main tee copy loop. Returns the process exit code.
fn run(config: &Config) -> i32 {
    let (mut files, mut all_ok) = open_files(config);

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    let mut buf = [0u8; BUF_SIZE];

    loop {
        // Read a chunk from stdin.
        let n = match stdin_lock.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => n,
            Err(e) => {
                eprintln!("tee: read error: {e}");
                return 1;
            }
        };

        let chunk = &buf[..n];

        // Write to stdout.
        if let Err(e) = stdout_lock.write_all(chunk) {
            if should_warn(config.output_error, &e) {
                eprintln!("tee: standard output: {e}");
            }
            if should_exit(config.output_error, &e) {
                return 1;
            }
            // For broken pipe on stdout with nopipe modes, we still mark
            // failure but continue writing to files.
            if !is_pipe_error(&e) {
                all_ok = false;
            }
        }

        // Write to each output file. Track indices of files that fatally
        // failed so we can remove them (avoiding repeated errors on the same
        // dead file descriptor).
        let mut failed_indices: Vec<usize> = Vec::new();

        for (idx, (path, file)) in files.iter_mut().enumerate() {
            if let Err(e) = file.write_all(chunk) {
                all_ok = false;
                if should_warn(config.output_error, &e) {
                    eprintln!("tee: {path}: {e}");
                }
                if should_exit(config.output_error, &e) {
                    return 1;
                }
                // Remove this file from future writes to avoid spamming
                // errors on every chunk.
                failed_indices.push(idx);
            }
        }

        // Remove failed files in reverse order to preserve indices.
        for idx in failed_indices.into_iter().rev() {
            files.remove(idx);
        }
    }

    // Flush stdout.
    if let Err(e) = stdout_lock.flush() {
        if should_warn(config.output_error, &e) {
            eprintln!("tee: standard output: {e}");
        }
        if !is_pipe_error(&e) {
            all_ok = false;
        }
    }

    // Flush all output files.
    for (path, mut file) in files {
        if let Err(e) = file.flush() {
            eprintln!("tee: {path}: {e}");
            all_ok = false;
        }
    }

    if all_ok { 0 } else { 1 }
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("Slate OS tee v{VERSION}");
    println!();
    println!("Copy standard input to each FILE, and also to standard output.");
    println!();
    println!("USAGE:");
    println!("  tee [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -a, --append              Append to the given FILEs, do not overwrite");
    println!("  -i, --ignore-interrupts   Ignore the SIGINT signal");
    println!("  -p                        Diagnose errors writing to non-pipes");
    println!("      --output-error=MODE   Set error behavior:");
    println!("                              warn         warn on error writing to any output");
    println!("                              warn-nopipe  warn except for pipe errors (default)");
    println!("                              exit         exit on error writing to any output");
    println!("                              exit-nopipe  exit on non-pipe write errors");
    println!("      --help                Display this help and exit");
    println!("      --version             Output version information and exit");
    println!();
    println!("With no FILE, or when FILE is -, copy standard input to standard output.");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    match parse_args(&args) {
        ParseResult::Help => {
            print_help();
            process::exit(0);
        }
        ParseResult::Version => {
            println!("tee (Slate OS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            // Note: `config.ignore_interrupts` is parsed and stored for
            // completeness. On SlateOS, signal handling uses IPC messages rather
            // than Unix signals, so SIGINT masking is a no-op until the SlateOS
            // signal-compatibility layer is wired up. The flag is accepted to
            // maintain CLI compatibility with GNU tee.
            let _ = config.ignore_interrupts; // acknowledged, no-op on SlateOS currently
            let code = run(&config);
            process::exit(code);
        }
    }
}
