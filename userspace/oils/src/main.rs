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
//! The command line is parsed in bash's two passes: [`parse_long_options`] takes
//! the GNU-style long options (`--norc`, `--rcfile FILE`, …) off the front, then
//! a getopt-style loop takes the single-letter ones. `set` options (`-e`, `-x`,
//! …), `-i`/`+i` and `-l` may be bundled with the `-c`/`-s` mode letter into a
//! single cluster (`-ec`, `-ic`, `-lc`, `-cx`). Because the passes are ordered,
//! a long option written *after* a short one is an error, as in bash.
//!
//! Once the options are settled the shell reads its startup files
//! ([`Shell::run_startup_files`]) and, on `exit` from a login shell,
//! `~/.bash_logout` ([`Shell::run_logout_file`]).
//!
//! See `design-decisions.md §72` for why this is a Rust reimplementation of the
//! OSH language rather than a cross-compile of upstream Oils.

use std::io::{self, BufRead, IsTerminal, Write};
use std::process;

use osh::bytes::{self, BStr, Str};
use osh::{Shell, StartupFiles};

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

/// What the shell will execute once its startup files have been read.
///
/// The two are separated because bash settles `$0`, the positional parameters
/// and the shell's mode *before* it reads any startup file, and that is
/// observable: `osh -l script.sh a b` runs `~/.bash_profile` with `$0` already
/// `script.sh` and `$# == 2`, and `osh -l nosuchfile` reads the profile and only
/// *then* reports the missing script. So each mode arm prepares the shell and
/// returns one of these, rather than running anything itself.
enum Plan<'a> {
    /// `-c`: the command string to run.
    Command(BStr<'a>),
    /// A named script file, not yet opened.
    Script(BStr<'a>),
    /// Read commands from stdin (the REPL, `-s`, or a bare `-`).
    Repl,
}

/// Write one diagnostic line to stderr, assembled from byte fragments.
///
/// The words a startup diagnostic quotes come from argv, and argv is bytes, so
/// the message is built and written as bytes: a token that is not text is
/// reported verbatim. `eprintln!` cannot do that — it needs a `String`, and the
/// only way to get one here is a lossy conversion that would print a *different*
/// token than the user typed.
fn ediag(parts: &[BStr<'_>]) {
    let mut buf = Str::new();
    for p in parts {
        buf.extend_from_slice(p);
    }
    buf.push(b'\n');
    // Nothing useful remains to be done if stderr itself is gone; the exit
    // status still carries the failure to the caller.
    let _ = io::stderr().write_all(&buf);
}

/// One recognised GNU-style long option, after its leading dashes are stripped.
/// Mirrors bash's `long_args[]` table in `shell.c`, restricted to the options
/// osh can honour truthfully — bash's `--posix`, `--restricted`, `--debugger`,
/// `--wordexp` and friends are deliberately absent rather than accepted as
/// no-ops, because accepting them would claim behaviour osh does not have.
#[derive(Clone, Copy)]
enum LongOpt {
    /// `--help`
    Help,
    /// `--login`, the long spelling of `-l`.
    Login,
    /// `--noediting`. osh has no line editor, so there is nothing to switch
    /// off — but the resulting shell *does* behave as asked, which is why this
    /// one is accepted where `--posix` is not.
    NoEditing,
    /// `--noprofile`
    NoProfile,
    /// `--norc`
    NoRc,
    /// `--rcfile FILE` / `--init-file FILE` (synonyms in bash too).
    RcFile,
    /// `--verbose`, the long spelling of `set -v`.
    Verbose,
    /// `--version`
    Version,
}

impl LongOpt {
    /// The table lookup, on the name with its dashes already removed.
    fn lookup(name: &str) -> Option<Self> {
        Some(match name {
            "help" => Self::Help,
            "init-file" | "rcfile" => Self::RcFile,
            "login" => Self::Login,
            "noediting" => Self::NoEditing,
            "noprofile" => Self::NoProfile,
            "norc" => Self::NoRc,
            "verbose" => Self::Verbose,
            "version" => Self::Version,
            _ => return None,
        })
    }

    /// Whether the option consumes the following word as its argument.
    fn takes_arg(self) -> bool {
        matches!(self, Self::RcFile)
    }
}

/// Everything the long-option pass can record, to be applied once the pass has
/// finished (bash keeps these in globals for the same reason: `--version` must
/// not short-circuit a later `--badopt` in the same pass).
#[derive(Default)]
struct LongOptions {
    help: bool,
    version: bool,
    login: bool,
    verbose: bool,
    no_profile: bool,
    no_rc: bool,
    /// The argument of the last `--rcfile`/`--init-file`. Bytes: it is a path.
    rc_file: Option<Str>,
}

/// bash's `parse_long_options` (`shell.c`): a first pass over the command line
/// that consumes *only* GNU-style long options, stopping at the first word that
/// is not one. Returns the index of the first unconsumed word, or `Err(status)`
/// if the pass is fatal.
///
/// Being a separate, earlier pass has consequences that scripts depend on, so
/// osh reproduces all of them:
///
/// * **Long options must precede every short one.** `osh -x --norc -c cmd` never
///   reaches this table for `--norc`; the letter loop below gets it instead and
///   reports `--: invalid option`, exactly as bash does.
/// * **One dash is as good as two:** `-login` == `--login`. bash steps over a
///   second dash only when there is one, then compares the tail.
/// * **A missing argument names the table entry, not the token** — `rcfile:
///   option requires an argument`, with no dashes and no usage summary.
/// * **An unmatched two-dash word is fatal** (with the original token in the
///   message, dashes included); an unmatched one-dash word merely ends the pass,
///   so the letter loop can have it.
/// * **`-` and `--` alone are not long options** — there is no tail to match —
///   so they end the pass and are handled by the letter loop.
/// * `--help`/`--version` are recorded, not acted on: bash finishes the pass
///   first, so `osh --version --bogus` still reports the bad option, while `osh
///   --version -q` prints the version and never reaches the letter loop.
fn parse_long_options(args: &[Str], opts: &mut LongOptions) -> Result<usize, i32> {
    let mut i = 1;
    while let Some(arg) = args.get(i) {
        if !arg.starts_with(b"-") {
            break;
        }
        // `--name` matches `name`, and so does `-name`. `--` and `-` have an
        // empty tail, which matches nothing.
        let two_dash = arg.starts_with(b"--") && arg.len() > 2;
        let name: BStr<'_> = if two_dash { &arg[2..] } else { &arg[1..] };
        // An option name is text by construction — the table holds only ASCII
        // spellings — so a tail that does not decode simply matches no entry,
        // which is the same answer the lookup gives for `--bogus`.
        let Some(opt) = bytes::as_str(name).and_then(LongOpt::lookup) else {
            if two_dash {
                ediag(&[b"osh: ", arg, b": invalid option"]);
                eprint_option_usage();
                return Err(2);
            }
            break;
        };
        let mut value = None;
        if opt.takes_arg() {
            let Some(v) = args.get(i + 1) else {
                ediag(&[b"osh: ", name, b": option requires an argument"]);
                return Err(2);
            };
            value = Some(v.clone());
            i += 1;
        }
        match opt {
            LongOpt::Help => opts.help = true,
            LongOpt::Login => opts.login = true,
            LongOpt::NoEditing => {}
            LongOpt::NoProfile => opts.no_profile = true,
            LongOpt::NoRc => opts.no_rc = true,
            LongOpt::RcFile => opts.rc_file = value,
            LongOpt::Verbose => opts.verbose = true,
            LongOpt::Version => opts.version = true,
        }
        i += 1;
    }
    Ok(i)
}

/// This process's argument vector, as bytes.
///
/// `args_os`, not `args`: `std::env::args()` **panics** on an argument that is
/// not valid UTF-8, so a shell built on it dies before it starts when handed a
/// SlateOS filename containing an arbitrary byte — `osh -c 'cat "$1"' -- <name>`
/// would abort rather than run. Reading the OS strings and widening them keeps
/// every argument intact all the way to `$1` (see known-issues
/// TD-OILS-ARGV-PANICS-ON-NON-UTF8).
fn os_argv() -> Vec<Str> {
    std::env::args_os().map(|a| bytes::os_to_bytes(&a)).collect()
}

fn main() {
    let args = os_argv();
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
            run(&os_argv())
        }
    };
    process::exit(code);
}

/// The operands trailing the shell's own options, as the positional parameters
/// the shell stores.
fn positional_args(args: Option<&[Str]>) -> Vec<Str> {
    args.unwrap_or_default().to_vec()
}

fn run(args: &[Str]) -> i32 {
    let mut sh = Shell::new();
    // `$0` is `argv[0]` until something more specific replaces it (the `-c`
    // *name* operand, or a script path — both settled below). Seeded here,
    // ahead of `import_environment`, because that step can already diagnose: a
    // malformed exported function is reported prefixed with `$0`, and bash
    // shows the invocation path there. `Shell::new`'s literal `osh` remains the
    // fallback for the case where the platform gives no `argv[0]` at all.
    if let Some(argv0) = args.first().filter(|a| !a.is_empty()) {
        sh.set_name(argv0);
    }
    // Take ownership of the inherited process environment: environment
    // variables become ordinary (exported) shell variables, so `unset`,
    // prefix matching (`${!P*}`), and `set` listings behave like bash.
    sh.import_environment();

    // `login(1)` marks the shell it starts by prefixing `argv[0]` with a `-`
    // (`-bash`, `-osh`) rather than passing a flag, so bash checks the raw
    // `argv[0]` — not its basename — before it looks at any option. `-l` and
    // `--login` say the same thing explicitly.
    if args.first().is_some_and(|a| a.starts_with(b"-")) {
        sh.set_login_shell();
    }

    // First pass: the GNU-style long options (see `parse_long_options`).
    let mut long = LongOptions::default();
    let mut base = match parse_long_options(args, &mut long) {
        Ok(i) => i,
        Err(code) => return code,
    };
    // bash acts on these the moment the long-option pass returns, before the
    // letter loop runs at all, so `osh --help -q` prints help and exits 0.
    if long.help {
        print_help();
        return 0;
    }
    if long.version {
        println!("{VERSION}");
        return 0;
    }
    if long.login {
        sh.set_login_shell();
    }
    if long.verbose {
        // `--verbose` is exactly `set -v`; there is no separate state.
        let _ = sh.apply_short_options("v", true);
    }

    // Second pass: leading `set`/`shopt`-style option flags (`bash -e`, `-x`,
    // `-eu`, `-o pipefail`, `-O extglob`, `+O nocasematch`, `-n`, …), applying
    // each to the shell before the mode token (`-c`, a script path, …) is
    // dispatched. `base` advances past them so the mode token and its arguments
    // keep their normal relative positions. `--` ends option processing; `-c`,
    // `-s`, and any unrecognised token are handled by the dispatch below, so we
    // stop at them.
    // Set once `--` is seen: everything after it is operands, so the token at
    // `base` is a script path even if it begins with `-` (`osh -- -c` opens a
    // *file* named `-c`, matching bash), and the flag arms below are skipped.
    let mut opts_ended = false;
    // `-i` / `+i` force interactivity on/off, overriding tty detection for the
    // REPL (bash's `--force-interactive`). `None` = decide by isatty.
    let mut force_interactive: Option<bool> = None;
    // bash's `want_pending_command` and `read_from_stdin`. These are two
    // independent flags, not one mode: the letters may be bundled with `set`
    // options and with each other in any order (`-ec`, `-ic`, `-cs`, `-sc`), and
    // when both are given `-c` wins whichever came first, because bash tests for
    // a pending command string before it looks at `read_from_stdin`.
    let mut want_command = false;
    let mut read_stdin = false;
    // Bare `-` also selects stdin, and unlike the letters it *does* end option
    // processing (bash returns from `parse_shell_options` on it).
    // An argument that is not text cannot be any of the option spellings below
    // — every one of them is ASCII — so it falls to the `None` arm and ends the
    // option pass, which is right: it is an operand (a script path, or `$1`).
    while let Some(arg) = args.get(base) {
        match bytes::as_str(arg) {
            // `--` and a bare `-` are the same thing: bash's option walk returns
            // on either, with no flag set. A bare `-` reads commands from stdin
            // only because nothing is *left* to run as a script — the classic `sh
            // -` spelling — so `osh - script.sh` runs the script, and `osh - -x`
            // looks for a file called `-x` rather than applying `-x`.
            Some("--" | "-") => {
                base += 1;
                opts_ended = true;
                break;
            }
            // osh's `-V` version shorthand, an extension: bash has no `-V` and
            // rejects it. Acted on here rather than left for the dispatch below so
            // it works in any option position, like `--version`. After `--` it is
            // a filename again, because that arm breaks out first.
            Some("-V") => {
                println!("{VERSION}");
                return 0;
            }
            // A `-`/`+` cluster of single-letter invocation options: `set`
            // letters (`-e`, `-x`, `-eux`, `+x`), `-i`/`+i` (force
            // interactivity), `-o`/`-O` with a following name, and — for `-` —
            // the mode letters `-c`/`-s`, which may be bundled (`-ec`, `-ic`,
            // `-cs`). bash parses these getopt-style: valid letters are applied
            // left-to-right, and the first unrecognised letter aborts with
            // `<opt>: invalid option` (exit 2), still having applied the good
            // letters before it.
            //
            // A mode letter does *not* end the pass — bash's `case 'c'` only sets
            // a flag and the argv walk continues — so `osh -c -x 'echo hi'` traces
            // `echo hi` rather than trying to run `-x`, and `osh -s -x` applies
            // `-x` instead of making it `$1`. Only `--`, `-`, or a word that
            // starts with neither `-` nor `+` ends it.
            //
            // A long option reaching here — `osh -x --norc` — is *not* excluded:
            // getopt sees `--norc` as the letters `-`, `n`, `o`, … and the very
            // first one is invalid, so bash reports `--: invalid option`. Letting
            // the cluster loop have it reproduces that wording exactly.
            Some(s) if (s.starts_with('-') || s.starts_with('+')) && s.len() > 1 => {
                let enable = s.starts_with('-');
                let sign = if enable { '-' } else { '+' };
                // bash's `next_arg` cursor. `o`/`O` take the *next word* — never
                // the rest of the cluster — and each one seen advances the cursor,
                // so `osh -oo pipefail xtrace` sets both. It is also where a `-c`
                // command starts, which is why `osh -oc pipefail 'echo hi'` runs
                // `echo hi`: the cursor is past `pipefail` by then.
                let mut next_arg = base + 1;
                // `set` letters are gathered and applied together, but flushed
                // first whenever something else in the cluster acts, so the
                // left-to-right order bash applies them in is preserved.
                let mut set_letters = String::new();
                for c in s[1..].chars() {
                    match c {
                        'i' => force_interactive = Some(enable),
                        // `-l`: start as a login shell. Invocation-only, like
                        // `-i`, so it does not end option processing. bash
                        // accepts `+l` and does nothing with it — a login shell
                        // is how the shell was *started*, so it cannot be
                        // switched off any more than `shopt -u login_shell` can.
                        'l' => {
                            if enable {
                                sh.set_login_shell();
                            }
                        }
                        // Mode letters only select a mode with the `-` sign.
                        'c' if enable => want_command = true,
                        's' if enable => read_stdin = true,
                        // `o` selects a `set -o` option by name, `O` a shopt one;
                        // both read the next *word*. With no word left bash
                        // *lists* the options instead of failing, and carries on.
                        'o' | 'O' => {
                            if !set_letters.is_empty() {
                                let _ = sh.apply_short_options(&set_letters, enable);
                                set_letters.clear();
                            }
                            let is_shopt = c == 'O';
                            let Some(name) = args.get(next_arg) else {
                                if is_shopt {
                                    print!("{}", sh.format_shopt_list(enable));
                                } else {
                                    print!("{}", sh.format_named_option_list(enable));
                                }
                                continue;
                            };
                            // An option *name* is text — every entry in either
                            // table is ASCII — so a word that does not decode
                            // names no option and takes the invalid-name path,
                            // reporting the bytes it was actually given.
                            let ok = match bytes::as_str(name) {
                                Some(n) if is_shopt => sh.apply_shopt_option(n, enable),
                                Some(n) => sh.apply_named_option(n, enable),
                                None => false,
                            };
                            if !ok {
                                let tail: BStr<'_> = if is_shopt {
                                    b": invalid shell option name"
                                } else {
                                    b": invalid option name"
                                };
                                ediag(&[b"osh: ", name, tail]);
                                return 2;
                            }
                            next_arg += 1;
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
                // Past this cluster *and* every word its `o`/`O` letters ate.
                base = next_arg;
            }
            _ => break,
        }
    }

    // `-c` beats `-s` in either order (bash tests `command_execution_string`
    // before `read_from_stdin`), and both beat the default script/REPL.
    let mode = if want_command {
        InvokeMode::Command
    } else if read_stdin {
        InvokeMode::Stdin
    } else {
        InvokeMode::Repl
    };

    // bash's `interactive_shell` (main.c): `-i` forces it, otherwise there must
    // be no `-c` string, nothing left to run as a script (or `-s`, which reads
    // stdin whatever the operands are), and both stdin *and* stderr must be
    // terminals. Computed here, before the dispatch, because the startup files
    // need it too — `osh -i -c cmd` is a non-REPL shell that still reads
    // `~/.bashrc`. A piped/redirected REPL (`echo cmd | osh`, `osh < file`) is
    // non-interactive: no prompts, aliases off by default, `line N:` in errors.
    //
    // bash's `read_from_stdin` is a *separate* flag: `-s` sets it, and so does
    // having no operand left to run as a script — but a `-c` string does not
    // clear it, which is why `osh -cs cmd` reports both `c` and `s` in `$-`.
    let reads_stdin = read_stdin || (!want_command && args.get(base).is_none());
    // A pending `-c` string keeps the shell non-interactive whatever else is
    // true (bash's `!command_execution_string` clause), which is why `-cs` on a
    // terminal is not an interactive shell even though it reports `s`.
    let interactive = force_interactive.unwrap_or_else(|| {
        !want_command && reads_stdin && io::stdin().is_terminal() && io::stderr().is_terminal()
    });
    // Both describe how the shell was *started*, so they are recorded before any
    // mode dispatch: the startup files need `interactive`, and `-i -c cmd` is an
    // interactive shell despite running a `-c` string.
    sh.set_interactive_shell(interactive);
    if reads_stdin {
        sh.set_reads_stdin();
    }

    // Settle `$0`, the positional parameters and the shell's mode, but run
    // nothing yet: the startup files must see all of it (see `Plan`).
    let plan = match mode {
        InvokeMode::Command => {
            let Some(command) = args.get(base) else {
                eprintln!("osh: -c: option requires an argument");
                return 2;
            };
            // `osh -c cmd [name [arg…]]`
            sh.set_command_mode();
            // bash exposes the `-c` command string as $BASH_EXECUTION_STRING.
            sh.set_execution_string(command);
            if let Some(name) = args.get(base + 1) {
                sh.set_name(name);
                sh.set_positional(positional_args(args.get(base + 2..)));
            }
            Plan::Command(command)
        }
        InvokeMode::Stdin => {
            // `osh -s [arg…]`: read commands from stdin like the bare REPL, but
            // with the operands bound as positional parameters ($1, $2, …).
            sh.set_positional(positional_args(args.get(base..)));
            Plan::Repl
        }
        InvokeMode::Repl => match args.get(base).map(Vec::as_slice) {
            Some(path) if opts_ended || !path.starts_with(b"-") => {
                sh.set_name(path);
                sh.set_script_mode();
                sh.set_positional(positional_args(args.get(base + 1..)));
                Plan::Script(path)
            }
            Some(other) => {
                // A backstop only: every `-`-prefixed word is claimed by the
                // long-option pass, the `-`/`--`/`-V`/`-o` arms, or the letter
                // cluster, each with bash's own wording. Kept so a future hole in
                // that coverage still produces a diagnostic rather than an
                // attempt to open the option as a script.
                ediag(&[b"osh: ", other, b": invalid option"]);
                eprint_option_usage();
                return 2;
            }
            None => Plan::Repl,
        },
    };

    // `/etc/profile` + `~/.bash_profile`, or `~/.bashrc`, or `$BASH_ENV` —
    // whichever this invocation selects. A file that runs `exit` pre-empts the
    // command/script/REPL entirely, but still gets the EXIT trap below.
    let startup = StartupFiles {
        interactive,
        no_profile: long.no_profile,
        no_rc: long.no_rc,
        rc_file: long.rc_file.as_deref(),
    };
    let code = if let Some(status) = sh.run_startup_files(&startup) {
        status
    } else {
        match plan {
            Plan::Command(command) => sh.run_source(command),
            // The script is opened only now: bash reads the startup files first,
            // so `osh -l nosuchfile` runs `~/.bash_profile` before reporting 127.
            // The path is opened as *bytes*, so a script whose name is not text
            // is still found (the diagnostic below quotes it verbatim too).
            Plan::Script(path) => match std::fs::read(bytes::bytes_to_path(path)) {
                Ok(src) => sh.run_source(&src),
                Err(e) => {
                    ediag(&[b"osh: ", path, b": ", e.to_string().as_bytes()]);
                    127
                }
            },
            Plan::Repl => repl(&mut sh),
        }
    };
    // `~/.bash_logout` comes first and only for a login shell that actually ran
    // `exit`/`logout`: bash reads it from inside that builtin, so it precedes the
    // EXIT trap and sees the status from before the `exit`.
    let code = sh.run_logout_file().unwrap_or(code);
    // Fire the EXIT trap (if any) once, on true shell exit. It preserves the
    // pending exit status, so `code` remains the shell's final status — unless
    // the handler itself ran `exit N`, which replaces it (bash: `trap 'exit 9'
    // EXIT; exit 2` exits 9).
    sh.run_exit_trap().unwrap_or(code)
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
    // Physical lines already consumed from the stream. bash numbers `$LINENO`
    // and its diagnostics across the *whole* input, not per command, so each
    // buffer is executed with this as its line base — otherwise every command
    // read from a pipe would report line 1.
    let mut consumed: u32 = 0;
    loop {
        if interactive {
            print_prompt(sh);
        }
        let mut buffer = Str::new();
        let mut read_lines: u32 = 0;
        let done = loop {
            // Shell input is *bytes*: a script (or a here-doc body typed at the
            // prompt) may legitimately carry a byte that is not text, and a
            // UTF-8-validating read would fail the whole line rather than run
            // it. See TD-OILS-BYTE-STRINGS.
            let mut line = Str::new();
            match lock.read_until(b'\n', &mut line) {
                Ok(0) => break true, // EOF
                Ok(_) => {}
                Err(e) => {
                    eprintln!("osh: read error: {e}");
                    return 1;
                }
            }
            read_lines = read_lines.saturating_add(1);
            let mut trimmed = line.as_slice();
            while let Some(rest) = trimmed.strip_suffix(b"\n").or_else(|| trimmed.strip_suffix(b"\r")) {
                trimmed = rest;
            }
            buffer.extend_from_slice(trimmed);
            // A line ending in an *odd* number of backslashes leaves a live
            // `\<newline>` pair, so more input is coming. Both characters stay
            // in the buffer: splicing them out here is wrong in every case
            // except the plain one, because only the lexer knows whether the
            // pair is a continuation at all (inside `'…'`, and inside an
            // unquoted here-doc body, the rules differ) — and dropping the
            // backslash also merged the two physical lines into two separate
            // *commands*, since a bare newline was pushed in its place.
            let trailing_backslashes =
                trimmed.iter().rev().take_while(|&&b| b == b'\\').count();
            // If the command so far is only *incomplete* — an unterminated quote
            // or substitution, an unfinished `if`/`while`/`for`/`case`/`{`/`(`
            // compound command, or a line ending on `&&`/`||`/`|` — keep reading
            // continuation lines (PS2) so a multi-line command typed across
            // several prompts is joined into one logical command, as in bash. A
            // complete command, or a genuine (non-continuable) syntax error, both
            // fall through to execution below.
            if trailing_backslashes % 2 == 1
                || (!bytes::trim(&buffer).is_empty() && sh.parse_incomplete(&buffer))
            {
                buffer.push(b'\n');
                if interactive {
                    print_continuation();
                }
                continue;
            }
            break false;
        };

        if !bytes::trim(&buffer).is_empty() {
            sh.run_source_at(&buffer, consumed);
            // `exit` (or any unwind reaching the top level) ends the shell: stop
            // reading, exactly as bash does. Without this the loop would treat
            // the exit as an ordinary status and prompt for the next command.
            // No trailing newline here (unlike the EOF path below): the user's
            // Enter already ended the line.
            if sh.exit_requested() {
                return sh.last_status();
            }
        }
        // Count blank/comment-only buffers too — bash's line counter tracks
        // physical lines read, not commands run.
        consumed = consumed.saturating_add(read_lines);
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
    eprintln!("Usage:\tosh [long option] [option] ... [script-file [arg ...]]");
    eprintln!("\tosh [long option] [option] ... -c command [name [arg ...]]");
    eprintln!("\tosh [long option] [option] ... -s [arg ...]");
    eprintln!("Long options (must come first):");
    eprintln!("\t--help --init-file --login --noediting --noprofile");
    eprintln!("\t--norc --rcfile --verbose --version");
    eprintln!("Shell options:");
    eprintln!("\t-abefhkmnptuvxBCEHPT or -o option");
    eprintln!("\t-ils or -c command\t(invocation only)");
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
    println!("  -l / --login                 Start as a login shell (enables `logout`).");
    println!("  -o NAME / +o NAME            Enable/disable a `set -o` option (e.g. pipefail).");
    println!("  -O NAME / +O NAME            Enable/disable a shopt option (e.g. extglob).");
    println!("  --                           End option processing.");
    println!();
    println!("Long options (must precede every single-letter option, as in bash):");
    println!("  --noprofile                  Skip /etc/profile and ~/.bash_profile etc.");
    println!("  --norc                       Skip ~/.bashrc in an interactive shell.");
    println!("  --rcfile FILE                Read FILE instead of ~/.bashrc.");
    println!("  --init-file FILE             Synonym for --rcfile.");
    println!("  --verbose                    Same as -v (echo input lines as read).");
    println!("  --noediting                  Accepted; osh has no line editor.");
    println!();
    println!("Startup files (login shells read the profiles, interactive non-login");
    println!("shells read ~/.bashrc, non-interactive shells read $BASH_ENV; a login");
    println!("shell reads ~/.bash_logout when `exit`/`logout` ends it).");
    println!();
    println!("A bash/POSIX-superset shell (OSH). Supports pipes, redirections,");
    println!("here-documents and here-strings, variables and parameter expansion,");
    println!("command and arithmetic substitution, if/while/until/for/case,");
    println!("functions, [[ … ]] conditionals, (( … )) arithmetic commands,");
    println!("filename globbing, indexed and associative arrays, and && || ; operators.");
}

#[cfg(test)]
mod tests {
    use super::{LongOptions, Str, parse_long_options, positional_args};

    /// An argv word from a byte literal, as `os_argv` would produce it.
    fn a(bytes: &[u8]) -> Str {
        bytes.to_vec()
    }

    /// A byte that begins no valid UTF-8 sequence. Reading such a word through
    /// `std::env::args()` panics, which is the bug these tests pin
    /// (known-issues TD-OILS-ARGV-PANICS-ON-NON-UTF8): every one of them would
    /// have aborted the process before it could assert anything.
    const RAW: &[u8] = b"na\xffme";

    #[test]
    fn a_non_text_operand_ends_the_long_option_pass_rather_than_aborting() {
        // The word is not any long option — it cannot be, they are all ASCII —
        // so the pass stops at it and leaves it for the caller as an operand.
        let args = [a(b"osh"), a(b"--norc"), a(RAW), a(b"--verbose")];
        let mut opts = LongOptions::default();
        assert_eq!(parse_long_options(&args, &mut opts), Ok(2));
        assert!(opts.no_rc);
        // `--verbose` sits *after* the operand, so the pass never saw it.
        assert!(!opts.verbose);
    }

    #[test]
    fn a_non_text_rcfile_argument_is_captured_verbatim() {
        // `--rcfile` names a file, and a file name is not necessarily text: the
        // bytes must reach `StartupFiles::rc_file` unchanged or the shell reads
        // a different file (or, before the fix, never starts at all).
        let args = [a(b"osh"), a(b"--rcfile"), a(RAW)];
        let mut opts = LongOptions::default();
        assert_eq!(parse_long_options(&args, &mut opts), Ok(3));
        assert_eq!(opts.rc_file.as_deref(), Some(RAW));
    }

    #[test]
    fn a_non_text_two_dash_word_is_a_fatal_invalid_option() {
        // Two dashes make an unmatched word fatal, in bash and here. What must
        // not happen is a panic, or the bytes being silently reinterpreted as
        // some *other* option name.
        let args = [a(b"osh"), a(b"--b\xffgus")];
        let mut opts = LongOptions::default();
        assert_eq!(parse_long_options(&args, &mut opts), Err(2));
    }

    #[test]
    fn non_text_operands_reach_the_positional_parameters_intact() {
        let args = [a(b"osh"), a(b"script.sh"), a(RAW), a(b"plain")];
        assert_eq!(
            positional_args(args.get(2..)),
            vec![RAW.to_vec(), b"plain".to_vec()],
            "a positional parameter is a value and must survive byte-for-byte"
        );
        // Past the end is no operands, not a panic.
        assert!(positional_args(args.get(9..)).is_empty());
    }
}
