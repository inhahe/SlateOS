//! tee — copy standard input to each FILE, and also to standard output.
//!
//! A port of GNU coreutils 9.4's `src/tee.c`, read rather than recalled. The
//! shipped version accepted `-a` and file names and nothing else: `-p`,
//! `--output-error`, `-i`, `--help` and `--version` were all missing, and a
//! file name holding a byte that is not valid UTF-8 — which this OS allows
//! everywhere but `/` and NUL — panicked before any of them mattered.
//!
//! `tee` exists to put a copy of a stream somewhere durable, so the one thing
//! it must never do is report success after failing to write that copy. Three
//! rules follow, and all three were once broken (`known-issues.md` →
//! `B-tee-REPORTS-SUCCESS-AFTER-LOSING-THE-DATA`):
//!
//! 1. A file that could not be opened is an error, not a warning: the exit
//!    status is 1 even though the rest of the copy succeeds.
//! 2. A write that fails is reported and drops that destination, and the exit
//!    status is 1. It is never discarded.
//! 3. Every write reaches the descriptor before the next one is attempted.
//!    Upstream gets that by setting every output unbuffered (`setvbuf`
//!    `_IONBF`); a buffered copy can report success for data still in memory.
//!
//! # What `--output-error` actually decides
//!
//! Upstream keeps five modes in one enum, and the whole of the logic is four
//! lines of `fail_output`:
//!
//! ```c
//! bool fail = errno != EPIPE
//!             || output_error == output_error_exit
//!             || output_error == output_error_warn;
//! ```
//!
//! So the test is **on the error, not on the file**: `nopipe` does not mean
//! "this output is a pipe", it means "this write failed with `EPIPE`". A unix
//! socket whose peer has gone is treated exactly like a pipe, because the
//! errno is the same — measured, not assumed. Everything else follows:
//!
//! | mode | `EPIPE` | any other error |
//! |---|---|---|
//! | `warn` | diagnose, drop it, exit 1 | diagnose, drop it, exit 1 |
//! | `warn-nopipe` (`-p`) | drop it silently | diagnose, drop it, exit 1 |
//! | `exit` | diagnose and stop now | diagnose and stop now |
//! | `exit-nopipe` | drop it silently | diagnose and stop now |
//!
//! and copying stops as soon as *every* output has been dropped, stdout
//! included, because there is nothing left to copy to.
//!
//! # The default mode, on an OS with no signals
//!
//! Upstream's fifth mode is `output_error_sigpipe`, the default, which differs
//! from `warn-nopipe` in exactly one respect: it leaves `SIGPIPE` at its
//! default disposition, so writing to a dead pipe kills the process outright
//! (a shell reports 141) instead of reaching `fail_output` at all.
//!
//! SlateOS does not use Unix signals for process control (`design.txt`), and
//! Rust masks `SIGPIPE` even where it exists, so "die from `SIGPIPE`" has no
//! translation. The faithful reading is that the default mode then *becomes*
//! its own `EPIPE` path — which is `fail = false`: drop that output, stay
//! quiet, keep copying to the others, exit on whatever the rest of the run
//! deserved. That is upstream's own code for the case, not an invention, and
//! it is what `cut`, `head`, `tail` and `uniq` in this tree already do with a
//! broken stdout. The one visible consequence: `yes | tee log | head -1`
//! leaves `tee` writing to `log` until the input ends, where GNU's `tee` dies
//! with the pipeline. Recorded as a deliberate difference in
//! `scripts/tee-diff.sh`.
//!
//! # What is deliberately not here
//!
//! * **`-i` is accepted and does nothing.** It ignores `SIGINT`, and there is
//!   no `SIGINT`. Accepting it matters more than refusing it would: `cmd |
//!   tee -i log` is written by scripts that have no idea which OS they land
//!   on, and rejecting the option would fail the whole pipeline over a request
//!   that is already satisfied.
//! * **No `iopoll`.** Upstream watches the first live output *while blocked
//!   reading*, so a `nopipe` run whose outputs have all become broken pipes
//!   ends immediately rather than at the next read. Reproducing that needs
//!   `poll(2)`, which needs a libc dependency this crate does not have. The
//!   copy is byte-identical either way; only the moment a doomed run gives up
//!   differs, and only when stdin is idle. See `known-issues.md`.
//!
//! # How this is tested
//!
//! `scripts/tee-diff.sh` builds this file for Linux inside WSL and runs it
//! against GNU coreutils case by case — the same answer `cmp`, `du`, `find`
//! and `ls` use (`design-decisions.md` §374). The unit tests below cover the
//! parser; nothing below `run` is reachable from them, because what it does is
//! make syscalls.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quotef, quotef_os};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::process::ExitCode;

/// `tee -Z; echo $?` is 1. Measured, not assumed: `ls`, `sort` and `grep` are 2.
const TEE: Program = Program::new("tee", 1);

/// Upstream's `getopt_long` string, verbatim. `p` carries no colon: the short
/// spelling never takes a value, and only `--output-error=MODE` does.
const SHORT_OPTIONS: &str = "aip";

/// Upstream's `long_options`, in its order — which is observable, because
/// glibc reports the *first* table entry an ambiguous prefix matched.
/// `scripts/getopt-ambiguity-check.py` compares this against `tee --=x`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("append", Takes::Nothing),
    ("ignore-interrupts", Takes::Nothing),
    // `Optional`, and that is observable: `--output-error warn` sets the
    // default mode and leaves `warn` behind as a *file name* to be written.
    ("output-error", Takes::Optional),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What to do about a write that failed. Upstream's `enum output_error`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum OutputError {
    /// The default. Upstream leaves `SIGPIPE` fatal here; see the module docs
    /// for why that lands on the same behaviour as `WarnNoPipe` for us.
    #[default]
    SigPipe,
    /// Diagnose every failure, including a broken pipe, and carry on.
    Warn,
    /// Diagnose every failure except a broken pipe, and carry on. `-p`.
    WarnNoPipe,
    /// Diagnose every failure and stop at the first.
    Exit,
    /// Stop at the first failure that is not a broken pipe; drop those quietly.
    ExitNoPipe,
}

impl OutputError {
    /// Upstream's `fail_output`: is this write failure worth reporting?
    ///
    /// The whole of the `nopipe` distinction. Note it is asked of the *errno*,
    /// never of the file — a socket and a FIFO give the same answer because
    /// they give the same error.
    fn reportable(self, broken_pipe: bool) -> bool {
        !broken_pipe || self == OutputError::Exit || self == OutputError::Warn
    }

    /// Whether a reported failure ends the run rather than dropping one output.
    fn fatal(self) -> bool {
        matches!(self, OutputError::Exit | OutputError::ExitNoPipe)
    }
}

/// The words `--output-error` accepts, in upstream's `output_error_args` order,
/// which is the order the "Valid arguments are:" list is printed in.
const OUTPUT_ERROR_ARGS: &[(&str, OutputError)] = &[
    ("warn", OutputError::Warn),
    ("warn-nopipe", OutputError::WarnNoPipe),
    ("exit", OutputError::Exit),
    ("exit-nopipe", OutputError::ExitNoPipe),
];

/// `BUFSIZ` on glibc, which is the size of one read and therefore of one write
/// to each output. Visible when an output dies mid-stream: it decides how much
/// of the input the surviving outputs received before the failure.
const BUFSIZ: usize = 8192;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Settings {
    append: bool,
    /// Parsed and kept so that the parser's shape matches upstream's, and so a
    /// future SlateOS interrupt mechanism has one obvious place to attach.
    ignore_interrupts: bool,
    output_error: OutputError,
    files: Vec<OsString>,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("tee (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(settings)) => run(&settings),
        Err(e) => {
            diag!("tee: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

/// Read the command line. Upstream's option loop, one arm per case.
///
/// # Errors
///
/// Any getopt diagnostic, plus `argmatch`'s for an `--output-error` value that
/// names no mode or several.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut set = Settings::default();
    for item in TEE.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'a', _) | Opt::Long("append", _) => set.append = true,
            Opt::Short(b'i', _) | Opt::Long("ignore-interrupts", _) => {
                set.ignore_interrupts = true;
            }
            // One arm upstream too: `-p` is `case 'p'` with a null `optarg`.
            Opt::Short(b'p', _) => set.output_error = OutputError::WarnNoPipe,
            Opt::Long("output-error", value) => {
                set.output_error = match value {
                    Some(word) => {
                        TEE.argmatch(&os_bytes(&word), "--output-error", OUTPUT_ERROR_ARGS)?
                    }
                    None => OutputError::WarnNoPipe,
                };
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Not special-cased, as POSIX requires: `tee -` writes a file
            // called `-`, it does not name standard output a second time.
            Opt::Operand(word) => set.files.push(word.clone()),
            // Every entry of `LONG_OPTIONS` and `SHORT_OPTIONS` is handled
            // above; an unknown one arrives as an `Err` from `parse`.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    Ok(Request::Run(set))
}

/// GNU's `--help`, minus the project's `Report bugs to:` block, as every
/// converted utility here omits it.
fn help_text() -> String {
    "\
Usage: tee [OPTION]... [FILE]...
Copy standard input to each FILE, and also to standard output.

  -a, --append              append to the given FILEs, do not overwrite
  -i, --ignore-interrupts   ignore interrupt signals
  -p                        operate in a more appropriate MODE with pipes.
      --output-error[=MODE]   set behavior on write error.  See MODE below
      --help        display this help and exit
      --version     output version information and exit

MODE determines behavior with write errors on the outputs:
  warn           diagnose errors writing to any output
  warn-nopipe    diagnose errors writing to any output not a pipe
  exit           exit on error writing to any output
  exit-nopipe    exit on error writing to any output not a pipe
The default MODE for the -p option is 'warn-nopipe'.
With \"nopipe\" MODEs, exit immediately if all outputs become broken pipes.
The default operation when --output-error is not specified, is to
exit immediately on error writing to a pipe, and diagnose errors
writing to non pipe outputs.
"
    .to_string()
}

// ------------------------------------------------------------------ copying ---

/// One destination: somewhere to write, and the name to blame if it fails.
///
/// The name is carried alongside rather than asked of the handle, because a
/// handle cannot tell you its own path — and because standard output's name is
/// not a path at all.
struct Output {
    /// As it appears in a diagnostic, already quoted: upstream renders it with
    /// `quotef` at every site, standard output included, which is why that one
    /// prints as `'standard output'` — the space forces the quotes on.
    label: String,
    sink: Sink,
}

enum Sink {
    /// Not a handle: the lock lives in `run`, because there is exactly one and
    /// it must outlive the loop that drops outputs.
    Stdout,
    File(File),
}

fn run(settings: &Settings) -> ExitCode {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut ok = true;

    // Standard output is upstream's `descriptors[0]`, which is why it is
    // written before any file and why its diagnostic comes first.
    let mut outputs: Vec<Output> = vec![Output {
        label: quotef(b"standard output"),
        sink: Sink::Stdout,
    }];

    for path in &settings.files {
        let opened = if settings.append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };
        match opened {
            Ok(f) => outputs.push(Output {
                label: quotef_os(path),
                sink: Sink::File(f),
            }),
            Err(e) => {
                diag!("tee: {}: {}", quotef_os(path), strerror(&e));
                ok = false;
                // `error (output_error == exit || == exit_nopipe, …)`: in the
                // exit modes a file that cannot be opened ends the run before
                // a single byte is copied anywhere.
                if settings.output_error.fatal() {
                    return ExitCode::from(1);
                }
            }
        }
    }

    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut buf = [0u8; BUFSIZ];
    let mut read_error: Option<io::Error> = None;

    // `while (n_outputs)`: with every destination gone there is nothing left
    // to copy to, so the input is not drained either.
    while !outputs.is_empty() {
        let n = match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            // Upstream's `errno == EINTR` retry. There are no signals here, but
            // the kernel may still short-circuit a read, and treating that as
            // end-of-input would silently truncate the copy.
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                read_error = Some(e);
                break;
            }
        };
        let chunk = buf.get(..n).unwrap_or(&[]);

        let mut i = 0usize;
        while i < outputs.len() {
            let Some(out) = outputs.get_mut(i) else { break };
            let result = match &mut out.sink {
                // Flushed here rather than at exit: upstream makes every
                // output unbuffered, and a `tee` that reports success for
                // bytes still sitting in a buffer is the bug this file exists
                // to not have.
                Sink::Stdout => stdout.write_all(chunk).and_then(|()| stdout.flush()),
                Sink::File(f) => f.write_all(chunk),
            };
            match result {
                Ok(()) => i += 1,
                Err(e) => {
                    let broken = e.kind() == io::ErrorKind::BrokenPipe;
                    let reportable = settings.output_error.reportable(broken);
                    if reportable {
                        diag!("tee: {}: {}", out.label, strerror(&e));
                        ok = false;
                    }
                    outputs.remove(i);
                    // Upstream's `error (status, …)` prints first and only
                    // then exits, and only for a failure it reported: a
                    // dropped `EPIPE` under `exit-nopipe` is not one.
                    if reportable && settings.output_error.fatal() {
                        return ExitCode::from(1);
                    }
                }
            }
        }
    }

    if let Some(e) = read_error {
        diag!("tee: read error: {}", strerror(&e));
        ok = false;
    }

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn settings(items: &[&str]) -> Settings {
        match parse_args(&argv(items)).unwrap() {
            Request::Run(s) => s,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    fn files(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    #[test]
    fn no_arguments_is_a_plain_copy() {
        let s = settings(&[]);
        assert!(!s.append);
        assert!(s.files.is_empty());
        assert_eq!(s.output_error, OutputError::SigPipe);
    }

    #[test]
    fn append_has_two_spellings() {
        assert!(settings(&["-a", "out"]).append);
        assert!(settings(&["--append", "out"]).append);
        assert_eq!(settings(&["-a", "out"]).files, files(&["out"]));
    }

    #[test]
    fn options_are_permuted() {
        // glibc permutes, so an option after an operand is still an option.
        // Without this `tee out -a` truncates the file it was asked to append
        // to, which is data loss with no diagnostic.
        let s = settings(&["out", "-a"]);
        assert!(s.append);
        assert_eq!(s.files, files(&["out"]));
    }

    #[test]
    fn double_dash_ends_the_options() {
        // The only way to write to a file called `-a`.
        let s = settings(&["--", "-a", "-Z"]);
        assert!(!s.append);
        assert_eq!(s.files, files(&["-a", "-Z"]));
    }

    #[test]
    fn a_bare_dash_is_a_file_called_dash() {
        // POSIX requires this, and upstream says so in a comment: "Do not
        // treat `-` specially". It is not a second name for standard output.
        assert_eq!(settings(&["-"]).files, files(&["-"]));
    }

    #[test]
    fn ignore_interrupts_is_accepted() {
        // It cannot do anything here, but refusing it would fail a pipeline
        // over a request that is already satisfied.
        assert!(settings(&["-i"]).ignore_interrupts);
        assert!(settings(&["--ignore-interrupts"]).ignore_interrupts);
    }

    #[test]
    fn dash_p_is_warn_nopipe() {
        // Upstream: `case 'p'` with a null `optarg`.
        assert_eq!(settings(&["-p"]).output_error, OutputError::WarnNoPipe);
        assert_eq!(
            settings(&["--output-error"]).output_error,
            OutputError::WarnNoPipe
        );
    }

    #[test]
    fn every_output_error_mode_is_spelled() {
        for (word, mode) in OUTPUT_ERROR_ARGS {
            let typed = format!("--output-error={word}");
            assert_eq!(settings(&[&typed]).output_error, *mode, "{word}");
        }
    }

    #[test]
    fn an_output_error_mode_may_be_abbreviated() {
        // `argmatch` resolves a unique prefix, so `--output-error=w` is
        // ambiguous but `--output-error=warn-` is not.
        assert_eq!(
            settings(&["--output-error=exit-"]).output_error,
            OutputError::ExitNoPipe
        );
    }

    #[test]
    fn output_error_takes_its_value_only_with_an_equals_sign() {
        // `Takes::Optional`, and this is what makes it observable: the word
        // after a bare `--output-error` stays an operand, so GNU writes a file
        // literally called `warn`.
        let s = settings(&["--output-error", "warn"]);
        assert_eq!(s.output_error, OutputError::WarnNoPipe);
        assert_eq!(s.files, files(&["warn"]));
    }

    #[test]
    fn a_bad_output_error_mode_lists_the_valid_ones() {
        let e = parse_args(&argv(&["--output-error=bogus"])).unwrap_err();
        assert!(e.sentence.contains("invalid argument"), "{}", e.sentence);
        assert!(e.sentence.contains("--output-error"), "{}", e.sentence);
        for (word, _) in OUTPUT_ERROR_ARGS {
            assert!(e.sentence.contains(word), "{} lacks {word}", e.sentence);
        }
        // argmatch is gnulib's, and dies with EXIT_FAILURE rather than the
        // caller's usage status. Both are 1 for tee, but the rule is the rule.
        assert_eq!(e.status, 1);
    }

    #[test]
    fn an_empty_output_error_mode_is_ambiguous_not_invalid() {
        // The empty string is a prefix of all four, so glibc's argmatch calls
        // it ambiguous. Measured: `tee --output-error=` says so.
        let e = parse_args(&argv(&["--output-error="])).unwrap_err();
        assert!(e.sentence.contains("ambiguous argument"), "{}", e.sentence);
    }

    #[test]
    fn an_unknown_option_refers_to_help() {
        let e = parse_args(&argv(&["-Z"])).unwrap_err();
        assert_eq!(e.status, 1);
        assert!(e.message().contains("invalid option -- 'Z'"), "{e}");
        assert!(e.message().contains("Try 'tee --help'"), "{e}");
    }

    #[test]
    fn help_and_version_end_the_parse() {
        assert_eq!(parse_args(&argv(&["--help", "-Z"])).unwrap(), Request::Help);
        assert_eq!(
            parse_args(&argv(&["--version", "-Z"])).unwrap(),
            Request::Version
        );
    }

    #[test]
    fn a_file_name_that_is_not_utf8_is_carried_through() {
        // The defect this rewrite began with: `env::args()` panics here, and
        // every byte but `/` and NUL is a legal name on this OS.
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0xff, 0xfe, b'x'])
        };
        #[cfg(not(unix))]
        let name = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0xd800, u16::from(b'x')])
        };
        let s = match parse_args(&[OsString::from("-a"), name.clone()]).unwrap() {
            Request::Run(s) => s,
            other => panic!("expected a run, got {other:?}"),
        };
        assert!(s.append);
        assert_eq!(s.files, vec![name]);
    }

    #[test]
    fn broken_pipe_is_reportable_only_in_the_two_full_modes() {
        // The whole of `--output-error`, as a table. Upstream's `fail_output`:
        // `errno != EPIPE || mode == exit || mode == warn`.
        for (mode, on_epipe, on_enospc) in [
            (OutputError::SigPipe, false, true),
            (OutputError::Warn, true, true),
            (OutputError::WarnNoPipe, false, true),
            (OutputError::Exit, true, true),
            (OutputError::ExitNoPipe, false, true),
        ] {
            assert_eq!(mode.reportable(true), on_epipe, "{mode:?} on EPIPE");
            assert_eq!(mode.reportable(false), on_enospc, "{mode:?} on ENOSPC");
        }
    }

    #[test]
    fn only_the_exit_modes_stop_the_run() {
        assert!(!OutputError::SigPipe.fatal());
        assert!(!OutputError::Warn.fatal());
        assert!(!OutputError::WarnNoPipe.fatal());
        assert!(OutputError::Exit.fatal());
        assert!(OutputError::ExitNoPipe.fatal());
    }

    #[test]
    fn standard_output_is_named_with_quotes() {
        // Not decoration: upstream renders it with `quotef` like any other
        // name, and the space in it forces the quotes on. A harness comparing
        // stderr byte for byte sees the difference.
        assert_eq!(quotef(b"standard output"), "'standard output'");
    }

    #[test]
    fn the_long_table_is_in_gnus_order() {
        // Order decides which option an ambiguous prefix names first, because
        // glibc reports the first entry that matched. `--output-error` before
        // `--help`, and `--help` before `--version`, is measured from
        // `tee --=x`; see scripts/getopt-ambiguity-check.py.
        let names: Vec<&str> = LONG_OPTIONS.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "append",
                "ignore-interrupts",
                "output-error",
                "help",
                "version"
            ]
        );
    }
}
