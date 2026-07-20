#![deny(clippy::all)]

//! `osh` — the Oils shell command-line entry point.
//!
//! Usage:
//!   osh                      Interactive REPL (reads commands from stdin).
//!   osh -c COMMAND [NAME ARG…]   Run COMMAND, with NAME as `$0` and ARG… as `$1…`.
//!   osh -s [ARG…]            Read commands from stdin, with ARG… as `$1…`.
//!   osh SCRIPT [ARG…]        Run SCRIPT, with ARG… as positional parameters.
//!   osh --version | --help
//!
//! Leading `set` options (`-e`, `-x`, …) and `-i`/`+i` may be bundled with the
//! `-c`/`-s` mode letter into a single cluster, getopt-style (`-ec`, `-ic`,
//! `-cx`), matching bash.
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

/// The shell's operating mode, selected by the leading `-c`/`-s` invocation
/// letters (which may appear bundled with `set` options, e.g. `-ec`, `-ic`).
/// `Repl` is the default when no mode letter is given: run a script file if one
/// is named, else read commands interactively/from stdin.
enum InvokeMode {
    /// Default: script file, or interactive/piped REPL.
    Repl,
    /// `-c COMMAND`: run COMMAND with the following operands as `$0`/`$1…`.
    Command,
    /// `-s`: read commands from stdin, with the operands as positional params.
    Stdin,
}

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
    // The mode selected by a `-c`/`-s` letter (default: REPL / script). bash
    // parses these getopt-style, so they may be bundled with `set` options and
    // `-i` in a single cluster (`-ec`, `-ic`, `-cs`); the mode letter can sit
    // anywhere in the cluster and the command/operands still come from the
    // following words.
    let mut mode = InvokeMode::Repl;
    while let Some(arg) = args.get(base) {
        match arg.as_str() {
            "--" => {
                base += 1;
                opts_ended = true;
                break;
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
            // Bare `-`: end option processing and read commands from stdin,
            // like `-s` with no operands (a legacy bash spelling). The operands
            // that follow become positional parameters.
            "-" => {
                base += 1;
                mode = InvokeMode::Stdin;
                break;
            }
            // osh's `-V` version shorthand is not a `set` letter; keep it out of
            // the option-cluster parser so it reaches the version dispatch below.
            "-V" => break,
            // A `-`/`+` cluster of single-letter invocation options: `set`
            // letters (`-e`, `-x`, `-eux`, `+x`), `-i`/`+i` (force
            // interactivity), and — for `-` — the mode letters `-c`/`-s`, which
            // may be bundled (`-ec`, `-ic`, `-cs`). bash parses these getopt-
            // style: valid letters are applied left-to-right, and the first
            // unrecognised letter aborts with `<opt>: invalid option` (exit 2),
            // still having applied the good letters before it.
            s if (s.starts_with('-') || s.starts_with('+')) && s.len() > 1 && !s.starts_with("--") => {
                let enable = s.starts_with('-');
                let sign = if enable { '-' } else { '+' };
                let mut set_letters = String::new();
                let mut found_mode = false;
                for c in s[1..].chars() {
                    match c {
                        'i' => force_interactive = Some(enable),
                        // Mode letters only select a mode with the `-` sign.
                        'c' if enable => {
                            mode = InvokeMode::Command;
                            found_mode = true;
                        }
                        's' if enable => {
                            mode = InvokeMode::Stdin;
                            found_mode = true;
                        }
                        _ if SET_OPTION_LETTERS.contains(c) => set_letters.push(c),
                        // First invalid letter: apply the good ones gathered so
                        // far (bash's behaviour), then report and abort.
                        _ => {
                            if !set_letters.is_empty() {
                                let _ = sh.apply_short_options(&set_letters, enable);
                            }
                            eprintln!("osh: {sign}{c}: invalid option");
                            eprint_option_usage();
                            return 2;
                        }
                    }
                }
                if !set_letters.is_empty() {
                    let _ = sh.apply_short_options(&set_letters, enable);
                }
                base += 1;
                // A mode letter ends option processing: the following words are
                // its command/operands, not more options.
                if found_mode {
                    break;
                }
            }
            _ => break,
        }
    }

    let code = match mode {
        InvokeMode::Command => {
            let Some(command) = args.get(base) else {
                eprintln!("osh: -c: option requires an argument");
                return 2;
            };
            // `osh -c cmd [name [arg…]]`
            sh.set_command_mode();
            // bash exposes the `-c` command string as $BASH_EXECUTION_STRING.
            sh.set_execution_string(command.clone());
            if let Some(name) = args.get(base + 1) {
                sh.set_name(name.clone());
                sh.set_positional(
                    args.get(base + 2..).map(<[String]>::to_vec).unwrap_or_default(),
                );
            }
            sh.run_source(command)
        }
        InvokeMode::Stdin => {
            // `osh -s [arg…]`: read commands from stdin like the bare REPL, but
            // with the operands bound as positional parameters ($1, $2, …).
            // Interactivity is still decided by `-i`/isatty, matching bash.
            sh.set_positional(args.get(base..).map(<[String]>::to_vec).unwrap_or_default());
            let interactive = force_interactive
                .unwrap_or_else(|| io::stdin().is_terminal() && io::stderr().is_terminal());
            sh.set_repl_interactive(interactive);
            repl(&mut sh)
        }
        InvokeMode::Repl => match args.get(base).map(String::as_str) {
            Some("--version" | "-V") if !opts_ended => {
                println!("{VERSION}");
                0
            }
            Some("--help" | "-h") if !opts_ended => {
                print_help();
                0
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
                // An unrecognised long option (`--foo`); bare `-`/`--`, `-V`,
                // and short clusters are handled above. Match bash's
                // `<opt>: invalid option` wording and usage summary.
                eprintln!("osh: {other}: invalid option");
                eprint_option_usage();
                2
            }
            None => {
                // Interactive iff `-i` forced it, else bash's rule: stdin AND
                // stderr are both terminals. A piped/redirected REPL
                // (`echo cmd | osh`, `osh < file`) is non-interactive — no
                // prompts, aliases off by default, `line N:` shown in errors.
                let interactive = force_interactive
                    .unwrap_or_else(|| io::stdin().is_terminal() && io::stderr().is_terminal());
                sh.set_repl_interactive(interactive);
                repl(&mut sh)
            }
        },
    };
    // Fire the EXIT trap (if any) once, on true shell exit. It preserves the
    // pending exit status, so `code` remains the shell's final status.
    sh.run_exit_trap();
    code
}

/// Interactive read-eval-print loop.
///
/// Reads a logical command at a time, joining physical lines two ways: a
/// trailing backslash is an explicit continuation (`\<newline>`), and an
/// otherwise-incomplete command — an open quote/substitution, an unfinished
/// `if`/`while`/`for`/`case`/`{`/`(` compound, or a line ending on `&&`/`||`/`|`
/// — keeps reading with a `PS2` continuation prompt until it parses as a
/// complete command (see [`Shell::parse_incomplete`]). This matches bash's
/// multi-line editing. Unterminated here-documents are the one gap: the lexer
/// treats `<<EOF` with no body as an empty here-doc rather than requesting more
/// input, so a here-doc body is not prompted for across separate lines.
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
            // A trailing backslash is an explicit line continuation: drop it and
            // join the next physical line (bash's lexer-level `\<newline>`).
            if let Some(cont) = trimmed.strip_suffix('\\') {
                buffer.push_str(cont);
                buffer.push('\n');
                if interactive {
                    print_continuation();
                }
                continue;
            }
            buffer.push_str(trimmed);
            // If the command so far is only *incomplete* — an unterminated quote
            // or substitution, an unfinished `if`/`while`/`for`/`case`/`{`/`(`
            // compound command, or a line ending on `&&`/`||`/`|` — keep reading
            // continuation lines (PS2) so a multi-line command typed across
            // several prompts is joined into one logical command, as in bash. A
            // complete command, or a genuine (non-continuable) syntax error, both
            // fall through to execution below.
            if !buffer.trim().is_empty() && sh.parse_incomplete(&buffer) {
                buffer.push('\n');
                if interactive {
                    print_continuation();
                }
                continue;
            }
            break false;
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

/// Print a concise invocation-usage summary to stderr, mirroring the summary
/// bash emits after an `invalid option` error (adapted to osh's own option set).
/// Kept short on purpose — the full feature list lives in `--help`.
fn eprint_option_usage() {
    eprintln!("Usage:\tosh [option] ... [script-file [arg ...]]");
    eprintln!("\tosh [option] ... -c command [name [arg ...]]");
    eprintln!("\tosh [option] ... -s [arg ...]");
    eprintln!("Shell options:");
    eprintln!("\t-abefhkmnptuvxBCEHPT or -o option");
    eprintln!("\t-ics or -c command\t(invocation only)");
    eprintln!("Run 'osh --help' for the full option list.");
}

fn print_help() {
    println!("{VERSION}");
    println!();
    println!("Usage:");
    println!("  osh                          Start an interactive shell.");
    println!("  osh -c COMMAND [NAME ARG…]   Execute COMMAND and exit.");
    println!("  osh -s [ARG…]                Read commands from stdin, ARG… as $1….");
    println!("  osh SCRIPT [ARG…]            Execute commands from SCRIPT.");
    println!("  osh -n …                     Check syntax without executing (noexec).");
    println!("  osh --version                Print version and exit.");
    println!("  osh --help                   Print this help and exit.");
    println!();
    println!("Leading options (applied before the command/script, as in bash):");
    println!("  -e -x -u -f -C …             Single-letter `set` options (clusters OK).");
    println!("  -i / +i                      Force interactive / non-interactive REPL.");
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
