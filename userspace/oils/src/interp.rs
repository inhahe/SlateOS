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
//!   a subshell (bash semantics), so a stage's variable mutations do not leak to
//!   the parent — except that with `shopt -s lastpipe` the final stage runs in
//!   the current shell, so `a | read x` sets `x`. Downstream early-exit propagates upstream: when a
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
//!   consume successive lines; a `> file` stdout redirect drives fd 1 through
//!   the file *live* (a scoped `exec_stdout` override) so output ordering — and
//!   a same-file `2>&1`/`&>` interleave of stdout and stderr — is preserved.
//!   A compound command's *stderr* is also redirectable (`{ …; } 2> err`,
//!   `for … done 2>&1`) via a `stderr_stack` consulted by every fd-2 write.
//!   The one exception is `2>&1` into a *captured* stdout (command substitution
//!   `$( … 2>&1 )`), where stderr is folded into the capture after the body
//!   (not byte-interleaved).
//! - Background (`&`) runs a single external command asynchronously; compound
//!   background jobs run synchronously.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::process::{Child, ChildStdout, Command as PCommand, Stdio};
use std::sync::{Arc, Mutex};

use crate::arith::{self, VarLookup};
use crate::ast::{
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, BulkOp, CaseClause, CaseTerm,
    Command,
    CondBinOp,
    CondExpr,
    ForArithClause, ForClause, IfClause, LoopClause, ParamOp, Pipeline, Program, Redirect,
    RedirectOp,
    ReplaceAnchor, SelectClause, SimpleCommand, UnaryOp, Word, WordPart,
};
use crate::parser::{parse, parse_with_aliases};

/// The bash release level this shell emulates, exposed via `$BASH_VERSION`
/// (and parsed into `$BASH_VERSINFO`). Scripts branch on this to gate features;
/// we report a 5.2 compatibility level with `slateos` as the vendor field.
const BASH_VERSION: &str = "5.2.0(1)-release";

/// The complete `shopt` option inventory (bash 5.2), in bash's own listing
/// order (roughly, but not strictly, alphabetical — it mirrors bash's internal
/// table), each paired with its default state for a **non-interactive** shell
/// (`bash -c 'shopt'`). Only a subset of these options actually changes osh's
/// behavior (glob/match toggles, `expand_aliases`); the rest are stored so
/// scripts that toggle them don't error and so `shopt`/`shopt -p`/`$BASHOPTS`
/// report them faithfully. `expand_aliases` is listed `false` here but its live
/// default is interactivity-dependent (see [`Shell::shopt_default`]).
const SHOPT_TABLE: &[(&str, bool)] = &[
    ("autocd", false),
    ("assoc_expand_once", false),
    ("cdable_vars", false),
    ("cdspell", false),
    ("checkhash", false),
    ("checkjobs", false),
    ("checkwinsize", true),
    ("cmdhist", true),
    ("compat31", false),
    ("compat32", false),
    ("compat40", false),
    ("compat41", false),
    ("compat42", false),
    ("compat43", false),
    ("compat44", false),
    ("completion_strip_exe", false),
    ("complete_fullquote", true),
    ("direxpand", false),
    ("dirspell", false),
    ("dotglob", false),
    ("execfail", false),
    ("expand_aliases", false),
    ("extdebug", false),
    ("extglob", false),
    ("extquote", true),
    ("failglob", false),
    ("force_fignore", true),
    ("globasciiranges", true),
    ("globskipdots", true),
    ("globstar", false),
    ("gnu_errfmt", false),
    ("histappend", false),
    ("histreedit", false),
    ("histverify", false),
    ("hostcomplete", true),
    ("huponexit", false),
    ("inherit_errexit", false),
    ("interactive_comments", true),
    ("lastpipe", false),
    ("lithist", false),
    ("localvar_inherit", false),
    ("localvar_unset", false),
    ("login_shell", false),
    ("mailwarn", false),
    ("no_empty_cmd_completion", false),
    ("nocaseglob", false),
    ("nocasematch", false),
    ("noexpand_translation", false),
    ("nullglob", false),
    ("patsub_replacement", true),
    ("progcomp", true),
    ("progcomp_alias", false),
    ("promptvars", true),
    ("restricted_shell", false),
    ("shift_verbose", false),
    ("sourcepath", true),
    ("varredir_close", false),
    ("xpg_echo", false),
];

/// Whether `name` is a recognized `shopt` option.
fn shopt_is_known(name: &str) -> bool {
    SHOPT_TABLE.iter().any(|(n, _)| *n == name)
}

/// Format one `shopt` listing line. In re-inputtable mode (`shopt -p`) the line
/// is `shopt -s NAME` / `shopt -u NAME` (which can be fed back to the shell);
/// otherwise it is bash's `NAME<pad-to-15>\ton|off` status form.
fn shopt_line(name: &str, on: bool, reinput: bool) -> String {
    if reinput {
        format!("shopt -{} {name}\n", if on { 's' } else { 'u' })
    } else {
        format!("{name:<15}\t{}\n", if on { "on" } else { "off" })
    }
}

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
#[derive(Clone)]
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
    /// `2>&N` (N ≥ 3) — write to a user-space write descriptor opened by
    /// `exec N> file`. The shared `Arc<File>` is append-positioned so writes
    /// land at the descriptor's current offset (matching a builtin `>&N`).
    WriteFd(Arc<File>),
}

/// A saved snapshot of one variable's complete state (scalar, indexed array,
/// associative array, export flag), captured when `local` shadows the name
/// inside a function so it can be restored when the function returns.
struct VarSnapshot {
    scalar: Option<String>,
    indexed: Option<BTreeMap<usize, String>>,
    assoc: Option<Vec<(String, String)>>,
    exported: bool,
    // Attribute flags, so `local -i`/`-l`/`-u`/`-n` scope to the function call
    // and are restored on return (bash: attributes set on a local are local).
    integer: bool,
    lower: bool,
    upper: bool,
    capcase: bool,
    nameref: bool,
    readonly: bool,
    array_valued: bool,
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
    /// Set by `disown -h`: the job stays in the table but is marked so it
    /// would not receive SIGHUP when the shell exits. (We have no SIGHUP
    /// delivery yet, so this is advisory bookkeeping matching bash semantics.)
    no_hup: bool,
}

/// One function frame's trap-inheritance mask (see [`Shell::trap_suppress`]).
/// A `true` field means the correspondingly-named trap is currently inherited
/// but suppressed: it exists in `self.traps` (so `trap -p` still shows it and it
/// persists globally) but must not fire while this frame is the innermost
/// function. Cleared per-name when the body reassigns that trap.
#[derive(Clone, Copy, Default)]
struct TrapSuppress {
    debug: bool,
    ret: bool,
    err: bool,
}

/// Which command(s) a programmable-completion spec applies to (see
/// [`Shell::comp_specs`]). `Name` is an ordinary command name; the three
/// specials mirror bash's `complete -D` (default, applied when no other spec
/// matches), `-E` (empty command line) and `-I` (initial non-assignment word).
#[derive(Clone, PartialEq, Eq)]
enum CompKey {
    Name(String),
    Default,
    Empty,
    Initial,
}

/// One `complete`/`compopt` completion specification. osh's line-oriented REPL
/// has no interactive tab-completion, so these specs are stored, printed
/// (`complete -p`) and mutated (`compopt`) purely for script compatibility —
/// sourcing a `bash_completion` file registers hundreds of them and must not
/// error. The generator side (`-F`/`-C`/…) is never actually invoked. Fields
/// map 1:1 onto bash's compspec so `complete -p` reproduces the definition.
#[derive(Clone, Default)]
struct CompSpec {
    /// `-o` option names present, e.g. `nospace`, `dirnames` (see `COMP_O_ORDER`).
    o_opts: Vec<String>,
    /// `-A`/shortcut action names present, e.g. `function`, `alias` (see `COMP_ACTIONS`).
    actions: Vec<String>,
    globpat: Option<String>,   // -G
    wordlist: Option<String>,  // -W
    prefix: Option<String>,    // -P
    suffix: Option<String>,    // -S
    filterpat: Option<String>, // -X
    command: Option<String>,   // -C
    function: Option<String>,  // -F
}

/// Canonical print order of `complete -o` option names (bash `pcomplete.c`).
const COMP_O_ORDER: &[&str] =
    &["bashdefault", "default", "dirnames", "filenames", "noquote", "nosort", "nospace", "plusdirs"];

/// Completion actions and their single-letter shortcut (`'\0'` = `-A name` only).
/// Order matches bash's `compacts[]`; `complete -p` prints shortcut actions
/// first (in this order), then the `-A`-only actions (also in this order).
const COMP_ACTIONS: &[(&str, char)] = &[
    ("alias", 'a'),
    ("arrayvar", '\0'),
    ("binding", '\0'),
    ("builtin", 'b'),
    ("command", 'c'),
    ("directory", 'd'),
    ("disabled", '\0'),
    ("enabled", '\0'),
    ("export", 'e'),
    ("file", 'f'),
    ("function", '\0'),
    ("group", 'g'),
    ("helptopic", '\0'),
    ("hostname", '\0'),
    ("job", 'j'),
    ("keyword", 'k'),
    ("running", '\0'),
    ("service", 's'),
    ("setopt", '\0'),
    ("shopt", '\0'),
    ("signal", '\0'),
    ("stopped", '\0'),
    ("user", 'u'),
    ("variable", 'v'),
];

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
    /// True once the real process environment has been imported into `vars`
    /// (via [`Shell::import_environment`], called by the binary at startup).
    /// When set, `vars` is the authoritative variable namespace: reads no
    /// longer fall back to `std::env` (so `unset PATH` actually hides it) and
    /// child processes are spawned with a cleared env populated only from the
    /// exported shell variables. Tests construct the shell via `new()` without
    /// importing, so they keep the on-demand `std::env` fallback and inherited
    /// child environment — staying deterministic and host-independent.
    env_imported: bool,
    funcs: HashMap<String, Program>,
    positional: Vec<String>,
    name: String,
    last_status: i32,
    /// Monotonic count of command substitutions performed. Used to detect
    /// whether a pure assignment's value contained a `$(...)`/backtick — bash
    /// sets the assignment's exit status from the last command substitution, or
    /// 0 when there was none (so `x=$?` still reads the prior status, but the
    /// assignment itself resets `$?` to 0).
    comsub_count: u64,
    /// `$BASH_SUBSHELL` — the subshell nesting depth. 0 at the top level; each
    /// `( … )` group, command substitution, pipeline stage, or other subshell
    /// increments it for its clone (bash).
    subshell_depth: u32,
    last_bg_pid: Option<u32>,
    /// `set -o pipefail`: a pipeline's status is the rightmost non-zero stage.
    pipefail: bool,
    /// Set when a write to a pipeline stage's downstream pipe fails with
    /// `BrokenPipe` (the reader closed early). The statement loops check it and
    /// unwind the stage — the in-process analogue of a producer taking `SIGPIPE`.
    /// Only ever set on a per-stage subshell clone, never the top-level shell.
    pipe_broken: bool,
    pid: u32,
    /// 1-based source line of the item currently executing, backing `$LINENO`.
    /// Updated by [`Shell::exec_program`] before each item runs.
    current_line: u32,
    /// Active stderr (fd 2) redirections, innermost last. Empty = real stderr.
    /// Pushed/popped by [`Shell::exec_redirected`] around a compound command's
    /// body so its stderr redirect (`{ …; } 2> err`) covers every command in
    /// the group. Consulted by [`Shell::emit_stderr`] (diagnostics/`>&2`) and
    /// [`Shell::run_external`] (child fd 2). Reset to empty in subshell clones.
    stderr_stack: Vec<StderrTarget>,
    /// Persistent stdout target set by a redirection-only `exec > file` /
    /// `exec >> file`: an open [`File`] handle (the file is opened once, so all
    /// subsequent writes share one OS offset — bash dups the fd, it does not
    /// reopen). `None` = the shell's real stdout. Inherited by subshell clones
    /// (a subshell shares the same `Arc<File>`, matching a real fd inheritance).
    /// A restore `exec 1>&N` points this at fd N's `open_write_fds` handle.
    /// Consulted by every ambient fd-1 write ([`Shell::write_bytes`]
    /// `Out::Inherit`, external children).
    exec_stdout: Option<std::sync::Arc<std::fs::File>>,
    /// Persistent stderr target set by a redirection-only `exec 2> file` /
    /// `exec 2>> file` (or mirrored from `exec_stdout` by `exec … 2>&1`, which
    /// shares the same `Arc<File>`). `None` = the shell's real stderr. A restore
    /// `exec 2>&N` points this at fd N's handle. Consulted by
    /// [`Shell::emit_stderr`] and external children as the base fd-2 sink
    /// beneath any `stderr_stack` entry.
    exec_stderr: Option<std::sync::Arc<std::fs::File>>,
    /// Persistent stdin source set by a redirection-only `exec < file` (or an
    /// `exec << EOF` here-doc): the file's bytes are read once into a
    /// position-tracking cursor so successive ambient `read` calls (and an
    /// external command inheriting fd 0) consume successive input. `None` = the
    /// shell's real stdin. Consulted wherever [`StdinSrc::Inherit`] is the base
    /// input ([`Shell::read_line`], [`Shell::read_record_input`],
    /// [`Shell::read_all_bytes`], and [`Shell::run_external`]). A subshell clone
    /// inherits a snapshot of the *remaining* bytes (independent offset — reads
    /// in the subshell do not advance the parent's cursor; a minor deviation
    /// from bash's shared-fd semantics, acceptable because our subshells already
    /// copy their stdin).
    exec_stdin: Option<RefCell<io::Cursor<Vec<u8>>>>,
    /// User-space table of non-standard input descriptors (fd ≥ 3) opened by a
    /// redirection-only `exec 3< file` / `exec 3<&-`. Each entry is the file's
    /// bytes in a position-tracking cursor, so `read -u 3` consumes successive
    /// records. Persistent across commands (like bash's `exec`-installed fds),
    /// but only consulted by `read -u N`; general per-command redirects to fd ≥ 3
    /// and *write* descriptors are not yet modelled. A subshell clone inherits a
    /// snapshot of each fd's *remaining* bytes with an independent offset (same
    /// approximation as [`Shell::exec_stdin`]).
    open_fds: std::collections::HashMap<i32, RefCell<io::Cursor<Vec<u8>>>>,
    /// User-space table of non-standard *write* descriptors (fd ≥ 3) opened by a
    /// redirection-only `exec 3> file` / `exec 3>> file`. Each entry is a shared
    /// [`std::fs::File`] handle; `echo … >&3` (builtins) and `cmd >&3`
    /// (externals) route their stdout to it via `RedirPlan::stdout_to_fd`, and
    /// `exec 3>&-` removes it. Persistent across commands. A subshell clone
    /// shares the same `Arc<File>` (bash: a subshell inherits the fd, so writes
    /// share one OS offset). `exec 3>&1` / `exec 3>&2` snapshot fd 1 / fd 2's
    /// current sink (an `exec`-redirected file, or a dup of the real terminal)
    /// into an entry here, matching bash's dup-at-exec-time semantics.
    open_write_fds: std::collections::HashMap<i32, std::sync::Arc<std::fs::File>>,
    /// Live *read* endpoints of running `coproc`s (fd ≥ 10 → the coproc's
    /// stdout). Unlike [`Shell::open_fds`] (which replays a byte snapshot), these
    /// are genuine OS pipe streams behind a persistent [`io::BufReader`] so
    /// successive `read <&N` / `read -u N` on `NAME[0]` consume successive lines
    /// as the coproc produces them (a chunk-buffering reader must persist across
    /// commands, or bytes read past a delimiter would be lost). A subshell clone
    /// `try_clone`s each handle (bash: a subshell inherits the coproc fd — one
    /// shared OS pipe), unlike the byte-snapshot used for `open_fds`. The
    /// *write* endpoint (`NAME[1]`) is stored in [`Shell::open_write_fds`]
    /// instead, so `>&"${NAME[1]}"` needs no write-path changes.
    coproc_read_fds: std::collections::HashMap<i32, RefCell<io::BufReader<std::fs::File>>>,
    /// Join handles for the background threads running each `coproc` body. Kept
    /// so the threads are not orphaned bookkeeping-wise; they finish when the
    /// coproc's stdin reaches EOF (its `NAME[1]` write end drops) and its body
    /// returns. A subshell clone starts with none (it cannot join the parent's
    /// threads).
    coproc_jobs: Vec<std::thread::JoinHandle<()>>,
    /// Temp files backing *input* process substitutions `<(cmd)` created while
    /// expanding the current command's words. Each holds `cmd`'s captured output;
    /// the enclosing command reads it, then it is deleted once the command
    /// finishes (drained by the `exec_simple` wrapper from the mark it recorded).
    procsub_in_temps: Vec<String>,
    /// Deferred *output* process substitutions `>(cmd)`: `(temp_path, body)`. The
    /// enclosing command writes to `temp_path`; after it finishes, `body` runs
    /// with the file's contents as its stdin, then the temp file is deleted.
    procsub_out_jobs: Vec<(String, Program)>,
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
    /// `set -f` (noglob): disable pathname (glob) expansion — patterns stay
    /// literal. Wired into the glob-expansion entry point.
    noglob: bool,
    /// `set -a` (allexport): automatically mark every subsequently-assigned
    /// variable for export. Consulted in `apply_assignment`.
    allexport: bool,
    /// `set -C` (noclobber): a plain `>` refuses to truncate an existing regular
    /// file; `>|` overrides. Consulted in `resolve_redirects`.
    noclobber: bool,
    /// `set -n` (noexec): read and parse commands but do not execute them — the
    /// syntax-check mode used by `bash -n script`. Once enabled it latches: the
    /// `set -n` that turns it on still runs (noexec is off when it is reached),
    /// but every command afterwards is skipped, so a later `set +n` can never
    /// re-enable execution (matching bash). Consulted at the top of
    /// [`Shell::exec_pipeline`].
    noexec: bool,
    /// `set -T` (functrace / `set -o functrace`): make the `DEBUG` and `RETURN`
    /// traps be inherited by shell functions, command substitutions, and
    /// subshells. When off (bash default), a called function does NOT see the
    /// caller's DEBUG/RETURN traps unless the function individually carries the
    /// trace attribute (`declare -ft name`, tracked in [`Shell::fn_trace_attr`]).
    /// Consulted on function entry in [`Shell::call_function`].
    functrace: bool,
    /// `set -E` (errtrace / `set -o errtrace`): make the `ERR` trap be inherited
    /// by shell functions, command substitutions, and subshells. When off (bash
    /// default), a failing command inside a called function does NOT fire the
    /// caller's ERR trap (only the function call itself failing does, at the
    /// caller level). Unlike DEBUG/RETURN this is not affected by the function
    /// trace attribute. Consulted on function entry in [`Shell::call_function`].
    errtrace: bool,
    /// The shell was invoked as `osh -c COMMAND`. Bash reports a `c` flag in
    /// `$-` for `-c` invocations (always last, after the `set`-toggled options);
    /// consulted only by `option_flags`. Set once at startup by the binary.
    command_mode: bool,
    /// The shell is executing a **script file** (`osh SCRIPT`), as opposed to
    /// `-c` or the interactive REPL. Bash includes a bottom `main` pseudo-frame
    /// in `FUNCNAME`/`BASH_SOURCE`/`BASH_LINENO` only for script (and sourced)
    /// execution; `-c` and interactive shells omit it. Set once at startup by
    /// the binary. See `refresh_funcname`.
    script_mode: bool,
    /// Nesting depth of errexit-exempt contexts (if/while/until conditions and
    /// negated commands). While `> 0`, a failing command does not trigger
    /// errexit. Incremented around condition evaluation; reset in subshells.
    errexit_suppress: u32,
    /// Set by expansion when a fatal word-expansion error occurs (a `nounset`
    /// unset-variable reference, a `${var:?word}` on an unset/null parameter, or
    /// a bad indirect/subscript expansion). Carries the **main-shell** exit
    /// status the shell aborts with: bash uses **127** for nounset and
    /// `${var:?}` errors, but **1** for bad indirect expansions and bad array
    /// subscripts. That carried code is only honoured in the *main shell
    /// environment* (`subshell_depth == 0`, which includes brace groups and
    /// function bodies); inside any subshell / command substitution / pipeline
    /// stage bash yields **1** for all of these — see [`Shell::fatal_abort_status`].
    /// The simple-command driver checks the flag and aborts (`Flow::Exit`) after
    /// expanding its words.
    unbound_error: Option<i32>,
    /// Set when an arithmetic evaluation error occurs while expanding a
    /// `$(( … ))` substitution in a word or assignment value. The simple-command
    /// driver checks and skips the command (status 1, `Flow::Next`) after
    /// expansion, matching bash (which discards the command on an arithmetic
    /// error rather than running it with a bogus value).
    arith_error: bool,
    /// The name of the builtin whose execution should prefix an arithmetic
    /// diagnostic — bash's `this_command_name`. bash reports arithmetic errors
    /// raised while a builtin runs as `<name>: line N: <builtin>: <expr>: …`
    /// (`let`, `((`, `declare`, `typeset`, `local`); errors from plain
    /// assignments and word/`$(( … ))` expansion carry no builtin tag. Set for
    /// the duration of the relevant builtin, `None` otherwise.
    arith_cmd: Option<&'static str>,
    /// Set (to the unmatched pattern) when `shopt -s failglob` is on and a glob
    /// in a command word matches nothing. The simple-command driver reports
    /// `no match: PATTERN` and aborts the command list (`Flow::Exit(1)`) after
    /// expansion, matching a non-interactive bash under `failglob`.
    glob_error: Option<String>,
    /// Stack of function-local variable scopes. Each frame records the variables
    /// shadowed by `local` in that function call and their prior state, restored
    /// on return. Non-empty exactly while executing a function body.
    local_frames: Vec<Vec<(String, VarSnapshot)>>,
    /// Names of the functions currently executing, innermost last. Drives the
    /// `FUNCNAME` array (bash: `FUNCNAME[0]` is the current function, then its
    /// callers, then `main`). Non-empty exactly while inside a function body;
    /// materialised into `arrays["FUNCNAME"]` by `refresh_funcname`.
    fn_stack: Vec<String>,
    /// The source line of each call site in `fn_stack`, innermost last:
    /// `call_line_stack[k]` is the line where `fn_stack[k]` was invoked. Drives
    /// `BASH_LINENO` and the `caller` builtin. Kept in lockstep with `fn_stack`.
    call_line_stack: Vec<u32>,
    /// Nesting depth of `source`/`.` invocations currently executing. `return`
    /// is only valid inside a function *or* a sourced script; this lets the
    /// `return` builtin distinguish a legal source-level return from an illegal
    /// top-level one (which bash reports as an error, status 2, without
    /// unwinding). Reset to 0 in subshell clones.
    source_depth: u32,
    /// Nesting depth of enclosing loops (`for`/`while`/`until`/`select`) whose
    /// body is currently executing. `break`/`continue` are only meaningful
    /// inside a loop: bash reports "only meaningful in a …" to stderr, returns
    /// status 0, and keeps executing when this is 0. Reset to 0 on function
    /// entry (a `break` inside a function must not escape to a caller's loop,
    /// matching bash) and in subshell clones.
    loop_depth: u32,
    /// Names marked `readonly` (or `declare -r`). Assigning to or unsetting a
    /// readonly variable is an error; the shell reports it and leaves the value
    /// unchanged. Copied into subshell clones so the attribute is inherited.
    readonly: HashSet<String>,
    /// `shopt` option toggles (e.g. `nullglob`, `dotglob`, `nocaseglob`). Only
    /// options present with `true` are enabled; absent/`false` = default off.
    /// Inherited by subshell clones.
    shopt: HashMap<String, bool>,
    /// Programmable-completion specifications registered by `complete` (and
    /// mutated by `compopt`), keyed by target command. Stored in insertion
    /// order; `complete -p` reproduces each definition verbatim. osh has no
    /// interactive completion, so these are never invoked — they exist so that
    /// scripts sourcing bash completion files run without error. Not inherited
    /// by subshell clones (bash does not propagate compspecs to subshells).
    comp_specs: Vec<(CompKey, CompSpec)>,
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
    /// Names with the capitalize attribute (`declare -c`). Assigned values have
    /// their first character uppercased and the remainder lowercased before
    /// storing (bash's `att_capcase`). Mutually exclusive with `-l`/`-u`.
    /// Inherited by subshell clones.
    capcase_attr: HashSet<String>,
    /// Names with the nameref attribute (`declare -n`/`local -n`). The variable's
    /// *value* is the name of another variable; reads and writes of the nameref
    /// are transparently redirected to that target (following chains, with a
    /// depth guard against cycles). Inherited by subshell clones.
    nameref_attr: HashSet<String>,
    /// Function names carrying the trace attribute (`declare -ft name`). Such a
    /// function inherits the caller's `DEBUG` and `RETURN` traps even when
    /// `functrace` (`set -T`) is off. Inherited by subshell clones.
    fn_trace_attr: HashSet<String>,
    /// Per-function-frame trap-inheritance suppression, pushed on every function
    /// entry and popped on return (kept in lockstep with `fn_stack`). Each frame
    /// names the traps that the caller installed but that this frame does NOT
    /// inherit, so they must not fire while this frame is the innermost function.
    /// DEBUG/RETURN are suppressed when neither `functrace` nor the function's
    /// trace attribute is set; ERR is suppressed when `errtrace` is off. A trap
    /// stops being suppressed for the current frame as soon as the body reassigns
    /// it via the `trap` builtin — bash: a body-installed trap fires normally and
    /// persists globally, whereas an inherited one is only masked, never removed
    /// from `self.traps`. Consulted at each trap fire site via
    /// [`Shell::trap_suppressed`]; cleared per-name by the `trap` builtin.
    trap_suppress: Vec<TrapSuppress>,
    /// Array names (indexed or associative) that have been *assigned a value*
    /// at least once — via a literal (`a=(…)`, including the empty `a=()`), an
    /// element assignment (`a[i]=v`), an append, or a value-carrying `declare`.
    /// This distinguishes an assigned-but-empty array (bash `declare -p` shows
    /// `declare -a a=()`) from one merely *declared* with `declare -a a` and
    /// never given a value (shown as the bare `declare -a a`). Once set it
    /// stays set until the whole variable is unset — emptying every element
    /// (`unset 'a[0]'`) does not clear it, matching bash's has-a-value flag.
    /// Inherited by subshell clones and scoped by `local`.
    array_valued: HashSet<String>,
    /// The directory stack below the current directory, managed by
    /// `pushd`/`popd`/`dirs`. Element 0 is the directory `popd` would return to;
    /// the *current* directory (the process cwd) is conceptually the top of the
    /// stack and is not stored here. Cloned into subshells.
    dir_stack: Vec<String>,
    /// Builtins disabled via `enable -n NAME`. A name present here is treated as
    /// *not* a builtin during command resolution, so a same-named external is
    /// run instead. Cloned into subshells (bash inherits the enable state).
    disabled_builtins: HashSet<String>,
    /// Command aliases defined via the `alias` builtin (name → replacement
    /// text). Expanded over the token stream before parsing (see
    /// `parse_with_aliases`). `BTreeMap` keeps `alias` listings sorted. Cloned
    /// into subshells (bash inherits aliases).
    aliases: BTreeMap<String, String>,
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
    /// The file-creation mask (`umask`). The low 9 bits (owner/group/other rwx)
    /// are the bits *cleared* from a newly created file's permissions. Consulted
    /// when a redirection creates a file (applied via the file mode on unix-family
    /// targets; the Windows host has no mode concept — see known-issues TD-OILS15).
    /// Inherited by subshell clones and children (the process umask).
    umask_val: u32,
    /// Remembered full pathnames for commands looked up via `$PATH`, keyed by the
    /// command name (`hash` builtin). Value is `(resolved path, hit count)`. bash
    /// caches every PATH search here and consults it before re-searching; the
    /// table is inherited by subshell clones. See `resolve_external`.
    cmd_hash: std::collections::HashMap<String, (std::path::PathBuf, u64)>,
    /// Per-shell resource limits for the `ulimit` builtin, keyed by the bash
    /// option letter (`'n'`, `'s'`, …). The value is a `(soft, hard)` pair in
    /// the *display units* bash uses for that resource (`None` == unlimited).
    ///
    /// This is a shell-level model: osh tracks and reports limits and honours
    /// get/set/`-H`/`-S` semantics, but does not yet query or enforce the real
    /// kernel `getrlimit(2)`/`setrlimit(2)` limits on slateos (host builds have
    /// no rlimit concept at all). See known-issues `TD-OILS-ULIMIT`. Inherited
    /// by subshell clones, matching bash's per-process limit inheritance.
    rlimits: std::collections::HashMap<char, (Option<u64>, Option<u64>)>,
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
        let mut sh = Shell {
            vars: HashMap::new(),
            arrays: HashMap::new(),
            assoc: HashMap::new(),
            exported: HashSet::new(),
            env_imported: false,
            funcs: HashMap::new(),
            positional: Vec::new(),
            name: "osh".to_string(),
            last_status: 0,
            comsub_count: 0,
            subshell_depth: 0,
            last_bg_pid: None,
            pipefail: false,
            pipe_broken: false,
            pid: std::process::id(),
            current_line: 1,
            stderr_stack: Vec::new(),
            exec_stdout: None,
            exec_stderr: None,
            exec_stdin: None,
            open_fds: std::collections::HashMap::new(),
            open_write_fds: std::collections::HashMap::new(),
            coproc_read_fds: std::collections::HashMap::new(),
            coproc_jobs: Vec::new(),
            procsub_in_temps: Vec::new(),
            procsub_out_jobs: Vec::new(),
            getopts_col: 0,
            getopts_optind: 1,
            seconds_anchor: std::time::Instant::now(),
            seconds_base: 0,
            // Seed `$RANDOM` from the wall clock so successive runs differ.
            rng: std::cell::Cell::new(initial_rng_seed()),
            errexit: false,
            nounset: false,
            xtrace: false,
            noglob: false,
            allexport: false,
            noclobber: false,
            noexec: false,
            functrace: false,
            errtrace: false,
            command_mode: false,
            script_mode: false,
            errexit_suppress: 0,
            unbound_error: None,
            arith_error: false,
            arith_cmd: None,
            glob_error: None,
            local_frames: Vec::new(),
            fn_stack: Vec::new(),
            call_line_stack: Vec::new(),
            source_depth: 0,
            loop_depth: 0,
            readonly: HashSet::new(),
            shopt: HashMap::new(),
            comp_specs: Vec::new(),
            integer_attr: HashSet::new(),
            array_valued: HashSet::new(),
            lower_attr: HashSet::new(),
            upper_attr: HashSet::new(),
            capcase_attr: HashSet::new(),
            nameref_attr: HashSet::new(),
            fn_trace_attr: HashSet::new(),
            trap_suppress: Vec::new(),
            dir_stack: Vec::new(),
            disabled_builtins: HashSet::new(),
            aliases: BTreeMap::new(),
            traps: HashMap::new(),
            exit_trap_done: false,
            in_trap: false,
            jobs: Vec::new(),
            umask_val: 0o022,
            cmd_hash: std::collections::HashMap::new(),
            rlimits: default_rlimits(),
        };
        sh.seed_shell_vars();
        sh
    }

    /// Seed the shell-internal variables bash defines unconditionally (not from
    /// the environment): the `BASH_VERSION` string and its parsed
    /// `BASH_VERSINFO` array. We report a bash 5.2 compatibility level (the
    /// language level this shell targets) with `slateos` as the vendor field.
    fn seed_shell_vars(&mut self) {
        self.vars
            .insert("BASH_VERSION".to_string(), BASH_VERSION.to_string());
        // Platform identity strings bash always defines at startup. We report
        // SlateOS's own values (not the host build's), so scripts that branch on
        // `$OSTYPE`/`$MACHTYPE` see the target platform. bash leaves these as
        // ordinary (non-exported) shell variables and lets an inherited
        // environment override them, which our seed-before-import order matches.
        for (name, val) in [
            ("HOSTTYPE", "x86_64"),
            ("OSTYPE", "slateos"),
            ("MACHTYPE", "x86_64-slateos"),
        ] {
            self.vars.insert(name.to_string(), val.to_string());
        }
        // BASH_VERSINFO: (major, minor, patch, build, status, machtype). bash
        // marks it readonly; matching that guards scripts that probe the level.
        let versinfo = [
            (0usize, "5"),
            (1, "2"),
            (2, "0"),
            (3, "1"),
            (4, "release"),
            (5, "x86_64-slateos"),
        ];
        let mut arr = BTreeMap::new();
        for (i, v) in versinfo {
            arr.insert(i, v.to_string());
        }
        self.arrays.insert("BASH_VERSINFO".to_string(), arr);
        // bash marks BASH_VERSINFO readonly; match that so scripts probing the
        // level can't clobber it. The `readonly -p` / `declare -p` listing now
        // renders readonly arrays correctly (TD-OILS-RO-ARRAY fixed), so this no
        // longer surfaces malformed output.
        self.readonly.insert("BASH_VERSINFO".to_string());
        // SHELLOPTS: bash exposes the enabled `set -o` options as a readonly,
        // colon-separated list. Seed it from the current (default) option state
        // and mark it readonly so scripts can't clobber it; `refresh_shellopts`
        // keeps it current as options are toggled.
        self.refresh_shellopts();
        self.readonly.insert("SHELLOPTS".to_string());
        // BASH: the absolute path to the running shell binary. bash sets this at
        // startup and leaves it reassignable (NOT readonly). We use the host
        // executable path (lossy), falling back to the bare name if it can't be
        // determined — matching bash, which still defines BASH even when argv[0]
        // is a bare name.
        let bash_path = std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "osh".to_string());
        self.vars.insert("BASH".to_string(), bash_path);
        // BASHOPTS: bash exposes the enabled `shopt` options as a readonly,
        // colon-separated, alphabetically-sorted list. Seed it from the current
        // (default) shopt state; `refresh_bashopts` keeps it current as options
        // are toggled.
        self.refresh_bashopts();
        self.readonly.insert("BASHOPTS".to_string());
    }

    /// Set `$0`, the shell/script name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Set the positional parameters (`$1`, `$2`, …).
    pub fn set_positional(&mut self, args: Vec<String>) {
        self.positional = args;
    }

    /// Mark the shell as invoked via `-c COMMAND` so `$-` reports the `c` flag
    /// (bash behaviour). Called once by the binary before running the command.
    pub fn set_command_mode(&mut self) {
        self.command_mode = true;
        // `expand_aliases` defaults off non-interactively, so BASHOPTS (seeded
        // in `Shell::new` before the mode was known) must be recomputed.
        self.refresh_bashopts();
    }

    /// Enable noexec (`set -n`) from the command line (`osh -n …`): parse the
    /// input for syntax errors but execute nothing. Called once by the binary
    /// before running the source, matching `bash -n`.
    pub fn set_noexec(&mut self) {
        self.noexec = true;
        self.refresh_shellopts();
    }

    /// Mark the shell as executing a script *file* (`osh SCRIPT`). Enables the
    /// bottom `main` pseudo-frame in the call-stack arrays (see
    /// `refresh_funcname`). Called once by the binary before running the script.
    ///
    /// Materialises the base frame immediately so that even at the script's top
    /// level (before any function call) `${BASH_SOURCE[0]}` is the script path
    /// and `${BASH_LINENO[0]}` is 0 — matching bash, which populates these from
    /// the moment the script starts. `refresh_funcname` only runs on function
    /// enter/exit, so without this the arrays would be empty until the first
    /// call.
    pub fn set_script_mode(&mut self) {
        self.script_mode = true;
        self.refresh_funcname();
        // `expand_aliases` defaults off non-interactively, so BASHOPTS (seeded
        // in `Shell::new` before the mode was known) must be recomputed.
        self.refresh_bashopts();
    }

    /// Set a shell variable.
    pub fn set_var(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(name.into(), value.into());
    }

    /// Import the real process environment into the shell variable namespace,
    /// marking every imported name exported (bash: environment variables *are*
    /// shell variables). Called once by the binary at startup. After this, the
    /// shell owns its environment: variable reads come from `vars` (no
    /// `std::env` fallback, so `unset PATH` truly hides it), prefix matching
    /// (`${!P*}`) and `set`/`export -p` listings see the inherited variables,
    /// and child processes are spawned from the exported set with a cleared
    /// base env. A name already defined in `vars` (e.g. set before import) is
    /// left untouched.
    pub fn import_environment(&mut self) {
        for (k, v) in std::env::vars() {
            self.vars.entry(k.clone()).or_insert(v);
            self.exported.insert(k);
        }
        // bash increments $SHLVL for each nested shell invocation: an unset or
        // non-numeric value becomes 1, otherwise the inherited level + 1. The
        // result is exported so child shells continue the chain.
        let next_lvl = self
            .vars
            .get("SHLVL")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        self.vars.insert("SHLVL".to_string(), next_lvl.to_string());
        self.exported.insert("SHLVL".to_string());
        self.env_imported = true;
    }

    /// The exit status of the most recently completed command.
    #[must_use]
    pub fn last_status(&self) -> i32 {
        self.last_status
    }

    /// Parse and execute shell source, returning the final exit status.
    /// The effective default for a `shopt` option the user hasn't explicitly
    /// toggled, taken from [`SHOPT_TABLE`] (bash's non-interactive defaults).
    /// `expand_aliases` is the one interactivity-dependent case: on in an
    /// interactive shell, off under `-c`/script — matching bash's
    /// alias-expansion rule.
    fn shopt_default(&self, name: &str) -> bool {
        match name {
            "expand_aliases" => !self.command_mode && !self.script_mode,
            _ => SHOPT_TABLE.iter().find(|(n, _)| *n == name).map(|(_, d)| *d).unwrap_or(false),
        }
    }

    /// Whether command-word aliases should be expanded when parsing new input.
    /// Gated on `expand_aliases` (see [`Self::shopt_default`]) so a
    /// non-interactive shell does not expand aliases unless the script opts in
    /// with `shopt -s expand_aliases`, matching bash.
    fn aliases_enabled(&self) -> bool {
        self.shopt
            .get("expand_aliases")
            .copied()
            .unwrap_or_else(|| self.shopt_default("expand_aliases"))
    }

    pub fn run_source(&mut self, src: &str) -> i32 {
        let mut out = Out::Inherit;
        self.run_source_out(src, &mut out)
    }

    /// Parse and execute `src`, sending its standard output to the caller's
    /// `out` sink. This lets internally-generated commands (e.g. the `mapfile
    /// -C` callback) participate in an active capture — a command substitution
    /// or brace-group redirect — rather than escaping to the real fd 1.
    fn run_source_out(&mut self, src: &str, out: &mut Out) -> i32 {
        // Expand command-word aliases only when `expand_aliases` is in effect;
        // otherwise parse the raw tokens so a non-interactive shell leaves alias
        // names untouched (bash parity).
        let parsed = if self.aliases_enabled() {
            parse_with_aliases(src, &self.aliases)
        } else {
            parse(src)
        };
        let prog = match parsed {
            Ok(p) => p,
            Err(e) => {
                self.errln(&format_parse_error(&e, &self.err_prefix()));
                self.last_status = 2;
                return 2;
            }
        };
        match self.exec_program(&prog, out, &StdinSrc::Inherit) {
            Flow::Exit(code) => {
                self.last_status = code;
                code
            }
            _ => self.last_status,
        }
    }

    /// Resolve the exit status of a fatal word-expansion abort (nounset,
    /// `${var:?}`, bad indirect/subscript). The carried `code` is the status
    /// bash uses in the *main shell environment* (127 for nounset/`:?`, 1 for
    /// indirect/subscript). Inside any subshell, command substitution, or
    /// pipeline stage (`subshell_depth > 0`) bash instead aborts that
    /// environment with status **1** for all of these, without touching the
    /// parent shell — so only honour the higher code at depth 0. Brace groups
    /// and function bodies run at the caller's depth, so they correctly inherit
    /// the main-shell status.
    fn fatal_abort_status(&self, code: i32) -> i32 {
        if self.subshell_depth == 0 {
            code
        } else {
            1
        }
    }

    fn exec_program(&mut self, prog: &Program, out: &mut Out, stdin: &StdinSrc) -> Flow {
        for item in &prog.items {
            self.current_line = item.line;
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
        // Snapshot whether the ERR trap is *armed for this frame* at the moment
        // this list starts. bash fires ERR for a failing command based on the
        // trap state when the command began — not afterwards. This matters when
        // the command is a function call that installs its own ERR trap and then
        // fails: at the caller frame the trap did not yet exist when the call
        // started, so the call's own non-zero return must not fire it (whereas a
        // *later* caller-frame command, run after the trap now exists globally,
        // does fire). Armed = an ERR trap exists and is not inheritance-masked.
        let err_armed_at_start = self.traps.contains_key("ERR") && !self.trap_suppressed("ERR");
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
        if failed_unexempt && err_armed_at_start {
            // The ERR trap fires regardless of whether `set -e` is on, but only
            // when it was armed for this frame at the command's start (see the
            // `err_armed_at_start` snapshot above). Suppression inside an
            // untraced function that merely inherited it is already folded into
            // that snapshot: without `errtrace` (`set -E`) a failure inside the
            // function does not fire the caller's ERR trap.
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
        // noexec (`set -n` / the command-line `-n` flag): parse but do not run.
        // The `set -n` that enables it is reached while noexec is still off, so
        // it executes and latches skipping for every later pipeline — including
        // `&&`/`||` continuations and compound-command bodies. This is the sole
        // execution chokepoint every command flows through, so a single guard
        // here covers simple commands, pipelines, and compounds alike.
        if self.noexec {
            return Flow::Next;
        }
        // bash fires the DEBUG trap in the parent shell once for each pipeline
        // stage that is a simple command, left-to-right, before the pipeline
        // runs. Compound/group stages don't fire it, and each stage's own
        // subshell resets the trap, so without functrace only these parent-side
        // firings are observable. A single-command "pipeline" fires DEBUG via
        // the normal `exec_simple` path instead, so handle only `len > 1` here.
        if pipe.commands.len() > 1
            && !self.in_trap
            && self.traps.contains_key("DEBUG")
            && !self.trap_suppressed("DEBUG")
        {
            for cmd in &pipe.commands {
                if let Some(sc) = Self::stage_simple(cmd) {
                    self.vars
                        .insert("BASH_COMMAND".to_string(), crate::unparse::simple_src(sc));
                    self.fire_trap("DEBUG");
                }
            }
        }
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
                    self.errln(&format!("{}pipe: {e}", self.err_prefix()));
                    self.last_status = 1;
                    return (vec![1; n], Flow::Next);
                }
            }
        }
        writers.push(None); // last stage writes to `out`

        let mut statuses = vec![0i32; n];
        // `shopt -s lastpipe`: with job control off (always, in osh) bash runs
        // the LAST pipeline stage in the current shell rather than a subshell,
        // so its variable/`cd`/function mutations persist and its control flow
        // (`exit`/`return`/`break`) propagates. Captured out of the scope below.
        let lastpipe = self.shopt.get("lastpipe").copied().unwrap_or(false);
        let mut last_flow = Flow::Next;

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
                    // A pipeline stage runs in its own subshell: fire its EXIT
                    // trap (if it set one) before the stage's state is dropped.
                    sub.run_exit_trap_out(&mut o, &stdin);
                    // `o` drops here, closing the write end → EOF downstream.
                    sub.last_status
                });
                handles.push((i, handle));
            }

            // Last stage: run on this thread (writing to `out`).
            let last = n - 1;
            let reader = readers[last].take();
            let stdin = match reader {
                Some(r) => StdinSrc::Pipe(RefCell::new(io::BufReader::new(r))),
                None => StdinSrc::Inherit,
            };
            if lastpipe {
                // Run in the current shell (not a subshell): mutations persist
                // and control flow propagates. No EXIT trap firing here — this
                // is the running shell, whose EXIT trap fires only on true exit.
                last_flow = self.exec_command(&cmds[last], out, &stdin);
                statuses[last] = self.last_status;
            } else {
                let mut sub = self.clone_for_subshell();
                sub.exec_command(&cmds[last], out, &stdin);
                // The last stage is a subshell too: fire its own EXIT trap.
                sub.run_exit_trap_out(out, &stdin);
                statuses[last] = sub.last_status;
            }
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
        // affect only that stage's subshell and never escape (bash semantics) —
        // except the last stage under `lastpipe`, whose flow we propagate.
        (statuses, last_flow)
    }

    /// The underlying [`SimpleCommand`] of a pipeline stage, if the stage is a
    /// (possibly redirected) simple command. Used to fire the `DEBUG` trap in
    /// the parent shell for each simple-command stage of a pipeline, matching
    /// bash (which fires `DEBUG` once per pipeline element that is a simple
    /// command, before the pipeline runs; compound/group stages don't fire).
    fn stage_simple(cmd: &Command) -> Option<&SimpleCommand> {
        match cmd {
            Command::Simple(sc) => Some(sc),
            Command::Redirected { inner, .. } => Self::stage_simple(inner),
            _ => None,
        }
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
            // When the shell owns its environment (imported at startup), spawn
            // from a cleared base so an `unset`/non-exported variable does not
            // leak in via the parent process's inherited environment.
            if self.env_imported {
                pc.env_clear();
            }
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
                        self.errln(&format!("{}{program}: command not found", self.err_prefix()));
                    } else {
                        self.errln(&format!("{}{program}: {e}", self.err_prefix()));
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
            // `break`/`continue` are only meaningful inside a loop body, so we
            // track loop nesting around each loop executor. `loop_depth` gates
            // the `break`/`continue` builtins (0 → warn-and-continue like bash).
            Command::Loop(c) => self.in_loop(|s| s.exec_loop(c, out, stdin)),
            Command::For(c) => self.in_loop(|s| s.exec_for(c, out, stdin)),
            Command::ForArith(c) => self.in_loop(|s| s.exec_for_arith(c, out, stdin)),
            Command::Select(c) => self.in_loop(|s| s.exec_select(c, out, stdin)),
            Command::Function(f) => {
                self.funcs.insert(f.name.clone(), f.body.clone());
                self.last_status = 0;
                Flow::Next
            }
            Command::Case(c) => self.exec_case(c, out, stdin),
            Command::Cond(e) => self.exec_cond(e),
            Command::Arith(raw) => self.exec_arith(raw),
            Command::BraceGroup(p) => self.exec_program(p, out, stdin),
            Command::Coproc { name, body } => self.exec_coproc(name.as_deref(), body),
            Command::Redirected { inner, redirects } => {
                self.exec_redirected(inner, redirects, out, stdin)
            }
            Command::Subshell(p) => {
                // A subshell gets a clone of the state; mutations don't escape.
                let mut sub = self.clone_for_subshell();
                // A subshell inherits the fds applied to it, including an active
                // compound-command stderr redirect (`( … ) 2>&1`, `( … ) 2>file`).
                // `clone_for_subshell` resets `stderr_stack` (so pipeline-stage
                // clones on threads don't chase an outer group's stderr), so copy
                // it back here for this inline subshell — otherwise a `>&2` inside
                // `$( ( … ) 2>&1 )` would leak to the real stderr instead of the
                // command-substitution capture. This runs on the current thread,
                // so `Send` is not a concern; `StderrTarget` is all `Arc`-based.
                sub.stderr_stack.clone_from(&self.stderr_stack);
                let flow = sub.exec_program(p, out, stdin);
                // Fire the subshell's own EXIT trap (if it set one) before its
                // state is discarded — matching bash, which runs an EXIT trap for
                // every exiting shell environment, not only the top level.
                sub.run_exit_trap_out(out, stdin);
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

    /// Run `f` with the loop-nesting counter bumped, so `break`/`continue`
    /// executed anywhere inside `f` see a non-zero `loop_depth` and are treated
    /// as meaningful. The counter is always restored, including on early return.
    fn in_loop<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.loop_depth = self.loop_depth.saturating_add(1);
        let r = f(self);
        self.loop_depth = self.loop_depth.saturating_sub(1);
        r
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
        // `failglob`: an unmatched glob in the word list is a fatal expansion
        // error — abort the loop before running the body, as bash does.
        if let Some(pat) = self.glob_error.take() {
            self.emit_stderr(format!("{}no match: {pat}\n", self.err_prefix()).as_bytes());
            self.last_status = 1;
            return Flow::Exit(1);
        }
        // A `for` over an empty list runs no body and has exit status 0.
        // `set -x` prints the (source-form) loop header before *each* iteration,
        // matching bash. A `for name; do` with no explicit list traces as
        // `for name in "$@"`.
        let header = if self.xtrace {
            let words = match &c.words {
                Some(words) => words.iter().map(crate::unparse::word_src).collect::<Vec<_>>().join(" "),
                None => "\"$@\"".to_string(),
            };
            Some(format!("for {} in {words}", c.var))
        } else {
            None
        };
        let mut body_status = 0;
        for item in items {
            if let Some(h) = &header {
                let prefix = self.xtrace_prefix();
                self.emit_stderr(format!("{prefix}{h}\n").as_bytes());
            }
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
        // `set -x` prints the (source-form) `select` header once, before the
        // menu — bash does not re-emit it per iteration.
        if self.xtrace {
            let words = match &c.words {
                Some(words) => words.iter().map(crate::unparse::word_src).collect::<Vec<_>>().join(" "),
                None => "\"$@\"".to_string(),
            };
            self.xtrace_emit(&format!("select {} in {words}", c.var));
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
                Some((l, _)) => l,
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
            self.emit_stderr(format!("{}let: expression expected\n", self.err_prefix()).as_bytes());
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
    /// Evaluate an arithmetic string with a builtin/context tag set for the
    /// duration (bash's `this_command_name`) — used by the `(( … ))` command and
    /// the C-style `for (( … ))` sections, whose errors bash tags with `((`.
    fn eval_arith_cmd(&mut self, raw: &str, tag: &'static str) -> Option<i64> {
        let saved = self.arith_cmd;
        self.arith_cmd = Some(tag);
        let r = self.eval_arith_raw(raw);
        self.arith_cmd = saved;
        r
    }

    fn eval_arith_raw(&mut self, raw: &str) -> Option<i64> {
        // `(( … ))` commands, `let`, and arithmetic array subscripts report
        // their own errors and set their own status — a nested `$(( … ))` here
        // must not additionally trip the simple-command abort flag, so save and
        // restore it around the sub-expansion.
        let saved_arith_error = self.arith_error;
        let expanded = self.expand_arith_params(raw);
        self.arith_error = saved_arith_error;
        match arith::eval(&expanded, self) {
            Ok(v) => Some(v),
            Err(e) => {
                // Route through `emit_arith_error` (not `eprintln!`) so the
                // diagnostic honours an active `2>`/`2>&1` redirect on the
                // enclosing command — bash silences `let "3 x" 2>/dev/null`,
                // `declare -i k="3 x" 2>/dev/null`, `(( 3 x )) 2>/dev/null`, etc.
                self.emit_arith_error(&expanded, &e);
                None
            }
        }
    }

    /// Evaluate an *integer-assignment* initializer — the value bound to a
    /// variable carrying the integer attribute (`declare -i x=EXPR`, a plain
    /// `x=EXPR` when `x` is `-i`, or an element of an `-ia` array). Unlike
    /// `let` / `(( … ))` (which merely return a non-zero status on a bad
    /// expression), an arithmetic error in an integer *assignment* is **fatal**
    /// in bash: the diagnostic is printed and the shell aborts with status 1.
    /// `eval_arith_raw` deliberately suppresses `arith_error` (so nested
    /// `$(( … ))` in `let`/`((` don't trip the abort), so this wrapper re-sets
    /// it on failure. The command driver / `run_builtin` tail consumes the flag
    /// and turns it into a `Flow::Exit(1)`. Yields 0 as the placeholder value.
    fn eval_int_assign(&mut self, raw: &str) -> i64 {
        match self.eval_arith_raw(raw) {
            Some(n) => n,
            None => {
                self.arith_error = true;
                0
            }
        }
    }

    /// C-style `for (( init; cond; update )); do body; done`. `init` runs once;
    /// the loop runs while `cond` is non-zero (an empty `cond` is always true);
    /// `update` runs after each iteration (including after `continue`). An
    /// arithmetic error in any section aborts the loop with status 1.
    fn exec_for_arith(&mut self, c: &ForArithClause, out: &mut Out, stdin: &StdinSrc) -> Flow {
        // `set -x` traces each section as `(( expr ))`; bash substitutes an
        // always-true `1` for an *empty* section (init/cond/update) and still
        // prints it, so `for ((;;))` traces `(( 1 ))` for init and cond.
        let trace_section = |s: &mut Self, raw: &str| {
            if s.xtrace {
                let expr = if raw.is_empty() { "1" } else { raw };
                s.xtrace_emit(&format!("(( {expr} ))"));
            }
        };
        self.last_status = 0;
        trace_section(self, &c.init);
        if !c.init.is_empty() && self.eval_arith_cmd(&c.init, "((").is_none() {
            self.last_status = 1;
            return Flow::Next;
        }
        loop {
            trace_section(self, &c.cond);
            if !c.cond.is_empty() {
                match self.eval_arith_cmd(&c.cond, "((") {
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
            trace_section(self, &c.update);
            if !c.update.is_empty() && self.eval_arith_cmd(&c.update, "((").is_none() {
                self.last_status = 1;
                return Flow::Next;
            }
        }
        Flow::Next
    }

    fn exec_case(&mut self, c: &CaseClause, out: &mut Out, stdin: &StdinSrc) -> Flow {
        // `set -x` prints `case WORD in` (WORD in source form, unexpanded) once
        // before pattern matching, matching bash.
        if self.xtrace {
            self.xtrace_emit(&format!("case {} in", crate::unparse::word_src(&c.word)));
        }
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
                self.errln(&format!("{}{msg}", self.err_prefix()));
                self.last_status = 1;
                return Flow::Next;
            }
        };
        self.exec_with_redirects(plan, out, stdin, |sh, o, s| sh.exec_command(inner, o, s))
    }

    /// Run `run` with `plan`'s redirects installed for its whole duration, then
    /// torn down. Shared by compound-command redirection (`{ …; } > f`,
    /// `while …; done 2> err`) and function-invocation redirection
    /// (`myfunc > f`): it establishes the stdin source (here-doc/here-string/
    /// `< file` bytes), pushes the stderr target (`2> file`/`2>&N`/`2>&1`),
    /// installs scoped fd ≥ 3 descriptors, captures stdout when it is file- or
    /// stderr-bound, runs the body, then restores everything and finalises the
    /// captured stdout / folded stderr. `run` receives the (possibly capture)
    /// `Out` and the redirected `StdinSrc`.
    fn exec_with_redirects(
        &mut self,
        plan: RedirPlan,
        out: &mut Out,
        stdin: &StdinSrc,
        run: impl FnOnce(&mut Self, &mut Out, &StdinSrc) -> Flow,
    ) -> Flow {
        // Establish the input bytes (if the command redirects stdin).
        let input_bytes: Option<Vec<u8>> = if let Some(data) = plan.stdin_data.clone() {
            Some(data)
        } else if let Some(path) = &plan.stdin {
            match std::fs::read(map_device_path(path)) {
                Ok(b) => Some(b),
                Err(e) => {
                    self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                    self.last_status = 1;
                    return Flow::Next;
                }
            }
        } else {
            None
        };

        // ---- fd 1 file sink (open once) ----
        // When the compound command redirects stdout to a file (`{ …; } > f`,
        // `> f 2>&1`, `&> f`), open it once here and drive fd 1 through it *live*
        // (via a scoped `exec_stdout` override below) instead of buffering the
        // whole body and dumping it at the end. Live writes preserve ordering
        // and — crucially — let a same-file `2>&1`/`&>` interleave stdout and
        // stderr at one shared OS offset (both fds `try_clone` this handle,
        // which references the same file object on Unix and Windows alike). This
        // is the faithful model of bash dup'ing fd 1 around the group's body.
        let mut stdout_file: Option<Arc<File>> = None;
        if let Some((path, append)) = &plan.stdout {
            match open_out(path, *append) {
                Ok(f) => stdout_file = Some(Arc::new(f)),
                Err(e) => {
                    self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                    self.last_status = 1;
                    return Flow::Next;
                }
            }
        }

        // ---- stderr setup: push a target covering the whole body ----
        // `stderr_merge_buf` is the buffer whose bytes must be folded into the
        // stdout capture once the body finishes (the `2>&1`-into-captured-stdout
        // case, where fd 1 and fd 2 share a command-substitution buffer).
        let mut pushed_stderr = false;
        let mut stderr_merge_buf: Option<Arc<Mutex<Vec<u8>>>> = None;
        // fd-2 file sink for the group, kept so it can also seed a scoped
        // `exec_stderr` override — a `( … ) 2> f` / `( … ) 2>&1` subshell body
        // clones `exec_stderr` (but *not* `stderr_stack`), so this is what lets
        // a subshell's stderr reach the group's file at all.
        let mut stderr_file: Option<Arc<File>> = None;
        if let Some((path, append)) = &plan.stderr {
            // `> f 2>&1` / `&> f` / `2>f 1>&2`: fd 2 is a *dup* of fd 1's handle
            // (the resolver set `stderr_shares_stdout`). Share fd 1's already-open
            // handle (a `try_clone`, referencing the same open file description and
            // therefore the same offset) so the two streams interleave. An
            // *independent* `>f 2>f` (same path, but `stderr_shares_stdout` false)
            // opens a fresh handle — each redirect truncates to offset 0, so the
            // writes clobber, matching bash.
            let share_stdout = plan.stderr_shares_stdout
                && plan.stdout.as_ref().is_some_and(|(sp, _)| sp == path);
            let opened: io::Result<File> = match (share_stdout, &stdout_file) {
                (true, Some(f)) => f.try_clone(),
                _ => open_out(path, *append),
            };
            match opened {
                Ok(f) => {
                    let f = Arc::new(f);
                    self.stderr_stack.push(StderrTarget::File(Arc::clone(&f)));
                    stderr_file = Some(f);
                    pushed_stderr = true;
                }
                Err(e) => {
                    self.errln(&format!("{}{path}: {e}", self.err_prefix()));
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
                        self.errln(&format!("{}pipe: {e}", self.err_prefix()));
                        self.last_status = 1;
                        return Flow::Next;
                    }
                },
                Out::Inherit => {
                    if stdout_file.is_some() {
                        // `2>&1 >file`: the `2>&1` dup copies fd 1's sink *before*
                        // the later `>file` rebinds it, so fd 2 must stay on the
                        // pre-override fd 1 (terminal, or a persistent `exec>other`)
                        // — not follow the file. `StderrTarget::Stdout` resolves
                        // `exec_stdout` dynamically and would wrongly chase the
                        // override installed below, so snapshot the current fd 1
                        // sink into a concrete handle now.
                        match self.snapshot_std_fd(1) {
                            Ok(f) => {
                                self.stderr_stack.push(StderrTarget::File(Arc::new(f)));
                                pushed_stderr = true;
                            }
                            Err(e) => {
                                self.errln(&format!("{}stdout: {e}", self.err_prefix()));
                                self.last_status = 1;
                                return Flow::Next;
                            }
                        }
                    } else {
                        self.stderr_stack.push(StderrTarget::Stdout);
                        pushed_stderr = true;
                    }
                }
            }
        }

        // ---- scoped extra fds (fd ≥ 3 redirects on the compound command) ----
        // `{ …; } 3< file`, `while read -u 3; done 3< file`, `… 4> log`: install
        // the descriptor into the shell's open-fd table for the duration of the
        // body only, saving each touched fd's prior binding so it is restored —
        // and the scoped fd removed — when the body finishes. This is what makes
        // `read -u 3` inside the body read the file while fd 0 stays free.
        let saved_fds = self.install_extra_fds(&plan.extra_fds, out);

        // `{ …; } 1>&N` / `{ …; } 2>&N` (N ≥ 3): route fd 1 / fd 2 to the write
        // descriptor fd N currently holds — bash dup's the std fd onto fd N's
        // target for the body's whole duration. The lookup runs *after* the
        // scoped extra fds above are installed, so a same-command `3>f 1>&3`
        // resolves fd 3 correctly. `stdout_to_fd`/`stderr_to_fd` are mutually
        // exclusive with the file / `>&1` / `>&2` cases (the resolver clears the
        // others), so it is safe to feed the fd-N handle through the existing
        // `stdout_file` / `stderr_file` machinery. An unbound N fails the whole
        // command (bash: "N: Bad file descriptor").
        let mut fd_alias_error = false;
        if stdout_file.is_none()
            && let Some(n) = plan.stdout_to_fd
        {
            match self.open_write_fds.get(&n).map(|f| f.try_clone()) {
                Some(Ok(c)) => stdout_file = Some(Arc::new(c)),
                _ => {
                    self.errln(&format!("{}{n}: Bad file descriptor", self.err_prefix()));
                    self.last_status = 1;
                    fd_alias_error = true;
                }
            }
        }
        if !fd_alias_error
            && stderr_file.is_none()
            && let Some(n) = plan.stderr_to_fd
        {
            match self.open_write_fds.get(&n).map(|f| f.try_clone()) {
                Some(Ok(c)) => {
                    let f = Arc::new(c);
                    self.stderr_stack.push(StderrTarget::WriteFd(Arc::clone(&f)));
                    pushed_stderr = true;
                    stderr_file = Some(f);
                }
                _ => {
                    self.errln(&format!("{}{n}: Bad file descriptor", self.err_prefix()));
                    self.last_status = 1;
                    fd_alias_error = true;
                }
            }
        }

        // Capture stdout only when it is routed to stderr (`1>&2`) with no file
        // target — that folds fd 1's bytes into the pre-redirect stderr sink
        // after the body. A plain `> f` sends fd 1 to the file live (below), so
        // it no longer needs a buffer. Otherwise the body writes straight to
        // `out`.
        let stdout_to_err = plan.stdout_to_stderr && plan.stdout.is_none();
        let mut capture: Option<Vec<u8>> = if stdout_to_err {
            Some(Vec::new())
        } else {
            None
        };

        // Scoped fd-1/fd-2 overrides: point ambient stdout/stderr at the group's
        // file(s) for the duration of the body, saving the previous bindings (a
        // persistent `exec > other`, or the real handles) to restore afterwards.
        // Subshell clones inherit these handles (but not `stderr_stack`), so a
        // `( … ) > f 2>&1` body reaches the file — matching bash fd inheritance.
        let saved_exec_stdout = stdout_file
            .as_ref()
            .map(|f| self.exec_stdout.replace(Arc::clone(f)));
        let saved_exec_stderr = stderr_file
            .as_ref()
            .map(|f| self.exec_stderr.replace(Arc::clone(f)));

        let flow = if fd_alias_error {
            // A `1>&N` / `2>&N` alias named an unbound descriptor: the command
            // fails without running its body (matching bash), but the redirect
            // scope is still torn down below.
            Flow::Next
        } else {
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
            if stdout_file.is_some() {
                // fd 1 flows to the file via `exec_stdout`; run with an ambient
                // `Out::Inherit` (the group redirect fully rebinds fd 1, so the
                // enclosing capture/pipe is bypassed for stdout).
                let mut o = Out::Inherit;
                run(self, &mut o, sin)
            } else {
                match &mut capture {
                    Some(buf) => {
                        let mut o = Out::Capture(buf);
                        run(self, &mut o, sin)
                    }
                    None => run(self, out, sin),
                }
            }
        };

        // Restore the previous ambient fd-1 / fd-2 bindings.
        if let Some(prev) = saved_exec_stdout {
            self.exec_stdout = prev;
        }
        if let Some(prev) = saved_exec_stderr {
            self.exec_stderr = prev;
        }

        // Finalise the `1>&2` capture (fd 1 → fd 2, no file target): flush the
        // buffered stdout to the pre-redirect stderr sink now, while that target
        // is still on the stack. (A `> f` stdout is written live above, so it is
        // never captured here.)
        if let Some(buf) = capture {
            // `{ …; } >&2 2>file` (or a redirected function call): the `>&2` dup
            // captured fd 2 *before* this command's own `2>file` took effect
            // (bash applies redirects left to right, and the resolver only sets
            // `stdout_to_stderr` for that dup-first ordering). The per-command
            // stderr, if any, is the freshly-pushed top of the stack — skip it so
            // fd 1 lands in the pre-redirect sink. This capture is flushed before
            // the pop, so the top is still present. See TD-OILS14.
            let depth = if pushed_stderr {
                self.stderr_stack.len().saturating_sub(1)
            } else {
                self.stderr_stack.len()
            };
            self.emit_stderr_depth(&buf, depth);
        }

        if pushed_stderr {
            self.stderr_stack.pop();
        }

        // Restore the scoped extra fds: remove whatever the body left for each
        // touched fd and reinstate its prior binding (if any).
        self.restore_extra_fds(saved_fds);

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

    /// Install a command's `extra_fds` (fd ≥ 3 dups/opens/closes) into the
    /// shell's fd tables, returning the saved prior bindings for a later
    /// [`Self::restore_extra_fds`]. Applying these *before* resolving a
    /// same-command `N>&M` dup is what lets a later redirect see a descriptor an
    /// earlier one created (`echo hi 3>&1 2>&3`): the collapsed [`RedirPlan`]
    /// loses left-to-right order, so the fd must be materialised in the table
    /// first. Shared by the compound-command path and simple builtins.
    fn install_extra_fds(&mut self, extra_fds: &[(i32, ExtraFdOp)], out: &Out) -> Vec<SavedFd> {
        let mut saved_fds: Vec<SavedFd> = Vec::new();
        let mut already_saved: std::collections::HashSet<i32> = std::collections::HashSet::new();
        for (fd, op) in extra_fds {
            if already_saved.insert(*fd) {
                let prev_r = self.open_fds.remove(fd);
                let prev_w = self.open_write_fds.remove(fd);
                saved_fds.push((*fd, prev_r, prev_w));
            } else {
                // A repeated fd: drop whatever the earlier op installed before
                // applying this one (prior binding already saved).
                self.open_fds.remove(fd);
                self.open_write_fds.remove(fd);
            }
            match op {
                ExtraFdOp::InputBytes(bytes) => {
                    self.open_fds
                        .insert(*fd, RefCell::new(io::Cursor::new(bytes.clone())));
                }
                ExtraFdOp::OutputFile(path, append) => match open_out(path, *append) {
                    Ok(f) => {
                        self.open_write_fds.insert(*fd, std::sync::Arc::new(f));
                    }
                    Err(e) => {
                        self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                        self.last_status = 1;
                    }
                },
                ExtraFdOp::AliasStd(n) => {
                    // `N>&1` (N ≥ 3 dup'ing fd 1) on a *pipeline stage* must alias
                    // fd N to the stage's output pipe, not the ambient terminal /
                    // persistent `exec` stdout that `snapshot_std_fd(1)` returns.
                    // Otherwise a `>&N` write inside the body (e.g.
                    // `{ echo x >&3; } 3>&1 | cat`) leaks past the pipe and the
                    // downstream stage sees nothing.
                    let handle = match (*n, out) {
                        (1, Out::Pipe(w)) => pipe_writer_to_file(w),
                        _ => self.snapshot_std_fd(*n),
                    };
                    match handle {
                        Ok(f) => {
                            self.open_write_fds.insert(*fd, std::sync::Arc::new(f));
                        }
                        Err(e) => {
                            self.errln(&format!("{}{fd}: {e}", self.err_prefix()));
                            self.last_status = 1;
                        }
                    }
                }
                ExtraFdOp::Close => {} // already removed above
            }
        }
        saved_fds
    }

    /// Restore fd bindings saved by [`Self::install_extra_fds`]: remove whatever
    /// the body left for each touched fd and reinstate its prior binding.
    fn restore_extra_fds(&mut self, saved_fds: Vec<SavedFd>) {
        for (fd, prev_r, prev_w) in saved_fds.into_iter().rev() {
            self.open_fds.remove(&fd);
            self.open_write_fds.remove(&fd);
            if let Some(r) = prev_r {
                self.open_fds.insert(fd, r);
            }
            if let Some(w) = prev_w {
                self.open_write_fds.insert(fd, w);
            }
        }
    }

    /// Execute a `[[ … ]]` conditional expression: exit 0 if true, 1 if false.
    fn exec_cond(&mut self, e: &CondExpr) -> Flow {
        let ok = self.cond_eval(e);
        self.last_status = i32::from(!ok);
        Flow::Next
    }

    /// Execute a `(( … ))` arithmetic command: exit 0 if the value is non-zero.
    fn exec_arith(&mut self, raw: &str) -> Flow {
        if self.xtrace {
            self.xtrace_emit(&format!("(( {raw} ))"));
        }
        let expanded = self.expand_arith_params(raw);
        // bash tags `(( … ))`-command arithmetic errors with `((`.
        let saved_arith_cmd = self.arith_cmd;
        self.arith_cmd = Some("((");
        match arith::eval(&expanded, self) {
            Ok(v) => self.last_status = i32::from(v == 0),
            Err(e) => {
                self.emit_arith_error(&expanded, &e);
                self.last_status = 1;
            }
        }
        self.arith_cmd = saved_arith_cmd;
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
                self.errln(&format!("{}[[: =~: invalid regex: {}", self.err_prefix(), e.0));
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
            // `-o optname` tests whether the named shell option is enabled.
            UnaryOp::OptionSet => {
                let name = self.expand_to_string(w);
                self.shell_option_enabled(&name)
            }
            // `-L`/`-h` — the operand is a path; test whether it is a symlink
            // (without following the final component).
            UnaryOp::Symlink => {
                let path = self.expand_to_string(w);
                std::fs::symlink_metadata(&path)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
            }
            // `-t fd` — the operand is a descriptor number, not a path.
            UnaryOp::Terminal => {
                let fd = self.expand_to_string(w);
                match fd.parse::<i32>() {
                    Ok(0) => io::stdin().is_terminal(),
                    Ok(1) => io::stdout().is_terminal(),
                    Ok(2) => io::stderr().is_terminal(),
                    _ => false,
                }
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
                    UnaryOp::ZeroLen
                    | UnaryOp::NonZeroLen
                    | UnaryOp::VarSet
                    | UnaryOp::OptionSet
                    | UnaryOp::Symlink
                    | UnaryOp::Terminal => unreachable!(),
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
                    // bash matches the RHS of `==`/`!=` in `[[ ]]` "as if the
                    // extglob shell option were enabled" (see the [[ ]] section
                    // of the manual) — extended patterns like `+(f|o)`/`@(a|b)`
                    // are always recognised here regardless of the `extglob`
                    // setting, unlike `case`/glob (which gate on it at parse).
                    let pat: Vec<char> = rhs.chars().collect();
                    glob_match_ci(&pat, &subject, ci, true)
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
            env_imported: self.env_imported,
            funcs: self.funcs.clone(),
            positional: self.positional.clone(),
            name: self.name.clone(),
            last_status: self.last_status,
            comsub_count: self.comsub_count,
            // Entering a subshell increments the nesting depth (`$BASH_SUBSHELL`).
            subshell_depth: self.subshell_depth.saturating_add(1),
            last_bg_pid: self.last_bg_pid,
            pipefail: self.pipefail,
            pipe_broken: false,
            pid: self.pid,
            current_line: self.current_line,
            // A subshell inherits fd 2 = the shell's real stderr; any active
            // compound-command stderr redirect does not carry into a pipeline
            // stage's own subshell (and keeping the `Arc`s off the clone is what
            // lets `Shell` stay `Send` for the scoped-thread pipeline).
            stderr_stack: Vec::new(),
            // A subshell inherits the shell's fd table, including any persistent
            // `exec > file` / `exec 2> file` redirection.
            exec_stdout: self.exec_stdout.clone(),
            exec_stderr: self.exec_stderr.clone(),
            // The subshell inherits a snapshot of the remaining stdin bytes with
            // an independent cursor (see the field doc).
            exec_stdin: self.exec_stdin.as_ref().map(|c| {
                let cur = c.borrow();
                let pos = cur.position();
                let mut copy = io::Cursor::new(cur.get_ref().clone());
                copy.set_position(pos);
                RefCell::new(copy)
            }),
            // Snapshot each open input fd with its remaining bytes and an
            // independent offset (same approximation as exec_stdin above).
            open_fds: self
                .open_fds
                .iter()
                .map(|(&fd, c)| {
                    let cur = c.borrow();
                    let pos = cur.position();
                    let mut copy = io::Cursor::new(cur.get_ref().clone());
                    copy.set_position(pos);
                    (fd, RefCell::new(copy))
                })
                .collect(),
            // Write descriptors share the same file handle (bash: a subshell
            // inherits the fd, so writes go to one OS offset).
            open_write_fds: self
                .open_write_fds
                .iter()
                .map(|(&fd, f)| (fd, std::sync::Arc::clone(f)))
                .collect(),
            // A subshell inherits each live coproc read fd as a *shared* OS pipe
            // (bash: the subshell dups the coproc fd, one open file description).
            // `try_clone` duplicates the handle; a fresh `BufReader` starts empty
            // (any bytes the parent already buffered are not replayed — an exotic
            // edge: reading a coproc fd from inside a subshell).
            coproc_read_fds: self
                .coproc_read_fds
                .iter()
                .filter_map(|(&fd, rd)| {
                    rd.borrow()
                        .get_ref()
                        .try_clone()
                        .ok()
                        .map(|f| (fd, RefCell::new(io::BufReader::new(f))))
                })
                .collect(),
            // The subshell cannot join the parent's coproc threads.
            coproc_jobs: Vec::new(),
            // A subshell manages its own process-substitution lifetimes.
            procsub_in_temps: Vec::new(),
            procsub_out_jobs: Vec::new(),
            getopts_col: self.getopts_col,
            getopts_optind: self.getopts_optind,
            seconds_anchor: self.seconds_anchor,
            seconds_base: self.seconds_base,
            rng: std::cell::Cell::new(self.rng.get()),
            errexit: self.errexit,
            nounset: self.nounset,
            xtrace: self.xtrace,
            noglob: self.noglob,
            allexport: self.allexport,
            noclobber: self.noclobber,
            noexec: self.noexec,
            functrace: self.functrace,
            errtrace: self.errtrace,
            command_mode: self.command_mode,
            script_mode: self.script_mode,
            // A subshell starts outside any condition/negation context.
            errexit_suppress: 0,
            unbound_error: None,
            arith_error: false,
            arith_cmd: None,
            glob_error: None,
            // A subshell body is not itself a function frame; a `local` there is
            // an error until it enters one of its own function calls.
            local_frames: Vec::new(),
            // A subshell inherits the enclosing function context, so `FUNCNAME`
            // (and further nested calls) stay consistent.
            fn_stack: self.fn_stack.clone(),
            call_line_stack: self.call_line_stack.clone(),
            // A subshell starts a fresh `source` nesting (it is not itself a
            // sourced script), though it inherits the function context above.
            source_depth: 0,
            // A subshell body is not itself inside the parent's loop for the
            // purpose of `break`/`continue`: bash resets loop_level in a
            // subshell, so `(break)` inside a loop is an error, not a break.
            loop_depth: 0,
            readonly: self.readonly.clone(),
            shopt: self.shopt.clone(),
            // Completion specs are not propagated to subshells (bash parity).
            comp_specs: Vec::new(),
            integer_attr: self.integer_attr.clone(),
            array_valued: self.array_valued.clone(),
            lower_attr: self.lower_attr.clone(),
            upper_attr: self.upper_attr.clone(),
            capcase_attr: self.capcase_attr.clone(),
            nameref_attr: self.nameref_attr.clone(),
            fn_trace_attr: self.fn_trace_attr.clone(),
            // Keep the suppression stack in lockstep with the cloned `fn_stack`.
            // A subshell resets non-ignored traps, so most suppression flags are
            // moot there, but preserving the stack keeps nested function calls
            // inside the subshell correctly aligned.
            trap_suppress: self.trap_suppress.clone(),
            dir_stack: self.dir_stack.clone(),
            disabled_builtins: self.disabled_builtins.clone(),
            aliases: self.aliases.clone(),
            // A subshell resets non-ignored traps to their default disposition
            // (bash). Ignored ('') traps are always inherited. Additionally, a
            // subshell inherits the DEBUG/RETURN traps when `functrace` is on and
            // the ERR trap when `errtrace` is on — matching bash, which
            // propagates these pseudo-signal traps into subshells only under the
            // corresponding trace option.
            traps: self
                .traps
                .iter()
                .filter(|(k, v)| {
                    v.is_empty()
                        || (self.functrace && (k.as_str() == "DEBUG" || k.as_str() == "RETURN"))
                        || (self.errtrace && k.as_str() == "ERR")
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            // A subshell fires its *own* EXIT trap when it exits (bash). The
            // parent's EXIT trap was filtered out above (only ignored `''` traps
            // are inherited), so this only fires a trap the subshell installs
            // itself; the caller invokes `run_exit_trap_out` at the subshell
            // boundary. `false` here means "not yet fired".
            exit_trap_done: false,
            in_trap: false,
            // A subshell does not inherit the parent's job table.
            jobs: Vec::new(),
            // The umask is a process attribute, inherited by subshells.
            umask_val: self.umask_val,
            // The command hash table is inherited by subshells (bash).
            cmd_hash: self.cmd_hash.clone(),
            // Resource limits are a process attribute inherited by subshells.
            rlimits: self.rlimits.clone(),
        }
    }

    // ---- assignments and arrays ---------------------------------------------

    /// Apply a standalone assignment to shell state, handling scalars, indexed
    /// elements (`name[i]=v`), whole arrays (`name=(a b c)`), and append (`+=`).
    /// Apply a variable/array assignment. Returns `false` (and reports) if the
    /// target is readonly, leaving the existing value intact; `true` otherwise.
    /// Apply the `declare -l`/`-u`/`-c` case attribute (if any) of `name` to a
    /// value about to be stored. Lowercase (`-l`), uppercase (`-u`) and
    /// capitalize (`-c`) are mutually exclusive in bash; if several are somehow
    /// set, uppercase wins, then capitalize, then lowercase.
    fn fold_case_attr(&self, name: &str, val: String) -> String {
        if self.upper_attr.contains(name) {
            val.to_uppercase()
        } else if self.capcase_attr.contains(name) {
            capcase(&val)
        } else if self.lower_attr.contains(name) {
            val.to_lowercase()
        } else {
            val
        }
    }

    /// Transform an array-element value by the array name's value attributes:
    /// under `-i` the element is evaluated as an arithmetic expression, else
    /// under `-l`/`-u` its case is folded. Mirrors the scalar assignment path so
    /// `declare -ia a=(1+1)` stores `2` and `declare -ua u=(ab)` stores `AB`.
    fn apply_elem_attrs(&mut self, name: &str, val: String) -> String {
        if self.integer_attr.contains(name) {
            self.eval_int_assign(&val).to_string()
        } else {
            self.fold_case_attr(name, val)
        }
    }

    /// Store a plain scalar value into a variable, honoring the readonly guard,
    /// nameref redirection, the case attributes, and `set -a` export. Returns
    /// `false` (with the `readonly variable` diagnostic already emitted) when the
    /// target is readonly. Used by write paths outside `apply_assignment` — the
    /// `read` builtin and temporary `NAME=val cmd` env prefixes — so a readonly
    /// variable cannot be overwritten there either (bash rejects both).
    fn set_scalar_checked(&mut self, name: &str, val: String) -> bool {
        let target = self.resolve_ref_name(name);
        if self.readonly.contains(&target) {
            self.emit_stderr(format!("{}{target}: readonly variable\n", self.err_prefix()).as_bytes());
            return false;
        }
        if self.allexport {
            self.exported.insert(target.clone());
        }
        let val = self.fold_case_attr(&target, val);
        self.set_scalar_store(&target, val);
        true
    }

    /// Store a scalar value under `name`, honoring bash's rule that a plain
    /// scalar assignment to an existing *indexed* array updates element 0 (so
    /// `a=(1 2 3); a=x` leaves `${a[@]}` == `x 2 3` and `$a` == `x`). For a
    /// non-array name (or an associative array) it stores an ordinary scalar.
    fn set_scalar_store(&mut self, name: &str, val: String) {
        if self.arrays.contains_key(name) {
            self.arrays
                .entry(name.to_string())
                .or_default()
                .insert(0, val);
        } else if self.assoc.contains_key(name) {
            // A subscript-less `name=value` on an existing associative array
            // targets key "0" (bash: `declare -A b; b=a` yields `b[0]=a`).
            self.assoc_set(name, "0".to_string(), val, false);
        } else {
            self.vars.insert(name.to_string(), val);
        }
    }

    /// Apply a variable assignment. `trace` is true only for a *bare* assignment
    /// command (`x=5`), which `set -x` echoes: an indexed-element or array
    /// assignment is traced here in source form (bash does not expand it for the
    /// trace), while a plain scalar's trace is emitted at the store site below so
    /// the RHS is expanded exactly once.
    fn apply_assignment(&mut self, a: &Assignment, trace: bool) -> bool {
        // A nameref (`declare -n ref=target`) redirects the assignment to its
        // target: rewrite the name and re-run. `resolve_ref_name` follows the
        // whole chain, so the rewritten name is not itself a nameref (no loop).
        let target = self.resolve_ref_name(&a.name);
        if target != a.name {
            let mut a2 = a.clone();
            // A nameref may point at an array element (`declare -n ref=arr[0]`):
            // convert `ref=v` into `arr[0]=v`. Only when `ref` carries no
            // explicit subscript of its own (`ref[i]=v` is a different beast).
            if a.index.is_none()
                && let Some(open) = target.find('[')
                && let Some(inner) = target.strip_suffix(']')
            {
                a2.name = target[..open].to_string();
                a2.index = Some(Word::literal(&inner[open + 1..]));
            } else {
                a2.name = target;
            }
            return self.apply_assignment(&a2, trace);
        }
        // `set -x`: a plain scalar assignment is traced with its *expanded* value
        // (emitted at the scalar store below); everything else (indexed element,
        // array literal) is traced now in source form.
        let trace_scalar =
            trace && self.xtrace && a.index.is_none() && matches!(a.value, AssignRhs::Scalar(_));
        if trace && self.xtrace && !trace_scalar {
            let prefix = self.xtrace_prefix();
            self.emit_stderr(format!("{prefix}{}\n", crate::unparse::assignment_src(a)).as_bytes());
        }
        // A readonly variable cannot be reassigned; report and leave it intact.
        if self.readonly.contains(&a.name) {
            self.emit_stderr(format!("{}{}: readonly variable\n", self.err_prefix(), a.name).as_bytes());
            return false;
        }
        // `set -a` (allexport): any assigned variable is given the export
        // attribute automatically.
        if self.allexport {
            self.exported.insert(a.name.clone());
        }
        let is_assoc = self.assoc.contains_key(&a.name);
        match &a.value {
            AssignRhs::Scalar(w) => {
                let val = self.expand_assignment_value(w);
                // `set -x` trace for a plain scalar (`x=…`/`x+=…`): the expanded
                // RHS, minimally quoted, emitted once here so no re-expansion.
                if trace_scalar {
                    let prefix = self.xtrace_prefix();
                    let op = if a.append { "+=" } else { "=" };
                    self.emit_stderr(
                        format!("{prefix}{}{op}{}\n", a.name, xtrace_quote(&val)).as_bytes(),
                    );
                }
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
                            base.wrapping_add(self.eval_int_assign(&val)).to_string()
                        } else {
                            val
                        };
                        // Integer append already folded the old value in, so
                        // store (not append) the computed result.
                        self.assoc_set(&a.name, key, stored, a.append && !is_int);
                    } else {
                        // `name[i]=val` — indexed element assignment. A negative
                        // index counts back from `highest_index + 1` (bash:
                        // `a[-1]=v` overwrites the last element). A malformed
                        // arithmetic subscript is fatal (see `eval_arith_index`).
                        let raw = self.eval_arith_index(idx_word);
                        let bound = self
                            .arrays
                            .get(&a.name)
                            .and_then(|arr| arr.keys().next_back().copied())
                            .map_or(0, |k| k.saturating_add(1));
                        let Some(idx) = Self::resolve_index(raw, bound) else {
                            self.errln(&format!("{}{}: bad array subscript", self.err_prefix(), a.name));
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
                            Some(base.wrapping_add(self.eval_int_assign(&val)))
                        } else {
                            None
                        };
                        // An element assignment gives the array a value (bash's
                        // has-a-value flag): even after every element is later
                        // unset, `declare -p` shows `=()`, not the bare form.
                        self.array_valued.insert(a.name.clone());
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
                    // `name+=val` — append to the scalar, to element 0 of an
                    // indexed array, or to key "0" of an associative array
                    // (bash treats a subscript-less array assignment as index 0).
                    if self.assoc.contains_key(&a.name) {
                        if is_int {
                            let base = self
                                .assoc_element(&a.name, "0")
                                .and_then(|s| s.trim().parse::<i64>().ok())
                                .unwrap_or(0);
                            let sum = base.wrapping_add(self.eval_int_assign(&val));
                            self.assoc_set(&a.name, "0".to_string(), sum.to_string(), false);
                        } else {
                            self.assoc_set(&a.name, "0".to_string(), val, true);
                        }
                    } else if is_int {
                        let base = self
                            .vars
                            .get(&a.name)
                            .and_then(|c| c.trim().parse::<i64>().ok())
                            .unwrap_or(0);
                        let sum = base.wrapping_add(self.eval_int_assign(&val));
                        self.vars.insert(a.name.clone(), sum.to_string());
                    } else if self.arrays.contains_key(&a.name) {
                        self.array_valued.insert(a.name.clone());
                        if let Some(arr) = self.arrays.get_mut(&a.name) {
                            arr.entry(0).or_default().push_str(&val);
                        }
                    } else {
                        let cur = self.vars.get(&a.name).cloned().unwrap_or_default();
                        self.vars.insert(a.name.clone(), cur + &val);
                    }
                } else if is_int {
                    let n = self.eval_int_assign(&val);
                    self.set_scalar_store(&a.name, n.to_string());
                } else {
                    self.set_scalar_store(&a.name, val);
                }
            }
            AssignRhs::Array(items) if is_assoc => {
                // Associative literal: `m=([k]=v …)` (m already `declare -A`).
                // The literal (even the empty `m=()`) gives the array a value.
                self.array_valued.insert(a.name.clone());
                if !a.append {
                    self.assoc.insert(a.name.clone(), Vec::new());
                }
                for e in items {
                    match e {
                        ArrayElem::Keyed { index, value } => {
                            let key = self.expand_to_string(index);
                            let val = self.expand_to_string(value);
                            let val = self.apply_elem_attrs(&a.name, val);
                            self.assoc_set(&a.name, key, val, false);
                        }
                        ArrayElem::Positional(_) => {
                            self.errln(&format!("{}{}: must use subscript when assigning associative array", self.err_prefix(),
                                a.name
                            ));
                        }
                    }
                }
            }
            AssignRhs::Array(items) => {
                // Indexed literal: positional elements append at the running
                // index; `[i]=v` elements place at an explicit index. Stored
                // sparsely (a BTreeMap), so gaps between explicit indices are
                // absent rather than filled with empty strings. The literal
                // (even the empty `a=()`) gives the array a value.
                self.array_valued.insert(a.name.clone());
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
                            // Brace expansion runs first (textually), so
                            // `a=({1..3})` and `a=(x{a,b})` expand like command
                            // words before parameter/other expansion.
                            for bw in crate::brace::expand_braces(w) {
                                for v in self.expand_word(&bw, true) {
                                    let v = self.apply_elem_attrs(&a.name, v);
                                    elems.insert(next, v);
                                    next = next.saturating_add(1);
                                }
                            }
                        }
                        ArrayElem::Keyed { index, value } => {
                            let idx = self.eval_arith_index(index);
                            let val = self.expand_to_string(value);
                            let val = self.apply_elem_attrs(&a.name, val);
                            if let Ok(idx) = usize::try_from(idx) {
                                elems.insert(idx, val);
                                next = idx.saturating_add(1);
                            } else {
                                self.errln(&format!("{}{}: bad array subscript", self.err_prefix(), a.name));
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
        // Any element write gives the array a value (bash's has-a-value flag),
        // so an emptied assoc still shows `=()` under `declare -p`.
        self.array_valued.insert(name.to_string());
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
        let name = &self.resolve_ref_name(name);
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

    /// Compute an array/positional slice (`${a[@]:off:len}`, `${@:off:len}`).
    /// Elements are gathered by position (0-based over the set values; for `@`/
    /// `*` the list is `$0` followed by the positional parameters, matching
    /// bash). A negative offset counts from the end; a negative length stops
    /// that many elements before the end; an absent length runs to the end.
    fn slice_elements(
        &mut self,
        name: &str,
        offset: &Word,
        length: &Option<Box<Word>>,
    ) -> Vec<String> {
        let elems: Vec<String> = if name == "@" || name == "*" {
            let mut v = vec![self.param_value("0").unwrap_or_default()];
            v.extend(self.positional.iter().cloned());
            v
        } else {
            self.array_elements(name)
        };
        let n = elems.len() as i64;
        let off = self.eval_arith_index(offset);
        let start = if off < 0 { (n + off).max(0) } else { off.min(n) };
        let end = match length {
            Some(l) => {
                let l = self.eval_arith_index(l);
                // Unlike a string substring (where a negative length counts back
                // from the end of the string), an array / positional-parameter
                // slice rejects a negative length as a fatal expansion error
                // (bash: "N: substring expression < 0"). Route it through the
                // same fatal-arith abort machinery so the shell aborts at the
                // main level and the subshell fails without aborting the parent.
                if l < 0 {
                    self.errln(&format!("{}{l}: substring expression < 0", self.err_prefix()));
                    self.arith_error = true;
                    return Vec::new();
                }
                (start + l).min(n)
            }
            None => n,
        };
        if start >= end {
            return Vec::new();
        }
        elems
            .into_iter()
            .skip(start as usize)
            .take((end - start) as usize)
            .collect()
    }

    /// Gather the elements of an array or the positional parameters and apply a
    /// [`BulkOp`] to each (`${a[@]#pat}`, `${@/x/y}`, `${a[*]^^}`, …). For `@`/
    /// `*` the list is the positional parameters (matching bash — unlike a
    /// slice, `$0` is *not* included here).
    fn bulk_elements(&mut self, name: &str, op: &BulkOp) -> Vec<String> {
        // `@k` / `@K` are key-aware: they interleave subscripts and values
        // rather than transforming each value in place.
        if let BulkOp::Transform { op: k @ ('k' | 'K') } = op {
            return self.bulk_keyvalue(name, *k == 'K');
        }
        // `@A` / `@a` over `[@]`/`[*]` are collection-wide, not per-element:
        // `@A` yields one re-inputtable declare/`set --`, `@a` yields each
        // element's attribute letters.
        if let BulkOp::Transform { op: t @ ('A' | 'a') } = op {
            return self.bulk_attr_transform(name, *t);
        }
        let elems: Vec<String> = if name == "@" || name == "*" {
            self.positional.clone()
        } else {
            self.array_elements(name)
        };
        elems
            .into_iter()
            .map(|v| self.apply_bulk_op(op, &v))
            .collect()
    }

    /// `${a[@]@A}` / `${@@A}` (whole-collection assignment form) and
    /// `${a[@]@a}` (per-element attribute letters) — collection-wide `@`
    /// transforms that do not fit the per-element [`Shell::apply_bulk_op`] model.
    ///
    /// `@A` on a real array/assoc yields a single field holding the full
    /// re-inputtable `declare` (identical to `declare -p`); on the positional
    /// params (`${@@A}`/`${*@A}`) it yields a single `set -- 'a' 'b' …` field,
    /// matching bash. `@a` yields one attribute-letter field per element (the
    /// array's flag letters, e.g. `a`/`A`/`ar`); positional params have no
    /// attributes, so each field is empty.
    fn bulk_attr_transform(&mut self, name: &str, op: char) -> Vec<String> {
        let name = &self.resolve_ref_name(name);
        let positional = name == "@" || name == "*";
        if op == 'A' {
            if positional {
                // With no positional parameters, bash yields nothing (`${@@A}`
                // is empty) rather than a bare `set -- `.
                if self.positional.is_empty() {
                    return Vec::new();
                }
                let body = self
                    .positional
                    .iter()
                    .map(|v| shell_quote(v))
                    .collect::<Vec<_>>()
                    .join(" ");
                return vec![format!("set -- {body}")];
            }
            return self.format_declare_def(name).map_or_else(Vec::new, |s| vec![s]);
        }
        // op == 'a'
        let count = if positional {
            self.positional.len()
        } else {
            self.array_elements(name).len()
        };
        let letters = if positional { String::new() } else { self.attr_flag_letters(name) };
        vec![letters; count]
    }

    /// `${a[@]@k}` / `${a[@]@K}` — expand an array (or the positional params) as
    /// interleaved subscript/value pairs. `@k` yields each key and value as a
    /// *separate* word (`0 x 1 y`); `@K` yields a single field holding the pairs
    /// with each value double-quoted (`0 "x" 1 "y"`), matching bash's
    /// re-inputtable form.
    fn bulk_keyvalue(&mut self, name: &str, quoted: bool) -> Vec<String> {
        let (keys, values): (Vec<String>, Vec<String>) = if name == "@" || name == "*" {
            let vals = self.positional.clone();
            let keys = (1..=vals.len()).map(|i| i.to_string()).collect();
            (keys, vals)
        } else {
            (self.array_keys(name), self.array_elements(name))
        };
        if quoted {
            let body = keys
                .iter()
                .zip(&values)
                .map(|(k, v)| format!("{k} {}", quote_declare_value(v)))
                .collect::<Vec<_>>()
                .join(" ");
            vec![body]
        } else {
            keys.into_iter()
                .zip(values)
                .flat_map(|(k, v)| [k, v])
                .collect()
        }
    }

    /// Apply a single [`BulkOp`] to one element value.
    fn apply_bulk_op(&mut self, op: &BulkOp, value: &str) -> String {
        let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
        match op {
            BulkOp::Trim {
                suffix,
                longest,
                pattern,
            } => {
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                param_trim(value, &pat, *suffix, *longest, extglob)
            }
            BulkOp::Replace {
                all,
                anchor,
                pattern,
                replacement,
            } => {
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                let repl = self.expand_to_string(replacement);
                param_replace(value, &pat, &repl, *all, *anchor, extglob)
            }
            BulkOp::Case {
                mode,
                all,
                pattern,
            } => {
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                param_case(value, &pat, *mode, *all, extglob)
            }
            BulkOp::Transform { op } => Self::transform_value(value, *op),
        }
    }

    /// The keys (associative) or indices (indexed) of `name`, in order.
    fn array_keys(&self, name: &str) -> Vec<String> {
        let name = &self.resolve_ref_name(name);
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
        let name = &self.resolve_ref_name(name);
        match index {
            None => self.param_value(name),
            Some(w) => {
                if self.assoc.contains_key(name) {
                    let key = self.expand_to_string(w);
                    self.assoc_element(name, &key)
                } else {
                    let idx = self.eval_arith_index(w);
                    self.array_element(name, idx)
                }
            }
        }
    }

    /// Write `value` back to a parameter or array element, honoring an optional
    /// subscript. Used by `${name[i]:=default}` (assign-default). Out-of-range
    /// negative indices are ignored (matching bash's "bad subscript" no-op here).
    fn assign_elem(&mut self, name: &str, index: &Option<Box<Word>>, value: String) {
        let name = &self.resolve_ref_name(name);
        match index {
            None => {
                self.vars.insert(name.to_string(), value);
            }
            Some(w) => {
                if self.assoc.contains_key(name) {
                    let key = self.expand_to_string(w);
                    self.assoc_set(name, key, value, false);
                } else {
                    let idx = self.eval_arith_index(w);
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
        // Nameref special case: `${!ref}` where `ref` has the `-n` attribute
        // expands to the *name* of the referenced variable, not a second level
        // of indirection (bash). Follow the chain to the final target name.
        if self.nameref_attr.contains(refname) {
            return self.resolve_ref_name(refname);
        }
        let Some(target) = self.param_value(refname) else {
            // The pointer variable itself is unset: bash reports
            // "invalid indirect expansion" and aborts a non-interactive shell.
            // Reuse the nounset fatal-expansion flag (checked by the simple-
            // command driver) so the following command never runs.
            self.emit_stderr(format!("{}{refname}: invalid indirect expansion\n", self.err_prefix()).as_bytes());
            // bash aborts a bad indirect expansion with status 1 (not 127).
            self.unbound_error = Some(1);
            return String::new();
        };
        // The resolved name must be a valid parameter name. An empty or
        // malformed name (`ptr=`, `ptr="a b"`, `ptr=1abc`) is a fatal
        // "invalid variable name" error in bash (unlike a valid-but-unset
        // target such as `ptr=missing`, which quietly expands to empty).
        if !is_valid_indirect_target(&target) {
            self.emit_stderr(format!("{}{target}: invalid variable name\n", self.err_prefix()).as_bytes());
            // bash aborts an invalid indirect target with status 1 (not 127).
            self.unbound_error = Some(1);
            return String::new();
        }
        // The referent may name an array element: `ref=a[0]`, `ref=m[key]`,
        // or a whole-array reference `ref=a[@]` / `ref=a[*]`.
        if let Some(open) = target.find('[')
            && let Some(inner) = target.strip_suffix(']')
        {
            let name = &target[..open];
            let sub = &inner[open + 1..];
            // `${!ref}` where ref resolves to `name[@]`/`name[*]` expands like
            // `${name[@]}`/`${name[*]}` (bash). In this scalar (unjoined) path we
            // join with a space, matching `expand_array_ref`'s `[@]`/`[*]` join.
            if sub == "@" || sub == "*" {
                return self.array_elements(name).join(" ");
            }
            if self.assoc.contains_key(name) {
                return self.assoc_element(name, sub).unwrap_or_default();
            }
            // A malformed arithmetic subscript in an indirect array reference is
            // fatal, like a direct `${a[3 x]}` (see `eval_int_assign`).
            let idx = self.eval_int_assign(sub);
            return self.array_element(name, idx).unwrap_or_default();
        }
        self.param_value(&target).unwrap_or_default()
    }

    /// If `${!ref}` names a whole-array reference (`ref=a[@]` / `ref=a[*]`),
    /// return the array's element list (used by the quoted `"${!ref}"` field-
    /// preserving path). Returns `None` for scalar/element/name-list referents.
    fn indirect_array_elems(&mut self, refname: &str) -> Option<Vec<String>> {
        if self.nameref_attr.contains(refname) {
            return None;
        }
        let target = self.param_value(refname)?;
        if target.is_empty() {
            return None;
        }
        let open = target.find('[')?;
        let inner = target.strip_suffix(']')?;
        let name = &target[..open];
        let sub = &inner[open + 1..];
        if sub == "@" || sub == "*" {
            Some(self.array_elements(name))
        } else {
            None
        }
    }

    /// Attribute-flag letters for a variable, in `declare -p` order: the kind
    /// (`a` indexed / `A` associative) followed by `n` (nameref), `i` (integer),
    /// `l` (lower), `u` (upper), `r` (readonly), `x` (exported). Empty when the
    /// variable has no attributes. Shared by the `${var@a}` transform.
    fn attr_flag_letters(&self, name: &str) -> String {
        let mut s = String::new();
        if self.assoc.contains_key(name) {
            s.push('A');
        } else if self.arrays.contains_key(name) {
            s.push('a');
        }
        if self.nameref_attr.contains(name) {
            s.push('n');
        }
        if self.integer_attr.contains(name) {
            s.push('i');
        }
        if self.lower_attr.contains(name) {
            s.push('l');
        }
        if self.upper_attr.contains(name) {
            s.push('u');
        }
        if self.readonly.contains(name) {
            s.push('r');
        }
        if self.exported.contains(name) {
            s.push('x');
        }
        // bash orders the capitalize flag after i/r/x (`-ic`→`ic`, `-cx`→`xc`).
        if self.capcase_attr.contains(name) {
            s.push('c');
        }
        s
    }

    /// `${name@op}` parameter transformation. Supports `Q` (quote so the value
    /// can be reused as shell input), `U`/`u`/`L` (upper-all/upper-first/
    /// lower-all), `E` (expand ANSI-C backslash escapes), `a` (attribute
    /// flags — the kind plus `n`/`i`/`l`/`u`/`r`/`x`, else empty), and `A`
    /// (a re-inputtable assignment/`declare` statement recreating the variable).
    fn param_transform(&mut self, name: &str, index: &Option<Box<Word>>, op: char) -> String {
        // The `a` (attributes) transform reports type even for an unset scalar.
        if op == 'a' {
            return self.attr_flag_letters(name);
        }
        // `@A` recreates an assignment/`declare` statement for the variable.
        // The whole-array forms (`${arr[@]@A}` / `${arr[*]@A}`) are handled in
        // the bulk path; here `index` is either absent or a single element.
        if op == 'A' {
            return self.transform_assign(name, index);
        }
        // An unset variable yields the empty string for every transform (bash):
        // `${x@Q}` on unset is empty, whereas a set-but-empty variable is still
        // quoted (`${x@Q}` → `''`). Distinguish the two by the Option itself.
        let Some(value) = self.param_elem_value(name, index) else {
            return String::new();
        };
        if op == 'P' {
            return self.prompt_expand(&value);
        }
        Self::transform_value(&value, op)
    }

    /// The `set -x` trace-line prefix. bash uses `PS4` (default `+ `), with
    /// prompt-style backslash escapes expanded; its first character is repeated
    /// once per level of expansion indirection (we only trace at the top level,
    /// so it appears once). An unset `PS4` yields the default `+ `; an
    /// explicitly empty `PS4` yields no prefix. Parameter/arithmetic expansion
    /// inside `PS4` (e.g. `PS4='+ $LINENO '`) is not modelled — see
    /// known-issues TD-OILS-XTRACE.
    fn xtrace_prefix(&self) -> String {
        let ps4 = self.vars.get("PS4").map_or_else(|| "+ ".to_string(), Clone::clone);
        self.prompt_expand(&ps4)
    }

    /// Emit a single `set -x` trace line (prefix + `text` + newline) to stderr.
    /// Callers gate on `self.xtrace` themselves so `text` need not be built when
    /// tracing is off.
    fn xtrace_emit(&mut self, text: &str) {
        let prefix = self.xtrace_prefix();
        self.emit_stderr(format!("{prefix}{text}\n").as_bytes());
    }

    /// `${var@P}` — expand `var`'s value as a prompt string, interpreting the
    /// bash prompt escape sequences (`\u`, `\h`, `\w`, `\t`, `\d`, `\D{fmt}`,
    /// …). Backslash escapes not recognised keep the backslash and following
    /// character. Time-based escapes render in UTC (no local-timezone model
    /// yet, consistent with the `%(…)T` printf conversion — see TD-OILS9).
    fn prompt_expand(&self, s: &str) -> String {
        let (epoch, _) = unix_time();
        let epoch = epoch as i64;
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            let Some(&e) = chars.peek() else {
                out.push('\\');
                break;
            };
            match e {
                'a' => {
                    out.push('\u{07}');
                    chars.next();
                }
                'e' => {
                    out.push('\u{1b}');
                    chars.next();
                }
                'n' => {
                    out.push('\n');
                    chars.next();
                }
                'r' => {
                    out.push('\r');
                    chars.next();
                }
                '\\' => {
                    out.push('\\');
                    chars.next();
                }
                'd' => {
                    out.push_str(&format_strftime("%a %b %e", epoch));
                    chars.next();
                }
                'D' => {
                    chars.next(); // consume 'D'
                    // `\D{format}` — strftime; `\D{}` uses bash's default `%X`
                    // (we render 24h HH:MM:SS). A missing `{` leaves `\D` alone.
                    if chars.peek() == Some(&'{') {
                        chars.next(); // consume '{'
                        let mut fmt = String::new();
                        for fc in chars.by_ref() {
                            if fc == '}' {
                                break;
                            }
                            fmt.push(fc);
                        }
                        let fmt = if fmt.is_empty() { "%H:%M:%S" } else { &fmt };
                        out.push_str(&format_strftime(fmt, epoch));
                    } else {
                        out.push_str("\\D");
                    }
                }
                't' => {
                    out.push_str(&format_strftime("%H:%M:%S", epoch));
                    chars.next();
                }
                'T' => {
                    out.push_str(&format_strftime("%I:%M:%S", epoch));
                    chars.next();
                }
                '@' => {
                    out.push_str(&format_strftime("%I:%M %p", epoch));
                    chars.next();
                }
                'A' => {
                    out.push_str(&format_strftime("%H:%M", epoch));
                    chars.next();
                }
                'h' => {
                    let host = self.prompt_hostname();
                    let short = host.split('.').next().unwrap_or(&host);
                    out.push_str(short);
                    chars.next();
                }
                'H' => {
                    out.push_str(&self.prompt_hostname());
                    chars.next();
                }
                'u' => {
                    out.push_str(&self.prompt_username());
                    chars.next();
                }
                's' => {
                    // Shell name — basename of `$0`.
                    let arg0 = self.param_value("0").unwrap_or_default();
                    let base = arg0.rsplit(['/', '\\']).next().unwrap_or(&arg0);
                    out.push_str(base);
                    chars.next();
                }
                'j' => {
                    let n = self.jobs.iter().filter(|j| j.status.is_none()).count();
                    out.push_str(&n.to_string());
                    chars.next();
                }
                'l' => {
                    out.push_str("tty");
                    chars.next();
                }
                'v' => {
                    out.push_str("5.2");
                    chars.next();
                }
                'V' => {
                    out.push_str("5.2.0");
                    chars.next();
                }
                'w' => {
                    out.push_str(&self.prompt_cwd(false));
                    chars.next();
                }
                'W' => {
                    out.push_str(&self.prompt_cwd(true));
                    chars.next();
                }
                '!' | '#' => {
                    // History / command number — no interactive history model,
                    // so bash's first-command value.
                    out.push('1');
                    chars.next();
                }
                '$' => {
                    // `#` for the super-user, `$` otherwise. We infer root from
                    // the user name (no UID model on-target yet).
                    let root = self.prompt_username() == "root";
                    out.push(if root { '#' } else { '$' });
                    chars.next();
                }
                '[' | ']' => {
                    // Non-printing-sequence delimiters: bash emits \001/\002
                    // markers; for display we drop them.
                    chars.next();
                }
                '0'..='7' => {
                    // `\nnn` — up to three octal digits.
                    let mut digits = String::new();
                    while digits.len() < 3 {
                        match chars.peek() {
                            Some(&d @ '0'..='7') => {
                                digits.push(d);
                                chars.next();
                            }
                            _ => break,
                        }
                    }
                    if let Ok(byte) = u8::from_str_radix(&digits, 8) {
                        out.push(byte as char);
                    }
                }
                _ => {
                    // Unknown escape: keep the backslash and the character.
                    out.push('\\');
                    out.push(e);
                    chars.next();
                }
            }
        }
        out
    }

    /// The host name for prompt `\h`/`\H` — from `$HOSTNAME`, else `localhost`.
    fn prompt_hostname(&self) -> String {
        self.param_value("HOSTNAME")
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "localhost".to_string())
    }

    /// The user name for prompt `\u` — from `$USER`, then `$LOGNAME`, else
    /// `user`.
    fn prompt_username(&self) -> String {
        self.param_value("USER")
            .filter(|u| !u.is_empty())
            .or_else(|| self.param_value("LOGNAME").filter(|u| !u.is_empty()))
            .unwrap_or_else(|| "user".to_string())
    }

    /// The working directory for prompt `\w` (full, `$HOME`→`~`) or `\W`
    /// (basename only).
    fn prompt_cwd(&self, basename_only: bool) -> String {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if basename_only {
            let base = cwd.rsplit(['/', '\\']).next().unwrap_or(&cwd);
            return if base.is_empty() { "/".to_string() } else { base.to_string() };
        }
        if let Some(home) = self.param_value("HOME").filter(|h| !h.is_empty()) {
            if cwd == home {
                return "~".to_string();
            }
            if let Some(rest) = cwd.strip_prefix(&home)
                && rest.starts_with(['/', '\\'])
            {
                return format!("~{rest}");
            }
        }
        cwd
    }

    /// `${name@A}` — a re-inputtable assignment/`declare` statement that would
    /// recreate `name` with its current value and attributes. A plain scalar
    /// with no attributes renders as `name=<shell-quoted value>` (bash's short
    /// form); an attributed scalar renders as `declare -flags name='value'`.
    ///
    /// For an array or associative array this is the *single-element* form
    /// (`${arr[i]@A}`, or the plain/element-0 `${arr@A}`): bash renders the
    /// array's declare flags with just that one element's value in scalar
    /// single-quoted form — e.g. `declare -a arr='v'`, or `declare -a arr` for
    /// an unset element. The *whole-array* form (`${arr[@]@A}` / `${arr[*]@A}`)
    /// is a re-inputtable full `declare` and is produced by the bulk path
    /// ([`Shell::bulk_attr_transform`]), not here. An unset variable yields the
    /// empty string.
    fn transform_assign(&mut self, name: &str, index: &Option<Box<Word>>) -> String {
        if self.assoc.contains_key(name) || self.arrays.contains_key(name) {
            let kind = if self.assoc.contains_key(name) { "A" } else { "a" };
            let flags = self.declare_attr_flags(name, kind);
            return match self.param_elem_value(name, index) {
                Some(v) => format!("declare {flags} {name}={}", shell_quote(&v)),
                None => format!("declare {flags} {name}"),
            };
        }
        let Some(v) = self.vars.get(name) else {
            return String::new();
        };
        let attributed = self.readonly.contains(name)
            || self.exported.contains(name)
            || self.integer_attr.contains(name)
            || self.lower_attr.contains(name)
            || self.upper_attr.contains(name)
            || self.capcase_attr.contains(name)
            || self.nameref_attr.contains(name);
        // Both the plain (`name='value'`) and attributed (`declare -r name='value'`)
        // scalar forms single-quote the value: bash's `@A` uses sh_single_quote
        // here, unlike `declare -p`, which double-quotes.
        if attributed {
            format!("declare {} {name}={}", self.declare_attr_flags(name, ""), shell_quote(v))
        } else {
            format!("{name}={}", shell_quote(v))
        }
    }

    /// Apply a `@`-operator ([`op`]) to a concrete string value. Shared by the
    /// scalar `${x@Q}` path and the bulk `${a[@]@Q}` path.
    fn transform_value(value: &str, op: char) -> String {
        match op {
            'Q' => shell_quote(value),
            'U' => value.chars().flat_map(char::to_uppercase).collect(),
            'u' => {
                let mut cs = value.chars();
                match cs.next() {
                    Some(f) => f.to_uppercase().chain(cs).collect(),
                    None => String::new(),
                }
            }
            'l' => {
                // `@l` lowercases only the first character (mirror of `@u`).
                let mut cs = value.chars();
                match cs.next() {
                    Some(f) => f.to_lowercase().chain(cs).collect(),
                    None => String::new(),
                }
            }
            'L' => value.chars().flat_map(char::to_lowercase).collect(),
            'E' => ansi_c_unescape(value),
            // `K`/`k` on a *scalar* or single array element behave like `@Q`:
            // bash quotes the value (`${v@K}` on `v=abc` → `'abc'`). The
            // key-aware array form (`${a[@]@K}`) is intercepted earlier in the
            // bulk path (`bulk_keyvalue`); only the single-value case reaches
            // here, so both letters just quote.
            'K' | 'k' => shell_quote(value),
            // `P` (prompt) is handled in `param_transform` (it needs shell
            // state). Anything else: return the value unchanged rather than
            // erroring.
            _ => value.to_string(),
        }
    }

    /// Special variables that bash always reports from `${!prefix*}` but that
    /// osh computes on demand (in `param_value`) or only materialises in a
    /// narrower context than bash. The prefix listing must name them explicitly
    /// so it matches bash. `BASH_SOURCE`/`BASH_LINENO` are call-stack arrays
    /// that bash keeps present (possibly empty) at every level; osh only stores
    /// them in `arrays` while inside a function/script, so they are listed here
    /// too to match bash at the top level. `FUNCNAME` is deliberately *absent*:
    /// bash does not list it outside a function, and osh's in-function
    /// `arrays` entry is picked up there, so it appears only where bash lists it.
    const DYNAMIC_SPECIAL_NAMES: &'static [&'static str] = &[
        "BASHPID",
        "BASH_LINENO",
        "BASH_SOURCE",
        "BASH_SUBSHELL",
        "LINENO",
        "RANDOM",
        "SECONDS",
        "EPOCHSECONDS",
        "EPOCHREALTIME",
    ];

    /// `${!prefix*}` / `${!prefix@}` — the names of all set variables (scalars,
    /// indexed arrays, associative arrays) whose name begins with `prefix`,
    /// sorted (bash lists them in lexicographic order).
    fn var_names_with_prefix(&self, prefix: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .vars
            .keys()
            .chain(self.arrays.keys())
            .chain(self.assoc.keys())
            .map(String::as_str)
            .chain(Self::DYNAMIC_SPECIAL_NAMES.iter().copied())
            .filter(|k| k.starts_with(prefix))
            .map(str::to_string)
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn expand_array_ref(&mut self, name: &str, index: &ArrayIndex, length: bool) -> String {
        let name = &self.resolve_ref_name(name);
        match index {
            ArrayIndex::All | ArrayIndex::Star => {
                let elems = self.array_elements(name);
                if length {
                    elems.len().to_string()
                } else if matches!(index, ArrayIndex::Star) {
                    // `${arr[*]}` joins with the first character of `$IFS`
                    // (space when unset, empty when IFS is empty) — bash. The
                    // quoted `"${arr[*]}"` form reaches this scalar path, so the
                    // separator is observable.
                    elems.join(&self.star_sep())
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
                    let idx = self.eval_arith_index(w);
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
        // Process substitutions in this command's words create temp files (and,
        // for `>(cmd)`, deferred bodies). Record the marks, run the command (and
        // its whole body, for a function), then tear the substitutions down —
        // running deferred `>(cmd)` bodies and deleting all temp files.
        let in_mark = self.procsub_in_temps.len();
        let out_mark = self.procsub_out_jobs.len();
        let flow = self.exec_simple_inner(sc, out, stdin);
        self.finish_procsubs(in_mark, out_mark);
        flow
    }

    fn exec_simple_inner(&mut self, sc: &SimpleCommand, out: &mut Out, stdin: &StdinSrc) -> Flow {
        // Clear any stale `failglob` marker so a miss raised in an unchecked
        // expansion context can never misfire on an unrelated later command;
        // this command's own glob expansions re-set it below if they miss.
        self.glob_error = None;
        // Record the command about to run for `$BASH_COMMAND` (the command
        // currently executing, as seen by DEBUG/ERR traps and readable
        // generally). Not updated while a trap handler runs, so the handler
        // still sees the command that triggered it (bash). Uses the reconstructed
        // *unexpanded* source text, matching bash.
        if !self.in_trap {
            self.vars
                .insert("BASH_COMMAND".to_string(), crate::unparse::simple_src(sc));
        }
        // The DEBUG trap runs before each simple command (guarded so a handler's
        // own commands don't recurse). Suppressed inside an untraced function
        // frame that merely inherited the caller's DEBUG trap (bash).
        if !self.in_trap && self.traps.contains_key("DEBUG") && !self.trap_suppressed("DEBUG") {
            self.fire_trap("DEBUG");
        }
        // Expand the command words into argv (with the current variable values,
        // before any prefix assignments take effect).
        //
        // If the command word is a declaration builtin (`export`, `declare`,
        // `typeset`, `local`, `readonly`), its `NAME=value` operands are
        // assignments: bash expands them in *assignment context* (tilde-
        // expanded after `:`/at value start, and neither word-split nor
        // glob-expanded). We detect the declaration builtin syntactically from
        // the first word (matching bash) and route each assignment-form operand
        // through `expand_decl_assignment`.
        let is_decl = sc
            .words
            .first()
            .and_then(word_as_plain_literal)
            .is_some_and(is_declaration_builtin);
        let mut argv: Vec<String> = Vec::new();
        for (wi, w) in sc.words.iter().enumerate() {
            if is_decl && wi > 0 && is_assignment_word(w) {
                argv.push(self.expand_decl_assignment(w));
                continue;
            }
            // Brace expansion runs first (textually, before parameter/other
            // expansion), turning one word into one or more words.
            for bw in crate::brace::expand_braces(w) {
                argv.extend(self.expand_word(&bw, true));
            }
        }

        // `set -u`: a reference to an unset variable during expansion aborts the
        // shell (matching a non-interactive bash under nounset). The abort status
        // is carried by the flag (127 for nounset/`:?`, 1 for indirect/subscript)
        // but only at the main shell; a subshell yields 1 (see fatal_abort_status).
        if let Some(code) = self.unbound_error.take() {
            let status = self.fatal_abort_status(code);
            self.last_status = status;
            return Flow::Exit(status);
        }

        // An arithmetic error while expanding a command word (`echo $((1/0))`)
        // is fatal in a non-interactive shell: bash reports it and exits with
        // status 1 without running the command (it does not fabricate a value),
        // and never reaches a following command. Arithmetic *commands* (`(( ))`,
        // `let`, `for ((`) are non-fatal, but they never set this flag — only the
        // `$(( … ))`/`$[ … ]` expansion path (`arith_sub`) does. Prefix
        // assignment-value arith errors are checked after their own expansion.
        if self.arith_error {
            self.arith_error = false;
            self.last_status = 1;
            return Flow::Exit(1);
        }

        // `shopt -s failglob`: a command-word glob that matched nothing is a
        // fatal expansion error — bash reports `no match: PATTERN` and discards
        // the command (and, in a non-interactive `-c`, the rest of the list)
        // without running it.
        if let Some(pat) = self.glob_error.take() {
            self.emit_stderr(format!("{}no match: {pat}\n", self.err_prefix()).as_bytes());
            self.last_status = 1;
            return Flow::Exit(1);
        }

        // Pure assignment (no command word): persist the variables/arrays.
        // A readonly-variable rejection makes the whole command fail (status 1).
        if argv.is_empty() {
            // The exit status of a pure assignment is that of the last command
            // substitution performed while expanding its values (bash), or 0 if
            // there was none — so `x=$(false); echo $?` reports 1 while
            // `false; x=1; echo $?` reports 0. `$?` read inside the value still
            // sees the prior status (expansion happens before the reset below).
            // A readonly-variable rejection fails the whole command (status 1).
            let comsub_before = self.comsub_count;
            let mut ok = true;
            for a in &sc.assignments {
                if !self.apply_assignment(a, true) {
                    ok = false;
                }
            }
            // A `failglob` miss while expanding an array-literal value
            // (`arr=(*.nope)`) is fatal, just like the command-word case.
            if let Some(pat) = self.glob_error.take() {
                self.emit_stderr(format!("{}no match: {pat}\n", self.err_prefix()).as_bytes());
                self.arith_error = false;
                self.last_status = 1;
                return Flow::Exit(1);
            }
            // A fatal word-expansion error while expanding a *bare* assignment
            // value — a `nounset` reference (`set -u; x=$UNSET`), a `${var:?}`,
            // or a bad indirect/subscript expansion — aborts the shell (the
            // diagnostic was already printed). Without this check the flag would
            // only fire on the *next* command's word expansion, so a bare
            // assignment as the final statement would wrongly exit 0. bash's
            // abort status is carried by the flag (127 for nounset/`:?`, 1 for
            // indirect/subscript).
            if let Some(code) = self.unbound_error.take() {
                self.arith_error = false;
                let status = self.fatal_abort_status(code);
                self.last_status = status;
                return Flow::Exit(status);
            }
            if !ok || self.arith_error {
                // A readonly rejection or an arithmetic error in the value of a
                // *bare* assignment command is fatal in a non-interactive shell:
                // bash reports it and exits with status 1 (`readonly c=1; c=2;
                // echo after` and `x=$((1/0)); echo after` never reach `after`).
                // A temporary assignment *prefix* to a command is not fatal — that
                // path is handled separately in the command-execution branch.
                self.arith_error = false;
                self.last_status = 1;
                return Flow::Exit(1);
            } else if self.comsub_count == comsub_before {
                // No command substitution ran; a plain assignment resets $? to 0.
                self.last_status = 0;
            }
            // Otherwise `command_sub` already left the last substitution's status
            // in `self.last_status`.
            return Flow::Next;
        }

        // Command present: build scalar env prefixes (`FOO=bar cmd`). Array and
        // indexed prefix assignments collapse to a space-joined scalar.
        let mut assigns: Vec<(String, String)> = Vec::with_capacity(sc.assignments.len());
        for a in &sc.assignments {
            assigns.push(self.assignment_prefix_value(a));
        }

        // An arithmetic error while expanding a prefix assignment value
        // (`x=$((1/0)) cmd`) is fatal in a non-interactive shell: bash reports
        // it and exits with status 1 without running the command (matching the
        // bare-assignment and command-word cases above).
        if self.arith_error {
            self.arith_error = false;
            self.last_status = 1;
            return Flow::Exit(1);
        }

        // A fatal word-expansion error while expanding a prefix value — a
        // nounset reference under `set -u` or a bad indirect expansion
        // (`x=${!nonexist} cmd`) — likewise aborts the shell before running the
        // command (the diagnostic was already printed at expansion time). The
        // carried status distinguishes nounset/`:?` (127) from indirect (1), and
        // only applies at the main shell (a subshell yields 1).
        if let Some(code) = self.unbound_error.take() {
            let status = self.fatal_abort_status(code);
            self.last_status = status;
            return Flow::Exit(status);
        }

        // A `failglob` miss while expanding a prefix assignment value
        // (`x=*.nope cmd`) is fatal, mirroring the command-word case.
        if let Some(pat) = self.glob_error.take() {
            self.emit_stderr(format!("{}no match: {pat}\n", self.err_prefix()).as_bytes());
            self.last_status = 1;
            return Flow::Exit(1);
        }

        // A readonly variable cannot be set even as a temporary command prefix
        // (`readonly x; x=1 cmd` → error, command not run, status 1). Guard
        // before dispatch so no path (function/builtin/external) mutates it.
        for (k, _) in &assigns {
            let target = self.resolve_ref_name(k);
            if self.readonly.contains(&target) {
                self.emit_stderr(format!("{}{target}: readonly variable\n", self.err_prefix()).as_bytes());
                self.last_status = 1;
                return Flow::Next;
            }
        }

        // `set -x`: trace the command before running it. bash emits each temporary
        // prefix assignment (`FOO=bar cmd`) on its own line first, then the
        // command with each argument minimally quoted, all behind the PS4 prefix.
        if self.xtrace {
            let prefix = self.xtrace_prefix();
            for (k, v) in &assigns {
                self.emit_stderr(format!("{prefix}{k}={}\n", xtrace_quote(v)).as_bytes());
            }
            let mut line = prefix;
            for (i, a) in argv.iter().enumerate() {
                if i > 0 {
                    line.push(' ');
                }
                line.push_str(&xtrace_quote(a));
            }
            line.push('\n');
            self.emit_stderr(line.as_bytes());
        }

        // A redirection-only `exec` (`exec > file`, `exec 3>&1 1>&2 2>&3`,
        // `exec 1>&3`) mutates the shell's persistent fd table and must apply
        // its redirects in strict source order — which the collapsed RedirPlan
        // cannot express. Handle it directly here, before plan resolution.
        // (`exec cmd …`, and the rare `command exec`/`builtin exec` re-dispatch,
        // still go through the plan-based path below.)
        if argv.len() == 1 && argv[0] == "exec" && !sc.redirects.is_empty() {
            let rc = self.apply_exec_redirects(&sc.redirects);
            self.last_status = rc;
            return Flow::Next;
        }

        // Resolve redirections (targets are expanded now).
        let redir = match self.resolve_redirects(&sc.redirects) {
            Ok(r) => r,
            Err(msg) => {
                self.errln(&format!("{}{msg}", self.err_prefix()));
                self.last_status = 1;
                return Flow::Next;
            }
        };

        let name = argv[0].clone();

        // `$_` tracks the last argument of the most recent simple command, to be
        // read by the *next* command (bash). argv is fully expanded now — and
        // any `$_` inside it already read the previous value — so update it here
        // for the following command. (The startup form, where `$_` is the shell/
        // script pathname, is not modelled.)
        if let Some(last) = argv.last() {
            self.vars.insert("_".to_string(), last.clone());
        }

        // `declare -A m=([k]=v)` one-liner: array-literal operands are attached
        // to the command as `decl_arrays`; apply them with the declared kind.
        // `readonly`/`export` also accept inline array literals (`readonly
        // arr=(1 2)`), applying their implied `-r`/`-x` attribute.
        if !sc.decl_arrays.is_empty()
            && matches!(
                name.as_str(),
                "declare" | "typeset" | "local" | "readonly" | "export"
            )
        {
            return self.exec_declare_with_arrays(&argv, &sc.decl_arrays, out, &redir);
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

        // Function? A function invocation's own redirects (`myfunc > file`,
        // `myfunc 2> err`, `myfunc < in`) apply to the whole function body, so
        // run it inside a redirect scope when any are present. Without redirects,
        // dispatch directly to avoid the scope-setup overhead.
        if self.funcs.contains_key(&name) {
            let args: Vec<String> = argv[1..].to_vec();
            if redir.needs_scope() {
                return self.exec_with_redirects(redir, out, stdin, move |sh, o, s| {
                    sh.call_function(&name, &args, &assigns, o, s, &RedirPlan::default())
                });
            }
            return self.call_function(&name, &args, &assigns, out, stdin, &redir);
        }

        // Builtin? (unless disabled via `enable -n`, in which case fall through
        // to the same-named external.)
        if self.builtin_enabled(&name) {
            return self.run_builtin(&name, &argv, &assigns, out, stdin, &redir);
        }

        // External command. If a bare command name resolves nowhere on `$PATH`
        // and a `command_not_found_handle` function is defined, bash invokes
        // that function with the command word and its arguments (as `$1`, `$2`,
        // …) instead of reporting "command not found". The cheap function-
        // existence check comes first so the common case never scans `$PATH`
        // twice.
        if !name.contains('/')
            && !name.contains('\\')
            && self.funcs.contains_key("command_not_found_handle")
            && self.find_in_path(&name).is_none()
        {
            return self.call_function(
                "command_not_found_handle",
                &argv,
                &assigns,
                out,
                stdin,
                &redir,
            );
        }

        // Install any scratch descriptors (`N>&M`, `N>&-`, `exec`-style dup
        // targets) opened by this same command before spawning, so the external
        // can resolve `>&N` / `2>&N` against them — then tear them back down.
        // The collapsed RedirPlan cannot express left-to-right order between a
        // dup *source* (`3>&1`) and a plain dup (`2>&3`) on its own, so the
        // executor materialises the extra fds here (mirroring the compound and
        // builtin paths) rather than only inside `exec`.
        let ext_saved_fds = if redir.extra_fds.is_empty() {
            Vec::new()
        } else {
            self.install_extra_fds(&redir.extra_fds, out)
        };
        self.run_external(&argv, &assigns, out, stdin, &redir);
        self.restore_extra_fds(ext_saved_fds);
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
        // Track the function name for `FUNCNAME` while the body runs, plus the
        // line at the call site (the item currently executing) for `BASH_LINENO`.
        self.fn_stack.push(name.to_string());
        self.call_line_stack.push(self.current_line);
        self.refresh_funcname();
        // The `DEBUG`/`RETURN`/`ERR` traps are, by default, NOT inherited by a
        // called function. bash propagates DEBUG/RETURN into a function only when
        // `functrace` (`set -T`) is on or the function carries the trace
        // attribute (`declare -ft name`); it propagates ERR only when `errtrace`
        // (`set -E`) is on. Rather than removing the traps (which would hide them
        // from `trap -p` and lose them globally), we *mask* them for this frame:
        // they stay in `self.traps` but are suppressed from firing while this is
        // the innermost function. A trap the body reassigns via `trap` clears its
        // own mask (`unsuppress_trap`) and then fires normally and persists after
        // return — matching bash, where a body-installed trap is global.
        let trace_this = self.functrace || self.fn_trace_attr.contains(name);
        self.trap_suppress.push(TrapSuppress {
            debug: !trace_this,
            ret: !trace_this,
            err: !self.errtrace,
        });
        // A function body starts a fresh loop-nesting context: a `break`/
        // `continue` in the body must not escape to a loop at the call site
        // (bash resets loop_level on function entry). Save and reset.
        let saved_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
        // With tracing on, bash fires the DEBUG trap once more on *entry* to the
        // function body — before the first body command and with `$BASH_COMMAND`
        // still the call word — in addition to the per-command firings. Reproduce
        // that extra entry firing so the DEBUG count matches bash under
        // `functrace`/`declare -ft`.
        if trace_this && !self.in_trap && self.traps.contains_key("DEBUG") {
            self.fire_trap("DEBUG");
        }
        let flow = self.exec_program(&body, out, stdin);
        self.loop_depth = saved_loop_depth;
        // The RETURN trap fires when the function returns, before its locals are
        // torn down (so the handler still sees the function's scope), matching
        // bash. It fires when this frame inherits RETURN (tracing on) or when the
        // body installed its own RETURN (which cleared the mask); a merely-
        // inherited-but-masked RETURN stays suppressed.
        if self.traps.contains_key("RETURN") && !self.trap_suppressed("RETURN") {
            // Under tracing, bash fires DEBUG once more immediately before the
            // RETURN trap action, with `$BASH_COMMAND` still the last body
            // command. (This extra firing only appears when a RETURN trap is
            // actually present.)
            if trace_this && !self.in_trap && self.traps.contains_key("DEBUG") {
                self.fire_trap("DEBUG");
            }
            self.fire_trap("RETURN");
        }
        // Drop this frame's trap mask. Any trap the body installed remains in
        // `self.traps` (persisting globally, matching bash's `trap -p`).
        self.trap_suppress.pop();
        if let Some(frame) = self.local_frames.pop() {
            // Restore shadowed variables in reverse declaration order.
            for (name, snap) in frame.into_iter().rev() {
                self.restore_var(&name, snap);
            }
        }
        // Pop this call from the `FUNCNAME` stack.
        self.fn_stack.pop();
        self.call_line_stack.pop();
        self.refresh_funcname();

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

    /// The source-file label bash reports for a *function* call frame in
    /// `BASH_SOURCE`. It depends on how the shell was started, because bash has
    /// no real file for `-c`/interactive frames and substitutes a sentinel:
    ///   * script file (`osh SCRIPT`) → the script path (`$0`),
    ///   * `-c COMMAND`               → the literal string `environment`,
    ///   * stdin / interactive REPL   → the literal string `main`.
    ///
    /// (`caller` uses a *different* sentinel — `NULL` — for the non-file cases;
    /// see `caller_source`.) Per-function definition source is not tracked, so
    /// every frame reports the same label (see known-issues TD-OILS21).
    fn frame_source(&self) -> String {
        if self.script_mode {
            self.name.clone()
        } else if self.command_mode {
            "environment".to_string()
        } else {
            "main".to_string()
        }
    }

    /// `FUNCNAME[i]` for the current call stack, or `None` past the end.
    /// Index 0 is the innermost function; in script-file mode a bottom `main`
    /// pseudo-frame sits just past the real function frames.
    fn funcname_at(&self, i: usize) -> Option<String> {
        let depth = self.fn_stack.len();
        if i < depth {
            Some(self.fn_stack[depth - 1 - i].clone())
        } else if self.script_mode && i == depth {
            Some("main".to_string())
        } else {
            None
        }
    }

    /// `BASH_LINENO[i]` — the line at which `FUNCNAME[i]` was invoked. In
    /// script-file mode the bottom frame (the script itself) reports line 0.
    fn bash_lineno_at(&self, i: usize) -> Option<u32> {
        let depth = self.fn_stack.len();
        if i < depth {
            Some(self.call_line_stack[depth - 1 - i])
        } else if self.script_mode && i == depth {
            Some(0)
        } else {
            None
        }
    }

    /// `BASH_SOURCE[i]` — the source label of frame `i`. Function frames share
    /// `frame_source`; the script-mode base frame reports the script path.
    fn bash_source_at(&self, i: usize) -> Option<String> {
        let depth = self.fn_stack.len();
        if i < depth {
            Some(self.frame_source())
        } else if self.script_mode && i == depth {
            Some(self.name.clone())
        } else {
            None
        }
    }

    /// Materialise the `FUNCNAME`, `BASH_LINENO`, and `BASH_SOURCE` arrays from
    /// the current call stack. Bash makes `FUNCNAME[0]` the currently-executing
    /// function, then each caller outward. `BASH_LINENO[i]` is the line where
    /// `FUNCNAME[i]` was called, and `BASH_SOURCE[i]` is the source label of
    /// that frame (see `frame_source`).
    ///
    /// Bash's boundary behaviour differs by frame array *and* invocation mode:
    ///   * `-c` / stdin: no bottom frame — all three arrays hold exactly the
    ///     active function frames (empty at top level).
    ///   * script file: there is always a bottom frame for the script itself.
    ///     `BASH_SOURCE`/`BASH_LINENO` carry it even at top level (so
    ///     `${BASH_SOURCE[0]}` yields the script path outside any function),
    ///     but `FUNCNAME` gains its bottom `main` entry only once at least one
    ///     function frame sits above it. This makes the arrays legitimately
    ///     differ in length at a script's top level (FUNCNAME 0, the others 1).
    fn refresh_funcname(&mut self) {
        let mut names: BTreeMap<usize, String> = BTreeMap::new();
        let mut linenos: BTreeMap<usize, String> = BTreeMap::new();
        let mut sources: BTreeMap<usize, String> = BTreeMap::new();
        let src = self.frame_source();
        let mut idx = 0usize;
        // Walk both stacks from innermost (last) to outermost (first).
        for (name, line) in self
            .fn_stack
            .iter()
            .rev()
            .zip(self.call_line_stack.iter().rev())
        {
            names.insert(idx, name.clone());
            linenos.insert(idx, line.to_string());
            sources.insert(idx, src.clone());
            idx += 1;
        }
        if self.script_mode {
            // FUNCNAME gains `main` only when a function frame sits above it.
            if !self.fn_stack.is_empty() {
                names.insert(idx, "main".to_string());
            }
            // BASH_SOURCE/BASH_LINENO always carry the script's own base frame.
            linenos.insert(idx, "0".to_string());
            sources.insert(idx, self.name.clone());
        }
        if names.is_empty() && linenos.is_empty() && sources.is_empty() {
            self.arrays.remove("FUNCNAME");
            self.arrays.remove("BASH_LINENO");
            self.arrays.remove("BASH_SOURCE");
            return;
        }
        self.arrays.insert("FUNCNAME".to_string(), names);
        self.arrays.insert("BASH_LINENO".to_string(), linenos);
        self.arrays.insert("BASH_SOURCE".to_string(), sources);
    }

    /// Capture the complete current state of a variable name (scalar / indexed /
    /// associative / export flag), for later restoration by `local`.
    fn snapshot_var(&self, name: &str) -> VarSnapshot {
        VarSnapshot {
            scalar: self.vars.get(name).cloned(),
            indexed: self.arrays.get(name).cloned(),
            assoc: self.assoc.get(name).cloned(),
            exported: self.exported.contains(name),
            integer: self.integer_attr.contains(name),
            lower: self.lower_attr.contains(name),
            upper: self.upper_attr.contains(name),
            capcase: self.capcase_attr.contains(name),
            nameref: self.nameref_attr.contains(name),
            readonly: self.readonly.contains(name),
            array_valued: self.array_valued.contains(name),
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
        // Restore the attribute flags to their pre-`local` state so any
        // `-i`/`-l`/`-u`/`-n`/`-r` set on the local does not leak out.
        Self::restore_flag(&mut self.integer_attr, name, snap.integer);
        Self::restore_flag(&mut self.lower_attr, name, snap.lower);
        Self::restore_flag(&mut self.upper_attr, name, snap.upper);
        Self::restore_flag(&mut self.capcase_attr, name, snap.capcase);
        Self::restore_flag(&mut self.nameref_attr, name, snap.nameref);
        Self::restore_flag(&mut self.readonly, name, snap.readonly);
        Self::restore_flag(&mut self.array_valued, name, snap.array_valued);
    }

    /// Set-or-clear `name`'s membership in an attribute set to match `present`.
    fn restore_flag(set: &mut HashSet<String>, name: &str, present: bool) {
        if present {
            set.insert(name.to_string());
        } else {
            set.remove(name);
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
        // Clear the current binding: a bare `local x` starts unset/empty and
        // without inherited attributes (bash: a local does not inherit a global's
        // `-i`/`-l`/`-u`/`-n`). `readonly` is intentionally left intact so a
        // readonly global is not silently shadowed. Any flags on the `local`
        // declaration itself are re-applied by the caller afterwards.
        self.vars.remove(name);
        self.arrays.remove(name);
        self.assoc.remove(name);
        self.integer_attr.remove(name);
        self.lower_attr.remove(name);
        self.upper_attr.remove(name);
        self.capcase_attr.remove(name);
        self.nameref_attr.remove(name);
        self.array_valued.remove(name);
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
                    self.emit_cmd_stderr(out, redir, &format!("{}command: {other}: invalid option", self.err_prefix()));
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
        // Run `target` bypassing functions. A disabled builtin (via `enable -n`)
        // runs the same-named external instead.
        if self.builtin_enabled(target) {
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
        self.emit_cmd_stderr(out, redir, &format!("{}builtin: {sub}: not a shell builtin", self.err_prefix()));
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
        } else if self.builtin_enabled(target) {
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
                self.errln(&format!("{}command: {target}: not found", self.err_prefix()));
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
        let path = match self.param_value("PATH") {
            Some(p) => p,
            // Only consult the real process PATH when the shell has not taken
            // ownership of its environment; once imported, an unset PATH means
            // no path search (bash).
            None if !self.env_imported => std::env::var("PATH").ok()?,
            None => return None,
        };
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

    /// Resolve an external command name to a full path for execution, consulting
    /// and populating the `hash` cache. A name containing a slash is used as-is
    /// (never hashed). For a bare name: a cached entry is reused (and its hit
    /// count bumped); otherwise a `$PATH` search runs and a hit is remembered.
    /// Returns `None` when the name cannot be resolved — the caller then falls
    /// back to letting the OS attempt the spawn (preserving prior behavior).
    fn resolve_external(&mut self, name: &str) -> Option<std::path::PathBuf> {
        if name.contains('/') || name.contains('\\') {
            return self.find_in_path(name);
        }
        if let Some((path, hits)) = self.cmd_hash.get_mut(name) {
            *hits += 1;
            return Some(path.clone());
        }
        let path = self.find_in_path(name)?;
        self.cmd_hash.insert(name.to_string(), (path.clone(), 1));
        Some(path)
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
        let path = match self.param_value("PATH") {
            Some(p) => p,
            None if !self.env_imported => match std::env::var("PATH") {
                Ok(p) => p,
                Err(_) => return out,
            },
            None => return out,
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
        // Resolve via the shell's `$PATH` (and the `hash` cache) when possible;
        // fall back to the bare name so the OS can still try to locate it.
        let mut cmd = match self.resolve_external(&argv[0]) {
            Some(path) => PCommand::new(path),
            None => PCommand::new(&argv[0]),
        };
        cmd.args(&argv[1..]);

        // Environment: exported shell vars + this command's temp assignments.
        // When the shell owns its environment, start from a cleared base so an
        // unset/non-exported variable does not leak in via inheritance.
        if self.env_imported {
            cmd.env_clear();
        }
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
        if let Some(n) = redir.stdin_from_fd {
            if let Some(rd) = self.coproc_read_fds.get(&n) {
                // `cmd <&"${COPROC[0]}"`: hand the child a dup of the live coproc
                // read pipe so it streams (slurping would block until the coproc
                // closed its stdout). Bytes already buffered by an earlier `read`
                // on this fd are not replayed (rare mixed use — documented).
                match rd.borrow().get_ref().try_clone() {
                    Ok(f) => {
                        cmd.stdin(Stdio::from(f));
                    }
                    Err(e) => {
                        self.errln(&format!("{}coproc: {e}", self.err_prefix()));
                        self.last_status = 1;
                        return;
                    }
                }
            } else {
                // `cmd <&N`: feed the child the source cursor's remaining bytes,
                // advancing it (a close approximation of a shared-offset dup).
                let mut rest = Vec::new();
                if let Some(cur) = self.input_fd_cursor(n) {
                    let _ = cur.borrow_mut().read_to_end(&mut rest);
                }
                input_bytes = Some(rest);
                cmd.stdin(Stdio::piped());
            }
        } else if let Some(data) = &redir.stdin_data {
            input_bytes = Some(data.clone());
            cmd.stdin(Stdio::piped());
        } else {
            match &redir.stdin {
                Some(path) => match std::fs::File::open(map_device_path(path)) {
                    Ok(f) => {
                        cmd.stdin(Stdio::from(f));
                    }
                    Err(e) => {
                        self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                        self.last_status = 1;
                        return;
                    }
                },
                None => match stdin {
                    StdinSrc::Inherit => {
                        // A persistent `exec < file` feeds the child fd 0 the
                        // cursor's remaining bytes (advancing it, so a later
                        // `read` continues after what the child consumed via the
                        // pipe buffer — a close approximation of a shared fd).
                        if let Some(cur) = &self.exec_stdin {
                            let mut rest = Vec::new();
                            let _ = cur.borrow_mut().read_to_end(&mut rest);
                            input_bytes = Some(rest);
                            cmd.stdin(Stdio::piped());
                        } else {
                            cmd.stdin(Stdio::inherit());
                        }
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
                                self.errln(&format!("{}pipe: {e}", self.err_prefix()));
                                self.last_status = 1;
                                return;
                            }
                        }
                    }
                },
            }
        }

        // stdout
        let capturing =
            matches!(out, Out::Capture(_)) && redir.stdout.is_none() && redir.stdout_to_fd.is_none();
        // For dup forms like `>f 2>&1` / `&>f` the resolver rewrites fd 2 to the
        // same file path as fd 1, but stdout and stderr must share ONE open file
        // description (interleaved, one offset) rather than two independent
        // truncating opens (which would clobber). Keep a clone of the stdout file
        // to hand to stderr in that case.
        let mut stdout_file_for_stderr: Option<File> = None;
        match &redir.stdout {
            Some((path, append)) => match open_out(path, *append) {
                Ok(f) => {
                    if redir.stderr_shares_stdout
                        && redir.stderr.as_ref().is_some_and(|(sp, _)| sp == path)
                        && let Ok(c) = f.try_clone()
                    {
                        stdout_file_for_stderr = Some(c);
                    }
                    cmd.stdout(Stdio::from(f));
                }
                Err(e) => {
                    self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                    self.last_status = 1;
                    return;
                }
            },
            None if redir.stdout_to_fd.is_some() => {
                // `cmd >&N` (N ≥ 3): the child's fd 1 is a user-space write
                // descriptor opened by `exec N> file`.
                let n = redir.stdout_to_fd.unwrap_or(0);
                match self.open_write_fds.get(&n).map(|f| f.try_clone()) {
                    Some(Ok(f)) => {
                        cmd.stdout(Stdio::from(f));
                    }
                    _ => {
                        self.errln(&format!("{}{n}: Bad file descriptor", self.err_prefix()));
                        self.last_status = 1;
                        return;
                    }
                }
            }
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
                            self.errln(&format!("{}{e}", self.err_prefix()));
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
                            self.errln(&format!("{}pipe: {e}", self.err_prefix()));
                            self.last_status = 1;
                            return;
                        }
                    }
                } else if let Some(f) = &self.exec_stdout {
                    // Persistent `exec > file`: the child's fd 1 is the file (a
                    // dup of the shared handle, so it writes at the live offset).
                    match f.try_clone() {
                        Ok(fc) => {
                            cmd.stdout(Stdio::from(fc));
                        }
                        Err(e) => {
                            self.errln(&format!("{}exec stdout: {e}", self.err_prefix()));
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
        if let Some(n) = redir.stderr_to_fd {
            // `cmd 2>&N` (N ≥ 3): the child's fd 2 is a user-space write fd.
            match self.open_write_fds.get(&n).map(|f| f.try_clone()) {
                Some(Ok(f)) => {
                    cmd.stderr(Stdio::from(f));
                }
                _ => {
                    self.errln(&format!("{}{n}: Bad file descriptor", self.err_prefix()));
                    self.last_status = 1;
                    return;
                }
            }
        } else if let Some((path, append)) = &redir.stderr {
            // Reuse the stdout handle for dup forms (shared file description),
            // otherwise open the target independently (`2>file` clobbers on its own).
            let opened = match stdout_file_for_stderr.take() {
                Some(f) => Ok(f),
                None => open_out(path, *append),
            };
            match opened {
                Ok(f) => {
                    cmd.stderr(Stdio::from(f));
                }
                Err(e) => {
                    self.errln(&format!("{}{path}: {e}", self.err_prefix()));
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
                        self.errln(&format!("{}pipe: {e}", self.err_prefix()));
                        self.last_status = 1;
                        return;
                    }
                }
            } else {
                cmd.stderr(Stdio::inherit());
            }
        } else {
            match self.stderr_stack.last() {
                None => {
                    // Base fd 2: a persistent `exec 2> file` target, else inherit.
                    if let Some(f) = &self.exec_stderr {
                        match f.try_clone() {
                            Ok(fc) => {
                                cmd.stderr(Stdio::from(fc));
                            }
                            Err(e) => {
                                self.errln(&format!("{}exec stderr: {e}", self.err_prefix()));
                                self.last_status = 1;
                                return;
                            }
                        }
                    }
                }
                Some(StderrTarget::File(f)) => match f.try_clone() {
                    Ok(fc) => {
                        cmd.stderr(Stdio::from(fc));
                    }
                    Err(e) => {
                        self.errln(&format!("{}stderr: {e}", self.err_prefix()));
                        self.last_status = 1;
                        return;
                    }
                },
                Some(StderrTarget::Pipe(p)) => match p.try_clone() {
                    Ok(pc) => {
                        cmd.stderr(Stdio::from(pc));
                    }
                    Err(e) => {
                        self.errln(&format!("{}pipe: {e}", self.err_prefix()));
                        self.last_status = 1;
                        return;
                    }
                },
                Some(StderrTarget::Buffer(b)) => {
                    cmd.stderr(Stdio::piped());
                    stderr_capture = Some(Arc::clone(b));
                }
                // An enclosing `2>&N` (N ≥ 3) scoped stderr: hand the child a
                // clone of the user-space write descriptor.
                Some(StderrTarget::WriteFd(f)) => match f.try_clone() {
                    Ok(fc) => {
                        cmd.stderr(Stdio::from(fc));
                    }
                    Err(e) => {
                        self.errln(&format!("{}stderr: {e}", self.err_prefix()));
                        self.last_status = 1;
                        return;
                    }
                },
                // fd 2 follows fd 1: a persistent `exec > file` target if set,
                // else inherit (fd 2 → terminal, same visual result at the
                // shell's controlling terminal).
                Some(StderrTarget::Stdout) => {
                    if let Some(f) = &self.exec_stdout {
                        match f.try_clone() {
                            Ok(fc) => {
                                cmd.stderr(Stdio::from(fc));
                            }
                            Err(e) => {
                                self.errln(&format!("{}exec stdout: {e}", self.err_prefix()));
                                self.last_status = 1;
                                return;
                            }
                        }
                    } else {
                        cmd.stderr(Stdio::inherit());
                    }
                }
            }
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    self.emit_cmd_stderr(out, redir, &format!("{}{}: command not found", self.err_prefix(), argv[0]));
                    self.last_status = 127;
                } else {
                    self.emit_cmd_stderr(out, redir, &format!("{}{}: {e}", self.err_prefix(), argv[0]));
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
                self.errln(&format!("{}wait failed: {e}", self.err_prefix()));
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
            if !argv.is_empty() && !self.funcs.contains_key(&argv[0]) && !self.builtin_enabled(&argv[0]) {
                let mut cmd = PCommand::new(&argv[0]);
                cmd.args(&argv[1..]);
                if self.env_imported {
                    cmd.env_clear();
                }
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
                            no_hup: false,
                        });
                        self.last_bg_pid = Some(pid);
                        self.last_status = 0;
                        return;
                    }
                    Err(e) => {
                        self.errln(&format!("{}{}: {e}", self.err_prefix(), argv[0]));
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

    /// Execute `coproc [NAME] body`: run `body` on a background thread with its
    /// stdin/stdout wired to two OS pipes, and expose the parent-side endpoints
    /// as `NAME[0]` (read the coproc's stdout), `NAME[1]` (write its stdin) plus
    /// the scalar `NAME_PID`. `NAME` defaults to `COPROC`.
    fn exec_coproc(&mut self, name: Option<&str>, body: &Command) -> Flow {
        let name = name.unwrap_or("COPROC").to_string();
        // Pipe A carries the parent's writes to the coproc's stdin; pipe B
        // carries the coproc's stdout back to the parent.
        let (child_stdin_r, parent_stdin_w) = match io::pipe() {
            Ok(p) => p,
            Err(e) => {
                self.errln(&format!("{}coproc: {e}", self.err_prefix()));
                self.last_status = 1;
                return Flow::Next;
            }
        };
        let (parent_stdout_r, child_stdout_w) = match io::pipe() {
            Ok(p) => p,
            Err(e) => {
                self.errln(&format!("{}coproc: {e}", self.err_prefix()));
                self.last_status = 1;
                return Flow::Next;
            }
        };
        // Clone the shell state for the coproc body *before* installing the
        // parent-side endpoints, so the body does not inherit copies of its own
        // coproc fds (bash closes them in the child).
        let mut sub = self.clone_for_subshell();
        let body_owned = body.clone();
        let handle = std::thread::spawn(move || {
            let mut out = Out::Pipe(child_stdout_w);
            let sin = StdinSrc::Pipe(RefCell::new(io::BufReader::new(child_stdin_r)));
            let _ = sub.exec_command(&body_owned, &mut out, &sin);
            // Dropping `out` (its `PipeWriter`) at scope end closes the coproc's
            // stdout, delivering EOF to the parent's `NAME[0]` reader.
        });
        self.coproc_jobs.push(handle);

        // Parent-side endpoints get fresh descriptors ≥ 10 (never colliding with
        // exec/varfd fds). Read end → the live coproc-read table; write end →
        // the ordinary write-fd table (so `>&"${NAME[1]}"` just works).
        let read_fd = self.alloc_varfd(&[]);
        let read_file = pipe_reader_into_file(parent_stdout_r);
        self.coproc_read_fds
            .insert(read_fd, RefCell::new(io::BufReader::new(read_file)));
        let write_fd = self.alloc_varfd(&[]);
        let write_file = pipe_writer_into_file(parent_stdin_w);
        self.open_write_fds
            .insert(write_fd, std::sync::Arc::new(write_file));

        // Publish `NAME=(read_fd write_fd)` and `NAME_PID`. The body runs as a
        // thread, not an OS process, so `NAME_PID` is a synthetic monotonic id
        // (best-effort, like other in-process background bodies).
        let synth_pid = COPROC_PID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut elems = BTreeMap::new();
        elems.insert(0usize, read_fd.to_string());
        elems.insert(1usize, write_fd.to_string());
        self.arrays.insert(name.clone(), elems);
        self.vars.insert(format!("{name}_PID"), synth_pid.to_string());
        self.last_bg_pid = Some(synth_pid);
        self.last_status = 0;
        Flow::Next
    }

    // ---- redirection resolution ---------------------------------------------

    /// Allocate the lowest free descriptor ≥ 10 for a `{name}>file` varfd
    /// redirect. Bash reserves 10+ for these auto-allocated fds so they never
    /// collide with user `exec 3>…` descriptors. `reserved` holds fds already
    /// handed out earlier in the *same* redirection list (multiple varfds in
    /// one command each get a distinct number).
    fn alloc_varfd(&self, reserved: &[i32]) -> i32 {
        let mut n = 10;
        while self.open_fds.contains_key(&n)
            || self.open_write_fds.contains_key(&n)
            || self.coproc_read_fds.contains_key(&n)
            || reserved.contains(&n)
        {
            n += 1;
        }
        n
    }

    /// Resolve a redirect's effective fd, honouring `{name}>…` varfd syntax.
    ///
    /// For an *open* form (`{name}>file`, `{name}<file`, `{name}>&N`, …) bash
    /// allocates the lowest free descriptor ≥ 10, stores its number in the
    /// shell variable `name`, and binds it. For the *close* form
    /// (`{name}>&-` / `{name}<&-`) bash does **not** allocate: `name`'s current
    /// value names the descriptor to close, and the variable is left unchanged.
    /// `Err` on assignment to a readonly variable, or a close form whose
    /// variable is unset / non-numeric. For a plain redirect this returns
    /// `r.fd`.
    fn redir_effective_fd(&mut self, r: &Redirect, reserved: &mut Vec<i32>) -> Result<i32, String> {
        match &r.varfd {
            Some(name) => {
                if redir_is_close(r) {
                    // `{v}>&-`: operate on the fd currently held in `$v`.
                    return match self.vars.get(name).and_then(|s| s.parse::<i32>().ok()) {
                        Some(n) => Ok(n),
                        None => Err(format!("{name}: ambiguous redirect")),
                    };
                }
                // Pre-check readonly so `set_scalar_checked` (which reports its
                // own error) can't double-print with the caller's report.
                let target = self.resolve_ref_name(name);
                if self.readonly.contains(&target) {
                    return Err(format!("{target}: readonly variable"));
                }
                let n = self.alloc_varfd(reserved);
                reserved.push(n);
                let ok = self.set_scalar_checked(name, n.to_string());
                debug_assert!(ok, "readonly pre-checked");
                Ok(n)
            }
            None => Ok(r.fd),
        }
    }

    /// Apply a single redirect to the shell's *persistent* fd table (the same
    /// bindings `exec >file` uses), returning `Err(msg)` on failure. Shared by
    /// [`Self::apply_exec_redirects`] and the persistent varfd path in
    /// [`Self::resolve_redirects`]. `fd` is the already-resolved descriptor
    /// (varfd-allocated or literal).
    fn apply_persistent_redirect(&mut self, r: &Redirect, fd: i32) -> Result<(), String> {
        match r.op {
            RedirectOp::Read => {
                let path = self.expand_to_string(&r.target);
                let bytes = std::fs::read(map_device_path(&path))
                    .map_err(|e| format!("{path}: {e}"))?;
                if fd == 0 {
                    self.exec_stdin = Some(RefCell::new(io::Cursor::new(bytes)));
                } else if fd >= 3 {
                    self.open_fds.insert(fd, RefCell::new(io::Cursor::new(bytes)));
                    self.open_write_fds.remove(&fd);
                }
            }
            RedirectOp::HereDoc | RedirectOp::HereStr => {
                let bytes = if matches!(r.op, RedirectOp::HereDoc) {
                    self.expand_double_quoted(&r.target.parts).into_bytes()
                } else {
                    let mut s = self.expand_to_string(&r.target);
                    s.push('\n');
                    s.into_bytes()
                };
                if fd == 0 {
                    self.exec_stdin = Some(RefCell::new(io::Cursor::new(bytes)));
                } else if fd >= 3 {
                    self.open_fds.insert(fd, RefCell::new(io::Cursor::new(bytes)));
                    self.open_write_fds.remove(&fd);
                }
            }
            RedirectOp::WriteBoth | RedirectOp::AppendBoth => {
                let target = self.expand_to_string(&r.target);
                let append = matches!(r.op, RedirectOp::AppendBoth);
                let f = open_out(&target, append).map_err(|e| format!("{target}: {e}"))?;
                // `&> file` = `> file 2>&1`: fd 1 and fd 2 share one handle.
                let a = std::sync::Arc::new(f);
                self.exec_stdout = Some(a.clone());
                self.exec_stderr = Some(a);
            }
            RedirectOp::Write | RedirectOp::Clobber | RedirectOp::Append => {
                let target = self.expand_to_string(&r.target);
                let append = matches!(r.op, RedirectOp::Append);
                if self.noclobber
                    && matches!(r.op, RedirectOp::Write)
                    && std::path::Path::new(map_device_path(&target))
                        .metadata()
                        .is_ok_and(|m| m.is_file())
                {
                    return Err(format!("{target}: cannot overwrite existing file"));
                }
                let f = open_out(&target, append).map_err(|e| format!("{target}: {e}"))?;
                let a = std::sync::Arc::new(f);
                match fd {
                    0 | 1 => self.exec_stdout = Some(a),
                    2 => self.exec_stderr = Some(a),
                    _ => {
                        self.open_write_fds.insert(fd, a);
                        self.open_fds.remove(&fd);
                    }
                }
            }
            RedirectOp::DupOut => {
                let target = self.expand_to_string(&r.target);
                if target == "-" {
                    // `N>&-` / `N<&-`: close the descriptor.
                    match fd {
                        1 => self.exec_stdout = None,
                        2 => self.exec_stderr = None,
                        _ => {
                            self.open_write_fds.remove(&fd);
                            self.open_fds.remove(&fd);
                        }
                    }
                } else if let Ok(n) = target.parse::<i32>() {
                    // `M>&N`: fd M becomes a dup of fd N's *current* sink.
                    let src = self
                        .exec_dup_source(n)
                        .map_err(|bad| format!("{bad}: Bad file descriptor"))?;
                    match fd {
                        1 => self.exec_stdout = src,
                        2 => self.exec_stderr = src,
                        _ => {
                            // A user-space write fd needs a concrete handle:
                            // reuse the source handle, or (when the source is a
                            // std fd still on the terminal) dup the terminal.
                            let handle = match src {
                                Some(h) => h,
                                None => std::sync::Arc::new(
                                    dup_std_handle(n == 1)
                                        .map_err(|e| format!("{fd}: {e}"))?,
                                ),
                            };
                            self.open_write_fds.insert(fd, handle);
                            self.open_fds.remove(&fd);
                        }
                    }
                } else if fd == 1 {
                    // `1>&$f` (non-numeric expansion): both streams to the file.
                    let f = open_out(&target, false).map_err(|e| format!("{target}: {e}"))?;
                    let a = std::sync::Arc::new(f);
                    self.exec_stdout = Some(a.clone());
                    self.exec_stderr = Some(a);
                } else {
                    return Err(format!("{target}: ambiguous redirect"));
                }
            }
            RedirectOp::DupIn => {
                let target = self.expand_to_string(&r.target);
                if target == "-" {
                    // `N<&-`: close the input descriptor.
                    if fd == 0 {
                        self.exec_stdin = None;
                    } else {
                        self.open_fds.remove(&fd);
                        self.open_write_fds.remove(&fd);
                    }
                } else if let Ok(n) = target.parse::<i32>() {
                    // `M<&N`: fd M becomes a dup of input fd N's *current* source.
                    // Our fd sources are byte cursors, so a dup is modelled by
                    // cloning the source cursor (data + offset) — an independent
                    // offset, the same approximation used when cloning
                    // `exec_stdin` into subshells.
                    let cloned = self.clone_input_fd(n)?;
                    if fd == 0 {
                        self.exec_stdin = Some(cloned);
                    } else {
                        self.open_fds.insert(fd, cloned);
                        self.open_write_fds.remove(&fd);
                    }
                } else {
                    return Err(format!("{target}: ambiguous redirect"));
                }
            }
        }
        Ok(())
    }

    /// Clone input fd `n`'s current byte cursor for an input dup (`M<&N`). fd 0
    /// resolves to `exec_stdin` (falling back to an empty stream), fds ≥ 3 to
    /// the `open_fds` table. An unbound descriptor is a "Bad file descriptor".
    fn clone_input_fd(&self, n: i32) -> Result<RefCell<io::Cursor<Vec<u8>>>, String> {
        let cur = if n == 0 {
            self.exec_stdin.as_ref()
        } else {
            self.open_fds.get(&n)
        };
        match cur {
            Some(c) => {
                let borrowed = c.borrow();
                let mut clone = io::Cursor::new(borrowed.get_ref().clone());
                clone.set_position(borrowed.position());
                Ok(RefCell::new(clone))
            }
            // fd 0 with no bound stdin: treat as an empty input stream so a dup
            // of it does not error (bash's stdin would be the terminal).
            None if n == 0 => Ok(RefCell::new(io::Cursor::new(Vec::new()))),
            None => Err(format!("{n}: Bad file descriptor")),
        }
    }

    fn resolve_redirects(&mut self, redirs: &[Redirect]) -> Result<RedirPlan, String> {
        let mut plan = RedirPlan::default();
        let mut reserved: Vec<i32> = Vec::new();
        for r in redirs {
            let fd = self.redir_effective_fd(r, &mut reserved)?;
            // A `{name}>file`-style varfd redirect (open form) binds a *new*
            // descriptor ≥ 10 that persists after the command — exactly like
            // `exec {name}>file` — so it is applied to the persistent fd table
            // here and left out of the transient `RedirPlan`. The close form
            // (`{name}>&-`) instead reuses `$name`'s current fd and is scoped
            // like a numeric `N>&-`, so it flows through the plan below.
            if r.varfd.is_some() && !redir_is_close(r) {
                self.apply_persistent_redirect(r, fd)?;
                continue;
            }
            match r.op {
                RedirectOp::Read => {
                    if fd == 0 {
                        plan.stdin = Some(self.expand_to_string(&r.target));
                        plan.stdin_data = None;
                    } else if fd >= 3 {
                        // `exec 3< file`: slurp the file now so a missing/unreadable
                        // path surfaces as an error at redirection time (bash also
                        // reports it then), then hand the bytes to `exec`.
                        let path = self.expand_to_string(&r.target);
                        match std::fs::read(map_device_path(&path)) {
                            Ok(bytes) => {
                                plan.extra_fds.push((fd, ExtraFdOp::InputBytes(bytes)));
                            }
                            Err(e) => return Err(format!("{path}: {e}")),
                        }
                    }
                }
                RedirectOp::HereDoc => {
                    if fd == 0 {
                        // Here-doc bodies expand like a double-quoted context:
                        // no tilde expansion, no field splitting, no globbing.
                        let body = self.expand_double_quoted(&r.target.parts);
                        plan.stdin = None;
                        plan.stdin_data = Some(body.into_bytes());
                    } else if fd >= 3 {
                        let body = self.expand_double_quoted(&r.target.parts);
                        plan.extra_fds
                            .push((fd, ExtraFdOp::InputBytes(body.into_bytes())));
                    }
                }
                RedirectOp::HereStr => {
                    if fd == 0 {
                        let mut s = self.expand_to_string(&r.target);
                        s.push('\n');
                        plan.stdin = None;
                        plan.stdin_data = Some(s.into_bytes());
                    } else if fd >= 3 {
                        let mut s = self.expand_to_string(&r.target);
                        s.push('\n');
                        plan.extra_fds
                            .push((fd, ExtraFdOp::InputBytes(s.into_bytes())));
                    }
                }
                RedirectOp::WriteBoth | RedirectOp::AppendBoth => {
                    // `&>file` / `&>>file` / `>&file` → both stdout and stderr to
                    // the file (bash: equivalent to `>file 2>&1`). noclobber does
                    // not apply to `&>` (bash treats it like `>|`).
                    let target = self.expand_to_string(&r.target);
                    let append = matches!(r.op, RedirectOp::AppendBoth);
                    plan.stdout = Some((target.clone(), append));
                    plan.stderr = Some((target, append));
                    // `&>` is `>file 2>&1`: fd 2 is a dup of fd 1, sharing one
                    // open file description (and offset) — writes interleave.
                    plan.stderr_shares_stdout = true;
                    // Both fds now target the file: clear every competing dup so
                    // this (later) redirect wins over earlier `2>&1`/`>&N` forms.
                    plan.stderr_to_stdout = false;
                    plan.stdout_to_stderr = false;
                    plan.stdout_to_fd = None;
                    plan.stderr_to_fd = None;
                }
                RedirectOp::Write | RedirectOp::Clobber | RedirectOp::Append => {
                    let target = self.expand_to_string(&r.target);
                    let append = matches!(r.op, RedirectOp::Append);
                    // With `set -C` (noclobber), a plain `>` refuses to truncate an
                    // existing regular file; `>|` (Clobber) and `>>` (Append)
                    // always proceed. Matches bash's noclobber semantics.
                    if self.noclobber
                        && matches!(r.op, RedirectOp::Write)
                        && std::path::Path::new(map_device_path(&target))
                            .metadata()
                            .is_ok_and(|m| m.is_file())
                    {
                        return Err(format!("{target}: cannot overwrite existing file"));
                    }
                    match fd {
                        2 => {
                            plan.stderr = Some((target, append));
                            // An explicit `2>file` is an *independent* open, even
                            // if it names the same path as `>file`: bash gives each
                            // its own offset, so the writes clobber (not share).
                            plan.stderr_shares_stdout = false;
                            // Later file redirect overrides any earlier stderr dup.
                            plan.stderr_to_stdout = false;
                            plan.stderr_to_fd = None;
                        }
                        // fd ≥ 3 (`exec 3> file`): a user-space write descriptor,
                        // not stdout. Only `exec` consumes it; on any other
                        // command it is a documented no-op (previously this fell
                        // into the stdout arm and wrongly redirected fd 1).
                        f if f >= 3 => plan
                            .extra_fds
                            .push((f, ExtraFdOp::OutputFile(target, append))),
                        _ => {
                            plan.stdout = Some((target, append));
                            plan.stdout_to_stderr = false;
                            plan.stdout_to_fd = None;
                        }
                    }
                }
                RedirectOp::DupOut => {
                    // `2>&1` → stderr follows stdout; `1>&2` → the reverse.
                    // When the followed fd already targets a file, copy that file
                    // target directly; otherwise flag the dup so the executor
                    // routes fd 2→fd 1 (or fd 1→fd 2) to the live sink (pipe,
                    // terminal, or capture), not just to a file path.
                    let target = self.expand_to_string(&r.target);
                    // `M>&word` / `M<&word`: after expansion, a dup target must
                    // be a descriptor number or `-` (close). A non-numeric
                    // expansion on fd 1 (`>&$f`, `1>&$f`, `1>&file`) means "both
                    // stdout and stderr to that file", exactly like `>&file`
                    // (which the parser already rewrote to `WriteBoth` for the
                    // no-explicit-fd literal form). On any other fd a non-numeric
                    // target is an ambiguous redirect, as bash reports.
                    if target != "-" && target.parse::<i32>().is_err() {
                        if fd == 1 {
                            plan.stdout = Some((target.clone(), false));
                            plan.stderr = Some((target, false));
                            // `>&file` is `>file 2>&1` — fd 2 dup's fd 1 (shared
                            // offset, interleaved writes).
                            plan.stderr_shares_stdout = true;
                            plan.stderr_to_stdout = false;
                            plan.stdout_to_stderr = false;
                            plan.stdout_to_fd = None;
                            plan.stderr_to_fd = None;
                        } else {
                            return Err(format!("{target}: ambiguous redirect"));
                        }
                    } else if fd == 2 && target == "1" {
                        // fd 2's destination is being (re)set: drop any earlier
                        // stderr file/fd target so this dup wins (last-writer).
                        plan.stderr_to_fd = None;
                        if plan.stdout.is_some() {
                            // `>file 2>&1`: fd 2 dup's fd 1's file — shared offset.
                            plan.stderr = plan.stdout.clone();
                            plan.stderr_shares_stdout = true;
                            plan.stderr_to_stdout = false;
                        } else {
                            plan.stderr = None;
                            plan.stderr_to_stdout = true;
                        }
                    } else if fd == 1 && target == "2" {
                        plan.stdout_to_fd = None;
                        if plan.stderr.is_some() {
                            // `2>file 1>&2`: fd 1 dup's fd 2's file — shared offset.
                            plan.stdout = plan.stderr.clone();
                            plan.stderr_shares_stdout = true;
                            plan.stdout_to_stderr = false;
                        } else {
                            plan.stdout = None;
                            plan.stdout_to_stderr = true;
                        }
                    } else if fd >= 3 && target == "-" {
                        // `exec 3<&-` / `exec 3>&-`: close descriptor 3.
                        plan.extra_fds.push((fd, ExtraFdOp::Close));
                    } else if fd >= 3 && (target == "1" || target == "2") {
                        // `exec 3>&1` / `exec 3>&2`: alias a user-space write
                        // descriptor to a standard fd. Consumed only by `exec`
                        // (and the scoped compound-command path), which snapshots
                        // fd 1 / fd 2's current sink into `open_write_fds[fd]`.
                        let n = if target == "1" { 1 } else { 2 };
                        plan.extra_fds.push((fd, ExtraFdOp::AliasStd(n)));
                    } else if let Ok(n) = target.parse::<i32>()
                        && n >= 3
                    {
                        // `M>&N` with N ≥ 3: duplicate fd M onto a user-space
                        // write descriptor (`echo … >&3`, `cmd 2>&3`). Routed to
                        // `Shell::open_write_fds[N]` by write_bytes / run_external.
                        if fd == 2 {
                            plan.stderr_to_fd = Some(n);
                            plan.stderr = None;
                            plan.stderr_to_stdout = false;
                        } else {
                            plan.stdout_to_fd = Some(n);
                            plan.stdout = None;
                            plan.stdout_to_stderr = false;
                        }
                    }
                }
                RedirectOp::DupIn => {
                    // `M<&N` — duplicate an *input* descriptor. `read <&3`,
                    // `cat <&$r`. The dup shares the source cursor's offset (see
                    // `stdin_from_fd`), matching bash. A `<&-` closes; a
                    // non-numeric expansion is an ambiguous redirect.
                    let target = self.expand_to_string(&r.target);
                    if target == "-" {
                        if fd >= 3 {
                            // `exec 3<&-`: close descriptor 3 (consumed by `exec`).
                            plan.extra_fds.push((fd, ExtraFdOp::Close));
                        }
                        // `0<&-` (close stdin) on a non-exec command is a rare
                        // corner not modelled here (documented limitation).
                    } else if let Ok(n) = target.parse::<i32>() {
                        if fd == 0 && n >= 3 {
                            // fd 0 reads from input descriptor N's shared cursor
                            // (a `read -u`/`exec 3<` byte fd) or, for a `coproc`
                            // read end, the live pipe. An unbound source fd fails
                            // the whole redirect (bash: "N: Bad file descriptor"),
                            // rather than silent EOF.
                            if !self.open_fds.contains_key(&n)
                                && !self.coproc_read_fds.contains_key(&n)
                            {
                                return Err(format!("{n}: Bad file descriptor"));
                            }
                            plan.stdin_from_fd = Some(n);
                            plan.stdin = None;
                            plan.stdin_data = None;
                        }
                        // `<&0` (and the rare `<&1`/`<&2`) leave fd 0 as the
                        // ambient stdin — a dup of fd 0 onto itself is a no-op, so
                        // the command reads from the inherited pipe/terminal/cursor
                        // unchanged. `exec 5<&3` (fd ≥ 3 input alias) is only
                        // meaningful for `exec`, which walks the raw redirects.
                    } else {
                        return Err(format!("{target}: ambiguous redirect"));
                    }
                }
            }
        }
        Ok(plan)
    }

    /// Apply a redirection-only `exec`'s redirects to the shell's *persistent*
    /// fd table, in strict left-to-right source order.
    ///
    /// Ordering matters: `exec 3>&1 1>&2 2>&3` must save fd 1 into fd 3, then
    /// point fd 1 at fd 2, then point fd 2 at the saved fd 3 — a stdout/stderr
    /// swap. The collapsed [`RedirPlan`] cannot express that (it buckets each
    /// effect into a fixed field and loses order), so `exec` bypasses the plan
    /// and walks the raw redirects here. Each `>&N`/`>&1`/`>&2`/`>&-` dup reads
    /// the *current* sink of its source fd (as already mutated by earlier
    /// redirects in the same `exec`), matching bash's dup-at-that-moment
    /// semantics. Returns the resulting status (1 if any redirect failed).
    fn apply_exec_redirects(&mut self, redirs: &[Redirect]) -> i32 {
        let mut rc = 0;
        let mut reserved: Vec<i32> = Vec::new();
        for r in redirs {
            // Resolve `{name}>…` varfd redirects: allocate a free fd ≥ 10 and
            // store its number in the named variable (or, for `{name}>&-`, read
            // it back). A readonly target aborts this redirect (like bash).
            let fd = match self.redir_effective_fd(r, &mut reserved) {
                Ok(n) => n,
                Err(e) => {
                    self.errln(&format!("{}{e}", self.err_prefix()));
                    rc = 1;
                    continue;
                }
            };
            if let Err(e) = self.apply_persistent_redirect(r, fd) {
                self.errln(&format!("{}{e}", self.err_prefix()));
                rc = 1;
            }
        }
        rc
    }

    /// Resolve the current write sink of source fd `n` for an `exec M>&N` dup:
    /// `Ok(Some(h))` = fd `n` currently writes to handle `h`; `Err(n)` = fd `n`
    /// (≥ 3) is not open (a `bad file descriptor`).
    ///
    /// When fd 1 / fd 2 is still on the terminal (`exec_stdout`/`exec_stderr` is
    /// `None`), this duplicates that *specific* real std fd rather than
    /// returning `None`. The distinction matters when the shell's real fd 1 and
    /// fd 2 differ (e.g. it was launched with `1>file`): `1>&2` must point fd 1
    /// at fd 2's actual sink, not at `None` (which write paths resolve to the
    /// real fd 1). `Ok(None)` is only produced if duplicating the real std fd
    /// fails (pathological — a closed std fd), in which case callers fall back
    /// to the real std fd. This is the swap-idiom fix (`exec 3>&1 1>&2 2>&3`).
    fn exec_dup_source(
        &self,
        n: i32,
    ) -> Result<Option<std::sync::Arc<std::fs::File>>, i32> {
        match n {
            1 | 2 => {
                let cur = if n == 1 { &self.exec_stdout } else { &self.exec_stderr };
                match cur {
                    Some(f) => Ok(Some(f.clone())),
                    None => match dup_std_handle(n == 1) {
                        Ok(f) => Ok(Some(std::sync::Arc::new(f))),
                        Err(_) => Ok(None),
                    },
                }
            }
            _ => match self.open_write_fds.get(&n) {
                Some(f) => Ok(Some(f.clone())),
                None => Err(n),
            },
        }
    }

    // ---- expansion ----------------------------------------------------------

    /// Expand a word, optionally field-splitting the results of unquoted
    /// expansions. Returns zero or more fields.
    fn expand_word(&mut self, word: &Word, split: bool) -> Vec<String> {
        if split {
            // Command-argument context: field-split unquoted expansions, then
            // apply pathname (glob) expansion to each resulting field.
            let fields = self.expand_word_annotated(word);
            if self.noglob {
                // `set -f`: pathname expansion is disabled; each field keeps its
                // literal (quote-removed) text without glob matching.
                return fields.iter().map(|f| f.iter().map(|e| e.c).collect()).collect();
            }
            let nullglob = self.shopt.get("nullglob").copied().unwrap_or(false);
            let failglob = self.shopt.get("failglob").copied().unwrap_or(false);
            let dotglob = self.shopt.get("dotglob").copied().unwrap_or(false);
            let nocaseglob = self.shopt.get("nocaseglob").copied().unwrap_or(false);
            let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
            let globstar = self.shopt.get("globstar").copied().unwrap_or(false);
            // GLOBIGNORE: a `:`-separated list of patterns. When set to a
            // non-empty value, bash (a) removes any glob-generated name that
            // matches one of the patterns and (b) enables a dotglob-like effect
            // so leading-`.` names are matched (`.` and `..` stay excluded). The
            // patterns match the whole generated pathname with pathname-style
            // semantics (`*`/`?`/`[]` do not cross `/`). See the bash manual.
            let globignore_val = self.vars.get("GLOBIGNORE").filter(|v| !v.is_empty());
            let globignore_active = globignore_val.is_some();
            let globignore: Vec<GlobIgnorePat> = globignore_val
                .map(|v| build_globignore(v, extglob))
                .unwrap_or_default();
            let mut out = Vec::new();
            let mut failed = None;
            for f in fields {
                glob_or_literal(
                    &f, &mut out, nullglob, failglob, dotglob, nocaseglob, extglob, globstar,
                    globignore_active, &globignore, &mut failed,
                );
            }
            // `failglob`: a pattern that matched nothing is a fatal expansion
            // error. Record it for the simple-command driver, which reports it
            // and aborts the command list (like a non-interactive bash).
            if let Some(pat) = failed {
                self.glob_error = Some(pat);
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
        // `open` == the current field `cur` holds/began real content and must be
        // emitted at the end of the word. Field-splitting in the `other` arm may
        // clear it after a delimiter so a trailing IFS run leaves no empty field.
        let mut open = false;
        for (idx, part) in word.parts.iter().enumerate() {
            match part {
                WordPart::Literal(s) => {
                    let s = if idx == 0 {
                        self.tilde_expand(s)
                    } else {
                        s.clone()
                    };
                    push_chars(&mut cur, &s, false);
                    open = true;
                }
                WordPart::SingleQuoted(s) => {
                    push_chars(&mut cur, s, true);
                    open = true;
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
                        // `"${a[@]:off:len}"` / `"${@:off:len}"` — one field per
                        // sliced element (the `[*]`/`$*` star form joins instead,
                        // handled by the scalar fallback below).
                        [
                            WordPart::ArraySlice {
                                name,
                                star: false,
                                offset,
                                length,
                            },
                        ] => Some(self.slice_elements(name, offset, length)),
                        // `"${a[@]#pat}"` / `"${@^^}"` — one field per element
                        // after the element-wise transform.
                        [
                            WordPart::ArrayBulk {
                                name,
                                star: false,
                                op,
                            },
                        ] => Some(self.bulk_elements(name, op)),
                        // `"${a[@]:-word}"` / `"${a[@]:+word}"` — one field per
                        // element when active; the operand word (as a single
                        // field) otherwise. The `[*]` star form joins instead and
                        // falls through to the scalar path below.
                        [
                            WordPart::ArrayOp {
                                name,
                                star: false,
                                op,
                                colon,
                                arg,
                            },
                        ] => Some(self.array_op_fields(name, false, *op, *colon, arg)),
                        // `"${!ref}"` where ref resolves to `name[@]` yields one
                        // field per element (bash), like `"${name[@]}"`.
                        [WordPart::Indirect(refname)] => self.indirect_array_elems(refname),
                        // `"$@"` expands to one field per positional parameter,
                        // preserving embedded whitespace (`"$*"` joins instead and
                        // is handled by the scalar fallback below).
                        [WordPart::Param(p)] if p == "@" => Some(self.positional.clone()),
                        _ => None,
                    };
                    if let Some(items) = per_element {
                        for (i, el) in items.into_iter().enumerate() {
                            if i > 0 {
                                fields.push(std::mem::take(&mut cur));
                            }
                            push_chars(&mut cur, &el, true);
                            open = true;
                        }
                    } else {
                        let s = self.expand_double_quoted(parts);
                        push_chars(&mut cur, &s, true);
                        open = true;
                    }
                }
                other => {
                    let val = self.expand_dynamic(other);
                    // Field-split the unquoted expansion against IFS while keeping
                    // the *boundary* delimiters relative to adjacent literal/quoted
                    // text. Only the characters produced by this expansion are split
                    // candidates, so the scan integrates directly with the
                    // in-progress field `cur` (splitting each expansion in
                    // isolation would discard the leading/trailing information
                    // needed to break against neighbouring parts): a
                    // leading IFS-whitespace run with a preceding open field closes
                    // that field, an all-whitespace value collapses to a single
                    // break, a non-whitespace IFS char always delimits (yielding an
                    // empty field when nothing precedes it), and interior IFS runs
                    // split as usual. Empty expansions contribute nothing.
                    let ifs = self
                        .vars
                        .get("IFS")
                        .cloned()
                        .unwrap_or_else(|| " \t\n".to_string());
                    let is_ws = |c: char| matches!(c, ' ' | '\t' | '\n') && ifs.contains(c);
                    let is_nonws = |c: char| !matches!(c, ' ' | '\t' | '\n') && ifs.contains(c);
                    let cv: Vec<char> = val.chars().collect();
                    let n = cv.len();
                    let mut i = 0;
                    while i < n {
                        let c = cv[i];
                        if is_ws(c) {
                            let had_open = open;
                            while i < n && is_ws(cv[i]) {
                                i += 1;
                            }
                            if had_open {
                                // The whitespace run closes the preceding field; an
                                // immediately following non-whitespace IFS char is
                                // absorbed into the same delimiter (`ws* nonws ws*` is
                                // one delimiter *between* fields).
                                fields.push(std::mem::take(&mut cur));
                                open = false;
                                if i < n && is_nonws(cv[i]) {
                                    i += 1;
                                    while i < n && is_ws(cv[i]) {
                                        i += 1;
                                    }
                                }
                            }
                            // With no preceding field, leading IFS whitespace is
                            // simply trimmed; a following non-whitespace delimiter is
                            // handled below as a standalone delimiter (empty field).
                        } else if is_nonws(c) {
                            fields.push(std::mem::take(&mut cur));
                            open = false;
                            i += 1;
                            while i < n && is_ws(cv[i]) {
                                i += 1;
                            }
                        } else {
                            cur.push(EChar { c, quoted: false });
                            open = true;
                            i += 1;
                        }
                    }
                }
            }
        }
        if open {
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

    /// Expand an assignment RHS to a single string. Like `expand_to_string`, but
    /// tilde expansion additionally applies to a tilde-prefix that immediately
    /// follows an unquoted `:` (bash's assignment-context rule, so that
    /// `PATH=~/a:~/b` and `x=$PATH:~/bin` expand every `~`). Only unquoted
    /// literal text is scanned for colon-delimited tilde positions; a `:`
    /// produced by a parameter/command expansion does not create one.
    fn expand_assignment_value(&mut self, word: &Word) -> String {
        let mut cur = String::new();
        // The very start of the value is a tilde position.
        let mut at_tilde_pos = true;
        for part in &word.parts {
            match part {
                WordPart::Literal(s) => {
                    for (i, seg) in s.split(':').enumerate() {
                        if i > 0 {
                            cur.push(':');
                        }
                        // The first segment inherits the running tilde position;
                        // every later segment follows a literal `:`, so it is one.
                        if i > 0 || at_tilde_pos {
                            cur.push_str(&self.tilde_expand(seg));
                        } else {
                            cur.push_str(seg);
                        }
                    }
                    at_tilde_pos = s.ends_with(':');
                }
                WordPart::SingleQuoted(t) => {
                    cur.push_str(t);
                    at_tilde_pos = false;
                }
                WordPart::DoubleQuoted(parts) => {
                    cur.push_str(&self.expand_double_quoted(parts));
                    at_tilde_pos = false;
                }
                other => {
                    cur.push_str(&self.expand_dynamic(other));
                    at_tilde_pos = false;
                }
            }
        }
        cur
    }

    /// Expand an assignment-form word (`NAME=value`, `NAME+=value`, or
    /// `NAME[idx]=value`) as passed to a declaration builtin (`export`,
    /// `declare`/`typeset`, `local`, `readonly`). The `NAME=`/`NAME+=` prefix is
    /// emitted literally; the value part is expanded in *assignment context* —
    /// no word splitting, no pathname (glob) expansion, and tilde-expanded after
    /// an unquoted `:` or at the start of the value, exactly like a bare
    /// `NAME=value` assignment (bash treats declaration-builtin operands as
    /// assignments). Returns the single resulting argv string.
    fn expand_decl_assignment(&mut self, word: &Word) -> String {
        let mut out = String::new();
        let mut at_tilde_pos = true;
        for (idx, part) in word.parts.iter().enumerate() {
            match part {
                WordPart::Literal(s) => {
                    // On the first part, split off the `NAME=`/`NAME+=` prefix
                    // (including any `[subscript]`) and emit it verbatim; the
                    // value begins right after the `=`.
                    let value_str: &str = if idx == 0 {
                        match s.find('=') {
                            Some(eq) => {
                                out.push_str(&s[..=eq]);
                                &s[eq + 1..]
                            }
                            // No `=` in the first literal (value came from a later
                            // part, e.g. `X=$y`): treat the whole literal as value.
                            None => s.as_str(),
                        }
                    } else {
                        s.as_str()
                    };
                    for (i, seg) in value_str.split(':').enumerate() {
                        if i > 0 {
                            out.push(':');
                        }
                        if i > 0 || at_tilde_pos {
                            out.push_str(&self.tilde_expand(seg));
                        } else {
                            out.push_str(seg);
                        }
                    }
                    at_tilde_pos = value_str.ends_with(':');
                }
                WordPart::SingleQuoted(t) => {
                    out.push_str(t);
                    at_tilde_pos = false;
                }
                WordPart::DoubleQuoted(parts) => {
                    out.push_str(&self.expand_double_quoted(parts));
                    at_tilde_pos = false;
                }
                other => {
                    out.push_str(&self.expand_dynamic(other));
                    at_tilde_pos = false;
                }
            }
        }
        out
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
            // `${#@}` / `${#*}` are the *count* of positional parameters, not the
            // length of their joined string.
            WordPart::Length(name) if name == "@" || name == "*" => {
                self.positional.len().to_string()
            }
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
                colon,
                arg,
            } => self.expand_param_op(name, index, *op, *colon, arg),
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
                // `${x:off:len}` — a malformed offset/length is fatal (bash).
                let off = self.eval_arith_index(offset);
                let len = length.as_ref().map(|l| self.eval_arith_index(l));
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
                mode,
                all,
                pattern,
            } => {
                let value = self.param_elem_value(name, index).unwrap_or_default();
                let pat: Vec<char> = self.expand_to_string(pattern).chars().collect();
                let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
                param_case(&value, &pat, *mode, *all, extglob)
            }
            WordPart::ParamTransform { name, index, op } => {
                self.param_transform(name, index, *op)
            }
            WordPart::ArraySlice {
                name,
                offset,
                length,
                ..
            } => self.slice_elements(name, offset, length).join(" "),
            WordPart::ArrayBulk { name, op, .. } => self.bulk_elements(name, op).join(" "),
            WordPart::ArrayOp {
                name,
                star,
                op,
                colon,
                arg,
            } => {
                let fields = self.array_op_fields(name, *star, *op, *colon, arg);
                if *star {
                    fields.join(&self.star_sep())
                } else {
                    fields.join(" ")
                }
            }
            WordPart::CommandSub(prog) => self.command_sub(prog),
            WordPart::ProcSub { input, body } => self.proc_sub(*input, body),
            WordPart::ArithSub(expr) => self.arith_sub(expr),
            WordPart::ArrayRef {
                name,
                index,
                length,
            } => self.expand_array_ref(name, index, *length),
            WordPart::ArrayKeys { name, .. } => self.array_keys(name).join(" "),
            WordPart::Indirect(refname) => self.expand_indirect(refname),
            WordPart::IndirectOp { refname, target } => {
                // `${!ref<op>}`: resolve the target variable *name* (the value of
                // `ref`, or the nameref chain's endpoint), then apply the modifier
                // to that variable. Mirrors `expand_indirect`'s error handling for
                // an unset pointer / invalid target name.
                let tname = if self.nameref_attr.contains(refname) {
                    self.resolve_ref_name(refname)
                } else {
                    match self.param_value(refname) {
                        Some(t) => t,
                        None => {
                            self.emit_stderr(
                                format!("{}{refname}: invalid indirect expansion\n", self.err_prefix()).as_bytes(),
                            );
                            // bash aborts a bad indirect expansion with status 1.
                            self.unbound_error = Some(1);
                            return String::new();
                        }
                    }
                };
                if !is_valid_indirect_target(&tname) {
                    self.emit_stderr(format!("{}{tname}: invalid variable name\n", self.err_prefix()).as_bytes());
                    // bash aborts an invalid indirect target with status 1.
                    self.unbound_error = Some(1);
                    return String::new();
                }
                let renamed = rename_param_target(target, &tname);
                self.expand_dynamic(&renamed)
            }
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
        colon: bool,
        arg: &Word,
    ) -> String {
        let cur = self.param_elem_value(name, index);
        // Bash: the colon forms (`:-`, `:=`, `:+`, `:?`) treat an empty value the
        // same as unset ("active" only when set AND non-empty). The colon-less
        // forms distinguish empty-but-set from genuinely unset ("active" whenever
        // the parameter is set, even if empty).
        let is_active = if colon {
            cur.as_ref().is_some_and(|v| !v.is_empty())
        } else {
            cur.is_some()
        };
        match op {
            ParamOp::UseDefault => {
                if is_active {
                    cur.unwrap_or_default()
                } else {
                    self.expand_to_string(arg)
                }
            }
            ParamOp::AssignDefault => {
                if is_active {
                    cur.unwrap_or_default()
                } else {
                    let v = self.expand_to_string(arg);
                    self.assign_elem(name, index, v.clone());
                    v
                }
            }
            ParamOp::UseAlternate => {
                if is_active {
                    self.expand_to_string(arg)
                } else {
                    String::new()
                }
            }
            ParamOp::ErrorIfUnset => {
                if is_active {
                    cur.unwrap_or_default()
                } else {
                    let msg = self.expand_to_string(arg);
                    // bash's default diagnostic distinguishes the two forms: the
                    // colon form (`:?`) tests null-or-unset ("parameter null or
                    // not set"); the colon-less form (`?`) tests only unset
                    // ("parameter not set").
                    let text = if !msg.is_empty() {
                        &msg
                    } else if colon {
                        "parameter null or not set"
                    } else {
                        "parameter not set"
                    };
                    // bash renders the parameter name with its subscript exactly
                    // as written in source (`${a[$i]?}` → `a[$i]`, unexpanded).
                    let disp = crate::unparse::name_sub(name, index);
                    self.emit_stderr(format!("{}{disp}: {text}\n", self.err_prefix()).as_bytes());
                    // bash: `${var:?word}` on an unset/null parameter writes the
                    // message and, in a non-interactive shell, exits with status
                    // 127. Reuse the nounset abort path so the simple-command
                    // driver terminates the (sub)shell before running the command.
                    self.unbound_error = Some(127);
                    String::new()
                }
            }
        }
    }

    /// Evaluate a whole-array use/alternate/error operator
    /// (`${a[@]:-word}` / `${a[*]:+word}` / `${a[@]:?msg}`) to its result fields.
    /// Bash treats `[@]`/`[*]` like `$@`: when the array is "active" the elements
    /// are the result (one field each); otherwise the operand `word` is used.
    ///
    /// "Active" for the colon forms means the array is non-null — it has at least
    /// one non-empty element; for the colon-less forms it means the array is
    /// merely *set* (exists with at least one element), matching bash's
    /// unset-vs-null distinction.
    fn array_op_fields(
        &mut self,
        name: &str,
        star: bool,
        op: ParamOp,
        colon: bool,
        arg: &Word,
    ) -> Vec<String> {
        let resolved = self.resolve_ref_name(name);
        let elements = self.array_elements(name);
        let exists = self.arrays.contains_key(&resolved)
            || self.assoc.contains_key(&resolved)
            || self.vars.contains_key(&resolved);
        let is_active = if colon {
            // Colon forms test for "null": bash joins the elements with the first
            // `$IFS` char (as `${a[*]}` would) and treats an empty result as null.
            // So `a=("")` is null (`""`), but `a=("" "")` is not (`" "`).
            !elements.join(&self.star_sep()).is_empty()
        } else {
            // Colon-less forms test only for "unset": an array with at least one
            // element is set; an empty/undefined array counts as unset.
            exists && !elements.is_empty()
        };
        match op {
            ParamOp::UseDefault => {
                if is_active {
                    elements
                } else {
                    vec![self.expand_to_string(arg)]
                }
            }
            ParamOp::UseAlternate => {
                if is_active {
                    vec![self.expand_to_string(arg)]
                } else {
                    Vec::new()
                }
            }
            ParamOp::AssignDefault => {
                if is_active {
                    // A non-null array is returned unchanged (no assignment
                    // needed), exactly like `:-` on an active array.
                    elements
                } else {
                    // Assigning the default would require writing to `a[@]`/`a[*]`,
                    // which bash rejects as a "bad array subscript". Report the
                    // same and abort the expansion.
                    let sub = if star { "*" } else { "@" };
                    self.emit_stderr(
                        format!("{}{name}[{sub}]: bad array subscript\n", self.err_prefix()).as_bytes(),
                    );
                    // bash aborts a bad array subscript with status 1 (not 127).
                    self.unbound_error = Some(1);
                    Vec::new()
                }
            }
            ParamOp::ErrorIfUnset => {
                if is_active {
                    elements
                } else {
                    let sub = if star { "*" } else { "@" };
                    let msg = self.expand_to_string(arg);
                    // Match bash's colon (null-or-unset) vs colon-less (unset)
                    // default-message distinction — see `expand_param_op`.
                    let text = if !msg.is_empty() {
                        &msg
                    } else if colon {
                        "parameter null or not set"
                    } else {
                        "parameter not set"
                    };
                    self.emit_stderr(format!("{}{name}[{sub}]: {text}\n", self.err_prefix()).as_bytes());
                    // `${a[@]:?}` on an unset/null array exits 127, like scalar `:?`.
                    self.unbound_error = Some(127);
                    Vec::new()
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
            // bash aborts a non-interactive shell with status 127 on a nounset
            // unset-variable reference.
            self.unbound_error = Some(127);
            self.emit_stderr(format!("{}{name}: unbound variable\n", self.err_prefix()).as_bytes());
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
    /// Resolve a variable name through any nameref chain (`declare -n`),
    /// returning the final target name. A nameref's value is the name it points
    /// to; following stops at the first non-nameref name (or an unset/empty
    /// target), and a depth guard prevents an infinite loop on a cycle. Only the
    /// bare-name portion is followed — a target that names an array element
    /// (`ref=arr[0]`) is returned as-is for the caller's subscript logic.
    fn resolve_ref_name(&self, name: &str) -> String {
        let mut cur = name.to_string();
        // A short bound: real nameref chains are tiny; this only guards cycles.
        for _ in 0..64 {
            if !self.nameref_attr.contains(&cur) {
                return cur;
            }
            match self.vars.get(&cur) {
                Some(target) if !target.is_empty() && target != &cur => cur = target.clone(),
                _ => return cur,
            }
        }
        cur
    }

    /// If `name` is a nameref whose target is an array element (`arr[0]` /
    /// `m[key]`), return the referenced element's value. `None` when `name` is
    /// not such a nameref (the caller then falls through to normal resolution).
    fn nameref_elem_value(&self, name: &str) -> Option<String> {
        if !self.nameref_attr.contains(name) {
            return None;
        }
        let target = self.resolve_ref_name(name);
        let open = target.find('[')?;
        let inner = target.strip_suffix(']')?;
        let base = &target[..open];
        let sub = &inner[open + 1..];
        if self.assoc.contains_key(base) {
            return self.assoc_element(base, sub);
        }
        // A literal integer subscript (the common `arr[0]` case). Non-numeric
        // subscripts on an indexed array fall back to index 0, as bash does.
        let idx = sub.parse::<i64>().unwrap_or(0);
        self.array_element(base, idx)
    }

    /// Build the value of `$-`: the currently-enabled single-letter shell
    /// option flags. We report the flags for the options we actually model,
    /// plus `h` (hashall) and `B` (brace expansion) which are always on here.
    /// `-o`-only options without a letter (e.g. `pipefail`) are not included,
    /// matching bash. Order follows bash's fixed flag-table ordering.
    fn option_flags(&self) -> String {
        let mut s = String::new();
        // (letter, enabled) in bash's canonical relative order.
        let flags: [(char, bool); 11] = [
            ('a', self.allexport),
            ('e', self.errexit),
            ('f', self.noglob),
            ('h', true),
            ('n', self.noexec),
            ('u', self.nounset),
            ('x', self.xtrace),
            ('B', true),
            ('C', self.noclobber),
            // bash's `shell_flags[]` orders `E` (errtrace) and `T` (functrace)
            // after `C`: the table is …,B,C,E,H,P,T, so `E` precedes `T` (with
            // the unmodeled `H`/`P` between them).
            ('E', self.errtrace),
            ('T', self.functrace),
        ];
        for (c, on) in flags {
            if on {
                s.push(c);
            }
        }
        // Bash appends `c` (invoked via `-c`) last, after every `set`-toggled
        // option letter, e.g. `hBc`, `ehBc`, `hBCc`.
        if self.command_mode {
            s.push('c');
        }
        s
    }

    /// The separator that `"$*"` (and `"${a[*]}"`) uses to join elements: the
    /// first character of `$IFS`. An unset `IFS` joins with a space; an empty
    /// `IFS` joins with nothing (bash).
    fn star_sep(&self) -> String {
        match self.vars.get("IFS") {
            None => " ".to_string(),
            Some(ifs) => ifs.chars().next().map_or(String::new(), |c| c.to_string()),
        }
    }

    fn param_value(&self, name: &str) -> Option<String> {
        if let Some(v) = self.nameref_elem_value(name) {
            return Some(v);
        }
        let name = &self.resolve_ref_name(name);
        match name.as_str() {
            "?" => Some(self.last_status.to_string()),
            "#" => Some(self.positional.len().to_string()),
            "$" => Some(self.pid.to_string()),
            "!" => self.last_bg_pid.map(|p| p.to_string()),
            // `$@` in a single-string context joins with a space; `$*` joins
            // with the first character of `$IFS` (unset ⇒ space, empty ⇒ none).
            "@" => Some(self.positional.join(" ")),
            "*" => Some(self.positional.join(&self.star_sep())),
            "0" => Some(self.name.clone()),
            "-" => Some(self.option_flags()),
            "BASHPID" => Some(self.pid.to_string()),
            "BASH_SUBSHELL" => Some(self.subshell_depth.to_string()),
            "LINENO" => Some(self.current_line.to_string()),
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
                // Once the environment is imported, `vars` is authoritative —
                // no `std::env` fallback, so `unset NAME` on an inherited env
                // variable actually hides it. Tests (no import) keep the
                // fallback so `$HOME`/`$PATH` still resolve host-independently.
                if let Some(v) = self.vars.get(name) {
                    return Some(v.clone());
                }
                if self.env_imported {
                    None
                } else {
                    std::env::var(name).ok()
                }
            }
        }
    }

    fn command_sub(&mut self, prog: &Program) -> String {
        // Count every command substitution so callers (e.g. pure assignments)
        // can tell whether a `$(...)` ran while expanding a value.
        self.comsub_count = self.comsub_count.wrapping_add(1);
        // bash fast path: a command substitution whose body is solely an input
        // redirection — `$(< file)` — reads and substitutes the file's contents
        // (equivalent to, but faster than, `$(cat file)`). Detect a single
        // simple command with no assignments, no words, and a fd-0 read redirect,
        // then slurp the file directly.
        if let Some(path) = self.comsub_read_file(prog) {
            let mut s = String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()).into_owned();
            while s.ends_with('\n') {
                s.pop();
            }
            return s;
        }
        // Command substitution runs in its own subshell (bash semantics): a
        // clone of the shell state so variable/cwd/function mutations made
        // inside `$(...)` do not leak into the parent. Only the captured stdout
        // and the exit status ($?) propagate back.
        let mut buf = Vec::new();
        let mut sub = self.clone_for_subshell();
        // A command substitution does not run the caller's DEBUG/RETURN/ERR
        // traps for its internal commands. `clone_for_subshell` would otherwise
        // propagate them under `functrace`/`errtrace`, but bash's behaviour for
        // these pseudo-signal traps inside `$( … )` is quirky and inconsistent
        // (it captures the trap's own output into the result and fires on the
        // sub's overall status rather than per-command). Rather than replicate
        // that ill-defined behaviour, drop the (non-ignored) trace traps here so
        // the substitution only propagates its captured stdout and exit status.
        for k in ["DEBUG", "RETURN", "ERR"] {
            if sub.traps.get(k).is_some_and(|v| !v.is_empty()) {
                sub.traps.remove(k);
            }
        }
        {
            let mut out = Out::Capture(&mut buf);
            let _ = sub.exec_program(prog, &mut out, &StdinSrc::Inherit);
            // A command substitution is a subshell: fire its own EXIT trap into
            // the same capture so its output is included in the result (bash).
            sub.run_exit_trap_out(&mut out, &StdinSrc::Inherit);
        }
        self.last_status = sub.last_status;
        let mut s = String::from_utf8_lossy(&buf).into_owned();
        // Strip trailing newlines, as command substitution does.
        while s.ends_with('\n') {
            s.pop();
        }
        s
    }

    /// If `prog` is exactly a null command with an input redirection
    /// (`$(< file)`), return the expanded filename to read; otherwise `None`.
    fn comsub_read_file(&mut self, prog: &Program) -> Option<String> {
        if prog.items.len() != 1 {
            return None;
        }
        let item = &prog.items[0];
        if item.background || !item.list.rest.is_empty() {
            return None;
        }
        let pipe = &item.list.first;
        if pipe.negated || pipe.commands.len() != 1 {
            return None;
        }
        let Command::Simple(sc) = &pipe.commands[0] else {
            return None;
        };
        if !sc.assignments.is_empty() || !sc.words.is_empty() || !sc.decl_arrays.is_empty() {
            return None;
        }
        // Use the last fd-0 read redirect (bash opens them in order; the last wins).
        let target = sc
            .redirects
            .iter()
            .rev()
            .find(|r| r.op == RedirectOp::Read && r.fd == 0)?;
        Some(self.expand_to_string(&target.target))
    }

    /// Expand a process substitution `<(cmd)` / `>(cmd)` to a temp-file pathname.
    ///
    /// The host has no `/dev/fd` or named-pipe support, so this uses a temp-file
    /// approximation (as several shells do on such systems): for the input form
    /// `<(cmd)` the command runs *now*, its stdout captured into a temp file whose
    /// path is substituted; for the output form `>(cmd)` an empty temp file is
    /// created and the command is deferred (run after the enclosing command, with
    /// the file's contents as its stdin). Both temp files are cleaned up when the
    /// enclosing command finishes (see [`Shell::finish_procsubs`]). This is not
    /// streaming — a `<(tail -f)`-style infinite producer would block here — which
    /// is a documented limitation (see known-issues TD-OILS22).
    fn proc_sub(&mut self, input: bool, body: &Program) -> String {
        let path = unique_temp_path("osh_psub");
        if input {
            let mut buf = Vec::new();
            {
                let mut out = Out::Capture(&mut buf);
                let _ = self.exec_program(body, &mut out, &StdinSrc::Inherit);
            }
            if std::fs::write(&path, &buf).is_ok() {
                self.procsub_in_temps.push(path.clone());
            }
        } else {
            // Create the (empty) target so the enclosing command can open it.
            let _ = std::fs::write(&path, b"");
            self.procsub_out_jobs.push((path.clone(), body.clone()));
        }
        path
    }

    /// Tear down the process substitutions created since the recorded marks: run
    /// each deferred *output* body with its temp file's contents as stdin, then
    /// delete every output and input temp file. Called by the `exec_simple`
    /// wrapper after the command (and its whole body, for functions) finishes.
    fn finish_procsubs(&mut self, in_mark: usize, out_mark: usize) {
        if self.procsub_out_jobs.len() > out_mark {
            let jobs: Vec<(String, Program)> = self.procsub_out_jobs.split_off(out_mark);
            for (path, body) in jobs {
                if let Ok(bytes) = std::fs::read(&path) {
                    let cursor = RefCell::new(io::Cursor::new(bytes));
                    let sin = StdinSrc::Cursor(&cursor);
                    let mut out = Out::Inherit;
                    let _ = self.exec_program(&body, &mut out, &sin);
                }
                let _ = std::fs::remove_file(&path);
            }
        }
        if self.procsub_in_temps.len() > in_mark {
            for path in self.procsub_in_temps.split_off(in_mark) {
                let _ = std::fs::remove_file(&path);
            }
        }
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

    /// Like [`eval_arith_word`], but an arithmetic *evaluation error* is **fatal**
    /// — the bash behavior for an arithmetic subscript on an indexed array
    /// (`${a[3 x]}`, `a[1/0]=v`) and for substring offset/length arithmetic
    /// (`${x:1 z}`, `${x:2:1 z}`). The diagnostic is printed (honoring an active
    /// stderr redirect) and `arith_error` is set, which the simple-command driver
    /// / bare-assignment path turns into a `Flow::Exit(1)` (status 1 at the main
    /// shell, or in a subshell without aborting the parent). A bare-identifier
    /// subscript (`${a[abc]}`) is a normal arithmetic variable reference resolving
    /// to 0, not an error. Non-fatal numeric-comparison contexts (`[[ a -eq b ]]`)
    /// deliberately keep using the tolerant `eval_arith_word`, matching bash.
    fn eval_arith_index(&mut self, w: &Word) -> i64 {
        let s = self.expand_to_string(w);
        let s = s.trim();
        if s.is_empty() {
            return 0;
        }
        match arith::eval(s, self) {
            Ok(v) => v,
            Err(e) => {
                self.emit_arith_error(s, &e);
                self.arith_error = true;
                0
            }
        }
    }

    fn arith_sub(&mut self, expr: &str) -> String {
        // Expand `$name` / `${name}` parameters inside the expression first;
        // bare identifiers are resolved by the evaluator via `VarLookup`.
        let expanded = self.expand_arith_params(expr);
        match arith::eval(&expanded, self) {
            Ok(v) => v.to_string(),
            Err(e) => {
                // Route through `emit_arith_error` so an active `2>`/`2>&1`
                // redirect on the command silences the diagnostic, matching bash.
                self.emit_arith_error(&expanded, &e);
                // An arithmetic error in a `$(( … ))` word/value substitution
                // makes the whole simple command abort (bash) rather than run
                // with a fabricated value; the driver consumes this flag.
                self.arith_error = true;
                "0".to_string()
            }
        }
    }

    /// Expand `$name`, `${name}`, `$1`, command substitutions `$(cmd)` /
    /// `` `cmd` ``, and nested arithmetic `$((expr))` inside an arithmetic
    /// string, substituting each with its (numeric or textual) value. Bare
    /// identifiers (no `$`) are left for the arithmetic evaluator to resolve via
    /// `VarLookup`. Command substitutions and nested arithmetic must be expanded
    /// here (before evaluation) so `$(( $(f) + $((n-1)) ))` works.
    fn expand_arith_params(&mut self, expr: &str) -> String {
        let chars: Vec<char> = expr.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '`' {
                // Backtick command substitution: consume up to the next backtick.
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && chars[j] != '`' {
                    j += 1;
                }
                let inner: String = chars[start..j].iter().collect();
                i = if j < chars.len() { j + 1 } else { j };
                out.push_str(self.run_command_sub_text(&inner).trim());
                continue;
            }
            if chars[i] != '$' {
                out.push(chars[i]);
                i += 1;
                continue;
            }
            // `chars[i] == '$'`
            match chars.get(i + 1) {
                Some('(') if chars.get(i + 2) == Some(&'(') => {
                    // `$((expr))` — nested arithmetic. Find the matching `))`.
                    if let Some((inner, next)) = Self::scan_arith_sub(&chars, i + 3) {
                        let val = self.arith_sub(&inner);
                        out.push_str(if val.trim().is_empty() { "0" } else { val.trim() });
                        i = next;
                        continue;
                    }
                    // Unbalanced: fall through and emit the literal `$`.
                    out.push('$');
                    i += 1;
                }
                Some('(') => {
                    // `$(cmd)` — command substitution. Find the matching `)`.
                    if let Some((inner, next)) = Self::scan_paren_group(&chars, i + 2) {
                        out.push_str(self.run_command_sub_text(&inner).trim());
                        i = next;
                        continue;
                    }
                    out.push('$');
                    i += 1;
                }
                Some('{') => {
                    // `${…}` — a full parameter expansion (length `${#x}`, array
                    // subscript `${a[i]}`, operators `${x:-y}`, indirection
                    // `${!x}`, transforms `${x@Q}`, …). Scan the balanced brace
                    // group (so nested `${…}` inside a default value survive),
                    // then run it through the real parameter-expansion parser and
                    // expander rather than the limited `param_value`, which only
                    // knows bare names. This is what makes `$(( ${#a[@]} ))` and
                    // `$(( ${x:-3} ))` evaluate like bash.
                    i += 2;
                    let start = i;
                    let mut depth = 0usize;
                    while i < chars.len() {
                        match chars[i] {
                            '{' => depth += 1,
                            '}' if depth == 0 => break,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                    let inner: String = chars[start..i].iter().collect();
                    if i < chars.len() {
                        i += 1; // consume the closing '}'
                    }
                    let val = match crate::parser::parse_braced_param(&inner) {
                        Ok(part) => {
                            let word = Word { parts: vec![part] };
                            self.expand_to_string(&word)
                        }
                        // Fall back to the bare-name lookup for anything the
                        // param parser rejects (keeps prior behaviour for the
                        // simple cases it can't reach).
                        Err(_) => self.param_value(&inner).unwrap_or_default(),
                    };
                    let val = val.trim();
                    out.push_str(if val.is_empty() { "0" } else { val });
                }
                _ => {
                    i += 1;
                    let mut n = String::new();
                    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                        n.push(chars[i]);
                        i += 1;
                    }
                    let val = self.param_value(&n).unwrap_or_default();
                    let val = val.trim();
                    out.push_str(if val.is_empty() { "0" } else { val });
                }
            }
        }
        out
    }

    /// Parse `text` as a shell program and capture its stdout (a command
    /// substitution body embedded in an arithmetic expression), reusing the
    /// normal `command_sub` path (trailing-newline stripping, `$(<file)` fast
    /// path). An unparseable body yields an empty string.
    fn run_command_sub_text(&mut self, text: &str) -> String {
        match crate::parser::parse(text) {
            Ok(prog) => self.command_sub(&prog),
            Err(_) => String::new(),
        }
    }

    /// From `chars[start..]` (just past an opening `(`), return the balanced
    /// group's inner text and the index just past its matching `)`.
    fn scan_paren_group(chars: &[char], start: usize) -> Option<(String, usize)> {
        let mut depth = 0usize;
        let mut i = start;
        while i < chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' if depth == 0 => {
                    return Some((chars[start..i].iter().collect(), i + 1));
                }
                ')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// From `chars[start..]` (just past the `$((`), return the arithmetic
    /// expression text and the index just past its matching `))`.
    fn scan_arith_sub(chars: &[char], start: usize) -> Option<(String, usize)> {
        let mut depth = 0usize;
        let mut i = start;
        while i < chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' if depth == 0 => {
                    if chars.get(i + 1) == Some(&')') {
                        return Some((chars[start..i].iter().collect(), i + 2));
                    }
                    return None;
                }
                ')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        None
    }

    fn tilde_expand(&self, s: &str) -> String {
        let Some(after) = s.strip_prefix('~') else {
            return s.to_string();
        };
        // The tilde-prefix runs from just after `~` to the first `/` (or end);
        // the remainder (including any leading `/`) is appended verbatim.
        let (prefix, rest) = match after.find('/') {
            Some(i) => (&after[..i], &after[i..]),
            None => (after, ""),
        };
        // Resolve the prefix to a directory. An unrecognised prefix (e.g. a
        // `~user` we cannot resolve — no user database on this target) leaves the
        // word untouched, matching bash's "no expansion if lookup fails" rule.
        let dir: Option<String> = if prefix.is_empty() {
            self.param_value("HOME")
        } else if prefix == "+" {
            self.param_value("PWD")
                .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()))
        } else if prefix == "-" {
            self.param_value("OLDPWD")
        } else if let Some(n) = parse_dirstack_index(prefix) {
            // `~N` / `~+N` count from the left (0 = current dir); `~-N` from the
            // right of the directory stack.
            let full = self.dir_stack_full();
            let len = full.len();
            match n {
                DirStackRef::FromLeft(k) => full.get(k).cloned(),
                DirStackRef::FromRight(k) => len
                    .checked_sub(1)
                    .and_then(|last| last.checked_sub(k))
                    .and_then(|i| full.get(i).cloned()),
            }
        } else {
            None
        };
        match dir {
            Some(d) => format!("{d}{rest}"),
            None => s.to_string(),
        }
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

        // Install any fd ≥ 3 dups/opens created by this command's own redirects
        // (`echo hi 3>&1 2>&3`) before resolving the `2>&N` / `>&N` duplications
        // below, so a later redirect can see a descriptor an earlier one made.
        // `exec` is exempt — it applies redirects to the *persistent* fd table
        // itself (further down), and must not have them torn down here.
        let builtin_saved_fds = if name != "exec" && !redir.extra_fds.is_empty() {
            self.install_extra_fds(&redir.extra_fds, out)
        } else {
            Vec::new()
        };

        // Scoped stderr redirect: a simple-command builtin honors its own
        // `2> file`/`2>> file`/`2>&1`/`2>&N` by pushing a StderrTarget for the
        // builtin's duration, so diagnostics and `>&2` output land in the right
        // sink (bash). Compound commands install their group-level stderr
        // separately (`exec_redirected`); `exec` manages redirects itself (it
        // sets the *persistent* `exec_stderr`), so it is exempt.
        //
        // NOTE: our `RedirPlan` is order-free, so the rare `>&2 2>file`
        // combination routes `>&2` to the file (the `2>file >&2` ordering)
        // rather than the pre-redirect stderr — see known-issues TD-OILS14.
        let mut pushed_stderr = false;
        let mut stderr_merge_buf: Option<Arc<Mutex<Vec<u8>>>> = None;
        if name != "exec" {
            if let Some((path, append)) = &redir.stderr {
                if let Ok(f) = open_out(path, *append) {
                    self.stderr_stack.push(StderrTarget::File(Arc::new(f)));
                    pushed_stderr = true;
                }
            } else if let Some(n) = redir.stderr_to_fd {
                if let Some(f) = self.open_write_fds.get(&n) {
                    self.stderr_stack.push(StderrTarget::WriteFd(Arc::clone(f)));
                    pushed_stderr = true;
                } else {
                    self.errln(&format!("{}{n}: Bad file descriptor", self.err_prefix()));
                }
            } else if redir.stderr_to_stdout {
                // `2>&1` with fd 1 not a file: fd 2 mirrors fd 1's live sink.
                match out {
                    Out::Capture(_) => {
                        let buf = Arc::new(Mutex::new(Vec::new()));
                        self.stderr_stack.push(StderrTarget::Buffer(Arc::clone(&buf)));
                        stderr_merge_buf = Some(buf);
                        pushed_stderr = true;
                    }
                    Out::Pipe(w) => {
                        if let Ok(wp) = w.try_clone() {
                            self.stderr_stack.push(StderrTarget::Pipe(Arc::new(wp)));
                            pushed_stderr = true;
                        }
                    }
                    Out::Inherit => {
                        self.stderr_stack.push(StderrTarget::Stdout);
                        pushed_stderr = true;
                    }
                }
            }
        }

        let mut flow = Flow::Next;
        let args = &argv[1..];
        // bash tags an arithmetic error with the running builtin's name
        // (`this_command_name`): `let`/`(( ))`/`declare`/`typeset`/`local`
        // arithmetic reports `<name>: line N: <builtin>: <expr>: …`. Set it for
        // the builtin's duration so `eval_int_assign` picks up the right tag.
        let saved_arith_cmd = self.arith_cmd;
        self.arith_cmd = match name {
            "let" => Some("let"),
            "declare" => Some("declare"),
            "typeset" => Some("typeset"),
            "local" => Some("local"),
            _ => None,
        };
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
            "export" => self.builtin_export(args, out, redir),
            "declare" | "typeset" => {
                let lead: String =
                    args.iter().take_while(|a| a.starts_with('-')).flat_map(|a| a.chars()).collect();
                // `declare -ft name` / `+ft name` toggles a function's trace
                // attribute (so it inherits DEBUG/RETURN traps); it is a
                // mutation, not a listing, and accepts a `+` sign.
                if let Some((enable, start)) = Self::func_trace_op(args) {
                    self.set_func_trace(enable, &args[start..])
                } else if lead.contains('F') || lead.contains('f') {
                    // `declare -F`/`-f` operate on functions (name listing).
                    self.declare_functions(args, lead.contains('F'), out, redir)
                } else if lead.contains('p') {
                    self.declare_print(args, out, redir)
                } else {
                    // `declare -A` / `-a` / `-i` / `-x` / `-r` / `-n` / `-l` /
                    // `-u` with NO name operands is a *listing* filtered by those
                    // attributes (bash), not a declaration. With names, or with no
                    // attribute flags at all, fall through to declare/assign.
                    let start = Self::declare_flag_end(args);
                    let has_names = args.get(start).is_some();
                    let has_attr = args[..start].iter().any(|a| {
                        a.chars().any(|c| matches!(c, 'A' | 'a' | 'i' | 'x' | 'r' | 'n' | 'l' | 'u'))
                    });
                    if !has_names && has_attr {
                        self.declare_list_filtered(args, out, redir)
                    } else {
                        self.builtin_declare(args, false)
                    }
                }
            }
            "local" => self.builtin_declare(args, true),
            "readonly" => self.builtin_readonly(args, out, redir),
            "shopt" => self.builtin_shopt(args, out, redir),
            "unset" => self.builtin_unset(args),
            "set" => self.builtin_set(args, out, redir),
            "shift" => self.builtin_shift(args),
            "getopts" => self.builtin_getopts(args),
            "mapfile" | "readarray" => self.builtin_mapfile(args, stdin, redir, out),
            "read" => self.builtin_read(args, stdin, redir),
            "test" | "[" => self.builtin_test(name, args),
            "let" => self.builtin_let(args),
            "eval" => {
                let joined = args.join(" ");
                self.run_source(&joined)
            }
            "source" | "." => self.builtin_source(args),
            "type" => self.builtin_type(args, out, redir),
            "compgen" => self.builtin_compgen(args, out, redir),
            "complete" => self.builtin_complete(args, out, redir),
            "compopt" => self.builtin_compopt(args),
            "trap" => self.builtin_trap(args, out, redir),
            "jobs" => self.builtin_jobs(args, out, redir),
            "wait" => self.builtin_wait(args),
            "disown" => self.builtin_disown(args),
            "fg" => self.builtin_fg(args, out, redir),
            "bg" => self.builtin_bg(args, out, redir),
            "caller" => self.builtin_caller(args, out, redir),
            "times" => self.builtin_times(out, redir),
            "enable" => self.builtin_enable(args, out, redir),
            "alias" => self.builtin_alias(args, out, redir),
            "unalias" => self.builtin_unalias(args),
            "help" => self.builtin_help(args, out, redir),
            "hash" => self.builtin_hash(args, out, redir),
            "umask" => self.builtin_umask(args, out, redir),
            "ulimit" => self.builtin_ulimit(args, out, redir),
            "exec" => {
                if args.is_empty() {
                    // Redirection-only `exec`: rebind the shell's own fds for
                    // every subsequent command. We model fd 1 / fd 2 file
                    // targets (`exec > log 2>&1` etc) and fd 0 input redirects
                    // (`exec < file`, `exec << EOF`).
                    let mut rc = 0;
                    // fd 0 (`< file` / here-doc): read the source fully into a
                    // position-tracking cursor so subsequent `read`s / externals
                    // consume successive input.
                    if let Some(data) = &redir.stdin_data {
                        self.exec_stdin = Some(RefCell::new(io::Cursor::new(data.clone())));
                    } else if let Some(path) = &redir.stdin {
                        match std::fs::read(map_device_path(path)) {
                            Ok(bytes) => {
                                self.exec_stdin = Some(RefCell::new(io::Cursor::new(bytes)));
                            }
                            Err(e) => {
                                self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                                rc = 1;
                            }
                        }
                    }
                    // fd 1 (`> file` / `>> file`): open the file once and keep
                    // the shared handle, so later commands accumulate into it at
                    // one OS offset (bash dups the fd, it does not reopen).
                    if let Some((path, append)) = &redir.stdout {
                        match open_out(path, *append) {
                            Ok(f) => {
                                self.exec_stdout = Some(std::sync::Arc::new(f));
                            }
                            Err(e) => {
                                self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                                rc = 1;
                            }
                        }
                    }
                    // fd 2 (`2> file` / `2>> file`).
                    if rc == 0 && let Some((path, append)) = &redir.stderr {
                        match open_out(path, *append) {
                            Ok(f) => {
                                self.exec_stderr = Some(std::sync::Arc::new(f));
                            }
                            Err(e) => {
                                self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                                rc = 1;
                            }
                        }
                    }
                    // `2>&1` with fd 1 not a file: fd 2 mirrors the fd 1 target
                    // (shares the same `Arc<File>` — a true dup, one offset).
                    if rc == 0 && redir.stderr_to_stdout {
                        self.exec_stderr = self.exec_stdout.clone();
                    }
                    // `1>&2` with fd 2 not a file: fd 1 mirrors the fd 2 target.
                    if rc == 0 && redir.stdout_to_stderr {
                        self.exec_stdout = self.exec_stderr.clone();
                    }
                    // Restore `exec 1>&N` / `exec 2>&N` (N ≥ 3): rebind ambient
                    // fd 1 / fd 2 to a user-space write descriptor's live handle
                    // (typically one saved earlier by `exec N>&1`). An unopened
                    // fd is a status-1 `N: Bad file descriptor`, as in bash.
                    if rc == 0 && let Some(n) = redir.stdout_to_fd {
                        match self.open_write_fds.get(&n) {
                            Some(f) => self.exec_stdout = Some(std::sync::Arc::clone(f)),
                            None => {
                                self.errln(&format!("{}{n}: Bad file descriptor", self.err_prefix()));
                                rc = 1;
                            }
                        }
                    }
                    if rc == 0 && let Some(n) = redir.stderr_to_fd {
                        match self.open_write_fds.get(&n) {
                            Some(f) => self.exec_stderr = Some(std::sync::Arc::clone(f)),
                            None => {
                                self.errln(&format!("{}{n}: Bad file descriptor", self.err_prefix()));
                                rc = 1;
                            }
                        }
                    }
                    // fd ≥ 3 input descriptors (`exec 3< file`, `exec 3<&-`):
                    // install / remove entries in the persistent open-fd table so
                    // `read -u N` can consume them.
                    if rc == 0 {
                        for (fd, op) in &redir.extra_fds {
                            match op {
                                ExtraFdOp::InputBytes(bytes) => {
                                    self.open_fds.insert(
                                        *fd,
                                        RefCell::new(io::Cursor::new(bytes.clone())),
                                    );
                                    // A descriptor is input xor output; drop any
                                    // prior write binding for the same number.
                                    self.open_write_fds.remove(fd);
                                }
                                ExtraFdOp::OutputFile(path, append) => {
                                    match open_out(path, *append) {
                                        Ok(f) => {
                                            self.open_write_fds
                                                .insert(*fd, std::sync::Arc::new(f));
                                            self.open_fds.remove(fd);
                                        }
                                        Err(e) => {
                                            self.errln(&format!("{}{path}: {e}", self.err_prefix()));
                                            rc = 1;
                                        }
                                    }
                                }
                                ExtraFdOp::AliasStd(n) => match self.snapshot_std_fd(*n) {
                                    Ok(f) => {
                                        self.open_write_fds
                                            .insert(*fd, std::sync::Arc::new(f));
                                        self.open_fds.remove(fd);
                                    }
                                    Err(e) => {
                                        self.errln(&format!("{}{fd}: {e}", self.err_prefix()));
                                        rc = 1;
                                    }
                                },
                                ExtraFdOp::Close => {
                                    self.open_fds.remove(fd);
                                    self.open_write_fds.remove(fd);
                                }
                            }
                        }
                    }
                    rc
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
                // `return` is only valid inside a function (`fn_stack` is
                // non-empty — inherited by subshells too) or a sourced script.
                // Elsewhere bash reports an error, yields status 2, and does
                // NOT unwind, so execution continues with the next command.
                if self.fn_stack.is_empty() && self.source_depth == 0 {
                    self.errln(
                        &format!("{}return: can only `return' from a function or sourced script", self.err_prefix()),
                    );
                    2
                } else {
                    let code = args
                        .first()
                        .and_then(|s| s.parse::<i32>().ok())
                        .unwrap_or(self.last_status);
                    flow = Flow::Return;
                    code
                }
            }
            "break" => {
                // Outside any loop, `break` is a no-op: bash warns to stderr,
                // returns status 0, and continues executing the next command
                // rather than unwinding.
                if self.loop_depth == 0 {
                    self.errln(&format!("{}break: only meaningful in a `for', `while', or `until' loop", self.err_prefix()));
                } else {
                    let n = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                    flow = Flow::Break(n.max(1));
                }
                0
            }
            "continue" => {
                if self.loop_depth == 0 {
                    self.errln(
                        &format!("{}continue: only meaningful in a `for', `while', or `until' loop", self.err_prefix()),
                    );
                } else {
                    let n = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                    flow = Flow::Continue(n.max(1));
                }
                0
            }
            _ => {
                self.errln(&format!("{}{name}: not a builtin", self.err_prefix()));
                127
            }
        };
        self.arith_cmd = saved_arith_cmd;

        // Tear down the scoped stderr redirect and, for the `2>&1`-into-captured-
        // stdout case, fold the buffered stderr into fd 1's sink after the
        // builtin's own stdout (line-level interleaving is not preserved — see
        // the module limitations).
        if pushed_stderr {
            self.stderr_stack.pop();
        }
        if let Some(buf) = stderr_merge_buf
            && let Ok(g) = buf.lock()
            && !g.is_empty()
        {
            let bytes = g.clone();
            drop(g);
            // A default plan writes straight to `out` (the fd 1 sink).
            self.write_bytes(out, &RedirPlan::default(), &bytes);
        }

        // Tear down the scoped fd ≥ 3 descriptors installed for this builtin.
        self.restore_extra_fds(builtin_saved_fds);

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
        // A fatal arithmetic error while binding an integer-attribute value
        // (`declare -i k="3 apples"`, `local -i n=1/0`, `declare -ia a=(x)`) is
        // fatal in bash — the shell aborts with status 1 — unlike `let`/`(( ))`,
        // which only return non-zero. The declare/local builtin sets
        // `arith_error` via `eval_int_assign`; honour it here exactly as the
        // simple-command driver does for a bare `k=$((…))` assignment. In a
        // subshell this `Flow::Exit(1)` yields status 1 without aborting the
        // parent (see `fatal_abort_status`'s companion subshell boundary).
        if self.arith_error {
            self.arith_error = false;
            self.last_status = 1;
            return Flow::Exit(1);
        }
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
        // Leading `-L`/`-P` select logical (default) vs physical (symlink-
        // resolved) handling. `-` is a target (`$OLDPWD`), not a flag.
        let mut physical = false;
        let mut i = 0;
        while let Some(a) = args.get(i) {
            match a.as_str() {
                "-L" => physical = false,
                "-P" => physical = true,
                "--" => {
                    i += 1;
                    break;
                }
                _ => break,
            }
            i += 1;
        }
        let rest = &args[i..];

        // `cd -` returns to `$OLDPWD` and echoes the new directory (bash).
        let is_dash = rest.first().map(String::as_str) == Some("-");
        let (mut target, mut echo) = match rest.first().map(String::as_str) {
            None => (
                self.param_value("HOME").unwrap_or_else(|| "/".to_string()),
                false,
            ),
            Some("-") => match self.param_value("OLDPWD") {
                Some(p) => (p, true),
                None => {
                    self.emit_stderr(format!("{}cd: OLDPWD not set\n", self.err_prefix()).as_bytes());
                    return 1;
                }
            },
            Some(p) => (p.to_string(), false),
        };

        // `CDPATH` search: a non-explicit relative target is looked up under
        // each `CDPATH` entry; a match through a non-`.` entry echoes the
        // destination path (bash), like `cd -`.
        if !is_dash
            && !cd_is_explicit(&target)
            && let Some(cdpath) = self.param_value("CDPATH")
        {
            for entry in cdpath.split(':') {
                let base = if entry.is_empty() { "." } else { entry };
                let candidate = format!("{base}/{target}");
                if std::path::Path::new(&candidate).is_dir() {
                    if base != "." {
                        echo = true;
                    }
                    target = candidate;
                    break;
                }
            }
        }

        match self.change_dir(&target) {
            Ok(mut cwd) => {
                // `-P`: report/store the canonical (symlink-resolved) path.
                if physical
                    && let Ok(canon) = std::fs::canonicalize(&cwd)
                {
                    cwd = canon.to_string_lossy().into_owned();
                    self.vars.insert("PWD".to_string(), cwd.clone());
                }
                if echo {
                    println!("{cwd}");
                }
                0
            }
            Err(e) => {
                self.errln(&format!("{}cd: {target}: {e}", self.err_prefix()));
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
                    self.emit_stderr(format!("{}pushd: no other directory\n", self.err_prefix()).as_bytes());
                    return 1;
                }
                let top = self.dir_stack[0].clone();
                match self.change_dir(&top) {
                    Ok(_) => self.dir_stack[0] = cur,
                    Err(e) => {
                        self.emit_stderr(format!("{}pushd: {top}: {e}\n", self.err_prefix()).as_bytes());
                        return 1;
                    }
                }
            }
            Some(spec) if is_rot(spec) => {
                let full = self.dir_stack_full();
                let len = full.len();
                let n: usize = spec[1..].parse().unwrap_or(0);
                if n >= len {
                    self.emit_stderr(format!("{}pushd: directory stack index out of range\n", self.err_prefix()).as_bytes());
                    return 1;
                }
                let idx = if spec.starts_with('+') { n } else { len - 1 - n };
                let mut rotated: Vec<String> = full[idx..].to_vec();
                rotated.extend_from_slice(&full[..idx]);
                let newtop = rotated[0].clone();
                match self.change_dir(&newtop) {
                    Ok(_) => self.dir_stack = rotated[1..].to_vec(),
                    Err(e) => {
                        self.emit_stderr(format!("{}pushd: {newtop}: {e}\n", self.err_prefix()).as_bytes());
                        return 1;
                    }
                }
            }
            Some(dir) => match self.change_dir(dir) {
                Ok(_) => self.dir_stack.insert(0, cur),
                Err(e) => {
                    self.emit_stderr(format!("{}pushd: {dir}: {e}\n", self.err_prefix()).as_bytes());
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
                    self.emit_stderr(format!("{}popd: directory stack index out of range\n", self.err_prefix()).as_bytes());
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
                self.emit_stderr(format!("{}popd: invalid argument\n", self.err_prefix()).as_bytes());
                return 1;
            }
        }
        self.print_dirs_line(out, redir)
    }

    /// Pop the saved top of the directory stack and change to it (the common
    /// `popd` with no rotation argument). Errors if the stack is empty.
    fn popd_top(&mut self, out: &mut Out, redir: &RedirPlan) -> i32 {
        if self.dir_stack.is_empty() {
            self.emit_stderr(format!("{}popd: directory stack empty\n", self.err_prefix()).as_bytes());
            return 1;
        }
        let top = self.dir_stack.remove(0);
        if let Err(e) = self.change_dir(&top) {
            self.emit_stderr(format!("{}popd: {top}: {e}\n", self.err_prefix()).as_bytes());
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
                self.emit_stderr(format!("{}dirs: directory stack index out of range\n", self.err_prefix()).as_bytes());
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
            // A pure builtin usage message: bash's `builtin_usage()` prints
            // `<builtin>: usage: …` with no `<name>: line N:` shell prefix.
            self.emit_stderr(b"trap: usage: trap [-lp] [[arg] signal_spec ...]\n");
            return 2;
        }
        let reset = action == "-";
        let mut status = 0;
        for spec in specs {
            match normalize_sigspec(spec) {
                Some(norm) => {
                    // The body has explicitly touched this trap, so it is no
                    // longer a merely-inherited trap for the current function
                    // frame: clear any inheritance mask so it fires normally
                    // (and, once set, persists globally after return).
                    self.unsuppress_trap(&norm);
                    if reset {
                        self.traps.remove(&norm);
                    } else {
                        self.traps.insert(norm, action.clone());
                    }
                }
                None => {
                    self.emit_stderr(
                        format!("{}trap: {spec}: invalid signal specification\n", self.err_prefix()).as_bytes(),
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
            let name = sigspec_display(sig);
            buf.push_str(&format!("trap -- {quoted} {name}\n"));
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
    /// Resolve a `wait`/`jobs` operand (`%n` job spec, bare job id, or pid) to
    /// an index into `self.jobs`.
    fn resolve_job_spec(&self, spec: &str) -> Option<usize> {
        if let Some(rest) = spec.strip_prefix('%') {
            rest.parse::<usize>().ok().and_then(|n| self.jobs.iter().position(|j| j.id == n))
        } else if let Ok(n) = spec.parse::<u32>() {
            self.jobs
                .iter()
                .position(|j| j.pid == n)
                .or_else(|| self.jobs.iter().position(|j| j.id as u32 == n))
        } else {
            None
        }
    }

    fn builtin_wait(&mut self, args: &[String]) -> i32 {
        // Parse flags: `-n` (return as soon as the next job completes) and
        // `-p VAR` (store the pid of the job whose status is returned in VAR).
        let mut wait_any = false;
        let mut pid_var: Option<String> = None;
        let mut operands: Vec<String> = Vec::new();
        let mut i = 0;
        while let Some(a) = args.get(i) {
            if a == "--" {
                operands.extend_from_slice(&args[i + 1..]);
                break;
            }
            if a == "-n" {
                wait_any = true;
                i += 1;
                continue;
            }
            if a == "-p" {
                let Some(v) = args.get(i + 1) else {
                    self.emit_stderr(format!("{}wait: -p: option requires an argument\n", self.err_prefix()).as_bytes());
                    return 2;
                };
                pid_var = Some(v.clone());
                i += 2;
                continue;
            }
            if let Some(rest) = a.strip_prefix("-p")
                && !rest.is_empty()
            {
                pid_var = Some(rest.to_string());
                i += 1;
                continue;
            }
            // First non-flag token: the rest are operands.
            operands.extend_from_slice(&args[i..]);
            break;
        }

        if wait_any {
            return self.wait_next(&operands, pid_var.as_deref());
        }

        if operands.is_empty() {
            // Wait for all jobs, blocking on each.
            let mut last = 0;
            let mut last_pid = None;
            for job in &mut self.jobs {
                if let Some(mut child) = job.child.take() {
                    last = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
                    job.status = Some(last);
                } else if let Some(s) = job.status {
                    last = s;
                }
                last_pid = Some(job.pid);
            }
            self.jobs.clear();
            if let (Some(var), Some(pid)) = (pid_var, last_pid) {
                self.vars.insert(var, pid.to_string());
            }
            return last;
        }
        let mut last = 0;
        for spec in &operands {
            let Some(idx) = self.resolve_job_spec(spec) else {
                self.emit_stderr(format!("{}wait: {spec}: no such job\n", self.err_prefix()).as_bytes());
                last = 127;
                continue;
            };
            let pid = self.jobs[idx].pid;
            let job = &mut self.jobs[idx];
            if let Some(mut child) = job.child.take() {
                last = child.wait().ok().and_then(|s| s.code()).unwrap_or(1);
            } else {
                last = job.status.unwrap_or(0);
            }
            self.jobs.remove(idx);
            if let Some(var) = &pid_var {
                self.vars.insert(var.clone(), pid.to_string());
            }
        }
        last
    }

    /// `wait -n [ids…]` — block until the *next* job in the candidate set (the
    /// named ids, or all jobs when none are named) terminates, then return its
    /// status and forget it. Returns 127 when there are no candidate jobs.
    fn wait_next(&mut self, operands: &[String], pid_var: Option<&str>) -> i32 {
        // Build the candidate index set.
        let candidates: Vec<usize> = if operands.is_empty() {
            (0..self.jobs.len()).collect()
        } else {
            let mut v = Vec::new();
            for spec in operands {
                match self.resolve_job_spec(spec) {
                    Some(idx) => v.push(idx),
                    None => {
                        self.emit_stderr(format!("{}wait: {spec}: no such job\n", self.err_prefix()).as_bytes());
                    }
                }
            }
            v.sort_unstable();
            v.dedup();
            v
        };
        if candidates.is_empty() {
            return 127;
        }
        // Poll the candidates until one reports termination. A job already
        // reaped (child == None, status set) completes immediately.
        loop {
            for &idx in &candidates {
                let Some(job) = self.jobs.get_mut(idx) else {
                    continue;
                };
                let done = match &mut job.child {
                    Some(child) => match child.try_wait() {
                        Ok(Some(st)) => Some(st.code().unwrap_or(1)),
                        Ok(None) => None,
                        Err(_) => Some(1),
                    },
                    None => Some(job.status.unwrap_or(0)),
                };
                if let Some(status) = done {
                    let pid = job.pid;
                    self.jobs.remove(idx);
                    if let Some(var) = pid_var {
                        self.vars.insert(var.to_string(), pid.to_string());
                    }
                    return status;
                }
            }
            // No candidate ready yet — yield briefly before re-polling. This is
            // a deliberate short poll of live child processes (not a retry of a
            // failing command); OS-level wait-any across std Child handles is
            // not available, so a bounded poll is the correct approach.
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// `disown [-h] [-ar] [jobspec ...]` — remove jobs from the shell's job
    /// table (so they are no longer tracked by `jobs`/`wait`). With `-h` the
    /// jobs are kept but marked so they would not receive SIGHUP on shell exit.
    /// `-a` selects all jobs, `-r` only running (not-yet-finished) jobs. With no
    /// jobspec and neither `-a` nor `-r`, the current (most recently backgrounded)
    /// job is used.
    fn builtin_disown(&mut self, args: &[String]) -> i32 {
        self.poll_jobs();
        let mut mark_hup = false;
        let mut all = false;
        let mut running_only = false;
        let mut specs: Vec<String> = Vec::new();
        let mut i = 0;
        while let Some(a) = args.get(i) {
            if a == "--" {
                specs.extend_from_slice(&args[i + 1..]);
                break;
            }
            if let Some(flags) = a.strip_prefix('-')
                && !flags.is_empty()
                && flags.chars().all(|c| matches!(c, 'h' | 'a' | 'r'))
            {
                for c in flags.chars() {
                    match c {
                        'h' => mark_hup = true,
                        'a' => all = true,
                        'r' => running_only = true,
                        _ => {}
                    }
                }
                i += 1;
                continue;
            }
            specs.extend_from_slice(&args[i..]);
            break;
        }

        // Resolve the set of target job ids.
        let mut target_ids: Vec<usize> = Vec::new();
        if !specs.is_empty() {
            for spec in &specs {
                match self.resolve_job_spec(spec) {
                    Some(idx) => target_ids.push(self.jobs[idx].id),
                    None => {
                        self.emit_stderr(format!("{}disown: {spec}: no such job\n", self.err_prefix()).as_bytes());
                        return 1;
                    }
                }
            }
        } else if all {
            target_ids = self.jobs.iter().map(|j| j.id).collect();
        } else if running_only {
            target_ids = self.jobs.iter().filter(|j| j.status.is_none()).map(|j| j.id).collect();
        } else {
            // No spec: operate on the current (last) job.
            match self.jobs.last() {
                Some(j) => target_ids.push(j.id),
                None => {
                    self.emit_stderr(format!("{}disown: current: no such job\n", self.err_prefix()).as_bytes());
                    return 1;
                }
            }
        }

        if running_only {
            target_ids.retain(|id| {
                self.jobs.iter().find(|j| j.id == *id).is_some_and(|j| j.status.is_none())
            });
        }

        if mark_hup {
            for id in &target_ids {
                if let Some(j) = self.jobs.iter_mut().find(|j| j.id == *id) {
                    j.no_hup = true;
                }
            }
        } else {
            self.jobs.retain(|j| !target_ids.contains(&j.id));
        }
        0
    }

    /// `fg [jobspec]` — bring a background job into the foreground: print its
    /// command line (as bash does) and block until it terminates, returning its
    /// exit status. With no jobspec the current (most recently backgrounded) job
    /// is used. Returns 1 when there is no such job.
    ///
    /// Note: we have no job-control terminal (no SIGTSTP/SIGCONT, no controlling
    /// tty transfer — see known-issues TD-OILS13), so `fg` cannot resume a
    /// *stopped* job; it simply waits for a still-running background job. This is
    /// the meaningful subset of `fg` for a shell without terminal job control.
    fn builtin_fg(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        self.poll_jobs();
        // Skip a leading `--` separator; the first remaining operand is the spec.
        let spec = args.iter().find(|a| a.as_str() != "--");
        let idx = match spec {
            Some(s) => match self.resolve_job_spec(s) {
                Some(i) => i,
                None => {
                    self.emit_stderr(format!("{}fg: {s}: no such job\n", self.err_prefix()).as_bytes());
                    return 1;
                }
            },
            None => {
                if self.jobs.is_empty() {
                    self.emit_stderr(format!("{}fg: current: no such job\n", self.err_prefix()).as_bytes());
                    return 1;
                }
                self.jobs.len() - 1
            }
        };
        // Echo the command line to stdout, matching bash's `fg` behavior.
        let cmd = self.jobs[idx].cmd.clone();
        let _ = self.write_bytes(out, redir, format!("{cmd}\n").as_bytes());
        // Wait for the job to finish, then remove it from the table.
        let job = &mut self.jobs[idx];
        let status = if let Some(mut child) = job.child.take() {
            child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
        } else {
            job.status.unwrap_or(0)
        };
        self.jobs.remove(idx);
        status
    }

    /// `bg [jobspec ...]` — resume stopped jobs in the background. Because we
    /// have no terminal job control (no SIGTSTP/SIGCONT — see known-issues
    /// TD-OILS13), backgrounded jobs are already running; `bg` therefore reports
    /// each targeted job in bash's `[id] cmd &` form and returns 0. With no
    /// jobspec the current (most recently backgrounded) job is used. Returns 1
    /// when a named/current job does not exist.
    fn builtin_bg(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        self.poll_jobs();
        let specs: Vec<&String> = args.iter().filter(|a| a.as_str() != "--").collect();
        let idxs: Vec<usize> = if specs.is_empty() {
            if self.jobs.is_empty() {
                self.emit_stderr(format!("{}bg: current: no such job\n", self.err_prefix()).as_bytes());
                return 1;
            }
            vec![self.jobs.len() - 1]
        } else {
            let mut v = Vec::new();
            for s in specs {
                match self.resolve_job_spec(s) {
                    Some(i) => v.push(i),
                    None => {
                        self.emit_stderr(format!("{}bg: {s}: no such job\n", self.err_prefix()).as_bytes());
                        return 1;
                    }
                }
            }
            v
        };
        let mut buf = String::new();
        for &idx in &idxs {
            let job = &self.jobs[idx];
            buf.push_str(&format!("[{}] {} &\n", job.id, job.cmd));
        }
        self.write_bytes(out, redir, buf.as_bytes())
    }

    /// `caller [expr]` — report the context of an active subroutine call.
    /// Without `expr`, prints "LINE SOURCE" for the current function's call
    /// site. With a non-negative `expr`, prints "LINE FUNCNAME SOURCE" for that
    /// frame in the call stack (0 = the current function). Returns 1 when not
    /// executing a function or `expr` is out of range / non-numeric, matching
    /// bash. The stack mirrors the `FUNCNAME`/`BASH_LINENO`/`BASH_SOURCE`
    /// arrays: frame `i` names `fn_stack[len-1-i]`, called at
    /// `call_line_stack[len-1-i]`, in source `$0`.
    fn builtin_caller(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let depth = self.fn_stack.len();
        // Not inside any function → no call context.
        if depth == 0 {
            self.emit_stderr(format!("{}caller: no such frame\n", self.err_prefix()).as_bytes());
            return 1;
        }
        // bash prints the source label of the *caller's* frame — BASH_SOURCE[n+1]
        // for `caller n`, BASH_SOURCE[1] for bare `caller` — with the literal
        // `NULL` when that frame does not exist (e.g. the caller is the
        // top-level of a `-c`/interactive shell, which has no bottom frame).
        let spec = args.iter().find(|a| a.as_str() != "--");
        match spec {
            None => {
                // Bare `caller`: line of the current call site (BASH_LINENO[0])
                // and the source of its caller (BASH_SOURCE[1]). Unlike the
                // numbered form, this never fails while inside a function.
                let Some(line) = self.bash_lineno_at(0) else {
                    return 1;
                };
                let src = self.bash_source_at(1).unwrap_or_else(|| "NULL".to_string());
                self.write_bytes(out, redir, format!("{line} {src}\n").as_bytes())
            }
            Some(expr) => {
                let Ok(n) = expr.parse::<usize>() else {
                    self.emit_stderr(
                        format!("{}caller: {expr}: invalid number\n", self.err_prefix()).as_bytes(),
                    );
                    return 1;
                };
                // Frame n reports BASH_LINENO[n] + FUNCNAME[n+1] (the caller of
                // the function at depth n). Out of range when the caller frame
                // does not exist — e.g. `caller 0` from a single function under
                // `-c`, where there is no bottom `main` frame.
                let (Some(line), Some(func)) = (self.bash_lineno_at(n), self.funcname_at(n + 1))
                else {
                    return 1;
                };
                let src = self.bash_source_at(n + 1).unwrap_or_else(|| "NULL".to_string());
                self.write_bytes(out, redir, format!("{line} {func} {src}\n").as_bytes())
            }
        }
    }

    /// `times` — print the accumulated user and system CPU times for the shell
    /// and its children, one pair per line (shell first, then children), in
    /// bash's `%dm%d.%03ds` form. We have no per-process CPU accounting yet
    /// (see known-issues TD-OILS10), so the reported times are zero; the format
    /// and line structure match bash so scripts that parse the output still work.
    fn builtin_times(&mut self, out: &mut Out, redir: &RedirPlan) -> i32 {
        // Each pair is "user sys"; both lines currently report 0m0.000s.
        let zero = "0m0.000s";
        let text = format!("{zero} {zero}\n{zero} {zero}\n");
        self.write_bytes(out, redir, text.as_bytes())
    }

    /// `help [-dms] [pattern ...]` — display information about shell builtins.
    /// With no pattern, list every builtin's one-line synopsis. Each pattern is
    /// matched against builtin names as a shell glob (a bare name is an exact
    /// topic); `-s` prints only the usage synopsis, `-d` prints only the short
    /// description, and the default prints both. A pattern matching nothing is a
    /// status-1 error (`no help topics match`).
    fn builtin_help(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut short = false; // -s: synopsis only
        let mut desc_only = false; // -d: description only
        let mut patterns: Vec<String> = Vec::new();
        let mut i = 0;
        while let Some(a) = args.get(i) {
            if a == "--" {
                patterns.extend_from_slice(&args[i + 1..]);
                break;
            }
            if let Some(flags) = a.strip_prefix('-')
                && !flags.is_empty()
                && flags.chars().all(|c| matches!(c, 's' | 'd' | 'm'))
            {
                for c in flags.chars() {
                    match c {
                        's' => short = true,
                        'd' => desc_only = true,
                        // -m (man-page-like format) is accepted; we render the
                        // same content as the default long form.
                        'm' => {}
                        _ => {}
                    }
                }
                i += 1;
                continue;
            }
            patterns.push(a.clone());
            i += 1;
        }

        // No pattern: list every builtin's synopsis (sorted).
        if patterns.is_empty() {
            let mut names: Vec<&str> = HELP_TABLE.iter().map(|(n, _, _)| *n).collect();
            names.sort_unstable();
            let mut text = String::new();
            for n in names {
                if let Some((_, usage, _)) = HELP_TABLE.iter().find(|(hn, _, _)| hn == &n) {
                    text.push_str(usage);
                    text.push('\n');
                }
            }
            return self.write_bytes(out, redir, text.as_bytes());
        }

        // Each pattern: collect matching topics (glob against builtin names).
        let mut status = 0;
        let mut text = String::new();
        for pat in &patterns {
            let mut matched = false;
            let pat_chars: Vec<char> = pat.chars().collect();
            for (name, usage, description) in HELP_TABLE {
                let name_chars: Vec<char> = name.chars().collect();
                if *name == pat || glob_match(&pat_chars, &name_chars, false) {
                    matched = true;
                    if desc_only {
                        text.push_str(name);
                        text.push_str(" - ");
                        text.push_str(description);
                        text.push('\n');
                    } else if short {
                        // bash short form: "NAME: usage".
                        text.push_str(name);
                        text.push_str(": ");
                        text.push_str(usage);
                        text.push('\n');
                    } else {
                        // bash long form: "NAME: usage" line, then indented
                        // description.
                        text.push_str(name);
                        text.push_str(": ");
                        text.push_str(usage);
                        text.push('\n');
                        text.push_str("    ");
                        text.push_str(description);
                        text.push('\n');
                    }
                }
            }
            if !matched {
                self.emit_stderr(format!("{}help: no help topics match `{pat}'\n", self.err_prefix()).as_bytes());
                status = 1;
            }
        }
        if !text.is_empty() {
            self.write_bytes(out, redir, text.as_bytes());
        }
        status
    }

    /// True when `name` is a builtin that has not been disabled via `enable -n`.
    /// Command resolution consults this (rather than the bare `is_builtin`) so a
    /// disabled builtin falls through to a same-named external.
    fn builtin_enabled(&self, name: &str) -> bool {
        is_builtin(name) && !self.disabled_builtins.contains(name)
    }

    /// `enable [-a] [-n] [name ...]` — enable or disable shell builtins. With
    /// `name`s and no `-n`, re-enable them; with `-n`, disable them (so a
    /// same-named external runs instead). With no `name`s: `-a` lists every
    /// builtin with its state, `-n` lists only the disabled ones, and bare
    /// `enable` lists the enabled ones — all in re-inputtable `enable NAME` /
    /// `enable -n NAME` form. An unknown name is a status-1 error.
    fn builtin_enable(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut disable = false;
        let mut list_all = false;
        let mut names: Vec<String> = Vec::new();
        let mut i = 0;
        while let Some(a) = args.get(i) {
            if a == "--" {
                names.extend_from_slice(&args[i + 1..]);
                break;
            }
            if let Some(flags) = a.strip_prefix('-')
                && !flags.is_empty()
                && flags.chars().all(|c| matches!(c, 'n' | 'a'))
            {
                for c in flags.chars() {
                    match c {
                        'n' => disable = true,
                        'a' => list_all = true,
                        _ => {}
                    }
                }
                i += 1;
                continue;
            }
            names.extend_from_slice(&args[i..]);
            break;
        }

        if names.is_empty() {
            // Listing mode. Sort for deterministic output.
            let mut all: Vec<&str> = BUILTIN_NAMES.to_vec();
            all.sort_unstable();
            let mut buf = String::new();
            for name in all {
                let off = self.disabled_builtins.contains(name);
                if list_all {
                    if off {
                        buf.push_str(&format!("enable -n {name}\n"));
                    } else {
                        buf.push_str(&format!("enable {name}\n"));
                    }
                } else if disable && off {
                    buf.push_str(&format!("enable -n {name}\n"));
                } else if !disable && !off {
                    buf.push_str(&format!("enable {name}\n"));
                }
            }
            return self.write_bytes(out, redir, buf.as_bytes());
        }

        let mut status = 0;
        for name in &names {
            if !is_builtin(name) {
                self.emit_stderr(format!("{}enable: {name}: not a shell builtin\n", self.err_prefix()).as_bytes());
                status = 1;
                continue;
            }
            if disable {
                self.disabled_builtins.insert(name.clone());
            } else {
                self.disabled_builtins.remove(name);
            }
        }
        status
    }

    /// `alias [-p] [name[=value] ...]` — define, print, or list aliases. With no
    /// operands (or `-p`), print every alias in re-inputtable `alias NAME='VAL'`
    /// form. `name=value` defines an alias; a bare `name` prints that one alias
    /// (status 1 if undefined). Aliases are expanded over the token stream before
    /// parsing (see `parse_with_aliases`).
    fn builtin_alias(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // Consume leading `-p` / `--`; other operands begin the name list.
        let mut i = 0;
        while let Some(a) = args.get(i) {
            match a.as_str() {
                "-p" => i += 1,
                "--" => {
                    i += 1;
                    break;
                }
                _ => break,
            }
        }
        let operands = &args[i..];
        if operands.is_empty() {
            let mut buf = String::new();
            for (name, val) in &self.aliases {
                buf.push_str(&format!("alias {name}={}\n", single_quote(val)));
            }
            return self.write_bytes(out, redir, buf.as_bytes());
        }
        let mut status = 0;
        for op in operands {
            if let Some(eq) = op.find('=') {
                let name = &op[..eq];
                if name.is_empty() {
                    self.emit_stderr(
                        format!("{}alias: `{op}': invalid alias name\n", self.err_prefix()).as_bytes(),
                    );
                    status = 1;
                    continue;
                }
                self.aliases.insert(name.to_string(), op[eq + 1..].to_string());
            } else if let Some(val) = self.aliases.get(op).cloned() {
                let line = format!("alias {op}={}\n", single_quote(&val));
                self.write_bytes(out, redir, line.as_bytes());
            } else {
                self.emit_stderr(format!("{}alias: {op}: not found\n", self.err_prefix()).as_bytes());
                status = 1;
            }
        }
        status
    }

    /// `unalias [-a] name ...` — remove aliases. `-a` removes every alias; an
    /// unknown name is a status-1 error.
    fn builtin_unalias(&mut self, args: &[String]) -> i32 {
        if args.is_empty() {
            // Pure usage message — unprefixed (see `trap` usage above).
            self.emit_stderr(b"unalias: usage: unalias [-a] name [name ...]\n");
            return 2;
        }
        if args.iter().any(|a| a == "-a") {
            self.aliases.clear();
            return 0;
        }
        let mut status = 0;
        for name in args {
            if name == "--" {
                continue;
            }
            if self.aliases.remove(name).is_none() {
                self.emit_stderr(format!("{}unalias: {name}: not found\n", self.err_prefix()).as_bytes());
                status = 1;
            }
        }
        status
    }

    /// `hash [-lr] [-p pathname] [-dt] [name ...]` — manage the remembered
    /// command-path table. No operand prints the table; `-r` forgets all; `-d`
    /// forgets the named commands; `-t` prints the remembered path of each name;
    /// `-p pathname name` remembers `name` at `pathname` without a search; `-l`
    /// lists entries in re-inputtable form. Bare `name`s force a fresh `$PATH`
    /// search and remember the result. Unknown/unfound names are a status-1 error.
    fn builtin_hash(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut clear = false;
        let mut delete = false;
        let mut print_path = false;
        let mut list = false;
        let mut pathname: Option<String> = None;
        let mut i = 0;
        while let Some(arg) = args.get(i) {
            if arg == "--" {
                i += 1;
                break;
            }
            if arg == "-p" {
                pathname = args.get(i + 1).cloned();
                if pathname.is_none() {
                    self.emit_stderr(format!("{}hash: -p: option requires an argument\n", self.err_prefix()).as_bytes());
                    return 2;
                }
                i += 2;
                continue;
            }
            if let Some(flags) = arg.strip_prefix('-')
                && !flags.is_empty()
                && flags.chars().all(|c| matches!(c, 'r' | 'd' | 't' | 'l'))
            {
                for c in flags.chars() {
                    match c {
                        'r' => clear = true,
                        'd' => delete = true,
                        't' => print_path = true,
                        'l' => list = true,
                        _ => {}
                    }
                }
                i += 1;
                continue;
            }
            break;
        }
        let names = &args[i..];

        if clear {
            self.cmd_hash.clear();
            return 0;
        }
        if let Some(p) = pathname {
            // `-p pathname name`: remember without searching.
            let Some(name) = names.first() else {
                return 0;
            };
            self.cmd_hash
                .insert(name.clone(), (std::path::PathBuf::from(p), 0));
            return 0;
        }
        if list {
            let mut entries: Vec<(&String, &(std::path::PathBuf, u64))> =
                self.cmd_hash.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut s = String::new();
            for (name, (path, _)) in entries {
                s.push_str(&format!("builtin hash -p {} {name}\n", path.to_string_lossy()));
            }
            return self.write_bytes(out, redir, s.as_bytes());
        }
        if delete {
            let mut status = 0;
            for name in names {
                if self.cmd_hash.remove(name).is_none() {
                    self.emit_stderr(format!("{}hash: {name}: not found\n", self.err_prefix()).as_bytes());
                    status = 1;
                }
            }
            return status;
        }
        if print_path {
            let mut s = String::new();
            let mut status = 0;
            let multiple = names.len() > 1;
            for name in names {
                if let Some((path, _)) = self.cmd_hash.get(name) {
                    if multiple {
                        s.push_str(&format!("{name}\t{}\n", path.to_string_lossy()));
                    } else {
                        s.push_str(&format!("{}\n", path.to_string_lossy()));
                    }
                } else {
                    self.emit_stderr(format!("{}hash: {name}: not found\n", self.err_prefix()).as_bytes());
                    status = 1;
                }
            }
            let w = self.write_bytes(out, redir, s.as_bytes());
            return if w != 0 { w } else { status };
        }
        if names.is_empty() {
            // Print the table (nothing when empty, matching bash).
            if self.cmd_hash.is_empty() {
                return 0;
            }
            let mut entries: Vec<(&String, &(std::path::PathBuf, u64))> =
                self.cmd_hash.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut s = String::from("hits\tcommand\n");
            for (_, (path, hits)) in entries {
                s.push_str(&format!("{hits:>4}\t{}\n", path.to_string_lossy()));
            }
            return self.write_bytes(out, redir, s.as_bytes());
        }
        // Bare names: forget any old entry and force a fresh `$PATH` search.
        let mut status = 0;
        for name in names {
            self.cmd_hash.remove(name);
            match self.find_in_path(name) {
                Some(path) => {
                    self.cmd_hash.insert(name.clone(), (path, 0));
                }
                None => {
                    self.emit_stderr(format!("{}hash: {name}: not found\n", self.err_prefix()).as_bytes());
                    status = 1;
                }
            }
        }
        status
    }

    /// `umask [-S] [-p] [mode]` — get or set the file-creation mask. With no
    /// mode operand it prints the current mask (octal `0NNN`, or symbolic with
    /// `-S`); `-p` prefixes the output with a re-inputtable `umask ` command.
    /// A mode operand sets the mask from an octal number or a symbolic
    /// permission list (`u=rwx,g=rx,o=`).
    fn builtin_umask(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut symbolic = false;
        let mut reusable = false;
        let mut mode: Option<&str> = None;
        for a in args {
            match a.as_str() {
                "-S" => symbolic = true,
                "-p" => reusable = true,
                s => mode = Some(s),
            }
        }

        if let Some(m) = mode {
            // Set the mask from an octal number or a symbolic clause list.
            let new = if m.bytes().all(|b| b.is_ascii_digit()) {
                match u32::from_str_radix(m, 8) {
                    Ok(v) => v & 0o777,
                    Err(_) => {
                        self.emit_stderr(format!("{}umask: {m}: invalid octal number\n", self.err_prefix()).as_bytes());
                        return 1;
                    }
                }
            } else {
                match parse_symbolic_umask(self.umask_val, m) {
                    Some(v) => v,
                    None => {
                        self.emit_stderr(format!("{}umask: {m}: invalid symbolic mode\n", self.err_prefix()).as_bytes());
                        return 1;
                    }
                }
            };
            self.umask_val = new;
            return 0;
        }

        // No mode operand: print the current mask.
        let body = if symbolic {
            symbolic_umask_string(self.umask_val)
        } else {
            format!("{:04o}", self.umask_val)
        };
        let line = if reusable {
            if symbolic {
                format!("umask -S {body}\n")
            } else {
                format!("umask {body}\n")
            }
        } else {
            format!("{body}\n")
        };
        self.write_bytes(out, redir, line.as_bytes())
    }

    /// The `ulimit` builtin: report or set per-shell resource limits.
    ///
    /// This models bash's `ulimit` at the shell level — it tracks a `(soft,
    /// hard)` pair per resource in [`Shell::rlimits`], supports the standard
    /// option letters, `-a`, `-H`/`-S`, and the `unlimited`/`hard`/`soft`
    /// operands — but does not query or enforce the real kernel limits yet (see
    /// known-issues `TD-OILS-ULIMIT`). Modelled on Linux bash's option set,
    /// which is what slateos (a unix/linux-family target) mirrors.
    fn builtin_ulimit(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut resources: Vec<char> = Vec::new();
        let mut want_hard = false;
        let mut want_soft = false;
        let mut show_all = false;
        let mut value: Option<&str> = None;

        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                i += 1;
                break;
            }
            if let Some(flags) = a.strip_prefix('-').filter(|f| !f.is_empty()) {
                for ch in flags.chars() {
                    match ch {
                        'H' => want_hard = true,
                        'S' => want_soft = true,
                        'a' => show_all = true,
                        c if RLIMIT_SPECS.iter().any(|s| s.opt == c) => resources.push(c),
                        other => {
                            self.emit_stderr(
                                format!("{}ulimit: -{other}: invalid option\n", self.err_prefix()).as_bytes(),
                            );
                            self.emit_stderr(ULIMIT_USAGE.as_bytes());
                            return 2;
                        }
                    }
                }
                i += 1;
            } else {
                value = Some(a.as_str());
                i += 1;
                break;
            }
        }
        // Any tokens after the value operand are extra arguments (bash errors).
        if i < args.len() {
            self.emit_stderr(format!("{}ulimit: too many arguments\n", self.err_prefix()).as_bytes());
            return 1;
        }

        if show_all {
            return self.ulimit_print_all(want_hard, out, redir);
        }

        // With no resource letters, bash defaults to the file-size limit (`-f`).
        if resources.is_empty() {
            resources.push('f');
        }

        // Neither -H nor -S: setting affects both, showing reports the soft limit.
        let set_soft = want_soft || !want_hard;
        let set_hard = want_hard || !want_soft;
        let show_hard = want_hard;

        if let Some(v) = value {
            for &opt in &resources {
                let entry = self.rlimits.entry(opt).or_insert((None, None));
                let new = match v {
                    "unlimited" => None,
                    "hard" => entry.1,
                    "soft" => entry.0,
                    n => match n.parse::<u64>() {
                        Ok(parsed) => Some(parsed),
                        Err(_) => {
                            self.emit_stderr(
                                format!("{}ulimit: {n}: invalid limit argument\n", self.err_prefix()).as_bytes(),
                            );
                            return 1;
                        }
                    },
                };
                if set_soft {
                    entry.0 = new;
                }
                if set_hard {
                    entry.1 = new;
                }
            }
            return 0;
        }

        // No value: report. A single resource prints the bare value; multiple
        // resources print one labelled line each (matching bash).
        if resources.len() == 1 {
            let opt = resources[0];
            let (soft, hard) = self.rlimits.get(&opt).copied().unwrap_or((None, None));
            let v = if show_hard { hard } else { soft };
            let line = format!("{}\n", ulimit_value_str(v));
            return self.write_bytes(out, redir, line.as_bytes());
        }
        let mut buf = Vec::new();
        for &opt in &resources {
            if let Some(spec) = RLIMIT_SPECS.iter().find(|s| s.opt == opt) {
                let (soft, hard) = self.rlimits.get(&opt).copied().unwrap_or((None, None));
                let v = if show_hard { hard } else { soft };
                buf.extend_from_slice(ulimit_line(spec, v).as_bytes());
            }
        }
        self.write_bytes(out, redir, &buf)
    }

    /// Print every known resource limit in bash's `ulimit -a` format.
    fn ulimit_print_all(&mut self, show_hard: bool, out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut buf = Vec::new();
        for spec in RLIMIT_SPECS {
            let (soft, hard) = self.rlimits.get(&spec.opt).copied().unwrap_or((None, None));
            let v = if show_hard { hard } else { soft };
            buf.extend_from_slice(ulimit_line(spec, v).as_bytes());
        }
        self.write_bytes(out, redir, &buf)
    }

    /// Whether the named trap (`DEBUG`/`RETURN`/`ERR`) is currently *suppressed*
    /// by trap-inheritance masking — i.e. it exists in `self.traps` but was
    /// installed by a caller and this innermost untraced function frame does not
    /// inherit it, so it must not fire here. Only the innermost frame matters
    /// (a nested function re-evaluates inheritance from its own attributes).
    fn trap_suppressed(&self, name: &str) -> bool {
        self.trap_suppress.last().is_some_and(|s| match name {
            "DEBUG" => s.debug,
            "RETURN" => s.ret,
            "ERR" => s.err,
            _ => false,
        })
    }

    /// Clear the suppression of the named trap for the innermost function frame.
    /// Called by the `trap` builtin when the body (re)assigns a DEBUG/RETURN/ERR
    /// trap: a body-installed trap fires normally from that point on (bash), so
    /// it is no longer treated as a merely-inherited, masked trap.
    fn unsuppress_trap(&mut self, name: &str) {
        if let Some(s) = self.trap_suppress.last_mut() {
            match name {
                "DEBUG" => s.debug = false,
                "RETURN" => s.ret = false,
                "ERR" => s.err = false,
                _ => {}
            }
        }
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
        let mut out = Out::Inherit;
        self.run_exit_trap_out(&mut out, &StdinSrc::Inherit);
    }

    /// Fire the EXIT trap (at most once), writing its output to `out`.
    ///
    /// bash fires an EXIT trap for **every** shell environment as it exits — not
    /// just the top-level shell, but also each subshell (`( … )`, a pipeline
    /// stage, and a command substitution `$( … )`) that *sets its own* EXIT trap.
    /// A subshell does not re-fire the parent's inherited EXIT trap (that one is
    /// reset to default in the subshell, per [`Shell::clone_for_subshell`]); only
    /// a trap installed inside the subshell fires on the subshell's exit. Routing
    /// through `out` is what lets `x=$( trap 'echo t' EXIT; … )` capture the
    /// trap's output into the substitution result, matching bash.
    fn run_exit_trap_out(&mut self, out: &mut Out, stdin: &StdinSrc) {
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
            match parse_with_aliases(&action, &self.aliases) {
                Ok(prog) => {
                    let _ = self.exec_program(&prog, out, stdin);
                }
                Err(e) => {
                    self.errln(&format_parse_error(&e, &self.err_prefix()));
                }
            }
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
        // Leading option words made up solely of the letters `neE` are flags
        // (bash: `-n` no newline, `-e` interpret backslash escapes, `-E` disable
        // that; they may be clustered, e.g. `-ne`). Parsing stops at the first
        // word that is not such a flag.
        let mut newline = true;
        let mut interpret = false;
        let mut start = 0;
        while let Some(a) = args.get(start) {
            if a.len() >= 2
                && a.starts_with('-')
                && a[1..].chars().all(|c| matches!(c, 'n' | 'e' | 'E'))
            {
                for c in a[1..].chars() {
                    match c {
                        'n' => newline = false,
                        'e' => interpret = true,
                        'E' => interpret = false,
                        _ => {}
                    }
                }
                start += 1;
            } else {
                break;
            }
        }
        let joined = args[start..].join(" ");
        let mut line = if interpret {
            let (text, suppress) = echo_expand_escapes(&joined);
            if suppress {
                newline = false;
            }
            text
        } else {
            joined
        };
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
                self.errln(&format!("{}printf: -v: option requires an argument", self.err_prefix()));
                return 2;
            };
            assign_var = Some(name.clone());
            i = 2;
        }
        let Some(fmt) = args.get(i) else {
            return 0;
        };
        // Collect per-argument "invalid number" diagnostics (bash writes each to
        // stderr and makes printf exit non-zero, but still emits the output with
        // the best-effort numeric value).
        let mut errors: Vec<String> = Vec::new();
        let text = format_printf(fmt, &args[i + 1..], &mut errors);
        for e in &errors {
            self.emit_stderr(format!("{}printf: {e}\n", self.err_prefix()).as_bytes());
        }
        let num_status = i32::from(!errors.is_empty());
        if let Some(name) = assign_var {
            // `-v` may target an array element: `printf -v 'arr[2]' …`.
            let (base, index) = match (name.find('['), name.strip_suffix(']')) {
                (Some(open), Some(inner)) => (
                    name[..open].to_string(),
                    Some(Box::new(Word::literal(&inner[open + 1..]))),
                ),
                _ => (name.clone(), None),
            };
            // A readonly target is rejected (status 1), leaving it intact.
            let resolved = self.resolve_ref_name(&base);
            if self.readonly.contains(&resolved) {
                self.emit_stderr(format!("{}{base}: readonly variable\n", self.err_prefix()).as_bytes());
                return 1;
            }
            self.assign_elem(&base, &index, text);
            num_status
        } else {
            let write_status = self.write_bytes(out, redir, text.as_bytes());
            // A write error dominates; otherwise report the numeric-parse status.
            if write_status != 0 { write_status } else { num_status }
        }
    }

    fn builtin_export(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // Parse leading flags: `-p` (list exported vars), `-n` (remove the export
        // attribute), `--` ends option processing. (`-f`, exporting functions,
        // is not modelled.)
        let mut print = false;
        let mut unexport = false;
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                i += 1;
                break;
            }
            if a.starts_with('-') && a.len() > 1 && !a.contains('=') {
                for c in a[1..].chars() {
                    match c {
                        'p' => print = true,
                        'n' => unexport = true,
                        _ => {
                            self.emit_stderr(
                                format!("{}export: -{c}: invalid option\n", self.err_prefix()).as_bytes(),
                            );
                            self.emit_stderr(
                                b"export: usage: export [-fn] [name[=value] ...] or export -p\n",
                            );
                            return 2;
                        }
                    }
                }
                i += 1;
            } else {
                break;
            }
        }
        let operands = &args[i..];
        if operands.is_empty() {
            // `-n` with no names is a no-op; `-p` or a bare `export` lists.
            if unexport {
                return 0;
            }
            return self.export_list(out, redir);
        }
        let _ = print; // `-p` with operands behaves like plain `export`.
        for a in operands {
            if let Some(eq) = a.find('=') {
                // Support the `NAME+=value` append form alongside `NAME=value`.
                let (k, append, v) = if eq > 0 && a.as_bytes()[eq - 1] == b'+' {
                    (a[..eq - 1].to_string(), true, a[eq + 1..].to_string())
                } else {
                    (a[..eq].to_string(), false, a[eq + 1..].to_string())
                };
                let stored = if append {
                    let mut cur = self.vars.get(&k).cloned().unwrap_or_default();
                    cur.push_str(&v);
                    cur
                } else {
                    v
                };
                self.vars.insert(k.clone(), stored);
                if unexport {
                    self.exported.remove(&k);
                } else {
                    self.exported.insert(k);
                }
            } else if unexport {
                self.exported.remove(a);
            } else {
                self.exported.insert(a.clone());
            }
        }
        0
    }

    /// List every exported variable in bash's `export -p` form, sorted by name:
    /// a set variable prints as `declare -x NAME="value"` (with any other
    /// attributes, e.g. `-rx` for readonly), and an exported-but-unset name
    /// prints as the bare `declare -x NAME`.
    fn export_list(&mut self, out: &mut Out, redir: &RedirPlan) -> i32 {
        let mut names: Vec<String> = self.exported.iter().cloned().collect();
        names.sort();
        names.dedup();
        let mut listing = String::new();
        for name in &names {
            if let Some(def) = self.format_declare_def(name) {
                // `format_declare_def` already folds in the `x` (and any other)
                // attribute flags for a set variable.
                listing.push_str(&def);
                listing.push('\n');
            } else {
                listing.push_str(&format!("declare -x {name}\n"));
            }
        }
        self.write_bytes(out, redir, listing.as_bytes())
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
    /// `declare -F` / `declare -f` — operate on functions. With no names,
    /// `-F` lists every function as `declare -f NAME` (sorted). With names,
    /// `-F` prints each name that is a function (bare `NAME`), returning status
    /// 1 if any name is not a function. `-f` shares the existence semantics
    /// (status 0 iff every name is a function) so idioms like
    /// `declare -f fn >/dev/null` work; printing the function *body* awaits an
    /// AST source pretty-printer (see known-issues TD-OILS18).
    /// Detect a `declare`/`typeset` function trace-attribute operation
    /// (`-ft name…`, `-f +t name…`, …). Returns `Some((enable, names_start))`
    /// when the leading flags select functions and toggle the trace flag (`t`);
    /// `enable` is the sign of the `t` flag (true = `-t` set, false = `+t`
    /// clear), and `names_start` is the index of the first name operand.
    ///
    /// Function mode is entered ONLY by a minus-signed `-f`/`-F`: bash never
    /// selects functions from a plus-signed `+f`, so e.g. `declare +ft name`
    /// does NOT touch the function's trace attribute (it falls through to the
    /// variable path). Returns `None` for any non-trace invocation.
    fn func_trace_op(args: &[String]) -> Option<(bool, usize)> {
        let mut func = false;
        let mut trace: Option<bool> = None;
        let mut i = 0;
        while let Some(arg) = args.get(i) {
            if arg == "--" {
                i += 1;
                break;
            }
            let enable = arg.starts_with('-');
            let Some(flags) = arg.strip_prefix(['-', '+']).filter(|f| !f.is_empty()) else {
                break;
            };
            for c in flags.chars() {
                match c {
                    // Only a `-f`/`-F` (minus) enters function mode.
                    'f' | 'F' if enable => func = true,
                    't' => trace = Some(enable),
                    _ => {}
                }
            }
            i += 1;
        }
        match (func, trace) {
            (true, Some(enable)) => Some((enable, i)),
            _ => None,
        }
    }

    /// Apply (`enable`) or remove (`+t`) the trace attribute on each named
    /// function, so it inherits the caller's `DEBUG`/`RETURN` traps even when
    /// `functrace` is off. A name that is not a defined function is an error
    /// (bash: `not found`, exit 1), matching `declare -f` on an unknown name.
    fn set_func_trace(&mut self, enable: bool, names: &[String]) -> i32 {
        let mut status = 0;
        for name in names {
            if self.funcs.contains_key(name) {
                if enable {
                    self.fn_trace_attr.insert(name.clone());
                } else {
                    self.fn_trace_attr.remove(name);
                }
            } else {
                self.errln(&format!("{}{name}: not found", self.err_prefix()));
                status = 1;
            }
        }
        status
    }

    fn declare_functions(
        &mut self,
        args: &[String],
        name_only: bool,
        out: &mut Out,
        redir: &RedirPlan,
    ) -> i32 {
        let names: Vec<&String> = args.iter().skip_while(|a| a.starts_with('-')).collect();
        if names.is_empty() {
            let mut all: Vec<&String> = self.funcs.keys().collect();
            all.sort();
            let mut listing = String::new();
            for name in all {
                if name_only {
                    // `declare -F` — list each function as a `declare -f NAME` line.
                    listing.push_str(&format!("declare -f {name}\n"));
                } else if let Some(body) = self.funcs.get(name) {
                    // `declare -f` — print every function's reconstructed source.
                    listing.push_str(&crate::unparse::unparse_function(name, body));
                }
            }
            return self.write_bytes(out, redir, listing.as_bytes());
        }
        let mut listing = String::new();
        let mut status = 0;
        for name in names {
            if let Some(body) = self.funcs.get(name) {
                if name_only {
                    listing.push_str(name);
                    listing.push('\n');
                } else {
                    // `declare -f NAME` prints the function's reconstructed source.
                    listing.push_str(&crate::unparse::unparse_function(name, body));
                }
            } else {
                status = 1;
            }
        }
        let write_status = self.write_bytes(out, redir, listing.as_bytes());
        if status != 0 { status } else { write_status }
    }

    /// Index of the first non-flag operand in a `declare`/`typeset` argument
    /// list — i.e. one past the leading `-x`/`+x` flag words (and a terminating
    /// `--`). Mirrors the flag loop in [`Shell::builtin_declare`].
    fn declare_flag_end(args: &[String]) -> usize {
        let mut i = 0;
        while let Some(arg) = args.get(i) {
            if arg == "--" {
                return i + 1;
            }
            match arg.strip_prefix(['-', '+']) {
                Some(f) if !f.is_empty() => i += 1,
                _ => return i,
            }
        }
        i
    }

    /// `declare -A`/`-a`/`-i`/`-x`/`-r`/`-n`/`-l`/`-u` with no name operands:
    /// list every variable that carries **at least one** of the requested
    /// attributes (bash's union semantics — `declare -ir` lists integer *or*
    /// readonly variables), sorted by name, in re-inputtable `declare -FLAGS
    /// name="value"` form. Internal bash-only arrays (`BASH_ALIASES`, etc.) are
    /// not modelled, so they simply don't appear.
    fn declare_list_filtered(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        let start = Self::declare_flag_end(args);
        let mut want: Vec<char> = Vec::new();
        for a in &args[..start] {
            for c in a.chars() {
                if matches!(c, 'A' | 'a' | 'i' | 'x' | 'r' | 'n' | 'l' | 'u') && !want.contains(&c) {
                    want.push(c);
                }
            }
        }
        let has_attr = |sh: &Shell, name: &str| {
            want.iter().any(|&c| match c {
                'A' => sh.assoc.contains_key(name),
                'a' => sh.arrays.contains_key(name),
                'i' => sh.integer_attr.contains(name),
                'x' => sh.exported.contains(name),
                'r' => sh.readonly.contains(name),
                'n' => sh.nameref_attr.contains(name),
                'l' => sh.lower_attr.contains(name),
                'u' => sh.upper_attr.contains(name),
                'c' => sh.capcase_attr.contains(name),
                _ => false,
            })
        };
        let mut all: Vec<&String> = self
            .vars
            .keys()
            .chain(self.arrays.keys())
            .chain(self.assoc.keys())
            .collect();
        all.sort();
        all.dedup();
        let names: Vec<String> =
            all.into_iter().filter(|n| has_attr(self, n)).cloned().collect();
        let mut listing = String::new();
        for name in &names {
            if let Some(def) = self.format_declare_def(name) {
                listing.push_str(&def);
                listing.push('\n');
            }
        }
        self.write_bytes(out, redir, listing.as_bytes())
    }

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
                    self.emit_stderr(format!("{}declare: {name}: not found\n", self.err_prefix()).as_bytes());
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
    /// Build the `declare` attribute-flag string for `name` (e.g. `-ir`, `-A`,
    /// `--` when there are none). `kind` seeds the collection-type letter
    /// (`"A"`/`"a"` for assoc/indexed arrays, `""` for a scalar).
    fn declare_attr_flags(&self, name: &str, kind: &str) -> String {
        let mut s = String::from(kind);
        if self.nameref_attr.contains(name) {
            s.push('n');
        }
        if self.integer_attr.contains(name) {
            s.push('i');
        }
        if self.lower_attr.contains(name) {
            s.push('l');
        }
        if self.upper_attr.contains(name) {
            s.push('u');
        }
        if self.readonly.contains(name) {
            s.push('r');
        }
        if self.exported.contains(name) {
            s.push('x');
        }
        // bash orders `-c` after i/r/x (`declare -rc`, `declare -xc`, `declare -ic`).
        if self.capcase_attr.contains(name) {
            s.push('c');
        }
        if s.is_empty() { "--".to_string() } else { format!("-{s}") }
    }

    fn format_declare_def(&self, name: &str) -> Option<String> {
        if self.assoc.contains_key(name) {
            return self
                .format_var_assignment(name)
                .map(|body| format!("declare {} {body}", self.declare_attr_flags(name, "A")));
        }
        if self.arrays.contains_key(name) {
            return self
                .format_var_assignment(name)
                .map(|body| format!("declare {} {body}", self.declare_attr_flags(name, "a")));
        }
        if self.vars.contains_key(name) {
            return self
                .format_var_assignment(name)
                .map(|body| format!("declare {} {body}", self.declare_attr_flags(name, "")));
        }
        None
    }

    /// Format a variable as a re-inputtable `name=value` / `name=([i]="v" …)`
    /// assignment (no `declare` prefix or attribute flags), or `None` if unset.
    /// Shared by `declare -p` and the bare `set` variable listing.
    fn format_var_assignment(&self, name: &str) -> Option<String> {
        if let Some(map) = self.assoc.get(name) {
            // bash distinguishes a never-assigned `declare -A m` (printed as the
            // bare name) from an assigned-but-empty `m=()` (printed `m=()`).
            // `array_valued` records whether the name has ever been given a
            // value; an empty map without that flag is a bare declaration. A
            // non-empty associative array prints with a trailing space before
            // the closing paren (`([k]="v" )`).
            if map.is_empty() {
                return Some(if self.array_valued.contains(name) {
                    format!("{name}=()")
                } else {
                    name.to_string()
                });
            }
            let body = map
                .iter()
                .map(|(k, v)| format!("[{}]={}", quote_declare_key(k), quote_declare_value(v)))
                .collect::<Vec<_>>()
                .join(" ");
            return Some(format!("{name}=({body} )"));
        }
        if let Some(arr) = self.arrays.get(name) {
            // As with associative arrays, an assigned-but-empty indexed array
            // (`a=()`) prints as `a=()` while a never-assigned `declare -a a`
            // prints as the bare name.
            if arr.is_empty() {
                return Some(if self.array_valued.contains(name) {
                    format!("{name}=()")
                } else {
                    name.to_string()
                });
            }
            let body = arr
                .iter()
                .map(|(i, v)| format!("[{i}]={}", quote_declare_value(v)))
                .collect::<Vec<_>>()
                .join(" ");
            return Some(format!("{name}=({body})"));
        }
        if let Some(v) = self.vars.get(name) {
            return Some(format!("{name}={}", quote_declare_value(v)));
        }
        None
    }

    /// Format a variable as the bare `set` builtin lists it. Arrays use the same
    /// double-quoted form as `declare -p`, but scalars use bash's minimal
    /// single-quote style (see `quote_set_value`) — e.g. `y=5`, `x='a b'` rather
    /// than `declare -p`'s `y="5"`, `x="a b"`.
    fn format_var_setline(&self, name: &str) -> Option<String> {
        if self.assoc.contains_key(name) || self.arrays.contains_key(name) {
            return self.format_var_assignment(name);
        }
        if let Some(v) = self.vars.get(name) {
            return Some(format!("{name}={}", quote_set_value(v)));
        }
        None
    }

    fn builtin_declare(&mut self, args: &[String], is_local: bool) -> i32 {
        if is_local && self.local_frames.is_empty() {
            self.emit_stderr(format!("{}local: can only be used in a function\n", self.err_prefix()).as_bytes());
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
        // (`-l`/`-u`/`-c` are mutually exclusive; `+l`/`+u`/`+c` clear). `None` =
        // untouched, `Some(0)` = clear, `Some(1)` = lowercase, `Some(2)` =
        // uppercase, `Some(3)` = capitalize. When two *different* enable
        // directions are given together bash cancels them all (stores unchanged,
        // no attribute); `case_conflict` records that so we clear instead.
        let mut case_dir: Option<u8> = None;
        let mut case_conflict = false;
        // Nameref attribute: `-n` sets it, `+n` removes it.
        let mut nameref = false;
        let mut unset_nameref = false;
        // `-g`: force global scope even inside a function (bash: `declare`
        // inside a function otherwise creates a *local*, like `local`).
        let mut global = false;
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
                        'l' | 'u' | 'c' => {
                            let dir = match c {
                                'l' => 1,
                                'u' => 2,
                                _ => 3,
                            };
                            if enable {
                                if matches!(case_dir, Some(prev) if prev != 0 && prev != dir) {
                                    case_conflict = true;
                                }
                                case_dir = Some(dir);
                            } else {
                                case_dir = Some(0);
                            }
                        }
                        'n' => {
                            if enable {
                                nameref = true;
                            } else {
                                unset_nameref = true;
                            }
                        }
                        'g' => global = enable,
                        _ => {} // -p: accepted, no effect here.
                    }
                }
                i += 1;
            } else {
                break;
            }
        }
        // Inside a function, `declare` (and `typeset`) create locals by default,
        // exactly like `local`; `declare -g` opts back out to global scope. The
        // `local` builtin is always local. Outside a function everything is
        // global regardless.
        let make_local = is_local || (!global && !self.local_frames.is_empty());
        let mut status = 0;
        for name_val in &args[i..] {
            // Split `NAME=value` / `NAME+=value`; the `+=` form appends to (or
            // numerically adds, under `-i`) the variable's current value.
            let (name, append, value) = match name_val.find('=') {
                Some(eq) => {
                    if eq > 0 && name_val.as_bytes()[eq - 1] == b'+' {
                        (
                            &name_val[..eq - 1],
                            true,
                            Some(name_val[eq + 1..].to_string()),
                        )
                    } else {
                        (&name_val[..eq], false, Some(name_val[eq + 1..].to_string()))
                    }
                }
                None => (name_val.as_str(), false, None),
            };
            if name.is_empty() {
                continue;
            }
            // Reassigning a value to an existing readonly variable is an error.
            if value.is_some() && self.readonly.contains(name) {
                self.emit_stderr(format!("{}{name}: readonly variable\n", self.err_prefix()).as_bytes());
                status = 1;
                continue;
            }
            // Shadow the name (snapshot + clear) before (re)binding it when this
            // declaration is function-local.
            if make_local {
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
            if nameref {
                self.nameref_attr.insert(name.to_string());
            } else if unset_nameref {
                self.nameref_attr.remove(name);
            }
            // Conflicting enable directions (e.g. `-lc`, `-lu`) cancel to none.
            let case_dir = if case_conflict { Some(0) } else { case_dir };
            match case_dir {
                Some(1) => {
                    // `-l`: lowercase (mutually exclusive with uppercase/capitalize).
                    self.lower_attr.insert(name.to_string());
                    self.upper_attr.remove(name);
                    self.capcase_attr.remove(name);
                }
                Some(2) => {
                    // `-u`: uppercase.
                    self.upper_attr.insert(name.to_string());
                    self.lower_attr.remove(name);
                    self.capcase_attr.remove(name);
                }
                Some(3) => {
                    // `-c`: capitalize first char, lowercase the rest.
                    self.capcase_attr.insert(name.to_string());
                    self.lower_attr.remove(name);
                    self.upper_attr.remove(name);
                }
                Some(_) => {
                    // `+l`/`+u`/`+c`: clear all case attributes.
                    self.lower_attr.remove(name);
                    self.upper_attr.remove(name);
                    self.capcase_attr.remove(name);
                }
                None => {}
            }
            if let Some(v) = value {
                if self.nameref_attr.contains(name) {
                    // `declare -n ref=target` — store the target *name* literally
                    // (no case-fold, and bypassing the assignment redirect so the
                    // nameref itself is bound, not its eventual target).
                    self.vars.insert(name.to_string(), v);
                } else if assoc || indexed {
                    // `declare -A m=str` / `-a a=str` — scalar init unsupported;
                    // ignore the value (bash would treat str as element/key).
                } else if self.integer_attr.contains(name) {
                    // Integer attribute: the initializer is an arithmetic
                    // expression, evaluated and stored as its decimal value. With
                    // `+=`, the result is added to the current numeric value. A
                    // bad expression is fatal (bash aborts the shell) — see
                    // `eval_int_assign`; `run_builtin` turns the flag into an exit.
                    let n = self.eval_int_assign(&v);
                    let n = if append {
                        let cur = self
                            .vars
                            .get(name)
                            .and_then(|s| s.trim().parse::<i64>().ok())
                            .unwrap_or(0);
                        cur.wrapping_add(n)
                    } else {
                        n
                    };
                    self.vars.insert(name.to_string(), n.to_string());
                } else {
                    // Case attribute (`-l`/`-u`), if any, folds the value. With
                    // `+=`, the (folded) value is appended to the current string.
                    let folded = self.fold_case_attr(name, v);
                    let stored = if append {
                        let mut cur = self.vars.get(name).cloned().unwrap_or_default();
                        cur.push_str(&folded);
                        cur
                    } else {
                        folded
                    };
                    self.vars.insert(name.to_string(), stored);
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
    fn exec_declare_with_arrays(
        &mut self,
        argv: &[String],
        decl_arrays: &[Assignment],
        out: &mut Out,
        redir: &RedirPlan,
    ) -> Flow {
        let cmd = argv.first().map(String::as_str).unwrap_or("");
        let is_local = cmd == "local";
        if is_local && self.local_frames.is_empty() {
            self.emit_stderr(format!("{}local: can only be used in a function\n", self.err_prefix()).as_bytes());
            self.last_status = 1;
            return Flow::Next;
        }
        // Determine the array kind, the value attributes (integer/case/readonly/
        // export/nameref), and whether `-g` forces global, from the leading
        // dashed flags. The attributes must be applied to the *array names* here
        // (they are in `decl_arrays`, not `argv`, so `builtin_declare` below only
        // sees them for scalar operands).
        let mut assoc = false;
        let mut indexed = false;
        let mut global = false;
        let mut integer = false;
        let mut unset_integer = false;
        let mut case_dir: Option<u8> = None;
        let mut case_conflict = false;
        // `readonly`/`export` imply the corresponding attribute on every name.
        let mut readonly = cmd == "readonly";
        let mut export = cmd == "export";
        let mut nameref = false;
        let mut unset_nameref = false;
        for arg in &argv[1..] {
            let enable = arg.starts_with('-');
            let Some(flags) = arg.strip_prefix(['-', '+']) else {
                break; // first non-flag operand — flags are done
            };
            if arg == "--" {
                break; // `--` ends option parsing
            }
            for c in flags.chars() {
                match c {
                    'A' => assoc = true,
                    'a' => indexed = true,
                    'g' => global = enable,
                    'i' if enable => integer = true,
                    'i' => unset_integer = true,
                    'l' | 'u' | 'c' => {
                        let dir = match c {
                            'l' => 1,
                            'u' => 2,
                            _ => 3,
                        };
                        if enable {
                            if matches!(case_dir, Some(prev) if prev != 0 && prev != dir) {
                                case_conflict = true;
                            }
                            case_dir = Some(dir);
                        } else {
                            case_dir = Some(0);
                        }
                    }
                    'r' if enable => readonly = true,
                    'x' if enable => export = true,
                    'n' if enable => nameref = true,
                    'n' => unset_nameref = true,
                    _ => {}
                }
            }
        }
        // As with scalar `declare`, an array declaration inside a function is
        // local by default unless `-g` was given.
        let make_local = is_local || (!global && !self.local_frames.is_empty());
        // Apply flags + any scalar operands (e.g. `declare -x FOO=bar`). For
        // `readonly`/`export`, route scalar operands through their own builtin —
        // but only when a non-flag operand is present, so an array-literal-only
        // invocation (`readonly arr=(1 2)`) never slips into listing mode.
        let has_scalar_operand = argv[1..]
            .iter()
            .any(|a| a != "--" && !a.starts_with(['-', '+']));
        let status = match cmd {
            "readonly" if has_scalar_operand => self.builtin_readonly(&argv[1..], out, redir),
            "export" if has_scalar_operand => self.builtin_export(&argv[1..], out, redir),
            "readonly" | "export" => 0,
            _ => self.builtin_declare(&argv[1..], is_local),
        };
        // Mark each array name's kind + attributes before applying the literal,
        // so `apply_assignment` routes to the right store and (for `-i`)
        // evaluates the values arithmetically.
        for a in decl_arrays {
            // A function-local array declaration shadows the name in the current
            // frame first.
            if make_local {
                self.declare_local(&a.name);
            }
            if assoc {
                self.assoc.entry(a.name.clone()).or_default();
            } else if indexed {
                self.arrays.entry(a.name.clone()).or_default();
            }
            // Apply the value attributes to the array name (mirrors the scalar
            // path in `builtin_declare`).
            if integer {
                self.integer_attr.insert(a.name.clone());
            } else if unset_integer {
                self.integer_attr.remove(&a.name);
            }
            let case_dir = if case_conflict { Some(0) } else { case_dir };
            match case_dir {
                Some(1) => {
                    self.lower_attr.insert(a.name.clone());
                    self.upper_attr.remove(&a.name);
                    self.capcase_attr.remove(&a.name);
                }
                Some(2) => {
                    self.upper_attr.insert(a.name.clone());
                    self.lower_attr.remove(&a.name);
                    self.capcase_attr.remove(&a.name);
                }
                Some(3) => {
                    self.capcase_attr.insert(a.name.clone());
                    self.lower_attr.remove(&a.name);
                    self.upper_attr.remove(&a.name);
                }
                Some(_) => {
                    self.lower_attr.remove(&a.name);
                    self.upper_attr.remove(&a.name);
                    self.capcase_attr.remove(&a.name);
                }
                None => {}
            }
            if nameref {
                self.nameref_attr.insert(a.name.clone());
            } else if unset_nameref {
                self.nameref_attr.remove(&a.name);
            }
            if export {
                self.exported.insert(a.name.clone());
            }
            // Default (no flag): an array literal makes an indexed array — which
            // `apply_assignment` already does for a name absent from `assoc`.
            // `trace = false`: the `declare`/`local` command itself is traced via
            // the command path, so the inner assignment must not trace again.
            self.apply_assignment(a, false);
            // `readonly` is applied *after* the value is bound (a readonly guard
            // in `apply_assignment` would otherwise reject the initializer).
            if readonly {
                self.readonly.insert(a.name.clone());
            }
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
            let mut ro: Vec<String> = self.readonly.iter().cloned().collect();
            ro.sort();
            let mut listing = String::new();
            for name in &ro {
                // bash's `readonly -p` reuses `declare -p` formatting: scalars
                // as `declare -r name="value"`, arrays as `declare -ar name=(…)`,
                // and a valueless readonly as a bare `declare -r name`.
                match self.format_declare_def(name) {
                    Some(def) => {
                        listing.push_str(&def);
                        listing.push('\n');
                    }
                    None => {
                        listing.push_str(&format!(
                            "declare {} {name}\n",
                            self.declare_attr_flags(name, "")
                        ));
                    }
                }
            }
            return self.write_bytes(out, redir, listing.as_bytes());
        }
        let mut status = 0;
        for name_val in names {
            // Support `NAME=value` and the `NAME+=value` append form.
            let (name, append, value) = match name_val.find('=') {
                Some(eq) => {
                    if eq > 0 && name_val.as_bytes()[eq - 1] == b'+' {
                        (
                            &name_val[..eq - 1],
                            true,
                            Some(name_val[eq + 1..].to_string()),
                        )
                    } else {
                        (&name_val[..eq], false, Some(name_val[eq + 1..].to_string()))
                    }
                }
                None => (name_val.as_str(), false, None),
            };
            if name.is_empty() {
                continue;
            }
            if value.is_some() && self.readonly.contains(name) {
                self.emit_stderr(format!("{}{name}: readonly variable\n", self.err_prefix()).as_bytes());
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
                let stored = if append {
                    let mut cur = self.vars.get(name).cloned().unwrap_or_default();
                    cur.push_str(&v);
                    cur
                } else {
                    v
                };
                self.vars.insert(name.to_string(), stored);
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
        let mut set = false;
        let mut unset = false;
        let mut quiet = false;
        let mut reinput = false; // `-p`: re-inputtable `shopt -s/-u NAME` listing
        // `-o`: operate on `set -o` options (noclobber, errexit, …) rather than
        // shopt options. Flags may be clustered (`shopt -qo NAME`, `-so NAME`).
        let mut opt_o = false;
        let mut i = 0;
        while let Some(arg) = args.get(i) {
            match arg.as_str() {
                "--" => {
                    i += 1;
                    break;
                }
                s if s.starts_with('-') && s.len() > 1 => {
                    for c in s.chars().skip(1) {
                        match c {
                            's' => set = true,
                            'u' => unset = true,
                            'q' => quiet = true,
                            'o' => opt_o = true,
                            'p' => reinput = true,
                            _ => {
                                self.emit_stderr(
                                    format!("{}shopt: -{c}: invalid option\n", self.err_prefix()).as_bytes(),
                                );
                                return 2;
                            }
                        }
                    }
                }
                _ => break,
            }
            i += 1;
        }
        let names: Vec<&String> = args[i..].iter().collect();

        // `-o` mode: query/toggle `set -o` shell options, reusing the same
        // machinery (and `%-15s\t%s` listing format) as the `set` builtin.
        if opt_o {
            return self.shopt_o_mode(&names, set, unset, quiet, out, redir);
        }

        // Listing/query mode: no names, OR names given without `-s`/`-u`. With
        // `-s`/`-u` and no names, the listing is filtered to the on/off options.
        if names.is_empty() || (!set && !unset) {
            if names.is_empty() {
                // List every known option; `-s`/`-u` filters to on/off.
                let mut listing = String::new();
                for &(opt, _) in SHOPT_TABLE {
                    let on = self.shopt.get(opt).copied().unwrap_or_else(|| self.shopt_default(opt));
                    if (set && !on) || (unset && on) {
                        continue;
                    }
                    listing.push_str(&shopt_line(opt, on, reinput));
                }
                if !quiet {
                    self.write_bytes(out, redir, listing.as_bytes());
                }
                return 0;
            }
            // Query specific names: status 0 iff all named options are set.
            let mut all_on = true;
            let mut listing = String::new();
            for name in &names {
                if !shopt_is_known(name) {
                    self.emit_stderr(
                        format!("{}shopt: {name}: invalid shell option name\n", self.err_prefix()).as_bytes(),
                    );
                    return 1;
                }
                let on = self
                    .shopt
                    .get(name.as_str())
                    .copied()
                    .unwrap_or_else(|| self.shopt_default(name));
                if !on {
                    all_on = false;
                }
                listing.push_str(&shopt_line(name, on, reinput));
            }
            if !quiet {
                self.write_bytes(out, redir, listing.as_bytes());
            }
            return i32::from(!all_on);
        }

        // Set/unset mode (names present with `-s` or `-u`).
        let mut status = 0;
        let mut changed = false;
        for name in names {
            if !shopt_is_known(name) {
                self.emit_stderr(
                    format!("{}shopt: {name}: invalid shell option name\n", self.err_prefix()).as_bytes(),
                );
                status = 1;
                continue;
            }
            self.shopt.insert(name.clone(), set);
            changed = true;
        }
        // Keep `$BASHOPTS` current (bash recomputes it on every shopt change).
        if changed {
            self.refresh_bashopts();
        }
        status
    }

    /// Recompute `$BASHOPTS` from the current `shopt` state: a colon-separated,
    /// alphabetically-sorted list of the enabled `shopt` option names, mirroring
    /// bash. bash keeps it as a readonly, dynamically-maintained variable; we
    /// refresh it here (bypassing the readonly gate, as `refresh_shellopts`
    /// does for `$SHELLOPTS`) whenever an option is toggled.
    fn refresh_bashopts(&mut self) {
        let mut on: Vec<&str> = Vec::new();
        for &(opt, _) in SHOPT_TABLE {
            if self.shopt.get(opt).copied().unwrap_or_else(|| self.shopt_default(opt)) {
                on.push(opt);
            }
        }
        on.sort_unstable();
        self.vars.insert("BASHOPTS".to_string(), on.join(":"));
    }

    /// `shopt -o …`: the `-o` variant operates on `set -o` options. Handles the
    /// list (`shopt -o`), query (`shopt -o NAME`), and set/unset
    /// (`shopt -so NAME` / `shopt -uo NAME`) forms, reusing the `set` builtin's
    /// option registry so only the options osh actually models report a live
    /// state (others are accepted but inert, as with `set -o`).
    fn shopt_o_mode(
        &mut self,
        names: &[&String],
        set: bool,
        unset: bool,
        quiet: bool,
        out: &mut Out,
        redir: &RedirPlan,
    ) -> i32 {
        // The full set of standard `set -o` option names, so a real option like
        // `braceexpand` isn't rejected even though osh doesn't model it; only a
        // truly unknown name is an error (matching bash).
        const SETO_NAMES: &[&str] = &[
            "allexport",
            "braceexpand",
            "emacs",
            "errexit",
            "errtrace",
            "functrace",
            "hashall",
            "histexpand",
            "history",
            "ignoreeof",
            "interactive-comments",
            "keyword",
            "monitor",
            "noclobber",
            "noexec",
            "noglob",
            "nolog",
            "notify",
            "nounset",
            "onecmd",
            "physical",
            "pipefail",
            "posix",
            "privileged",
            "verbose",
            "vi",
            "xtrace",
        ];

        // Set/unset mode.
        if set || unset {
            let mut status = 0;
            for name in names {
                if !SETO_NAMES.contains(&name.as_str()) {
                    self.emit_stderr(
                        format!("{}shopt: {name}: invalid option name\n", self.err_prefix()).as_bytes(),
                    );
                    status = 1;
                    continue;
                }
                self.set_named_option(name, set);
            }
            return status;
        }

        // List mode: no names → dump every modeled option in `set -o` format.
        if names.is_empty() {
            if quiet {
                return 0;
            }
            let listing = self.format_option_list(false);
            return self.write_bytes(out, redir, listing.as_bytes());
        }

        // Query mode: status 0 iff every named option is enabled.
        let mut all_on = true;
        let mut listing = String::new();
        for name in names {
            if !SETO_NAMES.contains(&name.as_str()) {
                self.emit_stderr(format!("{}shopt: {name}: invalid option name\n", self.err_prefix()).as_bytes());
                return 1;
            }
            let on = self.shell_option_enabled(name);
            if !on {
                all_on = false;
            }
            listing.push_str(&format!("{name:<15}\t{}\n", if on { "on" } else { "off" }));
        }
        if !quiet {
            self.write_bytes(out, redir, listing.as_bytes());
        }
        i32::from(!all_on)
    }

    fn builtin_unset(&mut self, args: &[String]) -> i32 {
        // Parse leading `-v` (variables only) / `-f` (functions only) flags.
        // Without a flag, a name that is a set variable is unset as a variable,
        // otherwise it is unset as a function (bash: variables take precedence).
        let mut funcs_only = false;
        let mut vars_only = false;
        // `-n`: unset the nameref itself, not the variable it points to.
        let mut nameref_only = false;
        let mut i = 0;
        while let Some(a) = args.get(i) {
            if a == "--" {
                i += 1;
                break;
            }
            if let Some(flags) = a.strip_prefix('-')
                && !flags.is_empty()
                && flags.chars().all(|c| matches!(c, 'v' | 'f' | 'n'))
            {
                for c in flags.chars() {
                    match c {
                        'f' => funcs_only = true,
                        'v' => vars_only = true,
                        'n' => nameref_only = true,
                        _ => {}
                    }
                }
                i += 1;
            } else {
                break;
            }
        }
        for a in &args[i..] {
            if funcs_only {
                self.funcs.remove(a);
                self.fn_trace_attr.remove(a);
                continue;
            }
            // `unset -n ref` removes the nameref binding itself.
            if nameref_only {
                self.nameref_attr.remove(a);
                self.vars.remove(a);
                continue;
            }
            // Without `-n`, unsetting a nameref unsets the variable it points to
            // (bash semantics); resolve the target name first.
            let a = &self.resolve_ref_name(a);
            // A readonly variable cannot be unset.
            if self.readonly.contains(a) {
                self.emit_stderr(
                    format!("{}unset: {a}: cannot unset: readonly variable\n", self.err_prefix()).as_bytes(),
                );
                return 1;
            }
            // `unset name[i]` removes a single element; `unset name` removes the
            // whole variable (or, without `-v` and when not a set variable, the
            // function).
            if let Some(open) = a.find('[')
                && a.ends_with(']')
            {
                let name = &a[..open];
                // An element of a readonly array cannot be unset either — bash
                // reports the base name as the readonly variable.
                if self.readonly.contains(name) {
                    self.emit_stderr(
                        format!("{}unset: {name}: cannot unset: readonly variable\n", self.err_prefix()).as_bytes(),
                    );
                    return 1;
                }
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
            let is_var = self.vars.contains_key(a)
                || self.arrays.contains_key(a)
                || self.assoc.contains_key(a);
            if !vars_only && !is_var {
                // Not a set variable: fall back to unsetting a function.
                self.funcs.remove(a);
                self.fn_trace_attr.remove(a);
                continue;
            }
            self.vars.remove(a);
            self.arrays.remove(a);
            self.assoc.remove(a);
            self.exported.remove(a);
            // Unsetting a variable also drops its attributes (bash semantics).
            self.integer_attr.remove(a);
            self.lower_attr.remove(a);
            self.upper_attr.remove(a);
            self.capcase_attr.remove(a);
            self.nameref_attr.remove(a);
            self.array_valued.remove(a);
        }
        0
    }

    fn builtin_set(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // Bare `set` (no operands) lists every shell variable in sorted,
        // re-inputtable `name=value` form, followed by every function definition
        // (matching bash, which prints functions after the variables).
        if args.is_empty() {
            let mut all: Vec<&String> = self
                .vars
                .keys()
                .chain(self.arrays.keys())
                .chain(self.assoc.keys())
                .collect();
            all.sort();
            all.dedup();
            let mut listing = String::new();
            for name in all {
                if let Some(def) = self.format_var_setline(name) {
                    listing.push_str(&def);
                    listing.push('\n');
                }
            }
            let mut fns: Vec<&String> = self.funcs.keys().collect();
            fns.sort();
            for name in fns {
                if let Some(body) = self.funcs.get(name) {
                    listing.push_str(&crate::unparse::unparse_function(name, body));
                }
            }
            return self.write_bytes(out, redir, listing.as_bytes());
        }
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
                        // Bare `set -o` lists options in `name  on/off` columns;
                        // `set +o` lists them as re-inputtable `set ±o name` lines.
                        let text = self.format_option_list(!enable);
                        return self.write_bytes(out, redir, text.as_bytes());
                    }
                }
                s if s.starts_with('-') || s.starts_with('+') => {
                    let enable = s.starts_with('-');
                    // A short-option cluster may embed `o` (long-option
                    // selector), e.g. `set -eo pipefail`. Each `o` consumes the
                    // next *word* as its option name (bash reads successive words
                    // for successive `o`s: `set -oo a b` sets both `a` and `b`),
                    // while the remaining cluster letters stay ordinary flags. An
                    // `o` with no following word lists the options like `set -o`.
                    let mut extra_words = 0usize;
                    for c in s[1..].chars() {
                        if c == 'o' {
                            match args.get(i + 1 + extra_words) {
                                Some(opt) => {
                                    self.set_named_option(opt, enable);
                                    extra_words += 1;
                                }
                                None => {
                                    let text = self.format_option_list(!enable);
                                    return self.write_bytes(out, redir, text.as_bytes());
                                }
                            }
                        } else {
                            self.set_short_option(c, enable);
                        }
                    }
                    i += 1 + extra_words;
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
            'f' => self.noglob = enable,
            'a' => self.allexport = enable,
            'C' => self.noclobber = enable,
            'n' => self.noexec = enable,
            'T' => self.functrace = enable,
            'E' => self.errtrace = enable,
            _ => {}
        }
        self.refresh_shellopts();
    }

    /// Apply a `set -o NAME` / `set +o NAME` long option. Unknown names are
    /// accepted and ignored.
    fn set_named_option(&mut self, name: &str, enable: bool) {
        match name {
            "pipefail" => self.pipefail = enable,
            "errexit" => self.errexit = enable,
            "nounset" => self.nounset = enable,
            "xtrace" => self.xtrace = enable,
            "noglob" => self.noglob = enable,
            "allexport" => self.allexport = enable,
            "noclobber" => self.noclobber = enable,
            "noexec" => self.noexec = enable,
            "functrace" => self.functrace = enable,
            "errtrace" => self.errtrace = enable,
            _ => {}
        }
        self.refresh_shellopts();
    }

    /// Recompute `$SHELLOPTS` from the current option state and store it as a
    /// readonly shell variable, mirroring bash. bash keeps `SHELLOPTS` as a
    /// colon-separated, alphabetically-sorted list of the enabled `set -o`
    /// options; for a non-interactive shell the always-on defaults are
    /// `braceexpand`, `hashall`, and `interactive-comments`, plus whichever of
    /// the modeled toggles are currently enabled. The result byte-matches bash
    /// because osh models exactly the options bash reports for `-c` scripts.
    /// The variable is readonly (bash renders it `declare -r`, not `-rx`), so it
    /// is not exported by default; refreshing here bypasses the readonly gate
    /// deliberately since option toggles are the legitimate mutation path.
    fn refresh_shellopts(&mut self) {
        // Always-on defaults for a non-interactive shell.
        let mut opts: Vec<&str> = vec!["braceexpand", "hashall", "interactive-comments"];
        if self.allexport {
            opts.push("allexport");
        }
        if self.errexit {
            opts.push("errexit");
        }
        if self.errtrace {
            opts.push("errtrace");
        }
        if self.functrace {
            opts.push("functrace");
        }
        if self.noclobber {
            opts.push("noclobber");
        }
        if self.noexec {
            opts.push("noexec");
        }
        if self.noglob {
            opts.push("noglob");
        }
        if self.nounset {
            opts.push("nounset");
        }
        if self.pipefail {
            opts.push("pipefail");
        }
        if self.xtrace {
            opts.push("xtrace");
        }
        opts.sort_unstable();
        self.vars.insert("SHELLOPTS".to_string(), opts.join(":"));
    }

    /// Render the `set -o` / `set +o` option listing. With `reinput` false
    /// (`set -o`), each line is `name<pad>on|off`; with `reinput` true
    /// (`set +o`), each line is a re-inputtable `set -o name` / `set +o name`.
    /// Only the options this shell actually models are listed, in alphabetical
    /// order, so the reported state is always truthful.
    fn format_option_list(&self, reinput: bool) -> String {
        // Alphabetical, matching bash's ordering of these names.
        let opts = [
            "allexport",
            "errexit",
            "errtrace",
            "functrace",
            "noclobber",
            "noexec",
            "noglob",
            "nounset",
            "pipefail",
            "xtrace",
        ];
        let mut s = String::new();
        for name in opts {
            let on = self.shell_option_enabled(name);
            if reinput {
                let flag = if on { "-o" } else { "+o" };
                s.push_str(&format!("set {flag} {name}\n"));
            } else {
                // bash's `set -o` listing is `%-15s\t%s` — a 15-wide left-
                // justified name, then a TAB, then the on/off state.
                let state = if on { "on" } else { "off" };
                s.push_str(&format!("{name:<15}\t{state}\n"));
            }
        }
        s
    }

    /// Return whether the named `set -o` option is currently enabled. Used by the
    /// `[ -o NAME ]` / `[[ -o NAME ]]` test operator. Unknown option names are
    /// reported as disabled (matching bash, which returns false for them).
    fn shell_option_enabled(&self, name: &str) -> bool {
        match name {
            "pipefail" => self.pipefail,
            "errexit" => self.errexit,
            "nounset" => self.nounset,
            "xtrace" => self.xtrace,
            "noglob" => self.noglob,
            "allexport" => self.allexport,
            "noclobber" => self.noclobber,
            "noexec" => self.noexec,
            "functrace" => self.functrace,
            "errtrace" => self.errtrace,
            _ => false,
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
                self.errln("getopts: usage: getopts optstring name [arg ...]");
                return 2;
            }
        };
        let name = match args.get(1) {
            Some(s) => s.clone(),
            None => {
                self.errln("getopts: usage: getopts optstring name [arg ...]");
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
                    self.errln(&format!("{}: illegal option -- {opt}", self.name));
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
                        self.errln(&format!("{}: option requires an argument -- {opt}", self.name));
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
    /// `-u N` reads from a descriptor opened by `exec N< file` (see
    /// [`Shell::open_fds`]); `N` = 0 falls back to normal stdin. `-t` is accepted
    /// (its argument consumed) but not yet honored — see known-issues.
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
        // `-u N`: read from user-space fd N instead of the ambient input.
        let mut ufd: Option<i32> = None;
        // `-t N`: read timeout. Only the special `-t 0` (non-consuming
        // availability poll) is honored — see the `input_available_now` check
        // below. A positive timeout is accepted but not enforced (script input
        // from files/here-strings is always immediately available anyway).
        let mut timeout: Option<f64> = None;
        // `-p PROMPT`: displayed on stderr before reading, but *only* when the
        // input is a terminal (bash). Captured here and emitted after the input
        // source is resolved, so a piped/redirected/here-string read stays quiet.
        let mut prompt: Option<String> = None;
        // Parse short options, honoring bash's cluster/attached-argument forms:
        // flags may be bundled (`-rs`) and an option-argument may be glued to its
        // letter (`-d:`, `-n5`, `-u3`) or supplied as the next token (`-d :`).
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                i += 1;
                names.extend(args[i..].iter().cloned());
                break;
            }
            if a.len() > 1 && a.starts_with('-') {
                let chars: Vec<char> = a.chars().skip(1).collect();
                let mut j = 0;
                while j < chars.len() {
                    let c = chars[j];
                    // Options that take an argument: the argument is the rest of
                    // this token after the letter, or (if none) the next token.
                    if matches!(c, 'a' | 'p' | 'd' | 'n' | 'N' | 'u' | 't' | 'i') {
                        let rest: String = chars[j + 1..].iter().collect();
                        let val = if rest.is_empty() {
                            i += 1;
                            args.get(i).cloned()
                        } else {
                            Some(rest)
                        };
                        match c {
                            'a' => array = val,
                            'p' => prompt = val,
                            // `-d ''` ⇒ NUL delimiter; otherwise the first byte.
                            'd' => {
                                delim = Some(val.and_then(|s| s.bytes().next()).unwrap_or(0));
                            }
                            'n' => nchars = val.and_then(|s| s.parse().ok()),
                            'N' => {
                                nchars = val.and_then(|s| s.parse().ok());
                                exact = true;
                            }
                            'u' => ufd = val.and_then(|s| s.parse().ok()),
                            't' => timeout = val.and_then(|s| s.parse().ok()),
                            // `-i` accepted but not honored yet.
                            _ => {}
                        }
                        break; // remainder of this token was consumed as the argument
                    }
                    match c {
                        'r' => raw = true,
                        // silent / readline-edit: no-op for non-tty input.
                        's' | 'e' => {}
                        _ => {} // unknown flag — ignored
                    }
                    j += 1;
                }
            } else {
                names.push(a.clone());
            }
            i += 1;
        }

        // `-u N` (N ≥ 3): read from the user-space fd table instead of the
        // ambient input, ignoring any `redir` stdin. Validate the fd up front
        // (before borrowing) so a bad descriptor is a clean error. Both the
        // `exec 3<`/`read -u` byte cursors and live `coproc` read ends qualify.
        if let Some(n) = ufd
            && n >= 3
            && !self.open_fds.contains_key(&n)
            && !self.coproc_read_fds.contains_key(&n)
        {
            self.errln(&format!("{}read: {n}: bad file descriptor", self.err_prefix()));
            return 1;
        }
        // A fresh `RedirPlan` masks `redir.stdin*` so the fd-N source is
        // authoritative. A `coproc` read end (live pipe) is routed via
        // `stdin_from_fd` — the same path `read <&N` uses — while an `open_fds`
        // byte fd is surfaced as a `StdinSrc::Cursor` (borrows `open_fds`
        // immutably; the borrow ends before later `&mut self` stores, per NLL).
        let mut ufd_plan = RedirPlan::default();
        let inherit_src = StdinSrc::Inherit;
        let ufd_active = ufd.is_some_and(|n| n >= 3);
        let ufd_stdin = ufd.filter(|&n| n >= 3).and_then(|n| {
            if self.coproc_read_fds.contains_key(&n) {
                ufd_plan.stdin_from_fd = Some(n);
                None
            } else {
                self.open_fds.get(&n).map(StdinSrc::Cursor)
            }
        });
        let (rd_stdin, rd_redir): (&StdinSrc, &RedirPlan) = if ufd_active {
            (ufd_stdin.as_ref().unwrap_or(&inherit_src), &ufd_plan)
        } else {
            (stdin, redir)
        };

        // `-p PROMPT`: bash writes it to stderr only when the input is an actual
        // terminal — i.e. the ambient stdin (not a `-u fd` cursor, `< file`,
        // here-doc/here-string, `exec < …` rebind, or an upstream pipeline), and
        // that stdin is a tty. A piped/redirected read shows no prompt.
        if let Some(p) = &prompt {
            let input_is_tty = matches!(rd_stdin, StdinSrc::Inherit)
                && self.exec_stdin.is_none()
                && rd_redir.stdin.is_none()
                && rd_redir.stdin_data.is_none()
                && rd_redir.stdin_from_fd.is_none()
                && io::stdin().is_terminal();
            if input_is_tty {
                self.emit_stderr(p.as_bytes());
            }
        }

        // `-t 0`: return immediately WITHOUT reading, reporting only whether a
        // read would proceed without blocking (bash: exit 0 if input is
        // available or the source is at EOF, non-zero if a read would block).
        // No variables are assigned.
        if timeout == Some(0.0) {
            return i32::from(!self.input_available_now(rd_stdin, rd_redir));
        }

        // `read -a array` resets the target array to empty up front (bash), so
        // even an EOF with no data leaves a defined, empty array (`declare -p`
        // shows `=()`), and a pre-existing array is replaced rather than merged
        // into. A readonly target rejects the read before any reset.
        if let Some(arr) = &array {
            if self.readonly.contains(arr) {
                self.emit_stderr(format!("{}{arr}: readonly variable\n", self.err_prefix()).as_bytes());
                return 1;
            }
            self.vars.remove(arr);
            self.assoc.remove(arr);
            self.array_valued.insert(arr.clone());
            self.arrays.insert(arr.clone(), BTreeMap::new());
        }

        // Choose the read strategy. Any of `-d`/`-n`/`-N` selects the
        // record reader; otherwise a plain newline-terminated line.
        let (line, terminated) = if delim.is_some() || nchars.is_some() {
            let d = delim.unwrap_or(b'\n');
            match self.read_record_input(rd_stdin, rd_redir, d, nchars, exact) {
                Some(rec) => rec,
                None => return 1, // EOF with no data
            }
        } else {
            match self.read_line(rd_stdin, rd_redir) {
                // A final line ending at EOF without a newline is a partial
                // read: the value is still assigned, but status is 1 (bash).
                Some((l, terminated)) => (l, terminated),
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
            // The target was already reset to empty (and readonly-checked) up
            // front; here we fill it with the split record.
            // `-N` assigns the raw record without IFS splitting: a single
            // element holding exactly the characters read (bash).
            let map: BTreeMap<usize, String> = if exact {
                let v = if raw { line } else { unescape_read_line(&line) };
                std::iter::once((0usize, v)).collect()
            } else {
                read_split(&line, &ifs, raw, None).into_iter().enumerate().collect()
            };
            self.vars.remove(&arr);
            self.assoc.remove(&arr);
            self.array_valued.insert(arr.clone());
            self.arrays.insert(arr, map);
            return eof_status;
        }

        if names.is_empty() {
            // No names: assign the (optionally unescaped) whole line to REPLY.
            let reply = if raw { line } else { unescape_read_line(&line) };
            // REPLY is rarely readonly, but honor it if so (status 1, no write).
            if !self.set_scalar_checked("REPLY", reply) {
                return 1;
            }
            return eof_status;
        }

        // `-N` does not split on IFS: the whole record goes to the first name
        // (any remaining names are cleared), matching bash's exact-read intent.
        let fields = if exact {
            let v = if raw { line } else { unescape_read_line(&line) };
            let mut f = vec![v];
            f.resize(names.len(), String::new());
            f
        } else {
            read_split(&line, &ifs, raw, Some(names.len()))
        };
        for (idx, name) in names.iter().enumerate() {
            let val = fields.get(idx).cloned().unwrap_or_default();
            // A readonly target aborts the read at that field (bash: earlier
            // fields are already assigned, the read fails with status 1).
            if !self.set_scalar_checked(name, val) {
                return 1;
            }
        }
        eof_status
    }

    /// Read the entire current input source (here-doc/here-string, `< file`
    /// redirect, pipeline cursor/pipe, or inherited stdin) to end-of-input.
    fn read_all_bytes(&self, stdin: &StdinSrc, redir: &RedirPlan) -> Vec<u8> {
        use io::Read;
        if let Some(n) = redir.stdin_from_fd {
            let mut buf = Vec::new();
            if let Some(rd) = self.coproc_read_fds.get(&n) {
                let _ = rd.borrow_mut().read_to_end(&mut buf);
            } else if let Some(c) = self.input_fd_cursor(n) {
                let _ = c.borrow_mut().read_to_end(&mut buf);
            }
            return buf;
        }
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
                if let Some(cur) = &self.exec_stdin {
                    let _ = cur.borrow_mut().read_to_end(&mut buf);
                } else {
                    let _ = io::stdin().lock().read_to_end(&mut buf);
                }
            }
        }
        buf
    }

    /// The `mapfile`/`readarray [-d delim] [-n count] [-O origin] [-s skip] [-t]
    /// [-C callback] [-c quantum] [array]` builtin: read lines from standard
    /// input into an indexed array (default `MAPFILE`). Each element retains the
    /// trailing delimiter unless `-t` is given. Supports `-d` (alternate
    /// delimiter), `-n` (max count, 0 = all), `-s` (skip leading lines), `-O`
    /// (starting array index), and `-C callback`/`-c quantum` (evaluate
    /// `callback` every `quantum` lines, passing the target index and the raw
    /// line — including its delimiter — before the element is assigned; the
    /// default quantum is 5000).
    fn builtin_mapfile(
        &mut self,
        args: &[String],
        stdin: &StdinSrc,
        redir: &RedirPlan,
        out: &mut Out,
    ) -> i32 {
        let mut strip = false;
        let mut delim = b'\n';
        let mut count: usize = 0; // 0 = unlimited
        let mut skip: usize = 0;
        let mut origin: usize = 0;
        let mut callback: Option<String> = None;
        let mut quantum: usize = 5000;
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
                "-n" => {
                    i += 1;
                    count = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(0);
                }
                "-c" => {
                    i += 1;
                    // A quantum of 0 disables the callback in bash; keep the
                    // default otherwise.
                    quantum = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(5000);
                }
                "-C" => {
                    i += 1;
                    callback = args.get(i).cloned();
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
                    self.errln(&format!("{}mapfile: {other}: invalid option", self.err_prefix()));
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

        // A callback only fires when `-C` was given and the quantum is non-zero.
        let fire_callback = callback.as_ref().is_some_and(|c| !c.is_empty()) && quantum != 0;

        let mut elems: BTreeMap<usize, String> = BTreeMap::new();
        let mut idx = origin;
        // Number of elements assigned so far (1-based when checking the quantum),
        // matching bash's `line_count % quantum == 0` boundary test.
        let mut assigned: usize = 0;
        for piece in pieces.into_iter().skip(skip) {
            if count != 0 && idx.saturating_sub(origin) >= count {
                break;
            }
            let mut s = String::from_utf8_lossy(&piece).into_owned();
            if strip && s.as_bytes().last() == Some(&delim) {
                s.pop();
            }
            // The callback runs before the element is assigned (so `${arr[$idx]}`
            // is still empty inside it) and receives the *same* value that will
            // be stored — i.e. the `-t`-stripped line, or the raw line (delimiter
            // included) when `-t` is absent. bash passes the target index and the
            // line as command arguments; the callback text itself is evaluated
            // as-is, so only the index and line need quoting.
            assigned = assigned.saturating_add(1);
            if fire_callback
                && assigned.is_multiple_of(quantum)
                && let Some(cb) = &callback
            {
                let cmd = format!("{cb} {idx} {}", single_quote(&s));
                self.run_source_out(&cmd, out);
            }
            elems.insert(idx, s);
            idx = idx.saturating_add(1);
        }
        self.vars.remove(&array);
        self.assoc.remove(&array);
        self.array_valued.insert(array.clone());
        self.arrays.insert(array, elems);
        0
    }

    fn builtin_source(&mut self, args: &[String]) -> i32 {
        let Some(path) = args.first() else {
            self.errln(&format!("{}source: filename argument required", self.err_prefix()));
            return 2;
        };
        match std::fs::read_to_string(path) {
            Ok(src) => {
                let saved = if args.len() > 1 {
                    Some(std::mem::replace(&mut self.positional, args[1..].to_vec()))
                } else {
                    None
                };
                // Mark that we are inside a sourced script so a `return` in it is
                // legal (and unwinds just this source, like bash).
                self.source_depth = self.source_depth.saturating_add(1);
                let code = self.run_source(&src);
                self.source_depth = self.source_depth.saturating_sub(1);
                if let Some(p) = saved {
                    self.positional = p;
                }
                code
            }
            Err(e) => {
                self.errln(&format!("{}source: {path}: {e}", self.err_prefix()));
                1
            }
        }
    }

    /// `compgen [options] [word]` — generate completion candidates and print
    /// each one matching the optional prefix `word` on its own line.
    ///
    /// Supported option subset: `-W wordlist` (IFS-split candidate list),
    /// action selectors (`-A action` plus the `-a -b -c -d -e -f -k -v`
    /// shortcuts: alias, builtin, command, directory, export, file, keyword,
    /// variable — and `-A function`), `-P prefix` / `-S suffix` (added to each
    /// match after filtering), and `-X filterpat` (glob-remove matches; a
    /// leading `!` inverts to keep-only). Returns 0 if at least one candidate
    /// was produced, else 1 (matching bash). The interactive/programmable
    /// selectors that require a live completion context (`-F`/`-C`/`-o`/`-G`
    /// and the user/group/job/service actions) are parsed-and-ignored so
    /// scripts that pass them still run without error.
    fn builtin_compgen(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        const KEYWORDS: &[&str] = &[
            "if", "then", "elif", "else", "fi", "time", "for", "in", "until", "while", "do",
            "done", "case", "esac", "coproc", "select", "function", "{", "}", "!", "[[", "]]",
        ];

        let mut wordlists: Vec<String> = Vec::new();
        let mut actions: Vec<String> = Vec::new();
        let mut prefix = String::new();
        let mut suffix = String::new();
        let mut filter: Option<String> = None;
        let mut word = String::new();
        let mut word_seen = false;

        let mut i = 0;
        let mut opts_done = false;
        while i < args.len() {
            let a = args[i].as_str();
            // After `--`, every remaining argument is a non-option; the first
            // becomes the word to complete (even if it begins with `-`, e.g.
            // `compgen -W '-a -b' -- -a`). bash consumes exactly one word.
            if opts_done {
                if !word_seen {
                    word = args[i].clone();
                    word_seen = true;
                }
                i += 1;
                continue;
            }
            match a {
                "--" => opts_done = true,
                // Options taking a following argument.
                "-W" | "-A" | "-P" | "-S" | "-X" | "-F" | "-C" | "-o" | "-G" => {
                    i += 1;
                    let val = args.get(i).cloned();
                    match a {
                        "-W" => {
                            if let Some(v) = val {
                                wordlists.push(v);
                            }
                        }
                        "-A" => {
                            if let Some(v) = val {
                                actions.push(v);
                            }
                        }
                        "-P" => prefix = val.unwrap_or_default(),
                        "-S" => suffix = val.unwrap_or_default(),
                        "-X" => filter = val,
                        // Accepted but not implemented (need a live context).
                        _ => {}
                    }
                }
                "-a" => actions.push("alias".into()),
                "-b" => actions.push("builtin".into()),
                "-c" => actions.push("command".into()),
                "-d" => actions.push("directory".into()),
                "-e" => actions.push("export".into()),
                "-f" => actions.push("file".into()),
                "-g" => actions.push("group".into()),
                "-j" => actions.push("job".into()),
                "-k" => actions.push("keyword".into()),
                "-s" => actions.push("service".into()),
                "-u" => actions.push("user".into()),
                "-v" => actions.push("variable".into()),
                _ => {
                    if !word_seen {
                        word = args[i].clone();
                        word_seen = true;
                    }
                }
            }
            i += 1;
        }

        // ---- gather raw candidates from every specified source ----
        let mut cands: Vec<String> = Vec::new();
        let ifs = self.vars.get("IFS").cloned().unwrap_or_else(|| " \t\n".to_string());
        let ifs_chars: Vec<char> = ifs.chars().collect();
        for wl in &wordlists {
            for tok in wl.split(|c| ifs_chars.contains(&c)).filter(|s| !s.is_empty()) {
                cands.push(tok.to_string());
            }
        }
        for action in &actions {
            match action.as_str() {
                "function" => cands.extend(self.funcs.keys().cloned()),
                "alias" => cands.extend(self.aliases.keys().cloned()),
                "builtin" => cands.extend(BUILTIN_NAMES.iter().map(|s| (*s).to_string())),
                "keyword" => cands.extend(KEYWORDS.iter().map(|s| (*s).to_string())),
                "variable" | "arrayvar" => {
                    cands.extend(self.vars.keys().cloned());
                    cands.extend(self.arrays.keys().cloned());
                    cands.extend(self.assoc.keys().cloned());
                }
                "export" => cands.extend(self.exported.iter().cloned()),
                "command" => {
                    cands.extend(BUILTIN_NAMES.iter().map(|s| (*s).to_string()));
                    cands.extend(KEYWORDS.iter().map(|s| (*s).to_string()));
                    cands.extend(self.funcs.keys().cloned());
                    cands.extend(self.aliases.keys().cloned());
                    cands.extend(self.compgen_path_commands(&word));
                }
                "file" => cands.extend(self.compgen_paths(&word, false)),
                "directory" => cands.extend(self.compgen_paths(&word, true)),
                // group/job/service/user and any unknown action: nothing.
                _ => {}
            }
        }

        // ---- keep candidates that start with the word prefix ----
        let mut list: Vec<String> = cands.into_iter().filter(|c| c.starts_with(&word)).collect();

        // ---- -X filterpat: glob-remove (leading '!' keeps only matches) ----
        if let Some(pat) = &filter
            && !pat.is_empty()
        {
            let extglob = self.shopt.get("extglob").copied().unwrap_or(false);
            let (invert, p) = match pat.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, pat.as_str()),
            };
            let pchars: Vec<char> = p.chars().collect();
            list.retain(|c| {
                let tchars: Vec<char> = c.chars().collect();
                let m = glob_match(&pchars, &tchars, extglob);
                // Default: drop matches. `!pat`: keep only matches.
                if invert { m } else { !m }
            });
        }

        // ---- decorate with -P/-S and emit one per line ----
        let empty = list.is_empty();
        let mut result = String::new();
        for c in &list {
            result.push_str(&prefix);
            result.push_str(c);
            result.push_str(&suffix);
            result.push('\n');
        }
        let write_status = self.write_bytes(out, redir, result.as_bytes());
        // bash: status 1 when no candidates were produced, else the write status.
        if empty { 1 } else { write_status }
    }

    /// Filesystem completion for `compgen -f`/`-d`: treat `word` as a partial
    /// path, list entries of its directory component whose names start with the
    /// basename component, and return each as `dirprefix + name`. `dirs_only`
    /// restricts results to directories (`-d`).
    fn compgen_paths(&self, word: &str, dirs_only: bool) -> Vec<String> {
        // Split into the directory prefix (kept verbatim on each result) and the
        // basename to prefix-match. `foo/ba` -> dir "foo/", base "ba"; "ba" ->
        // dir "" (cwd), base "ba".
        let (dir_prefix, dir_path, base) = match word.rfind('/') {
            Some(idx) => {
                let dp = &word[..=idx];
                let path = if idx == 0 { "/".to_string() } else { word[..idx].to_string() };
                (dp.to_string(), path, word[idx + 1..].to_string())
            }
            None => (String::new(), ".".to_string(), word.to_string()),
        };
        let Ok(rd) = std::fs::read_dir(&dir_path) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&base) {
                continue;
            }
            // Hidden files only appear when the base explicitly starts with '.'.
            if name.starts_with('.') && !base.starts_with('.') {
                continue;
            }
            if dirs_only && !ent.path().is_dir() {
                continue;
            }
            out.push(format!("{dir_prefix}{name}"));
        }
        out
    }

    /// Command-name candidates for `compgen -c`/`-A command`: every executable
    /// basename found on `$PATH`. Scans all PATH directories (the caller's
    /// prefix filter trims the results). On Windows the common executable
    /// extensions are stripped so bare command names are offered.
    fn compgen_path_commands(&self, _word: &str) -> Vec<String> {
        let path = match self.param_value("PATH") {
            Some(p) => p,
            None if !self.env_imported => std::env::var("PATH").unwrap_or_default(),
            None => return Vec::new(),
        };
        let mut out: Vec<String> = Vec::new();
        for dir in std::env::split_paths(&path) {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for ent in rd.flatten() {
                if ent.path().is_dir() {
                    continue;
                }
                let raw = ent.file_name().to_string_lossy().into_owned();
                #[cfg(windows)]
                let name = {
                    let mut n = raw;
                    for ext in [".exe", ".cmd", ".bat", ".com"] {
                        if let Some(stripped) = n.strip_suffix(ext) {
                            n = stripped.to_string();
                            break;
                        }
                    }
                    n
                };
                #[cfg(not(windows))]
                let name = raw;
                out.push(name);
            }
        }
        out
    }

    /// Look up a stored completion spec by target.
    fn comp_get(&self, key: &CompKey) -> Option<&CompSpec> {
        self.comp_specs.iter().find(|(k, _)| k == key).map(|(_, s)| s)
    }

    /// Mutable lookup of a stored completion spec by target.
    fn comp_get_mut(&mut self, key: &CompKey) -> Option<&mut CompSpec> {
        self.comp_specs.iter_mut().find(|(k, _)| k == key).map(|(_, s)| s)
    }

    /// Insert `spec` for `key`, replacing any existing spec in place (so the
    /// insertion order — which drives `complete -p` listing order — is stable
    /// across redefinition, as in bash's hash table).
    fn comp_set(&mut self, key: CompKey, spec: CompSpec) {
        if let Some(slot) = self.comp_specs.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = spec;
        } else {
            self.comp_specs.push((key, spec));
        }
    }

    /// Remove the completion spec for `key` (no-op if absent).
    fn comp_remove(&mut self, key: &CompKey) {
        self.comp_specs.retain(|(k, _)| k != key);
    }

    /// `complete [-abcdefgjksuv] [-pr] [-DEI] [-o option] [-A action]
    /// [-G globpat] [-W wordlist] [-F function] [-C command] [-X filterpat]
    /// [-P prefix] [-S suffix] [name ...]` — register, print (`-p`) or remove
    /// (`-r`) programmable-completion specifications.
    ///
    /// osh's line-oriented REPL has no interactive tab-completion, so a
    /// registered spec is never used to generate matches. The builtin exists so
    /// that scripts sourcing bash completion files (which call `complete`
    /// hundreds of times) run without error, and so `complete -p` round-trips a
    /// definition byte-for-byte. `-D`/`-E`/`-I` target the special
    /// default/empty/initial-word specs. Returns 2 on a usage error, 1 when
    /// `-p` names an unknown spec, else 0.
    ///
    /// Note: with multiple specs, `complete -p` (list all) emits them in
    /// insertion order; bash uses its internal hash-table order, which is not
    /// reproducible. Each individual definition still matches bash exactly.
    fn builtin_complete(&mut self, args: &[String], out: &mut Out, redir: &RedirPlan) -> i32 {
        // Option letters that take an argument (the rest of the cluster, or the
        // next word). Everything else is a no-argument flag.
        const ARG_LETTERS: &str = "oAGWFCXPS";
        const USAGE: &[u8] = b"complete: usage: complete [-abcdefgjksuv] [-pr] [-DEI] [-o option] [-A action] [-G globpat] [-W wordlist] [-F function] [-C command] [-X filterpat] [-P prefix] [-S suffix] [name ...]\n";

        let mut spec = CompSpec::default();
        let mut names: Vec<String> = Vec::new();
        let mut targets: Vec<CompKey> = Vec::new();
        let mut do_print = false;
        let mut do_remove = false;
        let mut has_def = false; // any spec-defining option seen

        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                i += 1;
                while i < args.len() {
                    names.push(args[i].clone());
                    i += 1;
                }
                break;
            }
            if a.len() > 1 && a.starts_with('-') {
                let chars: Vec<char> = a[1..].chars().collect();
                let mut ci = 0;
                while ci < chars.len() {
                    let c = chars[ci];
                    if ARG_LETTERS.contains(c) {
                        // Value: remainder of the cluster if any, else next word.
                        let val = if ci + 1 < chars.len() {
                            chars[ci + 1..].iter().collect::<String>()
                        } else {
                            i += 1;
                            match args.get(i) {
                                Some(v) => v.clone(),
                                None => {
                                    self.errln(&format!(
                                        "{}complete: -{c}: option requires an argument",
                                        self.err_prefix()
                                    ));
                                    self.emit_stderr(USAGE);
                                    return 2;
                                }
                            }
                        };
                        has_def = true;
                        match c {
                            'o' => {
                                if !COMP_O_ORDER.contains(&val.as_str()) {
                                    self.errln(&format!(
                                        "{}complete: {val}: invalid option name",
                                        self.err_prefix()
                                    ));
                                    return 2;
                                }
                                if !spec.o_opts.contains(&val) {
                                    spec.o_opts.push(val);
                                }
                            }
                            'A' => {
                                if !COMP_ACTIONS.iter().any(|(n, _)| *n == val) {
                                    self.errln(&format!(
                                        "{}complete: {val}: invalid action name",
                                        self.err_prefix()
                                    ));
                                    return 2;
                                }
                                if !spec.actions.contains(&val) {
                                    spec.actions.push(val);
                                }
                            }
                            'G' => spec.globpat = Some(val),
                            'W' => spec.wordlist = Some(val),
                            'F' => spec.function = Some(val),
                            'C' => spec.command = Some(val),
                            'X' => spec.filterpat = Some(val),
                            'P' => spec.prefix = Some(val),
                            'S' => spec.suffix = Some(val),
                            _ => {}
                        }
                        ci = chars.len(); // consumed the remainder of the cluster
                    } else {
                        let action = comp_short_action(c);
                        if let Some(act) = action {
                            has_def = true;
                            if !spec.actions.iter().any(|x| x == act) {
                                spec.actions.push(act.to_string());
                            }
                        } else {
                            match c {
                                'p' => do_print = true,
                                'r' => do_remove = true,
                                'D' => targets.push(CompKey::Default),
                                'E' => targets.push(CompKey::Empty),
                                'I' => targets.push(CompKey::Initial),
                                _ => {
                                    self.errln(&format!(
                                        "{}complete: -{c}: invalid option",
                                        self.err_prefix()
                                    ));
                                    self.emit_stderr(USAGE);
                                    return 2;
                                }
                            }
                        }
                        ci += 1;
                    }
                }
                i += 1;
            } else {
                names.push(a.clone());
                i += 1;
            }
        }

        // ---- remove mode (`-r`) ----
        if do_remove {
            if names.is_empty() && targets.is_empty() {
                self.comp_specs.clear();
            } else {
                for n in &names {
                    self.comp_remove(&CompKey::Name(n.clone()));
                }
                for t in &targets {
                    self.comp_remove(t);
                }
            }
            return 0;
        }

        // ---- print mode (`-p`, and bare `complete`) ----
        if do_print || (!has_def && names.is_empty() && targets.is_empty()) {
            if names.is_empty() && targets.is_empty() {
                let mut s = String::new();
                for (k, sp) in &self.comp_specs {
                    s.push_str(&format_compspec(k, sp));
                }
                return self.write_bytes(out, redir, s.as_bytes());
            }
            let mut keys: Vec<CompKey> = Vec::new();
            keys.extend(targets.iter().cloned());
            keys.extend(names.iter().map(|n| CompKey::Name(n.clone())));
            let mut status = 0;
            for k in keys {
                match self.comp_get(&k).map(|sp| format_compspec(&k, sp)) {
                    Some(line) => {
                        self.write_bytes(out, redir, line.as_bytes());
                    }
                    None => {
                        self.errln(&format!(
                            "{}complete: {}: no completion specification",
                            self.err_prefix(),
                            comp_key_label(&k)
                        ));
                        status = 1;
                    }
                }
            }
            return status;
        }

        // ---- define mode ----
        if names.is_empty() && targets.is_empty() {
            // Defining options given but nowhere to attach them.
            self.emit_stderr(USAGE);
            return 2;
        }
        // With -D/-E/-I present, bash ignores any command names.
        if targets.is_empty() {
            for n in &names {
                self.comp_set(CompKey::Name(n.clone()), spec.clone());
            }
        } else {
            for t in &targets {
                self.comp_set(t.clone(), spec.clone());
            }
        }
        0
    }

    /// `compopt [-o|+o option] [-DEI] [name ...]` — modify the `-o` options of an
    /// existing completion spec (`-o` adds, `+o` removes). With no name, bash
    /// errors unless invoked from within a running completion function; osh has
    /// no such context, so a nameless `compopt` always reports that error.
    /// Returns 2 on a usage error, 1 when a named spec does not exist, else 0.
    fn builtin_compopt(&mut self, args: &[String]) -> i32 {
        const USAGE: &[u8] = b"compopt: usage: compopt [-o|+o option] [-DEI] [name ...]\n";
        let mut add: Vec<String> = Vec::new();
        let mut del: Vec<String> = Vec::new();
        let mut targets: Vec<CompKey> = Vec::new();
        let mut names: Vec<String> = Vec::new();

        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "--" => {
                    i += 1;
                    while i < args.len() {
                        names.push(args[i].clone());
                        i += 1;
                    }
                    break;
                }
                "-o" | "+o" => {
                    i += 1;
                    let Some(v) = args.get(i) else {
                        self.errln(&format!(
                            "{}compopt: {a}: option requires an argument",
                            self.err_prefix()
                        ));
                        self.emit_stderr(USAGE);
                        return 2;
                    };
                    if !COMP_O_ORDER.contains(&v.as_str()) {
                        self.errln(&format!(
                            "{}compopt: {v}: invalid option name",
                            self.err_prefix()
                        ));
                        return 2;
                    }
                    if a == "-o" {
                        add.push(v.clone());
                    } else {
                        del.push(v.clone());
                    }
                }
                "-D" => targets.push(CompKey::Default),
                "-E" => targets.push(CompKey::Empty),
                "-I" => targets.push(CompKey::Initial),
                other if other.len() > 1 && other.starts_with('-') => {
                    let c = other.chars().nth(1).unwrap_or('?');
                    self.errln(&format!("{}compopt: -{c}: invalid option", self.err_prefix()));
                    self.emit_stderr(USAGE);
                    return 2;
                }
                _ => names.push(a.clone()),
            }
            i += 1;
        }

        let mut keys: Vec<CompKey> = Vec::new();
        keys.extend(targets.iter().cloned());
        keys.extend(names.iter().map(|n| CompKey::Name(n.clone())));

        if keys.is_empty() {
            // osh is never inside a completion function.
            self.errln(&format!(
                "{}compopt: not currently executing completion function",
                self.err_prefix()
            ));
            return 1;
        }

        let mut status = 0;
        for k in keys {
            if self.comp_get(&k).is_none() {
                self.errln(&format!(
                    "{}compopt: {}: no completion specification",
                    self.err_prefix(),
                    comp_key_label(&k)
                ));
                status = 1;
                continue;
            }
            if let Some(sp) = self.comp_get_mut(&k) {
                for o in &add {
                    if !sp.o_opts.contains(o) {
                        sp.o_opts.push(o.clone());
                    }
                }
                sp.o_opts.retain(|o| !del.contains(o));
            }
        }
        status
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
            let is_bi = self.builtin_enabled(name);
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
            // A command remembered in the hash table counts as found even when a
            // fresh PATH search comes up empty (bash reports it as hashed).
            let is_hashed = self.cmd_hash.contains_key(name.as_str());
            let found = is_kw || is_fn || is_bi || !files.is_empty() || is_hashed;
            if !found {
                if !mode_t && !mode_p && !mode_pp {
                    self.errln(&format!("{}type: {name}: not found", self.err_prefix()));
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
                } else if let Some((p, _)) = self.cmd_hash.get(name.as_str()) {
                    // A hashed command with no live PATH match still prints its
                    // remembered path.
                    let p = p.to_string_lossy().into_owned();
                    let _ = self.write_line(out, redir, &p);
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
                    if let Some(body) = self.funcs.get(name) {
                        let src = crate::unparse::unparse_function(name, body);
                        let _ = self.write_bytes(out, redir, src.as_bytes());
                    }
                }
                if is_bi {
                    let _ = self.write_line(out, redir, &format!("{name} is a shell builtin"));
                }
                for f in &files {
                    let _ =
                        self.write_line(out, redir, &format!("{name} is {}", f.to_string_lossy()));
                }
            } else {
                if is_kw {
                    let _ = self.write_line(out, redir, &format!("{name} is a shell keyword"));
                } else if is_fn {
                    // bash prints the "is a function" line followed by the
                    // reconstructed function source.
                    let _ = self.write_line(out, redir, &format!("{name} is a function"));
                    if let Some(body) = self.funcs.get(name) {
                        let src = crate::unparse::unparse_function(name, body);
                        let _ = self.write_bytes(out, redir, src.as_bytes());
                    }
                } else if is_bi {
                    let _ = self.write_line(out, redir, &format!("{name} is a shell builtin"));
                } else if let Some((p, _)) = self.cmd_hash.get(name.as_str()) {
                    // A previously-run command is remembered in the hash table.
                    let _ = self
                        .write_line(out, redir, &format!("{name} is hashed ({})", p.to_string_lossy()));
                } else {
                    let _ =
                        self.write_line(out, redir, &format!("{name} is {}", files[0].to_string_lossy()));
                }
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
                self.errln(&format!("{}[: missing ']'", self.err_prefix()));
                return 2;
            }
        }
        // `-v NAME` needs shell state (is the variable set?), which the free
        // `eval_test` helper cannot see — handle it here.
        if a.len() == 2 && a[0] == "-v" {
            return i32::from(!self.var_is_set(a[1]));
        }
        // `-o OPTNAME` likewise needs shell state (the option flags).
        if a.len() == 2 && a[0] == "-o" {
            return i32::from(!self.shell_option_enabled(a[1]));
        }
        match eval_test(&a) {
            Ok(b) => i32::from(!b),
            Err(operand) => {
                self.errln(&format!("{}{name}: {operand}: integer expression expected", self.err_prefix()));
                2
            }
        }
    }

    // ---- output helpers -----------------------------------------------------

    fn write_line(&mut self, out: &mut Out, redir: &RedirPlan, line: &str) -> i32 {
        let mut s = line.to_string();
        s.push('\n');
        self.write_bytes(out, redir, s.as_bytes())
    }

    fn write_bytes(&mut self, out: &mut Out, redir: &RedirPlan, bytes: &[u8]) -> i32 {
        // `echo msg >&3` on the builtin: fd 1 is a user-space write descriptor.
        if let Some(n) = redir.stdout_to_fd
            && redir.stdout.is_none()
        {
            return self.write_to_fd(n, bytes);
        }
        // `1>&2` on the builtin (e.g. `echo msg >&2`): the builtin's stdout is
        // the current stderr sink, not the ambient stdout.
        if redir.stdout_to_stderr && redir.stdout.is_none() {
            // When the same command also redirects its own stderr
            // (`echo msg >&2 2>file`), bash applies redirections left to right:
            // the `>&2` dup captures fd 2 *before* the `2>file` takes effect, so
            // fd 1 follows the pre-redirect (enclosing/inherited) stderr, not the
            // file. Our resolver only sets `stdout_to_stderr` for that dup-first
            // ordering (a `2>file >&2` sequence copies the file into `stdout`
            // instead), so when a per-command stderr redirect is present it is the
            // freshly-pushed top of `stderr_stack` — skip it. See TD-OILS14.
            let skip_top =
                redir.stderr.is_some() || redir.stderr_to_fd.is_some() || redir.stderr_to_stdout;
            let depth = if skip_top {
                self.stderr_stack.len().saturating_sub(1)
            } else {
                self.stderr_stack.len()
            };
            self.emit_stderr_depth(bytes, depth);
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
                    self.errln(&format!("{}{path}: {e}", self.err_prefix()));
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
                    // A persistent `exec > file` rebinds the shell's ambient
                    // fd 1 to a file; otherwise write to the real stdout.
                    if let Some(f) = &self.exec_stdout {
                        match f.try_clone() {
                            Ok(mut fc) => {
                                if fc.write_all(bytes).is_err() {
                                    return 1;
                                }
                                0
                            }
                            Err(_) => 1,
                        }
                    } else {
                        let stdout = io::stdout();
                        let mut lock = stdout.lock();
                        if lock.write_all(bytes).is_err() {
                            return 1;
                        }
                        let _ = lock.flush();
                        0
                    }
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

    /// Write bytes to a user-space write descriptor (`>&N`, N ≥ 3) opened by
    /// `exec N> file`. A `try_clone` of the shared handle is used so the write
    /// goes to the descriptor's current OS offset. An unopened fd is a status-1
    /// `N: bad file descriptor` (bash).
    /// Snapshot standard fd `n` (1 or 2) into an owned [`File`] for an
    /// `exec 3>&1` / `exec 3>&2` alias. If fd `n` is currently redirected to a
    /// file (`exec > file` / `exec 2> file`), duplicate that live handle so the
    /// alias writes to the same file at the shared offset; otherwise duplicate
    /// the real terminal handle. This captures the *current* sink (bash's
    /// dup-at-exec-time semantics), so a later `exec > other` does not retarget
    /// the alias.
    fn snapshot_std_fd(&self, n: i32) -> io::Result<std::fs::File> {
        let redirected = if n == 1 {
            self.exec_stdout.as_ref()
        } else {
            self.exec_stderr.as_ref()
        };
        match redirected {
            Some(f) => f.try_clone(),
            None => dup_std_handle(n == 1),
        }
    }

    fn write_to_fd(&mut self, fd: i32, bytes: &[u8]) -> i32 {
        match self.open_write_fds.get(&fd) {
            Some(f) => match f.try_clone() {
                Ok(mut fc) => {
                    if fc.write_all(bytes).is_err() {
                        return 1;
                    }
                    0
                }
                Err(_) => 1,
            },
            None => {
                self.errln(&format!("{}{fd}: Bad file descriptor", self.err_prefix()));
                1
            }
        }
    }

    /// Write raw bytes to the current stderr (fd 2) — the innermost active
    /// [`StderrTarget`], or the shell's real stderr when none is active. Used
    /// for command diagnostics and `>&2` builtin output so both honour a
    /// compound command's `2>` redirect.
    fn emit_stderr(&self, bytes: &[u8]) {
        self.emit_stderr_depth(bytes, self.stderr_stack.len());
    }

    /// Like [`emit_stderr`], but only consider the first `depth` entries of the
    /// `stderr_stack` when choosing the sink. `depth == stderr_stack.len()` is
    /// the normal case; a smaller depth lets `>&2` on a command that also has
    /// its own `2>file` redirect skip that just-pushed per-command stderr and
    /// target the *pre-redirect* sink (the dup-first ordering — see TD-OILS14).
    fn emit_stderr_depth(&self, bytes: &[u8], depth: usize) {
        match self.stderr_stack.get(..depth).and_then(<[_]>::last) {
            None => {
                // Base fd 2: a persistent `exec 2> file` target if set, else the
                // shell's real stderr.
                if let Some(f) = &self.exec_stderr {
                    if let Ok(mut fc) = f.try_clone() {
                        let _ = fc.write_all(bytes);
                    }
                } else {
                    let e = io::stderr();
                    let mut lock = e.lock();
                    let _ = lock.write_all(bytes);
                    let _ = lock.flush();
                }
            }
            Some(StderrTarget::Stdout) => {
                // fd 2 follows fd 1: a persistent `exec > file` target if set,
                // else the real stdout.
                if let Some(f) = &self.exec_stdout {
                    if let Ok(mut fc) = f.try_clone() {
                        let _ = fc.write_all(bytes);
                    }
                } else {
                    let o = io::stdout();
                    let mut lock = o.lock();
                    let _ = lock.write_all(bytes);
                    let _ = lock.flush();
                }
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
            Some(StderrTarget::WriteFd(f)) => {
                let _ = (&**f).write_all(bytes);
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

    /// The diagnostic prefix bash prepends to every shell error message. In a
    /// non-interactive shell (`-c` or a script) this is `<name>: line <N>: `,
    /// where `<name>` is `$0` (the `-c` pseudo-name `osh`, or the script path)
    /// and `<N>` is the current source line (`$LINENO`); in interactive mode
    /// bash omits the `line <N>:` component. Centralising the prefix here means
    /// every error site reports the correct source name (so a script's errors
    /// name the script, not a hard-coded `osh:`) and the line number, matching
    /// bash's `error_prefix()`.
    ///
    /// Note: bash uses the magic source name `environment` for functions defined
    /// in a `-c` string; osh deliberately keeps its own `$0`-based name instead,
    /// since byte-matching bash's name is impossible anyway (osh's `$0` is `osh`,
    /// not `bash`) and osh's own name is the more meaningful diagnostic.
    fn err_prefix(&self) -> String {
        if self.command_mode || self.script_mode {
            format!("{}: line {}: ", self.name, self.current_line)
        } else {
            format!("{}: ", self.name)
        }
    }

    /// Emit an arithmetic diagnostic in bash's form: `<name>: line N:
    /// [<builtin>: ]<expr>: <body> (error token is "…")`. `expr` is the
    /// (already parameter-expanded) arithmetic source; bash echoes it with
    /// leading whitespace trimmed, then the [`arith::ArithError`]'s body. When
    /// the arithmetic ran inside a builtin (`let`/`((`/`declare`/…), that
    /// builtin's name is inserted as a tag (see [`Shell::arith_cmd`]). Routed
    /// through `errln` so an active `2>`/`2>&1` redirect on the enclosing
    /// command silences it, as in bash.
    fn emit_arith_error(&mut self, expr: &str, e: &arith::ArithError) {
        let prefix = self.err_prefix();
        let expr = expr.trim_start();
        match self.arith_cmd {
            Some(tag) => self.errln(&format!("{prefix}{tag}: {expr}: {e}")),
            None => self.errln(&format!("{prefix}{expr}: {e}")),
        }
    }

    /// Write a command-level diagnostic (e.g. `foo: command not found`, or a
    /// special-builtin usage error) to the sink fd 2 resolves to **for this
    /// command**, honouring its own `2>file`/`2>/dev/null` (`redir.stderr`),
    /// `2>&1` (`redir.stderr_to_stdout`) and `2>&N` (`redir.stderr_to_fd`)
    /// redirects before falling back to the enclosing stderr sink. bash sends
    /// these messages to the command's redirected stderr — not the shell's — so
    /// `nosuchcmd 2>/dev/null` is silent.
    ///
    /// Use this (rather than the stderr-stack-only `errln`) wherever a command's
    /// own `RedirPlan` has not been installed onto the `stderr_stack`: the
    /// external-command path applies fd 2 to the child via `std::process::
    /// Command` (so a spawn failure must reproduce that routing here), and the
    /// `command`/`builtin` wrappers emit their own usage diagnostics before
    /// delegating, without pushing a scoped stderr redirect.
    fn emit_cmd_stderr(&mut self, out: &mut Out, redir: &RedirPlan, msg: &str) {
        let mut bytes = msg.as_bytes().to_vec();
        bytes.push(b'\n');
        if let Some((path, append)) = &redir.stderr {
            if let Ok(mut f) = open_out(path, *append) {
                let _ = f.write_all(&bytes);
                return;
            }
            // On open failure, fall through to the enclosing sink.
        } else if let Some(n) = redir.stderr_to_fd {
            let _ = self.write_to_fd(n, &bytes);
            return;
        } else if redir.stderr_to_stdout {
            // fd 2 follows fd 1: route to this command's stdout destination.
            match out {
                Out::Capture(buf) => {
                    buf.extend_from_slice(&bytes);
                    return;
                }
                Out::Pipe(w) => {
                    let _ = (&*w).write_all(&bytes);
                    return;
                }
                Out::Inherit => {
                    if let Some(f) = &self.exec_stdout
                        && let Ok(mut fc) = f.try_clone()
                    {
                        let _ = fc.write_all(&bytes);
                        return;
                    }
                    let o = io::stdout();
                    let mut lock = o.lock();
                    let _ = lock.write_all(&bytes);
                    let _ = lock.flush();
                    return;
                }
            }
        }
        self.emit_stderr(&bytes);
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
            Some(StderrTarget::WriteFd(f)) => f
                .try_clone()
                .map(Stdio::from)
                .map_err(|e| format!("stderr: {e}")),
        }
    }

    /// Resolve the byte cursor backing an input descriptor for a `<&N` dup:
    /// `exec_stdin` for fd 0, the `open_fds` table for fd ≥ 3. `None` when the
    /// descriptor is not bound (read yields immediate EOF, as bash does for a
    /// closed source).
    fn input_fd_cursor(&self, n: i32) -> Option<&RefCell<io::Cursor<Vec<u8>>>> {
        if n == 0 {
            self.exec_stdin.as_ref()
        } else {
            self.open_fds.get(&n)
        }
    }

    /// Non-consuming availability check for `read -t 0`: does a read from this
    /// source proceed without blocking? Bash's `read -t 0` returns success (0)
    /// when input is available or the source is at EOF, and failure (non-zero)
    /// only when a read would actually block.
    ///
    /// Seekable/buffered sources — here-strings/here-docs, file redirects, an
    /// fd cursor (`<&N` / `-u N`), and a persistent `exec <` cursor — are always
    /// "ready" (bash returns 0 even for an empty file or at EOF). A live
    /// upstream pipe is ready only when it already holds buffered bytes; an
    /// interactive terminal with no pending keystroke is treated as would-block.
    ///
    /// Known limitation: a pipe sitting exactly at EOF (writer closed, buffer
    /// drained) reports would-block here, where bash reports ready — a precise
    /// answer needs a non-blocking peek/`select` on the pipe fd. Likewise an
    /// interactive tty with a keystroke already queued is reported as
    /// would-block. Both are rare and documented rather than mis-consuming input.
    fn input_available_now(&self, stdin: &StdinSrc, redir: &RedirPlan) -> bool {
        if redir.stdin_data.is_some() || redir.stdin.is_some() || redir.stdin_from_fd.is_some() {
            return true;
        }
        match stdin {
            StdinSrc::Cursor(_) => true,
            StdinSrc::Inherit => self.exec_stdin.is_some() || stdin_readable_now(),
            StdinSrc::Pipe(r) => {
                let g = r.borrow();
                // Bytes already pulled into the BufReader are unconditionally
                // ready; otherwise probe the underlying OS pipe (data queued or
                // writer-closed EOF ⇒ ready; empty live pipe ⇒ would block).
                !g.buffer().is_empty() || pipe_reader_readable_now(g.get_ref())
            }
        }
    }

    fn read_line(&self, stdin: &StdinSrc, redir: &RedirPlan) -> Option<(String, bool)> {
        if let Some(n) = redir.stdin_from_fd {
            // `read <&N`: read from and advance descriptor N's shared source — a
            // live coproc pipe, or a `read -u`/`exec 3<` byte cursor.
            if let Some(rd) = self.coproc_read_fds.get(&n) {
                return read_one_line(&mut *rd.borrow_mut());
            }
            let c = self.input_fd_cursor(n)?;
            return read_one_line(&mut *c.borrow_mut());
        }
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
                // A persistent `exec < file` rebinds the shell's ambient fd 0.
                if let Some(cur) = &self.exec_stdin {
                    return read_one_line(&mut *cur.borrow_mut());
                }
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
        if let Some(n) = redir.stdin_from_fd {
            if let Some(rd) = self.coproc_read_fds.get(&n) {
                return read_record(&mut *rd.borrow_mut(), delim, nchars, exact);
            }
            let c = self.input_fd_cursor(n)?;
            return read_record(&mut *c.borrow_mut(), delim, nchars, exact);
        }
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
                if let Some(cur) = &self.exec_stdin {
                    return read_record(&mut *cur.borrow_mut(), delim, nchars, exact);
                }
                let stdin = io::stdin();
                let mut lock = stdin.lock();
                read_record(&mut lock, delim, nchars, exact)
            }
        }
    }
}

/// Let the arithmetic evaluator read shell variables.
impl VarLookup for Shell {
    fn get_str(&self, name: &str) -> Option<String> {
        // Return the raw value string; the arithmetic evaluator recursively
        // evaluates it (`b=a; a=5; $((b))` → 5), including octal/hex literals.
        self.param_value(name)
    }

    fn get_index_str(&self, name: &str, index: i64) -> Option<String> {
        // `array_element` already applies bash negative-index semantics.
        self.array_element(name, index)
    }

    fn is_assoc(&self, name: &str) -> bool {
        self.assoc.contains_key(name)
    }

    fn get_assoc_str(&self, name: &str, key: &str) -> Option<String> {
        // An unset key (or empty value) evaluates to 0; a non-empty value is
        // recursively arithmetic-evaluated by the caller.
        self.assoc_element(name, key)
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
    /// `<&N` — fd 0 is duplicated from input descriptor N (`read <&3`,
    /// `cat <&$r`). The command reads from — and advances — the shared cursor
    /// in [`Shell::open_fds`] (or [`Shell::exec_stdin`] for N == 0), matching
    /// bash's shared-offset dup. Takes precedence over `stdin` / `stdin_data`.
    stdin_from_fd: Option<i32>,
    stdout: Option<(String, bool)>,
    stderr: Option<(String, bool)>,
    /// True when `stderr`'s file target is a *dup* of `stdout`'s (from `&>file`,
    /// `>file 2>&1`, `2>file 1>&2`, or `1>&file`) — the two fds share one open
    /// file description and therefore one offset, so their writes interleave.
    /// False when `stdout` and `stderr` name the same path via two *independent*
    /// redirects (`>f 2>f`): bash opens each separately (each truncates to
    /// offset 0), so the writes clobber rather than interleave.
    stderr_shares_stdout: bool,
    /// `2>&1` — fd 2 follows fd 1 (stderr goes wherever stdout currently goes).
    /// Distinct from `stderr` (a file path) so the merge works even when stdout
    /// is a pipe/terminal/capture rather than a file.
    stderr_to_stdout: bool,
    /// `1>&2` — fd 1 follows fd 2 (stdout goes wherever stderr currently goes).
    stdout_to_stderr: bool,
    /// `1>&N` / `>&N` with N ≥ 3 — fd 1 is duplicated onto a user-space *write*
    /// descriptor previously opened by `exec N> file` (see
    /// [`Shell::open_write_fds`]). A builtin's stdout / an external child's fd 1
    /// is routed to that descriptor's file. `None` = no such dup.
    stdout_to_fd: Option<i32>,
    /// `2>&N` with N ≥ 3 — fd 2 duplicated onto a user-space write descriptor.
    stderr_to_fd: Option<i32>,
    /// Redirections to descriptors other than 0/1/2 (`3< file`, `4> log`,
    /// `4<&-`, …). Only the `exec` builtin currently consumes these, installing
    /// them in the shell's persistent [`Shell::open_fds`] / [`Shell::open_write_fds`]
    /// tables; on any other command they are ignored (a documented limitation —
    /// scoped per-command extra fds are not yet modelled).
    extra_fds: Vec<(i32, ExtraFdOp)>,
}

impl RedirPlan {
    /// True when the plan carries a redirect that [`Shell::exec_with_redirects`]
    /// can install for a whole command body (stdin source, stdout/stderr file or
    /// stream merge, or a scoped fd ≥ 3). Used to decide whether a function
    /// invocation (`myfunc > file`) must run inside a redirect scope; `stdout_to_fd`
    /// / `stderr_to_fd` (dup onto an `exec`-opened write descriptor) are *not*
    /// covered here — those are applied per-builtin/-external, not body-wide.
    fn needs_scope(&self) -> bool {
        self.stdin.is_some()
            || self.stdin_data.is_some()
            || self.stdin_from_fd.is_some()
            || self.stdout.is_some()
            || self.stderr.is_some()
            || self.stderr_to_stdout
            || self.stdout_to_stderr
            || !self.extra_fds.is_empty()
    }
}

/// A saved binding of one non-standard descriptor while a compound command's
/// scoped redirect (`{ …; } 3< file`) is in effect: `(fd, prior input cursor,
/// prior write handle)`. Both prior slots are taken by ownership out of the
/// shell's fd tables and reinstated when the body finishes.
type SavedFd = (
    i32,
    Option<RefCell<io::Cursor<Vec<u8>>>>,
    Option<std::sync::Arc<std::fs::File>>,
);

/// An operation on a non-standard file descriptor (fd ≥ 3), captured by
/// [`Shell::resolve_redirects`] and applied to the shell's fd tables by `exec`.
#[derive(Debug, Clone)]
enum ExtraFdOp {
    /// Open fd N for reading from these bytes — the contents of a `< file`
    /// redirect or a here-document / here-string body.
    InputBytes(Vec<u8>),
    /// Open fd N for writing to `path` (`N> file` / `N>> file`); the `bool` is
    /// the append flag.
    OutputFile(String, bool),
    /// Alias fd N to a standard write fd (`exec 3>&1` / `exec 3>&2`): the inner
    /// value is the target standard fd (`1` or `2`). At apply time the target's
    /// *current* sink is duplicated into fd N (a snapshot, matching bash's dup
    /// semantics — a later `exec > file` does not retarget the alias).
    AliasStd(i32),
    /// Close fd N (`N<&-` / `N>&-`).
    Close,
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

#[allow(clippy::too_many_arguments)]
fn glob_or_literal(
    field: &[EChar],
    out: &mut Vec<String>,
    nullglob: bool,
    failglob: bool,
    dotglob: bool,
    nocaseglob: bool,
    extglob: bool,
    globstar: bool,
    globignore_active: bool,
    globignore: &[GlobIgnorePat],
    failed: &mut Option<String>,
) {
    let has_meta = field_has_glob_meta(field, extglob);
    let literal: String = field.iter().map(|e| e.c).collect();
    if !has_meta {
        out.push(literal);
        return;
    }
    // A non-null GLOBIGNORE enables a dotglob-like effect for this expansion.
    let effective_dotglob = dotglob || globignore_active;
    let mut matches = glob_expand_field(field, effective_dotglob, nocaseglob, extglob, globstar);
    if globignore_active {
        // Drop names matching any GLOBIGNORE pattern, and always drop the `.`
        // and `..` entries (bash ignores them whenever GLOBIGNORE is non-null,
        // even for a leading-dot pattern like `.*`).
        matches.retain(|m| {
            let base = m.rsplit('/').next().unwrap_or(m.as_str());
            base != "." && base != ".." && !globignore.iter().any(|p| p.matches_path(m))
        });
    }
    if matches.is_empty() {
        // `failglob` takes precedence over `nullglob`: an unmatched pattern is a
        // reported error that aborts the command (bash). Record the first
        // unmatched pattern so the caller can raise it. Otherwise, `nullglob`
        // removes the word entirely and the default leaves it literal.
        if failglob {
            if failed.is_none() {
                *failed = Some(literal);
            }
        } else if !nullglob {
            out.push(literal);
        }
    } else {
        matches.sort();
        out.append(&mut matches);
    }
}

/// A single compiled `GLOBIGNORE` pattern, split into one glob token list per
/// `/`-separated path component. Matching is pathname-style: a candidate matches
/// iff it has the same number of components and each component matches the
/// corresponding token list (so `*`/`?`/`[]` never cross a `/`, mirroring the
/// per-component semantics of ordinary pathname expansion). This is how bash
/// tests names against `GLOBIGNORE` — `*.log` ignores `c.log` but not `sub/e.log`.
struct GlobIgnorePat {
    comps: Vec<Vec<PatTok>>,
}

impl GlobIgnorePat {
    /// Whether `path` matches this pattern with pathname-style semantics.
    fn matches_path(&self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != self.comps.len() {
            return false;
        }
        parts.iter().zip(&self.comps).all(|(seg, toks)| {
            let chars: Vec<char> = seg.chars().collect();
            match_glob_toks(toks, &chars)
        })
    }
}

/// Compile a `GLOBIGNORE` variable value (a `:`-separated list) into a set of
/// [`GlobIgnorePat`]s. Empty entries (e.g. a leading/trailing/doubled `:`) are
/// skipped. Each pattern's characters are treated as unquoted glob text.
fn build_globignore(value: &str, extglob: bool) -> Vec<GlobIgnorePat> {
    value
        .split(':')
        .filter(|p| !p.is_empty())
        .map(|pat| {
            let comps = pat
                .split('/')
                .map(|comp| {
                    let echars: Vec<EChar> =
                        comp.chars().map(|c| EChar { c, quoted: false }).collect();
                    compile_glob(&echars, extglob)
                })
                .collect();
            GlobIgnorePat { comps }
        })
        .collect()
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
    /// A POSIX character class such as `[:space:]` (stored as the name between
    /// the inner colons, e.g. `"space"`). Matched by [`posix_class_matches`].
    Posix(String),
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
        // POSIX character class `[:name:]` (e.g. `[[:space:]]`). Only valid
        // when the bracket is immediately followed by a colon; scan to the
        // closing `:]`. If no terminator is found, fall through and treat the
        // `[` literally.
        if c == '['
            && matches!(comp.get(i + 1).map(|e| e.c), Some(':'))
            && let Some(end) = (i + 2..comp.len()).find(|&k| {
                comp[k].c == ':' && matches!(comp.get(k + 1).map(|e| e.c), Some(']'))
            })
        {
            let name: String = comp[i + 2..end].iter().map(|e| e.c).collect();
            items.push(ClassItem::Posix(name));
            first = false;
            i = end + 2; // past `:]`
            continue;
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
        ClassItem::Posix(name) => posix_class_matches(name, ch),
    })
}

/// Whether `ch` belongs to the POSIX character class `name` (the text between
/// the inner colons of `[:name:]`). Unknown class names match nothing, matching
/// bash's behavior of treating an unrecognized class as an empty set.
fn posix_class_matches(name: &str, ch: char) -> bool {
    match name {
        "alnum" => ch.is_alphanumeric(),
        "alpha" => ch.is_alphabetic(),
        "blank" => ch == ' ' || ch == '\t',
        "cntrl" => ch.is_control(),
        "digit" => ch.is_ascii_digit(),
        "graph" => !ch.is_whitespace() && !ch.is_control(),
        "lower" => ch.is_lowercase(),
        "print" => !ch.is_control(),
        "punct" => ch.is_ascii_punctuation(),
        "space" => ch.is_whitespace(),
        "upper" => ch.is_uppercase(),
        "xdigit" => ch.is_ascii_hexdigit(),
        _ => false,
    }
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
fn param_case(
    value: &str,
    pattern: &[char],
    mode: crate::ast::CaseMode,
    all: bool,
    extglob: bool,
) -> String {
    use crate::ast::CaseMode;
    // An empty pattern matches every character (bash: `^^`/`,,`/`~~` with no
    // pattern transforms the whole value).
    let matches_char = |ch: char| pattern.is_empty() || glob_match(pattern, &[ch], extglob);
    let convert = |ch: char| {
        // `char::to_uppercase`/`to_lowercase` can yield multiple chars
        // (e.g. 'ß' → "SS"); bash uses towupper/towlower per rune, but the
        // multi-char expansion is the closest correct Unicode behavior.
        match mode {
            CaseMode::Upper => ch.to_uppercase().collect::<String>(),
            CaseMode::Lower => ch.to_lowercase().collect::<String>(),
            // Toggle: upper-case letters become lower-case and vice versa;
            // characters that are neither are left unchanged.
            CaseMode::Toggle => {
                if ch.is_uppercase() {
                    ch.to_lowercase().collect::<String>()
                } else if ch.is_lowercase() {
                    ch.to_uppercase().collect::<String>()
                } else {
                    ch.to_string()
                }
            }
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
/// transform). Values with control characters use ANSI-C `$'…'` quoting;
/// every other non-empty value is single-quoted (with embedded single quotes
/// escaped as `'\''`), and the empty string becomes `''`.
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
/// pseudo signals in bash's order (`DEBUG`, `ERR`, `RETURN`) — used to order
/// `trap -p` output deterministically.
fn sigspec_order(spec: &str) -> u16 {
    match spec {
        "EXIT" => 0,
        "DEBUG" => 200,
        "ERR" => 201,
        "RETURN" => 202,
        _ => SIGNALS
            .iter()
            .find(|(_, name)| *name == spec)
            .map_or(255, |(num, _)| u16::from(*num)),
    }
}

/// Render a normalized trap spec as bash's `trap -p` display name: real signals
/// carry a `SIG` prefix (`INT` → `SIGINT`), while the pseudo-signals
/// (`EXIT`/`ERR`/`DEBUG`/`RETURN`) are shown bare.
fn sigspec_display(spec: &str) -> String {
    match spec {
        "EXIT" | "ERR" | "DEBUG" | "RETURN" => spec.to_string(),
        _ => format!("SIG{spec}"),
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

/// The canonical completion-action name for a single-letter `complete` flag
/// (`-a` → `alias`, `-v` → `variable`, …), or `None` if the letter is not an
/// action shortcut.
fn comp_short_action(c: char) -> Option<&'static str> {
    COMP_ACTIONS.iter().find(|(_, flag)| *flag == c).map(|(name, _)| *name)
}

/// The label used for a completion target in diagnostics (`complete -p foo`
/// error → `foo`; the specials print as their flag).
fn comp_key_label(k: &CompKey) -> String {
    match k {
        CompKey::Name(n) => n.clone(),
        CompKey::Default => "-D".to_string(),
        CompKey::Empty => "-E".to_string(),
        CompKey::Initial => "-I".to_string(),
    }
}

/// Render a completion spec as a re-executable `complete …` line (matching
/// bash's `complete -p` output), terminated by a newline. Option groups are
/// emitted in bash's fixed print order: `-o` options, shortcut actions, `-A`
/// actions, then `-G -W -P -S -X -C -F`, then the target (`name` or `-D/-E/-I`).
fn format_compspec(key: &CompKey, sp: &CompSpec) -> String {
    let mut s = String::from("complete");
    // -o options, in canonical order.
    for o in COMP_O_ORDER {
        if sp.o_opts.iter().any(|x| x == o) {
            s.push_str(" -o ");
            s.push_str(o);
        }
    }
    // Actions: shortcut-flag actions first (in table order), then -A-only ones.
    for &(name, flag) in COMP_ACTIONS {
        if flag != '\0' && sp.actions.iter().any(|x| x == name) {
            s.push_str(" -");
            s.push(flag);
        }
    }
    for &(name, flag) in COMP_ACTIONS {
        if flag == '\0' && sp.actions.iter().any(|x| x == name) {
            s.push_str(" -A ");
            s.push_str(name);
        }
    }
    if let Some(v) = &sp.globpat {
        s.push_str(" -G ");
        s.push_str(&single_quote(v));
    }
    if let Some(v) = &sp.wordlist {
        s.push_str(" -W ");
        s.push_str(&single_quote(v));
    }
    if let Some(v) = &sp.prefix {
        s.push_str(" -P ");
        s.push_str(&single_quote(v));
    }
    if let Some(v) = &sp.suffix {
        s.push_str(" -S ");
        s.push_str(&single_quote(v));
    }
    if let Some(v) = &sp.filterpat {
        s.push_str(" -X ");
        s.push_str(&single_quote(v));
    }
    if let Some(v) = &sp.command {
        s.push_str(" -C ");
        s.push_str(&single_quote(v));
    }
    // bash prints the -F function name unquoted.
    if let Some(v) = &sp.function {
        s.push_str(" -F ");
        s.push_str(v);
    }
    match key {
        CompKey::Name(n) => {
            s.push(' ');
            s.push_str(n);
        }
        CompKey::Default => s.push_str(" -D"),
        CompKey::Empty => s.push_str(" -E"),
        CompKey::Initial => s.push_str(" -I"),
    }
    s.push('\n');
    s
}

/// Apply bash's capitalize attribute (`declare -c`, `att_capcase`): the first
/// character is uppercased and every remaining character lowercased, so
/// `hELLO` → `Hello` and `hello world` → `Hello world`. Uses Unicode-aware
/// case mapping (a single source char may map to several).
fn capcase(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        out.extend(first.to_uppercase());
        for c in chars {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// Quote `s` the way bash's `printf %q` does: an empty string becomes `''`, a
/// string with control characters uses the ANSI-C `$'…'` form, and otherwise
/// each shell-special character is backslash-escaped (bash uses backslash
/// escaping for `%q`, unlike `${v@Q}`/`shell_quote` which single-quote). The
/// result re-parses to the original word.
fn printf_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars().any(char::is_control) {
        // Reuse the ANSI-C form (matches bash, which emits `$'…'` here too).
        return shell_quote(s);
    }
    let mut out = String::new();
    for c in s.chars() {
        // Backslash-escape anything outside the "safe" reusable set.
        if c.is_ascii_alphanumeric() || "_./,:+-=@%^".contains(c) {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
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
    // bash's `${v@Q}`/`${v@A}` single-quote every non-empty, control-free value
    // — even a "plain" word like `hi` becomes `'hi'`. (`%q` printf uses a
    // different, backslash-escaping quoter, `printf_quote`.)
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
/// Interpret `echo -e` backslash escapes. Returns the processed text and a
/// flag that is `true` when a `\c` escape was seen (which stops output and
/// suppresses the trailing newline). Recognizes `\a \b \e \E \f \n \r \t \v
/// \\`, `\0nnn` (octal, up to three digits), `\xHH` (hex, up to two digits),
/// and `\c`. An unrecognized escape keeps its backslash (bash behavior).
fn echo_expand_escapes(s: &str) -> (String, bool) {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' || i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1; // consume '\'
        match chars[i] {
            'a' => {
                out.push('\u{07}');
                i += 1;
            }
            'b' => {
                out.push('\u{08}');
                i += 1;
            }
            'e' | 'E' => {
                out.push('\u{1b}');
                i += 1;
            }
            'f' => {
                out.push('\u{0c}');
                i += 1;
            }
            'n' => {
                out.push('\n');
                i += 1;
            }
            'r' => {
                out.push('\r');
                i += 1;
            }
            't' => {
                out.push('\t');
                i += 1;
            }
            'v' => {
                out.push('\u{0b}');
                i += 1;
            }
            '\\' => {
                out.push('\\');
                i += 1;
            }
            'c' => return (out, true),
            '0' => {
                // `\0nnn` — up to three octal digits after the 0.
                i += 1;
                let mut val: u32 = 0;
                let mut n = 0;
                while n < 3 && i < chars.len() && chars[i].is_digit(8) {
                    val = val.wrapping_mul(8).wrapping_add(chars[i].to_digit(8).unwrap_or(0));
                    i += 1;
                    n += 1;
                }
                if let Some(c) = char::from_u32(val) {
                    out.push(c);
                }
            }
            'x' => {
                // `\xHH` — up to two hex digits.
                i += 1;
                let mut val: u32 = 0;
                let mut n = 0;
                while n < 2 && i < chars.len() && chars[i].is_ascii_hexdigit() {
                    val = val.wrapping_mul(16).wrapping_add(chars[i].to_digit(16).unwrap_or(0));
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    // No hex digit followed: keep the literal `\x`.
                    out.push('\\');
                    out.push('x');
                } else if let Some(c) = char::from_u32(val) {
                    out.push(c);
                }
            }
            esc @ ('u' | 'U') => {
                // `\uHHHH` (up to 4 hex digits) / `\UHHHHHHHH` (up to 8) — a
                // Unicode code point, emitted as UTF-8 (osh is a UTF-8 system,
                // matching the `$'…'` ANSI-C decoder). A missing hex digit
                // leaves the literal `\u`/`\U`.
                let max = if esc == 'u' { 4 } else { 8 };
                i += 1;
                let mut val: u32 = 0;
                let mut n = 0;
                while n < max && i < chars.len() && chars[i].is_ascii_hexdigit() {
                    val = val.wrapping_mul(16).wrapping_add(chars[i].to_digit(16).unwrap_or(0));
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    out.push('\\');
                    out.push(esc);
                } else if let Some(c) = char::from_u32(val) {
                    out.push(c);
                }
            }
            other => {
                // Unrecognized escape: keep the backslash and the character.
                out.push('\\');
                out.push(other);
                i += 1;
            }
        }
    }
    (out, false)
}

/// Which flavour of backslash-escape decoding [`decode_escape`] performs. The
/// two differ in exactly two respects — octal syntax and `\c` — but otherwise
/// share the named/hex/unicode escapes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EscapeMode {
    /// ANSI-C `$'…'` / `${v@E}` / the printf FORMAT string. Octal is `\nnn`
    /// (1–3 octal digits; a leading `0` is just the first digit, so `\0101`
    /// is `\010` followed by a literal `1`). `\c` is not special.
    AnsiC,
    /// printf `%b` / `echo -e`. Octal is `\0nnn` (a leading `0` is a *prefix*,
    /// then 1–3 octal digits, so `\0101` is the single character `A`). `\c`
    /// halts all further output.
    EchoB,
}

/// Decode a single backslash escape. `chars` is positioned immediately after the
/// `\`. The decoded text is appended to `out`. Returns `true` only when output
/// should stop (an `EchoB`-mode `\c`).
fn decode_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    out: &mut String,
    mode: EscapeMode,
) -> bool {
    let Some(c) = chars.next() else {
        out.push('\\');
        return false;
    };
    match c {
        'n' => out.push('\n'),
        't' => out.push('\t'),
        'r' => out.push('\r'),
        'a' => out.push('\u{07}'),
        'b' => out.push('\u{08}'),
        'e' | 'E' => out.push('\u{1b}'),
        'f' => out.push('\u{0c}'),
        'v' => out.push('\u{0b}'),
        '\\' => out.push('\\'),
        '\'' => out.push('\''),
        '"' => out.push('"'),
        // `%b`/`echo -e` `\c`: suppress the `\c` and everything after it.
        'c' if mode == EscapeMode::EchoB => return true,
        'x' => {
            let mut hex = String::new();
            while hex.len() < 2 && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                hex.push(chars.next().unwrap_or('0'));
            }
            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                Some(ch) => out.push(ch),
                None => {
                    // No hex digits followed `\x`: emit it literally.
                    out.push('\\');
                    out.push('x');
                }
            }
        }
        'u' | 'U' => {
            let max = if c == 'u' { 4 } else { 8 };
            let mut hex = String::new();
            while hex.len() < max && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                hex.push(chars.next().unwrap_or('0'));
            }
            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                Some(ch) => out.push(ch),
                None => {
                    out.push('\\');
                    out.push(c);
                }
            }
        }
        '0'..='7' => {
            let mut oct = String::new();
            // In `EchoB` mode a leading `0` is a prefix rather than a digit.
            if !(mode == EscapeMode::EchoB && c == '0') {
                oct.push(c);
            }
            while oct.len() < 3 && chars.peek().is_some_and(|c| ('0'..='7').contains(c)) {
                oct.push(chars.next().unwrap_or('0'));
            }
            if oct.is_empty() {
                // A bare `\0` (EchoB) is a NUL byte.
                out.push('\0');
            } else if let Some(ch) = u32::from_str_radix(&oct, 8).ok().and_then(char::from_u32) {
                out.push(ch);
            }
        }
        other => {
            out.push('\\');
            out.push(other);
        }
    }
    false
}

/// Minimal shell-quoting as used by `set -x` traces: a value made only of
/// "safe" characters (including the empty string) is emitted verbatim; anything
/// else is wrapped in single quotes, with embedded single quotes rendered as
/// `'\''`. This matches bash's xtrace output (`x=5`, `x='a b'`, `x=` for empty)
/// — distinct from `@Q`/`shell_quote` (which always quotes) and `%q` (which
/// backslash-escapes).
fn xtrace_quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if s.chars().all(safe) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
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

/// ANSI-C (`$'…'` / `${v@E}`) backslash-escape expansion.
fn ansi_c_unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        decode_escape(&mut chars, &mut out, EscapeMode::AnsiC);
    }
    out
}

/// `printf %b` / `echo -e` backslash-escape expansion. The boolean is `true` if
/// a `\c` was seen, meaning the caller must stop producing any further output.
fn unescape_echo_b(s: &str) -> (String, bool) {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        if decode_escape(&mut chars, &mut out, EscapeMode::EchoB) {
            return (out, true);
        }
    }
    (out, false)
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

/// Every builtin command name the shell recognizes. Kept as a single source of
/// truth so `is_builtin` and `enable -a` (which lists all builtins) never drift.
/// One-line help entries for the `help` builtin: (name, usage synopsis, short
/// description). Keep in sync with `BUILTIN_NAMES` / the dispatch table.
const HELP_TABLE: &[(&str, &str, &str)] = &[
    (":", ": [arguments]", "Null command; expand arguments and return success."),
    ("true", "true", "Return a successful (zero) exit status."),
    ("false", "false", "Return an unsuccessful (non-zero) exit status."),
    ("cd", "cd [-L|-P] [dir]", "Change the shell working directory."),
    ("pwd", "pwd [-L|-P]", "Print the name of the current working directory."),
    ("pushd", "pushd [dir | +N | -N]", "Add a directory to the directory stack."),
    ("popd", "popd [+N | -N]", "Remove a directory from the directory stack."),
    ("dirs", "dirs [-clpv] [+N | -N]", "Display the directory stack."),
    ("echo", "echo [-neE] [arg ...]", "Write arguments to standard output."),
    ("printf", "printf [-v var] format [arguments]", "Format and print arguments."),
    ("export", "export [-p] [name[=value] ...]", "Set export attribute for shell variables."),
    ("declare", "declare [-aAfFgilnprtux] [name[=value] ...]", "Declare variables and give them attributes."),
    ("typeset", "typeset [-aAfFgilnprtux] [name[=value] ...]", "Declare variables (synonym for declare)."),
    ("local", "local [-aAilnrux] name[=value] ...", "Define local variables in a function."),
    ("readonly", "readonly [-aApf] [name[=value] ...]", "Mark shell variables as unchangeable."),
    ("shopt", "shopt [-psuq] [optname ...]", "Set and unset shell options."),
    ("unset", "unset [-fv] name ...", "Unset values and attributes of variables and functions."),
    ("set", "set [-abefuxCo] [--] [arg ...]", "Set or unset shell options and positional parameters."),
    ("shift", "shift [n]", "Shift positional parameters."),
    ("getopts", "getopts optstring name [arg ...]", "Parse option arguments."),
    ("mapfile", "mapfile [-d delim] [-n count] [-O origin] [-s count] [-t] [-C callback] [-c quantum] [array]", "Read lines into an indexed array variable."),
    ("readarray", "readarray [-d delim] [-n count] [-O origin] [-s count] [-t] [-C callback] [-c quantum] [array]", "Read lines into an array (synonym for mapfile)."),
    ("command", "command [-pVv] name [arg ...]", "Execute a command bypassing shell functions."),
    ("builtin", "builtin [shell-builtin [arg ...]]", "Execute a shell builtin."),
    ("read", "read [-raspd delim] [-nN count] [name ...]", "Read a line from standard input and split it."),
    ("test", "test [expr]", "Evaluate a conditional expression."),
    ("[", "[ expr ]", "Evaluate a conditional expression (test)."),
    ("let", "let arg [arg ...]", "Evaluate arithmetic expressions."),
    ("eval", "eval [arg ...]", "Execute arguments as a shell command."),
    ("source", "source filename [arguments]", "Execute commands from a file in the current shell."),
    (".", ". filename [arguments]", "Execute commands from a file (synonym for source)."),
    ("type", "type [-afptP] name ...", "Display information about command type."),
    (
        "compgen",
        "compgen [-abcdefkv] [-A action] [-W wordlist] [-P prefix] [-S suffix] [-X filterpat] [word]",
        "Display possible completions depending on the options.",
    ),
    (
        "complete",
        "complete [-abcdefgjksuv] [-pr] [-DEI] [-o option] [-A action] [-G globpat] [-W wordlist] [-F function] [-C command] [-X filterpat] [-P prefix] [-S suffix] [name ...]",
        "Specify how arguments are to be completed by Readline.",
    ),
    (
        "compopt",
        "compopt [-o|+o option] [-DEI] [name ...]",
        "Modify or display completion options.",
    ),
    ("trap", "trap [-lp] [[action] signal_spec ...]", "Trap signals and other events."),
    ("jobs", "jobs [-lp] [jobspec ...]", "Display status of jobs."),
    ("wait", "wait [-n] [-p var] [id ...]", "Wait for jobs to complete and report status."),
    ("disown", "disown [-h] [-ar] [jobspec ...]", "Remove jobs from the current shell."),
    ("fg", "fg [jobspec]", "Move a job to the foreground."),
    ("bg", "bg [jobspec ...]", "Move jobs to the background."),
    ("caller", "caller [expr]", "Return the context of the current subroutine call."),
    ("times", "times", "Display process times."),
    ("hash", "hash [-lr] [-p path] [-dt] [name ...]", "Remember or display program locations."),
    ("umask", "umask [-Sp] [mode]", "Display or set the file mode creation mask."),
    ("exec", "exec [command [arguments]]", "Replace the shell with the given command."),
    ("exit", "exit [n]", "Exit the shell."),
    ("return", "return [n]", "Return from a shell function."),
    ("break", "break [n]", "Exit for, while, until, or select loops."),
    ("continue", "continue [n]", "Resume for, while, until, or select loops."),
    ("enable", "enable [-a] [-n] [name ...]", "Enable and disable shell builtins."),
    ("alias", "alias [-p] [name[=value] ...]", "Define or display aliases."),
    ("unalias", "unalias [-a] name [name ...]", "Remove each name from the list of aliases."),
    ("help", "help [-dms] [pattern ...]", "Display information about builtin commands."),
];

const BUILTIN_NAMES: &[&str] = &[
    ":", "true", "false", "cd", "pwd", "pushd", "popd", "dirs", "echo", "printf", "export",
    "declare", "typeset", "local", "readonly", "shopt", "unset", "set", "shift", "getopts",
    "mapfile", "readarray", "command", "builtin", "read", "test", "[", "let", "eval", "source",
    ".", "type", "trap", "jobs", "wait", "disown", "fg", "bg", "caller", "times", "hash", "umask",
    "ulimit", "exec",
    "exit", "return", "break", "continue", "enable", "alias", "unalias", "help", "compgen",
    "complete", "compopt",
];

fn is_builtin(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}

/// A resource-limit descriptor for the `ulimit` builtin.
struct RlimitSpec {
    /// Single option letter (`ulimit -n` → `'n'`).
    opt: char,
    /// Human description printed by `ulimit -a`.
    label: &'static str,
    /// Unit annotation (`"blocks"`, `"kbytes"`, …); empty means no unit word.
    unit: &'static str,
    /// Starting soft-limit value in display units (`None` == unlimited).
    default: Option<u64>,
}

/// The resource limits osh models, in the order bash prints them for `-a`.
/// Values/units mirror Linux bash; defaults are conventional soft limits.
const RLIMIT_SPECS: &[RlimitSpec] = &[
    RlimitSpec { opt: 'c', label: "core file size", unit: "blocks", default: Some(0) },
    RlimitSpec { opt: 'd', label: "data seg size", unit: "kbytes", default: None },
    RlimitSpec { opt: 'e', label: "scheduling priority", unit: "", default: Some(0) },
    RlimitSpec { opt: 'f', label: "file size", unit: "blocks", default: None },
    RlimitSpec { opt: 'i', label: "pending signals", unit: "", default: None },
    RlimitSpec { opt: 'l', label: "max locked memory", unit: "kbytes", default: None },
    RlimitSpec { opt: 'm', label: "max memory size", unit: "kbytes", default: None },
    RlimitSpec { opt: 'n', label: "open files", unit: "", default: Some(1024) },
    RlimitSpec { opt: 'p', label: "pipe size", unit: "512 bytes", default: Some(8) },
    RlimitSpec { opt: 'q', label: "POSIX message queues", unit: "bytes", default: Some(819200) },
    RlimitSpec { opt: 'r', label: "real-time priority", unit: "", default: Some(0) },
    RlimitSpec { opt: 's', label: "stack size", unit: "kbytes", default: Some(8192) },
    RlimitSpec { opt: 't', label: "cpu time", unit: "seconds", default: None },
    RlimitSpec { opt: 'u', label: "max user processes", unit: "", default: None },
    RlimitSpec { opt: 'v', label: "virtual memory", unit: "kbytes", default: None },
    RlimitSpec { opt: 'x', label: "file locks", unit: "", default: None },
];

const ULIMIT_USAGE: &str = "ulimit: usage: ulimit [-SHacdefilmnpqrstuvx] [limit]\n";

/// Build the initial `(soft, hard)` limit table. Soft = the spec default,
/// hard = unlimited (osh does not enforce, so an unlimited hard ceiling is the
/// honest model — see `TD-OILS-ULIMIT`).
fn default_rlimits() -> std::collections::HashMap<char, (Option<u64>, Option<u64>)> {
    RLIMIT_SPECS.iter().map(|s| (s.opt, (s.default, None))).collect()
}

/// Format a limit value for display (`None` → `unlimited`).
fn ulimit_value_str(v: Option<u64>) -> String {
    match v {
        Some(n) => n.to_string(),
        None => "unlimited".to_string(),
    }
}

/// Render one `ulimit -a` line: description, right-aligned `(unit, -x)` token,
/// then the value. The `)` is placed at a fixed column so the tokens align.
fn ulimit_line(spec: &RlimitSpec, v: Option<u64>) -> String {
    let paren = if spec.unit.is_empty() {
        format!("(-{})", spec.opt)
    } else {
        format!("({}, -{})", spec.unit, spec.opt)
    };
    // Column of the closing paren; pad the description so `)` lands there.
    let width = 36usize.saturating_sub(paren.len());
    format!("{:<width$}{} {}\n", spec.label, paren, ulimit_value_str(v), width = width)
}

/// The declaration builtins whose `name=value` operands bash treats as
/// assignments (assignment-context expansion: tilde-expanded, no splitting/glob).
fn is_declaration_builtin(name: &str) -> bool {
    matches!(name, "export" | "declare" | "typeset" | "local" | "readonly")
}

/// If a word is a single unquoted literal, return it (used to recognise a
/// declaration-builtin command word syntactically, as bash does).
fn word_as_plain_literal(word: &Word) -> Option<&str> {
    match word.parts.as_slice() {
        [WordPart::Literal(s)] => Some(s.as_str()),
        _ => None,
    }
}

/// Does a word have the syntactic form of an assignment (`NAME=…`,
/// `NAME+=…`, or `NAME[subscript]=…`)? Used to route declaration-builtin
/// operands through assignment-context expansion.
fn is_assignment_word(word: &Word) -> bool {
    let Some(WordPart::Literal(s)) = word.parts.first() else {
        return false;
    };
    let bytes = s.as_bytes();
    let Some(&c0) = bytes.first() else {
        return false;
    };
    if !(c0.is_ascii_alphabetic() || c0 == b'_') {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    // Optional array subscript `[...]`.
    if i < bytes.len() && bytes[i] == b'[' {
        match s[i..].find(']') {
            Some(close) => i += close + 1,
            None => return false,
        }
    }
    // Optional `+` for the append form `NAME+=`.
    if i < bytes.len() && bytes[i] == b'+' {
        i += 1;
    }
    i < bytes.len() && bytes[i] == b'='
}

/// A numeric tilde-prefix reference into the directory stack.
enum DirStackRef {
    /// `~N` / `~+N` — the Nth entry counting from the left (0 = current dir).
    FromLeft(usize),
    /// `~-N` — the Nth entry counting from the right of the stack.
    FromRight(usize),
}

/// Parse the numeric part of a directory-stack tilde-prefix (`N`, `+N`, `-N`).
/// Returns `None` for a non-numeric prefix (e.g. a username), which leaves the
/// word unexpanded.
fn parse_dirstack_index(prefix: &str) -> Option<DirStackRef> {
    if let Some(digits) = prefix.strip_prefix('-') {
        digits.parse::<usize>().ok().map(DirStackRef::FromRight)
    } else if let Some(digits) = prefix.strip_prefix('+') {
        digits.parse::<usize>().ok().map(DirStackRef::FromLeft)
    } else {
        prefix.parse::<usize>().ok().map(DirStackRef::FromLeft)
    }
}

/// A unique temp-file path under the system temp dir, using the process id plus
/// a monotonic counter so concurrent expansions never collide. Used for process
/// substitution (`<(cmd)`/`>(cmd)`); the caller creates and later removes it.
fn unique_temp_path(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{prefix}_{}_{n}.tmp", std::process::id()));
    path.to_string_lossy().replace('\\', "/")
}

/// Map well-known Unix pseudo-device paths to the host's equivalent.
///
/// On the Windows test host `/dev/null` has no native path: opening it for
/// write would silently create a real file `\dev\null` at the drive root
/// (polluting the filesystem and breaking later reads), and reading it would
/// pick up that stray file instead of yielding EOF. Mapping it to `NUL` — the
/// Windows null device — makes writes discard and reads return EOF, matching
/// bash. On Unix and on SlateOS the OS provides `/dev/null` as a real device
/// node, so the path is returned unchanged.
#[cfg(windows)]
fn map_device_path(path: &str) -> &str {
    match path {
        "/dev/null" => "NUL",
        _ => path,
    }
}

#[cfg(not(windows))]
fn map_device_path(path: &str) -> &str {
    path
}

/// Format a parse error for display the way bash does. bash's unexpected-token
/// diagnostic is itself `syntax error near unexpected token '…'`, so blindly
/// prefixing every parser message with `syntax error: ` would double the phrase
/// (`syntax error: syntax error near …`). Only add the prefix for the
/// fragment-style messages (`expected ')'`, `empty command`, …); pass through a
/// message that already opens with `syntax error`.
fn format_parse_error(e: &crate::parser::ParseError, prefix: &str) -> String {
    let msg = e.to_string();
    if msg.starts_with("syntax error") {
        format!("{prefix}{msg}")
    } else {
        format!("{prefix}syntax error: {msg}")
    }
}

fn open_out(path: &str, append: bool) -> io::Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true);
    if append {
        opts.append(true);
    } else {
        opts.truncate(true);
    }
    opts.open(map_device_path(path))
}

/// Is this redirect a descriptor *close* (`N>&-` / `N<&-`)? For a varfd
/// redirect (`{v}>&-`) this is the one form that reuses `$v`'s current value
/// rather than allocating a new descriptor, so it must be distinguished from
/// the open forms (`{v}>file`, `{v}>&N`, …) which bind a fresh persistent fd.
fn redir_is_close(r: &Redirect) -> bool {
    matches!(r.op, RedirectOp::DupOut | RedirectOp::DupIn)
        && matches!(r.target.parts.as_slice(), [WordPart::Literal(s)] if s == "-")
}

/// Duplicate the process's real standard stdout (`is_stdout`) or stderr into an
/// owned [`File`]. Used to snapshot fd 1 / fd 2's *terminal* sink for
/// `exec 3>&1` / `exec 3>&2` when that fd is not currently redirected to a file.
/// Writing to the returned handle writes to the same terminal (a `dup`, so it
/// shares the OS file offset for a regular-file-backed standard fd).
#[cfg(unix)]
fn dup_std_handle(is_stdout: bool) -> io::Result<std::fs::File> {
    use std::os::fd::AsFd;
    let owned = if is_stdout {
        io::stdout().as_fd().try_clone_to_owned()?
    } else {
        io::stderr().as_fd().try_clone_to_owned()?
    };
    Ok(std::fs::File::from(owned))
}

/// Windows counterpart of [`dup_std_handle`] — duplicates the console/handle
/// backing stdout or stderr into an owned [`File`].
#[cfg(windows)]
fn dup_std_handle(is_stdout: bool) -> io::Result<std::fs::File> {
    use std::os::windows::io::AsHandle;
    let owned = if is_stdout {
        io::stdout().as_handle().try_clone_to_owned()?
    } else {
        io::stderr().as_handle().try_clone_to_owned()?
    };
    Ok(std::fs::File::from(owned))
}

/// Duplicate an anonymous-pipe write end into an owned [`File`] referencing the
/// same OS pipe. Lets a compound command's `N>&1` alias (fd N ≥ 3 dup'ing fd 1)
/// land on the *pipeline stage's* output pipe — writing to the returned handle
/// streams into the same pipe the stage's stdout feeds, so `>&N` inside the body
/// reaches the downstream stage instead of the terminal.
#[cfg(unix)]
fn pipe_writer_to_file(w: &io::PipeWriter) -> io::Result<std::fs::File> {
    let cloned = w.try_clone()?;
    Ok(std::fs::File::from(std::os::fd::OwnedFd::from(cloned)))
}

/// Windows counterpart of the Unix `pipe_writer_to_file`.
#[cfg(windows)]
fn pipe_writer_to_file(w: &io::PipeWriter) -> io::Result<std::fs::File> {
    let cloned = w.try_clone()?;
    Ok(std::fs::File::from(
        std::os::windows::io::OwnedHandle::from(cloned),
    ))
}

/// Consume an anonymous-pipe read end into an owned [`File`] over the same OS
/// pipe. Used to store a `coproc`'s parent-side stdout read end in the live
/// coproc-read table (a `File` is `Read`, so it drives the `read` builtins).
#[cfg(unix)]
fn pipe_reader_into_file(r: io::PipeReader) -> std::fs::File {
    std::fs::File::from(std::os::fd::OwnedFd::from(r))
}

/// Windows counterpart of `pipe_reader_into_file`.
#[cfg(windows)]
fn pipe_reader_into_file(r: io::PipeReader) -> std::fs::File {
    std::fs::File::from(std::os::windows::io::OwnedHandle::from(r))
}

/// Consume an anonymous-pipe write end into an owned [`File`] over the same OS
/// pipe. Used to store a `coproc`'s parent-side stdin write end in
/// [`Shell::open_write_fds`], so `>&"${NAME[1]}"` reaches the coproc.
#[cfg(unix)]
fn pipe_writer_into_file(w: io::PipeWriter) -> std::fs::File {
    std::fs::File::from(std::os::fd::OwnedFd::from(w))
}

/// Windows counterpart of `pipe_writer_into_file`.
#[cfg(windows)]
fn pipe_writer_into_file(w: io::PipeWriter) -> std::fs::File {
    std::fs::File::from(std::os::windows::io::OwnedHandle::from(w))
}

/// Monotonic synthetic pid source for `coproc` bodies (which run as threads,
/// not OS processes). Starts high to avoid colliding with real pids.
static COPROC_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(900_000);

/// Non-blocking readability probe for a Windows OS handle, used by `read -t 0`.
/// Returns true when a read would proceed immediately (data queued, or the
/// source is at EOF), false when it would block. Distinguishes the handle type:
/// a disk file never blocks; a pipe is queried with `PeekNamedPipe` (a
/// writer-closed pipe reports `ERROR_BROKEN_PIPE` ⇒ EOF ⇒ ready); a character
/// device (console / `NUL`) uses a zero-timeout wait (an idle console blocks; a
/// non-waitable device like `NUL` reads as immediate EOF ⇒ ready).
#[cfg(windows)]
fn handle_readable_now(handle: std::os::windows::io::RawHandle) -> bool {
    use core::ffi::c_void;
    const FILE_TYPE_DISK: u32 = 1;
    const FILE_TYPE_CHAR: u32 = 2;
    const FILE_TYPE_PIPE: u32 = 3;
    const ERROR_BROKEN_PIPE: u32 = 109;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;
    // SAFETY: these are the standard kernel32 signatures; all pointers we pass
    // are either null or point to a local `u32` valid for the call's duration.
    unsafe extern "system" {
        fn GetFileType(h_file: *mut c_void) -> u32;
        fn PeekNamedPipe(
            h_named_pipe: *mut c_void,
            lp_buffer: *mut c_void,
            n_buffer_size: u32,
            lp_bytes_read: *mut u32,
            lp_total_bytes_avail: *mut u32,
            lp_bytes_left_this_message: *mut u32,
        ) -> i32;
        fn WaitForSingleObject(h_handle: *mut c_void, dw_milliseconds: u32) -> u32;
        fn GetLastError() -> u32;
    }
    let h = handle.cast::<c_void>();
    // SAFETY: `handle` is a live OS handle borrowed from a pipe reader or the
    // process stdin. Each call only queries the handle (no mutation, no
    // ownership transfer), and `avail` is a valid local for the pipe query.
    unsafe {
        match GetFileType(h) {
            FILE_TYPE_DISK => true,
            FILE_TYPE_PIPE => {
                let mut avail: u32 = 0;
                let ok = PeekNamedPipe(
                    h,
                    core::ptr::null_mut(),
                    0,
                    core::ptr::null_mut(),
                    &raw mut avail,
                    core::ptr::null_mut(),
                );
                if ok != 0 {
                    avail > 0
                } else {
                    GetLastError() == ERROR_BROKEN_PIPE
                }
            }
            // WAIT_OBJECT_0 (input queued) and WAIT_FAILED (non-waitable, e.g.
            // NUL) ⇒ ready; only an idle console (WAIT_TIMEOUT) would block.
            FILE_TYPE_CHAR => WaitForSingleObject(h, 0) != WAIT_TIMEOUT,
            _ => true,
        }
    }
}

/// `read -t 0`: is the process's inherited stdin readable without blocking?
#[cfg(windows)]
fn stdin_readable_now() -> bool {
    use std::os::windows::io::AsRawHandle;
    handle_readable_now(io::stdin().as_raw_handle())
}

/// `read -t 0`: does the upstream pipeline pipe hold data (or sit at EOF)?
#[cfg(windows)]
fn pipe_reader_readable_now(r: &io::PipeReader) -> bool {
    use std::os::windows::io::AsRawHandle;
    handle_readable_now(r.as_raw_handle())
}

/// Fallback readability probe for non-Windows targets (including SlateOS).
/// A non-tty inherited stdin (file, `/dev/null`, an already-drained pipe) is
/// treated as ready; an interactive terminal as would-block. A precise answer
/// for a live OS pipe/tty needs a `poll(2)`-style query, which is not yet wired
/// for these targets — see TD-OILS-READ-T0-POLL in `known-issues.md`.
#[cfg(not(windows))]
fn stdin_readable_now() -> bool {
    !io::stdin().is_terminal()
}

/// Fallback: without a `poll(2)` peek, only bytes already buffered by the caller
/// count as ready for an upstream pipe (a bare OS-pipe query is not yet wired
/// for non-Windows targets — see TD-OILS-READ-T0-POLL).
#[cfg(not(windows))]
fn pipe_reader_readable_now(_r: &io::PipeReader) -> bool {
    false
}

/// Read one newline-terminated line, returning `(text, terminated)` where
/// `terminated` is true when an actual `\n` delimiter was consumed and false
/// when the input ended (EOF) before any newline. `read` reports status 1 for
/// an unterminated final line (matching bash), so the caller needs to know
/// which case occurred. Returns `None` only on immediate EOF with no bytes.
fn read_one_line<R: BufRead>(r: &mut R) -> Option<(String, bool)> {
    let mut line = String::new();
    let n = r.read_line(&mut line).ok()?;
    if n == 0 {
        return None;
    }
    let terminated = line.ends_with('\n');
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Some((line, terminated))
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

/// ANSI-C (`$'…'`) quote a string, escaping control characters so it re-inputs
/// as the same bytes. Shared by the `set` scalar listing and `declare -p`
/// associative-key formatting (both use this form when a control char is
/// present).
fn ansi_c_quote(s: &str) -> String {
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
    out
}

/// Quote an associative-array subscript for `declare -p` / `set` output the way
/// bash does: a key with only "safe" characters is emitted raw (`[key]`); a key
/// containing a control character uses ANSI-C `$'…'` quoting; otherwise a key
/// holding a shell metacharacter is double-quoted like a value (`["a b"]`). This
/// makes the printed subscript round-trip back to the same key on re-input.
fn quote_declare_key(k: &str) -> String {
    if k.is_empty() {
        return String::from("\"\"");
    }
    if k.chars().any(char::is_control) {
        return ansi_c_quote(k);
    }
    // Metacharacters that would break re-parsing of a bare `[subscript]`, so
    // bash double-quotes the key. Observed from real `declare -p` output (note
    // `#`, `~`, `@`, `.`, `,`, `/`, `:`, `=`, `+`, `%`, `-` do *not* force it).
    const KEY_METAS: &[char] = &[
        ' ', '\t', '"', '\\', '$', '`', '\'', '*', '!', '?', ';', '|', '&', '(', ')', '<', '>',
        '{', '}', '^', '[', ']',
    ];
    if k.chars().any(|c| KEY_METAS.contains(&c)) {
        return quote_declare_value(k);
    }
    k.to_string()
}

/// Quote a scalar value the way bash's bare `set` variable listing does — which
/// differs from `declare -p` (that one always double-quotes). Here a value is
/// rendered *raw* when it needs no quoting, ANSI-C `$'…'`-quoted when it holds a
/// control character, and single-quoted when it holds a shell metacharacter (or
/// leads with `#`/`~`, which would otherwise start a comment or tilde-expand on
/// re-input). An empty value renders as the bare `name=` (no quotes).
///
/// The metacharacter set mirrors bash's `sh_contains_shell_metas` as observed
/// from real `set` output (note: comma does *not* force quoting, matching bash).
fn quote_set_value(v: &str) -> String {
    if v.is_empty() {
        return String::new();
    }
    if v.chars().any(char::is_control) {
        return ansi_c_quote(v);
    }
    const METAS: &[char] = &[
        ' ', '\'', '"', '\\', '|', '&', ';', '(', ')', '<', '>', '!', '{', '}', '*', '[', '?',
        ']', '^', '$', '`',
    ];
    let leads = v.starts_with('#') || v.starts_with('~');
    if leads || v.chars().any(|c| METAS.contains(&c)) {
        let mut out = String::from("'");
        for c in v.chars() {
            if c == '\'' {
                out.push_str("'\\''");
            } else {
                out.push(c);
            }
        }
        out.push('\'');
        return out;
    }
    v.to_string()
}


/// A special shell parameter that is always considered "set" for `nounset`
/// purposes (referencing it never yields an unbound-variable error).
fn is_special_param(name: &str) -> bool {
    matches!(name, "@" | "*" | "#" | "?" | "$" | "!" | "0" | "-" | "_")
}

/// Whether `s` is a valid parameter name to use as the *target* of an indirect
/// expansion `${!ptr}` (i.e. the value held by `ptr`). bash accepts a special
/// parameter (`@`, `#`, …), a positional parameter (all digits), a plain
/// identifier, or an array-element reference `name[subscript]`, and reports
/// `"invalid variable name"` for anything else (`a-b`, `1abc`, empty, `[]`).
fn is_valid_indirect_target(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if is_special_param(s) {
        return true;
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    // A plain identifier, optionally followed by a non-empty `[subscript]`.
    let name = if let Some(open) = s.find('[') {
        let Some(inner) = s.strip_suffix(']') else {
            return false; // `[` without a closing `]`
        };
        if inner.get(open + 1..).unwrap_or("").is_empty() {
            return false; // empty subscript `name[]`
        }
        &s[..open]
    } else {
        s
    };
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(b) if b == b'_' || b.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric())
}

/// Clone a scalar-modifier `WordPart` (the `target` of a `${!ref<op>}` indirect
/// expansion), replacing its parameter name with the resolved target name. Only
/// the modifier variants that can follow indirection are handled; anything else
/// is returned unchanged (the parser guarantees `target` is one of these).
fn rename_param_target(part: &WordPart, new_name: &str) -> WordPart {
    let name = new_name.to_string();
    match part.clone() {
        WordPart::ParamOp {
            index,
            op,
            colon,
            arg,
            ..
        } => WordPart::ParamOp {
            name,
            index,
            op,
            colon,
            arg,
        },
        WordPart::ParamTrim {
            index,
            suffix,
            longest,
            pattern,
            ..
        } => WordPart::ParamTrim {
            name,
            index,
            suffix,
            longest,
            pattern,
        },
        WordPart::ParamSubstr {
            index,
            offset,
            length,
            ..
        } => WordPart::ParamSubstr {
            name,
            index,
            offset,
            length,
        },
        WordPart::ParamReplace {
            index,
            all,
            anchor,
            pattern,
            replacement,
            ..
        } => WordPart::ParamReplace {
            name,
            index,
            all,
            anchor,
            pattern,
            replacement,
        },
        WordPart::ParamCase {
            index,
            mode,
            all,
            pattern,
            ..
        } => WordPart::ParamCase {
            name,
            index,
            mode,
            all,
            pattern,
        },
        WordPart::ParamTransform { index, op, .. } => WordPart::ParamTransform { name, index, op },
        other => other,
    }
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
fn format_printf(fmt: &str, args: &[String], errors: &mut Vec<String>) -> String {
    // Bash reuses the format string until all arguments are consumed. Repeat the
    // format while arguments remain, stopping if a pass consumes none (the
    // format has no argument-consuming conversions) to avoid an infinite loop.
    let mut out = String::new();
    let mut arg_i = 0;
    loop {
        let start = arg_i;
        let (chunk, stop) = format_printf_once(fmt, args, &mut arg_i, errors);
        out.push_str(&chunk);
        // A `%b` argument containing `\c` halts all further output, format
        // recycling included.
        if stop || arg_i >= args.len() || arg_i == start {
            break;
        }
    }
    out
}

/// Render one pass over the format string. Returns the produced text and whether
/// a `\c` (in a `%b` argument) requested that output stop.
fn format_printf_once(
    fmt: &str,
    args: &[String],
    arg_i: &mut usize,
    errors: &mut Vec<String>,
) -> (String, bool) {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // FORMAT-string escapes use ANSI-C rules (octal `\nnn`, `\xHH`,
            // `\uHHHH`, `\UHHHHHHHH`, and the named escapes).
            '\\' => {
                decode_escape(&mut chars, &mut out, EscapeMode::AnsiC);
            }
            '%' => {
                if format_conversion(&mut chars, args, arg_i, &mut out, errors) {
                    return (out, true);
                }
            }
            other => out.push(other),
        }
    }
    (out, false)
}

/// Parse and render a single `%…` printf conversion. `chars` is positioned just
/// after the `%`. Supports flags (`-+ #0`), width and precision (numeric or `*`
/// dynamic from an argument), and the conversions `% s d i u x X o c b q f e g E G`.
/// Returns `true` when a `%b` argument's `\c` requested that output stop.
fn format_conversion(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    args: &[String],
    arg_i: &mut usize,
    out: &mut String,
    errors: &mut Vec<String>,
) -> bool {
    // Literal `%%` short-circuit (no flags/width may precede it).
    if chars.peek() == Some(&'%') {
        chars.next();
        out.push('%');
        return false;
    }

    // Collect flags.
    let mut spec = String::from("%");
    let mut left = false;
    let mut zero = false;
    let mut plus = false;
    let mut space = false;
    let mut hash = false;
    while let Some(&c) = chars.peek() {
        match c {
            '-' => left = true,
            '0' => zero = true,
            '+' => plus = true,
            ' ' => space = true,
            '#' => hash = true,
            // Thousands-grouping flag. We run in the C locale, which has no
            // grouping, so accept and ignore it (bash: `printf "%'d" 1234567`
            // prints `1234567` unless a grouping locale is active).
            '\'' => {}
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
        return false;
    }

    let Some(conv) = chars.next() else {
        // Trailing bare `%…` with no conversion: emit literally.
        out.push_str(&spec);
        out.push_str(&width);
        if let Some(p) = &prec {
            out.push('.');
            out.push_str(p);
        }
        return false;
    };

    let next_arg = |arg_i: &mut usize| -> String {
        let v = args.get(*arg_i).cloned().unwrap_or_default();
        *arg_i += 1;
        v
    };

    // Sign/base prefix rendered separately from the digit body so that
    // zero-padding can insert zeros *between* the prefix and the body
    // (e.g. `%+05d` on 5 → `+0005`, `%#06x` on 255 → `0x00ff`).
    let mut num_prefix = String::new();
    // Set when a `%b` argument's `\c` truncates output.
    let mut stop = false;
    let mut rendered = match conv {
        's' => {
            let mut s = next_arg(arg_i);
            if let Some(p) = prec_n {
                s.truncate(p);
            }
            s
        }
        'b' => {
            // Interpret `echo -e`-style backslash escapes in the argument; `\c`
            // stops all further output.
            let (s, st) = unescape_echo_b(&next_arg(arg_i));
            stop = st;
            s
        }
        'q' => printf_quote(&next_arg(arg_i)),
        'c' => next_arg(arg_i).chars().next().map_or(String::new(), |c| c.to_string()),
        'd' | 'i' => {
            let raw = next_arg(arg_i);
            let (n, err) = parse_printf_int_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            let (p, b) = split_sign(n.to_string(), plus, space);
            num_prefix = p;
            b
        }
        'u' => {
            let raw = next_arg(arg_i);
            let (n, err) = parse_printf_int_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            n.cast_unsigned().to_string()
        }
        'x' => {
            let raw = next_arg(arg_i);
            let (n, err) = parse_printf_int_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            let v = n.cast_unsigned();
            // `#` prefixes nonzero hex with `0x` (bash/C: zero gets no prefix).
            if hash && v != 0 {
                num_prefix.push_str("0x");
            }
            format!("{v:x}")
        }
        'X' => {
            let raw = next_arg(arg_i);
            let (n, err) = parse_printf_int_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            let v = n.cast_unsigned();
            if hash && v != 0 {
                num_prefix.push_str("0X");
            }
            format!("{v:X}")
        }
        'o' => {
            let raw = next_arg(arg_i);
            let (n, err) = parse_printf_int_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            let v = n.cast_unsigned();
            // `#` forces a leading `0` on octal; applied after precision below
            // so `%#.1o 8` → `010` (precision body `10`, then forced `0`).
            format!("{v:o}")
        }
        'f' | 'F' => {
            let raw = next_arg(arg_i);
            let (f, err) = parse_printf_float_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            let (p, b) = split_sign(format!("{:.*}", prec_n.unwrap_or(6), f), plus, space);
            num_prefix = p;
            b
        }
        'e' | 'E' => {
            let raw = next_arg(arg_i);
            let (f, err) = parse_printf_float_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            let s = format!("{:.*e}", prec_n.unwrap_or(6), f);
            let s = normalize_exp(&s);
            let s = if conv == 'E' { s.to_uppercase() } else { s };
            let (p, b) = split_sign(s, plus, space);
            num_prefix = p;
            b
        }
        'g' | 'G' => {
            let raw = next_arg(arg_i);
            let (f, err) = parse_printf_float_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            let s = format_g(f, prec_n.unwrap_or(6), conv == 'G', hash);
            let (p, b) = split_sign(s, plus, space);
            num_prefix = p;
            b
        }
        'a' | 'A' => {
            let raw = next_arg(arg_i);
            let (f, err) = parse_printf_float_checked(&raw);
            if let Some(kind) = err {
                errors.push(format!("{raw}: {kind}"));
            }
            let s = format_a(f, prec_n, conv == 'A');
            let (p, b) = split_sign(s, plus, space);
            num_prefix = p;
            b
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
            return false;
        }
    };

    // Integer conversions treat precision as a *minimum digit count*: the
    // magnitude body is zero-padded on the left to `prec` digits (this is
    // independent of, and combines with, field width). A precision of 0 applied
    // to the value 0 yields no digits at all (C/bash: `printf %.0d 0` → ``).
    // When a precision is present the `0` flag is ignored for integers, so
    // width is space-padded (`%08.3d 42` → `     042`).
    if matches!(conv, 'd' | 'i' | 'u' | 'x' | 'X' | 'o') {
        if let Some(p) = prec_n {
            zero = false;
            let cur = rendered.chars().count();
            if p == 0 && rendered == "0" {
                rendered.clear();
            } else if cur < p {
                let mut padded = String::with_capacity(p);
                padded.extend(std::iter::repeat_n('0', p - cur));
                padded.push_str(&rendered);
                rendered = padded;
            }
        }
        // `#` on octal forces the (precision-padded) body to begin with a `0`,
        // even when precision-0 emptied it (`printf %#.0o 0` → `0`).
        if hash && conv == 'o' && !rendered.starts_with('0') {
            rendered.insert(0, '0');
        }
    }

    // Apply field width padding. The sign/base prefix and the digit body are
    // padded as a unit; for zero-padding the zeros go between them.
    let total_len = num_prefix.chars().count() + rendered.chars().count();
    if total_len < width_n {
        let pad = width_n - total_len;
        if left {
            out.push_str(&num_prefix);
            out.push_str(&rendered);
            out.extend(std::iter::repeat_n(' ', pad));
        } else if zero
            && matches!(
                conv,
                'd' | 'i' | 'u' | 'x' | 'X' | 'o' | 'f' | 'F' | 'e' | 'E' | 'g' | 'G' | 'a' | 'A'
            )
        {
            // Zero-pad: prefix, then the padding zeros, then the body.
            out.push_str(&num_prefix);
            out.extend(std::iter::repeat_n('0', pad));
            out.push_str(&rendered);
        } else {
            out.extend(std::iter::repeat_n(' ', pad));
            out.push_str(&num_prefix);
            out.push_str(&rendered);
        }
    } else {
        out.push_str(&num_prefix);
        out.push_str(&rendered);
    }
    stop
}

/// Split a formatted numeric string into a sign prefix and its magnitude body,
/// applying printf's `+`/space flags when the value is non-negative. A leading
/// `-` always becomes the prefix; otherwise `+` (then space) is added if the
/// corresponding flag is set. Keeping the sign separate lets zero-padding place
/// the fill zeros after the sign (`%+05d` on 5 → `+0005`, not `000+5`).
fn split_sign(s: String, plus: bool, space: bool) -> (String, String) {
    if let Some(rest) = s.strip_prefix('-') {
        ("-".to_string(), rest.to_string())
    } else if plus {
        ("+".to_string(), s)
    } else if space {
        (" ".to_string(), s)
    } else {
        (String::new(), s)
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
    parse_printf_int_checked(s).0
}

/// Parse an integer `printf` argument with C/bash `strtoimax` semantics and
/// report whether it was fully valid. Leading whitespace and an optional sign
/// are skipped; a `0x`/`0X` prefix selects hex, a leading `0` selects octal,
/// otherwise decimal. The value is the leading run of valid digits (bash uses
/// that partial value even when it warns). The returned `Option` is `None` when
/// the whole argument was consumed, or `Some(kind)` — the bash diagnostic tail
/// (`"invalid number"` / `"invalid octal number"` / `"invalid hex number"`) —
/// when trailing junk (including trailing whitespace) or a bad digit remains.
fn parse_printf_int_checked(s: &str) -> (i64, Option<&'static str>) {
    // strtoimax skips *leading* whitespace but treats trailing whitespace as
    // junk, so trim only the front. An empty/blank argument is a valid 0.
    let t = s.trim_start();
    if t.is_empty() {
        return (0, None);
    }
    // `'c` / `"c` yields the numeric code of the first character (always valid).
    if let Some(rest) = t.strip_prefix('\'').or_else(|| t.strip_prefix('"')) {
        return (rest.chars().next().map_or(0, |c| i64::from(u32::from(c))), None);
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let (radix, digits, kind) = if let Some(h) = body.strip_prefix("0x").or_else(|| body.strip_prefix("0X"))
    {
        (16u32, h, "invalid hex number")
    } else if body.len() > 1 && body.starts_with('0') && body.as_bytes()[1].is_ascii_digit() {
        // Octal only when a digit follows the `0` (`08`, `019`); a `0` followed
        // by a letter (`0b101`) is decimal-with-junk, so bash reports the
        // generic "invalid number", not "invalid octal number".
        (8u32, &body[1..], "invalid octal number")
    } else {
        (10u32, body, "invalid number")
    };
    // Consume the leading run of digits valid for the radix. These are ASCII,
    // so the char count equals the byte length of the consumed prefix.
    let valid_len = digits.chars().take_while(|c| c.is_digit(radix)).count();
    let consumed = &digits[..valid_len];
    let remaining = &digits[valid_len..];
    let magnitude = i64::from_str_radix(consumed, radix).unwrap_or(0);
    let value = if neg { magnitude.wrapping_neg() } else { magnitude };
    let err = if consumed.is_empty() || !remaining.is_empty() {
        Some(kind)
    } else {
        None
    };
    (value, err)
}

/// Parse a floating-point `printf` argument with C/bash `strtod` semantics and
/// report validity. Like [`parse_printf_int_checked`], leading whitespace is
/// skipped and the value is the longest parseable leading prefix; the `Option`
/// is `Some("invalid number")` when trailing junk remains.
fn parse_printf_float_checked(s: &str) -> (f64, Option<&'static str>) {
    let t = s.trim_start();
    if t.is_empty() {
        return (0.0, None);
    }
    if let Some(rest) = t.strip_prefix('\'').or_else(|| t.strip_prefix('"')) {
        return (rest.chars().next().map_or(0.0, |c| f64::from(u32::from(c))), None);
    }
    if let Ok(v) = t.parse::<f64>() {
        return (v, None);
    }
    // Fall back to the longest leading prefix that parses (strtod partial value).
    let mut best = 0.0;
    for (i, c) in t.char_indices() {
        let end = i.saturating_add(c.len_utf8());
        if let Ok(v) = t[..end].parse::<f64>() {
            best = v;
        }
    }
    (best, Some("invalid number"))
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

/// Strip a `%g`-formatted string's trailing fractional zeros (and a trailing
/// bare decimal point), preserving any exponent suffix. `1.5000e+10` →
/// `1.5e+10`, `3.140` → `3.14`, `100.` → `100`.
fn strip_g_zeros(s: &str) -> String {
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    };
    let mant = if mant.contains('.') {
        mant.trim_end_matches('0').trim_end_matches('.')
    } else {
        mant
    };
    format!("{mant}{exp}")
}

/// Format a float using C's `%g`/`%G` rules: `prec` significant digits (a
/// precision of 0 is treated as 1). Chooses `%e` style when the decimal
/// exponent is `< -4` or `>= prec`, otherwise `%f` style; trailing zeros are
/// removed unless the `#` (alternate) flag is set. `upper` selects `%G`.
fn format_g(f: f64, prec: usize, upper: bool, hash: bool) -> String {
    if !f.is_finite() {
        let s = if f.is_nan() {
            "nan".to_string()
        } else if f < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
        return if upper { s.to_uppercase() } else { s };
    }
    let p = prec.max(1);
    // Format in %e style with p-1 fractional digits to learn the exponent and
    // to reuse for the scientific branch.
    let e_str = format!("{:.*e}", p - 1, f);
    let exp: i32 = e_str
        .rsplit(['e', 'E'])
        .next()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0);
    let mut s = if exp < -4 || exp >= p as i32 {
        normalize_exp(&e_str)
    } else {
        // %f style with (p - 1 - exp) fractional digits.
        let fprec = usize::try_from(p as i32 - 1 - exp).unwrap_or(0);
        format!("{f:.fprec$}")
    };
    if !hash {
        s = strip_g_zeros(&s);
    }
    if upper { s.to_uppercase() } else { s }
}

/// Format a float using C's `%a`/`%A` hexadecimal-float notation, e.g. `1.5`
/// → `0x1.8p+0`. Without an explicit precision the shortest exact fractional
/// representation is used (trailing zero hex digits stripped); with a
/// precision the fraction is rounded (round-half-to-even) to that many hex
/// digits. `upper` selects the `%A` (uppercase `0X`/`P`) form.
fn format_a(f: f64, prec: Option<usize>, upper: bool) -> String {
    if f.is_nan() {
        return if upper { "NAN".to_string() } else { "nan".to_string() };
    }
    if f.is_infinite() {
        let s = if f < 0.0 { "-inf" } else { "inf" };
        return if upper { s.to_uppercase() } else { s.to_string() };
    }
    let bits = f.to_bits();
    let neg = (bits >> 63) == 1;
    let exp_bits = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let mantissa = bits & 0x000f_ffff_ffff_ffff; // low 52 bits
    let (mut lead, exp2) = if exp_bits == 0 {
        // Zero or subnormal.
        if mantissa == 0 { (0u64, 0i64) } else { (0u64, -1022) }
    } else {
        (1u64, exp_bits - 1023)
    };
    // The 52-bit mantissa is exactly 13 hex digits (MSB first).
    let mut digits: Vec<u8> = (0..13)
        .map(|i| u8::try_from((mantissa >> (48 - 4 * i)) & 0xf).unwrap_or(0))
        .collect();
    if let Some(p) = prec {
        if p < digits.len() {
            let next = digits[p];
            let round_up = if next > 8 {
                true
            } else if next < 8 {
                false
            } else if digits[p + 1..].iter().any(|&d| d != 0) {
                true
            } else {
                // Exact halfway: round to even (up if the kept digit is odd).
                let last = if p == 0 { lead } else { u64::from(digits[p - 1]) };
                last % 2 == 1
            };
            digits.truncate(p);
            if round_up {
                let mut carry = true;
                for d in digits.iter_mut().rev() {
                    if *d == 0xf {
                        *d = 0;
                    } else {
                        *d += 1;
                        carry = false;
                        break;
                    }
                }
                if carry {
                    // A carry out of the fraction bumps the leading digit
                    // (e.g. `%.0a` of 1.5 → `0x2p+0`). glibc/bash keep the
                    // 2 rather than renormalizing to `0x1p+1`, so we do too.
                    lead += 1;
                }
            }
        } else {
            digits.resize(p, 0);
        }
    } else {
        while digits.last() == Some(&0) {
            digits.pop();
        }
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    s.push_str("0x");
    s.push(HEX[lead as usize] as char);
    if !digits.is_empty() {
        s.push('.');
        for &d in &digits {
            s.push(HEX[d as usize] as char);
        }
    }
    s.push('p');
    if exp2 >= 0 {
        s.push('+');
    } else {
        s.push('-');
    }
    s.push_str(&exp2.abs().to_string());
    if upper { s.to_uppercase() } else { s }
}

/// Evaluate a `test`/`[` expression. Returns `Ok(true)`/`Ok(false)` for the
/// boolean result, or `Err(operand)` when an arithmetic comparison was given a
/// non-integer operand (the caller reports `integer expression expected` and
/// exits 2, as bash does).
fn eval_test(a: &[&str]) -> Result<bool, String> {
    match a.len() {
        0 => Ok(false),
        1 => Ok(!a[0].is_empty()),
        2 => {
            // Unary operator.
            let (op, x) = (a[0], a[1]);
            if op == "!" {
                return Ok(x.is_empty());
            }
            Ok(eval_unary(op, x))
        }
        3 => {
            let (l, op, r) = (a[0], a[1], a[2]);
            // POSIX 3-argument rules, in order: a binary primary in the middle
            // wins first (so `[ ! = x ]` is a string comparison), then a leading
            // `!` negates the 2-argument test of the remaining operands (so
            // `[ ! -L path ]` / `[ ! -f path ]` work), then `( expr )` grouping.
            if is_test_binary_op(op) {
                return eval_binary(l, op, r);
            }
            if l == "!" {
                return Ok(!eval_test(&a[1..])?);
            }
            if l == "(" && r == ")" {
                return Ok(!op.is_empty());
            }
            eval_binary(l, op, r)
        }
        _ => {
            // Handle a leading `!`; otherwise fall back to the first 3 args.
            if a[0] == "!" {
                Ok(!eval_test(&a[1..])?)
            } else {
                eval_binary(a[0], a[1], a[2])
            }
        }
    }
}

/// Render the symbolic form of a umask value (bash `umask -S`): the string
/// describes the permissions that *remain* (the complement of the mask), e.g.
/// mask `0022` → `u=rwx,g=rx,o=rx`.
fn symbolic_umask_string(mask: u32) -> String {
    let allowed = !mask & 0o777;
    let mut parts = Vec::with_capacity(3);
    for (who, shift) in [('u', 6), ('g', 3), ('o', 0)] {
        let bits = (allowed >> shift) & 0o7;
        let mut perms = String::new();
        if bits & 0o4 != 0 {
            perms.push('r');
        }
        if bits & 0o2 != 0 {
            perms.push('w');
        }
        if bits & 0o1 != 0 {
            perms.push('x');
        }
        parts.push(format!("{who}={perms}"));
    }
    parts.join(",")
}

/// Parse a symbolic umask clause list (`u=rwx,g=rx,o=` / `a+r` / `go-w`) against
/// the current mask, returning the new mask value. The symbolic notation
/// operates on the *permission* set (the complement of the mask); the result is
/// re-complemented back into mask bits. Returns `None` on a malformed clause.
fn parse_symbolic_umask(current: u32, spec: &str) -> Option<u32> {
    // Work in "allowed permission" space, then invert back to a mask at the end.
    let mut allowed = !current & 0o777;
    for clause in spec.split(',') {
        if clause.is_empty() {
            continue;
        }
        let mut chars = clause.chars().peekable();
        // `who` set: any of u/g/o/a; empty defaults to `a` (all).
        let mut who_mask = 0u32; // bit per who: u=0o700, g=0o070, o=0o007
        while let Some(&c) = chars.peek() {
            match c {
                'u' => who_mask |= 0o700,
                'g' => who_mask |= 0o070,
                'o' => who_mask |= 0o007,
                'a' => who_mask |= 0o777,
                _ => break,
            }
            chars.next();
        }
        if who_mask == 0 {
            who_mask = 0o777;
        }
        let op = chars.next()?;
        if !matches!(op, '+' | '-' | '=') {
            return None;
        }
        // Permission letters → a 3-bit value replicated into every selected who.
        let mut pbits = 0u32;
        for c in chars {
            match c {
                'r' => pbits |= 0o4,
                'w' => pbits |= 0o2,
                'x' => pbits |= 0o1,
                _ => return None,
            }
        }
        let full = (pbits * 0o111) & who_mask; // spread rwx into u/g/o, then select
        match op {
            '+' => allowed |= full,
            '-' => allowed &= !full,
            '=' => {
                // Clear the selected who's bits, then set the new ones.
                allowed &= !who_mask;
                allowed |= full;
            }
            _ => unreachable!(),
        }
    }
    Some(!allowed & 0o777)
}

/// Whether a `cd` target is an *explicit* path (absolute or `.`/`..`-anchored)
/// for which `CDPATH` is not consulted — matching bash, which searches `CDPATH`
/// only for a bare relative name like `cd subdir`.
fn cd_is_explicit(t: &str) -> bool {
    t == "."
        || t == ".."
        || t.starts_with("./")
        || t.starts_with("../")
        || t.starts_with('/')
        || std::path::Path::new(t).is_absolute()
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
        // `-L`/`-h` — the path is a symbolic link. `symlink_metadata` does not
        // follow the final component, so a broken symlink still tests true.
        "-L" | "-h" => std::fs::symlink_metadata(x)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false),
        // `-t FD` — file descriptor `FD` is open and refers to a terminal.
        // Only the standard streams (0/1/2) are addressable from a shell.
        "-t" => match x.parse::<i32>() {
            Ok(0) => io::stdin().is_terminal(),
            Ok(1) => io::stdout().is_terminal(),
            Ok(2) => io::stderr().is_terminal(),
            _ => false,
        },
        _ => !x.is_empty(),
    }
}

/// Whether `op` is a `test`/`[` binary primary. Used to disambiguate the
/// 3-argument case (a binary operator in the middle beats a leading `!`).
fn is_test_binary_op(op: &str) -> bool {
    matches!(
        op,
        "=" | "==" | "!=" | "<" | ">" | "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" | "-nt"
            | "-ot" | "-ef"
    )
}

/// Parse an operand as a decimal integer for a `test`/`[` arithmetic
/// comparison (`-eq`, `-lt`, …). bash accepts optional surrounding whitespace
/// and a leading sign, but *only* base 10 — `0x10` is rejected here (unlike
/// `[[ … ]]`, which evaluates a full arithmetic expression). On failure the
/// operand is returned verbatim so the caller can report `integer expression
/// expected`, matching bash's diagnostic (and exit status 2).
fn test_parse_int(s: &str) -> Result<i64, String> {
    s.trim().parse::<i64>().map_err(|_| s.to_string())
}

/// Evaluate a `test`/`[` binary primary. Returns `Err(operand)` when an
/// arithmetic comparison is given a non-integer operand (bash prints
/// `integer expression expected` and exits 2 in that case).
fn eval_binary(l: &str, op: &str, r: &str) -> Result<bool, String> {
    match op {
        "=" | "==" => Ok(l == r),
        "!=" => Ok(l != r),
        "<" => Ok(l < r),
        ">" => Ok(l > r),
        "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
            // bash checks the left operand first, then the right.
            let a = test_parse_int(l)?;
            let b = test_parse_int(r)?;
            Ok(match op {
                "-eq" => a == b,
                "-ne" => a != b,
                "-lt" => a < b,
                "-le" => a <= b,
                "-gt" => a > b,
                "-ge" => a >= b,
                _ => false,
            })
        }
        "-nt" | "-ot" | "-ef" => Ok(file_cmp(op, l, r)),
        _ => Ok(false),
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
    use crate::parser::parse;

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
    fn syntax_error_message_not_doubled() {
        use crate::parser::ParseError;
        // A parser message that already opens with "syntax error" (bash's
        // canonical unexpected-token phrasing) must NOT get a second
        // "syntax error: " prefix.
        let e = ParseError("syntax error near unexpected token '--'".into());
        assert_eq!(
            format_parse_error(&e, "osh: "),
            "osh: syntax error near unexpected token '--'"
        );
        // A fragment-style message still gets the prefix.
        let e2 = ParseError("expected ')'".into());
        assert_eq!(
            format_parse_error(&e2, "osh: "),
            "osh: syntax error: expected ')'"
        );
    }

    #[test]
    fn error_prefix_includes_line_number_in_command_mode() {
        // In a non-interactive shell (`osh -c` / a script) bash prefixes every
        // diagnostic with `<name>: line <N>: `. osh matches that format (using
        // its own `$0` name). The default test `run()` harness is interactive-
        // like (no command/script mode), so it sees the bare `<name>: ` form;
        // here we exercise the command-mode path explicitly.
        fn run_cmd(src: &str) -> String {
            let mut sh = Shell::new();
            sh.set_command_mode();
            let mut buf = Vec::new();
            let prog = parse(src).expect("parse");
            {
                let mut out = Out::Capture(&mut buf);
                sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
            }
            String::from_utf8_lossy(&buf).into_owned()
        }
        // Unbound-variable diagnostic on line 1.
        assert_eq!(
            run_cmd("{ echo \"${y?}\"; } 2>&1"),
            "osh: line 1: y: parameter not set\n"
        );
        // A command-not-found error reports the line it occurs on (line 2),
        // and now honours the `2>&1` redirect (routed through errln).
        assert_eq!(
            run_cmd("echo hi\nno_such_cmd_xyz 2>&1"),
            "hi\nosh: line 2: no_such_cmd_xyz: command not found\n"
        );
        // readonly-assignment rejection also carries the line prefix.
        assert_eq!(
            run_cmd("readonly r=1\n{ r=2; } 2>&1"),
            "osh: line 2: r: readonly variable\n"
        );
        // Pure builtin *usage* messages (bash's `builtin_usage()`) stay
        // unprefixed even in command mode — just `<builtin>: usage: …`.
        assert_eq!(
            run_cmd("getopts 2>&1"),
            "getopts: usage: getopts optstring name [arg ...]\n"
        );
    }

    #[test]
    fn arith_error_matches_bash_format() {
        // bash reports arithmetic errors as `<name>: line N: [<builtin>: ]<expr>:
        // <body> (error token is "…")`. osh matches that format (bar its own
        // `$0` name): the `<expr>:` prefix, bash's body wording, the error
        // token, and the builtin tag (`(( `/`let`/`declare`) where bash uses one.
        fn run_cmd(src: &str) -> String {
            let mut sh = Shell::new();
            sh.set_command_mode();
            let mut buf = Vec::new();
            let prog = parse(src).expect("parse");
            {
                let mut out = Out::Capture(&mut buf);
                sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
            }
            String::from_utf8_lossy(&buf).into_owned()
        }
        // `$(( … ))` substitution: no builtin tag, `<expr>:` prefix. The
        // diagnostic is emitted during word expansion, before a *simple*
        // command's `2>&1` is installed (bash behaves the same), so wrap in a
        // brace group whose redirect is already active during expansion.
        assert_eq!(
            run_cmd("{ echo $((1/0)); } 2>&1"),
            "osh: line 1: 1/0: division by 0 (error token is \"0\")\n"
        );
        assert_eq!(
            run_cmd("{ echo $((5 +)); } 2>&1"),
            "osh: line 1: 5 +: syntax error: operand expected (error token is \"+\")\n"
        );
        // `(( … ))` command is tagged `((`.
        assert_eq!(
            run_cmd("(( 1/0 )) 2>&1"),
            "osh: line 1: ((: 1/0 : division by 0 (error token is \"0 \")\n"
        );
        // `let` is tagged `let`.
        assert_eq!(
            run_cmd("let '3 x' 2>&1"),
            "osh: line 1: let: 3 x: syntax error in expression (error token is \"x\")\n"
        );
        // `declare -i` is tagged `declare`; a plain `-i` assignment is untagged.
        assert_eq!(
            run_cmd("declare -i k='3 x' 2>&1"),
            "osh: line 1: declare: 3 x: syntax error in expression (error token is \"x\")\n"
        );
        assert_eq!(
            run_cmd("declare -i k; { k='3 x'; } 2>&1"),
            "osh: line 1: 3 x: syntax error in expression (error token is \"x\")\n"
        );
        // An active `2>/dev/null` silences the diagnostic (routed through errln).
        assert_eq!(run_cmd("let '1/0' 2>/dev/null; echo $?"), "1\n");
    }

    #[test]
    fn coproc_basic_default_name() {
        // `coproc { … }` with no explicit name → array `COPROC`; `COPROC[0]`
        // reads the coproc's stdout.
        assert_eq!(
            run(r#"coproc { echo fromco; }; read x <&"${COPROC[0]}"; echo "$x""#).0,
            "fromco\n"
        );
    }

    #[test]
    fn coproc_simple_command_body() {
        // A *simple* command body also works and defaults to `COPROC` (no
        // explicit name is accepted before a simple command).
        assert_eq!(
            run(r#"coproc echo hello; read x <&"${COPROC[0]}"; echo "$x""#).0,
            "hello\n"
        );
    }

    #[test]
    fn coproc_named() {
        // `coproc NAME compound` → the named array holds the endpoints.
        assert_eq!(
            run(r#"coproc myco { echo hi; }; read x <&"${myco[0]}"; echo "$x""#).0,
            "hi\n"
        );
    }

    #[test]
    fn coproc_bidirectional() {
        // Write to `COPROC[1]` (the coproc's stdin), read its reply from
        // `COPROC[0]` — the full round trip through both pipes.
        assert_eq!(
            run(r#"coproc { read line; echo "got:$line"; }
echo feed >&"${COPROC[1]}"
read out <&"${COPROC[0]}"
echo "$out""#)
            .0,
            "got:feed\n"
        );
    }

    #[test]
    fn coproc_read_u_fd() {
        // `read -u "${COPROC[0]}"` reads the coproc's stdout via the -u path.
        assert_eq!(
            run(r#"coproc { echo viaU; }; read -u "${COPROC[0]}" x; echo "$x""#).0,
            "viaU\n"
        );
    }

    #[test]
    fn coproc_multiple_lines_successive_reads() {
        // Successive `read <&N` on a live coproc fd must consume successive
        // lines — a fresh chunk-buffering reader per read would drop the second.
        assert_eq!(
            run(r#"coproc { printf 'a\nb\n'; }
read x <&"${COPROC[0]}"
read y <&"${COPROC[0]}"
echo "$x $y""#)
            .0,
            "a b\n"
        );
    }

    #[test]
    fn coproc_pid_is_set() {
        // `NAME_PID` is populated (a synthetic id for the in-process body).
        assert_eq!(
            run(r#"coproc { echo x; }; read _ <&"${COPROC[0]}"; [[ -n $COPROC_PID ]] && echo haspid"#).0,
            "haspid\n"
        );
    }

    #[test]
    fn coproc_endpoint_fds_are_high() {
        // Both endpoints are auto-allocated descriptors ≥ 10 (like varfds), and
        // the two are distinct.
        assert_eq!(
            run(r#"coproc { echo x; }
read _ <&"${COPROC[0]}"
r=${COPROC[0]}; w=${COPROC[1]}
if (( r >= 10 && w >= 10 && r != w )); then echo ok; fi"#)
            .0,
            "ok\n"
        );
    }

    #[test]
    fn coproc_parses_named_only_before_compound() {
        // `coproc NAME { … }` is a named coproc; the AST records the name.
        let prog = parse("coproc c1 { echo hi; }").expect("parse");
        match &prog.items[0].list.first.commands[0] {
            Command::Coproc { name, .. } => assert_eq!(name.as_deref(), Some("c1")),
            other => panic!("expected coproc, got {other:?}"),
        }
        // `coproc echo hi` is an *unnamed* coproc over a simple command (no name
        // is consumed before a simple command).
        let prog = parse("coproc echo hi").expect("parse");
        match &prog.items[0].list.first.commands[0] {
            Command::Coproc { name, .. } => assert_eq!(*name, None),
            other => panic!("expected coproc, got {other:?}"),
        }
    }

    /// Run `setup` (to define aliases), then parse+run `src` with those aliases
    /// expanded over the token stream — mirroring bash's rule that aliases apply
    /// to input read *after* the alias definition, not within the same parse.
    fn run_with_aliases(setup: &str, src: &str) -> (String, i32) {
        let mut sh = Shell::new();
        sh.run_source(setup);
        let mut buf = Vec::new();
        let prog = parse_with_aliases(src, &sh.aliases).expect("parse");
        {
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
        }
        (String::from_utf8_lossy(&buf).into_owned(), sh.last_status)
    }

    #[test]
    fn alias_expands_in_command_position() {
        let (o, s) = run_with_aliases("alias greet='echo hello'", "greet");
        assert_eq!(o, "hello\n");
        assert_eq!(s, 0);
    }

    #[test]
    fn alias_arguments_append_after_replacement() {
        let (o, _) = run_with_aliases("alias ll='echo LL'", "ll world");
        assert_eq!(o, "LL world\n");
    }

    #[test]
    fn alias_only_expands_command_word() {
        // `greet` as an argument is not alias-expanded.
        let (o, _) = run_with_aliases("alias greet='echo hello'", "echo greet");
        assert_eq!(o, "greet\n");
    }

    #[test]
    fn expand_aliases_shopt_is_recognized() {
        // `shopt -s/-u/-q expand_aliases` is a valid bash option name and must
        // not error with "invalid shell option name" (regression).
        assert_eq!(run("shopt -s expand_aliases; echo done"), ("done\n".into(), 0));
        assert_eq!(run("shopt -u expand_aliases; echo done"), ("done\n".into(), 0));
        // Toggling is observable via `shopt -q`.
        assert_eq!(run("shopt -s expand_aliases; shopt -q expand_aliases; echo $?").0, "0\n");
        assert_eq!(run("shopt -u expand_aliases; shopt -q expand_aliases; echo $?").0, "1\n");
        // It appears in the bare `shopt` listing.
        assert!(run("shopt").0.contains("expand_aliases"), "got: {:?}", run("shopt").0);
    }

    #[test]
    fn aliases_gated_on_expand_aliases_in_noninteractive_mode() {
        // In `-c`/script (non-interactive) mode `expand_aliases` defaults off, so
        // an alias defined in a prior parse unit is NOT expanded unless the shell
        // opts in — matching bash. A default (interactive-mode) shell keeps
        // expanding, which is what `run_with_aliases` exercises elsewhere.
        let mut sh = Shell::new();
        sh.set_command_mode();
        sh.run_source("alias g='echo hi'");
        let mut buf = Vec::new();
        {
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&parse_with_aliases("g", &sh.aliases).expect("parse"), &mut out, &StdinSrc::Inherit);
        }
        // Even though the alias is in the table, run_source in command mode does
        // not expand it; here we prove the gate itself is off by default.
        assert!(!sh.aliases_enabled(), "expand_aliases should default off in command mode");
        // Opting in flips the gate on.
        sh.run_source("shopt -s expand_aliases");
        assert!(sh.aliases_enabled(), "shopt -s expand_aliases should enable expansion");
    }

    #[test]
    fn alias_self_reference_terminates() {
        // `echo` aliased to `echo hi` must not recurse forever; the guard stops
        // the inner `echo` from re-expanding.
        let (o, _) = run_with_aliases("alias echo='echo hi'", "echo there");
        assert_eq!(o, "hi there\n");
    }

    #[test]
    fn alias_trailing_blank_expands_next_word() {
        // A value ending in a blank makes the following word alias-eligible.
        let (o, _) = run_with_aliases(
            "alias sudo='echo SUDO '; alias ll='echo LL'",
            "sudo ll",
        );
        assert_eq!(o, "SUDO echo LL\n");
    }

    #[test]
    fn alias_listing_and_single_lookup() {
        let (o, s) = run("alias foo='bar baz'; alias foo");
        assert_eq!(s, 0);
        assert_eq!(o, "alias foo='bar baz'\n");
    }

    #[test]
    fn alias_missing_lookup_errors() {
        let (_, s) = run("alias nope");
        assert_eq!(s, 1);
    }

    #[test]
    fn unalias_removes_and_reports_missing() {
        let (_, s) = run("alias foo='x'; unalias foo; alias foo");
        assert_eq!(s, 1);
    }

    #[test]
    fn unalias_all_clears_every_alias() {
        let (o, _) = run("alias a='1'; alias b='2'; unalias -a; alias");
        assert_eq!(o, "");
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
    fn param_default_colon_vs_plain() {
        // Colon form: empty-but-set uses the default.
        let (o, _) = run("x=; echo [${x:-D}]");
        assert_eq!(o, "[D]\n");
        // Plain form: empty-but-set is "set", so no default is applied.
        let (o, _) = run("x=; echo [${x-D}]");
        assert_eq!(o, "[]\n");
        // Both forms use the default when genuinely unset.
        let (o, _) = run("unset x; echo [${x:-D}][${x-D}]");
        assert_eq!(o, "[D][D]\n");
        // Set and non-empty: neither form applies the default.
        let (o, _) = run("x=v; echo [${x:-D}][${x-D}]");
        assert_eq!(o, "[v][v]\n");
    }

    #[test]
    fn param_alternate_colon_vs_plain() {
        // Colon form (`:+`): active only when set AND non-empty.
        let (o, _) = run("x=; echo [${x:+A}]");
        assert_eq!(o, "[]\n");
        // Plain form (`+`): active whenever set, even if empty.
        let (o, _) = run("x=; echo [${x+A}]");
        assert_eq!(o, "[A]\n");
        // Unset: neither form is active.
        let (o, _) = run("unset x; echo [${x:+A}][${x+A}]");
        assert_eq!(o, "[][]\n");
        // Set non-empty: both active.
        let (o, _) = run("x=v; echo [${x:+A}][${x+A}]");
        assert_eq!(o, "[A][A]\n");
    }

    #[test]
    fn param_assign_default_plain() {
        // Plain `=`: empty-but-set is left as-is (empty), not reassigned.
        let (o, _) = run("x=; echo [${x=D}][$x]");
        assert_eq!(o, "[][]\n");
        // Unset: assigns the default.
        let (o, _) = run("unset x; echo [${x=D}][$x]");
        assert_eq!(o, "[D][D]\n");
        // Colon `:=`: empty-but-set gets reassigned to the default.
        let (o, _) = run("x=; echo [${x:=D}][$x]");
        assert_eq!(o, "[D][D]\n");
    }

    #[test]
    fn arithmetic() {
        let (o, _) = run("echo $((6 * 7))");
        assert_eq!(o, "42\n");
    }

    #[test]
    fn echo_e_interprets_escapes() {
        // `-e` interprets backslash escapes; `-E`/default do not.
        assert_eq!(run("echo -e 'a\\nb'").0, "a\nb\n");
        assert_eq!(run("echo -e 'a\\tb'").0, "a\tb\n");
        assert_eq!(run("echo 'a\\nb'").0, "a\\nb\n");
        assert_eq!(run("echo -E 'a\\nb'").0, "a\\nb\n");
        // Clustered flags: `-ne` = no newline + interpret.
        assert_eq!(run("echo -ne 'x\\ty'").0, "x\ty");
        // `\c` stops output and suppresses the trailing newline.
        assert_eq!(run("echo -e 'keep\\cdrop'").0, "keep");
        // `\xHH` hex; `\0nnn` octal (needs the leading 0, else literal).
        assert_eq!(run("echo -e '\\x41\\0101'").0, "AA\n");
        assert_eq!(run("echo -e '\\101'").0, "\\101\n");
        // `\uHHHH` / `\UHHHHHHHH` Unicode code points, emitted as UTF-8
        // (matching osh's `$'…'` decoder). A missing hex digit stays literal.
        assert_eq!(run("echo -ne '\\u00e9'").0, "\u{e9}");
        assert_eq!(run("echo -ne '\\U0001F600'").0, "\u{1F600}");
        assert_eq!(run("echo -ne '\\u41'").0, "A");
        assert_eq!(run("echo -ne '\\uZ'").0, "\\uZ");
    }

    #[test]
    fn arithmetic_with_nested_command_sub() {
        // Command substitutions and nested arithmetic embedded in a `$(( ))`
        // expression must be expanded before evaluation.
        assert_eq!(run("f() { echo 5; }; echo $(( $(f) + 1 ))").0, "6\n");
        assert_eq!(run("f() { echo 5; }; echo $(( $(f) + $(f) ))").0, "10\n");
        assert_eq!(run("n=3; echo $(( $((n-1)) + 1 ))").0, "3\n");
        assert_eq!(run("echo $(( `echo 4` * 2 ))").0, "8\n");
        // Recursion through arithmetic command substitution (fibonacci).
        let fib = "fib() { local n=$1; if ((n<2)); then echo $n; \
                   else echo $(( $(fib $((n-1))) + $(fib $((n-2))) )); fi; }; fib 7";
        assert_eq!(run(fib).0, "13\n");
    }

    #[test]
    fn arithmetic_with_braced_param_expansion() {
        // A `${…}` inside `$(( ))` is a full parameter expansion, not just a
        // bare-name lookup: length (`${#x}`, `${#a[@]}`), operators (`${x:-N}`),
        // and subscripts must all evaluate. (Regression: these previously
        // yielded 0 because only bare names were handled.)
        assert_eq!(run("x=5; echo $(( ${#x} ))").0, "1\n");
        assert_eq!(run("x=hello; echo $(( ${#x} * 2 ))").0, "10\n");
        assert_eq!(run("a=(x y z); echo $(( ${#a[@]} ))").0, "3\n");
        assert_eq!(run("a=(x y z); echo $(( ${#a[@]} + 1 ))").0, "4\n");
        assert_eq!(run("echo $(( ${#BASH_VERSINFO[@]} ))").0, "6\n");
        // Operator forms and nested braces.
        assert_eq!(run("echo $(( ${x:-3} + 1 ))").0, "4\n");
        assert_eq!(run("x=10; echo $(( ${x:-3} + 1 ))").0, "11\n");
        assert_eq!(run("a=(5 6 7); echo $(( a[1] + ${a[2]} ))").0, "13\n");
        // Unset length is 0.
        assert_eq!(run("echo $(( ${#nonexist} ))").0, "0\n");
    }

    #[test]
    fn command_substitution() {
        let (o, _) = run("echo [$(echo inner)]");
        assert_eq!(o, "[inner]\n");
    }

    #[test]
    fn command_sub_read_file() {
        // `$(< file)` reads the file's contents (bash fast path), stripping the
        // trailing newline like any command substitution.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("osh_readf_{}_{}.txt", std::process::id(), nanos));
        let p = path.to_string_lossy().replace('\\', "/");
        std::fs::write(&path, b"hello world\nsecond line\n").expect("write");
        let (o, st) = run(&format!("x=$(< {p}); echo \"[$x]\""));
        assert_eq!(st, 0);
        assert_eq!(o, "[hello world\nsecond line]\n");
        // Also works with the `< file` form embedded directly in a comsub arg.
        let (o2, _) = run(&format!("echo \"<$(<{p})>\""));
        assert_eq!(o2, "<hello world\nsecond line>\n");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn error_if_unset_aborts_shell() {
        // `${var:?msg}` on an unset parameter writes the message and aborts the
        // (sub)shell before the command runs — the `echo` never executes.
        let (o, _) = run("unset zz; (echo \"${zz:?is unset}\") 2>/dev/null; echo \"after=$?\"");
        assert_eq!(o, "after=1\n");
        // A set, non-empty parameter is unaffected.
        let (o2, _) = run("y=set; echo \"${y:?msg}\"");
        assert_eq!(o2, "set\n");
        // At top level (main shell) the error aborts the whole run with 127,
        // matching bash — whereas the subshell case above yields 1.
        let (o3, st3) = run("unset q; echo \"${q:?gone}\" 2>/dev/null; echo unreached");
        assert_eq!(o3, "");
        assert_eq!(st3, 127);
    }

    #[test]
    fn fatal_expansion_abort_status_by_context() {
        // bash aborts a nounset / `${var:?}` error with 127 in the *main shell
        // environment* (top level, brace groups, function bodies) but with 1
        // inside a subshell / command substitution — and a subshell error does
        // not abort the enclosing shell. Bad indirect / subscript expansions are
        // always 1. These mirror bash 5.2, verified directly.

        // Main shell (top level): nounset and `:?` both give 127.
        assert_eq!(run("set -u; echo $undef; echo after").1, 127);
        assert_eq!(run("unset q; echo ${q:?} 2>/dev/null; echo after").1, 127);
        // Brace group is still the main shell environment -> 127.
        assert_eq!(run("set -u; { echo $undef; }").1, 127);
        // Function body runs in the main shell environment -> 127.
        assert_eq!(run("f(){ echo $undef; }; set -u; f; echo after").1, 127);
        // Bare and array assignments abort the main shell with 127 too.
        assert_eq!(run("set -u; x=$undef").1, 127);
        assert_eq!(run("set -u; a=($undef)").1, 127);

        // A subshell aborts with 1 and does NOT abort the parent (which goes on
        // to run `echo done` and exit 0).
        assert_eq!(run("(set -u; echo $undef)").1, 1);
        assert_eq!(run("(set -u; echo $undef); echo done").1, 0);
        // Command substitution is a subshell: it fails with 1, parent survives.
        assert_eq!(run("x=$(set -u; echo $undef); echo $?").0, "1\n");

        // Bad indirect / subscript expansions are always 1, even at the main
        // shell (no 127 remap).
        assert_eq!(run("echo ${!badref}").1, 1);
    }

    #[test]
    fn integer_assignment_arith_error_is_fatal() {
        // A bad arithmetic expression bound to an *integer-attribute* variable is
        // fatal in bash (status 1, the shell aborts) — unlike `let`/`(( ))`,
        // which merely return non-zero. Covers `declare -i NAME=EXPR`, a plain
        // assignment to an already-`-i` variable, `declare -ia` array elements,
        // and `name[i]=EXPR`. Both syntax errors and division by zero abort.
        assert_eq!(run("declare -i k=\"3 apples\"; echo after"), (String::new(), 1));
        assert_eq!(run("declare -i k=\"1/0\"; echo after"), (String::new(), 1));
        assert_eq!(run("declare -i k; k=\"3 apples\"; echo after"), (String::new(), 1));
        assert_eq!(run("declare -ia arr=(1 \"2 x\" 3); echo after"), (String::new(), 1));
        assert_eq!(run("declare -i a[0]=\"2 x\"; echo after"), (String::new(), 1));
        // A valid integer initializer still works and is non-fatal.
        assert_eq!(run("declare -i k=\"5+3\"; echo $k").0, "8\n");
        // A bare word that is a (bare) identifier is a variable reference -> 0,
        // not an error.
        assert_eq!(run("declare -i k=abc; echo $k").0, "0\n");

        // In a subshell / command substitution the abort is status 1 and does
        // NOT abort the parent.
        assert_eq!(run("(declare -i k=\"3 apples\"); echo done").1, 0);
        assert_eq!(run("x=$(declare -i k=\"3 apples\"; echo in); echo $?").0, "1\n");

        // `let` / `(( ))` remain non-fatal (the following command still runs).
        assert_eq!(run("let \"3 apples\" 2>/dev/null; echo after").0, "after\n");
        assert_eq!(run("(( 3 apples )) 2>/dev/null; echo after").0, "after\n");
    }

    #[test]
    fn subscript_and_substring_arith_error_is_fatal() {
        // Arithmetic errors that occur while *expanding a word* — an array
        // subscript, a substring/slice offset or length — are fatal in bash
        // (status 1, the shell aborts) at the main-shell level, but only fail
        // the subshell (status 1, parent survives) inside `( )`/`$( )`. This is
        // distinct from `[[ x -eq y ]]` / `[ x -eq y ]`, whose arithmetic
        // comparisons are non-fatal.

        // Array subscript read with a bad arithmetic expression -> fatal.
        assert_eq!(run("a=(1 2 3); echo ${a[3 x]}; echo after"), (String::new(), 1));
        // Substring offset with a bad arithmetic expression -> fatal.
        assert_eq!(run("x=abcdef; echo ${x:1 z}; echo after"), (String::new(), 1));
        // Substring length with a bad arithmetic expression -> fatal.
        assert_eq!(run("x=abcdef; echo ${x:1:2 z}; echo after"), (String::new(), 1));

        // In a subshell the abort is status 1 and does NOT abort the parent.
        assert_eq!(run("a=(1 2 3); (echo ${a[3 x]}); echo done").1, 0);
        assert_eq!(run("x=$(a=(1 2 3); echo ${a[3 x]}); echo $?").0, "1\n");

        // `[[ … -eq … ]]` / `[ … -eq … ]` arithmetic errors are NOT fatal: the
        // conditional reports non-zero but the following command still runs.
        assert_eq!(run("[[ \"3 x\" -eq 5 ]] 2>/dev/null; echo after").0, "after\n");
        assert_eq!(run("[ \"3 x\" -eq 5 ] 2>/dev/null; echo after").0, "after\n");
    }

    #[test]
    fn array_slice_negative_length_is_fatal() {
        // An array / positional-parameter slice rejects a *negative length* as a
        // fatal expansion error ("N: substring expression < 0"), unlike a string
        // substring where a negative length counts back from the end. Fatal at
        // the main shell (status 1, aborts), status 1 without aborting the parent
        // inside a subshell.
        assert_eq!(run("a=(1 2 3 4 5); echo ${a[@]:1:-1}; echo after"), (String::new(), 1));
        assert_eq!(run("a=(1 2 3); echo ${a[*]:0:-5}; echo after"), (String::new(), 1));
        assert_eq!(run("set -- a b c d e; echo ${@:1:-1}; echo after"), (String::new(), 1));
        // Subshell: fails with 1 but the parent survives to run `echo done`.
        assert_eq!(run("(a=(1 2 3); echo ${a[@]:0:-1}); echo done").1, 0);
        assert_eq!(run("x=$(a=(1 2 3); echo ${a[@]:0:-1}); echo $?").0, "1\n");
        // A string substring keeps its from-the-end negative-length semantics.
        assert_eq!(run("s=abcdef; echo ${s:1:-1}").0, "bcde\n");
        // A zero or positive length still slices normally.
        assert_eq!(run("a=(1 2 3 4 5); echo \"[${a[@]:1:0}]\"").0, "[]\n");
        assert_eq!(run("a=(1 2 3 4 5); echo ${a[@]:1:2}").0, "2 3\n");
    }

    #[test]
    fn arith_error_diagnostic_honors_stderr_redirect() {
        // Arithmetic-error diagnostics go through the shell's stderr routing, so
        // a command-scoped `2>/dev/null` silences them (bash parity) — regression
        // for a prior `eprintln!` that bypassed the redirect.
        assert_eq!(run("let \"3 apples\" 2>/dev/null; echo after"), ("after\n".into(), 0));
        assert_eq!(
            run("declare -i k=\"3 apples\" 2>/dev/null; echo after"),
            (String::new(), 1)
        );
        assert_eq!(
            run("for ((i=0;i<\"a b\";i++)); do :; done 2>/dev/null; echo after").0,
            "after\n"
        );
    }

    #[test]
    fn arith_error_in_word_aborts_command() {
        // Division by zero in a `$(( … ))` word is fatal in a non-interactive
        // shell (bash): the shell exits with status 1 without fabricating a "0",
        // so the following command never runs.
        let (o, st) = run("echo $((10/0)) 2>/dev/null; echo \"next=$?\"");
        assert_eq!(o, "");
        assert_eq!(st, 1);
        // An arithmetic error in an assignment value is likewise fatal.
        let (o2, st2) = run("x=$((1/0)) 2>/dev/null; echo \"st=$?\"");
        assert_eq!(o2, "");
        assert_eq!(st2, 1);
        // A `(( … ))` command reports its own error/status and does NOT leak the
        // abort flag onto the following command — it is not fatal.
        let (o3, _) = run("(( 1/0 )) 2>/dev/null; echo ok");
        assert_eq!(o3, "ok\n");
    }

    #[test]
    fn command_sub_runs_in_isolated_subshell() {
        // Variable assignments inside `$(...)` must not leak into the parent
        // shell (command substitution runs in a subshell, like bash).
        let (o, _) = run("count=0; r=$(count=5; echo $count); echo \"$r $count\"");
        assert_eq!(o, "5 0\n");
        // The command substitution still sees the parent's variables.
        let (o2, _) = run("x=hi; r=$(echo $x); echo $r");
        assert_eq!(o2, "hi\n");
        // Exit status of the command substitution propagates to $?.
        let (o3, _) = run("r=$(false); echo $?");
        assert_eq!(o3, "1\n");
        // A function defined inside `$(...)` does not persist afterwards.
        let (o4, _) = run("r=$(f(){ echo in; }; f); type -t f 2>/dev/null; echo \"[$r]\"");
        assert_eq!(o4, "[in]\n");
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
    fn test_builtin_integer_expression_error() {
        // A non-integer operand to an arithmetic comparison is an *error*
        // (exit 2), not a false result (exit 1) — matching bash. Base-10 only:
        // `0x10` is rejected here even though `[[ … ]]` would evaluate it.
        assert_eq!(run("[ 12 -eq 12.0 ]").1, 2);
        assert_eq!(run("[ 12.0 -eq 12 ]").1, 2);
        assert_eq!(run("[ 12 -eq abc ]").1, 2);
        assert_eq!(run("[ \"\" -eq 5 ]").1, 2);
        assert_eq!(run("[ 0x10 -eq 16 ]").1, 2);
        assert_eq!(run("test 12 -eq 12.0").1, 2);
        // Surrounding whitespace and a leading sign are still valid integers.
        assert_eq!(run("[ \" 5\" -eq 5 ]").1, 0);
        assert_eq!(run("[ +5 -eq 5 ]").1, 0);
        assert_eq!(run("[ -5 -lt 0 ]").1, 0);
        assert_eq!(run("[ 007 -eq 7 ]").1, 0);
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
    fn read_partial_final_line_status_one() {
        // A final line ending at EOF without a newline still assigns the value
        // but reports status 1 (matching bash). Here: two reads of "a\nb" — the
        // first line is newline-terminated (rc 0), the second hits EOF (rc 1).
        let (o, _) = run(
            "printf 'a\\nb' | { read x; echo \"rc1=$? x=$x\"; read y; echo \"rc2=$? y=$y\"; }",
        );
        assert_eq!(o, "rc1=0 x=a\nrc2=1 y=b\n");
        // A newline-terminated single line reports success.
        let (o2, _) = run("printf 'a\\n' | { read x; echo \"rc=$? x=$x\"; }");
        assert_eq!(o2, "rc=0 x=a\n");
    }

    #[test]
    fn read_custom_ifs() {
        let (o, _) = run("IFS=: read a b c <<< '1:2:3'; echo \"$a-$b-$c\"");
        assert_eq!(o, "1-2-3\n");
    }

    #[test]
    fn unquoted_word_split_honors_ifs() {
        // Unquoted expansion splits on a custom IFS, not just whitespace.
        assert_eq!(
            run(r#"IFS=:; x="a:b:c"; for w in $x; do echo "<$w>"; done"#).0,
            "<a>\n<b>\n<c>\n"
        );
        // Adjacent non-whitespace delimiters preserve an empty field.
        assert_eq!(
            run(r#"IFS=:; x="a::c"; for w in $x; do echo "<$w>"; done"#).0,
            "<a>\n<>\n<c>\n"
        );
        // A leading non-whitespace delimiter yields a leading empty field; a
        // trailing one does not add a trailing empty field.
        assert_eq!(
            run(r#"IFS=:; x=":a:"; for w in $x; do echo "<$w>"; done"#).0,
            "<>\n<a>\n"
        );
        // IFS whitespace runs still collapse and trim.
        assert_eq!(
            run(r#"x="  a   b  "; for w in $x; do echo "<$w>"; done"#).0,
            "<a>\n<b>\n"
        );
        // Mixed whitespace + non-whitespace IFS: whitespace around the delimiter
        // is absorbed.
        assert_eq!(
            run(r#"IFS=' :'; x="a : b"; for w in $x; do echo "<$w>"; done"#).0,
            "<a>\n<b>\n"
        );
    }

    #[test]
    fn unquoted_word_split_boundary_delims() {
        // IFS whitespace at an expansion boundary must break against adjacent
        // literal text (the classic bug: `[$x]` gluing `[` to the first field).
        // Values here match real bash's field splitting exactly.
        assert_eq!(
            run(r#"x="  a b  "; printf "<%s>" [$x]; echo"#).0,
            "<[><a><b><]>\n"
        );
        // Leading boundary whitespace splits `pre` from the first field.
        assert_eq!(run(r#"x="  a  "; printf "<%s>" pre$x; echo"#).0, "<pre><a>\n");
        // Trailing boundary whitespace splits the last field from `suf`.
        assert_eq!(
            run(r#"x="  a  "; printf "<%s>" ${x}suf; echo"#).0,
            "<a><suf>\n"
        );
        // An all-whitespace expansion between two literals still forces a split.
        assert_eq!(
            run(r#"x="  "; printf "<%s>" pre${x}suf; echo"#).0,
            "<pre><suf>\n"
        );
        // An empty (unset) expansion does NOT split — the literals glue.
        assert_eq!(
            run(r#"x=""; printf "<%s>" pre${x}suf; echo"#).0,
            "<presuf>\n"
        );
        // Non-whitespace IFS boundary: a trailing delimiter splits against the
        // following literal without producing a spurious empty field.
        assert_eq!(
            run(r#"IFS=:; x="a:"; printf "<%s>" ${x}suf; echo"#).0,
            "<a><suf>\n"
        );
        // A bare non-whitespace delimiter after literal content closes that
        // field (no empty field appears).
        assert_eq!(
            run(r#"IFS=:; x=":"; printf "<%s>" a$x; echo"#).0,
            "<a>\n"
        );
        // Mixed IFS `ws* nonws ws*` between two literals collapses to a single
        // delimiter (the non-whitespace char is absorbed into the whitespace
        // run), so no empty field appears between `pre` and `suf`.
        assert_eq!(
            run(r#"IFS=" :"; x=" : "; printf "<%s>" pre${x}suf; echo"#).0,
            "<pre><suf>\n"
        );
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
    fn read_attached_and_clustered_options() {
        // Attached option-argument: `-d:` glues the delimiter to the flag.
        assert_eq!(
            run("{ read -d: x; read -d: y; } <<< 'a:b:c'; echo \"$x-$y\"").0,
            "a-b\n"
        );
        // Attached numeric argument: `-n3`.
        assert_eq!(run("read -n3 x <<< 'abcdef'; echo \"$x\"").0, "abc\n");
        // Clustered flags with a trailing attached argument: `-rn3`.
        assert_eq!(run("read -rn3 x <<< 'ab\\cd'; echo \"$x\"").0, "ab\\\n");
        // Separated form still works.
        assert_eq!(run("read -d ':' p <<< 'a:b'; echo \"$p\"").0, "a\n");
    }

    #[test]
    fn read_prompt_suppressed_for_non_tty_input() {
        // `-p PROMPT` is written to stderr *only* when the input is a real
        // terminal. Under a pipeline, here-string, or redirect the read is
        // silent. We fold stderr into stdout via a command substitution
        // (`2>&1`) so that a wrongly-emitted prompt would show up in the
        // captured text; the fix keeps it empty.
        assert_eq!(
            run("out=$(echo x | { read -p 'P: ' y; echo \"$y\"; } 2>&1); echo \"[$out]\"").0,
            "[x]\n"
        );
        assert_eq!(
            run("out=$({ read -p 'P: ' y; echo \"$y\"; } 2>&1 <<< 'hi'); echo \"[$out]\"").0,
            "[hi]\n"
        );
        // The value is still read correctly regardless of the prompt.
        assert_eq!(
            run("printf 'a b\\n' | { read -p '> ' x z; echo \"$x-$z\"; }").0,
            "a-b\n"
        );
    }

    #[test]
    fn scalar_assign_to_array_updates_element_zero() {
        // bash: a plain scalar assignment (or `read`) to an existing indexed
        // array updates element 0, leaving the other elements intact.
        assert_eq!(run("a=(1 2 3); a=x; echo \"$a ${a[@]}\"").0, "x x 2 3\n");
        assert_eq!(run("a=(1 2 3); read a <<< 'q'; echo \"$a ${a[@]}\"").0, "q q 2 3\n");
        // Integer attribute still evaluates the RHS and lands in element 0.
        assert_eq!(run("declare -ia a=(1 2 3); a=4+5; echo \"$a ${a[@]}\"").0, "9 9 2 3\n");
    }

    #[test]
    fn declare_array_literal_applies_value_attributes() {
        // `declare -ia`/`-ai` sets the integer attribute on the array, so later
        // element assignments evaluate arithmetically.
        assert_eq!(run("declare -ai b=(1 2 3); b[1]=6+6; echo \"${b[@]}\"").0, "1 12 3\n");
        // `declare -ua` uppercases values stored into the array.
        assert_eq!(run("declare -ua u=(ab cd); u[0]=xy; echo \"${u[@]}\"").0, "XY CD\n");
        // `declare -ra` makes the array readonly: a later element assignment is
        // fatal in a non-interactive shell (bash) — the shell exits with status
        // 1, so the following `echo` lines never run.
        let (o, s) = run("declare -ra r=(1 2); r[0]=9; echo status=$?; echo \"${r[@]}\"");
        assert_eq!(o, "");
        assert_eq!(s, 1);
    }

    #[test]
    fn command_not_found_handle_invoked_with_args() {
        // A defined `command_not_found_handle` receives the command word as `$1`
        // and its arguments as `$2`…, and its exit status becomes `$?`.
        let src = "command_not_found_handle() { echo \"caught: $1 $2\"; return 42; }; \
                   no_such_cmd_xyz123 abc; echo status=$?";
        assert_eq!(run(src).0, "caught: no_such_cmd_xyz123 abc\nstatus=42\n");
    }

    #[test]
    fn command_not_found_handle_absent_reports_127() {
        // Without the handler, a missing command still reports 127.
        assert_eq!(run("no_such_cmd_xyz123; echo $?").0, "127\n");
    }

    #[test]
    fn command_not_found_handle_skipped_for_path_names() {
        // A name containing a slash bypasses the handler (bash: a slash path that
        // does not exist is a spawn error, not a "command not found" lookup).
        let src = "command_not_found_handle() { echo caught; }; ./no_such_cmd_xyz123; echo $?";
        assert_eq!(run(src).0, "127\n");
    }

    #[test]
    fn command_not_found_honors_stderr_redirect() {
        // The "command not found" diagnostic must follow the command's own fd 2,
        // not the shell's real stderr (bash: a redirected `2>` on the missing
        // command captures/silences the message). Without a redirect the message
        // goes to the shell's real stderr, so the stdout capture holds only the
        // trailing `done`.
        let (o, _) = run("no_such_cmd_xyz123; echo done");
        assert_eq!(o, "done\n");
        // `2>&1` routes fd 2 into fd 1: the diagnostic now lands in the stdout
        // capture, matching bash.
        let (o2, s2) = run("no_such_cmd_xyz123 2>&1; echo done");
        assert_eq!(o2, "osh: no_such_cmd_xyz123: command not found\ndone\n");
        assert_eq!(s2, 0);
    }

    #[test]
    fn error_if_unset_message_and_subscript() {
        // Colon-less `?` tests only unset ("parameter not set"); colon `:?` tests
        // null-or-unset ("parameter null or not set"). A custom message overrides
        // the default text. bash renders an array subscript exactly as written in
        // source (unexpanded) in the diagnostic name. The `${var?}` error aborts
        // the shell, so we fold the diagnostic into the captured stdout with a
        // group `{ …; } 2>&1` (the group's stderr redirect covers the abort path).
        let (o, _) = run("{ echo \"${y?}\"; } 2>&1");
        assert_eq!(o, "osh: y: parameter not set\n");
        let (o2, _) = run("x=; { echo \"${x:?}\"; } 2>&1");
        assert_eq!(o2, "osh: x: parameter null or not set\n");
        let (o3, _) = run("{ echo \"${z:?custom}\"; } 2>&1");
        assert_eq!(o3, "osh: z: custom\n");
        // Associative-array element: the key appears in the name.
        let (o4, _) = run("declare -A m; { echo \"${m[k]?}\"; } 2>&1");
        assert_eq!(o4, "osh: m[k]: parameter not set\n");
        // Indexed-array element: the subscript is rendered as written, not the
        // evaluated index (`a[$i]`, not `a[5]`).
        let (o5, _) = run("i=5; declare -a a; { echo \"${a[$i]?}\"; } 2>&1");
        assert_eq!(o5, "osh: a[$i]: parameter not set\n");
    }

    #[test]
    fn getopts_error_uses_dollar_zero_prefix() {
        // bash prefixes the getopts diagnostic with `$0` (the shell/script name),
        // not a `getopts:` command label. In the test harness `$0` is "osh".
        // `2>&1` routes the message into the stdout capture so we can inspect it.
        let (o, _) = run("set -- -x; getopts ab o 2>&1; echo done");
        assert_eq!(o, "osh: illegal option -- x\ndone\n");
        // Missing option-argument diagnostic uses the same `$0` prefix.
        let (o2, _) = run("set -- -a; getopts 'a:' o 2>&1; echo done");
        assert_eq!(o2, "osh: option requires an argument -- a\ndone\n");
    }

    #[test]
    fn builtin_diagnostics_honor_stderr_redirect() {
        // A builtin's own diagnostic must follow the command's fd 2 (bash), so
        // `2>&1` folds it into the stdout capture and it is silenced when it
        // would otherwise reach the shell's real stderr.
        // `cd` failure:
        let (o, _) = run("cd /no_such_dir_xyz123 2>&1 | sed 's/^/E:/'; echo done");
        assert!(o.contains("E:osh: cd: /no_such_dir_xyz123:"), "got: {o:?}");
        assert!(o.ends_with("done\n"));
        // `test`/`[` operand error:
        let (o2, _) = run("[ 5 -eq x ] 2>&1; echo done");
        assert_eq!(o2, "osh: [: x: integer expression expected\ndone\n");
        // The `builtin` wrapper's own "not a shell builtin" diagnostic:
        let (o3, _) = run("builtin no_such_builtin_xyz 2>&1; echo done");
        assert_eq!(o3, "osh: builtin: no_such_builtin_xyz: not a shell builtin\ndone\n");
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
        // `type g` now prints the reconstructed function body after the header.
        assert_eq!(run("g() { :; }; type g").0, "g is a function\ng () \n{ \n    :\n}\n");
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
    fn dbracket_match_always_uses_extglob() {
        // bash matches the RHS of `==`/`!=` in `[[ ]]` "as if extglob were
        // enabled", regardless of the shopt setting — unlike `case`/glob, which
        // gate on it. Extended patterns must match even with extglob OFF.
        assert_eq!(
            run("shopt -u extglob; [[ foo == +(f|o) ]] && echo y || echo n").0,
            "y\n"
        );
        assert_eq!(
            run("shopt -u extglob; [[ foo == @(foo|bar) ]] && echo y || echo n").0,
            "y\n"
        );
        // `!(...)` negation and `*.@(...)` alternation, still with extglob off.
        assert_eq!(
            run("shopt -u extglob; [[ hello == !(foo) ]] && echo y || echo n").0,
            "y\n"
        );
        assert_eq!(
            run("shopt -u extglob; [[ file.txt == *.@(txt|md) ]] && echo y || echo n").0,
            "y\n"
        );
        // The pattern really is a pattern, not a literal: `+(x)` doesn't match
        // the literal string "+(x)".
        assert_eq!(
            run("shopt -u extglob; [[ '+(x)' == +(x) ]] && echo y || echo n").0,
            "n\n"
        );
        // `!=` composes with the always-on extglob matching.
        assert_eq!(
            run("shopt -u extglob; [[ foo != +(f|o) ]] && echo y || echo n").0,
            "n\n"
        );
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
        // `~`/`~~` toggle case: `~` on the first char, `~~` on all matching.
        assert_eq!(run("x=hello; echo ${x~}").0, "Hello\n");
        assert_eq!(run("x=Hello; echo ${x~}").0, "hello\n");
        assert_eq!(run("x=aBcDeF; echo ${x~~}").0, "AbCdEf\n");
        assert_eq!(run("x=abcABC; echo ${x~~[a-c]}").0, "ABCABC\n");
        // Non-letters are left unchanged by a toggle.
        assert_eq!(run("x=123abc; echo ${x~~}").0, "123ABC\n");
        // Toggle over a whole array (per element).
        assert_eq!(run("a=(foo Bar BAZ); echo ${a[@]~~}").0, "FOO bAR baz\n");
    }

    #[test]
    fn param_indirect_expansion() {
        // `${!ref}` reads the variable named by `ref`.
        assert_eq!(run("x=hello; ref=x; echo ${!ref}").0, "hello\n");
        // Chained/renamed references.
        assert_eq!(run("a=b; b=c; c=done; echo ${!a} ${!b}").0, "c done\n");
        // A pointer that names an unset *target* yields empty (the target
        // `missing` is a valid name that is simply unset).
        assert_eq!(run("ref=missing; echo [${!ref}]").0, "[]\n");
        // An unset *pointer* itself is a fatal "invalid indirect expansion" in a
        // non-interactive shell (bash): the shell exits, so nothing is printed.
        let (o, s) = run("echo [${!nope}]; echo after");
        assert_eq!(o, "");
        assert_eq!(s, 1);
        // A pointer holding a malformed name is a fatal "invalid variable name".
        assert_eq!(run("ref='a-b'; echo [${!ref}]; echo after").0, "");
        // Referent naming an array element.
        assert_eq!(run("a=(x y z); ref='a[1]'; echo ${!ref}").0, "y\n");
        assert_eq!(
            run("declare -A m; m[k]=v; ref='m[k]'; echo ${!ref}").0,
            "v\n"
        );
        // Nameref special case: `${!ref}` yields the target NAME (not a second
        // indirection). `$ref` still yields the target value.
        assert_eq!(
            run("target=hi; declare -n ref=target; echo ${!ref} $ref").0,
            "target hi\n"
        );
        // Referent naming a whole array `a[@]`/`a[*]` expands like `${a[@]}`.
        assert_eq!(run("a=(1 2 3); ref='a[@]'; echo ${!ref}").0, "1 2 3\n");
        assert_eq!(run("a=(1 2 3); ref='a[*]'; echo ${!ref}").0, "1 2 3\n");
        // Quoted `"${!ref}"` preserves one field per element.
        assert_eq!(
            run(r#"a=("a b" c); ref='a[@]'; for x in "${!ref}"; do echo "<$x>"; done"#).0,
            "<a b>\n<c>\n"
        );
    }

    #[test]
    fn indirect_special_and_positional_referents() {
        // `${!#}` — indirect through `$#` (the count) selects the LAST
        // positional parameter (bash). With three args, that is the third.
        assert_eq!(run("set -- a b c; echo ${!#}").0, "c\n");
        assert_eq!(run("set -- x y z w; echo ${!#}").0, "w\n");
        // `${!N}` — indirect through a positional: the value of `$N` names the
        // variable to expand.
        assert_eq!(run("V=hi; set -- V; echo ${!1}").0, "hi\n");
        assert_eq!(run("b=BB; set -- a b c; echo ${!2}").0, "BB\n");
        // `${!?}` — indirect through `$?`. After a `true`, `$?` is 0, so this
        // resolves `${!0}` = `$0` (the shell name); just check it is non-empty
        // and does not error.
        assert_eq!(run("true; echo [${!?}]").1, 0);
        // `${!-}` — indirect through `$-` (option flags); those flag letters are
        // not a set variable, so it expands empty without error.
        assert_eq!(run("echo [${!-}]").0, "[]\n");
        // `$` and `!` are NOT valid indirect referents (bash: "bad
        // substitution"); osh rejects them at parse time.
        assert!(parse("echo ${!$}").is_err());
        assert!(parse("echo ${!!}").is_err());
    }

    #[test]
    fn indirect_expansion_with_modifier() {
        // `${!ref<op>}` resolves the target NAME via `ref`, then applies the
        // modifier to that variable (bash). Covers the use/default, case,
        // trim, substring, replace and transform modifiers.
        assert_eq!(run("foo=bar; x=foo; echo ${!x:-def}").0, "bar\n");
        assert_eq!(run("x=foo; echo ${!x:-def}").0, "def\n"); // foo unset
        assert_eq!(run("foo=bar; x=foo; echo ${!x^^}").0, "BAR\n");
        assert_eq!(run("foo=Hello; x=foo; echo ${!x,,}").0, "hello\n");
        assert_eq!(run("foo=barbaz; x=foo; echo ${!x#bar}").0, "baz\n");
        assert_eq!(run("foo=barbaz; x=foo; echo ${!x##*a}").0, "z\n");
        assert_eq!(run("foo=abcdef; x=foo; echo ${!x:2:3}").0, "cde\n");
        assert_eq!(run("foo=banana; x=foo; echo ${!x//a/X}").0, "bXnXnX\n");
        assert_eq!(run("foo=bar; x=foo; echo ${!x:+SET}").0, "SET\n");
        // `:=` assigns to the *resolved target*, not the pointer.
        assert_eq!(
            run("x=foo; echo ${!x:=assigned}; echo post=$foo").0,
            "assigned\npost=assigned\n"
        );
        // Unset pointer with a modifier is still a fatal invalid indirection.
        let (o, s) = run("echo ${!nope:-x}; echo after");
        assert_eq!((o.as_str(), s), ("", 1));
        // Round-trips through the unparser (`${!ref<op>}`).
        assert_eq!(
            crate::unparse::program_inline(&parse("echo ${!x:-def}").unwrap()).trim(),
            "echo ${!x:-def}"
        );
    }

    #[test]
    fn indirect_expansion_bad_pointer_is_fatal() {
        // An indirect expansion whose pointer is unset (or holds a malformed
        // name) is a fatal word-expansion error in a non-interactive shell
        // (bash): the shell exits with status 1 and the following command never
        // runs, in every expansion context.
        // Command word:
        let (o, s) = run("echo ${!nope}; echo after");
        assert_eq!((o.as_str(), s), ("", 1));
        // Bare assignment value:
        let (o, s) = run("x=${!nope}; echo after");
        assert_eq!((o.as_str(), s), ("", 1));
        // Temporary-prefix value (the command must NOT run):
        let (o, s) = run("x=${!nope} echo mid; echo after");
        assert_eq!((o.as_str(), s), ("", 1));
        // A valid-but-unset target is fine (empty, non-fatal):
        let (o, _) = run("p=missing; echo [${!p}]; echo after");
        assert_eq!(o, "[]\nafter\n");
    }

    #[test]
    fn nameref_to_array_element() {
        // A nameref may point at an array element: read and write route to it.
        assert_eq!(
            run("a=(x y z); declare -n ref=a[1]; echo $ref; ref=Y; echo \"${a[@]}\"").0,
            "y\nx Y z\n"
        );
        // Associative-array element target (string key).
        assert_eq!(
            run("declare -A m; m[k]=v; declare -n r=m[k]; echo $r; r=w; echo ${m[k]}").0,
            "v\nw\n"
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
        // Dynamically-computed special variables (values produced on demand in
        // param_value, not stored in `vars`) are still listed, like bash.
        assert_eq!(run("echo ${!RAND*}").0, "RANDOM\n");
        assert_eq!(run("echo ${!SEC*}").0, "SECONDS\n");
        assert_eq!(run("echo ${!LINE*}").0, "LINENO\n");
        assert_eq!(run("echo ${!EPOCH*}").0, "EPOCHREALTIME EPOCHSECONDS\n");
        assert_eq!(run("echo ${!BASHP*}").0, "BASHPID\n");
        // BASH_SOURCE is a call-stack array bash keeps present at every level;
        // osh lists it (and BASH_SUBSHELL) to match, sorted.
        assert_eq!(run("echo ${!BASH_S*}").0, "BASH_SOURCE BASH_SUBSHELL\n");
        // A user variable and a dynamic special sharing a prefix are merged and
        // sorted together.
        assert_eq!(run("SECRET=1; echo ${!SEC*}").0, "SECONDS SECRET\n");
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
    fn mapfile_callback_quantum() {
        // -C callback fires every -c quantum lines. bash appends the target
        // index and the line as command *arguments* (not $1/$2 of the caller).
        // Here a helper function receives them as $1/$2 and observes that the
        // element is not yet assigned (${arr[$1]} empty). With -t the line arg is
        // the stripped value. Quantum 1 fires per line.
        let src = "cb() { printf 'cb %s [%s] cur=[%s]\\n' \"$1\" \"$2\" \"${arr[$1]}\"; }\n\
                   mapfile -t -C cb -c 1 arr <<< $'p\\nq'\n\
                   printf 'final=%s\\n' \"${arr[*]}\"";
        assert_eq!(
            run(src).0,
            "cb 0 [p] cur=[]\ncb 1 [q] cur=[]\nfinal=p q\n"
        );
    }

    #[test]
    fn mapfile_callback_line_keeps_delim_without_t() {
        // Without -t the line argument to the callback keeps its delimiter.
        let src = "cb() { printf '[%s|%s]' \"$1\" \"$2\"; }\n\
                   mapfile -C cb -c 1 arr <<< $'x\\ny'";
        assert_eq!(run(src).0, "[0|x\n][1|y\n]");
    }

    #[test]
    fn mapfile_callback_quantum_two() {
        // Quantum 2 fires at the 2nd and 4th assigned lines, with the index of
        // the element about to be stored (1, then 3). `echo CB:` receives the
        // index and line as appended arguments.
        let src = "mapfile -t -C 'echo CB:' -c 2 arr <<< $'1\\n2\\n3\\n4\\n5'\n\
                   echo done";
        assert_eq!(run(src).0, "CB: 1 2\nCB: 3 4\ndone\n");
    }

    #[test]
    fn mapfile_no_callback_without_dash_c_uppercase() {
        // A large default quantum (5000) means no callback fires for a few lines
        // when -C is given without -c.
        let src = "mapfile -t -C 'echo NOPE' arr <<< $'a\\nb\\nc'\n\
                   echo \"${#arr[@]}\"";
        assert_eq!(run(src).0, "3\n");
    }

    #[test]
    fn mapfile_dash_n_still_counts() {
        // -n (count) is now distinct from -c (quantum): -n limits how many lines
        // are read.
        let src = "mapfile -t -n 2 arr <<< $'a\\nb\\nc\\nd'\n\
                   echo \"${#arr[@]}\"; echo \"${arr[*]}\"";
        assert_eq!(run(src).0, "2\na b\n");
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
        // -v can target an array element.
        assert_eq!(
            run("printf -v 'arr[2]' '%d' 99; echo \"${arr[2]}\"").0,
            "99\n"
        );
        // -v into an associative-array key.
        assert_eq!(
            run("declare -A m; printf -v 'm[k]' '%s!' hi; echo \"${m[k]}\"").0,
            "hi!\n"
        );
        // A readonly target is rejected and left intact.
        assert_eq!(
            run("readonly r=orig; printf -v r '%s' new 2>/dev/null; echo \"$r\"").0,
            "orig\n"
        );
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
    fn printf_invalid_number_diagnostics() {
        // A non-numeric arg warns to stderr and makes printf fail, but the
        // best-effort value (0, or the leading numeric prefix) is still emitted.
        assert_eq!(run("printf '%d\\n' abc"), ("0\n".to_string(), 1));
        // Leading digits are used as the value (strtoimax semantics).
        assert_eq!(run("printf '%d\\n' 12x"), ("12\n".to_string(), 1));
        // A leading `0` selects octal; `010` is 8, `08` is invalid → 0.
        assert_eq!(run("printf '%d\\n' 010"), ("8\n".to_string(), 0));
        assert_eq!(run("printf '%d\\n' 08"), ("0\n".to_string(), 1));
        // Bad hex digits are invalid.
        assert_eq!(run("printf '%d\\n' 0xzz"), ("0\n".to_string(), 1));
        // Valid numbers do not warn.
        assert_eq!(run("printf '%d\\n' 9"), ("9\n".to_string(), 0));
        assert_eq!(run("printf '%d\\n' ''"), ("0\n".to_string(), 0));
        // Floats: leading numeric prefix, invalid trailing junk.
        assert_eq!(run("printf '%.1f\\n' 3.5x"), ("3.5\n".to_string(), 1));
        assert_eq!(run("printf '%.1f\\n' xyz"), ("0.0\n".to_string(), 1));
    }

    #[test]
    fn printf_width_and_precision() {
        assert_eq!(run("printf '%5d' 42").0, "   42");
        assert_eq!(run("printf '%-5d|' 42").0, "42   |");
        assert_eq!(run("printf '%05d' 42").0, "00042");
        assert_eq!(run("printf '%.2s' abcd").0, "ab");
    }

    #[test]
    fn printf_integer_precision() {
        // For integer conversions, precision is the *minimum digit count*: the
        // body is zero-padded on the left (independent of field width). Verified
        // byte-for-byte against bash 5.x.
        assert_eq!(run("printf '[%.3d]' 7").0, "[007]");
        assert_eq!(run("printf '[%.5d]' 42").0, "[00042]");
        assert_eq!(run("printf '[%.3o]' 8").0, "[010]");
        assert_eq!(run("printf '[%.4x]' 255").0, "[00ff]");
        assert_eq!(run("printf '[%.4X]' 255").0, "[00FF]");
        assert_eq!(run("printf '[%.3u]' 7").0, "[007]");
        // A precision of 0 applied to the value 0 yields no digits at all.
        assert_eq!(run("printf '[%.0d]' 0").0, "[]");
        assert_eq!(run("printf '[%.0d]' 5").0, "[5]");
        assert_eq!(run("printf '[%.0x]' 0").0, "[]");
        // Precision disables the `0` flag → width is space-padded.
        assert_eq!(run("printf '[%08.3d]' 42").0, "[     042]");
        assert_eq!(run("printf '[%8.3d]' 42").0, "[     042]");
        assert_eq!(run("printf '[%-8.3d]' 42").0, "[042     ]");
        // Sign/space flags sit outside the zero-padded body.
        assert_eq!(run("printf '[%+.3d]' 42").0, "[+042]");
        assert_eq!(run("printf '[% .3d]' 42").0, "[ 042]");
        assert_eq!(run("printf '[%.3d]' -42").0, "[-042]");
        assert_eq!(run("printf '[%-8.3d]' -42").0, "[-042    ]");
        // `#` on octal forces a leading `0` after precision is applied.
        assert_eq!(run("printf '[%#.0o]' 0").0, "[0]");
        assert_eq!(run("printf '[%#.1o]' 8").0, "[010]");
        assert_eq!(run("printf '[%#.4x]' 255").0, "[0x00ff]");
        // Dynamic precision via `.*`.
        assert_eq!(run("printf '[%.*d]' 4 7").0, "[0007]");
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
        // bash's %q backslash-escapes shell-special chars (unlike @Q, which
        // single-quotes).
        assert_eq!(run("printf '%q' 'a b'").0, "a\\ b");
        assert_eq!(run("printf '%q' \"it's\"").0, "it\\'s");
        assert_eq!(run("printf '%q' ''").0, "''");
        assert_eq!(run("printf '%q' plain").0, "plain");
        assert_eq!(run("printf '%q' 'a$b`c'").0, "a\\$b\\`c");
        // @Q still uses single-quote style.
        assert_eq!(run("v='a b'; echo \"${v@Q}\"").0, "'a b'\n");
        assert_eq!(run("printf '%b' 'a\\tb'").0, "a\tb");
        assert_eq!(run("printf '%c' xyz").0, "x");
    }

    #[test]
    fn printf_format_string_escapes() {
        // The FORMAT string decodes octal, hex, and unicode escapes (not just
        // the named ones). Octal uses `\nnn` (a leading 0 is the first digit).
        assert_eq!(run("printf '\\x41'").0, "A");
        assert_eq!(run("printf '\\101'").0, "A");
        assert_eq!(run("printf '\\u0041'").0, "A");
        assert_eq!(run("printf '\\U00000041'").0, "A");
        assert_eq!(run("printf '\\e'").0, "\u{1b}");
        assert_eq!(run("printf '\\a'").0, "\u{07}");
        // `\0101` → `\010` (octal 010 = 0x08) then a literal `1`.
        assert_eq!(run("printf '\\0101'").0, "\u{08}1");
        assert_eq!(run("printf '\\07'").0, "\u{07}");
        assert_eq!(run("printf '\\0'").0, "\0");
        // Escapes and conversions interleave.
        assert_eq!(run("printf '%d\\n\\101' 5").0, "5\nA");
    }

    #[test]
    fn printf_b_conversion_escapes() {
        // `%b` uses `echo -e` octal rules: `\0nnn` (leading 0 is a prefix), so
        // `\0101` is the single character `A`.
        assert_eq!(run("printf '%b' '\\0101'").0, "A");
        // `\nnn` without the leading 0 also works in `%b`.
        assert_eq!(run("printf '%b' '\\101'").0, "A");
        assert_eq!(run("printf '%b' '\\x41'").0, "A");
        assert_eq!(run("printf '%b' '\\u0041'").0, "A");
        assert_eq!(run("printf '%b' '\\07'").0, "\u{07}");
        // `\c` stops all further output, including later literal text.
        assert_eq!(run("printf 'A%bC' '\\c stop'").0, "A");
        assert_eq!(run("printf '%b' 'x\\cy'").0, "x");
    }

    #[test]
    fn printf_sign_and_hash_flags() {
        // `+` forces a leading sign on non-negative numbers; negatives keep `-`.
        assert_eq!(run("printf '%+d %+d %+d' 5 -3 0").0, "+5 -3 +0");
        // The space flag reserves a sign column for non-negatives.
        assert_eq!(run("printf '% d|% d' 5 -3").0, " 5|-3");
        // Sign is placed before the zero padding, not swallowed by it.
        assert_eq!(run("printf '%+05d' 5").0, "+0005");
        assert_eq!(run("printf '%+05d' -5").0, "-0005");
        assert_eq!(run("printf '% 05d' 5").0, " 0005");
        // `+`/space apply to floats too.
        assert_eq!(run("printf '%+.2f' 3.5").0, "+3.50");
        assert_eq!(run("printf '% .2f' 3.5").0, " 3.50");
        // `#` adds base prefixes; the fill zeros go after the prefix.
        assert_eq!(run("printf '%#x' 255").0, "0xff");
        assert_eq!(run("printf '%#X' 255").0, "0XFF");
        assert_eq!(run("printf '%#06x' 255").0, "0x00ff");
        assert_eq!(run("printf '%#o' 8").0, "010");
        // `#` on zero produces no prefix (hex) / bare `0` (octal).
        assert_eq!(run("printf '%#x' 0").0, "0");
        assert_eq!(run("printf '%#o' 0").0, "0");
        // Left-justify keeps the sign attached to the number.
        assert_eq!(run("printf '%-+6d|' 5").0, "+5    |");
    }

    #[test]
    fn printf_float_conversion() {
        assert_eq!(run("printf '%.2f' 3.14159").0, "3.14");
        assert_eq!(run("printf '%f' 1").0, "1.000000");
    }

    #[test]
    fn printf_g_conversion() {
        // `%g` uses `prec` significant digits (default 6) and strips trailing
        // zeros; the exponent decides `%f`- vs `%e`-style.
        assert_eq!(run("printf '%.3g\\n' 3.14159").0, "3.14\n");
        assert_eq!(run("printf '%g\\n' 3.14159").0, "3.14159\n");
        assert_eq!(run("printf '%g\\n' 100000").0, "100000\n");
        assert_eq!(run("printf '%g\\n' 1000000").0, "1e+06\n");
        assert_eq!(run("printf '%g\\n' 0.0001").0, "0.0001\n");
        assert_eq!(run("printf '%g\\n' 0.00001").0, "1e-05\n");
        assert_eq!(run("printf '%.10g\\n' 3.14159").0, "3.14159\n");
        assert_eq!(run("printf '%G\\n' 0.00001").0, "1E-05\n");
        // `#` keeps trailing zeros.
        assert_eq!(run("printf '%#g\\n' 1.5").0, "1.50000\n");
        assert_eq!(run("printf '%g\\n' 0").0, "0\n");
    }

    #[test]
    fn printf_a_hex_float_conversion() {
        // `%a` renders IEEE-754 doubles in hexadecimal float form.
        assert_eq!(run("printf '%a\\n' 1.5").0, "0x1.8p+0\n");
        assert_eq!(run("printf '%A\\n' 1.5").0, "0X1.8P+0\n");
        assert_eq!(run("printf '%a\\n' 0").0, "0x0p+0\n");
        assert_eq!(run("printf '%a\\n' 2").0, "0x1p+1\n");
        assert_eq!(run("printf '%a\\n' -1.5").0, "-0x1.8p+0\n");
        assert_eq!(run("printf '%+a\\n' 1.5").0, "+0x1.8p+0\n");
        // Explicit precision rounds the fraction (round-half-to-even): 1.5's
        // `0x1.8` rounds to the even `0x2p+0`, not a renormalized `0x1p+1`.
        assert_eq!(run("printf '%.0a\\n' 1.5").0, "0x2p+0\n");
        // 0.1's 52-bit mantissa is exactly 13 hex digits.
        assert_eq!(run("printf '%a\\n' 0.1").0, "0x1.999999999999ap-4\n");
    }

    #[test]
    fn printf_thousands_flag_ignored() {
        // The `'` grouping flag is accepted; in the C locale it groups nothing.
        assert_eq!(run("printf \"%'d\\n\" 1234567").0, "1234567\n");
        assert_eq!(run("printf \"%'5d|\" 42").0, "   42|");
    }

    #[test]
    fn read_exact_n_no_ifs_split() {
        // `-N` reads exactly N characters and assigns them raw, without IFS
        // splitting or trimming (leading/trailing whitespace preserved).
        assert_eq!(run("read -N 3 x <<< 'ab cd'; echo \"[$x]\"").0, "[ab ]\n");
        assert_eq!(run("read -N 5 x <<< '  hi  there'; echo \"[$x]\"").0, "[  hi ]\n");
        // With several names the whole record goes to the first; the rest clear.
        assert_eq!(
            run("read -N 5 a b <<< 'x y z w'; echo \"[$a][$b]\"").0,
            "[x y z][]\n"
        );
        // An array target receives a single element holding the raw record.
        assert_eq!(
            run("read -N 5 -a arr <<< 'x y z w'; echo \"${#arr[@]}:[${arr[0]}]\"").0,
            "1:[x y z]\n"
        );
        // A custom IFS is likewise ignored under `-N`.
        assert_eq!(
            run("IFS=: read -N 3 x <<< 'a:b:c'; echo \"[$x]\"").0,
            "[a:b]\n"
        );
    }

    #[test]
    fn readonly_export_array_literal() {
        // `readonly arr=(1 2)` binds the array and applies the readonly attr,
        // formatting via `declare -p` as an indexed readonly array.
        assert_eq!(
            run("readonly arr=(1 2); echo \"${arr[1]}\"; declare -p arr").0,
            "2\ndeclare -ar arr=([0]=\"1\" [1]=\"2\")\n"
        );
        // `export arr=(1 2)` binds and marks the array exported (`-ax`).
        assert_eq!(
            run("export arr=(1 2); declare -p arr").0,
            "declare -ax arr=([0]=\"1\" [1]=\"2\")\n"
        );
        // `readonly -A m=([k]=v)` gives a readonly associative array.
        assert_eq!(
            run("readonly -A m=([k]=v); echo \"${m[k]}\"; declare -p m").0,
            "v\ndeclare -Ar m=([k]=\"v\" )\n"
        );
        // A scalar operand alongside an array literal is applied too.
        assert_eq!(
            run("readonly x=1 arr=(9 8); echo \"$x ${arr[0]}\"; declare -p x").0,
            "1 9\ndeclare -r x=\"1\"\n"
        );
    }

    #[test]
    fn hash_mid_word_is_literal() {
        // `#` only begins a comment at the *start* of a word; mid-word it is a
        // literal character (bash/POSIX), so `abc#def` is one word.
        assert_eq!(run("echo abc#def").0, "abc#def\n");
        assert_eq!(run("echo a#b#c").0, "a#b#c\n");
        assert_eq!(run("echo end#").0, "end#\n");
        // A `#` at word start (preceded by blanks) is still a comment.
        assert_eq!(run("echo abc #def").0, "abc\n");
        // The base-N arithmetic form survives as an assignment value.
        assert_eq!(run("n=16#ff; echo [$n]").0, "[16#ff]\n");
        assert_eq!(run("declare -i n=16#ff; echo $n").0, "255\n");
        // A whole-line comment still works.
        assert_eq!(run("echo a; #comment\necho b").0, "a\nb\n");
    }

    #[test]
    fn readonly_array_element_cannot_unset() {
        // An element of a readonly array cannot be unset (bash reports the base
        // name), and the array is left intact.
        let (out, status) = run("readonly arr=(1 2); unset arr[0]; echo $?; declare -p arr");
        assert_eq!(
            out,
            "1\ndeclare -ar arr=([0]=\"1\" [1]=\"2\")\n"
        );
        assert_eq!(status, 0); // last command (declare -p) succeeds
        // A readonly associative element is likewise protected.
        assert_eq!(
            run("readonly -A m=([k]=v); unset m[k]; declare -p m").0,
            "declare -Ar m=([k]=\"v\" )\n"
        );
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
    fn set_noexec_skips_execution() {
        // `set -n` (noexec) runs, then latches: every later command is parsed
        // but not executed. The `set -n` itself executes because noexec is off
        // when it is reached.
        assert_eq!(run("echo before; set -n; echo after; echo also").0, "before\n");
        // Assignments and expansions after noexec do not run either.
        assert_eq!(run("set -n; x=5; echo $x").0, "");
    }

    #[test]
    fn set_noexec_latches_and_ignores_plus_n() {
        // Under noexec, `set +n` cannot re-enable execution: it is itself
        // skipped, so the flag stays on (bash parity).
        assert_eq!(run("set -n; set +n; echo nope").0, "");
        // A `&&` continuation after `set -n` is also skipped.
        assert_eq!(run("set -n && echo x").0, "");
    }

    #[test]
    fn set_noexec_reported_in_options() {
        // `[[ -o noexec ]]` reflects the flag, and it appears in `set -o`.
        assert_eq!(run("[[ -o noexec ]] && echo on || echo off").0, "off\n");
        // `set -n` inside a condition still evaluates the condition itself
        // (noexec latches only for subsequent commands).
        assert_eq!(run("set -n; [[ -o noexec ]] && echo on").0, "");
        assert!(run("set -o").0.contains("noexec         \toff\n"));
    }

    // Trap-handler output is written through `run_source` (real stdout), not
    // the captured `out`, so these tests observe trap firing via a counter/flag
    // variable in `sh.vars` — the same convention as the existing DEBUG/RETURN
    // trap tests. (Whether a trap's *output* is captured by an enclosing command
    // substitution is a separate, subtler behaviour — see known-issues.)
    fn trap_var(src: &str, var: &str) -> Option<String> {
        let mut sh = Shell::new();
        sh.run_source(src);
        sh.vars.get(var).cloned()
    }

    #[test]
    fn return_trap_not_inherited_without_functrace() {
        // A RETURN trap set at the top level does NOT fire when a called
        // function returns (bash: functrace off is the default).
        assert_eq!(trap_var("trap 'RET=1' RETURN\nf(){ :; }\nf", "RET"), None);
    }

    #[test]
    fn return_trap_inherited_under_functrace() {
        // `set -T` (functrace) makes the RETURN trap fire on function return.
        assert_eq!(
            trap_var("set -T\ntrap 'RET=1' RETURN\nf(){ :; }\nf", "RET").as_deref(),
            Some("1")
        );
        // `set -o functrace` is the long spelling of the same option.
        assert_eq!(
            trap_var("set -o functrace\ntrap 'RET=1' RETURN\nf(){ :; }\nf", "RET").as_deref(),
            Some("1")
        );
    }

    #[test]
    fn return_trap_set_inside_function_fires_then_masked() {
        // A RETURN trap set inside a function fires once on that function's own
        // return. It then persists globally (bash: `trap -p` still shows it), but
        // a later *untraced* function does not inherit it, so it does not re-fire
        // on g's return. Net: IR is incremented exactly once.
        assert_eq!(
            trap_var(
                "f(){ trap 'IR=$((IR+1))' RETURN; :; }\nf\ng(){ :; }\ng",
                "IR"
            )
            .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn trap_set_inside_function_persists_globally() {
        // A DEBUG trap installed inside a function is not discarded on return
        // (bash): it stays in `self.traps` and fires for subsequent top-level
        // commands. `f` installs DEBUG; after `f` returns the two top-level
        // `:`/`echo` commands each fire it. `trap -p` output confirms persistence.
        let mut sh = Shell::new();
        sh.run_source("f(){ trap 'D=$((D+1))' DEBUG; }\nf\n:\n:");
        // Two top-level simple commands after the definition fire the persisted
        // DEBUG trap. (The `f` call itself fired it once before the body ran, but
        // the trap was not yet installed at that point, so it starts counting at
        // the first `:`.)
        assert_eq!(sh.vars.get("D").map(String::as_str), Some("2"));
    }

    #[test]
    fn debug_trap_not_inherited_without_functrace() {
        // Without functrace the DEBUG trap fires only before top-level commands
        // (the `f` call), not before the function-body commands. Here: the `f`
        // call fires once; the two body colons do not.
        assert_eq!(
            trap_var("trap 'D=$((D+1))' DEBUG\nf(){ :; :; }\nf", "D").as_deref(),
            Some("1")
        );
    }

    #[test]
    fn debug_trap_inherited_under_functrace() {
        // Under functrace the DEBUG trap also fires inside the function, plus an
        // extra entry firing on function entry. For `f(){ echo x; echo y; }; f`
        // that is: f-call, entry, echo x, echo y = 4 firings (matches bash).
        assert_eq!(
            trap_var(
                "set -T\ntrap 'D=$((D+1))' DEBUG\nf(){ echo x; echo y; }\nf",
                "D"
            )
            .as_deref(),
            Some("4")
        );
    }

    #[test]
    fn declare_ft_sets_and_clears_function_trace() {
        // `declare -ft NAME` makes just that function inherit RETURN traps even
        // with functrace off; other functions stay uninherited (R fires once).
        assert_eq!(
            trap_var(
                "trap 'R=$((R+1))' RETURN\nf(){ :; }\ng(){ :; }\ndeclare -ft f\nf\ng",
                "R"
            )
            .as_deref(),
            Some("1")
        );
        // `declare -f +t NAME` clears the trace attribute again (R never fires).
        assert_eq!(
            trap_var(
                "trap 'R=1' RETURN\nf(){ :; }\ndeclare -ft f\ndeclare -f +t f\nf",
                "R"
            ),
            None
        );
    }

    #[test]
    fn declare_plus_ft_does_not_clear_trace() {
        // bash only enters function mode from a minus-signed `-f`; a plus-signed
        // `+ft` does NOT touch the function's trace attribute, so RETURN still
        // fires after it.
        assert_eq!(
            trap_var(
                "trap 'R=1' RETURN\nf(){ :; }\ndeclare -ft f\ndeclare +ft f\nf",
                "R"
            )
            .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn debug_trap_fires_per_simple_pipeline_stage() {
        // bash fires DEBUG in the parent once per pipeline stage that is a
        // simple command: `true | true | true` → 3 firings.
        assert_eq!(
            trap_var("trap 'D=$((D+1))' DEBUG\ntrue | true | true", "D").as_deref(),
            Some("3")
        );
        // Group/compound stages do NOT fire it (bash), so a two-group pipeline
        // leaves the counter unset.
        assert_eq!(
            trap_var("trap 'D=$((D+1))' DEBUG\n{ true; } | { true; }", "D"),
            None
        );
        // A mixed pipeline fires only for the simple stage.
        assert_eq!(
            trap_var(
                "trap 'D=$((D+1))' DEBUG\ntrue | while read _l; do :; done",
                "D"
            )
            .as_deref(),
            Some("1")
        );
    }

    #[test]
    fn functrace_reported_in_options() {
        // `[[ -o functrace ]]`, `$-`, and `set -o` all reflect the flag.
        assert_eq!(run("[[ -o functrace ]] && echo on || echo off").0, "off\n");
        assert_eq!(run("set -T; [[ -o functrace ]] && echo on").0, "on\n");
        assert!(run("set -T; echo \"$-\"").0.contains('T'));
        assert!(run("set -o").0.contains("functrace      \toff\n"));
        // Enabled functrace is listed in SHELLOPTS.
        assert!(run("set -T; echo \"$SHELLOPTS\"").0.contains("functrace"));
    }

    #[test]
    fn errtrace_reported_in_options() {
        // `[[ -o errtrace ]]`, `$-` (letter `E`), `set -o`, and SHELLOPTS all
        // reflect the flag, both short (`-E`) and long (`-o errtrace`) spellings.
        assert_eq!(run("[[ -o errtrace ]] && echo on || echo off").0, "off\n");
        assert_eq!(run("set -E; [[ -o errtrace ]] && echo on").0, "on\n");
        assert_eq!(run("set -o errtrace; [[ -o errtrace ]] && echo on").0, "on\n");
        assert!(run("set -E; echo \"$-\"").0.contains('E'));
        // bash orders `E` before `T` in `$-` (…,C,E,H,P,T).
        let dollar_dash = run("set -ET; echo \"$-\"").0;
        let e = dollar_dash.find('E');
        let t = dollar_dash.find('T');
        assert!(e.is_some() && t.is_some() && e < t);
        assert!(run("set -o").0.contains("errtrace       \toff\n"));
        assert!(run("set -E; echo \"$SHELLOPTS\"").0.contains("errtrace"));
    }

    #[test]
    fn err_trap_not_inherited_without_errtrace() {
        // Without `errtrace`, a failing command inside a called function does not
        // fire the caller's ERR trap; only the function call itself failing at
        // the caller level does. So ERR fires exactly once.
        assert_eq!(
            trap_var("trap 'E=$((E+1))' ERR\nf(){ false; }\nf", "E").as_deref(),
            Some("1")
        );
    }

    #[test]
    fn err_trap_inherited_under_errtrace() {
        // Under `set -E`, the ERR trap is inherited into the function: `false`
        // inside fires it, and the function call failing at the top fires it
        // again — two firings total.
        assert_eq!(
            trap_var("set -E\ntrap 'E=$((E+1))' ERR\nf(){ false; }\nf", "E").as_deref(),
            Some("2")
        );
    }

    #[test]
    fn err_trap_installed_in_function_not_double_fired() {
        // When a function installs its OWN ERR trap and then fails, with no ERR
        // trap present at the caller when the call started, the call's own
        // non-zero return must not fire it again (bash): the trap did not exist
        // at the caller frame when the call began. So ERR fires exactly once.
        assert_eq!(
            trap_var("f(){ trap 'E=$((E+1))' ERR; false; }\nf", "E").as_deref(),
            Some("1")
        );
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
        // A nounset reference at the main shell aborts with 127 (bash), not 1.
        let (o, s) = run("set -u; echo $undefined; echo after");
        assert_eq!(o, "");
        assert_eq!(s, 127);
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
    fn funcname_reflects_call_stack() {
        // `$FUNCNAME` (element 0) is the current function.
        assert_eq!(run("f() { echo $FUNCNAME; }; f").0, "f\n");
        // The whole array is current-function … callers (no bottom `main`).
        assert_eq!(run("f() { echo \"${FUNCNAME[@]}\"; }; f").0, "f\n");
        assert_eq!(
            run("g() { echo \"${FUNCNAME[@]}\"; }; f() { g; }; f").0,
            "g f\n"
        );
        // Count is exactly the number of active call frames (no `main`).
        assert_eq!(run("f() { echo ${#FUNCNAME[@]}; }; f").0, "1\n");
        // Unset outside any function.
        assert_eq!(run("echo [${FUNCNAME[@]}]").0, "[]\n");
        // Restored after the function returns.
        assert_eq!(run("f() { :; }; f; echo [$FUNCNAME]").0, "[]\n");
    }

    #[test]
    fn funcname_script_mode_has_main_frame() {
        // In script-file mode (bash `osh SCRIPT`), the call-stack arrays gain a
        // bottom `main` pseudo-frame; `-c`/interactive (the plain harness) do not.
        let script_run = |src: &str| {
            let mut sh = Shell::new();
            sh.set_name("scr.sh");
            sh.set_script_mode();
            let mut buf = Vec::new();
            let mut out = Out::Capture(&mut buf);
            let prog = parse(src).expect("parse");
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
            String::from_utf8(buf).unwrap()
        };
        // Inside `f` at top level: FUNCNAME is `(f main)`, length 2.
        assert_eq!(
            script_run("f() { echo \"${#FUNCNAME[@]}:[${FUNCNAME[*]}]\"; }; f"),
            "2:[f main]\n"
        );
        // Nested: `g f main`, and BASH_LINENO ends in 0 (the main call line).
        assert_eq!(
            script_run("g() { echo \"${FUNCNAME[*]}|${BASH_LINENO[*]}\"; }\nf() { g; }\nf"),
            "g f main|2 3 0\n"
        );
        // At the *top level* of a script bash still exposes the base frame in
        // BASH_SOURCE/BASH_LINENO (the script itself), even though FUNCNAME is
        // empty there. The three arrays therefore differ in length.
        assert_eq!(
            script_run(
                "echo \"[${BASH_SOURCE[0]}] bl=${BASH_LINENO[0]} \
                 fn=${#FUNCNAME[@]} bs=${#BASH_SOURCE[@]}\""
            ),
            "[scr.sh] bl=0 fn=0 bs=1\n"
        );
        // BASH_SOURCE reports the script path for the frames of functions
        // defined in it, and `caller` walks up to the `main` base frame.
        assert_eq!(
            script_run("f() { echo \"src=${BASH_SOURCE[0]}\"; caller 0; }\nf"),
            "src=scr.sh\n2 main scr.sh\n"
        );
    }

    #[test]
    fn bash_lineno_and_source_arrays() {
        // The harness runs like stdin/REPL (neither `-c` nor a script file), so
        // bash labels function frames `main` in BASH_SOURCE.
        assert_eq!(run("f() { echo ${BASH_SOURCE[0]}; }; f").0, "main\n");
        // BASH_LINENO[0] is the line where the current function was called.
        assert_eq!(run("f() { echo ${BASH_LINENO[0]}; }\nf").0, "2\n");
        // Nested: BASH_LINENO tracks each call site. The harness runs like
        // `-c` (not a script file), so there is no bottom `main` frame.
        let src = "g() { echo \"${BASH_LINENO[@]}\"; }\nf() { g; }\nf";
        // g called on line 2 (inside f), f called on line 3.
        assert_eq!(run(src).0, "2 3\n");
        // Parallel arrays are all unset outside any function.
        assert_eq!(run("echo [${BASH_LINENO[@]}][${BASH_SOURCE[@]}]").0, "[][]\n");
    }

    #[test]
    fn caller_builtin() {
        // The harness runs like stdin/interactive: no bottom `main` frame, and
        // the source of a top-level caller is reported as the literal `NULL`.
        // Bare `caller` prints "LINE SOURCE" of the current call site; the
        // source is BASH_SOURCE[1] (the caller's frame), here `NULL`.
        assert_eq!(run("f() { caller; }\nf").0, "2 NULL\n");
        // `caller 0` from a single function needs FUNCNAME[1] (the caller),
        // which doesn't exist without a `main` frame → status 1, no output.
        let (o, c) = run("f() { caller 0; }\nf");
        assert_eq!(o, "");
        assert_eq!(c, 1);
        // Nested: `caller 0` from g reports g's call site + its caller f, whose
        // frame source is `main` in stdin/interactive mode.
        assert_eq!(run("g() { caller 0; }\nf() { g; }\nf").0, "2 f main\n");
        // Bare `caller` from g reports g's call line and f's source (`main`).
        assert_eq!(run("g() { caller; }\nf() { g; }\nf").0, "2 main\n");
        // `caller 1` needs FUNCNAME[2] (the `main` base frame), absent here.
        let (o, c) = run("g() { caller 1; }\nf() { g; }\nf");
        assert_eq!(o, "");
        assert_eq!(c, 1);
        // Out of range → status 1, no output.
        let (o, c) = run("f() { caller 5; }\nf");
        assert_eq!(o, "");
        assert_eq!(c, 1);
        // Outside any function → status 1.
        assert_eq!(run("caller").1, 1);
    }

    #[test]
    fn declare_in_function_is_local_by_default() {
        // Bash: `declare x=…` inside a function creates a *local*, so the global
        // is untouched after the function returns.
        let src = "x=outer; f() { declare x=inner; echo $x; }; f; echo $x";
        assert_eq!(run(src).0, "inner\nouter\n");
    }

    #[test]
    fn declare_g_forces_global_from_function() {
        // `declare -g` opts back out to global scope even inside a function.
        let src = "x=outer; f() { declare -g x=global; }; f; echo $x";
        assert_eq!(run(src).0, "global\n");
    }

    #[test]
    fn declare_g_array_forces_global_from_function() {
        // The array one-liner honors `-g` too.
        let src = "f() { declare -g -a a=(1 2 3); }; f; echo \"${a[@]}\"";
        assert_eq!(run(src).0, "1 2 3\n");
    }

    #[test]
    fn declare_array_in_function_is_local_by_default() {
        // Without `-g`, an array declaration inside a function is local.
        let src = "a=(g1 g2); f() { declare -a a=(l1 l2); echo \"${a[@]}\"; }; f; \
                   echo \"${a[@]}\"";
        assert_eq!(run(src).0, "l1 l2\ng1 g2\n");
    }

    #[test]
    fn declare_g_outside_function_is_plain_global() {
        // `-g` at global scope is a harmless no-op.
        assert_eq!(run("declare -g x=5; echo $x").0, "5\n");
    }

    #[test]
    fn local_integer_attr_does_not_leak() {
        // A `local -i` inside a function must not leave the integer attribute
        // set on the global after return: a later plain global assignment must
        // store the string verbatim, not evaluate it arithmetically.
        let src = "f() { local -i n; n=2+2; echo $n; }; f; n=3+4; echo $n";
        assert_eq!(run(src).0, "4\n3+4\n");
    }

    #[test]
    fn local_restores_shadowed_integer_attr() {
        // A bare `local g` does NOT inherit the global's `-i` attribute (bash
        // semantics), so `g=9+9` stores the string verbatim inside the
        // function. But the global's `-i` must be RESTORED on return, so the
        // later global `g=5+5` is evaluated arithmetically.
        let src = "declare -i g=1; f() { local g; g=9+9; echo $g; }; f; g=5+5; echo $g";
        assert_eq!(run(src).0, "9+9\n10\n");
    }

    #[test]
    fn local_nameref_does_not_leak() {
        // A `local -n` reference must not leave the nameref attribute set
        // globally after the function returns.
        let src = "target=orig; f() { local -n ref=target; ref=changed; }; f; \
                   echo $target; ref=plainvalue; echo $ref; echo $target";
        assert_eq!(run(src).0, "changed\nplainvalue\nchanged\n");
    }

    #[test]
    fn test_symlink_and_terminal_ops() {
        // -L/-h on a non-symlink is false; the operators parse without error in
        // both `[ ]` and `[[ ]]`.
        assert_eq!(run("[ -L . ]; echo $?").0, "1\n");
        assert_eq!(run("[[ -h . ]]; echo $?").0, "1\n");
        // -t on an invalid descriptor is false.
        assert_eq!(run("[ -t 99 ]; echo $?").0, "1\n");
        assert_eq!(run("[[ -t 99 ]]; echo $?").0, "1\n");
        // Negation composes with the new operators.
        assert_eq!(run("[ ! -L . ] && echo notlink").0, "notlink\n");
    }

    #[test]
    fn test_three_arg_negation_and_binary_precedence() {
        // Regression: `[ ! -f X ]` must negate the unary file test, not be
        // parsed as a bogus binary op. A missing file makes the negation true.
        assert_eq!(run("[ ! -f no_such_file_xyz ] && echo ok").0, "ok\n");
        assert_eq!(run("[ ! -d no_such_dir_xyz ] && echo ok").0, "ok\n");
        // A binary operator in the middle still wins over a leading `!`:
        // `[ ! = x ]` compares the strings "!" and "x" (not equal → false).
        assert_eq!(run("[ ! = x ]; echo $?").0, "1\n");
        // And a genuine equal comparison of "!" to itself is true.
        assert_eq!(run("[ ! = ! ]; echo $?").0, "0\n");
    }

    #[test]
    fn readonly_blocks_reassignment() {
        // Reassigning a readonly variable is fatal in a non-interactive shell
        // (bash): the shell exits with status 1 and the trailing `echo` never
        // runs.
        let (o, s) = run("readonly x=1; x=2; echo $x");
        assert_eq!(o, "");
        assert_eq!(s, 1);
        // The bare failing assignment alone also reports status 1.
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
        // Reassigning a `declare -r` readonly is fatal (bash): the shell exits
        // with status 1 before the `echo` runs.
        let (o, s) = run("declare -r y=const; y=other; echo $y");
        assert_eq!(o, "");
        assert_eq!(s, 1);
    }

    #[test]
    fn readonly_bare_name_then_assign_fails() {
        // `readonly x` marks an existing name; a later assignment is fatal in a
        // non-interactive shell (bash): the shell exits before the `echo`.
        let (o, s) = run("x=v; readonly x; x=w; echo $x");
        assert_eq!(o, "");
        assert_eq!(s, 1);
    }

    #[test]
    fn readonly_print_lists_vars() {
        // `readonly -p` reuses `declare -p` formatting (bash), not the old
        // `readonly name=value` form. Filter to the names under test so the
        // always-readonly BASH_VERSINFO line doesn't interfere.
        let (o, _) = run("readonly a=1; readonly b=2; readonly -p | grep ' [ab]='");
        assert_eq!(o, "declare -r a=\"1\"\ndeclare -r b=\"2\"\n");
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
        // bash prints a non-empty associative array with a trailing space
        // before the closing paren.
        assert_eq!(
            run("declare -A m; m[k]=v; declare -p m").0,
            "declare -A m=([k]=\"v\" )\n"
        );
        // An empty array (indexed or associative) prints as the bare name.
        assert_eq!(run("declare -a e; declare -p e").0, "declare -a e\n");
        assert_eq!(run("declare -A me; declare -p me").0, "declare -A me\n");
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
        // Long-form option names work too; the nounset abort exits 127 (bash).
        let (_, s) = run("set -o nounset; echo $undefined; echo after");
        assert_eq!(s, 127);
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
    fn brace_expansion_in_array_literal() {
        // Brace expansion runs on array-literal positional elements, just like
        // command words: `a=({1..3})` yields three elements, not one literal.
        assert_eq!(run("a=({1..3}); echo \"${a[@]}\" ${#a[@]}").0, "1 2 3 3\n");
        assert_eq!(run("a=(x{1..3}y); echo \"${a[@]}\"").0, "x1y x2y x3y\n");
        assert_eq!(run("a=(a{b,c}d); echo \"${a[@]}\"").0, "abd acd\n");
    }

    #[test]
    fn param_transform_quote_and_case() {
        // @Q quotes a value with a space; @U/@u/@L transform case.
        assert_eq!(run("x=\"a b\"; echo \"${x@Q}\"").0, "'a b'\n");
        assert_eq!(run("x=hello; echo \"${x@U}\"").0, "HELLO\n");
        assert_eq!(run("x=hello; echo \"${x@u}\"").0, "Hello\n");
        assert_eq!(run("x=HeLLo; echo \"${x@L}\"").0, "hello\n");
        // @l lowercases only the first character (mirror of @u).
        assert_eq!(run("x=HeLLo; echo \"${x@l}\"").0, "heLLo\n");
        assert_eq!(run("x=HELLO; echo \"${x@l}\"").0, "hELLO\n");
        // bash single-quotes every set value under @Q, even a plain word.
        assert_eq!(run("x=word; echo \"${x@Q}\"").0, "'word'\n");
        // An unset variable yields empty; a set-but-empty one yields `''`.
        assert_eq!(run("unset x; echo \"[${x@Q}]\"").0, "[]\n");
        assert_eq!(run("x=; echo \"[${x@Q}]\"").0, "['']\n");
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
    fn special_var_bash_version() {
        // BASH_VERSION is seeded (non-empty, 5.x compatibility level).
        assert_eq!(run("echo $BASH_VERSION").0, "5.2.0(1)-release\n");
        // BASH_VERSINFO is a 6-element array; [0] is the major version.
        assert_eq!(run("echo ${BASH_VERSINFO[0]}").0, "5\n");
        assert_eq!(run("echo ${#BASH_VERSINFO[@]}").0, "6\n");
        assert_eq!(run("echo ${BASH_VERSINFO[4]}").0, "release\n");
    }

    #[test]
    fn special_var_platform_identity() {
        // bash always defines these; we report SlateOS's own values.
        assert_eq!(run("echo $HOSTTYPE").0, "x86_64\n");
        assert_eq!(run("echo $OSTYPE").0, "slateos\n");
        assert_eq!(run("echo $MACHTYPE").0, "x86_64-slateos\n");
        // Ordinary shell variables: reassignable, unlike readonly BASH_VERSINFO.
        assert_eq!(run("OSTYPE=custom; echo $OSTYPE").0, "custom\n");
    }

    #[test]
    fn special_var_seconds_and_epoch() {
        assert_eq!(run("echo $SECONDS").0, "0\n");
        assert_eq!(run("SECONDS=100; echo $SECONDS").0, "100\n");
        assert_eq!(run("[ $EPOCHSECONDS -gt 1000000000 ] && echo ok").0, "ok\n");
    }

    #[test]
    fn special_var_lineno() {
        // $LINENO reflects the 1-based source line of the executing item.
        assert_eq!(run("echo $LINENO").0, "1\n");
        assert_eq!(run("echo $LINENO\necho $LINENO").0, "1\n2\n");
        // Blank and comment lines still advance the counter.
        assert_eq!(run("\n\necho $LINENO").0, "3\n");
        assert_eq!(run("# comment\necho $LINENO").0, "2\n");
        // Semicolon-separated commands on one line share a line number.
        assert_eq!(run("echo $LINENO; echo $LINENO").0, "1\n1\n");
    }

    #[test]
    fn special_var_underscore() {
        // `$_` is the last argument of the previous simple command.
        assert_eq!(run("echo hello world; echo $_").0, "hello world\nworld\n");
        assert_eq!(run("true a b c; echo $_").0, "c\n");
        // A single-word command leaves `$_` as that word (the command name).
        assert_eq!(run("echo solo; echo $_").0, "solo\nsolo\n");
        // Updates across commands.
        assert_eq!(run(": one; : two; echo $_").0, "two\n");
    }

    #[test]
    fn special_var_bash_command() {
        // $BASH_COMMAND holds the *unexpanded* source of the running command.
        assert_eq!(run("echo $BASH_COMMAND").0, "echo $BASH_COMMAND\n");
        // Redirections and prefix assignments are part of the reconstructed text.
        assert_eq!(run("x=1 echo $BASH_COMMAND").0, "x=1 echo $BASH_COMMAND\n");
        // An ERR trap sees the command that failed. (Trap stdout is not captured
        // by the harness, so capture BASH_COMMAND into a variable and read it
        // back after the trap has run.)
        assert_eq!(run("trap 'ERRCMD=$BASH_COMMAND' ERR\nfalse\necho \"$ERRCMD\"").0, "false\n");
    }

    #[test]
    fn special_var_dash_flags() {
        // $- reports enabled single-letter option flags; h and B are always on.
        assert_eq!(run("echo $-").0, "hB\n");
        // set -e adds 'e' in the fixed flag order (a e f h u x B C).
        assert_eq!(run("set -e; echo $-").0, "ehB\n");
        // Multiple flags appear in canonical order, not the order set.
        assert_eq!(run("set -xu; echo $-").0, "huxB\n");
        // Disabling drops the flag again.
        assert_eq!(run("set -e; set +e; echo $-").0, "hB\n");
    }

    #[test]
    fn special_var_dash_command_mode() {
        // A `-c` invocation appends `c` last (bash: `hBc`, `ehBc`, `hBCc`).
        let cmd_dash = |src: &str| {
            let mut sh = Shell::new();
            sh.set_command_mode();
            let mut buf = Vec::new();
            let mut out = Out::Capture(&mut buf);
            let prog = parse(src).expect("parse");
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
            String::from_utf8(buf).unwrap()
        };
        assert_eq!(cmd_dash("echo $-"), "hBc\n");
        assert_eq!(cmd_dash("set -eC; echo $-"), "ehBCc\n");
    }

    #[test]
    fn compgen_wordlist_and_prefix() {
        // `-W` splits on IFS; the trailing word prefix-filters the candidates,
        // preserving wordlist order (no sort).
        assert_eq!(run("compgen -W 'foo bar baz' ba").0, "bar\nbaz\n");
        // No trailing word: every candidate is offered.
        assert_eq!(run("compgen -W 'one two three'").0, "one\ntwo\nthree\n");
        // No match: no output, status 1.
        let (o, st) = run("compgen -W 'foo bar' xyz; echo \"st=$?\"");
        assert_eq!(o, "st=1\n");
        assert_eq!(st, 0);
    }

    #[test]
    fn compgen_actions_and_decorate() {
        // Function/builtin/variable actions draw from live shell state.
        assert_eq!(run("f1(){ :; }; f2(){ :; }; compgen -A function f2").0, "f2\n");
        assert_eq!(run("compgen -b tru").0, "true\n");
        // Variables come from a hash map (unordered); sort before asserting.
        let vout = run("xy=1; xyz=2; z=3; compgen -v xy").0;
        let mut vars: Vec<&str> = vout.lines().collect();
        vars.sort_unstable();
        assert_eq!(vars, vec!["xy", "xyz"]);
        // -P prefix / -S suffix wrap each match (applied after filtering).
        assert_eq!(run("compgen -P 'p_' -S '!' -W 'a ab' a").0, "p_a!\np_ab!\n");
    }

    #[test]
    fn compgen_filter_pattern() {
        // -X removes matches; a leading '!' keeps only matches.
        assert_eq!(
            run("compgen -W 'a.txt b.log c.txt' -X '*.log'").0,
            "a.txt\nc.txt\n"
        );
        assert_eq!(run("compgen -W 'a b c' -X '!b'").0, "b\n");
    }

    #[test]
    fn compgen_double_dash_ends_options() {
        // `--` terminates options; the following argument is the word to
        // complete — even when it begins with `-`. Regression: previously `--`
        // itself was consumed as the word, so nothing prefix-matched.
        assert_eq!(run("compgen -W 'apple apricot banana' -- ap").0, "apple\napricot\n");
        assert_eq!(run("compgen -W '-a -b -c' -- -a").0, "-a\n");
        // `--` with no following word offers every candidate.
        assert_eq!(run("compgen -W 'one two' --").0, "one\ntwo\n");
    }

    #[test]
    fn complete_register_and_print() {
        // A registered spec round-trips through `complete -p NAME` verbatim.
        assert_eq!(run("complete -W 'x y z' foo; complete -p foo").0, "complete -W 'x y z' foo\n");
        // `-F` function name is printed unquoted; other values are single-quoted.
        assert_eq!(
            run("complete -F _f -o bashdefault prog; complete -p prog").0,
            "complete -o bashdefault -F _f prog\n"
        );
        // Redefinition replaces the previous spec (keeps its slot).
        assert_eq!(run("complete -W a c; complete -W b c; complete -p c").0, "complete -W 'b' c\n");
        // A bare name stores an empty spec.
        assert_eq!(run("complete cmd; complete -p cmd").0, "complete cmd\n");
    }

    #[test]
    fn complete_canonical_option_order() {
        // Options are emitted in bash's fixed print order regardless of input
        // order: -o opts, shortcut actions, -A actions, then -G -W -P -S -X -C -F.
        let src = "complete -o nospace -o bashdefault -A function -S SUF -P PRE \
                   -X '*.x' -W 'w1 w2' -G '*.g' -C mycmd -F myf cmd; complete -p cmd";
        assert_eq!(
            run(src).0,
            "complete -o bashdefault -o nospace -A function -G '*.g' -W 'w1 w2' \
             -P 'PRE' -S 'SUF' -X '*.x' -C 'mycmd' -F myf cmd\n"
        );
        // Shortcut actions print before -A-only actions; each group in table order.
        assert_eq!(
            run("complete -A signal -v -a cmd; complete -p cmd").0,
            "complete -a -v -A signal cmd\n"
        );
        // Clustered shortcut flags expand in canonical order.
        assert_eq!(
            run("complete -abcdefgjksuv cmd; complete -p cmd").0,
            "complete -a -b -c -d -e -f -g -j -k -s -u -v cmd\n"
        );
    }

    #[test]
    fn complete_quotes_embedded_single_quotes() {
        // Values are single-quoted with the POSIX `'\''` escape for embedded quotes.
        assert_eq!(
            run("complete -W \"a 'b' c\" cmd; complete -p cmd").0,
            "complete -W 'a '\\''b'\\'' c' cmd\n"
        );
    }

    #[test]
    fn complete_remove_and_special_targets() {
        // `-r NAME` removes just that spec; `-r` with no args clears all.
        assert_eq!(run("complete -W a x; complete -r x; complete -p x; echo rc=$?").0, "rc=1\n");
        assert_eq!(run("complete -W a x; complete -W b y; complete -r; complete -p").0, "");
        // `-D`/`-E`/`-I` store the special default/empty/initial specs.
        assert_eq!(run("complete -F _d -D; complete -pD").0, "complete -F _d -D\n");
        // With -D present, command names are ignored.
        assert_eq!(run("complete -D -W x -F _f name; complete -p").0, "complete -W 'x' -F _f -D\n");
    }

    #[test]
    fn complete_error_status_codes() {
        // `-p` on an unknown spec is status 1 (message goes to stderr).
        assert_eq!(run("complete -p nope; echo rc=$?").0, "rc=1\n");
        // Invalid option / option name / action name are usage errors (status 2).
        assert_eq!(run("complete -Z cmd; echo rc=$?").0, "rc=2\n");
        assert_eq!(run("complete -o badopt cmd; echo rc=$?").0, "rc=2\n");
        assert_eq!(run("complete -A badaction cmd; echo rc=$?").0, "rc=2\n");
        // Defining options with no name/target is a usage error.
        assert_eq!(run("complete -o nospace; echo rc=$?").0, "rc=2\n");
        // A missing required argument is a usage error.
        assert_eq!(run("complete -F; echo rc=$?").0, "rc=2\n");
        // Bare `complete` and `complete -r nope` succeed.
        assert_eq!(run("complete; echo rc=$?").0, "rc=0\n");
        assert_eq!(run("complete -r nope; echo rc=$?").0, "rc=0\n");
    }

    #[test]
    fn compopt_modifies_options() {
        // `-o` adds an option, `+o` removes it, both preserving canonical order.
        assert_eq!(
            run("complete -W a cmd; compopt -o nospace -o plusdirs cmd; complete -p cmd").0,
            "complete -o nospace -o plusdirs -W 'a' cmd\n"
        );
        assert_eq!(
            run("complete -o nospace -o dirnames -W a cmd; compopt +o nospace cmd; complete -p cmd").0,
            "complete -o dirnames -W 'a' cmd\n"
        );
        // A nameless compopt (no completion function running) is status 1.
        assert_eq!(run("compopt -o nospace; echo rc=$?").0, "rc=1\n");
        // compopt on an unknown spec is status 1.
        assert_eq!(run("compopt -o nospace nope; echo rc=$?").0, "rc=1\n");
        // An invalid compopt option is a usage error (status 2).
        assert_eq!(run("compopt -Z; echo rc=$?").0, "rc=2\n");
    }

    #[test]
    fn complete_is_a_builtin() {
        // Both builtins are recognised by `type -t` and `compgen -b`.
        assert_eq!(run("type -t complete").0, "builtin\n");
        assert_eq!(run("type -t compopt").0, "builtin\n");
        assert_eq!(run("compgen -b complete").0, "complete\n");
    }

    #[test]
    fn shopt_full_inventory_listing() {
        // The bare listing reports every option in bash's table order with the
        // `NAME<pad-to-15>\ton|off` status form. Spot-check a few known entries.
        let out = run("shopt").0;
        assert!(out.contains("autocd         \toff\n"), "got: {out}");
        assert!(out.contains("interactive_comments\ton\n"), "got: {out}");
        assert!(out.contains("progcomp       \ton\n"), "got: {out}");
        // The test harness leaves neither command nor script mode set, so it
        // behaves interactively — `expand_aliases` defaults on.
        assert!(out.contains("expand_aliases \ton\n"), "got: {out}");
    }

    #[test]
    fn shopt_p_reinput_form() {
        // `-p` prints re-inputtable `shopt -s/-u NAME` lines.
        let out = run("shopt -p progcomp autocd").0;
        assert_eq!(out, "shopt -s progcomp\nshopt -u autocd\n");
    }

    #[test]
    fn shopt_s_filters_to_enabled() {
        // `shopt -s` with no names lists only currently-enabled options.
        let out = run("shopt -s").0;
        assert!(out.contains("progcomp       \ton\n"), "got: {out}");
        assert!(!out.contains("autocd"), "got: {out}");
    }

    #[test]
    fn bashopts_is_sorted_enabled_options() {
        // $BASHOPTS is a colon-joined, alphabetically-sorted list of enabled
        // shopt options; toggling one updates it.
        // The harness is interactive-like, so `expand_aliases` is enabled and
        // appears in the sorted list.
        assert_eq!(
            run("echo $BASHOPTS").0,
            "checkwinsize:cmdhist:complete_fullquote:expand_aliases:extquote:\
             force_fignore:globasciiranges:globskipdots:hostcomplete:\
             interactive_comments:patsub_replacement:progcomp:promptvars:sourcepath\n"
        );
        // Enabling `autocd` inserts it in sorted position.
        assert!(run("shopt -s autocd; echo $BASHOPTS").0.starts_with("autocd:checkwinsize:"));
    }

    #[test]
    fn bashopts_is_readonly() {
        // bash marks $BASHOPTS readonly; `readonly -p` renders it as such.
        assert!(run("readonly -p").0.contains("declare -r BASHOPTS="));
    }

    #[test]
    fn bash_var_is_reassignable() {
        // $BASH is defined and, unlike $BASHOPTS, is reassignable.
        assert!(!run("echo $BASH").0.trim().is_empty());
        assert_eq!(run("BASH=foo; echo $BASH").0, "foo\n");
    }

    #[test]
    fn lastpipe_runs_final_stage_in_current_shell() {
        // Without lastpipe the final stage is a subshell, so `read` cannot set a
        // variable in the parent (the value is lost).
        assert_eq!(run("echo hi | read x; echo \"${x:-empty}\"").0, "empty\n");
        // With `shopt -s lastpipe` the final stage runs in the current shell, so
        // `read` persists, a `while read` accumulator survives, and `mapfile`
        // populates a parent array.
        assert_eq!(
            run("shopt -s lastpipe; echo hi | read x; echo \"${x:-empty}\"").0,
            "hi\n"
        );
        assert_eq!(
            run("shopt -s lastpipe; seq 3 | while read l; do s=$((s+l)); done; echo sum=$s").0,
            "sum=6\n"
        );
        assert_eq!(
            run("shopt -s lastpipe; printf 'a\\nb\\nc\\n' | mapfile -t arr; echo ${arr[1]}").0,
            "b\n"
        );
    }

    #[test]
    fn builtin_help() {
        // `help NAME` prints a "NAME: usage" line then an indented description
        // (bash prefixes the synopsis with the builtin name and a colon).
        let out = run("help cd").0;
        assert!(out.contains("cd: cd [-L|-P] [dir]"), "got: {out:?}");
        assert!(out.contains("    Change the shell working directory."), "got: {out:?}");
        // `-s` prints only the "NAME: usage" line, no description.
        let out = run("help -s pwd").0;
        assert_eq!(out, "pwd: pwd [-L|-P]\n");
        // `-d` prints only the short description.
        assert_eq!(run("help -d true").0, "true - Return a successful (zero) exit status.\n");
        // A glob pattern matches multiple topics.
        let out = run("help -s 'tru*'").0;
        assert_eq!(out, "true: true\n");
        // No-arg lists every builtin synopsis (sorted); spot-check a couple.
        let out = run("help").0;
        assert!(out.contains("echo [-neE] [arg ...]"), "got: {out:?}");
        assert!(out.contains("help [-dms] [pattern ...]"), "got: {out:?}");
        // Unknown topic is a status-1 error with no stdout.
        let (o, code) = run("help nosuchbuiltin");
        assert_eq!(o, "");
        assert_eq!(code, 1);
    }

    #[test]
    fn tilde_expand_in_assignment() {
        // A bare assignment tilde-expands after each unquoted `:` (bash's
        // assignment-context rule), not just at the start of the value.
        assert_eq!(run("HOME=/h; x=~/a:~/b; echo \"$x\"").0, "/h/a:/h/b\n");
        // A tilde inside a preceding parameter expansion is preserved, while a
        // literal tilde following a literal `:` is expanded.
        assert_eq!(run("HOME=/h; p=/one; x=$p:~/bin; echo \"$x\"").0, "/one:/h/bin\n");
        // A quoted tilde is NOT expanded.
        assert_eq!(run("HOME=/h; x=~/a:'~/b'; echo \"$x\"").0, "/h/a:~/b\n");
        // ~+ / ~- work in assignment position too.
        assert_eq!(run("PWD=/p; x=~+/sub; echo \"$x\"").0, "/p/sub\n");
    }

    #[test]
    fn tilde_expand_in_declaration_builtin() {
        // export/declare/typeset/readonly operands are assignments: the RHS
        // tilde-expands after `:`/at value start, just like a bare NAME=value.
        assert_eq!(run("HOME=/h; export X=~/foo; echo \"$X\"").0, "/h/foo\n");
        assert_eq!(
            run("HOME=/h; declare Y=~/a:~/b; echo \"$Y\"").0,
            "/h/a:/h/b\n"
        );
        assert_eq!(
            run("HOME=/h; Z=/pre; typeset Z=$Z:~/bin; echo \"$Z\"").0,
            "/pre:/h/bin\n"
        );
        assert_eq!(run("HOME=/h; readonly R=~/r; echo \"$R\"").0, "/h/r\n");
        // A quoted tilde stays literal even for a declaration builtin.
        assert_eq!(run("HOME=/h; export Q=~/a:'~/b'; echo \"$Q\"").0, "/h/a:~/b\n");
        // The append form NAME+=value expands its RHS too, for declare, export,
        // and readonly alike (previously mis-parsed as a var named "NAME+").
        assert_eq!(
            run("HOME=/h; A=/pre; declare A+=:~/bin; echo \"$A\"").0,
            "/pre:/h/bin\n"
        );
        assert_eq!(
            run("HOME=/h; B=/pre; export B+=:~/bin; echo \"$B\"").0,
            "/pre:/h/bin\n"
        );
        assert_eq!(
            run("HOME=/h; C=/pre; readonly C+=:~/bin; echo \"$C\"").0,
            "/pre:/h/bin\n"
        );
        // Under -i, += performs numeric addition.
        assert_eq!(run("declare -i n=10; declare n+=5; echo \"$n\"").0, "15\n");
        // `local` inside a function gets assignment-context expansion.
        assert_eq!(
            run("HOME=/h; f() { local L=~/lib; echo \"$L\"; }; f").0,
            "/h/lib\n"
        );
        // The RHS is not word-split even when it contains spaces (assignment
        // context), unlike an ordinary command word.
        assert_eq!(
            run("HOME=/h; v='a b'; export S=$v; echo \"$S\"").0,
            "a b\n"
        );
    }

    #[test]
    fn tilde_expand_plus_minus_and_home() {
        // ~ expands to $HOME; ~/path keeps the remainder.
        assert_eq!(run("HOME=/home/me; echo ~").0, "/home/me\n");
        assert_eq!(run("HOME=/home/me; echo ~/docs").0, "/home/me/docs\n");
        // ~+ = $PWD, ~- = $OLDPWD, both with an optional trailing path.
        assert_eq!(run("PWD=/a/b; echo ~+").0, "/a/b\n");
        assert_eq!(run("PWD=/a/b; echo ~+/c").0, "/a/b/c\n");
        assert_eq!(run("OLDPWD=/x/y; echo ~-").0, "/x/y\n");
        assert_eq!(run("OLDPWD=/x/y; echo ~-/z").0, "/x/y/z\n");
        // ~+0 is the current directory-stack top (a real path, not literal).
        let out = run("echo ~+0").0;
        assert!(!out.starts_with('~'), "got: {out:?}");
        // An unresolvable ~user prefix is left untouched.
        assert_eq!(run("echo ~nosuchuser42/bin").0, "~nosuchuser42/bin\n");
    }

    #[test]
    fn param_transform_escape_and_attrs() {
        // @E expands backslash escapes; @a reports array attributes.
        assert_eq!(run("x='a\\tb'; printf '%s' \"${x@E}\"").0, "a\tb");
        assert_eq!(run("declare -A m; m[k]=v; echo \"${m@a}\"").0, "A\n");
        assert_eq!(run("a=(1 2 3); echo \"${a@a}\"").0, "a\n");
        // Scalar attributes: integer, readonly, export, lower/upper.
        assert_eq!(run("declare -i n=5; echo \"${n@a}\"").0, "i\n");
        assert_eq!(run("readonly r=1; echo \"${r@a}\"").0, "r\n");
        assert_eq!(run("export e=1; echo \"${e@a}\"").0, "x\n");
        assert_eq!(run("declare -l lo=X; echo \"${lo@a}\"").0, "l\n");
        // Combined attributes render in declare -p order (kind, n, i, l, u, r, x).
        assert_eq!(run("declare -ir z=5; echo \"${z@a}\"").0, "ir\n");
        // A plain scalar has no attribute flags.
        assert_eq!(run("p=1; echo \"[${p@a}]\"").0, "[]\n");
    }

    #[test]
    fn param_transform_prompt() {
        // @P expands prompt escapes using shell state.
        assert_eq!(
            run("USER=alice; HOSTNAME=box.example.com; p='\\u@\\h'; echo \"${p@P}\"").0,
            "alice@box\n"
        );
        // \H is the full hostname; \n/\\ and unknown escapes.
        assert_eq!(
            run("HOSTNAME=box.example.com; p='\\H'; echo \"${p@P}\"").0,
            "box.example.com\n"
        );
        // \w with HOME contraction is exercised indirectly; here test \s (shell
        // name from $0), octal, and a literal-preserving unknown escape.
        assert_eq!(run("p='\\061\\062'; echo \"${p@P}\"").0, "12\n"); // \061=1 \062=2
        assert_eq!(run("p='a\\qb'; echo \"${p@P}\"").0, "a\\qb\n"); // unknown escape kept
        // Defaults when USER/HOSTNAME unset.
        assert_eq!(run("unset USER LOGNAME; p='\\u'; echo \"${p@P}\"").0, "user\n");
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
    fn param_replace_preserves_literal_whitespace() {
        // The pattern and replacement of `${var/pat/repl}` are single words —
        // bash applies expansion and quote removal but neither word-splitting nor
        // operator tokenization, so leading/trailing/embedded whitespace is
        // literal (previously osh trimmed it via the word-splitting lexer).
        assert_eq!(run("s=world; echo \"[${s/#/hello }]\"").0, "[hello world]\n");
        assert_eq!(run("s=world; echo \"[${s/o/O }]\"").0, "[wO rld]\n");
        assert_eq!(run("s=world; echo \"[${s/w/ X}]\"").0, "[ Xorld]\n");
        // A literal space as the pattern.
        assert_eq!(run("s='a b c'; echo \"[${s/ /_}]\"").0, "[a_b c]\n");
        assert_eq!(run("s='a b c'; echo \"[${s// /_}]\"").0, "[a_b_c]\n");
        // Expansion inside the replacement still applies.
        assert_eq!(run("s=x; r='A B'; echo \"[${s/x/$r}]\"").0, "[A B]\n");
    }

    #[test]
    fn param_brace_closes_at_first_unescaped_brace() {
        // bash's `${…}` scanner closes at the first unquoted, unescaped `}`
        // that is not part of a nested `$…` construct. A bare `{` does NOT
        // open a new nesting level, so `${x//[{}]/_}` closes at the `}` inside
        // `[{}]` (leaving `]/_}` as literal text after the expansion). Pattern
        // is therefore `[{` which matches nothing in `a{b}c`.
        assert_eq!(run("x=a{b}c; echo \"${x//[{}]/_}\"").0, "a{b}c]/_}\n");
        // A backslash-escaped `}` inside the body is a literal, not a
        // terminator: pattern `\}` matches the literal `}`.
        assert_eq!(run("x=a}c; echo \"${x/\\}/X}\"").0, "aXc\n");
        // A backslash-escaped `{` is likewise literal.
        assert_eq!(run("x=a{b; echo \"${x/\\{/X}\"").0, "aXb\n");
        // The realistic JSON-stripping case: remove `{`, `}`, and `"`.
        assert_eq!(
            run("json='{\"a\":1}'; echo \"${json//[\\{\\}\\\"]/}\"").0,
            "a:1\n"
        );
    }

    #[test]
    fn param_brace_balances_nested_dollar_constructs() {
        // Nested `${…}`, `$(…)`, `$((…))`, and backtick spans inside a `${…}`
        // must balance with their own terminators, so a `}` or `)` within them
        // is not mistaken for the outer terminator.
        assert_eq!(run("unset x; y=hi; echo \"${x:-${y}}\"").0, "hi\n");
        assert_eq!(run("x=; echo \"${x:-$(echo })}\"").0, "}\n");
        assert_eq!(run("x=; echo \"${x:-`echo }`}\"").0, "}\n");
        assert_eq!(run("x=q; echo \"${x:-$((1+1))}\"").0, "q\n");
        // A `}` protected by quotes inside the body is also not a terminator.
        assert_eq!(run("x=; echo \"${x:-$(echo \"}\")}\"").0, "}\n");
    }

    #[test]
    fn param_ops_preserve_literal_whitespace() {
        // The pattern of `#`/`%` trims and `^`/`,` case ops, and the argument of
        // the `:-`/`:=`/`:+`/`:?` default ops, are single words with literal
        // whitespace — bash applies expansion and quote removal but not
        // word-splitting (previously osh trimmed embedded/leading/trailing
        // spaces via the word-splitting lexer, corrupting the pattern).
        // Trim with a trailing-space suffix pattern.
        assert_eq!(run("s='hello '; echo \"[${s% }]\"").0, "[hello]\n");
        // Longest-suffix trim with a space in the pattern: `${f%% *}` strips
        // from the first space onward. osh used to collapse ` *` → `*`.
        assert_eq!(run("f='my dog runs'; echo \"[${f%% *}]\"").0, "[my]\n");
        // Prefix trim where the pattern itself contains a space.
        assert_eq!(run("s='foo bar'; echo \"[${s#foo }]\"").0, "[bar]\n");
        // Default value preserves embedded and leading spaces when quoted.
        assert_eq!(run("echo \"[${x:-a  b}]\"").0, "[a  b]\n");
        assert_eq!(run("echo \"[${x:-  lead}]\"").0, "[  lead]\n");
        // Trailing space in an alternate value.
        assert_eq!(run("x=set; echo \"[${x:+hi }]\"").0, "[hi ]\n");
    }

    #[test]
    fn param_transform_assign() {
        // `@A` on a plain scalar → short `name='value'` (bash single-quotes
        // every value, even a plain word).
        assert_eq!(run("x=hello; echo \"${x@A}\"").0, "x='hello'\n");
        assert_eq!(run("x='a b'; echo \"${x@A}\"").0, "x='a b'\n");
        // An attributed scalar renders as a full `declare` statement whose
        // value is single-quoted (bash's `@A` uses single quotes even for the
        // attributed form, unlike `declare -p`'s double quotes).
        assert_eq!(
            run("declare -i n=5; echo \"${n@A}\"").0,
            "declare -i n='5'\n"
        );
        assert_eq!(
            run("declare -r x=5; echo \"${x@A}\"").0,
            "declare -r x='5'\n"
        );
        assert_eq!(
            run("declare -x x='a b'; echo \"${x@A}\"").0,
            "declare -x x='a b'\n"
        );
        // A bare array/assoc name is element 0 / key "0" (bash: `${a@A}` ==
        // `${a[0]@A}`), rendered as the array's declare flags with that single
        // element's value in scalar single-quoted form — NOT the whole array.
        assert_eq!(run("a=(x y); echo \"${a@A}\"").0, "declare -a a='x'\n");
        assert_eq!(run("a=(x y); echo \"${a[0]@A}\"").0, "declare -a a='x'\n");
        // A specific element likewise: `${a[1]@A}` uses element 1's value.
        assert_eq!(run("a=(x y); echo \"${a[1]@A}\"").0, "declare -a a='y'\n");
        // An unset element drops the value entirely (`declare -a a`).
        assert_eq!(run("a=(x y); echo \"${a[9]@A}\"").0, "declare -a a\n");
        // Associative: bare name is key "0"; unset there → no value.
        assert_eq!(run("declare -A m; m[k]=v; echo \"[${m@A}]\"").0, "[declare -A m]\n");
        assert_eq!(run("declare -A m; m[0]=z; echo \"${m@A}\"").0, "declare -A m='z'\n");
        // The WHOLE array/assoc form is `${a[@]@A}` / `${a[*]@A}` — a full
        // re-inputtable `declare` (see `array_transform_at_a_whole`).
        assert_eq!(
            run("a=(x y); echo \"${a[@]@A}\"").0,
            "declare -a a=([0]=\"x\" [1]=\"y\")\n"
        );
        // An unset variable yields the empty string.
        assert_eq!(run("echo \"[${nope@A}]\"").0, "[]\n");
    }

    #[test]
    fn array_transform_at_a_whole() {
        // `${arr[@]@A}` / `${arr[*]@A}` produce one field: the full re-inputtable
        // `declare` (matching `declare -p`), not a per-element transform.
        assert_eq!(
            run("declare -a a=(1 2 3); echo \"${a[@]@A}\"").0,
            "declare -a a=([0]=\"1\" [1]=\"2\" [2]=\"3\")\n"
        );
        assert_eq!(
            run("declare -a a=(1 2 3); echo \"${a[*]@A}\"").0,
            "declare -a a=([0]=\"1\" [1]=\"2\" [2]=\"3\")\n"
        );
        // A readonly array carries its flags through.
        assert_eq!(
            run("declare -ar a=(1 2); echo \"${a[@]@A}\"").0,
            "declare -ar a=([0]=\"1\" [1]=\"2\")\n"
        );
        // Positional params render as a single `set -- 'a' 'b' …` statement.
        assert_eq!(run("set -- a b c; echo \"${@@A}\"").0, "set -- 'a' 'b' 'c'\n");
        assert_eq!(run("set -- a b c; echo \"${*@A}\"").0, "set -- 'a' 'b' 'c'\n");
        // `@a` yields one attribute-letter field per element.
        assert_eq!(run("declare -a a=(1 2 3); echo \"${a[@]@a}\"").0, "a a a\n");
        assert_eq!(run("declare -ar a=(1 2); echo \"${a[@]@a}\"").0, "ar ar\n");
        assert_eq!(run("declare -a a=(1 2 3); echo \"${a[1]@a}\"").0, "a\n");
    }

    #[test]
    fn nameref_scalar_read_write() {
        // Reading a nameref returns the target's value.
        assert_eq!(run("target=hi; declare -n ref=target; echo $ref").0, "hi\n");
        // Writing through a nameref updates the target.
        assert_eq!(
            run("target=old; declare -n ref=target; ref=new; echo $target").0,
            "new\n"
        );
        // Retargeting: create the target lazily through the nameref.
        assert_eq!(
            run("declare -n ref=t; ref=made; echo $t").0,
            "made\n"
        );
    }

    #[test]
    fn nameref_array_access() {
        // A nameref to an array reads/writes its elements.
        assert_eq!(
            run("a=(x y z); declare -n r=a; echo ${r[1]}").0,
            "y\n"
        );
        assert_eq!(
            run("a=(x y z); declare -n r=a; echo \"${r[@]}\"").0,
            "x y z\n"
        );
        assert_eq!(
            run("a=(x y z); declare -n r=a; echo ${#r[@]}").0,
            "3\n"
        );
        // Writing an element through the nameref hits the target array.
        assert_eq!(
            run("a=(x y z); declare -n r=a; r[1]=Y; echo \"${a[@]}\"").0,
            "x Y z\n"
        );
    }

    #[test]
    fn nameref_in_function() {
        // The canonical "pass an array by reference to a function" pattern.
        let src = "fill() { declare -n out=$1; out=(1 2 3); }; \
                   fill data; echo \"${data[@]}\"";
        assert_eq!(run(src).0, "1 2 3\n");
    }

    #[test]
    fn nameref_unset_and_declare_p() {
        // `unset -n` drops the nameref; the target survives.
        assert_eq!(
            run("t=keep; declare -n r=t; unset -n r; echo \"[${r}][$t]\"").0,
            "[][keep]\n"
        );
        // Plain `unset` through a nameref removes the target.
        assert_eq!(
            run("t=gone; declare -n r=t; unset r; echo \"[$t]\"; echo done").0,
            "[]\ndone\n"
        );
        // `declare -p` shows the `-n` attribute.
        assert_eq!(
            run("t=v; declare -n r=t; declare -p r").0,
            "declare -n r=\"t\"\n"
        );
    }

    #[test]
    fn param_transform_keyvalue() {
        // `@k` interleaves subscripts and values as separate words.
        assert_eq!(run("a=(x y z); echo ${a[@]@k}").0, "0 x 1 y 2 z\n");
        // `@K` yields one field: subscripts with double-quoted values.
        assert_eq!(run("a=(x y); echo \"${a[@]@K}\"").0, "0 \"x\" 1 \"y\"\n");
        // Associative arrays interleave string keys.
        assert_eq!(
            run("declare -A m; m[a]=1; m[b]=2; echo ${m[@]@k}").0,
            "a 1 b 2\n"
        );
        // Positional parameters key from 1.
        assert_eq!(run("set -- p q; echo ${@@k}").0, "1 p 2 q\n");
        // `@k` keeps each word separate even when quoted.
        assert_eq!(
            run("a=('x 1' y); for w in \"${a[@]@k}\"; do echo \"[$w]\"; done").0,
            "[0]\n[x 1]\n[1]\n[y]\n"
        );
        // On a *scalar* (or single array element) `@K`/`@k` quote like `@Q`.
        assert_eq!(run("v=abc; echo ${v@K}").0, "'abc'\n");
        assert_eq!(run("v=abc; echo ${v@k}").0, "'abc'\n");
        assert_eq!(run("v='a b'; echo ${v@K}").0, "'a b'\n");
        assert_eq!(run("v=; echo ${v@K}").0, "''\n");
        assert_eq!(run("a=(x y z); echo ${a[1]@K}").0, "'y'\n");
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
    fn glob_posix_char_classes() {
        let g = |p: &str, t: &str| {
            glob_match(
                &p.chars().collect::<Vec<_>>(),
                &t.chars().collect::<Vec<_>>(),
                false,
            )
        };
        // Single POSIX classes match one appropriate character.
        assert!(g("[[:digit:]]", "5"));
        assert!(!g("[[:digit:]]", "a"));
        assert!(g("[[:alpha:]]", "a"));
        assert!(g("[[:space:]]", " "));
        assert!(g("[[:upper:]]", "A"));
        assert!(!g("[[:upper:]]", "a"));
        // Negated class: `[![:space:]]` = a non-space char.
        assert!(g("[![:space:]]", "x"));
        assert!(!g("[![:space:]]", " "));
        // Mixed with literals/ranges inside one bracket.
        assert!(g("[[:digit:]_]", "_"));
        assert!(g("[a-c[:digit:]]", "7"));
        // The classic left-trim idiom: strip everything from the first
        // non-space onward, leaving the leading whitespace.
        assert_eq!(param_trim("  trim  ", &"[![:space:]]*".chars().collect::<Vec<_>>(), true, true, false), "  ");
        // Shortest leading-whitespace `#` strip removes just one space char.
        assert_eq!(param_trim("  trim  ", &"[[:space:]]*".chars().collect::<Vec<_>>(), false, false, false), " trim  ");
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

    /// Run `src` on a shell that has imported the real process environment
    /// (as the binary does at startup). Reads process env — no mutation — so
    /// it is safe under the parallel test harness.
    fn run_imported(src: &str) -> (String, i32) {
        let mut sh = Shell::new();
        sh.import_environment();
        let mut buf = Vec::new();
        let prog = parse(src).expect("parse");
        {
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
        }
        (String::from_utf8_lossy(&buf).into_owned(), sh.last_status)
    }

    #[test]
    fn env_import_env_vars_are_shell_vars() {
        // After importing the environment, an inherited env variable behaves
        // exactly like a shell variable: it is readable, appears in prefix
        // name-matching (`${!PATH*}`), and `unset` truly hides it (no silent
        // std::env resurrection). PATH is present in every environment.
        assert_eq!(run_imported("echo \"${PATH:+yes}\"").0, "yes\n");
        // Prefix matching includes the inherited PATH.
        assert_eq!(
            run_imported("for n in ${!PATH*}; do [ \"$n\" = PATH ] && echo found; done").0,
            "found\n"
        );
        // unset removes it — no fallback to the real process environment.
        assert_eq!(run_imported("unset PATH; echo \"[${PATH-gone}]\"").0, "[gone]\n");
    }

    #[test]
    fn env_import_increments_shlvl() {
        // bash increments $SHLVL per nested shell. import_environment does the
        // same: the value is at least 1, exported, and a plain subshell keeps
        // the same level (it is not a new shell invocation).
        let (out, _) = run_imported("echo $SHLVL");
        let lvl: i64 = out.trim().parse().expect("SHLVL numeric");
        assert!(lvl >= 1, "SHLVL should be >= 1, got {lvl}");
        // A `(...)` subshell does not re-increment.
        let (out2, _) = run_imported("(echo $SHLVL)");
        assert_eq!(out2.trim(), lvl.to_string());
    }

    #[test]
    fn dollar_literal_before_closing_dquote() {
        // A `$` immediately before the closing `"` is a literal `$` (bash),
        // and inside double quotes `$'…'`/`$"…"` are NOT the ANSI-C-quote /
        // locale forms — the `$` is literal and the quote is handled by the
        // enclosing double-quote scanner. Previously osh's `read_dollar`
        // consumed the closing quote, giving "unterminated double quote".
        assert_eq!(run("echo \"abc$\"").0, "abc$\n");
        assert_eq!(run("echo \"^[0-9]+$\"").0, "^[0-9]+$\n");
        assert_eq!(run("echo \"$\"").0, "$\n");
        assert_eq!(run("x=5; echo \"val=$x$\"").0, "val=5$\n");
        // `$'x'` inside double quotes is the 4 literal chars `$'x'`.
        assert_eq!(run("echo \"a$'x'\"").0, "a$'x'\n");
    }

    #[test]
    fn legacy_dollar_bracket_arith() {
        // `$[ … ]` is bash's deprecated arithmetic expansion, an alias for
        // `$(( … ))`. Verify basic evaluation, spacing, variables, array
        // subscripts, and use inside a double-quoted context.
        assert_eq!(run("echo $[1+2]").0, "3\n");
        assert_eq!(run("echo $[ 2 * 3 ]").0, "6\n");
        assert_eq!(run("x=4; echo $[x*x]").0, "16\n");
        assert_eq!(run("a=(10 20 30); echo $[a[1]+5]").0, "25\n");
        assert_eq!(run("echo \"$[10/3]\"").0, "3\n");
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
    fn bash_subshell_depth() {
        // `$BASH_SUBSHELL` is 0 at top level and increments in each subshell.
        assert_eq!(run("echo $BASH_SUBSHELL").0, "0\n");
        assert_eq!(run("( echo $BASH_SUBSHELL )").0, "1\n");
        assert_eq!(run("( ( echo $BASH_SUBSHELL ) )").0, "2\n");
        // A command substitution is also a subshell.
        assert_eq!(run("echo $(echo $BASH_SUBSHELL)").0, "1\n");
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
        // Across separate `declare` commands the later flag wins (`-u` replaces
        // the earlier `-l`).
        assert_eq!(run("declare -l w; declare -u w; w=AbC; echo $w").0, "ABC\n");
        // But *within one cluster* two conflicting case flags cancel to none
        // (bash: `-ul`/`-lu`/`-cl`… store the value unchanged, no attribute).
        assert_eq!(run("declare -ul v=AbC; echo $v").0, "AbC\n");
        assert_eq!(run("declare -ul v=AbC; declare -p v").0, "declare -- v=\"AbC\"\n");
        // `+u` removes the attribute.
        assert_eq!(run("declare -u q=abc; declare +u q; q=def; echo $q").0, "def\n");
        // Array elements are folded too.
        assert_eq!(run("declare -u arr; arr[0]=xy; echo ${arr[0]}").0, "XY\n");
        // `declare -p` reflects the case attribute.
        assert_eq!(run("declare -l s=Hi; declare -p s").0, "declare -l s=\"hi\"\n");
    }

    #[test]
    fn declare_capcase_attribute() {
        // `declare -c` (bash's att_capcase): uppercase the first character and
        // lowercase the rest. Verified byte-for-byte against bash 5.x.
        assert_eq!(run("declare -c x=hELLO; echo $x").0, "Hello\n");
        assert_eq!(run("declare -c x='hello world'; echo \"$x\"").0, "Hello world\n");
        assert_eq!(run("declare -c x=HELLO; echo $x").0, "Hello\n");
        assert_eq!(run("declare -c x=a; echo $x").0, "A\n");
        // A leading non-letter blocks capitalization; the rest is lowercased.
        assert_eq!(run("declare -c x=123abc; echo $x").0, "123abc\n");
        // The attribute persists across reassignment.
        assert_eq!(run("declare -c x=foo; x=bAR; echo $x").0, "Bar\n");
        // Array elements are capitalized too.
        assert_eq!(run("declare -c a; a=(oNE tWO); echo \"${a[@]}\"").0, "One Two\n");
        // `typeset -c` and `local -c` behave identically.
        assert_eq!(run("typeset -c x=wORLD; echo $x").0, "World\n");
        assert_eq!(run("f() { local -c y=hELLO; echo \"$y\"; }; f").0, "Hello\n");
        // A function-local `-c` does not leak to the caller's variable.
        assert_eq!(run("f() { local -c y=hi; }; y=OUTER; f; echo $y").0, "OUTER\n");
        // `declare -p` shows `-c`, ordered after i/r/x (`-rc`, `-xc`, `-ic`).
        assert_eq!(run("declare -c x=foo; declare -p x").0, "declare -c x=\"Foo\"\n");
        assert_eq!(run("declare -cr x=foo; declare -p x").0, "declare -rc x=\"Foo\"\n");
        // `${x@a}` reports the `c` flag (after i/r/x).
        assert_eq!(run("declare -c x=Ab; echo ${x@a}").0, "c\n");
        assert_eq!(run("declare -cx x=Ab; echo ${x@a}").0, "xc\n");
        // `+c` removes the attribute.
        assert_eq!(run("declare -c x=foo; declare +c x; x=bAR; echo $x").0, "bAR\n");
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
    fn globignore_pattern_matching() {
        // Pathname-style: `*` does not cross `/`.
        let pats = build_globignore("*.log", false);
        assert_eq!(pats.len(), 1);
        assert!(pats[0].matches_path("c.log"));
        assert!(!pats[0].matches_path("sub/e.log"));
        assert!(!pats[0].matches_path("a.txt"));

        // A `/`-bearing pattern matches component-for-component.
        let pats = build_globignore("sub/*.log", false);
        assert!(pats[0].matches_path("sub/e.log"));
        assert!(!pats[0].matches_path("e.log"));
        assert!(!pats[0].matches_path("sub/d.txt"));

        // `:` separates multiple patterns; empty entries are skipped.
        let pats = build_globignore("*.log::*.txt:", false);
        assert_eq!(pats.len(), 2);
        assert!(pats.iter().any(|p| p.matches_path("c.log")));
        assert!(pats.iter().any(|p| p.matches_path("a.txt")));
        assert!(!pats.iter().any(|p| p.matches_path("keep.md")));
    }

    #[test]
    fn globignore_filesystem_filtering() {
        let _cwd = cwd_guard();
        let uniq = format!(
            "osh_globignore_{}_{}",
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

        // GLOBIGNORE=*.log active: drops c.log, and the dotglob effect surfaces
        // .hidden (which `*` would normally skip).
        let gi = build_globignore(&format!("{uniq}/*.log"), false);
        let mut out = Vec::new();
        let mut failed = None;
        glob_or_literal(
            &field_lit(&format!("{uniq}/*")),
            &mut out,
            false,
            false,
            false,
            false,
            false,
            false,
            true,
            &gi,
            &mut failed,
        );
        let mut names: Vec<String> = out.iter().map(|p| basename(p)).collect();
        names.sort();
        assert_eq!(
            names,
            vec![".hidden".to_string(), "a.txt".to_string(), "b.txt".to_string()]
        );

        // With GLOBIGNORE active, a `.*` pattern still excludes `.` and `..`.
        let mut out = Vec::new();
        let mut failed = None;
        glob_or_literal(
            &field_lit(&format!("{uniq}/.*")),
            &mut out,
            false,
            false,
            false,
            false,
            false,
            false,
            true,
            &[],
            &mut failed,
        );
        let names: Vec<String> = out.iter().map(|p| basename(p)).collect();
        assert!(names.iter().all(|n| n != "." && n != ".."));
        assert!(names.contains(&".hidden".to_string()));

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
    fn shopt_failglob_aborts_on_no_match() {
        // With `failglob`, an unmatched command-word glob is a fatal expansion
        // error: the command does not run, `$?` is 1, and (as in a single
        // non-interactive `-c` list) a following command is discarded.
        let (out, st) = run("shopt -s failglob; echo osh_no_such_glob_*.zzz; echo after");
        assert_eq!(out, "");
        assert_eq!(st, 1);
        // A non-glob word is unaffected by failglob.
        assert_eq!(run("shopt -s failglob; echo hello").0, "hello\n");
        // failglob also aborts an unmatched glob in an array-literal value.
        let (aout, ast) = run("shopt -s failglob; a=(osh_no_such_*.zzz); echo after");
        assert_eq!(aout, "");
        assert_eq!(ast, 1);
        // A stale marker from an aborted command must not misfire on the next.
        assert_eq!(run("shopt -s failglob; echo osh_no_*.zzz\necho ok").1, 1);
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
    fn shopt_o_queries_set_o_options() {
        // `shopt -o NAME` operates on `set -o` options, not shopt options.
        // Off by default → status 1, and the listing uses `%-15s\t%s`.
        assert_eq!(
            run("shopt -o noclobber; echo $?").0,
            "noclobber      \toff\n1\n"
        );
        // Enabling via `set -o` is reflected; `-q` suppresses output.
        assert_eq!(
            run("set -o noclobber; shopt -qo noclobber; echo $?").0,
            "0\n"
        );
        // `shopt -so NAME` enables the option (like `set -o NAME`).
        assert_eq!(
            run("shopt -so noclobber; [[ -o noclobber ]] && echo on").0,
            "on\n"
        );
        // A truly unknown option name is a status-1 error.
        assert_eq!(run("shopt -o bogus_xyz 2>/dev/null; echo $?").0, "1\n");
    }

    #[test]
    fn shopt_listing_is_padded() {
        // The plain `shopt` listing pads the name to 15 then a TAB (bash).
        assert_eq!(run("shopt nullglob").0, "nullglob       \toff\n");
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
    fn cd_uses_cdpath_and_echoes() {
        // Mutates the process-global cwd; serialize with the other cwd tests.
        let _cwd = cwd_guard();
        let orig = std::env::current_dir().expect("cwd");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let uniq = format!("osh_cdpath_{}_{}", std::process::id(), nanos);
        let tmp = std::env::temp_dir();
        let base = tmp.join(&uniq);
        let sub = base.join("proj");
        std::fs::create_dir_all(&sub).expect("mkdir");
        let ptmp = tmp.to_string_lossy().replace('\\', "/");

        // `CDPATH` is a colon-separated list; on the Windows host we use a
        // *relative* entry (the unique dir name) so the drive-letter `:` in an
        // absolute path does not collide with the list separator. First cd into
        // the temp dir (an explicit absolute path, so CDPATH is not consulted),
        // then `cd proj` resolves through CDPATH=<uniq> to <uniq>/proj.
        let (o, st) = run(&format!("cd {ptmp}\nCDPATH={uniq}\ncd proj\npwd"));
        std::env::set_current_dir(&orig).expect("restore cwd");

        assert_eq!(st, 0, "cd via CDPATH should succeed; output {o:?}");
        // `pwd` (captured) confirms the relative name resolved under CDPATH.
        // (The `cd` destination echo itself goes to the real stdout, matching
        // the existing `cd -` behavior, so it is not in the captured buffer.)
        assert!(o.contains(&uniq), "expected cwd under {uniq}, got {o:?}");
        assert!(o.contains("proj"), "expected to land in proj, got {o:?}");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn cd_physical_flag_changes_directory() {
        let _cwd = cwd_guard();
        let orig = std::env::current_dir().expect("cwd");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let uniq = format!("osh_cdp_{}_{}", std::process::id(), nanos);
        let dir = std::env::temp_dir().join(&uniq);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let pdir = dir.to_string_lossy().replace('\\', "/");

        // `cd -P dir` accepts the flag and changes directory (canonical PWD).
        let (o, st) = run(&format!("cd -P {pdir}\npwd"));
        std::env::set_current_dir(&orig).expect("restore cwd");

        assert_eq!(st, 0, "cd -P should succeed; output {o:?}");
        assert!(o.contains(&uniq), "expected cwd under {uniq}, got {o:?}");

        std::fs::remove_dir_all(&dir).ok();
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
        // `trap -p` renders real signals with bash's `SIG` prefix.
        let (o, _) = run("trap 'x' sigint; trap -p int");
        assert!(o.contains("trap -- 'x' SIGINT"), "got {o:?}");

        // `trap - SIG` resets (removes) the handler.
        let (o, _) = run("trap 'x' INT; trap - INT; trap -p");
        assert!(!o.contains("INT"), "reset should remove INT, got {o:?}");

        // Ignore form ('') round-trips (real signal → `SIG`-prefixed display).
        let (o, _) = run("trap '' TERM; trap -p TERM");
        assert!(o.contains("trap -- '' SIGTERM"), "got {o:?}");

        // An invalid spec is a status-1 error.
        let (_, st) = run("trap 'x' NOPE");
        assert_eq!(st, 1);
    }

    #[test]
    fn trap_print_sig_prefix_and_order() {
        // bash's `trap -p` prints real signals with a `SIG` prefix but the
        // pseudo-signals (EXIT/ERR/DEBUG/RETURN) bare, ordered EXIT, then real
        // signals by number, then DEBUG, ERR, RETURN.
        let (o, _) = run(
            "trap 'e' EXIT; trap 'h' HUP; trap 'i' INT; \
             trap 'd' DEBUG; trap 'r' RETURN; trap 'x' ERR; trap -p",
        );
        // The DEBUG trap fires before each command, emitting stray `d` lines;
        // keep only the `trap --` listing lines to check prefix and ordering.
        let lines: Vec<&str> = o.lines().filter(|l| l.starts_with("trap --")).collect();
        assert_eq!(
            lines,
            vec![
                "trap -- 'e' EXIT",
                "trap -- 'h' SIGHUP",
                "trap -- 'i' SIGINT",
                "trap -- 'd' DEBUG",
                "trap -- 'x' ERR",
                "trap -- 'r' RETURN",
            ],
            "got {o:?}"
        );
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
    fn subshell_exit_trap_fires() {
        // A subshell that installs its own EXIT trap fires it when the subshell
        // exits — for `( … )`, a command substitution, and a pipeline stage —
        // while the parent's inherited EXIT trap is *not* re-fired in a subshell.
        // Output is folded into stdout with `2>&1`-free echoes so `run` captures.
        let (o, _) = run("( trap 'echo sub' EXIT; echo insub ); echo out");
        assert_eq!(o, "insub\nsub\nout\n");

        // An explicit `exit N` inside the subshell still fires the trap, and the
        // subshell's status ($?) is the exit code, not the trap's.
        let (o2, _) = run("( trap 'echo sub' EXIT; exit 3 ); echo \"out=$?\"");
        assert_eq!(o2, "sub\nout=3\n");

        // The parent EXIT trap is reset (not fired) inside a subshell that does
        // not set its own — only the parent's fires, once, at real exit.
        let mut sh = Shell::new();
        let mut buf = Vec::new();
        {
            let mut out = Out::Capture(&mut buf);
            let prog = parse("trap 'echo P' EXIT; ( echo in ); echo out").expect("parse");
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
        }
        assert_eq!(String::from_utf8_lossy(&buf), "in\nout\n");
        // The parent's EXIT trap only fires now, at the real shell exit.
        let mut buf2 = Vec::new();
        {
            let mut out = Out::Capture(&mut buf2);
            sh.run_exit_trap_out(&mut out, &StdinSrc::Inherit);
        }
        assert_eq!(String::from_utf8_lossy(&buf2), "P\n");

        // A command substitution captures its own EXIT-trap output in the result.
        let (o3, _) = run("x=$( trap 'echo t' EXIT; echo body ); echo \"[$x]\"");
        assert_eq!(o3, "[body\nt]\n");

        // A pipeline-stage subshell fires its own EXIT trap.
        let (o4, _) = run("false | ( trap 'echo PS' EXIT; cat >/dev/null ); echo done");
        assert_eq!(o4, "PS\ndone\n");
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
        // A RETURN trap fires on function return only when the trap is inherited
        // by the function — i.e. under `functrace` (`set -T`), the function's
        // trace attribute, or when the trap is installed inside the function.
        // Without any of those it does NOT fire (bash's default); see the
        // `return_trap_*` tests for the full matrix.
        let mut sh = Shell::new();
        sh.run_source("set -T\ntrap 'RET=1' RETURN\nf() { :; }\nf");
        assert_eq!(sh.vars.get("RET").map(String::as_str), Some("1"));
    }

    #[test]
    fn return_outside_function_is_error_and_continues() {
        // Top-level `return` is an error (status 2) that does NOT unwind: the
        // following command still runs. (bash: same behaviour.)
        let (o, _) = run("echo before; return; echo after");
        assert_eq!(o, "before\nafter\n");
        // The error message and status-2 are observable via a group's 2>&1.
        let (o2, _) = run("{ return; } 2>&1; echo \"s=$?\"");
        assert_eq!(
            o2,
            "osh: return: can only `return' from a function or sourced script\ns=2\n"
        );
        // Inside a function, `return` unwinds normally with its status.
        assert_eq!(run("f() { echo in; return 7; echo out; }; f; echo $?").0, "in\n7\n");
        // A `return` in a subshell inside a function exits just the subshell.
        assert_eq!(
            run("f() { ( echo a; return 5; echo b ); echo \"s=$?\"; }; f").0,
            "a\ns=5\n"
        );
        // A `return` in a top-level subshell is still an error (no unwind).
        let (o3, _) = run("( echo a; return 3; echo b ) 2>/dev/null; echo done");
        assert_eq!(o3, "a\nb\ndone\n");
    }

    #[test]
    fn break_continue_outside_loop_warn_and_continue() {
        // Outside a loop, `break` warns to stderr, returns status 0, and does
        // NOT unwind — the following command still runs. (bash: same.)
        let (o, _) = run("echo before; break; echo after");
        assert_eq!(o, "before\nafter\n");
        let (o2, _) = run("{ break; } 2>&1; echo \"s=$?\"");
        assert_eq!(
            o2,
            "osh: break: only meaningful in a `for', `while', or `until' loop\ns=0\n"
        );
        let (o3, _) = run("{ continue; } 2>&1; echo \"s=$?\"");
        assert_eq!(
            o3,
            "osh: continue: only meaningful in a `for', `while', or `until' loop\ns=0\n"
        );
        // Inside a loop, `break` still works normally.
        assert_eq!(run("for i in 1 2 3; do echo $i; break; done; echo done").0, "1\ndone\n");
        // A `break` inside a function called from a loop must NOT break the
        // enclosing loop: bash resets loop nesting on function entry, so the
        // break is an error inside the function and the loop keeps iterating.
        let (o4, _) = run("f() { break; }; for i in 1 2 3; do echo $i; f; done 2>/dev/null");
        assert_eq!(o4, "1\n2\n3\n");
        // A `break` in a subshell inside a loop likewise does not break the
        // loop (bash resets loop nesting in the subshell).
        let (o5, _) = run("for i in 1 2; do echo $i; ( break ); done 2>/dev/null");
        assert_eq!(o5, "1\n2\n");
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
        // The use/alternate/error operators on `[@]`/`[*]` treat the array like
        // `$@` (see `array_use_alternate_error_operators` for full coverage).
        assert_eq!(run("a=(); echo ${a[@]:-def}").0, "def\n");
        assert_eq!(run("a=(p q); echo ${a[@]:-def}").0, "p q\n");
        // …and the element-wise (bulk) operators.
        assert_eq!(run("a=(a.x b.x); echo ${a[*]#*.}").0, "x x\n");
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
    fn assoc_key_with_unquoted_spaces() {
        // bash's tokenizer keeps a `name[…]=` subscript as one assignment word
        // even with unquoted interior spaces, so `h[a b]=v` stores under the
        // literal key "a b". Chained assignments on one line also work.
        assert_eq!(
            run("declare -A h; h[a b]=1 h[c d]=2; echo \"${h[a b]}${h[c d]}\"").0,
            "12\n"
        );
        assert_eq!(
            run("declare -A h; h[key with space]=v; echo \"${h[key with space]}\"").0,
            "v\n"
        );
        // Argument position still word-splits: `echo h[a b]` prints two fields.
        assert_eq!(run("echo h[a b]").0, "h[a b]\n");
    }

    #[test]
    fn assoc_key_preserves_surrounding_whitespace() {
        // bash never trims an associative subscript: `h[ x ]=v` keys on the
        // literal " x " (with surrounding spaces), on both the store and the read
        // path. A no-space read `${h[x]}` therefore does NOT match. (Regression
        // for TD-OILS-ASSOC-KEY-TRIM.)
        assert_eq!(run("declare -A h; h[ x ]=v; echo \"[${!h[@]}]\"").0, "[ x ]\n");
        assert_eq!(run("declare -A h; h[ x ]=v; echo \"[${h[ x ]}]\"").0, "[v]\n");
        assert_eq!(run("declare -A h; h[ x ]=v; echo \"[${h[x]}]\"").0, "[]\n");
        // Same for a keyed array-literal element `([ x ]=v)`.
        assert_eq!(
            run("declare -A m=([ x ]=v); echo \"[${!m[@]}]\"").0,
            "[ x ]\n"
        );
        // Indexed subscripts still arithmetic-evaluate (whitespace ignored).
        assert_eq!(run("declare -a a; a[ 1 + 2 ]=v; echo ${!a[@]}").0, "3\n");
    }

    #[test]
    fn declare_p_quotes_assoc_keys_needing_it() {
        // `declare -p` quotes an associative key when it holds a shell
        // metacharacter (so the subscript round-trips), and leaves "safe" keys
        // bare — matching bash.
        assert_eq!(
            run("declare -A m; m[x]=v; declare -p m").0,
            "declare -A m=([x]=\"v\" )\n"
        );
        assert_eq!(
            run("declare -A m; m[\"a b\"]=v; declare -p m").0,
            "declare -A m=([\"a b\"]=\"v\" )\n"
        );
        // `-`/`@`/`#` do not force quoting.
        assert_eq!(
            run("declare -A m; m[\"a-b\"]=v; declare -p m").0,
            "declare -A m=([a-b]=\"v\" )\n"
        );
    }

    #[test]
    fn assoc_scalar_assignment_targets_key_zero() {
        // A subscript-less `name=value` on an existing associative array assigns
        // to key "0" (bash: `declare -A b; b=a` yields `b[0]=a`). Same for the
        // `+=` append and the `-i` integer forms, and it coexists with other keys.
        assert_eq!(run("declare -A b; b=a; echo \"[${b[0]}]\"").0, "[a]\n");
        assert_eq!(run("declare -A b; b=x; b+=y; echo \"[${b[0]}]\"").0, "[xy]\n");
        assert_eq!(run("declare -A b; b+=y; echo \"[${b[0]}]\"").0, "[y]\n");
        assert_eq!(run("declare -Ai b; b=5; b+=3; echo \"[${b[0]}]\"").0, "[8]\n");
        assert_eq!(
            run("declare -A b=([k]=v); b=scalar; echo \"${b[0]}-${b[k]}\"").0,
            "scalar-v\n"
        );
    }

    #[test]
    fn declare_p_empty_array_distinguishes_assigned_from_declared() {
        // bash distinguishes an assigned-but-empty array (`a=()` → `=()`) from
        // one merely declared with `declare -a a` and never given a value
        // (bare `declare -a a`). osh tracks a per-name has-a-value flag.
        assert_eq!(run("a=(); declare -p a").0, "declare -a a=()\n");
        assert_eq!(run("declare -A m=(); declare -p m").0, "declare -A m=()\n");
        assert_eq!(run("declare -a a; declare -p a").0, "declare -a a\n");
        assert_eq!(run("declare -A m; declare -p m").0, "declare -A m\n");
        // An element assignment gives the array a value even after every
        // element is unset — the flag is sticky until the whole var is unset.
        assert_eq!(
            run("declare -a a; a[3]=x; unset 'a[3]'; declare -p a").0,
            "declare -a a=()\n"
        );
        assert_eq!(
            run("declare -A m; m[k]=v; unset 'm[k]'; declare -p m").0,
            "declare -A m=()\n"
        );
        // `+=` on an empty array is still an assignment.
        assert_eq!(run("a=(); a+=(); declare -p a").0, "declare -a a=()\n");
        // Combined attribute + empty literal.
        assert_eq!(run("declare -ai a=(); declare -p a").0, "declare -ai a=()\n");
        // Unsetting the whole variable clears the flag.
        assert_eq!(
            run("a=(); unset a; declare -a a; declare -p a").0,
            "declare -a a\n"
        );
    }

    #[test]
    fn read_a_creates_empty_array_on_eof() {
        // bash's `read -a arr` resets the target to empty up front, so even an
        // EOF with no data leaves a defined, empty array (and a pre-existing
        // array is replaced, not merged).
        assert_eq!(
            run("read -a arr < /dev/null; echo rc=$?; declare -p arr").0,
            "rc=1\ndeclare -a arr=()\n"
        );
        assert_eq!(
            run("arr=(old); read -a arr < /dev/null; declare -p arr").0,
            "declare -a arr=()\n"
        );
        assert_eq!(
            run("read -a arr <<< 'x y'; declare -p arr").0,
            "declare -a arr=([0]=\"x\" [1]=\"y\")\n"
        );
    }

    #[test]
    fn shellopts_reflects_option_state_and_is_readonly() {
        // bash exposes the enabled `set -o` options as a readonly, colon-
        // separated, alphabetically-sorted list. A non-interactive shell's
        // default is braceexpand:hashall:interactive-comments.
        assert_eq!(
            run("echo \"[$SHELLOPTS]\"").0,
            "[braceexpand:hashall:interactive-comments]\n"
        );
        // Enabling an option inserts its long name in sorted position.
        assert_eq!(
            run("set -u; echo \"$SHELLOPTS\"").0,
            "braceexpand:hashall:interactive-comments:nounset\n"
        );
        assert_eq!(
            run("set -f -a -C -x; echo \"$SHELLOPTS\"").0,
            "allexport:braceexpand:hashall:interactive-comments:noclobber:noglob:xtrace\n"
        );
        // Disabling removes it again.
        assert_eq!(
            run("set -u; set +u; echo \"$SHELLOPTS\"").0,
            "braceexpand:hashall:interactive-comments\n"
        );
        // `declare -p` renders it readonly (not exported).
        assert_eq!(
            run("set -u; declare -p SHELLOPTS").0,
            "declare -r SHELLOPTS=\"braceexpand:hashall:interactive-comments:nounset\"\n"
        );
        // It cannot be assigned to.
        assert_eq!(run("SHELLOPTS=foo; echo after").1, 1);
    }

    #[test]
    fn bulk_attr_transform_empty_positional_is_empty() {
        // `${@@A}` / `${*@A}` with no positional parameters yields nothing (not a
        // bare `set -- `), matching bash.
        assert_eq!(run("set -- ; echo \"[${@@A}]\"").0, "[]\n");
        assert_eq!(run("set -- ; echo \"[${*@A}]\"").0, "[]\n");
        // With params it still produces the re-inputtable `set --` form.
        assert_eq!(run("set -- x y; echo \"${@@A}\"").0, "set -- 'x' 'y'\n");
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
        // Absolute path under the temp dir (forward slashes so it feeds cleanly
        // into shell scripts). Using an absolute path makes these tests
        // independent of the process cwd, so they never race the cwd-mutating
        // tests (`cd`/`pushd`) even though they don't hold `cwd_guard`.
        let tmp = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        let tmp = tmp.trim_end_matches('/');
        format!(
            "{tmp}/osh_{tag}_{}_{}",
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
    fn dev_null_read_yields_eof() {
        // Reading `/dev/null` must yield EOF (empty, rc=1), not the contents of
        // a stray host file. On Windows this is mapped to the `NUL` device.
        let (o, st) = run("read x < /dev/null; echo \"rc=$? x=[$x]\"");
        assert_eq!(o, "rc=1 x=[]\n");
        assert_eq!(st, 0);
        assert_eq!(run("cat < /dev/null; echo done").0, "done\n");
    }

    #[test]
    fn dev_null_write_discards() {
        // Writing to `/dev/null` must discard output, not create a real file.
        let (_, st) = run("echo junk > /dev/null");
        assert_eq!(st, 0);
        // A subsequent read of /dev/null still sees EOF (nothing was persisted).
        assert_eq!(run("read x < /dev/null; echo \"[$x]\"").0, "[]\n");
    }

    #[test]
    fn same_command_fd_dup_resolves() {
        // Regression: a *simple* command that opens a scratch descriptor and
        // then dups from it within the SAME command (`3>&1 2>&3`) must resolve
        // the dup rather than report "3: Bad file descriptor". The collapsed
        // RedirPlan buckets each effect into a fixed slot and loses left-to-
        // right order, so only `exec` and compound commands used to install the
        // scratch fds. The executor now materialises `extra_fds` for simple
        // builtins and externals too (via install/restore around the run),
        // mirroring the compound path. Captured through command substitution
        // (fd 1 is a pipe), so the dup aliases onto that pipe.
        // Builtin `echo`:
        assert_eq!(run("r=$(echo hi 3>&1 2>&3); echo \"[$r]\""), ("[hi]\n".into(), 0));
        // A different scratch fd number works the same way.
        assert_eq!(run("r=$(echo hi 4>&1 2>&4); echo \"[$r]\""), ("[hi]\n".into(), 0));
        // External command (`env echo`): the child inherits the resolved fds.
        assert_eq!(run("r=$(env echo hi 3>&1 2>&3); echo \"[$r]\""), ("[hi]\n".into(), 0));
    }

    #[test]
    fn read_dash_t_zero_polls_without_consuming() {
        // `read -t 0` is a non-consuming availability poll: it assigns nothing
        // and exits 0 when a read would proceed without blocking (bash returns 0
        // for a seekable/ready source — including an empty file or EOF — and
        // non-zero only when a read would block). Regression for `-t` previously
        // being parsed-and-ignored, which turned `-t 0` into a real blocking
        // read that also consumed a line.
        // A here-string source is always ready:
        assert_eq!(run("read -t 0 <<< \"x\"; echo $?"), ("0\n".into(), 0));
        // An empty here-string is still ready (bash: 0):
        assert_eq!(run("read -t 0 <<< \"\"; echo $?"), ("0\n".into(), 0));
        // /dev/null (immediate EOF) counts as ready:
        assert_eq!(run("read -t 0 </dev/null; echo $?"), ("0\n".into(), 0));
        // It must NOT consume: a following `read` still sees the first line.
        assert_eq!(run("{ read -t 0; read x; echo \"[$x]\"; } <<< \"keep\""), ("[keep]\n".into(), 0));
    }

    #[test]
    fn ulimit_reports_and_sets_limits() {
        // A single resource prints the bare value; `-n` defaults to 1024.
        assert_eq!(run("ulimit -n"), ("1024\n".into(), 0));
        // No option letter defaults to the file-size limit (`-f`), unlimited.
        assert_eq!(run("ulimit"), ("unlimited\n".into(), 0));
        // Setting a soft limit is honoured by a subsequent query.
        assert_eq!(run("ulimit -n 512; ulimit -n"), ("512\n".into(), 0));
        // `unlimited` operand round-trips.
        assert_eq!(run("ulimit -c unlimited; ulimit -c"), ("unlimited\n".into(), 0));
        // Setting without -H/-S touches both; -H then reads the hard value.
        assert_eq!(run("ulimit -n 256; ulimit -Hn"), ("256\n".into(), 0));
        // -S alone leaves the hard limit unchanged (still unlimited by default).
        assert_eq!(run("ulimit -Sn 100; ulimit -Hn"), ("unlimited\n".into(), 0));
        // Accepting a set operand returns success (common `ulimit -c 0` idiom).
        assert_eq!(run("ulimit -c 0; echo $?"), ("0\n".into(), 0));
    }

    #[test]
    fn ulimit_errors_and_multi() {
        // An unknown option letter is a usage error (exit 2), like bash.
        let (out, code) = run("ulimit -z 2>&1");
        assert_eq!(code, 2);
        assert!(out.contains("invalid option"), "got {out:?}");
        // A bad limit argument is rejected without changing state.
        let (out, code) = run("ulimit -n abc 2>&1; ulimit -n");
        assert_eq!(code, 0); // trailing `ulimit -n` succeeds
        assert!(out.contains("invalid limit argument"), "got {out:?}");
        assert!(out.trim_end().ends_with("1024"), "limit unchanged: {out:?}");
        // Multiple resource letters print one labelled line each.
        let (out, code) = run("ulimit -c -n");
        assert_eq!(code, 0);
        assert!(out.contains("core file size") && out.contains("open files"), "got {out:?}");
    }

    #[test]
    fn ulimit_dash_a_lists_all() {
        let (out, code) = run("ulimit -a");
        assert_eq!(code, 0);
        // One line per modelled resource, in bash's order.
        assert_eq!(out.lines().count(), 16);
        assert!(out.contains("open files"));
        assert!(out.contains("(-n) 1024"));
        assert!(out.starts_with("core file size"));
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
    fn pipe_ampersand_pipes_stdout_and_stderr() {
        // `cmd |& rhs` is bash shorthand for `cmd 2>&1 | rhs`: both streams reach
        // the right-hand side. The RHS reads each line and re-emits it.
        let (o, _) = run("{ echo o; echo e >&2; } |& while read l; do echo \"[$l]\"; done");
        assert_eq!(o, "[o]\n[e]\n");
    }

    #[test]
    fn pipe_ampersand_with_explicit_left_redirect() {
        // The implicit `2>&1` is applied *after* the left command's own
        // redirects, so a preceding `2>/dev/null` is overridden and stderr still
        // reaches the pipe (bash semantics).
        let (o, _) =
            run("{ echo o; echo e >&2; } 2>/dev/null |& while read l; do echo \"<$l>\"; done");
        assert_eq!(o, "<o>\n<e>\n");
    }

    #[test]
    fn redirect_dup_last_writer_wins() {
        // `2>/dev/null 2>&1` — the later `2>&1` re-points stderr onto stdout's
        // sink, overriding the earlier file target (a common `|&`-adjacent idiom
        // that the old order-free RedirPlan got wrong).
        let (o, _) = run("x=$( { echo out; echo err >&2; } 2>/dev/null 2>&1 ); echo \"$x\"");
        assert_eq!(o, "out\nerr\n");
        // Reverse order: the later `2>/dev/null` wins, so only stdout is captured.
        let (o2, _) = run("x=$( { echo out; echo err >&2; } 2>&1 2>/dev/null ); echo \"[$x]\"");
        assert_eq!(o2, "[out]\n");
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

    #[test]
    fn set_cluster_consumes_o_long_option() {
        // `set -eo pipefail` must enable BOTH errexit (`-e`) and pipefail (the
        // `-o` selector consuming the following word `pipefail`). Regression:
        // osh previously treated the clustered `o` as an ignored flag and left
        // `pipefail` to become a positional, so pipefail stayed off.
        let mut sh = Shell::new();
        assert_eq!(sh.run_source("set -eo pipefail; shopt -oq pipefail && shopt -oq errexit"), 0);
        // The `o` may appear anywhere in the cluster; remaining letters stay
        // flags, and successive `o`s consume successive following words.
        let mut sh = Shell::new();
        assert_eq!(sh.run_source("set -oe pipefail; shopt -oq pipefail && shopt -oq errexit"), 0);
        let mut sh = Shell::new();
        assert_eq!(
            sh.run_source("set -oo pipefail xtrace; shopt -oq pipefail && shopt -oq xtrace"),
            0
        );
        // Words left after the consumed option name become positionals.
        assert_eq!(run(r#"set -eo pipefail extra; echo "$1""#).0, "extra\n");
        // `+eo` disables both.
        let mut sh = Shell::new();
        sh.run_source("set -e -o pipefail");
        sh.run_source("set +eo pipefail");
        assert_eq!(sh.run_source("shopt -oq pipefail || shopt -oq errexit"), 1);
        // The end-to-end effect: errexit fires on a pipefail-surfaced failure.
        assert_eq!(run("set -eo pipefail; false | true; echo reached"), (String::new(), 1));
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
    fn wait_n_returns_next_completed() {
        // `wait -n` returns as soon as one job finishes; a second `wait -n`
        // reaps the other. `-p VAR` records the returned job's pid.
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 5 &");
        sh.run_source("cmd /c exit 6 &");
        assert_eq!(sh.jobs.len(), 2);
        let first = sh.run_source("wait -n -p done_pid");
        assert!(first == 5 || first == 6, "unexpected status {first}");
        assert_eq!(sh.jobs.len(), 1);
        // The pid variable was set to a plausible pid.
        assert!(sh.vars.get("done_pid").is_some_and(|p| p.parse::<u32>().is_ok()));
        let second = sh.run_source("wait -n");
        assert!(second == 5 || second == 6);
        assert_ne!(first, second);
        assert!(sh.jobs.is_empty());
        // `wait -n` with no jobs left returns 127.
        assert_eq!(sh.run_source("wait -n"), 127);
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

    #[cfg(windows)]
    #[test]
    fn fg_echoes_command_and_waits_for_status() {
        // `fg` (no spec) foregrounds the current job: it prints the command line
        // and blocks until the job finishes, returning its exit status.
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 7 &");
        assert_eq!(sh.jobs.len(), 1);
        let mut buf = Vec::new();
        let status = {
            let prog = parse("fg").expect("parse");
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
            sh.last_status
        };
        assert_eq!(status, 7);
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("cmd /c exit 7"), "fg output: {s:?}");
        assert!(sh.jobs.is_empty(), "fg should remove the job");
    }

    #[cfg(windows)]
    #[test]
    fn fg_by_job_spec_targets_named_job() {
        // `fg %n` targets a specific job by its job number.
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 3 &");
        assert_eq!(sh.run_source("fg %1"), 3);
        assert!(sh.jobs.is_empty());
    }

    #[test]
    fn fg_no_jobs_errors() {
        // With no jobs, `fg` reports an error and returns 1.
        let mut sh = Shell::new();
        assert_eq!(sh.run_source("fg"), 1);
        // A non-existent job spec is also an error.
        assert_eq!(sh.run_source("fg %9"), 1);
    }

    #[cfg(windows)]
    #[test]
    fn bg_reports_job_in_bash_form() {
        // `bg` reports the targeted job in `[id] cmd &` form and returns 0.
        // (Jobs already run in the background here — bg is a reporting no-op.)
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 0 &");
        let mut buf = Vec::new();
        {
            let prog = parse("bg").expect("parse");
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
        }
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("[1]"), "bg output: {s:?}");
        assert!(s.contains("cmd /c exit 0 &"), "bg output: {s:?}");
        assert_eq!(sh.last_status, 0);
        sh.run_source("wait");
    }

    #[test]
    fn bg_no_jobs_errors() {
        // With no jobs, `bg` reports an error and returns 1.
        let mut sh = Shell::new();
        assert_eq!(sh.run_source("bg"), 1);
        assert_eq!(sh.run_source("bg %5"), 1);
    }

    #[cfg(windows)]
    #[test]
    fn disown_removes_job_from_table() {
        // `disown %1` drops the job so `jobs` no longer reports it.
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 0 &");
        assert_eq!(sh.jobs.len(), 1);
        assert_eq!(sh.run_source("disown %1"), 0);
        assert!(sh.jobs.is_empty(), "job should be removed after disown");
    }

    #[cfg(windows)]
    #[test]
    fn disown_all_and_running_flags() {
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 0 &");
        sh.run_source("cmd /c exit 0 &");
        assert_eq!(sh.jobs.len(), 2);
        // `disown -a` clears every tracked job.
        assert_eq!(sh.run_source("disown -a"), 0);
        assert!(sh.jobs.is_empty(), "disown -a should clear all jobs");
    }

    #[cfg(windows)]
    #[test]
    fn disown_h_marks_without_removing() {
        // `disown -h` keeps the job but flags it no-SIGHUP.
        let mut sh = Shell::new();
        sh.run_source("cmd /c exit 0 &");
        assert_eq!(sh.run_source("disown -h %1"), 0);
        assert_eq!(sh.jobs.len(), 1, "disown -h keeps the job in the table");
        assert!(sh.jobs[0].no_hup, "disown -h sets no_hup");
        sh.run_source("wait");
    }

    #[cfg(windows)]
    #[test]
    fn disown_bad_spec_errors() {
        let mut sh = Shell::new();
        assert_eq!(sh.run_source("disown %9"), 1);
    }

    #[test]
    fn enable_lists_builtins() {
        // Bare `enable` lists enabled builtins in re-inputtable form.
        let (o, s) = run("enable");
        assert_eq!(s, 0);
        assert!(o.contains("enable echo\n"), "enable output: {o:?}");
        assert!(o.contains("enable times\n"), "enable output: {o:?}");
    }

    #[test]
    fn enable_n_disables_and_lists() {
        // `enable -n NAME` disables; `enable -n` then lists the disabled ones.
        let (o, s) = run("enable -n cd; enable -n");
        assert_eq!(s, 0);
        assert!(o.contains("enable -n cd\n"), "disabled list: {o:?}");
    }

    #[test]
    fn enable_reenable_removes_from_disabled() {
        let (o, _) = run("enable -n cd; enable cd; enable -n");
        assert!(!o.contains("enable -n cd\n"), "cd should be re-enabled: {o:?}");
    }

    #[test]
    fn enable_unknown_name_errors() {
        assert_eq!(run("enable nosuchbuiltin").1, 1);
    }

    #[test]
    fn enable_n_bypasses_builtin_resolution() {
        // `command -v times` finds the builtin (status 0); once `times` is
        // disabled it is no longer a builtin, so with no external of that name
        // resolution fails (status 1) — proving `enable -n` bypasses the builtin.
        assert_eq!(run("command -v times").1, 0);
        assert_eq!(run("enable -n times; command -v times").1, 1);
        // `type -t` likewise stops reporting it as a builtin.
        assert_eq!(run("type -t times").0, "builtin\n");
        assert_eq!(run("enable -n times; type -t times").1, 1);
    }

    #[test]
    fn declare_big_f_lists_functions() {
        let (o, s) = run("foo() { echo hi; }; bar() { :; }; declare -F");
        assert_eq!(s, 0);
        assert!(o.contains("declare -f foo\n"), "declare -F output: {o:?}");
        assert!(o.contains("declare -f bar\n"), "declare -F output: {o:?}");
    }

    #[test]
    fn declare_attr_filter_lists_matching_vars() {
        // `declare -A` (no names) lists only associative arrays, in
        // re-inputtable declare-prefix form, sorted by name.
        let (o, s) = run("declare -A m=([x]=1); declare -A n=([y]=2); a=(1 2); declare -A");
        assert_eq!(s, 0);
        assert_eq!(o, "declare -A m=([x]=\"1\" )\ndeclare -A n=([y]=\"2\" )\n");

        // `declare -i` lists only integer-attributed variables.
        let (o2, _) = run("declare -i k=5; s=hi; declare -i k2=9; declare -i");
        assert_eq!(o2, "declare -i k=\"5\"\ndeclare -i k2=\"9\"\n");

        // Union semantics: `declare -il` lists variables that are integer OR
        // lowercase-attributed (bash), each shown with its full attribute set.
        // (Avoids bash/osh internal readonly vars like BASH_VERSINFO that a
        // `-ir` union would also match.)
        let (o3, _) = run("declare -i ii=1; declare -l low=HELLO; plain=3; declare -il");
        assert_eq!(o3, "declare -i ii=\"1\"\ndeclare -l low=\"hello\"\n");

        // A declaration *with* a name operand still declares (not a listing).
        assert_eq!(run("declare -A m=([k]=v); echo ${m[k]}").0, "v\n");
    }

    #[test]
    fn declare_big_f_named_prints_name() {
        let (o, s) = run("foo() { :; }; declare -F foo");
        assert_eq!(s, 0);
        assert_eq!(o, "foo\n");
    }

    #[test]
    fn declare_big_f_unknown_status_1() {
        let (o, s) = run("declare -F nofunc");
        assert_eq!(s, 1);
        assert_eq!(o, "");
    }

    #[test]
    fn declare_small_f_existence_status() {
        // `declare -f fn` returns 0 for an existing function and 1 otherwise.
        assert_eq!(run("foo() { :; }; declare -f foo").1, 0);
        assert_eq!(run("declare -f nofunc").1, 1);
    }

    #[test]
    fn declare_small_f_prints_body() {
        // `declare -f fn` reconstructs the function's source.
        let (o, s) = run("foo() { echo hi; }; declare -f foo");
        assert_eq!(s, 0);
        assert!(o.contains("foo () "), "declare -f output: {o:?}");
        assert!(o.contains("echo hi"), "declare -f output: {o:?}");
        // The dump re-parses and runs to the same effect.
        let (o2, _) = run("foo() { echo hi; }; eval \"$(declare -f foo)\"; foo");
        assert_eq!(o2, "hi\n");
    }

    #[test]
    fn type_function_prints_body() {
        // `type fn` prints the "is a function" line then the reconstructed source.
        let (o, _) = run("foo() { echo hi; }; type foo");
        assert!(o.contains("foo is a function"), "type output: {o:?}");
        assert!(o.contains("echo hi"), "type output: {o:?}");
    }

    #[test]
    fn bare_set_lists_functions() {
        // Bare `set` prints functions after the variables.
        let (o, _) = run("foo() { echo hi; }; set");
        assert!(o.contains("foo () "), "set output: {o:?}");
        assert!(o.contains("echo hi"), "set output: {o:?}");
    }

    #[test]
    fn set_f_disables_globbing() {
        // `set -f` (noglob): glob patterns stay literal.
        assert_eq!(run("set -f; echo *.xyz").0, "*.xyz\n");
        assert_eq!(run("set -f; echo a?b").0, "a?b\n");
        // Long form via `set -o noglob`.
        assert_eq!(run("set -o noglob; echo *").0, "*\n");
    }

    #[test]
    fn set_a_allexport_marks_exported() {
        // `set -a` (allexport): assigned variables are auto-exported.
        let (o, s) = run("set -a; foo=bar; declare -p foo");
        assert_eq!(s, 0);
        assert!(o.contains("declare -x"), "declare -p output: {o:?}");
        assert!(o.contains("foo"), "declare -p output: {o:?}");
    }

    #[test]
    fn export_p_lists_exported_variables() {
        // `export -p` (and bare `export`) list exported variables as
        // `declare -x NAME="value"`, an exported-but-unset name as the bare
        // `declare -x NAME`, and fold in other attributes (readonly → -rx).
        let (o, s) = run("export FOO=bar; export BARE; export EMPTY=; export -p");
        assert_eq!(s, 0);
        assert!(o.contains("declare -x FOO=\"bar\"\n"), "got {o:?}");
        assert!(o.contains("declare -x BARE\n"), "got {o:?}");
        assert!(o.contains("declare -x EMPTY=\"\"\n"), "got {o:?}");
        // Readonly + exported shows both flags.
        let (o2, _) = run("declare -rx RO=locked; export -p");
        assert!(o2.contains("declare -rx RO=\"locked\"\n"), "got {o2:?}");
        // `-n` removes the export attribute (variable value is kept).
        let (o3, _) = run("export FOO=bar; export -n FOO; export -p");
        assert!(!o3.contains("declare -x FOO"), "got {o3:?}");
        assert_eq!(run("export FOO=bar; export -n FOO; echo \"$FOO\"").0, "bar\n");
        // Bare `export` (no flags/operands) lists too.
        let (o4, _) = run("export ZZ=1; export");
        assert!(o4.contains("declare -x ZZ=\"1\"\n"), "got {o4:?}");
    }

    #[test]
    fn set_o_lists_noglob_and_allexport() {
        let (o, _) = run("set -o");
        assert!(o.contains("noglob"), "set -o list: {o:?}");
        assert!(o.contains("allexport"), "set -o list: {o:?}");
    }

    #[test]
    fn test_o_operator_reads_noglob() {
        // `[ -o noglob ]` reflects the current option state.
        assert_eq!(run("set -f; [ -o noglob ]; echo $?").0, "0\n");
        assert_eq!(run("[ -o noglob ]; echo $?").0, "1\n");
    }

    #[test]
    fn noclobber_blocks_overwrite() {
        // `set -C` makes a plain `>` refuse to truncate an existing file.
        let p = std::env::temp_dir().join("osh_noclobber_1.txt");
        let _ = std::fs::remove_file(&p);
        let ps = p.to_string_lossy().replace('\\', "/");
        let src = format!("echo one > \"{ps}\"; set -C; echo two > \"{ps}\"; echo status=$?");
        let (o, _) = run(&src);
        assert!(o.contains("status=1"), "output: {o:?}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\n");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn clobber_override_bypasses_noclobber() {
        // `>|` overrides noclobber and truncates the existing file.
        let p = std::env::temp_dir().join("osh_noclobber_2.txt");
        let _ = std::fs::remove_file(&p);
        let ps = p.to_string_lossy().replace('\\', "/");
        let src = format!("echo one > \"{ps}\"; set -C; echo two >| \"{ps}\"; echo status=$?");
        let (o, _) = run(&src);
        assert!(o.contains("status=0"), "output: {o:?}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two\n");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn set_plus_c_re_enables_overwrite() {
        // `set +C` clears noclobber, so `>` may truncate again.
        let p = std::env::temp_dir().join("osh_noclobber_3.txt");
        let _ = std::fs::remove_file(&p);
        let ps = p.to_string_lossy().replace('\\', "/");
        let src =
            format!("echo one > \"{ps}\"; set -C; set +C; echo two > \"{ps}\"; echo status=$?");
        let (o, _) = run(&src);
        assert!(o.contains("status=0"), "output: {o:?}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two\n");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn xtrace_traces_assignments_and_commands() {
        // A plain scalar traces its expanded value, minimally single-quoted.
        assert_eq!(run("{ set -x; x=5; } 2>&1").0, "+ x=5\n");
        assert_eq!(run("{ y=hi; set -x; x=\"$y z\"; } 2>&1").0, "+ x='hi z'\n");
        // An empty value is shown unquoted (bash: `+ x=`).
        assert_eq!(run("{ set -x; x=; } 2>&1").0, "+ x=\n");
        // Append keeps the `+=` operator with the RHS value.
        assert_eq!(run("{ x=1; set -x; x+=2; } 2>&1").0, "+ x+=2\n");
        // Array and indexed-element assignments trace in source form (bash does
        // not expand them for the trace).
        assert_eq!(run("{ set -x; a=(1 2 3); } 2>&1").0, "+ a=(1 2 3)\n");
        assert_eq!(run("{ declare -a x; set -x; x[1+1]=v; } 2>&1").0, "+ x[1+1]=v\n");
        // Command arguments are minimally quoted behind the default `+ ` prefix.
        // (`true` is used so the trace is the only output — the harness buffers
        // stdout and the redirected stderr separately, so mixing them would make
        // ordering harness-dependent.)
        assert_eq!(run("{ set -x; true 'a b' c; } 2>&1").0, "+ true 'a b' c\n");
        // A temporary prefix assignment traces on its own line first.
        assert_eq!(run("{ set -x; x=5 true; } 2>&1").0, "+ x=5\n+ true\n");
        // `PS4` overrides the trace prefix.
        assert_eq!(run("{ PS4='TRACE '; set -x; x=5; } 2>&1").0, "TRACE x=5\n");
    }

    #[test]
    fn xtrace_traces_compound_headers() {
        // `for` prints a source-form header before *each* iteration.
        assert_eq!(
            run("{ set -x; for i in a b; do :; done; } 2>&1").0,
            "+ for i in a b\n+ :\n+ for i in a b\n+ :\n"
        );
        // `for name; do` (no explicit list) traces as `for name in \"$@\"`.
        assert_eq!(
            run("{ set -- p; set -x; for i; do :; done; } 2>&1").0,
            "+ for i in \"$@\"\n+ :\n"
        );
        // C-style `for ((...))`: init once, cond before each test, update after
        // each body; an empty section traces as always-true `(( 1 ))`.
        assert_eq!(
            run("{ set -x; for ((i=0;i<2;i++)); do :; done; } 2>&1").0,
            "+ (( i=0 ))\n+ (( i<2 ))\n+ :\n+ (( i++ ))\n+ (( i<2 ))\n+ :\n+ (( i++ ))\n+ (( i<2 ))\n"
        );
        assert_eq!(
            run("{ set -x; for ((;;)); do break; done; } 2>&1").0,
            "+ (( 1 ))\n+ (( 1 ))\n+ break\n"
        );
        // `(( ))` command traces with its raw (whitespace-preserving) text.
        assert_eq!(run("{ set -x; ((1+1)); } 2>&1").0, "+ (( 1+1 ))\n");
        assert_eq!(run("{ set -x; (( 2 > 1 )); } 2>&1").0, "+ ((  2 > 1  ))\n");
        // `while`/`until` have no header; their `(( ))` conditions self-trace.
        assert_eq!(
            run("{ i=0; set -x; while ((i<1)); do ((i++)); done; } 2>&1").0,
            "+ (( i<1 ))\n+ (( i++ ))\n+ (( i<1 ))\n"
        );
        // `case` prints `case WORD in` (source form) before matching.
        assert_eq!(
            run("{ x=foo; set -x; case $x in f*) :;; esac; } 2>&1").0,
            "+ case $x in\n+ :\n"
        );
    }

    #[test]
    fn noclobber_allows_append() {
        // `>>` is always permitted, even under noclobber.
        let p = std::env::temp_dir().join("osh_noclobber_4.txt");
        let _ = std::fs::remove_file(&p);
        let ps = p.to_string_lossy().replace('\\', "/");
        let src = format!("echo one > \"{ps}\"; set -C; echo two >> \"{ps}\"; echo status=$?");
        let (o, _) = run(&src);
        assert!(o.contains("status=0"), "output: {o:?}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\ntwo\n");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn noclobber_in_option_list_and_test_operator() {
        let (o, _) = run("set -o");
        assert!(o.contains("noclobber"), "set -o list: {o:?}");
        assert_eq!(run("set -C; [ -o noclobber ]; echo $?").0, "0\n");
        assert_eq!(run("[ -o noclobber ]; echo $?").0, "1\n");
    }

    #[test]
    fn times_prints_two_cpu_lines() {
        // `times` prints two "user sys" lines in bash's %dm%d.%03ds form.
        let (o, s) = run("times");
        assert_eq!(s, 0);
        assert_eq!(o, "0m0.000s 0m0.000s\n0m0.000s 0m0.000s\n");
    }

    #[test]
    fn umask_octal_get_set() {
        assert_eq!(run("umask").0, "0022\n");
        assert_eq!(run("umask 077; umask").0, "0077\n");
        assert_eq!(run("umask 0027; umask").0, "0027\n");
    }

    #[test]
    fn umask_symbolic_output() {
        assert_eq!(run("umask -S").0, "u=rwx,g=rx,o=rx\n");
        assert_eq!(run("umask 077; umask -S").0, "u=rwx,g=,o=\n");
    }

    #[test]
    fn umask_symbolic_set() {
        // allowed u=rwx,g=rx,o= → 0750 → mask 0027.
        assert_eq!(run("umask u=rwx,g=rx,o=; umask").0, "0027\n");
        // `a=` clears all permission bits → mask 0777.
        assert_eq!(run("umask a=; umask").0, "0777\n");
        // From default 0022 (allowed 0755), `go-r` → allowed 0711 → mask 0066.
        assert_eq!(run("umask go-r; umask").0, "0066\n");
    }

    #[test]
    fn umask_reusable_form() {
        assert_eq!(run("umask -p").0, "umask 0022\n");
        assert_eq!(run("umask -p -S").0, "umask -S u=rwx,g=rx,o=rx\n");
    }

    #[test]
    fn umask_invalid_mode() {
        assert_eq!(run("umask 8qq").1, 1);
        assert_eq!(run("umask u=z").1, 1);
    }

    #[test]
    fn test_o_option_operator() {
        // `[[ -o NAME ]]` and `[ -o NAME ]` report whether a `set -o` option is on.
        assert_eq!(run("set -e; [[ -o errexit ]] && echo yes").0, "yes\n");
        assert_eq!(run("[[ -o errexit ]] && echo yes || echo no").0, "no\n");
        assert_eq!(run("set -o pipefail; [ -o pipefail ] && echo p").0, "p\n");
        assert_eq!(run("set -u; [[ -o nounset ]] && echo u").0, "u\n");
        // Turning an option back off is reflected.
        assert_eq!(run("set -x; set +x; [[ -o xtrace ]] && echo x || echo off").0, "off\n");
        // Unknown option names are reported disabled (bash returns false).
        assert_eq!(run("[[ -o bogus ]] && echo on || echo off").0, "off\n");
    }

    #[test]
    fn hash_p_remembers_and_prints() {
        // `-p PATH NAME` remembers without a search; `-t` prints it back.
        assert_eq!(run("hash -p /bin/foo foo; hash -t foo").0, "/bin/foo\n");
        // Multiple `-t` names are prefixed with the name.
        let (o, _) = run("hash -p /a x; hash -p /b y; hash -t x y");
        assert_eq!(o, "x\t/a\ny\t/b\n");
    }

    #[test]
    fn hash_lists_and_clears() {
        // Bare `hash` prints the table (paths, sorted); `-l` is re-inputtable.
        let (o, _) = run("hash -p /bin/a a; hash -p /bin/b b; hash");
        assert!(o.starts_with("hits\tcommand\n"), "got {o:?}");
        assert!(o.contains("/bin/a"), "got {o:?}");
        assert!(o.contains("/bin/b"), "got {o:?}");
        assert_eq!(
            run("hash -p /bin/a a; hash -l").0,
            "builtin hash -p /bin/a a\n"
        );
        // `-r` forgets everything; `-t` then fails.
        assert_eq!(run("hash -p /x foo; hash -r; hash -t foo").1, 1);
        // Empty table prints nothing.
        assert_eq!(run("hash").0, "");
    }

    #[test]
    fn hash_delete_and_missing() {
        assert_eq!(run("hash -p /a x; hash -d x; hash -t x").1, 1);
        assert_eq!(run("hash -d nope").1, 1);
    }

    #[cfg(windows)]
    #[test]
    fn hash_caches_executed_external() {
        // Running an external caches its resolved path; `hash -t` finds it.
        let (o, s) = run("cmd /c exit 0\nhash -t cmd");
        assert_eq!(s, 0, "hash -t cmd should succeed; out {o:?}");
        assert!(o.to_lowercase().contains("cmd"), "got {o:?}");
    }

    #[test]
    fn array_slice_expansion() {
        // `${a[@]:off:len}` selects a run of elements by position.
        assert_eq!(run("a=(zero one two three four); echo ${a[@]:1:2}").0, "one two\n");
        assert_eq!(run("a=(zero one two three four); echo ${a[@]:2}").0, "two three four\n");
        // Negative offset counts from the end. A negative *length*, unlike a
        // string substring, is a fatal error for an array slice (bash: "N:
        // substring expression < 0") — see `array_slice_negative_length_is_fatal`.
        assert_eq!(run("a=(zero one two three four); echo \"${a[@]: -2}\"").0, "three four\n");
        assert_eq!(run("a=(zero one two three four); echo ${a[@]:1:-1}"), (String::new(), 1));
        // Quoted slice preserves one field per element (spaces inside survive).
        assert_eq!(
            run("a=('a b' 'c d' e); for x in \"${a[@]:0:2}\"; do echo \"[$x]\"; done").0,
            "[a b]\n[c d]\n"
        );
        // Out-of-range slice yields nothing.
        assert_eq!(run("a=(x y); echo \"end[${a[@]:5}]\"").0, "end[]\n");
        // The slice offset/length may themselves contain a `${…}` whose `]`
        // must not be mistaken for the subscript's close. (Regression: the
        // subscript-close scan used the *last* `]` in the body, so
        // `${a[@]:${#a[@]}-2}` failed to parse as "unterminated '{}'".)
        assert_eq!(run("a=(1 2 3 4 5); echo ${a[@]:${#a[@]}-2}").0, "4 5\n");
        assert_eq!(run("a=(1 2 3 4 5); echo ${a[@]:1:${#a[@]}-2}").0, "2 3 4\n");
        assert_eq!(run("a=(1 2 3); echo ${a[${#a[@]}-1]}").0, "3\n");
        // A genuinely nested `[` in the subscript still balances correctly.
        assert_eq!(run("a=(10 20 30); echo ${a[a[0]/10]}").0, "20\n");
    }

    #[test]
    fn array_use_alternate_error_operators() {
        // `${a[@]:-word}` substitutes the default only when the array is null
        // (no elements, or all elements empty so the `[*]`-join is empty).
        assert_eq!(run("a=(); echo \"[${a[@]:-DEF}]\"").0, "[DEF]\n");
        assert_eq!(run("a=(1 2); echo \"${a[@]:-DEF}\"").0, "1 2\n");
        // A single empty element joins to "" → null → default used; two empty
        // elements join to " " → non-null → the elements (a space) are used.
        assert_eq!(run("a=(\"\"); echo \"[${a[@]:-DEF}]\"").0, "[DEF]\n");
        assert_eq!(run("a=(\"\" \"\"); echo \"[${a[@]:-DEF}]\"").0, "[ ]\n");
        // Quoted `[@]` keeps one field per element; the default splits when the
        // whole expansion is unquoted-substituted.
        assert_eq!(
            run("a=(1 2); for w in \"${a[@]:-d}\"; do echo \"<$w>\"; done").0,
            "<1>\n<2>\n"
        );
        assert_eq!(
            run("a=(); for w in \"${a[@]:-d e}\"; do echo \"<$w>\"; done").0,
            "<d e>\n"
        );
        // `[*]` joins with the first IFS char.
        assert_eq!(run("a=(a b); IFS=:; echo \"${a[*]:-x}\"").0, "a:b\n");
        // `:+` substitutes the alternate once when the array is non-null.
        assert_eq!(run("a=(1 2 3); echo \"${a[@]:+X}\"").0, "X\n");
        assert_eq!(run("a=(); echo \"end[${a[@]:+X}]\"").0, "end[]\n");
        // `:?` on a null array aborts with the message; a non-null array passes.
        assert_eq!(run("a=(1); echo \"${a[@]:?msg}\"").0, "1\n");
        let (out, st) = run("a=(); echo \"${a[@]:?msg}\"; echo after");
        assert_eq!(out, "");
        assert_ne!(st, 0);
        // `:=` returns a non-null array unchanged, but assigning to `a[@]` on a
        // null array is a "bad array subscript" error (bash) — abort.
        assert_eq!(run("a=(1 2); echo \"${a[@]:=x}\"").0, "1 2\n");
        let (out2, st2) = run("a=(); echo \"${a[@]:=x}\"; echo after");
        assert_eq!(out2, "");
        assert_ne!(st2, 0);
    }

    #[test]
    fn positional_slice_expansion() {
        // `${@:off:len}` slices positional parameters ($0 is index 0).
        assert_eq!(run("set -- p q r s; echo ${@:2:2}").0, "q r\n");
        assert_eq!(run("set -- p q r s; echo ${@:3}").0, "r s\n");
    }

    #[test]
    fn quoted_at_preserves_fields() {
        // `"$@"` yields one field per positional parameter, preserving spaces.
        assert_eq!(
            run(r#"set -- "a b" c d; for x in "$@"; do echo "<$x>"; done"#).0,
            "<a b>\n<c>\n<d>\n"
        );
        // `"$*"` joins into a single field (default IFS ⇒ space separator).
        assert_eq!(
            run(r#"set -- "a b" c d; for x in "$*"; do echo "<$x>"; done"#).0,
            "<a b c d>\n"
        );
    }

    #[test]
    fn star_joins_with_ifs() {
        // `"$*"` joins with the first character of IFS.
        assert_eq!(run(r#"set -- a b c; IFS=-; echo "$*""#).0, "a-b-c\n");
        // Empty IFS joins with no separator.
        assert_eq!(run(r#"set -- a b c; IFS=; echo "$*""#).0, "abc\n");
    }

    #[test]
    fn array_star_joins_with_ifs() {
        // `"${arr[*]}"` joins with the first character of `$IFS` (like `"$*"`),
        // not always a space; previously osh hard-coded a space separator.
        assert_eq!(run(r#"a=(1 2 3); echo "${a[*]}""#).0, "1 2 3\n");
        assert_eq!(run(r#"a=(1 2 3); IFS=,; echo "${a[*]}""#).0, "1,2,3\n");
        assert_eq!(run(r#"a=(x y z); IFS=-; echo "${a[*]}""#).0, "x-y-z\n");
        // Empty IFS joins with no separator.
        assert_eq!(run(r#"a=(x y z); IFS=; echo "${a[*]}""#).0, "xyz\n");
        // Assigned to a scalar then read back.
        assert_eq!(run(r#"a=(1 2 3); IFS=:; s="${a[*]}"; echo "$s""#).0, "1:2:3\n");
    }

    #[test]
    fn count_of_positional_params() {
        // `${#@}` and `${#*}` are the count of positional parameters, not the
        // length of their joined string.
        assert_eq!(run("set -- p q r; echo ${#@} ${#*}").0, "3 3\n");
        assert_eq!(run("set -- one two; echo ${#@}").0, "2\n");
    }

    #[test]
    fn bulk_array_transforms() {
        // Suffix/prefix removal applied to every element.
        assert_eq!(
            run("a=(foo.txt bar.txt baz.txt); echo ${a[@]%.txt}").0,
            "foo bar baz\n"
        );
        assert_eq!(
            run("a=(x_1 x_2 x_3); echo ${a[@]#x_}").0,
            "1 2 3\n"
        );
        // Substitution applied per element.
        assert_eq!(
            run("a=(a.b c.d e.f); echo ${a[@]/./_}").0,
            "a_b c_d e_f\n"
        );
        // Global substitution per element.
        assert_eq!(
            run("a=(aa bb); echo ${a[@]//a/X}").0,
            "XX bb\n"
        );
        // Case modification per element.
        assert_eq!(
            run("a=(foo bar); echo ${a[@]^^}").0,
            "FOO BAR\n"
        );
        assert_eq!(
            run("a=(Foo Bar); echo ${a[@]^}").0,
            "Foo Bar\n"
        );
        // `@`-transform (`@Q`) quotes each element; quoted keeps per-element fields.
        assert_eq!(
            run("a=('a b' c); for x in \"${a[@]@Q}\"; do echo \"[$x]\"; done").0,
            "['a b']\n['c']\n"
        );
        // Quoted bulk trim yields one field per element (spaces inside survive).
        assert_eq!(
            run("a=('a b.x' 'c d.x'); for e in \"${a[@]%.x}\"; do echo \"[$e]\"; done").0,
            "[a b]\n[c d]\n"
        );
    }

    #[test]
    fn bulk_positional_transforms() {
        // Element-wise transform over the positional parameters.
        assert_eq!(run("set -- one.c two.c; echo ${@%.c}").0, "one two\n");
        assert_eq!(run("set -- ab cd; echo ${@^^}").0, "AB CD\n");
        assert_eq!(run("set -- a.b c.d; echo ${*/./-}").0, "a-b c-d\n");
    }

    #[test]
    fn unset_v_and_f_flags() {
        // `-v` unsets only the variable, leaving a same-named function intact.
        assert_eq!(
            run("f() { echo fn; }; f=1; unset -v f; f").0,
            "fn\n"
        );
        // `-f` unsets only the function, leaving the variable intact.
        assert_eq!(
            run("g() { echo fn; }; g=val; unset -f g; echo $g").0,
            "val\n"
        );
        // No flag: a set variable is removed in preference to the function.
        assert_eq!(
            run("h() { echo fn; }; h=v; unset h; echo \"[$h]\"; h").0,
            "[]\nfn\n"
        );
    }

    #[test]
    fn type_reports_hashed_command() {
        // A remembered command is described as "hashed (path)".
        let (o, s) = run("hash -p /bin/foo foo; type foo");
        assert_eq!(s, 0);
        assert_eq!(o, "foo is hashed (/bin/foo)\n");
    }

    #[test]
    fn set_no_args_lists_variables() {
        // Bare `set` lists shell variables in sorted, re-inputtable form.
        // Scalars use bash's minimal single-quote style (plain values unquoted),
        // while arrays keep the double-quoted element form.
        let (o, s) = run("zebra=1; apple=2; arr=(x y); set");
        assert_eq!(s, 0);
        assert!(o.contains("apple=2\n"), "got {o:?}");
        assert!(o.contains("zebra=1\n"), "got {o:?}");
        assert!(o.contains("arr=([0]=\"x\" [1]=\"y\")\n"), "got {o:?}");
        // Sorted: apple must appear before zebra.
        let ai = o.find("apple=").expect("apple");
        let zi = o.find("zebra=").expect("zebra");
        assert!(ai < zi, "expected sorted output, got {o:?}");
    }

    #[test]
    fn set_scalar_quoting_matches_bash() {
        // bash's bare `set` quotes scalar values minimally: raw when safe,
        // single-quoted around shell metacharacters, `$'…'` for control chars.
        let src = "a=hello; b='a b'; c=; g=a=b; num=5; lh='#x'; star='a*b'; nl=$'a\\nb'";
        let (o, s) = run(&format!("{src}; set"));
        assert_eq!(s, 0);
        assert!(o.contains("a=hello\n"), "got {o:?}");
        assert!(o.contains("b='a b'\n"), "got {o:?}");
        assert!(o.contains("c=\n"), "got {o:?}");
        assert!(o.contains("g=a=b\n"), "got {o:?}");
        assert!(o.contains("num=5\n"), "got {o:?}");
        // Leading `#`/`~` and glob/meta chars force single-quoting.
        assert!(o.contains("lh='#x'\n"), "got {o:?}");
        assert!(o.contains("star='a*b'\n"), "got {o:?}");
        // A newline (control char) uses ANSI-C quoting.
        assert!(o.contains("nl=$'a\\nb'\n"), "got {o:?}");
    }

    #[test]
    fn set_o_lists_options() {
        // `set -o` prints each modelled option with its on/off state, using
        // bash's `%-15s\t%s` layout (15-wide name, then a TAB, then the state).
        let (o, s) = run("set -e; set -o");
        assert_eq!(s, 0);
        assert!(o.contains("errexit        \ton\n"), "got {o:?}");
        assert!(o.contains("nounset        \toff\n"), "got {o:?}");
        assert!(o.contains("pipefail       \toff\n"), "got {o:?}");
        assert!(o.contains("xtrace         \toff\n"), "got {o:?}");
    }

    #[test]
    fn set_plus_o_lists_reinputtable() {
        // `set +o` prints re-inputtable `set -o NAME` / `set +o NAME` lines.
        let (o, _) = run("set -o pipefail; set +o");
        assert!(o.contains("set +o errexit\n"), "got {o:?}");
        assert!(o.contains("set -o pipefail\n"), "got {o:?}");
    }

    #[test]
    fn exec_no_command_is_noop() {
        // `exec` with no command word is a no-op that keeps running the script.
        let (o, s) = run("exec\necho hi");
        assert_eq!(o, "hi\n");
        assert_eq!(s, 0);
    }

    /// Run `src` with `Out::Inherit` (the ambient/terminal fd 1) so a
    /// persistent `exec > file` redirect is exercised, then return the bytes
    /// written to `path`. Uses a real temp file since the redirect diverts the
    /// shell's ambient stdout away from any in-memory capture.
    fn run_exec_redirect(src_tmpl: &str) -> String {
        use std::io::Read;
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let uniq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "osh_exec_{}_{}.txt",
            std::process::id(),
            uniq
        ));
        let p = path.to_string_lossy().replace('\\', "/");
        let src = src_tmpl.replace("{FILE}", &p);
        let mut sh = Shell::new();
        let prog = parse(&src).expect("parse");
        {
            let mut out = Out::Inherit;
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
        }
        let mut contents = String::new();
        if let Ok(mut f) = std::fs::File::open(&path) {
            let _ = f.read_to_string(&mut contents);
        }
        let _ = std::fs::remove_file(&path);
        contents
    }

    #[test]
    fn exec_redirects_stdout_persistently() {
        // `exec > file` rebinds fd 1 for every following command; both echoes
        // land in the file (append semantics after the initial truncate).
        assert_eq!(
            run_exec_redirect("exec > \"{FILE}\"\necho one\necho two"),
            "one\ntwo\n"
        );
    }

    #[test]
    fn exec_redirects_stdout_and_stderr_combined() {
        // `exec > file 2>&1` folds fd 2 into fd 1's file target, so both a
        // normal echo and a `>&2` diagnostic accumulate in the same file.
        assert_eq!(
            run_exec_redirect("exec > \"{FILE}\" 2>&1\necho out\necho err >&2"),
            "out\nerr\n"
        );
    }

    #[test]
    fn exec_redirects_stderr_only() {
        // `exec 2> file` redirects only fd 2; a `>&2` write lands in the file
        // while normal stdout is untouched (not written to the file).
        assert_eq!(
            run_exec_redirect("exec 2> \"{FILE}\"\necho diag >&2"),
            "diag\n"
        );
    }

    #[test]
    fn varfd_exec_allocates_and_persists() {
        // `exec {v}>file` allocates the lowest free fd ≥ 10, stores it in `v`,
        // and rebinds it persistently. `echo … >&$v` then writes to the file
        // through that descriptor.
        assert_eq!(
            run_exec_redirect("exec {v}>\"{FILE}\"\necho \"v=$v\" >&$v\necho hi >&$v"),
            "v=10\nhi\n"
        );
    }

    #[test]
    fn varfd_exec_variable_value() {
        // The auto-allocated descriptor number starts at 10 and is exported into
        // the named shell variable, readable after the redirect.
        let (o, s) = run("exec {v}>/dev/null; echo \"[$v]\"");
        assert_eq!(s, 0);
        assert_eq!(o, "[10]\n");
    }

    #[test]
    fn varfd_multiple_distinct_fds() {
        // Two `{name}>…` redirects in the same command get distinct descriptors
        // (10 and 11); neither collides with the other.
        let (o, s) = run("exec {a}>/dev/null {b}>/dev/null; echo \"$a $b\"");
        assert_eq!(s, 0);
        assert_eq!(o, "10 11\n");
    }

    #[test]
    fn varfd_compound_command() {
        // `{ … } {w}>file`: a compound command can carry a varfd redirect; the
        // body's `>&$w` writes reach the file and `$w` survives afterward.
        assert_eq!(
            run_exec_redirect("{ echo body >&$w; } {w}>\"{FILE}\"\necho \"w=$w\" >&$w"),
            "body\nw=10\n"
        );
    }

    #[test]
    fn dup_target_from_variable_is_a_dup_not_a_file() {
        // `>&$v` where `$v` expands to a descriptor number must duplicate that
        // descriptor, not create a file literally named after the number.
        // (Regression: the parser used to classify any non-literal `>&` target
        // as "both to file".)
        assert_eq!(
            run_exec_redirect("exec 3>\"{FILE}\"\nv=3\necho hi >&$v"),
            "hi\n"
        );
    }

    #[test]
    fn dup_target_nonnumeric_variable_on_fd1_is_both_to_file() {
        // `>&$f` with a non-numeric expansion on fd 1 means "both streams to the
        // file", matching bash's `>&file` behaviour.
        assert_eq!(
            run_exec_redirect("f=\"{FILE}\"\necho hi >&$f\necho oops >&2"),
            "hi\n"
        );
    }

    #[test]
    fn dup_target_nonnumeric_variable_on_fd2_is_ambiguous() {
        // On any fd other than 1 a non-numeric dup target is an ambiguous
        // redirect (an error), not a file.
        let (_o, s) = run("f=out.txt; echo hi 2>&$f");
        assert_eq!(s, 1);
    }

    /// Write `input` to a temp file, substitute its path for `{FILE}` in
    /// `src_tmpl`, run the script capturing stdout, and return `(stdout, status)`.
    /// Used by the input-fd-dup (`<&N`) tests.
    fn run_input_redirect(input: &str, src_tmpl: &str) -> (String, i32) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let uniq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "osh_in_{}_{}.txt",
            std::process::id(),
            uniq
        ));
        std::fs::write(&path, input).expect("write input");
        let p = path.to_string_lossy().replace('\\', "/");
        let src = src_tmpl.replace("{FILE}", &p);
        let mut sh = Shell::new();
        let mut buf = Vec::new();
        let prog = parse(&src).expect("parse");
        {
            let mut out = Out::Capture(&mut buf);
            sh.exec_program(&prog, &mut out, &StdinSrc::Inherit);
        }
        let _ = std::fs::remove_file(&path);
        (String::from_utf8_lossy(&buf).into_owned(), sh.last_status)
    }

    #[test]
    fn input_dup_reads_and_shares_offset() {
        // `read a <&3; read b <&3`: the input dup shares fd 3's offset, so the
        // two reads yield successive lines (regression: `<&3` used to be routed
        // as an *output* dup, mis-handling — and hanging — the input case).
        let (o, s) =
            run_input_redirect("L1\nL2\nL3\n", "exec 3<\"{FILE}\"\nread a <&3\nread b <&3\necho \"$a-$b\"");
        assert_eq!(s, 0);
        assert_eq!(o, "L1-L2\n");
    }

    #[test]
    fn input_dup_from_varfd_roundtrip() {
        // `exec {r}<file; read <&$r`: a varfd-allocated input descriptor can be
        // duplicated onto fd 0 for a plain `read`.
        let (o, s) = run_input_redirect(
            "first\nsecond\n",
            "exec {r}<\"{FILE}\"\nread a <&$r\nread b <&$r\necho \"$a|$b|r=$r\"",
        );
        assert_eq!(s, 0);
        assert_eq!(o, "first|second|r=10\n");
    }

    #[test]
    fn input_dup_unbound_fd_is_bad_descriptor() {
        // `read <&9` with fd 9 unbound fails the redirect (status 1), matching
        // bash's "Bad file descriptor" rather than silently reading EOF.
        let (_o, s) = run("read x <&9");
        assert_eq!(s, 1);
    }

    #[test]
    fn input_dup_of_fd0_is_noop() {
        // `<&0` duplicates fd 0 onto itself — a no-op that leaves the ambient
        // stdin (here a here-string) intact.
        let (o, s) = run_input_redirect("payload\n", "read x <\"{FILE}\" <&0; echo \"[$x]\"");
        assert_eq!(s, 0);
        assert_eq!(o, "[payload]\n");
    }

    #[test]
    fn dup_stdout_before_stderr_redirect() {
        // `echo x >&2 2>file`: bash applies redirects left to right, so `>&2`
        // duplicates fd 2 (the terminal) into fd 1 *before* `2>file` rebinds
        // fd 2. The echo therefore lands on the original stderr, not the file —
        // so the file must be empty. (Regression for TD-OILS14; the order-free
        // RedirPlan previously routed the echo into the file.)
        assert_eq!(run_exec_redirect("echo x >&2 2>\"{FILE}\""), "");
        // The reverse order `2>file >&2` binds fd 1 to the already-redirected
        // fd 2, so the echo *does* land in the file.
        assert_eq!(run_exec_redirect("echo x 2>\"{FILE}\" >&2"), "x\n");
        // Same for a compound command: the body's stdout goes through the
        // compound's `>&2` to the pre-redirect stderr, so the `2>file` stays
        // empty. (The body writes to fd 1; `echo body >&2` inside would instead
        // target fd 2 = the file, which is a different case.)
        assert_eq!(run_exec_redirect("{ echo body; } >&2 2>\"{FILE}\""), "");
    }

    #[test]
    fn exec_save_and_restore_stdout() {
        // The canonical save/restore idiom: `exec 3>&1` snapshots fd 1, then
        // `exec > file` redirects it. A `>&3` write bypasses the file (goes to
        // the saved original fd 1), and `exec 1>&3` restores fd 1 so later
        // output leaves the file too. The file must hold *only* the pre-restore,
        // non-`>&3` writes. (Regression for TD-OILS14 `exec 3>&1`/`exec 1>&3`.)
        assert_eq!(
            run_exec_redirect(
                "exec 3>&1\nexec > \"{FILE}\"\necho to-file\necho bypass >&3\nexec 1>&3\necho after-restore"
            ),
            "to-file\n"
        );
    }

    #[test]
    fn exec_swap_stdout_stderr() {
        // The classic swap idiom `exec 3>&1 1>&2 2>&3 3>&-` exchanges fd 1 and
        // fd 2. With fd 2 pre-pointed at the file, after the swap fd 1 is the
        // file and fd 2 is the terminal: `echo a` lands in the file while
        // `echo b >&2` does not. This exercises strict left-to-right ordering of
        // exec redirects (the collapsed RedirPlan could not express it).
        assert_eq!(
            run_exec_redirect(
                "exec 2> \"{FILE}\"\nexec 3>&1 1>&2 2>&3 3>&-\necho a\necho b >&2"
            ),
            "a\n"
        );
    }

    #[test]
    fn exec_input_redirect_persistent() {
        // `exec < file` rebinds fd 0 for every following command: successive
        // `read`s consume successive lines from the file.
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let uniq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "osh_exec_in_{}_{}.txt",
            std::process::id(),
            uniq
        ));
        std::fs::write(&path, b"line1\nline2\nline3\n").expect("write input");
        let p = path.to_string_lossy().replace('\\', "/");
        let src = format!(
            "exec < \"{p}\"\nread a\nread b\necho \"$a=$b\"\nread rest\necho \"$rest\""
        );
        let (out, status) = run(&src);
        let _ = std::fs::remove_file(&path);
        assert_eq!(status, 0);
        assert_eq!(out, "line1=line2\nline3\n");
    }

    #[test]
    fn exec_named_fd_read_u() {
        // `exec 3< file` opens a user-space descriptor; `read -u 3` consumes
        // successive lines from it, independently of fd 0.
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let uniq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "osh_exec_fd3_{}_{}.txt",
            std::process::id(),
            uniq
        ));
        std::fs::write(&path, b"alpha\nbeta\ngamma\n").expect("write input");
        let p = path.to_string_lossy().replace('\\', "/");
        let src = format!(
            "exec 3< \"{p}\"\nread -u 3 a\nread -u 3 b\necho \"$a-$b\"\nexec 3<&-"
        );
        let (out, status) = run(&src);
        let _ = std::fs::remove_file(&path);
        assert_eq!(status, 0);
        assert_eq!(out, "alpha-beta\n");
    }

    #[test]
    fn read_u_bad_fd_errors() {
        // `read -u 7` with no such open descriptor fails (status 1) without
        // touching the named variables.
        let (_out, status) = run("read -u 7 x; echo done");
        assert_eq!(status, 0); // the `echo done` sets the final status
    }

    #[test]
    fn exec_named_write_fd() {
        // `exec 3> file` opens a user-space write descriptor; `echo … >&3`
        // routes builtin stdout there, and successive writes accumulate.
        assert_eq!(
            run_exec_redirect(
                "exec 3> \"{FILE}\"\necho hi >&3\necho bye >&3\nexec 3>&-"
            ),
            "hi\nbye\n"
        );
        // `exec 3>> file` appends rather than truncating.
        assert_eq!(
            run_exec_redirect(
                "echo first > \"{FILE}\"\nexec 3>> \"{FILE}\"\necho second >&3\nexec 3>&-"
            ),
            "first\nsecond\n"
        );
    }

    #[test]
    fn write_fd_bad_descriptor_errors() {
        // `>&5` with no open write descriptor is a status-1 error and does not
        // reach the ambient stdout.
        let (out, status) = run("echo hi >&5");
        assert_eq!(status, 1);
        assert_eq!(out, "");
    }

    #[test]
    fn builtin_stderr_redirect_to_file() {
        // A simple-command builtin honors its own `2> file`: the `read` bad-fd
        // diagnostic lands in the file, not on the real stderr.
        let contents = run_exec_redirect("read -u 88 v 2> \"{FILE}\"");
        assert_eq!(contents, "osh: read: 88: bad file descriptor\n");
    }

    #[test]
    fn builtin_stderr_to_write_fd() {
        // `2>&3` on a builtin routes its stderr to a user-space write descriptor
        // opened by `exec 3> file` (TD-OILS14 builtin-stderr item).
        let contents = run_exec_redirect(
            "exec 3> \"{FILE}\"\nread -u 88 v 2>&3\nexec 3>&-",
        );
        assert_eq!(contents, "osh: read: 88: bad file descriptor\n");
    }

    #[test]
    fn builtin_stderr_to_stdout_capture() {
        // `2>&1` on a builtin folds its stderr into the (captured) stdout sink,
        // so a command substitution sees the diagnostic as stdout.
        let (out, _) = run("v=$(read -u 88 x 2>&1); echo \"[$v]\"");
        assert_eq!(out, "[osh: read: 88: bad file descriptor]\n");
    }

    #[test]
    fn function_invocation_stdout_redirect() {
        // `myfunc > file` applies the redirect to the whole function body: both
        // echoes land in the file, nothing on the caller's stdout.
        let contents = run_exec_redirect(
            "greet() { echo hello; echo world; }\ngreet > \"{FILE}\"",
        );
        assert_eq!(contents, "hello\nworld\n");
    }

    #[test]
    fn function_invocation_stderr_redirect() {
        // `myfunc 2> file` routes the body's diagnostics (a bad-fd `read`) to the
        // file, leaving the caller's stderr untouched.
        let contents = run_exec_redirect(
            "boom() { read -u 88 v; echo done; }\nboom 2> \"{FILE}\"",
        );
        assert_eq!(contents, "osh: read: 88: bad file descriptor\n");
    }

    #[test]
    fn function_invocation_stdin_redirect() {
        // `myfunc < file` feeds the file to the body's `read`, so the function
        // sees the redirected stdin.
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let uniq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "osh_fn_stdin_{}_{}.txt",
            std::process::id(),
            uniq
        ));
        std::fs::write(&path, b"redirected-line\n").expect("write input");
        let p = path.to_string_lossy().replace('\\', "/");
        let src = format!("f() {{ read x; echo \"got:$x\"; }}\nf < \"{p}\"");
        let (out, status) = run(&src);
        let _ = std::fs::remove_file(&path);
        assert_eq!(status, 0);
        assert_eq!(out, "got:redirected-line\n");
    }

    #[test]
    fn process_sub_input_redirect() {
        // `read x < <(printf hello)`: the input process substitution runs printf,
        // captures its output to a temp file, and the redirect feeds it to read.
        let (out, status) = run("read x < <(printf 'hello\\n'); echo \"$x\"");
        assert_eq!(status, 0);
        assert_eq!(out, "hello\n");
    }

    #[test]
    fn process_sub_input_as_source_arg() {
        // `. <(echo v=42)`: the substituted pathname is sourced, running the
        // captured script text (a variable assignment) in the current shell.
        let (out, _) = run(". <(echo 'v=42'); echo \"$v\"");
        assert_eq!(out, "42\n");
    }

    #[test]
    fn process_sub_two_inputs_distinct_files() {
        // `diff`-style `cmd <(a) <(b)` gives two *distinct* substituted paths.
        // Source both and confirm each captured its own command's output.
        let (out, _) = run(". <(echo 'a=1'); . <(echo 'b=2'); echo \"$a$b\"");
        assert_eq!(out, "12\n");
    }

    #[test]
    fn process_sub_output_deferred() {
        // `echo hello > >(read line; …)`: hello is written to the output process
        // substitution's temp file; after the command, its body runs with that
        // file as stdin, so `read line` sees "hello" and writes it to {FILE}.
        let contents =
            run_exec_redirect("echo hello > >(read line; echo \"$line\" > \"{FILE}\")");
        assert_eq!(contents, "hello\n");
    }

    #[test]
    fn scoped_compound_input_fd_read_u() {
        // `while read -u 3 …; done 3< file` reads the file via fd 3 while fd 0
        // stays free; fd 3 is scoped to the loop and gone afterward.
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let uniq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "osh_scoped_fd3_{}_{}.txt",
            std::process::id(),
            uniq
        ));
        std::fs::write(&path, b"a\nb\nc\n").expect("write input");
        let p = path.to_string_lossy().replace('\\', "/");
        // The loop prints each line; then fd 3 is closed, so a trailing
        // `read -u 3` fails (scoped, diagnostic to real stderr) — its non-zero
        // status is swallowed by the final `echo end`.
        let src = format!(
            "while read -u 3 x; do echo \"L:$x\"; done 3< \"{p}\"\nread -u 3 y; echo end"
        );
        let (out, status) = run(&src);
        let _ = std::fs::remove_file(&path);
        assert_eq!(status, 0);
        assert_eq!(out, "L:a\nL:b\nL:c\nend\n");
    }

    #[test]
    fn read_into_readonly_var_fails() {
        // `read x` where x is readonly leaves x unchanged and the read reports
        // status 1 (diagnostic goes to real stderr, not captured stdout).
        let (out, _) = run("readonly x=orig; read x <<< 'new'; echo \"$x $?\"");
        assert_eq!(out, "orig 1\n");
    }

    #[test]
    fn read_field_readonly_aborts_after_earlier_fields() {
        // `read a b` with b readonly: a is assigned, then b's field is rejected
        // and the read fails with status 1 (matching bash's field-order abort).
        let (out, _) = run("readonly b=keep; read a b <<< 'x y'; echo \"$a|$b|$?\"");
        assert_eq!(out, "x|keep|1\n");
    }

    #[test]
    fn read_array_readonly_fails() {
        // `read -a arr` with arr readonly rejects the whole read (no mutation).
        let (out, _) = run(
            "readonly arr; read -a arr <<< 'p q'; s=$?; echo \"[${arr[*]}]|$s\"",
        );
        assert_eq!(out, "[]|1\n");
    }

    #[test]
    fn env_prefix_readonly_var_errors() {
        // `readonly y; y=1 cmd` cannot temporarily override a readonly variable:
        // the command is not run and the status is 1, y keeps its value.
        let (out, _) = run("readonly y=5; y=9 :; echo \"$y|$?\"");
        assert_eq!(out, "5|1\n");
    }

    #[test]
    fn extra_write_fd_does_not_corrupt_stdout() {
        // A per-command `3> file` (fd ≥ 3, not via exec) must NOT redirect
        // stdout (regression: fd ≥ 3 formerly fell into the stdout arm, which
        // would have swallowed "hi"). The command word never writes to fd 3, so
        // stdout still receives "hi". Bash *does* open (create) the file for the
        // command's duration even though nothing is written to it, and osh now
        // matches that (the fd is materialised via `install_extra_fds`).
        let path = uniq_path("extrafd");
        let (out, status) = run(&format!("echo hi 3>{path}"));
        let created = std::path::Path::new(&path).exists();
        std::fs::remove_file(&path).ok();
        assert_eq!(status, 0);
        assert_eq!(out, "hi\n");
        // Bash parity: opening fd 3 creates the file even with no writes to it.
        assert!(created, "3>{path} should create the file (bash parity)");
    }

    #[test]
    fn amp_redirect_both_streams() {
        // `&>file` sends both stdout and stderr to the file. A group with a
        // normal echo and a `>&2` diagnostic both accumulate there, interleaved
        // in execution order (fd 1 and fd 2 share one live handle/offset — bash).
        assert_eq!(
            run_exec_redirect("{ echo out; echo err >&2; } &> \"{FILE}\""),
            "out\nerr\n"
        );
        // `&>>file` appends rather than truncating.
        assert_eq!(
            run_exec_redirect("echo a &> \"{FILE}\"\necho b &>> \"{FILE}\""),
            "a\nb\n"
        );
        // `>&file` (non-numeric target) is the same as `&>file`.
        assert_eq!(
            run_exec_redirect("{ echo x; echo y >&2; } >& \"{FILE}\""),
            "x\ny\n"
        );
        // A numeric `>&N` stays an fd duplication, not a file redirect.
        assert_eq!(
            run_exec_redirect("exec > \"{FILE}\"\necho hi >&1"),
            "hi\n"
        );
    }

    #[test]
    fn group_redirect_stdout_stderr_interleave() {
        // `{ …; } > f 2>&1`: fd 1 and fd 2 share one live file handle, so their
        // writes interleave in execution order (regression for the old model
        // that buffered stdout and folded it in after stderr — see the module
        // docs and TD-OILS-STDERR-INTERLEAVE).
        assert_eq!(
            run_exec_redirect("{ echo o; echo e >&2; echo o2; } > \"{FILE}\" 2>&1"),
            "o\ne\no2\n"
        );
    }

    #[test]
    fn for_loop_redirect_stdout_stderr_interleave() {
        // The same live-interleave holds across a loop body's iterations.
        assert_eq!(
            run_exec_redirect(
                "for i in 1 2; do echo o$i; echo e$i >&2; done > \"{FILE}\" 2>&1"
            ),
            "o1\ne1\no2\ne2\n"
        );
    }

    #[test]
    fn subshell_redirect_stdout_stderr_interleave() {
        // A `( … ) > f 2>&1` subshell clones `exec_stdout`/`exec_stderr` (not the
        // `stderr_stack`), so the scoped fd-1/fd-2 overrides are what route the
        // body's output to the file — and keep it interleaved.
        assert_eq!(
            run_exec_redirect("( echo o; echo e >&2 ) > \"{FILE}\" 2>&1"),
            "o\ne\n"
        );
    }

    #[test]
    fn subshell_stderr_only_redirect_reaches_file() {
        // `( … ) 2> f`: the subshell's `>&2` write must reach the group's file
        // (via the cloned `exec_stderr`), not leak to the real terminal.
        assert_eq!(
            run_exec_redirect("( echo keep; echo diag >&2 ) 2> \"{FILE}\""),
            "diag\n"
        );
    }

    #[test]
    fn subshell_2to1_into_capture_folds_stderr() {
        // `$( ( … ) 2>&1 )`: a subshell body under `2>&1` inside a command
        // substitution must fold its `>&2` output into the captured stdout, not
        // leak it to the real stderr. Regression: the subshell clone reset
        // `stderr_stack`, so the `2>&1` Buffer target was lost and `>&2` escaped.
        // (Line-order interleaving of the two streams is a separate, documented
        // limitation — stdout goes live, stderr is folded after — so this only
        // asserts both streams are present in the capture.)
        let (o, s) = run(r#"x=$( ( echo out; echo err >&2 ) 2>&1 ); echo "[$x]""#);
        assert_eq!(s, 0);
        assert!(o.contains("out"), "stdout missing: {o:?}");
        assert!(o.contains("err"), "stderr not folded into capture: {o:?}");
    }

    #[test]
    fn nested_subshell_2to1_into_capture_folds_stderr() {
        // The fix must survive nesting: `$( ( ( … >&2 ) 2>&1 ) )` — the inner
        // subshell's stderr propagates out through both subshell layers into the
        // command-substitution capture.
        let (o, s) = run(r#"y=$( ( ( echo deep >&2 ) 2>&1 ) ); echo "[$y]""#);
        assert_eq!(s, 0);
        assert_eq!(o, "[deep]\n");
    }

    #[test]
    fn stderr_then_stdout_redirect_order_keeps_stderr_on_prior_sink() {
        // `2>&1 > f`: the `2>&1` copies fd 1's sink *before* `> f` rebinds it, so
        // fd 2 stays on the pre-redirect stdout (here the capture, not the file)
        // and only fd 1's output reaches the file. Verifies the override does not
        // wrongly drag fd 2 along to the file.
        assert_eq!(
            run_exec_redirect("{ echo o; echo e >&2; } 2>&1 > \"{FILE}\""),
            "o\n"
        );
    }

    #[test]
    fn compound_stdout_alias_fd_routes_to_target() {
        // `{ …; } 1>&N` (N ≥ 3) on a compound command must dup fd 1 onto fd N's
        // target for the body — the body's stdout lands in fd 3's file, not the
        // ambient stdout. Regression: `exec_with_redirects` ignored
        // `stdout_to_fd`, so the body wrote to the inherited stdout instead.
        assert_eq!(
            run_exec_redirect("exec 3> \"{FILE}\"\n{ echo hi; echo two; } 1>&3\nexec 3>&-"),
            "hi\ntwo\n"
        );
    }

    #[test]
    fn compound_stderr_alias_fd_routes_to_target() {
        // `{ …; } 2>&N`: fd 2 dup'd onto fd N's target for the body. A `>&2`
        // write inside the group reaches fd 3's file.
        assert_eq!(
            run_exec_redirect("exec 3> \"{FILE}\"\n{ echo err >&2; } 2>&3\nexec 3>&-"),
            "err\n"
        );
    }

    #[test]
    fn subshell_stderr_alias_fd_routes_to_target() {
        // The `2>&N` alias sets a scoped `exec_stderr`, so a *subshell* body
        // (which inherits `exec_stderr` but not `stderr_stack`) also reaches
        // fd N's target — matching bash fd inheritance.
        assert_eq!(
            run_exec_redirect("exec 3> \"{FILE}\"\n( echo err >&2 ) 2>&3\nexec 3>&-"),
            "err\n"
        );
    }

    #[test]
    fn compound_stdout_alias_to_pipe_reaches_downstream() {
        // `{ …; } 3>&1 | downstream` where fd 1 is the OS pipe to the next stage:
        // a `>&3` write inside the body must land in the pipe (reaching the
        // downstream stage), not leak to the ambient terminal. Regression:
        // `AliasStd(1)` snapshotted the persistent/real fd 1 instead of the
        // stage's `Out::Pipe`.
        let (o, s) = run("{ echo x >&3; } 3>&1 | cat");
        assert_eq!(s, 0);
        assert_eq!(o, "x\n");
    }

    #[test]
    fn compound_stdout_alias_unbound_fd_fails() {
        // `{ …; } 1>&N` with N unbound is a status-1 "Bad file descriptor" and
        // the body does not run (matching bash).
        let (o, s) = run("{ echo hi; } 1>&9; echo done");
        assert_eq!(s, 0); // final `echo done` status
        assert!(!o.contains("hi"), "body ran despite bad fd: {o:?}");
        assert!(o.contains("done"));
    }

    #[test]
    fn redirect_two_independent_opens_same_file_clobber() {
        // `>f 2>f`: fd 1 and fd 2 are TWO independent truncating opens of the
        // same path, so each has its own offset. Both writers start at 0 →
        // last-writer-wins (bash: the file ends up with only fd 2's "err").
        // Regression: osh's `same_as_stdout` shortcut cloned fd 1's handle for
        // fd 2, sharing one offset and interleaving to "out\nerr".
        assert_eq!(
            run_exec_redirect("{ echo out; echo err >&2; } > \"{FILE}\" 2> \"{FILE}\""),
            "err\n"
        );
    }

    #[test]
    fn redirect_dup_shares_one_open_interleaves() {
        // `>f 2>&1`: fd 2 is dup'd from fd 1, so they SHARE one open file
        // description (one offset) and both writes interleave. Contrast with the
        // two-independent-opens case above. `stderr_shares_stdout` must keep this
        // sharing intact.
        assert_eq!(
            run_exec_redirect("{ echo out; echo err >&2; } > \"{FILE}\" 2>&1"),
            "out\nerr\n"
        );
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
