#![deny(clippy::all)]

//! `osh` — the Oils shell command-line entry point.
//!
//! Usage:
//!   osh                      Interactive REPL (reads commands from stdin).
//!   osh -c COMMAND [NAME ARG…]   Run COMMAND, with NAME as `$0` and ARG… as `$1…`.
//!   osh SCRIPT [ARG…]        Run SCRIPT, with ARG… as positional parameters.
//!   osh --version | --help
//!
//! See `design-decisions.md §72` for why this is a Rust reimplementation of the
//! OSH language rather than a cross-compile of upstream Oils.

use std::io::{self, BufRead, Write};
use std::process;

use osh::Shell;

const VERSION: &str = concat!("osh (Oils for SlateOS) ", env!("CARGO_PKG_VERSION"));

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = run(&args);
    process::exit(code);
}

fn run(args: &[String]) -> i32 {
    let mut sh = Shell::new();

    let code = match args.get(1).map(String::as_str) {
        Some("--version" | "-V") => {
            println!("{VERSION}");
            0
        }
        Some("--help" | "-h") => {
            print_help();
            0
        }
        Some("-c") => {
            let Some(command) = args.get(2) else {
                eprintln!("osh: -c: option requires an argument");
                return 2;
            };
            // `osh -c cmd [name [arg…]]`
            if let Some(name) = args.get(3) {
                sh.set_name(name.clone());
                sh.set_positional(args.get(4..).map(<[String]>::to_vec).unwrap_or_default());
            }
            sh.run_source(command)
        }
        Some(path) if !path.starts_with('-') => {
            match std::fs::read_to_string(path) {
                Ok(src) => {
                    sh.set_name(path.to_string());
                    sh.set_positional(args.get(2..).map(<[String]>::to_vec).unwrap_or_default());
                    sh.run_source(&src)
                }
                Err(e) => {
                    eprintln!("osh: {path}: {e}");
                    127
                }
            }
        }
        Some(other) => {
            eprintln!("osh: unrecognized option '{other}'");
            2
        }
        None => repl(&mut sh),
    };
    // Fire the EXIT trap (if any) once, on true shell exit. It preserves the
    // pending exit status, so `code` remains the shell's final status.
    sh.run_exit_trap();
    code
}

/// Interactive read-eval-print loop.
///
/// Reads a logical line at a time; a trailing backslash continues onto the next
/// physical line. Multi-line compound commands typed across separate prompts
/// are not yet joined (a grow-phase item).
fn repl(sh: &mut Shell) -> i32 {
    let stdin = io::stdin();
    let mut lock = stdin.lock();
    loop {
        print_prompt(sh);
        let mut buffer = String::new();
        let done = loop {
            let mut line = String::new();
            match lock.read_line(&mut line) {
                Ok(0) => break true, // EOF
                Ok(_) => {}
                Err(e) => {
                    eprintln!("osh: read error: {e}");
                    return 1;
                }
            }
            let trimmed = line.trim_end_matches(['\n', '\r']);
            if let Some(cont) = trimmed.strip_suffix('\\') {
                buffer.push_str(cont);
                buffer.push('\n');
                print_continuation();
            } else {
                buffer.push_str(trimmed);
                break false;
            }
        };

        if !buffer.trim().is_empty() {
            sh.run_source(&buffer);
        }
        if done {
            println!();
            return sh.last_status();
        }
    }
}

fn print_prompt(sh: &Shell) {
    // Default prompt; `$?` shown when non-zero so failures are visible.
    let status = sh.last_status();
    if status == 0 {
        print!("osh$ ");
    } else {
        print!("osh[{status}]$ ");
    }
    let _ = io::stdout().flush();
}

fn print_continuation() {
    print!("> ");
    let _ = io::stdout().flush();
}

fn print_help() {
    println!("{VERSION}");
    println!();
    println!("Usage:");
    println!("  osh                          Start an interactive shell.");
    println!("  osh -c COMMAND [NAME ARG…]   Execute COMMAND and exit.");
    println!("  osh SCRIPT [ARG…]            Execute commands from SCRIPT.");
    println!("  osh --version                Print version and exit.");
    println!("  osh --help                   Print this help and exit.");
    println!();
    println!("A bash/POSIX-superset shell (OSH). Supports pipes, redirections,");
    println!("here-documents and here-strings, variables and parameter expansion,");
    println!("command and arithmetic substitution, if/while/until/for/case,");
    println!("functions, [[ … ]] conditionals, (( … )) arithmetic commands,");
    println!("filename globbing, indexed and associative arrays, and && || ; operators.");
}
