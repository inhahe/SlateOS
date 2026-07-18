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
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, CaseClause, Command, CondBinOp,
    CondExpr,
    ForArithClause, ForClause, IfClause, LoopClause, ParamOp, Pipeline, Program, Redirect,
    RedirectOp,
    ReplaceAnchor, SimpleCommand, UnaryOp, Word, WordPart,
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
        let flow = self.exec_pipeline(&ao.first, out, stdin);
        if !matches!(flow, Flow::Next) {
            return flow;
        }
        for (op, pipe) in &ao.rest {
            let run = match op {
                AndOrOp::And => self.last_status == 0,
                AndOrOp::Or => self.last_status != 0,
            };
            if run {
                let flow = self.exec_pipeline(pipe, out, stdin);
                if !matches!(flow, Flow::Next) {
                    return flow;
                }
            }
        }
        Flow::Next
    }

    fn exec_pipeline(&mut self, pipe: &Pipeline, out: &mut Out, stdin: &StdinSrc) -> Flow {
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
        flow
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
        let flow = self.exec_program(&c.cond, out, stdin);
        if !matches!(flow, Flow::Next) {
            return flow;
        }
        if self.last_status == 0 {
            return self.exec_program(&c.body, out, stdin);
        }
        for (cond, body) in &c.elifs {
            let flow = self.exec_program(cond, out, stdin);
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
        loop {
            let flow = self.exec_program(&c.cond, out, stdin);
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
        }
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
        }
        Flow::Next
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
        self.last_status = 0;
        for item in &c.items {
            for pat in &item.patterns {
                let pattern: Vec<char> = self.expand_to_string(pat).chars().collect();
                if glob_match(&pattern, &subject) {
                    return self.exec_program(&item.body, out, stdin);
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
        let re = match crate::ere::Regex::new(&pattern) {
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
                    UnaryOp::ZeroLen | UnaryOp::NonZeroLen => unreachable!(),
                }
            }
        }
    }

    fn cond_binary(&mut self, l: &Word, op: CondBinOp, r: &Word) -> bool {
        match op {
            CondBinOp::StrEq | CondBinOp::StrNe => {
                let subject: Vec<char> = self.expand_to_string(l).chars().collect();
                let rhs = self.expand_to_string(r);
                // A fully-quoted RHS is a literal; otherwise it is a glob pattern.
                let matched = if word_is_all_quoted(r) {
                    subject.iter().collect::<String>() == rhs
                } else {
                    let pat: Vec<char> = rhs.chars().collect();
                    glob_match(&pat, &subject)
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
        }
    }

    // ---- assignments and arrays ---------------------------------------------

    /// Apply a standalone assignment to shell state, handling scalars, indexed
    /// elements (`name[i]=v`), whole arrays (`name=(a b c)`), and append (`+=`).
    fn apply_assignment(&mut self, a: &Assignment) {
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
                        return;
                    }
                    if a.name == "SECONDS" {
                        self.seconds_base = val.trim().parse::<u64>().unwrap_or(0);
                        self.seconds_anchor = std::time::Instant::now();
                        return;
                    }
                }
                if let Some(idx_word) = &a.index {
                    if is_assoc {
                        // `name[key]=val` — associative element (string key).
                        let key = self.expand_to_string(idx_word);
                        self.assoc_set(&a.name, key, val, a.append);
                    } else {
                        // `name[i]=val` — indexed element assignment. A negative
                        // index counts back from `highest_index + 1` (bash:
                        // `a[-1]=v` overwrites the last element).
                        let raw = self.eval_arith_word(idx_word);
                        let arr = self.arrays.entry(a.name.clone()).or_default();
                        let bound = arr.keys().next_back().map_or(0, |k| k.saturating_add(1));
                        let Some(idx) = Self::resolve_index(raw, bound) else {
                            eprintln!("osh: {}: bad array subscript", a.name);
                            return;
                        };
                        if a.append {
                            arr.entry(idx).or_default().push_str(&val);
                        } else {
                            arr.insert(idx, val);
                        }
                    }
                } else if a.append {
                    // `name+=val` — append to the scalar (or to element 0 of an array).
                    if let Some(arr) = self.arrays.get_mut(&a.name) {
                        arr.entry(0).or_default().push_str(&val);
                    } else {
                        let cur = self.vars.get(&a.name).cloned().unwrap_or_default();
                        self.vars.insert(a.name.clone(), cur + &val);
                    }
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

        // Pure assignment (no command word): persist the variables/arrays.
        if argv.is_empty() {
            for a in &sc.assignments {
                self.apply_assignment(a);
            }
            self.last_status = 0;
            return Flow::Next;
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

        let flow = self.exec_program(&body, out, stdin);

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
                        self.last_bg_pid = Some(child.id());
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
            let mut out = Vec::new();
            for f in fields {
                glob_or_literal(&f, &mut out);
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
            WordPart::Param(name) => self.param_value(name).unwrap_or_default(),
            WordPart::Length(name) => self
                .param_value(name)
                .map_or(0, |v| v.chars().count())
                .to_string(),
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
                param_trim(&value, &pat, *suffix, *longest)
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
                param_replace(&value, &pat, &repl, *all, *anchor)
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
                param_case(&value, &pat, *upper, *all)
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
            "echo" => self.builtin_echo(args, out, redir),
            "printf" => self.builtin_printf(args, out, redir),
            "export" => self.builtin_export(args),
            "declare" | "typeset" | "local" => self.builtin_declare(args),
            "unset" => self.builtin_unset(args),
            "set" => self.builtin_set(args),
            "shift" => self.builtin_shift(args),
            "getopts" => self.builtin_getopts(args),
            "mapfile" | "readarray" => self.builtin_mapfile(args, stdin, redir),
            "read" => self.builtin_read(args, stdin, redir),
            "test" | "[" => self.builtin_test(name, args),
            "eval" => {
                let joined = args.join(" ");
                self.run_source(&joined)
            }
            "source" | "." => self.builtin_source(args),
            "type" => self.builtin_type(args, out, redir),
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

    fn builtin_cd(&mut self, args: &[String]) -> i32 {
        let target = match args.first() {
            Some(p) => p.clone(),
            None => self.param_value("HOME").unwrap_or_else(|| "/".to_string()),
        };
        match std::env::set_current_dir(&target) {
            Ok(()) => {
                if let Ok(cwd) = std::env::current_dir() {
                    self.vars
                        .insert("PWD".to_string(), cwd.to_string_lossy().into_owned());
                }
                0
            }
            Err(e) => {
                eprintln!("osh: cd: {target}: {e}");
                1
            }
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
        let Some(fmt) = args.first() else {
            return 0;
        };
        let text = format_printf(fmt, &args[1..]);
        self.write_bytes(out, redir, text.as_bytes())
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
    /// (associative array), `-a` (indexed array), and scalar `name=value`.
    /// Other type flags (`-r`, `-x`, `-i`, `-g`, …) are accepted but only
    /// `-x`/`-g`'s export effect via a following `export` is honoured elsewhere;
    /// here they are parsed and ignored. The combined form `declare -A m=(…)`
    /// is not supported (parse restriction) — use `declare -A m; m=([k]=v …)`.
    fn builtin_declare(&mut self, args: &[String]) -> i32 {
        let mut assoc = false;
        let mut indexed = false;
        let mut export = false;
        let mut i = 0;
        while let Some(arg) = args.get(i) {
            if arg == "--" {
                i += 1;
                break;
            }
            if let Some(flags) = arg.strip_prefix('-') {
                for c in flags.chars() {
                    match c {
                        'A' => assoc = true,
                        'a' => indexed = true,
                        'x' => export = true,
                        _ => {} // -r/-i/-g/-l/-u/-n/-p: accepted, no effect here.
                    }
                }
                i += 1;
            } else {
                break;
            }
        }
        for name_val in &args[i..] {
            let (name, value) = match name_val.find('=') {
                Some(eq) => (&name_val[..eq], Some(name_val[eq + 1..].to_string())),
                None => (name_val.as_str(), None),
            };
            if name.is_empty() {
                continue;
            }
            if assoc {
                self.assoc.entry(name.to_string()).or_default();
            } else if indexed {
                self.arrays.entry(name.to_string()).or_default();
            }
            if let Some(v) = value {
                if assoc || indexed {
                    // `declare -A m=str` / `-a a=str` — scalar init unsupported;
                    // ignore the value (bash would treat str as element/key).
                } else {
                    self.vars.insert(name.to_string(), v);
                }
            }
            if export {
                self.exported.insert(name.to_string());
            }
        }
        0
    }

    /// Handle the combined `declare -A m=([k]=v)` / `declare -a a=(x y)` form,
    /// where the array literal is an operand of a declaration builtin. Flags and
    /// any scalar/plain operands in `argv` go through [`Shell::builtin_declare`];
    /// each array literal is then marked with the declared kind (`-A` → assoc,
    /// `-a`/default → indexed) and applied via [`Shell::apply_assignment`].
    fn exec_declare_with_arrays(&mut self, argv: &[String], decl_arrays: &[Assignment]) -> Flow {
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
        let status = self.builtin_declare(&argv[1..]);
        // Mark each array name's kind before applying, so `apply_assignment`
        // routes the literal to the associative or indexed store correctly.
        for a in decl_arrays {
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

    fn builtin_unset(&mut self, args: &[String]) -> i32 {
        for a in args {
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
        }
        0
    }

    fn builtin_set(&mut self, args: &[String]) -> i32 {
        // `set -o pipefail` / `set +o pipefail` toggle the pipefail option.
        // (`set -o` / `set +o` with an option name; only pipefail is honoured,
        // other options are accepted and ignored.)
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-o" | "+o" => {
                    let enable = args[i].starts_with('-');
                    if let Some(opt) = args.get(i + 1) {
                        if opt == "pipefail" {
                            self.pipefail = enable;
                        }
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                "--" => {
                    self.positional = args[i + 1..].to_vec();
                    return 0;
                }
                other if !other.starts_with('-') && !other.starts_with('+') => {
                    self.positional = args[i..].to_vec();
                    return 0;
                }
                _ => {
                    i += 1;
                }
            }
        }
        0
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

    fn builtin_read(&mut self, args: &[String], stdin: &StdinSrc, redir: &RedirPlan) -> i32 {
        let line = match self.read_line(stdin, redir) {
            Some(l) => l,
            None => return 1, // EOF
        };
        let names: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
        if names.is_empty() {
            self.vars.insert("REPLY".to_string(), line);
            return 0;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        for (i, name) in names.iter().enumerate() {
            let val = if i + 1 == names.len() {
                // Last variable gets the remaining fields joined.
                fields[i.min(fields.len())..].join(" ")
            } else {
                fields.get(i).map_or(String::new(), |s| (*s).to_string())
            };
            self.vars.insert((*name).clone(), val);
        }
        0
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
        let mut status = 0;
        for a in args {
            let desc = if self.funcs.contains_key(a) {
                format!("{a} is a function")
            } else if is_builtin(a) {
                format!("{a} is a shell builtin")
            } else {
                status = 1;
                format!("{a}: not found")
            };
            let _ = self.write_line(out, redir, &desc);
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
fn glob_or_literal(field: &[EChar], out: &mut Vec<String>) {
    let has_meta = field
        .iter()
        .any(|e| !e.quoted && matches!(e.c, '*' | '?' | '['));
    let literal: String = field.iter().map(|e| e.c).collect();
    if !has_meta {
        out.push(literal);
        return;
    }
    let mut matches = glob_expand_field(field);
    if matches.is_empty() {
        out.push(literal);
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
}

enum ClassItem {
    Ch(char),
    Range(char, char),
}

/// Compile one annotated path component into glob tokens. Quoted characters are
/// always literal; unquoted `* ? [` are special.
fn compile_glob(comp: &[EChar]) -> Vec<PatTok> {
    let mut toks = Vec::new();
    let mut i = 0;
    while i < comp.len() {
        let e = comp[i];
        if e.quoted {
            toks.push(PatTok::Lit(e.c));
            i += 1;
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

/// Match a compiled glob against a filename (anchored, star-backtracking).
fn match_glob_toks(toks: &[PatTok], name: &[char]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ti < name.len() {
        if matches!(toks.get(pi), Some(PatTok::Star)) {
            star = Some((pi, ti));
            pi += 1;
            continue;
        }
        let matched = match toks.get(pi) {
            Some(PatTok::Any) => true,
            Some(PatTok::Lit(c)) => *c == name[ti],
            Some(PatTok::Class { negate, items }) => {
                class_matches(items, name[ti]) ^ *negate
            }
            _ => false,
        };
        if matched {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while matches!(toks.get(pi), Some(PatTok::Star)) {
        pi += 1;
    }
    pi == toks.len()
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
fn glob_expand_field(field: &[EChar]) -> Vec<String> {
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
    let mut cands: Vec<String> = vec![if absolute { "/".to_string() } else { String::new() }];
    for comp in &comps {
        let has_meta = comp
            .iter()
            .any(|e| !e.quoted && matches!(e.c, '*' | '?' | '['));
        let comp_literal: String = comp.iter().map(|e| e.c).collect();
        let mut next: Vec<String> = Vec::new();
        for base in &cands {
            if has_meta {
                let dir = if base.is_empty() { "." } else { base.as_str() };
                let toks = compile_glob(comp);
                let allow_dot = glob_starts_with_dot(&toks);
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
                    if match_glob_toks(&toks, &nch) {
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

/// Match `text` against a shell glob `pattern` (`*`, `?`, `[...]`), anchored at
/// both ends (as `case` patterns and `[[ … == … ]]` require). Uses iterative
/// star-backtracking so it runs in linear space and near-linear time.
fn glob_match(pattern: &[char], text: &[char]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    // Last '*' position in the pattern and the text index it was matched at, so
    // we can backtrack and let the star consume one more character.
    let mut star: Option<(usize, usize)> = None;
    while ti < text.len() {
        if pi < pattern.len() && pattern[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
            continue;
        }
        let m = if pi < pattern.len() {
            glob_match_one(pattern, pi, text[ti])
        } else {
            None
        };
        match m {
            Some((true, next)) => {
                pi = next;
                ti += 1;
            }
            _ => {
                if let Some((sp, st)) = star {
                    pi = sp + 1;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                } else {
                    return false;
                }
            }
        }
    }
    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }
    pi == pattern.len()
}

/// Match a single non-`*` pattern element at `pi` against `ch`. Returns
/// `(matched, index-after-the-element)`, or `None` if the pattern is exhausted.
fn glob_match_one(pattern: &[char], pi: usize, ch: char) -> Option<(bool, usize)> {
    match pattern.get(pi)? {
        '?' => Some((true, pi + 1)),
        '[' => Some(glob_match_class(pattern, pi, ch)),
        c => Some((*c == ch, pi + 1)),
    }
}

/// Match a `[...]` character class starting at `pattern[pi] == '['`. Supports
/// ranges (`a-z`) and a leading `!`/`^` negation. An unterminated class is
/// treated as a literal `[`.
fn glob_match_class(pattern: &[char], pi: usize, ch: char) -> (bool, usize) {
    let mut i = pi + 1;
    let mut negate = false;
    if matches!(pattern.get(i), Some('!' | '^')) {
        negate = true;
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < pattern.len() {
        let c = pattern[i];
        if c == ']' && !first {
            return (matched ^ negate, i + 1);
        }
        first = false;
        if i + 2 < pattern.len() && pattern[i + 1] == '-' && pattern[i + 2] != ']' {
            if pattern[i] <= ch && ch <= pattern[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if c == ch {
                matched = true;
            }
            i += 1;
        }
    }
    // Unterminated: treat the '[' literally.
    (ch == '[', pi + 1)
}

/// Longest match of `pattern` starting at `text[start]`; returns the end index
/// (exclusive) of the match, or `None`. Used by `${…/…/…}` substitution.
fn glob_match_at(pattern: &[char], text: &[char], start: usize) -> Option<usize> {
    for j in (start..=text.len()).rev() {
        if glob_match(pattern, &text[start..j]) {
            return Some(j);
        }
    }
    None
}

/// `${name#pat}` / `${name##pat}` / `${name%pat}` / `${name%%pat}`.
fn param_trim(value: &str, pattern: &[char], suffix: bool, longest: bool) -> String {
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
            if glob_match(pattern, &v[k..]) {
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
            if glob_match(pattern, &v[..k]) {
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
fn param_case(value: &str, pattern: &[char], upper: bool, all: bool) -> String {
    // An empty pattern matches every character (bash: `^^`/`,,` with no
    // pattern uppercases/lowercases the whole value).
    let matches_char = |ch: char| pattern.is_empty() || glob_match(pattern, &[ch]);
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
) -> String {
    let v: Vec<char> = value.chars().collect();
    match anchor {
        ReplaceAnchor::Start => {
            if let Some(end) = glob_match_at(pattern, &v, 0) {
                let mut s = replacement.to_string();
                s.extend(v[end..].iter());
                return s;
            }
            value.to_string()
        }
        ReplaceAnchor::End => {
            for i in 0..=v.len() {
                if glob_match(pattern, &v[i..]) {
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
                    && let Some(end) = glob_match_at(pattern, &v, i)
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
            | "echo"
            | "printf"
            | "export"
            | "declare"
            | "typeset"
            | "local"
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
            | "eval"
            | "source"
            | "."
            | "type"
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

/// Split a string on the default IFS (whitespace), dropping empty fields.
fn split_ifs(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
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
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            '%' => match chars.next() {
                Some('%') => out.push('%'),
                Some('s') => {
                    out.push_str(args.get(*arg_i).map_or("", String::as_str));
                    *arg_i += 1;
                }
                Some('d') => {
                    let n = args
                        .get(*arg_i)
                        .and_then(|s| s.trim().parse::<i64>().ok())
                        .unwrap_or(0);
                    out.push_str(&n.to_string());
                    *arg_i += 1;
                }
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            },
            other => out.push(other),
        }
    }
    out
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
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn here_string_read() {
        let (o, _) = run("read x <<< hello; echo got $x");
        assert_eq!(o, "got hello\n");
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
        let g = |p: &str, t: &str| glob_match(&p.chars().collect::<Vec<_>>(), &t.chars().collect::<Vec<_>>());
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
            match_glob_toks(&compile_glob(&field_lit(p)), &n.chars().collect::<Vec<_>>())
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
        let toks = compile_glob(&field);
        assert!(match_glob_toks(&toks, &['*']));
        assert!(!match_glob_toks(&toks, &['a']));
    }

    #[test]
    fn glob_filesystem_expansion() {
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
        let mut txt: Vec<String> = glob_expand_field(&field_lit(&format!("{uniq}/*.txt")))
            .iter()
            .map(|p| basename(p))
            .collect();
        txt.sort();
        assert_eq!(txt, vec!["a.txt".to_string(), "b.txt".to_string()]);

        // `*` honors the leading-dot rule (no `.hidden`).
        let all = glob_expand_field(&field_lit(&format!("{uniq}/*")));
        assert!(all.iter().all(|p| !p.ends_with(".hidden")));
        assert_eq!(all.len(), 3);

        // An explicit leading `.` matches hidden files.
        let dot = glob_expand_field(&field_lit(&format!("{uniq}/.*")));
        assert!(dot.iter().any(|p| p.ends_with(".hidden")));

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn glob_no_match_stays_literal() {
        // With no match and no `nullglob`, the pattern is left as the word.
        assert_eq!(run("echo osh_definitely_no_such_glob_*.zzz").0, "osh_definitely_no_such_glob_*.zzz\n");
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
}
