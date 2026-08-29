//! xargs — build command lines from standard input and run them.
//!
//! A transcription of GNU findutils 4.9.0 `xargs/xargs.c` together with the
//! half of `lib/buildcmd.c` that xargs is the only caller of. The differences
//! from upstream are enumerated under "Divergences" below; everything else here
//! is meant to be the same program, byte for byte, including its diagnostics
//! and its exit statuses.
//!
//! # What was here before
//!
//! The stub this replaces was 285 lines and wrong in ways that a caller feels
//! rather than reads about:
//!
//! | Stub | GNU |
//! |---|---|
//! | `env::args()` — panics on a non-UTF-8 argument | argv is bytes |
//! | `read_to_string` — errors on a non-UTF-8 *input* | input is bytes |
//! | `split_whitespace` | a quote/backslash state machine |
//! | dropped empty items | `''` is an argument |
//! | `-0`, `-n`, `-I` only | 18 long options, 14 short |
//! | every child failure is exit 1 | 123 / 124 / 125 / 126 / 127 |
//! | empty input ran nothing | the command runs once unless `-r` |
//!
//! The last row is the one that changes what a script does: `xargs` with no
//! items still runs `COMMAND` once, and `find … | xargs rm -f` relies on it not
//! mattering. The stub returned success without running anything.
//!
//! # The shape of the original, kept
//!
//! Two structures here would be written differently in new code and are kept
//! because changing them changes behaviour:
//!
//! * **`push_arg` calls `do_exec` re-entrantly.** Upstream's `bc_push_arg`
//!   flushes the accumulated command line from inside the push that would have
//!   overflowed it, then pushes into the emptied state. Hoisting the flush to
//!   the caller would reorder it against the `die` cases in the same function,
//!   which are what distinguish "argument list too long" from "cannot fit
//!   single argument within argument list size limit".
//! * **`exit_via_atexit` emulates `atexit` LIFO.** Upstream registers
//!   `close_stdin` and then `wait_for_proc_all`, so the reaper runs *first* and
//!   can override an already-decided status: a command that returns 0 and then
//!   has a child fail during the final reap exits 123, not 0. Every exit path
//!   here goes through `exit_via_atexit` for that reason, and it is guarded by
//!   `waiting` exactly as upstream's is, because `wait_for_proc` itself exits
//!   with 124 and 125 from inside the reap.
//!
//! # No `close_stdout`
//!
//! xargs, unlike coreutils, registers no `close_stdout`, so a diagnostic that
//! could not be written must **not** turn into a failure status — a `-t` trace
//! lost to `2>&-` still exits with whatever the children earned. That is why
//! every exit here is `stdfd::exit_now(status, status)` rather than the usual
//! funnel. `print_args` is the single site upstream checks its own write at,
//! and it dies with `Failed to write to stderr` when it fails.
//!
//! # Divergences
//!
//! 1. **`endbuf` is computed in `i64`.** Upstream forms
//!    `linebuf + arg_max - cmd_initial_argv_chars - 1`, which is undefined
//!    behaviour when the initial arguments are longer than `arg_max`; here the
//!    subtraction saturates at 0 and the first input byte trips the
//!    "argument line too long" check, which is what the UB does in practice.
//! 2. **`do_insert` copies bytes, not a C string.** Upstream `strcpy`s the
//!    replacement in, so an item containing a NUL is truncated there. Here the
//!    item is copied whole. Items cannot contain NUL in line mode (it is the
//!    delimiter warning case) and in `-0` mode `-I` is not usually combined, so
//!    this is unreachable rather than merely rare.
//! 3. **`do_insert` guards against a zero-length match.** `-I ''` with an empty
//!    line makes upstream's `do { … } while (*arg)` spin forever; this breaks
//!    out instead.
//! 4. **Children are reaped in slot order, not arrival order.** Upstream
//!    `waitpid`s for whichever child exited first; `std::process::Child` can
//!    only be polled per child, so with `-P` above 1 the *order* of the
//!    per-child diagnostics can differ. With the default `-P 1` there is one
//!    child and no difference.
//! 5. **`-o`'s stdin redirect happens in the parent.** Upstream reopens
//!    `/dev/tty` after the fork, so `-o` with no controlling terminal dies in
//!    the child; here it fails before the spawn and exits 1.

use coreutils::errmsg::strerror;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{Style, os_bytes, os_from_bytes, quote};
use coreutils::stdfd;
use std::ffi::OsString;
use std::io::{BufReader, Read};
use std::process::{Child, Command, Stdio};

coreutils::guard_std_fds!();

/// `xargs` exits with status values with specific meanings (this is a POSIX
/// requirement). These are the values.
const XARGS_EXIT_CLIENT_EXIT_NONZERO: u8 = 123;
const XARGS_EXIT_CLIENT_EXIT_255: u8 = 124;
const XARGS_EXIT_CLIENT_FATAL_SIG: u8 = 125;
const XARGS_EXIT_COMMAND_CANNOT_BE_RUN: u8 = 126;
const XARGS_EXIT_COMMAND_NOT_FOUND: u8 = 127;

/// What a child uses to tell xargs to stop dead. Upstream calls this
/// `CHILD_EXIT_PLEASE_STOP_IMMEDIATELY`; a child exiting 255 makes xargs exit
/// 124 without starting anything further.
const CHILD_EXIT_PLEASE_STOP_IMMEDIATELY: i32 = 255;

/// `MAX_PROC_MAX` is `SIG_ATOMIC_MAX`, because upstream's `proc_max` is a
/// `sig_atomic_t` written from a signal handler.
const MAX_PROC_MAX: i64 = 2_147_483_647;

/// `XARGS_POSIX_HEADROOM` — how much of `ARG_MAX` upstream refuses to spend, so
/// that the child has room to grow its own environment.
const XARGS_POSIX_HEADROOM: usize = 2048;

/// `_POSIX_ARG_MAX`: the smallest `ARG_MAX` a conforming system may have, and
/// therefore the floor `-s` may not be pushed below.
const POSIX_ARG_SIZE_MIN: usize = 4096;

/// glibc's `legacy_ARG_MAX`, the floor of `sysconf (_SC_ARG_MAX)`.
const LEGACY_ARG_MAX: usize = 131_072;

/// `bc_use_sensible_arg_max`'s 128 KiB, clamped into the POSIX range.
const SENSIBLE_ARG_SIZE: usize = 128 * 1024;

/// A bad option exits 1, measured: `xargs --zzz; echo $?`.
const XARGS_FAILURE: u8 = 1;
const XARGS: Program = Program::new("xargs", XARGS_FAILURE as i32);

/// The leading `+` is load-bearing: option parsing stops at the first operand,
/// so `xargs argv -n` runs the command `argv -n` rather than reading `-n` as
/// xargs's own.
const SHORT_OPTIONS: &str = "+0a:E:e::i::I:l::L:n:oprs:txP:d:";

/// Upstream's `PROCESS_SLOT_VAR = CHAR_MAX + 1` — a long option with no short
/// spelling. Any byte no short option uses would do; this one cannot collide
/// because `SHORT_OPTIONS` is ASCII.
const PROCESS_SLOT_VAR: u8 = 0xff;

/// In upstream's declaration order, which is observable: `getopt_long` resolves
/// an abbreviation to the *first* table entry it matches, and reports an
/// ambiguity against the whole list. `--v` is ambiguous between `verbose` and
/// `version`; `--max` between four; `--a` is `arg-file` and eats the next word.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("null", Takes::Nothing),
    ("arg-file", Takes::Required),
    ("delimiter", Takes::Required),
    ("eof", Takes::Optional),
    ("replace", Takes::Optional),
    ("max-lines", Takes::Optional),
    ("max-args", Takes::Required),
    ("open-tty", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("no-run-if-empty", Takes::Nothing),
    ("max-chars", Takes::Required),
    ("verbose", Takes::Nothing),
    ("show-limits", Takes::Nothing),
    ("exit", Takes::Nothing),
    ("max-procs", Takes::Required),
    ("process-slot-var", Takes::Required),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
];

/// The `val` field of upstream's `longopts`, so that one `match` can serve both
/// spellings exactly as upstream's `switch (optc)` does.
///
/// Two of these are worth naming because they are not the letter you would
/// guess: `--replace` is `'I'` (not `'i'`) and `--max-lines` is `'l'` (not
/// `'L'`), so `--max-lines=0` is reported by `parse_num` as a bad `-l`.
fn long_val(name: &str) -> u8 {
    match name {
        "null" => b'0',
        "arg-file" => b'a',
        "delimiter" => b'd',
        "eof" => b'e',
        "replace" => b'I',
        "max-lines" => b'l',
        "max-args" => b'n',
        "open-tty" => b'o',
        "interactive" => b'p',
        "no-run-if-empty" => b'r',
        "max-chars" => b's',
        "verbose" => b't',
        "show-limits" => b'S',
        "exit" => b'x',
        "max-procs" => b'P',
        "process-slot-var" => PROCESS_SLOT_VAR,
        "version" => b'v',
        _ => b'h',
    }
}

/// `ISBLANK(c)` — `isascii (c) && isblank (c)`, i.e. space and tab and nothing
/// else. `c` is an `int` because it is a `getc` result and may be `EOF`.
fn is_blank(c: i32) -> bool {
    c == i32::from(b' ') || c == i32::from(b'\t')
}

/// `ISSPACE(c)` — `ISBLANK` plus the four vertical motions. Note that this is
/// *not* C's `isspace`: it is spelled out so that a locale cannot widen it.
fn is_space(c: i32) -> bool {
    is_blank(c) || c == i32::from(b'\n') || c == i32::from(b'\r') || c == 0x0c || c == 0x0b
}

/// Upstream's `struct buildcmd_control`: the limits, fixed once at startup.
struct Ctl {
    posix_arg_size_min: usize,
    posix_arg_size_max: usize,
    exit_if_size_exceeded: bool,
    max_arg_count: usize,
    arg_max: usize,
    replace_pat: Option<Vec<u8>>,
    rplen: usize,
    initial_argc: usize,
    lines_per_exec: usize,
    args_per_exec: usize,
}

/// Upstream's `struct buildcmd_state`: the command line being accumulated.
///
/// `cmd_argv` holds `None` where upstream holds the terminating `NULL` pointer,
/// because `cmd_argc` counts it and `do_exec` copies it.
///
/// `cmd_argc` is a separate field rather than `cmd_argv.len()` because
/// `bc_clear_args` lowers the count *without* releasing the entries, and the
/// difference is observable: with `-I`, `initial_argc` stays 0, so after the
/// final `bc_clear_args` upstream's `cmd_argv[0]` still points at the command
/// name — which is what the "exited with status 255" diagnostic from the
/// closing `atexit` reap names.
struct BuildState {
    cmd_argv: Vec<Option<Vec<u8>>>,
    cmd_argc: usize,
    cmd_argv_chars: usize,
    cmd_initial_argv_chars: usize,
    largest_successful_arg_count: usize,
    smallest_failed_arg_count: usize,
}

/// Which way `bc_init_controlinfo` came out. Upstream returns
/// `BC_INIT_ENV_TOO_BIG` rather than dying, because `--show-limits` wants to
/// print the numbers that make the environment too big.
enum InitStatus {
    Ok,
    EnvTooBig,
}

/// Everything upstream keeps in file-scope statics plus `main`'s locals, so
/// that the transcribed functions can keep their upstream shape.
struct Xargs {
    ctl: Ctl,
    state: BuildState,

    /// Upstream's `const char *input_file = "-"` — the name `-a` last named,
    /// still the literal `"-"` (meaning stdin) when `-a` was never given.
    /// Compared against `"-"` rather than tested for emptiness because
    /// `xargs -a ''` must attempt to open the empty name and fail.
    input_file: Vec<u8>,
    /// The input `FILE *`: stdin, or `-a`'s file.
    input: BufReader<Box<dyn Read>>,
    /// Upstream's `initial_args`: true while the words of argv are being
    /// pushed, false once the first item from the input has been.
    initial_args: bool,
    /// One `read_line`/`read_string` `static bool eof`. Only one of the two
    /// readers is ever used in a run, so one flag serves both.
    read_eof: bool,
    /// True when `read_args` is `read_string` — set by `-0` and by `-d`.
    read_string_mode: bool,
    /// Upstream's `input_delimiter`, which is a plain `char` and therefore
    /// *signed* on x86-64. Held sign-extended so that a delimiter of 0x80 or
    /// above compares unequal to every `getc` result, exactly as upstream's
    /// does.
    input_delimiter: i32,
    eof_str: Option<Vec<u8>>,
    linebuf: Vec<u8>,
    lineno: usize,
    /// Set once the "null character seen" warning has been given.
    nullwarning_given: bool,

    print_command: bool,
    query_before_executing: bool,
    open_tty: bool,
    keep_stdin: bool,
    always_run_command: bool,
    show_limits: bool,
    slot_var_name: Option<Vec<u8>>,
    /// `print_args`'s `static FILE *tty_stream`, opened at most once.
    tty_stream: Option<BufReader<std::fs::File>>,

    proc_max: i64,
    pids: Vec<Option<Child>>,
    procs_executing: usize,
    procs_executed: bool,
    child_error: u8,
    original_exit_value: u8,
    waiting: bool,

    /// `bc_size_of_environment ()`, snapshotted before the option loop because
    /// that is when `bc_init_controlinfo` reads it.
    env_size: usize,
    /// The same after `--process-slot-var`'s `unsetenv`, which `--show-limits`
    /// reads *again* and so reports a smaller number for.
    env_size_live: usize,
    /// Upstream's `act_on_init_result`, deferred so that `xargs --help` works
    /// even when the environment is too big to exec anything.
    init_status: InitStatus,
}

fn main() {
    stdfd::restore();
    let mut xargs = Xargs::new();
    let status = xargs.run();
    // atexit LIFO: wait_for_proc_all was registered last, so it runs first and
    // may still override the status with 123.
    xargs.original_exit_value = status;
    xargs.wait_for_proc_all();
    stdfd::exit_now(status, status)
}

impl Xargs {
    fn new() -> Self {
        Xargs {
            ctl: Ctl {
                posix_arg_size_min: 0,
                posix_arg_size_max: 0,
                exit_if_size_exceeded: false,
                max_arg_count: 0,
                arg_max: 0,
                replace_pat: None,
                rplen: 0,
                initial_argc: 0,
                lines_per_exec: 0,
                args_per_exec: 0,
            },
            state: BuildState {
                cmd_argv: Vec::new(),
                cmd_argc: 0,
                cmd_argv_chars: 0,
                cmd_initial_argv_chars: 0,
                largest_successful_arg_count: 0,
                smallest_failed_arg_count: 0,
            },
            input_file: b"-".to_vec(),
            input: BufReader::new(Box::new(std::io::empty())),
            initial_args: true,
            read_eof: false,
            read_string_mode: false,
            input_delimiter: 0,
            eof_str: None,
            linebuf: Vec::new(),
            lineno: 0,
            nullwarning_given: false,
            print_command: false,
            query_before_executing: false,
            open_tty: false,
            keep_stdin: false,
            always_run_command: true,
            show_limits: false,
            slot_var_name: None,
            tty_stream: None,
            proc_max: 1,
            pids: Vec::new(),
            procs_executing: 0,
            procs_executed: false,
            child_error: 0,
            original_exit_value: 0,
            waiting: false,
            env_size: 0,
            env_size_live: 0,
            init_status: InitStatus::Ok,
        }
    }

    // ---- diagnostics -----------------------------------------------------

    /// `error (0, 0, …)` — a warning that does not exit.
    fn warn(text: &str) {
        Self::warn_bytes(text.as_bytes());
    }

    /// The same, for a message that embeds a command name and so is bytes
    /// rather than text. Upstream prints `argv[0]` with a bare `%s`.
    fn warn_bytes(text: &[u8]) {
        let mut line = b"xargs: ".to_vec();
        line.extend_from_slice(text);
        line.push(b'\n');
        stdfd::diag_bytes(&line);
    }

    /// `die (status, 0, …)` for a message carrying bytes.
    fn die_bytes(&mut self, status: u8, text: &[u8]) -> ! {
        Self::warn_bytes(text);
        self.exit_via_atexit(status)
    }

    /// `die (status, 0, …)` — the message, then every exit path's `atexit`
    /// handlers.
    fn die(&mut self, status: u8, text: &str) -> ! {
        Self::warn(text);
        self.exit_via_atexit(status)
    }

    /// `die (status, errno, …)` — glibc's `error` appends `: strerror (errnum)`.
    fn die_errno(&mut self, status: u8, text: &str, e: &std::io::Error) -> ! {
        let msg = format!("{text}: {}", strerror(e));
        self.die(status, &msg)
    }

    /// `usage (EXIT_FAILURE)`: the referral only, on stderr.
    fn usage_failure(&mut self) -> ! {
        Self::warn_raw("Try 'xargs --help' for more information.");
        self.exit_via_atexit(XARGS_FAILURE)
    }

    /// A line that already carries whatever prefix it needs — `usage`'s
    /// referral has none, and `--show-limits` prints six of them.
    fn warn_raw(text: &str) {
        stdfd::diag_line(text);
    }

    /// Everything `exit ()` does here: the reaper first (it was registered
    /// last), then the status it may have replaced.
    fn exit_via_atexit(&mut self, status: u8) -> ! {
        self.wait_for_proc_all();
        stdfd::exit_now(status, status)
    }
}
// ---- the readers --------------------------------------------------------

/// The state machine of `read_line`. It starts in `Space` so that leading
/// blanks are always stripped, even under `-i`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadState {
    Norm,
    Space,
    Quote,
    Backslash,
}

impl Xargs {
    /// `getc (input_stream)`.
    ///
    /// A read error becomes EOF, which is what `getc` reports to a caller that
    /// never consults `ferror` — and xargs never does. It is why `xargs -a .`
    /// runs the command once with no arguments instead of complaining about
    /// EISDIR: the open succeeds and every read fails.
    fn getc(&mut self) -> i32 {
        let mut byte = [0u8; 1];
        match self.input.read(&mut byte) {
            Ok(1) => i32::from(byte[0]),
            _ => -1,
        }
    }

    /// `EOF_STR (linebuf)`, where `len` counts the terminating NUL.
    ///
    /// Upstream compares C strings, so the comparison stops at the first NUL in
    /// `linebuf` — which an input NUL can put there, warning and all.
    fn eof_str_matches(&self, len: usize) -> bool {
        let Some(eof_str) = self.eof_str.as_deref() else {
            return false;
        };
        let text = self.linebuf.get(..len.saturating_sub(1)).unwrap_or(&[]);
        let text = match text.iter().position(|&b| b == 0) {
            Some(at) => text.get(..at).unwrap_or(&[]),
            None => text,
        };
        eof_str == text
    }

    /// `endbuf - linebuf`: including the NUL, the args must not grow past here.
    ///
    /// Upstream forms this as a pointer, `linebuf + arg_max -
    /// cmd_initial_argv_chars - 1`, which is undefined when the initial
    /// arguments are longer than `arg_max`. Saturating at 0 is what that
    /// undefined behaviour amounts to in practice: the first byte read trips
    /// the check.
    fn endpos(&self) -> usize {
        self.ctl
            .arg_max
            .saturating_sub(self.state.cmd_initial_argv_chars)
            .saturating_sub(1)
    }

    /// `(*read_args) ()` — `read_line` unless `-0` or `-d` chose `read_string`.
    fn read_args(&mut self) -> i32 {
        if self.read_string_mode {
            self.read_string()
        } else {
            self.read_line()
        }
    }

    /// Read a line of arguments from the input and add them to the list of
    /// arguments to pass to the command. Ignore blank lines and initial blanks.
    /// Single and double quotes and backslashes quote metacharacters and blanks
    /// as they do in the shell.
    ///
    /// Return -1 if eof (either physical or logical) is reached, otherwise the
    /// length of the last string read (including the null).
    #[allow(clippy::too_many_lines)]
    fn read_line(&mut self) -> i32 {
        let mut state = ReadState::Space;
        let mut quotc = 0i32;
        let mut c = -1i32;
        let mut first = true;
        let mut seen_arg = false;
        let mut p = 0usize;
        let endbuf = self.endpos();

        if self.read_eof {
            return -1;
        }
        loop {
            let prevc = c;
            c = self.getc();

            if c == -1 {
                // COMPAT: SYSV seems to ignore stuff on a line that ends
                // without a \n; we don't.
                self.read_eof = true;
                if p == 0 {
                    return -1;
                }
                self.put(p, 0);
                p = p.saturating_add(1);
                let len = p;
                if state == ReadState::Quote {
                    self.exec_if_possible();
                    let which = if quotc == i32::from(b'"') {
                        "double"
                    } else {
                        "single"
                    };
                    self.die(
                        XARGS_FAILURE,
                        &format!(
                            "unmatched {which} quote; by default quotes are \
                             special to xargs unless you use the -0 option"
                        ),
                    );
                }
                if first && self.eof_str_matches(len) {
                    return -1;
                }
                if self.ctl.replace_pat.is_none() {
                    self.push_line(len);
                }
                return truncate_len(len);
            }

            if state == ReadState::Space {
                if is_space(c) {
                    continue;
                }
                // C's `FALLTHROUGH` from SPACE into NORM.
                state = ReadState::Norm;
            }

            match state {
                ReadState::Space | ReadState::Norm => {
                    if c == i32::from(b'\n') {
                        if !is_blank(prevc) {
                            self.lineno = self.lineno.saturating_add(1); // For -l.
                        }
                        if p == 0 && !seen_arg {
                            // Blank line.
                            state = ReadState::Space;
                            continue;
                        }
                        // An empty argument is added to the list as normal.
                        self.put(p, 0);
                        p = p.saturating_add(1);
                        let len = p;
                        if self.eof_str_matches(len) {
                            self.read_eof = true;
                            return if first { -1 } else { truncate_len(len) };
                        }
                        if self.ctl.replace_pat.is_none() {
                            self.push_line(len);
                        }
                        return truncate_len(len);
                    }
                    seen_arg = true;

                    // POSIX: in the POSIX locale the separators are <SPC> and
                    // <TAB>, but not <FF> or <VT>.
                    if self.ctl.replace_pat.is_none() && is_blank(c) {
                        self.put(p, 0);
                        p = p.saturating_add(1);
                        let len = p;
                        if self.eof_str_matches(len) {
                            self.read_eof = true;
                            return if first { -1 } else { truncate_len(len) };
                        }
                        self.push_line(len);
                        p = 0;
                        state = ReadState::Space;
                        first = false;
                        continue;
                    }
                    if c == i32::from(b'\\') {
                        state = ReadState::Backslash;
                        continue;
                    }
                    if c == i32::from(b'\'') || c == i32::from(b'"') {
                        state = ReadState::Quote;
                        quotc = c;
                        continue;
                    }
                }
                ReadState::Quote => {
                    if c == i32::from(b'\n') {
                        self.exec_if_possible();
                        let which = if quotc == i32::from(b'"') {
                            "double"
                        } else {
                            "single"
                        };
                        self.die(
                            XARGS_FAILURE,
                            &format!(
                                "unmatched {which} quote; by default quotes are \
                                 special to xargs unless you use the -0 option"
                            ),
                        );
                    }
                    if c == quotc {
                        state = ReadState::Norm;
                        // Makes a difference for e.g. just '' or "" as the
                        // first arg on a line.
                        seen_arg = true;
                        continue;
                    }
                }
                ReadState::Backslash => state = ReadState::Norm,
            }

            if c == 0 && !self.nullwarning_given {
                // This is just a warning message. We only issue it once.
                Self::warn(
                    "WARNING: a NUL character occurred in the input.  It cannot \
                     be passed through in the argument list.  Did you mean to \
                     use the --null option?",
                );
                self.nullwarning_given = true;
            }

            if p >= endbuf {
                self.exec_if_possible();
                self.die(XARGS_FAILURE, "argument line too long");
            }
            self.put(p, byte_of(c));
            p = p.saturating_add(1);
        }
    }

    /// Read a string (terminated by the delimiter, which may be NUL) from the
    /// input and add it to the list of arguments to pass to the command.
    ///
    /// The return value is the length of the added argument, including its
    /// terminating NUL. The added argument is always terminated by NUL, even if
    /// that is not the delimiter.
    ///
    /// If we reach physical EOF before seeing the delimiter, we treat any
    /// characters read as the final argument. If no argument was read (that is,
    /// we reached physical EOF before reading any characters) then -1 is
    /// returned.
    fn read_string(&mut self) -> i32 {
        let mut p = 0usize;
        let endbuf = self.endpos();

        if self.read_eof {
            return -1;
        }
        loop {
            let c = self.getc();
            if c == -1 {
                self.read_eof = true;
                if p == 0 {
                    return -1;
                }
                self.put(p, 0);
                p = p.saturating_add(1);
                let len = p;
                if self.ctl.replace_pat.is_none() {
                    self.push_line(len);
                }
                return truncate_len(len);
            }
            if c == self.input_delimiter {
                self.lineno = self.lineno.saturating_add(1); // For -l.
                self.put(p, 0);
                p = p.saturating_add(1);
                let len = p;
                if self.ctl.replace_pat.is_none() {
                    self.push_line(len);
                }
                return truncate_len(len);
            }
            if p >= endbuf {
                self.exec_if_possible();
                self.die(XARGS_FAILURE, "argument line too long");
            }
            self.put(p, byte_of(c));
            p = p.saturating_add(1);
        }
    }

    /// `linebuf[at] = value`, growing the buffer if `arg_max` left it short.
    fn put(&mut self, at: usize, value: u8) {
        if at >= self.linebuf.len() {
            self.linebuf.resize(at.saturating_add(1), 0);
        }
        if let Some(slot) = self.linebuf.get_mut(at) {
            *slot = value;
        }
    }

    /// `bc_push_arg (…, linebuf, len, NULL, 0, initial_args)`.
    fn push_line(&mut self, len: usize) {
        let arg = self.linebuf.get(..len).unwrap_or(&[]).to_vec();
        let initial = self.initial_args;
        self.push_arg(&arg, len, initial);
    }
}

/// `read_line` and `read_string` return an `int`, so a length past `INT_MAX`
/// would wrap. `arg_max` is far below that, but the conversion has to be
/// spelled somewhere and this is where.
fn truncate_len(len: usize) -> i32 {
    i32::try_from(len).unwrap_or(i32::MAX)
}

/// The low byte of a `getc` result, which is all `*p++ = c` keeps.
fn byte_of(c: i32) -> u8 {
    u8::try_from(c & 0xff).unwrap_or(0)
}
// ---- buildcmd: accumulating and flushing a command line ------------------

impl Xargs {
    /// `bc_argc_limit_reached`.
    ///
    /// Return true if there would not be enough room for an additional
    /// argument. We check the total number of arguments only, not the space
    /// occupied by those arguments. If we return false, there still may not be
    /// enough room for the next argument, depending on its length.
    fn argc_limit_reached(&self, initial_args: bool) -> bool {
        // Check to see if we are about to exceed a limit set by -n.
        if !initial_args
            && self.ctl.args_per_exec != 0
            && self.state.cmd_argc.saturating_sub(self.ctl.initial_argc) == self.ctl.args_per_exec
        {
            return true;
        }
        // Upstream deliberately uses an equality test here rather than >=, to
        // force a software failure if a new argument ever skips this check.
        self.state.cmd_argc == self.ctl.max_arg_count
    }

    /// `bc_args_complete` — push the terminating NULL.
    fn push_terminator(&mut self) {
        self.store(None);
    }

    /// `bc_push_arg (…, arg, len, NULL, 0, initial_args)`.
    ///
    /// `len` is the length of `arg` **including** the terminating NUL. xargs
    /// never passes a prefix, so the `prefix`/`pfxlen` parameters upstream
    /// carries for `find -exec` are dropped here.
    fn push_arg(&mut self, arg: &[u8], len: usize, initial_args: bool) {
        if self.state.cmd_argv_chars.saturating_add(len) > self.ctl.arg_max {
            if initial_args || self.state.cmd_argc == self.ctl.initial_argc {
                self.die(
                    XARGS_FAILURE,
                    "cannot fit single argument within argument list size limit",
                );
            }
            // xargs option -i (replace_pat) implies -x (exit_if_size_exceeded).
            if self.ctl.replace_pat.is_some()
                || (self.ctl.exit_if_size_exceeded
                    && (self.ctl.lines_per_exec != 0 || self.ctl.args_per_exec != 0))
            {
                self.die(XARGS_FAILURE, "argument list too long");
            }
            self.do_exec();
        }
        if self.argc_limit_reached(initial_args) {
            self.do_exec();
        }

        // `strcpy` stops at the first NUL; `cmd_argv_chars` is charged the full
        // `len` regardless. An input item with an embedded NUL therefore
        // reaches the child truncated, and the warning `read_line` gives is the
        // only notice of it.
        let stored = match arg.iter().position(|&b| b == 0) {
            Some(at) => arg.get(..at).unwrap_or(&[]).to_vec(),
            None => arg.to_vec(),
        };
        self.store(Some(stored));
        self.state.cmd_argv_chars = self.state.cmd_argv_chars.saturating_add(len);

        // If we have now collected enough arguments, do the exec immediately.
        if self.argc_limit_reached(initial_args) {
            self.do_exec();
        }

        // If this is an initial argument, set the high-water mark.
        if initial_args {
            self.state.cmd_initial_argv_chars = self.state.cmd_argv_chars;
        }
    }

    /// `state->cmd_argv[state->cmd_argc++] = …`, keeping any entry past
    /// `cmd_argc` that a previous command line left behind.
    fn store(&mut self, value: Option<Vec<u8>>) {
        let at = self.state.cmd_argc;
        if at < self.state.cmd_argv.len() {
            if let Some(slot) = self.state.cmd_argv.get_mut(at) {
                *slot = value;
            }
        } else {
            self.state.cmd_argv.push(value);
        }
        self.state.cmd_argc = at.saturating_add(1);
    }

    /// `bc_clear_args` — reset the count, not the storage.
    fn clear_args(&mut self) {
        self.state.cmd_argc = self.ctl.initial_argc;
        self.state.cmd_argv_chars = self.state.cmd_initial_argv_chars;
    }

    /// `copy_args`: all the initial arguments, plus up to `limit` more.
    ///
    /// The `< limit` bound counts the terminating NULL, which is deliberate:
    /// `limit` starts at `cmd_argc`, which includes it.
    fn copy_args(&self, limit: usize, done: usize) -> Vec<Option<Vec<u8>>> {
        let mut working: Vec<Option<Vec<u8>>> = Vec::new();
        let mut src_pos = 0usize;
        while src_pos < self.ctl.initial_argc {
            working.push(self.state.cmd_argv.get(src_pos).cloned().flatten());
            src_pos = src_pos.saturating_add(1);
        }
        src_pos = src_pos.saturating_add(done);
        while src_pos < self.state.cmd_argc && working.len() < limit {
            working.push(self.state.cmd_argv.get(src_pos).cloned().flatten());
            src_pos = src_pos.saturating_add(1);
        }
        // `working_args[dst_pos] = NULL` — the caller's `dst_pos` is the length
        // before this, so the NULL is not counted in the return value.
        working
    }

    /// `update_limit`: our best guess at how many arguments the next attempt
    /// should carry. Only reached when the exec came back `E2BIG`.
    fn update_limit(&mut self, success: bool, mut limit: usize) -> usize {
        if success {
            if limit > self.state.largest_successful_arg_count {
                self.state.largest_successful_arg_count = limit;
            }
        } else if limit < self.state.smallest_failed_arg_count
            || self.state.smallest_failed_arg_count == 0
        {
            self.state.smallest_failed_arg_count = limit;
        }

        if self.state.largest_successful_arg_count == 0
            || self.state.smallest_failed_arg_count <= self.state.largest_successful_arg_count
        {
            // No success yet, or running on a system which has limits on total
            // argv length, but not arg count.
            if success {
                limit = limit.saturating_add(1);
            } else {
                limit /= 2;
            }
        } else {
            // We can use bisection.
            let shift = self
                .state
                .smallest_failed_arg_count
                .saturating_sub(self.state.largest_successful_arg_count)
                / 2;
            if success {
                limit = limit.saturating_add(if shift == 0 { 1 } else { shift });
            } else {
                limit = limit.saturating_sub(if shift == 0 { 1 } else { shift });
            }
        }

        // Make sure the returned value is such that progress is possible.
        if self.ctl.initial_argc != 0 && limit <= self.ctl.initial_argc.saturating_add(1) {
            limit = self.ctl.initial_argc.saturating_add(1);
        }
        if limit == 0 {
            limit = 1;
        }
        limit
    }

    /// `bc_do_exec` — run the command with the currently-built argument list,
    /// shortening it and retrying for as long as the exec reports `E2BIG`.
    fn do_exec(&mut self) {
        // Terminate the args.
        self.push_terminator();

        let mut done = 0usize;
        let mut limit = self.state.cmd_argc;

        loop {
            let working = self.copy_args(limit, done);
            let dst_pos = working.len();
            if self.exec_callback(&working) {
                limit = self.update_limit(true, limit);
                done = done.saturating_add(dst_pos.saturating_sub(self.ctl.initial_argc));
            } else if limit <= self.ctl.initial_argc.saturating_add(1) {
                // No room to reduce the length of the argument list. Issue an
                // error message and give up.
                self.die(
                    XARGS_FAILURE,
                    "can't call exec() due to argument size restrictions",
                );
            } else {
                // Try fewer arguments.
                limit = self.update_limit(false, limit);
            }
            // `cmd_argc - initial_argc` includes the terminating NULL, which is
            // why 1 is added to `done` in the test.
            if done.saturating_add(1) >= self.state.cmd_argc.saturating_sub(self.ctl.initial_argc) {
                break;
            }
        }

        self.clear_args();
    }

    /// `bc_do_insert`: replace every instance of `replace_pat` in `arg` with
    /// `linebuf`, and add the result to the argument list.
    ///
    /// `arglen` is the length of `arg` not including the NUL; `lblen` likewise
    /// for the item. xargs passes no prefix.
    fn do_insert(&mut self, arg: &[u8], arglen: usize, item: &[u8], lblen: usize) {
        let mut insertbuf: Vec<u8> = Vec::new();
        let mut bytes_left = self.ctl.arg_max.saturating_sub(1);
        let mut at = 0usize;
        let mut arglen = arglen;
        let rplen = self.ctl.rplen;
        let pat = self.ctl.replace_pat.clone().unwrap_or_default();

        loop {
            let rest = arg.get(at..).unwrap_or(&[]);
            let found = find_sub(rest, &pat);
            let len = found.unwrap_or(arglen);

            if bytes_left <= len {
                break;
            }
            bytes_left = bytes_left.saturating_sub(len);

            insertbuf.extend_from_slice(rest.get(..len).unwrap_or(&[]));
            at = at.saturating_add(len);
            arglen = arglen.saturating_sub(len);

            if found.is_some() {
                if bytes_left <= lblen {
                    break;
                }
                bytes_left = bytes_left.saturating_sub(lblen);

                // Divergence 2: upstream `strcpy`s the item, truncating it at
                // an embedded NUL. The whole item is copied here.
                insertbuf.extend_from_slice(item.get(..lblen).unwrap_or(&[]));

                at = at.saturating_add(rplen);
                arglen = arglen.saturating_sub(rplen);

                // Divergence 3: an empty `replace_pat` matches at every
                // position without consuming anything, so upstream's
                // `do { … } while (*arg)` never terminates. Stop instead.
                if rplen == 0 && len == 0 {
                    break;
                }
            }

            if arg.get(at).copied().unwrap_or(0) == 0 {
                break;
            }
        }

        if arg.get(at).copied().unwrap_or(0) != 0 {
            self.die(XARGS_FAILURE, "command too long");
        }
        insertbuf.push(0);

        let len = insertbuf.len();
        let initial = self.initial_args;
        self.push_arg(&insertbuf, len, initial);
    }

    /// `exec_if_possible` — flush a partial command line before dying, but only
    /// where doing so is upstream's behaviour rather than a surprise.
    fn exec_if_possible(&mut self) {
        if self.ctl.replace_pat.is_some()
            || self.initial_args
            || self.state.cmd_argc == self.ctl.initial_argc
            || self.ctl.exit_if_size_exceeded
        {
            return;
        }
        self.do_exec();
    }
}

/// `mbsstr (haystack, needle)`, reduced to a byte search.
///
/// The multibyte version differs only for a needle that could appear
/// mid-character in a stateful encoding; in UTF-8, which is all this OS uses,
/// a byte match is a character match.
fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len().saturating_sub(needle.len()))
        .find(|&at| haystack.get(at..at.saturating_add(needle.len())) == Some(needle))
}
// ---- running and reaping -------------------------------------------------

impl Xargs {
    /// `bc_state.cmd_argv[0]` — the name every child diagnostic reports.
    ///
    /// Read past `cmd_argc` on purpose: see [`BuildState`].
    fn cmd_name(&self) -> Vec<u8> {
        self.state
            .cmd_argv
            .first()
            .cloned()
            .flatten()
            .unwrap_or_default()
    }

    /// `print_args` — write the command about to run, and under `-p` ask.
    ///
    /// Note the `cmd_argc - 1`: this renders the *accumulated* list, whose last
    /// entry is the terminating NULL that `bc_args_complete` has just pushed,
    /// rather than the `working_args` the child will actually receive. The two
    /// differ only when the exec is being retried after `E2BIG`.
    fn print_args(&mut self, ask: bool) -> bool {
        let mut out: Vec<u8> = Vec::new();
        for i in 0..self.state.cmd_argc.saturating_sub(1) {
            if i != 0 {
                out.push(b' ');
            }
            let arg = self
                .state
                .cmd_argv
                .get(i)
                .cloned()
                .flatten()
                .unwrap_or_default();
            out.extend_from_slice(&Style::ShellEscape.quote(&arg));
        }
        if let Err(e) = stdfd::write_all(2, &out) {
            self.die_errno(XARGS_FAILURE, "Failed to write to stderr", &e);
        }

        if ask {
            if self.tty_stream.is_none() {
                match std::fs::File::open("/dev/tty") {
                    Ok(file) => self.tty_stream = Some(BufReader::new(file)),
                    Err(e) => {
                        self.die_errno(XARGS_FAILURE, "failed to open /dev/tty for reading", &e);
                    }
                }
            }
            if let Err(e) = stdfd::write_all(2, b"?...") {
                self.die_errno(XARGS_FAILURE, "Failed to write to stderr", &e);
            }

            let mut c = self.tty_getc();
            let savec = c;
            while c != -1 && c != i32::from(b'\n') {
                c = self.tty_getc();
            }
            if c == -1 {
                let e = std::io::Error::last_os_error();
                self.die_errno(XARGS_FAILURE, "Failed to read from stdin", &e);
            }
            if savec == i32::from(b'y') || savec == i32::from(b'Y') {
                return true;
            }
        } else {
            // Upstream's `putc ('\n', stderr)`, which it does not check.
            let _ = stdfd::write_all(2, b"\n");
        }
        false
    }

    /// `getc (tty_stream)`.
    fn tty_getc(&mut self) -> i32 {
        let Some(stream) = self.tty_stream.as_mut() else {
            return -1;
        };
        let mut byte = [0u8; 1];
        match stream.read(&mut byte) {
            Ok(1) => i32::from(byte[0]),
            _ => -1,
        }
    }

    /// The index `add_proc` would hand out — which is what upstream's *child*
    /// computes with `add_proc (0)` to name its own slot.
    fn free_slot(&self) -> usize {
        self.pids
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.pids.len())
    }

    /// `add_proc` — record a running child and return the slot it occupies.
    fn add_proc(&mut self, child: Child) -> usize {
        let slot = self.free_slot();
        if slot >= self.pids.len() {
            self.pids.resize_with(slot.saturating_add(1), || None);
        }
        if let Some(entry) = self.pids.get_mut(slot) {
            *entry = Some(child);
        }
        self.procs_executing = self.procs_executing.saturating_add(1);
        self.procs_executed = true;
        slot
    }

    /// `xargs_do_exec` — the `exec_callback`. Returns false only for `E2BIG`,
    /// which tells `do_exec` to retry with a shorter list.
    fn exec_callback(&mut self, working: &[Option<Vec<u8>>]) -> bool {
        if self.proc_max != 0 {
            while i64::try_from(self.procs_executing).unwrap_or(i64::MAX) >= self.proc_max {
                self.wait_for_proc(false, 1);
            }
        }

        // `-p` and the user said no: report success and build the next line.
        if self.query_before_executing && !self.print_args(true) {
            return true;
        }
        if !self.query_before_executing && self.print_command {
            self.print_args(false);
        }

        // Before forking, reap any already-exited child, so that unreaped
        // children do not pile up while the next command line is built.
        self.wait_for_proc(false, 0);

        let argv: Vec<Vec<u8>> = working
            .iter()
            .take_while(|entry| entry.is_some())
            .filter_map(|entry| entry.clone())
            .collect();
        let Some((program, rest)) = argv.split_first() else {
            return true;
        };

        if self.args_exceed_testing_limit(&argv) {
            return false;
        }

        let mut command = Command::new(os_from_bytes(program));
        for arg in rest {
            command.arg(os_from_bytes(arg));
        }

        // `prep_child_for_exec`: the slot the child would have computed for
        // itself, then its standard input.
        let slot = self.free_slot();
        if let Some(name) = self.slot_var_name.clone() {
            command.env(os_from_bytes(&name), slot.to_string());
        }
        if self.keep_stdin && !self.open_tty {
            command.stdin(Stdio::inherit());
        } else if self.open_tty {
            match std::fs::File::open("/dev/tty") {
                Ok(file) => {
                    command.stdin(Stdio::from(file));
                }
                Err(e) => {
                    let text = quote(b"/dev/tty");
                    self.die_errno(XARGS_FAILURE, &text, &e);
                }
            }
        } else {
            command.stdin(Stdio::null());
        }

        match command.spawn() {
            Ok(child) => {
                self.add_proc(child);
                true
            }
            Err(e) => {
                // E2BIG is the one failure the caller can do something about,
                // and the one upstream's child does not report.
                if e.raw_os_error() == Some(E2BIG) {
                    return false;
                }
                let mut line = program.clone();
                line.extend_from_slice(b": ");
                line.extend_from_slice(strerror(&e).as_bytes());
                Self::warn_bytes(&line);
                let status = if e.kind() == std::io::ErrorKind::NotFound {
                    XARGS_EXIT_COMMAND_NOT_FOUND
                } else {
                    XARGS_EXIT_COMMAND_CANNOT_BE_RUN
                };
                self.exit_via_atexit(status)
            }
        }
    }

    /// `wait_for_proc`. If `all`, wait for every child; otherwise reap at least
    /// `minreap` and then take whatever else is already dead without blocking.
    fn wait_for_proc(&mut self, all: bool, minreap: usize) {
        let mut reaped = 0usize;

        while self.procs_executing != 0 {
            let nohang = !all && reaped >= minreap;
            let Some((slot, status)) = self.reap_one(nohang) else {
                if !nohang {
                    // Should not happen: `procs_executing` is the number of
                    // children still running, so the loop should have ended.
                    let n = self.procs_executing;
                    Self::warn(&format!("WARNING: Lost track of {n} child processes"));
                }
                break;
            };

            // Remove the child from the list.
            if let Some(entry) = self.pids.get_mut(slot) {
                *entry = None;
            }
            self.procs_executing = self.procs_executing.saturating_sub(1);
            reaped = reaped.saturating_add(1);

            let name = self.cmd_name();
            if status.code() == Some(CHILD_EXIT_PLEASE_STOP_IMMEDIATELY) {
                let mut line = name;
                line.extend_from_slice(b": exited with status 255; aborting");
                self.die_bytes(XARGS_EXIT_CLIENT_EXIT_255, &line);
            }
            // Upstream also tests `WIFSTOPPED`, which `waitpid` without
            // `WUNTRACED` can never report; there is no equivalent here.
            if let Some(signal) = terminating_signal(&status) {
                let mut line = name;
                line.extend_from_slice(format!(": terminated by signal {signal}").as_bytes());
                self.die_bytes(XARGS_EXIT_CLIENT_FATAL_SIG, &line);
            }
            if status.code() != Some(0) {
                self.child_error = XARGS_EXIT_CLIENT_EXIT_NONZERO;
            }
        }
    }

    /// One `waitpid`, as far as `std::process::Child` can express it.
    ///
    /// Divergence 4: with several children this returns whichever *slot* is
    /// ready first rather than whichever *child* exited first. With the default
    /// `-P 1` there is one child and the two coincide.
    fn reap_one(&mut self, nohang: bool) -> Option<(usize, std::process::ExitStatus)> {
        let live: Vec<usize> = (0..self.pids.len())
            .filter(|&i| self.pids.get(i).is_some_and(Option::is_some))
            .collect();
        if live.is_empty() {
            return None;
        }

        if !nohang && live.len() == 1 {
            let slot = *live.first()?;
            let outcome = self
                .pids
                .get_mut(slot)
                .and_then(Option::as_mut)
                .map(Child::wait)?;
            return match outcome {
                Ok(status) => Some((slot, status)),
                Err(e) => self.die_errno(XARGS_FAILURE, "error waiting for child process", &e),
            };
        }

        loop {
            for &slot in &live {
                let outcome = self
                    .pids
                    .get_mut(slot)
                    .and_then(Option::as_mut)
                    .map(Child::try_wait);
                match outcome {
                    Some(Ok(Some(status))) => return Some((slot, status)),
                    Some(Ok(None)) | None => {}
                    Some(Err(e)) => {
                        self.die_errno(XARGS_FAILURE, "error waiting for child process", &e);
                    }
                }
            }
            if nohang {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// `wait_for_proc_all`, the `atexit` handler registered last and therefore
    /// run first — which is how a child that fails during the closing reap can
    /// still turn a successful run into 123.
    fn wait_for_proc_all(&mut self) {
        if self.waiting {
            return;
        }
        self.waiting = true;
        self.wait_for_proc(true, 0);
        self.waiting = false;

        if self.original_exit_value != self.child_error {
            stdfd::exit_now(self.child_error, self.child_error);
        }
    }

    /// `exceeds` — nonzero if the value in `name` is smaller than `quantity`.
    fn exceeds(&mut self, name: &str, quantity: usize) -> bool {
        let Some(value) = std::env::var_os(name) else {
            return false;
        };
        let bytes = os_bytes(&value);
        let (limit, rest) = strtoul(&bytes);
        if rest.is_empty() && !bytes.is_empty() {
            return u128::from(u64::try_from(quantity).unwrap_or(u64::MAX)) > limit;
        }
        self.die(
            XARGS_FAILURE,
            &format!("Environment variable {name} is not set to a valid decimal number"),
        )
    }

    /// `bc_args_exceed_testing_limit` — the hook the findutils test suite uses
    /// to make an exec fail with `E2BIG` without needing an argument list that
    /// really is too long.
    fn args_exceed_testing_limit(&mut self, argv: &[Vec<u8>]) -> bool {
        let args = argv.len();
        let chars = argv.iter().map(Vec::len).sum();
        self.exceeds("__GNU_FINDUTILS_EXEC_ARG_COUNT_LIMIT", args)
            || self.exceeds("__GNU_FINDUTILS_EXEC_ARG_LENGTH_LIMIT", chars)
    }
}

/// `E2BIG`, which `std::io::ErrorKind` only learned to name in a form that is
/// still unstable for some targets.
const E2BIG: i32 = 7;

/// `WIFSIGNALED (status) ? WTERMSIG (status) : 0`.
#[cfg(unix)]
fn terminating_signal(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

/// Off Unix a process cannot be killed by a signal, so the ladder skips this
/// rung. The crate builds for Windows too, where this is the honest answer.
#[cfg(not(unix))]
fn terminating_signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}
// ---- limits --------------------------------------------------------------

impl Xargs {
    /// `bc_init_controlinfo`. Both of upstream's failure returns lead to the
    /// same `fail_due_to_env_size`, so they are one variant here.
    fn init_controlinfo(&mut self, headroom: usize) -> InitStatus {
        let environment = self.env_size;

        // POSIX requires that _POSIX_ARG_MAX is 4096. That is the lowest
        // possible value for ARG_MAX on a POSIX compliant system.
        self.ctl.posix_arg_size_min = POSIX_ARG_SIZE_MIN;
        self.ctl.posix_arg_size_max = bc_get_arg_max();
        self.ctl.exit_if_size_exceeded = false;

        // Take the size of the environment into account.
        if environment > self.ctl.posix_arg_size_max {
            return InitStatus::EnvTooBig;
        }
        if headroom.saturating_add(environment) >= self.ctl.posix_arg_size_max {
            // POSIX.2 requires xargs to subtract 2048, and ARG_MAX is
            // guaranteed to be at least 4096; a system where that does not
            // leave room is one xargs cannot be POSIX-compliant on.
            return InitStatus::EnvTooBig;
        }
        self.ctl.posix_arg_size_max = self
            .ctl
            .posix_arg_size_max
            .saturating_sub(environment)
            .saturating_sub(headroom);

        // The 2 subtracted on the next line is for Linux/PPC.
        self.ctl.max_arg_count = (self.ctl.posix_arg_size_max / SIZEOF_CHAR_PTR).saturating_sub(2);
        self.ctl.rplen = 0;
        self.ctl.replace_pat = None;
        self.ctl.initial_argc = 0;
        self.ctl.lines_per_exec = 0;
        self.ctl.args_per_exec = 0;

        // Start with the largest value we can tolerate; -s narrows it.
        self.ctl.arg_max = self.ctl.posix_arg_size_max;

        InitStatus::Ok
    }

    /// `bc_use_sensible_arg_max` — 128 KiB, clamped into the POSIX range.
    fn use_sensible_arg_max(&mut self) {
        self.ctl.arg_max = if SENSIBLE_ARG_SIZE > self.ctl.posix_arg_size_max {
            self.ctl.posix_arg_size_max
        } else if SENSIBLE_ARG_SIZE < self.ctl.posix_arg_size_min {
            self.ctl.posix_arg_size_min
        } else {
            SENSIBLE_ARG_SIZE
        };
    }

    /// `act_on_init_result ()` — `fail_due_to_env_size` or nothing.
    fn act_on_init_result(&mut self) {
        if matches!(self.init_status, InitStatus::EnvTooBig) {
            self.die(XARGS_FAILURE, "environment is too large for exec");
        }
    }
}

/// `sizeof (char *)`, which decides `max_arg_count`.
const SIZEOF_CHAR_PTR: usize = 8;

/// `bc_get_arg_max ()` — `sysconf (_SC_ARG_MAX)`.
///
/// glibc computes that as `MAX (legacy_ARG_MAX, RLIMIT_STACK.rlim_cur / 4)`,
/// and answers `legacy_ARG_MAX` outright when the stack is unlimited. Reading
/// the soft stack limit out of `/proc/self/limits` reproduces it without a libc
/// binding; if the file cannot be read the legacy floor is the answer, which is
/// also what an unlimited stack gives.
fn bc_get_arg_max() -> usize {
    match stack_limit() {
        Some(stack) => std::cmp::max(LEGACY_ARG_MAX, stack / 4),
        None => LEGACY_ARG_MAX,
    }
}

/// The soft `RLIMIT_STACK`, or `None` for `unlimited` (and for any system that
/// does not publish one).
fn stack_limit() -> Option<usize> {
    let limits = std::fs::read_to_string("/proc/self/limits").ok()?;
    for line in limits.lines() {
        if let Some(rest) = line.strip_prefix("Max stack size") {
            // Columns are blank-padded: soft, hard, units.
            let soft = rest.split_whitespace().next()?;
            if soft == "unlimited" {
                return None;
            }
            return soft.parse().ok();
        }
    }
    None
}

/// `bc_size_of_environment ()` — `strlen (*envp) + 1` over `environ`, where
/// each entry is `KEY=VALUE`.
fn size_of_environment() -> usize {
    let mut len = 0usize;
    for (key, value) in std::env::vars_os() {
        len = len
            .saturating_add(os_bytes(&key).len())
            .saturating_add(os_bytes(&value).len())
            .saturating_add(2);
    }
    len
}

// ---- option arguments ----------------------------------------------------

impl Xargs {
    /// `parse_num`.
    ///
    /// Three things about it are not what a reimplementer would write. It
    /// prints through a bare `fprintf` rather than `error`; it always names the
    /// **short** letter, so `--max-lines=0` is reported against `-l`; and the
    /// "invalid number" branch exits whatever `fatal` says, because `usage
    /// (EXIT_FAILURE)` is followed by an unconditional `exit`.
    fn parse_num(&mut self, text: &[u8], option: u8, min: i64, max: i64, fatal: bool) -> i64 {
        let (val, rest, converted) = strtol(text);
        if !converted || !rest.is_empty() {
            let mut line = b"xargs: invalid number \"".to_vec();
            line.extend_from_slice(text);
            line.extend_from_slice(format!("\" for -{} option\n", option as char).as_bytes());
            stdfd::diag_bytes(&line);
            self.usage_failure();
        }
        if val < min {
            let mut line = b"xargs: value ".to_vec();
            line.extend_from_slice(text);
            line.extend_from_slice(
                format!(" for -{} option should be >= {min}\n", option as char).as_bytes(),
            );
            stdfd::diag_bytes(&line);
            if fatal {
                self.usage_failure();
            }
            return min;
        }
        if max >= 0 && val > max {
            let mut line = b"xargs: value ".to_vec();
            line.extend_from_slice(text);
            line.extend_from_slice(
                format!(" for -{} option should be <= {max}\n", option as char).as_bytes(),
            );
            stdfd::diag_bytes(&line);
            if fatal {
                self.usage_failure();
            }
            return max;
        }
        val
    }

    /// `get_char_oct_or_hex_escape`.
    fn char_oct_or_hex_escape(&mut self, s: &[u8]) -> u8 {
        let second = s.get(1).copied().unwrap_or(0);
        let (digits, base) = if second == b'x' {
            (s.get(2..).unwrap_or(&[]), 16u32)
        } else if second.is_ascii_digit() {
            (s.get(1..).unwrap_or(&[]), 8u32)
        } else {
            let mut line = b"Invalid escape sequence ".to_vec();
            line.extend_from_slice(s);
            line.extend_from_slice(b" in input delimiter specification.");
            self.die_bytes(XARGS_FAILURE, &line);
        };

        let (val, overflowed, rest) = strtoul_base(digits, base);
        if overflowed || val > 255 {
            let ceiling = if base == 16 { "ff" } else { "377" };
            let mut line = b"Invalid escape sequence ".to_vec();
            line.extend_from_slice(s);
            line.extend_from_slice(
                format!(
                    " in input delimiter specification; character values must \
                     not exceed {ceiling}."
                )
                .as_bytes(),
            );
            self.die_bytes(XARGS_FAILURE, &line);
        }
        if !rest.is_empty() {
            let mut line = b"Invalid escape sequence ".to_vec();
            line.extend_from_slice(s);
            line.extend_from_slice(b" in input delimiter specification; trailing characters ");
            line.extend_from_slice(rest);
            line.extend_from_slice(b" not recognised.");
            self.die_bytes(XARGS_FAILURE, &line);
        }
        u8::try_from(val).unwrap_or(0)
    }

    /// `get_input_delimiter`.
    ///
    /// The return is a C `char`, which is signed here, so the result is
    /// sign-extended: `-d '\xff'` can never match a `getc` result, and upstream
    /// behaves the same way.
    fn get_input_delimiter(&mut self, s: &[u8]) -> i32 {
        if s.len() == 1 {
            let byte = s.first().copied().unwrap_or(0);
            return i32::from(byte as i8);
        }
        if s.first().copied() == Some(b'\\') {
            let byte = match s.get(1).copied().unwrap_or(0) {
                b'a' => 0x07,
                b'b' => 0x08,
                b'f' => 0x0c,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'v' => 0x0b,
                b'\\' => b'\\',
                _ => self.char_oct_or_hex_escape(s),
            };
            return i32::from(byte as i8);
        }
        let mut line = b"Invalid input delimiter specification ".to_vec();
        line.extend_from_slice(s);
        line.extend_from_slice(
            b": the delimiter must be either a single character or an escape \
              sequence starting with \\.",
        );
        self.die_bytes(XARGS_FAILURE, &line)
    }

    /// `warn_mutually_exclusive`. Note that the *offending* option is named
    /// twice and the new one once, in that order.
    fn warn_mutually_exclusive(option: &str, offending: &str) {
        Self::warn(&format!(
            "warning: options {offending} and {option} are mutually exclusive, \
             ignoring previous {offending} value"
        ));
    }
}

/// C's `strtol` in base 10, over a byte string with no terminator.
///
/// Returns the value, whatever follows the digits, and whether any conversion
/// happened at all — which is upstream's `eptr == str` test. Overflow
/// saturates, as `strtol` does when it sets `ERANGE`; `parse_num` never looks
/// at `errno`, so the saturated value is what it range-checks.
fn strtol(s: &[u8]) -> (i64, &[u8], bool) {
    let mut at = 0usize;
    while s
        .get(at)
        .copied()
        .is_some_and(|b| b.is_ascii_whitespace() || b == 0x0b)
    {
        at = at.saturating_add(1);
    }
    let negative = match s.get(at).copied() {
        Some(b'-') => {
            at = at.saturating_add(1);
            true
        }
        Some(b'+') => {
            at = at.saturating_add(1);
            false
        }
        _ => false,
    };

    let start = at;
    let mut magnitude: i128 = 0;
    while let Some(digit) = s.get(at).copied().filter(u8::is_ascii_digit) {
        if magnitude <= i128::from(i64::MAX) {
            magnitude = magnitude
                .saturating_mul(10)
                .saturating_add(i128::from(digit - b'0'));
        }
        at = at.saturating_add(1);
    }
    if at == start {
        // No conversion performed: `strtol` leaves `endptr` at the start.
        return (0, s, false);
    }

    let value = if negative {
        i64::try_from(-magnitude).unwrap_or(i64::MIN)
    } else {
        i64::try_from(magnitude).unwrap_or(i64::MAX)
    };
    (value, s.get(at..).unwrap_or(&[]), true)
}

/// C's `strtoul` in base 10, for `exceeds`. Returns the value and the tail.
fn strtoul(s: &[u8]) -> (u128, &[u8]) {
    let (value, overflowed, rest) = strtoul_base(s, 10);
    (if overflowed { u128::MAX } else { value }, rest)
}

/// C's `strtoul` in an arbitrary base, reporting overflow separately because
/// `get_char_oct_or_hex_escape` distinguishes `ERANGE` from a large value.
fn strtoul_base(s: &[u8], base: u32) -> (u128, bool, &[u8]) {
    let mut at = 0usize;
    while s
        .get(at)
        .copied()
        .is_some_and(|b| b.is_ascii_whitespace() || b == 0x0b)
    {
        at = at.saturating_add(1);
    }
    if s.get(at).copied() == Some(b'+') {
        at = at.saturating_add(1);
    }
    let start = at;
    let mut value: u128 = 0;
    let mut overflowed = false;
    while let Some(digit) = s
        .get(at)
        .copied()
        .and_then(|b| char::from(b).to_digit(base))
    {
        value = value
            .saturating_mul(u128::from(base))
            .saturating_add(u128::from(digit));
        if value > u128::from(u64::MAX) {
            overflowed = true;
            value = u128::from(u64::MAX);
        }
        at = at.saturating_add(1);
    }
    if at == start {
        return (0, false, s);
    }
    (value, overflowed, s.get(at..).unwrap_or(&[]))
}
// ---- main ----------------------------------------------------------------

impl Xargs {
    /// Everything upstream's `main` does, returning what it returns.
    #[allow(clippy::too_many_lines)]
    fn run(&mut self) -> u8 {
        self.env_size = size_of_environment();
        self.env_size_live = self.env_size;

        // xargs is required by POSIX to allow 2048 bytes of headroom for extra
        // environment variables that the utility might want to set before
        // execing something else.
        self.init_status = self.init_controlinfo(XARGS_POSIX_HEADROOM);

        // `bc_init_controlinfo` may have found the environment too big. The
        // complaint is deferred until after the option loop so that `xargs
        // --help` still works in that case.
        if matches!(self.init_status, InitStatus::Ok) {
            // IEEE Std 1003.1, 2003 specifies that the combined argument and
            // environment list shall not exceed {ARG_MAX} - 2048 bytes.
            let val = bc_get_arg_max();
            self.ctl.arg_max =
                std::cmp::min(self.ctl.arg_max, val.saturating_sub(XARGS_POSIX_HEADROOM));
            // Start with a reasonable default size, adjustable via -s.
            self.use_sensible_arg_max();
        }

        let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
        let optind = self.parse_options(&argv);

        if self.eof_str.is_some() && self.read_string_mode {
            // The format carries its own newline and `error` adds another, so a
            // blank line follows this warning.
            Self::warn("warning: the -E option has no effect if -0 or -d is used.\n");
        }

        // If failing due to the environment size was deferred, do it now.
        self.act_on_init_result();

        let input_file = self.input_file.clone();
        if input_file == b"-" {
            self.input = BufReader::new(Box::new(std::io::stdin()));
        } else {
            self.keep_stdin = true; // see prep_child_for_exec
            match std::fs::File::open(os_from_bytes(&input_file)) {
                Ok(file) => self.input = BufReader::new(Box::new(file)),
                Err(e) => {
                    let text = format!("Cannot open input file {}", quote(&input_file));
                    self.die_errno(XARGS_FAILURE, &text, &e);
                }
            }
        }

        if self.ctl.replace_pat.is_some() || self.ctl.lines_per_exec != 0 {
            self.ctl.exit_if_size_exceeded = true;
        }

        // SYSV xargs runs `echo` when no command is given.
        let mut words: Vec<Vec<u8>> = argv.get(optind..).unwrap_or(&[]).iter().map(cstr).collect();
        if words.is_empty() {
            words.push(b"echo".to_vec());
        }

        if self.show_limits {
            self.print_limits(&words);
        }

        self.linebuf = vec![0u8; self.ctl.arg_max.saturating_add(1)];

        if self.ctl.replace_pat.is_none() {
            for word in &words {
                let len = word.len().saturating_add(1);
                let initial = self.initial_args;
                let mut arg = word.clone();
                arg.push(0);
                self.push_arg(&arg, len, initial);
            }
            self.initial_args = false;
            self.ctl.initial_argc = self.state.cmd_argc;
            self.state.cmd_initial_argv_chars = self.state.cmd_argv_chars;

            while self.read_args() != -1 {
                if self.ctl.lines_per_exec != 0 && self.lineno >= self.ctl.lines_per_exec {
                    self.do_exec();
                    self.lineno = 0;
                }
            }

            // SYSV xargs seems to do at least one exec, even if the input is
            // empty.
            if self.state.cmd_argc != self.ctl.initial_argc
                || (self.always_run_command && !self.procs_executed)
            {
                self.do_exec();
            }
        } else {
            self.ctl.rplen = self.ctl.replace_pat.as_ref().map_or(0, Vec::len);
            let Some((program, rest)) = words.split_first() else {
                return self.child_error;
            };
            loop {
                let args = self.read_args();
                if args == -1 {
                    break;
                }
                let mut len = usize::try_from(args).unwrap_or(0);

                // Don't do insert on the command name.
                self.clear_args();
                self.state.cmd_argv_chars = 0; // begin at start of buffer

                let mut arg = program.clone();
                arg.push(0);
                let arglen = program.len().saturating_add(1);
                let initial = self.initial_args;
                self.push_arg(&arg, arglen, initial);
                len = len.saturating_sub(1);
                self.initial_args = false;

                let item = self.linebuf.get(..len).unwrap_or(&[]).to_vec();
                for word in rest {
                    self.do_insert(word, word.len(), &item, len);
                }
                self.do_exec();
            }
        }

        self.child_error
    }

    /// The `getopt_long` loop, returning upstream's `optind`.
    ///
    /// Our parser *yields* the first operand where C's, with the leading `+`,
    /// returns -1 and leaves `optind` pointing at it. So an operand means
    /// `optind` is one before where the parser now is, and running out means it
    /// is exactly where the parser is.
    #[allow(clippy::too_many_lines)]
    fn parse_options(&mut self, argv: &[OsString]) -> usize {
        let mut parser = XARGS.parse(argv, SHORT_OPTIONS, LONG_OPTIONS);
        let optind;
        loop {
            let Some(item) = parser.next() else {
                optind = parser.optind();
                break;
            };
            let (code, value) = match item {
                Ok(Opt::Short(code, value)) => (code, value),
                Ok(Opt::Long(name, value)) => (long_val(name), value),
                Ok(Opt::Operand(_)) => {
                    optind = parser.optind().saturating_sub(1);
                    break;
                }
                Err(e) => {
                    XARGS.report(&e);
                    let status = u8::try_from(e.status).unwrap_or(XARGS_FAILURE);
                    self.exit_via_atexit(status);
                }
            };
            self.handle_option(code, value.as_ref());
        }
        optind
    }

    /// One arm of upstream's `switch (optc)`.
    #[allow(clippy::too_many_lines)]
    fn handle_option(&mut self, code: u8, value: Option<&OsString>) {
        let optarg = value.map(cstr);
        match code {
            b'0' => {
                self.read_string_mode = true;
                self.input_delimiter = 0;
            }
            b'd' => {
                self.read_string_mode = true;
                let text = optarg.unwrap_or_default();
                self.input_delimiter = self.get_input_delimiter(&text);
            }
            // -E is POSIX, -e deprecated; both take an empty value to mean
            // "there is no logical EOF string".
            b'E' | b'e' => {
                self.eof_str = match optarg {
                    Some(text) if !text.is_empty() => Some(text),
                    _ => None,
                };
            }
            b'h' => {
                let _ = stdfd::write_all(1, help_text().as_bytes());
                self.exit_via_atexit(0);
            }
            // -I is POSIX, -i deprecated. -i excludes -n and -l.
            b'I' | b'i' => {
                self.ctl.replace_pat = Some(match optarg {
                    Some(text) => text,
                    None => b"{}".to_vec(),
                });
                if self.ctl.args_per_exec != 0 {
                    Self::warn_mutually_exclusive("--replace/-I/-i", "--max-args");
                    self.ctl.args_per_exec = 0;
                }
                if self.ctl.lines_per_exec != 0 {
                    Self::warn_mutually_exclusive("--replace/-I/-i", "--max-lines");
                    self.ctl.lines_per_exec = 0;
                }
            }
            // -L excludes -i and -n.
            b'L' => {
                let text = optarg.unwrap_or_default();
                let lines = self.parse_num(&text, b'L', 1, -1, true);
                self.ctl.lines_per_exec = usize::try_from(lines).unwrap_or(0);
                if self.ctl.args_per_exec != 0 {
                    Self::warn_mutually_exclusive("-L", "--max-args");
                    self.ctl.args_per_exec = 0;
                }
                if self.ctl.replace_pat.is_some() {
                    Self::warn_mutually_exclusive("-L", "--replace");
                    self.ctl.replace_pat = None;
                }
            }
            // -l is the deprecated spelling, and its argument is optional.
            b'l' => {
                let lines = match optarg {
                    Some(text) => self.parse_num(&text, b'l', 1, -1, true),
                    None => 1,
                };
                self.ctl.lines_per_exec = usize::try_from(lines).unwrap_or(0);
                if self.ctl.args_per_exec != 0 {
                    Self::warn_mutually_exclusive("--max-lines/-l", "--max-args");
                    self.ctl.args_per_exec = 0;
                }
                if self.ctl.replace_pat.is_some() {
                    Self::warn_mutually_exclusive("--max-lines/-l", "--replace");
                    self.ctl.replace_pat = None;
                }
            }
            // -n excludes -i and -l.
            b'n' => {
                let text = optarg.unwrap_or_default();
                let args = self.parse_num(&text, b'n', 1, -1, true);
                self.ctl.args_per_exec = usize::try_from(args).unwrap_or(0);
                if self.ctl.lines_per_exec != 0 {
                    Self::warn_mutually_exclusive("--max-args/-n", "--max-lines");
                    self.ctl.lines_per_exec = 0;
                }
                if self.ctl.replace_pat.is_some() {
                    if self.ctl.args_per_exec == 1 {
                        // Ignore -n1 in `-i -n1`; see
                        // https://sv.gnu.org/patch/?1500
                        self.ctl.args_per_exec = 0;
                    } else {
                        Self::warn_mutually_exclusive("--max-args/-n", "--replace");
                        self.ctl.replace_pat = None;
                    }
                }
            }
            // POSIX says it is not an error for -s to name a size the
            // implementation cannot support: the relevant limit is used.
            b's' => {
                self.act_on_init_result();
                let text = optarg.unwrap_or_default();
                let ceiling = i64::try_from(self.ctl.posix_arg_size_max).unwrap_or(i64::MAX);
                let size = self.parse_num(&text, b's', 1, ceiling, false);
                // Upstream's follow-up "value too large" warning is dead code:
                // `parse_num` has already clamped to `posix_arg_size_max`.
                self.ctl.arg_max = usize::try_from(size).unwrap_or(0);
            }
            b'S' => self.show_limits = true,
            b't' => self.print_command = true,
            b'x' => self.ctl.exit_if_size_exceeded = true,
            b'o' => self.open_tty = true,
            b'p' => {
                self.query_before_executing = true;
                self.print_command = true;
            }
            b'r' => self.always_run_command = false,
            // Allow only up to MAX_PROC_MAX child processes.
            b'P' => {
                let text = optarg.unwrap_or_default();
                self.proc_max = self.parse_num(&text, b'P', 0, MAX_PROC_MAX, true);
            }
            b'a' => self.input_file = optarg.unwrap_or_default(),
            b'v' => {
                let _ = stdfd::write_all(1, version_text().as_bytes());
                self.exit_via_atexit(0);
            }
            PROCESS_SLOT_VAR => {
                let name = optarg.unwrap_or_default();
                if name.contains(&b'=') {
                    self.die(
                        XARGS_FAILURE,
                        "option --process-slot-var may not be set to a value \
                         which includes `='",
                    );
                }
                // Upstream `unsetenv`s the variable so that no two children can
                // inherit the same value; here each child gets an explicit
                // `env` entry instead, which has the same effect. The one
                // observable part is the size of the environment, which
                // `--show-limits` reports.
                if name.is_empty() {
                    let e = std::io::Error::from_raw_os_error(EINVAL);
                    self.die_errno(XARGS_FAILURE, "failed to unset environment variable ", &e);
                }
                if let Some(value) = std::env::var_os(os_from_bytes(&name)) {
                    let cost = name
                        .len()
                        .saturating_add(os_bytes(&value).len())
                        .saturating_add(2);
                    self.env_size_live = self.env_size_live.saturating_sub(cost);
                }
                self.slot_var_name = Some(name);
            }
            _ => self.usage_failure(),
        }
    }

    /// `--show-limits`.
    ///
    /// Six lines with no `xargs: ` prefix, then — only when standard input is a
    /// terminal — a continuation notice. Note that the fourth line subtracts
    /// the environment size a *second* time: `posix_arg_size_max` already had
    /// it taken off, so the number is deliberately pessimistic.
    fn print_limits(&mut self, words: &[Vec<u8>]) {
        let environment = self.env_size_live;
        Self::warn_raw(&format!(
            "Your environment variables take up {environment} bytes"
        ));
        Self::warn_raw(&format!(
            "POSIX upper limit on argument length (this system): {}",
            self.ctl.posix_arg_size_max
        ));
        Self::warn_raw(&format!(
            "POSIX smallest allowable upper limit on argument length (all \
             systems): {}",
            self.ctl.posix_arg_size_min
        ));
        Self::warn_raw(&format!(
            "Maximum length of command we could actually use: {}",
            self.ctl.posix_arg_size_max.saturating_sub(environment)
        ));
        Self::warn_raw(&format!(
            "Size of command buffer we are actually using: {}",
            self.ctl.arg_max
        ));
        Self::warn_raw(&format!(
            "Maximum parallelism (--max-procs must be no greater): {MAX_PROC_MAX}"
        ));

        if stdfd::is_tty(0) {
            Self::warn_raw(
                "\nExecution of xargs will continue now, and it will try to \
                 read its input and run commands; if this is not what you \
                 wanted to happen, please type the end-of-file keystroke.",
            );
            if self.always_run_command {
                let mut line = b"Warning: ".to_vec();
                line.extend_from_slice(words.first().map_or(&[][..], Vec::as_slice));
                line.extend_from_slice(
                    b" will be run at least once.  If you do not want that to \
                      happen, then press the interrupt keystroke.",
                );
                line.push(b'\n');
                stdfd::diag_bytes(&line);
            }
        }
    }
}

/// `EINVAL`, which `unsetenv ("")` fails with.
const EINVAL: i32 = 22;

/// An option argument as C sees it: a NUL-terminated string, so anything from
/// the first NUL on is invisible. argv words cannot contain NUL on any system
/// we run on, so this is fidelity rather than behaviour.
fn cstr(value: &OsString) -> Vec<u8> {
    let bytes = os_bytes(value);
    match bytes.iter().position(|&b| b == 0) {
        Some(at) => bytes.get(..at).unwrap_or(&[]).to_vec(),
        None => bytes.into_owned(),
    }
}
/// GNU's `usage (EXIT_SUCCESS)` body, verbatim.
///
/// What is *not* here is upstream's closing `explain_how_to_report_bugs`
/// block: it names the GNU project's bug address and manual, which are not
/// ours to point at. `scripts/xargs-diff.sh` records that difference as an
/// expected failure rather than pretending it does not exist.
fn help_text() -> String {
    "\
Usage: xargs [OPTION]... COMMAND [INITIAL-ARGS]...
Run COMMAND with arguments INITIAL-ARGS and more arguments read from input.

Mandatory and optional arguments to long options are also
mandatory or optional for the corresponding short option.
  -0, --null                   items are separated by a null, not whitespace;
                                 disables quote and backslash processing and
                                 logical EOF processing
  -a, --arg-file=FILE          read arguments from FILE, not standard input
  -d, --delimiter=CHARACTER    items in input stream are separated by CHARACTER,
                                 not by whitespace; disables quote and backslash
                                 processing and logical EOF processing
  -E END                       set logical EOF string; if END occurs as a line
                                 of input, the rest of the input is ignored
                                 (ignored if -0 or -d was specified)
  -e, --eof[=END]              equivalent to -E END if END is specified;
                                 otherwise, there is no end-of-file string
  -I R                         same as --replace=R
  -i, --replace[=R]            replace R in INITIAL-ARGS with names read
                                 from standard input, split at newlines;
                                 if R is unspecified, assume {}
  -L, --max-lines=MAX-LINES    use at most MAX-LINES non-blank input lines per
                                 command line
  -l[MAX-LINES]                similar to -L but defaults to at most one non-
                                 blank input line if MAX-LINES is not specified
  -n, --max-args=MAX-ARGS      use at most MAX-ARGS arguments per command line
  -o, --open-tty               Reopen stdin as /dev/tty in the child process
                                 before executing the command; useful to run an
                                 interactive application.
  -P, --max-procs=MAX-PROCS    run at most MAX-PROCS processes at a time
  -p, --interactive            prompt before running commands
      --process-slot-var=VAR   set environment variable VAR in child processes
  -r, --no-run-if-empty        if there are no arguments, then do not run COMMAND;
                                 if this option is not given, COMMAND will be
                                 run at least once
  -s, --max-chars=MAX-CHARS    limit length of command line to MAX-CHARS
      --show-limits            show limits on command-line length
  -t, --verbose                print commands before executing them
  -x, --exit                   exit if the size (see -s) is exceeded
      --help                   display this help and exit
      --version                output version information and exit
"
    .to_string()
}

fn version_text() -> String {
    "xargs (SlateOS coreutils) 0.1.0\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        LONG_OPTIONS, SHORT_OPTIONS, find_sub, help_text, is_blank, is_space, long_val, strtol,
        strtoul, strtoul_base,
    };

    #[test]
    fn blank_is_space_and_tab_only() {
        assert!(is_blank(i32::from(b' ')));
        assert!(is_blank(i32::from(b'\t')));
        // The vertical motions are not blanks, and neither is EOF: `read_line`
        // relies on `ISBLANK (prevc)` being false for its initial `prevc = EOF`.
        assert!(!is_blank(i32::from(b'\n')));
        assert!(!is_blank(0x0b));
        assert!(!is_blank(-1));
        // Not a locale's idea of blank: a non-ASCII byte never qualifies,
        // because upstream guards `isblank` with `isascii`.
        assert!(!is_blank(0xa0));
    }

    #[test]
    fn space_adds_the_vertical_motions() {
        for c in [b' ', b'\t', b'\n', b'\r', 0x0c, 0x0b] {
            assert!(is_space(i32::from(c)), "{c:#04x} should be a space");
        }
        assert!(!is_space(i32::from(b'a')));
        assert!(!is_space(0));
        assert!(!is_space(-1));
    }

    #[test]
    fn strtol_matches_the_c_one() {
        // Leading blanks are skipped, a sign is honoured, and the tail is
        // returned rather than rejected — `parse_num` is what rejects it, and
        // it rejects on a non-empty tail, not on the conversion flag alone.
        assert_eq!(strtol(b"  12abc"), (12, &b"abc"[..], true));
        assert_eq!(strtol(b"-3"), (-3, &b""[..], true));
        assert_eq!(strtol(b"+3"), (3, &b""[..], true));
        // No digits at all: `endptr == str`, so the flag is false and the tail
        // is the whole string. This is the case `parse_num` calls "invalid
        // number" even when the sign was consumed.
        assert_eq!(strtol(b"abc"), (0, &b"abc"[..], false));
        assert_eq!(strtol(b""), (0, &b""[..], false));
        assert_eq!(strtol(b"-"), (0, &b"-"[..], false));
        // Overflow saturates rather than wrapping. `parse_num` never reads
        // errno, so the saturated value is what it range-checks — and for
        // `-n`/`-L` (max -1, i.e. unbounded) it is therefore accepted.
        assert_eq!(strtol(b"99999999999999999999"), (i64::MAX, &b""[..], true));
        assert_eq!(strtol(b"-99999999999999999999"), (i64::MIN, &b""[..], true));
    }

    #[test]
    fn strtoul_consumes_what_it_can() {
        // Base 10 only: this one parses the __GNU_FINDUTILS_EXEC_ARG_*_LIMIT
        // variables, where a leading zero is not an octal prefix.
        assert_eq!(strtoul(b"12"), (12, &b""[..]));
        assert_eq!(strtoul(b"012"), (12, &b""[..]));
        assert_eq!(strtoul(b"  7 "), (7, &b" "[..]));
        // A tail is returned, not rejected; `exceeds` is what insists on an
        // empty one before believing the number.
        assert_eq!(strtoul(b"12x"), (12, &b"x"[..]));
        // No digits: zero and the whole string, so `exceeds` sees a tail and
        // dies rather than silently reading the limit as 0.
        assert_eq!(strtoul(b"x"), (0, &b"x"[..]));
        assert_eq!(strtoul(b""), (0, &b""[..]));
        // Overflow saturates to the flag's ceiling.
        assert_eq!(strtoul(b"999999999999999999999999"), (u128::MAX, &b""[..]));
    }

    #[test]
    fn escape_bases_are_read_the_way_c_reads_them() {
        // `-d '\x41'` is hex and `-d '\101'` is octal, so the same digits mean
        // different bytes. Overflow is reported separately from a large value
        // because `get_char_oct_or_hex_escape` gives the same message for both.
        assert_eq!(strtoul_base(b"41", 16), (0x41, false, &b""[..]));
        assert_eq!(strtoul_base(b"101", 8), (65, false, &b""[..]));
        assert_eq!(strtoul_base(b"ffz", 16), (255, false, &b"z"[..]));
        // 8 and 9 are not octal digits, so `\18` stops at the 1 and the 8 is a
        // trailing character rather than part of the value.
        assert_eq!(strtoul_base(b"18", 8), (1, false, &b"8"[..]));
        let (value, overflowed, tail) = strtoul_base(b"ffffffffffffffffff", 16);
        assert_eq!((value, tail), (u128::from(u64::MAX), &b""[..]));
        assert!(overflowed);
    }

    #[test]
    fn long_options_are_in_gnu_declaration_order() {
        // getopt_long resolves an ambiguous abbreviation by listing candidates
        // in declaration order, so this order is observable: `--m` names
        // --max-lines, --max-args, --max-chars and --max-procs in this
        // sequence and no other.
        let names: Vec<&str> = LONG_OPTIONS.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "null",
                "arg-file",
                "delimiter",
                "eof",
                "replace",
                "max-lines",
                "max-args",
                "open-tty",
                "interactive",
                "no-run-if-empty",
                "max-chars",
                "verbose",
                "show-limits",
                "exit",
                "max-procs",
                "process-slot-var",
                "version",
                "help",
            ]
        );
    }

    #[test]
    fn every_long_option_has_a_switch_label() {
        // `long_val` falls back to 'h', so a name that fell through would
        // silently print the help instead of doing its job.
        for (name, _) in LONG_OPTIONS {
            if *name == "help" {
                continue;
            }
            assert_ne!(long_val(name), b'h', "--{name} has no label of its own");
        }
        // The two that are not the letter you would guess.
        assert_eq!(long_val("replace"), b'I');
        assert_eq!(long_val("max-lines"), b'l');
    }

    #[test]
    fn find_sub_is_a_byte_search() {
        assert_eq!(find_sub(b"a{}b", b"{}"), Some(1));
        assert_eq!(find_sub(b"{}", b"{}"), Some(0));
        assert_eq!(find_sub(b"ab", b"{}"), None);
        // Not a string search: an embedded NUL is an ordinary byte on both
        // sides, because `-I` may name one and the item may contain one.
        assert_eq!(find_sub(b"a\0{}", b"{}"), Some(2));
        assert_eq!(find_sub(b"a\0b", b"\0b"), Some(1));
        // The empty needle matches at 0, as `strstr` does; `do_insert` is what
        // guards against looping on it.
        assert_eq!(find_sub(b"ab", b""), Some(0));
    }

    #[test]
    fn short_options_keep_the_leading_plus() {
        // The `+` is what stops option parsing at the command word, so
        // `xargs echo -n` passes `-n` to echo rather than taking it as ours.
        assert!(SHORT_OPTIONS.starts_with('+'));
        // -S, -h and -v are long-only; a short spelling would shadow a word
        // the command is entitled to receive.
        for absent in ['S', 'h', 'v'] {
            assert!(
                !SHORT_OPTIONS.contains(absent),
                "-{absent} should have no short form"
            );
        }
    }

    #[test]
    fn help_text_ends_where_the_gnu_referral_begins() {
        let text = help_text();
        assert!(text.starts_with("Usage: xargs [OPTION]... COMMAND [INITIAL-ARGS]...\n"));
        assert!(
            text.ends_with("      --version                output version information and exit\n")
        );
        // Upstream's closing block names an upstream this is not.
        assert!(!text.contains("bug-findutils"));
        assert!(!text.contains("GNU"));
    }
}
