//! Tree-walking interpreter for the OSH shell.
//!
//! Executes a parsed [`Program`]: variable/parameter expansion, builtins,
//! external command execution (real fork/exec via [`std::process::Command`]),
//! pipelines, redirections, command substitution, arithmetic, and control
//! flow (`if`/`while`/`until`/`for`/`case`, functions, `&&`/`||`, `;`),
//! here-documents (`<<`, `<<-`), here-strings (`<<<`), `[[ … ]]` conditional
//! expressions, and `(( … ))` arithmetic commands.
//!
//! Pathname (glob) expansion (`*.txt`, `src/*.rs`, `[abc]?.log`) applies to
//! command arguments, honoring quoting and the leading-dot rule; an unmatched
//! pattern is left literal (bash default, no `nullglob`).
//!
//! Indexed arrays are supported: `a=(x y z)`, `a[i]=v`, `a+=(w)`, `${a[i]}`,
//! `${a[@]}`/`${a[*]}`, `${#a[@]}`, `${!a[@]}` (indices), and `unset a[i]`.
//! Array literals may be keyed/sparse (`a=([2]=x y)`). `"${a[@]}"` preserves
//! element boundaries (one field per element).
//!
//! Associative arrays: `declare -A m` (also `typeset`/`local`) then `m[key]=v`,
//! `m=([k1]=v1 [k2]=v2)`, or the combined one-liner `declare -A m=([k]=v)`;
//! `${m[key]}`, `${m[@]}`/`${m[*]}` (values, insertion order), `${!m[@]}`
//! (keys), `${#m[@]}`, and `unset m[key]`. Subscripts on an associative array
//! are string keys (expanded, not arithmetic). The one-liner works for indexed
//! arrays too (`declare -a a=(x y)` / the flagless `declare a=(x y)`).
//!
//! ## Known limitations (tracked for the grow phase — see the crate docs and
//! `design-decisions.md §72`):
//! - `${a[-1]}` negative subscripts count from the end (bash semantics; a
//!   scalar acts as a one-element array). Array elements are addressable inside
//!   arithmetic (`$(( a[i] + 1 ))`, `(( a[-1] ))`); the subscript is itself an
//!   arithmetic expression. A subscript may be combined with a parameter
//!   operator (`${a[i]:-def}`, `${a[i]#pat}`, `${a[i]:off:len}`,
//!   `${a[i]/pat/repl}`, and `${a[i]:=v}` which writes the element back);
//!   associative subscripts use the string key. Combining `[@]`/`[*]` with an
//!   operator (a bulk element transform) is still rejected at parse time.
//!   Indexed arrays are sparse (an ordered `index → value` map): a sparse
//!   literal (`a=([5]=x)`) stores a single element, `${#a[@]}` counts only
//!   assigned elements, `${!a[@]}` lists only the assigned indices, `unset
//!   a[i]` leaves a gap (no shift), and a negative subscript counts back from
//!   `highest_index + 1`.
//! - `[[ … ]]` supports `=~` (POSIX-ERE regex match) via the in-tree linear-time
//!   Pike-VM engine in [`crate::ere`] (ReDoS-safe — no catastrophic backtracking).
//!   The RHS undergoes parameter expansion; on a successful match `BASH_REMATCH`
//!   is populated (`[0]` = whole match, `[i]` = capture group `i`). The lexer
//!   reads the `=~` RHS as one regex word so `(`, `)`, `|`, `<`, `>` are literal
//!   metacharacters. The RHS is quote-aware (`regex_pattern_from_rhs`): quoted
//!   spans (`"a.b"`, `'a.b'`, `"$p"`) match literally — their metacharacters are
//!   escaped — while unquoted spans (`a.b`, `$p`) are live regex, per bash. The
//!   `-r`/`-x` file tests are approximated as "exists" pending the slateos
//!   permission model.
//! - Pipelines run *concurrently* and stream. An all-external pipeline (every
//!   stage a plain external command, no per-stage redirects) is wired with real
//!   OS pipes between child processes. Any pipeline containing a builtin,
//!   function, or compound stage — or a stage with its own redirect — uses the
//!   *threaded* path: each stage runs in its own subshell on its own thread,
//!   connected by real OS pipes, so data flows as it is produced rather than
//!   being buffered whole. A stage's own redirect composes with the inter-stage
//!   pipe: `run_external`/`run_builtin` resolve the stage's `RedirPlan` against
//!   the pipe endpoints, so `a | b > f` diverts `b`'s stdout to the file while
//!   its stdin still streams from `a` (and likewise for `2>err`). Every stage is
//!   a subshell (bash semantics, no lastpipe), so a stage's variable mutations
//!   do not leak to the parent. Downstream early-exit propagates upstream: when a
//!   consumer stops, an in-process producer's next write hits the `pipe_broken`
//!   flag (the `SIGPIPE`/`EPIPE` analogue) and unwinds, and an external producer
//!   terminates on the OS's broken-pipe signal (`yes | head` exits) on targets
//!   that deliver it — the slateos target does; see the note in the pipeline
//!   tests about the Windows test host. Both paths publish every stage's exit
//!   code in `${PIPESTATUS[@]}` (in pipeline order) and honour `set -o pipefail`
//!   (`$?` becomes the rightmost non-zero stage's status; `set +o pipefail`
//!   restores last-stage semantics).
//! - Compound commands accept trailing redirections
//!   (`while read …; do …; done < file`, `for … done > out`, `{ …; } >> log`).
//!   Input is fed through a shared cursor so successive `read`s in the body
//!   consume successive lines; captured stdout is written to the target file.
//!   A compound command's *stderr* is also redirectable (`{ …; } 2> err`,
//!   `for … done 2>&1`) via a `stderr_stack` consulted by every fd-2 write;
//!   `2>&1` into a captured stdout folds stderr in after the body (not
//!   byte-interleaved).
//! - Background (`&`) runs a single external command asynchronously; compound
//!   background jobs run synchronously.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::process::{Child, ChildStdout, Command as PCommand, Stdio};
use std::sync::{Arc, Mutex};

use crate::arith::{self, VarLookup};
use crate::ast::{
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, CaseClause, CaseTerm, Command,
    CondBinOp,
    CondExpr,
    ForArithClause, ForClause, IfClause, LoopClause, ParamOp, Pipeline, Program, Redirect,
    RedirectOp,
    ReplaceAnchor, SelectClause, SimpleCommand, UnaryOp, Word, WordPart,
};
use crate::parser::parse;

/// Non-local control flow produced while executing statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Continue with the next statement.
    Next,
    /// `break N` — unwind N enclosing loops.
    Break(u32),
    /// `continue N` — restart the Nth enclosing loop.
    Continue(u32),
    /// `return` from a function/script.
    Return,
    /// `exit N` — terminate the shell.
    Exit(i32),
}

/// Where a command's standard output should go.
enum Out<'a> {
    /// Inherit the shell's real stdout.
    Inherit,
    /// Append to a capture buffer (command substitution / pipeline stage).
    Capture(&'a mut Vec<u8>),
    /// Stream to the write end of an OS pipe. Used by a *concurrent* pipeline
    /// stage that runs an in-process builtin/compound command: bytes flow to the
    /// next stage as they are produced (not buffered), and a write that fails
    /// with `BrokenPipe` (the reader closed early, e.g. `… | head`) signals the
    /// stage to abort — the in-process analogue of `SIGPIPE`.
    Pipe(io::PipeWriter),
}

/// A command's standard input source.
enum StdinSrc<'a> {
    /// Inherit the shell's real stdin.
    Inherit,
    /// Read from a shared, position-tracking byte cursor. Used for pipeline
    /// stage input and compound-command `< file` / here-doc redirects so that
    /// repeated `read` calls (e.g. `while read …; done < file`) consume
    /// successive lines rather than restarting from the beginning.
    Cursor(&'a RefCell<io::Cursor<Vec<u8>>>),
    /// Read from the read end of an OS pipe fed by a concurrent upstream stage.
    /// Wrapped in a `BufReader`/`RefCell` so line-oriented `read` builtins can
    /// consume successive lines from the stream (interior mutability behind the
    /// `&StdinSrc` shared borrow, matching [`StdinSrc::Cursor`]).
    Pipe(RefCell<io::BufReader<io::PipeReader>>),
}

/// Where a command's *stderr* (fd 2) is currently directed. Pushed onto
/// [`Shell::stderr_stack`] while executing the body of a compound command that
/// carries a stderr redirect (`{ …; } 2> err`, `for … done 2>&1`). An empty
/// stack means fd 2 is the shell's real stderr (the default).
///
/// All handles are `Arc`-based so the enclosing [`Shell`] stays `Send` — a
/// pipeline stage's subshell clone is moved into a scoped thread. (Clones reset
/// the stack to empty via [`Shell::clone_for_subshell`], so the `Arc` contents
/// never actually cross a thread boundary, but the type must still be `Send`.)
enum StderrTarget {
    /// `2> file` / `2>> file` — write to this already-opened file (shared by all
    /// commands in the group via `try_clone`).
    File(Arc<File>),
    /// `2>&1` where stdout is a downstream pipe — merge into the same pipe so
    /// stdout and stderr interleave at the reader (bash `… 2>&1 | next`).
    Pipe(Arc<io::PipeWriter>),
    /// `2>&1`/stderr capture into a buffer — used when the surrounding stdout is
    /// itself captured (command substitution `$( … 2>&1 )`). The buffer is
    /// merged into the stdout capture once the body finishes (line-level
    /// interleaving with stdout is not preserved — see the module limitations).
    Buffer(Arc<Mutex<Vec<u8>>>),
    /// `2>&1` where stdout is the shell's real stdout — write to fd 1.
    Stdout,
}

/// A saved snapshot of one variable's complete state (scalar, indexed array,
/// associative array, export flag), captured when `local` shadows the name
/// inside a function so it can be restored when the function returns.
struct VarSnapshot {
    scalar: Option<String>,
    indexed: Option<BTreeMap<usize, String>>,
    assoc: Option<Vec<(String, String)>>,
    exported: bool,
}

/// A background job started with `&`. Tracks the spawned child so `wait`/`jobs`
/// can reap and report it. `child` becomes `None` once the process has been
/// reaped (its final status is kept in `status`).
struct Job {
    /// Job number as shown by `jobs` and referenced by `%n`.
    id: usize,
    /// OS process id (also reported by `$!` at spawn time).
    pid: u32,
    /// The live child handle, or `None` after the process has been reaped.
    child: Option<std::process::Child>,
    /// The command line, for `jobs` display.
    cmd: String,
    /// Final exit status once the process has finished and been reaped.
    status: Option<i32>,
}

/// The shell interpreter and its mutable session state.
pub struct Shell {
    vars: HashMap<String, String>,
    /// Indexed arrays: `name=(a b c)` and `name[i]=v`. Kept separate from
    /// `vars`; `${name}` reads element 0, `${name[@]}`/`${name[*]}` read all.
    /// Sparse by construction: an ordered `index → value` map, so `a=([5]=x)`
    /// stores a single entry at 5 (no gap-filling) and `${!a[@]}` lists only the
    /// indices actually assigned. `BTreeMap` keeps iteration in ascending-index
    /// order, matching bash's `${a[@]}`/`${!a[@]}` traversal.
    arrays: HashMap<String, BTreeMap<usize, String>>,
    /// Associative arrays (`declare -A m; m[key]=v`). Insertion-ordered
    /// key/value pairs for deterministic iteration. A name present here is
    /// associative: subscripts are string keys, not arithmetic indices.
    assoc: HashMap<String, Vec<(String, String)>>,
    exported: HashSet<String>,
    funcs: HashMap<String, Program>,
    positional: Vec<String>,
    name: String,
    last_status: i32,
    last_bg_pid: Option<u32>,
    /// `set -o pipefail`: a pipeline's status is the rightmost non-zero stage.
    pipefail: bool,
    /// Set when a write to a pipeline stage's downstream pipe fails with
    /// `BrokenPipe` (the reader closed early). The statement loops check it and
    /// unwind the stage — the in-process analogue of a producer taking `SIGPIPE`.
    /// Only ever set on a per-stage subshell clone, never the top-level shell.
    pipe_broken: bool,
    pid: u32,
    /// Active stderr (fd 2) redirections, innermost last. Empty = real stderr.
    /// Pushed/popped by [`Shell::exec_redirected`] around a compound command's
    /// body so its stderr redirect (`{ …; } 2> err`) covers every command in
    /// the group. Consulted by [`Shell::emit_stderr`] (diagnostics/`>&2`) and
    /// [`Shell::run_external`] (child fd 2). Reset to empty in subshell clones.
    stderr_stack: Vec<StderrTarget>,
    /// `getopts` cursor within the current argument (0 = at the start of a new
    /// argument, i.e. examine the leading `-`). Tracks position inside a bundled
    /// flag group like `-abc` across successive `getopts` calls.
    getopts_col: usize,
    /// The value of `OPTIND` `getopts` last saw, so an external reset
    /// (`OPTIND=1`) is detected and the intra-argument cursor is cleared.
    getopts_optind: usize,
    /// Anchor instant for `$SECONDS` (reset when `SECONDS` is assigned).
    seconds_anchor: std::time::Instant,
    /// Base value added to elapsed seconds for `$SECONDS` (set by assignment).
    seconds_base: u64,
    /// State for the `$RANDOM` pseudo-random generator. `Cell` so a read
    /// (`param_value(&self)`) can advance it; assigning `RANDOM=n` reseeds it.
    rng: std::cell::Cell<u32>,
    /// `set -e` (errexit): exit the shell when a command fails, except in the
    /// exempt positions (conditions, non-final `&&`/`||` operands, negated
    /// pipelines) tracked by [`Shell::errexit_suppress`].
    errexit: bool,
    /// `set -u` (nounset): expanding an unset variable is an error that aborts.
    nounset: bool,
    /// `set -x` (xtrace): print each simple command (prefixed `+ `) to stderr
    /// before executing it.
    xtrace: bool,
    /// Nesting depth of errexit-exempt contexts (if/while/until conditions and
    /// negated commands). While `> 0`, a failing command does not trigger
    /// errexit. Incremented around condition evaluation; reset in subshells.
    errexit_suppress: u32,
    /// Set by expansion when `nounset` is on and an unset variable is referenced;
    /// the simple-command driver checks and aborts (`Flow::Exit(1)`) after
    /// expanding its words.
    unbound_error: bool,
    /// Stack of function-local variable scopes. Each frame records the variables
    /// shadowed by `local` in that function call and their prior state, restored
    /// on return. Non-empty exactly while executing a function body.
    local_frames: Vec<Vec<(String, VarSnapshot)>>,
    /// Names marked `readonly` (or `declare -r`). Assigning to or unsetting a
    /// readonly variable is an error; the shell reports it and leaves the value
    /// unchanged. Copied into subshell clones so the attribute is inherited.
    readonly: HashSet<String>,
    /// `shopt` option toggles (e.g. `nullglob`, `dotglob`, `nocaseglob`). Only
    /// options present with `true` are enabled; absent/`false` = default off.
    /// Inherited by subshell clones.
    shopt: HashMap<String, bool>,
    /// Names with the integer attribute (`declare -i`). Assignments to these are
    /// evaluated as arithmetic before storing (`x=5+3` stores `8`, `x+=2` adds).
    /// Inherited by subshell clones.
    integer_attr: HashSet<String>,
    /// Names with the lowercase attribute (`declare -l`). Assigned values are
    /// converted to lowercase before storing. Inherited by subshell clones.
    lower_attr: HashSet<String>,
    /// Names with the uppercase attribute (`declare -u`). Assigned values are
    /// converted to uppercase before storing. Inherited by subshell clones.
    upper_attr: HashSet<String>,
    /// The directory stack below the current directory, managed by
    /// `pushd`/`popd`/`dirs`. Element 0 is the directory `popd` would return to;
    /// the *current* directory (the process cwd) is conceptually the top of the
    /// stack and is not stored here. Cloned into subshells.
    dir_stack: Vec<String>,
    /// Signal/pseudo-signal traps set by the `trap` builtin, keyed by the
    /// normalized spec (`EXIT`, `ERR`, `INT`, …). The value is the action
    /// command string; an empty string means "ignore". Currently only the
    /// `EXIT` trap is actually fired (on top-level shell exit); other specs are
    /// stored/printed faithfully but async delivery awaits kernel signal
    /// support (see known-issues TD-OILS11). NOT cloned into subshells (bash
    /// resets non-ignored traps to their default in a subshell).
    traps: HashMap<String, String>,
    /// Guards the `EXIT` trap so it fires at most once when the shell exits.
    exit_trap_done: bool,
    /// True while a trap handler (`ERR`/`DEBUG`/`RETURN`) is running, to prevent
    /// a handler's own commands from recursively re-triggering the same trap.
    in_trap: bool,
    /// Background jobs started with `&`, tracked so `jobs`/`wait` can report and
    /// reap them. NOT inherited by subshell clones (a subshell has no jobs).
    /// Each new job takes the lowest unused job number (bash semantics), so the
    /// numbering restarts at 1 once the table drains.
    jobs: Vec<Job>,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    /// Create a fresh shell with `$0` defaulting to `osh`.
    #[must_use]
    pub fn new() -> Self {
        Shell {
            vars: HashMap::new(),
            arrays: HashMap::new(),
            assoc: HashMap::new(),
            exported: HashSet::new(),
            funcs: HashMap::new(),
            positional: Vec::new(),
            name: "osh".to_string(),
            last_status: 0,
            last_bg_pid: None,
            pipefail: false,
            pipe_broken: false,
            pid: std::process::id(),
            stderr_stack: Vec::new(),
            getopts_col: 0,
            getopts_optind: 1,
            seconds_anchor: std::time::Instant::now(),
            seconds_base: 0,
            // Seed `$RANDOM` from the wall clock so successive runs differ.
            rng: std::cell::Cell::new(initial_rng_seed()),
            errexit: false,
            nounset: false,
            xtrace: false,
            errexit_suppress: 0,
            unbound_error: false,
            local_frames: Vec::new(),
            readonly: HashSet::new(),
            shopt: HashMap::new(),
            integer_attr: HashSet::new(),
            lower_attr: HashSet::new(),
            upper_attr: HashSet::new(),
            dir_stack: Vec::new(),
            traps: HashMap::new(),
            exit_trap_done: false,
            in_trap: false,
            jobs: Vec::new(),
        }
    }

    /// Set `$0`, the shell/script name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Set the positional parameters (`$1`, `$2`, …).
    pub fn set_positional(&mut self, args: Vec<String>) {
        self.positional = args;
    }

    /// Set a shell variable.
    pub fn set_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(name.into(), value.into());
    }

    /// The exit status of the most recently completed command.
    #[must_use]
    pub fn last_status(&self) -> i32 {
        self.last_status
    }

    /// Parse and execute shell source, returning the final exit status.
    pub fn run_source(&mut self, src: &str) -> i32 {
        let prog = match parse(src) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("osh: syntax error: {e}");
                self.last_status = 2;
                return 2;
            }
        };
        let mut out = Out::Inherit;
        match self.exec_program(&prog, &mut out, &StdinSrc::Inherit) {
            Flow::Exit(code) => {
                self.last_status = code;
                code
            }
            _ => self.last_status,
        }
    }

    fn exec_program(&mut self, prog: &Program, out: &mut Out, stdin: &StdinSrc) -> Flow {
        for item in &prog.items {
            if item.background {
                // Only a single external simple command is truly backgrounded;
                // everything else runs synchronously (documented limitation).
                self.exec_background(&item.list);
                continue;
            }
            let flow = self.exec_and_or(&item.list, out, stdin);
            match flow {
                Flow::Next => {}
                other => return other,
            }
            // A downstream pipe closed mid-stage (e.g. `… | head`): unwind the
            // whole stage like a producer taking SIGPIPE. Modelled as an exit so
            // enclosing loops/compounds stop; only ever set on a stage subshell.
            if self.pipe_broken {
                return Flow::Exit(141);
            }
        }
        Flow::Next
    }

    fn exec_and_or(&mut self, ao: &AndOr, out: &mut Out, stdin: &StdinSrc) -> Flow {
        let n_rest = ao.rest.len();
        let flow = self.exec_pipeline(&ao.first, out, stdin);
        if !matches!(flow, Flow::Next) {
            return flow;
        }
        // Track whether the *final* structural element of the list executed. Per
        // POSIX, `set -e` ignores the failure of any command in an AND-OR list
        // other than the one following the final `&&`/`||`.
        let mut ran_final = n_rest == 0;
        for (idx, (op, pipe)) in ao.rest.iter().enumerate() {
            let run = match op {
                AndOrOp::And => self.last_status == 0,
                AndOrOp::Or => self.last_status != 0,
            };
            if run {
                let flow = self.exec_pipeline(pipe, out, stdin);
                if !matches!(flow, Flow::Next) {
                    return flow;
                }
                ran_final = idx + 1 == n_rest;
            } else {
                ran_final = false;
            }
        }
        // errexit / ERR trap: both trigger when the final command executed failed
        // and we are not in an exempt context (condition/negation) or a negated
        // final pipeline (whose status inversion already exempts it).
        let final_pipe = ao.rest.last().map_or(&ao.first, |(_, p)| p);
        let failed_unexempt = self.errexit_suppress == 0
            && ran_final
            && !final_pipe.negated
            && self.last_status != 0;
        if failed_unexempt {
            // The ERR trap fires regardless of whether `set -e` is on.
            self.fire_trap("ERR");
        }
        if self.errexit && failed_unexempt {
            return Flow::Exit(self.last_status);
        }
        Flow::Next
    }

    /// Execute a `Program` used as a condition (if/while/until test), with
    /// errexit suppressed for its duration so a failing test does not exit the
    /// shell under `set -e`.
    fn exec_condition(&mut self, p: &Program, out: &mut Out, stdin: &StdinSrc) -> Flow {
        self.errexit_suppress += 1;
        let flow = self.exec_program(p, out, stdin);
        self.errexit_suppress = self.errexit_suppress.saturating_sub(1);
        flow
    }

    fn exec_pipeline(&mut self, pipe: &Pipeline, out: &mut Out, stdin: &StdinSrc) -> Flow {
        let start = if pipe.timed {
            Some(std::time::Instant::now())
        } else {
            None
        };
        let (statuses, flow) = if pipe.commands.len() == 1 {
            let flow = self.exec_command(&pipe.commands[0], out, stdin);
            (vec![self.last_status], flow)
        } else if pipe.commands.iter().all(|c| self.stage_is_plain_external(c)) {
            // All-external pipeline → real OS pipes (concurrent, SIGPIPE-aware).
            self.exec_concurrent_pipeline(&pipe.commands, out)
        } else {
            // A builtin/function/compound stage is present → threaded pipeline
            // (each in-process stage on its own thread, real OS pipes between).
            self.exec_threaded_pipeline(&pipe.commands, out)
        };
        // Publish `${PIPESTATUS[@]}` and fold per-stage statuses into `$?`.
        self.finish_pipeline(&statuses);
        if pipe.negated {
            self.last_status = i32::from(self.last_status == 0);
        }
        if let Some(start) = start {
            let real = start.elapsed().as_secs_f64();
            self.emit_stderr(Self::format_time_report(real, pipe.time_posix).as_bytes());
        }
        flow
    }

    /// Render a `time`/`time -p` report. `real` is wall-clock seconds; user and
    /// system CPU times are reported as zero because the host does not expose
    /// per-child CPU accounting through `std::process` (see known-issues
    /// TD-OILS10). The default (bash) form is `\nreal\tNmS.SSSs\n…`; the POSIX
    /// `-p` form is `real S.SS\n…` with two decimals and no leading newline.
    fn format_time_report(real: f64, posix: bool) -> String {
        if posix {
            format!("real {real:.2}\nuser {:.2}\nsys {:.2}\n", 0.0, 0.0)
        } else {
            let fmt = |s: f64| {
                let mins = (s / 60.0).floor() as u64;
                let secs = s - (mins as f64) * 60.0;
                format!("{mins}m{secs:.3}s")
            };
            format!(
                "\nreal\t{}\nuser\t{}\nsys\t{}\n",
                fmt(real),
                fmt(0.0),
                fmt(0.0)
            )
        }
    }

    /// Store the per-stage exit codes in `${PIPESTATUS[@]}` and set `$?`.
    ///
    /// Without `pipefail`, `$?` is the last stage's status (POSIX). With
    /// `set -o pipefail`, it is the rightmost non-zero stage (bash semantics),
    /// or `0` when every stage succeeded.
    fn finish_pipeline(&mut self, statuses: &[i32]) {
        self.arrays.insert(
            "PIPESTATUS".to_string(),
            statuses
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.to_string()))
                .collect(),
        );
        self.last_status = if self.pipefail {
            statuses.iter().rev().copied().find(|&s| s != 0).unwrap_or(0)
        } else {
            statuses.last().copied().unwrap_or(0)
        };
    }

    /// Run a multi-stage pipeline that contains at least one in-process stage
    /// (builtin, shell function, or compound command), connecting the stages
    /// with real OS pipes so they run **concurrently** and stream. Each stage
    /// executes in its own subshell clone — matching bash's rule that every
    /// pipeline stage runs in a subshell, so a stage's variable/`cd`/function
    /// mutations do not leak into the parent shell. Because the stages stream,
    /// an unbounded producer terminates early when a downstream stage closes
    /// its input (`SIGPIPE` for an external producer; the [`Shell::pipe_broken`]
    /// flag unwinds an in-process producer). Returns the per-stage exit codes
    /// (in pipeline order) for `${PIPESTATUS[@]}` / `pipefail`.
    fn exec_threaded_pipeline(&mut self, cmds: &[Command], out: &mut Out) -> (Vec<i32>, Flow) {
        let n = cmds.len();
        // Build the n-1 connecting pipes up front. `readers[i]`/`writers[i]` are
        // stage i's input/output endpoints; stage 0 inherits stdin and the last
        // stage writes to the ambient `out`, so those endpoints are `None`.
        let mut readers: Vec<Option<io::PipeReader>> = Vec::with_capacity(n);
        let mut writers: Vec<Option<io::PipeWriter>> = Vec::with_capacity(n);
        readers.push(None); // stage 0 reads the pipeline's own stdin
        for _ in 0..n - 1 {
            match io::pipe() {
                Ok((r, w)) => {
                    writers.push(Some(w)); // stage k writes here
                    readers.push(Some(r)); // stage k+1 reads here
                }
                Err(e) => {
                    eprintln!("osh: pipe: {e}");
                    self.last_status = 1;
                    return (vec![1; n], Flow::Next);
                }
            }
        }
        writers.push(None); // last stage writes to `out`

        let mut statuses = vec![0i32; n];

        // Scoped threads let each stage borrow the shared AST (`cmds`) while
        // owning its subshell clone and pipe endpoints (all `Send`). `out` is
        // used only on this thread by the last stage.
        std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(n.saturating_sub(1));
            for i in 0..n - 1 {
                let mut sub = self.clone_for_subshell();
                let cmd = &cmds[i];
                let reader = readers[i].take();
                let Some(writer) = writers[i].take() else {
                    continue; // unreachable for non-last stages
                };
                let handle = scope.spawn(move || {
                    let stdin = match reader {
                        Some(r) => StdinSrc::Pipe(RefCell::new(io::BufReader::new(r))),
                        None => StdinSrc::Inherit,
                    };
                    let mut o = Out::Pipe(writer);
                    sub.exec_command(cmd, &mut o, &stdin);
                    // `o` drops here, closing the write end → EOF downstream.
                    sub.last_status
                });
                handles.push((i, handle));
            }

            // Last stage: run on this thread (writing to `out`) in a clone.
            let last = n - 1;
            let mut sub = self.clone_for_subshell();
            let reader = readers[last].take();
            let stdin = match reader {
                Some(r) => StdinSrc::Pipe(RefCell::new(io::BufReader::new(r))),
                None => StdinSrc::Inherit,
            };
            sub.exec_command(&cmds[last], out, &stdin);
            statuses[last] = sub.last_status;
            // Close this stage's read end NOW (before joining) so an upstream
            // producer that outlives the consumer sees EOF/EPIPE and stops —
            // otherwise the still-open reader would deadlock the join.
            drop(stdin);

            // Join the workers (scope also joins, but we need their statuses).
            for (i, handle) in handles {
                statuses[i] = handle.join().unwrap_or(1);
            }
        });

        // A pipeline is a single command; `exit`/`return`/`break` inside a stage
        // affect only that stage's subshell and never escape (bash semantics).
        (statuses, Flow::Next)
    }

    /// A pipeline stage qualifies for the real-pipe (concurrent) path only if it
    /// is structurally a plain external command: a simple command with at least
    /// one word, a *literal* command word that is neither a builtin nor a shell
    /// function, and no per-stage redirections. The check is purely syntactic
    /// (no expansion) so it has no side effects and never double-runs a command
    /// substitution — anything it can't prove external falls back to buffering.
    fn stage_is_plain_external(&self, cmd: &Command) -> bool {
        let Command::Simple(sc) = cmd else {
            return false;
        };
        if !sc.redirects.is_empty() {
            return false;
        }
        let Some(first) = sc.words.first() else {
            return false; // pure assignment (no command word)
        };
        let [WordPart::Literal(name)] = first.parts.as_slice() else {
            return false; // command word uses expansion — resolve via buffered path
        };
        !is_builtin(name) && !self.funcs.contains_key(name)
    }

    /// Run an all-external pipeline with real OS pipes so the stages execute
    /// concurrently. Returns the per-stage exit codes (in pipeline order) so the
    /// caller can publish `${PIPESTATUS[@]}` and apply `pipefail`. The caller
    /// guarantees every stage passes [`Shell::stage_is_plain_external`].
    fn exec_concurrent_pipeline(&mut self, cmds: &[Command], out: &mut Out) -> (Vec<i32>, Flow) {
        let capturing = matches!(out, Out::Capture(_));
        let last = cmds.len().saturating_sub(1);
        let mut children: Vec<Child> = Vec::with_capacity(cmds.len());
        let mut prev_stdout: Option<ChildStdout> = None;
        // Per-stage exit code, indexed by pipeline position. Stages that expand
        // to nothing default to 0 (an empty command succeeds).
        let mut stage_status: Vec<i32> = vec![0; cmds.len()];
        // Pipeline position of each spawned child, parallel to `children`.
        let mut child_cmd_idx: Vec<usize> = Vec::with_capacity(cmds.len());

        for (i, cmd) in cmds.iter().enumerate() {
            let Command::Simple(sc) = cmd else {
                continue; // guaranteed Simple by the classifier
            };
            let mut argv: Vec<String> = Vec::new();
            for w in &sc.words {
                argv.extend(self.expand_word(w, true));
            }
            let assigns: Vec<(String, String)> = sc
                .assignments
                .iter()
                .map(|a| self.assignment_prefix_value(a))
                .collect();
            let Some(program) = argv.first() else {
                // Expanded to nothing (e.g. `$empty`) — skip this stage; its
                // successor sees EOF on stdin.
                prev_stdout = None;
                continue;
            };

            let mut pc = PCommand::new(program);
            pc.args(&argv[1..]);
            for (k, v) in &self.vars {
                if self.exported.contains(k) {
                    pc.env(k, v);
                }
            }
            for (k, v) in &assigns {
                pc.env(k, v);
            }

            // stdin: first stage inherits; later stages read the previous pipe
            // (or a closed/null stream if the previous stage failed to start).
            if i == 0 {
                pc.stdin(Stdio::inherit());
            } else if let Some(so) = prev_stdout.take() {
                pc.stdin(Stdio::from(so));
            } else {
                pc.stdin(Stdio::null());
            }

            // stdout: last stage → capture or inherit; earlier stages → a pipe.
            if i == last {
                if capturing {
                    pc.stdout(Stdio::piped());
                } else {
                    pc.stdout(Stdio::inherit());
                }
            } else {
                pc.stdout(Stdio::piped());
            }

            match pc.spawn() {
                Ok(mut child) => {
                    if i != last {
                        prev_stdout = child.stdout.take();
                    }
                    child_cmd_idx.push(i);
                    children.push(child);
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::NotFound {
                        eprintln!("osh: {program}: command not found");
                    } else {
                        eprintln!("osh: {program}: {e}");
                    }
                    prev_stdout = None;
                    // A stage that fails to spawn reports 127 (not found) / 126.
                    stage_status[i] =
                        if e.kind() == io::ErrorKind::NotFound { 127 } else { 126 };
                }
            }
        }

        // Read the final stage's output into the capture buffer before waiting,
        // so the producer isn't blocked on a full pipe (avoids deadlock).
        if capturing
            && let Some(mut so) = children.last_mut().and_then(|c| c.stdout.take())
        {
            let mut buf = Vec::new();
            let _ = so.read_to_end(&mut buf);
            if let Out::Capture(b) = out {
                b.extend_from_slice(&buf);
            }
        }

        // Wait for every child and record its exit code at its pipeline position.
        for (pos, mut child) in children.into_iter().enumerate() {
            let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
            if let Some(&cmd_i) = child_cmd_idx.get(pos) {
                stage_status[cmd_i] = code;
            }
        }
        (stage_status, Flow::Next)
    }

    fn exec_command(&mut self, cmd: &Command, out: &mut Out, stdin: &StdinSrc) -> Flow {
        match cmd {
            Command::Simple(sc) => self.exec_simple(sc, out, stdin),
            Command::If(c) => self.exec_if(c, out, stdin),
            Command::Loop(c) => self.exec_loop(c, out, stdin),
            Command::For(c) => self.exec_for(c, out, stdin),
            Command::ForArith(c) => self.exec_for_arith(c, out, stdin),
            Command::Select(c) => self.exec_select(c, out, stdin),
            Command::Function(f) => {
                self.funcs.insert(f.name.clone(), f.body.clone());
                self.last_status = 0;
                Flow::Next
            }
            Command::Case(c) => self.exec_case(c, out, stdin),
            Command::Cond(e) => self.exec_cond(e),
            Command::Arith(raw) => self.exec_arith(raw),
            Command::BraceGroup(p) => self.exec_program(p, out, stdin),
            Command::Redirected { inner, redirects } => {
                self.exec_redirected(inner, redirects, out, stdin)
            }
            Command::Subshell(p) => {
                // A subshell gets a clone of the state; mutations don't escape.
                let mut sub = self.clone_for_subshell();
                let flow = sub.exec_program(p, out, stdin);
                self.last_status = sub.last_status;
                // Propagate an explicit exit from the subshell as a status only.
                match flow {
                    Flow::Exit(c) => {
                        self.last_status = c;
                        Flow::Next
                    }
                    _ => Flow::Next,
                }
            }
        }
    }

    fn exec_if(&mut self, c: &IfClause, out: &mut Out, stdin: &StdinSrc) -> Flow {
        let flow = self.exec_condition(&c.cond, out, stdin);
        if !matches!(flow, Flow::Next) {
            return flow;
        }
        if self.last_status == 0 {
            return self.exec_program(&c.body, out, stdin);
        }
        for (cond, body) in &c.elifs {
            let flow = self.exec_condition(cond, out, stdin);
            if !matches!(flow, Flow::Next) {
                return flow;
            }
            if self.last_status == 0 {
                return self.exec_program(body, out, stdin);
            }
        }
        if let Some(eb) = &c.else_body {
            return self.exec_program(eb, out, stdin);
        }
        self.last_status = 0;
        Flow::Next
    }

    fn exec_loop(&mut self, c: &LoopClause, out: &mut Out, stdin: &StdinSrc) -> Flow {
        // POSIX: the loop's exit status is that of the last body execution, or 0
        // if the body never ran. Track it so a failing *condition* test (whose
        // non-zero status ends the loop) does not leak out — which matters under
        // `set -e`.
        let mut body_status = 0;
        loop {
            let flow = self.exec_condition(&c.cond, out, stdin);
            if !matches!(flow, Flow::Next) {
                return flow;
            }
            let cond_true = self.last_status == 0;
            let run = if c.until { !cond_true } else { cond_true };
            if !run {
                break;
            }
            match self.exec_program(&c.body, out, stdin) {
                Flow::Next => {}
                Flow::Break(n) => {
                    if n > 1 {
                        return Flow::Break(n - 1);
                    }
                    break;
                }
                Flow::Continue(n) => {
                    if n > 1 {
                        return Flow::Continue(n - 1);
                    }
                }
                other => return other,
            }
            body_status = self.last_status;
        }
        self.last_status = body_status;
        Flow::Next
    }

    fn exec_for(&mut self, c: &ForClause, out: &mut Out, stdin: &StdinSrc) -> Flow {
        let items: Vec<String> = match &c.words {
            Some(words) => {
                let mut v = Vec::new();
                for w in words {
                    for bw in crate::brace::expand_braces(w) {
                        v.extend(self.expand_word(&bw, true));
                    }
                }
                v
            }
            None => self.positional.clone(),
        };
        // A `for` over an empty list runs no body and has exit status 0.
        let mut body_status = 0;
        for item in items {
            self.vars.insert(c.var.clone(), item);
            match self.exec_program(&c.body, out, stdin) {
                Flow::Next => {}
                Flow::Break(n) => {
                    if n > 1 {
                        return Flow::Break(n - 1);
                    }
                    break;
                }
                Flow::Continue(n) => {
                    if n > 1 {
                        return Flow::Continue(n - 1);
                    }
                }
                other => return other,
            }
            body_status = self.last_status;
        }
        self.last_status = body_status;
        Flow::Next
    }

    /// `select name [in words]; do body; done` — bash's interactive menu loop.
    /// Prints the numbered word list (once, and again after a blank line) to
    /// stderr, writes the `PS3` prompt (default `#? `), reads a line from stdin,
    /// stores the raw line in `REPLY`, sets `name` to the chosen word (empty when
    /// the input is not a valid item number), and runs the body — repeating until
    /// EOF or `break`. The loop's exit status is the last body execution (0 if the
    /// body never runs).
    fn exec_select(&mut self, c: &SelectClause, out: &mut Out, stdin: &StdinSrc) -> Flow {
        let items: Vec<String> = match &c.words {
            Some(words) => {
                let mut v = Vec::new();
                for w in words {
                    for bw in crate::brace::expand_braces(w) {
                        v.extend(self.expand_word(&bw, true));
                    }
                }
                v
            }
            None => self.positional.clone(),
        };
        // An empty item list runs no body and exits with status 0.
        if items.is_empty() {
            self.last_status = 0;
            return Flow::Next;
        }
        let ps3 = self.vars.get("PS3").cloned().unwrap_or_else(|| "#? ".to_string());
        let redir = RedirPlan::default();
        let mut body_status = 0;
        let mut show_menu = true;
        loop {
            if show_menu {
                let mut menu = String::new();
                for (i, it) in items.iter().enumerate() {
                    // `i + 1` cannot overflow: item counts are bounded by memory.
                    menu.push_str(&format!("{}) {it}\n", i + 1));
                }
                self.emit_stderr(menu.as_bytes());
                show_menu = false;
            }
            self.emit_stderr(ps3.as_bytes());
            let line = match self.read_line(stdin, &redir) {
                Some(l) => l,
                None => {
                    // EOF: bash emits a newline and terminates the loop.
                    self.emit_stderr(b"\n");
                    break;
                }
            };
            self.vars.insert("REPLY".to_string(), line.clone());
            let trimmed = line.trim();
            if trimmed.is_empty() {
                // A blank line reprints the menu without running the body.
                show_menu = true;
                continue;
            }
            let choice = match trimmed.parse::<usize>() {
                Ok(n) if n >= 1 && n <= items.len() => items[n - 1].clone(),
                _ => String::new(),
            };
            self.vars.insert(c.var.clone(), choice);
            match self.exec_program(&c.body, out, stdin) {
                Flow::Next => {}
                Flow::Break(n) => {
                    if n > 1 {
                        return Flow::Break(n - 1);
                    }
                    break;
                }
                Flow::Continue(n) => {
                    if n > 1 {
                        return Flow::Continue(n - 1);
                    }
                }
                other => return other,
            }
            body_status = self.last_status;
        }
        self.last_status = body_status;
        Flow::Next
    }

    /// The `let arg …` builtin. Evaluates each argument as an arithmetic
    /// expression (applying any assignment/increment side effects). The exit
    /// status is 0 when the *last* expression evaluates non-zero, 1 when it is
    /// zero; an arithmetic error or no arguments yields status 1 (bash: 2 for
    /// "expression expected", but 1 for a zero result — we report 1 for both).
    fn builtin_let(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            self.emit_stderr(b"osh: let: expression expected\n");
            return 1;
        }
        let mut last = 0i64;
        for arg in args {
            match self.eval_arith_raw(arg) {
                Some(v) => last = v,
                None => return 1, // the arithmetic error was already reported
            }
        }
        i32::from(last == 0)
    }

    /// Evaluate a raw arithmetic section (expand `$params`, then evaluate),
    /// mutating shell state for any assignment/increment operators. Returns the
    /// value, or `None` after printing the error.
    fn eval_arith_raw(&mut self, raw: &str) -> Option<i64> {
        let expanded = self.expand_arith_params(raw);
        match arith::eval(&expanded, self) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("osh: arithmetic: {e}");
                None
            }
        }
    }

    /// C-style `for (( init; cond; update )); do body; done`. `init` runs once;
    /// the loop runs while `cond` is non-zero (an empty `cond` is always true);
    /// `update` runs after each iteration (including after `continue`). An
    /// arithmetic error in any section aborts the loop with status 1.
    fn exec_for_arith(&mut self, c: &ForArithClause, out: &mut Out, stdin: &StdinSrc) -> Flow {
        self.last_status = 0;
        if !c.init.is_empty() && self.eval_arith_raw(&c.init).is_none() {
            self.last_status = 1;
            return Flow::Next;
        }
        loop {
            if !c.cond.is_empty() {
                match self.eval_arith_raw(&c.cond) {
                    Some(0) => break,
                    Some(_) => {}
                    None => {
                        self.last_status = 1;
                        return Flow::Next;
                    }
                }
            }
            match self.exec_program(&c.body, out, stdin) {
                Flow::Next => {}
                Flow::Break(n) => {
                    if n > 1 {
                        return Flow::Break(n - 1);
                    }
                    break;
                }
                Flow::Continue(n) => {
                    if n > 1 {
                        return Flow::Continue(n - 1);
                    }
                    // `continue` still runs the update section below.
                }
                other => return other,
            }
            if !c.update.is_empty() && self.eval_arith_raw(&c.update).is_none() {
                self.last_status = 1;
                return Flow::Next;
            }
        }
        Flow::Next
    }

    fn exec_case(&mut self, c: &CaseClause, out: &mut Out, stdin: &StdinSrc) -> Flow {
        let subject: Vec<char> = self.expand_to_string(&c.word).chars().collect();
        // `shopt -s nocasematch` makes `case` (and `[[ == ]]`) matching
        // case-insensitive.
        let ci = self.shopt.get("nocasematch").copied().unwrap_or(false);
        let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
        self.last_status = 0;
        let mut idx = 0;
        while idx < c.items.len() {
            let item = &c.items[idx];
            let matched = item.patterns.iter().any(|pat| {
                let pattern: Vec<char> = self.expand_to_string(pat).chars().collect();
                glob_match_ci(&pattern, &subject, ci, extglob)
            });
            if !matched {
                idx += 1;
                continue;
            }
            // Run this arm's body, then honor its terminator. `;&` falls through
            // to run the next arm's body unconditionally; `;;&` resumes pattern
            // testing at the following arms; `;;` (Break) stops.
            let flow = self.exec_program(&c.items[idx].body, out, stdin);
            if !matches!(flow, Flow::Next) {
                return flow;
            }
            match c.items[idx].term {
                CaseTerm::Break => return Flow::Next,
                CaseTerm::ContinueMatch => {
                    idx += 1;
                }
                CaseTerm::FallThrough => {
                    // Fall through: run subsequent arm bodies unconditionally
                    // until one breaks or we run out of arms.
                    idx += 1;
                    while idx < c.items.len() {
                        let flow = self.exec_program(&c.items[idx].body, out, stdin);
                        if !matches!(flow, Flow::Next) {
                            return flow;
                        }
                        match c.items[idx].term {
                            CaseTerm::Break => return Flow::Next,
                            CaseTerm::ContinueMatch => {
                                // Resume pattern testing at the following arm.
                                idx += 1;
                                break;
                            }
                            CaseTerm::FallThrough => idx += 1,
                        }
                    }
                }
            }
        }
        Flow::Next
    }

    /// Execute a compound command carrying trailing redirections.
    ///
    /// Input redirects (`< file`, here-doc `<<`, here-string `<<<`) load their
    /// bytes into a shared position-tracking cursor that is threaded through the
    /// whole command, so a `while read …; done < file` loop consumes successive
    /// lines. Output redirects (`> file`, `>> file`) capture the command's
    /// entire stdout and write it to the file when it finishes.
    ///
    /// Stderr redirects (`2> file`, `2>> file`, `2>&1`) push a [`StderrTarget`]
    /// onto [`Shell::stderr_stack`] for the duration of the body, so every
    /// command in the group — externals, builtin diagnostics, and `>&2` writes —
    /// honours the redirect. `1>&2` (`stdout_to_stderr`) routes the body's stdout
    /// to the current stderr sink.
    fn exec_redirected(
        &mut self,
        inner: &Command,
        redirects: &[Redirect],
        out: &mut Out,
        stdin: &StdinSrc,
    ) -> Flow {
        let plan = match self.resolve_redirects(redirects) {
            Ok(p) => p,
            Err(msg) => {
                self.errln(&format!("osh: {msg}"));
                self.last_status = 1;
                return Flow::Next;
            }
        };

        // Establish the input bytes (if the command redirects stdin).
        let input_bytes: Option<Vec<u8>> = if let Some(data) = plan.stdin_data.clone() {
            Some(data)
        } else if let Some(path) = &plan.stdin {
            match std::fs::read(path) {
                Ok(b) => Some(b),
                Err(e) => {
                    self.errln(&format!("osh: {path}: {e}"));
                    self.last_status = 1;
                    return Flow::Next;
                }
            }
        } else {
            None
        };

        // ---- stderr setup: push a target covering the whole body ----
        // `stderr_merge_buf` is the buffer whose bytes must be folded into the
        // stdout capture once the body finishes (the `2>&1`-into-captured-stdout
        // case, where fd 1 and fd 2 share a command-substitution buffer).
        let mut pushed_stderr = false;
        let mut stderr_merge_buf: Option<Arc<Mutex<Vec<u8>>>> = None;
        if let Some((path, append)) = &plan.stderr {
            match open_out(path, *append) {
                Ok(f) => {
                    self.stderr_stack.push(StderrTarget::File(Arc::new(f)));
                    pushed_stderr = true;
                }
                Err(e) => {
                    self.errln(&format!("osh: {path}: {e}"));
                    self.last_status = 1;
                    return Flow::Next;
                }
            }
        } else if plan.stderr_to_stdout {
            // `2>&1` with fd 1 not a file: mirror fd 1's live sink.
            match out {
                Out::Capture(_) => {
                    let buf = Arc::new(Mutex::new(Vec::new()));
                    self.stderr_stack.push(StderrTarget::Buffer(Arc::clone(&buf)));
                    stderr_merge_buf = Some(buf);
                    pushed_stderr = true;
                }
                Out::Pipe(w) => match w.try_clone() {
                    Ok(wp) => {
                        self.stderr_stack.push(StderrTarget::Pipe(Arc::new(wp)));
                        pushed_stderr = true;
                    }
                    Err(e) => {
                        self.errln(&format!("osh: pipe: {e}"));
                        self.last_status = 1;
                        return Flow::Next;
                    }
                },
                Out::Inherit => {
                    self.stderr_stack.push(StderrTarget::Stdout);
                    pushed_stderr = true;
                }
            }
        }

        // Capture stdout when it is redirected to a file (`> f`) or routed to
        // stderr (`1>&2`); otherwise the body writes straight to `out`.
        let stdout_to_err = plan.stdout_to_stderr && plan.stdout.is_none();
        let mut capture: Option<Vec<u8>> = if plan.stdout.is_some() || stdout_to_err {
            Some(Vec::new())
        } else {
            None
        };

        let flow = {
            let input_cursor;
            let owned_stdin;
            let sin: &StdinSrc = match input_bytes {
                Some(bytes) => {
                    input_cursor = RefCell::new(io::Cursor::new(bytes));
                    owned_stdin = StdinSrc::Cursor(&input_cursor);
                    &owned_stdin
                }
                None => stdin,
            };
            match &mut capture {
                Some(buf) => {
                    let mut o = Out::Capture(buf);
                    self.exec_command(inner, &mut o, sin)
                }
                None => self.exec_command(inner, out, sin),
            }
        };

        // Finalise captured stdout: to the target file, or to the stderr sink
        // (`1>&2`) — the latter while the stderr target is still on the stack.
        if let Some(buf) = capture {
            if let Some((path, append)) = &plan.stdout {
                match open_out(path, *append) {
                    Ok(mut f) => {
                        if let Err(e) = f.write_all(&buf) {
                            self.errln(&format!("osh: {path}: {e}"));
                            self.last_status = 1;
                        }
                    }
                    Err(e) => {
                        self.errln(&format!("osh: {path}: {e}"));
                        self.last_status = 1;
                    }
                }
            } else if stdout_to_err {
                self.emit_stderr(&buf);
            }
        }

        if pushed_stderr {
            self.stderr_stack.pop();
        }

        // Fold captured stderr (`2>&1` into a captured stdout) into `out` after
        // the target is popped. Interleaving with stdout is not preserved.
        if let Some(buf) = stderr_merge_buf
            && let Ok(g) = buf.lock()
            && let Out::Capture(obuf) = out
        {
            obuf.extend_from_slice(&g);
        }
        flow
    }

    /// Execute a `[[ … ]]` conditional expression: exit 0 if true, 1 if false.
    fn exec_cond(&mut self, e: &CondExpr) -> Flow {
        let ok = self.cond_eval(e);
        self.last_status = i32::from(!ok);
        Flow::Next
    }

    /// Execute a `(( … ))` arithmetic command: exit 0 if the value is non-zero.
    fn exec_arith(&mut self, raw: &str) -> Flow {
        let expanded = self.expand_arith_params(raw);
        match arith::eval(&expanded, self) {
            Ok(v) => self.last_status = i32::from(v == 0),
            Err(e) => {
                eprintln!("osh: arithmetic: {e}");
                self.last_status = 1;
            }
        }
        Flow::Next
    }

    /// Evaluate a `[[ … ]]` conditional expression tree to a boolean.
    fn cond_eval(&mut self, e: &CondExpr) -> bool {
        match e {
            CondExpr::Word(w) => !self.expand_to_string(w).is_empty(),
            CondExpr::Not(inner) => !self.cond_eval(inner),
            CondExpr::And(a, b) => self.cond_eval(a) && self.cond_eval(b),
            CondExpr::Or(a, b) => self.cond_eval(a) || self.cond_eval(b),
            CondExpr::Unary(op, w) => self.cond_unary(*op, w),
            CondExpr::Binary(l, op, r) => self.cond_binary(l, *op, r),
            CondExpr::Regex(l, r) => self.cond_regex(l, r),
        }
    }

    /// Evaluate `lhs =~ rhs` (POSIX-ERE match). On success, populate the
    /// `BASH_REMATCH` indexed array (element 0 = whole match, i = capture i;
    /// unmatched groups become empty strings) and return true. A malformed
    /// pattern reports an error to stderr and yields false (matching bash,
    /// which returns status 2 — we surface false without aborting the shell).
    fn cond_regex(&mut self, l: &Word, r: &Word) -> bool {
        let subject = self.expand_to_string(l);
        // Quote-aware RHS: bash treats *unquoted* portions of the pattern as
        // regex and *quoted* portions (single/double quotes) as literal text —
        // so `[[ a.b =~ "a.b" ]]` matches only the literal, while `[[ … =~ a.b ]]`
        // lets `.` be any char. `regex_pattern_from_rhs` escapes the metacharacters
        // of quoted segments and passes unquoted ones through untouched.
        let pattern = self.regex_pattern_from_rhs(r);
        // `shopt -s nocasematch` also makes `=~` case-insensitive.
        let ci = self.shopt.get("nocasematch").copied().unwrap_or(false);
        let re = match crate::ere::Regex::new_flags(&pattern, ci) {
            Ok(re) => re,
            Err(e) => {
                self.errln(&format!("osh: [[: =~: invalid regex: {}", e.0));
                return false;
            }
        };
        match re.captures(&subject) {
            Some(groups) => {
                // Each capture slot maps 1:1 to a BASH_REMATCH index; unmatched
                // optional groups are stored as empty strings, as bash does.
                let elems: BTreeMap<usize, String> = groups
                    .into_iter()
                    .enumerate()
                    .map(|(i, g)| (i, g.unwrap_or_default()))
                    .collect();
                self.arrays.insert("BASH_REMATCH".to_string(), elems);
                true
            }
            None => {
                // A failed match clears BASH_REMATCH (bash empties it).
                self.arrays.insert("BASH_REMATCH".to_string(), BTreeMap::new());
                false
            }
        }
    }

    /// Build the ERE pattern for a `=~` right-hand side, honouring bash's
    /// quote-aware rule: characters that come from *quoted* word parts
    /// (single- or double-quoted, including the expanded contents of a
    /// double-quoted `"$var"`) are matched literally — their regex
    /// metacharacters are backslash-escaped — while *unquoted* parts (bare
    /// literals and unquoted `$var`/`$(…)` expansions) contribute active regex
    /// syntax. No field splitting or globbing is performed (this is `[[ … ]]`).
    fn regex_pattern_from_rhs(&mut self, word: &Word) -> String {
        fn escape_ere(s: &str, out: &mut String) {
            for c in s.chars() {
                // The full ERE metacharacter set; escaping any other char is a
                // no-op but escaping these makes the segment match literally.
                if matches!(
                    c,
                    '\\' | '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}'
                        | '|'
                ) {
                    out.push('\\');
                }
                out.push(c);
            }
        }
        let mut pattern = String::new();
        for part in &word.parts {
            match part {
                // Unquoted literal text is live regex syntax.
                WordPart::Literal(s) => pattern.push_str(s),
                // Single quotes: everything literal.
                WordPart::SingleQuoted(s) => escape_ere(s, &mut pattern),
                // Double quotes: expand (params/cmd-sub run) but the result is
                // matched literally, per bash.
                WordPart::DoubleQuoted(parts) => {
                    let s = self.expand_double_quoted(parts);
                    escape_ere(&s, &mut pattern);
                }
                // Unquoted dynamic parts (`$var`, `${…}`, `$(…)`, `$((…))`):
                // their expansion is live regex, so a variable can carry a
                // pattern (`p='^h.*o$'; [[ hello =~ $p ]]`).
                other => pattern.push_str(&self.expand_dynamic(other)),
            }
        }
        pattern
    }

    fn cond_unary(&mut self, op: UnaryOp, w: &Word) -> bool {
        // `-z`/`-n` operate on the string value; the rest are file tests.
        match op {
            UnaryOp::ZeroLen => self.expand_to_string(w).is_empty(),
            UnaryOp::NonZeroLen => !self.expand_to_string(w).is_empty(),
            // `-v name` tests whether the shell variable/element is set; the
            // operand is the *name*, not a value to expand to.
            UnaryOp::VarSet => {
                let name = self.expand_to_string(w);
                self.var_is_set(&name)
            }
            _ => {
                let path = self.expand_to_string(w);
                let meta = std::fs::metadata(&path);
                match op {
                    UnaryOp::Exists => meta.is_ok(),
                    UnaryOp::File => meta.map(|m| m.is_file()).unwrap_or(false),
                    UnaryOp::Dir => meta.map(|m| m.is_dir()).unwrap_or(false),
                    UnaryOp::NonEmptyFile => {
                        meta.map(|m| m.is_file() && m.len() > 0).unwrap_or(false)
                    }
                    // Best-effort permission tests: `-r` ≈ exists, `-w` ≈ exists
                    // and not read-only, `-x` ≈ exists. Proper mode-bit checks
                    // arrive with the slateos permission model (see todo.txt).
                    UnaryOp::Readable => meta.is_ok(),
                    UnaryOp::Writable => meta.map(|m| !m.permissions().readonly()).unwrap_or(false),
                    UnaryOp::Executable => meta.is_ok(),
                    UnaryOp::ZeroLen | UnaryOp::NonZeroLen | UnaryOp::VarSet => unreachable!(),
                }
            }
        }
    }

    fn cond_binary(&mut self, l: &Word, op: CondBinOp, r: &Word) -> bool {
        match op {
            CondBinOp::StrEq | CondBinOp::StrNe => {
                let subject: Vec<char> = self.expand_to_string(l).chars().collect();
                let rhs = self.expand_to_string(r);
                // `shopt -s nocasematch` folds case for both the literal and the
                // glob comparison.
                let ci = self.shopt.get("nocasematch").copied().unwrap_or(false);
                // A fully-quoted RHS is a literal; otherwise it is a glob pattern.
                let matched = if word_is_all_quoted(r) {
                    let lhs: String = subject.iter().collect();
                    if ci {
                        lhs.to_lowercase() == rhs.to_lowercase()
                    } else {
                        lhs == rhs
                    }
                } else {
                    let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
                    let pat: Vec<char> = rhs.chars().collect();
                    glob_match_ci(&pat, &subject, ci, extglob)
                };
                if matches!(op, CondBinOp::StrEq) {
                    matched
                } else {
                    !matched
                }
            }
            CondBinOp::StrLt => self.expand_to_string(l) < self.expand_to_string(r),
            CondBinOp::StrGt => self.expand_to_string(l) > self.expand_to_string(r),
            CondBinOp::NumEq
            | CondBinOp::NumNe
            | CondBinOp::NumLt
            | CondBinOp::NumLe
            | CondBinOp::NumGt
            | CondBinOp::NumGe => {
                let a = self.eval_arith_word(l);
                let b = self.eval_arith_word(r);
                match op {
                    CondBinOp::NumEq => a == b,
                    CondBinOp::NumNe => a != b,
                    CondBinOp::NumLt => a < b,
                    CondBinOp::NumLe => a <= b,
                    CondBinOp::NumGt => a > b,
                    CondBinOp::NumGe => a >= b,
                    _ => unreachable!(),
                }
            }
            CondBinOp::FileNewer => {
                file_cmp("-nt", &self.expand_to_string(l), &self.expand_to_string(r))
            }
            CondBinOp::FileOlder => {
                file_cmp("-ot", &self.expand_to_string(l), &self.expand_to_string(r))
            }
            CondBinOp::SameFile => {
                file_cmp("-ef", &self.expand_to_string(l), &self.expand_to_string(r))
            }
        }
    }

    fn clone_for_subshell(&self) -> Shell {
        Shell {
            vars: self.vars.clone(),
            arrays: self.arrays.clone(),
            assoc: self.assoc.clone(),
            exported: self.exported.clone(),
            funcs: self.funcs.clone(),
            positional: self.positional.clone(),
            name: self.name.clone(),
            last_status: self.last_status,
            last_bg_pid: self.last_bg_pid,
            pipefail: self.pipefail,
            pipe_broken: false,
            pid: self.pid,
            // A subshell inherits fd 2 = the shell's real stderr; any active
            // compound-command stderr redirect does not carry into a pipeline
            // stage's own subshell (and keeping the `Arc`s off the clone is what
            // lets `Shell` stay `Send` for the scoped-thread pipeline).
            stderr_stack: Vec::new(),
            getopts_col: self.getopts_col,
            getopts_optind: self.getopts_optind,
            seconds_anchor: self.seconds_anchor,
            seconds_base: self.seconds_base,
            rng: std::cell::Cell::new(self.rng.get()),
            errexit: self.errexit,
            nounset: self.nounset,
            xtrace: self.xtrace,
            // A subshell starts outside any condition/negation context.
            errexit_suppress: 0,
            unbound_error: false,
            // A subshell body is not itself a function frame; a `local` there is
            // an error until it enters one of its own function calls.
            local_frames: Vec::new(),
            readonly: self.readonly.clone(),
            shopt: self.shopt.clone(),
            integer_attr: self.integer_attr.clone(),
            lower_attr: self.lower_attr.clone(),
            upper_attr: self.upper_attr.clone(),
            dir_stack: self.dir_stack.clone(),
            // A subshell resets non-ignored traps to their default disposition
            // (bash). Ignored ('') traps are inherited; keep only those.
            traps: self
                .traps
                .iter()
                .filter(|(_, v)| v.is_empty())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            // The EXIT trap only fires for the top-level shell in our model.
            exit_trap_done: true,
            in_trap: false,
            // A subshell does not inherit the parent's job table.
            jobs: Vec::new(),
        }
    }

    // ---- assignments and arrays ---------------------------------------------

    /// Apply a standalone assignment to shell state, handling scalars, indexed
    /// elements (`name[i]=v`), whole arrays (`name=(a b c)`), and append (`+=`).
    /// Apply a variable/array assignment. Returns `false` (and reports) if the
    /// target is readonly, leaving the existing value intact; `true` otherwise.
    /// Apply the `declare -l`/`-u` case attribute (if any) of `name` to a value
    /// about to be stored. Lowercase (`-l`) and uppercase (`-u`) are mutually
    /// exclusive in bash; if both are somehow set, uppercase wins here.
    fn fold_case_attr(&self, name: &str, val: String) -> String {
        if self.upper_attr.contains(name) {
            val.to_uppercase()
        } else if self.lower_attr.contains(name) {
            val.to_lowercase()
        } else {
            val
        }
    }

    fn apply_assignment(&mut self, a: &Assignment) -> bool {
        // A readonly variable cannot be reassigned; report and leave it intact.
        if self.readonly.contains(&a.name) {
            self.emit_stderr(format!("osh: {}: readonly variable\n", a.name).as_bytes());
            return false;
        }
        let is_assoc = self.assoc.contains_key(&a.name);
        match &a.value {
            AssignRhs::Scalar(w) => {
                let val = self.expand_to_string(w);
                // `RANDOM=n` reseeds the generator; `SECONDS=n` rebases the
                // elapsed-seconds counter. Both are dynamic and not stored in
                // `vars` (reads go through `param_value`'s special arms).
                if a.index.is_none() && !a.append {
                    if a.name == "RANDOM" {
                        if let Ok(seed) = val.trim().parse::<u32>() {
                            self.rng.set(seed);
                        }
                        return true;
                    }
                    if a.name == "SECONDS" {
                        self.seconds_base = val.trim().parse::<u64>().unwrap_or(0);
                        self.seconds_anchor = std::time::Instant::now();
                        return true;
                    }
                }
                // With the integer attribute (`declare -i`), the value is an
                // arithmetic expression: it is evaluated before storing, and
                // `+=` performs numeric addition rather than string append.
                let is_int = self.integer_attr.contains(&a.name);
                // With `declare -l`/`-u`, fold the value's case before storing.
                // Integer values are numeric (no case), so folding is skipped
                // on the integer path.
                let val = if is_int {
                    val
                } else {
                    self.fold_case_attr(&a.name, val)
                };
                if let Some(idx_word) = &a.index {
                    if is_assoc {
                        // `name[key]=val` — associative element (string key).
                        let key = self.expand_to_string(idx_word);
                        let stored = if is_int {
                            let base = if a.append {
                                self.assoc_element(&a.name, &key)
                                    .and_then(|s| s.trim().parse::<i64>().ok())
                                    .unwrap_or(0)
                            } else {
                                0
                            };
                            base.wrapping_add(self.eval_arith_raw(&val).unwrap_or(0))
                                .to_string()
                        } else {
                            val
                        };
                        // Integer append already folded the old value in, so
                        // store (not append) the computed result.
                        self.assoc_set(&a.name, key, stored, a.append && !is_int);
                    } else {
                        // `name[i]=val` — indexed element assignment. A negative
                        // index counts back from `highest_index + 1` (bash:
                        // `a[-1]=v` overwrites the last element).
                        let raw = self.eval_arith_word(idx_word);
                        let bound = self
                            .arrays
                            .get(&a.name)
                            .and_then(|arr| arr.keys().next_back().copied())
                            .map_or(0, |k| k.saturating_add(1));
                        let Some(idx) = Self::resolve_index(raw, bound) else {
                            eprintln!("osh: {}: bad array subscript", a.name);
                            return true;
                        };
                        let int_val = if is_int {
                            let base = if a.append {
                                self.arrays
                                    .get(&a.name)
                                    .and_then(|arr| arr.get(&idx))
                                    .and_then(|s| s.trim().parse::<i64>().ok())
                                    .unwrap_or(0)
                            } else {
                                0
                            };
                            Some(base.wrapping_add(self.eval_arith_raw(&val).unwrap_or(0)))
                        } else {
                            None
                        };
                        let arr = self.arrays.entry(a.name.clone()).or_default();
                        match int_val {
                            Some(n) => {
                                arr.insert(idx, n.to_string());
                            }
                            None if a.append => {
                                arr.entry(idx).or_default().push_str(&val);
                            }
                            None => {
                                arr.insert(idx, val);
                            }
                        }
                    }
                } else if a.append {
                    // `name+=val` — append to the scalar (or to element 0 of an array).
                    if is_int {
                        let base = self
                            .vars
                            .get(&a.name)
                            .and_then(|c| c.trim().parse::<i64>().ok())
                            .unwrap_or(0);
                        let sum = base.wrapping_add(self.eval_arith_raw(&val).unwrap_or(0));
                        self.vars.insert(a.name.clone(), sum.to_string());
                    } else if let Some(arr) = self.arrays.get_mut(&a.name) {
                        arr.entry(0).or_default().push_str(&val);
                    } else {
                        let cur = self.vars.get(&a.name).cloned().unwrap_or_default();
                        self.vars.insert(a.name.clone(), cur + &val);
                    }
                } else if is_int {
                    let n = self.eval_arith_raw(&val).unwrap_or(0);
                    self.vars.insert(a.name.clone(), n.to_string());
                } else {
                    self.vars.insert(a.name.clone(), val);
                }
            }
            AssignRhs::Array(items) if is_assoc => {
                // Associative literal: `m=([k]=v …)` (m already `declare -A`).
                if !a.append {
                    self.assoc.insert(a.name.clone(), Vec::new());
                }
                for e in items {
                    match e {
                        ArrayElem::Keyed { index, value } => {
                            let key = self.expand_to_string(index);
                            let val = self.expand_to_string(value);
                            self.assoc_set(&a.name, key, val, false);
                        }
                        ArrayElem::Positional(_) => {
                            eprintln!(
                                "osh: {}: must use subscript when assigning associative array",
                                a.name
                            );
                        }
                    }
                }
            }
            AssignRhs::Array(items) => {
                // Indexed literal: positional elements append at the running
                // index; `[i]=v` elements place at an explicit index. Stored
                // sparsely (a BTreeMap), so gaps between explicit indices are
                // absent rather than filled with empty strings.
                let mut elems: BTreeMap<usize, String> = if a.append {
                    self.arrays.get(&a.name).cloned().unwrap_or_default()
                } else {
                    BTreeMap::new()
                };
                // Append continues after the highest existing index.
                let mut next = elems.keys().next_back().map_or(0, |k| k.saturating_add(1));
                for e in items {
                    match e {
                        ArrayElem::Positional(w) => {
                            for v in self.expand_word(w, true) {
                                elems.insert(next, v);
                                next = next.saturating_add(1);
                            }
                        }
                        ArrayElem::Keyed { index, value } => {
                            let idx = self.eval_arith_word(index);
                            let val = self.expand_to_string(value);
                            if let Ok(idx) = usize::try_from(idx) {
                                elems.insert(idx, val);
                                next = idx.saturating_add(1);
                            } else {
                                eprintln!("osh: {}: bad array subscript", a.name);
                            }
                        }
                    }
                }
                self.arrays.insert(a.name.clone(), elems);
            }
        }
        true
    }

    /// Set an associative-array element, creating the array if needed.
    fn assoc_set(&mut self, name: &str, key: String, val: String, append: bool) {
        let map = self.assoc.entry(name.to_string()).or_default();
        if let Some(slot) = map.iter_mut().find(|(k, _)| *k == key) {
            if append {
                slot.1.push_str(&val);
            } else {
                slot.1 = val;
            }
        } else {
            map.push((key, val));
        }
    }

    /// Collapse an assignment into a `(name, value)` pair for command-prefix use
    /// (`FOO=bar cmd`). Arrays join their elements with a single space.
    fn assignment_prefix_value(&mut self, a: &Assignment) -> (String, String) {
        let val = match &a.value {
            AssignRhs::Scalar(w) => self.expand_to_string(w),
            AssignRhs::Array(items) => {
                let mut elems: Vec<String> = Vec::new();
                for e in items {
                    match e {
                        ArrayElem::Positional(w) => elems.extend(self.expand_word(w, true)),
                        ArrayElem::Keyed { value, .. } => elems.push(self.expand_to_string(value)),
                    }
                }
                elems.join(" ")
            }
        };
        (a.name.clone(), val)
    }

    /// All values of `name`, treating a plain scalar as a one-element array.
    fn array_elements(&self, name: &str) -> Vec<String> {
        if let Some(m) = self.assoc.get(name) {
            m.iter().map(|(_, v)| v.clone()).collect()
        } else if let Some(a) = self.arrays.get(name) {
            a.values().cloned().collect()
        } else if let Some(v) = self.vars.get(name) {
            vec![v.clone()]
        } else {
            Vec::new()
        }
    }

    /// The keys (associative) or indices (indexed) of `name`, in order.
    fn array_keys(&self, name: &str) -> Vec<String> {
        if let Some(m) = self.assoc.get(name) {
            m.iter().map(|(k, _)| k.clone()).collect()
        } else if let Some(a) = self.arrays.get(name) {
            a.keys().map(usize::to_string).collect()
        } else if self.vars.contains_key(name) {
            vec!["0".to_string()]
        } else {
            Vec::new()
        }
    }

    /// Resolve a possibly-negative array subscript against a length, using bash
    /// semantics: a negative index counts back from the end (`-1` is the last
    /// element, `-2` the second-to-last, …). Returns `None` when the resolved
    /// index is negative (out of range past the start); a non-negative result
    /// past the end is left for the caller's bounds check.
    fn resolve_index(idx: i64, len: usize) -> Option<usize> {
        let abs = if idx < 0 {
            // len + idx, e.g. -1 → len-1. Guard the conversions/overflow.
            i64::try_from(len).ok()?.checked_add(idx)?
        } else {
            idx
        };
        usize::try_from(abs).ok()
    }

    /// A single array element by index (scalar acts as a one-element array).
    /// Negative indices count from the end (`-1` = last). `None` if the index
    /// is out of range.
    fn array_element(&self, name: &str, idx: i64) -> Option<String> {
        if let Some(a) = self.arrays.get(name) {
            // Negative indices count back from `highest_index + 1` (bash sparse
            // semantics), not from the element count.
            let bound = a.keys().next_back().map_or(0, |k| k.saturating_add(1));
            let real = Self::resolve_index(idx, bound)?;
            a.get(&real).cloned()
        } else if let Some(v) = self.vars.get(name) {
            // A scalar behaves as a one-element array at index 0.
            match Self::resolve_index(idx, 1)? {
                0 => Some(v.clone()),
                _ => None,
            }
        } else {
            None
        }
    }

    /// An associative-array value by string key.
    fn assoc_element(&self, name: &str, key: &str) -> Option<String> {
        self.assoc
            .get(name)?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    /// Resolve the base value for a parameter expansion operator, honoring an
    /// optional element subscript. `None` (no subscript) reads the scalar/plain
    /// parameter; `Some(w)` reads element `w` — a string key for associative
    /// arrays, else an arithmetic index for indexed arrays. Returns `None` when
    /// the parameter/element is unset.
    fn param_elem_value(&mut self, name: &str, index: &Option<Box<Word>>) -> Option<String> {
        match index {
            None => self.param_value(name),
            Some(w) => {
                if self.assoc.contains_key(name) {
                    let key = self.expand_to_string(w);
                    self.assoc_element(name, &key)
                } else {
                    let idx = self.eval_arith_word(w);
                    self.array_element(name, idx)
                }
            }
        }
    }

    /// Write `value` back to a parameter or array element, honoring an optional
    /// subscript. Used by `${name[i]:=default}` (assign-default). Out-of-range
    /// negative indices are ignored (matching bash's "bad subscript" no-op here).
    fn assign_elem(&mut self, name: &str, index: &Option<Box<Word>>, value: String) {
        match index {
            None => {
                self.vars.insert(name.to_string(), value);
            }
            Some(w) => {
                if self.assoc.contains_key(name) {
                    let key = self.expand_to_string(w);
                    self.assoc_set(name, key, value, false);
                } else {
                    let idx = self.eval_arith_word(w);
                    let arr = self.arrays.entry(name.to_string()).or_default();
                    let bound = arr.keys().next_back().map_or(0, |k| k.saturating_add(1));
                    if let Some(real) = Self::resolve_index(idx, bound) {
                        arr.insert(real, value);
                    }
                }
            }
        }
    }

    /// Expand `${name[index]}` / `${name[@]}` / `${#name[@]}` to a string
    /// (scalar context; `[@]`/`[*]` join with a space).
    /// `${!ref}` — indirect expansion: read the variable whose name is the
    /// value of `ref`. The referent may itself name an array element
    /// (`ref=a[0]` / `ref=m[key]`).
    fn expand_indirect(&mut self, refname: &str) -> String {
        let Some(target) = self.param_value(refname) else {
            return String::new();
        };
        if target.is_empty() {
            return String::new();
        }
        // The referent may name an array element: `ref=a[0]`, `ref=m[key]`.
        if let Some(open) = target.find('[')
            && let Some(inner) = target.strip_suffix(']')
        {
            let name = &target[..open];
            let sub = &inner[open + 1..];
            if self.assoc.contains_key(name) {
                return self.assoc_element(name, sub).unwrap_or_default();
            }
            let idx = self.eval_arith_raw(sub).unwrap_or(0);
            return self.array_element(name, idx).unwrap_or_default();
        }
        self.param_value(&target).unwrap_or_default()
    }

    /// `${name@op}` parameter transformation. Supports `Q` (quote so the value
    /// can be reused as shell input), `U`/`u`/`L` (upper-all/upper-first/
    /// lower-all), `E` (expand ANSI-C backslash escapes), and `a` (attribute
    /// flags — `a` for indexed array, `A` for associative, else empty).
    fn param_transform(&mut self, name: &str, index: &Option<Box<Word>>, op: char) -> String {
        // The `a` (attributes) transform reports type even for an unset scalar.
        if op == 'a' {
            let mut flags = String::new();
            if self.assoc.contains_key(name) {
                flags.push('A');
            } else if self.arrays.contains_key(name) {
                flags.push('a');
            }
            return flags;
        }
        let value = self.param_elem_value(name, index).unwrap_or_default();
        match op {
            'Q' => shell_quote(&value),
            'U' => value.chars().flat_map(char::to_uppercase).collect(),
            'u' => {
                let mut cs = value.chars();
                match cs.next() {
                    Some(f) => f.to_uppercase().chain(cs).collect(),
                    None => String::new(),
                }
            }
            'L' => value.chars().flat_map(char::to_lowercase).collect(),
            'E' => ansi_c_unescape(&value),
            // `P` (prompt) and `K`/`k` (assoc key/value) are not implemented;
            // return the value unchanged rather than erroring.
            _ => value,
        }
    }

    /// `${!prefix*}` / `${!prefix@}` — the names of all set variables (scalars,
    /// indexed arrays, associative arrays) whose name begins with `prefix`,
    /// sorted (bash lists them in lexicographic order).
    fn var_names_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .vars
            .keys()
            .chain(self.arrays.keys())
            .chain(self.assoc.keys())
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn expand_array_ref(&mut self, name: &str, index: &ArrayIndex, length: bool) -> String {
        match index {
            ArrayIndex::All | ArrayIndex::Star => {
                let elems = self.array_elements(name);
                if length {
                    elems.len().to_string()
                } else {
                    elems.join(" ")
                }
            }
            ArrayIndex::Index(w) => {
                // Associative subscripts are string keys, not arithmetic.
                let val = if self.assoc.contains_key(name) {
                    let key = self.expand_to_string(w);
                    self.assoc_element(name, &key)
                } else {
                    let idx = self.eval_arith_word(w);
                    self.array_element(name, idx)
                };
                if length {
                    val.map_or(0, |v| v.chars().count()).to_string()
                } else {
                    val.unwrap_or_default()
                }
            }
        }
    }

    // ---- simple command execution -------------------------------------------

    fn exec_simple(&mut self, sc: &SimpleCommand, out: &mut Out, stdin: &StdinSrc) -> Flow {
        // The DEBUG trap runs before each simple command (guarded so a handler's
        // own commands don't recurse).
        if !self.in_trap && self.traps.contains_key("DEBUG") {
            self.fire_trap("DEBUG");
        }
        // Expand the command words into argv (with the current variable values,
        // before any prefix assignments take effect).
        let mut argv: Vec<String> = Vec::new();
        for w in &sc.words {
            // Brace expansion runs first (textually, before parameter/other
            // expansion), turning one word into one or more words.
            for bw in crate::brace::expand_braces(w) {
                argv.extend(self.expand_word(&bw, true));
            }
        }

        // `set -u`: a reference to an unset variable during expansion aborts the
        // shell (matching a non-interactive bash under nounset).
        if self.unbound_error {
            self.unbound_error = false;
            self.last_status = 1;
            return Flow::Exit(1);
        }

        // Pure assignment (no command word): persist the variables/arrays.
        // A readonly-variable rejection makes the whole command fail (status 1).
        if argv.is_empty() {
            let mut ok = true;
            for a in &sc.assignments {
                if !self.apply_assignment(a) {
                    ok = false;
                }
            }
            self.last_status = i32::from(!ok);
            return Flow::Next;
        }

        // `set -x`: trace the command (prefixed with PS4's default `+ `) to
        // stderr before running it.
        if self.xtrace {
            let mut line = String::from("+ ");
            line.push_str(&argv.join(" "));
            line.push('\n');
            self.emit_stderr(line.as_bytes());
        }

        // Command present: build scalar env prefixes (`FOO=bar cmd`). Array and
        // indexed prefix assignments collapse to a space-joined scalar.
        let mut assigns: Vec<(String, String)> = Vec::with_capacity(sc.assignments.len());
        for a in &sc.assignments {
            assigns.push(self.assignment_prefix_value(a));
        }

        // Resolve redirections (targets are expanded now).
        let redir = match self.resolve_redirects(&sc.redirects) {
            Ok(r) => r,
            Err(msg) => {
                eprintln!("osh: {msg}");
                self.last_status = 1;
                return Flow::Next;
            }
        };

        let name = argv[0].clone();

        // `declare -A m=([k]=v)` one-liner: array-literal operands are attached
        // to the command as `decl_arrays`; apply them with the declared kind.
        if !sc.decl_arrays.is_empty() && matches!(name.as_str(), "declare" | "typeset" | "local") {
            return self.exec_declare_with_arrays(&argv, &sc.decl_arrays);
        }

        // `command …` (bypass shell functions) and `builtin …` (force builtin
        // lookup) re-dispatch a sub-command, so they are handled before the
        // normal function/builtin/external resolution below.
        if name == "command" {
            return self.exec_command_builtin(&argv, &assigns, out, stdin, &redir);
        }
        if name == "builtin" {
            return self.exec_builtin_builtin(&argv, &assigns, out, stdin, &redir);
        }

        // Function?
        if self.funcs.contains_key(&name) {
            return self.call_function(&name, &argv[1..], &assigns, out, stdin, &redir);
        }

        // Builtin?
        if is_builtin(&name) {
            return self.run_builtin(&name, &argv, &assigns, out, stdin, &redir);
        }

        // External command.
        self.run_external(&argv, &assigns, out, stdin, &redir);
        Flow::Next
    }

    fn call_function(
        &mut self,
        name: &str,
        args: &[String],
        assigns: &[(String, String)],
        out: &mut Out,
        stdin: &StdinSrc,
        _redir: &RedirPlan,
    ) -> Flow {
        let Some(body) = self.funcs.get(name).cloned() else {
            self.last_status = 127;
            return Flow::Next;
        };
        // Temporarily apply assignments and swap positionals.
        let saved_pos = std::mem::replace(&mut self.positional, args.to_vec());
        let saved: Vec<(String, Option<String>)> = assigns
            .iter()
            .map(|(k, v)| {
                let old = self.vars.insert(k.clone(), v.clone());
                (k.clone(), old)
            })
            .collect();

        // Push a fresh local scope so `local` declarations inside the body are
        // restored on return.
        self.local_frames.push(Vec::new());
        let flow = self.exec_program(&body, out, stdin);
        // The RETURN trap fires when the function returns, before its locals are
        // torn down (so the handler still sees the function's scope), matching
        // bash.
        if self.traps.contains_key("RETURN") {
            self.fire_trap("RETURN");
        }
        if let Some(frame) = self.local_frames.pop() {
            // Restore shadowed variables in reverse declaration order.
            for (name, snap) in frame.into_iter().rev() {
                self.restore_var(&name, snap);
            }
        }

        self.positional = saved_pos;
        for (k, old) in saved {
            match old {
                Some(v) => {
                    self.vars.insert(k, v);
                }
                None => {
                    self.vars.remove(&k);
                }
            }
        }
        match flow {
            Flow::Return | Flow::Next => Flow::Next,
            other => other,
        }
    }

    /// Capture the complete current state of a variable name (scalar / indexed /
    /// associative / export flag), for later restoration by `local`.
    fn snapshot_var(&self, name: &str) -> VarSnapshot {
        VarSnapshot {
            scalar: self.vars.get(name).cloned(),
            indexed: self.arrays.get(name).cloned(),
            assoc: self.assoc.get(name).cloned(),
            exported: self.exported.contains(name),
        }
    }

    /// Restore a variable to a previously captured [`VarSnapshot`], clearing any
    /// current binding first so a name that was unset before becomes unset again.
    fn restore_var(&mut self, name: &str, snap: VarSnapshot) {
        self.vars.remove(name);
        self.arrays.remove(name);
        self.assoc.remove(name);
        self.exported.remove(name);
        if let Some(v) = snap.scalar {
            self.vars.insert(name.to_string(), v);
        }
        if let Some(a) = snap.indexed {
            self.arrays.insert(name.to_string(), a);
        }
        if let Some(a) = snap.assoc {
            self.assoc.insert(name.to_string(), a);
        }
        if snap.exported {
            self.exported.insert(name.to_string());
        }
    }

    /// Mark `name` as function-local: snapshot its prior state into the current
    /// local frame (once per name) and clear the current binding so the `local`
    /// declaration starts fresh. Returns `false` if not inside a function.
    fn declare_local(&mut self, name: &str) -> bool {
        let Some(frame) = self.local_frames.last() else {
            return false;
        };
        if !frame.iter().any(|(n, _)| n == name) {
            let snap = self.snapshot_var(name);
            // `last_mut` is guaranteed present (we just checked `last`).
            if let Some(frame) = self.local_frames.last_mut() {
                frame.push((name.to_string(), snap));
            }
        }
        // Clear the current binding: a bare `local x` starts unset/empty.
        self.vars.remove(name);
        self.arrays.remove(name);
        self.assoc.remove(name);
        true
    }

    /// `command [-v|-V] [-p] name [args]` — run `name` bypassing shell
    /// functions (builtin if it is one, else external), or describe it with
    /// `-v` (terse: name/path) / `-V` (verbose).
    fn exec_command_builtin(
        &mut self,
        argv: &[String],
        assigns: &[(String, String)],
        out: &mut Out,
        stdin: &StdinSrc,
        redir: &RedirPlan,
    ) -> Flow {
        let mut i = 1;
        let mut terse = false;
        let mut verbose = false;
        while i < argv.len() && argv[i].starts_with('-') && argv[i].len() > 1 {
            match argv[i].as_str() {
                "-v" => terse = true,
                "-V" => verbose = true,
                "-p" => {} // "default PATH" — we use the current PATH.
                "--" => {
                    i += 1;
                    break;
                }
                other => {
                    self.errln(&format!("osh: command: {other}: invalid option"));
                    self.last_status = 2;
                    return Flow::Next;
                }
            }
            i += 1;
        }
        let rest = &argv[i..];
        let Some(target) = rest.first() else {
            self.last_status = 0;
            return Flow::Next;
        };
        if terse || verbose {
            return self.command_describe(target, verbose, out, redir);
        }
        // Run `target` bypassing functions.
        if is_builtin(target) {
            return self.run_builtin(target, rest, assigns, out, stdin, redir);
        }
        self.run_external(rest, assigns, out, stdin, redir);
        Flow::Next
    }

    /// `builtin name [args]` — run the shell builtin `name` even if a function
    /// of the same name exists.
    fn exec_builtin_builtin(
        &mut self,
        argv: &[String],
        assigns: &[(String, String)],
        out: &mut Out,
        stdin: &StdinSrc,
        redir: &RedirPlan,
    ) -> Flow {
        let Some(sub) = argv.get(1) else {
            self.last_status = 0;
            return Flow::Next;
        };
        if is_builtin(sub) {
            let sub = sub.clone();
            return self.run_builtin(&sub, &argv[1..], assigns, out, stdin, redir);
        }
        self.errln(&format!("osh: builtin: {sub}: not a shell builtin"));
        self.last_status = 1;
        Flow::Next
    }

    /// Implement `command -v`/`-V`: report how `target` would be resolved.
    fn command_describe(
        &mut self,
        target: &str,
        verbose: bool,
        out: &mut Out,
        redir: &RedirPlan,
    ) -> Flow {
        if self.funcs.contains_key(target) {
            let line = if verbose {
                format!("{target} is a function")
            } else {
                target.to_string()
            };
            let _ = self.write_line(out, redir, &line);
            self.last_status = 0;
        } else if is_builtin(target) {
            let line = if verbose {
                format!("{target} is a shell builtin")
            } else {
                target.to_string()
            };
            let _ = self.write_line(out, redir, &line);
            self.last_status = 0;
        } else if let Some(path) = self.find_in_path(target) {
            let ps = path.to_string_lossy().into_owned();
            let line = if verbose {
                format!("{target} is {ps}")
            } else {
                ps
            };
            let _ = self.write_line(out, redir, &line);
            self.last_status = 0;
        } else {
            if verbose {
                self.errln(&format!("osh: command: {target}: not found"));
            }
            self.last_status = 1;
        }
        Flow::Next
    }

    /// Search `$PATH` for an executable named `name`. A name containing a slash
    /// is checked directly. Returns the first matching regular file.
    fn find_in_path(&self, name: &str) -> Option<std::path::PathBuf> {
        use std::path::Path;
        if name.contains('/') || name.contains('\\') {
            let p = Path::new(name);
            return p.is_file().then(|| p.to_path_buf());
        }
        let path = self
            .param_value("PATH")
            .or_else(|| std::env::var("PATH").ok())?;
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
            // Host convenience: try common Windows executable extensions.
            #[cfg(windows)]
            for ext in ["exe", "cmd", "bat"] {
                let c = cand.with_extension(ext);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
        None
    }

    /// Like `find_in_path`, but returns *every* matching executable across all
    /// `$PATH` directories in order (used by `type -a`). Duplicate paths are
    /// suppressed while preserving first-seen order.
    fn find_all_in_path(&self, name: &str) -> Vec<std::path::PathBuf> {
        use std::path::Path;
        let mut out: Vec<std::path::PathBuf> = Vec::new();
        if name.contains('/') || name.contains('\\') {
            let p = Path::new(name);
            if p.is_file() {
                out.push(p.to_path_buf());
            }
            return out;
        }
        let Some(path) = self
            .param_value("PATH")
            .or_else(|| std::env::var("PATH").ok())
        else {
            return out;
        };
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(name);
            if cand.is_file() && !out.contains(&cand) {
                out.push(cand.clone());
            }
            #[cfg(windows)]
            for ext in ["exe", "cmd", "bat"] {
                let c = cand.with_extension(ext);
                if c.is_file() && !out.contains(&c) {
                    out.push(c);
                }
            }
        }
        out
    }

    fn run_external(
        &mut self,
        argv: &[String],
        assigns: &[(String, String)],
        out: &mut Out,
        stdin: &StdinSrc,
        redir: &RedirPlan,
    ) {
        let mut cmd = PCommand::new(&argv[0]);
        cmd.args(&argv[1..]);

        // Environment: exported shell vars + this command's temp assignments.
        for (k, v) in &self.vars {
            if self.exported.contains(k) {
                cmd.env(k, v);
            }
        }
        for (k, v) in assigns {
            cmd.env(k, v);
        }

        // stdin — a here-doc/here-string body takes precedence, then a file
        // redirect, then the inherited pipeline input.
        let mut input_bytes: Option<Vec<u8>> = None;
        if let Some(data) = &redir.stdin_data {
            input_bytes = Some(data.clone());
            cmd.stdin(Stdio::piped());
        } else {
            match &redir.stdin {
                Some(path) => match std::fs::File::open(path) {
                    Ok(f) => {
                        cmd.stdin(Stdio::from(f));
                    }
                    Err(e) => {
                        self.errln(&format!("osh: {path}: {e}"));
                        self.last_status = 1;
                        return;
                    }
                },
                None => match stdin {
                    StdinSrc::Inherit => {
                        cmd.stdin(Stdio::inherit());
                    }
                    StdinSrc::Cursor(c) => {
                        // Feed the external the cursor's remaining bytes (from the
                        // current position to the end), advancing the cursor.
                        let mut rest = Vec::new();
                        let _ = c.borrow_mut().read_to_end(&mut rest);
                        input_bytes = Some(rest);
                        cmd.stdin(Stdio::piped());
                    }
                    StdinSrc::Pipe(r) => {
                        // Hand the child a live clone of the upstream pipe read
                        // end so it streams (buffering would deadlock an
                        // unbounded producer like `yes`). Bytes already buffered
                        // by an earlier in-stage `read` are not replayed — a rare
                        // edge case (mixing `read` and an external in one stage).
                        match r.borrow().get_ref().try_clone() {
                            Ok(rp) => {
                                cmd.stdin(Stdio::from(rp));
                            }
                            Err(e) => {
                                self.errln(&format!("osh: pipe: {e}"));
                                self.last_status = 1;
                                return;
                            }
                        }
                    }
                },
            }
        }

        // stdout
        let capturing = matches!(out, Out::Capture(_)) && redir.stdout.is_none();
        match &redir.stdout {
            Some((path, append)) => match open_out(path, *append) {
                Ok(f) => {
                    cmd.stdout(Stdio::from(f));
                }
                Err(e) => {
                    self.errln(&format!("osh: {path}: {e}"));
                    self.last_status = 1;
                    return;
                }
            },
            None => {
                if redir.stdout_to_stderr {
                    // `1>&2` and fd 2 is not a file: send fd 1 to the current
                    // stderr sink (an enclosing compound `2>` redirect, or the
                    // shell's real stderr).
                    match self.child_stdio_for_stderr() {
                        Ok(s) => {
                            cmd.stdout(s);
                        }
                        Err(e) => {
                            self.errln(&format!("osh: {e}"));
                            self.last_status = 1;
                            return;
                        }
                    }
                } else if capturing {
                    cmd.stdout(Stdio::piped());
                } else if let Out::Pipe(w) = out {
                    // Stream the child's stdout straight into the downstream pipe
                    // (a clone; the parent stage keeps its own writer, which is
                    // fine — `SIGPIPE`/EOF key on the read end, not extra writers).
                    match w.try_clone() {
                        Ok(wp) => {
                            cmd.stdout(Stdio::from(wp));
                        }
                        Err(e) => {
                            self.errln(&format!("osh: pipe: {e}"));
                            self.last_status = 1;
                            return;
                        }
                    }
                } else {
                    cmd.stdout(Stdio::inherit());
                }
            }
        }

        // stderr routing precedence:
        //   1. an explicit per-command `2> file` / `2>> file`
        //   2. `2>&1` (`stderr_to_stdout`) — follow fd 1's live sink
        //   3. an enclosing compound command's stderr redirect (`stderr_stack`)
        //   4. otherwise inherit the shell's real stderr
        // When fd 2 must be captured into a buffer we pipe it and drain the
        // child's stderr after spawn (`stderr_capture`).
        let mut stderr_capture: Option<Arc<Mutex<Vec<u8>>>> = None;
        // For `2>&1` with a captured stdout, fd 2 is appended to the same
        // capture buffer as fd 1.
        let mut stderr_to_stdout_capture = false;
        if let Some((path, append)) = &redir.stderr {
            match open_out(path, *append) {
                Ok(f) => {
                    cmd.stderr(Stdio::from(f));
                }
                Err(e) => {
                    self.errln(&format!("osh: {path}: {e}"));
                    self.last_status = 1;
                    return;
                }
            }
        } else if redir.stderr_to_stdout {
            // `2>&1` and fd 1 is not a file (else the file target was copied
            // into `redir.stderr` already): mirror fd 1's live sink.
            if capturing {
                cmd.stderr(Stdio::piped());
                stderr_to_stdout_capture = true;
            } else if let Out::Pipe(w) = out {
                match w.try_clone() {
                    Ok(wp) => {
                        cmd.stderr(Stdio::from(wp));
                    }
                    Err(e) => {
                        self.errln(&format!("osh: pipe: {e}"));
                        self.last_status = 1;
                        return;
                    }
                }
            } else {
                cmd.stderr(Stdio::inherit());
            }
        } else {
            match self.stderr_stack.last() {
                None => {}
                Some(StderrTarget::File(f)) => match f.try_clone() {
                    Ok(fc) => {
                        cmd.stderr(Stdio::from(fc));
                    }
                    Err(e) => {
                        self.errln(&format!("osh: stderr: {e}"));
                        self.last_status = 1;
                        return;
                    }
                },
                Some(StderrTarget::Pipe(p)) => match p.try_clone() {
                    Ok(pc) => {
                        cmd.stderr(Stdio::from(pc));
                    }
                    Err(e) => {
                        self.errln(&format!("osh: pipe: {e}"));
                        self.last_status = 1;
                        return;
                    }
                },
                Some(StderrTarget::Buffer(b)) => {
                    cmd.stderr(Stdio::piped());
                    stderr_capture = Some(Arc::clone(b));
                }
                // Real fd 1: inherit (fd 2 → terminal, same visual result at the
                // shell's controlling terminal).
                Some(StderrTarget::Stdout) => {
                    cmd.stderr(Stdio::inherit());
                }
            }
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    self.errln(&format!("osh: {}: command not found", argv[0]));
                    self.last_status = 127;
                } else {
                    self.errln(&format!("osh: {}: {e}", argv[0]));
                    self.last_status = 126;
                }
                return;
            }
        };

        if let Some(bytes) = input_bytes
            && let Some(mut si) = child.stdin.take()
        {
            let _ = si.write_all(&bytes); // child may exit early; ignore EPIPE
        }

        // Drain a piped stderr into its capture buffer before waiting, so a child
        // that fills the stderr pipe cannot deadlock on a full buffer.
        if let Some(buf) = &stderr_capture
            && let Some(mut se) = child.stderr.take()
        {
            let mut captured = Vec::new();
            let _ = se.read_to_end(&mut captured);
            if let Ok(mut g) = buf.lock() {
                g.extend_from_slice(&captured);
            }
        }

        if capturing {
            let mut captured = Vec::new();
            if let Some(mut so) = child.stdout.take() {
                let _ = so.read_to_end(&mut captured);
            }
            // `2>&1` into a capture: fold fd 2 into the same buffer after fd 1.
            if stderr_to_stdout_capture
                && let Some(mut se) = child.stderr.take()
            {
                let _ = se.read_to_end(&mut captured);
            }
            if let Out::Capture(buf) = out {
                buf.extend_from_slice(&captured);
            }
        }

        match child.wait() {
            Ok(status) => {
                self.last_status = status.code().unwrap_or(1);
            }
            Err(e) => {
                self.errln(&format!("osh: wait failed: {e}"));
                self.last_status = 1;
            }
        }
    }

    fn exec_background(&mut self, ao: &AndOr) {
        // Only handle the common case: a single external simple command.
        if ao.rest.is_empty()
            && ao.first.commands.len() == 1
            && !ao.first.negated
            && let Command::Simple(sc) = &ao.first.commands[0]
        {
            let mut argv = Vec::new();
            for w in &sc.words {
                argv.extend(self.expand_word(w, true));
            }
            if !argv.is_empty() && !self.funcs.contains_key(&argv[0]) && !is_builtin(&argv[0]) {
                let mut cmd = PCommand::new(&argv[0]);
                cmd.args(&argv[1..]);
                for (k, v) in &self.vars {
                    if self.exported.contains(k) {
                        cmd.env(k, v);
                    }
                }
                match cmd.spawn() {
                    Ok(child) => {
                        let pid = child.id();
                        // Bash assigns the lowest job number not currently in
                        // use, so numbering restarts at 1 once the table empties.
                        let mut id = 1;
                        while self.jobs.iter().any(|j| j.id == id) {
                            id += 1;
                        }
                        self.jobs.push(Job {
                            id,
                            pid,
                            child: Some(child),
                            cmd: argv.join(" "),
                            status: None,
                        });
                        self.last_bg_pid = Some(pid);
                        self.last_status = 0;
                        return;
                    }
                    Err(e) => {
                        eprintln!("osh: {}: {e}", argv[0]);
                        self.last_status = 1;
                        return;
                    }
                }
            }
        }
        // Fallback: run synchronously.
        let mut out = Out::Inherit;
        let _ = self.exec_and_or(ao, &mut out, &StdinSrc::Inherit);
    }

    // ---- redirection resolution ---------------------------------------------

    fn resolve_redirects(&mut self, redirs: &[Redirect]) -> Result<RedirPlan, String> {
        let mut plan = RedirPlan::default();
        for r in redirs {
            match r.op {
                RedirectOp::Read => {
                    if r.fd == 0 {
                        plan.stdin = Some(self.expand_to_string(&r.target));
                        plan.stdin_data = None;
                    }
                }
                RedirectOp::HereDoc => {
                    if r.fd == 0 {
                        // Here-doc bodies expand like a double-quoted context:
                        // no tilde expansion, no field splitting, no globbing.
                        let body = self.expand_double_quoted(&r.target.parts);
                        plan.stdin = None;
                        plan.stdin_data = Some(body.into_bytes());
                    }
                }
                RedirectOp::HereStr => {
                    if r.fd == 0 {
                        let mut s = self.expand_to_string(&r.target);
                        s.push('\n');
                        plan.stdin = None;
                        plan.stdin_data = Some(s.into_bytes());
                    }
                }
                RedirectOp::Write | RedirectOp::Append => {
                    let target = self.expand_to_string(&r.target);
                    let append = matches!(r.op, RedirectOp::Append);
                    match r.fd {
                        2 => plan.stderr = Some((target, append)),
                        _ => plan.stdout = Some((target, append)),
                    }
                }
                RedirectOp::DupOut => {
                    // `2>&1` → stderr follows stdout; `1>&2` → the reverse.
                    // When the followed fd already targets a file, copy that file
                    // target directly; otherwise flag the dup so the executor
                    // routes fd 2→fd 1 (or fd 1→fd 2) to the live sink (pipe,
                    // terminal, or capture), not just to a file path.
                    let target = self.expand_to_string(&r.target);
                    if r.fd == 2 && target == "1" {
                        if plan.stdout.is_some() {
                            plan.stderr = plan.stdout.clone();
                        } else {
                            plan.stderr_to_stdout = true;
                        }
                    } else if r.fd == 1 && target == "2" {
                        if plan.stderr.is_some() {
                            plan.stdout = plan.stderr.clone();
                        } else {
                            plan.stdout_to_stderr = true;
                        }
                    }
                }
            }
        }
        Ok(plan)
    }

    // ---- expansion ----------------------------------------------------------

    /// Expand a word, optionally field-splitting the results of unquoted
    /// expansions. Returns zero or more fields.
    fn expand_word(&mut self, word: &Word, split: bool) -> Vec<String> {
        if split {
            // Command-argument context: field-split unquoted expansions, then
            // apply pathname (glob) expansion to each resulting field.
            let fields = self.expand_word_annotated(word);
            let nullglob = self.shopt.get("nullglob").copied().unwrap_or(false);
            let dotglob = self.shopt.get("dotglob").copied().unwrap_or(false);
            let nocaseglob = self.shopt.get("nocaseglob").copied().unwrap_or(false);
            let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
            let globstar = self.shopt.get("globstar").copied().unwrap_or(false);
            let mut out = Vec::new();
            for f in fields {
                glob_or_literal(&f, &mut out, nullglob, dotglob, nocaseglob, extglob, globstar);
            }
            return out;
        }
        // Non-splitting context (assignment values, redirect targets, `[[ ]]`
        // operands): concatenate everything into one field, no splitting/glob.
        let mut cur = String::new();
        let mut started = false;
        for (idx, part) in word.parts.iter().enumerate() {
            match part {
                WordPart::Literal(s) => {
                    let s = if idx == 0 {
                        self.tilde_expand(s)
                    } else {
                        s.clone()
                    };
                    cur.push_str(&s);
                    started = true;
                }
                WordPart::SingleQuoted(s) => {
                    cur.push_str(s);
                    started = true;
                }
                WordPart::DoubleQuoted(parts) => {
                    cur.push_str(&self.expand_double_quoted(parts));
                    started = true;
                }
                other => {
                    cur.push_str(&self.expand_dynamic(other));
                    started = true;
                }
            }
        }
        if started {
            vec![cur]
        } else {
            Vec::new()
        }
    }

    /// Expand a word into fields of quote-annotated characters (splitting
    /// unquoted expansions on IFS). The quoting flag lets a later glob step
    /// treat quoted metacharacters as literals.
    fn expand_word_annotated(&mut self, word: &Word) -> Vec<Vec<EChar>> {
        let mut fields: Vec<Vec<EChar>> = Vec::new();
        let mut cur: Vec<EChar> = Vec::new();
        let mut started = false;
        for (idx, part) in word.parts.iter().enumerate() {
            match part {
                WordPart::Literal(s) => {
                    let s = if idx == 0 {
                        self.tilde_expand(s)
                    } else {
                        s.clone()
                    };
                    push_chars(&mut cur, &s, false);
                    started = true;
                }
                WordPart::SingleQuoted(s) => {
                    push_chars(&mut cur, s, true);
                    started = true;
                }
                WordPart::DoubleQuoted(parts) => {
                    // `"${arr[@]}"` (and `"$@"`) expand to one field per element,
                    // preserving embedded whitespace; empty arrays yield no field.
                    // `"${!arr[@]}"` does the same over the keys/indices.
                    let per_element: Option<Vec<String>> = match parts.as_slice() {
                        [
                            WordPart::ArrayRef {
                                name,
                                index: ArrayIndex::All,
                                length: false,
                            },
                        ] => Some(self.array_elements(name)),
                        [WordPart::ArrayKeys { name, star: false }] => Some(self.array_keys(name)),
                        [WordPart::VarNames { prefix, star: false }] => {
                            Some(self.var_names_with_prefix(prefix))
                        }
                        _ => None,
                    };
                    if let Some(items) = per_element {
                        for (i, el) in items.into_iter().enumerate() {
                            if i > 0 {
                                fields.push(std::mem::take(&mut cur));
                            }
                            push_chars(&mut cur, &el, true);
                            started = true;
                        }
                    } else {
                        let s = self.expand_double_quoted(parts);
                        push_chars(&mut cur, &s, true);
                        started = true;
                    }
                }
                other => {
                    let val = self.expand_dynamic(other);
                    let pieces = split_ifs(&val);
                    if !pieces.is_empty() {
                        push_chars(&mut cur, &pieces[0], false);
                        started = true;
                        for extra in &pieces[1..] {
                            fields.push(std::mem::take(&mut cur));
                            push_chars(&mut cur, extra, false);
                        }
                    }
                }
            }
        }
        if started {
            fields.push(cur);
        }
        fields
    }

    /// Expand a word to a single string (no field splitting) — used for
    /// assignment values and redirection targets.
    fn expand_to_string(&mut self, word: &Word) -> String {
        let fields = self.expand_word(word, false);
        fields.join("")
    }

    fn expand_double_quoted(&mut self, parts: &[WordPart]) -> String {
        let mut s = String::new();
        for part in parts {
            match part {
                WordPart::Literal(t) | WordPart::SingleQuoted(t) => s.push_str(t),
                other => s.push_str(&self.expand_dynamic(other)),
            }
        }
        s
    }

    /// Expand a dynamic word part (parameter/command/arithmetic) to a string.
    fn expand_dynamic(&mut self, part: &WordPart) -> String {
        match part {
            WordPart::Param(name) => match self.param_value(name) {
                Some(v) => v,
                None => {
                    self.note_unbound(name);
                    String::new()
                }
            },
            WordPart::Length(name) => match self.param_value(name) {
                Some(v) => v.chars().count().to_string(),
                None => {
                    self.note_unbound(name);
                    "0".to_string()
                }
            },
            WordPart::ParamOp {
                name,
                index,
                op,
                arg,
            } => self.expand_param_op(name, index, *op, arg),
            WordPart::ParamTrim {
                name,
                index,
                suffix,
                longest,
                pattern,
            } => {
                let value = self.param_elem_value(name, index).unwrap_or_default();
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
                param_trim(&value, &pat, *suffix, *longest, extglob)
            }
            WordPart::ParamSubstr {
                name,
                index,
                offset,
                length,
            } => {
                let value = self.param_elem_value(name, index).unwrap_or_default();
                let off = self.eval_arith_word(offset);
                let len = length.as_ref().map(|l| self.eval_arith_word(l));
                param_substr(&value, off, len)
            }
            WordPart::ParamReplace {
                name,
                index,
                all,
                anchor,
                pattern,
                replacement,
            } => {
                let value = self.param_elem_value(name, index).unwrap_or_default();
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                let repl = self.expand_to_string(replacement);
                let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
                param_replace(&value, &pat, &repl, *all, *anchor, extglob)
            }
            WordPart::ParamCase {
                name,
                index,
                upper,
                all,
                pattern,
            } => {
                let value = self.param_elem_value(name, index).unwrap_or_default();
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
                param_case(&value, &pat, *upper, *all, extglob)
            }
            WordPart::ParamTransform { name, index, op } => {
                self.param_transform(name, index, *op)
            }
            WordPart::CommandSub(prog) => self.command_sub(prog),
            WordPart::ArithSub(expr) => self.arith_sub(expr),
            WordPart::ArrayRef {
                name,
                index,
                length,
            } => self.expand_array_ref(name, index, *length),
            WordPart::ArrayKeys { name, .. } => self.array_keys(name).join(" "),
            WordPart::Indirect(refname) => self.expand_indirect(refname),
            WordPart::VarNames { prefix, .. } => self.var_names_with_prefix(prefix).join(" "),
            // Literal/quoted handled by callers.
            WordPart::Literal(s) | WordPart::SingleQuoted(s) => s.clone(),
            WordPart::DoubleQuoted(parts) => self.expand_double_quoted(parts),
        }
    }

    fn expand_param_op(
        &mut self,
        name: &str,
        index: &Option<Box<Word>>,
        op: ParamOp,
        arg: &Word,
    ) -> String {
        let cur = self.param_elem_value(name, index);
        let is_set_nonempty = cur.as_ref().is_some_and(|v| !v.is_empty());
        match op {
            ParamOp::UseDefault => {
                if is_set_nonempty {
                    cur.unwrap_or_default()
                } else {
                    self.expand_to_string(arg)
                }
            }
            ParamOp::AssignDefault => {
                if is_set_nonempty {
                    cur.unwrap_or_default()
                } else {
                    let v = self.expand_to_string(arg);
                    self.assign_elem(name, index, v.clone());
                    v
                }
            }
            ParamOp::UseAlternate => {
                if is_set_nonempty {
                    self.expand_to_string(arg)
                } else {
                    String::new()
                }
            }
            ParamOp::ErrorIfUnset => {
                if is_set_nonempty {
                    cur.unwrap_or_default()
                } else {
                    let msg = self.expand_to_string(arg);
                    eprintln!(
                        "osh: {name}: {}",
                        if msg.is_empty() {
                            "parameter null or not set"
                        } else {
                            &msg
                        }
                    );
                    String::new()
                }
            }
        }
    }

    /// Advance the `$RANDOM` generator and return a value in `0..=32767`
    /// (matching bash's 15-bit range). Uses a classic LCG; `param_value` reads
    /// through `&self`, so the state lives behind a `Cell`.
    fn next_random(&self) -> u32 {
        // Numerical Recipes LCG constants.
        let next = self.rng.get().wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.rng.set(next);
        (next >> 16) & 0x7fff
    }

    /// Record a reference to an unset parameter. Under `set -u` (nounset) this
    /// flags an error (checked by the simple-command driver, which aborts) and
    /// prints a diagnostic; special parameters (`$@`, `$*`, `$?`, `$!`, etc.)
    /// are always considered set and never trigger it.
    fn note_unbound(&mut self, name: &str) {
        if self.nounset && !is_special_param(name) {
            self.unbound_error = true;
            self.emit_stderr(format!("osh: {name}: unbound variable\n").as_bytes());
        }
    }

    /// Whether a variable is "set" for `-v` / `test -v`. Accepts a plain scalar
    /// name, an array/associative name (set if the array exists), or an explicit
    /// element reference `name[subscript]` (set if that element exists). Special
    /// parameters (`$?`, `$#`, positional `$1`, …) count as set when they have a
    /// value.
    fn var_is_set(&self, name: &str) -> bool {
        // Explicit element reference `name[subscript]`.
        if let Some(open) = name.find('[')
            && name.ends_with(']')
        {
            let base = &name[..open];
            let sub = &name[open + 1..name.len() - 1];
            if let Some(map) = self.assoc.get(base) {
                return map.iter().any(|(k, _)| k == sub);
            }
            if let Some(arr) = self.arrays.get(base) {
                // `[@]`/`[*]` — set if the array has any element.
                if sub == "@" || sub == "*" {
                    return !arr.is_empty();
                }
                if let Ok(idx) = sub.parse::<usize>() {
                    return arr.contains_key(&idx);
                }
            }
            return false;
        }
        if self.vars.contains_key(name)
            || self.arrays.contains_key(name)
            || self.assoc.contains_key(name)
        {
            return true;
        }
        // Special/positional parameters: set iff they resolve to a value.
        self.param_value(name).is_some()
    }

    /// Resolve a parameter's value; `None` means unset.
    fn param_value(&self, name: &str) -> Option<String> {
        match name {
            "?" => Some(self.last_status.to_string()),
            "#" => Some(self.positional.len().to_string()),
            "$" => Some(self.pid.to_string()),
            "!" => self.last_bg_pid.map(|p| p.to_string()),
            "@" | "*" => Some(self.positional.join(" ")),
            "0" => Some(self.name.clone()),
            "-" => Some(String::new()),
            "BASHPID" => Some(self.pid.to_string()),
            "RANDOM" => Some(self.next_random().to_string()),
            "SECONDS" => Some(
                self.seconds_base
                    .saturating_add(self.seconds_anchor.elapsed().as_secs())
                    .to_string(),
            ),
            "EPOCHSECONDS" => Some(unix_time().0.to_string()),
            "EPOCHREALTIME" => {
                let (secs, micros) = unix_time();
                Some(format!("{secs}.{micros:06}"))
            }
            _ => {
                if let Ok(n) = name.parse::<usize>() {
                    if n == 0 {
                        return Some(self.name.clone());
                    }
                    return self.positional.get(n - 1).cloned();
                }
                // A plain array reference (`$arr` / `${arr}`) reads element 0
                // (indexed) or the value at key "0" (associative).
                if let Some(m) = self.assoc.get(name) {
                    return m.iter().find(|(k, _)| k == "0").map(|(_, v)| v.clone());
                }
                if let Some(arr) = self.arrays.get(name) {
                    return arr.get(&0).cloned();
                }
                self.vars
                    .get(name)
                    .cloned()
                    .or_else(|| std::env::var(name).ok())
            }
        }
    }

    fn command_sub(&mut self, prog: &Program) -> String {
        let mut buf = Vec::new();
        {
            let mut out = Out::Capture(&mut buf);
            let _ = self.exec_program(prog, &mut out, &StdinSrc::Inherit);
        }
        let mut s = String::from_utf8_lossy(&buf).into_owned();
        // Strip trailing newlines, as command substitution does.
        while s.ends_with('\n') {
            s.pop();
        }
        s
    }

    /// Expand a word to a string and evaluate it as an arithmetic expression
    /// (used for `${name:offset:length}`). Errors/empties yield `0`.
    fn eval_arith_word(&mut self, w: &Word) -> i64 {
        let s = self.expand_to_string(w);
        let s = s.trim();
        if s.is_empty() {
            return 0;
        }
        arith::eval(s, self).unwrap_or(0)
    }

    fn arith_sub(&mut self, expr: &str) -> String {
        // Expand `$name` / `${name}` parameters inside the expression first;
        // bare identifiers are resolved by the evaluator via `VarLookup`.
        let expanded = self.expand_arith_params(expr);
        match arith::eval(&expanded, self) {
            Ok(v) => v.to_string(),
            Err(e) => {
                eprintln!("osh: arithmetic: {e}");
                "0".to_string()
            }
        }
    }

    /// Replace `$name`, `${name}`, and `$1` inside an arithmetic string with
    /// the parameter's (numeric) value.
    fn expand_arith_params(&self, expr: &str) -> String {
        let chars: Vec<char> = expr.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '$' {
                i += 1;
                let name = if chars.get(i) == Some(&'{') {
                    i += 1;
                    let mut n = String::new();
                    while i < chars.len() && chars[i] != '}' {
                        n.push(chars[i]);
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1; // consume '}'
                    }
                    n
                } else {
                    let mut n = String::new();
                    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                        n.push(chars[i]);
                        i += 1;
                    }
                    n
                };
                let val = self.param_value(&name).unwrap_or_default();
                let val = val.trim();
                out.push_str(if val.is_empty() { "0" } else { val });
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    fn tilde_expand(&self, s: &str) -> String {
        if s == "~" {
            return self.param_value("HOME").unwrap_or_else(|| "~".to_string());
        }
        if let Some(rest) = s.strip_prefix("~/")
            && let Some(home) = self.param_value("HOME")
        {
            return format!("{home}/{rest}");
        }
        s.to_string()
    }

    // ---- builtins -----------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn run_builtin(
        &mut self,
        name: &str,
        argv: &[String],
        assigns: &[(String, String)],
        out: &mut Out,
        stdin: &StdinSrc,
        redir: &RedirPlan,
    ) -> Flow {
        // Apply temporary assignments for the duration of the builtin.
        let saved: Vec<(String, Option<String>)> = assigns
            .iter()
            .map(|(k, v)| (k.clone(), self.vars.insert(k.clone(), v.clone())))
            .collect();

        let mut flow = Flow::Next;
        let args = &argv[1..];
        let status = match name {
            ":" | "true" => 0,
            "false" => 1,
            "cd" => self.builtin_cd(args),
            "pwd" => self.builtin_pwd(out, redir),
            "pushd" => self.builtin_pushd(args, out, redir),
            "popd" => self.builtin_popd(args, out, redir),
            "dirs" => self.builtin_dirs(args, out, redir),
            "echo" => self.builtin_echo(args, out, redir),
            "printf" => self.builtin_printf(args, out, redir),
            "export" => self.builtin_export(args),
            "declare" | "typeset" => {
                // `declare -p [names]` prints definitions instead of declaring.
                if args.iter().take_while(|a| a.starts_with('-')).any(|a| a.contains('p')) {
                    self.declare_print(args, out, redir)
                } else {
                    self.builtin_declare(args, false)
                }
            }
            "local" => self.builtin_declare(args, true),
            "readonly" => self.builtin_readonly(args, out, redir),
            "shopt" => self.builtin_shopt(args, out, redir),
            "unset" => self.builtin_unset(args),
            "set" => self.builtin_set(args),
            "shift" => self.builtin_shift(args),
            "getopts" => self.builtin_getopts(args),
            "mapfile" | "readarray" => self.builtin_mapfile(args, stdin, redir),
            "read" => self.builtin_read(args, stdin, redir),
            "test" | "[" => self.builtin_test(name, args),
            "let" => self.builtin_let(args),
            "eval" => {
                let joined = args.join(" ");
                self.run_source(&joined)
            }
            "source" | "." => self.builtin_source(args),
            "type" => self.builtin_type(args, out, redir),
            "trap" => self.builtin_trap(args, out, redir),
            "jobs" => self.builtin_jobs(args, out, redir),
            "wait" => self.builtin_wait(args),
            "exec" => {
                if args.is_empty() {
                    // `exec` with no command applies its redirections to the
                    // shell persistently; we don't yet support rebinding the
                    // shell's own fds (documented, TD-OILS14). Treat as success.
                    0
                } else {
                    // Replace the shell with the command: run it with the current
                    // environment/redirections, then exit with its status. A true
                    // in-place `execve` that preserves the pid awaits kernel
                    // support; observationally the shell does not continue past
                    // `exec` (a following command never runs).
                    self.run_external(args, assigns, out, stdin, redir);
                    let code = self.last_status;
                    flow = Flow::Exit(code);
                    code
                }
            }
            "exit" => {
                let code = args.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(self.last_status);
                flow = Flow::Exit(code);
                code
            }
            "return" => {
                let code = args.first().and_then(|s| s.parse::<i32>().ok()).unwrap_or(self.last_status);
                flow = Flow::Return;
                code
            }
            "break" => {
                let n = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                flow = Flow::Break(n.max(1));
                0
            }
            "continue" => {
                let n = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                flow = Flow::Continue(n.max(1));
                0
            }
            _ => {
                eprintln!("osh: {name}: not a builtin");
                127
            }
        };

        // Restore temporary assignments (builtins don't persist them, except
        // pure-assignment which never reaches here).
        for (k, old) in saved {
            match old {
                Some(v) => {
                    self.vars.insert(k, v);
                }
                None => {
                    self.vars.remove(&k);
                }
            }
        }

        self.last_status = status;
        flow
    }

    /// Change the process working directory to `path`, updating `$OLDPWD`
    /// (to the previous cwd) and `$PWD` (to the new cwd). Returns the new cwd
    /// as a display string on success, or an OS error string on failure.
    fn change_dir(&mut self, path: &str) -> Result<String, String> {
        let old = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .ok();
        std::env::set_current_dir(path).map_err(|e| e.to_string())?;
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        if let Some(o) = old {
            self.vars.insert("OLDPWD".to_string(), o);
        }
        self.vars.insert("PWD".to_string(), cwd.clone());
        Ok(cwd)
    }

    fn builtin_cd(&mut self, args: &[String]) -> i32 {
        // `cd -` returns to `$OLDPWD` and echoes the new directory (bash).
        let (target, echo) = match args.first().map(String::as_str) {
            None => (
                self.param_value("HOME").unwrap_or_else(|| "/".to_string()),
                false,
            ),
            Some("-") => match self.param_value("OLDPWD") {
                Some(p) => (p, true),
                None => {
                    self.emit_stderr(b"osh: cd: OLDPWD not set\n");
                    return 1;
                }
            },
            Some(p) => (p.to_string(), false),
        };
        match self.change_dir(&target) {
            Ok(cwd) => {
                if echo {
                    println!("{cwd}");
                }
                0
            }
            Err(e) => {
                eprintln!("osh: cd: {target}: {e}");
                1
            }
        }
    }

    /// Render a directory path for `dirs`/`pushd`/`popd` output: unless `long`,
    /// contract a leading `$HOME` to `~` (bash's default short form).
    fn dirs_render(&self, path: &str, long: bool) -> String {
        if long {
            return path.to_string();
        }
        if let Some(home) = self.param_value("HOME") {
            if !home.is_empty() && path == home {
                return "~".to_string();
            }
            if let Some(rest) = path.strip_prefix(&format!("{home}/")) {
                return format!("~/{rest}");
            }
        }
        path.to_string()
    }

    /// The current directory stack as a list with the current directory first
    /// (the conceptual top), followed by the saved `dir_stack` entries.
    fn dir_stack_full(&self) -> Vec<String> {
        let cur = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut full = Vec::with_capacity(self.dir_stack.len() + 1);
        full.push(cur);
        full.extend(self.dir_stack.iter().cloned());
        full
    }

    /// Print the directory stack in the default single-line form (used after a
    /// successful `pushd`/`popd`).
    fn print_dirs_line(&mut self, out: &mut Out, redir: &RedirPlan) -> i32 {
        let line = self
            .dir_stack_full()
            .iter()
            .map(|p| self.dirs_render(p, false))
            .collect::<Vec<_>>()
            .join(" ");
        self.write_line(out, redir, &line)
    }

    /// `pushd [dir | +N | -N]` — push onto the directory stack and change to the
    /// new top. With no argument, swap the top two entries. `+N`/`-N` rotate the
    /// stack so the N-th entry (from the left / right) becomes current.
    fn builtin_pushd(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let is_rot = |s: &str| {
            s.len() > 1
                && (s.starts_with('+') || s.starts_with('-'))
                && s[1..].chars().all(|c| c.is_ascii_digit())
        };
        let cur = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        match args.first().map(String::as_str) {
            None => {
                if self.dir_stack.is_empty() {
                    self.emit_stderr(b"osh: pushd: no other directory\n");
                    return 1;
                }
                let top = self.dir_stack[0].clone();
                match self.change_dir(&top) {
                    Ok(_) => self.dir_stack[0] = cur,
                    Err(e) => {
                        self.emit_stderr(format!("osh: pushd: {top}: {e}\n").as_bytes());
                        return 1;
                    }
                }
            }
            Some(spec) if is_rot(spec) => {
                let full = self.dir_stack_full();
                let len = full.len();
                let n: usize = spec[1..].parse().unwrap_or(0);
                if n >= len {
                    self.emit_stderr(b"osh: pushd: directory stack index out of range\n");
                    return 1;
                }
                let idx = if spec.starts_with('+') { n } else { len - 1 - n };
                let mut rotated: Vec<String> = full[idx..].to_vec();
                rotated.extend_from_slice(&full[..idx]);
                let newtop = rotated[0].clone();
                match self.change_dir(&newtop) {
                    Ok(_) => self.dir_stack = rotated[1..].to_vec(),
                    Err(e) => {
                        self.emit_stderr(format!("osh: pushd: {newtop}: {e}\n").as_bytes());
                        return 1;
                    }
                }
            }
            Some(dir) => match self.change_dir(dir) {
                Ok(_) => self.dir_stack.insert(0, cur),
                Err(e) => {
                    self.emit_stderr(format!("osh: pushd: {dir}: {e}\n").as_bytes());
                    return 1;
                }
            },
        }
        self.print_dirs_line(out, redir)
    }

    /// `popd [+N | -N]` — pop the top of the directory stack and change to it.
    /// `+N`/`-N` remove the N-th entry (from the left / right) instead; removing
    /// the current entry (index 0) behaves like a plain `popd`.
    fn builtin_popd(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let is_rot = |s: &str| {
            s.len() > 1
                && (s.starts_with('+') || s.starts_with('-'))
                && s[1..].chars().all(|c| c.is_ascii_digit())
        };
        match args.first().map(String::as_str) {
            Some(spec) if is_rot(spec) => {
                let len = self.dir_stack.len() + 1; // current + saved
                let n: usize = spec[1..].parse().unwrap_or(0);
                if n >= len {
                    self.emit_stderr(b"osh: popd: directory stack index out of range\n");
                    return 1;
                }
                let idx = if spec.starts_with('+') { n } else { len - 1 - n };
                if idx == 0 {
                    // Removing the current directory: fall back to a plain popd.
                    return self.popd_top(out, redir);
                }
                // idx-1 indexes into the saved stack.
                self.dir_stack.remove(idx - 1);
            }
            None => return self.popd_top(out, redir),
            Some(_) => {
                self.emit_stderr(b"osh: popd: invalid argument\n");
                return 1;
            }
        }
        self.print_dirs_line(out, redir)
    }

    /// Pop the saved top of the directory stack and change to it (the common
    /// `popd` with no rotation argument). Errors if the stack is empty.
    fn popd_top(&mut self, out: &mut Out, redir: &RedirPlan) -> i32 {
        if self.dir_stack.is_empty() {
            self.emit_stderr(b"osh: popd: directory stack empty\n");
            return 1;
        }
        let top = self.dir_stack.remove(0);
        if let Err(e) = self.change_dir(&top) {
            self.emit_stderr(format!("osh: popd: {top}: {e}\n").as_bytes());
            return 1;
        }
        self.print_dirs_line(out, redir)
    }

    /// `dirs [-c] [-l] [-p] [-v] [+N | -N]` — display the directory stack.
    /// `-c` clears it, `-l` uses long (un-contracted) paths, `-p` prints one per
    /// line, `-v` prints one per line with an index; `+N`/`-N` print a single
    /// entry (from the left / right).
    fn builtin_dirs(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut clear = false;
        let mut long = false;
        let mut per_line = false;
        let mut verbose = false;
        let mut single: Option<String> = None;
        for a in args {
            if a.len() > 1
                && (a.starts_with('+') || a.starts_with('-'))
                && a[1..].chars().all(|c| c.is_ascii_digit())
            {
                single = Some(a.clone());
            } else if let Some(flags) = a.strip_prefix('-') {
                for c in flags.chars() {
                    match c {
                        'c' => clear = true,
                        'l' => long = true,
                        'p' => per_line = true,
                        'v' => verbose = true,
                        _ => {}
                    }
                }
            }
        }
        if clear {
            self.dir_stack.clear();
            return 0;
        }
        let full = self.dir_stack_full();
        if let Some(spec) = single {
            let len = full.len();
            let n: usize = spec[1..].parse().unwrap_or(0);
            if n >= len {
                self.emit_stderr(b"osh: dirs: directory stack index out of range\n");
                return 1;
            }
            let idx = if spec.starts_with('+') { n } else { len - 1 - n };
            let rendered = self.dirs_render(&full[idx], long);
            return self.write_line(out, redir, &rendered);
        }
        if per_line || verbose {
            let mut s = String::new();
            for (i, p) in full.iter().enumerate() {
                if verbose {
                    s.push_str(&format!("{i:2}  "));
                }
                s.push_str(&self.dirs_render(p, long));
                s.push('\n');
            }
            return self.write_bytes(out, redir, s.as_bytes());
        }
        let line = full
            .iter()
            .map(|p| self.dirs_render(p, long))
            .collect::<Vec<_>>()
            .join(" ");
        self.write_line(out, redir, &line)
    }

    /// `trap [-lp] [[action] sigspec ...]` — set, reset, print, or list signal
    /// and pseudo-signal handlers. Only the `EXIT` trap is currently *fired*
    /// (on top-level shell exit); other specs are recorded and printed
    /// faithfully but not yet delivered — async signal delivery needs kernel
    /// support (see known-issues TD-OILS11).
    fn builtin_trap(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut print = false;
        let mut list = false;
        let mut i = 0;
        while let Some(a) = args.get(i) {
            match a.as_str() {
                "--" => {
                    i += 1;
                    break;
                }
                "-p" => print = true,
                "-l" => list = true,
                "-lp" | "-pl" => {
                    print = true;
                    list = true;
                }
                _ => break,
            }
            i += 1;
        }
        let rest = &args[i..];

        if list {
            return self.trap_list(out, redir);
        }
        // With no action operands (or `-p`), print the current traps.
        let first_is_spec = rest.first().is_some_and(|s| normalize_sigspec(s).is_some());
        if print || rest.is_empty() || first_is_spec {
            return self.trap_print(rest, out, redir);
        }

        // Otherwise the first operand is the action; the rest are sigspecs.
        let action = &rest[0];
        let specs = &rest[1..];
        if specs.is_empty() {
            self.emit_stderr(b"osh: trap: usage: trap [-lp] [[arg] signal_spec ...]\n");
            return 2;
        }
        let reset = action == "-";
        let mut status = 0;
        for spec in specs {
            match normalize_sigspec(spec) {
                Some(norm) => {
                    if reset {
                        self.traps.remove(&norm);
                    } else {
                        self.traps.insert(norm, action.clone());
                    }
                }
                None => {
                    self.emit_stderr(
                        format!("osh: trap: {spec}: invalid signal specification\n").as_bytes(),
                    );
                    status = 1;
                }
            }
        }
        status
    }

    /// Print the currently-set traps in re-inputtable form (`trap -- 'act' SIG`),
    /// sorted by signal number. When `specs` is non-empty, only those are shown.
    fn trap_print(&mut self, specs: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let filter: Option<Vec<String>> = if specs.is_empty() {
            None
        } else {
            Some(specs.iter().filter_map(|s| normalize_sigspec(s)).collect())
        };
        let mut entries: Vec<(&String, &String)> = self
            .traps
            .iter()
            .filter(|(k, _)| filter.as_ref().is_none_or(|f| f.contains(k)))
            .collect();
        entries.sort_by_key(|(k, _)| sigspec_order(k));
        let mut buf = String::new();
        for (sig, action) in entries {
            let quoted = single_quote(action);
            buf.push_str(&format!("trap -- {quoted} {sig}\n"));
        }
        self.write_bytes(out, redir, buf.as_bytes())
    }

    /// `trap -l` — list the known signal names, five per line, numbered.
    fn trap_list(&mut self, out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut buf = String::new();
        for (idx, (num, name)) in SIGNALS.iter().enumerate() {
            buf.push_str(&format!("{num:2}) SIG{name:<9}"));
            if idx % 5 == 4 {
                buf.push('\n');
            }
        }
        if !buf.ends_with('\n') {
            buf.push('\n');
        }
        self.write_bytes(out, redir, buf.as_bytes())
    }

    /// Poll every tracked background job without blocking, recording the exit
    /// status of any that have finished (their child handle is dropped once
    /// reaped). Called before `jobs`/`wait` so the reported state is current.
    fn poll_jobs(&mut self) {
        for job in &mut self.jobs {
            if let Some(child) = job.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(es)) => {
                        job.status = Some(es.code().unwrap_or(1));
                        job.child = None;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        // Treat an un-waitable child as finished with failure so
                        // it does not linger in the table forever.
                        job.status = Some(1);
                        job.child = None;
                    }
                }
            }
        }
    }

    /// `jobs [-l] [-p]` — list background jobs. `-l` adds the pid column; `-p`
    /// prints pids only. Finished jobs are reported once and then removed from
    /// the table (matching bash's notify-and-forget behavior).
    fn builtin_jobs(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        self.poll_jobs();
        let mut long = false;
        let mut pids_only = false;
        for a in args {
            match a.as_str() {
                "-l" => long = true,
                "-p" => pids_only = true,
                _ => {}
            }
        }
        let mut buf = String::new();
        for job in &self.jobs {
            if pids_only {
                buf.push_str(&job.pid.to_string());
                buf.push('\n');
                continue;
            }
            let state = if job.status.is_some() { "Done" } else { "Running" };
            if long {
                buf.push_str(&format!("[{}]  {} {:<24}{}\n", job.id, job.pid, state, job.cmd));
            } else {
                buf.push_str(&format!("[{}]  {:<24}{}\n", job.id, state, job.cmd));
            }
        }
        let status = self.write_bytes(out, redir, buf.as_bytes());
        // Drop the jobs we just reported as Done.
        self.jobs.retain(|j| j.status.is_none());
        status
    }

    /// `wait [id|pid|%job ...]` — wait for background jobs to finish. With no
    /// operands, wait for all jobs. Returns the exit status of the last waited
    /// job (0 when there are no jobs to wait for). Each waited job is removed
    /// from the table.
    fn builtin_wait(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // Wait for all jobs, blocking on each.
            let mut last = 0;
            for job in &mut self.jobs {
                if let Some(mut child) = job.child.take() {
                    last = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
                    job.status = Some(last);
                } else if let Some(s) = job.status {
                    last = s;
                }
            }
            self.jobs.clear();
            return last;
        }
        let mut last = 0;
        for spec in args {
            // Resolve the operand to a job index: `%n` job spec, bare job id, or pid.
            let idx = if let Some(rest) = spec.strip_prefix('%') {
                rest.parse::<usize>().ok().and_then(|n| self.jobs.iter().position(|j| j.id == n))
            } else if let Ok(n) = spec.parse::<u32>() {
                self.jobs
                    .iter()
                    .position(|j| j.pid == n)
                    .or_else(|| self.jobs.iter().position(|j| j.id as u32 == n))
            } else {
                None
            };
            let Some(idx) = idx else {
                self.emit_stderr(format!("osh: wait: {spec}: no such job\n").as_bytes());
                last = 127;
                continue;
            };
            let job = &mut self.jobs[idx];
            if let Some(mut child) = job.child.take() {
                last = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
            } else {
                last = job.status.unwrap_or(0);
            }
            self.jobs.remove(idx);
        }
        last
    }

    /// Fire a synchronous trap handler (`ERR`/`DEBUG`/`RETURN`) if one is set
    /// and we are not already inside a trap. The handler runs with the current
    /// `$?` visible and does not clobber it (a handler that changes `$?` has it
    /// restored afterwards, matching bash's "the trap does not alter the
    /// command's status" behavior for these traps).
    fn fire_trap(&mut self, name: &str) {
        if self.in_trap {
            return;
        }
        let Some(action) = self.traps.get(name).cloned() else {
            return;
        };
        if action.is_empty() {
            return;
        }
        self.in_trap = true;
        let saved = self.last_status;
        self.run_source(&action);
        self.last_status = saved;
        self.in_trap = false;
    }

    /// Run the `EXIT` trap, if set, exactly once when the top-level shell exits.
    /// Called by the binary driver at each true-exit point.
    pub fn run_exit_trap(&mut self) {
        if self.exit_trap_done {
            return;
        }
        self.exit_trap_done = true;
        if let Some(action) = self.traps.get("EXIT").cloned()
            && !action.is_empty()
        {
            // bash: the shell's exit status is the one in effect when the trap
            // fires; preserve it across the handler (a handler that itself runs
            // `exit N` is a rare case we do not special-case).
            let saved = self.last_status;
            self.run_source(&action);
            self.last_status = saved;
        }
    }

    fn builtin_pwd(&mut self, out: &mut Out, redir: &RedirPlan) -> i32 {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.write_line(out, redir, &cwd)
    }

    fn builtin_echo(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut newline = true;
        let mut start = 0;
        if args.first().map(String::as_str) == Some("-n") {
            newline = false;
            start = 1;
        }
        let mut line = args[start..].join(" ");
        if newline {
            line.push('\n');
        }
        self.write_bytes(out, redir, line.as_bytes())
    }

    fn builtin_printf(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // `-v var`: store the result in the shell variable `var` instead of
        // writing it to stdout.
        let mut i = 0;
        let mut assign_var: Option<String> = None;
        if args.first().map(String::as_str) == Some("-v") {
            let Some(name) = args.get(1) else {
                eprintln!("osh: printf: -v: option requires an argument");
                return 2;
            };
            assign_var = Some(name.clone());
            i = 2;
        }
        let Some(fmt) = args.get(i) else {
            return 0;
        };
        let text = format_printf(fmt, &args[i + 1..]);
        if let Some(name) = assign_var {
            self.vars.insert(name, text);
            0
        } else {
            self.write_bytes(out, redir, text.as_bytes())
        }
    }

    fn builtin_export(&mut self, args: &[String]) -> i32 {
        for a in args {
            if let Some(eq) = a.find('=') {
                let (k, v) = (a[..eq].to_string(), a[eq + 1..].to_string());
                self.vars.insert(k.clone(), v);
                self.exported.insert(k);
            } else {
                self.exported.insert(a.clone());
            }
        }
        0
    }

    /// `declare`/`typeset`/`local`: create typed variables. Supports `-A`
    /// (associative array), `-a` (indexed array), `-x` (export), `-r`
    /// (readonly), `-i`/`+i` (integer attribute — assignments evaluated as
    /// arithmetic), and scalar `name=value`. Other type flags (`-g`, `-l`,
    /// `-u`, `-n`) are accepted but have no effect here. The combined form
    /// `declare -A m=(…)` is handled by [`Shell::exec_declare_with_arrays`].
    /// `declare`/`typeset` (`is_local = false`) and `local` (`is_local = true`).
    /// For `local`, each named variable is first shadowed in the current
    /// function frame; using it outside a function is an error.
    /// `declare -p [names]` — print variable definitions in a re-inputtable
    /// form. With names, print each named variable (error + status 1 for any
    /// that is unset); with none, print every set variable sorted by name.
    fn declare_print(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // Names are the non-flag operands after the leading dashed flags.
        let names: Vec<&String> = args.iter().skip_while(|a| a.starts_with('-')).collect();
        let mut listing = String::new();
        let mut status = 0;
        if names.is_empty() {
            let mut all: Vec<&String> = self
                .vars
                .keys()
                .chain(self.arrays.keys())
                .chain(self.assoc.keys())
                .collect();
            all.sort();
            all.dedup();
            for name in all {
                if let Some(def) = self.format_declare_def(name) {
                    listing.push_str(&def);
                    listing.push('\n');
                }
            }
        } else {
            for name in names {
                if let Some(def) = self.format_declare_def(name) {
                    listing.push_str(&def);
                    listing.push('\n');
                } else {
                    self.emit_stderr(format!("osh: declare: {name}: not found\n").as_bytes());
                    status = 1;
                }
            }
        }
        let w = self.write_bytes(out, redir, listing.as_bytes());
        if w != 0 { w } else { status }
    }

    /// Format one variable's `declare` definition, or `None` if it is unset.
    /// Attribute flags (`-r` readonly, `-x` exported, `-a`/`-A` array kind) are
    /// combined into a single flag group, e.g. `declare -rx name="v"`.
    fn format_declare_def(&self, name: &str) -> Option<String> {
        let readonly = self.readonly.contains(name);
        let exported = self.exported.contains(name);
        let integer = self.integer_attr.contains(name);
        let lower = self.lower_attr.contains(name);
        let upper = self.upper_attr.contains(name);
        // Build the trailing attribute letters shared by all kinds.
        let attr = |kind: &str| -> String {
            let mut s = String::from(kind);
            if integer {
                s.push('i');
            }
            if lower {
                s.push('l');
            }
            if upper {
                s.push('u');
            }
            if readonly {
                s.push('r');
            }
            if exported {
                s.push('x');
            }
            if s.is_empty() { "--".to_string() } else { format!("-{s}") }
        };
        if let Some(map) = self.assoc.get(name) {
            let body = map
                .iter()
                .map(|(k, v)| format!("[{k}]={}", quote_declare_value(v)))
                .collect::<Vec<_>>()
                .join(" ");
            return Some(format!("declare {} {name}=({body})", attr("A")));
        }
        if let Some(arr) = self.arrays.get(name) {
            let body = arr
                .iter()
                .map(|(i, v)| format!("[{i}]={}", quote_declare_value(v)))
                .collect::<Vec<_>>()
                .join(" ");
            return Some(format!("declare {} {name}=({body})", attr("a")));
        }
        if let Some(v) = self.vars.get(name) {
            return Some(format!("declare {} {name}={}", attr(""), quote_declare_value(v)));
        }
        None
    }

    fn builtin_declare(&mut self, args: &[String], is_local: bool) -> i32 {
        if is_local && self.local_frames.is_empty() {
            self.emit_stderr(b"osh: local: can only be used in a function\n");
            return 1;
        }
        let mut assoc = false;
        let mut indexed = false;
        let mut export = false;
        let mut readonly = false;
        // Integer attribute: `-i` sets it, `+i` removes it.
        let mut integer = false;
        let mut unset_integer = false;
        // Case attribute directive, updated in flag order so the last one wins
        // (`-l`/`-u` are mutually exclusive; `+l`/`+u` clear). `None` = untouched,
        // `Some(0)` = clear, `Some(1)` = lowercase, `Some(2)` = uppercase.
        let mut case_dir: Option<u8> = None;
        let mut i = 0;
        while let Some(arg) = args.get(i) {
            if arg == "--" {
                i += 1;
                break;
            }
            // Flags may be introduced with `-` (enable) or `+` (disable).
            let enable = arg.starts_with('-');
            if let Some(flags) = arg.strip_prefix(['-', '+'])
                && !flags.is_empty()
            {
                for c in flags.chars() {
                    match c {
                        'A' => assoc = true,
                        'a' => indexed = true,
                        'x' => export = true,
                        'r' => readonly = true,
                        'i' => {
                            if enable {
                                integer = true;
                            } else {
                                unset_integer = true;
                            }
                        }
                        'l' => case_dir = Some(if enable { 1 } else { 0 }),
                        'u' => case_dir = Some(if enable { 2 } else { 0 }),
                        _ => {} // -g/-n/-p: accepted, no effect here.
                    }
                }
                i += 1;
            } else {
                break;
            }
        }
        let mut status = 0;
        for name_val in &args[i..] {
            let (name, value) = match name_val.find('=') {
                Some(eq) => (&name_val[..eq], Some(name_val[eq + 1..].to_string())),
                None => (name_val.as_str(), None),
            };
            if name.is_empty() {
                continue;
            }
            // Reassigning a value to an existing readonly variable is an error.
            if value.is_some() && self.readonly.contains(name) {
                self.emit_stderr(format!("osh: {name}: readonly variable\n").as_bytes());
                status = 1;
                continue;
            }
            // `local` shadows the name (snapshot + clear) before (re)binding it.
            if is_local {
                self.declare_local(name);
            }
            if assoc {
                self.assoc.entry(name.to_string()).or_default();
            } else if indexed {
                self.arrays.entry(name.to_string()).or_default();
            }
            // Apply/remove the integer and case attributes before binding the
            // value, so a `declare -i x=5+3` initial value is evaluated
            // arithmetically and `declare -u x=abc` is folded to uppercase.
            if integer {
                self.integer_attr.insert(name.to_string());
            } else if unset_integer {
                self.integer_attr.remove(name);
            }
            match case_dir {
                Some(1) => {
                    // `-l`: lowercase (mutually exclusive with uppercase).
                    self.lower_attr.insert(name.to_string());
                    self.upper_attr.remove(name);
                }
                Some(2) => {
                    // `-u`: uppercase.
                    self.upper_attr.insert(name.to_string());
                    self.lower_attr.remove(name);
                }
                Some(_) => {
                    // `+l`/`+u`: clear both case attributes.
                    self.lower_attr.remove(name);
                    self.upper_attr.remove(name);
                }
                None => {}
            }
            if let Some(v) = value {
                if assoc || indexed {
                    // `declare -A m=str` / `-a a=str` — scalar init unsupported;
                    // ignore the value (bash would treat str as element/key).
                } else if self.integer_attr.contains(name) {
                    // Integer attribute: the initializer is an arithmetic
                    // expression, evaluated and stored as its decimal value.
                    let n = self.eval_arith_raw(&v).unwrap_or(0);
                    self.vars.insert(name.to_string(), n.to_string());
                } else {
                    // Case attribute (`-l`/`-u`), if any, folds the value.
                    self.vars.insert(name.to_string(), self.fold_case_attr(name, v));
                }
            }
            if export {
                self.exported.insert(name.to_string());
            }
            // Mark readonly *after* the (initial) value is bound so the value is
            // accepted; subsequent assignments then hit the guard above.
            if readonly {
                self.readonly.insert(name.to_string());
            }
        }
        status
    }

    /// Handle the combined `declare -A m=([k]=v)` / `declare -a a=(x y)` form,
    /// where the array literal is an operand of a declaration builtin. Flags and
    /// any scalar/plain operands in `argv` go through [`Shell::builtin_declare`];
    /// each array literal is then marked with the declared kind (`-A` → assoc,
    /// `-a`/default → indexed) and applied via [`Shell::apply_assignment`].
    fn exec_declare_with_arrays(&mut self, argv: &[String], decl_arrays: &[Assignment]) -> Flow {
        let is_local = argv.first().map(String::as_str) == Some("local");
        if is_local && self.local_frames.is_empty() {
            self.emit_stderr(b"osh: local: can only be used in a function\n");
            self.last_status = 1;
            return Flow::Next;
        }
        // Determine the array kind from the leading dashed flags.
        let mut assoc = false;
        let mut indexed = false;
        for arg in &argv[1..] {
            let Some(flags) = arg.strip_prefix('-') else {
                break; // first non-flag operand — flags are done
            };
            if flags == "-" {
                break; // `--` ends option parsing
            }
            for c in flags.chars() {
                match c {
                    'A' => assoc = true,
                    'a' => indexed = true,
                    _ => {}
                }
            }
        }
        // Apply flags + any scalar operands (e.g. `declare -x FOO=bar`).
        let status = self.builtin_declare(&argv[1..], is_local);
        // Mark each array name's kind before applying, so `apply_assignment`
        // routes the literal to the associative or indexed store correctly.
        for a in decl_arrays {
            // `local a=(…)` shadows the name in the current function frame first.
            if is_local {
                self.declare_local(&a.name);
            }
            if assoc {
                self.assoc.entry(a.name.clone()).or_default();
            } else if indexed {
                self.arrays.entry(a.name.clone()).or_default();
            }
            // Default (no flag): an array literal makes an indexed array — which
            // `apply_assignment` already does for a name absent from `assoc`.
            self.apply_assignment(a);
        }
        self.last_status = status;
        Flow::Next
    }

    /// The `readonly [-p] [-a] [-A] name[=value] …` builtin. Marks each named
    /// variable read-only (assigning an initial value first), so later
    /// assignments and `unset` are rejected. With no name operands (or `-p`),
    /// prints the current readonly variables in a re-inputtable form.
    fn builtin_readonly(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // Strip leading flags; only `-a`/`-A` (array kinds) affect storage.
        let mut names: Vec<&String> = Vec::new();
        let mut assoc = false;
        let mut indexed = false;
        let mut print_only = false;
        let mut i = 0;
        while let Some(arg) = args.get(i) {
            if arg == "--" {
                i += 1;
                break;
            }
            if let Some(flags) = arg.strip_prefix('-')
                && !flags.is_empty()
            {
                for c in flags.chars() {
                    match c {
                        'A' => assoc = true,
                        'a' => indexed = true,
                        'p' => print_only = true,
                        _ => {} // -f/-g: accepted, no effect here.
                    }
                }
                i += 1;
            } else {
                break;
            }
        }
        names.extend(&args[i..]);
        if names.is_empty() || print_only {
            let mut ro: Vec<&String> = self.readonly.iter().collect();
            ro.sort();
            let mut listing = String::new();
            for name in ro {
                match self.vars.get(name) {
                    Some(v) => listing.push_str(&format!("readonly {name}={v}\n")),
                    None => listing.push_str(&format!("readonly {name}\n")),
                }
            }
            return self.write_bytes(out, redir, listing.as_bytes());
        }
        let mut status = 0;
        for name_val in names {
            let (name, value) = match name_val.find('=') {
                Some(eq) => (&name_val[..eq], Some(name_val[eq + 1..].to_string())),
                None => (name_val.as_str(), None),
            };
            if name.is_empty() {
                continue;
            }
            if value.is_some() && self.readonly.contains(name) {
                self.emit_stderr(format!("osh: {name}: readonly variable\n").as_bytes());
                status = 1;
                continue;
            }
            if assoc {
                self.assoc.entry(name.to_string()).or_default();
            } else if indexed {
                self.arrays.entry(name.to_string()).or_default();
            }
            if let Some(v) = value
                && !assoc
                && !indexed
            {
                self.vars.insert(name.to_string(), v);
            }
            self.readonly.insert(name.to_string());
        }
        status
    }

    /// `shopt [-s|-u] [-q] [optname …]` — set/unset/query shell option toggles.
    ///
    /// Supported options: `nullglob`, `dotglob`, `nocaseglob`, `nocasematch`,
    /// `extglob`, `globstar`, `failglob`. Only `nullglob` and `dotglob`
    /// currently affect behavior (pathname expansion); the rest are stored so
    /// scripts that toggle them don't error, and `shopt` reports them
    /// faithfully. `-s` enables, `-u` disables, `-q` suppresses output (status
    /// only). With no option flag, listing/query mode is used.
    fn builtin_shopt(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // Known option names. Unknown names are an error (like bash).
        const KNOWN: &[&str] = &[
            "nullglob",
            "dotglob",
            "nocaseglob",
            "nocasematch",
            "extglob",
            "globstar",
            "failglob",
            "histappend",
            "checkwinsize",
            "cmdhist",
            "lithist",
            "autocd",
        ];
        let mut set = false;
        let mut unset = false;
        let mut quiet = false;
        let mut i = 0;
        while let Some(arg) = args.get(i) {
            match arg.as_str() {
                "-s" => set = true,
                "-u" => unset = true,
                "-q" => quiet = true,
                // `-o` restricts to `set -o` options; not modeled here — accept
                // and ignore so `shopt -o …` doesn't hard-error.
                "-o" | "-p" => {}
                "--" => {
                    i += 1;
                    break;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    self.emit_stderr(format!("osh: shopt: {s}: invalid option\n").as_bytes());
                    return 2;
                }
                _ => break,
            }
            i += 1;
        }
        let names: Vec<&String> = args[i..].iter().collect();

        // Query/list mode: neither -s nor -u.
        if !set && !unset {
            if names.is_empty() {
                // List all known options with their on/off state.
                let mut listing = String::new();
                for opt in KNOWN {
                    let on = self.shopt.get(*opt).copied().unwrap_or(false);
                    listing.push_str(&format!("{opt}\t{}\n", if on { "on" } else { "off" }));
                }
                if quiet {
                    return 0;
                }
                return self.write_bytes(out, redir, listing.as_bytes());
            }
            // Query specific names: status 0 iff all named options are set.
            let mut all_on = true;
            let mut listing = String::new();
            for name in &names {
                if !KNOWN.contains(&name.as_str()) {
                    self.emit_stderr(
                        format!("osh: shopt: {name}: invalid shell option name\n").as_bytes(),
                    );
                    return 1;
                }
                let on = self.shopt.get(name.as_str()).copied().unwrap_or(false);
                if !on {
                    all_on = false;
                }
                listing.push_str(&format!("{name}\t{}\n", if on { "on" } else { "off" }));
            }
            if !quiet {
                self.write_bytes(out, redir, listing.as_bytes());
            }
            return i32::from(!all_on);
        }

        // Set/unset mode.
        let mut status = 0;
        for name in names {
            if !KNOWN.contains(&name.as_str()) {
                self.emit_stderr(
                    format!("osh: shopt: {name}: invalid shell option name\n").as_bytes(),
                );
                status = 1;
                continue;
            }
            self.shopt.insert(name.clone(), set);
        }
        status
    }

    fn builtin_unset(&mut self, args: &[String]) -> i32 {
        for a in args {
            // A readonly variable cannot be unset.
            if self.readonly.contains(a) {
                self.emit_stderr(format!("osh: {a}: cannot unset: readonly variable\n").as_bytes());
                return 1;
            }
            // `unset name[i]` removes a single element; `unset name` removes the
            // whole variable/array/function.
            if let Some(open) = a.find('[')
                && a.ends_with(']')
            {
                let name = &a[..open];
                let idx_src = &a[open + 1..a.len() - 1];
                if let Some(map) = self.assoc.get_mut(name) {
                    // Associative: remove by string key.
                    map.retain(|(k, _)| k != idx_src);
                } else if let Some(arr) = self.arrays.get_mut(name)
                    && let Ok(idx) = idx_src.parse::<usize>()
                {
                    // Sparse: remove only that index (leaves a gap, bash
                    // semantics — no shifting of higher elements down).
                    arr.remove(&idx);
                }
                continue;
            }
            self.vars.remove(a);
            self.arrays.remove(a);
            self.assoc.remove(a);
            self.exported.remove(a);
            self.funcs.remove(a);
            // Unsetting a variable also drops its attributes (bash semantics).
            self.integer_attr.remove(a);
            self.lower_attr.remove(a);
            self.upper_attr.remove(a);
        }
        0
    }

    fn builtin_set(&mut self, args: &[String]) -> i32 {
        // Handle option flags (`-e`/`-u`/`-x`/… as clusters, `-o name`) and, on
        // the first non-option operand, reset the positional parameters. `--`
        // ends option processing; a bare `-`/`+` is ignored.
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            match arg.as_str() {
                "--" => {
                    self.positional = args[i + 1..].to_vec();
                    return 0;
                }
                "-" | "+" => {
                    i += 1;
                }
                "-o" | "+o" => {
                    let enable = arg.starts_with('-');
                    if let Some(opt) = args.get(i + 1) {
                        self.set_named_option(opt, enable);
                        i += 2;
                    } else {
                        // `set -o` with no name: list options (not implemented);
                        // accept as a no-op.
                        i += 1;
                    }
                }
                s if s.starts_with('-') || s.starts_with('+') => {
                    let enable = s.starts_with('-');
                    for c in s[1..].chars() {
                        self.set_short_option(c, enable);
                    }
                    i += 1;
                }
                _ => {
                    self.positional = args[i..].to_vec();
                    return 0;
                }
            }
        }
        0
    }

    /// Apply a single-letter `set` option (`-e`/`-u`/`-x`/…). Unknown letters are
    /// accepted and ignored for compatibility.
    fn set_short_option(&mut self, c: char, enable: bool) {
        match c {
            'e' => self.errexit = enable,
            'u' => self.nounset = enable,
            'x' => self.xtrace = enable,
            _ => {}
        }
    }

    /// Apply a `set -o NAME` / `set +o NAME` long option. Unknown names are
    /// accepted and ignored.
    fn set_named_option(&mut self, name: &str, enable: bool) {
        match name {
            "pipefail" => self.pipefail = enable,
            "errexit" => self.errexit = enable,
            "nounset" => self.nounset = enable,
            "xtrace" => self.xtrace = enable,
            _ => {}
        }
    }

    fn builtin_shift(&mut self, args: &[String]) -> i32 {
        let n = args.first().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
        if n <= self.positional.len() {
            self.positional.drain(..n);
            0
        } else {
            1
        }
    }

    /// The `getopts optstring name [args...]` builtin: POSIX-style option
    /// parser. Reads one option per invocation, tracking position across calls
    /// via the `OPTIND` shell variable and the internal `getopts_col` cursor
    /// (for bundled flags like `-abc`). Sets `name` to the option character,
    /// `OPTARG` to any option-argument. Returns 0 while options remain, 1 at
    /// the end of the option list.
    fn builtin_getopts(&mut self, args: &[String]) -> i32 {
        let optstring = match args.first() {
            Some(s) => s.clone(),
            None => {
                eprintln!("osh: getopts: usage: getopts optstring name [arg ...]");
                return 2;
            }
        };
        let name = match args.get(1) {
            Some(s) => s.clone(),
            None => {
                eprintln!("osh: getopts: usage: getopts optstring name [arg ...]");
                return 2;
            }
        };
        let silent = optstring.starts_with(':');
        // Arguments to scan: explicit args after `name`, else the positionals.
        let pos: Vec<String> = if args.len() > 2 {
            args[2..].to_vec()
        } else {
            self.positional.clone()
        };
        let mut optind = self
            .vars
            .get("OPTIND")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1);
        if optind == 0 {
            optind = 1;
        }
        // If OPTIND was reset externally (e.g. `OPTIND=1`), restart bundling.
        if optind != self.getopts_optind {
            self.getopts_col = 0;
        }

        loop {
            // No more arguments to scan. (optind >= 1 is guaranteed above.)
            if optind > pos.len() {
                self.getopts_col = 0;
                self.getopts_optind = optind;
                self.vars.insert("OPTIND".to_string(), optind.to_string());
                return 1;
            }
            let arg = &pos[optind - 1];
            if self.getopts_col == 0 {
                // Start of a fresh argument.
                if !arg.starts_with('-') || arg == "-" {
                    self.getopts_optind = optind;
                    self.vars.insert("OPTIND".to_string(), optind.to_string());
                    return 1;
                }
                if arg == "--" {
                    optind += 1;
                    self.getopts_col = 0;
                    self.getopts_optind = optind;
                    self.vars.insert("OPTIND".to_string(), optind.to_string());
                    return 1;
                }
                self.getopts_col = 1;
            }
            let chars: Vec<char> = arg.chars().collect();
            if self.getopts_col >= chars.len() {
                // Exhausted this argument's flags; advance to the next.
                optind += 1;
                self.getopts_col = 0;
                continue;
            }
            let opt = chars[self.getopts_col];
            self.getopts_col += 1;

            // Look the option up in the optstring (skipping ':' modifiers).
            let ospec: Vec<char> = optstring.chars().collect();
            let mut found = false;
            let mut takes_arg = false;
            for (i, &c) in ospec.iter().enumerate() {
                if c == ':' {
                    continue;
                }
                if c == opt {
                    found = true;
                    takes_arg = ospec.get(i + 1) == Some(&':');
                    break;
                }
            }

            let arg_exhausted = self.getopts_col >= chars.len();

            if !found {
                self.vars.insert(name.clone(), "?".to_string());
                if silent {
                    self.vars.insert("OPTARG".to_string(), opt.to_string());
                } else {
                    eprintln!("osh: getopts: illegal option -- {opt}");
                    self.vars.remove("OPTARG");
                }
                if arg_exhausted {
                    optind += 1;
                    self.getopts_col = 0;
                }
                self.getopts_optind = optind;
                self.vars.insert("OPTIND".to_string(), optind.to_string());
                return 0;
            }

            if takes_arg {
                if !arg_exhausted {
                    // Remainder of the current argument is the option-argument.
                    let optarg: String = chars[self.getopts_col..].iter().collect();
                    self.vars.insert("OPTARG".to_string(), optarg);
                    optind += 1;
                    self.getopts_col = 0;
                } else if optind < pos.len() {
                    // The next argument is the option-argument.
                    let optarg = pos[optind].clone();
                    self.vars.insert("OPTARG".to_string(), optarg);
                    optind += 2;
                    self.getopts_col = 0;
                } else {
                    // Missing required argument.
                    optind += 1;
                    self.getopts_col = 0;
                    if silent {
                        self.vars.insert(name.clone(), ":".to_string());
                        self.vars.insert("OPTARG".to_string(), opt.to_string());
                    } else {
                        eprintln!("osh: getopts: option requires an argument -- {opt}");
                        self.vars.insert(name.clone(), "?".to_string());
                        self.vars.remove("OPTARG");
                    }
                    self.getopts_optind = optind;
                    self.vars.insert("OPTIND".to_string(), optind.to_string());
                    return 0;
                }
                self.vars.insert(name.clone(), opt.to_string());
                self.getopts_optind = optind;
                self.vars.insert("OPTIND".to_string(), optind.to_string());
                return 0;
            }

            // Plain flag with no argument.
            self.vars.insert(name.clone(), opt.to_string());
            self.vars.remove("OPTARG");
            if arg_exhausted {
                optind += 1;
                self.getopts_col = 0;
            }
            self.getopts_optind = optind;
            self.vars.insert("OPTIND".to_string(), optind.to_string());
            return 0;
        }
    }

    /// The `read [-r] [-a array] [-p prompt] [-s] name...` builtin. Reads one
    /// line from the current input, then splits it on `$IFS` (honoring the
    /// whitespace-vs-non-whitespace IFS distinction) and assigns the fields to
    /// the named variables — the last variable receiving the raw remainder.
    /// Without `-r`, backslash acts as an escape (and prevents field splitting on
    /// the escaped character). With `-a`, all fields go into one indexed array.
    /// `-d`/`-n`/`-N`/`-t`/`-u` are accepted (their argument consumed) but not
    /// yet honored — see known-issues.
    fn builtin_read(&mut self, args: &[String], stdin: &StdinSrc, redir: &RedirPlan) -> i32 {
        let mut raw = false;
        let mut array: Option<String> = None;
        let mut names: Vec<String> = Vec::new();
        // `-d delim` (record delimiter; empty ⇒ NUL), `-n N` (stop after N
        // characters or the delimiter), `-N N` (read exactly N characters,
        // ignoring the delimiter). None ⇒ default line-oriented read.
        let mut delim: Option<u8> = None;
        let mut nchars: Option<usize> = None;
        let mut exact = false;
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-r" => raw = true,
                "-s" => {} // silent (no terminal echo) — no-op for non-tty input.
                "-a" => {
                    i += 1;
                    array = args.get(i).cloned();
                }
                "-p" => {
                    i += 1;
                    if let Some(prompt) = args.get(i) {
                        self.emit_stderr(prompt.as_bytes());
                    }
                }
                "-d" => {
                    i += 1;
                    // `-d ''` ⇒ NUL delimiter; otherwise the first byte.
                    delim = Some(args.get(i).and_then(|s| s.bytes().next()).unwrap_or(0));
                }
                "-n" => {
                    i += 1;
                    nchars = args.get(i).and_then(|s| s.parse().ok());
                }
                "-N" => {
                    i += 1;
                    nchars = args.get(i).and_then(|s| s.parse().ok());
                    exact = true;
                }
                // Accepted but not honored yet; consume the option-argument forms.
                "-t" | "-u" => i += 1,
                other if other.starts_with('-') && other.len() > 1 => {} // unknown flag
                _ => names.push(a.clone()),
            }
            i += 1;
        }

        // Choose the read strategy. Any of `-d`/`-n`/`-N` selects the
        // record reader; otherwise a plain newline-terminated line.
        let (line, terminated) = if delim.is_some() || nchars.is_some() {
            let d = delim.unwrap_or(b'\n');
            match self.read_record_input(stdin, redir, d, nchars, exact) {
                Some(rec) => rec,
                None => return 1, // EOF with no data
            }
        } else {
            match self.read_line(stdin, redir) {
                Some(l) => (l, true),
                None => return 1, // EOF
            }
        };
        // Exit status: for `-N`, success iff exactly N characters were read
        // (a short read at EOF is status 1). For `-d`/`-n`, success iff the
        // record was terminated (delimiter seen or the `-n` count reached); a
        // missing delimiter at EOF is a partial read (status 1) but the value
        // is still assigned. The default line path always reports success.
        let eof_status = if exact {
            i32::from(nchars.is_some_and(|n| line.chars().count() < n))
        } else {
            i32::from(!terminated)
        };

        let ifs = self.vars.get("IFS").cloned().unwrap_or_else(|| " \t\n".to_string());

        if let Some(arr) = array {
            let fields = read_split(&line, &ifs, raw, None);
            let map: BTreeMap<usize, String> =
                fields.into_iter().enumerate().collect();
            self.vars.remove(&arr);
            self.assoc.remove(&arr);
            self.arrays.insert(arr, map);
            return eof_status;
        }

        if names.is_empty() {
            // No names: assign the (optionally unescaped) whole line to REPLY.
            let reply = if raw { line } else { unescape_read_line(&line) };
            self.vars.insert("REPLY".to_string(), reply);
            return eof_status;
        }

        let fields = read_split(&line, &ifs, raw, Some(names.len()));
        for (idx, name) in names.iter().enumerate() {
            let val = fields.get(idx).cloned().unwrap_or_default();
            self.vars.insert(name.clone(), val);
        }
        eof_status
    }

    /// Read the entire current input source (here-doc/here-string, `< file`
    /// redirect, pipeline cursor/pipe, or inherited stdin) to end-of-input.
    fn read_all_bytes(&self, stdin: &StdinSrc, redir: &RedirPlan) -> Vec<u8> {
        use io::Read;
        if let Some(data) = &redir.stdin_data {
            return data.clone();
        }
        if let Some(path) = &redir.stdin {
            return std::fs::read(path).unwrap_or_default();
        }
        let mut buf = Vec::new();
        match stdin {
            StdinSrc::Cursor(c) => {
                let _ = c.borrow_mut().read_to_end(&mut buf);
            }
            StdinSrc::Pipe(r) => {
                let _ = r.borrow_mut().read_to_end(&mut buf);
            }
            StdinSrc::Inherit => {
                let _ = io::stdin().lock().read_to_end(&mut buf);
            }
        }
        buf
    }

    /// The `mapfile`/`readarray [-t] [-d delim] [-n count] [-s skip] [-O origin]
    /// [array]` builtin: read lines from standard input into an indexed array
    /// (default `MAPFILE`). Each element retains the trailing delimiter unless
    /// `-t` is given. Supports `-d` (alternate delimiter), `-n` (max count, 0 =
    /// all), `-s` (skip leading lines), and `-O` (starting array index).
    fn builtin_mapfile(&mut self, args: &[String], stdin: &StdinSrc, redir: &RedirPlan) -> i32 {
        let mut strip = false;
        let mut delim = b'\n';
        let mut count: usize = 0; // 0 = unlimited
        let mut skip: usize = 0;
        let mut origin: usize = 0;
        let mut array = String::from("MAPFILE");
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "-t" => strip = true,
                "-d" => {
                    i += 1;
                    delim = args.get(i).and_then(|s| s.bytes().next()).unwrap_or(0);
                }
                "-n" | "-c" => {
                    i += 1;
                    count = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
                }
                "-s" => {
                    i += 1;
                    skip = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
                }
                "-O" => {
                    i += 1;
                    origin = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
                }
                other if other.starts_with('-') && other.len() > 1 => {
                    eprintln!("osh: mapfile: {other}: invalid option");
                    return 2;
                }
                _ => array = a.clone(),
            }
            i += 1;
        }

        let data = self.read_all_bytes(stdin, redir);
        // Split on the delimiter, keeping the delimiter on each piece (as bash
        // does), except for a trailing empty piece after a final delimiter.
        let mut pieces: Vec<Vec<u8>> = Vec::new();
        let mut cur: Vec<u8> = Vec::new();
        for &b in &data {
            cur.push(b);
            if b == delim {
                pieces.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            pieces.push(cur);
        }

        let mut elems: BTreeMap<usize, String> = BTreeMap::new();
        let mut idx = origin;
        for piece in pieces.into_iter().skip(skip) {
            if count != 0 && idx.saturating_sub(origin) >= count {
                break;
            }
            let mut s = String::from_utf8_lossy(&piece).into_owned();
            if strip && s.as_bytes().last() == Some(&delim) {
                s.pop();
            }
            elems.insert(idx, s);
            idx = idx.saturating_add(1);
        }
        self.vars.remove(&array);
        self.assoc.remove(&array);
        self.arrays.insert(array, elems);
        0
    }

    fn builtin_source(&mut self, args: &[String]) -> i32 {
        let Some(path) = args.first() else {
            eprintln!("osh: source: filename argument required");
            return 2;
        };
        match std::fs::read_to_string(path) {
            Ok(src) => {
                let saved = if args.len() > 1 {
                    Some(std::mem::replace(&mut self.positional, args[1..].to_vec()))
                } else {
                    None
                };
                let code = self.run_source(&src);
                if let Some(p) = saved {
                    self.positional = p;
                }
                code
            }
            Err(e) => {
                eprintln!("osh: source: {path}: {e}");
                1
            }
        }
    }

    fn builtin_type(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // Shell keywords recognized by `type` (reserved words that introduce or
        // punctuate compound commands).
        const KEYWORDS: &[&str] = &[
            "if", "then", "elif", "else", "fi", "time", "for", "in", "until", "while", "do",
            "done", "case", "esac", "coproc", "select", "function", "{", "}", "!", "[[", "]]",
        ];

        // Parse flags: -t (type word), -p (path if file), -P (force PATH search),
        // -a (all locations), -f (skip function lookup). Flags may be clustered.
        let mut mode_t = false;
        let mut mode_p = false;
        let mut mode_pp = false;
        let mut mode_a = false;
        let mut skip_func = false;
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                i += 1;
                break;
            }
            if let Some(flags) = a.strip_prefix('-')
                && !flags.is_empty()
                && flags.chars().all(|c| "tpPaf".contains(c))
            {
                for c in flags.chars() {
                    match c {
                        't' => mode_t = true,
                        'p' => mode_p = true,
                        'P' => mode_pp = true,
                        'a' => mode_a = true,
                        'f' => skip_func = true,
                        _ => {}
                    }
                }
                i += 1;
            } else {
                break;
            }
        }
        let names: Vec<&String> = args[i..].iter().collect();

        let mut status = 0;
        for name in names {
            let is_kw = KEYWORDS.contains(&name.as_str());
            let is_fn = !skip_func && self.funcs.contains_key(name);
            let is_bi = is_builtin(name);
            // `-P` forces a filesystem search even when the name is a builtin,
            // function, or keyword.
            // Search the filesystem when any flag needs paths, or (for default
            // output) only when the name isn't already a keyword/function/builtin.
            let need_files =
                mode_pp || mode_p || mode_a || mode_t || (!is_kw && !is_fn && !is_bi);
            let files = if need_files {
                self.find_all_in_path(name)
            } else {
                Vec::new()
            };
            let found = is_kw || is_fn || is_bi || !files.is_empty();
            if !found {
                if !mode_t && !mode_p && !mode_pp {
                    self.errln(&format!("osh: type: {name}: not found"));
                }
                status = 1;
                continue;
            }

            if mode_pp {
                // Force PATH search; print only file paths.
                if files.is_empty() {
                    status = 1;
                } else if mode_a {
                    for f in &files {
                        let _ = self.write_line(out, redir, &f.to_string_lossy());
                    }
                } else {
                    let _ = self.write_line(out, redir, &files[0].to_string_lossy());
                }
                continue;
            }

            if mode_t {
                // Single type word (highest precedence): keyword > function >
                // builtin > file.
                let word = if is_kw {
                    "keyword"
                } else if is_fn {
                    "function"
                } else if is_bi {
                    "builtin"
                } else {
                    "file"
                };
                let _ = self.write_line(out, redir, word);
                continue;
            }

            if mode_p {
                // Print the path only when the name would resolve to a file
                // (i.e. it is not a keyword/function/builtin). With -a, print
                // all file paths.
                if is_kw || is_fn || is_bi {
                    // Nothing to print, but the name is found ⇒ status stays 0.
                } else if mode_a {
                    for f in &files {
                        let _ = self.write_line(out, redir, &f.to_string_lossy());
                    }
                } else if let Some(f) = files.first() {
                    let _ = self.write_line(out, redir, &f.to_string_lossy());
                }
                continue;
            }

            // Default (verbose) output. Without -a, print the highest-precedence
            // location only; with -a, print every location in precedence order.
            if mode_a {
                if is_kw {
                    let _ = self.write_line(out, redir, &format!("{name} is a shell keyword"));
                }
                if is_fn {
                    let _ = self.write_line(out, redir, &format!("{name} is a function"));
                }
                if is_bi {
                    let _ = self.write_line(out, redir, &format!("{name} is a shell builtin"));
                }
                for f in &files {
                    let _ =
                        self.write_line(out, redir, &format!("{name} is {}", f.to_string_lossy()));
                }
            } else {
                let desc = if is_kw {
                    format!("{name} is a shell keyword")
                } else if is_fn {
                    format!("{name} is a function")
                } else if is_bi {
                    format!("{name} is a shell builtin")
                } else {
                    format!("{name} is {}", files[0].to_string_lossy())
                };
                let _ = self.write_line(out, redir, &desc);
            }
        }
        status
    }

    fn builtin_test(&mut self, name: &str, args: &[String]) -> i32 {
        // For `[`, the last argument must be `]`.
        let mut a: Vec<&str> = args.iter().map(String::as_str).collect();
        if name == "[" {
            if a.last() == Some(&"]") {
                a.pop();
            } else {
                eprintln!("osh: [: missing ']'");
                return 2;
            }
        }
        // `-v NAME` needs shell state (is the variable set?), which the free
        // `eval_test` helper cannot see — handle it here.
        if a.len() == 2 && a[0] == "-v" {
            return i32::from(!self.var_is_set(a[1]));
        }
        i32::from(!eval_test(&a))
    }

    // ---- output helpers -----------------------------------------------------

    fn write_line(&mut self, out: &mut Out, redir: &RedirPlan, line: &str) -> i32 {
        let mut s = line.to_string();
        s.push('\n');
        self.write_bytes(out, redir, s.as_bytes())
    }

    fn write_bytes(&mut self, out: &mut Out, redir: &RedirPlan, bytes: &[u8]) -> i32 {
        // `1>&2` on the builtin (e.g. `echo msg >&2`): the builtin's stdout is
        // the current stderr sink, not the ambient stdout.
        if redir.stdout_to_stderr && redir.stdout.is_none() {
            self.emit_stderr(bytes);
            return 0;
        }
        // A `>`/`>>` redirect on the builtin wins over the ambient sink.
        if let Some((path, append)) = &redir.stdout {
            match open_out(path, *append) {
                Ok(mut f) => {
                    if f.write_all(bytes).is_err() {
                        return 1;
                    }
                    0
                }
                Err(e) => {
                    self.errln(&format!("osh: {path}: {e}"));
                    1
                }
            }
        } else {
            match out {
                Out::Capture(buf) => {
                    buf.extend_from_slice(bytes);
                    0
                }
                Out::Inherit => {
                    let stdout = io::stdout();
                    let mut lock = stdout.lock();
                    if lock.write_all(bytes).is_err() {
                        return 1;
                    }
                    let _ = lock.flush();
                    0
                }
                Out::Pipe(w) => {
                    // A downstream reader that closed early yields `BrokenPipe`;
                    // flag it so the enclosing stage unwinds (SIGPIPE analogue).
                    match w.write_all(bytes) {
                        Ok(()) => 0,
                        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
                            self.pipe_broken = true;
                            141 // 128 + SIGPIPE(13), as a shell would report
                        }
                        Err(_) => 1,
                    }
                }
            }
        }
    }

    /// Write raw bytes to the current stderr (fd 2) — the innermost active
    /// [`StderrTarget`], or the shell's real stderr when none is active. Used
    /// for command diagnostics and `>&2` builtin output so both honour a
    /// compound command's `2>` redirect.
    fn emit_stderr(&self, bytes: &[u8]) {
        match self.stderr_stack.last() {
            None => {
                let e = io::stderr();
                let mut lock = e.lock();
                let _ = lock.write_all(bytes);
                let _ = lock.flush();
            }
            Some(StderrTarget::Stdout) => {
                let o = io::stdout();
                let mut lock = o.lock();
                let _ = lock.write_all(bytes);
                let _ = lock.flush();
            }
            // `&File`/`&PipeWriter` both implement `Write`; the shared handle is
            // append-positioned (files opened once) so concurrent writers from
            // several group commands interleave without clobbering.
            Some(StderrTarget::File(f)) => {
                let _ = (&**f).write_all(bytes);
            }
            Some(StderrTarget::Pipe(p)) => {
                let _ = (&**p).write_all(bytes);
            }
            Some(StderrTarget::Buffer(b)) => {
                if let Ok(mut g) = b.lock() {
                    g.extend_from_slice(bytes);
                }
            }
        }
    }

    /// Write a diagnostic line (a trailing newline is appended) to the current
    /// stderr. Replaces bare `eprintln!` in command-execution paths so shell
    /// error messages honour an active `2>`/`2>&1` redirect, as in bash.
    fn errln(&self, msg: &str) {
        let mut line = msg.as_bytes().to_vec();
        line.push(b'\n');
        self.emit_stderr(&line);
    }

    /// Build a child-process [`Stdio`] that writes to the current stderr sink.
    /// Used for `1>&2` (`stdout_to_stderr`) on an external command. The
    /// buffer-capture sink can't back a live child fd, so it (and the real-fd-1
    /// `Stdout` case) fall back to inheriting fd 2 — a rare edge documented in
    /// the module limitations.
    fn child_stdio_for_stderr(&self) -> Result<Stdio, String> {
        match self.stderr_stack.last() {
            None | Some(StderrTarget::Stdout | StderrTarget::Buffer(_)) => Ok(Stdio::inherit()),
            Some(StderrTarget::File(f)) => f
                .try_clone()
                .map(Stdio::from)
                .map_err(|e| format!("stderr: {e}")),
            Some(StderrTarget::Pipe(p)) => p
                .try_clone()
                .map(Stdio::from)
                .map_err(|e| format!("pipe: {e}")),
        }
    }

    fn read_line(&self, stdin: &StdinSrc, redir: &RedirPlan) -> Option<String> {
        if let Some(data) = &redir.stdin_data {
            // Here-doc/here-string: read the first line. (Multi-line `read`
            // loops over here-docs require compound-command redirects, which are
            // not yet supported — see the module limitations.)
            let mut r = io::BufReader::new(&data[..]);
            return read_one_line(&mut r);
        }
        if let Some(path) = &redir.stdin {
            let f = std::fs::File::open(path).ok()?;
            let mut r = io::BufReader::new(f);
            return read_one_line(&mut r);
        }
        match stdin {
            StdinSrc::Cursor(c) => {
                // `io::Cursor` implements `BufRead`; `read_line` advances its
                // position exactly past the consumed newline, so successive
                // reads yield successive lines.
                read_one_line(&mut *c.borrow_mut())
            }
            StdinSrc::Pipe(r) => {
                // Streaming upstream stage: the `BufReader` yields successive
                // lines as the producer writes them.
                read_one_line(&mut *r.borrow_mut())
            }
            StdinSrc::Inherit => {
                let stdin = io::stdin();
                let mut lock = stdin.lock();
                read_one_line(&mut lock)
            }
        }
    }

    /// Read a single record for `read -d`/`-n`/`-N` from the current input
    /// source. `delim` is the record terminator (consumed, not stored);
    /// `nchars` caps the record at that many characters; `exact` (`-N`)
    /// ignores `delim` and reads exactly `nchars` characters. Returns
    /// `(text, terminated)` where `terminated` is true when a delimiter was
    /// consumed (for `-N`, true when the full character count was read).
    /// `None` signals immediate EOF with no data.
    fn read_record_input(
        &self,
        stdin: &StdinSrc,
        redir: &RedirPlan,
        delim: u8,
        nchars: Option<usize>,
        exact: bool,
    ) -> Option<(String, bool)> {
        if let Some(data) = &redir.stdin_data {
            let mut r = io::BufReader::new(&data[..]);
            return read_record(&mut r, delim, nchars, exact);
        }
        if let Some(path) = &redir.stdin {
            let f = std::fs::File::open(path).ok()?;
            let mut r = io::BufReader::new(f);
            return read_record(&mut r, delim, nchars, exact);
        }
        match stdin {
            StdinSrc::Cursor(c) => read_record(&mut *c.borrow_mut(), delim, nchars, exact),
            StdinSrc::Pipe(r) => read_record(&mut *r.borrow_mut(), delim, nchars, exact),
            StdinSrc::Inherit => {
                let stdin = io::stdin();
                let mut lock = stdin.lock();
                read_record(&mut lock, delim, nchars, exact)
            }
        }
    }
}

/// Let the arithmetic evaluator read shell variables.
impl VarLookup for Shell {
    fn get(&self, name: &str) -> Option<i64> {
        self.param_value(name).and_then(|v| v.trim().parse::<i64>().ok())
    }

    fn get_index(&self, name: &str, index: i64) -> Option<i64> {
        // `array_element` already applies bash negative-index semantics.
        self.array_element(name, index)
            .and_then(|v| v.trim().parse::<i64>().ok())
    }

    fn is_assoc(&self, name: &str) -> bool {
        self.assoc.contains_key(name)
    }

    fn get_assoc(&self, name: &str, key: &str) -> Option<i64> {
        // Bash reads an associative element's value as a number in `(( … ))`;
        // an unset key (or a non-numeric value) evaluates to 0.
        self.assoc_element(name, key)
            .and_then(|v| v.trim().parse::<i64>().ok())
    }

    fn set(&mut self, name: &str, value: i64) {
        self.vars.insert(name.to_string(), value.to_string());
    }

    fn set_index(&mut self, name: &str, index: i64, value: i64) {
        // Mirror the indexed branch of `assign_elem`: negative indices count
        // back from `highest_index + 1` (bash sparse semantics).
        let arr = self.arrays.entry(name.to_string()).or_default();
        let bound = arr.keys().next_back().map_or(0, |k| k.saturating_add(1));
        if let Some(real) = Self::resolve_index(index, bound) {
            arr.insert(real, value.to_string());
        }
    }

    fn set_assoc(&mut self, name: &str, key: &str, value: i64) {
        self.assoc_set(name, key.to_string(), value.to_string(), false);
    }
}

// ---- free helpers -----------------------------------------------------------

/// Per-command redirection plan (expanded targets).
#[derive(Debug, Clone, Default)]
struct RedirPlan {
    stdin: Option<String>,
    /// In-memory stdin bytes from a here-document / here-string (takes
    /// precedence over `stdin` and the inherited pipeline input).
    stdin_data: Option<Vec<u8>>,
    stdout: Option<(String, bool)>,
    stderr: Option<(String, bool)>,
    /// `2>&1` — fd 2 follows fd 1 (stderr goes wherever stdout currently goes).
    /// Distinct from `stderr` (a file path) so the merge works even when stdout
    /// is a pipe/terminal/capture rather than a file.
    stderr_to_stdout: bool,
    /// `1>&2` — fd 1 follows fd 2 (stdout goes wherever stderr currently goes).
    stdout_to_stderr: bool,
}

/// A single expanded character tagged with whether it came from a quoted
/// context. Quoted characters are exempt from field splitting (already done)
/// and pathname (glob) expansion — a quoted `*` matches a literal `*`.
#[derive(Clone, Copy)]
struct EChar {
    c: char,
    quoted: bool,
}

/// Append the characters of `s` to `buf`, tagging each with `quoted`.
fn push_chars(buf: &mut Vec<EChar>, s: &str, quoted: bool) {
    for c in s.chars() {
        buf.push(EChar { c, quoted });
    }
}

/// Apply pathname expansion to one annotated field, pushing the results (or the
/// literal field, if it has no unquoted metacharacter or matches nothing) into
/// `out`. This implements bash's default (no-`nullglob`) behavior: an
/// unmatched pattern is left as the literal word.
/// Whether an annotated field contains an unquoted glob metacharacter (`*`,
/// `?`, `[`), or — when `extglob` is set — an unquoted `X(` extended-pattern
/// operator (`X ∈ ?*+@!`). A field with no metacharacter is a literal word.
fn field_has_glob_meta(field: &[EChar], extglob: bool) -> bool {
    field.iter().enumerate().any(|(i, e)| {
        if e.quoted {
            return false;
        }
        matches!(e.c, '*' | '?' | '[')
            || (extglob
                && matches!(e.c, '?' | '*' | '+' | '@' | '!')
                && matches!(field.get(i + 1), Some(n) if !n.quoted && n.c == '('))
    })
}

fn glob_or_literal(
    field: &[EChar],
    out: &mut Vec<String>,
    nullglob: bool,
    dotglob: bool,
    nocaseglob: bool,
    extglob: bool,
    globstar: bool,
) {
    let has_meta = field_has_glob_meta(field, extglob);
    let literal: String = field.iter().map(|e| e.c).collect();
    if !has_meta {
        out.push(literal);
        return;
    }
    let mut matches = glob_expand_field(field, dotglob, nocaseglob, extglob, globstar);
    if matches.is_empty() {
        // Default (no `nullglob`): an unmatched pattern is left as the literal
        // word. With `nullglob` on, the word is removed entirely (produces no
        // field).
        if !nullglob {
            out.push(literal);
        }
    } else {
        matches.sort();
        out.append(&mut matches);
    }
}

/// A compiled glob pattern token (for one path component).
enum PatTok {
    /// `*` — match any run of characters.
    Star,
    /// `?` — match any single character.
    Any,
    /// A literal character (either an ordinary char or a quoted metacharacter).
    Lit(char),
    /// `[...]` character class.
    Class { negate: bool, items: Vec<ClassItem> },
    /// An `extglob` group: `?(list)`, `*(list)`, `+(list)`, `@(list)`, or
    /// `!(list)`, where each alternative is itself a compiled sub-pattern.
    Group { kind: ExtKind, alts: Vec<Vec<PatTok>> },
}

/// The five `extglob` operators (bash / ksh extended pattern matching).
#[derive(Clone, Copy)]
enum ExtKind {
    /// `?(list)` — zero or one occurrence of any alternative.
    Optional,
    /// `*(list)` — zero or more occurrences.
    Star,
    /// `+(list)` — one or more occurrences.
    Plus,
    /// `@(list)` — exactly one occurrence.
    Once,
    /// `!(list)` — anything except a string matched by an alternative.
    Not,
}

enum ClassItem {
    Ch(char),
    Range(char, char),
}

/// Compile one annotated path component into glob tokens. Quoted characters are
/// always literal; unquoted `* ? [` are special. When `extglob` is set, an
/// unquoted `?(`, `*(`, `+(`, `@(`, or `!(` begins an extended-pattern group.
fn compile_glob(comp: &[EChar], extglob: bool) -> Vec<PatTok> {
    let mut toks = Vec::new();
    let mut i = 0;
    while i < comp.len() {
        let e = comp[i];
        if e.quoted {
            toks.push(PatTok::Lit(e.c));
            i += 1;
            continue;
        }
        // extglob: `X(` where X ∈ ?*+@! and the paren is unquoted.
        if extglob
            && matches!(e.c, '?' | '*' | '+' | '@' | '!')
            && matches!(comp.get(i + 1), Some(n) if !n.quoted && n.c == '(')
            && let Some((tok, next)) = compile_ext_group(comp, i, extglob)
        {
            toks.push(tok);
            i = next;
            continue;
        }
        match e.c {
            '*' => {
                toks.push(PatTok::Star);
                i += 1;
            }
            '?' => {
                toks.push(PatTok::Any);
                i += 1;
            }
            '[' => {
                if let Some((tok, next)) = compile_class(comp, i) {
                    toks.push(tok);
                    i = next;
                } else {
                    toks.push(PatTok::Lit('['));
                    i += 1;
                }
            }
            c => {
                toks.push(PatTok::Lit(c));
                i += 1;
            }
        }
    }
    toks
}

/// Compile an `extglob` group beginning at `comp[start]` (the operator char,
/// with `comp[start + 1] == '('`). Alternatives are separated by top-level
/// unquoted `|`; nested parens are tracked so inner groups stay intact. Returns
/// the compiled [`PatTok::Group`] and the index just past the closing `)`, or
/// `None` if the group is unterminated (caller then treats the operator char
/// literally).
fn compile_ext_group(comp: &[EChar], start: usize, extglob: bool) -> Option<(PatTok, usize)> {
    let kind = match comp[start].c {
        '?' => ExtKind::Optional,
        '*' => ExtKind::Star,
        '+' => ExtKind::Plus,
        '@' => ExtKind::Once,
        '!' => ExtKind::Not,
        _ => return None,
    };
    let mut i = start + 2; // past the operator char and '('
    let mut depth = 1usize;
    let mut alts: Vec<Vec<EChar>> = Vec::new();
    let mut cur: Vec<EChar> = Vec::new();
    while i < comp.len() {
        let e = comp[i];
        if e.quoted {
            cur.push(e);
        } else {
            match e.c {
                '(' => {
                    depth += 1;
                    cur.push(e);
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        alts.push(cur);
                        let compiled = alts.iter().map(|a| compile_glob(a, extglob)).collect();
                        return Some((PatTok::Group { kind, alts: compiled }, i + 1));
                    }
                    cur.push(e);
                }
                '|' if depth == 1 => {
                    alts.push(std::mem::take(&mut cur));
                }
                _ => cur.push(e),
            }
        }
        i += 1;
    }
    None
}

/// Compile a `[...]` class starting at `comp[start] == '['`. Returns the token
/// and the index just past the closing `]`, or `None` if unterminated.
fn compile_class(comp: &[EChar], start: usize) -> Option<(PatTok, usize)> {
    let mut i = start + 1;
    let mut negate = false;
    if matches!(comp.get(i).map(|e| e.c), Some('!' | '^')) {
        negate = true;
        i += 1;
    }
    let mut items = Vec::new();
    let mut first = true;
    while i < comp.len() {
        let c = comp[i].c;
        if c == ']' && !first {
            return Some((PatTok::Class { negate, items }, i + 1));
        }
        first = false;
        if i + 2 < comp.len() && comp[i + 1].c == '-' && comp[i + 2].c != ']' {
            items.push(ClassItem::Range(c, comp[i + 2].c));
            i += 3;
        } else {
            items.push(ClassItem::Ch(c));
            i += 1;
        }
    }
    None
}

/// Match a compiled glob against a filename, anchored at both ends. Recursive so
/// that `extglob` groups (`?()`/`*()`/`+()`/`@()`/`!()`) — which need
/// backtracking over alternatives and repetitions — are handled uniformly with
/// `*`. Patterns and names are short (one path component / one field), so the
/// worst-case backtracking cost is not a concern in practice.
fn match_glob_toks(toks: &[PatTok], name: &[char]) -> bool {
    let Some((first, rest)) = toks.split_first() else {
        return name.is_empty();
    };
    match first {
        PatTok::Star => (0..=name.len()).any(|k| match_glob_toks(rest, &name[k..])),
        PatTok::Any => !name.is_empty() && match_glob_toks(rest, &name[1..]),
        PatTok::Lit(c) => name.first() == Some(c) && match_glob_toks(rest, &name[1..]),
        PatTok::Class { negate, items } => {
            !name.is_empty()
                && (class_matches(items, name[0]) ^ *negate)
                && match_glob_toks(rest, &name[1..])
        }
        PatTok::Group { kind, alts } => match_ext_group(*kind, alts, rest, name),
    }
}

/// Match an `extglob` group followed by `rest` against `name`.
fn match_ext_group(kind: ExtKind, alts: &[Vec<PatTok>], rest: &[PatTok], name: &[char]) -> bool {
    // Whether any alternative matches the whole slice `sub`.
    let any_alt = |sub: &[char]| alts.iter().any(|a| match_glob_toks(a, sub));
    match kind {
        // Exactly one occurrence: some prefix matches an alternative, rest matches the tail.
        ExtKind::Once => {
            (0..=name.len()).any(|k| any_alt(&name[..k]) && match_glob_toks(rest, &name[k..]))
        }
        // Zero or one occurrence.
        ExtKind::Optional => {
            match_glob_toks(rest, name)
                || (1..=name.len())
                    .any(|k| any_alt(&name[..k]) && match_glob_toks(rest, &name[k..]))
        }
        // Zero or more occurrences.
        ExtKind::Star => match_star_group(alts, rest, name),
        // One or more occurrences: one alternative, then zero or more.
        ExtKind::Plus => (1..=name.len())
            .any(|k| any_alt(&name[..k]) && match_star_group(alts, rest, &name[k..])),
        // Negation: some split where the prefix is *not* matched by any
        // alternative and the rest matches the tail.
        ExtKind::Not => {
            (0..=name.len()).any(|k| !any_alt(&name[..k]) && match_glob_toks(rest, &name[k..]))
        }
    }
}

/// Match zero or more repetitions of any alternative, then `rest`. Each
/// repetition consumes at least one character (`k >= 1`), guaranteeing progress.
fn match_star_group(alts: &[Vec<PatTok>], rest: &[PatTok], name: &[char]) -> bool {
    if match_glob_toks(rest, name) {
        return true;
    }
    (1..=name.len()).any(|k| {
        alts.iter().any(|a| match_glob_toks(a, &name[..k]))
            && match_star_group(alts, rest, &name[k..])
    })
}

fn class_matches(items: &[ClassItem], ch: char) -> bool {
    items.iter().any(|it| match it {
        ClassItem::Ch(c) => *c == ch,
        ClassItem::Range(a, b) => *a <= ch && ch <= *b,
    })
}

/// Whether a compiled component's first token is a literal `.` — controls the
/// hidden-file rule (a leading `.` in a name is only matched explicitly).
fn glob_starts_with_dot(toks: &[PatTok]) -> bool {
    matches!(toks.first(), Some(PatTok::Lit('.')))
}

/// Expand an annotated field containing at least one unquoted metacharacter
/// against the filesystem, returning the matching paths (unsorted).
fn glob_expand_field(
    field: &[EChar],
    dotglob: bool,
    nocaseglob: bool,
    extglob: bool,
    globstar: bool,
) -> Vec<String> {
    let absolute = field.first().is_some_and(|e| e.c == '/');
    // Split into non-empty components on '/'.
    let mut comps: Vec<Vec<EChar>> = Vec::new();
    let mut cur: Vec<EChar> = Vec::new();
    for &e in field {
        if e.c == '/' {
            if !cur.is_empty() {
                comps.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(e);
        }
    }
    if !cur.is_empty() {
        comps.push(cur);
    }
    if comps.is_empty() {
        return Vec::new();
    }
    let last = comps.len().saturating_sub(1);
    let mut cands: Vec<String> = vec![if absolute { "/".to_string() } else { String::new() }];
    for (ci, comp) in comps.iter().enumerate() {
        // `**` with `globstar` matches across directory levels: as an
        // intermediate component it stands for the base plus every descendant
        // directory (zero-or-more levels), and as the final component it stands
        // for every descendant file *and* directory.
        if globstar && is_globstar_comp(comp) {
            let terminal = ci == last;
            let mut next: Vec<String> = Vec::new();
            for base in &cands {
                globstar_walk(base, dotglob, terminal, &mut next);
            }
            next.sort();
            next.dedup();
            cands = next;
            continue;
        }
        let has_meta = field_has_glob_meta(comp, extglob);
        let comp_literal: String = comp.iter().map(|e| e.c).collect();
        let mut next: Vec<String> = Vec::new();
        for base in &cands {
            if has_meta {
                let dir = if base.is_empty() { "." } else { base.as_str() };
                let toks = compile_glob(comp, extglob);
                // With `nocaseglob`, match against an ASCII-lowercased copy of
                // both the pattern and each filename (token structure is kept
                // 1:1 by using ASCII folding). The original filename is still
                // the value returned.
                let toks_ci = nocaseglob.then(|| {
                    let low: Vec<EChar> = comp
                        .iter()
                        .map(|e| EChar {
                            c: e.c.to_ascii_lowercase(),
                            quoted: e.quoted,
                        })
                        .collect();
                    compile_glob(&low, extglob)
                });
                // A leading `.` in a filename is only matched when the pattern
                // itself begins with a literal dot, or when `dotglob` is set.
                // Even with `dotglob`, `.` and `..` are never matched by a glob.
                let allow_dot = dotglob || glob_starts_with_dot(&toks);
                let Ok(rd) = std::fs::read_dir(dir) else {
                    continue;
                };
                let mut names: Vec<String> = Vec::new();
                for ent in rd.flatten() {
                    let name = ent.file_name().to_string_lossy().into_owned();
                    let nch: Vec<char> = name.chars().collect();
                    if nch.first() == Some(&'.') && !allow_dot {
                        continue;
                    }
                    if dotglob && !glob_starts_with_dot(&toks) && (name == "." || name == "..") {
                        continue;
                    }
                    let matched = match &toks_ci {
                        Some(tci) => {
                            let low: Vec<char> =
                                nch.iter().map(|c| c.to_ascii_lowercase()).collect();
                            match_glob_toks(tci, &low)
                        }
                        None => match_glob_toks(&toks, &nch),
                    };
                    if matched {
                        names.push(name);
                    }
                }
                names.sort();
                for name in names {
                    next.push(join_glob(base, &name));
                }
            } else {
                let joined = join_glob(base, &comp_literal);
                if std::path::Path::new(&joined).exists() {
                    next.push(joined);
                }
            }
        }
        cands = next;
    }
    cands
}

/// Whether a path component is the globstar token `**` (both characters
/// unquoted). Only meaningful when `shopt -s globstar` is set.
fn is_globstar_comp(comp: &[EChar]) -> bool {
    comp.len() == 2 && comp.iter().all(|e| e.c == '*' && !e.quoted)
}

/// Expand a `**` (globstar) component under `base`. When `terminal` (the last
/// path component), appends every descendant file and directory of `base`;
/// otherwise appends `base` itself (the zero-levels case) plus every descendant
/// directory — the candidate directories for the following component. Dotfiles
/// are skipped unless `dotglob`. Symlinked directories are not recursed into
/// (matching bash ≥ 4.3), which also prevents symlink-loop infinite recursion.
fn globstar_walk(base: &str, dotglob: bool, terminal: bool, out: &mut Vec<String>) {
    if !terminal && (base.is_empty() || std::path::Path::new(base).is_dir()) {
        out.push(base.to_string());
    }
    globstar_descend(base, dotglob, terminal, out);
}

/// Recursive worker for [`globstar_walk`]: descends `base`, pushing matching
/// descendants. In terminal mode every entry is pushed; otherwise only
/// directories (which are also the ones recursed into).
fn globstar_descend(base: &str, dotglob: bool, terminal: bool, out: &mut Vec<String>) {
    let dir = if base.is_empty() { "." } else { base };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<(String, bool)> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && !dotglob {
            continue;
        }
        let is_dir = ent.file_type().is_ok_and(|t| t.is_dir());
        entries.push((name, is_dir));
    }
    for (name, is_dir) in entries {
        let path = join_glob(base, &name);
        if terminal || is_dir {
            out.push(path.clone());
        }
        if is_dir {
            globstar_descend(&path, dotglob, terminal, out);
        }
    }
}

/// Join a base path and a component with a single `/` separator, preserving a
/// leading-`/` (absolute) base and cwd-relative (empty) base.
fn join_glob(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if base == "/" {
        format!("/{name}")
    } else if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// Whether every part of a word is quoted (single- or double-quoted). Used by
/// `[[ … == … ]]` to decide whether the right-hand side is a literal string
/// (fully quoted) or a glob pattern (any unquoted part).
fn word_is_all_quoted(w: &Word) -> bool {
    !w.parts.is_empty()
        && w.parts
            .iter()
            .all(|p| matches!(p, WordPart::SingleQuoted(_) | WordPart::DoubleQuoted(_)))
}

/// Case-aware glob match. When `ci` is set (`shopt -s nocasematch`), both the
/// pattern and the text are lowercased before matching — including
/// character-class ranges (`[A-Z]` → `[a-z]`), which gives the case-folded
/// semantics bash applies to `case`/`[[ == ]]`. `extglob` enables the extended
/// pattern operators.
fn glob_match_ci(pattern: &[char], text: &[char], ci: bool, extglob: bool) -> bool {
    if ci {
        let p: Vec<char> = pattern.iter().flat_map(|c| c.to_lowercase()).collect();
        let t: Vec<char> = text.iter().flat_map(|c| c.to_lowercase()).collect();
        glob_match(&p, &t, extglob)
    } else {
        glob_match(pattern, text, extglob)
    }
}

/// Match `text` against a shell glob `pattern` (`*`, `?`, `[...]`, and — when
/// `extglob` is set — `?()`/`*()`/`+()`/`@()`/`!()`), anchored at both ends (as
/// `case` patterns and `[[ … == … ]]` require). The pattern chars are treated as
/// unquoted (quoting is resolved before this point) and compiled to the same
/// [`PatTok`] engine used for pathname expansion.
fn glob_match(pattern: &[char], text: &[char], extglob: bool) -> bool {
    let comp: Vec<EChar> = pattern
        .iter()
        .map(|&c| EChar { c, quoted: false })
        .collect();
    let toks = compile_glob(&comp, extglob);
    match_glob_toks(&toks, text)
}

/// Longest match of `pattern` starting at `text[start]`; returns the end index
/// (exclusive) of the match, or `None`. Used by `${…/…/…}` substitution.
fn glob_match_at(pattern: &[char], text: &[char], start: usize, extglob: bool) -> Option<usize> {
    for j in (start..=text.len()).rev() {
        if glob_match(pattern, &text[start..j], extglob) {
            return Some(j);
        }
    }
    None
}

/// `${name#pat}` / `${name##pat}` / `${name%pat}` / `${name%%pat}`.
fn param_trim(value: &str, pattern: &[char], suffix: bool, longest: bool, extglob: bool) -> String {
    let v: Vec<char> = value.chars().collect();
    if suffix {
        // Remove a matching suffix `v[k..]`, keeping `v[..k]`. Shortest match =
        // largest k; longest match = smallest k.
        let range: Vec<usize> = if longest {
            (0..=v.len()).collect()
        } else {
            (0..=v.len()).rev().collect()
        };
        for k in range {
            if glob_match(pattern, &v[k..], extglob) {
                return v[..k].iter().collect();
            }
        }
    } else {
        // Remove a matching prefix `v[..k]`, keeping `v[k..]`. Shortest match =
        // smallest k; longest match = largest k.
        let range: Vec<usize> = if longest {
            (0..=v.len()).rev().collect()
        } else {
            (0..=v.len()).collect()
        };
        for k in range {
            if glob_match(pattern, &v[..k], extglob) {
                return v[k..].iter().collect();
            }
        }
    }
    value.to_string()
}

/// `${name^pat}` / `${name^^pat}` (upper) / `${name,pat}` / `${name,,pat}`
/// (lower) — case modification. `pattern` selects which characters convert (a
/// glob matched against a single character); an empty pattern matches any
/// character. `all` converts every matching character; otherwise only the
/// first character of the value is considered (and only converted if it
/// matches `pattern`).
fn param_case(value: &str, pattern: &[char], upper: bool, all: bool, extglob: bool) -> String {
    // An empty pattern matches every character (bash: `^^`/`,,` with no
    // pattern uppercases/lowercases the whole value).
    let matches_char = |ch: char| pattern.is_empty() || glob_match(pattern, &[ch], extglob);
    let convert = |ch: char| {
        if upper {
            // `char::to_uppercase`/`to_lowercase` can yield multiple chars
            // (e.g. 'ß' → "SS"); bash uses towupper/towlower per rune, but the
            // multi-char expansion is the closest correct Unicode behavior.
            ch.to_uppercase().collect::<String>()
        } else {
            ch.to_lowercase().collect::<String>()
        }
    };
    let mut out = String::with_capacity(value.len());
    let mut done = false;
    for ch in value.chars() {
        if !done && matches_char(ch) {
            out.push_str(&convert(ch));
            if !all {
                done = true;
            }
        } else {
            out.push(ch);
            if !all {
                // For the single-char form only the first character is
                // eligible; everything after is copied verbatim.
                done = true;
            }
        }
    }
    out
}

/// Current Unix time as `(seconds, microseconds)`. Falls back to `(0, 0)` if
/// the system clock is before the epoch (should not happen).
fn unix_time() -> (u64, u32) {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or((0, 0), |d| (d.as_secs(), d.subsec_micros()))
}

/// A nonzero seed for `$RANDOM`, derived from the wall clock.
fn initial_rng_seed() -> u32 {
    let (secs, micros) = unix_time();
    let mixed = (secs as u32).wrapping_mul(1_000_003).wrapping_add(micros);
    if mixed == 0 { 0x2545_F491 } else { mixed }
}

/// Quote `s` so it can be reused verbatim as shell input (the `${v@Q}`
/// transform). Simple safe words are returned unquoted; values with control
/// characters use ANSI-C `$'…'` quoting; everything else is single-quoted with
/// embedded single quotes escaped as `'\''`.
/// The signals `trap`/`trap -l` know about, as `(number, name-without-SIG)`.
/// Numbers follow the common Linux x86 layout. `EXIT` (0) and the pseudo
/// signals `ERR`/`DEBUG`/`RETURN` are handled as specs but not listed here.
const SIGNALS: &[(u8, &str)] = &[
    (1, "HUP"),
    (2, "INT"),
    (3, "QUIT"),
    (4, "ILL"),
    (5, "TRAP"),
    (6, "ABRT"),
    (7, "BUS"),
    (8, "FPE"),
    (9, "KILL"),
    (10, "USR1"),
    (11, "SEGV"),
    (12, "USR2"),
    (13, "PIPE"),
    (14, "ALRM"),
    (15, "TERM"),
    (16, "STKFLT"),
    (17, "CHLD"),
    (18, "CONT"),
    (19, "STOP"),
    (20, "TSTP"),
    (21, "TTIN"),
    (22, "TTOU"),
    (23, "URG"),
    (24, "XCPU"),
    (25, "XFSZ"),
    (26, "VTALRM"),
    (27, "PROF"),
    (28, "WINCH"),
    (29, "IO"),
    (30, "PWR"),
    (31, "SYS"),
];

/// Normalize a `trap` signal spec to a canonical name (`EXIT`, `ERR`, `INT`, …).
/// Accepts case-insensitive names with or without a `SIG` prefix, the pseudo
/// signals `EXIT`/`ERR`/`DEBUG`/`RETURN`, and signal numbers (`0` = `EXIT`).
/// Returns `None` for an unrecognized spec.
fn normalize_sigspec(spec: &str) -> Option<String> {
    if let Ok(n) = spec.parse::<u8>() {
        if n == 0 {
            return Some("EXIT".to_string());
        }
        return SIGNALS
            .iter()
            .find(|(num, _)| *num == n)
            .map(|(_, name)| (*name).to_string());
    }
    let upper = spec.to_ascii_uppercase();
    let bare = upper.strip_prefix("SIG").unwrap_or(&upper);
    if matches!(bare, "EXIT" | "ERR" | "DEBUG" | "RETURN") {
        return Some(bare.to_string());
    }
    SIGNALS
        .iter()
        .find(|(_, name)| *name == bare)
        .map(|(_, name)| (*name).to_string())
}

/// Sort key placing `EXIT` first, then real signals by number, then the other
/// pseudo signals — used to order `trap -p` output deterministically.
fn sigspec_order(spec: &str) -> u16 {
    match spec {
        "EXIT" => 0,
        "DEBUG" => 200,
        "RETURN" => 201,
        "ERR" => 202,
        _ => SIGNALS
            .iter()
            .find(|(_, name)| *name == spec)
            .map_or(255, |(num, _)| u16::from(*num)),
    }
}

/// Wrap `s` in single quotes for `trap -p` output, escaping embedded quotes the
/// POSIX way (`'\''`). Always quotes (even simple words), matching bash.
fn single_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars().any(char::is_control) {
        let mut out = String::from("$'");
        for c in s.chars() {
            match c {
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("\\'"),
                c if c.is_control() => out.push_str(&format!("\\x{:02x}", u32::from(c))),
                c => out.push(c),
            }
        }
        out.push('\'');
        return out;
    }
    let safe = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_./,:+-=@%^".contains(c));
    if safe {
        return s.to_string();
    }
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// Expand ANSI-C backslash escapes in `s` (the `${v@E}` transform): the common
/// `\n \t \r \\ \' \" \a \b \e \f \v` escapes, plus `\xHH` and `\0nnn`/`\nnn`
/// numeric escapes. An unrecognized escape keeps its backslash.
fn ansi_c_unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('a') => out.push('\u{07}'),
            Some('b') => out.push('\u{08}'),
            Some('e' | 'E') => out.push('\u{1b}'),
            Some('f') => out.push('\u{0c}'),
            Some('v') => out.push('\u{0b}'),
            Some('x') => {
                let mut hex = String::new();
                while hex.len() < 2 && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    hex.push(chars.next().unwrap_or('0'));
                }
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) {
                        out.push(ch);
                    }
                } else {
                    out.push('\\');
                    out.push('x');
                }
            }
            Some(d @ '0'..='7') => {
                let mut oct = String::from(d);
                while oct.len() < 3 && chars.peek().is_some_and(|c| ('0'..='7').contains(c)) {
                    oct.push(chars.next().unwrap_or('0'));
                }
                if let Ok(n) = u32::from_str_radix(&oct, 8)
                    && let Some(ch) = char::from_u32(n)
                {
                    out.push(ch);
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `${name:offset[:length]}` — a negative offset counts from the end; a negative
/// length is an offset from the end.
fn param_substr(value: &str, offset: i64, length: Option<i64>) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len() as i64;
    let mut start = offset;
    if start < 0 {
        start += n;
    }
    start = start.clamp(0, n);
    let end = match length {
        None => n,
        Some(len) if len < 0 => (n + len).max(start),
        Some(len) => (start + len).min(n),
    };
    let end = end.clamp(start, n);
    chars[start as usize..end as usize].iter().collect()
}

/// `${name/pat/repl}` and friends.
fn param_replace(
    value: &str,
    pattern: &[char],
    replacement: &str,
    all: bool,
    anchor: ReplaceAnchor,
    extglob: bool,
) -> String {
    let v: Vec<char> = value.chars().collect();
    match anchor {
        ReplaceAnchor::Start => {
            if let Some(end) = glob_match_at(pattern, &v, 0, extglob) {
                let mut s = replacement.to_string();
                s.extend(v[end..].iter());
                return s;
            }
            value.to_string()
        }
        ReplaceAnchor::End => {
            for i in 0..=v.len() {
                if glob_match(pattern, &v[i..], extglob) {
                    let mut s: String = v[..i].iter().collect();
                    s.push_str(replacement);
                    return s;
                }
            }
            value.to_string()
        }
        ReplaceAnchor::None => {
            let mut result = String::new();
            let mut i = 0;
            let mut done = false;
            while i < v.len() {
                let can_replace = !done || all;
                if can_replace
                    && let Some(end) = glob_match_at(pattern, &v, i, extglob)
                    && end > i
                {
                    result.push_str(replacement);
                    i = end;
                    done = true;
                    continue;
                }
                result.push(v[i]);
                i += 1;
            }
            result
        }
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        ":" | "true"
            | "false"
            | "cd"
            | "pwd"
            | "pushd"
            | "popd"
            | "dirs"
            | "echo"
            | "printf"
            | "export"
            | "declare"
            | "typeset"
            | "local"
            | "readonly"
            | "shopt"
            | "unset"
            | "set"
            | "shift"
            | "getopts"
            | "mapfile"
            | "readarray"
            | "command"
            | "builtin"
            | "read"
            | "test"
            | "["
            | "let"
            | "eval"
            | "source"
            | "."
            | "type"
            | "trap"
            | "jobs"
            | "wait"
            | "exec"
            | "exit"
            | "return"
            | "break"
            | "continue"
    )
}

fn open_out(path: &str, append: bool) -> io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true);
    if append {
        opts.append(true);
    } else {
        opts.truncate(true);
    }
    opts.open(path)
}

fn read_one_line<R: BufRead>(r: &mut R) -> Option<String> {
    let mut line = String::new();
    let n = r.read_line(&mut line).ok()?;
    if n == 0 {
        return None;
    }
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Some(line)
}

/// Read a record for `read -d`/`-n`/`-N`. Reads byte-by-byte so streaming
/// pipes yield data as produced. `delim` terminates the record (consumed, not
/// stored) unless `exact` is set. `nchars` caps the record at that many
/// *characters* (UTF-8 aware: a byte begins a new character when it is not a
/// `10xxxxxx` continuation byte). `exact` (`-N`) ignores `delim`. Returns
/// `(text, terminated)`; `None` on immediate EOF with no bytes read.
fn read_record<R: BufRead>(
    r: &mut R,
    delim: u8,
    nchars: Option<usize>,
    exact: bool,
) -> Option<(String, bool)> {
    let mut bytes: Vec<u8> = Vec::new();
    let mut chars = 0usize;
    let mut hit_delim = false;
    let mut any = false;
    loop {
        // Peek at the next byte without holding the borrow across `consume`.
        let b = {
            let buf = match r.fill_buf() {
                Ok(b) if !b.is_empty() => b,
                _ => break, // EOF or read error
            };
            buf[0]
        };
        let is_char_start = b & 0xC0 != 0x80;
        // Stop once the character limit is reached, at the next char boundary.
        if let Some(n) = nchars
            && is_char_start
            && chars >= n
        {
            hit_delim = true; // full requested count read
            break;
        }
        // `-n` (not `-N`) also stops at the delimiter.
        if !exact && b == delim {
            r.consume(1);
            hit_delim = true;
            any = true;
            break;
        }
        r.consume(1);
        any = true;
        bytes.push(b);
        if is_char_start {
            chars += 1;
        }
    }
    if !any && bytes.is_empty() {
        return None;
    }
    Some((String::from_utf8_lossy(&bytes).into_owned(), hit_delim))
}

/// Quote a value for a `declare`/`readonly -p` listing: wrap in double quotes
/// and backslash-escape the characters that are special inside double quotes
/// (`"`, `\`, `$`, and backtick), matching bash's re-inputtable output.
fn quote_declare_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    for c in v.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Split a string on the default IFS (whitespace), dropping empty fields.
fn split_ifs(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// A special shell parameter that is always considered "set" for `nounset`
/// purposes (referencing it never yields an unbound-variable error).
fn is_special_param(name: &str) -> bool {
    matches!(name, "@" | "*" | "#" | "?" | "$" | "!" | "0" | "-" | "_")
}

/// Remove `read`'s backslash escapes from a whole line (non-`-r` mode): a
/// backslash makes the following character literal.
fn unescape_read_line(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Split a line the way the `read` builtin does: on `$IFS`, distinguishing
/// IFS-whitespace (space/tab/newline — runs collapse, and leading/trailing are
/// trimmed) from non-whitespace IFS characters (each a single delimiter). When
/// `limit` is `Some(n)`, at most `n` fields are produced and the last captures
/// the raw remainder (trailing IFS-whitespace stripped) — matching bash's
/// assignment of the rest of the line to the final variable. Without `raw`, a
/// backslash escapes the next character (so it neither delimits nor is dropped
/// from the field boundary logic).
fn read_split(line: &str, ifs: &str, raw: bool, limit: Option<usize>) -> Vec<String> {
    // Empty IFS disables splitting entirely: the whole line is one field.
    if ifs.is_empty() {
        let whole = if raw { line.to_string() } else { unescape_read_line(line) };
        return vec![whole];
    }
    let ws: Vec<char> = ifs.chars().filter(|c| matches!(c, ' ' | '\t' | '\n')).collect();
    let other: Vec<char> = ifs.chars().filter(|c| !matches!(c, ' ' | '\t' | '\n')).collect();
    let is_ws = |c: char| ws.contains(&c);
    let is_other = |c: char| other.contains(&c);

    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut fields: Vec<String> = Vec::new();
    let mut i = 0;
    // Trim leading IFS whitespace.
    while i < n && is_ws(chars[i]) {
        i += 1;
    }
    while i < n {
        // Last allowed field: take the raw remainder (trailing IFS-ws trimmed).
        if let Some(lim) = limit
            && fields.len() + 1 == lim
        {
            let mut end = n;
            while end > i && is_ws(chars[end - 1]) {
                end -= 1;
            }
            let seg: String = chars[i..end].iter().collect();
            fields.push(if raw { seg } else { unescape_read_line(&seg) });
            return fields;
        }
        // Accumulate one field up to the next delimiter.
        let mut field = String::new();
        while i < n {
            let c = chars[i];
            if !raw && c == '\\' && i + 1 < n {
                field.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if is_ws(c) {
                // Consume the whole run of IFS whitespace.
                while i < n && is_ws(chars[i]) {
                    i += 1;
                }
                break;
            }
            if is_other(c) {
                i += 1;
                break;
            }
            field.push(c);
            i += 1;
        }
        fields.push(field);
    }
    if let Some(lim) = limit {
        while fields.len() < lim {
            fields.push(String::new());
        }
    }
    fields
}

/// Minimal `printf`: handles `%s`, `%d`, `%%`, and common backslash escapes.
fn format_printf(fmt: &str, args: &[String]) -> String {
    // Bash reuses the format string until all arguments are consumed. Repeat the
    // format while arguments remain, stopping if a pass consumes none (the
    // format has no argument-consuming conversions) to avoid an infinite loop.
    let mut out = String::new();
    let mut arg_i = 0;
    loop {
        let start = arg_i;
        out.push_str(&format_printf_once(fmt, args, &mut arg_i));
        if arg_i >= args.len() || arg_i == start {
            break;
        }
    }
    out
}

fn format_printf_once(fmt: &str, args: &[String], arg_i: &mut usize) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('a') => out.push('\x07'),
                Some('b') => out.push('\x08'),
                Some('f') => out.push('\x0c'),
                Some('v') => out.push('\x0b'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            '%' => format_conversion(&mut chars, args, arg_i, &mut out),
            other => out.push(other),
        }
    }
    out
}

/// Parse and render a single `%…` printf conversion. `chars` is positioned just
/// after the `%`. Supports flags (`-+ #0`), width and precision (numeric or `*`
/// dynamic from an argument), and the conversions `% s d i u x X o c b q f e g E G`.
fn format_conversion(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    args: &[String],
    arg_i: &mut usize,
    out: &mut String,
) {
    // Literal `%%` short-circuit (no flags/width may precede it).
    if chars.peek() == Some(&'%') {
        chars.next();
        out.push('%');
        return;
    }

    // Collect flags.
    let mut spec = String::from("%");
    let mut left = false;
    let mut zero = false;
    while let Some(&c) = chars.peek() {
        match c {
            '-' => left = true,
            '0' => zero = true,
            '+' | ' ' | '#' => {}
            _ => break,
        }
        spec.push(c);
        chars.next();
    }
    // Width. A `*` takes the width from the next argument (bash: a negative
    // dynamic width means left-justify with the absolute magnitude).
    let mut width = String::new();
    let mut star_left = false;
    if chars.peek() == Some(&'*') {
        chars.next();
        let raw = args.get(*arg_i).cloned().unwrap_or_default();
        *arg_i += 1;
        let n = parse_printf_int(&raw);
        if n < 0 {
            star_left = true;
        }
        width = n.unsigned_abs().to_string();
    } else {
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                width.push(c);
                chars.next();
            } else {
                break;
            }
        }
    }
    if star_left {
        left = true;
    }
    // Precision. A `*` takes the precision from the next argument; a negative
    // dynamic precision is treated as if no precision were given (bash/C).
    let mut prec: Option<String> = None;
    if chars.peek() == Some(&'.') {
        chars.next();
        if chars.peek() == Some(&'*') {
            chars.next();
            let raw = args.get(*arg_i).cloned().unwrap_or_default();
            *arg_i += 1;
            let n = parse_printf_int(&raw);
            if n >= 0 {
                prec = Some(n.to_string());
            }
        } else {
            let mut p = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    p.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            prec = Some(p);
        }
    }

    let width_n: usize = width.parse().unwrap_or(0);
    let prec_n: Option<usize> = prec.as_ref().map(|p| p.parse().unwrap_or(0));

    // `%(FORMAT)T` — strftime-style time conversion. The parenthesised format
    // occupies the position of the conversion character and is followed by `T`.
    // It consumes one argument: seconds since the Unix epoch (missing, empty, or
    // a negative value ⇒ the current time; bash's `-2` "shell start" is
    // approximated as now here). Time is rendered in UTC.
    if chars.peek() == Some(&'(') {
        chars.next();
        let mut tfmt = String::new();
        let mut depth = 1usize;
        for c in chars.by_ref() {
            match c {
                '(' => {
                    depth += 1;
                    tfmt.push(c);
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    tfmt.push(c);
                }
                _ => tfmt.push(c),
            }
        }
        // Consume the trailing `T` conversion letter if present.
        if chars.peek() == Some(&'T') {
            chars.next();
        }
        let secs: i64 = {
            let has_arg = args.get(*arg_i).is_some();
            if has_arg {
                let raw = args.get(*arg_i).cloned().unwrap_or_default();
                *arg_i += 1;
                let n = parse_printf_int(&raw);
                #[allow(clippy::cast_possible_wrap)]
                if n < 0 { unix_time().0 as i64 } else { n }
            } else {
                #[allow(clippy::cast_possible_wrap)]
                {
                    unix_time().0 as i64
                }
            }
        };
        let rendered = format_strftime(&tfmt, secs);
        // String-style field-width padding (never zero-padded).
        let len = rendered.chars().count();
        if len < width_n {
            let pad = width_n - len;
            if left {
                out.push_str(&rendered);
                out.extend(std::iter::repeat_n(' ', pad));
            } else {
                out.extend(std::iter::repeat_n(' ', pad));
                out.push_str(&rendered);
            }
        } else {
            out.push_str(&rendered);
        }
        return;
    }

    let Some(conv) = chars.next() else {
        // Trailing bare `%…` with no conversion: emit literally.
        out.push_str(&spec);
        out.push_str(&width);
        if let Some(p) = &prec {
            out.push('.');
            out.push_str(p);
        }
        return;
    };

    let next_arg = |arg_i: &mut usize| -> String {
        let v = args.get(*arg_i).cloned().unwrap_or_default();
        *arg_i += 1;
        v
    };

    let rendered = match conv {
        's' => {
            let mut s = next_arg(arg_i);
            if let Some(p) = prec_n {
                s.truncate(p);
            }
            s
        }
        'b' => {
            // Interpret backslash escapes in the argument.
            ansi_c_unescape(&next_arg(arg_i))
        }
        'q' => shell_quote(&next_arg(arg_i)),
        'c' => next_arg(arg_i).chars().next().map_or(String::new(), |c| c.to_string()),
        'd' | 'i' => {
            let n = parse_printf_int(&next_arg(arg_i));
            n.to_string()
        }
        'u' => parse_printf_int(&next_arg(arg_i)).cast_unsigned().to_string(),
        'x' => format!("{:x}", parse_printf_int(&next_arg(arg_i)).cast_unsigned()),
        'X' => format!("{:X}", parse_printf_int(&next_arg(arg_i)).cast_unsigned()),
        'o' => format!("{:o}", parse_printf_int(&next_arg(arg_i)).cast_unsigned()),
        'f' | 'F' => {
            let f = parse_printf_float(&next_arg(arg_i));
            format!("{:.*}", prec_n.unwrap_or(6), f)
        }
        'e' | 'E' => {
            let f = parse_printf_float(&next_arg(arg_i));
            let s = format!("{:.*e}", prec_n.unwrap_or(6), f);
            let s = normalize_exp(&s);
            if conv == 'E' { s.to_uppercase() } else { s }
        }
        'g' | 'G' => {
            let f = parse_printf_float(&next_arg(arg_i));
            let s = format!("{f}");
            if conv == 'G' { s.to_uppercase() } else { s }
        }
        other => {
            // Unknown conversion: emit literally.
            let mut s = spec.clone();
            s.push_str(&width);
            if let Some(p) = &prec {
                s.push('.');
                s.push_str(p);
            }
            s.push(other);
            out.push_str(&s);
            return;
        }
    };

    // Apply field width padding (numeric width only).
    if rendered.chars().count() < width_n {
        let pad = width_n - rendered.chars().count();
        if left {
            out.push_str(&rendered);
            out.extend(std::iter::repeat_n(' ', pad));
        } else {
            // Zero-pad only for numeric conversions.
            let pad_ch = if zero && matches!(conv, 'd' | 'i' | 'u' | 'x' | 'X' | 'o' | 'f' | 'F' | 'e' | 'E' | 'g' | 'G') {
                '0'
            } else {
                ' '
            };
            out.extend(std::iter::repeat_n(pad_ch, pad));
            out.push_str(&rendered);
        }
    } else {
        out.push_str(&rendered);
    }
}

/// Parse an integer argument for printf, tolerating leading/trailing whitespace,
/// a leading `0x`/`0` base prefix, and a leading `'c` character-code form.
/// Convert a day count relative to 1970-01-01 into a civil `(year, month,
/// day)`. Uses Howard Hinnant's `civil_from_days` algorithm (valid for the
/// full proleptic Gregorian range).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m as u32, d)
}

/// Inverse of [`civil_from_days`]: day count relative to 1970-01-01 for a
/// civil date. Used to derive the day-of-year.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mi = i64::from(m);
    let doy = (153 * (if m > 2 { mi - 3 } else { mi + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Render a `strftime`-style format for `printf '%(FORMAT)T'`. `epoch` is
/// seconds since the Unix epoch; the broken-down time is computed in **UTC**
/// (SlateOS has no timezone database — see known-issues TD-OILS). Supports the
/// common specifiers `%Y %C %y %m %d %e %H %I %M %S %p %P %A %a %B %b %h %j %u
/// %w %s %n %t %F %T %R %D %%`; an unknown `%x` is emitted verbatim.
fn format_strftime(fmt: &str, epoch: i64) -> String {
    const WDAY_FULL: [&str; 7] = [
        "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
    ];
    const WDAY_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MON_FULL: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September",
        "October", "November", "December",
    ];
    const MON_ABBR: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let days = epoch.div_euclid(86_400);
    let rem = epoch.rem_euclid(86_400);
    let hour = (rem / 3600) as u32;
    let minute = ((rem % 3600) / 60) as u32;
    let second = (rem % 60) as u32;
    let (year, month, day) = civil_from_days(days);
    // 1970-01-01 was a Thursday (index 4 with Sunday = 0).
    let wday = (((days + 4) % 7 + 7) % 7) as usize;
    let yday = days - days_from_civil(year, 1, 1) + 1;
    let mon_i = (month.max(1) - 1) as usize;

    // Render one specifier letter to `out`. `%F`/`%T`/`%R`/`%D` recurse.
    fn emit(out: &mut String, c: char, ctx: &StrftimeCtx) {
        match c {
            'Y' => out.push_str(&ctx.year.to_string()),
            'C' => out.push_str(&format!("{:02}", ctx.year.div_euclid(100))),
            'y' => out.push_str(&format!("{:02}", ctx.year.rem_euclid(100))),
            'm' => out.push_str(&format!("{:02}", ctx.month)),
            'd' => out.push_str(&format!("{:02}", ctx.day)),
            'e' => out.push_str(&format!("{:2}", ctx.day)),
            'H' => out.push_str(&format!("{:02}", ctx.hour)),
            'I' => {
                let h12 = match ctx.hour % 12 {
                    0 => 12,
                    h => h,
                };
                out.push_str(&format!("{h12:02}"));
            }
            'M' => out.push_str(&format!("{:02}", ctx.minute)),
            'S' => out.push_str(&format!("{:02}", ctx.second)),
            'p' => out.push_str(if ctx.hour < 12 { "AM" } else { "PM" }),
            'P' => out.push_str(if ctx.hour < 12 { "am" } else { "pm" }),
            'A' => out.push_str(ctx.wday_full),
            'a' => out.push_str(ctx.wday_abbr),
            'B' => out.push_str(ctx.mon_full),
            'b' | 'h' => out.push_str(ctx.mon_abbr),
            'j' => out.push_str(&format!("{:03}", ctx.yday)),
            'u' => out.push_str(&(if ctx.wday == 0 { 7 } else { ctx.wday }).to_string()),
            'w' => out.push_str(&ctx.wday.to_string()),
            's' => out.push_str(&ctx.epoch.to_string()),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            '%' => out.push('%'),
            'F' => {
                for k in ['Y', '-', 'm', '-', 'd'] {
                    if k == '-' {
                        out.push('-');
                    } else {
                        emit(out, k, ctx);
                    }
                }
            }
            'T' => {
                emit(out, 'H', ctx);
                out.push(':');
                emit(out, 'M', ctx);
                out.push(':');
                emit(out, 'S', ctx);
            }
            'R' => {
                emit(out, 'H', ctx);
                out.push(':');
                emit(out, 'M', ctx);
            }
            'D' => {
                emit(out, 'm', ctx);
                out.push('/');
                emit(out, 'd', ctx);
                out.push('/');
                emit(out, 'y', ctx);
            }
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }

    let ctx = StrftimeCtx {
        year,
        month,
        day,
        hour,
        minute,
        second,
        wday,
        yday,
        epoch,
        wday_full: WDAY_FULL[wday],
        wday_abbr: WDAY_ABBR[wday],
        mon_full: MON_FULL[mon_i],
        mon_abbr: MON_ABBR[mon_i],
    };
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some(sp) => emit(&mut out, sp, &ctx),
                None => out.push('%'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Broken-down UTC time plus preformatted name strings, passed to the
/// `strftime` specifier renderer.
struct StrftimeCtx {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    wday: usize,
    yday: i64,
    epoch: i64,
    wday_full: &'static str,
    wday_abbr: &'static str,
    mon_full: &'static str,
    mon_abbr: &'static str,
}

fn parse_printf_int(s: &str) -> i64 {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix('\'').or_else(|| t.strip_prefix('"')) {
        return rest.chars().next().map_or(0, |c| i64::from(u32::from(c)));
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).unwrap_or(0);
    }
    if let Some(hex) = t.strip_prefix("-0x").or_else(|| t.strip_prefix("-0X")) {
        return i64::from_str_radix(hex, 16).map(|n| -n).unwrap_or(0);
    }
    t.parse::<i64>().unwrap_or(0)
}

fn parse_printf_float(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

/// Rust formats exponents as `1.5e2`; C/bash use `1.5e+02`. Normalize to the
/// C-style two-digit signed exponent.
fn normalize_exp(s: &str) -> String {
    if let Some(pos) = s.find('e') {
        let (mant, exp) = s.split_at(pos);
        let exp = &exp[1..];
        let (sign, digits) = if let Some(d) = exp.strip_prefix('-') {
            ('-', d)
        } else if let Some(d) = exp.strip_prefix('+') {
            ('+', d)
        } else {
            ('+', exp)
        };
        format!("{mant}e{sign}{digits:0>2}")
    } else {
        s.to_string()
    }
}

/// Evaluate a `test`/`[` expression. Returns the boolean result (true = success).
fn eval_test(a: &[&str]) -> bool {
    match a.len() {
        0 => false,
        1 => !a[0].is_empty(),
        2 => {
            // Unary operator.
            let (op, x) = (a[0], a[1]);
            if op == "!" {
                return x.is_empty();
            }
            eval_unary(op, x)
        }
        3 => {
            let (l, op, r) = (a[0], a[1], a[2]);
            if op == "!" {
                // `! op x` handled as negation of a 2-arg test.
                return !eval_test(&a[1..]);
            }
            eval_binary(l, op, r)
        }
        _ => {
            // Handle a leading `!`; otherwise fall back to the first 3 args.
            if a[0] == "!" {
                !eval_test(&a[1..])
            } else {
                eval_binary(a[0], a[1], a[2])
            }
        }
    }
}

fn eval_unary(op: &str, x: &str) -> bool {
    match op {
        "-z" => x.is_empty(),
        "-n" => !x.is_empty(),
        "-e" => std::path::Path::new(x).exists(),
        "-f" => std::path::Path::new(x).is_file(),
        "-d" => std::path::Path::new(x).is_dir(),
        "-s" => std::fs::metadata(x).map(|m| m.len() > 0).unwrap_or(false),
        "-r" | "-w" | "-x" => std::path::Path::new(x).exists(),
        _ => !x.is_empty(),
    }
}

fn eval_binary(l: &str, op: &str, r: &str) -> bool {
    match op {
        "=" | "==" => l == r,
        "!=" => l != r,
        "<" => l < r,
        ">" => l > r,
        "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
            let (Ok(a), Ok(b)) = (l.parse::<i64>(), r.parse::<i64>()) else {
                return false;
            };
            match op {
                "-eq" => a == b,
                "-ne" => a != b,
                "-lt" => a < b,
                "-le" => a <= b,
                "-gt" => a > b,
                "-ge" => a >= b,
                _ => false,
            }
        }
        "-nt" | "-ot" | "-ef" => file_cmp(op, l, r),
        _ => false,
    }
}

/// File-comparison test operators shared by `test`/`[` and `[[ … ]]`:
/// `-nt` (newer-than), `-ot` (older-than), `-ef` (same file).
///
/// `-nt`/`-ot` compare modification times, with bash's existence rule: `a -nt b`
/// is also true when `a` exists and `b` does not (and symmetrically for `-ot`).
/// `-ef` compares canonicalized paths — the portable stand-in for a
/// device+inode match, which the standard library does not expose across our
/// host and target (true hard links to *different* names are not detected; see
/// known-issues TD-OILS12).
fn file_cmp(op: &str, l: &str, r: &str) -> bool {
    let lmtime = std::fs::metadata(l).and_then(|m| m.modified()).ok();
    let rmtime = std::fs::metadata(r).and_then(|m| m.modified()).ok();
    match op {
        "-nt" => match (lmtime, rmtime) {
            (Some(a), Some(b)) => a > b,
            (Some(_), None) => true,
            _ => false,
        },
        "-ot" => match (lmtime, rmtime) {
            (Some(a), Some(b)) => a < b,
            (None, Some(_)) => true,
            _ => false,
        },
        "-ef" => match (std::fs::canonicalize(l), std::fs::canonicalize(r)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that read or mutate the process-global current
    /// working directory. Tests that call `set_current_dir` (the directory-
    /// stack test) and tests that create/glob cwd-relative paths must all
    /// hold this lock so a cwd change in one never races another.
    fn cwd_guard() -> std::sync::MutexGuard<'static, ()> {
        static CWD_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        CWD_LOCK
            .get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn run(src: &str) -> (String, i32) {
        // Capture stdout by running through command-substitution-style capture.
        let mut sh = Shell::new();
        let mut buf = Vec::new();
        let prog = parse(src).expect("parse");
        {
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
        }
        (String::from_utf8_lossy(&buf).into_owned(), sh.last_status)
    }

    #[test]
    fn echo_and_status() {
        let (o, s) = run("echo hello");
        assert_eq!(o, "hello\n");
        assert_eq!(s, 0);
    }

    #[test]
    fn variables_and_expansion() {
        let (o, _) = run("x=world; echo hi $x");
        assert_eq!(o, "hi world\n");
    }

    #[test]
    fn param_default() {
        let (o, _) = run("echo ${undefined:-fallback}");
        assert_eq!(o, "fallback\n");
    }

    #[test]
    fn arithmetic() {
        let (o, _) = run("echo $((6 * 7))");
        assert_eq!(o, "42\n");
    }

    #[test]
    fn command_substitution() {
        let (o, _) = run("echo [$(echo inner)]");
        assert_eq!(o, "[inner]\n");
    }

    #[test]
    fn if_true() {
        let (o, _) = run("if true; then echo yes; else echo no; fi");
        assert_eq!(o, "yes\n");
    }

    #[test]
    fn for_loop() {
        let (o, _) = run("for x in a b c; do echo $x; done");
        assert_eq!(o, "a\nb\nc\n");
    }

    #[test]
    fn while_with_break() {
        let (o, _) = run("x=0; while true; do echo $x; x=$((x+1)); if [ $x -ge 3 ]; then break; fi; done");
        assert_eq!(o, "0\n1\n2\n");
    }

    #[test]
    fn and_or() {
        let (o, _) = run("true && echo a; false || echo b; false && echo c");
        assert_eq!(o, "a\nb\n");
    }

    #[test]
    fn function_call() {
        let (o, _) = run("greet() { echo hi $1; }; greet there");
        assert_eq!(o, "hi there\n");
    }

    #[test]
    fn test_builtin() {
        let (_, s) = run("[ 3 -gt 2 ]");
        assert_eq!(s, 0);
        let (_, s2) = run("[ 1 -gt 2 ]");
        assert_eq!(s2, 1);
    }

    #[test]
    fn test_v_variable_set() {
        // `[ -v name ]` and `[[ -v name ]]` test whether a variable is set.
        assert_eq!(run("x=1; [ -v x ]").1, 0);
        assert_eq!(run("[ -v x ]").1, 1);
        assert_eq!(run("x=1; [[ -v x ]] && echo yes").0, "yes\n");
        assert_eq!(run("[[ -v missing ]] || echo no").0, "no\n");
        // An empty-but-set variable still counts as set.
        assert_eq!(run("x=; [ -v x ]").1, 0);
    }

    #[test]
    fn test_v_array_element() {
        // Whole array is set; specific element presence honored.
        assert_eq!(run("a=(x y z); [[ -v a ]] && echo arr").0, "arr\n");
        assert_eq!(run("a=(x y z); [[ -v a[1] ]] && echo e1").0, "e1\n");
        assert_eq!(run("a=([5]=x); [[ -v a[2] ]] || echo gap").0, "gap\n");
        assert_eq!(run("declare -A m; m[k]=v; [[ -v m[k] ]] && echo key").0, "key\n");
        assert_eq!(run("declare -A m; [[ -v m[nope] ]] || echo nokey").0, "nokey\n");
    }

    #[test]
    fn length_expansion() {
        let (o, _) = run("x=hello; echo ${#x}");
        assert_eq!(o, "5\n");
    }

    #[test]
    fn negated_pipeline_status() {
        let (_, s) = run("! true");
        assert_eq!(s, 1);
    }

    #[test]
    fn quoted_no_split() {
        let (o, _) = run(r#"x="a b c"; for w in "$x"; do echo $w; done"#);
        assert_eq!(o, "a b c\n");
    }

    #[test]
    fn case_literal_and_glob() {
        let (o, _) = run("case hello in h*) echo star;; *) echo other;; esac");
        assert_eq!(o, "star\n");
        let (o2, _) = run("case foo in a|foo|b) echo alt;; esac");
        assert_eq!(o2, "alt\n");
        let (o3, _) = run("case xyz in a*) echo a;; esac; echo done");
        assert_eq!(o3, "done\n");
    }

    #[test]
    fn case_uses_variable() {
        let (o, _) = run("x=cat.txt; case $x in *.txt) echo text;; *.md) echo md;; esac");
        assert_eq!(o, "text\n");
    }

    #[test]
    fn case_char_class() {
        let (o, _) = run("case 5 in [0-9]) echo digit;; *) echo no;; esac");
        assert_eq!(o, "digit\n");
    }

    #[test]
    fn case_fallthrough_semi_amp() {
        // `;&` runs the next arm's body unconditionally.
        let (o, _) = run("case x in x) echo a ;& y) echo b ;; z) echo c ;; esac");
        assert_eq!(o, "a\nb\n");
    }

    #[test]
    fn case_fallthrough_stops_at_break() {
        // Fall through a chain of `;&` until a `;;` breaks.
        let (o, _) = run("case a in a) echo 1 ;& b) echo 2 ;& c) echo 3 ;; d) echo 4 ;; esac");
        assert_eq!(o, "1\n2\n3\n");
    }

    #[test]
    fn case_continue_match_dsemi_amp() {
        // `;;&` resumes pattern testing; both matching arms run.
        let (o, _) = run("case abc in a*) echo one ;;& *c) echo two ;; *) echo three ;; esac");
        assert_eq!(o, "one\ntwo\n");
    }

    #[test]
    fn case_continue_match_no_second() {
        // `;;&` resumes matching but no later arm matches.
        let (o, _) = run("case abc in a*) echo one ;;& z*) echo two ;; esac; echo done");
        assert_eq!(o, "one\ndone\n");
    }

    #[test]
    fn select_picks_by_number() {
        let (o, _) = run("select x in a b c; do echo \"picked $x\"; break; done <<< \"2\"");
        assert_eq!(o, "picked b\n");
    }

    #[test]
    fn select_invalid_gives_empty() {
        let (o, _) = run("select x in a b; do echo \"got=$x\"; break; done <<< \"9\"");
        assert_eq!(o, "got=\n");
    }

    #[test]
    fn select_sets_reply() {
        let (o, _) = run("select x in a b; do echo \"r=$REPLY x=$x\"; break; done <<< \"1\"");
        assert_eq!(o, "r=1 x=a\n");
    }

    #[test]
    fn select_eof_terminates() {
        // The here-string provides one line; the next read hits EOF and ends
        // the loop (no infinite spin, no `break` needed).
        let (o, _) = run("select x in a b; do echo \"$x\"; done <<< \"1\"");
        assert_eq!(o, "a\n");
    }

    #[test]
    fn here_string_read() {
        let (o, _) = run("read x <<< hello; echo got $x");
        assert_eq!(o, "got hello\n");
    }

    #[test]
    fn read_last_var_keeps_internal_spacing() {
        // The final variable receives the raw remainder (internal runs of IFS
        // whitespace preserved), unlike a naive re-join.
        let (o, _) = run("read a b <<< '1   2   3'; echo \"[$a][$b]\"");
        assert_eq!(o, "[1][2   3]\n");
    }

    #[test]
    fn read_into_array() {
        let (o, _) = run("read -a arr <<< 'x y z'; echo \"${#arr[@]}:${arr[1]}\"");
        assert_eq!(o, "3:y\n");
    }

    #[test]
    fn read_raw_vs_escape() {
        // Without -r, a backslash escapes the next char; with -r it is literal.
        assert_eq!(run("read x <<< 'a\\tb'; echo \"$x\"").0, "atb\n");
        assert_eq!(run("read -r x <<< 'a\\tb'; echo \"$x\"").0, "a\\tb\n");
    }

    #[test]
    fn read_custom_ifs() {
        let (o, _) = run("IFS=: read a b c <<< '1:2:3'; echo \"$a-$b-$c\"");
        assert_eq!(o, "1-2-3\n");
    }

    #[test]
    fn read_nchars_limit() {
        // `-n N` stops after N characters (here-string adds a trailing \n, so
        // there are plenty of characters available).
        assert_eq!(run("read -n 3 x <<< 'abcdef'; echo \"$x\"").0, "abc\n");
        // Status 0 because the character count was reached.
        assert_eq!(run("read -n 3 x <<< 'abcdef'; echo $?").0, "0\n");
    }

    #[test]
    fn read_exact_nchars() {
        // `-N N` reads exactly N characters, ignoring delimiters/spaces.
        assert_eq!(run("read -N 5 x <<< 'a b c d'; echo \"[$x]\"").0, "[a b c]\n");
        // A short read (fewer than N available) yields status 1.
        assert_eq!(run("read -N 20 x <<< 'abc'; echo $?").0, "1\n");
    }

    #[test]
    fn read_custom_delim() {
        // `-d :` reads up to the first ':' delimiter.
        assert_eq!(run("read -d : x <<< 'foo:bar'; echo \"$x\"").0, "foo\n");
        // Delimiter found ⇒ status 0.
        assert_eq!(run("read -d : x <<< 'foo:bar'; echo $?").0, "0\n");
    }

    #[test]
    fn type_word_classification() {
        assert_eq!(run("type -t echo").0, "builtin\n");
        assert_eq!(run("type -t if").0, "keyword\n");
        assert_eq!(run("f() { :; }; type -t f").0, "function\n");
        // Unknown name: -t prints nothing and reports status 1.
        assert_eq!(run("type -t osh_no_such_cmd_xyz; echo $?").0, "1\n");
    }

    #[test]
    fn type_default_descriptions() {
        assert_eq!(run("type echo").0, "echo is a shell builtin\n");
        assert_eq!(run("type while").0, "while is a shell keyword\n");
        assert_eq!(run("g() { :; }; type g").0, "g is a function\n");
    }

    #[test]
    fn nocasematch_case_and_test() {
        // Default: case is case-sensitive.
        assert_eq!(
            run("case ABC in abc) echo y;; *) echo n;; esac").0,
            "n\n"
        );
        // With nocasematch, `case` folds case.
        assert_eq!(
            run("shopt -s nocasematch; case ABC in abc) echo y;; *) echo n;; esac").0,
            "y\n"
        );
        // `[[ == ]]` glob and literal both fold case under nocasematch.
        assert_eq!(run("shopt -s nocasematch; [[ Hello == h* ]] && echo y").0, "y\n");
        assert_eq!(run("shopt -s nocasematch; [[ Hello == hello ]] && echo y").0, "y\n");
        // Sanity: without it, the literal comparison is case-sensitive.
        assert_eq!(run("[[ Hello == hello ]] && echo y || echo n").0, "n\n");
    }

    #[test]
    fn nocasematch_regex() {
        // `=~` is case-sensitive by default, case-insensitive under nocasematch.
        assert_eq!(run("[[ Hello =~ ^hello$ ]] && echo y || echo n").0, "n\n");
        assert_eq!(
            run("shopt -s nocasematch; [[ Hello =~ ^hello$ ]] && echo y || echo n").0,
            "y\n"
        );
        // Character-class ranges fold too.
        assert_eq!(
            run("shopt -s nocasematch; [[ ABC =~ ^[a-z]+$ ]] && echo y || echo n").0,
            "y\n"
        );
    }

    #[test]
    fn extglob_in_test_and_case() {
        // Without extglob, `@(...)` is not special: the `@` and `(` are literal,
        // so `abc` does not match the pattern `@(a|b)c` textually.
        assert_eq!(run("[[ abc == @(a|b)c ]] && echo y || echo n").0, "n\n");
        // @(a|b) — exactly one alternative.
        assert_eq!(
            run("shopt -s extglob; [[ ac == @(a|b)c ]] && echo y || echo n").0,
            "y\n"
        );
        assert_eq!(
            run("shopt -s extglob; [[ xc == @(a|b)c ]] && echo y || echo n").0,
            "n\n"
        );
        // ?(...) zero or one.
        assert_eq!(
            run("shopt -s extglob; [[ color == colo?(u)r ]] && echo y || echo n").0,
            "y\n"
        );
        assert_eq!(
            run("shopt -s extglob; [[ colour == colo?(u)r ]] && echo y || echo n").0,
            "y\n"
        );
        assert_eq!(
            run("shopt -s extglob; [[ colouur == colo?(u)r ]] && echo y || echo n").0,
            "n\n"
        );
        // *(...) zero or more, +(...) one or more.
        assert_eq!(
            run("shopt -s extglob; [[ aaa == +(a) ]] && echo y || echo n").0,
            "y\n"
        );
        assert_eq!(
            run("shopt -s extglob; [[ '' == +(a) ]] && echo y || echo n").0,
            "n\n"
        );
        assert_eq!(
            run("shopt -s extglob; [[ '' == *(a) ]] && echo y || echo n").0,
            "y\n"
        );
        assert_eq!(
            run("shopt -s extglob; [[ ababab == *(ab) ]] && echo y || echo n").0,
            "y\n"
        );
        // !(...) negation.
        assert_eq!(
            run("shopt -s extglob; [[ foo == !(bar) ]] && echo y || echo n").0,
            "y\n"
        );
        assert_eq!(
            run("shopt -s extglob; [[ bar == !(bar) ]] && echo y || echo n").0,
            "n\n"
        );
        // In `case`.
        assert_eq!(
            run("shopt -s extglob; case abc in @(a|x)bc) echo m;; *) echo no;; esac").0,
            "m\n"
        );
    }

    #[test]
    fn extglob_in_param_and_glob() {
        // Parameter-trim with an extglob pattern.
        assert_eq!(
            run("shopt -s extglob; v=foobar; echo ${v##+(o|f)}").0,
            "bar\n"
        );
        // Replacement using an extglob alternation.
        assert_eq!(
            run("shopt -s extglob; v=cat; echo ${v/@(c|d)/b}").0,
            "bat\n"
        );
        // Nested extglob groups.
        assert_eq!(
            run("shopt -s extglob; [[ abab == *(@(a|b)) ]] && echo y || echo n").0,
            "y\n"
        );
    }

    #[test]
    fn here_doc_read_and_expand() {
        let (o, _) = run("name=world\nread line <<EOF\nhi $name\nEOF\necho $line");
        assert_eq!(o, "hi world\n");
    }

    #[test]
    fn here_doc_quoted_delim_no_expand() {
        let (o, _) = run("name=world\nread line <<'EOF'\nhi $name\nEOF\necho $line");
        assert_eq!(o, "hi $name\n");
    }

    #[test]
    fn param_trim_prefix_suffix() {
        assert_eq!(run("x=foo.tar.gz; echo ${x#*.}").0, "tar.gz\n");
        assert_eq!(run("x=foo.tar.gz; echo ${x##*.}").0, "gz\n");
        assert_eq!(run("x=foo.tar.gz; echo ${x%.*}").0, "foo.tar\n");
        assert_eq!(run("x=foo.tar.gz; echo ${x%%.*}").0, "foo\n");
    }

    #[test]
    fn param_case_modification() {
        // Doubled `^^`/`,,` convert every character.
        assert_eq!(run("x=hello; echo ${x^^}").0, "HELLO\n");
        assert_eq!(run("x=HELLO; echo ${x,,}").0, "hello\n");
        // Single `^`/`,` convert only the first character.
        assert_eq!(run("x=hello; echo ${x^}").0, "Hello\n");
        assert_eq!(run("x=HELLO; echo ${x,}").0, "hELLO\n");
        // A pattern selects which characters convert.
        assert_eq!(run("x=hello; echo ${x^^[aeiou]}").0, "hEllO\n");
        assert_eq!(run("x=HELLO; echo ${x,,[AEIOU]}").0, "HeLLo\n");
        // Single form only converts the first char if it matches the pattern.
        assert_eq!(run("x=hello; echo ${x^[b]}").0, "hello\n");
        assert_eq!(run("x=hello; echo ${x^[h]}").0, "Hello\n");
        // Mixed already-cased input is normalized.
        assert_eq!(run("x=\"Hello World\"; echo \"${x^^}\"").0, "HELLO WORLD\n");
        // Works on array elements.
        assert_eq!(run("a=(foo bar); echo ${a[1]^^}").0, "BAR\n");
        // Empty/unset value yields empty.
        assert_eq!(run("echo [${nope^^}]").0, "[]\n");
    }

    #[test]
    fn param_indirect_expansion() {
        // `${!ref}` reads the variable named by `ref`.
        assert_eq!(run("x=hello; ref=x; echo ${!ref}").0, "hello\n");
        // Chained/renamed references.
        assert_eq!(run("a=b; b=c; c=done; echo ${!a} ${!b}").0, "c done\n");
        // Unset referent yields empty.
        assert_eq!(run("echo [${!nope}]").0, "[]\n");
        // Referent naming an array element.
        assert_eq!(run("a=(x y z); ref='a[1]'; echo ${!ref}").0, "y\n");
        assert_eq!(
            run("declare -A m; m[k]=v; ref='m[k]'; echo ${!ref}").0,
            "v\n"
        );
    }

    #[test]
    fn param_prefix_names() {
        // `${!prefix*}` lists set variable names beginning with the prefix.
        assert_eq!(run("aa=1; ab=2; ba=3; echo ${!a*}").0, "aa ab\n");
        // `@` form behaves the same unquoted.
        assert_eq!(run("foo1=x; foo2=y; echo ${!foo@}").0, "foo1 foo2\n");
        // Names are sorted.
        assert_eq!(run("v_c=1; v_a=2; v_b=3; echo ${!v_*}").0, "v_a v_b v_c\n");
        // Quoted `@` form yields one field per name.
        assert_eq!(
            run("p1=x; p2=y; for n in \"${!p@}\"; do echo $n; done").0,
            "p1\np2\n"
        );
        // Arrays and assoc arrays are included by name.
        assert_eq!(
            run("arr=(1 2); declare -A amap; amap[k]=v; ascore=9; echo ${!a*}").0,
            "amap arr ascore\n"
        );
        // No match → empty.
        assert_eq!(run("echo [${!zzz*}]").0, "[]\n");
    }

    #[test]
    fn ansi_c_quoting() {
        // Common C escapes.
        assert_eq!(run("printf '%s' $'a\\tb'").0, "a\tb");
        assert_eq!(run("printf '%s' $'line1\\nline2'").0, "line1\nline2");
        // A literal backslash and a single quote inside.
        assert_eq!(run("printf '%s' $'a\\\\b'").0, "a\\b");
        assert_eq!(run("printf '%s' $'it\\'s'").0, "it's");
        // Hex and octal escapes.
        assert_eq!(run("printf '%s' $'\\x41\\x42'").0, "AB"); // 0x41=A 0x42=B
        assert_eq!(run("printf '%s' $'\\101\\102'").0, "AB"); // octal 101=A 102=B
        // Unicode escape.
        assert_eq!(run("printf '%s' $'\\u00e9'").0, "\u{e9}"); // é
        // No expansion inside $'…' (a `$var` stays literal).
        assert_eq!(run("x=hi; printf '%s' $'$x'").0, "$x");
        // Unknown escape keeps the backslash.
        assert_eq!(run("printf '%s' $'\\q'").0, "\\q");
        // Concatenation with adjacent text.
        assert_eq!(run("echo pre$'\\t'post").0, "pre\tpost\n");
    }

    #[test]
    fn getopts_basic() {
        // Flags, an option with an argument, and the OPTIND/remaining split.
        let src = "set -- -a -b val -c foo bar\n\
                   out=\n\
                   while getopts \"ab:c\" opt; do\n\
                     case $opt in\n\
                       a) out=\"$out a\" ;;\n\
                       b) out=\"$out b=$OPTARG\" ;;\n\
                       c) out=\"$out c\" ;;\n\
                     esac\n\
                   done\n\
                   shift $((OPTIND - 1))\n\
                   echo \"opts:$out rest:$*\"";
        assert_eq!(run(src).0, "opts: a b=val c rest:foo bar\n");
    }

    #[test]
    fn getopts_bundled_flags() {
        // Bundled short flags like -abc are parsed one per call.
        let src = "set -- -abc\n\
                   out=\n\
                   while getopts \"abc\" opt; do out=\"$out$opt\"; done\n\
                   echo \"$out\"";
        assert_eq!(run(src).0, "abc\n");
    }

    #[test]
    fn getopts_unknown_option() {
        // Unknown option sets opt to '?'; silent mode (leading ':') puts the
        // bad char in OPTARG instead of printing to stderr.
        let src = "set -- -x\n\
                   getopts \":ab\" opt\n\
                   echo \"$opt $OPTARG\"";
        assert_eq!(run(src).0, "? x\n");
    }

    #[test]
    fn getopts_missing_argument_silent() {
        // Missing required argument in silent mode: opt=':' and OPTARG=optchar.
        let src = "set -- -b\n\
                   getopts \":ab:\" opt\n\
                   echo \"$opt $OPTARG\"";
        assert_eq!(run(src).0, ": b\n");
    }

    #[test]
    fn mapfile_reads_lines() {
        // -t strips the trailing newline from each element.
        let src = "mapfile -t arr <<< $'a\\nb\\nc'\n\
                   echo \"${#arr[@]}\"\n\
                   echo \"${arr[0]}-${arr[1]}-${arr[2]}\"";
        assert_eq!(run(src).0, "3\na-b-c\n");
    }

    #[test]
    fn mapfile_keeps_delimiter_by_default() {
        // Without -t, each element retains its trailing newline.
        let src = "mapfile arr <<< $'x\\ny'\n\
                   printf '[%s]' \"${arr[@]}\"";
        assert_eq!(run(src).0, "[x\n][y\n]");
    }

    #[test]
    fn printf_recycles_format() {
        // The format string repeats until all arguments are consumed.
        assert_eq!(run("printf '%s\\n' a b c").0, "a\nb\nc\n");
        assert_eq!(run("printf '[%s:%d]' x 1 y 2").0, "[x:1][y:2]");
        // No arg-consuming conversion → format emitted exactly once.
        assert_eq!(run("printf 'hi\\n'").0, "hi\n");
    }

    #[test]
    fn printf_assign_with_v() {
        assert_eq!(run("printf -v out '%s-%s' a b; echo \"$out\"").0, "a-b\n");
        // -v suppresses stdout output.
        assert_eq!(run("printf -v x 'hi'").0, "");
    }

    #[test]
    fn printf_integer_conversions() {
        assert_eq!(run("printf '%x' 255").0, "ff");
        assert_eq!(run("printf '%X' 255").0, "FF");
        assert_eq!(run("printf '%o' 8").0, "10");
        assert_eq!(run("printf '%i' 42").0, "42");
        assert_eq!(run("printf '%u' 5").0, "5");
        // Hex input to %d.
        assert_eq!(run("printf '%d' 0xff").0, "255");
    }

    #[test]
    fn printf_width_and_precision() {
        assert_eq!(run("printf '%5d' 42").0, "   42");
        assert_eq!(run("printf '%-5d|' 42").0, "42   |");
        assert_eq!(run("printf '%05d' 42").0, "00042");
        assert_eq!(run("printf '%.2s' abcd").0, "ab");
    }

    #[test]
    fn printf_dynamic_width_and_precision() {
        // `*` takes the field width from the next argument.
        assert_eq!(run("printf '%*d' 5 42").0, "   42");
        // `.*` takes the precision from the next argument.
        assert_eq!(run("printf '%.*f' 2 3.14159").0, "3.14");
        // Both dynamic in one conversion.
        assert_eq!(run("printf '%*.*f' 8 2 3.14159").0, "    3.14");
        // A negative dynamic width left-justifies with the absolute magnitude.
        assert_eq!(run("printf '%*d|' -5 42").0, "42   |");
        // `.*` on a string precision truncates.
        assert_eq!(run("printf '%.*s' 2 abcd").0, "ab");
    }

    #[test]
    fn printf_q_b_and_c_conversions() {
        assert_eq!(run("printf '%q' 'a b'").0, "'a b'");
        assert_eq!(run("printf '%b' 'a\\tb'").0, "a\tb");
        assert_eq!(run("printf '%c' xyz").0, "x");
    }

    #[test]
    fn printf_float_conversion() {
        assert_eq!(run("printf '%.2f' 3.14159").0, "3.14");
        assert_eq!(run("printf '%f' 1").0, "1.000000");
    }

    #[test]
    fn printf_time_conversion() {
        // Epoch 0 = 1970-01-01 00:00:00 UTC (a Thursday, day-of-year 001).
        assert_eq!(run("printf '%(%F)T\\n' 0").0, "1970-01-01\n");
        assert_eq!(run("printf '%(%A)T\\n' 0").0, "Thursday\n");
        assert_eq!(run("printf '%(%j)T\\n' 0").0, "001\n");
        // 12-hour clock: midnight is 12 AM.
        assert_eq!(run("printf '%(%I %p)T\\n' 0").0, "12 AM\n");
        // A known later timestamp: 1000000000 = 2001-09-09 01:46:40 UTC (Sunday).
        assert_eq!(
            run("printf '%(%Y-%m-%d %H:%M:%S)T\\n' 1000000000").0,
            "2001-09-09 01:46:40\n"
        );
        assert_eq!(run("printf '%(%B %a)T\\n' 1000000000").0, "September Sun\n");
        // `%T`/`%R` compound specifiers.
        assert_eq!(run("printf '%(%T)T\\n' 1000000000").0, "01:46:40\n");
        assert_eq!(run("printf '%(%R)T\\n' 1000000000").0, "01:46\n");
        // A negative argument means "now"; just check it produces 4-digit year.
        assert_eq!(run("printf '%(%Y)T\\n' -1").0.trim().len(), 4);
    }

    #[test]
    fn set_errexit_exits_on_failure() {
        // A failing command aborts the script under `set -e`.
        let (o, s) = run("set -e; false; echo after");
        assert_eq!(o, "");
        assert_eq!(s, 1);
        // A successful command chain still runs to completion.
        assert_eq!(run("set -e; true; echo after").0, "after\n");
    }

    #[test]
    fn set_errexit_condition_exempt() {
        // Failing commands in a condition do not trigger errexit.
        assert_eq!(run("set -e; if false; then echo t; fi; echo done").0, "done\n");
        assert_eq!(run("set -e; while false; do echo x; done; echo done").0, "done\n");
        // A non-final `&&` operand failure is exempt; a negated command too.
        assert_eq!(run("set -e; false && echo skip; echo done").0, "done\n");
        assert_eq!(run("set -e; ! true; echo done").0, "done\n");
    }

    #[test]
    fn set_errexit_final_and_or_fires() {
        // The command after the final `&&` is subject to errexit.
        let (o, s) = run("set -e; true && false; echo after");
        assert_eq!(o, "");
        assert_eq!(s, 1);
    }

    #[test]
    fn set_nounset_aborts_on_unset() {
        let (o, s) = run("set -u; echo $undefined; echo after");
        assert_eq!(o, "");
        assert_eq!(s, 1);
        // A default/alternate operator suppresses the error.
        assert_eq!(run("set -u; echo ${undefined:-ok}").0, "ok\n");
        // Special parameters are always considered set.
        assert_eq!(run("set -u; echo $#").0, "0\n");
        // Set variables expand normally.
        assert_eq!(run("set -u; x=hi; echo $x").0, "hi\n");
    }

    #[test]
    fn local_shadows_global() {
        // A local variable does not leak out of the function.
        assert_eq!(
            run("x=outer; f() { local x=inner; echo $x; }; f; echo $x").0,
            "inner\nouter\n"
        );
        // A previously-unset name is restored to unset after the function.
        assert_eq!(
            run("f() { local y=hi; echo $y; }; f; echo \"[${y-unset}]\"").0,
            "hi\n[unset]\n"
        );
    }

    #[test]
    fn local_mutation_is_isolated() {
        // Assignments to a local inside the function don't affect the global.
        let src = "c=0; inc() { local c=$1; c=$((c+1)); echo $c; }; inc 5; echo $c";
        assert_eq!(run(src).0, "6\n0\n");
    }

    #[test]
    fn local_outside_function_errors() {
        // `local` at top level is an error (non-zero status), not a crash.
        let (_, s) = run("local x=1");
        assert_eq!(s, 1);
    }

    #[test]
    fn local_array_is_scoped() {
        let src = "a=(g1 g2); f() { local a=(l1 l2); echo \"${a[@]}\"; }; f; echo \"${a[@]}\"";
        assert_eq!(run(src).0, "l1 l2\ng1 g2\n");
    }

    #[test]
    fn readonly_blocks_reassignment() {
        // The value stays at the readonly binding; reassignment fails (status 1)
        // and leaves the original intact.
        let (o, s) = run("readonly x=1; x=2; echo $x");
        assert_eq!(o, "1\n");
        assert_eq!(s, 0); // the trailing `echo` succeeds
        // The assignment itself reports failure.
        assert_eq!(run("readonly x=1; x=2").1, 1);
    }

    #[test]
    fn readonly_blocks_unset() {
        let (o, _) = run("readonly x=hi; unset x; echo $x");
        assert_eq!(o, "hi\n");
        assert_eq!(run("readonly x=hi; unset x").1, 1);
    }

    #[test]
    fn declare_r_marks_readonly() {
        let (o, _) = run("declare -r y=const; y=other; echo $y");
        assert_eq!(o, "const\n");
    }

    #[test]
    fn readonly_bare_name_then_assign_fails() {
        // `readonly x` marks an existing/empty name; a later assignment fails.
        let (o, _) = run("x=v; readonly x; x=w; echo $x");
        assert_eq!(o, "v\n");
    }

    #[test]
    fn readonly_print_lists_vars() {
        let (o, _) = run("readonly a=1; readonly b=2; readonly -p");
        assert_eq!(o, "readonly a=1\nreadonly b=2\n");
    }

    #[test]
    fn declare_p_scalar() {
        assert_eq!(run("x=5; declare -p x").0, "declare -- x=\"5\"\n");
        // Readonly / exported attributes show in the flag group.
        assert_eq!(run("readonly x=5; declare -p x").0, "declare -r x=\"5\"\n");
        assert_eq!(run("export x=5; declare -p x").0, "declare -x x=\"5\"\n");
    }

    #[test]
    fn declare_p_arrays() {
        assert_eq!(run("a=(x y); declare -p a").0, "declare -a a=([0]=\"x\" [1]=\"y\")\n");
        assert_eq!(
            run("declare -A m; m[k]=v; declare -p m").0,
            "declare -A m=([k]=\"v\")\n"
        );
    }

    #[test]
    fn declare_p_missing_is_error() {
        assert_eq!(run("declare -p nope").1, 1);
    }

    #[test]
    fn declare_p_quotes_specials() {
        // A value with a double quote and `$` is backslash-escaped.
        assert_eq!(run("x='a\"b$c'; declare -p x").0, "declare -- x=\"a\\\"b\\$c\"\n");
    }

    #[test]
    fn set_can_disable_options() {
        // `set +e` turns errexit back off.
        assert_eq!(run("set -e; set +e; false; echo after").0, "after\n");
        // Long-form option names work too.
        let (_, s) = run("set -o nounset; echo $undefined; echo after");
        assert_eq!(s, 1);
    }

    #[test]
    fn brace_expansion_command_words() {
        assert_eq!(run("echo a{b,c,d}e").0, "abe ace ade\n");
        assert_eq!(run("echo {1..5}").0, "1 2 3 4 5\n");
        assert_eq!(run("echo {1..9..2}").0, "1 3 5 7 9\n");
        assert_eq!(run("echo file{01..03}.txt").0, "file01.txt file02.txt file03.txt\n");
        assert_eq!(run("echo {a..c}{1,2}").0, "a1 a2 b1 b2 c1 c2\n");
        // Quoted braces stay literal; invalid braces stay literal.
        assert_eq!(run("echo '{a,b}'").0, "{a,b}\n");
        assert_eq!(run("echo {abc}").0, "{abc}\n");
    }

    #[test]
    fn brace_expansion_in_for_loop() {
        let (o, _) = run("for i in {1..3}; do echo x$i; done");
        assert_eq!(o, "x1\nx2\nx3\n");
    }

    #[test]
    fn brace_expansion_with_param() {
        // A parameter reference inside an alternative expands after braces.
        assert_eq!(run("v=Z; echo {$v,b}").0, "Z b\n");
    }

    #[test]
    fn param_transform_quote_and_case() {
        // @Q quotes a value with a space; @U/@u/@L transform case.
        assert_eq!(run("x=\"a b\"; echo \"${x@Q}\"").0, "'a b'\n");
        assert_eq!(run("x=hello; echo \"${x@U}\"").0, "HELLO\n");
        assert_eq!(run("x=hello; echo \"${x@u}\"").0, "Hello\n");
        assert_eq!(run("x=HeLLo; echo \"${x@L}\"").0, "hello\n");
        // A simple safe word needs no quoting under @Q.
        assert_eq!(run("x=word; echo \"${x@Q}\"").0, "word\n");
    }

    #[test]
    fn command_and_builtin_builtins() {
        assert_eq!(run("command echo hi").0, "hi\n");
        assert_eq!(run("builtin echo hi").0, "hi\n");
        assert_eq!(run("command -v echo").0, "echo\n");
        assert_eq!(run("command -V echo").0, "echo is a shell builtin\n");
        // A function shadowing a builtin is bypassed by `command`.
        assert_eq!(run("echo() { printf OVERRIDE; }; command echo hi").0, "hi\n");
        // -v on a function prints the name; an unknown name → status 1, no output.
        assert_eq!(run("greet() { :; }; command -v greet").0, "greet\n");
        assert_eq!(run("command -v no_such_cmd_xyz; echo $?").0, "1\n");
    }

    #[test]
    fn special_var_random() {
        // Deterministic when reseeded, and within bash's 15-bit range.
        assert_eq!(run("RANDOM=1; a=$RANDOM; RANDOM=1; b=$RANDOM; [ \"$a\" = \"$b\" ] && echo same").0, "same\n");
        assert_eq!(run("RANDOM=7; r=$RANDOM; [ \"$r\" -ge 0 ] && [ \"$r\" -lt 32768 ] && echo ok").0, "ok\n");
    }

    #[test]
    fn special_var_seconds_and_epoch() {
        assert_eq!(run("echo $SECONDS").0, "0\n");
        assert_eq!(run("SECONDS=100; echo $SECONDS").0, "100\n");
        assert_eq!(run("[ $EPOCHSECONDS -gt 1000000000 ] && echo ok").0, "ok\n");
    }

    #[test]
    fn param_transform_escape_and_attrs() {
        // @E expands backslash escapes; @a reports array attributes.
        assert_eq!(run("x='a\\tb'; printf '%s' \"${x@E}\"").0, "a\tb");
        assert_eq!(run("declare -A m; m[k]=v; echo \"${m@a}\"").0, "A\n");
        assert_eq!(run("a=(1 2 3); echo \"${a@a}\"").0, "a\n");
    }

    #[test]
    fn readarray_default_var_and_count() {
        // Alias readarray, default MAPFILE array, and -n limit.
        let src = "readarray -t -n 2 <<< $'1\\n2\\n3\\n4'\n\
                   echo \"${#MAPFILE[@]} ${MAPFILE[0]} ${MAPFILE[1]}\"";
        assert_eq!(run(src).0, "2 1 2\n");
    }

    #[test]
    fn param_substring() {
        assert_eq!(run("x=abcdef; echo ${x:2}").0, "cdef\n");
        assert_eq!(run("x=abcdef; echo ${x:2:3}").0, "cde\n");
        assert_eq!(run("x=abcdef; echo ${x: -2}").0, "ef\n");
        assert_eq!(run("x=abcdef; echo ${x:1:-1}").0, "bcde\n");
    }

    #[test]
    fn param_replace_forms() {
        assert_eq!(run("x=aXbXc; echo ${x/X/-}").0, "a-bXc\n");
        assert_eq!(run("x=aXbXc; echo ${x//X/-}").0, "a-b-c\n");
        assert_eq!(run("x=abcabc; echo ${x/#abc/Z}").0, "Zabc\n");
        assert_eq!(run("x=abcabc; echo ${x/%abc/Z}").0, "abcZ\n");
        assert_eq!(run("x=hello; echo ${x//l/}").0, "heo\n");
    }

    #[test]
    fn param_ops_still_work() {
        assert_eq!(run("echo ${u:-default}").0, "default\n");
        assert_eq!(run("x=set; echo ${x:+yes}").0, "yes\n");
    }

    #[test]
    fn glob_match_basics() {
        let g = |p: &str, t: &str| {
            glob_match(
                &p.chars().collect::<Vec<_>>(),
                &t.chars().collect::<Vec<_>>(),
                false,
            )
        };
        assert!(g("*", "anything"));
        assert!(g("h?llo", "hello"));
        assert!(g("a*c", "abbbc"));
        assert!(!g("a*c", "abbb"));
        assert!(g("[a-c]x", "bx"));
        assert!(!g("[a-c]x", "dx"));
        assert!(g("[!0-9]", "z"));
        assert!(!g("[!0-9]", "5"));
        assert!(g("file.txt", "file.txt"));
        assert!(!g("file.txt", "file.md"));
    }

    #[test]
    fn cond_string_equality() {
        assert_eq!(run("[[ foo == foo ]]").1, 0);
        assert_eq!(run("[[ foo == bar ]]").1, 1);
        assert_eq!(run("[[ foo != bar ]]").1, 0);
        assert_eq!(run("x=hello; [[ $x = hello ]]").1, 0);
    }

    #[test]
    fn cond_glob_and_quoting() {
        // Unquoted RHS is a glob pattern.
        assert_eq!(run("[[ foobar == foo* ]]").1, 0);
        assert_eq!(run("[[ foobar == baz* ]]").1, 1);
        // Quoted RHS is a literal, so the `*` does not match.
        assert_eq!(run("[[ foobar == \"foo*\" ]]").1, 1);
        assert_eq!(run("[[ 'foo*' == \"foo*\" ]]").1, 0);
    }

    #[test]
    fn cond_numeric() {
        assert_eq!(run("[[ 3 -gt 2 ]]").1, 0);
        assert_eq!(run("[[ 2 -gt 3 ]]").1, 1);
        assert_eq!(run("[[ 5 -eq 5 ]]").1, 0);
        assert_eq!(run("x=4; [[ $x -le 4 ]]").1, 0);
        // Operands undergo arithmetic evaluation.
        assert_eq!(run("[[ 2+2 -eq 4 ]]").1, 0);
    }

    #[test]
    fn cond_string_len() {
        assert_eq!(run("[[ -z \"\" ]]").1, 0);
        assert_eq!(run("[[ -n foo ]]").1, 0);
        assert_eq!(run("x=; [[ -z $x ]]").1, 0);
        assert_eq!(run("x=set; [[ -n $x ]]").1, 0);
    }

    #[test]
    fn cond_logical_and_grouping() {
        assert_eq!(run("[[ 1 -eq 1 && 2 -eq 2 ]]").1, 0);
        assert_eq!(run("[[ 1 -eq 1 && 2 -eq 3 ]]").1, 1);
        assert_eq!(run("[[ 1 -eq 2 || 3 -eq 3 ]]").1, 0);
        assert_eq!(run("[[ ! 1 -eq 2 ]]").1, 0);
        assert_eq!(run("[[ ( 1 -eq 1 || 1 -eq 2 ) && 3 -eq 3 ]]").1, 0);
    }

    #[test]
    fn cond_string_ordering() {
        assert_eq!(run("[[ abc < abd ]]").1, 0);
        assert_eq!(run("[[ abd < abc ]]").1, 1);
        assert_eq!(run("[[ b > a ]]").1, 0);
    }

    #[test]
    fn cond_in_if() {
        assert_eq!(run("if [[ foo == foo ]]; then echo yes; fi").0, "yes\n");
        assert_eq!(
            run("x=3; if [[ $x -gt 5 ]]; then echo big; else echo small; fi").0,
            "small\n"
        );
    }

    #[test]
    fn cond_regex_match() {
        // Basic ERE match sets a zero exit status.
        assert_eq!(run("[[ abc123 =~ ^[a-z]+[0-9]+$ ]]").1, 0);
        // Non-match yields status 1.
        assert_eq!(run("[[ abc =~ ^[0-9]+$ ]]").1, 1);
    }

    #[test]
    fn cond_regex_in_if() {
        assert_eq!(
            run("if [[ foo42 =~ [0-9]+ ]]; then echo num; else echo none; fi").0,
            "num\n"
        );
    }

    #[test]
    fn cond_regex_bash_rematch() {
        // Whole match in [0], captures in [1..]; extract via ${BASH_REMATCH[n]}.
        let (o, _) = run(
            "[[ 2026-07-18 =~ ([0-9]+)-([0-9]+)-([0-9]+) ]]; \
             echo \"${BASH_REMATCH[0]} ${BASH_REMATCH[1]} ${BASH_REMATCH[2]} ${BASH_REMATCH[3]}\"",
        );
        assert_eq!(o, "2026-07-18 2026 07 18\n");
    }

    #[test]
    fn cond_regex_rhs_expansion() {
        // The RHS undergoes parameter expansion before compilation.
        assert_eq!(run("p='^h.*o$'; [[ hello =~ $p ]]").1, 0);
    }

    #[test]
    fn cond_regex_invalid_pattern() {
        // A malformed regex yields false (non-zero status), not a crash.
        assert_ne!(run("[[ x =~ ( ]]").1, 0);
    }

    #[test]
    fn cond_regex_negated() {
        assert_eq!(run("[[ ! abc =~ [0-9] ]] && echo nonum").0, "nonum\n");
    }

    #[test]
    fn cond_regex_double_quoted_rhs_is_literal() {
        // A double-quoted RHS matches literally: `.` is a real dot, not "any".
        assert_eq!(run("[[ a.b =~ \"a.b\" ]]").1, 0);
        assert_eq!(run("[[ axb =~ \"a.b\" ]]").1, 1);
        // Unquoted, the same text is a regex and `.` matches any char.
        assert_eq!(run("[[ axb =~ a.b ]]").1, 0);
    }

    #[test]
    fn cond_regex_single_quoted_rhs_is_literal() {
        assert_eq!(run("[[ a.b =~ 'a.b' ]]").1, 0);
        assert_eq!(run("[[ axb =~ 'a.b' ]]").1, 1);
    }

    #[test]
    fn cond_regex_mixed_quoting() {
        // Only the quoted `.` is literal; the surrounding text stays regex.
        assert_eq!(run("[[ a.b =~ a\".\"b ]]").1, 0);
        assert_eq!(run("[[ axb =~ a\".\"b ]]").1, 1);
    }

    #[test]
    fn cond_regex_quoted_var_is_literal() {
        // Quoted expansion is literal; unquoted expansion is a live pattern.
        assert_eq!(run("p='a.b'; [[ a.b =~ \"$p\" ]]").1, 0);
        assert_eq!(run("p='a.b'; [[ axb =~ \"$p\" ]]").1, 1);
        assert_eq!(run("p='a.b'; [[ axb =~ $p ]]").1, 0);
    }

    #[test]
    fn arith_command_status() {
        assert_eq!(run("(( 1 + 1 ))").1, 0);
        assert_eq!(run("(( 0 ))").1, 1);
        assert_eq!(run("(( 5 > 3 ))").1, 0);
        assert_eq!(run("(( 3 > 5 ))").1, 1);
    }

    #[test]
    fn arith_command_with_vars() {
        assert_eq!(run("x=4; (( x > 3 ))").1, 0);
        assert_eq!(run("x=2; (( x > 3 ))").1, 1);
        // Used as a condition.
        assert_eq!(run("x=10; if (( x > 5 )); then echo big; fi").0, "big\n");
    }

    #[test]
    fn let_builtin_assigns_and_status() {
        // `let` evaluates the expression, mutating the variable.
        assert_eq!(run("let x=3+4; echo $x").0, "7\n");
        // Status is 0 when the last expression is non-zero, 1 when zero.
        assert_eq!(run("let '1 + 1'").1, 0);
        assert_eq!(run("let '0'").1, 1);
        // Multiple expressions: the last one drives the status.
        assert_eq!(run("let 'a=5' 'a>3'").1, 0);
        // Increment operators work.
        assert_eq!(run("x=4; let x++; echo $x").0, "5\n");
    }

    #[test]
    fn let_no_args_fails() {
        assert_eq!(run("let").1, 1);
    }

    #[test]
    fn nested_subshell_still_works() {
        // `( ( … ) )` with an inner space is nested subshells, not arithmetic.
        assert_eq!(run("( ( echo hi ) )").0, "hi\n");
    }

    #[test]
    fn arith_ternary_and_comma() {
        // Ternary in `$(( … ))` is a common idiom for conditional values.
        assert_eq!(run("x=5; echo $(( x > 3 ? 100 : 200 ))").0, "100\n");
        assert_eq!(run("x=1; echo $(( x > 3 ? 100 : 200 ))").0, "200\n");
        // Comma evaluates all, yields the last.
        assert_eq!(run("echo $(( 1 + 1, 2 * 5 ))").0, "10\n");
        // As a `(( … ))` command, the exit status reflects the final value.
        assert_eq!(run("(( 1 ? 1 : 0 ))").1, 0);
        assert_eq!(run("(( 1 ? 0 : 1 ))").1, 1);
    }

    #[test]
    fn arith_number_bases() {
        // base#num and leading-zero octal survive `$(( … ))` expansion:
        // the `#` must not be mistaken for a comment.
        assert_eq!(run("echo $(( 2#1010 ))").0, "10\n");
        assert_eq!(run("echo $(( 16#ff ))").0, "255\n");
        assert_eq!(run("echo $(( 8#17 ))").0, "15\n");
        assert_eq!(run("echo $(( 017 ))").0, "15\n");
        assert_eq!(run("echo $(( 64#_ ))").0, "63\n");
    }

    #[test]
    fn declare_integer_attribute() {
        // `declare -i` makes later plain assignments evaluate arithmetically.
        assert_eq!(run("declare -i x; x=5+3; echo $x").0, "8\n");
        // The initializer on the declare itself is also evaluated.
        assert_eq!(run("declare -i y=2*3; echo $y").0, "6\n");
        // `+=` on an integer variable performs numeric addition.
        assert_eq!(run("declare -i z=10; z+=5; echo $z").0, "15\n");
        // A non-numeric expression evaluates to 0.
        assert_eq!(run("declare -i q=abc; echo $q").0, "0\n");
        // `+i` removes the attribute; assignments become plain strings again.
        assert_eq!(run("declare -i w=4; declare +i w; w=1+2; echo $w").0, "1+2\n");
        // Integer array elements are evaluated too.
        assert_eq!(run("declare -ia arr; arr[0]=3+4; echo ${arr[0]}").0, "7\n");
        // `declare -p` reflects the integer attribute.
        assert_eq!(run("declare -i n=9; declare -p n").0, "declare -i n=\"9\"\n");
    }

    #[test]
    fn declare_case_attributes() {
        // `-l` lowercases assigned values; `-u` uppercases them.
        assert_eq!(run("declare -l x; x=HeLLo; echo $x").0, "hello\n");
        assert_eq!(run("declare -u y; y=HeLLo; echo $y").0, "HELLO\n");
        // The initializer on the declare itself is folded too.
        assert_eq!(run("declare -u z=abc; echo $z").0, "ABC\n");
        // `-u` and `-l` are mutually exclusive; the later flag wins.
        assert_eq!(run("declare -l w; declare -u w; w=AbC; echo $w").0, "ABC\n");
        // Within one cluster the last case flag wins (`-ul` → lowercase).
        assert_eq!(run("declare -ul v=AbC; echo $v").0, "abc\n");
        // `+u` removes the attribute.
        assert_eq!(run("declare -u q=abc; declare +u q; q=def; echo $q").0, "def\n");
        // Array elements are folded too.
        assert_eq!(run("declare -u arr; arr[0]=xy; echo ${arr[0]}").0, "XY\n");
        // `declare -p` reflects the case attribute.
        assert_eq!(run("declare -l s=Hi; declare -p s").0, "declare -l s=\"hi\"\n");
    }

    #[test]
    fn arith_assignment_command() {
        // `(( x = … ))` writes back to the shell scalar.
        assert_eq!(run("(( x = 5 )); echo $x").0, "5\n");
        // Compound assignment reads-modifies-writes.
        assert_eq!(run("x=5; (( x += 3 )); echo $x").0, "8\n");
        assert_eq!(run("x=10; (( x -= 4 )); echo $x").0, "6\n");
        assert_eq!(run("x=3; (( x *= 4 )); echo $x").0, "12\n");
        assert_eq!(run("x=20; (( x /= 6 )); echo $x").0, "3\n");
        assert_eq!(run("x=20; (( x %= 6 )); echo $x").0, "2\n");
        assert_eq!(run("x=1; (( x <<= 4 )); echo $x").0, "16\n");
        assert_eq!(run("x=6; (( x &= 4 )); echo $x").0, "4\n");
        // The exit status reflects the assigned value (0 → exit 1).
        assert_eq!(run("(( x = 0 ))").1, 1);
        assert_eq!(run("(( x = 7 ))").1, 0);
    }

    #[test]
    fn arith_increment_command() {
        // Pre/post increment and decrement mutate the shell scalar.
        assert_eq!(run("x=5; (( x++ )); echo $x").0, "6\n");
        assert_eq!(run("x=5; (( ++x )); echo $x").0, "6\n");
        assert_eq!(run("x=5; (( x-- )); echo $x").0, "4\n");
        assert_eq!(run("x=5; (( --x )); echo $x").0, "4\n");
        // Post-increment yields the old value in `$(( … ))`.
        assert_eq!(run("x=5; echo $(( x++ )); echo $x").0, "5\n6\n");
        // Pre-increment yields the new value.
        assert_eq!(run("x=5; echo $(( ++x )); echo $x").0, "6\n6\n");
        // Increment on an unset variable starts from 0.
        assert_eq!(run("echo $(( n++ )); echo $n").0, "0\n1\n");
    }

    #[test]
    fn arith_assignment_array_elements() {
        // Assign to an indexed array element inside arithmetic.
        assert_eq!(run("a=(10 20 30); (( a[1] = 99 )); echo ${a[1]}").0, "99\n");
        assert_eq!(run("a=(10 20 30); (( a[0] += 5 )); echo ${a[0]}").0, "15\n");
        assert_eq!(run("a=(10 20 30); (( a[2]++ )); echo ${a[2]}").0, "31\n");
        // Assign to an associative element by string key.
        assert_eq!(
            run("declare -A m; m[foo]=7; (( m[foo] += 3 )); echo ${m[foo]}").0,
            "10\n"
        );
    }

    #[test]
    fn arith_c_style_for_loop() {
        // Classic counting loop.
        assert_eq!(
            run("for (( i=0; i<3; i++ )); do echo $i; done").0,
            "0\n1\n2\n"
        );
        // Multiple init/update via comma.
        assert_eq!(
            run("for (( i=0, j=10; i<3; i++, j-- )); do echo $i:$j; done").0,
            "0:10\n1:9\n2:8\n"
        );
        // Empty sections mean forever/true; `break` exits.
        assert_eq!(
            run("i=0; for (( ; ; )); do echo $i; (( i++ )); (( i >= 2 )) && break; done").0,
            "0\n1\n"
        );
        // A false condition from the start runs the body zero times.
        assert_eq!(run("for (( i=5; i<3; i++ )); do echo $i; done").0, "");
    }

    #[test]
    fn arith_associative_subscript() {
        // Inside `(( … ))`/`$(( … ))` an associative element is read by its
        // string key (not an arithmetic subscript), like bash.
        assert_eq!(
            run("declare -A m; m[foo]=7; m[bar]=13; echo $(( m[foo] + m[bar] ))").0,
            "20\n"
        );
        // Used as a condition.
        assert_eq!(
            run("declare -A m; m[on]=1; if (( m[on] )); then echo yes; fi").0,
            "yes\n"
        );
        // A key supplied via expansion works too (`$k` expands before arith).
        assert_eq!(
            run("declare -A m; m[foo]=5; k=foo; echo $(( m[$k] ))").0,
            "5\n"
        );
        // An indexed array still uses arithmetic subscripts.
        assert_eq!(run("a=(10 20 30); echo $(( a[1] + a[2] ))").0, "50\n");
    }

    fn field_lit(s: &str) -> Vec<EChar> {
        s.chars().map(|c| EChar { c, quoted: false }).collect()
    }

    #[test]
    fn glob_toks_match() {
        let f = |p: &str, n: &str| {
            match_glob_toks(
                &compile_glob(&field_lit(p), false),
                &n.chars().collect::<Vec<_>>(),
            )
        };
        assert!(f("*.txt", "a.txt"));
        assert!(!f("*.txt", "a.log"));
        assert!(f("h?llo", "hello"));
        assert!(f("[ab]?", "a1"));
        assert!(!f("[ab]?", "c1"));
        assert!(f("[!0-9]x", "zx"));
        assert!(!f("[!0-9]x", "5x"));
    }

    #[test]
    fn glob_quoted_metachar_is_literal() {
        // A quoted `*` is a literal star, never a pattern.
        let mut field = field_lit("");
        field.push(EChar { c: '*', quoted: true });
        let toks = compile_glob(&field, false);
        assert!(match_glob_toks(&toks, &['*']));
        assert!(!match_glob_toks(&toks, &['a']));
    }

    #[test]
    fn glob_filesystem_expansion() {
        let _cwd = cwd_guard();
        // Use a uniquely-named cwd-relative dir to avoid the process-wide-cwd
        // race between parallel tests (no `set_current_dir`).
        let uniq = format!(
            "osh_globtest_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let dir = std::path::Path::new(&uniq);
        std::fs::create_dir_all(dir).expect("mkdir");
        for n in ["a.txt", "b.txt", "c.log", ".hidden"] {
            std::fs::File::create(dir.join(n)).expect("touch");
        }

        let basename = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();

        // `*.txt` matches the two text files (sorted), not the log or hidden.
        let mut txt: Vec<String> = glob_expand_field(&field_lit(&format!("{uniq}/*.txt")), false, false, false, false)
            .iter()
            .map(|p| basename(p))
            .collect();
        txt.sort();
        assert_eq!(txt, vec!["a.txt".to_string(), "b.txt".to_string()]);

        // `*` honors the leading-dot rule (no `.hidden`).
        let all = glob_expand_field(&field_lit(&format!("{uniq}/*")), false, false, false, false);
        assert!(all.iter().all(|p| !p.ends_with(".hidden")));
        assert_eq!(all.len(), 3);

        // An explicit leading `.` matches hidden files.
        let dot = glob_expand_field(&field_lit(&format!("{uniq}/.*")), false, false, false, false);
        assert!(dot.iter().any(|p| p.ends_with(".hidden")));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn glob_globstar_recursive() {
        let _cwd = cwd_guard();
        // Build a small tree:  root/{a.rs, sub/{b.rs, deep/c.rs}}
        let uniq = format!(
            "osh_gstar_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let root = std::path::Path::new(&uniq);
        std::fs::create_dir_all(root.join("sub").join("deep")).expect("mkdir");
        std::fs::File::create(root.join("a.rs")).expect("touch");
        std::fs::File::create(root.join("sub").join("b.rs")).expect("touch");
        std::fs::File::create(root.join("sub").join("deep").join("c.rs")).expect("touch");

        // `root/**/*.rs` with globstar finds every .rs at any depth.
        let mut rs = glob_expand_field(
            &field_lit(&format!("{uniq}/**/*.rs")),
            false,
            false,
            false,
            true,
        );
        rs.sort();
        assert_eq!(
            rs,
            vec![
                format!("{uniq}/a.rs"),
                format!("{uniq}/sub/b.rs"),
                format!("{uniq}/sub/deep/c.rs"),
            ]
        );

        // Without globstar, `**` behaves like `*` (single level only).
        let one = glob_expand_field(
            &field_lit(&format!("{uniq}/**/*.rs")),
            false,
            false,
            false,
            false,
        );
        assert_eq!(one, vec![format!("{uniq}/sub/b.rs")]);

        // Terminal `**` lists every descendant file and directory.
        let mut all = glob_expand_field(
            &field_lit(&format!("{uniq}/**")),
            false,
            false,
            false,
            true,
        );
        all.sort();
        assert_eq!(
            all,
            vec![
                format!("{uniq}/a.rs"),
                format!("{uniq}/sub"),
                format!("{uniq}/sub/b.rs"),
                format!("{uniq}/sub/deep"),
                format!("{uniq}/sub/deep/c.rs"),
            ]
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn shopt_nocaseglob_matches_case_insensitively() {
        let _cwd = cwd_guard();
        let uniq = format!(
            "osh_nocaseglob_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let dir = std::path::Path::new(&uniq);
        std::fs::create_dir_all(dir).expect("mkdir");
        for n in ["README.md", "Notes.TXT"] {
            std::fs::File::create(dir.join(n)).expect("touch");
        }

        // Case-sensitive: lowercase pattern misses the uppercase-extension file.
        let field = field_lit(&format!("{uniq}/*.txt"));
        let cs = glob_expand_field(&field, false, false, false, false);
        assert!(cs.is_empty());
        // With nocaseglob, `*.txt` matches `Notes.TXT`.
        let ci = glob_expand_field(&field, false, true, false, false);
        assert_eq!(ci.len(), 1);
        assert!(ci[0].ends_with("Notes.TXT"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn glob_no_match_stays_literal() {
        // With no match and no `nullglob`, the pattern is left as the word.
        assert_eq!(run("echo osh_definitely_no_such_glob_*.zzz").0, "osh_definitely_no_such_glob_*.zzz\n");
    }

    #[test]
    fn shopt_nullglob_removes_unmatched() {
        // With `nullglob`, an unmatched glob produces no word at all.
        assert_eq!(
            run("shopt -s nullglob; echo x osh_no_such_glob_*.zzz y").0,
            "x y\n"
        );
        // Unsetting restores the literal-word default.
        assert_eq!(
            run("shopt -s nullglob; shopt -u nullglob; echo osh_no_such_glob_*.zzz").0,
            "osh_no_such_glob_*.zzz\n"
        );
    }

    #[test]
    fn shopt_query_status() {
        // `shopt -q name` returns 0 iff the option is set.
        assert_eq!(run("shopt -q nullglob; echo $?").0, "1\n");
        assert_eq!(run("shopt -s nullglob; shopt -q nullglob; echo $?").0, "0\n");
    }

    #[test]
    fn shopt_unknown_name_errors() {
        // An unknown option name is a status-1 error.
        assert_eq!(run("shopt -s no_such_option_xyz; echo $?").0, "1\n");
    }

    #[test]
    fn shopt_dotglob_includes_hidden() {
        let _cwd = cwd_guard();
        let uniq = format!(
            "osh_dotglob_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let dir = std::path::Path::new(&uniq);
        std::fs::create_dir_all(dir).expect("mkdir");
        for n in ["a.txt", ".hidden"] {
            std::fs::File::create(dir.join(n)).expect("touch");
        }

        // Without dotglob, `*` skips the dotfile; with it, the dotfile is
        // included (but never `.`/`..`).
        let field = field_lit(&format!("{uniq}/*"));
        let plain = glob_expand_field(&field, false, false, false, false);
        assert!(plain.iter().all(|p| !p.ends_with(".hidden")));
        let with_dot = glob_expand_field(&field, true, false, false, false);
        assert!(with_dot.iter().any(|p| p.ends_with(".hidden")));
        assert!(with_dot.iter().all(|p| {
            let b = p.rsplit('/').next().unwrap_or(p);
            b != "." && b != ".."
        }));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn dir_stack_pushd_popd_dirs() {
        // Mutates the process-global cwd, so serialize against the cwd-relative
        // glob tests.
        let _cwd = cwd_guard();
        let orig = std::env::current_dir().expect("cwd");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let uniq = format!("osh_dirstack_{}_{}", std::process::id(), nanos);
        let da = std::env::temp_dir().join(format!("{uniq}_a"));
        let db = std::env::temp_dir().join(format!("{uniq}_b"));
        std::fs::create_dir_all(&da).expect("mkdir a");
        std::fs::create_dir_all(&db).expect("mkdir b");
        // Feed the shell forward-slash paths (accepted by the OS on all
        // platforms); the stored/echoed paths come back in native form.
        let pa = da.to_string_lossy().replace('\\', "/");
        let pb = db.to_string_lossy().replace('\\', "/");

        // pushd a, pushd b -> stack top is b, next is a. popd returns to a.
        let script = format!(
            "pushd {pa}\npushd {pb}\necho ---\ndirs +0\ndirs +1\npopd\necho ===\ndirs +0"
        );
        let (o, _) = run(&script);

        // Restore the process cwd before asserting (run() moved it).
        std::env::set_current_dir(&orig).expect("restore cwd");

        let lines: Vec<&str> = o.lines().collect();
        let dash = lines.iter().position(|l| *l == "---").expect("--- marker");
        let eq = lines.iter().position(|l| *l == "===").expect("=== marker");
        // After both pushes: +0 is b, +1 is a.
        assert!(
            lines[dash + 1].ends_with(&format!("{uniq}_b")),
            "dirs +0 should be b, got {:?}",
            lines[dash + 1]
        );
        assert!(
            lines[dash + 2].ends_with(&format!("{uniq}_a")),
            "dirs +1 should be a, got {:?}",
            lines[dash + 2]
        );
        // After popd, the current directory is a.
        assert!(
            lines[eq + 1].ends_with(&format!("{uniq}_a")),
            "dirs +0 after popd should be a, got {:?}",
            lines[eq + 1]
        );

        std::fs::remove_dir_all(&da).ok();
        std::fs::remove_dir_all(&db).ok();
    }

    #[test]
    fn time_keyword_runs_pipeline() {
        // `time` runs the pipeline transparently: stdout is unaffected (the
        // report goes to stderr) and the pipeline's exit status is preserved.
        assert_eq!(run("time echo hi").0, "hi\n");
        assert_eq!(run("time false").1, 1);
        assert_eq!(run("time -p true").1, 0);
        // Timing a multi-stage pipeline still streams stdout normally.
        assert_eq!(run("time echo hi | cat").0, "hi\n");
    }

    #[test]
    fn time_report_formatting() {
        // POSIX `-p` form: two decimals, space-separated, no leading newline.
        let p = Shell::format_time_report(1.5, true);
        assert_eq!(p, "real 1.50\nuser 0.00\nsys 0.00\n");
        // Default (bash) form: leading newline, tab-separated, NmS.SSSs.
        let d = Shell::format_time_report(62.25, false);
        assert_eq!(d, "\nreal\t1m2.250s\nuser\t0m0.000s\nsys\t0m0.000s\n");
        let z = Shell::format_time_report(0.0, false);
        assert_eq!(z, "\nreal\t0m0.000s\nuser\t0m0.000s\nsys\t0m0.000s\n");
    }

    #[test]
    fn trap_set_print_reset() {
        // Setting a trap and printing it back in re-inputtable form.
        let (o, _) = run("trap 'echo bye' EXIT; trap -p");
        assert!(o.contains("trap -- 'echo bye' EXIT"), "got {o:?}");

        // A signal name with/without SIG prefix, case-insensitive, normalizes.
        let (o, _) = run("trap 'x' sigint; trap -p int");
        assert!(o.contains("trap -- 'x' INT"), "got {o:?}");

        // `trap - SIG` resets (removes) the handler.
        let (o, _) = run("trap 'x' INT; trap - INT; trap -p");
        assert!(!o.contains("INT"), "reset should remove INT, got {o:?}");

        // Ignore form ('') round-trips.
        let (o, _) = run("trap '' TERM; trap -p TERM");
        assert!(o.contains("trap -- '' TERM"), "got {o:?}");

        // An invalid spec is a status-1 error.
        let (_, st) = run("trap 'x' NOPE");
        assert_eq!(st, 1);
    }

    #[test]
    fn trap_list_names() {
        let (o, st) = run("trap -l");
        assert_eq!(st, 0);
        assert!(o.contains("SIGINT"));
        assert!(o.contains("SIGTERM"));
        assert!(o.contains("SIGKILL"));
    }

    #[test]
    fn trap_exit_fires_once() {
        let mut sh = Shell::new();
        sh.run_source("trap 'TRAP_MARK=1' EXIT");
        // The handler has not run yet — only stored.
        assert!(!sh.vars.contains_key("TRAP_MARK"));
        sh.run_exit_trap();
        assert_eq!(sh.vars.get("TRAP_MARK").map(String::as_str), Some("1"));
        // A second call is a no-op (fires at most once).
        sh.vars.remove("TRAP_MARK");
        sh.run_exit_trap();
        assert!(!sh.vars.contains_key("TRAP_MARK"));
    }

    #[test]
    fn trap_err_fires_on_failure() {
        let mut sh = Shell::new();
        sh.run_source("trap 'ERR_HIT=1' ERR\nfalse");
        assert_eq!(sh.vars.get("ERR_HIT").map(String::as_str), Some("1"));

        // ERR does not fire for a successful command...
        let mut sh = Shell::new();
        sh.run_source("trap 'ERR_HIT=1' ERR\ntrue");
        assert!(!sh.vars.contains_key("ERR_HIT"));

        // ...nor for a failure inside an `if` condition (exempt context).
        let mut sh = Shell::new();
        sh.run_source("trap 'ERR_HIT=1' ERR\nif false; then :; fi");
        assert!(!sh.vars.contains_key("ERR_HIT"));
    }

    #[test]
    fn trap_debug_fires_before_each_command() {
        let mut sh = Shell::new();
        sh.run_source("trap 'DBG=$((DBG+1))' DEBUG\n:\n:\n:");
        assert_eq!(sh.vars.get("DBG").map(String::as_str), Some("3"));
    }

    #[test]
    fn trap_return_fires_on_function_return() {
        let mut sh = Shell::new();
        sh.run_source("trap 'RET=1' RETURN\nf() { :; }\nf");
        assert_eq!(sh.vars.get("RET").map(String::as_str), Some("1"));
    }

    #[test]
    fn test_file_comparison_operators() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let base = std::env::temp_dir();
        let a = base.join(format!("osh_ntef_{}_{nanos}_a", std::process::id()));
        let b = base.join(format!("osh_ntef_{}_{nanos}_b", std::process::id()));
        std::fs::write(&a, b"x").expect("write a");
        // Ensure b's mtime is strictly later than a's.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&b, b"y").expect("write b");
        let pa = a.to_string_lossy().replace('\\', "/");
        let pb = b.to_string_lossy().replace('\\', "/");

        // `[[ … ]]` form.
        assert_eq!(run(&format!("[[ {pb} -nt {pa} ]] && echo yes")).0, "yes\n");
        assert_eq!(run(&format!("[[ {pa} -ot {pb} ]] && echo yes")).0, "yes\n");
        assert_eq!(
            run(&format!("[[ {pa} -nt {pb} ]] && echo yes || echo no")).0,
            "no\n"
        );
        assert_eq!(run(&format!("[[ {pa} -ef {pa} ]] && echo yes")).0, "yes\n");
        assert_eq!(
            run(&format!("[[ {pa} -ef {pb} ]] && echo yes || echo no")).0,
            "no\n"
        );

        // `test` / `[` form.
        assert_eq!(run(&format!("[ {pb} -nt {pa} ] && echo yes")).0, "yes\n");
        assert_eq!(run(&format!("test {pa} -ef {pa} && echo yes")).0, "yes\n");

        // Existence rule: a exists, missing sibling does not → `-nt` is true.
        let missing = base.join(format!("osh_ntef_{}_{nanos}_gone", std::process::id()));
        let pm = missing.to_string_lossy().replace('\\', "/");
        assert_eq!(run(&format!("[[ {pa} -nt {pm} ]] && echo yes")).0, "yes\n");

        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

    #[test]
    fn array_literal_and_index() {
        assert_eq!(run("a=(one two three); echo ${a[0]}").0, "one\n");
        assert_eq!(run("a=(one two three); echo ${a[2]}").0, "three\n");
        // A bare reference reads element 0.
        assert_eq!(run("a=(one two three); echo $a").0, "one\n");
        // Out-of-range index expands to empty.
        assert_eq!(run("a=(one two); echo x${a[9]}y").0, "xy\n");
    }

    #[test]
    fn array_all_and_star() {
        assert_eq!(run("a=(x y z); echo ${a[@]}").0, "x y z\n");
        assert_eq!(run("a=(x y z); echo ${a[*]}").0, "x y z\n");
    }

    #[test]
    fn array_length() {
        assert_eq!(run("a=(a b c d); echo ${#a[@]}").0, "4\n");
        assert_eq!(run("a=(a b c d); echo ${#a[*]}").0, "4\n");
        // Length of a specific element.
        assert_eq!(run("a=(hi hello); echo ${#a[1]}").0, "5\n");
    }

    #[test]
    fn array_indexed_assignment() {
        assert_eq!(run("a=(x y z); a[1]=Q; echo ${a[@]}").0, "x Q z\n");
        // Assigning past the end adds one sparse element (bash: no gap-fill), so
        // the element count is 2 (indices 0 and 3), not 4.
        assert_eq!(run("a=(x); a[3]=w; echo ${#a[@]}").0, "2\n");
        assert_eq!(run("a=(x); a[3]=w; echo ${a[3]}").0, "w\n");
    }

    #[test]
    fn array_append() {
        assert_eq!(run("a=(x y); a+=(z w); echo ${a[@]}").0, "x y z w\n");
        assert_eq!(run("a=(x y); a+=(z); echo ${#a[@]}").0, "3\n");
    }

    #[test]
    fn array_negative_index() {
        // -1 is the last element, -2 the second-to-last (bash semantics).
        assert_eq!(run("a=(x y z); echo ${a[-1]}").0, "z\n");
        assert_eq!(run("a=(x y z); echo ${a[-2]}").0, "y\n");
        assert_eq!(run("a=(x y z); echo ${a[-3]}").0, "x\n");
        // Out-of-range negative → empty (bash treats it as a bad subscript).
        assert_eq!(run("a=(x y z); echo [${a[-4]}]").0, "[]\n");
        // Length of the last element via ${#a[-1]}.
        assert_eq!(run("a=(x yy zzz); echo ${#a[-1]}").0, "3\n");
        // A scalar behaves as a one-element array: [-1] is the value.
        assert_eq!(run("x=hello; echo ${x[-1]}").0, "hello\n");
        // Negative index in an assignment target overwrites from the end.
        assert_eq!(run("a=(x y z); a[-1]=Q; echo ${a[@]}").0, "x y Q\n");
        assert_eq!(run("a=(x y z); a[-2]=Q; echo ${a[@]}").0, "x Q z\n");
        // Out-of-range negative assignment is a no-op error (array unchanged).
        assert_eq!(run("a=(x y); a[-9]=Q; echo ${a[@]}").0, "x y\n");
    }

    #[test]
    fn arith_array_subscript() {
        // Array elements are addressable inside $(( … )) and (( … )).
        assert_eq!(run("a=(10 20 30); echo $(( a[1] ))").0, "20\n");
        assert_eq!(run("a=(10 20 30); i=2; echo $(( a[i] + 1 ))").0, "31\n");
        assert_eq!(run("a=(10 20 30); echo $(( a[i+1] ))").0, "20\n"); // i unset → 0, a[1]
        // Negative subscript inside arithmetic (last element).
        assert_eq!(run("a=(10 20 30); echo $(( a[-1] ))").0, "30\n");
        // A (( … )) command: a[0] is non-zero → success (exit 0).
        assert_eq!(run("a=(5 0); (( a[0] ))").1, 0);
        assert_eq!(run("a=(5 0); (( a[1] ))").1, 1); // zero → exit 1
    }

    #[test]
    fn array_element_with_operator() {
        // `:-` use-default on a present vs. absent element.
        assert_eq!(run("a=(x y z); echo ${a[1]:-def}").0, "y\n");
        assert_eq!(run("a=(x y z); echo ${a[9]:-def}").0, "def\n");
        // Negative subscript combined with an operator.
        assert_eq!(run("a=(x y z); echo ${a[-1]:-def}").0, "z\n");
        assert_eq!(run("a=(x y z); echo ${a[-9]:-def}").0, "def\n");
        // `:+` use-alternate: element set → arg; unset → empty.
        assert_eq!(run("a=(x y); echo [${a[0]:+set}]").0, "[set]\n");
        assert_eq!(run("a=(x y); echo [${a[5]:+set}]").0, "[]\n");
        // Prefix/suffix trim on an element.
        assert_eq!(run("a=(foo.txt bar.md); echo ${a[0]%.txt}").0, "foo\n");
        assert_eq!(run("a=(abcabc); echo ${a[0]#a}").0, "bcabc\n");
        // Substring on an element.
        assert_eq!(run("a=(hello world); echo ${a[1]:0:3}").0, "wor\n");
        // Pattern replacement on an element.
        assert_eq!(run("a=(a-b-c); echo ${a[0]//-/_}").0, "a_b_c\n");
        // `:=` assign-default writes the element back.
        assert_eq!(run("a=(x); echo ${a[2]:=new}; echo ${a[2]}").0, "new\nnew\n");
        // Associative element with an operator (string key).
        assert_eq!(
            run("declare -A m; m[k]=v; echo ${m[k]:-def} ${m[nope]:-def}").0,
            "v def\n"
        );
        // Combining `[@]`/`[*]` with an operator is rejected at parse time.
        assert!(parse("echo ${a[@]:-def}").is_err());
        assert!(parse("echo ${a[*]#pat}").is_err());
    }

    #[test]
    fn sparse_indexed_array() {
        // A sparse literal does NOT fill the gaps: only the assigned indices
        // exist, so the element count is the number of set elements.
        assert_eq!(run("a=([5]=x); echo ${#a[@]}").0, "1\n");
        assert_eq!(run("a=([5]=x); echo ${!a[@]}").0, "5\n");
        // Multiple explicit indices keep their gaps; `${a[@]}` and `${!a[@]}`
        // iterate in ascending-index order.
        assert_eq!(run("a=([2]=a [5]=b); echo ${a[@]}").0, "a b\n");
        assert_eq!(run("a=([2]=a [5]=b); echo ${!a[@]}").0, "2 5\n");
        // A plain `${a}` reads index 0 specifically — empty when unset.
        assert_eq!(run("a=([5]=x); echo [${a}]").0, "[]\n");
        assert_eq!(run("a=([5]=x); echo [${a[0]}]").0, "[]\n");
        // Negative index counts from the highest index + 1, not the count.
        assert_eq!(run("a=([2]=a [5]=b); echo ${a[-1]}").0, "b\n");
        // `unset a[i]` removes only that index (leaves a gap, no shift down).
        assert_eq!(run("a=(x y z); unset a[1]; echo ${!a[@]}").0, "0 2\n");
        assert_eq!(run("a=(x y z); unset a[1]; echo ${a[@]}").0, "x z\n");
        // Positional elements after a keyed one resume at that index + 1.
        assert_eq!(run("a=([2]=x y); echo ${!a[@]}").0, "2 3\n");
        // A sparse `a[i]=v` past the end adds one entry, not a filled range.
        assert_eq!(run("a=([5]=x); a[10]=y; echo ${#a[@]}").0, "2\n");
    }

    #[test]
    fn array_quoted_all_preserves_fields() {
        // "${a[@]}" keeps element boundaries even with embedded spaces.
        let out = run(r#"a=("a b" c); for w in "${a[@]}"; do echo "[$w]"; done"#).0;
        assert_eq!(out, "[a b]\n[c]\n");
    }

    #[test]
    fn array_unquoted_all_splits() {
        // Unquoted ${a[@]} field-splits, so "a b" becomes two words.
        let out = run(r#"a=("a b" c); for w in ${a[@]}; do echo "[$w]"; done"#).0;
        assert_eq!(out, "[a]\n[b]\n[c]\n");
    }

    #[test]
    fn array_unset() {
        assert_eq!(run("a=(x y z); unset a[1]; echo ${a[@]}").0, "x z\n");
        assert_eq!(run("a=(x y z); unset a; echo ${#a[@]}").0, "0\n");
    }

    #[test]
    fn array_from_glob_and_expansion() {
        // Array elements undergo splitting/expansion.
        assert_eq!(run("s='p q'; a=($s r); echo ${#a[@]}").0, "3\n");
    }

    #[test]
    fn array_keyed_literal_indexed() {
        // `[i]=v` elements place at an explicit index; positional resume after.
        assert_eq!(run("a=([2]=x y); echo ${a[2]} ${a[3]}").0, "x y\n");
        assert_eq!(run("a=(p [5]=q); echo ${a[0]} ${a[5]}").0, "p q\n");
    }

    #[test]
    fn assoc_basic_set_and_read() {
        let src = "declare -A m; m[foo]=1; m[bar]=2; echo ${m[foo]} ${m[bar]}";
        assert_eq!(run(src).0, "1 2\n");
    }

    #[test]
    fn assoc_all_values_and_keys() {
        // Values and keys come back in insertion order.
        assert_eq!(run("declare -A m; m[a]=x; m[b]=y; echo ${m[@]}").0, "x y\n");
        assert_eq!(run("declare -A m; m[a]=x; m[b]=y; echo ${!m[@]}").0, "a b\n");
    }

    #[test]
    fn assoc_length_and_overwrite() {
        assert_eq!(run("declare -A m; m[a]=1; m[b]=2; echo ${#m[@]}").0, "2\n");
        // Re-assigning a key overwrites in place (count unchanged).
        let src = "declare -A m; m[k]=1; m[k]=2; echo ${m[k]}; echo ${#m[@]}";
        assert_eq!(run(src).0, "2\n1\n");
    }

    #[test]
    fn assoc_literal_init() {
        let src = "declare -A m; m=([x]=1 [y]=2); echo ${m[x]} ${m[y]}; echo ${#m[@]}";
        assert_eq!(run(src).0, "1 2\n2\n");
    }

    #[test]
    fn assoc_expanded_key() {
        // The subscript on assignment/read is a string key, expanded not arith'd.
        assert_eq!(run("declare -A m; k=foo; m[$k]=bar; echo ${m[foo]}").0, "bar\n");
    }

    #[test]
    fn assoc_unset_key() {
        let src = "declare -A m; m[a]=1; m[b]=2; unset m[a]; echo ${!m[@]}; echo ${#m[@]}";
        assert_eq!(run(src).0, "b\n1\n");
    }

    #[test]
    fn assoc_quoted_all_preserves_fields() {
        let src = r#"declare -A m; m[a]="x y"; m[b]=z; for v in "${m[@]}"; do echo "[$v]"; done"#;
        assert_eq!(run(src).0, "[x y]\n[z]\n");
    }

    #[test]
    fn assoc_bare_ref_reads_key_zero() {
        // `$m` on an associative array reads the value at key "0", not "first".
        assert_eq!(run("declare -A m; m[foo]=a; m[0]=z; echo $m").0, "z\n");
    }

    #[test]
    fn declare_assoc_oneliner() {
        // The combined `declare -A m=([k]=v)` form now works in one statement.
        let src = "declare -A m=([x]=1 [y]=2); echo ${m[x]} ${m[y]}; echo ${#m[@]}";
        assert_eq!(run(src).0, "1 2\n2\n");
    }

    #[test]
    fn declare_indexed_oneliner() {
        // `declare -a a=(x y z)` (and the flagless `declare a=(…)`) make an
        // indexed array in one statement.
        assert_eq!(run("declare -a a=(x y z); echo ${a[1]} ${#a[@]}").0, "y 3\n");
        assert_eq!(run("declare a=(p q); echo ${a[@]}").0, "p q\n");
    }

    #[test]
    fn declare_assoc_oneliner_is_associative() {
        // A `-A` one-liner must be associative (string subscripts), not indexed:
        // a non-numeric key must round-trip.
        let src = "declare -A m=([foo]=bar); echo ${m[foo]}; echo ${!m[@]}";
        assert_eq!(run(src).0, "bar\nfoo\n");
    }

    /// A unique cwd-relative temp path (no `set_current_dir`, so parallel-safe).
    fn uniq_path(tag: &str) -> String {
        format!(
            "osh_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        )
    }

    #[test]
    fn compound_while_read_from_file() {
        let path = uniq_path("whileread");
        std::fs::write(&path, "alpha\nbeta\ngamma\n").expect("write");
        let (o, _) = run(&format!(
            "while read line; do echo \"got:$line\"; done < {path}"
        ));
        std::fs::remove_file(&path).ok();
        assert_eq!(o, "got:alpha\ngot:beta\ngot:gamma\n");
    }

    #[test]
    fn compound_for_loop_output_redirect() {
        let path = uniq_path("forout");
        let (_, _) = run(&format!("for x in 1 2 3; do echo n$x; done > {path}"));
        let contents = std::fs::read_to_string(&path).expect("read");
        std::fs::remove_file(&path).ok();
        assert_eq!(contents, "n1\nn2\nn3\n");
    }

    #[test]
    fn compound_output_append_redirect() {
        let path = uniq_path("append");
        run(&format!("for x in a b; do echo $x; done > {path}"));
        run(&format!("for x in c d; do echo $x; done >> {path}"));
        let contents = std::fs::read_to_string(&path).expect("read");
        std::fs::remove_file(&path).ok();
        assert_eq!(contents, "a\nb\nc\nd\n");
    }

    #[test]
    fn compound_brace_group_redirect() {
        let path = uniq_path("brace");
        run(&format!("{{ echo one; echo two; }} > {path}"));
        let contents = std::fs::read_to_string(&path).expect("read");
        std::fs::remove_file(&path).ok();
        assert_eq!(contents, "one\ntwo\n");
    }

    #[test]
    fn compound_while_read_from_heredoc() {
        let (o, _) = run("while read l; do echo [$l]; done <<EOF\nfoo\nbar\nEOF");
        assert_eq!(o, "[foo]\n[bar]\n");
    }

    #[test]
    fn compound_stderr_redirect_to_file() {
        // `2> file` on a brace group must capture the group's stderr (including
        // an inner `>&2`) to the file while stdout still reaches the outer sink.
        let path = uniq_path("stderrfile");
        let (o, _) = run(&format!(
            "{{ echo out; echo err >&2; }} 2> {path}"
        ));
        let contents = std::fs::read_to_string(&path).expect("read");
        std::fs::remove_file(&path).ok();
        assert_eq!(o, "out\n");
        assert_eq!(contents, "err\n");
    }

    #[test]
    fn compound_stderr_append_redirect() {
        let path = uniq_path("stderrappend");
        run(&format!("{{ echo a >&2; }} 2> {path}"));
        run(&format!("{{ echo b >&2; }} 2>> {path}"));
        let contents = std::fs::read_to_string(&path).expect("read");
        std::fs::remove_file(&path).ok();
        assert_eq!(contents, "a\nb\n");
    }

    #[test]
    fn compound_stderr_to_stdout_in_capture() {
        // `2>&1` inside command substitution folds the group's stderr into the
        // captured stdout.
        let (o, _) = run("x=$( { echo a; echo b >&2; } 2>&1 ); echo \"$x\"");
        assert_eq!(o, "a\nb\n");
    }

    #[test]
    fn compound_stderr_to_stdout_top_level_capture() {
        // Same merge when the outer sink is a plain capture (not a subshell).
        let (o, _) = run("{ echo a; echo b >&2; } 2>&1");
        assert_eq!(o, "a\nb\n");
    }

    #[test]
    fn compound_for_loop_stderr_redirect() {
        let path = uniq_path("forstderr");
        run(&format!(
            "for x in 1 2; do echo e$x >&2; done 2> {path}"
        ));
        let contents = std::fs::read_to_string(&path).expect("read");
        std::fs::remove_file(&path).ok();
        assert_eq!(contents, "e1\ne2\n");
    }

    #[test]
    fn compound_read_count_lines() {
        // Classic idiom: count lines via a redirected while-read loop.
        let path = uniq_path("count");
        std::fs::write(&path, "l1\nl2\nl3\nl4\n").expect("write");
        let (o, _) = run(&format!(
            "n=0; while read _; do n=$((n+1)); done < {path}; echo $n"
        ));
        std::fs::remove_file(&path).ok();
        assert_eq!(o, "4\n");
    }

    #[test]
    fn pipeline_into_while_read() {
        // Feeding a while-read loop from a pipeline must consume successive
        // lines streamed over the connecting OS pipe.
        let (o, _) = run("printf 'x\\ny\\nz\\n' | while read v; do echo \"<$v>\"; done");
        assert_eq!(o, "<x>\n<y>\n<z>\n");
    }

    #[test]
    fn threaded_pipeline_builtin_stages_stream() {
        // Two in-process stages (printf → a while-read that filters) connected
        // by a real pipe; the threaded path must carry and transform the data.
        let (o, _) = run(
            "printf 'a\\nbb\\nc\\n' | while read v; do echo \"$v$v\"; done | while read w; do echo \"[$w]\"; done",
        );
        assert_eq!(o, "[aa]\n[bbbb]\n[cc]\n");
    }

    #[test]
    fn threaded_pipeline_stage_runs_in_subshell() {
        // A pipeline stage's variable mutation must NOT leak to the parent
        // shell (each stage is a subshell — bash semantics, no lastpipe).
        let (o, _) = run("v=outer; printf 'inner\\n' | read v; echo $v");
        assert_eq!(o, "outer\n");
    }

    #[test]
    fn pipeline_classifier_routes_external_vs_builtin() {
        let mut sh = Shell::new();
        sh.funcs.insert("myfn".to_string(), parse("echo hi").unwrap());
        let classify = |sh: &Shell, src: &str| -> bool {
            let prog = parse(src).unwrap();
            let cmds = &prog.items[0].list.first.commands;
            cmds.iter().all(|c| sh.stage_is_plain_external(c))
        };
        // All-external → real-pipe (concurrent) path.
        assert!(classify(&sh, "cat a | grep b | wc -l"));
        // A builtin stage → threaded path.
        assert!(!classify(&sh, "cat a | echo hi"));
        assert!(!classify(&sh, "printf x | cat"));
        // A per-stage redirection → threaded path.
        assert!(!classify(&sh, "cat a | grep b > out"));
        // A command word needing expansion can't be proven external → threaded.
        assert!(!classify(&sh, "$cmd a | cat"));
        // A shell function stage → threaded path.
        assert!(!classify(&sh, "cat a | myfn"));
        // A compound stage → threaded path.
        assert!(!classify(&sh, "cat a | while read x; do echo $x; done"));
    }

    // NOTE: there is deliberately no `external_producer_terminates_early` test.
    // Terminating an *unbounded external* producer when its downstream consumer
    // exits early is an OS-signal property (bash relies on SIGPIPE; the slateos
    // target delivers EPIPE), not shell logic. The Windows test host cannot
    // exercise it faithfully: `cmd`'s `echo` ignores broken-pipe write errors
    // and loops forever, so `Child::wait` never returns. The shell-side cascade
    // (a stage stopping once its write end breaks) is covered by the in-process
    // test below. See known-issues.md TD-OILS4.
    #[test]
    fn threaded_pipeline_inprocess_producer_terminates_early() {
        // An unbounded *in-process* producer (`while true; do echo`) feeding a
        // consumer that stops after one line must terminate via the pipe_broken
        // (SIGPIPE analogue) flag rather than looping forever.
        use std::sync::mpsc;
        let (tx, rx) = mpsc::channel();
        let h = std::thread::spawn(move || {
            let (o, _) = run("while true; do echo x; done | while read v; do echo got; break; done");
            let _ = tx.send(o);
        });
        let out = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("in-process producer should stop on broken pipe, not hang");
        h.join().ok();
        assert_eq!(out, "got\n");
    }

    // The following exercise the real-OS-pipe path with actual external
    // processes; they use `cmd` (always present on the Windows test host).
    #[cfg(windows)]
    #[test]
    fn concurrent_pipeline_carries_data() {
        // Two `cmd` echoes feed Windows `sort`; the pipe must carry both lines.
        let (o, _) = run(r#"cmd /c "echo b&echo a" | sort"#);
        let norm = o.replace("\r\n", "\n");
        assert_eq!(norm, "a\nb\n");
    }

    #[cfg(windows)]
    #[test]
    fn pipeline_stage_stdout_redirect_composes_with_pipe() {
        // An external stage that carries its own `> file` redirect takes the
        // threaded path (it is not "plain external"); the pipe must still feed
        // its stdin while the redirect captures its stdout. Producer `cmd echo`
        // → consumer `findstr` (matches every line) → file.
        let f = std::env::temp_dir().join("osh_pipe_redir_stdout.txt");
        let _ = std::fs::remove_file(&f);
        let fp = f.to_string_lossy().replace('\\', "/");
        let script = format!(r#"cmd /c "echo hi" | findstr "h" > "{fp}""#);
        run(&script);
        let content = std::fs::read_to_string(&f).unwrap_or_default();
        let _ = std::fs::remove_file(&f);
        assert_eq!(content.replace("\r\n", "\n"), "hi\n");
    }

    #[cfg(windows)]
    #[test]
    fn pipeline_stage_stderr_redirect_composes_with_pipe() {
        // The last stage's own `2> file` must capture its stderr even though its
        // stdin is the inter-stage pipe. The consumer ignores stdin and writes
        // to its stderr, which the redirect diverts to the file.
        let f = std::env::temp_dir().join("osh_pipe_redir_stderr.txt");
        let _ = std::fs::remove_file(&f);
        let fp = f.to_string_lossy().replace('\\', "/");
        let script = format!(r#"cmd /c "echo E 1>&2" 2> "{fp}" | cmd /c "sort""#);
        run(&script);
        let content = std::fs::read_to_string(&f).unwrap_or_default();
        let _ = std::fs::remove_file(&f);
        // `echo E 1>&2` emits "E " (cmd keeps the space before the redirect),
        // so the diverted stderr is "E \r\n". The point is that the redirect
        // captured the stage's stderr while its stdout fed the downstream pipe.
        assert_eq!(content.replace("\r\n", "\n"), "E \n");
    }

    #[cfg(windows)]
    #[test]
    fn concurrent_pipeline_status_is_last_stage() {
        // `$?` reflects the last stage, not earlier ones (no pipefail).
        assert_eq!(run("cmd /c exit 0 | cmd /c exit 5").1, 5);
        assert_eq!(run("cmd /c exit 7 | cmd /c exit 0").1, 0);
    }

    #[cfg(windows)]
    #[test]
    fn pipefail_reports_rightmost_nonzero_stage() {
        // With pipefail, a middle/leftmost failure surfaces even though the last
        // stage succeeds; the rightmost non-zero stage wins.
        assert_eq!(run("set -o pipefail; cmd /c exit 7 | cmd /c exit 0").1, 7);
        assert_eq!(
            run("set -o pipefail; cmd /c exit 3 | cmd /c exit 5 | cmd /c exit 0").1,
            5
        );
        // All-success pipeline is still 0 under pipefail.
        assert_eq!(run("set -o pipefail; cmd /c exit 0 | cmd /c exit 0").1, 0);
        // `set +o pipefail` restores last-stage semantics.
        assert_eq!(
            run("set -o pipefail; set +o pipefail; cmd /c exit 7 | cmd /c exit 0").1,
            0
        );
    }

    #[cfg(windows)]
    #[test]
    fn pipestatus_array_records_every_stage() {
        // `${PIPESTATUS[@]}` holds one code per stage, in pipeline order.
        let (o, _) =
            run(r#"cmd /c exit 3 | cmd /c exit 0 | cmd /c exit 5; echo "${PIPESTATUS[@]}""#);
        assert_eq!(o, "3 0 5\n");
        // Individual subscripts are addressable.
        let (o, _) = run(r#"cmd /c exit 4 | cmd /c exit 0; echo "${PIPESTATUS[0]}""#);
        assert_eq!(o, "4\n");
    }

    #[test]
    fn pipestatus_single_command_has_one_element() {
        // A bare command still populates a one-element PIPESTATUS.
        let (o, _) = run(r#"true; echo "${PIPESTATUS[@]}""#);
        assert_eq!(o, "0\n");
        let (o, _) = run(r#"false; echo "${PIPESTATUS[@]}""#);
        assert_eq!(o, "1\n");
    }

    #[test]
    fn pipefail_buffered_path_folds_status() {
        // The buffered path (builtin stages) also honours pipefail + PIPESTATUS.
        // `false | true` — last stage true, but pipefail surfaces the failure.
        let mut sh = Shell::new();
        assert_eq!(sh.run_source("set -o pipefail; false | true"), 1);
        let (o, _) = run(r#"false | true; echo "${PIPESTATUS[@]}""#);
        assert_eq!(o, "1 0\n");
    }

    #[cfg(windows)]
    #[test]
    fn wait_reaps_background_job_status() {
        // A `&` command is tracked as a job and sets `$!`; `wait` (no operand)
        // blocks until it finishes and yields its exit status.
        let mut sh = Shell::new();
        assert_eq!(sh.run_source("cmd /c exit 7 &"), 0);
        assert!(sh.last_bg_pid.is_some());
        assert_eq!(sh.jobs.len(), 1);
        assert_eq!(sh.run_source("wait"), 7);
        assert!(sh.jobs.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn wait_by_pid_and_job_spec() {
        // `wait PID` and `wait %n` both target a specific job.
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 3 &");
        let pid = sh.last_bg_pid.expect("bg pid");
        assert_eq!(sh.run_source(&format!("wait {pid}")), 3);
        assert!(sh.jobs.is_empty());

        sh.run_source("cmd /c exit 4 &");
        assert_eq!(sh.run_source("wait %1"), 4);
        assert!(sh.jobs.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn jobs_lists_background_job() {
        // `jobs` reports the tracked job with its job number and command line.
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 0 &");
        let mut buf = Vec::new();
        {
            let prog = parse("jobs").expect("parse");
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
        }
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("[1]"), "jobs output: {s:?}");
        assert!(s.contains("cmd /c exit 0"), "jobs output: {s:?}");
        // `wait` cleans up any still-tracked job for a tidy teardown.
        sh.run_source("wait");
    }

    #[test]
    fn exec_no_command_is_noop() {
        // `exec` with no command word is a no-op that keeps running the script.
        let (o, s) = run("exec\necho hi");
        assert_eq!(o, "hi\n");
        assert_eq!(s, 0);
    }

    #[cfg(windows)]
    #[test]
    fn exec_replaces_shell_and_stops() {
        // `exec cmd` runs the command and the shell does not continue past it.
        let mut sh = Shell::new();
        let st = sh.run_source("exec cmd /c exit 5\nAFTER=1");
        assert_eq!(st, 5);
        assert!(!sh.vars.contains_key("AFTER"));
    }

    #[cfg(windows)]
    #[test]
    fn exec_missing_command_exits_127() {
        // A failed `exec` of a missing command exits the shell with 127.
        let mut sh = Shell::new();
        let st = sh.run_source("exec no_such_command_xyz_123\nAFTER=1");
        assert_eq!(st, 127);
        assert!(!sh.vars.contains_key("AFTER"));
    }
}
