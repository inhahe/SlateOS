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

use std::io::{self, BufRead, IsTerminal, Write};
use std::process;

use osh::Shell;

const VERSION: &str = concat!("osh (Oils for SlateOS) ", env!("CARGO_PKG_VERSION"));

/// Stack reserved for the interpreter thread. A tree-walking shell recurses
/// natively once per nested function call / compound command, so the ~1 MiB
/// default main-thread stack overflows (and aborts the process) after only a
/// few hundred nested calls — far short of the several thousand bash tolerates.
/// A 64 MiB reserved stack gives comparable head-room (~thousands of levels)
/// while `FUNCNEST` still provides the graceful, bash-compatible ceiling. The
/// range is reserved virtual address space, grown on demand via guard pages —
/// not eagerly committed — so this is cheap on the host and on SlateOS alike.
const INTERP_STACK_SIZE: usize = 64 * 1024 * 1024;

/// Single-letter `set` options accepted as leading command-line flags (`bash
/// -e`, `-x`, `-eu`, …). Mirrors `Shell::apply_short_options` / the `set`
/// builtin's letter set: the modelled options plus the ones bash accepts as
/// no-ops. Mode letters (`c`, `s`, `i`, `l`, `r`) are deliberately excluded so
/// clusters containing them fall through to the mode dispatch.
const SET_OPTION_LETTERS: &str = "euxfaCnTEBmbhkptvHP";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Run the shell on a dedicated large-stack thread (see `INTERP_STACK_SIZE`).
    // If the thread cannot be spawned, fall back to running directly on the
    // main thread — a smaller stack, but still functional for shallow use.
    let code = match std::thread::Builder::new()
        .stack_size(INTERP_STACK_SIZE)
        .spawn(move || run(&args))
    {
        Ok(handle) => handle.join().unwrap_or_else(|_| {
            eprintln!("osh: fatal: shell thread terminated abnormally");
            2
        }),
        Err(e) => {
            eprintln!("osh: warning: could not allocate interpreter stack ({e}); running with default stack");
            let args: Vec<String> = std::env::args().collect();
            run(&args)
        }
    };
    process::exit(code);
}

fn run(args: &[String]) -> i32 {
    let mut sh = Shell::new();
    // Take ownership of the inherited process environment: environment
    // variables become ordinary (exported) shell variables, so `unset`,
    // prefix matching (`${!P*}`), and `set` listings behave like bash.
    sh.import_environment();

    // Consume leading `set`/`shopt`-style option flags (`bash -e`, `-x`, `-eu`,
    // `-o pipefail`, `-O extglob`, `+O nocasematch`, `-n`, …), applying each to
    // the shell before the mode token (`-c`, a script path, …) is dispatched.
    // `base` advances past them so the mode token and its arguments keep their
    // normal relative positions. `--` ends option processing; `-c`, `-s`, the
    // long options, and any unrecognised token are handled by the dispatch
    // below, so we stop at them.
    let mut base = 1;
    // Set once `--` is seen: everything after it is operands, so the token at
    // `base` is a script path even if it begins with `-` (`osh -- -c` opens a
    // *file* named `-c`, matching bash), and the flag arms below are skipped.
    let mut opts_ended = false;
    // `-i` / `+i` force interactivity on/off, overriding tty detection for the
    // REPL (bash's `--force-interactive`). `None` = decide by isatty.
    let mut force_interactive: Option<bool> = None;
    while let Some(arg) = args.get(base) {
        match arg.as_str() {
            "--" => {
                base += 1;
                opts_ended = true;
                break;
            }
            // `-i` / `+i`: force the REPL interactive / non-interactive. Only
            // meaningful on the REPL path (`-c`/script shells are never
            // interactive); it is consumed here so it does not fall through to
            // the mode dispatch as an unrecognised option.
            "-i" => {
                force_interactive = Some(true);
                base += 1;
            }
            "+i" => {
                force_interactive = Some(false);
                base += 1;
            }
            // `-n` keeps its dedicated latch path (see `Shell::set_noexec`).
            "-n" => {
                sh.set_noexec();
                base += 1;
            }
            // `-o NAME` / `+o NAME` (long `set` option), `-O NAME` / `+O NAME`
            // (shopt option). Each consumes the following word as its name.
            "-o" | "+o" | "-O" | "+O" => {
                let enable = arg.starts_with('-');
                let is_shopt = arg.ends_with('O');
                let Some(name) = args.get(base + 1) else {
                    eprintln!("osh: {arg}: option requires an argument");
                    return 2;
                };
                let ok = if is_shopt {
                    sh.apply_shopt_option(name, enable)
                } else {
                    sh.apply_named_option(name, enable)
                };
                if !ok {
                    if is_shopt {
                        eprintln!("osh: {name}: invalid shell option name");
                    } else {
                        eprintln!("osh: {name}: invalid option name");
                    }
                    return 2;
                }
                base += 2;
            }
            // A `-`/`+` cluster made up entirely of single-letter `set` options
            // (`-e`, `-x`, `-eux`, `+x`). Clusters containing a mode letter
            // (`-c`, `-s`, …) or an unknown letter fall through to the dispatch.
            s if (s.starts_with('-') || s.starts_with('+'))
                && s.len() > 1
                && s[1..].chars().all(|c| SET_OPTION_LETTERS.contains(c)) =>
            {
                let enable = s.starts_with('-');
                // Every letter was validated by the guard above.
                let _ = sh.apply_short_options(&s[1..], enable);
                base += 1;
            }
            _ => break,
        }
    }

    let code = match args.get(base).map(String::as_str) {
        Some("--version" | "-V") if !opts_ended => {
            println!("{VERSION}");
            0
        }
        Some("--help" | "-h") if !opts_ended => {
            print_help();
            0
        }
        Some("-c") if !opts_ended => {
            let Some(command) = args.get(base + 1) else {
                eprintln!("osh: -c: option requires an argument");
                return 2;
            };
            // `osh -c cmd [name [arg…]]`
            sh.set_command_mode();
            // bash exposes the `-c` command string as $BASH_EXECUTION_STRING.
            sh.set_execution_string(command.clone());
            if let Some(name) = args.get(base + 2) {
                sh.set_name(name.clone());
                sh.set_positional(
                    args.get(base + 3..).map(<[String]>::to_vec).unwrap_or_default(),
                );
            }
            sh.run_source(command)
        }
        Some(path) if opts_ended || !path.starts_with('-') => {
            match std::fs::read_to_string(path) {
                Ok(src) => {
                    sh.set_name(path.to_string());
                    sh.set_script_mode();
                    sh.set_positional(
                        args.get(base + 1..).map(<[String]>::to_vec).unwrap_or_default(),
                    );
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
        None => {
            // Interactive iff `-i` forced it, else bash's rule: stdin AND
            // stderr are both terminals. A piped/redirected REPL
            // (`echo cmd | osh`, `osh < file`) is non-interactive — no prompts,
            // aliases off by default, `line N:` shown in errors.
            let interactive = force_interactive
                .unwrap_or_else(|| io::stdin().is_terminal() && io::stderr().is_terminal());
            sh.set_repl_interactive(interactive);
            repl(&mut sh)
        }
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
    // Prompts (`PS1`/`PS2`) and the EOF newline are only emitted for a
    // terminal-attached interactive shell; a piped/redirected REPL stays silent
    // so its output byte-matches bash reading the same stream.
    let interactive = sh.is_interactive();
    let stdin = io::stdin();
    let mut lock = stdin.lock();
    loop {
        if interactive {
            print_prompt(sh);
        }
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
                if interactive {
                    print_continuation();
                }
            } else {
                buffer.push_str(trimmed);
                break false;
            }
        };

        if !buffer.trim().is_empty() {
            sh.run_source(&buffer);
        }
        if done {
            if interactive {
                println!();
            }
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
    println!("  osh -n …                     Check syntax without executing (noexec).");
    println!("  osh --version                Print version and exit.");
    println!("  osh --help                   Print this help and exit.");
    println!();
    println!("Leading options (applied before the command/script, as in bash):");
    println!("  -e -x -u -f -C …             Single-letter `set` options (clusters OK).");
    println!("  -o NAME / +o NAME            Enable/disable a `set -o` option (e.g. pipefail).");
    println!("  -O NAME / +O NAME            Enable/disable a shopt option (e.g. extglob).");
    println!("  --                           End option processing.");
    println!();
    println!("A bash/POSIX-superset shell (OSH). Supports pipes, redirections,");
    println!("here-documents and here-strings, variables and parameter expansion,");
    println!("command and arithmetic substitution, if/while/until/for/case,");
    println!("functions, [[ … ]] conditionals, (( … )) arithmetic commands,");
    println!("filename globbing, indexed and associative arrays, and && || ; operators.");
}
